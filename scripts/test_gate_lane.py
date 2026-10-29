#!/usr/bin/env python3
"""Self-tests of the gate's stage timing (GL-1) and of the lane machinery (tasks/gate-lane-spec.md).

No cargo build, no audit run: gate.sh's own text, its helper functions run in bash on fixtures, and
fixture audits under scripts/testdata/gate_lane/.

    python3 -B scripts/test_gate_lane.py [test_name ...]
"""
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
