//! Hill-climb / best-observed runtime for `@[goal]` autonomous optimization.
//!
//! ## Layer 3 semantics (live hill-climb, narrow case)
//!
//! Layer 3 promotes `__axon_goal_run` from a retrospective lookup to an
//! **online** optimizer for the narrow signature `fn(i64) -> i64`:
//!
//!   1. Look the named function up in the adaptive registry
//!      ([`crate::adaptive_registry`]).
//!   2. If found: pick a starting input (best-observed input from the
//!      provenance store, or `0` if none), then hill-climb — try `cur ±
//!      step`, accept any improvement, halve `step` on stall, halt when
//!      `step < 1` or `max_evals` is exhausted.  Return the closest score
//!      observed against `target`.
//!   3. If not found: fall back to the Layer-2 retrospective path —
//!      argmin `|score - target|` over the in-memory provenance records.
//!
//! Because each evaluation goes through the user fn's normal `@[adaptive]`
//! prologue / typed-return logging, hill-climb data accumulates in the
//! provenance store automatically.
//!
//! ### v1 narrowing
//!
//! Live hill-climb is **only** available for `@[adaptive] fn(i64) -> i64`.
//! Anything else — multi-arg, `f64` input, `str` input, struct return —
//! never lands in the registry, so `__axon_goal_run` cleanly falls back to
//! the retrospective Layer-2 behaviour.  Backward compatibility is
//! preserved: programs that called `goal_run` against a function whose
//! signature is not (i64) -> i64, or against a function that has not yet
//! been called, behave exactly as they did under Layer 2.
//!
//! ### "Best" definition
//!
//! Direction-agnostic: closest observed score to `target` (smallest
//! `|score - target|`).  Ties go to the earliest record.
//!
//! ### `max_evals`
//!
//! Caps the number of **callbacks** the hill-climb performs in the live
//! path, and the number of **historical records consulted** in the
//! fallback path.  Non-positive ⇒ "no cap" in fallback; in the live path
//! we treat it as "no cap" as well, but the step-halving termination
//! guarantees finite work either way.

use crate::adaptive_registry::lookup_adaptive_i64;
use crate::provenance;

/// Run the hill-climb / best-observed lookup for the function named
/// `fn_name`.
///
/// `_fn_ptr` is reserved for a future direct-call ABI; v1 still dispatches
/// by name through the adaptive registry.  `target_score` is the goal
/// threshold the search is trying to reach.  On exit, `*out_score` receives
/// the best score observed (closest to `target_score`), or `target_score`
/// itself when neither path produced any data.
///
/// Dispatch:
///   1. If a `fn(i64) -> i64` is registered under `fn_name`, hill-climb
///      live, then return the closest observed score.
///   2. Otherwise fall back to argmin `|score - target|` over the in-memory
///      provenance store (Layer-2 behaviour).
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

    // Layer-3: prefer live hill-climb when the registry has a callable for
    // this name.  Otherwise fall back to the Layer-2 retrospective path so
    // existing fixtures (and any non-eligible adaptive fns) keep working.
    let best = if let Some(f) = lookup_adaptive_i64(name) {
        hill_climb_i64(name, f, target_score, max_evals)
    } else {
        best_observed(name, target_score, max_evals)
    };
    if !out_score.is_null() {
        unsafe { *out_score = best; }
    }
}

/// Live hill-climb for an `@[adaptive] fn(i64) -> i64`.
///
/// Strategy:
/// * Start from the best-observed input in the provenance store if any —
///   today the store records *return values*, not inputs, so we use `0` as
///   the starting input.  (Storing inputs is a future improvement.)
/// * Keep `step = max(1, |start| / 4)` (or `1` when `start = 0`).
/// * Each iteration: try `cur + step` and `cur - step`.  If either is
///   strictly closer to `target` than the current best, move there and
///   continue without halving.  Otherwise halve the step.  Halt when the
///   step would drop below `1`, or when `max_evals` is exhausted.
///
/// All callbacks go through the user function's normal `@[adaptive]`
/// prologue / typed-return logging, so the provenance store accumulates
/// hill-climb data as a side effect.
fn hill_climb_i64(
    name: &str,
    f: unsafe extern "C" fn(i64) -> i64,
    target: f64,
    max_evals: i64,
) -> f64 {
    // F11: warm-start from the best-observed INPUT in the provenance store, if
    // one was recorded (via `__axon_provenance_log_ret_i64_in`). The store now
    // carries `(input, score)` pairs, so we seed the climb at the input whose
    // prior score was closest to `target` instead of always cold-starting at 0
    // — this is what lets a native goal_run resume a search across calls. Falls
    // back to 0 when no input-bearing record exists (back-compat with the
    // score-only path and the canonical squared-error fixture).
    let mut cur_input: i64 = provenance::best_input_for(name, target).unwrap_or(0);

    // Initial call to seed `best_score`.
    let initial: f64 = unsafe { f(cur_input) } as f64;
    let mut best_input: i64 = cur_input;
    let mut best_score: f64 = initial;
    let mut best_dist: f64 = (initial - target).abs();
    let mut evals: i64 = 1;

    let cap_evals = max_evals; // negative or zero ⇒ no cap (loop exits via step)
    let unlimited = cap_evals <= 0;

    let mut step: i64 = {
        let abs = cur_input.unsigned_abs() as i64;
        std::cmp::max(1, abs / 4)
    };

    while step >= 1 {
        if !unlimited && evals >= cap_evals {
            break;
        }

        let mut improved = false;

        // Try cur + step.
        let up_input = cur_input.saturating_add(step);
        let up_score = unsafe { f(up_input) } as f64;
        evals += 1;
        let up_dist = (up_score - target).abs();
        if up_dist < best_dist {
            best_dist = up_dist;
            best_score = up_score;
            best_input = up_input;
            improved = true;
        }

        if !unlimited && evals >= cap_evals {
            // Budget exhausted after the up-step; we already recorded any
            // improvement into `best_score` / `best_dist`.  No need to
            // update `cur_input` (loop is about to exit).
            let _ = improved; // silence unused on this path
            break;
        }

        // Try cur - step.
        let dn_input = cur_input.saturating_sub(step);
        let dn_score = unsafe { f(dn_input) } as f64;
        evals += 1;
        let dn_dist = (dn_score - target).abs();
        if dn_dist < best_dist {
            best_dist = dn_dist;
            best_score = dn_score;
            best_input = dn_input;
            improved = true;
        }

        if improved {
            // Walk to the improved point.  If both directions improved,
            // best_input already reflects whichever was strictly better
            // (down only wins on strict <, so up wins ties — keeps things
            // deterministic).
            cur_input = best_input;
        } else {
            // No improvement in either direction — refine the step.
            step /= 2;
        }
    }

    best_score
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
        __axon_provenance_log_ret_i64_in,
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

    // ── Layer-3: live hill-climb tests ────────────────────────────────────
    //
    // These exercise the `__axon_register_adaptive` → `__axon_goal_run` path:
    // when the registry has a `fn(i64) -> i64` for the target name, goal_run
    // should call it back, hill-climb the input, and return a score close to
    // the target.

    use crate::adaptive_registry::__axon_register_adaptive;
    use std::ffi::c_void;

    extern "C" fn squared_error_x_minus_7(x: i64) -> i64 {
        let d = x - 7;
        d * d
    }

    extern "C" fn linear_2x(x: i64) -> i64 { 2 * x }

    #[test]
    fn goal_run_hillclimb_finds_minimum_of_quadratic() {
        // f(x) = (x - 7)^2.  Target = 0.0 ⇒ optimum is x = 7, score 0.0.
        let name = b"goal_hill_quadratic";
        __axon_register_adaptive(
            name.as_ptr(),
            name.len() as i64,
            squared_error_x_minus_7 as *const c_void,
        );

        let mut out: f64 = -1.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            0.0,
            64,
            &mut out as *mut f64,
        );
        // With 64 evals on a 1-D quadratic the climb easily reaches the
        // global minimum.  Allow a generous tolerance — this test is about
        // "did we actually call the user fn and improve", not about the
        // exact optimizer policy.
        assert!(out >= 0.0, "score must be non-negative, got {}", out);
        assert!(out < 4.0, "expected score near 0, got {} (hill-climb stuck?)", out);
    }

    #[test]
    fn goal_run_hillclimb_with_max_evals_one_returns_initial_call() {
        // max_evals = 1: we make exactly the seeding call (f(0)) and bail.
        // Score = (0 - 7)^2 = 49.
        let name = b"goal_hill_max_evals_one";
        __axon_register_adaptive(
            name.as_ptr(),
            name.len() as i64,
            squared_error_x_minus_7 as *const c_void,
        );

        let mut out: f64 = -1.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            0.0,
            1,
            &mut out as *mut f64,
        );
        // Initial seed call is f(0) = 49.  No further evals allowed.
        assert!((out - 49.0).abs() < 1e-9, "got {}", out);
    }

    #[test]
    fn goal_run_hillclimb_linear_finds_target_score() {
        // f(x) = 2x.  Target = 100 ⇒ optimum is x = 50, score 100.
        let name = b"goal_hill_linear";
        __axon_register_adaptive(
            name.as_ptr(),
            name.len() as i64,
            linear_2x as *const c_void,
        );

        let mut out: f64 = -1.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            100.0,
            128,
            &mut out as *mut f64,
        );
        // Should land exactly on 100 (step-1 hits x = 50 cleanly from 0).
        assert!((out - 100.0).abs() < 1e-9, "got {}", out);
    }

    #[test]
    fn goal_run_warm_starts_from_best_recorded_input() {
        // F11: a quadratic centered far from 0, where a cold start at x=0 with a
        // tiny eval budget cannot reach the optimum, but a warm start near it
        // can. f(x) = (x - 1000)^2; optimum x=1000, score 0.
        extern "C" fn sq_x_minus_1000(x: i64) -> i64 {
            let d = x - 1000;
            d * d
        }
        let name = b"f11_goal_warmstart";
        __axon_register_adaptive(
            name.as_ptr(),
            name.len() as i64,
            sq_x_minus_1000 as *const c_void,
        );
        // Seed the store with a good prior input near the optimum.
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 990, 100);

        let mut out: f64 = -1.0;
        // Tiny budget: from x=0 (step 1) this couldn't get near 1000, but warm-
        // started at 990 the climb reaches the optimum quickly.
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            0.0,
            64,
            &mut out as *mut f64,
        );
        assert!(out < 100.0, "warm-started climb should beat the 990-seed score of 100, got {}", out);
    }

    #[test]
    fn goal_run_falls_back_to_retrospective_when_registry_empty() {
        // Use a unique name that is NOT registered.  The retrospective path
        // should still return the closest-to-target observation from the
        // provenance store.
        let name = b"goal_fallback_when_unregistered";
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 5);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 95);

        let mut out: f64 = 0.0;
        __axon_goal_run(
            std::ptr::null(),
            name.as_ptr(),
            name.len() as i64,
            100.0,
            16,
            &mut out as *mut f64,
        );
        // 95 is closer to 100 than 5 is.
        assert!((out - 95.0).abs() < 1e-9, "got {}", out);
    }
}
