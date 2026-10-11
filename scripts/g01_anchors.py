#!/usr/bin/env python3
"""g01_anchors.py — every ACTIVE G01 mutation row still anchors exactly once.

    python3 scripts/g01_anchors.py [ROOT]

A row of scripts/v022_g01_mutations.py mutates its `old` text in its file. When
that text is absent or occurs more than once, the row mutates nothing (or the
wrong place), and the full mutation run reports it as stale only after hours.
This reads the registry and counts each active row's `old` text in its file:
seconds, no cargo. Rows in RETIRED, SIBLING_ONLY or STALE_REFACTORED are not
active and are skipped.

v022_refusal_coverage.py makes the same count, but only for rows whose file is
in its protected scope; this covers every active row (scripts/, profiles/ and
the other crates included).

ROOT defaults to the repository this script lives in. Exit 0 when every active
row's `old` text occurs exactly once; 1 otherwise, listing each bad row as
`<id> <file> <count>`.
"""
import importlib.util
import os
import sys

root = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
# Importing the registry must not create the harness's per-run TMPDIR.
os.environ["V022_KEEP_TMPDIR"] = "1"
sys.path.insert(0, os.path.join(root, "scripts"))
spec = importlib.util.spec_from_file_location("v022_g01_mutations", os.path.join(root, "scripts", "v022_g01_mutations.py"))
m = importlib.util.module_from_spec(spec)
spec.loader.exec_module(m)

skip = set(m.RETIRED) | set(m.SIBLING_ONLY) | set(m.STALE_REFACTORED)
bad, n = [], 0
for row in m.MUTATIONS:
    rid, f, old = row[0], row[2], row[3]
    if rid in skip:
        continue
    n += 1
    with open(os.path.join(root, f), encoding="utf-8") as fh:
        c = fh.read().count(old)
    if c != 1:
        bad.append((rid, f, c))
print(f"g01_anchors: checked {n} rows, {len(bad)} bad")
for b in bad:
    print(*b)
sys.exit(1 if bad else 0)
