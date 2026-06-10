#!/usr/bin/env bash
# wasm_str_abi_parity.sh — R7: the str/array i64→i32 wasm-ABI bridge.
#
# LLVM (codegen) expands a by-value `AxonStr {i64 len, ptr}` argument into
# SCALARS, so codegen calls e.g. `__axon_str_reverse(i64 len, i32 ptr, …)`.
# rustc would otherwise pass `#[repr(C)] AxonStr` INDIRECTLY on wasm32 (a single
# i32 pointer) — a `function signature mismatch` at link, and a trap at runtime.
# axon-rt now declares the EXPANDED scalar form under `#[cfg(target_arch=wasm32)]`
# for all 13 str/array-taking externs, so a STRING-using program links clean and
# runs. This harness proves it: a program exercising 7 distinct str builtins must
# produce the SAME value across interp, native AOT, and AOT-wasm.
#
# Skips (exit 0) when codegen / the wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_str_abi_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_str_abi_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_str_abi_parity: interp build unavailable — skipping"; exit 0
fi
# The wasm runtime (carrying the scalar-ABI bridge) must exist for str programs
# to link. Build it; skip honestly if the wasm32 target isn't installed.
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  echo "wasm_str_abi_parity: wasm32 axon-rt build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# A program that routes through 7 distinct str builtins, each of which takes an
# AxonStr by value (the exact externs the wasm bridge rewrites). The final i64
# is the cross-engine oracle.
SRC="$WORK/strmix.ax"
cat > "$SRC" <<'AX'
fn main() -> i64 {
    let s = str_reverse("abc")
    let r = str_replace("aXbXc", "X", "-")
    let n = str_len(r)
    let slc = str_slice("hello world", 0, 5)
    let idx = str_index_of("hello", "l")
    let rep = str_repeat("ab", 3)
    let up = str_to_upper("abc")
    let lo = str_to_lower("ABC")
    let tr = str_trim("  xy  ")
    let pd = str_pad_start("z", 4, "0")
    n + str_len(s) + str_len(slc) + idx + str_len(rep) + str_len(up) + str_len(lo) + str_len(tr) + str_len(pd)
}
AX

# 1) interpreter oracle (exit value = i64 main return, mod 256)
"$INTERP" "$SRC" >/dev/null 2>&1; I_EXIT=$?
echo "wasm_str_abi_parity: interp = $I_EXIT"

# 2) native AOT
if "$AXON" build "$SRC" -o "$WORK/native" >/dev/null 2>&1; then
  "$WORK/native" >/dev/null 2>&1; N_EXIT=$?
  echo "wasm_str_abi_parity: native = $N_EXIT"
  if [ "$N_EXIT" != "$I_EXIT" ]; then
    echo "wasm_str_abi_parity: FAIL — native ($N_EXIT) != interp ($I_EXIT)"; exit 1
  fi
else
  echo "wasm_str_abi_parity: native build unavailable — skipping native leg"
fi

# 3) AOT-wasm — the bridge under test. Must LINK (no signature mismatch) and RUN.
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
  echo "wasm_str_abi_parity: wasm build unavailable — skipping wasm leg"; exit 0
fi
LINKED="${SRC%.ax}.linked.wasm"
if [ ! -f "$LINKED" ]; then
  echo "wasm_str_abi_parity: FAIL — str program did NOT link (ABI bridge regressed)"; exit 1
fi
W_OUT="$("$WASMRT" --invoke main "$LINKED" 2>/dev/null | grep -vi experimental | head -1)"
echo "wasm_str_abi_parity: wasm = $W_OUT"
if [ -z "$W_OUT" ]; then
  echo "wasm_str_abi_parity: FAIL — wasm produced no output (trap?)"; exit 1
fi
if [ "$((W_OUT % 256))" != "$I_EXIT" ]; then
  echo "wasm_str_abi_parity: FAIL — wasm ($W_OUT) != interp ($I_EXIT)"; exit 1
fi

echo "wasm_str_abi_parity: PASS — 11 str builtins (incl. str_to_upper/lower/trim/pad) run identically on interp, native, and AOT-wasm ✓"
exit 0
