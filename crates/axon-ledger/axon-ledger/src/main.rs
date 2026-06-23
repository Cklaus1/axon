use std::fs;
use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand};

use axon_ledger::ingest::edge::infer_edges;
use axon_ledger::ingest::git::ingest_git;
use axon_ledger::ingest::session::ingest_session;
use axon_ledger::query::{diff, why};
use axon_ledger::store::Store;

#[derive(Parser)]
#[command(name = "axon-ledger", about = "Provenance ledger for Axon")]
struct Cli {
    /// Directory for ledger data (default: $HOME/.axon/ledger)
    #[arg(long, global = true)]
    ledger_dir: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Ingest data into the ledger
    Ingest {
        #[command(subcommand)]
        source: IngestSource,
    },
    /// Explain why a commit happened
    Why {
        /// SHA prefix of the commit
        sha: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// List records in a time window
    Diff {
        /// Start time (ISO 8601)
        #[arg(long)]
        from: String,
        /// End time (ISO 8601)
        #[arg(long)]
        to: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Show ledger statistics
    Stats {
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum IngestSource {
    /// Ingest git commits from a repository
    Git {
        /// Path to the git repository
        #[arg(long)]
        repo: PathBuf,
    },
    /// Ingest a single Claude Code session JSONL file
    Session {
        /// Path to the session JSONL file
        path: PathBuf,
    },
    /// Ingest all session JSONL files in a directory
    SessionDir {
        /// Directory containing session JSONL files
        dir: PathBuf,
    },
    /// Infer agent→commit edges from existing sessions and commits
    Edges,
}

fn default_ledger_dir() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
    PathBuf::from(home).join(".axon").join("ledger")
}

fn ledger_dir(cli_override: Option<&PathBuf>) -> PathBuf {
    cli_override
        .cloned()
        .unwrap_or_else(default_ledger_dir)
}

fn parse_iso_cli(s: &str) -> Result<u64> {
    axon_ledger::ingest::session::parse_iso_to_ms(s)
        .ok_or_else(|| anyhow::anyhow!("Could not parse timestamp: {}", s))
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let dir = ledger_dir(cli.ledger_dir.as_ref());
    let mut store = Store::open(&dir)?;

    match cli.command {
        Commands::Ingest { source } => match source {
            IngestSource::Git { repo } => {
                let n = ingest_git(&repo, &mut store)?;
                println!("Ingested {} new git commits.", n);
            }
            IngestSource::Session { path } => {
                match ingest_session(&path, &mut store)? {
                    Some(r) => println!("Ingested session: {}", r.id),
                    None => println!("Session already in ledger, skipped."),
                }
            }
            IngestSource::SessionDir { dir: sessions_dir } => {
                let mut total = 0usize;
                let entries = fs::read_dir(&sessions_dir)?;
                for entry in entries {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl")
                        && ingest_session(&path, &mut store)?.is_some()
                    {
                        total += 1;
                    }
                }
                println!("Ingested {} new sessions.", total);
            }
            IngestSource::Edges => {
                let n = infer_edges(&mut store)?;
                println!("Inferred {} new edges.", n);
            }
        },

        Commands::Why { sha, json } => {
            let result = why(&sha, &store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("Commit:  {}", result.commit.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("?"));
                println!("Message: {}", result.commit.payload.get("message").and_then(|v| v.as_str()).unwrap_or("?"));
                println!("Author:  {}", result.commit.payload.get("author").and_then(|v| v.as_str()).unwrap_or("?"));
                if let Some(session) = &result.agent_session {
                    println!(
                        "Session: {}",
                        session.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("?")
                    );
                    println!(
                        "  Files: {}",
                        session
                            .payload
                            .get("files_touched")
                            .and_then(|v| v.as_array())
                            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>().join(", "))
                            .unwrap_or_default()
                    );
                } else {
                    println!("Session: (none found)");
                }
                if result.outcomes.is_empty() {
                    println!("Outcomes: (none)");
                } else {
                    println!("Outcomes ({}):", result.outcomes.len());
                    for o in &result.outcomes {
                        println!("  - {}", o.id);
                    }
                }
            }
        }

        Commands::Diff { from, to, json } => {
            let t1 = parse_iso_cli(&from)?;
            let t2 = parse_iso_cli(&to)?;
            let records = diff(t1, t2, &store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&records)?);
            } else {
                println!("{} records in window [{}, {}]:", records.len(), from, to);
                for r in &records {
                    println!("  {} {:?} {}", r.ts_ms, r.effect, r.principal);
                }
            }
        }

        Commands::Stats { json } => {
            let all = store.all()?;
            let git_count = all.iter().filter(|r| r.effect == axon_ledger::model::Effect::GitCommit).count();
            let session_count = all.iter().filter(|r| r.effect == axon_ledger::model::Effect::AgentSession).count();
            let edge_count = all.iter().filter(|r| r.effect == axon_ledger::model::Effect::AgentEdge).count();
            let outcome_count = all.iter().filter(|r| r.effect == axon_ledger::model::Effect::MetricOutcome).count();

            if json {
                let stats = serde_json::json!({
                    "total": all.len(),
                    "git_commits": git_count,
                    "agent_sessions": session_count,
                    "agent_edges": edge_count,
                    "metric_outcomes": outcome_count,
                });
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Ledger stats:");
                println!("  Total records:    {}", all.len());
                println!("  Git commits:      {}", git_count);
                println!("  Agent sessions:   {}", session_count);
                println!("  Agent edges:      {}", edge_count);
                println!("  Metric outcomes:  {}", outcome_count);
            }
        }
    }

    Ok(())
}
