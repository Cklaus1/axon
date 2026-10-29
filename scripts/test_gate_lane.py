#!/usr/bin/env python3
"""Self-tests of the gate's stage timing (GL-1) and of the lane machinery (tasks/gate-lane-spec.md).

No cargo build, no audit run: gate.sh's own text, its helper functions run in bash on fixtures, and
fixture audits under scripts/testdata/gate_lane/.

    python3 -B scripts/test_gate_lane.py [test_name ...]
"""
import json
import os
import re
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
GATE = os.path.join(ROOT, "scripts", "gate.sh")
TESTS = []


def test(fn):
    TESTS.append(fn)
    return fn


def gate_lines():
    return open(GATE).read().split("\n")


def strict_block(lines):
    """[start, end) of the `if [ "$STRICT" = 1 ]; then … fi` block holding the strict-only stages."""
    start = next(i for i, l in enumerate(lines) if l.startswith('if [ "$STRICT" = 1 ]; then')
                 and i + 1 < len(lines) and "── gate:" in lines[i + 1])
    end = next(i for i in range(start + 1, len(lines)) if lines[i] == "fi")
    return start, end + 1


def gate_functions(*names):
    """The bash source of top-level gate.sh functions, as written there (`name() {` … `}` at column 0)."""
    text = open(GATE).read()
    out = []
    for n in names:
        m = re.search(r"^%s\(\) \{\n.*?^\}\n" % re.escape(n), text, re.S | re.M)
        assert m, f"gate.sh defines no top-level function {n}()"
        out.append(m.group(0))
    return "\n".join(out)


# ── GL-1 ────────────────────────────────────────────────────────────────────

@test
def stage_ids_match_banners():
    lines = gate_lines()
    s0, s1 = strict_block(lines)
    open_id, ids, bad = None, [], []
    in_fn = None
    for i, l in enumerate(lines):
        m = re.match(r"^(stage_[a-z0-9_]+)\(\) \{", l)
        if m:
            in_fn = m.group(1)
        elif in_fn and l == "}":
            in_fn = None
        m = re.match(r"^\s*stage_begin (S[0-9]+[A-Z]?) ", l)
        if m:
            assert open_id is None, f"gate.sh:{i + 1}: stage_begin {m.group(1)} inside open stage {open_id}"
            open_id = m.group(1)
            ids.append(open_id)
        elif re.match(r"^\s*stage_end(\s|$)", l):
            assert open_id, f"gate.sh:{i + 1}: stage_end with no open stage"
            open_id = None
        if not re.match(r'^\s*echo "── gate:', l) or s0 <= i < s1:
            continue
        if open_id or (in_fn and in_fn.startswith("stage_")):
            continue
        if "join: v0.22 trust-root preflight" in l or "coverage notice" in l:
            continue  # the trust-root join (not a stage) and the coverage notice: exempt
        bad.append(f"gate.sh:{i + 1}: {l.strip()}")
    assert not bad, "banners outside any stage_begin/stage_end pair or par stage function:\n  " + "\n  ".join(bad)
    for want in ("S08", "S09", "S10", "S16", "S20", "S21"):
        assert want in ids, f"no stage_begin {want}"
    assert len(ids) == len(set(ids)), f"duplicate stage ids: {ids}"


def run_stage_fixture(body):
    d = tempfile.mkdtemp(prefix="gate-lane-test-")
    tsv = os.path.join(d, "stages.tsv")
    script = "\n".join([
        "set -uo pipefail", 'TIER=gate', 'PAR_DIR=""; PAR_NAMES=(); PAR_PIDS=()',
        f'STAGE_TSV="{tsv}"; : > "$STAGE_TSV"',
        gate_functions("fail", "kill_tree", "par_abort", "stage_begin", "stage_end"),
        body])
    r = subprocess.run(["bash", "-c", script], capture_output=True, text=True)
    rows = [l.split("\t") for l in open(tsv).read().splitlines()]
    return r, rows


@test
def fail_records_open_stage():
    r, rows = run_stage_fixture('stage_begin S99 "fixture"\nfail "boom"\necho NOT_REACHED')
    assert r.returncode == 1 and "NOT_REACHED" not in r.stdout, r.stdout + r.stderr
    assert len(rows) == 1 and rows[0][0] == "S99" and rows[0][3] == "1", rows
    assert int(rows[0][2]) >= int(rows[0][1]), rows
    assert "(stage 'S99 fixture':" in r.stdout and "exit 1)" in r.stdout, r.stdout


@test
def stage_end_records_exit_and_duration():
    r, rows = run_stage_fixture('stage_begin S98 "ok"\nsleep 0.2\nstage_end\n'
                                'stage_begin S97 "skip"\nstage_end 77\nfail "after"')
    assert r.returncode == 1, r.stdout + r.stderr
    assert [x[0] for x in rows] == ["S98", "S97"] and [x[3] for x in rows] == ["0", "77"], rows
    assert int(rows[0][2]) - int(rows[0][1]) >= 150, rows
    assert "(stage 'S98 ok': 0.2s, exit 0)" in r.stdout, r.stdout


# ── GL-2 ────────────────────────────────────────────────────────────────────

def gl():
    sys.path.insert(0, os.path.join(ROOT, "scripts"))
    import gate_lane
    return gate_lane


def registry():
    return json.load(open(os.path.join(ROOT, "governance", "gate-lanes.json")))


@test
def registry_covers_every_gate_stage():
    g = gl()
    assert g.check(ROOT) == [], g.check(ROOT)
    text = open(GATE).read()
    extra = text.replace('stage_begin S19 "lockfile"',
                         'stage_begin S99 "new"\n./scripts/new.sh || fail x\nstage_end\nstage_begin S19 "lockfile"')
    extra = extra.replace('par "completeness manifest" stage_completeness',
                          'par "completeness manifest" stage_completeness\npar "brand new check" stage_new')
    bad = g.check(ROOT, gate=g.Gate(extra))
    assert any("stage S99 has no row" in b for b in bad), bad
    assert any("'brand new check' has no row" in b for b in bad), bad
    man = json.load(open(os.path.join(ROOT, "governance", "release-verification.json")))
    man["crates"]["axon-new"] = {"class": "A", "required": ["cargo test --locked -p axon-new"]}
    assert any("manifest crate axon-new" in b for b in g.check(ROOT, manifest=man)), "new manifest crate"
    unbuilt = text.replace('./scripts/lockfile_gate.sh || fail "lockfile gate"',
                           'cargo build --locked -p axon-x || fail x\n./scripts/lockfile_gate.sh || fail "lockfile gate"')
    assert any("cargo build --locked -p axon-x" in b for b in g.check(ROOT, gate=g.Gate(unbuilt))), "unregistered build"


@test
def registry_rejects_orphan_row():
    g = gl()
    reg = registry()
    reg["stages"].append({"id": "S98", "class": "selectable", "split": "none"})
    reg["par"].append("vanished check")
    reg["stages"][[s["id"] for s in reg["stages"]].index("S20")]["crates"].append("axon-gone")
    bad = g.check(ROOT, reg=reg)
    for want in ("row S98 names a stage", "'vanished check' names a par stage", "row axon-gone names a crate"):
        assert any(want in b for b in bad), (want, bad)
    reg = registry()
    reg["stages"][1]["banner"] = "renamed"
    assert any("row S02 banner 'renamed'" in b for b in g.check(ROOT, reg=reg)), "banner drift"


@test
def registry_build_text_matches_gate():
    g = gl()
    reg = registry()
    reg["builds"][0]["cmd"] = reg["builds"][0]["cmd"].replace(" -q", "")
    reg["stages"][[s["id"] for s in reg["stages"]].index("S16")]["split_cmd"] += " --lib"
    bad = g.check(ROOT, reg=reg)
    assert any("build B1" in b and "differs from gate.sh" in b for b in bad), bad
    assert any("S16 split_cmd" in b for b in bad), bad


@test
def units_derived_from_metadata():
    g = gl()
    ids = [u["id"] for u in g.units(g.derive(ROOT))]
    for want in ("S16:axon-core:lib", "S16:axon-core:bin:axon", "S16:axon-core:bin:axon-run",
                 "S16:axon-core:test:cli_run", "S16:axon-core:doc", "S20:axon-fabric:test:cortex_via_fabric",
                 "S03:axon-fabric:test:cortex_via_fabric", "S09:lint", "S09:axon-cortex:lib", "S14", "S04H",
                 "S20:axon-rt-ai:build", "S20:axon-fabric:test:workspace"):
        assert want in ids, (want, [i for i in ids if i.startswith(want.split(":")[0])][:40])
    assert len(ids) == len(set(ids)), "duplicate unit ids"
    assert not any(i.startswith(("S01", "S15", "S19", "S21")) for i in ids), "always/profile stages are not units"
    # the full gate's order: S02 before S03 before S16 before S20
    first = {s: next(n for n, i in enumerate(ids) if i.startswith(s + ":") or i == s) for s in ("S02", "S03", "S16", "S20")}
    assert first["S02"] < first["S03"] < first["S16"] < first["S20"], first


# ── GL-3 ────────────────────────────────────────────────────────────────────

FIXTURES = os.path.join(ROOT, "scripts", "testdata", "gate_lane")
_META = []


def meta():
    if not _META:
        _META.append(gl().cargo_metadata(ROOT))
    return _META[0]


def events(name):
    return [json.loads(l) for l in open(os.path.join(FIXTURES, name)) if l.strip()]


def unit_of(uid):
    g = gl()
    return next(s for s in g.derive(ROOT, meta=meta()) if s["id"] == uid)


def assembled(evs, unit, rcs=None, prior=(), root=None, pkg_sets=None):
    """One audited unit `unit` (renamed U1 in the fixture streams) after the build steps `prior`."""
    g = gl()
    u = dict(unit, id="U1")
    steps = list(prior) + [u]
    obs = g.attribute(evs, [s["id"] for s in steps])
    m = meta()
    out, errors = g.assemble(steps, obs, rcs or {}, m, g.read_gate(ROOT), pkg_sets or {},
                             [p["name"] for p in m["packages"]], root=root)
    return out["U1"], errors


@test
def audit_sentinel_bounds_window():
    obs = gl().attribute(events("window.jsonl"), ["U1", "U2"])
    assert obs["U1"]["reads"] == {"crates/a/src/lib.rs"}, obs["U1"]
    assert obs["U2"]["reads"] == {"crates/b/src/lib.rs"} and obs["U2"]["dirs"] == {"crates/b/tests/"}, obs["U2"]


@test
def audit_between_units_goes_to_both():
    obs = gl().attribute(events("between.jsonl"), ["U1", "U2"])
    assert "lingering/child.txt" in obs["U1"]["reads"] and "lingering/child.txt" in obs["U2"]["reads"], obs


@test
def audit_overflow_marks_unit_unaudited():
    g = gl()
    obs = g.attribute(events("overflow.jsonl"), ["U1", "U2"])
    assert obs["U1"]["overflow"] and not obs["U2"]["overflow"], obs
    a, _ = assembled(events("overflow.jsonl")[:4], unit_of("S14"))
    assert a["status"] == "unaudited" and "overflow" in a["why"], a


@test
def audit_skip_marks_unit_unaudited():
    a, _ = assembled(events("window.jsonl")[:3], unit_of("S08"), rcs={"U1": 77})
    assert a["status"] == "unaudited" and "77" in a["why"], a
    a, _ = assembled(events("window.jsonl")[:3], unit_of("S08"), rcs={"U1": 0})
    assert a["status"] == "ok", a
    # and the lane always runs an unaudited unit
    g = gl()
    us = [dict(unit_of("S08"))]
    sel, _ = g.decide({"units": {"S08": dict(a, status="unaudited", why="SKIP (exit 77)")}, "packages": {},
                       "universe": []}, us, [])
    assert sel["S08"] and "unaudited" in sel["S08"], sel


def fake_target(entries):
    """A target dir holding dep-info files {relative path: [repo paths it lists]}, written as cargo writes
    them: workspace-relative in deps/*.d, absolute in a bin's top-level .d."""
    t = tempfile.mkdtemp(prefix="gate-lane-target-")
    for rel, paths in entries.items():
        p = os.path.join(t, "debug", rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            names = paths if rel.startswith("deps/") else [os.path.join(ROOT, x) for x in paths]
            f.write(f"{p[:-2]}: " + " ".join(names) + "\n")
    return t


@test
def audit_exec_adds_package_closure():
    g = gl()
    t = fake_target({"cortex.d": ["crates/axon-cortex/src/bin/cortex.rs"]})
    sets = g.package_compile_sets(meta(), ROOT, t)
    # The spec's case: fabric's cortex_via_fabric executes workspace-bins/debug/cortex.
    a, err = assembled(events("exec_private.jsonl"), unit_of("S03:axon-fabric:test:cortex_via_fabric"), pkg_sets=sets)
    assert not err and "axon-cortex" in a["closure"], a
    files, _ = g.unit_paths({"packages": sets}, a)
    assert "crates/axon-cortex/src/bin/cortex.rs" in files, sorted(files)[:20]
    # Control: a unit whose own closure lacks axon-cortex gets it from the exec alone.
    ledger = next(u for u in g.derive(ROOT, meta=meta()) if u["id"].startswith("S20:axon-ledger:test:"))
    a0, _ = assembled([{"t": "begin", "step": "U1"}, {"t": "end", "step": "U1"}], ledger, pkg_sets=sets)
    assert "axon-cortex" not in a0["closure"], a0["closure"]
    a1, _ = assembled(events("exec_private.jsonl"), ledger, pkg_sets=sets)
    assert "axon-cortex" in a1["closure"] and a1["execs"] == ["private:cortex"], a1
    # The static workspace_bin scan reaches the same edge without the exec.
    a2, _ = assembled([{"t": "begin", "step": "U1"}, {"t": "end", "step": "U1"}],
                      unit_of("S03:axon-fabric:test:cortex_via_fabric"), root=ROOT)
    assert "axon-cortex" in a2["static_unobserved"], a2["static_unobserved"]


@test
def audit_lint_unit_has_closure_compile_set():
    g = gl()
    t = fake_target({"deps/axon_ledger-0123abcd.d": ["crates/axon-ledger/src/lib.rs"]})
    sets = g.package_compile_sets(meta(), ROOT, t)
    assert "crates/axon-ledger/src/lib.rs" in sets["axon-ledger"]["files"], sets["axon-ledger"]
    a, _ = assembled([], unit_of("S14"), pkg_sets=sets)
    files, _ = g.unit_paths({"packages": sets}, a)
    assert "crates/axon-ledger/src/lib.rs" in files, a["closure"]


@test
def audit_relative_dep_info_feeds_compile_sets():
    # cargo writes deps/*.d entries workspace-relative (`crates/…`, `Cargo.toml`); the first real audit
    # dropped them all, left every package set at its Cargo.toml and skipped units a lib edit changes.
    g = gl()
    t = fake_target({"deps/axon_ledger-0123abcd.d": ["crates/axon-ledger/src/lib.rs",
                                                      "crates/axon-ledger/src/query.rs", "Cargo.toml"]})
    sets = g.package_compile_sets(meta(), ROOT, t)
    led = sets["axon-ledger"]
    assert "crates/axon-ledger/src/query.rs" in led["files"] and "Cargo.toml" in led["files"], led
    # A package whose dep-info names none of its own files falls back to its directory as a prefix.
    t2 = fake_target({"deps/axon_ledger-0123abcd.d": ["crates/axon-core/src/lib.rs"]})
    led2 = g.package_compile_sets(meta(), ROOT, t2)["axon-ledger"]
    assert "crates/axon-ledger/" in led2["prefixes"], led2
    # Transitive closure: an axon-signal test is selected when its dependency axon-ledger changes.
    sig = next(u for u in g.derive(ROOT, meta=meta()) if u["id"].startswith("S20:axon-signal:test:"))
    a, _ = assembled([{"t": "begin", "step": "U1"}, {"t": "end", "step": "U1"}], sig, pkg_sets=sets)
    assert "axon-ledger" in a["closure"], a["closure"]
    audit = {"units": {sig["id"]: a}, "packages": sets, "universe": ["crates/axon-ledger/src/query.rs"]}
    sel, unread = g.decide(audit, [sig], [("M", "crates/axon-ledger/src/query.rs")])
    assert sel[sig["id"]] and not unread, (sel, unread)


@test
def audit_git_open_flags_history():
    obs = gl().attribute(events("git.jsonl"), ["U1", "U2"])
    assert obs["U1"]["git"] and not obs["U2"]["git"], obs
    a, _ = assembled(events("git.jsonl")[:3], unit_of("S16:axon-core:test:pci_delta_note"))
    assert a["git_history"], a


@test
def audit_undeclared_producer_fails():
    g = gl()
    ledger = next(u for u in g.derive(ROOT, meta=meta()) if u["id"].startswith("S20:axon-ledger:test:"))
    _, err = assembled(events("exec_main.jsonl"), ledger)
    assert any("executes cortex that no preceding build step" in e for e in err), err
    b2 = next(s for s in g.derive(ROOT, meta=meta()) if s["id"] == "B2")
    a, err = assembled(events("exec_main.jsonl"), ledger, prior=[b2])
    assert not err and a["needs"] == ["B2"], (a, err)


@test
def audit_workspace_compile_unit_produces_its_own_bins():
    # S05/S06/S07 run scripts whose `cargo test -p <pkg>` builds that package's bins (Q9 = A: the whole
    # workspace), so a bin they execute needs no prior build step (the second real audit's 14 false errors).
    a, err = assembled(events("exec_main.jsonl"), unit_of("S06"))
    assert not err and a["needs"] == [], (a, err)


@test
def audit_never_writes_a_gate_receipt():
    g = gl()
    cmd = g.unit_command({"id": "S14"})
    assert cmd == ["./scripts/gate.sh", "--lane-units=S14"], cmd
    code = [l for l in open(os.path.join(ROOT, "scripts", "gate_lane.py")) if not l.lstrip().startswith("#")]
    assert not any("run_managed" in l and ("subprocess" in l or "[" in l) for l in code), "the audit runs run_managed"
    # gate.sh: a --lane-units run exits before the PASS line, and run_managed records any lane run as no gate.
    text = open(GATE).read()
    assert text.index('if [ -n "$LANE_SEL" ]; then\n  echo ""') < text.index('echo "✅ gate PASSED"')
    assert "--lane|--lane=*|--lane-units=*) lane=yes" in open(os.path.join(ROOT, "scripts", "run_managed.sh")).read()


# ── GL-4 ────────────────────────────────────────────────────────────────────

def fixture_audit():
    """The GL-4 fixture audit: every derived unit, closure from cargo, runtime facts from audit_overlay.json."""
    g = gl()
    m = meta()
    steps = g.derive(ROOT, meta=m)
    ov = json.load(open(os.path.join(FIXTURES, "audit_overlay.json")))
    obs = {s["id"]: {"reads": set(), "dirs": set(), "execs": set(), "git": False, "overflow": False} for s in steps}
    for uid, o in ov["units"].items():
        obs[uid]["reads"] |= set(o.get("reads", []))
        obs[uid]["dirs"] |= set(o.get("dirs", []))
        obs[uid]["git"] = o.get("git_history", False)
    sets = g.package_compile_sets(m, ROOT, os.path.join(tempfile.gettempdir(), "gate-lane-no-target"))
    us, err = g.assemble(steps, obs, {}, m, g.read_gate(ROOT), sets, [p["name"] for p in m["packages"]], root=ROOT)
    assert not err, err
    universe = [p for p in subprocess.run(["git", "ls-files"], cwd=ROOT, capture_output=True, text=True,
                                          check=True).stdout.splitlines() if p not in ov["absent_from_universe"]]
    return {"commit": "f" * 40, "units": us, "packages": sets, "universe": universe}, g.units(steps)


_FA = []


def lane(chg, audit=True):
    if not _FA:
        _FA.append(fixture_audit())
    a, us = _FA[0]
    sel, _ = gl().decide(a if audit else None, us, chg, read_text=lambda f: "")
    return sel, us


def everything_selected(sel, us):
    return all(sel[u["id"]] for u in us)


@test
def lane_cortex_bin_change_selects_fabric_cortex_via_fabric():
    sel, us = lane([("M", "crates/axon-cortex/src/bin/cortex.rs")])
    for uid in ("S03:axon-fabric:test:cortex_via_fabric", "S20:axon-fabric:test:cortex_via_fabric"):
        assert sel[uid], (uid, sel[uid])
    assert not everything_selected(sel, us)


@test
def lane_cortex_lib_change_selects_reverse_deps():
    sel, us = lane([("M", "crates/axon-cortex/src/runner.rs")])
    for p in ("axon-fabric", "axon-loop", "axon-loop-contracts", "axon-reflex", "cortex-policy-adapter"):
        mine = [u for u in us if u.get("pkg") == p]
        assert mine and all(sel[u["id"]] for u in mine), (p, [u["id"] for u in mine if not sel[u["id"]]])
    assert not sel["S20:axon-ledger:lib"], "a crate outside axon-cortex's reverse deps was selected"


@test
def lane_new_crate_test_selects_harness_binaries():
    sel, _ = lane([("A", "crates/axon-cortex/tests/fence_sweep.rs")])
    assert sel["S16:axon-core:test:harness_binaries"] and "enumerated" in sel["S16:axon-core:test:harness_binaries"], \
        sel["S16:axon-core:test:harness_binaries"]
    assert sel["S02:axon-cortex:lib"] and "added in package" in sel["S02:axon-cortex:lib"], sel["S02:axon-cortex:lib"]


@test
def lane_cortex_src_selects_claims_gate():
    sel, _ = lane([("M", "crates/axon-cortex/src/runner.rs")])
    assert sel["S16:axon-core:test:cli_run"] == "reads crates/axon-cortex/src/runner.rs", sel["S16:axon-core:test:cli_run"]


@test
def lane_core_src_selects_pci_delta_note():
    sel, _ = lane([("M", "crates/axon-core/src/lib.rs")])
    assert sel["S16:axon-core:test:pci_delta_note"], sel["S16:axon-core:test:pci_delta_note"]
    sel, _ = lane([])
    assert "git-history" in sel["S16:axon-core:test:pci_delta_note"], sel["S16:axon-core:test:pci_delta_note"]


@test
def lane_lint_only_change_selects_runtime_clippy():
    sel, _ = lane([("M", "crates/axon-ledger/src/lib.rs")])
    assert sel["S14"] == "compiles crates/axon-ledger/src/lib.rs", sel["S14"]
    assert not sel["S03:axon-fabric:test:cortex_via_fabric"]


@test
def lane_gate_definition_selects_everything():
    for p in ("scripts/lib/axon_bin.sh", "scripts/gate.sh", "scripts/gate_lane.py", "governance/gate-lanes.json"):
        sel, us = lane([("M", p)])
        assert everything_selected(sel, us), p


@test
def lane_deleted_or_mode_selects_everything():
    for st in ("D", "T"):
        sel, us = lane([(st, "crates/axon-ledger/src/lib.rs")])
        assert everything_selected(sel, us), st


@test
def lane_unknown_path_selects_everything():
    sel, us = lane([("A", "newdir/x.txt")])
    assert everything_selected(sel, us) and sel["S14"] == "unclassified path newdir/x.txt", sel["S14"]


@test
def lane_without_audit_selects_everything():
    sel, us = lane([("M", "crates/axon-ledger/src/lib.rs")], audit=False)
    assert everything_selected(sel, us) and "no read-set audit" in sel["S14"]


@test
def lane_every_stage_reachable():
    g = gl()
    lane([])
    a, us = _FA[0]
    assert g.audit_problems(a, us) == [], g.audit_problems(a, us)
    empty = json.loads(json.dumps(a))
    empty["units"]["S14"] = dict(empty["units"]["S14"], closure=[], reads=[])
    assert g.audit_problems(empty, us) == [f"unit S14 has an empty read set in audit {'f' * 12}"]


@test
def full_gate_check_never_reads_the_audit_store():
    # The full gate's result must not depend on the untracked .git/axon-gate-lanes/ store; the lane's must.
    g = gl()
    calls, real = [], g.find_audit
    g.find_audit = lambda root: calls.append(root)
    try:
        assert g.main(["check", "--quiet"]) == 0 and calls == [], calls
        g.main(["check", "--audit", "--quiet"])
        assert calls, "check --audit never loaded the stored audit"
    finally:
        g.find_audit = real
    text = open(GATE).read()
    assert "gate_lane.py check --audit --quiet" in text
    full = [l for l in text.split("\n") if "gate_lane.py check" in l and "test_gate_lane.py" in l]
    assert full and all("--audit" not in l for l in full), full


@test
def stage_log_is_per_run_and_fail_prints_it():
    text = open(GATE).read()
    assert 'STAGE_TSV="${CARGO_TARGET_DIR:-$ROOT/target}/gate-stages.$$.tsv"' in text
    body = text[text.index("fail() {"):]
    body = body[:body.index("\n}\n")]
    i, j, k = body.index("stage_end 1"), body.index("stage_table"), body.index("par_abort")
    assert i < j < k and body.index("fi\n") < j, body

@test
def lane_guard_form():
    text = open(GATE).read()
    code = "\n".join(l for l in text.split("\n") if not l.lstrip().startswith("#"))
    assert not re.search(r"lane_runs [^;\n]*&&", code), "a `lane_runs … &&` guard runs `|| fail` on a skip"
    lines = text.split("\n")
    for i, l in enumerate(lines):
        m = re.match(r'^\s*stage_begin (\S+) ', l)
        if m and m.group(1) != "S21":
            assert lines[i - 1].strip() == f"if lane_runs {m.group(1)}; then", f"gate.sh:{i + 1} unguarded {m.group(1)}"


@test
def no_unguarded_read_of_a_guarded_assignment():
    # A variable first set inside `if lane_runs X; then … fi` and read outside every guard is
    # unset when a lane skips X: under `set -u` that kills the lane (TR_BINS, the first audit).
    lines = open(GATE).read().split("\n")
    depth, inside, outside, reads = 0, set(), set(), []
    for i, l in enumerate(lines):
        s = l.split("#", 1)[0] if not l.lstrip().startswith("#") else ""
        if re.match(r"^if lane_runs \S+; then$", s):
            depth += 1
            continue
        if depth and s == "fi":
            depth -= 1
            continue
        for v in re.findall(r"^\s*(?:local\s+)?([A-Za-z_][A-Za-z0-9_]*)=", s):
            (inside if depth else outside).add(v)
        if not depth:
            reads += [(i + 1, v) for v in re.findall(r"\$\{?([A-Za-z_][A-Za-z0-9_]*)", s)]
    bad = sorted({f"gate.sh:{n} reads {v}" for n, v in reads if v in inside and v not in outside})
    assert not bad, bad


def gate(*args):
    return subprocess.run(["bash", GATE, *args], cwd=ROOT, capture_output=True, text=True)


@test
def lane_refuses_strict_fast_ref():
    for args, want in ((["--lane", "--strict"], "--lane is NOT the gate"),
                       (["--lane", "--nextest"], "--lane is NOT the gate"),
                       (["--lane", "--profile=v022-stage5"], "requires --strict"),
                       (["--lane=main"], "--lane takes no ref; the base is the newest audit (see --plan)"),
                       (["--plan"], "requires --lane")):
        r = gate(*args)
        assert r.returncode == 2 and want in r.stderr, (args, r.returncode, r.stderr)


_PLAN = []


def plan():
    if not _PLAN:
        _PLAN.append(gate("--lane", "--plan"))
    return _PLAN[0]


@test
def lane_plan_banner_never_claims_gate():
    r = plan()
    assert r.returncode == 0, r.stdout + r.stderr
    out = r.stdout.split("\n")
    assert out[0].startswith("════ LANE — NOT THE GATE, NOT EVIDENCE · audit="), out[:2]
    assert "gate PASSED" not in r.stdout and "nothing was run" in r.stdout, r.stdout[-300:]
    assert any(l.startswith("NOT RUN (strict-only):") for l in out)


@test
def run_managed_records_lane_tier():
    d = tempfile.mkdtemp(prefix="gate-lane-receipt-")
    with open(os.path.join(d, "cmd"), "w") as f:
        f.write("./scripts/gate.sh --lane --plan\n")
    with open(os.path.join(d, "log"), "w") as f:
        f.write(plan().stdout)
    src = open(os.path.join(ROOT, "scripts", "run_managed.sh")).read()
    fn = re.search(r"^write_receipt\(\) \{\n.*?^\}\n", src, re.S | re.M).group(0)
    r = subprocess.run(["bash", "-c", f'ROOT="$1"\n{fn}\nwrite_receipt "$2" 0', "_", ROOT, d],
                       capture_output=True, text=True)
    receipt = open(os.path.join(d, "receipt")).read()
    assert r.returncode == 0 and "gate_tier=lane\n" in receipt and "gate_run=no\n" in receipt \
        and "gate_strict=no\n" in receipt, r.stderr + receipt



# ── GL-5 ────────────────────────────────────────────────────────────────────

@test
def fast_is_refused_with_pointer_to_lane():
    for args in (["--fast"], ["--fast", "--base=main"], ["--base=main"]):
        r = gate(*args)
        assert r.returncode == 2 and "use --lane" in r.stderr, (args, r.returncode, r.stderr)


@test
def fast_only_checks_run_in_full_gate():
    text = open(GATE).read()
    code = "\n".join(l for l in text.split("\n") if not l.lstrip().startswith("#"))
    assert 'if [ "$FAST" = 1 ]' not in code and "fast_banner" not in code and "FAST_BASE" not in code
    for name, fn in (("G01 mutation anchors", "stage_g01_anchors"), ("harness skip rules/shapes", "stage_harness_skip")):
        assert f'\npar "{name}" {fn}\n' in code, name
    assert "python3 -B scripts/g01_anchors.py" in gate_functions("stage_g01_anchors")
    assert "bash scripts/test_harness_skip.sh" in gate_functions("stage_harness_skip")
    s22 = code[code.index('stage_begin S22 '):]
    s22 = s22[:s22.index("stage_end")]
    assert 'AXON_BIN="$(built_bin axon "" release)" ./scripts/vm_parity.sh' in s22, s22
    assert "lane_cargo B7 cargo build --locked -q --release -p axon-core --no-default-features --bin axon" in s22
    rm = open(os.path.join(ROOT, "scripts", "run_managed.sh")).read()
    assert "tier=fast" not in rm and "--fast) fast=yes" not in rm


def main(argv):
    want = set(argv)
    failed = 0
    ran = 0
    for fn in TESTS:
        if want and fn.__name__ not in want:
            continue
        ran += 1
        try:
            fn()
            print(f"ok   {fn.__name__}")
        except Exception as e:  # noqa: BLE001 — every failure is reported, then counted
            failed += 1
            print(f"FAIL {fn.__name__}: {type(e).__name__}: {e}")
    print(f"test_gate_lane: {ran - failed} passed, {failed} failed")
    return 1 if failed or not ran else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
