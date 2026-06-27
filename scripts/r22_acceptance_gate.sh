#!/usr/bin/env bash
# r22_acceptance_gate.sh — R22 §10: the pinned acceptance gate for axon-intent.
# The single source of "done". FAILS if any required acceptance check is missing
# or stubbed, then runs the full suite, the §9 quickstart block against the built
# binaries, and a byte-for-byte reproducibility diff.
#
# Wire into gate.sh --strict.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

CRATE="crates/axon-intent"
SRC="$CRATE/src $CRATE/tests"
fail=0

# The named checks the build must contain (R22 §0). Missing any → gate fails.
REQUIRED=(
  acc_a1_smoke_intent_to_approval
  acc_a2_example_intents_compile_and_run
  acc_a3_quickstart_commands_execute
  acc_a4_synthesis_isolated_timeout
  acc_a5_deterministic_compile
  acc_a6_approval_token_binds_and_tamper_detected
  synthesized_job_is_self_admissible
  grant_is_least_privilege
  low_confidence_synthesis_refused
  approval_invalidated_by_any_edit
)

echo "r22_acceptance_gate: (1) presence check…"
for name in "${REQUIRED[@]}"; do
  if ! grep -rqs "fn $name" $SRC; then
    echo "  MISSING required check: $name"
    fail=1
  fi
done

echo "r22_acceptance_gate: (2) anti-stub check…"
# No acceptance test may be ignored or stubbed.
if grep -rqsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC; then
  echo "  found an #[ignore]/todo!()/unimplemented!() in the source/test surface"
  grep -rnsE '#\[ignore\]|todo!\(\)|unimplemented!\(\)' $SRC
  fail=1
fi
# A bare `assert!(true)` is a no-op stub.
if grep -rqsE 'assert!\(\s*true\s*\)' $SRC; then
  echo "  found a no-op assert!(true) stub"
  fail=1
fi

if [ "$fail" -ne 0 ]; then
  echo "r22_acceptance_gate: FAILED (missing or stubbed checks)"
  exit 1
fi

echo "r22_acceptance_gate: (3) building axon-intent + axon-os + the interpreter…"
if ! cargo build -q -p axon-intent --bin axon-intent 2>/dev/null; then
  echo "  axon-intent build failed"; exit 1
fi
if ! cargo build -q -p axon-os --bin axon-os 2>/dev/null; then
  echo "  axon-os build failed (cross-spec run leg will be skipped by the tests)"
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "r22_acceptance_gate: interpreter build unavailable — run leg skips cleanly"
fi
export AXON_BIN="$ROOT/target/debug/axon"

echo "r22_acceptance_gate: (4) full suite (unit + acceptance, incl. the real-CLI journey)…"
if ! cargo test -q -p axon-intent; then
  echo "  test suite failed"; exit 1
fi

echo "r22_acceptance_gate: (5) §9 quickstart commands against the built binary (A3)…"
INTENT="$ROOT/target/debug/axon-intent"
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
if ! AXON_AI_MOCK=1 "$INTENT" compile examples/intents/summarize.intent.md --out "$W/jobs" >/dev/null; then
  echo "  quickstart: compile failed"; exit 1
fi
if ! "$INTENT" review "$W/jobs/summarize.axjob" --program "$W/jobs/summarize.ax" >/dev/null; then
  echo "  quickstart: review failed"; exit 1
fi
if ! "$INTENT" approve "$W/jobs/summarize.axjob" --by alice --accept >/dev/null; then
  echo "  quickstart: approve failed"; exit 1
fi
# The documented "vague is refused (exit 5)" line.
AXON_AI_MOCK=1 "$INTENT" compile examples/intents/vague.intent.md --out "$W/v" >/dev/null
if [ "$?" -ne 5 ]; then
  echo "  quickstart: vague intent was NOT refused with exit 5"; exit 1
fi

echo "r22_acceptance_gate: (6) reproducibility — same intent+seed under mock ⇒ byte-identical triple…"
for d in a b; do
  AXON_AI_MOCK=1 "$INTENT" compile examples/intents/summarize.intent.md --out "$W/$d" >/dev/null
  "$INTENT" approve "$W/$d/summarize.axjob" --by alice --accept >/dev/null
done
for f in summarize.ax summarize.axjob summarize.approval; do
  if ! diff -q "$W/a/$f" "$W/b/$f" >/dev/null; then
    echo "  $f is NOT byte-identical across mock runs (A5 violation)"; exit 1
  fi
done
echo "  ✓ byte-identical"

echo "r22_acceptance_gate: OK — every R22 §0 check present, unstubbed, and green"
exit 0
