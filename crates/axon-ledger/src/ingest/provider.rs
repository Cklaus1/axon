/// Outcome ingestion from observability providers.
///
/// Translates Datadog, Sentry, and PostHog webhook payloads (or their exported
/// JSON formats) into normalized `MetricOutcome` ledger records linked to the
/// git commit that caused them.
///
/// Usage:
///   axon-ledger ingest outcome-provider --provider datadog --commit <sha> --file event.json
///   axon-ledger ingest outcome-provider --provider sentry   --commit <sha> --file issue.json
///   axon-ledger ingest outcome-provider --provider posthog  --commit <sha> --file event.json
///   axon-ledger ingest outcome-provider --provider generic  --commit <sha> --file metrics.json
///
/// The `generic` provider accepts any flat JSON object of numeric/bool metrics.
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::{json, Value};

use crate::hash::record_id;
use crate::model::{Effect, LedgerRecord};
use crate::query::why;
use crate::store::Store;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Provider {
    Datadog,
    Sentry,
    PostHog,
    Generic,
}

impl Provider {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "datadog" | "dd" => Ok(Self::Datadog),
            "sentry" => Ok(Self::Sentry),
            "posthog" | "ph" => Ok(Self::PostHog),
            "generic" | "json" => Ok(Self::Generic),
            other => anyhow::bail!("Unknown provider: {other}. Valid: datadog, sentry, posthog, generic"),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Datadog => "datadog",
            Self::Sentry => "sentry",
            Self::PostHog => "posthog",
            Self::Generic => "generic",
        }
    }
}

/// Normalize a provider-specific JSON payload into a flat metrics map.
///
/// Returns `{ metric_name: value, ... }` where values are f64 or bool.
pub fn normalize_payload(provider: Provider, raw: &Value) -> Value {
    match provider {
        Provider::Datadog => normalize_datadog(raw),
        Provider::Sentry => normalize_sentry(raw),
        Provider::PostHog => normalize_posthog(raw),
        Provider::Generic => normalize_generic(raw),
    }
}

fn normalize_datadog(raw: &Value) -> Value {
    // Datadog event/monitor webhook shape:
    //   { "metric": "...", "value": N, "tags": [...], "status": "...", ... }
    // Also handles the series export format:
    //   { "series": [{ "metric": "...", "points": [[ts, val], ...] }] }
    let mut metrics = serde_json::Map::new();

    // Single metric event
    if let (Some(metric), Some(value)) = (
        raw.get("metric").and_then(|v| v.as_str()),
        raw.get("value").and_then(|v| v.as_f64()),
    ) {
        metrics.insert(metric.replace('.', "_"), json!(value));
    }

    // Status from monitor alert
    if let Some(status) = raw.get("alert_status").and_then(|v| v.as_str()) {
        metrics.insert("alert_triggered".to_string(), json!(status != "recovered"));
    }
    if let Some(status) = raw.get("status").and_then(|v| v.as_str()) {
        metrics.insert("status_ok".to_string(), json!(status == "OK" || status == "Recovered"));
    }

    // Series export
    if let Some(series) = raw.get("series").and_then(|v| v.as_array()) {
        for s in series {
            if let Some(metric) = s.get("metric").and_then(|v| v.as_str()) {
                // Take the last point value
                if let Some(last_val) = s.get("points")
                    .and_then(|p| p.as_array())
                    .and_then(|pts| pts.last())
                    .and_then(|pt| pt.as_array())
                    .and_then(|pair| pair.get(1))
                    .and_then(|v| v.as_f64())
                {
                    metrics.insert(metric.replace('.', "_"), json!(last_val));
                }
            }
        }
    }

    // Tags as metadata
    if let Some(tags) = raw.get("tags").and_then(|v| v.as_array()) {
        let tag_strs: Vec<String> = tags.iter()
            .filter_map(|t| t.as_str().map(String::from))
            .collect();
        metrics.insert("tags".to_string(), json!(tag_strs));
    }

    if metrics.is_empty() {
        // Fallback: treat top-level numeric fields as metrics
        normalize_generic(raw)
    } else {
        Value::Object(metrics)
    }
}

fn normalize_sentry(raw: &Value) -> Value {
    // Sentry issue webhook / export shape:
    //   { "level": "error", "count": N, "userCount": N, "title": "...", "action": "...", ... }
    let mut metrics = serde_json::Map::new();

    if let Some(count) = raw.get("count").and_then(|v| v.as_f64()) {
        metrics.insert("event_count".to_string(), json!(count));
    }
    if let Some(users) = raw.get("userCount").and_then(|v| v.as_f64()) {
        metrics.insert("affected_users".to_string(), json!(users));
    }
    if let Some(level) = raw.get("level").and_then(|v| v.as_str()) {
        metrics.insert("is_error".to_string(), json!(level == "error" || level == "fatal"));
        metrics.insert("is_fatal".to_string(), json!(level == "fatal"));
        metrics.insert("level".to_string(), json!(level));
    }
    if let Some(action) = raw.get("action").and_then(|v| v.as_str()) {
        metrics.insert("resolved".to_string(), json!(action == "resolved"));
        metrics.insert("regressed".to_string(), json!(action == "regression"));
    }
    if let Some(title) = raw.get("title").and_then(|v| v.as_str()) {
        metrics.insert("title".to_string(), json!(title));
    }
    if let Some(freq) = raw.get("frequency").and_then(|v| v.as_f64()) {
        metrics.insert("frequency".to_string(), json!(freq));
    }

    // Nested data.count shape from Sentry API
    if let Some(data) = raw.get("data") {
        if let Some(count) = data.get("count").and_then(|v| v.as_f64()) {
            metrics.entry("event_count".to_string()).or_insert(json!(count));
        }
    }

    if metrics.is_empty() { normalize_generic(raw) } else { Value::Object(metrics) }
}

fn normalize_posthog(raw: &Value) -> Value {
    // PostHog event shape (from event API export or webhook):
    //   { "event": "...", "properties": { "$feature_flag_response": "...", "value": N, ... } }
    // Or insight/trend export:
    //   { "result": [{ "count": N, "data": [...], "label": "..." }] }
    let mut metrics = serde_json::Map::new();

    // Event webhook
    if let Some(event) = raw.get("event").and_then(|v| v.as_str()) {
        metrics.insert("event_name".to_string(), json!(event));
    }
    if let Some(props) = raw.get("properties").and_then(|v| v.as_object()) {
        for (k, v) in props {
            if k.starts_with('$') { continue; } // skip PostHog internal props
            if v.is_f64() || v.is_i64() || v.is_u64() || v.is_boolean() {
                let safe_key = k.replace(['.', '-', ' '], "_");
                metrics.insert(safe_key, v.clone());
            }
        }
    }

    // Insight/trend result
    if let Some(result) = raw.get("result").and_then(|v| v.as_array()) {
        for r in result {
            if let (Some(label), Some(count)) = (
                r.get("label").and_then(|v| v.as_str()),
                r.get("count").and_then(|v| v.as_f64()),
            ) {
                let safe = label.replace(['.', '-', ' '], "_").to_lowercase();
                metrics.insert(safe, json!(count));
            }
        }
    }

    if metrics.is_empty() { normalize_generic(raw) } else { Value::Object(metrics) }
}

fn normalize_generic(raw: &Value) -> Value {
    // Accept any flat JSON object — pass numeric and bool values through,
    // skip nested objects and arrays (they'd need provider-specific handling).
    let mut metrics = serde_json::Map::new();
    if let Some(obj) = raw.as_object() {
        for (k, v) in obj {
            if v.is_f64() || v.is_i64() || v.is_u64() || v.is_boolean() || v.is_string() {
                let safe_key = k.replace(['.', '-', ' '], "_");
                metrics.insert(safe_key, v.clone());
            }
        }
    }
    Value::Object(metrics)
}

/// Ingest a provider-specific outcome file and link it to a commit.
pub fn ingest_provider_outcome(
    provider: Provider,
    commit_sha_prefix: &str,
    file: &Path,
    store: &mut Store,
) -> Result<LedgerRecord> {
    let why_result = why(commit_sha_prefix, store)
        .with_context(|| format!("Commit '{}' not found — run `ingest git` first", commit_sha_prefix))?;
    let commit_id = why_result.commit.id.clone();
    let commit_sha = why_result.commit.payload
        .get("sha").and_then(|v| v.as_str()).unwrap_or(commit_sha_prefix).to_string();

    let raw_str = fs::read_to_string(file)
        .with_context(|| format!("Cannot read outcome file: {}", file.display()))?;
    let raw: Value = serde_json::from_str(&raw_str)
        .with_context(|| format!("Outcome file is not valid JSON: {}", file.display()))?;

    let metrics = normalize_payload(provider, &raw);
    let ts_ms = why_result.commit.ts_ms + 1;

    let payload = json!({
        "commit_sha": commit_sha,
        "provider": provider.name(),
        "file": file.to_string_lossy(),
        "metrics": metrics,
    });

    let id = record_id(
        &format!("outcome:{}:{}", provider.name(), commit_sha),
        &Effect::MetricOutcome,
        ts_ms,
        &payload,
    );

    let record = LedgerRecord {
        id,
        principal: format!("outcome:{}:{}", provider.name(), commit_sha),
        effect: Effect::MetricOutcome,
        causal_parent: Some(commit_id),
        ts_ms,
        payload,
    };

    store.append(&record)?;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_from_str() {
        assert_eq!(Provider::from_str("datadog").unwrap(), Provider::Datadog);
        assert_eq!(Provider::from_str("sentry").unwrap(), Provider::Sentry);
        assert_eq!(Provider::from_str("posthog").unwrap(), Provider::PostHog);
        assert_eq!(Provider::from_str("generic").unwrap(), Provider::Generic);
        assert!(Provider::from_str("unknown").is_err());
    }

    #[test]
    fn test_normalize_datadog_single_metric() {
        let raw = json!({ "metric": "system.cpu.user", "value": 42.5, "status": "OK" });
        let m = normalize_datadog(&raw);
        assert_eq!(m["system_cpu_user"], 42.5);
        assert_eq!(m["status_ok"], true);
    }

    #[test]
    fn test_normalize_datadog_series() {
        let raw = json!({
            "series": [{ "metric": "web.latency.p99", "points": [[1000, 120.5], [2000, 115.0]] }]
        });
        let m = normalize_datadog(&raw);
        assert_eq!(m["web_latency_p99"], 115.0);
    }

    #[test]
    fn test_normalize_sentry_issue() {
        let raw = json!({ "level": "error", "count": 37, "userCount": 12, "action": "regression", "title": "NullPointerException" });
        let m = normalize_sentry(&raw);
        assert_eq!(m["event_count"].as_f64(), Some(37.0));
        assert_eq!(m["affected_users"].as_f64(), Some(12.0));
        assert_eq!(m["is_error"], true);
        assert_eq!(m["regressed"], true);
    }

    #[test]
    fn test_normalize_sentry_resolved() {
        let raw = json!({ "level": "info", "action": "resolved", "count": 0 });
        let m = normalize_sentry(&raw);
        assert_eq!(m["resolved"], true);
        assert_eq!(m["is_error"], false);
    }

    #[test]
    fn test_normalize_posthog_event() {
        let raw = json!({
            "event": "signup",
            "properties": { "$browser": "Chrome", "conversion_rate": 0.42, "paid": true }
        });
        let m = normalize_posthog(&raw);
        assert_eq!(m["event_name"], "signup");
        assert_eq!(m["conversion_rate"], 0.42);
        assert_eq!(m["paid"], true);
        assert!(m.get("$browser").is_none(), "internal props should be skipped");
    }

    #[test]
    fn test_normalize_posthog_insight() {
        let raw = json!({
            "result": [
                { "label": "Page Views", "count": 1500 },
                { "label": "Sign Ups", "count": 42 }
            ]
        });
        let m = normalize_posthog(&raw);
        assert_eq!(m["page_views"].as_f64(), Some(1500.0));
        assert_eq!(m["sign_ups"].as_f64(), Some(42.0));
    }

    #[test]
    fn test_normalize_generic_flat() {
        let raw = json!({ "p99_ms": 120.5, "error_rate": 0.02, "ok": true, "label": "prod" });
        let m = normalize_generic(&raw);
        assert_eq!(m["p99_ms"], 120.5);
        assert_eq!(m["error_rate"], 0.02);
        assert_eq!(m["ok"], true);
        assert_eq!(m["label"], "prod");
    }

    #[test]
    fn test_normalize_generic_skips_nested() {
        let raw = json!({ "count": 5, "nested": { "a": 1 } });
        let m = normalize_generic(&raw);
        assert_eq!(m["count"], 5);
        assert!(m.get("nested").is_none());
    }
}
