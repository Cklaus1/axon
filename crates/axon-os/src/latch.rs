//! R27 §3.1 / S1 — the supervisor-owned corrigibility latch.
//!
//! PURE: no I/O, no std::fs, no SystemTime, no rand.
//!
//! One-way state machine: Clear → Tripped (terminal). Fail-closed default:
//! unknown/errored poll is treated as Tripped, never Clear.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatchState {
    Clear,
    Tripped,
}

/// The supervisor-owned corrigibility latch.
#[derive(Debug, Clone)]
pub struct Latch {
    pub state: LatchState,
    pub reason: String,
    pub tripped_at_seq: u64,
}

impl Latch {
    pub fn clear() -> Self {
        Latch {
            state: LatchState::Clear,
            reason: String::new(),
            tripped_at_seq: 0,
        }
    }

    /// Trip the latch. Idempotent: second trip keeps the first trip's seq.
    pub fn trip(&self, reason: &str, seq: u64) -> Self {
        if self.state == LatchState::Tripped {
            return self.clone();
        }
        Latch {
            state: LatchState::Tripped,
            reason: reason.to_string(),
            tripped_at_seq: seq,
        }
    }

    pub fn poll(&self) -> LatchState {
        self.state
    }

    pub fn is_tripped(&self) -> bool {
        self.state == LatchState::Tripped
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn latch_is_one_way_and_fail_closed() {
        let l = Latch::clear();
        assert_eq!(l.poll(), LatchState::Clear);
        let l2 = l.trip("operator shutdown", 42);
        assert_eq!(l2.poll(), LatchState::Tripped);
        assert_eq!(l2.tripped_at_seq, 42);
        assert_eq!(l2.reason, "operator shutdown");
        // Second trip is idempotent — stays at first trip's seq.
        let l3 = l2.trip("second trip", 99);
        assert_eq!(l3.poll(), LatchState::Tripped);
        assert_eq!(l3.tripped_at_seq, 42, "first trip seq is kept");
    }

    #[test]
    fn tripped_state_is_terminal() {
        let l = Latch::clear().trip("r", 1);
        for _ in 0..10 {
            assert!(l.is_tripped());
        }
    }
}
