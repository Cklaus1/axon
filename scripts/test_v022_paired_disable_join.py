#!/usr/bin/env python3
"""`v022_paired_disable.py --join` makes the status file only from shards that
cover every retirement record exactly once at THIS commit, each made from THIS
registry in a clean tree (C9 round 4). Each case below is
a join that must be refused, plus the control that must be accepted. Synthetic
shard files; no cargo. Run from a clean tree:

    python3 scripts/test_v022_paired_disable_join.py
"""
import json
import os
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
sys.path.insert(0, os.path.join(ROOT, "scripts"))
import v022_g01_mutations as mut  # noqa: E402
import v022_paired_disable as pd  # noqa: E402

HARNESS = os.path.join(ROOT, "scripts", "v022_paired_disable.py")
BLOBS = mut.registry_blobs()
COMMIT = subprocess.run(["git", "-C", ROOT, "rev-parse", "HEAD"], capture_output=True,
                        text=True, check=True).stdout.strip()
UNIVERSE = sorted(set(mut.EQUIVALENT_DID) | set(mut.STALE_REFACTORED), key=lambda r: int(r[1:]))


def shard(k, n, commit=COMMIT, drop=None, add=None, holds=True, rec_commit=None,
          blobs=None, clean=True, bad_edits=None):
    sel = [r for i, r in enumerate(UNIVERSE) if i % n == k]
    recs = [r for r in sel if r != drop] + ([add] if add else [])
    return {"schema": "axon-v022-paired-disable/2", "commit": commit, "all_hold": holds,
            "shard": f"{k}/{n}", "selected": sel,
            "registry_blobs": blobs or BLOBS, "tree_clean": clean,
            "records": [{"mutation": r, "holds": holds, "commit": rec_commit or commit,
                         "edits_sha256": "0" * 64 if r == bad_edits else pd.current_edits_digest(r)}
                        for r in recs]}


def join(tmp, docs):
    paths = []
    for i, d in enumerate(docs):
        p = os.path.join(tmp, f"s{i}.json")
        json.dump(d, open(p, "w"))
        paths.append(p)
    out = os.path.join(tmp, "joined.json")
    if os.path.exists(out):
        os.unlink(out)
    r = subprocess.run([sys.executable, HARNESS, "--join", out, *paths], cwd=ROOT,
                       capture_output=True, text=True)
    return r.returncode, r.stdout + r.stderr, out


def main():
    assert len(UNIVERSE) >= 3, UNIVERSE
    fails = []
    with tempfile.TemporaryDirectory() as tmp:
        other = "0" * 40
        cases = [
            ("a shard from another commit", [shard(0, 2), shard(1, 2, commit=other)]),
            ("a record from another commit", [shard(0, 2), shard(1, 2, rec_commit=other)]),
            ("a missing shard", [shard(0, 3), shard(1, 3)]),
            ("a shard given twice", [shard(0, 2), shard(0, 2)]),
            ("shards of different N", [shard(0, 2), shard(1, 3)]),
            ("a shard missing one of its records", [shard(0, 2), shard(1, 2, drop=UNIVERSE[1])]),
            ("a shard carrying another slice's record",
             [shard(0, 2, add=UNIVERSE[1]), shard(1, 2)]),
            ("a full-run file, not a shard", [{"commit": COMMIT, "records": []}]),
            # C9 round 4: a shard made from a locally edited registry or marker
            # file, or in a dirty tree, or executing edits that are not this
            # registry's rows.
            ("a shard from another registry",
             [shard(0, 2), shard(1, 2, blobs={f: "0" * 40 for f in BLOBS})]),
            ("a shard from a dirty tree", [shard(0, 2), shard(1, 2, clean=False)]),
            ("a shard with no recorded registry", [shard(0, 2), {**shard(1, 2), "registry_blobs": None}]),
            ("a record executed with other edits",
             [shard(0, 2), shard(1, 2, bad_edits=UNIVERSE[1])]),
        ]
        for name, docs in cases:
            code, out, path = join(tmp, docs)
            if code == 0 or os.path.exists(path) or "refused" not in out:
                fails.append(f"ATTACK: {name} was joined (exit {code}): {out.strip()}")
        # Control: every slice once, at this commit -> accepted, complete.
        code, out, path = join(tmp, [shard(1, 2), shard(0, 2)])
        doc = json.load(open(path)) if os.path.exists(path) else {}
        got = [r["mutation"] for r in doc.get("records", [])]
        if code != 0 or got != UNIVERSE or doc.get("commit") != COMMIT or not doc.get("all_hold"):
            fails.append(f"control: a complete join was not accepted as complete (exit {code}): {out}")
        # A complete join whose records do not all hold is written but fails.
        code, out, path = join(tmp, [shard(0, 2), shard(1, 2, holds=False)])
        if code == 0 or json.load(open(path)).get("all_hold") is not False:
            fails.append(f"ATTACK: a join with failing records reported success (exit {code})")
    for f in fails:
        print(f)
    print(f"paired-disable join: {'FAIL' if fails else 'PASS'} ({len(cases) + 2} cases)")
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
