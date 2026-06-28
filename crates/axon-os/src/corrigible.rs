//! R27 §2 — Corrigible glue: latch + ledger + coalition → verdict.
//!
//! PURE core. Maps each fail-closed R27 outcome to its exit code (§5.3).
//!
//! Exit codes: 4 = HALTED (kill-switch), 9 = RESOURCE_BOUND, 10 = COALITION_BOUND.

use crate::coalition::CoalitionBound;
use crate::latch::LatchState;
use crate::ledger::ResourceBound;
use crate::verdict::Verdict;

pub const HALTED_EXIT_CODE: i32 = 4;
pub const RESOURCE_BOUND_EXIT_CODE: i32 = 9;
pub const COALITION_BOUND_EXIT_CODE: i32 = 10;

/// Poll-before-progress gate: if the latch is Tripped, return a Halted verdict.
/// Fail-closed: the caller must treat channel-error as Tripped before calling.
pub fn check_kill(state: LatchState, reason: &str) -> Option<Verdict> {
    match state {
        LatchState::Tripped => Some(Verdict::Halted { reason: reason.to_string() }),
        LatchState::Clear => None,
    }
}

pub fn resource_bound_verdict(rb: ResourceBound) -> Verdict {
    Verdict::ResourceBound { axis: rb.axis, used: rb.used, cap: rb.cap }
}

pub fn coalition_bound_verdict(cb: CoalitionBound) -> Verdict {
    Verdict::CoalitionBound { axis: cb.axis, rollup: cb.rollup, ceiling: cb.ceiling }
}

/// R27 TCB addendum: the four R27 enforcement modules are part of the TCB.
/// This constant is folded into the `axtcb1:` digest (smt.rs) so a tampered
/// enforcement binary is detected, not just a tampered record.
pub const R27_TCB_ADDENDUM: &str = concat!(
    "R27-latch:clear-trip-one-way-fail-closed",
    "\x1f",
    "R27-ledger:carve-saturating-append-only",
    "\x1f",
    "R27-coalition:lineage-ceiling-quorum-power",
    "\x1f",
    "R27-corrigible:check_kill-resource_bound-coalition_bound",
);

/// A6 assertion: the four R27 TCB modules are in the addendum string.
/// Used by `acc_a6_kill_below_model_fail_closed`.
pub fn r27_tcb_modules_present() -> bool {
    R27_TCB_ADDENDUM.contains("R27-latch")
        && R27_TCB_ADDENDUM.contains("R27-ledger")
        && R27_TCB_ADDENDUM.contains("R27-coalition")
        && R27_TCB_ADDENDUM.contains("R27-corrigible")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latch::LatchState;
    use crate::ledger::ResourceBound;
    use crate::coalition::CoalitionBound;

    #[test]
    fn kill_below_model_maps_tripped_to_halted_verdict() {
        let v = check_kill(LatchState::Tripped, "operator shutdown");
        assert!(v.is_some());
        let verd = v.unwrap();
        assert_eq!(verd.exit_code(), HALTED_EXIT_CODE);
        assert!(matches!(verd, Verdict::Halted { .. }));
    }

    #[test]
    fn clear_latch_produces_no_verdict() {
        assert!(check_kill(LatchState::Clear, "").is_none());
    }

    #[test]
    fn resource_bound_maps_to_exit_9() {
        let rb = ResourceBound { axis: "budget".into(), used: 100, cap: 100, attempted: 1 };
        let v = resource_bound_verdict(rb);
        assert_eq!(v.exit_code(), RESOURCE_BOUND_EXIT_CODE);
    }

    #[test]
    fn coalition_bound_maps_to_exit_10() {
        let cb = CoalitionBound { axis: "compute".into(), rollup: 100, ceiling: 100, attempted: 10, member_count: 3 };
        let v = coalition_bound_verdict(cb);
        assert_eq!(v.exit_code(), COALITION_BOUND_EXIT_CODE);
    }

    #[test]
    fn r27_tcb_addendum_covers_all_four_modules() {
        assert!(r27_tcb_modules_present(), "all four R27 TCB modules must be in the addendum");
    }
}
