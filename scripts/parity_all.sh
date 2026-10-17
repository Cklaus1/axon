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
#   AXON_HARNESS_STRICT=1 scripts/parity_all.sh  # a skip fails unless allow-listed
#
# Exit 0 iff no harness FAILED (skips are fine), and, under
# AXON_HARNESS_STRICT=1, no harness skipped that scripts/parity_allowed_skips.txt
# does not list with a reason (R50 §11).
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"

# ── AXON_HARNESS_STRICT (R50 §11) ───────────────────────────────────────────
# Without it every skip counts as success, so a harness that silently stops
# running (a toolchain probe gone wrong, a build that now "skips") vanishes from
# the suite while the run stays green. With AXON_HARNESS_STRICT=1 a skipped
# harness — including one skipped by PARITY_SKIP_WASM — fails the run unless
# scripts/parity_allowed_skips.txt lists it. Each entry is
# `<harness-name> <reason>`; an entry without a reason is itself an error,
# because an unexplained exemption is what this mode exists to prevent.
STRICT="${AXON_HARNESS_STRICT:-0}"
ALLOWED_SKIPS_FILE="$ROOT/scripts/parity_allowed_skips.txt"
declare -A allowed_skip=()
if [ "$STRICT" = 1 ]; then
  if [ ! -f "$ALLOWED_SKIPS_FILE" ]; then
    echo "parity_all: AXON_HARNESS_STRICT=1 but $ALLOWED_SKIPS_FILE is missing" >&2
    exit 2
  fi
  while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in ''|'#'*) continue ;; esac
    read -r _name _reason <<<"$line"
    if [ -z "${_reason:-}" ]; then
      echo "parity_all: $ALLOWED_SKIPS_FILE: entry '$_name' has no reason" >&2
      exit 2
    fi
    allowed_skip["$_name"]="$_reason"
  done <"$ALLOWED_SKIPS_FILE"
fi
strict_skip_names=""
# note_skip <name> — under AXON_HARNESS_STRICT=1, record a skip the allow-list
# does not cover.
note_skip() {
  [ "$STRICT" = 1 ] || return 0
  [ -n "${allowed_skip[$1]+x}" ] && return 0
  echo "        ^ not in scripts/parity_allowed_skips.txt: fails under AXON_HARNESS_STRICT=1"
  strict_skip_names="$strict_skip_names $1"
}

# ── THE skip rule ────────────────────────────────────────────────────────────
# `harness_says_skip` lives in scripts/lib/harness_skip.sh (sourced above), the
# one shell copy of the rule; scripts/test_harness_skip.sh pins it against the
# same rows as `skip_rule_table` in crates/axon-core/tests/cli_run.rs, and
# greps the scripts for refused shapes. A broken detector would misfile every
# harness, so it is checked before any runs; the message names the section.
if ! ths_out="$(bash "$ROOT/scripts/test_harness_skip.sh" 2>&1)"; then
  ths_failed="$(printf '%s\n' "$ths_out" | grep -oE '^test_harness_skip: FAIL \[[a-z]+\]' \
                | grep -oE '\[[a-z]+\]' | paste -sd ' ')"
  echo "parity_all: FAILED — scripts/test_harness_skip.sh failed in section(s) ${ths_failed:-unknown}:" >&2
  printf '%s\n' "$ths_out" >&2
  exit 2
fi

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
    note_skip "$name"
    continue
  fi

  out="$(bash "$h" 2>&1)"
  rc=$?

  # A harness's VERDICT is its exit status, then `harness_says_skip` (the rule
  # in scripts/lib/harness_skip.sh): the final non-empty line decides when it
  # states a skip or a verdict, else a column-0 skip in the last three lines.
  # Detecting "skip" anywhere in the output is wrong: a passing harness may
  # print a per-case "  SKIP <case>" line (e.g. wasm_aot_run skips a single loop
  # program) while its overall result is PASS. `last_line` is only for display
  # of a PASS; a SKIP's reason is the line the rule matched (`harness_skip_reason`).
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
    # The line is the one the skip rule matched, not the last line: a skip
    # followed by build-error text printed that error as the skip's reason.
    reason="$(harness_skip_reason "$out" | sed "s/^$name: *//" | cut -c1-58)"
    printf "  SKIP  %-30s (%s)\n" "$name" "${reason:-reason not stated}"
    skip=$((skip+1))
    note_skip "$name"
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
if [ -n "$strict_skip_names" ]; then
  echo "parity_all: FAILED — AXON_HARNESS_STRICT=1 and these harnesses skipped without an entry in scripts/parity_allowed_skips.txt:$strict_skip_names"
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
# The floor defaults to every harness that is not allow-listed to skip: the count
# of scripts/*_parity.sh minus the entries of scripts/parity_allowed_skips.txt
# that name one. It used to be a hard-coded 40 under a 44-of-49 healthy count,
# so a harness could stop running (and new harnesses accumulate) with nobody
# told. A box that genuinely lacks a toolchain sets EXPECT_MIN_PASS and says why.
if [ -z "${EXPECT_MIN_PASS:-}" ]; then
  _harnesses=0; _exempt=0
  for h in scripts/*_parity.sh; do _harnesses=$((_harnesses+1)); done
  if [ -f "$ALLOWED_SKIPS_FILE" ]; then
    while read -r _name _; do
      case "$_name" in ''|'#'*) continue ;; esac
      [ -f "scripts/$_name.sh" ] && _exempt=$((_exempt+1))
    done <"$ALLOWED_SKIPS_FILE"
  fi
  EXPECT_MIN_PASS=$((_harnesses - _exempt))
fi
if [ "$pass" -lt "$EXPECT_MIN_PASS" ]; then
  echo "parity_all: FAILED — only $pass harness(es) actually PASSED, expected >= $EXPECT_MIN_PASS"
  echo "parity_all: a suite that skips everything is not a green suite. If this box"
  echo "            genuinely lacks toolchains, set EXPECT_MIN_PASS explicitly and say why."
  exit 1
fi

echo "parity_all: PASS — no interp↔codegen / AOT-wasm divergence ✓ ($pass harnesses asserted)"
exit 0
