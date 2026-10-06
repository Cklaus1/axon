//! C9 round 4c, EQGATE (amendment 81; M1926-M1929): every conjunct of
//! `learning_eligible` is a guard on what may feed learning, and the gate could
//! not see the function (it returns `Result<bool, _>`): dropping the verification
//! or usage conjunct left axon-loop-contracts and axon-loop green, and the
//! corpus-role term was caught by one assert in a different test.
//!
//! Each test below starts from an episode that IS eligible (the control) and
//! changes exactly one thing; the function must say no.
//!
//! The `status == Completed` conjunct is NOT rowed, and this is stated rather
//! than hidden: `LoopEpisode::validate` (which `learning_eligible` runs first)
//! already refuses `verification passed` over any other status, so with the
//! `Passed` conjunct in place the status term can never be the deciding one.
//! `a_passed_verification_requires_a_completed_episode` pins THAT fact, so the
//! day validate stops saying it the redundancy is gone and this test fails.

use axon_loop_contracts::*;
use serde_json::Value;

fn episode(edit: impl FnOnce(&mut Value)) -> LoopEpisode {
    let b: Value = parse_value(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut e = b["episode"].clone();
    e["corpus_role"] = "discovery".into();
    edit(&mut e);
    parse::<LoopEpisode>(&serde_json::to_string(&e).unwrap()).expect("setup: the episode parses")
}

#[test]
fn a_completed_passed_final_discovery_episode_is_eligible() {
    assert!(learning_eligible(&episode(|_| {})).unwrap(), "control");
    assert!(
        learning_eligible(&episode(|e| e["corpus_role"] = "tuning".into())).unwrap(),
        "control: tuning is eligible too"
    );
}

#[test]
fn only_discovery_and_tuning_episodes_feed_learning() {
    for role in ["confirmation", "reporting"] {
        let got = learning_eligible(&episode(|e| e["corpus_role"] = role.into())).unwrap();
        assert!(
            !got,
            "ATTACK: a {role}-role episode was learning-eligible: protected outcomes would feed \
             the proposer"
        );
    }
}

#[test]
fn a_mechanism_test_episode_never_feeds_learning() {
    let got = learning_eligible(&episode(|e| e["corpus_role"] = "mechanism_test".into())).unwrap();
    assert!(
        !got,
        "ATTACK: a mechanism_test episode was learning-eligible"
    );
}

#[test]
fn an_episode_that_did_not_pass_verification_never_feeds_learning() {
    for result in ["failed", "not_run", "unknown"] {
        let got = learning_eligible(&episode(|e| e["verification"]["result"] = result.into()))
            .unwrap();
        assert!(
            !got,
            "ATTACK: a discovery episode whose verification is `{result}` was learning-eligible"
        );
    }
}

#[test]
fn an_episode_with_estimated_usage_never_feeds_learning() {
    for state in ["estimated", "unknown"] {
        let got = learning_eligible(&episode(|e| {
            e["usage"]["state"] = state.into();
            if state == "unknown" {
                e["usage"]["cost_micro"] = Value::Null;
            }
        }))
        .unwrap();
        assert!(
            !got,
            "ATTACK: a discovery episode with `{state}` usage was learning-eligible: estimated \
             cost is not final comparative evidence"
        );
    }
}

/// The fact the unrowed `status == Completed` conjunct is redundant by.
#[test]
fn a_passed_verification_requires_a_completed_episode() {
    let b: Value = parse_value(
        &std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let mut e = b["episode"].clone();
    e["corpus_role"] = "discovery".into();
    e["status"] = "failed".into();
    let got = parse::<LoopEpisode>(&serde_json::to_string(&e).unwrap());
    assert!(
        got.is_err(),
        "validate no longer refuses `verification passed` over a non-completed status, so \
         learning_eligible's `status == Completed` conjunct is no longer redundant: row it"
    );
}
