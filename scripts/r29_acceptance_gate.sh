#!/usr/bin/env bash
# R29 acceptance gate. Fails if any §0 check is missing, stubbed, or fails.
#
# Single source of "done" for R29: Continuous Compliance Monitor.
# FAILS if any required §0 check is missing, stubbed, or not green.
# Proves: monitor polls JSONL ledger; violations trip R27 kill-switch;
# fail-closed on monitor crash; ledger rotation followed; false-positive-free.
#
# Run: scripts/r29_acceptance_gate.sh
# Wire into: gate.sh --strict
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/.." && pwd)"
PASS=0; FAIL=0

ok()   { echo "PASS: $1"; PASS=$((PASS+1)); }
fail() { echo "FAIL: $1"; FAIL=$((FAIL+1)); }

# ── §-Gate step 1: presence check — every named check must exist ──────────────
echo "r29_acceptance_gate: (1) presence check — all §0 named checks must exist..."


# ── S2 (O037), extended sweep: require each named test to RUN and PASS ────────
# The original loop grepped the source for the NAME, which a comment, a
# docstring or an `#[ignore]`d body satisfies. One suite run is parsed for every
# name (not one cargo per name — nested cargo contends on the build lock that
# makes the parity harnesses flaky, O036).
require_named_tests_pass() {
    local log="$1"; shift
    local name
    for name in "$@"; do
        if grep -qE "^test .*${name}.* \.\.\. ok$" "$log"; then
            ok "ran and passed: $name"
        elif grep -qE "^test .*${name}.* \.\.\. ignored" "$log"; then
            fail "IGNORED (not run): $name — a name-grep would have called this green"
        else
            fail "did not run: $name (not present in the suite output)"
        fi
    done
}

CHECKS=(
  acc_a1_smoke_compliance_journey
  acc_a2_allowed_effects_pass_through
  acc_a3_quickstart_commands_execute
  acc_a4_hermetic_isolated_timeout
  acc_a5_deterministic_detection
  acc_a6_monitor_mandatory_fail_closed
  violation_detected_within_2s
  false_positive_rate_zero
  monitor_exit_matches_job_exit
  monitor_survives_ledger_rotation
)

SRC="$ROOT/crates/axon-os/tests/r29_compliance.rs"
if [[ ! -f "$SRC" ]]; then
  fail "r29_compliance.rs not found at $SRC"
else
  S2_LOG="$(mktemp)"
  cargo test -p axon-os 2>&1 | tee "$S2_LOG" | tail -3
  require_named_tests_pass "$S2_LOG" "${CHECKS[@]}"
  rm -f "$S2_LOG"
fi

# ── §-Gate step 2: anti-stub check ────────────────────────────────────────────
echo ""
echo "r29_acceptance_gate: (2) anti-stub check..."

ANTIPATTERNS='todo!()\|unimplemented!()\|assert!(true)'

if [[ -f "$SRC" ]]; then
  if grep -q '#\[ignore\]' "$SRC"; then
    fail "anti-stub: found #[ignore] annotations in $SRC"
  else
    ok "anti-stub: no #[ignore] annotations"
  fi

  if grep -q "$ANTIPATTERNS" "$SRC"; then
    fail "anti-stub: found todo!()/unimplemented!()/assert!(true) in $SRC"
  else
    ok "anti-stub: no stub patterns found"
  fi
fi

# ── §-Gate step 3: CONTAINMENT_VIOLATION_EXIT_CODE = 12 ──────────────────────
echo ""
echo "r29_acceptance_gate: (3) exit-code check — CONTAINMENT_VIOLATION_EXIT_CODE = 12..."

MONITOR_SRC="$ROOT/crates/axon-os/src/monitor.rs"
if [[ ! -f "$MONITOR_SRC" ]]; then
  fail "monitor.rs not found at $MONITOR_SRC"
else
  if grep -q 'CONTAINMENT_VIOLATION_EXIT_CODE.*=.*12' "$MONITOR_SRC"; then
    ok "exit-code: CONTAINMENT_VIOLATION_EXIT_CODE = 12 found in monitor.rs"
  else
    fail "exit-code: CONTAINMENT_VIOLATION_EXIT_CODE = 12 NOT found in monitor.rs"
  fi
fi

# ── §-Gate step 4: spec file presence ────────────────────────────────────────
echo ""
echo "r29_acceptance_gate: (4) spec file presence..."

SPEC="$ROOT/governance/specs/R29-continuous-compliance-monitor.md"
if [[ -f "$SPEC" ]]; then
  ok "spec: R29 spec file exists at governance/specs/R29-continuous-compliance-monitor.md"
else
  fail "spec: R29 spec file NOT found at $SPEC"
fi

# ── §-Gate step 5: monitor.rs exists and is non-trivial ─────────────────────
echo ""
echo "r29_acceptance_gate: (5) monitor.rs substance check..."

if [[ -f "$MONITOR_SRC" ]]; then
  LINES=$(wc -l < "$MONITOR_SRC")
  if [[ "$LINES" -gt 50 ]]; then
    ok "substance: monitor.rs has $LINES lines (>50)"
  else
    fail "substance: monitor.rs has only $LINES lines (expected >50)"
  fi

  if grep -q 'ComplianceMonitor' "$MONITOR_SRC"; then
    ok "substance: ComplianceMonitor struct found in monitor.rs"
  else
    fail "substance: ComplianceMonitor struct NOT found in monitor.rs"
  fi

  if grep -q 'ViolationDetected' "$MONITOR_SRC"; then
    ok "substance: MonitorResult::ViolationDetected found in monitor.rs"
  else
    fail "substance: MonitorResult::ViolationDetected NOT found in monitor.rs"
  fi

  if grep -q 'allowed_effects' "$MONITOR_SRC"; then
    ok "substance: allowed_effects field found in monitor.rs"
  else
    fail "substance: allowed_effects field NOT found in monitor.rs"
  fi
fi

# ── §-Gate step 6: CLI --monitor flag wired in ───────────────────────────────
echo ""
echo "r29_acceptance_gate: (6) CLI --monitor flag presence..."

CLI_SRC="$ROOT/crates/axon-os/src/cli.rs"
if [[ -f "$CLI_SRC" ]]; then
  if grep -q '"--monitor"' "$CLI_SRC"; then
    ok "cli: --monitor flag found in cli.rs"
  else
    fail "cli: --monitor flag NOT found in cli.rs"
  fi
  if grep -q '"--ledger"' "$CLI_SRC"; then
    ok "cli: --ledger flag found in cli.rs"
  else
    fail "cli: --ledger flag NOT found in cli.rs"
  fi
  if grep -q 'CONTAINMENT_VIOLATION_EXIT_CODE' "$CLI_SRC"; then
    ok "cli: CONTAINMENT_VIOLATION_EXIT_CODE used in cli.rs"
  else
    fail "cli: CONTAINMENT_VIOLATION_EXIT_CODE NOT used in cli.rs"
  fi
else
  fail "cli.rs not found at $CLI_SRC"
fi

# ── §-Gate step 7: cargo test r29 ───────────────────────────────────────────
echo ""
echo "r29_acceptance_gate: (7) cargo test -p axon-os r29..."

cd "$ROOT"
TEST_OUT=$(cargo test -p axon-os --test r29_compliance 2>&1)
if echo "$TEST_OUT" | grep -q 'test result: ok'; then
  PASSED=$(echo "$TEST_OUT" | grep 'test result: ok' | grep -oE '[0-9]+ passed' | head -1 | awk '{print $1}')
  ok "cargo test: r29 tests passed (${PASSED:-?} tests)"
else
  fail "cargo test: some r29 tests FAILED"
  echo "$TEST_OUT" | tail -20
fi

# ── §-Gate step 8: fail-closed invariants present in source ─────────────────
echo ""
echo "r29_acceptance_gate: (8) fail-closed invariants (I-1 through I-6)..."

if grep -q 'I-1\|I-2\|I-3\|I-4\|I-5\|I-6' "$MONITOR_SRC"; then
  ok "invariants: fail-closed I-* invariants documented in monitor.rs"
else
  fail "invariants: fail-closed I-* invariants NOT found in monitor.rs"
fi

if grep -q 'R29_TCB_ADDENDUM' "$MONITOR_SRC"; then
  ok "tcb: R29_TCB_ADDENDUM constant found in monitor.rs"
else
  fail "tcb: R29_TCB_ADDENDUM constant NOT found in monitor.rs"
fi

# ── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo "r29_acceptance_gate: PASS=$PASS FAIL=$FAIL"
if [[ "$FAIL" -gt 0 ]]; then
  echo "r29_acceptance_gate: GATE FAILED — $FAIL checks did not pass"
  exit 1
else
  echo "r29_acceptance_gate: ALL CHECKS PASSED"
  exit 0
fi
