//! Hill-climb runtime for `@[goal]` autonomous optimization.
//!
//! V1 SCAFFOLDING: this module records the *intent* to run a hill-climb
//! search but does not actually evaluate variants. Real evaluation requires
//! test-set integration described in PRD lines 935–950 (variant generation
//! via `__axon_ai_complete`, score function pluggability, and re-link of the
//! winning variant). We log the call to provenance so the harness can
//! observe which goals fired during a run, and return a stub score (the
//! supplied target) so callers see deterministic behaviour.
//!
//! The shape of the function-pointer entry point matches the eventual
//! production signature so callers don't need to be re-wired when the real
//! optimizer lands.

use crate::provenance;

/// Run the hill-climb optimizer for the function named `fn_name`.
///
/// `fn_ptr` is reserved for a future direct-call ABI; v1 ignores it and
/// dispatches by name. `target_score` is the goal threshold the search is
/// trying to reach. `max_evals` caps the number of variant evaluations.
/// On exit, `*out_score` receives the best score observed (currently always
/// equal to `target_score` — see TODO above).
#[no_mangle]
pub extern "C" fn __axon_goal_run(
    _fn_ptr: *const u8,
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    target_score: f64,
    max_evals: i64,
    out_score: *mut f64,
) {
    let name = slice_to_str(fn_name_ptr, fn_name_len);

    // Record intent in provenance so a later harness step can pick this up.
    let payload = format!(
        "goal_run name={} target={:.6} max_evals={}",
        name, target_score, max_evals
    );
    provenance::__axon_provenance_log(
        fn_name_ptr,
        fn_name_len,
        payload.as_ptr(),
        payload.len() as i64,
    );

    // TODO(prd-935-950): generate variants via __axon_ai_complete, evaluate
    // each against the registered test/score function, return the best.
    let stub = target_score;
    if !out_score.is_null() {
        unsafe { *out_score = stub; }
    }
}

fn slice_to_str<'a>(ptr: *const u8, len: i64) -> &'a str {
    if ptr.is_null() || len <= 0 {
        return "";
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        std::str::from_utf8(bytes).unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn goal_run_writes_score() {
        let mut out: f64 = 0.0;
        let name = b"opt_fn";
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            0.85,
            32,
            &mut out as *mut f64,
        );
        assert!((out - 0.85).abs() < 1e-9);
    }
}
