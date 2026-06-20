/// Team benchmark — compare the team's effectiveness to industry percentile tiers.
///
/// In the absence of external baseline data we use empirically-motivated tiers
/// derived from the score formula (goal clarity 0-100, turns-per-commit, scope fit,
/// rework absence, commit lag). These tiers will be updated with real percentile
/// data as axon-signal adoption grows; they are clearly labeled as estimates.
///
/// Tiers
/// ──────────────────────────────────────────────────
///  Platinum  ≥ 80   top ~10%  — specific goals, tight loops, low rework
///  Gold      65–79  top ~25%  — good goals, mostly efficient sessions
///  Silver    50–64  median    — room to improve: goal clarity or scope drift
///  Bronze    35–49  bottom half — vague goals, high turn counts or no commits
///  Developing < 35  — sessions often produce no commits or trigger rework
/// ──────────────────────────────────────────────────
///
/// The benchmark also compares per-dimension medians so teams know which lever
/// to pull (goal clarity, turns/commit, scope fit, rework rate).
use serde::{Deserialize, Serialize};

use axon_ledger::store::Store;

use crate::score::{score_sessions, SessionScore};

/// Estimated industry percentile breakpoints (based on score formula analysis).
/// These are advisory — see the caveats field in BenchmarkReport.
pub const PLATINUM_THRESHOLD: f64 = 80.0;
pub const GOLD_THRESHOLD: f64 = 65.0;
pub const SILVER_THRESHOLD: f64 = 50.0;
pub const BRONZE_THRESHOLD: f64 = 35.0;

#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum BenchmarkTier {
    Platinum,
    Gold,
    Silver,
    Bronze,
    Developing,
}

impl BenchmarkTier {
    pub fn from_score(score: f64) -> Self {
        if score >= PLATINUM_THRESHOLD { BenchmarkTier::Platinum }
        else if score >= GOLD_THRESHOLD { BenchmarkTier::Gold }
        else if score >= SILVER_THRESHOLD { BenchmarkTier::Silver }
        else if score >= BRONZE_THRESHOLD { BenchmarkTier::Bronze }
        else { BenchmarkTier::Developing }
    }

    pub fn label(&self) -> &'static str {
        match self {
            BenchmarkTier::Platinum  => "Platinum",
            BenchmarkTier::Gold      => "Gold",
            BenchmarkTier::Silver    => "Silver",
            BenchmarkTier::Bronze    => "Bronze",
            BenchmarkTier::Developing=> "Developing",
        }
    }

    /// Approximate industry percentile for a team at this tier.
    pub fn percentile_estimate(&self) -> &'static str {
        match self {
            BenchmarkTier::Platinum  => "top ~10%",
            BenchmarkTier::Gold      => "top ~25%",
            BenchmarkTier::Silver    => "median (~50th percentile)",
            BenchmarkTier::Bronze    => "bottom ~40%",
            BenchmarkTier::Developing=> "bottom ~20%",
        }
    }

    /// Next tier up (None if already Platinum).
    pub fn next(&self) -> Option<BenchmarkTier> {
        match self {
            BenchmarkTier::Developing=> Some(BenchmarkTier::Bronze),
            BenchmarkTier::Bronze    => Some(BenchmarkTier::Silver),
            BenchmarkTier::Silver    => Some(BenchmarkTier::Gold),
            BenchmarkTier::Gold      => Some(BenchmarkTier::Platinum),
            BenchmarkTier::Platinum  => None,
        }
    }

    /// Points needed to reach the next tier.
    pub fn points_to_next(&self, current_avg: f64) -> Option<f64> {
        let target = match self {
            BenchmarkTier::Developing=> BRONZE_THRESHOLD,
            BenchmarkTier::Bronze    => SILVER_THRESHOLD,
            BenchmarkTier::Silver    => GOLD_THRESHOLD,
            BenchmarkTier::Gold      => PLATINUM_THRESHOLD,
            BenchmarkTier::Platinum  => return None,
        };
        Some((target - current_avg).max(0.0))
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct DimensionBenchmark {
    pub name: String,
    pub team_value: f64,
    /// Estimated industry median for this dimension (advisory).
    pub industry_median: f64,
    pub unit: String,
    /// true = higher is better; false = lower is better
    pub higher_is_better: bool,
    pub gap: f64,
    pub recommendation: String,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EngineerBenchmark {
    pub engineer: String,
    pub avg_score: f64,
    pub tier: BenchmarkTier,
    pub sessions: usize,
    pub percentile_within_team: u8,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BenchmarkReport {
    pub team_avg: f64,
    pub team_tier: BenchmarkTier,
    pub points_to_next_tier: Option<f64>,
    pub next_tier: Option<BenchmarkTier>,
    pub sessions_analyzed: usize,
    pub engineers_analyzed: usize,
    pub dimensions: Vec<DimensionBenchmark>,
    pub engineers: Vec<EngineerBenchmark>,
    /// Ordered list of highest-leverage improvements.
    pub top_recommendations: Vec<String>,
    /// Reminder that industry baselines are synthetic estimates.
    pub caveats: String,
}

pub fn compute_benchmark(store: &Store, days: Option<u64>) -> anyhow::Result<BenchmarkReport> {
    let mut scores: Vec<SessionScore> = score_sessions(store)?;

    if let Some(d) = days {
        let cutoff = now_ms().saturating_sub(d * 86_400_000);
        scores.retain(|s| s.ts_ms >= cutoff);
    }

    let n = scores.len();
    if n == 0 {
        return Ok(BenchmarkReport {
            team_avg: 0.0,
            team_tier: BenchmarkTier::Developing,
            points_to_next_tier: Some(BRONZE_THRESHOLD),
            next_tier: Some(BenchmarkTier::Bronze),
            sessions_analyzed: 0,
            engineers_analyzed: 0,
            dimensions: vec![],
            engineers: vec![],
            top_recommendations: vec!["Ingest more sessions to generate benchmark data.".to_string()],
            caveats: CAVEATS.to_string(),
        });
    }

    // Team average score
    let team_avg = scores.iter().map(|s| s.score as f64).sum::<f64>() / n as f64;
    let team_tier = BenchmarkTier::from_score(team_avg);
    let next_tier = team_tier.next();
    let points_to_next = team_tier.points_to_next(team_avg);

    // Per-dimension medians
    let goal_clarity_avg = scores.iter().map(|s| s.goal_clarity as f64).sum::<f64>() / n as f64;
    let turns_per_commit_avg = {
        let sessions_with_commits: Vec<f64> = scores.iter()
            .filter(|s| s.commits_linked > 0)
            .map(|s| s.turns_per_commit)
            .collect();
        if sessions_with_commits.is_empty() { 0.0 }
        else { sessions_with_commits.iter().sum::<f64>() / sessions_with_commits.len() as f64 }
    };
    let rework_rate = scores.iter().filter(|s| s.rework_signal).count() as f64 / n as f64 * 100.0;
    let commit_rate = scores.iter().filter(|s| s.commits_linked > 0).count() as f64 / n as f64 * 100.0;

    // Industry medians (estimated from score formula analysis)
    // goal_clarity: median ~55 (40 base + occasional file ref or verb)
    // turns_per_commit: median ~22 turns/commit
    // rework_rate: median ~18% of sessions trigger rework
    // commit_rate: median ~70% of sessions produce ≥1 commit
    let dimensions = vec![
        DimensionBenchmark {
            name: "Goal Clarity".to_string(),
            team_value: round1(goal_clarity_avg),
            industry_median: 55.0,
            unit: "/100".to_string(),
            higher_is_better: true,
            gap: round1(goal_clarity_avg - 55.0),
            recommendation: goal_clarity_recommendation(goal_clarity_avg),
        },
        DimensionBenchmark {
            name: "Turns per Commit".to_string(),
            team_value: round1(turns_per_commit_avg),
            industry_median: 22.0,
            unit: "turns".to_string(),
            higher_is_better: false,
            gap: round1(22.0 - turns_per_commit_avg),
            recommendation: turns_recommendation(turns_per_commit_avg),
        },
        DimensionBenchmark {
            name: "Rework Rate".to_string(),
            team_value: round1(rework_rate),
            industry_median: 18.0,
            unit: "%".to_string(),
            higher_is_better: false,
            gap: round1(18.0 - rework_rate),
            recommendation: rework_recommendation(rework_rate),
        },
        DimensionBenchmark {
            name: "Commit Rate".to_string(),
            team_value: round1(commit_rate),
            industry_median: 70.0,
            unit: "%".to_string(),
            higher_is_better: true,
            gap: round1(commit_rate - 70.0),
            recommendation: commit_rate_recommendation(commit_rate),
        },
    ];

    // Top recommendations: rank dimensions by impact potential
    let mut recs: Vec<(f64, String)> = vec![];
    // Goal clarity is 30% weight in score — biggest lever
    if goal_clarity_avg < 55.0 {
        recs.push((55.0 - goal_clarity_avg, format!(
            "Improve goal clarity ({:.0}/100 vs median 55): add file refs and measurable outcomes to session goals.",
            goal_clarity_avg
        )));
    }
    if turns_per_commit_avg > 30.0 {
        recs.push((turns_per_commit_avg - 22.0, format!(
            "Reduce turns/commit ({:.0} vs median 22): scope sessions to one file or one function.",
            turns_per_commit_avg
        )));
    }
    if rework_rate > 25.0 {
        recs.push((rework_rate - 18.0, format!(
            "Cut rework rate ({:.0}% vs median 18%): define exit criteria upfront — 'success = tests pass'.",
            rework_rate
        )));
    }
    if commit_rate < 60.0 {
        recs.push((70.0 - commit_rate, format!(
            "Lift commit rate ({:.0}% vs median 70%): close sessions with an explicit commit even for partial progress.",
            commit_rate
        )));
    }
    recs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let top_recommendations: Vec<String> = if recs.is_empty() {
        vec!["Team is performing above industry medians on all dimensions — maintain cadence.".to_string()]
    } else {
        recs.into_iter().map(|(_, r)| r).take(3).collect()
    };

    // Per-engineer breakdown with intra-team percentile
    let mut by_eng: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
    for s in &scores {
        by_eng.entry(s.engineer.clone()).or_default().push(s.score);
    }
    let mut eng_avgs: Vec<(String, f64)> = by_eng.iter().map(|(eng, v)| {
        let avg = v.iter().map(|&s| s as f64).sum::<f64>() / v.len() as f64;
        (eng.clone(), avg)
    }).collect();
    eng_avgs.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let n_engs = eng_avgs.len();
    let engineers: Vec<EngineerBenchmark> = eng_avgs.iter().enumerate().map(|(i, (eng, avg))| {
        let pct = if n_engs == 1 { 50u8 } else {
            ((n_engs - 1 - i) * 100 / (n_engs - 1)) as u8
        };
        EngineerBenchmark {
            engineer: eng.clone(),
            avg_score: round1(*avg),
            tier: BenchmarkTier::from_score(*avg),
            sessions: by_eng[eng].len(),
            percentile_within_team: pct,
        }
    }).collect();

    Ok(BenchmarkReport {
        team_avg: round1(team_avg),
        team_tier,
        points_to_next_tier: points_to_next.map(round1),
        next_tier,
        sessions_analyzed: n,
        engineers_analyzed: n_engs,
        dimensions,
        engineers,
        top_recommendations,
        caveats: CAVEATS.to_string(),
    })
}

pub fn render_benchmark(r: &BenchmarkReport) -> String {
    let mut out = String::new();
    let tier_color = match r.team_tier {
        BenchmarkTier::Platinum  => "★★★★",
        BenchmarkTier::Gold      => "★★★☆",
        BenchmarkTier::Silver    => "★★☆☆",
        BenchmarkTier::Bronze    => "★☆☆☆",
        BenchmarkTier::Developing=> "☆☆☆☆",
    };

    out.push_str(&format!(
        "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\
         axon-signal  ·  Team Benchmark\n\
         ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n\
         TIER  {} {}  ({} sessions, {} engineers)\n\
         SCORE  {:.0}/100  ·  {}\n",
        tier_color, r.team_tier.label(),
        r.sessions_analyzed, r.engineers_analyzed,
        r.team_avg, r.team_tier.percentile_estimate()
    ));

    if let (Some(ref next), Some(pts)) = (&r.next_tier, r.points_to_next_tier) {
        out.push_str(&format!("       +{:.0} pts → {} tier\n", pts, next.label()));
    }

    out.push_str("\nDIMENSIONS  (vs estimated industry median)\n");
    for d in &r.dimensions {
        let better = d.gap >= 0.0;
        let sym = if better { "✓" } else { "!" };
        out.push_str(&format!(
            "  {} {:22}  {:.0}{}  (median {:.0}{})\n",
            sym, d.name, d.team_value, d.unit, d.industry_median, d.unit
        ));
    }

    if !r.top_recommendations.is_empty() {
        out.push_str("\nTOP RECOMMENDATIONS\n");
        for (i, rec) in r.top_recommendations.iter().enumerate() {
            out.push_str(&format!("  {}. {}\n", i + 1, rec));
        }
    }

    out.push_str("\nENGINEER BREAKDOWN\n");
    for e in &r.engineers {
        out.push_str(&format!(
            "  {:35}  {:8}  {:.0}/100  {}th percentile\n",
            e.engineer, e.tier.label(), e.avg_score, e.percentile_within_team
        ));
    }

    out.push_str(&format!("\n⚠  {}\n", r.caveats));
    out
}

fn goal_clarity_recommendation(avg: f64) -> String {
    if avg >= 70.0 { "Above median — keep adding file refs and measurable criteria.".to_string() }
    else if avg >= 55.0 { "Near median — add measurable outcomes ('tests pass', '< 50ms') to push higher.".to_string() }
    else { format!("Below median ({:.0}/100) — start goals with a specific file name and one verb (fix/add/refactor).", avg) }
}

fn turns_recommendation(avg: f64) -> String {
    if avg <= 15.0 { "Excellent — very tight sessions.".to_string() }
    else if avg <= 22.0 { "Good — near median efficiency.".to_string() }
    else { format!("High turns/commit ({:.0} vs median 22) — scope sessions to one function or one file.", avg) }
}

fn rework_recommendation(rate: f64) -> String {
    if rate <= 10.0 { "Low rework — strong exit criteria discipline.".to_string() }
    else if rate <= 18.0 { "Near median — acceptable rework rate.".to_string() }
    else { format!("High rework rate ({:.0}% vs median 18%) — define 'done' before starting each session.", rate) }
}

fn commit_rate_recommendation(rate: f64) -> String {
    if rate >= 80.0 { "High commit rate — sessions reliably ship.".to_string() }
    else if rate >= 70.0 { "Good — near median.".to_string() }
    else { format!("Low commit rate ({:.0}% vs median 70%) — commit partial progress; don't wait for perfection.", rate) }
}

fn round1(x: f64) -> f64 { (x * 10.0).round() / 10.0 }

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const CAVEATS: &str = "Industry medians are estimated from the score formula, not external survey data. \
    Tiers will be updated as aggregate data is collected. Use for directional guidance, not absolute ranking.";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tier_from_score() {
        assert_eq!(BenchmarkTier::from_score(85.0), BenchmarkTier::Platinum);
        assert_eq!(BenchmarkTier::from_score(70.0), BenchmarkTier::Gold);
        assert_eq!(BenchmarkTier::from_score(57.0), BenchmarkTier::Silver);
        assert_eq!(BenchmarkTier::from_score(42.0), BenchmarkTier::Bronze);
        assert_eq!(BenchmarkTier::from_score(20.0), BenchmarkTier::Developing);
    }

    #[test]
    fn test_tier_next_and_points() {
        let t = BenchmarkTier::Silver;
        assert_eq!(t.next(), Some(BenchmarkTier::Gold));
        let pts = t.points_to_next(58.0).unwrap();
        assert!((pts - 7.0).abs() < 0.5);
    }

    #[test]
    fn test_platinum_has_no_next() {
        assert_eq!(BenchmarkTier::Platinum.next(), None);
        assert!(BenchmarkTier::Platinum.points_to_next(90.0).is_none());
    }

    #[test]
    fn test_compute_benchmark_empty() {
        let dir = tempfile::tempdir().unwrap();
        let store = axon_ledger::store::Store::open(dir.path()).unwrap();
        let r = compute_benchmark(&store, None).unwrap();
        assert_eq!(r.sessions_analyzed, 0);
        assert_eq!(r.team_tier, BenchmarkTier::Developing);
    }

    #[test]
    fn test_render_benchmark_nonempty() {
        let report = BenchmarkReport {
            team_avg: 71.5,
            team_tier: BenchmarkTier::Gold,
            points_to_next_tier: Some(8.5),
            next_tier: Some(BenchmarkTier::Platinum),
            sessions_analyzed: 12,
            engineers_analyzed: 3,
            dimensions: vec![
                DimensionBenchmark {
                    name: "Goal Clarity".to_string(),
                    team_value: 68.0,
                    industry_median: 55.0,
                    unit: "/100".to_string(),
                    higher_is_better: true,
                    gap: 13.0,
                    recommendation: "Above median.".to_string(),
                },
            ],
            engineers: vec![
                EngineerBenchmark {
                    engineer: "alice@example.com".to_string(),
                    avg_score: 78.0,
                    tier: BenchmarkTier::Gold,
                    sessions: 5,
                    percentile_within_team: 100,
                },
            ],
            top_recommendations: vec!["Cut rework rate.".to_string()],
            caveats: "Industry medians are estimated.".to_string(),
        };
        let text = render_benchmark(&report);
        assert!(text.contains("Gold"));
        // render uses {:.0} — 71.5 rounds to 72 in display
        assert!(text.contains("72") || text.contains("71"));
        assert!(text.contains("alice@example.com"));
        assert!(text.contains("Goal Clarity"));
    }
}
