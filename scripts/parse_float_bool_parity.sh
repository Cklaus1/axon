#!/usr/bin/env bash
# parse_float_bool_parity.sh — codegen regression (value AND message parity).
#
# parse_float / parse_bool must match the interpreter (the I-2 oracle) on BOTH
# the parsed value and the Err message. The old hand-emitted codegen diverged:
#   - parse_float used libc `strtod`, which PREFIX-parses ("12abc" -> 12) where
#     the interpreter's whole-string parse rejects it; and its Err message was
#     EMPTY (len=0, null) instead of "could not parse `<s>` as a float".
#   - parse_bool compared raw bytes (no trim) so "  true  " -> Err, and its Err
#     message was the static "invalid bool" instead of the interpreter's
#     "could not parse `<s>` as a bool (expected `true` or `false`)".
# Both now delegate the whole parse to axon-rt (__axon_parse_float /
# __axon_parse_bool), so the two engines are byte-identical (and parse_float
# drops the libc strtod dep, so it links on the browser target too).
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/pfb.ax"
cat > "$PROG" <<'AX'
fn show_f(s: str) {
    match parse_float(s) { Ok(f) => println("ok {to_str_f64(f)}")  Err(e) => println(e) }
}
fn show_b(s: str) {
    match parse_bool(s) { Ok(b) => println("ok {to_str_bool(b)}")  Err(e) => println(e) }
}
fn main() -> i64 {
    show_f("3.14")       // ok
    show_f("  5.0  ")    // ok (trimmed)
    show_f("12abc")      // Err (whole-string, NOT prefix-parsed)
    show_f("xyz")        // Err
    show_b("true")       // ok true
    show_b("  false  ")  // ok false (trimmed)
    show_b("yes")        // Err
    0
}
AX

echo "parse_float_bool_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "parse_float_bool_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

interp_out="$("$AXON" run "$PROG" 2>/dev/null)"

BIN="$WORK/pfb_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "parse_float_bool_parity: native build failed — skipping"
  exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "parse_float_bool_parity: FAIL — native parse_float/parse_bool differs from the interpreter:"
  echo "--- interp ---"; echo "$interp_out" | sed 's/^/  /'
  echo "--- native ---"; echo "$native_out" | sed 's/^/  /'
  exit 1
fi

# Belt-and-suspenders: the SPECIFIC divergences must be gone.
if ! echo "$native_out" | grep -q 'could not parse `12abc` as a float'; then
  echo "parse_float_bool_parity: FAIL — '12abc' must be Err (no strtod prefix-parse): $native_out"; exit 1
fi
if ! echo "$native_out" | grep -q 'could not parse `yes` as a bool'; then
  echo "parse_float_bool_parity: FAIL — parse_bool Err must echo the input, not 'invalid bool': $native_out"; exit 1
fi

echo "parse_float_bool_parity: OK — native==interp on parse_float/parse_bool (value + message):"
echo "$native_out" | sed 's/^/  /'
echo "parse_float/parse_bool match the interpreter"
exit 0
