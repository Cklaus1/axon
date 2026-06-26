//! R21 §3.5 — the outcome of a supervised run and its exit-code mapping.
//!
//! Pure. Every supervisor outcome maps to exactly one `Verdict`, and every
//! `Verdict` to exactly one exit code, reusing Axon's carved scheme (6 refine,
//! 7 budget, 8 sandbox/capability) plus 9 (record tamper / replay divergence)
//! and 2 (malformed/usage). Fail-closed outcomes never collapse to a generic
//! error — the reason is always carried.

/// The sealing outcome of a supervised run (or a pre-run denial).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(tag = "kind")]
pub enum Verdict {
    /// The program ran to completion with this i64 result. → exit 0.
    Completed { value: i64 },
    /// The manifest or invocation was malformed (bad input / usage). → exit 2.
    Malformed { reason: String },
    /// A capability/sandbox boundary was refused — either the static gate
    /// (before any run) or a runtime sandbox violation. → exit 8.
    Denied { reason: String, axis: String },
    /// A resource budget axis was exhausted. → exit 7.
    BudgetExhausted { axis: String },
    /// A refinement-type contract was violated at runtime. → exit 6.
    RefineViolation { reason: String },
    /// A stored record failed integrity verification, or a replay diverged
    /// from the recorded run. → exit 9.
    VerifyMismatch { detail: String },
}

impl Verdict {
    /// The process exit code for this verdict (R21 §3.5). A contract: never
    /// collapse a fail-closed outcome into a generic code.
    pub fn exit_code(&self) -> i32 {
        match self {
            Verdict::Completed { .. } => 0,
            Verdict::Malformed { .. } => 2,
            Verdict::RefineViolation { .. } => 6,
            Verdict::BudgetExhausted { .. } => 7,
            Verdict::Denied { .. } => 8,
            Verdict::VerifyMismatch { .. } => 9,
        }
    }

    /// A short, human-legible one-line rendering of the verdict (R21 §5.2:
    /// output is legible, not just an exit code).
    pub fn legible(&self) -> String {
        match self {
            Verdict::Completed { value } => format!("\u{2713} completed (value={value})"),
            Verdict::Malformed { reason } => format!("malformed: {reason}"),
            Verdict::Denied { reason, axis } => {
                format!("\u{26a0} DENIED: {reason} (axis: {axis})")
            }
            Verdict::BudgetExhausted { axis } => format!("budget exhausted: {axis}"),
            Verdict::RefineViolation { reason } => format!("refinement violated: {reason}"),
            Verdict::VerifyMismatch { detail } => format!("\u{2717} verify mismatch: {detail}"),
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
        assert_eq!(
            Verdict::RefineViolation { reason: "x".into() }.exit_code(),
            6
        );
        assert_eq!(
            Verdict::BudgetExhausted {
                axis: "tokens".into()
            }
            .exit_code(),
            7
        );
        assert_eq!(
            Verdict::Denied {
                reason: "x".into(),
                axis: "net".into()
            }
            .exit_code(),
            8
        );
        assert_eq!(
            Verdict::VerifyMismatch { detail: "x".into() }.exit_code(),
            9
        );
    }
}
