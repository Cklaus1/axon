//! Provenance logger for `@[adaptive]` functions.
//!
//! Each invocation of an `@[adaptive]` function appends one JSON line to
//! `~/.cache/axon/provenance.jsonl` (or `$XDG_CACHE_HOME/axon/...`) recording
//! the function name, an event tag (`"call"` / `"return"` / `"event"`), an
//! optional payload string, and a timestamp.  In addition to the on-disk log,
//! the runtime keeps an in-memory `Mutex<Vec<Record>>` of return-value
//! observations so `__axon_goal_run` can compute a best observed score
//! without re-parsing the JSONL file.
//!
//! ABI symbols exported here:
//! * `__axon_provenance_log(fn_name, payload)` — original Layer-1 entry point.
//!   Used by codegen at adaptive prologue / return sites with `"call"` /
//!   `"return"` payloads.  Backward-compatible: nothing about the on-disk
//!   format has changed.
//! * `__axon_provenance_log_ret_i64(fn_name, ret)` — Layer-2 addition.  Codegen
//!   calls this immediately before `build_return` in an `@[adaptive]` function
//!   whose return type is `i64`.  Records the return value into the in-memory
//!   log AND appends a JSON line with `"event":"return","score":<f64>`.
//! * `__axon_provenance_log_ret_f64(fn_name, ret)` — same, for `f64` returns.
//!
//! Reading the in-memory log:
//! * `provenance_records_for(name)` — Rust-side helper returning a snapshot of
//!   all records whose `fn_name` matches `name`.  Consumed by
//!   [`crate::goal::__axon_goal_run`].
//!
//! All filesystem errors are silently swallowed — this logger must never panic
//! in user code.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

// ── In-memory record store ────────────────────────────────────────────────────

/// A single recorded execution of an `@[adaptive]` function's return site.
///
/// Stored in a process-global `Mutex<Vec<Record>>`.  Read by `goal_run` to
/// compute a best observed score without re-parsing the JSONL log.
#[derive(Debug, Clone)]
pub struct Record {
    pub fn_name: String,
    /// Recorded return value, normalised to `f64` for scoring purposes.
    /// Integer returns are widened via `n as f64`.
    pub score: f64,
    /// Wall-clock timestamp when the record was captured (ms since epoch).
    pub ts_ms: u64,
    /// F11: the i64 input (first scalar arg) that produced this score, when the
    /// codegen logged it via `__axon_provenance_log_ret_i64_in`. `None` for the
    /// score-only entry points — so `goal_run` can warm-start its hill-climb
    /// from the best prior input instead of always cold-starting at 0.
    pub input: Option<i64>,
}

fn store() -> &'static Mutex<Vec<Record>> {
    use std::sync::OnceLock;
    static STORE: OnceLock<Mutex<Vec<Record>>> = OnceLock::new();
    STORE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Return a snapshot of all records whose `fn_name` exactly matches `name`.
///
/// The returned vector is independent of the global store; callers may iterate
/// freely without holding the global lock.
pub fn provenance_records_for(name: &str) -> Vec<Record> {
    let guard = match store().lock() {
        Ok(g)  => g,
        Err(p) => p.into_inner(), // recover from poisoning rather than panic
    };
    guard.iter().filter(|r| r.fn_name == name).cloned().collect()
}

fn record(fn_name: &str, score: f64) {
    record_with_input(fn_name, score, None);
}

/// F11: record a `(input, score)` observation (or score-only when `input` is
/// `None`). Single insertion point so the store stays consistent.
fn record_with_input(fn_name: &str, score: f64, input: Option<i64>) {
    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Ok(mut g) = store().lock() {
        g.push(Record { fn_name: fn_name.to_string(), score, ts_ms: ts, input });
    }
}

/// F11: the i64 input whose recorded score is closest to `target` for `name`,
/// among records that carry an input. `None` when no input-bearing record
/// exists — the caller (hill-climb) then cold-starts at 0 as before. Ties go to
/// the earliest record (stable, matches `best_observed`).
pub fn best_input_for(name: &str, target: f64) -> Option<i64> {
    let guard = match store().lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    let mut best: Option<(i64, f64)> = None; // (input, dist)
    for r in guard.iter().filter(|r| r.fn_name == name) {
        if let Some(inp) = r.input {
            let dist = (r.score - target).abs();
            match best {
                Some((_, bd)) if dist >= bd => {}
                _ => best = Some((inp, dist)),
            }
        }
    }
    best.map(|(inp, _)| inp)
}

// ── ABI: original event-style log (Layer 1) ───────────────────────────────────

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
    let _ = log_event(fn_name, payload, None);
}

// ── ABI: Layer-2 typed-return log ─────────────────────────────────────────────

/// Layer-2 entry point: log an `i64` return value from an adaptive function.
///
/// Records into the in-memory store AND appends a JSON line with
/// `"event":"return","score":<f64>`.  Codegen emits a call to this symbol
/// immediately before `build_return` whenever the enclosing function is
/// `@[adaptive]` and returns a value whose LLVM type is `i64`.
#[no_mangle]
pub extern "C" fn __axon_provenance_log_ret_i64(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    ret: i64,
) {
    let fn_name = slice_to_str(fn_name_ptr, fn_name_len);
    if !fn_name.is_empty() {
        record(fn_name, ret as f64);
    }
    let payload = format!("ret_i64={}", ret);
    // R4: emit the SAME discriminator the interpreter writes
    // (`event:"adaptive_return"`, `zone:"adaptive"`) so native and interp
    // provenance carry matching shape — the I-13 engine-parity guarantee.
    let _ = log_adaptive_return(fn_name, &payload, ret as f64);
}

/// F11 entry point: log an `i64` return value TOGETHER with the `i64` input
/// that produced it. Codegen emits a call to this (instead of the score-only
/// `__axon_provenance_log_ret_i64`) when the enclosing `@[adaptive] fn(i64,…)
/// -> i64` has a leading i64 parameter, so `goal_run` can warm-start its
/// hill-climb from the best prior input (F11) rather than always seeding at 0.
/// The JSONL line gains an `"input":<i64>` field; the in-memory record carries
/// the input for `best_input_for`.
#[no_mangle]
pub extern "C" fn __axon_provenance_log_ret_i64_in(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    input: i64,
    ret: i64,
) {
    let fn_name = slice_to_str(fn_name_ptr, fn_name_len);
    if !fn_name.is_empty() {
        record_with_input(fn_name, ret as f64, Some(input));
    }
    let payload = format!("ret_i64={} input={}", ret, input);
    let _ = log_adaptive_return(fn_name, &payload, ret as f64);
}

/// Layer-2 entry point: log an `f64` return value from an adaptive function.
#[no_mangle]
pub extern "C" fn __axon_provenance_log_ret_f64(
    fn_name_ptr: *const u8,
    fn_name_len: i64,
    ret: f64,
) {
    let fn_name = slice_to_str(fn_name_ptr, fn_name_len);
    if !fn_name.is_empty() {
        record(fn_name, ret);
    }
    let payload = format!("ret_f64={}", ret);
    let _ = log_adaptive_return(fn_name, &payload, ret);
}

// ── Internals ─────────────────────────────────────────────────────────────────

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
///
/// `score` is an optional numeric score that, when present, is included in the
/// JSON line as `"score":<f64>` so external tooling can read it without
/// re-parsing the payload.
fn log_event(fn_name: &str, payload: &str, score: Option<f64>) -> std::io::Result<()> {
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
    let line = match score {
        Some(s) => format!(
            "{{\"ts_ms\":{ts},\"fn\":{fn_q},\"event\":{ev_q},\"payload\":{pl_q},\"score\":{s}}}\n",
            ts    = ts,
            fn_q  = json_quote(fn_name),
            ev_q  = json_quote(event),
            pl_q  = json_quote(body),
            s     = format_f64(s),
        ),
        None => format!(
            "{{\"ts_ms\":{ts},\"fn\":{fn_q},\"event\":{ev_q},\"payload\":{pl_q}}}\n",
            ts    = ts,
            fn_q  = json_quote(fn_name),
            ev_q  = json_quote(event),
            pl_q  = json_quote(body),
        ),
    };

    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(line.as_bytes())?;
    Ok(())
}

/// R4 engine-parity: append an `@[adaptive]` return record whose shape matches
/// the interpreter's `append_provenance_jsonl` (interp.rs) — same `event` and
/// `zone` discriminator so native and interpreted runs of the same program
/// produce structurally identical provenance (I-13).  The interpreter also
/// stamps `input` (the call's first scalar arg) and `src` (the source path);
/// codegen does not thread those through the ABI yet, so they are omitted here
/// rather than faked — the discriminating fields (`event`/`zone`/`fn`/`score`)
/// are the parity contract.
fn log_adaptive_return(fn_name: &str, payload: &str, score: f64) -> std::io::Result<()> {
    let dir = provenance_dir().ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no cache dir")
    })?;
    fs::create_dir_all(&dir)?;
    let path = dir.join("provenance.jsonl");

    let ts = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);

    let line = format!(
        "{{\"ts_ms\":{ts},\"fn\":{fn_q},\"event\":\"adaptive_return\",\"zone\":\"adaptive\",\"payload\":{pl_q},\"score\":{s}}}\n",
        ts   = ts,
        fn_q = json_quote(fn_name),
        pl_q = json_quote(payload),
        s    = format_f64(score),
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

fn format_f64(x: f64) -> String {
    // JSON cannot encode NaN/Inf; coerce to 0 to keep the file parseable.
    if x.is_finite() {
        format!("{}", x)
    } else {
        "0".to_string()
    }
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
        // Redirect to a per-test tempdir.  XDG_CACHE_HOME is process-global,
        // so other parallel tests that also call log_event() will end up
        // writing to whichever value is set last.  We therefore filter the
        // resulting file for our own unique fn-name rather than asserting
        // line count.
        let tmp = std::env::temp_dir().join(format!("axon-prov-test-{}", std::process::id()));
        std::env::set_var("XDG_CACHE_HOME", &tmp);
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();

        let unique = format!("logtest_fn_{}", std::process::id());
        log_event(&unique, "call", None).unwrap();
        log_event(&unique, "return", None).unwrap();

        let path = tmp.join("axon").join("provenance.jsonl");
        let content = fs::read_to_string(&path).unwrap();
        let mine: Vec<&str> = content
            .lines()
            .filter(|l| l.contains(&format!("\"fn\":\"{unique}\"")))
            .collect();
        assert_eq!(mine.len(), 2, "expected 2 of our lines, got {}", mine.len());
        assert!(mine[0].contains("\"event\":\"call\""));
        assert!(mine[1].contains("\"event\":\"return\""));

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

    #[test]
    fn ret_i64_records_into_store() {
        // Use a per-test unique name so parallel tests don't interfere with
        // the global store.  The store is process-global; clearing it in one
        // test would race with the others.
        let name = b"prov_test_ret_i64_fn";
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 42);
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 17);
        let recs = provenance_records_for("prov_test_ret_i64_fn");
        assert!(recs.len() >= 2, "expected ≥2 records, got {}", recs.len());
        // Find the values we just inserted.
        assert!(recs.iter().any(|r| (r.score - 42.0).abs() < 1e-9));
        assert!(recs.iter().any(|r| (r.score - 17.0).abs() < 1e-9));
    }

    #[test]
    fn ret_f64_records_into_store() {
        let name = b"prov_test_ret_f64_fn";
        __axon_provenance_log_ret_f64(name.as_ptr(), name.len() as i64, 0.85);
        __axon_provenance_log_ret_f64(name.as_ptr(), name.len() as i64, 0.92);
        let recs = provenance_records_for("prov_test_ret_f64_fn");
        assert!(recs.len() >= 2, "expected ≥2 records, got {}", recs.len());
        assert!(recs.iter().any(|r| (r.score - 0.85).abs() < 1e-9));
        assert!(recs.iter().any(|r| (r.score - 0.92).abs() < 1e-9));
    }

    #[test]
    fn records_filter_by_name() {
        let a = b"prov_test_filter_a";
        let b = b"prov_test_filter_b";
        __axon_provenance_log_ret_i64(a.as_ptr(), a.len() as i64, 1);
        __axon_provenance_log_ret_i64(b.as_ptr(), b.len() as i64, 2);
        __axon_provenance_log_ret_i64(a.as_ptr(), a.len() as i64, 3);
        let foo = provenance_records_for("prov_test_filter_a");
        let bar = provenance_records_for("prov_test_filter_b");
        assert_eq!(foo.len(), 2);
        assert_eq!(bar.len(), 1);
    }

    #[test]
    fn ret_i64_with_null_name_does_not_panic() {
        // Defence-in-depth — codegen should never pass null, but if it did we
        // must not panic because that would tear down user code.
        __axon_provenance_log_ret_i64(std::ptr::null(), 0, 99);
    }

    // ── F11: (input, score) logging + best_input_for ──────────────────────────
    #[test]
    fn log_ret_in_records_input_and_score() {
        let name = b"f11_test_in_basic";
        // (input=10 -> score=100), (input=20 -> score=400)
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 10, 100);
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 20, 400);
        let recs = provenance_records_for("f11_test_in_basic");
        assert!(recs.len() >= 2);
        assert!(recs.iter().any(|r| r.input == Some(10) && (r.score - 100.0).abs() < 1e-9));
        assert!(recs.iter().any(|r| r.input == Some(20) && (r.score - 400.0).abs() < 1e-9));
    }

    #[test]
    fn best_input_for_picks_input_of_closest_score() {
        let name = b"f11_test_best_input";
        // scores 100/400/225 at inputs 10/20/15; target 225 → input 15 wins.
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 10, 100);
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 20, 400);
        __axon_provenance_log_ret_i64_in(name.as_ptr(), name.len() as i64, 15, 225);
        assert_eq!(best_input_for("f11_test_best_input", 225.0), Some(15));
    }

    #[test]
    fn best_input_for_is_none_without_input_records() {
        // Score-only entry point records no input → no warm-start seed.
        let name = b"f11_test_no_input";
        __axon_provenance_log_ret_i64(name.as_ptr(), name.len() as i64, 42);
        assert_eq!(best_input_for("f11_test_no_input", 42.0), None);
    }
}
