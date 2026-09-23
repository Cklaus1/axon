#!/usr/bin/env python3
"""Prove `asyncify-ignore-indirect` is SAFE for a given axon-wasm module.

binaryen's Asyncify instruments every function that can reach the suspending
import. By default it assumes ANY `call_indirect` might reach it, which made it
instrument 1418 of 1423 functions in axon-wasm: code grew ~3.9x, and executing
`println("hello")` through the result cost ~15 GB of V8 memory (the harness's
four programs peaked at 18-31 GB) against 36 MB uninstrumented.

`--pass-arg=asyncify-ignore-indirect` drops that assumption. It is only sound if
NO function that can reach the suspend point is ever called indirectly. If one
were, a suspend through it would unwind a frame Asyncify never instrumented and
corrupt the resume — silently, and only on the path that happened to go
indirect. So this checks the invariant directly on the module, rather than
trusting that it held when the flag was added:

  S = functions that can reach the suspending import via direct calls
  T = address-taken functions (the only possible call_indirect targets)
  safe  <=>  S ∩ T is empty

Exit 0 = safe, 1 = unsafe (names the offenders), 2 = could not decide (a parse
that finds nothing is reported as undecided, never as safe).

Usage: asyncify_indirect_guard.py <module.wasm> <import-module> <import-name>
"""
import collections, re, subprocess, sys

def main():
    if len(sys.argv) != 4:
        print(__doc__.strip().splitlines()[-1]); return 2
    wasm, imod, iname = sys.argv[1:]
    try:
        wat = subprocess.run(["wasm-dis", wasm], capture_output=True, text=True, check=True).stdout.splitlines()
    except (OSError, subprocess.CalledProcessError) as e:
        print(f"asyncify_indirect_guard: UNDECIDED — cannot disassemble ({e})"); return 2

    imp = None
    for l in wat:
        m = re.search(rf'\(import "{re.escape(imod)}" "{re.escape(iname)}" \(func (\$\S+)', l)
        if m: imp = m.group(1).rstrip(")"); break
    if imp is None:
        print(f"asyncify_indirect_guard: UNDECIDED — import {imod}.{iname} not found"); return 2

    calls, cur, nfuncs = collections.defaultdict(set), None, 0
    for l in wat:
        if l.startswith(" (func "):
            cur = l.split()[1]; nfuncs += 1
        elif cur:
            for m in re.finditer(r"\(call (\$\S+)", l):
                calls[cur].add(m.group(1).rstrip(")"))
    edges = sum(len(v) for v in calls.values())
    # A parser that silently matches nothing would prove "safe" about any module.
    # Refuse to decide on an implausibly empty graph.
    if nfuncs < 10 or edges < nfuncs:
        print(f"asyncify_indirect_guard: UNDECIDED — call graph implausibly small "
              f"({nfuncs} funcs, {edges} edges); refusing to call that safe"); return 2

    taken = set()
    for l in wat:
        if l.lstrip().startswith("(elem"):
            taken |= {"$" + m.group(1) for m in re.finditer(r"\$([^\s()]+)", l)}

    rev = collections.defaultdict(set)
    for a, bs in calls.items():
        for b in bs: rev[b].add(a)
    reach, stack = {imp}, [imp]
    while stack:
        x = stack.pop()
        for p in rev[x]:
            if p not in reach: reach.add(p); stack.append(p)
    reach.discard(imp)
    if not reach:
        print("asyncify_indirect_guard: UNDECIDED — nothing calls the suspending import"); return 2

    bad = sorted(reach & taken)
    if bad:
        print(f"asyncify_indirect_guard: UNSAFE — {len(bad)} function(s) reach "
              f"{imod}.{iname} AND are address-taken, so an indirect call could "
              f"suspend through an uninstrumented frame:")
        for b in bad[:20]: print("    " + b)
        return 1
    print(f"asyncify_indirect_guard: SAFE — {len(reach)} function(s) reach "
          f"{imod}.{iname}; none of {len(taken)} address-taken functions is among them")
    return 0

sys.exit(main())
