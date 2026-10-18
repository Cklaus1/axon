//! Giving up sooner on a defect one function body cannot repair.
//!
//! Cortex replaces ONE function body per attempt. A defect that needs a second
//! edit, a signature change or a type change is therefore out of its reach, and
//! before this module the only way it said so was to run every candidate's
//! whole step budget and report `budget_exhausted`. Measured on the hard set's
//! 13 out-of-scope tasks that took 547 s against a plain agent's 148 s, and 76%
//! of those seconds were model calls made after a candidate's first proposal
//! (`benchmarks/earlystop/README.md`).
//!
//! Two signals, both read from what Cortex already observes and neither from
//! the hidden check:
//!
//! 1. **No function is reached by every failing visible check** (decided
//!    before any model call). A verified fix needs every check that was
//!    failing to pass, so the function that is edited must be one that all of
//!    them run. When no function is, no single body edit can verify, whatever
//!    a generator writes. It is a necessary condition, so a fix is never
//!    refused by it unless the static call analysis under-approximates what a
//!    check runs. When some functions are reached by all, the candidates that
//!    are not are never asked either.
//! 2. **The visible verdict does not move** (decided between proposals). A
//!    proposal is *inert* when it compiled, was refused, and left the failing
//!    visible checks exactly as they were before any proposal. A candidate
//!    whose last `inert_limit` compiled proposals were all inert is not asked
//!    again: the generator has seen the visible output after the first one
//!    and the second changed nothing about it.
//!
//! The outcome is `out_of_scope_suspected` (exit 28). It is a suspicion, not a
//! proof, and is reported as one.

use crate::generate::RejectionReason;
use crate::runner::AttemptSignal;

/// Default for `--inert-limit`.
pub const DEFAULT_INERT_LIMIT: usize = 2;

/// The configuration of both signals. `Default` is the shipped behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EarlyStop {
    /// Decide from which functions the failing visible checks reach, before
    /// any model call. `--early-stop off` clears this and `inert_limit`.
    pub structural: bool,
    /// Stop asking a candidate after this many consecutive inert proposals.
    /// `0` never stops one.
    pub inert_limit: usize,
}

impl Default for EarlyStop {
    fn default() -> Self {
        EarlyStop {
            structural: true,
            inert_limit: DEFAULT_INERT_LIMIT,
        }
    }
}

impl EarlyStop {
    /// Both signals off: the walk of earlier versions, byte for byte.
    pub fn off() -> Self {
        EarlyStop {
            structural: false,
            inert_limit: 0,
        }
    }

    pub fn is_off(&self) -> bool {
        !self.structural && self.inert_limit == 0
    }
}

/// The candidates worth asking, in their ranked order.
///
/// `ranked` is the spectrum order, `reached_by_all` the functions every
/// failing visible check reaches. The result is `ranked` filtered to that
/// set; empty means no function qualifies and the walk should not start.
pub fn candidates_in_reach(ranked: &[String], reached_by_all: &[String]) -> Vec<String> {
    ranked
        .iter()
        .filter(|c| reached_by_all.contains(c))
        .cloned()
        .collect()
}

/// Whether a proposal left the visible verdict where it found it: it
/// compiled, the checker refused it, and exactly the checks that failed
/// before any proposal still fail.
///
/// `baseline` must be non-empty: with nothing failing visibly, "unchanged" is
/// the state of every proposal that merely satisfied the visible checks, which
/// is the ordinary state of a candidate the hidden check is still refusing.
/// A compile failure is no evidence either way; it is not inert.
pub fn is_inert(baseline: &[String], attempt: &AttemptSignal) -> bool {
    if baseline.is_empty() || attempt.rejection != RejectionReason::CheckRefused {
        return false;
    }
    match &attempt.visible_failing {
        Some(now) => {
            now.len() == baseline.len() && baseline.iter().all(|b| now.iter().any(|n| n == b))
        }
        None => false,
    }
}

/// The run of inert proposals ending the candidate's history. A proposal that
/// did not compile, whose check stopped at a process bound (R2), or that was
/// refused for a capability before it ran (R1b) neither extends nor breaks
/// it: no verdict came back, so nothing was learned about whether the
/// verdict moves. Any other proposal ends it.
pub fn inert_streak(baseline: &[String], attempts: &[AttemptSignal]) -> usize {
    let mut streak = 0;
    for a in attempts.iter().rev() {
        if matches!(
            a.rejection,
            RejectionReason::DidNotCompile
                | RejectionReason::TimedOut
                | RejectionReason::CapabilityDenied
        ) {
            continue;
        }
        if is_inert(baseline, a) {
            streak += 1;
        } else {
            break;
        }
    }
    streak
}

/// Whether a candidate should not be asked again.
pub fn should_stop(limit: usize, baseline: &[String], attempts: &[AttemptSignal]) -> bool {
    limit > 0 && inert_streak(baseline, attempts) >= limit
}

/// Why a run stopped early, for the detail line of the outcome.
pub fn no_common_function_reason(failing: &[String]) -> String {
    format!(
        "no single function is reached by all {} failing visible check(s) ({}); a one-body \
         edit cannot make them all pass, so the defect likely spans several functions, a \
         signature or a type. The file is unchanged and no model was asked",
        failing.len(),
        failing.join(", ")
    )
}

pub fn unmoved_reason(candidate: &str, limit: usize, failing: &[String]) -> String {
    format!(
        "{limit} compiled proposals for `{candidate}` in a row left the failing visible \
         check(s) exactly as they were ({}); the visible verdict is not moving",
        failing.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn att(n: usize, r: RejectionReason, v: Option<&[&str]>) -> AttemptSignal {
        AttemptSignal {
            proposal: n,
            rejection: r,
            visible_failing: v.map(|v| v.iter().map(|s| s.to_string()).collect()),
        }
    }
    fn names(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reach_filter_keeps_rank_order() {
        let ranked = names(&["c", "a", "b"]);
        assert_eq!(
            candidates_in_reach(&ranked, &names(&["a", "c"])),
            names(&["c", "a"])
        );
        assert!(candidates_in_reach(&ranked, &[]).is_empty());
    }

    #[test]
    fn inert_means_same_failing_set_in_any_order() {
        let base = names(&["t1", "t2"]);
        let r = RejectionReason::CheckRefused;
        assert!(is_inert(&base, &att(1, r.clone(), Some(&["t2", "t1"]))));
        assert!(!is_inert(&base, &att(1, r.clone(), Some(&["t1"]))));
        assert!(!is_inert(
            &base,
            &att(1, r.clone(), Some(&["t1", "t2", "t3"]))
        ));
        assert!(!is_inert(&base, &att(1, r.clone(), Some(&[]))));
        assert!(!is_inert(&base, &att(1, r.clone(), None)));
        assert!(!is_inert(
            &base,
            &att(1, RejectionReason::DidNotCompile, None)
        ));
        // Nothing failing visibly to begin with: never inert.
        assert!(!is_inert(&[], &att(1, r, Some(&[]))));
    }

    #[test]
    fn streak_skips_compile_failures_and_ends_on_movement() {
        let base = names(&["t"]);
        let r = RejectionReason::CheckRefused;
        let inert = |n| att(n, r.clone(), Some(&["t"]));
        let moved = |n| att(n, r.clone(), Some(&[]));
        let broke = |n| att(n, RejectionReason::DidNotCompile, None);
        assert_eq!(inert_streak(&base, &[]), 0);
        assert_eq!(inert_streak(&base, &[inert(1), inert(2)]), 2);
        assert_eq!(inert_streak(&base, &[inert(1), broke(2), inert(3)]), 2);
        assert_eq!(inert_streak(&base, &[inert(1), moved(2)]), 0);
        assert_eq!(inert_streak(&base, &[moved(1), inert(2)]), 1);
        assert!(should_stop(2, &base, &[inert(1), inert(2)]));
        assert!(!should_stop(2, &base, &[inert(1)]));
        assert!(!should_stop(0, &base, &[inert(1), inert(2), inert(3)]));
    }

    /// R2: a proposal whose check stopped at a process bound brought back no
    /// verdict, so like a compile failure it neither extends nor breaks the
    /// streak.
    #[test]
    fn streak_skips_timed_out_attempts() {
        let base = names(&["t"]);
        let inert = |n| att(n, RejectionReason::CheckRefused, Some(&["t"]));
        let timed_out = |n| att(n, RejectionReason::TimedOut, None);
        assert_eq!(inert_streak(&base, &[inert(1), timed_out(2), inert(3)]), 2);
        assert_eq!(inert_streak(&base, &[inert(1), timed_out(2)]), 1);
    }

    /// R1b: a proposal refused for a capability never ran a check, so like a
    /// compile failure it neither extends nor breaks the streak.
    #[test]
    fn streak_skips_capability_denied_attempts() {
        let base = names(&["t"]);
        let inert = |n| att(n, RejectionReason::CheckRefused, Some(&["t"]));
        let denied = |n| att(n, RejectionReason::CapabilityDenied, None);
        assert_eq!(inert_streak(&base, &[inert(1), denied(2), inert(3)]), 2);
        assert_eq!(inert_streak(&base, &[inert(1), denied(2)]), 1);
    }
}
