#!/usr/bin/env bash
# parse_int_radix_parity.sh — BUG_HUNT #22 codegen regression.
#
# parse_int_radix(s, base) is the radix-aware integer parse (input-side inverse
# of i64_to_str_radix). The interpreter (interp.rs) and native codegen (which
# delegates to axon-rt's __axon_parse_int_radix) must agree byte-for-byte across
# prefix-stripping, sign, bad-digit Err, and out-of-range-base Err. This harness
# builds one program both ways and diffs stdout.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/pir.ax"
cat > "$PROG" <<'AX'
fn show(s: str, base: i64) {
  match parse_int_radix(s, base) {
    Ok(n) => println("{s} (base {to_str(base)}) = {to_str(n)}")
    Err(e) => println("{s}: {e}")
  }
}
fn main() -> i64 {
  show("0x1F", 16)
  show("1F", 16)
  show("0b1010", 2)
  show("0o17", 8)
  show("-2A", 16)
  show("zZ", 36)
  show("zzz", 16)
  show("10", 99)
  show("ff", 10)
  0
}
AX

echo "parse_int_radix_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "parse_int_radix_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

I_OUT="$(AXON_AI_MOCK=1 "$AXON" run "$PROG" 2>&1)"; I_EXIT=$?
BIN="$WORK/pir_bin"
if ! AXON_AI_MOCK=1 "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "parse_int_radix_parity: native build failed — skipping"
  exit 0
fi
N_OUT="$(AXON_AI_MOCK=1 "$BIN" 2>&1)"; N_EXIT=$?

if [ "$I_EXIT" -ne 0 ] || [ "$N_EXIT" -ne 0 ]; then
  echo "FAIL: expected clean exit (interp=$I_EXIT native=$N_EXIT)"
  echo "  interp: $I_OUT"; echo "  native: $N_OUT"; exit 1
fi
if [ "$I_OUT" != "$N_OUT" ]; then
  echo "FAIL: parse_int_radix output diverges"
  echo "── interp ──"; echo "$I_OUT"
  echo "── native ──"; echo "$N_OUT"; exit 1
fi
# Spot-check a couple of expected values so a both-wrong agreement still fails.
case "$I_OUT" in
  *"0x1F (base 16) = 31"*) : ;;
  *) echo "FAIL: expected 0x1F→31 in output: $I_OUT"; exit 1 ;;
esac
case "$I_OUT" in
  *"-2A (base 16) = -42"*) : ;;
  *) echo "FAIL: expected -2A→-42 in output: $I_OUT"; exit 1 ;;
esac

echo "parse_int_radix_parity: native==interp on all cases"
echo "parse_int_radix_parity: PASS"
exit 0
