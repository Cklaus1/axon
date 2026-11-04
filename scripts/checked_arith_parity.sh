#!/usr/bin/env bash
# checked_arith_parity.sh — native↔interp parity on checked integer arithmetic.
#
# The interpreter is the reference semantics (`interp/value.rs`): signed i64
# `+`/`-`/`*` overflow and `/`/`%` by zero are CHECKED — they raise a graceful
# panic (`axon: panic: …`, exit 101), never a silent two's-complement wrap (a
# wrong answer masquerading as success — ARCHITECTURE INVARIANTS I-9) and never
# a raw hardware SIGFPE (exit 136, no message). Native codegen used to do
# exactly those wrong things; this harness pins the fix so it can't regress.
#
# For each program we compare BOTH the exit code AND stdout/stderr between
# `axon run` (interp oracle) and the native AOT binary. They must be identical.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"
. "$ROOT/scripts/lib/axon_bin.sh"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ -n "${AXON:-}" ]; then
  named_bin AXON checked_arith_parity
else
  echo "checked_arith_parity: building codegen axon binary…"
  cargo_build_or_skip checked_arith_parity llvm cargo build -q -p axon-core --bin axon
  cargo_build_or_skip checked_arith_parity interp cargo build -q -p axon-rt
  use_built AXON axon
fi
require_codegen_axon checked_arith_parity

# (label, body) — each body is a `fn main()` body, after the label's DEFS (fn
# definitions), if any. The interesting cases are the faults (must panic exit
# 101 with a matching message) and the boundary cases that must NOT panic
# (INT_MIN/-1 div+rem, max value that just fits). Every case is built at O0 and
# at O3, where the optimised lowerings (AX-78's accumulator loop) apply.
declare -A CASES
declare -A DEFS
CASES[div_zero]='let a = 5
    let b = 0
    println(to_str(a / b))'
CASES[mod_zero]='let a = 5
    let b = 0
    println(to_str(a % b))'
CASES[overflow_add]='let big = 9223372036854775807
    println(to_str(big + 1))'
CASES[overflow_sub]='let m = 0 - 9223372036854775807
    println(to_str(m - 2))'
CASES[overflow_mul]='let big = 9223372036854775807
    println(to_str(big * 2))'
# Boundary: INT_MIN / -1 traps in hardware AND overflows arithmetically (the
# true answer 2^63 does not fit), so both engines must report the same graceful
# overflow panic — never a SIGFPE, and never the silent INT_MIN this used to
# return (governance/reviews/2026-07-31-deep-review.md §349).
# INT_MIN % -1 is different: 0 IS representable, so it stays defined and both
# engines must return 0.
CASES[int_min_div_neg1]='let m = 0 - 9223372036854775807
    let mm = m - 1
    println(to_str(mm / (0 - 1)))'
CASES[int_min_rem_neg1]='let m = 0 - 9223372036854775807
    let mm = m - 1
    println(to_str(mm % (0 - 1)))'
# Boundary: 20! fits in i64 (no panic); 21! overflows (panic). The loop also
# exercises the guard inside a hot path.
CASES[factorial_20_ok]='let f = 1
    let i = 1
    while i <= 20 {
        f = f * i
        i = i + 1
    }
    println(to_str(f))'
CASES[factorial_21_overflow]='let f = 1
    let i = 1
    while i <= 21 {
        f = f * i
        i = i + 1
    }
    println(to_str(f))'
# Normal arithmetic must be untouched by the guards.
CASES[normal_mix]='println(to_str(2 + 3 * 4 - 17 / 5 + 17 % 5))'
# abs/pow runtime faults: must panic 101 with the SAME message on both engines
# (interp used to leak a raw Rust "negate with overflow" thread panic; native
# used to abort() / SIGABRT exit 134). Variable operands so nothing folds early.
CASES[abs_i64_min]='let m = 0 - 9223372036854775807
    let mm = m - 1
    println(to_str(abs_i64(mm)))'
CASES[pow_neg_exp]='let e = 0 - 1
    println(to_str(pow_i64(2, e)))'
CASES[abs_ok]='println(to_str(abs_i64(0 - 42)))'
# Arithmetic builtins overflow like the operators (checker-soundness-2 CS2-2):
# exit 101 with the same message on both engines -- pow_i64 used to wrap (and
# truncate an exponent above u32::MAX to 0), arr_sum_i64 to saturate, dict_inc
# to raise a Rust overflow panic. arr_mean_i64 sums exactly in i128.
CASES[pow_ovf]='let e = 40
    println(to_str(pow_i64(3, e)))'
CASES[pow_exp_trunc]='let e = 4294967296
    println(to_str(pow_i64(2, e)))'
CASES[sum_ovf]='let a = [9223372036854775807, 1]
    println(to_str(arr_sum_i64(a)))'
CASES[dictinc_ovf]='let d = dict_new()
    dict_set(d, "k", 9223372036854775807)
    println(to_str(dict_inc(d, "k")))'
CASES[mean_i128]='let m = 9223372036854775807
    let n = 0 - m - 1
    println(to_str_f64(arr_mean_i64([m, m, n, n])))
    println(to_str_f64(arr_mean_i64([m, m, 1])))'
# Array bounds: OOB read must panic 101 with the interp message on BOTH engines
# (native used to do an unchecked GEP → garbage value / arbitrary-memory read at
# exit 0). Index via a variable so nothing folds; len-3 slice.
CASES[arr_oob_high]='let a = [10, 20, 30]
    let i = 5
    println(to_str(a[i]))'
CASES[arr_oob_neg]='let a = [10, 20, 30]
    let i = 0 - 1
    println(to_str(a[i]))'
CASES[arr_in_bounds]='let a = [10, 20, 30]
    let i = 2
    println(to_str(a[i]))'

# AX-74: a constant divisor drops the zero check and the MIN/-1 guard it
# cannot trip; the quotient, the remainder and `MIN / -1` stay the same.
CASES[const_divisor]='let m = 0 - 9223372036854775807
    let mm = m - 1
    println(to_str(mm / 7))
    println(to_str(mm % 7))
    println(to_str(mm % -1))'
CASES[const_divisor_neg1]='let m = 0 - 9223372036854775807
    let mm = m - 1
    println(to_str(mm / -1))'
# AX-70: an `if` with checked arithmetic in its arms computes both arms and
# selects; the selected arm overflowing must panic with the SAME operands
# (the first faulting op of that arm), and the other arm overflowing must not.
CASES[if_arm_mul_ovf]='let x = 3074457345618258603
    let y = if x % 2 == 0 { x / 2 } else { 3 * x + 1 }
    println(to_str(y))'
CASES[if_arm_second_op_ovf]='let x = 4611686018427387903
    let y = if x < 0 { x } else { x * 2 + 2 }
    println(to_str(y))'
CASES[if_untaken_arm_ovf]='let x = 9223372036854775807
    let y = if x > 0 { x - 1 } else { x * 3 }
    println(to_str(y))'
CASES[if_count_ovf]='let c = 9223372036854775806
    let i = 0
    while i < 3 {
        if i != 1 { c = c + 1 }
        i = i + 1
    }
    println(to_str(c))'
# AX-76: a counter bumped once per trip of `for k in lo..end` and started
# at c0 <= lo cannot overflow, so its bump is unchecked. At c0 = lo it
# reaches i64::MAX exactly; at c0 > lo the bump stays checked and panics.
CASES[for_count_at_max]='let c = 9223372036854775805
    for k in 9223372036854775805..9223372036854775807 {
        if k > 0 { c = c + 1 }
    }
    println(to_str(c))'
CASES[for_count_start_above_lo_ovf]='let c = 9223372036854775806
    for k in 0..3 {
        c = c + 1
    }
    println(to_str(c))'
# AX-76: `arr_repeat` of a bool fills bytes (a memset); elements read back.
CASES[bool_repeat]='let f = arr_repeat(true, 5)
    f[1] = false
    let n = 0
    for k in 0..5 {
        if f[k] { n = n + 1 }
    }
    println(to_str(n))'
# AX-78: above O0 `fn f(..) -> i64 { if C { B } else { L + f(A) } }` runs as
# a loop summing the left operands. Each sum must overflow exactly when one of
# the source's adds does, with that add's operands: deep in a binary recursion
# (`fibk`), past `MIN` reached exactly (`fibneg`), in the inner add after the
# loop stops reordering (`sumk`), only in an outer add after a negative term
# (`dip`, re-run in source order), past the 64-trip frame budget (`long`); a
# fault in `L` is the source's own (`div3`). Panicking cases print nothing
# first, so stdout buffering cannot reorder the combined output.
ACCUM_FNS='fn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }
fn fibk(n: i64) -> i64 { if n < 2 { n * 1000000000000000 } else { fibk(n - 1) + fibk(n - 2) } }
fn fibneg(n: i64) -> i64 { if n < 2 { 0 - n * 4611686018427387904 } else { fibneg(n - 1) + fibneg(n - 2) } }
fn sumk(n: i64, k: i64) -> i64 { if n == 0 { 0 } else { k + sumk(n - 1, k) } }
fn dip(n: i64) -> i64 { if n == 0 { 4611686018427387904 } else { (if n == 1 { 0 - 1 } else { 4611686018427387904 }) + dip(n - 1) } }
fn long(n: i64) -> i64 { if n == 0 { 9223372036854775000 } else { (if n == 1 { 0 - 1000 } else { 1 }) + long(n - 1) } }
fn div3(n: i64) -> i64 { if n < 1 { 0 } else { 100 / (n - 3) + div3(n - 1) } }'
CASES[accum_values]='println(to_str(fib(25)))
    println(to_str(fibk(20)))
    println(to_str(fibneg(3)))
    println(to_str(sumk(2, 4611686018427387903)))
    println(to_str(sumk(5000, 1)))
    println(to_str(dip(2)))
    println(to_str(long(200)))
    println(to_str(div3(2)))'
CASES[accum_fib_ovf]='println(to_str(fibk(23)))'
CASES[accum_fib_neg_ovf]='println(to_str(fibneg(4)))'
CASES[accum_inner_add_ovf]='println(to_str(sumk(3, 4611686018427387904)))'
CASES[accum_outer_add_ovf]='println(to_str(dip(3)))'
CASES[accum_budget_ovf]='println(to_str(long(2000)))'
CASES[accum_left_div_zero]='println(to_str(div3(5)))'
for label in accum_values accum_fib_ovf accum_fib_neg_ovf accum_inner_add_ovf \
  accum_outer_add_ovf accum_budget_ovf accum_left_div_zero; do
  DEFS[$label]="$ACCUM_FNS"
done

fail=0
for label in "${!CASES[@]}"; do
  PROG="$WORK/$label.ax"
  {
    if [ -n "${DEFS[$label]:-}" ]; then echo "${DEFS[$label]}"; fi
    echo "fn main() {"
    echo "    ${CASES[$label]}"
    echo "}"
  } > "$PROG"

  # Interpreter (oracle). Capture stderr too (panic messages go there).
  iout="$("$AXON" run "$PROG" 2>&1)"
  iexit=$?

  # Native, unoptimised and optimised.
  for level in 0 3; do
    BIN="$WORK/${label}_O${level}_bin"
    if ! berr="$("$AXON" build "$PROG" -o "$BIN" --no-cache --opt-level "$level" 2>&1)"; then
      # A build that FAILED is a RESULT, not an absence. This used to discard
      # stderr and report a codegen regression in the feature under test as
      # "skipping" — see scripts/lib/harness_skip.sh.
      native_build_failed checked_arith_parity "$label O$level" "$berr" || exit 1
    fi
    nout="$("$BIN" 2>&1)"
    nexit=$?

    if [ "$iexit" -ne "$nexit" ] || [ "$iout" != "$nout" ]; then
      echo "checked_arith_parity: FAIL — $label O$level diverged"
      echo "  interp: exit=$iexit out=[$iout]"
      echo "  native: exit=$nexit out=[$nout]"
      fail=1
    else
      echo "  $label O$level: interp==native (exit $iexit) ✓"
    fi
  done
done

if [ "$fail" -ne 0 ]; then
  echo "checked_arith_parity: FAILED"
  exit 1
fi
echo "checked_arith_parity: OK — native checked arithmetic matches the interpreter (I-9)"
exit 0
