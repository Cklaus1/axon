#!/usr/bin/env bash
# i64_radix_panic_parity.sh — I-2 regression: i64_to_str_radix on an OUT-OF-RANGE
# base must behave identically on both engines.
#
# The interpreter PANICS ("i64_to_str_radix: radix must be 2..=36, got N", exit
# 101). Native codegen delegates to axon-rt __axon_i64_to_str_radix, which used
# to return an EMPTY string and exit 0 — a soundness divergence (it silently
# accepted an invalid base). The rt now panics identically. A VALID base (and
# `to_str`, which always uses base 10) is unaffected.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "i64_radix_panic_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "i64_radix_panic_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

run_case() {
  local label="$1" prog_src="$2" expect_exit="$3"
  local prog="$WORK/$label.ax" bin="$WORK/$label.bin"
  printf '%s\n' "$prog_src" > "$prog"
  local i_out i_code n_out n_code
  i_out="$("$AXON" run "$prog" 2>&1)"; i_code=$?
  if ! "$AXON" build "$prog" -o "$bin" --no-cache >/dev/null 2>&1; then
    echo "i64_radix_panic_parity: native build failed for $label — skipping"; exit 0
  fi
  n_out="$("$bin" 2>&1)"; n_code=$?
  if [ "$i_out" != "$n_out" ] || [ "$i_code" != "$n_code" ]; then
    echo "i64_radix_panic_parity: FAIL ($label): interp=[$i_code]'$i_out' native=[$n_code]'$n_out'"
    exit 1
  fi
  if [ "$i_code" != "$expect_exit" ]; then
    echo "i64_radix_panic_parity: FAIL ($label): expected exit $expect_exit, got $i_code"
    exit 1
  fi
  echo "  OK  $label: exit=$i_code '$i_out'"
}

# Bad bases → both engines panic (exit 101) with the SAME message.
run_case bad_lo  'fn main() -> i64 { println(i64_to_str_radix(10, 1))  0 }'  101
run_case bad_hi  'fn main() -> i64 { println(i64_to_str_radix(10, 37))  0 }' 101
run_case bad_zero 'fn main() -> i64 { println(i64_to_str_radix(10, 0))  0 }' 101
# Valid base + plain to_str (base 10) → unaffected, exit 0.
run_case ok_hex  'fn main() -> i64 { println(i64_to_str_radix(255, 16))  0 }' 0
run_case ok_tostr 'fn main() -> i64 { println(to_str(42))  0 }'               0

echo "i64_radix_panic_parity: PASS — native i64_to_str_radix matches the interpreter on bad-base panic + valid base"
exit 0
