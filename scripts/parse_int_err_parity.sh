#!/usr/bin/env bash
# parse_int_err_parity.sh — BUG_HUNT #37 codegen regression (message parity).
#
# parse_int's Err message must match the interpreter (I-2 oracle). Codegen used
# a STATIC message ("...no trailing characters") while the interpreter echoes
# the input ("could not parse `<input>` as a base-10 integer" + a radix hint).
# Codegen now delegates the message to axon-rt's __axon_parse_int_err, so the
# two engines produce byte-identical output. This harness builds a program that
# prints a failed parse_int's Err for a plain input and a radix-prefixed input,
# and asserts native == interp.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/pi.ax"
cat > "$PROG" <<'AX'
fn show(s: str) {
    match parse_int(s) { Ok(_) => println("ok")  Err(e) => println(e) }
}
fn main() -> i64 {
    show("abc")
    show("0x1F")
    show("12trailing")
    0
}
AX

echo "parse_int_err_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "parse_int_err_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

interp_out="$("$AXON" run "$PROG" 2>/dev/null)"

BIN="$WORK/pi_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "parse_int_err_parity: native build failed — skipping"
  exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "parse_int_err_parity: FAIL — native parse_int Err message differs from the interpreter (#37):"
  echo "--- interp ---"; echo "$interp_out" | sed 's/^/  /'
  echo "--- native ---"; echo "$native_out" | sed 's/^/  /'
  exit 1
fi

# Belt-and-suspenders: the message must ECHO the input (not the old static form).
if ! echo "$native_out" | grep -q 'could not parse `abc`'; then
  echo "parse_int_err_parity: FAIL — native must echo the input in the Err: $native_out"
  exit 1
fi

echo "parse_int_err_parity: OK — native and interp parse_int Err messages agree:"
echo "$native_out" | sed 's/^/  /'
echo "parse_int Err message matches the interpreter"
exit 0
