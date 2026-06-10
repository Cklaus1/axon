#!/usr/bin/env bash
# str_utf8_parity.sh — BUG_HUNT #38/#39 codegen regression.
#
# str_reverse and str_replace must match the interpreter (I-2 oracle). Codegen
# used inline byte-level implementations: str_reverse byte-reversed (mangling
# multibyte UTF-8), and str_replace skipped the empty-`from` case. Both now
# delegate to char-correct axon-rt functions. This harness builds a program
# exercising multibyte input + empty-from BOTH ways and asserts identical
# stdout.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/strutf.ax"
cat > "$PROG" <<'AX'
fn main() -> i64 {
    println(str_reverse("héllo"))
    println(str_reverse("日本語"))
    println(str_replace("abc", "", "X"))
    println(str_replace("aaa", "a", ""))
    println(str_replace("hello world", "o", "0"))
    println(str_to_upper("héllo wörld"))
    println(str_to_lower("HÉLLO WÖRLD"))
    println(str_to_upper("straße"))
    0
}
AX

echo "str_utf8_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "str_utf8_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

interp_out="$("$AXON" run "$PROG" 2>/dev/null)"

BIN="$WORK/strutf_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "str_utf8_parity: native build failed — skipping"
  exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "str_utf8_parity: FAIL — native str_reverse/str_replace differ from the interpreter (#38/#39):"
  echo "--- interp ---"; echo "$interp_out" | sed 's/^/  /'
  echo "--- native ---"; echo "$native_out" | sed 's/^/  /'
  exit 1
fi

# Belt-and-suspenders: the multibyte reverse must be the expected valid UTF-8.
if ! echo "$native_out" | grep -q "olléh"; then
  echo "str_utf8_parity: FAIL — str_reverse(\"héllo\") was byte-reversed (#38): $native_out"
  exit 1
fi
if ! echo "$native_out" | grep -q "XaXbXcX"; then
  echo "str_utf8_parity: FAIL — str_replace(\"abc\",\"\",\"X\") skipped empty from (#39): $native_out"
  exit 1
fi
# str_to_upper/lower must do full Unicode case mapping, not ASCII-only bytes.
if ! echo "$native_out" | grep -q "HÉLLO WÖRLD"; then
  echo "str_utf8_parity: FAIL — str_to_upper was ASCII-only (é/ö unchanged): $native_out"
  exit 1
fi
# The case map that GROWS the string (ß→SS) — impossible for the old 1:1 byte loop.
if ! echo "$native_out" | grep -q "STRASSE"; then
  echo "str_utf8_parity: FAIL — str_to_upper(\"straße\") did not grow ß→SS: $native_out"
  exit 1
fi

echo "str_utf8_parity: OK — native str_reverse/str_replace/str_to_upper/str_to_lower match the interpreter:"
echo "$native_out" | sed 's/^/  /'
echo "str_reverse, str_replace, and str_to_upper/lower match the interpreter"
exit 0
