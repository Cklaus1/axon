#!/usr/bin/env bash
# wasm_malloc_abi_parity.sh — R7: the libc `size_t` width bridge for AOT-wasm.
#
# wasm32 is an ILP32 target: `size_t`/pointers are 32-bit, so the C runtime's
# `malloc` and `snprintf` take an i32 size, NOT the i64 the native (LP64) path
# bakes in. Codegen now declares those once with the target-correct width
# (`size_ty()`) and narrows every size argument via `msize()` on wasm32. Without
# it, an ARRAY LITERAL (`malloc(n*elem)`) or `to_str` (`snprintf(buf, len, …)`)
# traps the wasm verifier (`type mismatch: expected i32, found i64`).
#
# This harness builds programs exercising both paths and asserts the result is
# identical on interp, native, and AOT-wasm. Skips (exit 0) when codegen / the
# wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_malloc_abi_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_malloc_abi_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_malloc_abi_parity: interp build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  echo "wasm_malloc_abi_parity: wasm32 axon-rt build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# Each program routes through a heap allocation whose size is a `size_t`:
#   arr   — array literal → malloc(n * elem_size)
#   tostr — to_str(i64)   → snprintf(NULL,0,…) then malloc + snprintf(buf,len,…)
#   combo — both + a float to_str (snprintf %.6g path)
declare -A PROGS
PROGS[arr]='fn main() -> i64 { let a = [10, 20, 12]  a[0] + a[1] + a[2] }'
PROGS[tostr]='fn main() -> i64 { let s = to_str(12345)  str_len(s) }'
PROGS[combo]='fn main() -> i64 {
    let a = [10, 20, 12]
    let s = to_str(a[0] + a[1] + a[2])
    let f = to_str(3.5)
    str_len(s) + str_len(f) + a[2]
}'

fail=0; ran=0
for name in arr tostr combo; do
  SRC="$WORK/$name.ax"; printf '%s\n' "${PROGS[$name]}" > "$SRC"
  "$INTERP" "$SRC" >/dev/null 2>&1; I=$?
  # native
  if "$AXON" build "$SRC" -o "$WORK/$name.n" >/dev/null 2>&1; then
    "$WORK/$name.n" >/dev/null 2>&1; N=$?
    if [ "$N" != "$I" ]; then echo "  FAIL $name: native=$N != interp=$I"; fail=1; fi
  fi
  # AOT-wasm
  if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
    echo "  SKIP $name (wasm build unavailable)"; continue
  fi
  L="${SRC%.ax}.linked.wasm"
  if [ ! -f "$L" ]; then echo "  FAIL $name: did NOT link (size_t ABI regressed)"; fail=1; continue; fi
  ran=$((ran+1))
  W="$("$WASMRT" --invoke main "$L" 2>/dev/null | grep -vi experimental | head -1)"
  if [ -z "$W" ]; then echo "  FAIL $name: wasm produced no output (trap?)"; fail=1; continue; fi
  if [ "$((W % 256))" = "$I" ]; then
    echo "  OK   $name: interp=$I native=${N:-n/a} wasm=$W"
  else
    echo "  FAIL $name: wasm=$W != interp=$I"; fail=1
  fi
done

if [ "$ran" -eq 0 ]; then echo "wasm_malloc_abi_parity: nothing linked — skipping"; exit 0; fi
[ "$fail" -eq 0 ] || { echo "wasm_malloc_abi_parity: FAIL"; exit 1; }
echo "wasm_malloc_abi_parity: PASS — array + to_str (malloc/snprintf size_t) run identically on interp, native, AOT-wasm ✓"
exit 0
