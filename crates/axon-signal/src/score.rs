/// Session effectiveness scoring.
///
/// Scores are computed entirely from ledger data — no LLM calls.
/// Range: 0–100. See SIGNAL_PRODUCT.md for full methodology.
use serde::{Deserialize, Serialize};

use axon_ledger::model::{Effect, LedgerRecord};
use axon_ledger::store::Store;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionScore {
    pub session_id: String,
    pub goal: String,
    pub score: u8,
    pub label: &'static str,
    pub turns: u64,
    pub commits_linked: usize,
    pub files_touched: usize,
    pub goal_clarity: u8,
    pub turns_per_commit: f64,
    pub scope_fit: u8,
    pub rework_signal: bool,
    pub training_tier: TrainingTier,
    pub engineer: String,
    pub ts_ms: u64,
}

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum TrainingTier {
    /// score ≥ 85, commits ≥ 1, no rework — primary training examples
    PositiveGold,
    /// score 65–84, commits ≥ 1 — training with lower weight
    PositiveSilver,
    /// score < 40, 0 commits OR rework triggered — contrastive training (DPO)
    Negative,
    /// score 40–64, ambiguous — hold out
    Filtered,
}

impl TrainingTier {
    pub fn label(&self) -> &'static str {
        match self {
            Self::PositiveGold => "positive_gold",
            Self::PositiveSilver => "positive_silver",
            Self::Negative => "negative",
            Self::Filtered => "filtered",
        }
    }
}

fn score_label(score: u8) -> &'static str {
    match score {
        85..=100 => "excellent",
        65..=84  => "good",
        45..=64  => "fair",
        25..=44  => "poor",
        _        => "low_signal",
    }
}

/// Score goal clarity without LLM. Returns 0–100.
///
/// Rules:
///   Base 40 for any non-empty goal text
///   +20 if contains a filename or module path indicator (. or / in a word)
///   +20 if contains a measurable outcome (<, >, %, ms, passes, fails, error)
///   +10 if verb is specific (fix, add, refactor, migrate, extract, remove, update)
///   -20 if under 5 words
///   -30 if under 3 words or missing
pub fn score_goal_clarity(goal: &str) -> u8 {
    let goal = goal.trim();
    if goal.is_empty() || goal.starts_with('<') {
        return 0;
    }

    let words: Vec<&str> = goal.split_whitespace().collect();
    let lower = goal.to_lowercase();

    let mut score: i32 = 40;

    // File or module reference
    if words.iter().any(|w| w.contains('.') || w.contains('/')) {
        score += 20;
    }

    // Measurable outcome signal
    let measurable = ["<", ">", "%", "ms", "passes", "fails", "drops", "below", "above",
                      "error rate", "latency", "test", "0.", "1.", "2.", "3."];
    if measurable.iter().any(|m| lower.contains(m)) {
        score += 20;
    }

    // Specific verb
    let specific_verbs = ["fix", "add", "refactor", "migrate", "extract", "remove",
                          "update", "replace", "implement", "delete", "rename", "move",
                          "complete", "finish", "build", "write", "test", "debug",
                          "wire", "land", "ship", "deploy", "integrate", "extend",
                          "pick up", "continue", "investigate"];
    if specific_verbs.iter().any(|v| lower.starts_with(v) || lower.contains(&format!(" {v} "))) {
        score += 10;
    }

    // Penalize vague or short goals
    if words.len() < 3 {
        score -= 30;
    } else if words.len() < 5 {
        score -= 20;
    }

    // Penalize known-vague openers
    let vague = ["improve", "clean", "work on", "look at", "fix stuff", "fix things",
                 "make better", "update things", "various", "misc"];
    if vague.iter().any(|v| lower.starts_with(v)) {
        score -= 15;
    }

    score.clamp(0, 100) as u8
}

/// Score turns efficiency. Returns 0–100.
/// Sweet spot: 8–20 turns/commit. Penalize above that.
fn score_turns_efficiency(turns: u64, commits: usize) -> u8 {
    if commits == 0 {
        // No commits at all — worst case, but still give partial credit for short sessions
        return if turns < 10 { 30 } else if turns < 30 { 15 } else { 0 };
    }
    let ratio = turns as f64 / commits as f64;
    let score = if ratio <= 8.0 {
        100
    } else if ratio <= 20.0 {
        // Linear from 100 to 75
        (100.0 - (ratio - 8.0) / 12.0 * 25.0) as i32
    } else if ratio <= 50.0 {
        // Linear from 75 to 30
        (75.0 - (ratio - 20.0) / 30.0 * 45.0) as i32
    } else {
        10
    };
    score.clamp(0, 100) as u8
}

/// Score scope fit. Returns 0–100.
/// Few focused files relative to goal breadth = good.
fn score_scope_fit(files: usize, goal_words: usize) -> u8 {
    // Heuristic: expect ~2 files per goal word up to a limit
    let expected_max = (goal_words * 3).max(4).min(20);
    if files == 0 {
        return 50; // unknown
    }
    if files <= expected_max {
        100
    } else {
        let over = files - expected_max;
        (100i32 - (over as i32 * 8)).clamp(0, 100) as u8
    }
}

fn training_tier(score: u8, commits: usize, rework: bool) -> TrainingTier {
    // Rework always = Negative regardless of score (something went wrong)
    if rework {
        return TrainingTier::Negative;
    }
    if score >= 85 && commits >= 1 {
        TrainingTier::PositiveGold
    } else if score >= 65 && commits >= 1 {
        TrainingTier::PositiveSilver
    } else if score < 40 || commits == 0 {
        TrainingTier::Negative
    } else {
        TrainingTier::Filtered
    }
}

/// Score all agent sessions in the store.
pub fn score_sessions(store: &Store) -> anyhow::Result<Vec<SessionScore>> {
    let all = store.all().map_err(anyhow::Error::from)?;

    // Build session→commits map from edges
    let mut session_commits: std::collections::HashMap<String, Vec<String>> =
        std::collections::HashMap::new();
    for r in all.iter().filter(|r| r.effect == Effect::AgentEdge) {
        let sid = r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let sha = r.payload.get("commit_sha").and_then(|v| v.as_str()).unwrap_or("").to_string();
        if !sid.is_empty() && !sha.is_empty() {
            session_commits.entry(sid).or_default().push(sha);
        }
    }

    let sessions: Vec<&LedgerRecord> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .collect();

    let mut scores = Vec::new();
    for s in sessions {
        let p = &s.payload;
        let session_id = p.get("session_id").and_then(|v| v.as_str()).unwrap_or(&s.id[..8]).to_string();
        let goal = p.get("goal").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let turns = p.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
        let files: Vec<String> = p.get("files_touched")
            .and_then(|v| v.as_array())
            .map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect())
            .unwrap_or_default();
        let engineer = s.principal
            .trim_start_matches("session:")
            .trim_start_matches("agent:")
            .to_string();

        let commits_linked = session_commits.get(&session_id).map(|v| v.len()).unwrap_or(0);
        let goal_clarity = score_goal_clarity(&goal);
        let turns_eff = score_turns_efficiency(turns, commits_linked);
        let scope_fit = score_scope_fit(files.len(), goal.split_whitespace().count());
        let rework_signal = false; // populated by rework module

        // Weighted composite
        let composite = (
            goal_clarity as f64 * 0.30 +
            turns_eff as f64 * 0.25 +
            scope_fit as f64 * 0.20 +
            if !rework_signal { 100.0 } else { 0.0 } * 0.15 +
            // commit lag: simplified — if commits > 0, full points
            if commits_linked > 0 { 100.0 } else { 20.0 } * 0.10
        ).round() as u8;

        let tier = training_tier(composite, commits_linked, rework_signal);

        scores.push(SessionScore {
            session_id,
            goal,
            score: composite,
            label: score_label(composite),
            turns,
            commits_linked,
            files_touched: files.len(),
            goal_clarity,
            turns_per_commit: if commits_linked > 0 { turns as f64 / commits_linked as f64 } else { turns as f64 },
            scope_fit,
            rework_signal,
            training_tier: tier,
            engineer,
            ts_ms: s.ts_ms,
        });
    }

    scores.sort_by(|a, b| b.ts_ms.cmp(&a.ts_ms));
    Ok(scores)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_ledger::hash::record_id;
    use serde_json::json;

    #[test]
    fn test_goal_clarity_specific() {
        let score = score_goal_clarity("fix JWT clock skew in auth/jwt.go — reproduces when device time > 5min off");
        assert!(score >= 80, "specific goal should score high, got {score}");
    }

    #[test]
    fn test_goal_clarity_vague() {
        let score = score_goal_clarity("improve things");
        assert!(score <= 30, "vague goal should score low, got {score}");
    }

    #[test]
    fn test_goal_clarity_empty() {
        assert_eq!(score_goal_clarity(""), 0);
        assert_eq!(score_goal_clarity("<local-command-caveat>"), 0);
    }

    #[test]
    fn test_goal_clarity_with_file() {
        let score = score_goal_clarity("refactor auth/middleware.rs — extract token validation");
        assert!(score >= 70, "goal with file ref should score well, got {score}");
    }

    #[test]
    fn test_turns_efficiency_ideal() {
        assert_eq!(score_turns_efficiency(12, 2), 100); // 6 turns/commit — ideal
    }

    #[test]
    fn test_turns_efficiency_no_commits() {
        let score = score_turns_efficiency(80, 0);
        assert!(score < 20, "no commits should score very low, got {score}");
    }

    #[test]
    fn test_turns_efficiency_bloated() {
        let score = score_turns_efficiency(100, 1); // 100 turns/commit
        assert!(score <= 20, "100 turns/commit should score low, got {score}");
    }

    #[test]
    fn test_training_tier_gold() {
        assert_eq!(training_tier(90, 2, false), TrainingTier::PositiveGold);
    }

    #[test]
    fn test_training_tier_negative() {
        assert_eq!(training_tier(25, 0, false), TrainingTier::Negative);
    }

    #[test]
    fn test_training_tier_rework() {
        // Even high score + rework = negative (rework means something went wrong)
        assert_eq!(training_tier(85, 3, true), TrainingTier::Negative);
    }

    #[test]
    fn test_training_tier_serializes_snake_case() {
        let gold = TrainingTier::PositiveGold;
        let silver = TrainingTier::PositiveSilver;
        let neg = TrainingTier::Negative;
        let filt = TrainingTier::Filtered;
        assert_eq!(serde_json::to_string(&gold).unwrap(), "\"positive_gold\"");
        assert_eq!(serde_json::to_string(&silver).unwrap(), "\"positive_silver\"");
        assert_eq!(serde_json::to_string(&neg).unwrap(), "\"negative\"");
        assert_eq!(serde_json::to_string(&filt).unwrap(), "\"filtered\"");
    }

    /// Integration: score_sessions produces scores from a minimal store,
    /// and the MetricOutcome write-back is visible via query::why().
    #[test]
    fn test_score_writeback_visible_in_why() {
        use axon_ledger::query::why;
        use axon_ledger::store::Store;
        use tempfile::tempdir;

        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path()).unwrap();

        let ts = 3_000_000_000_000u64;

        // Minimal session record
        let session_payload = json!({
            "session_id": "int-test-session-1",
            "goal": "fix JWT clock skew in auth/jwt.go — reproduces when device time > 5min off",
            "files_touched": ["auth/jwt.go", "auth/jwt_test.go"],
            "turn_count": 12,
            "start_ts": "",
            "end_ts": "",
        });
        let session_id = record_id("agent:int-test-session-1", &Effect::AgentSession, ts - 300_000, &session_payload);
        let session = LedgerRecord {
            id: session_id,
            principal: "agent:int-test-session-1".to_string(),
            effect: Effect::AgentSession,
            causal_parent: None,
            ts_ms: ts - 300_000,
            payload: session_payload,
            repo: None,
        };

        // Commit record
        let commit_sha = "aabbcc112233";
        let commit_payload = json!({
            "sha": commit_sha,
            "message": "fix: JWT clock skew tolerance",
            "author": "test@example.com",
            "files": ["auth/jwt.go", "auth/jwt_test.go"],
        });
        let commit_id = record_id("git:test@example.com", &Effect::GitCommit, ts, &commit_payload);
        let commit = LedgerRecord {
            id: commit_id.clone(),
            principal: "git:test@example.com".to_string(),
            effect: Effect::GitCommit,
            causal_parent: None,
            ts_ms: ts,
            payload: commit_payload,
            repo: None,
        };

        // Edge linking session → commit
        let edge_payload = json!({
            "session_id": "int-test-session-1",
            "commit_sha": commit_sha,
            "commit_record_id": commit_id,
            "confidence": "high",
        });
        let edge_id = record_id("ledger", &Effect::AgentEdge, ts, &edge_payload);
        let edge = LedgerRecord {
            id: edge_id,
            principal: "ledger".to_string(),
            effect: Effect::AgentEdge,
            causal_parent: Some(commit_id.clone()),
            ts_ms: ts,
            payload: edge_payload,
            repo: None,
        };

        store.append(&session).unwrap();
        store.append(&commit).unwrap();
        store.append(&edge).unwrap();

        // Score sessions
        let scores = score_sessions(&store).unwrap();
        assert!(!scores.is_empty(), "should produce at least one score");
        let s = scores.iter().find(|s| s.session_id == "int-test-session-1").unwrap();
        assert!(s.score >= 60, "well-scoped goal with commits should score ≥ 60, got {}", s.score);

        // Simulate score --ingest: write MetricOutcome with session_id in payload, causal_parent: None
        let outcome_payload = json!({
            "session_id": "int-test-session-1",
            "score": s.score,
            "training_tier": s.training_tier.label(),
            "source": "axon-signal",
        });
        let outcome_id = record_id(
            "signal:session:int-test-session-1",
            &Effect::MetricOutcome,
            ts + 1,
            &outcome_payload,
        );
        let outcome = LedgerRecord {
            id: outcome_id,
            principal: "signal:test@example.com".to_string(),
            effect: Effect::MetricOutcome,
            causal_parent: None,  // intentionally not linked to commit
            ts_ms: ts + 1,
            payload: outcome_payload,
            repo: None,
        };
        store.append(&outcome).unwrap();

        // Verify why() surfaces the score via session_id lookup
        let result = why(commit_sha, &store).unwrap();
        assert_eq!(result.outcomes.len(), 1, "why() should find the MetricOutcome via session_id");
        assert_eq!(
            result.outcomes[0].payload.get("score").and_then(|v| v.as_u64()).unwrap_or(0),
            s.score as u64
        );
    }
}
