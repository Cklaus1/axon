/// Prompt pattern library: extract goal structures that reliably produce commits.
///
/// Patterns are derived entirely from ledger data — no LLM required.
/// The key structure is the VERB + OBJECT + (CONTEXT) shape of high-scoring goals.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::score::SessionScore;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct GoalPattern {
    /// The abstract pattern, e.g. "fix [error] in [file]"
    pub pattern: String,
    /// Example goal that matched this pattern
    pub example: String,
    /// Average effectiveness score for sessions matching this pattern
    pub avg_score: f64,
    /// Number of sessions matching this pattern
    pub session_count: usize,
    /// Average commits produced by sessions with this pattern
    pub avg_commits: f64,
}

/// Extract the leading verb from a goal string.
fn extract_verb(goal: &str) -> Option<&str> {
    let verbs = ["fix", "add", "refactor", "migrate", "extract", "remove",
                 "update", "replace", "implement", "delete", "rename", "move",
                 "improve", "clean", "work on", "make"];
    let lower = goal.to_lowercase();
    verbs.iter().find(|&&v| lower.starts_with(v)).copied()
}

/// Classify a goal into a coarse pattern bucket.
pub fn classify_goal_pattern(goal: &str) -> String {
    let lower = goal.to_lowercase();
    let has_file = goal.split_whitespace().any(|w| w.contains('.') || w.contains('/'));
    let has_metric = ["<", ">", "%", "ms", "passes", "test", "error", "rate"]
        .iter().any(|m| lower.contains(m));
    let verb = extract_verb(goal);

    match (verb, has_file, has_metric) {
        (Some(v), true, true)  => format!("{v} [X] in [file] — [metric]"),
        (Some(v), true, false) => format!("{v} [X] in [file]"),
        (Some(v), false, true) => format!("{v} [X] — [metric]"),
        (Some(v), false, false) => format!("{v} [X]"),
        (None, _, _)           => "(no clear verb)".to_string(),
    }
}

/// Build a pattern library from scored sessions.
///
/// Returns patterns sorted by avg_score descending, filtered to min_sessions.
pub fn build_pattern_library(
    scores: &[SessionScore],
    min_sessions: usize,
    min_score: u8,
) -> Vec<GoalPattern> {
    // Bucket sessions by pattern
    let mut buckets: HashMap<String, Vec<&SessionScore>> = HashMap::new();
    for s in scores {
        if s.score >= min_score {
            let pat = classify_goal_pattern(&s.goal);
            buckets.entry(pat).or_default().push(s);
        }
    }

    let mut patterns: Vec<GoalPattern> = buckets
        .into_iter()
        .filter(|(_, sessions)| sessions.len() >= min_sessions)
        .map(|(pattern, sessions)| {
            let avg_score = sessions.iter().map(|s| s.score as f64).sum::<f64>() / sessions.len() as f64;
            let avg_commits = sessions.iter().map(|s| s.commits_linked as f64).sum::<f64>() / sessions.len() as f64;
            // Pick the highest-scoring session as the example
            let example = sessions.iter()
                .max_by_key(|s| s.score)
                .map(|s| s.goal.clone())
                .unwrap_or_default();
            GoalPattern {
                pattern,
                example,
                avg_score,
                session_count: sessions.len(),
                avg_commits,
            }
        })
        .collect();

    patterns.sort_by(|a, b| b.avg_score.partial_cmp(&a.avg_score).unwrap_or(std::cmp::Ordering::Equal));
    patterns
}

/// Patterns that consistently underperformed.
pub fn antipatterns(scores: &[SessionScore], min_sessions: usize) -> Vec<GoalPattern> {
    let mut buckets: HashMap<String, Vec<&SessionScore>> = HashMap::new();
    for s in scores {
        if s.score < 45 {
            let pat = classify_goal_pattern(&s.goal);
            buckets.entry(pat).or_default().push(s);
        }
    }

    let mut patterns: Vec<GoalPattern> = buckets
        .into_iter()
        .filter(|(_, sessions)| sessions.len() >= min_sessions)
        .map(|(pattern, sessions)| {
            let avg_score = sessions.iter().map(|s| s.score as f64).sum::<f64>() / sessions.len() as f64;
            let avg_commits = sessions.iter().map(|s| s.commits_linked as f64).sum::<f64>() / sessions.len() as f64;
            let example = sessions.iter()
                .min_by_key(|s| s.score)
                .map(|s| s.goal.clone())
                .unwrap_or_default();
            GoalPattern { pattern, example, avg_score, session_count: sessions.len(), avg_commits }
        })
        .collect();

    patterns.sort_by(|a, b| a.avg_score.partial_cmp(&b.avg_score).unwrap_or(std::cmp::Ordering::Equal));
    patterns
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_specific_goal() {
        let pat = classify_goal_pattern("fix JWT clock skew in auth/jwt.go — error rate > 2%");
        assert!(pat.contains("fix"), "should contain verb: {pat}");
        assert!(pat.contains("file"), "should detect file ref: {pat}");
        assert!(pat.contains("metric"), "should detect metric: {pat}");
    }

    #[test]
    fn test_classify_vague_goal() {
        let pat = classify_goal_pattern("improve things");
        assert!(pat.contains("improve"), "should extract verb: {pat}");
        assert!(!pat.contains("file"), "no file in vague goal: {pat}");
    }

    #[test]
    fn test_classify_no_verb() {
        let pat = classify_goal_pattern("authentication refactor");
        assert_eq!(pat, "(no clear verb)");
    }

    #[test]
    fn test_build_library_requires_min_sessions() {
        use crate::score::TrainingTier;
        let scores = vec![
            SessionScore {
                session_id: "s1".into(), goal: "fix bug in auth.rs".into(),
                score: 80, label: "good", turns: 10, commits_linked: 2,
                files_touched: 1, goal_clarity: 70, turns_per_commit: 5.0,
                scope_fit: 90, rework_signal: false,
                training_tier: TrainingTier::PositiveSilver,
                engineer: "chris".into(), ts_ms: 0,
            },
        ];
        let lib = build_pattern_library(&scores, 2, 50);
        assert!(lib.is_empty(), "should require min 2 sessions");

        let lib2 = build_pattern_library(&scores, 1, 50);
        assert!(!lib2.is_empty(), "min 1 session should produce patterns");
    }
}
