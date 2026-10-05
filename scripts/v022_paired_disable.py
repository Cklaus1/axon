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
the existing file (each record carries the commit it was executed at). A kept
record must still be CURRENT (C9 round 3; widened C9 round 4): the git blob of
the row's file, of its test file(s) and of every sibling's file must be the
same now as at the record's commit, NO file of the owner package or of any
consumer package (whose full suites the record vouches for) may have changed
since, and the registry edits and attack marker it was executed with must be
this registry's. A stale kept record refuses the run; --reexecute-stale runs
it again instead. --check-stale only lists them.

The tree must be clean -- ALL of it, not only crates/ (C9 round 4: 24 rows
guard files in scripts/ and profiles/, and the registry itself is in
scripts/). Every run and shard records the registry and marker blobs, each
record's edit digest, and the uid and environment its cells ran under;
--join refuses shards that disagree with this tree's registry. Cells run with
AXON / AXON_BIN / CORTEX_BIN removed (a consumer cell is handed the
interpreter this run built), a full-suite cell reports the root-only tests
that SKIPPED rather than counting them as passes, and every executable a
script built into the workspace target dir from a MUTATED tree is removed
after the cell, with this run's prerequisites rebuilt.

    python3 scripts/v022_paired_disable.py --check-stale [STATUS.json]

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


def cargo_target_dir():
    """The directory cargo builds into from ROOT, as CARGO resolves it
    (CARGO_TARGET_DIR, then any config's build.target-dir, then ROOT/target):
    never a guess (scripts/lib/axon_bin.sh, C9 round 4)."""
    r = subprocess.run(["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT,
                       capture_output=True, text=True)
    try:
        return json.loads(r.stdout)["target_directory"]
    except (ValueError, KeyError):
        sys.exit(f"refused: cannot tell where cargo builds (cargo metadata: {r.stderr.strip()[-300:]})")


def build_tests(pkg, flags, env=""):
    """Build the tests a cell runs, ALONE: (ok, output). Whether the cell's
    edit broke the build is the outcome of THIS cargo invocation, never a
    string found in a test's output: tests build and exec workspace binaries
    themselves (script_spawn::workspace_bin), and a nested build that fails
    prints `could not compile` into the test's output (C9 round 4,
    amendment 59; M58's consumer baseline read CONSUMER_BASELINE_BROKEN)."""
    build_flags = f" {flags} ".split(" -- ")[0].strip()
    r = sh(f"source scripts/lib_bounded_run.sh && {mut.UNSET_AMBIENT}"
           f"{env}bounded_run 12G 2400 cargo test -q -p {pkg} {build_flags} --no-run 2>&1")
    return r.returncode == 0, r.stdout + r.stderr


def run_test(pkg, target, test):
    """True iff the row's test PASSES (attack refused). None: the build
    failed. A cell that did not pass keeps its whole output (amendment 59)."""
    built, bout = build_tests(pkg, target)
    if not built:
        mut.keep_output(f"pd-cell-build-{pkg}", bout)
        return None, bout  # a broken edit (or a killed build), not a verdict
    cmd = (f"source scripts/lib_bounded_run.sh && {mut.UNSET_AMBIENT}"
           f"bounded_run 12G 1200 cargo test -q -p {pkg} {target} -- --exact {test}")
    r = sh(cmd)
    out = r.stdout + r.stderr
    passed = r.returncode == 0 and "1 passed" in out
    if not passed:
        mut.keep_output(f"pd-cell-{pkg}-{test.split('::')[-1]}", out)
    return passed, out


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
# (pkg, flags, env) -> the tests the last such cell reported as SKIPPED.
CELL_SKIPS = {}
# One test thread per consumer suite: several consumers' tests write an
# executable stand-in and exec it while another test thread forks, and the
# fork's inherited write fd makes the exec fail ETXTBSY ("Text file busy").
# Measured on the CLEAN tree: axon-psv's runner suite failed 1 run in 3 that
# way inside the harness, which read as SUITE_BROKEN for rows it says nothing
# about (C9 round 2).
CONSUMER_FLAGS = "-- --test-threads=1"


def interpreter_env():
    """Consumers exec the interpreter this harness built: name it, so none of
    them falls back to a stale or ambient binary (axon-os looks under the
    WORKSPACE target dir, not CARGO_TARGET_DIR, and SKIPS when nothing is
    there). Consumers only: axon-core's own parity tests read AXON_BIN as a
    NATIVE (codegen) compiler, which the interpreter build is not."""
    return f"AXON_BIN={os.path.join(cargo_target_dir(), 'debug', 'axon')} "


# The marker a root-only (or host-dependent) test prints when it returns
# without exercising anything: `eprintln!("skipped: ...")` across the Fabric
# suites. libtest reports such a test as PASSED; --show-output lets the cell
# see and COUNT it as a skip (C9 round 4).
SKIP_MARK = "skipped:"


def skipped_tests(out):
    """Tests whose captured output (under --show-output) says they skipped."""
    import re as _re
    names, cur = [], None
    for line in out.splitlines():
        m = _re.match(r"^---- (\S+) std(?:out|err) ----$", line)
        if m:
            cur = m.group(1)
            continue
        if line.startswith(("successes:", "failures:", "test result:")):
            cur = None
        elif cur and line.strip().startswith(SKIP_MARK) and cur not in names:
            names.append(cur)
    return names


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
    show = f"{flags} --show-output" if " -- " in f" {flags} " else f"{flags} -- --show-output"
    # The build first, alone: a broken build is THIS invocation's failure,
    # and its output is kept like any other cell's (amendment 59: the early
    # return used to come BEFORE the keep, so a compile error kept nothing).
    built, bout = build_tests(pkg, flags, env)
    if not built:
        mut.keep_output(f"pd-suite-build-{pkg}", bout)
        return None, bout
    # The bound is for the WHOLE package suite (C9 round 4b, final): at 2400 s
    # the axon-fabric suite (~21 min idle, --test-threads=1) was cut mid-binary
    # under a 6-shard load (M214's full-suite cell at 78832d1b: readiness_attribution
    # stopped after 38 of 40 tests, no failure printed) and read SUITE_BROKEN.
    cmd = (f"source scripts/lib_bounded_run.sh && {mut.UNSET_AMBIENT}"
           f"{env}bounded_run 12G {SUITE_BOUND_S} cargo test -q -p {pkg} {show} 2>&1")
    r = sh(cmd)
    out = r.stdout + r.stderr
    if r.returncode in (124, 137):
        # A suite cut by its wall-clock or memory bound proves nothing about
        # any test: it is named as such, never left to read as a failure.
        what = "TIMEOUT" if r.returncode == 124 else "RESOURCE_EXHAUSTED"
        out += f"\n[paired-disable] the suite was cut by bounded_run ({what}, {SUITE_BOUND_S} s / 12G)\n"
        mut.keep_output(f"pd-suite-{pkg}", out)
        CELL_SKIPS.update({(pkg, flags, env): skipped_tests(out)})
        return False, [f"<{what}: the whole suite exceeded bounded_run {SUITE_BOUND_S} s / 12G>"]
    # The tests are built: compiler text in this output is a TEST's (a nested
    # build it ran, or a diagnostic it printed), judged as that test's result.
    # Both libtest formats: `name ... FAILED` and, under -q, `name --- FAILED`.
    fails = sorted(set(_re.findall(r"^\s*(\S+)\s+(?:\.\.\.|---)\s+FAILED", out, _re.M)))
    ok = (r.returncode == 0 and "test result: FAILED" not in out)
    CELL_SKIPS[(pkg, flags, env)] = skipped_tests(out)
    if not ok:
        # Keep the failing cell's whole output: a failure that does not
        # reproduce is diagnosable only from the panic it actually printed.
        mut.keep_output(f"pd-suite-{pkg}", out)
    return ok, fails


# Wall-clock bound for one whole-package suite cell (full_suite_ok).
SUITE_BOUND_S = 7200


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


def test_files(row):
    """The source files a row's test is made of: for `--lib`, the module
    file the test path names; for `--test NAME`, tests/NAME.rs and every file
    under the package's tests/common. None when they cannot be found (the
    record then cannot be shown current)."""
    pkg, target, test = row[5], row[6], row[7]
    base = f"crates/{pkg}"
    t = target.split()
    if "--test" in t:
        name = t[t.index("--test") + 1]
        files = [f"{base}/tests/{name}.rs"]
        common = os.path.join(ROOT, base, "tests", "common")
        for d, _, fs in os.walk(common):
            files += [os.path.relpath(os.path.join(d, f), ROOT) for f in sorted(fs)]
        return files
    if "--lib" in t:
        segs = test.split("::")[:-1]
        segs = [x for x in segs if x != "tests"]
        for cand in (f"{base}/src/{'/'.join(segs)}.rs", f"{base}/src/{'/'.join(segs)}/mod.rs"):
            if segs and os.path.exists(os.path.join(ROOT, cand)):
                return [cand]
        return None
    return None


def blob_at(commit, path):
    r = sh(f"git rev-parse --verify --quiet {commit}:{path}")
    return r.stdout.strip() or None if r.returncode == 0 else None


def blob_now(path):
    if not os.path.exists(os.path.join(ROOT, path)):
        return None
    return sh(f"git hash-object -- {path}").stdout.strip() or None


def stale_reasons(record, guard_sets):
    """Why a kept record is no longer evidence about this tree (empty: it is
    current). Every file its matrix depended on must be byte-identical (by
    git blob) to what it was at the record's commit."""
    rid, commit = record["mutation"], record.get("commit")
    if not commit or sh(f"git cat-file -e {commit}^{{commit}}").returncode != 0:
        return [f"its commit {commit!r} is not in this repository"]
    ids = [rid]
    if rid in guard_sets:
        ids += guard_sets[rid]["siblings"]
    elif rid in mut.STALE_REFACTORED and mut.STALE_REFACTORED[rid].get("replacement"):
        ids.append(mut.STALE_REFACTORED[rid]["replacement"])
    paths = set()
    for i in ids:
        if i not in BY_ID:
            return [f"{i} is no longer a registry row"]
        paths.add(BY_ID[i][2])
    tf = test_files(BY_ID[ids[-1] if rid in mut.STALE_REFACTORED else rid])
    if tf is None:
        return [f"the test files of {rid} cannot be located"]
    paths |= set(tf)
    out = []
    for path in sorted(paths):
        then, now = blob_at(commit, path), blob_now(path)
        if then != now:
            out.append(f"{path} changed since {commit[:8]}")
    # The record's full-suite cells vouch for the WHOLE owner package and every
    # consumer package, and its all-paths argument reasons about files the list
    # above never named (M602's bin/axon-custodian.rs, M186's route
    # derivation): any change in those packages makes it stale (C9 round 4).
    row = BY_ID[ids[-1] if rid in mut.STALE_REFACTORED else rid]
    owner = row[2].split("/")[1] if row[2].startswith("crates/") else row[5]
    pkgs = sorted({owner, row[5], *consumer_packages(owner)})
    changed = sh(f"git diff --name-only {commit} -- " + " ".join(f"crates/{p}" for p in pkgs)).stdout.split()
    if changed:
        out.append(f"{len(changed)} file(s) of the owner/consumer packages {pkgs} changed since "
                   f"{commit[:8]} (e.g. {changed[0]})")
    # ...and it was executed with exactly this registry's edits and marker.
    if record.get("edits_sha256") != current_edits_digest(rid):
        out.append("its edits or attack marker are not this registry's (or were not recorded)")
    return out


def build_axon():
    return sh("source scripts/lib_bounded_run.sh && bounded_run 16G 1800 "
              "cargo build -q -p axon-core --no-default-features --bin axon").returncode == 0


def after_cell(edits, spawned):
    """After a cell that ran a MUTATED tree, the next cell must meet no binary
    built from it (C9 round 4, EQUIVALENCE (6d)), and pay nothing when nothing
    can be stale (amendment 59; a 2-shard run spent ~45 min per record
    rebuilding every prerequisite after every cell):

    * scrub the workspace target dir only when the cell ran a script that
      builds (`spawned`: the row's test target, or any suite the cell ran,
      calls the spawn helper) from a tree edited under crates/;
    * rebuild a prerequisite only when it is missing or its sha256 is not the
      clean build's recorded digest;
    * rebuild and byte-compare the interpreter always (a no-op build when
      nothing changed), re-running axon-core's build script if needed: a
      mutated cell can leave it `-dirty`, and a restoring build keeps it.

    A prerequisite that cannot be restored is the record's failure."""
    if spawned and any(e[0].startswith("crates/") for e in edits):
        mut.scrub_workspace_binaries()
    for what, cmd, arts in PREREQS:
        if what == "axon" or all(
                os.path.exists(a) and mut.sha(a) == PREREQ_SHA.get(a) for a in arts):
            continue
        r = sh(f"source scripts/lib_bounded_run.sh && bounded_run 16G 1800 {cmd}")
        bad = [a for a in arts if not os.path.exists(a) or mut.sha(a) != PREREQ_SHA.get(a)]
        if r.returncode != 0 or bad:
            INTERP_FAULTS.append(f"{what}: not restored from the clean tree ({bad or cmd})")
    if INTERP:
        why = mut.restore_interpreter(INTERP["path"], INTERP["sha256"])
        if why:
            INTERP_FAULTS.append(why)


def prereq_artifacts():
    """(what, build command, the files it leaves) for every prerequisite a
    cell execs, the files named from cargo's own metadata (the bins of
    axon-cortex), never a guessed list."""
    tgt = cargo_target_dir()
    meta = json.loads(subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps"], cwd=ROOT,
        capture_output=True, text=True).stdout or "{}")
    cortex = [t["name"] for p in meta.get("packages", []) if p["name"] == "axon-cortex"
              for t in p["targets"] if "bin" in t["kind"]]
    return [
        ("axon", "cargo build -q -p axon-core --no-default-features --bin axon",
         [os.path.join(tgt, "debug", "axon")]),
        ("cortex bins", "cargo build -q -p axon-cortex --bins",
         [os.path.join(tgt, "debug", n) for n in cortex]),
        ("psv_dev", "cargo build -q -p axon-psv --example psv_dev",
         [os.path.join(tgt, "debug", "examples", "psv_dev")]),
    ]


# Every prerequisite and the sha256 of each file the clean build left.
PREREQS = []
PREREQ_SHA = {}


# The run's interpreter (path, sha256), recorded once the prerequisites are
# built from the clean tree; and every failure to restore it after a cell.
INTERP = {}
INTERP_FAULTS = []


def edits_digest(rid, siblings):
    """sha256 of exactly what a record executed: the retired row's and each
    sibling's old/new text, and the retired row's attack marker."""
    return hashlib.sha256(json.dumps(
        [mut.row_digest(BY_ID[i]) for i in [rid, *siblings]] + [mut.ATTACK_MARKERS.get(rid)],
        sort_keys=True).encode()).hexdigest()


def environment():
    """Who and where the cells ran: a full-suite cell with root-only tests
    means something different as root, and with or without /etc/axon."""
    return {"euid": os.geteuid(), "etc_axon_present": os.path.isdir("/etc/axon"),
            "unset": list(mut.AMBIENT_BINARY_VARS), "consumer_axon_bin": interpreter_env().strip()}


def build_prereqs():
    """Every binary a full-suite cell execs, built for THIS tree into THIS
    target dir, before any cell runs. A missing one (the cortex bins, say, in
    a fresh shard target) breaks every cell of the suite that needs it for a
    reason that says nothing about the guard, so a run without them is
    refused rather than recorded as a page of SUITE_BROKEN."""
    for what, cmd, _ in prereq_artifacts():
        if sh(f"source scripts/lib_bounded_run.sh && bounded_run 16G 1800 {cmd}").returncode != 0:
            sys.exit(f"refused: could not build {what} ({cmd}); no cell runs without it")


def matrix_holds(m):
    """Whether a four-cell matrix holds (amendment 74): base, A-off and B-off
    refuse the attack, A+B-off lets it succeed, the whole suite stays green, no
    interpreter fault. ONE definition: the run that writes a record and the
    join that reads it judge by it, so a shard's `holds` label is never taken
    on trust."""
    return (isinstance(m, dict)
            and m.get("baseline") == "ATTACK_REFUSED"
            and m.get("retired_guard_disabled") == "ATTACK_REFUSED"
            and m.get("sibling_set_disabled") == "ATTACK_REFUSED"
            and m.get("guard_set_disabled") == "ATTACK_SUCCEEDS"
            and m.get("retired_guard_full_suite") == "SUITE_OK"
            and not m.get("interpreter_not_restored"))


def stale_holds(rec):
    """A STALE_REFACTORED record holds when its old text is gone and its named
    replacement was killed by its own attack (the run adds: no interpreter
    fault, which a record does not carry)."""
    return rec.get("old_string_present") is False and rec.get("replacement_state") == "REPLACEMENT_KILLED"


def record_derivable_holds(r):
    """What a record's own evidence supports. None: not derivable."""
    if r.get("status") == "STALE_REFACTORED":
        return stale_holds(r)
    if r.get("status") == "EQUIVALENT_DID":
        return matrix_holds(r.get("matrix"))
    return None


def join_shards(argv, commit, universe):
    """`--join OUT S0.json S1.json ...`: the status file from shard runs. It
    refuses unless every shard was executed at THIS commit (the registry the
    coverage is judged against is this commit's, and the tree is clean), the
    shards are exactly 0..N-1 of one N, each carries exactly the records its
    slice selects, and together they cover every record exactly once."""
    if len(argv) < 2:
        sys.exit("usage: --join OUT.json SHARD.json...")
    if mut.uncommitted():
        sys.exit("refused: --join judges coverage against this commit's registry; the tree is not clean")
    docs = [json.load(open(p)) for p in argv[1:]]
    bad = [p for p, d in zip(argv[1:], docs) if d.get("commit") != commit]
    if bad:
        sys.exit(f"refused: shard(s) {bad} were not executed at {commit[:8]}")
    # Each shard ran from THIS registry, in a clean tree (C9 round 4): the same
    # registry/marker blobs, and every record's edits the ones this registry
    # holds -- a shard made from a locally edited registry is refused.
    here = mut.registry_blobs()
    for p, d in zip(argv[1:], docs):
        if d.get("registry_blobs") != here:
            sys.exit(f"refused: shard {p} ran from registry/marker blobs {d.get('registry_blobs')}, "
                     f"not this tree's {here}")
        if d.get("tree_clean") is not True:
            sys.exit(f"refused: shard {p} does not record a clean tree")
    try:
        slices = [tuple(int(x) for x in d["shard"].split("/")) for d in docs]
    except (KeyError, AttributeError, ValueError):
        sys.exit("refused: an input is not a shard run (no shard K/N)")
    ns = {n for _, n in slices}
    if len(ns) != 1 or sorted(k for k, _ in slices) != list(range(ns.pop())):
        sys.exit(f"refused: shards {sorted(slices)} are not exactly 0..N-1 of one N")
    records, seen = [], []
    for (k, n), d in zip(slices, docs):
        want = [r for i, r in enumerate(universe) if i % n == k]
        got = [r["mutation"] for r in d["records"]]
        if sorted(d.get("selected", [])) != sorted(want) or sorted(got) != sorted(want):
            sys.exit(f"refused: shard {k}/{n} does not hold exactly its slice at this commit "
                     f"(want {want}, has {sorted(got)})")
        if any(r.get("commit", commit) != commit for r in d["records"]):
            sys.exit(f"refused: shard {k}/{n} carries a record from another commit")
        for r in d["records"]:
            if r.get("edits_sha256") != current_edits_digest(r["mutation"]):
                sys.exit(f"refused: shard {k}/{n} executed {r['mutation']} with edits that are not "
                         "this registry's")
            # Amendment 74: HOLDS is recomputed from the recorded cells with
            # the run's own predicate, never taken from the label. A record
            # whose evidence cannot be derived is refused. A stale record that
            # says it does not hold is allowed to (the run also counts an
            # interpreter fault, which a record does not carry).
            want = record_derivable_holds(r)
            if want is None:
                sys.exit(f"refused: shard {k}/{n}: {r['mutation']} carries no matrix or replacement "
                         "state to derive its verdict from")
            if bool(r.get("holds")) != want and not (r.get("status") == "STALE_REFACTORED" and not r.get("holds")):
                sys.exit(f"refused: shard {k}/{n}: {r['mutation']} claims holds={bool(r.get('holds'))} "
                         f"but its recorded cells give {want}")
        records += d["records"]
        seen += got
    dup = sorted({r for r in seen if seen.count(r) > 1})
    if dup or sorted(seen) != sorted(universe):
        sys.exit(f"refused: coverage is not every record exactly once (duplicates {dup}, "
                 f"missing {sorted(set(universe) - set(seen))})")
    for r in records:
        r["commit"] = commit
    ok = all(r["holds"] for r in records)
    records.sort(key=lambda r: int(r["mutation"][1:]))
    doc = {"schema": "axon-v022-paired-disable/2", "commit": commit, "all_hold": ok,
           "registry_blobs": here, "tree_clean": True,
           "environment": [d.get("environment") for d in docs], "records": records}
    with open(os.path.join(ROOT, argv[0]), "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print(f"paired-disable (joined {len(docs)} shards): {sum(r['holds'] for r in records)}/{len(records)} hold -> {argv[0]}")
    sys.exit(0 if ok else 1)


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
    # C9 round 2 (rows): check's sha256 rule vs names_every_digest (A69).
    "M217": {"siblings": ["M473"], "kind": "pair"},
    "M377": {"siblings": ["M378"], "kind": "pair"},
    "M378": {"siblings": ["M377"], "kind": "pair"},
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
    # C9 round 2: provenance skip-worktree tag vs the byte comparison.
    "M346": {"siblings": ["M451"], "kind": "pair"},
    # C9 round 2 (decision C): git untracked views vs the filesystem walk.
    "M347": {"siblings": ["M501"], "kind": "pair"},
    "M414": {"siblings": ["M501"], "kind": "pair"},
    "M60": {"siblings": ["M69"], "kind": "pair"},
    "M89": {"siblings": ["M86", "M96"], "kind": "set"},
    # C9 round 2 (harness): read_regular's regular-file check vs the
    # non-blocking open (M481) and the one-read rule (M335); service_leaf's
    # symlink check vs is_dir (M486) and the mode check (M327).
    "M482": {"siblings": ["M481", "M335"], "kind": "set"},
    "M487": {"siblings": ["M486", "M327"], "kind": "set"},
    # C9 round 3 (harness): mutual pairs on the decision-A/D path.
    # Interpreter-is-a-script refusal vs the verified descriptor being
    # close-on-exec (kernel ENOENT); staging-root 0700 vs the per-launch
    # dir's 0700. EQUIV_RECORD[...]["all_paths"].
    "M594": {"siblings": ["M595"], "kind": "pair"},
    "M595": {"siblings": ["M594"], "kind": "pair"},
    "M596": {"siblings": ["M597"], "kind": "pair"},
    "M597": {"siblings": ["M596"], "kind": "pair"},
    # C9 round 3 (rows): no route leaves these guards alone.
    # EQUIV_RECORD[...]["all_paths"]. M186 on the direct route (the
    # privileged route's own sibling M620 in the set too); M286/M459 vs
    # the hashed ancestry walk; M453 vs the common-dir rule; M602 vs the
    # protected custodian's spend rule, with the production helper and
    # custodian.
    "M186": {"siblings": ["M606", "M620"], "kind": "set"},
    "M286": {"siblings": ["M581"], "kind": "pair"},
    "M453": {"siblings": ["M580"], "kind": "pair"},
    "M459": {"siblings": ["M581"], "kind": "pair"},
    # C9 round 4b, final (amendment 66): a non-root helper also cannot verify
    # the pinned custodian program (it cannot open another uid's
    # /proc/<pid>/exe), so the pin verification (M1489) is in M602's set.
    "M602": {"siblings": ["M628", "M1489"], "kind": "set"},
    # C9 round 4c, GATE (amendment 74): EVL's `!issuer_ok` arm vs the same
    # predicate applied first by verify_check_evidence (M10) and bind_episode
    # (M1299, a library primitive whose production route this is).
    "M1726": {"siblings": ["M10", "M1299"], "kind": "set"},
    # C9 round 4 (rows, EQUIVALENCE): the two rule functions whose one
    # production caller is ProtectedHost::operator(), where each is
    # dominated; executed with the production axon-fabric.
    "M704": {"siblings": ["M548"], "kind": "pair"},
    "M546": {"siblings": ["M548"], "kind": "pair"},
    "M705": {"siblings": ["M633"], "kind": "pair"},
    "M634": {"siblings": ["M633"], "kind": "pair"},
    # C9 round 4 fix wave, ROWS2 wave 2, STRICT.
    "M796": {"siblings": ["M623", "M797"], "kind": "set"},
    "M801": {"siblings": ["M800"], "kind": "pair"},
    "M803": {"siblings": ["M802"], "kind": "pair"},
    "M804": {"siblings": ["M802"], "kind": "pair"},
    "M809": {"siblings": ["M810"], "kind": "pair"},
    "M813": {"siblings": ["M325"], "kind": "pair"},
    "M338": {"siblings": ["M812"], "kind": "pair"},
    # C9 round 4 fix wave, rows2 wave 2, LOOP (EQUIV_RECORD in the registry).
    "M830": {"siblings": ["M857"], "kind": "pair"},
    "M852": {"siblings": ["M122", "M104"], "kind": "set"},
    "M853": {"siblings": ["M854"], "kind": "pair"},
    "M854": {"siblings": ["M853"], "kind": "pair"},
    "M855": {"siblings": ["M856"], "kind": "pair"},
    "M856": {"siblings": ["M855"], "kind": "pair"},
    # C9 round 4b, ROWS4B (amendment 62; EQUIV_RECORD in the registry).
    "M1038": {"siblings": ["M1039"], "kind": "pair"},
    "M1043": {"siblings": ["M1035"], "kind": "pair"},
    "M1081": {"siblings": ["M1082"], "kind": "pair"},
    "M1082": {"siblings": ["M1081"], "kind": "pair"},
    "M1083": {"siblings": ["M1082"], "kind": "pair"},
    "M1072": {"siblings": ["M1085"], "kind": "pair"},
    "M1085": {"siblings": ["M1072"], "kind": "pair"},
    # ROWS4B wave 2 (amendment 62, strict reading).
    "M1086": {"siblings": ["M451"], "kind": "pair"},
    "M1087": {"siblings": ["M450"], "kind": "pair"},
    "M1089": {"siblings": ["M500"], "kind": "pair"},
    "M1091": {"siblings": ["M1081", "M1082"], "kind": "set"},
    "M1095": {"siblings": ["M1054"], "kind": "pair"},
    "M1096": {"siblings": ["M1054"], "kind": "pair"},
    "M1097": {"siblings": ["M1071"], "kind": "pair"},
    # C9 round 4b fix wave, rows4a (amendment 61; EQUIV_RECORD in the registry).
    "M948": {"siblings": ["M950"], "kind": "pair"},
    "M954": {"siblings": ["M955"], "kind": "pair"},
    "M955": {"siblings": ["M954"], "kind": "pair"},
    "M956": {"siblings": ["M957"], "kind": "pair"},
    "M957": {"siblings": ["M956"], "kind": "pair"},
    "M962": {"siblings": ["M15", "M963"], "kind": "set"},
    "M964": {"siblings": ["M965"], "kind": "pair"},
    "M970": {"siblings": ["M971"], "kind": "pair"},
    "M971": {"siblings": ["M970"], "kind": "pair"},
    "M972": {"siblings": ["M973"], "kind": "pair"},
    "M973": {"siblings": ["M972"], "kind": "pair"},
    "M974": {"siblings": ["M972", "M973"], "kind": "set"},
    "M979": {"siblings": ["M980", "M998"], "kind": "set"},
    "M980": {"siblings": ["M979", "M998"], "kind": "set"},
    "M998": {"siblings": ["M979", "M980"], "kind": "set"},
    "M981": {"siblings": ["M819"], "kind": "pair"},
}
# C9 round 4b, INTEGRATE-A (amendment 64; EQUIV_RECORD in the registry).
GUARD_SETS.update({
    'M1202': {"siblings": ['M1201'], "kind": "pair"},
    'M1203': {"siblings": ['M1202', 'M1201'], "kind": "set"},
    'M1207': {"siblings": ['M1200'], "kind": "pair"},
    'M1219': {"siblings": ['M1204', 'M1240'], "kind": "set"},
    'M1220': {"siblings": ['M1238'], "kind": "pair"},
    'M1225': {"siblings": ['M1223'], "kind": "pair"},
    'M1226': {"siblings": ['M1224'], "kind": "pair"},
    'M1237': {"siblings": ['M1212'], "kind": "pair"},
    'M1238': {"siblings": ['M1220'], "kind": "pair"},
    'M1239': {"siblings": ['M1222'], "kind": "pair"},
    'M1240': {"siblings": ['M1218'], "kind": "pair"},
    'M1242': {"siblings": ['M1221'], "kind": "pair"},
    'M1243': {"siblings": ['M1216'], "kind": "pair"},
    'M1245': {"siblings": ['M1210'], "kind": "pair"},
    'M1246': {"siblings": ['M1210'], "kind": "pair"},
    'M1247': {"siblings": ['M1208'], "kind": "pair"},
    'M1248': {"siblings": ['M1209'], "kind": "pair"},
    'M1249': {"siblings": ['M1218'], "kind": "pair"},
    'M1250': {"siblings": ['M1269'], "kind": "pair"},
    'M1251': {"siblings": ['M1220'], "kind": "pair"},
    'M1252': {"siblings": ['M1211'], "kind": "pair"},
    'M1254': {"siblings": ['M1221'], "kind": "pair"},
    'M1255': {"siblings": ['M1227'], "kind": "pair"},
    'M1256': {"siblings": ['M1236'], "kind": "pair"},
    'M1257': {"siblings": ['M1218'], "kind": "pair"},
    'M1258': {"siblings": ['M1241'], "kind": "pair"},
    'M1259': {"siblings": ['M1220'], "kind": "pair"},
    'M1260': {"siblings": ['M1211'], "kind": "pair"},
    'M1261': {"siblings": ['M1244'], "kind": "pair"},
    'M1262': {"siblings": ['M1253'], "kind": "pair"},
    'M1265': {"siblings": ['M1263'], "kind": "pair"},
    'M1266': {"siblings": ['M1330'], "kind": "pair"},
    'M1267': {"siblings": ['M1218', 'M1266', 'M1330'], "kind": "set"},
})

# C9 round 4b, INTEGRATE-B (amendment 64; EQUIV_RECORD in the registry).
GUARD_SETS.update({
    "M1294": {"siblings": ["M1295"], "kind": "pair"},
    "M1296": {"siblings": ["M34", "M1291"], "kind": "set"},
})
# ── C9 round 4b, INTEGRATE-C (amendment 64) ──
GUARD_SETS.update({
    'M1309': {"siblings": ['M1310', 'M1311'], "kind": 'set'},
    'M1310': {"siblings": ['M1312'], "kind": 'pair'},
    'M1314': {"siblings": ['M979'], "kind": 'pair'},
    'M1329': {"siblings": ['M1330'], "kind": 'pair'},
    'M1330': {"siblings": ['M1329'], "kind": 'pair'},
    'M1334': {"siblings": ['M1322'], "kind": 'pair'},
})
# ── end INTEGRATE-C ──
# C9 round 4b, INTEGRATE-D (amendment 64; EQUIV_RECORD in the registry).
GUARD_SETS.update({
    "M1346": {"siblings": ["M1345"], "kind": "pair"},
    "M1360": {"siblings": ["M1362", "M1363"], "kind": "set"},
    "M1361": {"siblings": ["M1362", "M1363"], "kind": "set"},
    "M1365": {"siblings": ["M1011"], "kind": "pair"},
    "M1366": {"siblings": ["M1011"], "kind": "pair"},
    "M1367": {"siblings": ["M1011"], "kind": "pair"},
    "M1368": {"siblings": ["M1369", "M1362", "M1011"], "kind": "set"},
    "M1370": {"siblings": ["M1006"], "kind": "pair"},
    "M1371": {"siblings": ["M1008"], "kind": "pair"},
})

# C9 round 4b, INTEGRATE-2 (amendment 64; EQUIV_RECORD in the registry).
GUARD_SETS.update({
    "M1364": {"siblings": ["M1012"], "kind": "pair"},
    "M1372": {"siblings": ["M1249", "M1257", "M1218"], "kind": "set"},
    "M1400": {"siblings": ["M1220", "M1238"], "kind": "set"},
    "M1402": {"siblings": ["M45"], "kind": "pair"},
    "M1387": {"siblings": ["M1388", "M1334"], "kind": "set"},
})

# C9 round 4b, INTEGRATE-E (amendment 64; EQUIV_RECORD in the registry).
GUARD_SETS.update({
    "M1375": {"siblings": ["M108"], "kind": "pair"},
    "M1377": {"siblings": ["M978"], "kind": "pair"},
    "M1378": {"siblings": ["M1379", "M978", "M965"], "kind": "set"},
    "M1379": {"siblings": ["M1378", "M978", "M965"], "kind": "set"},
    "M1380": {"siblings": ["M02"], "kind": "pair"},
})


def current_edits_digest(rid):
    """What executing record `rid` NOW would run (its edits and marker)."""
    if rid in GUARD_SETS:
        return edits_digest(rid, GUARD_SETS[rid]["siblings"])
    rep = (mut.STALE_REFACTORED.get(rid) or {}).get("replacement")
    return edits_digest(rep, []) if rep in BY_ID else None


def main():
    if "--check-stale" not in sys.argv[1:] and mut.uncommitted():
        sys.exit("refused: uncommitted changes in the tree — paired-disable is evidence about a commit\n"
                 + mut.uncommitted())
    commit = sh("git rev-parse HEAD").stdout.strip()
    flags = {"--reexecute-stale", "--check-stale"}
    argv = [a for a in sys.argv[1:]
            if not a.startswith(("--only=", "--shard=")) and a not in flags and a != "--join"]
    join = "--join" in sys.argv[1:]
    shard_arg = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--shard=")), None)
    reexecute_stale = "--reexecute-stale" in sys.argv[1:]
    check_stale = "--check-stale" in sys.argv[1:]
    only_arg = next((a.split("=", 1)[1] for a in sys.argv[1:] if a.startswith("--only=")), None)
    only = None if only_arg is None else set(only_arg.split(","))
    # Every retired row has a matrix and no active row has one.
    if set(GUARD_SETS) != set(mut.EQUIVALENT_DID):
        sys.exit(f"refused: GUARD_SETS {sorted(GUARD_SETS)} != EQUIVALENT_DID {sorted(mut.EQUIVALENT_DID)}")
    # Amendment 64: a SIBLING-ONLY edit exists only as a member of retired
    # rows' guard sets -- exactly the sets its record names.
    for sid, srec in sorted(mut.SIBLING_RECORD.items()):
        users = sorted(r for r, g in GUARD_SETS.items() if sid in g["siblings"])
        if users != sorted(srec["member_of"]):
            sys.exit(f"refused: SIBLING_ONLY {sid} is a member of {users}, its record says {srec['member_of']}")
    universe = sorted(set(GUARD_SETS) | set(mut.STALE_REFACTORED), key=lambda r: int(r[1:]))
    if join:
        join_shards(argv, commit, universe)
    sel = None
    if shard_arg is not None:
        # One of N disjoint slices of the records, run in its OWN clone and
        # target dir at the same commit; `--join` makes the status file only
        # when the slices cover every record exactly once at that commit.
        if only is not None or reexecute_stale or check_stale:
            sys.exit("refused: --shard runs a fresh slice; it takes no --only/--reexecute-stale/--check-stale")
        try:
            k, n = (int(x) for x in shard_arg.split("/"))
        except ValueError:
            sys.exit(f"refused: --shard={shard_arg} is not K/N")
        if not (n >= 1 and 0 <= k < n):
            sys.exit(f"refused: --shard={shard_arg}: K is 0-indexed and below N")
        if not argv:
            sys.exit("refused: a shard writes its own OUT.json, never the status file")
        sel = {r for i, r in enumerate(universe) if i % n == k}
    out_path = os.path.join(ROOT, argv[0] if argv else "governance/status/v022-psv-paired-disable.json")
    if check_stale:
        prev = json.load(open(out_path))
        n = 0
        for r in prev.get("records", []):
            r.setdefault("commit", prev.get("commit"))
            why = stale_reasons(r, GUARD_SETS)
            if why:
                n += 1
                print(f"STALE {r['mutation']} (executed at {str(r['commit'])[:8]}): " + "; ".join(why))
            else:
                print(f"current {r['mutation']} (executed at {str(r['commit'])[:8]})")
        print(f"{n} of {len(prev.get('records', []))} records stale under the currency rule")
        sys.exit(1 if n else 0)
    if only is not None:
        unknown = sorted(only - set(GUARD_SETS) - set(mut.STALE_REFACTORED))
        if unknown:
            sys.exit(f"--only: no retirement record for {unknown}")
        # A record kept from an earlier commit must still be current.
        try:
            prev_doc = json.load(open(out_path))
        except (OSError, ValueError):
            prev_doc = {"records": []}
        stale = {}
        for r in prev_doc.get("records", []):
            if r["mutation"] in only:
                continue
            r.setdefault("commit", prev_doc.get("commit"))
            why = stale_reasons(r, GUARD_SETS)
            if why:
                stale[r["mutation"]] = why
        if stale and not reexecute_stale:
            for k, v in sorted(stale.items()):
                print(f"STALE {k}: " + "; ".join(v), flush=True)
            sys.exit(f"refused: {len(stale)} kept record(s) are not current at this commit "
                     f"({sorted(stale)}); re-execute them (--reexecute-stale) or name them in --only")
        only |= set(stale)
    build_prereqs()
    INTERP["path"] = os.path.join(cargo_target_dir(), "debug", "axon")
    INTERP["sha256"] = mut.clean_interpreter(INTERP["path"])
    PREREQS[:] = prereq_artifacts()
    PREREQ_SHA.update({a: mut.sha(a) for _, _, arts in PREREQS for a in arts if os.path.exists(a)})
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
        if sel is not None and rid not in sel:
            continue
        rec = mut.EQUIV_RECORD[rid]
        row = BY_ID[rid]
        pkg, target, test = row[5], row[6], row[7]
        faults_before = len(INTERP_FAULTS)
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
                after_cell(edits, mut.spawns_scripts(pkg, target))
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
                after_cell(edits, any(mut.package_spawns_scripts(p)
                                      for p in [pkg, owner_crate, *consumers]))
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
        # The row's own package set too (C9 round 2): axon-core's native-parity
        # harnesses read `target/debug/axon` under the WORKSPACE, whatever
        # CARGO_TARGET_DIR says, so under a private target dir they fail on
        # the clean tree, and that read as SUITE_BROKEN for M58/M60/M89.
        own_pkgs = pkg if owner_crate == pkg else f"{pkg} -p {owner_crate}"
        own_key = (own_pkgs, row_flags(target))
        if own_key not in CONSUMER_BASELINE:
            ook, ofails = full_suite_ok(own_pkgs, row_flags(target))
            CONSUMER_BASELINE[own_key] = ("SUITE_OK" if ook else
                                          ("COMPILE_ERROR" if ook is None else "SUITE_BROKEN"), ofails)
        own_base, own_base_fails = CONSUMER_BASELINE[own_key]
        if own_base != "SUITE_OK":
            full_state, full_fails, cons_states = "BASELINE_BROKEN", [
                f"{own_pkgs}: {own_base} on the clean tree"] + list(own_base_fails)[:11], {}
        elif any(v != "SUITE_OK" for v in cons_base.values()):
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
        # Root-only tests that returned early are SKIPS, never passes: named
        # per package so a reader sees what the full-suite cell did not run.
        matrix["retired_guard_full_suite_skipped"] = {
            f"{k[0]}{(' ' + k[1]) if k[1] else ''}": v
            for k, v in CELL_SKIPS.items() if v and (k[0] in (own_pkgs, *consumers))}
        sib_only = phase(b)              # B removed, A present
        matrix["sibling_set_disabled"] = sib_only
        faults = INTERP_FAULTS[faults_before:]
        if faults:
            matrix["interpreter_not_restored"] = faults
        good = matrix_holds(matrix)
        ok &= good
        records.append({
            "mutation": rid, "status": "EQUIVALENT_DID", "kind": gs["kind"],
            "property": rec["property"], "original_guard": {"file": row[2]},
            "subsumed_by": sibs, "live_killing_mutant": rec["killer"],
            "all_paths": rec.get("all_paths"),
            "matrix": matrix, "holds": good, "commit": commit,
            "edits_sha256": edits_digest(rid, sibs), "environment": environment(),
        })
        print(f"{'OK ' if good else 'BAD'} {rid} [{gs['kind']}]: base={baseline} "
              f"retired_off={retired_only} sib_off={sib_only} set_off={joint} full_suite={full_state}"
              + (f"  INTERPRETER NOT RESTORED: {faults}" if faults else ""),
              flush=True)
        if faults:
            break  # no later record is judged on another interpreter
    # STALE rows (C9 round 1). "The old text is absent" shows only that the TEXT
    # changed: M204 was recorded stale while its guard lived on, refactored,
    # with no row. A stale row holds ONLY if it names a replacement that is an
    # ACTIVE row (not retired, its text present exactly once) AND that
    # replacement is killed here by its OWN attack: baseline passes, the
    # replacement's mutation fails the test on its attack marker.
    for rid, rec in mut.STALE_REFACTORED.items():
        if only is not None and rid not in only:
            continue
        if sel is not None and rid not in sel:
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
                    after_cell([edit_of(rep)], mut.spawns_scripts(pkg, target))
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
        holds = stale_holds({"old_string_present": old_present, "replacement_state": rep_state}) \
            and not INTERP_FAULTS
        ok &= holds
        records.append({
            "mutation": rid, "status": "STALE_REFACTORED", "property": rec["property"],
            "original_guard": rec["how"], "old_string_present": old_present,
            "replacement": rep, "replacement_state": rep_state,
            "matrix": None, "holds": holds, "commit": commit,
            "edits_sha256": edits_digest(rep, []) if rep in BY_ID else None,
            "environment": environment(),
        })
        print(f"{'OK ' if holds else 'BAD'} {rid}: stale ({rec['how']}); replacement {rep}: {rep_state}",
              flush=True)
    out = argv[0] if argv else "governance/status/v022-psv-paired-disable.json"
    path = out_path
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
    missing = sorted((sel if sel is not None else set(universe)) - {r["mutation"] for r in records})
    if missing:
        print(f"BAD no record for {missing}", flush=True)
        ok = False
    doc = {"schema": "axon-v022-paired-disable/2", "commit": commit, "all_hold": ok,
           "registry_blobs": mut.registry_blobs(), "tree_clean": True, "environment": environment(),
           "records": records}
    if sel is not None:
        doc |= {"shard": shard_arg, "selected": sorted(sel, key=lambda r: int(r[1:]))}
    with open(path, "w") as f:
        json.dump(doc, f, indent=2)
        f.write("\n")
    print(f"paired-disable: {sum(r['holds'] for r in records)}/{len(records)} hold -> {out}")
    sys.exit(0 if ok else 1)


if __name__ == "__main__":
    main()
