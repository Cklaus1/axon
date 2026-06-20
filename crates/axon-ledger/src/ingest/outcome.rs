use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::json;

use crate::hash::record_id;
use crate::model::{Effect, LedgerRecord};
use crate::query::why;
use crate::store::Store;

/// Ingest a metric outcome JSON file and link it causally to a commit.
///
/// `commit_sha_prefix`: short or full SHA of the target commit.
/// `file`: path to a JSON file (e.g. `{"precision": 1.0, "recall": 0.52}`).
///
/// The new MetricOutcome record gets `causal_parent = commit.id`.
pub fn ingest_outcome(commit_sha_prefix: &str, file: &Path, store: &mut Store) -> Result<LedgerRecord> {
    // Resolve the commit record to get its id for causal linking
    let why_result = why(commit_sha_prefix, store)
        .with_context(|| format!("Commit '{}' not found in ledger — run `ingest git` first", commit_sha_prefix))?;
    let commit_id = why_result.commit.id.clone();
    let commit_sha = why_result.commit.payload
        .get("sha").and_then(|v| v.as_str()).unwrap_or(commit_sha_prefix).to_string();

    let raw = fs::read_to_string(file)
        .with_context(|| format!("Cannot read outcome file: {}", file.display()))?;
    let metrics: serde_json::Value = serde_json::from_str(&raw)
        .with_context(|| format!("Outcome file is not valid JSON: {}", file.display()))?;

    let ts_ms = why_result.commit.ts_ms + 1; // slightly after the commit

    let payload = json!({
        "commit_sha": commit_sha,
        "file": file.to_string_lossy(),
        "metrics": metrics,
    });

    let id = record_id(
        &format!("outcome:{}", commit_sha),
        &Effect::MetricOutcome,
        ts_ms,
        &payload,
    );

    let record = LedgerRecord {
        id,
        principal: format!("outcome:{}", commit_sha),
        effect: Effect::MetricOutcome,
        causal_parent: Some(commit_id),
        ts_ms,
        payload,
        repo: None,
    };

    store.append(&record)?;
    Ok(record)
}
