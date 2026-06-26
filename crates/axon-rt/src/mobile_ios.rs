//! R14 (iOS slices 1–2) — the iOS `AxonRuntime` lifecycle bridge.
//!
//! Spec §4: a mobile app is NOT compute — the OS owns the run loop
//! (`UIApplicationMain` / scene lifecycle), so the wrapper's real job is to
//! bridge that OS-owned, inverted-control-flow lifecycle into Axon callbacks.
//! That bridge is a runtime component, not a mechanical shim, and it suspends /
//! re-enters the Axon computation via the Phase-6 `host_await` resume runtime
//! (R15, already landed) as the app moves through lifecycle events.
//!
//! ## What is gated how (the honest macOS-host boundary, spec §12 Q4)
//!
//! This file has two strata:
//!
//! 1. **The cross-platform lifecycle CORE** (`AxonRuntime`, `Phase`, the
//!    transition state machine). It has NO iOS dependency — it is a pure state
//!    machine over the Axon entry callbacks — so it compiles and is unit-tested
//!    on EVERY host, including the Linux/WSL2 gate. This is what proves the
//!    bridge logic is correct without a device.
//!
//! 2. **The iOS FFI entry points** (`axon_ios_*`, `#[no_mangle] extern "C"`).
//!    These are the symbols the generated Swift `@main` shim + the `AxonRuntime`
//!    Swift support library call from `UIApplicationDelegate`/scene callbacks.
//!    They are `#[cfg(target_os = "ios")]`-gated, so they are **NEVER compiled on
//!    Linux** — the Linux gate cannot regress on them. They are compiled + run
//!    only by the macOS CI job (`.github/workflows/ios.yml`) on an iOS simulator.
//!
//! The whole module is additionally behind the off-by-default `ios` feature, so
//! the standard Linux `gate.sh` (which never passes `--features ios`) does not
//! build it at all — belt and suspenders on the "never breaks Linux" rule.

/// The lifecycle phase of an `AxonRuntime`. Mirrors the iOS app lifecycle the
/// `AxonRuntime` Swift bridge translates from (`didFinishLaunching` →
/// `sceneWillEnterForeground`/`didEnterBackground` → `willTerminate`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    /// Constructed but `on_start` not yet run.
    New,
    /// `on_start` has run; the frame loop (`CADisplayLink`) drives `on_frame`.
    Running,
    /// Backgrounded — the Axon computation is suspended (parked at a
    /// `host_await`); no `on_frame` ticks are delivered.
    Suspended,
    /// Torn down (`willTerminate`); no further callbacks are valid.
    TornDown,
}

/// The result of a lifecycle transition: `Ok(value)` where `value` is the i64 the
/// Axon entry returned (e.g. a frame's render-status), or `Err` if the transition
/// was invalid for the current phase (never aborts — I-4).
pub type TransitionResult = Result<i64, String>;

/// One Axon entry callback: a C-ABI `fn(ctx) -> i64`. `ctx` is an opaque pointer
/// the bridge hands through (the platform view / surface context); the Axon side
/// treats it as an i64 handle. Modelled here as a Rust fn pointer so the CORE is
/// testable on Linux with stub callbacks.
pub type EntryFn = extern "C" fn(ctx: i64) -> i64;

/// The iOS lifecycle bridge state machine. Owns the current [`Phase`] and the
/// three entry callbacks (`on_start` / `on_frame` / `on_resume`). The Swift
/// `AxonRuntime` support library constructs one of these and drives it from the
/// OS callbacks; this Rust core enforces the legal transition graph so an
/// out-of-order OS event (e.g. a frame tick after teardown) is a graceful refusal
/// rather than a wild call into freed Axon state.
pub struct AxonRuntime {
    phase: Phase,
    start: EntryFn,
    frame: EntryFn,
    resume: EntryFn,
    /// Monotonic count of delivered frames (the parity/journey observable).
    frames: u64,
}

impl AxonRuntime {
    /// Construct a runtime bound to the app's three entry callbacks. Starts in
    /// [`Phase::New`]; the OS's launch event drives [`AxonRuntime::start`].
    pub fn new(start: EntryFn, frame: EntryFn, resume: EntryFn) -> Self {
        AxonRuntime {
            phase: Phase::New,
            start,
            frame,
            resume,
            frames: 0,
        }
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn frames_delivered(&self) -> u64 {
        self.frames
    }

    /// `didFinishLaunching` → run `on_start`. Legal only from `New`.
    pub fn start(&mut self, ctx: i64) -> TransitionResult {
        if self.phase != Phase::New {
            return Err(format!(
                "AxonRuntime: start() called in phase {:?} (expected New)",
                self.phase
            ));
        }
        let r = (self.start)(ctx);
        self.phase = Phase::Running;
        Ok(r)
    }

    /// `CADisplayLink` tick → run `on_frame`. Legal only while `Running`. A tick
    /// arriving while Suspended/TornDown is a graceful no-op refusal (the display
    /// link should be paused, but races happen — never call into a parked
    /// computation).
    pub fn frame(&mut self, ctx: i64) -> TransitionResult {
        if self.phase != Phase::Running {
            return Err(format!(
                "AxonRuntime: frame() called in phase {:?} (expected Running)",
                self.phase
            ));
        }
        let r = (self.frame)(ctx);
        self.frames += 1;
        Ok(r)
    }

    /// `didEnterBackground` → suspend the Axon computation. Legal from `Running`.
    /// The actual suspension is the `host_await` park inside the Axon program;
    /// the bridge just records the phase + pauses frame delivery.
    pub fn suspend(&mut self) -> TransitionResult {
        if self.phase != Phase::Running {
            return Err(format!(
                "AxonRuntime: suspend() called in phase {:?} (expected Running)",
                self.phase
            ));
        }
        self.phase = Phase::Suspended;
        Ok(0)
    }

    /// `willEnterForeground` → run `on_resume` and rewind to `Running`. Legal only
    /// from `Suspended`.
    pub fn resume(&mut self, ctx: i64) -> TransitionResult {
        if self.phase != Phase::Suspended {
            return Err(format!(
                "AxonRuntime: resume() called in phase {:?} (expected Suspended)",
                self.phase
            ));
        }
        let r = (self.resume)(ctx);
        self.phase = Phase::Running;
        Ok(r)
    }

    /// `willTerminate` → tear down. Legal from any non-`TornDown` phase; after
    /// this, every transition refuses. Idempotent-safe: a second teardown is a
    /// graceful refusal, not a crash.
    pub fn teardown(&mut self) -> TransitionResult {
        if self.phase == Phase::TornDown {
            return Err("AxonRuntime: teardown() called twice".to_string());
        }
        self.phase = Phase::TornDown;
        Ok(0)
    }
}

// ── The iOS-only `#[no_mangle] extern "C"` entry points ──────────────────────
//
// These are the symbols the generated Swift `@main` shim + the `AxonRuntime`
// Swift support library call. They wrap a process-global `AxonRuntime`. They are
// `cfg(target_os = "ios")`-gated, so the Linux gate NEVER compiles them; they are
// built + exercised only by the macOS CI simulator run.
//
// A C-ABI extern cannot unwind across the FFI line, so an invalid transition is
// surfaced as a negative i64 status (the bridge maps it to a graceful app-level
// error / pauses the display link) rather than a panic — I-4 "never abort the
// host". The Swift side reads the status and decides; it never sees a crash.

#[cfg(target_os = "ios")]
mod ffi {
    use super::{AxonRuntime, EntryFn};
    use std::sync::Mutex;

    /// The process-global runtime. A mobile app is a single run; the Swift bridge
    /// constructs it once at launch via `axon_ios_runtime_new`.
    static RUNTIME: Mutex<Option<AxonRuntime>> = Mutex::new(None);

    /// Map a `TransitionResult` to the C-ABI i64 status: the entry's return value
    /// on success, or a negative error sentinel on an invalid transition.
    fn status(r: super::TransitionResult) -> i64 {
        match r {
            Ok(v) => v,
            Err(e) => {
                // Log to the iOS unified log via stderr (NSLog-bridged by the
                // Swift side); never panic across the FFI boundary.
                eprintln!("axon-ios: {e}");
                -1
            }
        }
    }

    /// `axon_ios_runtime_new(start, frame, resume)` — construct the global
    /// runtime bound to the app's three C-ABI entry symbols. Called once from the
    /// Swift `AxonRuntime` bridge at `didFinishLaunching`.
    ///
    /// # Safety
    /// The three function pointers must be valid `extern "C" fn(i64) -> i64`
    /// symbols exported by the linked Axon static lib (the generated shim binds
    /// them). The Swift bridge supplies them via `@_silgen_name`.
    #[no_mangle]
    pub extern "C" fn axon_ios_runtime_new(start: EntryFn, frame: EntryFn, resume: EntryFn) {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        *g = Some(AxonRuntime::new(start, frame, resume));
    }

    /// `axon_ios_start(ctx)` — run `on_start` (the `didFinishLaunching` hook).
    #[no_mangle]
    pub extern "C" fn axon_ios_start(ctx: i64) -> i64 {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            Some(rt) => status(rt.start(ctx)),
            None => -1,
        }
    }

    /// `axon_ios_frame(ctx)` — deliver one frame (the `CADisplayLink` hook).
    #[no_mangle]
    pub extern "C" fn axon_ios_frame(ctx: i64) -> i64 {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            Some(rt) => status(rt.frame(ctx)),
            None => -1,
        }
    }

    /// `axon_ios_suspend()` — `didEnterBackground`.
    #[no_mangle]
    pub extern "C" fn axon_ios_suspend() -> i64 {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            Some(rt) => status(rt.suspend()),
            None => -1,
        }
    }

    /// `axon_ios_resume(ctx)` — `willEnterForeground`.
    #[no_mangle]
    pub extern "C" fn axon_ios_resume(ctx: i64) -> i64 {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            Some(rt) => status(rt.resume(ctx)),
            None => -1,
        }
    }

    /// `axon_ios_teardown()` — `willTerminate`.
    #[no_mangle]
    pub extern "C" fn axon_ios_teardown() -> i64 {
        let mut g = RUNTIME.lock().unwrap_or_else(|p| p.into_inner());
        match g.as_mut() {
            Some(rt) => status(rt.teardown()),
            None => -1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Stub Axon entries: each returns a distinct sentinel so we can see which
    // callback the bridge invoked. These stand in for the codegen-emitted
    // `on_start`/`on_frame`/`on_resume` symbols.
    extern "C" fn stub_start(_ctx: i64) -> i64 {
        100
    }
    extern "C" fn stub_frame(ctx: i64) -> i64 {
        ctx + 1
    }
    extern "C" fn stub_resume(_ctx: i64) -> i64 {
        200
    }

    fn rt() -> AxonRuntime {
        AxonRuntime::new(stub_start, stub_frame, stub_resume)
    }

    #[test]
    fn happy_path_lifecycle() {
        let mut r = rt();
        assert_eq!(r.phase(), Phase::New);
        assert_eq!(r.start(0).unwrap(), 100);
        assert_eq!(r.phase(), Phase::Running);
        assert_eq!(r.frame(41).unwrap(), 42);
        assert_eq!(r.frame(41).unwrap(), 42);
        assert_eq!(r.frames_delivered(), 2);
        assert_eq!(r.suspend().unwrap(), 0);
        assert_eq!(r.phase(), Phase::Suspended);
        assert_eq!(r.resume(0).unwrap(), 200);
        assert_eq!(r.phase(), Phase::Running);
        assert_eq!(r.teardown().unwrap(), 0);
        assert_eq!(r.phase(), Phase::TornDown);
    }

    #[test]
    fn frame_before_start_refused() {
        let mut r = rt();
        assert!(r.frame(0).is_err());
        // The refusal does not advance the phase or the frame counter (I-4).
        assert_eq!(r.phase(), Phase::New);
        assert_eq!(r.frames_delivered(), 0);
    }

    #[test]
    fn frame_while_suspended_refused() {
        // A display-link race: a tick after backgrounding must not call into the
        // parked Axon computation.
        let mut r = rt();
        r.start(0).unwrap();
        r.suspend().unwrap();
        assert!(r.frame(0).is_err());
        assert_eq!(r.frames_delivered(), 0);
    }

    #[test]
    fn resume_without_suspend_refused() {
        let mut r = rt();
        r.start(0).unwrap();
        assert!(r.resume(0).is_err());
    }

    #[test]
    fn double_teardown_refused_not_crash() {
        let mut r = rt();
        r.start(0).unwrap();
        assert!(r.teardown().is_ok());
        // Second teardown is a graceful refusal, never a crash (I-4).
        assert!(r.teardown().is_err());
        assert_eq!(r.phase(), Phase::TornDown);
    }

    #[test]
    fn callbacks_after_teardown_refused() {
        let mut r = rt();
        r.start(0).unwrap();
        r.teardown().unwrap();
        assert!(r.frame(0).is_err());
        assert!(r.suspend().is_err());
        assert!(r.resume(0).is_err());
        assert!(r.start(0).is_err());
    }
}
