//! B282 / G32-r22-evidence-laundering: a laundered bundle — every document
//! INDIVIDUALLY schema-valid, every reference re-derived so the digests bind —
//! must not cross independent admission. Per-point refusals are tested
//! elsewhere (evl.rs, intake.rs, redteam.rs); this drives each laundering
//! class through the WHOLE chain on the candidate arm: independent evaluation
//! → admission → the fenced pointer. A positive control with the same world
//! and no laundering is Accepted, so a refusal here is the laundering's doing.
//!
//! Classes (the gate's own list): mutated receipt, forged issuer identity,
//! hash-only success, missing attempt, cross-tenant reference.

mod common;
use axon_loop::admission::Decision;
use axon_loop::evl::Outcome;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

/// Re-derive the episode's refs to its (edited) documents, so the only thing
/// wrong with the trial is the laundering itself — never a stale digest.
fn launder(
    t: &mut Value,
    edit: impl FnOnce(&mut LoopEpisode, &mut ExecutionContextReceipt, &mut ExecutionReceipt),
) {
    let mut ep: LoopEpisode = serde_json::from_value(t["episode"].clone()).unwrap();
    let mut ctx: ExecutionContextReceipt = serde_json::from_value(t["context"].clone()).unwrap();
    let req: ComputeRequest = serde_json::from_value(t["acf_request"].clone()).unwrap();
    let mut rc: ExecutionReceipt = serde_json::from_value(t["acf_receipt"].clone()).unwrap();
    edit(&mut ep, &mut ctx, &mut rc);
    ep.context_ref = digest(&ctx).unwrap();
    ep.acf_request_ref = digest(&req).unwrap();
    ep.acf_receipt_ref = digest(&rc).unwrap();
    // Individually schema-valid: every document still validates on its own.
    ep.validate().unwrap();
    ctx.validate().unwrap();
    rc.validate().unwrap();
    t["episode"] = json!(ep);
    t["context"] = json!(ctx);
    t["acf_receipt"] = json!(rc);
}

/// One experiment on a fresh world; `edit` launders the delivered trials.
/// Returns (candidate verified passes, assigned, decision, reasons, pointer moved?).
fn run(
    exp: &str,
    edit: impl FnOnce(&mut Value),
) -> (
    u64,
    u64,
    Decision,
    Vec<String>,
    bool,
    Vec<(Outcome, String)>,
) {
    let w = world();
    let before = axon_loop::pointer::load(&w.s, &scope()).unwrap();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    edit(&mut v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let outcomes = c
        .trials
        .iter()
        .map(|t| (t.outcome, t.reason.clone()))
        .collect();
    let (adm, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    let after = axon_loop::pointer::load(&w.s, &scope()).unwrap();
    (
        c.verified_pass as u64,
        c.assigned as u64,
        adm.decision,
        adm.reasons,
        before != after,
        outcomes,
    )
}

/// The candidate's delivered trials in the request (incumbent first, 2 each).
fn candidate_trials(v: &mut Value) -> impl Iterator<Item = &mut Value> {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
}

#[test]
fn a_clean_bundle_is_accepted_positive_control() {
    let (pass, assigned, d, r, moved, _) = run("clean", |_| {});
    assert_eq!((pass, assigned), (2, 2));
    assert_eq!(d, Decision::Accept, "{r:?}");
    assert!(!moved, "admission alone never moves the pointer");
}

#[test]
fn laundered_evidence_never_crosses_independent_admission() {
    let other_tree: Acf1Ref = Acf1Ref::new(format!("acf1:{}", "9".repeat(64))).unwrap();
    let forged = OpaqueRef::new("fixture:forged-verifier").unwrap();
    let foreign = TenantId::new("tenant-b").unwrap();
    type Laundry = Box<dyn Fn(&mut Value)>;
    let classes: Vec<(&str, Laundry)> = vec![
        (
            "mutated receipt (input tree swapped, digests re-derived)",
            Box::new(move |v| {
                for t in candidate_trials(v) {
                    let o = other_tree.clone();
                    launder(t, move |_, _, rc| rc.input_workspace_ref = o);
                }
            }),
        ),
        (
            "forged issuer identity (a verifier the operator never trusted)",
            Box::new(move |v| {
                for t in candidate_trials(v) {
                    let f = forged.clone();
                    launder(t, move |ep, _, _| ep.verification.issuer_ref = Some(f));
                }
            }),
        ),
        (
            "hash-only success (a receipt digest with no delivered bytes)",
            Box::new(|v| {
                for t in candidate_trials(v) {
                    let mut ep: LoopEpisode = serde_json::from_value(t["episode"].clone()).unwrap();
                    ep.acf_receipt_ref =
                        digest_value(&json!({"claimed": "a receipt nobody can read"})).unwrap();
                    ep.validate().unwrap();
                    t["episode"] = json!(ep);
                }
            }),
        ),
        (
            "missing attempt (an assigned trial never delivered)",
            Box::new(|v| {
                let trials = v["trials"].as_array_mut().unwrap();
                trials.retain(|t| t["episode"]["identity"]["trial_id"] != "c1");
            }),
        ),
        (
            "cross-tenant reference (evidence minted for another tenant)",
            Box::new(move |v| {
                for t in candidate_trials(v) {
                    let f = foreign.clone();
                    launder(t, move |ep, ctx, _| {
                        ep.scope.tenant_id = f.clone();
                        ctx.scope.tenant_id = f;
                    });
                }
            }),
        ),
    ];
    for (i, (why, laundry)) in classes.into_iter().enumerate() {
        let (pass, assigned, d, r, moved, outcomes) = run(&format!("laundry-{i}"), |v| laundry(v));
        eprintln!("PROBE {why}: {outcomes:?}");
        assert!(
            pass < assigned,
            "{why}: the laundered trials counted as verified passes ({pass}/{assigned})"
        );
        // WHICH check refused it — the class's own first-line check, not an
        // accident further down. Mutation: remove evl's cross-tenant check and
        // the cross-tenant reason changes (a second layer still refuses it).
        let want = match i {
            0 => "Fabric input mismatch",
            1 => "unknown verifier cannot establish outcome",
            2 => "Fabric reference mismatch",
            3 => "missing: no episode delivered",
            _ => "cross-tenant evidence",
        };
        assert!(
            outcomes.iter().any(|(_, reason)| reason.contains(want)),
            "{why}: expected a trial refused for `{want}`: {outcomes:?}"
        );
        if !why.starts_with("missing attempt") {
            assert_eq!(pass, 0, "{why}: {outcomes:?}");
            assert!(
                outcomes.iter().all(|(o, _)| *o != Outcome::VerifiedPass),
                "{why}: {outcomes:?}"
            );
        }
        assert_ne!(
            d,
            Decision::Accept,
            "{why}: laundered evidence was ADMITTED ({r:?})"
        );
        assert!(!moved, "{why}: the pointer moved");
    }
}
