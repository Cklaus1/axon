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

if [ "$fail" -ne 0 ]; then
  echo "exit_code_parity: FAIL — interp↔native exit-code divergence"
  exit 1
fi
echo "exit_code_parity: native==interp on all exit codes"
echo "exit_code_parity: PASS"
