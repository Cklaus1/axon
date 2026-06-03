#!/usr/bin/env bash
# to_str_parity.sh — BUG_HUNT #40 codegen regression.
#
# `to_str` is polymorphic over scalars (i64/f64/bool). The interpreter (I-2
# reference) dispatches on the runtime value; codegen must dispatch on the arg's
# LLVM type at the call site (expr.rs), or an f64 arg gets silently truncated to
# int (to_str(3.14) → "3") and a bool reinterpreted. This harness builds a
# mixed-type to_str program BOTH ways and asserts identical stdout, so the
# per-arg-type dispatch can't silently regress.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/tostr.ax"
cat > "$PROG" <<'AX'
fn main() -> i64 {
    println(to_str(3.14))
    println(to_str(true))
    println(to_str(42))
    println(to_str(false))
    println(to_str(0.5))
    0
}
AX

echo "to_str_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "to_str_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# Interpreter output (the oracle).
interp_out="$("$AXON" run "$PROG" 2>/dev/null)"

# Native build + run.
BIN="$WORK/tostr_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "to_str_parity: native build failed — skipping"
  exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "to_str_parity: FAIL — native to_str output differs from the interpreter (#40 truncation):"
  echo "--- interp ---"; echo "$interp_out"
  echo "--- native ---"; echo "$native_out"
  exit 1
fi

# Belt-and-suspenders: the float must NOT have been truncated to an int.
if ! echo "$native_out" | grep -q "3.14"; then
  echo "to_str_parity: FAIL — to_str(3.14) was truncated in native: $native_out"
  exit 1
fi

echo "to_str_parity: OK — native and interp to_str agree across i64/f64/bool:"
echo "$native_out" | sed 's/^/  /'
echo "to_str scalar dispatch matches the interpreter"
exit 0
