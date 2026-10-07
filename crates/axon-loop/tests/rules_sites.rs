//! Amendment 98 (C9 round 9, eqgate5): `rules.rs`'s refusal of an ABSENT rule
//! field, judged on `Rules::parse` itself. `freeze` refuses an unset plan
//! through `unset_fields` before it ever parses the rules, so this refusal is
//! the SECOND line (admission re-parses a plan it loads from the store); with
//! only the first line tested, replacing `p.independent_units.ok_or(..)?` by
//! `.unwrap_or(0)` left the whole suite green. That edit is FAIL-OPEN: a
//! rules document that omits the field would carry no minimum sample of
//! distinct tasks. Every test here is an ATTACK with its CONTROL.

mod common;
use axon_loop::plan::PilotPlan;
use axon_loop::rules::Rules;
use common::*;
use serde_json::{json, Value};

fn plan(edit: impl FnOnce(&mut Value)) -> PilotPlan {
    let w = world();
    let mut v = complete_plan("rules-sites", &w.inc_ref, &w.cand_ref);
    edit(&mut v);
    PilotPlan::from_value(&v).unwrap()
}

/// CONTROL: the complete plan parses, with the fields it carries.
#[test]
fn the_complete_plan_parses_into_the_rules_it_names() {
    let r = Rules::parse(&plan(|_| {})).unwrap();
    assert_eq!(r.independent_units, 2);
    assert_eq!(r.candidate_budget, 1);
}

/// A rules document that omits the minimum number of independent units is
/// refused, not read as "no minimum" (0).
#[test]
fn a_plan_without_independent_units_yields_no_rules() {
    let got = Rules::parse(&plan(|v| v["independent_units"] = Value::Null));
    assert!(
        got.is_err(),
        "ATTACK: a plan with no independent_units was read as rules with no minimum sample: {got:?}"
    );
    assert!(got.unwrap_err().contains("independent_units unset"));
}

/// Same for the candidate budget (0 would be fail-closed at admission, but
/// the refusal is its own decision and says what is missing).
#[test]
fn a_plan_without_a_candidate_budget_yields_no_rules() {
    let got = Rules::parse(&plan(|v| v["candidate_budget"] = Value::Null));
    assert!(
        got.is_err(),
        "ATTACK: a plan with no candidate_budget was read as rules with a budget of 0: {got:?}"
    );
    assert!(got.unwrap_err().contains("candidate_budget unset"));
}

/// An unset keyed rule is refused by name, never parsed as an empty string.
#[test]
fn a_plan_with_an_unset_keyed_rule_names_the_field() {
    for field in ["quality_margin", "budget_rule"] {
        let got = Rules::parse(&plan(|v| v[field] = Value::Null));
        let e = got.expect_err(&format!("ATTACK: a plan with {field} unset parsed"));
        assert!(
            e.contains(&format!("{field} unset")),
            "ATTACK: an unset {field} was refused as something else: {e}"
        );
    }
    // CONTROL: an unset keyed rule is not the same as a malformed one.
    let e = Rules::parse(&plan(|v| {
        v["quality_margin"] = json!("pass_rate_margin_ppm=05")
    }))
    .unwrap_err();
    assert!(e.contains("not executable"), "{e}");
}
