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
# Requires: the freestanding kernel (AXON_GUEST_KERNEL, else built here),
# plus Firecracker + KVM (fc_boot_test.sh provides the boot harness). Prerequisites
# absent → prints "skipping" and exits 0; gate.sh records that as a SKIP, never a PASS.
set -uo pipefail
cd "$(dirname "$0")/.."

command -v firecracker >/dev/null 2>&1 || { echo "kernel_enforce_test: firecracker absent — skipping"; exit 0; }
[[ -e /dev/kvm ]] || { echo "kernel_enforce_test: /dev/kvm absent — skipping"; exit 0; }

# The kernel booted is the one the caller names in AXON_GUEST_KERNEL, or the one
# built HERE from this tree -- never one that merely sits under target/ or
# $CARGO_TARGET_DIR (C9 round 4; scripts/lib/axon_bin.sh).
. scripts/lib/axon_bin.sh
if [[ -z "${AXON_GUEST_KERNEL:-}" ]]; then
    if ! kerr="$(cargo build -q -p axon-guest-kernel \
            --target "$(pwd)/crates/axon-guest-kernel/targets/x86_64-axon-metal.json" \
            -Z build-std=core,compiler_builtins -Z build-std-features=compiler-builtins-mem \
            -Z json-target-spec --release 2>&1)"; then
        printf '%s\n' "$kerr" | tail -5 | sed 's/^/    /'
        echo "kernel_enforce_test: kernel build unavailable — skipping"
        exit 0
    fi
    use_built AXON_GUEST_KERNEL axon-guest-kernel x86_64-axon-metal release
fi
KERNEL="$AXON_GUEST_KERNEL"
export AXON_GUEST_KERNEL   # fc_boot_test.sh boots exactly this artifact
if [[ ! -f "$KERNEL" ]]; then
    echo "kernel_enforce_test: FAIL — AXON_GUEST_KERNEL=$KERNEL is not a file" >&2
    exit 1
fi

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
