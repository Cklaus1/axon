//! ADR-001 §5 typed UnknownKind: every delivered Unknown names WHY, from a
//! closed set; a trial never delivered is `missing`, not a kind; and every
//! arm satisfies assigned = pass + fail + Σ(unknown by kind) + missing.

mod common;
use axon_loop::evl::{Outcome, UnknownKind};
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

#[test]
fn each_unknown_names_its_kind_and_the_arm_adds_up() {
    let w = world();
    freeze_plan_n(&w.s, "kinds", &w.inc_ref, &w.cand_ref, 4, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 4, 4, 1, Some(100), Some(50));
    // c1: the check never reached a verdict (the run timed out).
    specs.iter_mut().find(|s| s.3 == "c1").unwrap().4 = Out::Unknown;
    let mut v = evl_request(
        "kinds",
        &w.inc,
        &w.cand,
        &specs,
        &EvlOpts {
            deliver: Box::new(|t| t != "c3"), // c3: never delivered
            ..EvlOpts::default()
        },
    );
    // Intake the genuine bundle, then swap c2's attestation out: delivered,
    // intaken, but no longer authenticated.
    let refused = intake_all(&w.s, &v);
    assert!(refused.is_empty(), "{refused:?}");
    trial_mut(&mut v, "c2")["verification_attestation"] = Value::Null;
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let kind = |t: &str| {
        let r = c.trials.iter().find(|x| x.trial_id.as_str() == t).unwrap();
        (r.outcome, r.unknown_kind)
    };
    assert_eq!(kind("c0"), (Outcome::VerifiedPass, None));
    assert_eq!(kind("c1"), (Outcome::Unknown, Some(UnknownKind::TimedOut)));
    assert_eq!(
        kind("c2"),
        (Outcome::Unknown, Some(UnknownKind::Unverifiable))
    );
    assert_eq!(
        kind("c3"),
        (Outcome::Unknown, None),
        "missing is not a kind"
    );
    assert_eq!(c.missing, 1);
    assert_eq!(
        c.unknown_kinds,
        [(UnknownKind::TimedOut, 1), (UnknownKind::Unverifiable, 1)]
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
    // The record says so on the wire.
    let wire = serde_json::to_value(&rec).unwrap();
    let arm = wire["arms"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["arm_id"] == "challenger-1")
        .unwrap();
    assert_eq!(
        arm["unknown_kinds"],
        json!({"timed_out": 1, "unverifiable": 1})
    );
}
