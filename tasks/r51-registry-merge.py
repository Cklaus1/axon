#!/usr/bin/env python3
"""Resolve a merge conflict in governance/status/r51-deferred-tests.json.

Run in the integration worktree while the merge is in progress. Three-way merge per entry,
keyed by (file, name), from the index stages (1 base, 2 ours, 3 theirs): an entry one side
changed and the other did not takes the changed side; an entry both sides changed differently
is a conflict and the script fails listing it. `count` and `by_slice` are recomputed from the
merged entries. The output keeps ours' entry order, then theirs' new entries in their order.
"""
import collections
import json
import subprocess
import sys

P = "governance/status/r51-deferred-tests.json"


def stage(n):
    return json.loads(subprocess.run(["git", "show", f":{n}:{P}"], capture_output=True, text=True, check=True).stdout)


base, ours, theirs = stage(1), stage(2), stage(3)
key = lambda t: (t["file"], t["name"])
b = {key(t): t for t in base["tests"]}
o = {key(t): t for t in ours["tests"]}
th = {key(t): t for t in theirs["tests"]}
out, bad = [], []
for k in [key(t) for t in ours["tests"]] + [key(t) for t in theirs["tests"] if key(t) not in o]:
    ov, tv, bv = o.get(k), th.get(k), b.get(k)
    if ov == tv:
        v = ov
    elif ov == bv:
        v = tv
    elif tv == bv:
        v = ov
    else:
        bad.append((k, ov, tv))
        continue
    if v is not None:
        out.append(v)
if bad:
    for k, ov, tv in bad:
        print("CONFLICT", k, "\n ours:", ov, "\n theirs:", tv)
    sys.exit(1)
merged = dict(ours)
merged["tests"] = out
merged["count"] = len(out)
c = collections.Counter(t["slice"] for t in out)
merged["by_slice"] = {s: c.get(s, 0) for s in list(ours["by_slice"]) + [s for s in c if s not in ours["by_slice"]]}
open(P, "w").write(json.dumps(merged, indent=2, ensure_ascii=False) + "\n")
subprocess.run(["git", "add", P], check=True)
print(f"{P}: {len(out)} entries, by_slice {merged['by_slice']}")
