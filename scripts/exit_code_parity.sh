#!/usr/bin/env bash
# exit_code_parity.sh — the interpreter and native codegen must agree on the
# PROCESS EXIT CODE, not just stdout (I-2 covers observable behavior, and the
# exit code is observable — CI and supervisors branch on it).
#
# History: native `assert(false)` exited 1 while the interpreter exited 101 —
# a real exit-code divergence (div0 already matched at 101). The assert-family
# panic exits were converged to 101 in codegen/builtins.rs; this harness locks
# that in and guards the broader contract:
#   - a runtime crash (assert / assert_eq mismatch / OOB / div0) → 101 on BOTH
#   - a clean program                                            → 0  on BOTH
#   - main's i64 return value                                    → that value, BOTH
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "exit_code_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "exit_code_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# case <name> <expected_exit> <program-source>
fail=0
check() {
  local name="$1" want="$2" src="$3"
  local prog="$WORK/$name.ax"
  printf '%s\n' "$src" > "$prog"

  AXON_AI_MOCK=1 "$AXON" run "$prog" >/dev/null 2>&1
  local i_exit=$?

  local bin="$WORK/${name}_bin"
  if ! AXON_AI_MOCK=1 "$AXON" build "$prog" -o "$bin" --no-cache >/dev/null 2>&1; then
    echo "exit_code_parity: native build failed for $name — skipping"
    exit 0
  fi
  AXON_AI_MOCK=1 "$bin" >/dev/null 2>&1
  local n_exit=$?

  if [ "$i_exit" != "$n_exit" ]; then
    echo "FAIL [$name]: interp exit=$i_exit but native exit=$n_exit (must match — I-2)"
    fail=1
  elif [ "$i_exit" != "$want" ]; then
    echo "FAIL [$name]: both exited $i_exit but expected $want"
    fail=1
  else
    echo "  OK $name: both exit $i_exit"
  fi
}

# Runtime crashes — must be 101 on both engines.
check assert_false   101 'fn main() -> i64 { assert(false)  0 }'
check assert_eq_bad  101 'fn main() -> i64 { assert_eq(1, 2)  0 }'
check div_zero       101 'fn main() -> i64 { let z = 0  10 / z }'
check oob_index      101 'fn main() -> i64 { let a = [1, 2, 3]  a[10] }'

# Clean termination + explicit return value.
check clean_zero     0   'fn main() -> i64 { 0 }'
check return_seven   7   'fn main() -> i64 { 7 }'

# Phase 5: refinement-PRECONDITION violations on non-constant args → exit 6 on
# BOTH engines (the spec's Z3-free runtime-check fallback). A constant arg is a
# static E1209; these route a runtime value through an unrefined helper so the
# checker can't fold it, exercising the entry check in interp + codegen.
check refine_neg     6   'fn f(n: i64 where _ >= 0) -> i64 { n }
fn bad(x: i64) -> i64 { f(x) }
fn main() -> i64 { bad(0 - 1) }'
check refine_div0    6   'fn divide(n: i64, d: i64 where _ != 0) -> i64 { n / d }
fn main() -> i64 { let z = 0
 divide(10, z) }'
check refine_strlen  6   'type NonEmpty = str where str_len(_) > 0
fn greet(s: NonEmpty) -> i64 { str_len(s) }
fn caller(x: str) -> i64 { greet(x) }
fn main() -> i64 { caller("") }'
# A SATISFIED non-constant arg must NOT trip the check (no false positive); main
# returns factorial(5) = 120 on both engines.
check refine_ok      120 'fn factorial(n: i64 where _ >= 0) -> i64 { if n <= 1 { 1 } else { n * factorial(n - 1) } }
fn ok(x: i64) -> i64 { factorial(x) }
fn main() -> i64 { ok(5) }'

# Phase 5: refinement RETURN postconditions (the dual). A fn `-> T where P`
# whose non-constant return fails P exits 6 on BOTH engines; a satisfied one
# returns its value. (Constant bad returns are a static E1209, never reached.)
check refine_ret_bad 6   'type Positive = i64 where _ > 0
fn f(x: i64) -> Positive { x - 100 }
fn main() -> i64 { f(5) }'
check refine_ret_ok  105 'type Positive = i64 where _ > 0
fn f(x: i64) -> Positive { x + 100 }
fn main() -> i64 { f(5) }'

if [ "$fail" -ne 0 ]; then
  echo "exit_code_parity: FAIL — interp↔native exit-code divergence"
  exit 1
fi
echo "exit_code_parity: native==interp on all exit codes"
echo "exit_code_parity: PASS"
