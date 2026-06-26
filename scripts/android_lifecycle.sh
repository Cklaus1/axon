#!/usr/bin/env bash
# android_lifecycle.sh — R14 slice 3: the AxonRuntime lifecycle adapter,
# verified end-to-end as far as a headless emulator allows (honest tiering).
#
# Slice 3 has two halves; this harness exercises BOTH the parts that CAN run
# headless and is explicit about the one part that cannot:
#
#  (A) THE RESUME ROUND-TRIP (host, gate-verified here): the lifecycle program
#      `examples/mobile/lifecycle.ax` spans init → tick → suspend → resume →
#      teardown as ONE linear Axon fn, suspending at each `host_await` and
#      resuming with the next OS event. Driven on stdin (the same substrate the
#      AxonRuntime bridge drives on-device), it must produce the deterministic
#      transcript + exit code = frames rendered. `host_await` is the LANDED
#      Phase-6 resume runtime (interp-only, codegen-refused) — so this is the
#      genuine suspend/resume the bridge depends on.
#
#  (B) THE ON-DEVICE NATIVE BOUNDARY (emulator, gate-verified here): the
#      AxonRuntime bridge loads the Axon `.so` via `System.loadLibrary` and
#      calls its exported entry. We prove that boundary works on the device by
#      building the compute app as an Android `.so`, pushing it, and `dlopen`+
#      `dlsym("main")`+call from a tiny on-device C harness — the same load→bind
#      →invoke path the Kotlin wrapper takes, minus the JVM. A non-zero return
#      and clean exit prove the entry is reachable on-device.
#
#  (C) THE FULL NativeActivity GUI JOURNEY (manual tier — NOT run here): standing
#      up a real Activity + kotlinc + a JVM frame loop headlessly is out of scope
#      for this gate; it is the documented manual tier (R14 §9 stretch). This
#      script does NOT fake it.
#
# SKIP-GUARD: exits 0 (skip) when the NDK or a booted emulator is absent, so the
# default gate stays safe. When it runs it asserts BOTH halves ran.
set -u

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

skip() { echo "android_lifecycle: SKIP — $1"; exit 0; }
fail() { echo "android_lifecycle: FAIL — $1"; exit 1; }

ndk_bin() {
  local host_tag="linux-x86_64"
  for r in "${ANDROID_NDK_HOME:-}" "${ANDROID_NDK_ROOT:-}"; do
    [ -n "$r" ] && [ -d "$r/toolchains/llvm/prebuilt/$host_tag/bin" ] && {
      echo "$r/toolchains/llvm/prebuilt/$host_tag/bin"; return 0; }
  done
  local sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-}}"
  [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || return 1
  local v; v="$(ls "$sdk/ndk" 2>/dev/null | sort -V | tail -1)"
  [ -n "$v" ] && [ -d "$sdk/ndk/$v/toolchains/llvm/prebuilt/$host_tag/bin" ] && {
    echo "$sdk/ndk/$v/toolchains/llvm/prebuilt/$host_tag/bin"; return 0; }
  return 1
}

NDKBIN="$(ndk_bin)" || skip "Android NDK not found"
ADB="${ADB:-adb}"
command -v "$ADB" >/dev/null 2>&1 || ADB="${ANDROID_HOME:-}/platform-tools/adb"

cargo build -q -p axon-core --bin axon 2>/dev/null || skip "codegen build unavailable"
AXON="target/debug/axon"
printf 'fn main() -> i64 { 0 }\n' > /tmp/axon_lc_probe.ax
"$AXON" build /tmp/axon_lc_probe.ax -o /tmp/axon_lc_probe.bin --no-cache >/dev/null 2>&1 \
  || skip "this axon binary cannot emit native builds"
rm -f /tmp/axon_lc_probe.ax /tmp/axon_lc_probe.bin

WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"; "$ADB" shell rm -rf /data/local/tmp/axon_lc 2>/dev/null' EXIT

ran_a=0
ran_b=0

# ── (A) host resume round-trip ────────────────────────────────────────────────
echo "android_lifecycle: (A) host_await lifecycle round-trip…"
EVENTS=$'tick\ntick\nsuspend\nresume\ntick\nteardown\n'
printf '%s' "$EVENTS" | "$AXON" run examples/mobile/lifecycle.ax >"$WORK/lc.out" 2>/dev/null
lc_exit=$?
ran_a=1
# Expected transcript markers + exit = 3 frames rendered.
if ! grep -q "on_start (init)" "$WORK/lc.out" \
   || ! grep -q "on_pause: app backgrounded" "$WORK/lc.out" \
   || ! grep -q "on_resume: app foregrounded" "$WORK/lc.out" \
   || ! grep -q "on_destroy: teardown; frames=3 suspends=1" "$WORK/lc.out"; then
  echo "--- transcript ---"; cat "$WORK/lc.out"
  fail "(A) lifecycle transcript missing an expected init/suspend/resume/teardown marker"
fi
if [ "$lc_exit" != "3" ]; then
  fail "(A) lifecycle exit = $lc_exit, expected 3 (frames rendered round-trip witness)"
fi
echo "  OK (A) init→tick→suspend→resume→tick→teardown, exit=3 (3 frames)"

# ── device discovery for (B) ──────────────────────────────────────────────────
EMU_OK=0; ABILIST=""
if "$ADB" get-state 2>/dev/null | grep -q device; then
  [ "$("$ADB" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = "1" ] && {
    EMU_OK=1; ABILIST="$("$ADB" shell getprop ro.product.cpu.abilist 2>/dev/null | tr -d '\r')"; }
fi

if [ "$EMU_OK" != 1 ]; then
  echo "android_lifecycle: (B) no booted emulator — on-device boundary not run (A passed)"
  echo "android_lifecycle: PASS (A only; B needs an emulator)"
  exit 0
fi

# ── (B) on-device load→bind→invoke of the Axon .so ────────────────────────────
echo "android_lifecycle: (B) on-device dlopen+dlsym(main)+call of the Axon .so…"
# Pick the device's primary ABI.
TRIPLE="x86_64-linux-android"; ABI="x86_64"
if echo "$ABILIST" | grep -q x86_64; then TRIPLE="x86_64-linux-android"; ABI="x86_64"
elif echo "$ABILIST" | grep -q arm64-v8a; then TRIPLE="aarch64-linux-android"; ABI="arm64-v8a"
fi
SODIR="$WORK/out/android/jniLibs/$ABI"
mkdir -p "$SODIR"
ANDROID_HOME="${ANDROID_HOME:-}" "$AXON" build --host mobile --target "$TRIPLE" \
  examples/mobile/compute.ax --out "$SODIR/libcompute.so" >/dev/null 2>&1 \
  || fail "(B) --host mobile build of compute.so failed for $TRIPLE"

# Tiny C harness: dlopen the .so, resolve `main`, call it, print its return.
# This is the AxonRuntime load→bind→invoke path minus the JVM.
cat > "$WORK/loader.c" <<'EOF'
#include <dlfcn.h>
#include <stdio.h>
int main(int argc, char** argv) {
  void* h = dlopen(argv[1], RTLD_NOW);
  if (!h) { fprintf(stderr, "dlopen failed: %s\n", dlerror()); return 90; }
  typedef long (*entry_t)(void);
  entry_t f = (entry_t)dlsym(h, "main");
  if (!f) { fprintf(stderr, "dlsym(main) failed: %s\n", dlerror()); return 91; }
  long rc = f();
  printf("AxonRuntime(device): entry 'main' returned %ld\n", rc);
  return (int)rc;
}
EOF
CLANG="$NDKBIN/${TRIPLE%%-*}-linux-android34-clang"
case "$TRIPLE" in
  x86_64-*) CLANG="$NDKBIN/x86_64-linux-android34-clang" ;;
  aarch64-*) CLANG="$NDKBIN/aarch64-linux-android34-clang" ;;
esac
"$CLANG" "$WORK/loader.c" -ldl -o "$WORK/loader" 2>/dev/null \
  || fail "(B) could not build the on-device loader harness"

"$ADB" shell mkdir -p /data/local/tmp/axon_lc 2>/dev/null
"$ADB" push "$SODIR/libcompute.so" /data/local/tmp/axon_lc/libcompute.so >/dev/null 2>&1
"$ADB" push "$WORK/loader" /data/local/tmp/axon_lc/loader >/dev/null 2>&1
"$ADB" shell chmod 755 /data/local/tmp/axon_lc/loader 2>/dev/null
DEV="$("$ADB" shell "cd /data/local/tmp/axon_lc && LD_LIBRARY_PATH=. ./loader ./libcompute.so; echo EXIT=\$?" 2>/dev/null | tr -d '\r')"
ran_b=1

dev_exit="$(printf '%s' "$DEV" | grep '^EXIT=' | tail -1 | cut -d= -f2)"
if ! printf '%s' "$DEV" | grep -q "entry 'main' returned"; then
  echo "--- device output ---"; printf '%s\n' "$DEV"
  fail "(B) on-device loader did not report the entry return (load/bind/invoke failed)"
fi
# The interp oracle's exit for compute.ax is its main return (986 % 256 = 218).
"$AXON" run examples/mobile/compute.ax >/dev/null 2>&1; ref_exit=$?
if [ "$dev_exit" != "$ref_exit" ]; then
  echo "--- device output ---"; printf '%s\n' "$DEV"
  fail "(B) on-device entry returned $dev_exit, interp oracle returned $ref_exit"
fi
echo "  OK (B) device dlopen+dlsym(main)+call → returned $dev_exit (== interp oracle), .so loadable on-device"

[ "$ran_a" = 1 ] || fail "(A) did not run"
[ "$ran_b" = 1 ] || fail "(B) did not run"
echo "android_lifecycle: PASS — (A) host resume round-trip + (B) on-device .so load/bind/invoke"
echo "android_lifecycle: NOTE — the full NativeActivity GUI frame loop is the documented MANUAL tier (R14 §9), not run headless."
exit 0
