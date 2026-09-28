//! ADR-001 §3.6: "Assignments and attempt ids are issued, and stored in the
//! operator-owned store, BEFORE execution." Review wf_d788c05a-be2 executed
//! two ways to choose the counted population AFTER outcomes existed — a later
//! passing ATTEMPT swapped in for a recorded failing one (G32), and a
//! post-hoc TRIAL assigned instead of the failing one (G33) — each flipping a
//! REJECT to ACCEPT. The population is now issued before execution, and only
//! the issued attempt, intaken after the issue, counts.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::evl::{Outcome, UnknownKind};
use axon_loop::plan;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn trial_mut<'a>(v: &'a mut Value, trial: &str) -> &'a mut Value {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["episode"]["identity"]["trial_id"] == trial)
        .unwrap()
}

/// G32 attack: the candidate's c0 FAILED (attempt a1, issued, intaken). A
/// second attempt a2 PASSES and is intaken too; the request delivers a2.
/// Only the issued attempt counts: a2 is Unbound, and the decision is not
/// ACCEPT. Control: delivering the issued a1 counts its failure.
#[test]
fn a_later_attempt_is_never_swapped_in_for_the_issued_one() {
    let w = world();
    freeze_plan(&w.s, "k", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    specs.iter_mut().find(|s| s.3 == "c0").unwrap().4 = Out::Fail;
    let mut v = evl_request("k", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    let mut a2 = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    a2.attempt = "a2";
    let a2 = trial(&a2);
    let mut only_a2 = v.clone();
    only_a2["trials"] = json!([a2.clone()]);
    assert!(
        intake_all(&w.s, &only_a2).is_empty(),
        "a new attempt intakes"
    );
    *trial_mut(&mut v, "c0") = a2;
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let r = c
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == "c0")
        .unwrap();
    assert_eq!(
        (r.outcome, r.unknown_kind),
        (Outcome::Unknown, Some(UnknownKind::Unbound)),
        "{}",
        r.reason
    );
    assert!(r.reason.contains("issued attempt"), "{}", r.reason);
    assert_eq!(c.verified_pass, 1, "{c:?}");
    let (adm, _) = admit(&w.s, "k", &e, ADMITTER, false).unwrap();
    assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
}

/// G33 attack: the population was issued with c0; the evaluator requests a
/// post-hoc c0b instead. Refused whole, writing nothing.
#[test]
fn a_trial_is_never_assigned_after_outcomes_exist() {
    let w = world();
    freeze_plan(&w.s, "g33", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("g33", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    let mut picked = specs.clone();
    picked.iter_mut().find(|s| s.3 == "c0").unwrap().3 = "c0b".into();
    let v2 = evl_request("g33", &w.inc, &w.cand, &picked, &EvlOpts::default());
    let before = snapshot(w.dir.path());
    match axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v2.to_string()).unwrap(),
    ) {
        Err(LoopError::Refused(m)) => {
            assert!(m.contains("not the one journalled before execution"), "{m}")
        }
        o => panic!("a post-hoc population was evaluated: {o:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
}

/// No issued population, no evaluation; and a trial already intaken cannot
/// be issued afterwards (the population is never chosen from outcomes).
#[test]
fn a_population_is_issued_before_any_outcome() {
    let w = world();
    freeze_plan(&w.s, "none", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("none", &w.inc, &w.cand, &specs, &EvlOpts::default());
    match axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    ) {
        Err(LoopError::Refused(m)) => assert!(m.contains("no assignment journalled"), "{m}"),
        o => panic!("evaluated with no issued population: {o:?}"),
    }
    // Intake one trial BEFORE issuing: the issue is refused.
    let acks = [ack_for(&w.inc).to_string(), ack_for(&w.cand).to_string()];
    for p in [&w.inc, &w.cand] {
        axon_loop::candidates::put_policy(&w.s, p).unwrap();
    }
    let t = &v["trials"][0];
    let text = |x: &Value| (!x.is_null()).then(|| x.to_string());
    axon_loop::intake::intake_episode(
        &w.s,
        &axon_loop::intake::IntakeInput {
            episode: &t["episode"].to_string(),
            context: &t["context"].to_string(),
            acks: &acks,
            projection: None,
            source_episode: None,
            verification_request: text(&t["verification_request"]).as_deref(),
            verification_receipt: text(&t["verification_receipt"]).as_deref(),
            verification_attestation: text(&t["verification_attestation"]).as_deref(),
            verification_psv_evidence: None,
        },
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    match plan::assign(&w.s, &assignment_of_request(&v)) {
        Err(LoopError::Refused(m)) => assert!(m.contains("intaken before its assignment"), "{m}"),
        o => panic!("issued a population over a recorded outcome: {o:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
}

/// The issuer of the population is an independent admitter: not Compute
/// Fabric (a verifier), nor the EVO proposer (the ranker), even listed as an
/// admitter; and a population is issued once.
#[test]
fn the_population_is_issued_once_by_an_independent_admitter() {
    let w = world();
    freeze_plan(&w.s, "who", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut cfg = w.s.config().unwrap();
    for who in [VERIFIER, PROPOSER] {
        cfg.trusted_admitters.push(OpaqueRef::new(who).unwrap());
    }
    w.s.write_config(&cfg).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("who", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let before = snapshot(w.dir.path());
    for (who, why) in [
        (VERIFIER, "a trusted verifier"),
        (PROPOSER, "an EVO proposer"),
        ("op:stranger", "not a trusted admitter"),
    ] {
        let mut a = assignment_of_request(&v);
        a.issuer_ref = OpaqueRef::new(who).unwrap();
        match plan::assign(&w.s, &a) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{who}: {m}"),
            o => panic!("{who} issued the population: {o:?}"),
        }
    }
    assert_eq!(snapshot(w.dir.path()), before);
    let a = assignment_of_request(&v);
    let r = plan::assign(&w.s, &a).unwrap();
    assert_eq!(plan::assign(&w.s, &a).unwrap(), r, "idempotent");
    let mut other = a.clone();
    other.trials.reverse();
    other.trials[0].attempt_id = AttemptId::new("i1-a9").unwrap();
    match plan::assign(&w.s, &other) {
        Err(LoopError::Refused(m)) => assert!(m.contains("already has its assignment"), "{m}"),
        o => panic!("a second population was issued: {o:?}"),
    }
}

/// PROTECTED class: the observer-signed preflight must follow the issue. A
/// trial preflighted before its population was issued is refused whole.
#[test]
fn a_protected_trial_runs_only_after_it_is_issued() {
    let w = world();
    let mut cfg = w.s.config().unwrap();
    cfg.protected_scopes.push(scope());
    w.s.write_config(&cfg).unwrap();
    freeze_plan(&w.s, "early", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    // Built (preflighted) first, issued after: the order an attacker needs.
    let v = evl_request("early", &w.inc, &w.cand, &specs, &EvlOpts::default());
    std::thread::sleep(std::time::Duration::from_millis(5));
    assign_request(&w.s, &v);
    match evaluate(&w.s, &v) {
        Err(LoopError::Refused(m)) => {
            assert!(m.contains("before its assignment was issued"), "{m}")
        }
        o => panic!("a protected trial preflighted before its issue was evaluated: {o:?}"),
    }
}
