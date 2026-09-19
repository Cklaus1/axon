#!/usr/bin/env bash
# acceptance_gate.sh — R21 §10: the pinned acceptance gate for axon-os. The
# single source of "done". FAILS if any required acceptance check is missing or
# stubbed, then runs the full suite + the real-CLI journey + reproducibility.
#
# Wire into gate.sh --strict.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CRATE="crates/axon-os"
SRC="$CRATE/src $CRATE/tests"
fail=0

# The named checks the build must contain (R21 §0). Missing any → gate fails.
REQUIRED=(
  acc_a1_smoke_user_journey
  acc_a2_example_jobs_run_and_overreach_denied
  acc_a3_quickstart_commands_execute
  acc_a4_hermetic_isolated_timeout
  acc_a5_deterministic_byte_identical
  acc_a6_record_tamper_detected
  gate_denies_effect_outside_grant
  runtime_overreach_fails_closed
  mint_cannot_exceed_supervisor_grant
  replay_reproduces_and_verifies
)

echo "acceptance_gate: (1) presence check…"
for name in "${REQUIRED[@]}"; do
  # Require a real DEFINITION, not the name appearing anywhere. The loose form
  # was satisfied by the name in a comment, so deleting a required check while
  # leaving a `// see foo_test` behind kept the gate green — the exact failure
  # r28_acceptance_gate.sh records having hit and fixed for itself.
  if ! grep -rqsE "^[[:space:]]*(pub )?(async )?fn $name\\(" $SRC; then
    echo "  MISSING required check: $name"
    fail=1
  fi
done

echo "acceptance_gate: (2) anti-stub check…"
# No acceptance test may be ignored or stubbed.
if grep -rqsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC; then
  echo "  found an #[ignore]/todo!()/unimplemented!() in the test surface"
  grep -rnsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC
  fail=1
fi
# A bare `assert!(true)` is a no-op stub.
if grep -rqsE 'assert!\(\s*true\s*\)' $SRC; then
  echo "  found a no-op assert!(true) stub"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "acceptance_gate: FAILED (missing or stubbed checks)"
  exit 1
fi

echo "acceptance_gate: (3) building axon-os + the interpreter…"
if ! cargo build -q -p axon-os --bin axon-os 2>/dev/null; then
  echo "  axon-os build failed"; exit 1
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "acceptance_gate: interpreter build unavailable — running pure tests only"
fi
export AXON_BIN="$ROOT/target/debug/axon"

echo "acceptance_gate: (4) full suite (unit + acceptance, incl. the real-CLI journey)…"
if ! cargo test -q -p axon-os; then
  echo "  test suite failed"; exit 1
fi

echo "acceptance_gate: (5) reproducibility — same job+seed ⇒ byte-identical record…"
# Guard on the binary this check actually INVOKES, and resolve it through
# CARGO_TARGET_DIR. The guard used to test `target/debug/axon` while running
# `target/debug/axon-os`, which failed both ways: with a custom
# CARGO_TARGET_DIR the hardcoded path does not exist, both runs fail, and the
# `diff` then reports "records are NOT byte-identical (A5 violation)" — a
# reproducibility failure that never happened, blamed on the wrong thing. In
# the other direction a runnable check was skipped as "interpreter absent".
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
OSBIN="$TARGET_DIR/debug/axon-os"
if [ -x "$OSBIN" ]; then
  W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
  # A run that fails is reported as a run that failed, not as a diff result.
  if ! "$OSBIN" run examples/jobs/summarize.axjob --run-id d --out "$W/a" >/dev/null \
     || ! "$OSBIN" run examples/jobs/summarize.axjob --run-id d --out "$W/b" >/dev/null; then
    echo "  axon-os could not complete the job — this is NOT an A5 result"; exit 1
  fi
  if ! diff -q "$W/a/d.json" "$W/b/d.json" >/dev/null; then
    echo "  records are NOT byte-identical across runs (A5 violation)"; exit 1
  fi
  echo "  ✓ byte-identical"
else
  # A skip must name the thing that is actually missing, at the path actually
  # searched, or it hides the defect it was meant to report.
  echo "  SKIPPED: no axon-os binary at $OSBIN"; exit 1
fi

echo "acceptance_gate: OK — every R21 §0 check present, unstubbed, and green"
exit 0
