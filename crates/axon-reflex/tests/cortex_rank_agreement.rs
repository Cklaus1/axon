//! `cortex repair`'s ranking step and `shortlist::decide` judge a provider's
//! output by the same rules.
//!
//! Cortex cannot call `decide` (this crate depends on `axon-cortex`, so the
//! reverse edge would be a cycle); `axon_cortex::rank::validate_ranking`
//! mirrors its abstain rules instead. This feeds both the same outputs so the
//! mirror cannot drift silently. There is exactly one intended difference:
//! `decide` accepts a SUBSET (a provider may remove ids), while Cortex refuses
//! anything that is not a full permutation, because a dropped candidate is one
//! the repair walk would never try.

use axon_cortex::rank::validate_ranking;
use axon_loop_contracts::{CandidateId, Ref, Scope, TaskFamily, TenantId};
use axon_reflex::shortlist::{
    decide, AbstainReason, DecisionRequest, DecisionResult, ProviderError, ProviderId,
    ProviderView, ShortlistProvider, TaskFeatures,
};
use std::collections::BTreeSet;

const ELIGIBLE: [&str; 3] = ["alpha", "beta", "gamma"];

/// Returns the scripted ids, best first, with descending ordinal scores — the
/// shape a model ranking becomes.
struct Scripted(Vec<&'static str>);

impl ShortlistProvider for Scripted {
    fn id(&self) -> ProviderId {
        ProviderId::new("cortex-model-rank").unwrap()
    }
    fn version(&self) -> String {
        "agreement-test/1".into()
    }
    fn evidence_refs(&self) -> Vec<Ref> {
        Vec::new()
    }
    fn rank(&self, _: &ProviderView<'_>) -> Result<Vec<(CandidateId, i64)>, ProviderError> {
        let n = self.0.len() as i64;
        Ok(self
            .0
            .iter()
            .enumerate()
            .map(|(i, c)| (CandidateId::new(*c).unwrap(), n - i as i64))
            .collect())
    }
}

fn reflex(out: &[&'static str]) -> DecisionResult {
    let r = |c: char| Ref::new(format!("cl22:{}", c.to_string().repeat(64))).unwrap();
    let eligible: BTreeSet<CandidateId> = ELIGIBLE
        .iter()
        .map(|c| CandidateId::new(*c).unwrap())
        .collect();
    let req = DecisionRequest::new(
        Scope {
            tenant_id: TenantId::new("t1").unwrap(),
            task_family: TaskFamily::new("axon-repair").unwrap(),
        },
        eligible,
        r('a'),
        r('b'),
        TaskFeatures::default(),
        ProviderId::new("cortex-model-rank").unwrap(),
    )
    .unwrap();
    decide(&req, &Scripted(out.to_vec()))
}

fn cortex(out: &[&str]) -> Result<Vec<String>, String> {
    let eligible: Vec<String> = ELIGIBLE.iter().map(|s| s.to_string()).collect();
    let out: Vec<String> = out.iter().map(|s| s.to_string()).collect();
    validate_ranking(&eligible, &out)
}

#[test]
fn cortex_and_decide_accept_the_same_permutations_in_the_same_order() {
    for out in [
        vec!["alpha", "beta", "gamma"],
        vec!["gamma", "alpha", "beta"],
        vec!["beta", "gamma", "alpha"],
    ] {
        let ids: Vec<String> = reflex(&out)
            .ids()
            .expect("decide accepts a permutation")
            .into_iter()
            .map(|c| c.as_str().to_string())
            .collect();
        assert_eq!(cortex(&out).expect("cortex accepts it too"), ids);
    }
}

#[test]
fn cortex_and_decide_refuse_the_same_outputs_for_the_same_reason() {
    let cases: Vec<(Vec<&'static str>, AbstainReason, &str)> = vec![
        (
            vec!["gamma", "delta", "alpha"],
            AbstainReason::NotEligible(CandidateId::new("delta").unwrap()),
            "`delta` is not an eligible candidate",
        ),
        (
            vec!["gamma", "gamma", "alpha"],
            AbstainReason::Duplicate(CandidateId::new("gamma").unwrap()),
            "`gamma` is ranked twice",
        ),
        // First offence wins in both: the duplicate precedes the stranger.
        (
            vec!["beta", "beta", "delta"],
            AbstainReason::Duplicate(CandidateId::new("beta").unwrap()),
            "`beta` is ranked twice",
        ),
        (vec![], AbstainReason::EmptyOutput, "empty ranking"),
    ];
    for (out, want_reflex, want_cortex) in cases {
        match reflex(&out) {
            DecisionResult::Abstain { reason, .. } => assert_eq!(reason, want_reflex, "{out:?}"),
            other => panic!("decide should abstain on {out:?}: {other:?}"),
        }
        assert_eq!(cortex(&out), Err(want_cortex.to_string()), "{out:?}");
    }
}

/// THE ONE INTENDED DIFFERENCE. A valid subset is a shortlist to `decide`
/// (a provider may remove ids) and a refusal to Cortex (the walk would never
/// try the removed candidate).
#[test]
fn a_valid_subset_is_a_shortlist_to_decide_and_a_refusal_to_cortex() {
    for out in [vec!["gamma", "alpha"], vec!["beta"]] {
        assert!(
            reflex(&out).is_shortlist(),
            "decide accepts the subset {out:?}"
        );
        let err = cortex(&out).expect_err("cortex refuses anything but a permutation");
        assert_eq!(
            err,
            format!("incomplete ranking: {} of 3 candidates", out.len())
        );
    }
}
