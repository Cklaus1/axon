//! Axon AI runtime -- live Anthropic inference via __axon_ai_complete.
//!
//! Compiled as a static library and linked into every Axon binary that uses
//! the ai_complete builtin.  All exported symbols use C linkage so the
//! LLVM-emitted code can call them directly by name.
//!
//! ABI mirrors __axon_read_file from axon-rt:
//! - success: *out_len >= 0, *out_ptr points to heap-allocated UTF-8
//! - error:   *out_len < 0 (negated message length), *out_ptr points to
//!   the error message
//!
//! Layer-3 ASI typed-extraction surface (`__axon_ai_extract_uncertain_i64` /
//! `__axon_ai_extract_uncertain_f64`) uses a different ABI: a return-code
//! plus typed value/confidence out-params; the err string out-param is only
//! populated when the return code is 1.  See per-symbol docs.
//!
//! ## Lint posture (BUG_HUNT #35)
//!
//! Like `axon-rt`, every `#[no_mangle] pub extern "C"` symbol is a
//! codegen-emitted-IR entry point whose pointer args come from codegen's own
//! str/out-param ABI — the unsafe contract is at that boundary, not a Rust
//! caller. We `allow(not_unsafe_ptr_arg_deref)` crate-wide with that rationale
//! so the gate's `--all-targets` pass enforces every other lint here.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use std::alloc::Layout;

// -- Provider selection --------------------------------------------------------
//
// Axon speaks two LLM wire formats:
//
//   * Anthropic Messages API  (`/v1/messages`, `x-api-key`, `content[0].text`)
//     — the default. An Anthropic-COMPATIBLE gateway/proxy (Helicone passthrough,
//     a trainloop proxy, LiteLLM in /v1/messages mode, Cloudflare AI Gateway in
//     Anthropic mode) works with ZERO code change: just set ANTHROPIC_BASE_URL.
//
//   * OpenAI Chat Completions  (`/v1/chat/completions`, `Authorization: Bearer`,
//     `choices[0].message.content`) — opt in with AXON_AI_PROVIDER=openai. This
//     unlocks NVIDIA NIM, OpenRouter, vLLM, LiteLLM (OpenAI mode), Together,
//     Groq, and most OpenAI-compatible gateways from one codec.
//
// Env knobs (provider-neutral names take precedence over the legacy
// ANTHROPIC_* ones, so a single config drives either provider):
//   AXON_AI_PROVIDER   "anthropic" (default) | "openai"
//   AXON_AI_BASE_URL   base URL override (falls back to ANTHROPIC_BASE_URL,
//                      then the provider default)
//   AXON_AI_API_KEY    API key (falls back to ANTHROPIC_API_KEY)
// Model strings are already env-overridable per tier (AXON_AI_MODEL_{CHEAP,
// BALANCED,STRONG}) — set those to the backend's model ids (e.g.
// "meta/llama-3.1-70b-instruct" for NIM) when using a non-Anthropic provider.

// -- .env loading --------------------------------------------------------------
//
// Load API keys / provider config from a `.env` file so users don't have to
// export secrets into their shell. Searched once (lazily, before the first key
// lookup): `$AXON_DOTENV` if set, else `.env` in the current dir, then walking
// up parent dirs to the filesystem root (so it works from a subdirectory of a
// project). Lines are `KEY=VALUE`; `#` comments and blank lines are skipped;
// surrounding quotes on the value are stripped. An EXISTING process env var is
// never overwritten — the real environment always wins over the file.

use std::sync::Once;
static DOTENV_ONCE: Once = Once::new();

fn load_dotenv_once() {
    DOTENV_ONCE.call_once(|| {
        let path = match std::env::var("AXON_DOTENV").ok().filter(|s| !s.is_empty()) {
            Some(p) => Some(std::path::PathBuf::from(p)),
            None => find_dotenv_upwards(),
        };
        if let Some(p) = path {
            if let Ok(contents) = std::fs::read_to_string(&p) {
                apply_dotenv(&contents);
            }
        }
    });
}

/// Walk from the current directory up to the root looking for a `.env` file.
fn find_dotenv_upwards() -> Option<std::path::PathBuf> {
    let mut dir = std::env::current_dir().ok()?;
    loop {
        let candidate = dir.join(".env");
        if candidate.is_file() {
            return Some(candidate);
        }
        if !dir.pop() {
            return None;
        }
    }
}

/// Parse `KEY=VALUE` lines and set them in the process env, without overwriting
/// any var that is already set. Pulled out for unit testing.
fn apply_dotenv(contents: &str) {
    for (k, v) in parse_dotenv(contents) {
        if std::env::var_os(&k).is_none() {
            std::env::set_var(&k, &v);
        }
    }
}

/// Pure parser: `KEY=VALUE` pairs, skipping `#` comments / blanks, trimming an
/// optional `export ` prefix and surrounding single/double quotes on the value.
fn parse_dotenv(contents: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for raw in contents.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let key = k.trim();
        if key.is_empty() {
            continue;
        }
        let mut val = v.trim();
        // Strip a matching pair of surrounding quotes.
        if (val.starts_with('"') && val.ends_with('"') && val.len() >= 2)
            || (val.starts_with('\'') && val.ends_with('\'') && val.len() >= 2)
        {
            val = &val[1..val.len() - 1];
        }
        out.push((key.to_string(), val.to_string()));
    }
    out
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Provider {
    Anthropic,
    OpenAi,
}

/// Resolve the active provider from `AXON_AI_PROVIDER` (default Anthropic).
/// Unknown values fall back to Anthropic (the safe default) rather than erroring.
pub fn provider() -> Provider {
    load_dotenv_once();
    match std::env::var("AXON_AI_PROVIDER").ok().as_deref() {
        Some("openai") | Some("OpenAI") | Some("openai-compatible") => Provider::OpenAi,
        _ => Provider::Anthropic,
    }
}

/// The base URL for the active provider. Neutral `AXON_AI_BASE_URL` wins; then
/// the legacy `ANTHROPIC_BASE_URL` (back-compat); then the provider default.
fn base_url(p: Provider) -> String {
    load_dotenv_once();
    let neutral = std::env::var("AXON_AI_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let legacy = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty());
    let default = match p {
        Provider::Anthropic => "https://api.anthropic.com",
        Provider::OpenAi => "https://api.openai.com",
    };
    let base = neutral.or(legacy).unwrap_or_else(|| default.to_string());
    base.trim_end_matches('/').to_string()
}

/// The full chat/completions endpoint URL for the active provider.
fn endpoint_url(p: Provider) -> String {
    match p {
        Provider::Anthropic => format!("{}/v1/messages", base_url(p)),
        Provider::OpenAi => format!("{}/v1/chat/completions", base_url(p)),
    }
}

/// The API key for the active provider. Neutral `AXON_AI_API_KEY` wins, then the
/// legacy `ANTHROPIC_API_KEY`. Returns a provider-named error when neither is set.
fn api_key(p: Provider) -> Result<String, String> {
    load_dotenv_once();
    std::env::var("AXON_AI_API_KEY")
        .ok()
        .filter(|s| !s.is_empty())
        .or_else(|| {
            std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .ok_or_else(|| match p {
            Provider::Anthropic => "ANTHROPIC_API_KEY (or AXON_AI_API_KEY) is not set".to_string(),
            Provider::OpenAi => "AXON_AI_API_KEY (or ANTHROPIC_API_KEY) is not set".to_string(),
        })
}

/// Apply the provider's auth + content headers to a request builder.
fn auth_headers(
    p: Provider,
    req: reqwest::blocking::RequestBuilder,
    key: &str,
) -> reqwest::blocking::RequestBuilder {
    match p {
        Provider::Anthropic => req
            .header("x-api-key", key)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json"),
        Provider::OpenAi => req
            .header("authorization", format!("Bearer {key}"))
            .header("content-type", "application/json"),
    }
}

// -- malloc helper (mirrors axon-rt's libc_malloc) ----------------------------

unsafe fn libc_malloc(size: usize) -> *mut u8 {
    let layout = Layout::from_size_align(size.max(1), 1).unwrap();
    std::alloc::alloc(layout)
}

/// Write s into a heap buffer, NUL-terminate it, set *out_len and *out_ptr.
unsafe fn write_str_out(s: &str, out_len: *mut i64, out_ptr: *mut *mut u8) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let buf = libc_malloc(len + 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
    *buf.add(len) = 0;
    *out_len = len as i64;
    *out_ptr = buf;
}

/// Write s as an error (negated length).
unsafe fn write_err_out(s: &str, out_len: *mut i64, out_ptr: *mut *mut u8) {
    let bytes = s.as_bytes();
    let len = bytes.len();
    let buf = libc_malloc(len + 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
    *buf.add(len) = 0;
    *out_len = -(len as i64);
    *out_ptr = buf;
}

// -- Main exported symbol ------------------------------------------------------

/// Call the Anthropic Messages API with prompt as a single user message.
///
/// Out-param ABI (mirrors __axon_read_file):
/// - success: *out_len >= 0, *out_ptr = heap buffer with the assistant reply
/// - error:   *out_len < 0 (negated message length), *out_ptr = error string
///
/// Reads ANTHROPIC_API_KEY from the environment.  If unset, returns an error.
/// Uses model claude-sonnet-4-6, max_tokens 1024.
#[no_mangle]
pub extern "C" fn __axon_ai_complete(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_len: *mut i64,
    out_ptr: *mut *mut u8,
) {
    // Safety: the Axon codegen always passes valid str{ptr,len} pairs.
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };

    match ai_complete_inner(prompt) {
        Ok(reply) => unsafe { write_str_out(&reply, out_len, out_ptr) },
        Err(e) => unsafe { write_err_out(&e, out_len, out_ptr) },
    }
}

/// Whether deterministic mock-LLM responses are enabled (`AXON_AI_MOCK` set and
/// not "0"/empty) — the SAME predicate the interpreter uses
/// (`interp::ai_mock_enabled`). When on, the native AI path returns the
/// interpreter's exact deterministic stub instead of hitting the network, so a
/// program's native and interpreted output match byte-for-byte under mock
/// (closes the R1 parity gap for the AI examples; the native path previously
/// ignored AXON_AI_MOCK and errored on a missing API key).
fn ai_mock_enabled() -> bool {
    std::env::var("AXON_AI_MOCK")
        .map(|v| !v.is_empty() && v != "0")
        .unwrap_or(false)
}

/// The interpreter's canonical mock `ai_complete` response — must stay
/// byte-identical to `interp.rs`'s stub for native==interp parity under mock.
const MOCK_AI_COMPLETE: &str = "Mock summary: the single most important fact, stated concisely.";

/// The default model when a caller doesn't pin one (back-compat with the
/// original hardcoded value). Tier-aware callers pass the resolved tier's model.
const DEFAULT_MODEL: &str = "claude-sonnet-4-6";

fn ai_complete_inner(prompt: &str) -> Result<String, String> {
    ai_complete_inner_model(prompt, DEFAULT_MODEL)
}

/// `ai_complete` with an explicit model string (R3 tier routing): the live
/// request sends `model`, so selecting the `strong` tier actually routes to the
/// strong model instead of the hardcoded sonnet. The interpreter passes
/// `Tier::api_model()` (itself env-overridable for a proxy/gateway deployment).
fn ai_complete_inner_model(prompt: &str, model: &str) -> Result<String, String> {
    ai_complete_inner_model_usage(prompt, model).map(|(text, _tokens)| text)
}

/// `ai_complete` returning BOTH the reply and the REAL total token count
/// (input + output) from the model's `usage` field — for accurate per-token
/// cost metering (R3 / F4). Under AXON_AI_MOCK the count is a deterministic
/// estimate from the prompt (~4 chars/token) so mocked cost stays reproducible
/// and identical to the interpreter's pre-dispatch estimate; only a LIVE call
/// reports the model's actual usage.
fn ai_complete_inner_model_usage(prompt: &str, model: &str) -> Result<(String, i64), String> {
    // Deterministic mock: the interpreter's stub + the same token estimate the
    // pre-dispatch meter uses, so mock metering is exact and reproducible.
    if ai_mock_enabled() {
        let est = (prompt.len() as i64) / 4 + 1;
        return Ok((MOCK_AI_COMPLETE.to_string(), est));
    }
    let p = provider();
    let key = api_key(p)?;

    // Build the provider-specific request body. Both carry the same logical
    // request (model + single user message + token cap); only the schema differs.
    let body = match p {
        Provider::Anthropic => serde_json::json!({
            "model": model,
            "max_tokens": 1024,
            "messages": [ { "role": "user", "content": prompt } ]
        }),
        Provider::OpenAi => serde_json::json!({
            "model": model,
            "max_tokens": 1024,
            "messages": [ { "role": "user", "content": prompt } ]
        }),
    };

    let client = reqwest::blocking::Client::new();
    let response = auth_headers(p, client.post(endpoint_url(p)), &key)
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    if !status.is_success() {
        let label = match p {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
        };
        return Err(format!("{label} API error {}: {}", status.as_u16(), text));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response as JSON: {}", e))?;

    // Extract the reply text — different response shapes per provider.
    let reply = match p {
        // Anthropic: content[0].text
        Provider::Anthropic => json["content"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|item| item["text"].as_str())
            .ok_or_else(|| format!("Unexpected Anthropic response shape: {}", text))?
            .to_string(),
        // OpenAI: choices[0].message.content
        Provider::OpenAi => json["choices"]
            .as_array()
            .and_then(|arr| arr.first())
            .and_then(|item| item["message"]["content"].as_str())
            .ok_or_else(|| format!("Unexpected OpenAI response shape: {}", text))?
            .to_string(),
    };

    // Real token usage (input + output). Anthropic: usage.{input,output}_tokens;
    // OpenAI: usage.{prompt,completion}_tokens (or usage.total_tokens). Fall back
    // to a prompt/reply-length estimate if the field is missing (some proxies).
    let total = match p {
        Provider::Anthropic => match (
            json["usage"]["input_tokens"].as_i64(),
            json["usage"]["output_tokens"].as_i64(),
        ) {
            (Some(i), Some(o)) => i + o,
            _ => (prompt.len() as i64) / 4 + 1 + (reply.len() as i64) / 4,
        },
        Provider::OpenAi => json["usage"]["total_tokens"].as_i64().unwrap_or_else(|| {
            match (
                json["usage"]["prompt_tokens"].as_i64(),
                json["usage"]["completion_tokens"].as_i64(),
            ) {
                (Some(i), Some(o)) => i + o,
                _ => (prompt.len() as i64) / 4 + 1 + (reply.len() as i64) / 4,
            }
        }),
    };

    Ok((reply, total))
}

/// Rust-callable plain completion (mirrors the `__axon_ai_complete` C ABI).
///
/// Returns `Ok(reply)` or `Err(message)`. Used by the tree-walking interpreter
/// (`axon-core`'s `interp`) to back the `ai_complete` builtin without going
/// through the pointer/length C ABI.
pub fn complete(prompt: &str) -> Result<String, String> {
    ai_complete_inner(prompt)
}

/// `complete` with the model pinned by the resolved tier (R3 tier routing).
/// `model` is the wire model string (e.g. `claude-opus-4-8` for `strong`); the
/// interpreter sources it from `ai_routing::Tier::api_model()`.
pub fn complete_with_model(prompt: &str, model: &str) -> Result<String, String> {
    ai_complete_inner_model(prompt, model)
}

/// `complete_with_model` returning BOTH the reply and the REAL total token count
/// (input + output) for accurate per-token cost metering (R3 / F4). The
/// interpreter uses this on a LIVE call so the cost reflects what the model
/// actually charged, not a prompt-length estimate. Under AXON_AI_MOCK the count
/// is the same deterministic estimate the pre-dispatch meter uses (reproducible).
pub fn complete_with_model_usage(prompt: &str, model: &str) -> Result<(String, i64), String> {
    ai_complete_inner_model_usage(prompt, model)
}

// -- Layer-3 ASI: typed structured-output ("Uncertain<T>") ---------------------
//
// These helpers force Anthropic to return its answer through a tool-use call
// whose JSON schema demands `{value, confidence}`.  This makes the LLM's
// self-reported confidence cross into Axon's type system as `Uncertain<T>`,
// so user code can branch on `.confidence > threshold`.

/// Build the Anthropic Messages API request body for typed structured output.
///
/// `value_type` should be `"integer"` for i64 or `"number"` for f64; the rest
/// of the schema is identical.  Exposed as `pub` so unit tests can pin the
/// JSON shape without going over the wire.
pub fn build_typed_uncertain_body(prompt: &str, value_type: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "tools": [{
            "name": "answer",
            "description": "Return your answer with a confidence score in [0.0, 1.0].",
            "input_schema": {
                "type": "object",
                "properties": {
                    "value":      { "type": value_type },
                    "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
                },
                "required": ["value", "confidence"]
            }
        }],
        "tool_choice": { "type": "tool", "name": "answer" },
        "messages": [
            { "role": "user", "content": prompt }
        ]
    })
}

/// Extract the `answer` tool_use block from a Messages API response and
/// return its `input` object.  Returns Err if no matching tool_use is found.
fn parse_tool_use_input(json: &serde_json::Value) -> Result<&serde_json::Value, String> {
    let content = json["content"]
        .as_array()
        .ok_or_else(|| format!("Unexpected API response shape (no content array): {}", json))?;
    for item in content {
        if item["type"].as_str() == Some("tool_use") && item["name"].as_str() == Some("answer") {
            return item
                .get("input")
                .ok_or_else(|| format!("tool_use block missing `input`: {}", item));
        }
    }
    Err(format!("No `answer` tool_use block in response: {}", json))
}

/// The JSON-Schema `properties`/`required` for the structured `answer` tool,
/// shared by both providers. With confidence → `{value, confidence}`; without →
/// `{value}`. This is the single source of the schema so the two provider body
/// shapes can't drift on the field set.
fn answer_schema(
    value_type: &str,
    with_confidence: bool,
) -> (serde_json::Value, serde_json::Value) {
    if with_confidence {
        (
            serde_json::json!({
                "value":      { "type": value_type },
                "confidence": { "type": "number", "minimum": 0.0, "maximum": 1.0 }
            }),
            serde_json::json!(["value", "confidence"]),
        )
    } else {
        (
            serde_json::json!({ "value": { "type": value_type } }),
            serde_json::json!(["value"]),
        )
    }
}

/// Build the OpenAI Chat Completions structured-output request: a single
/// `answer` function tool whose parameters are the answer schema, forced via
/// `tool_choice`. OpenAI nests the schema under `function.parameters` (vs
/// Anthropic's top-level `input_schema`) and wraps each tool in `{type,function}`.
fn build_openai_tool_body(
    model: &str,
    prompt: &str,
    value_type: &str,
    with_confidence: bool,
) -> serde_json::Value {
    let (properties, required) = answer_schema(value_type, with_confidence);
    let desc = if with_confidence {
        "Return your answer with a confidence score in [0.0, 1.0]."
    } else {
        "Return your answer as the single `value` field."
    };
    serde_json::json!({
        "model": model,
        "max_tokens": 1024,
        "tools": [{
            "type": "function",
            "function": {
                "name": "answer",
                "description": desc,
                "parameters": {
                    "type": "object",
                    "properties": properties,
                    "required": required
                }
            }
        }],
        "tool_choice": { "type": "function", "function": { "name": "answer" } },
        "messages": [ { "role": "user", "content": prompt } ]
    })
}

/// Extract the `answer` tool call's arguments from an OpenAI Chat Completions
/// response. OpenAI returns `choices[0].message.tool_calls[0].function.arguments`
/// as a JSON *string*, so we parse it into an object (vs Anthropic's already-
/// structured `input`). Returns the parsed arguments object.
fn parse_openai_tool_args(json: &serde_json::Value) -> Result<serde_json::Value, String> {
    let calls = json["choices"]
        .as_array()
        .and_then(|a| a.first())
        .and_then(|c| c["message"]["tool_calls"].as_array())
        .ok_or_else(|| format!("Unexpected OpenAI response shape (no tool_calls): {}", json))?;
    for call in calls {
        if call["function"]["name"].as_str() == Some("answer") {
            let args = call["function"]["arguments"]
                .as_str()
                .ok_or_else(|| format!("tool call `arguments` is not a string: {}", call))?;
            return serde_json::from_str(args)
                .map_err(|e| format!("tool call arguments are not valid JSON ({e}): {args}"));
        }
    }
    Err(format!(
        "No `answer` tool call in OpenAI response: {}",
        json
    ))
}

/// POST a typed-uncertain request to Anthropic and parse the result.
///
/// Returns `(value_json, confidence)` on success.  The caller projects
/// `value_json` to the expected concrete type.
/// POST a structured-output request (the `answer` tool) for the active provider
/// and return the parsed arguments object (`{value, ...}`). Shared by the
/// uncertain (with confidence) and flat (value-only) paths via `with_confidence`.
/// Anthropic and OpenAI differ in body schema, auth, and where the tool result
/// lives in the response — all handled here so callers see one object.
fn complete_structured_inner(
    prompt: &str,
    value_type: &str,
    with_confidence: bool,
) -> Result<serde_json::Value, String> {
    let p = provider();
    let key = api_key(p)?;
    // The interpreter passes the tier-resolved model via the plain path; the
    // structured path historically pinned sonnet. Honor the balanced-tier model
    // override so a non-Anthropic provider can name its own model.
    let model = std::env::var("AXON_AI_MODEL_BALANCED")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_MODEL.to_string());

    let body = match p {
        Provider::Anthropic => {
            // Keep the existing Anthropic body builders so their pinned-shape
            // unit tests stay authoritative; swap the model in.
            let mut b = if with_confidence {
                build_typed_uncertain_body(prompt, value_type)
            } else {
                build_typed_flat_body(prompt, value_type)
            };
            b["model"] = serde_json::json!(model);
            b
        }
        Provider::OpenAi => build_openai_tool_body(&model, prompt, value_type, with_confidence),
    };

    let client = reqwest::blocking::Client::new();
    let response = auth_headers(p, client.post(endpoint_url(p)), &key)
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    if !status.is_success() {
        let label = match p {
            Provider::Anthropic => "Anthropic",
            Provider::OpenAi => "OpenAI",
        };
        return Err(format!("{label} API error {}: {}", status.as_u16(), text));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response as JSON: {}", e))?;

    match p {
        Provider::Anthropic => parse_tool_use_input(&json).cloned(),
        Provider::OpenAi => parse_openai_tool_args(&json),
    }
}

fn complete_typed_uncertain_inner(
    prompt: &str,
    value_type: &str,
) -> Result<(serde_json::Value, f64), String> {
    let input = complete_structured_inner(prompt, value_type, true)?;

    let value = input
        .get("value")
        .ok_or_else(|| format!("structured output missing `value`: {}", input))?
        .clone();

    let confidence = input["confidence"]
        .as_f64()
        .ok_or_else(|| format!("structured output `confidence` is not a number: {}", input))?;

    if !(0.0..=1.0).contains(&confidence) {
        return Err(format!("confidence {} out of range [0.0, 1.0]", confidence));
    }

    Ok((value, confidence))
}

/// Send `prompt` to Anthropic with a structured-output tool that constrains
/// the answer to `{value: integer, confidence: number}`.  Returns
/// `(value, confidence)` on success.
pub fn complete_typed_uncertain_i64(prompt: &str) -> Result<(i64, f64), String> {
    let (value, confidence) = complete_typed_uncertain_inner(prompt, "integer")?;
    let v = value
        .as_i64()
        .ok_or_else(|| format!("tool_use input `value` is not an integer: {}", value))?;
    Ok((v, confidence))
}

/// Send `prompt` to Anthropic with a structured-output tool that constrains
/// the answer to `{value: number, confidence: number}`.  Returns
/// `(value, confidence)` on success.
pub fn complete_typed_uncertain_f64(prompt: &str) -> Result<(f64, f64), String> {
    let (value, confidence) = complete_typed_uncertain_inner(prompt, "number")?;
    let v = value
        .as_f64()
        .ok_or_else(|| format!("tool_use input `value` is not a number: {}", value))?;
    Ok((v, confidence))
}

// -- Layer-3 ASI flat-T structured output (no confidence) ---------------------
//
// Variant of the typed-uncertain flow where the schema demands only a single
// `value` field — no confidence.  Used by the generic `ai_extract::<T>` builtin
// for non-Uncertain Ts (i64, f64, bool).  Each helper hard-codes the per-T
// JSON schema; we deliberately keep them as separate strings for v1 rather
// than introduce a generic schema generator (deferred to a later wedge that
// also adds struct/enum support).

/// Build the Anthropic Messages API request body for flat-T structured
/// output.  `value_type` should be one of `"integer"`, `"number"`, or
/// `"boolean"`.  Exposed as `pub` for unit-test pinning of the JSON shape.
pub fn build_typed_flat_body(prompt: &str, value_type: &str) -> serde_json::Value {
    serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "tools": [{
            "name": "answer",
            "description": "Return your answer as the single `value` field.",
            "input_schema": {
                "type": "object",
                "properties": {
                    "value": { "type": value_type }
                },
                "required": ["value"]
            }
        }],
        "tool_choice": { "type": "tool", "name": "answer" },
        "messages": [
            { "role": "user", "content": prompt }
        ]
    })
}

/// POST a flat-T structured-output request and return the parsed `value` as
/// raw `serde_json::Value`.  The caller projects to the expected scalar type.
fn complete_typed_flat_inner(prompt: &str, value_type: &str) -> Result<serde_json::Value, String> {
    let input = complete_structured_inner(prompt, value_type, false)?;
    input
        .get("value")
        .ok_or_else(|| format!("structured output missing `value`: {}", input))
        .cloned()
}

/// Send `prompt` and return a single `i64`.  Tool schema constrains
/// `{value: integer}`; rejection (no integer) becomes `Err`.
pub fn complete_typed_i64(prompt: &str) -> Result<i64, String> {
    let value = complete_typed_flat_inner(prompt, "integer")?;
    value
        .as_i64()
        .ok_or_else(|| format!("tool_use input `value` is not an integer: {}", value))
}

/// Send `prompt` and return a single `f64`.  Tool schema constrains
/// `{value: number}`.  Integers are accepted (JSON numbers).
pub fn complete_typed_f64(prompt: &str) -> Result<f64, String> {
    let value = complete_typed_flat_inner(prompt, "number")?;
    value
        .as_f64()
        .ok_or_else(|| format!("tool_use input `value` is not a number: {}", value))
}

/// Send `prompt` and return a single `bool`.  Tool schema constrains
/// `{value: boolean}`.
pub fn complete_typed_bool(prompt: &str) -> Result<bool, String> {
    let value = complete_typed_flat_inner(prompt, "boolean")?;
    value
        .as_bool()
        .ok_or_else(|| format!("tool_use input `value` is not a boolean: {}", value))
}

// -- Layer-3 typed-extraction extern bridges ----------------------------------
//
// ABI:
//   - On success the function returns 0.  `*out_value` and `*out_confidence`
//     hold the typed value and the confidence in [0.0, 1.0].
//     `*out_err_len` is set to 0 and `*out_err_ptr` is left untouched (the
//     codegen does not read it on the success path).
//   - On error the function returns 1.  `*out_err_len` and `*out_err_ptr`
//     describe a heap-allocated UTF-8 NUL-terminated error message
//     (mirroring the success-side ABI of `__axon_ai_complete`'s reply
//     buffer; the length here is *positive* — the return code is the
//     success/error discriminant, not a sign bit on the length).
//     `*out_value` and `*out_confidence` are left untouched.
//
// Null-pointer safety: any null out-param is treated as "callee discards"
// and skipped.  This keeps the bridge robust against codegen bugs that omit
// an alloca, but is not a normal path — Axon codegen always passes valid
// stack slots.

unsafe fn write_typed_err(msg: &str, out_err_len: *mut i64, out_err_ptr: *mut *mut u8) {
    if out_err_len.is_null() || out_err_ptr.is_null() {
        return;
    }
    let bytes = msg.as_bytes();
    let len = bytes.len();
    let buf = libc_malloc(len + 1);
    std::ptr::copy_nonoverlapping(bytes.as_ptr(), buf, len);
    *buf.add(len) = 0;
    *out_err_len = len as i64;
    *out_err_ptr = buf;
}

/// Anthropic structured-output extraction → `Uncertain<i64>`.
///
/// See module doc for the ABI.  Reads ANTHROPIC_API_KEY from the environment.
#[no_mangle]
pub extern "C" fn __axon_ai_extract_uncertain_i64(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_value: *mut i64,
    out_confidence: *mut f64,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) -> i32 {
    if prompt_ptr.is_null() || prompt_len < 0 {
        unsafe {
            write_typed_err(
                "ai_extract_uncertain_i64: invalid prompt pointer/length",
                out_err_len,
                out_err_ptr,
            );
        }
        return 1;
    }
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    match complete_typed_uncertain_i64(prompt) {
        Ok((v, c)) => {
            unsafe {
                if !out_value.is_null() {
                    *out_value = v;
                }
                if !out_confidence.is_null() {
                    *out_confidence = c;
                }
                if !out_err_len.is_null() {
                    *out_err_len = 0;
                }
            }
            0
        }
        Err(e) => {
            unsafe {
                write_typed_err(&e, out_err_len, out_err_ptr);
            }
            1
        }
    }
}

/// Anthropic structured-output extraction → `Uncertain<f64>`.
///
/// See module doc for the ABI.  Reads ANTHROPIC_API_KEY from the environment.
#[no_mangle]
pub extern "C" fn __axon_ai_extract_uncertain_f64(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_value: *mut f64,
    out_confidence: *mut f64,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) -> i32 {
    if prompt_ptr.is_null() || prompt_len < 0 {
        unsafe {
            write_typed_err(
                "ai_extract_uncertain_f64: invalid prompt pointer/length",
                out_err_len,
                out_err_ptr,
            );
        }
        return 1;
    }
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    match complete_typed_uncertain_f64(prompt) {
        Ok((v, c)) => {
            unsafe {
                if !out_value.is_null() {
                    *out_value = v;
                }
                if !out_confidence.is_null() {
                    *out_confidence = c;
                }
                if !out_err_len.is_null() {
                    *out_err_len = 0;
                }
            }
            0
        }
        Err(e) => {
            unsafe {
                write_typed_err(&e, out_err_len, out_err_ptr);
            }
            1
        }
    }
}

// -- Layer-3 generic ai_extract::<T> flat-T extern bridges --------------------
//
// ABI mirrors the Uncertain bridges above but drops the `out_confidence` slot:
//   - rc 0: success.  `*out_value` set; `*out_err_len` set to 0.
//   - rc 1: error.    `*out_err_*` describe a heap-allocated UTF-8 NUL-terminated
//                     error string.  `*out_value` left untouched.
//
// Null-pointer safety: any null out-param is skipped without panicking, same
// as the Uncertain path.  The codegen always passes valid stack slots.

/// Anthropic structured-output extraction → `i64`.  Five-arg ABI.
#[no_mangle]
pub extern "C" fn __axon_ai_extract_i64(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_value: *mut i64,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) -> i32 {
    if prompt_ptr.is_null() || prompt_len < 0 {
        unsafe {
            write_typed_err(
                "ai_extract_i64: invalid prompt pointer/length",
                out_err_len,
                out_err_ptr,
            );
        }
        return 1;
    }
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    match complete_typed_i64(prompt) {
        Ok(v) => {
            unsafe {
                if !out_value.is_null() {
                    *out_value = v;
                }
                if !out_err_len.is_null() {
                    *out_err_len = 0;
                }
            }
            0
        }
        Err(e) => {
            unsafe {
                write_typed_err(&e, out_err_len, out_err_ptr);
            }
            1
        }
    }
}

/// Anthropic structured-output extraction → `f64`.  Five-arg ABI.
#[no_mangle]
pub extern "C" fn __axon_ai_extract_f64(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_value: *mut f64,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) -> i32 {
    if prompt_ptr.is_null() || prompt_len < 0 {
        unsafe {
            write_typed_err(
                "ai_extract_f64: invalid prompt pointer/length",
                out_err_len,
                out_err_ptr,
            );
        }
        return 1;
    }
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    match complete_typed_f64(prompt) {
        Ok(v) => {
            unsafe {
                if !out_value.is_null() {
                    *out_value = v;
                }
                if !out_err_len.is_null() {
                    *out_err_len = 0;
                }
            }
            0
        }
        Err(e) => {
            unsafe {
                write_typed_err(&e, out_err_len, out_err_ptr);
            }
            1
        }
    }
}

/// Anthropic structured-output extraction → `bool`.  Five-arg ABI.
///
/// `out_value` is `*mut bool`; LLVM lowers Axon's `bool` to `i1` but the C
/// ABI uses `_Bool` (1 byte).  Rust's `bool` matches that, and the codegen's
/// `out_value` slot is allocated with `bool_type()` (an `i1` slot), which
/// LLVM normalises to a 1-byte slot at the ABI boundary.
#[no_mangle]
pub extern "C" fn __axon_ai_extract_bool(
    prompt_ptr: *const u8,
    prompt_len: i64,
    out_value: *mut bool,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) -> i32 {
    if prompt_ptr.is_null() || prompt_len < 0 {
        unsafe {
            write_typed_err(
                "ai_extract_bool: invalid prompt pointer/length",
                out_err_len,
                out_err_ptr,
            );
        }
        return 1;
    }
    let prompt = unsafe {
        let slice = std::slice::from_raw_parts(prompt_ptr, prompt_len as usize);
        std::str::from_utf8_unchecked(slice)
    };
    match complete_typed_bool(prompt) {
        Ok(v) => {
            unsafe {
                if !out_value.is_null() {
                    *out_value = v;
                }
                if !out_err_len.is_null() {
                    *out_err_len = 0;
                }
            }
            0
        }
        Err(e) => {
            unsafe {
                write_typed_err(&e, out_err_len, out_err_ptr);
            }
            1
        }
    }
}

// -- Tests --------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// The structured-output request body must declare a single `answer` tool
    /// whose schema requires both `value` and `confidence`, and the tool_choice
    /// must force that tool.  This is the contract that turns LLM output into
    /// a typed `Uncertain<T>`; if it drifts, callers will receive raw text and
    /// `parse_tool_use_input` will fail at runtime.
    #[test]
    fn typed_body_i64_has_required_shape() {
        let body = build_typed_uncertain_body("How many planets?", "integer");
        assert_eq!(body["model"], "claude-sonnet-4-6");
        assert_eq!(body["max_tokens"], 1024);

        let tools = body["tools"].as_array().expect("tools is array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "answer");
        let schema = &tools[0]["input_schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["value"]["type"], "integer");
        assert_eq!(schema["properties"]["confidence"]["type"], "number");
        assert_eq!(schema["properties"]["confidence"]["minimum"], 0.0);
        assert_eq!(schema["properties"]["confidence"]["maximum"], 1.0);
        let required = schema["required"].as_array().expect("required is array");
        let names: Vec<&str> = required.iter().filter_map(|v| v.as_str()).collect();
        assert!(names.contains(&"value"));
        assert!(names.contains(&"confidence"));

        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], "answer");

        let messages = body["messages"].as_array().expect("messages is array");
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["role"], "user");
        assert_eq!(messages[0]["content"], "How many planets?");
    }

    #[test]
    fn typed_body_f64_uses_number_for_value() {
        let body = build_typed_uncertain_body("Temperature in Paris?", "number");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["value"]["type"],
            "number"
        );
    }

    #[test]
    fn parse_tool_use_input_finds_answer_block() {
        let response = json!({
            "content": [
                { "type": "text", "text": "Sure!" },
                { "type": "tool_use", "name": "answer",
                  "input": { "value": 8, "confidence": 0.95 } }
            ]
        });
        let input = parse_tool_use_input(&response).expect("found tool_use");
        assert_eq!(input["value"], 8);
        assert_eq!(input["confidence"], 0.95);
    }

    #[test]
    fn parse_tool_use_input_errors_when_missing() {
        let response = json!({
            "content": [
                { "type": "text", "text": "I cannot answer that." }
            ]
        });
        let err = parse_tool_use_input(&response).unwrap_err();
        assert!(err.contains("No `answer` tool_use block"));
    }

    #[test]
    fn parse_tool_use_input_skips_other_tools() {
        let response = json!({
            "content": [
                { "type": "tool_use", "name": "other",
                  "input": { "x": 1 } }
            ]
        });
        assert!(parse_tool_use_input(&response).is_err());
    }

    // Tests that mutate ANTHROPIC_API_KEY share a global lock so cargo's
    // parallel test runner can't interleave their env-var changes.
    use std::sync::Mutex;
    static ENV_LOCK: Mutex<()> = Mutex::new(());

    /// Exercise the bridges with an unset API key so we never touch the network.
    /// The bridges must serialise the error through `out_err_*`, return 1, and
    /// leave the success out-params untouched.  Tested as one block (under the
    /// env lock) for both i64 and f64 plus the null-out-param defensive path.
    #[test]
    fn extract_bridges_report_missing_api_key_and_tolerate_nulls() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());

        let prev = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");

        // ── i64 bridge ──
        let prompt = b"hi";
        let mut value_i: i64 = i64::MIN;
        let mut conf_i: f64 = -1.0;
        let mut err_len: i64 = 0;
        let mut err_ptr: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_uncertain_i64(
            prompt.as_ptr(),
            prompt.len() as i64,
            &mut value_i,
            &mut conf_i,
            &mut err_len,
            &mut err_ptr,
        );
        assert_eq!(rc, 1, "missing API key must return 1");
        assert!(err_len > 0, "err_len should be positive on the error path");
        assert!(!err_ptr.is_null());
        // Out value/confidence untouched on error.
        assert_eq!(value_i, i64::MIN);
        assert_eq!(conf_i, -1.0);
        let msg = unsafe {
            let slice = std::slice::from_raw_parts(err_ptr, err_len as usize);
            std::str::from_utf8_unchecked(slice)
        };
        assert!(
            msg.contains("ANTHROPIC_API_KEY"),
            "expected key-error message, got: {msg}"
        );

        // ── f64 bridge ──
        let mut value_f: f64 = f64::NAN;
        let mut conf_f: f64 = -1.0;
        let mut err_len2: i64 = 0;
        let mut err_ptr2: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_uncertain_f64(
            prompt.as_ptr(),
            prompt.len() as i64,
            &mut value_f,
            &mut conf_f,
            &mut err_len2,
            &mut err_ptr2,
        );
        assert_eq!(rc, 1);
        assert!(err_len2 > 0);
        assert!(!err_ptr2.is_null());
        assert_eq!(conf_f, -1.0);

        // ── null out-params: must not segfault, return 1 ──
        let rc = __axon_ai_extract_uncertain_i64(
            prompt.as_ptr(),
            prompt.len() as i64,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(rc, 1);

        // Restore env var.
        if let Some(v) = prev {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
    }

    /// Null prompt pointer is rejected before any network attempt.
    #[test]
    fn extract_bridge_rejects_null_prompt() {
        let mut value: i64 = 0;
        let mut conf: f64 = 0.0;
        let mut err_len: i64 = 0;
        let mut err_ptr: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_uncertain_i64(
            std::ptr::null(),
            0,
            &mut value,
            &mut conf,
            &mut err_len,
            &mut err_ptr,
        );
        assert_eq!(rc, 1);
        assert!(err_len > 0);
        assert!(!err_ptr.is_null());
        let msg = unsafe {
            let slice = std::slice::from_raw_parts(err_ptr, err_len as usize);
            std::str::from_utf8_unchecked(slice)
        };
        assert!(msg.contains("invalid prompt pointer/length"), "got: {msg}");
    }

    // ── Flat-T schema-shape pinning ─────────────────────────────────────────

    /// The flat-T tool schema must declare exactly one required field
    /// (`value`) typed against the JSON Schema primitive corresponding to T.
    /// If this drifts, the LLM may emit unconstrained text and parsing will
    /// fail at runtime — so we pin the contract here.
    #[test]
    fn typed_flat_body_i64_has_required_shape() {
        let body = build_typed_flat_body("How many planets?", "integer");
        assert_eq!(body["model"], "claude-sonnet-4-6");
        let tools = body["tools"].as_array().expect("tools is array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "answer");
        let schema = &tools[0]["input_schema"];
        assert_eq!(schema["type"], "object");
        assert_eq!(schema["properties"]["value"]["type"], "integer");
        // Crucially: NO confidence field — that would conflict with the
        // schema for the Uncertain<T> variants and could confuse the LLM.
        assert!(
            schema["properties"].get("confidence").is_none(),
            "flat-T schema must not include a confidence field"
        );
        let required: Vec<&str> = schema["required"]
            .as_array()
            .expect("required")
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(required, vec!["value"]);
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], "answer");
    }

    #[test]
    fn typed_flat_body_f64_uses_number() {
        let body = build_typed_flat_body("Speed of light?", "number");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["value"]["type"],
            "number"
        );
    }

    #[test]
    fn typed_flat_body_bool_uses_boolean() {
        let body = build_typed_flat_body("Is the sky blue?", "boolean");
        assert_eq!(
            body["tools"][0]["input_schema"]["properties"]["value"]["type"],
            "boolean"
        );
    }

    /// Reuse `parse_tool_use_input` (already covered) to extract the value
    /// from a synthetic flat-T response; verify the per-T projection works
    /// without going over the wire.  This pins the success path of
    /// `complete_typed_*` end-to-end up to (but not including) the HTTP call.
    #[test]
    fn flat_response_projection_i64_ok() {
        let response = json!({
            "content": [
                { "type": "tool_use", "name": "answer",
                  "input": { "value": 42 } }
            ]
        });
        let input = parse_tool_use_input(&response).expect("found tool_use");
        let v = input["value"].as_i64().expect("integer");
        assert_eq!(v, 42);
    }

    #[test]
    fn flat_response_projection_f64_ok() {
        let response = json!({
            "content": [
                { "type": "tool_use", "name": "answer",
                  "input": { "value": 2.99792458e8 } }
            ]
        });
        let input = parse_tool_use_input(&response).expect("found tool_use");
        let v = input["value"].as_f64().expect("number");
        assert!((v - 2.99792458e8).abs() < 1e-3);
    }

    #[test]
    fn flat_response_projection_bool_ok() {
        let response = json!({
            "content": [
                { "type": "tool_use", "name": "answer",
                  "input": { "value": true } }
            ]
        });
        let input = parse_tool_use_input(&response).expect("found tool_use");
        assert_eq!(input["value"].as_bool(), Some(true));
    }

    /// A response without the `value` field produces a clean error rather
    /// than a panic — schema-violating responses must be surfaced as
    /// `Err(String)` to the Axon caller.
    #[test]
    fn flat_response_missing_value_field_is_error() {
        let response = json!({
            "content": [
                { "type": "tool_use", "name": "answer",
                  "input": { "not_value": 1 } }
            ]
        });
        let input = parse_tool_use_input(&response).expect("found tool_use");
        // `complete_typed_flat_inner`'s value extraction maps to None.
        assert!(input.get("value").is_none() || !input.get("value").unwrap().is_i64());
    }

    /// Exercise the three new flat-T extern bridges with no API key.  Each
    /// must serialise its error through `out_err_*`, return 1, and leave
    /// `*out_value` untouched.  Also covers the null-out-param defensive path.
    #[test]
    fn flat_extract_bridges_report_missing_api_key_and_tolerate_nulls() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var("ANTHROPIC_API_KEY").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");

        let prompt = b"hi";

        // ── i64 bridge ──
        let mut v_i: i64 = i64::MIN;
        let mut elen: i64 = 0;
        let mut eptr: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_i64(
            prompt.as_ptr(),
            prompt.len() as i64,
            &mut v_i,
            &mut elen,
            &mut eptr,
        );
        assert_eq!(rc, 1);
        assert!(elen > 0);
        assert!(!eptr.is_null());
        assert_eq!(v_i, i64::MIN, "out_value must be untouched on error");
        let msg = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(eptr, elen as usize))
        };
        assert!(msg.contains("ANTHROPIC_API_KEY"), "got: {msg}");

        // ── f64 bridge ──
        let mut v_f: f64 = f64::NAN;
        let mut elen2: i64 = 0;
        let mut eptr2: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_f64(
            prompt.as_ptr(),
            prompt.len() as i64,
            &mut v_f,
            &mut elen2,
            &mut eptr2,
        );
        assert_eq!(rc, 1);
        assert!(elen2 > 0);
        assert!(!eptr2.is_null());

        // ── bool bridge ──
        let mut v_b: bool = false;
        let mut elen3: i64 = 0;
        let mut eptr3: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_bool(
            prompt.as_ptr(),
            prompt.len() as i64,
            &mut v_b,
            &mut elen3,
            &mut eptr3,
        );
        assert_eq!(rc, 1);
        assert!(elen3 > 0);
        assert!(!eptr3.is_null());
        assert!(!v_b, "out_value must be untouched on error");

        // ── null out-params defensive path (i64 bridge as representative) ──
        let rc = __axon_ai_extract_i64(
            prompt.as_ptr(),
            prompt.len() as i64,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        );
        assert_eq!(rc, 1);

        // ── null prompt pointer rejected before any network attempt ──
        let mut v_i2: i64 = 0;
        let mut elen4: i64 = 0;
        let mut eptr4: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_i64(std::ptr::null(), 0, &mut v_i2, &mut elen4, &mut eptr4);
        assert_eq!(rc, 1);
        let msg2 = unsafe {
            std::str::from_utf8_unchecked(std::slice::from_raw_parts(eptr4, elen4 as usize))
        };
        assert!(
            msg2.contains("invalid prompt pointer/length"),
            "got: {msg2}"
        );

        if let Some(v) = prev {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
    }

    /// R1 parity: with AXON_AI_MOCK set, the native AI path returns the
    /// interpreter's exact deterministic stub — no API key, no network — so a
    /// native binary's AI output matches the interpreter byte-for-byte (closing
    /// the last 2-of-28 example-parity gap). Previously native ignored the env
    /// var and errored on a missing key.
    #[test]
    fn ai_complete_honors_axon_ai_mock() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_mock = std::env::var("AXON_AI_MOCK").ok();
        std::env::remove_var("ANTHROPIC_API_KEY"); // prove no key is needed
        std::env::set_var("AXON_AI_MOCK", "1");

        let got = ai_complete_inner("summarize this").expect("mock must succeed without a key");
        assert_eq!(
            got, MOCK_AI_COMPLETE,
            "native mock must equal the interpreter's stub"
        );

        // AXON_AI_MOCK=0 must NOT mock (falls through to the key check → error).
        std::env::set_var("AXON_AI_MOCK", "0");
        assert!(
            ai_complete_inner("x").is_err(),
            "AXON_AI_MOCK=0 must not mock"
        );

        match prev_mock {
            Some(v) => std::env::set_var("AXON_AI_MOCK", v),
            None => std::env::remove_var("AXON_AI_MOCK"),
        }
        if let Some(v) = prev_key {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
    }

    #[test]
    fn complete_with_usage_returns_deterministic_token_estimate_under_mock() {
        // R3 / F4: complete_with_model_usage returns (reply, total_tokens). Under
        // mock the token count is the SAME deterministic estimate the interpreter's
        // pre-dispatch meter uses (~4 chars/token + 1), so mocked cost stays exact
        // and reproducible; only a LIVE call reports the model's real usage.
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_key = std::env::var("ANTHROPIC_API_KEY").ok();
        let prev_mock = std::env::var("AXON_AI_MOCK").ok();
        std::env::remove_var("ANTHROPIC_API_KEY");
        std::env::set_var("AXON_AI_MOCK", "1");

        // 16-char prompt → 16/4 + 1 = 5 tokens.
        let (reply, tokens) = complete_with_model_usage("sixteen char p..", "claude-opus-4-8")
            .expect("mock succeeds with no key");
        assert_eq!(reply, MOCK_AI_COMPLETE);
        assert_eq!(
            tokens, 5,
            "mock token count is the deterministic prompt-length estimate"
        );
        // Determinism: same prompt → same count.
        let (_r2, t2) = complete_with_model_usage("sixteen char p..", "claude-opus-4-8").unwrap();
        assert_eq!(tokens, t2);

        match prev_mock {
            Some(v) => std::env::set_var("AXON_AI_MOCK", v),
            None => std::env::remove_var("AXON_AI_MOCK"),
        }
        if let Some(v) = prev_key {
            std::env::set_var("ANTHROPIC_API_KEY", v);
        }
    }

    // ── Provider codec (OpenAI-compatible: NIM / OpenRouter / vLLM / …) ──────

    #[test]
    fn openai_plain_body_has_chat_completions_shape() {
        // The OpenAI plain-completion body: model + max_tokens + a single user
        // message. (Same logical shape as Anthropic, but it POSTs to
        // /v1/chat/completions with a Bearer token — wiring covered below.)
        let body = build_openai_tool_body("meta/llama-3.1-70b-instruct", "Hi", "integer", false);
        assert_eq!(body["model"], "meta/llama-3.1-70b-instruct");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "Hi");
    }

    #[test]
    fn openai_tool_body_nests_schema_under_function() {
        // OpenAI tool-calling differs from Anthropic: each tool is
        // {type:"function", function:{name, parameters}}, the schema lives under
        // `function.parameters` (not top-level `input_schema`), and tool_choice
        // is {type:"function", function:{name}}. Pin that shape.
        let body = build_openai_tool_body("gpt-4o", "How many?", "integer", true);
        let tool = &body["tools"][0];
        assert_eq!(tool["type"], "function");
        assert_eq!(tool["function"]["name"], "answer");
        let params = &tool["function"]["parameters"];
        assert_eq!(params["type"], "object");
        assert_eq!(params["properties"]["value"]["type"], "integer");
        assert_eq!(params["properties"]["confidence"]["type"], "number");
        let required: Vec<&str> = params["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert!(required.contains(&"value") && required.contains(&"confidence"));
        assert_eq!(body["tool_choice"]["type"], "function");
        assert_eq!(body["tool_choice"]["function"]["name"], "answer");
    }

    #[test]
    fn openai_flat_tool_body_omits_confidence() {
        let body = build_openai_tool_body("gpt-4o", "x", "number", false);
        let props = &body["tools"][0]["function"]["parameters"]["properties"];
        assert_eq!(props["value"]["type"], "number");
        assert!(
            props.get("confidence").is_none(),
            "flat schema must not carry confidence"
        );
    }

    #[test]
    fn parse_openai_tool_args_decodes_arguments_string() {
        // OpenAI returns the tool arguments as a JSON *string* under
        // choices[0].message.tool_calls[0].function.arguments — parse it.
        let resp = json!({
            "choices": [{
                "message": {
                    "tool_calls": [{
                        "function": { "name": "answer", "arguments": "{\"value\": 8, \"confidence\": 0.9}" }
                    }]
                }
            }]
        });
        let args = parse_openai_tool_args(&resp).expect("parsed args");
        assert_eq!(args["value"], 8);
        assert_eq!(args["confidence"], 0.9);
    }

    #[test]
    fn parse_openai_tool_args_errors_without_answer_call() {
        let resp = json!({
            "choices": [{ "message": { "tool_calls": [{
                "function": { "name": "other", "arguments": "{}" }
            }]}}]
        });
        assert!(parse_openai_tool_args(&resp).is_err());
    }

    #[test]
    fn provider_defaults_to_anthropic_and_reads_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev = std::env::var("AXON_AI_PROVIDER").ok();
        std::env::remove_var("AXON_AI_PROVIDER");
        assert_eq!(provider(), Provider::Anthropic, "default is Anthropic");
        std::env::set_var("AXON_AI_PROVIDER", "openai");
        assert_eq!(provider(), Provider::OpenAi);
        std::env::set_var("AXON_AI_PROVIDER", "nonsense");
        assert_eq!(
            provider(),
            Provider::Anthropic,
            "unknown falls back to Anthropic"
        );
        match prev {
            Some(v) => std::env::set_var("AXON_AI_PROVIDER", v),
            None => std::env::remove_var("AXON_AI_PROVIDER"),
        }
    }

    #[test]
    fn endpoint_and_base_url_per_provider() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prev_base = std::env::var("AXON_AI_BASE_URL").ok();
        let prev_abase = std::env::var("ANTHROPIC_BASE_URL").ok();
        std::env::remove_var("AXON_AI_BASE_URL");
        std::env::remove_var("ANTHROPIC_BASE_URL");
        // Defaults.
        assert_eq!(
            endpoint_url(Provider::Anthropic),
            "https://api.anthropic.com/v1/messages"
        );
        assert_eq!(
            endpoint_url(Provider::OpenAi),
            "https://api.openai.com/v1/chat/completions"
        );
        // Neutral override wins, trailing slash tolerated, path appended per provider.
        std::env::set_var("AXON_AI_BASE_URL", "https://integrate.api.nvidia.com/");
        assert_eq!(
            endpoint_url(Provider::OpenAi),
            "https://integrate.api.nvidia.com/v1/chat/completions"
        );
        // Legacy ANTHROPIC_BASE_URL still works (a trainloop gateway uses this).
        std::env::remove_var("AXON_AI_BASE_URL");
        std::env::set_var("ANTHROPIC_BASE_URL", "https://gateway.internal");
        assert_eq!(
            endpoint_url(Provider::Anthropic),
            "https://gateway.internal/v1/messages"
        );
        match prev_base {
            Some(v) => std::env::set_var("AXON_AI_BASE_URL", v),
            None => std::env::remove_var("AXON_AI_BASE_URL"),
        }
        match prev_abase {
            Some(v) => std::env::set_var("ANTHROPIC_BASE_URL", v),
            None => std::env::remove_var("ANTHROPIC_BASE_URL"),
        }
    }

    // ── .env loading ────────────────────────────────────────────────────────

    #[test]
    fn dotenv_parser_handles_comments_quotes_and_export() {
        let src = "\
# a comment\n\
\n\
ANTHROPIC_API_KEY=sk-plain\n\
export AXON_AI_PROVIDER=\"openai\"\n\
AXON_AI_BASE_URL='https://x.example/'\n\
  SPACED = value \n\
NOEQUALS\n\
=novalue\n";
        let pairs = parse_dotenv(src);
        let get = |k: &str| {
            pairs
                .iter()
                .find(|(kk, _)| kk == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(get("ANTHROPIC_API_KEY"), Some("sk-plain"));
        assert_eq!(
            get("AXON_AI_PROVIDER"),
            Some("openai"),
            "double quotes stripped"
        );
        assert_eq!(
            get("AXON_AI_BASE_URL"),
            Some("https://x.example/"),
            "single quotes stripped"
        );
        assert_eq!(get("SPACED"), Some("value"), "key/value trimmed");
        assert!(get("NOEQUALS").is_none(), "no `=` line skipped");
        assert!(
            pairs.iter().all(|(k, _)| !k.is_empty()),
            "empty key skipped"
        );
    }

    #[test]
    fn dotenv_does_not_override_existing_env() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var("AXON_TEST_DOTENV_VAR", "from-shell");
        apply_dotenv("AXON_TEST_DOTENV_VAR=from-file\nAXON_TEST_DOTENV_NEW=from-file");
        // The real environment wins; a new key is set.
        assert_eq!(
            std::env::var("AXON_TEST_DOTENV_VAR").as_deref(),
            Ok("from-shell")
        );
        assert_eq!(
            std::env::var("AXON_TEST_DOTENV_NEW").as_deref(),
            Ok("from-file")
        );
        std::env::remove_var("AXON_TEST_DOTENV_VAR");
        std::env::remove_var("AXON_TEST_DOTENV_NEW");
    }
}
