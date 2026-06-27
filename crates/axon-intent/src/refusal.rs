//! R22 §3.6 — the refusal taxonomy + its exit-code mapping. Pure.
//!
//! Every way the gateway can refuse to ship a triple maps to exactly one exit
//! code, reusing Axon's carved scheme (3 rejected/human, 5 AI-policy band, 8
//! sandbox/capability) plus 2 (malformed/usage). A refusal NEVER collapses to a
//! generic error — the reason is always carried (fail closed, legibly).

/// Why the gateway refused to produce (or the human rejected) an approved triple.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// The intent file or invocation was malformed (bad input / usage). → exit 2.
    Malformed { reason: String },
    /// Synthesis produced a low-quality / under-specified program that must NOT
    /// be shipped (confidence gate). Carries the reason + score. → exit 5.
    LowConfidence { reason: String, score: u8 },
    /// The synthesized program could exceed the inferred grant — a bug or an
    /// adversarial program; refuse, never ship. → exit 8.
    NotAdmissible { axis: String, reason: String },
    /// Inference would need more authority than the intent's ceiling permits;
    /// the human must explicitly widen the intent. → exit 8.
    GrantExceedsCeiling { axis: String },
    /// The human rejected the resolved (program, grant) at approval. → exit 3.
    Rejected,
}

impl Refusal {
    /// The process exit code for this refusal (R22 §3.6). A contract.
    pub fn exit_code(&self) -> i32 {
        match self {
            Refusal::Malformed { .. } => 2,
            Refusal::Rejected => 3,
            Refusal::LowConfidence { .. } => 5,
            Refusal::NotAdmissible { .. } | Refusal::GrantExceedsCeiling { .. } => 8,
        }
    }

    /// A short, human-legible one-line rendering (R22 §5.2: output is legible,
    /// not just an exit code).
    pub fn legible(&self) -> String {
        match self {
            Refusal::Malformed { reason } => format!("malformed intent: {reason}"),
            Refusal::LowConfidence { reason, score } => {
                format!("REFUSED (low confidence {score}/100): {reason}")
            }
            Refusal::NotAdmissible { axis, reason } => {
                format!("REFUSED (not admissible on {axis}): {reason}")
            }
            Refusal::GrantExceedsCeiling { axis } => {
                format!(
                    "REFUSED: the program needs `{axis}` authority the intent's `## Allowed` ceiling does not permit \u{2014} widen the intent explicitly"
                )
            }
            Refusal::Rejected => "rejected by the operator".to_string(),
        }
    }
}

/// The human's approval decision (R22 §3.5).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
    Approved,
    Rejected,
}

impl Decision {
    pub fn as_str(&self) -> &'static str {
        match self {
            Decision::Approved => "approved",
            Decision::Rejected => "rejected",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_codes_are_the_carved_scheme() {
        assert_eq!(Refusal::Malformed { reason: "x".into() }.exit_code(), 2);
        assert_eq!(Refusal::Rejected.exit_code(), 3);
        assert_eq!(
            Refusal::LowConfidence {
                reason: "x".into(),
                score: 10
            }
            .exit_code(),
            5
        );
        assert_eq!(
            Refusal::NotAdmissible {
                axis: "net".into(),
                reason: "x".into()
            }
            .exit_code(),
            8
        );
        assert_eq!(
            Refusal::GrantExceedsCeiling {
                axis: "fs_read".into()
            }
            .exit_code(),
            8
        );
    }
}
