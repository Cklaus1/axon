//! B269 Axon side: `axon-loop intake episode` over MiCode-SHAPED sidecars.
//!
//! The documents here are built the way `micode-persist::loop_sidecar::build`
//! and `active_policy::acknowledgement` build them (not_run verification,
//! estimated usage, `not_produced` markers for ACF refs, `projection_ref` =
//! the ack's `cl22:`). The real-bytes proof — MiCode's own binary writing
//! them — is `scripts/loop_interop_gate.sh`; these pin the refusal rules and
//! the unit conversion.

mod common;

use axon_loop::error::LoopError;
use axon_loop::intake::{
    cost_micro_from_micro_cents, intake_episode, micode_not_produced_ref, IntakeInput,
};
use axon_loop::store::Store;
use axon_loop_contracts::*;
use common::{snapshot, store_with_config};
use serde_json::{json, Value};

const CANDIDATES: &[&str] = &["bash", "edit", "grep", "read", "write"];

fn candidate_set_ref() -> Ref {
    digest_value(&json!(CANDIDATES)).unwrap()
}

fn scope_json() -> Value {
    json!({"tenant_id": "tenant-a", "task_family": "coding"})
}

fn policy(shortlist: &[&str]) -> PolicyEnvelope {
    parse(
        &json!({
            "schema": "axon.closed-loop.policy/1",
            "policy_id": "pol-dec-1",
            "parent_policy_ref": format!("cl22:{}", "0".repeat(64)),
            "scope": scope_json(),
            "mode": "shortlist_only",
            "candidate_set_ref": candidate_set_ref(),
            "controls_ref": format!("cl22:{}", "c".repeat(64)),
            "shortlist": shortlist,
            "discovery_evidence_refs": [],
            "authority_expansion": false,
        })
        .to_string(),
    )
    .unwrap()
}

fn identity() -> Value {
    json!({"task_id":"task-1","arm_id":"challenger-1","trial_id":"trial-1",
           "attempt_id":"attempt-1","operation_id":"op-1","execution_id":"exec-1"})
}

fn context(epoch: u64, observed_head: &str) -> Value {
    let facts = |base: &str| {
        json!({
            "repo_id": "fixture", "base_commit": base,
            "workspace_ref": format!("acf1:{}", "a".repeat(64)),
            "branch": "main", "worktree_id": "repo", "working_directory": "/w/repo",
            "build_namespace": "shared:default-target",
            "model_ref": format!("cl22:{}", "3".repeat(64)), "role": "implementation",
            "namespace_ref": format!("cl22:{}", "4".repeat(64)),
            "read_paths": [], "write_paths": ["src/**"], "is_primary_worktree": true,
        })
    };
    json!({
        "schema": "axon.closed-loop.context/1", "context_id": "ctx-1",
        "identity": identity(), "scope": scope_json(),
        "expected": facts("b".repeat(40).as_str()), "observed": facts(observed_head),
        "expected_issuer_ref": "fixture-harness", "observed_issuer_ref": "micode-host-observer",
        "observed_evidence_ref": format!("cl22:{}", "5".repeat(64)),
        "created_ms": 1_000, "expires_ms": 601_000, "authority_epoch": epoch,
    })
}

fn ack(p: &PolicyEnvelope) -> Value {
    json!({
        "schema": "micode.closed-loop.policy-ack/1",
        "pin": {"state": "pinned", "policy_id": p.policy_id, "policy_ref": digest(p).unwrap(),
                "controls_ref": p.controls_ref, "candidate_set_ref": p.candidate_set_ref,
                "shortlist": p.shortlist},
        "candidates": CANDIDATES,
        "candidate_set_ref": candidate_set_ref(),
    })
}

fn source_episode(micro_cents: u64) -> Value {
    json!({"episode_id": "ep-1", "cost": {"micro_cents": micro_cents, "tokens": 13,
           "turns": 1, "tool_calls": 0, "wall_clock_ms": 5}})
}

/// A MiCode-shaped sidecar for `p`, with `cost_micro` as MiCode's converter
/// would compute it from `micro_cents` (None = unpriced session).
fn sidecar(
    p: &PolicyEnvelope,
    ctx: &Value,
    ack: &Value,
    src: &Value,
    micro_cents: Option<u64>,
) -> Value {
    let cost = cost_micro_from_micro_cents(micro_cents);
    json!({
        "schema": "axon.closed-loop.episode/1",
        "identity": identity(), "scope": scope_json(),
        "policy_ref": digest(p).unwrap(), "controls_ref": p.controls_ref,
        "context_ref": digest_value(ctx).unwrap(), "candidate_set_ref": p.candidate_set_ref,
        "input_workspace_ref": ctx["observed"]["workspace_ref"],
        "output_workspace_ref": null,
        "acf_request_ref": micode_not_produced_ref("acf_request_ref"),
        "acf_receipt_ref": micode_not_produced_ref("acf_receipt_ref"),
        "source_episode_ref": digest_value(src).unwrap(),
        "authority_epoch": ctx["authority_epoch"],
        "status": "completed",
        "verification": {"result": "not_run", "matched_checks": 0, "issuer_ref": null,
                         "verifier_ref": null, "output_workspace_ref": null, "evidence_refs": []},
        "usage": {"state": if cost.is_some() {"estimated"} else {"unknown"}, "cost_micro": cost,
                  "unresolved_liability_micro": 0, "currency": "USD",
                  "price_schedule_ref": micode_not_produced_ref("price_schedule_ref"),
                  "attempt_refs": [digest_value(&json!({"attempt_id":"attempt-1"})).unwrap()]},
        "corpus_role": "mechanism_test",
        "data_use_ref": format!("cl22:{}", "d".repeat(64)),
        "projection_ref": digest_value(ack).unwrap(),
    })
}

struct Case {
    _dir: tempfile::TempDir,
    s: Store,
    p: PolicyEnvelope,
    ctx: Value,
    ack: Value,
    src: Value,
    ep: Value,
}

fn case(micro_cents: Option<u64>) -> Case {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config(dir.path());
    let p = policy(&["grep", "read"]);
    s.put_cas("policies", &p).unwrap();
    // Create the lock file up front so the no-change snapshots compare data.
    drop(axon_loop::ledger::Tx::begin(&s).unwrap());
    let ctx = context(0, &"b".repeat(40));
    let ack = ack(&p);
    let src = source_episode(micro_cents.unwrap_or(4_000));
    let ep = sidecar(&p, &ctx, &ack, &src, micro_cents);
    Case {
        _dir: dir,
        s,
        p,
        ctx,
        ack,
        src,
        ep,
    }
}

fn run(c: &Case, ep: &Value, src: bool) -> Result<axon_loop::intake::IntakeOutcome, LoopError> {
    intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            ack: &c.ack.to_string(),
            source_episode: src.then(|| c.src.to_string()).as_deref(),
        },
    )
}

#[test]
fn micro_cents_convert_to_cost_micro_rounding_up_and_unknown_stays_null() {
    assert_eq!(cost_micro_from_micro_cents(None), None);
    assert_eq!(cost_micro_from_micro_cents(Some(0)), Some(0));
    assert_eq!(cost_micro_from_micro_cents(Some(1)), Some(1));
    assert_eq!(cost_micro_from_micro_cents(Some(100)), Some(1));
    assert_eq!(cost_micro_from_micro_cents(Some(101)), Some(2));
    assert_eq!(
        cost_micro_from_micro_cents(Some(100_000_000)),
        Some(1_000_000)
    );
}

#[test]
fn a_micode_shaped_episode_is_recorded_once_with_its_corpus_role_and_cost() {
    // 12_345 micro-cents = 123.45 µUSD → 124 (rounded up, never down).
    let c = case(Some(12_345));
    let out = run(&c, &c.ep, true).unwrap();
    assert!(out.recorded_now);
    assert_eq!(out.record.cost_micro, Some(124));
    assert_eq!(out.record.corpus_role, CorpusRole::MechanismTest);
    assert_eq!(out.record.policy_ref, digest(&c.p).unwrap());
    assert!(out.record.source_episode_checked);
    // MiCode's own receipt runs in the primary checkout: recorded, not hidden.
    assert!(out
        .record
        .trial_profile_refusal
        .as_deref()
        .is_some_and(|r| r.contains("primary")));
    // Idempotent on the same bytes; nothing new written.
    let before = snapshot(c.s.root());
    let again = run(&c, &c.ep, true).unwrap();
    assert!(!again.recorded_now);
    assert_eq!(again.ledger_seq, out.ledger_seq);
    assert_eq!(before, snapshot(c.s.root()));
}

#[test]
fn an_unknown_cost_is_recorded_as_null_not_zero() {
    let c = case(None);
    let out = run(&c, &c.ep, true).unwrap();
    assert_eq!(out.record.cost_micro, None);
    assert_eq!(out.record.usage_state, UsageState::Unknown);
}

/// Mutation: round DOWN in the producer (123 instead of 124) → refused.
#[test]
fn a_cost_not_converted_by_the_round_up_rule_is_refused() {
    let c = case(Some(12_345));
    let mut ep = c.ep.clone();
    ep["usage"]["cost_micro"] = json!(123);
    let before = snapshot(c.s.root());
    let e = run(&c, &ep, true).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("unit conversion")),
        "{e}"
    );
    assert_eq!(before, snapshot(c.s.root()));
    // Treating MicroCents as µUSD (no conversion at all) is refused too.
    ep["usage"]["cost_micro"] = json!(12_345);
    assert!(run(&c, &ep, true).is_err());
}

/// Every refusal writes nothing to the store.
#[test]
fn tampered_unknown_or_unjoinable_episodes_are_refused_with_the_store_unchanged() {
    let c = case(Some(500));
    type Want = fn(&LoopError) -> bool;
    let mut cases: Vec<(&str, Value, Want)> = Vec::new();

    let mut v = c.ep.clone();
    v["policy_ref"] = json!(format!("cl22:{}", "e".repeat(64)));
    cases.push(("tampered policy_ref", v, |e| {
        matches!(e, LoopError::Refused(_))
    }));

    let mut v = c.ep.clone();
    v["surprise"] = json!(1);
    cases.push(("unknown field", v, |e| matches!(e, LoopError::Malformed(_))));

    let mut v = c.ep.clone();
    v["context_ref"] = json!(format!("cl22:{}", "f".repeat(64)));
    cases.push(("context_ref", v, |e| matches!(e, LoopError::Refused(_))));

    let mut v = c.ep.clone();
    v["authority_epoch"] = json!(3);
    cases.push(("stale epoch", v, |e| matches!(e, LoopError::Refused(_))));

    let mut v = c.ep.clone();
    v["status"] = json!("refused");
    cases.push(("refused status", v, |e| matches!(e, LoopError::Refused(_))));

    let mut v = c.ep.clone();
    v["policy_ref"] = json!(micode_not_produced_ref("policy_ref"));
    cases.push((
        "incumbent fallback",
        v,
        |e| matches!(e, LoopError::Refused(ref m) if m.contains("not-produced")),
    ));

    let mut v = c.ep.clone();
    v["projection_ref"] = json!(format!("cl22:{}", "9".repeat(64)));
    cases.push(("ack not the named one", v, |e| {
        matches!(e, LoopError::Refused(_))
    }));

    let mut v = c.ep.clone();
    v["usage"]["cost_micro"] = json!(0);
    v["usage"]["state"] = json!("unknown");
    cases.push(("unknown with 0", v, |e| {
        matches!(e, LoopError::Malformed(_))
    }));

    for (name, ep, want) in cases {
        let before = snapshot(c.s.root());
        let e = run(&c, &ep, true).expect_err(name);
        assert!(want(&e), "{name}: wrong refusal {e:?}");
        assert_eq!(before, snapshot(c.s.root()), "{name}: store changed");
    }

    // A duplicate key in the raw bytes is a strict-parse refusal.
    let raw = c.ep.to_string().replacen(
        "\"corpus_role\":\"mechanism_test\"",
        "\"corpus_role\":\"discovery\",\"corpus_role\":\"mechanism_test\"",
        1,
    );
    assert_ne!(raw, c.ep.to_string());
    let e = intake_episode(
        &c.s,
        &IntakeInput {
            episode: &raw,
            context: &c.ctx.to_string(),
            ack: &c.ack.to_string(),
            source_episode: None,
        },
    )
    .unwrap_err();
    assert!(matches!(e, LoopError::Malformed(_)), "{e:?}");
}

#[test]
fn an_unbound_context_is_task_not_started_not_an_episode() {
    let mut c = case(Some(500));
    c.ctx = context(0, &"a".repeat(40)); // observed head != expected base
    c.ep = sidecar(&c.p, &c.ctx, &c.ack, &c.src, Some(500));
    let e = run(&c, &c.ep, true).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("TASK_NOT_STARTED")),
        "{e}"
    );
}

#[test]
fn an_ack_that_pinned_a_different_shortlist_is_refused() {
    let mut c = case(Some(500));
    c.ack["pin"]["shortlist"] = json!(["read"]);
    c.ep = sidecar(&c.p, &c.ctx, &c.ack, &c.src, Some(500));
    let e = run(&c, &c.ep, true).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("shortlist")),
        "{e}"
    );
}

#[test]
fn a_policy_the_store_does_not_hold_is_refused() {
    let c = case(Some(500));
    // Re-point everything consistently at a policy the store never stored.
    let other = policy(&["read"]);
    let mut a = ack(&other);
    a["pin"]["policy_id"] = json!(other.policy_id);
    let ep = sidecar(&other, &c.ctx, &a, &c.src, Some(500));
    let e = intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            ack: &a.to_string(),
            source_episode: None,
        },
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("not a policy this store knows")),
        "{e}"
    );
}

#[test]
fn the_same_trial_with_different_bytes_is_a_conflict() {
    let c = case(Some(500));
    run(&c, &c.ep, false).unwrap();
    let mut ep = c.ep.clone();
    ep["corpus_role"] = json!("discovery");
    assert!(matches!(run(&c, &ep, false), Err(LoopError::Conflict(_))));
}
