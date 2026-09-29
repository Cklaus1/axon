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
"attack succeeds" == that test FAILS and the panic that fails it matches R's
attack marker (scripts/v022_attack_markers.py, shared with the mutation run).
This demonstrates the SAME property reopening, not an unrelated red.

STALE rows (C9 round 1): the old text being absent is not accepted as proof
that a guard is gone. A stale row must name a `replacement`, an ACTIVE row
mutating the guard's current form, and this script executes that
replacement's kill (baseline passes; mutated, the test fails on the
replacement's own attack marker).

    python3 scripts/v022_paired_disable.py [--only=M1,M2] [OUT.json]

--only re-executes just the named records and keeps every other record of
the existing file (each record carries the commit it was executed at).

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


def _workspace_packages():
    """{name: [workspace deps (normal, dev, build)]} from cargo metadata."""
    r = sh("cargo metadata --format-version 1 --no-deps")
    meta = json.loads(r.stdout)
    names = {p["name"] for p in meta["packages"]}
    return {p["name"]: sorted({d["name"] for d in p["dependencies"] if d["name"] in names})
            for p in meta["packages"]}


def consumer_packages(owner):
    """Every package whose tests exercise `owner`'s code, other than `owner`
    (C9 round 2, harness; the consumer-suite gap that hid M254):

    * its workspace REVERSE dependencies (they link `owner`'s library), and
    * for axon-core, every package whose tests or sources exec the `axon`
      INTERPRETER BINARY (they name `AXON_BIN`): axon-fabric's check runs,
      axon-psv's runner, axon-cortex's executor and the rest link nothing of
      axon-core, so no dependency edge names them.

    Derived from the tree, not a list, so a new consumer joins the cell."""
    pk = _workspace_packages()
    out = {n for n, deps in pk.items() if owner in deps}
    if owner == "axon-core":
        r = sh("grep -rl AXON_BIN crates/*/src crates/*/tests 2>/dev/null")
        out |= {line.split("/")[1] for line in r.stdout.split() if line.startswith("crates/")}
    out.discard(owner)
    return sorted(n for n in out if n in pk)


CONSUMER_BASELINE = {}
# One test thread per consumer suite: several consumers' tests write an
# executable stand-in and exec it while another test thread forks, and the
# fork's inherited write fd makes the exec fail ETXTBSY ("Text file busy").
# Measured on the CLEAN tree: axon-psv's runner suite failed 1 run in 3 that
# way inside the harness, which read as SUITE_BROKEN for rows it says nothing
# about (C9 round 2).
CONSUMER_FLAGS = "-- --test-threads=1"


def interpreter_env():
    """Consumers exec target/debug/axon: name it, so none of them falls back
    to a stale or ambient binary (axon-os looks under the WORKSPACE target
    dir, not CARGO_TARGET_DIR, and SKIPS when nothing is there). Consumers
    only: axon-core's own parity tests read AXON_BIN as a NATIVE (codegen)
    compiler, which the interpreter build is not."""
    target = os.environ.get("CARGO_TARGET_DIR", os.path.join(ROOT, "target"))
    return f"AXON_BIN={os.path.join(target, 'debug', 'axon')} "


def full_suite_ok(pkg, flags="", env=""):
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
           f"{env}bounded_run 12G 2400 cargo test -q -p {pkg} {flags} 2>&1")
    r = sh(cmd)
    out = r.stdout + r.stderr
    if "could not compile" in out or "error[E" in out:
        return None, out
    # Both libtest formats: `name ... FAILED` and, under -q, `name --- FAILED`.
    fails = sorted(set(_re.findall(r"^\s*(\S+)\s+(?:\.\.\.|---)\s+FAILED", out, _re.M)))
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
    argv = [a for a in sys.argv[1:] if not a.startswith("--only=")]
    only_arg = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--only=")), None)
    only = None if only_arg is None else set(only_arg.split(","))
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
        # (M209/M210 again EQUIVALENT since M360/M361: C9 round 1b, below.)
        "M245": {"siblings": ["M264"], "kind": "pair"},
        # M58 (call_fn_frame's break/continue arm) vs M59 (contain_frame). ALL
        # PATHS: call_fn_frame has exactly ONE caller (interp.rs, in call_fn),
        # and it wraps the call in contain_frame, which maps break/continue to
        # the same panic. The attack is M58's OWN (function-body escapes only);
        # the older test also attacked a closure, a route M58 never guarded.
        "M58": {"siblings": ["M59"], "kind": "pair"},
        # C9 round 1 (harness workstream): kills that were another check's
        # refusal. The attack is each row's own test, which accepts ANY
        # refusal (the layers are independent) and panics "ATTACK: …" on
        # acceptance; the all-paths argument is EQUIV_RECORD[...]["all_paths"].
        "M27": {"siblings": ["M30"], "kind": "pair"},
        "M29": {"siblings": ["M30"], "kind": "pair"},
        "M214": {"siblings": ["M233"], "kind": "pair"},
        "M216": {"siblings": ["M299"], "kind": "pair"},
        "M377": {"siblings": ["M378"], "kind": "pair"},
        "M378": {"siblings": ["M377"], "kind": "pair"},
        "M384": {"siblings": ["M29", "M30"], "kind": "set"},
        "M285": {"siblings": ["M385", "M289"], "kind": "set"},
        "M385": {"siblings": ["M285", "M289"], "kind": "set"},
        "M287": {"siblings": ["M290"], "kind": "pair"},
        "M288": {"siblings": ["M290"], "kind": "pair"},
        # C9 round 1b (LOOP). M19: EVL's D3 execution-leg filter vs the backend
        # join in verify_execution (M428), which runs right after it on the
        # same receipt. M209/M210: derive's rooted-identity checks vs the join
        # to the re-verified signer (M360/M361), which reverify_protected runs
        # for the same trial on every path (EQUIV_RECORD "all_paths").
        "M19": {"siblings": ["M428"], "kind": "pair"},
        "M209": {"siblings": ["M360"], "kind": "pair"},
        "M210": {"siblings": ["M361"], "kind": "pair"},
        # C9 round 1b (psv workstream). M152: the attack is an UNRELABELLED
        # signature made for another authority (the relabelled one is M153's
        # own attack). M418: a stranger key named as observer_key_id AND
        # signing the observation.
        "M152": {"siblings": ["M153"], "kind": "pair"},
        "M418": {"siblings": ["M339", "M340"], "kind": "set"},
        # C9 round 1b (fabric workstream): mutual pairs whose attack is each
        # pair's shared test; all-paths arguments in EQUIV_RECORD.
        "M139": {"siblings": ["M141"], "kind": "pair"},
        "M141": {"siblings": ["M139"], "kind": "pair"},
        "M183": {"siblings": ["M312"], "kind": "pair"},
        "M187": {"siblings": ["M400"], "kind": "pair"},
        "M400": {"siblings": ["M187"], "kind": "pair"},
        "M273": {"siblings": ["M401"], "kind": "pair"},
        "M401": {"siblings": ["M273"], "kind": "pair"},
        # C9 round 1b (core workstream): EQUIV_RECORD[...]["all_paths"].
        # C9 round 1b, integration: M04 vs the first-match rule M436 --
        # REINSTATED ACTIVE in C9 round 2 (harness): the order alone guards an
        # honest candidate's verdict, and its full-suite cell fails on that.
        "M60": {"siblings": ["M69"], "kind": "pair"},
        "M89": {"siblings": ["M86", "M96"], "kind": "set"},
        # C9 round 2 (harness): read_regular's regular-file check vs the
        # non-blocking open (M481) and the one-read rule (M335); service_leaf's
        # symlink check vs is_dir (M486) and the mode check (M327).
        "M482": {"siblings": ["M481", "M335"], "kind": "set"},
        "M487": {"siblings": ["M486", "M327"], "kind": "set"},
    }
    # Every retired row has a matrix and no active row has one.
    if set(GUARD_SETS) != set(mut.EQUIVALENT_DID):
        sys.exit(f"refused: GUARD_SETS {sorted(GUARD_SETS)} != EQUIVALENT_DID {sorted(mut.EQUIVALENT_DID)}")
    if only is not None:
        unknown = sorted(only - set(GUARD_SETS) - set(mut.STALE_REFACTORED))
        if unknown:
            sys.exit(f"--only: no retirement record for {unknown}")
    records = []
    # A row with no marker can never show its attack succeeding, so its joint
    # cell would read OTHER_FAILURE by construction (M58/M245, C9 round 1b).
    unmarked = sorted(r for r in GUARD_SETS if r not in mut.ATTACK_MARKERS)
    if unmarked:
        sys.exit(f"refused: GUARD_SETS rows with no attack marker {unmarked}")
    ok = True
    for rid, gs in GUARD_SETS.items():
        if only is not None and rid not in only:
            continue
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
            # row's own attack marker IN THE PANIC THAT FAILED THE TEST counts
            # as the attack succeeding (C9 round 1: the marker is the one the
            # mutation run uses, and a caught earlier panic no longer counts).
            if not mut.attack_succeeded(rid, out, test):
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
                # The row's package AND the crate that owns the guard: a
                # loop-contracts guard tested through axon-loop must leave
                # loop-contracts' own suite green too.
                owner = row[2].split("/")[1] if row[2].startswith("crates/") else pkg
                pkgs = pkg if owner == pkg else f"{pkg} -p {owner}"
                fok, fails = full_suite_ok(pkgs, row_flags(target))
                # ...and every CONSUMER of the owner crate, each in its own
                # default configuration (C9 round 2: the row's flags, e.g.
                # axon-core's --no-default-features, are not theirs).
                states = {}
                for c in consumers:
                    cok, cfails = full_suite_ok(c, CONSUMER_FLAGS, env=interpreter_env())
                    states[c] = "COMPILE_ERROR" if cok is None else ("SUITE_OK" if cok else "SUITE_BROKEN")
                    if cok is False:
                        # Named per consumer, ahead of the owner's (the record
                        # keeps the first 12).
                        fails = [f"{c}: {t}" for t in cfails][:6] + [f"{c}: (suite failed)"] + fails
            finally:
                rest()
                if any(e[0].startswith("crates/axon-core/") for e in edits):
                    build_axon()
            if fok is None or "COMPILE_ERROR" in states.values():
                return "COMPILE_ERROR", [], states
            ok_all = fok and all(v == "SUITE_OK" for v in states.values())
            return ("SUITE_OK" if ok_all else "SUITE_BROKEN"), fails, states
        owner_crate = row[2].split("/")[1] if row[2].startswith("crates/") else pkg
        consumers = [c for c in consumer_packages(owner_crate) if c != pkg]
        # A consumer suite that is red on the CLEAN tree says nothing about the
        # guard (an environment failure must not read as evidence, and must
        # not read as a pass either): it makes the cell CONSUMER_BASELINE_BROKEN.
        cons_base = {}
        for c in consumers:
            if c not in CONSUMER_BASELINE:  # the clean tree is the same for every row
                cok, _ = full_suite_ok(c, CONSUMER_FLAGS, env=interpreter_env())
                CONSUMER_BASELINE[c] = "SUITE_OK" if cok else ("COMPILE_ERROR" if cok is None else "SUITE_BROKEN")
            cons_base[c] = CONSUMER_BASELINE[c]
        if any(v != "SUITE_OK" for v in cons_base.values()):
            full_state, full_fails, cons_states = "CONSUMER_BASELINE_BROKEN", [
                f"{c}: {v} on the clean tree" for c, v in cons_base.items() if v != "SUITE_OK"], {}
        else:
            full_state, full_fails, cons_states = full_after(a)

        matrix = {"baseline": baseline, "retired_guard_disabled": retired_only,
                  "guard_set_disabled": joint, "guard_set": [rid] + sibs,
                  "retired_guard_full_suite": full_state,
                  "full_suite_packages": [pkg] + ([owner_crate] if owner_crate != pkg else []) + consumers,
                  "consumer_suites": cons_states}
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
            "all_paths": rec.get("all_paths"),
            "matrix": matrix, "holds": good, "commit": commit,
        })
        print(f"{'OK ' if good else 'BAD'} {rid} [{gs['kind']}]: base={baseline} "
              f"retired_off={retired_only} sib_off={sib_only} set_off={joint} full_suite={full_state}",
              flush=True)
    # STALE rows (C9 round 1). "The old text is absent" shows only that the TEXT
    # changed: M204 was recorded stale while its guard lived on, refactored,
    # with no row. A stale row holds ONLY if it names a replacement that is an
    # ACTIVE row (not retired, its text present exactly once) AND that
    # replacement is killed here by its OWN attack: baseline passes, the
    # replacement's mutation fails the test on its attack marker.
    for rid, rec in mut.STALE_REFACTORED.items():
        if only is not None and rid not in only:
            continue
        rep = rec.get("replacement")
        old_present = open(os.path.join(ROOT, BY_ID[rid][2])).read().count(BY_ID[rid][3]) > 0
        rep_state = "NO_REPLACEMENT"
        if rep in BY_ID and rep not in mut.RETIRED:
            rrow = BY_ID[rep]
            pkg, target, test = rrow[5], rrow[6], rrow[7]
            base_ok, _ = run_test(pkg, target, test)
            rest = apply_edits([edit_of(rep)])
            if not base_ok:
                rep_state = "REPLACEMENT_BASELINE_FAILS"
            elif rest is None:
                rep_state = "REPLACEMENT_NOT_APPLICABLE"
            else:
                core = rrow[2].startswith("crates/axon-core/")
                try:
                    if core:
                        build_axon()
                    passed, out = run_test(pkg, target, test)
                finally:
                    rest()
                    if core:
                        build_axon()
                if passed is None:
                    rep_state = "REPLACEMENT_COMPILE_ERROR"
                elif passed:
                    rep_state = "REPLACEMENT_SURVIVES"
                elif mut.attack_succeeded(rep, out, test):
                    rep_state = "REPLACEMENT_KILLED"
                else:
                    rep_state = "REPLACEMENT_REFUSED_ELSEWHERE"
        elif rep in BY_ID:
            rep_state = "REPLACEMENT_RETIRED"
        holds = (not old_present) and rep_state == "REPLACEMENT_KILLED"
        ok &= holds
        records.append({
            "mutation": rid, "status": "STALE_REFACTORED", "property": rec["property"],
            "original_guard": rec["how"], "old_string_present": old_present,
            "replacement": rep, "replacement_state": rep_state,
            "matrix": None, "holds": holds, "commit": commit,
        })
        print(f"{'OK ' if holds else 'BAD'} {rid}: stale ({rec['how']}); replacement {rep}: {rep_state}",
              flush=True)
    out = argv[0] if argv else "governance/status/v022-psv-paired-disable.json"
    path = os.path.join(ROOT, out)
    if only is not None:
        # A partial run replaces ONLY the named rows' records in the existing
        # file; every other record keeps the commit it was executed at.
        try:
            prev = json.load(open(path))
            kept = [r for r in prev.get("records", [])
                    if r["mutation"] not in only
                    and (r["mutation"] in GUARD_SETS or r["mutation"] in mut.STALE_REFACTORED)]
        except (OSError, ValueError):
            kept = []
        for r in kept:
            r.setdefault("commit", prev.get("commit"))
        records = kept + records
        ok = all(r["holds"] for r in records)
    missing = sorted((set(GUARD_SETS) | set(mut.STALE_REFACTORED)) - {r["mutation"] for r in records})
    if missing:
        print(f"BAD no record for {missing}", flush=True)
        ok = False
    doc = {"schema": "axon-v022-paired-disable/2", "commit": commit, "all_hold": ok,
           "records": records}
    with open(path, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print(f"paired-disable: {sum(r['holds'] for r in records)}/{len(records)} hold -> {out}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
