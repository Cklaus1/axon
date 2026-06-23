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

echo ""
echo "All parity checks passed."
