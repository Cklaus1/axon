#!/usr/bin/env bash
# build-interactive.sh — produce the Asyncify-instrumented axon-wasm INTERPRETER
# that interactive.html runs (R15 §13 B3). Unlike index.html (which runs an
# AOT-compiled single program), this ships the whole interpreter so the page can
# run any .ax you type, with host_await suspending across browser input.
#
#   bash examples/browser/build-interactive.sh
#   python3 -m http.server -d examples/browser   # then open interactive.html
#
# Requires: rustup target wasm32-unknown-unknown + wasm-opt (binaryen).
set -euo pipefail
ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT"
OUT="examples/browser/axon_interp.async.wasm"

echo "building axon-wasm (wasm32-unknown-unknown)…"
cargo build -q -p axon-wasm --target wasm32-unknown-unknown --release
RAW="target/wasm32-unknown-unknown/release/axon_wasm.wasm"

# binaryen 108 must be told which modern-rustc wasm features to accept; --asyncify
# instruments only the axon_host_await import so host_await is the single suspend point.
FEATURES="--enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-simd --enable-reference-types --enable-multivalue"
echo "wasm-opt --asyncify → $OUT"
wasm-opt $FEATURES --asyncify --pass-arg=asyncify-imports@env.axon_host_await "$RAW" -o "$OUT"
echo "done: $OUT ($(wc -c < "$OUT") bytes)"
