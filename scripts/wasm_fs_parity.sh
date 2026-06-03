#!/usr/bin/env bash
# wasm_fs_parity.sh — R7: the AxonHost seam extends cross-platform parity PAST
# pure-compute to actual file I/O.
#
# The interpreter's `read_file`/`write_file` route through `crate::host` (the
# AxonHost seam). `DefaultHost` uses std::fs — and WASI provides std::fs under a
# capability grant. So a file-round-trip program runs IDENTICALLY on native and
# on wasm32-wasip1 (under `wasmtime --dir <grant>`), and the capability model
# carries over (wasm touches only granted dirs). This harness proves it: it
# writes + reads a file both ways and asserts byte-identical stdout + exit code.
#
# Requires wasm32-wasip1 + a wasm runtime; skips (exit 0) when absent.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
if [ -z "$WASMRT" ]; then
  echo "wasm_fs_parity: no wasm runtime — skipping"; exit 0
fi
if ! rustup target list --installed 2>/dev/null | grep -q wasm32-wasip1; then
  echo "wasm_fs_parity: wasm32-wasip1 target not installed — skipping"; exit 0
fi

echo "wasm_fs_parity: building axon-run (native + wasm32-wasip1)…"
cargo build -q -p axon-core --no-default-features --bin axon-run || { echo "native build failed"; exit 1; }
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 || { echo "wasm build failed"; exit 1; }

NATIVE="target/debug/axon-run"
WASM="target/wasm32-wasip1/debug/axon-run.wasm"

PROG="examples/file_roundtrip.ax"
[ -f "$PROG" ] || { echo "wasm_fs_parity: $PROG missing"; exit 1; }

# Use a distinct file per engine so they don't read each other's writes — we are
# testing that each engine's OWN write+read round-trips, not that they share a fs.
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

n_out="$(cd "$WORK" && "$ROOT/$NATIVE" "$ROOT/$PROG" 2>/dev/null)"; n_code=$?
# wasmtime needs the file path and a dir grant for both the program's reads and
# its /tmp writes.
w_out="$("$WASMRT" --dir / --dir /tmp "$WASM" "$ROOT/$PROG" 2>/dev/null)"; w_code=$?

echo "  native: [$n_out] (exit $n_code)"
echo "  wasm:   [$w_out] (exit $w_code)"

if [ "$n_code" = "$w_code" ] && [ "$n_out" = "$w_out" ]; then
  case "$n_out" in
    *"read back: hello from axon"*) : ;;
    *) echo "wasm_fs_parity: FAIL — unexpected output: $n_out"; exit 1 ;;
  esac
  echo "wasm_fs_parity: PASS — file I/O is byte-identical on native and wasm (WASI) ✓"
  exit 0
else
  echo "wasm_fs_parity: FAIL — native and wasm file I/O diverge"
  exit 1
fi
