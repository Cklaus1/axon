//! G01-r22-unknown-outcome under ADR-001 D12: timeout, cancellation, unmatched
//! checks, missing evidence and unverifiable output stay DISTINCT non-success
//! states for the trials the real bridge produces — episodes whose execution
//! ran under local MiCode authority (not-produced execution markers) and whose
//! acceptance check alone went through Fabric — and none of them, nor any D12
//! verdict, is ever counted as a pass.

mod common;
use axon_loop::error::LoopError;
use axon_loop::evl::{Outcome, UnknownKind};
use axon_loop::intake::micode_not_produced_ref;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn trial_mut<'a>(v: &'a mut Value, trial: &str) -> &'a mut Value {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["episode"]["identity"]["trial_id"] == trial)
        .unwrap()
}

/// What MiCode writes under D12: the execution documents are named by its
/// not-produced markers and none is delivered.
fn d12(t: &mut Value) {
    t["episode"]["acf_request_ref"] = json!(micode_not_produced_ref("acf_request_ref"));
    t["episode"]["acf_receipt_ref"] = json!(micode_not_produced_ref("acf_receipt_ref"));
    for k in ["acf_request", "acf_receipt", "projection"] {
        t[k] = Value::Null;
    }
}

/// The acceptance check reached no verdict, and the verifier SIGNED how it
/// ended. The episode cites that receipt with `unknown`, as MiCode does.
fn cited_unknown(t: &mut Value, status: &str, verification: &str, matched: Option<u64>) {
    let mut rc = t["verification_receipt"].clone();
    assert!(rc.is_object(), "start from a trial that cites a check");
    rc["status"] = json!(status);
    rc["verification"] = json!(verification);
    rc["matched_checks"] = json!(matched);
    if status != "completed" {
        rc["process_exit_code"] = Value::Null;
    }
    t["verification_attestation"] = attest(VERIFIER, &t["verification_request"], &rc);
    let v = &mut t["episode"]["verification"];
    v["result"] = json!("unknown");
    v["matched_checks"] = json!(matched.unwrap_or(0));
    v["verifier_ref"] = json!(digest_value(&rc).unwrap());
    t["verification_receipt"] = rc;
}

/// No check was cited at all.
fn uncited(t: &mut Value, status: &str, result: &str) {
    t["episode"]["status"] = json!(status);
    let v = &mut t["episode"]["verification"];
    v["result"] = json!(result);
    v["matched_checks"] = json!(0);
    v["verifier_ref"] = Value::Null;
    v["evidence_refs"] = json!([]);
    for k in [
        "verification_request",
        "verification_receipt",
        "verification_attestation",
    ] {
        t[k] = Value::Null;
    }
}

#[test]
fn every_d12_non_success_keeps_its_kind_and_nothing_passes() {
    let w = world();
    freeze_plan_n(&w.s, "d12", &w.inc_ref, &w.cand_ref, 10, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 10, 10, 10, Some(100), Some(50));
    specs.iter_mut().find(|s| s.3 == "c1").unwrap().4 = Out::Fail;
    let mut v = evl_request("d12", &w.inc, &w.cand, &specs, &EvlOpts::default());
    for i in 0..10 {
        d12(trial_mut(&mut v, &format!("c{i}")));
    }
    // c0: an authenticated D12 pass; c1: an authenticated D12 fail.
    cited_unknown(trial_mut(&mut v, "c2"), "timed_out", "unknown", None);
    cited_unknown(trial_mut(&mut v, "c3"), "canceled", "unknown", None);
    cited_unknown(trial_mut(&mut v, "c4"), "completed", "not_run", Some(0));
    cited_unknown(trial_mut(&mut v, "c5"), "timed_out", "unknown", None);
    cited_unknown(trial_mut(&mut v, "c6"), "timed_out", "unknown", None);
    uncited(trial_mut(&mut v, "c7"), "cancelled", "not_run");
    uncited(trial_mut(&mut v, "c8"), "completed", "not_run");
    uncited(trial_mut(&mut v, "c9"), "outcome_unknown", "unknown");
    let refused = intake_all(&w.s, &v);
    assert!(refused.is_empty(), "{refused:?}");
    // Intaken genuinely; then c5's check documents are not delivered, and
    // c6's attestation is swapped for a signature over another receipt.
    for k in ["verification_request", "verification_receipt"] {
        trial_mut(&mut v, "c5")[k] = Value::Null;
    }
    let other = trial_mut(&mut v, "c2")["verification_attestation"].clone();
    trial_mut(&mut v, "c6")["verification_attestation"] = other;

    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let kind = |t: &str| {
        let r = c.trials.iter().find(|x| x.trial_id.as_str() == t).unwrap();
        (r.outcome, r.unknown_kind, r.reason.clone())
    };
    for (t, want, why) in [
        ("c0", UnknownKind::Unbound, "D12"),
        ("c1", UnknownKind::Unbound, "D12"),
        ("c2", UnknownKind::TimedOut, "TimedOut"),
        ("c3", UnknownKind::Cancelled, "Canceled"),
        ("c4", UnknownKind::Unmatched, "0 matched"),
        ("c5", UnknownKind::MissingEvidence, "not delivered"),
        ("c6", UnknownKind::Unverifiable, "unauthenticated"),
        ("c7", UnknownKind::Cancelled, "not run"),
        ("c8", UnknownKind::NotRun, "not run"),
        ("c9", UnknownKind::MissingEvidence, "unknown"),
    ] {
        let (o, k, reason) = kind(t);
        assert_eq!((o, k), (Outcome::Unknown, Some(want)), "{t}: {reason}");
        assert!(reason.contains(why), "{t}: {reason}");
    }
    // An authenticated D12 verdict is demoted, so it cites no evidence as if
    // it had counted.
    for t in ["c0", "c1"] {
        let r = c.trials.iter().find(|x| x.trial_id.as_str() == t).unwrap();
        assert!(r.verification.is_none(), "{t}: {:?}", r.verification);
    }
    // No default pass, and a D12 failure does not count either.
    assert_eq!((c.verified_pass, c.fail), (0, 0), "{c:?}");
    assert_eq!(
        c.unknown_kinds,
        [
            (UnknownKind::Unbound, 2),
            (UnknownKind::TimedOut, 1),
            (UnknownKind::Cancelled, 2),
            (UnknownKind::Unmatched, 1),
            (UnknownKind::MissingEvidence, 2),
            (UnknownKind::Unverifiable, 1),
            (UnknownKind::NotRun, 1),
        ]
        .into_iter()
        .collect()
    );
    for a in &rec.arms {
        let kinds: u64 = a.unknown_kinds.values().sum();
        assert_eq!(
            a.assigned,
            a.verified_pass + a.fail + kinds + a.missing,
            "{a:?}"
        );
    }
    // The genuine, Fabric-bound incumbent still counts: the D12 rule is not a
    // blanket refusal.
    let i = rec.arm_for_policy(&w.inc_ref).unwrap();
    assert_eq!(i.verified_pass, 10, "{i:?}");
}

/// A signed timed-out CHECK is a timeout even when the execution completed:
/// the kind comes from the receipt the verifier signed, not from the
/// subject's execution receipt (which says completed).
#[test]
fn a_cited_unknown_takes_its_kind_from_the_signed_check() {
    let w = world();
    freeze_plan(&w.s, "cited", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("cited", &w.inc, &w.cand, &specs, &EvlOpts::default());
    cited_unknown(trial_mut(&mut v, "c0"), "timed_out", "unknown", None);
    cited_unknown(trial_mut(&mut v, "c1"), "completed", "not_run", Some(0));
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let k = |t: &str| {
        c.trials
            .iter()
            .find(|x| x.trial_id.as_str() == t)
            .unwrap()
            .unknown_kind
    };
    assert_eq!(k("c0"), Some(UnknownKind::TimedOut));
    assert_eq!(k("c1"), Some(UnknownKind::Unmatched));
}

/// A D12 episode names no execution documents; a request that delivers some
/// anyway is a contradiction and is refused whole, with nothing recorded.
#[test]
fn a_d12_trial_with_execution_documents_is_refused() {
    let w = world();
    freeze_plan(&w.s, "mixed", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("mixed", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let t = trial_mut(&mut v, "c0");
    let receipt = t["acf_receipt"].clone();
    d12(t);
    t["acf_receipt"] = receipt;
    intake_all(&w.s, &v);
    let before = snapshot(w.dir.path());
    match axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    ) {
        Err(LoopError::Refused(m)) => assert!(m.contains("must be null"), "{m}"),
        other => panic!("a D12 trial carried execution documents: {other:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
}

/// ADR-001 D3: a D12 trial ran on no Fabric backend at all, so a protected
/// evaluation never counts it — even with its CHECK on the protected backend.
#[test]
fn a_protected_evaluation_never_counts_a_d12_trial() {
    let w = world();
    let mut cfg = w.s.config().unwrap();
    cfg.protected_scopes.push(scope());
    cfg.verifier_pins
        .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
        .unwrap()
        .backend_profiles
        .push("linux-microvm-protected".into());
    w.s.write_config(&cfg).unwrap();
    freeze_plan(&w.s, "prot", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let mut v = evl_request("prot", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let t = trial_mut(&mut v, "c0");
    d12(t);
    t["verification_receipt"]["backend_profile_ref"] = json!("linux-microvm-protected");
    t["episode"]["verification"]["verifier_ref"] =
        json!(digest_value(&t["verification_receipt"]).unwrap());
    t["verification_attestation"] = attest(
        VERIFIER,
        &t["verification_request"],
        &t["verification_receipt"],
    );
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let r = c
        .trials
        .iter()
        .find(|x| x.trial_id.as_str() == "c0")
        .unwrap();
    assert_eq!(
        (r.outcome, r.unknown_kind),
        (Outcome::Unknown, Some(UnknownKind::Unverifiable)),
        "{}",
        r.reason
    );
    assert!(r.reason.contains("D12 local execution"), "{}", r.reason);
}

/// A run that ended cancelled has no counted verdict, even an authenticated
/// failure of its output: the interruption is the outcome (non-D12, so the
/// verdict would otherwise count as a Fail).
#[test]
fn a_cancelled_run_does_not_count_its_failed_check() {
    let w = world();
    freeze_plan(&w.s, "cancel", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    specs.iter_mut().find(|s| s.3 == "c0").unwrap().4 = Out::Fail;
    let mut v = evl_request("cancel", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let t = trial_mut(&mut v, "c0");
    t["episode"]["status"] = json!("cancelled");
    t["acf_receipt"]["status"] = json!("canceled");
    t["acf_receipt"]["process_exit_code"] = Value::Null;
    t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let r = c
        .trials
        .iter()
        .find(|x| x.trial_id.as_str() == "c0")
        .unwrap();
    assert_eq!(
        (r.outcome, r.unknown_kind),
        (Outcome::Unknown, Some(UnknownKind::Cancelled)),
        "{}",
        r.reason
    );
    assert_eq!(c.fail, 0, "{c:?}");
}
