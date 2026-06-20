/// MCP (Model Context Protocol) server for axon-signal analytics.
///
/// Exposes session effectiveness analytics as tools Claude Code can call
/// mid-session. Run with: `axon-signal mcp [--ledger-dir <path>]`
///
/// Tools:
///   signal_score      { engineer?: str, days?: int }  → SessionScore[]
///   signal_weekly     { days?: int }                  → WeeklyReport
///   signal_rework     { days?: int }                  → ReworkHotspot[]
///   signal_patterns   { min_score?: int }             → PatternLibrary
///   signal_trends     { weeks?: int, engineer?: str } → TrendsReport
///   signal_goals      { days?: int }                  → EngineerGoalSummary[]
///   signal_loops      { turns_threshold?: int, days?: int } → LoopCandidate[]
///   signal_benchmark        { days?: int }                          → BenchmarkReport
///   signal_ab_status        {}                                      → AbReport
///   signal_ab_track         { text: str, week?: str, baseline?: f64, engineer?: str } → rec
///   signal_export_training  { min_score?: int, format?: str }      → TrainingExport
use std::io::{self, BufRead, Write};
use std::path::Path;

use anyhow::Result;
use serde_json::{json, Value};

use axon_ledger::store::Store;

use crate::ab::{compute_ab_report, current_iso_week, record_recommendation};
use crate::benchmark::compute_benchmark;
use crate::export::{export_training, ExportFormat, ExportOptions};
use crate::patterns::{antipatterns, build_pattern_library};
use crate::rework::find_rework_hotspots;
use crate::score::score_sessions;
use crate::trends::compute_trends;
use crate::weekly::generate_weekly;

pub fn run_mcp_server(ledger_dir: &Path) -> Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();

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
            "serverInfo": { "name": "axon-signal", "version": "0.1.0" }
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
                    "name": "signal_score",
                    "description": "Score AI coding sessions for effectiveness (0–100). Shows goal clarity, turns-per-commit, scope fit, and training tier (positive_gold / positive_silver / filtered / negative). Use this to understand whether sessions are high-quality or need improvement.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "engineer": { "type": "string", "description": "Filter by engineer email prefix (optional)" },
                            "days": { "type": "integer", "description": "Only score sessions from the last N days (default: all time)" }
                        }
                    }
                },
                {
                    "name": "signal_weekly",
                    "description": "Weekly team effectiveness report: average score, top sessions, rework hotspots, and goal quality trend. Call with no args for the last 7 days.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Window in days (default: 7)" }
                        }
                    }
                },
                {
                    "name": "signal_rework",
                    "description": "Find files touched by multiple AI sessions in a short window — a rework hotspot signals vague goals, partial fixes, or scope drift. Returns file path, session count, goals, and time window.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Rework window in days (default: 14)" },
                            "min_sessions": { "type": "integer", "description": "Minimum distinct sessions to qualify as hotspot (default: 3 — use 2 to catch all)" }
                        }
                    }
                },
                {
                    "name": "signal_patterns",
                    "description": "Show which session goal patterns produce the best outcomes (high score, many commits) and which are antipatterns (low score, few commits). Helps teams write better session goals.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "min_score": { "type": "integer", "description": "Minimum score threshold for top patterns (default: 65)" },
                            "min_sessions": { "type": "integer", "description": "Minimum session count to include a pattern (default: 2)" }
                        }
                    }
                },
                {
                    "name": "signal_goals",
                    "description": "Per-engineer goal clarity breakdown. Shows average, best, and worst goal clarity score per engineer, plus a team recommendation when averages are low.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Window in days (default: 30)" }
                        }
                    }
                },
                {
                    "name": "signal_loops",
                    "description": "Detect sessions that would benefit from a /loop or /workflow automation: high turn counts with few commits suggest the engineer was doing manually what a loop could automate.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "turns_threshold": { "type": "integer", "description": "Min turns to flag as a loop candidate (default: 40)" },
                            "days": { "type": "integer", "description": "Window in days (default: 30)" }
                        }
                    }
                },
                {
                    "name": "signal_trends",
                    "description": "Per-engineer week-over-week score trends: who is improving, who is declining, and why. Includes per-week averages, delta from first to last week, and a tailored recommendation for each engineer.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "weeks": { "type": "integer", "description": "Number of weeks to include (default: 8)" },
                            "engineer": { "type": "string", "description": "Filter to a specific engineer email prefix (optional)" }
                        }
                    }
                },
                {
                    "name": "signal_benchmark",
                    "description": "Compare the team's AI coding effectiveness to industry percentile tiers (Developing/Bronze/Silver/Gold/Platinum). Returns team average score, tier, dimension-level comparison vs industry medians (goal clarity, turns/commit, rework rate, commit rate), per-engineer tier breakdown, and ranked recommendations for improvement.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "days": { "type": "integer", "description": "Only include sessions from the last N days (default: all time)" }
                        }
                    }
                },
                {
                    "name": "signal_export_training",
                    "description": "Export high-signal sessions as training data for Trainloop/LoRA fine-tuning. Returns JSONL records with messages + metadata (signal_score, goal, commits_produced, training_tier). Use to build a training corpus of effective AI coding sessions.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "min_score": { "type": "integer", "description": "Minimum signal score to include (default: 75)" },
                            "format": { "type": "string", "description": "Output format: 'trainloop' (default) or 'dpo' (chosen/rejected pairs for DPO training)" },
                            "anonymize": { "type": "boolean", "description": "Anonymize engineer names (default: false)" }
                        }
                    }
                },
                {
                    "name": "signal_ab_status",
                    "description": "Show A/B tracking outcomes for weekly recommendations: which recommendation types correlated with score improvement the following week. Observational (not a randomised trial) — use for directional guidance. Record recommendations with signal_ab_track.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {}
                    }
                },
                {
                    "name": "signal_ab_track",
                    "description": "Record a recommendation for A/B outcome tracking. Call this after advising an engineer to change their goal-writing style, session scope, or loop usage — then check signal_ab_status next week to see if scores improved. Returns the recorded recommendation id.",
                    "inputSchema": {
                        "type": "object",
                        "required": ["text"],
                        "properties": {
                            "text": { "type": "string", "description": "The recommendation text to track" },
                            "engineer": { "type": "string", "description": "Engineer email to attribute this to (optional)" },
                            "week": { "type": "string", "description": "ISO week string like '2025-W23' (default: current week)" },
                            "baseline": { "type": "number", "description": "Baseline effectiveness score before the recommendation (default: computed from last 7 days)" }
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
        "signal_score" => {
            let days = args.get("days").and_then(|v| v.as_u64());
            let engineer = args.get("engineer").and_then(|v| v.as_str()).map(String::from);
            score_sessions(&store).map(|mut scores| {
                if let Some(d) = days {
                    let cutoff = now_ms().saturating_sub(d * 86_400_000);
                    scores.retain(|s| s.ts_ms >= cutoff);
                }
                if let Some(ref eng) = engineer {
                    scores.retain(|s| s.engineer.contains(eng.as_str()));
                }
                serde_json::to_value(scores).unwrap_or(json!([]))
            })
        }
        "signal_weekly" => {
            let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(7);
            let to_ms = now_ms();
            let from_ms = to_ms.saturating_sub(days * 86_400_000);
            generate_weekly(&store, from_ms, to_ms, 14)
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "signal_rework" => {
            let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(14);
            let min_sessions = args.get("min_sessions").and_then(|v| v.as_u64()).unwrap_or(3) as usize;
            find_rework_hotspots(&store, days)
                .map(|all| {
                    let filtered: Vec<_> = all.into_iter().filter(|h| h.session_count >= min_sessions).collect();
                    serde_json::to_value(filtered).unwrap_or(json!([]))
                })
        }
        "signal_patterns" => {
            let min_score = args.get("min_score").and_then(|v| v.as_u64()).unwrap_or(65) as u8;
            let min_sessions = args.get("min_sessions").and_then(|v| v.as_u64()).unwrap_or(2) as usize;
            score_sessions(&store).map(|scores| {
                let top = build_pattern_library(&scores, min_sessions, min_score);
                let anti = antipatterns(&scores, min_sessions);
                json!({ "top_patterns": top, "antipatterns": anti })
            })
        }
        "signal_goals" => {
            let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(30);
            let cutoff = now_ms().saturating_sub(days * 86_400_000);
            score_sessions(&store).map(|scores| {
                let mut by_eng: std::collections::HashMap<String, Vec<u8>> = std::collections::HashMap::new();
                for s in scores.iter().filter(|s| s.ts_ms >= cutoff) {
                    by_eng.entry(s.engineer.clone()).or_default().push(s.goal_clarity);
                }
                let mut summary: Vec<_> = by_eng.into_iter().map(|(eng, v)| {
                    let avg = v.iter().map(|&c| c as f64).sum::<f64>() / v.len() as f64;
                    json!({
                        "engineer": eng,
                        "avg_goal_clarity": (avg * 10.0).round() / 10.0,
                        "sessions": v.len(),
                        "best": v.iter().copied().max().unwrap_or(0),
                        "worst": v.iter().copied().min().unwrap_or(0),
                    })
                }).collect();
                summary.sort_by(|a, b| {
                    b["avg_goal_clarity"].as_f64().unwrap_or(0.0)
                        .partial_cmp(&a["avg_goal_clarity"].as_f64().unwrap_or(0.0))
                        .unwrap_or(std::cmp::Ordering::Equal)
                });
                json!(summary)
            })
        }
        "signal_loops" => {
            let threshold = args.get("turns_threshold").and_then(|v| v.as_u64()).unwrap_or(40);
            let days = args.get("days").and_then(|v| v.as_u64()).unwrap_or(30);
            let cutoff = now_ms().saturating_sub(days * 86_400_000);
            score_sessions(&store).map(|scores| {
                let candidates: Vec<_> = scores.iter()
                    .filter(|s| s.ts_ms >= cutoff && s.turns >= threshold)
                    .map(|s| json!({
                        "session_id": s.session_id,
                        "engineer": s.engineer,
                        "goal": s.goal,
                        "turns": s.turns,
                        "commits_linked": s.commits_linked,
                        "turns_per_commit": s.turns_per_commit,
                        "score": s.score,
                        "recommendation": if s.commits_linked == 0 {
                            "High turns, no commits — try /loop to iterate automatically"
                        } else {
                            "High turns/commit ratio — /loop could have automated this iteration"
                        }
                    }))
                    .collect();
                json!(candidates)
            })
        }
        "signal_trends" => {
            let weeks = args.get("weeks").and_then(|v| v.as_u64()).unwrap_or(8) as usize;
            let engineer = args.get("engineer").and_then(|v| v.as_str()).map(String::from);
            compute_trends(&store, weeks).map(|mut r| {
                if let Some(ref eng) = engineer {
                    r.engineers.retain(|e| e.engineer.contains(eng.as_str()));
                }
                serde_json::to_value(r).unwrap_or(json!({}))
            })
        }
        "signal_benchmark" => {
            let days = args.get("days").and_then(|v| v.as_u64());
            compute_benchmark(&store, days)
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "signal_ab_status" => {
            compute_ab_report(ledger_dir, &store)
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "signal_ab_track" => {
            let text = match args.get("text").and_then(|v| v.as_str()) {
                Some(t) => t.to_string(),
                None => return json_error(id.as_ref(), -32602, "signal_ab_track requires 'text'"),
            };
            let engineer = args.get("engineer").and_then(|v| v.as_str()).map(String::from);
            let week = args.get("week").and_then(|v| v.as_str())
                .map(String::from)
                .unwrap_or_else(current_iso_week);
            let baseline = args.get("baseline").and_then(|v| v.as_f64()).unwrap_or_else(|| {
                score_sessions(&store)
                    .ok()
                    .and_then(|scores| {
                        let cutoff = now_ms().saturating_sub(7 * 86_400_000);
                        let recent: Vec<_> = scores.iter().filter(|s| s.ts_ms >= cutoff).collect();
                        if recent.is_empty() { None }
                        else { Some(recent.iter().map(|s| s.score as f64).sum::<f64>() / recent.len() as f64) }
                    })
                    .unwrap_or(0.0)
            });
            record_recommendation(ledger_dir, &week, &text, baseline, engineer.as_deref())
                .map(|r| serde_json::to_value(r).unwrap_or(json!({})))
        }
        "signal_export_training" => {
            let min_score = args.get("min_score").and_then(|v| v.as_u64()).unwrap_or(75) as u8;
            let format_str = args.get("format").and_then(|v| v.as_str()).unwrap_or("trainloop");
            let anonymize = args.get("anonymize").and_then(|v| v.as_bool()).unwrap_or(false);
            let format = if format_str == "dpo" { ExportFormat::Dpo } else { ExportFormat::Trainloop };
            let opts = ExportOptions { format, min_score, anonymize, ..ExportOptions::default() };
            let mut buf = Vec::new();
            export_training(&store, &opts, &mut buf)
                .map(|count| {
                    let records: Vec<serde_json::Value> = String::from_utf8_lossy(&buf)
                        .lines()
                        .filter_map(|l| serde_json::from_str(l).ok())
                        .collect();
                    json!({ "count": count, "format": format_str, "records": records })
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

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_initialize_response() {
        let id = Some(json!(1));
        let r = handle_initialize(&id);
        assert_eq!(r["result"]["serverInfo"]["name"], "axon-signal");
        assert_eq!(r["result"]["protocolVersion"], "2024-11-05");
    }

    #[test]
    fn test_tools_list_has_eleven_tools() {
        let id = Some(json!(1));
        let r = handle_tools_list(&id);
        let tools = r["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 11, "expected 11 MCP tools, got {}", tools.len());
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in &["signal_score", "signal_weekly", "signal_rework",
                          "signal_patterns", "signal_goals", "signal_loops",
                          "signal_trends", "signal_benchmark", "signal_ab_status",
                          "signal_ab_track", "signal_export_training"] {
            assert!(names.contains(expected), "missing tool: {expected}");
        }
    }

    #[test]
    fn test_export_training_on_empty_store() {
        let id = Some(json!(1));
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let _ = store; // ensure store exists
        let params = json!({ "name": "signal_export_training", "arguments": { "min_score": 50 } });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some(), "should not error: {:?}", r);
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let data: serde_json::Value = serde_json::from_str(text).unwrap();
        assert_eq!(data["count"], 0);
        assert_eq!(data["format"], "trainloop");
    }

    #[test]
    fn test_unknown_tool_returns_error() {
        let id = Some(json!(1));
        let dir = tempdir().unwrap();
        let params = json!({ "name": "does_not_exist", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("error").is_some());
        assert_eq!(r["error"]["code"], -32602);
    }

    #[test]
    fn test_score_on_empty_ledger() {
        let id = Some(json!(1));
        let dir = tempdir().unwrap();
        let params = json!({ "name": "signal_score", "arguments": {} });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some());
        let text = r["result"]["content"][0]["text"].as_str().unwrap();
        let arr: Value = serde_json::from_str(text).unwrap();
        assert!(arr.as_array().unwrap().is_empty());
    }

    #[test]
    fn test_rework_on_empty_ledger() {
        let id = Some(json!(1));
        let dir = tempdir().unwrap();
        let params = json!({ "name": "signal_rework", "arguments": { "days": 14 } });
        let r = handle_tools_call(&id, &params, dir.path());
        assert!(r.get("result").is_some());
    }

    #[test]
    fn test_json_error_format() {
        let id = Some(json!(99));
        let r = json_error(id.as_ref(), -32700, "Parse error");
        assert_eq!(r["error"]["code"], -32700);
        assert_eq!(r["id"], 99);
    }
}
