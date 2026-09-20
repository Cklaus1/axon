#!/usr/bin/env python3
"""Top-k localization accuracy on the real corpus.

The verdict answers "which function", and scoring only that cannot say whether
a wrong answer was a near miss or nonsense. This asks the ranking directly: at
what rank does the function that was actually broken appear?

That distinction decides a design question rather than settling a curiosity —
if the truth is usually rank 2 when rank 1 is wrong, the loop should try
candidates in order; if it is usually absent, ranking deeper buys nothing.
"""
import os, re, json, subprocess, sys, tempfile, shutil, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from measure import (tests_in, fn_span, defined_fns, run, failing_tests,
                     AXON, SWAPS)

CORTEX = os.environ.get("CORTEX_BIN")
rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    checks = tests_in(src0)
    if len(checks) < 2: continue
    hidden = checks[-1]
    if run([AXON, "check", f])[0] != 0: continue
    if failing_tests(f): continue
    targets = [n for n in defined_fns(src0) if n not in checks and n != "main"]
    done = 0
    for name in targets:
        if done >= 3: break
        span = fn_span(src0, name)
        if not span: continue
        a, b = span
        body = src0[a:b]
        for old, new in SWAPS:
            if old not in body: continue
            mutant = src0[:a] + body.replace(old, new, 1) + src0[b:]
            ws = tempfile.mkdtemp(prefix="topk_")
            rel = os.path.basename(f)
            open(os.path.join(ws, rel), "w").write(mutant)
            if run([AXON, "check", os.path.join(ws, rel)])[0] != 0:
                shutil.rmtree(ws, ignore_errors=True); continue
            if not [t for t in failing_tests(os.path.join(ws, rel)) if t != hidden]:
                shutil.rmtree(ws, ignore_errors=True); continue
            rc, out = run([CORTEX, "locate", "--workspace", ws, "--file", rel,
                           "--check", hidden, "--axon", AXON, "--json"])
            try:
                d = json.loads(out.strip().split("\n")[-1])
                ranked = [r["symbol"] for r in d["ranked"]]
                scores = [r["score"] for r in d["ranked"]]
            except Exception:
                ranked, scores = [], []
            rank = ranked.index(name) + 1 if name in ranked else 0
            # A tie at the top is a refusal in the verdict, so record the width
            # of the top group: a rank-1 answer inside a 5-way tie is not an
            # answer.
            tie = sum(1 for s in scores if scores and abs(s - scores[0]) < 1e-12)
            rows.append({"file": f, "broke": name, "rank": rank,
                         "n": len(ranked), "tie_at_top": tie})
            shutil.rmtree(ws, ignore_errors=True)
            done += 1
            break
print(json.dumps(rows))
