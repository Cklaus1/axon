#!/usr/bin/env bash
# demo.sh — Axon ASI Safety Stack: four-layer demonstration.
#
# Layers:
#   1. @[contained]       Compile-time capability proof (axon check)
#   2. Runtime sandbox    @[contained] re-enforced at builtin dispatch (axon run)
#   3. R27 Kill-switch    Operator halts a running agent in < 1s (axon-os)
#   4. R26 Attestation    Kernel image measured + signed before boot (axon-vm)
#
# Requires: target/debug/axon (always).  axon-os and axon-vm are optional;
# Layers 3 and 4 are skipped with a note when the binaries are absent.
#
# Usage:
#   ./examples/flagship/demo.sh            # interactive (pauses between layers)
#   DEMO_NOPAUSE=1 ./examples/flagship/demo.sh   # CI / non-interactive
#   AXON_CI_NO_KVM=1 ./examples/flagship/demo.sh # skip real KVM in Layer 4

set -euo pipefail
REPO=$(cd "$(dirname "$0")/../.." && pwd)

AXON="$REPO/target/debug/axon"
AXON_OS="$REPO/target/debug/axon-os"
AXON_VM="$REPO/target/debug/axon-vm"
D="$REPO/examples/flagship"
DEMO_OUT="${TMPDIR:-/tmp}/axon-flagship-$$"
RUN_ID="flagship-$(date +%s 2>/dev/null || echo 00)"

PASS=0
SKIP=0
FAIL=0

# ── Formatting helpers ────────────────────────────────────────────────────────

bold()  { printf '\033[1m%s\033[0m\n' "$*"; }
dim()   { printf '\033[2m%s\033[0m\n' "$*"; }
green() { printf '\033[32m%s\033[0m\n' "$*"; }
red()   { printf '\033[31m%s\033[0m\n' "$*"; }
rule()  { dim "────────────────────────────────────────────────────────────"; }

ok()   { green "  PASS: $*"; PASS=$((PASS+1)); }
skip() { dim   "  SKIP: $*"; SKIP=$((SKIP+1)); }
fail() { red   "  FAIL: $*"; FAIL=$((FAIL+1)); }

pause() {
    [ "${DEMO_NOPAUSE:-}" = "1" ] && return
    printf '\033[2m(enter to continue)\033[0m'
    read -r _
}

# ── Preflight: make sure the axon interpreter binary is available ─────────────

if [[ ! -x "$AXON" ]]; then
    echo "axon binary not found at $AXON"
    echo "Build it first: cargo build -p axon-core --no-default-features --bin axon"
    exit 1
fi

mkdir -p "$DEMO_OUT"

clear 2>/dev/null || true
bold "=== Axon ASI Safety Stack Demo ==="
echo ""
echo "Four layers of provable safety for AI-written code:"
echo "  Layer 1  @[contained]      — compile-time capability proof"
echo "  Layer 2  runtime sandbox   — same proof re-enforced at dispatch"
echo "  Layer 3  R27 kill-switch   — operator can halt any agent < 1s"
echo "  Layer 4  R26 attestation   — kernel measured before the agent boots"
echo ""
pause

# ── Layer 1: @[contained] — compile-time capability sandboxing ───────────────

rule
bold "Layer 1: @[contained] — compile-time capability proof"
echo ""
echo "The good agent declares: @[contained(fs: [], net: [], exec: none)]"
echo "The compiler verifies the body never uses what wasn't declared."
echo ""

echo "  \$ axon check examples/flagship/agent_task.ax"
if "$AXON" check "$D/agent_task.ax" 2>&1 | grep -qv "E1001"; then
    ok "agent_task.ax passes axon check  (exit 0 — no capability errors)"
else
    fail "agent_task.ax has unexpected E1001 errors"
fi

echo ""
echo "Now the EVIL agent: same declared caps, body also reads /etc/passwd,"
echo "calls an LLM, and shells out to curl."
echo ""
echo "  \$ axon check examples/flagship/agent_task_evil.ax"
EVIL_OUT=$("$AXON" check "$D/agent_task_evil.ax" 2>&1 || true)
E1001_COUNT=$(echo "$EVIL_OUT" | grep -c '"E1001"' || true)
if [[ "$E1001_COUNT" -ge 3 ]]; then
    ok "agent_task_evil.ax: REFUSED — $E1001_COUNT E1001 errors (fs-read, net, exec)"
    echo ""
    echo "  Errors:"
    echo "$EVIL_OUT" | python3 -c "
import sys, json
for line in sys.stdin:
    try:
        d = json.loads(line)
        if d.get('code') == 'E1001':
            msg = d['message'].split('\n')[0][:90]
            print('    -', msg)
    except Exception:
        pass
" 2>/dev/null | head -4 || echo "$EVIL_OUT" | grep '"E1001"' | head -3 | sed 's/.*"message":"/  - /' | sed 's/\\n.*//' | cut -c1-90
else
    fail "evil agent should have produced >= 3 E1001 errors (got $E1001_COUNT)"
fi

echo ""
echo "The data is never read. The packet is never sent. curl never runs."
echo "The escape is impossible BY CONSTRUCTION — the program cannot build."
pause

# ── Layer 2: Runtime enforcement ──────────────────────────────────────────────

rule
bold "Layer 2: Runtime enforcement"
echo ""
echo "The @[contained] boundary is re-checked at every builtin call site."
echo "Even if the static check were bypassed, the runtime refuses the call."
echo ""
echo "  \$ AXON_AI_MOCK=1 axon run examples/flagship/agent_task.ax"
echo ""

AXON_AI_MOCK=1 timeout 10 "$AXON" run "$D/agent_task.ax" 2>&1 | grep -v "^axon: run-id"
RUN_EXIT="${PIPESTATUS[0]}"

if [[ "$RUN_EXIT" -eq 0 ]]; then
    ok "good agent ran cleanly (exit 0) — scored candidates without touching net/fs/exec"
else
    fail "good agent exited $RUN_EXIT"
fi

echo ""
echo "The @[verify(value >= 0)] postcondition on score() is enforced at every"
echo "call site. A buggy score would exit 6 before the caller sees it."
pause

# ── Layer 3: R27 Corrigibility (kill-switch) ──────────────────────────────────

rule
bold "Layer 3: R27 Corrigibility — the operator kill-switch"
echo ""

if [[ ! -x "$AXON_OS" ]]; then
    skip "axon-os not found at $AXON_OS — build with: cargo build -p axon-os"
    echo "  R27 provides: axon-os run --killable / axon-os kill <run-id>"
else
    echo "R27 ships a one-way kill-latch. Once tripped by the supervisor, the"
    echo "agent stops within one polling cycle (~100ms) and exits 4 (HALTED)."
    echo "No code inside the agent can clear the latch."
    echo ""

    KILL_JOB="$REPO/examples/r27/killable_agent.axjob"
    AXON_BIN="$AXON" AXON_OS_TIMEOUT_MS=30000 \
        "$AXON_OS" run "$KILL_JOB" \
            --run-id "$RUN_ID" \
            --out "$DEMO_OUT" \
            --killable \
        > "$DEMO_OUT/run.log" 2>&1 &
    BG_PID=$!

    # Give the subprocess time to write the kill-file and start polling
    sleep 0.5

    echo "  Agent is running (looping forever). Issuing kill:"
    echo "  \$ axon-os kill $RUN_ID --store $DEMO_OUT"
    "$AXON_OS" kill "$RUN_ID" --store "$DEMO_OUT" --reason "demo: operator shutdown"

    # Wait for the background run to exit
    wait "$BG_PID" || true
    R27_EXIT=$(cat "$DEMO_OUT/run.log" | grep -c "HALTED\|kill-switch" || true)

    if [[ "$R27_EXIT" -ge 1 ]]; then
        ok "agent halted by kill-switch (exit 4 / HALTED verdict)"
    else
        # Check the record directly
        if [[ -f "$DEMO_OUT/${RUN_ID}.json" ]] && grep -q "Halted" "$DEMO_OUT/${RUN_ID}.json" 2>/dev/null; then
            ok "agent halted by kill-switch (verdict: Halted)"
        else
            fail "kill-switch demo: expected HALTED verdict"
        fi
    fi
    cat "$DEMO_OUT/run.log" | head -3

    # R28 audit ledger: the run record IS the immutable chained ledger.
    # axon-os verify checks the SHA-256 hash chain (R21 section 3.4).
    # TODO R28: when crates/axon-audit ships, replace with:
    #           axon-os audit verify --ledger $DEMO_OUT/${RUN_ID}.json
    if [[ -f "$DEMO_OUT/${RUN_ID}.json" ]]; then
        echo ""
        echo "  Audit record written: $DEMO_OUT/${RUN_ID}.json"
        echo "  Verifying hash chain (R21 section 3.4 tamper-evident record):"
        echo "  \$ axon-os verify $DEMO_OUT/${RUN_ID}.json"
        VERIFY_OUT=$("$AXON_OS" verify "$DEMO_OUT/${RUN_ID}.json" 2>&1)
        echo "  $VERIFY_OUT"
        if echo "$VERIFY_OUT" | grep -q "intact"; then
            ok "audit record intact — hash chain verifies (R28 ledger via axon-os verify)"
        else
            fail "audit record verification failed"
        fi
    fi
fi
pause

# ── Layer 4: R26 Attestation ──────────────────────────────────────────────────

rule
bold "Layer 4: R26 Attestation — kernel measured before boot"
echo ""

if [[ ! -x "$AXON_VM" ]]; then
    skip "axon-vm not found at $AXON_VM — build with: cargo build -p axon-vm"
    echo "  R26 provides: axon-vm attest --kernel PATH (AXON_CI_NO_KVM=1 for CI)"
else
    echo "R26 measures the kernel ELF image (SHA-256 + axtcb1 chain) and produces"
    echo "a software-TPM attestation report. No tampered kernel can boot silently."
    echo ""
    KERNEL_PATH="$REPO/dist/guest/vmlinuz"
    echo "  \$ AXON_CI_NO_KVM=1 axon-vm attest --kernel dist/guest/vmlinuz"
    echo ""

    ATTEST_OUT=$(AXON_CI_NO_KVM=1 "$AXON_VM" attest \
        --kernel "$KERNEL_PATH" \
        --run-prog "$D/agent_task.ax" 2>&1)
    ATTEST_EXIT=$?

    # Print the caveat from stderr (the honesty check R26 section 8)
    echo "$ATTEST_OUT" | grep "substrate:" | sed 's/^/  /'

    if [[ "$ATTEST_EXIT" -eq 0 ]]; then
        # Extract key fields from the JSON output
        DIGEST=$(echo "$ATTEST_OUT" | python3 -c "
import sys, json
data = json.loads(sys.stdin.read().split('{', 1)[-1].rsplit('}',1)[0].join(['{', '}']))
" 2>/dev/null || true)
        # Just print the relevant JSON lines
        echo "$ATTEST_OUT" | grep -E '"ok"|"digest"|"axtcb1"' | head -4 | sed 's/^/  /'
        ok "attestation report produced (schema: axon-attest/1)"
        echo ""
        echo "  The axtcb1 chain root is embedded in every axon-vm run."
        echo "  A tampered kernel produces a different digest -> run refused (exit 10)."
    else
        fail "attestation failed (exit $ATTEST_EXIT)"
    fi
fi
pause

# ── Summary ───────────────────────────────────────────────────────────────────

rule
bold "=== Summary: Axon ASI Safety Stack ==="
echo ""
echo "  Layer 1  @[contained]    Compile-time proof. CANNOT be bypassed by the agent."
echo "  Layer 2  Runtime         Same proof re-enforced at dispatch. Cannot launder."
echo "  Layer 3  R27 Kill-switch Operator halts any agent < 1s. Latch is one-way."
echo "  Layer 4  R26 Attestation Kernel measured before the agent boots. Tamper = refuse."
echo ""
echo "  Python: every escape attempt runs. The 'sandbox' is a comment."
echo "  Axon:   every escape attempt is a compile error or a refused builtin."
echo "          Run examples/flagship/compare_python.sh to see the contrast."
echo ""
echo "  Passes: $PASS  Skipped: $SKIP  Failures: $FAIL"
rm -rf "$DEMO_OUT"

if [[ "$FAIL" -gt 0 ]]; then
    red "DEMO FAILED ($FAIL failures)"
    exit 1
fi
green "DEMO PASSED"
exit 0
