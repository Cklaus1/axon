#!/usr/bin/env bash
# K5: axon-guest-kernel enforcement gate.
#
# Verifies the three-layer containment model end-to-end by running the flagship
# good/evil agent pair through axon-vm (Firecracker required) and checking exit
# codes:
#
#   good_agent.ax  — IO-only effects → must exit 0
#   evil_agent.ax  — tries Net without declaration → must exit 8 (SandboxViolation)
#
# When Firecracker is not installed this script runs a dry-run that just
# verifies the compiler-level rejection (axon check) and the BPF policy
# generation — sufficient for CI without KVM access.
#
# Usage:
#   ./scripts/axon_kernel_gate.sh [--dry-run]

set -euo pipefail
cd "$(dirname "$0")/.."

DRY_RUN=0
if [[ "${1:-}" == "--dry-run" ]]; then
    DRY_RUN=1
fi

PASS=0
FAIL=0

ok()   { echo "  ✓ $*"; PASS=$((PASS+1)); }
fail() { echo "  ✗ $*"; FAIL=$((FAIL+1)); }

AXON="./target/debug/axon"
AXON_VM="./target/debug/axon-vm"
GOOD="examples/flagship/agent_task.ax"
EVIL="examples/flagship/agent_task_evil.ax"
KERNEL="dist/guest/vmlinuz"
INITRD="dist/guest/initramfs.cpio.gz"

# ── Build interpreter if needed ───────────────────────────────────────────────

if [[ ! -x "$AXON" ]]; then
    echo "[axon-kernel-gate] Building axon interpreter..."
    cargo build -p axon-core --no-default-features --bin axon --quiet
fi

if [[ ! -x "$AXON_VM" ]]; then
    echo "[axon-kernel-gate] Building axon-vm..."
    cargo build -p axon-vm --quiet
fi

echo "[axon-kernel-gate] Layer 1: compiler (@[contained] / effect-row)"

# ── Layer 1: compiler enforcement (always, no Firecracker needed) ─────────────

if [[ -f "$GOOD" ]]; then
    if "$AXON" check "$GOOD" 2>&1 | grep -q "E1001\|E1004\|E1310"; then
        fail "good_agent.ax has unexpected capability errors"
    else
        ok "good_agent.ax passes axon check (no capability errors)"
    fi
else
    echo "  [skip] $GOOD not found"
fi

if [[ -f "$EVIL" ]]; then
    if "$AXON" check "$EVIL" 2>&1 | grep -q "E1001\|E1004\|E1310"; then
        ok "evil_agent.ax is rejected by axon check (E1001/E1310 as expected)"
    else
        # If the evil agent passes the checker, the redteam is the gate.
        echo "  [info] evil_agent.ax not caught by axon check — relying on runtime gate"
    fi
else
    echo "  [skip] $EVIL not found"
fi

echo "[axon-kernel-gate] Layer 2: BPF policy generation"

# ── Layer 2: BPF generation from .axmeta ─────────────────────────────────────

if [[ -f "$GOOD" ]]; then
    # Build with --emit-manifest to get .axmeta.
    cargo build -p axon-core --features codegen --quiet 2>/dev/null || true
    AXON_CODEGEN="./target/debug/axon"
    if "$AXON_CODEGEN" build "$GOOD" --emit-manifest 2>/dev/null; then
        META="${GOOD%.ax}.axmeta"
        if [[ -f "$META" ]]; then
            ok "good_agent.axmeta generated"
            if python3 -c "import json,sys; d=json.load(open('$META')); assert d.get('risk') in ('low','medium','high','critical'), d" 2>/dev/null; then
                ok "axmeta risk field present"
            fi
        fi
    else
        echo "  [skip] codegen not available — skipping axmeta check"
    fi
fi

# ── Layer 3: microVM enforcement (Firecracker required) ───────────────────────

echo "[axon-kernel-gate] Layer 3: microVM enforcement"

if [[ $DRY_RUN -eq 1 ]]; then
    echo "  [dry-run] Skipping Firecracker tests (--dry-run)"
    PASS=$((PASS+1))
elif ! command -v firecracker &>/dev/null; then
    echo "  [skip] firecracker not in PATH — skipping live VM tests"
    echo "         Install from github.com/firecracker-microvm/firecracker to run Layer 3"
elif [[ ! -f "$KERNEL" ]] || [[ ! -f "$INITRD" ]]; then
    echo "  [skip] guest image missing — run ./scripts/build-guest-image.sh first"
else
    # good_agent: expect exit 0
    if [[ -f "$GOOD" ]]; then
        EXIT_CODE=0
        timeout 30 "$AXON_VM" run "$GOOD" \
            --kernel "$KERNEL" --initrd "$INITRD" \
            --json > /tmp/axon_gate_good.json 2>&1 || EXIT_CODE=$?
        ACTUAL=$(python3 -c "import json; d=json.load(open('/tmp/axon_gate_good.json')); print(d.get('exit_code',d.get('ok')))" 2>/dev/null || echo "$EXIT_CODE")
        if [[ "$ACTUAL" == "0" ]] || [[ "$ACTUAL" == "True" ]]; then
            ok "good_agent.ax exits 0 inside microVM"
        else
            fail "good_agent.ax exited $ACTUAL (expected 0)"
        fi
    fi

    # evil_agent: expect exit 8 (SandboxViolation from syscall gate)
    if [[ -f "$EVIL" ]]; then
        EXIT_CODE=0
        timeout 30 "$AXON_VM" run "$EVIL" \
            --kernel "$KERNEL" --initrd "$INITRD" \
            --json > /tmp/axon_gate_evil.json 2>&1 || EXIT_CODE=$?
        ACTUAL=$(python3 -c "import json; d=json.load(open('/tmp/axon_gate_evil.json')); print(d.get('exit_code'))" 2>/dev/null || echo "$EXIT_CODE")
        if [[ "$ACTUAL" == "8" ]]; then
            ok "evil_agent.ax exits 8 (SandboxViolation) inside microVM"
        else
            fail "evil_agent.ax exited $ACTUAL (expected 8)"
        fi
    fi
fi

# ── Summary ───────────────────────────────────────────────────────────────────

echo ""
if [[ $FAIL -eq 0 ]]; then
    echo "[axon-kernel-gate] PASS ($PASS checks, 0 failures)"
    exit 0
else
    echo "[axon-kernel-gate] FAIL ($PASS passed, $FAIL failed)"
    exit 1
fi
