#!/usr/bin/env bash
# wasm_browser_parity.sh — R7c (browser target): `axon target build --target
# wasm32-unknown-unknown` produces a genuinely WASI-FREE wasm module that runs
# the value identically to the interpreter.
#
# wasm32-unknown-unknown is the BROWSER target — a browser has no WASI, so the
# module must import NOTHING from wasi_snapshot_preview1 (unlike the wasip1
# headless target, which links the wasi libc). The fix: try_link_wasm links the
# unknown-unknown axon-rt and NO wasi libc for this triple. This harness proves,
# per compute/str program: (1) it LINKS, (2) the linked wasm is wasi-FREE, and
# (3) `wasmtime --invoke main` returns the interpreter's value.
#
# v1 is compute + str (no I/O): a program that PRINTS needs a stdout sink the
# browser provides via JS glue (wasm-bindgen), not wasi — those honestly fall
# back to object-only. Skips (exit 0) when the toolchain is absent.
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
[ -n "$WASMRT" ] || { echo "wasm_browser_parity: no wasm runtime — skipping"; exit 0; }

AXON="${AXON:-target/debug/axon}"
if [ ! -x "$AXON" ]; then
  cargo build -q -p axon-core --bin axon 2>/dev/null || { echo "wasm_browser_parity: codegen unavailable — skipping"; exit 0; }
fi
# The browser (unknown-unknown) axon-rt must be built for the wasi-free link.
# Distinguish an ABSENT target from a BROKEN build. The probe used to be
# `if ! cargo build ... 2>/dev/null` reporting "unavailable - skipping", which
# said the same thing for both and threw away the compiler error naming which.
# That is how a `data:`/`ptr:` typo in a `#[cfg(target_arch = "wasm32")]` arm
# (4a600f2) took all 8 wasm_* harnesses dark for a day while parity_all printed
# "SKIP (toolchain absent)" -- both wasm32 targets were installed the whole time.
# A skip must be honest about WHY, or it is a silent loss of coverage.
if ! rustup target list --installed 2>/dev/null | grep -qx "wasm32-unknown-unknown"; then
  echo "wasm_browser_parity: wasm32-unknown-unknown target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-unknown-unknown 2>&1)"; then
  echo "wasm_browser_parity: FAIL - axon-rt does NOT build for wasm32-unknown-unknown (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
# Probe: can this binary link an unknown-unknown module at all?
PROBE="$(mktemp -d)/p.ax"; printf 'fn main() -> i64 { 7 }\n' > "$PROBE"
# Report WHY the link failed instead of calling every cause "unavailable".
# E0907 means this `axon` was built --no-default-features, i.e. another
# harness clobbered the shared target/debug/axon with a codegen-less binary.
# That is a suite bug, not an absent toolchain, and it silently un-asserted
# this harness while parity_all printed "SKIP (toolchain absent)".
_probe_err="$("$AXON" target build "$PROBE" --target wasm32-unknown-unknown 2>&1)"
if [ ! -f "${PROBE%.ax}.linked.wasm" ]; then
  if echo "$_probe_err" | grep -q E0907; then
    echo "wasm_browser_parity: FAIL — \$AXON ($AXON) has no codegen backend (E0907)."
    echo "    A --no-default-features build clobbered the shared target/debug/axon."
    exit 1
  fi
  echo "wasm_browser_parity: unknown-unknown link unavailable — skipping"; exit 0
fi

declare -A PROGS
PROGS[arith]='fn main() -> i64 { (21 + 21) * 2 - 4 }'
PROGS[loop]='fn main() -> i64 { let s = 0  let i = 1  while i <= 10 { s = s + i  i = i + 1 }  s }'
# The values below are read off an EXIT CODE, which cannot represent 2..=15 or
# 101 -- a `main` returning one of those exits 1 instead
# (governance/EXIT_CODES.md). `str` used to total 5 and `dict` 7: both engines
# answered 1, agreed, and printed OK without either ever computing the right
# number. The other harnesses in this class fixed it by printing the value from a
# non-`main` probe, but a browser module is wasi-free (no stdout) and the linker
# exports only `main` (`--export=main`, main.rs:2752), so there is nothing to
# print with and nothing else to invoke. The values are scaled out of the band
# instead, and the guard below makes that a checked property rather than luck.
PROGS[str]='fn main() -> i64 { let u = str_to_upper("hi")  let r = str_reverse("abc")  str_len(u) * 100 + str_len(r) * 10 }'
PROGS[dict]='fn main() -> i64 { let d = dict_new()  dict_set(d, "a", 5)  dict_inc(d, "a")  dict_get_or(d, "a", 0) * 30 + dict_len(d) }'

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
pass=0; fail=0; ran=0
for name in "${!PROGS[@]}"; do
  src="$WORK/$name.ax"; printf '%s\n' "${PROGS[$name]}" > "$src"
  "$AXON" run "$src" >/dev/null 2>&1; i_exit=$?
  if ! "$AXON" target build "$src" --target wasm32-unknown-unknown >/dev/null 2>&1; then
    echo "  SKIP $name (build failed)"; continue
  fi
  linked="${src%.ax}.linked.wasm"
  [ -f "$linked" ] || { echo "  SKIP $name (object-only — needs JS glue / allocator)"; continue; }
  ran=$((ran+1))
  # A value the exit code cannot carry makes this case blind: both engines would
  # report 1 and agree. Fail rather than pass vacuously -- rescale the program.
  if [ "$i_exit" -eq 1 ]; then
    echo "  FAIL $name: interp exited 1 — either a real failure, or a return value"
    echo "       in the reserved 2..=15/101 band (or outside 0..=255), which the"
    echo "       exit code collapses to 1. This case cannot see a wrong answer."
    fail=$((fail+1)); continue
  fi
  # (2) WASI-FREE: a browser module must import nothing from wasi.
  if strings "$linked" | grep -qi wasi_snapshot; then
    echo "  FAIL $name: module imports WASI — not browser-safe"; fail=$((fail+1)); continue
  fi
  # (3) correct value.
  w_out="$("$WASMRT" --invoke main "$linked" 2>/dev/null | grep -vi experimental | head -1)"
  if [ "$((i_exit))" = "$((w_out % 256))" ] 2>/dev/null; then
    echo "  OK   $name: interp=$i_exit browser-wasm=$w_out (wasi-free)"; pass=$((pass+1))
  else
    echo "  DIFF $name: interp=$i_exit browser-wasm=$w_out"; fail=$((fail+1))
  fi
done

echo "wasm_browser_parity: $pass/$ran wasi-free + value-matched, $fail bad"
if [ "$ran" -eq 0 ]; then echo "wasm_browser_parity: nothing linked — skipping"; exit 0; fi
[ "$fail" -eq 0 ] || exit 1
# All 4 programs are compute/str (no I/O) and MUST link wasi-free on the browser
# target; if fewer ran, something regressed the unknown-unknown link.
if [ "$ran" -lt "${#PROGS[@]}" ]; then
  echo "wasm_browser_parity: FAIL — only $ran/${#PROGS[@]} linked; a browser-link regression silently skipped the rest"; exit 1
fi
echo "wasm_browser_parity: PASS — $pass compute/str programs compile to WASI-FREE wasm32-unknown-unknown and match the interpreter ✓"
exit 0
