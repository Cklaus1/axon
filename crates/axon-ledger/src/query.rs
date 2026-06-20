use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ingest::session::parse_iso_to_ms;
use crate::model::{Effect, LedgerRecord};
use crate::store::Store;

#[derive(Serialize, Deserialize, Debug)]
pub struct WhyResult {
    pub commit: LedgerRecord,
    pub agent_session: Option<LedgerRecord>,
    pub edge: Option<LedgerRecord>,
    pub outcomes: Vec<LedgerRecord>,
}

pub fn why(sha: &str, store: &Store) -> Result<WhyResult> {
    // 1. Find GitCommit record where payload["sha"] starts with sha
    let commits = store.find_by_effect(&Effect::GitCommit)?;
    let commit = commits
        .into_iter()
        .find(|r| {
            r.payload
                .get("sha")
                .and_then(|v| v.as_str())
                .map(|s| s.starts_with(sha))
                .unwrap_or(false)
        })
        .ok_or_else(|| anyhow::anyhow!("No commit found with sha prefix: {}", sha))?;

    // 2. Find all AgentEdge records that point to this commit, then pick the best.
    //    "Best" = the edge whose session started closest to (and before) the commit,
    //    constrained to the same 7-hour window used during inference.
    let edges = store.find_by_effect(&Effect::AgentEdge)?;
    let sessions = store.find_by_effect(&Effect::AgentSession)?;

    let commit_ts = commit.ts_ms;
    let max_gap_ms: u64 = 25_200_000; // 7 h

    let candidates: Vec<LedgerRecord> = edges
        .into_iter()
        .filter(|r| {
            r.payload
                .get("commit_sha")
                .and_then(|v| v.as_str())
                .map(|s| s.starts_with(sha))
                .unwrap_or(false)
        })
        .collect();

    // Score each candidate edge by |session_start - commit_ts|, prefer sessions
    // that started before the commit and whose gap is within the inference window.
    let edge = candidates.into_iter().min_by_key(|e| {
        let session_id = e.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
        let session_ts = sessions
            .iter()
            .find(|s| s.payload.get("session_id").and_then(|v| v.as_str()) == Some(session_id))
            .map(|s| {
                // Prefer end_ts over start_ts as the "session activity" anchor
                let end_ts = s.payload.get("end_ts").and_then(|v| v.as_str())
                    .and_then(parse_iso_to_ms)
                    .unwrap_or(s.ts_ms);
                // Use end_ts if it's plausible (within 24h of start), else start
                if end_ts > s.ts_ms && end_ts - s.ts_ms < 86_400_000 { end_ts } else { s.ts_ms }
            })
            .unwrap_or(e.ts_ms);
        // Gap from session to commit; sessions after the commit are penalized heavily
        let gap = if commit_ts >= session_ts {
            commit_ts - session_ts
        } else {
            (session_ts - commit_ts) + max_gap_ms * 10
        };
        gap
    });

    // 3. From edge, find AgentSession
    let agent_session = if let Some(ref e) = edge {
        let session_id = e
            .payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let sessions = store.find_by_effect(&Effect::AgentSession)?;
        sessions.into_iter().find(|r| {
            r.payload
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(|s| s == session_id)
                .unwrap_or(false)
        })
    } else {
        None
    };

    // 4. Find MetricOutcome records with causal_parent == commit.id
    let all = store.all()?;
    let outcomes = all
        .into_iter()
        .filter(|r| {
            r.effect == Effect::MetricOutcome
                && r.causal_parent.as_deref() == Some(commit.id.as_str())
        })
        .collect();

    Ok(WhyResult {
        commit,
        agent_session,
        edge,
        outcomes,
    })
}

pub fn diff(t1_ms: u64, t2_ms: u64, store: &Store) -> Result<Vec<LedgerRecord>> {
    let all = store.all()?;
    Ok(all
        .into_iter()
        .filter(|r| r.ts_ms >= t1_ms && r.ts_ms <= t2_ms)
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::record_id;
    use crate::model::{Effect, LedgerRecord};
    use crate::store::Store;
    use serde_json::json;
    use std::env;

    fn make_commit(sha: &str, ts_ms: u64) -> LedgerRecord {
        let payload = json!({
            "sha": sha,
            "message": "test commit",
            "author": "test@example.com",
            "files": ["src/main.rs"],
        });
        let id = record_id("git:test@example.com", &Effect::GitCommit, ts_ms, &payload);
        LedgerRecord {
            id,
            principal: "git:test@example.com".to_string(),
            effect: Effect::GitCommit,
            causal_parent: None,
            ts_ms,
            payload,
        }
    }

    fn make_session(session_id: &str, ts_ms: u64) -> LedgerRecord {
        let payload = json!({
            "session_id": session_id,
            "file_path": format!("/tmp/{}.jsonl", session_id),
            "start_ts": "",
            "end_ts": "",
            "files_touched": ["src/main.rs"],
            "turn_count": 3,
            "summary": "test",
        });
        let id = record_id(
            &format!("agent:{}", session_id),
            &Effect::AgentSession,
            ts_ms,
            &payload,
        );
        LedgerRecord {
            id,
            principal: format!("agent:{}", session_id),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms,
            payload,
        }
    }

    fn make_edge(session_id: &str, commit_sha: &str, commit_id: &str, ts_ms: u64) -> LedgerRecord {
        let payload = json!({
            "session_id": session_id,
            "commit_sha": commit_sha,
            "commit_record_id": commit_id,
            "confidence": "high",
        });
        let id = record_id("ledger", &Effect::AgentEdge, ts_ms, &payload);
        LedgerRecord {
            id,
            principal: "ledger".to_string(),
            effect: Effect::AgentEdge,
            causal_parent: Some(commit_id.to_string()),
            ts_ms,
            payload,
        }
    }

    #[test]
    fn test_why_finds_all_fields() {
        let dir = env::temp_dir().join(format!("axon-ledger-why-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let ts = 1_000_000_000_000u64;
        let commit = make_commit("deadbeef1234abcd", ts);
        let commit_id = commit.id.clone();
        let session = make_session("my-session-001", ts - 600_000);
        let edge = make_edge("my-session-001", "deadbeef1234abcd", &commit_id, ts);

        store.append(&commit).unwrap();
        store.append(&session).unwrap();
        store.append(&edge).unwrap();

        let result = why("deadbeef", &store).unwrap();

        assert_eq!(
            result
                .commit
                .payload
                .get("sha")
                .and_then(|v| v.as_str()),
            Some("deadbeef1234abcd")
        );
        assert!(result.agent_session.is_some(), "Should find agent session");
        assert!(result.edge.is_some(), "Should find edge");
        assert!(result.outcomes.is_empty(), "No outcomes in this test");
    }

    #[test]
    fn test_why_returns_error_for_unknown_sha() {
        let dir = env::temp_dir().join(format!("axon-ledger-why-err-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir).unwrap();

        let result = why("nonexistentsha", &store);
        assert!(result.is_err(), "Should return error for unknown sha");
    }

    #[test]
    fn test_diff_filters_by_time_window() {
        let dir = env::temp_dir().join(format!("axon-ledger-diff-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let t1 = 1_000_000u64;
        let t2 = 2_000_000u64;

        let c1 = make_commit("sha000001", t1 - 100); // before window
        let c2 = make_commit("sha000002", t1 + 100); // inside window
        let c3 = make_commit("sha000003", t2 + 100); // after window

        store.append(&c1).unwrap();
        store.append(&c2).unwrap();
        store.append(&c3).unwrap();

        let result = diff(t1, t2, &store).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].payload.get("sha").and_then(|v| v.as_str()),
            Some("sha000002")
        );
    }
}
