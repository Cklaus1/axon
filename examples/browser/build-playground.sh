#!/usr/bin/env bash
# build-playground.sh — build the plain axon-wasm interpreter that playground.html
# runs: a paste-and-run REPL that surfaces capability/type diagnostics live (it
# runs the static check before running, like the `axon run` CLI). No Asyncify
# needed — the playground programs don't suspend on host_await.
#
#   bash examples/browser/build-playground.sh
#   python3 -m http.server -d examples/browser   # then open /playground.html
#
# Requires: rustup target wasm32-unknown-unknown.
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
OUT="examples/browser/axon_interp.wasm"

echo "building axon-wasm (wasm32-unknown-unknown)…"
cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release
cp target/wasm32-unknown-unknown/release/axon_wasm.wasm "$OUT"
echo "done: $OUT ($(wc -c < "$OUT") bytes) — serve this dir and open /playground.html"
