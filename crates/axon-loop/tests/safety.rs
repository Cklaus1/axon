//! ADR-001 §5: per-attempt trial safety and the admission veto. Each refusal
//! writes nothing; each veto has a positive control that differs only in the
//! finding, so the veto — not something else — is what decided.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::safety::{self, SafetyState, ViolationCode};
use common::*;
use serde_json::{json, Value};

const OBSERVER: &str = "fixture:observer";

/// A frozen dev experiment whose candidate would be ACCEPTED on quality and
/// economics: `(world, request)`.
fn acceptable(exp: &str) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    (w, v)
}

fn candidate_trial(v: &Value, i: usize) -> Value {
    v["trials"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        .nth(i)
        .unwrap()
        .clone()
}

fn decide(w: &World, exp: &str, v: &Value) -> (Decision, Vec<String>) {
    let (_, e) = evaluate(&w.s, v).unwrap();
    let (rec, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    (rec.decision, rec.reasons)
}

fn report(w: &World, r: &Value, sig: Option<&Value>) -> Result<(), LoopError> {
    safety::report(&w.s, &r.to_string(), sig.map(|s| s.to_string()).as_deref()).map(|_| ())
}

/// One unsafe candidate attempt VETOES the candidate, although quality and
/// economics would ACCEPT it; the veto is recorded as its own verdict and the
/// candidate cannot be activated. Positive control: the same experiment with
/// no finding is ACCEPTED.
///
/// Mutation: delete the veto stage in `admission::decide` → ACCEPT, red.
#[test]
fn a_candidate_violation_vetoes_before_any_utility() {
    let (w, v) = acceptable("ok");
    assert_eq!(decide(&w, "ok", &v).0, Decision::Accept);

    let (w, v) = acceptable("veto");
    trust_monitor(&w.s);
    let r = safety_report(
        &candidate_trial(&v, 0),
        "violation",
        Some("secret_exposure"),
        MONITOR,
    );
    report(&w, &r, None).unwrap();
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, adm) = admit(&w.s, "veto", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Vetoed, "{:?}", rec.reasons);
    assert!(
        rec.reasons.iter().all(|r| r.contains("safety veto")),
        "{:?}",
        rec.reasons
    );
    // Its own verdict in the hypothesis history, distinct from a Reject.
    assert!(axon_loop::evo::history(&w.s, &scope())
        .unwrap()
        .iter()
        .any(|h| matches!(
            h,
            axon_loop::evo::Hypothesis::Verdict {
                verdict: axon_loop::evo::Verdict::Vetoed,
                ..
            }
        )));
    // And it activates nothing.
    let before = snapshot(w.dir.path());
    let err = axon_loop::pointer::transition(
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
    assert!(matches!(err, LoopError::Refused(_)), "{err}");
    assert_eq!(snapshot(w.dir.path()), before);
}

/// Any party may incriminate its OWN arm: the trial's observer reports its
/// own trial unsafe, unsigned, and the candidate is vetoed.
#[test]
fn a_trials_own_subject_may_incriminate_it() {
    let (w, v) = acceptable("self");
    let r = safety_report(
        &candidate_trial(&v, 1),
        "violation",
        Some("scope_violation"),
        OBSERVER,
    );
    report(&w, &r, None).unwrap();
    assert_eq!(decide(&w, "self", &v).0, Decision::Vetoed);
}

/// Nobody else can veto by assertion: a stranger's violation report, a report
/// about a trial never intaken, and a violation with no code are refused and
/// write nothing.
#[test]
fn a_stranger_cannot_report_a_trial_unsafe() {
    let (w, v) = acceptable("stranger");
    let t = candidate_trial(&v, 0);
    let before = snapshot(w.dir.path());
    let e = report(
        &w,
        &safety_report(
            &t,
            "violation",
            Some("policy_violation"),
            "fixture:stranger",
        ),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("neither a trusted monitor nor a subject")),
        "{e}"
    );
    let mut ghost = t.clone();
    ghost["episode"]["identity"]["trial_id"] = json!("never-ran");
    let e = report(
        &w,
        &safety_report(&ghost, "violation", Some("policy_violation"), OBSERVER),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("no intaken trial")),
        "{e}"
    );
    let e = report(&w, &safety_report(&t, "violation", None, OBSERVER), None).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("exactly one code")),
        "{e}"
    );
    assert_eq!(snapshot(w.dir.path()), before);
}

/// A CLEARANCE counts only from a trusted monitor, independent of the trial,
/// signed under its registered key. Every other clearance is refused and
/// writes nothing; the genuine one is recorded.
#[test]
fn a_clearance_needs_an_independent_authenticated_monitor() {
    let (w, v) = acceptable("clear");
    trust_monitor(&w.s);
    let t = candidate_trial(&v, 0);
    let good = safety_report(&t, "clear", None, MONITOR);
    let before = snapshot(w.dir.path());
    let refused = |r: &Value, sig: Option<&Value>, want: &str| {
        let e = report(&w, r, sig).unwrap_err();
        assert!(
            matches!(e, LoopError::Refused(ref m) if m.contains(want)),
            "{want}: {e}"
        );
        assert_eq!(snapshot(w.dir.path()), before, "{want}");
    };
    refused(&good, None, "no monitor signature");
    let (impostor, _) = axon_loop_contracts::attestation::generate().unwrap();
    let forged = axon_loop_contracts::attestation::sign_document(
        &impostor,
        safety::CLEARANCE_DOMAIN,
        &axon_loop_contracts::OpaqueRef::new(MONITOR).unwrap(),
        &good,
    )
    .unwrap();
    refused(&good, Some(&forged), "not by");
    // A genuine signature over ANOTHER report (another trial) does not carry.
    let other = safety_report(&candidate_trial(&v, 1), "clear", None, MONITOR);
    refused(&good, Some(&monitor_sign(&other)), "doc_ref");
    // A signature made for another domain never passes as a clearance.
    let wrong_domain = axon_loop_contracts::attestation::sign_document(
        &monitor_key().0,
        "some.other/1",
        &axon_loop_contracts::OpaqueRef::new(MONITOR).unwrap(),
        &good,
    )
    .unwrap();
    refused(&good, Some(&wrong_domain), "domain");
    // Not a trusted monitor.
    let stranger = safety_report(&t, "clear", None, "fixture:stranger");
    refused(
        &stranger,
        Some(&monitor_sign(&stranger)),
        "independent of the trial",
    );
    // A subject of the trial cannot clear it, even as a trusted keyed monitor.
    let mut cfg = w.s.config().unwrap();
    let obs = axon_loop_contracts::OpaqueRef::new(OBSERVER).unwrap();
    cfg.trusted_monitors.push(obs.clone());
    cfg.monitor_keys.insert(obs, monitor_key().1.clone());
    w.s.write_config(&cfg).unwrap();
    let before = snapshot(w.dir.path());
    let own = safety_report(&t, "clear", None, OBSERVER);
    let sig = axon_loop_contracts::attestation::sign_document(
        &monitor_key().0,
        safety::CLEARANCE_DOMAIN,
        &axon_loop_contracts::OpaqueRef::new(OBSERVER).unwrap(),
        &own,
    )
    .unwrap();
    let e = report(&w, &own, Some(&sig)).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("independent of the trial")),
        "{e}"
    );
    assert_eq!(snapshot(w.dir.path()), before);
    // The genuine clearance is recorded.
    report(&w, &good, Some(&monitor_sign(&good))).unwrap();
}

/// A violation wins over a clearance whichever arrives first, and the state
/// is fixed into the evaluation: a finding recorded AFTER the evaluation does
/// not change the admission re-derived from it.
#[test]
fn a_violation_wins_and_the_state_is_fixed_at_evaluation() {
    let (w, v) = acceptable("order");
    trust_monitor(&w.s);
    let t = candidate_trial(&v, 0);
    let clear = safety_report(&t, "clear", None, MONITOR);
    report(&w, &clear, Some(&monitor_sign(&clear))).unwrap();
    let bad = safety_report(&t, "violation", Some("destructive_action"), MONITOR);
    report(&w, &bad, None).unwrap();
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let c0 = rec
        .arm_for_policy(&w.cand_ref)
        .unwrap()
        .trials
        .iter()
        .find(|x| x.trial_id.as_str() == "c0")
        .unwrap()
        .clone();
    assert_eq!(
        c0.safety,
        SafetyState::Violation {
            code: ViolationCode::DestructiveAction
        }
    );
    let (adm, _) = admit(&w.s, "order", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Vetoed);

    // The reverse order: a clearance recorded AFTER a violation does not
    // wash it out.
    let (w, v) = acceptable("order2");
    trust_monitor(&w.s);
    let t = candidate_trial(&v, 0);
    let bad = safety_report(&t, "violation", Some("oversight_evasion"), MONITOR);
    report(&w, &bad, None).unwrap();
    let clear = safety_report(&t, "clear", None, MONITOR);
    report(&w, &clear, Some(&monitor_sign(&clear))).unwrap();
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (adm, _) = admit(&w.s, "order2", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Vetoed, "{:?}", adm.reasons);

    // After the evaluation: c1's violation is not in it.
    let (w, v) = acceptable("late");
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let r = safety_report(
        &candidate_trial(&v, 1),
        "violation",
        Some("scope_violation"),
        OBSERVER,
    );
    report(&w, &r, None).unwrap();
    let (adm, _) = admit(&w.s, "late", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
}
