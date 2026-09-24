//! B276 EVL + B277 admission + end-to-end into the fenced pointer.
mod common;
use axon_loop::admission::{self, Decision};
use axon_loop::error::LoopError;
use axon_loop::evl::{self, Outcome};
use axon_loop::plan::{self, PilotPlan};
use axon_loop::{evo, null_policy_ref, pointer, Store};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

struct World {
    _d: tempfile::TempDir,
    s: Store,
    inc: PolicyEnvelope,
    cand: PolicyEnvelope,
    inc_ref: Ref,
    cand_ref: Ref,
}

fn world() -> World {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let req = evo::parse_request(
        &evo_request(&inc, 3, "cand-1", vec![discovery_episode(&inc, "d1")]).to_string(),
    )
    .unwrap();
    let p = evo::propose(&s, &req).unwrap();
    let inc_ref = digest(&inc).unwrap();
    World {
        _d: d,
        s,
        inc,
        cand: p.candidate,
        inc_ref,
        cand_ref: p.candidate_policy_ref,
    }
}

fn register_plan(w: &World, id: &str, edit: impl FnOnce(&mut Value)) {
    let mut v = complete_plan(id, &w.inc_ref, &w.cand_ref);
    edit(&mut v);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    plan::freeze(&w.s, id).unwrap();
}

/// (arm, policy, task, trial, outcome, cost)
type Spec<'a> = (
    &'a str,
    &'a PolicyEnvelope,
    &'a str,
    &'a str,
    Out,
    Option<u64>,
);

fn evl_request(w: &World, specs: &[Spec], deliver: impl Fn(&str) -> bool) -> Value {
    let mut assigned = vec![];
    let mut trials = vec![];
    for (arm, p, task, t, out, cost) in specs {
        assigned.push(json!({"task_id": task, "arm_id": arm, "trial_id": t, "policy_ref": digest(*p).unwrap()}));
        if deliver(t) {
            let mut tr = Trial::new(p, task, arm, t);
            tr.out = *out;
            tr.cost = *cost;
            trials.push(trial(&tr));
        }
    }
    json!({
        "schema": "axon.loop.evl-request/1",
        "scope": scope(),
        "evaluator_ref": EVALUATOR,
        "subject_issuers": [WORKER],
        "policies": [w.inc, w.cand],
        "assigned": assigned,
        "trials": trials,
    })
}

fn good_specs(w: &World) -> Vec<Spec<'_>> {
    vec![
        ("incumbent", &w.inc, "task-1", "i1", Out::Pass, Some(100)),
        ("incumbent", &w.inc, "task-2", "i2", Out::Pass, Some(100)),
        ("challenger-1", &w.cand, "task-1", "c1", Out::Pass, Some(50)),
        ("challenger-1", &w.cand, "task-2", "c2", Out::Pass, Some(50)),
    ]
}

fn evaluate(w: &World, v: &Value) -> (evl::EvaluationRecord, Ref) {
    evl::evaluate(&w.s, &evl::parse_request(&v.to_string()).unwrap()).unwrap()
}

fn admit(
    w: &World,
    id: &str,
    eval: &Ref,
    admitter: &str,
    mech: bool,
) -> Result<(admission::AdmissionRecord, Ref), LoopError> {
    let req = admission::parse_request(
        &json!({
            "schema": "axon.loop.admit-request/1",
            "experiment_id": id,
            "evaluation_ref": eval,
            "admitter_ref": admitter,
            "mechanism_test": mech
        })
        .to_string(),
    )?;
    admission::admit(&w.s, &req)
}

#[test]
fn evl_counts_missing_and_unknown_against_quality() {
    let w = world();
    let mut specs = good_specs(&w);
    specs[2].4 = Out::Unknown; // c1 timed out
    specs.push(("challenger-1", &w.cand, "task-3", "c3", Out::Fail, Some(10)));
    specs.push(("incumbent", &w.inc, "task-3", "i3", Out::Pass, Some(10)));
    let v = evl_request(&w, &specs, |t| t != "c2"); // c2 never delivered
    let (rec, _) = evaluate(&w, &v);
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(
        (c.assigned, c.verified_pass, c.fail, c.unknown, c.missing),
        (3, 0, 1, 2, 1)
    );
    let reasons: Vec<_> = c
        .trials
        .iter()
        .map(|t| (t.trial_id.as_str().to_string(), t.outcome))
        .collect();
    assert!(reasons.contains(&("c2".into(), Outcome::Unknown)));
    let i = rec.arm_for_policy(&w.inc_ref).unwrap();
    assert_eq!(i.verified_pass, 3);
}

#[test]
fn evl_rejects_subject_or_untrusted_verifier_and_stale_epoch() {
    let w = world();
    let mut v = evl_request(&w, &good_specs(&w), |_| true);
    // c1: verified by the WORKER (a subject) — trusted? no; also subject.
    let mut tr = Trial::new(&w.cand, "task-1", "challenger-1", "c1");
    tr.verifier = WORKER;
    v["trials"][2] = trial(&tr);
    // c2: verified by the proposer — subject by EVO history even if not listed.
    let mut tr = Trial::new(&w.cand, "task-2", "challenger-1", "c2");
    tr.verifier = PROPOSER;
    v["trials"][3] = trial(&tr);
    // i1: stale authority epoch
    let mut tr = Trial::new(&w.inc, "task-1", "incumbent", "i1");
    tr.epoch = 5;
    v["trials"][0] = trial(&tr);
    let (rec, _) = evaluate(&w, &v);
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!((c.verified_pass, c.unknown), (0, 2));
    assert!(rec.subject_issuers.iter().any(|s| s.as_str() == PROPOSER));
    let i = rec.arm_for_policy(&w.inc_ref).unwrap();
    assert_eq!((i.verified_pass, i.unknown), (1, 1));
}

#[test]
fn evl_refusals_write_nothing() {
    let w = world();
    let before = snapshot(w.s.root());
    // unassigned trial delivered
    let mut v = evl_request(&w, &good_specs(&w), |_| true);
    v["assigned"].as_array_mut().unwrap().remove(0);
    assert!(evl::evaluate(&w.s, &evl::parse_request(&v.to_string()).unwrap()).is_err());
    // evaluator is a subject
    let mut v = evl_request(&w, &good_specs(&w), |_| true);
    v["evaluator_ref"] = json!(PROPOSER);
    assert!(matches!(
        evl::evaluate(&w.s, &evl::parse_request(&v.to_string()).unwrap()),
        Err(LoopError::Refused(_))
    ));
    // tampered episode byte ⇒ malformed/refused, never a pass
    let mut v = evl_request(&w, &good_specs(&w), |_| true);
    v["trials"][2]["episode"]["verification"]["matched_checks"] = json!(0);
    assert!(evl::evaluate(&w.s, &evl::parse_request(&v.to_string()).unwrap()).is_err());
    assert_eq!(snapshot(w.s.root()), before);
}

#[test]
fn full_loop_accept_activate_future_task_rollback() {
    let w = world();
    register_plan(&w, "exp", |_| {});
    let (_, eval_ref) = evaluate(&w, &evl_request(&w, &good_specs(&w), |_| true));
    // proposer cannot admit (even if an operator wrongly trusted it)
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(PROPOSER).unwrap());
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.s.root());
    assert!(matches!(
        admit(&w, "exp", &eval_ref, PROPOSER, false),
        Err(LoopError::Refused(_))
    ));
    assert!(matches!(
        admit(&w, "exp", &eval_ref, "op:stranger", false),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.s.root()), before);

    let (rec, adm) = admit(&w, "exp", &eval_ref, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    // deterministic: same inputs → same ref
    assert_eq!(admit(&w, "exp", &eval_ref, ADMITTER, false).unwrap().1, adm);

    // Incumbent must be active first (its own admission seeded).
    let inc_adm = seed_admission(&w.s, &w.inc, Decision::Accept, false, true);
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "a0",
            "activate",
            &null_policy_ref(),
            Some(&w.inc_ref),
            0,
            Some(&inc_adm),
            false,
        )),
    )
    .unwrap();
    // admission does not activate
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().0.version.digest,
        w.inc_ref
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
    let (pin, env) = pointer::resolve(&w.s, &scope()).unwrap();
    assert_eq!(
        (pin.version.digest.clone(), pin.epoch.get()),
        (w.cand_ref.clone(), 2)
    );
    assert_eq!(env, w.cand);
    // regression ⇒ fenced rollback to the still-valid admitted incumbent
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&inc_adm),
            false,
        )),
    )
    .unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().0.version.digest,
        w.inc_ref
    );
    let h = evo::history(&w.s, &scope()).unwrap();
    assert!(h.iter().any(|x| matches!(
        x,
        evo::Hypothesis::Verdict {
            verdict: evo::Verdict::Accept,
            ..
        }
    )));
}

#[test]
fn inconclusive_on_small_sample_liability_and_unknowns() {
    let w = world();
    // sample size: plan wants 3 independent tasks, arms have 2
    register_plan(&w, "small", |v| v["independent_units"] = json!(3));
    let (_, e) = evaluate(&w, &evl_request(&w, &good_specs(&w), |_| true));
    let (rec, adm) = admit(&w, "small", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Inconclusive);
    assert!(
        rec.reasons.iter().any(|r| r.contains("sample size")),
        "{:?}",
        rec.reasons
    );
    // an INCONCLUSIVE admission cannot activate
    assert!(matches!(
        pointer::transition(
            &w.s,
            &tparse(&transition(
                "z",
                "activate",
                &null_policy_ref(),
                Some(&w.cand_ref),
                0,
                Some(&adm),
                false
            ))
        ),
        Err(LoopError::Refused(_))
    ));

    // unknown liability over tolerance
    let w = world();
    register_plan(&w, "liab", |_| {});
    let mut v = evl_request(&w, &good_specs(&w), |_| true);
    let mut tr = Trial::new(&w.cand, "task-1", "challenger-1", "c1");
    tr.cost = Some(50);
    tr.liability = 5;
    v["trials"][2] = trial(&tr);
    let (_, e) = evaluate(&w, &v);
    let (rec, _) = admit(&w, "liab", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Inconclusive);
    assert!(
        rec.reasons.iter().any(|r| r.contains("liability")),
        "{:?}",
        rec.reasons
    );

    // unknown outcome: noninferiority cannot be established
    let w = world();
    register_plan(&w, "unk", |_| {});
    let mut specs = good_specs(&w);
    specs[2].4 = Out::Unknown;
    let (_, e) = evaluate(&w, &evl_request(&w, &specs, |_| true));
    let (rec, _) = admit(&w, "unk", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Inconclusive, "{:?}", rec.reasons);
    assert!(rec.reasons.iter().any(|r| r.contains("noninferiority")));

    // a non-executable rule never guesses
    let w = world();
    register_plan(&w, "prose", |v| {
        v["quality_margin"] = json!("about 2 percent")
    });
    let (_, e) = evaluate(&w, &evl_request(&w, &good_specs(&w), |_| true));
    assert_eq!(
        admit(&w, "prose", &e, ADMITTER, false).unwrap().0.decision,
        Decision::Inconclusive
    );
}

#[test]
fn reject_on_inferiority_or_no_economic_benefit() {
    let w = world();
    register_plan(&w, "worse", |_| {});
    let mut specs = good_specs(&w);
    specs[2].4 = Out::Fail;
    specs[3].4 = Out::Fail;
    let (_, e) = evaluate(&w, &evl_request(&w, &specs, |_| true));
    assert_eq!(
        admit(&w, "worse", &e, ADMITTER, false).unwrap().0.decision,
        Decision::Reject
    );

    let w = world();
    register_plan(&w, "pricey", |_| {});
    let mut specs = good_specs(&w);
    specs[2].5 = Some(95);
    specs[3].5 = Some(95); // only 5% cheaper; plan needs 10%
    let (_, e) = evaluate(&w, &evl_request(&w, &specs, |_| true));
    let (rec, _) = admit(&w, "pricey", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Reject, "{:?}", rec.reasons);
}

#[test]
fn admission_refuses_unfrozen_plan_and_wrong_corpus_role() {
    let w = world();
    let v = complete_plan("unfrozen", &w.inc_ref, &w.cand_ref);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    let (_, e) = evaluate(&w, &evl_request(&w, &good_specs(&w), |_| true));
    let before = snapshot(w.s.root());
    assert!(matches!(
        admit(&w, "unfrozen", &e, ADMITTER, false),
        Err(LoopError::NotReady(_))
    ));
    plan::freeze(&w.s, "unfrozen").unwrap();
    // confirmation-role evidence may not be admitted as a mechanism test
    let before2 = snapshot(w.s.root());
    assert!(matches!(
        admit(&w, "unfrozen", &e, ADMITTER, true),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.s.root()), before2);
    assert_ne!(before, before2);
}
