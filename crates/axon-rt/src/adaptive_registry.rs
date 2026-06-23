//! Adaptive function registry for ASI Layer-3 live hill-climb.
//!
//! ## Purpose
//!
//! The Layer-2 `__axon_goal_run` is a **retrospective** optimizer: it walks
//! the in-memory provenance store and returns the best-observed score from
//! prior calls.  Layer-3 makes `goal_run` an **online** optimizer for a
//! narrow case: it can call the named `@[adaptive]` function back with
//! perturbed inputs and measure the result.  This module provides the bridge:
//! at module-init time codegen registers each eligible adaptive function's
//! function pointer here, and `goal_run` looks it up by name.
//!
//! ## v1 narrowing (strict)
//!
//! Only `@[adaptive]` functions matching the signature `fn(i64) -> i64` are
//! eligible for live hill-climb.  Anything else is silently skipped at the
//! codegen registration site and falls back to the Layer-2 retrospective
//! path inside `__axon_goal_run`.  No multi-arg, no `f64` input, no `str`
//! input — those are deferred to a future iteration.
//!
//! ## ABI
//!
//! `__axon_register_adaptive(name_ptr, name_len, fn_ptr)` is the C-linkage
//! registration entry point.  Codegen emits one call per eligible adaptive
//! function from inside `main`'s prologue (or an init thunk invoked from
//! `main`).  The `fn_ptr` is a raw pointer that the runtime stores as
//! `usize` and transmutes back to `unsafe extern "C" fn(i64) -> i64` only
//! at the call site (see `lookup_adaptive_i64`).
//!
//! ## Re-registration
//!
//! Re-registering the same name **overwrites** the previous entry.  This
//! matches the natural codegen flow (registrations happen once per process
//! start, in source order) and gives users a sensible "last definition wins"
//! story if they ever rebind manually.  No diagnostic is emitted.

use std::collections::HashMap;
use std::ffi::c_void;
use std::sync::Mutex;

/// Function pointers are stored as `usize` to satisfy Rust's ownership /
/// `Send` / `Sync` rules — a raw `*const c_void` is neither `Send` nor
/// `Sync`, but `usize` is.  At the call site we transmute back to the
/// concrete fn-pointer type.  The codegen contract is that the registered
/// pointer has signature `unsafe extern "C" fn(i64) -> i64`; calling it via
/// a different signature is undefined behaviour.
fn store() -> &'static Mutex<HashMap<String, usize>> {
    use std::sync::OnceLock;
    static STORE: OnceLock<Mutex<HashMap<String, usize>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register an adaptive function pointer by name.
///
/// Called from codegen-emitted IR at module-init time, once per eligible
/// `@[adaptive] fn(i64) -> i64`.  Re-registration overwrites the previous
/// entry without warning.
///
/// Safety: `fn_ptr` MUST refer to a function with C ABI signature
/// `extern "C" fn(i64) -> i64`.  Codegen is responsible for emitting calls
/// here only for eligible functions; the runtime takes the contract on faith.
#[no_mangle]
pub extern "C" fn __axon_register_adaptive(
    name_ptr: *const u8,
    name_len: i64,
    fn_ptr: *const c_void,
) {
    if name_ptr.is_null() || name_len <= 0 || fn_ptr.is_null() {
        return;
    }
    let bytes = unsafe { std::slice::from_raw_parts(name_ptr, name_len as usize) };
    let name = match std::str::from_utf8(bytes) {
        Ok(s) => s.to_string(),
        Err(_) => return,
    };
    let as_usize = fn_ptr as usize;
    if let Ok(mut g) = store().lock() {
        g.insert(name, as_usize);
    }
}

/// Look up a registered `fn(i64) -> i64` by name.
///
/// Returns `None` when the name has not been registered (or was registered
/// with a different signature — callers must trust the codegen contract).
pub(crate) fn lookup_adaptive_i64(name: &str) -> Option<unsafe extern "C" fn(i64) -> i64> {
    let g = store().lock().ok()?;
    let raw = *g.get(name)?;
    if raw == 0 {
        return None;
    }
    // Safety: codegen only registers pointers whose signature matches
    // `extern "C" fn(i64) -> i64` (see emit_register_adaptive).  Any other
    // use is UB and explicitly out of scope for v1.
    let f: unsafe extern "C" fn(i64) -> i64 = unsafe { std::mem::transmute(raw) };
    Some(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    extern "C" fn double(x: i64) -> i64 {
        x * 2
    }
    extern "C" fn add_one(x: i64) -> i64 {
        x + 1
    }

    #[test]
    fn register_and_lookup_calls_through() {
        let name = b"adapt_test_double";
        __axon_register_adaptive(name.as_ptr(), name.len() as i64, double as *const c_void);
        let f = lookup_adaptive_i64("adapt_test_double").expect("expected registered fn pointer");
        let got = unsafe { f(21) };
        assert_eq!(got, 42);
    }

    #[test]
    fn lookup_unknown_name_returns_none() {
        // Use a name unlikely to collide with anything else parallel tests
        // might register.
        assert!(lookup_adaptive_i64("adapt_test_never_registered_xyz").is_none());
    }

    #[test]
    fn re_registration_overwrites() {
        let name = b"adapt_test_overwrite";
        __axon_register_adaptive(name.as_ptr(), name.len() as i64, double as *const c_void);
        let f1 = lookup_adaptive_i64("adapt_test_overwrite").unwrap();
        assert_eq!(unsafe { f1(5) }, 10);

        // Re-register with a different fn pointer.
        __axon_register_adaptive(name.as_ptr(), name.len() as i64, add_one as *const c_void);
        let f2 = lookup_adaptive_i64("adapt_test_overwrite").unwrap();
        assert_eq!(unsafe { f2(5) }, 6, "second registration should win");
    }

    #[test]
    fn null_name_is_ignored() {
        __axon_register_adaptive(std::ptr::null(), 0, double as *const c_void);
        // Nothing observable happened; lookup with empty name fails.
        assert!(lookup_adaptive_i64("").is_none());
    }

    #[test]
    fn null_fn_ptr_is_ignored() {
        let name = b"adapt_test_null_fn";
        __axon_register_adaptive(name.as_ptr(), name.len() as i64, std::ptr::null());
        assert!(lookup_adaptive_i64("adapt_test_null_fn").is_none());
    }
}
