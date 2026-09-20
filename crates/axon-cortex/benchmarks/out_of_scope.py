#!/usr/bin/env python3
"""What happens at the EDGE of what the loop can express?

Every other harness here injects a defect the loop can repair: one wrong
thing inside one function body, which is exactly the shape of the one action
Cortex has. So the results say nothing about the boundary — and a controller's
behaviour at its boundary is the part that decides whether it can be trusted
with anything else.

This swaps the bodies of two functions. It compiles, the file's own checks
catch it, and NO single `PatchSymbolBody` can undo it: repairing either half
leaves the other wrong.

The question is therefore not whether it repairs. It cannot. The question is
whether it fails honestly — a defect outside what an action can say must never
produce exit 0, and the workspace must be left as it was found.

The generator is given the correct body for one of the two swapped functions,
which is the most favourable case a real generator could ever hit: it proposes
something genuinely right, and the file is still broken afterwards.
"""
import json
import os
import shutil
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, swap_two_bodies

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")

rows = []
for f in sorted(sys.argv[1:]):
    src0 = open(f).read()
    checks = set(tests_in(src0))
    if len(checks) < 2:
        continue
    if run([AXON, "check", f])[0] != 0 or failing_tests(f):
        continue
    made = swap_two_bodies(src0, defined_fns(src0), checks)
    if made is None:
        continue
    mutant, how = made
    ws = tempfile.mkdtemp(prefix="oos_")
    rel = os.path.basename(f)
    p = os.path.join(ws, rel)
    open(p, "w").write(mutant)
    if run([AXON, "check", p])[0] != 0:
        shutil.rmtree(ws, ignore_errors=True)
        continue
    allf = failing_tests(p)
    if len(allf) < 2:
        shutil.rmtree(ws, ignore_errors=True)
        continue
    hidden = allf[-1]
    a_name, b_name = how.split(":", 1)[1].split("<->")

    # VERIFY THE PREMISE, per trial, instead of asserting it.
    #
    # "A swap needs two edits" is true of the SOURCE and not necessarily of
    # the CHECKS: if the second function's borrowed body still satisfies
    # everything that exercises it, restoring the first alone cleans the file
    # and the trial was never out of scope. One row did exactly that — exit 0
    # with a genuinely clean file — and it would have sat in these results as
    # a safety datum about a case that does not belong here.
    def cleans_alone(name):
        sp_src, sp_mut = fn_span(src0, name), fn_span(mutant, name)
        if not sp_src or not sp_mut:
            return False
        fixed = mutant[: sp_mut[0]] + src0[sp_src[0]:sp_src[1]] + mutant[sp_mut[1]:]
        probe = os.path.join(ws, "probe_" + rel)
        open(probe, "w").write(fixed)
        ok = run([AXON, "check", probe])[0] == 0 and not failing_tests(probe)
        os.remove(probe)
        return ok

    if cleans_alone(a_name) or cleans_alone(b_name):
        shutil.rmtree(ws, ignore_errors=True)
        continue

    # The correct body for ONE of the two swapped functions — the best a
    # generator could possibly do here, and still not enough.
    span = fn_span(src0, a_name)
    oracle_body = src0[span[0]:span[1]] if span else ""
    rc, out = run(
        [CORTEX, "repair", "--workspace", ws, "--file", rel, "--check", hidden,
         "--axon", AXON, "--write-prefix", rel, "--generator", "literal:" + oracle_body],
        timeout=240,
    )
    after = open(p).read()
    rows.append({
        "file": f, "how": how, "exit": rc,
        # Asked of the FILE. The exit code is the claim under test.
        "file_clean": not failing_tests(p),
        "workspace_unchanged": after == mutant,
        "tail": out.strip().split("\n")[-1][:100],
    })
    shutil.rmtree(ws, ignore_errors=True)
print(json.dumps(rows, indent=1))
