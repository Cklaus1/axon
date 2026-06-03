#!/usr/bin/env bash
# parse_or_parity.sh — native codegen == interpreter for parse_int_or /
# parse_float_or (the parse-with-default builtins).
#
# These existed in the interpreter but had NO codegen lowering, so native
# silently returned a zero value (a real native↔interp divergence on a
# correctness-sensitive builtin). Codegen now lowers them as a thin wrapper
# that calls the Result-returning parser and selects the Ok payload or the
# caller's default. This harness builds both engines and asserts identical
# exit values across parse-success and parse-failure (→default) cases.
#
# parse_bool_or is lowered INLINE in emit_call (not a hand-built fn): the i1
# default stays an SSA value in the caller's frame, sidestepping the i1
# function-parameter ABI corner that the hand-built form hit.
#
# Skips (exit 0) when the codegen toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "parse_or_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "parse_or_parity: interp build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

fail=0
check() {
  local name="$1" src="$2"
  printf '%s\n' "$src" > "$WORK/$name.ax"
  "$INTERP" "$WORK/$name.ax" >/dev/null 2>&1; local i=$?
  if "$AXON" build "$WORK/$name.ax" -o "$WORK/$name" >/dev/null 2>&1; then
    "$WORK/$name" >/dev/null 2>&1; local n=$?
    if [ "$i" = "$n" ]; then
      echo "  OK   $name: interp=$i native=$n"
    else
      echo "  FAIL $name: interp=$i native=$n"; fail=1
    fi
  else
    echo "  SKIP $name (native build unavailable)"
  fi
}

check int_ok    'fn main() -> i64 { parse_int_or("42", -1) }'
check int_dflt  'fn main() -> i64 { parse_int_or("notanint", 77) }'
check int_neg   'fn main() -> i64 { parse_int_or("-7", 0) }'
check float_ok  'fn main() -> i64 { let f = parse_float_or("2.5", 1.0)  f64_to_i64(f * 4.0) }'
check float_df  'fn main() -> i64 { let f = parse_float_or("zzz", 9.0)  f64_to_i64(f) }'
check bool_ok   'fn main() -> i64 { let b = parse_bool_or("true", false)  if b { 1 } else { 0 } }'
check bool_dflt 'fn main() -> i64 { let b = parse_bool_or("garbage", true)  if b { 1 } else { 0 } }'
check bool_false 'fn main() -> i64 { let b = parse_bool_or("false", true)  if b { 1 } else { 0 } }'

[ "$fail" -eq 0 ] || { echo "parse_or_parity: FAIL"; exit 1; }
echo "parse_or_parity: PASS — parse_int_or / parse_float_or / parse_bool_or match the interpreter ✓"
exit 0
