#!/usr/bin/env bash
# wasm_parity.sh — R7 Slice A acceptance harness.
#
# Builds the codegen-free `axon-run` for both native and wasm32-wasip1, then
# runs a corpus of pure-compute `.ax` examples through each and asserts the exit
# code AND stdout are identical. This is the executable form of the R7 §9
# acceptance criterion `wasm_interp_matches_native_on_pure_compute`: the wasm
# interpreter is the *same* interp.rs compiled to a second target, so the only
# divergence surface is the host interface (R7 §4.3) — and pure-compute programs
# touch none of it.
#
# Requires: rustup target wasm32-wasip1, and a wasm runtime (wasmtime) on PATH.
# Skips (exit 0 with a notice) if the wasm toolchain is absent, so it is safe to
# run in environments without it.
#
# Usage:  scripts/wasm_parity.sh
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Locate a wasm runtime.
WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
if [ -z "$WASMRT" ]; then
  echo "wasm_parity: no wasm runtime (wasmtime) found — skipping (install: curl https://wasmtime.dev/install.sh | bash)"
  exit 0
fi
if ! rustup target list --installed 2>/dev/null | grep -q wasm32-wasip1; then
  echo "wasm_parity: wasm32-wasip1 target not installed — skipping (rustup target add wasm32-wasip1)"
  exit 0
fi

echo "wasm_parity: building axon-run (native + wasm32-wasip1)…"
cargo build -q -p axon-core --no-default-features --bin axon-run || { echo "native build failed"; exit 1; }
cargo build -q -p axon-core --no-default-features --bin axon-run --target wasm32-wasip1 || { echo "wasm build failed"; exit 1; }

NATIVE="target/debug/axon-run"
WASM="target/wasm32-wasip1/debug/axon-run.wasm"

# Pure-compute corpus: examples that use no fs/env/AI/threads (R7 §4.1 row 1).
CORPUS=(
  hello.ax math.ax while.ax algorithms.ax modulo.ax
  logical_ops.ax floats.ax comprehensive.ax structs.ax enums.ax
  options.ax slices.ax interpolation.ax escapes.ax math_builtins.ax
)

pass=0; fail=0
for name in "${CORPUS[@]}"; do
  f="examples/$name"
  [ -f "$f" ] || continue
  # Native.
  n_out="$("$NATIVE" "$f" 2>/dev/null)"; n_code=$?
  # wasm (wasmtime needs an absolute path and a dir grant to read the file).
  w_out="$("$WASMRT" --dir / "$WASM" "$ROOT/$f" 2>/dev/null)"; w_code=$?
  if [ "$n_code" = "$w_code" ] && [ "$n_out" = "$w_out" ]; then
    echo "  OK   $name (exit $n_code)"
    pass=$((pass+1))
  else
    echo "  DIFF $name native=(code $n_code) wasm=(code $w_code)"
    echo "    native stdout: $n_out"
    echo "    wasm   stdout: $w_out"
    fail=$((fail+1))
  fi
done

echo "wasm_parity: $pass passed, $fail differ"
[ "$fail" -eq 0 ] || exit 1
[ "$pass" -gt 0 ] || { echo "wasm_parity: no corpus files ran"; exit 1; }
echo "wasm_parity: native and wasm interpreters agree on the pure-compute corpus ✓"
