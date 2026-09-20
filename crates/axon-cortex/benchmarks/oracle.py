#!/usr/bin/env python3
"""Does the whole repair loop work on REAL code?

Localization accuracy says which function Cortex would aim at. It says nothing
about whether the machinery behind it — grant scoping, symbol-body splicing,
the hidden-check adjudication, the revert, the attempt isolation — actually
works on files larger and stranger than a hand-written fixture.

So this removes the model from the question entirely. A defect is injected,
and the loop is handed the ORIGINAL body as a `literal:` patch. The generator
is therefore a perfect oracle, and every remaining failure is a failure of the
CONTROL LOOP rather than of a model or of the ranking.

Read it as an upper bound. A real generator proposes text that may be wrong;
this one cannot be. If the loop cannot close with a perfect oracle, no
generator would have saved it.
"""
import os, re, json, subprocess, sys, tempfile, shutil, collections
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, MUTATORS

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
CANDIDATES = os.environ.get("CANDIDATES", "3")

rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    checks = tests_in(src0)
    if len(checks) < 2: continue
    # hidden is chosen PER MUTANT below. It must be a check that FAILS on the
    # mutant, or it cannot witness the repair — choosing it up front (the last
    # @[test] in the file) is what made the first run of this experiment report
    # 54 successes, most of them having changed nothing at all: the check
    # passed on the broken file because it never exercised the broken function.
    if run([AXON, "check", f])[0] != 0: continue
    if failing_tests(f): continue
    targets = [n for n in defined_fns(src0) if n not in checks and n != "main"]
    done = 0
    for name in targets:
        if done >= 8: break
        span = fn_span(src0, name)
        if not span: continue
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
            if done >= 8:
                break
            made = mfn(body)
            if made is None: continue
            newbody, how = made
            mutant = src0[:a] + newbody + src0[b:]
            ws = tempfile.mkdtemp(prefix="oracle_")
            rel = os.path.basename(f)
            p = os.path.join(ws, rel)
            open(p, "w").write(mutant)
            if run([AXON, "check", p])[0] != 0:
                shutil.rmtree(ws, ignore_errors=True); continue
            allf = failing_tests(p)
            # Two are needed: one to adjudicate (hidden — and it must FAIL, or
            # it witnesses nothing) and at least one left visible as evidence
            # for localization. With a single failing check there is nothing to
            # localize from once it is hidden.
            if len(allf) < 2:
                shutil.rmtree(ws, ignore_errors=True); continue
            hidden = allf[-1]
            # The perfect oracle: the body this file had before it was broken.
            rc, out = run([CORTEX, "repair", "--workspace", ws, "--file", rel,
                           "--check", hidden, "--axon", AXON,
                           "--write-prefix", rel, "--candidates", CANDIDATES,
                           "--generator", "literal:" + body], timeout=180)
            restored = open(p).read()
            rows.append({
                "file": f, "broke": name, "mutator": mname, "how": how,
                "exit": rc,
                "repaired": bool(re.search(r"repaired `%s`" % re.escape(name), out)),
                # Did the loop end with a file that actually passes everything,
                # including the check it was graded on? Asked of the FILE, not
                # of the exit code: an exit code is a claim about the file and
                # this is the file itself.
                "file_clean": not failing_tests(p),
                # And when it failed, did it leave the workspace as it found
                # it? A run that reports failure having rewritten a function is
                # reporting on a workspace nobody asked for.
                "restored": restored == mutant,
                "tail": out.strip().split("\n")[-1][:100],
            })
            shutil.rmtree(ws, ignore_errors=True)
            done += 1
print(json.dumps(rows))
