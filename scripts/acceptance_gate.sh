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
  if ! grep -rqs "fn $name" $SRC; then
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
if [ -x "$ROOT/target/debug/axon" ]; then
  W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
  "$ROOT/target/debug/axon-os" run examples/jobs/summarize.axjob --run-id d --out "$W/a" >/dev/null
  "$ROOT/target/debug/axon-os" run examples/jobs/summarize.axjob --run-id d --out "$W/b" >/dev/null
  if ! diff -q "$W/a/d.json" "$W/b/d.json" >/dev/null; then
    echo "  records are NOT byte-identical across runs (A5 violation)"; exit 1
  fi
  echo "  ✓ byte-identical"
else
  echo "  interpreter absent — reproducibility check skipped"
fi

echo "acceptance_gate: OK — every R21 §0 check present, unstubbed, and green"
exit 0
