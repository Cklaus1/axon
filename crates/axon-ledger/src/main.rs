use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

use axon_ledger::ingest::edge::infer_edges;
use axon_ledger::ingest::git::ingest_git;
use axon_ledger::ingest::outcome::ingest_outcome;
use axon_ledger::ingest::session::{ingest_session, GateOptions};
use axon_ledger::query::{diff, why};
use axon_ledger::store::Store;
use axon_ledger::watch::watch_sessions;

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
    /// Watch a directory for new Claude Code sessions and auto-ingest them
    Watch {
        /// Directory to watch for .jsonl session files
        #[arg(long)]
        dir: PathBuf,
        /// Poll interval in seconds (default: 60)
        #[arg(long, default_value = "60")]
        interval: u64,
        /// Run the brief gate on ingested sessions
        #[arg(long)]
        gate: bool,
        /// Explicit path to the axon binary
        #[arg(long)]
        axon_bin: Option<PathBuf>,
        /// Explicit path to brief-gate.ax
        #[arg(long)]
        gate_script: Option<PathBuf>,
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
        /// Run the brief gate on the session's extracted goal before ingesting
        #[arg(long)]
        gate: bool,
        /// Explicit path to the axon binary (auto-discovered if omitted)
        #[arg(long)]
        axon_bin: Option<PathBuf>,
        /// Explicit path to brief-gate.ax (auto-discovered if omitted)
        #[arg(long)]
        gate_script: Option<PathBuf>,
    },
    /// Ingest all session JSONL files in a directory
    SessionDir {
        /// Directory containing session JSONL files
        dir: PathBuf,
        /// Run the brief gate on each session's extracted goal before ingesting
        #[arg(long)]
        gate: bool,
        /// Explicit path to the axon binary (auto-discovered if omitted)
        #[arg(long)]
        axon_bin: Option<PathBuf>,
        /// Explicit path to brief-gate.ax (auto-discovered if omitted)
        #[arg(long)]
        gate_script: Option<PathBuf>,
    },
    /// Infer agent->commit edges from existing sessions and commits
    Edges,
    /// Ingest a metric outcome JSON file and link it to a commit
    Outcome {
        /// SHA prefix of the commit this outcome belongs to
        #[arg(long)]
        commit: String,
        /// Path to a JSON file containing metric key/value pairs
        #[arg(long)]
        file: PathBuf,
    },
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
    let dir_path = dir.clone();
    let mut store = Store::open(&dir)?;

    match cli.command {
        Commands::Ingest { source } => match source {
            IngestSource::Git { repo } => {
                let n = ingest_git(&repo, &mut store)?;
                println!("Ingested {} new git commits.", n);
            }
            IngestSource::Session { path, gate, axon_bin, gate_script } => {
                let gate_opts = GateOptions { enabled: gate, axon_bin, gate_script };
                match ingest_session(&path, &mut store, &gate_opts)? {
                    Some(r) => println!("Ingested session: {}", r.id),
                    None => println!("Session already in ledger, skipped."),
                }
            }
            IngestSource::SessionDir { dir: sessions_dir, gate, axon_bin, gate_script } => {
                let gate_opts = GateOptions { enabled: gate, axon_bin: axon_bin.clone(), gate_script: gate_script.clone() };
                let mut total = 0usize;
                let mut rejected = 0usize;
                let entries = fs::read_dir(&sessions_dir)?;
                for entry in entries {
                    let entry = entry?;
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                        continue;
                    }
                    match ingest_session(&path, &mut store, &gate_opts) {
                        Ok(Some(_)) => total += 1,
                        Ok(None) => {}
                        Err(e) if e.to_string().starts_with("brief gate:") => {
                            eprintln!("[ledger] skipped {}: {}", path.display(), e);
                            rejected += 1;
                        }
                        Err(e) => return Err(e),
                    }
                }
                println!("Ingested {} new sessions ({} rejected by brief gate).", total, rejected);
            }
            IngestSource::Edges => {
                let n = infer_edges(&mut store)?;
                println!("Inferred {} new edges.", n);
            }
            IngestSource::Outcome { commit, file } => {
                let r = ingest_outcome(&commit, &file, &mut store)?;
                println!("Ingested outcome: {} (linked to commit {})", r.id, commit);
            }
        },

        Commands::Why { sha, json } => {
            let result = why(&sha, &store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                let p = &result.commit.payload;
                let commit_sha  = p.get("sha").and_then(|v| v.as_str()).unwrap_or("?");
                let msg         = p.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                let author      = p.get("author").and_then(|v| v.as_str()).unwrap_or("?");
                let files: Vec<&str> = p.get("files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                    .unwrap_or_default();

                println!("COMMIT   {}", &commit_sha[..commit_sha.len().min(12)]);
                println!("  msg:   {}", msg);
                println!("  by:    {}", author);
                println!("  files: {}", if files.is_empty() { "(none)".to_string() } else { files.join("  ") });
                println!();

                if let Some(session) = &result.agent_session {
                    let sp = &session.payload;
                    let sid     = sp.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
                    let goal    = sp.get("goal").and_then(|v| v.as_str()).unwrap_or("(no goal extracted)");
                    let start   = sp.get("start_ts").and_then(|v| v.as_str()).unwrap_or("?");
                    let turns   = sp.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                    let sfiles: Vec<&str> = sp.get("files_touched")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter().filter_map(|v| v.as_str()).collect())
                        .unwrap_or_default();
                    let conf_str = result.edge.as_ref()
                        .and_then(|e| e.payload.get("confidence"))
                        .map(|v| {
                            if let Some(f) = v.as_f64() {
                                format!("{:.2}", f)
                            } else {
                                v.as_str().unwrap_or("?").to_string()
                            }
                        })
                        .unwrap_or_else(|| "?".to_string());

                    println!("AGENT SESSION  {} (inferred, confidence {})", &sid[..sid.len().min(8)], conf_str);
                    println!("  goal:   {}", goal);
                    println!("  start:  {}", start);
                    println!("  turns:  {}", turns);
                    if !sfiles.is_empty() {
                        println!("  files:  {}", sfiles[..sfiles.len().min(8)].join("  "));
                    }
                } else {
                    println!("AGENT SESSION  (none — not found in ledger or outside inference window)");
                }
                println!();

                if result.outcomes.is_empty() {
                    println!("OUTCOMES  (none recorded)");
                } else {
                    println!("OUTCOMES ({}):", result.outcomes.len());
                    for o in &result.outcomes {
                        let file = o.payload.get("file").and_then(|v| v.as_str()).unwrap_or(&o.id);
                        println!("  {}", file);
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

        Commands::Watch { dir, interval, gate, axon_bin, gate_script } => {
            let gate_opts = GateOptions { enabled: gate, axon_bin, gate_script };
            println!("[ledger-watch] polling {} every {}s  (Ctrl-C to stop)", dir.display(), interval);
            watch_sessions(&dir, &dir_path, Duration::from_secs(interval), &gate_opts, true)?;
        }
    }

    Ok(())
}
