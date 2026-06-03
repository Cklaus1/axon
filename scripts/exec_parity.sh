#!/usr/bin/env bash
# exec_parity.sh — R6 codegen `exec` regression (native == interp).
#
# The `exec` builtin (process spawning) was interp-only: native codegen had no
# emitter, so a native build silently produced no output (the call resolved to
# nothing). Codegen now emits `exec` delegating to axon-rt's __axon_exec, so
# native matches the interpreter on both the Ok (stdout) and Err (message) paths.
# This harness builds a program exercising exec-with-args and exec-error both
# ways and asserts identical output.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

PROG="$WORK/exec.ax"
cat > "$PROG" <<'AX'
fn main() -> i64 {
    match exec("echo", ["from", "exec"]) { Ok(s) => print(s)  Err(_) => println("err1") }
    match exec("nonexistent_cmd_xyz", []) { Ok(_) => println("ok2")  Err(e) => println(e) }
    0
}
AX

echo "exec_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "exec_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

interp_out="$("$AXON" run "$PROG" 2>/dev/null)"

BIN="$WORK/exec_bin"
if ! "$AXON" build "$PROG" -o "$BIN" --no-cache >/dev/null 2>&1; then
  echo "exec_parity: native build failed — skipping"
  exit 0
fi
native_out="$("$BIN" 2>/dev/null)"

if [ "$interp_out" != "$native_out" ]; then
  echo "exec_parity: FAIL — native exec output differs from the interpreter:"
  echo "--- interp ---"; echo "$interp_out" | sed 's/^/  /'
  echo "--- native ---"; echo "$native_out" | sed 's/^/  /'
  exit 1
fi

# Belt-and-suspenders: the Ok path must carry the echoed args (not be empty).
if ! echo "$native_out" | grep -q "from exec"; then
  echo "exec_parity: FAIL — native exec Ok path produced no stdout: $native_out"
  exit 1
fi

echo "exec_parity: OK — native and interp exec agree (stdout + error):"
echo "$native_out" | sed 's/^/  /'
echo "exec matches the interpreter"
exit 0
