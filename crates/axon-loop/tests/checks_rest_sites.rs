//! Amendment 64 (C9 round 4b, integrate-2): the last refusal sites of
//! `axon-loop-contracts/src/checks.rs`, each judged on the production route
//! that reads the attacker's document — `candidates::put_policy` after the
//! CLI's `parse` (the policy put verb), and `evl::evaluate` after intake (the
//! production order).
//!
//! Both are four-cell retirements: the test is the attack the retired check
//! and its siblings refuse, accepting any of their reasons; only the attack
//! getting through is the failure.

mod common;
use axon_loop::error::LoopError;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

/// M1400 (retired EQUIVALENT, set with M1220 and M1238): a policy with an
/// EMPTY shortlist is never stored. `check_shortlist`'s own emptiness check
/// (checks.rs) is the third refusal on this route: the schema's `minItems`
/// (the walker, M1220) and `PolicyEnvelope::validate`'s array bound
/// (`check_array`, M1238) refuse the same document first.
#[test]
fn a_policy_with_an_empty_shortlist_is_never_stored() {
    let put = |edit: &dyn Fn(&mut Value)| -> Result<Ref, LoopError> {
        let d = tempfile::tempdir().unwrap();
        let s = store_with_config(d.path());
        let mut v = serde_json::to_value(incumbent()).unwrap();
        v["policy_id"] = json!("pol-empty");
        edit(&mut v);
        let p: PolicyEnvelope = parse(&v.to_string()).map_err(LoopError::Malformed)?;
        axon_loop::candidates::put_policy(&s, &p)
    };
    put(&|_| {}).unwrap_or_else(|e| panic!("CONTROL: an honest policy was refused: {e}"));
    match put(&|v| v["shortlist"] = json!([])) {
        Ok(r) => panic!("ATTACK: a policy with an empty shortlist was stored ({r})"),
        Err(e) => {
            let e = e.to_string();
            assert!(
                ["fewer than 1", "shortlist: 0 items", "empty shortlist"]
                    .iter()
                    .any(|y| e.contains(y)),
                "{e}"
            );
        }
    }
}

/// M1402 (retired EQUIVALENT, pair with M45): the verification's own CHECK
/// request never doubles as the episode's EXECUTION request. The trial
/// delivers the registered check's request as its ACF request (and moves the
/// execution's input tree onto the checked output so every other join of
/// `bind_acf` holds, with the observer's context re-signed over it): its
/// verification then cites, as its evidence, a document of the very execution
/// it judges. Intake's role rule (M45) refuses the episode; with that removed,
/// evaluation's (`bind_acf`'s) refuses to count it. Control: the sibling trial
/// c1, untouched, is a verified pass.
#[test]
fn a_check_request_doubling_as_the_execution_request_never_counts() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    {
        let t = &mut v["trials"][2];
        assert_eq!(t["episode"]["identity"]["trial_id"], "c0");
        let vreq = t["verification_request"].clone();
        t["acf_request"] = vreq.clone();
        t["episode"]["acf_request_ref"] = json!(digest_value(&vreq).unwrap());
        let out = t["episode"]["output_workspace_ref"].clone();
        t["episode"]["input_workspace_ref"] = out.clone();
        t["acf_receipt"]["input_workspace_ref"] = out.clone();
        let rc = t["acf_receipt"].clone();
        t["episode"]["acf_receipt_ref"] = json!(digest_value(&rc).unwrap());
        t["context"]["observed"]["workspace_ref"] = out.clone();
        t["context"]["expected"]["workspace_ref"] = out;
        let ctx = t["context"].clone();
        let c: ExecutionContextReceipt = serde_json::from_value(ctx.clone()).unwrap();
        t["context_signature"] = axon_loop_contracts::attestation::sign_document(
            &observer_key().0,
            axon_loop::evl::CONTEXT_DOMAIN,
            &c.observed_issuer_ref,
            &ctx,
        )
        .unwrap();
        t["episode"]["context_ref"] = json!(digest_value(&ctx).unwrap());
    }
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
    let get = |id: &str| {
        arm.trials
            .iter()
            .find(|t| t.trial_id.as_str() == id)
            .unwrap()
    };
    let c1 = get("c1");
    assert_eq!(
        c1.outcome,
        axon_loop::evl::Outcome::VerifiedPass,
        "CONTROL: {}",
        c1.reason
    );
    let c0 = get("c0");
    if c0.outcome == axon_loop::evl::Outcome::VerifiedPass {
        panic!(
            "ATTACK: a verification citing its own execution request as evidence counted as a pass"
        );
    }
    let why = first_refusal("c0", &c0.reason);
    assert!(why.contains("role upgrade"), "{why}");
}
