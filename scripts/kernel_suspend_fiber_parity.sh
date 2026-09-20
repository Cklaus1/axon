#!/usr/bin/env bash
# kernel_suspend_fiber_parity.sh — the kernel scheduler's FiberState::Suspended
# state machine (R15 Slice 3) behaves consistently: a fiber PARKED at a
# host_await suspension leaves the Ready set (the scheduler won't re-run it) and
# is non-terminal (not counted done/failed), and resume() unparks it back to
# Ready exactly once (a second / unknown resume is a no-op false — the §6
# "unknown/already-finished token" host-side error).
#
# The scheduler is a pure, interpreter-owned Rust state machine (the fiber BODIES
# run in the interpreter; this module owns only the queue + state bookkeeping), so
# the "parity" here is between the documented behavior and the gated unit test
# that pins it — the same shape as the other *_parity.sh state-machine harnesses.
# No codegen / LLVM is involved (the scheduler is interp-side only).
#
# Vacuous-pass guard: counts the cases run; FAILS on zero.
set -uo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"

ran=0
fail=0

echo "kernel_suspend_fiber_parity: exercising the FiberState::Suspended state machine…"
TESTS=(
  kernel::tests::scheduler_suspend_parks_off_queue_and_resume_requeues
  # Regression: the pre-existing fanout/restart state machine must still hold with
  # the new Suspended arm added (no terminal-tally drift).
  kernel::tests::scheduler_fanout_collect_and_failure_is_observable
)
for t in "${TESTS[@]}"; do
  # `cargo test <filter>` exits 0 when the filter matches NOTHING, so testing
  # the exit status alone counted a renamed/moved/#[ignore]d test as "ran" —
  # and the `ran -eq 0` vacuity guard below then saw a healthy count.
  if cargo_test_must_run "$t" \
       cargo test -q -p axon-core --no-default-features --lib "$t" -- --exact; then
    echo "  ok  [$t]"
    ran=$((ran+1))
  else
    echo "kernel_suspend_fiber_parity: FAIL [$t]"
    fail=1
  fi
done

if [ "$ran" -eq 0 ]; then
  echo "kernel_suspend_fiber_parity: FAIL — ran 0 cases (vacuous pass; tests vanished?)"
  exit 1
fi
if [ "$fail" -ne 0 ]; then
  echo "kernel_suspend_fiber_parity: FAILED ($ran cases attempted)"
  exit 1
fi
echo "kernel_suspend_fiber_parity: PASS — $ran cases (park-off-queue / resume-requeues / no-tally-drift)"
