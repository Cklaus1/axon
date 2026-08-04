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
    "coalition_bound_limits_same_lineage"
    "coalition_cap_does_not_block_distinct_lineage_roots"
    "coalition_cap_at_even_n_sockpuppet_blocked"
    "coalition_cap_at_even_n_distinct_roots_meets_quorum"
    "legacy_votes_missing_lineage_root_share_one_capped_bucket"
    "frame_round_trips_arbitrary_bytes"
    "connect_and_round_trip_over_real_tcp_socket_returns_the_voters_response"
    "connect_and_round_trip_to_a_dead_port_is_an_io_error_not_a_panic"
    "broadcast_and_collect_gathers_every_responsive_peers_vote"
    "broadcast_and_collect_drops_unreachable_peers_without_blocking_the_others"
    "broadcast_and_collect_wall_clock_does_not_scale_with_peer_count"
    "respond_once_on_a_peer_that_sends_the_eof_sentinel_instead_of_a_request_is_an_io_error"
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

# ── 8. R27 coalition ceiling: real CLI sock-puppet journey ───────────────────
echo ""
echo "8. R27 coalition ceiling: real CLI sock-puppet journey (n=3)"

SOCKPUPPET_DIR="$(mktemp -d /tmp/r33-sockpuppet-XXXXXX)"
trap 'rm -rf "$WORK_DIR" "$MISMATCH_DIR" "$SOCKPUPPET_DIR"' EXIT

"$BIN" quorum propose \
    --run-id "r33-sockpuppet" \
    --prog "$PROG" \
    --action "wire_transfer(amount=1000000)" \
    --out "$SOCKPUPPET_DIR/request.json" \
    >/dev/null 2>&1

# 3 YES votes, ALL declaring the SAME --lineage-root: a coalition of 3
# instances minted from one principal, all voting YES. Without the R27 cap
# this would be a trivial 3/3 majority; WITH it (default cap for n=3 is
# ceil(3/2)-1=1), only 1 vote is admitted — quorum must be BLOCKED.
for i in 1 2 3; do
    "$BIN" quorum vote --request "$SOCKPUPPET_DIR/request.json" --approve \
        --reason "sock puppet $i" --lineage-root "sockpuppet-principal" \
        --out "$SOCKPUPPET_DIR/voter$i.vote" >/dev/null 2>&1
done

set +e
SOCKPUPPET_OUT=$("$BIN" quorum check --responses-dir "$SOCKPUPPET_DIR" --n 3 2>&1)
SOCKPUPPET_RC=$?
set -e

if [ "$SOCKPUPPET_RC" -eq 13 ]; then
    pass "3 YES votes from ONE lineage root exit 13 (QUORUM_BLOCKED), not 0"
else
    fail "sock-puppet coalition exited $SOCKPUPPET_RC, expected 13: $SOCKPUPPET_OUT"
fi

if echo "$SOCKPUPPET_OUT" | grep -qi "coalition"; then
    pass "output names the coalition cap as the blocking cause, not a generic minority"
else
    fail "output does not name 'coalition' as the cause: $SOCKPUPPET_OUT"
fi

# Control: the SAME 3 approvals from 3 DISTINCT lineage roots must meet
# quorum normally — the cap must not over-trigger on legitimate diversity.
DISTINCT_DIR="$(mktemp -d /tmp/r33-distinct-XXXXXX)"
trap 'rm -rf "$WORK_DIR" "$MISMATCH_DIR" "$SOCKPUPPET_DIR" "$DISTINCT_DIR"' EXIT

"$BIN" quorum propose \
    --run-id "r33-distinct" \
    --prog "$PROG" \
    --action "wire_transfer(amount=1000000)" \
    --out "$DISTINCT_DIR/request.json" \
    >/dev/null 2>&1

for i in 1 2 3; do
    "$BIN" quorum vote --request "$DISTINCT_DIR/request.json" --approve \
        --reason "distinct voter $i" --lineage-root "principal-$i" \
        --out "$DISTINCT_DIR/voter$i.vote" >/dev/null 2>&1
done

set +e
DISTINCT_OUT=$("$BIN" quorum check --responses-dir "$DISTINCT_DIR" --n 3 2>&1)
DISTINCT_RC=$?
set -e

if [ "$DISTINCT_RC" -eq 0 ]; then
    pass "3 YES votes from 3 DISTINCT lineage roots exit 0 (QUORUM MET) — cap does not over-trigger"
else
    fail "3 distinct-root approvals exited $DISTINCT_RC, expected 0: $DISTINCT_OUT"
fi

# ── 9. R26/R31 regression check ──────────────────────────────────────────────
echo ""
echo "9. R26/R31 regression check"

if (cd "$REPO_ROOT" && cargo test -p axon-attest --quiet 2>&1); then
    pass "R26/R31 tests (axon-attest) still pass — R33 did not regress them"
else
    fail "axon-attest tests FAILED — R33 must not regress R26/R31"
fi

# ── 10. R33 S4: `axon deploy --quorum-dir` real cross-binary journey ─────────
echo ""
echo "10. R33 S4: axon deploy --quorum-dir cross-binary journey"

AXON_BIN="$REPO_ROOT/target/debug/axon"
if (cd "$REPO_ROOT" && cargo build -p axon-core --no-default-features --bin axon --quiet 2>&1); then
    pass "axon binary builds"
else
    fail "axon binary FAILED to build"
fi

HELLO_AX="$REPO_ROOT/examples/hello.ax"

# These checks exercise the R33 QUORUM gate, not the Phase-11 pipeline-gate
# policy. hello.ax defines no simulate/stress/redteam_check/assert_deployable,
# and since T33 (P7-SEC-07) a missing pipeline gate BLOCKS at Risk >= High —
# correctly, but it would mask what this section is actually measuring.
# --allow-missing-gates keeps the pipeline-gate question out of the way and
# leaves the quorum outcome as the only variable.
DEPLOY_FLAGS=(--allow-missing-gates)

# 10a. Backward compat: no --quorum-dir → gate is open, no "quorum" field.
NOQUORUM_OUT=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk high --json 2>/dev/null)
if echo "$NOQUORUM_OUT" | grep -q '"status":"deployed"' && ! echo "$NOQUORUM_OUT" | grep -q '"quorum"'; then
    pass "no --quorum-dir → deploy succeeds, no quorum field (100% backward compatible)"
else
    fail "no --quorum-dir case regressed: $NOQUORUM_OUT"
fi

# 10b. Sock-puppet: 3 YES votes, ONE shared --lineage-root → deploy BLOCKED.
DEPLOY_SP_DIR="$(mktemp -d /tmp/r33-deploy-sockpuppet-XXXXXX)"
trap 'rm -rf "$WORK_DIR" "$MISMATCH_DIR" "$SOCKPUPPET_DIR" "$DISTINCT_DIR" "$DEPLOY_SP_DIR" "$DEPLOY_OK_DIR"' EXIT

"$BIN" quorum propose --run-id "deploy-sockpuppet" --prog "$HELLO_AX" \
    --action "deploy hello.ax" --out "$DEPLOY_SP_DIR/request.json" >/dev/null 2>&1
for i in 1 2 3; do
    "$BIN" quorum vote --request "$DEPLOY_SP_DIR/request.json" --approve \
        --reason "sock puppet $i" --lineage-root "sockpuppet-principal" \
        --out "$DEPLOY_SP_DIR/voter$i.vote" >/dev/null 2>&1
done

set +e
DEPLOY_SP_OUT=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk high --quorum-dir "$DEPLOY_SP_DIR" --quorum-n 3 --json 2>/dev/null)
DEPLOY_SP_RC=$?
set -e

if [ "$DEPLOY_SP_RC" -eq 1 ] && echo "$DEPLOY_SP_OUT" | grep -q '"gate":"quorum"'; then
    pass "sock-puppet coalition → axon deploy BLOCKED at gate 'quorum', exit 1"
else
    fail "sock-puppet deploy → exit $DEPLOY_SP_RC, expected 1: $DEPLOY_SP_OUT"
fi
if echo "$DEPLOY_SP_OUT" | grep -q '"exit_code":13'; then
    pass "JSON surfaces axon-vm's real exit_code 13 (QUORUM_BLOCKED) for detail"
else
    fail "JSON does not surface exit_code 13: $DEPLOY_SP_OUT"
fi

# 10c. Legitimate quorum: 3 YES votes, 3 DISTINCT roots → deploy succeeds.
DEPLOY_OK_DIR="$(mktemp -d /tmp/r33-deploy-legit-XXXXXX)"
"$BIN" quorum propose --run-id "deploy-legit" --prog "$HELLO_AX" \
    --action "deploy hello.ax" --out "$DEPLOY_OK_DIR/request.json" >/dev/null 2>&1
for i in 1 2 3; do
    "$BIN" quorum vote --request "$DEPLOY_OK_DIR/request.json" --approve \
        --reason "voter $i" --lineage-root "principal-$i" \
        --out "$DEPLOY_OK_DIR/voter$i.vote" >/dev/null 2>&1
done

set +e
DEPLOY_OK_OUT=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk high --quorum-dir "$DEPLOY_OK_DIR" --quorum-n 3 --json 2>/dev/null)
DEPLOY_OK_RC=$?
set -e

if [ "$DEPLOY_OK_RC" -eq 0 ] && echo "$DEPLOY_OK_OUT" | grep -q '"status":"deployed"' && echo "$DEPLOY_OK_OUT" | grep -q '"quorum_met":true'; then
    pass "3 DISTINCT-root approvals → axon deploy succeeds with quorum_met:true"
else
    fail "legitimate-quorum deploy → exit $DEPLOY_OK_RC: $DEPLOY_OK_OUT"
fi

# 10d. Missing axon-vm binary + explicit --quorum-dir → hard error (exit 2),
#      never a silently-open gate (I-9).
set +e
MISSING_BIN_OUT=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk high --quorum-dir "$DEPLOY_OK_DIR" \
    --axon-vm-bin /nonexistent/axon-vm --json 2>&1)
MISSING_BIN_RC=$?
set -e

if [ "$MISSING_BIN_RC" -eq 2 ] && echo "$MISSING_BIN_OUT" | grep -qi "axon-vm binary not found"; then
    pass "missing --axon-vm-bin → hard error exit 2, never a silent open gate"
else
    fail "missing axon-vm binary case → exit $MISSING_BIN_RC, expected 2: $MISSING_BIN_OUT"
fi

# 10e. Risk >= Critical with NO --quorum-dir at all: the gate stays open (by
#      design — same convention as every other Phase-11 gate) but a decision
#      audit flagged this as the highest-risk choice in the whole R33.S4
#      integration (a silent false-sense-of-security failure mode), so a
#      visible stderr warning is now required, not just a silent skip.
CRITICAL_NO_QUORUM_ERR=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk critical --json 2>&1 >/dev/null)
CRITICAL_NO_QUORUM_OUT=$("$AXON_BIN" deploy "${DEPLOY_FLAGS[@]}" "$HELLO_AX" --risk critical --json 2>/dev/null)
if echo "$CRITICAL_NO_QUORUM_ERR" | grep -qi "quorum gate is NOT enforced"; then
    pass "Risk critical + no --quorum-dir → visible warning that the quorum gate is unenforced"
else
    fail "Risk critical + no --quorum-dir did not warn: '$CRITICAL_NO_QUORUM_ERR'"
fi
if echo "$CRITICAL_NO_QUORUM_OUT" | grep -q '"status":"deployed"'; then
    pass "...and still deploys (the gate stays open by design — this is visibility, not a behavior change)"
else
    fail "Risk critical + no --quorum-dir unexpectedly changed deploy behavior: $CRITICAL_NO_QUORUM_OUT"
fi

echo ""
echo "11. R33.S2d: real 'quorum vote --listen' CLI journey (TCP-loopback wire protocol)"

if command -v python3 >/dev/null 2>&1; then
    LISTEN_PORT=$((20000 + RANDOM % 10000))
    LISTEN_LOG="$(mktemp /tmp/r33-listen-XXXXXX.log)"
    AXON_CI_NO_KVM=1 "$BIN" quorum vote --listen "$LISTEN_PORT" --approve \
        --reason "gate journey" --lineage-root "gate-voter" >"$LISTEN_LOG" 2>&1 &
    LISTEN_PID=$!
    sleep 0.3 # let the listener bind before the client connects

    CLIENT_OUT=$(python3 - "$LISTEN_PORT" <<'PYEOF'
import socket, struct, json, sys
port = int(sys.argv[1])
req = {"run_id": "gate-run", "prog_hash": "sha256:gate", "voter_tcb": "axtcb1-ext:proposer",
       "proposed_action": "deploy", "timestamp_ms": 1}
payload = json.dumps(req).encode("utf-8")
s = socket.create_connection(("127.0.0.1", port), timeout=5)
s.sendall(struct.pack("<I", len(payload)) + payload)
lbuf = s.recv(4)
(length,) = struct.unpack("<I", lbuf)
resp = b""
while len(resp) < length:
    resp += s.recv(length - len(resp))
print(json.dumps(json.loads(resp)))
PYEOF
)
    set +e
    wait "$LISTEN_PID" 2>/dev/null
    LISTEN_RC=$?
    set -e

    if [ "$LISTEN_RC" -eq 0 ] \
        && echo "$CLIENT_OUT" | grep -q '"approved": true' \
        && echo "$CLIENT_OUT" | grep -q '"lineage_root": "gate-voter"' \
        && echo "$CLIENT_OUT" | grep -q '"run_id": "gate-run"'; then
        pass "propose (python client) -> vote --listen -> real VoteResponse over TCP loopback"
    else
        fail "--listen CLI journey: rc=$LISTEN_RC client_out=$CLIENT_OUT listen_log=$(cat "$LISTEN_LOG")"
    fi
    rm -f "$LISTEN_LOG"

    # --listen conflicts with --request/--out (clap-level, not a runtime check)
    set +e
    CONFLICT_OUT=$("$BIN" quorum vote --request /nonexistent --listen $((LISTEN_PORT + 1)) --approve 2>&1)
    CONFLICT_RC=$?
    set -e
    if [ "$CONFLICT_RC" -ne 0 ] && echo "$CONFLICT_OUT" | grep -qi "cannot be used with"; then
        pass "--listen conflicts with --request/--out (clap-enforced, not a silent override)"
    else
        fail "--listen + --request should conflict: rc=$CONFLICT_RC out=$CONFLICT_OUT"
    fi

    # backward compat: omitting --listen still requires --request/--out (clap-enforced)
    set +e
    BACKCOMPAT_OUT=$("$BIN" quorum vote --approve 2>&1)
    BACKCOMPAT_RC=$?
    set -e
    if [ "$BACKCOMPAT_RC" -ne 0 ] && echo "$BACKCOMPAT_OUT" | grep -qi "required arguments were not provided"; then
        pass "omitting --listen still requires --request/--out (100% backward compatible)"
    else
        fail "no --listen backward-compat check: rc=$BACKCOMPAT_RC out=$BACKCOMPAT_OUT"
    fi
else
    echo "  (skipped: python3 not available)"
fi

echo ""
echo "12. R33.S2e: real 'quorum propose --broadcast' CLI journey (real vote --listen peers)"

BROADCAST_DIR="$(mktemp -d /tmp/r33-broadcast-XXXXXX)"
PORT_A=$((30000 + RANDOM % 5000))
PORT_B=$((PORT_A + 1))
PORT_DEAD=$((PORT_A + 2)) # never listened on — a real unreachable peer

AXON_CI_NO_KVM=1 "$BIN" quorum vote --listen "$PORT_A" --approve --reason "peer A" \
    --lineage-root peer-a >"$BROADCAST_DIR/voter_a.log" 2>&1 &
VOTER_A_PID=$!
AXON_CI_NO_KVM=1 "$BIN" quorum vote --listen "$PORT_B" --approve --reason "peer B" \
    --lineage-root peer-b >"$BROADCAST_DIR/voter_b.log" 2>&1 &
VOTER_B_PID=$!
sleep 0.3 # let both listeners bind before the broadcast connects

set +e
MET_OUT=$(AXON_CI_NO_KVM=1 "$BIN" quorum propose --run-id "gate-broadcast" --prog "$HELLO_AX" \
    --action "deploy" --out "$BROADCAST_DIR/req.json" \
    --broadcast "127.0.0.1:$PORT_A,127.0.0.1:$PORT_B,127.0.0.1:$PORT_DEAD" \
    --n 3 --deadline-ms 3000 --json 2>&1)
MET_RC=$?
wait "$VOTER_A_PID" "$VOTER_B_PID" 2>/dev/null
set -e

if [ "$MET_RC" -eq 0 ] \
    && echo "$MET_OUT" | grep -q '"approvals": 2' \
    && echo "$MET_OUT" | grep -q '"quorum_met": true'; then
    pass "propose --broadcast against 2 real vote --listen peers + 1 unreachable -> QUORUM MET (2/3), exit 0"
else
    fail "propose --broadcast quorum-met journey: rc=$MET_RC out=$MET_OUT"
fi

# Insufficient approvals: both peers unreachable -> QUORUM BLOCKED, exit 13, never a hang.
set +e
BLOCKED_OUT=$(AXON_CI_NO_KVM=1 "$BIN" quorum propose --run-id "gate-broadcast-blocked" --prog "$HELLO_AX" \
    --action "deploy" --out "$BROADCAST_DIR/req2.json" \
    --broadcast "127.0.0.1:$((PORT_DEAD + 1)),127.0.0.1:$((PORT_DEAD + 2))" \
    --n 2 --deadline-ms 500 --json 2>&1)
BLOCKED_RC=$?
set -e
if [ "$BLOCKED_RC" -eq 13 ] && echo "$BLOCKED_OUT" | grep -q '"quorum_met": false'; then
    pass "propose --broadcast against 2 unreachable peers -> QUORUM BLOCKED, exit 13, no hang"
else
    fail "propose --broadcast blocked journey: rc=$BLOCKED_RC (want 13) out=$BLOCKED_OUT"
fi

# Backward compat: omitting --broadcast is unaffected (always exit 0, no quorum check runs).
set +e
NOBCAST_OUT=$(AXON_CI_NO_KVM=1 "$BIN" quorum propose --run-id "gate-nobcast" --prog "$HELLO_AX" \
    --action "deploy" --out "$BROADCAST_DIR/req3.json" 2>&1)
NOBCAST_RC=$?
set -e
if [ "$NOBCAST_RC" -eq 0 ] && ! echo "$NOBCAST_OUT" | grep -q 'broadcasting to\|"quorum_met"'; then
    pass "omitting --broadcast is unaffected (writes the file, exits 0, no quorum check runs)"
else
    fail "no --broadcast backward-compat check: rc=$NOBCAST_RC out=$NOBCAST_OUT"
fi

rm -rf "$BROADCAST_DIR"

# ── Final result ──────────────────────────────────────────────────────────────
echo ""
if [ "$FAIL" -eq 0 ]; then
    echo "=== R33 acceptance gate: ALL CHECKS PASSED ==="
    exit 0
else
    echo "=== R33 acceptance gate: FAILED (see above) ==="
    exit 1
fi
