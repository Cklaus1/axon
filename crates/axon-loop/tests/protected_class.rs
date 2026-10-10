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
            // A GENUINE protected execution: an OBSERVED launch on the
            // protected profile, attested by Fabric (PSV-7), not merely the
            // receipt's backend name.
            observed_protected_execution(t);
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
                |m| m.authority.epoch = epoch,
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
        assert_eq!(
            arm.verified_pass, 0,
            "ATTACK: a development-backend verdict counted in a protected evaluation: {arm:?}"
        );
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
            let e = r.err().unwrap_or_else(|| {
                panic!("ATTACK: a rollback in a protected scope took a development-class admission")
            });
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
                // The execution leg on a development backend is refused by
                // EVL's D3 filter AND by `verify_execution` (no attestation
                // here; the backend join when attested: see
                // an_attested_development_execution_leg_counts_nothing), each
                // alone, so either reason is correct for it. The verification
                // leg's reason stays exact.
                assert!(
                    c.trials.iter().all(|t| t
                        .reason
                        .contains(&format!("development {leg} backend"))
                        || (leg == "execution"
                            && t.reason.contains("not attested as a protected execution"))),
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
    .err()
    .unwrap_or_else(|| panic!("ATTACK: a protected scope served a mechanism-test fixture"));
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
            // C9 round 2 (PSV-5): the verdict's preflight observation was
            // signed by this observer too, and re-verifying the bundle now
            // finds no trusted observer holding its key, first.
            "no observer the store trusts",
        ),
        (
            "observer-rekeyed",
            |c| {
                c.observer_keys
                    .insert(OpaqueRef::new(OBSERVER).unwrap(), fresh_key());
            },
            "no observer the store trusts",
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
            // The verdicts' preflight OBSERVATIONS stay signed by the fixture
            // observer key, which the store now registers for a second trusted
            // identity (C9 round 2: an observation counts only under a key the
            // store registers for a trusted observer). This test is about the
            // CONTEXT's key.
            let signer = OpaqueRef::new("fixture:observation-signer").unwrap();
            c.trusted_observers.push(signer.clone());
            c.observer_keys.insert(signer, observer_key().1.clone());
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
            o => panic!("ATTACK: {auth}: activated on a revoked key: {o:?}"),
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
            cfg.verifier_keys.insert(p.clone(), planted_pk.clone());
            // Qualified for the protected profile (amendment 61, A92), so
            // only the operator root's refusal of its key stands.
            let pin = cfg.verifier_pins[&OpaqueRef::new(VERIFIER).unwrap()].clone();
            cfg.verifier_pins.insert(p, pin);
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
    // The WHOLE borrow (C9 round 1): c1 cites every document of c0's trial,
    // context signature and execution leg included, so each re-verifies from
    // its own stored documents. Only the episode-identity join is left to see
    // that the documents are another trial's. It runs first, so a mutant is
    // scored on it. Borrowing the episode and verdict alone (below) is also
    // refused by the context-signature check, which could not show this guard
    // is load-bearing (C9 dev review, M263).
    let fe = forge_eval(&w, "bw", &rec, |j| {
        for arm in j["arms"].as_array_mut().unwrap() {
            if arm["policy_ref"] == cand {
                let ts = arm["trials"].as_array_mut().unwrap();
                let src = ts[0].as_object().unwrap().clone();
                let dst = ts[1].as_object_mut().unwrap();
                for (k, val) in src {
                    if k != "trial_id" && k != "task_id" && k != "safety" {
                        dst.insert(k, val);
                    }
                }
            }
        }
    });
    match admit(&w.s, "bw", &fe, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains("another trial's"), "{e}"),
        Ok((adm, _)) => panic!(
            "ATTACK: a trial counted another trial's whole verified evidence: {:?} {:?}",
            adm.decision, adm.reasons
        ),
    }
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
    // Borrowing only the episode and verdict: the identity join refuses it,
    // and so does the context-signature check (c1's signature vouches for
    // c1's context), each alone. Any refusal is correct here.
    match admit(&w.s, "bw", &fe, ADMITTER, false) {
        Err(e) => assert!(
            e.to_string().contains("another trial's")
                || e.to_string().contains("context signature"),
            "{e}"
        ),
        Ok((adm, _)) => panic!(
            "ATTACK: a trial counted another trial's verdict: {:?}",
            adm.decision
        ),
    }
}

/// PSV-7 (dev review round wf_7cb5856d-806, executed there to ACCEPT):
/// clearances forged into the ledger WITHOUT a monitor signature (the
/// monitor's public key id copied in) clear nothing in a protected decision.
/// Control: `protected_accepted`, whose clearances are genuinely signed.
#[test]
fn a_forged_unsigned_clearance_clears_nothing() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    trust_monitor(&w.s);
    freeze_plan(&w.s, "fc", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "fc", &specs_for(&w));
    let mut v = evl_request("fc", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    assert!(intake_all(&w.s, &v).is_empty(), "the bundle intakes");
    let kid = axon_loop_contracts::attestation::key_id_of_hex(&monitor_key().1);
    for t in v["trials"].as_array().unwrap() {
        let report: axon_loop::safety::SafetyReport =
            serde_json::from_value(safety_report(t, "clear", None, MONITOR)).unwrap();
        forged_append(
            w.s.root(),
            axon_loop::ledger::Event::SafetyReport {
                scope: scope(),
                report: Box::new(report),
                key_id: kid.clone(),
                signature_ref: None,
            },
        );
    }
    let (_, e) = evaluate(&w.s, &v).unwrap();
    match admit(&w.s, "fc", &e, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains("no longer cleared"), "{e}"),
        Ok((adm, _)) => assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons),
    }
    // Control: protected_accepted's genuinely signed clearances ACCEPT.
    let _ = protected_accepted("fc-ok");
}

/// Amendment 115 (eqgate9): clearances forged into the ledger that NAME a stored monitor signature which is
/// not in the store clear nothing. `clearance_verifies` answers false for a signature it cannot read; flipped to
/// true the clearance counted without `verify_document` ever running, and the whole axon-loop and axon-fabric
/// suites stayed green (the other forged-clearance test names NO signature at all).
/// Control: `protected_accepted`, whose clearances are genuinely signed.
#[test]
fn a_clearance_whose_stored_signature_is_missing_clears_nothing() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    trust_monitor(&w.s);
    freeze_plan(&w.s, "fm", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "fm", &specs_for(&w));
    let mut v = evl_request("fm", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    assert!(intake_all(&w.s, &v).is_empty(), "the bundle intakes");
    let kid = axon_loop_contracts::attestation::key_id_of_hex(&monitor_key().1);
    for t in v["trials"].as_array().unwrap() {
        let report: axon_loop::safety::SafetyReport =
            serde_json::from_value(safety_report(t, "clear", None, MONITOR)).unwrap();
        forged_append(
            w.s.root(),
            axon_loop::ledger::Event::SafetyReport {
                scope: scope(),
                report: Box::new(report),
                key_id: kid.clone(),
                signature_ref: Some(r('9')),
            },
        );
    }
    let (_, e) = evaluate(&w.s, &v).unwrap();
    match admit(&w.s, "fm", &e, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains("no longer cleared"), "{e}"),
        Ok((adm, _)) => assert_ne!(
            adm.decision,
            Decision::Accept,
            "ATTACK: a clearance naming a monitor signature that is not stored cleared the trial: {:?}",
            adm.reasons
        ),
    }
    // Control: protected_accepted's genuinely signed clearances ACCEPT.
    let _ = protected_accepted("fm-ok");
}

/// PSV-7: the evaluation's class is the frozen plan's. A DEVELOPMENT plan's
/// evaluation (here even with genuinely protected documents) relabelled
/// protected after the scope was protected is refused.
#[test]
fn an_evaluation_class_other_than_the_frozen_plans_is_refused() {
    // The DOWNGRADE route first (C9 round 1): a PROTECTED plan's genuine
    // protected evaluation relabelled development. Nothing downstream
    // re-verifies a development-class record (reverify_protected, the grounding
    // and clearance checks are all keyed on the evaluation's OWN class), so the
    // class join is the only guard on this route. The relabel-to-protected
    // route below is also refused by reverify_protected, so it cannot show that
    // this guard is load-bearing (C9 dev review, M262).
    {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        trust_monitor(&w.s);
        freeze_plan(&w.s, "cd", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "cd", &specs_for(&w));
        let mut v = evl_request("cd", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        clear_all(&w.s, &v);
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        assert_eq!(
            rec.evaluation_class,
            axon_loop::plan::EvaluationClass::Protected
        );
        let fe = forge_eval(&w, "cd", &rec, |j| {
            j["evaluation_class"] = json!("development")
        });
        match admit(&w.s, "cd", &fe, ADMITTER, false) {
            Err(e) => assert!(
                e.to_string().contains("class is not the frozen plan's"),
                "{e}"
            ),
            Ok((adm, _)) => panic!(
                "ATTACK: a protected plan admitted an evaluation relabelled development: {:?} {:?}",
                adm.decision, adm.reasons
            ),
        }
    }
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

/// Round-3 joins: a counted protected trial's stored record must agree with
/// its re-verified documents (arm policy, signed outcome, signed context).
#[test]
fn a_protected_record_must_agree_with_its_re_verified_documents() {
    let cases: [(&str, &str); 3] = [
        ("policy", "ran policy"),
        ("outcome", "is not the signed verdict"),
        ("context", "context signature"),
    ];
    for (case, why) in cases {
        let exp = format!("r3-{case}");
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        freeze_plan(&w.s, &exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, &exp, &specs_for(&w));
        let mut v = evl_request(&exp, &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        clear_all(&w.s, &v);
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let cand = w.cand_ref.to_string();
        let junk =
            w.s.put_cas("context-signatures", &json!({"schema": "junk"}))
                .unwrap();
        let fe = forge_eval(&w, &exp, &rec, |j| match case {
            "policy" => {
                let arms = j["arms"].as_array_mut().unwrap();
                let (a, b) = (arms[0]["policy_ref"].clone(), arms[1]["policy_ref"].clone());
                arms[0]["policy_ref"] = b;
                arms[1]["policy_ref"] = a;
            }
            "outcome" => {
                for arm in j["arms"].as_array_mut().unwrap() {
                    if arm["policy_ref"] == cand {
                        arm["trials"][0]["outcome"] = json!("fail");
                        let vp = arm["verified_pass"].as_u64().unwrap();
                        let f = arm["fail"].as_u64().unwrap();
                        arm["verified_pass"] = json!(vp - 1);
                        arm["fail"] = json!(f + 1);
                    }
                }
            }
            _ => {
                for arm in j["arms"].as_array_mut().unwrap() {
                    for t in arm["trials"].as_array_mut().unwrap() {
                        if t["verification"].is_object() {
                            t["context_signature_ref"] = json!(junk);
                        }
                    }
                }
            }
        });
        refused_or_not_accepted(&w, &exp, &fe, why);
    }
}

/// PSV-4 / PSV-7, Candidate-8 certifying review wf_ae3a5a74-41e. A store writer
/// takes a GENUINE protected evaluation and repoints every counted trial's
/// verification at a verdict Fabric genuinely signed, but whose class is not
/// protected: guest-unobserved (a protected-profile launch with no
/// observation) or development (the local interpreter). EVL never sees the
/// substitution, so the only admission-side stop is the check that each counted
/// receipt CLAIMS protected evidence (mutation M255). That guard had been
/// retired as "equivalent", and the whole axon-loop suite stayed green without
/// it because no test forged this route. With it removed, both variants
/// reached ACCEPT.
fn a_genuinely_signed_verdict_of_class(class: &str) {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "sub", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    assign_specs(&w.s, "sub", &specs);
    let mut v = evl_request("sub", &w.inc, &w.cand, &specs, &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, genuine) = evaluate(&w.s, &v).unwrap();
    assert_eq!(
        rec.evaluation_class,
        axon_loop::plan::EvaluationClass::Protected
    );

    // Each delivered trial: the SAME verification request, with a receipt of
    // `class` that Fabric genuinely signs, stored where intake would put it.
    let mut subst: std::collections::BTreeMap<String, (Ref, Ref, Ref)> = Default::default();
    for t in v["trials"].as_array().unwrap() {
        if !t["verification_receipt"].is_object() {
            continue;
        }
        let mut rc = t["verification_receipt"].clone();
        let refs: Vec<Value> = rc["evidence_refs"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                let s = e.as_str().unwrap();
                !(s.starts_with("evidence-class:")
                    || s.starts_with("launch-manifest-sha256:")
                    || s.starts_with("preflight-observation-sha256:"))
            })
            .cloned()
            .chain([json!(format!("evidence-class:{class}"))])
            .collect();
        rc["evidence_refs"] = json!(refs);
        if class == "development" {
            rc["backend_profile_ref"] = json!(CHECK_PROFILE);
        }
        let att = attest(VERIFIER, &t["verification_request"], &rc);
        let mut ep = t["episode"].clone();
        ep["verification"]["verifier_ref"] = json!(digest_value(&rc).unwrap());
        let rc_ref = w.s.put_cas("fabric-receipts", &rc).unwrap();
        let att_ref = w.s.put_cas("fabric-attestations", &att).unwrap();
        let ep_ref = w.s.put_cas("episodes", &ep).unwrap();
        let _ =
            w.s.put_cas("fabric-requests", &t["verification_request"])
                .unwrap();
        let tid = t["episode"]["identity"]["trial_id"]
            .as_str()
            .unwrap()
            .to_string();
        subst.insert(tid, (ep_ref, rc_ref, att_ref));
    }
    assert!(!subst.is_empty(), "setup: no delivered trial to substitute");

    let mut j = serde_json::to_value(&rec).unwrap();
    let mut n = 0;
    for arm in j["arms"].as_array_mut().unwrap() {
        for t in arm["trials"].as_array_mut().unwrap() {
            let tid = t["trial_id"].as_str().unwrap().to_string();
            if let (Some((ep, rc, att)), true) = (subst.get(&tid), t["verification"].is_object()) {
                t["episode_ref"] = json!(ep);
                t["verification"]["receipt_ref"] = json!(rc);
                t["verification"]["attestation_ref"] = json!(att);
                let ve = t["verification"].as_object_mut().unwrap();
                ve.remove("psv_evidence_ref");
                // …and the record states what an unobserved verdict honestly
                // re-verifies as: no observation signer. Leaving the genuine
                // record's signer in place let the observation-signer join
                // refuse the forgery first (C9 round 2), so the claims-protected
                // check (M255) was no longer the guard this attacked.
                ve.remove("observation_signed_by");
                n += 1;
            }
        }
    }
    assert!(n > 0, "setup: no counted trial substituted");
    let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(j).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    forged_append(
        w.s.root(),
        axon_loop::ledger::Event::Evaluation {
            scope: scope(),
            experiment_id: "sub".into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
    );
    match admit(&w.s, "sub", &fe, ADMITTER, false) {
        Err(e) => assert!(
            e.to_string().contains("does not claim protected evidence"),
            "{class}: refused, but not by the claims-protected check: {e}"
        ),
        Ok((adm, _)) => panic!(
            "{class}: a genuinely signed {class} verdict was counted in a PROTECTED decision: {:?} {:?}",
            adm.decision, adm.reasons
        ),
    }
    // Control: the genuine protected evaluation it was forged from is admitted.
    let (ok, _) = admit(&w.s, "sub", &genuine, ADMITTER, false).unwrap();
    assert_eq!(ok.decision, Decision::Accept, "control: {:?}", ok.reasons);
}

#[test]
fn a_genuinely_signed_unobserved_verdict_cannot_count_in_a_protected_record() {
    a_genuinely_signed_verdict_of_class("guest-unobserved")
}

#[test]
fn a_genuinely_signed_development_verdict_cannot_count_in_a_protected_record() {
    a_genuinely_signed_verdict_of_class("development")
}

/// The candidate arm's delivered trials.
fn challenger_trials(v: &mut Value) -> impl Iterator<Item = &mut Value> {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
}

/// Drop every evidence ref of the receipt that starts with `prefix`.
fn drop_ref(rc: &mut Value, prefix: &str) {
    rc["evidence_refs"]
        .as_array_mut()
        .unwrap()
        .retain(|e| !e.as_str().unwrap().starts_with(prefix));
}

/// C9 round 1b (consumer-side join, class b; M425-M427): an execution leg
/// Fabric ATTESTED, on the protected backend, whose receipt does not itself
/// state an OBSERVED protected launch counts nothing. Fabric no longer
/// attests such a receipt (A54), but an attestation it issued before that fix
/// still verifies under the same operator-rooted key; the consumer does not
/// rest on the producer's discipline. Each attack is the genuine leg with ONE
/// observed-launch ref missing or wrong, genuinely attested over the edited
/// bytes, so the class join is the only thing that can refuse it. Control:
/// the genuine observed leg counts.
#[test]
fn an_unobserved_execution_leg_counts_nothing_in_a_protected_evaluation() {
    type Edit = fn(&mut Value);
    let cases: [(&str, Edit); 7] = [
        ("genuine", |_| {}),
        ("no evidence class", |rc| drop_ref(rc, "evidence-class:")),
        ("guest-unobserved class", |rc| {
            drop_ref(rc, "evidence-class:");
            rc["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(json!("evidence-class:guest-unobserved"));
        }),
        ("no launch manifest", |rc| {
            drop_ref(rc, "launch-manifest-sha256:")
        }),
        ("no preflight observation", |rc| {
            drop_ref(rc, "preflight-observation-sha256:")
        }),
        // Amendment 61: each arm of the one-sha256 rule (names_one_sha256).
        ("a launch manifest that is not a sha256", |rc| {
            drop_ref(rc, "launch-manifest-sha256:");
            rc["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(json!("launch-manifest-sha256:not-a-digest"));
        }),
        ("a second launch manifest", |rc| {
            rc["evidence_refs"]
                .as_array_mut()
                .unwrap()
                .push(json!(format!("launch-manifest-sha256:{}", "c".repeat(64))));
        }),
    ];
    for (case, edit) in cases {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        freeze_plan(&w.s, "obs", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "obs", &specs_for(&w));
        let mut v = evl_request("obs", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        for t in challenger_trials(&mut v) {
            observed_protected_execution_with(t, edit);
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
        if case == "genuine" {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
            continue;
        }
        if arm.verified_pass != 0 {
            panic!(
                "ATTACK: {case}: an unobserved execution leg was counted as protected: {:?}",
                arm.trials
            );
        }
        assert!(
            arm.trials
                .iter()
                .all(|t| t.reason.contains("not attested as a protected execution")),
            "{case}: {:?}",
            arm.trials
        );
    }
}

/// ADR-001 D3 (M19; with M428 its four-cell pair): an execution leg on a
/// DEVELOPMENT backend counts nothing in a protected evaluation even when its
/// receipt carries every observed-launch ref and Fabric's attestation over it
/// verifies. Two independent layers refuse it on this route: EVL's D3 leg
/// filter and `verify_execution`'s backend join, which runs unconditionally
/// right after it on the same receipt. Which of them refuses is not the
/// property, so any Unverifiable refusal is accepted; only a COUNTED trial is
/// the attack.
#[test]
fn an_attested_development_execution_leg_counts_nothing() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "devx", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "devx", &specs_for(&w));
    let mut v = evl_request("devx", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    for t in challenger_trials(&mut v) {
        observed_protected_execution_with(t, |rc| {
            rc["backend_profile_ref"] = json!(CHECK_PROFILE);
        });
    }
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
    if arm.verified_pass != 0 {
        panic!(
            "ATTACK: an attested execution on a development backend was counted as protected: {:?}",
            arm.trials
        );
    }
    assert!(
        arm.trials.iter().all(|t| t.outcome == Outcome::Unknown
            && t.unknown_kind == Some(axon_loop::evl::UnknownKind::Unverifiable)),
        "{:?}",
        arm.trials
    );
}

/// C9 round 1b (M428): a protected ADMISSION re-checks the execution leg's
/// BACKEND from its own stored documents, as EVL does. A store writer points
/// each counted trial at an episode whose execution receipt names a
/// development backend (every observed-launch ref present, Fabric's
/// attestation over it genuine), with its attestation stored beside it. Every
/// other document re-verifies; admission's re-derivation never runs EVL's D3
/// leg filter, so the join in `verify_execution` is the only refusal.
/// Control: the unforged record ACCEPTs.
#[test]
fn a_protected_admission_re_checks_the_execution_legs_backend() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "xb", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "xb", &specs_for(&w));
    let mut v = evl_request("xb", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let (adm, _) = admit(&w.s, "xb", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
    // The store writer's documents: per counted episode, a twin whose
    // execution receipt is on the development backend, and its attestation.
    let mut twins = std::collections::BTreeMap::new();
    for t in rec.arms.iter().flat_map(|a| &a.trials) {
        if t.verification.is_none() {
            continue;
        }
        let ep_ref = t.episode_ref.clone().unwrap();
        let mut ep: Value =
            serde_json::from_str(&w.s.get_cas_text("episodes", &ep_ref).unwrap()).unwrap();
        let areq: Value = serde_json::from_str(
            &w.s.get_cas_text(
                "acf-requests",
                &Ref::new(ep["acf_request_ref"].as_str().unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        let mut arc: Value = serde_json::from_str(
            &w.s.get_cas_text(
                "acf-receipts",
                &Ref::new(ep["acf_receipt_ref"].as_str().unwrap()).unwrap(),
            )
            .unwrap(),
        )
        .unwrap();
        arc["backend_profile_ref"] = json!(CHECK_PROFILE);
        let att = attest_execution(VERIFIER, &areq, &arc);
        ep["acf_receipt_ref"] = json!(w.s.put_cas("acf-receipts", &arc).unwrap());
        let ep: LoopEpisode = serde_json::from_value(ep).unwrap();
        let twin = w.s.put_cas("episodes", &ep).unwrap();
        let att_ref = w.s.put_cas("acf-attestations", &att).unwrap();
        twins.insert(ep_ref.to_string(), (twin, att_ref));
    }
    assert!(!twins.is_empty());
    let fe = forge_eval(&w, "xb", &rec, |j| {
        for arm in j["arms"].as_array_mut().unwrap() {
            for t in arm["trials"].as_array_mut().unwrap() {
                if let Some((twin, att)) = t["episode_ref"].as_str().and_then(|r| twins.get(r)) {
                    t["episode_ref"] = json!(twin);
                    t["verification"]["execution_attestation_ref"] = json!(att);
                }
            }
        }
    });
    match admit(&w.s, "xb", &fe, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains("not a protected profile"), "{e}"),
        Ok((adm, _)) => panic!(
            "ATTACK: a development-backend execution leg was admitted as protected: {:?} {:?}",
            adm.decision, adm.reasons
        ),
    }
}

/// ADR-001 D3 (M20): a protected evaluation takes NOTHING from a
/// development-backend verification, not even the kind of an unknown. A
/// CITED unknown (the check timed out, as the verifier signed) whose
/// verification ran on the development backend is Unverifiable, never the
/// receipt's TimedOut: `UnknownKind::Unverifiable` is the kind the loop
/// reports for "a backend ineligible for this evaluation class". A verdict
/// (passed/failed) from that backend is also refused by the protected-evidence
/// check (M213), but that check never runs for a cited unknown, so here the
/// leg filter is the only refusal.
#[test]
fn a_cited_unknown_from_a_development_verification_is_unverifiable() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "cu", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "cu", &specs_for(&w));
    let mut v = evl_request("cu", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    // Protected, observed execution legs; development verification legs.
    on_backends(&mut v, true, false);
    let t = challenger_trials(&mut v).next().unwrap();
    let trial_id = t["episode"]["identity"]["trial_id"]
        .as_str()
        .unwrap()
        .to_string();
    let rc = &mut t["verification_receipt"];
    rc["status"] = json!("timed_out");
    rc["verification"] = json!("unknown");
    rc["process_exit_code"] = Value::Null;
    rc["matched_checks"] = json!(0);
    t["episode"]["verification"]["result"] = json!("unknown");
    t["episode"]["verification"]["matched_checks"] = json!(0);
    t["episode"]["verification"]["verifier_ref"] =
        json!(digest_value(&t["verification_receipt"]).unwrap());
    t["verification_attestation"] = attest(
        VERIFIER,
        &t["verification_request"],
        &t["verification_receipt"],
    );
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
    let r = arm
        .trials
        .iter()
        .find(|x| x.trial_id.as_str() == trial_id)
        .unwrap();
    assert_eq!(r.outcome, Outcome::Unknown, "{r:?}");
    if r.unknown_kind != Some(axon_loop::evl::UnknownKind::Unverifiable) {
        panic!(
            "ATTACK: a development-backend verification decided a protected trial's unknown \
             kind: {r:?}"
        );
    }
    assert!(
        r.reason.contains("development verification backend"),
        "{r:?}"
    );
}

/// C9 round 4 fix wave, ROWS2 wave 2 (M829): the counters a protected
/// decision reads ARE its trials' outcomes. A store writer marks the
/// candidate's trials Unknown (nothing left to re-verify) while the stored
/// counters still say two passes; the trials are still the plan's population,
/// so only the counter check refuses it.
#[test]
fn a_protected_arm_whose_counters_are_not_its_trials_is_refused() {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    freeze_plan(&w.s, "gc", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "gc", &specs_for(&w));
    let mut v = evl_request("gc", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
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
    match admit(&w.s, "gc", &fe, ADMITTER, false) {
        Err(LoopError::Refused(m)) => assert!(m.contains("counts are not its trials"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: a protected decision counted stored counters its trials do not bear: {:?} \
             {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("refused otherwise: {e}"),
    }
    let (adm, _) = admit(&w.s, "gc", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
}

/// C9 round 4 fix wave, ROWS2 wave 2 (M852; four-cell record against M122
/// and M104): a protected decision re-verifies each counted context under an
/// observer the operator TRUSTS now. The contexts here are observed and
/// signed by a SECOND observer (its own key, rooted in the operator's observer
/// root), while the verdicts' preflight observations are the fixture
/// observer's; after the ACCEPT the operator withdraws the second observer
/// (its key stays registered and rooted). Activation re-derives the admission:
/// the context no longer counts. Three checks refuse it, each alone: the
/// re-verification's trust check (M852), the any-class context-observer check
/// (M122) and the protected attribution check (M104). Control: nothing
/// withdrawn, it activates.
#[test]
fn a_context_observer_the_operator_withdrew_counts_nothing_at_activation() {
    const OBSERVER2: &str = "fixture:observer-two";
    let key2 = axon_loop_contracts::attestation::generate().unwrap();
    let root = operator_root();
    std::fs::write(root.join("observer/second.pub"), format!("{}\n", key2.1)).unwrap();
    let setup = |exp: &str| -> (World, Ref) {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        withdraw(&w, |c| {
            let o = OpaqueRef::new(OBSERVER2).unwrap();
            c.trusted_observers.push(o.clone());
            c.observer_keys.insert(o, key2.1.clone());
        });
        freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, exp, &specs_for(&w));
        let mut v = evl_request(exp, &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        for t in v["trials"].as_array_mut().unwrap() {
            t["context"]["observed_issuer_ref"] = json!(OBSERVER2);
            t["context_signature"] = axon_loop_contracts::attestation::sign_document(
                &key2.0,
                axon_loop::evl::CONTEXT_DOMAIN,
                &OpaqueRef::new(OBSERVER2).unwrap(),
                &t["context"],
            )
            .unwrap();
            t["episode"]["context_ref"] = json!(digest_value(&t["context"]).unwrap());
        }
        on_protected_backend(&mut v);
        clear_all(&w.s, &v);
        let (_, e) = evaluate(&w.s, &v).unwrap();
        let (rec, adm) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
        assert_eq!(
            rec.decision,
            Decision::Accept,
            "setup: the second observer's contexts count: {:?}",
            rec.reasons
        );
        (w, adm)
    };
    let act = |w: &World, adm: &Ref| {
        apply(
            w,
            transition(
                "a1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(adm),
                false,
            ),
        )
    };
    let (w, adm) = setup("withdrawn");
    withdraw(&w, |c| {
        c.trusted_observers.retain(|o| o.as_str() != OBSERVER2)
    });
    match act(&w, &adm) {
        Err(LoopError::Refused(m)) => assert!(m.contains("no longer trusts"), "{m}"),
        o => panic!(
            "ATTACK: a protected activation counted contexts observed by an observer the \
             operator withdrew: {o:?}"
        ),
    }
    let (w, adm) = setup("control");
    assert_eq!(act(&w, &adm).unwrap(), Some(w.cand_ref.clone()), "control");
}

// ── amendment 61 (C9 round 4b, rows4a): the execution attestation's signer ──
//
// `evl::verify_execution` is the one primitive both doors use (EVL's protected
// execution leg and admission's `reverify_protected`). Its signer must be a
// verifier the operator TRUSTS (round 4b EQUIVALENCE: the trust check had no
// row, and the suites stayed green without it) and one the operator QUALIFIED
// for the backend profile the execution claims (A92: a verifier pinned only
// for development backends attested a protected execution, and it counted).

const EXEC_VERIFIER: &str = "agent:exec-verifier";

/// Register a second verifier [`EXEC_VERIFIER`]: its key under the operator's
/// verifier root and in the store, `trusted` as asked, and pinned (a copy of
/// the fixture verifier's pin) for `profiles`. Returns its PKCS#8.
fn second_verifier(w: &World, trusted: bool, profiles: &[&str]) -> Vec<u8> {
    let (k, pk) = axon_loop_contracts::attestation::generate().unwrap();
    std::fs::write(
        operator_root().join("verifier").join("exec-verifier.pub"),
        format!("{pk}\n"),
    )
    .unwrap();
    let x = OpaqueRef::new(EXEC_VERIFIER).unwrap();
    let mut cfg = w.s.config().unwrap();
    cfg.verifier_keys.insert(x.clone(), pk);
    if trusted {
        cfg.trusted_verifiers.push(x.clone());
    }
    let mut pin = cfg.verifier_pins[&OpaqueRef::new(VERIFIER).unwrap()].clone();
    pin.backend_profiles = profiles.iter().map(|p| p.to_string()).collect();
    cfg.verifier_pins.insert(x, pin);
    w.s.write_config(&cfg).unwrap();
    k
}

fn drop_second_verifier_key() {
    let _ = std::fs::remove_file(operator_root().join("verifier").join("exec-verifier.pub"));
}

/// A protected evaluation of `exp` whose challenger executions are attested
/// by [`EXEC_VERIFIER`] (signed with `k`); every verdict is the fixture
/// verifier's, as before. Returns the challenger arm and the evaluation.
fn evaluated_with_exec_signer(w: &World, exp: &str, k: &[u8]) -> (axon_loop::evl::ArmResult, Ref) {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, exp, &specs_for(w));
    let mut v = evl_request(exp, &w.inc, &w.cand, &specs_for(w), &EvlOpts::default());
    on_protected_backend(&mut v);
    for t in challenger_trials(&mut v) {
        t["acf_attestation"] =
            attest_execution_with(k, EXEC_VERIFIER, &t["acf_request"], &t["acf_receipt"]);
    }
    clear_all(&w.s, &v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    (rec.arm_for_policy(&w.cand_ref).unwrap().clone(), e)
}

fn protected_world() -> World {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    w
}

/// EVL: an execution attested by a verifier the operator does not (or no
/// longer) trusts counts nothing, though its key is registered, operator-
/// rooted and pinned for the protected profile: only the trust check stands
/// between it and a protected count (round 4b reproduction,
/// eqv4b_revoked_verifier_test). Control: the same verifier, trusted, counts.
#[test]
fn a_revoked_verifiers_execution_attestation_counts_nothing() {
    for trusted in [true, false] {
        let w = protected_world();
        let k = second_verifier(&w, trusted, &[PROTECTED]);
        let (arm, _) = evaluated_with_exec_signer(&w, "rv", &k);
        drop_second_verifier_key();
        if trusted {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else {
            assert_eq!(
                arm.verified_pass, 0,
                "ATTACK: an execution attested by a verifier the operator does not trust counted \
                 as a protected execution: {:?}",
                arm.trials
            );
            assert!(
                arm.trials
                    .iter()
                    .all(|t| t.reason.contains("is not a trusted verifier")),
                "{:?}",
                arm.trials
            );
        }
    }
}

/// Amendment 115 (eqgate9): a verifier the operator trusts, under an operator-rooted key, with NO pin at all
/// attests a protected execution. Having no pin is not being qualified: `verifier_pins.get(..).is_some_and(..)`
/// flipped to `is_none_or` counted it, and every test pinned the second verifier for SOME profile.
#[test]
fn an_execution_attested_by_a_verifier_with_no_pin_at_all_counts_nothing() {
    let w = protected_world();
    let k = second_verifier(&w, true, &[PROTECTED]);
    let mut cfg = w.s.config().unwrap();
    assert!(
        cfg.verifier_pins
            .remove(&OpaqueRef::new(EXEC_VERIFIER).unwrap())
            .is_some(),
        "setup: the second verifier was pinned"
    );
    w.s.write_config(&cfg).unwrap();
    let (arm, _) = evaluated_with_exec_signer(&w, "np", &k);
    drop_second_verifier_key();
    assert_eq!(
        arm.verified_pass, 0,
        "ATTACK: an execution attested by a verifier with no pin at all counted as a protected \
         execution: {:?}",
        arm.trials
    );
    assert!(
        arm.trials
            .iter()
            .all(|t| t.reason.contains("is not qualified")),
        "{:?}",
        arm.trials
    );
}

/// Admission (`reverify_protected`): the execution attester trusted when the
/// evaluation ran, then withdrawn by the operator; the verdicts stay the
/// fixture verifier's, still trusted, so only the execution leg's trust check
/// refuses the ACCEPT. Control: nothing withdrawn, it is accepted.
#[test]
fn a_protected_admission_refuses_an_execution_attester_the_operator_withdrew() {
    for withdrawn in [false, true] {
        let w = protected_world();
        let k = second_verifier(&w, true, &[PROTECTED]);
        let (arm, e) = evaluated_with_exec_signer(&w, "aw", &k);
        assert_eq!(arm.verified_pass, 2, "setup: {:?}", arm.trials);
        if withdrawn {
            withdraw(&w, |c| {
                c.trusted_verifiers.retain(|v| v.as_str() != EXEC_VERIFIER)
            });
        }
        let got = admit(&w.s, "aw", &e, ADMITTER, false);
        drop_second_verifier_key();
        match (withdrawn, got) {
            (false, Ok((rec, _))) => {
                assert_eq!(rec.decision, Decision::Accept, "control: {:?}", rec.reasons)
            }
            (false, Err(e)) => panic!("control: {e}"),
            (true, Ok((rec, _))) => assert_ne!(
                rec.decision,
                Decision::Accept,
                "ATTACK: a protected admission accepted an execution attested by a verifier the \
                 operator withdrew: {:?}",
                rec.reasons
            ),
            (true, Err(e)) => assert!(e.to_string().contains("is not a trusted verifier"), "{e}"),
        }
    }
}

/// A92 (EVL): a verifier the operator trusts, under an operator-rooted key,
/// but QUALIFIED only for a development backend (its pin does not name the
/// protected profile) attests a protected execution. Its attestation counts
/// nothing: the operator's per-profile qualification bounds what a verifier
/// may attest, as it bounds what its verdicts may come from. Control: the
/// same verifier pinned for the protected profile counts.
#[test]
fn an_execution_attested_by_a_verifier_not_qualified_for_its_profile_counts_nothing() {
    for qualified in [true, false] {
        let w = protected_world();
        let profiles: &[&str] = if qualified {
            &[PROTECTED]
        } else {
            &["local-process-dev"]
        };
        let k = second_verifier(&w, true, profiles);
        let (arm, _) = evaluated_with_exec_signer(&w, "nq", &k);
        drop_second_verifier_key();
        if qualified {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else {
            assert_eq!(
                arm.verified_pass, 0,
                "ATTACK: an execution attested by a verifier qualified only for development \
                 backends counted as a protected execution: {:?}",
                arm.trials
            );
            assert!(
                arm.trials
                    .iter()
                    .all(|t| t.reason.contains("is not qualified")),
                "{:?}",
                arm.trials
            );
        }
    }
}

/// A92 (admission): the execution attester was qualified for the protected
/// profile when the evaluation ran; the operator then withdraws that profile
/// from its pin. Admission re-verifies the execution leg against the pin as it
/// is NOW, so the ACCEPT is refused. Control: the pin unchanged, accepted.
#[test]
fn a_protected_admission_refuses_an_execution_attester_no_longer_qualified() {
    for withdrawn in [false, true] {
        let w = protected_world();
        let k = second_verifier(&w, true, &[PROTECTED]);
        let (arm, e) = evaluated_with_exec_signer(&w, "aq", &k);
        assert_eq!(arm.verified_pass, 2, "setup: {:?}", arm.trials);
        if withdrawn {
            withdraw(&w, |c| {
                c.verifier_pins
                    .get_mut(&OpaqueRef::new(EXEC_VERIFIER).unwrap())
                    .unwrap()
                    .backend_profiles
                    .retain(|p| p != PROTECTED)
            });
        }
        let got = admit(&w.s, "aq", &e, ADMITTER, false);
        drop_second_verifier_key();
        match (withdrawn, got) {
            (false, Ok((rec, _))) => {
                assert_eq!(rec.decision, Decision::Accept, "control: {:?}", rec.reasons)
            }
            (false, Err(e)) => panic!("control: {e}"),
            (true, Ok((rec, _))) => assert_ne!(
                rec.decision,
                Decision::Accept,
                "ATTACK: a protected admission accepted an execution attested by a verifier no \
                 longer qualified for its profile: {:?}",
                rec.reasons
            ),
            (true, Err(e)) => assert!(e.to_string().contains("is not qualified"), "{e}"),
        }
    }
}

/// The context signature's VERIFICATION (`attestation::verify_document`, the
/// primitive every detached signature in the loop goes through: contexts,
/// execution attestations, clearances). The signature document presents the
/// observer's registered key and every bound field genuinely, but its
/// signature bytes were made by another key: only the Ed25519 verification
/// can refuse it (amendment 61: that check had no row). Control: the
/// observer's own signature counts.
#[test]
fn a_context_signature_not_made_by_the_key_it_presents_counts_nothing() {
    for forged in [false, true] {
        let w = protected_world();
        trust_monitor(&w.s);
        freeze_plan(&w.s, "fs", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        assign_specs(&w.s, "fs", &specs_for(&w));
        let mut v = evl_request("fs", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
        on_protected_backend(&mut v);
        if forged {
            let (other, _) = axon_loop_contracts::attestation::generate().unwrap();
            for t in challenger_trials(&mut v) {
                let by_other = axon_loop_contracts::attestation::sign_document(
                    &other,
                    axon_loop::evl::CONTEXT_DOMAIN,
                    &OpaqueRef::new(OBSERVER).unwrap(),
                    &t["context"],
                )
                .unwrap();
                // Everything genuine but the signature bytes.
                t["context_signature"]["signature"] = by_other["signature"].clone();
            }
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let arm = rec.arm_for_policy(&w.cand_ref).unwrap().clone();
        if !forged {
            assert_eq!(arm.verified_pass, 2, "control: {:?}", arm.trials);
        } else {
            assert_eq!(
                arm.verified_pass, 0,
                "ATTACK: a context signature presenting the observer's key but made by another \
                 key counted: {:?}",
                arm.trials
            );
            assert!(
                arm.trials
                    .iter()
                    .all(|t| t.reason.contains("does not verify")),
                "{:?}",
                arm.trials
            );
        }
    }
}
