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
