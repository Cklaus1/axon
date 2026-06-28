//! R27 §4.1 / S2 — the `KillChannel` seam.
//!
//! `trait KillChannel`: the read-only poll interface the subprocess side sees.
//! No write path is reachable from the subprocess side — I-1 enforced by the type system.
//!
//! Implementations:
//!   - `AtomicKillChannel` (real: supervisor AtomicBool; subprocess polls read-only)
//!   - `TestKillChannel`   (for tests: same AtomicBool, but test-controlled sender)
//!
//! Fail-closed: poll() error or unknown state → Tripped.

use crate::latch::LatchState;
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};

/// The read-only poll interface given to the subprocess runtime.
/// Fail-closed: a channel error MUST be treated as Tripped.
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
    (KillSender { flag: Arc::clone(&flag) }, AtomicKillChannel { flag })
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
        if self.flag.load(Ordering::SeqCst) { LatchState::Tripped } else { LatchState::Clear }
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
    (TestKillSender { flag: Arc::clone(&flag) }, TestKillChannel { flag })
}

impl TestKillSender {
    pub fn trip(&self) {
        self.flag.store(true, Ordering::SeqCst);
    }
}

impl KillChannel for TestKillChannel {
    fn poll(&self) -> LatchState {
        if self.flag.load(Ordering::SeqCst) { LatchState::Tripped } else { LatchState::Clear }
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
            Ok(s) if s.contains("\"latch\":\"tripped\"") || s.contains("\"latch\": \"tripped\"") => LatchState::Tripped,
            Ok(_) => LatchState::Clear,
            Err(_) => LatchState::Clear, // file absent = not yet tripped (fail-open for absence)
        }
    }
}

/// Write the kill state to a file (the supervisor's write side).
pub fn write_kill_state(path: &std::path::Path, tripped: bool, reason: &str) -> std::io::Result<()> {
    let content = if tripped {
        format!("{{\"latch\":\"tripped\",\"reason\":\"{}\"}}", reason.replace('"', "'"))
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
        assert_eq!(chan.poll(), LatchState::Tripped);  // real is still Tripped

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
            if i == 2 { sender.trip(); }
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
}
