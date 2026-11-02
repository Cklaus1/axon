//! GV-1: the run's outcome record (`axon-outcome/1`).
//!
//! A supervisor that grades a run (axon-os `run_sandboxed_capture`, the job
//! path's `classify`) must learn how the run ended from a channel the program
//! cannot write. The program owns stdout and stderr, and every carved fault
//! code (2-9, 101) is also a legal `exit(n)` status, so neither the status nor
//! any stderr line can tell "the interpreter refused this" from "the program
//! said so".
//!
//! The supervisor hands `axon run` a pipe on [`OUTCOME_FD_ENV`] and a per-run
//! nonce on [`OUTCOME_NONCE_ENV`]. The CLI reads both once at start, removes
//! them from the environment, makes the fd close-on-exec and installs it here
//! ([`install`]). Every exit `axon run` can take then goes through
//! [`exit_with_outcome`], which writes exactly one newline-terminated JSON line
//!
//! `{"schema":"axon-outcome/1","nonce":"..","kind":"exit"|"fault"|"compile","code":N,"class":"..","reason":".."}`
//!
//! to that fd before `process::exit(code)`. With no channel installed (every
//! run without the variables, every other verb) it is exactly
//! `process::exit(code)`: stdout, stderr and the status are unchanged.
//!
//! The interpreter's run entry (`interp::run_program_inner`) states how the
//! program ended with [`record_run`]; the normal end of `axon run` writes that
//! with [`exit_with_run_outcome`]. A run that stated nothing (a Rust panic or
//! abort in the interpreter) writes no record, which the supervisor reads as
//! a refusal (fail closed).

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// The variable naming the fd the outcome record is written to.
pub const OUTCOME_FD_ENV: &str = "AXON_OUTCOME_FD";
/// The variable carrying the per-run nonce every record repeats.
pub const OUTCOME_NONCE_ENV: &str = "AXON_OUTCOME_NONCE";
/// The record's `schema`.
pub const OUTCOME_SCHEMA: &str = "axon-outcome/1";
/// A record line, newline included, is at most this many bytes (`PIPE_BUF`,
/// so one `write` of it is atomic). `reason` is cut first to fit.
pub const OUTCOME_RECORD_MAX: usize = 4096;

/// How a run ended, as the record states it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The program ended itself: `main` returned, `exit(n)`, or a stray
    /// `return`/`break`/`continue` left `main`.
    Exit,
    /// The interpreter stopped the program (a sandbox violation, a failed
    /// `@[verify]`, a panic, ...).
    Fault,
    /// Nothing of the program ran: it did not parse or check, or the run was
    /// refused before it started.
    Compile,
}

impl Kind {
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Exit => "exit",
            Kind::Fault => "fault",
            Kind::Compile => "compile",
        }
    }
}

struct Channel {
    fd: i32,
    nonce: String,
}

static CHANNEL: OnceLock<Channel> = OnceLock::new();
static WRITTEN: AtomicBool = AtomicBool::new(false);
/// What the last interpreter run stated: (kind, class, reason).
static RUN_OUTCOME: Mutex<Option<(Kind, &'static str, String)>> = Mutex::new(None);

/// Whether `nonce` has the shape a supervisor sends: 16 to 128 lowercase hex
/// digits (it is copied into a JSON string unescaped by nobody but us, and a
/// shape check refuses a variable that is not a nonce at all).
pub fn valid_nonce(nonce: &str) -> bool {
    (16..=128).contains(&nonce.len())
        && nonce
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Install the outcome channel (the CLI calls this once, after it removed the
/// variables and made `fd` close-on-exec). A second call is ignored.
pub fn install(fd: i32, nonce: String) {
    let _ = CHANNEL.set(Channel { fd, nonce });
}

/// Whether an outcome channel is installed.
pub fn installed() -> bool {
    CHANNEL.get().is_some()
}

/// The interpreter states how the program ended. The last call wins: the
/// outermost run finishes last.
pub fn record_run(kind: Kind, class: &'static str, reason: &str) {
    if let Ok(mut g) = RUN_OUTCOME.lock() {
        *g = Some((kind, class, reason.to_string()));
    }
}

fn push_json_char(out: &mut String, c: char) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
        c => out.push(c),
    }
}

fn json_str(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        push_json_char(out, c);
    }
    out.push('"');
}

/// The record line (newline included), at most [`OUTCOME_RECORD_MAX`] bytes:
/// `reason` is cut, at a character boundary, so the line fits.
pub fn record_line(nonce: &str, kind: Kind, class: &str, code: i32, reason: &str) -> String {
    let mut s = String::with_capacity(160 + reason.len().min(OUTCOME_RECORD_MAX));
    s.push_str("{\"schema\":");
    json_str(&mut s, OUTCOME_SCHEMA);
    s.push_str(",\"nonce\":");
    json_str(&mut s, nonce);
    s.push_str(",\"kind\":");
    json_str(&mut s, kind.as_str());
    s.push_str(&format!(",\"code\":{code},\"class\":"));
    json_str(&mut s, class);
    s.push_str(",\"reason\":\"");
    // What follows `reason`'s text: `"}` and the newline.
    let budget = OUTCOME_RECORD_MAX.saturating_sub(3);
    let mut esc = String::with_capacity(8);
    for c in reason.chars() {
        esc.clear();
        push_json_char(&mut esc, c);
        if s.len() + esc.len() > budget {
            break;
        }
        s.push_str(&esc);
    }
    s.push_str("\"}\n");
    s
}

/// Write the record once, if a channel is installed.
fn write_record(kind: Kind, class: &str, code: i32, reason: &str) {
    let Some(ch) = CHANNEL.get() else {
        return;
    };
    if WRITTEN.swap(true, Ordering::SeqCst) {
        return;
    }
    let line = record_line(&ch.nonce, kind, class, code, reason);
    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::fd::FromRawFd;
        // SAFETY: the CLI checked the fd is open when it installed it, and
        // nothing else in this process closes it; `ManuallyDrop` leaves it
        // open (the process exits right after).
        let mut f = std::mem::ManuallyDrop::new(unsafe { std::fs::File::from_raw_fd(ch.fd) });
        let _ = f.write_all(line.as_bytes());
    }
    #[cfg(not(unix))]
    let _ = line;
}

/// Write the outcome record (when a channel is installed), then
/// `process::exit(code)`. Every exit `axon run` takes goes through here.
pub fn exit_with_outcome(kind: Kind, class: &str, code: i32, reason: &str) -> ! {
    write_record(kind, class, code, reason);
    std::process::exit(code)
}

/// The normal end of `axon run`: write what the interpreter stated
/// ([`record_run`]) with the process status `code`, then exit. A run that
/// stated nothing writes no record.
pub fn exit_with_run_outcome(code: i32) -> ! {
    let stated = RUN_OUTCOME.lock().ok().and_then(|g| g.clone());
    if let Some((kind, class, reason)) = stated {
        write_record(kind, class, code, &reason);
    }
    std::process::exit(code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_line_is_one_terminated_line_within_the_bound() {
        let n = "0123456789abcdef0123456789abcdef";
        let l = record_line(
            n,
            Kind::Fault,
            "sandbox",
            8,
            "axon: sandbox violation: x\ny",
        );
        assert_eq!(
            l,
            "{\"schema\":\"axon-outcome/1\",\"nonce\":\"0123456789abcdef0123456789abcdef\",\
             \"kind\":\"fault\",\"code\":8,\"class\":\"sandbox\",\"reason\":\"axon: sandbox \
             violation: x\\ny\"}\n"
        );
        let long = "é\"".repeat(5000);
        let l = record_line(n, Kind::Compile, "check", 2, &long);
        assert!(l.len() <= OUTCOME_RECORD_MAX, "{}", l.len());
        assert!(l.ends_with("\"}\n") && l.matches('\n').count() == 1);
        assert!(
            l.len() > OUTCOME_RECORD_MAX - 16,
            "cut more than needed: {}",
            l.len()
        );
    }

    #[test]
    fn only_hex_nonces_are_nonces() {
        assert!(valid_nonce("0123456789abcdef"));
        assert!(!valid_nonce("0123456789abcde"));
        assert!(!valid_nonce("0123456789abcdeg"));
        assert!(!valid_nonce("0123456789ABCDEF"));
        assert!(!valid_nonce("0123456789abcdef\",\"x"));
    }
}
