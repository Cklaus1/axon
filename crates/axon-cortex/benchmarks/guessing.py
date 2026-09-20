#!/usr/bin/env python3
"""Repair rate with a generator that is NOT told the answer.

`oracle.py` hands the loop the correct body — an upper bound on the machinery.
`adversary.py` hands it something confidently wrong — a lower bound on safety.
Neither is a generator. This one is: `guess.py` proposes plausible single-token
edits, in a fixed order, knowing nothing about which defect was injected, and
must learn from the rejections the loop feeds back.

It is not a model and its numbers are not a model's. What it measures is
whether the loop can be DRIVEN to a repair by something that guesses — which
is the shape of the problem a model actually faces, minus the understanding.
"""
import json
import os
import re
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, MUTATORS

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
GUESS = os.path.join(os.path.dirname(os.path.abspath(__file__)), "guess.py")

rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    if len(tests_in(src0)) < 2:
        continue
    if run([AXON, "check", f])[0] != 0 or failing_tests(f):
        continue
    checks = tests_in(src0)
    targets = [n for n in defined_fns(src0) if n not in checks and n != "main"]
    done = 0
    for name in targets:
        if done >= 2:
            break
        span = fn_span(src0, name)
        if not span:
            continue
        a, b = span
        body = src0[a:b]
        for mname, mfn in MUTATORS:
            made = mfn(body)
            if made is None:
                continue
            mutant, how = made
            ws = tempfile.mkdtemp(prefix="guess_")
            rel = os.path.basename(f)
            p = os.path.join(ws, rel)
            open(p, "w").write(src0[:a] + mutant + src0[b:])
            if run([AXON, "check", p])[0] != 0:
                shutil.rmtree(ws, ignore_errors=True)
                continue
            allf = failing_tests(p)
            if len(allf) < 2:
                shutil.rmtree(ws, ignore_errors=True)
                continue
            hidden = allf[-1]
            rc, out = run(
                [CORTEX, "repair", "--workspace", ws, "--file", rel, "--check", hidden,
                 "--axon", AXON, "--write-prefix", rel, "--budget", "12",
                 "--generator", "cmd:" + GUESS],
                timeout=300,
            )
            rows.append({
                "file": f, "broke": name, "mutator": mname, "how": how,
                "exit": rc,
                # Asked of the FILE. The exit code is the claim under test.
                "file_clean": not failing_tests(p),
                "repaired_right_fn": bool(
                    re.search(r"repaired `%s`" % re.escape(name), out)
                ),
                "tail": out.strip().split("\n")[-1][:90],
            })
            shutil.rmtree(ws, ignore_errors=True)
            done += 1
print(json.dumps(rows))
