//! R21 §3.5 + R27 §5.3 — the outcome of a supervised run and its exit-code mapping.
//!
//! Pure. Every supervisor outcome maps to exactly one `Verdict`, and every
//! `Verdict` to exactly one exit code, reusing Axon's carved scheme:
//!   0 ok, 2 usage/malformed, 4 halted (R27 kill-switch), 6 refine, 7 budget,
//!   8 sandbox/capability, 9 resource-bound (R27), 10 coalition-bound (R27),
//!   11 record tamper / replay divergence.
//! Fail-closed outcomes never collapse to a generic error — the reason is always
//! carried. Exit codes 4/9/10 are R27-new (§5.3 "do not collapse").

/// The sealing outcome of a supervised run (or a pre-run denial).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Verdict {
    /// The program ran to completion with this i64 result. → exit 0.
    Completed { value: i64 },
    /// The manifest or invocation was malformed (bad input / usage). → exit 2.
    Malformed { reason: String },
    /// R27: the supervisor kill-switch was tripped; the contained run was stopped.
    /// → exit 4 (`HALTED_EXIT_CODE`). Reuses the interpreter's carved code.
    Halted { reason: String },
    /// A refinement-type contract was violated at runtime. → exit 6.
    RefineViolation { reason: String },
    /// A resource budget axis was exhausted. → exit 7.
    BudgetExhausted { axis: String },
    /// A capability/sandbox boundary was refused — either the static gate
    /// (before any run) or a runtime sandbox violation. → exit 8.
    Denied { reason: String, axis: String },
    /// R27: a resource-acquisition carve exceeded the principal's carved cap.
    /// → exit 9 (`RESOURCE_BOUND_EXIT_CODE`).
    ResourceBound { axis: String, used: u64, cap: u64 },
    /// R27: the coalition rollup / quorum power exceeded the per-coalition ceiling.
    /// → exit 10 (`COALITION_BOUND_EXIT_CODE`).
    CoalitionBound { axis: String, rollup: u64, ceiling: u64 },
    /// A stored record failed integrity verification, or a replay diverged
    /// from the recorded run. → exit 11. (Moved from 9 to free it for R27.)
    VerifyMismatch { detail: String },
}

impl Verdict {
    /// The process exit code for this verdict (R21 §3.5 + R27 §5.3). A contract:
    /// never collapse a fail-closed outcome into a generic code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Verdict::Completed { .. } => 0,
            Verdict::Malformed { .. } => 2,
            Verdict::Halted { .. } => 4,
            Verdict::RefineViolation { .. } => 6,
            Verdict::BudgetExhausted { .. } => 7,
            Verdict::Denied { .. } => 8,
            Verdict::ResourceBound { .. } => 9,
            Verdict::CoalitionBound { .. } => 10,
            Verdict::VerifyMismatch { .. } => 11,
        }
    }

    /// A short, human-legible one-line rendering of the verdict (R21 §5.2:
    /// output is legible, not just an exit code).
    pub fn legible(&self) -> String {
        match self {
            Verdict::Completed { value } => format!("\u{2713} completed (value={value})"),
            Verdict::Malformed { reason } => format!("malformed: {reason}"),
            Verdict::Halted { reason } => format!("\u{1f6d1} HALTED: {reason} (kill-switch tripped)"),
            Verdict::Denied { reason, axis } => {
                format!("\u{26a0} DENIED: {reason} (axis: {axis})")
            }
            Verdict::BudgetExhausted { axis } => format!("budget exhausted: {axis}"),
            Verdict::RefineViolation { reason } => format!("refinement violated: {reason}"),
            Verdict::ResourceBound { axis, used, cap } => {
                format!("\u{26a0} RESOURCE BOUND: {axis} used={used} cap={cap}")
            }
            Verdict::CoalitionBound { axis, rollup, ceiling } => {
                format!("\u{26a0} COALITION BOUND: {axis} rollup={rollup} ceiling={ceiling}")
            }
            Verdict::VerifyMismatch { detail } => format!("\u{2717} TAMPERED: {detail}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_the_carved_scheme() {
        assert_eq!(Verdict::Completed { value: 0 }.exit_code(), 0);
        assert_eq!(Verdict::Malformed { reason: "x".into() }.exit_code(), 2);
        assert_eq!(Verdict::Halted { reason: "x".into() }.exit_code(), 4);
        assert_eq!(
            Verdict::RefineViolation { reason: "x".into() }.exit_code(),
            6
        );
        assert_eq!(
            Verdict::BudgetExhausted { axis: "tokens".into() }.exit_code(),
            7
        );
        assert_eq!(
            Verdict::Denied { reason: "x".into(), axis: "net".into() }.exit_code(),
            8
        );
        assert_eq!(
            Verdict::ResourceBound { axis: "budget".into(), used: 100, cap: 100 }.exit_code(),
            9
        );
        assert_eq!(
            Verdict::CoalitionBound { axis: "compute".into(), rollup: 50, ceiling: 50 }.exit_code(),
            10
        );
        assert_eq!(
            Verdict::VerifyMismatch { detail: "x".into() }.exit_code(),
            11
        );
    }
}
