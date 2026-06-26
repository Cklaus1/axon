#!/usr/bin/env bash
# setup-environments.sh — install the platform-target verification environments.
#
# Stands up everything needed to run the manual-tier gates (GPU render, browser
# parity, Android compute/lifecycle) on a headless x86_64 Linux host — no GPU,
# phone, or display required. iOS is macOS-only and runs in CI (.github/workflows/ios.yml),
# not here. See ENVIRONMENTS.md for the full explanation + verification steps.
#
# Idempotent: re-running skips what's already present. Needs sudo (apt) + rustup.
# Usage:  bash scripts/setup-environments.sh [gpu|browser|android|all]   (default: all)
set -uo pipefail
WHAT="${1:-all}"
ANDROID_HOME="${ANDROID_HOME:-$HOME/android-sdk}"
NDK_VER="27.0.12077973"
ok()   { echo "  ✓ $1"; }
step() { echo ""; echo "── $1 ──"; }

setup_gpu() {
  step "GPU — Mesa lavapipe (software Vulkan)"
  if [ -f /usr/share/vulkan/icd.d/lvp_icd.json ] && command -v vulkaninfo >/dev/null; then
    ok "lavapipe + vulkan-tools already present"
  else
    sudo apt-get install -y -q mesa-vulkan-drivers vulkan-tools libvulkan1
  fi
  if VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json vulkaninfo --summary 2>/dev/null \
       | grep -qi llvmpipe; then
    ok "headless Vulkan device: llvmpipe (CPU)"
  else
    echo "  ✗ lavapipe did not enumerate — check 'vulkaninfo --summary'"; return 1
  fi
}

setup_browser() {
  step "Browser — headless Chrome"
  if command -v google-chrome >/dev/null; then
    ok "google-chrome already present ($(google-chrome --version))"
  else
    wget -q https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb -O /tmp/chrome.deb
    sudo apt-get install -y -q /tmp/chrome.deb
  fi
  if google-chrome --headless --no-sandbox --dump-dom about:blank >/dev/null 2>&1; then
    ok "headless Chrome works"
  else
    echo "  ✗ headless Chrome check failed"; return 1
  fi
  command -v chromedriver >/dev/null && ok "chromedriver present" \
    || echo "  • chromedriver not found — 'npm i -g chromedriver' for wasm-bindgen-test"
}

setup_android() {
  step "Android — NDK + KVM-accelerated emulator"
  command -v java >/dev/null || sudo apt-get install -y -q default-jre unzip
  local SDK="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
  if [ ! -x "$SDK" ]; then
    mkdir -p "$ANDROID_HOME/cmdline-tools"
    wget -q https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip -O /tmp/cmdtools.zip
    rm -rf /tmp/cmdline-tools "$ANDROID_HOME/cmdline-tools/latest"
    unzip -q /tmp/cmdtools.zip -d /tmp && mv /tmp/cmdline-tools "$ANDROID_HOME/cmdline-tools/latest"
  fi
  ok "cmdline-tools at $ANDROID_HOME"
  yes 2>/dev/null | "$SDK" --licenses >/dev/null 2>&1 || true
  "$SDK" "platform-tools" "ndk;$NDK_VER" "platforms;android-34" "build-tools;34.0.0" \
         "emulator" "system-images;android-34;google_apis;x86_64" >/dev/null
  ok "platform-tools, ndk;$NDK_VER, emulator, system-image installed"
  echo no | "$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager" \
        create avd -n axon_test -k "system-images;android-34;google_apis;x86_64" --force >/dev/null 2>&1 || true
  ok "AVD 'axon_test' ready"
  rustup target add aarch64-linux-android x86_64-linux-android armv7-linux-androideabi >/dev/null 2>&1
  ok "rust android targets added"
  if [ -e /dev/kvm ]; then
    sudo usermod -aG kvm "$(whoami)" 2>/dev/null || true
    sudo chmod 666 /dev/kvm 2>/dev/null || true
    ok "/dev/kvm accessible (emulator acceleration)"
  else
    echo "  • /dev/kvm absent — emulator will run unaccelerated (slow)"
  fi
  echo "  → boot:  \$ANDROID_HOME/emulator/emulator -avd axon_test -no-window -gpu swiftshader_indirect -accel on &"
}

# Keep Axon codegen on LLVM 17 even after a clang/llvm install repoints llvm-config to 18.
pin_llvm17() {
  if [ -x /usr/bin/llvm-config-17 ]; then
    sudo ln -sf /usr/bin/llvm-config-17 /usr/bin/llvm-config 2>/dev/null || true
    ok "llvm-config pinned to 17 (codegen needs LLVM 17; LLVM_SYS_170_PREFIX=/usr/lib/llvm-17)"
  fi
}

setup_ebpf() {
  step "eBPF — clang-bpf + kernel verifier"
  command -v clang-18 >/dev/null || sudo apt-get install -y -q clang-18 libbpf-dev linux-tools-common
  pin_llvm17   # IMPORTANT: clang-18 is fine for the bpf target, but keep llvm-config → 17 for codegen
  if clang-18 -target bpf -O2 -c -x c /dev/null -o /tmp/_probe.bpf.o 2>/dev/null; then
    ok "clang bpf target OK"
  else
    echo "  ✗ clang -target bpf failed"; return 1
  fi
  [ -f /sys/kernel/btf/vmlinux ] && ok "kernel BTF present (real verifier load works)" \
    || echo "  • no kernel BTF — ebpf_verify.sh falls back to structural / rbpf checks"
}

setup_zephyr() {
  step "Zephyr RTOS — SDK + QEMU Cortex-M"
  pip install --break-system-packages -q west pyelftools 2>/dev/null || true
  command -v qemu-system-arm >/dev/null || sudo apt-get install -y -q qemu-system-arm cmake ninja-build device-tree-compiler gperf
  if [ ! -d ~/zephyrproject/zephyr ]; then
    west init ~/zephyrproject >/dev/null 2>&1 && (cd ~/zephyrproject && west update >/dev/null 2>&1 && west zephyr-export >/dev/null 2>&1)
  fi
  ok "zephyr workspace at ~/zephyrproject"
  if [ ! -d ~/zephyr-sdk/arm-zephyr-eabi ]; then
    local SDK=0.17.0
    wget -qO /tmp/zsdk.tar.xz "https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v${SDK}/zephyr-sdk-${SDK}_linux-x86_64_minimal.tar.xz"
    mkdir -p ~/zephyr-sdk && tar xf /tmp/zsdk.tar.xz -C ~/zephyr-sdk --strip-components=1
    (cd ~/zephyr-sdk && ./setup.sh -t arm-zephyr-eabi -c >/dev/null 2>&1) || true
  fi
  # The bleeding-edge tree wants find_package(Zephyr-sdk 1.0); the 0.17 label gates it.
  [ -f ~/zephyr-sdk/sdk_version ] && ! grep -q '^1\.' ~/zephyr-sdk/sdk_version 2>/dev/null && echo "1.0.0" > ~/zephyr-sdk/sdk_version
  [ -x ~/zephyr-sdk/arm-zephyr-eabi/bin/arm-zephyr-eabi-gcc ] \
    && ok "Zephyr SDK + arm toolchain ready (set ZEPHYR_SDK_INSTALL_DIR=~/zephyr-sdk)" \
    || echo "  ✗ arm-zephyr-eabi toolchain missing"
}

case "$WHAT" in
  gpu)     setup_gpu ;;
  browser) setup_browser ;;
  android) setup_android ;;
  ebpf)    setup_ebpf ;;
  zephyr)  setup_zephyr ;;
  all)     setup_gpu; setup_browser; setup_android; setup_ebpf; setup_zephyr ;;
  *) echo "usage: $0 [gpu|browser|android|ebpf|zephyr|all]"; exit 2 ;;
esac

echo ""
echo "Done. Off-host boundaries verified in CI: iOS (.github/workflows/ios.yml, macOS) and"
echo "TEE hardware attestation (.github/workflows/tee.yml, confidential runner)."
echo "See ENVIRONMENTS.md for per-gate verification commands."
