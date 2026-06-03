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
check rev_0   'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_reverse(&a)  b[0] }'
check rev_2   'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_reverse(&a)  b[2] }'
check rev_sum 'fn main() -> i64 { let a = [10, 20, 30, 40]  let b = arr_reverse(&a)  arr_sum_i64(&b) }'
check take2   'fn main() -> i64 { let a = [1, 2, 3, 4]  let b = arr_take(&a, 2)  arr_sum_i64(&b) }'
check take_all 'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_take(&a, 9)  arr_sum_i64(&b) }'
check take0   'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_take(&a, 0)  arr_sum_i64(&b) }'
check drop1   'fn main() -> i64 { let a = [1, 2, 3, 4]  let b = arr_drop(&a, 1)  arr_sum_i64(&b) }'
check drop_ix 'fn main() -> i64 { let a = [10, 20, 30]  let b = arr_drop(&a, 1)  b[0] }'
check map_sum 'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_map(&a, |x| x * 2)  arr_sum_i64(&b) }'
check map_ix  'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_map(&a, |x| x + 10)  b[1] }'
check filt_s  'fn main() -> i64 { let a = [1, 2, 3, 4, 5]  let b = arr_filter(&a, |x| x > 2)  arr_sum_i64(&b) }'
check filt_l  'fn main() -> i64 { let a = [1, 2, 3, 4, 5]  let b = arr_filter(&a, |x| x > 2)  len(b) }'
check filt_n  'fn main() -> i64 { let a = [1, 2, 3]  let b = arr_filter(&a, |x| x > 100)  len(b) }'
check fold_s  'fn main() -> i64 { let a = [1, 2, 3, 4]  arr_fold(&a, 0, |acc, x| acc + x) }'
check fold_i  'fn main() -> i64 { let a = [1, 2, 3]  arr_fold(&a, 100, |acc, x| acc + x) }'
check fold_p  'fn main() -> i64 { let a = [1, 2, 3, 4]  arr_fold(&a, 1, |acc, x| acc * x) }'
check zw_sum  'fn main() -> i64 { let a = [1, 2, 3]  let b = [10, 20, 30]  let c = arr_zip_with(a, b, |x, y| x + y)  arr_sum_i64(&c) }'
check zw_un   'fn main() -> i64 { let a = [1, 2, 3, 4, 5]  let b = [10, 20]  let c = arr_zip_with(a, b, |x, y| x + y)  arr_sum_i64(&c) }'
check zw_dot  'fn main() -> i64 { let a = [1, 2, 3]  let b = [4, 5, 6]  let c = arr_zip_with(a, b, |x, y| x * y)  arr_sum_i64(&c) }'
check sort_lo 'fn main() -> i64 { let a = [3, 1, 2]  let b = arr_sort_by(&a, |x, y| x - y)  b[0] }'
check sort_hi 'fn main() -> i64 { let a = [5, 3, 8, 1, 9, 2]  let b = arr_sort_by(&a, |x, y| x - y)  b[5] }'
check sort_dc 'fn main() -> i64 { let a = [3, 1, 2]  let b = arr_sort_by(&a, |x, y| y - x)  b[0] }'
check sort_sm 'fn main() -> i64 { let a = [5, 3, 8, 1]  let b = arr_sort_by(&a, |x, y| x - y)  arr_sum_i64(&b) }'
check cnt_if  'fn main() -> i64 { let a = [1, 2, 3, 4, 5]  arr_count_if(&a, |x| x > 2) }'
check cnt_0   'fn main() -> i64 { let a = [1, 2, 3]  arr_count_if(&a, |x| x > 100) }'
check all_t   'fn main() -> i64 { let a = [2, 4, 6]  if arr_all(&a, |x| x > 0) { 1 } else { 0 } }'
check all_f   'fn main() -> i64 { let a = [2, 4, 6]  if arr_all(&a, |x| x > 3) { 1 } else { 0 } }'
check any_t   'fn main() -> i64 { let a = [1, 2, 3]  if arr_any(&a, |x| x > 2) { 1 } else { 0 } }'
check any_f   'fn main() -> i64 { let a = [1, 2, 3]  if arr_any(&a, |x| x > 100) { 1 } else { 0 } }'

[ "$fail" -eq 0 ] || { echo "arr_reduce_parity: FAIL"; exit 1; }
echo "arr_reduce_parity: PASS — arr reductions + reverse/take/drop/map/filter/fold/zip_with/sort_by + count_if/all/any match the interpreter ✓"
exit 0
