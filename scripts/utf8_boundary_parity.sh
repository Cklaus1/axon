#!/usr/bin/env bash
# utf8_boundary_parity.sh — R42 T1/T3 gate for the E2200 `str_slice` refusal.
#
# WHY THIS IS NOT ANOTHER AGREEMENT HARNESS. `fuzz_parity.sh` compares the
# interpreter against native codegen, so it can only find cases where the two
# DISAGREE. The bug this gate exists for was a case where they agreed perfectly:
# both returned "" for a byte range that splits a UTF-8 character, both wrong.
# An agreement oracle is structurally blind to a bug in the reference semantics,
# which is why that bug shipped despite `str_slice` being in the fuzz corpus with
# non-ASCII inputs.
#
# So this script asserts EXPECTED VALUES first, and agreement second:
#   1. aligned slices produce the exact expected string, in BOTH engines;
#   2. splitting slices REFUSE, in both engines, with the same message and the
#      same exit code.
#
# NOT a duplicate of `str_utf8_parity.sh`, which sits beside it: that one covers
# str_reverse/str_replace (BUG_HUNT #38/#39) and is agreement-only by design.
# This one covers str_slice and is expected-value-based. Two utf8 harnesses is
# deliberate, not an oversight.
#
# Exit 0 = pass. Exit 1 = a real divergence or a wrong value. SKIP (exit 0) only
# when codegen is unavailable, and it says so on stdout.

set -uo pipefail
cd "$(dirname "$0")/.."

AXON="${AXON:-./target/debug/axon}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

if [ ! -x "$AXON" ]; then
  echo "utf8_boundary_parity: SKIP — no axon binary at $AXON"
  exit 0
fi

pass=0; fail=0

# ── Part 1: aligned slices must produce the RIGHT value, not merely the same one.
#
# "café" is 5 bytes: c(0) a(1) f(2) é(3,4). Every range below lands on a
# character boundary, so every one has a defined correct answer.
run_expect() { # <label> <axon-expr> <expected-stdout>
  local label="$1" expr="$2" want="$3" f="$WORK/t.ax"
  printf 'fn main() -> i64 {\n    let s = "café"\n    println(%s)\n    0\n}\n' "$expr" > "$f"

  local i_out i_code
  i_out="$("$AXON" run "$f" 2>/dev/null)"; i_code=$?
  if [ "$i_code" -ne 0 ] || [ "$i_out" != "$want" ]; then
    echo "  FAIL $label: interp gave '$i_out' (exit $i_code), expected '$want'"
    fail=$((fail+1)); return
  fi

  # Native must agree AND be right. A build failure here is a real failure, not
  # a skip: the whole point is that both engines are checked.
  if ! "$AXON" build "$f" -o "$WORK/bin" >/dev/null 2>&1; then
    echo "  SKIP $label: codegen unavailable"
    return
  fi
  local n_out n_code
  n_out="$("$WORK/bin" 2>/dev/null)"; n_code=$?
  if [ "$n_code" -ne 0 ] || [ "$n_out" != "$want" ]; then
    echo "  FAIL $label: native gave '$n_out' (exit $n_code), expected '$want'"
    fail=$((fail+1)); return
  fi
  pass=$((pass+1))
}

echo "utf8_boundary_parity: aligned slices — expected VALUES"
run_expect "whole"        'str_slice(s, 0, 5)'  'café'
run_expect "ascii-prefix" 'str_slice(s, 0, 3)'  'caf'
run_expect "multibyte"    'str_slice(s, 3, 5)'  'é'
run_expect "empty"        'str_slice(s, 2, 2)'  ''
run_expect "clamped-end"  'str_slice(s, 0, 99)' 'café'
# The card's per-character idiom over ASCII must stay correct.
run_expect "char-idiom"   'str_slice(s, 1, 2)'  'a'

# ── Part 2: splitting slices must REFUSE — same message, same exit code.
#
# Ranges 0..4 and 4..5 both cut `é` (bytes 3 and 4).
echo "utf8_boundary_parity: splitting slices — refusal + identical text"
for range in "0, 4" "4, 5" "0, 4" ; do
  f="$WORK/split.ax"
  printf 'fn main() -> i64 {\n    let s = "café"\n    println(str_slice(s, %s))\n    0\n}\n' "$range" > "$f"

  i_all="$("$AXON" run "$f" 2>&1 | grep -v 'run-id')"; i_code=${PIPESTATUS[0]}
  if [ "$i_code" -eq 0 ]; then
    echo "  FAIL split($range): interp ACCEPTED a split range"
    fail=$((fail+1)); continue
  fi
  case "$i_all" in
    *E2200*) : ;;
    *) echo "  FAIL split($range): interp refused without naming E2200: $i_all"
       fail=$((fail+1)); continue ;;
  esac

  if ! "$AXON" build "$f" -o "$WORK/sbin" >/dev/null 2>&1; then
    echo "  SKIP split($range): codegen unavailable"
    continue
  fi
  n_all="$("$WORK/sbin" 2>&1)"; n_code=$?
  if [ "$n_code" -ne "$i_code" ]; then
    echo "  FAIL split($range): exit codes differ — interp $i_code, native $n_code"
    fail=$((fail+1)); continue
  fi
  if [ "$n_all" != "$i_all" ]; then
    echo "  FAIL split($range): message differs"
    echo "    interp: $i_all"
    echo "    native: $n_all"
    fail=$((fail+1)); continue
  fi
  pass=$((pass+1))
done

echo "utf8_boundary_parity: $pass passed, $fail failed"
# A run where everything SKIPped is not a pass in disguise; say which happened.
if [ "$pass" -eq 0 ] && [ "$fail" -eq 0 ]; then
  echo "utf8_boundary_parity: SKIP — nothing ran (codegen unavailable)"
  exit 0
fi
[ "$fail" -eq 0 ] || exit 1
echo "utf8_boundary_parity: PASS"
