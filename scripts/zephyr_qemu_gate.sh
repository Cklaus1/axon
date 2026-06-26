#!/usr/bin/env bash
# R21 acceptance gate: an Axon program runs AS a Zephyr application on an ARM
# Cortex-M target, verified headlessly under QEMU.
#
# Pipeline:
#   1. axon build --freestanding --target zephyr --emit-obj  → arm-zephyr-eabi .o
#   2. west build -b qemu_cortex_m3 examples/zephyr  (links the Axon object)
#   3. west build -t run  → QEMU Cortex-M3, capture console output (with timeout)
#   4. grep for the expected Axon output markers (AXON banner + computed values)
#
# Requires: axon (with codegen), the Zephyr workspace (ZEPHYR_BASE or
# ~/zephyrproject/zephyr), the Zephyr SDK (arm-zephyr-eabi toolchain), west,
# cmake, ninja, qemu-system-arm. SKIPs gracefully (exit 0) if any is absent —
# so the default `gate.sh` stays green on hosts without the Zephyr SDK.
#
# Usage: scripts/zephyr_qemu_gate.sh [--timeout SECS]   (default: 20s)
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$SCRIPT_DIR/.." && pwd)"
TIMEOUT_SECS=20

while [[ $# -gt 0 ]]; do
	case "$1" in
	--timeout)
		TIMEOUT_SECS="$2"
		shift 2
		;;
	*)
		echo "Unknown arg: $1" >&2
		exit 1
		;;
	esac
done

skip() {
	echo "SKIP: $*" >&2
	exit 0
}

# ── Tool / environment checks (SKIP-guarded) ─────────────────────────────────

command -v west >/dev/null 2>&1 || skip "west not found (install the Zephyr meta-tool)"
command -v cmake >/dev/null 2>&1 || skip "cmake not found"
command -v ninja >/dev/null 2>&1 || skip "ninja not found"
command -v qemu-system-arm >/dev/null 2>&1 || skip "qemu-system-arm not found"

# Locate the Zephyr base (the zephyr/ tree inside the workspace).
ZBASE="${ZEPHYR_BASE:-$HOME/zephyrproject/zephyr}"
[[ -f "$ZBASE/zephyr-env.sh" ]] || skip "Zephyr base not found at $ZBASE (set ZEPHYR_BASE)"
export ZEPHYR_BASE="$ZBASE"

# Locate the Zephyr SDK (the arm-zephyr-eabi toolchain).
ZSDK="${ZEPHYR_SDK_INSTALL_DIR:-$HOME/zephyr-sdk}"
[[ -x "$ZSDK/arm-zephyr-eabi/bin/arm-zephyr-eabi-gcc" ]] ||
	skip "arm-zephyr-eabi toolchain not found under $ZSDK (set ZEPHYR_SDK_INSTALL_DIR)"
export ZEPHYR_SDK_INSTALL_DIR="$ZSDK"

# Find the axon binary (prefer the workspace target dir) and confirm codegen.
AXON_BIN=""
for candidate in \
	"$REPO/target/debug/axon" \
	"$REPO/target/release/axon" \
	"$(command -v axon 2>/dev/null || true)"; do
	if [[ -x "$candidate" ]]; then
		AXON_BIN="$candidate"
		break
	fi
done
[[ -n "$AXON_BIN" ]] || skip "axon binary not found (build with: cargo build -p axon-core)"
if ! "$AXON_BIN" build --help 2>&1 | grep -q "emit-obj"; then
	skip "axon binary lacks codegen support (build with: cargo build -p axon-core)"
fi

# ── Build ────────────────────────────────────────────────────────────────────

TMPDIR_LOCAL="$(mktemp -d)"
trap 'rm -rf "$TMPDIR_LOCAL"' EXIT

APP_AX="$REPO/examples/zephyr/app.ax"
AXON_OBJ="$TMPDIR_LOCAL/axon_app.o"
ZBUILD="$TMPDIR_LOCAL/zb"

echo "=== R21: Axon-on-Zephyr QEMU gate ===" >&2
echo "  axon:   $AXON_BIN" >&2
echo "  zephyr: $ZEPHYR_BASE" >&2
echo "  sdk:    $ZEPHYR_SDK_INSTALL_DIR" >&2
echo "  qemu:   $(command -v qemu-system-arm)" >&2
echo "" >&2

echo "1/3 compiling Axon app → arm-zephyr-eabi object ($AXON_OBJ)" >&2
"$AXON_BIN" build --freestanding --target zephyr --emit-obj "$APP_AX" --out "$AXON_OBJ" 2>&1 |
	grep -v "warning\[E0905\]" || true
[[ -f "$AXON_OBJ" ]] || {
	echo "FAIL: Axon object was not emitted" >&2
	exit 1
}

echo "2/3 west build Zephyr app for qemu_cortex_m3" >&2
west build -p auto -b qemu_cortex_m3 "$REPO/examples/zephyr" -d "$ZBUILD" \
	-- -DAXON_OBJ="$AXON_OBJ" 2>&1 | tail -6

# ── Run under QEMU ───────────────────────────────────────────────────────────

echo "" >&2
echo "3/3 booting under QEMU (timeout ${TIMEOUT_SECS}s)..." >&2

QEMU_OUTPUT="$TMPDIR_LOCAL/qemu_out.txt"
set +e
# The Zephyr app idles after Axon returns; QEMU runs until the timeout fires
# (124), which is expected and not a failure.
timeout "$TIMEOUT_SECS" west build -d "$ZBUILD" -t run >"$QEMU_OUTPUT" 2>&1
RUN_EXIT=$?
set -e

if [[ $RUN_EXIT -ne 0 && $RUN_EXIT -ne 124 ]]; then
	echo "FAIL: QEMU run exited with code $RUN_EXIT" >&2
	cat "$QEMU_OUTPUT" >&2
	exit 1
fi

CAPTURED="$(cat "$QEMU_OUTPUT")"
echo "--- QEMU console ---" >&2
echo "$CAPTURED" | grep -E "Zephyr|AXON|^[0-9]+$" >&2 || true
echo "--------------------" >&2

# Assert the Axon-produced output: the AXON banner and the two computed values
# (sensor_avg=23, answer=42). All three must be present. We normalise the
# console capture first (strip CR / control chars QEMU's UART may interleave),
# then match each value as a standalone token (surrounded by non-digits so a
# substring of a larger number can't satisfy it).
NORM="$(printf '%s' "$CAPTURED" | tr -d '\r' | tr -c '0-9A-Za-z' '\n')"
ok=1
echo "$CAPTURED" | grep -q "AXON" || {
	echo "FAIL: 'AXON' banner not found on console" >&2
	ok=0
}
echo "$NORM" | grep -qx "23" || {
	echo "FAIL: refinement-checked sensor_avg=23 not found" >&2
	ok=0
}
echo "$NORM" | grep -qx "42" || {
	echo "FAIL: computed answer=42 not found" >&2
	ok=0
}

if [[ $ok -eq 1 ]]; then
	echo "" >&2
	echo "PASS: Axon ran on Zephyr/Cortex-M under QEMU — banner + computed 23 + 42" >&2
	exit 0
else
	echo "" >&2
	echo "Full output was:" >&2
	echo "$CAPTURED" >&2
	exit 1
fi
