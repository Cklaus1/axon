/// MCP (Model Context Protocol) server over stdio.
///
/// Exposes ledger queries as tools that Claude Code and other MCP clients
/// can call mid-session. Run with: `axon-ledger mcp [--ledger-dir <path>]`
///
/// Tools exposed:
///   ledger_why    { sha: string }                → WhyResult
///   ledger_search { query: string, limit?: int } → SearchHit[]
///   ledger_as_of  { timestamp: string }          → AsOfResult
///   ledger_stats  {}                             → StatsResult
use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use crate::query::{as_of, search, why};
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
    fn test_tools_list_has_four_tools() {
        let id = Some(json!(1));
        let r = handle_tools_list(&id);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 4);
        let names: Vec<&str> = tools.iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert!(names.contains(&"ledger_why"));
        assert!(names.contains(&"ledger_search"));
        assert!(names.contains(&"ledger_as_of"));
        assert!(names.contains(&"ledger_stats"));
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
