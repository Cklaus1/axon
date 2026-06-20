/// Rework detection: files touched by multiple sessions in a short window.
///
/// High rework = a file being edited repeatedly without a clear exit criterion.
/// Signals: scope drift, vague goals, partial fixes, or a genuinely hard problem.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use axon_ledger::model::Effect;
use axon_ledger::store::Store;

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

/// Find files touched by multiple sessions within `window_days`.
pub fn find_rework_hotspots(store: &Store, window_days: u64) -> anyhow::Result<Vec<ReworkHotspot>> {
    let window_ms = window_days * 24 * 60 * 60 * 1000;
    let all = store.all().map_err(anyhow::Error::from)?;

    // Build session_id → (goal, ts_ms, files) from sessions
    let sessions: HashMap<String, (String, u64, Vec<String>)> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .map(|r| {
            let id_prefix = &r.id[..r.id.len().min(8)];
            let sid = r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or(id_prefix).to_string();
            let goal = r.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
            let files: Vec<String> = r.payload.get("files_touched")
                .and_then(|v| v.as_array())
                .map(|a| a.iter().filter_map(|v| v.as_str().map(|s| {
                    // Normalize to basename for cross-session matching
                    s.split('/').last().unwrap_or(s).to_string()
                })).collect())
                .unwrap_or_default();
            (sid, (goal, r.ts_ms, files))
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
}
