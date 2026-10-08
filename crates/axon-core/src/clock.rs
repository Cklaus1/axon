//! Deterministic virtual clock — the last piece of "an Axon run replays exactly".
//!
//! # Why this exists
//!
//! `AXON_SEED` makes every `random_*` reproducible and `AXON_AI_REPLAY` makes
//! every `ai_complete` reproducible, so `axon trace --replay <run-id>` claims a
//! deterministic `(Trace, Seed)` pair "for every run". That claim was FALSE for
//! any program that reads the clock: `now_ms()` went straight to the host and
//! returned a different answer every time, so a replay of
//!
//! ```text
//! let t = now_ms()
//! println(to_str(t))
//! ```
//!
//! reproduced nothing at all. Replay is the property this language sells to
//! auditors of agent behaviour — an agent you cannot replay is an agent you
//! cannot review — so a hole in it is a correctness bug, not a missing feature.
//!
//! # The clock is MONOTONIC, not frozen
//!
//! A frozen clock (`now_ms()` always returns the same value) is the obvious
//! implementation and it is wrong. Real programs measure elapsed time:
//!
//! ```text
//! let t = now_ms()  sleep_ms(1)  let t2 = now_ms()
//! if t2 > t { ... }
//! ```
//!
//! `tests/fixtures/io_builtins.ax` does exactly this. Under a frozen clock
//! `t2 == t` and the program takes the other branch — so a "determinism" feature
//! would have silently changed what programs compute. Instead each `now_ms()`
//! call returns the current virtual time and then advances it by `tick`, so
//! successive reads strictly increase and elapsed-time logic keeps working.
//!
//! `sleep_ms(n)` advances the virtual clock by `n` and does NOT really sleep.
//! That is deliberate and doubly useful: it keeps the timeline consistent (a
//! program that sleeps 100 ms sees 100 ms pass) and it makes replaying a run
//! that slept for ten seconds instant.
//!
//! # What is NOT virtualized
//!
//! Only the `now_ms`/`now_ns`/`sleep_ms` BUILTINS — the program's view of time. The
//! provenance log's own `ts_ms` timestamps and the RNG's entropy fallback use a
//! separate private helper on the real clock, because a log that lies about when
//! it was written is useless for an audit.
//!
//! # Native parity
//!
//! `now_ms`/`now_ns`/`sleep_ms` are also native externs (`__axon_now_ms`,
//! `__axon_now_ns`, `__axon_sleep_ms` in `axon-rt`), and `axon-core` does not depend on `axon-rt`,
//! so this logic is implemented twice. That is a divergence risk by
//! construction, which is what `scripts/clock_parity.sh` is for — the same
//! arrangement as `AXON_AI_MOCK`, which native had to learn to honour after it
//! was found silently ignoring it.

use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

/// Whether a virtual clock is in force. Process-wide rather than thread-local:
/// the native suspend/resume substrate runs Axon on a worker thread (R15), and a
/// clock that reset per thread would make a suspend look like a time jump.
static ENABLED: AtomicBool = AtomicBool::new(false);
/// Whether `AXON_CLOCK` has been consulted yet (so an unset var is only read once
/// and a programmatic `set` is not overwritten by a later lazy init).
static INITIALIZED: AtomicBool = AtomicBool::new(false);
static CURRENT: AtomicI64 = AtomicI64::new(0);
static TICK: AtomicI64 = AtomicI64::new(1);
/// The configured start (`set`/`AXON_CLOCK`): the origin of the virtual `now_ns`.
static START: AtomicI64 = AtomicI64::new(0);

/// Environment variable: `AXON_CLOCK=<start_ms>` or `AXON_CLOCK=<start_ms>:<tick_ms>`.
///
/// `tick` defaults to 1 ms and may be 0 for a clock that only moves when
/// `sleep_ms` advances it. A start of 0 is legitimate (the Unix epoch), which is
/// why "enabled" is a separate flag rather than a sentinel value.
pub const ENV_VAR: &str = "AXON_CLOCK";

/// Enable the virtual clock explicitly. Used by `axon trace --replay`, which
/// anchors it to the `ts_ms` the original run recorded — so a replayed run sees
/// the wall-clock time of the run it is reproducing, not of the replay.
pub fn set(start_ms: i64, tick_ms: i64) {
    CURRENT.store(start_ms, Ordering::SeqCst);
    START.store(start_ms, Ordering::SeqCst);
    TICK.store(tick_ms.max(0), Ordering::SeqCst);
    ENABLED.store(true, Ordering::SeqCst);
    INITIALIZED.store(true, Ordering::SeqCst);
}

/// Disable the virtual clock and forget any lazy init. Test-only: the statics are
/// process-wide, so a test that enables the clock must undo it or it leaks into
/// every later test in the same binary.
pub fn reset_for_tests() {
    ENABLED.store(false, Ordering::SeqCst);
    INITIALIZED.store(false, Ordering::SeqCst);
    CURRENT.store(0, Ordering::SeqCst);
    START.store(0, Ordering::SeqCst);
    TICK.store(1, Ordering::SeqCst);
}

fn init_from_env() {
    if INITIALIZED.swap(true, Ordering::SeqCst) {
        return;
    }
    let Ok(raw) = std::env::var(ENV_VAR) else {
        return;
    };
    let raw = raw.trim();
    if raw.is_empty() {
        return;
    }
    let (start, tick) = match raw.split_once(':') {
        Some((s, t)) => (s.trim().parse::<i64>(), t.trim().parse::<i64>().ok()),
        None => (raw.parse::<i64>(), None),
    };
    // A malformed value leaves the clock OFF rather than guessing a start time.
    // Guessing would make `AXON_CLOCK=lol` silently produce a deterministic run
    // whose timeline nobody chose, which is worse than ignoring it.
    if let Ok(start) = start {
        CURRENT.store(start, Ordering::SeqCst);
        START.store(start, Ordering::SeqCst);
        TICK.store(tick.unwrap_or(1).max(0), Ordering::SeqCst);
        ENABLED.store(true, Ordering::SeqCst);
    }
}

/// `true` when a virtual clock governs `now_ms`/`sleep_ms`.
pub fn enabled() -> bool {
    init_from_env();
    ENABLED.load(Ordering::SeqCst)
}

/// The virtual "now", advancing by `tick` per call. `None` when no virtual clock
/// is in force, in which case the caller must use the real host clock.
pub fn now_ms() -> Option<i64> {
    if !enabled() {
        return None;
    }
    // fetch_add returns the PREVIOUS value, which is what this call should see —
    // so the first read is exactly the configured start.
    Some(CURRENT.fetch_add(TICK.load(Ordering::SeqCst), Ordering::SeqCst))
}

/// Nanoseconds per virtual millisecond. `now_ns` has no finer virtual unit: the
/// timeline is one counter in ms, shared with `now_ms`/`sleep_ms`.
pub const NS_PER_MS: i64 = 1_000_000;

/// The virtual clock as the monotonic `now_ns` builtin sees it: the SAME
/// timeline as [`now_ms`], read as nanoseconds SINCE THE CONFIGURED START —
/// `(virtual_ms - start_ms) * 1_000_000` — and advanced by `tick` exactly like a
/// `now_ms` read. One timeline, not two: a program mixing `now_ms()` and
/// `now_ns()` under `AXON_CLOCK` sees every ms delta as exactly 1e6 ns.
///
/// Why relative to the start rather than `virtual_ms * 1e6`: the usual start is
/// an epoch in ms (`AXON_CLOCK=1700000000000`), and 1.7e12 ms is 1.7e21 ns, past
/// `i64::MAX` (9.2e18) — the absolute scheme would pin every read at the top and
/// FREEZE the clock. `now_ns` has no defined origin anyway (the real clock is
/// relative to a process-local anchor), so "ns since the run's start" is the
/// virtual mirror of the real behaviour. Saturating, so a timeline sleeping past
/// ~292 years pins at `i64::MAX` instead of wrapping. `None` → real clock.
pub fn now_ns() -> Option<i64> {
    now_ms().map(|ms| {
        ms.saturating_sub(START.load(Ordering::SeqCst))
            .max(0)
            .saturating_mul(NS_PER_MS)
    })
}

/// Advance the virtual clock by `ms` (a `sleep`). Returns `false` when no virtual
/// clock is in force, meaning the caller should really sleep.
pub fn advance(ms: i64) -> bool {
    if !enabled() {
        return false;
    }
    CURRENT.fetch_add(ms.max(0), Ordering::SeqCst);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests mutate process-wide statics, so they must not run
    /// concurrently with each other. One test function, sequential asserts.
    #[test]
    fn virtual_clock_is_monotonic_and_anchored() {
        reset_for_tests();
        assert!(now_ms().is_none(), "off by default without AXON_CLOCK");
        assert!(!advance(5), "advance is a no-op when disabled");

        // Anchored: the FIRST read is the configured start, not start+tick.
        set(1_000, 10);
        assert_eq!(now_ms(), Some(1_000));
        assert_eq!(now_ms(), Some(1_010));
        assert_eq!(now_ms(), Some(1_020));

        // A sleep moves the timeline by exactly its argument.
        assert!(advance(500));
        assert_eq!(now_ms(), Some(1_530));

        // The elapsed-time shape that a frozen clock would have broken:
        // read, sleep, read again must strictly increase.
        set(0, 1);
        let t = now_ms().unwrap();
        advance(1);
        let t2 = now_ms().unwrap();
        assert!(
            t2 > t,
            "t2 ({t2}) must exceed t ({t}) — io_builtins.ax relies on it"
        );

        // tick 0 means the clock ONLY moves on sleep — a legitimate config, and
        // the reason `enabled` is a flag rather than a non-zero sentinel.
        set(42, 0);
        assert_eq!(now_ms(), Some(42));
        assert_eq!(now_ms(), Some(42));
        assert!(advance(8));
        assert_eq!(now_ms(), Some(50));

        // Start 0 is a real start (the Unix epoch), not "unset".
        set(0, 0);
        assert_eq!(now_ms(), Some(0));
        assert!(enabled());

        // `now_ns` is the SAME timeline in ns since the configured start, and a
        // read advances it by `tick` like a `now_ms` read — so interleaved reads
        // agree and keep increasing.
        set(1_000, 1);
        assert_eq!(now_ns(), Some(0));
        assert_eq!(now_ms(), Some(1_001));
        assert_eq!(now_ns(), Some(2 * NS_PER_MS));
        assert!(advance(10));
        assert_eq!(now_ns(), Some(13 * NS_PER_MS));

        // An epoch-scale start (the usual AXON_CLOCK) must not overflow: an
        // absolute `ms * 1e6` would be 1.7e21 and freeze at i64::MAX.
        set(1_700_000_000_000, 1);
        let a = now_ns().unwrap();
        assert!(advance(250));
        let b = now_ns().unwrap();
        assert_eq!((a, b), (0, 251 * NS_PER_MS));

        // Saturating, never wrapping: a timeline slept past i64::MAX ns pins at
        // the top instead of going negative — the clock never runs backwards.
        set(0, 0);
        assert!(advance(i64::MAX / NS_PER_MS + 1));
        assert_eq!(now_ns(), Some(i64::MAX));
        assert_eq!(now_ns(), Some(i64::MAX));

        reset_for_tests();
        assert!(!enabled());
    }
}
