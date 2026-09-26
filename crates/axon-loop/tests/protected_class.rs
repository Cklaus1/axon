//! ADR-001 D3: the local interpreter is DEVELOPMENT-ONLY. A protected plan
//! counts a trial only when every receipt it rests on came from a
//! `PROTECTED_PROFILES` backend, and a protected scope promotes only on a
//! protected evaluation. Each refusal is paired with a positive control that
//! differs only in the backend (or the class), so the refusal is D3's doing.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::evl::Outcome;
use axon_loop::pointer;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

const PROTECTED: &str = "linux-microvm-protected";

/// Protect the scope (operator config) — a plan frozen after this is a
/// PROTECTED evaluation.
fn protect(s: &axon_loop::store::Store) {
    let mut cfg = s.config().unwrap();
    cfg.protected_scopes.push(scope());
    s.write_config(&cfg).unwrap();
}

/// Every delivered trial's receipts re-stamped as coming from the protected
/// backend: the execution receipt (episode `acf_receipt_ref` re-derived) and
/// the verification receipt (episode `verifier_ref` re-derived, attestation
/// genuinely re-signed).
fn on_protected_backend(v: &mut Value) {
    for t in v["trials"].as_array_mut().unwrap() {
        t["acf_receipt"]["backend_profile_ref"] = json!(PROTECTED);
        t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
        if t["verification_receipt"].is_object() {
            t["verification_receipt"]["backend_profile_ref"] = json!(PROTECTED);
            t["episode"]["verification"]["verifier_ref"] =
                json!(digest_value(&t["verification_receipt"]).unwrap());
            t["verification_attestation"] = attest(
                VERIFIER,
                &t["verification_request"],
                &t["verification_receipt"],
            );
        }
    }
}

fn specs_for(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}

fn pin_protected_backend(s: &axon_loop::store::Store) {
    let mut cfg = s.config().unwrap();
    cfg.verifier_pins
        .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
        .unwrap()
        .backend_profiles
        .push(PROTECTED.into());
    s.write_config(&cfg).unwrap();
}

/// A protected plan evaluated on the development backend: every trial is
/// Unknown for D3's reason, nothing is admitted. The same plan with the
/// receipts from the protected backend verifies normally.
///
/// Mutation: delete the protected-class block in `evl::judge` → the
/// development-backend trials count and the first half fails.
#[test]
fn a_protected_plan_counts_only_protected_backends() {
    let w = world();
    protect(&w.s);
    freeze_plan(&w.s, "prot-dev", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (rec, e) = evaluate(
        &w.s,
        &evl_request("prot-dev", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    for arm in &rec.arms {
        assert_eq!(arm.verified_pass, 0, "{arm:?}");
        assert!(
            arm.trials.iter().all(|t| t.outcome == Outcome::Unknown
                && t.reason.contains("ineligible for a protected evaluation")),
            "{:?}",
            arm.trials
        );
    }
    let (adm, _) = admit(&w.s, "prot-dev", &e, ADMITTER, false).unwrap();
    assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);

    // Positive control: same class, receipts from the protected backend.
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "prot-ok", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut v = evl_request("prot-ok", &w.inc, &w.cand, &specs, &EvlOpts::default());
    on_protected_backend(&mut v);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(c.verified_pass, 2, "{:?}", c.trials);
}

/// A protected scope refuses to promote on a development-class evaluation,
/// and writes nothing; a protected evaluation promotes there.
///
/// Mutation: delete the protected-scope block in `pointer::check_activate` →
/// the development evaluation activates and the first half fails.
#[test]
fn a_protected_scope_promotes_only_on_a_protected_evaluation() {
    let activate = |w: &World, adm: &Ref| {
        pointer::transition(
            &w.s,
            &tparse(&transition(
                "a1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(adm),
                false,
            )),
        )
    };

    // A development evaluation (frozen BEFORE the operator protected the
    // scope — the class is fixed at freeze) cannot promote once the scope is
    // protected: refused, nothing written.
    let w = world();
    freeze_plan(&w.s, "dev", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    protect(&w.s);
    let (_, e) = evaluate(
        &w.s,
        &evl_request("dev", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default()),
    )
    .unwrap();
    let (rec, adm) = admit(&w.s, "dev", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    let before = snapshot(w.dir.path());
    let err = activate(&w, &adm).unwrap_err();
    assert!(
        matches!(err, LoopError::Refused(ref m) if m.contains("scope is protected")),
        "{err}"
    );
    assert_eq!(snapshot(w.dir.path()), before);

    // Protected evaluation on the protected backend: promotes.
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "prot", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut v = evl_request("prot", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, adm) = admit(&w.s, "prot", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    activate(&w, &adm).unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.cand_ref
    );
}
