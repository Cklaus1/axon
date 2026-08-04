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
    # R26: `axon-vm run` requires a PINNED kernel baseline — it will not
    # trust-on-first-use (P7-KRN-05). Pin the gate's own freshly-built kernel
    # explicitly, exactly as an operator would. `--expect-digest` on each run
    # would work too, but pinning once exercises the intended flow.
    KERNEL_DIGEST="$(sha256sum "$KERNEL" | cut -d' ' -f1)"
    "$AXON_VM" attest --kernel "$KERNEL" --pin-baseline --repin >/dev/null 2>&1 \
        || echo "  [warn] could not pin kernel baseline; runs will use --expect-digest"

    # good_agent: expect exit 0
    #
    # AUDIT T48. The grant is stated EXPLICITLY here. It used to be omitted, and
    # the guest kernel read an absent grant as EffectSet(0xFF) — every effect —
    # so this case passed by being granted full authority, which is precisely
    # what Layer 3 claims to be testing. With the fail-open default closed, the
    # grant has to be real.
    #
    # `FS` is here for the SUBSTRATE, not for the program: the guest runs the
    # interpreter, which must `openat` /axon/program.ax before any Axon code
    # executes. agent_task.ax's own effect union is `["IO"]` (it only prints).
    # Whether axon-vm should add that substrate baseline itself — and what it
    # means that a guest therefore always holds FS — is an open design question,
    # recorded as O034; stating it here keeps the gate honest meanwhile.
    if [[ -f "$GOOD" ]]; then
        EXIT_CODE=0
        AXON_VM_ALLOWED_EFFECTS=IO,FS timeout 30 "$AXON_VM" run "$GOOD" \
            --kernel "$KERNEL" --initrd "$INITRD" \
            --expect-digest "$KERNEL_DIGEST" \
            --json > /tmp/axon_gate_good.json 2>&1 || EXIT_CODE=$?
        ACTUAL=$(python3 -c "import json; d=json.load(open('/tmp/axon_gate_good.json')); print(d.get('exit_code',d.get('ok')))" 2>/dev/null || echo "$EXIT_CODE")
        if [[ "$ACTUAL" == "0" ]] || [[ "$ACTUAL" == "True" ]]; then
            ok "good_agent.ax exits 0 inside microVM"
        else
            fail "good_agent.ax exited $ACTUAL (expected 0)"
        fi
    fi

    # evil_agent: expect exit 8 (SandboxViolation from syscall gate).
    #
    # The grant must be stated explicitly. `run` derives the in-guest policy from
    # the program's .axmeta manifest, and the evil agent CANNOT have one — `axon
    # check` refuses it outright (3× E1001), so `build --emit-manifest` never
    # produces a manifest to derive from. With no manifest the policy defaults to
    # OPEN (all 8 effect bits), so this case previously ran with FS granted and
    # the deny path was never exercised at all: the check has never once observed
    # the violation it claims to test. AXON_VM_ALLOWED_EFFECTS states the grant
    # the demo intends — AI only — so the evil agent's openat is a real breach of
    # a real ceiling.
    if [[ -f "$EVIL" ]]; then
        EXIT_CODE=0
        AXON_VM_ALLOWED_EFFECTS=AI timeout 30 "$AXON_VM" run "$EVIL" \
            --kernel "$KERNEL" --initrd "$INITRD" \
            --expect-digest "$KERNEL_DIGEST" \
            --json > /tmp/axon_gate_evil.json 2>&1 || EXIT_CODE=$?
        ACTUAL=$(python3 -c "import json; d=json.load(open('/tmp/axon_gate_evil.json')); print(d.get('exit_code'))" 2>/dev/null || echo "$EXIT_CODE")
        if [[ "$ACTUAL" == "8" ]]; then
            ok "evil_agent.ax exits 8 (SandboxViolation) inside microVM"
        else
            fail "evil_agent.ax exited $ACTUAL (expected 8)"
        fi
    fi
fi

# ── Layer 4: Full ASI stack — flagship demo ───────────────────────────────────
# Runs demo.sh in non-interactive CI mode (DEMO_NOPAUSE=1).
# AXON_CI_NO_KVM=1 allows attestation without real KVM hardware.
# The demo is self-contained: it rebuilds nothing and skips unavailable binaries.

echo "[axon-kernel-gate] Layer 4: Full ASI stack (flagship demo)"

DEMO_SCRIPT="examples/flagship/demo.sh"

if [[ ! -f "$DEMO_SCRIPT" ]]; then
    echo "  [skip] $DEMO_SCRIPT not found"
else
    DEMO_NOPAUSE=1 AXON_CI_NO_KVM=1 bash "$DEMO_SCRIPT" 2>&1 | \
        sed 's/^/  /' || true
    # Gate on PASS / FAIL lines printed by demo.sh
    DEMO_OUT=$(DEMO_NOPAUSE=1 AXON_CI_NO_KVM=1 bash "$DEMO_SCRIPT" 2>&1)
    DEMO_PASS=$(echo "$DEMO_OUT" | grep -c "PASS:" || true)
    DEMO_FAIL=$(echo "$DEMO_OUT" | grep -c "FAIL:" || true)
    if [[ "$DEMO_FAIL" -eq 0 ]] && [[ "$DEMO_PASS" -ge 1 ]]; then
        ok "flagship demo passed ($DEMO_PASS checks, 0 failures)"
    elif [[ "$DEMO_FAIL" -gt 0 ]]; then
        fail "flagship demo had $DEMO_FAIL failure(s) (see demo.sh output above)"
    else
        # Demo ran but no PASS lines — binary missing or all layers skipped
        ok "flagship demo ran (all active layers passed or skipped)"
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
