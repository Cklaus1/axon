#!/usr/bin/env python3
"""Can a real model generate useful repairs through this narrow interface?

Everything else here measures the CONTROLLER. With a perfect generator the
loop's failures are exactly the ranking's misses, so the control path is not
the bottleneck; with a deliberately wrong one it produced no false successes.
The open term is the middle one:

    overall repair
      = localization coverage
      x generator success GIVEN the target was attempted
      x controller acceptance correctness

This estimates the middle factor, which is why the headline is CONDITIONAL:

    P(repair succeeds | the true symbol was among the candidates attempted)

Reported separately from the unconditional rate, because a failure whose true
target was never attempted is a localization result wearing a generator's
clothes.

The model runs behind `cmd:` — credentials stay in the driver's own
environment and never enter Axon. Stratified across the five defect classes so
one easy class cannot carry the number.
"""
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from common import ROOT, tests_in, fn_span, defined_fns, run, failing_tests, AXON, MUTATORS

CORTEX = os.environ.get("CORTEX_BIN", f"{ROOT}/target/debug/cortex")
DRIVER = os.path.join(os.path.dirname(os.path.abspath(__file__)), "drivers", "claude_cli.sh")
PER_CLASS = int(os.environ.get("PER_CLASS", "4"))
CANDIDATES = os.environ.get("CANDIDATES", "3")


def trial(f, src0, name, mname, mfn, hidden_pref=None):
    span = fn_span(src0, name)
    if not span:
        return None
    a, b = span
    made = mfn(src0[a:b])
    if made is None:
        return None
    mutant, how = made
    ws = tempfile.mkdtemp(prefix="real_")
    rel = os.path.basename(f)
    p = os.path.join(ws, rel)
    open(p, "w").write(src0[:a] + mutant + src0[b:])
    if run([AXON, "check", p])[0] != 0:
        shutil.rmtree(ws, ignore_errors=True)
        return None
    allf = failing_tests(p)
    if len(allf) < 2:
        shutil.rmtree(ws, ignore_errors=True)
        return None
    hidden = allf[-1]

    # The ranking, BEFORE the repair runs — so a failure can be attributed to
    # localization or to the generator rather than to "the run".
    rc, out = run([CORTEX, "locate", "--workspace", ws, "--file", rel,
                   "--check", hidden, "--axon", AXON, "--json"])
    try:
        ranked = [r["symbol"] for r in json.loads(out.strip().split("\n")[-1])["ranked"]]
    except Exception:
        ranked = []
    rank = ranked.index(name) + 1 if name in ranked else 0

    log = os.path.join(ws, "model.jsonl")
    env = dict(os.environ, CORTEX_MODEL_LOG=log)
    began = __import__("time").time()
    proc = subprocess.run(
        [CORTEX, "repair", "--workspace", ws, "--file", rel, "--check", hidden,
         "--axon", AXON, "--write-prefix", rel, "--candidates", CANDIDATES,
         "--budget", "12", "--generator", "cmd:" + DRIVER, "--json"],
        capture_output=True, text=True, timeout=1800, env=env,
    )
    wall = __import__("time").time() - began
    try:
        verdict = json.loads(proc.stdout.strip().split("\n")[-1])
    except Exception:
        verdict = {}
    calls = []
    if os.path.exists(log):
        calls = [json.loads(l) for l in open(log) if l.strip()]

    row = {
        "file": f, "symbol": name, "mutator": mname, "how": how,
        "localization_rank": rank, "ranked": ranked[: int(CANDIDATES)],
        "attempted": verdict.get("attempted", []),
        "target_attempted": name in verdict.get("attempted", []),
        "outcome": verdict.get("outcome"), "exit": proc.returncode,
        # Asked of the FILE, never of the exit code.
        "file_clean": not failing_tests(p),
        "workspace_restored": (open(p).read() == src0[:a] + mutant + src0[b:]),
        # THE EPISODE, kept whole. The run in flight stored only selected
        # verdict fields, so "invalid-output cost" — a proposal whose
        # transport was fine and whose CODE was unusable — could not be
        # recovered afterwards. `patch_reverted` events are exactly that
        # signal, and discarding them made a real bucket unmeasurable.
        "episode": verdict.get("episode"),
        "reverted": sum(
            1 for e in (verdict.get("episode") or {}).get("events", [])
            if e.get("kind") == "patch_reverted"
        ),
        "extraction_status": [c.get("extraction_status") for c in calls],
        "proposals": len(calls),
        "bodies": [c["body"] for c in calls],
        "prior_rejections_seen": [c["prior_rejections"] for c in calls],
        "models": sorted({m for c in calls for m in (c["model"] or [])}),
        "per_proposal": [
            {
                "n": c["proposal_number"],
                "priors": c["prior_rejections"],
                "input": c["input_tokens"],
                "output": c["output_tokens"],
                "cache_create": c["cache_create"],
                "cache_read": c["cache_read"],
                "cost_usd": c["cost_usd"],
                "api_ms": c["duration_api_ms"],
            }
            for c in calls
        ],
        "cost_usd": round(sum(c["cost_usd"] or 0 for c in calls), 4),
        "api_ms": sum(c["duration_api_ms"] or 0 for c in calls),
        "wall_s": round(wall, 1),
        "output_tokens": sum(c["output_tokens"] or 0 for c in calls),
    }
    shutil.rmtree(ws, ignore_errors=True)
    return row


# PROVENANCE, recorded before trial 1. An experiment whose subject moved
# midway is a diagnostic sweep, not an experiment.
import hashlib
commit = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"],
                        capture_output=True, text=True).stdout.strip()
driver_sha = hashlib.sha256(open(DRIVER, "rb").read()).hexdigest()[:16]
meta = {
    "cortex_commit": commit,
    "driver_sha256_16": driver_sha,
    "model": os.environ.get("CORTEX_MODEL", "claude-haiku-4-5-20251001"),
    "candidates": CANDIDATES,
    "per_class": PER_CLASS,
}
print(f"  provenance: {meta}", file=sys.stderr, flush=True)

rows = []
files = sorted(sys.argv[1:])
for mname, mfn in MUTATORS:
    taken = 0
    for f in files:
        if taken >= PER_CLASS:
            break
        src0 = open(f).read()
        checks = set(tests_in(src0))
        if len(checks) < 2 or run([AXON, "check", f])[0] != 0 or failing_tests(f):
            continue
        for name in [n for n in defined_fns(src0) if n not in checks and n != "main"]:
            if taken >= PER_CLASS:
                break
            r = trial(f, src0, name, mname, mfn)
            if r:
                rows.append(r)
                taken += 1
                print(
                    f"  [{mname}] {os.path.basename(f)}::{name} rank={r['localization_rank']} "
                    f"attempted={r['target_attempted']} {r['outcome']} clean={r['file_clean']} "
                    f"props={r['proposals']} ${r['cost_usd']}",
                    file=sys.stderr, flush=True,
                )
            break  # one function per file per class, so no file dominates
print(json.dumps({"meta": meta, "trials": rows}, indent=1))
