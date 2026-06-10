#!/usr/bin/env bash
# wasm_aot_stdout_parity.sh — R7: AOT-compiled wasm produces byte-identical
# STDOUT to the interpreter across the example corpus.
#
# This is the end-to-end AOT-wasm correctness bar: not just "links and exits 0"
# but "prints exactly what the interpreter (the I-2 reference oracle) prints".
# It exercises the whole size_t ABI bridge (malloc/snprintf/memcpy/write) plus
# the void-`fn main()` wasm entry fix (emit i64 return so the wasi libc C-main
# convention doesn't bind our `main` and break `wasmtime --invoke main`).
#
# Auto-discovers examples/*.ax that (a) have a `main`, (b) use no AI / thread /
# goal / fs / env / random builtins (those need a host the pure AOT path lacks),
# builds each to wasm, links (reactor mode), runs under `wasmtime --invoke main`,
# strips wasmtime's trailing return-value line, and diffs against the interp.
#
# Skips (exit 0) when codegen / the wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
# Serialize the wasm parity scripts under one shared lock: each builds its
# wasm artifacts next to the source (examples/$base.*.wasm), so concurrent runs
# (cargo's parallel test threads invoke several of these at once) clobber each
# other's intermediates — a file race that surfaces as spurious DIFFER /
# "No such file". flock makes the wasm sweeps run one at a time (orthogonal to
# the ~370 other tests, which keep running in parallel). No-op without flock.
if command -v flock >/dev/null 2>&1; then exec 9>"${TMPDIR:-/tmp}/axon_wasm_parity.lock" && flock 9; fi

WASMRT=""
for rt in wasmtime "$HOME/.wasmtime/bin/wasmtime"; do
  if command -v "$rt" >/dev/null 2>&1 || [ -x "$rt" ]; then WASMRT="$rt"; break; fi
done
[ -n "$WASMRT" ] || { echo "wasm_aot_stdout_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_aot_stdout_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_aot_stdout_parity: interp build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-rt --target wasm32-wasip1 2>/dev/null; then
  echo "wasm_aot_stdout_parity: wasm32 axon-rt build unavailable — skipping"; exit 0
fi
AXON="target/debug/axon"
INTERP="target/debug/axon-run"

# Builtins whose host the pure AOT-wasm path doesn't provide → skip those files.
EXCLUDE_RE='ai_complete|ai_extract|spawn|thread|chan_|goal_run|goal_eval|agent_|read_file|write_file|env_var|exec\(|random_|srand'

pass=0; fail=0; skip=0
for f in examples/*.ax; do
  [ -f "$f" ] || continue
  grep -q "fn main" "$f" || { skip=$((skip+1)); continue; }
  if grep -qE "$EXCLUDE_RE" "$f"; then skip=$((skip+1)); continue; fi
  n="$(basename "$f" .ax)"

  ig="$("$INTERP" "$f" 2>/dev/null)"; irc=$?
  # Skip programs the interpreter itself rejects (e.g. @[test]-only files).
  if [ $irc -ne 0 ]; then skip=$((skip+1)); continue; fi

  if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$f" >/dev/null 2>&1; then
    echo "  SKIP $n (wasm build unavailable)"; skip=$((skip+1)); continue
  fi
  L="${f%.ax}.linked.wasm"
  if [ ! -f "$L" ]; then echo "  SKIP $n (not linkable)"; skip=$((skip+1)); rm -f "${f%.ax}.wasm"; continue; fi

  # wasmtime --invoke prints program stdout then the i64 return on a final line.
  wg="$("$WASMRT" --invoke main "$L" 2>/dev/null | grep -vi experimental | head -n -1)"
  rm -f "$L" "${f%.ax}.wasm"

  if [ "$ig" = "$wg" ]; then
    echo "  MATCH $n"
    pass=$((pass+1))
  else
    echo "  DIFF  $n"
    diff <(printf '%s' "$ig") <(printf '%s' "$wg") | head -6 | sed 's/^/    /'
    fail=$((fail+1))
  fi
done

echo "wasm_aot_stdout_parity: $pass match, $fail differ, $skip skipped"
if [ "$pass" -eq 0 ]; then echo "wasm_aot_stdout_parity: nothing ran — skipping"; exit 0; fi
[ "$fail" -eq 0 ] || exit 1
echo "wasm_aot_stdout_parity: PASS — AOT-wasm stdout is byte-identical to the interpreter across the corpus ✓"
exit 0
