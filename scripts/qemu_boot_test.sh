#!/usr/bin/env bash
# R17 Slice 1 acceptance gate: kernel boots under QEMU, writes "axon s1" to debugcon.
#
# Requires: axon (with codegen), nasm, qemu-system-x86_64, ld.
# Skips gracefully if any tool is missing or if axon lacks codegen support.
#
# Usage: scripts/qemu_boot_test.sh [--timeout SECS]   (default timeout: 5s)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
TIMEOUT_SECS=5

while [[ $# -gt 0 ]]; do
    case "$1" in
        --timeout) TIMEOUT_SECS="$2"; shift 2 ;;
        *) echo "Unknown arg: $1" >&2; exit 1 ;;
    esac
done

skip() { echo "SKIP: $*" >&2; exit 0; }

# ── Tool checks ──────────────────────────────────────────────────────────────

command -v nasm             >/dev/null 2>&1 || skip "nasm not found (install nasm)"
command -v qemu-system-x86_64 >/dev/null 2>&1 || skip "qemu-system-x86_64 not found"

# Find the axon binary (prefer the one just built in the workspace target dir)
AXON_BIN=""
for candidate in \
    "$REPO/target/debug/axon" \
    "$REPO/target/release/axon" \
    "$(command -v axon 2>/dev/null || true)"
do
    if [[ -x "$candidate" ]]; then
        AXON_BIN="$candidate"
        break
    fi
done
[[ -n "$AXON_BIN" ]] || skip "axon binary not found (build with: cargo build -p axon-core)"

# Find a bare-metal linker (ld.bfd → ld → x86_64-elf-ld)
LD_BIN=""
for candidate in ld.bfd ld x86_64-elf-ld; do
    if command -v "$candidate" >/dev/null 2>&1; then
        LD_BIN="$candidate"
        break
    fi
done
[[ -n "$LD_BIN" ]] || skip "no bare-metal linker found (tried ld.bfd, ld, x86_64-elf-ld)"

# ── Build ────────────────────────────────────────────────────────────────────

TMPDIR_LOCAL="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_LOCAL"' EXIT

KERNEL_AX="$REPO/examples/kernel/hello_kernel_slice1.ax"
BOOT_ASM="$REPO/examples/kernel/boot_stub.asm"
LINKER_SCRIPT="$REPO/scripts/kernel.ld"
KERNEL_OBJ="$TMPDIR_LOCAL/kernel.o"
BOOT_OBJ="$TMPDIR_LOCAL/boot_stub.o"
KERNEL_ELF="$TMPDIR_LOCAL/kernel.elf"

echo "=== R17 Slice 1: QEMU boot test ===" >&2
echo "  axon:  $AXON_BIN" >&2
echo "  nasm:  $(command -v nasm)" >&2
echo "  qemu:  $(command -v qemu-system-x86_64)" >&2
echo "  ld:    $LD_BIN" >&2
echo "" >&2

echo "1/3 compiling Axon kernel → $KERNEL_OBJ" >&2
# Probe codegen support via the real build attempt rather than a --help flag
# check: --emit-obj is now listed in --help unconditionally (the CLI surface
# is the same either way; only the runtime behavior is feature-gated), so a
# flag-presence check can no longer distinguish a codegen-enabled binary from
# one built with --no-default-features. Skip gracefully on that specific
# runtime error; any other failure is a real FAIL.
set +e
BUILD_OUT="$("$AXON_BIN" build --freestanding --emit-obj "$KERNEL_AX" --out "$KERNEL_OBJ" 2>&1)"
BUILD_EXIT=$?
set -e
echo "$BUILD_OUT" >&2
if [[ $BUILD_EXIT -ne 0 ]]; then
    if echo "$BUILD_OUT" | grep -q "requires building axon with the .codegen. feature"; then
        skip "axon binary lacks codegen support (build with: cargo build -p axon-core)"
    fi
    echo "FAIL: axon build failed (exit $BUILD_EXIT)" >&2
    exit 1
fi

echo "2/3 assembling boot stub → $BOOT_OBJ" >&2
nasm -f elf64 "$BOOT_ASM" -o "$BOOT_OBJ" 2>&1

echo "3/3 linking → $KERNEL_ELF" >&2
"$LD_BIN" \
    -T "$LINKER_SCRIPT" \
    --entry _start \
    -static \
    --no-dynamic-linker \
    -o "$KERNEL_ELF" \
    "$BOOT_OBJ" "$KERNEL_OBJ" 2>&1

# ── Run under QEMU ───────────────────────────────────────────────────────────

echo "" >&2
echo "Booting under QEMU (timeout ${TIMEOUT_SECS}s)..." >&2

QEMU_OUTPUT="$TMPDIR_LOCAL/qemu_out.txt"

# -debugcon stdio → sends I/O port 0xE9 writes to stdout
# -display none   → headless
# -no-reboot      → exit instead of rebooting on triple fault
set +e
timeout "$TIMEOUT_SECS" \
    qemu-system-x86_64 \
        -kernel "$KERNEL_ELF" \
        -debugcon stdio \
        -display none \
        -no-reboot \
        2>/dev/null \
    > "$QEMU_OUTPUT"
QEMU_EXIT=$?
set -e

# timeout exits 124 when it times out; QEMU exits 0 normally.
# A timeout is OK — the kernel halts in the idle loop, so QEMU runs until timeout.
if [[ $QEMU_EXIT -ne 0 && $QEMU_EXIT -ne 124 ]]; then
    echo "FAIL: qemu exited with code $QEMU_EXIT" >&2
    exit 1
fi

CAPTURED="$(cat "$QEMU_OUTPUT")"
echo "QEMU debugcon output: $(echo "$CAPTURED" | head -3)" >&2

if echo "$CAPTURED" | grep -q "axon s1"; then
    echo "" >&2
    echo "PASS: kernel booted and wrote 'axon s1' to debugcon" >&2
    exit 0
else
    echo "" >&2
    echo "FAIL: 'axon s1' not found in debugcon output" >&2
    echo "Output was: $CAPTURED" >&2
    exit 1
fi
