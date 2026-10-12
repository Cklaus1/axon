//! Amendment 64 (C9 round 4b, integrate-D): `parse` applies the contract's
//! TYPED rules after the checked-in schema, and the one rule no schema can
//! state (a transition's fence is contiguous: next_epoch = expected_epoch + 1,
//! an arithmetic relation between two fields) is refused by it. This is the
//! library primitive every reader of a contract document calls (the loop's
//! CLI, its store's get_contract, contract_from_value); the pointer's own
//! epoch checks re-judge a transition (pointer.rs), so the production route
//! reaches this rule only behind them. Control: the fixture transition parses.

use axon_loop_contracts::*;
use serde_json::{json, Value};

fn transition() -> Value {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle.json"),
    )
    .unwrap();
    parse_value(&text).unwrap()["transition"].clone()
}

#[test]
fn parse_never_admits_a_noncontiguous_fence() {
    let t = transition();
    assert!(parse::<PolicyTransition>(&t.to_string()).is_ok(), "control");
    let mut bad = t.clone();
    let expected = bad["expected_epoch"].as_u64().unwrap();
    bad["next_epoch"] = json!(expected + 2);
    match parse::<PolicyTransition>(&bad.to_string()) {
        Ok(_) => panic!("ATTACK: parse admitted a transition whose fence skips an epoch"),
        Err(e) => assert!(e.to_string().contains("noncontiguous fence"), "{e}"),
    }
}

/// PolicyEnvelope's typed rule holds for a value built in code (the schema
/// walk judges only parsed text): `validate` never accepts a policy claiming
/// an authority expansion. Control: the fixture policy validates.
#[test]
fn a_policy_built_in_code_claiming_an_expansion_never_validates() {
    let text = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle.json"),
    )
    .unwrap();
    let mut p: PolicyEnvelope = parse(&parse_value(&text).unwrap()["policy"].to_string()).unwrap();
    assert!(p.validate().is_ok(), "control");
    p.authority_expansion = true;
    if p.validate().is_ok() {
        panic!("ATTACK: a policy built in code claiming an authority expansion validated");
    }
}
