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

/// (candidate verified passes, assigned, decision, reasons, pointer moved?,
/// per-trial (outcome, reason)).
type Run = (
    u64,
    u64,
    Decision,
    Vec<String>,
    bool,
    Vec<(Outcome, String)>,
);

/// One experiment on a fresh world; `edit` launders the delivered trials.
fn run(exp: &str, edit: impl FnOnce(&mut Value)) -> Run {
    run_after(exp, false, edit)
}

/// [`run`], optionally intaking the GENUINE bundle before laundering it: the
/// store then holds honest intake records, so whatever refuses the laundered
/// delivery is evaluation's own re-check (evidence swapped after intake), not
/// intake refusing the laundered bytes.
fn run_after(exp: &str, genuine_intaken: bool, edit: impl FnOnce(&mut Value)) -> Run {
    let w = world();
    let before = axon_loop::pointer::load(&w.s, &scope()).unwrap();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    if genuine_intaken {
        assert!(
            intake_all(&w.s, &v).is_empty(),
            "the genuine bundle intakes"
        );
    }
    edit(&mut v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let outcomes = c
        .trials
        .iter()
        .map(|t| (t.outcome, first_refusal(t.trial_id.as_str(), &t.reason)))
        .collect();
    let (adm, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    let after = axon_loop::pointer::load(&w.s, &scope()).unwrap();
    (
        c.verified_pass,
        c.assigned,
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
            // Refused at intake since ADR-001 §8 (evaluation reads only what
            // intake admitted); evaluation's cross-tenant check is second line.
            _ => "intake: refused: bind: cross-scope episode",
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

/// G01-r22-independent-issuer through the whole chain: a verdict counts only if
/// its issuer AUTHENTICATED it. The candidate's trials keep every document
/// intact and name the trusted verifier; only the attestation is forged —
/// removed, signed by an impostor key claiming the verifier's name, or a
/// genuine attestation of ANOTHER trial's receipt replayed. Each trial is
/// Unknown for that reason, the candidate has no verified pass, admission does
/// not accept, the pointer does not move. An unattested FAILURE is Unknown too:
/// a forged failure cannot sink an arm.
///
/// The genuine bundle is intaken FIRST, so the forged attestation is swapped in
/// between intake and evaluation — only evaluation's own re-verification can
/// refuse it. Mutation: delete the authentication block in `evl::judge` → the
/// forged verdicts count and this fails.
#[test]
fn an_unauthenticated_verdict_never_counts() {
    let (impostor, _) = axon_loop_contracts::attestation::generate().unwrap();
    type Forge = Box<dyn Fn(&mut Value)>;
    let forgeries: Vec<(&str, &str, Forge)> = vec![
        (
            "attestation removed",
            "not authenticated",
            Box::new(|v| {
                for t in candidate_trials(v) {
                    t["verification_attestation"] = Value::Null;
                }
            }),
        ),
        (
            "impostor key claiming the trusted verifier",
            "not by",
            Box::new(move |v| {
                for t in candidate_trials(v) {
                    let req: ComputeRequest =
                        serde_json::from_value(t["verification_request"].clone()).unwrap();
                    let rc: ExecutionReceipt =
                        serde_json::from_value(t["verification_receipt"].clone()).unwrap();
                    t["verification_attestation"] = axon_loop_contracts::attestation::sign(
                        &impostor,
                        &OpaqueRef::new(common::VERIFIER).unwrap(),
                        &req,
                        &rc,
                        axon_loop::now_ms(),
                    )
                    .unwrap();
                }
            }),
        ),
        (
            "genuine attestation of another trial, replayed",
            "the evidence it must vouch for",
            Box::new(|v| {
                let donor = v["trials"][0]["verification_attestation"].clone();
                for t in candidate_trials(v) {
                    t["verification_attestation"] = donor.clone();
                }
            }),
        ),
    ];
    for (i, (why, reason_part, forge)) in forgeries.into_iter().enumerate() {
        let (pass, _, d, r, moved, outcomes) =
            run_after(&format!("unauth-{i}"), true, |v| forge(v));
        assert_eq!(pass, 0, "{why}: {outcomes:?}");
        assert!(
            outcomes.iter().all(|(o, reason)| *o == Outcome::Unknown
                && reason.contains("unauthenticated verification")
                && reason.contains(reason_part)),
            "{why}: {outcomes:?}"
        );
        assert_ne!(d, Decision::Accept, "{why}: {r:?}");
        assert!(!moved, "{why}");
    }

    // A forged FAILURE: the candidate's trials fail, unattested.
    let w = world();
    freeze_plan(&w.s, "unauth-fail", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    // cand_pass = 0: every candidate trial reports failure.
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    let mut v = evl_request("unauth-fail", &w.inc, &w.cand, &specs, &EvlOpts::default());
    for t in candidate_trials(&mut v) {
        t["verification_attestation"] = Value::Null;
    }
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert!(
        c.trials.iter().all(|t| t.outcome == Outcome::Unknown),
        "an unattested failure is not a failure: {:?}",
        c.trials
    );
}

/// The operator pins hold in EVL exactly as at intake (one shared join): the
/// candidate's verification ran another verifier REVISION, genuinely signed by
/// the trusted verifier, with the episode re-derived to cite it. Each such
/// trial is Unknown for that reason, and nothing is admitted.
/// Mutation: drop the pin block in `verify_check_evidence` → the verdicts
/// count and this fails.
#[test]
fn a_verdict_from_an_unpinned_verifier_revision_never_counts_in_evl() {
    let (pass, _, d, r, moved, outcomes) = run("unpinned-rev", |v| {
        for t in candidate_trials(v) {
            let mut req = t["verification_request"].clone();
            req["executable_digest"] = json!(format!("acf1:{}", "f".repeat(64)));
            let rc = t["verification_receipt"].clone();
            t["verification_attestation"] = common::attest(common::VERIFIER, &req, &rc);
            t["episode"]["verification"]["evidence_refs"] = json!([digest_value(&req).unwrap()]);
            t["verification_request"] = req;
        }
    });
    assert_eq!(pass, 0, "{outcomes:?}");
    assert!(
        outcomes
            .iter()
            .all(|(o, reason)| *o == Outcome::Unknown && reason.contains("verifier revision")),
        "{outcomes:?}"
    );
    assert_ne!(d, Decision::Accept, "{r:?}");
    assert!(!moved);
}

/// G01 re-audit 2: the evaluation record said a trial was "independently
/// verified" but not by WHAT — the signed verification it was authenticated
/// on was discarded. Each counted verdict (pass or fail) now cites the
/// request, receipt and attestation digests, the issuer and the key id, and
/// the cited attestation re-verifies against the cited documents; an Unknown
/// (here: an undelivered trial) cites nothing.
#[test]
fn a_counted_verdict_cites_the_evidence_it_was_authenticated_on() {
    let w = world();
    freeze_plan(&w.s, "cite", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 1, 2, Some(100), Some(50));
    let v = evl_request(
        "cite",
        &w.inc,
        &w.cand,
        &specs,
        &EvlOpts {
            deliver: Box::new(|t| t != "c1"),
            ..EvlOpts::default()
        },
    );
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let delivered = |trial: &str| {
        v["trials"]
            .as_array()
            .unwrap()
            .iter()
            .find(|t| t["episode"]["identity"]["trial_id"] == trial)
            .unwrap()
            .clone()
    };
    let pk = &verifier_key().1;
    let (mut pass, mut fail, mut unknown) = (0, 0, 0);
    for t in rec.arms.iter().flat_map(|a| &a.trials) {
        match t.outcome {
            Outcome::VerifiedPass | Outcome::Fail => {
                if t.outcome == Outcome::Fail {
                    fail += 1
                } else {
                    pass += 1
                }
                let ev = t.verification.as_ref().expect("a counted verdict cites");
                let d = delivered(t.trial_id.as_str());
                let (q, r, a) = (
                    &d["verification_request"],
                    &d["verification_receipt"],
                    &d["verification_attestation"],
                );
                assert_eq!(ev.request_ref, digest_value(q).unwrap());
                assert_eq!(ev.receipt_ref, digest_value(r).unwrap());
                assert_eq!(ev.attestation_ref, digest_value(a).unwrap());
                assert_eq!(ev.issuer_ref.as_str(), VERIFIER);
                let key_id = attestation::verify(
                    a,
                    &ev.issuer_ref,
                    &serde_json::from_value(q.clone()).unwrap(),
                    &serde_json::from_value(r.clone()).unwrap(),
                    pk,
                )
                .expect("the cited attestation re-verifies");
                assert_eq!(ev.key_id, key_id);
            }
            Outcome::Unknown => {
                unknown += 1;
                assert!(t.verification.is_none(), "{t:?}");
            }
        }
    }
    assert_eq!((pass, fail, unknown), (2, 1, 1));
}

/// ADR-001 §8 (architecture review wf_6c790b05): evaluation read the trials the
/// REQUEST carried, so an episode intake would refuse — or one never presented
/// to intake at all — was judged as if every intake check had passed. Now a
/// delivered episode counts only if intake recorded it in this scope. Here the
/// genuine bundle goes straight to evaluation: nothing counts. Positive
/// control: the same bundle after intake is two verified passes per arm.
///
/// Mutation: drop the `intake_join` arm in `evaluate` → the un-intaken
/// bundle is judged as verified passes and this fails.
#[test]
fn an_episode_intake_never_recorded_never_counts() {
    let w = world();
    freeze_plan(&w.s, "raw", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("raw", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assign_request(&w.s, &v);
    let (rec, e) = axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    )
    .unwrap();
    for arm in &rec.arms {
        assert_eq!(arm.verified_pass, 0, "{arm:?}");
        assert!(arm
            .trials
            .iter()
            .all(|t| t.outcome == Outcome::Unknown && t.reason.contains("not intaken")));
    }
    let (adm, _) = admit(&w.s, "raw", &e, ADMITTER, false).unwrap();
    assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);

    let w = world();
    freeze_plan(&w.s, "intaken", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let v = evl_request("intaken", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    let (rec, _) = axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    )
    .unwrap();
    assert!(
        rec.arms.iter().all(|a| a.verified_pass == 2),
        "{:?}",
        rec.arms
    );
}

/// The candidate trials' verification check re-run AS THE TRIAL'S OBSERVER
/// principal, genuinely re-signed by the trusted verifier, every ref re-derived.
fn checked_by_the_observer(v: &mut Value) {
    for t in candidate_trials(v) {
        let observer = t["context"]["observed_issuer_ref"].clone();
        t["verification_request"]["principal_ref"] = observer;
        t["episode"]["verification"]["evidence_refs"] =
            json!([digest_value(&t["verification_request"]).unwrap()]);
        t["verification_attestation"] = attest(
            VERIFIER,
            &t["verification_request"],
            &t["verification_receipt"],
        );
    }
}

/// G01 re-audit 3 (clause auditor, executed): EVL judged by a subject set
/// WITHOUT the trial's observer, while intake's includes it — so a check run
/// as the observer principal was refused at intake yet counted by EVL.
///
/// (a) Production order: intake refuses it, so it never counts.
/// (b) EVL on its own: the genuine bundle is intaken, then a store-level
///     writer forges intake records for the laundered episodes (bypassing
///     intake's checks). EVL must still refuse them by its own subject rule.
///     Mutation: drop the observer from EVL's per-trial subject set → red.
#[test]
fn a_check_run_as_the_observer_never_counts_at_either_door() {
    let (pass, _, d, _, _, outcomes) = run("obs-a", checked_by_the_observer);
    assert_eq!(pass, 0, "{outcomes:?}");
    assert_ne!(d, Decision::Accept);
    assert!(
        outcomes
            .iter()
            .all(|(_, r)| r.contains("intake: refused") && r.contains("subject")),
        "{outcomes:?}"
    );

    let w = world();
    freeze_plan(&w.s, "obs-b", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("obs-b", &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    let genuine: Vec<axon_loop::intake::IntakeRecord> = {
        let tx = axon_loop::ledger::Tx::begin(&w.s).unwrap();
        tx.entries()
            .iter()
            .filter_map(|e| match &e.event {
                axon_loop::ledger::Event::EpisodeIntake { intake, .. } => Some((**intake).clone()),
                _ => None,
            })
            .collect()
    };
    checked_by_the_observer(&mut v);
    let mut tx = axon_loop::ledger::Tx::begin(&w.s).unwrap();
    for t in candidate_trials(&mut v) {
        let ep: LoopEpisode = serde_json::from_value(t["episode"].clone()).unwrap();
        let mut forged = genuine
            .iter()
            .find(|r| r.identity.trial_id == ep.identity.trial_id)
            .unwrap()
            .clone();
        forged.episode_ref = digest(&ep).unwrap();
        tx.append(axon_loop::ledger::Event::EpisodeIntake {
            scope: scope(),
            intake: Box::new(forged),
        })
        .unwrap();
    }
    drop(tx);
    let (rec, _) = axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    )
    .unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert_eq!(c.verified_pass, 0, "{:?}", c.trials);
    assert!(
        c.trials
            .iter()
            .all(|t| t.outcome == Outcome::Unknown && t.reason.contains("subject")),
        "{:?}",
        c.trials
    );
}
