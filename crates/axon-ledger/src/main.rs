use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, Subcommand};

use axon_ledger::ingest::edge::infer_edges;
use axon_ledger::ingest::git::ingest_git;
use axon_ledger::ingest::outcome::ingest_outcome;
use axon_ledger::ingest::session::{ingest_session, GateOptions};
use axon_ledger::mcp::run_mcp_server;
use axon_ledger::query::{as_of, diff, search, why};
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
    /// Search commits and sessions by keyword
    Search {
        /// One or more search terms (all must match)
        #[arg(required = true)]
        terms: Vec<String>,
        /// Maximum results to show (default: 10)
        #[arg(long, default_value = "10")]
        limit: usize,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Reconstruct what was known/shipped at a point in time
    AsOf {
        /// ISO 8601 timestamp (e.g. 2026-06-19T00:00:00Z)
        timestamp: String,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Start an MCP (Model Context Protocol) server over stdio
    Mcp,
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

/// Shorten an absolute path to at most `max_components` trailing components.
/// "/home/user/project/crates/foo/src/bar.rs" → "crates/foo/src/bar.rs" (max 4)
fn short_path(path: &str, max_components: usize) -> String {
    // Strip leading ./
    let path = path.trim_start_matches("./");
    let parts: Vec<&str> = path.split('/').collect();
    if parts.len() <= max_components {
        path.to_string()
    } else {
        parts[parts.len() - max_components..].join("/")
    }
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
                let files: Vec<String> = p.get("files")
                    .and_then(|v| v.as_array())
                    .map(|a| a.iter().filter_map(|v| v.as_str()).map(|f| short_path(f, 4)).collect())
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
                    let sfiles: Vec<String> = sp.get("files_touched")
                        .and_then(|v| v.as_array())
                        .map(|a| a.iter()
                            .filter_map(|v| v.as_str())
                            .filter(|f| {
                                !f.contains('*') && !f.starts_with('-')
                                    && !f.ends_with(".jsonl") // session metadata, not source
                            })
                            .map(|f| short_path(f, 3))
                            .collect())
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
                        // Dedup after shortening, show up to 6
                        let mut seen = std::collections::HashSet::new();
                        let deduped: Vec<&String> = sfiles.iter()
                            .filter(|f| seen.insert(f.as_str()))
                            .take(6)
                            .collect();
                        println!("  files:  {}", deduped.iter().map(|s| s.as_str()).collect::<Vec<_>>().join("  "));
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

        Commands::Search { terms, limit, json } => {
            let query = terms.join(" ");
            let hits = search(&query, &store, limit)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&hits)?);
            } else if hits.is_empty() {
                println!("No results for {:?}", query);
            } else {
                println!("{} result(s) for {:?}:\n", hits.len(), query);
                for hit in &hits {
                    match hit.record.effect {
                        axon_ledger::model::Effect::GitCommit => {
                            let sha = hit.record.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("?");
                            let author = hit.record.payload.get("author").and_then(|v| v.as_str()).unwrap_or("?");
                            let msg = hit.record.payload.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("COMMIT  {}  ({})", &sha[..sha.len().min(10)], author);
                            println!("  msg:  {}", msg);
                            if hit.matched_field == "commit.file" {
                                println!("  file: {}", short_path(&hit.matched_text, 4));
                            }
                            if let Some(goal) = &hit.session_goal {
                                println!("  why:  {}", goal);
                            }
                        }
                        axon_ledger::model::Effect::AgentSession => {
                            let sid = hit.record.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
                            let turns = hit.record.payload.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                            let start = hit.record.payload.get("start_ts").and_then(|v| v.as_str()).unwrap_or("?");
                            let goal = hit.record.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("?");
                            println!("SESSION {}  ({} turns, {})", &sid[..sid.len().min(8)], turns, &start[..start.len().min(10)]);
                            println!("  goal: {}", goal);
                            if hit.matched_field == "session.file" {
                                println!("  file: {}", short_path(&hit.matched_text, 3));
                            }
                        }
                        _ => {
                            println!("{:?}  {}", hit.record.effect, hit.record.id);
                        }
                    }
                    println!();
                }
            }
        }

        Commands::AsOf { timestamp, json } => {
            let ts_ms = parse_iso_cli(&timestamp)?;
            let result = as_of(ts_ms, &store)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("SNAPSHOT AT  {}", result.ts_iso);
                println!("  commits:  {} total", result.commit_count);
                println!("  sessions: {} total", result.session_count);
                println!();

                println!("RECENT COMMITS ({}):", result.recent_commits.len());
                for c in &result.recent_commits {
                    let sha = c.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("?");
                    let msg = c.payload.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                    println!("  {}  {}", &sha[..sha.len().min(10)], msg);
                }
                println!();

                if result.active_sessions.is_empty() {
                    println!("ACTIVE SESSIONS  (none at this timestamp)");
                } else {
                    println!("ACTIVE SESSIONS ({}):", result.active_sessions.len());
                    for s in &result.active_sessions {
                        let sid = s.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("?");
                        let goal = s.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("?");
                        let turns = s.payload.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                        println!("  {} ({} turns)", &sid[..sid.len().min(8)], turns);
                        println!("    goal: {}", goal);
                    }
                }
                println!();

                if !result.files_in_flight.is_empty() {
                    println!("FILES IN FLIGHT:");
                    for f in &result.files_in_flight {
                        println!("  {}", f);
                    }
                }
            }
        }

        Commands::Mcp => {
            let store = Store::open(&dir_path)?;
            run_mcp_server(store)?;
        }

        Commands::Watch { dir, interval, gate, axon_bin, gate_script } => {
            let gate_opts = GateOptions { enabled: gate, axon_bin, gate_script };
            println!("[ledger-watch] polling {} every {}s  (Ctrl-C to stop)", dir.display(), interval);
            watch_sessions(&dir, &dir_path, Duration::from_secs(interval), &gate_opts, true)?;
        }
    }

    Ok(())
}
