use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LedgerRecord {
    pub id: String,
    pub principal: String,
    pub effect: Effect,
    pub causal_parent: Option<String>,
    pub ts_ms: u64,
    pub payload: serde_json::Value,
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
