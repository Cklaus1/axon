"""Mutation operators for hx (spec FU-1a): the five type-correct `subtle`
operators and multi-mutation composition.

A new file rather than additions to ../heldout/mutators.py: that module is an
input of the frozen heldout/once sets, and anything added there would change
what their builder can plant. The existing operators are reused, not copied:
`ALL_OPS` is ../heldout/mutators.py's `DEV_CLASS_MUTATORS` (the six dev
classes of ../common.py and ../ab/corpus.py, at any site) and `NEW_MUTATORS`,
plus `SUBTLE_OPS` below.

`subtle` operators (each keeps every type, so the defect shows only as
behaviour; a mutant that does not type-check is discarded by the builder):

  bound          a loop or range bound moved by one: the right side of a
                 `while` comparison, or either end of a `for .. in A..B`
  argswap-typed  two arguments of a call to a function of this file swapped
                 where the callee declares both parameters with the same type
                 (../common.py `argswap` swaps any two plain names)
  wrong-field    a `.field` read replaced by another field declared with the
                 same type (../heldout/mutators.py `wrong_field`, reused)
  acc-init       an accumulator's initial value swapped for the other identity
                 (0 <-> 1, 0.0 <-> 1.0, true <-> false; any other integer -> 0):
                 a `let mut x = LIT` that is later reassigned from itself
  drop-else      the `else { ... }` arm of an `if` dropped; only statement-
                 position ifs survive the type check

Every operator takes `(body, k, ctx)` and returns the k-th site's
`(mutant_body, how)` or None once the sites run out (../heldout/mutators.py's
convention). String literals and `//` comments are never sites.

Composition (`multi`): `edit_of` turns a single mutant into one edit
`(start, end, replacement)` on the original body, `disjoint` says whether
edits neither overlap nor touch, and `compose` applies several at once.

`erase` (hx2 HX-3a, class `erase`) is not an operator: `erase(body, ret)`
replaces the whole body by the typed default of the return type `ret`
(`return_type` reads it). It is not in `ALL_OPS`, so `multi` never composes it.
"""
import importlib.util
import os
import re

_HELDOUT = os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), "heldout", "mutators.py")
_spec = importlib.util.spec_from_file_location("heldout_mutators", _HELDOUT)
H = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(H)

IDENT = H.IDENT
_code_spans = H._code_spans
_code_matches = H._code_matches
wrong_field = H.wrong_field


# ------------------------------------------------------------ helpers

def _cond_end(body, i, mask):
    """Index of the `{` that ends a `while`/`for` header starting at i."""
    depth = 0
    while i < len(body):
        c = body[i]
        if mask[i]:
            if c in "([":
                depth += 1
            elif c in ")]":
                depth -= 1
            elif c == "{" and depth == 0:
                return i
            elif c == "\n" and depth == 0:
                return None
        i += 1
    return None


def _shift(expr, delta):
    """`expr` moved by `delta` (+1/-1), written as a programmer would."""
    e = expr.strip()
    if re.fullmatch(r"-?\d+", e):
        return str(int(e) + delta)
    m = re.fullmatch(r"(.*?\S)\s*([+-])\s*(\d+)", e)
    if m:
        c = (int(m.group(3)) if m.group(2) == "+" else -int(m.group(3))) + delta
        return m.group(1) if c == 0 else f"{m.group(1)} {'+' if c > 0 else '-'} {abs(c)}"
    if re.fullmatch(r"[\w.]+(\(.*\))?(\[.*\])?", e):
        return f"{e} {'+' if delta > 0 else '-'} 1"
    return f"({e}) {'+' if delta > 0 else '-'} 1"


def _split_top(text, mask_offset, mask, sep=",", opens="([{", closes=")]}"):
    """[(start, end)] of `text`'s top-level parts, split at `sep` outside
    brackets and strings; offsets are relative to `text`."""
    parts, depth, start = [], 0, 0
    for i, c in enumerate(text):
        if not mask[mask_offset + i]:
            continue
        if c in opens:
            depth += 1
        elif c in closes:
            depth -= 1
        elif c == sep and depth == 0:
            parts.append((start, i))
            start = i + 1
    parts.append((start, len(text)))
    return parts


# ------------------------------------------------------------ subtle operators

def bound(body, k, ctx):
    mask = _code_spans(body)
    sites = []
    for m in _code_matches(r"\bwhile\b", body):
        e = _cond_end(body, m.end(), mask)
        if e is None:
            continue
        cond = body[m.end():e]
        if "&&" in cond or "||" in cond:
            continue
        cm = [c for c in re.finditer(r"(?<![<>=!\-])(<=|>=|<|>)(?![<>=])", cond) if mask[m.end() + c.start()]]
        if len(cm) != 1:
            continue
        a = m.end() + cm[0].end()
        rhs = body[a:e]
        lead = len(rhs) - len(rhs.lstrip())
        a, b = a + lead, a + len(rhs.rstrip())
        if a < b:
            sites.append((a, b, "while"))
    for m in _code_matches(r"\bfor\b", body):
        e = _cond_end(body, m.end(), mask)
        if e is None:
            continue
        head = body[m.end():e]
        im = re.search(r"\bin\b", head)
        if not im:
            continue
        rs = m.end() + im.end()
        rm = re.search(r"\.\.=?", body[rs:e])
        if not rm:
            continue
        lo_a, lo_b = rs, rs + rm.start()
        hi_a, hi_b = rs + rm.end(), e
        for a, b, tag in ((lo_a, lo_b, "range-lo"), (hi_a, hi_b, "range-hi")):
            seg = body[a:b]
            lead = len(seg) - len(seg.lstrip())
            a2, b2 = a + lead, a + len(seg.rstrip())
            if a2 < b2:
                sites.append((a2, b2, tag))
    out = []
    for a, b, tag in sites:
        for delta in (1, -1):
            new = _shift(body[a:b], delta)
            if new != body[a:b]:
                out.append((body[:a] + new + body[b:], f"bound:{tag}:{body[a:b]}->{new}"))
    return out[k] if k < len(out) else None


def typed_argswap(body, k, ctx):
    sigs = ctx.get("sigs", {})
    mask = _code_spans(body)
    sites = []
    for m in _code_matches(r"(?<![\w.])(%s)\(" % IDENT, body):
        name = m.group(1)
        if name not in sigs or body[max(0, m.start() - 3):m.start()] == "fn ":
            continue
        o = m.end() - 1
        d, close = 0, None
        for i in range(o, len(body)):
            if not mask[i]:
                continue
            if body[i] in "([{":
                d += 1
            elif body[i] in ")]}":
                d -= 1
                if d == 0:
                    close = i
                    break
        if close is None:
            continue
        inner = body[o + 1:close]
        parts = _split_top(inner, o + 1, mask)
        types = sigs[name]
        if len(parts) != len(types) or not inner.strip():
            continue
        args = [(o + 1 + a, o + 1 + b) for a, b in parts]
        texts = [body[a:b].strip() for a, b in args]
        for i in range(len(args)):
            for j in range(i + 1, len(args)):
                if types[i] != types[j] or texts[i] == texts[j]:
                    continue
                (ai, bi), (aj, bj) = args[i], args[j]
                li = body[ai:bi]
                lj = body[aj:bj]
                new_i = li.replace(li.strip(), texts[j], 1)
                new_j = lj.replace(lj.strip(), texts[i], 1)
                mut = body[:ai] + new_i + body[bi:aj] + new_j + body[bj:]
                sites.append((mut, f"argswap-typed:{name}#{i}<->{j}"))
    return sites[k] if k < len(sites) else None


_ACC_SWAP = {"0": "1", "1": "0", "0.0": "1.0", "1.0": "0.0", "true": "false", "false": "true"}


def acc_init(body, k, ctx):
    mask = _code_spans(body)
    sites = []
    for m in re.finditer(r"\blet\s+mut\s+(%s)(\s*:\s*[^=\n]+)?\s*=\s*(-?\d+(?:\.\d+)?|true|false)[ \t]*$"
                         % IDENT, body, re.M):
        if not mask[m.start()]:
            continue
        name, lit = m.group(1), m.group(3)
        rest = body[m.end():]
        n = re.escape(name)
        if not re.search(r"(?<![\w.])%s\s*(?:[-+*/]=|=(?!=)[^\n]*(?<![\w.])%s\b)" % (n, n), rest):
            continue
        if lit in _ACC_SWAP:
            new = _ACC_SWAP[lit]
        elif "." in lit:
            new = "0.0"
        else:
            new = "0"
        a = m.start(3)
        sites.append((body[:a] + new + body[m.end(3):], f"acc-init:{name}:{lit}->{new}"))
    return sites[k] if k < len(sites) else None


def drop_else(body, k, ctx):
    mask = _code_spans(body)
    sites = []
    for m in _code_matches(r"\belse\s*\{", body):
        pre = body[:m.start()].rstrip()
        if not pre.endswith("}"):
            continue
        o = m.end() - 1
        close = H._matching(body, o)
        if close is None:
            continue
        if not body[o + 1:close].strip():
            continue
        sites.append((body[:len(pre)] + body[close + 1:], "drop-else"))
    return sites[k] if k < len(sites) else None


SUBTLE_OPS = [
    ("bound", bound),
    ("argswap-typed", typed_argswap),
    ("wrong-field", wrong_field),
    ("acc-init", acc_init),
    ("drop-else", drop_else),
]

# Every operator, for the classes that take any defect shape (multi, caller,
# caps, refine): the dev classes at any site, the held-out shapes, and the
# subtle ones; one entry per operator name.
ALL_OPS = list({n: f for n, f in H.DEV_CLASS_MUTATORS + H.NEW_MUTATORS + SUBTLE_OPS}.items())


# ------------------------------------------------------------ erase (HX-3a)

INT_TYPES = ("i64", "i32", "i16", "i8", "u64", "u32", "u16", "u8", "isize", "usize")


def typed_default(ret):
    """The typed default literal of return type `ret`, or None (no default:
    a struct, enum, Option, Result, ...)."""
    if ret in INT_TYPES:
        return "0"
    if ret == "f64":
        return "0.0"
    if ret == "bool":
        return "false"
    if ret in ("str", "String"):
        return '""'
    if ret and re.fullmatch(r"\[.+\]", ret):
        return "[]"
    return None


def return_type(src, name):
    """The declared return type of `fn name(` (whitespace-normalized), or None
    without `->` or without the function."""
    m = re.search(r"\bfn %s\(" % re.escape(name), src)
    if not m:
        return None
    d = 0
    for i in range(m.end() - 1, len(src)):
        if src[i] == "(":
            d += 1
        elif src[i] == ")":
            d -= 1
            if d == 0:
                break
    else:
        return None
    o = src.find("{", i)
    head = src[i + 1:o].strip() if o >= 0 else ""
    if not head.startswith("->"):
        return None
    return re.sub(r"\s+", " ", head[2:].strip()) or None


def erase(body, ret):
    """`body` (the text between a function's braces) replaced by the typed
    default of `ret`, indented like the body's first line; None when `ret`
    has no typed default."""
    d = typed_default(ret)
    if d is None:
        return None
    if "\n" not in body:
        return f" {d} "
    first = next((ln for ln in body.split("\n") if ln.strip()), "    ")
    indent = re.match(r"[ \t]*", first).group(0)
    tail = body[body.rfind("\n") + 1:]
    return f"\n{indent}{d}\n{tail if not tail.strip() else ''}"


# ------------------------------------------------------------ context

def signatures(src):
    """{fn name: [declared parameter types]} for this file's functions; a
    name defined twice (impl methods) is left out: its callee is ambiguous."""
    seen, out = set(), {}
    for m in re.finditer(r"^\s*(?:pub\s+)?fn\s+(%s)\s*(?:<[^>]*>)?\(" % IDENT, src, re.M):
        name = m.group(1)
        o = m.end() - 1
        d, close = 0, None
        for i in range(o, len(src)):
            if src[i] == "(":
                d += 1
            elif src[i] == ")":
                d -= 1
                if d == 0:
                    close = i
                    break
        if close is None:
            continue
        params = src[o + 1:close]
        types = []
        for a, b in _split_top(params, 0, [True] * len(params), opens="([{<", closes=")]}>"):
            p = params[a:b].strip()
            if not p:
                continue
            pm = re.match(r"(?:mut\s+)?%s\s*:\s*(.+)$" % IDENT, p, re.S)
            types.append(re.sub(r"\s+", " ", pm.group(1).strip()) if pm else None)
        if name in seen:
            out.pop(name, None)
            continue
        seen.add(name)
        if all(types):
            out[name] = types
    return out


def context(src, name):
    """../heldout/mutators.py's context plus the file's signatures."""
    ctx = H.context(src, name)
    ctx["sigs"] = signatures(src)
    return ctx


# ------------------------------------------------------------ composition

def edit_of(body, mutant):
    """(start, end, replacement): `mutant` == body[:start] + replacement + body[end:]."""
    n = min(len(body), len(mutant))
    p = 0
    while p < n and body[p] == mutant[p]:
        p += 1
    s = 0
    while s < n - p and body[len(body) - 1 - s] == mutant[len(mutant) - 1 - s]:
        s += 1
    return (p, len(body) - s, mutant[p:len(mutant) - s])


def disjoint(edits):
    """True when no two edits overlap or touch."""
    es = sorted(edits)
    return all(a[1] < b[0] for a, b in zip(es, es[1:]))


def compose(body, edits):
    assert disjoint(edits), "compose: edits overlap"
    out = body
    for a, b, r in sorted(edits, reverse=True):
        out = out[:a] + r + out[b:]
    return out


def sites(body, ops, ctx, max_sites):
    """[(op name, mutant, how)]: every operator's first `max_sites` sites."""
    out = []
    for name, op in ops:
        for k in range(max_sites):
            r = op(body, k, ctx)
            if r is None:
                break
            if r[0] != body:
                out.append((name, r[0], r[1]))
    return out
