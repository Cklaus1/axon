#!/usr/bin/env python3
"""Run every R51-deferred test with the seal flipped on, grouped by test target.
Usage: tasks/r51-s6/probe.py ROOT OUTDIR. Prints per-target pass/fail and writes failing names."""
import collections, json, os, subprocess, sys

root, out = sys.argv[1], sys.argv[2]
os.makedirs(out, exist_ok=True)
d = json.load(open(os.path.join(root, "governance/status/r51-deferred-tests.json")))
groups = collections.defaultdict(list)
for x in d["tests"]:
    f = x["file"]
    pkg = f.split("/")[1]
    if "/tests/" in f:
        tgt = ["--test", os.path.basename(f)[:-3]]
    else:
        tgt = ["--lib"]
    groups[(pkg, tuple(tgt))].append(x["name"])
summary = []
for (pkg, tgt), names in sorted(groups.items()):
    cmd = ["cargo", "test", "-p", pkg, "--no-default-features" if pkg == "axon-core" else "--quiet", *tgt,
           "--", "--include-ignored", *names]
    cmd = [c for c in cmd if c != "--quiet"]
    r = subprocess.run(cmd, cwd=root, capture_output=True, text=True)
    log = r.stdout + r.stderr
    tag = f"{pkg}{'_'.join(tgt)}".replace("--", "_")
    open(os.path.join(out, tag + ".log"), "w").write(log)
    failed = [l.split()[1] for l in log.splitlines() if l.startswith("test ") and l.rstrip().endswith("FAILED")]
    passed = [l.split()[1] for l in log.splitlines() if l.startswith("test ") and l.rstrip().endswith("ok")]
    built = "error: could not compile" not in log
    summary.append((pkg, " ".join(tgt), len(names), len(passed), len(failed), built))
    print(f"{pkg} {' '.join(tgt)}: registry {len(names)}, ran ok {len(passed)}, failed {len(failed)}, built={built}", flush=True)
    for n in failed:
        print("   FAIL", n, flush=True)
json.dump(summary, open(os.path.join(out, "summary.json"), "w"), indent=1)
