//! B265 (G32) — receipt roles and artifact recheck at the evaluation join.
//!
//! Every case starts from a delivered trial whose documents genuinely bind
//! (a positive control proves it is a VerifiedPass), changes ONE thing, and
//! re-derives every digest so the only failure left is the one under test.

mod common;
use axon_loop::evl::Outcome;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

/// Re-derive the episode's refs to its (edited) documents.
fn rebind(
    t: &mut Value,
    edit: impl FnOnce(
        &mut LoopEpisode,
        &mut ExecutionContextReceipt,
        &mut ComputeRequest,
        &mut ExecutionReceipt,
    ),
) {
    let mut ep: LoopEpisode = serde_json::from_value(t["episode"].clone()).unwrap();
    let mut ctx: ExecutionContextReceipt = serde_json::from_value(t["context"].clone()).unwrap();
    let mut req: ComputeRequest = serde_json::from_value(t["acf_request"].clone()).unwrap();
    let mut rc: ExecutionReceipt = serde_json::from_value(t["acf_receipt"].clone()).unwrap();
    edit(&mut ep, &mut ctx, &mut req, &mut rc);
    ep.context_ref = digest(&ctx).unwrap();
    ep.acf_request_ref = digest(&req).unwrap();
    ep.acf_receipt_ref = digest(&rc).unwrap();
    t["episode"] = json!(ep);
    t["context"] = json!(ctx);
    t["acf_request"] = json!(req);
    t["acf_receipt"] = json!(rc);
}

/// Evaluate a fresh experiment in which challenger trial `c0` was delivered
/// as `edit` leaves it; return c0's (outcome, reason).
fn judge_c0(edit: impl FnOnce(&mut Value)) -> (Outcome, String) {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert_eq!(v["trials"][2]["episode"]["identity"]["trial_id"], "c0");
    edit(&mut v["trials"][2]);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let t = c
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == "c0")
        .unwrap();
    // The untouched sibling stays a pass: the change is the only difference.
    let c1 = c
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == "c1")
        .unwrap();
    assert_eq!(c1.outcome, Outcome::VerifiedPass, "{}", c1.reason);
    (t.outcome, t.reason.clone())
}

#[test]
fn positive_control_the_unedited_trial_is_a_verified_pass() {
    let (o, why) = judge_c0(|t| rebind(t, |_, _, _, _| {}));
    assert_eq!(o, Outcome::VerifiedPass, "{why}");
}

// ── roles ───────────────────────────────────────────────────────────────────

#[test]
fn the_execution_receipt_cannot_stand_as_independent_verification() {
    let (o, why) = judge_c0(|t| {
        rebind(t, |_, _, _, _| {});
        // After the refs are final: cite the receipt as the verifier's evidence.
        let rref = t["episode"]["acf_receipt_ref"].clone();
        t["episode"]["verification"]["evidence_refs"] = json!([rref]);
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("role upgrade"), "{why}");
}

#[test]
fn a_preflight_context_cannot_stand_as_the_verifier() {
    let (o, why) = judge_c0(|t| {
        rebind(t, |_, _, _, _| {});
        let cref = t["episode"]["context_ref"].clone();
        t["episode"]["verification"]["verifier_ref"] = cref;
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("role upgrade"), "{why}");
}

#[test]
fn a_worker_reported_receipt_is_not_supervisor_observed_process_facts() {
    let (o, why) = judge_c0(|t| {
        rebind(t, |_, _, _, rc| {
            rc.evidence_source = EvidenceSource::WorkerReported;
        })
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("not supervisor-observed"), "{why}");
}

#[test]
fn a_context_document_offered_as_the_execution_receipt_is_refused_whole() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    v["trials"][2]["acf_receipt"] = v["trials"][2]["context"].clone();
    let before = snapshot(w.dir.path());
    assert!(evaluate(&w.s, &v).is_err(), "a context is not a receipt");
    assert_eq!(snapshot(w.dir.path()), before, "and nothing is recorded");
}

// ── artifact recheck ────────────────────────────────────────────────────────

fn acf1(c: char) -> Acf1Ref {
    Acf1Ref::new(format!("acf1:{}", c.to_string().repeat(64))).unwrap()
}

#[test]
fn output_bytes_changed_after_verification_are_not_a_pass() {
    // The execution left different bytes than the verifier checked.
    let (o, why) = judge_c0(|t| {
        rebind(t, |_, _, _, rc| {
            rc.output_workspace_ref = Some(acf1('e'));
        })
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("output"), "{why}");
    // The whole chain moved together EXCEPT the verification: still no pass.
    let (o, why) = judge_c0(|t| {
        rebind(t, |ep, _, _, rc| {
            rc.output_workspace_ref = Some(acf1('e'));
            ep.output_workspace_ref = Some(acf1('e'));
        })
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("output"), "{why}");
}

/// `bind_acf` holds the artifact rule ITSELF — not only transitively through
/// `bind_episode` — so a caller that joins ACF evidence alone cannot accept
/// a pass for bytes other than the ones the verifier checked.
#[test]
fn bind_acf_alone_refuses_a_pass_over_other_bytes() {
    let w = world();
    let tr = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    let mut t = trial(&tr);
    let parts = |t: &Value| {
        (
            parse::<LoopEpisode>(&t["episode"].to_string()).unwrap(),
            parse::<ComputeRequest>(&t["acf_request"].to_string()).unwrap(),
            parse::<ExecutionReceipt>(&t["acf_receipt"].to_string()).unwrap(),
            parse::<PolicyProjection>(&t["projection"].to_string()).unwrap(),
        )
    };
    let (ep, req, rc, proj) = parts(&t);
    bind_acf(&ep, &req, &rc, &proj).expect("positive control binds");
    // Episode and receipt agree on an output the verifier never checked.
    rebind(&mut t, |ep, _, _, rc| {
        rc.output_workspace_ref = Some(acf1('e'));
        ep.output_workspace_ref = Some(acf1('e'));
    });
    let (ep, req, rc, proj) = parts(&t);
    assert_ne!(
        ep.verification.output_workspace_ref,
        rc.output_workspace_ref
    );
    let e = bind_acf(&ep, &req, &rc, &proj).unwrap_err().to_string();
    assert!(e.contains("bytes changed after verification"), "{e}");
}

// ── tenancy ─────────────────────────────────────────────────────────────────

#[test]
fn evidence_from_another_tenant_never_joins_this_evaluation() {
    let (o, why) = judge_c0(|t| {
        rebind(t, |ep, ctx, _, _| {
            let other = Scope {
                tenant_id: TenantId::new("other-tenant").unwrap(),
                task_family: TaskFamily::new("fixture-coding").unwrap(),
            };
            ep.scope = other.clone();
            ctx.scope = other;
        })
    });
    assert_eq!(o, Outcome::Unknown);
    assert!(why.contains("cross-tenant"), "{why}");
}
