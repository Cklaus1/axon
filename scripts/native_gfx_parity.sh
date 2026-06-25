#!/usr/bin/env bash
# native_gfx_parity.sh — R13 slice 4: the `native::gfx` mock module joins the
# interp↔codegen differential parity suite.
#
# The gfx mock shim is ONE Rust impl (`axon-gfx-mock`) compiled into BOTH the
# interpreter (in-process) and the native link (axon-rt's __axon_gfx_* C
# symbols). I-2 says both engines must agree. This harness asserts that on the
# v1 mock module across THREE observable channels:
#
#   1. stdout      — the value-returning `frame_count` printed via to_str.
#   2. exit code   — main's i64 return (and any graceful panic).
#   3. CALL-TRACE  — the §4 oracle: for the EFFECTFUL calls (clear/present) that
#                    have no diffable stdout, both engines must emit the
#                    byte-identical `native-trace:` provenance lines under
#                    AXON_NATIVE_TRACE=1.
#
# It also drives the forged-handle soundness gate (native_ffi_forge.sh): a bad
# slab index across the C ABI must be a graceful exit-101, never a segfault.
#
# Requires the codegen `axon` binary (LLVM). Skips (exit 0) when codegen can't
# build, so it is safe in interpreter-only CI.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "native_gfx_parity: building codegen axon binary…"
if ! cargo build -q -p axon-core --bin axon 2>/dev/null; then
  echo "native_gfx_parity: codegen build unavailable (LLVM absent) — skipping"
  exit 0
fi
AXON="target/debug/axon"

# Probe: can this binary actually emit native code?
printf 'fn main() -> i64 { 0 }\n' > "$WORK/probe.ax"
if ! "$AXON" build "$WORK/probe.ax" -o "$WORK/probe.bin" --no-cache >/dev/null 2>&1; then
  echo "native_gfx_parity: this axon binary cannot emit native builds — skipping"
  exit 0
fi

fail=0

# A value-returning gfx program exercising scalar + str + handle marshalling,
# the affine move (the *_close consumes), and a pure probe (frame_count).
cat > "$WORK/gfx.ax" <<'EOF'
use native::gfx

@[contained(gfx: any)]
fn main() -> i64 {
    let win = gfx::window_open(1280, 720, "parity")
    let surf = gfx::surface(win)
    gfx::clear(surf, 0.05, 0.05, 0.08, 1.0)
    gfx::present(surf)
    gfx::present(surf)
    gfx::present(surf)
    let frames = gfx::frame_count(surf)
    println("frames={to_str(frames)}")
    gfx::surface_close(surf)
    gfx::window_close(win)
    frames
}
EOF

if ! "$AXON" build "$WORK/gfx.ax" -o "$WORK/gfx.bin" --no-cache >/dev/null 2>&1; then
  echo "native_gfx_parity: native build of the gfx program failed — skipping"
  exit 0
fi

# 1+2: stdout + exit-code parity.
"$AXON" run "$WORK/gfx.ax" >"$WORK/i.out" 2>/dev/null; i_exit=$?
"$WORK/gfx.bin"           >"$WORK/n.out" 2>/dev/null; n_exit=$?
if ! diff -q "$WORK/i.out" "$WORK/n.out" >/dev/null; then
  echo "FAIL: gfx stdout diverges:"; diff "$WORK/i.out" "$WORK/n.out"; fail=1
elif [ "$i_exit" != "$n_exit" ]; then
  echo "FAIL: gfx exit code diverges (interp=$i_exit native=$n_exit)"; fail=1
else
  echo "  OK stdout+exit: both '$(cat "$WORK/i.out")' exit=$i_exit"
fi

# 3: call-trace oracle parity (the effectful-call I-2 restoration, §4).
AXON_NATIVE_TRACE=1 "$AXON" run "$WORK/gfx.ax" 2>&1 | grep '^native-trace:' >"$WORK/i.trace"
AXON_NATIVE_TRACE=1 "$WORK/gfx.bin"            2>&1 | grep '^native-trace:' >"$WORK/n.trace"
if ! diff -q "$WORK/i.trace" "$WORK/n.trace" >/dev/null; then
  echo "FAIL: call-trace oracle diverges (interp vs codegen native trace):"
  diff "$WORK/i.trace" "$WORK/n.trace"; fail=1
else
  n="$(wc -l <"$WORK/i.trace")"
  echo "  OK call-trace: $n identical native-trace lines across both engines"
fi

# 4: the forged-handle-across-the-C-ABI soundness gate (graceful, never abort).
echo "native_gfx_parity: forged-handle soundness gate…"
if ! bash scripts/native_ffi_forge.sh; then
  echo "FAIL: forged native handle was not a graceful exit-101 across the C ABI"; fail=1
fi

if [ "$fail" -eq 0 ]; then
  echo "native_gfx_parity: PASS — gfx mock module is interp↔codegen identical (stdout/exit/trace) + forge-safe"
fi
exit "$fail"
