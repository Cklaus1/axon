//! R27 §3.2 / S4 — the resource acquisition ledger.
//!
//! PURE: no I/O, no SystemTime, no rand.
//!
//! Every compute/budget/persistence unit flows through `carve`; `_used` fields
//! never decrease (append-only, §8 edge case). Checked arithmetic prevents wrap.

use std::fmt;

#[derive(Debug, Clone, Copy, Default)]
pub struct Carve {
    pub compute: u64,
    pub budget: u64,
    pub persist_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceBound {
    pub axis: String,
    pub used: u64,
    pub cap: u64,
    pub attempted: u64,
}

impl fmt::Display for ResourceBound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "resource bound exceeded on {}: used={} cap={} attempted={}", self.axis, self.used, self.cap, self.attempted)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceLedger {
    pub lineage_root: String,
    pub compute_used: u64,
    pub compute_cap: u64,
    pub budget_used: u64,
    pub budget_cap: u64,
    pub persist_bytes_used: u64,
    pub persist_bytes_cap: u64,
    pub seq: u64,
}

impl ResourceLedger {
    pub fn new(lineage_root: &str, compute_cap: u64, budget_cap: u64, persist_bytes_cap: u64) -> Self {
        ResourceLedger {
            lineage_root: lineage_root.to_string(),
            compute_used: 0, compute_cap,
            budget_used: 0, budget_cap,
            persist_bytes_used: 0, persist_bytes_cap,
            seq: 0,
        }
    }

    pub fn would_exceed(&self, c: Carve) -> bool {
        self.compute_used.saturating_add(c.compute) > self.compute_cap
            || self.budget_used.saturating_add(c.budget) > self.budget_cap
            || self.persist_bytes_used.saturating_add(c.persist_bytes) > self.persist_bytes_cap
    }

    pub fn carve(&self, c: Carve) -> Result<Self, ResourceBound> {
        if self.compute_used.saturating_add(c.compute) > self.compute_cap {
            return Err(ResourceBound { axis: "compute".into(), used: self.compute_used, cap: self.compute_cap, attempted: c.compute });
        }
        if self.budget_used.saturating_add(c.budget) > self.budget_cap {
            return Err(ResourceBound { axis: "budget".into(), used: self.budget_used, cap: self.budget_cap, attempted: c.budget });
        }
        if self.persist_bytes_used.saturating_add(c.persist_bytes) > self.persist_bytes_cap {
            return Err(ResourceBound { axis: "persist_bytes".into(), used: self.persist_bytes_used, cap: self.persist_bytes_cap, attempted: c.persist_bytes });
        }
        Ok(ResourceLedger {
            lineage_root: self.lineage_root.clone(),
            compute_used: self.compute_used + c.compute,
            compute_cap: self.compute_cap,
            budget_used: self.budget_used + c.budget,
            budget_cap: self.budget_cap,
            persist_bytes_used: self.persist_bytes_used + c.persist_bytes,
            persist_bytes_cap: self.persist_bytes_cap,
            seq: self.seq + 1,
        })
    }

    pub fn compute_remaining(&self) -> u64 { self.compute_cap.saturating_sub(self.compute_used) }
    pub fn budget_remaining(&self) -> u64 { self.budget_cap.saturating_sub(self.budget_used) }
    pub fn persist_remaining(&self) -> u64 { self.persist_bytes_cap.saturating_sub(self.persist_bytes_used) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    // The `_R20`/`_R25` suffixes are required VERBATIM by
    // `scripts/r27_acceptance_gate.sh`, which now runs these tests by name.
    // Renaming them to satisfy the lint would break the gate, so the lint is
    // silenced instead of the name changed.
    #[allow(non_snake_case)]
    fn mint_beyond_grant_refused_R20() {
        let ledger = ResourceLedger::new("root", 100, 50, 1024);
        let r = ledger.carve(Carve { compute: 0, budget: 51, persist_bytes: 0 });
        assert!(r.is_err());
        let err = r.unwrap_err();
        assert_eq!(err.axis, "budget");
        assert_eq!(err.cap, 50);
        assert_eq!(err.attempted, 51);
    }

    #[test]
    fn budget_acquisition_blocked() {
        // Exhaust the budget cap (100), then any further budget carve is denied.
        let ledger = ResourceLedger::new("root", 1000, 100, 0);
        let l2 = ledger.carve(Carve { compute: 0, budget: 100, persist_bytes: 0 }).unwrap();
        assert_eq!(l2.budget_used, 100);
        assert_eq!(l2.budget_remaining(), 0);
        // Any further budget carve is denied.
        let r = l2.carve(Carve { compute: 0, budget: 1, persist_bytes: 0 });
        assert!(r.is_err(), "exhausted budget cap must deny further carves");
        // Overflow attempt: u64::MAX saturates above cap → denied (no wrap-to-zero exploit).
        let r2 = l2.carve(Carve { compute: 0, budget: u64::MAX, persist_bytes: 0 });
        assert!(r2.is_err(), "saturating_add prevents wrap-to-zero exploit");
        // Compute cap (1000) is not exhausted, so compute-only carves still succeed.
        let r3 = l2.carve(Carve { compute: 1, budget: 0, persist_bytes: 0 });
        assert!(r3.is_ok(), "non-exhausted compute cap allows compute carve");
    }

    #[test]
    fn seq_is_monotone_per_carve() {
        let l = ResourceLedger::new("root", 100, 100, 100);
        assert_eq!(l.seq, 0);
        let l2 = l.carve(Carve { compute: 1, budget: 1, persist_bytes: 0 }).unwrap();
        assert_eq!(l2.seq, 1);
        let l3 = l2.carve(Carve { compute: 1, budget: 1, persist_bytes: 0 }).unwrap();
        assert_eq!(l3.seq, 2);
    }

    #[test]
    fn used_never_decreases() {
        let l = ResourceLedger::new("root", 100, 100, 100);
        let l2 = l.carve(Carve { compute: 50, budget: 50, persist_bytes: 0 }).unwrap();
        assert!(l2.compute_used >= l.compute_used);
        assert!(l2.budget_used >= l.budget_used);
    }
}
