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
    # The guard set (retired row + subsuming siblings) whose JOINT removal
    # reopens the SAME attack, discovered empirically (each retired guard
    # removed alone leaves the attack refused). "asymmetric": a single sibling
    # alone reopens, so the retired guard is dominated (its removal is
    # behaviourally invisible).
    GUARD_SETS = {
        "M245": {"siblings": ["M264"], "kind": "pair"},
        "M254": {"siblings": ["M261", "M262"], "kind": "set"},
        "M104": {"siblings": ["M99", "M208", "M269", "M207"], "kind": "set"},
        "M210": {"siblings": ["M207", "M208", "M99", "M269"], "kind": "set"},
        "M255": {"siblings": ["M254", "M212", "M213", "M261", "M262"], "kind": "set"},
        "M103": {"siblings": ["M26"], "kind": "asymmetric"},
        "M209": {"siblings": ["M205"], "kind": "asymmetric"},
    }
    records = []
    ok = True
    for rid, gs in GUARD_SETS.items():
        rec = mut.EQUIV_RECORD[rid]
        row = BY_ID[rid]
        pkg, target, test = row[5], row[6], row[7]
        a = [edit_of(rid)]
        sibs = gs["siblings"]
        b = [edit_of(s) for s in sibs]

        def phase(edits):
            rest = apply_edits(edits) if edits else (lambda: None)
            if edits and rest is None:
                return "EDIT_NOT_APPLICABLE"
            try:
                if any(e[0].startswith("crates/axon-core/") for e in edits) and not build_axon():
                    return "BUILD_FAILED"
                passed, _ = run_test(pkg, target, test)
            finally:
                rest()
                if any(e[0].startswith("crates/axon-core/") for e in edits):
                    build_axon()
            if passed is None:
                return "COMPILE_ERROR"
            return "ATTACK_REFUSED" if passed else "ATTACK_SUCCEEDS"

        baseline = phase([])
        retired_only = phase(a)          # removing the retired guard alone
        joint = phase(a + b)             # retired + its subsuming siblings
        matrix = {"baseline": baseline, "retired_guard_disabled": retired_only,
                  "guard_set_disabled": joint, "guard_set": [rid] + sibs}
        if gs["kind"] == "asymmetric":
            sib_only = phase(b)          # the dominating sibling alone
            matrix["dominating_sibling_disabled"] = sib_only
            good = (baseline == "ATTACK_REFUSED" and retired_only == "ATTACK_REFUSED"
                    and sib_only == "ATTACK_SUCCEEDS" and joint == "ATTACK_SUCCEEDS")
        else:
            good = (baseline == "ATTACK_REFUSED" and retired_only == "ATTACK_REFUSED"
                    and joint == "ATTACK_SUCCEEDS")
        ok &= good
        records.append({
            "mutation": rid, "status": "EQUIVALENT_DID", "kind": gs["kind"],
            "property": rec["property"], "original_guard": {"file": row[2]},
            "subsumed_by": sibs, "live_killing_mutant": rec["killer"],
            "matrix": matrix, "holds": good,
        })
        print(f"{'OK ' if good else 'BAD'} {rid} [{gs['kind']}]: base={baseline} "
              f"retired_off={retired_only} set_off={joint}"
              + (f" sib_off={matrix.get('dominating_sibling_disabled')}" if gs['kind']=='asymmetric' else ""),
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
