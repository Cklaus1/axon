//! R33 — cross-VM safety quorum: attested `VoteRequest`/`VoteResponse` +
//! strict-majority `check_quorum` aggregation.
//!
//! Scoped slice of `governance/specs/R33-cross-vm-safety-quorum.md`: a file-based
//! `propose`/`vote`/`check` CLI exchange with a pure aggregator, not yet the vsock
//! broadcast transport or the R27 per-lineage coalition ceiling (see that spec's
//! `spec-meta` note for exactly what remains open).

pub mod logic;
pub mod io;

#[cfg(test)]
mod tests {
    use super::logic::{check_quorum, VoteResponse};

    fn mk_vote(tcb: &str, approved: bool) -> VoteResponse {
        VoteResponse {
            voter_tcb: tcb.to_string(),
            run_id: "r1".to_string(),
            approved,
            reason: String::new(),
        }
    }

    /// Gate-2 red test: 3 of 5 approvals is a strict majority (> 5/2 = 2) → quorum met.
    #[test]
    fn check_quorum_3_of_5_meets_strict_majority() {
        let votes = vec![
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", false),
            mk_vote("axtcb1-ext:aaa", false),
        ];
        let r = check_quorum(&votes, 5);
        assert!(r.quorum_met, "3/5 must meet strict majority: {r:?}");
        assert_eq!(r.approvals, 3);
        assert_eq!(r.coalition_size, 5);
        assert!(r.blocking_reason.is_none());
    }

    /// Gate-4 edge case 1: no votes at all → never met, whatever `required_n` is.
    #[test]
    fn check_quorum_empty_votes_not_met() {
        let votes: Vec<VoteResponse> = vec![];
        let r = check_quorum(&votes, 3);
        assert!(!r.quorum_met, "zero votes can never form a quorum: {r:?}");
        assert_eq!(r.approvals, 0);
        assert_eq!(r.coalition_size, 0);
        assert!(r.blocking_reason.is_some());
    }

    /// Gate-4 edge case 3 (THE off-by-one edge case): 2 of 4 approvals is
    /// EXACTLY half, which is NOT a strict majority (need > 4/2 = 2, i.e. >= 3).
    /// This is the case that separates "majority" from "quorum >= half" — a
    /// `>=` formula would wrongly accept this.
    #[test]
    fn check_quorum_2_of_4_fails_exact_half_edge_case() {
        let votes = vec![
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", false),
            mk_vote("axtcb1-ext:aaa", false),
        ];
        let r = check_quorum(&votes, 4);
        assert!(
            !r.quorum_met,
            "exactly half (2/4) must NOT meet strict majority: {r:?}"
        );
        assert_eq!(r.approvals, 2);
    }

    /// Gate-4 edge case 4: voters disagree on `voter_tcb` → a distinct
    /// "attestation mismatch" failure, NOT folded into "insufficient approvals".
    /// Even though all 3 votes approve (which would otherwise be a 3/3 majority),
    /// the mismatch must block and must say so — never silently pass.
    #[test]
    fn check_quorum_mismatched_voter_tcb_is_attest_fail_not_minority() {
        let votes = vec![
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:bbb", true), // different voter_tcb — the mismatch
            mk_vote("axtcb1-ext:aaa", true),
        ];
        let r = check_quorum(&votes, 3);
        assert!(
            !r.quorum_met,
            "a voter_tcb mismatch must block even with unanimous approval: {r:?}"
        );
        let reason = r.blocking_reason.expect("must carry a blocking reason");
        assert!(
            reason.contains("attestation") || reason.contains("mismatch"),
            "reason must name the attestation mismatch, not report a generic minority: {reason}"
        );
        assert!(
            !reason.contains("insufficient approvals"),
            "attestation mismatch must be a DISTINCT failure mode from insufficient approvals: {reason}"
        );
    }

    /// Gate-4 edge case 5: unanimous NO → not met.
    #[test]
    fn check_quorum_all_deny_not_met() {
        let votes = vec![
            mk_vote("axtcb1-ext:aaa", false),
            mk_vote("axtcb1-ext:aaa", false),
            mk_vote("axtcb1-ext:aaa", false),
        ];
        let r = check_quorum(&votes, 3);
        assert!(!r.quorum_met, "unanimous denial must not meet quorum: {r:?}");
        assert_eq!(r.approvals, 0);
    }

    /// Gate-4 edge case 7: a coalition of `ceil(N/2) - 1` approvals can never
    /// meet quorum, for a spread of fleet sizes (the ceiling property named in
    /// the R27/R33 design: no minority-sized bloc can force approval).
    #[test]
    fn coalition_ceil_n_over_2_minus_1_cannot_meet_quorum() {
        for n in 3usize..=12 {
            let ceil_half = n.div_ceil(2);
            let coalition = ceil_half - 1;
            let mut votes = Vec::new();
            for _ in 0..coalition {
                votes.push(mk_vote("axtcb1-ext:aaa", true));
            }
            for _ in coalition..n {
                votes.push(mk_vote("axtcb1-ext:aaa", false));
            }
            let r = check_quorum(&votes, n);
            assert!(
                !r.quorum_met,
                "N={n}: a coalition of ceil(N/2)-1={coalition} approvals must NOT meet quorum: {r:?}"
            );
        }
    }

    /// Determinism (spec A5, scoped to the pure aggregator): the same vote set
    /// in a different arrival order produces a byte-identical `QuorumResult`,
    /// because `check_quorum` only counts — it never depends on vote position.
    #[test]
    fn check_quorum_is_order_independent() {
        let a = vec![
            mk_vote("axtcb1-ext:aaa", true),
            mk_vote("axtcb1-ext:aaa", false),
            mk_vote("axtcb1-ext:aaa", true),
        ];
        let mut b = a.clone();
        b.reverse();
        let mut c = a.clone();
        c.swap(0, 1);

        let ra = check_quorum(&a, 3);
        let rb = check_quorum(&b, 3);
        let rc = check_quorum(&c, 3);
        assert_eq!(ra, rb, "reversed order must produce an identical QuorumResult");
        assert_eq!(ra, rc, "swapped order must produce an identical QuorumResult");
    }
}
