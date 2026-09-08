#!/usr/bin/env bash
# R19 Slice C parity gate: interp vs native for fixed-width / unsigned integer types.
# Usage: bash scripts/unsigned_parity.sh
# Exit 0 = all cases byte-identical; non-zero = failure.

set -euo pipefail
AXON=$(pwd)/target/debug/axon
TMPDIR_LOCAL=$(mktemp -d)
trap 'rm -rf "$TMPDIR_LOCAL"' EXIT

fail() { echo "FAIL: $1"; exit 1; }
pass() { echo "PASS: $1"; }

run_case() {
  local name="$1"
  local src="$2"
  local expect_exit="${3:-0}"

  local axfile="$TMPDIR_LOCAL/${name}.ax"
  local native="$TMPDIR_LOCAL/${name}_native"
  printf '%s\n' "$src" > "$axfile"

  # --- interp ---
  set +e
  interp_out=$("$AXON" run "$axfile" 2>/dev/null)
  interp_exit=$?
  set -e

  if [[ "$interp_exit" -ne "$expect_exit" ]]; then
    fail "$name: interp exit=$interp_exit expected=$expect_exit"
  fi

  # --- native ---
  "$AXON" build --no-cache "$axfile" -o "$native" 2>/dev/null
  set +e
  native_out=$("$native" 2>/dev/null)
  native_exit=$?
  set -e

  if [[ "$native_exit" -ne "$expect_exit" ]]; then
    fail "$name: native exit=$native_exit expected=$expect_exit"
  fi

  if [[ "$interp_out" != "$native_out" ]]; then
    fail "$name: stdout diverges
  interp: $(printf '%q' "$interp_out")
  native: $(printf '%q' "$native_out")"
  fi

  pass "$name"
}

# A struct/str/array argument to `as_i64`/`as_f64`/`as_u8`/… is a runtime panic
# in the interpreter. Native used to drop out of the builtin arm into the generic
# user-call path and emit a call to a function that does not exist with the wrong
# arity, so it died in the LLVM verifier with "Incorrect number of arguments
# passed to called function!" — naming neither the builtin nor the file. E0910
# now, with the interpreter's own reason.
run_refused() {
  local name="$1" src="$2"
  local axfile="$TMPDIR_LOCAL/${name}.ax"
  local native="$TMPDIR_LOCAL/${name}_native"
  printf '%s\n' "$src" > "$axfile"

  set +e
  out=$("$AXON" build --no-cache "$axfile" -o "$native" 2>&1)
  set -e
  case "$out" in
    *E0910*) ;;
    *) fail "$name: expected an E0910 refusal, got: $(printf '%s' "$out" | head -3)" ;;
  esac
  # …and it must be a REFUSAL, not a crash that happens to mention E0910.
  case "$out" in
    *"verification failed"*) fail "$name: reached the IR verifier: $out" ;;
  esac
  pass "$name (refused)"
}

# --- Test cases ---

run_case "u32_max_display" '
fn main() -> i64 {
  let a: u32 = 4294967295
  println(to_str(a))
  0
}'

run_case "u32_add_nowrap" '
fn main() -> i64 {
  let a: u32 = 2000000000
  let b: u32 = 1000000000
  let c = a + b
  println(to_str(c))
  0
}'

run_case "u8_display" '
fn main() -> i64 {
  let a: u8 = 255
  println(to_str(a))
  0
}'

run_case "u8_div" '
fn main() -> i64 {
  let a: u8 = 200
  let b: u8 = 10
  println(to_str(a / b))
  0
}'

run_case "u32_unsigned_cmp" '
fn main() -> i64 {
  let x: u32 = 3000000000
  let y: u32 = 100
  if x > y { println("gt") } else { println("le") }
  0
}'

run_case "i32_signed_arith" '
fn main() -> i64 {
  let a: i32 = 1000000
  let b: i32 = 500000
  println(to_str(a - b))
  0
}'

run_case "u16_rem" '
fn main() -> i64 {
  let a: u16 = 65000
  let b: u16 = 256
  println(to_str(a % b))
  0
}'

run_case "u8_overflow_panic" '
fn main() -> i64 {
  let a: u8 = 200
  let b: u8 = 100
  let c = a + b
  println(to_str(c))
  0
}' 101

run_case "u32_div_by_zero_panic" '
fn main() -> i64 {
  let a: u32 = 10
  let b: u32 = 0
  println(to_str(a / b))
  0
}' 101

run_case "i8_overflow_panic" '
fn main() -> i64 {
  let a: i8 = 100
  let b: i8 = 100
  let c = a + b
  println(to_str(c))
  0
}' 101

run_refused "as_i64_struct" 'type P = { a: i64, b: i64 }
fn main() -> i64 { let p = P { a: 1, b: 2 }  as_i64(p) }'

run_refused "as_i64_str" '
fn main() -> i64 { as_i64("hi") }'

run_refused "as_f64_struct" 'type P = { a: i64, b: i64 }
fn main() -> i64 { let p = P { a: 1, b: 2 }  let x = as_f64(p)  0 }'

run_refused "as_u8_struct" 'type P = { a: i64, b: i64 }
fn main() -> i64 { let p = P { a: 1, b: 2 }  as_i64(as_u8(p)) }'

# …and the scalar conversions the refusal sits next to must keep lowering.
run_case "as_scalar_casts_still_lower" '
fn main() -> i64 {
  println(to_str(as_i64(1.9)))
  println(to_str(as_i64(as_u8(44))))
  println(to_str(as_i64(true)))
  println(to_str_f64(as_f64(3)))
  0
}'

# --- Un-lowerable struct FIELD: refuse the type, do not desync its indices ---
#
# `declare_types` built the LLVM struct body with `filter_map`, silently DROPPING
# any field with no LLVM lowering (`Dict`), while the field-NAME list kept every
# field. Every consumer indexes the LLVM struct with a position from the name
# list, so the two disagreed. With the un-lowerable field last, the codegen
# worker panicked on a bare `unwrap()` (`Err(GEPIndex)`) naming neither struct
# nor file; with it FIRST, the shifted indices stayed in range and reads/writes
# silently landed on the WRONG field — a wrong answer, not a refusal.
run_refused "dict_field_last" '
type B = { n: i64, sums: Dict }
fn main() -> i64 { let b = B { n: 3, sums: dict_new() }  b.n }'

run_refused "dict_field_first" '
type B = { d: Dict, a: i64, b: i64 }
fn main() -> i64 { let x = B { d: dict_new(), a: 11, b: 22 }  x.a }'

# The ordinary struct next door must keep lowering — the refusal is per-type.
run_case "plain_struct_still_lowers" '
type P = { x: i64, y: f64, n: str }
fn main() -> i64 {
  let p = P { x: 7, y: 1.5, n: "hi" }
  println(to_str(p.x))
  println(p.n)
  0
}'

# Same desync at the PARAMETER list: `declare_one_fn_named` filtered the LLVM
# signature while the binding loop walks `f.params.iter().enumerate()` and calls
# `get_nth_param(i)` with the SOURCE position — so `f(d: Dict, a, b)` bound `a`
# to the slot holding `b`. Wrong answer, no diagnostic.
run_refused "dict_param_first" '
fn f(d: Dict, a: i64, b: i64) -> i64 { a * 100 + b }
fn main() -> i64 { f(dict_new(), 11, 22) }'

run_refused "dict_param_last" '
fn f(a: i64, d: Dict) -> i64 { a }
fn main() -> i64 { f(7, dict_new()) }'

# …and at the RETURN type: an un-lowerable return silently became `void`, so the
# caller emitted `call void @mk()` and the IR VERIFIER died with "Incorrect
# number of arguments passed to called function!" — naming neither fn nor file.
run_refused "dict_in_tuple_return" '
fn mk() -> (i64, Dict, i64) { (11, dict_new(), 22) }
fn main() -> i64 { let t = mk()  t.0 }'

# Controls: a genuinely-void fn and an ordinary tuple return must keep lowering
# (Unit/Never lower to `None` legitimately — the guard must not catch them).
run_case "void_fn_and_tuple_return_still_lower" '
fn shout(s: str) { println(s) }
fn pair() -> (i64, i64) { (3, 4) }
fn main() -> i64 {
  shout("hi")
  let p = pair()
  println(to_str(p.0))
  println(to_str(p.1))
  0
}'

# A struct field referring to a type declared LATER must lower. This used to
# depend on source order: `llvm_type` resolves a struct by name, so a
# forward reference gave `None`, which the old `filter_map` dropped — desyncing
# the field indices of a program with no un-lowerable types in it at all.
# `declare_types` now creates every named struct opaque in a first pass.
run_case "forward_referenced_struct_field" '
type Outer = { inner: Inner, tag: i64 }
type Inner = { v: i64 }
fn main() -> i64 {
  let o = Outer { inner: Inner { v: 5 }, tag: 9 }
  println(to_str(o.inner.v))
  println(to_str(o.tag))
  0
}'

# Fifth site of the same family, this one at the CALL: an argument that failed
# to emit was `continue`d past, so the call went out with a SHORT argument list
# — `call void @println()` with no args — and died in the IR verifier as
# "Incorrect number of arguments passed to called function!", naming neither the
# call nor the file. Three shipped examples (classify/code_review/goal_attribute)
# died exactly this way. The argument must sink the call instead.
run_refused "unlowerable_call_argument" '
fn main() -> i64 {
  let d = dict_new()
  dict_set(d, "k", 1)
  println(to_str(as_i64(d)))
  0
}'

echo ""
echo "All parity checks passed."
