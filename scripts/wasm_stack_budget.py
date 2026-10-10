#!/usr/bin/env python3
"""wasm_stack_budget.py - check the wasm32 stack budget (`nest_cost`) is sound.

Usage: wasm_stack_budget.py <interp.rs> <debug|release> <wasm-objdump -x> \
           <wasm-objdump -d> <objdump -d of the wasmtime-compiled module>

On wasm32 every interpreter function that carries a `nest_guard!` charges
its `nest_cost` constant against a budget of `max_depth x NEST_PER_DEPTH`
bytes (interp.rs, compilebench AX-56). The budget bounds the native stack
only if every recursion through the interpreter passes a guarded function,
and each guarded function's constant covers its own frame plus every
unguarded frame that can sit above it before the next guarded one.

This script checks exactly that on the build under test:

- the call graph is the module's direct `call`s (wasm-objdump -d); it also
  checks that adding every `call_indirect` -> same-type table function edge
  leaves each checked component unchanged, so no recursion runs through a
  trait object or fn pointer;
- the frames are the Cranelift x86-64 frames wasmtime gives each function
  (`sub $N,%rsp` + return address + frame pointer);
- the checked components are the strongly connected component holding
  `Interp::eval` and every other recursive component reachable from it
  that holds a guarded function (the bytecode compiler's `Compiler::expr`
  and `stmt`, which recurse once per level of source nesting);
- a guarded function reachable from `eval` but in no checked component
  (`Interp::compile_body`, `compile_lambda`: a frame live while one
  compile runs) is checked the same way, its unguarded tail stopping at
  checked components and at the recursions ALLOW classifies;
- every other recursive component reachable from `eval` is unguarded, so
  ALLOW must classify it by what its depth follows: a run-time value
  (compilebench AX-59), one source construct (compilebench AX-60, AX-61),
  a fixed bound, a std algorithm's log n, or the panic path. Work outside
  those runs above the last guarded frame, in the headroom the budget leaves
  below the stack size; value and source depth are not bounded by it and can
  still trap;
- the parser (compilebench AX-60): every recursive component reachable
  from `Parser::parse_program` either holds a call that charges one
  `MAX_EXPR_DEPTH` unit (PARSER_CHARGES), and the wasm32 limit times the
  worst frame chain between two charges fits the budget, or holds no
  parser method and is one ALLOW classifies.

It fails when a `nest_cost` constant is not charged by exactly one guard
site in the function it is named after, when indirect calls widen a checked
component, when the unguarded part of one has a cycle (a recursion no guard
sees), when a constant is smaller than the guarded function's frame plus the
deepest unguarded chain it can call, when a recursion reachable from `eval`
is neither checked nor in ALLOW, when a cycle of `Value` drop glue avoids
`drop_bounded`, or when a recursion reachable from `parse_program` with no
charged call holds a parser method or is not in ALLOW. Exit 0 ok, 1
failure, 2 bad input.
"""
import os
import re
import sys
from collections import defaultdict


# Unguarded recursions reachable from `eval`, each with why its depth is not
# the program's call depth: (regex over a member's symbol, kind). First
# match wins. "bounded (DROP_INLINE_DEPTH)": the drop glue of a nested
# `Value`, which on wasm32 passes `bounded_drop` (interp.rs, compilebench
# AX-59) and so holds at most `DROP_INLINE_DEPTH` levels of the component
# at once; checked below: without its `drop_bounded` instances the
# component must be acyclic, and DROP_INLINE_DEPTH passes of it must fit
# the headroom the budget leaves; a `Value` drop component without
# `bounded_drop` fails. "value": the depth
# of a run-time value or string (a nested array, an `Uncertain` chain,
# JSON, a regex) walked by something other than drop, outside the budget.
# "source": the nesting of one source construct (a pattern, a type, an
# expression walked whole), walked by the same function under both
# engines, at most the parser's wasm32 `MAX_EXPR_DEPTH` deep (compilebench
# AX-60); `parser::axon_type_to_str` renders one parsed `AxonType`, each
# level of which a charged `parse_type_atom` built. "lexer": the
# logos-generated `Token::lex` (the parser re-lexes each interpolation
# slot). On release its one cycle is logos's skip re-entry (`lex.trivia();
# Token::lex(lex)` after a skipped match), at most two deep: a whitespace
# run is matched whole and a `//` comment runs to the newline, which is a
# token. On debug its states also call themselves once per character of
# one token, compilebench AX-65, out of scope (R50 §4 S12). "bounded":
# a fixed depth (`PURE_DEPTH`; a numeric helper that recurses once). "std":
# a std algorithm's log-n recursion. "panic": the panic path, which ends
# the run.
DROP_CLASS = "bounded (DROP_INLINE_DEPTH)"
ALLOW = [
    (r"bounded_drop::|12bounded_drop", DROP_CLASS),
    (r"interp::Value as .*Clone|6interp5Value|Rc<interp::EnumVal>|interp::SendValue|9SendValue"
     r"|send_value_display|value::display|fields_display|values_equal|Fields>::equal"
     r"|numeric_score|eval_binop_vals", "value"),
    (r"serde_json|10serde_json|serde_core", "value"),
    (r"interp::regex|6interp5regex", "value"),
    (r"ast::(Expr|Pattern|AxonType|FmtPart|HandlerExpr|MatchArm)|3ast\d+[A-Z]|3ast4Ex"
     r"|types::Type|5types4Type|resolver::collect|match_pattern|pattern_binds"
     r"|parser::axon_type_to_str|6parser16axon_type_to_str", "source"),
    (r"token::Token as logos\S*::Logos>::lex|5Token\w*?5logos5Logos3lex", "lexer"),
    (r"vm::pure::|2vm4pure4Pure|compile_pure_(at|stmts|if)\b", "bounded (PURE_DEPTH)"),
    (r"gamma_sample|log_gamma|reg_inc_beta|beta_cdf|sub_timespec|slice_error_fail", "bounded (recurses once)"),
    (r"slice4sort|btree", "std"),
    (r"backtrace|panicking|ThreadId>::new::exhausted", "panic"),
]
# The drop glue of an interpreter `Value` (the shared `EnumVal` cell, or the
# `Value` enum itself): a component holding it must hold `bounded_drop`.
VALUE_DROP = re.compile(r"Rc<interp::EnumVal>>::drop_slow|drop_glue::<interp::Value>"
                        r"|drop_glue\w*?6interp5ValueE")
# An instance of `bounded_drop::drop_bounded`, the function every cycle of
# `Value` drop glue must pass.
DROP_BOUNDED = re.compile(r"bounded_drop::drop_bounded\b|12bounded_drop12drop_bounded")
# wasmtime's default `max-wasm-stack`, which the wasm32 budget is sized for.
WASM_STACK = 512 * 1024
# A parser method's name, and the calls that charge one `MAX_EXPR_DEPTH`
# unit (`Parser::nested`, inlined into its caller): (caller, callee), a
# `None` caller meaning any. parser.rs keeps each callee out of line on
# wasm32 so it is nameable here.
PARSER_FN = re.compile(r"^<axon_core\[[0-9a-f]+\]::parser::Parser>::(\w+)$")
PARSER_CHARGES = [
    (None, "parse_expr"),
    (None, "parse_primary"),
    (None, "parse_pattern"),
    (None, "parse_type_atom"),
    (None, "parse_attr_atom"),
    ("parse_if", "parse_if"),
    ("parse_match|parse_match_operand", "parse_logical"),
]


def find_cycle(nodes, calls):
    """A cycle of direct calls among `nodes` (its functions in call order),
    or [] when the subgraph is acyclic. Iterative DFS."""
    color = {}
    for v0 in sorted(nodes):
        if v0 in color:
            continue
        color[v0] = 1
        path, work = [v0], [iter(sorted(g for g in calls.get(v0, ()) if g in nodes))]
        while work:
            w = next(work[-1], None)
            if w is None:
                color[path.pop()] = 2
                work.pop()
            elif color.get(w) == 1:
                return path[path.index(w):]
            elif w not in color:
                color[w] = 1
                path.append(w)
                work.append(iter(sorted(g for g in calls.get(w, ()) if g in nodes)))
    return []


def main() -> int:
    if len(sys.argv) != 6:
        print(__doc__.split("\n\n")[0], file=sys.stderr)
        return 2
    src, profile, wasm_x, wasm_d, cdis = sys.argv[1:]
    text = open(src, encoding="utf-8").read()

    def const(pat, where, txt):
        m = re.search(pat, txt, re.S)
        if not m:
            print(f"wasm_stack_budget: no {where}", file=sys.stderr)
            return None
        return int(m.group(1).replace("_", ""))

    # The budget the guards charge against (`RECURSION_LIMIT` x
    # `NEST_PER_DEPTH` on wasm32), the headroom it leaves below the stack,
    # the bounded drop's inline depth, and the parser's wasm32 limit.
    parser_src = open(os.path.join(os.path.dirname(os.path.abspath(src)), "parser.rs"), encoding="utf-8").read()
    limit = const(r'#\[cfg\(target_arch = "wasm32"\)\]\s*const RECURSION_LIMIT: usize = ([\d_]+);',
                  "wasm32 `RECURSION_LIMIT` in interp.rs", text)
    per_depth = const(r"const NEST_PER_DEPTH: usize = ([\d_]+);", "`NEST_PER_DEPTH` in interp.rs", text)
    drop_depth = const(r"const DROP_INLINE_DEPTH: usize = ([\d_]+);", "`DROP_INLINE_DEPTH` in interp.rs", text)
    expr_depth = const(r'#\[cfg\(target_arch = "wasm32"\)\]\s*const MAX_EXPR_DEPTH: usize = ([\d_]+);',
                       "wasm32 `MAX_EXPR_DEPTH` in parser.rs", parser_src)
    if None in (limit, per_depth, drop_depth, expr_depth):
        return 2
    budget = limit * per_depth
    headroom = WASM_STACK - budget
    body = re.search(r"pub\(super\) mod nest_cost \{(.*?)\n\}", text, re.S)
    if not body:
        print("wasm_stack_budget: no `mod nest_cost` in interp.rs", file=sys.stderr)
        return 2
    costs = {
        n.lower(): int(d if profile == "debug" else r)
        for n, d, r in re.findall(r"pub const (\w+): usize = b\((\d+), (\d+)\);", body.group(1))
    }
    # `pub const A: usize = B + C;` (one or more terms): the function named
    # after A charges the sum, which must cover A's own frame and tail.
    alias = {
        a.lower(): [t.lower() for t in ts.split(" + ")]
        for a, ts in re.findall(r"pub const (\w+): usize = ([A-Z_]+(?: \+ [A-Z_]+)*);", body.group(1))
    }
    for a, ts in alias.items():
        for t in ts:
            if t not in costs:
                print(f"wasm_stack_budget: alias {a.upper()} names no `b(..)` constant {t.upper()}", file=sys.stderr)
                return 2
        costs[a] = sum(costs[t] for t in ts)
    if not costs:
        print("wasm_stack_budget: no constants in `mod nest_cost`", file=sys.stderr)
        return 2
    # A constant counts only if its function really charges it: every
    # `nest_guard!(self, X…)` / `nest::<{…nest_cost::X}>` site under src/ must
    # sit in the function named after X (`Compiler::<f>` for COMPILE_<F>), and
    # every constant, aliases included, needs exactly one such site.
    sites = defaultdict(list)
    fn_re = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+)?(?:unsafe\s+)?fn\s+(\w+)")
    site_re = re.compile(r"nest_guard!\(\s*self\s*,\s*([A-Z_]+)\b|nest_cost::([A-Z_]+)\s*\}>")
    root_dir = os.path.dirname(os.path.abspath(src))
    for dirpath, _, files in os.walk(root_dir):
        for fname in sorted(files):
            if not fname.endswith(".rs"):
                continue
            path = os.path.join(dirpath, fname)
            enclosing = None
            for lineno, line in enumerate(open(path, encoding="utf-8"), 1):
                m = fn_re.match(line)
                if m:
                    enclosing = m.group(1)
                if line.lstrip().startswith("//"):
                    continue
                for m in site_re.finditer(line):
                    const = (m.group(1) or m.group(2)).lower()
                    fn = enclosing if m.group(1) else f"compile_{enclosing}"
                    sites[const].append((fn, f"{os.path.relpath(path, root_dir)}:{lineno}"))
    bad_sites = 0
    for c in sorted(set(costs) | set(sites)):
        where = sites.get(c, [])
        if c not in costs:
            problem = "names no constant in `mod nest_cost`"
        elif len(where) != 1:
            problem = f"has {len(where)} guard sites, not 1"
        elif where[0][0] != c:
            problem = f"is charged in `{where[0][0]}`, not `{c}`"
        else:
            continue
        bad_sites += 1
        print(f"wasm_stack_budget: {profile:<7} FAIL guard {c.upper()} {problem}"
              + (f" ({', '.join(w for _, w in where)})" if where else ""))
    if bad_sites:
        return 1

    names, sig, table = {}, {}, set()
    for line in open(wasm_x, encoding="utf-8", errors="replace"):
        m = re.match(r"^ - func\[(\d+)\] sig=(\d+) <(.*)>", line)
        if m:
            names[int(m.group(1))] = m.group(3)
            sig[int(m.group(1))] = int(m.group(2))
            continue
        m = re.match(r"^\s+- elem\[\d+\] = ref\.func:(\d+)", line)
        if m:
            table.add(int(m.group(1)))
    calls = defaultdict(set)
    indirect = defaultdict(set)
    cur = None
    for line in open(wasm_d, encoding="utf-8", errors="replace"):
        m = re.match(r"^[0-9a-f]+ func\[(\d+)\] <", line)
        if m:
            cur = int(m.group(1))
            continue
        if cur is None:
            continue
        m = re.search(r"\|\s+call (\d+) <", line)
        if m:
            calls[cur].add(int(m.group(1)))
            continue
        m = re.search(r"\|\s+call_indirect \d+ \(type (\d+)\)", line)
        if m:
            indirect[cur].add(int(m.group(1)))
    if not table or not indirect:
        print("wasm_stack_budget: no element table or call_indirect parsed", file=sys.stderr)
        return 2

    head = re.compile(r"^[0-9a-f]+ <wasm\[0\]::function\[(\d+)\]::(.*)>:$")
    sub = re.compile(r"\bsub\s+\$0x([0-9a-f]+),%rsp")
    frame, sym = {}, {}
    cur, left = None, 0
    for line in open(cdis, encoding="utf-8", errors="replace"):
        if line[:1] not in (" ", "\n", ""):
            cur = None
            h = head.match(line.rstrip("\n"))
            if h:
                cur, left = int(h.group(1)), 16
                sym[cur] = h.group(2)
                frame[cur] = 16  # return address + frame pointer
            continue
        if cur is None or left == 0:
            continue
        s = sub.search(line)
        if s:
            frame[cur] = int(s.group(1), 16) + 16
            left = 0
            continue
        left -= 1

    # `Interp::<f>` maps to the constant `<F>`; the bytecode compiler's
    # `Compiler::<f>` (vm/compile.rs) to `COMPILE_<F>`.
    dem = re.compile(
        r"^<axon_core\[[0-9a-f]+\]::interp::(?:vm::compile::(Compiler)|Interp)>::(\w+)(?:::<[^{]*>)?$"
    )
    mang = re.compile(r"^_RI?NvM\w*?(?:6Interp|8(Compiler))(\d+)(\w+)")

    def name_of(f):
        s = sym.get(f, "")
        m = dem.match(s)
        if m:
            n = m.group(2)
        else:
            m = mang.match(s)
            if not (m and len(m.group(3)) >= int(m.group(2))):
                return None
            n = m.group(3)[: int(m.group(2))]
        return "compile_" + n if m.group(1) else n

    # Tarjan, iterative.
    nodes = set(calls) | set(frame)
    index, low, onst, st, comp, idx = {}, {}, set(), [], {}, 0
    for v0 in sorted(nodes):
        if v0 in index:
            continue
        index[v0] = low[v0] = idx
        idx += 1
        st.append(v0)
        onst.add(v0)
        work = [(v0, iter(sorted(calls.get(v0, ()))))]
        while work:
            u, it = work[-1]
            w = next(it, None)
            if w is not None:
                if w not in index:
                    index[w] = low[w] = idx
                    idx += 1
                    st.append(w)
                    onst.add(w)
                    work.append((w, iter(sorted(calls.get(w, ())))))
                elif w in onst:
                    low[u] = min(low[u], index[w])
                continue
            work.pop()
            if work:
                low[work[-1][0]] = min(low[work[-1][0]], low[u])
            if low[u] == index[u]:
                while True:
                    x = st.pop()
                    onst.discard(x)
                    comp[x] = u
                    if x == u:
                        break

    evals = [f for f in sym if name_of(f) == "eval"]
    if not evals:
        print("wasm_stack_budget: no `Interp::eval` in the disassembly", file=sys.stderr)
        return 2
    members = defaultdict(set)
    for f, r in comp.items():
        members[r].add(f)

    def reach(start, edges):
        seen, todo = set(start), list(start)
        while todo:
            for g in edges.get(todo.pop(), ()):
                if g not in seen:
                    seen.add(g)
                    todo.append(g)
        return seen

    # The checked components: the one through `eval`, and every other
    # recursive component reachable from it that holds a guarded function
    # (the bytecode compiler's `Compiler::expr`/`stmt`, which recurse over
    # source nesting without coming back to `eval`).
    root = comp[evals[0]]
    roots = [root] + sorted(
        {comp[f] for f in reach(evals[:1], calls) if name_of(f) in costs and comp[f] != root}
    )
    roots = [r for r in roots if len(members[r]) > 1 or r in calls.get(r, ())]

    # Each component uses direct calls only. A `call_indirect` can reach
    # any table function of its type, so add those edges (over-
    # approximating the targets) and require every checked component to
    # stay the same: no recursion runs through a vtable or fn pointer.
    # The program entry is left out of the targets: its only address-taken
    # use is the std runtime start, outside any recursion.
    by_sig = defaultdict(set)
    for f in table:
        if not names.get(f, "").endswith("8axon_run4main"):
            by_sig[sig.get(f)].add(f)
    full = {f: set(calls[f]).union(*(by_sig[t] for t in indirect[f])) for f in set(calls) | set(indirect)}
    back = defaultdict(set)
    for f, gs in full.items():
        for g in gs:
            back[g].add(f)

    def label(f):
        n = name_of(f)
        if n:
            return n
        s = sym.get(f) or names.get(f, str(f))
        s = re.sub(r"<axon_core\[[0-9a-f]+\]::interp::Interp>::", "", s)
        return re.sub(r"axon_core\[[0-9a-f]+\]::", "", s)[:100]

    bad = 0
    need, worst, summary = defaultdict(int), {}, []
    for r in roots:
        scc = members[r]
        guarded = {f for f in scc if name_of(f) in costs}
        unguarded = scc - guarded
        what = "eval" if r == root else label(min(guarded, key=label))
        start = sorted(scc)[:1]
        wide = reach(start, full) & reach(start, back)
        if wide != scc:
            extra = [names.get(f, str(f))[:100] for f in sorted(wide - scc)[:8]]
            print(f"wasm_stack_budget: {profile:<7} FAIL  indirect calls add {len(wide - scc)} functions "
                  f"to the recursion through {what}: {extra}")
            return 1
        tail, via, state = {}, {}, {}

        def tail_of(f):
            # Deepest stack an unguarded chain starting at f holds before it
            # calls a guarded function (or returns).
            nonlocal bad
            if f in tail:
                return tail[f]
            if state.get(f) == 1:
                bad += 1
                print(f"wasm_stack_budget: {profile:<7} FAIL unguarded recursion through {label(f)}")
                return 0
            state[f] = 1
            best, arg = 0, None
            for g in calls.get(f, ()):
                if g in unguarded:
                    t = tail_of(g)
                    if t > best:
                        best, arg = t, g
            state[f] = 2
            tail[f], via[f] = frame.get(f, 16) + best, arg
            return tail[f]

        if not guarded:
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL no guarded function in the recursion through {what}")
        for f in sorted(guarded):
            best, arg = 0, None
            for g in calls.get(f, ()):
                if g in unguarded:
                    t = tail_of(g)
                    if t > best:
                        best, arg = t, g
            n = name_of(f)
            if frame[f] + best > need[n]:
                need[n] = frame[f] + best
                chain, g = [], arg
                while g is not None:
                    chain.append(f"{label(g)} {frame.get(g, 16)}")
                    g = via.get(g)
                worst[n] = (frame[f], chain)
        summary.append(f"wasm_stack_budget: {profile:<7} cycle through {what}: {len(scc)} functions, "
                       f"{len(guarded)} guarded, {len(unguarded)} unguarded")
    # A guarded function reachable from `eval` outside every checked
    # component still charges its constant while its own frame and
    # unguarded callees are live, so the same rule applies. Its tail stops
    # at checked components (charged by their own guards) and at other
    # recursive components (classified by ALLOW below), so it is acyclic.
    in_root = {f for r in roots for f in members[r]}
    cyclic = {r for r, ms in members.items() if len(ms) > 1 or r in calls.get(r, ())}
    flat = {}

    def flat_tail(f):
        if f not in flat:
            best, arg = 0, None
            for g in calls.get(f, ()):
                if g in in_root or name_of(g) in costs or comp.get(g) in cyclic:
                    continue
                t = flat_tail(g)[0]
                if t > best:
                    best, arg = t, g
            flat[f] = (frame.get(f, 16) + best, arg)
        return flat[f]

    for f in sorted(reach(evals[:1], calls) - in_root):
        n = name_of(f)
        if n not in costs or comp.get(f) in cyclic:
            continue
        best, arg = 0, None
        for g in calls.get(f, ()):
            if g in in_root or name_of(g) in costs or comp.get(g) in cyclic:
                continue
            t = flat_tail(g)[0]
            if t > best:
                best, arg = t, g
        if frame[f] + best > need[n]:
            need[n] = frame[f] + best
            chain, g = [], arg
            while g is not None:
                chain.append(f"{label(g)} {frame.get(g, 16)}")
                g = flat[g][1]
            worst[n] = (frame[f], chain)
    for n, c in sorted(costs.items()):
        if n not in need:
            print(f"wasm_stack_budget: {profile:<7} {n:<24} no instance (inlined in this profile)")
            continue
        ok = need[n] <= c
        bad += not ok
        own, chain = worst[n]
        detail = f"frame {own}" + (" + " + " + ".join(chain) if chain else "")
        print(f"wasm_stack_budget: {profile:<7} {n:<24} {need[n]:>5} <= {c:<5} "
              + ("ok" if ok else "FAIL") + f"  ({detail})")

    # Every other recursion reachable from `eval` is unguarded, so it must
    # be one ALLOW names, with the reason its depth is not the program's
    # call depth.
    kinds = defaultdict(int)
    for r in sorted({comp[f] for f in reach(evals[:1], calls)} - set(roots)):
        ms = members[r]
        if len(ms) == 1 and r not in calls.get(r, ()):
            continue
        full_names = [re.sub(r"axon_core\[[0-9a-f]+\]::", "", name_of(f) or sym.get(f) or names.get(f, str(f)))
                      for f in ms]
        why = next((w for p, w in ALLOW if any(re.search(p, s) for s in full_names)), None)
        if why != DROP_CLASS and any(VALUE_DROP.search(s) for s in full_names):
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL Value drop recursion without `bounded_drop`: "
                  f"{sorted(label(f) for f in ms)[:4]}")
            continue
        if why is None:
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL unguarded recursion not in ALLOW: "
                  f"{sorted(label(f) for f in ms)[:4]}")
            continue
        kinds[why] += 1
        if why == DROP_CLASS:
            # Only `drop_bounded` counts the drops live and defers past
            # DROP_INLINE_DEPTH, so every cycle of drop glue must pass one of
            # its instances: without them the component must be acyclic. A
            # cycle that avoids it (a container whose `Drop` does not hand
            # its contents to `drop_bounded`, or whose glue still drops them)
            # recurses once per level of the value and fails.
            gate = {f for f in ms
                    if any(DROP_BOUNDED.search(s) for s in (sym.get(f, ""), names.get(f, "")))}
            loop = find_cycle(ms - gate, calls)
            if not gate or loop:
                bad += 1
                print(f"wasm_stack_budget: {profile:<7} FAIL Value drop cycle avoiding `drop_bounded`: "
                      + (" -> ".join(label(f) for f in loop + loop[:1]) if loop else "no `drop_bounded`"))
                continue
            # At most DROP_INLINE_DEPTH passes through the component sit on
            # the stack, each at most the component's summed frame, above
            # the budget.
            total = sum(frame.get(f, 16) for f in ms)
            ok = drop_depth * total <= headroom
            bad += not ok
            print(f"wasm_stack_budget: {profile:<7} {'value drop':<24} {drop_depth * total:>5} <= {headroom:<5} "
                  + ("ok" if ok else "FAIL")
                  + f"  (DROP_INLINE_DEPTH {drop_depth} x {len(ms)} functions, {total} B; "
                  + f"acyclic without its {len(gate)} `drop_bounded`)")

    # The parser runs before the budget is charged, so its recursion must fit
    # the budget by itself: between two calls that charge one
    # `MAX_EXPR_DEPTH` unit (`nested`) the stack holds at most the worst
    # chain of frames from one charged callee to the next charged call, and
    # the wasm32 limit times that chain fits the budget.
    pname = {f: m.group(1) for f, s in sym.items() if (m := PARSER_FN.match(s))}
    # A PARSER_CHARGES entry counts only if parser.rs really charges it:
    # `fn <e>` runs `self.nested(Self::<e>_inner)` (a `None` caller), or a
    # function the caller pattern names runs `self.nested(Self::<e>)`. And
    # every `nested` site has its entry.
    psites, enclosing = set(), None
    for line in parser_src.splitlines():
        m = fn_re.match(line)
        if m:
            enclosing = m.group(1)
        if line.lstrip().startswith("//"):
            continue
        for m in re.finditer(r"self\.nested\(Self::(\w+)\)", line):
            e = m.group(1)
            psites.add((None, enclosing) if e == f"{enclosing}_inner" else (enclosing, e))

    def site_of(r, e, sr, se):
        return e == se and (r is None) == (sr is None) and (r is None or re.fullmatch(r, sr))

    charges = []
    for r, e in PARSER_CHARGES:
        if any(site_of(r, e, sr, se) for sr, se in psites):
            charges.append((r, e))
        else:
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL parser charge ({r}, {e}) has no `nested` site in parser.rs")
    for sr, se in sorted(psites, key=str):
        if not any(site_of(r, e, sr, se) for r, e in PARSER_CHARGES):
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL `nested` site ({sr}, {se}) in parser.rs not in PARSER_CHARGES")

    def charged(f, g):
        callee = pname.get(g)
        return callee is not None and any(
            callee == e and (r is None or re.fullmatch(r, pname.get(f, ""))) for r, e in charges)

    heads = {g for f, gs in calls.items() for g in gs if charged(f, g)}
    if not any(pname.get(g) == "parse_expr" for g in heads):
        print("wasm_stack_budget: no charged `Parser::parse_expr` call in the disassembly", file=sys.stderr)
        return 2
    starts = [f for f in sym if pname.get(f) == "parse_program"]
    if not starts:
        print("wasm_stack_budget: no `Parser::parse_program` in the disassembly", file=sys.stderr)
        return 2
    pscc = set().union(*(members[comp[f]] for f in heads))
    # Every recursion reachable from `parse_program` must pass a charged
    # call (checked by `unit_tail` below), or be one ALLOW names that holds
    # no parser method: a parser recursion with no charge is bounded by
    # nothing, so the wasm32 front end traps on its nesting.
    pkinds = defaultdict(int)
    for r in sorted({comp[f] for f in reach(starts, calls)}):
        ms = members[r]
        if len(ms) == 1 and r not in calls.get(r, ()):
            continue
        if any(charged(f, g) for f in ms for g in calls.get(f, ()) if g in ms):
            pscc |= ms
            continue
        full_names = [re.sub(r"axon_core\[[0-9a-f]+\]::", "", name_of(f) or sym.get(f) or names.get(f, str(f)))
                      for f in ms]
        why = next((w for p, w in ALLOW if any(re.search(p, s) for s in full_names)), None)
        if why is None or any(f in pname for f in ms):
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL parser recursion without a charge: "
                  f"{sorted(label(f).replace('<parser::Parser>::', '') for f in ms)[:4]}")
            continue
        pkinds[why] += 1
    unit_of, pstate = {}, {}

    def unit_tail(f):
        # Deepest frames from entering f to its next charged call.
        nonlocal bad
        if f in unit_of:
            return unit_of[f][0]
        if pstate.get(f) == 1:
            bad += 1
            print(f"wasm_stack_budget: {profile:<7} FAIL parser recursion without a charge through {label(f)}")
            return 0
        pstate[f] = 1
        best, arg = 0, None
        for g in sorted(calls.get(f, ())):
            if g in pscc and not charged(f, g):
                t = unit_tail(g)
                if t > best:
                    best, arg = t, g
        pstate[f] = 2
        unit_of[f] = (frame.get(f, 16) + best, arg)
        return unit_of[f][0]

    unit, worst_fn = 0, None
    for f in sorted(heads & pscc):
        u = unit_tail(f)
        if u > unit:
            unit, worst_fn = u, f
    chain, g = [], worst_fn
    while g is not None:
        chain.append(f"{label(g).replace('<parser::Parser>::', '')} {frame.get(g, 16)}")
        g = unit_of[g][1]
    ok = expr_depth * unit <= budget
    bad += not ok
    print(f"wasm_stack_budget: {profile:<7} {'parser':<24} {expr_depth * unit:>6} <= {budget:<6} "
          + ("ok" if ok else "FAIL")
          + f"  (MAX_EXPR_DEPTH {expr_depth} x {unit} B per unit: {' + '.join(chain)})")
    print("\n".join(summary))
    print(f"wasm_stack_budget: {profile:<7} other recursion reachable from eval: "
          + ", ".join(f"{k} {v}" for k, v in sorted(kinds.items())))
    print(f"wasm_stack_budget: {profile:<7} other recursion reachable from parse_program: "
          + (", ".join(f"{k} {v}" for k, v in sorted(pkinds.items())) or "none"))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
