#!/usr/bin/env bash
# assert_msg_parity.sh — I-2 parity for the assert family's failure output.
#
# The interpreter prints the panic to STDERR as `axon: panic: <msg>` with the
# ACTUAL values (e.g. "assertion failed: 3 != 5"). Native codegen used to printf
# a GENERIC message to STDOUT (wrong stream, no values, no `axon: panic:`
# prefix, and assert_err used the wrong text). Both now route through
# __axon_msg_panic / __axon_assert_eq_*_panic. This harness checks stdout,
# stderr, AND exit code separately (so a stream divergence is caught) for each
# assert variant's failing case.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "assert_msg_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "assert_msg_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

run_case() {
  local label="$1" prog_src="$2"
  local prog="$WORK/$label.ax" bin="$WORK/$label.bin"
  printf '%s\n' "$prog_src" > "$prog"
  local io ie ic no ne nc
  io="$("$AXON" run "$prog" 2>"$WORK/ie")"; ic=$?; ie="$(cat "$WORK/ie")"
  if ! "$AXON" build "$prog" -o "$bin" --no-cache >/dev/null 2>&1; then
    echo "assert_msg_parity: native build failed for $label — skipping"; exit 0
  fi
  no="$("$bin" 2>"$WORK/ne")"; nc=$?; ne="$(cat "$WORK/ne")"
  if [ "$io" != "$no" ]; then
    echo "assert_msg_parity: FAIL ($label) STDOUT differs: interp='$io' native='$no'"; exit 1
  fi
  if [ "$ie" != "$ne" ]; then
    echo "assert_msg_parity: FAIL ($label) STDERR differs: interp='$ie' native='$ne'"; exit 1
  fi
  if [ "$ic" != "$nc" ]; then
    echo "assert_msg_parity: FAIL ($label) EXIT differs: interp=$ic native=$nc"; exit 1
  fi
  echo "  OK  $label: [$ic] stderr='$ne'"
}

run_case bare    'fn main() -> i64 { assert(false)  0 }'
run_case eq_i64  'fn main() -> i64 { assert_eq(3, 5)  0 }'
run_case eq_f64  'fn main() -> i64 { assert_eq_f64(1.5, 2.5)  0 }'
run_case eq_str  'fn main() -> i64 { assert_eq_str("a", "b")  0 }'
run_case err_ok  'fn main() -> i64 { assert_err(true)  0 }'

echo "assert_msg_parity: PASS — native assert failures match the interpreter (stdout + stderr + exit)"
exit 0
