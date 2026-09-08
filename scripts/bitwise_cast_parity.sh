#!/usr/bin/env bash
# bitwise_cast_parity.sh — native codegen == interpreter for the bitwise/shift
# builtins (bit_and/bit_or/bit_xor/bit_not/shl/shr) and the polymorphic numeric
# casts (as_i64/as_f64).
#
# These had NO codegen lowering, so native silently returned 0 (a real
# native↔interp divergence on simple, commonly-used builtins). They are now
# lowered INLINE in emit_call (trivial integer ops / call-site type dispatch).
# This harness builds both engines and asserts identical exit values.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "bitwise_cast_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "bitwise_cast_parity: interp build unavailable — skipping"; exit 0
fi
AXON="${AXON:-target/debug/axon}"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

fail=0
check() {
  local name="$1" src="$2"
  printf '%s\n' "$src" > "$WORK/$name.ax"
  "$INTERP" "$WORK/$name.ax" >/dev/null 2>&1; local i=$?
  local berr; berr="$("$AXON" build "$WORK/$name.ax" -o "$WORK/$name" 2>&1)"
  if [ -f "$WORK/$name" ]; then
    "$WORK/$name" >/dev/null 2>&1; local n=$?
    if [ "$i" = "$n" ]; then echo "  OK   $name: interp=$i native=$n"
    else echo "  FAIL $name: interp=$i native=$n"; fail=1; fi
  elif printf '%s' "$berr" | grep -qE 'IR verification failed|LLVM ERROR|E0910'; then
    # A COMPILER refusal or bug, NOT an unavailable toolchain. This used to
    # discard stderr entirely (`>/dev/null 2>&1`), so every codegen failure --
    # including invalid IR -- printed SKIP and the harness still summarised
    # PASS. A sibling harness hid a real `arr_max_by`-over-structs IR bug
    # exactly this way.
    echo "  FAIL $name (native build error, not unavailability):"
    printf '%s\n' "$berr" | head -3 | sed 's/^/        /'
    fail=1
  else
    echo "  SKIP $name (native build unavailable):"
    printf '%s\n' "$berr" | head -2 | sed 's/^/        /'
  fi
}

check band   'fn main() -> i64 { bit_and(12, 10) }'
check bor    'fn main() -> i64 { bit_or(12, 10) }'
check bxor   'fn main() -> i64 { bit_xor(12, 10) }'
check bnot   'fn main() -> i64 { bit_not(0) }'
check shl    'fn main() -> i64 { shl(1, 4) }'
check shr    'fn main() -> i64 { shr(256, 2) }'
check shrneg 'fn main() -> i64 { shr(0 - 8, 1) }'
check asi_f  'fn main() -> i64 { as_i64(3.9) }'
check asi_b  'fn main() -> i64 { as_i64(true) }'
check asf_i  'fn main() -> i64 { f64_to_i64(as_f64(7) * 2.0) }'
check asf_b  'fn main() -> i64 { f64_to_i64(as_f64(true) * 10.0) }'

[ "$fail" -eq 0 ] || { echo "bitwise_cast_parity: FAIL"; exit 1; }
echo "bitwise_cast_parity: PASS — bitwise/shift + as_i64/as_f64 match the interpreter ✓"
exit 0
