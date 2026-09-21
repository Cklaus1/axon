#!/usr/bin/env bash
# wasm_malloc_abi_parity.sh — R7: the libc `size_t` width bridge for AOT-wasm.
#
# wasm32 is an ILP32 target: `size_t`/pointers are 32-bit, so the C runtime's
# `malloc` and `snprintf` take an i32 size, NOT the i64 the native (LP64) path
# bakes in. Codegen now declares those once with the target-correct width
# (`size_ty()`) and narrows every size argument via `msize()` on wasm32. Without
# it, an ARRAY LITERAL (`malloc(n*elem)`) or `to_str` (`snprintf(buf, len, …)`)
# traps the wasm verifier (`type mismatch: expected i32, found i64`).
#
# This harness builds programs exercising both paths and asserts the result is
# identical on interp, native, and AOT-wasm. Skips (exit 0) when codegen / the
# wasm toolchain is absent.
set -u
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"
. "$ROOT/scripts/lib/harness_skip.sh"
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
[ -n "$WASMRT" ] || { echo "wasm_malloc_abi_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_malloc_abi_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_malloc_abi_parity: interp build unavailable — skipping"; exit 0
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
  echo "wasm_malloc_abi_parity: cannot determine whether wasm32-wasip1 is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  echo "wasm_malloc_abi_parity: wasm32-wasip1 target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-wasip1 2>&1)"; then
  echo "wasm_malloc_abi_parity: FAIL - axon-rt does NOT build for wasm32-wasip1 (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
AXON="${AXON:-target/debug/axon}"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# Each program routes through a heap allocation whose size is a `size_t`:
#   arr   — array literal → malloc(n * elem_size)
#   tostr — to_str(i64)   → snprintf(NULL,0,…) then malloc + snprintf(buf,len,…)
#   combo — both + a float to_str (snprintf %.6g path)
# interp — string interpolation lowers to `axon_concat`, which malloc's a buffer
# and memcpy's both halves: exercises the memcpy/memset size_t path too.
declare -A PROGS
PROGS[arr]='fn main() -> i64 { let a = [10, 20, 12]  a[0] + a[1] + a[2] }'
PROGS[tostr]='fn main() -> i64 { let s = to_str(12345)  str_len(s) }'
PROGS[interp]='fn main() -> i64 { let a = "foo"  let b = "bar"  let c = "{a}{b}"  str_len(c) }'
# eprintln writes to stderr via `write(2, buf, count)` — count is size_t (i32 on
# wasm32). Exercises the write() size_t width. Returns 0 (stderr is side-effect).
PROGS[eprint]='fn main() -> i64 { eprintln("to stderr")  0 }'
PROGS[combo]='fn main() -> i64 {
    let a = [10, 20, 12]
    let s = to_str(a[0] + a[1] + a[2])
    let f = to_str(3.5)
    let msg = "n={s}"
    str_len(s) + str_len(f) + a[2] + str_len(msg)
}'

fail=0; ran=0
for name in arr tostr interp eprint combo; do
  # Compare PRINTED values, not exit codes. `main`'s i64 return is not the exit
  # code: 2..=15 and 101 are remapped to 1, as is anything outside 0..=255
  # (governance/EXIT_CODES.md). Two of these five cases were in that band --
  # `tostr` truly returns 5 and `interp` truly returns 6 -- so all three engines
  # reported 1 and agreed for the wrong reason. Any wasm answer that also
  # remapped to 1 would have printed OK, in the harness that guards the
  # malloc/snprintf/memcpy size_t ABI.
  SRC="$WORK/$name.ax"
  printf '%s\n' "${PROGS[$name]}" | sed 's/fn main() -> i64 {/fn probe() -> i64 {/' > "$SRC"
  printf 'fn main() { println(to_str(probe())) }\n' >> "$SRC"
  I="$("$INTERP" "$SRC" 2>/dev/null | grep -v '^axon: run-id ' | tail -1)"
  [ -n "$I" ] || { echo "  FAIL $name: interp printed nothing"; fail=1; continue; }
  # native
  # A failed native build used to vanish silently out of this `if`.
  if berr="$("$AXON" build "$SRC" -o "$WORK/$name.n" 2>&1)"; then
    N="$("$WORK/$name.n" 2>/dev/null | tail -1)"
    if [ "$N" != "$I" ]; then echo "  FAIL $name: native=$N != interp=$I"; fail=1; fi
  elif native_build_unavailable "$berr"; then
    echo "  SKIP $name (native leg: codegen unavailable)"
  else
    echo "  FAIL $name: native build FAILED (a build error is not a skip):"
    printf '%s\n' "$berr" | head -3 | sed 's/^/        /'
    fail=1
  fi
  # AOT-wasm
  if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
    echo "  SKIP $name (wasm build unavailable)"; continue
  fi
  L="${SRC%.ax}.linked.wasm"
  if [ ! -f "$L" ]; then echo "  FAIL $name: did NOT link (size_t ABI regressed)"; fail=1; continue; fi
  ran=$((ran+1))
  # `--invoke` prints the callee's return AFTER the experimental warning; the
  # probe's main prints the answer and returns nothing, so cut at the warning and
  # keep the program's own stdout.
  # NB 2>/dev/null, not 2>&1: the `eprint` case writes to stderr on purpose, and
  # merging it would fold "to stderr" into the answer. The experimental warning
  # is on stderr too, so discarding stderr both removes it and keeps the
  # program's own stderr out of the comparison -- the --invoke return value is
  # 0 for every probe (its main returns nothing) and prints on stdout, so take
  # the FIRST line, which is the program's println.
  W="$("$WASMRT" --invoke main "$L" 2>/dev/null | head -1 | tr -d '[:space:]')"
  if [ -z "$W" ]; then echo "  FAIL $name: wasm produced no output (trap?)"; fail=1; continue; fi
  if [ "$W" = "$I" ]; then
    echo "  OK   $name: interp=$I native=${N:-n/a} wasm=$W"
  else
    echo "  FAIL $name: wasm=$W != interp=$I"; fail=1
  fi
done

if [ "$ran" -eq 0 ]; then echo "wasm_malloc_abi_parity: nothing linked — skipping"; exit 0; fi
[ "$fail" -eq 0 ] || { echo "wasm_malloc_abi_parity: FAIL"; exit 1; }
echo "wasm_malloc_abi_parity: PASS — array + to_str + string-interpolation + eprintln (malloc/snprintf/memcpy/write size_t) run identically on interp, native, AOT-wasm ✓"
exit 0
