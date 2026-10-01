//! Amendment 61 (C9 round 4b, rows4a): the refusal sites of `evl.rs` that
//! had no mutation row, each judged on the PRODUCTION route where it decides
//! (`evl::evaluate`, `plan::assign`, `intake::intake_episode`, `admit`).
//!
//! Every test is an ATTACK (one defect, everything else genuine) with its
//! CONTROL (the same request without the defect is recorded). Where two
//! checks refuse the same attack, each alone (a four-cell retirement in
//! `scripts/v022_g01_mutations.py`), the assertion accepts either reason: only
//! the attack getting through is the failure.

mod common;
use axon_loop::error::LoopError;
use axon_loop::evl::EvaluationRecord;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn specs(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}

/// A world with `exp` frozen and its population assigned, and the honest
/// request for it (not yet intaken).
fn frozen(exp: &str) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, exp, &specs(&w));
    let v = evl_request(exp, &w.inc, &w.cand, &specs(&w), &EvlOpts::default());
    (w, v)
}

/// `evl::evaluate` alone (what was delivered is intaken by the caller first).
fn evaluate_only(w: &World, v: &Value) -> Result<(EvaluationRecord, Ref), LoopError> {
    axon_loop::evl::evaluate(&w.s, &axon_loop::evl::parse_request(&v.to_string())?)
}

/// The attack `v` is refused for `why` (any of them), writing nothing.
fn refused(w: &World, v: &Value, why: &[&str], attack: &str) {
    let before = snapshot(w.dir.path());
    match evaluate_only(w, v) {
        Ok((rec, _)) => panic!(
            "ATTACK: {attack}: the evaluation was recorded: {:?}",
            rec.arms
        ),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "expected one of {why:?}: {e}"
        ),
    }
    assert_eq!(
        snapshot(w.dir.path()),
        before,
        "{attack}: a refusal wrote something"
    );
}

fn challenger(v: &mut Value) -> impl Iterator<Item = &mut Value> {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
}

/// CONTROL for every evaluation attack below: the honest request is recorded
/// and both arms count their passes.
#[test]
fn the_honest_evaluation_is_recorded() {
    let (w, v) = frozen("ok");
    assert!(intake_all(&w.s, &v).is_empty());
    let (rec, _) = evaluate_only(&w, &v).unwrap();
    assert!(
        rec.arms.iter().all(|a| a.verified_pass == 2),
        "{:?}",
        rec.arms
    );
}

/// No trusted verifier configured: nothing can be verified, so no evaluation
/// is recorded (the experiment's one evaluation is not spent on nothing).
#[test]
fn an_evaluation_with_no_trusted_verifier_is_refused() {
    let (w, v) = frozen("nv");
    intake_all(&w.s, &v);
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_verifiers.clear();
    w.s.write_config(&cfg).unwrap();
    refused(
        &w,
        &v,
        &["no trusted verifiers configured"],
        "an evaluation with no trusted verifier",
    );
}

/// The request supplies a policy the frozen plan does not name, beside its
/// two arms: refused, so nothing but the plan's arms is ever stored as the
/// evaluated policies.
#[test]
fn an_evaluation_supplying_a_policy_outside_the_plan_is_refused() {
    let (w, mut v) = frozen("sp");
    intake_all(&w.s, &v);
    let other = policy("other-policy", &["read"]);
    v["policies"].as_array_mut().unwrap().push(json!(other));
    refused(
        &w,
        &v,
        &["must be exactly the frozen plan's incumbent and candidate"],
        "a policy outside the plan was supplied",
    );
}

/// Four-cell pair (evaluation scope vs the plan's; each policy's scope vs the
/// request's): an evaluation that names another scope than its plan's, over
/// the plan's own policies, is refused; with BOTH checks removed it is
/// recorded under the other scope.
#[test]
fn an_evaluation_under_another_scope_is_refused() {
    let (w, mut v) = frozen("xs");
    intake_all(&w.s, &v);
    v["scope"]["tenant_id"] = json!("other-tenant");
    refused(
        &w,
        &v,
        &[
            "evaluation scope differs from the frozen plan",
            "is for another scope",
        ],
        "an evaluation under another scope than its plan's",
    );
}

/// Four-cell pair (one evaluation per experiment; a trial id is evaluated
/// once): a second evaluation of a frozen experiment, delivering a different
/// subset of the same population (a REJECT re-rolled), is refused; with BOTH
/// checks removed it is journalled as a second evaluation.
#[test]
fn a_second_evaluation_of_an_experiment_is_refused() {
    let (w, v) = frozen("twice");
    assert!(intake_all(&w.s, &v).is_empty());
    evaluate_only(&w, &v).unwrap();
    let mut again = v.clone();
    again["trials"]
        .as_array_mut()
        .unwrap()
        .retain(|t| t["episode"]["identity"]["arm_id"] == "challenger-1");
    refused(
        &w,
        &again,
        &["already has its evaluation", "was already evaluated"],
        "a second evaluation of a frozen experiment",
    );
}

/// A delivered trial the population never assigned is refused: an unassigned
/// result never rides into the population's record.
#[test]
fn an_unassigned_trial_never_rides_into_an_evaluation() {
    let (w, mut v) = frozen("ua");
    intake_all(&w.s, &v);
    let mut extra = Trial::new(&w.cand, "task-0", "challenger-1", "c9");
    extra.out = Out::Pass;
    v["trials"].as_array_mut().unwrap().push(trial(&extra));
    refused(
        &w,
        &v,
        &["was never assigned"],
        "an unassigned trial was carried in",
    );
}

/// A preflight dated in the future refuses the whole evaluation.
#[test]
fn a_future_dated_preflight_refuses_the_evaluation() {
    let (w, mut v) = frozen("fu");
    intake_all(&w.s, &v);
    let later = axon_loop::now_ms() + 3_600_000;
    let t = &mut v["trials"][0];
    t["context"]["created_ms"] = json!(later);
    t["context"]["expires_ms"] = json!(later + 3_600_000);
    t["episode"]["context_ref"] = json!(digest_value(&t["context"]).unwrap());
    refused(
        &w,
        &v,
        &["claims a preflight in the future"],
        "a future-dated preflight",
    );
}

/// A trial preflighted BEFORE the plan froze refuses the evaluation: an
/// outcome that predates the rule is never judged by it. The trial is
/// otherwise genuine and intaken, so without the check it counts.
#[test]
fn a_preflight_before_the_freeze_refuses_the_evaluation() {
    let (w, mut v) = frozen("pf");
    let t = &mut v["trials"][0];
    t["context"]["created_ms"] = json!(1_000);
    t["context"]["expires_ms"] = json!(axon_loop::now_ms() + 3_600_000);
    t["episode"]["context_ref"] = json!(digest_value(&t["context"]).unwrap());
    intake_all(&w.s, &v);
    refused(
        &w,
        &v,
        &["before the plan froze"],
        "a trial preflighted before the plan froze",
    );
}

/// The same trial delivered twice is refused: which copy counts is never the
/// requester's choice.
#[test]
fn a_trial_delivered_twice_refuses_the_evaluation() {
    let (w, mut v) = frozen("d2");
    intake_all(&w.s, &v);
    let dup = v["trials"][0].clone();
    v["trials"].as_array_mut().unwrap().push(dup);
    refused(
        &w,
        &v,
        &["trial delivered twice"],
        "a trial delivered twice",
    );
}

/// Four-cell (EVL's cross-tenant check vs intake's: the episode must be
/// intaken in this scope, M15, and intake binds an episode only to a context
/// of its own scope): a challenger trial whose context is another tenant's,
/// named by its episode, never counts. Any refusal is accepted; only a
/// counted trial is the attack.
#[test]
fn a_cross_tenant_context_never_counts() {
    for cross in [false, true] {
        let (w, mut v) = frozen("xt");
        if cross {
            for t in challenger(&mut v) {
                t["context"]["scope"]["tenant_id"] = json!("other-tenant");
                t["episode"]["context_ref"] = json!(digest_value(&t["context"]).unwrap());
            }
        }
        intake_all(&w.s, &v);
        let (rec, _) = evaluate_only(&w, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
        if !cross {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else if arm.verified_pass != 0 {
            panic!(
                "ATTACK: a trial whose context is another tenant's counted: {:?}",
                arm.trials
            );
        }
    }
}

/// Four-cell (EVL's arm-policy check vs bind_episode's policy binding, the
/// identical predicate): an episode that ran the INCUMBENT's policy,
/// delivered and intaken as a challenger trial, never counts for the
/// challenger arm.
#[test]
fn an_episode_of_another_policy_never_counts_for_an_arm() {
    for swapped in [false, true] {
        let (w, mut v) = frozen("op");
        if swapped {
            let inc = w.inc.clone();
            for t in challenger(&mut v) {
                let id = &t["episode"]["identity"];
                let mut tr = Trial::new(
                    &inc,
                    id["task_id"].as_str().unwrap(),
                    "challenger-1",
                    id["trial_id"].as_str().unwrap(),
                );
                tr.cost = Some(50);
                *t = trial(&tr);
            }
        }
        intake_all(&w.s, &v);
        let (rec, _) = evaluate_only(&w, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
        if !swapped {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else if arm.verified_pass != 0 {
            panic!(
                "ATTACK: an episode of the incumbent's policy counted for the challenger arm: {:?}",
                arm.trials
            );
        }
    }
}

// ── the population (`check_population`), on the route that ISSUES it ───────

/// `{task_id, arm_id, trial_id, attempt_id, policy_ref}`.
fn row(task: &str, arm: &str, trial: &str, p: &Ref) -> Value {
    json!({"task_id": task, "arm_id": arm, "trial_id": trial,
           "attempt_id": format!("{trial}-a1"), "policy_ref": p})
}

/// `plan::assign` refuses `trials` for `why`; the population is not issued.
fn never_issued(w: &World, exp: &str, trials: Vec<Value>, why: &[&str], attack: &str) {
    let rec = assignment_record(exp, trials);
    match axon_loop::plan::assign(&w.s, &rec) {
        Ok(_) => panic!("ATTACK: {attack}: the population was issued"),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "{attack}: expected one of {why:?}: {e}"
        ),
    }
}

fn frozen_only(exp: &str, reps: u64) -> World {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |p| {
        p["repetitions"] = json!(reps)
    })
    .unwrap();
    w
}

/// CONTROL for the population attacks: the exact population is issued.
#[test]
fn the_exact_population_is_issued() {
    let w = frozen_only("pop", 1);
    let (i, c) = (&w.inc_ref, &w.cand_ref);
    let rec = assignment_record(
        "pop",
        vec![
            row("task-0", "incumbent", "i0", i),
            row("task-1", "incumbent", "i1", i),
            row("task-0", "challenger-1", "c0", c),
            row("task-1", "challenger-1", "c1", c),
        ],
    );
    axon_loop::plan::assign(&w.s, &rec).unwrap();
}

/// Each population defect check_population ALONE refuses, issued through
/// `plan::assign` (the population is fixed before execution). One case per
/// guard; every other property of the population holds.
#[test]
fn each_population_defect_is_never_issued() {
    type Case = (&'static str, &'static str, fn(&Ref, &Ref) -> Vec<Value>);
    let cases: [Case; 4] = [
        (
            "an arm assigned two policies",
            "assigned two policies",
            |i, c| {
                vec![
                    row("task-0", "incumbent", "i0", i),
                    row("task-1", "incumbent", "i1", c),
                    row("task-0", "challenger-1", "c0", c),
                    row("task-1", "challenger-1", "c1", i),
                ]
            },
        ),
        (
            "only one arm assigned",
            "must assign both the incumbent and the candidate arm",
            |i, _| {
                vec![
                    row("task-0", "incumbent", "i0", i),
                    row("task-1", "incumbent", "i1", i),
                ]
            },
        ),
        (
            "a subset of the task manifest",
            "must cover exactly the manifest",
            |i, c| {
                vec![
                    row("task-0", "incumbent", "i0", i),
                    row("task-0", "challenger-1", "c0", c),
                ]
            },
        ),
        (
            "a task assigned more often than the plan's repetitions",
            "plan repetitions",
            |i, c| {
                vec![
                    row("task-0", "incumbent", "i0", i),
                    row("task-0", "incumbent", "i0b", i),
                    row("task-1", "incumbent", "i1", i),
                    row("task-0", "challenger-1", "c0", c),
                    row("task-1", "challenger-1", "c1", c),
                ]
            },
        ),
    ];
    for (n, (case, why, rows)) in cases.into_iter().enumerate() {
        let exp = format!("pd{n}");
        let w = frozen_only(&exp, 1);
        never_issued(&w, &exp, rows(&w.inc_ref, &w.cand_ref), &[why], case);
    }
}

/// Four-cell (check_population's arm-policy check vs `plan::assign`'s own):
/// a trial assigned a policy the frozen plan does not name is never issued.
#[test]
fn a_population_naming_a_policy_outside_the_plan_is_never_issued() {
    let w = frozen_only("po", 1);
    let (i, c) = (&w.inc_ref, &w.cand_ref);
    let other = r('e');
    never_issued(
        &w,
        "po",
        vec![
            row("task-0", "incumbent", "i0", i),
            row("task-1", "incumbent", "i1", i),
            row("task-0", "challenger-1", "c0", c),
            row("task-1", "challenger-1", "c1", c),
            row("task-1", "challenger-2", "x1", &other),
        ],
        &["not supplied", "not one of the frozen plan's arms"],
        "a trial assigned a policy outside the plan",
    );
}

/// Four-cell (check_population's trial-id check vs `plan::assign`'s issued-
/// twice check): one trial id for two tasks is never issued.
#[test]
fn a_trial_id_issued_for_two_tasks_is_never_issued() {
    let w = frozen_only("tid", 1);
    let (i, c) = (&w.inc_ref, &w.cand_ref);
    never_issued(
        &w,
        "tid",
        vec![
            row("task-0", "incumbent", "i0", i),
            row("task-1", "incumbent", "i0", i),
            row("task-0", "challenger-1", "c0", c),
            row("task-1", "challenger-1", "c1", c),
        ],
        &["is assigned twice in this evaluation", "is issued twice"],
        "one trial id issued for two tasks",
    );
}

/// Four-cell (check_population's key check vs its trial-id check and
/// `plan::assign`'s): the same trial assigned twice is never issued. Under
/// two repetitions, so the repetition count holds with the duplicate in it.
#[test]
fn a_trial_assigned_twice_is_never_issued() {
    let w = frozen_only("dup", 2);
    let (i, c) = (&w.inc_ref, &w.cand_ref);
    never_issued(
        &w,
        "dup",
        vec![
            row("task-0", "incumbent", "i0", i),
            row("task-0", "incumbent", "i0", i),
            row("task-1", "incumbent", "i1", i),
            row("task-1", "incumbent", "i1b", i),
            row("task-0", "challenger-1", "c0", c),
            row("task-0", "challenger-1", "c0b", c),
            row("task-1", "challenger-1", "c1", c),
            row("task-1", "challenger-1", "c1b", c),
        ],
        &["assigned twice", "is issued twice"],
        "a trial assigned twice",
    );
}

// ── the evaluation admission reads ───────────────────────────────────────

/// An evaluation record placed in the store's CAS but never journalled by
/// `evl evaluate` is never admitted. Control: the journalled one is.
#[test]
fn an_unjournalled_evaluation_is_never_admitted() {
    let (w, v) = frozen("uj");
    assert!(intake_all(&w.s, &v).is_empty());
    let (rec, e) = evaluate_only(&w, &v).unwrap();
    // The same record with one count changed: stored, never journalled.
    let mut j = serde_json::to_value(&rec).unwrap();
    j["arms"][0]["trials"][0]["reason"] = json!("hand-placed");
    let forged: EvaluationRecord = serde_json::from_value(j).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    match admit(&w.s, "uj", &fe, ADMITTER, false) {
        Ok((adm, _)) => panic!(
            "ATTACK: an evaluation never journalled was admitted: {:?}",
            adm.decision
        ),
        Err(e) => assert!(e.to_string().contains("never journalled"), "{e}"),
    }
    admit(&w.s, "uj", &e, ADMITTER, false).expect("control: the journalled evaluation");
}

/// Intake (`bind_episode`'s scope join): an episode that names a context of
/// another tenant is never intaken; every other binding of the trial holds
/// (the episode names that context's exact bytes). Control: the honest trial
/// is intaken.
#[test]
fn an_episode_bound_to_another_tenants_context_is_never_intaken() {
    let (w, mut v) = frozen("it");
    let t = &mut v["trials"][0];
    t["context"]["scope"]["tenant_id"] = json!("other-tenant");
    t["episode"]["context_ref"] = json!(digest_value(&t["context"]).unwrap());
    let id = t["episode"]["identity"]["trial_id"]
        .as_str()
        .unwrap()
        .to_string();
    let refusals = intake_all(&w.s, &v);
    match refusals.iter().find(|(t, _)| t == &id) {
        None => panic!("ATTACK: an episode bound to another tenant's context was intaken"),
        Some((_, why)) => assert!(why.contains("cross-scope"), "{why}"),
    }
    assert_eq!(
        refusals.len(),
        1,
        "control: the honest trials intake: {refusals:?}"
    );
}
