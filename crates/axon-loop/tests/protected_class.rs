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
    on_backends(v, true, true)
}

/// Re-stamp each leg independently: `exec` / `verif` true = the protected
/// backend, false = left on the development backend.
fn on_backends(v: &mut Value, exec: bool, verif: bool) {
    for t in v["trials"].as_array_mut().unwrap() {
        if exec {
            t["acf_receipt"]["backend_profile_ref"] = json!(PROTECTED);
        }
        t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
        if t["verification_receipt"].is_object() && verif {
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
    // ADR-001 §5: a protected evaluation ACCEPTs only trials an independent,
    // authenticated monitor cleared.
    clear_all(&w.s, &v);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, adm) = admit(&w.s, "prot", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    activate(&w, &adm).unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.cand_ref
    );
}

/// World `w` with the candidate admitted on a DEVELOPMENT-class evaluation
/// (frozen before any protection): `(admission_ref)`. `mech` makes every
/// trial a mechanism test and the admission a mechanism-test admission.
fn dev_admitted(w: &World, exp: &str, mech: bool) -> Ref {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let o = EvlOpts {
        role: if mech {
            CorpusRole::MechanismTest
        } else {
            CorpusRole::Confirmation
        },
        ..EvlOpts::default()
    };
    let (_, e) = evaluate(&w.s, &evl_request(exp, &w.inc, &w.cand, &specs_for(w), &o)).unwrap();
    let (rec, adm) = admit(&w.s, exp, &e, ADMITTER, mech).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    adm
}

fn apply(w: &World, t: Value) -> Result<Option<Ref>, LoopError> {
    pointer::transition(&w.s, &tparse(&t)).map(|p| p.active_policy_ref)
}

/// Re-audit 3 (two auditors, executed): the D3 check skipped any transition
/// labelled `mechanism_test`, so a development-class evaluation activated —
/// and was served — in a protected scope. A protected scope serves no
/// mechanism-test fixture. Positive control: unprotected, it activates.
///
/// Mutation: drop the mechanism-test refusal in `protected_scope_gate` → red.
#[test]
fn a_protected_scope_serves_no_mechanism_test_fixture() {
    let w = world();
    let adm = dev_admitted(&w, "mech", true);
    protect(&w.s);
    let before = snapshot(w.dir.path());
    let e = apply(
        &w,
        transition(
            "m1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            true,
        ),
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("serves no mechanism-test")),
        "{e}"
    );
    assert_eq!(snapshot(w.dir.path()), before);

    let w = world();
    let adm = dev_admitted(&w, "mech", true);
    assert_eq!(
        apply(
            &w,
            transition(
                "m1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                true
            )
        )
        .unwrap(),
        Some(w.cand_ref.clone())
    );
}

/// Re-audit 3 (executed): rollback never consulted D3, so a policy admitted
/// on development evidence came back active after the scope was protected.
/// In a protected scope, rolling back to the incumbent-of-record is allowed
/// (the designed exemption); rolling back to the dev-admitted candidate is
/// refused and writes nothing. Positive control: unprotected, both succeed.
///
/// Mutation: skip `protected_scope_gate` for rollback → red.
#[test]
fn a_rollback_in_a_protected_scope_needs_a_protected_admission() {
    for protected in [true, false] {
        let w = world();
        let adm = dev_admitted(&w, "rb", false);
        apply(
            &w,
            transition(
                "a1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                false,
            ),
        )
        .unwrap();
        if protected {
            protect(&w.s);
        }
        // Back to the incumbent-of-record: exempt, allowed either way.
        apply(
            &w,
            transition(
                "r1",
                "rollback",
                &w.cand_ref,
                Some(&w.inc_ref),
                2,
                Some(&w.baseline),
                false,
            ),
        )
        .unwrap();
        let before = snapshot(w.dir.path());
        let r = apply(
            &w,
            transition(
                "r2",
                "rollback",
                &w.inc_ref,
                Some(&w.cand_ref),
                3,
                Some(&adm),
                false,
            ),
        );
        if protected {
            let e = r.unwrap_err();
            assert!(
                matches!(e, LoopError::Refused(ref m) if m.contains("development-class evaluation")),
                "{e}"
            );
            assert_eq!(snapshot(w.dir.path()), before);
        } else {
            assert_eq!(r.unwrap(), Some(w.cand_ref.clone()));
        }
    }
}

/// Re-audit 3 (mutation reviewer, executed): every D3 test flipped BOTH
/// receipts together, so dropping either leg from the check survived the whole
/// suite. Each leg alone on the development backend now counts nothing, for a
/// reason naming that leg; the both-protected control counts 2.
///
/// Mutations: drop the execution leg (M19) / the verification leg (M20) → red.
#[test]
fn each_d3_leg_on_a_development_backend_counts_nothing() {
    for (exec, verif, leg) in [
        (true, false, Some("verification")),
        (false, true, Some("execution")),
        (true, true, None),
    ] {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        freeze_plan(&w.s, "legs", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let mut v = evl_request("legs", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_backends(&mut v, exec, verif);
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let c = rec.arm_for_policy(&w.cand_ref).unwrap();
        match leg {
            Some(leg) => {
                assert_eq!(c.verified_pass, 0, "{leg}: {:?}", c.trials);
                assert!(
                    c.trials
                        .iter()
                        .all(|t| t.reason.contains(&format!("development {leg} backend"))),
                    "{leg}: {:?}",
                    c.trials
                );
            }
            None => assert_eq!(c.verified_pass, 2, "{:?}", c.trials),
        }
    }
}

/// ADR-001 §5: in a PROTECTED evaluation an unknown safety state blocks
/// ACCEPT — nothing independent and authenticated cleared the trials. The
/// same protected experiment with every trial cleared by the monitor is
/// ACCEPTED (a_protected_scope_promotes_only_on_a_protected_evaluation).
///
/// Mutation: drop the protected unknown-safety block in `decide` → ACCEPT.
#[test]
fn a_protected_evaluation_accepts_only_cleared_trials() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "unk", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut v = evl_request("unk", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    assert_eq!(rec.arm_for_policy(&w.cand_ref).unwrap().verified_pass, 2);
    let (adm, _) = admit(&w.s, "unk", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Inconclusive, "{:?}", adm.reasons);
    assert!(
        adm.reasons.iter().any(|r| r.contains("safety unknown")),
        "{:?}",
        adm.reasons
    );
}
