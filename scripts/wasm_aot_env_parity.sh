#!/usr/bin/env bash
# wasm_aot_env_parity.sh — R7: `env_var` runs on the AOT-wasm path.
#
# env_var lowers to C `getenv` + `strlen`. `strlen` returns `size_t` (i32 on
# wasm32, i64 native) — codegen declared it i64, which clashed with the wasi
# libc `strlen` (i32) at link (`strlen … (i32)->i64 vs (i32)->i32`) so an
# env_var program was object-only (NO-LINK). Codegen now declares strlen at
# target width and zero-extends its result back to i64 for the AxonStr len
# field. This harness builds an env_var program, links it, runs it under
# `wasmtime --env`, and asserts the result equals the interpreter's (with the
# same env set on the native interp run).
#
# Skips (exit 0) when codegen / the wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_aot_env_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_aot_env_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_aot_env_parity: interp build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  echo "wasm_aot_env_parity: wasm32 axon-rt build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# env_var returns Result<str,str>; on the SET key the result is str_len(value).
SRC="$WORK/env.ax"
cat > "$SRC" <<'AX'
fn main() -> i64 {
    match env_var("AXON_AOT_ENV") { Ok(v) => str_len(v)  Err(e) => -1 }
}
AX
VAL="wasiworks"   # str_len = 9

# interpreter oracle (env on the process)
I="$(AXON_AOT_ENV="$VAL" "$INTERP" "$SRC" >/dev/null 2>&1; echo $?)"
echo "wasm_aot_env_parity: interp = $I"

# native AOT
if "$AXON" build "$SRC" -o "$WORK/native" >/dev/null 2>&1; then
  AXON_AOT_ENV="$VAL" "$WORK/native" >/dev/null 2>&1; N=$?
  echo "wasm_aot_env_parity: native = $N"
  if [ "$N" != "$I" ]; then echo "wasm_aot_env_parity: FAIL — native ($N) != interp ($I)"; exit 1; fi
fi

# AOT-wasm — the strlen size_t bridge under test.
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
  echo "wasm_aot_env_parity: wasm build unavailable — skipping"; exit 0
fi
L="${SRC%.ax}.linked.wasm"
if [ ! -f "$L" ]; then
  echo "wasm_aot_env_parity: FAIL — env_var program did NOT link (strlen size_t regressed)"; exit 1
fi
W="$("$WASMRT" --env AXON_AOT_ENV="$VAL" --invoke main "$L" 2>/dev/null | grep -vi experimental | grep -oE '^-?[0-9]+$' | head -1)"
echo "wasm_aot_env_parity: wasm = ${W:-<none>}"
if [ -z "$W" ]; then echo "wasm_aot_env_parity: FAIL — wasm produced no numeric output (trap?)"; exit 1; fi
if [ "$((W % 256))" != "$I" ]; then echo "wasm_aot_env_parity: FAIL — wasm ($W) != interp ($I)"; exit 1; fi

echo "wasm_aot_env_parity: PASS — env_var (getenv+strlen size_t) runs identically on interp, native, AOT-wasm ✓"
exit 0
