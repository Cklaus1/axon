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
| **eBPF — clang-bpf + kernel BTF** | `scripts/ebpf_verify.sh` (R23) | No (in-kernel verifier / rbpf) | `clang -target bpf -c -x c /dev/null -o /dev/null && echo ok` |
| **Zephyr RTOS — SDK + QEMU** | `scripts/zephyr_qemu_gate.sh` (R25) | No (QEMU Cortex-M) | `west --version && qemu-system-arm --version` |
| **TEE — Gramine sim / CI attest** | `scripts/tee_sim_run.sh`, `.github/workflows/tee.yml` (R24) | **attestation: TEE hardware only** → cloud CI | `axon check examples/tee/*.ax` (E1810 rule) |
| **Formal methods — TLC + Coq** | `scripts/r32_acceptance_gate.sh` (R32) | No | `bash scripts/r32_acceptance_gate.sh` (20/20 PASS, 0 SKIPPED) |

> ⚠️ **LLVM pin (read before installing eBPF tooling):** Axon codegen requires **LLVM 17**
> (`llvm-sys` 170). Installing the `llvm`/`clang` apt *metapackage* on Ubuntu 24.04 pulls
> **LLVM 18** and repoints `/usr/bin/llvm-config`, which breaks fresh codegen builds with
> `could not find native static library 'Polly'`. Install **`clang-18`** for the bpf target
> (it's independent of `llvm-config`) but keep `llvm-config` → 17:
> `sudo ln -sf /usr/bin/llvm-config-17 /usr/bin/llvm-config` (and/or
> `export LLVM_SYS_170_PREFIX=/usr/lib/llvm-17`). The setup script does this for you.

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

## 5. eBPF — clang-bpf + kernel verifier (R23)

The Axon→eBPF target (`axon build --target bpf`) emits an `elf64-bpf` object the Linux
verifier loads. Verification is fully local: the **in-kernel verifier** accepts the object
(or the userspace `rbpf` VM runs the bytecode).

```bash
sudo apt-get install -y clang-18 libbpf-dev linux-tools-common   # clang-18, NOT the llvm metapackage (see LLVM pin above)
sudo ln -sf /usr/bin/llvm-config-17 /usr/bin/llvm-config          # keep codegen on LLVM 17
```

Verify:

```bash
clang -target bpf -O2 -c -x c /dev/null -o /dev/null && echo "bpf target OK"
ls /sys/kernel/btf/vmlinux && echo "kernel BTF present (real verifier load works)"
bash scripts/ebpf_verify.sh            # builds the .bpf.o, loads it, asserts the verifier ACCEPTS
```

Note: WSL2's `/usr/sbin/bpftool` is a per-kernel wrapper that no-ops; `ebpf_verify.sh` uses a
direct `bpf(BPF_PROG_LOAD)` syscall loader (needs `sudo`), falling back to a structural check
or `rbpf` if unprivileged. SKIP-guards if clang-bpf is absent.

## 6. Zephyr RTOS — SDK + QEMU Cortex-M (R25)

`axon build --target zephyr` emits a `thumbv7m-none-eabi` object linked into a Zephyr app and
run under QEMU — no board needed.

```bash
pip install --break-system-packages west pyelftools
west init ~/zephyrproject && (cd ~/zephyrproject && west update && west zephyr-export)
# Zephyr SDK (arm toolchain):
SDK=0.17.0; wget -qO /tmp/z.tar.xz \
  https://github.com/zephyrproject-rtos/sdk-ng/releases/download/v$SDK/zephyr-sdk-${SDK}_linux-x86_64_minimal.tar.xz
mkdir -p ~/zephyr-sdk && tar xf /tmp/z.tar.xz -C ~/zephyr-sdk --strip-components=1
(cd ~/zephyr-sdk && ./setup.sh -t arm-zephyr-eabi -c)
sudo apt-get install -y qemu-system-arm cmake ninja-build device-tree-compiler gperf
```

> Gotcha: the bleeding-edge Zephyr tree requires `find_package(Zephyr-sdk 1.0)`; if the SDK's
> `~/zephyr-sdk/sdk_version` label reads below `1.0`, bump it to `1.0.0` (binaries unchanged).

Verify:

```bash
cd ~/zephyrproject && source zephyr/zephyr-env.sh
ZEPHYR_SDK_INSTALL_DIR=~/zephyr-sdk west build -p auto -b qemu_cortex_m3 zephyr/samples/hello_world -d /tmp/zb && west build -d /tmp/zb -t run   # prints "Hello World"
bash scripts/zephyr_qemu_gate.sh       # builds the Axon object, runs it on Cortex-M QEMU, asserts AXON / 23 / 42
```

## 7. TEE — Gramine simulation (local) + cloud attestation (CI) (R24)

The **type rule is local and gated** — `@[enclave]` / `E1810` ensures a sealed `Secret` is only
unsealed inside an enclave (`axon check examples/tee/*.ax`). **Real hardware attestation is the
one genuine off-host boundary** (this host has no SEV-SNP/TDX/SGX — `sme` only), so:

- Local: a Gramine *direct* (no-SGX) simulated run, if Gramine is installed. As of writing it is
  **not in the Ubuntu 24.04 apt repos** — add the Gramine apt repo to enable it; otherwise
  `scripts/tee_sim_run.sh` SKIP-guards and verifies the baseline enclave-env run + the E1810 rule.
- Real attestation: **`.github/workflows/tee.yml`** (`workflow_dispatch`, a self-hosted
  `[sgx, confidential]` runner) builds under Gramine-SGX, produces a DCAP quote bound to a nonce,
  and verifies it against Intel's PCS. Runs remotely on confidential hardware — never faked here.

```bash
axon check examples/tee/confidential_score.ax   # the E1810 Secret-unseal-only-in-enclave rule
bash scripts/tee_sim_run.sh                      # baseline enclave run (+ gramine-direct if installed)
```

---

## 8. Formal methods — TLC (TLA+) + Coq (R32)

R32's proof is only genuinely "machine-checked" when TLC and `coqc` actually run — a `.tla`/`.v`
file that merely has no `Admitted` and ends in `Qed` is **structurally valid, not verified**
(found the hard way 2026-07-18: neither tool had ever run against this file before, and doing so
for the first time surfaced two real proof bugs plus a gate-script bug that had been hiding behind
a SKIP; see `governance/specs/R32-formal-corrigibility-proof.md` §14). Both tools install cleanly
on a bare Ubuntu 24.04 host with **no hardware requirement**:

```bash
# TLC — a self-contained jar, no root needed beyond a JRE
sudo apt-get install -y default-jre
wget -q https://github.com/tlaplus/tlaplus/releases/download/v1.8.0/tla2tools.jar -O ~/tla2tools.jar

# Coq — apt's `coq` package is 8.18.0 on Ubuntu 24.04, matching the spec's pin exactly.
# (The spec's own §8 opam-from-source instructions are the portable fallback for hosts
# without this apt package; apt is faster wherever it's available.)
sudo apt-get install -y coq
```

Verify:

```bash
bash scripts/r32_acceptance_gate.sh   # → 20/20 PASS, 0 SKIPPED (was 18/20 + 2 SKIPPED before either tool was installed)
```

`scripts/setup-environments.sh formal` does the above idempotently and re-runs the gate to confirm.

---

## What still genuinely needs hardware (manual tier)

Even with all of the above, a few journeys remain manual because they need a real display or
device, and are intentionally **not** gated:

- An **on-screen** `winit` window (R13) — offscreen render is gated; the visible window is not.
- A full **Android NativeActivity GUI** frame loop (R14 S3) — the lifecycle *adapter* is gated
  headless; the real Activity + JVM GUI app is manual.
- **iOS on a physical device** (vs. the simulator) — needs provisioning/signing.
- **TEE hardware attestation** (R24) — a genuine SEV-SNP/TDX/SGX quote rooted in CPU keys
  needs confidential hardware; verified remotely via `tee.yml`, never on this host. The
  `@[enclave]`/`E1810` *type rule* is local and gated.

These are marked manual-tier in the relevant specs under `governance/specs/`.
