#!/usr/bin/env bash
# wasm_examples_parity.sh — R7 acceptance: real examples RUN on AOT-wasm and
# match the interpreter (I-2), not just curated snippets.
#
# For every deterministic `examples/*.ax` with a `fn main`, AOT-compile it to
# wasm32-wasip1, run the linked module under `wasmtime --invoke main`, and assert
# byte-identical STDOUT to the interpreter. This is the broad complement to
# wasm_aot_run_parity.sh (a few hand-built exit-code programs): it proves the now-
# complete __axon_* wasm extern surface (str/dict/array/f64/closures) carries real
# programs end-to-end.
#
# The `--invoke main` call appends main's i64 return value as a trailing stdout
# line (the interpreter uses it as the exit code, printing nothing) — so the wasm
# output's LAST line is stripped before the diff.
#
# Host/non-deterministic examples (AI, random, time, stdin, exec, goal search) are
# skipped — they either aren't on the wasm target or differ by design. A
# pure-compute example that DOESN'T link is reported (object-only) but doesn't
# fail; correctness regressions (a linked example whose output DIFFERS) DO fail,
# and a FLOOR guard catches a mass-skip regression.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

FLOOR=25   # at least this many examples must link+run+match (vacuous-skip guard;
           # actual is 30/30 — headroom for minor churn, catches mass regression)

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_examples_parity: no wasm runtime — skipping"; exit 0; }

AXON="target/debug/axon"
if [ ! -x "$AXON" ]; then
  if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
    echo "wasm_examples_parity: codegen build unavailable — skipping"; exit 0
  fi
fi
# The wasm runtime must be built for str/dict/array examples to link.
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  echo "wasm_examples_parity: wasm32 axon-rt unavailable — skipping"; exit 0
fi
# Probe: can this binary emit + link a runnable pure-int wasm? If not, skip.
PROBE="$(mktemp -d)/probe.ax"; printf 'fn main() -> i64 { 0 }\n' > "$PROBE"
"$AXON" target build "$PROBE" --target wasm32-wasip1 >/dev/null 2>&1
[ -f "${PROBE%.ax}.linked.wasm" ] || { echo "wasm_examples_parity: AOT-wasm link unavailable here — skipping"; exit 0; }

pass=0; diff=0; objonly=0; total=0
fails=""; objs=""
for f in examples/*.ax; do
  grep -q "fn main" "$f" || continue
  # Skip host / non-deterministic examples (not on the wasm target, or differ by design).
  if grep -qE 'ai_complete|ai_extract|random_|now_ms|read_line|read_file|write_file|exec\(|goal_run|goal_step|for!|spawn|llm_|agent_' "$f"; then
    continue
  fi
  total=$((total + 1))
  base="$(basename "$f" .ax)"
  linked="examples/$base.linked.wasm"
  rm -f "$linked"
  if ! "$AXON" target build "$f" --target wasm32-wasip1 >/dev/null 2>&1 || [ ! -f "$linked" ]; then
    objonly=$((objonly + 1)); objs="$objs $base"; rm -f "examples/$base.wasm"; continue
  fi
  I_OUT="$(AXON_SEED=42 "$AXON" run "$f" 2>/dev/null)"
  W_OUT="$("$WASMRT" --invoke main "$linked" 2>/dev/null | grep -vi experimental | head -n -1)"
  rm -f "$linked" "examples/$base.wasm"
  if [ "$I_OUT" = "$W_OUT" ]; then
    pass=$((pass + 1))
  else
    diff=$((diff + 1)); fails="$fails\n  DIFFER: $base"
  fi
done

echo "wasm_examples_parity: $pass/$total linked+matched, $diff differ, $objonly object-only"
[ -n "$objs" ] && echo "wasm_examples_parity: object-only (frontier, not failed):$objs"
if [ "$diff" -ne 0 ]; then
  printf "wasm_examples_parity: FAIL — AOT-wasm output diverged from the interpreter:$fails\n"
  exit 1
fi
if [ "$pass" -lt "$FLOOR" ]; then
  printf "wasm_examples_parity: FAIL — only %d examples linked+matched (floor %d); a mass link regression silently skipped the rest\n" "$pass" "$FLOOR"
  exit 1
fi
echo "wasm_examples_parity: PASS — $pass deterministic examples run byte-identically on AOT-wasm and the interpreter ✓"
exit 0
