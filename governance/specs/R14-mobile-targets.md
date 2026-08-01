# Mobile Targets — iOS / Android via static lib + generated wrapper

**Spec ID:** `R14-mobile-targets` (ties to `REQUIREMENTS.md` R7; depends on R13 native-FFI, R7c browser-host)
**Status:** 🚧 Implementing (re-verified 2026-07-18) — **Slices 1–3 LANDED (2026-06-25), Slices 4–5
Deferred.** Android: cross-compile toolchain, C-ABI `.so` export + generated Kotlin wrapper +
compute parity, `AxonRuntime` lifecycle bridge (host_await-based suspend/resume across
init→tick→suspend→resume→teardown) all verified headless on this host —
`cargo test --lib mobile::` 14/14 PASS; `cli_run` `android_compute_parity_r14` +
`android_lifecycle_adapter_r14` PASS. iOS counterparts of Slices 1–3 are authored + Linux-gated
but only verifiable for real by the remote macOS CI job (Apple SDK is macOS-only) — and that job
(`ios.yml`) is currently RED: its sole run ever (2026-06-26) failed at the simulator-parity step
via a guard bug (see the §9 legend note, 2026-07-31); the cross-build/Mach-O/wrapper steps within
that run did pass. See §11's iOS-slice status table. Slice 4 (`native::platform` full device impl) and Slice 5 (gfx Metal/Vulkan
surface bridge) are genuinely **not started** past a headless refusal stub — this is real,
correctly-tracked remaining scope, not staleness. This header previously said a bare "Draft" with
no detail, misleadingly implying zero progress on a spec whose own body already tracked landed
slices accurately; same staleness class as R17/R21/R22/R23/R26/R27/R28/R29/R31/R32/R12, caught by
the same outer-loop sweep (`EXECUTION_MODEL.md` §2) — but note this one needed a genuinely partial
status, not a mechanical Draft→Landed flip.

**ASI-trajectory pass, 2026-07-31.** §12 Q1's engineering premise ("mobile = toolchain + C-ABI export +
wrapper/lifecycle bridge, no new codegen path") **holds** — it is a toolchain claim, not a capability claim,
and the tree backs it. R14's *safety* framing did not. Corrections landed this pass: §7's I-12 row was
**false** as written (the wrapper template is fixed, its substitutions are unvalidated — §7 I-12, E1713);
I-11 is unresolved below "may talk to the OS permission system at all" (§7 I-11, Q6); §3's own example
derives Risk = **Low** and skips the Phase-11 gate chain (§7 Phase-11 row); the shipped `.so` has **no
binding to the approved AST** (§7 approval row, Q8); the artifact exports every symbol, not the three
reviewed entries (§9 KNOWN HOLE 2). A new **§7a** states the threat model and the currently-unenforced
reliance on human review. The fixes are cheap and reuse landed machinery (approval records, `.axmeta`, R28
ledger, R1f differential fuzzer, effect rows) — scoped as §11 slices 6 and 7a–7c.

```spec-meta
id: R14-mobile-targets
status-claim: Implementing
depends-on: R13-native-ffi
blocks: none
blocked-by: none
supersedes: none
related: R7-targets, R15-resume-runtime
conflicts-with: none
reserves: E1710, E1711, E1712, E1713, E1714 (mobile toolchain/cross-build/link; E1712 added 2026-07-31 — it was defined in §6 + allocated/emitted in error.rs / codegen/link.rs but omitted here. E1713 (wrapper identifier validation) + E1714 (entry-signature validation) reserved 2026-07-31 by the ASI-trajectory pass, §6 — NOT yet implemented)
evidence: cargo test --lib mobile:: --no-default-features -p axon-core (14 tests, re-verified 2026-07-18); scripts/android_compute_parity.sh; scripts/android_lifecycle.sh
```
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
`~/.config/axon/cross.toml` per-triple linker mechanism (`read_cross_linker`, `codegen/link.rs:608`), not a new one.

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

@[contained(platform: any, gfx: any)]                 // ← TODAY (unresolved grant; see §7 I-11 and Q6)
fn on_start(s: ref AppState) -> i64 {
  platform::request_permission("Camera")          // -> Result-shaped i64 (granted/denied/pending)
  platform::haptic("Light")
  let win = gfx::window_open(0, 0, "app")          // 0,0 = full-screen on mobile (no OS window concept)
  ...
}
```

**Target surface (§11 slice 7 / Q6 — NOT implemented).** The grant above is a single flat token and the
permission name is a runtime `str`, so the approved AST does not show the approver which OS permission the
end user will be asked for (§7 I-11). The intended shape is R13's promised *structured* sub-capability grant,
with the permission argument required to be a **string literal** (reusing the checker's existing `const_eval`
path — the same machinery that discharges constant refinement predicates), and any request outside the grant
rejected E1004:

```axon
@[contained(platform: [permission("Camera"), haptic], gfx: [present, clear])]   // ← TARGET
fn on_start(s: ref AppState) -> i64 {
  platform::request_permission("Camera")   // literal required; a non-literal is E1004/E1209
  platform::request_permission(chosen)     // REJECTED — not statically resolvable, not in the grant
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
already exist** (verified 2026-06-07; citations refreshed 2026-07-31 — the file moved to
`codegen/link.rs` and lines drifted): codegen takes an arbitrary `target_triple` and builds a `TargetMachine`
from it (`emit_object_and_link`, `codegen/link.rs:252`; `Target::initialize_all` at `codegen/link.rs:266`
registers every LLVM backend incl. AArch64), and a per-triple cross-linker is read from
`~/.config/axon/cross.toml` (`read_cross_linker`, `codegen/link.rs:608`). So mobile reuses both; it does not
add a codegen path or a linker
mechanism.

| Input class | Behavior |
|---|---|
| pure-compute `.ax` → iOS/Android | Codegen emits AArch64 (already supported, R1 — `initialize_all` registers the backend) for the device triple; the program runs identically to a desktop native run (I-2). The static lib exports a C-ABI entry the wrapper calls. |
| `--target aarch64-apple-ios` | LLVM codegen with the iOS triple → `.o` (the `emit_object_and_link` triple path, already wired); the linker comes from the `[target.aarch64-apple-ios]` section of `cross.toml` (the Xcode `ld`/`clang` with the iOS SDK) → `.a`; packaged into an `.xcframework`. The axon-rt staticlib is cross-built for the triple (same as the wasm cross-build precedent). |
| `--target aarch64-linux-android` / `x86_64-linux-android` | LLVM codegen with the Android triple → `.so` via the NDK linker (configured in `cross.toml`); placed in the `jniLibs/<abi>/` layout; the Kotlin wrapper loads it via JNI. |
| UI program (`use native::gfx`) | wgpu's Metal backend (iOS) / Vulkan backend (Android) acquires a surface from the platform view (`CAMetalLayer` / `ANativeWindow`) handed in by the wrapper. Same `gfx` shim as desktop/browser — wgpu abstracts the backend. The event/render loop is driven by the platform display link (`CADisplayLink` / `Choreographer`), bridged to Axon `update`/`view` via the R13 callback ABI. |
| `use native::platform` (lifecycle/permissions/haptics/notifications/biometrics) | Each maps to a C-ABI fn in a per-OS half of the `platform` shim: iOS calls UIKit/AVFoundation/LocalAuthentication; Android calls the Android SDK via JNI upcalls into the Kotlin wrapper. Async permission prompts return a `pending` handle resolved via callback (R13 Q3). |
| permission denied / capability absent | Graceful Result (`denied`), never a crash (I-4). A `@[contained(platform:...)]`-ungranted call is E1004 at compile time. |
| interpreter run of a mobile program (on a dev machine) | The `platform`/`gfx` modules' interp impls return clean refusals / stubs when there's no device (headless), exactly like R13's headless handling — so `axon run app.ax` on a laptop type-checks and runs the non-device logic, stubbing device calls (I-9: clean refusal, not silent success). **Caveat (2026-07-31):** `is_device()`→0 makes this an *observable* eval-vs-device discriminator — see §7a and Q7. |

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
| E1713 | **[RESERVED 2026-07-31, NOT implemented]** app/lib name or entry name outside the wrapper identifier alphabet `^[A-Za-z_][A-Za-z0-9_]{0,63}$` | `mobile wrapper name '<name>' is not a valid identifier; rename the source file (wrapper generation refuses to interpolate it)` |
| E1714 | **[RESERVED 2026-07-31, NOT implemented]** a fn named by `@[entry]`/`@[frame]`/`@[resume]` under `--host mobile` whose signature is not the frozen C-ABI scalar shape `(i64) -> i64` | `mobile entry '<fn>' has signature <sig>; --host mobile entries must be (i64) -> i64` |
| (reuse) E1004 | ungranted `use native::platform`/`gfx` | (existing capability-denied message) |
| (reuse) E1800–E1803 | FFI-repr / handle errors at a `platform`/`gfx` call | (R13 codes) |

(Corrected 2026-07-31: this table previously said "reuse E1701–E1703" and "allocate from the same
E17xx band R13 opens" — both wrong. E1700–E1707 are **R17's** freestanding/HAL codes; R13's
native-FFI codes landed in the free **E18xx** band (`error.rs`: E1800–E1803), and mobile boundary
errors reuse those, per error.rs's own R14 comment. E1710–E1712 are correctly allocated from the
free tail of the E17xx band and are the only R14-owned codes.)

(Corrected 2026-07-31, second pass: the **E1711 row states INTENDED semantics that are not
implemented**. Its only emission site is the iOS object-emission failure path (`main.rs:2394`;
`error.rs:166` labels it "iOS-specific staging code", and the adjacent comment admits it "stands in
for the cross-built-rt gate"). The specced trigger — a native crate not cross-built for the device
triple — never fires on Android: `build_crate_staticlib` returns `None` and `android_link` silently
omits the lib (see §8 adversarial). Treat E1711 as unimplemented-as-specced until the Android
emission site and a test land; the table row is retained as the target semantics.)

### 7. Invariants touched

- **I-2 (parity)** — *preserved, at a stated scope*: mobile is R1 native codegen for a different triple; the
  Axon logic is the same compiled IR. Device calls go through R13 native modules (already parity-bound).
  (Corrected 2026-07-31, ASI-trajectory pass: this row previously said a mobile compute result is
  **provably** the desktop-native result. It is not *proven* — no SMT machinery is involved. What is
  verified is: **byte-identical stdout+exit over the 6-example pure-compute corpus × 2 arches, for the
  tested Android triples, at the last manual run.** That is a curated smoke corpus chosen by the same
  people who wrote the codegen; it is adequate for hand-authored programs and weak as a claim about
  machine-generated programs at volume, whose input space it was not chosen to cover. See §11 slice 6 for
  the differential-fuzz extension that would widen it.)
- **I-11 (capability boundary)** — *partially preserved; resolution gap recorded*: `platform`/`gfx` are
  capability-gated native modules (R13); the OS permission system is a *second* runtime layer beneath
  `@[contained]` (camera/location/etc. require both the Axon grant and the OS grant). **What is NOT preserved
  is resolution below "may talk to the OS permission system at all"** (found 2026-07-31): `native::platform`
  carries a single flat grant key (`capability: "platform"`, `native.rs:543`) and a blanket
  `effects: &["IO"]` (`native.rs:545`), and the permission *name* is an ordinary runtime `str` argument
  (`native.rs:518–524`) — so `platform::request_permission(x)` with a non-literal `x` (including one derived
  from `ai_complete` output) is indistinguishable in the approved AST from `request_permission("Camera")`.
  Under ROADMAP §2.4 the approved AST is the legal artifact, so an approver reading `platform: any` cannot
  determine which OS permission the app will request of the end user — the permission set can be selected
  after review. R13's promised structured sub-capabilities (`@[contained(gfx: [present, clear])]`) are
  **not implemented** for `platform`. Closing this is §11 slice 7 / §12 Q6; until it lands, treat
  `platform: any` as an *unresolved* grant, not a bounded one.
- **I-4 (never abort the host)** — *preserved*: permission denials and missing-device cases are graceful
  Results; the wrapper marshals a panic into a clean app-level error, never an uncaught crash.
- **I-12 (self-mod cannot weaken TCB)** — **CORRECTED 2026-07-31 (ASI-trajectory pass): the previous wording
  of this row was false.** It read: "the wrapper is generated from a fixed template (not AI-authored), so a
  self-improving pass cannot inject native code into the wrapper to escape." The *template* is fixed; its
  **substitutions are not validated**. `main.rs:2808–2812` (and `main.rs:2334–2337` for `axon target build`)
  derive the app/lib name from `first.file_stem()` with no sanitization, and that value is interpolated raw
  into `mobile.rs:297` (`package app.axon.{app}`), `:303` (`System.loadLibrary("{app}")`), `:302`
  (`class {app}Activity`), `:402` (`init {{ System.loadLibrary("{lib}") }}`) and `:274` (Swift
  `struct {app}App`). POSIX filenames admit `"`, `{`, `}`, `(`, `)`, `;`, and newlines, so a source-file stem
  can close the string/object body and inject arbitrary Kotlin/Swift into a file this spec itself designates
  as TCB and "DO NOT EDIT". The golden tests cited as the guard
  (`wrapper_is_byte_stable_golden`, `swift_shim_is_byte_stable`) only ever pass benign stems (`app`, `Demo`,
  `compute`, `Game`) — determinism over one benign input says nothing about the reachable input space, and in
  the project's own stated direction (`.ax` as a machine-generated IR) the filename is generator-controlled,
  which is exactly the "self-improving pass" this row names.
  **Honest restatement (the invariant R14 may claim once §11 slice 7a lands):** *the wrapper is generated
  from a fixed template over a **validated identifier alphabet**; the generator hard-errors (E1713) on any
  app/lib or entry name outside `^[A-Za-z_][A-Za-z0-9_]{0,63}$` rather than sanitizing it silently (a
  rewritten lib name would silently break `System.loadLibrary`).* Until that lands, I-12 is **claimed but not
  held** for the mobile wrapper.
- **I-6 (frozen layouts)** — reuses str/array/Handle layouts at the C-ABI boundary; no new layout. **Not
  enforced at the mobile entry boundary** (found 2026-07-31): `generate_kotlin_wrapper` unconditionally emits
  `external fun {n}(arg: Long): Long` for whatever fn `MobileEntries::from_program` resolved
  (`mobile.rs:345–384`), and nothing validates that the named Axon fn actually has that signature — so
  `@[entry] fn boot(cfg: str) -> Point` yields a wrapper declaring an ABI that does not exist, discovered as
  UB at the JNI call on device, far from the build. E1714 (§6) is the fix site.
- **Phase-11 risk interaction — *NOT preserved; recorded as a gap* (found 2026-07-31).** `derive_risk_from_ast`
  (`main.rs:5236–5283`) recognises only `exec`, `net`, and `fs`/`io`/`write`; it has **no case for `platform`
  or `gfx`**. §3's own example program (`@[contained(platform: any, gfx: any)]`, no declared effect row)
  therefore derives Risk = **Low (0)**, and `cmd_deploy`'s `requires_full_pipeline = risk >= 2`
  (`main.rs:5352`) takes the short verify→deploy path: **no simulate, no stress, no `redteam_check`, no
  quorum.** So the artifact class with the widest blast radius this project produces — a signed binary
  installed on an end user's phone with camera/microphone/biometric reach — is the class the landed risk gate
  currently rates *lowest*. The only thing standing between a camera-requesting generated app and an ungated
  deploy is the operator-supplied `--risk` flag. Fix tracked as §11 slice 7b.
- **Approval / attestation binding — *ABSENT; recorded as a gap* (found 2026-07-31).** Before this pass, this
  spec contained zero occurrences of approve/audit/attest/ledger/provenance. In the tree,
  `cmd_target_build` (`main.rs:2318–2331`) and the `--host mobile` path (`main.rs:2937–2955`) type-check and
  emit `out/android/jniLibs/<abi>/lib<name>.so` + `Axon.kt` — **neither consults `<file>.ax.approved`**; only
  `cmd_deploy` does (`main.rs:5355`), and even there an absent record is a *warning*, not a block
  (`main.rs:5504`). No risk is derived, no R28 ledger record is written, and `.axmeta` is opt-in behind
  `emit_manifest`. ROADMAP §2.4 makes the approved AST the artifact that "bug reports, audits, and rollbacks
  reference" — yet given a `.so` recovered from a phone there is currently **no way to determine which AST was
  approved to produce it**. §12 Q5 defers signing/packaging to `xcodebuild`/`gradle`, which is precisely where
  that binding would be anchored; the deferral was stated without noticing what it gives up. Fix tracked as
  §11 slice 7c / §12 Q8.

#### 7a. Threat model — assumed generator, and where it expires

R14's safety framing was written against a **careless** author: one that might get a target triple wrong or
call an ungranted module, whose mistakes are caught by E1710/E1004 and a golden test. Several of the gaps
above are invisible under that assumption and load-bearing under an **optimizing** one — an author that reads
this spec and selects behaviour against the acceptance gate. Stated explicitly so it can expire on purpose:

- **Stated limit (currently relied upon, NOT enforced):** R14's mobile artifact path assumes **a human reviews
  each generated app before it ships**. Nothing in the toolchain enforces this — `--host mobile` emits a
  distributable artifact with no approval record, no risk derivation, and no attestation (see the
  approval/attestation row above). This assumption expires the moment mobile artifacts are produced at a rate
  a human cannot review, which is the project's own stated direction. §11 slices 7a–7c convert the
  assumption into an enforced gate; until then it is a **documented reliance on human review, not a property
  of the system**.
- **Eval-vs-deployment discriminator.** `platform::is_device()` (`native.rs:566`, →0 headless) is a shipped,
  documented, first-class predicate for "am I in an audited path right now" — see §4's headless row and the
  note appended there. Every audited path (`axon ast review`, the interpreter oracle in
  `android_compute_parity.sh`, `redteam_check` under `axon deploy`, the §8 journey headless mock, every
  `@[test]`) observes `is_device()==0`; **nothing in the audited path ever observes the device branch.** The
  I-9 "clean refusal, not silent success" rationale for the stub is right for a careless author and hands an
  optimizing one exactly the branch it needs. Tracked as §12 Q7.
- **Manual-tier throughput.** The checks that actually exercise the device path are human-executed (§8/§9:
  `kotlinc` wrapper compile, the simulator/emulator journeys, and the Android compute-parity gate itself,
  which self-skips absent NDK/emulator). A manual tier is a viable acceptance path at one mobile app per
  quarter and is not one at a continuous generation rate; a self-skipping gate is also indistinguishable from
  a passing gate to the R10 4-gate firewall's G3 test-preservation check, so a self-improving pass sees **no
  cost** in touching mobile code paths. Tracked as §11 slice 6 / §12 Q9.
- **Exported-symbol surface.** The reviewed contract is three C-ABI entries; the artifact exports everything
  (§9 KNOWN HOLE 2), so reachability arguments over the three reviewed entries do not constrain the shipped
  `.so`.

### 8. Test plan

(Checkboxes ticked 2026-07-31 to match §9's landed status — they had all been left unchecked even for
items §9 marks gate-verified. **Second pass, same day:** the first pass over-ticked — the E1711
adversarial clause and the iOS parity leg were marked verified on evidence that does not support
them; both are now split out and un-ticked below, with the refutations inline.)

- [x] Unit: wrapper generator emits a byte-stable Swift/Kotlin wrapper for a given entry signature (golden
      test; the generator is deterministic) — `mobile::tests::wrapper_is_byte_stable_golden` +
      `swift_shim_is_byte_stable` / `kotlin_shim_is_byte_stable` (§9).
- [x] Integration: axon-rt + the mock native module (R13) cross-build for `aarch64-apple-ios` and
      `aarch64-linux-android` (a `cargo build --target` gate). Android Linux-verified; the iOS
      cross-build runs REMOTE-CI-ONLY (`ios.yml`, §9 — currently red overall, see the legend note;
      the cross-build STEP passed within that sole run). (Corrected 2026-07-31, second pass: this
      item previously claimed "E1711 guards regressions" — it does not; see the un-ticked
      adversarial item below.)
- [x] CLI e2e: `axon target build --target aarch64-linux-android --host mobile app.ax` emits `.so` + `.kt` +
      jniLibs layout; the `.so` is ELF-verified for the right arch. (`kotlinc` wrapper compile stays the
      manual tier; the wrapper is golden-tested for byte-stability instead — §9.)
- [x] Parity (compute) — **Android leg only**: the pure-compute example set, cross-compiled for the
      device triple, run under a simulator/emulator (or QEMU-user for the Android AArch64 ELF) →
      same exit/stdout as the interpreter oracle — Android landed
      (`scripts/android_compute_parity.sh`, self-skips when NDK/emulator absent). (Corrected
      2026-07-31, second pass: the iOS leg is **un-ticked** — its delegated remote verifier is the
      exact `ios.yml` simulator-parity step that failed (exit 127) in the workflow's sole run ever,
      2026-06-26; see the §9 legend note. Also note the Android leg has **no standing automated
      re-run**: Linux CI has no NDK, the gate self-skips there, so it is re-verified only by manual
      runs on the NDK host — last 2026-06-25, re-run 2026-07-18.)
- [ ] Parity (compute), iOS leg: the iOS-Simulator run is REMOTE-CI-ONLY and its verifier
      (`ios.yml`'s simulator-parity step) is currently the failing step of the workflow's only run.
- [x] Adversarial (verified subset): missing SDK → E1710; ungranted `platform` call → E1004;
      permission-denied → graceful Result not crash (per §9's Android block + iOS table row 4).
- [ ] Adversarial — **uncross-built rt → E1711: NOT verified and NOT implemented as specced**
      (corrected 2026-07-31, second pass — the same-day first pass wrongly folded this clause into
      the ticked item above, citing evidence that does not contain it: §9's Android adversarial list
      is "E1710, iOS-on-Linux refusal, unknown `--host`, missing Android target" — no E1711).
      Refutation: no test anywhere references E1711; its only emission site is the **iOS
      object-EMISSION failure** path (`main.rs:2394`, whose own comment admits "E1711 stands in for
      the cross-built-rt gate"; `error.rs:166` labels it "iOS-specific staging code"); and on the
      Android path a failed axon-rt/axon-ai cross-build returns `None` from
      `build_crate_staticlib` (its cargo stderr is nulled) and the lib is **silently omitted** from
      the link args (`android_link`'s `if let Some(ref lib)` has no else). Fix: emit
      `[E1711] '<crate>' not available for '<triple>'; cross-build it first` when
      `build_crate_staticlib(..., Some(triple))` returns `None` on the Android path, plus a test.
- [ ] **Adversarial — wrapper identifier injection (E1713): NOT implemented, NOT tested** (added
      2026-07-31, ASI-trajectory pass; refutes §7's old I-12 claim). Required cases, each asserting the
      build **FAILS** rather than emitting a wrapper: a source-file stem containing `"` + `}` + `;` that
      closes the Kotlin object body (e.g. `a") } ; init {`), a stem containing a newline, a stem that is a
      non-identifier Unicode string, and a stem exceeding 64 chars. Also golden-test the wrapper across a
      **matrix** of entry configurations rather than the single benign name currently pinned.
- [ ] **Adversarial — entry signature validation (E1714): NOT implemented, NOT tested** (added 2026-07-31):
      `@[entry] fn boot(cfg: str) -> Point` under `--host mobile` must be a hard build-time error, not a
      wrapper declaring a nonexistent ABI. Plus: register `frame`/`resume` as known attributes (verified
      2026-07-31 — only `entry` is in `builtins.rs:2345`'s list, so `@[frame]`/`@[resume]` warn W0001 while
      nonetheless steering the wrapper), and resolve the `@[entry]` name collision with R17's bare-metal
      entry attribute (same `builtins.rs` entry, documented there as "the bare-metal entry point (replaces
      host main/_start)") — either rename R14's marker `@[mobile_entry]` or document the collision in §3.
- [ ] **Adversarial — risk derivation over `platform`/`gfx`: NOT implemented, NOT tested** (added
      2026-07-31): assert that §3's **own example program** derives Risk ≥ High and triggers the full
      simulate→stress→redteam→verify chain. It currently derives Low and skips all of it (§7, Phase-11 row).
- [ ] **Adversarial — exported-symbol surface: NOT implemented, NOT tested** (added 2026-07-31): assert the
      `.so`'s exported dynamic symbol set **equals** the resolved `MobileEntries` set (§9 KNOWN HOLE 2).
- [ ] **Adversarial — approval binding: NOT implemented, NOT tested** (added 2026-07-31): `--host mobile`
      on a source with no `<file>.ax.approved` record must refuse (or require a loud `--unapproved`), and the
      emitted artifact must carry the approved-AST hash (§11 slice 7c / Q8).
- [ ] Journey (UI): the window-clear journey built for iOS simulator + Android emulator renders one cleared
      frame via Metal/Vulkan. Manual tier (needs simulators). (Corrected 2026-07-31: previously cited
      `examples/gfx/window_clear.ax`, which does not exist. The real artifacts are
      `examples/mobile/ios_window_clear.ax` (headless mock, iOS) plus the native gfx examples
      `examples/native/gfx_demo.ax` / `gfx_render.ax`; the **Android** window-clear example does not exist
      yet and must be authored as part of slice 5 — it is part of the `android_emu_window_clear` gate.)

### 9. Acceptance criteria (the done gate)

**Android landed (gate-verified on this Linux host with NDK 27 + a KVM x86_64 emulator), 2026-06-25:**

- [x] Test `mobile_wrapper_is_byte_stable` passes (golden wrapper, deterministic generator) —
      `mobile::tests::wrapper_is_byte_stable_golden` (asserts byte-stable Kotlin output + ≤80 lines).
- [x] axon-rt **and** axon-ai cross-build for `aarch64-linux-android` + `x86_64-linux-android` via the NDK
      (`build_crate_staticlib(..., Some(triple))`, the NDK linker/ar/CC auto-wired); E1710 guards the
      toolchain-absent case. (Corrected 2026-07-31, second pass: this previously also claimed E1711
      guards the un-cross-built case — false; on the Android path a failed cross-build is silently
      dropped from the link, see §6/§8. iOS cross-build is gated on a macOS host, §4/Q4 — NOT done.)
- [x] **`android_compute_parity` passes** (the `android_so_compute_parity` acceptance gate): the pure-compute
      example set, cross-compiled to Android x86_64 **and** AArch64, runs on the device and returns the
      interpreter-oracle value byte-for-byte (stdout + exit). **Execution path:** x86_64 runs NATIVELY on the
      KVM-accelerated emulator (`adb push`+`adb shell`); AArch64 runs on the SAME emulator via its
      `libndk_translation` arm64 native bridge (qemu-user is a fallback). 12 runs (6 examples × 2 arches) all
      identical. `scripts/android_compute_parity.sh` + the `android_compute_parity_r14` integration test
      (self-skips when NDK/emulator absent so the default gate stays safe). (Scope note, 2026-07-31,
      second pass: the self-skip is honest and the harness asserts ran>0 when it runs, but **no
      automated environment exercises this gate** — Linux CI (`ci.yml`) has no NDK/emulator, so
      after the 2026-06-25 verification (re-run 2026-07-18) these gates are green-by-skip everywhere
      except manual runs on the NDK host. They are manual-tier regression checks with a last-run
      date, not standing regression protection; an NDK-only CI tier (cross-build + ELF-verify +
      qemu-user, no emulator needed) would close this. **Rate note, 2026-07-31 (ASI-trajectory pass):** a
      manual-tier gate with a last-run date is an acceptance path whose throughput does not scale with
      build rate, and a self-skipping gate is indistinguishable from a passing gate to the R10 4-gate
      firewall's G3 test-preservation check — so a self-improving pass sees no cost in touching mobile code
      paths. Beyond the NDK-only CI tier, `--host mobile` should **refuse to emit an artifact when the
      recorded last-verification timestamp for the target's parity gate predates the source hash** — making
      manual-tier staleness a build-time fact rather than a doc-comment date, which is the only form of it
      that survives an increase in build rate. See §12 Q9.)
- [x] `axon build --host mobile --target <android> app.ax` emits a **loadable `.so`** into
      `out/android/jniLibs/<abi>/lib<name>.so` + the generated `out/android/Axon.kt` wrapper. The `.so` is
      ELF-verified for the right arch and exports the Axon entry symbol (`main`); `kotlinc` compilation of the
      wrapper is the manual tier (the wrapper is golden-tested for byte-stability instead). CLI e2e adversarial
      cases verified: E1710 (no NDK), iOS-on-Linux refusal, unknown `--host`, missing Android target.
      **KNOWN HOLE (found 2026-07-31, second pass — open):** the shared-object link args are
      `-shared -Wl,--export-dynamic` with NO `-Wl,--no-undefined` (`android_link`), and a failed
      rt/ai cross-build is silently skipped (previous item) — so on a host with the NDK but without
      the `aarch64-linux-android` rust target, the `.so` **links green with undefined `__axon_*`
      symbols** and fails only at `System.loadLibrary`/`dlopen` on the device, far from the build.
      The in-code comment "the link fails with a clear undefined symbol __axon_*" is true only for
      the executable path, not this artifact. Fix: add `-Wl,--no-undefined` to the shared link (or
      hard-error on a `None` staticlib — the natural E1711 site) + an adversarial test that a
      poisoned cross-build FAILS the build instead of emitting a `.so`.
      **KNOWN HOLE 2 (found 2026-07-31, ASI-trajectory pass — open):** the link args are
      `-shared -Wl,--export-dynamic` with **no symbol filtering** (`link.rs:773–778`), so **every** Axon fn
      in the program plus the whole `__axon_*` runtime surface is a dynamically callable entry point from
      the JVM side or from any other library loaded into the same process. The reviewed three-entry
      `AxonEntries` contract (§3/§4) is therefore **advisory, not enforced**: a helper fn that is dead code
      in the approved AST's call graph is still directly invocable on device, and reachability arguments over
      the three reviewed entries do not constrain the artifact. Both holes stem from the link args never
      having been specified as a *capability surface*, only as a mechanism. Fix: restrict exports to exactly
      the resolved `MobileEntries` (a generated `-Wl,--version-script`, or `--exclude-libs ALL` plus an
      explicit `-Wl,--export-dynamic-symbol=<entry>` per entry) and extend the ELF verification — which
      already checks arch and entry-symbol presence — to assert the exported dynamic symbol set **equals**
      the entry set. Land `-Wl,--no-undefined` in the same change.
- [x] Generated wrapper is **≤ 80 lines** (asserted in the golden test). (Scope note, 2026-07-31: this
      criterion is **near-vacuous as a safety property**. It is a proxy for human reviewability, but the
      wrapper is byte-stable boilerplate nobody reads per-build; the part that actually varies per build —
      the app/lib name and the resolved entry names — is the part **no golden pins**, and is exactly the
      injection surface of §7 I-12. Replace it with a load-bearing criterion: assert the generated wrapper's
      declared bindings match the program's resolved entry signatures (E1714), over a matrix of entry
      configurations.)
- [ ] (Stretch / **manual tier**) `ios_sim_window_clear` and `android_emu_window_clear` render one frame —
      NOT run; the GPU/Vulkan surface bridge (slice 5) and a full NativeActivity GUI app are the manual tier.

**iOS status legend:** ✅ authored + verified on Linux · 🟡 authored on Linux, **verification REMOTE-CI-ONLY**
(macOS job `.github/workflows/ios.yml`; CANNOT be verified on this Linux/WSL2 host — §12 Q4) · ⬜ not yet built.

> **🟡 verifier status (2026-07-31, second pass): `ios.yml` is currently RED and structurally
> guaranteed red.** It has run exactly once ever (run 28248266522, 2026-06-26, on main): FAILURE,
> exit 127 at the "Run compute parity on the iOS Simulator" step. Cause is in the tree: the step
> guards the harness with `[ -d tools/ios-parity-harness ]`, but that directory IS committed
> (README.md only), so the guard always takes the run branch and `bash run_on_sim.sh` — which does
> not exist — exits 127. The guard must be script-presence (`[ -f .../run_on_sim.sh ]`, the
> README's own contract), then the workflow re-dispatched for a green baseline. Within that sole
> red run the iOS cross-build, Mach-O check, wrapper generation, and interpreter-oracle steps DID
> individually pass — so "authored + cross-built once" has evidence — but an expected-red workflow
> that has not run in a month cannot serve as a standing verifier for any 🟡 item (a new regression
> in the green steps is indistinguishable from the known failure). Read every 🟡 below with that
> caveat.

- [x] ✅ Test `mobile_wrapper_is_byte_stable` passes (golden Swift+Kotlin wrapper, deterministic generator) —
      `mobile::tests::swift_shim_is_byte_stable` / `kotlin_shim_is_byte_stable` (Linux gate-green).
- [x] 🟡 `axon_rt_cross_builds_for_ios` — the iOS-sim + iOS-device `cargo build -p axon-rt --features ios
      --target aarch64-apple-ios{,-sim}` steps in `ios.yml`. The cross-platform `AxonRuntime` bridge CORE +
      its tests are Linux-gate-green (`axon-rt --features ios`); the actual iOS-target cross-build + the
      `axon_ios_*` symbol-export check run REMOTELY on macOS (NOT verifiable on Linux). (Android
      counterpart: LANDED — see the Android block above; marker corrected 2026-07-31, it previously read
      "⬜" here as a per-pass note and contradicted the Android-landed block.)
- (`android_so_compute_parity`: LANDED — see the Android block above, the `android_compute_parity` [x]
  item. Corrected 2026-07-31: this line previously carried a "⬜" per-iOS-pass marker that contradicted
  the authoritative Android status; the Android block is the single source of truth for Android gates.)
- [x] ✅/🟡 `axon target build --host mobile` emits the byte-stable wrapper + (codegen) the device object;
      Linux-verified for wrapper emission + the E1710 clean refusal on the iOS link (no Apple toolchain). The
      `swiftc`-compiles assertion runs REMOTELY on macOS (`ios.yml`).
- [x] ✅ Generated wrapper is **≤ ~80 lines** each — `mobile::tests::shims_are_under_80_lines` (Linux-green).
- [ ] 🟡 (Stretch / manual tier) `ios_sim_window_clear` renders one frame — the headless-mock journey runs on
      the Linux gate (`examples/mobile/ios_window_clear.ax`); the **real Metal-backed** simulator render is the
      macOS-host-authored remainder (`tools/ios-parity-harness/`, REMOTE-CI-ONLY). (`android_emu_window_clear`
      is tracked in the Android block above — still ⬜ there, slice 5.)
- [x] 🟡 iOS compute-parity oracle authored (`examples/mobile/ios_compute_parity.ax`, oracle output pinned in
      `ios.yml` as `$EXPECTED`); the interpreter oracle is Linux-green, the iOS-Simulator run is REMOTE-CI-ONLY.

### 10. Performance budget

Binary size mirrors the PRD's ~3.5 MB GPU-stack figure (`AI_Language_Plan.md:288`); the non-UI lib should be
well under that. The compute parity tests double as a perf guard (AArch64 codegen should be within the same
order as x86-64 native; no mobile-specific hot path). Wrapper generation is build-time, negligible.

### 11. Rollout & rollback

- Behind `--host mobile`; iOS additionally behind axon-rt's `--features ios`. (Corrected 2026-07-31:
  previously also claimed a `--features android` — no such feature exists anywhere; the landed Android
  slices are featureless, gated purely by `--host`/`--target`.) Inert otherwise;
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
     The **headless stub is LANDED** (corrected 2026-07-31; this bullet previously called it "the next
     increment"): `native.rs` registers `native::platform` with `request_permission`→DENIED,
     `haptic`→no-op, `is_device`→0, gated by the `platform_registered` test (matches the iOS table row 4,
     "Linux-green"). The actual remaining increment is the **on-device implementation**: JNI upcalls into
     the Kotlin wrapper (Android) / UIKit-backed calls (iOS). Deferred.
  5. **gfx Metal/Vulkan surface bridge** — wgpu surface from the platform view; the UI journey tier. Deferred.
  6. **[SCOPED 2026-07-31, not started] Differential parity fuzz for the device triples** — parameterize
     `scripts/fuzz_parity.sh` (the landed R1f differential fuzzer, ~51 builtins; currently host-only, it
     hardcodes `AXON="target/debug/axon"` at line 53 with no triple or run-under parameterization) with a
     target triple and a run-under adapter (`adb shell` / `qemu-aarch64`), and make the Android parity leg a
     **bounded fuzz run against the interpreter oracle** rather than a fixed 6-example list; keep the six
     examples as the fast smoke tier. This is what upgrades I-2's mobile claim from "verified over a curated
     corpus" toward a claim that scales with generation rate, and it also gives the NDK-only CI tier (§9)
     something worth running. Revertible; no behaviour change to the compiler.
  7. **[SCOPED 2026-07-31, not started] Close the mobile safety seam** — three independent sub-slices, each
     small, each reusing landed machinery, ordered by severity:
     - **7a. Wrapper identifier validation (E1713).** Validate the app/lib name — and, once `@[entry]`/
       `@[frame]`/`@[resume]` can name arbitrary fns, the entry names — against
       `^[A-Za-z_][A-Za-z0-9_]{0,63}$` in `mobile.rs` **before any interpolation**, and hard-error rather
       than silently sanitize (a rewritten lib name would silently break `System.loadLibrary`). Add the
       hostile-stem adversarial goldens from §8. **This is the prerequisite for §7's I-12 row being true at
       all** — until it lands, R14 claims an invariant it does not hold. Ships with E1714 (entry-signature
       validation) and the `frame`/`resume` attribute registration.
     - **7b. Make mobile visible to the risk gate.** Give `native::platform` per-fn effect rows
       (`Device`/`Sensor`/`Biometric`) instead of the blanket `IO` (`native.rs:545`), and add `platform`/`gfx`
       grant keys to `derive_risk_from_ast` (`main.rs:5236–5283`) with device-permission ⇒ **High** minimum
       and biometrics/camera ⇒ **Critical**. Then §3's own example triggers the full gate chain instead of
       skipping it. Pairs naturally with the structured sub-capability grant (Q6), which supplies the
       resolution the risk mapping needs.
     - **7c. Bind the shipped artifact to the approved AST.** (a) Require an `.approved` record for
       `--host mobile` — a **hard** gate, not `cmd_deploy`'s warning, because this artifact is *distributed*
       rather than run locally (loud `--unapproved` opt-out at most). (b) Emit `.axmeta` unconditionally for
       mobile and embed the approved-AST hash **into the artifact itself** — a `.note.axon` ELF note on the
       `.so`, a `__AXON,__meta` Mach-O section on the `.a` — so an on-device attestation (R31) or a recovered
       binary can be tied back to the reviewed AST. (c) Write one R28 ledger record per mobile build (source
       hash, approved hash, triple, granted capabilities, derived risk). All three reuse landed machinery.
- Blast radius: confined to mobile builds. `git revert` of any slice leaves a clean tree. Slices 1–2 (headless
  mobile compute) are independently useful even if 4–5 never land.

#### iOS-slice authoring status (this pass — Linux-authored, macOS-CI-verified)

The **iOS** counterparts of slices 1–2 (+ the slice-3 bridge core, the slice-4 `platform` headless stub, and
the slice-5 journey shape) are **AUTHORED on this Linux host and `cfg`/feature-gated so the Linux `gate.sh`
stays green**, but their iOS build/run is **VERIFIED ONLY by the remote macOS CI job** — it CANNOT be run on
this Linux/WSL2 host (Apple SDK/Xcode/Simulator are macOS-only; §12 Q4). Status per slice:

| Slice (iOS) | What landed (Linux-authored) | Verification |
|---|---|---|
| 1 (toolchain) | iOS triple recognition + E1710 toolchain probe (`mobile.rs`); `--host mobile` routes iOS → wrapper + clean E1710 on Linux; `emit_object_for_triple` LLVM cross-emits the iOS object | Linux-green for recognition/probe/refusal; the real iOS-target `axon-rt` cross-build is `ios.yml` (REMOTE) |
| 2 (C-ABI export + shim + parity) | byte-stable Swift `@main` shim generator (`gen_swift_shim`, golden test); compute-parity oracle (`examples/mobile/ios_compute_parity.ax`) | wrapper + oracle Linux-green; iOS-Simulator parity run is `ios.yml` (REMOTE) |
| 3 (`AxonRuntime` bridge) | the lifecycle state-machine CORE + the `axon_ios_*` `#[no_mangle] extern "C"` entry points (`axon-rt/src/mobile_ios.rs`; core compiles on any host, FFI `cfg(target_os="ios")`) — built on R15 resume | core + transition tests Linux-green under `--features ios`; the iOS-linked symbols verified by `ios.yml` (REMOTE) |
| 4 (`native::platform`) | the **headless stub** `native::platform` module (`request_permission`→denied, `haptic`→no-op, `is_device`→0); E1004 capability gate | Linux-green (interp run + E1004); the UIKit-backed device impl is the macOS remainder |
| 5 (gfx Metal bridge) | the `ios_window_clear` journey against the GPU-free mock | headless mock Linux-green; the real Metal `CAMetalLayer` render is `tools/ios-parity-harness/` (REMOTE, macOS-authored) |

**macOS-host remainder (NOT authored here — needs Xcode to author exactly):** the `AxonRuntime` Swift support
library, the SwiftPM/`.xcodeproj` simulator harness, and `run_on_sim.sh` (scaffolded + contract-documented in
`tools/ios-parity-harness/README.md`). (Corrected 2026-07-31, second pass: this paragraph previously
claimed "The CI workflow degrades gracefully (a `::warning::`) until they exist" — **disproven by
the workflow's only real run** (2026-06-26, exit 127). `ios.yml` guards on directory presence
(`[ -d tools/ios-parity-harness ]`) while the README documents script-presence semantics; the
directory is committed, `run_on_sim.sh` is not, so the graceful branch is unreachable and the run
fails. See the §9 legend note; the guard needs `[ -f .../run_on_sim.sh ]`.)

### 12. Open questions

- **Q1 (the decisive fork): does mobile need a new codegen backend, or just toolchain + wrapper?** Resolved:
  **toolchain + wrapper.** R1 already emits AArch64 via LLVM — the device ISA — and `emit_object_and_link`
  already accepts an arbitrary triple (`codegen/link.rs:252`) with the cross-linker resolved from `cross.toml`
  (`read_cross_linker`, `codegen/link.rs:608`), so the codegen+link *plumbing* is in place; mobile adds **no codegen path**. What it adds is
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
  (Amended 2026-07-31, ASI-trajectory pass: this deferral was stated without noticing what it gives up.
  Signing/packaging is exactly where an approved-AST↔artifact binding would naturally be anchored, and
  deferring it wholesale is why there is currently no path from a `.so` recovered off a phone back to the AST
  a human approved. The *toolchain-side* half — `.axmeta` + an embedded approved-AST hash + an R28 ledger
  record — is **in scope for v1** and does not require `xcodebuild`/`gradle`; see §11 slice 7c and Q8. What
  remains legitimately deferred is platform code-signing itself.)
- **Q6 (structured `platform` sub-capabilities — OPEN, added 2026-07-31).** R13 promised
  "the grant is **structured, not `any`** … `@[contained(gfx: [present, clear])]`", and `platform` shipped
  with a single flat key and a runtime-`str` permission argument instead (§7 I-11). The proposed shape is in
  §3: enumerate the permission names as declared sub-capabilities, require the grant to name them
  (`@[contained(platform: [permission("Camera"), haptic])]`), require the request argument to be a **string
  literal** (checker `const_eval`), and E1004/E1209 anything outside the grant. Open questions this leaves:
  (a) is the permission enumeration a **closed** compiler-side list (auditable, but it must track two moving
  OS vendor surfaces) or an open string set validated only for literal-ness (extensible, weaker)? (b) does a
  `pending`/callback-resolved permission (Q3) need a distinct sub-capability from the initial request? (c)
  does `gfx` need the same treatment at the same time, or can it follow? **Not answering these here** — the
  choice binds R13's capability model, not just R14, and should be resolved with R13.
- **Q7 (`platform::is_device()` — should an eval-vs-deployment discriminator exist at all? OPEN, added
  2026-07-31).** `is_device()` (`native.rs:566`) is a shipped, documented predicate that is false in **every**
  audited path (`axon ast review`, the interpreter oracle, `redteam_check`, the headless journey mock, every
  `@[test]`) and true only on device. §7's I-2 parity claim is proven over the pure-compute corpus, which by
  construction never uses it — parity is asserted at exactly the scope where the discriminator is absent.
  Three candidate resolutions, none adopted here: (a) **remove it from the surface** — the I-9 design already
  makes headless-vs-device observable as a graceful `Result` at each call site, so an explicit boolean is
  redundant capability; (b) **keep it and taint it** — treat it as a taint source in the landed R25
  information-flow monitor / `Tainted` lattice, and make branching on it inside a `@[contained]`-granted fn a
  diagnostic, so a device-divergent branch must be *declared* and shows up in the approved AST; (c) keep it
  unchanged and rely on review. Independently of the choice, a cheap partial mitigation stands on its own:
  **record in the deploy/audit record which fns were never entered during the headless oracle run** — a
  branch hidden behind `is_device` is by construction one with zero headless coverage.
- **Q8 (what binds a shipped mobile artifact to the AST a human approved? OPEN, added 2026-07-31).** §11
  slice 7c proposes the mechanism (hard `.approved` gate + embedded approved-AST hash + R28 ledger record).
  What is genuinely open: (a) is the hash embedded as an ELF note / Mach-O section (survives `strip`? survives
  `gradle` repackaging?) or carried out-of-band in `.axmeta` and reconciled at attestation time (R31)? (b)
  does the binding cover the **wrapper** too, which is TCB and separately compiled by `kotlinc`/`swiftc`
  outside Axon's control? (c) what is the intended failure mode on device when the attested hash does not
  match — refuse to load, or report? Deliberately unanswered: this is the R28/R31 interface, and R14 should
  adopt whatever those specs settle rather than inventing a mobile-local scheme.
- **Q9 (what replaces the manual tier as build rate rises? OPEN, added 2026-07-31).** §9's Android
  compute-parity gate, the `kotlinc` wrapper compile, and both window-clear journeys are human-executed. The
  NDK-only CI tier (§9) plus the parity fuzz (§11 slice 6) close most of the *compute* leg automatically, but
  the **GUI/lifecycle** leg has no proposed automated substitute — a real emulator with a display is expensive
  and flaky in CI. Open: is the intended policy (a) an emulator-backed CI tier accepted as slow/flaky, (b) a
  build-time staleness refusal (§9's rate note) that lets manual verification remain the gate but makes its
  age a hard fact, or (c) an explicit ceiling on how much of R14 may ship without a human in the loop —
  i.e. §7a's stated limit written down as policy rather than as a caveat? No answer invented here; note that
  (b) is cheap and compatible with either of the others.
