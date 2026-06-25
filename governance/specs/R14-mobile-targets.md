# Mobile Targets — iOS / Android via static lib + generated wrapper

**Spec ID:** `R14-mobile-targets` (ties to `REQUIREMENTS.md` R7; depends on R13 native-FFI, R7c browser-host)
**Status:** Draft
**Risk class:** Structural
**Author / date:** cklaus, 2026-06-07

---

### 1. Motivation

The PRD's mobile story (`AI_Language_Plan.md:294–316`) is: compile Axon to a native static lib, wrap it in a
~50-line auto-generated Swift/Kotlin shim the AI never edits, and render with the same wgpu GPU layer (Metal
on iOS, Vulkan on Android). Today this is **0% — entirely greenfield** (`MILESTONE.md`: "mobile runtimes …
remain greenfield"). Mobile is the *last* of the three platform blockers and the most downstream: it depends
on the native FFI layer (R13 — to drive wgpu) and reuses the same GPU shim as desktop and browser.

Two honest caveats reshape the "mostly toolchain" framing:

1. **iOS and Android are profoundly asymmetric.** Android cross-compiles cleanly from Linux (the NDK is a
   download; bionic libc; AArch64 ELF runs under QEMU-user). **iOS does not**: producing a Mach-O iOS binary
   requires the Apple SDK and Xcode's linker, which Apple licenses **only on macOS** — it cannot be built on
   this repo's Linux/WSL2 host. So `cross.toml`-configures-the-linker is true *only for Android* here; iOS is
   gated on a macOS build host (Q4). v1 is **Android-first and Linux-buildable**; iOS is specified but
   build-blocked in this environment.
2. **The "~50-line wrapper" and "mostly toolchain" claims undersell the real work.** Compute *is* mostly
   toolchain — R1 already emits AArch64. But a real app is not compute: the **OS owns the run loop**
   (`UIApplicationMain` / Android `Activity` lifecycle), so the wrapper's actual job is to bridge an
   OS-owned, inverted-control-flow lifecycle into Axon — which is the *same* suspend/resume problem as the
   native event loop (R13 Q3) and the browser frame loop (R7c). That bridge is a real runtime component, not
   a mechanical ~50-line shim, and it is gated on the Phase-6 `resume` runtime (see Dependencies, §4).

What R14 genuinely adds, then: the **cross-compile toolchain targets**, the **C-ABI library surface**, a
**wrapper/lifecycle-bridge runtime** (not just a generator), and the device-side `native::platform`/`gfx`
modules — with the compute layer being the easy, already-mostly-built part.

### 2. Requirement link

`REQUIREMENTS.md` **R7** (cross-platform; mobile is the explicitly-listed unbuilt sub-target). Acceptance
criterion: *the same `.ax` source that runs on native/wasm/browser compiles to an iOS `.a`+Swift wrapper and
an Android `.so`+Kotlin wrapper, each producing a runnable app whose Axon logic computes identically to the
native run (I-2), with UI rendered via wgpu's Metal/Vulkan backends.* The mobile platform stdlib
(`std.platform.*` — lifecycle, permissions, notifications, haptics, biometrics, `AI_Language_Plan.md:298–307`)
is delivered as a `native::platform` module (R13), not new language surface.

### 3. Surface (what the user writes)

**No new `.ax` surface for the language.** Mobile is a build target + two native modules (`gfx`, `platform`).
The surface is the build CLI and the generated-wrapper contract. `--host mobile` is a **new** flag (shared
with R7c's `--host` addition); the cross-link toolchain is configured through the **already-shipped**
`~/.config/axon/cross.toml` per-triple linker mechanism (`read_cross_linker`, `link.rs:217`), not a new one.

```bash
# iOS: AArch64 static lib + generated Swift wrapper + xcframework.
# (cross-linker for the triple read from ~/.config/axon/cross.toml, the existing mechanism)
axon target build --target aarch64-apple-ios --host mobile app.ax
#   → out/ios/libapp.a  +  out/ios/Axon.swift (generated, ~50 lines)  +  out/ios/Axon.xcframework

# Android: AArch64/x86_64 shared lib + generated Kotlin wrapper. [LANDED]
axon build --target aarch64-linux-android --host mobile app.ax
#   → out/android/jniLibs/arm64-v8a/libapp.so  +  out/android/Axon.kt (generated)

# Android compute-only executable (no wrapper) — the I-2 parity artifact:
axon build --target x86_64-linux-android app.ax -o app   # runnable via adb/qemu
```

The NDK toolchain is **auto-detected** from `$ANDROID_NDK_HOME`/`$ANDROID_NDK_ROOT`, else the
highest `$ANDROID_HOME/ndk/<version>`; the per-arch NDK clang (`aarch64-linux-android34-clang`
etc., API overridable via `$AXON_ANDROID_API`, default 34) is the linker. An explicit override is
the existing `~/.config/axon/cross.toml` mechanism — it takes precedence over auto-detection:

```toml
[target.aarch64-linux-android]
linker = "/path/to/ndk/.../bin/aarch64-linux-android34-clang"
[target.x86_64-linux-android]
linker = "/path/to/ndk/.../bin/x86_64-linux-android34-clang"
```

```axon
// Platform integration — the std.platform.* surface as a native module (R13), same source on every device.
use native::platform
use native::gfx

@[contained(platform: any, gfx: any)]
fn on_start(s: ref AppState) -> i64 {
  platform::request_permission("Camera")          // -> Result-shaped i64 (granted/denied/pending)
  platform::haptic("Light")
  let win = gfx::window_open(0, 0, "app")          // 0,0 = full-screen on mobile (no OS window concept)
  ...
}
```

```swift
// The wrapper = a hand-written, versioned `AxonRuntime` lifecycle bridge (in the TCB) + a tiny GENERATED
// per-app shim. The generated part is mechanical; the BRIDGE is not — it adapts the OS-owned run loop
// (UIApplicationMain / scene lifecycle) into Axon `on_start`/`on_resume`/`on_frame` callbacks, which needs
// the Phase-6 resume runtime to suspend/re-enter Axon across lifecycle events (see §4 Dependencies).
import AxonRuntime
@main struct App: AxonApp {            // AxonApp = the bridge; conforms to UIApplicationDelegate under the hood
  // generated: the entry-name bindings only (~10 lines). The run loop is owned by AxonRuntime, not by main().
  static let entries = AxonEntries(start: "on_start", frame: "on_frame", resume: "on_resume")
}
```

### 4. Semantics (what it does)

Mobile = (R1 AArch64 codegen) + (R13 native modules over Metal/Vulkan) + (a thin generated wrapper). The Axon
*execution* is unchanged native codegen; the new work is the toolchain and the wrapper. **Two foundations
already exist** (verified 2026-06-07): codegen takes an arbitrary `target_triple` and builds a `TargetMachine`
from it (`emit_object_and_link`, `link.rs:65–79`; `Target::initialize_all` at `link.rs:72` registers every
LLVM backend incl. AArch64), and a per-triple cross-linker is read from `~/.config/axon/cross.toml`
(`read_cross_linker`, `link.rs:217`). So mobile reuses both; it does not add a codegen path or a linker
mechanism.

| Input class | Behavior |
|---|---|
| pure-compute `.ax` → iOS/Android | Codegen emits AArch64 (already supported, R1 — `initialize_all` registers the backend) for the device triple; the program runs identically to a desktop native run (I-2). The static lib exports a C-ABI entry the wrapper calls. |
| `--target aarch64-apple-ios` | LLVM codegen with the iOS triple → `.o` (the `emit_object_and_link` triple path, already wired); the linker comes from the `[target.aarch64-apple-ios]` section of `cross.toml` (the Xcode `ld`/`clang` with the iOS SDK) → `.a`; packaged into an `.xcframework`. The axon-rt staticlib is cross-built for the triple (same as the wasm cross-build precedent). |
| `--target aarch64-linux-android` / `x86_64-linux-android` | LLVM codegen with the Android triple → `.so` via the NDK linker (configured in `cross.toml`); placed in the `jniLibs/<abi>/` layout; the Kotlin wrapper loads it via JNI. |
| UI program (`use native::gfx`) | wgpu's Metal backend (iOS) / Vulkan backend (Android) acquires a surface from the platform view (`CAMetalLayer` / `ANativeWindow`) handed in by the wrapper. Same `gfx` shim as desktop/browser — wgpu abstracts the backend. The event/render loop is driven by the platform display link (`CADisplayLink` / `Choreographer`), bridged to Axon `update`/`view` via the R13 callback ABI. |
| `use native::platform` (lifecycle/permissions/haptics/notifications/biometrics) | Each maps to a C-ABI fn in a per-OS half of the `platform` shim: iOS calls UIKit/AVFoundation/LocalAuthentication; Android calls the Android SDK via JNI upcalls into the Kotlin wrapper. Async permission prompts return a `pending` handle resolved via callback (R13 Q3). |
| permission denied / capability absent | Graceful Result (`denied`), never a crash (I-4). A `@[contained(platform:...)]`-ungranted call is E1004 at compile time. |
| interpreter run of a mobile program (on a dev machine) | The `platform`/`gfx` modules' interp impls return clean refusals / stubs when there's no device (headless), exactly like R13's headless handling — so `axon run app.ax` on a laptop type-checks and runs the non-device logic, stubbing device calls (I-9: clean refusal, not silent success). |

**The wrapper = bridge runtime + generated shim (not one ~50-line file).** Two parts:
- **`AxonRuntime` lifecycle bridge** — hand-written once per OS, *versioned, in the TCB*. It owns the OS run
  loop (`UIApplicationMain` / `Activity`) and translates lifecycle + frame + input events into Axon callbacks,
  suspending and re-entering the Axon computation via the Phase-6 `resume` runtime. This is the genuinely hard
  part and is **not** mechanical, **not** byte-stable boilerplate, and **not** ~50 lines.
- **Generated per-app shim** — *this* part is mechanical/deterministic/byte-stable: ~10 lines binding the
  app's exported C-ABI entry names (`on_start`/`on_frame`/`on_resume`) into the bridge. The "~50 lines"
  figure (§9) applies to this shim, not to the bridge.

#### Dependencies — compute now, interactive after Phase-6 (and iOS after a macOS host)

| Target | Needs |
|---|---|
| **Android compute** (cross-compiled `.so`, QEMU-verifiable, no lifecycle) | This spec, slices 1–2. **No Phase-6, no macOS, Linux-buildable now.** |
| **Interactive Android app** (lifecycle, frame loop, `native::platform`) | This spec **+ the Phase-6 `resume` runtime** (unbuilt) **+ R-UI** + a device/emulator (manual tier). |
| **Any iOS target** | All of the above **+ a macOS build host** (Apple SDK + Xcode linker; not available on this repo's host — Q4). |

Same two-tier reality as R13/R7c — the compute layer is reachable now (Android), and everything interactive
converges on the one unbuilt runtime capability (suspend/resume). iOS adds a hard environmental gate on top.
The platform critical path is **R14 (Android compute) → Phase-6 resume → R-UI**, with iOS branching off at a
macOS host.

### 5. Type rules

N/A — no language-surface or type change. Mobile reuses R13's `Handle`/FFI-repr type rules and R1's AArch64
codegen unchanged. (A future `@[target(ios)]` conditional-compile attribute — R7 §12 Q3 — stays deferred; the
same source compiles for every device.)

### 6. Error codes

| Code | Trigger | Message shape |
|---|---|---|
| E1710 | `--target <mobile-triple>` with the SDK/NDK toolchain not found | `mobile target '<triple>' requires <Xcode SDK / Android NDK>; not found on PATH (<hint>)` |
| E1711 | axon-rt / a native module not cross-built for the device triple | `'<crate>' not available for '<triple>'; cross-build it first (<hint>)` |
| E1712 | mobile link failed (xcframework / jniLibs packaging) | `mobile link failed for '<triple>': <linker detail>` |
| (reuse) E1004 | ungranted `use native::platform`/`gfx` | (existing capability-denied message) |
| (reuse) E1701–E1703 | FFI-repr / handle errors at a `platform`/`gfx` call | (R13 codes) |

(Allocate from the same E17xx band R13 opens; confirm the exact free numbers against `error.rs` — I-14.)

### 7. Invariants touched

- **I-2 (parity)** — *preserved*: mobile is R1 native codegen for a different triple; the Axon logic is the
  same compiled IR. Device calls go through R13 native modules (already parity-bound). A mobile compute result
  is provably the desktop-native result (same codegen, AArch64 vs x86-64 — guarded by running the
  cross-compiled compute tests under a device/simulator or QEMU in the parity harness).
- **I-11 (capability boundary)** — *preserved*: `platform`/`gfx` are capability-gated native modules (R13);
  the OS permission system is a *second* runtime layer beneath `@[contained]` (camera/location/etc. require
  both the Axon grant and the OS grant).
- **I-4 (never abort the host)** — *preserved*: permission denials and missing-device cases are graceful
  Results; the wrapper marshals a panic into a clean app-level error, never an uncaught crash.
- **I-12 (self-mod cannot weaken TCB)** — the `platform` module is TCB; the wrapper is generated from a fixed
  template (not AI-authored), so a self-improving pass cannot inject native code into the wrapper to escape.
- **I-6 (frozen layouts)** — reuses str/array/Handle layouts at the C-ABI boundary; no new layout.

### 8. Test plan

- [ ] Unit: wrapper generator emits a byte-stable Swift/Kotlin wrapper for a given entry signature (golden
      test; the generator is deterministic).
- [ ] Integration: axon-rt + the mock native module (R13) cross-build for `aarch64-apple-ios` and
      `aarch64-linux-android` (a `cargo build --target` gate; E1711 guards regressions).
- [ ] CLI e2e: `axon target build --target aarch64-linux-android --host mobile app.ax` emits `.so` + `.kt` +
      jniLibs layout; the `.so` is ELF-verified for the right arch; the wrapper compiles with `kotlinc`.
- [ ] Parity (compute): the pure-compute example set, cross-compiled for the device triple, run under a
      simulator/emulator (or QEMU-user for the Android AArch64 ELF) → same exit/stdout as the interpreter
      oracle. Manual/`#[ignore]` tier until CI has a simulator.
- [ ] Adversarial: missing SDK → E1710; uncross-built rt → E1711; ungranted `platform` call → E1004;
      permission-denied → graceful Result not crash.
- [ ] Journey (UI): `examples/gfx/window_clear.ax` built for iOS simulator + Android emulator renders one
      cleared frame via Metal/Vulkan. Manual tier (needs simulators).

### 9. Acceptance criteria (the done gate)

**Android landed (gate-verified on this Linux host with NDK 27 + a KVM x86_64 emulator), 2026-06-25:**

- [x] Test `mobile_wrapper_is_byte_stable` passes (golden wrapper, deterministic generator) —
      `mobile::tests::wrapper_is_byte_stable_golden` (asserts byte-stable Kotlin output + ≤80 lines).
- [x] axon-rt **and** axon-ai cross-build for `aarch64-linux-android` + `x86_64-linux-android` via the NDK
      (`build_crate_staticlib(..., Some(triple))`, the NDK linker/ar/CC auto-wired); E1710/E1711 guard the
      toolchain-absent / un-cross-built cases. (iOS cross-build is gated on a macOS host, §4/Q4 — NOT done.)
- [x] **`android_compute_parity` passes** (the `android_so_compute_parity` acceptance gate): the pure-compute
      example set, cross-compiled to Android x86_64 **and** AArch64, runs on the device and returns the
      interpreter-oracle value byte-for-byte (stdout + exit). **Execution path:** x86_64 runs NATIVELY on the
      KVM-accelerated emulator (`adb push`+`adb shell`); AArch64 runs on the SAME emulator via its
      `libndk_translation` arm64 native bridge (qemu-user is a fallback). 12 runs (6 examples × 2 arches) all
      identical. `scripts/android_compute_parity.sh` + the `android_compute_parity_r14` integration test
      (self-skips when NDK/emulator absent so the default gate stays safe).
- [x] `axon build --host mobile --target <android> app.ax` emits a **loadable `.so`** into
      `out/android/jniLibs/<abi>/lib<name>.so` + the generated `out/android/Axon.kt` wrapper. The `.so` is
      ELF-verified for the right arch and exports the Axon entry symbol (`main`); `kotlinc` compilation of the
      wrapper is the manual tier (the wrapper is golden-tested for byte-stability instead). CLI e2e adversarial
      cases verified: E1710 (no NDK), iOS-on-Linux refusal, unknown `--host`, missing Android target.
- [x] Generated wrapper is **≤ 80 lines** (asserted in the golden test).
- [ ] (Stretch / **manual tier**) `ios_sim_window_clear` and `android_emu_window_clear` render one frame —
      NOT run; the GPU/Vulkan surface bridge (slice 5) and a full NativeActivity GUI app are the manual tier.

### 10. Performance budget

Binary size mirrors the PRD's ~3.5 MB GPU-stack figure (`AI_Language_Plan.md:288`); the non-UI lib should be
well under that. The compute parity tests double as a perf guard (AArch64 codegen should be within the same
order as x86-64 native; no mobile-specific hot path). Wrapper generation is build-time, negligible.

### 11. Rollout & rollback

- Behind `--host mobile` + per-target `--features ios`/`--features android`. Inert otherwise;
  native/wasm/browser untouched.
- Slices, each revertible (**Android-first**; iOS variants of 1–2 gated on a macOS host):
  1. **[LANDED 2026-06-25] Cross-compile toolchain targets (Android)** — axon-rt + axon-ai cross-build for the
     Android triples via the NDK (auto-detected from `$ANDROID_NDK_HOME`/`$ANDROID_HOME/ndk/<ver>`, or an
     explicit `cross.toml` linker override); the NDK clang is the linker (PIE bionic ELF), `llvm-ar`/CC wired
     for the staticlib cross-build; E1710 (NDK absent) / E1711 gates. Pure toolchain + config; **no codegen
     change** (R1 AArch64 + `target_triple` plumbing already existed). `link.rs::{is_android_triple,
     android_linker, android_link, build_crate_staticlib}`. Linux-buildable. Revertible. (iOS counterpart
     deferred to a macOS host.)
  2. **[LANDED 2026-06-25] C-ABI entry export + generated shim + Android compute parity** — `--host mobile`
     emits a loadable `.so` (`emit_shared_lib`, `-shared`+`--export-dynamic`) into `jniLibs/<abi>/` plus the
     deterministic Kotlin wrapper (`mobile.rs`, golden-tested). Compute parity proven on the KVM emulator (+
     arm64 native bridge) — `scripts/android_compute_parity.sh`. The load-bearing *compute* slice; no lifecycle
     UI yet.
  3. **[LANDED 2026-06-25, headless tier] `AxonRuntime` lifecycle bridge** — the run-loop adapter contract:
     the generated `Axon.kt` binds `on_start`/`on_frame`/`on_resume` into the hand-written `AxonRuntime` bridge,
     which suspends/re-enters Axon across lifecycle events via the **landed Phase-6 `host_await` resume
     runtime**. Verified headless two ways (`scripts/android_lifecycle.sh`): (A) the
     `examples/mobile/lifecycle.ax` resume round-trip (init→tick→suspend→resume→teardown as one linear Axon fn,
     deterministic transcript + exit=frames) on the host substrate the bridge drives; (B) the on-device
     load→bind→invoke boundary — the Axon `.so` is `dlopen`+`dlsym("main")`+called on the emulator, returning
     the interp-oracle value (the AxonRuntime→native path minus the JVM). **MANUAL tier (not run headless):** a
     full NativeActivity GUI app + a JVM `kotlinc` frame loop.
  4. **`native::platform` module** — lifecycle/permissions/haptics/etc. over JNI (Android) / UIKit (iOS).
     Deferred (headless device shim returning clean refusals is the next increment).
  5. **gfx Metal/Vulkan surface bridge** — wgpu surface from the platform view; the UI journey tier. Deferred.
- Blast radius: confined to mobile builds. `git revert` of any slice leaves a clean tree. Slices 1–2 (headless
  mobile compute) are independently useful even if 4–5 never land.

### 12. Open questions

- **Q1 (the decisive fork): does mobile need a new codegen backend, or just toolchain + wrapper?** Resolved:
  **toolchain + wrapper.** R1 already emits AArch64 via LLVM — the device ISA — and `emit_object_and_link`
  already accepts an arbitrary triple (`link.rs:65`) with the cross-linker resolved from `cross.toml`
  (`link.rs:217`), so the codegen+link *plumbing* is in place; mobile adds **no codegen path**. What it adds is
  cross-built artifacts (axon-rt + native modules for the device triples), the `cross.toml` linker entries
  (SDK/NDK), a C-ABI *entry* export, and a *wrapper generator*. This is the same shape as the wasm cross-build
  (axon-rt for a new triple), not a new engine. This is why mobile is "mostly toolchain" despite being 0%
  today — and it's *less* greenfield than first stated, because the triple+cross-linker mechanism already
  shipped.
- **Q2 (Swift/Kotlin wrapper: generated vs hand-written runtime crate).** Lean **a fixed `AxonRuntime`
  Swift/Kotlin support library (hand-written once, versioned) + a tiny generated per-app `@main` shim** —
  rather than generating the whole runtime each build. The generated part is just the entry-call (~10 lines);
  the "~50 lines" is shim + minimal glue. Keeps the generator trivial and the runtime testable.
- **Q3 (async platform APIs — permissions, biometrics).** These are callback/async on both OSes. v1 returns a
  `pending` handle resolved via the R13 callback ABI; a clean `await` surface is a follow-on (shared with
  R7c Q4).
- **Q4 (which triples in v1, and the macOS-host wall).** **Android ships first and is Linux-buildable now:**
  `aarch64-linux-android` + `x86_64-linux-android` (emulator). **iOS (`aarch64-apple-ios`) is specified but
  build-blocked in this repo's environment** — Mach-O + the iOS SDK require Apple's toolchain on macOS, which
  the Linux/WSL2 host does not have and cannot legally substitute. iOS slices are real but gated on a macOS
  CI/build host; they must not be marked DONE from a Linux run. iOS simulator (`aarch64-apple-ios-sim`) and
  32-bit Android deferred. Acceptance tests for iOS live in the manual/macOS tier; the Linux gate covers
  Android end-to-end (compute) and iOS only up to "object emits for the triple" (which LLVM can do
  cross-platform — it's the *link* that needs macOS).
- **Q5 (signing / packaging into `.ipa`/`.apk`).** Out of scope for v1 — the build emits the lib + wrapper +
  framework/jniLibs; final `.ipa`/`.apk` packaging and code-signing are left to the platform's own toolchain
  (`xcodebuild`/`gradle`). The PRD's "→ .ipa/.apk" is the *eventual* shape; v1 stops at the loadable artifact.
