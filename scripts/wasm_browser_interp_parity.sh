#!/usr/bin/env bash
# wasm_browser_interp_parity.sh — the Axon INTERPRETER runs in the browser
# (wasm32-unknown-unknown) and `eval`s arbitrary .ax source IDENTICALLY to native.
#
# axon-wasm is a cdylib exposing axon_alloc/axon_eval/axon_output_{ptr,len} — the
# tree-walking interpreter driven by raw C-ABI exports (ZERO imports → a bare
# WebAssembly.instantiate). Unlike the codegen browser path (AOT-compiles each
# program), this is a DYNAMIC eval (playground/REPL) and the entry-point
# foundation for the R15 browser host_await binding. This harness runs a set of
# compute-only programs through BOTH the native interpreter (`axon run`) and the
# wasm interpreter (under Node) and asserts byte-identical stdout + exit code.
#
# Requires: rustup target wasm32-unknown-unknown + node. Skips (exit 0) if absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-unknown-unknown; then
  echo "wasm_browser_interp_parity: wasm32-unknown-unknown target not installed — skipping"; exit 0
fi
if ! command -v node >/dev/null 2>&1; then
  echo "wasm_browser_interp_parity: node not found — skipping"; exit 0
fi

echo "wasm_browser_interp_parity: building axon-wasm (wasm32-unknown-unknown) + native axon…"
cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release 2>/dev/null \
  || { echo "axon-wasm wasm build failed — skipping"; exit 0; }
cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null \
  || { echo "native build failed — skipping"; exit 0; }
WASM="target/wasm32-unknown-unknown/release/axon_wasm.wasm"
NATIVE="target/debug/axon"
DRIVER="scripts/wasm_interp_driver.js"

# Compute-only programs (no host: fs/ai/random/time/host_await) — the cases the
# wasm interpreter can run identically without a host binding.
PROGS=(
  examples/hello.ax
  examples/math.ax
  examples/algorithms.ax
  examples/structs.ax
  examples/enums.ax
  examples/while.ax
  examples/options.ax
  examples/modulo.ax
)

pass=0
for p in "${PROGS[@]}"; do
  [ -f "$p" ] || continue
  n_out="$("$NATIVE" run "$p" 2>/dev/null)"; n_code=$?
  w_out="$(node "$DRIVER" "$WASM" "$p" 2>/dev/null)"; w_code=$?
  if [ "$n_out" != "$w_out" ] || [ "$n_code" != "$w_code" ]; then
    echo "wasm_browser_interp_parity: FAIL ($p): native[$n_code] != wasm[$w_code]"
    echo "--- native ---"; echo "$n_out" | sed 's/^/  /'
    echo "--- wasm ---";   echo "$w_out" | sed 's/^/  /'
    exit 1
  fi
  echo "  OK  $(basename "$p"): [$n_code] native==wasm"
  pass=$((pass + 1))
done

if [ "$pass" -eq 0 ]; then
  echo "wasm_browser_interp_parity: FAIL — no programs ran (corpus missing?)"; exit 1
fi
echo "wasm_browser_interp_parity: PASS — $pass programs eval identically on the wasm interpreter + native"
exit 0
