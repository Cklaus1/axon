//! Hill-climb / best-observed runtime for `@[goal]` autonomous optimization.
//!
//! ## Layer 2 semantics
//!
//! At Layer 1 this module was a pure stub: `__axon_goal_run` always returned
//! the supplied `target_score` and only logged its intent to the provenance
//! file.
//!
//! Layer 2 makes the call **read-real-data**: it consults the in-memory
//! provenance store (populated by `__axon_provenance_log_ret_i64` /
//! `__axon_provenance_log_ret_f64` from each `@[adaptive]` function's return
//! site) and returns the **best observed score** for the named function.
//!
//! ### "Best" definition
//!
//! The notion of "best" depends on the metric direction.  Today the
//! `@[adaptive(metric: ..., target: T)]` parser does not distinguish minimize-
//! vs maximize, so we use a direction-agnostic heuristic: **best observed =
//! the recorded score whose absolute distance from `target` is smallest**.
//! Ties go to the earliest record.  This works for both
//! "approach-the-target" and "match-the-target" framings, and degenerates
//! gracefully to "argmin |score - target|" when no specific direction is
//! provided.
//!
//! ### Empty store
//!
//! If no records exist for `fn_name`, `__axon_goal_run` falls back to the
//! Layer-1 stub behaviour — return `target_score` — so existing fixtures
//! that call `goal_run` without first invoking the adaptive function keep
//! working.
//!
//! ### `max_evals`
//!
//! In Layer 2 we don't generate new variants (Track stretch goal — skipped).
//! `max_evals` therefore caps the **number of historical records consulted**
//! (most recent first).  A non-positive value means "no cap".  This still
//! keeps the parameter useful and reserves it for the eventual real
//! optimizer.
//!
//! ### Hill-climb (stretch — skipped)
//!
//! Real evaluation requires test-set integration described in PRD lines
//! 935–950 (variant generation via `__axon_ai_complete`, score function
//! pluggability, and re-link of the winning variant).  Layer 2 deliberately
//! ships a retrospective best-observed view; the function-pointer ABI
//! parameter `_fn_ptr` is reserved for the future direct-call path so callers
//! don't need to be re-wired when the real optimizer lands.

use crate::provenance;

/// Run the hill-climb / best-observed lookup for the function named
/// `fn_name`.
///
/// `_fn_ptr` is reserved for a future direct-call ABI; v1 ignores it and
/// dispatches by name.  `target_score` is the goal threshold the search is
/// trying to reach.  `max_evals` caps the number of records consulted (most
/// recent first); non-positive means "no cap".  On exit, `*out_score` receives
/// the best score observed (closest to `target_score`), or `target_score`
/// itself when no records exist.
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

    let best = best_observed(name, target_score, max_evals);
    if !out_score.is_null() {
        unsafe { *out_score = best; }
    }
}

/// Compute the best observed score for `name` against `target`.  Returns
/// `target` when no records exist (preserving Layer-1 stub behaviour).
fn best_observed(name: &str, target: f64, max_evals: i64) -> f64 {
    if name.is_empty() {
        return target;
    }
    let mut records = provenance::provenance_records_for(name);
    if records.is_empty() {
        return target;
    }
    // Honour `max_evals`: if positive, keep the most recent N records.
    if max_evals > 0 && (max_evals as usize) < records.len() {
        let drop = records.len() - max_evals as usize;
        records.drain(0..drop);
    }
    // Pick the score closest to `target` (absolute distance).
    // Ties resolved by earliest-record-wins (stable).
    let mut best = records[0].score;
    let mut best_dist = (best - target).abs();
    for r in &records[1..] {
        let d = (r.score - target).abs();
        if d < best_dist {
            best = r.score;
            best_dist = d;
        }
    }
    best
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
    use crate::provenance::{
        __axon_provenance_log_ret_i64, __axon_provenance_log_ret_f64,
    };

    #[test]
    fn goal_run_returns_target_when_no_records() {
        let mut out: f64 = 0.0;
        // Unique name so other parallel tests can't pollute the store for it.
        let name = b"goal_test_never_called_xyz";
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

    #[test]
    fn goal_run_picks_closest_to_target_i64() {
        let name = b"goal_test_measured_i64";
        // Record three observations: 20, 40, 60.  Target 50 → 40 wins
        // (|40-50|=10 vs |60-50|=10, tie; earliest wins → 40).
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 20);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 40);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 60);

        let mut out: f64 = 0.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            50.0,
            100,
            &mut out as *mut f64,
        );
        // Should be 40.0 (closer in tie, earliest wins).
        assert!((out - 40.0).abs() < 1e-9, "got {}", out);
    }

    #[test]
    fn goal_run_picks_closest_to_target_f64() {
        let name = b"goal_test_measured_f64";
        __axon_provenance_log_ret_f64(name.as_ptr(), name.len() as i64, 0.10);
        __axon_provenance_log_ret_f64(name.as_ptr(), name.len() as i64, 0.85);
        __axon_provenance_log_ret_f64(name.as_ptr(), name.len() as i64, 0.99);

        let mut out: f64 = 0.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            0.95,
            0, // 0 means no cap
            &mut out as *mut f64,
        );
        assert!((out - 0.99).abs() < 1e-9, "got {}", out);
    }

    #[test]
    fn goal_run_max_evals_keeps_recent_records() {
        let name = b"goal_test_recent_wins";
        // Old observations far from target.
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 1000);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 2000);
        // Recent observations closer to target.
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 51);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 49);

        let mut out: f64 = 0.0;
        // Cap at 2 most-recent records: only 51 and 49 considered.
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            50.0,
            2,
            &mut out as *mut f64,
        );
        // Either 51 or 49 — both equidistant; earliest of the kept slice wins (51).
        assert!((out - 51.0).abs() < 1e-9, "got {}", out);
    }

    #[test]
    fn goal_run_handles_null_name_pointer() {
        let mut out: f64 = 1.23;
        __axon_goal_run(
            std::ptr::null(),
            std::ptr::null(),
            0,
            0.42,
            8,
            &mut out as *mut f64,
        );
        // No name → no records → returns target (0.42).
        assert!((out - 0.42).abs() < 1e-9, "got {}", out);
    }
}
