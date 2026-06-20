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
        Commands::Score { engineer, week, json } => {
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

        Commands::Weekly { from, to, slack_webhook: _, json } => {
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
