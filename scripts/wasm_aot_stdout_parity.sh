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
# shellcheck source=lib/harness_skip.sh
. "$(dirname "${BASH_SOURCE[0]}")/lib/harness_skip.sh"
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
# Distinguish an ABSENT target from a BROKEN build. The probe used to be
# `if ! cargo build ... 2>/dev/null` reporting "unavailable - skipping", which
# said the same thing for both and threw away the compiler error naming which.
# That is how a `data:`/`ptr:` typo in a `#[cfg(target_arch = "wasm32")]` arm
# (4a600f2) took all 8 wasm_* harnesses dark for a day while parity_all printed
# "SKIP (toolchain absent)" -- both wasm32 targets were installed the whole time.
# A skip must be honest about WHY, or it is a silent loss of coverage.
# A skip must prove its own reason: the previous probe discarded
# rustup's exit status, so a missing rustup concluded "not installed".
rust_target_installed wasm32-wasip1; _rt=$?
if [ "$_rt" -eq 2 ]; then
  echo "wasm_aot_stdout_parity: cannot determine whether wasm32-wasip1 is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  echo "wasm_aot_stdout_parity: wasm32-wasip1 target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-wasip1 2>&1)"; then
  echo "wasm_aot_stdout_parity: FAIL - axon-rt does NOT build for wasm32-wasip1 (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
AXON="${AXON:-target/debug/axon}"
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
# A vacuous green, found by the coverage-metric audit: this used to read
#   if [ "$pass" -eq 0 ]; then ... exit 0; fi
# so `AXON=/bin/false bash scripts/wasm_aot_stdout_parity.sh` printed
# "0 match, 0 differ, 42 skipped" and exited 0. Nothing ran and the harness
# said so, but it said so with a SUCCESS code.
#
# "Nothing ran" cannot mean "toolchain absent" HERE: every toolchain
# precondition above already exits 0 with a stated reason before the loop
# starts. Reaching this line with pass=0 means the compiler under test is
# broken or the corpus vanished — both failures. The two sibling wasm
# harnesses already guard this with a FLOOR; this one was the omission.
FLOOR=${FLOOR:-25}   # actual is 29/42 today; headroom for corpus churn, still
                     # far above the mass-skip regression this exists to catch
if [ "$pass" -lt "$FLOOR" ]; then
  printf "wasm_aot_stdout_parity: FAIL — only %d examples ran (floor %d). Every toolchain precondition passed, so this is a broken compiler or a vanished corpus, not an absent toolchain.\n" "$pass" "$FLOOR"
  exit 1
fi
[ "$fail" -eq 0 ] || exit 1
echo "wasm_aot_stdout_parity: PASS — AOT-wasm stdout is byte-identical to the interpreter across the corpus ✓"
exit 0
