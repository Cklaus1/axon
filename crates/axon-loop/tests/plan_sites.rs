//! Amendment 61 (C9 round 4b, rows4a): `plan.rs`'s refusals (register,
//! freeze, the candidate check, start, assign), each judged on its production
//! route. Every test is an ATTACK with its CONTROL; where two checks refuse
//! the same attack, each alone (a four-cell retirement), any of their reasons
//! is accepted: only the attack getting through is the failure.

mod common;
use axon_loop::admission::Decision;
use axon_loop::candidates::{self, CandidateSet};
use axon_loop::plan::{self, PilotPlan};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn register(w: &World, v: &Value) -> Result<Ref, axon_loop::error::LoopError> {
    plan::register(&w.s, &PilotPlan::from_value(v)?)
}

/// Register `v` (a complete plan with `edit` applied) and freeze it.
fn freeze_with(
    w: &World,
    id: &str,
    inc: &Ref,
    cand: &Ref,
    edit: impl FnOnce(&mut Value),
) -> Result<Ref, axon_loop::error::LoopError> {
    let mut v = complete_plan(id, inc, cand);
    v["task_manifest_ref"] = json!(register_tasks(&w.s, 2));
    edit(&mut v);
    register(w, &v)?;
    plan::freeze(&w.s, id)
}

/// The freeze is refused for one of `why`; ATTACK if it froze.
fn never_frozen(r: Result<Ref, axon_loop::error::LoopError>, why: &[&str], attack: &str) {
    match r {
        Ok(_) => panic!("ATTACK: {attack}: the plan froze"),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "{attack}: expected one of {why:?}: {e}"
        ),
    }
}

/// CONTROL for every freeze attack: the complete plan freezes.
#[test]
fn the_complete_plan_freezes() {
    let w = world();
    freeze_with(&w, "ok", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
}

/// A frozen plan is never registered again (changed).
#[test]
fn a_frozen_plan_is_never_registered_again() {
    let w = world();
    freeze_with(&w, "fz", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut v = complete_plan("fz", &w.inc_ref, &w.cand_ref);
    v["task_manifest_ref"] = json!(register_tasks(&w.s, 2));
    v["quality_margin"] = json!("pass_rate_margin_ppm=500000");
    match register(&w, &v) {
        Ok(r) => panic!("ATTACK: a frozen plan was registered again as {r}"),
        Err(e) => assert!(e.to_string().contains("is frozen"), "{e}"),
    }
}

/// A frozen plan's CAS file rewritten in place (its margin widened to accept
/// the losing candidate) never decides an admission. Control: the unedited
/// plan REJECTs the candidate.
#[test]
fn a_frozen_plans_file_edited_in_place_never_decides() {
    for edited in [false, true] {
        let w = world();
        let p = freeze_with(&w, "pe", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 1, Some(100), Some(50));
        let v = evl_request("pe", &w.inc, &w.cand, &specs, &EvlOpts::default());
        let (_, e) = evaluate(&w.s, &v).unwrap();
        if edited {
            let path = w.s.cas_path("plans", &p).unwrap();
            let mut doc: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            doc["quality_margin"] = json!("pass_rate_margin_ppm=999999");
            std::fs::write(&path, canonical_json(&doc).unwrap()).unwrap();
        }
        match admit(&w.s, "pe", &e, ADMITTER, false) {
            Ok((adm, _)) if edited && adm.decision == Decision::Accept => panic!(
                "ATTACK: a plan edited in the store decided an admission: {:?}",
                adm.reasons
            ),
            Ok((adm, _)) => assert!(
                !edited && adm.decision != Decision::Accept,
                "{edited}: {:?} {:?}",
                adm.decision,
                adm.reasons
            ),
            Err(err) => assert!(edited && err.to_string().contains("changed"), "{err}"),
        }
    }
}

/// A plan with an operator field unset (here the approval) never freezes.
#[test]
fn a_plan_with_an_operator_field_unset_never_freezes() {
    let w = world();
    never_frozen(
        freeze_with(&w, "un", &w.inc_ref, &w.cand_ref, |v| {
            v["approval_ref"] = Value::Null
        }),
        &["operator fields unset"],
        "a plan with no approval_ref",
    );
}

/// Four-cell (incumbent == candidate; the candidate's parent is the plan's
/// incumbent): a plan comparing the EVO candidate with itself never freezes.
#[test]
fn a_plan_comparing_a_policy_with_itself_never_freezes() {
    let w = world();
    never_frozen(
        freeze_with(&w, "self", &w.cand_ref, &w.cand_ref, |_| {}),
        &[
            "candidate equals incumbent",
            "parent is not the plan's incumbent",
        ],
        "a plan comparing a policy with itself",
    );
}

/// One experiment per candidate: a candidate frozen in one experiment never
/// freezes in a second (no plan shopping).
#[test]
fn a_candidate_frozen_once_never_freezes_in_a_second_experiment() {
    let w = world();
    freeze_with(&w, "first", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    never_frozen(
        freeze_with(&w, "second", &w.inc_ref, &w.cand_ref, |_| {}),
        &["is already frozen in experiment"],
        "a candidate frozen in a second experiment",
    );
}

/// A task manifest smaller than the planned independent units never freezes.
#[test]
fn a_manifest_smaller_than_the_planned_units_never_freezes() {
    let w = world();
    never_frozen(
        freeze_with(&w, "units", &w.inc_ref, &w.cand_ref, |v| {
            v["independent_units"] = json!(3)
        }),
        &["< independent_units"],
        "a manifest smaller than the planned units",
    );
}

/// A plan whose controls are not its policies' never freezes.
#[test]
fn a_plan_whose_controls_are_not_its_policies_never_freezes() {
    let w = world();
    never_frozen(
        freeze_with(&w, "ctl", &w.inc_ref, &w.cand_ref, |v| {
            v["controls_ref"] = json!(r('c'))
        }),
        &["controls differ from the plan's frozen controls"],
        "a plan whose controls are not its policies'",
    );
}

/// An EVO candidate proposed from ANOTHER parent (the first candidate) never
/// freezes against the incumbent it does not descend from.
#[test]
fn a_candidate_of_another_parent_never_freezes() {
    let w = world();
    let (_, grandchild) = propose(&w.s, &w.cand, 5, "cand-2");
    never_frozen(
        freeze_with(&w, "par", &w.inc_ref, &grandchild, |_| {}),
        &["parent is not the plan's incumbent"],
        "a candidate of another parent",
    );
}

/// A hand-made policy (put within the registered list, never proposed by
/// EVO) derived from the incumbent: `edit` makes it the attack. Returns its
/// ref.
fn hand_made(w: &World, id: &str, edit: impl FnOnce(&mut PolicyEnvelope)) -> Ref {
    let mut p = w.inc.clone();
    p.policy_id = PolicyId::new(id).unwrap();
    p.parent_policy_ref = w.inc_ref.clone();
    p.shortlist = vec![CandidateId::new("read").unwrap()];
    edit(&mut p);
    candidates::put_policy(&w.s, &p).unwrap()
}

/// A candidate EVO never proposed (no proposer on record), otherwise a
/// subtractive child of the incumbent, never freezes.
#[test]
fn a_candidate_evo_never_proposed_never_freezes() {
    let w = world();
    let c = hand_made(&w, "hand-1", |_| {});
    never_frozen(
        freeze_with(&w, "hand", &w.inc_ref, &c, |_| {}),
        &["was not produced by EVO"],
        "a candidate EVO never proposed",
    );
}

/// Four-cell (no proposer on record; the shortlist is subtractive): a
/// hand-made candidate that ADDS a tool of the eligible list never freezes.
#[test]
fn a_candidate_adding_a_tool_never_freezes() {
    let w = world();
    let c = hand_made(&w, "adds", |p| {
        p.shortlist = vec![
            CandidateId::new("read").unwrap(),
            CandidateId::new("write").unwrap(),
        ]
    });
    never_frozen(
        freeze_with(&w, "adds", &w.inc_ref, &c, |_| {}),
        &[
            "was not produced by EVO",
            "a shortlist mutation may only reorder/remove",
        ],
        "a candidate adding a tool",
    );
}

/// Four-cell (no proposer on record; the candidate keeps the incumbent's
/// candidate view): a hand-made candidate over ANOTHER registered candidate
/// list never freezes.
#[test]
fn a_candidate_over_another_candidate_view_never_freezes() {
    let w = world();
    let other = CandidateSet::parse(
        &json!({"schema":"axon.loop.candidate-set/1","scope":scope(),
                "candidates":["edit","read","search","shell","write"],"issuer_ref":ADMITTER})
        .to_string(),
    )
    .unwrap();
    let view = candidates::put(&w.s, &other).unwrap();
    let c = hand_made(&w, "view", |p| p.candidate_set_ref = view);
    never_frozen(
        freeze_with(&w, "view", &w.inc_ref, &c, |_| {}),
        &[
            "was not produced by EVO",
            "changes the eligible candidate view",
        ],
        "a candidate over another candidate view",
    );
}

/// Four-cell (no proposer on record IN THE PLAN'S SCOPE; the policies are the
/// plan's scope): a hand-made candidate of another tenant never freezes.
#[test]
fn a_candidate_of_another_scope_never_freezes() {
    let w = world();
    let other_scope = Scope {
        tenant_id: TenantId::new("other-tenant").unwrap(),
        task_family: scope().task_family,
    };
    let set = CandidateSet::parse(
        &json!({"schema":"axon.loop.candidate-set/1","scope":other_scope,
                "candidates":candidate_list(),"issuer_ref":ADMITTER})
        .to_string(),
    )
    .unwrap();
    candidates::put(&w.s, &set).unwrap();
    let c = hand_made(&w, "xscope", |p| p.scope = other_scope.clone());
    never_frozen(
        freeze_with(&w, "xscope", &w.inc_ref, &c, |_| {}),
        &["was not produced by EVO", "scope differs from the plan"],
        "a candidate of another scope",
    );
}

/// A frozen plan with a start blocker (the runtime is not ready) is never
/// evaluated. Control: the ready plan is.
#[test]
fn a_blocked_plan_never_starts() {
    for ready in [true, false] {
        let w = world();
        freeze_with(&w, "blk", &w.inc_ref, &w.cand_ref, |v| {
            v["runtime_ready"] = json!(ready)
        })
        .unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let v = evl_request("blk", &w.inc, &w.cand, &specs, &EvlOpts::default());
        match (ready, evaluate(&w.s, &v)) {
            (true, r) => {
                r.expect("control: a ready plan is evaluated");
            }
            (false, Ok(_)) => panic!("ATTACK: a plan with a start blocker was evaluated"),
            (false, Err(e)) => assert!(e.to_string().contains("may not start"), "{e}"),
        }
    }
}

// ── plan::assign: who issues the population, once, and of which trials ──

fn rows(w: &World, ids: [&str; 4]) -> Vec<Value> {
    let (i, c) = (&w.inc_ref, &w.cand_ref);
    let row = |task: &str, arm: &str, t: &str, p: &Ref| {
        json!({"task_id": task, "arm_id": arm, "trial_id": t,
               "attempt_id": format!("{t}-a1"), "policy_ref": p})
    };
    vec![
        row("task-0", "incumbent", ids[0], i),
        row("task-1", "incumbent", ids[1], i),
        row("task-0", "challenger-1", ids[2], c),
        row("task-1", "challenger-1", ids[3], c),
    ]
}

fn record(exp: &str, scope: Value, issuer: &str, trials: Vec<Value>) -> plan::AssignmentRecord {
    plan::parse_assignment(
        &json!({"schema":"axon.loop.assignment/1","experiment_id":exp,"scope":scope,
                "issuer_ref":issuer,"trials":trials})
        .to_string(),
    )
    .unwrap()
}

/// Each assignment defect plan::assign refuses, one per case, against a plan
/// frozen for it. Control: the honest assignment is issued.
#[test]
fn each_assignment_defect_is_never_issued() {
    let w = world();
    freeze_with(&w, "as", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let ids = ["i0", "i1", "c0", "c1"];
    let other_scope = json!({"tenant_id": "other-tenant", "task_family": scope().task_family});
    // An admitter that is also this scope's EVO proposer.
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(PROPOSER).unwrap());
    w.s.write_config(&cfg).unwrap();
    for (case, rec, why) in [
        (
            "an assignment for another scope",
            record("as", other_scope, ADMITTER, rows(&w, ids)),
            "assignment scope differs",
        ),
        (
            "an assignment from a principal that is no admitter",
            record("as", json!(scope()), WORKER, rows(&w, ids)),
            "is not a trusted admitter",
        ),
        (
            "an assignment from the scope's EVO proposer",
            record("as", json!(scope()), PROPOSER, rows(&w, ids)),
            "is an EVO proposer",
        ),
    ] {
        match plan::assign(&w.s, &rec) {
            Ok(r) => panic!("ATTACK: {case}: the population was issued as {r}"),
            Err(e) => assert!(e.to_string().contains(why), "{case}: {e}"),
        }
    }
    plan::assign(&w.s, &record("as", json!(scope()), ADMITTER, rows(&w, ids)))
        .expect("control: the honest assignment");
    // A second, different population for the same experiment.
    let again = record(
        "as",
        json!(scope()),
        ADMITTER,
        rows(&w, ["i0", "i1", "c0", "c9"]),
    );
    match plan::assign(&w.s, &again) {
        Ok(r) => panic!("ATTACK: a second population was issued as {r}"),
        Err(e) => assert!(e.to_string().contains("already has its assignment"), "{e}"),
    }
}

/// A trial id issued to one experiment is never issued to another of the
/// scope (whose trials were not yet run, so only this check stands).
#[test]
fn a_trial_id_of_another_experiment_is_never_issued() {
    let w = world();
    freeze_with(&w, "ea", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let (_, c2) = propose(&w.s, &w.inc, 7, "cand-b");
    freeze_with(&w, "eb", &w.inc_ref, &c2, |_| {}).unwrap();
    plan::assign(
        &w.s,
        &record(
            "ea",
            json!(scope()),
            ADMITTER,
            rows(&w, ["i0", "i1", "c0", "c1"]),
        ),
    )
    .unwrap();
    let row = |task: &str, arm: &str, t: &str, p: &Ref| {
        json!({"task_id": task, "arm_id": arm, "trial_id": t,
               "attempt_id": format!("{t}-a1"), "policy_ref": p})
    };
    let reused = vec![
        row("task-0", "incumbent", "i0", &w.inc_ref),
        row("task-1", "incumbent", "j1", &w.inc_ref),
        row("task-0", "challenger-1", "d0", &c2),
        row("task-1", "challenger-1", "d1", &c2),
    ];
    match plan::assign(&w.s, &record("eb", json!(scope()), ADMITTER, reused)) {
        Ok(r) => panic!("ATTACK: a trial id of another experiment was issued again as {r}"),
        Err(e) => assert!(
            e.to_string().contains("was already issued to experiment"),
            "{e}"
        ),
    }
}
