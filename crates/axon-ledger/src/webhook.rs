/// Webhook egress for ledger events.
///
/// Webhooks are configured per-ledger in `<ledger-dir>/webhooks.json`.
/// Currently supported events: `unexplained-deploy` (fired by `pre-deploy`
/// when commits with no linked AI session are found).
///
/// Supported providers: `slack` (Incoming Webhooks) and `pagerduty` (Events API v2).
///
/// Delivery uses `curl` so no extra Rust dependencies are needed. Failures are
/// logged to stderr but do not block the caller — webhook delivery is best-effort.
use std::path::Path;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// Supported webhook event types.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum WebhookEvent {
    /// Fired when `pre-deploy` finds commits with no linked AI session.
    UnexplainedDeploy,
}

impl WebhookEvent {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "unexplained-deploy" => Ok(Self::UnexplainedDeploy),
            other => anyhow::bail!("Unknown event: {other}. Valid: unexplained-deploy"),
        }
    }
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::UnexplainedDeploy => "unexplained-deploy",
        }
    }
}

/// Supported webhook providers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WebhookProvider {
    /// Slack Incoming Webhook URL.
    Slack,
    /// PagerDuty Events API v2 integration URL.
    PagerDuty,
    /// Generic HTTP POST — sends the raw JSON payload.
    Generic,
}

impl WebhookProvider {
    pub fn from_str(s: &str) -> Result<Self> {
        match s.to_lowercase().as_str() {
            "slack" => Ok(Self::Slack),
            "pagerduty" | "pd" => Ok(Self::PagerDuty),
            "generic" | "http" => Ok(Self::Generic),
            other => anyhow::bail!("Unknown provider: {other}. Valid: slack, pagerduty, generic"),
        }
    }
}

/// A registered webhook.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: String,
    pub event: WebhookEvent,
    pub provider: WebhookProvider,
    pub url: String,
}

/// Load webhooks from `<ledger-dir>/webhooks.json`. Returns empty vec if file absent.
pub fn load_webhooks(ledger_dir: &Path) -> Result<Vec<Webhook>> {
    let path = ledger_dir.join("webhooks.json");
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)?;
    Ok(serde_json::from_str(&raw).unwrap_or_default())
}

/// Persist webhooks to `<ledger-dir>/webhooks.json`.
pub fn save_webhooks(ledger_dir: &Path, hooks: &[Webhook]) -> Result<()> {
    let path = ledger_dir.join("webhooks.json");
    let raw = serde_json::to_string_pretty(hooks)?;
    std::fs::write(path, raw)?;
    Ok(())
}

/// Add a new webhook. Returns the new hook's id.
pub fn add_webhook(ledger_dir: &Path, event: WebhookEvent, provider: WebhookProvider, url: &str) -> Result<String> {
    let mut hooks = load_webhooks(ledger_dir)?;
    let id = format!("wh_{:08x}", hooks.len() + 1);
    hooks.push(Webhook { id: id.clone(), event, provider, url: url.to_string() });
    save_webhooks(ledger_dir, &hooks)?;
    Ok(id)
}

/// Remove a webhook by id. Returns true if found and removed.
pub fn remove_webhook(ledger_dir: &Path, id: &str) -> Result<bool> {
    let mut hooks = load_webhooks(ledger_dir)?;
    let before = hooks.len();
    hooks.retain(|h| h.id != id);
    if hooks.len() < before {
        save_webhooks(ledger_dir, &hooks)?;
        Ok(true)
    } else {
        Ok(false)
    }
}

/// Fire all webhooks registered for an event, delivering a structured payload.
/// Failures are printed to stderr but never propagate — best-effort delivery.
pub fn fire_webhooks(ledger_dir: &Path, event: &WebhookEvent, payload: &Value) {
    let hooks = match load_webhooks(ledger_dir) {
        Ok(h) => h,
        Err(e) => { eprintln!("axon-ledger webhook: could not load config: {e}"); return; }
    };

    for hook in hooks.iter().filter(|h| &h.event == event) {
        match deliver_webhook(hook, payload) {
            Ok(code) if code == "200" || code == "202" || code == "204" =>
                eprintln!("axon-ledger webhook: {} fired → HTTP {code}", hook.id),
            Ok(code) =>
                eprintln!("axon-ledger webhook: {} returned HTTP {code} — check the URL or key", hook.id),
            Err(e) =>
                eprintln!("axon-ledger webhook: {} delivery failed (curl?): {e}", hook.id),
        }
    }
}

fn deliver_webhook(hook: &Webhook, payload: &Value) -> Result<String> {
    let body = format_payload(&hook.provider, payload);
    let body_str = serde_json::to_string(&body)?;

    let mut cmd = std::process::Command::new("curl");
    cmd.args(["-s", "-o", "/dev/null", "-w", "%{http_code}", "-X", "POST"]);

    match &hook.provider {
        WebhookProvider::PagerDuty => {
            cmd.args(["-H", "Content-Type: application/json"]);
        }
        _ => {
            cmd.args(["-H", "Content-Type: application/json"]);
        }
    }

    cmd.args(["-d", &body_str, &hook.url]);
    let out = cmd.output()?;
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn format_payload(provider: &WebhookProvider, raw: &Value) -> Value {
    match provider {
        WebhookProvider::Slack => format_slack(raw),
        WebhookProvider::PagerDuty => format_pagerduty(raw),
        WebhookProvider::Generic => raw.clone(),
    }
}

fn format_slack(raw: &Value) -> Value {
    let unexplained = raw["unexplained"].as_u64().unwrap_or(0);
    let total = raw["total_commits"].as_u64().unwrap_or(0);
    let range = raw["range"].as_str().unwrap_or("unknown range");
    let coverage = raw["coverage_pct"].as_u64().unwrap_or(0);

    let header = if unexplained == 0 {
        format!("✅ Pre-deploy: all {total} commits explained ({coverage}%)")
    } else {
        format!("⚠️ Pre-deploy: {unexplained}/{total} commits UNEXPLAINED ({coverage}% coverage)")
    };

    let mut blocks = vec![
        json!({
            "type": "header",
            "text": { "type": "plain_text", "text": header }
        }),
        json!({
            "type": "section",
            "text": { "type": "mrkdwn",
                "text": format!("*Range:* `{range}`\n*Explained:* {}/{total} commits", total - unexplained) }
        }),
    ];

    // List unexplained commits
    if let Some(arr) = raw["unexplained_commits"].as_array() {
        if !arr.is_empty() {
            let lines: Vec<String> = arr.iter().take(5).map(|c| {
                let sha = c["sha"].as_str().unwrap_or("?");
                let msg = c["message"].as_str().unwrap_or("?");
                let author = c["author"].as_str().unwrap_or("?");
                format!("• `{}` {} — {}", &sha[..sha.len().min(10)], msg, author)
            }).collect();
            let mut text = lines.join("\n");
            if arr.len() > 5 { text.push_str(&format!("\n…and {} more", arr.len() - 5)); }
            blocks.push(json!({
                "type": "section",
                "text": { "type": "mrkdwn", "text": format!("*Unexplained commits:*\n{text}") }
            }));
        }
    }

    blocks.push(json!({
        "type": "context",
        "elements": [{ "type": "mrkdwn", "text": "axon-ledger · run `axon-ledger pre-deploy` locally to investigate" }]
    }));

    json!({ "blocks": blocks })
}

fn format_pagerduty(raw: &Value) -> Value {
    let unexplained = raw["unexplained"].as_u64().unwrap_or(0);
    let range = raw["range"].as_str().unwrap_or("unknown range");
    let coverage = raw["coverage_pct"].as_u64().unwrap_or(0);

    let severity = if unexplained == 0 { "info" } else if coverage < 50 { "critical" } else { "warning" };
    let summary = if unexplained == 0 {
        format!("axon-ledger: all commits explained for {range}")
    } else {
        format!("axon-ledger: {unexplained} unexplained commit(s) before deploy ({range}, {coverage}% coverage)")
    };

    json!({
        "routing_key": "",   // set by the URL — PD Events API v2 embeds key in URL
        "event_action": if unexplained == 0 { "resolve" } else { "trigger" },
        "payload": {
            "summary": summary,
            "severity": severity,
            "source": "axon-ledger",
            "custom_details": raw,
        },
        "dedup_key": format!("axon-ledger-predeploy-{range}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_webhook_event_roundtrip() {
        assert_eq!(WebhookEvent::from_str("unexplained-deploy").unwrap(), WebhookEvent::UnexplainedDeploy);
        assert!(WebhookEvent::from_str("unknown").is_err());
    }

    #[test]
    fn test_webhook_provider_roundtrip() {
        assert_eq!(WebhookProvider::from_str("slack").unwrap(), WebhookProvider::Slack);
        assert_eq!(WebhookProvider::from_str("pagerduty").unwrap(), WebhookProvider::PagerDuty);
        assert_eq!(WebhookProvider::from_str("pd").unwrap(), WebhookProvider::PagerDuty);
        assert_eq!(WebhookProvider::from_str("generic").unwrap(), WebhookProvider::Generic);
        assert!(WebhookProvider::from_str("unknown").is_err());
    }

    #[test]
    fn test_add_and_load_webhooks() {
        let dir = tempdir().unwrap();
        let id = add_webhook(dir.path(), WebhookEvent::UnexplainedDeploy, WebhookProvider::Slack, "https://hooks.slack.com/test").unwrap();
        assert!(id.starts_with("wh_"));
        let hooks = load_webhooks(dir.path()).unwrap();
        assert_eq!(hooks.len(), 1);
        assert_eq!(hooks[0].url, "https://hooks.slack.com/test");
        assert_eq!(hooks[0].event, WebhookEvent::UnexplainedDeploy);
    }

    #[test]
    fn test_remove_webhook() {
        let dir = tempdir().unwrap();
        let id = add_webhook(dir.path(), WebhookEvent::UnexplainedDeploy, WebhookProvider::Generic, "https://example.com").unwrap();
        assert!(remove_webhook(dir.path(), &id).unwrap());
        assert!(!remove_webhook(dir.path(), &id).unwrap()); // already gone
        assert!(load_webhooks(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn test_load_empty_ledger_dir() {
        let dir = tempdir().unwrap();
        let hooks = load_webhooks(dir.path()).unwrap();
        assert!(hooks.is_empty());
    }

    #[test]
    fn test_slack_payload_has_blocks() {
        let payload = json!({
            "range": "HEAD~5..HEAD",
            "total_commits": 5,
            "unexplained": 2,
            "coverage_pct": 60,
            "unexplained_commits": [
                { "sha": "abc1234567", "message": "fix auth bug", "author": "alice@example.com" }
            ]
        });
        let slack = format_slack(&payload);
        assert!(slack["blocks"].is_array());
        let blocks = slack["blocks"].as_array().unwrap();
        assert!(blocks.len() >= 2);
        // Header should mention unexplained count
        let header_text = blocks[0]["text"]["text"].as_str().unwrap_or("");
        assert!(header_text.contains("UNEXPLAINED") || header_text.contains("explained"));
    }

    #[test]
    fn test_pagerduty_payload_severity() {
        let low_coverage = json!({ "range": "HEAD~5..HEAD", "unexplained": 3, "coverage_pct": 40, "total_commits": 5 });
        let pd = format_pagerduty(&low_coverage);
        assert_eq!(pd["payload"]["severity"], "critical");
        assert_eq!(pd["event_action"], "trigger");

        let all_explained = json!({ "range": "HEAD~3..HEAD", "unexplained": 0, "coverage_pct": 100, "total_commits": 3 });
        let pd2 = format_pagerduty(&all_explained);
        assert_eq!(pd2["event_action"], "resolve");
    }
}
