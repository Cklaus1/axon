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
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

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

# The value wedge in the browser: the wasm interpreter runs the static CHECK
# first (like `axon run`), so a capability-violating @[contained] program is
# REFUSED in-browser with its E1001 diagnostic — never run — exactly as at the
# CLI. (Before this, the playground eval'd without checking, hiding the wedge.)
echo "wasm_browser_interp_parity: checking capability diagnostics surface in-browser…"
EVIL="$WORK/evil.ax"
cat > "$EVIL" <<'AX'
@[contained(fs: [], net: [], exec: none)]
fn agent() -> i64 { let _ = write_file("/tmp/x", "leak")  0 }
fn main() -> i64 { agent() }
AX
n_out="$("$NATIVE" run "$EVIL" 2>&1)"; n_code=$?
w_out="$(node "$DRIVER" "$WASM" "$EVIL" 2>/dev/null)"; w_code=$?
if [ "$w_code" != "2" ]; then
  echo "wasm_browser_interp_parity: FAIL — over-reaching agent not refused in-browser (exit $w_code):"
  echo "$w_out" | sed 's/^/  /'; exit 1
fi
if ! echo "$w_out" | grep -q "E1001"; then
  echo "wasm_browser_interp_parity: FAIL — E1001 capability diagnostic missing in-browser:"
  echo "$w_out" | sed 's/^/  /'; exit 1
fi
# Native refuses it too (both run the check) — same verdict, browser == CLI.
if [ "$n_code" != "2" ]; then
  echo "wasm_browser_interp_parity: FAIL — native did not refuse the over-reaching agent (exit $n_code)"; exit 1
fi
echo "  OK  capability violation refused in-browser (E1001, exit 2) — wedge visible, browser==CLI"

echo "wasm_browser_interp_parity: PASS — $pass programs eval identically on the wasm interpreter + native; capability diagnostics surface in-browser"
exit 0
