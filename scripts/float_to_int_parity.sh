#!/usr/bin/env bash
# float_to_int_parity.sh — native↔interp parity on f64→i64 conversion.
#
# The interpreter converts with Rust's `as i64`, which has been SATURATING since
# Rust 1.45: out-of-range → i64::MAX / i64::MIN, NaN → 0. Native codegen used the
# raw LLVM `fptosi`, whose result is UNDEFINED for out-of-range / NaN inputs — it
# produced garbage (i64::MIN for 1e30, NaN, +Inf), a silent-wrong-result (I-9).
# The native wrapper is now saturating too; this harness pins the agreement.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "float_to_int_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "float_to_int_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# (label, body of fn main) — each prints f64_to_i64 of an edge value. Values are
# built through arithmetic so nothing folds to a constant at compile time.
declare -A CASES
CASES[huge_pos]='let f = 1.0e30
    println(to_str(f64_to_i64(f)))'
CASES[huge_neg]='let f = 0.0 - 1.0e30
    println(to_str(f64_to_i64(f)))'
CASES[nan]='let a = 0.0
    let b = 0.0
    println(to_str(f64_to_i64(a / b)))'
CASES[pos_inf]='let a = 1.0
    let b = 0.0
    println(to_str(f64_to_i64(a / b)))'
CASES[neg_inf]='let a = 0.0 - 1.0
    let b = 0.0
    println(to_str(f64_to_i64(a / b)))'
CASES[normal]='let f = 3.7
    println(to_str(f64_to_i64(f)))'
CASES[normal_neg]='let f = 0.0 - 3.7
    println(to_str(f64_to_i64(f)))'
CASES[as_cast_huge]='let f = 1.0e30
    println(to_str(f as i64))'

fail=0
for label in "${!CASES[@]}"; do
  PROG="$WORK/$label.ax"
  { echo "fn main() {"; echo "    ${CASES[$label]}"; echo "}"; } > "$PROG"

  iout="$("$AXON" run "$PROG" 2>&1)"; iexit=$?
  BIN="$WORK/${label}_bin"
  if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
    echo "float_to_int_parity: native build failed for $label — skipping"; exit 0
  fi
  nout="$("$BIN" 2>&1)"; nexit=$?

  if [ "$iexit" -ne "$nexit" ] || [ "$iout" != "$nout" ]; then
    echo "float_to_int_parity: FAIL — $label diverged"
    echo "  interp: exit=$iexit out=[$iout]"
    echo "  native: exit=$nexit out=[$nout]"
    fail=1
  else
    echo "  $label: interp==native ($iout) ✓"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "float_to_int_parity: FAILED"; exit 1
fi
echo "float_to_int_parity: OK — native f64→i64 saturates like the interpreter"
exit 0
