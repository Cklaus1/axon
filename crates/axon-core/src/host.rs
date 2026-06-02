//! The interpreter's host-interface seam — `AxonHost`.
//!
//! Five std touchpoints (fs / env / time / sleep) live behind a single trait
//! so a browser or wasm build can supply a virtual implementation without
//! threading a parameter through the interpreter's call chain. On native the
//! active host is `DefaultHost` which wraps std exactly as today, so native
//! behavior is byte-identical.

use std::cell::RefCell;
use std::time::{SystemTime, UNIX_EPOCH};

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

    fn now_ms(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0)
    }

    fn sleep_ms(&self, ms: u64) {
        std::thread::sleep(std::time::Duration::from_millis(ms));
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
        env:   std::sync::Arc<Mutex<HashMap<String, String>>>,
        now:   std::sync::Arc<Mutex<Option<i64>>>,
    }

    impl TestHost {
        fn new() -> Self {
            Self {
                files: std::sync::Arc::new(Mutex::new(HashMap::new())),
                env:   std::sync::Arc::new(Mutex::new(HashMap::new())),
                now:   std::sync::Arc::new(Mutex::new(None)),
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
            self.files.lock().unwrap().insert(path.to_string(), data.to_string());
            Ok(())
        }

        fn env_var(&self, key: &str) -> Option<String> {
            self.env.lock().unwrap().get(key).cloned()
        }

        fn now_ms(&self) -> i64 {
            let mut guard = self.now.lock().unwrap();
            if guard.is_none() {
                *guard = Some(SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0));
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
        host.env.lock().unwrap().insert("MY_KEY".into(), "my_val".into());

        with_host_override(Box::new(host.clone()), || {
            // write_file goes to the in-memory map.
            assert!(with_host(|h| h.write_file("/a", "aaa")).is_ok());

            // read_file reads from the in-memory map.
            assert_eq!(
                with_host(|h| h.read_file("/a")).unwrap(),
                "aaa"
            );

            // A path not in the map errors.
            assert!(with_host(|h| h.read_file("/notfound")).is_err());

            // env_var is intercepted.
            assert_eq!(
                with_host(|h| h.env_var("MY_KEY")).unwrap(),
                "my_val"
            );
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
        assert_eq!(
            with_host(|h| h.env_var(&pre_key)).unwrap(),
            "preset_value"
        );
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
}