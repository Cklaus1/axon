//! C9 round 4c, ADMIT (amendment 76): the verdicts the loop PRODUCES. The
//! refusal-site gate now reads a refusal expressed as a returned verdict
//! (`Outcome::Fail`, `Decision::Vetoed`, ...); each test below is the attack on
//! one such site, everything else genuine, with a control proving the same
//! evaluation counts when it should. A panic that starts `ATTACK:` is the
//! attack getting through; nothing else is a kill.

mod common;
use axon_loop::evl::Outcome;
use common::*;

/// c0's and c1's outcomes in an evaluation where challenger trial `c0` is
/// delivered as `c0_out` (and `c0` is delivered at all when `deliver_c0`).
fn challenger(c0_out: Out, deliver_c0: bool) -> (Option<Outcome>, Outcome) {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    specs.iter_mut().find(|s| s.3 == "c0").unwrap().4 = c0_out;
    let v = evl_request(
        "exp",
        &w.inc,
        &w.cand,
        &specs,
        &EvlOpts {
            deliver: Box::new(move |t| deliver_c0 || t != "c0"),
            ..EvlOpts::default()
        },
    );
    let (rec, _) = evaluate(&w.s, &v).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    let of = |t: &str| c.trials.iter().find(|x| x.trial_id.as_str() == t);
    (of("c0").map(|t| t.outcome), of("c1").unwrap().outcome)
}

/// EVL: a verifier that reported FAILURE is a `Fail`, never a pass. Control: the
/// untouched sibling c1 (and an all-pass run) is a VerifiedPass.
#[test]
fn a_verifier_reported_failure_is_a_fail_never_a_pass() {
    let (c0, c1) = challenger(Out::Pass, true);
    assert_eq!(
        (c0, c1),
        (Some(Outcome::VerifiedPass), Outcome::VerifiedPass),
        "control: a genuine pass counts"
    );
    let (c0, c1) = challenger(Out::Fail, true);
    assert_eq!(
        c1,
        Outcome::VerifiedPass,
        "control: the sibling still counts"
    );
    if c0 == Some(Outcome::VerifiedPass) {
        panic!("ATTACK: a verifier-reported FAILURE was counted as a verified pass");
    }
    assert_eq!(c0, Some(Outcome::Fail), "the failure counts as a Fail");
}

/// EVL: a trial the Fabric never delivered an episode for is `Unknown`
/// (missing), never a pass. Control: delivered, the same trial passes.
#[test]
fn a_trial_with_no_delivered_episode_is_never_a_pass() {
    let (c0, _) = challenger(Out::Pass, true);
    assert_eq!(
        c0,
        Some(Outcome::VerifiedPass),
        "control: delivered, it counts"
    );
    let (c0, c1) = challenger(Out::Pass, false);
    assert_eq!(
        c1,
        Outcome::VerifiedPass,
        "control: the sibling still counts"
    );
    if c0 == Some(Outcome::VerifiedPass) {
        panic!("ATTACK: a trial no episode was delivered for was counted as a verified pass");
    }
    assert_eq!(c0, Some(Outcome::Unknown), "missing is Unknown");
}

// ── admission: the disposition `decide` returns and the verdict it is recorded as

use axon_loop::admission::Decision;
use axon_loop::evo::Verdict;
use axon_loop::safety;
use serde_json::Value;

/// A frozen dev experiment whose candidate would be ACCEPTED on quality and
/// economics, with trial specs the scenario then spoils.
fn scenario(exp: &str, cand_pass: usize, unknown_c0: bool) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, cand_pass, Some(100), Some(50));
    if unknown_c0 {
        specs[2].4 = Out::Unknown;
    }
    let v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    assert!(intake_all(&w.s, &v).is_empty());
    (w, v)
}

fn decided(w: &World, exp: &str, v: &Value) -> (Decision, Vec<Verdict>) {
    let (_, e) = evaluate(&w.s, v).unwrap();
    let (rec, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    let recorded = axon_loop::evo::history(&w.s, &scope())
        .unwrap()
        .into_iter()
        .filter_map(|h| match h {
            axon_loop::evo::Hypothesis::Verdict { verdict, .. } => Some(verdict),
            _ => None,
        })
        .collect();
    (rec.decision, recorded)
}

/// The three spoiled scenarios and the disposition each must get.
fn spoiled(which: &str) -> (Decision, Vec<Verdict>) {
    match which {
        // Quality inferiority is established: 0 of 2 against the incumbent's 2.
        "reject" => {
            let (w, v) = scenario("reject", 0, false);
            decided(&w, "reject", &v)
        }
        // One challenger trial reached no verdict: noninferiority cannot be established.
        "inconclusive" => {
            let (w, v) = scenario("inconclusive", 2, true);
            decided(&w, "inconclusive", &v)
        }
        // One unsafe attempt in the candidate's arm.
        "veto" => {
            let (w, v) = scenario("veto", 2, false);
            trust_monitor(&w.s);
            let t = v["trials"]
                .as_array()
                .unwrap()
                .iter()
                .find(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
                .unwrap()
                .clone();
            let r = safety_report(&t, "violation", Some("secret_exposure"), MONITOR);
            safety::report(&w.s, &r.to_string(), None).unwrap();
            decided(&w, "veto", &v)
        }
        other => panic!("setup: no scenario {other}"),
    }
}

/// A candidate with a safety violation is VETOED, whatever its quality and
/// cost (ADR-001 §5). Control: the same experiment with no finding is accepted.
#[test]
fn a_candidate_with_an_unsafe_attempt_is_never_accepted() {
    let (w, v) = scenario("ok", 2, false);
    assert_eq!(decided(&w, "ok", &v).0, Decision::Accept, "control");
    let (d, _) = spoiled("veto");
    if d == Decision::Accept {
        panic!("ATTACK: a candidate with an unsafe attempt was ACCEPTED");
    }
    assert_eq!(d, Decision::Vetoed);
}

/// A candidate inferior on quality is REJECTED.
#[test]
fn a_candidate_inferior_on_quality_is_never_accepted() {
    let (d, _) = spoiled("reject");
    if d == Decision::Accept {
        panic!("ATTACK: a candidate established inferior on quality was ACCEPTED");
    }
    assert_eq!(d, Decision::Reject);
}

/// A candidate whose noninferiority cannot be established is INCONCLUSIVE.
#[test]
fn a_candidate_whose_noninferiority_cannot_be_established_is_never_accepted() {
    let (d, _) = spoiled("inconclusive");
    if d == Decision::Accept {
        panic!("ATTACK: a candidate whose noninferiority could not be established was ACCEPTED");
    }
    assert_eq!(d, Decision::Inconclusive);
}

/// Each disposition is recorded in the hypothesis history as ITS OWN verdict:
/// a rejected, an inconclusive and a vetoed candidate are never recorded as
/// accepted (the history is what a later proposer reads).
#[test]
fn each_admission_disposition_is_recorded_as_its_own_verdict() {
    for (which, want, attack) in [
        (
            "reject",
            Verdict::Reject,
            "ATTACK: a REJECTED admission was recorded as an ACCEPT verdict",
        ),
        (
            "inconclusive",
            Verdict::Inconclusive,
            "ATTACK: an INCONCLUSIVE admission was recorded as an ACCEPT verdict",
        ),
        (
            "veto",
            Verdict::Vetoed,
            "ATTACK: a VETOED admission was recorded as an ACCEPT verdict",
        ),
    ] {
        let (_, recorded) = spoiled(which);
        if recorded.contains(&Verdict::Accept) {
            panic!("{attack}: {recorded:?}");
        }
        assert_eq!(recorded, vec![want], "{which}");
    }
}
