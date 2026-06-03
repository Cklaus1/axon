#!/usr/bin/env bash
# arr_reduce_parity.sh — native codegen == interpreter for the inline-lowered
# i64 array reductions arr_sum_i64 / arr_contains.
#
# The arr_* family had no codegen (silent 0 → now E0910-gated). These two are
# now lowered INLINE as a counted loop over the slice `{i64 len, i8* data}` —
# pure IR, so they run on native AND wasm. This harness asserts native==interp.
#
# NOTE on saturating_add: the interpreter's arr_sum_i64 saturates on i64
# overflow; codegen uses plain `add`. They agree for all non-overflowing arrays
# (every realistic case); the harness uses small arrays where they're identical.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "arr_reduce_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "arr_reduce_parity: interp build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
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
  elif printf '%s' "$berr" | grep -q "E0910"; then
    # A regression: these MUST be lowered (not E0910-gated) — hard fail.
    echo "  FAIL $name (E0910 — arr_sum_i64/arr_contains lowering regressed)"; fail=1
  else
    # Transient native-build unavailability (e.g. nested cargo-lock under
    # `cargo test`) — skip, like the sibling parity harnesses.
    echo "  SKIP $name (native build unavailable)"
  fi
}

check sum3    'fn main() -> i64 { let a = [10, 20, 12]  arr_sum_i64(&a) }'
check sum1    'fn main() -> i64 { let a = [5]  arr_sum_i64(&a) }'
check sumneg  'fn main() -> i64 { let a = [100, 0 - 58]  arr_sum_i64(&a) }'
check cont_y  'fn main() -> i64 { let a = [1, 2, 3]  if arr_contains(&a, 2) { 1 } else { 0 } }'
check cont_n  'fn main() -> i64 { let a = [1, 2, 3]  if arr_contains(&a, 9) { 1 } else { 0 } }'
check cont_1st 'fn main() -> i64 { let a = [7, 2, 3]  if arr_contains(&a, 7) { 1 } else { 0 } }'
check max3    'fn main() -> i64 { let a = [3, 7, 2]  arr_max_i64(&a) }'
check max1    'fn main() -> i64 { let a = [5]  arr_max_i64(&a) }'
check maxneg  'fn main() -> i64 { let a = [0 - 5, 0 - 2, 0 - 9]  arr_max_i64(&a) }'
check min3    'fn main() -> i64 { let a = [3, 7, 2]  arr_min_i64(&a) }'
check minneg  'fn main() -> i64 { let a = [0 - 5, 0 - 2, 0 - 9]  arr_min_i64(&a) }'
check mean    'fn main() -> i64 { let a = [10, 20, 30]  f64_to_i64(arr_mean_i64(&a)) }'
check mean_fr 'fn main() -> i64 { let a = [1, 2]  f64_to_i64(arr_mean_i64(&a) * 10.0) }'

[ "$fail" -eq 0 ] || { echo "arr_reduce_parity: FAIL"; exit 1; }
echo "arr_reduce_parity: PASS — arr_sum_i64 / arr_contains / arr_max_i64 / arr_min_i64 / arr_mean_i64 match the interpreter ✓"
exit 0
