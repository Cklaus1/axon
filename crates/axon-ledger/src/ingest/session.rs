use std::collections::HashSet;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::json;

use crate::gate::run_brief_gate;
use crate::hash::record_id;
use crate::model::{Effect, LedgerRecord};
use crate::store::Store;

/// Options for the brief gate check during session ingestion.
#[derive(Default)]
pub struct GateOptions {
    /// Enable the brief gate check (default: false = skip).
    pub enabled: bool,
    /// Explicit path to the `axon` binary (auto-discovered if None).
    pub axon_bin: Option<PathBuf>,
    /// Explicit path to `brief-gate.ax` (auto-discovered if None).
    pub gate_script: Option<PathBuf>,
}

/// Parse ISO 8601 timestamp string into unix milliseconds.
/// Handles formats: `YYYY-MM-DDTHH:MM:SS.sssZ`, `YYYY-MM-DDTHH:MM:SSZ`,
/// and `YYYY-MM-DDTHH:MM:SS+HH:MM` (offset ignored, treated as UTC).
pub fn parse_iso_to_ms(s: &str) -> Option<u64> {
    // Strip trailing Z or offset like +00:00
    let s = s.trim();
    let dt_part = if let Some(pos) = s.find('Z') {
        &s[..pos]
    } else if let Some(pos) = s.rfind('+') {
        if pos > 10 {
            &s[..pos]
        } else {
            s
        }
    } else if let Some(pos) = s.rfind('-') {
        // Check if it's the date separator or offset separator
        if pos > 10 {
            &s[..pos]
        } else {
            s
        }
    } else {
        s
    };

    // dt_part is now like YYYY-MM-DDTHH:MM:SS or YYYY-MM-DDTHH:MM:SS.sss
    let (date_part, time_part) = dt_part.split_once('T')?;

    let date_parts: Vec<&str> = date_part.split('-').collect();
    if date_parts.len() < 3 {
        return None;
    }
    let year: i64 = date_parts[0].parse().ok()?;
    let month: i64 = date_parts[1].parse().ok()?;
    let day: i64 = date_parts[2].parse().ok()?;

    // Split milliseconds if present
    let (hms_part, ms_part) = if let Some(dot_pos) = time_part.find('.') {
        let ms_str = &time_part[dot_pos + 1..];
        let ms: u64 = ms_str.chars().take(3).collect::<String>().parse().ok()?;
        (&time_part[..dot_pos], ms)
    } else {
        (time_part, 0u64)
    };

    let time_parts: Vec<&str> = hms_part.split(':').collect();
    if time_parts.len() < 3 {
        return None;
    }
    let hour: i64 = time_parts[0].parse().ok()?;
    let minute: i64 = time_parts[1].parse().ok()?;
    let second: i64 = time_parts[2].parse().ok()?;

    // Compute days since Unix epoch (1970-01-01)
    let days = days_since_epoch(year, month, day)?;
    let total_secs = days * 86400 + hour * 3600 + minute * 60 + second;
    if total_secs < 0 {
        return None;
    }
    let total_ms = (total_secs as u64) * 1000 + ms_part;
    Some(total_ms)
}

fn days_since_epoch(year: i64, month: i64, day: i64) -> Option<i64> {
    // Use the Julian Day Number formula then subtract epoch Julian Day
    // Simplified: compute from 1970-01-01
    let y = if month <= 2 { year - 1 } else { year };
    let m = if month <= 2 { month + 12 } else { month };
    let a = y / 100;
    let b = 2 - a + a / 4;
    let jd = ((365.25 * (y + 4716) as f64) as i64)
        + ((30.6001 * (m + 1) as f64) as i64)
        + day
        + b
        - 1524;
    // Julian Day of 1970-01-01 is 2440588
    Some(jd - 2440588)
}

/// Strip markdown formatting from a raw first-user-message to get a clean goal summary.
///
/// Handles the common pattern of `/loop` goals like:
///   `# Goal: fix the auth bug\n\nMore context here...`
/// → `fix the auth bug`
///
/// And plain messages like:
///   `analyze this project, whats the status`
/// → unchanged
fn clean_goal_text(raw: &str) -> String {
    // Walk lines, skip blanks, heading-only lines, and pipe-table rows (│ / |)
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Skip markdown table rows — lines containing │ or starting with |
        if line.contains('│') || line.starts_with('|') {
            continue;
        }
        // Strip leading # markers (e.g. "# Goal: ..." → "Goal: ..." → strip "Goal: " prefix too)
        let without_hashes = line.trim_start_matches('#').trim();
        if without_hashes.is_empty() {
            continue;
        }
        // Common pattern: "Goal: <text>" or "Goal:\n<text>" — strip the label
        let text = if let Some(rest) = without_hashes.strip_prefix("Goal:") {
            rest.trim()
        } else {
            without_hashes
        };
        if text.is_empty() {
            continue;
        }
        // Take up to 120 chars of this first content line (Unicode-safe)
        return text.chars().take(120).collect();
    }
    // All lines were empty or table rows — no suitable goal text
    "(structured session)".to_string()
}

pub fn ingest_session(
    session_path: &Path,
    store: &mut Store,
    gate: &GateOptions,
    repo_name: Option<&str>,
    engineer: Option<&str>,
) -> Result<Option<LedgerRecord>> {
    let session_id = session_path
        .file_stem()
        .and_then(|s| s.to_str())
        .map(String::from)
        .context("Could not determine session id from file path")?;

    // Dedup: check if session already in store
    let existing = store.find_by_effect(&Effect::AgentSession)?;
    for r in &existing {
        if r.payload
            .get("session_id")
            .and_then(|v| v.as_str())
            .map(|s| s == session_id)
            .unwrap_or(false)
        {
            return Ok(None);
        }
    }

    let file = File::open(session_path)
        .with_context(|| format!("Cannot open session file: {}", session_path.display()))?;
    let reader = BufReader::new(file);

    let mut turn_count: u64 = 0;
    let mut first_ts: Option<String> = None;
    let mut last_ts: Option<String> = None;
    let mut files_touched: HashSet<String> = HashSet::new();
    let mut goal_text: Option<String> = None; // first user message text

    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        let event: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Capture timestamps
        if let Some(ts) = event.get("timestamp").and_then(|v| v.as_str()) {
            if first_ts.is_none() {
                first_ts = Some(ts.to_string());
            }
            last_ts = Some(ts.to_string());
        }

        // Claude Code JSONL: type="user" = human turn, type="assistant" = model turn
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if event_type == "user" {
            turn_count += 1;
        }
        if goal_text.is_none() {
            if let Some(text) = extract_first_user_text(&event) {
                goal_text = Some(text);
            }
        }

        // Extract files from tool_use content blocks.
        // In Claude Code JSONL the content array lives at event.message.content
        // (for user/assistant events) or at event.content (for some tool events).
        let content_sources: &[&str] = &["message", "content"];
        let content_arr = content_sources.iter().find_map(|key| {
            if *key == "message" {
                event.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array())
            } else {
                event.get("content").and_then(|c| c.as_array())
            }
        });
        if let Some(content) = content_arr
        {
            for block in content {
                if block.get("type").and_then(|v| v.as_str()) == Some("tool_use") {
                    let tool_name = block.get("name").and_then(|v| v.as_str()).unwrap_or("");
                    let input = block.get("input").cloned().unwrap_or(serde_json::Value::Null);

                    match tool_name {
                        "Edit" | "Write" => {
                            if let Some(fp) = input.get("file_path").and_then(|v| v.as_str()) {
                                files_touched.insert(fp.to_string());
                            }
                        }
                        "Bash" => {
                            if let Some(cmd) = input.get("command").and_then(|v| v.as_str()) {
                                extract_file_paths_from_command(cmd, &mut files_touched);
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    let start_ts = first_ts.clone().unwrap_or_default();
    let end_ts = last_ts.unwrap_or_default();
    let ts_ms = first_ts
        .as_deref()
        .and_then(parse_iso_to_ms)
        .unwrap_or(0);

    let mut files_list: Vec<String> = files_touched.into_iter().collect();
    files_list.sort();

    // Filter out Claude Code system-injected caveat headers that start with '<'
    // (e.g. "<local-command-caveat>...") — these are not real user goals.
    let raw_goal = goal_text
        .as_deref()
        .filter(|g| !g.starts_with('<'))
        .unwrap_or("(no user goal found in session)");
    let goal_owned = clean_goal_text(raw_goal);
    let goal = goal_owned.as_str();

    // ── Brief gate ────────────────────────────────────────────────────────────
    if gate.enabled {
        let outcome = run_brief_gate(
            goal,
            &session_id,
            gate.axon_bin.as_deref(),
            gate.gate_script.as_deref(),
        )?;
        if !outcome.passed {
            anyhow::bail!("brief gate: {}", outcome.reason);
        }
        // Log the gate reason when it was available and passed (soft-pass is silent)
        if !outcome.reason.contains("skipped") {
            eprintln!("[ledger] {}", outcome.reason);
        }
    }

    let summary = format!(
        "{} ({} turns, {} files)",
        goal.chars().take(120).collect::<String>(),
        turn_count,
        files_list.len()
    );

    let payload = json!({
        "session_id": session_id,
        "file_path": session_path.to_string_lossy(),
        "start_ts": start_ts,
        "end_ts": end_ts,
        "files_touched": files_list,
        "turn_count": turn_count,
        "goal": goal,
        "summary": summary,
    });

    let id = record_id(
        &format!("agent:{}", session_id),
        &Effect::AgentSession,
        ts_ms,
        &payload,
    );

    let principal = engineer
        .map(String::from)
        .unwrap_or_else(|| format!("agent:{}", session_id));

    let record = LedgerRecord {
        id,
        principal,
        effect: Effect::AgentSession,
        causal_parent: None,
        ts_ms,
        payload,
        repo: repo_name.map(String::from),
    };

    store.append(&record)?;
    Ok(Some(record))
}

fn extract_file_paths_from_command(cmd: &str, files: &mut HashSet<String>) {
    let extensions = [".rs", ".ax", ".toml", ".md", ".json", ".jsonl", ".sh", ".lock"];
    for token in cmd.split_whitespace() {
        let token = token.trim_matches(|c: char| {
            matches!(c, '"' | '\'' | ';' | '|' | '(' | ')' | ',' )
        });
        // Reject CLI flags, glob patterns, shell metacharacters
        if token.starts_with('-')
            || token.starts_with('!')
            || token.contains('*')
            || token.contains('+')
            || token.contains('=')
            || token.contains('[')
            || token.contains(' ')
        {
            continue;
        }
        // Must end with a known extension
        let Some(ext) = extensions.iter().find(|e| token.ends_with(*e)) else {
            continue;
        };
        // Basename (the part before the extension) must be non-empty
        let basename_end = token.len() - ext.len();
        let basename = &token[..basename_end];
        if basename.is_empty() {
            continue;
        }
        // No interior path segment should itself look like a source file
        // (catches artifacts like "gate.sh/parity_all.sh", "expr.rs/mod.rs")
        let segments: Vec<&str> = token.split('/').collect();
        let interior_ok = segments[..segments.len().saturating_sub(1)]
            .iter()
            .all(|seg| !extensions.iter().any(|e| seg.ends_with(e)));
        if !interior_ok {
            continue;
        }
        files.insert(token.to_string());
    }
}

/// Extract the text of the first user message from a session event.
/// Handles both plain-string content and content-block arrays.
fn extract_first_user_text(event: &serde_json::Value) -> Option<String> {
    let msg = event.get("message")?;
    let role = msg.get("role").and_then(|v| v.as_str())?;
    if role != "user" {
        return None;
    }
    let content = msg.get("content")?;
    let text = if let Some(s) = content.as_str() {
        s.to_string()
    } else if let Some(arr) = content.as_array() {
        arr.iter()
            .filter_map(|block| {
                if block.get("type").and_then(|v| v.as_str()) == Some("text") {
                    block.get("text").and_then(|v| v.as_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
            .join(" ")
    } else {
        return None;
    };
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return None;
    }
    // Cap at 200 chars for use as goal text
    Some(trimmed.chars().take(200).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_goal_markdown_header() {
        assert_eq!(
            clean_goal_text("# Goal: fix the auth bug\n\nMore context here"),
            "fix the auth bug"
        );
    }

    #[test]
    fn test_clean_goal_plain_text() {
        assert_eq!(
            clean_goal_text("analyze this project, whats the status"),
            "analyze this project, whats the status"
        );
    }

    #[test]
    fn test_clean_goal_double_hash() {
        assert_eq!(
            clean_goal_text("## complete the remaining Axon roadmap requirements\n\nSome more text"),
            "complete the remaining Axon roadmap requirements"
        );
    }

    #[test]
    fn test_clean_goal_hash_no_label() {
        assert_eq!(
            clean_goal_text("# migrate payments to Stripe\n\n- step 1"),
            "migrate payments to Stripe"
        );
    }

    #[test]
    fn test_clean_goal_empty_header_then_content() {
        assert_eq!(
            clean_goal_text("# \n\nactual goal text here"),
            "actual goal text here"
        );
    }

    #[test]
    fn test_parse_iso_to_ms_basic() {
        // 1970-01-01T00:00:00.000Z should be 0ms
        assert_eq!(parse_iso_to_ms("1970-01-01T00:00:00.000Z"), Some(0));
    }

    #[test]
    fn test_parse_iso_to_ms_with_offset() {
        let ts = "2026-06-20T15:58:18.700Z";
        let ms = parse_iso_to_ms(ts);
        assert!(ms.is_some());
        assert!(ms.unwrap() > 0);
    }

    #[test]
    fn test_clean_goal_pipe_table_skipped() {
        let raw = "│ Phase │ Status │\n│ 1 │ ✅ │\nadd payment integration to checkout.rs";
        assert_eq!(clean_goal_text(raw), "add payment integration to checkout.rs");
    }

    #[test]
    fn test_clean_goal_unicode_truncate() {
        // Goal with Unicode em-dashes and box-drawing chars should not panic on 120-char limit
        let long_goal = "fix the auth bug — reproduces when device clock drift exceeds 5 min; affects all OAuth flows; see auth/jwt.go line 42 for the failing assertion that needs a leeway parameter";
        let result = clean_goal_text(long_goal);
        assert!(result.len() <= 200, "should not exceed byte length for reasonable input");
        assert!(!result.contains("│"));
    }
}
