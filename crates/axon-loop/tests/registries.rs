//! Amendment 61 (C9 round 4b, rows4a): the two operator registries a plan and
//! a policy rest on (the task manifest, the eligible candidate list), judged
//! on their production routes: registration (`tasks::put`,
//! `candidates::put`), a plan freeze, a population issue and a policy put.
//! Each test is an ATTACK with its CONTROL.

mod common;
use axon_loop::candidates::{self, CandidateSet};
use axon_loop::tasks::{self, TaskManifest};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn manifest_doc(tasks: &[&str], issuer: &str) -> Value {
    json!({"schema":"axon.loop.task-manifest/1","scope":scope(),"tasks":tasks,"issuer_ref":issuer})
}

fn candidates_doc(cands: &[&str], issuer: &str) -> Value {
    json!({"schema":"axon.loop.candidate-set/1","scope":scope(),"candidates":cands,"issuer_ref":issuer})
}

/// Register a task manifest the production way: parse its text, then put.
fn put_tasks(w: &World, doc: &Value) -> Result<Ref, axon_loop::error::LoopError> {
    tasks::put(&w.s, &TaskManifest::parse(&doc.to_string())?)
}

fn put_candidates(w: &World, doc: &Value) -> Result<Ref, axon_loop::error::LoopError> {
    candidates::put(&w.s, &CandidateSet::parse(&doc.to_string())?)
}

/// Each defective task manifest is never registered; the honest one is.
#[test]
fn each_defective_task_manifest_is_never_registered() {
    let w = world();
    put_tasks(&w, &manifest_doc(&["task-a", "task-b"], ADMITTER)).expect("control");
    for (case, doc, why) in [
        (
            "an empty task manifest",
            manifest_doc(&[], ADMITTER),
            "1..=100000 items",
        ),
        (
            "an unsorted task manifest",
            manifest_doc(&["task-b", "task-a"], ADMITTER),
            "sorted and duplicate-free",
        ),
        (
            "a task manifest from a principal that is no admitter",
            manifest_doc(&["task-c"], WORKER),
            "not a trusted admitter",
        ),
    ] {
        match put_tasks(&w, &doc) {
            Ok(r) => panic!("ATTACK: {case} was registered as {r}"),
            Err(e) => assert!(e.to_string().contains(why), "{case}: {e}"),
        }
    }
}

/// Each defective candidate list is never registered; the honest one is.
#[test]
fn each_defective_candidate_list_is_never_registered() {
    let w = world();
    put_candidates(&w, &candidates_doc(&["read", "search"], ADMITTER)).expect("control");
    for (case, doc, why) in [
        (
            "an empty candidate list",
            candidates_doc(&[], ADMITTER),
            "1..=4096 items",
        ),
        (
            "an unsorted candidate list",
            candidates_doc(&["search", "read"], ADMITTER),
            "sorted and duplicate-free",
        ),
        (
            "a candidate list from a principal that is no admitter",
            candidates_doc(&["edit"], WORKER),
            "not a trusted admitter",
        ),
    ] {
        match put_candidates(&w, &doc) {
            Ok(r) => panic!("ATTACK: {case} was registered as {r}"),
            Err(e) => assert!(e.to_string().contains(why), "{case}: {e}"),
        }
    }
}

/// A task manifest a store writer PLANTED (its file at the scope's path, no
/// registration event) never freezes a plan. Control: the registered one does.
#[test]
fn a_planted_task_manifest_never_freezes_a_plan() {
    for planted in [false, true] {
        let w = world();
        // Three tasks: the fixture world registers the two-task manifest.
        let doc = manifest_doc(&["task-0", "task-1", "task-2"], ADMITTER);
        let m = TaskManifest::parse(&doc.to_string()).unwrap();
        let r = m.manifest_ref().unwrap();
        if planted {
            let p = w.s.scoped_cas_path("task-manifests", &scope(), &r).unwrap();
            w.s.write_atomic(&p, &canonical_json(&m).unwrap()).unwrap();
        } else {
            tasks::put(&w.s, &m).unwrap();
        }
        let mut plan = complete_plan("pm", &w.inc_ref, &w.cand_ref);
        plan["task_manifest_ref"] = json!(r);
        let frozen = axon_loop::plan::PilotPlan::from_value(&plan)
            .and_then(|p| axon_loop::plan::register(&w.s, &p))
            .and_then(|_| axon_loop::plan::freeze(&w.s, "pm"));
        match (planted, frozen) {
            (false, r) => {
                r.expect("control: a registered manifest freezes");
            }
            (true, Ok(_)) => panic!("ATTACK: a plan froze over a planted task manifest"),
            (true, Err(e)) => assert!(
                e.to_string().contains("not a registered task manifest"),
                "{e}"
            ),
        }
    }
}

/// A REGISTERED task manifest's file rewritten in place by a store writer
/// (two tasks become one) never decides a population: the population over
/// one task is not issued. Control: the unedited manifest refuses that
/// population for its own reason (it does not cover the manifest).
#[test]
fn a_task_manifest_edited_in_place_never_decides_a_population() {
    let w = world();
    freeze_plan(&w.s, "me", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let r = task_manifest_ref(2);
    let p = w.s.scoped_cas_path("task-manifests", &scope(), &r).unwrap();
    let one = TaskManifest::parse(&manifest_doc(&["task-0"], ADMITTER).to_string()).unwrap();
    std::fs::write(&p, canonical_json(&one).unwrap()).unwrap();
    let row = |arm: &str, t: &str, pol: &Ref| {
        json!({"task_id": "task-0", "arm_id": arm, "trial_id": t,
               "attempt_id": format!("{t}-a1"), "policy_ref": pol})
    };
    let rec = assignment_record(
        "me",
        vec![
            row("incumbent", "i0", &w.inc_ref),
            row("challenger-1", "c0", &w.cand_ref),
        ],
    );
    match axon_loop::plan::assign(&w.s, &rec) {
        Ok(_) => panic!("ATTACK: a population over an edited task manifest was issued"),
        Err(e) => assert!(e.to_string().contains("does not match its name"), "{e}"),
    }
}

/// A candidate list PLANTED by a store writer (no registration event) never
/// lets a policy in. Control: registered, the same policy is stored.
#[test]
fn a_planted_candidate_list_never_admits_a_policy() {
    for planted in [false, true] {
        let w = world();
        let doc = candidates_doc(&["read", "search", "shell"], ADMITTER);
        let c = CandidateSet::parse(&doc.to_string()).unwrap();
        let r = c.candidate_set_ref().unwrap();
        if planted {
            let p = w.s.scoped_cas_path("candidate-sets", &scope(), &r).unwrap();
            w.s.write_atomic(&p, &canonical_json(&c).unwrap()).unwrap();
        } else {
            candidates::put(&w.s, &c).unwrap();
        }
        let mut pol = policy("shell-policy", &["shell"]);
        pol.candidate_set_ref = r;
        match (planted, candidates::put_policy(&w.s, &pol)) {
            (false, r) => {
                r.expect("control: within a registered list");
            }
            (true, Ok(_)) => panic!("ATTACK: a policy was stored over a planted candidate list"),
            (true, Err(e)) => assert!(
                e.to_string().contains("not a registered candidate list"),
                "{e}"
            ),
        }
    }
}

/// A REGISTERED candidate list's file rewritten in place to add a tool never
/// lets a policy shortlist that tool. Control: the unedited list refuses it
/// for its own reason (the tool is not eligible).
#[test]
fn a_candidate_list_edited_in_place_never_admits_a_policy() {
    let w = world();
    let c =
        CandidateSet::parse(&candidates_doc(&["read", "search"], ADMITTER).to_string()).unwrap();
    let r = candidates::put(&w.s, &c).unwrap();
    let mut pol = policy("shell-policy", &["shell"]);
    pol.candidate_set_ref = r.clone();
    let e = candidates::put_policy(&w.s, &pol).unwrap_err();
    assert!(
        e.to_string().contains("expands eligible candidates"),
        "control: {e}"
    );
    let more =
        CandidateSet::parse(&candidates_doc(&["read", "search", "shell"], ADMITTER).to_string())
            .unwrap();
    let p = w.s.scoped_cas_path("candidate-sets", &scope(), &r).unwrap();
    std::fs::write(&p, canonical_json(&more).unwrap()).unwrap();
    match candidates::put_policy(&w.s, &pol) {
        Ok(_) => panic!("ATTACK: a policy was stored over an edited candidate list"),
        Err(e) => assert!(e.to_string().contains("does not match its name"), "{e}"),
    }
}

/// A plan whose rule is not the one the code executes never freezes: the
/// frozen rule set is exactly what admission runs. One case per refusal of
/// `Rules::parse`; control: the complete plan freezes.
#[test]
fn each_unexecutable_rule_never_freezes() {
    let w = world();
    freeze_plan(&w.s, "rules-ok", &w.inc_ref, &w.cand_ref, |_| {}).expect("control");
    for (n, (case, field, value)) in [
        (
            "an order rule the code does not run",
            "order_rule",
            "anything goes",
        ),
        (
            "a 100% quality margin",
            "quality_margin",
            "pass_rate_margin_ppm=1000000",
        ),
        (
            "an economic threshold above 100%",
            "economic_threshold",
            "min_cost_reduction_ppm=1000001",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        // One world per case: a candidate freezes in one experiment only, so
        // a shared world would refuse every case for that reason instead.
        let w = world();
        let id = format!("rules-{n}");
        let r = freeze_plan(&w.s, &id, &w.inc_ref, &w.cand_ref, |p| {
            p[field] = json!(value)
        });
        match r {
            Ok(_) => panic!("ATTACK: {case}: the plan froze"),
            Err(e) => assert!(e.to_string().contains("cannot freeze"), "{case}: {e}"),
        }
    }
}
