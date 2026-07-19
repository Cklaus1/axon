#!/usr/bin/env bash
# R17 acceptance gate `axon_kernel_handles_timer_interrupt`: a real 256-entry
# IDT (built via the fixed-physical-address idiom, R17 spec §12 Q8) is loaded
# via `lidt`, the PIC is remapped, the PIT is programmed, interrupts are
# enabled, and a real hardware timer interrupt must fire repeatedly and reach
# an Axon-compiled @[interrupt] handler — proven by a marker byte ('T')
# streaming to the QEMU debugcon port on every fire. Repeated firing (not
# just one) also proves the handler's EOI actually unmasked the next
# interrupt. Also exercises R17 §12 Q9's freestanding arithmetic trap (the
# IDT-fill loop's address arithmetic runs for real here) — a spurious
# 'A'/'B'/'R' marker would mean it fired when it shouldn't have.
#
# Requires: axon (with codegen), nasm, qemu-system-x86_64, ld.
# Skips gracefully if any tool is missing or if axon lacks codegen support.
#
# Usage: scripts/timer_irq_qemu_test.sh [--timeout SECS]   (default: 2s)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
TIMEOUT_SECS=2

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

KERNEL_AX="$REPO/examples/kernel/hello_kernel_timer_irq.ax"
BOOT_ASM="$REPO/examples/kernel/boot_stub.asm"
LINKER_SCRIPT="$REPO/scripts/kernel.ld"
KERNEL_OBJ="$TMPDIR_LOCAL/kernel.o"
BOOT_OBJ="$TMPDIR_LOCAL/boot_stub.o"
KERNEL_ELF="$TMPDIR_LOCAL/kernel.elf"

echo "=== R17: timer-interrupt QEMU boot test ===" >&2
echo "  axon:  $AXON_BIN" >&2
echo "  nasm:  $(command -v nasm)" >&2
echo "  qemu:  $(command -v qemu-system-x86_64)" >&2
echo "  ld:    $LD_BIN" >&2
echo "" >&2

echo "1/3 compiling Axon kernel → $KERNEL_OBJ" >&2
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

# timeout exits 124 when it times out; QEMU exits 0 normally. Both are OK — the
# kernel idle-loops in `hlt` between interrupts, so QEMU runs until timeout.
if [[ $QEMU_EXIT -ne 0 && $QEMU_EXIT -ne 124 ]]; then
    echo "FAIL: qemu exited with code $QEMU_EXIT" >&2
    exit 1
fi

# grep exits 1 on zero matches even though `wc -l` (rightmost in the pipe)
# still succeeds and correctly prints "0" — under `pipefail` that 1 still
# poisons the whole assignment's exit status, and zero PANICS is the
# expected, PASSING case here, so guard both with a trailing `|| true` (the
# variable is already correctly assigned by the time that runs; this only
# stops `set -e` from treating "found nothing" as a script-ending failure).
TICKS="$(grep -o "T" "$QEMU_OUTPUT" | wc -l)" || true
PANICS="$(grep -oE "[ABR]" "$QEMU_OUTPUT" | wc -l)" || true
echo "debugcon: $TICKS timer tick(s), $PANICS freestanding-trap marker(s)" >&2

if [[ "$PANICS" -gt 0 ]]; then
    echo "" >&2
    echo "FAIL: a freestanding safety trap fired (§12 Q9's arith/bounds/refine" >&2
    echo "trap wrote an 'A'/'B'/'R' marker) — the IDT-fill arithmetic should" >&2
    echo "never overflow/OOB/violate a refinement in this kernel" >&2
    exit 1
fi

# Require more than a couple of ticks — proves the handler's EOI actually
# unmasked the PIC for the next interrupt, not just a lucky one-shot fire.
if [[ "$TICKS" -ge 5 ]]; then
    echo "" >&2
    echo "PASS: timer interrupt fired $TICKS times — IDT/lidt/PIC-remap/PIT/sti/ISR/EOI all correct" >&2
    exit 0
else
    echo "" >&2
    echo "FAIL: expected >=5 timer ticks on debugcon, got $TICKS" >&2
    echo "Output was: $(cat "$QEMU_OUTPUT" | head -c 200)" >&2
    exit 1
fi
