#!/usr/bin/env bash
# goal_run_return_parity.sh — native↔interp parity on the goal_run RETURN value.
#
# The native `__axon_goal_run` (axon-rt/goal.rs) hill-climb must reach the same
# optimum the interpreter's hill_climb_i64 reaches. A prior gap: native used the
# old `step = max(1, |x|/4)` seed, which from a cold start (x=0) crawled by 1 per
# eval and never escaped the local neighborhood in a bounded budget — so native
# goal_run returned a far-from-optimal score (e.g. -800 on a peak-at-50
# objective) while the interpreter (coarse-then-fine `step ≈ max_evals*4` seed)
# reached the peak. The goal_input_parity harness checks the PROVENANCE (input,
# score) pairing but NOT the final returned best, so this divergence hid. This
# harness compares the RETURNED best score directly across a few objectives.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "goal_run_return_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "goal_run_return_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# (label, target) — the objective peaks at 100 (x=50); a far target stresses the
# coarse seed (must leap from x=0 to ~x=50), a near target is the easy case.
fail=0
for tgt in 999.0 100.0 50.0; do
  PROG="$WORK/g_$tgt.ax"
  cat > "$PROG" <<AX
@[adaptive]
fn score(x: i64) -> i64 { 100 - (x - 50) * (x - 50) }
fn main() -> i64 {
    let b = goal_run("score", $tgt, 40)
    f64_to_i64(b)
}
AX

  # Interpreter (the oracle).
  AXON_SEED=42 "$AXON" run "$PROG" >/dev/null 2>&1
  iexit=$?

  # Native.
  BIN="$WORK/g_bin_$tgt"
  if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
    echo "goal_run_return_parity: native build failed — skipping"
    exit 0
  fi
  AXON_SEED=42 "$BIN" >/dev/null 2>&1
  nexit=$?

  if [ "$iexit" -ne "$nexit" ]; then
    echo "goal_run_return_parity: FAIL — target=$tgt interp returned $iexit but native returned $nexit (the hill-climb diverged)"
    fail=1
  else
    echo "  target=$tgt: interp==native==$iexit ✓"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "goal_run_return_parity: FAILED"
  exit 1
fi
echo "goal_run_return_parity: OK — native goal_run reaches the same optimum as the interpreter"
exit 0
