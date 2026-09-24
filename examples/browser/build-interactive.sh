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

# binaryen must be told which modern-rustc wasm features to accept (every version
# tested: 108/120/127 — not a version pin); --asyncify
# instruments only the axon_host_await import so host_await is the single suspend point.
FEATURES="--enable-bulk-memory --enable-sign-ext --enable-mutable-globals --enable-nontrapping-float-to-int --enable-simd --enable-reference-types --enable-multivalue"
echo "wasm-opt --asyncify → $OUT"
# -O2 IS REQUIRED: the unoptimized asyncify output runs away (axon_eval never
# returns; linear memory grows until the host is exhausted) on wasm-opt 120/127.
wasm-opt $FEATURES -O2 --asyncify --pass-arg=asyncify-imports@env.axon_host_await "$RAW" -o "$OUT"
# PROVENANCE STAMP. The page refuses a module this script did not produce. The
# artifact is generated and gitignored, so the file on disk is whatever the last
# local build left — and on 2026-09-24 that was a stale UNOPTIMIZED build that
# exhausts host memory on any program (governance/incidents/2026-09-24-asyncify-
# linear-memory.md). A stamp binds the bytes to the flags that made them; a
# missing stamp, a different digest, or a different -O level is refused by
# interactive.html rather than run.
grep -qa asyncify_start_unwind "$OUT" || { echo "error: $OUT is not asyncify-instrumented" >&2; exit 1; }
printf '{"schema":"axon-asyncify-artifact/1","opt":"O2","sha256":"%s","bytes":%s}\n' \
  "$(sha256sum "$OUT" | cut -d' ' -f1)" "$(wc -c < "$OUT")" > "$OUT.stamp.json"
echo "done: $OUT ($(wc -c < "$OUT") bytes), stamp $OUT.stamp.json"
