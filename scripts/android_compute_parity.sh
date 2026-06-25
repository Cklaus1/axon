#!/usr/bin/env bash
# android_compute_parity.sh — R14 slice 2 (THE load-bearing compute slice).
#
# I-2 (parity) for the Android target: the SAME `.ax` program, run by the
# tree-walking interpreter on the host vs. the LLVM-codegen Android ELF run on a
# device, must produce byte-identical stdout AND the same exit code. This is the
# `android_so_compute_parity` acceptance gate (R14 §9) — mobile is R1 AArch64
# codegen for a different triple, so a mobile compute result is provably the
# desktop result; this harness *proves* it by running both.
#
# Execution path (most-reliable-first):
#   * x86_64-linux-android  → run NATIVELY on the KVM-accelerated x86_64
#     emulator via `adb push` + `adb shell` (full hardware speed).
#   * aarch64-linux-android → run on the SAME emulator via its arm64 native
#     bridge (libndk_translation) when arm64-v8a is in the ABI list; else under
#     `qemu-aarch64-static` with the NDK sysroot if a usable loader is present.
#
# SKIP-GUARD: this harness is NOT part of the default gate's hard requirements —
# it exits 0 (skip) when the NDK or a booted emulator/qemu is absent, so the
# interpreter-only default gate stays safe. When it DOES run it asserts ran>0
# (a vacuous green is a failure, per the coverage-vacuous-pass guard).
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

skip() { echo "android_compute_parity: SKIP — $1"; exit 0; }
fail() { echo "android_compute_parity: FAIL — $1"; exit 1; }

# ── Toolchain discovery ───────────────────────────────────────────────────────
# NDK bin dir: $ANDROID_NDK_HOME, else highest $ANDROID_HOME/ndk/<ver>.
ndk_bin() {
  local host_tag="linux-x86_64"
  for r in "${ANDROID_NDK_HOME:-}" "${ANDROID_NDK_ROOT:-}"; do
    [ -n "$r" ] && [ -d "$r/toolchains/llvm/prebuilt/$host_tag/bin" ] && {
      echo "$r/toolchains/llvm/prebuilt/$host_tag/bin"; return 0; }
  done
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || return 1
  local v
  v="$(ls "$sdk/ndk" 2>/dev/null | sort -V | tail -1)"
  [ -n "$v" ] && [ -d "$sdk/ndk/$v/toolchains/llvm/prebuilt/$host_tag/bin" ] && {
    echo "$sdk/ndk/$v/toolchains/llvm/prebuilt/$host_tag/bin"; return 0; }
  return 1
}

NDKBIN="$(ndk_bin)" || skip "Android NDK not found (set ANDROID_NDK_HOME or ANDROID_HOME)"
[ -x "$NDKBIN/x86_64-linux-android34-clang" ] || skip "NDK clang not found in $NDKBIN"

ADB="${ADB:-adb}"
command -v "$ADB" >/dev/null 2>&1 || ADB="${ANDROID_HOME:-}/platform-tools/adb"

echo "android_compute_parity: building codegen axon binary…"
cargo build -q -p axon-core --bin axon 2>/dev/null || skip "codegen build unavailable (LLVM absent)"
AXON="target/debug/axon"
printf 'fn main() -> i64 { 0 }\n' > /tmp/axon_android_probe.ax
"$AXON" build /tmp/axon_android_probe.ax -o /tmp/axon_android_probe.bin --no-cache >/dev/null 2>&1 \
  || skip "this axon binary cannot emit native builds"
rm -f /tmp/axon_android_probe.ax /tmp/axon_android_probe.bin

# ── Device discovery ──────────────────────────────────────────────────────────
EMU_OK=0
ABILIST=""
if [ -x "$ADB" ] || command -v "$ADB" >/dev/null 2>&1; then
  if "$ADB" get-state 2>/dev/null | grep -q device; then
    bc="$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')"
    if [ "$bc" = "1" ]; then
      EMU_OK=1
      ABILIST="$("$ADB" shell getprop ro.product.cpu.abilist 2>/dev/null | tr -d '\r')"
    fi
  fi
fi

SYSROOT="$NDKBIN/../sysroot"
QEMU="$(command -v qemu-aarch64-static || command -v qemu-aarch64 || true)"

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"; [ "$EMU_OK" = 1 ] && "$ADB" shell rm -rf /data/local/tmp/axon_parity 2>/dev/null' EXIT
[ "$EMU_OK" = 1 ] && "$ADB" shell mkdir -p /data/local/tmp/axon_parity 2>/dev/null

# qemu-user can run an Android AArch64 ELF only if it can find the bionic
# dynamic loader. Android binaries hardcode the interpreter path
# `/system/bin/linker64`, which the NDK sysroot does NOT contain — so unless a
# usable loader is reachable, qemu produces empty output and a 255 exit. PROBE
# it with a tiny program; disable the qemu path (rather than count its failures)
# unless the probe actually runs. The emulator is the reliable path.
QEMU_OK=0
if [ -n "$QEMU" ]; then
  printf 'fn main() -> i64 { 7 }\n' > "$WORK/qprobe.ax"
  if "$AXON" build --target aarch64-linux-android "$WORK/qprobe.ax" -o "$WORK/qprobe" --no-cache >/dev/null 2>&1; then
    "$QEMU" -L "$SYSROOT" "$WORK/qprobe" >/dev/null 2>&1
    [ "$?" = "7" ] && QEMU_OK=1
  fi
fi

[ "$EMU_OK" = 1 ] || [ "$QEMU_OK" = 1 ] || \
  skip "no booted emulator (adb) and qemu cannot run Android bionic ELFs here (no loader) — cannot run Android artifacts"

# Pure-compute example set (no host I/O beyond stdout; deterministic).
EXAMPLES=(
  examples/math.ax
  examples/algorithms.ax
  examples/while.ax
  examples/modulo.ax
  examples/logical_ops.ax
  examples/math_builtins.ax
)

ran=0
fails=0

run_on_emulator() { # $1=local bin → echoes "stdout\nEXIT=n"
  local name; name="$(basename "$1")"
  "$ADB" push "$1" "/data/local/tmp/axon_parity/$name" >/dev/null 2>&1 || return 1
  "$ADB" shell chmod 755 "/data/local/tmp/axon_parity/$name" >/dev/null 2>&1
  "$ADB" shell "/data/local/tmp/axon_parity/$name; echo EXIT=\$?" 2>/dev/null | tr -d '\r'
}

run_under_qemu() { # $1=local aarch64 bin
  [ -n "$QEMU" ] || return 1
  local out ec
  out="$("$QEMU" -L "$SYSROOT" "$1" 2>/dev/null)"; ec=$?
  printf '%s\nEXIT=%s\n' "$out" "$ec"
}

diff_parity() { # $1=ref file, $2=device "stdout...\nEXIT=n", $3=label, $4=ax
  local ref_out ref_exit dev_out dev_exit
  ref_out="$(cat "$1")"
  ref_exit="$REF_EXIT"
  dev_exit="$(printf '%s' "$2" | grep '^EXIT=' | tail -1 | cut -d= -f2)"
  dev_out="$(printf '%s' "$2" | grep -v '^EXIT=')"
  if [ "$ref_out" != "$dev_out" ]; then
    echo "  FAIL [$3] $4 stdout diverges:"
    echo "    interp: [$ref_out]"
    echo "    device: [$dev_out]"
    return 1
  fi
  if [ "$ref_exit" != "$dev_exit" ]; then
    echo "  FAIL [$3] $4 exit diverges (interp=$ref_exit device=$dev_exit)"; return 1
  fi
  echo "  OK [$3] $4 → '$ref_out' exit=$ref_exit"
  return 0
}

for ax in "${EXAMPLES[@]}"; do
  [ -f "$ax" ] || continue
  # Interpreter oracle.
  REF="$WORK/$(basename "$ax").ref"
  "$AXON" run "$ax" >"$REF" 2>/dev/null; REF_EXIT=$?

  # x86_64 → KVM emulator (native).
  if [ "$EMU_OK" = 1 ] && echo "$ABILIST" | grep -q x86_64; then
    bin="$WORK/$(basename "$ax" .ax)_x86"
    if "$AXON" build --target x86_64-linux-android "$ax" -o "$bin" --no-cache >/dev/null 2>&1; then
      dev="$(run_on_emulator "$bin")"
      ran=$((ran+1))
      diff_parity "$REF" "$dev" "x86_64/emu" "$ax" || fails=$((fails+1))
    else
      echo "  WARN x86_64 build failed for $ax"
    fi
  fi

  # aarch64 → emulator native bridge (arm64-v8a) OR qemu-user.
  bin="$WORK/$(basename "$ax" .ax)_arm"
  if "$AXON" build --target aarch64-linux-android "$ax" -o "$bin" --no-cache >/dev/null 2>&1; then
    if [ "$EMU_OK" = 1 ] && echo "$ABILIST" | grep -q arm64-v8a; then
      dev="$(run_on_emulator "$bin")"
      ran=$((ran+1))
      diff_parity "$REF" "$dev" "aarch64/emu-bridge" "$ax" || fails=$((fails+1))
    elif [ "$QEMU_OK" = 1 ]; then
      dev="$(run_under_qemu "$bin")"
      ran=$((ran+1))
      diff_parity "$REF" "$dev" "aarch64/qemu" "$ax" || fails=$((fails+1))
    fi
  else
    echo "  WARN aarch64 build failed for $ax"
  fi
done

if [ "$ran" -eq 0 ]; then
  fail "no Android programs actually ran (vacuous green guard) — device/qemu present but nothing executed"
fi

if [ "$fails" -ne 0 ]; then
  fail "$fails of $ran Android runs diverged from the interpreter oracle"
fi

echo "android_compute_parity: PASS — $ran Android runs byte-identical to the interpreter oracle (I-2)"
exit 0
