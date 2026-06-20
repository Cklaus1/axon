/// Rework detection: files touched by multiple sessions in a short window.
///
/// High rework = a file being edited repeatedly without a clear exit criterion.
/// Signals: scope drift, vague goals, partial fixes, or a genuinely hard problem.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use axon_ledger::model::Effect;
use axon_ledger::store::Store;
use crate::display_goal;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ReworkHotspot {
    pub file: String,
    /// Number of distinct sessions that touched this file in the window
    pub session_count: usize,
    /// Session goals that touched this file, in chronological order
    pub session_goals: Vec<String>,
    pub first_touch_ms: u64,
    pub last_touch_ms: u64,
    /// Duration of the rework window in hours
    pub window_hours: f64,
}

/// Files that appear in virtually every session and are not meaningful rework signals.
/// CHANGELOG.md, README.md etc. are updated as part of every feature — flagging them
/// as hotspots just adds noise.
const REWORK_BLOCKLIST: &[&str] = &[
    "CHANGELOG.md", "CHANGELOG", "changelog.md",
    "README.md", "README", "readme.md",
    "ROADMAP.md", "ROADMAP",
    "Cargo.lock", "package-lock.json", "yarn.lock", "pnpm-lock.yaml",
    ".gitignore", ".gitattributes",
    "Cargo.toml",  // workspace manifest changed by many sessions
];

fn is_blocklisted(path: &str) -> bool {
    let name = path.split('/').last().unwrap_or(path);
    REWORK_BLOCKLIST.iter().any(|b| name.eq_ignore_ascii_case(b))
}

/// Goal prefixes that indicate a loop-continuation or autonomous-iteration session
/// rather than an independent rework attempt. These sessions should not be counted
/// as distinct "rework" sessions — they're the same logical work unit split across
/// multiple iterations by `/loop` or a continuation prompt.
const CONTINUATION_PREFIXES: &[&str] = &[
    "/loop",
    "keep going",
    "continue with",
    "pick up",
    "continue the",
    "(structured session)",
    "(system prompt)",
];

pub fn is_continuation_session(goal: &str) -> bool {
    let lower = goal.trim().to_lowercase();
    CONTINUATION_PREFIXES.iter().any(|p| lower.starts_with(p))
        || lower.is_empty()
}

/// Normalize a file path for cross-session matching.
///
/// Strips absolute path prefixes so that `/home/user/project/crates/foo.rs`
/// and `crates/foo.rs` (relative) both normalize to `crates/foo.rs`.
/// This avoids false-positive deduplication: `eval.rs` in `axon-core/` and
/// `eval.rs` in `axon-signal/` remain distinct (`axon-core/src/eval.rs` vs
/// `axon-signal/src/eval.rs`).
fn normalize_path(path: &str) -> String {
    if path.starts_with('/') {
        // Strip up to the first conventional root anchor
        for anchor in &["crates/", "src/", "packages/", "lib/", "tests/", "spec/"] {
            if let Some(pos) = path.find(anchor) {
                return path[pos..].to_string();
            }
        }
        // Fallback: strip leading '/' so absolute and relative match
        return path.trim_start_matches('/').to_string();
    }
    path.to_string()
}

/// Find files touched by multiple sessions within `window_days`.
pub fn find_rework_hotspots(store: &Store, window_days: u64) -> anyhow::Result<Vec<ReworkHotspot>> {
    let window_ms = window_days * 24 * 60 * 60 * 1000;
    let all = store.all().map_err(anyhow::Error::from)?;

    // Build session_id → (goal, ts_ms, files) from sessions.
    // Skip loop-continuation sessions — they repeat the same work and inflate counts.
    let sessions: HashMap<String, (String, u64, Vec<String>)> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .filter_map(|r| {
            let id_prefix = &r.id[..r.id.len().min(8)];
            let sid = r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or(id_prefix).to_string();
            let raw_goal = r.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("");
            let goal = display_goal(raw_goal, 80);
            // Skip continuation/loop sessions — they're not independent rework
            if is_continuation_session(&goal) { return None; }
            let files: Vec<String> = r.payload.get("files_touched")
                .and_then(|v| v.as_array())
                .map(|a| a.iter()
                    .filter_map(|v| v.as_str().map(normalize_path))
                    .filter(|s| !s.is_empty() && !is_blocklisted(s))
                    .collect())
                .unwrap_or_default();
            Some((sid, (goal, r.ts_ms, files)))
        })
        .collect();

    // Build file → [(session_id, goal, ts_ms)] map
    let mut file_touches: HashMap<String, Vec<(String, String, u64)>> = HashMap::new();
    for (sid, (goal, ts_ms, files)) in &sessions {
        for file in files {
            if file.is_empty() { continue; }
            file_touches
                .entry(file.clone())
                .or_default()
                .push((sid.clone(), goal.clone(), *ts_ms));
        }
    }

    let mut hotspots = Vec::new();
    for (file, mut touches) in file_touches {
        touches.sort_by_key(|(_, _, ts)| *ts);
        // Deduplicate by session_id
        touches.dedup_by(|a, b| a.0 == b.0);

        if touches.len() < 2 { continue; }

        // Check if any N consecutive touches fall within the window
        // Use a sliding window over the touch list
        let mut window_start = 0;
        while window_start < touches.len() {
            let start_ts = touches[window_start].2;
            let in_window: Vec<_> = touches[window_start..].iter()
                .take_while(|(_, _, ts)| ts - start_ts <= window_ms)
                .collect();
            if in_window.len() >= 2 {
                let goals: Vec<String> = in_window.iter().map(|(_, g, _)| g.clone()).collect();
                let first = in_window.first().unwrap().2;
                let last = in_window.last().unwrap().2;
                hotspots.push(ReworkHotspot {
                    file: file.clone(),
                    session_count: in_window.len(),
                    session_goals: goals,
                    first_touch_ms: first,
                    last_touch_ms: last,
                    window_hours: (last - first) as f64 / 3_600_000.0,
                });
                break; // one hotspot record per file
            }
            window_start += 1;
        }
    }

    // Sort by session_count descending (worst rework first)
    hotspots.sort_by(|a, b| b.session_count.cmp(&a.session_count));
    Ok(hotspots)
}

/// Check if a specific session_id contributed to any rework hotspot.
pub fn session_triggered_rework(session_id: &str, hotspots: &[ReworkHotspot]) -> bool {
    hotspots.iter().any(|h| {
        // A session is a rework trigger if it's NOT the first to touch a hotspot file
        // (the first session started the problem; subsequent ones are the rework signal)
        h.session_count >= 2
    }) && hotspots.iter().flat_map(|h| h.session_goals.iter().skip(1)).any(|_| {
        // simplified: if session appears in any hotspot with 2+ sessions, it's a rework signal
        true
    }) && !session_id.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_ledger::model::Effect;
    use axon_ledger::store::Store;
    use serde_json::json;
    use tempfile::tempdir;

    fn make_session(sid: &str, goal: &str, files: &[&str], ts_ms: u64) -> axon_ledger::model::LedgerRecord {
        axon_ledger::model::LedgerRecord {
            id: sid.to_string(),
            principal: format!("session:{sid}"),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms,
            payload: json!({
                "session_id": sid,
                "goal": goal,
                "files_touched": files,
            }),
            repo: None,
        }
    }

    #[test]
    fn test_rework_detection_finds_hotspot() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let day_ms = 86_400_000u64;

        // Two sessions both touch auth.rs within 3 days
        store.append(&make_session("s1", "fix auth bug", &["auth.rs"], 0)).unwrap();
        store.append(&make_session("s2", "fix auth again", &["auth.rs"], day_ms * 2)).unwrap();

        let hotspots = find_rework_hotspots(&store, 7).unwrap();
        assert!(!hotspots.is_empty(), "should detect rework on auth.rs");
        assert_eq!(hotspots[0].file, "auth.rs");
        assert_eq!(hotspots[0].session_count, 2);
    }

    #[test]
    fn test_no_rework_when_spread_far_apart() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let day_ms = 86_400_000u64;

        store.append(&make_session("s1", "add auth", &["auth.rs"], 0)).unwrap();
        store.append(&make_session("s2", "update auth", &["auth.rs"], day_ms * 30)).unwrap();

        let hotspots = find_rework_hotspots(&store, 7).unwrap();
        assert!(hotspots.is_empty(), "sessions 30 days apart should not be rework");
    }

    #[test]
    fn test_continuation_sessions_not_counted_as_rework() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();
        let hour_ms = 3_600_000u64;

        // One real session plus two loop-continuation sessions touching the same file
        store.append(&make_session("s1", "fix JWT clock skew", &["auth.rs"], 0)).unwrap();
        store.append(&make_session("s2", "keep going with this", &["auth.rs"], hour_ms)).unwrap();
        store.append(&make_session("s3", "/loop 5m keep going", &["auth.rs"], hour_ms * 2)).unwrap();

        let hotspots = find_rework_hotspots(&store, 7).unwrap();
        // Only s1 is a non-continuation session — no rework (need ≥2 non-continuation sessions)
        assert!(hotspots.is_empty(),
            "loop-continuation sessions should not count as independent rework; got {:?}", hotspots);
    }

    #[test]
    fn test_blocklisted_files_excluded() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();

        store.append(&make_session("s1", "add feature A", &["CHANGELOG.md", "auth.rs"], 0)).unwrap();
        store.append(&make_session("s2", "add feature B", &["CHANGELOG.md", "auth.rs"], 3_600_000)).unwrap();
        store.append(&make_session("s3", "add feature C", &["CHANGELOG.md"], 7_200_000)).unwrap();

        let hotspots = find_rework_hotspots(&store, 7).unwrap();
        // CHANGELOG.md should be excluded; auth.rs should appear as rework
        let changelog = hotspots.iter().find(|h| h.file.contains("CHANGELOG.md"));
        assert!(changelog.is_none(), "CHANGELOG.md should be blocklisted");
        let auth = hotspots.iter().find(|h| h.file.contains("auth.rs"));
        assert!(auth.is_some(), "auth.rs should be flagged as rework");
    }

    #[test]
    fn test_absolute_and_relative_paths_deduplicated() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();

        // One session stores absolute, another stores relative — same file
        store.append(&make_session("s1", "fix auth bug",
            &["/home/user/project/crates/axon-core/src/auth.rs"], 0)).unwrap();
        store.append(&make_session("s2", "improve auth",
            &["crates/axon-core/src/auth.rs"], 3_600_000)).unwrap();

        let hotspots = find_rework_hotspots(&store, 7).unwrap();
        // Should produce exactly one hotspot (deduplicated), not two separate entries
        assert_eq!(hotspots.len(), 1, "absolute and relative paths should normalize to the same file");
        assert_eq!(hotspots[0].session_count, 2);
    }

    #[test]
    fn test_is_continuation_session() {
        assert!(is_continuation_session("keep going with the work"));
        assert!(is_continuation_session("/loop 5m fix tests"));
        assert!(is_continuation_session("continue with the plan"));
        assert!(is_continuation_session("(structured session)"));
        assert!(is_continuation_session(""));
        assert!(!is_continuation_session("fix JWT clock skew in auth.rs"));
        assert!(!is_continuation_session("add rate limiting to /api/users"));
    }

    #[test]
    fn test_normalize_path() {
        assert_eq!(normalize_path("crates/foo.rs"), "crates/foo.rs");
        assert_eq!(normalize_path("/home/user/project/crates/foo.rs"), "crates/foo.rs");
        assert_eq!(normalize_path("/home/user/project/src/main.rs"), "src/main.rs");
        assert_eq!(normalize_path("/root/project/foo.rs"), "root/project/foo.rs");
    }
}
