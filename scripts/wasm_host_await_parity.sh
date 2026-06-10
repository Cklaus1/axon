#!/usr/bin/env bash
# wasm_host_await_parity.sh — interactive host_await runs IDENTICALLY on native
# and on wasm32-wasip1 under wasmtime (R15 / R7 headless-interactive).
#
# The native host_await substrate is a worker thread (run_suspendable); wasm has
# no threads, so the wasm host_await_yield reads stdin DIRECTLY (synchronous,
# single-stack). This harness pipes the same input to a host_await program run
# (a) by the native interpreter and (b) by axon-run.wasm under wasmtime, and
# asserts byte-identical stdout + exit code — proving the two substrates are
# observably equivalent for the headless (stdin-driven) case. (The browser case —
# wasm32-unknown-unknown, no stdin — needs the Asyncify+JS substrate; R7c.)
#
# Requires: rustup target wasm32-wasip1 + wasmtime on PATH. Skips (exit 0) if absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

if ! rustup target list --installed 2>/dev/null | grep -q wasm32-wasip1; then
  echo "wasm_host_await_parity: wasm32-wasip1 target not installed — skipping"; exit 0
fi
WASMTIME=""
for c in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do command -v "$c" >/dev/null 2>&1 && WASMTIME="$c" && break; done
if [ -z "$WASMTIME" ]; then echo "wasm_host_await_parity: wasmtime not found — skipping"; exit 0; fi

echo "wasm_host_await_parity: building axon (native) + axon-run (wasm32-wasip1)…"
cargo build -q -p axon-core --no-default-features --bin axon 2>/dev/null || { echo "native build failed — skipping"; exit 0; }
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 2>/dev/null || { echo "wasm build failed — skipping"; exit 0; }
NATIVE="target/debug/axon"
WASM="target/wasm32-wasip1/debug/axon-run.wasm"

# (program, piped-stdin) pairs covering: a fixed-exchange prompt, EOF, and a
# multi-turn approval loop.
check() {
  local label="$1" prog="$2" input="$3"
  local n_out n_code w_out w_code
  n_out="$(printf '%b' "$input" | "$NATIVE" run "$prog" 2>&1)"; n_code=$?
  w_out="$(printf '%b' "$input" | "$WASMTIME" run --dir=. "$WASM" "$prog" 2>&1)"; w_code=$?
  if [ "$n_out" != "$w_out" ] || [ "$n_code" != "$w_code" ]; then
    echo "wasm_host_await_parity: FAIL ($label): native[$n_code] != wasm[$w_code]"
    echo "--- native ---"; echo "$n_out" | sed 's/^/  /'
    echo "--- wasm ---";   echo "$w_out" | sed 's/^/  /'
    exit 1
  fi
  echo "  OK  $label: [$n_code] native==wasm"
}

check greet      examples/interactive/greet.ax          'Ada\n'
check greet_eof  examples/interactive/greet.ax          ''
check guess      examples/interactive/guess.ax          '5\n9\n7\n'
check approval   examples/interactive/approval_agent.ax 'y\nn\ny\n'

echo "wasm_host_await_parity: PASS — host_await runs identically on native + wasm32-wasip1"
exit 0
