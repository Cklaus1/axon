/// Per-engineer week-over-week score trends.
///
/// Groups sessions into ISO calendar weeks and computes per-engineer
/// effectiveness score averages. Shows trajectory: improving, declining,
/// or stable. No LLM calls — derived entirely from ledger data.
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use axon_ledger::store::Store;

use crate::score::{score_sessions, SessionScore};

/// One week's statistics for one engineer.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WeekSlice {
    /// ISO year-week, e.g. "2026-W24"
    pub week: String,
    pub avg_score: f64,
    pub sessions: usize,
    pub commits: usize,
    pub goal_clarity_avg: f64,
}

/// Trend direction over the last N weeks.
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum TrendDirection {
    Improving,
    Declining,
    Stable,
    Insufficient, // fewer than 2 weeks of data
}

/// Per-engineer trend report.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct EngineerTrend {
    pub engineer: String,
    pub weeks: Vec<WeekSlice>,
    pub direction: TrendDirection,
    /// Score delta: last week avg − first week avg (positive = improving)
    pub delta: f64,
    /// All-time average score for this engineer
    pub overall_avg: f64,
    pub total_sessions: usize,
    pub recommendation: String,
}

/// Full trends report for the team.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct TrendsReport {
    pub engineers: Vec<EngineerTrend>,
    pub team_avg_this_week: f64,
    pub team_avg_last_week: f64,
    pub team_direction: TrendDirection,
    pub weeks_included: usize,
}

fn ms_to_iso_week(ts_ms: u64) -> String {
    // Simple ISO week calculation without chrono dependency.
    // Days since Unix epoch → year + week number.
    let days = (ts_ms / 86_400_000) as i64;
    // Shift to Thursday-based week (ISO 8601: week containing Thursday)
    // Jan 1 1970 was a Thursday (day 4 of week).
    let thursday_days = days + 3; // make weeks start Mon by aligning to Thursday
    let year_approx = (thursday_days as f64 / 365.2425) as i64 + 1970;

    // Compute day-of-year for Jan 4 of year_approx (always in week 1)
    let epoch_year_start = year_days_since_epoch(year_approx);
    let epoch_jan4 = epoch_year_start + 3; // Jan 4

    let week_num = (thursday_days - epoch_jan4) / 7 + 1;
    if week_num < 1 {
        // Belongs to previous year's last week
        let prev_year = year_approx - 1;
        let prev_jan4 = year_days_since_epoch(prev_year) + 3;
        let w = (thursday_days - prev_jan4) / 7 + 1;
        return format!("{prev_year}-W{w:02}");
    }
    format!("{year_approx}-W{week_num:02}")
}

fn year_days_since_epoch(year: i64) -> i64 {
    // Days from Unix epoch (1970-01-01) to Jan 1 of `year`.
    let y = year - 1970;
    let leap_days = (y - 1) / 4 - (y - 1) / 100
        + (y - 1) / 400
        + if y < 0 {
            -((-y) / 4 - (-y) / 100 + (-y) / 400) * 2
        } else {
            0
        };
    y * 365 + leap_days
}

fn trend_direction(weeks: &[WeekSlice]) -> (TrendDirection, f64) {
    if weeks.len() < 2 {
        return (TrendDirection::Insufficient, 0.0);
    }
    let first = weeks.first().unwrap().avg_score;
    let last = weeks.last().unwrap().avg_score;
    let delta = last - first;
    let dir = if delta > 5.0 {
        TrendDirection::Improving
    } else if delta < -5.0 {
        TrendDirection::Declining
    } else {
        TrendDirection::Stable
    };
    (dir, delta)
}

fn make_recommendation(eng: &str, trend: &EngineerTrend) -> String {
    match &trend.direction {
        TrendDirection::Improving => format!(
            "{eng}: +{:.0} pts over {} weeks — keep it up! Top pattern: add file refs to session goals.",
            trend.delta, trend.weeks.len()
        ),
        TrendDirection::Declining => format!(
            "{eng}: {:.0} pts over {} weeks — try adding measurable outcomes to goals (e.g. 'p99_ms < 50').",
            trend.delta, trend.weeks.len()
        ),
        TrendDirection::Stable => {
            if trend.overall_avg < 50.0 {
                format!("{eng}: avg {:.0}/100 — goals need file refs and specific verbs (fix/add/refactor).", trend.overall_avg)
            } else if trend.overall_avg < 70.0 {
                format!("{eng}: avg {:.0}/100 — add measurable outcomes to push past 70.", trend.overall_avg)
            } else {
                format!("{eng}: avg {:.0}/100 — solid. Consider /loop to automate high-turn sessions.", trend.overall_avg)
            }
        }
        TrendDirection::Insufficient => format!(
            "{eng}: only {} week(s) of data — need 2+ for trend direction.", trend.weeks.len()
        ),
    }
}

/// Compute per-engineer trend charts from all scored sessions.
pub fn compute_trends(store: &Store, weeks_back: usize) -> anyhow::Result<TrendsReport> {
    let scores: Vec<SessionScore> = score_sessions(store)?;

    // Filter to the requested window
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64;
    let cutoff_ms = now_ms.saturating_sub(weeks_back as u64 * 7 * 86_400_000);
    let scores: Vec<&SessionScore> = scores.iter().filter(|s| s.ts_ms >= cutoff_ms).collect();

    // Group by (engineer, week)
    let mut by_eng_week: HashMap<String, HashMap<String, Vec<&SessionScore>>> = HashMap::new();
    for s in &scores {
        let week = ms_to_iso_week(s.ts_ms);
        by_eng_week
            .entry(s.engineer.clone())
            .or_default()
            .entry(week)
            .or_default()
            .push(s);
    }

    let mut engineers: Vec<EngineerTrend> = by_eng_week
        .into_iter()
        .map(|(eng, week_map)| {
            let mut weeks: Vec<WeekSlice> = week_map
                .into_iter()
                .map(|(week, sessions)| {
                    let avg_score = sessions.iter().map(|s| s.score as f64).sum::<f64>()
                        / sessions.len() as f64;
                    let goal_clarity_avg =
                        sessions.iter().map(|s| s.goal_clarity as f64).sum::<f64>()
                            / sessions.len() as f64;
                    let commits = sessions.iter().map(|s| s.commits_linked).sum();
                    WeekSlice {
                        week,
                        avg_score,
                        sessions: sessions.len(),
                        commits,
                        goal_clarity_avg,
                    }
                })
                .collect();
            weeks.sort_by(|a, b| a.week.cmp(&b.week)); // chronological

            let (direction, delta) = trend_direction(&weeks);
            let all_scores: Vec<f64> = weeks
                .iter()
                .flat_map(|w| {
                    // re-derive from week slices — weighted by session count
                    std::iter::repeat_n(w.avg_score, w.sessions)
                })
                .collect();
            let overall_avg = if all_scores.is_empty() {
                0.0
            } else {
                all_scores.iter().sum::<f64>() / all_scores.len() as f64
            };
            let total_sessions = weeks.iter().map(|w| w.sessions).sum();

            let mut trend = EngineerTrend {
                engineer: eng.clone(),
                weeks,
                direction,
                delta,
                overall_avg,
                total_sessions,
                recommendation: String::new(),
            };
            trend.recommendation = make_recommendation(&eng, &trend);
            trend
        })
        .collect();

    engineers.sort_by(|a, b| {
        b.overall_avg
            .partial_cmp(&a.overall_avg)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Team week-over-week
    let current_week = ms_to_iso_week(now_ms);
    let last_week_ms = now_ms.saturating_sub(7 * 86_400_000);
    let last_week = ms_to_iso_week(last_week_ms);

    let team_this: Vec<f64> = scores
        .iter()
        .filter(|s| ms_to_iso_week(s.ts_ms) == current_week)
        .map(|s| s.score as f64)
        .collect();
    let team_last: Vec<f64> = scores
        .iter()
        .filter(|s| ms_to_iso_week(s.ts_ms) == last_week)
        .map(|s| s.score as f64)
        .collect();

    let team_avg_this = if team_this.is_empty() {
        0.0
    } else {
        team_this.iter().sum::<f64>() / team_this.len() as f64
    };
    let team_avg_last = if team_last.is_empty() {
        0.0
    } else {
        team_last.iter().sum::<f64>() / team_last.len() as f64
    };

    let team_delta = team_avg_this - team_avg_last;
    let team_direction = if team_this.is_empty() || team_last.is_empty() {
        TrendDirection::Insufficient
    } else if team_delta > 5.0 {
        TrendDirection::Improving
    } else if team_delta < -5.0 {
        TrendDirection::Declining
    } else {
        TrendDirection::Stable
    };

    Ok(TrendsReport {
        engineers,
        team_avg_this_week: team_avg_this,
        team_avg_last_week: team_avg_last,
        team_direction,
        weeks_included: weeks_back,
    })
}

/// Render trends as formatted text.
pub fn render_trends(report: &TrendsReport) -> String {
    let mut out = String::new();
    let arrow = match &report.team_direction {
        TrendDirection::Improving => "↑",
        TrendDirection::Declining => "↓",
        TrendDirection::Stable => "→",
        TrendDirection::Insufficient => "?",
    };

    out.push_str(&format!(
        "Team trend  {arrow}  this week {:.0}/100  |  last week {:.0}/100  ({}w window)\n\n",
        report.team_avg_this_week, report.team_avg_last_week, report.weeks_included
    ));

    for eng in &report.engineers {
        let dir_sym = match &eng.direction {
            TrendDirection::Improving => "↑",
            TrendDirection::Declining => "↓",
            TrendDirection::Stable => "→",
            TrendDirection::Insufficient => "·",
        };
        out.push_str(&format!(
            "  {dir_sym} {:40}  avg {:.0}/100  {} sessions\n",
            eng.engineer, eng.overall_avg, eng.total_sessions
        ));

        // Week-by-week mini chart
        if eng.weeks.len() > 1 {
            let chart: Vec<String> = eng
                .weeks
                .iter()
                .map(|w| {
                    let bar = "█".repeat((w.avg_score / 20.0) as usize).to_string();
                    format!("    {} {:>3.0}  {}", w.week, w.avg_score, bar)
                })
                .collect();
            out.push_str(&chart.join("\n"));
            out.push('\n');
        }

        out.push_str(&format!("     💡 {}\n\n", eng.recommendation));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_iso_week_basic() {
        // 2026-06-15 is in week 25 of 2026
        let ts = 1_750_000_000_000u64; // approx 2025-06
        let w = ms_to_iso_week(ts);
        assert!(w.starts_with("202"), "should be a 2020s week: {w}");
        assert!(w.contains("-W"), "should be ISO week format: {w}");
    }

    #[test]
    fn test_trend_direction_improving() {
        let weeks = vec![
            WeekSlice {
                week: "2026-W20".into(),
                avg_score: 50.0,
                sessions: 2,
                commits: 1,
                goal_clarity_avg: 50.0,
            },
            WeekSlice {
                week: "2026-W21".into(),
                avg_score: 70.0,
                sessions: 2,
                commits: 2,
                goal_clarity_avg: 60.0,
            },
            WeekSlice {
                week: "2026-W22".into(),
                avg_score: 80.0,
                sessions: 3,
                commits: 3,
                goal_clarity_avg: 70.0,
            },
        ];
        let (dir, delta) = trend_direction(&weeks);
        assert_eq!(dir, TrendDirection::Improving);
        assert!(delta > 0.0);
    }

    #[test]
    fn test_trend_direction_declining() {
        let weeks = vec![
            WeekSlice {
                week: "2026-W20".into(),
                avg_score: 85.0,
                sessions: 2,
                commits: 2,
                goal_clarity_avg: 80.0,
            },
            WeekSlice {
                week: "2026-W21".into(),
                avg_score: 60.0,
                sessions: 1,
                commits: 1,
                goal_clarity_avg: 50.0,
            },
        ];
        let (dir, delta) = trend_direction(&weeks);
        assert_eq!(dir, TrendDirection::Declining);
        assert!(delta < 0.0);
    }

    #[test]
    fn test_trend_direction_insufficient() {
        let weeks = vec![WeekSlice {
            week: "2026-W20".into(),
            avg_score: 70.0,
            sessions: 1,
            commits: 1,
            goal_clarity_avg: 65.0,
        }];
        let (dir, _) = trend_direction(&weeks);
        assert_eq!(dir, TrendDirection::Insufficient);
    }

    #[test]
    fn test_render_trends_nonempty() {
        let report = TrendsReport {
            engineers: vec![EngineerTrend {
                engineer: "alice@example.com".into(),
                weeks: vec![
                    WeekSlice {
                        week: "2026-W20".into(),
                        avg_score: 60.0,
                        sessions: 2,
                        commits: 1,
                        goal_clarity_avg: 55.0,
                    },
                    WeekSlice {
                        week: "2026-W21".into(),
                        avg_score: 75.0,
                        sessions: 3,
                        commits: 2,
                        goal_clarity_avg: 70.0,
                    },
                ],
                direction: TrendDirection::Improving,
                delta: 15.0,
                overall_avg: 68.0,
                total_sessions: 5,
                recommendation: "Keep it up!".into(),
            }],
            team_avg_this_week: 75.0,
            team_avg_last_week: 60.0,
            team_direction: TrendDirection::Improving,
            weeks_included: 4,
        };
        let text = render_trends(&report);
        assert!(text.contains("alice@example.com"));
        assert!(text.contains("↑"));
        assert!(text.contains("2026-W20"));
    }
}
