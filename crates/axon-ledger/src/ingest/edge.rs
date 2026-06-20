use anyhow::Result;
use serde_json::json;

use crate::hash::record_id;
use crate::ingest::session::parse_iso_to_ms;
use crate::model::{Effect, LedgerRecord};
use crate::store::Store;

pub fn infer_edges(store: &mut Store) -> Result<usize> {
    let sessions = store.find_by_effect(&Effect::AgentSession)?;
    let commits = store.find_by_effect(&Effect::GitCommit)?;
    let existing_edges = store.find_by_effect(&Effect::AgentEdge)?;

    // Build a set of existing (session_id, commit_sha) pairs for dedup
    let existing_pairs: std::collections::HashSet<(String, String)> = existing_edges
        .iter()
        .filter_map(|e| {
            let sid = e
                .payload
                .get("session_id")
                .and_then(|v| v.as_str())
                .map(String::from)?;
            let sha = e
                .payload
                .get("commit_sha")
                .and_then(|v| v.as_str())
                .map(String::from)?;
            Some((sid, sha))
        })
        .collect();

    let mut added = 0;
    let mut new_edges: Vec<LedgerRecord> = Vec::new();

    for session in &sessions {
        let session_id = match session
            .payload
            .get("session_id")
            .and_then(|v| v.as_str())
        {
            Some(s) => s.to_string(),
            None => continue,
        };

        let files_touched: Vec<String> = session
            .payload
            .get("files_touched")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        let session_start_ms = session.ts_ms;
        let end_ts_ms = session
            .payload
            .get("end_ts")
            .and_then(|v| v.as_str())
            .and_then(parse_iso_to_ms)
            .unwrap_or(session_start_ms);

        let window_start = session_start_ms.saturating_sub(3_600_000); // 1h before
        let window_end = end_ts_ms + 21_600_000; // 6h after end

        for commit in &commits {
            let commit_sha = match commit.payload.get("sha").and_then(|v| v.as_str()) {
                Some(s) => s.to_string(),
                None => continue,
            };

            // Dedup check
            if existing_pairs.contains(&(session_id.clone(), commit_sha.clone())) {
                continue;
            }

            // Time window check
            if commit.ts_ms < window_start || commit.ts_ms > window_end {
                continue;
            }

            // File overlap check
            let commit_files: Vec<String> = commit
                .payload
                .get("files")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();

            // Count commit files covered by the session (not session files matching commit —
            // that can exceed commit_files.len() via basename collisions giving ratio > 1).
            let covered_commit_files = commit_files
                .iter()
                .filter(|cf| files_touched.iter().any(|sf| files_overlap(sf, cf)))
                .count();

            if covered_commit_files == 0 {
                continue;
            }

            // Confidence = file_coverage_ratio × time_decay
            // file_coverage_ratio: fraction of the commit's files the session touched (0..1)
            let file_ratio = if commit_files.is_empty() {
                0.5
            } else {
                covered_commit_files as f64 / commit_files.len() as f64
            };
            // time_decay: 1.0 when commit is right at session end, decays to 0.1 at window edge (7h)
            let gap_ms = if commit.ts_ms >= session_start_ms {
                commit.ts_ms - session_start_ms
            } else {
                session_start_ms - commit.ts_ms
            };
            let decay = 1.0_f64 - 0.9 * (gap_ms as f64 / 25_200_000.0_f64).min(1.0);
            let confidence = (file_ratio * decay * 100.0).round() / 100.0;

            let payload = json!({
                "session_id": session_id,
                "commit_sha": commit_sha,
                "session_record_id": session.id,
                "commit_record_id": commit.id,
                "confidence": confidence,
                "overlap_count": covered_commit_files,
            });

            let ts_ms = commit.ts_ms;
            let principal = "ledger".to_string();
            let id = record_id(&principal, &Effect::AgentEdge, ts_ms, &payload);

            let edge = LedgerRecord {
                id,
                principal,
                effect: Effect::AgentEdge,
                causal_parent: Some(commit.id.clone()),
                ts_ms,
                payload,
                repo: commit.repo.clone(),
            };

            new_edges.push(edge);
            added += 1;
        }
    }

    for edge in new_edges {
        store.append(&edge)?;
    }

    Ok(added)
}

/// Check if two file paths refer to the same file (basename match or exact match).
fn files_overlap(a: &str, b: &str) -> bool {
    if a == b {
        return true;
    }
    // Compare basenames
    let a_base = a.rsplit('/').next().unwrap_or(a);
    let b_base = b.rsplit('/').next().unwrap_or(b);
    if a_base == b_base && !a_base.is_empty() {
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Store;
    use serde_json::json;
    use std::env;

    fn make_session_record(id: &str, files: Vec<&str>, start_ms: u64, end_ms: u64) -> LedgerRecord {
        let payload = json!({
            "session_id": id,
            "file_path": format!("/tmp/{}.jsonl", id),
            "start_ts": "",
            "end_ts": ms_to_iso(end_ms),
            "files_touched": files,
            "turn_count": 5,
            "summary": "test session",
        });
        let rid = record_id(&format!("agent:{}", id), &Effect::AgentSession, start_ms, &payload);
        LedgerRecord {
            id: rid,
            principal: format!("agent:{}", id),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms: start_ms,
            payload,
            repo: None,
        }
    }

    fn make_commit_record(sha: &str, files: Vec<&str>, ts_ms: u64) -> LedgerRecord {
        let payload = json!({
            "sha": sha,
            "message": "test commit",
            "author": "test@example.com",
            "files": files,
        });
        let rid = record_id("git:test@example.com", &Effect::GitCommit, ts_ms, &payload);
        LedgerRecord {
            id: rid,
            principal: "git:test@example.com".to_string(),
            effect: Effect::GitCommit,
            causal_parent: None,
            ts_ms,
            payload,
            repo: None,
        }
    }

    fn ms_to_iso(ms: u64) -> String {
        // Convert ms since epoch to a proper ISO 8601 string that round-trips
        let secs = ms / 1000;
        // Use simple day/time decomposition
        let total_days = secs / 86400;
        let time_secs = secs % 86400;
        let h = time_secs / 3600;
        let m = (time_secs % 3600) / 60;
        let s = time_secs % 60;

        // Convert days since epoch to year/month/day (Gregorian calendar)
        // Julian Day Number of 1970-01-01 is 2440588
        let jd = total_days as i64 + 2440588;
        let a = jd + 32044;
        let b = (4 * a + 3) / 146097;
        let c = a - (146097 * b) / 4;
        let d = (4 * c + 3) / 1461;
        let e = c - (1461 * d) / 4;
        let m_raw = (5 * e + 2) / 153;
        let day = e - (153 * m_raw + 2) / 5 + 1;
        let month = m_raw + 3 - 12 * (m_raw / 10);
        let year = 100 * b + d - 4800 + m_raw / 10;

        format!("{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z", year, month, day, h, m, s)
    }

    #[test]
    fn test_infer_edges_creates_edge_for_overlapping_files() {
        let dir = env::temp_dir().join(format!("axon-ledger-edge-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let base_ms = 1_000_000_000_000u64; // some time
        let session = make_session_record(
            "sess-abc",
            vec!["src/main.rs", "Cargo.toml"],
            base_ms,
            base_ms + 3_600_000,
        );
        let commit = make_commit_record(
            "deadbeef1234",
            vec!["src/main.rs"],
            base_ms + 600_000, // 10 min after session start
        );

        store.append(&session).unwrap();
        store.append(&commit).unwrap();

        let added = infer_edges(&mut store).unwrap();
        assert_eq!(added, 1, "should have inferred 1 edge");

        let edges = store.find_by_effect(&Effect::AgentEdge).unwrap();
        assert_eq!(edges.len(), 1);
        assert_eq!(
            edges[0]
                .payload
                .get("session_id")
                .and_then(|v| v.as_str()),
            Some("sess-abc")
        );
        assert_eq!(
            edges[0]
                .payload
                .get("commit_sha")
                .and_then(|v| v.as_str()),
            Some("deadbeef1234")
        );
    }

    #[test]
    fn test_infer_edges_no_overlap_no_edge() {
        let dir = env::temp_dir().join(format!("axon-ledger-no-edge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let base_ms = 1_000_000_000_000u64;
        let session = make_session_record(
            "sess-xyz",
            vec!["src/lib.rs"],
            base_ms,
            base_ms + 3_600_000,
        );
        let commit = make_commit_record(
            "cafebabe5678",
            vec!["src/main.rs"], // different file
            base_ms + 600_000,
        );

        store.append(&session).unwrap();
        store.append(&commit).unwrap();

        let added = infer_edges(&mut store).unwrap();
        assert_eq!(added, 0, "no overlap means no edge");
    }

    #[test]
    fn test_infer_edges_dedup() {
        let dir = env::temp_dir().join(format!("axon-ledger-dedup-edge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let base_ms = 1_000_000_000_000u64;
        let session = make_session_record(
            "sess-dedup",
            vec!["src/main.rs"],
            base_ms,
            base_ms + 3_600_000,
        );
        let commit = make_commit_record("aabbccdd", vec!["src/main.rs"], base_ms + 600_000);

        store.append(&session).unwrap();
        store.append(&commit).unwrap();

        let added1 = infer_edges(&mut store).unwrap();
        assert_eq!(added1, 1);

        let added2 = infer_edges(&mut store).unwrap();
        assert_eq!(added2, 0, "second run should not duplicate the edge");
    }
}
