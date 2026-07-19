#!/usr/bin/env bash
# R30: Unified ASI Deployment Safety Gate
#
# Runs R26→R29 acceptance gates + the flagship end-to-end demo in dependency
# order as a single pass/fail pipeline.  One script is all CI/operators need.
#
# Usage:
#   ./scripts/axon_safety_gate.sh                   # full gate
#   SKIP_BUILD=1 ./scripts/axon_safety_gate.sh      # skip Stage 1 (pre-built)
#   JSON_OUT=/tmp/gate.json ./scripts/axon_safety_gate.sh  # write report
#   ./scripts/axon_safety_gate.sh --self-test        # validate JSON output
#   ./scripts/axon_safety_gate.sh --help
#
# Environment variables:
#   SKIP_BUILD        non-empty → skip Stage 1 build
#   SKIP_UNIT_TESTS   non-empty → skip Stage 2 unit tests (for structural testing)
#   SKIP_R26          non-empty → skip Stage 3 R26 attestation (structural testing only)
#   SKIP_R27          non-empty → skip Stage 4 R27 corrigibility (structural testing only)
#   SKIP_R28          non-empty → skip Stage 5 R28 audit ledger (structural testing only)
#   SKIP_R29          non-empty → skip Stage 6 R29 compliance monitor (structural testing only)
#   AXON_CI_NO_KVM    forwarded to r26 / flagship (software-TPM stand-in)
#   AXON_AI_MOCK      forwarded to flagship demo (stub AI responses)
#   AXON_SEED         RNG seed forwarded to sub-scripts (default: 42)
#   JSON_OUT          path to write machine-readable JSON report
#
# Exit codes:
#   0   all stages passed (or skipped)
#   1   a stage failed; gate halts at the failing stage
#   2   usage error

set -euo pipefail

REPO="$(cd "$(dirname "$0")/.." && pwd)"

# ── Parse arguments ────────────────────────────────────────────────────────────

SELF_TEST=0
for arg in "$@"; do
    case "$arg" in
        --self-test) SELF_TEST=1 ;;
        --skip-build) SKIP_BUILD=1 ;;   # also accept as a flag (mirrors env var)
        --help|-h)
            sed -n '2,20p' "$0" | sed 's/^# //' | sed 's/^#//'
            exit 0
            ;;
        *) echo "axon_safety_gate: unknown flag: $arg" >&2; exit 2 ;;
    esac
done

# ── Self-test mode (acc_a3 + acc_a6) ──────────────────────────────────────────

if [[ "$SELF_TEST" -eq 1 ]]; then
    echo "=== axon_safety_gate --self-test ==="
    SELF_TEST_JSON="/tmp/axon_gate_selftest_$$.json"
    SELF_PASS=0; SELF_FAIL=0

    # acc_a3: JSON output is valid and has the required fields
    echo ""
    echo "acc_a3: JSON output valid..."
    JSON_OUT="$SELF_TEST_JSON" SKIP_BUILD=1 \
        bash "$0" --skip-build > /tmp/axon_gate_selftest_run_$$.log 2>&1 || true
    if python3 -c "
import json, sys
with open('$SELF_TEST_JSON') as f:
    d = json.load(f)
assert d.get('schema') == 'axon-safety-gate/1', 'schema mismatch'
assert 'ok' in d, 'missing ok'
assert 'stages' in d, 'missing stages'
assert 'timestamp' in d, 'missing timestamp'
print('  schema:', d['schema'])
print('  ok:', d['ok'])
print('  stages:', len(d['stages']))
" 2>&1; then
        echo "acc_a3 PASS"
        SELF_PASS=$((SELF_PASS+1))
    else
        echo "acc_a3 FAIL"
        SELF_FAIL=$((SELF_FAIL+1))
    fi

    # acc_a5: skipped stages appear with ok:true and skipped:true
    echo ""
    echo "acc_a5: skipped stages have ok:true and skipped:true..."
    if python3 -c "
import json
with open('$SELF_TEST_JSON') as f:
    d = json.load(f)
skipped = [s for s in d['stages'] if s.get('skipped')]
for s in skipped:
    assert s['ok'] == True, f'skipped stage {s[\"name\"]} has ok!=true'
    assert 'reason' in s, f'skipped stage {s[\"name\"]} missing reason'
print('  skipped stages:', [s['name'] for s in skipped])
" 2>&1; then
        echo "acc_a5 PASS"
        SELF_PASS=$((SELF_PASS+1))
    else
        echo "acc_a5 FAIL"
        SELF_FAIL=$((SELF_FAIL+1))
    fi

    # acc_a6: exit 0 on all-pass
    echo ""
    echo "acc_a6: exit code 0 on all-pass..."
    if JSON_OUT=/dev/null SKIP_BUILD=1 bash "$0" --skip-build > /dev/null 2>&1; then
        echo "acc_a6 PASS"
        SELF_PASS=$((SELF_PASS+1))
    else
        echo "acc_a6 FAIL (gate returned non-zero despite SKIP_BUILD)"
        SELF_FAIL=$((SELF_FAIL+1))
    fi

    rm -f "$SELF_TEST_JSON" "/tmp/axon_gate_selftest_run_$$.log"

    echo ""
    echo "SELF-TEST: $SELF_PASS passed, $SELF_FAIL failed"
    [[ "$SELF_FAIL" -eq 0 ]] && exit 0 || exit 1
fi

# ── Runtime state ──────────────────────────────────────────────────────────────

export AXON_SEED="${AXON_SEED:-42}"
export AXON_AI_MOCK="${AXON_AI_MOCK:-${AXON_AI_MOCK:-}}"
export AXON_CI_NO_KVM="${AXON_CI_NO_KVM:-}"

JSON_OUT="${JSON_OUT:-}"
SKIP_BUILD="${SKIP_BUILD:-}"
SKIP_UNIT_TESTS="${SKIP_UNIT_TESTS:-}"
SKIP_R26="${SKIP_R26:-}"
SKIP_R27="${SKIP_R27:-}"
SKIP_R28="${SKIP_R28:-}"
SKIP_R29="${SKIP_R29:-}"

RESULTS=()
OVERALL_OK=true
# PID-suffixed: unqualified paths here would let two concurrent invocations of this script (e.g.
# two Claude sessions on a shared host each running gate.sh --strict) interleave/clobber each
# other's per-stage diagnostic logs. Found while investigating R30's documented "flaky under host
# contention" acceptance-gate note (REQUIREMENTS.md) -- doesn't affect pass/fail (that's the
# underlying command's own exit code, not a re-read of this log), but a corrupted diagnostic log
# actively hinders debugging the exact flake this fix is a response to.
STAGE_LOG_PREFIX="/tmp/axon_gate_stage_$$"

# ── Helper functions ───────────────────────────────────────────────────────────

run_stage() {
    local stage_num="$1"
    local stage_name="$2"
    local cmd="$3"
    local log_file="${STAGE_LOG_PREFIX}_${stage_num}.log"

    echo ""
    echo "━━━ Stage $stage_num: $stage_name ━━━"
    if eval "$cmd" > "$log_file" 2>&1; then
        echo "  PASS"
        RESULTS+=("{\"stage\":$stage_num,\"name\":\"$stage_name\",\"ok\":true}")
    else
        local exit_code=$?
        echo "  FAIL (exit $exit_code)"
        tail -20 "$log_file"
        RESULTS+=("{\"stage\":$stage_num,\"name\":\"$stage_name\",\"ok\":false}")
        OVERALL_OK=false
        emit_report
        echo ""
        echo "✗ GATE FAILED at Stage $stage_num ($stage_name) — do not deploy"
        exit 1
    fi
}

skip_stage() {
    local stage_num="$1"
    local stage_name="$2"
    local reason="$3"

    echo ""
    echo "━━━ Stage $stage_num: $stage_name ━━━"
    echo "  SKIP: $reason"
    RESULTS+=("{\"stage\":$stage_num,\"name\":\"$stage_name\",\"ok\":true,\"skipped\":true,\"reason\":\"$reason\"}")
}

emit_report() {
    local timestamp
    timestamp="$(date -u +"%Y-%m-%dT%H:%M:%SZ" 2>/dev/null || echo "1970-01-01T00:00:00Z")"
    local ok_str
    $OVERALL_OK && ok_str="true" || ok_str="false"
    local stages_json
    stages_json="$(IFS=,; printf '%s' "${RESULTS[*]}")"
    local report="{\"schema\":\"axon-safety-gate/1\",\"timestamp\":\"$timestamp\",\"ok\":$ok_str,\"stages\":[$stages_json]}"

    if [[ -n "$JSON_OUT" ]] && [[ "$JSON_OUT" != "/dev/null" ]]; then
        printf '%s\n' "$report" > "$JSON_OUT"
    fi
    # Pretty-print if python3 is available; otherwise raw JSON
    if command -v python3 >/dev/null 2>&1; then
        printf '%s\n' "$report" | python3 -m json.tool 2>/dev/null || printf '%s\n' "$report"
    else
        printf '%s\n' "$report"
    fi
}

# ── Stage 1: Build ─────────────────────────────────────────────────────────────

if [[ -n "$SKIP_BUILD" ]]; then
    skip_stage 1 "BUILD" "--skip-build"
else
    # Build axon-core (interpreter, no codegen — sub-second on a warm cache)
    # then the safety crates (attest, os, vm) that are known to exist.
    run_stage 1 "BUILD" \
        "cargo build -p axon-core --no-default-features --bin axon \
         && cargo build -p axon-attest \
         && cargo build -p axon-os \
         && cargo build -p axon-vm"
fi

# ── Stage 2: Unit tests ────────────────────────────────────────────────────────
# Prerequisite: Cargo.toml must not have merge conflict markers — a committed
# conflict breaks the entire workspace.  Detect early so the failure is obvious.

if [[ -n "$SKIP_UNIT_TESTS" ]]; then
    skip_stage 2 "UNIT_TESTS" "--skip-unit-tests"
elif grep -q '^<<<<<<' "$REPO/Cargo.toml" 2>/dev/null; then
    # Committed merge conflict markers make every cargo command fail.
    # This is a repo-health failure, not a test failure — fail loudly.
    echo ""
    echo "━━━ Stage 2: UNIT_TESTS ━━━"
    echo "  FAIL: Cargo.toml has unresolved merge conflict markers."
    echo "        Resolve the conflict in Cargo.toml before running the gate."
    echo "        (Set SKIP_UNIT_TESTS=1 to bypass for structural testing only.)"
    RESULTS+=("{\"stage\":2,\"name\":\"UNIT_TESTS\",\"ok\":false,\"reason\":\"Cargo.toml merge conflict\"}")
    OVERALL_OK=false
    emit_report
    echo ""
    echo "✗ GATE FAILED at Stage 2 (UNIT_TESTS) — do not deploy"
    exit 1
else
    run_stage 2 "UNIT_TESTS" \
        "cargo test -p axon-core 2>&1 | tail -5"
fi

# ── Stage 3: R26 attestation ───────────────────────────────────────────────────

if [[ -n "$SKIP_R26" ]]; then
    skip_stage 3 "R26_ATTESTATION" "SKIP_R26 set"
elif [[ -f "$REPO/scripts/r26_acceptance_gate.sh" ]]; then
    run_stage 3 "R26_ATTESTATION" \
        "bash $REPO/scripts/r26_acceptance_gate.sh"
else
    skip_stage 3 "R26_ATTESTATION" "r26_acceptance_gate.sh not found"
fi

# ── Stage 4: R27 corrigibility ─────────────────────────────────────────────────

if [[ -n "$SKIP_R27" ]]; then
    skip_stage 4 "R27_CORRIGIBILITY" "SKIP_R27 set"
elif [[ -f "$REPO/scripts/r27_acceptance_gate.sh" ]]; then
    run_stage 4 "R27_CORRIGIBILITY" \
        "bash $REPO/scripts/r27_acceptance_gate.sh"
else
    skip_stage 4 "R27_CORRIGIBILITY" "r27_acceptance_gate.sh not found"
fi

# ── Stage 5: R28 audit ledger ─────────────────────────────────────────────────

if [[ -n "$SKIP_R28" ]]; then
    skip_stage 5 "R28_AUDIT_LEDGER" "SKIP_R28 set"
elif [[ -f "$REPO/scripts/r28_acceptance_gate.sh" ]]; then
    run_stage 5 "R28_AUDIT_LEDGER" \
        "bash $REPO/scripts/r28_acceptance_gate.sh"
else
    skip_stage 5 "R28_AUDIT_LEDGER" "r28_acceptance_gate.sh not found"
fi

# ── Stage 6: R29 compliance monitor ───────────────────────────────────────────

if [[ -n "$SKIP_R29" ]]; then
    skip_stage 6 "R29_COMPLIANCE" "SKIP_R29 set"
elif [[ -f "$REPO/scripts/r29_acceptance_gate.sh" ]]; then
    run_stage 6 "R29_COMPLIANCE" \
        "bash $REPO/scripts/r29_acceptance_gate.sh"
else
    skip_stage 6 "R29_COMPLIANCE" "r29_acceptance_gate.sh not found"
fi

# ── Stage 7: Flagship end-to-end demo ─────────────────────────────────────────
# Requires the axon binary from Stage 1 (or pre-built).  axon-os and axon-vm
# are optional; the demo script skips their layers when absent.

if [[ -f "$REPO/examples/flagship/demo.sh" ]]; then
    run_stage 7 "FLAGSHIP_DEMO" \
        "DEMO_NOPAUSE=1 bash $REPO/examples/flagship/demo.sh"
else
    skip_stage 7 "FLAGSHIP_DEMO" "examples/flagship/demo.sh not found"
fi

# ── Stage 8: Report ────────────────────────────────────────────────────────────

echo ""
echo "━━━ Stage 8: REPORT ━━━"
RESULTS+=("{\"stage\":8,\"name\":\"REPORT\",\"ok\":true}")
emit_report

if [[ -n "$JSON_OUT" ]] && [[ "$JSON_OUT" != "/dev/null" ]]; then
    echo "  Report written to $JSON_OUT"
fi

echo ""
if $OVERALL_OK; then
    echo "✓ ALL STAGES PASSED — safe to deploy"
    exit 0
else
    echo "✗ GATE FAILED — do not deploy"
    exit 1
fi
