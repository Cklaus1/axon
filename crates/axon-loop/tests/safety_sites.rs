//! Amendment 61 (C9 round 4b, rows4a): `safety::report`'s refusals that had
//! no mutation row, judged on the production route (`safety::report`, then
//! the admission it feeds). Each test is an ATTACK with its CONTROL; each
//! refusal writes nothing.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::safety;
use axon_loop_contracts::OpaqueRef;
use common::*;
use serde_json::Value;

/// A frozen dev experiment whose candidate would be ACCEPTED, its trials
/// intaken: `(world, request)`.
fn acceptable(exp: &str) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    (w, v)
}

fn candidate_trial(v: &Value) -> Value {
    v["trials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        .unwrap()
        .clone()
}

fn report(w: &World, r: &Value, sig: Option<&Value>) -> Result<(), LoopError> {
    safety::report(&w.s, &r.to_string(), sig.map(|s| s.to_string()).as_deref()).map(|_| ())
}

fn decision(w: &World, exp: &str, v: &Value) -> Decision {
    let (_, e) = evaluate(&w.s, v).unwrap();
    admit(&w.s, exp, &e, ADMITTER, false).unwrap().0.decision
}

/// A stranger (neither a trusted monitor nor a subject of the trial) reports
/// the candidate's trial unsafe: refused, writing nothing, and the candidate
/// is still ACCEPTED. Control: the trial's own observer (a subject) may
/// incriminate it, which vetoes.
#[test]
fn a_strangers_violation_never_vetoes_a_candidate() {
    let (w, v) = acceptable("sv");
    let t = candidate_trial(&v);
    let before = snapshot(w.dir.path());
    let r = safety_report(
        &t,
        "violation",
        Some("policy_violation"),
        "fixture:stranger",
    );
    match report(&w, &r, None) {
        Ok(()) => panic!("ATTACK: a stranger's violation report was recorded"),
        Err(e) => assert!(
            e.to_string()
                .contains("neither a trusted monitor nor a subject"),
            "{e}"
        ),
    }
    assert_eq!(snapshot(w.dir.path()), before);
    assert_eq!(decision(&w, "sv", &v), Decision::Accept);
    let (w, v) = acceptable("sv-control");
    let own = safety_report(
        &candidate_trial(&v),
        "violation",
        Some("policy_violation"),
        OBSERVER,
    );
    report(&w, &own, None).expect("control: a subject incriminates its own trial");
    assert_ne!(
        decision(&w, "sv-control", &v),
        Decision::Accept,
        "control: vetoed"
    );
}

/// A subject of the trial (the candidate policy's proposer), registered as a
/// trusted monitor under a key the operator holds and signing genuinely, may
/// not clear its own trial. Control: the independent monitor's clearance is
/// recorded.
#[test]
fn a_subject_keyed_as_a_monitor_never_clears_its_own_trial() {
    let (w, v) = acceptable("sc");
    trust_monitor(&w.s);
    let t = candidate_trial(&v);
    let mut cfg = w.s.config().unwrap();
    let prop = OpaqueRef::new(PROPOSER).unwrap();
    cfg.trusted_monitors.push(prop.clone());
    cfg.monitor_keys
        .insert(prop.clone(), monitor_key().1.clone());
    w.s.write_config(&cfg).unwrap();
    let own = safety_report(&t, "clear", None, PROPOSER);
    let sig = axon_loop_contracts::attestation::sign_document(
        &monitor_key().0,
        safety::CLEARANCE_DOMAIN,
        &prop,
        &own,
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    match report(&w, &own, Some(&sig)) {
        Ok(()) => panic!("ATTACK: a subject's clearance of its own trial was recorded"),
        Err(e) => assert!(e.to_string().contains("independent of the trial"), "{e}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
    let good = safety_report(&t, "clear", None, MONITOR);
    report(&w, &good, Some(&monitor_sign(&good))).expect("control: the monitor clears");
}

/// A clearance named as the trusted monitor but signed by another key: its
/// signature does not verify, so it is refused and writes nothing. Control:
/// the monitor's own signature is recorded.
#[test]
fn a_clearance_whose_signature_does_not_verify_is_never_recorded() {
    let (w, v) = acceptable("cs");
    trust_monitor(&w.s);
    let good = safety_report(&candidate_trial(&v), "clear", None, MONITOR);
    let (impostor, _) = axon_loop_contracts::attestation::generate().unwrap();
    let forged = axon_loop_contracts::attestation::sign_document(
        &impostor,
        safety::CLEARANCE_DOMAIN,
        &OpaqueRef::new(MONITOR).unwrap(),
        &good,
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    match report(&w, &good, Some(&forged)) {
        Ok(()) => panic!("ATTACK: a clearance whose signature does not verify was recorded"),
        Err(e) => assert!(e.to_string().contains("clearance signature refused"), "{e}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
    report(&w, &good, Some(&monitor_sign(&good))).expect("control: the genuine clearance");
}
