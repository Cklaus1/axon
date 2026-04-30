//! Provenance logger for `@[adaptive]` functions.
//!
//! Each invocation appends one JSON line to `~/.cache/axon/provenance.jsonl`
//! recording the function name, an event ("call" / "return"), a free-form
//! payload string, and a timestamp (RFC3339-ish, derived from
//! `SystemTime::now()`).
//!
//! The implementation is intentionally best-effort: any filesystem error
//! (missing $HOME, permission denied, full disk, etc.) is silently swallowed
//! so we never panic in user code.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

/// Append one provenance event to `~/.cache/axon/provenance.jsonl`.
///
/// Called from compiler-generated IR for any function carrying
/// `@[adaptive(...)]`.  Both string buffers are pointer + length pairs in the
/// usual Axon `str` ABI.  `payload` may be empty.
#[no_mangle]
pub extern "C" fn __axon_provenance_log(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    payload_ptr: *const u8,
    payload_len: i64,
) {
    // Safety: the Axon codegen passes valid {ptr,len} pairs that came from
    // the constant pool or from compiler-emitted string literals.  An empty
    // payload is encoded as len=0 and a possibly-null pointer.
    let fn_name = slice_to_str(fn_name_ptr, fn_name_len);
    let payload = slice_to_str(payload_ptr, payload_len);
    let _ = log_event(fn_name, payload);
}

fn slice_to_str<'a>(ptr: *const u8, len: i64) -> &'a str {
    if ptr.is_null() || len <= 0 {
        return "";
    }
    unsafe {
        let bytes = std::slice::from_raw_parts(ptr, len as usize);
        std::str::from_utf8(bytes).unwrap_or("")
    }
}

/// `payload` is conventionally either the literal string `"call"` / `"return"`
/// (treated as the event tag) or a serialised payload produced by the user.
/// For v1 we always treat the first 16 bytes-or-less as the event tag if it
/// matches `call`/`return`; otherwise it is recorded as a generic payload.
fn log_event(fn_name: &str, payload: &str) -> std::io::Result<()> {
    let dir = provenance_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no cache dir")
    })?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("provenance.jsonl");

    let (event, body) = match payload {
        "call" | "return" => (payload, ""),
        other             => ("event", other),
    };

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    // Hand-rolled JSON to avoid pulling serde_json into axon-rt.
    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{fn_q},\"event\":{ev_q},\"payload\":{pl_q}}}\n",
        ts    = ts,
        fn_q  = json_quote(fn_name),
        ev_q  = json_quote(event),
        pl_q  = json_quote(body),
    );

    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

fn provenance_dir() -> Option<PathBuf> {
    // Honour $XDG_CACHE_HOME if set, otherwise fall back to $HOME/.cache.
    if let Ok(xdg) = std::env::var("XDG_CACHE_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("axon"));
        }
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".cache").join("axon"))
}

fn json_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"'  => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c    => out.push(c),
        }
    }
    out.push('"');
    out
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn log_event_writes_line() {
        // Redirect to a tempdir.
        let tmp = std::env::temp_dir().join(format!("axon-prov-test-{}", std::process::id()));
        std::env::set_var("XDG_CACHE_HOME", &tmp);
        let _ = fs::remove_dir_all(&tmp);

        log_event("test_fn", "call").unwrap();
        log_event("test_fn", "return").unwrap();

        let path = tmp.join("axon").join("provenance.jsonl");
        let content = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"fn\":\"test_fn\""));
        assert!(lines[0].contains("\"event\":\"call\""));
        assert!(lines[1].contains("\"event\":\"return\""));

        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn extern_log_does_not_panic_on_null() {
        // null fn_name and payload; should not panic.
        __axon_provenance_log(std::ptr::null(), 0, std::ptr::null(), 0);
    }

    #[test]
    fn json_quote_escapes_specials() {
        assert_eq!(json_quote("a\"b"),  "\"a\\\"b\"");
        assert_eq!(json_quote("a\nb"),  "\"a\\nb\"");
    }
}
