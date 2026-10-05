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
