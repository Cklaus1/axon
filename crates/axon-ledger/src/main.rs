use std::fs;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};

use axon_ledger::ingest::edge::infer_edges;
use axon_ledger::ingest::git::ingest_git;
use axon_ledger::ingest::outcome::ingest_outcome;
use axon_ledger::ingest::session::{ingest_session, GateOptions};
use axon_ledger::mcp::run_mcp_server;
use axon_ledger::query::{as_of, diff, history, search, why};
use axon_ledger::rbac::{resolve_caller, RbacConfig};
use axon_ledger::store::Store;
use axon_ledger::watch::watch_sessions;
use axon_ledger::webhook::{add_webhook, fire_webhooks, load_webhooks, remove_webhook,
                           WebhookEvent, WebhookProvider};

#[derive(Parser)]
#[command(name = "axon-ledger", about = "Provenance ledger for Axon")]
struct Cli {
    /// Directory for ledger data (default: $HOME/.axon/ledger)
    #[arg(long, global = true)]
    ledger_dir: Option<PathBuf>,

    /// Caller identity (email) for RBAC filtering. Overrides AXON_PRINCIPAL env var.
    #[arg(long = "as", global = true)]
    caller: Option<String>,

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
        /// Filter to a specific repo name (multi-repo ledger)
        #[arg(long)]
        repo: Option<String>,
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
        /// Filter to a specific repo name (multi-repo ledger)
        #[arg(long)]
        repo: Option<String>,
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
    /// Show the AI session history of a file — who worked on it, with what goal, producing which commits
    History {
        /// File path (suffix matching: "auth/jwt.rs" matches "src/auth/jwt.rs")
        file: String,
        /// Filter to a specific repo name (multi-repo ledger)
        #[arg(long)]
        repo: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Flag unexplained commits before a deploy (commits with no linked AI session)
    PreDeploy {
        /// Git commit range, e.g. HEAD~8..HEAD or sha1..sha2 (default: HEAD~10..HEAD)
        range: Option<String>,
        /// Path to the git repo (default: current directory)
        #[arg(long, default_value = ".")]
        repo: PathBuf,
        /// Fail (exit 1) if any unexplained commits are found
        #[arg(long)]
        fail_on_unexplained: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Weekly digest — sessions, commits, top goals, and rework hotspots for a time window
    Weekly {
        /// Start of window (ISO 8601 or relative: "7 days ago"; default: 7 days ago)
        #[arg(long)]
        from: Option<String>,
        /// End of window (ISO 8601 or relative; default: now)
        #[arg(long)]
        to: Option<String>,
        /// Filter to a specific repo name (multi-repo ledger)
        #[arg(long)]
        repo: Option<String>,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Compliance query — list all sessions and commits that touched files under a module path
    Audit {
        /// Module path prefix, e.g. "payments/" or "src/auth"
        #[arg(long)]
        module: String,
        /// Only include records after this date (ISO 8601 or "90 days ago")
        #[arg(long)]
        since: Option<String>,
        /// Filter to a specific repo name (multi-repo ledger)
        #[arg(long)]
        repo: Option<String>,
        /// Output as JSON (machine-readable)
        #[arg(long)]
        json: bool,
    },
    /// Delete records older than a threshold (GDPR / data minimization)
    Prune {
        /// Delete records older than this ISO 8601 date or relative duration (e.g. "90 days ago", "2025-01-01")
        #[arg(long)]
        older_than: String,
        /// Dry run — show what would be deleted without changing the ledger
        #[arg(long)]
        dry_run: bool,
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
    /// Backfill engineer identity for sessions ingested before --engineer was available.
    ///
    /// Rewrites all records whose principal starts with "agent:" (the anonymous fallback)
    /// to the given email address. Auto-detects from `git config user.email` when omitted.
    ///
    /// Example: axon-ledger engineer-backfill --email chris@example.com
    ///          axon-ledger engineer-backfill          # auto-detect from git config
    EngineerBackfill {
        /// Engineer email to assign (default: auto-detected from `git config user.email`)
        #[arg(long)]
        email: Option<String>,
        /// Dry run — show counts without modifying the ledger
        #[arg(long)]
        dry_run: bool,
    },
    /// Re-read session JSONL files and refresh stale ledger fields (turn_count, files_touched).
    ///
    /// Only updates sessions whose turn_count is 0 (ingested before turn counting existed).
    /// Use this after upgrading axon-ledger if old sessions show "0 turns" in reports.
    ///
    /// Example: axon-ledger session-refresh ~/.claude/projects/-home-user-myrepo/
    SessionRefresh {
        /// Directory containing Claude Code session .jsonl files
        dir: PathBuf,
        /// Dry run — show what would be updated without changing the ledger
        #[arg(long)]
        dry_run: bool,
    },
    /// Manage webhook egress — notify Slack or PagerDuty when events occur
    Webhook {
        #[command(subcommand)]
        action: WebhookAction,
    },
    /// Manage RBAC — control who can view which records
    Rbac {
        #[command(subcommand)]
        action: RbacAction,
    },
    /// One-shot ledger refresh: ingest git commits, Claude Code sessions, then infer edges.
    ///
    /// Equivalent to running these three commands in sequence:
    ///   axon-ledger ingest git [--repo <path>]
    ///   axon-ledger ingest session-dir <session-dir> [--engineer <email>]
    ///   axon-ledger ingest edges
    ///
    /// Designed for the common post-work-session workflow: run this once and
    /// `axon-ledger pre-deploy` will reflect the new commits and sessions.
    ///
    /// Example:
    ///   axon-ledger refresh
    ///   axon-ledger refresh --repo /path/to/repo --session-dir ~/.claude/projects/myrepo/
    Refresh {
        /// Git repo to ingest (default: current directory)
        #[arg(long)]
        repo: Option<PathBuf>,
        /// Claude Code session directory (default: auto-detected from ~/.claude/projects/)
        #[arg(long)]
        session_dir: Option<PathBuf>,
        /// Engineer email for sessions (default: auto-detected from git config user.email)
        #[arg(long)]
        engineer: Option<String>,
        /// Only ingest commits since this ISO 8601 date (passed to ingest git)
        #[arg(long)]
        since: Option<String>,
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
        /// Engineer email/name to attribute watched sessions to (falls back to git config user.email)
        #[arg(long)]
        engineer: Option<String>,
    },
}

#[derive(Subcommand)]
enum IngestSource {
    /// Ingest git commits from a repository
    Git {
        /// Path to the git repository
        #[arg(long)]
        repo: PathBuf,
        /// Only ingest commits after this date/time (ISO 8601 or relative like "30 days ago")
        #[arg(long)]
        since: Option<String>,
        /// Tag all ingested records with this repo name (e.g. "api", "frontend") — enables multi-repo filtering
        #[arg(long)]
        repo_name: Option<String>,
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
        /// Tag this session with a repo name (e.g. "api", "frontend") — enables multi-repo filtering
        #[arg(long)]
        repo_name: Option<String>,
        /// Engineer email/name to attribute these sessions to (e.g. alice@example.com)
        #[arg(long)]
        engineer: Option<String>,
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
        /// Tag all sessions with a repo name (e.g. "api", "frontend") — enables multi-repo filtering
        #[arg(long)]
        repo_name: Option<String>,
        /// Engineer email/name to attribute these sessions to (e.g. alice@example.com).
        /// If omitted, falls back to git config user.email in the current directory.
        #[arg(long)]
        engineer: Option<String>,
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
    /// Ingest an observability provider payload (Datadog, Sentry, PostHog, or generic JSON)
    OutcomeProvider {
        /// Provider: datadog, sentry, posthog, generic
        #[arg(long)]
        provider: String,
        /// SHA prefix of the commit this outcome belongs to
        #[arg(long)]
        commit: String,
        /// Path to the provider JSON payload (webhook body or API export)
        #[arg(long)]
        file: PathBuf,
        /// Output as JSON
        #[arg(long)]
        json: bool,
    },
}

#[derive(Subcommand)]
enum WebhookAction {
    /// Register a new webhook
    Add {
        /// Event to subscribe to: unexplained-deploy
        #[arg(long)]
        event: String,
        /// Provider: slack, pagerduty, generic
        #[arg(long)]
        provider: String,
        /// Webhook URL (Slack Incoming Webhook or PagerDuty Events API v2 endpoint)
        #[arg(long)]
        url: String,
    },
    /// List registered webhooks
    List,
    /// Remove a webhook by id
    Rm {
        /// Webhook id (from webhook list)
        id: String,
    },
}

#[derive(Subcommand)]
enum RbacAction {
    /// Grant admin role to an engineer (can view all records)
    Grant {
        /// Engineer email address
        email: String,
    },
    /// Revoke admin role from an engineer
    Revoke {
        /// Engineer email address
        email: String,
    },
    /// List current RBAC config
    List,
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
    let rbac = RbacConfig::load(&dir_path)?;
    let caller = resolve_caller(cli.caller.as_deref());

    match cli.command {
        Commands::Ingest { source } => match source {
            IngestSource::Git { repo, since, repo_name } => {
                let n = ingest_git(&repo, &mut store, since.as_deref(), repo_name.as_deref())?;
                let filter_note = since.as_deref()
                    .map(|s| format!(" (after {})", s))
                    .unwrap_or_default();
                let repo_note = repo_name.as_deref()
                    .map(|r| format!(" [repo={}]", r))
                    .unwrap_or_default();
                println!("Ingested {} new git commits{}{}.", n, filter_note, repo_note);
            }
            IngestSource::Session { path, gate, axon_bin, gate_script, repo_name, engineer } => {
                let gate_opts = GateOptions { enabled: gate, axon_bin, gate_script };
                match ingest_session(&path, &mut store, &gate_opts, repo_name.as_deref(), engineer.as_deref())? {
                    Some(r) => println!("Ingested session: {}", r.id),
                    None => println!("Session already in ledger, skipped."),
                }
            }
            IngestSource::SessionDir { dir: sessions_dir, gate, axon_bin, gate_script, repo_name, engineer } => {
                let gate_opts = GateOptions { enabled: gate, axon_bin: axon_bin.clone(), gate_script: gate_script.clone() };
                // If --engineer not given, fall back to git config user.email
                let resolved_engineer = engineer.or_else(|| {
                    std::process::Command::new("git")
                        .args(["config", "user.email"])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });
                let mut total = 0usize;
                let mut skipped = 0usize;
                let mut rejected = 0usize;
                // Collect and sort for deterministic ordering
                let mut paths: Vec<_> = fs::read_dir(&sessions_dir)?
                    .filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("jsonl"))
                    .collect();
                paths.sort();
                let n_files = paths.len();
                for (i, path) in paths.iter().enumerate() {
                    eprint!("\r  [{}/{}] {}", i + 1, n_files,
                        path.file_name().unwrap_or_default().to_string_lossy());
                    match ingest_session(path, &mut store, &gate_opts, repo_name.as_deref(), resolved_engineer.as_deref()) {
                        Ok(Some(_)) => total += 1,
                        Ok(None) => skipped += 1,
                        Err(e) if e.to_string().starts_with("brief gate:") => {
                            rejected += 1;
                        }
                        Err(e) => { eprintln!(); return Err(e); }
                    }
                }
                if n_files > 0 { eprintln!(); }
                let eng_note = resolved_engineer.as_deref()
                    .map(|e| format!(" [engineer={}]", e))
                    .unwrap_or_default();
                println!("Ingested {} new sessions ({} already known, {} rejected by gate){}.",
                    total, skipped, rejected, eng_note);
            }
            IngestSource::Edges => {
                let n = infer_edges(&mut store)?;
                println!("Inferred {} new edges.", n);
            }
            IngestSource::Outcome { commit, file } => {
                let r = ingest_outcome(&commit, &file, &mut store)?;
                println!("Ingested outcome: {} (linked to commit {})", r.id, commit);
            }
            IngestSource::OutcomeProvider { provider, commit, file, json } => {
                use axon_ledger::ingest::provider::{ingest_provider_outcome, Provider};
                let p = Provider::from_str(&provider)?;
                let r = ingest_provider_outcome(p, &commit, &file, &mut store)?;
                if json {
                    println!("{}", serde_json::to_string_pretty(&r)?);
                } else {
                    let metric_count = r.payload.get("metrics")
                        .and_then(|m| m.as_object()).map(|o| o.len()).unwrap_or(0);
                    println!("Ingested {} outcome: {} metric(s) linked to commit {}",
                        provider, metric_count, commit);
                }
            }
        },

        Commands::Why { sha, repo, json } => {
            let result = why(&sha, &store)?;
            if let Some(ref r) = repo {
                if result.commit.repo.as_deref() != Some(r.as_str()) {
                    anyhow::bail!("commit {} is not tagged with repo {:?} (actual: {:?})",
                        &sha, r, result.commit.repo);
                }
            }
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
            use axon_ledger::model::Effect;
            let all = rbac.filter_owned(store.all()?, caller.as_deref());
            let git_count = all.iter().filter(|r| r.effect == Effect::GitCommit).count();
            let session_count = all.iter().filter(|r| r.effect == Effect::AgentSession).count();
            let edge_count = all.iter().filter(|r| r.effect == Effect::AgentEdge).count();
            let outcome_count = all.iter().filter(|r| r.effect == Effect::MetricOutcome).count();

            // Coverage: unique commit SHAs that appear in at least one edge
            let linked_commit_shas: std::collections::HashSet<String> = all.iter()
                .filter(|r| r.effect == Effect::AgentEdge)
                .filter_map(|r| r.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
                .collect();
            let linked_commits = linked_commit_shas.len();
            let coverage_pct = if git_count > 0 {
                (linked_commits as f64 / git_count as f64 * 100.0).round() as u64
            } else {
                0
            };

            if json {
                let stats = serde_json::json!({
                    "total": all.len(),
                    "git_commits": git_count,
                    "agent_sessions": session_count,
                    "agent_edges": edge_count,
                    "metric_outcomes": outcome_count,
                    "linked_commits": linked_commits,
                    "coverage_pct": coverage_pct,
                });
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!("Ledger stats:");
                println!("  Total records:    {}", all.len());
                println!("  Git commits:      {}", git_count);
                println!("  Agent sessions:   {}", session_count);
                println!("  Agent edges:      {}", edge_count);
                println!("  Metric outcomes:  {}", outcome_count);
                println!("  Coverage:         {}/{} commits linked ({}%)", linked_commits, git_count, coverage_pct);
            }
        }

        Commands::Search { terms, limit, repo, json } => {
            let query = terms.join(" ");
            let mut hits = search(&query, &store, limit)?;
            if let Some(ref r) = repo {
                hits.retain(|h| h.record.repo.as_deref() == Some(r.as_str()));
            }
            // RBAC: member can only see their own session/commit records
            if !rbac.admins.is_empty() {
                hits.retain(|h| {
                    rbac.filter_visible(vec![&h.record], caller.as_deref()).len() == 1
                });
            }
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

        Commands::History { file, repo, json } => {
            let mut result = history(&file, &store)?;
            if let Some(ref r) = repo {
                result.chapters.retain(|ch| ch.session.repo.as_deref() == Some(r.as_str()));
                result.total_sessions = result.chapters.len();
                result.total_commits = result.chapters.iter().map(|ch| ch.commits.len()).sum();
            }
            if json {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else if result.chapters.is_empty() {
                println!("No AI sessions found for {:?}.", file);
                println!("(Try a shorter path suffix, e.g. \"jwt.rs\" instead of the full path)");
            } else {
                println!("HISTORY  {}", result.file);
                println!("  {} session(s)  ·  {} linked commit(s)\n", result.total_sessions, result.total_commits);
                for (i, ch) in result.chapters.iter().enumerate() {
                    let sid = ch.session.payload.get("session_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&ch.session.id[..ch.session.id.len().min(8)]);
                    let turns = ch.session.payload.get("turn_count")
                        .and_then(|v| v.as_u64()).unwrap_or(0);
                    let conf = ch.confidence.map(|c| format!(" confidence {:.2}", c)).unwrap_or_default();
                    println!("  Chapter {}  [{}]  {}{}",
                        i + 1, &sid[..sid.len().min(8)], &ch.date[..10], conf);
                    let goal = if ch.goal.chars().count() > 72 {
                        let end = ch.goal.char_indices().nth(72).map(|(i, _)| i).unwrap_or(ch.goal.len());
                        format!("{}…", &ch.goal[..end])
                    } else {
                        ch.goal.clone()
                    };
                    println!("  goal:   {}", goal);
                    println!("  turns:  {}", turns);
                    if ch.commits.is_empty() {
                        println!("  commits: (none linked to this file)");
                    } else {
                        for c in &ch.commits {
                            let sha = c.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("?");
                            let msg = c.payload.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                            let short_msg = if msg.chars().count() > 60 {
                                let end = msg.char_indices().nth(60).map(|(i, _)| i).unwrap_or(msg.len());
                                format!("{}…", &msg[..end])
                            } else { msg.to_string() };
                            println!("  commit: {}  {}", &sha[..sha.len().min(10)], short_msg);
                        }
                    }
                    println!();
                }
            }
        }

        Commands::PreDeploy { range, repo, fail_on_unexplained, json } => {
            let range = range.as_deref().unwrap_or("HEAD~10..HEAD");
            // Get SHAs in range from git
            let git_out = std::process::Command::new("git")
                .arg("-C").arg(&repo)
                .args(["log", "--format=%H|%ae|%s", range])
                .output()
                .context("Failed to run git log for pre-deploy check")?;
            if !git_out.status.success() {
                anyhow::bail!("git log failed: {}", String::from_utf8_lossy(&git_out.stderr));
            }
            let range_commits: Vec<(String, String, String)> = String::from_utf8_lossy(&git_out.stdout)
                .lines()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(3, '|').collect();
                    if parts.len() == 3 { Some((parts[0].to_string(), parts[1].to_string(), parts[2].to_string())) }
                    else { None }
                })
                .collect();

            // Build set of explained SHAs (appear in at least one edge)
            let all = store.all()?;
            use axon_ledger::model::Effect;
            let explained_shas: std::collections::HashSet<String> = all.iter()
                .filter(|r| r.effect == Effect::AgentEdge)
                .filter_map(|r| r.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
                .collect();

            // Session goals indexed by sha for display
            let mut sha_to_goal: std::collections::HashMap<String, String> = std::collections::HashMap::new();
            for edge in all.iter().filter(|r| r.effect == Effect::AgentEdge) {
                let sha = edge.payload.get("commit_sha").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let sid = edge.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                if let Some(session) = all.iter().find(|r| r.effect == Effect::AgentSession &&
                    r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("") == sid)
                {
                    let goal = session.payload.get("goal").and_then(|v| v.as_str())
                        .filter(|g| !g.starts_with('<'))
                        .unwrap_or("(no goal)").to_string();
                    sha_to_goal.entry(sha).or_insert(goal);
                }
            }

            let explained: Vec<_> = range_commits.iter()
                .filter(|(sha, _, _)| explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha.as_str())))
                .collect();
            let unexplained: Vec<_> = range_commits.iter()
                .filter(|(sha, _, _)| !explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha.as_str())))
                .collect();

            let coverage = if range_commits.is_empty() { 100u64 }
                else { (explained.len() * 100 / range_commits.len()) as u64 };

            if json {
                let out = serde_json::json!({
                    "range": range,
                    "total_commits": range_commits.len(),
                    "explained": explained.len(),
                    "unexplained": unexplained.len(),
                    "coverage_pct": coverage,
                    "unexplained_commits": unexplained.iter().map(|(sha, author, msg)| serde_json::json!({
                        "sha": sha, "author": author, "message": msg
                    })).collect::<Vec<_>>(),
                    "explained_commits": explained.iter().map(|(sha, author, msg)| {
                        let goal = sha_to_goal.get(sha.as_str()).cloned().unwrap_or_default();
                        serde_json::json!({ "sha": sha, "author": author, "message": msg, "session_goal": goal })
                    }).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Pre-deploy check  {range}\n");
                for (sha, _author, msg) in &range_commits {
                    let short = &sha[..sha.len().min(10)];
                    let is_explained = explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha.as_str()));
                    let msg_trunc = if msg.chars().count() > 55 {
                        let end = msg.char_indices().nth(55).map(|(i,_)|i).unwrap_or(msg.len());
                        format!("{}…", &msg[..end])
                    } else { msg.clone() };
                    if is_explained {
                        let goal = sha_to_goal.get(sha.as_str()).cloned()
                            .unwrap_or_else(|| "(goal)".to_string());
                        let goal_trunc = if goal.chars().count() > 50 {
                            let end = goal.char_indices().nth(50).map(|(i,_)|i).unwrap_or(goal.len());
                            format!("{}…", &goal[..end])
                        } else { goal };
                        println!("  ✓ {short}  {msg_trunc}");
                        println!("         goal: {goal_trunc}");
                    } else {
                        println!("  ⚠ {short}  {msg_trunc}");
                        println!("         NO LINKED SESSION");
                    }
                }
                println!("\n  {}/{} commits explained  ({}% coverage)",
                    explained.len(), range_commits.len(), coverage);
                if !unexplained.is_empty() {
                    println!("  {} unexplained commit(s) — review before deploy", unexplained.len());
                }
            }

            // Fire webhooks for unexplained-deploy event (best-effort, non-blocking)
            if !unexplained.is_empty() {
                let webhook_payload = serde_json::json!({
                    "range": range,
                    "total_commits": range_commits.len(),
                    "unexplained": unexplained.len(),
                    "explained": explained.len(),
                    "coverage_pct": coverage,
                    "unexplained_commits": unexplained.iter().map(|(sha, author, msg)| serde_json::json!({
                        "sha": sha, "author": author, "message": msg
                    })).collect::<Vec<_>>(),
                });
                fire_webhooks(&dir_path, &WebhookEvent::UnexplainedDeploy, &webhook_payload);
            }

            if fail_on_unexplained && !unexplained.is_empty() {
                std::process::exit(1);
            }
        }

        Commands::Weekly { from, to, repo, json } => {
            use axon_ledger::model::Effect;
            use axon_ledger::ingest::session::parse_iso_to_ms;

            // Parse window bounds (default: last 7 days, auto-expand to 30d if fewer than 3 sessions)
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
            let week_ms = 7 * 24 * 60 * 60 * 1000u64;
            let month_ms = 30 * 24 * 60 * 60 * 1000u64;

            let explicit_from = from.as_deref().and_then(parse_iso_to_ms);
            let to_ms = to.as_deref()
                .and_then(parse_iso_to_ms)
                .unwrap_or(now_ms);

            // Auto-expand: if no --from given and <3 sessions in last 7d, use last 30d
            let from_ms = if let Some(f) = explicit_from {
                f
            } else {
                let all_raw_check = store.all().unwrap_or_default();
                let sessions_7d = all_raw_check.iter()
                    .filter(|r| r.effect == Effect::AgentSession)
                    .filter(|r| r.ts_ms >= now_ms.saturating_sub(week_ms) && r.ts_ms <= to_ms)
                    .count();
                if sessions_7d < 3 {
                    now_ms.saturating_sub(month_ms)
                } else {
                    now_ms.saturating_sub(week_ms)
                }
            };

            let all_raw = store.all()?;
            let all = rbac.filter_owned(all_raw, caller.as_deref());
            let in_window: Vec<_> = all.iter()
                .filter(|r| r.ts_ms >= from_ms && r.ts_ms <= to_ms)
                .filter(|r| repo.as_deref().map(|rn| r.repo.as_deref() == Some(rn)).unwrap_or(true))
                .collect();

            let sessions: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::AgentSession).collect();
            let commits: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::GitCommit).collect();
            let edges: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::AgentEdge).collect();

            // Goals from sessions, sorted by ts_ms
            let mut goals: Vec<(u64, String)> = sessions.iter().map(|s| {
                let goal = s.payload.get("goal").and_then(|v| v.as_str())
                    .filter(|g| !g.starts_with('<')).unwrap_or("(no goal)").to_string();
                (s.ts_ms, goal)
            }).collect();
            goals.sort_by_key(|(ts, _)| *ts);

            // Files touched (for rework detection — files with 2+ sessions)
            let mut file_session_count: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for s in &sessions {
                if let Some(files) = s.payload.get("files_touched").and_then(|v| v.as_array()) {
                    for f in files {
                        if let Some(fname) = f.as_str() {
                            *file_session_count.entry(fname.split('/').last().unwrap_or(fname).to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            let mut rework_files: Vec<(String, usize)> = file_session_count.into_iter()
                .filter(|(_, c)| *c >= 2).collect();
            rework_files.sort_by(|a, b| b.1.cmp(&a.1));

            let explained_shas: std::collections::HashSet<String> = edges.iter()
                .filter_map(|e| e.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
                .collect();
            let explained_count = commits.iter().filter(|c| {
                let sha = c.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("");
                explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha))
            }).count();

            if json {
                let out = serde_json::json!({
                    "from_ms": from_ms, "to_ms": to_ms,
                    "sessions": sessions.len(),
                    "commits": commits.len(),
                    "explained_commits": explained_count,
                    "coverage_pct": if commits.is_empty() { 100u64 } else { (explained_count * 100 / commits.len()) as u64 },
                    "goals": goals.iter().map(|(ts, g)| serde_json::json!({"ts_ms": ts, "goal": g})).collect::<Vec<_>>(),
                    "rework_hotspots": rework_files.iter().take(5).map(|(f, c)| serde_json::json!({"file": f, "session_count": c})).collect::<Vec<_>>(),
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Weekly digest  ({} sessions, {} commits, {}% explained)\n",
                    sessions.len(), commits.len(),
                    if commits.is_empty() { 100usize } else { explained_count * 100 / commits.len() });
                println!("Goals shipped:");
                for (_, goal) in &goals {
                    let trunc = if goal.chars().count() > 72 {
                        let end = goal.char_indices().nth(72).map(|(i,_)|i).unwrap_or(goal.len());
                        format!("{}…", &goal[..end])
                    } else { goal.clone() };
                    println!("  • {trunc}");
                }
                if !rework_files.is_empty() {
                    println!("\nRework hotspots (multiple sessions, same file):");
                    for (file, count) in rework_files.iter().take(5) {
                        println!("  ⚠ {file}  ({count} sessions)");
                    }
                }
            }
        }

        Commands::Audit { module, since, repo, json } => {
            use axon_ledger::model::Effect;
            use axon_ledger::ingest::session::parse_iso_to_ms;

            let since_ms = since.as_deref().and_then(parse_iso_to_ms).unwrap_or(0);
            let module_lower = module.to_lowercase();

            let all_raw = store.all()?;
            let all = rbac.filter_owned(all_raw, caller.as_deref());
            let in_window: Vec<_> = all.iter()
                .filter(|r| r.ts_ms >= since_ms)
                .filter(|r| repo.as_deref().map(|rn| r.repo.as_deref() == Some(rn)).unwrap_or(true))
                .collect();

            // Sessions that touched files under the module path
            let matching_sessions: Vec<_> = in_window.iter()
                .filter(|r| r.effect == Effect::AgentSession)
                .filter(|r| {
                    r.payload.get("files_touched")
                        .and_then(|v| v.as_array())
                        .map(|files| files.iter().any(|f| {
                            f.as_str().map(|s| s.to_lowercase().contains(&module_lower)).unwrap_or(false)
                        }))
                        .unwrap_or(false)
                })
                .collect();

            // Commits that touched files under the module path
            // Note: git ingest stores files under "files" key (not "files_changed")
            let matching_commits: Vec<_> = in_window.iter()
                .filter(|r| r.effect == Effect::GitCommit)
                .filter(|r| {
                    let files = r.payload.get("files")
                        .or_else(|| r.payload.get("files_changed"));
                    files.and_then(|v| v.as_array())
                        .map(|files| files.iter().any(|f| {
                            f.as_str().map(|s| s.to_lowercase().contains(&module_lower)).unwrap_or(false)
                        }))
                        .unwrap_or(false)
                })
                .collect();

            // Edges linking matching sessions to matching commits
            let matching_edges: Vec<_> = in_window.iter()
                .filter(|r| r.effect == Effect::AgentEdge)
                .filter(|r| {
                    let sid = r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                    matching_sessions.iter().any(|s| {
                        s.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("") == sid
                    })
                })
                .collect();

            if json {
                let out = serde_json::json!({
                    "module": module,
                    "since_ms": since_ms,
                    "sessions": matching_sessions.iter().map(|s| serde_json::json!({
                        "session_id": s.payload.get("session_id").and_then(|v| v.as_str()),
                        "goal": s.payload.get("goal").and_then(|v| v.as_str()),
                        "engineer": s.principal.trim_start_matches("session:").trim_start_matches("agent:"),
                        "ts_ms": s.ts_ms,
                        "files_touched": s.payload.get("files_touched"),
                    })).collect::<Vec<_>>(),
                    "commits": matching_commits.iter().map(|c| serde_json::json!({
                        "sha": c.payload.get("sha").and_then(|v| v.as_str()),
                        "message": c.payload.get("message").and_then(|v| v.as_str()),
                        "author": c.payload.get("author").and_then(|v| v.as_str()),
                        "ts_ms": c.ts_ms,
                    })).collect::<Vec<_>>(),
                    "edges": matching_edges.len(),
                    "total_sessions": matching_sessions.len(),
                    "total_commits": matching_commits.len(),
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Audit: module={module}  sessions={} commits={}\n",
                    matching_sessions.len(), matching_commits.len());
                for s in &matching_sessions {
                    let goal = s.payload.get("goal").and_then(|v| v.as_str())
                        .filter(|g| !g.starts_with('<')).unwrap_or("(no goal)");
                    let trunc = if goal.chars().count() > 60 {
                        let end = goal.char_indices().nth(60).map(|(i,_)|i).unwrap_or(goal.len());
                        format!("{}…", &goal[..end])
                    } else { goal.to_string() };
                    let eng = s.principal.trim_start_matches("session:").trim_start_matches("agent:");
                    println!("  [session] {eng}  {trunc}");
                }
                for c in &matching_commits {
                    let sha = c.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("?");
                    let msg = c.payload.get("message").and_then(|v| v.as_str()).unwrap_or("?");
                    let short_sha = &sha[..sha.len().min(10)];
                    let msg_trunc = if msg.chars().count() > 55 {
                        let end = msg.char_indices().nth(55).map(|(i,_)|i).unwrap_or(msg.len());
                        format!("{}…", &msg[..end])
                    } else { msg.to_string() };
                    println!("  [commit]  {short_sha}  {msg_trunc}");
                }
            }
        }

        Commands::Prune { older_than, dry_run, yes, json } => {
            use axon_ledger::ingest::session::parse_iso_to_ms;

            // Parse the cutoff — try ISO 8601 first, then ask git to interpret it
            let cutoff_ms = if let Some(ms) = parse_iso_to_ms(&older_than) {
                ms
            } else {
                // Use `date -d "<arg>"` to resolve relative expressions like "90 days ago"
                let date_out = std::process::Command::new("date")
                    .args(["-d", &older_than, "+%s%3N"])
                    .output()
                    .context("Could not resolve date — use ISO 8601 (e.g. 2025-01-01) or a GNU date expression")?;
                if !date_out.status.success() {
                    anyhow::bail!("Cannot parse date '{}'. Use ISO 8601 or a GNU date expression like '90 days ago'.", older_than);
                }
                String::from_utf8_lossy(&date_out.stdout).trim().parse::<u64>()
                    .context("date output was not a valid millisecond timestamp")?
            };

            // Count what would be pruned without touching the store
            let all = store.all()?;
            let would_prune = all.iter().filter(|r| r.ts_ms < cutoff_ms).count();
            let would_keep = all.len() - would_prune;

            if json {
                let out = serde_json::json!({
                    "older_than": older_than,
                    "cutoff_ms": cutoff_ms,
                    "total_records": all.len(),
                    "would_prune": would_prune,
                    "would_keep": would_keep,
                    "dry_run": dry_run,
                });
                println!("{}", serde_json::to_string_pretty(&out)?);
                if dry_run { return Ok(()); }
            } else {
                println!("Prune: records older than '{older_than}' (cutoff_ms={cutoff_ms})");
                println!("  Total records : {}", all.len());
                println!("  Would delete  : {would_prune}");
                println!("  Would keep    : {would_keep}");
                if dry_run {
                    println!("\n(dry run — no changes made)");
                    return Ok(());
                }
            }

            if would_prune == 0 {
                println!("Nothing to prune.");
                return Ok(());
            }

            // Confirm unless --yes
            if !yes {
                eprint!("Delete {would_prune} record(s)? [y/N] ");
                let mut input = String::new();
                std::io::stdin().read_line(&mut input)?;
                if !input.trim().eq_ignore_ascii_case("y") {
                    eprintln!("Aborted.");
                    return Ok(());
                }
            }

            let mut store_mut = Store::open(&dir_path)?;
            let (kept, pruned) = store_mut.prune(cutoff_ms)?;
            if json {
                let out = serde_json::json!({ "kept": kept, "pruned": pruned, "ok": true });
                println!("{}", serde_json::to_string_pretty(&out)?);
            } else {
                println!("Done. Pruned {pruned} record(s); {kept} remain.");
            }
        }

        Commands::EngineerBackfill { email, dry_run } => {
            // Resolve email: explicit flag → git config user.email → error
            let resolved = email
                .or_else(|| {
                    std::process::Command::new("git")
                        .args(["config", "user.email"])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                })
                .ok_or_else(|| anyhow::anyhow!(
                    "Could not detect engineer email. Pass --email <email> explicitly or set git config user.email"
                ))?;

            let all = store.all()?;
            let would_update = all.iter().filter(|r| r.principal.starts_with("agent:")).count();

            if dry_run {
                println!("Dry run: would backfill {would_update}/{} records → {resolved}", all.len());
                println!("(no changes made)");
            } else {
                let mut store_mut = Store::open(&dir_path)?;
                let (total, updated) = store_mut.rewrite_principals("agent:", &resolved)?;
                println!("Engineer backfill complete: {updated}/{total} records → {resolved}");
            }
        }

        Commands::SessionRefresh { dir, dry_run } => {
            use axon_ledger::model::Effect;

            let existing = store.all()?;
            // Build map: session_id → (record_id, turn_count) for AgentSession records with turn_count=0
            let is_stale = |r: &&axon_ledger::model::LedgerRecord| -> bool {
                let turn_count = r.payload.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0);
                if turn_count == 0 { return true; }
                let goal = r.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("");
                // Garbled table fallback (fixed in clean_goal_text)
                if goal.contains('📋') || goal.contains("buildoncerun") || goal.contains("🟢 Strong") {
                    return true;
                }
                // "(no user goal found in session)" — now we look past the <caveat> for a real goal
                goal == "(no user goal found in session)"
            };
            // Include original principal so session-refresh preserves engineer identity
            let stale: Vec<(String, String, String)> = existing.iter()
                .filter(|r| r.effect == Effect::AgentSession)
                .filter(is_stale)
                .filter_map(|r| {
                    let sid = r.payload.get("session_id").and_then(|v| v.as_str())?;
                    Some((r.id.clone(), sid.to_string(), r.principal.clone()))
                })
                .collect();

            println!("Stale sessions: {}/{}", stale.len(), existing.iter().filter(|r| r.effect == Effect::AgentSession).count());
            if stale.is_empty() {
                println!("Nothing to refresh.");
                return Ok(());
            }

            // Map session_id → .jsonl file path
            let mut session_files: std::collections::HashMap<String, PathBuf> = std::collections::HashMap::new();
            if let Ok(rd) = std::fs::read_dir(&dir) {
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                            session_files.insert(stem.to_string(), path);
                        }
                    }
                }
            }

            let mut refreshed = 0usize;
            let mut not_found = 0usize;
            let gate = axon_ledger::ingest::session::GateOptions::default();

            for (old_id, session_id, original_principal) in &stale {
                let Some(session_path) = session_files.get(session_id) else {
                    not_found += 1;
                    continue;
                };
                // Use a throwaway store in a fresh temp dir to re-parse without dedup
                let tmp_dir = std::env::temp_dir().join(format!("axon_refresh_{}_{}", &session_id[..8], std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos()));
                let mut tmp_store = Store::open(&tmp_dir)?;
                match ingest_session(session_path, &mut tmp_store, &gate, None, None) {
                    Ok(Some(mut new_record)) => {
                        // Preserve the original record's id and principal (engineer identity must survive refresh)
                        new_record.id = old_id.clone();
                        new_record.principal = original_principal.clone();
                        if dry_run {
                            println!("  would refresh {} → turn_count={}", &session_id[..8], new_record.payload.get("turn_count").and_then(|v| v.as_u64()).unwrap_or(0));
                        } else {
                            let mut store_mut = Store::open(&dir_path)?;
                            store_mut.replace_record(old_id, &new_record)?;
                        }
                        refreshed += 1;
                    }
                    Ok(None) => { /* deduped in tmp store — shouldn't happen but safe */ }
                    Err(e) => eprintln!("  [skip] {}: {e}", &session_id[..8]),
                }
            }

            if dry_run {
                println!("\nDry run: would refresh {refreshed}, skip {not_found} (JSONL not found)");
            } else {
                println!("Done: refreshed {refreshed}, skipped {not_found} (JSONL not found)");
            }
        }

        Commands::Webhook { action } => match action {
            WebhookAction::Add { event, provider, url } => {
                let ev = WebhookEvent::from_str(&event)?;
                let prov = WebhookProvider::from_str(&provider)?;
                let id = add_webhook(&dir_path, ev, prov, &url)?;
                println!("Webhook registered: {id}  event={event}  provider={provider}");
                println!("  URL: {url}");
            }
            WebhookAction::List => {
                let hooks = load_webhooks(&dir_path)?;
                if hooks.is_empty() {
                    println!("No webhooks configured. Add one with: axon-ledger webhook add --event unexplained-deploy --provider slack --url <url>");
                } else {
                    println!("{} webhook(s):\n", hooks.len());
                    for h in &hooks {
                        println!("  {}  event={}  provider={:?}", h.id, h.event.as_str(), h.provider);
                        println!("       url: {}", h.url);
                    }
                }
            }
            WebhookAction::Rm { id } => {
                if remove_webhook(&dir_path, &id)? {
                    println!("Removed webhook {id}.");
                } else {
                    eprintln!("No webhook with id '{id}' found.");
                    std::process::exit(1);
                }
            }
        },

        Commands::Rbac { action } => match action {
            RbacAction::Grant { email } => {
                let mut config = RbacConfig::load(&dir_path)?;
                config.add_admin(&email);
                config.save(&dir_path)?;
                println!("Granted admin role to {email}.");
                println!("Admins can view all records; members see only their own.");
            }
            RbacAction::Revoke { email } => {
                let mut config = RbacConfig::load(&dir_path)?;
                if config.is_admin(&email) {
                    config.remove_admin(&email);
                    config.save(&dir_path)?;
                    println!("Revoked admin role from {email}.");
                } else {
                    eprintln!("{email} is not an admin.");
                    std::process::exit(1);
                }
            }
            RbacAction::List => {
                let config = RbacConfig::load(&dir_path)?;
                if config.admins.is_empty() {
                    println!("RBAC is disabled (no admins configured). All records are visible to everyone.");
                    println!("Enable with: axon-ledger rbac grant <email>");
                } else {
                    println!("Admins ({}):", config.admins.len());
                    for a in &config.admins {
                        println!("  {a}");
                    }
                    println!("\nMembers see only their own records. Use --as <email> to identify yourself.");
                }
            }
        },

        Commands::Refresh { repo, session_dir, engineer, since, json } => {
            let repo_path = repo.unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

            // Step 1: ingest git commits
            let mut store_mut = Store::open(&dir_path)?;
            let commits = ingest_git(&repo_path, &mut store_mut, since.as_deref(), None)
                .unwrap_or_else(|e| { eprintln!("[refresh] git ingest: {e}"); 0 });

            // Step 2: ingest Claude Code sessions
            // Auto-detect session dir from ~/.claude/projects/ by matching cwd
            let resolved_session_dir = session_dir.or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                let cwd_slug = cwd.to_string_lossy().replace('/', "-");
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                let candidate = PathBuf::from(home).join(".claude").join("projects").join(&cwd_slug);
                if candidate.exists() { Some(candidate) } else { None }
            });

            let resolved_engineer = engineer.or_else(|| {
                std::process::Command::new("git")
                    .args(["config", "user.email"])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            });

            let sessions = if let Some(sdir) = resolved_session_dir {
                let gate = GateOptions::default();
                let eng_ref = resolved_engineer.as_deref();
                let rd = std::fs::read_dir(&sdir)
                    .map_err(|e| anyhow::anyhow!("Cannot read session dir: {e}"))?;
                let mut ingested = 0usize;
                for entry in rd.flatten() {
                    let path = entry.path();
                    if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                        match ingest_session(&path, &mut store_mut, &gate, None, eng_ref) {
                            Ok(Some(_)) => ingested += 1,
                            Ok(None) => {} // already known
                            Err(e) => eprintln!("[refresh] session {}: {e}", path.display()),
                        }
                    }
                }
                ingested
            } else {
                0
            };

            // Step 3: infer causal edges
            let edges = infer_edges(&mut store_mut).unwrap_or_else(|e| { eprintln!("[refresh] edges: {e}"); 0 });

            if json {
                println!("{}", serde_json::json!({
                    "ok": true,
                    "commits_ingested": commits,
                    "sessions_ingested": sessions,
                    "edges_inferred": edges,
                }));
            } else {
                println!("Refresh complete:");
                println!("  Git commits ingested : {commits}");
                println!("  Sessions ingested    : {sessions}");
                println!("  Edges inferred       : {edges}");
                if commits == 0 && sessions == 0 {
                    println!("\n  Ledger is up to date.");
                }
            }
        }

        Commands::Mcp => {
            run_mcp_server(&dir_path)?;
        }

        Commands::Watch { dir, interval, gate, axon_bin, gate_script, engineer } => {
            let gate_opts = GateOptions { enabled: gate, axon_bin, gate_script };
            let resolved_engineer = engineer.or_else(|| {
                std::process::Command::new("git")
                    .args(["config", "user.email"])
                    .output()
                    .ok()
                    .and_then(|o| String::from_utf8(o.stdout).ok())
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
            });
            println!("[ledger-watch] polling {} every {}s  (Ctrl-C to stop)", dir.display(), interval);
            watch_sessions(&dir, &dir_path, Duration::from_secs(interval), &gate_opts, true, resolved_engineer.as_deref())?;
        }
    }

    Ok(())
}
