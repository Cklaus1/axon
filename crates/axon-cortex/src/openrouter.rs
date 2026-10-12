//! The one way `cortex repair`'s advisory steps reach a model: the ranking
//! step ([`crate::rank`]) and the post-fix audit ([`crate::audit`]).
//!
//! Both make at most ONE JSON POST to OpenRouter, both read the key from
//! [`KEY_ENV`] only, and both turn every failure — no key, no HTTP client in
//! this build, a transport error, a timeout, a non-200, a body that is not
//! JSON, an `error` body — into a reason a caller prints, with the key
//! redacted. Neither may change a verdict, an exit code or a written file, so
//! nothing here returns anything a caller could act on except an answer to
//! parse or a reason to print.

use serde_json::Value;
use std::time::{Duration, Instant};

pub const CHAT_URL: &str = "https://openrouter.ai/api/v1/chat/completions";
pub const DECISIONS_URL: &str = "https://openrouter.ai/api/alpha/decisions";
/// The only place the key is read from. It is never logged, written or echoed.
pub const KEY_ENV: &str = "OPENROUTER_API_KEY";
/// Wall-clock bound on any one call.
pub const TIMEOUT: Duration = Duration::from_secs(20);
/// Error bodies and replies quoted in a reason are cut to this many characters.
const MAX_QUOTED: usize = 200;

/// One JSON POST. `Err` is transport only; any HTTP status is `Ok`.
pub trait Transport {
    fn post_json(
        &self,
        url: &str,
        key: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, String), String>;
}

/// The workspace's model HTTP client (`axon-ai`, reqwest + rustls).
#[cfg(feature = "ai")]
pub struct HttpTransport;

#[cfg(feature = "ai")]
impl Transport for HttpTransport {
    fn post_json(
        &self,
        url: &str,
        key: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<(u16, String), String> {
        axon_ai::post_json_bearer(url, key, body, timeout)
    }
}

/// The transport this build has, if any.
pub fn default_transport() -> Option<&'static dyn Transport> {
    #[cfg(feature = "ai")]
    {
        Some(&HttpTransport)
    }
    #[cfg(not(feature = "ai"))]
    {
        None
    }
}

/// Where a step's call goes and with what. Deliberately not `Debug`: it holds
/// the key.
pub struct Client<'t> {
    pub key: Option<String>,
    pub transport: Option<&'t dyn Transport>,
    pub chat_url: String,
    pub decisions_url: String,
    pub timeout: Duration,
}

impl Client<'static> {
    /// The production configuration: key from [`KEY_ENV`], this build's
    /// transport, OpenRouter's endpoints, [`TIMEOUT`].
    pub fn from_env() -> Self {
        Client {
            key: std::env::var(KEY_ENV).ok().filter(|k| !k.is_empty()),
            transport: default_transport(),
            chat_url: CHAT_URL.into(),
            decisions_url: DECISIONS_URL.into(),
            timeout: TIMEOUT,
        }
    }
}

/// A 200 whose body is a JSON object without an `error`.
pub struct Reply {
    pub body: Value,
    pub latency_ms: u64,
    /// `usage.cost`, when OpenRouter reported one.
    pub cost_usd: Option<f64>,
}

/// Why no usable reply came back, with whatever was measured on the way.
pub struct Failure {
    /// Already redacted.
    pub reason: String,
    pub latency_ms: Option<u64>,
    pub cost_usd: Option<f64>,
}

impl Client<'_> {
    /// `Err(reason)` when no call can be made at all: no key, or no HTTP
    /// client in this build. Checked before a request is built, so a step
    /// that cannot call says so without rendering anything.
    pub fn ready(&self) -> Result<(), String> {
        if self.key.is_none() {
            return Err(format!("{KEY_ENV} not set"));
        }
        if self.transport.is_none() {
            return Err("this build has no HTTP client: rebuild with `--features ai`".into());
        }
        Ok(())
    }

    /// POST `body` to `url` once.
    pub fn post(&self, url: &str, body: &Value) -> Result<Reply, Failure> {
        let fail = |reason: String, latency_ms: Option<u64>, cost_usd: Option<f64>| Failure {
            reason: self.redact(&reason),
            latency_ms,
            cost_usd,
        };
        let (key, transport) = match (self.key.as_deref(), self.transport) {
            (Some(k), Some(t)) => (k, t),
            _ => {
                let reason = self.ready().err().unwrap_or_default();
                return Err(fail(reason, None, None));
            }
        };
        let began = Instant::now();
        let sent = transport.post_json(url, key, body, self.timeout);
        let latency_ms = began.elapsed().as_millis() as u64;
        let latency = Some(latency_ms);
        let (status, text) = sent.map_err(|e| fail(e, latency, None))?;
        if status != 200 {
            return Err(fail(
                format!("HTTP {status}: {}", quote(&text)),
                latency,
                None,
            ));
        }
        let body: Value = crate::parse_strict(&text)
            .map_err(|e| fail(format!("response is not JSON: {e}"), latency, None))?;
        let cost = body["usage"]["cost"].as_f64();
        if let Some(err) = body.get("error").filter(|e| !e.is_null()) {
            return Err(fail(
                format!("error body: {}", quote(&err.to_string())),
                latency,
                cost,
            ));
        }
        Ok(Reply {
            body,
            latency_ms,
            cost_usd: cost,
        })
    }

    /// `s` with the key replaced. Every reason a step reports passes through
    /// here, including ones quoting a model's reply.
    pub fn redact(&self, s: &str) -> String {
        match self.key.as_deref() {
            Some(k) if !k.is_empty() => s.replace(k, "<redacted>"),
            _ => s.to_string(),
        }
    }
}

/// `openrouter:` / `decision:` model ids: non-empty, no whitespace. `what`
/// names the flag family in the error.
pub(crate) fn model_id(what: &str, spec: &str, m: &str) -> Result<String, String> {
    if m.is_empty() || m.chars().any(char::is_whitespace) {
        Err(format!(
            "{what} `{spec}`: the model id must be non-empty with no spaces"
        ))
    } else {
        Ok(m.to_string())
    }
}

/// First [`MAX_QUOTED`] characters of `s`, on one line.
pub(crate) fn quote(s: &str) -> String {
    let one_line: String = s
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .take(MAX_QUOTED)
        .collect();
    if s.chars().count() > MAX_QUOTED {
        format!("{one_line}…")
    } else {
        one_line
    }
}
