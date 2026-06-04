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

  if [ "$rc" -ne 0 ]; then
    printf "  FAIL  %-30s (exit %d)\n" "$name" "$rc"
    fail=$((fail+1))
    failed_names="$failed_names $name"
    # On failure, always show the harness output — that's the divergence.
    echo "$out" | sed 's/^/        | /'
  elif echo "$out" | grep -qiE "skip|unavailable"; then
    printf "  SKIP  %-30s (toolchain absent)\n" "$name"
    skip=$((skip+1))
  else
    printf "  PASS  %-30s\n" "$name"
    pass=$((pass+1))
    [ "$QUIET" = 0 ] && echo "$out" | tail -1 | sed 's/^/        /'
  fi
done

echo ""
echo "parity_all: $pass passed, $skip skipped, $fail failed (of $((pass+skip+fail)) harnesses)"
if [ "$fail" -ne 0 ]; then
  echo "parity_all: FAILED —$failed_names"
  exit 1
fi
echo "parity_all: PASS — no interp↔codegen / AOT-wasm divergence ✓"
exit 0
