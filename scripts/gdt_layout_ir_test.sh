#!/usr/bin/env bash
# R17 Slice 3 acceptance gate `axon_repr_c_gdt_layout_byte_exact` (golden IR).
#
# A `@[repr(C)] @[packed]` hardware-descriptor struct must lower to a byte-exact
# packed LLVM struct type with no inter-field padding. The x86-64 GDT entry is
# exactly 8 bytes: u16 + u16 + u8 + u8 + u8 + u8 → `<{ i16, i16, i8, i8, i8, i8 }>`.
# The `<{ … }>` (angle-brace) form is LLVM's packed-struct syntax — the
# load-bearing property: without @[packed] the fields could be padded.
#
# Requires: axon (with codegen). Skips gracefully if codegen is absent.
# Usage: scripts/gdt_layout_ir_test.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"

skip() { echo "SKIP: $*" >&2; exit 0; }
fail() { echo "FAIL: $*" >&2; exit 1; }

AXON_BIN=""
for candidate in \
    "$REPO/target/debug/axon" \
    "$REPO/target/release/axon" \
    "$(command -v axon 2>/dev/null || true)"
do
    if [[ -x "$candidate" ]]; then AXON_BIN="$candidate"; break; fi
done
[[ -n "$AXON_BIN" ]] || skip "axon binary not found (build with: cargo build -p axon-core)"

if ! "$AXON_BIN" build --help 2>&1 | grep -q "emit-llvm"; then
    skip "axon binary lacks codegen / --emit-llvm support"
fi

# Robust codegen detection (a --no-default-features binary still parses the flag
# but prints a "requires the codegen feature" error and exits 1).
PROBE="$(mktemp /tmp/axon_probe.XXXXXX.ax)"
PROBE_LL="${PROBE%.ax}.ll"
printf 'fn main() { let _x = 1 }\n' > "$PROBE"
probe_out="$("$AXON_BIN" build --emit-llvm "$PROBE" --out "$PROBE_LL" 2>&1 || true)"
rm -f "$PROBE" "$PROBE_LL"
if echo "$probe_out" | grep -q "requires building axon with the \`codegen\` feature"; then
    skip "axon binary built without the codegen feature (use: cargo build -p axon-core)"
fi

SRC="$REPO/examples/kernel/hello_kernel_slice3.ax"
[[ -f "$SRC" ]] || fail "missing example $SRC"

IR="$(mktemp /tmp/axon_gdt_ir.XXXXXX.ll)"
trap 'rm -f "$IR"' EXIT

"$AXON_BIN" build --freestanding --emit-llvm "$SRC" --out "$IR" >/dev/null 2>&1 \
    || fail "axon build --emit-llvm failed"

# The byte-exact packed GDT layout. Whitespace in LLVM IR is normalized, so an
# exact-substring grep is a faithful golden check.
EXPECT='%GdtEntry = type <{ i16, i16, i8, i8, i8, i8 }>'
if ! grep -qF "$EXPECT" "$IR"; then
    echo "--- emitted struct types ---" >&2
    grep -nE '= type' "$IR" >&2 || true
    fail "GDT layout not byte-exact; expected: $EXPECT"
fi

# Guard against a silent regression to a non-packed layout (no angle braces).
if grep -qE '%GdtEntry = type \{ ' "$IR"; then
    fail "GdtEntry emitted as a NON-packed struct (lost @[packed] padding control)"
fi

echo "PASS: GdtEntry lowers to byte-exact packed layout: $EXPECT"
exit 0
