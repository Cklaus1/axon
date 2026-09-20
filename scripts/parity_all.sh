#!/usr/bin/env bash
# parity_all.sh — run every scripts/*_parity.sh harness and aggregate the result.
#
# WHY THIS EXISTS: the two-engine invariant (I-2 — the interpreter is the
# reference oracle; native codegen and AOT-wasm must agree byte-for-byte) is
# enforced by ~22 hand-written `*_parity.sh` harnesses. Historically those were
# run AD HOC, not by the gate — which is exactly how the "silent divergence
# found only by a periodic audit" bugs (#27/#36/#38/#39/parse_*_or, …) reached
# main. This runner makes the whole suite one command so `gate.sh --strict` can
# enforce it on every change.
#
# CONTRACT each harness already honors (verified across all 22):
#   - exit 0 + a "PASS"/"match" line                → PASS
#   - exit 0 + a "skip"/"unavailable" line          → SKIP (toolchain absent;
#                                                       e.g. no LLVM, no wasmtime)
#   - exit nonzero                                  → FAIL (a real divergence)
# Both PASS and SKIP exit 0, so SKIP is detected from the harness's own output
# marker. A harness that diverges exits nonzero and turns the whole run red.
#
# Determinism: AXON_SEED + AXON_AI_MOCK pinned (same as gate.sh) so seeded-RNG
# and AI-mock harnesses are reproducible.
#
# Usage:
#   scripts/parity_all.sh           # run all; exit 1 if any harness FAILS
#   scripts/parity_all.sh --quiet   # only print the per-harness status + summary
#   PARITY_SKIP_WASM=1 scripts/parity_all.sh   # skip the 7 wasm_* harnesses
#
# Exit 0 iff no harness FAILED (skips are fine).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"

# ── THE skip rule ────────────────────────────────────────────────────────────
# Kept byte-for-byte in step with `harness_skipped` in
# crates/axon-core/tests/cli_run.rs — the two used to DISAGREE: this file judged
# by `tail -1` alone while cli_run.rs scanned the last three non-empty lines, so
# a harness that printed its skip prose and then elaborated (handler_resume,
# exit_code_parity's catch-all — both ended on build-error text) was SKIP to one
# reader and PASS to the other. Same run, two verdicts.
#
# Preferred shape is the EXPLICIT final marker `<name>: SKIP — <reason>` that
# `harness_skip` emits; the prose forms are grandfathered for harnesses that
# have not been converted. A non-zero exit is a FAILURE and never a skip — that
# is decided on `$rc` before this is consulted.
# A skip line is HARNESS-LEVEL: it starts at column 0. An indented
# "  SKIP <case>" is one case of many and says nothing about the verdict —
# that distinction is why a whole-output grep was wrong in the first place.
_skip_re=': SKIP|skipping|this is a SKIP'
_verdict_re='PASS|FAILED|FAIL|: OK|OK —'
harness_says_skip() { # <output>
  local body final
  body="$(printf '%s\n' "$1" | grep -v '^[[:space:]]*$')"
  final="$(printf '%s\n' "$body" | tail -1)"
  # 1. The final line decides when it states anything at all.
  printf '%s\n' "$final" | grep -qE "^[^[:space:]].*($_skip_re)" && return 0
  printf '%s\n' "$final" | grep -qE "^[^[:space:]].*($_verdict_re)" && return 1
  # 2. Otherwise a skip may have been followed by elaboration (up to 2 lines).
  printf '%s\n' "$body" | tail -3 | grep -qE "^[^[:space:]].*($_skip_re)"
}

QUIET=0
for arg in "$@"; do
  case "$arg" in
    --quiet) QUIET=1 ;;
    *) echo "parity_all: unknown flag $arg" >&2; exit 2 ;;
  esac
done

export AXON_SEED="${AXON_SEED:-42}"
export AXON_AI_MOCK="${AXON_AI_MOCK:-1}"

pass=0; skip=0; fail=0
failed_names=""

for h in scripts/*_parity.sh; do
  name="$(basename "$h" .sh)"

  # Optional escape hatch: the 7 wasm_* harnesses need wasm32 targets + wasmtime;
  # let a caller opt out without editing this script.
  if [ "${PARITY_SKIP_WASM:-0}" = 1 ] && [[ "$name" == wasm* ]]; then
    printf "  SKIP  %-30s (PARITY_SKIP_WASM=1)\n" "$name"
    skip=$((skip+1))
    continue
  fi

  out="$(bash "$h" 2>&1)"
  rc=$?

  # A harness's VERDICT is its FINAL summary line (`<name>: PASS …` /
  # `<name>: … skipping`). Detecting "skip" anywhere in the output is wrong: a
  # passing harness may print a per-case "SKIP <case>" line (e.g. wasm_aot_run
  # skips a single loop program) while its overall result is PASS — grepping the
  # whole output then false-labels the harness as skipped and DROPS real
  # coverage. Judge by the last line only.
  last_line="$(printf '%s\n' "$out" | grep -v '^[[:space:]]*$' | tail -1)"
  if [ "$rc" -ne 0 ]; then
    printf "  FAIL  %-30s (exit %d)\n" "$name" "$rc"
    fail=$((fail+1))
    failed_names="$failed_names $name"
    # On failure, always show the harness output — that's the divergence.
    echo "$out" | sed 's/^/        | /'
  elif harness_says_skip "$out"; then
    # Print the harness's OWN reason, not a guess.
    #
    # This said "(toolchain absent)" for every skip. Two harnesses are OPT-IN
    # rather than unavailable — `browser_compute_parity` skips unless
    # `BROWSER_PARITY=1` and says so on that very line — and labelling that
    # "toolchain absent" tells the reader the environment cannot do it. On this
    # host it can: `BROWSER_PARITY=1` builds the oracle and runs
    # wasm-bindgen-test in headless Chrome. A reader, or a CI author, who
    # believed the label would leave real verification switched off.
    #
    # Trimmed to the part after the harness name, so the column stays readable.
    reason="$(echo "$last_line" | sed "s/^$name: *//" | cut -c1-58)"
    printf "  SKIP  %-30s (%s)\n" "$name" "${reason:-reason not stated}"
    skip=$((skip+1))
  else
    printf "  PASS  %-30s\n" "$name"
    pass=$((pass+1))
    [ "$QUIET" = 0 ] && echo "$last_line" | sed 's/^/        /'
  fi
done

echo ""
echo "parity_all: $pass passed, $skip skipped, $fail failed (of $((pass+skip+fail)) harnesses)"
if [ "$fail" -ne 0 ]; then
  echo "parity_all: FAILED —$failed_names"
  exit 1
fi

# AUDIT T36 (finding GATE-01). The only decision used to be `$fail -ne 0`, with
# no floor on $pass anywhere in this file — so an all-SKIP run printed
# "0 passed, 49 skipped, 0 failed" and then "PASS ✓". A box with no LLVM, or a
# breakage that made every harness bail early, was byte-indistinguishable from a
# green suite. This is the aggregator gate.sh relies on for invariant I-2, and
# gate.sh's own comment names ad-hoc parity running as how bugs
# #27/#36/#38/#39/parse_*_or reached main.
#
# The floor is deliberately below the observed healthy count (44 passed / 5
# skipped of 49 on 2026-08-04; the 5 are android + browser/wasm-browser, which
# need an NDK or headless Chrome) so an ordinary box missing a couple of
# toolchains still passes — but "nothing ran" and "half the suite vanished"
# cannot.
EXPECT_MIN_PASS="${EXPECT_MIN_PASS:-40}"
if [ "$pass" -lt "$EXPECT_MIN_PASS" ]; then
  echo "parity_all: FAILED — only $pass harness(es) actually PASSED, expected >= $EXPECT_MIN_PASS"
  echo "parity_all: a suite that skips everything is not a green suite. If this box"
  echo "            genuinely lacks toolchains, set EXPECT_MIN_PASS explicitly and say why."
  exit 1
fi

echo "parity_all: PASS — no interp↔codegen / AOT-wasm divergence ✓ ($pass harnesses asserted)"
exit 0
