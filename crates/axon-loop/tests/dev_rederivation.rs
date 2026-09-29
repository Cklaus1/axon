//! A DEVELOPMENT-class decision re-checks the operator's pins every time it is
//! re-derived: at activation and at rollback, not only when it was admitted.
//!
//! The protected class re-verifies its counted verdicts from their stored
//! documents (`reverify_protected`), which re-runs the pin check. The
//! development class does not, so the derive-time `check_pins` in admission is
//! its ONLY re-derivation pin check. The Candidate-8 certifying review
//! (wf_ae3a5a74-41e) showed that this guard (mutation M103) had been retired as
//! "equivalent" on the strength of a protected-class test: with it removed, a
//! development candidate ACTIVATED after the operator withdrew the profile,
//! changed the revision pin, removed the pin or cleared the task acceptance, and
//! a rollback landed on a withdrawn profile. The whole axon-loop suite stayed
//! green, because no development-class test exercised the route. These are
//! those tests. G11-r22-rollback-revalidate records the same development half.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::pointer;
use axon_loop_contracts::*;
use common::*;

/// The refusal must be THIS guard's, not some other check's.
fn assert_unpinned(label: &str, r: Result<impl std::fmt::Debug, LoopError>) {
    match r {
        Err(LoopError::Refused(msg)) => assert!(
            msg.contains("no longer pinned by the operator"),
            "{label}: refused, but not by the re-derivation pin check: {msg}"
        ),
        other => panic!("{label}: a DEVELOPMENT decision re-derived on a withdrawn pin: {other:?}"),
    }
}

/// Admit a development-class candidate, then let the operator change the config.
fn admitted_then(withdraw: impl FnOnce(&mut axon_loop::store::Config)) -> (World, Ref) {
    let w = world();
    freeze_plan(&w.s, "dev", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    assign_specs(&w.s, "dev", &specs);
    let (_, e) = evaluate(&w.s, &evl_request("dev", &w.inc, &w.cand, &specs, &EvlOpts::default())).unwrap();
    let (rec, adm) = admit(&w.s, "dev", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "setup: {:?}", rec.reasons);
    let mut cfg = w.s.config().unwrap();
    withdraw(&mut cfg);
    w.s.write_config(&cfg).unwrap();
    (w, adm)
}

fn activate(w: &World, adm: &Ref) -> Result<impl std::fmt::Debug, LoopError> {
    pointer::transition(
        &w.s,
        &tparse(&transition("a1", "activate", &w.inc_ref, Some(&w.cand_ref), 1, Some(adm), false)),
    )
    .map(|p| p.active_policy_ref)
}

fn verifier() -> OpaqueRef {
    OpaqueRef::new(VERIFIER).unwrap()
}

#[test]
fn a_development_activation_after_the_profile_is_withdrawn_is_refused() {
    let (w, adm) = admitted_then(|c| c.verifier_pins.get_mut(&verifier()).unwrap().backend_profiles.clear());
    assert_unpinned("profile withdrawn", activate(&w, &adm));
}

#[test]
fn a_development_activation_after_the_revision_pin_changes_is_refused() {
    let (w, adm) = admitted_then(|c| {
        c.verifier_pins.get_mut(&verifier()).unwrap().executable_digest = format!("acf1:{}", "f".repeat(64))
    });
    assert_unpinned("revision changed", activate(&w, &adm));
}

#[test]
fn a_development_activation_after_the_pin_is_removed_is_refused() {
    let (w, adm) = admitted_then(|c| c.verifier_pins.clear());
    assert_unpinned("pin removed", activate(&w, &adm));
}

#[test]
fn a_development_activation_after_the_task_acceptance_is_cleared_is_refused() {
    let (w, adm) = admitted_then(|c| c.task_acceptance.clear());
    assert_unpinned("acceptance cleared", activate(&w, &adm));
}

/// Control: with nothing withdrawn, the same development activation succeeds,
/// so each refusal above is the withdrawal being noticed.
#[test]
fn a_development_activation_with_its_pins_intact_succeeds() {
    let (w, adm) = admitted_then(|_| {});
    activate(&w, &adm).expect("control: an unwithdrawn development activation must succeed");
}

/// G11-r22-rollback-revalidate, development class: a rollback onto a candidate
/// whose profile the operator has since withdrawn is refused.
#[test]
fn a_development_rollback_onto_a_withdrawn_profile_is_refused() {
    let (w, adm) = admitted_then(|_| {});
    let t = |id: &str, k: &str, from: &Ref, to: &Ref, ep: u64, a: &Ref| {
        tparse(&transition(id, k, from, Some(to), ep, Some(a), false))
    };
    pointer::transition(&w.s, &t("a1", "activate", &w.inc_ref, &w.cand_ref, 1, &adm)).unwrap();
    pointer::transition(&w.s, &t("r1", "rollback", &w.cand_ref, &w.inc_ref, 2, &w.baseline)).unwrap();
    let mut cfg = w.s.config().unwrap();
    cfg.verifier_pins.get_mut(&verifier()).unwrap().backend_profiles.clear();
    w.s.write_config(&cfg).unwrap();
    let r = pointer::transition(&w.s, &t("r2", "rollback", &w.inc_ref, &w.cand_ref, 3, &adm))
        .map(|p| p.active_policy_ref);
    assert_unpinned("rollback onto a withdrawn profile", r);
}
