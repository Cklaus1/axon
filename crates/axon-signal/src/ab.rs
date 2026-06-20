/// Recommendation A/B tracking.
///
/// When `signal weekly --track` is run, the top recommendation shown is stored
/// in `<ledger-dir>/recommendations.ndjson`. Each record captures the week,
/// recommendation type, baseline score, and which engineer (if filtered).
///
/// `signal ab-status` reads those records and computes whether scores improved
/// in the week following each recommendation, giving a per-type outcome table.
///
/// This is an observational before/after measurement, not a randomised trial —
/// the output is labeled accordingly.
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use axon_ledger::store::Store;

use crate::score::score_sessions;

/// Broad categories of weekly recommendation, used to group outcomes.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq, Eq, Hash)]
pub enum RecommendationType {
    /// "Add file refs / specific verbs to goals"
    GoalClarity,
    /// "Scope sessions to one file/function"
    ScopeControl,
    /// "Define exit criteria upfront"
    ReworkPrevention,
    /// "Commit partial progress"
    CommitRate,
    /// "Use /loop to automate high-turn sessions"
    LoopAdoption,
    /// Catch-all for recommendations that don't match the above
    Other,
}

impl RecommendationType {
    pub fn detect(text: &str) -> Self {
        let t = text.to_lowercase();
        if t.contains("goal") || t.contains("file ref") || t.contains("clarity") {
            RecommendationType::GoalClarity
        } else if t.contains("scope") || t.contains("one file") || t.contains("one function") {
            RecommendationType::ScopeControl
        } else if t.contains("rework") || t.contains("exit crit") || t.contains("done") {
            RecommendationType::ReworkPrevention
        } else if t.contains("commit") {
            RecommendationType::CommitRate
        } else if t.contains("loop") || t.contains("automate") {
            RecommendationType::LoopAdoption
        } else {
            RecommendationType::Other
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            RecommendationType::GoalClarity    => "Goal Clarity",
            RecommendationType::ScopeControl   => "Scope Control",
            RecommendationType::ReworkPrevention => "Rework Prevention",
            RecommendationType::CommitRate     => "Commit Rate",
            RecommendationType::LoopAdoption   => "Loop Adoption",
            RecommendationType::Other          => "Other",
        }
    }
}

/// One recorded recommendation instance.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecommendationRecord {
    pub id: String,
    /// ISO year-week, e.g. "2026-W25"
    pub week: String,
    pub rec_type: RecommendationType,
    pub text: String,
    /// Team average score at the time the recommendation was shown.
    pub baseline_score: f64,
    /// Specific engineer this was shown for, or None for team-wide.
    pub engineer: Option<String>,
    /// Unix milliseconds when the recommendation was recorded.
    pub recorded_ms: u64,
}

/// Outcome of a recommendation: did the following week improve?
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RecommendationOutcome {
    pub rec: RecommendationRecord,
    /// Average score in the week after the recommendation.
    pub follow_up_score: Option<f64>,
    /// Score delta (follow_up - baseline). None if no data yet.
    pub delta: Option<f64>,
    /// Whether scores improved (delta > 2 pts).
    pub improved: Option<bool>,
}

/// Summary by recommendation type.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AbTypeSummary {
    pub rec_type: RecommendationType,
    pub instances: usize,
    pub instances_with_followup: usize,
    pub avg_delta: Option<f64>,
    pub improved_rate: Option<f64>,
}

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AbReport {
    pub outcomes: Vec<RecommendationOutcome>,
    pub by_type: Vec<AbTypeSummary>,
    pub total_tracked: usize,
    pub total_with_followup: usize,
    pub disclaimer: String,
}

fn recs_path(ledger_dir: &Path) -> PathBuf {
    ledger_dir.join("recommendations.ndjson")
}

/// Append a recommendation record for tracking.
pub fn record_recommendation(
    ledger_dir: &Path,
    week: &str,
    text: &str,
    baseline_score: f64,
    engineer: Option<&str>,
) -> anyhow::Result<RecommendationRecord> {
    let rec_type = RecommendationType::detect(text);
    let now = now_ms();
    let id = format!("rec-{}-{:x}", week, now & 0xFFFF);

    let rec = RecommendationRecord {
        id,
        week: week.to_string(),
        rec_type,
        text: text.to_string(),
        baseline_score,
        engineer: engineer.map(String::from),
        recorded_ms: now,
    };

    let path = recs_path(ledger_dir);
    let mut file = OpenOptions::new().create(true).append(true).open(&path)?;
    writeln!(file, "{}", serde_json::to_string(&rec)?)?;
    Ok(rec)
}

fn load_records(ledger_dir: &Path) -> anyhow::Result<Vec<RecommendationRecord>> {
    let path = recs_path(ledger_dir);
    if !path.exists() {
        return Ok(vec![]);
    }
    let file = File::open(&path)?;
    let reader = BufReader::new(file);
    let mut recs = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let t = line.trim();
        if t.is_empty() { continue; }
        if let Ok(r) = serde_json::from_str::<RecommendationRecord>(t) {
            recs.push(r);
        }
    }
    Ok(recs)
}

/// Compute A/B outcomes by looking up the following week's scores.
pub fn compute_ab_report(ledger_dir: &Path, store: &Store) -> anyhow::Result<AbReport> {
    let recs = load_records(ledger_dir)?;
    let scores = score_sessions(store)?;

    // Group scores by ISO week
    let mut by_week: std::collections::HashMap<String, Vec<f64>> = std::collections::HashMap::new();
    for s in &scores {
        let w = ms_to_iso_week(s.ts_ms);
        by_week.entry(w).or_default().push(s.score as f64);
    }

    let week_avg = |week: &str| -> Option<f64> {
        by_week.get(week).map(|v| v.iter().sum::<f64>() / v.len() as f64)
    };

    let next_week = |week: &str| -> Option<String> {
        // Parse year and week number from "YYYY-WNN"
        let (year_s, week_s) = week.split_once("-W")?;
        let year: i64 = year_s.parse().ok()?;
        let wn: i64 = week_s.parse().ok()?;
        if wn >= 52 {
            Some(format!("{}-W01", year + 1))
        } else {
            Some(format!("{}-W{:02}", year, wn + 1))
        }
    };

    let mut outcomes: Vec<RecommendationOutcome> = recs.iter().map(|rec| {
        let follow_up_score = next_week(&rec.week).and_then(|nw| week_avg(&nw));
        let delta = follow_up_score.map(|f| f - rec.baseline_score);
        let improved = delta.map(|d| d > 2.0);
        RecommendationOutcome {
            rec: rec.clone(),
            follow_up_score: follow_up_score.map(|f| (f * 10.0).round() / 10.0),
            delta: delta.map(|d| (d * 10.0).round() / 10.0),
            improved,
        }
    }).collect();

    // Sort: most recent first
    outcomes.sort_by(|a, b| b.rec.recorded_ms.cmp(&a.rec.recorded_ms));

    // Aggregate by type
    use std::collections::HashMap;
    let mut type_map: HashMap<String, (usize, Vec<f64>, usize)> = HashMap::new();
    for o in &outcomes {
        let key = format!("{:?}", o.rec.rec_type);
        let entry = type_map.entry(key).or_insert((0, vec![], 0));
        entry.0 += 1;
        if let Some(d) = o.delta {
            entry.1.push(d);
            if o.improved == Some(true) { entry.2 += 1; }
        }
    }

    let mut by_type: Vec<AbTypeSummary> = outcomes.iter().map(|o| o.rec.rec_type.clone())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .map(|rt| {
            let key = format!("{:?}", rt);
            let (instances, deltas, improved_count) = type_map.get(&key)
                .cloned()
                .unwrap_or((0, vec![], 0));
            let instances_with_followup = deltas.len();
            let avg_delta = if deltas.is_empty() { None }
                else { Some((deltas.iter().sum::<f64>() / deltas.len() as f64 * 10.0).round() / 10.0) };
            let improved_rate = if instances_with_followup == 0 { None }
                else { Some((improved_count as f64 / instances_with_followup as f64 * 100.0).round()) };
            AbTypeSummary { rec_type: rt, instances, instances_with_followup, avg_delta, improved_rate }
        })
        .collect();

    // Sort by avg_delta descending (best improvement first)
    by_type.sort_by(|a, b| {
        b.avg_delta.unwrap_or(f64::NEG_INFINITY)
            .partial_cmp(&a.avg_delta.unwrap_or(f64::NEG_INFINITY))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    let total_with_followup = outcomes.iter().filter(|o| o.delta.is_some()).count();

    Ok(AbReport {
        total_tracked: outcomes.len(),
        total_with_followup,
        outcomes,
        by_type,
        disclaimer: AB_DISCLAIMER.to_string(),
    })
}

pub fn render_ab_report(r: &AbReport) -> String {
    let mut out = String::new();
    out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");
    out.push_str("axon-signal  ·  Recommendation Outcomes\n");
    out.push_str("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n\n");

    if r.total_tracked == 0 {
        out.push_str("No recommendations tracked yet.\n");
        out.push_str("Run `signal weekly --track` to start recording recommendations.\n");
        return out;
    }

    out.push_str(&format!(
        "{} recommendations tracked  ·  {} with follow-up data\n\n",
        r.total_tracked, r.total_with_followup
    ));

    if !r.by_type.is_empty() {
        out.push_str("BY RECOMMENDATION TYPE\n");
        out.push_str(&format!("  {:<22}  {:>5}  {:>9}  {:>8}  {}\n",
            "Type", "N", "Follow-up", "Avg Δ", "Improved%"));
        out.push_str(&format!("  {}\n", "─".repeat(62)));
        for t in &r.by_type {
            let followup = if t.instances_with_followup > 0 {
                format!("{}/{}", t.instances_with_followup, t.instances)
            } else {
                format!("0/{}", t.instances)
            };
            let delta = t.avg_delta.map(|d| format!("{:+.1}", d)).unwrap_or("—".to_string());
            let rate = t.improved_rate.map(|r| format!("{:.0}%", r)).unwrap_or("—".to_string());
            out.push_str(&format!("  {:<22}  {:>5}  {:>9}  {:>8}  {}\n",
                t.rec_type.label(), t.instances, followup, delta, rate));
        }
        out.push('\n');
    }

    out.push_str("RECENT RECOMMENDATIONS\n");
    for o in r.outcomes.iter().take(10) {
        let status = match o.improved {
            Some(true)  => format!("↑ +{:.1} pts", o.delta.unwrap_or(0.0)),
            Some(false) => format!("↓ {:.1} pts", o.delta.unwrap_or(0.0)),
            None        => "awaiting next week".to_string(),
        };
        out.push_str(&format!("  {}  [{:18}]  {}\n",
            o.rec.week, o.rec.rec_type.label(), status));
        out.push_str(&format!("     \"{}\"\n", o.rec.text.chars().take(70).collect::<String>()));
    }

    out.push_str(&format!("\n⚠  {}\n", r.disclaimer));
    out
}

pub fn current_iso_week() -> String {
    ms_to_iso_week(now_ms())
}

fn ms_to_iso_week(ts_ms: u64) -> String {
    let days = (ts_ms / 86_400_000) as i64;
    let thursday_days = days + 3;
    let year_approx = (thursday_days as f64 / 365.2425) as i64 + 1970;
    let epoch_jan4 = year_days_since_epoch(year_approx) + 3;
    let week_num = (thursday_days - epoch_jan4) / 7 + 1;
    if week_num < 1 {
        let prev = year_approx - 1;
        let prev_jan4 = year_days_since_epoch(prev) + 3;
        let w = (thursday_days - prev_jan4) / 7 + 1;
        return format!("{prev}-W{w:02}");
    }
    format!("{year_approx}-W{week_num:02}")
}

fn year_days_since_epoch(year: i64) -> i64 {
    let y = year - 1970;
    y * 365 + (y - 1).max(0) / 4 - (y - 1).max(0) / 100 + (y - 1).max(0) / 400
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

const AB_DISCLAIMER: &str = "This is an observational before/after measurement per recommendation week, \
    not a randomised trial. Confounding factors (project complexity, team changes) \
    are not controlled. Use directionally, not as causal proof.";

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_rec_type_detection() {
        assert_eq!(RecommendationType::detect("add file refs to goals"), RecommendationType::GoalClarity);
        assert_eq!(RecommendationType::detect("scope to one function"), RecommendationType::ScopeControl);
        assert_eq!(RecommendationType::detect("define exit criteria"), RecommendationType::ReworkPrevention);
        assert_eq!(RecommendationType::detect("commit partial progress"), RecommendationType::CommitRate);
        assert_eq!(RecommendationType::detect("use /loop to automate"), RecommendationType::LoopAdoption);
        assert_eq!(RecommendationType::detect("something unrelated"), RecommendationType::Other);
    }

    #[test]
    fn test_record_and_load() {
        let dir = tempdir().unwrap();
        let rec = record_recommendation(dir.path(), "2026-W25", "add file refs to goals", 62.0, None).unwrap();
        assert_eq!(rec.rec_type, RecommendationType::GoalClarity);
        assert_eq!(rec.week, "2026-W25");
        let loaded = load_records(dir.path()).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].week, "2026-W25");
    }

    #[test]
    fn test_load_missing_file() {
        let dir = tempdir().unwrap();
        let recs = load_records(dir.path()).unwrap();
        assert!(recs.is_empty());
    }

    #[test]
    fn test_compute_ab_empty() {
        let dir = tempdir().unwrap();
        let store = axon_ledger::store::Store::open(dir.path()).unwrap();
        let report = compute_ab_report(dir.path(), &store).unwrap();
        assert_eq!(report.total_tracked, 0);
        assert!(report.outcomes.is_empty());
    }

    #[test]
    fn test_render_ab_no_data() {
        let report = AbReport {
            outcomes: vec![],
            by_type: vec![],
            total_tracked: 0,
            total_with_followup: 0,
            disclaimer: "test".to_string(),
        };
        let text = render_ab_report(&report);
        assert!(text.contains("No recommendations tracked"));
        assert!(text.contains("--track"));
    }

    #[test]
    fn test_render_ab_with_data() {
        let report = AbReport {
            outcomes: vec![
                RecommendationOutcome {
                    rec: RecommendationRecord {
                        id: "rec-2026-W25-1".to_string(),
                        week: "2026-W25".to_string(),
                        rec_type: RecommendationType::GoalClarity,
                        text: "add file refs to goals".to_string(),
                        baseline_score: 62.0,
                        engineer: None,
                        recorded_ms: 1_000_000,
                    },
                    follow_up_score: Some(68.0),
                    delta: Some(6.0),
                    improved: Some(true),
                },
            ],
            by_type: vec![AbTypeSummary {
                rec_type: RecommendationType::GoalClarity,
                instances: 1,
                instances_with_followup: 1,
                avg_delta: Some(6.0),
                improved_rate: Some(100.0),
            }],
            total_tracked: 1,
            total_with_followup: 1,
            disclaimer: "observational".to_string(),
        };
        let text = render_ab_report(&report);
        assert!(text.contains("Goal Clarity"));
        assert!(text.contains("2026-W25"));
        assert!(text.contains("+6.0"));
    }
}
