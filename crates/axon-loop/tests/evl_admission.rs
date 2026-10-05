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

/// EVL's own independence rule for a PASS (evl.rs `} else if !issuer_ok`,
/// amendment 74): a pass verified by an issuer the evaluation names as a
/// SUBJECT never counts, even when the operator trusts that verifier and its
/// signature holds. Here the request lists the fixture's trusted VERIFIER
/// among its subject issuers; every pass it verified must stay Unknown.
/// (Dominated: bind_episode, M1299, and verify_check_evidence, M10, apply the
/// same rule with the same sets first; four cells, EQUIV_RECORD["M1726"].)
/// Control: the same request without the verifier as a subject counts.
#[test]
fn a_pass_verified_by_a_subject_issuer_never_counts() {
    let run = |verifier_is_subject: bool| {
        let w = world();
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
        if verifier_is_subject {
            v["subject_issuers"]
                .as_array_mut()
                .unwrap()
                .push(json!(VERIFIER));
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        let c = rec.arm_for_policy(&w.cand_ref).unwrap();
        let i = rec.arm_for_policy(&w.inc_ref).unwrap();
        (c.verified_pass, i.verified_pass)
    };
    let got = run(true);
    assert_eq!(
        got,
        (0, 0),
        "ATTACK: a pass verified by a subject issuer was counted (candidate, incumbent)"
    );
    assert_eq!(
        run(false),
        (2, 2),
        "control: an independent verifier's passes count"
    );
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
    freeze_plan(&w.s, "liab", &w.inc_ref, &w.cand_ref, |v| {
        v["budget_rule"] = json!("max_unresolved_liability_micro=0")
    })
    .unwrap();
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
    // A frozen COST criterion over Fabric-executed trials: each execution's
    // cost is unknown (D10) and holds its reservation as liability, so the
    // economics are never established — INCONCLUSIVE, never a known-cost
    // ACCEPT on a partial total (review wf_d788c05a-be2). The Known-totals
    // rejection is pinned on `decide` itself (admission.rs decide_tests).
    let w = world();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (d, r) = decision(&w, "costed", &specs, EvlOpts::default(), |v| {
        v["economic_threshold"] = json!("min_cost_reduction_ppm=100000")
    });
    assert_eq!(d, Decision::Inconclusive, "{r:?}");
    assert!(
        r.iter()
            .any(|x| x.contains("economics cannot be established")),
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
    // An observer cannot even be REGISTERED as an admitter (ADR-002), so its
    // refusal happens at config write, before any admission.
    let mut bad = cfg.clone();
    bad.trusted_admitters
        .push(OpaqueRef::new(OBSERVER).unwrap());
    match w.s.write_config(&bad) {
        Err(LoopError::Refused(m)) => assert!(m.contains("is also a trusted admitter"), "{m}"),
        other => panic!("an observer was registered as an admitter: {other:?}"),
    }
    for who in [VERIFIER, MONITOR, PROPOSER, EVALUATOR, WORKER] {
        cfg.trusted_admitters.push(OpaqueRef::new(who).unwrap());
    }
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.dir.path());
    for (who, why) in [
        (VERIFIER, "a trusted verifier (Compute Fabric)"),
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

/// G11-r22-independent-admission (C9 round 1b, M114): the candidate's
/// PROPOSER (the ranker) cannot admit it, even when the evaluation record does
/// not list it as a subject. EVL always adds the arm proposers to the
/// record's `subject_issuers`, so on an honest record the subject-issuer check
/// refuses the proposer too; but the record is a store writer's to write, and
/// admission re-derives from it. Here a store writer drops the proposer from
/// `subject_issuers` of a genuine evaluation, so the proposer==admitter check
/// is the only refusal. Control: the same forged record is admissible by the
/// independent admitter (the forgery changes nothing else).
#[test]
fn the_proposer_cannot_admit_its_own_candidate_whatever_the_record_lists() {
    let w = world();
    trust_monitor(&w.s);
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (rec, _) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    assert!(
        rec.subject_issuers.iter().any(|s| s.as_str() == PROPOSER),
        "EVL lists the proposer as a subject: {:?}",
        rec.subject_issuers
    );
    let mut forged = rec.clone();
    forged.subject_issuers.retain(|s| s.as_str() != PROPOSER);
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    forged_append(
        w.s.root(),
        axon_loop::ledger::Event::Evaluation {
            scope: scope(),
            experiment_id: "exp".into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
    );
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(PROPOSER).unwrap());
    w.s.write_config(&cfg).unwrap();
    match admit(&w.s, "exp", &fe, PROPOSER, false) {
        Err(LoopError::Refused(m)) => assert!(m.contains("the proposer (ranker)"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: the proposer admitted its own candidate: {:?} {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("{e}"),
    }
    let (adm, _) = admit(&w.s, "exp", &fe, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
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

/// ADR-002: every role has its own key and an observer holds no other role,
/// so Fabric (a verifier) can consume an observation but never mint one. Both
/// the write path and the read path refuse, so a hand-edited config.json does
/// not smuggle a shared key past `write_config`.
#[test]
fn an_observer_key_and_identity_are_its_own() {
    let w = world();
    let base = w.s.config().unwrap();
    let obs = OpaqueRef::new(OBSERVER).unwrap();
    let ver = OpaqueRef::new(VERIFIER).unwrap();
    // One key for two roles: the verifier's key also registered as an observer's.
    let mut shared = base.clone();
    shared.observer_keys.insert(
        OpaqueRef::new("fixture:observer2").unwrap(),
        verifier_key().1.clone(),
    );
    // An observer that is also a trusted verifier.
    let mut dual = base.clone();
    dual.trusted_verifiers.push(obs.clone());
    for (bad, why) in [
        (&shared, "registered for two roles"),
        (&dual, "is also a trusted verifier"),
    ] {
        match w.s.write_config(bad) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{m}"),
            other => {
                panic!("ATTACK: a config breaking role separation was written ({why}): {other:?}")
            }
        }
    }
    // The read path refuses the same config written behind the store's back.
    assert!(
        base.verifier_keys.contains_key(&ver),
        "fixture registers a verifier key"
    );
    std::fs::write(
        w.dir.path().join("config.json"),
        serde_json::to_vec(&shared).unwrap(),
    )
    .unwrap();
    match w.s.config() {
        Err(LoopError::Refused(m)) => assert!(m.contains("registered for two roles"), "{m}"),
        other => panic!("ATTACK: a shared-key config was read back: {other:?}"),
    }
}

/// G11-r22-admission-disposition: "stale/unknown costs … cannot be hidden by
/// aggregate utility". The Fabric EXECUTION is a cost component of every
/// executed trial: unknown under D10, holding at least its reservation and
/// any cost the receipt reports as liability. A candidate whose execution
/// receipts report a huge cost is never ACCEPTed as the cheaper arm on its
/// model usage alone, and its total is never stated as Known (review
/// wf_d788c05a-be2, executed).
#[test]
fn an_execution_cost_is_never_omitted_from_the_economics() {
    let w = world();
    freeze_plan(&w.s, "exec", &w.inc_ref, &w.cand_ref, |v| {
        v["economic_threshold"] = json!("min_cost_reduction_ppm=100000")
    })
    .unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("exec", &w.inc, &w.cand, &specs, &EvlOpts::default());
    for t in v["trials"].as_array_mut().unwrap() {
        if t["episode"]["identity"]["arm_id"] == "challenger-1" {
            t["acf_receipt"]["cost_micro"] = json!(900_000);
            t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
        }
    }
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    match c.economics.single_total() {
        Some(axon_loop::tel::Total::Unresolved {
            unresolved_liability_micro,
            ..
        }) => assert!(*unresolved_liability_micro >= 2 * 900_000, "{c:?}"),
        other => panic!("the candidate's cost was stated without its execution: {other:?}"),
    }
    let (adm, _) = admit(&w.s, "exec", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Inconclusive, "{:?}", adm.reasons);
    assert!(
        adm.reasons
            .iter()
            .any(|r| r.contains("economics cannot be established")),
        "{:?}",
        adm.reasons
    );
}

/// Review wf_8aad6d16-ad6 (G11-r22-admission-disposition, executed): one
/// trial's usage relabelled to another currency made the arm's economics
/// multi-currency, and the admission read its liability as 0 — the frozen
/// liability tolerance passed and the candidate was ACCEPTed. Now the
/// relabelled trial is Unbound (its usage is not in its execution's
/// currency) and nothing is ACCEPTed; control: the same plan without the
/// relabel is INCONCLUSIVE on the liability it really holds.
#[test]
fn a_relabelled_currency_never_hides_a_liability() {
    let run = |relabel: bool| {
        let w = world();
        let exp = if relabel { "eur" } else { "usd" };
        freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |v| {
            v["budget_rule"] = json!("max_unresolved_liability_micro=300000")
        })
        .unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let mut v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
        for t in v["trials"].as_array_mut().unwrap() {
            if t["episode"]["identity"]["arm_id"] == "challenger-1" {
                t["acf_receipt"]["cost_micro"] = json!(900_000);
                t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
                if relabel && t["episode"]["identity"]["trial_id"] == "c1" {
                    t["episode"]["usage"]["currency"] = json!("EUR");
                }
            }
        }
        let (rec, e) = evaluate(&w.s, &v).unwrap();
        let (adm, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
        (rec, adm)
    };
    let (_, adm) = run(false);
    assert_eq!(adm.decision, Decision::Inconclusive, "{:?}", adm.reasons);
    assert!(
        adm.reasons
            .iter()
            .any(|r| r.contains("exceeds plan tolerance")),
        "{:?}",
        adm.reasons
    );
    let (rec, adm) = run(true);
    let c = rec.arm_for_policy(&rec.arms[1].policy_ref).unwrap();
    let _ = c;
    let cand = rec
        .arms
        .iter()
        .find(|a| a.arm_id.as_str() == "challenger-1")
        .unwrap();
    let c1 = cand
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == "c1")
        .unwrap();
    assert!(c1.reason.contains("usage currency EUR"), "{}", c1.reason);
    assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
}

/// A development-class verdict counts only while the observer its context
/// was admitted under is trusted: withdrawing it after admission refuses the
/// activation (review wf_8aad6d16-ad6, MAJOR-ADJACENT, executed).
#[test]
fn a_development_verdict_rests_on_a_currently_trusted_observer() {
    let w = world();
    let adm = accepted(&w, "obs");
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_observers.retain(|o| o.as_str() != OBSERVER);
    cfg.observer_keys.remove(&OpaqueRef::new(OBSERVER).unwrap());
    w.s.write_config(&cfg).unwrap();
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
    match pointer::transition(&w.s, &tparse(&t)) {
        Err(LoopError::Refused(m)) => assert!(m.contains("no longer trusts"), "{m}"),
        o => panic!("activated on a withdrawn observer: {o:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
}

/// ADR-001 D4: a report-only ACCEPT says economics were not assessed.
#[test]
fn a_report_only_accept_says_so() {
    let w = world();
    let adm = accepted(&w, "ro");
    let rec = axon_loop::admission::load(&w.s, &adm).unwrap();
    assert!(
        rec.reasons.iter().any(|r| r.contains("report-only")),
        "{:?}",
        rec.reasons
    );
}

// ── C9 round 4 fix wave, ROWS2 wave 2 (rows M815-M859): refusal sites of
// `admission.rs` that had no row. Each attack goes through the production
// `admission::admit` (or activation through `pointer::transition`), over
// records a STORE WRITER forges (the evaluation record and the ledger are the
// store writer's to write; the signatures are not), and is refused by the
// one check under test. Control first: the unforged route admits.

/// A genuine dev-class evaluation of `exp`, forged by `forge` (the record's
/// JSON), put in the CAS and journalled by a forged ledger append. Returns
/// (the genuine record, the forged ref).
fn forged_evaluation(
    w: &World,
    exp: &str,
    forge: impl FnOnce(&mut serde_json::Value),
) -> (axon_loop::evl::EvaluationRecord, Ref) {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (rec, _) = evaluate(
        &w.s,
        &evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let mut j = serde_json::to_value(&rec).unwrap();
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
    (rec, fe)
}

/// Each binding and independence rule of `derive`, on the route where it
/// alone refuses (M815-M821): one field of a genuine evaluation forged by a
/// store writer, admitted by the trusted, independent admitter.
#[test]
fn each_admission_binding_refuses_its_own_forgery() {
    let w = world();
    let (_, fe) = forged_evaluation(&w, "ctl", |_| {});
    let (adm, _) = admit(&w.s, "ctl", &fe, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
    type Forge = fn(&mut serde_json::Value);
    let cases: [(&str, Forge, &str); 7] = [
        (
            "an evaluation of another scope",
            |j| j["scope"]["task_family"] = json!("other-family"),
            "evaluation scope differs",
        ),
        (
            "an evaluation of another experiment",
            |j| j["experiment_id"] = json!("another-experiment"),
            "belongs to a different experiment",
        ),
        (
            "an evaluation made under another plan",
            |j| j["plan_ref"] = json!(r('9')),
            "made under a different plan",
        ),
        (
            "an evaluation made under another freeze",
            |j| j["freeze_seq"] = json!(j["freeze_seq"].as_u64().unwrap() + 1000),
            "not made under this freeze",
        ),
        (
            "an evaluation with a third arm",
            |j| {
                let mut extra = j["arms"][0].clone();
                extra["arm_id"] = json!("extra-arm");
                extra["policy_ref"] = json!(r('7'));
                j["arms"].as_array_mut().unwrap().push(extra);
            },
            "exactly an incumbent and a candidate arm",
        ),
        (
            "an evaluation listing the admitter as a subject issuer",
            |j| {
                j["subject_issuers"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!(ADMITTER))
            },
            "it is a subject issuer",
        ),
        (
            "an evaluation naming the admitter as its evaluator",
            |j| j["evaluator_ref"] = json!(ADMITTER),
            "it is the evaluator",
        ),
    ];
    for (i, (attack, forge, why)) in cases.into_iter().enumerate() {
        let w = world();
        let exp = format!("exp{i}");
        let (_, fe) = forged_evaluation(&w, &exp, forge);
        match admit(&w.s, &exp, &fe, ADMITTER, false) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{attack}: {m}"),
            Ok((adm, _)) => panic!(
                "ATTACK: {attack} was admitted: {:?} {:?}",
                adm.decision, adm.reasons
            ),
            Err(e) => panic!("{attack}: refused otherwise: {e}"),
        }
    }
}

/// M823: a counted verdict whose evaluation does not record which verifier
/// authenticated it does not count. A store writer drops the `verification`
/// of one of the candidate's passes; the counters still say it passed.
#[test]
fn a_counted_verdict_with_no_recorded_verifier_is_refused() {
    let w = world();
    let cand = w.cand_ref.to_string();
    let (_, fe) = forged_evaluation(&w, "exp", |j| {
        for arm in j["arms"].as_array_mut().unwrap() {
            if arm["policy_ref"] == cand {
                arm["trials"][0]
                    .as_object_mut()
                    .unwrap()
                    .remove("verification");
            }
        }
    });
    match admit(&w.s, "exp", &fe, ADMITTER, false) {
        Err(LoopError::Refused(m)) => assert!(m.contains("does not record which"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: a verdict with no recorded verifier counted: {:?} {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("refused otherwise: {e}"),
    }
}

/// M822: only a trusted admitter admits. `op:stranger` holds no loop role,
/// so no role rule refuses it; only the trusted-admitter set does.
#[test]
fn an_admitter_the_operator_does_not_trust_admits_nothing() {
    let w = world();
    let (_, fe) = forged_evaluation(&w, "exp", |_| {});
    match admit(&w.s, "exp", &fe, "op:stranger", false) {
        Err(LoopError::Refused(m)) => assert!(m.contains("trusted-admitter"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: an untrusted admitter admitted: {:?} {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("refused otherwise: {e}"),
    }
    let (adm, _) = admit(&w.s, "exp", &fe, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
}

/// M824: a mechanism-test admission counts only mechanism-test evidence.
/// The genuine evaluation's trials are confirmation-corpus.
#[test]
fn a_mechanism_test_admission_counts_no_confirmation_trial() {
    let w = world();
    let (_, fe) = forged_evaluation(&w, "exp", |_| {});
    match admit(&w.s, "exp", &fe, ADMITTER, true) {
        Err(LoopError::Refused(m)) => assert!(m.contains("corpus role"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: a mechanism-test admission counted confirmation evidence: {:?} {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("refused otherwise: {e}"),
    }
}

/// M825: an evaluation journalled BEFORE the freeze it names is not an
/// evaluation under that freeze. A store writer rewrites the (unkeyed)
/// ledger, putting a genuine evaluation's journal entry just before the
/// plan's freeze, re-chained; the record names the freeze's new sequence, so
/// every other binding holds.
#[test]
fn an_evaluation_journalled_before_its_freeze_is_refused() {
    let w = world();
    let (rec, _) = forged_evaluation(&w, "exp", |_| {});
    let mut l = read_ledger(w.s.root());
    let at = l
        .iter()
        .position(|e| matches!(&e.event, axon_loop::ledger::Event::Freeze { .. }))
        .unwrap();
    let mut forged = rec.clone();
    forged.freeze_seq = l[at].seq + 1;
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    let ev = axon_loop::ledger::Entry {
        schema: Default::default(),
        seq: 0,
        prev: r('0'),
        recorded_ms: l[at].recorded_ms,
        event: axon_loop::ledger::Event::Evaluation {
            scope: scope(),
            experiment_id: "exp".into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
        mac: None,
    };
    l.insert(at, ev);
    rechain(&mut l);
    write_ledger(w.s.root(), &l);
    forge_head(w.s.root(), &l);
    match admit(&w.s, "exp", &fe, ADMITTER, false) {
        Err(LoopError::Refused(m)) => assert!(m.contains("predates the freeze"), "{m}"),
        Ok((adm, _)) => panic!(
            "ATTACK: an evaluation journalled before its freeze was admitted: {:?} {:?}",
            adm.decision, adm.reasons
        ),
        Err(e) => panic!("refused otherwise: {e}"),
    }
}

/// Renumber and re-chain a forged ledger from its first entry.
fn rechain(l: &mut [axon_loop::ledger::Entry]) {
    for i in 0..l.len() {
        l[i].seq = i as u64 + 1;
        if i > 0 {
            l[i].prev = digest(&l[i - 1]).unwrap();
        }
    }
}

fn activate(w: &World, adm: &Ref) -> Result<PointerRecordOut, LoopError> {
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
    .map(|_| ())
}
type PointerRecordOut = ();

/// M826, M827: activation re-derives the admission from the journal. A store
/// writer (a) removes a genuine ACCEPT's journal entry, re-chaining the
/// ledger (the record is still in the CAS, and still re-derives identically:
/// only "journalled by `admit`" refuses it), or (b) stores an ACCEPT record
/// whose reasons differ from what the plan and evaluation decide, with a
/// forged journal entry for it (only the re-derivation equality refuses it).
/// Control: the genuine journalled ACCEPT activates.
#[test]
fn activation_rests_only_on_a_journalled_admission_that_re_derives() {
    let w = world();
    let adm = accepted(&w, "exp");
    let mut l = read_ledger(w.s.root());
    l.retain(|e| {
        !matches!(&e.event, axon_loop::ledger::Event::Admission { admission_ref, .. }
            if admission_ref == &adm)
    });
    rechain(&mut l);
    write_ledger(w.s.root(), &l);
    forge_head(w.s.root(), &l);
    match activate(&w, &adm) {
        Err(LoopError::Refused(m)) => assert!(m.contains("never journalled"), "{m}"),
        other => panic!("ATTACK: an admission never journalled by `admit` activated: {other:?}"),
    }

    let w = world();
    let adm = accepted(&w, "exp");
    let mut rec: axon_loop::admission::AdmissionRecord =
        w.s.get_record("admissions", &adm).unwrap();
    rec.reasons.push("a reason the plan never gave".into());
    let fa = w.s.put_cas("admissions", &rec).unwrap();
    forged_append(
        w.s.root(),
        axon_loop::ledger::Event::Admission {
            scope: rec.scope.clone(),
            experiment_id: rec.experiment_id.clone(),
            admission_ref: fa.clone(),
            target_policy_ref: rec.target_policy_ref.clone(),
            decision: rec.decision,
            mechanism_test: rec.mechanism_test,
        },
    );
    match activate(&w, &fa) {
        Err(LoopError::Refused(m)) => assert!(m.contains("does not re-derive"), "{m}"),
        other => panic!(
            "ATTACK: an admission record that does not re-derive from its plan and evaluation \
             activated: {other:?}"
        ),
    }
    activate(&w, &adm).expect("control: the genuine journalled ACCEPT activates");
}

/// M857 (and the four-cell record of M830 against it): evaluation re-checks
/// each delivered trial against its INTAKEN episode, which names its context
/// by digest. The episodes here were intaken with their genuine contexts;
/// the evaluation request then delivers one candidate trial with another
/// context (another `context_id`, nothing else). `intake_join` checks only the
/// episode's existence, so the context binding in `bind_episode` is the one
/// refusal: the trial does not count. Control: the genuine request counts it.
#[test]
fn a_trial_delivered_with_a_context_other_than_its_episodes_counts_nothing() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    intake_all(&w.s, &v);
    let mut forged = v.clone();
    let i = forged["trials"]
        .as_array()
        .unwrap()
        .iter()
        .position(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        .unwrap();
    forged["trials"][i]["context"]["context_id"] = json!("ctx-delivered-instead");
    let trial_id = forged["trials"][i]["episode"]["identity"]["trial_id"]
        .as_str()
        .unwrap()
        .to_string();
    let (rec, _) = axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&forged.to_string()).unwrap(),
    )
    .unwrap();
    let t = rec
        .arm_for_policy(&w.cand_ref)
        .unwrap()
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == trial_id)
        .unwrap()
        .clone();
    assert!(
        t.outcome == Outcome::Unknown,
        "ATTACK: a trial delivered with a context other than the one its intaken episode names \
         counted: {:?} {}",
        t.outcome,
        t.reason
    );
    assert!(t.reason.contains("byte mismatch"), "{}", t.reason);
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (rec, _) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    assert_eq!(
        rec.arm_for_policy(&w.cand_ref).unwrap().verified_pass,
        2,
        "control: the genuine request counts both candidate passes"
    );
}
