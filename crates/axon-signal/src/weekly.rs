/// Weekly AI coding effectiveness report.
///
/// Generates a human-readable or JSON report covering a time window.
/// Requires no LLM calls — all analysis is from ledger + signal data.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::display_goal;
use crate::patterns::{antipatterns, build_pattern_library, GoalPattern};
use crate::rework::{find_rework_hotspots, ReworkHotspot};
use crate::score::{score_sessions, SessionScore};
use axon_ledger::store::Store;

#[derive(Serialize, Deserialize, Debug)]
pub struct EngineerSummary {
    pub engineer: String,
    pub sessions: usize,
    pub commits: usize,
    pub avg_score: f64,
    pub avg_turns_per_commit: f64,
    pub top_goal: String,
}

#[derive(Serialize, Debug)]
pub struct WeeklyReport {
    pub from_ms: u64,
    pub to_ms: u64,
    pub total_sessions: usize,
    pub total_commits: usize,
    pub coverage_pct: u64,
    pub avg_score: f64,
    pub prev_avg_score: Option<f64>,
    pub engineers: Vec<EngineerSummary>,
    pub top_sessions: Vec<SessionScore>,
    pub rework_hotspots: Vec<ReworkHotspot>,
    pub top_patterns: Vec<GoalPattern>,
    pub antipatterns: Vec<GoalPattern>,
    pub recommendations: Vec<String>,
}

pub fn generate_weekly(
    store: &Store,
    from_ms: u64,
    to_ms: u64,
    window_days: u64,
) -> anyhow::Result<WeeklyReport> {
    let all_scores = score_sessions(store)?;

    // Filter to window
    let window_scores: Vec<SessionScore> = all_scores.iter()
        .filter(|s| s.ts_ms >= from_ms && s.ts_ms <= to_ms)
        .cloned()
        .collect();

    // Previous window (same duration, immediately before)
    let duration = to_ms.saturating_sub(from_ms);
    let prev_scores: Vec<SessionScore> = all_scores.iter()
        .filter(|s| s.ts_ms >= from_ms.saturating_sub(duration) && s.ts_ms < from_ms)
        .cloned()
        .collect();

    let total_commits: usize = window_scores.iter().map(|s| s.commits_linked).sum();
    let avg_score = if window_scores.is_empty() {
        0.0
    } else {
        window_scores.iter().map(|s| s.score as f64).sum::<f64>() / window_scores.len() as f64
    };
    let prev_avg_score = if prev_scores.is_empty() {
        None
    } else {
        Some(prev_scores.iter().map(|s| s.score as f64).sum::<f64>() / prev_scores.len() as f64)
    };

    // Per-engineer breakdown
    let mut by_engineer: HashMap<String, Vec<&SessionScore>> = HashMap::new();
    for s in &window_scores {
        by_engineer.entry(s.engineer.clone()).or_default().push(s);
    }
    let mut engineers: Vec<EngineerSummary> = by_engineer.iter().map(|(eng, sessions)| {
        let avg_score = sessions.iter().map(|s| s.score as f64).sum::<f64>() / sessions.len() as f64;
        let total_commits: usize = sessions.iter().map(|s| s.commits_linked).sum();
        let total_turns: u64 = sessions.iter().map(|s| s.turns).sum();
        let avg_tpc = if total_commits > 0 { total_turns as f64 / total_commits as f64 } else { total_turns as f64 };
        let top_goal = sessions.iter().max_by_key(|s| s.score).map(|s| s.goal.clone()).unwrap_or_default();
        EngineerSummary {
            engineer: eng.clone(),
            sessions: sessions.len(),
            commits: total_commits,
            avg_score,
            avg_turns_per_commit: avg_tpc,
            top_goal,
        }
    }).collect();
    engineers.sort_by(|a, b| b.avg_score.partial_cmp(&a.avg_score).unwrap_or(std::cmp::Ordering::Equal));

    // Top 3 sessions this week
    let mut top_sessions = window_scores.clone();
    top_sessions.sort_by(|a, b| b.score.cmp(&a.score));
    top_sessions.truncate(3);

    let hotspots = find_rework_hotspots(store, window_days)?;
    let patterns = build_pattern_library(&all_scores, 2, 70);
    let anti = antipatterns(&all_scores, 2);

    // Generate recommendations
    let recommendations = generate_recommendations(
        &window_scores,
        &engineers,
        &hotspots,
        &anti,
    );

    // Coverage: from ledger stats
    let all = store.all().map_err(anyhow::Error::from)?;
    use axon_ledger::model::Effect;
    let git_count = all.iter().filter(|r| r.effect == Effect::GitCommit).count();
    let linked: std::collections::HashSet<String> = all.iter()
        .filter(|r| r.effect == Effect::AgentEdge)
        .filter_map(|r| r.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
        .collect();
    let coverage_pct = if git_count > 0 { (linked.len() * 100 / git_count) as u64 } else { 0 };

    Ok(WeeklyReport {
        from_ms,
        to_ms,
        total_sessions: window_scores.len(),
        total_commits,
        coverage_pct,
        avg_score,
        prev_avg_score,
        engineers,
        top_sessions,
        rework_hotspots: hotspots.into_iter().take(3).collect(),
        top_patterns: patterns.into_iter().take(5).collect(),
        antipatterns: anti.into_iter().take(3).collect(),
        recommendations,
    })
}

fn generate_recommendations(
    scores: &[SessionScore],
    engineers: &[EngineerSummary],
    hotspots: &[ReworkHotspot],
    anti: &[GoalPattern],
) -> Vec<String> {
    let mut recs = Vec::new();

    // 1. Per-engineer: lowest scorer with vague goals
    if let Some(low) = engineers.iter().min_by(|a, b| {
        a.avg_score.partial_cmp(&b.avg_score).unwrap_or(std::cmp::Ordering::Equal)
    }) {
        if low.avg_score < 55.0 {
            // Find their worst goal
            let vague = scores.iter()
                .filter(|s| s.engineer == low.engineer && s.score < 50)
                .map(|s| s.goal.as_str())
                .next()
                .unwrap_or("(vague goal)");
            recs.push(format!(
                "{}: Try scoping goals to one file or function. \
                 e.g. instead of {:?}, try \"fix [specific error] in [specific file]\".",
                low.engineer, vague
            ));
        }
    }

    // 2. Rework hotspot recommendation
    if let Some(hot) = hotspots.first() {
        if hot.session_count >= 3 {
            recs.push(format!(
                "{} was touched by {} sessions in {:.0}h — rework hotspot. \
                 Suggest one dedicated session: \"audit all [file] edge cases, \
                 write a test for each one that previously failed\".",
                hot.file, hot.session_count, hot.window_hours
            ));
        }
    }

    // 3. Loop opportunity: many short repeated turns
    let repeated_pattern_sessions = scores.iter()
        .filter(|s| s.turns > 30 && s.commits_linked == 0)
        .count();
    if repeated_pattern_sessions >= 2 {
        recs.push(format!(
            "{} sessions ran 30+ turns with no commits. \
             Consider: /loop 5m \"run tests, fix first failure, commit if green\".",
            repeated_pattern_sessions
        ));
    }

    // 4. Anti-pattern warning
    if let Some(anti) = anti.first() {
        recs.push(format!(
            "Goal pattern {:?} averaged {:.0}/100 effectiveness. \
             Add a file reference and a measurable outcome to improve results.",
            anti.pattern, anti.avg_score
        ));
    }

    recs
}

/// Render the report as a human-readable string.
pub fn render_text(r: &WeeklyReport) -> String {
    use std::fmt::Write;
    let mut out = String::new();

    let sep = "━".repeat(50);
    let _ = writeln!(out, "{sep}");
    let _ = writeln!(out, "axon-signal  ·  Weekly AI Coding Report");
    let _ = writeln!(out, "{sep}");
    let _ = writeln!(out);

    // Team summary
    let trend = match r.prev_avg_score {
        Some(p) if r.avg_score > p + 2.0 => format!(" (↑ {:.0} from last week)", r.avg_score - p),
        Some(p) if r.avg_score < p - 2.0 => format!(" (↓ {:.0} from last week)", p - r.avg_score),
        Some(_) => " (→ stable)".to_string(),
        None => String::new(),
    };

    let _ = writeln!(out, "TEAM VELOCITY");
    let _ = writeln!(out, "  Sessions: {}  ·  Commits: {}  ·  Coverage: {}%",
        r.total_sessions, r.total_commits, r.coverage_pct);
    let _ = writeln!(out, "  Avg effectiveness: {:.0}{}", r.avg_score, trend);
    let _ = writeln!(out);

    // Per-engineer
    let _ = writeln!(out, "ENGINEER BREAKDOWN");
    let star = r.engineers.iter().max_by(|a, b| {
        a.avg_score.partial_cmp(&b.avg_score).unwrap_or(std::cmp::Ordering::Equal)
    }).map(|e| e.engineer.clone()).unwrap_or_default();
    for e in &r.engineers {
        let bar = "█".repeat((e.avg_score / 10.0) as usize) +
                  &"░".repeat(10usize.saturating_sub((e.avg_score / 10.0) as usize));
        let flag = if e.engineer == star { "  ★" } else if e.avg_score < 50.0 { "  ←" } else { "" };
        let _ = writeln!(out, "  {:<12} {}  {:.0}  {} sessions  {} commits{}",
            e.engineer, bar, e.avg_score, e.sessions, e.commits, flag);
    }
    let _ = writeln!(out);

    // Top sessions
    if !r.top_sessions.is_empty() {
        let _ = writeln!(out, "TOP SESSIONS THIS WEEK");
        for s in &r.top_sessions {
            let goal = display_goal(&s.goal, 60);
            let _ = writeln!(out, "  ★ {:?}", goal);
            let _ = writeln!(out, "    score {}  ·  {} turns  ·  {} commits  ·  {}",
                s.score, s.turns, s.commits_linked, s.engineer);
        }
        let _ = writeln!(out);
    }

    // What worked
    if !r.top_patterns.is_empty() {
        let _ = writeln!(out, "WHAT WORKED (top goal patterns)");
        for p in r.top_patterns.iter().take(3) {
            let _ = writeln!(out, "  {:?}  →  avg {:.0}/100, {:.1} commits/session",
                p.pattern, p.avg_score, p.avg_commits);
        }
        let _ = writeln!(out);
    }

    // Rework hotspots
    if !r.rework_hotspots.is_empty() {
        let _ = writeln!(out, "REWORK HOTSPOTS");
        for h in &r.rework_hotspots {
            let _ = writeln!(out, "  ⚠ {}  ({} sessions in {:.0}h)", h.file, h.session_count, h.window_hours);
            for g in h.session_goals.iter().take(3) {
                let _ = writeln!(out, "    → {:?}", g);
            }
        }
        let _ = writeln!(out);
    }

    // Recommendations
    if !r.recommendations.is_empty() {
        let _ = writeln!(out, "RECOMMENDATIONS");
        for (i, rec) in r.recommendations.iter().enumerate() {
            let _ = writeln!(out, "  {}. {}", i + 1, rec);
        }
        let _ = writeln!(out);
    }

    let _ = writeln!(out, "{sep}");
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_empty_store_report() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let report = generate_weekly(&store, 0, u64::MAX, 7).unwrap();
        assert_eq!(report.total_sessions, 0);
        assert_eq!(report.total_commits, 0);
    }

    #[test]
    fn test_render_text_non_empty() {
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let report = generate_weekly(&store, 0, u64::MAX, 7).unwrap();
        let text = render_text(&report);
        assert!(text.contains("axon-signal"));
        assert!(text.contains("TEAM VELOCITY"));
    }
}
