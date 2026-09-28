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
        if exec {
            // A GENUINE protected execution: Fabric's execution attestation
            // (PSV-7), not merely the receipt's backend name.
            t["acf_attestation"] = attest_execution(VERIFIER, &t["acf_request"], &t["acf_receipt"]);
        }
        if t["verification_receipt"].is_object() && verif {
            t["verification_receipt"]["backend_profile_ref"] = json!(PROTECTED);
            // A GENUINE protected verdict (M4 + B2): its class and digest refs
            // derived from a real launch manifest and an observation signed by
            // the operator-rooted fixture observer, delivered as its bundle.
            let req = t["verification_request"].clone();
            // Observed under the trial's own authority epoch (the loop joins it).
            let epoch = t["episode"]["authority_epoch"].as_u64().unwrap();
            let bundle = make_protected(
                &req,
                &mut t["verification_receipt"],
                |_| {},
                |o| o.epoch = epoch,
            );
            t["verification_psv_evidence"] = serde_json::from_str(&bundle).unwrap();
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
    assign_specs(&w.s, "prot-dev", &specs);
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
    assign_specs(&w.s, "prot-ok", &specs);
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
    assign_specs(&w.s, "prot", &specs_for(&w));
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
        assign_specs(&w.s, "legs", &specs_for(&w));
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
    assign_specs(&w.s, "unk", &specs_for(&w));
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

/// ADR-001 §5 rollback revalidation (re-audit 4: a rollback to the
/// incumbent-of-record skipped the "baseline issuer still trusted" check that
/// activating it performs). A rollback now re-validates its predecessor NOW:
/// a baseline needs its issuer still trusted; an admission must still
/// re-derive, its admitter still trusted. Otherwise refused, writing nothing.
/// Positive control: the same rollbacks while the authority holds succeed.
///
/// Mutation: drop the revalidation block in `check_rollback` → red.
#[test]
fn a_rollback_revalidates_its_predecessor() {
    const OTHER: &str = "op:admitter-2";
    let untrust = |w: &World| {
        let mut cfg = w.s.config().unwrap();
        cfg.trusted_admitters.retain(|a| a.as_str() != ADMITTER);
        w.s.write_config(&cfg).unwrap();
    };
    let as_other = |mut t: Value| {
        t["issuer_ref"] = json!(OTHER);
        t
    };
    for revoke in [true, false] {
        let w = world();
        let mut cfg = w.s.config().unwrap();
        cfg.trusted_admitters
            .push(axon_loop_contracts::OpaqueRef::new(OTHER).unwrap());
        w.s.write_config(&cfg).unwrap();
        let adm = dev_admitted(&w, "rv", false);
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
        // Baseline predecessor, its issuer (ADMITTER) no longer trusted.
        if revoke {
            untrust(&w);
        }
        let before = snapshot(w.dir.path());
        let r = apply(
            &w,
            as_other(transition(
                "r1",
                "rollback",
                &w.cand_ref,
                Some(&w.inc_ref),
                2,
                Some(&w.baseline),
                false,
            )),
        );
        if revoke {
            let e = r.unwrap_err();
            assert!(
                matches!(e, LoopError::Refused(ref m) if m.contains("baseline issuer is no longer trusted")),
                "{e}"
            );
            assert_eq!(snapshot(w.dir.path()), before);
            continue;
        }
        r.unwrap();
        // Admission predecessor, its admitter (ADMITTER) no longer trusted.
        untrust(&w);
        let before = snapshot(w.dir.path());
        let e = apply(
            &w,
            as_other(transition(
                "r2",
                "rollback",
                &w.inc_ref,
                Some(&w.cand_ref),
                3,
                Some(&adm),
                false,
            )),
        )
        .unwrap_err();
        assert!(
            matches!(e, LoopError::Refused(ref m) if m.contains("admission no longer holds")),
            "{e}"
        );
        assert_eq!(snapshot(w.dir.path()), before);
    }
}

/// Re-audit 4 (mutation reviewer): M17 was "killed" only because the message
/// changed — its fixture was development-class, so the D3 class check still
/// refused. Here the mechanism-test admission rests on a PROTECTED-class
/// evaluation (protected backend, every trial cleared): the fixture ban is
/// the only thing between it and activation, so removing that guard flips
/// the outcome from refused to activated.
#[test]
fn a_protected_class_mechanism_test_is_still_not_served() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "pmech", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let o = EvlOpts {
        role: CorpusRole::MechanismTest,
        ..EvlOpts::default()
    };
    assign_specs(&w.s, "pmech", &specs_for(&w));
    let mut v = evl_request("pmech", &w.inc, &w.cand, &specs_for(&w), &o);
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    assert_eq!(
        rec.evaluation_class,
        axon_loop::plan::EvaluationClass::Protected
    );
    let (adm, adm_ref) = admit(&w.s, "pmech", &e, ADMITTER, true).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
    let before = snapshot(w.dir.path());
    let e = apply(
        &w,
        transition(
            "m1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm_ref),
            true,
        ),
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("serves no mechanism-test")),
        "{e}"
    );
    assert_eq!(snapshot(w.dir.path()), before);
}

/// G32-r22-sidecar-bindings: in a PROTECTED evaluation a trial's preflight
/// context is AUTHENTICATED, not named — the worker writes the context, so a
/// receipt naming the trusted observer proves nothing on its own. Refused
/// (the trial is unknown, never a pass): no signature; a signature under a key
/// the operator did not register for the observer; a signature by another
/// name. Honest control: the observer's own signature counts both passes.
///
/// Mutation: make `authenticated_context` accept everything → red.
#[test]
fn a_protected_context_is_authenticated_not_named() {
    let run = |alter: &dyn Fn(&mut Value)| {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        trust_monitor(&w.s);
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "exp", &specs_for(&w));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        for t in v["trials"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        {
            alter(t);
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        rec.arm_for_policy(&w.cand_ref).unwrap().clone()
    };
    let (other_sk, _) = axon_loop_contracts::attestation::generate().unwrap();
    let sign_as = |sk: &[u8], who: &str, t: &Value| {
        axon_loop_contracts::attestation::sign_document(
            sk,
            axon_loop::evl::CONTEXT_DOMAIN,
            &OpaqueRef::new(who).unwrap(),
            &t["context"],
        )
        .unwrap()
    };
    for (why, alter) in [
        (
            "unsigned",
            Box::new(|t: &mut Value| {
                t.as_object_mut().unwrap().remove("context_signature");
            }) as Box<dyn Fn(&mut Value)>,
        ),
        (
            "unregistered key",
            Box::new(|t: &mut Value| t["context_signature"] = sign_as(&other_sk, OBSERVER, t)),
        ),
        (
            "another name",
            Box::new(|t: &mut Value| {
                t["context_signature"] = sign_as(&observer_key().0, ADMITTER, t)
            }),
        ),
    ] {
        let arm = run(&*alter);
        assert_eq!(arm.verified_pass, 0, "{why}: {:?}", arm.trials);
        assert!(
            arm.trials.iter().all(|t| t.reason.contains("context")),
            "{why}: {:?}",
            arm.trials
        );
    }
    assert_eq!(
        run(&|_| {}).verified_pass,
        2,
        "the observer's own signature must count"
    );
}

/// World with the candidate ACCEPTed on a PROTECTED evaluation (protected
/// backend, every trial cleared, every context observer-signed).
fn protected_accepted(exp: &str) -> (World, Ref) {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, exp, &specs_for(&w));
    let mut v = evl_request(exp, &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, adm) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    (w, adm)
}

fn withdraw(w: &World, f: impl FnOnce(&mut axon_loop::store::Config)) {
    let mut cfg = w.s.config().unwrap();
    f(&mut cfg);
    w.s.write_config(&cfg).unwrap();
}

fn fresh_key() -> String {
    axon_loop_contracts::attestation::generate().unwrap().1
}

/// G11-r22-admission-disposition: "only accepted and CURRENTLY authorized
/// evidence permits activation". Every authority a protected ACCEPT rests on
/// — the clearing monitor, the context observer, the verifier's pins and the
/// task's acceptance check — is re-checked when the admission is activated;
/// withdrawing any one refuses the activation, writing nothing. Positive
/// control: nothing withdrawn, it activates (review wf_d788c05a-be2).
#[test]
fn a_protected_activation_rests_only_on_current_authority() {
    type Withdrawal = fn(&mut axon_loop::store::Config);
    let cases: [(&str, Withdrawal, &str); 7] = [
        (
            "monitor-untrusted",
            |c| c.trusted_monitors.clear(),
            "no longer cleared",
        ),
        (
            "monitor-rekeyed",
            |c| {
                let m = OpaqueRef::new(MONITOR).unwrap();
                c.monitor_keys.insert(m, fresh_key());
            },
            "no longer cleared",
        ),
        (
            "observer-untrusted",
            |c| {
                let o = OpaqueRef::new(OBSERVER).unwrap();
                c.trusted_observers.retain(|x| x != &o);
                c.observer_keys.remove(&o);
            },
            "no longer trusts",
        ),
        (
            "observer-rekeyed",
            |c| {
                c.observer_keys
                    .insert(OpaqueRef::new(OBSERVER).unwrap(), fresh_key());
            },
            "protected context is not authenticated",
        ),
        (
            "profile-withdrawn",
            |c| {
                c.verifier_pins
                    .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
                    .unwrap()
                    .backend_profiles
                    .retain(|p| p != PROTECTED);
            },
            "compute profile",
        ),
        (
            "revision-changed",
            |c| {
                c.verifier_pins
                    .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
                    .unwrap()
                    .executable_digest = format!("acf1:{}", "f".repeat(64));
            },
            "verifier revision",
        ),
        (
            "acceptance-withdrawn",
            |c| c.task_acceptance.clear(),
            "acceptance",
        ),
    ];
    for (name, f, why) in cases {
        let (w, adm) = protected_accepted(name);
        withdraw(&w, f);
        let before = snapshot(w.dir.path());
        let t = transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        );
        match apply(&w, t) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{name}: {m}"),
            o => panic!("{name}: activated on withdrawn authority: {o:?}"),
        }
        assert_eq!(
            snapshot(w.dir.path()),
            before,
            "{name}: a refusal wrote something"
        );
    }
    let (w, adm) = protected_accepted("control");
    let t = transition(
        "a1",
        "activate",
        &w.inc_ref,
        Some(&w.cand_ref),
        1,
        Some(&adm),
        false,
    );
    assert_eq!(apply(&w, t).unwrap(), Some(w.cand_ref.clone()));
}

/// G11-r22-rollback-revalidate: "a rollback rechecks … profile
/// qualification". A predecessor admitted on verdicts from a compute profile
/// the operator has since withdrawn is not reinstated: there is no safe
/// predecessor, and nothing is written.
#[test]
fn a_rollback_rechecks_profile_qualification() {
    let (w, adm) = protected_accepted("rb-profile");
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
    withdraw(&w, |c| {
        c.verifier_pins
            .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
            .unwrap()
            .backend_profiles
            .retain(|p| p != PROTECTED);
    });
    let before = snapshot(w.dir.path());
    match apply(
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
    ) {
        Err(LoopError::Refused(m)) => assert!(
            m.contains("no safe predecessor") && m.contains("compute profile"),
            "{m}"
        ),
        o => panic!("rolled back onto a withdrawn profile: {o:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
}

/// O2 / A18 for the OBSERVER: in a protected evaluation a context signed by a
/// key the STORE registers for the trusted observer — but that the operator's
/// observer root does not hold — authenticates nothing. Installing that same
/// key in the operator root (the operator's act) makes it count: the store
/// narrows, it never adds.
#[test]
fn an_observer_key_planted_in_the_store_is_not_authority() {
    let (planted_sk, planted_pk) = axon_loop_contracts::attestation::generate().unwrap();
    let run = || {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        trust_monitor(&w.s);
        withdraw(&w, |c| {
            c.observer_keys
                .insert(OpaqueRef::new(OBSERVER).unwrap(), planted_pk.clone());
        });
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "exp", &specs_for(&w));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        for t in v["trials"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        {
            t["context_signature"] = axon_loop_contracts::attestation::sign_document(
                &planted_sk,
                axon_loop::evl::CONTEXT_DOMAIN,
                &OpaqueRef::new(OBSERVER).unwrap(),
                &t["context"],
            )
            .unwrap();
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        rec.arm_for_policy(&w.cand_ref).unwrap().clone()
    };
    let arm = run();
    assert_eq!(arm.verified_pass, 0, "{:?}", arm.trials);
    assert!(
        arm.trials
            .iter()
            .all(|t| t.reason.contains("operator's observer root")),
        "{:?}",
        arm.trials
    );
    // The operator installs the key: now it is authority.
    let root = common::operator_root().join("observer");
    std::fs::write(
        root.join(format!("planted-{}.pub", &planted_pk[..8])),
        format!("{planted_pk}\n"),
    )
    .unwrap();
    assert_eq!(run().verified_pass, 2, "an operator-rooted key counts");
}

/// O2: REVOCATION at the operator root. A protected admission whose verdicts
/// and contexts were authenticated under keys the operator has since REMOVED
/// from its root is not activated — even though the store still names both
/// keys (the store never re-grants what the operator revoked). Per-thread test
/// root: this revocation touches no other test.
#[test]
fn a_key_revoked_at_the_operator_root_no_longer_counts() {
    for auth in ["verifier", "observer", "monitor"] {
        let (w, adm) = protected_accepted(&format!("revoke-{auth}"));
        let key = common::operator_root().join(auth).join("fixture.pub");
        let saved = std::fs::read(&key).unwrap();
        std::fs::remove_file(&key).unwrap();
        let before = snapshot(w.dir.path());
        let t = transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        );
        match apply(&w, t) {
            Err(LoopError::Refused(_)) => {}
            o => panic!("{auth}: activated on a revoked key: {o:?}"),
        }
        assert_eq!(
            snapshot(w.dir.path()),
            before,
            "{auth}: a refusal wrote something"
        );
        std::fs::write(&key, saved).unwrap();
    }
}

/// PSV-7 (review wf_1bc28496-38e): a safety-monitor key that exists ONLY in
/// the mutable store never clears a trial in a protected scope — the
/// reviewer's reproduction reached ACCEPT and activation on one. Control: the
/// operator installs the same key at the monitor root, and it counts.
#[test]
fn a_monitor_key_planted_in_the_store_never_clears_a_protected_trial() {
    const PLANTED: &str = "agent:planted-monitor";
    let (sk, pk) = axon_loop_contracts::attestation::generate().unwrap();
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "mon", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "mon", &specs_for(&w));
    let mut v = evl_request("mon", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    assert!(intake_all(&w.s, &v).is_empty(), "the bundle intakes");
    let mut cfg = w.s.config().unwrap();
    let m = OpaqueRef::new(PLANTED).unwrap();
    cfg.trusted_monitors.push(m.clone());
    cfg.monitor_keys.insert(m.clone(), pk.clone());
    w.s.write_config(&cfg).unwrap();
    let t = &v["trials"][0];
    let r = safety_report(t, "clear", None, PLANTED);
    let sig = axon_loop_contracts::attestation::sign_document(
        &sk,
        axon_loop::safety::CLEARANCE_DOMAIN,
        &m,
        &r,
    )
    .unwrap();
    let e = axon_loop::safety::report(&w.s, &r.to_string(), Some(&sig.to_string())).unwrap_err();
    assert!(e.to_string().contains("operator's monitor root"), "{e}");
    // The operator installs the key: now the clearance counts.
    let rooted = common::operator_root().join("monitor").join("planted.pub");
    std::fs::write(&rooted, format!("{pk}\n")).unwrap();
    let ok = axon_loop::safety::report(&w.s, &r.to_string(), Some(&sig.to_string()));
    std::fs::remove_file(&rooted).unwrap();
    ok.unwrap();
}

/// PSV-7 (dev review round wf_336353cb-a2b, executed there to ACCEPT and
/// activation): an execution receipt RELABELLED to the protected backend,
/// with no Fabric execution attestation, counts nothing; nor does one
/// attested under a key the operator's verifier root does not hold. Control:
/// the genuinely attested execution counts.
#[test]
fn a_relabelled_execution_leg_counts_nothing_in_a_protected_evaluation() {
    // "unrooted": a verifier the STORE trusts and keys, but whose key the
    // operator's verifier root does not hold, attests the execution.
    const PLANTED: &str = "agent:planted-verifier";
    let (planted, planted_pk) = axon_loop_contracts::attestation::generate().unwrap();
    for case in ["genuine", "unattested", "unrooted"] {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        trust_monitor(&w.s);
        if case == "unrooted" {
            let mut cfg = w.s.config().unwrap();
            let p = OpaqueRef::new(PLANTED).unwrap();
            cfg.trusted_verifiers.push(p.clone());
            cfg.verifier_keys.insert(p, planted_pk.clone());
            w.s.write_config(&cfg).unwrap();
        }
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "exp", &specs_for(&w));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        for t in v["trials"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        {
            match case {
                "unattested" => {
                    t.as_object_mut().unwrap().remove("acf_attestation");
                }
                "unrooted" => {
                    t["acf_attestation"] = attest_execution_with(
                        &planted,
                        PLANTED,
                        &t["acf_request"],
                        &t["acf_receipt"],
                    )
                }
                _ => {}
            }
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap().clone();
        if case == "genuine" {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else {
            assert_eq!(arm.verified_pass, 0, "{case}: {:?}", arm.trials);
            assert!(
                arm.trials
                    .iter()
                    .all(|t| t.reason.contains("not attested as a protected execution")),
                "{case}: {:?}",
                arm.trials
            );
        }
    }
}

/// PSV-7: a protected ADMISSION re-verifies the execution leg from its own
/// stored documents. A store writer who repoints a counted trial's execution
/// attestation at junk, or drops it, gets no ACCEPT.
#[test]
fn a_protected_admission_re_verifies_the_execution_leg_from_its_documents() {
    for case in ["junk-attestation", "no-attestation-ref"] {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        freeze_plan(&w.s, "rx", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "rx", &specs_for(&w));
        let mut v = evl_request("rx", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        clear_all(&w.s, &v);
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let junk =
            w.s.put_cas("acf-attestations", &json!({"schema": "not-an-attestation"}))
                .unwrap();
        let mut j = serde_json::to_value(&rec).unwrap();
        for arm in j["arms"].as_array_mut().unwrap() {
            for t in arm["trials"].as_array_mut().unwrap() {
                if t["verification"].is_object() {
                    if case == "junk-attestation" {
                        t["verification"]["execution_attestation_ref"] = json!(junk);
                    } else {
                        t["verification"]
                            .as_object_mut()
                            .unwrap()
                            .remove("execution_attestation_ref");
                    }
                }
            }
        }
        let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(j).unwrap();
        let fe = w.s.put_cas("evaluations", &forged).unwrap();
        forged_append(
            w.s.root(),
            axon_loop::ledger::Event::Evaluation {
                scope: scope(),
                experiment_id: "rx".into(),
                evaluation_ref: fe.clone(),
                freeze_seq: forged.freeze_seq,
                authority_epoch: forged.authority_epoch,
            },
        );
        match admit(&w.s, "rx", &fe, ADMITTER, false) {
            Err(e) => assert!(e.to_string().contains("execution"), "{case}: {e}"),
            Ok((adm, _)) => assert_ne!(adm.decision, Decision::Accept, "{case}: {:?}", adm.reasons),
        }
    }
}

/// A store writer forges a GENUINE protected evaluation record: `forge` edits
/// its JSON, then the record goes in the CAS with a forged ledger append.
fn forge_eval(
    w: &World,
    exp: &str,
    rec: &axon_loop::evl::EvaluationRecord,
    forge: impl FnOnce(&mut Value),
) -> Ref {
    let mut j = serde_json::to_value(rec).unwrap();
    forge(&mut j);
    let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(j).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    forged_append(
        w.s.root(),
        axon_loop::ledger::Event::Evaluation {
            scope: scope(),
            experiment_id: exp.into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
    );
    fe
}

fn refused_or_not_accepted(w: &World, exp: &str, fe: &Ref, why: &str) {
    match admit(&w.s, exp, fe, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains(why), "{e}"),
        Ok((adm, _)) => panic!("{why}: admitted {:?} {:?}", adm.decision, adm.reasons),
    }
}

/// PSV-7 (dev review round wf_7cb5856d-806, executed there to ACCEPT): a
/// protected decision counts only the trials it re-verifies. Here the
/// candidate's trials are marked Unknown with no verification (nothing to
/// re-verify) while the stored counters still say 2 passes.
#[test]
fn a_protected_decision_counts_only_its_re_verified_trials() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "gc", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "gc", &specs_for(&w));
    let mut v = evl_request("gc", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let cand = w.cand_ref.to_string();
    let fe = forge_eval(&w, "gc", &rec, |j| {
        for arm in j["arms"].as_array_mut().unwrap() {
            if arm["policy_ref"] == cand {
                for t in arm["trials"].as_array_mut().unwrap() {
                    t["outcome"] = json!("unknown");
                    t.as_object_mut().unwrap().remove("verification");
                }
            }
        }
    });
    refused_or_not_accepted(&w, "gc", &fe, "counts are not its trials");
}

/// PSV-7: a counted trial re-verifies against ITS OWN episode: borrowing
/// another trial's genuine episode and verdict counts one pass twice.
#[test]
fn a_counted_trial_cannot_borrow_another_trials_verdict() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "bw", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "bw", &specs_for(&w));
    let mut v = evl_request("bw", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let cand = w.cand_ref.to_string();
    let fe = forge_eval(&w, "bw", &rec, |j| {
        for arm in j["arms"].as_array_mut().unwrap() {
            if arm["policy_ref"] == cand {
                let ts = arm["trials"].as_array_mut().unwrap();
                let (ep, ver) = (ts[0]["episode_ref"].clone(), ts[0]["verification"].clone());
                ts[1]["episode_ref"] = ep;
                ts[1]["verification"] = ver;
            }
        }
    });
    refused_or_not_accepted(&w, "bw", &fe, "another trial's");
}

/// PSV-7: the evaluation's class is the frozen plan's. A DEVELOPMENT plan's
/// evaluation (here even with genuinely protected documents) relabelled
/// protected after the scope was protected is refused.
#[test]
fn an_evaluation_class_other_than_the_frozen_plans_is_refused() {
    let w = world();
    pin_protected_backend(&w.s);
    trust_monitor(&w.s);
    freeze_plan(&w.s, "cb", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "cb", &specs_for(&w));
    let mut v = evl_request("cb", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    assert_eq!(
        rec.evaluation_class,
        axon_loop::plan::EvaluationClass::Development
    );
    protect(&w.s);
    let fe = forge_eval(&w, "cb", &rec, |j| {
        j["evaluation_class"] = json!("protected")
    });
    refused_or_not_accepted(&w, "cb", &fe, "class is not the frozen plan's");
}

/// M4 / A13 / A14: in a protected evaluation a verdict counts ONLY as
/// protected evidence. The same guest-path verdict WITHOUT a verified
/// observation (`guest-unobserved`), or a `development` one — each genuinely
/// signed on a protected backend — counts nothing, as Unverifiable.
#[test]
fn only_protected_class_evidence_counts_in_a_protected_evaluation() {
    for class in ["guest-unobserved", "development"] {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        trust_monitor(&w.s);
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "exp", &specs_for(&w));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        for t in v["trials"].as_array_mut().unwrap() {
            if !t["verification_receipt"].is_object() {
                continue;
            }
            for e in t["verification_receipt"]["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .iter_mut()
            {
                if e == "evidence-class:protected" {
                    *e = json!(format!("evidence-class:{class}"));
                }
            }
            // Such a receipt carries no PSV bundle (it claims nothing protected).
            t.as_object_mut()
                .unwrap()
                .remove("verification_psv_evidence");
            t["episode"]["verification"]["verifier_ref"] =
                json!(digest_value(&t["verification_receipt"]).unwrap());
            t["verification_attestation"] = attest(
                VERIFIER,
                &t["verification_request"],
                &t["verification_receipt"],
            );
        }
        clear_all(&w.s, &v);
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
        assert_eq!(arm.verified_pass, 0, "{class}: {:?}", arm.trials);
        assert!(
            arm.trials
                .iter()
                .all(|t| t.reason.contains(&format!("evidence class is {class}"))),
            "{class}: {:?}",
            arm.trials
        );
    }
}
