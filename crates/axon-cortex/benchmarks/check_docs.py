#!/usr/bin/env python3
"""Do the numbers in the prose still come from the data?

Twice in one session a figure outlived the measurement it came from: the
binary's own `--help` advertised a top-1 accuracy this directory had
explicitly WITHDRAWN, and the method section described an adjudicator
selection that had not been run for several commits — while the corrections
section three screens below said so.

A document that corrects itself in one place and not another is worse than one
that is simply stale: a reader cannot tell which half is current.

So this checks both directions against `localization-*.json`:

  * every figure in the artifact appears somewhere in the prose;
  * every decimal figure in the prose traces back to the artifact.

The second direction is the one that matters. A number with no source is
exactly what a withdrawn measurement looks like after the measurement is gone.

    python3 check_docs.py        # exits non-zero on drift
"""
import glob
import json
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
art_path = sorted(glob.glob(os.path.join(HERE, "localization-*.json")))[-1]
art = json.load(open(art_path))
readme = open(os.path.join(HERE, "README.md")).read()

loc, orc = art["localization"], art["oracle_perfect_generator"]
must_appear = [
    ("localization top-1", loc["top1_sole_candidate"]),
    ("localization top-3", loc["top3"]),
    ("localization trials", loc["trials"]),
    ("oracle verified", orc["verified"]),
    ("oracle trials", orc["trials"]),
    ("out-of-scope trials", art["out_of_scope_defects"]["trials"]),
    ("guessing repaired", art["guessing_generator"]["repaired"]),
]
missing = [(k, v) for k, v in must_appear if str(v) not in readme]

sourced = {str(loc[k]) for k in ("top1_sole_candidate", "top3", "truth_absent")}
for cls in loc["per_class"].values():
    sourced.add(str(cls["top1_sole"]))
    sourced.add(str(cls["top3"]))
for d in art["call_depth_ablation"]:
    row = art["call_depth_ablation"][d]
    if isinstance(row, dict):
        sourced |= {str(row.get("top3")), str(row.get("truth_absent"))}
sourced.add(str(round(100 * orc["verified"] / orc["trials"], 1)))
g = art["guessing_generator"]
sourced.add(str(round(100 * g["repaired"] / g["trials"], 1)))
orphans = sorted(n for n in set(re.findall(r"\b\d{1,3}\.\d\b", readme)) if n not in sourced)

for k, v in missing:
    print(f"  MISSING from the prose: {k} = {v}")
for n in orphans:
    print(f"  UNSOURCED in the prose: {n} — no figure in {os.path.basename(art_path)} produces it")
if missing or orphans:
    print("\ndocs have drifted from the data they cite")
    sys.exit(1)
print(f"docs match {os.path.basename(art_path)} in both directions")
