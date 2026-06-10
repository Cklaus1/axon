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
ROOT="$(cd "$(dirname "$0")/.." && pwd)"; cd "$ROOT"

command -v node >/dev/null 2>&1 || { echo "wasm_browser_io_parity: no node — skipping"; exit 0; }
HOSTJS="scripts/wasm_browser_host.js"
[ -f "$HOSTJS" ] || { echo "wasm_browser_io_parity: host harness missing — skipping"; exit 0; }

AXON="target/debug/axon"
if [ ! -x "$AXON" ]; then
  cargo build -q -p axon-core --bin axon 2>/dev/null || { echo "wasm_browser_io_parity: codegen unavailable — skipping"; exit 0; }
fi
if ! cargo build -q -p axon-rt --target wasm32-unknown-unknown 2>/dev/null; then
  echo "wasm_browser_io_parity: unknown-unknown axon-rt unavailable — skipping"; exit 0
fi
PROBE="$(mktemp -d)/p.ax"; printf 'fn main() { println("ok") }\n' > "$PROBE"
"$AXON" target build "$PROBE" --target wasm32-unknown-unknown >/dev/null 2>&1
[ -f "${PROBE%.ax}.linked.wasm" ] || { echo "wasm_browser_io_parity: browser println link unavailable — skipping"; exit 0; }

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
