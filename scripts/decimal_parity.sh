#!/usr/bin/env bash
# decimal_parity.sh — R21 Decimal contract harness.
#
# Decimal (exact i128 fixed-point money) is INTERP-ONLY in this slice: native
# codegen E0910-REFUSES any function that touches Decimal rather than emit
# subtly-wrong money IR (sound-by-refusal, I-2). This harness locks in:
#   1. INTERP correctness: the exact-arithmetic contracts hold under `axon run`
#      (0.1d + 0.2d == 0.3d, overflow→graceful panic, overdraft→exit 6).
#   2. CODEGEN refusal: `axon build` on a Decimal program reports E0910 and does
#      NOT produce a binary (the honest gate — no silent miscompile).
#
# The codegen half SKIPS cleanly when LLVM is unavailable, so this is safe in
# interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

fail=0

echo "decimal_parity: building interpreter axon binary…"
if ! cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null; then
  echo "decimal_parity: interpreter build FAILED"
  exit 1
fi
IAXON="target/debug/axon"

# ── 1. interpreter correctness ───────────────────────────────────────────────
# case <name> <expected_exit> <program>
icheck() {
  local name="$1" want="$2" src="$3"
  local prog="$WORK/$name.ax"
  printf '%s\n' "$src" > "$prog"
  AXON_AI_MOCK=1 "$IAXON" run "$prog" >/dev/null 2>&1
  local got=$?
  if [ "$got" != "$want" ]; then
    echo "  ✗ interp $name: expected exit $want, got $got"
    fail=1
  else
    echo "  ✓ interp $name (exit $got)"
  fi
}

echo "decimal_parity: interpreter contracts…"
# 0.1 + 0.2 == 0.3 exactly → assert passes → exit 0
icheck exact_sum 0 'fn main() { assert(0.1d + 0.2d == 0.3d) }'
# exact mul
icheck exact_mul 0 'fn main() { assert(19.99d * 3d == 59.97d) }'
# overflow → graceful panic (exit 101), NOT a silent wrap
icheck overflow_panic 101 'fn main() { let b = 100000000000000000000d  let _ = b * b }'
# division by zero → graceful panic
icheck div_zero 101 'fn main() { let _ = decimal_div(1d, 0d, "half_even") }'
# overdraft refinement `where _ >= 0d` → exit 6 (REFINE_VIOLATION)
icheck overdraft 6 'fn f(b: Decimal, a: Decimal) -> Decimal { let n: Decimal where _ >= 0d = b - a  n }
fn main() { let _ = f(100d, 150d) }'
# a legal balance passes the refinement → exit 0
icheck balance_ok 0 'fn f(b: Decimal, a: Decimal) -> Decimal { let n: Decimal where _ >= 0d = b - a  n }
fn main() { assert(f(100d, 30d) == 70d) }'

# ── 2. codegen refusal ───────────────────────────────────────────────────────
echo "decimal_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "decimal_parity: codegen build unavailable (LLVM absent) — skipping refusal check"
else
  CAXON="target/debug/axon"
  prog="$WORK/dec_codegen.ax"
  printf '%s\n' 'fn main() { assert(0.1d + 0.2d == 0.3d) }' > "$prog"
  out="$("$CAXON" build "$prog" 2>&1)"
  if printf '%s' "$out" | grep -q 'E0910'; then
    echo "  ✓ codegen refuses Decimal with E0910"
  else
    echo "  ✗ codegen did NOT refuse Decimal (expected E0910):"
    printf '%s\n' "$out" | sed 's/^/      /'
    fail=1
  fi
  # No binary should have been produced.
  if [ -f "${prog%.ax}" ] || [ -f "./dec_codegen" ]; then
    echo "  ✗ codegen produced a binary for a Decimal program (should have aborted)"
    rm -f "${prog%.ax}" ./dec_codegen
    fail=1
  fi
fi

if [ "$fail" = 0 ]; then
  echo "decimal_parity: PASS"
  exit 0
else
  echo "decimal_parity: FAIL"
  exit 1
fi
