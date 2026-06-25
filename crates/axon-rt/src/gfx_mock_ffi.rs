//! R13 slice 4 — the native-link side of the `native::gfx` mock shim.
//!
//! These `#[no_mangle] extern "C"` symbols are what codegen declares and links
//! (the BUILTIN_EXTERNS path, generalized to native modules). They wrap a
//! **process-global** [`GfxMock`] — the same `axon-gfx-mock` impl the
//! interpreter drives per-`Interp`. One impl, two call sites (spec §3/§11), so
//! the value-returning `frame_count` is byte-identical across engines (the
//! differential-parity corpus member) and the effectful calls share the
//! call-trace oracle.
//!
//! ## The handle ABI — `{i64 tag, i64 payload}` by value (I-6 frozen layout)
//!
//! A native `Handle` crosses the boundary as a 16-byte `#[repr(C)] {i64,i64}`
//! aggregate, which on the supported targets (x86-64 / aarch64 SysV) passes and
//! returns identically to LLVM's `{i64,i64}` struct (two integer registers).
//! `tag` is the frozen nominal id ([`axon_gfx_mock::tag_for`]); `payload` is the
//! **slab index** — NEVER a raw pointer (I-4/I-11): only an i64 index crosses
//! the line, so a forged/stale index can at worst hit an absent slot and return
//! a graceful error, never deref a wild pointer or abort the host.
//!
//! ## Errors are graceful (I-4)
//!
//! A bad-handle / wrong-shape dispatch returns `Err`; the C-ABI shim cannot
//! unwind across the FFI line, so it prints a panic line and `process::exit`s
//! with the runtime-panic code (101) — exactly like axon-rt's `abs_i64`
//! overflow guard. The host is never aborted with a segfault.

use std::sync::Mutex;

use axon_gfx_mock::{tag_for, GfxArg, GfxMock, GfxValue};

use crate::{AxonStr, RUNTIME_PANIC_EXIT_CODE};

/// The frozen native-handle ABI: `{ i64 tag, i64 payload }`, matching codegen's
/// `{i64,i64}` Handle struct exactly. `tag` = nominal id, `payload` = slab idx.
#[repr(C)]
#[derive(Clone, Copy)]
pub struct AxonHandle {
    pub tag: i64,
    pub payload: i64,
}

impl AxonHandle {
    fn as_arg(self) -> GfxArg {
        GfxArg::Handle {
            tag: self.tag,
            payload: self.payload,
        }
    }
    fn of(name: &str, payload: i64) -> AxonHandle {
        AxonHandle {
            tag: tag_for(name),
            payload,
        }
    }
}

/// The process-global mock backend. A native Axon program is a single run, so a
/// global (behind a `Mutex` for soundness if a future emit threads) is the right
/// shape — the interpreter keeps its own per-`Interp` instance.
static GFX: Mutex<Option<GfxMock>> = Mutex::new(None);

/// Run `f` against the global mock, lazily initializing it.
fn with_gfx<R>(f: impl FnOnce(&mut GfxMock) -> R) -> R {
    let mut guard = GFX.lock().unwrap_or_else(|p| p.into_inner());
    let mock = guard.get_or_insert_with(GfxMock::new);
    f(mock)
}

/// Map an `Err` (or an unexpected value shape) to a graceful exit-101 panic.
/// A C-ABI extern cannot unwind, so we print + `exit` rather than `panic!`.
fn graceful_abort(msg: &str) -> ! {
    eprintln!("axon: panic: {msg}");
    std::process::exit(RUNTIME_PANIC_EXIT_CODE);
}

fn expect_handle(r: axon_gfx_mock::GfxResult, who: &str) -> AxonHandle {
    match r {
        Ok(GfxValue::Handle { name, payload }) => AxonHandle::of(name, payload),
        Ok(_) => graceful_abort(&format!("native::gfx::{who}: expected a handle result")),
        Err(e) => graceful_abort(&e),
    }
}

fn expect_unit(r: axon_gfx_mock::GfxResult, who: &str) {
    match r {
        Ok(GfxValue::Unit) => {}
        Ok(_) => graceful_abort(&format!("native::gfx::{who}: expected a unit result")),
        Err(e) => graceful_abort(&e),
    }
}

fn expect_int(r: axon_gfx_mock::GfxResult, who: &str) -> i64 {
    match r {
        Ok(GfxValue::Int(n)) => n,
        Ok(_) => graceful_abort(&format!("native::gfx::{who}: expected an int result")),
        Err(e) => graceful_abort(&e),
    }
}

// ── The C symbols codegen declares & links (spec §3 manifest `symbol`s) ───────

/// `window_open(w: i64, h: i64, title: str) -> Window`.
#[no_mangle]
pub extern "C" fn __axon_gfx_window_open(w: i64, h: i64, title: AxonStr) -> AxonHandle {
    // SAFETY: `title` came from codegen's str ABI (the same contract every
    // str-taking axon-rt extern relies on).
    let t = unsafe { title.as_str() };
    let r = with_gfx(|m| m.dispatch("window_open", &[GfxArg::Int(w), GfxArg::Int(h), GfxArg::Str(t.to_string())]));
    expect_handle(r, "window_open")
}

/// `surface(ref win: Window) -> Surface`.
#[no_mangle]
pub extern "C" fn __axon_gfx_surface(win: AxonHandle) -> AxonHandle {
    let r = with_gfx(|m| m.dispatch("surface", &[win.as_arg()]));
    expect_handle(r, "surface")
}

/// `clear(ref s: Surface, r,g,b,a: f64) -> Unit`.
#[no_mangle]
pub extern "C" fn __axon_gfx_clear(s: AxonHandle, r: f64, g: f64, b: f64, a: f64) {
    let res = with_gfx(|m| {
        m.dispatch(
            "clear",
            &[
                s.as_arg(),
                GfxArg::Float(r),
                GfxArg::Float(g),
                GfxArg::Float(b),
                GfxArg::Float(a),
            ],
        )
    });
    expect_unit(res, "clear");
}

/// `present(ref s: Surface) -> Unit`.
#[no_mangle]
pub extern "C" fn __axon_gfx_present(s: AxonHandle) {
    let r = with_gfx(|m| m.dispatch("present", &[s.as_arg()]));
    expect_unit(r, "present");
}

/// `frame_count(ref s: Surface) -> i64`.
#[no_mangle]
pub extern "C" fn __axon_gfx_frame_count(s: AxonHandle) -> i64 {
    let r = with_gfx(|m| m.dispatch("frame_count", &[s.as_arg()]));
    expect_int(r, "frame_count")
}

/// `surface_close(s: Surface) -> Unit` — consumes the Surface.
#[no_mangle]
pub extern "C" fn __axon_gfx_surface_close(s: AxonHandle) {
    let r = with_gfx(|m| m.dispatch("surface_close", &[s.as_arg()]));
    expect_unit(r, "surface_close");
}

/// `window_close(win: Window) -> Unit` — consumes the Window.
#[no_mangle]
pub extern "C" fn __axon_gfx_window_close(win: AxonHandle) {
    let r = with_gfx(|m| m.dispatch("window_close", &[win.as_arg()]));
    expect_unit(r, "window_close");
}
