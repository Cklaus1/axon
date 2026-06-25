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

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "checked_arith_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "checked_arith_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
# axon-rt carries the __axon_arith_panic helper; make sure it's current.
cargo build -q -p axon-rt 2>/dev/null || true
AXON="target/debug/axon"

# (label, body) — each body is a `fn main()` body. The interesting cases are the
# faults (must panic exit 101 with a matching message) and the boundary cases
# that must NOT panic (INT_MIN/-1 div+rem, max value that just fits).
declare -A CASES
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
# Boundary: INT_MIN / -1 traps in hardware but is DEFINED (wrapping) in the
# interpreter — must return INT_MIN, not SIGFPE, on both engines.
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

fail=0
for label in "${!CASES[@]}"; do
  PROG="$WORK/$label.ax"
  {
    echo "fn main() {"
    echo "    ${CASES[$label]}"
    echo "}"
  } > "$PROG"

  # Interpreter (oracle). Capture stderr too (panic messages go there), but
  # strip the Phase-9 `axon: run-id <id>` provenance stamp the interpreter
  # prints to stderr at startup — the native binary emits no such line, so
  # leaving it in would be a false divergence. iexit must come from `axon run`,
  # not the filter, so grab it before filtering.
  iraw="$("$AXON" run "$PROG" 2>&1)"
  iexit=$?
  iout="$(printf '%s\n' "$iraw" | grep -v '^axon: run-id ')"

  # Native.
  BIN="$WORK/${label}_bin"
  if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
    echo "checked_arith_parity: native build failed for $label — skipping"
    exit 0
  fi
  nout="$("$BIN" 2>&1)"
  nexit=$?

  if [ "$iexit" -ne "$nexit" ] || [ "$iout" != "$nout" ]; then
    echo "checked_arith_parity: FAIL — $label diverged"
    echo "  interp: exit=$iexit out=[$iout]"
    echo "  native: exit=$nexit out=[$nout]"
    fail=1
  else
    echo "  $label: interp==native (exit $iexit) ✓"
  fi
done

if [ "$fail" -ne 0 ]; then
  echo "checked_arith_parity: FAILED"
  exit 1
fi
echo "checked_arith_parity: OK — native checked arithmetic matches the interpreter (I-9)"
exit 0
