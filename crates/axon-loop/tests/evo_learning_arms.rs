//! C9 round 4c, EQGATE (amendment 81; M1930-M1933): the four EXCLUSION arms of
//! `evo::propose`'s `let reason = if .. { Some("..") }` chain. The chain decides
//! by `Some(reason)`, which the gate could not see, and two arms (the
//! mechanism-test arm, the `!learning_eligible` arm) could be disabled with the
//! whole axon-loop suite green.
//!
//! Each test sends ONE eligible discovery episode (the control: it is the
//! evidence) beside ONE episode that exactly one arm excludes, and asks for the
//! recorded exclusion and its reason.
//!
//! The mechanism-test arm is dominated for the exclusion itself (the
//! `!learning_eligible` arm excludes a mechanism-test episode too: the corpus
//! role is part of that predicate); what that arm decides is the RECORDED
//! reason, which is what a reader of the hypothesis history sees. Its row's
//! attack is therefore "an episode excluded under the wrong reason", stated
//! here and in amendment 81, not counted as more than that.
mod common;
use axon_loop::evo;
use axon_loop_contracts::*;
use common::*;

fn excluded_with(s: &axon_loop::Store, extra: serde_json::Value, who: &str) -> Vec<String> {
    let inc = incumbent();
    let req = evo::parse_request(
        &evo_request(
            &inc,
            7,
            "cand-arms",
            vec![discovery_episode(&inc, "d1"), extra],
        )
        .to_string(),
    )
    .unwrap();
    let p = evo::propose(s, &req).unwrap_or_else(|e| panic!("setup ({who}): {e}"));
    assert_eq!(
        p.candidate.discovery_evidence_refs.len(),
        1,
        "control ({who}): the eligible episode is the only evidence"
    );
    match p.hypothesis {
        evo::Hypothesis::Proposed { excluded, .. } => {
            excluded.into_iter().map(|e| e.reason).collect()
        }
        other => panic!("setup: {other:?}"),
    }
}

fn trial_episode(edit: impl FnOnce(&mut Trial)) -> serde_json::Value {
    let inc = incumbent();
    let mut t = Trial::new(&inc, "disc-task", "incumbent", "x1");
    t.role = CorpusRole::Discovery;
    t.created_ms = Some(1_000);
    edit(&mut t);
    trial(&t)["episode"].clone()
}

#[test]
fn a_mechanism_test_episode_is_excluded_as_one() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let r = excluded_with(
        &s,
        episode_with_role(&inc, "m1", CorpusRole::MechanismTest),
        "mechanism",
    );
    assert!(
        r.iter()
            .any(|x| x.contains("mechanism_test episodes never feed learning")),
        "ATTACK: a mechanism_test episode was excluded under another reason, or not at all: {r:?}"
    );
}

#[test]
fn an_episode_that_is_not_learning_eligible_is_excluded_as_such() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    // A discovery episode whose verification FAILED: eligible by role, issuer
    // and verifier, ineligible only by the predicate.
    let r = excluded_with(&s, trial_episode(|t| t.out = Out::Fail), "ineligible");
    assert!(
        r.iter().any(|x| x.contains("not learning-eligible")),
        "ATTACK: a discovery episode that failed verification was not excluded as \
         learning-ineligible: {r:?}"
    );
}

#[test]
fn an_episode_the_proposer_verified_itself_is_excluded_even_if_it_is_a_trusted_verifier() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let mut cfg = s.config().unwrap();
    cfg.trusted_verifiers
        .push(OpaqueRef::new(PROPOSER).unwrap());
    s.write_config(&cfg).unwrap();
    let r = excluded_with(
        &s,
        trial_episode(|t| t.verifier = PROPOSER),
        "self-verified",
    );
    assert!(
        r.iter()
            .any(|x| x.contains("verified by the proposer itself")),
        "ATTACK: an episode the proposer verified itself fed learning (or was excluded under \
         another reason): {r:?}"
    );
}

#[test]
fn an_episode_whose_verifier_is_not_trusted_is_excluded() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let r = excluded_with(
        &s,
        trial_episode(|t| t.verifier = "agent:stranger"),
        "untrusted",
    );
    assert!(
        r.iter()
            .any(|x| x.contains("not a configured trusted verifier")),
        "ATTACK: an episode verified by an issuer the operator never trusted fed learning (or \
         was excluded under another reason): {r:?}"
    );
}
