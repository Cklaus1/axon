//! Regression tests for the red-team report on axon-loop-contracts @ 52665e4
//! (`.axon-v022/redteam/contracts.md`). One test per DEFECT row, named after
//! the row id; each reproduces the harness input exactly.
//!
//! D1 struct accepted as a positional array (S01 S02 S03 A1 A3)
//! D2 unit enum accepted as `{"variant":null}` (S04 S05 S06 A2)
//! D3 PinAck unit variant ignored extra fields (S07 A4)
//! D4 `-0` refused as a float although the reference reads it as 0 (P07 P07b)

use axon_loop_contracts::*;
use serde_json::{json, Value};
use std::path::PathBuf;

fn bundle() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/bundle.json");
    parse_value(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn m(k: &str) -> Value {
    bundle()[k].clone()
}

fn s(v: &Value) -> String {
    serde_json::to_string(v).unwrap()
}

fn refused<T: Contract + std::fmt::Debug>(json: &str) -> Refusal {
    match parse::<T>(json) {
        Ok(v) => panic!("ACCEPTED, expected refusal: {v:?}"),
        Err(e) => e,
    }
}

// ── D1: non-object where the schema says object ─────────────────────────────

#[test]
fn s01_scope_as_positional_array_is_refused() {
    let mut v = m("policy");
    v["scope"] = json!(["fixture-tenant", "fixture-coding"]);
    let e = refused::<PolicyEnvelope>(&s(&v));
    assert!(e.to_string().contains("scope"), "{e}");
}

#[test]
fn s02_episode_identity_as_positional_array_is_refused() {
    let mut v = m("episode");
    let i = v["identity"].clone();
    v["identity"] = json!([
        i["task_id"],
        i["arm_id"],
        i["trial_id"],
        i["attempt_id"],
        i["operation_id"],
        i["execution_id"]
    ]);
    refused::<LoopEpisode>(&s(&v));
}

#[test]
fn s03_whole_policy_as_positional_array_is_refused() {
    let p = m("policy");
    let arr = json!([
        p["schema"],
        p["policy_id"],
        p["parent_policy_ref"],
        [p["scope"]["tenant_id"], p["scope"]["task_family"]],
        p["mode"],
        p["candidate_set_ref"],
        p["controls_ref"],
        p["shortlist"],
        p["discovery_evidence_refs"],
        p["authority_expansion"]
    ]);
    // Before the fix this parsed AND digested equal to the object form: two
    // byte documents, one identity.
    refused::<PolicyEnvelope>(&s(&arr));
}

#[test]
fn a1_compute_request_required_as_array_is_refused() {
    let mut r = m("acf_request");
    let q = r["required"].clone();
    r["required"] = json!([
        q["engine"],
        q["hardware_isolation"],
        q["os"],
        q["architecture"],
        q["network_mode"],
        q["checkpoint_kind"]
    ]);
    refused::<ComputeRequest>(&s(&r));
}

#[test]
fn a3_context_observed_as_array_is_refused() {
    let mut r = m("context");
    let o = r["observed"].clone();
    let ks = [
        "repo_id",
        "base_commit",
        "workspace_ref",
        "branch",
        "worktree_id",
        "working_directory",
        "build_namespace",
        "model_ref",
        "role",
        "namespace_ref",
        "read_paths",
        "write_paths",
        "is_primary_worktree",
    ];
    r["observed"] = Value::Array(ks.iter().map(|k| o[*k].clone()).collect());
    refused::<ExecutionContextReceipt>(&s(&r));
}

#[test]
fn every_top_level_contract_refuses_an_array_document() {
    // A class sweep over every contract. NOTE: `values()` yields SORTED-key
    // order, not declaration order, so these arrays would mostly have failed
    // typed serde even before the fix (measured: this test passed on the
    // unfixed tree). The declaration-order witnesses that DID reproduce D1
    // are s01/s02/s03/a1/a3; this sweep only guards the object form still
    // parsing and no array form slipping through.
    for (key, f) in [
        ("policy", parse_policy as fn(&str) -> bool),
        ("context", parse_context),
        ("episode", parse_episode),
        ("transition", parse_transition),
        ("acf_request", parse_request),
        ("acf_receipt", parse_receipt),
        ("policy_projection", parse_projection),
    ] {
        let v = m(key);
        let arr = Value::Array(v.as_object().unwrap().values().cloned().collect());
        assert!(!f(&s(&arr)), "{key} accepted as an array");
        assert!(f(&s(&v)), "{key} object form must still parse");
    }
}

fn parse_policy(j: &str) -> bool {
    parse::<PolicyEnvelope>(j).is_ok()
}
fn parse_context(j: &str) -> bool {
    parse::<ExecutionContextReceipt>(j).is_ok()
}
fn parse_episode(j: &str) -> bool {
    parse::<LoopEpisode>(j).is_ok()
}
fn parse_transition(j: &str) -> bool {
    parse::<PolicyTransition>(j).is_ok()
}
fn parse_request(j: &str) -> bool {
    parse::<ComputeRequest>(j).is_ok()
}
fn parse_receipt(j: &str) -> bool {
    parse::<ExecutionReceipt>(j).is_ok()
}
fn parse_projection(j: &str) -> bool {
    parse::<PolicyProjection>(j).is_ok()
}

// ── D2: enum/const as an object ─────────────────────────────────────────────

#[test]
fn s04_transition_kind_as_object_is_refused() {
    let mut v = m("transition");
    v["kind"] = json!({"activate": null});
    refused::<PolicyTransition>(&s(&v));
}

#[test]
fn s05_episode_status_and_corpus_role_as_objects_are_refused() {
    let mut v = m("episode");
    v["status"] = json!({"completed": null});
    v["corpus_role"] = json!({"discovery": null});
    refused::<LoopEpisode>(&s(&v));
    // Each alone, too.
    let mut v = m("episode");
    v["corpus_role"] = json!({"mechanism_test": null});
    refused::<LoopEpisode>(&s(&v));
    let mut v = m("episode");
    v["verification"]["result"] = json!({"passed": null});
    refused::<LoopEpisode>(&s(&v));
}

#[test]
fn s06_policy_mode_as_object_is_refused() {
    let mut v = m("policy");
    v["mode"] = json!({"shortlist_only": null});
    refused::<PolicyEnvelope>(&s(&v));
}

#[test]
fn a2_receipt_status_as_object_is_refused() {
    let mut r = m("acf_receipt");
    r["status"] = json!({"completed": null});
    refused::<ExecutionReceipt>(&s(&r));
    let mut r = m("acf_request");
    r["required"]["engine"] = json!({"native_process": null});
    refused::<ComputeRequest>(&s(&r));
}

// ── D3: PinAck unit variant with extra fields ───────────────────────────────

const DIGEST0: &str = "cl22:0000000000000000000000000000000000000000000000000000000000000000";

fn pin(ack: Value) -> String {
    s(&json!({
        "version": {"policy_id": "p", "digest": DIGEST0},
        "epoch": 1,
        "pinned_at_ms": 5,
        "ack": ack
    }))
}

#[test]
fn s07_pin_ack_acknowledged_with_unknown_field_is_refused() {
    refused::<PolicyPin>(&pin(json!({"state": "acknowledged", "smuggled": true})));
}

#[test]
fn a4_pin_ack_acknowledged_carrying_a_reason_is_refused() {
    refused::<PolicyPin>(&pin(
        json!({"state": "acknowledged", "reason": "refused-actually"}),
    ));
}

#[test]
fn pin_ack_well_formed_variants_still_parse() {
    let p: PolicyPin = parse(&pin(json!({"state": "acknowledged"}))).unwrap();
    assert_eq!(p.ack, PinAck::Acknowledged);
    let p: PolicyPin = parse(&pin(json!({"state": "refused", "reason": "x"}))).unwrap();
    assert!(matches!(p.ack, PinAck::Refused { .. }));
    // S07b stays refused.
    refused::<PolicyPin>(&pin(
        json!({"state": "refused", "reason": "x", "smuggled": true}),
    ));
    refused::<PolicyPin>(&pin(json!({"state": "refused"})));
    refused::<PolicyPin>(&pin(json!("acknowledged")));
}

// ── D4: -0 is integer 0, as in the reference ────────────────────────────────

#[test]
fn p07_negative_zero_is_integer_zero() {
    // Reference: strict_json('-0') == 0 (int('-0') in bounded_int).
    let v = parse_value("-0").unwrap();
    assert_eq!(v, json!(0));
    assert_eq!(canonical_bytes(&v).unwrap(), b"0");
    let v = parse_value(r#"{"a":[-0,"-0",-0]}"#).unwrap();
    assert_eq!(v, json!({"a": [0, "-0", 0]}));
    // Floats spelled with a -0 prefix are still floats, and still refused.
    for f in ["-0.0", "-0e0", "-0E1", "[-0.5]"] {
        assert!(matches!(parse_value(f), Err(Refusal::Float)), "{f}");
    }
    // A leading-zero integer is still malformed JSON.
    assert!(parse_value("-01").is_err());
    // Inside strings nothing is rewritten, including after an escaped quote.
    assert_eq!(parse_value(r#""\"-0""#).unwrap(), json!("\"-0"));
}

#[test]
fn p07b_episode_authority_epoch_negative_zero_parses_as_zero() {
    let j = s(&m("episode")).replace("\"authority_epoch\":7", "\"authority_epoch\":-0");
    assert!(j.contains(":-0"));
    let ep: LoopEpisode = parse(&j).unwrap();
    assert_eq!(ep.authority_epoch.get(), 0);
}

// ── the schema walk covers every checked-in schema ─────────────────────────

#[test]
fn every_checked_in_schema_is_within_the_supported_subset() {
    // validate_against refuses an unsupported keyword or pattern only when it
    // meets it, so drive every schema against its own fixture (which visits
    // the conditional branches the fixture takes) AND statically scan every
    // node for keywords and patterns.
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("schemas");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.extension().and_then(|x| x.to_str()) != Some("json") {
            continue;
        }
        let schema: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
        scan(&schema, &p.display().to_string());
        n += 1;
    }
    assert_eq!(n, 9, "six package schemas + three crate-local");
}

fn scan(v: &Value, at: &str) {
    const OK: &[&str] = &[
        "$schema",
        "$id",
        "title",
        "description",
        "type",
        "properties",
        "additionalProperties",
        "required",
        "const",
        "enum",
        "pattern",
        "minLength",
        "maxLength",
        "minimum",
        "maximum",
        "items",
        "minItems",
        "maxItems",
        "uniqueItems",
        "anyOf",
        "oneOf",
        "allOf",
        "if",
        "then",
    ];
    const PATTERNS: &[&str] = &[
        "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$",
        "^(cl22|acf1|sha256):[0-9a-f]{64}$",
        "^acf1:[0-9a-f]{64}$",
        "^[A-Z]{3}$",
        "^[a-z][a-z0-9.-]{0,95}/v?(0|[1-9][0-9]{0,3})$",
    ];
    match v {
        Value::Object(o) => {
            for (k, x) in o {
                assert!(OK.contains(&k.as_str()), "{at}: keyword {k}");
                if k == "pattern" {
                    assert!(PATTERNS.contains(&x.as_str().unwrap()), "{at}: pattern {x}");
                }
                if k == "properties" {
                    for (pk, px) in x.as_object().unwrap() {
                        scan(px, &format!("{at}.{pk}"));
                    }
                } else if !matches!(k.as_str(), "const" | "enum" | "required") {
                    scan(x, &format!("{at}/{k}"));
                }
            }
        }
        Value::Array(a) => a.iter().for_each(|x| scan(x, at)),
        _ => {}
    }
}
