//! Amendment 64 (C9 round 4b, integrate-D): the plan/rules refusals that
//! rows4a exempted as dominated (another check refuses the same input first),
//! each now an attack on the production route (`plan::register` /
//! `plan::freeze`) that the four-cell retirement executes: with either check
//! alone the attack is refused, with both removed it freezes. Every test is
//! an ATTACK with its CONTROL (plan_sites.rs `the_complete_plan_freezes`).

mod common;
use axon_loop::candidates;
use axon_loop::plan::{self, PilotPlan};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn plan_of(w: &World, id: &str, cand: &Ref, edit: impl FnOnce(&mut Value)) -> Value {
    let mut v = complete_plan(id, &w.inc_ref, cand);
    v["task_manifest_ref"] = json!(register_tasks(&w.s, 2));
    edit(&mut v);
    v
}

fn register(w: &World, v: &Value) -> Result<Ref, axon_loop::error::LoopError> {
    plan::register(&w.s, &PilotPlan::from_value(v)?)
}

fn never_frozen(r: Result<Ref, axon_loop::error::LoopError>, why: &[&str], attack: &str) {
    match r {
        Ok(_) => panic!("ATTACK: {attack}: the plan froze"),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "{attack}: expected one of {why:?}: {e}"
        ),
    }
}

/// Four-cell (the freeze's re-registration check; register's refusal of a
/// frozen id, M1006): a frozen experiment whose plan is registered again is
/// never reported frozen under the new plan.
#[test]
fn a_re_registered_frozen_plan_is_never_frozen_under_its_new_plan() {
    let w = world();
    let first = plan_of(&w, "rr", &w.cand_ref, |_| {});
    register(&w, &first).unwrap();
    let frozen = plan::freeze(&w.s, "rr").unwrap();
    let second = plan_of(&w, "rr", &w.cand_ref, |v| {
        v["quality_margin"] = json!("pass_rate_margin_ppm=500000")
    });
    let why = ["is frozen", "re-registered after its freeze"];
    if let Err(e) = register(&w, &second) {
        assert!(why.iter().any(|y| e.to_string().contains(y)), "{e}");
        return;
    }
    match plan::freeze(&w.s, "rr") {
        Ok(r) if r != frozen => panic!(
            "ATTACK: a re-registered plan was reported frozen as {r}, not the frozen {frozen}"
        ),
        Ok(r) => panic!("re-registration did not change the plan ref ({r})"),
        Err(e) => assert!(why.iter().any(|y| e.to_string().contains(y)), "{e}"),
    }
}

/// Four-cell (check_candidate's authority_expansion refusal; the policy
/// parse (schema + typed validation) that get_contract applies; the EVO
/// proposer record, M1011): a hand-made candidate claiming an authority
/// expansion never freezes.
#[test]
fn a_candidate_claiming_an_authority_expansion_never_freezes() {
    let w = world();
    let mut p = w.inc.clone();
    p.policy_id = PolicyId::new("expand").unwrap();
    p.parent_policy_ref = w.inc_ref.clone();
    p.shortlist = vec![CandidateId::new("read").unwrap()];
    p.authority_expansion = true;
    let c = candidates::put_policy(&w.s, &p);
    let c = match c {
        Ok(c) => c,
        Err(e) => {
            assert!(e.to_string().contains("authority_expansion"), "{e}");
            return;
        }
    };
    let v = plan_of(&w, "expand", &c, |_| {});
    let r = register(&w, &v).and_then(|_| plan::freeze(&w.s, "expand"));
    never_frozen(
        r,
        &[
            "authority_expansion",
            "was not produced by EVO",
            "does not parse",
        ],
        "a candidate claiming an authority expansion",
    );
}

/// Four-cell (Rules::parse's unset word rule; the freeze's unset-field check,
/// M1008): a plan with no missing-data rule never freezes.
#[test]
fn a_plan_with_no_missing_data_rule_never_freezes() {
    let w = world();
    let v = plan_of(&w, "nomd", &w.cand_ref, |v| {
        v["missing_data_rule"] = Value::Null
    });
    let r = register(&w, &v).and_then(|_| plan::freeze(&w.s, "nomd"));
    never_frozen(
        r,
        &["operator fields unset", "missing_data_rule unset"],
        "a plan with no missing-data rule",
    );
}

/// Four-cell (PolicyEnvelope's typed rule; the checked-in schema's
/// `const false`): `axon-loop policy put` never stores a policy claiming an
/// authority expansion. Control: the same policy without the claim is stored.
#[test]
fn policy_put_never_stores_an_authority_expansion() {
    let d = tempfile::tempdir().unwrap();
    let _s = store_with_config(d.path());
    let put = |v: &Value| {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), v.to_string()).unwrap();
        let o = std::process::Command::new(env!("CARGO_BIN_EXE_axon-loop"))
            .env_remove("AXON_ATTEST_KEY")
            .arg("--store")
            .arg(d.path())
            .args(["policy", "put", "--in"])
            .arg(tmp.path())
            .output()
            .unwrap();
        (
            o.status.code().unwrap(),
            String::from_utf8_lossy(&o.stderr).into_owned(),
        )
    };
    let honest = serde_json::to_value(policy("other", &["read"])).unwrap();
    let (c, err) = put(&honest);
    assert_eq!(c, 0, "control: {err}");
    let mut bad = honest.clone();
    bad["policy_id"] = json!("other-expanding");
    bad["authority_expansion"] = json!(true);
    let (c, err) = put(&bad);
    if c == 0 {
        panic!("ATTACK: policy put stored a policy claiming an authority expansion");
    }
    assert!(err.contains("authority_expansion"), "{err}");
}
