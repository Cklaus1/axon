//! R14 — Mobile targets (iOS / Android): the **host-agnostic** toolchain +
//! wrapper-generator layer.
//!
//! This module is the cross-platform CORE of `--host mobile` (spec §3/§4). It is
//! **pure logic** — triple recognition, the deterministic Swift/Kotlin wrapper
//! generator, the toolchain-presence probes behind E1710/E1711/E1712, and the
//! Android `jniLibs/` packaging layout — so it compiles and is fully unit-tested
//! on EVERY host, including this repo's Linux/WSL2 box. The *iOS-specific* parts
//! that genuinely cannot exist on Linux (the Mach-O link, the
//! `#[no_mangle] extern "C"` lifecycle entry points the Swift app calls, the
//! Metal surface bridge) live behind `cfg(target_os="ios")` or an off-by-default
//! `ios` feature in `axon-rt`, NOT here. So the Linux gate stays green: it
//! compiles this generator + its tests, and the iOS link is exercised only by
//! the macOS CI job (`.github/workflows/ios.yml`).
//!
//! Per spec §3/§4/Q2 the wrapper is **two parts**:
//!   * a hand-written, versioned `AxonRuntime` lifecycle bridge (Kotlin/Swift,
//!     in the TCB) — NOT generated here; it lives in the platform support lib;
//!   * a tiny GENERATED per-app `@main` shim (~10 lines) that binds this app's
//!     exported C-ABI entry names (`on_start`/`on_frame`/`on_resume`) into the
//!     bridge.
//!
//! This module owns the generated shim. The generators are **deterministic**
//! (pure functions of the entries + the lib name), so the golden tests can
//! assert byte-stability — and a self-improving pass cannot inject native code
//! into them (I-12: a fixed template, not AI-authored).
//!
//! ## Two generator APIs (the R14 Android + iOS slices merged)
//!
//! Two parallel surfaces coexist here, both deterministic, dispatched by triple
//! at the call sites in `main.rs`:
//!   * The **triple-dispatched** `gen_swift_shim` / `gen_kotlin_shim` pair (over
//!     the simple [`Entries`] record) backs `axon target build --host mobile`,
//!     selecting Swift for an `*-apple-ios` triple and Kotlin for an
//!     `*-linux-android` triple.
//!   * The **`Option`-aware** [`MobileEntries`] + [`generate_kotlin_wrapper`]
//!     path (which resolves entries from the program's `@[entry]`/`@[frame]`/
//!     `@[resume]` attrs and emits `null` lambdas for absent entries) backs the
//!     Android `.so` packaging path in `axon build --host mobile`, alongside the
//!     `jniLibs/<abi>/` layout from [`android_jni_abi`].
//!
//! ## What is and isn't buildable here (spec §4 / §12 Q4 — be honest)
//!
//! | Layer | Buildable on Linux? |
//! |---|---|
//! | triple recognition + wrapper generation (THIS module) | yes — pure logic |
//! | LLVM object emission for `aarch64-apple-ios` | yes — LLVM cross-emits (`link.rs`) |
//! | the Android `.so` link + jniLibs packaging | yes — the NDK linker is Linux-buildable |
//! | the iOS *link* into `.a`/`.xcframework` | **NO** — needs the Apple `ld` + iOS SDK (macOS-only) |
//! | booting an iOS Simulator + running the parity oracle | **NO** — needs Xcode + macOS |
//!
//! The bottom two rows are the macOS-host wall (§12 Q4). They are verified by the
//! macOS CI workflow, never on Linux. A Linux `axon target build --target
//! aarch64-apple-ios` reaches "object emits for the triple" and then E1710s on
//! the missing Apple toolchain — a CLEAN refusal, never a fake success (I-9).

use crate::ast::{Item, Program};

// ── Triple recognition (shared apple-ios + linux-android core) ────────────────

/// A recognised mobile target family (spec §4 / §12 Q4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MobileOs {
    /// Apple iOS — Mach-O; the link needs the Apple SDK + Xcode `ld` (macOS-only).
    Ios,
    /// Google Android — ELF `.so`; the link needs the NDK linker (Linux-buildable).
    Android,
}

/// One recognised mobile triple and its OS family + the wrapper language.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MobileTarget {
    pub triple: &'static str,
    pub os: MobileOs,
    /// `true` for the simulator/emulator triple (x86_64 / arm64-sim) — the parity
    /// oracle runs here in CI; `false` for a physical-device triple.
    pub simulator: bool,
}

/// The mobile triples v1 recognises (spec §4 input table / §12 Q4).
///
/// iOS device + iOS simulator (both arm64 and x86_64-sim, since CI may run on an
/// Intel or Apple-silicon macOS runner) + the two Android ABIs. The simulator
/// triples are the ones the compute-parity oracle actually runs under in CI.
pub const MOBILE_TARGETS: &[MobileTarget] = &[
    MobileTarget {
        triple: "aarch64-apple-ios",
        os: MobileOs::Ios,
        simulator: false,
    },
    MobileTarget {
        triple: "aarch64-apple-ios-sim",
        os: MobileOs::Ios,
        simulator: true,
    },
    MobileTarget {
        triple: "x86_64-apple-ios",
        os: MobileOs::Ios,
        simulator: true,
    },
    MobileTarget {
        triple: "aarch64-linux-android",
        os: MobileOs::Android,
        simulator: false,
    },
    MobileTarget {
        triple: "x86_64-linux-android",
        os: MobileOs::Android,
        simulator: true,
    },
];

/// Look up a recognised mobile target by exact triple.
pub fn mobile_target(triple: &str) -> Option<&'static MobileTarget> {
    MOBILE_TARGETS.iter().find(|t| t.triple == triple)
}

/// Is `triple` a recognised mobile triple (any OS)?
pub fn is_mobile_triple(triple: &str) -> bool {
    mobile_target(triple).is_some()
}

/// Is `triple` an Apple-iOS triple? (Used to gate the macOS-host wall: an iOS
/// build can produce the object on Linux but must E1710 at the link step.)
pub fn is_ios_triple(triple: &str) -> bool {
    matches!(mobile_target(triple), Some(t) if t.os == MobileOs::Ios)
}

// ── E1710 — toolchain presence probe (spec §6, shared android+ios) ────────────
//
// `--target <mobile-triple>` with the SDK/NDK toolchain not on PATH → E1710. For
// iOS the toolchain is `xcrun` (the Xcode SDK indirection); for Android it's the
// NDK linker. The probe is a `which`-style PATH check — it does NOT attempt the
// build, so it gives an actionable up-front error rather than a cryptic link
// failure. On Linux, `xcrun` is ALWAYS absent (it is macOS-only), so an iOS build
// here always reaches this clean refusal.

/// The toolchain command an OS needs on PATH, and a one-line hint for E1710.
pub fn toolchain_requirement(os: MobileOs) -> (&'static str, &'static str) {
    match os {
        MobileOs::Ios => (
            "xcrun",
            "Xcode + the iOS SDK (macOS only); set up a macOS build host — \
             iOS cannot be linked on Linux",
        ),
        MobileOs::Android => (
            "ANDROID_NDK_HOME",
            "install the Android NDK and configure the linker in \
             ~/.config/axon/cross.toml under [target.<triple>]",
        ),
    }
}

/// Probe for the OS toolchain. Returns `Ok(())` if present, else the E1710
/// message body (the caller prefixes the code). `xcrun` is probed on PATH;
/// the NDK is probed via the `ANDROID_NDK_HOME` env var OR a cross.toml linker.
///
/// `path_lookup` and `env_lookup` are injected so the probe is unit-testable
/// without touching the real host — the production callers pass the real
/// `which::which` / `std::env::var_os` closures.
pub fn probe_toolchain<P, E>(triple: &str, path_lookup: P, env_lookup: E) -> Result<(), String>
where
    P: Fn(&str) -> bool,
    E: Fn(&str) -> bool,
{
    let target = match mobile_target(triple) {
        Some(t) => t,
        None => {
            return Err(format!(
                "'{triple}' is not a recognised mobile target triple (known: {})",
                MOBILE_TARGETS
                    .iter()
                    .map(|t| t.triple)
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        }
    };
    let (cmd, hint) = toolchain_requirement(target.os);
    let present = match target.os {
        // iOS needs `xcrun` on PATH (macOS only).
        MobileOs::Ios => path_lookup(cmd),
        // Android is satisfied by EITHER ANDROID_NDK_HOME or a cross.toml linker
        // (the env probe stands in for both here; the cross.toml path is checked
        // separately at link time).
        MobileOs::Android => env_lookup(cmd),
    };
    if present {
        Ok(())
    } else {
        let sdk = match target.os {
            MobileOs::Ios => "Xcode SDK",
            MobileOs::Android => "Android NDK",
        };
        Err(format!(
            "mobile target '{triple}' requires {sdk}; '{cmd}' not found on PATH ({hint})"
        ))
    }
}

// ── jniLibs/ packaging (Android) ──────────────────────────────────────────────

/// Map an Android LLVM triple to its `jniLibs/<abi>/` directory name.
///
/// Returns `None` for a non-Android or unrecognized triple.
pub fn android_jni_abi(triple: &str) -> Option<&'static str> {
    if !triple.contains("-android") {
        return None;
    }
    if triple.starts_with("aarch64") {
        Some("arm64-v8a")
    } else if triple.starts_with("x86_64") {
        Some("x86_64")
    } else if triple.starts_with("armv7") || triple.starts_with("arm-") {
        Some("armeabi-v7a")
    } else if triple.starts_with("i686") {
        Some("x86")
    } else {
        None
    }
}

// ── The triple-dispatched shim generators (spec §8 unit / §9 acceptance) ──────
//
// Spec §4: the wrapper = a hand-written `AxonRuntime` BRIDGE (in the TCB, ships
// in the platform's support library) + a TINY generated per-app `@main` shim. We
// generate ONLY the shim — the byte-stable, deterministic part. The bridge is
// authored once, not generated each build (§12 Q2). The shim binds the app's
// exported C-ABI entry names into the bridge; it is ≤ ~80 lines (§9).

/// The exported C-ABI entry names a mobile app provides (spec §3 `AxonEntries`).
/// Each is an `extern "C"` symbol the bridge calls at the matching lifecycle
/// point. `start`/`frame`/`resume` are the v1 trio; the names are the app's own
/// fns (default `on_start`/`on_frame`/`on_resume`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entries {
    pub start: String,
    pub frame: String,
    pub resume: String,
}

impl Default for Entries {
    fn default() -> Self {
        Entries {
            start: "on_start".to_string(),
            frame: "on_frame".to_string(),
            resume: "on_resume".to_string(),
        }
    }
}

/// Generate the **byte-stable** iOS Swift `@main` shim for `app` with the given
/// entry names (spec §8 golden test / §9 `mobile_wrapper_is_byte_stable`).
///
/// This is the ~10-line generated half (§4): it imports the hand-written
/// `AxonRuntime` support library and declares the per-app entry bindings. The OS
/// run loop is owned by `AxonRuntime` (which conforms to `UIApplicationDelegate`
/// and drives `host_await`-based suspend/resume across lifecycle events), NOT by
/// this shim. Deterministic in `(app, entries)` → a golden test pins it.
pub fn gen_swift_shim(app: &str, entries: &Entries) -> String {
    format!(
        "// GENERATED by `axon target build --host mobile` — DO NOT EDIT.\n\
         // The per-app @main shim (spec R14 §4). The run loop + lifecycle bridge\n\
         // live in the hand-written, versioned `AxonRuntime` support library (TCB);\n\
         // this shim only binds the app's exported C-ABI entry names into it.\n\
         import AxonRuntime\n\
         \n\
         // The Axon static lib (`lib{app}.a`) exports these C-ABI symbols.\n\
         @_silgen_name(\"{start}\") func axon_start(_ ctx: UnsafeMutableRawPointer?) -> Int64\n\
         @_silgen_name(\"{frame}\") func axon_frame(_ ctx: UnsafeMutableRawPointer?) -> Int64\n\
         @_silgen_name(\"{resume}\") func axon_resume(_ ctx: UnsafeMutableRawPointer?) -> Int64\n\
         \n\
         @main\n\
         struct {app}App: AxonApp {{\n\
         \u{20}\u{20}// AxonApp = the bridge; conforms to UIApplicationDelegate under the hood.\n\
         \u{20}\u{20}static let entries = AxonEntries(\n\
         \u{20}\u{20}\u{20}\u{20}start: axon_start,\n\
         \u{20}\u{20}\u{20}\u{20}frame: axon_frame,\n\
         \u{20}\u{20}\u{20}\u{20}resume: axon_resume\n\
         \u{20}\u{20})\n\
         }}\n",
        app = app,
        start = entries.start,
        frame = entries.frame,
        resume = entries.resume,
    )
}

/// Generate the byte-stable Android Kotlin `@main` shim (the Android counterpart;
/// included so the generator covers both OSes from the one host-agnostic core).
pub fn gen_kotlin_shim(app: &str, entries: &Entries) -> String {
    format!(
        "// GENERATED by `axon target build --host mobile` — DO NOT EDIT.\n\
         // The per-app shim (spec R14 §4). The Activity lifecycle bridge lives in\n\
         // the hand-written, versioned AxonRuntime support library (TCB); this shim\n\
         // only loads the .so and binds the exported C-ABI entry names into it.\n\
         package app.axon.{app}\n\
         \n\
         import com.axon.runtime.AxonActivity\n\
         import com.axon.runtime.AxonEntries\n\
         \n\
         class {app}Activity : AxonActivity() {{\n\
         \u{20}\u{20}companion object {{ init {{ System.loadLibrary(\"{app}\") }} }}\n\
         \u{20}\u{20}external fun {start}(ctx: Long): Long\n\
         \u{20}\u{20}external fun {frame}(ctx: Long): Long\n\
         \u{20}\u{20}external fun {resume}(ctx: Long): Long\n\
         \u{20}\u{20}override val entries = AxonEntries(::{start}, ::{frame}, ::{resume})\n\
         }}\n",
        app = app,
        start = entries.start,
        frame = entries.frame,
        resume = entries.resume,
    )
}

/// The line count of a generated shim — used by the §9 acceptance check that the
/// wrapper is ≤ ~80 lines.
pub fn shim_line_count(shim: &str) -> usize {
    shim.lines().count()
}

// ── The Option-aware Android wrapper (resolved from program attrs) ────────────
//
// This is the Android `.so` packaging path's generator (`axon build --host
// mobile`): it resolves the lifecycle entries from the program's
// `@[entry]`/`@[frame]`/`@[resume]` attributes (falling back to the conventional
// names) and emits `null` Kotlin lambdas for absent entries.

/// The exported C-ABI entry names a mobile app binds into the runtime bridge.
///
/// `start` is required (the app's main entry); `frame`/`resume` are optional
/// (an app with no frame loop / no lifecycle just omits them). Resolved from
/// the program by [`MobileEntries::from_program`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MobileEntries {
    pub start: Option<String>,
    pub frame: Option<String>,
    pub resume: Option<String>,
}

impl MobileEntries {
    /// Resolve the lifecycle entry names from `@[entry]`/`@[frame]`/`@[resume]`
    /// attributes, falling back to the conventional names (`on_start`,
    /// `on_frame`, `on_resume`) or `main` when none is annotated.
    pub fn from_program(program: &Program) -> Self {
        let mut e = MobileEntries::default();
        let mut has_main = false;
        let mut has_on_start = false;
        for item in &program.items {
            if let Item::FnDef(f) = item {
                if f.name == "main" {
                    has_main = true;
                }
                if f.name == "on_start" {
                    has_on_start = true;
                }
                if f.name == "on_frame" && e.frame.is_none() {
                    e.frame = Some("on_frame".to_string());
                }
                if f.name == "on_resume" && e.resume.is_none() {
                    e.resume = Some("on_resume".to_string());
                }
                for a in &f.attrs {
                    match a.name.as_str() {
                        "entry" => e.start = Some(f.name.clone()),
                        "frame" => e.frame = Some(f.name.clone()),
                        "resume" => e.resume = Some(f.name.clone()),
                        _ => {}
                    }
                }
            }
        }
        if e.start.is_none() {
            e.start = if has_on_start {
                Some("on_start".to_string())
            } else if has_main {
                Some("main".to_string())
            } else {
                None
            };
        }
        e
    }
}

/// Generate the deterministic Android Kotlin `@main` shim for `lib`
/// (the `.so` base name, e.g. `app` for `libapp.so`) and `entries`.
///
/// The shim is intentionally minimal (the "~50 lines" budget, §9): it loads the
/// native lib, declares the `external fun` bindings for the exported entries,
/// and registers them with the hand-written `AxonRuntime` bridge. The run loop
/// lives in `AxonRuntime`, not here.
pub fn generate_kotlin_wrapper(lib: &str, entries: &MobileEntries) -> String {
    let mut s = String::new();
    s.push_str("// Generated by `axon target build --host mobile` — DO NOT EDIT.\n");
    s.push_str("// The hand-written AxonRuntime lifecycle bridge owns the Android\n");
    s.push_str("// Activity run loop and calls these entries (R14 §3/§4, Q2).\n");
    s.push_str("package dev.axon.app\n\n");
    s.push_str("import dev.axon.runtime.AxonRuntime\n");
    s.push_str("import dev.axon.runtime.AxonEntries\n\n");
    s.push_str("object AxonApp {\n");
    s.push_str(&format!("    init {{ System.loadLibrary(\"{lib}\") }}\n\n"));

    // External bindings for each present entry. The C-ABI entries take/return
    // i64 (the frozen scalar ABI); the bridge marshals lifecycle state.
    let emit_extern = |s: &mut String, name: &str| {
        s.push_str(&format!("    external fun {name}(arg: Long): Long\n"));
    };
    if let Some(ref n) = entries.start {
        emit_extern(&mut s, n);
    }
    if let Some(ref n) = entries.frame {
        emit_extern(&mut s, n);
    }
    if let Some(ref n) = entries.resume {
        emit_extern(&mut s, n);
    }
    s.push('\n');

    // Register the entry table with the bridge.
    s.push_str("    val entries = AxonEntries(\n");
    s.push_str(&format!(
        "        start = {},\n",
        kt_lambda(entries.start.as_deref())
    ));
    s.push_str(&format!(
        "        frame = {},\n",
        kt_lambda(entries.frame.as_deref())
    ));
    s.push_str(&format!(
        "        resume = {}\n",
        kt_lambda(entries.resume.as_deref())
    ));
    s.push_str("    )\n\n");
    s.push_str("    fun launch() = AxonRuntime.run(entries)\n");
    s.push_str("}\n");
    s
}

fn kt_lambda(name: Option<&str>) -> String {
    match name {
        Some(n) => format!("{{ a -> {n}(a) }}"),
        None => "null".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Triple recognition + toolchain probe (iOS + Android shared core) ──────

    #[test]
    fn ios_triples_recognised() {
        assert!(is_mobile_triple("aarch64-apple-ios"));
        assert!(is_mobile_triple("aarch64-apple-ios-sim"));
        assert!(is_mobile_triple("x86_64-apple-ios"));
        assert!(is_ios_triple("aarch64-apple-ios"));
        assert!(!is_ios_triple("aarch64-linux-android"));
        assert!(!is_mobile_triple("x86_64-unknown-linux-gnu"));
        assert!(!is_mobile_triple("wasm32-wasip1"));
    }

    #[test]
    fn android_triples_recognised() {
        assert!(is_mobile_triple("aarch64-linux-android"));
        assert!(is_mobile_triple("x86_64-linux-android"));
        assert!(!is_ios_triple("aarch64-linux-android"));
    }

    #[test]
    fn simulator_flag_correct() {
        assert!(!mobile_target("aarch64-apple-ios").unwrap().simulator);
        assert!(mobile_target("aarch64-apple-ios-sim").unwrap().simulator);
        assert!(mobile_target("x86_64-apple-ios").unwrap().simulator);
    }

    #[test]
    fn ios_toolchain_absent_is_e1710_body() {
        // Simulate a host with NO `xcrun` on PATH (i.e. Linux — always the case
        // here). The probe must refuse, not pretend to succeed (I-9).
        let no_path = |_: &str| false;
        let no_env = |_: &str| false;
        let r = probe_toolchain("aarch64-apple-ios", no_path, no_env);
        assert!(r.is_err());
        let msg = r.unwrap_err();
        assert!(msg.contains("Xcode SDK"));
        assert!(msg.contains("xcrun"));
        assert!(msg.contains("aarch64-apple-ios"));
    }

    #[test]
    fn ios_toolchain_present_passes() {
        // On a real macOS host `xcrun` is on PATH → the probe passes.
        let has_xcrun = |c: &str| c == "xcrun";
        assert!(probe_toolchain("aarch64-apple-ios-sim", has_xcrun, |_| false).is_ok());
    }

    #[test]
    fn android_toolchain_via_ndk_env() {
        let has_ndk = |c: &str| c == "ANDROID_NDK_HOME";
        assert!(probe_toolchain("aarch64-linux-android", |_| false, has_ndk).is_ok());
        // Without the NDK env → E1710 body.
        assert!(probe_toolchain("aarch64-linux-android", |_| false, |_| false).is_err());
    }

    #[test]
    fn unknown_triple_probe_errs() {
        assert!(probe_toolchain("totally-bogus", |_| true, |_| true).is_err());
    }

    // ── jniLibs ABI mapping (Android) ─────────────────────────────────────────

    #[test]
    fn jni_abi_mapping() {
        assert_eq!(android_jni_abi("aarch64-linux-android"), Some("arm64-v8a"));
        assert_eq!(android_jni_abi("x86_64-linux-android"), Some("x86_64"));
        assert_eq!(
            android_jni_abi("armv7-linux-androideabi"),
            Some("armeabi-v7a")
        );
        assert_eq!(android_jni_abi("aarch64-apple-ios"), None);
        assert_eq!(android_jni_abi("x86_64-unknown-linux-gnu"), None);
    }

    // ── Triple-dispatched Swift / Kotlin shim generators ─────────────────────

    #[test]
    fn swift_shim_is_byte_stable() {
        // The golden test (§9 `mobile_wrapper_is_byte_stable`): the generator is
        // deterministic, so the same (app, entries) yields a byte-identical shim.
        let e = Entries::default();
        let a = gen_swift_shim("Demo", &e);
        let b = gen_swift_shim("Demo", &e);
        assert_eq!(a, b, "generator must be deterministic");
        // Spot-check the load-bearing contents.
        assert!(a.contains("import AxonRuntime"));
        assert!(a.contains("@main"));
        assert!(a.contains("struct DemoApp: AxonApp"));
        assert!(a.contains("@_silgen_name(\"on_start\")"));
        assert!(a.contains("@_silgen_name(\"on_frame\")"));
        assert!(a.contains("@_silgen_name(\"on_resume\")"));
        assert!(a.contains("AxonEntries("));
    }

    #[test]
    fn swift_shim_honours_custom_entries() {
        let e = Entries {
            start: "boot".into(),
            frame: "tick".into(),
            resume: "wake".into(),
        };
        let s = gen_swift_shim("Game", &e);
        assert!(s.contains("@_silgen_name(\"boot\")"));
        assert!(s.contains("@_silgen_name(\"tick\")"));
        assert!(s.contains("@_silgen_name(\"wake\")"));
        assert!(s.contains("struct GameApp: AxonApp"));
    }

    #[test]
    fn kotlin_shim_is_byte_stable() {
        let e = Entries::default();
        let a = gen_kotlin_shim("Demo", &e);
        let b = gen_kotlin_shim("Demo", &e);
        assert_eq!(a, b);
        assert!(a.contains("class DemoActivity : AxonActivity()"));
        assert!(a.contains("System.loadLibrary(\"Demo\")"));
        assert!(a.contains("external fun on_start"));
    }

    #[test]
    fn shims_are_under_80_lines() {
        // §9 acceptance: generated wrapper ≤ ~80 lines each.
        let e = Entries::default();
        assert!(shim_line_count(&gen_swift_shim("Demo", &e)) <= 80);
        assert!(shim_line_count(&gen_kotlin_shim("Demo", &e)) <= 80);
    }

    // ── Option-aware Android wrapper (resolved from program attrs) ────────────

    #[test]
    fn wrapper_is_byte_stable_golden() {
        // The golden — a fixed entry set must produce a fixed wrapper, twice.
        let entries = MobileEntries {
            start: Some("on_start".to_string()),
            frame: Some("on_frame".to_string()),
            resume: Some("on_resume".to_string()),
        };
        let a = generate_kotlin_wrapper("app", &entries);
        let b = generate_kotlin_wrapper("app", &entries);
        assert_eq!(a, b, "generator must be deterministic");

        // Byte-stable golden (regenerate intentionally if the template changes).
        let golden = "\
// Generated by `axon target build --host mobile` — DO NOT EDIT.
// The hand-written AxonRuntime lifecycle bridge owns the Android
// Activity run loop and calls these entries (R14 §3/§4, Q2).
package dev.axon.app

import dev.axon.runtime.AxonRuntime
import dev.axon.runtime.AxonEntries

object AxonApp {
    init { System.loadLibrary(\"app\") }

    external fun on_start(arg: Long): Long
    external fun on_frame(arg: Long): Long
    external fun on_resume(arg: Long): Long

    val entries = AxonEntries(
        start = { a -> on_start(a) },
        frame = { a -> on_frame(a) },
        resume = { a -> on_resume(a) }
    )

    fun launch() = AxonRuntime.run(entries)
}
";
        assert_eq!(a, golden, "wrapper diverged from golden");

        // The "~50 lines" budget (§9, ≤80 with headroom).
        assert!(
            a.lines().count() <= 80,
            "generated wrapper exceeds 80 lines: {}",
            a.lines().count()
        );
    }

    #[test]
    fn missing_entries_emit_null_lambdas() {
        let entries = MobileEntries {
            start: Some("main".to_string()),
            frame: None,
            resume: None,
        };
        let w = generate_kotlin_wrapper("compute", &entries);
        assert!(w.contains("external fun main(arg: Long): Long"));
        assert!(w.contains("frame = null"));
        assert!(w.contains("resume = null"));
        assert!(!w.contains("external fun on_frame"));
    }
}
