# Reproducible build & verification environments

Axon's core (`axon-core` interpreter + LLVM codegen) builds and gates with **no special
hardware** — `cargo build -p axon-core` and `bash scripts/gate.sh` are all you need for
99% of the project. This document covers the **four extra environments** required to run
the *platform-target* gates (GPU rendering, browser, Android, iOS) that were historically
marked "manual tier."

Every command below was run and verified on **Ubuntu 24.04 (WSL2, x86_64)**. None of these
environments needs a physical GPU, a phone, or a display — they use software rasterization
(Mesa lavapipe), headless Chrome, and a KVM-accelerated Android emulator. iOS is the sole
exception (Apple toolchain is macOS-only) and is verified in CI instead.

> **One-shot setup:** `bash scripts/setup-environments.sh` installs everything below
> (idempotent; skips what's already present). The per-environment sections explain what it
> does and how to verify each independently.

| Environment | Enables gate | Needs hardware? | One-line verify |
|---|---|---|---|
| **GPU — Mesa lavapipe** | `scripts/gfx_wgpu_render_gate.sh` (R13 S5) | No (CPU Vulkan) | `VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json vulkaninfo --summary` |
| **Browser — headless Chrome** | `scripts/browser_*.sh` (R7c) | No | `google-chrome --headless --dump-dom about:blank` |
| **Android — NDK + KVM emulator** | `scripts/android_compute_parity.sh`, `android_lifecycle.sh` (R14) | No (KVM accel, `swiftshader` GPU) | `$ANDROID_HOME/emulator/emulator -list-avds` |
| **iOS — GitHub Actions macOS** | `.github/workflows/ios.yml` (R14 iOS) | **macOS only** → runs in CI | (verified remotely on push) |

---

## 1. GPU — Mesa lavapipe (software Vulkan)

`wgpu` (the real `axon-gfx` backend, R13 Slice 5) needs a Vulkan adapter. **lavapipe**
(`llvmpipe`) is Mesa's CPU-based Vulkan implementation — wgpu renders to an offscreen
texture on it with no GPU and no display.

```bash
sudo apt-get install -y mesa-vulkan-drivers vulkan-tools libvulkan1
```

Verify a headless device enumerates (force the lavapipe ICD):

```bash
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json vulkaninfo --summary | grep -i deviceName
# → deviceName = llvmpipe (LLVM 20.x, 256 bits)   [PHYSICAL_DEVICE_TYPE_CPU]
```

Run any wgpu program against it with:

```bash
export VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.json
export WGPU_BACKEND=vulkan
```

This is what `scripts/gfx_wgpu_render_gate.sh` sets internally; it renders a frame, reads
back a pixel, and asserts the cleared color. The gate SKIPs gracefully if no Vulkan ICD is
present, so it never breaks a bare CI.

---

## 2. Browser — headless Chrome (+ software WebGPU)

R7c's pure-compute parity (`wasm-bindgen-test`) and the WebGPU canvas frame run in a real
**headless Chrome**. Compute parity needs only headless Chrome; the WebGPU canvas uses
Chrome's SwiftShader (or lavapipe) software backend.

```bash
wget https://dl.google.com/linux/direct/google-chrome-stable_current_amd64.deb -O /tmp/chrome.deb
sudo apt-get install -y /tmp/chrome.deb
# chromedriver matching the installed Chrome major version (for wasm-bindgen-test):
npm i -g chromedriver    # or fetch from the Chrome-for-Testing endpoint
```

Verify:

```bash
google-chrome --version
google-chrome --headless --no-sandbox --dump-dom about:blank >/dev/null && echo "headless OK"
```

WebGPU under headless Chrome (software):

```bash
google-chrome --headless=new --no-sandbox \
  --enable-unsafe-webgpu --use-angle=swiftshader
```

`scripts/browser_*.sh` point `wasm-bindgen-test` at headless Chrome and SKIP-guard if Chrome
or chromedriver is absent.

---

## 3. Android — NDK cross-build + KVM-accelerated emulator

R14 cross-builds Axon for Android (NDK) and verifies compute parity + the lifecycle adapter
on a **headless emulator**. The emulator uses **KVM** for near-native speed and
`swiftshader` for software GPU — no device, no display.

### 3a. SDK / NDK / emulator

```bash
export ANDROID_HOME="$HOME/android-sdk"
mkdir -p "$ANDROID_HOME/cmdline-tools"
wget https://dl.google.com/android/repository/commandlinetools-linux-11076708_latest.zip -O /tmp/cmdtools.zip
unzip -q /tmp/cmdtools.zip -d /tmp && mv /tmp/cmdline-tools "$ANDROID_HOME/cmdline-tools/latest"
SDK="$ANDROID_HOME/cmdline-tools/latest/bin/sdkmanager"
sudo apt-get install -y default-jre        # sdkmanager needs a JRE
yes | "$SDK" --licenses
"$SDK" "platform-tools" "ndk;27.0.12077973" "platforms;android-34" \
       "build-tools;34.0.0" "emulator" "system-images;android-34;google_apis;x86_64"
echo no | "$ANDROID_HOME/cmdline-tools/latest/bin/avdmanager" \
       create avd -n axon_test -k "system-images;android-34;google_apis;x86_64" --force
```

### 3b. Rust targets + KVM access

```bash
rustup target add aarch64-linux-android x86_64-linux-android armv7-linux-androideabi
sudo usermod -aG kvm "$USER"     # persists across logins
sudo chmod 666 /dev/kvm          # this session, until the group membership takes effect
```

### 3c. Boot the emulator headless and verify

```bash
export ANDROID_HOME="$HOME/android-sdk"
"$ANDROID_HOME/emulator/emulator" -avd axon_test \
  -no-window -no-audio -gpu swiftshader_indirect -no-snapshot -accel on &
"$ANDROID_HOME/platform-tools/adb" wait-for-device
# poll until boot completes:
until [ "$("$ANDROID_HOME/platform-tools/adb" shell getprop sys.boot_completed 2>/dev/null | tr -d '\r')" = 1 ]; do sleep 2; done
echo "emulator booted"
ANDROID_HOME="$ANDROID_HOME" bash scripts/android_compute_parity.sh   # 12 runs, byte-identical to interp
```

Notes:
- x86_64 binaries run natively on the x86_64 system image; aarch64 binaries run on the same
  emulator via its built-in arm64 native bridge (`libndk_translation`).
- `qemu-aarch64-static` cannot run Android *bionic* ELFs (no `linker64` in the NDK sysroot),
  so the parity script uses the emulator, not qemu-user.
- The android gate scripts SKIP-guard on a missing NDK/emulator, so a bare CI is unaffected.

---

## 4. iOS — GitHub Actions macOS runner (the one off-host path)

iOS **cannot** be built or run on Linux: Xcode, the iOS SDK, the Simulator, and code-signing
are macOS-only and cannot be legally or technically substituted. The Axon iOS code is
therefore `cfg(target_os="ios")`/feature-gated (the Linux build never compiles it, and
`gate.sh` stays green), and the real verification runs in CI on Apple hardware:

- **`.github/workflows/ios.yml`** (`runs-on: macos-14`) — installs the `*-apple-ios` Rust
  targets, cross-builds the runtime for the simulator + device, checks the Mach-O archive and
  `axon_ios_*` symbol exports, boots an iOS Simulator (`xcrun simctl`), and runs the
  compute-parity oracle + the Metal `ios_sim_window_clear` journey.

This job runs automatically when a branch with iOS changes is pushed. There is no local
equivalent; a contributor without a Mac relies on the CI result.

---

## What still genuinely needs hardware (manual tier)

Even with all of the above, a few journeys remain manual because they need a real display or
device, and are intentionally **not** gated:

- An **on-screen** `winit` window (R13) — offscreen render is gated; the visible window is not.
- A full **Android NativeActivity GUI** frame loop (R14 S3) — the lifecycle *adapter* is gated
  headless; the real Activity + JVM GUI app is manual.
- **iOS on a physical device** (vs. the simulator) — needs provisioning/signing.

These are marked manual-tier in the relevant specs under `governance/specs/`.
