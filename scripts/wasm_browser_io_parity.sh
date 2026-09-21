#!/usr/bin/env bash
# wasm_browser_io_parity.sh — R7c (browser I/O): `println` works on the browser
# target via a host import, with STDOUT identical to the interpreter.
#
# A browser has no wasi, so println can't use stdout/fd_write. codegen lowers
# println to C `puts`; the unknown-unknown axon-rt shims `puts` to call an
# imported `axon_host_write(ptr,len)` that the JS/wasm-bindgen glue supplies. The
# link allows exactly that one symbol undefined (--allow-undefined-file), so the
# module stays wasi-free with a single host import. This harness drives it with a
# minimal Node host (scripts/wasm_browser_host.js, standing in for the browser
# glue) and asserts byte-identical stdout to the interpreter.
#
# Number/format printing (snprintf/malloc, not yet shimmed for the browser) is a
# follow-on — those programs honestly fall back to object-only. Skips (exit 0)
# when node / the wasm toolchain is absent.
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

command -v node >/dev/null 2>&1 || { echo "wasm_browser_io_parity: no node — skipping"; exit 0; }
HOSTJS="scripts/wasm_browser_host.js"
[ -f "$HOSTJS" ] || { echo "wasm_browser_io_parity: host harness missing — skipping"; exit 0; }

AXON="${AXON:-target/debug/axon}"
if [ ! -x "$AXON" ]; then
  cargo build -q -p axon-core --bin axon 2>/dev/null || { echo "wasm_browser_io_parity: codegen unavailable — skipping"; exit 0; }
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
rust_target_installed wasm32-unknown-unknown; _rt=$?
if [ "$_rt" -eq 2 ]; then
  echo "wasm_browser_io_parity: cannot determine whether wasm32-unknown-unknown is installed — refusing to call that a skip" >&2
  exit 1
elif [ "$_rt" -ne 0 ]; then
  echo "wasm_browser_io_parity: wasm32-unknown-unknown target not installed - skipping"; exit 0
fi
if ! _rt_err="$(cargo build -q -p axon-rt --target wasm32-unknown-unknown 2>&1)"; then
  echo "wasm_browser_io_parity: FAIL - axon-rt does NOT build for wasm32-unknown-unknown (target IS installed):"
  echo "$_rt_err" | sed 's/^/    | /'
  exit 1
fi
PROBE="$(mktemp -d)/p.ax"; printf 'fn main() { println("ok") }\n' > "$PROBE"
# Report WHY the link failed instead of calling every cause "unavailable".
# E0907 means this `axon` was built --no-default-features, i.e. another harness
# clobbered the shared target/debug/axon with a codegen-less binary. That is a
# suite bug, not an absent toolchain, and it silently un-asserted this harness.
_probe_err="$("$AXON" target build "$PROBE" --target wasm32-unknown-unknown 2>&1)"
if [ ! -f "${PROBE%.ax}.linked.wasm" ]; then
  if echo "$_probe_err" | grep -q E0907; then
    echo "wasm_browser_io_parity: FAIL — \$AXON ($AXON) has no codegen backend (E0907)."
    echo "    A --no-default-features build clobbered the shared target/debug/axon."
    exit 1
  fi
  echo "wasm_browser_io_parity: browser println link unavailable — skipping"; exit 0
fi

# String-println programs (the path that lowers to puts → host write).
declare -A PROGS
PROGS[hello]='fn main() { println("hello from the browser") }'
PROGS[multi]='fn main() { println("line one")  println("line two")  println("line three") }'
PROGS[strop]='fn main() { println(str_to_upper("shout"))  println(str_reverse("abc")) }'
PROGS[nums]='fn main() { println(to_str(42))  println(to_str(0 - 7))  println("sum={6 * 7}") }'
PROGS[floats]='fn main() { println(to_str_f64(3.14))  println(to_str_f64(1000000.0))  println("pi={3.14159}") }'

WORK="$(mktemp -d)"; trap 'rm -rf "$WORK"' EXIT
pass=0; fail=0; ran=0
for name in "${!PROGS[@]}"; do
  src="$WORK/$name.ax"; printf '%s\n' "${PROGS[$name]}" > "$src"
  I_OUT="$("$AXON" run "$src" 2>/dev/null)"
  if ! "$AXON" target build "$src" --target wasm32-unknown-unknown >/dev/null 2>&1; then
    echo "  SKIP $name (build failed)"; continue
  fi
  linked="${src%.ax}.linked.wasm"
  [ -f "$linked" ] || { echo "  SKIP $name (object-only)"; continue; }
  ran=$((ran+1))
  if strings "$linked" | grep -qi wasi_snapshot; then
    echo "  FAIL $name: imports WASI — not browser-safe"; fail=$((fail+1)); continue
  fi
  W_OUT="$(node "$HOSTJS" "$linked" 2>/dev/null)"
  if [ "$I_OUT" = "$W_OUT" ]; then
    echo "  OK   $name: stdout matches (wasi-free, via JS host)"; pass=$((pass+1))
  else
    echo "  DIFF $name: interp=[$I_OUT] browser=[$W_OUT]"; fail=$((fail+1))
  fi
done

# Gate the committed end-to-end browser demo (examples/browser/demo.ax) too, so
# the user-facing artifact can't rot.
DEMO="examples/browser/demo.ax"
if [ -f "$DEMO" ]; then
  D_I="$("$AXON" run "$DEMO" 2>/dev/null)"
  rm -f examples/browser/demo.linked.wasm examples/browser/demo.wasm
  if "$AXON" target build "$DEMO" --target wasm32-unknown-unknown >/dev/null 2>&1 && [ -f examples/browser/demo.linked.wasm ]; then
    ran=$((ran+1))
    D_W="$(node "$HOSTJS" examples/browser/demo.linked.wasm 2>/dev/null)"
    if [ "$D_I" = "$D_W" ]; then
      echo "  OK   demo (examples/browser/demo.ax): stdout matches via JS host"; pass=$((pass+1))
    else
      echo "  DIFF demo: interp=[$D_I] browser=[$D_W]"; fail=$((fail+1))
    fi
    rm -f examples/browser/demo.linked.wasm examples/browser/demo.wasm
  else
    echo "  SKIP demo (object-only)"
  fi
fi

echo "wasm_browser_io_parity: $pass/$ran stdout-matched (wasi-free), $fail bad"
if [ "$ran" -eq 0 ]; then echo "wasm_browser_io_parity: nothing linked — skipping"; exit 0; fi
[ "$fail" -eq 0 ] || exit 1
if [ "$ran" -lt "${#PROGS[@]}" ]; then
  echo "wasm_browser_io_parity: FAIL — only $ran/${#PROGS[@]} linked; a browser println-link regression silently skipped the rest"; exit 1
fi
echo "wasm_browser_io_parity: PASS — $pass println programs run on the browser target (wasi-free) with stdout identical to the interpreter ✓"
exit 0
