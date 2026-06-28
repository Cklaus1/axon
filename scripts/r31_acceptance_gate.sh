#!/usr/bin/env bash
# R31 acceptance gate — validates every §0 normative check exists and passes.
# Exit 0 = all checks green.  Any failure prints which check failed and exits non-zero.
# Wire into gate.sh --strict once R28/R29 reach stable artifact paths.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FAIL=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; FAIL=1; }

echo "=== R31 acceptance gate ==="

# ── 1. Presence check: every §0 normative test name must appear in sources ───
echo ""
echo "1. Normative test name presence check"

REQUIRED_NAMES=(
    "acc_a1_smoke_extended_tcb_journey"
    "acc_a2_byte_identical_across_runs"
    "acc_a3_quickstart_commands_execute"
    "acc_a4_hermetic_isolated_timeout"
    "acc_a5_tampered_component_detected"
    "acc_a6_chaining_order_canonical"
    "missing_component_fails_closed"
    "component_version_in_report"
    "r26_baseline_backward_compatible"
    "extended_tcb_wired_into_run"
)

LIB_SRC="$REPO_ROOT/crates/axon-attest/src/lib.rs"
VM_SRC="$REPO_ROOT/crates/axon-vm/src/main.rs"

for name in "${REQUIRED_NAMES[@]}"; do
    if grep -q "$name" "$LIB_SRC" "$VM_SRC" 2>/dev/null; then
        pass "found: $name"
    else
        fail "MISSING test name: $name (check $LIB_SRC and $VM_SRC)"
    fi
done

# ── 2. Anti-stub check: no todo!/unimplemented!/assert!(true) in R31 tests ───
echo ""
echo "2. Anti-stub check"

# The one permitted #[ignore] annotation has reason string "R28 not yet shipped"
STUB_VIOLATIONS=$(grep -n 'todo!\|unimplemented!\|assert!(true)' \
    "$LIB_SRC" "$VM_SRC" 2>/dev/null | grep -v '^Binary' || true)

if [ -z "$STUB_VIOLATIONS" ]; then
    pass "no stub patterns (todo!/unimplemented!/assert!(true)) found in R31 sources"
else
    fail "stub patterns found — tests must not be stubbed:\n$STUB_VIOLATIONS"
fi

# ── 3. Anti-vacuous tamper check: acc_a5 must exercise ≥3 slot variants ─────
echo ""
echo "3. Anti-vacuous tamper check (acc_a5 must tamper ≥3 slots)"

# Count "Tamper slot" markers in the test
TAMPER_COUNT=$(grep -c "Tamper slot" "$LIB_SRC" 2>/dev/null || echo 0)
if [ "$TAMPER_COUNT" -ge 3 ]; then
    pass "acc_a5_tampered_component_detected exercises $TAMPER_COUNT slot-tamper cases (≥3 required)"
else
    fail "acc_a5 only exercises $TAMPER_COUNT slot-tamper cases — need ≥3"
fi

# ── 4. Canonical-order proof: acc_a6 must verify reordering produces different digest ──
echo ""
echo "4. Canonical-order proof check"

if grep -q "swapping kernel and axon-os" "$LIB_SRC" 2>/dev/null; then
    pass "acc_a6_chaining_order_canonical proves reordering produces a different digest"
else
    fail "acc_a6 does not prove that reordering produces a different digest"
fi

# ── 5. R26 regression check ───────────────────────────────────────────────────
echo ""
echo "5. R26 regression check (axon-attest tests must all pass)"

if cargo test -p axon-attest --quiet 2>&1; then
    pass "R26 tests still pass (all axon-attest tests green)"
else
    fail "R26 tests FAILED — R31 must not regress R26"
fi

# ── 6. Full R31 test suite ────────────────────────────────────────────────────
echo ""
echo "6. Full test suite (axon-attest + axon-vm)"

if cargo test -p axon-attest -p axon-vm --quiet 2>&1; then
    pass "cargo test -p axon-attest -p axon-vm: all tests pass"
else
    fail "cargo test -p axon-attest -p axon-vm: FAILED"
fi

# ── 7. Schema version check: lib functions produce the right schema strings ───
echo ""
echo "7. Schema version check"

if grep -q '"axon-vm-report/2"' "$LIB_SRC" 2>/dev/null; then
    pass "report_to_json_extended produces schema axon-vm-report/2"
else
    fail "schema 'axon-vm-report/2' not found in $LIB_SRC"
fi

if grep -q '"axon-vm-report/1"' "$LIB_SRC" 2>/dev/null; then
    pass "report_to_json (R26) produces schema axon-vm-report/1"
else
    fail "schema 'axon-vm-report/1' not found in $LIB_SRC"
fi

# Verify the schema check in the test itself
if grep -q 'schema.*axon-vm-report/2' "$LIB_SRC" 2>/dev/null; then
    pass "r26_baseline_backward_compatible asserts schema /2 for extended reports"
else
    fail "no schema /2 assertion found in R31 tests"
fi

# ── Final result ──────────────────────────────────────────────────────────────
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "=== R31 acceptance gate: ALL CHECKS PASSED ==="
    exit 0
else
    echo "=== R31 acceptance gate: FAILED (see above) ==="
    exit 1
fi
