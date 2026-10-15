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
. "$ROOT/scripts/lib/axon_bin.sh"
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

if [ -n "${AXON:-}" ]; then
  named_bin AXON wasm_browser_io_parity
else
  echo "wasm_browser_io_parity: building codegen axon binary…"
  cargo_build_or_skip wasm_browser_io_parity llvm cargo build -q -p axon-core --bin axon
  use_built AXON axon
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
# E0907 means this `axon` has no codegen backend: a skip when $AXON is an
# interp-only build (require_codegen_axon), a FAIL when it can build native code
# and still answers E0907. Neither is an absent toolchain.
_probe_err="$("$AXON" target build "$PROBE" --target wasm32-unknown-unknown 2>&1)"; _probe_rc=$?
if [ ! -f "${PROBE%.ax}.linked.wasm" ]; then
  if echo "$_probe_err" | grep -q E0907; then
    require_codegen_axon wasm_browser_io_parity
    echo "wasm_browser_io_parity: FAIL — \$AXON ($AXON) builds native code yet answered E0907:"
    printf '%s\n' "$_probe_err" | head -5 | sed 's/^/        /'
    exit 1
  fi
  # The target and axon-rt for it were proven above. A build that FAILED is a
  # result; only a build that succeeded without producing a linked module (the
  # environment's wasm link is unavailable) is a skip.
  if [ "$_probe_rc" -ne 0 ]; then
    native_build_failed wasm_browser_io_parity "browser probe build" "$_probe_err" || exit 1
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
  if ! werr="$("$AXON" target build "$src" --target wasm32-unknown-unknown 2>&1)"; then
    # The probe above linked a module, so the toolchain is here: a failed
    # build is a result unless stderr proves an unavailability.
    if native_build_unavailable "$werr"; then
      echo "  SKIP $name (build unavailable)"; continue
    fi
    echo "  FAIL $name: browser-wasm build FAILED (a build error is not a skip):"
    printf '%s\n' "$werr" | head -5 | sed 's/^/        /'
    fail=$((fail+1)); continue
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
  if ! derr="$("$AXON" target build "$DEMO" --target wasm32-unknown-unknown 2>&1)" \
     && ! native_build_unavailable "$derr"; then
    echo "  FAIL demo: browser-wasm build FAILED (a build error is not a skip):"
    printf '%s\n' "$derr" | head -5 | sed 's/^/        /'
    fail=$((fail+1))
  elif [ -f examples/browser/demo.linked.wasm ]; then
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
# A failure decides first: with every case FAILED, ran is 0 too.
[ "$fail" -eq 0 ] || exit 1
if [ "$ran" -eq 0 ]; then echo "wasm_browser_io_parity: nothing linked — skipping"; exit 0; fi
if [ "$ran" -lt "${#PROGS[@]}" ]; then
  echo "wasm_browser_io_parity: FAIL — only $ran/${#PROGS[@]} linked; a browser println-link regression silently skipped the rest"; exit 1
fi
echo "wasm_browser_io_parity: PASS — $pass println programs run on the browser target (wasi-free) with stdout identical to the interpreter ✓"
exit 0
