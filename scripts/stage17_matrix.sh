#!/usr/bin/env bash
# Localize the stage-17 VM loss. DIAGNOSTIC ONLY — this changes nothing about
# the gate and concludes nothing on its own.
#
# Three strict-gate runs (gate8, gate13, gate14) died at
# `per-requirement acceptance gates (R22, R44)` with the WSL VM restarting;
# gate15 passed the same stage. A jobs cap below is a PROBE, not a proposed
# fix: if -j1 survives while unrestricted repeatedly dies, that points at
# concurrency; if every isolated case survives while the full gate dies, it
# points at state accumulated by stages 1-16.
set -uo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
RM=scripts/run_managed.sh
PROBE=scripts/stage17_probe.sh
RESULTS="$HOME/axon-scratch/stage17/matrix.tsv"
mkdir -p "$(dirname "$RESULTS")"
[ -s "$RESULTS" ] || printf 'case\tstatus\tseconds\tsamples\tsample_file\n' > "$RESULTS"

run_case() {
  local name="$1"; shift
  echo "── case: $name ──"
  local f; f="$($PROBE start "$name")"
  $PROBE mark "BEGIN $name"
  local t0=$SECONDS
  local d; d=$("$RM" start "s17_$name" -- "$@")
  # Poll rather than block, so a VM loss leaves the samples on disk either way.
  local st="running"
  while :; do
    st=$("$RM" status "$d")
    case "$st" in running|starting) ;; *) break ;; esac
    sleep 5
  done
  local secs=$((SECONDS - t0))
  $PROBE mark "END $name status=$st"
  $PROBE stop
  printf '%s\t%s\t%s\t%s\t%s\n' "$name" "$st" "$secs" "$(grep -c '^---' "$f")" "$f" >> "$RESULTS"
  echo "   $name -> $st (${secs}s)"
  # Deliberately NOT removing the run dir: its log is the only record of what
  # the harness printed before a loss.
}

# PRIME the cache the way the gate leaves it. Without this the matrix measures
# the wrong thing entirely: run standalone with a warm cache, R44 finishes in
# 5 SECONDS, where in the gate it is one of two back-to-back full rebuilds of
# axon-core.
#
# The gate flips axon-core's feature set at least four times — `cargo build -p
# axon-core` (default, codegen/LLVM), `cargo test -p axon-core
# --no-default-features`, then default again for the integration and
# builtin-externs tests. Stage 17 contains the worst pair: R22 builds
# `--no-default-features` and R44 immediately tests with defaults, so each
# rebuilds axon-core and everything downstream, the default side including the
# whole inkwell codegen path.
#
# Priming with DEFAULT features reproduces the state stage 17 actually starts
# from, so R22's flip costs what it costs in the gate.
prime_default() {
  echo "   priming cache with DEFAULT features (as stages 1-16 leave it)…"
  cargo build -q -p axon-core >/dev/null 2>&1 || true
}

case "${1:-all}" in
  cold_r22)  prime_default; run_case r22_cold bash scripts/r22_acceptance_gate.sh ;;
  cold_both) prime_default; run_case r22_then_r44_cold bash -c 'bash scripts/r22_acceptance_gate.sh && bash scripts/r44_acceptance_gate.sh' ;;
  cold_j1)   prime_default; run_case r22_then_r44_cold_j1 env CARGO_BUILD_JOBS=1 bash -c 'bash scripts/r22_acceptance_gate.sh && bash scripts/r44_acceptance_gate.sh' ;;
  cold_j4)   prime_default; run_case r22_then_r44_cold_j4 env CARGO_BUILD_JOBS=4 bash -c 'bash scripts/r22_acceptance_gate.sh && bash scripts/r44_acceptance_gate.sh' ;;
  r22)      run_case r22_only bash scripts/r22_acceptance_gate.sh ;;
  r44)      run_case r44_only bash scripts/r44_acceptance_gate.sh ;;
  both)     run_case r22_then_r44 bash -c 'bash scripts/r22_acceptance_gate.sh && bash scripts/r44_acceptance_gate.sh' ;;
  j1)       run_case r44_j1 env CARGO_BUILD_JOBS=1 bash scripts/r44_acceptance_gate.sh ;;
  j4)       run_case r44_j4 env CARGO_BUILD_JOBS=4 bash scripts/r44_acceptance_gate.sh ;;
  all)
    for c in r22 r44 both j1 j4; do bash "$0" "$c"; done
    echo; echo "── matrix ──"; column -t -s$'\t' "$RESULTS" ;;
  show)     column -t -s$'\t' "$RESULTS" ;;
  *) echo "usage: stage17_matrix.sh {r22|r44|both|j1|j4|all|show}" >&2; exit 2 ;;
esac
