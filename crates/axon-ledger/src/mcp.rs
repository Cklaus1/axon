/// MCP (Model Context Protocol) server over stdio.
///
/// Exposes ledger queries as tools that Claude Code and other MCP clients
/// can call mid-session. Run with: `axon-ledger mcp [--ledger-dir <path>]`
///
/// Tools exposed:
///   ledger_why        { sha: string }                → WhyResult
///   ledger_history    { file: string }               → HistoryResult
///   ledger_search     { query: string, limit?: int } → SearchHit[]
///   ledger_as_of      { timestamp: string }           → AsOfResult
///   ledger_stats      {}                              → StatsResult
///   ledger_weekly     { days?: int }                  → WeeklyDigest
///   ledger_audit      { module: string, since?: string } → AuditResult
///   ledger_pre_deploy { range?: string }              → PreDeployResult
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};

use anyhow::Result;
use serde_json::{json, Value};

use crate::ingest::edge::infer_edges;
use crate::ingest::git::ingest_git;
use crate::ingest::session::{ingest_session, GateOptions};
use crate::query::{as_of, history, search, why};
use crate::store::Store;

/// Blocking MCP server loop. Reads JSON-RPC 2.0 requests from stdin,
/// writes responses to stdout. Each message is newline-delimited JSON.
///
/// The store is re-opened per request so the server always sees the latest
/// records — safe because the ledger is append-only JSONL and Store::open
/// is a path wrapper (essentially free).
pub fn run_mcp_server(ledger_dir: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();

    // Send the MCP server-info / capabilities on startup
    let init = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });
    writeln!(stdout.lock(), "{}", init)?;

    for line in stdin.lock().lines() {
        let line = line?;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let req: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(e) => {
                let err = json_error(None, -32700, &format!("Parse error: {e}"));
                writeln!(stdout.lock(), "{}", err)?;
                continue;
            }
        };

        let id = req.get("id").cloned();
        let method = req.get("method").and_then(|v| v.as_str()).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => handle_initialize(&id),
            "tools/list" => handle_tools_list(&id),
            "tools/call" => handle_tools_call(&id, &params, ledger_dir),
            "ping" => json!({ "jsonrpc": "2.0", "id": id, "result": {} }),
            _ => json_error(id.as_ref(), -32601, &format!("Method not found: {method}")),
        };

        writeln!(stdout.lock(), "{}", response)?;
    }

    Ok(())
}

fn handle_initialize(id: &Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "protocolVersion": "2024-11-05",
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "axon-ledger",
                "version": "0.1.0"
            }
        }
    })
}

fn handle_tools_list(id: &Option<Value>) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "tools": [
                {
                    "name": "ledger_why",
                    "description": "Explain why a git commit happened — shows the AI agent session that produced it, the original goal the engineer typed, and any metric outcomes.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "sha": { "type": "string", "description": "Git commit SHA prefix (7+ chars)" }
                        },
                        "required": ["sha"]
                    }
                },
                {
                    "name": "ledger_history",
                    "description": "Show the AI session history of a file — which sessions worked on it, with what goal, and which commits each session produced. Answers 'why does this file look the way it does?'",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "file": { "type": "string", "description": "File path or suffix, e.g. 'auth/jwt.rs' or 'jwt.rs'" }
                        },
                        "required": ["file"]
                    }
                },
                {
                    "name": "ledger_search",
                    "description": "Search commits and sessions by keyword. Returns commits whose messages or files match, and sessions whose goals or files match, ranked by recency.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "query": { "type": "string", "description": "Search terms (all must match — AND semantics)" },
                            "limit": { "type": "integer", "description": "Max results (default 10)" }
                        },
                        "required": ["query"]
                    }
                },
                {
                    "name": "ledger_as_of",
                    "description": "Reconstruct what was known and shipped at a point in time — recent commits, active agent sessions, and files being worked on.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "timestamp": { "type": "string", "description": "ISO 8601 timestamp, e.g. 2026-06-19T00:00:00Z" }
                        },
                        "required": ["timestamp"]
                    }
                },
                {
                    "name": "ledger_stats",
                    "description": "Show ledger statistics: total commits, sessions, edges, and outcomes ingested.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "ledger_weekly",
                    "description": "Weekly digest of AI coding activity: sessions, commits, goals shipped, coverage %, and rework hotspots. Auto-expands from 7 to 30 days when session count is low. Use this to understand what work happened in a recent period.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Window in days (default: 7, auto-expands to 30 if fewer than 3 sessions)" }
                        }
                    }
                },
                {
                    "name": "ledger_audit",
                    "description": "Compliance query: list all AI sessions and commits that touched files under a module path. Use for 'who worked on payments/ in the last 90 days?' or 'which sessions touched auth/?'",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "module": { "type": "string", "description": "Module path prefix to match, e.g. 'payments/', 'src/auth', 'lib/db'" },
                            "since": { "type": "string", "description": "Only include records after this date (ISO 8601 or '90 days ago')" }
                        },
                        "required": ["module"]
                    }
                },
                {
                    "name": "ledger_pre_deploy",
                    "description": "Flag unexplained commits before a deploy — commits with no linked AI session. Returns coverage % and lists which commits lack session provenance. Use before merging or deploying to check that all changes are explainable.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "range": { "type": "string", "description": "Git commit range, e.g. 'HEAD~8..HEAD' or 'sha1..sha2' (default: HEAD~10..HEAD)" },
                            "repo": { "type": "string", "description": "Path to the git repository (default: current directory)" }
                        }
                    }
                },
                {
                    "name": "ledger_refresh",
                    "description": "Refresh the ledger: ingest new git commits, ingest new Claude Code sessions, then infer causal edges. Call this at the start of a session or after making commits to ensure the ledger has current data. Returns counts of newly ingested records.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "repo": { "type": "string", "description": "Path to the git repository (default: current directory)" },
                            "session_dir": { "type": "string", "description": "Directory containing Claude Code JSONL session files (default: auto-detected from ~/.claude/projects/)" },
                            "engineer": { "type": "string", "description": "Engineer email for session attribution (default: git config user.email)" }
                        }
                    }
                }
            ]
        }
    })
}

fn handle_tools_call(id: &Option<Value>, params: &Value, ledger_dir: &Path) -> Value {
    let tool_name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(json!({}));

    let store = match Store::open(ledger_dir).map_err(anyhow::Error::from) {
        Ok(s) => s,
        Err(e) => return json_error(id.as_ref(), -32000, &format!("Could not open ledger: {e}")),
    };

    let result = match tool_name {
        "ledger_why" => {
            let sha = args.get("sha").and_then(|v| v.as_str()).unwrap_or("");
            if sha.is_empty() {
                return json_error(id.as_ref(), -32602, "ledger_why requires 'sha'");
            }
            why(sha, &store)
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "ledger_history" => {
            let file = args.get("file").and_then(|v| v.as_str()).unwrap_or("");
            if file.is_empty() {
                return json_error(id.as_ref(), -32602, "ledger_history requires 'file'");
            }
            history(file, &store)
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "ledger_search" => {
            let query = args.get("query").and_then(|v| v.as_str()).unwrap_or("");
            if query.is_empty() {
                return json_error(id.as_ref(), -32602, "ledger_search requires 'query'");
            }
            let limit = args.get("limit").and_then(|v| v.as_u64()).unwrap_or(10) as usize;
            search(query, &store, limit)
                .map(|hits| serde_json::to_value(hits).unwrap_or(json!([])))
        }
        "ledger_as_of" => {
            let ts_str = args.get("timestamp").and_then(|v| v.as_str()).unwrap_or("");
            let ts_ms = crate::ingest::session::parse_iso_to_ms(ts_str)
                .ok_or_else(|| anyhow::anyhow!("Invalid timestamp: {ts_str}"));
            ts_ms.and_then(|ms| as_of(ms, &store))
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "ledger_stats" => {
            store.all()
                .map_err(anyhow::Error::from)
                .map(|all| {
                    use crate::model::Effect;
                    json!({
                        "total": all.len(),
                        "git_commits": all.iter().filter(|r| r.effect == Effect::GitCommit).count(),
                        "agent_sessions": all.iter().filter(|r| r.effect == Effect::AgentSession).count(),
                        "agent_edges": all.iter().filter(|r| r.effect == Effect::AgentEdge).count(),
                        "metric_outcomes": all.iter().filter(|r| r.effect == Effect::MetricOutcome).count(),
                    })
                })
        }
        "ledger_weekly" => {
            use crate::model::Effect;
            let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7);
            let now_ms = now_ms();
            let to_ms = now_ms;
            let cutoff = now_ms.saturating_sub(days * 86_400_000);
            // Auto-expand if sparse
            let all_raw = match store.all().map_err(anyhow::Error::from) {
                Ok(v) => v,
                Err(e) => return json_error(id.as_ref(), -32000, &e.to_string()),
            };
            let sessions_in_window = all_raw.iter()
                .filter(|r| r.effect == Effect::AgentSession && r.ts_ms >= cutoff)
                .count();
            let from_ms = if sessions_in_window < 3 && days <= 7 {
                now_ms.saturating_sub(30 * 86_400_000)
            } else { cutoff };

            let in_window: Vec<_> = all_raw.iter()
                .filter(|r| r.ts_ms >= from_ms && r.ts_ms <= to_ms)
                .collect();
            let sessions: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::AgentSession).collect();
            let commits: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::GitCommit).collect();
            let edges: Vec<_> = in_window.iter().filter(|r| r.effect == Effect::AgentEdge).collect();

            let goals: Vec<serde_json::Value> = {
                let mut g: Vec<(u64, String)> = sessions.iter().map(|s| {
                    let goal = s.payload.get("goal").and_then(|v| v.as_str())
                        .filter(|g| !g.starts_with('<')).unwrap_or("(no goal)").to_string();
                    (s.ts_ms, goal)
                }).collect();
                g.sort_by_key(|(ts, _)| *ts);
                g.iter().map(|(ts, goal)| json!({"ts_ms": ts, "goal": goal})).collect()
            };

            let explained_shas: std::collections::HashSet<String> = edges.iter()
                .filter_map(|e| e.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
                .collect();
            let explained_count = commits.iter().filter(|c| {
                let sha = c.payload.get("sha").and_then(|v| v.as_str()).unwrap_or("");
                explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha))
            }).count();

            let mut file_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
            for s in &sessions {
                if let Some(files) = s.payload.get("files_touched").and_then(|v| v.as_array()) {
                    for f in files {
                        if let Some(fname) = f.as_str() {
                            *file_counts.entry(fname.split('/').last().unwrap_or(fname).to_string()).or_insert(0) += 1;
                        }
                    }
                }
            }
            let mut rework: Vec<_> = file_counts.into_iter().filter(|(_, c)| *c >= 2).collect();
            rework.sort_by(|a, b| b.1.cmp(&a.1));

            Ok(json!({
                "from_ms": from_ms, "to_ms": to_ms, "window_days": (to_ms - from_ms) / 86_400_000,
                "sessions": sessions.len(), "commits": commits.len(), "explained_commits": explained_count,
                "coverage_pct": if commits.is_empty() { 100u64 } else { (explained_count * 100 / commits.len()) as u64 },
                "goals": goals,
                "rework_hotspots": rework.iter().take(5).map(|(f, c)| json!({"file": f, "session_count": c})).collect::<Vec<_>>(),
            }))
        }
        "ledger_audit" => {
            use crate::model::Effect;
            use crate::ingest::session::parse_iso_to_ms;
            let module = args.get("module").and_then(|v| v.as_str()).unwrap_or("");
            if module.is_empty() {
                return json_error(id.as_ref(), -32602, "ledger_audit requires 'module'");
            }
            let since_ms = args.get("since").and_then(|v| v.as_str()).and_then(parse_iso_to_ms).unwrap_or(0);
            let module_lower = module.to_lowercase();

            store.all().map_err(anyhow::Error::from).map(|all| {
                let in_window: Vec<_> = all.iter().filter(|r| r.ts_ms >= since_ms).collect();
                let matching_sessions: Vec<_> = in_window.iter()
                    .filter(|r| r.effect == Effect::AgentSession)
                    .filter(|r| r.payload.get("files_touched").and_then(|v| v.as_array())
                        .map(|files| files.iter().any(|f| f.as_str().map(|s| s.to_lowercase().contains(&module_lower)).unwrap_or(false)))
                        .unwrap_or(false))
                    .collect();
                let matching_commits: Vec<_> = in_window.iter()
                    .filter(|r| r.effect == Effect::GitCommit)
                    .filter(|r| r.payload.get("files_changed").and_then(|v| v.as_array())
                        .map(|files| files.iter().any(|f| f.as_str().map(|s| s.to_lowercase().contains(&module_lower)).unwrap_or(false)))
                        .unwrap_or(false))
                    .collect();
                json!({
                    "module": module, "since_ms": since_ms,
                    "total_sessions": matching_sessions.len(),
                    "total_commits": matching_commits.len(),
                    "sessions": matching_sessions.iter().map(|s| json!({
                        "session_id": s.payload.get("session_id").and_then(|v| v.as_str()),
                        "goal": s.payload.get("goal").and_then(|v| v.as_str()),
                        "engineer": s.principal.trim_start_matches("session:").trim_start_matches("agent:"),
                        "ts_ms": s.ts_ms,
                    })).collect::<Vec<_>>(),
                    "commits": matching_commits.iter().map(|c| json!({
                        "sha": c.payload.get("sha").and_then(|v| v.as_str()),
                        "message": c.payload.get("message").and_then(|v| v.as_str()),
                        "author": c.payload.get("author").and_then(|v| v.as_str()),
                        "ts_ms": c.ts_ms,
                    })).collect::<Vec<_>>(),
                })
            })
        }
        "ledger_pre_deploy" => {
            use crate::model::Effect;
            let range = args.get("range").and_then(|v| v.as_str()).unwrap_or("HEAD~10..HEAD");
            let repo = args.get("repo").and_then(|v| v.as_str()).unwrap_or(".");

            let git_out = match std::process::Command::new("git")
                .arg("-C").arg(repo)
                .args(["log", "--format=%H|%ae|%s", range])
                .output()
            {
                Ok(o) => o,
                Err(e) => return json_error(id.as_ref(), -32000, &format!("git log failed: {e}")),
            };
            if !git_out.status.success() {
                return json_error(id.as_ref(), -32000,
                    &format!("git log failed: {}", String::from_utf8_lossy(&git_out.stderr)));
            }
            let range_commits: Vec<(String, String, String)> = String::from_utf8_lossy(&git_out.stdout)
                .lines()
                .filter_map(|l| {
                    let parts: Vec<&str> = l.splitn(3, '|').collect();
                    if parts.len() == 3 { Some((parts[0].to_string(), parts[1].to_string(), parts[2].to_string())) }
                    else { None }
                })
                .collect();

            store.all().map_err(anyhow::Error::from).map(|all| {
                let explained_shas: std::collections::HashSet<String> = all.iter()
                    .filter(|r| r.effect == Effect::AgentEdge)
                    .filter_map(|r| r.payload.get("commit_sha").and_then(|v| v.as_str()).map(String::from))
                    .collect();
                let mut sha_to_goal: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for edge in all.iter().filter(|r| r.effect == Effect::AgentEdge) {
                    let sha = edge.payload.get("commit_sha").and_then(|v| v.as_str()).unwrap_or("").to_string();
                    let sid = edge.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("");
                    if let Some(session) = all.iter().find(|r| r.effect == Effect::AgentSession &&
                        r.payload.get("session_id").and_then(|v| v.as_str()).unwrap_or("") == sid)
                    {
                        let goal = session.payload.get("goal").and_then(|v| v.as_str())
                            .filter(|g| !g.starts_with('<')).unwrap_or("(no goal)").to_string();
                        sha_to_goal.entry(sha).or_insert(goal);
                    }
                }
                let is_explained = |sha: &str| explained_shas.iter().any(|s| sha.starts_with(s.as_str()) || s.starts_with(sha));
                let explained: Vec<_> = range_commits.iter().filter(|(sha, _, _)| is_explained(sha)).collect();
                let unexplained: Vec<_> = range_commits.iter().filter(|(sha, _, _)| !is_explained(sha)).collect();
                let coverage = if range_commits.is_empty() { 100u64 }
                    else { (explained.len() * 100 / range_commits.len()) as u64 };
                json!({
                    "range": range, "total_commits": range_commits.len(),
                    "explained": explained.len(), "unexplained": unexplained.len(),
                    "coverage_pct": coverage,
                    "explained_commits": explained.iter().map(|(sha, author, msg)| {
                        let goal = sha_to_goal.get(sha.as_str()).cloned().unwrap_or_default();
                        json!({"sha": sha, "author": author, "message": msg, "session_goal": goal})
                    }).collect::<Vec<_>>(),
                    "unexplained_commits": unexplained.iter().map(|(sha, author, msg)| {
                        json!({"sha": sha, "author": author, "message": msg})
                    }).collect::<Vec<_>>(),
                })
            })
        }
        "ledger_refresh" => {
            let repo_path = args.get("repo").and_then(|v| v.as_str())
                .map(PathBuf::from)
                .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());
            let session_dir_arg = args.get("session_dir").and_then(|v| v.as_str()).map(PathBuf::from);
            let engineer = args.get("engineer").and_then(|v| v.as_str())
                .map(String::from)
                .or_else(|| {
                    std::process::Command::new("git")
                        .args(["config", "user.email"])
                        .output()
                        .ok()
                        .and_then(|o| String::from_utf8(o.stdout).ok())
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                });

            let mut store_mut = match Store::open(ledger_dir) {
                Ok(s) => s,
                Err(e) => return json_error(id.as_ref(), -32000, &format!("Could not open ledger: {e}")),
            };
            let commits = ingest_git(&repo_path, &mut store_mut, None, None).unwrap_or(0);

            let resolved_session_dir = session_dir_arg.or_else(|| {
                let cwd = std::env::current_dir().ok()?;
                let cwd_slug = cwd.to_string_lossy().replace('/', "-");
                let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string());
                let candidate = PathBuf::from(home).join(".claude").join("projects").join(&cwd_slug);
                if candidate.exists() { Some(candidate) } else { None }
            });

            let sessions = if let Some(sdir) = resolved_session_dir {
                let gate = GateOptions::default();
                let eng_ref = engineer.as_deref();
                let mut ingested = 0usize;
                if let Ok(rd) = std::fs::read_dir(&sdir) {
                    for entry in rd.flatten() {
                        let path = entry.path();
                        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                            if ingest_session(&path, &mut store_mut, &gate, None, eng_ref).is_ok() {
                                ingested += 1;
                            }
                        }
                    }
                }
                ingested
            } else { 0 };

            let edges = infer_edges(&mut store_mut).unwrap_or(0);

            Ok(json!({
                "ok": true,
                "commits_ingested": commits,
                "sessions_ingested": sessions,
                "edges_inferred": edges,
                "message": format!("Refreshed: {commits} commits, {sessions} sessions, {edges} edges")
            }))
        }
        _ => return json_error(id.as_ref(), -32602, &format!("Unknown tool: {tool_name}")),
    };

    match result {
        Ok(value) => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": {
                "content": [{ "type": "text", "text": value.to_string() }]
            }
        }),
        Err(e) => json_error(id.as_ref(), -32000, &e.to_string()),
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

fn json_error(id: Option<&Value>, code: i32, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn temp_ledger() -> TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    #[test]
    fn test_initialize_response() {
        let id = Some(json!(1));
        let r = handle_initialize(&id);
        assert_eq!(r["result"]["serverInfo"]["name"], "axon-ledger");
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_tools_list_has_nine_tools() {
        let id = Some(json!(1));
        let r = handle_tools_list(&id);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 9, "expected 9 MCP tools, got {}", tools.len());
        let names: Vec<&str> = tools.iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        for expected in &["ledger_why", "ledger_history", "ledger_search",
                          "ledger_as_of", "ledger_stats",
                          "ledger_weekly", "ledger_audit", "ledger_pre_deploy",
                          "ledger_refresh"] {
            assert!(names.contains(expected), "missing tool: {expected}");
        }
    }

    #[test]
    fn test_refresh_on_empty_dir() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_refresh", "arguments": {
            "repo": "/tmp",
            "session_dir": "/tmp/nonexistent_sessions_xyz"
        }});
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some(), "refresh should not error: {:?}", r);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let data: Value = serde_json::from_str(text).unwrap();
        assert_eq!(data["ok"], true);
        assert_eq!(data["commits_ingested"], 0); // /tmp has no git repo
        assert_eq!(data["sessions_ingested"], 0); // nonexistent dir
    }

    #[test]
    fn test_weekly_on_empty_ledger() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_weekly", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some());
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let data: Value = serde_json::from_str(text).unwrap();
        assert_eq!(data["sessions"], 0);
        assert_eq!(data["coverage_pct"], 100);
    }

    #[test]
    fn test_audit_requires_module() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_audit", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("error").is_some());
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn test_audit_on_empty_ledger() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_audit", "arguments": { "module": "src/auth" } });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some());
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let data: Value = serde_json::from_str(text).unwrap();
        assert_eq!(data["total_sessions"], 0);
        assert_eq!(data["module"], "src/auth");
    }

    #[test]
    fn test_unknown_tool_returns_error() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "does_not_exist", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("error").is_some(), "expected error for unknown tool");
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn test_missing_sha_returns_error() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_why", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("error").is_some());
    }

    #[test]
    fn test_stats_on_empty_ledger() {
        let id = Some(json!(1));
        let dir = temp_ledger();
        let params = json!({ "name": "ledger_stats", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some(), "expected result for stats on empty ledger");
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let data: Value = serde_json::from_str(text).unwrap();
        assert_eq!(data["total"], 0);
    }

    #[test]
    fn test_json_error_format() {
        let id = Some(json!(42));
        let r = json_error(id.as_ref(), -32700, "Parse error");
        assert_eq!(r["jsonrpc"], "2.0");
        assert_eq!(r["id"], 42);
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["error"]["message"], "Parse error");
    }
}
