/// Training data export for Trainloop / MegaBrain fine-tuning pipeline.
///
/// Exports high-signal sessions as (prompt, response) pairs in Trainloop-compatible
/// JSONL format, filtered by effectiveness score. Supports both standard fine-tuning
/// format and DPO (chosen/rejected) pairs for contrastive training.
///
/// See docs/TRAINLOOP_INTEGRATION.md for the full pipeline design.
use std::io::Write;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use axon_ledger::model::Effect;
use axon_ledger::store::Store;
use crate::score::{score_sessions, SessionScore, TrainingTier};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ExportFormat {
    /// Standard fine-tuning: {"messages": [...], "metadata": {...}}
    Trainloop,
    /// DPO pairs: {"prompt": "...", "chosen": [...], "rejected": [...]}
    Dpo,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TrainloopRecord {
    pub messages: Vec<Message>,
    pub metadata: TrainloopMeta,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct Message {
    pub role: String,
    pub content: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct TrainloopMeta {
    pub session_id: String,
    pub signal_score: u8,
    pub goal: String,
    pub commits_produced: usize,
    pub training_tier: String,
    pub engineer: String,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DpoPair {
    pub prompt: String,
    pub chosen: Vec<Message>,
    pub rejected: Vec<Message>,
    pub metadata: DpoMeta,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DpoMeta {
    pub chosen_session_id: String,
    pub rejected_session_id: String,
    pub chosen_score: u8,
    pub rejected_score: u8,
    pub goal_similarity: f64,
}

/// Export options.
pub struct ExportOptions {
    pub format: ExportFormat,
    pub min_score: u8,
    pub since_ms: Option<u64>,
    pub anonymize: bool,
    pub exclude_code_content: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            format: ExportFormat::Trainloop,
            min_score: 75,
            since_ms: None,
            anonymize: false,
            exclude_code_content: false,
        }
    }
}

/// Export training data to a writer (JSONL, one record per line).
pub fn export_training<W: Write>(
    store: &Store,
    opts: &ExportOptions,
    out: &mut W,
) -> Result<usize> {
    let scores = score_sessions(store)?;

    // Filter by score and time window
    let eligible: Vec<SessionScore> = scores.into_iter()
        .filter(|s| s.score >= opts.min_score)
        .filter(|s| opts.since_ms.map_or(true, |ms| s.ts_ms >= ms))
        .filter(|s| matches!(s.training_tier, TrainingTier::PositiveGold | TrainingTier::PositiveSilver))
        .collect();

    // Load raw session file paths from ledger to extract conversation turns
    let all = store.all().map_err(anyhow::Error::from)?;
    let session_files: std::collections::HashMap<String, String> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .filter_map(|r| {
            let sid = r.payload.get("session_id")?.as_str()?.to_string();
            let path = r.payload.get("file_path")?.as_str()?.to_string();
            Some((sid, path))
        })
        .collect();

    let mut exported = 0;
    for session in &eligible {
        let file_path = match session_files.get(&session.session_id) {
            Some(p) => p,
            None => continue, // no raw file available
        };

        let messages = match load_messages(file_path, opts.exclude_code_content) {
            Ok(m) if !m.is_empty() => m,
            _ => continue,
        };

        let engineer = if opts.anonymize {
            format!("engineer_{}", &session.session_id[..4])
        } else {
            session.engineer.clone()
        };

        let record = TrainloopRecord {
            messages,
            metadata: TrainloopMeta {
                session_id: session.session_id.clone(),
                signal_score: session.score,
                goal: session.goal.clone(),
                commits_produced: session.commits_linked,
                training_tier: session.training_tier.label().to_string(),
                engineer,
            },
        };

        let line = serde_json::to_string(&record)?;
        writeln!(out, "{line}")?;
        exported += 1;
    }

    Ok(exported)
}

/// Export DPO pairs: match high-score sessions with low-score sessions on similar goals.
pub fn export_dpo<W: Write>(
    store: &Store,
    opts: &ExportOptions,
    out: &mut W,
) -> Result<usize> {
    let scores = score_sessions(store)?;

    let positives: Vec<&SessionScore> = scores.iter()
        .filter(|s| s.score >= opts.min_score)
        .filter(|s| matches!(s.training_tier, TrainingTier::PositiveGold))
        .collect();

    let negatives: Vec<&SessionScore> = scores.iter()
        .filter(|s| s.training_tier == TrainingTier::Negative)
        .collect();

    let all = store.all().map_err(anyhow::Error::from)?;
    let session_files: std::collections::HashMap<String, String> = all.iter()
        .filter(|r| r.effect == Effect::AgentSession)
        .filter_map(|r| {
            let sid = r.payload.get("session_id")?.as_str()?.to_string();
            let path = r.payload.get("file_path")?.as_str()?.to_string();
            Some((sid, path))
        })
        .collect();

    let mut exported = 0;
    for pos in &positives {
        // Find a negative session: prefer one with same leading verb, fall back to any negative
        let pos_verb = pos.goal.split_whitespace().next().unwrap_or("").to_lowercase();
        let neg_by_verb = negatives.iter().find(|n| {
            n.goal.to_lowercase().starts_with(&pos_verb) && n.session_id != pos.session_id
        });
        let verb_matched = neg_by_verb.is_some();
        let neg = match neg_by_verb.or_else(|| negatives.iter().find(|n| n.session_id != pos.session_id)) {
            Some(n) => n,
            None => continue,
        };

        let pos_file = match session_files.get(&pos.session_id) { Some(p) => p, None => continue };
        let neg_file = match session_files.get(&neg.session_id) { Some(p) => p, None => continue };

        let chosen = match load_messages(pos_file, opts.exclude_code_content) {
            Ok(m) if !m.is_empty() => m,
            _ => continue,
        };
        let rejected = match load_messages(neg_file, opts.exclude_code_content) {
            Ok(m) if !m.is_empty() => m,
            _ => continue,
        };

        let pair = DpoPair {
            prompt: pos.goal.clone(),
            chosen,
            rejected,
            metadata: DpoMeta {
                chosen_session_id: pos.session_id.clone(),
                rejected_session_id: neg.session_id.clone(),
                chosen_score: pos.score,
                rejected_score: neg.score,
                goal_similarity: if verb_matched { 0.7 } else { 0.3 },
            },
        };

        let line = serde_json::to_string(&pair)?;
        writeln!(out, "{line}")?;
        exported += 1;
    }

    Ok(exported)
}

/// Load conversation turns from a Claude Code JSONL session file.
fn load_messages(path: &str, exclude_code: bool) -> Result<Vec<Message>> {
    let content = std::fs::read_to_string(path)?;
    let mut messages = Vec::new();

    for line in content.lines() {
        let event: Value = match serde_json::from_str(line) { Ok(v) => v, Err(_) => continue };
        let event_type = event.get("type").and_then(|v| v.as_str()).unwrap_or("");

        let role = match event_type {
            "user" => "user",
            "assistant" => "assistant",
            _ => continue,
        };

        let content_val = event.get("message")
            .and_then(|m| m.get("content"))
            .or_else(|| event.get("content"));

        let text = match content_val {
            Some(Value::String(s)) => s.clone(),
            Some(Value::Array(blocks)) => {
                blocks.iter()
                    .filter_map(|b| b.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("\n")
            }
            _ => continue,
        };

        if text.trim().is_empty() { continue; }

        let text = if exclude_code {
            // Strip content after common code-dump markers
            let markers = ["Here is the file:", "```", "Current file contents:"];
            let mut t = text.as_str();
            for m in &markers {
                if let Some(pos) = t.find(m) {
                    t = &t[..pos];
                }
            }
            t.trim().to_string()
        } else {
            text
        };

        if !text.trim().is_empty() {
            messages.push(Message { role: role.to_string(), content: text });
        }
    }

    Ok(messages)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_export_empty_store() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        let opts = ExportOptions::default();
        let mut buf = Vec::new();
        let n = export_training(&store, &opts, &mut buf).unwrap();
        assert_eq!(n, 0);
        assert!(buf.is_empty());
    }

    #[test]
    fn test_load_messages_missing_file() {
        let result = load_messages("/nonexistent/path.jsonl", false);
        assert!(result.is_err());
    }
}
