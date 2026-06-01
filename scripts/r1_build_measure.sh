#!/usr/bin/env bash
# R1 native-build measurement harness
# ─────────────────────────────────────────────────────────────────────────────
# Purpose: produce a HONEST signal on whether the inline-IR→axon-rt migration
# (governance/specs/R1-codegen-build-unblock.md) is moving the stalled native
# build toward "finishes". The full build hangs on the dev box (BUILD_DIAGNOSIS.md:
# >25 min, never finishes), so this script measures the leading indicators that
# DO terminate, plus a TIME-BOUNDED full build that reports how far it got.
#
# RUN THIS ON A BEEFY MACHINE (high core count + RAM). The maintainers' own note:
# "faster-machine validation" (BUILD_DIAGNOSIS.md §6). On a laptop the bounded
# build will simply time out — that is itself a data point, but the IR-volume and
# per-function metrics below are machine-independent and are the real progress
# signal.
#
# Usage:
#   scripts/r1_build_measure.sh                 # all phases, default 45-min build cap
#   BUILD_CAP_SECS=5400 scripts/r1_build_measure.sh   # 90-min cap on a fast box
#   PHASES="metrics ir" scripts/r1_build_measure.sh   # skip the slow full build
#
# Phases (space-separated in $PHASES, default "metrics ir build"):
#   metrics  – static IR-volume proxies (instant, machine-independent). The
#              primary progress metric: build_wrappers:: call count + inline
#              body count in codegen/builtins.rs vs the BUILD_DIAGNOSIS baseline.
#   ir       – cargo check timing (frontend, always finishes ~5s) as the control,
#              proving the frontend stays fast and isolating the backend.
#   build    – TIME-BOUNDED `cargo build -p axon-core` (codegen feature). Reports
#              wall-clock if it finishes, or how many axon_core*.o objects were
#              emitted before the cap (the "did it even reach object emission"
#              signal from BUILD_DIAGNOSIS Experiment A).
# ─────────────────────────────────────────────────────────────────────────────
set -uo pipefail
cd "$(dirname "$0")/.." || exit 1
REPO="$(pwd)"

BUILD_CAP_SECS="${BUILD_CAP_SECS:-2700}"   # 45 min default
PHASES="${PHASES:-metrics ir build}"
BUILTINS="crates/axon-core/src/codegen/builtins.rs"

# BUILD_DIAGNOSIS.md baselines (2026-05-27, pre-migration) — the reference points.
BASELINE_WRAPPER_CALLS=951      # build_wrappers:: + .build_ in builtins.rs at diagnosis time
BASELINE_INLINE_BODIES=158      # hand-emitted inline IR function bodies
BASELINE_CHECK_SECS="4.5"       # cargo check (frontend, finishes)
BASELINE_BUILD="UNBOUNDED (>25min, never finished)"

ts()   { date -u +%Y-%m-%dT%H:%M:%SZ; }
line() { printf '%s\n' "────────────────────────────────────────────────────────"; }

echo "R1 build measurement — $(ts)"
echo "repo: $REPO"
echo "HEAD: $(git rev-parse --short HEAD 2>/dev/null) ($(git rev-parse --abbrev-ref HEAD 2>/dev/null))"
echo "cores: $(nproc 2>/dev/null || echo '?')   mem: $(free -h 2>/dev/null | awk '/^Mem:/{print $2}' || echo '?')"
echo "phases: $PHASES   build cap: ${BUILD_CAP_SECS}s"
line

# ── Phase: metrics ───────────────────────────────────────────────────────────
if [[ " $PHASES " == *" metrics "* ]]; then
  echo "[metrics] static IR-volume proxies (machine-independent — the progress metric)"
  wrapper_calls=$(grep -cE 'build_wrappers::|\.build_' "$BUILTINS")
  inline_bodies=$(grep -c 'append_basic_block' "$BUILTINS")
  externs=$(grep -c 'add_function("__axon_' "$BUILTINS")
  total_lines=$(wc -l < "$BUILTINS")

  pct_wrap=$(awk "BEGIN{printf \"%.1f\", (1-$wrapper_calls/$BASELINE_WRAPPER_CALLS)*100}")
  pct_body=$(awk "BEGIN{printf \"%.1f\", (1-$inline_bodies/$BASELINE_INLINE_BODIES)*100}")

  printf '  %-28s %8s  %10s  %s\n' "metric" "baseline" "now" "reduction"
  printf '  %-28s %8s  %10s  %s%%\n' "IR-builder calls"  "$BASELINE_WRAPPER_CALLS" "$wrapper_calls" "$pct_wrap"
  printf '  %-28s %8s  %10s  %s%%\n' "inline IR bodies"  "$BASELINE_INLINE_BODIES" "$inline_bodies" "$pct_body"
  printf '  %-28s %8s  %10s\n'       "migrated externs"  "0" "$externs"
  printf '  %-28s %8s  %10s\n'       "builtins.rs lines" "3961" "$total_lines"

  # Lever 2 metric (BUILD_DIAGNOSIS_2.md): DIRECT unwrapped inkwell builder
  # calls across ALL codegen. Each is a distinct generic instantiation that
  # mono-collection must walk; routing through a non-generic #[inline(never)]
  # wrapper collapses N→1. THIS is the real stall driver (mono-collection),
  # not LLVM-IR lines.
  direct_calls=$(grep -rhcE 'self\.ir\.builder\.build_' crates/axon-core/src/codegen/*.rs | awk '{s+=$1} END{print s}')
  printf '  %-28s %8s  %10s\n'       "DIRECT inkwell calls (lever2)" "~350" "$direct_calls"
  echo "  → DIRECT-call count is the mono-collection proxy (BUILD_DIAGNOSIS_2.md):"
  echo "    the live stall is collect_items_rec normalizing inkwell generics, and"
  echo "    each direct build_* is one instantiation. Wrapping collapses N→1."
  line
fi

# ── Phase: ir (cargo check control) ──────────────────────────────────────────
if [[ " $PHASES " == *" ir "* ]]; then
  echo "[ir] cargo check timing (frontend control — must stay fast; isolates backend)"
  # Touch builtins.rs so check actually re-runs the crate.
  touch "$BUILTINS"
  start=$(date +%s.%N)
  if cargo check -p axon-core >/tmp/r1_check.log 2>&1; then
    end=$(date +%s.%N)
    secs=$(awk "BEGIN{printf \"%.2f\", $end-$start}")
    echo "  cargo check -p axon-core: ${secs}s (baseline ~${BASELINE_CHECK_SECS}s)"
    echo "  → frontend finishes. If this is fast and the build below is not, the cost"
    echo "    is 100% backend IR-gen/codegen (the BUILD_DIAGNOSIS verdict)."
  else
    echo "  cargo check FAILED — see /tmp/r1_check.log (fix before measuring the build)"
  fi
  line
fi

# ── Phase: build (the real, time-bounded gate) ───────────────────────────────
if [[ " $PHASES " == *" build "* ]]; then
  echo "[build] TIME-BOUNDED cargo build -p axon-core (codegen) — cap ${BUILD_CAP_SECS}s"
  echo "  this is the only metric that answers 'does it finish'. On a laptop it will"
  echo "  likely hit the cap; run on a high-core box for a real verdict."
  # Clean only axon-core's artifacts so deps stay warm (don't rebuild LLVM/inkwell).
  cargo clean -p axon-core 2>/dev/null
  objdir="target/debug/deps"
  before=$(find "$objdir" -name 'axon_core*.o' 2>/dev/null | wc -l)
  start=$(date +%s)
  # CARGO_INCREMENTAL=0 for a clean measurement, matching BUILD_DIAGNOSIS.
  timeout "${BUILD_CAP_SECS}" env CARGO_INCREMENTAL=0 \
    cargo build -p axon-core >/tmp/r1_build.log 2>&1
  rc=$?
  end=$(date +%s)
  elapsed=$((end - start))
  after=$(find "$objdir" -name 'axon_core*.o' 2>/dev/null | wc -l)
  emitted=$((after - before))

  if [[ $rc -eq 0 ]]; then
    echo "  ✅ BUILD FINISHED in ${elapsed}s  (baseline: $BASELINE_BUILD)"
    echo "  → the migration crossed the finish threshold. Record this number."
    # If it finishes, run the real R1 acceptance: native vs interpreter parity.
    echo "  next: run the native-vs-interpreter corpus parity (R1 §8 acceptance)."
  elif [[ $rc -eq 124 ]]; then
    echo "  ⏱  TIMED OUT at ${BUILD_CAP_SECS}s (did not finish)."
    echo "     axon_core objects emitted before cap: ${emitted}"
    echo "     → compare 'objects emitted' to a prior run: rising = progress toward"
    echo "       object-emission; still 0 = stuck in serial IR-gen (BUILD_DIAGNOSIS"
    echo "       Experiment A had 0 objects in 25min pre-migration). tail /tmp/r1_build.log:"
    tail -3 /tmp/r1_build.log | sed 's/^/       /'
  else
    echo "  ❌ build exited $rc (not a timeout) — see /tmp/r1_build.log"
    tail -5 /tmp/r1_build.log | sed 's/^/       /'
  fi
  line
fi

echo "done — $(ts)"
echo
echo "Interpretation guide:"
echo "  • IR-builder-call % (metrics) is the machine-independent progress signal."
echo "    It rises with every migrated builtin. BUILD_DIAGNOSIS ties the stall to"
echo "    this volume, so a large % cut is the prerequisite for a finishing build."
echo "  • 'objects emitted before cap' (build, on timeout) is the secondary signal:"
echo "    pre-migration it was 0 in 25min. >0 means codegen reached object emission."
echo "  • A finishing build (rc=0) is the only definitive 'past R1' signal. Record"
echo "    the wall-clock and re-run after each batch to chart the curve."
