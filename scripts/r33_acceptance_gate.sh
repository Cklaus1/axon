#!/usr/bin/env bash
# R33 acceptance gate — cross-VM safety quorum (scoped slice: file-based
# propose/vote/check CLI + pure check_quorum aggregator).
#
# Exit 0 = all checks green. Any failure prints which check failed and exits
# non-zero. Follows the pattern of scripts/r31_acceptance_gate.sh.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
FAIL=0

pass() { echo "  ✓ $1"; }
fail() { echo "  ✗ $1"; FAIL=1; }

echo "=== R33 acceptance gate ==="

QUORUM_DIR="$REPO_ROOT/crates/axon-vm/src/quorum"

# ── 1. Presence check: every required test name must appear in sources ──────
echo ""
echo "1. Test name presence check"

REQUIRED_NAMES=(
    "check_quorum_empty_votes_not_met"
    "check_quorum_3_of_5_meets_strict_majority"
    "check_quorum_2_of_4_fails_exact_half_edge_case"
    "check_quorum_mismatched_voter_tcb_is_attest_fail_not_minority"
    "check_quorum_all_deny_not_met"
    "vote_request_json_round_trip"
    "vote_response_json_round_trip"
    "coalition_ceil_n_over_2_minus_1_cannot_meet_quorum"
    "collect_responses_malformed_file_is_io_error_not_panic"
)

for name in "${REQUIRED_NAMES[@]}"; do
    if grep -rq "$name" "$QUORUM_DIR" 2>/dev/null; then
        pass "found: $name"
    else
        fail "MISSING test name: $name (check $QUORUM_DIR)"
    fi
done

# ── 2. Anti-stub check ────────────────────────────────────────────────────────
echo ""
echo "2. Anti-stub check"

STUB_VIOLATIONS=$(grep -rn 'todo!\|unimplemented!\|assert!(true)' "$QUORUM_DIR" 2>/dev/null || true)
if [ -z "$STUB_VIOLATIONS" ]; then
    pass "no stub patterns (todo!/unimplemented!/assert!(true)) found in R33 quorum sources"
else
    fail "stub patterns found — tests must not be stubbed:
$STUB_VIOLATIONS"
fi

# ── 3. Anti-vacuous attestation check ────────────────────────────────────────
echo ""
echo "3. Anti-vacuous attestation-mismatch check"

if grep -q '!reason.contains("insufficient approvals")' "$QUORUM_DIR/mod.rs" 2>/dev/null; then
    pass "check_quorum_mismatched_voter_tcb test asserts the failure is NOT collapsed into 'insufficient approvals'"
else
    fail "the attestation-mismatch test does not assert distinctness from the minority failure mode"
fi

# ── 4. Unit test suite ────────────────────────────────────────────────────────
echo ""
echo "4. Unit test suite (cargo test -p axon-vm quorum)"

if (cd "$REPO_ROOT" && cargo test -p axon-vm quorum --quiet 2>&1); then
    pass "all quorum unit tests pass"
else
    fail "quorum unit tests FAILED"
fi

# ── 5. Build the real CLI binary ─────────────────────────────────────────────
echo ""
echo "5. Build axon-vm binary"

if (cd "$REPO_ROOT" && cargo build -p axon-vm --quiet 2>&1); then
    pass "axon-vm binary builds"
else
    fail "axon-vm binary FAILED to build"
fi

BIN="$REPO_ROOT/target/debug/axon-vm"

# ── 6. End-to-end CLI journey: propose → 2 votes (deny+approve) → BLOCKED  ───
#       (n=3) → third approve vote → MET (n=3, exit 0)                     ──
echo ""
echo "6. End-to-end CLI journey (propose → vote → check)"

WORK_DIR="$(mktemp -d /tmp/r33-acceptance-XXXXXX)"
trap 'rm -rf "$WORK_DIR"' EXIT

export AXON_CI_NO_KVM=1
PROG="$REPO_ROOT/examples/hello.ax"

set +e
"$BIN" quorum propose \
    --run-id "r33-demo-1" \
    --prog "$PROG" \
    --action "wire_transfer(amount=1000000)" \
    --out "$WORK_DIR/request.json" \
    2>"$WORK_DIR/propose.stderr"
PROPOSE_RC=$?
set -e

if [ "$PROPOSE_RC" -eq 0 ] && [ -f "$WORK_DIR/request.json" ]; then
    pass "quorum propose wrote a VoteRequest"
else
    fail "quorum propose FAILED (exit $PROPOSE_RC)"
    cat "$WORK_DIR/propose.stderr" || true
fi

"$BIN" quorum vote --request "$WORK_DIR/request.json" --deny \
    --reason "policy score below threshold" --out "$WORK_DIR/voter1.vote" \
    2>"$WORK_DIR/vote1.stderr"
"$BIN" quorum vote --request "$WORK_DIR/request.json" --approve \
    --reason "policy score above threshold" --out "$WORK_DIR/voter2.vote" \
    2>"$WORK_DIR/vote2.stderr"

if [ -f "$WORK_DIR/voter1.vote" ] && [ -f "$WORK_DIR/voter2.vote" ]; then
    pass "quorum vote wrote 2 VoteResponse files (1 deny, 1 approve)"
else
    fail "quorum vote did not write both response files"
fi

set +e
CHECK1_OUT=$("$BIN" quorum check --responses-dir "$WORK_DIR" --n 3 2>&1)
CHECK1_RC=$?
set -e

if [ "$CHECK1_RC" -eq 13 ]; then
    pass "quorum check with 1/3 approvals exits 13 (QUORUM_BLOCKED)"
else
    fail "quorum check with 1/3 approvals exited $CHECK1_RC, expected 13"
    echo "$CHECK1_OUT"
fi

if echo "$CHECK1_OUT" | grep -qi "QUORUM BLOCKED"; then
    pass "output names 'QUORUM BLOCKED'"
else
    fail "output does not contain 'QUORUM BLOCKED': $CHECK1_OUT"
fi

"$BIN" quorum vote --request "$WORK_DIR/request.json" --approve \
    --reason "policy score above threshold" --out "$WORK_DIR/voter3.vote" \
    2>"$WORK_DIR/vote3.stderr"

set +e
CHECK2_OUT=$("$BIN" quorum check --responses-dir "$WORK_DIR" --n 3 2>&1)
CHECK2_RC=$?
set -e

if [ "$CHECK2_RC" -eq 0 ]; then
    pass "quorum check with 2/3 approvals exits 0 (QUORUM MET)"
else
    fail "quorum check with 2/3 approvals exited $CHECK2_RC, expected 0"
    echo "$CHECK2_OUT"
fi

if echo "$CHECK2_OUT" | grep -qi "QUORUM MET"; then
    pass "output names 'QUORUM MET'"
else
    fail "output does not contain 'QUORUM MET': $CHECK2_OUT"
fi

# ── 7. Exit-code distinctness: attestation mismatch → exit 14, not 13 ────────
echo ""
echo "7. Exit-code distinctness (attestation mismatch → 14, not 13)"

MISMATCH_DIR="$(mktemp -d /tmp/r33-mismatch-XXXXXX)"
trap 'rm -rf "$WORK_DIR" "$MISMATCH_DIR"' EXIT

cat > "$MISMATCH_DIR/v1.vote" <<'EOF'
{"voter_tcb":"axtcb1-ext:aaaa","run_id":"r33-mismatch","approved":true,"reason":"ok"}
EOF
cat > "$MISMATCH_DIR/v2.vote" <<'EOF'
{"voter_tcb":"axtcb1-ext:bbbb","run_id":"r33-mismatch","approved":true,"reason":"ok"}
EOF
cat > "$MISMATCH_DIR/v3.vote" <<'EOF'
{"voter_tcb":"axtcb1-ext:aaaa","run_id":"r33-mismatch","approved":true,"reason":"ok"}
EOF

set +e
MISMATCH_OUT=$("$BIN" quorum check --responses-dir "$MISMATCH_DIR" --n 3 2>&1)
MISMATCH_RC=$?
set -e

if [ "$MISMATCH_RC" -eq 14 ]; then
    pass "mismatched voter_tcb across votes exits 14 (QUORUM_ATTEST_FAIL), distinct from 13"
else
    fail "mismatched voter_tcb exited $MISMATCH_RC, expected 14"
    echo "$MISMATCH_OUT"
fi

# ── 8. R26/R31 regression check ──────────────────────────────────────────────
echo ""
echo "8. R26/R31 regression check"

if (cd "$REPO_ROOT" && cargo test -p axon-attest --quiet 2>&1); then
    pass "R26/R31 tests (axon-attest) still pass — R33 did not regress them"
else
    fail "axon-attest tests FAILED — R33 must not regress R26/R31"
fi

# ── Final result ──────────────────────────────────────────────────────────────
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "=== R33 acceptance gate: ALL CHECKS PASSED ==="
    exit 0
else
    echo "=== R33 acceptance gate: FAILED (see above) ==="
    exit 1
fi
