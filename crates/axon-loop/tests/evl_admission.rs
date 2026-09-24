//! B276 EVL + B277 admission + end-to-end into the fenced pointer.
mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::evl::Outcome;
use axon_loop::{evo, pointer};
use axon_loop_contracts::*;
use common::*;
use serde_json::json;

#[test]
fn evl_counts_missing_and_unknown_against_quality() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 3, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 3, 3, 3, Some(100), Some(50));
    specs[3].4 = Out::Unknown; // c0 timed out
    specs[5].4 = Out::Fail; // c2 failed
    let o = EvlOpts {
        deliver: Box::new(|t| t != "c1"),
        ..Default::default()
    };
    let (rec, _) = evaluate(&w.s, &evl_request("exp", &w.inc, &w.cand, &specs, &o)).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(
        (c.assigned, c.verified_pass, c.fail, c.unknown, c.missing),
        (3, 0, 1, 2, 1)
    );
    assert!(c
        .trials
        .iter()
        .any(|t| t.trial_id.as_str() == "c1" && t.outcome == Outcome::Unknown));
    assert_eq!(c.economics.missing_records, 1);
    assert_eq!(rec.arm_for_policy(&w.inc_ref).unwrap().verified_pass, 3);
}

#[test]
fn evl_rejects_subject_or_untrusted_verifier_and_stale_epoch() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let mut tr = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    tr.verifier = WORKER;
    v["trials"][2] = trial(&tr);
    let mut tr = Trial::new(&w.cand, "task-1", "challenger-1", "c1");
    tr.verifier = PROPOSER;
    v["trials"][3] = trial(&tr);
    let mut tr = Trial::new(&w.inc, "task-0", "incumbent", "i0");
    tr.epoch = 5;
    v["trials"][0] = trial(&tr);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!((c.verified_pass, c.unknown), (0, 2));
    assert!(rec.subject_issuers.iter().any(|s| s.as_str() == PROPOSER));
    let i = rec.arm_for_policy(&w.inc_ref).unwrap();
    assert_eq!((i.verified_pass, i.unknown), (1, 1));
}

#[test]
fn evl_refusals_write_nothing() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let good = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let before = snapshot(w.dir.path());
    let mut v = good.clone();
    v["assigned"].as_array_mut().unwrap().remove(0);
    assert!(evaluate(&w.s, &v).is_err(), "unassigned delivered");
    let mut v = good.clone();
    v["evaluator_ref"] = json!(PROPOSER);
    assert!(matches!(evaluate(&w.s, &v), Err(LoopError::Refused(_))));
    let mut v = good.clone();
    v["trials"][2]["episode"]["verification"]["matched_checks"] = json!(0);
    assert!(evaluate(&w.s, &v).is_err(), "tampered episode");
    let mut v = good.clone();
    v["experiment_id"] = json!("not-frozen");
    assert!(matches!(evaluate(&w.s, &v), Err(LoopError::NotReady(_))));
    assert_eq!(snapshot(w.dir.path()), before);
}

#[test]
fn full_loop_accept_activate_future_task_rollback() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, eval_ref) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(PROPOSER).unwrap());
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.dir.path());
    assert!(matches!(
        admit(&w.s, "exp", &eval_ref, PROPOSER, false),
        Err(LoopError::Refused(_))
    ));
    assert!(matches!(
        admit(&w.s, "exp", &eval_ref, "op:stranger", false),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);

    let (rec, adm) = admit(&w.s, "exp", &eval_ref, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    assert_eq!(
        admit(&w.s, "exp", &eval_ref, ADMITTER, false).unwrap().1,
        adm,
        "deterministic"
    );
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref,
        "admission does not activate"
    );
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    let r = pointer::resolve(&w.s, &scope()).unwrap();
    assert_eq!(
        (r.pin.version.digest.clone(), r.pin.epoch.get()),
        (w.cand_ref.clone(), 2)
    );
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref
    );
    let h = evo::history(&w.s, &scope()).unwrap();
    assert!(h.iter().any(|x| matches!(
        x,
        evo::Hypothesis::Verdict {
            verdict: evo::Verdict::Accept,
            mechanism_test: false,
            ..
        }
    )));
}

fn decision(
    w: &World,
    exp: &str,
    specs: &[Spec],
    o: EvlOpts,
    edit: impl FnOnce(&mut serde_json::Value),
) -> (Decision, Vec<String>) {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, edit).unwrap();
    let (_, e) = evaluate(&w.s, &evl_request(exp, &w.inc, &w.cand, specs, &o)).unwrap();
    let (rec, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    (rec.decision, rec.reasons)
}

#[test]
fn inconclusive_on_small_sample_liability_and_unknowns() {
    // Sample size is now fixed at FREEZE: a manifest smaller than the planned
    // independent units cannot be frozen, so no evaluation can be too small.
    let w = world();
    let e = freeze_plan(&w.s, "small", &w.inc_ref, &w.cand_ref, |v| {
        v["independent_units"] = json!(3)
    })
    .unwrap_err();
    assert!(
        matches!(e, LoopError::NotReady(ref m) if m.contains("independent_units")),
        "{e}"
    );

    let w = world();
    freeze_plan(&w.s, "liab", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("liab", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let mut tr = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    tr.cost = Some(50);
    tr.liability = 5;
    v["trials"][2] = trial(&tr);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, _) = admit(&w.s, "liab", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Inconclusive);
    assert!(
        rec.reasons.iter().any(|r| r.contains("liability")),
        "{:?}",
        rec.reasons
    );

    let w = world();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    specs[2].4 = Out::Unknown;
    let (d, r) = decision(&w, "unk", &specs, EvlOpts::default(), |_| {});
    assert_eq!(d, Decision::Inconclusive, "{r:?}");
    assert!(r.iter().any(|x| x.contains("noninferiority")));
}

#[test]
fn reject_on_inferiority_or_no_economic_benefit() {
    let w = world();
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    assert_eq!(
        decision(&w, "worse", &specs, EvlOpts::default(), |_| {}).0,
        Decision::Reject
    );
    let w = world();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(95));
    assert_eq!(
        decision(&w, "pricey", &specs, EvlOpts::default(), |_| {}).0,
        Decision::Reject
    );
}

#[test]
fn admission_refuses_wrong_corpus_role() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    assert!(matches!(
        admit(&w.s, "exp", &e, ADMITTER, true),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);
}
