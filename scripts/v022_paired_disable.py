#!/usr/bin/env python3
"""Paired-disable discriminator for the EQUIVALENT_DID mutation rows
(operator 2026-09-28). For each retired defence-in-depth row R with subsuming
sibling(s) S, prove the retirement is genuine by the matrix:

    A present, B present  -> attack REFUSED   (baseline)
    A removed,  B present  -> attack REFUSED   (R's guard alone is redundant)
    A present, B removed  -> attack REFUSED    (S's guard alone is redundant)
    A removed,  B removed  -> attack SUCCEEDS   (the two are jointly load-bearing)

"A removed" applies R's own registry mutation; "B removed" applies each
subsuming sibling's registry mutation. The attack is R's own killing test:
normally it asserts the attack is refused, so "attack succeeds" == that test
FAILS. This demonstrates the SAME property reopening, not an unrelated red.

Refuses to run against a dirty tree, and restores every file it edits.
Writes an evidence manifest (schema axon-v022-paired-disable/1) and exits
non-zero unless every row's matrix holds.
"""
import hashlib
import importlib.util
import json
import os
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
spec = importlib.util.spec_from_file_location("mut", os.path.join(ROOT, "scripts/v022_g01_mutations.py"))
mut = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mut)
BY_ID = {r[0]: r for r in mut.MUTATIONS}


def sh(cmd):
    return subprocess.run(["bash", "-c", cmd], cwd=ROOT, capture_output=True, text=True)


def run_test(pkg, target, test):
    """True iff the row's test PASSES (attack refused)."""
    cmd = ("source scripts/lib_bounded_run.sh && "
           f"bounded_run 12G 1200 cargo test -q -p {pkg} {target} -- --exact {test}")
    r = sh(cmd)
    out = r.stdout + r.stderr
    if "could not compile" in out or "error[E" in out:
        return None, out  # a broken edit, not a verdict
    return (r.returncode == 0 and "1 passed" in out), out


def apply_edits(edits):
    """edits: list of (path, old, new). Returns a restore() closure or None if
    any old is not uniquely present."""
    originals = {}
    for path, old, _ in edits:
        p = os.path.join(ROOT, path)
        if p not in originals:
            originals[p] = open(p).read()
    for path, old, new in edits:
        p = os.path.join(ROOT, path)
        src = open(p).read()
        if src.count(old) != 1:
            for q, o in originals.items():
                open(q, "w").write(o)
            return None
        open(p, "w").write(src.replace(old, new, 1))

    def restore():
        for q, o in originals.items():
            open(q, "w").write(o)
    return restore


def edit_of(mid):
    r = BY_ID[mid]
    return (r[2], r[3], r[4])


def build_axon():
    return sh("source scripts/lib_bounded_run.sh && bounded_run 16G 1800 "
              "cargo build -q -p axon-core --no-default-features --bin axon").returncode == 0


def main():
    if sh("git status --porcelain -- crates").stdout.strip():
        sys.exit("refused: uncommitted changes under crates/ — paired-disable is evidence about a commit")
    commit = sh("git rev-parse HEAD").stdout.strip()
    records = []
    ok = True
    for rid, rec in mut.EQUIV_RECORD.items():
        row = BY_ID[rid]
        pkg, target, test = row[5], row[6], row[7]
        a = [edit_of(rid)]
        b = [edit_of(s) for s in rec["subsumed_by"]]

        def phase(edits, label):
            rest = apply_edits(edits) if edits else (lambda: None)
            if edits and rest is None:
                return "EDIT_NOT_APPLICABLE"
            try:
                if any(e[0].startswith("crates/axon-core/") for e in edits) and not build_axon():
                    return "BUILD_FAILED"
                passed, _ = run_test(pkg, target, test)
            finally:
                rest()
                # restore the shared interpreter for the next phase
                if any(e[0].startswith("crates/axon-core/") for e in edits):
                    build_axon()
            if passed is None:
                return "COMPILE_ERROR"
            return "ATTACK_REFUSED" if passed else "ATTACK_SUCCEEDS"

        baseline = phase([], "baseline")
        a_only = phase(a, "A")
        b_only = phase(b, "B")
        both = phase(a + b, "A+B")
        good = (baseline == "ATTACK_REFUSED" and a_only == "ATTACK_REFUSED"
                and b_only == "ATTACK_REFUSED" and both == "ATTACK_SUCCEEDS")
        ok &= good
        records.append({
            "mutation": rid, "status": "EQUIVALENT_DID", "property": rec["property"],
            "original_guard": {"file": row[2]}, "subsumed_by": rec["subsumed_by"],
            "live_killing_mutant": rec["killer"],
            "matrix": {"baseline": baseline, "original_guard_disabled": a_only,
                       "subsuming_guard_disabled": b_only, "both_disabled": both},
            "holds": good,
        })
        print(f"{'OK ' if good else 'BAD'} {rid}: base={baseline} A={a_only} B={b_only} A+B={both}",
              flush=True)
    # M204 (refactored): no current guard to disable; its property is covered
    # by live killing rows, recorded but not paired.
    for rid, rec in mut.STALE_REFACTORED.items():
        records.append({
            "mutation": rid, "status": "STALE_REFACTORED", "property": rec["property"],
            "original_guard": "refactored away (amendment 16)",
            "subsumed_by": rec["subsumed_by"], "live_killing_mutant": rec["killer"],
            "matrix": None, "holds": True,
        })
        print(f"OK  {rid}: refactored; property covered by live killers {rec['subsumed_by']}",
              flush=True)
    doc = {"schema": "axon-v022-paired-disable/1", "commit": commit, "all_hold": ok,
           "records": records}
    out = sys.argv[1] if len(sys.argv) > 1 else "governance/status/v022-psv-paired-disable.json"
    with open(os.path.join(ROOT, out), "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print(f"paired-disable: {sum(r['holds'] for r in records)}/{len(records)} hold -> {out}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
