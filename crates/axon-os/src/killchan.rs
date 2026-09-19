//! R27 §4.1 / S2 — the `KillChannel` seam.
//!
//! `trait KillChannel`: the read-only poll interface the subprocess side sees.
//! No write path is reachable from the subprocess side — I-1 enforced by the type system.
//!
//! Implementations:
//!   - `AtomicKillChannel` (real: supervisor AtomicBool; subprocess polls read-only)
//!   - `TestKillChannel`   (for tests: same AtomicBool, but test-controlled sender)
//!
//! Kill channel semantics (decided 2026-09-19, triage OSK-P4-H7):
//!
//!   - missing channel file:                     Clear
//!   - readable file with clear state:           Clear
//!   - readable file with trip state:            Tripped
//!   - any other read/parse/access failure:      Tripped
//!
//! The invariant: **absence of a kill signal may mean Clear; inability to
//! DETERMINE the kill state must not mean Clear.** `NotFound` is the sole
//! fail-open case, because "the kill file has never been created" is a valid
//! steady state and part of the protocol (axon-os creates it holding
//! `{"latch":"clear"}`), not an exceptional failure. Everything else —
//! permission denied, an I/O fault, an unreadable or corrupt file — means the
//! channel can no longer be trusted to say "continue", so it says Tripped.
//!
//! The decision is derivable from the CURRENT error kind alone. It deliberately
//! does not depend on whether the file was once readable: safety behaviour that
//! turns on retained process-local history is harder to reason about and
//! differs between a fresh poll and a resumed one.

use crate::latch::LatchState;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

/// The read-only poll interface given to the subprocess runtime.
///
/// Fail-closed except for `NotFound` — see the module docs for the full table.
pub trait KillChannel: Send + Sync {
    fn poll(&self) -> LatchState;
}

// ── Real implementation ───────────────────────────────────────────────────────

/// Supervisor side: can trip the latch. NOT exposed to contained code.
#[derive(Clone)]
pub struct KillSender {
    flag: Arc<AtomicBool>,
}

/// Subprocess side: read-only poll. No setter reachable from this handle.
#[derive(Clone)]
pub struct AtomicKillChannel {
    flag: Arc<AtomicBool>,
}

pub fn kill_channel() -> (KillSender, AtomicKillChannel) {
    let flag = Arc::new(AtomicBool::new(false));
    (
        KillSender {
            flag: Arc::clone(&flag),
        },
        AtomicKillChannel { flag },
    )
}

impl KillSender {
    pub fn trip(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
    pub fn is_tripped(&self) -> bool {
        self.flag.load(Ordering::SeqCst)
    }
}

impl KillChannel for AtomicKillChannel {
    fn poll(&self) -> LatchState {
        if self.flag.load(Ordering::SeqCst) {
            LatchState::Tripped
        } else {
            LatchState::Clear
        }
    }
}

// ── Test implementation ────────────────────────────────────────────────────────

pub struct TestKillSender {
    flag: Arc<AtomicBool>,
}

pub struct TestKillChannel {
    flag: Arc<AtomicBool>,
}

pub fn test_kill_channel() -> (TestKillSender, TestKillChannel) {
    let flag = Arc::new(AtomicBool::new(false));
    (
        TestKillSender {
            flag: Arc::clone(&flag),
        },
        TestKillChannel { flag },
    )
}

impl TestKillSender {
    pub fn trip(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

impl KillChannel for TestKillChannel {
    fn poll(&self) -> LatchState {
        if self.flag.load(Ordering::SeqCst) {
            LatchState::Tripped
        } else {
            LatchState::Clear
        }
    }
}

// ── File-backed kill channel (for cross-process kill) ─────────────────────────

/// A kill channel backed by a file on disk. The supervisor writes the kill state;
/// the `run_bounded` loop reads it. Used for the `axon-os run --killable` /
/// `axon-os kill` cross-process flow.
pub struct FileKillChannel {
    path: std::path::PathBuf,
}

impl FileKillChannel {
    pub fn new(path: impl Into<std::path::PathBuf>) -> Self {
        FileKillChannel { path: path.into() }
    }
}

impl KillChannel for FileKillChannel {
    fn poll(&self) -> LatchState {
        match std::fs::read_to_string(&self.path) {
            Ok(s)
                if s.contains("\"latch\":\"tripped\"") || s.contains("\"latch\": \"tripped\"") =>
            {
                LatchState::Tripped
            }
            Ok(_) => LatchState::Clear,
            // NotFound is the SOLE fail-open case: the file has never been
            // created, which is a valid steady state, not a channel failure.
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => LatchState::Clear,
            // Anything else — permission denied, an I/O fault, an unreadable
            // file — means we cannot DETERMINE the state. This used to be
            // `Err(_) => Clear`, which made a kill switch that had become
            // unreadable indistinguishable from one that was not tripped, in
            // direct contradiction of this module's own documented contract.
            Err(_) => LatchState::Tripped,
        }
    }
}

/// Write the kill state to a file (the supervisor's write side).
pub fn write_kill_state(
    path: &std::path::Path,
    tripped: bool,
    reason: &str,
) -> std::io::Result<()> {
    let content = if tripped {
        format!(
            "{{\"latch\":\"tripped\",\"reason\":\"{}\"}}",
            reason.replace('"', "'")
        )
    } else {
        "{\"latch\":\"clear\"}".to_string()
    };
    std::fs::write(path, content)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contained_code_cannot_disable_latch() {
        // R1 / O-CORRIGIBLE: the subprocess side (TestKillChannel) exposes ONLY poll().
        // There is no clear()/reset() on the receiver — compile-time enforcement.
        //
        // Disable attempts exercised:
        //   1. Contained closure calls every method on the channel: only poll() exists.
        //   2. Concurrent trip from the supervisor side; channel stays Tripped.
        //   3. A forged separate Clear channel does NOT affect the real one.
        //   4. Double-trip is idempotent.

        let (sender, chan) = test_kill_channel();
        // Attempt 1: before trip — Clear.
        let contained_poll = || chan.poll();
        assert_eq!(contained_poll(), LatchState::Clear);

        // Supervisor trips (sender side; not reachable from contained code).
        sender.trip();

        // Attempt 2: contained code polls — must see Tripped.
        assert_eq!(contained_poll(), LatchState::Tripped);

        // Attempt 3: contained code makes a SEPARATE Clear channel.
        // This does NOT affect the real supervisor channel.
        let (_fake_s, fake_c) = test_kill_channel();
        assert_eq!(fake_c.poll(), LatchState::Clear); // fake is Clear
        assert_eq!(chan.poll(), LatchState::Tripped); // real is still Tripped

        // Attempt 4: double-trip is idempotent.
        sender.trip();
        assert_eq!(chan.poll(), LatchState::Tripped);
    }

    #[test]
    fn kill_channel_pair_is_linked() {
        let (s, c) = kill_channel();
        assert_eq!(c.poll(), LatchState::Clear);
        s.trip();
        assert_eq!(c.poll(), LatchState::Tripped);
    }

    #[test]
    fn poll_before_progress_gate() {
        // Simulate poll-before-progress: actions dispatched only while Clear.
        let (sender, chan) = test_kill_channel();
        let mut dispatched = 0u32;
        let mut denied = 0u32;
        for i in 0..5u32 {
            if chan.poll() == LatchState::Clear {
                dispatched += 1;
            } else {
                denied += 1;
            }
            if i == 2 {
                sender.trip();
            }
        }
        assert_eq!(dispatched, 3);
        assert_eq!(denied, 2);
    }

    #[test]
    fn file_kill_channel_reads_state() {
        let dir = std::env::temp_dir().join(format!("axon-r27-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("kill.json");
        let chan = FileKillChannel::new(&path);
        // Absent file = Clear.
        assert_eq!(chan.poll(), LatchState::Clear);
        // Write clear.
        write_kill_state(&path, false, "").unwrap();
        assert_eq!(chan.poll(), LatchState::Clear);
        // Write tripped.
        write_kill_state(&path, true, "test reason").unwrap();
        assert_eq!(chan.poll(), LatchState::Tripped);
        let _ = std::fs::remove_dir_all(&dir);
    }
    /// The full kill-channel table (triage OSK-P4-H7, decided 2026-09-19).
    ///
    /// The invariant under test: **absence of a kill signal may mean Clear;
    /// inability to DETERMINE the kill state must not mean Clear.** Before the
    /// fix this was `Err(_) => Clear`, so a kill file that had become
    /// unreadable was indistinguishable from one that was not tripped — a kill
    /// switch that silently stops working, in direct contradiction of this
    /// module's own documented contract.
    #[test]
    fn poll_fails_closed_on_everything_except_not_found() {
        let dir = std::env::temp_dir().join(format!("axon-kc-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // missing file → Clear. The SOLE fail-open case: never created is a
        // valid steady state, and a fresh killable run starts here.
        let missing = dir.join("nope.kill");
        assert_eq!(FileKillChannel::new(&missing).poll(), LatchState::Clear);

        // readable + clear → Clear
        let clear = dir.join("clear.kill");
        std::fs::write(&clear, "{\"latch\":\"clear\"}").unwrap();
        assert_eq!(FileKillChannel::new(&clear).poll(), LatchState::Clear);

        // readable + tripped → Tripped
        let tripped = dir.join("tripped.kill");
        std::fs::write(&tripped, "{\"latch\":\"tripped\"}").unwrap();
        assert_eq!(FileKillChannel::new(&tripped).poll(), LatchState::Tripped);

        // ANY OTHER read failure → Tripped. A directory in the file's place
        // yields an IsADirectory error on every platform and for every user —
        // unlike a permission test, which a root-owned CI would silently pass
        // by succeeding at the read.
        let as_dir = dir.join("adir.kill");
        std::fs::create_dir_all(&as_dir).unwrap();
        assert_eq!(
            FileKillChannel::new(&as_dir).poll(),
            LatchState::Tripped,
            "an undeterminable kill state must fail CLOSED"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }
}
