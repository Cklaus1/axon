//! NH-R1 E3 (O-S1-07): the hard deadline of an unattended run.
//!
//! Before it, the deadline was soft: an in-flight `ai:` call ran up to 30 s
//! past it, checks were not cut, and nothing stopped a run stalled anywhere
//! else. The watchdog is a thread armed with the deadline. When it passes:
//!
//! 1. it takes the run's write lock ([`crate::cas::freeze`]): every workspace
//!    write of the main thread holds it, so none is half-done, and once
//!    frozen no other thread takes it again;
//! 2. it kills the process group of every running job (the check, a
//!    `cmd:` generator) ([`axon_jobctl::kill_running_jobs`]);
//! 3. it runs the caller's restore + report under the lock: the restore goes
//!    through the CAS helper, so after the bound the only write is the found
//!    bytes put back over this run's own;
//! 4. it writes the report straight to fd 1 (no stdout lock the main thread
//!    could hold) and calls `_exit(20)`. An in-flight model call dies with
//!    the process.
//!
//! [`HARD_GRACE`] bounds steps 1-4: S10 measures it under the host's real
//! load (`tests/watchdog.rs`, §10 Q4).

use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// The bound on lock wait + kill + restore + report + exit after the
/// deadline. NOT fixed in advance (§10 Q4): S10's measured tail under the
/// host's real load plus a margin. Measured 2026-10-31 (120 runs, the
/// model/check/writing stall cases, loadavg 50-76 on 32 cores): p99 423 ms,
/// max 693 ms. Set to p99 + 1577 ms margin (about 2.9x the observed max), so
/// a heavier load spike than the one measured still exits inside it.
pub const HARD_GRACE: Duration = Duration::from_millis(2000);

/// The exit code of a run the watchdog stopped (budget exhausted).
pub const EXIT_DEADLINE: i32 = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Armed,
    Disarmed,
    Fired,
}

type Shared = Arc<(Mutex<State>, Condvar)>;

/// An armed watchdog. Dropping it without [`Armed::disarm`] leaves it armed:
/// only a run that finished says so.
pub struct Armed {
    shared: Shared,
}

/// Arm the watchdog for `deadline`. `fire` runs at most once, under the
/// run's write lock, after every running job was killed; what it returns
/// is printed and the process exits [`EXIT_DEADLINE`].
pub fn arm<F>(deadline: Instant, fire: F) -> Armed
where
    F: FnOnce(&mut crate::cas::Ledger) -> String + Send + 'static,
{
    let shared: Shared = Arc::new((Mutex::new(State::Armed), Condvar::new()));
    let theirs = Arc::clone(&shared);
    std::thread::spawn(move || watch(&theirs, deadline, fire));
    Armed { shared }
}

fn watch<F>(shared: &Shared, deadline: Instant, fire: F)
where
    F: FnOnce(&mut crate::cas::Ledger) -> String,
{
    let (state, cv) = &**shared;
    let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
    loop {
        if *s == State::Disarmed {
            return;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        s = cv
            .wait_timeout(s, deadline - now)
            .unwrap_or_else(|p| p.into_inner())
            .0;
    }
    *s = State::Fired;
    drop(s);
    let mut ledger = crate::cas::freeze();
    axon_jobctl::kill_running_jobs();
    let report = fire(&mut ledger);
    write_fd1(report.as_bytes());
    // SAFETY: `_exit` ends the process without running destructors or
    // flushing stdio; the report was written straight to fd 1.
    unsafe { libc::_exit(EXIT_DEADLINE) }
}

/// `write(2)` all of `bytes` to fd 1, retrying a short write.
fn write_fd1(mut bytes: &[u8]) {
    while !bytes.is_empty() {
        // SAFETY: a write of a live slice to fd 1.
        let n = unsafe { libc::write(1, bytes.as_ptr().cast(), bytes.len()) };
        if n <= 0 {
            return;
        }
        bytes = &bytes[n as usize..];
    }
}

impl Armed {
    /// The run finished before its deadline. When the watchdog has already
    /// fired, it owns the verdict and the exit: this never returns, so the
    /// main thread cannot print a second verdict.
    pub fn disarm(self) {
        let (state, cv) = &*self.shared;
        let mut s = state.lock().unwrap_or_else(|p| p.into_inner());
        if *s == State::Fired {
            drop(s);
            loop {
                std::thread::park();
            }
        }
        *s = State::Disarmed;
        cv.notify_all();
    }
}
