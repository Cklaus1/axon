//! Provenance / audit logging for the interpreter (R0 slice 1 — extracted
//! verbatim from `interp.rs`, zero behavior change). Writes + reads the
//! `provenance.jsonl` NDJSON log shared with axon-rt: `@[adaptive]` score
//! rows, `@[agent]` action audit, R3 `ai_call` replay records, and the
//! goal-resume readers. `super::now_ms` (a file-wide builtin helper) stays
//! in the parent; everything that touches the log lives here.

// `use super::*` brings in the parent's shared imports — including
// `std::io::Write as _` (for `write_all`) and `now_ms` — so this module needs
// no `use` of its own.
use super::*;

/// The provenance log path: `$XDG_CACHE_HOME/axon/provenance.jsonl` (or
/// `$HOME/.cache/axon/...`), matching axon-rt's location.
pub(super) fn provenance_log_path() -> Option<std::path::PathBuf> {
    let base = std::env::var("XDG_CACHE_HOME")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var("HOME").ok().map(|h| std::path::PathBuf::from(h).join(".cache")))?;
    Some(base.join("axon").join("provenance.jsonl"))
}

/// Append one `@[adaptive]` return to the provenance JSONL, in the same shape
/// Source identity of the currently-running program, stamped into every
/// provenance record so `axon trace` can tell one program's `metric` apart
/// from another's (BUG_HUNT #4 / I-9). Set once at the CLI boundary via
/// [`set_provenance_source`]; defaults to "" (unknown) when unset.
static PROVENANCE_SOURCE: std::sync::OnceLock<String> = std::sync::OnceLock::new();

/// Record the source program's identity (e.g. its file path) for provenance.
/// Idempotent — only the first call takes effect, matching one program per
/// process.
pub fn set_provenance_source(src: impl Into<String>) {
    let _ = PROVENANCE_SOURCE.set(src.into());
}

pub(super) fn provenance_source() -> &'static str {
    PROVENANCE_SOURCE.get().map(String::as_str).unwrap_or("")
}

/// axon-rt writes (`ts_ms`/`fn`/`event`/`payload`/`score`), so `axon trace`
/// and the observability tools see interpreter runs. We add a `src` field
/// (the program path) so records from different programs that happen to share
/// a function name don't blend in `trace`. Best-effort (errors ignored —
/// provenance is advisory, not load-bearing).
pub(super) fn append_provenance_jsonl(
    fn_name: &str,
    payload: &str,
    score: f64,
    input: Option<i64>,
    zone: &str,
    label: Option<&str>,
) {
    let Some(path) = provenance_log_path() else { return };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let ts = now_ms().max(0) as u64;
    let s = if score.is_finite() { format!("{score}") } else { "0".to_string() };
    // `input` (the goal-search arg) is an additive field; axon-rt readers ignore it.
    let inp = match input {
        Some(x) => format!(",\"input\":{x}"),
        None => String::new(),
    };
    let src = provenance_source();
    let src_field = if src.is_empty() {
        String::new()
    } else {
        format!(",\"src\":{}", json_quote(src))
    };
    // R4: the record names its zone and the event type derived from it
    // (`adaptive_return` / `experiment_return`), replacing the old
    // placeholder `"event":"event"`. `label` is the `@[experiment(label)]`
    // tag, omitted for non-experiment zones. Readers ignore unknown fields,
    // so this is backward-compatible with axon-rt's format.
    let label_field = match label {
        Some(l) => format!(",\"label\":{}", json_quote(l)),
        None => String::new(),
    };
    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{f},\"event\":{ev},\"zone\":{z},\"payload\":{p},\"score\":{s}{inp}{label_field}{src_field}}}\n",
        f = json_quote(fn_name),
        ev = json_quote(&format!("{zone}_return")),
        z = json_quote(zone),
        p = json_quote(payload),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// R4 §4.3 — append one `event:"agent_action"` audit record for a capability-
/// bearing action taken inside an `@[agent]` function. `action` is the tool
/// (builtin) name; `caps_used` is the capability kind it exercises (the I-11
/// link). Compiler-injected at the call site, so an agent's actions are logged
/// whether or not the agent "cooperates" (I-13, the highest-trust zone).
pub(super) fn append_agent_action_jsonl(fn_name: &str, action: &str, caps_used: &str) {
    let Some(path) = provenance_log_path() else { return };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let ts = now_ms().max(0) as u64;
    let src = provenance_source();
    let src_field = if src.is_empty() {
        String::new()
    } else {
        format!(",\"src\":{}", json_quote(src))
    };
    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{f},\"event\":\"agent_action\",\"zone\":\"agent\",\
         \"action\":{a},\"caps_used\":{c}{src_field}}}\n",
        f = json_quote(fn_name),
        a = json_quote(action),
        c = json_quote(caps_used),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Hex SHA-256 of `s` — the `prompt_hash`/`params_hash` scheme for R3's
/// `ai_call` provenance. Hashing (not the raw text) is what lands in the log,
/// so a replay can key on the exact prompt without the log leaking it verbatim.
pub(super) fn sha256_hex(s: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(s.as_bytes());
    format!("{:x}", h.finalize())
}

/// One AI-call provenance record (R3 §4.3). Every `ai_complete` execution —
/// live, mock, or fallback — appends exactly one `event:"ai_call"` NDJSON line
/// to the same provenance log as the score rows, attributed to its caller and
/// carrying the replay key (model + hashes) without the prompt verbatim.
#[allow(clippy::too_many_arguments)]
pub(super) fn append_ai_call_jsonl(
    fn_name: &str,
    prompt: &str,
    tier: &str,
    model: &str,
    model_version: &str,
    params: &str,
    mode: &str,
    reason: &str,
    cost_usd: f64,
) {
    let Some(path) = provenance_log_path() else { return };
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let ts = now_ms().max(0) as u64;
    let src = provenance_source();
    let src_field = if src.is_empty() {
        String::new()
    } else {
        format!(",\"src\":{}", json_quote(src))
    };
    let cost = if cost_usd.is_finite() { format!("{cost_usd}") } else { "0".to_string() };
    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{f},\"event\":\"ai_call\",\"tier\":{t},\"model\":{m},\
         \"model_version\":{mv},\"params_hash\":{ph},\"prompt_hash\":{prh},\"mode\":{md},\
         \"reason\":{rs},\"cost_usd\":{cost}{src_field}}}\n",
        f = json_quote(fn_name),
        t = json_quote(tier),
        m = json_quote(model),
        mv = json_quote(model_version),
        ph = json_quote(&sha256_hex(params)),
        prh = json_quote(&sha256_hex(prompt)),
        md = json_quote(mode),
        rs = json_quote(reason),
    );
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}

/// Minimal JSON string quoting (for the provenance log).
pub(super) fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Cross-run continuation is opt-in via `AXON_GOAL_CONTINUE` (so default runs —
/// and tests — stay deterministic, starting each hill-climb from 0).
pub(super) fn goal_continue_enabled() -> bool {
    std::env::var("AXON_GOAL_CONTINUE").map(|v| !v.is_empty() && v != "0").unwrap_or(false)
}

/// Read the persisted provenance JSONL and return the recorded `input` whose
/// `score` is closest to `target` for `fn_name` — i.e. the best prior search
/// position to resume a hill-climb from. `None` if no usable record exists.
pub(super) fn read_best_input(fn_name: &str, target: f64) -> Option<i64> {
    let path = provenance_log_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let needle = format!("\"fn\":{}", json_quote(fn_name));
    let mut best_input: Option<i64> = None;
    let mut best_dist = f64::INFINITY;
    for line in content.lines() {
        if !line.contains(&needle) {
            continue;
        }
        let (Some(input), Some(score)) =
            (extract_json_num(line, "\"input\":"), extract_json_num(line, "\"score\":"))
        else {
            continue;
        };
        let dist = (score - target).abs();
        if dist < best_dist {
            best_dist = dist;
            best_input = Some(input as i64);
        }
    }
    best_input
}

/// Read the provenance JSONL and return the maximum `score` among entries
/// written at or after `since_ts_ms` (epoch ms). `axon goal --iterate` records
/// a start timestamp and calls this after each run to detect convergence — when
/// the best score stops improving — so it can stop early. Scoping by timestamp
/// ignores unrelated prior entries that share the (accumulating) log file.
/// `None` if the log is absent or has no qualifying scored entry.
pub fn best_recorded_score(since_ts_ms: u64) -> Option<f64> {
    let path = provenance_log_path()?;
    let content = std::fs::read_to_string(&path).ok()?;
    let mut best: Option<f64> = None;
    for line in content.lines() {
        let ts = extract_json_num(line, "\"ts_ms\":").unwrap_or(0.0);
        if (ts as u64) < since_ts_ms {
            continue;
        }
        if let Some(s) = extract_json_num(line, "\"score\":") {
            best = Some(best.map_or(s, |b| b.max(s)));
        }
    }
    best
}

/// One parsed provenance record — the fields `axon trace` reports on.
pub struct ProvRecord {
    pub ts_ms: u64,
    pub func: String,
    pub score: f64,
    pub input: Option<i64>,
    /// Source program identity (file path). Empty for records written before
    /// the field existed, or by tools that don't set it. `trace` groups by
    /// `(func, src)` so two programs' same-named metrics don't blend.
    pub src: String,
}

/// Read and parse the provenance JSONL (best-effort; malformed lines skipped).
/// `path` defaults to the standard log location. Returns `None` only if the log
/// file is absent/unreadable. Used by `axon trace`.
pub fn read_provenance(path: Option<&std::path::Path>) -> Option<Vec<ProvRecord>> {
    let owned;
    let path = match path {
        Some(p) => p,
        None => {
            owned = provenance_log_path()?;
            &owned
        }
    };
    let content = std::fs::read_to_string(path).ok()?;
    let mut out = Vec::new();
    for line in content.lines() {
        let (Some(func), Some(score)) =
            (extract_json_str(line, "\"fn\":"), extract_json_num(line, "\"score\":"))
        else {
            continue;
        };
        out.push(ProvRecord {
            ts_ms: extract_json_num(line, "\"ts_ms\":").unwrap_or(0.0) as u64,
            func,
            score,
            input: extract_json_num(line, "\"input\":").map(|x| x as i64),
            src: extract_json_str(line, "\"src\":").unwrap_or_default(),
        });
    }
    Some(out)
}

/// One `ai_call` audit record from the provenance log — the AI-call trail behind
/// `axon trace --ai`. Captures who called which routed model, in what mode
/// (live/mock/replay/fallback), and the metered cost.
#[derive(Debug, Clone)]
pub struct AiCallRecord {
    pub func: String,
    pub tier: String,
    pub model: String,
    /// "live" | "mock" | "replay" | "fallback".
    pub mode: String,
    pub cost_usd: f64,
    pub src: String,
}

/// Read the `ai_call` events from the provenance log (the AI-call audit trail).
/// `None` when the log is absent/unreadable; non-`ai_call` rows (adaptive scores,
/// agent actions) are skipped.
pub fn read_ai_calls(path: Option<&std::path::Path>) -> Option<Vec<AiCallRecord>> {
    let owned;
    let path = match path {
        Some(p) => p,
        None => {
            owned = provenance_log_path()?;
            &owned
        }
    };
    let content = std::fs::read_to_string(path).ok()?;
    let mut out = Vec::new();
    for line in content.lines() {
        if extract_json_str(line, "\"event\":").as_deref() != Some("ai_call") {
            continue;
        }
        let Some(func) = extract_json_str(line, "\"fn\":") else { continue };
        out.push(AiCallRecord {
            func,
            tier: extract_json_str(line, "\"tier\":").unwrap_or_default(),
            model: extract_json_str(line, "\"model\":").unwrap_or_default(),
            mode: extract_json_str(line, "\"mode\":").unwrap_or_default(),
            cost_usd: extract_json_num(line, "\"cost_usd\":").unwrap_or(0.0),
            src: extract_json_str(line, "\"src\":").unwrap_or_default(),
        });
    }
    Some(out)
}

/// Extract the string value following `key` (e.g. `"fn":`) in a JSON line.
/// Tolerant of our own fixed log format; assumes no escaped quotes in the value
/// (true for fn names). `None` if absent or unterminated.
pub(super) fn extract_json_str(line: &str, key: &str) -> Option<String> {
    let start = line.find(key)? + key.len();
    let rest = line[start..].trim_start().strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Extract the numeric value following `key` in a JSON line (up to the next
/// `,` or `}`). Tolerant of our own fixed log format; not a general parser.
pub(super) fn extract_json_num(line: &str, key: &str) -> Option<f64> {
    let start = line.find(key)? + key.len();
    let rest = &line[start..];
    let end = rest.find([',', '}']).unwrap_or(rest.len());
    rest[..end].trim().parse::<f64>().ok()
}

// ── F2 (ROADMAP §9.5): deterministic LLM-call replay ─────────────────────────
//
// With AXON_AI_REPLAY=<file>, every `ai_complete(prompt)` is MEMOIZED by
// (prompt, model): a first run RECORDS (response, token-count) to the file; a
// re-run with the same file REPLAYS the recorded response verbatim — no live
// call, no mock, no API key — so an AI-driven run is exactly reproducible (the
// auditability backbone the demo's "auditable" claim depends on, R3 §4.3). The
// key is sha256(model\0prompt); the line is `<key> <tokens> <response-hex>`, so a
// response containing newlines/quotes needs no escaping.

/// The replay-cache file path from `AXON_AI_REPLAY` (None when unset/empty).
pub(super) fn ai_replay_path() -> Option<std::path::PathBuf> {
    std::env::var("AXON_AI_REPLAY")
        .ok()
        .filter(|s| !s.is_empty())
        .map(std::path::PathBuf::from)
}

fn ai_replay_key(prompt: &str, model: &str) -> String {
    sha256_hex(&format!("{model}\u{0}{prompt}"))
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < bytes.len() {
        let hi = (bytes[i] as char).to_digit(16)?;
        let lo = (bytes[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

/// Look up a memoized `(prompt, model)` response. Returns `(response, tokens)` on
/// a hit, or `None` (no replay file / miss / unreadable). Malformed lines are
/// skipped, never panic.
pub(super) fn ai_replay_lookup(prompt: &str, model: &str) -> Option<(String, i64)> {
    let path = ai_replay_path()?;
    let key = ai_replay_key(prompt, model);
    let contents = std::fs::read_to_string(&path).ok()?;
    for line in contents.lines() {
        let mut it = line.split_whitespace();
        let (Some(k), Some(tok), Some(resp_hex)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        if k != key {
            continue;
        }
        if let (Ok(tokens), Some(resp)) =
            (tok.parse::<i64>(), hex_decode(resp_hex).and_then(|b| String::from_utf8(b).ok()))
        {
            return Some((resp, tokens));
        }
    }
    None
}

/// Record a `(prompt, model) → (response, tokens)` entry for later replay. No-op
/// when `AXON_AI_REPLAY` is unset. The dispatch only calls this on a cache MISS,
/// so no duplicate keys accumulate.
pub(super) fn ai_replay_store(prompt: &str, model: &str, response: &str, tokens: i64) {
    let Some(path) = ai_replay_path() else { return };
    if let Some(dir) = path.parent() {
        if !dir.as_os_str().is_empty() && std::fs::create_dir_all(dir).is_err() {
            return;
        }
    }
    let line = format!("{} {tokens} {}\n", ai_replay_key(prompt, model), hex_encode(response.as_bytes()));
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}
