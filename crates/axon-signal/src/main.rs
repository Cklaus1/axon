use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use axon_ledger::store::Store;
use axon_signal::export::{export_dpo, export_training, ExportFormat, ExportOptions};
use axon_signal::patterns::{antipatterns, build_pattern_library};
use axon_signal::rework::find_rework_hotspots;
use axon_signal::score::score_sessions;
use axon_signal::weekly::{generate_weekly, render_text};

#[derive(Parser)]
#[command(name = "axon-signal", about = "AI coding effectiveness analytics")]
struct Cli {
    /// Ledger directory (default: $HOME/.axon/ledger)
    #[arg(long, global = true)]
    ledger_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Score all sessions (or filter by engineer/week)
    Score {
        /// Filter to a specific engineer (by email prefix)
        #[arg(long)]
        engineer: Option<String>,
        /// Only score sessions from this week
        #[arg(long)]
        week: bool,
        /// Write effectiveness scores back into the ledger as MetricOutcome records
        #[arg(long)]
        ingest: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Detect rework hotspots (files touched by multiple sessions)
    Rework {
        /// Window in days (default: 14)
        #[arg(long, default_value = "14")]
        days: u64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show goal patterns that worked vs. didn't
    Patterns {
        /// Minimum sessions to include a pattern (default: 2)
        #[arg(long, default_value = "2")]
        min_sessions: usize,
        /// Minimum score threshold for top patterns (default: 65)
        #[arg(long, default_value = "65")]
        min_score: u8,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Generate weekly team effectiveness report
    Weekly {
        /// Start of window (ISO 8601, default: 7 days ago)
        #[arg(long)]
        from: Option<String>,
        /// End of window (ISO 8601, default: now)
        #[arg(long)]
        to: Option<String>,
        /// Slack webhook URL (post report to Slack)
        #[arg(long)]
        slack_webhook: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Per-engineer goal quality breakdown — who writes the clearest session goals?
    Goals {
        /// Filter to sessions from the last N days (default: 30)
        #[arg(long, default_value = "30")]
        days: u64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Detect sessions that would benefit from a loop/workflow (high-turn, low-commit)
    Loops {
        /// Turn-count threshold to flag a session (default: 40)
        #[arg(long, default_value = "40")]
        turns_threshold: u64,
        /// Only include sessions from the last N days (default: 30)
        #[arg(long, default_value = "30")]
        days: u64,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Export training data for Trainloop / MegaBrain fine-tuning
    ExportTraining {
        /// Minimum signal score to include (default: 75)
        #[arg(long, default_value = "75")]
        min_score: u8,
        /// Only include sessions after this date (ISO 8601)
        #[arg(long)]
        since: Option<String>,
        /// Output format: trainloop (default) or dpo (chosen/rejected pairs)
        #[arg(long, default_value = "trainloop")]
        format: String,
        /// Anonymize engineer names in the export
        #[arg(long)]
        anonymize: bool,
        /// Strip code file contents from prompts (keep only instructions)
        #[arg(long)]
        exclude_code_content: bool,
        /// Output file (default: stdout)
        #[arg(long)]
        out: Option<PathBuf>,
    },
}

fn default_ledger_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".axon").join("ledger")
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn week_ago_ms() -> u64 {
    now_ms().saturating_sub(7 * 24 * 60 * 60 * 1000)
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let ledger_dir = cli.ledger_dir.unwrap_or_else(default_ledger_dir);
    let store = Store::open(&ledger_dir)?;

    match cli.command {
        Commands::Score { engineer, week, ingest, json } => {
            let mut scores = score_sessions(&store)?;

            if let Some(eng) = &engineer {
                scores.retain(|s| s.engineer.contains(eng.as_str()));
            }
            if week {
                let cutoff = week_ago_ms();
                scores.retain(|s| s.ts_ms >= cutoff);
            }

            if json {
                println!("{}", serde_json::to_string_pretty(&scores)?);
            } else {
                println!("{} sessions scored:\n", scores.len());
                for s in &scores {
                    let bar = "█".repeat(s.score as usize / 10)
                        + &"░".repeat(10usize.saturating_sub(s.score as usize / 10));
                    let goal = if s.goal.len() > 55 { format!("{}…", &s.goal[..55]) } else { s.goal.clone() };
                    println!("  {} {:>3}  {} turns → {} commits  {:?}",
                        bar, s.score, s.turns, s.commits_linked, goal);
                }
                let avg = if scores.is_empty() { 0.0 } else {
                    scores.iter().map(|s| s.score as f64).sum::<f64>() / scores.len() as f64
                };
                println!("\n  Average: {:.1}/100", avg);
            }

            if ingest {
                use axon_ledger::hash::record_id;
                use axon_ledger::model::{Effect, LedgerRecord};
                let mut store_mut = Store::open(&ledger_dir)?;
                let mut written = 0usize;
                for s in &scores {
                    let payload = serde_json::json!({
                        "session_id": s.session_id,
                        "score": s.score,
                        "label": s.label,
                        "goal_clarity": s.goal_clarity,
                        "turns_per_commit": s.turns_per_commit,
                        "scope_fit": s.scope_fit,
                        "training_tier": s.training_tier.label(),
                        "rework_signal": s.rework_signal,
                        "source": "axon-signal",
                    });
                    let ts_ms = s.ts_ms + 1;
                    let id = record_id(
                        &format!("signal:session:{}", s.session_id),
                        &Effect::MetricOutcome,
                        ts_ms,
                        &payload,
                    );
                    let record = LedgerRecord {
                        id,
                        principal: format!("signal:{}", s.engineer),
                        effect: Effect::MetricOutcome,
                        causal_parent: None,
                        ts_ms,
                        payload,
                    };
                    if store_mut.append(&record).is_ok() {
                        written += 1;
                    }
                }
                eprintln!("Ingested {written} signal score(s) into ledger.");
            }
        }

        Commands::Rework { days, json } => {
            let hotspots = find_rework_hotspots(&store, days)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hotspots)?);
            } else if hotspots.is_empty() {
                println!("No rework hotspots detected in the last {} days.", days);
            } else {
                println!("{} rework hotspot(s) (last {} days):\n", hotspots.len(), days);
                for h in &hotspots {
                    println!("  ⚠ {}  ({} sessions, {:.0}h window)", h.file, h.session_count, h.window_hours);
                    for g in &h.session_goals {
                        println!("    → {:?}", g);
                    }
                    println!();
                }
            }
        }

        Commands::Patterns { min_sessions, min_score, json } => {
            let scores = score_sessions(&store)?;
            let patterns = build_pattern_library(&scores, min_sessions, min_score);
            let anti = antipatterns(&scores, min_sessions);

            if json {
                let out = serde_json::json!({ "top_patterns": patterns, "antipatterns": anti });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Top goal patterns (avg score ≥ {min_score}, ≥ {min_sessions} sessions):\n");
                for (i, p) in patterns.iter().enumerate() {
                    println!("  {}. {:?}  →  avg {:.0}/100, {:.1} commits/session  ({} sessions)",
                        i + 1, p.pattern, p.avg_score, p.avg_commits, p.session_count);
                    println!("     e.g. {:?}", p.example);
                }
                if !anti.is_empty() {
                    println!("\nPatterns to avoid:\n");
                    for p in &anti {
                        println!("  ✗ {:?}  →  avg {:.0}/100, {:.1} commits/session  ({} sessions)",
                            p.pattern, p.avg_score, p.avg_commits, p.session_count);
                        println!("    e.g. {:?}", p.example);
                    }
                }
            }
        }

        Commands::Weekly { from, to, slack_webhook, json } => {
            let from_ms = from.as_deref()
                .and_then(|s| axon_ledger::ingest::session::parse_iso_to_ms(s))
                .unwrap_or_else(week_ago_ms);
            let to_ms = to.as_deref()
                .and_then(|s| axon_ledger::ingest::session::parse_iso_to_ms(s))
                .unwrap_or_else(now_ms);

            let report = generate_weekly(&store, from_ms, to_ms, 14)?;

            if json {
                println!("{}", serde_json::to_string_pretty(&report)?);
            } else {
                print!("{}", render_text(&report));
            }

            if let Some(webhook_url) = slack_webhook {
                let text = render_text(&report);
                let payload = serde_json::json!({ "text": text });
                let payload_str = serde_json::to_string(&payload)?;
                let status = std::process::Command::new("curl")
                    .args([
                        "-s", "-o", "/dev/null", "-w", "%{http_code}",
                        "-X", "POST",
                        "-H", "Content-Type: application/json",
                        "-d", &payload_str,
                        &webhook_url,
                    ])
                    .output();
                match status {
                    Ok(out) => {
                        let code = String::from_utf8_lossy(&out.stdout);
                        if code.trim() == "200" {
                            eprintln!("Weekly report posted to Slack.");
                        } else {
                            eprintln!("Slack webhook returned HTTP {code} — check the URL.");
                        }
                    }
                    Err(e) => eprintln!("Failed to post to Slack (curl not found?): {e}"),
                }
            }
        }

        Commands::Goals { days, json } => {
            let cutoff = now_ms().saturating_sub(days * 24 * 60 * 60 * 1000);
            let mut scores = score_sessions(&store)?;
            scores.retain(|s| s.ts_ms >= cutoff);

            // Group by engineer
            let mut by_eng: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
            for s in &scores {
                by_eng.entry(s.engineer.clone()).or_default().push(s.goal_clarity);
            }
            let mut eng_summary: Vec<(String, f64, usize, u8, u8)> = by_eng.into_iter().map(|(eng, clarities)| {
                let avg = clarities.iter().map(|&c| c as f64).sum::<f64>() / clarities.len() as f64;
                let max = clarities.iter().copied().max().unwrap_or(0);
                let min = clarities.iter().copied().min().unwrap_or(0);
                (eng, avg, clarities.len(), max, min)
            }).collect();
            eng_summary.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            if json {
                let out_val = serde_json::json!(eng_summary.iter().map(|(eng, avg, count, max, min)| serde_json::json!({
                    "engineer": eng, "avg_goal_clarity": avg, "sessions": count, "max": max, "min": min
                })).collect::<Vec<_>>());
                println!("{}", serde_json::to_string_pretty(&out_val)?);
            } else {
                println!("Goal clarity by engineer (last {} days):\n", days);
                println!("  {:40}  {:>5}  {:>7}  {:>4}  {:>4}",
                    "engineer", "avg", "sessions", "best", "worst");
                println!("  {}", "-".repeat(67));
                for (eng, avg, count, max, min) in &eng_summary {
                    let bar = "█".repeat((*avg as usize) / 10)
                        + &"░".repeat(10usize.saturating_sub(*avg as usize / 10));
                    println!("  {bar}  {avg:>4.0}/100  {count:>3} sessions  best:{max:>3}  worst:{min:>3}  {eng}");
                }

                // Team recommendation
                let low: Vec<_> = eng_summary.iter().filter(|(_, avg, _, _, _)| *avg < 50.0).collect();
                if !low.is_empty() {
                    println!("\n  Recommendation: {} engineer(s) avg below 50 — suggest adding file refs and measurable outcomes to session goals.", low.len());
                }
            }
        }

        Commands::Loops { turns_threshold, days, json } => {
            let cutoff = now_ms().saturating_sub(days * 24 * 60 * 60 * 1000);
            let scores = score_sessions(&store)?;

            // Flag sessions with high turns and low commits — would benefit from a loop
            let mut candidates: Vec<_> = scores.iter()
                .filter(|s| s.ts_ms >= cutoff)
                .filter(|s| s.turns >= turns_threshold)
                .collect();
            candidates.sort_by(|a, b| b.turns.cmp(&a.turns));

            if json {
                let out_val = serde_json::json!(candidates.iter().map(|s| serde_json::json!({
                    "session_id": s.session_id,
                    "engineer": s.engineer,
                    "goal": s.goal,
                    "turns": s.turns,
                    "commits_linked": s.commits_linked,
                    "turns_per_commit": s.turns_per_commit,
                    "score": s.score,
                    "recommendation": if s.commits_linked == 0 {
                        "High turn count with no commits — consider /loop to iterate automatically"
                    } else if s.turns_per_commit > 40.0 {
                        "High turns-per-commit — a /loop workflow could have automated the iteration"
                    } else {
                        "Possible loop candidate"
                    }
                })).collect::<Vec<_>>());
                println!("{}", serde_json::to_string_pretty(&out_val)?);
            } else if candidates.is_empty() {
                println!("No loop-opportunity sessions found (last {} days, turns > {}).", days, turns_threshold);
            } else {
                println!("Loop opportunity candidates (last {} days, turns > {}):\n", days, turns_threshold);
                for s in &candidates {
                    let goal_trunc = if s.goal.chars().count() > 55 {
                        let end = s.goal.char_indices().nth(55).map(|(i,_)|i).unwrap_or(s.goal.len());
                        format!("{}…", &s.goal[..end])
                    } else { s.goal.clone() };
                    println!("  {} turns → {} commits  {} turns/commit",
                        s.turns, s.commits_linked,
                        if s.commits_linked > 0 { format!("{:.0}", s.turns_per_commit) } else { "∞".to_string() });
                    println!("  {}", goal_trunc);
                    if s.commits_linked == 0 {
                        println!("  → try: /loop <your-goal>");
                    } else {
                        println!("  → try: /loop 5m <your-goal>  (automate the iteration)");
                    }
                    println!();
                }
            }
        }

        Commands::ExportTraining { min_score, since, format, anonymize, exclude_code_content, out } => {
            let since_ms = since.as_deref()
                .and_then(|s| axon_ledger::ingest::session::parse_iso_to_ms(s));
            let fmt = match format.as_str() {
                "dpo" => ExportFormat::Dpo,
                _ => ExportFormat::Trainloop,
            };
            let opts = ExportOptions { format: fmt, min_score, since_ms, anonymize, exclude_code_content };

            let mut writer: Box<dyn std::io::Write> = match &out {
                Some(path) => Box::new(std::fs::File::create(path)?),
                None => Box::new(std::io::stdout()),
            };

            let n = match fmt {
                ExportFormat::Dpo => export_dpo(&store, &opts, &mut writer)?,
                ExportFormat::Trainloop => export_training(&store, &opts, &mut writer)?,
            };

            eprintln!("Exported {} training record(s) (min score: {}, format: {}).", n, min_score, format);
        }
    }

    Ok(())
}
