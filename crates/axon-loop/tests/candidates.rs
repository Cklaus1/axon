//! Interop gap G2: Axon enforces `shortlist ⊆ candidates` itself, against a
//! registered candidate LIST, at every entry point. MiCode is not the only
//! enforcement point any more.
mod common;
use axon_loop::candidates::{self, CandidateSet};
use axon_loop::error::LoopError;
use axon_loop::{evo, null_policy_ref, plan, pointer};
use common::*;
use serde_json::json;

fn cs(v: serde_json::Value) -> Result<CandidateSet, LoopError> {
    CandidateSet::parse(&v.to_string())
}

#[test]
fn registered_list_digests_to_the_policy_view() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    assert_eq!(
        register_candidates(&s),
        candidate_set_ref(),
        "idempotent, same ref"
    );
    assert_eq!(incumbent().candidate_set_ref, candidate_set_ref());
    candidates::put_policy(&s, &incumbent()).unwrap();
}

#[test]
fn candidate_list_shape_and_issuer() {
    let d = tempfile::tempdir().unwrap();
    let s = store_without_candidates(d.path());
    let before = snapshot(d.path());
    // unsorted / duplicate / empty lists are malformed
    for list in [json!(["write", "read"]), json!(["read", "read"]), json!([])] {
        let mut v = candidate_set_doc(&[]);
        v["candidates"] = list.clone();
        assert!(matches!(cs(v), Err(LoopError::Malformed(_))), "{list}");
    }
    // untrusted issuer
    let mut v = candidate_set_doc(&candidate_list());
    v["issuer_ref"] = json!(WORKER);
    assert!(matches!(
        candidates::put(&s, &cs(v).unwrap()),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(d.path()), before);
}

/// G2 itself: a tool-ADDING policy is refused by `policy put`.
#[test]
fn g2_policy_put_refuses_a_tool_adding_shortlist() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let before = snapshot(d.path());
    let adds = policy("adds", &["read", "teleport"]);
    let e = candidates::put_policy(&s, &adds).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("teleport")),
        "{e}"
    );
    assert_eq!(snapshot(d.path()), before);
}

/// An unregistered candidate_set_ref is refused, never "unknown ⇒ allow".
#[test]
fn g2_unknown_candidate_view_is_refused_everywhere() {
    let d = tempfile::tempdir().unwrap();
    let s = store_without_candidates(d.path());
    let before = snapshot(d.path());
    let e = candidates::put_policy(&s, &incumbent()).unwrap_err();
    assert!(
        e.to_string().contains("not a registered candidate list"),
        "{e}"
    );
    // a policy naming a view nobody registered, in a store that has one
    let d2 = tempfile::tempdir().unwrap();
    let s2 = store_with_config(d2.path());
    let mut alien = incumbent();
    alien.candidate_set_ref = r('a');
    assert!(candidates::put_policy(&s2, &alien).is_err());
    // evo propose from an unregistered view
    let req = evo::parse_request(
        &evo_request(
            &incumbent(),
            1,
            "c",
            vec![discovery_episode(&incumbent(), "d1")],
        )
        .to_string(),
    )
    .unwrap();
    assert!(matches!(evo::propose(&s, &req), Err(LoopError::Refused(_))));
    assert_eq!(snapshot(d.path()), before);
}

/// EVO: the caller's `eligible` must be the registered list (it cannot widen
/// the mutation space), and the incumbent must be within it.
#[test]
fn g2_evo_eligible_must_equal_the_registered_list() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let before = snapshot(d.path());
    let mut v = evo_request(
        &incumbent(),
        1,
        "c",
        vec![discovery_episode(&incumbent(), "d1")],
    );
    v["eligible"] = json!(["edit", "read", "search", "teleport", "write"]);
    let e = evo::propose(&s, &evo::parse_request(&v.to_string()).unwrap()).unwrap_err();
    assert!(e.to_string().contains("registered candidate list"), "{e}");
    assert_eq!(snapshot(d.path()), before);
}

/// Baseline designation and activation re-check against the list, so a
/// tool-adding policy that reached `policies/` by any other route (hand-
/// written CAS file) still cannot be pinned.
#[test]
fn g2_baseline_and_activation_recheck_the_list() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let adds = policy("adds", &["read", "teleport"]);
    let aref = s.put_cas("policies", &adds).unwrap(); // bypasses `policy put`
    let before = snapshot(d.path());
    let e = pointer::designate_baseline(
        &s,
        &pointer::parse_baseline(&baseline_doc(&aref).to_string()).unwrap(),
    )
    .unwrap_err();
    assert!(e.to_string().contains("teleport"), "{e}");
    assert_eq!(snapshot(d.path()), before);

    // An honest baseline, then a later re-check at activation time.
    let w = world();
    let adds_ref = w.s.put_cas("policies", &adds).unwrap();
    let before = snapshot(w.dir.path());
    let e = pointer::transition(
        &w.s,
        &tparse(&transition(
            "x",
            "rollback",
            &w.inc_ref,
            Some(&adds_ref),
            1,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap_err();
    assert!(matches!(e, LoopError::Refused(_)), "{e}");
    assert_eq!(snapshot(w.dir.path()), before);
    let _ = null_policy_ref();
}

/// Freeze (and therefore admit, which re-runs the same candidate check)
/// refuses a plan whose candidate is outside the registered list.
#[test]
fn g2_freeze_refuses_a_candidate_outside_the_list() {
    let w = world();
    let mut adds = policy("adds-cand", &["read", "teleport"]);
    adds.parent_policy_ref = w.inc_ref.clone();
    let aref = w.s.put_cas("policies", &adds).unwrap();
    plan::register(
        &w.s,
        &plan::PilotPlan::from_value(&complete_plan("addexp", &w.inc_ref, &aref)).unwrap(),
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    let e = plan::freeze(&w.s, "addexp").unwrap_err();
    assert!(e.to_string().contains("teleport"), "{e}");
    assert_eq!(snapshot(w.dir.path()), before);
}

fn other_scope_doc(mut v: serde_json::Value) -> serde_json::Value {
    v["scope"]["tenant_id"] = json!("tenant-b");
    v
}

/// NS3a/b (rt4i): a trusted admitter registers the IDENTICAL candidate list
/// for another scope. Scope A must be unaffected: its genuinely admitted
/// activation and an EVO proposal still succeed. Before NS3 the second
/// registration overwrote `candidate-sets/<hex>.json` and every scope-A path
/// exited 2 "store corrupt", with no CLI repair.
#[test]
fn ns3ab_same_candidate_list_for_another_scope_does_not_brick_the_first() {
    let w = world();
    let adm = accepted(&w, "exp");
    let b = cs(other_scope_doc(candidate_set_doc(&candidate_list()))).unwrap();
    assert_eq!(candidates::put(&w.s, &b).unwrap(), candidate_set_ref());
    // the ref is registered for both scopes, each with its own bytes
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "ns3",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .expect("scope A activation must survive tenant-b's registration");
    propose(&w.s, &w.inc, 9, "cand-ns3");
    // and a repeat of either registration is an idempotent no-op
    let before = snapshot(w.dir.path());
    candidates::put(&w.s, &b).unwrap();
    register_candidates(&w.s);
    assert_eq!(snapshot(w.dir.path()), before);
}

/// NS3c (rt4i): the same for task manifests — scope A still freezes over its
/// own (identical) registered manifest after tenant-b registers it.
#[test]
fn ns3c_same_task_manifest_for_another_scope_does_not_brick_the_first() {
    let w = world();
    let mut tasks: Vec<String> = (0..2).map(|i| format!("task-{i}")).collect();
    tasks.sort();
    let m = axon_loop::tasks::TaskManifest::parse(
        &json!({"schema":"axon.loop.task-manifest/1",
                "scope":{"tenant_id":"tenant-b","task_family":"fixture-coding"},
                "tasks":tasks,"issuer_ref":ADMITTER})
        .to_string(),
    )
    .unwrap();
    assert_eq!(
        axon_loop::tasks::put(&w.s, &m).unwrap(),
        task_manifest_ref(2)
    );
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {})
        .expect("scope A freeze must survive tenant-b's manifest registration");
}

/// NS3b repair (rt4i): a store written BEFORE NS3 kept the list at the flat
/// `candidate-sets/<hex>.json`, which another scope's registration
/// overwrote. The flat file is read only when its bytes name the right
/// scope, so scope A is refused (exit 2), and re-putting scope A's list
/// now REPAIRS it (before: an idempotent no-op that left it bricked).
#[test]
fn ns3b_a_pre_ns3_overwritten_store_is_repaired_by_re_putting_the_list() {
    let w = world();
    let adm = accepted(&w, "exp");
    let root = w.s.root().to_path_buf();
    let hex = candidate_set_ref().hex().to_string();
    let scoped = root
        .join("candidate-sets/fixture-tenant/fixture-coding")
        .join(format!("{hex}.json"));
    // Reconstruct the pre-NS3 broken state: only a flat file, holding the
    // tenant-b document.
    let b = cs(other_scope_doc(candidate_set_doc(&candidate_list()))).unwrap();
    std::fs::remove_file(&scoped).unwrap();
    std::fs::write(
        root.join("candidate-sets").join(format!("{hex}.json")),
        axon_loop_contracts::canonical_json(&b).unwrap(),
    )
    .unwrap();
    let act = |id: &str| {
        pointer::transition(
            &w.s,
            &tparse(&transition(
                id,
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                false,
            )),
        )
    };
    assert_eq!(act("before").unwrap_err().exit_code(), 2);
    register_candidates(&w.s);
    act("after").expect("re-put must repair scope A");
}
