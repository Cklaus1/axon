#!/usr/bin/env bash
# wasm_browser_examples_parity.sh — R7c breadth: real examples/*.ax run on the
# BROWSER target (wasm32-unknown-unknown, wasi-free) with stdout identical to the
# interpreter, driven by the Node host (scripts/wasm_browser_host.js, standing in
# for the page's WebAssembly glue).
#
# This is the browser counterpart of wasm_examples_parity.sh (which targets the
# headless wasip1 backend via wasmtime). Here the module is wasi-free and
# `println` is routed through the `axon_host_write` host import, so the Node host
# captures stdout. Host/non-deterministic examples are skipped; an example that
# doesn't link (e.g. needs a not-yet-browser-shimmed libc symbol) is reported
# object-only but doesn't fail; a linked example whose output DIFFERS fails, and a
# FLOOR guard catches a mass-skip regression. Skips (exit 0) without node/toolchain.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

FLOOR=20   # at least this many examples must link+run+match on the browser target

command -v node >/dev/null 2>&1 || { echo "wasm_browser_examples_parity: no node — skipping"; exit 0; }
HOSTJS="scripts/wasm_browser_host.js"
[ -f "$HOSTJS" ] || { echo "wasm_browser_examples_parity: host harness missing — skipping"; exit 0; }
AXON="target/debug/axon"
if [ ! -x "$AXON" ]; then
  cargo build -q -p axon-core --bin axon 2>/dev/null || { echo "wasm_browser_examples_parity: codegen unavailable — skipping"; exit 0; }
fi
if ! cargo build -q -p axon-rt --target wasm32-unknown-unknown 2>/dev/null; then
  echo "wasm_browser_examples_parity: unknown-unknown axon-rt unavailable — skipping"; exit 0
fi
PROBE="$(mktemp -d)/p.ax"; printf 'fn main() { println("ok") }\n' > "$PROBE"
"$AXON" target build "$PROBE" --target wasm32-unknown-unknown >/dev/null 2>&1
[ -f "${PROBE%.ax}.linked.wasm" ] || { echo "wasm_browser_examples_parity: browser link unavailable — skipping"; exit 0; }

pass=0; diff=0; objonly=0; total=0
fails=""; objs=""
for f in examples/*.ax; do
  grep -q "fn main" "$f" || continue
  # Skip host / non-deterministic / time-dependent examples: AI, randomness, the
  # clock (now_ms directly AND temporal_*, which stamps created_at via __axon_now_ms
  # internally — no wasi clock on the browser; a Date.now() host import is the
  # follow-on), I/O, exec, goal search, spawn. (parse_int users currently stay
  # object-only on the browser — they pull C strtoll, not yet shimmed/extern'd —
  # reported below, not failed.)
  if grep -qE 'ai_complete|ai_extract|random_|now_ms|temporal_|sleep_ms|read_line|read_file|write_file|exec\(|goal_run|goal_step|for!|spawn|llm_|agent_' "$f"; then
    continue
  fi
  total=$((total + 1))
  base="$(basename "$f" .ax)"
  linked="examples/$base.linked.wasm"
  rm -f "$linked" "examples/$base.wasm"
  if ! "$AXON" target build "$f" --target wasm32-unknown-unknown >/dev/null 2>&1 || [ ! -f "$linked" ]; then
    objonly=$((objonly + 1)); objs="$objs $base"; rm -f "examples/$base.wasm"; continue
  fi
  if strings "$linked" | grep -qi wasi_snapshot; then
    diff=$((diff + 1)); fails="$fails\n  WASI-IMPORT: $base"; rm -f "$linked" "examples/$base.wasm"; continue
  fi
  I_OUT="$(AXON_SEED=42 "$AXON" run "$f" 2>/dev/null)"
  W_OUT="$(node "$HOSTJS" "$linked" 2>/dev/null)"
  rm -f "$linked" "examples/$base.wasm"
  if [ "$I_OUT" = "$W_OUT" ]; then
    pass=$((pass + 1))
  else
    diff=$((diff + 1)); fails="$fails\n  DIFFER: $base"
  fi
done

echo "wasm_browser_examples_parity: $pass/$total linked+matched, $diff differ, $objonly object-only"
[ -n "$objs" ] && echo "wasm_browser_examples_parity: object-only (frontier, not failed):$objs"
if [ "$diff" -ne 0 ]; then
  printf "wasm_browser_examples_parity: FAIL — browser output diverged from the interpreter:$fails\n"
  exit 1
fi
if [ "$pass" -lt "$FLOOR" ]; then
  printf "wasm_browser_examples_parity: FAIL — only %d examples ran on the browser target (floor %d); a mass link regression silently skipped the rest\n" "$pass" "$FLOOR"
  exit 1
fi
echo "wasm_browser_examples_parity: PASS — $pass deterministic examples run on the WASI-FREE browser target with stdout identical to the interpreter ✓"
exit 0
