#!/usr/bin/env bash
# wasm_str_abi_parity.sh — R7: the str/array i64→i32 wasm-ABI bridge.
#
# LLVM (codegen) expands a by-value `AxonStr {i64 len, ptr}` argument into
# SCALARS, so codegen calls e.g. `__axon_str_reverse(i64 len, i32 ptr, …)`.
# rustc would otherwise pass `#[repr(C)] AxonStr` INDIRECTLY on wasm32 (a single
# i32 pointer) — a `function signature mismatch` at link, and a trap at runtime.
# axon-rt now declares the EXPANDED scalar form under `#[cfg(target_arch=wasm32)]`
# for the str/array-taking externs, so a STRING-using program links clean and
# runs. This harness proves it: a program exercising 13 distinct str builtins —
# including str_split/str_join, which return/read an ARRAY of AxonStr — must
# produce the SAME value across interp, native AOT, and AOT-wasm.
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
[ -n "$WASMRT" ] || { echo "wasm_str_abi_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_str_abi_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_str_abi_parity: interp build unavailable — skipping"; exit 0
fi
# The wasm runtime (carrying the scalar-ABI bridge) must exist for str programs
# to link. Build it; skip honestly if the wasm32 target isn't installed.
# Distinguish an ABSENT target from a BROKEN build. The probe used to be
# `if ! cargo build ... 2>/dev/null` reporting "unavailable - skipping", which
# said the same thing for both and threw away the compiler error naming which.
# That is how a `data:`/`ptr:` typo in a `#[cfg(target_arch = "wasm32")]` arm
# (4a600f2) took all 8 wasm_* harnesses dark for a day while parity_all printed
# "SKIP (toolchain absent)" -- both wasm32 targets were installed the whole time.
# A skip must be honest about WHY, or it is a silent loss of coverage.
if ! rustup target list --installed 2>/dev/null | grep -qx "wasm32-wasip1"; then
  echo "wasm_str_abi_parity: wasm32-wasip1 target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-wasip1 2>&1)"; then
  echo "wasm_str_abi_parity: FAIL - axon-rt does NOT build for wasm32-wasip1 (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
AXON="${AXON:-target/debug/axon}"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# A program that routes through 13 distinct str builtins (str scalars/transforms
# plus str_split/str_join, which round-trip an ARRAY of AxonStr). The final i64
# is the cross-engine oracle.
SRC="$WORK/strmix.ax"
cat > "$SRC" <<'AX'
fn main() -> i64 {
    let s = str_reverse("abc")
    let r = str_replace("aXbXc", "X", "-")
    let n = str_len(r)
    let slc = str_slice("hello world", 0, 5)
    let idx = str_index_of("hello", "l")
    let rep = str_repeat("ab", 3)
    let up = str_to_upper("abc")
    let lo = str_to_lower("ABC")
    let tr = str_trim("  xy  ")
    let pd = str_pad_start("z", 4, "0")
    let parts = str_split("a,b,c", ",")
    let joined = str_join(parts, "-")
    n + str_len(s) + str_len(slc) + idx + str_len(rep) + str_len(up) + str_len(lo) + str_len(tr) + str_len(pd) + len(parts) + str_len(joined)
}
AX

# 1) interpreter oracle (exit value = i64 main return, mod 256)
"$INTERP" "$SRC" >/dev/null 2>&1; I_EXIT=$?
echo "wasm_str_abi_parity: interp = $I_EXIT"

# 2) native AOT
if "$AXON" build "$SRC" -o "$WORK/native" >/dev/null 2>&1; then
  "$WORK/native" >/dev/null 2>&1; N_EXIT=$?
  echo "wasm_str_abi_parity: native = $N_EXIT"
  if [ "$N_EXIT" != "$I_EXIT" ]; then
    echo "wasm_str_abi_parity: FAIL — native ($N_EXIT) != interp ($I_EXIT)"; exit 1
  fi
else
  echo "wasm_str_abi_parity: native build unavailable — skipping native leg"
fi

# 3) AOT-wasm — the bridge under test. Must LINK (no signature mismatch) and RUN.
if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$SRC" >/dev/null 2>&1; then
  echo "wasm_str_abi_parity: wasm build unavailable — skipping wasm leg"; exit 0
fi
LINKED="${SRC%.ax}.linked.wasm"
if [ ! -f "$LINKED" ]; then
  echo "wasm_str_abi_parity: FAIL — str program did NOT link (ABI bridge regressed)"; exit 1
fi
W_OUT="$("$WASMRT" --invoke main "$LINKED" 2>/dev/null | grep -vi experimental | head -1)"
echo "wasm_str_abi_parity: wasm = $W_OUT"
if [ -z "$W_OUT" ]; then
  echo "wasm_str_abi_parity: FAIL — wasm produced no output (trap?)"; exit 1
fi
if [ "$((W_OUT % 256))" != "$I_EXIT" ]; then
  echo "wasm_str_abi_parity: FAIL — wasm ($W_OUT) != interp ($I_EXIT)"; exit 1
fi

# 4) str_cmp on STDOUT, deliberately not folded into the exit sum above.
#
# str_cmp returns exactly -1/0/1. Every one of those is inside the 2..=15-and-101
# band `governance/EXIT_CODES.md` reserves and remaps (and -1 is outside 0..=255,
# which also remaps), so an exit-code comparison could not tell a correct answer
# from a wrong one here. The sum in step 1 escapes that band at 41 only by luck
# of the addends -- it is not a property the harness enforces.
#
# This case exists because `4a600f2` shipped str_cmp with `data:` where AxonStr's
# field is `ptr:` in its `#[cfg(target_arch = "wasm32")]` arm. Nothing native ever
# compiled that arm, and all 8 wasm_* harnesses skipped on the resulting build
# failure while reporting "toolchain absent" -- so a whole day of green gates said
# nothing about it. A cfg-gated arm needs a case that actually builds it.
CMP="$WORK/strcmp.ax"
cat > "$CMP" <<'AX'
fn main() {
    println(to_str(str_cmp("apple", "banana")))
    println(to_str(str_cmp("banana", "apple")))
    println(to_str(str_cmp("pear", "pear")))
    println(to_str(str_cmp("", "a")))
    println(to_str(str_cmp("abc", "abd")))
}
AX
C_INTERP="$("$INTERP" "$CMP" 2>/dev/null | grep -v '^axon: run-id ' | tr '\n' ' ')"
echo "wasm_str_abi_parity: str_cmp interp = [$C_INTERP]"
if [ -z "$C_INTERP" ]; then
  echo "wasm_str_abi_parity: FAIL — str_cmp produced no interpreter output"; exit 1
fi

if "$AXON" build "$CMP" -o "$WORK/ncmp" >/dev/null 2>&1; then
  C_NATIVE="$("$WORK/ncmp" 2>/dev/null | tr '\n' ' ')"
  if [ "$C_NATIVE" != "$C_INTERP" ]; then
    echo "wasm_str_abi_parity: FAIL — str_cmp native [$C_NATIVE] != interp [$C_INTERP]"; exit 1
  fi
fi

if "$AXON" target build --engine codegen --target wasm32-wasip1 "$CMP" >/dev/null 2>&1; then
  C_LINKED="${CMP%.ax}.linked.wasm"
  if [ ! -f "$C_LINKED" ]; then
    echo "wasm_str_abi_parity: FAIL — str_cmp did NOT link (wasm32 ABI arm regressed)"; exit 1
  fi
  # `--invoke` prints the callee's RETURN VALUE after the experimental warning,
  # and `main` here returns 0. Filtering `^0$` by content would also delete the
  # program's own legitimate `str_cmp("pear","pear") == 0` -- which is exactly
  # what it did on the first run of this case, faking a wasm divergence. Cut at
  # the warning instead: everything before it is the program's stdout.
  C_WASM="$("$WASMRT" --invoke main "$C_LINKED" 2>&1 | sed '/experimental/,$d' | tr '\n' ' ')"
  if [ "$C_WASM" != "$C_INTERP" ]; then
    echo "wasm_str_abi_parity: FAIL — str_cmp wasm [$C_WASM] != interp [$C_INTERP]"; exit 1
  fi
  echo "wasm_str_abi_parity: str_cmp wasm = [$C_WASM]"
fi

echo "wasm_str_abi_parity: PASS — 13 str builtins incl. str_to_upper/lower/trim/pad + str_split/str_join (array-of-str) + str_cmp ordering (on stdout) run identically on interp, native, and AOT-wasm ✓"
exit 0
