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
. "$ROOT/scripts/lib/axon_bin.sh"
. "$ROOT/scripts/lib/harness_skip.sh"

if [ -n "${AXON:-}" ]; then
  named_bin AXON bitwise_cast_parity
else
  echo "bitwise_cast_parity: building codegen axon binary…"
  cargo_build_or_skip bitwise_cast_parity llvm cargo build -q -p axon-core --bin axon
  use_built AXON axon
fi
require_codegen_axon bitwise_cast_parity
if [ -n "${AXON_RUN:-}" ]; then
  named_bin AXON_RUN bitwise_cast_parity
else
  cargo_build_or_skip bitwise_cast_parity interp cargo build -q -p axon-core --no-default-features --bin axon-run
  use_built AXON_RUN axon-run
fi
INTERP="$AXON_RUN"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

fail=0
ran=0   # rows that actually COMPARED the two engines
check() {
  local name="$1" src="$2"
  # The value under test travels on STDOUT, not through the exit code.
  # `governance/EXIT_CODES.md` reserves 2..=15 (and 101), and a `main`
  # returning any of them is remapped to exit 1 -- so `bit_and(12,10)`=8 and
  # `bit_or(12,10)`=14 both arrive as exit 1 on BOTH sides, and a native
  # answer of 8 where the interpreter says 14 used to pass as parity. Seven of
  # the eleven rows below sat in that blind zone. Rename the case's `main` to
  # `probe` and print its result from a fresh `main` so the full value is
  # comparable; the exit code is still compared alongside.
  printf '%s\n' "${src/fn main() -> i64/fn probe() -> i64}" > "$WORK/$name.ax"
  printf 'fn main() { println(to_str(probe())) }\n' >> "$WORK/$name.ax"
  local io; io="$("$INTERP" "$WORK/$name.ax" 2>&1)"; local i=$?
  local berr; berr="$("$AXON" build "$WORK/$name.ax" -o "$WORK/$name" 2>&1)"
  if [ -f "$WORK/$name" ]; then
    local no; no="$("$WORK/$name" 2>&1)"; local n=$?
    ran=$((ran+1))
    if [ "$i" = "$n" ] && [ "$io" = "$no" ]; then echo "  OK   $name: interp==native (exit=$i out=$io)"
    else
      echo "  FAIL $name: interp(exit=$i out=[$io]) native(exit=$n out=[$no])"; fail=1
    fi
  elif native_build_unavailable "$berr"; then
    # Only a PROVEN reason (scripts/lib/harness_skip.sh's allow-list) skips a
    # row. This used to be a deny-list -- only IR-verifier / LLVM / E0910 text
    # failed, so any other compiler error printed SKIP and the harness still
    # summarised PASS. A sibling harness hid a real `arr_max_by`-over-structs
    # IR bug exactly this way.
    echo "  SKIP $name (native build unavailable):"
    printf '%s\n' "$berr" | head -2 | sed 's/^/        /'
  else
    echo "  FAIL $name (native build FAILED — a build error is not a skip):"
    printf '%s\n' "$berr" | head -5 | sed 's/^/        /'
    fail=1
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
# Vacuity guard: if every `check` row took the SKIP branch this would print
# PASS for zero comparisons. A harness that compared nothing is a SKIP.
if [ "$ran" -eq 0 ]; then
  harness_skip bitwise_cast_parity "nothing ran (native build unavailable for every case)"
fi
echo "bitwise_cast_parity: PASS — bitwise/shift + as_i64/as_f64 match the interpreter ✓"
exit 0
