//! Cortex's defaults are the PROMOTED policy, and nothing else.
//!
//! `benchmarks/promotion/policy/cortex-policy.json` is the policy in force;
//! it changes only through `benchmarks/promotion/promote.py` (an `axon-loop`
//! admission over a frozen plan) and every change appends a record to
//! `benchmarks/promotion/decisions.jsonl`. These tests tie the two ends:
//!
//! * the policy file is byte-for-byte the one the last placing record put in
//!   place, so the file cannot be edited without a decision record;
//! * the compiled defaults (`rank::select(None, None)`,
//!   `audit::select(None, None)`) and the generator driver's default model
//!   equal the policy, so a default cannot be edited without the file.
//!
//! A promotion therefore lands as one commit that appends the decision,
//! rewrites the policy file and changes the default in code.

use axon_cortex::{audit, rank};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;

/// Decisions that put a policy in place (promote.py `PLACING`).
const PLACING: [&str; 3] = ["GRANDFATHERED", "PROMOTE", "ROLLBACK"];
const POLICY_KEYS: [&str; 5] = ["ranker", "audit", "walk", "model", "transport"];

fn promotion_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("benchmarks/promotion")
}

fn policy() -> (Vec<u8>, Value) {
    let p = promotion_dir().join("policy/cortex-policy.json");
    let bytes = std::fs::read(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let v: Value = serde_json::from_slice(&bytes).expect("policy is JSON");
    assert_eq!(v["schema"], "cortex-policy/1", "policy schema");
    let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
    for k in POLICY_KEYS {
        assert!(v[k].is_string(), "policy field `{k}` must be a string");
    }
    assert_eq!(keys.len(), POLICY_KEYS.len() + 1, "policy fields: {keys:?}");
    (bytes, v)
}

fn field<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap()
}

#[test]
fn policy_file_is_what_the_last_decision_placed() {
    let (bytes, _) = policy();
    let p = promotion_dir().join("decisions.jsonl");
    let text = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let last = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str::<Value>(l).expect("decision record is JSON"))
        .inspect(|r| assert_eq!(r["schema"], "cortex-promotion-decision/1"))
        .filter(|r| PLACING.contains(&r["decision"].as_str().unwrap_or("")))
        // Folded, not `next_back()`: every record must pass the schema check
        // above, not only the ones after the last placing decision.
        .fold(None, |_, r| Some(r))
        .expect("decisions.jsonl places no policy");
    let got = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        got, last["policy_after"]["sha256"],
        "policy/cortex-policy.json is not the policy the last decision ({} {}) put in place: \
         a policy changes only through `promote.py run --record` / `rollback --record`",
        last["decision"], last["experiment_id"]
    );
}

#[test]
fn compiled_ranker_default_is_the_promoted_ranker() {
    let (_, p) = policy();
    let compiled = rank::select(None, None).expect("default ranker").label();
    assert_eq!(
        compiled,
        field(&p, "ranker"),
        "rank.rs's default ranker differs from the promoted policy: a default changes only \
         with a PROMOTE record (benchmarks/promotion/README.md)"
    );
}

#[test]
fn compiled_audit_default_is_the_promoted_audit() {
    let (_, p) = policy();
    let compiled = audit::select(None, None).expect("default audit").label();
    assert_eq!(
        compiled,
        field(&p, "audit"),
        "audit.rs's default audit differs from the promoted policy: a default changes only \
         with a PROMOTE record (benchmarks/promotion/README.md)"
    );
}

#[test]
fn walk_is_the_promoted_walk() {
    // The binary has one walk, breadth-first (src/bin/cortex.rs: pass 1 gives
    // every candidate one proposal, pass 2 resumes each with the rest of its
    // budget), pinned behaviourally by tests/cli.rs C16b/C16c. A different
    // walk is a code change those tests see; this one sees the policy side.
    let (_, p) = policy();
    assert_eq!(
        field(&p, "walk"),
        "breadth",
        "policy walk vs the compiled walk"
    );
}

#[test]
fn transport_driver_default_model_is_the_promoted_model() {
    let (_, p) = policy();
    let driver = match field(&p, "transport") {
        "micode" => "micode.sh",
        "claude" => "claude_cli.sh",
        t => panic!("policy transport {t:?} has no driver under benchmarks/drivers"),
    };
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("benchmarks/drivers")
        .join(driver);
    let text = std::fs::read_to_string(&path).unwrap();
    let defaults: Vec<&str> = text
        .lines()
        .filter_map(|l| l.trim().strip_prefix("model=${CORTEX_MODEL:-"))
        .filter_map(|r| r.strip_suffix('}'))
        .collect();
    assert_eq!(
        defaults,
        [field(&p, "model")],
        "{driver}'s default generator model (model=${{CORTEX_MODEL:-...}}) differs from the \
         promoted policy"
    );
}
