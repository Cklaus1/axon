//! The interpreter's host-interface seam — `AxonHost`.
//!
//! Five std touchpoints (fs / env / time / sleep) live behind a single trait
//! so a browser or wasm build can supply a virtual implementation without
//! threading a parameter through the interpreter's call chain. On native the
//! active host is `DefaultHost` which wraps std exactly as today, so native
//! behavior is byte-identical.

use std::cell::RefCell;
// Only the native clock path + the test mock use SystemTime; the wasm now_ms is
// a fixed 0 (no clock there), so the import is native/test-only.
#[cfg(not(target_arch = "wasm32"))]
use std::time::{SystemTime, UNIX_EPOCH};
// reqwest blocking client is gated on asi-runtime (same feature that enables axon-ai).
#[cfg(feature = "asi-runtime")]
use reqwest;

// ── The trait ────────────────────────────────────────────────────────────────

/// The host capabilities the interpreter needs from its environment. A
/// `DefaultHost` (native) wraps std; a browser/wasm build supplies a virtual
/// impl (fetch-backed FS, a provided env map, performance.now). Every method
/// returns the SAME shape the builtin already returns, so behavior is identical.
pub trait AxonHost {
    fn read_file(&self, path: &str) -> Result<String, String>;
    fn write_file(&self, path: &str, data: &str) -> Result<(), String>;
    fn env_var(&self, key: &str) -> Option<String>;
    fn now_ms(&self) -> i64;
    fn sleep_ms(&self, ms: u64);
    /// Spawn a process (the `exec` builtin / `@[contained] exec` capability).
    /// `cmd` is the program; `args` the argument list. Returns the captured
    /// stdout on success, or a `str` error. Defaults to **denied** — a host that
    /// does not explicitly grant process spawning refuses it, so a virtual
    /// (browser/wasm/sandbox) host is exec-free unless it opts in. `DefaultHost`
    /// (native) overrides this to actually spawn.
    fn exec(&self, _cmd: &str, _args: &[String]) -> Result<String, String> {
        Err("exec is not permitted by the active host".to_string())
    }

    /// HTTP GET `url` with `headers` (a JSON object string, `"{}"` for none).
    /// Returns Ok(body) on 2xx, Err("HTTP <status>: <body>") on non-2xx, or
    /// Err(message) on transport failure. Defaults to **denied** — a virtual
    /// (wasm/sandbox) host is network-free unless it opts in. `DefaultHost`
    /// (native, `asi-runtime` feature) overrides this to use a blocking reqwest
    /// client.
    fn http_get(&self, _url: &str, _headers: &str) -> Result<String, String> {
        Err("http_get requires the asi-runtime feature or a network-capable host".to_string())
    }

    /// HTTP POST `url` with `headers` (JSON object string) and `body`.
    /// Returns Ok(response_body) on 2xx, Err("HTTP <status>: <body>") on non-2xx,
    /// or Err(message) on transport failure. Defaults to **denied**.
    fn http_post(&self, _url: &str, _headers: &str, _body: &str) -> Result<String, String> {
        Err("http_post requires the asi-runtime feature or a network-capable host".to_string())
    }

    /// Stream Server-Sent Events from `url`. Sets `Accept: text/event-stream`
    /// automatically. `headers` is a JSON object string (`""` for none). Blocks
    /// until the stream closes, collecting each SSE `data:` payload into a
    /// `Vec<String>` (one entry per event, multiline events joined with `\n`).
    /// Returns `Ok(events)` or `Err(message)`. Defaults to **denied**.
    fn http_sse(&self, _url: &str, _headers: &str) -> Result<Vec<String>, String> {
        Err("http_sse requires the asi-runtime feature or a network-capable host".to_string())
    }

    /// Like `http_sse` but issues a POST request with `body`. Required by LLM
    /// provider streaming APIs (Anthropic, OpenAI) which use POST + stream=true.
    /// Sets `Accept: text/event-stream` automatically. Defaults to **denied**.
    fn http_sse_post(
        &self,
        _url: &str,
        _headers: &str,
        _body: &str,
    ) -> Result<Vec<String>, String> {
        Err("http_sse_post requires the asi-runtime feature or a network-capable host".to_string())
    }
}

// ── SSE parsing ─────────────────────────────────────────────────────────────

/// Collect Server-Sent Events from any `BufRead` source.
/// One entry per event; multiline events (multiple `data:` fields before the
/// blank-line boundary) are joined with `\n`.  `event:`, `id:`, and `retry:`
/// fields are silently ignored — only `data:` payloads are returned.
#[allow(dead_code)]
pub(crate) fn collect_sse_events(reader: impl std::io::BufRead) -> Result<Vec<String>, String> {
    let mut events: Vec<String> = Vec::new();
    let mut current_data = String::new();
    for line in reader.lines() {
        let line = line.map_err(|e| format!("http_sse: read error: {e}"))?;
        if let Some(data) = line.strip_prefix("data: ") {
            if !current_data.is_empty() {
                current_data.push('\n');
            }
            current_data.push_str(data);
        } else if line.is_empty() && !current_data.is_empty() {
            events.push(std::mem::take(&mut current_data));
        }
    }
    if !current_data.is_empty() {
        events.push(current_data);
    }
    Ok(events)
}

// ── Default implementation ───────────────────────────────────────────────────

/// Wraps the Rust std library exactly as the interpreter used to call it
/// directly — byte-identical native behavior.
pub struct DefaultHost;

impl AxonHost for DefaultHost {
    fn read_file(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(path).map_err(|e| e.to_string())
    }

    fn write_file(&self, path: &str, data: &str) -> Result<(), String> {
        std::fs::write(path, data).map_err(|e| e.to_string())
    }

    fn env_var(&self, key: &str) -> Option<String> {
        std::env::var(key).ok()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }
    // wasm32 has no SystemTime clock (it PANICS on unknown-unknown). A real clock
    // arrives via the JS-import host in the R7c browser binding; here `now_ms()`
    // is a fixed 0 so a program that reads it doesn't trap.
    #[cfg(target_arch = "wasm32")]
    fn now_ms(&self) -> i64 {
        0
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
    }
    // wasm32 has no OS threads — sleeping is a no-op (a browser host would yield
    // to the event loop via the R7c async binding instead).
    #[cfg(target_arch = "wasm32")]
    fn sleep_ms(&self, _ms: u64) {}

    fn exec(&self, cmd: &str, args: &[String]) -> Result<String, String> {
        let output = std::process::Command::new(cmd)
            .args(args)
            .output()
            .map_err(|e| format!("exec `{cmd}` failed: {e}"))?;
        if output.status.success() {
            Ok(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            let code = output
                .status
                .code()
                .map(|c| c.to_string())
                .unwrap_or_else(|| "signal".to_string());
            let stderr = String::from_utf8_lossy(&output.stderr);
            Err(format!("exec `{cmd}` exited {code}: {}", stderr.trim()))
        }
    }

    #[cfg(feature = "asi-runtime")]
    fn http_get(&self, url: &str, headers: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let mut req = client.get(url);
        if !headers.is_empty() && headers != "{}" {
            let hmap: std::collections::HashMap<String, String> = serde_json::from_str(headers)
                .map_err(|e| format!("http_get: invalid headers JSON: {e}"))?;
            for (k, v) in hmap {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        let resp = req.send().map_err(|e| format!("http_get: {e}"))?;
        let status = resp.status();
        let body = resp
            .text()
            .map_err(|e| format!("http_get: body read failed: {e}"))?;
        if status.is_success() {
            Ok(body)
        } else {
            Err(format!("http_get: HTTP {status}: {body}"))
        }
    }

    #[cfg(feature = "asi-runtime")]
    fn http_post(&self, url: &str, headers: &str, body: &str) -> Result<String, String> {
        let client = reqwest::blocking::Client::new();
        let mut req = client.post(url).body(body.to_string());
        if !headers.is_empty() && headers != "{}" {
            let hmap: std::collections::HashMap<String, String> = serde_json::from_str(headers)
                .map_err(|e| format!("http_post: invalid headers JSON: {e}"))?;
            for (k, v) in hmap {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        let resp = req.send().map_err(|e| format!("http_post: {e}"))?;
        let status = resp.status();
        let resp_body = resp
            .text()
            .map_err(|e| format!("http_post: body read failed: {e}"))?;
        if status.is_success() {
            Ok(resp_body)
        } else {
            Err(format!("http_post: HTTP {status}: {resp_body}"))
        }
    }

    #[cfg(feature = "asi-runtime")]
    fn http_sse(&self, url: &str, headers: &str) -> Result<Vec<String>, String> {
        use std::io::BufReader;
        let client = reqwest::blocking::Client::new();
        let mut req = client
            .get(url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache");
        if !headers.is_empty() {
            let hmap: std::collections::HashMap<String, String> = serde_json::from_str(headers)
                .map_err(|e| format!("http_sse: invalid headers JSON: {e}"))?;
            for (k, v) in hmap {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        let resp = req.send().map_err(|e| format!("http_sse: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let body = resp
                .text()
                .map_err(|e| format!("http_sse: body read failed: {e}"))?;
            return Err(format!("http_sse: HTTP {status}: {body}"));
        }
        collect_sse_events(BufReader::new(resp))
    }

    #[cfg(feature = "asi-runtime")]
    fn http_sse_post(&self, url: &str, headers: &str, body: &str) -> Result<Vec<String>, String> {
        use std::io::BufReader;
        let client = reqwest::blocking::Client::new();
        let mut req = client
            .post(url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache")
            .body(body.to_string());
        if !headers.is_empty() {
            let hmap: std::collections::HashMap<String, String> = serde_json::from_str(headers)
                .map_err(|e| format!("http_sse_post: invalid headers JSON: {e}"))?;
            for (k, v) in hmap {
                req = req.header(k.as_str(), v.as_str());
            }
        }
        let resp = req.send().map_err(|e| format!("http_sse_post: {e}"))?;
        if !resp.status().is_success() {
            let status = resp.status();
            let err_body = resp
                .text()
                .map_err(|e| format!("http_sse_post: body read failed: {e}"))?;
            return Err(format!("http_sse_post: HTTP {status}: {err_body}"));
        }
        collect_sse_events(BufReader::new(resp))
    }
}

// ── Thread-local host storage ────────────────────────────────────────────────

thread_local! {
    static HOST: RefCell<Box<dyn AxonHost>> = RefCell::new(Box::new(DefaultHost));
}

/// Borrow the thread-local host and call `f` on it.
pub fn with_host<R>(f: impl FnOnce(&dyn AxonHost) -> R) -> R {
    HOST.with(|h| f(h.borrow().as_ref()))
}

/// Replace the thread-local host unconditionally.
pub fn set_host(h: Box<dyn AxonHost>) {
    HOST.with(|cell| *cell.borrow_mut() = h);
}

/// Scoped guard that resets the thread-local host to `DefaultHost` on drop,
/// discarding whatever was installed during its lifetime.
///
/// Mirrors the `FnNameGuard` / `OUTPUT_SINK` save-and-restore pattern used
/// elsewhere in the interpreter: it captures the host that was active when it
/// was installed and restores **that** host on drop — NOT unconditionally
/// `DefaultHost`. This makes nested overrides correct (an inner guard restores
/// the outer override, not the global default).
pub struct HostGuard {
    prev: Option<Box<dyn AxonHost>>,
}

impl HostGuard {
    /// Install `h` as the current host, capturing the previous host so it can be
    /// restored on drop. Returns the guard.
    pub fn install(h: Box<dyn AxonHost>) -> Self {
        let prev = HOST.with(|cell| cell.replace(h));
        Self { prev: Some(prev) }
    }
}

impl Drop for HostGuard {
    fn drop(&mut self) {
        if let Some(prev) = self.prev.take() {
            HOST.with(|cell| *cell.borrow_mut() = prev);
        }
    }
}

/// Run `f` with a custom host for its duration. On return (or panic), the
/// previous host is automatically restored.
pub fn with_host_override<R>(h: Box<dyn AxonHost>, f: impl FnOnce() -> R) -> R {
    let _guard = HostGuard::install(h);
    f()
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// In-memory host backed by a simple string map — files live in a
    /// `HashMap<String, String>`.  Env vars are read from a separate map.
    #[derive(Clone)]
    struct TestHost {
        files: std::sync::Arc<Mutex<HashMap<String, String>>>,
        env: std::sync::Arc<Mutex<HashMap<String, String>>>,
        now: std::sync::Arc<Mutex<Option<i64>>>,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                files: std::sync::Arc::new(Mutex::new(HashMap::new())),
                env: std::sync::Arc::new(Mutex::new(HashMap::new())),
                now: std::sync::Arc::new(Mutex::new(None)),
            }
        }
    }

    impl AxonHost for TestHost {
        fn read_file(&self, path: &str) -> Result<String, String> {
            self.files
                .lock()
                .unwrap()
                .get(path)
                .cloned()
                .ok_or_else(|| format!("not found: {path}"))
        }

        fn write_file(&self, path: &str, data: &str) -> Result<(), String> {
            self.files
                .lock()
                .unwrap()
                .insert(path.to_string(), data.to_string());
            Ok(())
        }

        fn env_var(&self, key: &str) -> Option<String> {
            self.env.lock().unwrap().get(key).cloned()
        }

        fn now_ms(&self) -> i64 {
            let mut guard = self.now.lock().unwrap();
            if guard.is_none() {
                *guard = Some(
                    SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|d| d.as_millis() as i64)
                        .unwrap_or(0),
                );
            }
            guard.unwrap()
        }

        fn sleep_ms(&self, _ms: u64) {
            // No-op in tests.
        }
    }

    // ── Unit tests ─────────────────────────────────────────────────────────

    /// DefaultHost write/read/env/now behave like the direct std calls.
    #[test]
    fn default_host_matches_std_behavior() {
        let h = DefaultHost;

        // write_file → read_file round-trip via std.
        let tmp = format!("/tmp/axon_test_{}", std::process::id());
        let payload = "hello host seam";
        assert!(h.write_file(&tmp, payload).is_ok());
        let read_via_host = h.read_file(&tmp).unwrap();
        assert_eq!(read_via_host, payload);

        // Write via std, read via host — same content.
        std::fs::write(&tmp, "std wrote this").unwrap();
        assert_eq!(h.read_file(&tmp).unwrap(), "std wrote this");

        // env_var mirrors std::env.
        std::env::set_var("AXON_HOST_TEST_VAR", "test_value");
        assert_eq!(h.env_var("AXON_HOST_TEST_VAR").unwrap(), "test_value");
        assert!(h.env_var("NONEXISTENT_AXON_HOST_X").is_none());

        // now_ms > 0.
        assert!(h.now_ms() > 0);

        let _ = std::fs::remove_file(&tmp);
    }

    /// Installing a TestHost via with_host_override intercepts all I/O.
    #[test]
    fn host_seam_routes_file_io_through_axonhost() {
        let host = TestHost::new();

        // Pre-seed env.
        host.env
            .lock()
            .unwrap()
            .insert("MY_KEY".into(), "my_val".into());

        with_host_override(Box::new(host.clone()), || {
            // write_file goes to the in-memory map.
            assert!(with_host(|h| h.write_file("/a", "aaa")).is_ok());

            // read_file reads from the in-memory map.
            assert_eq!(with_host(|h| h.read_file("/a")).unwrap(), "aaa");

            // A path not in the map errors.
            assert!(with_host(|h| h.read_file("/notfound")).is_err());

            // env_var is intercepted.
            assert_eq!(with_host(|h| h.env_var("MY_KEY")).unwrap(), "my_val");
            assert!(with_host(|h| h.env_var("GHOST")).is_none());
        });

        // After the override drops, DefaultHost is restored — real fs.
        let tmp = format!("/tmp/axon_restore_{}", std::process::id());
        // DefaultHost should error on a nonexistent file (not the test host).
        assert!(with_host(|h| h.read_file(&tmp)).is_err());
        // But a file that DOES exist can be read.
        std::fs::write(&tmp, "real file").unwrap();
        assert_eq!(with_host(|h| h.read_file(&tmp)).unwrap(), "real file");
        let _ = std::fs::remove_file(&tmp);
    }

    /// After a scoped override drops, the thread-local host is DefaultHost
    /// again — no leakage across tests.
    #[test]
    fn scoped_restore_returns_to_default() {
        let test_host = TestHost::new();
        let pre_key = format!("PRESET_HOST_KEY_{}", std::process::id());

        // Set a real env var so DefaultHost can read it.
        std::env::set_var(&pre_key, "preset_value");

        with_host_override(Box::new(test_host), || {
            // Inside, env_var hits the test host (empty map).
            assert!(with_host(|h| h.env_var(&pre_key)).is_none());
        });

        // After restore, env_var goes to real std::env.
        assert_eq!(with_host(|h| h.env_var(&pre_key)).unwrap(), "preset_value");
    }

    /// Now_ms from DefaultHost is a reasonable epoch value.
    #[test]
    fn now_ms_returns_epoch_ms() {
        let h = DefaultHost;
        let ms = h.now_ms();
        // Should be roughly 1.7–1.8 trillion for 2026.
        assert!(ms > 1_700_000_000_000i64, "now_ms expected ~1.7T, got {ms}");
    }

    /// NESTED overrides must restore the OUTER host, not the global default.
    /// (Regression guard: an earlier guard reset unconditionally to DefaultHost,
    /// which would clobber an enclosing override — broken composition.)
    #[test]
    fn nested_override_restores_the_outer_host() {
        let outer = {
            let h = TestHost::new();
            h.files.lock().unwrap().insert("/k".into(), "OUTER".into());
            h
        };
        let inner = {
            let h = TestHost::new();
            h.files.lock().unwrap().insert("/k".into(), "INNER".into());
            h
        };
        with_host_override(Box::new(outer), || {
            assert_eq!(with_host(|h| h.read_file("/k")).unwrap(), "OUTER");
            with_host_override(Box::new(inner), || {
                assert_eq!(with_host(|h| h.read_file("/k")).unwrap(), "INNER");
            });
            // After the inner guard drops, the OUTER host must be active again
            // — not DefaultHost (which would error / hit the real fs).
            assert_eq!(
                with_host(|h| h.read_file("/k")).unwrap(),
                "OUTER",
                "inner override must restore the outer host, not the global default"
            );
        });
    }

    // ── SSE parsing tests ─────────────────────────────────────────────────

    #[test]
    fn sse_parses_two_simple_events() {
        let raw = b"data: hello\n\ndata: world\n\n";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert_eq!(events, vec!["hello", "world"]);
    }

    #[test]
    fn sse_joins_multiline_event_data() {
        let raw = b"data: line1\ndata: line2\n\ndata: single\n\n";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert_eq!(events, vec!["line1\nline2", "single"]);
    }

    #[test]
    fn sse_skips_event_id_retry_fields() {
        let raw = b"id: 1\nevent: update\nretry: 3000\ndata: payload\n\n";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert_eq!(events, vec!["payload"]);
    }

    #[test]
    fn sse_empty_stream_returns_no_events() {
        let raw = b"";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert!(events.is_empty());
    }

    #[test]
    fn sse_done_sentinel_is_preserved_verbatim() {
        // LLM providers end streams with `data: [DONE]` — callers filter this.
        let raw = b"data: {\"delta\":\"hi\"}\n\ndata: [DONE]\n\n";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert_eq!(events, vec!["{\"delta\":\"hi\"}", "[DONE]"]);
    }

    #[test]
    fn sse_trailing_data_without_blank_line_flushed() {
        // Stream closed without a final blank line — partial event still emitted.
        let raw = b"data: hello\n\ndata: trailing";
        let events = collect_sse_events(raw.as_ref()).unwrap();
        assert_eq!(events, vec!["hello", "trailing"]);
    }

    #[test]
    fn default_host_http_sse_denied_without_asi_runtime() {
        // Without the asi-runtime feature, the *trait default* returns Err.
        // With the feature enabled the DefaultHost override is active, so this
        // test only validates the deny path via a virtual (non-Default) host.
        struct DenyHost;
        impl AxonHost for DenyHost {
            fn read_file(&self, _: &str) -> Result<String, String> {
                Ok(String::new())
            }
            fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn env_var(&self, _: &str) -> Option<String> {
                None
            }
            fn now_ms(&self) -> i64 {
                0
            }
            fn sleep_ms(&self, _: u64) {}
            // http_sse intentionally NOT overridden — inherits the deny default.
        }
        let h = DenyHost;
        assert!(h.http_sse("http://example.com", "").is_err());
    }
}
