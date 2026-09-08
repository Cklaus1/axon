#!/usr/bin/env bash
# wasm_aot_env_parity.sh — R7: `env_var` runs on the AOT-wasm path.
#
# env_var lowers to C `getenv` + `strlen`. `strlen` returns `size_t` (i32 on
# wasm32, i64 native) — codegen declared it i64, which clashed with the wasi
# libc `strlen` (i32) at link (`strlen … (i32)->i64 vs (i32)->i32`) so an
# env_var program was object-only (NO-LINK). Codegen now declares strlen at
# target width and zero-extends its result back to i64 for the AxonStr len
# field. This harness builds an env_var program, links it, runs it under
# `wasmtime --env`, and asserts the result equals the interpreter's (with the
# same env set on the native interp run).
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
[ -n "$WASMRT" ] || { echo "wasm_aot_env_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_aot_env_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_aot_env_parity: interp build unavailable — skipping"; exit 0
fi
# Distinguish an ABSENT target from a BROKEN build. The probe used to be
# `if ! cargo build ... 2>/dev/null` reporting "unavailable - skipping", which
# said the same thing for both and threw away the compiler error naming which.
# That is how a `data:`/`ptr:` typo in a `#[cfg(target_arch = "wasm32")]` arm
# (4a600f2) took all 8 wasm_* harnesses dark for a day while parity_all printed
# "SKIP (toolchain absent)" -- both wasm32 targets were installed the whole time.
# A skip must be honest about WHY, or it is a silent loss of coverage.
if ! rustup target list --installed 2>/dev/null | grep -qx "wasm32-wasip1"; then
  echo "wasm_aot_env_parity: wasm32-wasip1 target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-wasip1 2>&1)"; then
  echo "wasm_aot_env_parity: FAIL - axon-rt does NOT build for wasm32-wasip1 (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
AXON="${AXON:-target/debug/axon}"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# env_var returns Result<str,str>; on the SET key the result is str_len(value).
SRC="$WORK/env.ax"
# Print the answer rather than returning it from `main`. An i64 falling out of
# `main` is NOT the exit code: 2..=15 and 101 are remapped to 1, as is anything
# outside 0..=255 (governance/EXIT_CODES.md). The Err arm returns -1, which
# remaps to 1 -- so an engine where env_var broke reports the same 1 as a dozen
# other outcomes. This passed only because str_len("wasiworks") is 9 and 9 is
# outside the band: shorten VAL to 5 characters and the check goes blind. Read
# the printed value and neither the answer nor the failure mode is lossy.
cat > "$SRC" <<'AX'
fn probe() -> i64 {
    match env_var("AXON_AOT_ENV") { Ok(v) => str_len(v)  Err(e) => -1 }
}
fn main() { println(to_str(probe())) }
AX
VAL="wasiworks"   # str_len = 9

# interpreter oracle (env on the process)
I="$(AXON_AOT_ENV="$VAL" "$INTERP" "$SRC" 2>/dev/null | grep -v '^axon: run-id ' | tail -1)"
[ -n "$I" ] || { echo "wasm_aot_env_parity: FAIL — interp printed nothing"; exit 1; }
echo "wasm_aot_env_parity: interp = $I"

# native AOT
if "$AXON" build "$SRC" -o "$WORK/native" >/dev/null 2>&1; then
  N="$(AXON_AOT_ENV="$VAL" "$WORK/native" 2>/dev/null | tail -1)"
  echo "wasm_aot_env_parity: native = $N"
  if [ "$N" != "$I" ]; then echo "wasm_aot_env_parity: FAIL — native ($N) != interp ($I)"; exit 1; fi
fi

# AOT-wasm — the strlen size_t bridge under test.
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
  echo "wasm_aot_env_parity: wasm build unavailable — skipping"; exit 0
fi
L="${SRC%.ax}.linked.wasm"
if [ ! -f "$L" ]; then
  echo "wasm_aot_env_parity: FAIL — env_var program did NOT link (strlen size_t regressed)"; exit 1
fi
# 2>/dev/null drops the experimental warning; the probe's main prints on stdout
# and returns nothing, so the first line is the program's own output.
W="$("$WASMRT" --env AXON_AOT_ENV="$VAL" --invoke main "$L" 2>/dev/null | grep -oE '^-?[0-9]+$' | head -1)"
echo "wasm_aot_env_parity: wasm = ${W:-<none>}"
if [ -z "$W" ]; then echo "wasm_aot_env_parity: FAIL — wasm produced no numeric output (trap?)"; exit 1; fi
if [ "$W" != "$I" ]; then echo "wasm_aot_env_parity: FAIL — wasm ($W) != interp ($I)"; exit 1; fi

echo "wasm_aot_env_parity: PASS — env_var (getenv+strlen size_t) runs identically on interp, native, AOT-wasm ✓"
exit 0
