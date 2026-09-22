use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LedgerRecord {
    pub id: String,
    pub principal: String,
    pub effect: Effect,
    pub causal_parent: Option<String>,
    pub ts_ms: u64,
    pub payload: serde_json::Value,
    /// Optional repository name tag, e.g. "api", "frontend", "infra".
    /// None means "untagged" (single-repo ledger or pre-v1 records).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    /// WHO ACTUALLY WROTE THIS RECORD, as distinct from `principal`, which is
    /// who the record is ABOUT.
    ///
    /// `principal` is a SUBJECT and is legitimately caller-chosen: a CI
    /// account ingesting many engineers' sessions must be able to say whose
    /// session it is (`ingest session --engineer`). That made it forgeable —
    /// a member could write a record attributed to another principal, which
    /// then appeared inside the victim's RBAC view and nobody else's.
    ///
    /// The fix is not to restrict the subject — that would break the normal
    /// ingest path — but to record the ACTOR alongside it. This field is
    /// stamped by `Store::append` from the OS-authenticated identity and
    /// OVERWRITES anything a caller supplies, so it cannot be forged through
    /// any write path.
    ///
    /// `Option` and `skip_serializing_if` for the same reason `repo` has
    /// them: records written before this field existed stay readable, and
    /// nothing computes a digest over the struct (the record id hashes
    /// `principal|effect|ts_ms|payload` only), so adding it breaks no
    /// existing record and no integrity check.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recorded_by: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    GitCommit,
    AgentSession,
    MetricOutcome,
    AgentEdge,
}

impl Effect {
    pub fn as_str(&self) -> &'static str {
        match self {
            Effect::GitCommit => "git_commit",
            Effect::AgentSession => "agent_session",
            Effect::MetricOutcome => "metric_outcome",
            Effect::AgentEdge => "agent_edge",
        }
    }
}

/// The first `n` CHARACTERS of an id, for display. Never panics.
///
/// The idiom this replaces was `&id[..8]`, which panics two ways on data that
/// deserialises perfectly well:
///
///   * an id shorter than 8 bytes  -> "end byte index 8 is out of bounds"
///   * a multi-byte char spanning byte 8 -> "is not a char boundary"
///
/// Both were reachable, and one of them was worse than a crash in a CLI: the
/// axon-signal dashboard calls this path per request, so a SINGLE request
/// against a ledger holding one such record killed the server outright (exit
/// 101, connection refused thereafter) — and that endpoint requires no
/// credentials. Measured on ids "r1" and "€€€".
///
/// Byte slicing was also the wrong operation for the intent. "First 8" of a
/// display id means characters, not bytes, so this truncates on char
/// boundaries and returns the whole string when it is shorter.
pub fn short_id(id: &str, n: usize) -> &str {
    match id.char_indices().nth(n) {
        Some((byte_idx, _)) => &id[..byte_idx],
        None => id,
    }
}

#[cfg(test)]
mod short_id_tests {
    use super::short_id;

    #[test]
    fn short_id_never_panics_on_the_inputs_that_used_to_crash() {
        // Shorter than the requested length: the whole string, no panic.
        assert_eq!(short_id("r1", 8), "r1");
        assert_eq!(short_id("", 8), "");
        // A multi-byte char spanning byte 8. `&s[..8]` panics here with
        // "not a char boundary"; this truncates on a char boundary instead.
        assert_eq!(
            short_id("\u{20ac}\u{20ac}\u{20ac}", 8),
            "\u{20ac}\u{20ac}\u{20ac}"
        );
        assert_eq!(short_id("\u{20ac}\u{20ac}\u{20ac}", 2), "\u{20ac}\u{20ac}");
        // The ordinary case still truncates to the requested length.
        assert_eq!(short_id("0123456789abcdef", 8), "01234567");
        assert_eq!(short_id("01234567", 8), "01234567");
    }
}
