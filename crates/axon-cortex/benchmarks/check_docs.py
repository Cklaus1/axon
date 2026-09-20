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

def broke(why):
    """The checker itself failed.

    Exits 2, not 1. A crash and a drift both exit non-zero, and reporting
    "the docs cite figures the data does not produce" when the truth is "this
    script does not understand the artifact" is the same absent-vs-failed
    collapse everything else here guards against.
    """
    print(f"  CHECKER ERROR: {why}")
    print("\nthis script could not evaluate the docs; that is not a finding about them")
    sys.exit(2)


HERE = os.path.dirname(os.path.abspath(__file__))
art_path = sorted(glob.glob(os.path.join(HERE, "localization-*.json")))[-1]
art = json.load(open(art_path))
readme = open(os.path.join(HERE, "README.md")).read()

for section in ("localization", "oracle_perfect_generator", "adversarial_generator",
                "guessing_generator", "out_of_scope_defects", "call_depth_ablation"):
    if section not in art:
        broke(f"the artifact has no `{section}` section")
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
oos = art["out_of_scope_defects"]
sourced.add(str(round(100 * g["repaired"] / g["trials"], 1)))
orphans = sorted(n for n in set(re.findall(r"\b\d{1,3}\.\d\b", readme)) if n not in sourced)

# FRACTIONS TOO — "99 / 111", "16 / 16".
#
# The first version checked only decimals, so perturbing `16 / 16` to
# `15 / 16` passed: the numerator is not a decimal and the denominator still
# appeared elsewhere. A checker that misses the shape most of its own tables
# are written in is the defect it exists to catch, one layer up.
try:
    _ = (orc["verified"], orc["trials"], orc["file_actually_clean"],
         orc["repaired_right_function"], g["repaired"], g["trials"],
         oos["exit_zero"], oos["trials"])
except KeyError as e:
    broke(f"the artifact is missing the field {e}")
legit_fractions = {
    (orc["verified"], orc["trials"]),
    (orc["file_actually_clean"], orc["trials"]),
    (orc["repaired_right_function"], orc["verified"]),
    (g["repaired"], g["trials"]),
    (oos["exit_zero"], oos["trials"]),
    (oos["trials"], oos["trials"]),
    (art["adversarial_generator"]["false_successes"], art["adversarial_generator"]["trials"]),
}
# Any "a/b" the artifact RECORDS, wherever it records it. Enumerating the
# fields by name meant adding a measurement to the data required editing this
# script too — and forgetting to is indistinguishable from drift, which is
# exactly what happened the first time: a real figure was reported as
# unsourced because the checker had not been taught about its field.
def _walk(node):
    if isinstance(node, dict):
        for v in node.values():
            yield from _walk(v)
    elif isinstance(node, list):
        for v in node:
            yield from _walk(v)
    elif isinstance(node, str):
        m = re.fullmatch(r"(\d{1,4})\s*/\s*(\d{1,4})", node.strip())
        if m:
            yield (int(m.group(1)), int(m.group(2)))


legit_fractions |= set(_walk(art))

legit = {f"{a} / {b}" for a, b in legit_fractions} | {f"{a}/{b}" for a, b in legit_fractions}
bad_fractions = sorted(
    f for f in set(re.findall(r"\b\d{1,3}\s*/\s*\d{1,3}\b", readme)) if f not in legit
)

# THE SAME FIGURES LIVE IN TWO OTHER DOCUMENTS.
#
# The crate README and CLAUDE.md quote the localization percentages too, and
# both were corrected BY HAND when the measurement moved. Checking only the
# benchmark README fixes the instance and leaves the class: the next time a
# number changes, those two drift silently and the gate stays green.
#
# CLAUDE.md is scoped to its Cortex section — the rest of that file is full of
# unrelated figures, and a checker that cried wolf on them would be switched
# off within a day.
def _cortex_section(text):
    start = text.find("### Cortex — the repair control loop")
    if start < 0:
        return ""
    nxt = text.find("\n## ", start)
    return text[start : nxt if nxt > 0 else len(text)]


ROOT = os.path.abspath(os.path.join(HERE, "..", "..", ".."))
elsewhere = []
crate_readme = os.path.join(HERE, "..", "README.md")
if os.path.exists(crate_readme):
    elsewhere.append(("crates/axon-cortex/README.md", open(crate_readme).read()))
claude = os.path.join(ROOT, "CLAUDE.md")
if os.path.exists(claude):
    sec = _cortex_section(open(claude).read())
    if not sec:
        broke("CLAUDE.md has no `### Cortex` section to scope the check to")
    elsewhere.append(("CLAUDE.md (Cortex section)", sec))

# PERCENTAGES ONLY, in these two.
#
# A bare-decimal scan flagged `§9.5` (a ROADMAP section reference) and read
# the exit-code list `20/21/22/23/24` as fractions. Those are false positives,
# and the comment at the top of this file says what happens to a checker that
# produces them: it gets switched off, and then it protects nothing.
#
# A quoted accuracy is always written with a `%` here, so that is the shape to
# check. The benchmark README keeps the fuller scan because its tables ARE
# measurements; these two documents only ever cite one.
stale_elsewhere = []
for name, text in elsewhere:
    for n in sorted(set(re.findall(r"\b(\d{1,3}\.\d)%", text))):
        if n not in sourced:
            stale_elsewhere.append((name, n + "%"))

for name, n in stale_elsewhere:
    print(f"  UNSOURCED in {name}: {n} — no figure in {os.path.basename(art_path)} produces it")

for k, v in missing:
    print(f"  MISSING from the prose: {k} = {v}")
for n in orphans:
    print(f"  UNSOURCED in the prose: {n} — no figure in {os.path.basename(art_path)} produces it")
for f in bad_fractions:
    print(f"  UNSOURCED fraction in the prose: {f} — no pair in {os.path.basename(art_path)} produces it")
if missing or orphans or bad_fractions or stale_elsewhere:
    print("\ndocs have drifted from the data they cite")
    sys.exit(1)
print(
    f"docs match {os.path.basename(art_path)} in both directions "
    f"({1 + len(elsewhere)} documents checked)"
)
