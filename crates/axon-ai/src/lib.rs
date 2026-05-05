//! Axon AI runtime -- live Anthropic inference via __axon_ai_complete.
//!
//! Compiled as a static library and linked into every Axon binary that uses
//! the ai_complete builtin.  All exported symbols use C linkage so the
//! LLVM-emitted code can call them directly by name.
//!
//! ABI mirrors __axon_read_file from axon-rt:
//!   - success: *out_len >= 0, *out_ptr points to heap-allocated UTF-8
//!   - error:   *out_len < 0 (negated message length), *out_ptr points to
//!              the error message
//!
//! Layer-3 ASI typed-extraction surface (`__axon_ai_extract_uncertain_i64` /
//! `__axon_ai_extract_uncertain_f64`) uses a different ABI: a return-code
//! plus typed value/confidence out-params; the err string out-param is only
//! populated when the return code is 1.  See per-symbol docs.

use std::alloc::Layout;

// -- Endpoint resolution -------------------------------------------------------
//
// Reads the optional ANTHROPIC_BASE_URL environment variable and returns the
// full Messages API URL.  Defaults to api.anthropic.com when unset or empty.
//
// Use cases:
//   * Cost / latency observability via Helicone (point at oai.helicone.ai)
//   * Local mock / replay during demo development
//   * Region-specific deployments
//
// Trailing slashes on the base URL are tolerated.
fn messages_url() -> String {
    let base = std::env::var("ANTHROPIC_BASE_URL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "https://api.anthropic.com".to_string());
    let trimmed = base.trim_end_matches('/');
    format!("{}/v1/messages", trimmed)
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
        Err(e)    => unsafe { write_err_out(&e,     out_len, out_ptr) },
    }
}

fn ai_complete_inner(prompt: &str) -> Result<String, String> {
    // Read API key from environment.
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY is not set".to_string())?;

    // Build request body.
    let body = serde_json::json!({
        "model": "claude-sonnet-4-6",
        "max_tokens": 1024,
        "messages": [
            { "role": "user", "content": prompt }
        ]
    });

    // POST to Anthropic Messages API.
    let client = reqwest::blocking::Client::new();
    let response = client
        .post(messages_url())
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    if !status.is_success() {
        return Err(format!("Anthropic API error {}: {}", status.as_u16(), text));
    }

    // Parse the response JSON and extract content[0].text.
    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response as JSON: {}", e))?;

    let reply = json["content"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item["text"].as_str())
        .ok_or_else(|| format!("Unexpected API response shape: {}", text))?
        .to_string();

    Ok(reply)
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
    let content = json["content"].as_array()
        .ok_or_else(|| format!("Unexpected API response shape (no content array): {}", json))?;
    for item in content {
        if item["type"].as_str() == Some("tool_use")
            && item["name"].as_str() == Some("answer")
        {
            return item.get("input")
                .ok_or_else(|| format!("tool_use block missing `input`: {}", item));
        }
    }
    Err(format!("No `answer` tool_use block in response: {}", json))
}

/// POST a typed-uncertain request to Anthropic and parse the result.
///
/// Returns `(value_json, confidence)` on success.  The caller projects
/// `value_json` to the expected concrete type.
fn complete_typed_uncertain_inner(
    prompt: &str,
    value_type: &str,
) -> Result<(serde_json::Value, f64), String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY is not set".to_string())?;

    let body = build_typed_uncertain_body(prompt, value_type);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(messages_url())
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    if !status.is_success() {
        return Err(format!("Anthropic API error {}: {}", status.as_u16(), text));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response as JSON: {}", e))?;

    let input = parse_tool_use_input(&json)?;

    let value = input.get("value")
        .ok_or_else(|| format!("tool_use input missing `value`: {}", input))?
        .clone();

    let confidence = input["confidence"].as_f64()
        .ok_or_else(|| format!("tool_use input `confidence` is not a number: {}", input))?;

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
    let v = value.as_i64()
        .ok_or_else(|| format!("tool_use input `value` is not an integer: {}", value))?;
    Ok((v, confidence))
}

/// Send `prompt` to Anthropic with a structured-output tool that constrains
/// the answer to `{value: number, confidence: number}`.  Returns
/// `(value, confidence)` on success.
pub fn complete_typed_uncertain_f64(prompt: &str) -> Result<(f64, f64), String> {
    let (value, confidence) = complete_typed_uncertain_inner(prompt, "number")?;
    let v = value.as_f64()
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
fn complete_typed_flat_inner(
    prompt: &str,
    value_type: &str,
) -> Result<serde_json::Value, String> {
    let api_key = std::env::var("ANTHROPIC_API_KEY")
        .map_err(|_| "ANTHROPIC_API_KEY is not set".to_string())?;

    let body = build_typed_flat_body(prompt, value_type);

    let client = reqwest::blocking::Client::new();
    let response = client
        .post(messages_url())
        .header("x-api-key", &api_key)
        .header("anthropic-version", "2023-06-01")
        .header("content-type", "application/json")
        .json(&body)
        .send()
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    let text = response
        .text()
        .map_err(|e| format!("Failed to read response body: {}", e))?;

    if !status.is_success() {
        return Err(format!("Anthropic API error {}: {}", status.as_u16(), text));
    }

    let json: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("Failed to parse API response as JSON: {}", e))?;

    let input = parse_tool_use_input(&json)?;
    let value = input.get("value")
        .ok_or_else(|| format!("tool_use input missing `value`: {}", input))?
        .clone();
    Ok(value)
}

/// Send `prompt` and return a single `i64`.  Tool schema constrains
/// `{value: integer}`; rejection (no integer) becomes `Err`.
pub fn complete_typed_i64(prompt: &str) -> Result<i64, String> {
    let value = complete_typed_flat_inner(prompt, "integer")?;
    value.as_i64()
        .ok_or_else(|| format!("tool_use input `value` is not an integer: {}", value))
}

/// Send `prompt` and return a single `f64`.  Tool schema constrains
/// `{value: number}`.  Integers are accepted (JSON numbers).
pub fn complete_typed_f64(prompt: &str) -> Result<f64, String> {
    let value = complete_typed_flat_inner(prompt, "number")?;
    value.as_f64()
        .ok_or_else(|| format!("tool_use input `value` is not a number: {}", value))
}

/// Send `prompt` and return a single `bool`.  Tool schema constrains
/// `{value: boolean}`.
pub fn complete_typed_bool(prompt: &str) -> Result<bool, String> {
    let value = complete_typed_flat_inner(prompt, "boolean")?;
    value.as_bool()
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

unsafe fn write_typed_err(
    msg: &str,
    out_err_len: *mut i64,
    out_err_ptr: *mut *mut u8,
) {
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
            write_typed_err("ai_extract_uncertain_i64: invalid prompt pointer/length", out_err_len, out_err_ptr);
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
                if !out_value.is_null()      { *out_value = v; }
                if !out_confidence.is_null() { *out_confidence = c; }
                if !out_err_len.is_null()    { *out_err_len = 0; }
            }
            0
        }
        Err(e) => {
            unsafe { write_typed_err(&e, out_err_len, out_err_ptr); }
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
            write_typed_err("ai_extract_uncertain_f64: invalid prompt pointer/length", out_err_len, out_err_ptr);
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
                if !out_value.is_null()      { *out_value = v; }
                if !out_confidence.is_null() { *out_confidence = c; }
                if !out_err_len.is_null()    { *out_err_len = 0; }
            }
            0
        }
        Err(e) => {
            unsafe { write_typed_err(&e, out_err_len, out_err_ptr); }
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
            write_typed_err("ai_extract_i64: invalid prompt pointer/length", out_err_len, out_err_ptr);
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
                if !out_value.is_null()   { *out_value = v; }
                if !out_err_len.is_null() { *out_err_len = 0; }
            }
            0
        }
        Err(e) => {
            unsafe { write_typed_err(&e, out_err_len, out_err_ptr); }
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
            write_typed_err("ai_extract_f64: invalid prompt pointer/length", out_err_len, out_err_ptr);
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
                if !out_value.is_null()   { *out_value = v; }
                if !out_err_len.is_null() { *out_err_len = 0; }
            }
            0
        }
        Err(e) => {
            unsafe { write_typed_err(&e, out_err_len, out_err_ptr); }
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
            write_typed_err("ai_extract_bool: invalid prompt pointer/length", out_err_len, out_err_ptr);
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
                if !out_value.is_null()   { *out_value = v; }
                if !out_err_len.is_null() { *out_err_len = 0; }
            }
            0
        }
        Err(e) => {
            unsafe { write_typed_err(&e, out_err_len, out_err_ptr); }
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
        assert!(msg.contains("ANTHROPIC_API_KEY"), "expected key-error message, got: {msg}");

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
        if let Some(v) = prev { std::env::set_var("ANTHROPIC_API_KEY", v); }
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
        assert!(schema["properties"].get("confidence").is_none(),
            "flat-T schema must not include a confidence field");
        let required: Vec<&str> = schema["required"].as_array().expect("required")
            .iter().filter_map(|v| v.as_str()).collect();
        assert_eq!(required, vec!["value"]);
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], "answer");
    }

    #[test]
    fn typed_flat_body_f64_uses_number() {
        let body = build_typed_flat_body("Speed of light?", "number");
        assert_eq!(body["tools"][0]["input_schema"]["properties"]["value"]["type"], "number");
    }

    #[test]
    fn typed_flat_body_bool_uses_boolean() {
        let body = build_typed_flat_body("Is the sky blue?", "boolean");
        assert_eq!(body["tools"][0]["input_schema"]["properties"]["value"]["type"], "boolean");
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
            prompt.as_ptr(), prompt.len() as i64,
            &mut v_i, &mut elen, &mut eptr,
        );
        assert_eq!(rc, 1);
        assert!(elen > 0);
        assert!(!eptr.is_null());
        assert_eq!(v_i, i64::MIN, "out_value must be untouched on error");
        let msg = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(eptr, elen as usize)) };
        assert!(msg.contains("ANTHROPIC_API_KEY"), "got: {msg}");

        // ── f64 bridge ──
        let mut v_f: f64 = f64::NAN;
        let mut elen2: i64 = 0;
        let mut eptr2: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_f64(
            prompt.as_ptr(), prompt.len() as i64,
            &mut v_f, &mut elen2, &mut eptr2,
        );
        assert_eq!(rc, 1);
        assert!(elen2 > 0);
        assert!(!eptr2.is_null());

        // ── bool bridge ──
        let mut v_b: bool = false;
        let mut elen3: i64 = 0;
        let mut eptr3: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_bool(
            prompt.as_ptr(), prompt.len() as i64,
            &mut v_b, &mut elen3, &mut eptr3,
        );
        assert_eq!(rc, 1);
        assert!(elen3 > 0);
        assert!(!eptr3.is_null());
        assert_eq!(v_b, false, "out_value must be untouched on error");

        // ── null out-params defensive path (i64 bridge as representative) ──
        let rc = __axon_ai_extract_i64(
            prompt.as_ptr(), prompt.len() as i64,
            std::ptr::null_mut(), std::ptr::null_mut(), std::ptr::null_mut(),
        );
        assert_eq!(rc, 1);

        // ── null prompt pointer rejected before any network attempt ──
        let mut v_i2: i64 = 0;
        let mut elen4: i64 = 0;
        let mut eptr4: *mut u8 = std::ptr::null_mut();
        let rc = __axon_ai_extract_i64(
            std::ptr::null(), 0,
            &mut v_i2, &mut elen4, &mut eptr4,
        );
        assert_eq!(rc, 1);
        let msg2 = unsafe { std::str::from_utf8_unchecked(std::slice::from_raw_parts(eptr4, elen4 as usize)) };
        assert!(msg2.contains("invalid prompt pointer/length"), "got: {msg2}");

        if let Some(v) = prev { std::env::set_var("ANTHROPIC_API_KEY", v); }
    }
}
