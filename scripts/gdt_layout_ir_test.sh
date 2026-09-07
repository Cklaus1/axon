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

# No --help-based codegen probe: --emit-llvm is listed unconditionally, so
# flag-presence never distinguishes a codegen build from a --no-default-features
# one. The real probe is the trial build below.

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

# Field-STORE width check (found 2026-07-20 as a live bug, not a hypothetical):
# the struct TYPE being byte-exact is necessary but not sufficient — a literal-
# valued field was being stored as `store i64 <val>, ptr <gep>` regardless of
# the field's actual declared width. In a `@[packed]` struct (no padding to
# absorb the overrun), an i64 store at a GEP offset the layout only reserved
# 1 or 2 bytes for silently clobbers the following field(s), or overruns the
# alloca outright for the last field. The struct-type check above would still
# PASS on that broken codegen, which is exactly how it shipped undetected —
# so assert every store into a GdtEntry field alloca is at the field's actual
# width, not a blanket i64.
BAD_STORES="$(grep -E 'store i64 [^,]+, ptr %(limit_lo|base_lo|base_mid|access|flags|base_hi)' "$IR" || true)"
if [[ -n "$BAD_STORES" ]]; then
    echo "--- wrong-width field stores (i64 into a narrower packed slot) ---" >&2
    echo "$BAD_STORES" >&2
    fail "GdtEntry field(s) stored at i64 width, not their declared narrow width — corrupts adjacent packed fields"
fi

echo "PASS: GdtEntry lowers to byte-exact packed layout: $EXPECT (fields store at correct width)"
exit 0
