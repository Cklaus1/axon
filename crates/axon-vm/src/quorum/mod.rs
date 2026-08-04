//! R33 — cross-VM safety quorum: `VoteRequest`/`VoteResponse` + strict-majority
//! `check_quorum` aggregation (with the R27 per-lineage coalition ceiling folded
//! in — landed, see `logic::default_coalition_cap`).
//!
//! # Votes are NOT authenticated (AUDIT T49)
//!
//! This header used to say "attested VoteRequest/VoteResponse". Nothing verifies
//! either. A `.vote` file carries no signature or MAC, `collect_responses` reads
//! every `*.vote` in a directory, and `voter_tcb` is a string the vote declares
//! about itself. Executed against the CLI: three hand-written JSON files
//! produced `QUORUM MET: 3/3 approvals`, exit 0. Anyone who can write that
//! directory is the entire quorum.
//!
//! T49 closed the two holes that do NOT require key material — votes are bound
//! to the run they name, and the operator can pin the expected `voter_tcb` — but
//! authenticity needs signing, which needs a key-distribution decision this
//! module cannot make for the operator. Tracked as O035. Until then, treat the
//! responses directory as part of the TCB and say so in deployment docs: a
//! quorum whose transport anyone can write is a quorum of one.
//!
//! Scoped slice of `governance/specs/R33-cross-vm-safety-quorum.md`: a file-based
//! `propose`/`vote`/`check` CLI exchange with a pure aggregator. The vsock broadcast
//! transport (S2) is landing in sub-slices — `vsock` currently has only the wire
//! protocol (S2a); the real socket transport and broadcast/collect/listen loops are
//! not yet built (see that spec's §5.2.2 for exactly what remains open).

pub mod io;
pub mod logic;
pub mod vsock;

#[cfg(test)]
mod tests {
    use super::logic::{check_quorum, VoteResponse};
    use std::sync::atomic::{AtomicU64, Ordering};

    /// Every `mk_vote` call gets its OWN fresh `lineage_root` (a monotonic
    /// counter, thread-safe since `cargo test` runs tests in parallel) so the
    /// R27 coalition cap (folded into `check_quorum` itself — see its doc
    /// comment) is a no-op for every test in this file that doesn't care
    /// about lineage grouping: no two votes here ever accidentally share a
    /// root. `coalition_bound_limits_same_lineage` below is the one test that
    /// deliberately opts OUT of this via `mk_vote_with_root`, to exercise the
    /// cap on purpose.
    static NEXT_ROOT: AtomicU64 = AtomicU64::new(0);

    fn mk_vote(tcb: &str, approved: bool) -> VoteResponse {
        let root = NEXT_ROOT.fetch_add(1, Ordering::Relaxed);
        mk_vote_with_root(tcb, approved, &format!("auto-root-{root}"))
    }

    fn mk_vote_with_root(tcb: &str, approved: bool, lineage_root: &str) -> VoteResponse {
        VoteResponse {
            voter_tcb: tcb.to_string(),
            run_id: "r1".to_string(),
            approved,
            reason: String::new(),
            lineage_root: lineage_root.to_string(),
        }
    }

    #[test]
    fn votes_for_a_different_run_do_not_count_p4_os_12() {
        // AUDIT T49 (P4-OS-12 / P7-SEC-04 / P7-KRN-07). The proposal was not an
        // input to `check_quorum` at all, so votes naming DIFFERENT runs
        // aggregated into one decision. Executed against the CLI before the fix:
        //
        //   three .vote files naming "benign-dry-run", "some-other-job" and
        //   "deploy-prod"  ->  "QUORUM MET: 3/3 approvals", exit 0
        //
        // Honest approvals gathered for one action authorised another. This needs
        // no forgery and no key material to exploit — only a fleet that votes on
        // more than one thing.
        let mut a = mk_vote("axtcb1-ext:aaa", true);
        a.run_id = "benign-dry-run".to_string();
        let mut b = mk_vote("axtcb1-ext:aaa", true);
        b.run_id = "some-other-job".to_string();
        let mut c = mk_vote("axtcb1-ext:aaa", true);
        c.run_id = "deploy-prod".to_string();

        let r = check_quorum(&[a, b, c], 3, "deploy-prod", None);
        assert!(
            !r.quorum_met,
            "one real approval must not become three by borrowing other runs' votes: {r:?}"
        );
        assert_eq!(r.approvals, 1, "only the vote for this run counts: {r:?}");
        assert_eq!(r.coalition_size, 1);

        // A directory with NO vote for this run is blocked, and says so rather
        // than reporting a generic minority — the operator needs to know the
        // votes were for something else.
        let mut x = mk_vote("axtcb1-ext:aaa", true);
        x.run_id = "elsewhere".to_string();
        let r = check_quorum(&[x], 1, "deploy-prod", None);
        assert!(!r.quorum_met);
        let reason = r.blocking_reason.unwrap_or_default();
        assert!(
            reason.contains("no votes for run") && reason.contains("other runs"),
            "the reason must distinguish 'voted against' from 'voted on something else': {reason}"
        );
    }

    #[test]
    fn a_pinned_voter_tcb_rejects_self_declared_identities_t49() {
        // The consistency check only required votes to AGREE on `voter_tcb` — a
        // value each vote declares about itself. Three forged votes agreeing on
        // a made-up digest passed it. Pinning lets the operator state the
        // expected identity, the same shape as the T31/T32 `--expect-digest`
        // and `--pin-baseline` gates.
        let forged = |n: u8| {
            let mut v = mk_vote("axtcb1-ext:0000", true);
            v.run_id = "r1".to_string();
            v.lineage_root = format!("h{n}");
            v
        };
        let votes = vec![forged(1), forged(2), forged(3)];

        // Unpinned: they agree with each other, so the old check is satisfied.
        let r = check_quorum(&votes, 3, "r1", None);
        assert!(
            r.quorum_met,
            "without a pin, mutually-agreeing forged votes still pass — this is the \
             residual hole that only signing closes (see O035): {r:?}"
        );

        // Pinned to the real digest: refused, as an attestation failure.
        let r = check_quorum(&votes, 3, "r1", Some("axtcb1-ext:REAL"));
        assert!(
            !r.quorum_met,
            "a pinned TCB must reject forged identities: {r:?}"
        );
        let reason = r.blocking_reason.unwrap_or_default();
        assert!(
            reason.contains("attestation mismatch") && reason.contains("axtcb1-ext:REAL"),
            "the refusal must name the pinned value it expected: {reason}"
        );

        // NEGATIVE CONTROL: matching votes still pass with the pin in place.
        let honest = |n: u8| {
            let mut v = mk_vote("axtcb1-ext:REAL", true);
            v.run_id = "r1".to_string();
            v.lineage_root = format!("h{n}");
            v
        };
        let r = check_quorum(
            &[honest(1), honest(2), honest(3)],
            3,
            "r1",
            Some("axtcb1-ext:REAL"),
        );
        assert!(
            r.quorum_met,
            "honest votes matching the pin must still pass: {r:?}"
        );
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
        let r = check_quorum(&votes, 5, "r1", None);
        assert!(r.quorum_met, "3/5 must meet strict majority: {r:?}");
        assert_eq!(r.approvals, 3);
        assert_eq!(r.coalition_size, 5);
        assert!(r.blocking_reason.is_none());
    }

    /// Gate-4 edge case 1: no votes at all → never met, whatever `required_n` is.
    #[test]
    fn check_quorum_empty_votes_not_met() {
        let votes: Vec<VoteResponse> = vec![];
        let r = check_quorum(&votes, 3, "r1", None);
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
        let r = check_quorum(&votes, 4, "r1", None);
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
        let r = check_quorum(&votes, 3, "r1", None);
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
        let r = check_quorum(&votes, 3, "r1", None);
        assert!(
            !r.quorum_met,
            "unanimous denial must not meet quorum: {r:?}"
        );
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
            let r = check_quorum(&votes, n, "r1", None);
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

        let ra = check_quorum(&a, 3, "r1", None);
        let rb = check_quorum(&b, 3, "r1", None);
        let rc = check_quorum(&c, 3, "r1", None);
        assert_eq!(
            ra, rb,
            "reversed order must produce an identical QuorumResult"
        );
        assert_eq!(
            ra, rc,
            "swapped order must produce an identical QuorumResult"
        );
    }

    /// R33 spec §4.5 / §7's own worked example, verbatim: 3 verified YES
    /// votes, ALL sharing one `lineage_root` (a "sock puppet" coalition —
    /// N instances minted from the same R27 principal, all voting YES).
    /// Without the coalition cap this would be a trivial 3/3 majority; WITH
    /// it (default cap for N=3 is `ceil(3/2)-1 = 1`), only 1 of the 3 YES
    /// votes is admitted, so admitted approvals = 1, which is NOT a strict
    /// majority of 3 (need > 3/2 = 1, i.e. >= 2) — quorum is blocked. This is
    /// the R27 sock-puppet attack R33 §4.5 exists to close: spinning up N
    /// VMs from one lineage root cannot alone force approval.
    #[test]
    fn coalition_bound_limits_same_lineage() {
        let votes = vec![
            mk_vote_with_root("axtcb1-ext:aaa", true, "sockpuppet-root"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "sockpuppet-root"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "sockpuppet-root"),
        ];
        let r = check_quorum(&votes, 3, "r1", None);
        assert!(
            !r.quorum_met,
            "3 YES votes from ONE lineage root must not alone form quorum: {r:?}"
        );
        assert_eq!(
            r.approvals, 1,
            "only cap=ceil(3/2)-1=1 YES vote from a single root may be admitted: {r:?}"
        );
        let reason = r.blocking_reason.expect("must carry a blocking reason");
        assert!(
            reason.contains("coalition"),
            "must name the coalition cap as the cause, not a generic minority: {reason}"
        );
    }

    /// The coalition cap must not trip on legitimately DISTINCT lineage
    /// roots: 3 YES votes from 3 different roots (N=3, no single root
    /// exceeding cap=1) must meet quorum exactly like the un-capped case.
    #[test]
    fn coalition_cap_does_not_block_distinct_lineage_roots() {
        let votes = vec![
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-a"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-b"),
            mk_vote_with_root("axtcb1-ext:aaa", false, "root-c"),
        ];
        let r = check_quorum(&votes, 3, "r1", None);
        assert!(
            r.quorum_met,
            "2 YES votes from 2 DISTINCT roots (cap=1 each, not exceeded) must meet quorum: {r:?}"
        );
        assert_eq!(r.approvals, 2);
    }

    /// Coalition cap at EVEN N — previously untested (every prior coalition
    /// test used N=3, odd). The spec's own prose ("default max_quorum_power
    /// is one fewer than the quorum threshold") and its worked numeric
    /// example (`ceil(N/2)-1`) only provably agree for ODD N (where
    /// `ceil(N/2) == floor(N/2)+1 == quorum_threshold`); for even N they
    /// diverge (`ceil(N/2) != quorum_threshold`). This pins the
    /// `ceil(N/2)-1` formula's actual behavior at even N=4 and N=6, so a
    /// future change to the formula can't silently regress the untested
    /// case — the exact gap a decision audit flagged after landing the
    /// coalition cap.
    #[test]
    fn coalition_cap_at_even_n_sockpuppet_blocked() {
        // N=4: quorum threshold = 4/2+1 = 3 (need > 2). cap = ceil(4/2)-1 = 1.
        // 4 sock-puppet YES votes, one shared root -> only 1 admitted -> blocked.
        let votes_n4: Vec<VoteResponse> = (0..4)
            .map(|_| mk_vote_with_root("axtcb1-ext:aaa", true, "sockpuppet-root"))
            .collect();
        let r4 = check_quorum(&votes_n4, 4, "r1", None);
        assert!(
            !r4.quorum_met,
            "N=4: 4 sock-puppet YES votes from one root must not meet quorum: {r4:?}"
        );
        assert_eq!(
            r4.approvals, 1,
            "N=4: cap=ceil(4/2)-1=1 admitted YES vote: {r4:?}"
        );
        assert!(
            r4.blocking_reason
                .as_deref()
                .unwrap_or("")
                .contains("coalition"),
            "N=4: must name the coalition cap, not a generic minority: {r4:?}"
        );

        // N=6: quorum threshold = 6/2+1 = 4 (need > 3). cap = ceil(6/2)-1 = 2.
        // 6 sock-puppet YES votes, one shared root -> only 2 admitted -> blocked.
        let votes_n6: Vec<VoteResponse> = (0..6)
            .map(|_| mk_vote_with_root("axtcb1-ext:aaa", true, "sockpuppet-root"))
            .collect();
        let r6 = check_quorum(&votes_n6, 6, "r1", None);
        assert!(
            !r6.quorum_met,
            "N=6: 6 sock-puppet YES votes from one root must not meet quorum: {r6:?}"
        );
        assert_eq!(
            r6.approvals, 2,
            "N=6: cap=ceil(6/2)-1=2 admitted YES votes: {r6:?}"
        );
        assert!(
            r6.blocking_reason
                .as_deref()
                .unwrap_or("")
                .contains("coalition"),
            "N=6: must name the coalition cap, not a generic minority: {r6:?}"
        );
    }

    /// Control for the even-N case: the SAME vote counts from DISTINCT
    /// lineage roots (no single root exceeding its cap) must meet quorum
    /// normally — proves the even-N cap doesn't over-trigger either.
    #[test]
    fn coalition_cap_at_even_n_distinct_roots_meets_quorum() {
        // N=4: 3 YES votes from 3 distinct roots (each count=1 <= cap=1) -> all
        // admitted -> approvals=3 > 4/2=2 -> quorum met.
        let votes_n4 = vec![
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-a"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-b"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-c"),
            mk_vote_with_root("axtcb1-ext:aaa", false, "root-d"),
        ];
        let r4 = check_quorum(&votes_n4, 4, "r1", None);
        assert!(
            r4.quorum_met,
            "N=4: 3 distinct-root YES votes (cap=1 each, not exceeded) must meet quorum: {r4:?}"
        );
        assert_eq!(r4.approvals, 3);

        // N=6: 4 YES votes from 4 distinct roots (each count=1 <= cap=2) -> all
        // admitted -> approvals=4 > 6/2=3 -> quorum met.
        let votes_n6 = vec![
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-a"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-b"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-c"),
            mk_vote_with_root("axtcb1-ext:aaa", true, "root-d"),
            mk_vote_with_root("axtcb1-ext:aaa", false, "root-e"),
            mk_vote_with_root("axtcb1-ext:aaa", false, "root-f"),
        ];
        let r6 = check_quorum(&votes_n6, 6, "r1", None);
        assert!(
            r6.quorum_met,
            "N=6: 4 distinct-root YES votes (cap=2 each, not exceeded) must meet quorum: {r6:?}"
        );
        assert_eq!(r6.approvals, 4);
    }

    /// Legacy `.vote` JSON predating the `lineage_root` field: multiple such
    /// votes all deserialize to the SAME empty-string default (see the
    /// field's own doc comment) and therefore coalesce into one coalition
    /// bucket under the cap — previously reasoned about but never actually
    /// exercised end-to-end (the one existing hand-written-JSON fixture
    /// missing this field short-circuits earlier on attestation mismatch,
    /// never reaching the coalition-cap code at all). This pins the real,
    /// intended behavior: legacy votes are treated as ONE undeclared
    /// coalition (fail-closed), not N independent voters (fail-open).
    #[test]
    fn legacy_votes_missing_lineage_root_share_one_capped_bucket() {
        // Deserialize from raw JSON with NO lineage_root field at all —
        // exactly what a pre-R33-coalition-ceiling .vote file looks like.
        let json_without_field = |approved: bool| -> VoteResponse {
            let raw = format!(
                r#"{{"voter_tcb":"axtcb1-ext:aaa","run_id":"legacy","approved":{approved},"reason":""}}"#
            );
            serde_json::from_str(&raw)
                .expect("legacy .vote JSON (no lineage_root) must still parse")
        };
        let votes = vec![
            json_without_field(true),
            json_without_field(true),
            json_without_field(true),
        ];
        assert!(
            votes.iter().all(|v| v.lineage_root.is_empty()),
            "legacy votes must default lineage_root to the empty-string sentinel"
        );

        // The fixture's run_id is "legacy" (T49: votes are now bound to the run
        // they name, so the check must ask about that run).
        let r = check_quorum(&votes, 3, "legacy", None);
        assert!(
            !r.quorum_met,
            "3 legacy (no lineage_root) YES votes coalesce into ONE bucket and must not force quorum: {r:?}"
        );
        assert_eq!(
            r.approvals, 1,
            "cap=ceil(3/2)-1=1 applies to the shared empty-string bucket: {r:?}"
        );
        assert!(
            r.blocking_reason
                .as_deref()
                .unwrap_or("")
                .contains("coalition"),
            "must name the coalition cap as the cause: {r:?}"
        );
    }
}
