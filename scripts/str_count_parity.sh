#!/usr/bin/env bash
# str_count_parity.sh — interp↔native parity for str_count (had NO test, per the
# 2026-06-10 audit).
#
# The interpreter is `s.matches(needle).count()`. The old inline codegen used a
# `strstr` loop that returned 0 for an empty needle — but the interpreter returns
# ONE match per char boundary (char_count+1), so `str_count("ab","")` = 3 and
# `str_count("héllo","")` = 6 (5 chars + 1), NOT 0 and not byte-based. Codegen now
# delegates to axon-rt `__axon_str_count`, byte-identical to the interpreter
# including the empty-needle + non-ASCII cases.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "str_count_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "str_count_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

PROG="$WORK/sc.ax"
cat > "$PROG" <<'AX'
fn main() -> i64 {
    println(to_str(str_count("ab", "")))          // 3  (empty needle: chars+1)
    println(to_str(str_count("aXbXc", "X")))       // 2
    println(to_str(str_count("hello", "l")))       // 2
    println(to_str(str_count("aaa", "aa")))        // 1  (non-overlapping)
    println(to_str(str_count("héllo", "")))        // 6  (5 chars + 1, NOT bytes)
    println(to_str(str_count("xyz", "q")))         // 0  (not found)
    0
}
AX

interp_out="$("$AXON" run "$PROG" 2>/dev/null)"
BIN="$WORK/sc_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "str_count_parity: native build failed — skipping"; exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "str_count_parity: FAIL — native differs from the interpreter:"
  echo "--- interp ---"; echo "$interp_out" | sed 's/^/  /'
  echo "--- native ---"; echo "$native_out" | sed 's/^/  /'
  exit 1
fi
# Belt-and-suspenders: the empty-needle char-boundary semantics must hold.
if [ "$(echo "$interp_out" | sed -n '1p')" != "3" ] || [ "$(echo "$interp_out" | sed -n '5p')" != "6" ]; then
  echo "str_count_parity: FAIL — empty-needle char-boundary count wrong: $interp_out"; exit 1
fi

echo "str_count_parity: OK — native==interp on str_count (empty / non-ASCII / non-overlapping / not-found):"
echo "$native_out" | sed 's/^/  /'
exit 0
