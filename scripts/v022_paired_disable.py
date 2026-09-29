#!/usr/bin/env python3
"""Paired-disable discriminator for the EQUIVALENT_DID mutation rows
(operator 2026-09-28). For each retired defence-in-depth row R with subsuming
sibling(s) S, prove the retirement is genuine by the matrix:

    A present, B present  -> attack REFUSED   (baseline)
    A removed,  B present  -> attack REFUSED   (A alone is redundant)
    A present, B removed  -> attack REFUSED   (B alone is redundant: A guards it)
    A removed,  B removed  -> attack SUCCEEDS  (jointly load-bearing, same attack)

ALL FOUR cells are required (operator rule, 2026-09-29). A row that cannot
show them for one named attack stays ACTIVE.

"A removed" applies R's own registry mutation; "B removed" applies each
subsuming sibling's registry mutation. The attack is R's own killing test:
normally it asserts the attack is refused, so "attack succeeds" == that test
FAILS. This demonstrates the SAME property reopening, not an unrelated red.

FULL-SUITE CONDITION (C8 certifying review wf_bff9835f-4a0): with A removed
alone, the WHOLE package suite must stay green (`retired_guard_full_suite` =
SUITE_OK), not just R's assigned test. A guard that enforces several
properties can pass the matrix above when its assigned test exercises only a
property that is independently covered; M254 did exactly that and was in fact
load-bearing. A row failing this condition is a false retirement.

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


def row_flags(target):
    """The feature flags of a row's cargo target (e.g. --no-default-features)
    without its --test/--lib selector, so the full-suite check builds the SAME
    configuration the row's own test runs in (not, say, the LLVM codegen suite
    for an interpreter-only row)."""
    out, skip = [], False
    for t in target.split():
        if skip:
            skip = False
            continue
        if t in ("--test", "--bin", "--example", "--bench"):
            skip = True
            continue
        if t != "--lib":
            out.append(t)
    return " ".join(out)


def full_suite_ok(pkg, flags=""):
    """True iff the WHOLE package suite passes. A retired (equivalent) guard,
    removed ALONE, must not break ANY test in the package — not merely its own
    --exact test. This closes the methodology gap the C8 certifying review
    (wf_bff9835f-4a0) found: M254 passed its assigned relabel test with its
    guard removed (that property is independently covered), yet removing the
    reverify_protected call broke three OTHER protected-class tests, so the
    guard was load-bearing, not equivalent. A retirement that fails this check
    is a FALSE retirement. Returns (ok, failing_tests) or (None, out) on a
    broken build."""
    import re as _re
    cmd = ("source scripts/lib_bounded_run.sh && "
           f"bounded_run 12G 1800 cargo test -q -p {pkg} {flags} 2>&1")
    r = sh(cmd)
    out = r.stdout + r.stderr
    if "could not compile" in out or "error[E" in out:
        return None, out
    fails = sorted(set(_re.findall(r"^\s*(\S+)\s+\.\.\.\s+FAILED", out, _re.M)))
    ok = (r.returncode == 0 and "test result: FAILED" not in out)
    return ok, fails


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
    # The guard set B (subsuming siblings) for each retired row A. A row is
    # retired ONLY if all four cells hold for ONE named attack (operator rule,
    # 2026-09-29): A+B present -> refused; A removed -> refused (by B);
    # B removed -> refused (by A); A+B removed -> the same attack SUCCEEDS.
    # "Asymmetric" (B alone reopens) is NOT a retirement: it means A does not
    # guard that attack at all, and may guard another route — exactly how M103
    # (development class) and M255 (store-writer route) hid until the C8 review.
    GUARD_SETS = {
        # The only row that survived the C9 re-audit (all four cells executed):
        # admission.rs's monitor key_id check is dominated by clearance_verifies,
        # which calls rooted_key with the identical arguments. M104, M209, M210
        # (store-writer misattribution), M103 (development class), M254 and
        # M255 (store-writer route) were all load-bearing and are ACTIVE.
        "M245": {"siblings": ["M264"], "kind": "pair",
                 "attack_marker": "monitor: activated on a revoked key"},
        # M58 (call_fn_frame's break/continue arm) vs M59 (contain_frame). ALL
        # PATHS: call_fn_frame has exactly ONE caller (interp.rs, in call_fn),
        # and it wraps the call in contain_frame, which maps break/continue to
        # the same panic. The attack is M58's OWN (function-body escapes only);
        # the older test also attacked a closure, a route M58 never guarded.
        "M58": {"siblings": ["M59"], "kind": "pair",
                "attack_marker": "escaped a function body"},
    }
    # Every retired row has a matrix and no active row has one.
    if set(GUARD_SETS) != set(mut.EQUIVALENT_DID):
        sys.exit(f"refused: GUARD_SETS {sorted(GUARD_SETS)} != EQUIVALENT_DID {sorted(mut.EQUIVALENT_DID)}")
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
                passed, out = run_test(pkg, target, test)
            finally:
                rest()
                if any(e[0].startswith("crates/axon-core/") for e in edits):
                    build_axon()
            if passed is None:
                return "COMPILE_ERROR"
            if passed:
                return "ATTACK_REFUSED"
            # A test can fail for a reason that is not the attack (a setup panic,
            # a different property's assertion). C9 re-audit: M209's "attack
            # succeeds" cell was a setup panic, and M104/M210's joint cells
            # failed on the VERIFIER iteration, i.e. another property. Only the
            # row's own attack message counts as the attack succeeding.
            if gs["attack_marker"] not in out:
                return "OTHER_FAILURE"
            return "ATTACK_SUCCEEDS"

        baseline = phase([])
        retired_only = phase(a)          # removing the retired guard alone
        joint = phase(a + b)             # retired + its subsuming siblings

        # Full-suite check (methodology fix, C8 review wf_bff9835f-4a0): the
        # retired guard removed ALONE must not break ANY test in the package,
        # not merely its own --exact test. M254 passed its assigned relabel
        # test with its guard removed yet broke three OTHER protected-class
        # tests — a load-bearing guard the old single-test matrix hid.
        def full_after(edits):
            rest = apply_edits(edits)
            if rest is None:
                return "EDIT_NOT_APPLICABLE", []
            try:
                if any(e[0].startswith("crates/axon-core/") for e in edits) and not build_axon():
                    return "BUILD_FAILED", []
                fok, fails = full_suite_ok(pkg, row_flags(target))
            finally:
                rest()
                if any(e[0].startswith("crates/axon-core/") for e in edits):
                    build_axon()
            if fok is None:
                return "COMPILE_ERROR", []
            return ("SUITE_OK" if fok else "SUITE_BROKEN"), fails
        full_state, full_fails = full_after(a)

        matrix = {"baseline": baseline, "retired_guard_disabled": retired_only,
                  "guard_set_disabled": joint, "guard_set": [rid] + sibs,
                  "retired_guard_full_suite": full_state}
        if full_fails:
            matrix["retired_guard_full_suite_failures"] = full_fails[:12]
        sib_only = phase(b)              # B removed, A present
        matrix["sibling_set_disabled"] = sib_only
        good = (baseline == "ATTACK_REFUSED" and retired_only == "ATTACK_REFUSED"
                and sib_only == "ATTACK_REFUSED" and joint == "ATTACK_SUCCEEDS"
                and full_state == "SUITE_OK")
        ok &= good
        records.append({
            "mutation": rid, "status": "EQUIVALENT_DID", "kind": gs["kind"],
            "property": rec["property"], "original_guard": {"file": row[2]},
            "subsumed_by": sibs, "live_killing_mutant": rec["killer"],
            "matrix": matrix, "holds": good,
        })
        print(f"{'OK ' if good else 'BAD'} {rid} [{gs['kind']}]: base={baseline} "
              f"retired_off={retired_only} sib_off={sib_only} set_off={joint} full_suite={full_state}",
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
