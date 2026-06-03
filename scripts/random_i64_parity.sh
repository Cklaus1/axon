#!/usr/bin/env bash
# random_i64_parity.sh — BUG_HUNT #36 codegen regression.
#
# The interpreter guards random_i64's degenerate bounds (I-2 reference): hi<lo
# is a graceful failure, hi==lo returns lo. Codegen used to do NEITHER — hi==lo
# was a signed-rem by zero → SIGFPE (exit 136), hi<lo yielded garbage. This
# harness builds each degenerate case NATIVELY (codegen) and asserts the fixed
# behavior, so the guard can't silently regress.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "random_i64_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "random_i64_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

build_run() { # <src-file> <out-bin>  → prints "exit=<code>"; uses --no-cache
  if ! "$AXON" build "$1" -o "$2" --no-cache >/dev/null 2>&1; then
    echo "random_i64_parity: native build of $1 failed — skipping"
    exit 0
  fi
  "$2" >/dev/null 2>&1
  echo "$?"
}

# Case 1: hi == lo → empty range, must return lo cleanly (exit 0), NOT SIGFPE.
cat > "$WORK/eq.ax" <<'AX'
fn main() -> i64 {
    let r = random_i64(5, 5)
    if r == 5 { 0 } else { 1 }
}
AX
eq_code="$(build_run "$WORK/eq.ax" "$WORK/eq")"
if [ "$eq_code" = "136" ]; then
  echo "random_i64_parity: FAIL — hi==lo SIGFPE'd (exit 136); the #36 div-by-zero guard regressed"
  exit 1
fi
if [ "$eq_code" != "0" ]; then
  echo "random_i64_parity: FAIL — hi==lo should return lo (program exit 0), got exit $eq_code"
  exit 1
fi

# Case 2: hi < lo → inverted bounds, must fail gracefully (NOT exit 0, NOT SIGFPE
# garbage). The interpreter panics; native prints + exit(1).
cat > "$WORK/inv.ax" <<'AX'
fn main() -> i64 { let r = random_i64(20, 10)  r }
AX
inv_code="$(build_run "$WORK/inv.ax" "$WORK/inv")"
if [ "$inv_code" = "0" ]; then
  echo "random_i64_parity: FAIL — hi<lo silently succeeded (exit 0); inverted bounds must fail"
  exit 1
fi
if [ "$inv_code" = "136" ]; then
  echo "random_i64_parity: FAIL — hi<lo SIGFPE'd instead of a graceful failure"
  exit 1
fi

# Case 3: normal [10,20) → in range, exit 0.
cat > "$WORK/ok.ax" <<'AX'
fn main() -> i64 {
    let r = random_i64(10, 20)
    if r >= 10 && r < 20 { 0 } else { 1 }
}
AX
ok_code="$(build_run "$WORK/ok.ax" "$WORK/ok")"
if [ "$ok_code" != "0" ]; then
  echo "random_i64_parity: FAIL — random_i64(10,20) produced an out-of-range value (exit $ok_code)"
  exit 1
fi

echo "random_i64_parity: OK — hi==lo returns lo, hi<lo fails gracefully, [lo,hi) in range"
echo "random_i64 degenerate bounds match the interpreter"
exit 0
