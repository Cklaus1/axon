//! Record and replay every host interaction — `RecordingHost` / `ReplayHost`.
//!
//! # Why this exists
//!
//! `AXON_SEED` pins entropy, `AXON_CLOCK` pins time, and `AXON_AI_REPLAY` pins
//! model calls. Each closes one nondeterminism source with its own bespoke
//! cache, and each was added only after someone noticed the hole — the typed
//! `ai_extract_uncertain_*` builtins bypassed the AI cache entirely for months
//! because a per-builtin mechanism has to be remembered at every call site.
//!
//! Everything else a program can observe about its environment — stdin, file
//! contents, directory listings, HTTP responses, subprocess output, whether a
//! path exists — was NOT reproducible at all. A run that read a file replayed
//! only if that file still held the same bytes.
//!
//! The fix is structural rather than per-builtin: every environmental effect
//! already passes through exactly one trait, [`crate::host::AxonHost`]. Wrapping
//! it once closes the whole column and cannot be forgotten at a call site,
//! because a new builtin that wants to touch the world has nowhere else to go.
//!
//! # Partial replay is worse than no replay
//!
//! A replayed run that quietly consults live state produces an authoritative
//! -looking transcript of something that never happened, and an auditor
//! reviewing it is being misled with extra confidence. So `ReplayHost` REFUSES
//! on any miss — an unrecorded method, an argument mismatch, or a journal that
//! ran out — and refuses in a way the program under replay cannot swallow (see
//! [`divergence`]).
//!
//! # Replay does not touch the world
//!
//! On replay, `write_file`/`exec`/`http_post` return their RECORDED outcome
//! without performing anything. Re-executing the side effects of an agent run
//! in order to study it would be its own kind of accident.
//!
//! # A journal is as sensitive as the run it records
//!
//! It contains, verbatim, every file the program read, every HTTP response body,
//! every subprocess's stdout, every line of stdin, and the VALUE of every env var
//! it looked up. If the program read an API key, the key is in the journal. The
//! hex encoding is a delimiter-safety measure, NOT encryption — `xxd -r` reverses
//! it.
//!
//! This is inherent: a record faithful enough to reproduce a run is by definition
//! a record of everything the run saw. It is called out because the natural
//! instinct with an audit artifact is to attach it to a ticket. Treat a journal
//! with the same care as the secrets its run touched. (`@[contained]` denies
//! `env_var` outright for this family of reasons — reading a secret through an
//! ambient channel — but an unconstrained program has no such guard.)
//!
//! # What is deliberately NOT journaled
//!
//! - **The provenance log itself** (`interp/provenance.rs` uses `std::fs`
//!   directly). It is the recorder; routing it through the host would make
//!   recording recursive.
//! - **`now_ms`, when `AXON_CLOCK` is set.** `program_now_ms` consults the
//!   virtual clock BEFORE the host, so the clock wins and the journal never
//!   sees the call. Two mechanisms answering the same question with different
//!   answers is precisely the `temporal_*` two-timelines bug; the precedence is
//!   fixed and tested rather than left to chance.

use crate::host::{AxonHost, SharedHost};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Process exit code when a replay diverges from its journal.
///
/// Distinct from 101 (crash) and from the whole enforcement family — 8 sandbox,
/// 7 goal-budget, 6 refinement, 5 ai-policy, 4 kill-switch, 3 verify, 2 static —
/// so a supervisor can branch on "this replay is not the run it claims to be"
/// specifically. 9/10/12/13/14/15 are already claimed by `axon-os` and
/// `axon-vm` (resource-bound, coalition-bound, containment, quorum, chain), so
/// **11** is the free slot; verified against the exit-code table.
pub const REPLAY_DIVERGENCE_EXIT_CODE: i32 = 11;

/// `AXON_RECORD=<path>` — record this run's host interactions to `<path>`.
pub const RECORD_ENV_VAR: &str = "AXON_RECORD";
/// `AXON_REPLAY=<path>` — serve this run's host interactions from `<path>`.
pub const REPLAY_ENV_VAR: &str = "AXON_REPLAY";

// ── The journal ──────────────────────────────────────────────────────────────

/// How a host call turned out. One uniform shape for every return type in the
/// trait, so adding a trait method needs no new encoding:
///
/// | trait return              | status  | payload             |
/// |---------------------------|---------|---------------------|
/// | `Result<String, String>`  | ok/err  | `[value]`/`[error]` |
/// | `Result<(), String>`      | ok/err  | `[]`/`[error]`      |
/// | `Result<Vec<String>, _>`  | ok/err  | the items/`[error]` |
/// | `Option<String>`          | some/none | `[value]`/`[]`    |
/// | `i64` / `bool`            | val     | `[rendered]`        |
/// | `()`                      | val     | `[]`                |
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HostEvent {
    /// 0-based position. Redundant with file order and that is the point: it
    /// makes a hand-edited or concatenated journal detectable.
    pub seq: usize,
    pub method: String,
    pub args: Vec<String>,
    pub status: String,
    pub payload: Vec<String>,
}

/// Encode one variable-length field.
///
/// The leading `x` is not decoration: an empty string hex-encodes to the empty
/// string, which would make the field VANISH under any trailing-whitespace strip
/// (an editor, a copy-paste, a `sed`) and silently corrupt a journal that is
/// meant to be an audit artifact. With the marker, every field is at least one
/// character and a stripped line fails to parse loudly instead.
fn encode_field(s: &str) -> String {
    format!("x{}", hex_encode(s.as_bytes()))
}

fn decode_field(f: &str) -> Option<String> {
    String::from_utf8(hex_decode(f.strip_prefix('x')?)?).ok()
}

fn hex_encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(s.len() / 2);
    let mut i = 0;
    while i < b.len() {
        let hi = (b[i] as char).to_digit(16)?;
        let lo = (b[i + 1] as char).to_digit(16)?;
        out.push((hi * 16 + lo) as u8);
        i += 2;
    }
    Some(out)
}

impl HostEvent {
    /// One line: `<seq> <method> <status> <nargs> <arg-hex>… <npayload> <pay-hex>…`
    ///
    /// Every variable-length field is hex, following the `AXON_AI_REPLAY` cache's
    /// precedent: a file body or an HTTP response containing spaces, newlines or
    /// quotes then needs no escaping, so there is no escaping bug to have. The
    /// counts make the line self-delimiting rather than relying on field position.
    pub fn to_line(&self) -> String {
        let mut s = format!(
            "{} {} {} {}",
            self.seq,
            self.method,
            self.status,
            self.args.len()
        );
        for a in &self.args {
            s.push(' ');
            s.push_str(&encode_field(a));
        }
        s.push_str(&format!(" {}", self.payload.len()));
        for p in &self.payload {
            s.push(' ');
            s.push_str(&encode_field(p));
        }
        s.push('\n');
        s
    }

    /// Parse a line. `None` on ANY malformation — a truncated or corrupt journal
    /// must not silently replay as a shorter one (that would present a partial
    /// run as complete), so callers treat `None` as fatal rather than skipping
    /// the line. This is the opposite of the provenance reader's
    /// skip-malformed-lines policy, because provenance is advisory and a replay
    /// journal is load-bearing.
    pub fn from_line(line: &str) -> Option<Self> {
        let mut it = line.split(' ');
        let seq: usize = it.next()?.parse().ok()?;
        let method = it.next()?.to_string();
        let status = it.next()?.to_string();
        let nargs: usize = it.next()?.parse().ok()?;
        let mut args = Vec::with_capacity(nargs);
        for _ in 0..nargs {
            args.push(decode_field(it.next()?)?);
        }
        let npay: usize = it.next()?.parse().ok()?;
        let mut payload = Vec::with_capacity(npay);
        for _ in 0..npay {
            payload.push(decode_field(it.next()?)?);
        }
        // A trailing field means the writer and reader disagree about the format.
        if it.next().is_some() {
            return None;
        }
        Some(HostEvent {
            seq,
            method,
            args,
            status,
            payload,
        })
    }
}

/// Read a journal file. `Err` on a corrupt line, naming it — never a silent
/// short read.
pub fn read_journal(path: &std::path::Path) -> Result<Vec<HostEvent>, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read replay journal {}: {e}", path.display()))?;
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let ev = HostEvent::from_line(line).ok_or_else(|| {
            format!(
                "replay journal {} line {} is malformed — refusing to replay a \
                 partial journal (delete it and re-record rather than trusting it)",
                path.display(),
                i + 1
            )
        })?;
        if ev.seq != out.len() {
            return Err(format!(
                "replay journal {} line {}: seq {} out of order (expected {}) — \
                 the journal has been edited or two runs were concatenated",
                path.display(),
                i + 1,
                ev.seq,
                out.len()
            ));
        }
        out.push(ev);
    }
    Ok(out)
}

// ── RecordingHost ────────────────────────────────────────────────────────────

/// Wraps an inner host, performing every call for real and appending the outcome
/// to a journal.
pub struct RecordingHost {
    inner: SharedHost,
    path: std::path::PathBuf,
    /// Serialises append + counter so a concurrent program (`spawn`) still
    /// produces a journal whose seq numbers are dense and whose lines are whole.
    state: Mutex<usize>,
}

impl RecordingHost {
    /// Record to `path`, delegating to `inner`. Truncates `path` — a journal is
    /// one run, and appending a second run to it would produce exactly the
    /// concatenated-journal corruption `read_journal` rejects.
    pub fn new(inner: SharedHost, path: impl Into<std::path::PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if let Some(dir) = path.parent() {
            if !dir.as_os_str().is_empty() {
                std::fs::create_dir_all(dir)
                    .map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
            }
        }
        std::fs::write(&path, "")
            .map_err(|e| format!("cannot open journal {}: {e}", path.display()))?;
        Ok(Self {
            inner,
            path,
            state: Mutex::new(0),
        })
    }

    /// Append one event. Flushed per call, deliberately: the run you most want
    /// to replay is the one that crashed, so the journal has to survive a panic
    /// or a `process::exit`, not be buffered until a clean shutdown.
    fn record(&self, method: &str, args: Vec<String>, status: &str, payload: Vec<String>) {
        use std::io::Write as _;
        let mut seq = self.state.lock().unwrap_or_else(|p| p.into_inner());
        let ev = HostEvent {
            seq: *seq,
            method: method.to_string(),
            args,
            status: status.to_string(),
            payload,
        };
        *seq += 1;
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&self.path) {
            let _ = f.write_all(ev.to_line().as_bytes());
            let _ = f.flush();
        }
    }

    fn rec_res_str(
        &self,
        m: &str,
        args: Vec<String>,
        r: Result<String, String>,
    ) -> Result<String, String> {
        match &r {
            Ok(v) => self.record(m, args, "ok", vec![v.clone()]),
            Err(e) => self.record(m, args, "err", vec![e.clone()]),
        }
        r
    }

    fn rec_res_unit(
        &self,
        m: &str,
        args: Vec<String>,
        r: Result<(), String>,
    ) -> Result<(), String> {
        match &r {
            Ok(()) => self.record(m, args, "ok", vec![]),
            Err(e) => self.record(m, args, "err", vec![e.clone()]),
        }
        r
    }

    fn rec_res_list(
        &self,
        m: &str,
        args: Vec<String>,
        r: Result<Vec<String>, String>,
    ) -> Result<Vec<String>, String> {
        match &r {
            Ok(v) => self.record(m, args, "ok", v.clone()),
            Err(e) => self.record(m, args, "err", vec![e.clone()]),
        }
        r
    }
}

impl AxonHost for RecordingHost {
    fn read_file(&self, path: &str) -> Result<String, String> {
        let r = self.inner.read_file(path);
        self.rec_res_str("read_file", vec![path.to_string()], r)
    }

    fn write_file(&self, path: &str, data: &str) -> Result<(), String> {
        let r = self.inner.write_file(path, data);
        // `data` is journaled as an argument: a replay must be able to CHECK that
        // the same bytes were offered, not merely that a write happened.
        self.rec_res_unit("write_file", vec![path.to_string(), data.to_string()], r)
    }

    fn env_var(&self, key: &str) -> Option<String> {
        let r = self.inner.env_var(key);
        match &r {
            Some(v) => self.record("env_var", vec![key.to_string()], "some", vec![v.clone()]),
            None => self.record("env_var", vec![key.to_string()], "none", vec![]),
        }
        r
    }

    fn now_ms(&self) -> i64 {
        let t = self.inner.now_ms();
        self.record("now_ms", vec![], "val", vec![t.to_string()]);
        t
    }

    fn sleep_ms(&self, ms: u64) {
        self.inner.sleep_ms(ms);
        self.record("sleep_ms", vec![ms.to_string()], "val", vec![]);
    }

    fn read_line(&self) -> Result<String, String> {
        let r = self.inner.read_line();
        self.rec_res_str("read_line", vec![], r)
    }

    fn file_exists(&self, path: &str) -> bool {
        let b = self.inner.file_exists(path);
        self.record(
            "file_exists",
            vec![path.to_string()],
            "val",
            vec![b.to_string()],
        );
        b
    }

    fn dir_create(&self, path: &str) -> Result<(), String> {
        let r = self.inner.dir_create(path);
        self.rec_res_unit("dir_create", vec![path.to_string()], r)
    }

    fn dir_list(&self, path: &str) -> Result<Vec<String>, String> {
        let r = self.inner.dir_list(path);
        self.rec_res_list("dir_list", vec![path.to_string()], r)
    }

    fn file_copy(&self, from: &str, to: &str) -> Result<(), String> {
        let r = self.inner.file_copy(from, to);
        self.rec_res_unit("file_copy", vec![from.to_string(), to.to_string()], r)
    }

    fn file_rename(&self, from: &str, to: &str) -> Result<(), String> {
        let r = self.inner.file_rename(from, to);
        self.rec_res_unit("file_rename", vec![from.to_string(), to.to_string()], r)
    }

    fn exec(&self, cmd: &str, args: &[String]) -> Result<String, String> {
        let r = self.inner.exec(cmd, args);
        let mut key = vec![cmd.to_string()];
        key.extend(args.iter().cloned());
        self.rec_res_str("exec", key, r)
    }

    fn http_get(&self, url: &str, headers: &str) -> Result<String, String> {
        let r = self.inner.http_get(url, headers);
        self.rec_res_str("http_get", vec![url.to_string(), headers.to_string()], r)
    }

    fn http_post(&self, url: &str, headers: &str, body: &str) -> Result<String, String> {
        let r = self.inner.http_post(url, headers, body);
        self.rec_res_str(
            "http_post",
            vec![url.to_string(), headers.to_string(), body.to_string()],
            r,
        )
    }

    fn http_sse(&self, url: &str, headers: &str) -> Result<Vec<String>, String> {
        let r = self.inner.http_sse(url, headers);
        self.rec_res_list("http_sse", vec![url.to_string(), headers.to_string()], r)
    }

    fn http_sse_post(&self, url: &str, headers: &str, body: &str) -> Result<Vec<String>, String> {
        let r = self.inner.http_sse_post(url, headers, body);
        self.rec_res_list(
            "http_sse_post",
            vec![url.to_string(), headers.to_string(), body.to_string()],
            r,
        )
    }
}

// ── Divergence reporting ─────────────────────────────────────────────────────

/// The first divergence, latched. `OnceLock`-like semantics via a flag so the
/// FIRST divergence is the one reported: later calls also diverge (the journal
/// is misaligned from here on), and the tenth mismatch is noise while the first
/// is the answer.
static DIVERGED: AtomicBool = AtomicBool::new(false);
static DIVERGENCE: Mutex<Option<String>> = Mutex::new(None);

/// Record a divergence and return the message.
///
/// The report also goes to stderr immediately. A `ReplayHost` method can only
/// return the trait's own `Result`/`Option`/scalar, and a program is free to
/// `match … Err(e) => …` and carry on — so returning an error is NOT by itself
/// a loud failure. The un-swallowable half is [`divergence`]: the CLI consults
/// it after the program ends and forces exit
/// [`REPLAY_DIVERGENCE_EXIT_CODE`] no matter how the program behaved. A run
/// that catches the error and prints "all good" still exits 11.
fn diverge(detail: String) -> String {
    if !DIVERGED.swap(true, Ordering::SeqCst) {
        // Printed the moment it happens, not at exit: the run being replayed may
        // be one that crashes, and the divergence point is the thing worth having.
        eprintln!("axon: replay divergence: {detail}");
        *DIVERGENCE.lock().unwrap_or_else(|p| p.into_inner()) = Some(detail);
    }
    // The message handed BACK to the program is deliberately terse. The full
    // report is an operator-facing artifact; returning it here would splice a
    // paragraph of harness diagnostics into the program's own output (a program
    // that prints its `Err` would interleave them), which makes the transcript
    // harder to read at exactly the moment it matters.
    "replay diverged from the journal — see stderr; this run exits 11".to_string()
}

/// The first divergence detected during a replay, if any. The CLI checks this
/// after the program finishes and overrides the exit code.
pub fn divergence() -> Option<String> {
    DIVERGENCE.lock().unwrap_or_else(|p| p.into_inner()).clone()
}

/// Clear the latch. For tests, which run many replays in one process.
pub fn reset_divergence_for_tests() {
    DIVERGED.store(false, Ordering::SeqCst);
    *DIVERGENCE.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

// ── ReplayHost ───────────────────────────────────────────────────────────────

/// Serves host calls from a recorded journal, in order, performing nothing.
pub struct ReplayHost {
    events: Vec<HostEvent>,
    cursor: AtomicUsize,
}

impl ReplayHost {
    pub fn new(events: Vec<HostEvent>) -> Self {
        Self {
            events,
            cursor: AtomicUsize::new(0),
        }
    }

    pub fn from_path(path: &std::path::Path) -> Result<Self, String> {
        Ok(Self::new(read_journal(path)?))
    }

    /// How many recorded events were not consumed. A replay that stops early is
    /// as much a divergence as one that asks for too much — it just cannot be
    /// noticed from inside, so the CLI checks it at the end.
    pub fn unconsumed(&self) -> usize {
        self.events
            .len()
            .saturating_sub(self.cursor.load(Ordering::SeqCst))
    }

    /// Take the next event, requiring it to be `method` with `args`.
    ///
    /// Matching is SEQUENTIAL, not keyed. Keyed lookup (the `AXON_AI_REPLAY`
    /// approach) cannot tell a faithful replay from a program that made the same
    /// calls in a different order or a different number of times. Sequential
    /// matching makes the first mismatch the exact point where this run stopped
    /// being the recorded one — which is the question an auditor actually has,
    /// and it is free.
    fn next_event(&self, method: &str, args: &[String]) -> Result<&HostEvent, String> {
        let i = self.cursor.fetch_add(1, Ordering::SeqCst);
        let Some(ev) = self.events.get(i) else {
            return Err(diverge(format!(
                "at event {i} the program called `{}({})`, but the journal has only {} \
                 events — this run does more than the recorded one",
                method,
                args.join(", "),
                self.events.len()
            )));
        };
        if ev.method != method {
            return Err(diverge(format!(
                "at event {i} the program called `{}`, but the recorded run called `{}` \
                 — this is the first point at which the two runs differ",
                method, ev.method
            )));
        }
        if ev.args != args {
            return Err(diverge(format!(
                "at event {i} `{}` was called with [{}], but the recorded run passed [{}] \
                 — this is the first point at which the two runs differ",
                method,
                args.join(", "),
                ev.args.join(", ")
            )));
        }
        Ok(ev)
    }

    /// Rebuild a `Result<String, String>` from a matched event.
    fn as_res_str(ev: &HostEvent, method: &str) -> Result<String, String> {
        match ev.status.as_str() {
            "ok" => Ok(ev.payload.first().cloned().unwrap_or_default()),
            "err" => Err(ev.payload.first().cloned().unwrap_or_default()),
            other => Err(diverge(format!(
                "event {} (`{method}`) has status `{other}`, which is not valid for this \
                 method — the journal does not match this build",
                ev.seq
            ))),
        }
    }

    fn as_res_unit(ev: &HostEvent, method: &str) -> Result<(), String> {
        match ev.status.as_str() {
            "ok" => Ok(()),
            "err" => Err(ev.payload.first().cloned().unwrap_or_default()),
            other => Err(diverge(format!(
                "event {} (`{method}`) has status `{other}`, which is not valid for this \
                 method — the journal does not match this build",
                ev.seq
            ))),
        }
    }

    fn as_res_list(ev: &HostEvent, method: &str) -> Result<Vec<String>, String> {
        match ev.status.as_str() {
            "ok" => Ok(ev.payload.clone()),
            "err" => Err(ev.payload.first().cloned().unwrap_or_default()),
            other => Err(diverge(format!(
                "event {} (`{method}`) has status `{other}`, which is not valid for this \
                 method — the journal does not match this build",
                ev.seq
            ))),
        }
    }
}

impl AxonHost for ReplayHost {
    fn read_file(&self, path: &str) -> Result<String, String> {
        let ev = self.next_event("read_file", &[path.to_string()])?;
        Self::as_res_str(ev, "read_file")
    }

    fn write_file(&self, path: &str, data: &str) -> Result<(), String> {
        // Nothing is written. Replaying an agent run must not re-perform its
        // effects; the recorded outcome is the answer.
        let ev = self.next_event("write_file", &[path.to_string(), data.to_string()])?;
        Self::as_res_unit(ev, "write_file")
    }

    fn env_var(&self, key: &str) -> Option<String> {
        let Ok(ev) = self.next_event("env_var", &[key.to_string()]) else {
            // A divergence is already latched and reported. `None` is the
            // fail-closed answer: it cannot be mistaken for a real value, and
            // the forced exit code does not depend on what the program does next.
            return None;
        };
        match ev.status.as_str() {
            "some" => Some(ev.payload.first().cloned().unwrap_or_default()),
            "none" => None,
            other => {
                diverge(format!(
                    "event {} (`env_var`) has status `{other}` — the journal does not \
                     match this build",
                    ev.seq
                ));
                None
            }
        }
    }

    fn now_ms(&self) -> i64 {
        let Ok(ev) = self.next_event("now_ms", &[]) else {
            return 0;
        };
        ev.payload
            .first()
            .and_then(|s| s.parse().ok())
            .unwrap_or_else(|| {
                diverge(format!("event {} (`now_ms`) has no i64 payload", ev.seq));
                0
            })
    }

    fn sleep_ms(&self, ms: u64) {
        // Recorded, so a mismatched sleep is still a divergence; nothing sleeps.
        let _ = self.next_event("sleep_ms", &[ms.to_string()]);
    }

    fn read_line(&self) -> Result<String, String> {
        let ev = self.next_event("read_line", &[])?;
        Self::as_res_str(ev, "read_line")
    }

    fn file_exists(&self, path: &str) -> bool {
        let Ok(ev) = self.next_event("file_exists", &[path.to_string()]) else {
            return false;
        };
        ev.payload.first().map(|s| s == "true").unwrap_or(false)
    }

    fn dir_create(&self, path: &str) -> Result<(), String> {
        let ev = self.next_event("dir_create", &[path.to_string()])?;
        Self::as_res_unit(ev, "dir_create")
    }

    fn dir_list(&self, path: &str) -> Result<Vec<String>, String> {
        let ev = self.next_event("dir_list", &[path.to_string()])?;
        Self::as_res_list(ev, "dir_list")
    }

    fn file_copy(&self, from: &str, to: &str) -> Result<(), String> {
        let ev = self.next_event("file_copy", &[from.to_string(), to.to_string()])?;
        Self::as_res_unit(ev, "file_copy")
    }

    fn file_rename(&self, from: &str, to: &str) -> Result<(), String> {
        let ev = self.next_event("file_rename", &[from.to_string(), to.to_string()])?;
        Self::as_res_unit(ev, "file_rename")
    }

    fn exec(&self, cmd: &str, args: &[String]) -> Result<String, String> {
        // Nothing is spawned.
        let mut key = vec![cmd.to_string()];
        key.extend(args.iter().cloned());
        let ev = self.next_event("exec", &key)?;
        Self::as_res_str(ev, "exec")
    }

    fn http_get(&self, url: &str, headers: &str) -> Result<String, String> {
        let ev = self.next_event("http_get", &[url.to_string(), headers.to_string()])?;
        Self::as_res_str(ev, "http_get")
    }

    fn http_post(&self, url: &str, headers: &str, body: &str) -> Result<String, String> {
        // No request is sent.
        let ev = self.next_event(
            "http_post",
            &[url.to_string(), headers.to_string(), body.to_string()],
        )?;
        Self::as_res_str(ev, "http_post")
    }

    fn http_sse(&self, url: &str, headers: &str) -> Result<Vec<String>, String> {
        let ev = self.next_event("http_sse", &[url.to_string(), headers.to_string()])?;
        Self::as_res_list(ev, "http_sse")
    }

    fn http_sse_post(&self, url: &str, headers: &str, body: &str) -> Result<Vec<String>, String> {
        let ev = self.next_event(
            "http_sse_post",
            &[url.to_string(), headers.to_string(), body.to_string()],
        )?;
        Self::as_res_list(ev, "http_sse_post")
    }
}

// ── Human-readable transcript ────────────────────────────────────────────────
//
// WHY THIS EXISTS, AND WHY IT IS NOT COSMETIC.
//
// A journal is hex. It is exactly what a replaying engine needs and exactly what
// a PERSON cannot read — so the machine half of auditability was built and the
// human half was not. "The run is reproducible" is worth little to a reviewer who
// cannot see what the run DID; they end up trusting the agent's own account of
// itself, which is the thing an audit exists to avoid.
//
// So the transcript is the reviewable artifact: one line per interaction, in
// order, in plain language. It answers "what did this agent touch?" without
// running anything.

/// How much of a payload to show inline before truncating. A file's contents can
/// be megabytes; a transcript that dumps them is not reviewable either.
const PREVIEW_LEN: usize = 68;

/// Options for rendering a transcript.
#[derive(Default)]
pub struct RenderOpts {
    /// Show payload VALUES. **Defaults to `false`, and that is a security
    /// decision rather than a formatting preference.**
    ///
    /// A journal contains every file the run read, every HTTP body, and the VALUE
    /// of every env var it looked up — including an API key, if the program read
    /// one. The transcript's whole purpose is to be handed to a reviewer, and a
    /// format that leaks credentials by default is a format people learn not to
    /// share, which defeats the purpose.
    ///
    /// Redacted rendering still shows each payload's size and a content digest, so
    /// a reviewer can see the SHAPE of what happened and can still tell two runs
    /// apart — which is what makes a redacted diff useful instead of merely safe.
    /// `--show-values` is there for when the payloads themselves are the point.
    pub show_values: bool,
}

fn digest8(s: &str) -> String {
    // Short content fingerprint: enough to tell "same bytes" from "different
    // bytes" at a glance without disclosing the bytes.
    crate::interp::sha256_hex(s).chars().take(8).collect()
}

fn preview(s: &str, opts: &RenderOpts) -> String {
    if !opts.show_values {
        return format!("<{} bytes, {}>", s.len(), digest8(s));
    }
    let one_line: String = s.chars().map(|c| if c == '\n' { '⏎' } else { c }).collect();
    if one_line.chars().count() <= PREVIEW_LEN {
        format!("{one_line:?}")
    } else {
        let head: String = one_line.chars().take(PREVIEW_LEN).collect();
        format!("{head:?}… ({} bytes total)", s.len())
    }
}

impl HostEvent {
    /// One human-readable line: what was asked, and what came back.
    pub fn describe(&self, opts: &RenderOpts) -> String {
        let arg = |i: usize| self.args.get(i).cloned().unwrap_or_default();
        let pay0 = self.payload.first().cloned().unwrap_or_default();
        let failed = self.status == "err";
        // The outcome half. An `err` is rendered with its message even when values
        // are redacted: an error string is what a reviewer most needs and is not
        // the channel secrets travel on.
        let outcome = match (self.method.as_str(), self.status.as_str()) {
            (_, "err") => format!("FAILED: {pay0}"),
            ("env_var", "none") => "not set".to_string(),
            ("env_var", "some") => format!("= {}", preview(&pay0, opts)),
            ("file_exists", _) => {
                if pay0 == "true" {
                    "exists".to_string()
                } else {
                    "does not exist".to_string()
                }
            }
            ("now_ms", _) => format!("= {pay0}"),
            ("sleep_ms", _) => "ok".to_string(),
            ("dir_list" | "http_sse" | "http_sse_post", _) => {
                format!("{} item(s)", self.payload.len())
            }
            ("write_file" | "dir_create" | "file_copy" | "file_rename", _) => "ok".to_string(),
            _ => format!("→ {}", preview(&pay0, opts)),
        };
        let action = match self.method.as_str() {
            "read_file" => format!("read file {}", arg(0)),
            "write_file" => format!("WRITE file {} ({})", arg(0), preview(&arg(1), opts)),
            "read_line" => "read a line from stdin".to_string(),
            "env_var" => format!("read env {}", arg(0)),
            "now_ms" => "read the clock".to_string(),
            "sleep_ms" => format!("slept {}ms", arg(0)),
            "file_exists" => format!("checked whether {} exists", arg(0)),
            "dir_create" => format!("CREATE dir {}", arg(0)),
            "dir_list" => format!("listed dir {}", arg(0)),
            "file_copy" => format!("COPY {} → {}", arg(0), arg(1)),
            "file_rename" => format!("MOVE {} → {}", arg(0), arg(1)),
            "exec" => format!("EXEC `{}` {:?}", arg(0), &self.args[1..]),
            "http_get" => format!("HTTP GET {}", arg(0)),
            "http_post" => format!("HTTP POST {} ({})", arg(0), preview(&arg(2), opts)),
            "http_sse" => format!("HTTP STREAM {}", arg(0)),
            "http_sse_post" => format!("HTTP STREAM-POST {}", arg(0)),
            other => format!("{other}({})", self.args.join(", ")),
        };
        // A leading marker so the effects that CHANGE the world, and the ones that
        // failed, are findable by eye in a long transcript rather than having to
        // be read for.
        let marker = if failed {
            "!"
        } else if matches!(
            self.method.as_str(),
            "write_file" | "exec" | "http_post" | "dir_create" | "file_copy" | "file_rename"
        ) {
            "*"
        } else {
            " "
        };
        format!("{marker} {:>4}  {action}  {outcome}", self.seq)
    }
}

/// Render a whole journal as a reviewable transcript, with a summary.
///
/// Values are REDACTED unless `opts.show_values` — see [`RenderOpts`].
pub fn render_transcript(events: &[HostEvent], opts: &RenderOpts) -> String {
    let mut out = String::new();
    out.push_str(&format!("{} host interaction(s)\n\n", events.len()));
    if !opts.show_values {
        out.push_str(
            "  values redacted (sizes + digests shown); pass --show-values to see them\n\n",
        );
    }
    out.push_str("  *=changes the world  !=failed\n\n");
    for ev in events {
        out.push_str(&ev.describe(opts));
        out.push('\n');
    }
    // The summary is what a reviewer reads FIRST: the question is usually "did
    // this run touch anything it should not have", not "what was event 34".
    let mut mutating: Vec<&str> = Vec::new();
    let mut failures = 0usize;
    let mut reads: Vec<&str> = Vec::new();
    let mut net: Vec<&str> = Vec::new();
    for ev in events {
        if ev.status == "err" {
            failures += 1;
        }
        match ev.method.as_str() {
            "write_file" | "dir_create" | "file_copy" | "file_rename" => {
                mutating.push(ev.args.first().map(String::as_str).unwrap_or(""))
            }
            "exec" => mutating.push(ev.args.first().map(String::as_str).unwrap_or("")),
            "read_file" => reads.push(ev.args.first().map(String::as_str).unwrap_or("")),
            "http_get" | "http_post" | "http_sse" | "http_sse_post" => {
                net.push(ev.args.first().map(String::as_str).unwrap_or(""))
            }
            _ => {}
        }
    }
    let uniq = |mut v: Vec<&str>| -> Vec<String> {
        v.sort_unstable();
        v.dedup();
        v.into_iter().map(String::from).collect()
    };
    out.push_str("\n── summary ───────────────────────────────────────────────\n");
    let m = uniq(mutating);
    let r = uniq(reads);
    let n = uniq(net);
    out.push_str(&format!(
        "  changed the world : {}\n",
        if m.is_empty() {
            "nothing".to_string()
        } else {
            m.join(", ")
        }
    ));
    out.push_str(&format!(
        "  read              : {}\n",
        if r.is_empty() {
            "nothing".to_string()
        } else {
            r.join(", ")
        }
    ));
    out.push_str(&format!(
        "  network           : {}\n",
        if n.is_empty() {
            "none".to_string()
        } else {
            n.join(", ")
        }
    ));
    out.push_str(&format!("  failed calls      : {failures}\n"));
    out
}

// ── Journal diff ─────────────────────────────────────────────────────────────

/// Where two runs stopped being the same run.
pub struct JournalDiff {
    /// How many leading events were identical.
    pub common_prefix: usize,
    /// `None` when one journal is simply a prefix of the other.
    pub first_difference: Option<(Option<HostEvent>, Option<HostEvent>)>,
    pub len_a: usize,
    pub len_b: usize,
}

/// Compare two journals and report the FIRST point at which they differ.
///
/// This is the question a reviewer actually asks — "what changed between these
/// two runs?" — and the reason `ReplayHost` matches sequentially rather than by
/// key: sequential order makes "first divergence" a well-defined position instead
/// of a set difference. `axon-os`'s `replay` answers the coarser form (are the
/// whole records equal?); this says WHERE.
pub fn diff_journals(a: &[HostEvent], b: &[HostEvent]) -> JournalDiff {
    let mut i = 0usize;
    // `seq` is excluded from the comparison: it is positional by construction, so
    // including it would make every event after a divergence "differ" too and bury
    // the one that matters.
    let same = |x: &HostEvent, y: &HostEvent| {
        x.method == y.method && x.args == y.args && x.status == y.status && x.payload == y.payload
    };
    while i < a.len() && i < b.len() && same(&a[i], &b[i]) {
        i += 1;
    }
    let first_difference = if i < a.len() || i < b.len() {
        Some((a.get(i).cloned(), b.get(i).cloned()))
    } else {
        None
    };
    JournalDiff {
        common_prefix: i,
        first_difference,
        len_a: a.len(),
        len_b: b.len(),
    }
}

impl JournalDiff {
    pub fn render(&self, opts: &RenderOpts) -> String {
        let mut out = String::new();
        match &self.first_difference {
            None => {
                out.push_str(&format!(
                    "IDENTICAL — {} events, every one the same.\n\nThe two runs saw exactly the \
                     same environment.\n",
                    self.common_prefix
                ));
            }
            Some((ea, eb)) => {
                out.push_str(&format!(
                    "The two runs agree for {} event(s), then DIVERGE at event {}.\n\n",
                    self.common_prefix, self.common_prefix
                ));
                match (ea, eb) {
                    (Some(x), Some(y)) => {
                        out.push_str(&format!("  A: {}\n", x.describe(opts).trim_start()));
                        out.push_str(&format!("  B: {}\n", y.describe(opts).trim_start()));
                        // Name the axis, so a reviewer does not have to spot the
                        // difference by comparing two similar lines.
                        let why = if x.method != y.method {
                            format!("different operation ({} vs {})", x.method, y.method)
                        } else if x.args != y.args {
                            "same operation, DIFFERENT ARGUMENTS".to_string()
                        } else if x.status != y.status {
                            format!(
                                "same call, different outcome ({} vs {})",
                                x.status, y.status
                            )
                        } else {
                            "same call and outcome, DIFFERENT RESULT DATA".to_string()
                        };
                        out.push_str(&format!("\n  what differs: {why}\n"));
                    }
                    (Some(x), None) => {
                        out.push_str(&format!(
                            "  A: {}\n  B: (nothing — run B stopped here)\n\n  \
                             run A did MORE: {} extra event(s)\n",
                            x.describe(opts).trim_start(),
                            self.len_a - self.common_prefix
                        ));
                    }
                    (None, Some(y)) => {
                        out.push_str(&format!(
                            "  A: (nothing — run A stopped here)\n  B: {}\n\n  \
                             run B did MORE: {} extra event(s)\n",
                            y.describe(opts).trim_start(),
                            self.len_b - self.common_prefix
                        ));
                    }
                    (None, None) => unreachable!("a difference with neither side present"),
                }
            }
        }
        out.push_str(&format!(
            "\n  A: {} events   B: {} events\n",
            self.len_a, self.len_b
        ));
        out
    }
}

// ── CLI wiring ───────────────────────────────────────────────────────────────

/// What [`install_from_env`] set up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Off,
    Recording,
    Replaying,
}

/// The live `ReplayHost`, kept so [`finish`] can ask whether the journal was
/// fully consumed. A replay that STOPS EARLY is a divergence too, and it is the
/// one shape a host method cannot detect from the inside — nobody calls it to
/// say "I'm done".
static ACTIVE_REPLAY: Mutex<Option<std::sync::Arc<ReplayHost>>> = Mutex::new(None);

/// Install a recording or replaying host from `AXON_RECORD` / `AXON_REPLAY`.
///
/// Called explicitly at the CLI boundary rather than lazily on first use (the
/// pattern `clock.rs` uses): a bad journal path must fail the run up front, not
/// halfway through, and a lazily-installed recorder would miss every call made
/// before the first one that happened to trigger it.
pub fn install_from_env() -> Result<Mode, String> {
    let rec = std::env::var(RECORD_ENV_VAR).ok().filter(|s| !s.is_empty());
    let rep = std::env::var(REPLAY_ENV_VAR).ok().filter(|s| !s.is_empty());
    match (rec, rep) {
        (Some(_), Some(_)) => Err(format!(
            "{RECORD_ENV_VAR} and {REPLAY_ENV_VAR} are both set — a run either \
             records the world or is served from a journal, never both. Unset one."
        )),
        (Some(path), None) => {
            let host = RecordingHost::new(crate::host::current_host(), &path)?;
            crate::host::set_host(std::sync::Arc::new(host));
            Ok(Mode::Recording)
        }
        (None, Some(path)) => {
            let host = std::sync::Arc::new(ReplayHost::from_path(std::path::Path::new(&path))?);
            *ACTIVE_REPLAY.lock().unwrap_or_else(|p| p.into_inner()) = Some(host.clone());
            crate::host::set_host(host);
            Ok(Mode::Replaying)
        }
        (None, None) => Ok(Mode::Off),
    }
}

/// After the program ends: the divergence report, if this run was not the run its
/// journal describes. `None` means a faithful replay (or no replay at all).
///
/// This is the half a program cannot swallow. A `ReplayHost` method can only
/// return the trait's own types, so a program is free to catch the error and
/// print "all good" — but the exit code is decided here, from state the program
/// never touched.
pub fn finish() -> Option<Divergence> {
    if let Some(report) = divergence() {
        // `diverge` already put this on stderr at the moment it happened.
        return Some(Divergence {
            report,
            already_reported: true,
        });
    }
    let guard = ACTIVE_REPLAY.lock().unwrap_or_else(|p| p.into_inner());
    let left = guard.as_ref()?.unconsumed();
    (left > 0).then(|| Divergence {
        report: format!(
            "the program finished having consumed only part of its journal — \
             {left} recorded host interaction(s) never happened, so this run did \
             LESS than the recorded one"
        ),
        // Nothing printed this: stopping early is invisible from inside a host
        // call, so it is discovered here and reported here.
        already_reported: false,
    })
}

/// A replay that is not the run its journal describes.
pub struct Divergence {
    pub report: String,
    /// Whether `diverge` already wrote the report to stderr. Keeps the CLI from
    /// printing the same paragraph twice.
    pub already_reported: bool,
}

/// Drop the installed replay handle. For tests, which run many in one process.
pub fn reset_for_tests() {
    reset_divergence_for_tests();
    *ACTIVE_REPLAY.lock().unwrap_or_else(|p| p.into_inner()) = None;
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn tmp(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("axon_replay_{}_{name}", std::process::id()))
    }

    /// A journal line survives a round-trip through values that would break a
    /// naive space/newline-delimited format.
    #[test]
    fn event_round_trips_through_hostile_payloads() {
        for payload in [
            "plain",
            "has spaces",
            "has\nnewline",
            "has\"quote\\backslash",
            "",
            "unicode → ✓ 日本",
        ] {
            let ev = HostEvent {
                seq: 7,
                method: "read_file".into(),
                args: vec!["/a b/c".into()],
                status: "ok".into(),
                payload: vec![payload.to_string()],
            };
            let line = ev.to_line();
            assert!(!line.trim_end().contains('\n'), "one event is one line");
            let back = HostEvent::from_line(line.trim_end()).expect("parses");
            assert_eq!(back, ev, "round-trip must be exact for {payload:?}");
        }
    }

    /// A corrupt line is a hard error, NOT a skipped line — a partial journal
    /// replaying as a complete one is the failure mode this whole module exists
    /// to prevent.
    #[test]
    fn corrupt_journal_line_is_refused_not_skipped() {
        let p = tmp("corrupt");
        std::fs::write(
            &p,
            "0 read_file ok 1 x2f61 1 x6f6b\nthis is not a journal line\n",
        )
        .unwrap();
        let err = read_journal(&p).expect_err("must refuse");
        assert!(err.contains("malformed"), "got: {err}");
        assert!(err.contains("line 2"), "must name the line: {err}");
        let _ = std::fs::remove_file(&p);
    }

    /// Concatenating two runs' journals is detected by the seq column.
    #[test]
    fn concatenated_journals_are_refused() {
        let p = tmp("concat");
        let a = HostEvent {
            seq: 0,
            method: "read_line".into(),
            args: vec![],
            status: "ok".into(),
            payload: vec!["x".into()],
        };
        std::fs::write(&p, format!("{}{}", a.to_line(), a.to_line())).unwrap();
        let err = read_journal(&p).expect_err("must refuse");
        assert!(err.contains("out of order"), "got: {err}");
        let _ = std::fs::remove_file(&p);
    }

    /// Record → replay serves the recorded answers with no inner host at all.
    #[test]
    fn replay_serves_recorded_answers_without_the_inner_host() {
        let _serial = crate::host::tests_host_lock();
        reset_divergence_for_tests();
        let p = tmp("roundtrip");

        struct Fake;
        impl AxonHost for Fake {
            fn read_file(&self, path: &str) -> Result<String, String> {
                Ok(format!("contents of {path}"))
            }
            fn write_file(&self, _: &str, _: &str) -> Result<(), String> {
                Ok(())
            }
            fn env_var(&self, k: &str) -> Option<String> {
                (k == "PRESENT").then(|| "yes".to_string())
            }
            fn now_ms(&self) -> i64 {
                42
            }
            fn sleep_ms(&self, _: u64) {}
            fn read_line(&self) -> Result<String, String> {
                Ok("typed input".into())
            }
            fn dir_list(&self, _: &str) -> Result<Vec<String>, String> {
                Ok(vec!["a".into(), "b c".into()])
            }
        }

        let rec = RecordingHost::new(Arc::new(Fake), &p).unwrap();
        assert_eq!(rec.read_file("/f").unwrap(), "contents of /f");
        assert_eq!(rec.read_line().unwrap(), "typed input");
        assert_eq!(rec.env_var("PRESENT").as_deref(), Some("yes"));
        assert_eq!(rec.env_var("ABSENT"), None);
        assert_eq!(rec.now_ms(), 42);
        assert_eq!(rec.dir_list("/d").unwrap(), vec!["a", "b c"]);
        drop(rec);

        // Replay with NO inner host: every answer must come from the journal.
        let rp = ReplayHost::from_path(&p).unwrap();
        assert_eq!(rp.read_file("/f").unwrap(), "contents of /f");
        assert_eq!(rp.read_line().unwrap(), "typed input");
        assert_eq!(rp.env_var("PRESENT").as_deref(), Some("yes"));
        assert_eq!(rp.env_var("ABSENT"), None);
        assert_eq!(rp.now_ms(), 42);
        assert_eq!(rp.dir_list("/d").unwrap(), vec!["a", "b c"]);
        assert_eq!(rp.unconsumed(), 0, "journal fully consumed");
        assert!(divergence().is_none(), "a faithful replay must not diverge");
        let _ = std::fs::remove_file(&p);
    }

    /// Replay does NOT perform writes or spawn processes.
    #[test]
    fn replay_does_not_touch_the_world() {
        let _serial = crate::host::tests_host_lock();
        reset_divergence_for_tests();
        let target = tmp("must_not_exist");
        let _ = std::fs::remove_file(&target);
        let ev = HostEvent {
            seq: 0,
            method: "write_file".into(),
            args: vec![target.display().to_string(), "payload".into()],
            status: "ok".into(),
            payload: vec![],
        };
        let rp = ReplayHost::new(vec![ev]);
        assert!(rp
            .write_file(&target.display().to_string(), "payload")
            .is_ok());
        assert!(
            !target.exists(),
            "replaying a write must NOT create the file — re-performing an \
             agent run's effects in order to study it is its own accident"
        );
        assert!(divergence().is_none());
    }

    /// A diverging call names the FIRST divergence and latches it.
    #[test]
    fn divergence_reports_the_first_departure_point() {
        let _serial = crate::host::tests_host_lock();
        reset_divergence_for_tests();
        let rp = ReplayHost::new(vec![HostEvent {
            seq: 0,
            method: "read_file".into(),
            args: vec!["/expected".into()],
            status: "ok".into(),
            payload: vec!["x".into()],
        }]);
        // Same method, different argument → divergence at event 0.
        assert!(rp.read_file("/actual").is_err());
        let d = divergence().expect("must latch");
        assert!(d.contains("event 0"), "must name the event: {d}");
        assert!(d.contains("/actual") && d.contains("/expected"), "got: {d}");

        // A SECOND divergence must not overwrite the first — the first is the answer.
        assert!(rp.read_file("/third").is_err());
        assert_eq!(divergence().as_deref(), Some(d.as_str()));
    }

    /// Running past the end of the journal is a divergence, not a fall-through
    /// to the real world.
    #[test]
    fn exhausted_journal_diverges_rather_than_going_live() {
        let _serial = crate::host::tests_host_lock();
        reset_divergence_for_tests();
        let rp = ReplayHost::new(vec![]);
        let r = rp.read_file("/etc/passwd");
        assert!(r.is_err(), "must not read the real file");
        let d = divergence().expect("must latch");
        assert!(d.contains("only 0 events"), "got: {d}");
    }

    /// EVERY `AxonHost` method must be recorded AND replayed.
    ///
    /// This is the gate, not a nicety. `ai_extract_uncertain_*` bypassed
    /// `AXON_AI_REPLAY` for months because "does this new effect have a replay
    /// story?" was a question someone had to remember to ask. Now that the seam
    /// is one trait, the set of effects is ENUMERABLE — so the question can be
    /// asked mechanically, and a new trait method that either host forgets to
    /// override fails here instead of silently inheriting a default that goes
    /// live (recording) or returns a fail-closed stub (replay).
    ///
    /// The check reads this file's own source rather than using reflection, which
    /// Rust does not have. That is crude but it fails in the safe direction: a
    /// method that is not mentioned in an `impl` block cannot be overridden, so a
    /// missing name is always a real gap. Adding the name in a comment would
    /// defeat it, which is why the extraction is scoped to the two `impl AxonHost`
    /// blocks.
    #[test]
    fn every_host_method_is_both_recorded_and_replayed() {
        let host_src = include_str!("host.rs");
        let this_src = include_str!("replay.rs");

        // The trait's method list: `fn name(` at four-space indent, inside the
        // `pub trait AxonHost` block only.
        let trait_body = {
            let start = host_src
                .find("pub trait AxonHost {")
                .expect("the trait must exist");
            let rest = &host_src[start..];
            let end = rest
                .find("\n// ── SSE parsing")
                .expect("the trait block must be delimited by the SSE section");
            &rest[..end]
        };
        let methods: Vec<&str> = trait_body
            .lines()
            .filter_map(|l| l.strip_prefix("    fn "))
            .filter_map(|l| l.split('(').next())
            .collect();
        assert!(
            methods.len() >= 16,
            "expected at least the 16 known host methods, found {}: {methods:?} — if the \
             extraction broke, this test is passing vacuously",
            methods.len()
        );

        // Each host's impl block.
        let impl_block = |marker: &str| -> &str {
            let start = this_src
                .find(marker)
                .unwrap_or_else(|| panic!("{marker} must exist"));
            let rest = &this_src[start..];
            // Ends at the next top-level section comment.
            let end = rest.find("\n// ──").unwrap_or(rest.len());
            &rest[..end]
        };
        let recording = impl_block("impl AxonHost for RecordingHost {");
        let replaying = impl_block("impl AxonHost for ReplayHost {");

        let mut missing: Vec<String> = Vec::new();
        for m in &methods {
            let sig = format!("fn {m}(");
            if !recording.contains(&sig) {
                missing.push(format!("RecordingHost::{m}"));
            }
            if !replaying.contains(&sig) {
                missing.push(format!("ReplayHost::{m}"));
            }
        }
        assert!(
            missing.is_empty(),
            "these host effects have no replay story: {missing:?}\n\n\
             Every AxonHost method must be overridden in BOTH RecordingHost (to journal it) and \
             ReplayHost (to serve it). A method left to the trait default silently goes LIVE \
             during recording, or returns a fail-closed stub during replay — either way a run \
             that touches it is not reproducible, which is exactly how the \
             ai_extract_uncertain_* replay hole survived for months."
        );
    }

    /// No interpreter builtin may reach the world except through the seam.
    ///
    /// The companion to the check above: that one proves the seam is complete,
    /// this one proves nothing routes around it. `read_line` called
    /// `std::io::stdin()` directly for the whole life of the seam, so "everything
    /// goes through AxonHost" was false by one member and nothing noticed.
    #[test]
    fn no_interp_builtin_reaches_the_world_directly() {
        let src = include_str!("interp/builtins.rs");
        // `std::process::Command` is the exec path; `stdin()` is the input path;
        // `std::fs::` is the filesystem. `std::env::var` is deliberately NOT here:
        // the interpreter reads its own AXON_* configuration vars, which are not
        // program-observable effects.
        let banned = ["std::io::stdin(", "std::fs::", "std::process::Command"];

        // KNOWN, NAMED EXEMPTION — not a way to make the test pass.
        //
        // The `dstore_*` builtins (Phase 7 durable `Store<T,C>`) read and write
        // their own append-only log directly. That IS program-observable
        // environment: `dstore_open` replays a log written by a PREVIOUS run, so a
        // replayed run sees whatever the store holds now, which is exactly the
        // "replay quietly consults live state" hazard.
        //
        // It is exempted rather than fixed because closing it needs a
        // `file_remove` on `AxonHost` (`dstore_clear` deletes the log), and that
        // method is DELIBERATELY absent: irreversible deletion whose risk
        // classification is unresolved and tagged needs-human (R42 §9 Q3, see
        // host.rs). Adding it here to turn this test green would be making a TCB
        // decision that was explicitly reserved for a person, in order to satisfy
        // a lint. So the gap is recorded in the open where it can be scheduled,
        // and the gate keeps its teeth for every other builtin.
        //
        // The exemption is by LINE CONTENT, scoped to the dstore log helper, so it
        // cannot accidentally cover a new unrelated `std::fs::` call.
        let exempt_dstore = |line: &str| -> bool {
            line.contains("store_log_path")
                || line.contains("std::fs::read_to_string(p)")
                || line.contains("std::fs::create_dir_all(dir)")
                || line.contains("std::fs::OpenOptions::new()")
                // `dstore_clear` — and the very call that BLOCKS the fix, since
                // `AxonHost` has no `file_remove` by deliberate decision.
                || line.contains("std::fs::remove_file(p)")
        };

        let mut hits: Vec<String> = Vec::new();
        let mut exempted = 0usize;
        for (i, line) in src.lines().enumerate() {
            let code = line.split("//").next().unwrap_or(line);
            for b in &banned {
                if code.contains(b) {
                    if exempt_dstore(code) {
                        exempted += 1;
                        continue;
                    }
                    hits.push(format!("interp/builtins.rs:{}: {}", i + 1, line.trim()));
                }
            }
        }
        // If the exemption stops matching anything, the dstore code was reworked —
        // re-check whether it still needs exempting rather than leaving a dead
        // allowlist that quietly covers something else later.
        assert!(
            exempted > 0,
            "the dstore exemption matched nothing — if dstore no longer touches std::fs              directly, DELETE the exemption; a stale allowlist is how a real bypass gets              covered later"
        );
        assert!(
            hits.is_empty(),
            "these builtins bypass the AxonHost seam:\n{}\n\n\
             An effect that does not go through the seam cannot be recorded, replayed, \
             virtualised for wasm/browser, or intercepted by a sandbox. Route it through a \
             trait method (default-denied, like exec/http_*) instead.",
            hits.join("\n")
        );
    }

    fn ev(seq: usize, m: &str, args: &[&str], status: &str, pay: &[&str]) -> HostEvent {
        HostEvent {
            seq,
            method: m.into(),
            args: args.iter().map(|s| s.to_string()).collect(),
            status: status.into(),
            payload: pay.iter().map(|s| s.to_string()).collect(),
        }
    }

    /// A transcript must NOT print payload values by default.
    ///
    /// This is the property that makes a transcript shareable, and therefore the
    /// property that makes human review of an agent run practical at all. A run
    /// that read an API key has that key in its journal; a default-verbose
    /// renderer would put it in every pasted transcript.
    #[test]
    fn transcript_redacts_values_by_default_and_shows_them_on_request() {
        let secret = "sk-live-DO-NOT-LEAK-abcdef";
        let events = vec![ev(0, "env_var", &["TOKEN"], "some", &[secret])];

        let redacted = render_transcript(&events, &RenderOpts::default());
        assert!(
            !redacted.contains(secret),
            "the default transcript must not contain a payload value:\n{redacted}"
        );
        assert!(
            redacted.contains("read env TOKEN"),
            "it must still say WHAT happened:\n{redacted}"
        );
        assert!(
            redacted.contains(&format!("{} bytes", secret.len())),
            "a redacted payload should still disclose its size:\n{redacted}"
        );

        let shown = render_transcript(&events, &RenderOpts { show_values: true });
        assert!(
            shown.contains(secret),
            "--show-values must actually show it:\n{shown}"
        );
    }

    /// Redaction must still let a reviewer tell two different payloads apart —
    /// otherwise a diff under redaction would be useless and they would have to
    /// un-redact (and paste secrets) to review a change.
    #[test]
    fn redacted_payloads_are_still_distinguishable_by_digest() {
        let a = render_transcript(
            &[ev(0, "read_file", &["/c"], "ok", &["threshold=5"])],
            &RenderOpts::default(),
        );
        let b = render_transcript(
            &[ev(0, "read_file", &["/c"], "ok", &["threshold=9"])],
            &RenderOpts::default(),
        );
        assert_ne!(
            a, b,
            "two different payloads of the SAME length must render differently \
             under redaction, or a redacted diff cannot show a content change"
        );
    }

    /// World-changing effects and failures must be findable by eye, and the
    /// summary must lead with what was changed — that is the reviewer's question.
    #[test]
    fn transcript_flags_mutating_effects_and_summarises_them() {
        let events = vec![
            ev(0, "read_file", &["/in"], "ok", &["data"]),
            ev(1, "write_file", &["/out", "body"], "ok", &[]),
            ev(2, "exec", &["rm", "-rf"], "ok", &[""]),
            ev(3, "read_file", &["/nope"], "err", &["not found"]),
        ];
        let t = render_transcript(&events, &RenderOpts::default());
        assert!(
            t.contains("* "),
            "mutating effects must carry a marker:\n{t}"
        );
        assert!(t.contains("! "), "failures must carry a marker:\n{t}");
        assert!(
            t.contains("changed the world : ") && t.contains("/out"),
            "the summary must name what was changed:\n{t}"
        );
        assert!(
            t.contains("failed calls      : 1"),
            "the summary must count failures:\n{t}"
        );
        // A pure-read run must say so positively rather than leaving it blank.
        let ro = render_transcript(
            &[ev(0, "read_file", &["/in"], "ok", &["x"])],
            &RenderOpts::default(),
        );
        assert!(
            ro.contains("changed the world : nothing"),
            "a read-only run must state that it changed nothing:\n{ro}"
        );
    }

    /// The diff must report the FIRST divergence and name the axis.
    #[test]
    fn diff_reports_the_first_divergence_and_what_differs() {
        let a = vec![
            ev(0, "read_file", &["/a"], "ok", &["same"]),
            ev(1, "read_file", &["/b"], "ok", &["A-side"]),
            ev(2, "read_file", &["/c"], "ok", &["also differs"]),
        ];
        let b = vec![
            ev(0, "read_file", &["/a"], "ok", &["same"]),
            ev(1, "read_file", &["/b"], "ok", &["B-side"]),
            ev(2, "read_file", &["/c"], "ok", &["differs too"]),
        ];
        let d = diff_journals(&a, &b);
        assert_eq!(d.common_prefix, 1, "one leading event was identical");
        let r = d.render(&RenderOpts::default());
        assert!(r.contains("DIVERGE at event 1"), "{r}");
        assert!(
            r.contains("DIFFERENT RESULT DATA"),
            "must name the axis, not just show two lines:\n{r}"
        );
        // Only the FIRST divergence is reported, even though event 2 differs too.
        assert!(
            !r.contains("event 2"),
            "reporting every later difference buries the one that matters:\n{r}"
        );
    }

    /// Identical journals are reported as identical, and a differing ARGUMENT is
    /// distinguished from differing RESULT DATA — they mean different things
    /// (the program behaved differently vs the world did).
    #[test]
    fn diff_distinguishes_argument_change_from_result_change() {
        let base = vec![ev(0, "read_file", &["/a"], "ok", &["x"])];
        assert!(diff_journals(&base, &base).first_difference.is_none());

        let other_arg = vec![ev(0, "read_file", &["/DIFFERENT"], "ok", &["x"])];
        let r = diff_journals(&base, &other_arg).render(&RenderOpts::default());
        assert!(r.contains("DIFFERENT ARGUMENTS"), "{r}");

        let other_method = vec![ev(0, "read_line", &[], "ok", &["x"])];
        let r2 = diff_journals(&base, &other_method).render(&RenderOpts::default());
        assert!(r2.contains("different operation"), "{r2}");
    }

    /// One journal being a PREFIX of the other is a divergence with a specific
    /// meaning: one run did more (or less) than the other.
    #[test]
    fn diff_reports_a_prefix_as_one_run_doing_more() {
        let short = vec![ev(0, "read_file", &["/a"], "ok", &["x"])];
        let long = vec![
            ev(0, "read_file", &["/a"], "ok", &["x"]),
            ev(1, "write_file", &["/b", "y"], "ok", &[]),
        ];
        let r = diff_journals(&short, &long).render(&RenderOpts::default());
        assert!(r.contains("run B did MORE"), "{r}");
        assert!(r.contains("1 extra event"), "{r}");
        let r2 = diff_journals(&long, &short).render(&RenderOpts::default());
        assert!(r2.contains("run A did MORE"), "{r2}");
    }

    /// A replay that stops EARLY is also a divergence — visible only from
    /// outside, which is why `unconsumed` exists.
    #[test]
    fn short_replay_leaves_unconsumed_events() {
        let _serial = crate::host::tests_host_lock();
        reset_divergence_for_tests();
        let rp = ReplayHost::new(vec![
            HostEvent {
                seq: 0,
                method: "read_line".into(),
                args: vec![],
                status: "ok".into(),
                payload: vec!["a".into()],
            },
            HostEvent {
                seq: 1,
                method: "read_line".into(),
                args: vec![],
                status: "ok".into(),
                payload: vec!["b".into()],
            },
        ]);
        assert_eq!(rp.read_line().unwrap(), "a");
        assert_eq!(rp.unconsumed(), 1, "one recorded event never happened");
        assert!(
            divergence().is_none(),
            "stopping early cannot be detected from inside a host call"
        );
    }
}
