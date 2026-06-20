use sha2::{Digest, Sha256};

use crate::model::Effect;

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

pub fn record_id(
    principal: &str,
    effect: &Effect,
    ts_ms: u64,
    payload: &serde_json::Value,
) -> String {
    let payload_canonical = serde_json::to_string(payload).unwrap_or_default();
    let input = format!(
        "{}|{}|{}|{}",
        principal,
        effect.as_str(),
        ts_ms,
        payload_canonical
    );
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    let result = hasher.finalize();
    hex_encode(&result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_record_id_stable() {
        let payload = json!({"key": "value"});
        let id1 = record_id("git:user@example.com", &Effect::GitCommit, 1000, &payload);
        let id2 = record_id("git:user@example.com", &Effect::GitCommit, 1000, &payload);
        assert_eq!(id1, id2, "record_id must be deterministic");
        assert!(!id1.is_empty(), "record_id must not be empty");
        assert_eq!(id1.len(), 64, "SHA-256 hex is 64 chars");
    }

    #[test]
    fn test_record_id_changes_on_different_principal() {
        let payload = json!({"key": "value"});
        let id1 = record_id("git:alice@example.com", &Effect::GitCommit, 1000, &payload);
        let id2 = record_id("git:bob@example.com", &Effect::GitCommit, 1000, &payload);
        assert_ne!(id1, id2, "Different principals must produce different ids");
    }

    #[test]
    fn test_record_id_changes_on_different_ts() {
        let payload = json!({"key": "value"});
        let id1 = record_id("git:user@example.com", &Effect::GitCommit, 1000, &payload);
        let id2 = record_id("git:user@example.com", &Effect::GitCommit, 2000, &payload);
        assert_ne!(id1, id2, "Different timestamps must produce different ids");
    }

    #[test]
    fn test_record_id_changes_on_different_effect() {
        let payload = json!({"key": "value"});
        let id1 = record_id("agent:session1", &Effect::GitCommit, 1000, &payload);
        let id2 = record_id("agent:session1", &Effect::AgentSession, 1000, &payload);
        assert_ne!(id1, id2, "Different effects must produce different ids");
    }

    #[test]
    fn test_record_id_changes_on_different_payload() {
        let payload1 = json!({"sha": "abc123"});
        let payload2 = json!({"sha": "def456"});
        let id1 = record_id("git:user@example.com", &Effect::GitCommit, 1000, &payload1);
        let id2 = record_id("git:user@example.com", &Effect::GitCommit, 1000, &payload2);
        assert_ne!(id1, id2, "Different payloads must produce different ids");
    }
}
