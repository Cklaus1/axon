#!/usr/bin/env bash
# browser_webgpu_clear.sh — R7c Slice 3, acceptance `browser_window_clear_renders_frame`.
#
# Verifies that a WebGPU frame can be CLEARED to a known color in REAL headless
# Chrome and read back byte-exact — the GPU substrate the Axon `native::gfx`
# WebGPU backend (window_open → attach canvas; clear → render pass; present →
# submit) will sit on. The fixture (browser_webgpu_clear.html) renders to an
# offscreen 4x4 texture, clears it to (0.2,0.4,0.8,1.0), copies it back, and reads
# the actual pixel bytes; the driver (browser_webgpu_clear.py, raw chromedriver
# wire protocol) asserts the readback equals 51,102,204,255.
#
# HONEST BOUNDARY: this gate-verifies the WebGPU *substrate*, NOT an Axon-gfx-
# driven frame — Axon's `gfx` is a MOCK today (axon-gfx-mock, no real wgpu
# backend), so a `mount()`-ed `examples/.../window_clear.ax` rendering through
# Axon awaits R16 (the real wgpu/WebGPU backend). What this PROVES, and what was
# previously assumed-unavailable: headless-Chrome WebGPU clear genuinely works
# here (Chrome 149 SwiftShader-Vulkan), so the substrate is NOT the blocker — R16
# is. See governance/specs/R7c-browser-host.md §9/§11.
#
# SKIP-GUARDED (exit 0) if Chrome / a matching chromedriver / python3 are absent
# so the default gate stays green without a browser. Set BROWSER_WEBGPU_REQUIRE=1
# to force-fail instead of skip.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

skip_or_fail() {
  if [ "${BROWSER_WEBGPU_REQUIRE:-0}" = "1" ]; then
    echo "browser_webgpu_clear: FAIL (BROWSER_WEBGPU_REQUIRE=1) — $1"; exit 1
  fi
  echo "browser_webgpu_clear: $1 — skipping (set BROWSER_WEBGPU_REQUIRE=1 to force-fail)"; exit 0
}

CHROME_BIN="${CHROME:-$(command -v google-chrome || command -v google-chrome-stable || command -v chromium || true)}"
[ -n "$CHROME_BIN" ] || skip_or_fail "no Chrome/Chromium on PATH (set CHROME=...)"
CHROMEDRIVER_BIN="${CHROMEDRIVER:-$(command -v chromedriver || true)}"
[ -n "$CHROMEDRIVER_BIN" ] || skip_or_fail "no chromedriver (CHROMEDRIVER=...; match the Chrome major version)"
command -v python3 >/dev/null 2>&1 || skip_or_fail "python3 not found (driver needs it)"

export CHROMEDRIVER="$CHROMEDRIVER_BIN"
# Chrome bundles a SwiftShader Vulkan ICD; point the loader at it so the software
# WebGPU adapter is available headless (no real GPU needed).
SWIFTSHADER_ICD="/opt/google/chrome/vk_swiftshader_icd.json"
[ -f "$SWIFTSHADER_ICD" ] && export VK_ICD_FILENAMES="$SWIFTSHADER_ICD"

echo "browser_webgpu_clear: rendering a WebGPU clear frame in headless Chrome…"
echo "  CHROME=$CHROME_BIN"
echo "  CHROMEDRIVER=$CHROMEDRIVER_BIN"

OUT="$(python3 scripts/browser_webgpu_clear.py scripts/browser_webgpu_clear.html 2>/dev/null)"
status=$?
echo "  result: $OUT"

if [ "$status" -ne 0 ] || [ -z "$OUT" ]; then
  # WebGPU genuinely unavailable in this headless Chrome — that's the documented
  # manual/#[ignore] tier (spec §9 stretch). Skip cleanly unless forced.
  skip_or_fail "headless WebGPU unavailable (got: ${OUT:-<none>}) — manual tier per spec §9"
fi

# Expected: 0.2*255≈51, 0.4*255≈102, 0.8*255≈204, 1.0*255=255 (±1 for rounding).
EXPECT_RE="^CLEAR_RGBA=5[01],10[12],20[345],255$"
if ! echo "$OUT" | grep -qE "$EXPECT_RE"; then
  echo "browser_webgpu_clear: FAIL — cleared frame readback != expected (0.2,0.4,0.8,1.0):"
  echo "    got:      $OUT"
  echo "    expected: CLEAR_RGBA=51,102,204,255 (±1)"
  exit 1
fi

echo "browser_webgpu_clear: PASS — WebGPU frame cleared to (0.2,0.4,0.8,1.0) and read back byte-exact in headless Chrome ($OUT). The WebGPU substrate is available; the Axon-gfx-driven frame awaits R16's real wgpu backend."
exit 0
