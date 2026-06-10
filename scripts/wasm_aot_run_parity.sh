#!/usr/bin/env bash
# wasm_aot_run_parity.sh — R7: AOT-compiled wasm RUNS and matches the interpreter
# on pure-compute programs.
#
# After dead-function pruning, a pure-integer program's wasm object has no
# i64-ABI __axon_* imports, so `axon target build --target wasm32-wasip1` links
# it (reactor mode, --export=main) into a runnable .wasm. This harness builds a
# few pure-int programs, runs the linked wasm under `wasmtime --invoke main`, and
# asserts the result equals the interpreter's exit value. End-to-end AOT-wasm
# EXECUTION, not just object emission.
#
# Skips (exit 0) when codegen / the wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_aot_run_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_aot_run_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_aot_run_parity: interp build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# Pure-integer programs (no str/array runtime → link cleanly after pruning).
declare -A PROGS
PROGS[fib]='fn f(n: i64) -> i64 { if n < 2 { n } else { f(n-1) + f(n-2) } }
fn main() -> i64 { f(10) }'
PROGS[arith]='fn main() -> i64 { (21 + 21) * 2 - 4 }'
# NB: Axon has no `let mut` — declare with `let`, reassign with bare `x = …`.
# The old `let mut` form parse-errored, so this case SILENTLY SKIPPED forever
# (build-failed → SKIP), never actually testing a loop on wasm (a vacuous skip).
PROGS[loop]='fn main() -> i64 { let s = 0  let i = 1  while i <= 10 { s = s + i  i = i + 1 }  s }'

pass=0; fail=0; ran=0
for name in "${!PROGS[@]}"; do
  src="$WORK/$name.ax"; printf '%s\n' "${PROGS[$name]}" > "$src"
  # interp exit value (the i64 main return becomes the process exit code, mod 256)
  "$INTERP" "$src" >/dev/null 2>&1; i_exit=$?
  # AOT-wasm: build (which links if pure-int) then run.
  if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$src" >/dev/null 2>&1; then
    echo "  SKIP $name (wasm build failed)"; continue
  fi
  linked="${src%.ax}.linked.wasm"
  if [ ! -f "$linked" ]; then echo "  SKIP $name (not linkable — has runtime externs)"; continue; fi
  ran=$((ran+1))
  w_out="$("$WASMRT" --invoke main "$linked" 2>/dev/null | grep -vi experimental | head -1)"
  # interp exit code is the value mod 256; the wasm --invoke prints the full i64.
  if [ "$((i_exit))" = "$((w_out % 256))" ]; then
    echo "  OK   $name: interp=$i_exit wasm=$w_out"
    pass=$((pass+1))
  else
    echo "  DIFF $name: interp=$i_exit wasm=$w_out"
    fail=$((fail+1))
  fi
done

echo "wasm_aot_run_parity: $pass/$ran ran-and-matched, $fail differ"
if [ "$ran" -eq 0 ]; then echo "wasm_aot_run_parity: nothing linked — skipping"; exit 0; fi
# Vacuous-skip guard: every PROGS entry is PURE-INT and MUST link+run. If the
# env works (ran>0) but some program silently skipped (build/link failure — e.g.
# a syntax break like the old `let mut`), that's a REGRESSION, not a skip.
total=${#PROGS[@]}
if [ "$ran" -lt "$total" ]; then
  echo "wasm_aot_run_parity: FAIL — only $ran/$total pure-int programs linked+ran; the rest silently skipped (a build/link failure on a pure-int program is a regression, not a skip)"; exit 1
fi
[ "$fail" -eq 0 ] || exit 1
echo "wasm_aot_run_parity: PASS — AOT wasm runs identically to the interpreter on pure-int programs ✓"
exit 0
