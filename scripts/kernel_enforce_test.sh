#!/usr/bin/env bash
# kernel_enforce_test.sh — proves the axon-guest-kernel's syscall gate performs LIVE
# capability enforcement: a real `openat` syscall is intercepted by the hardware gate
# (SYSCALL → LSTAR → syscall_dispatch) and DENIED when the policy withholds the FS effect,
# and ALLOWED when it is granted. This is the end-to-end K3+K5 enforcement claim.
#
# Requires: the freestanding kernel built at target/x86_64-axon-metal/release/, plus
# Firecracker + KVM (fc_boot_test.sh provides the boot harness). Skips cleanly if absent.
set -uo pipefail
cd "$(dirname "$0")/.."

KERNEL="target/x86_64-axon-metal/release/axon-guest-kernel"
if [[ ! -f "$KERNEL" ]]; then
    echo "kernel_enforce_test: kernel not built — skipping"
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

# ── Case 1: FS WITHHELD → the openat must be DENIED (live VIOLATION) ───────────
echo "kernel_enforce_test: case 1 — policy IO-only (FS withheld) → expect VIOLATION"
OUT_DENY="$(boot "$(b64 '{"allowed_effects":["IO"]}')")"
if grep -q "VIOLATION: syscall 257 blocked (FS not in policy)" <<<"$OUT_DENY" \
   && grep -q "VIOLATION8" <<<"$OUT_DENY"; then
    echo "  ✓ the gate DENIED the openat syscall and halted with exit code 8"
else
    echo "  ✗ expected a live VIOLATION on openat under an FS-withholding policy"
    echo "$OUT_DENY" | grep -iE 'axon-kernel|K5|violation' | sed 's/^/      /' | tail -8
    fail=1
fi

# ── Case 2: FS GRANTED → no violation, clean halt ─────────────────────────────
echo "kernel_enforce_test: case 2 — policy IO+FS (granted) → expect NO violation"
OUT_OK="$(boot "$(b64 '{"allowed_effects":["IO","FS"]}')")"
if grep -q "K5: policy GRANTS FS" <<<"$OUT_OK" && ! grep -q "VIOLATION: syscall" <<<"$OUT_OK"; then
    echo "  ✓ the gate PERMITTED the openat (FS granted) — no false violation"
else
    echo "  ✗ expected a clean run (no violation) under an FS-granting policy"
    fail=1
fi

echo ""
if [[ $fail -eq 0 ]]; then
    echo "kernel_enforce_test: PASS — the syscall gate denies/permits by policy, live, end-to-end"
    exit 0
else
    echo "kernel_enforce_test: FAIL"
    exit 1
fi
