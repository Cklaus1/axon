//! R27 §3.3 / S6 — the per-lineage-root collusion bound.
//!
//! PURE: no I/O, no SystemTime, no rand.
//!
//! A `Coalition` groups all principals minted from one lineage root. The ceiling
//! bounds the SUM across all members — splitting a forbidden total into N shards
//! still hits the ceiling at the rollup.
//!
//! Anti-sock-puppet: membership is set by the SUPERVISOR at admission, never
//! by self-report. No contained code can register a new coalition root.

use crate::ledger::Carve;

pub type PrincipalId = String;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalitionCeiling {
    pub total_compute: u64,
    pub total_budget: u64,
    pub max_quorum_power: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoalitionBound {
    pub axis: String,
    pub rollup: u64,
    pub ceiling: u64,
    pub attempted: u64,
    pub member_count: usize,
}

impl std::fmt::Display for CoalitionBound {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "coalition bound exceeded on {}: rollup={} ceiling={} attempted={} members={}", self.axis, self.rollup, self.ceiling, self.attempted, self.member_count)
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Rollup {
    pub total_compute: u64,
    pub total_budget: u64,
    pub total_persist: u64,
}

#[derive(Debug, Clone, Default)]
struct MemberLedger {
    compute_used: u64,
    budget_used: u64,
    persist_used: u64,
}

#[derive(Debug, Clone)]
pub struct Coalition {
    pub root: PrincipalId,
    pub ceiling: CoalitionCeiling,
    ledgers: Vec<MemberLedger>,
    members: Vec<PrincipalId>,
    quorum_power_used: u64,
}

impl Coalition {
    pub fn new(root: &str, ceiling: CoalitionCeiling) -> Self {
        Coalition { root: root.to_string(), ceiling, ledgers: Vec::new(), members: Vec::new(), quorum_power_used: 0 }
    }

    /// Join a new member (supervisor-bound; never called by contained code).
    pub fn join(&mut self, member: PrincipalId) -> usize {
        self.ledgers.push(MemberLedger::default());
        self.members.push(member);
        self.members.len() - 1
    }

    pub fn rollup(&self) -> Rollup {
        Rollup {
            total_compute: self.ledgers.iter().map(|l| l.compute_used).sum(),
            total_budget: self.ledgers.iter().map(|l| l.budget_used).sum(),
            total_persist: self.ledgers.iter().map(|l| l.persist_used).sum(),
        }
    }

    /// Apply a resource carve to member `slot`. Fails if the coalition rollup
    /// would exceed the ceiling (checked FIRST, fail-closed).
    pub fn carve_for_member(&mut self, slot: usize, c: Carve) -> Result<(), CoalitionBound> {
        let r = self.rollup();
        if r.total_compute.saturating_add(c.compute) > self.ceiling.total_compute {
            return Err(CoalitionBound { axis: "compute".into(), rollup: r.total_compute, ceiling: self.ceiling.total_compute, attempted: c.compute, member_count: self.members.len() });
        }
        if r.total_budget.saturating_add(c.budget) > self.ceiling.total_budget {
            return Err(CoalitionBound { axis: "budget".into(), rollup: r.total_budget, ceiling: self.ceiling.total_budget, attempted: c.budget, member_count: self.members.len() });
        }
        let Some(ledger) = self.ledgers.get_mut(slot) else {
            return Err(CoalitionBound { axis: "slot".into(), rollup: 0, ceiling: 0, attempted: 0, member_count: self.members.len() });
        };
        ledger.compute_used = ledger.compute_used.saturating_add(c.compute);
        ledger.budget_used = ledger.budget_used.saturating_add(c.budget);
        ledger.persist_used = ledger.persist_used.saturating_add(c.persist_bytes);
        Ok(())
    }

    /// Attempt a vote. `claimed_pid` must match the supervisor-bound slot.
    /// Fails if claimed_pid ≠ bound pid (pid_forgery) OR quorum ceiling exceeded.
    pub fn propose_vote(&mut self, slot: usize, claimed_pid: &str, power: u64) -> Result<(), CoalitionBound> {
        let bound_pid = self.members.get(slot).cloned().unwrap_or_default();
        if bound_pid != claimed_pid {
            return Err(CoalitionBound { axis: "pid_forgery".into(), rollup: slot as u64, ceiling: 0, attempted: 0, member_count: self.members.len() });
        }
        if self.quorum_power_used.saturating_add(power) > self.ceiling.max_quorum_power {
            return Err(CoalitionBound { axis: "quorum_power".into(), rollup: self.quorum_power_used, ceiling: self.ceiling.max_quorum_power, attempted: power, member_count: self.members.len() });
        }
        self.quorum_power_used += power;
        Ok(())
    }

    pub fn member_count(&self) -> usize { self.members.len() }
    pub fn member_pid(&self, slot: usize) -> Option<&str> { self.members.get(slot).map(|s| s.as_str()) }
    pub fn quorum_power_used(&self) -> u64 { self.quorum_power_used }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ceiling(compute: u64, budget: u64, quorum: u64) -> CoalitionCeiling {
        CoalitionCeiling { total_compute: compute, total_budget: budget, max_quorum_power: quorum }
    }

    #[test]
    fn colluding_instances_exceed_coalition_bound_blocked() {
        // R4: 5 members each at <per-instance cap; SUM exceeds ceiling → blocked.
        // Anti-vacuous-pass: assert at least one denied attempt.
        let mut c = Coalition::new("root-A", ceiling(100, 200, 50));
        for i in 0..5 { c.join(format!("agent-{i}")); }
        // 3 carves of 30 each = 90 total (under 100).
        for slot in 0..3 {
            c.carve_for_member(slot, Carve { compute: 30, budget: 0, persist_bytes: 0 }).expect("under ceiling");
        }
        assert_eq!(c.rollup().total_compute, 90);
        // 4th carve of 30 = 120 > 100 → blocked (at least 1 denied attempt).
        let res = c.carve_for_member(3, Carve { compute: 30, budget: 0, persist_bytes: 0 });
        assert!(res.is_err(), "coalition ceiling must block colluding instances");
        let err = res.unwrap_err();
        assert_eq!(err.axis, "compute");
        assert!(err.rollup + err.attempted > err.ceiling, "anti-vacuous: denied attempt is over the ceiling");
    }

    #[test]
    fn sockpuppet_majority_cannot_force() {
        // R4: inflating instance count doesn't increase n. Quorum power is capped.
        let mut c = Coalition::new("root-B", ceiling(1000, 1000, 10));
        for i in 0..3 { c.join(format!("real-{i}")); }
        // Use 5+5=10 quorum power (the ceiling).
        c.propose_vote(0, "real-0", 5).unwrap();
        c.propose_vote(1, "real-1", 5).unwrap();
        // 3rd vote pushes over ceiling → blocked.
        let r = c.propose_vote(2, "real-2", 5);
        assert!(r.is_err(), "sockpuppet majority must be blocked at quorum ceiling");
        let err = r.unwrap_err();
        assert_eq!(err.axis, "quorum_power");
        assert_eq!(err.rollup, 10);
        assert_eq!(err.ceiling, 10);
    }

    #[test]
    fn instance_cannot_vote_as_another_pid() {
        // R4: claimed_pid ≠ bound slot pid → pid_forgery refused.
        let mut c = Coalition::new("root-C", ceiling(1000, 1000, 100));
        c.join("alice".to_string());
        c.join("bob".to_string());
        // Alice (slot=0) tries to vote AS bob.
        let r = c.propose_vote(0, "bob", 1);
        assert!(r.is_err(), "pid forgery must be refused");
        assert_eq!(r.unwrap_err().axis, "pid_forgery");
        // Bob (slot=1) votes correctly.
        c.propose_vote(1, "bob", 1).unwrap();
    }
}
