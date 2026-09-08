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
[ -n "$WASMRT" ] || { echo "wasm_aot_run_parity: no wasm runtime — skipping"; exit 0; }

if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "wasm_aot_run_parity: codegen build unavailable — skipping"; exit 0
fi
if ! cargo build -q -p axon-core --no-default-features --bin axon-run 2>/dev/null; then
  echo "wasm_aot_run_parity: interp build unavailable — skipping"; exit 0
fi
AXON="${AXON:-target/debug/axon}"
INTERP="target/debug/axon-run"
WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT

# AUDIT O013: give this harness its OWN cargo target directory for the wasm
# builds below. Several wasm harnesses run `--target wasm32-*` concurrently under
# the full test suite and contended on the shared target/, so some builds failed
# and their cases reported "wasm build failed" — the test then correctly refused
# to pass on partial coverage, but the cause was contention, not a real parity
# break. Set AFTER the two host `cargo build` calls above so those still reuse
# the shared cache and are not rebuilt into the temp dir.
#
# Same isolation the SMT harnesses already use (smt_discharge_parity.sh:41).
export CARGO_TARGET_DIR="$WORK/wasm-target"

# The AOT-wasm-linkable subset: programs that, after dead-function pruning, have
# no SURVIVING i64-ABI `__axon_*` import that clashes with wasm32's i32 libc — so
# they link in reactor mode and run under `wasmtime --invoke main`. This is wider
# than "pure-int": recursion, while/reassignment loops, f64 math (LLVM intrinsics,
# no extern), arrays-of-i64 + closures (inline-IR reductions), str builtins
# (incl. str_split/str_join — see wasm_str_abi_parity.sh), and the int-counter
# dict pattern (dict_new/set/get/inc/get_or/len) all link+run. The gap was never
# a deep "i64→i32 retarget" — it was MISSING `#[cfg(target_arch="wasm32")]` extern
# variants; adding them (scalar-expanded for AxonStr-by-value args) unblocks each.
# The ENTIRE dict API now links+runs on wasm (all 17 __axon_dict_* externs,
# incl. the closure-taking map_values/filter/each — the closure's by-value AxonStr
# key is expanded to (i64 len, i32 ptr) scalars in the wasm transmute, matching
# codegen's lambda ABI). Object-only on wasm now = only programs hitting a builtin
# whose extern still lacks a wasm variant (audit case-by-case if a new one appears).
declare -A PROGS
PROGS[fib]='fn f(n: i64) -> i64 { if n < 2 { n } else { f(n-1) + f(n-2) } }
fn main() -> i64 { f(10) }'
PROGS[arith]='fn main() -> i64 { (21 + 21) * 2 - 4 }'
PROGS[float]='fn main() -> i64 { f64_to_i64(sqrt(16.0)) }'
PROGS[array]='fn main() -> i64 { let xs = [1, 2, 3]  let ys = arr_map(xs, |x| x * 10)  arr_sum_i64(&ys) }'
PROGS[dict]='fn main() -> i64 {
    let d = dict_new()
    dict_set(d, "a", 10)
    dict_set(d, "b", 20)
    dict_inc(d, "a")
    let h = if dict_has(d, "a") { 1 } else { 0 }
    let ks = dict_keys(d)
    let vs = dict_values(d)
    let removed = dict_remove(d, "b")
    let r = match removed { Some(v) => v  None => 0 }
    dict_get_or(d, "a", 0) + len(ks) + len(vs) + h + r + dict_len(d)
}'
PROGS[dict_closure]='fn main() -> i64 {
    let d = dict_new()
    dict_set(d, "a", 1)
    dict_set(d, "bb", 2)
    dict_set(d, "ccc", 3)
    let doubled = dict_map_values(d, |v| v * 10)
    let da = dict_get_or(doubled, "a", 0)
    let kept = dict_filter(d, |k, v| str_len(k) > 1)
    da + dict_len(kept) + dict_len(d)
}'
# NB: Axon has no `let mut` — declare with `let`, reassign with bare `x = …`.
# The old `let mut` form parse-errored, so this case SILENTLY SKIPPED forever
# (build-failed → SKIP), never actually testing a loop on wasm (a vacuous skip).
PROGS[loop]='fn main() -> i64 { let s = 0  let i = 1  while i <= 10 { s = s + i  i = i + 1 }  s }'

pass=0; fail=0; ran=0
for name in "${!PROGS[@]}"; do
  src="$WORK/$name.ax"; printf '%s\n' "${PROGS[$name]}" > "$src"
  # Read the interpreter's answer by INVOKING main the same way wasmtime does,
  # rather than off the process exit code. `main`'s i64 return is not the exit
  # code: values in 2..=15 and 101 are remapped to 1, as is anything outside
  # 0..=255 (governance/EXIT_CODES.md). Two of the seven cases below were landing
  # in that band and comparing 1 against 1 -- `float` truly returns 4 and
  # `dict_closure` truly returns 15, and the harness would have printed OK for
  # ANY wasm answer that also remapped to 1. Renaming main->probe and printing
  # the value makes the comparison mean what the OK line claims.
  probe_src="${src%.ax}_probe.ax"
  sed 's/fn main() -> i64 {/fn probe() -> i64 {/' "$src" > "$probe_src"
  printf 'fn main() { println(to_str(probe())) }\n' >> "$probe_src"
  i_exit="$("$INTERP" "$probe_src" 2>/dev/null | grep -v '^axon: run-id ' | tail -1)"
  # BOTH engines must read the same way. wasm implements the SAME exit remap, so
  # rewriting only the interp side manufactures a divergence that is not there.
  if [ -z "$i_exit" ]; then
    echo "  DIFF $name: interp printed nothing (probe build broken?)"; fail=$((fail+1)); continue
  fi
  # AOT-wasm: build (which links if pure-int) then run.
  if ! "$AXON" target build --engine codegen --target wasm32-wasip1 "$probe_src" >/dev/null 2>&1; then
    echo "  SKIP $name (wasm build failed)"; continue
  fi
  linked="${probe_src%.ax}.linked.wasm"
  if [ ! -f "$linked" ]; then echo "  SKIP $name (not linkable — has runtime externs)"; continue; fi
  ran=$((ran+1))
  # `--invoke` prints the callee's return value AFTER the experimental warning.
  # The probe's main returns nothing and PRINTS the answer, so cut at the warning
  # and take the program's own stdout. Filtering the warning by content instead
  # would also delete a legitimate program line.
  w_out="$("$WASMRT" --invoke main "$linked" 2>&1 | sed '/experimental/,$d' | tr -d '[:space:]')"
  # Both sides are now the full i64 value -- no mod 256, no exit remap.
  if [ "$((i_exit))" = "$((w_out))" ]; then
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
echo "wasm_aot_run_parity: PASS — AOT wasm runs identically to the interpreter across the linkable subset (recursion/loop/f64/array+closure/full dict API incl. map_values/filter/each) ✓"
exit 0
