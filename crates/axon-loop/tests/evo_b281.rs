//! B281 holdout separation at its only reader (C9 round 4c, r4c-fixes part 2,
//! amendment 71). `evo::propose` is the one place the discovery evidence's
//! CORPUS ROLE is judged before a candidate is built: plan::check_candidate
//! re-judges the shortlist, parent and scope, never the evidence roles, and
//! admission then counts Confirmation-role trials for the same candidate. So a
//! protected-role episode the proposer saw would be both what proposed a
//! candidate and what admits it. Each attack goes through `evo::propose`, the
//! function the `axon-loop evo propose` verb hands its request to; the
//! CONTROL is the same request with only eligible discovery evidence.
mod common;
use axon_loop::error::LoopError;
use axon_loop::evo;
use axon_loop_contracts::*;
use common::*;

fn propose(s: &axon_loop::Store, eps: Vec<serde_json::Value>) -> Result<evo::Proposal, LoopError> {
    let req = evo::parse_request(&evo_request(&incumbent(), 7, "cand-b281", eps).to_string())?;
    evo::propose(s, &req)
}

/// The control: one eligible discovery episode proposes, with that episode
/// as the only evidence.
fn control() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = propose(&s, vec![discovery_episode(&incumbent(), "d1")])
        .unwrap_or_else(|e| panic!("control: an eligible discovery episode proposes: {e}"));
    assert_eq!(
        p.candidate.discovery_evidence_refs.len(),
        1,
        "control: the evidence"
    );
}

/// A Confirmation- or Reporting-role episode in the request refuses the WHOLE
/// request: protected outcomes never reach the proposer, even beside
/// eligible discovery evidence.
#[test]
fn a_protected_role_episode_never_reaches_the_proposer() {
    for role in [CorpusRole::Confirmation, CorpusRole::Reporting] {
        let d = tempfile::tempdir().unwrap();
        let s = store_with_config(d.path());
        let inc = incumbent();
        let before = snapshot(d.path());
        match propose(
            &s,
            vec![
                discovery_episode(&inc, "d1"),
                episode_with_role(&inc, "p1", role),
            ],
        ) {
            Err(LoopError::Refused(m)) => {
                assert!(m.contains("never reach the proposer"), "{m}");
                assert_eq!(snapshot(d.path()), before, "a refused proposal wrote");
            }
            Ok(p) => panic!(
                "ATTACK: a {role:?}-role episode reached the proposer as discovery evidence \
                 ({} evidence refs): the candidate it proposes would be admitted on the same \
                 protected outcomes",
                p.candidate.discovery_evidence_refs.len()
            ),
            Err(e) => panic!("setup: the {role:?} request was refused on something else: {e}"),
        }
    }
    control();
}

/// An episode of another scope (task family) never feeds this scope's
/// proposer: the whole request is refused.
#[test]
fn an_episode_of_another_scope_never_reaches_the_proposer() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    // An episode exactly like an eligible one, but of another task family.
    let mut foreign = discovery_episode(&inc, "x1");
    foreign["scope"]["task_family"] = serde_json::json!("another-family");
    assert_ne!(
        foreign["scope"],
        serde_json::to_value(scope()).unwrap(),
        "setup: the episode names another scope"
    );
    match propose(&s, vec![discovery_episode(&inc, "d1"), foreign]) {
        Err(LoopError::Refused(m)) => assert!(m.contains("different scope"), "{m}"),
        Ok(p) => panic!(
            "ATTACK: an episode of another scope fed this scope's proposer ({} evidence refs)",
            p.candidate.discovery_evidence_refs.len()
        ),
        Err(e) => panic!("setup: refused on something else: {e}"),
    }
    control();
}

/// A request whose every episode is excluded (mechanism-test only) proposes
/// nothing: a candidate is never grounded on no eligible evidence.
#[test]
fn a_proposal_needs_eligible_discovery_evidence() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    match propose(
        &s,
        vec![episode_with_role(&inc, "m1", CorpusRole::MechanismTest)],
    ) {
        Err(LoopError::Refused(m)) => assert!(m.contains("nothing to propose from"), "{m}"),
        Ok(p) => panic!(
            "ATTACK: EVO proposed a candidate from no learning-eligible evidence ({} refs, \
             excluded {:?})",
            p.candidate.discovery_evidence_refs.len(),
            match &p.hypothesis {
                evo::Hypothesis::Proposed { excluded, .. } => excluded.len(),
                _ => 0,
            }
        ),
        Err(e) => panic!("setup: refused on something else: {e}"),
    }
    control();
}
