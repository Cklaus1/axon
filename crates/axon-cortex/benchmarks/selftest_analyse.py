#!/usr/bin/env python3
"""Adversarial fixtures for the analyser's candidate<->proposal join.

The analyser produces every headline number this experiment reports, and it is
not covered by anything the Rust suite runs. The defect these fixtures exist
for was real and load-bearing: proposal ownership was inferred from ORDER,
because `attempted.push()` happens before the episode runs and a candidate can
therefore be attempted while producing no proposal at all.

Measured with the guard removed, on CASE_SILENT_MIDDLE + CASE_SILENT_FIRST:

    ranking waste is 100% of inference spend ($0.50 finding vs $0.00 fixing)

when 2 of the 4 proposals were spent ON the true target, and a first-shot
repair was counted as a recovery. A confident number with the sign flipped is
worse than no number, which is why the join now fails closed.

Run: python3 selftest_analyse.py   (exit 0 pass, 1 fail, 2 could not run)
"""
import json, os, re, subprocess, sys, tempfile

HERE = os.path.dirname(os.path.abspath(__file__))
ANALYSER = os.path.join(HERE, "analyse_real_model.py")


# Keys are checked against the harness that actually writes them, below. A
# fixture whose shape has drifted from production tests the analyser against
# data it will never see, and fails for its own reasons rather than the
# behaviour under test.
PER_PROPOSAL_KEYS = {"n", "priors", "input", "output", "cache_create",
                     "cache_read", "cost_usd", "api_ms"}


def call(priors, cost=0.1):
    return {"n": priors + 1, "priors": priors, "input": 1, "output": 1,
            "cache_create": 0, "cache_read": 0, "cost_usd": cost, "api_ms": 1}


def assert_fixture_shape():
    """The fixture must carry the keys the real harness emits.

    Read out of `real_model.py` rather than restated here: a copy would agree
    with production only until the next field is added.
    """
    src = open(os.path.join(HERE, "real_model.py")).read()
    try:
        block = src.split('"per_proposal": [', 1)[1].split("]", 1)[0]
    except IndexError:
        print("could not locate per_proposal in real_model.py", file=sys.stderr)
        sys.exit(2)
    real = set(re.findall(r'"(\w+)":\s*c\[', block))
    if not real:
        print("per_proposal shape unreadable", file=sys.stderr)
        sys.exit(2)
    missing = real - set(call(0))
    if missing:
        print(f"per_proposal fixture is missing: {sorted(missing)}", file=sys.stderr)
        sys.exit(2)
    row = src.split("    row = {", 1)[1].split("\n    }", 1)[0]
    # Drop the nested per_proposal list: its keys belong to `call`, not to the
    # trial row, and counting them here makes the check fail for the wrong
    # reason — a check that fails for its own reasons gets disabled.
    row = re.sub(r'"per_proposal": \[.*?\n        \],', "", row, flags=re.S)
    rowkeys = set(re.findall(r'"(\w+)":', row))
    missing = rowkeys - set(trial([], "x", 1, []))
    if missing:
        print(f"trial fixture is missing: {sorted(missing)}", file=sys.stderr)
        sys.exit(2)


def trial(attempted, symbol, rank, props):
    return {"file": "f.ax", "symbol": symbol, "mutator": "operator", "how": "+ -> -",
            "localization_rank": rank, "ranked": attempted,
            "attempted": attempted, "target_attempted": symbol in attempted,
            "outcome": "verified_done", "exit": 0, "file_clean": True,
            "verdict_read": True,
            "workspace_restored": True, "episode": None, "reverted": 0,
            "extraction_status": [None] * len(props), "proposals": len(props),
            "bodies": [""] * len(props), "prior_rejections_seen": [c["priors"] for c in props],
            "models": ["m"], "per_proposal": props,
            "cost_usd": round(0.1 * len(props), 4), "api_ms": len(props),
            "wall_s": 1, "output_tokens": len(props)}


# A candidate in the MIDDLE produced nothing: a positional join hands the true
# target's proposal to the silent candidate before it.
CASE_SILENT_MIDDLE = trial(["wrong_A", "wrong_B", "true_C"], "true_C", 3,
                           [call(0), call(1), call(2), call(0)])
# The simplest off-by-one: the FIRST candidate produced nothing.
CASE_SILENT_FIRST = trial(["wrong_A", "true_B"], "true_B", 2, [call(0)])
# Nothing silent — attribution is sound here and must NOT be suppressed, or the
# guard would be indistinguishable from an analyser that reports nothing.
CASE_CLEAN = trial(["wrong_A", "true_B"], "true_B", 2, [call(0), call(0), call(1)])


def run(trials):
    with tempfile.NamedTemporaryFile("w", suffix=".json", delete=False) as fh:
        json.dump({"meta": {"model": "m", "cortex_commit": "a" * 40,
                            "driver_sha256_16": "d", "candidates": "3"},
                   "trials": trials}, fh)
        path = fh.name
    try:
        p = subprocess.run([sys.executable, ANALYSER, path],
                           capture_output=True, text=True, timeout=120)
    finally:
        os.unlink(path)
    if p.returncode != 0:
        print(f"FAIL: analyser exited {p.returncode}\n{p.stderr}", file=sys.stderr)
        sys.exit(1)
    return p.stdout


assert_fixture_shape()

fails = []
for name, t in (("silent-middle", CASE_SILENT_MIDDLE), ("silent-first", CASE_SILENT_FIRST)):
    out = run([t])
    if "UNATTRIBUTABLE" not in out:
        fails.append(f"{name}: a silent candidate was not detected — proposal "
                     f"ownership was inferred from order and reported as fact")
    # By CLASS, not one phrase at a time. Every number below is derived from
    # candidate<->proposal ownership, so if the join failed, none of them may
    # appear at all — a zero or an n/a in these slots reads as a measurement.
    for leak in ("ranking waste is", "on the true target", "mean $ ON target",
                 "mean $ BEFORE target", "mean proposals ON target",
                 "first-shot repair       :", "recovered after feedback:",
                 "recovery | first proposal rejected"):
        if leak in out:
            fails.append(f"{name}: `{leak}` was reported from an unsound join")

# The control. A guard that suppressed everything would pass the two cases
# above while measuring nothing at all.
out = run([CASE_CLEAN])
if "UNATTRIBUTABLE" in out:
    fails.append("clean: attribution was suppressed on a trial where every "
                 "candidate produced proposals — the guard is over-firing")
if "ranking waste is" not in out:
    fails.append("clean: no finding-vs-fixing split was reported on a soundly "
                 "attributable trial")

if fails:
    for f in fails:
        print("FAIL " + f, file=sys.stderr)
    sys.exit(1)
print("analyse_real_model self-test: 3 cases pass (2 adversarial + 1 control)")
