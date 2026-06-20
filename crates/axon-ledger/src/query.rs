use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::ingest::session::parse_iso_to_ms;
use crate::model::{Effect, LedgerRecord};
use crate::store::Store;

/// Snapshot of ledger state at a given point in time.
#[derive(Serialize, Deserialize, Debug)]
pub struct AsOfResult {
    /// Timestamp used for the snapshot (ms since epoch)
    pub ts_ms: u64,
    /// Human-readable timestamp
    pub ts_iso: String,
    /// Most recent commits at or before `ts_ms` (up to 10)
    pub recent_commits: Vec<LedgerRecord>,
    /// Sessions active at or before `ts_ms` (started before, ended after or unknown)
    pub active_sessions: Vec<LedgerRecord>,
    /// Files being actively worked on across visible sessions
    pub files_in_flight: Vec<String>,
    /// Total commits known at this point
    pub commit_count: usize,
    /// Total sessions known at this point
    pub session_count: usize,
}

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

/// Reconstruct the ledger state at a given point in time.
///
/// Returns what was known/shipped up to `ts_ms`: the most recent commits,
/// any sessions that were active at that moment, and the files in flight.
/// This is the "what did we know then?" query — the key differentiator
/// over `git log`, which only shows the code state, not the decision context.
pub fn as_of(ts_ms: u64, store: &Store) -> Result<AsOfResult> {
    let all = store.all()?;

    // All records visible at ts_ms
    let visible: Vec<&LedgerRecord> = all.iter().filter(|r| r.ts_ms <= ts_ms).collect();

    // Commits up to ts_ms, most recent first (up to 10)
    let mut commits: Vec<LedgerRecord> = visible
        .iter()
        .filter(|r| r.effect == Effect::GitCommit)
        .map(|r| (*r).clone())
        .collect();
    commits.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms));
    let recent_commits = commits.iter().take(10).cloned().collect();
    let commit_count = commits.len();

    // Sessions that started before ts_ms. "Active" = ended after ts_ms, or no
    // end_ts recorded (treat as active if it started within 8h of ts_ms).
    let eight_hours_ms: u64 = 28_800_000;
    let mut active_sessions: Vec<LedgerRecord> = visible
        .iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .filter(|r| {
            let end_ts_ms = r.payload
                .get("end_ts")
                .and_then(|v| v.as_str())
                .and_then(parse_iso_to_ms);
            match end_ts_ms {
                Some(end) => end >= ts_ms,
                // No end_ts: treat as active if session started within 8h
                None => ts_ms.saturating_sub(r.ts_ms) <= eight_hours_ms,
            }
        })
        .map(|r| (*r).clone())
        .collect();
    active_sessions.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms));
    let session_count = visible.iter().filter(|r| r.effect == Effect::AgentSession).count();

    // Files in flight: union of files_touched across active sessions, deduped.
    // Only keep paths that look like real source files: no flags, globs, or shell fragments.
    let valid_extensions = [".rs", ".ax", ".toml", ".md", ".json", ".jsonl", ".sh", ".lock", ".html", ".js", ".ts", ".py"];
    let mut files_set: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for s in &active_sessions {
        if let Some(arr) = s.payload.get("files_touched").and_then(|v| v.as_array()) {
            for f in arr {
                if let Some(path) = f.as_str() {
                    // Skip shell artifacts: flags, globs, bash groupings
                    if path.starts_with('-') || path.starts_with('(')
                        || path.contains('*') || path.contains('+')
                        || path.contains('=') || path.contains('[')
                        || path.contains(' ')
                    {
                        continue;
                    }
                    if !valid_extensions.iter().any(|ext| path.ends_with(ext)) {
                        continue;
                    }
                    // Keep last 2 path components for readability
                    let parts: Vec<&str> = path.rsplitn(3, '/').collect();
                    let label = if parts.len() >= 2 {
                        format!("{}/{}", parts[1], parts[0])
                    } else {
                        path.to_string()
                    };
                    files_set.insert(label);
                }
            }
        }
    }
    let files_in_flight: Vec<String> = files_set.into_iter().take(20).collect();

    // Build a human-readable timestamp from ts_ms
    let ts_iso = ms_to_iso_approx(ts_ms);

    Ok(AsOfResult {
        ts_ms,
        ts_iso,
        recent_commits,
        active_sessions,
        files_in_flight,
        commit_count,
        session_count,
    })
}

/// A single search hit with its context.
#[derive(Serialize, Deserialize, Debug)]
pub struct SearchHit {
    pub record: LedgerRecord,
    /// Which field matched and what the matched text was
    pub matched_field: String,
    pub matched_text: String,
    /// For commit hits: the linked session goal (if any)
    pub session_goal: Option<String>,
}

/// Full-text search across commit messages, session goals, and file paths.
/// Returns hits ranked by recency (most recent first), up to `limit`.
pub fn search(query: &str, store: &Store, limit: usize) -> Result<Vec<SearchHit>> {
    let q_lower = query.to_lowercase();
    let terms: Vec<&str> = q_lower.split_whitespace().collect();
    if terms.is_empty() {
        return Ok(vec![]);
    }

    let all = store.all()?;
    let sessions = store.find_by_effect(&Effect::AgentSession)?;
    let edges = store.find_by_effect(&Effect::AgentEdge)?;

    // Build session-id → goal map for enriching commit hits
    let session_goals: std::collections::HashMap<&str, &str> = sessions
        .iter()
        .filter_map(|s| {
            let id = s.payload.get("session_id").and_then(|v| v.as_str())?;
            let goal = s.payload.get("goal").and_then(|v| v.as_str())?;
            Some((id, goal))
        })
        .collect();

    // Build commit-sha → session-id map via edges
    let commit_to_session: std::collections::HashMap<&str, &str> = edges
        .iter()
        .filter_map(|e| {
            let sha = e.payload.get("commit_sha").and_then(|v| v.as_str())?;
            let sid = e.payload.get("session_id").and_then(|v| v.as_str())?;
            Some((sha, sid))
        })
        .collect();

    let mut hits: Vec<SearchHit> = Vec::new();

    for record in &all {
        // Search commits: message and files
        if record.effect == Effect::GitCommit {
            let msg = record.payload.get("message").and_then(|v| v.as_str()).unwrap_or("");
            let sha = record.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("");
            let files_arr = record.payload.get("files").and_then(|v| v.as_array());

            let msg_match = matches_all(&msg.to_lowercase(), &terms);
            let file_match = files_arr.and_then(|arr| {
                arr.iter()
                    .find_map(|f| f.as_str().filter(|p| matches_all(&p.to_lowercase(), &terms)))
            });

            let (matched_field, matched_text) = if msg_match {
                ("commit.message".to_string(), msg.chars().take(120).collect())
            } else if let Some(fp) = file_match {
                ("commit.file".to_string(), fp.to_string())
            } else {
                continue;
            };

            let session_goal = commit_to_session.get(sha)
                .and_then(|sid| session_goals.get(*sid))
                .map(|g| g.chars().take(120).collect());

            hits.push(SearchHit { record: record.clone(), matched_field, matched_text, session_goal });
        }

        // Search sessions: goal text and files_touched
        if record.effect == Effect::AgentSession {
            let goal = record.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("");
            let files_arr = record.payload.get("files_touched").and_then(|v| v.as_array());

            let goal_match = matches_all(&goal.to_lowercase(), &terms);
            let file_match = files_arr.and_then(|arr| {
                arr.iter()
                    .find_map(|f| f.as_str().filter(|p| matches_all(&p.to_lowercase(), &terms)))
            });

            let (matched_field, matched_text) = if goal_match {
                ("session.goal".to_string(), goal.chars().take(120).collect())
            } else if let Some(fp) = file_match {
                ("session.file".to_string(), fp.to_string())
            } else {
                continue;
            };

            hits.push(SearchHit {
                record: record.clone(),
                matched_field,
                matched_text,
                session_goal: if goal_match { Some(goal.chars().take(120).collect()) } else { None },
            });
        }
    }

    // Sort by recency (newest first), then deduplicate by record id
    hits.sort_by(|a, b| b.record.ts_ms.cmp(&a.record.ts_ms));
    hits.dedup_by_key(|h| h.record.id.clone());
    hits.truncate(limit);
    Ok(hits)
}

/// Returns true if the text contains ALL whitespace-separated terms.
fn matches_all(text: &str, terms: &[&str]) -> bool {
    terms.iter().all(|t| text.contains(t))
}

// ─── history ─────────────────────────────────────────────────────────────────

/// One chapter in a file's history: the AI session that worked on it,
/// the original goal, and the commits it produced that touched this file.
#[derive(Serialize, Deserialize, Debug)]
pub struct FileChapter {
    /// The session that touched this file
    pub session: LedgerRecord,
    /// Resolved session goal text
    pub goal: String,
    /// Commits produced by this session that mention this file
    pub commits: Vec<LedgerRecord>,
    /// Edge confidence for the session→commit links
    pub confidence: Option<f64>,
    /// Human-readable session start time
    pub date: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct HistoryResult {
    /// Normalized file path used for matching
    pub file: String,
    /// Chapters in chronological order (oldest first)
    pub chapters: Vec<FileChapter>,
    pub total_sessions: usize,
    pub total_commits: usize,
}

/// Return the provenance history of a file: which AI sessions shaped it,
/// in what order, with which goals, and which commits each session produced.
///
/// Matching is done on path suffix so "auth/jwt.rs" matches
/// "crates/core/src/auth/jwt.rs", "auth/jwt.rs", and "jwt.rs".
pub fn history(file_path: &str, store: &Store) -> Result<HistoryResult> {
    let all = store.all().map_err(anyhow::Error::from)?;

    // Normalise the query: strip leading ./ and lower-case
    let query = file_path.trim_start_matches("./");
    let query_lower = query.to_lowercase();

    // Helper: does a stored path match the query?
    let file_matches = |stored: &str| -> bool {
        let s = stored.trim_start_matches("./").to_lowercase();
        s == query_lower || s.ends_with(&format!("/{query_lower}")) || s.ends_with(&query_lower)
    };

    // ── 1. Find sessions that touched this file ──────────────────────────────
    let sessions: Vec<&LedgerRecord> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .filter(|r| {
            r.payload.get("files_touched")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).any(|f| file_matches(f)))
                .unwrap_or(false)
        })
        .collect();

    // ── 2. Build session_id → commit list from edges ─────────────────────────
    let mut session_to_edge: std::collections::HashMap<String, &LedgerRecord> =
        std::collections::HashMap::new();
    for r in all.iter().filter(|r| r.effect == Effect::AgentEdge) {
        let sid = r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
        session_to_edge.entry(sid.to_string()).or_insert(r);
    }

    // Build commit sha → record map
    let commit_by_sha: std::collections::HashMap<String, &LedgerRecord> = all.iter()
        .filter(|r| r.effect == Effect::GitCommit)
        .filter_map(|r| {
            r.payload.get("sha").and_then(|v| v.as_str())
                .map(|sha| (sha.to_string(), r))
        })
        .collect();

    // For each session, collect its edges and then the commits that touched our file
    let mut session_commits: std::collections::HashMap<String, Vec<&LedgerRecord>> =
        std::collections::HashMap::new();
    for edge in all.iter().filter(|r| r.effect == Effect::AgentEdge) {
        let sid = edge.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
        let sha = edge.payload.get("commit_sha").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(&commit) = commit_by_sha.get(sha) {
            let files_match = commit.payload.get("files")
                .and_then(|v| v.as_array())
                .map(|arr| arr.iter().filter_map(|v| v.as_str()).any(|f| file_matches(f)))
                .unwrap_or(false);
            if files_match {
                session_commits.entry(sid.to_string()).or_default().push(commit);
            }
        }
    }

    // ── 3. Build chapters ────────────────────────────────────────────────────
    let mut chapters: Vec<FileChapter> = sessions.iter().map(|s| {
        let p = &s.payload;
        let sid = p.get("session_id").and_then(|v| v.as_str()).unwrap_or(&s.id[..s.id.len().min(8)]);
        let goal = p.get("goal").and_then(|v| v.as_str())
            .filter(|g| !g.starts_with('<'))
            .unwrap_or("(no goal recorded)")
            .to_string();
        let commits = session_commits.get(sid).cloned().unwrap_or_default()
            .into_iter().cloned().collect();
        let edge = session_to_edge.get(sid);
        let confidence = edge.and_then(|e| e.payload.get("confidence").and_then(|v| v.as_f64()));
        FileChapter {
            session: (*s).clone(),
            goal,
            commits,
            confidence,
            date: ms_to_iso_approx(s.ts_ms),
        }
    }).collect();

    // Oldest first
    chapters.sort_by_key(|c| c.session.ts_ms);

    let total_commits: usize = chapters.iter().map(|c| c.commits.len()).sum();

    Ok(HistoryResult {
        file: query.to_string(),
        chapters,
        total_sessions: sessions.len(),
        total_commits,
    })
}

fn ms_to_iso_approx(ms: u64) -> String {
    let secs = ms / 1000;
    let total_days = secs / 86400;
    let time_secs = secs % 86400;
    let h = time_secs / 3600;
    let m = (time_secs % 3600) / 60;
    let s = time_secs % 60;
    // Julian Day Number → Gregorian (same algorithm as edge.rs tests)
    let jd = total_days as i64 + 2_440_588;
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

    fn make_session_with_file(sid: &str, goal: &str, file: &str, ts_ms: u64) -> LedgerRecord {
        let payload = json!({
            "session_id": sid,
            "goal": goal,
            "turn_count": 10,
            "files_touched": [file],
        });
        let id = record_id(&format!("session:{sid}"), &Effect::AgentSession, ts_ms, &payload);
        LedgerRecord { id, principal: format!("session:{sid}"), effect: Effect::AgentSession,
            causal_parent: None, ts_ms, payload }
    }

    #[test]
    fn test_history_finds_sessions_by_suffix() {
        let dir = env::temp_dir().join(format!("axon-ledger-hist-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        let s = make_session_with_file("sess1", "fix the auth bug", "src/auth/jwt.rs", 1_000_000);
        store.append(&s).unwrap();

        // Match by basename
        let r = history("jwt.rs", &store).unwrap();
        assert_eq!(r.chapters.len(), 1);
        assert_eq!(r.chapters[0].goal, "fix the auth bug");

        // Match by suffix
        let r2 = history("auth/jwt.rs", &store).unwrap();
        assert_eq!(r2.chapters.len(), 1);
    }

    #[test]
    fn test_history_empty_for_unknown_file() {
        let dir = env::temp_dir().join(format!("axon-ledger-hist2-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let store = Store::open(&dir).unwrap();
        let r = history("nonexistent.rs", &store).unwrap();
        assert_eq!(r.chapters.len(), 0);
        assert_eq!(r.total_sessions, 0);
    }

    #[test]
    fn test_history_chronological_order() {
        let dir = env::temp_dir().join(format!("axon-ledger-hist3-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let mut store = Store::open(&dir).unwrap();

        // Insert in reverse order
        store.append(&make_session_with_file("s2", "second goal", "lib.rs", 2_000_000)).unwrap();
        store.append(&make_session_with_file("s1", "first goal", "lib.rs", 1_000_000)).unwrap();

        let r = history("lib.rs", &store).unwrap();
        assert_eq!(r.chapters.len(), 2);
        assert_eq!(r.chapters[0].goal, "first goal",  "should be oldest first");
        assert_eq!(r.chapters[1].goal, "second goal");
    }
}
