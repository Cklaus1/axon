#!/usr/bin/env python3
"""What does the loop do when the generator is CONFIDENTLY WRONG?

`oracle.py` hands the loop the correct body and asks how often the machinery
closes. This asks the opposite and more important question: when the generator
proposes something that compiles and is wrong — the realistic failure mode of
any model — does the loop ever report success?

The answer must be NEVER, and unlike the oracle's number this one is not a
bound to be improved. A single false success here would mean the hidden check
can be satisfied by a repair that did not repair anything, which is the whole
claim the system rests on.

The generator used is deliberately plausible rather than random: it returns the
CURRENT (broken) body with a further mutation applied. That compiles, it looks
like a repair, and it is wrong — which is exactly what a model produces when it
has misunderstood the defect.
"""
import json
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, MUTATORS

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")

rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    if len(tests_in(src0)) < 2:
        continue
    if run([AXON, "check", f])[0] != 0 or failing_tests(f):
        continue
    targets = [n for n in defined_fns(src0) if n not in tests_in(src0) and n != "main"]
    done = 0
    for name in targets:
        if done >= 4:
            break
        span = fn_span(src0, name)
        if not span:
            continue
        a, b = span
        body = src0[a:b]
        for mname, mfn in MUTATORS:
            # The cap is checked HERE as well as in the outer loop. It
            # increments inside this one, so a single function could
            # overshoot and dominate a file's sample. Measured on the
            # current corpus it never did — max 8 trials per file with
            # the cap at 8, and no function yielding more than 3 valid
            # mutants — so this closes the hole without moving any
            # published number.
            if done >= 4:
                break
            made = mfn(body)
            if made is None:
                continue
            broken, how = made
            ws = tempfile.mkdtemp(prefix="adv_")
            rel = os.path.basename(f)
            p = os.path.join(ws, rel)
            open(p, "w").write(src0[:a] + broken + src0[b:])
            if run([AXON, "check", p])[0] != 0:
                shutil.rmtree(ws, ignore_errors=True)
                continue
            allf = failing_tests(p)
            if len(allf) < 2:
                shutil.rmtree(ws, ignore_errors=True)
                continue
            hidden = allf[-1]
            # A SECOND wrong body: plausible, compiles, does not repair.
            #
            # It must not be the ORIGINAL. Several mutators are involutions —
            # swapping two arguments twice, flipping a boolean twice — so
            # applying one to the broken body can hand back exactly the correct
            # code. The pilot run did precisely that and scored a "false
            # success" that was really a correct repair. A control that is not
            # inert on the metric measures nothing.
            worse = mfn(broken)
            if worse is None or worse[0] == body:
                shutil.rmtree(ws, ignore_errors=True)
                continue
            rc, out = run(
                [CORTEX, "repair", "--workspace", ws, "--file", rel,
                 "--check", hidden, "--axon", AXON, "--write-prefix", rel,
                 "--generator", "literal:" + worse[0]],
                timeout=180,
            )
            after = open(p).read()
            rows.append({
                "file": f, "broke": name, "mutator": mname, "how": how,
                "exit": rc,
                # The claim under test. Asked of the FILE: an exit code is a
                # statement about the file, and this is the file.
                "file_clean": not failing_tests(p),
                "unchanged": after == src0[:a] + broken + src0[b:],
                # Recorded so a trial that was accidentally correct can be told
                # from one where the loop was fooled.
                "proposal_was_original": worse[0] == body,
                "tail": out.strip().split("\n")[-1][:90],
            })
            shutil.rmtree(ws, ignore_errors=True)
            done += 1
print(json.dumps(rows))
