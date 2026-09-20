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
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, MUTATORS

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    checks = tests_in(src0)
    if len(checks) < 2: continue
    # hidden is chosen per-mutant below: it must be a check that FAILS on the
    # mutant, or it cannot witness the repair. Choosing it up front (the last
    # @[test] in the file) was the flaw that made the first oracle run report
    # 54 successes, most of them having changed nothing — the check passed on
    # the broken file because it never exercised the broken function.
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
            ws = tempfile.mkdtemp(prefix="topk_")
            rel = os.path.basename(f)
            open(os.path.join(ws, rel), "w").write(mutant)
            if run([AXON, "check", os.path.join(ws, rel)])[0] != 0:
                shutil.rmtree(ws, ignore_errors=True); continue
            allf = failing_tests(os.path.join(ws, rel))
            # Need at least two: one to adjudicate (hidden, and it must fail or
            # it witnesses nothing) and one to remain as visible evidence for
            # localization. With only one failing check there is nothing left
            # to localize from once it is hidden.
            if len(allf) < 2:
                shutil.rmtree(ws, ignore_errors=True); continue
            hidden = allf[-1]
            fails = [t for t in allf if t != hidden]
            if not fails:
                shutil.rmtree(ws, ignore_errors=True); continue
            rc, out = run([CORTEX, "locate", "--workspace", ws, "--file", rel,
                           "--check", hidden, "--axon", AXON, "--json"])
            # A RANKING THAT COULD NOT BE READ IS NOT A RANKING THAT MISSED.
            #
            # `rank = 0` is this file's encoding for "the truth was ABSENT from
            # the ranking", and `truth_absent` is the figure
            # localization-*.json publishes and locate.rs cites as the sole
            # reason MAX_CALL_DEPTH is 4. A crash, a 60s timeout (common.run
            # returns (-1, "TIMEOUT")), or a non-JSON trailing line all landed
            # in one `except` and became rank 0 — an infrastructure failure
            # reported as a localization miss, in the number that justifies a
            # compiler constant.
            #
            # Identified by schema rather than by position: `run()` returns
            # stdout+stderr CONCATENATED, so "the last line" is not an identity.
            ranked, scores, rank_read = [], [], False
            if rc == 0:
                for line in reversed(out.strip().split("\n")):
                    try:
                        d = json.loads(line)
                    except Exception:
                        continue
                    if isinstance(d, dict) and d.get("schema") == "cortex-locate/1":
                        ranked = [r["symbol"] for r in d.get("ranked", [])]
                        scores = [r["score"] for r in d.get("ranked", [])]
                        rank_read = True
                        break
            if not rank_read:
                # Excluded rather than scored. Counting it as absent inflates
                # exactly the figure this harness exists to report.
                rows.append({"file": f, "broke": name, "rank": None,
                             "rank_read": False, "mutator": mname, "how": how,
                             "why": f"cortex locate did not produce a ranking (rc={rc})"})
                shutil.rmtree(ws, ignore_errors=True); continue
            rank = ranked.index(name) + 1 if name in ranked else 0
            # A tie at the top is a refusal in the verdict, so record the width
            # of the top group: a rank-1 answer inside a 5-way tie is not an
            # answer.
            tie = sum(1 for s in scores if scores and abs(s - scores[0]) < 1e-12)
            rows.append({"file": f, "broke": name, "rank": rank,
                         "rank_read": True,
                         "n": len(ranked), "tie_at_top": tie,
                         "mutator": mname, "how": how})
            shutil.rmtree(ws, ignore_errors=True)
            done += 1
print(json.dumps(rows))
