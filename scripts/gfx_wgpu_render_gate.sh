#!/usr/bin/env bash
# R13 Slice 5 acceptance gate — the FIRST REAL native module (`axon-gfx` on wgpu).
#
# Builds axon-core with the OFF-BY-DEFAULT `gfx-wgpu` feature, then runs an .ax
# program that:
#   * opens an OFFSCREEN render target (no winit window, no display),
#   * clears it to a KNOWN color (r=0, g=128/255, b=1, a=1),
#   * presents the frame,
#   * reads back the top-left pixel and asserts it equals the cleared color
#     packed 0xRRGGBBAA = 0x0080FFFF (= 8454143).
#
# This is the native analog of the spec's browser `…_clear_renders_frame`: a real
# wgpu frame verified by offscreen pixel read-back. It runs HEADLESSLY on Mesa
# lavapipe (software Vulkan) — the env vars below pin that backend.
#
# GUARD: if lavapipe / a usable Vulkan ICD is unavailable, SKIP gracefully (exit
# 0) so the default gate never breaks on a GPU-less host — exactly like the QEMU
# boot tests. On THIS host lavapipe is present and the gate runs for real.
#
# Usage: scripts/gfx_wgpu_render_gate.sh
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
cd "$REPO"

skip() { echo "SKIP: $*" >&2; exit 0; }
fail() { echo ""; echo "❌ gfx-wgpu render gate FAILED: $*" >&2; exit 1; }

AX="examples/native/gfx_render.ax"
WANT_PIXEL=8454143   # 0x0080FFFF — r=0x00 g=0x80 b=0xFF a=0xFF

# ── Pin the headless software-Vulkan backend (Mesa lavapipe) ──────────────────
LVP_ICD="${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.json}"
export WGPU_BACKEND="${WGPU_BACKEND:-vulkan}"

# ── GUARD: a usable Vulkan ICD must exist, else skip (don't break the gate) ────
if [[ ! -f "$LVP_ICD" ]]; then
    # Fall back to any installed ICD; if none, skip.
    if compgen -G "/usr/share/vulkan/icd.d/*.json" >/dev/null 2>&1; then
        unset VK_ICD_FILENAMES   # let the loader auto-discover
    else
        skip "no Vulkan ICD found (install Mesa lavapipe for headless rendering)"
    fi
else
    export VK_ICD_FILENAMES="$LVP_ICD"
fi

[[ -f "$AX" ]] || fail "missing $AX"

# ── Build the interpreter with the real wgpu backend (gfx-wgpu, no codegen) ───
echo "→ building axon-run with --features gfx-wgpu (real wgpu offscreen renderer)…"
if ! cargo build -p axon-core --no-default-features --features gfx-wgpu --bin axon-run \
        >/tmp/gfx_wgpu_build.log 2>&1; then
    cat /tmp/gfx_wgpu_build.log >&2
    fail "axon-run build with --features gfx-wgpu did not compile"
fi
AXON_RUN="$REPO/target/debug/axon-run"
[[ -x "$AXON_RUN" ]] || fail "axon-run binary not produced"

# ── Render headlessly + read back the pixel ───────────────────────────────────
echo "→ rendering offscreen frame on $WGPU_BACKEND (lavapipe) and reading back the pixel…"
OUT="$("$AXON_RUN" run "$AX" 2>&1)"
RC=$?
echo "$OUT"

# A device-acquisition failure (no usable adapter even with the ICD) is a clean
# refusal — SKIP rather than fail, so a flaky/headless host never breaks the gate.
if echo "$OUT" | grep -qi "no usable wgpu adapter\|device request failed"; then
    skip "wgpu could not acquire a headless adapter on this host"
fi

# Vacuous-pass guard #1: assert a REAL adapter was actually used (the backend
# announces it once). No adapter line ⇒ the render never touched a GPU backend.
if ! echo "$OUT" | grep -q "axon-gfx: wgpu adapter ="; then
    fail "no 'axon-gfx: wgpu adapter =' line — the real wgpu backend never ran \
(was the gfx-wgpu feature actually compiled in?)"
fi
ADAPTER="$(echo "$OUT" | grep "axon-gfx: wgpu adapter =" | head -1)"
echo "→ $ADAPTER"

# The acceptance assertion is in the .ax (assert_eq(px, 8454143)); exit 0 = pass.
[[ $RC -eq 0 ]] || fail "render program exited $RC (pixel readback did not match the cleared color)"

# Vacuous-pass guard #2: assert the readback line is present with the wanted value.
if ! echo "$OUT" | grep -q "read_pixel = $WANT_PIXEL"; then
    fail "expected 'read_pixel = $WANT_PIXEL' in output — pixel readback missing/mismatched"
fi

echo ""
echo "✅ gfx-wgpu render gate PASSED — real wgpu offscreen frame cleared to 0x0080FFFF,"
echo "   verified by GPU pixel read-back on a headless Vulkan adapter."
exit 0
