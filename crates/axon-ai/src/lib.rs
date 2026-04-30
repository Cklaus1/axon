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

use std::alloc::Layout;

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
        .post("https://api.anthropic.com/v1/messages")
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
