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
///
/// `lineage_root` is the R27 coalition-ceiling grouping key (spec §4.5): the
/// `PrincipalId` this voter was minted from. It is a SEPARATE concept from
/// `voter_tcb` — `voter_tcb` identifies *what software* is running (and, in a
/// healthy fleet, is expected to be IDENTICAL across every honest voter),
/// while `lineage_root` identifies *which principal/coalition* the voter
/// belongs to (and should differ across independent voters). Conflating the
/// two would be backwards: defaulting an unset `lineage_root` to `voter_tcb`
/// would make every honestly-attested voter in a fleet look like one giant
/// coalition, capping legitimate quorums. See `cmd_quorum_vote` for the
/// actual default (a fresh per-invocation value, not `voter_tcb`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VoteResponse {
    pub voter_tcb: String,
    pub run_id: String,
    pub approved: bool,
    pub reason: String,
    /// `#[serde(default)]` (empty string) so a `.vote` file written before
    /// this field existed still parses — deliberately a SHARED sentinel, not
    /// a per-instance-unique one (this module is pure per its own header, no
    /// clock/pid access here; `cmd_quorum_vote` is where a real fresh unique
    /// value gets generated for the CLI's own default). Multiple old-format
    /// votes all defaulting to `""` will coalesce into one coalition bucket
    /// and get capped together — a conservative, fail-closed reading of
    /// missing lineage data, not a fail-open one.
    #[serde(default)]
    pub lineage_root: String,
}

/// The aggregator's decision (R33 §3.3, scoped).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct QuorumResult {
    pub quorum_met: bool,
    pub coalition_size: usize,
    pub approvals: usize,
    pub blocking_reason: Option<String>,
}

/// The R27 per-lineage-root coalition ceiling (spec §4.5), applied before
/// counting approvals: no single `lineage_root` may contribute more than this
/// many admitted YES votes, regardless of how many voters actually share it.
///
/// Default value, per the spec's own concrete worked example (§4.5 point 4 /
/// the `coalition_bound_limits_same_lineage` test): `ceil(N/2) - 1` — for
/// N=3 this is `ceil(3/2) - 1 = 1`. (The spec's prose paraphrase, "one fewer
/// than the quorum threshold," is exact for odd N but not for even N; the
/// worked numeric example is the load-bearing definition, followed here.)
/// This is the spec's own explicitly-blessed fallback when R27's real
/// `Coalition`/`max_quorum_power` type isn't wired in yet (§6 slice-risk
/// note S4, §-Gate anti-stub carve-out) — not a corner cut.
fn default_coalition_cap(required_n: usize) -> u64 {
    required_n.div_ceil(2).saturating_sub(1) as u64
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
///
/// Before counting, applies the R27 per-lineage coalition ceiling (spec
/// §4.5): YES votes are grouped by `lineage_root`, and any root's admitted
/// YES count is capped at [`default_coalition_cap`] — excess YES votes from
/// an over-represented root are excluded from `approvals`, exactly as if
/// they had voted NO, so a coalition sharing one root can never alone force
/// a quorum (I-4). This is enforced unconditionally (not opt-in): callers
/// that don't care about lineage grouping simply give every vote a distinct
/// `lineage_root` (the default `cmd_quorum_vote` CLI behavior — see its own
/// doc comment), for which the cap is a no-op.
/// AUDIT T49 (findings P4-OS-12 / P7-SEC-04 / P7-KRN-07). `run_id` and
/// `expect_tcb` are new parameters, and both close an executed hole:
///
///   * **Cross-proposal aggregation.** The request was not an input at all, so
///     votes for DIFFERENT `run_id`s aggregated into one decision. Executed:
///     three votes naming `benign-dry-run`, `some-other-job` and `deploy-prod`
///     returned `QUORUM MET: 3/3 approvals`, exit 0. Honest approvals gathered
///     for one action authorised another. A vote must now name the run it is
///     voting on; anything else is not counted.
///   * **Self-declared identity.** The consistency check only required the
///     votes to AGREE on `voter_tcb`, so three forged votes agreeing on a made-up
///     digest passed it. `expect_tcb` lets the operator PIN the expected identity,
///     the same shape as the T31/T32 `--expect-digest` / `--pin-baseline` gates.
///
/// This does NOT make votes authentic — see the module header. A `.vote` file
/// carries no signature, so anyone who can write the responses directory can
/// still manufacture approvals; pinning raises the bar (the attacker must know
/// the real TCB digest and the real run id) but is not a substitute for signing.
pub fn check_quorum(
    votes: &[VoteResponse],
    required_n: usize,
    run_id: &str,
    expect_tcb: Option<&str>,
) -> QuorumResult {
    // Bind to the proposal FIRST. A vote that names a different run is not a
    // vote on this action; it is not counted, in either direction.
    let foreign = votes.iter().filter(|v| v.run_id != run_id).count();
    let votes: Vec<VoteResponse> = votes
        .iter()
        .filter(|v| v.run_id == run_id)
        .cloned()
        .collect();
    let votes = &votes[..];
    let coalition_size = votes.len();

    if votes.is_empty() {
        return QuorumResult {
            quorum_met: false,
            coalition_size: 0,
            approvals: 0,
            blocking_reason: Some(format!(
                "no votes for run `{run_id}` ({foreign} vote(s) present for other runs)"
            )),
        };
    }

    // Pinned identity, when the operator supplied one. Checked before the
    // agreement test below: "they all agree" is worth nothing if what they agree
    // on is a value the voters chose for themselves.
    if let Some(expected) = expect_tcb {
        let wrong: Vec<&str> = votes
            .iter()
            .filter(|v| v.voter_tcb != expected)
            .map(|v| v.voter_tcb.as_str())
            .collect();
        if !wrong.is_empty() {
            return QuorumResult {
                quorum_met: false,
                coalition_size,
                approvals: votes.iter().filter(|v| v.approved).count(),
                blocking_reason: Some(format!(
                    "attestation mismatch: {} vote(s) do not match the pinned voter_tcb \
                     `{expected}` (saw: {})",
                    wrong.len(),
                    wrong.join(", ")
                )),
            };
        }
    }

    // Attestation consistency FIRST, independent of the approval count: every
    // vote that arrived must agree on `voter_tcb`. In this fleet, all voters
    // are provisioned to run the same expected image (R31 axtcb1-ext: is
    // pinned per deployment, not per voter slot, in this scoped module) — a
    // disagreement means at least one voter is not the software this quorum
    // trusts, which is a strictly more serious signal than being outvoted, so
    // it is reported as a distinct failure mode and short-circuits the count.
    let mut distinct_tcbs: Vec<&str> = Vec::new();
    for v in votes.iter() {
        if !distinct_tcbs.contains(&v.voter_tcb.as_str()) {
            distinct_tcbs.push(v.voter_tcb.as_str());
        }
    }
    if distinct_tcbs.len() > 1 {
        let approvals = votes.iter().filter(|v| v.approved).count();
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

    // Coalition ceiling: cap admitted YES votes per lineage_root BEFORE
    // counting approvals (spec §-Notes: "the cap happens at the door," i.e.
    // before any CoordGoal-equivalent proposal — there is no separate
    // "admit then retract" step here since this aggregator counts directly).
    let cap = default_coalition_cap(required_n);
    let mut admitted_per_root: std::collections::HashMap<&str, u64> =
        std::collections::HashMap::new();
    let mut capped_out = 0usize;
    let mut approvals = 0usize;
    for v in votes {
        if !v.approved {
            continue;
        }
        let count = admitted_per_root
            .entry(v.lineage_root.as_str())
            .or_insert(0);
        if *count >= cap {
            capped_out += 1;
            continue;
        }
        *count += 1;
        approvals += 1;
    }

    // Strict majority: approvals > required_n / 2 (integer floor) — NOT >=,
    // which would wrongly admit an exactly-half coalition at even N.
    let quorum_met = approvals > required_n / 2;
    let blocking_reason = if quorum_met {
        None
    } else if capped_out > 0 {
        Some(format!(
            "coalition cap: {capped_out} YES vote(s) excluded (lineage-root cap={cap}); \
             {approvals}/{required_n} admitted (need > {required_n}/2)"
        ))
    } else {
        Some(format!(
            "insufficient approvals: {approvals}/{required_n} (need > {}/2)",
            required_n
        ))
    };

    QuorumResult {
        quorum_met,
        coalition_size,
        approvals,
        blocking_reason,
    }
}
