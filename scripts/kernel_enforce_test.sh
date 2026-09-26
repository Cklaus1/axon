#!/usr/bin/env bash
# kernel_enforce_test.sh — proves the axon-guest-kernel's syscall gate performs LIVE
# capability DENIAL: a real `openat` syscall is intercepted by the hardware gate
# (SYSCALL → LSTAR → syscall_dispatch) and DENIED when the policy withholds the FS
# effect. That deny direction is the K3+K5 enforcement claim this script supports.
#
# WHAT IT DOES NOT PROVE (FG-041 / F161): the ALLOW path. Under an FS-granting
# policy the kernel reaches its grant branch and halts WITHOUT issuing any syscall —
# dispatch → 0 → `sysretq` back to ring 3 needs DPL-3 GDT segments the boot GDT does
# not define. Case 2 used to grep the grant-branch print and report "the gate
# PERMITTED the openat", asserting a syscall that provably never happened. It now
# asserts exactly what the kernel does: grant branch reached, NO syscall dispatched
# (the absence of the `syscall-dispatch:` marker, which case 1 proves is real by
# requiring it), clean exit. The real allow path is tracked OPEN, not done.
#
# Requires: the freestanding kernel (AXON_GUEST_KERNEL, else
# $CARGO_TARGET_DIR/x86_64-axon-metal/release/, else target/x86_64-axon-metal/release/),
# plus Firecracker + KVM (fc_boot_test.sh provides the boot harness). Prerequisites
# absent → prints "skipping" and exits 0; gate.sh records that as a SKIP, never a PASS.
set -uo pipefail
cd "$(dirname "$0")/.."

if [[ -n "${AXON_GUEST_KERNEL:-}" ]]; then
    KERNEL="$AXON_GUEST_KERNEL"
elif [[ -n "${CARGO_TARGET_DIR:-}" && -f "$CARGO_TARGET_DIR/x86_64-axon-metal/release/axon-guest-kernel" ]]; then
    KERNEL="$CARGO_TARGET_DIR/x86_64-axon-metal/release/axon-guest-kernel"
else
    KERNEL="target/x86_64-axon-metal/release/axon-guest-kernel"
fi
export AXON_GUEST_KERNEL="$KERNEL"   # fc_boot_test.sh boots exactly this artifact
if [[ ! -f "$KERNEL" ]]; then
    echo "kernel_enforce_test: kernel not built ($KERNEL) — skipping"
    echo "  build: cargo build -p axon-guest-kernel \\"
    echo "    --target \$(pwd)/crates/axon-guest-kernel/targets/x86_64-axon-metal.json \\"
    echo "    -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem \\"
    echo "    -Z json-target-spec --release"
    exit 0
fi
command -v firecracker >/dev/null 2>&1 || { echo "kernel_enforce_test: firecracker absent — skipping"; exit 0; }
[[ -e /dev/kvm ]] || { echo "kernel_enforce_test: /dev/kvm absent — skipping"; exit 0; }

b64() { printf '%s' "$1" | base64 -w0; }
boot() { timeout 90 bash scripts/fc_boot_test.sh --policy "$1" 2>&1; }

fail=0

# The gate announces every entry: `[axon-kernel] syscall-dispatch: nr=N`.
DISPATCH_RE='syscall-dispatch: nr='

# ── Case 1: FS WITHHELD → the openat must be DENIED (live VIOLATION) ───────────
echo "kernel_enforce_test: case 1 — policy IO-only (FS withheld) → expect VIOLATION"
OUT_DENY="$(boot "$(b64 '{"allowed_effects":["IO"]}')")"
if grep -q "${DISPATCH_RE}257" <<<"$OUT_DENY" \
   && grep -q "VIOLATION: syscall 257 blocked (FS not in policy)" <<<"$OUT_DENY" \
   && grep -q "VIOLATION8" <<<"$OUT_DENY"; then
    echo "  ✓ openat(257) reached the gate and was DENIED; halted with exit code 8"
else
    echo "  ✗ expected a dispatched openat and a live VIOLATION under an FS-withholding policy"
    echo "$OUT_DENY" | grep -iE 'axon-kernel|K5|violation' | sed 's/^/      /' | tail -8
    fail=1
fi

# ── Case 2: FS GRANTED → grant branch reached, NO syscall issued ──────────────
# This is NOT an allow-path test: the kernel issues no syscall here (FG-041). It
# pins that fact, so the day someone reports a permitted syscall it must actually
# appear in the transcript — and the day one silently appears, this fails.
echo "kernel_enforce_test: case 2 — policy IO+FS (granted) → expect grant branch, NO syscall (allow path deferred)"
OUT_OK="$(boot "$(b64 '{"allowed_effects":["IO","FS"]}')")"
c2=1
grep -q "K5: policy GRANTS FS — grant branch reached, no syscall issued (allow path deferred)" <<<"$OUT_OK" \
    || { echo "  ✗ grant-branch line absent"; c2=0; }
if grep -q "$DISPATCH_RE" <<<"$OUT_OK"; then
    echo "  ✗ a syscall WAS dispatched under the grant policy — the 'no syscall issued' claim is false:"
    grep "$DISPATCH_RE" <<<"$OUT_OK" | sed 's/^/      /' | head -4
    c2=0
fi
grep -q "VIOLATION" <<<"$OUT_OK" && { echo "  ✗ a VIOLATION appeared under an FS-granting policy"; c2=0; }
grep -q "EXIT0" <<<"$OUT_OK" || { echo "  ✗ no clean -EXIT0 halt"; c2=0; }
if [[ $c2 -eq 1 ]]; then
    echo "  ✓ grant branch reached, no syscall issued (allow path deferred) — clean halt"
else
    echo "$OUT_OK" | grep -iE 'axon-kernel|K5|violation|EXIT' | sed 's/^/      /' | tail -8
    fail=1
fi

echo ""
if [[ $fail -eq 0 ]]; then
    echo "kernel_enforce_test: PASS — deny direction enforced live; allow path NOT implemented (grant branch issues no syscall)"
    exit 0
else
    echo "kernel_enforce_test: FAIL"
    exit 1
fi
