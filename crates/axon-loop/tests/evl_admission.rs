//! B276 EVL + B277 admission + end-to-end into the fenced pointer.
mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::evl::Outcome;
use axon_loop::{evo, pointer};
use axon_loop_contracts::*;
use common::*;
use serde_json::json;

#[test]
fn evl_counts_missing_and_unknown_against_quality() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 3, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 3, 3, 3, Some(100), Some(50));
    specs[3].4 = Out::Unknown; // c0 timed out
    specs[5].4 = Out::Fail; // c2 failed
    let o = EvlOpts {
        deliver: Box::new(|t| t != "c1"),
        ..Default::default()
    };
    let (rec, _) = evaluate(&w.s, &evl_request("exp", &w.inc, &w.cand, &specs, &o)).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(
        (c.assigned, c.verified_pass, c.fail, c.unknown, c.missing),
        (3, 0, 1, 2, 1)
    );
    assert!(c
        .trials
        .iter()
        .any(|t| t.trial_id.as_str() == "c1" && t.outcome == Outcome::Unknown));
    assert_eq!(c.economics.missing_records, 1);
    assert_eq!(rec.arm_for_policy(&w.inc_ref).unwrap().verified_pass, 3);
}

#[test]
fn evl_rejects_subject_or_untrusted_verifier_and_stale_epoch() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let mut tr = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    tr.verifier = WORKER;
    v["trials"][2] = trial(&tr);
    let mut tr = Trial::new(&w.cand, "task-1", "challenger-1", "c1");
    tr.verifier = PROPOSER;
    v["trials"][3] = trial(&tr);
    let mut tr = Trial::new(&w.inc, "task-0", "incumbent", "i0");
    tr.epoch = 5;
    v["trials"][0] = trial(&tr);
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!((c.verified_pass, c.unknown), (0, 2));
    assert!(rec.subject_issuers.iter().any(|s| s.as_str() == PROPOSER));
    let i = rec.arm_for_policy(&w.inc_ref).unwrap();
    assert_eq!((i.verified_pass, i.unknown), (1, 1));
}

#[test]
fn evl_refusals_write_nothing() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let good = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    // Intake first (production order); the refusals under test are
    // evaluation's, and a re-intake of recorded bytes writes nothing.
    intake_all(&w.s, &good);
    let before = snapshot(w.dir.path());
    let mut v = good.clone();
    v["assigned"].as_array_mut().unwrap().remove(0);
    assert!(evaluate(&w.s, &v).is_err(), "unassigned delivered");
    let mut v = good.clone();
    v["evaluator_ref"] = json!(PROPOSER);
    assert!(matches!(evaluate(&w.s, &v), Err(LoopError::Refused(_))));
    let mut v = good.clone();
    v["trials"][2]["episode"]["verification"]["matched_checks"] = json!(0);
    assert!(evaluate(&w.s, &v).is_err(), "tampered episode");
    let mut v = good.clone();
    v["experiment_id"] = json!("not-frozen");
    assert!(matches!(evaluate(&w.s, &v), Err(LoopError::NotReady(_))));
    assert_eq!(snapshot(w.dir.path()), before);
}

#[test]
fn full_loop_accept_activate_future_task_rollback() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, eval_ref) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(PROPOSER).unwrap());
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.dir.path());
    assert!(matches!(
        admit(&w.s, "exp", &eval_ref, PROPOSER, false),
        Err(LoopError::Refused(_))
    ));
    assert!(matches!(
        admit(&w.s, "exp", &eval_ref, "op:stranger", false),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);

    let (rec, adm) = admit(&w.s, "exp", &eval_ref, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    assert_eq!(
        admit(&w.s, "exp", &eval_ref, ADMITTER, false).unwrap().1,
        adm,
        "deterministic"
    );
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref,
        "admission does not activate"
    );
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    let r = pointer::resolve(&w.s, &scope()).unwrap();
    assert_eq!(
        (r.pin.version.digest.clone(), r.pin.epoch.get()),
        (w.cand_ref.clone(), 2)
    );
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref
    );
    let h = evo::history(&w.s, &scope()).unwrap();
    assert!(h.iter().any(|x| matches!(
        x,
        evo::Hypothesis::Verdict {
            verdict: evo::Verdict::Accept,
            mechanism_test: false,
            ..
        }
    )));
}

fn decision(
    w: &World,
    exp: &str,
    specs: &[Spec],
    o: EvlOpts,
    edit: impl FnOnce(&mut serde_json::Value),
) -> (Decision, Vec<String>) {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, edit).unwrap();
    let (_, e) = evaluate(&w.s, &evl_request(exp, &w.inc, &w.cand, specs, &o)).unwrap();
    let (rec, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    (rec.decision, rec.reasons)
}

#[test]
fn inconclusive_on_small_sample_liability_and_unknowns() {
    // Sample size is now fixed at FREEZE: a manifest smaller than the planned
    // independent units cannot be frozen, so no evaluation can be too small.
    let w = world();
    let e = freeze_plan(&w.s, "small", &w.inc_ref, &w.cand_ref, |v| {
        v["independent_units"] = json!(3)
    })
    .unwrap_err();
    assert!(
        matches!(e, LoopError::NotReady(ref m) if m.contains("independent_units")),
        "{e}"
    );

    let w = world();
    freeze_plan(&w.s, "liab", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("liab", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let mut tr = Trial::new(&w.cand, "task-0", "challenger-1", "c0");
    tr.cost = Some(50);
    tr.liability = 5;
    v["trials"][2] = trial(&tr);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, _) = admit(&w.s, "liab", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Inconclusive);
    assert!(
        rec.reasons.iter().any(|r| r.contains("liability")),
        "{:?}",
        rec.reasons
    );

    let w = world();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    specs[2].4 = Out::Unknown;
    let (d, r) = decision(&w, "unk", &specs, EvlOpts::default(), |_| {});
    assert_eq!(d, Decision::Inconclusive, "{r:?}");
    assert!(r.iter().any(|x| x.contains("noninferiority")));
}

#[test]
fn reject_on_inferiority_or_no_economic_benefit() {
    // G11-r22-admission-disposition: explicit REASONS, asserted — not just
    // the decision.
    let w = world();
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    let (d, r) = decision(&w, "worse", &specs, EvlOpts::default(), |_| {});
    assert_eq!(d, Decision::Reject);
    assert!(
        r.iter()
            .any(|x| x.contains("quality inferiority established")),
        "{r:?}"
    );
    let w = world();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(95));
    let (d, r) = decision(&w, "pricey", &specs, EvlOpts::default(), |_| {});
    assert_eq!(d, Decision::Reject);
    assert!(
        r.iter().any(|x| x.contains("economic benefit not met")),
        "{r:?}"
    );
}

#[test]
fn admission_refuses_wrong_corpus_role() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    assert!(matches!(
        admit(&w.s, "exp", &e, ADMITTER, true),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);
}

/// Re-audit 4 (clause auditor, executed): removing a verifier's key or trust
/// did not stop its verdicts from counting — admission and activation read
/// only the stored evaluation. Now each counted verdict is re-checked against
/// the CURRENT config: its issuer still trusted, its registered key still the
/// one it verified under. Positive control: an unrevoked experiment ACCEPTs.
///
/// Mutation: drop the re-check in `admission::derive` → red.
#[test]
fn a_revoked_verifier_s_verdicts_stop_counting() {
    let setup = |exp: &str| {
        let w = world();
        freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let (_, e) = evaluate(
            &w.s,
            &evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default()),
        )
        .unwrap();
        (w, e)
    };
    let edit = |w: &World, f: &dyn Fn(&mut axon_loop::store::Config)| {
        let mut cfg = w.s.config().unwrap();
        f(&mut cfg);
        w.s.write_config(&cfg).unwrap();
    };
    let v = OpaqueRef::new(VERIFIER).unwrap();

    let (w, e) = setup("ok");
    assert_eq!(
        admit(&w.s, "ok", &e, ADMITTER, false).unwrap().0.decision,
        Decision::Accept
    );

    // Key removed after the evaluation: nothing to admit.
    let (w, e) = setup("unkeyed");
    edit(&w, &|c| {
        c.verifier_keys.remove(&v);
    });
    let err = admit(&w.s, "unkeyed", &e, ADMITTER, false).unwrap_err();
    assert!(
        matches!(err, LoopError::Refused(ref m) if m.contains("no longer trusts")),
        "{err}"
    );

    // Re-keyed: the verdicts were authenticated under the OLD key.
    let (w, e) = setup("rekeyed");
    let (_, other) = axon_loop_contracts::attestation::generate().unwrap();
    edit(&w, &|c| {
        c.verifier_keys.insert(v.clone(), other.clone());
    });
    assert!(admit(&w.s, "rekeyed", &e, ADMITTER, false).is_err());

    // Untrusted AFTER the admission: the activation re-derives and refuses,
    // writing nothing.
    let (w, e) = setup("late");
    let (rec, adm) = admit(&w.s, "late", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept);
    edit(&w, &|c| c.trusted_verifiers.retain(|x| x != &v));
    let before = snapshot(w.dir.path());
    let err = pointer::transition(
        &w.s,
        &tparse(&transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap_err();
    assert!(
        matches!(err, LoopError::Refused(ref m) if m.contains("no longer trusts")),
        "{err}"
    );
    assert_eq!(snapshot(w.dir.path()), before);
}

/// G11-r22-independent-admission: an admitter may hold NO other loop role,
/// and every binding is refused by its own named reason. Compute Fabric (a
/// trusted verifier), a context observer and a safety monitor could all be
/// listed as trusted admitters before; the proposer — EVO's `intervention` is
/// the learned ranking, so the proposer IS the ranker — the subject and the
/// evaluator were refused, but only by a shared message nothing asserted.
/// Each refusal writes nothing; the independent admitter still admits.
///
/// Mutation: drop the role-separation block in `derive` → red.
#[test]
fn an_admitter_holds_no_other_loop_role() {
    let w = world();
    trust_monitor(&w.s);
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, eval_ref) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let mut cfg = w.s.config().unwrap();
    for who in [VERIFIER, OBSERVER, MONITOR, PROPOSER, EVALUATOR, WORKER] {
        cfg.trusted_admitters.push(OpaqueRef::new(who).unwrap());
    }
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.dir.path());
    for (who, why) in [
        (VERIFIER, "a trusted verifier (Compute Fabric)"),
        (OBSERVER, "a context observer"),
        (MONITOR, "a safety monitor"),
        (PROPOSER, "the proposer (ranker)"),
        (EVALUATOR, "it is the evaluator"),
        (WORKER, "it is a subject issuer"),
    ] {
        match admit(&w.s, "exp", &eval_ref, who, false) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{who}: {m}"),
            other => panic!("{who} admitted: {other:?}"),
        }
    }
    assert_eq!(snapshot(w.dir.path()), before, "a refusal wrote something");
    let (rec, _) = admit(&w.s, "exp", &eval_ref, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
}

/// G11-r22-independent-admission: complete experiment bindings. An evaluation
/// cannot be admitted under another experiment's frozen plan because a
/// candidate can be frozen in ONE experiment only (no plan shopping): the
/// second freeze is refused, so no other plan exists to admit it under. The
/// per-field `binding:` refusals in `derive` are the defence behind this.
#[test]
fn a_candidate_is_bound_to_one_experiment() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    match freeze_plan(&w.s, "other", &w.inc_ref, &w.cand_ref, |_| {}) {
        Err(LoopError::Refused(m)) => assert!(m.contains("no plan shopping"), "{m}"),
        other => panic!("a second experiment froze the same candidate: {other:?}"),
    }
}

/// G11-r22-admission-disposition: only an ACCEPTED admission by a CURRENTLY
/// authorized admitter permits activation. A genuinely journalled REJECT or
/// INCONCLUSIVE — decided by the real rule, not hand-written — is refused at
/// activation, and so is an ACCEPT whose admitter the operator has since
/// untrusted. The pointer does not move in any of them.
///
/// Mutation: drop the `decision != Accept` refusal in `rederive` → red.
#[test]
fn only_an_accepted_currently_authorized_admission_activates() {
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
    for (exp, want, why) in [
        ("worse", Decision::Reject, "quality inferiority established"),
        (
            "unk",
            Decision::Inconclusive,
            "noninferiority cannot be established",
        ),
    ] {
        let w = world();
        freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let mut specs = pair(
            &w.inc,
            &w.cand,
            2,
            2,
            if exp == "worse" { 0 } else { 2 },
            Some(100),
            Some(50),
        );
        if exp == "unk" {
            specs[2].4 = Out::Unknown;
        }
        let (_, e) = evaluate(
            &w.s,
            &evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default()),
        )
        .unwrap();
        let (rec, adm) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
        assert_eq!(rec.decision, want, "{:?}", rec.reasons);
        assert!(
            rec.reasons.iter().any(|x| x.contains(why)),
            "{:?}",
            rec.reasons
        );
        match activate(&w, &adm) {
            Err(LoopError::Refused(m)) => assert!(m.contains("not ACCEPT"), "{exp}: {m}"),
            other => panic!("{exp}: a {want:?} admission activated: {other:?}"),
        }
        assert_eq!(
            pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
            w.inc_ref
        );
    }
    // An ACCEPT whose admitter was untrusted after admission.
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, adm) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters.retain(|a| a.as_str() != ADMITTER);
    w.s.write_config(&cfg).unwrap();
    match activate(&w, &adm) {
        Err(LoopError::Refused(m)) => assert!(m.contains("trusted-admitter"), "{m}"),
        other => panic!("an untrusted admitter's ACCEPT activated: {other:?}"),
    }
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref
    );
}

/// G33-r22-decision-rule-freeze / G01 freshness: the rule is frozen BEFORE
/// the outcomes it judges. `acf-receipt-attestation/2` signs the verifier's
/// clock (`issued_ms`); a verdict attested before the plan froze does not
/// count — here genuinely signed by the TRUSTED verifier, only dated earlier.
/// Before `/2` the ordering rested on the unsigned episode context alone.
/// Honest control: the same evaluation with current attestations accepts.
///
/// Mutation: drop the `issued < freeze_ms` rule in EVL's judge → red.
#[test]
fn a_verdict_attested_before_the_freeze_does_not_count() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    for t in v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
    {
        let req: ComputeRequest =
            serde_json::from_value(t["verification_request"].clone()).unwrap();
        let rc: ExecutionReceipt =
            serde_json::from_value(t["verification_receipt"].clone()).unwrap();
        t["verification_attestation"] = axon_loop_contracts::attestation::sign(
            &verifier_key().0,
            &OpaqueRef::new(VERIFIER).unwrap(),
            &req,
            &rc,
            1, // long before the freeze
        )
        .unwrap();
    }
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let arm = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(arm.verified_pass, 0, "a pre-freeze verdict counted");
    assert_eq!(arm.unknown, 2);
    let t = &arm.trials[0];
    assert!(t.reason.contains("before the plan froze"), "{:?}", t.reason);

    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let (rec, _) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default()),
    )
    .unwrap();
    assert_eq!(rec.arm_for_policy(&w.cand_ref).unwrap().verified_pass, 2);
}

fn specs_for(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}
