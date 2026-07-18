//! R33 §4 — pure quorum aggregation core (no I/O, no clock).
//!
//! `check_quorum` is the trusted decision function: it receives votes already
//! collected by the I/O layer (`quorum::io`) and returns a `QuorumResult`. Two
//! distinct failure modes are surfaced, never conflated (spec §4.2/§8 invariant
//! I-6 "exit code distinctness"; the R33 exit-code split is 13 = quorum blocked,
//! 14 = a vote's attestation was rejected):
//!   - **insufficient approvals** ("minority") — not enough YES votes for a
//!     strict majority of the operator-configured fleet size.
//!   - **attestation mismatch** — the votes that arrived don't agree on
//!     `voter_tcb` (an R31 `axtcb1-ext:` digest). In this fleet every voter is
//!     provisioned to run the same expected image, so disagreement means at
//!     least one voter is running unexpected software — a materially different,
//!     more serious signal than merely being outvoted. It must never be folded
//!     into "insufficient approvals" in `blocking_reason`.

use serde::{Deserialize, Serialize};

/// A proposing VM's request for quorum approval of an action (R33 §3.1, scoped:
/// this session's file-based exchange carries a single `voter_tcb`, the
/// proposer's own R31 `axtcb1-ext:`, rather than the full vsock broadcast).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoteRequest {
    pub run_id: String,
    pub prog_hash: String,
    pub voter_tcb: String,
    pub proposed_action: String,
    pub timestamp_ms: u64,
}

/// A peer VM's vote on a `VoteRequest` (R33 §3.2, scoped: a boolean
/// approve/deny rather than the full 0-100 policy score).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoteResponse {
    pub voter_tcb: String,
    pub run_id: String,
    pub approved: bool,
    pub reason: String,
}

/// The aggregator's decision (R33 §3.3, scoped).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuorumResult {
    pub quorum_met: bool,
    pub coalition_size: usize,
    pub approvals: usize,
    pub blocking_reason: Option<String>,
}

/// Strict-majority quorum check over already-collected votes.
///
/// `required_n` is the operator-configured fleet size — NOT `votes.len()`; a
/// vote that never arrived is simply absent (spec §8 "Peer VM unreachable").
///
/// Strict majority: `approvals > required_n / 2` (integer floor division).
/// This is `coord_goal_quorum`'s `n/2 + 1` threshold (`examples/stdlib/coord.ax`)
/// restated as a comparison — for integers, `approvals >= n/2 + 1` is exactly
/// `approvals > n/2`. The distinction matters at even `N`: for `N=4`,
/// `n/2 == 2`, so exactly 2 approvals is NOT a majority (`2 > 2` is false) —
/// only 3 or more clears it. A `>=` (non-strict) formula would wrongly accept
/// exactly-half as quorum, which is the off-by-one this function must avoid.
pub fn check_quorum(votes: &[VoteResponse], required_n: usize) -> QuorumResult {
    let coalition_size = votes.len();
    let approvals = votes.iter().filter(|v| v.approved).count();

    // Attestation consistency FIRST, independent of the approval count: every
    // vote that arrived must agree on `voter_tcb`. In this fleet, all voters
    // are provisioned to run the same expected image (R31 axtcb1-ext: is
    // pinned per deployment, not per voter slot, in this scoped module) — a
    // disagreement means at least one voter is not the software this quorum
    // trusts, which is a strictly more serious signal than being outvoted, so
    // it is reported as a distinct failure mode and short-circuits the count.
    let mut distinct_tcbs: Vec<&str> = Vec::new();
    for v in votes {
        if !distinct_tcbs.contains(&v.voter_tcb.as_str()) {
            distinct_tcbs.push(v.voter_tcb.as_str());
        }
    }
    if distinct_tcbs.len() > 1 {
        return QuorumResult {
            quorum_met: false,
            coalition_size,
            approvals,
            blocking_reason: Some(format!(
                "attestation mismatch: {} distinct voter_tcb values across {} votes ({})",
                distinct_tcbs.len(),
                coalition_size,
                distinct_tcbs.join(", "),
            )),
        };
    }

    // Strict majority: approvals > required_n / 2 (integer floor) — NOT >=,
    // which would wrongly admit an exactly-half coalition at even N.
    let quorum_met = approvals > required_n / 2;
    let blocking_reason = if quorum_met {
        None
    } else {
        Some(format!(
            "insufficient approvals: {approvals}/{required_n} (need > {}/2)",
            required_n
        ))
    };

    QuorumResult { quorum_met, coalition_size, approvals, blocking_reason }
}
