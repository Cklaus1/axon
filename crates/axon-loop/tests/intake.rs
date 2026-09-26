//! B269 Axon side: `axon-loop intake episode` over MiCode-SHAPED sidecars.
//!
//! The documents here are built the way `micode-persist::loop_sidecar::build`
//! and `active_policy::acknowledgement` build them (not_run verification,
//! estimated usage, `not_produced` markers for ACF refs, `projection_ref` =
//! null — the ack is joined by content, G6). The real-bytes proof — MiCode's own binary writing
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

/// A canonical MiCode episode as MiCode writes it now: the legacy `micro_cents`
/// default beside the explicit `spend_micro_cents` (`null` = unknown).
fn source_episode(spend: Option<u64>) -> Value {
    json!({"episode_id": "ep-1", "cost": {"micro_cents": 0, "spend_micro_cents": spend,
           "tokens": 13, "turns": 1, "tool_calls": 0, "wall_clock_ms": 5}})
}

/// A MiCode-shaped sidecar for `p`, with `cost_micro` as MiCode's converter
/// would compute it from `micro_cents` (None = unpriced session).
fn sidecar(
    p: &PolicyEnvelope,
    ctx: &Value,
    _ack: &Value,
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
        "projection_ref": null,
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

use common::verifier_key;

fn attest(sk: &[u8], issuer: &str, req: &Value, rc: &Value) -> Value {
    axon_loop_contracts::attestation::sign(
        sk,
        &OpaqueRef::new(issuer).unwrap(),
        &serde_json::from_value(req.clone()).unwrap(),
        &serde_json::from_value(rc.clone()).unwrap(),
    )
    .unwrap()
}

fn case(micro_cents: Option<u64>) -> Case {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config(dir.path());
    let mut config = s.config().unwrap();
    config.verifier_keys.insert(
        OpaqueRef::new(common::VERIFIER).unwrap(),
        verifier_key().1.clone(),
    );
    s.write_config(&config).unwrap();
    let names: Vec<CandidateId> = CANDIDATES
        .iter()
        .map(|c| CandidateId::new(*c).unwrap())
        .collect();
    let cs = axon_loop::candidates::CandidateSet::parse(
        &json!({"schema":"axon.loop.candidate-set/1","scope":scope_json(),"candidates":names,"issuer_ref":common::ADMITTER}).to_string(),
    )
    .unwrap();
    assert_eq!(
        axon_loop::candidates::put(&s, &cs).unwrap(),
        candidate_set_ref()
    );
    let p = policy(&["grep", "read"]);
    axon_loop::candidates::put_policy(&s, &p).unwrap();
    // Create the lock file up front so the no-change snapshots compare data.
    drop(axon_loop::ledger::Tx::begin(&s).unwrap());
    let ctx = context(0, &"b".repeat(40));
    let ack = ack(&p);
    let src = source_episode(micro_cents);
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
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: src.then(|| c.src.to_string()).as_deref(),
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
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

    // G6: projection_ref means a PolicyProjection. A non-null ref with no
    // projection presented is refused (it is never read as "the ack").
    let mut v = c.ep.clone();
    v["projection_ref"] = json!(format!("cl22:{}", "9".repeat(64)));
    cases.push((
        "projection_ref with no projection",
        v,
        |e| matches!(e, LoopError::Refused(ref m) if m.contains("PolicyProjection")),
    ));

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
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: None,
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
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
            acks: &[a.to_string()],
            projection: None,
            source_episode: None,
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
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

/// G1: a canonical episode that records ZERO spend against a metered sidecar
/// is still refused (never repaired), and the message names the producer gap.
#[test]
fn g1_zero_canonical_cost_is_refused_with_a_clear_reason() {
    let c = case(Some(7_500));
    // A canonical episode written before the spend producer: v014's default 0,
    // no spend field — no KNOWN spend, never a zero one.
    let src = json!({"episode_id": "ep-1", "cost": {"micro_cents": 0, "tokens": 13,
                     "turns": 1, "tool_calls": 0, "wall_clock_ms": 5}});
    let mut ep = c.ep.clone();
    ep["source_episode_ref"] = json!(digest_value(&src).unwrap());
    let before = snapshot(c.s.root());
    let e = intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: Some(&src.to_string()),
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
        },
    )
    .unwrap_err();
    let m = e.to_string();
    assert!(matches!(e, LoopError::Refused(_)), "{m}");
    assert!(
        m.contains("unit conversion")
            && m.contains("G1")
            && m.contains("NO known spend")
            && m.contains("nothing recorded"),
        "{m}"
    );
    assert_eq!(snapshot(c.s.root()), before);
}

fn run_with(
    c: &Case,
    acks: &[String],
    projection: Option<&str>,
    ep: &Value,
) -> Result<axon_loop::intake::IntakeOutcome, LoopError> {
    intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            acks,
            projection,
            source_episode: None,
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
        },
    )
}

/// G6: the ack is found by CONTENT among the presented files — unrelated acks
/// and non-ack files are ignored; the same ack twice is fine.
#[test]
fn g6_ack_is_selected_by_content() {
    let c = case(Some(500));
    let mut other = ack(&policy(&["read"]));
    other["pin"]["policy_ref"] = json!(format!("cl22:{}", "7".repeat(64)));
    let acks = vec![
        "{\"not\":\"an ack\"}".to_string(),
        other.to_string(),
        c.ack.to_string(),
        c.ack.to_string(),
    ];
    let out = run_with(&c, &acks, None, &c.ep).unwrap();
    assert_eq!(out.record.ack_ref, digest_value(&c.ack).unwrap());
}

/// G6: zero matching acks ⇒ "no ack"; two DIFFERENT matching acks ⇒
/// "ambiguous ack". Both refusals leave the store unchanged.
#[test]
fn g6_no_or_ambiguous_ack_is_refused() {
    let c = case(Some(500));
    let before = snapshot(c.s.root());
    let e = run_with(&c, &[], None, &c.ep).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("no ack")),
        "{e}"
    );
    let mut twin = c.ack.clone();
    twin["pin"]["shortlist"] = json!(["read"]);
    let e = run_with(&c, &[c.ack.to_string(), twin.to_string()], None, &c.ep).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("ambiguous ack")),
        "{e}"
    );
    // a matching but malformed ack (extra field) is refused, not skipped
    let mut bad = c.ack.clone();
    bad["extra"] = json!(1);
    assert!(run_with(&c, &[bad.to_string()], None, &c.ep).is_err());
    assert_eq!(before, snapshot(c.s.root()));
}

/// G6: a non-null projection_ref must be a real PolicyProjection over THIS
/// episode's policy; a matching one is accepted.
#[test]
fn g6_non_null_projection_ref_is_validated() {
    let c = case(Some(500));
    let proj = |sidecar: &Ref| {
        json!({"sidecar_policy_ref": sidecar,
               "acf_policy_digest": format!("acf1:{}", "c".repeat(64)),
               "projection_ref": format!("cl22:{}", "9".repeat(64))})
    };
    let good = proj(&digest(&c.p).unwrap());
    let mut ep = c.ep.clone();
    ep["projection_ref"] = json!(digest_value(&good).unwrap());
    let acks = [c.ack.to_string()];
    let before = snapshot(c.s.root());
    // the ack's bytes presented as the projection: refused
    assert!(run_with(&c, &acks, Some(&c.ack.to_string()), &ep).is_err());
    // a projection for a different sidecar policy: refused
    let wrong = proj(&Ref::new(format!("cl22:{}", "8".repeat(64))).unwrap());
    let mut ep2 = c.ep.clone();
    ep2["projection_ref"] = json!(digest_value(&wrong).unwrap());
    let e = run_with(&c, &acks, Some(&wrong.to_string()), &ep2).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("different sidecar policy")),
        "{e}"
    );
    assert_eq!(before, snapshot(c.s.root()));
    run_with(&c, &acks, Some(&good.to_string()), &ep).unwrap();
}

// ── step 8: a MiCode acceptance check that ran through Fabric (G3, D12) ─────

const OUT_TREE: &str = "acf1:7777777777777777777777777777777777777777777777777777777777777777";

fn check_request() -> Value {
    json!({
        "schema": "acf-compute-request/1",
        "operation_id": "op-1", "task_id": "task-1", "trial_id": "trial-1", "attempt_id": "attempt-1",
        "principal_ref": "principal:micode-check", "grant_ref": "grant:check", "approval_ref": null,
        "job_kind": "registered_check", "registered_executable_ref": "axon-test-local",
        "executable_digest": format!("acf1:{}", "e".repeat(64)),
        "workspace_version_ref": OUT_TREE, "semantic_state_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "required": {"engine": "axon_interpreter", "hardware_isolation": false, "os": "none",
                     "architecture": "x86_64", "network_mode": "deny", "checkpoint_kind": "none"},
        "limits": {"cpu_millicores": 1000, "memory_bytes": 268435456, "disk_bytes": 268435456,
                   "wall_time_ms": 60000, "output_bytes": 1048576, "max_cost_micro": 100,
                   "currency_code": "USD", "price_schedule_ref": "unpriced:test"},
        "argv": ["check:acceptance", "t_"], "result_schema_ref": "cortex-check-report/1",
    })
}

fn check_receipt(verification: &str, matched: u64) -> Value {
    json!({
        "schema": "acf-execution-receipt/1",
        "operation_id": "op-1", "task_id": "task-1", "trial_id": "trial-1", "attempt_id": "attempt-1",
        "execution_id": "exec-1", "backend_profile_ref": "fabric:local-interpreter",
        "input_workspace_ref": OUT_TREE, "output_workspace_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "status": "completed", "process_exit_code": 0,
        "verification": verification, "matched_checks": matched,
        "evidence_source": "supervisor_observed",
        "evidence_refs": ["check-report:fixture", common::check_suite()],
        "usage_state": "unknown", "cost_micro": null, "unresolved_liability_micro": 100,
    })
}

/// Point `ep`'s verification at `(req, rc)`, re-deriving every ref so the only
/// fault left is whatever the caller changed.
fn verified(ep: &Value, req: &Value, rc: &Value, result: &str) -> Value {
    let mut ep = ep.clone();
    ep["output_workspace_ref"] = json!(OUT_TREE);
    ep["verification"] = json!({
        "result": result,
        "matched_checks": rc["matched_checks"],
        "issuer_ref": common::VERIFIER,
        "verifier_ref": digest_value(rc).unwrap(),
        "output_workspace_ref": OUT_TREE,
        "evidence_refs": [digest_value(req).unwrap()],
    });
    ep
}

/// Step 8 with the evidence the fixture verifier genuinely signed.
fn run_v(
    c: &Case,
    ep: &Value,
    req: Option<&Value>,
    rc: Option<&Value>,
) -> Result<axon_loop::intake::IntakeOutcome, LoopError> {
    let att = match (req, rc) {
        (Some(q), Some(r)) => Some(attest(&verifier_key().0, common::VERIFIER, q, r)),
        _ => None,
    };
    run_va(c, ep, req, rc, att.as_ref())
}

fn run_va(
    c: &Case,
    ep: &Value,
    req: Option<&Value>,
    rc: Option<&Value>,
    att: Option<&Value>,
) -> Result<axon_loop::intake::IntakeOutcome, LoopError> {
    intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: None,
            verification_request: req.map(|v| v.to_string()).as_deref(),
            verification_receipt: rc.map(|v| v.to_string()).as_deref(),
            verification_attestation: att.map(|v| v.to_string()).as_deref(),
        },
    )
}

#[test]
fn a_fabric_check_on_the_output_tree_is_recorded_as_the_verification() {
    let c = case(Some(500));
    let (req, rc) = (check_request(), check_receipt("passed", 2));
    let ep = verified(&c.ep, &req, &rc, "passed");
    let out = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap();
    assert!(out.recorded_now);
    assert_eq!(
        out.record.verification_receipt_ref,
        Some(digest_value(&rc).unwrap())
    );
    assert_eq!(
        out.record.verification_request_ref,
        Some(digest_value(&req).unwrap())
    );
    // The documents are held, content-addressed, for any later re-check.
    let hex = digest_value(&rc).unwrap().hex().to_string();
    assert!(c
        .s
        .root()
        .join("fabric-receipts")
        .join(format!("{hex}.json"))
        .exists());

    // A failed check is recorded as a failed verification, not dropped.
    let c = case(Some(500));
    let rc = check_receipt("failed", 2);
    let ep = verified(&c.ep, &req, &rc, "failed");
    assert!(run_v(&c, &ep, Some(&req), Some(&rc)).is_ok());
}

/// A record with no verifier keeps its pre-G3 bytes: the new fields are ABSENT.
#[test]
fn an_unverified_record_serialises_without_the_new_fields() {
    let c = case(Some(500));
    let out = run(&c, &c.ep, false).unwrap();
    let v = serde_json::to_value(&out.record).unwrap();
    assert!(v.get("verification_receipt_ref").is_none());
    assert!(v.get("verification_request_ref").is_none());
}

/// Each case changes ONE thing from the positive control; every refusal writes
/// nothing. Mutation: delete any single check in `check_verification` and the
/// matching case here is accepted.
#[test]
fn verification_that_does_not_join_is_refused_with_the_store_unchanged() {
    let c = case(Some(500));
    let (req, rc) = (check_request(), check_receipt("passed", 2));
    let good = verified(&c.ep, &req, &rc, "passed");
    type Edit = Box<dyn Fn(&mut Value, &mut Value, &mut Value)>;
    let cases: Vec<(&str, Edit, &str)> = vec![
        (
            "receipt for another operation",
            Box::new(|_, _, r| r["operation_id"] = json!("op-2")),
            "operation",
        ),
        (
            "request for another attempt",
            Box::new(|_, q, _| q["attempt_id"] = json!("attempt-2")),
            "operation",
        ),
        (
            "execution id not the sidecar's",
            Box::new(|_, _, r| r["execution_id"] = json!("exec-9")),
            "operation",
        ),
        (
            "checked a different tree",
            Box::new(|_, q, r| {
                let other = json!(format!("acf1:{}", "8".repeat(64)));
                q["workspace_version_ref"] = other.clone();
                r["input_workspace_ref"] = other;
            }),
            "output tree",
        ),
        (
            "receipt input differs from request",
            Box::new(|_, _, r| {
                r["input_workspace_ref"] = json!(format!("acf1:{}", "9".repeat(64)))
            }),
            "output tree",
        ),
        (
            "provider-reported evidence",
            Box::new(|_, _, r| r["evidence_source"] = json!("provider_reported")),
            "supervisor",
        ),
        (
            "not a registered check",
            Box::new(|_, q, _| q["job_kind"] = json!("interpreter_run")),
            "registered_check",
        ),
        (
            "sidecar upgrades a failed check to passed",
            Box::new(|_, _, r| r["verification"] = json!("failed")),
            "check receipt says",
        ),
        (
            "sidecar claims a pass the check never reached",
            Box::new(|e, _, r| {
                r["status"] = json!("timed_out");
                r["verification"] = json!("unknown");
                e["verification"]["result"] = json!("passed");
            }),
            "check receipt says",
        ),
        (
            "matched_checks inflated",
            Box::new(|e, _, _| e["verification"]["matched_checks"] = json!(5)),
            "matched_checks",
        ),
        (
            "check ran as the subject",
            Box::new(|_, q, _| q["principal_ref"] = json!("micode-host-observer")),
            "principal",
        ),
        (
            "untrusted verifier",
            // A FAILED result: bind_episode checks the issuer only for a pass,
            // so this reaches step 8's own rule.
            Box::new(|e, _, r| {
                e["verification"]["issuer_ref"] = json!("fabric:someone-else");
                e["verification"]["result"] = json!("failed");
                r["verification"] = json!("failed");
            }),
            "trusted verifier",
        ),
        (
            "the subject as verifier",
            // A FAILED result: bind_episode checks the issuer only for a pass,
            // so this reaches step 8's own rule.
            Box::new(|e, _, r| {
                e["verification"]["issuer_ref"] = json!("micode-host-observer");
                e["verification"]["result"] = json!("failed");
                r["verification"] = json!("failed");
            }),
            "trusted verifier",
        ),
        // A replayed earlier receipt of the SAME operation, from a run on the
        // pre-session tree: every id matches, the bytes checked do not.
        (
            "stale receipt of this operation",
            Box::new(|_, q, r| {
                let before = json!(format!("acf1:{}", "a".repeat(64)));
                q["workspace_version_ref"] = before.clone();
                r["input_workspace_ref"] = before;
            }),
            "output tree",
        ),
        (
            "extra evidence ref",
            Box::new(|e, _, _| {
                let extra = json!(format!("cl22:{}", "1".repeat(64)));
                e["verification"]["evidence_refs"]
                    .as_array_mut()
                    .unwrap()
                    .push(extra);
            }),
            "evidence_refs",
        ),
    ];
    for (why, edit, want) in cases {
        let (mut e, mut q, mut r) = (good.clone(), req.clone(), rc.clone());
        edit(&mut e, &mut q, &mut r);
        // Re-derive the refs the sidecar holds unless the case is about them.
        if !why.contains("matched_checks") && !why.contains("evidence ref") {
            let result = e["verification"]["result"].as_str().unwrap().to_string();
            let issuer = e["verification"]["issuer_ref"].clone();
            e = verified(&e, &q, &r, &result);
            e["verification"]["issuer_ref"] = issuer;
        }
        let before = snapshot(c.s.root());
        let err = run_v(&c, &e, Some(&q), Some(&r)).expect_err(why);
        // Refused by step 8, or earlier by the contract's own schema (a
        // provider-reported receipt does not even parse): either way, not recorded.
        assert!(
            matches!(&err, LoopError::Refused(_) | LoopError::Malformed(_))
                && err.to_string().contains(want),
            "{why}: {err}"
        );
        assert_eq!(before, snapshot(c.s.root()), "{why}: store changed");
    }

    // Documents that are not the ones the sidecar names.
    let before = snapshot(c.s.root());
    let other_rc = check_receipt("passed", 3);
    let e = run_v(&c, &good, Some(&req), Some(&other_rc)).unwrap_err();
    assert!(
        matches!(&e, LoopError::Refused(m) if m.contains("verifier_ref")),
        "{e}"
    );
    // A verifier named but its evidence withheld.
    let e = run_v(&c, &good, None, Some(&rc)).unwrap_err();
    assert!(
        matches!(&e, LoopError::Refused(m) if m.contains("not both presented")),
        "{e}"
    );
    // Evidence presented for a sidecar that cites none.
    let e = run_v(&c, &c.ep, Some(&req), Some(&rc)).unwrap_err();
    assert!(
        matches!(&e, LoopError::Refused(m) if m.contains("names no verifier_ref")),
        "{e}"
    );
    assert_eq!(before, snapshot(c.s.root()));
}

/// B280 / G16-r22-peer-failure-matrix: a PARTIAL export (the sidecar arrives,
/// its Fabric evidence does not) is refused with the store unchanged; the
/// COMPLETE export later is recorded; re-sending it is idempotent; and a
/// partial re-send after that is still refused — never "replayed" into a
/// pass it could not have earned on its own.
#[test]
fn a_partial_export_is_refused_then_the_complete_one_is_recorded_once() {
    let c = case(Some(500));
    let (req, rc) = (check_request(), check_receipt("passed", 2));
    let ep = verified(&c.ep, &req, &rc, "passed");
    let before = snapshot(c.s.root());
    assert!(
        run_v(&c, &ep, None, None).is_err(),
        "partial: no Fabric documents"
    );
    assert!(
        run_v(&c, &ep, Some(&req), None).is_err(),
        "partial: request without receipt"
    );
    assert_eq!(
        snapshot(c.s.root()),
        before,
        "a partial export writes nothing"
    );
    let first = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap();
    assert!(first.recorded_now);
    let after = snapshot(c.s.root());
    let again = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap();
    assert!(!again.recorded_now);
    assert_eq!(again.ledger_seq, first.ledger_seq);
    assert!(
        run_v(&c, &ep, None, Some(&rc)).is_err(),
        "a partial re-send is not a replay"
    );
    assert_eq!(snapshot(c.s.root()), after);
}

/// B280 / G16-r22-peer-failure-matrix: an episode sidecar of ANOTHER schema
/// version is refused, store unchanged — never read as the nearest version.
#[test]
fn an_episode_of_another_schema_version_is_refused() {
    let c = case(Some(500));
    let mut ep = c.ep.clone();
    ep["schema"] = json!("axon.closed-loop.episode/2");
    let before = snapshot(c.s.root());
    let e = run(&c, &ep, false).unwrap_err();
    assert!(matches!(e, LoopError::Malformed(_)), "{e}");
    assert_eq!(snapshot(c.s.root()), before);
}

/// G10-r22-trial-identity, intake side: a second trial of the SAME task and
/// arm is a new record, never deduplicated by task id; only the identical
/// trial's identical bytes replay, and the same trial with different bytes is
/// a conflict.
#[test]
fn a_repeated_trial_of_one_task_and_arm_is_recorded_as_its_own() {
    let c = case(Some(500));
    let first = run(&c, &c.ep, false).unwrap();
    assert!(first.recorded_now);
    // Trial 2 of the same task and arm: new trial/attempt/operation ids.
    let mut ctx = c.ctx.clone();
    for k in ["trial_id", "attempt_id", "operation_id", "execution_id"] {
        ctx["identity"][k] = json!(format!("{}-2", ctx["identity"][k].as_str().unwrap()));
    }
    let mut ep = c.ep.clone();
    ep["identity"] = ctx["identity"].clone();
    ep["context_ref"] = json!(digest_value(&ctx).unwrap());
    let second = intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &ctx.to_string(),
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: None,
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
        },
    )
    .unwrap();
    assert!(
        second.recorded_now,
        "a fresh trial was deduplicated by its task id"
    );
    assert_ne!(second.record.episode_ref, first.record.episode_ref);
    assert_eq!(
        second.record.identity.task_id,
        first.record.identity.task_id
    );
    assert_eq!(second.ledger_seq, first.ledger_seq + 1);
    // The identical trial replays; nothing new is written.
    assert!(!run(&c, &c.ep, false).unwrap().recorded_now);
}

/// G16-r22-negotiation, Axon side of the pinned spend migration: every
/// historical canonical-episode shape reads as MiCode's `ResourceCostWire`
/// reads it — v014's never-written default 0 is UNKNOWN, a v0.22-dev null is
/// unknown and a non-zero value is the spend, and the explicit field (null or a
/// genuine 0) wins. A malformed cost is a shape error, not a guess. And the join
/// refuses a known figure on either side against an unknown on the other.
///
/// Mutation: drop the `filter(|&c| c != 0)` on the legacy branch → the v014
/// episode reads as a known zero and this fails.
#[test]
fn every_historical_spend_shape_is_read_through_the_pinned_table() {
    use axon_loop::intake::source_episode_spend;
    let spend = |cost: Value| source_episode_spend(&json!({"cost": cost}));
    assert_eq!(
        spend(json!({"micro_cents": 0})).unwrap(),
        None,
        "v014 default"
    );
    assert_eq!(
        spend(json!({"micro_cents": null})).unwrap(),
        None,
        "v0.22-dev unknown"
    );
    assert_eq!(
        spend(json!({"micro_cents": 7500})).unwrap(),
        Some(7500),
        "v0.22-dev spend"
    );
    assert_eq!(
        spend(json!({"micro_cents": 0, "spend_micro_cents": null})).unwrap(),
        None,
        "now, unknown"
    );
    assert_eq!(
        spend(json!({"micro_cents": 0, "spend_micro_cents": 0})).unwrap(),
        Some(0),
        "now, a genuine zero"
    );
    assert!(spend(json!({"micro_cents": -1})).is_err());
    assert!(spend(json!({"spend_micro_cents": 1.5, "micro_cents": 0})).is_err());
    assert!(
        spend(json!({})).is_err(),
        "a cost that states no spend at all"
    );

    // A known canonical spend against an "unknown" sidecar: a figure dropped.
    let c = case(None);
    let src = source_episode(Some(4_000));
    let mut ep = c.ep.clone();
    ep["source_episode_ref"] = json!(digest_value(&src).unwrap());
    let before = snapshot(c.s.root());
    let e = intake_episode(
        &c.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &c.ctx.to_string(),
            acks: &[c.ack.to_string()],
            projection: None,
            source_episode: Some(&src.to_string()),
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
        },
    )
    .unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("dropped")),
        "{e}"
    );
    assert_eq!(snapshot(c.s.root()), before);
}

/// G01-r22-independent-issuer / G32-r22-sidecar-bindings: verification evidence
/// is AUTHENTICATED, not named — and authentication does not replace the
/// semantic joins. The sidecar names a trusted verifier and every digest joins;
/// what decides is that verifier's own signature over exactly this request and
/// receipt, under the key the operator registered for it. Each forgery below is
/// refused for ITS OWN reason, and none writes a byte:
///
/// missing; malformed; self-signed attacker key claiming the verifier's name;
/// another REGISTERED verifier's key claiming this verifier's name; a genuine
/// signature over another request; over another receipt; a genuine attestation
/// replayed from another trial; the receipt tampered after signing; the request
/// tampered after signing; a trusted verifier with no registered key; and the
/// subject vouching for itself with a genuinely registered key.
///
/// Mutation: delete the attestation block in `check_verification` → the
/// forgeries are recorded and this fails.
#[test]
fn verification_evidence_is_authenticated_not_named() {
    let c = case(Some(500));
    let req = check_request();
    let rc = check_receipt("passed", 1);
    let ep = verified(&c.ep, &req, &rc, "passed");
    let genuine = |q: &Value, r: &Value| attest(&verifier_key().0, common::VERIFIER, q, r);
    // A second verifier the operator DOES trust and has a key for.
    let (b_sk, b_pk) = axon_loop_contracts::attestation::generate().unwrap();
    let mut config = c.s.config().unwrap();
    config
        .trusted_verifiers
        .push(OpaqueRef::new("fixture:verifier-b").unwrap());
    config
        .verifier_keys
        .insert(OpaqueRef::new("fixture:verifier-b").unwrap(), b_pk);
    c.s.write_config(&config).unwrap();
    let before = snapshot(c.s.root());

    let refused = |ep: &Value, req: &Value, rc: &Value, att: Option<&Value>, why: &str| {
        let e = run_va(&c, ep, Some(req), Some(rc), att).unwrap_err();
        assert!(
            matches!(e, LoopError::Refused(ref m) if m.contains(why)),
            "expected a refusal for `{why}`: {e}"
        );
        assert_eq!(snapshot(c.s.root()), before, "`{why}` wrote to the store");
    };
    refused(&ep, &req, &rc, None, "not authenticated");
    refused(
        &ep,
        &req,
        &rc,
        Some(&json!("not an attestation")),
        "not a JSON object",
    );
    let (impostor, _) = axon_loop_contracts::attestation::generate().unwrap();
    refused(
        &ep,
        &req,
        &rc,
        Some(&attest(&impostor, common::VERIFIER, &req, &rc)),
        "not by",
    );
    refused(
        &ep,
        &req,
        &rc,
        Some(&attest(&b_sk, common::VERIFIER, &req, &rc)),
        "not by",
    );

    let mut other_req = check_request();
    other_req["limits"]["max_cost_micro"] = json!(99);
    refused(
        &ep,
        &req,
        &rc,
        Some(&genuine(&other_req, &rc)),
        "request_ref",
    );
    let other_rc = check_receipt("failed", 1);
    refused(
        &ep,
        &req,
        &rc,
        Some(&genuine(&req, &other_rc)),
        "receipt_ref",
    );
    let mut req2 = check_request();
    req2["trial_id"] = json!("trial-2");
    let mut rc2 = check_receipt("passed", 1);
    rc2["trial_id"] = json!("trial-2");
    // (bound fields are compared in order; another trial's receipt differs first)
    refused(&ep, &req, &rc, Some(&genuine(&req2, &rc2)), "receipt_ref");

    // Tampered after signing: the sidecar is re-derived to cite the tampered
    // document, so only the signature can notice.
    let signed = genuine(&req, &rc);
    let mut rc_t = rc.clone();
    rc_t["matched_checks"] = json!(2);
    refused(
        &verified(&c.ep, &req, &rc_t, "passed"),
        &req,
        &rc_t,
        Some(&signed),
        "receipt_ref",
    );
    let mut req_t = req.clone();
    req_t["argv"] = json!(["f.ax", "t_other"]);
    refused(
        &verified(&c.ep, &req_t, &rc, "passed"),
        &req_t,
        &rc,
        Some(&signed),
        "request_ref",
    );

    // The subject vouching for itself, with a key the operator registered.
    let subject_id = c.ctx["observed_issuer_ref"].as_str().unwrap().to_string();
    let (s_sk, s_pk) = axon_loop_contracts::attestation::generate().unwrap();
    let mut config = c.s.config().unwrap();
    config
        .trusted_verifiers
        .push(OpaqueRef::new(&subject_id).unwrap());
    config
        .verifier_keys
        .insert(OpaqueRef::new(&subject_id).unwrap(), s_pk);
    c.s.write_config(&config).unwrap();
    let before_subject = snapshot(c.s.root());
    let mut ep_s = ep.clone();
    ep_s["verification"]["issuer_ref"] = json!(subject_id);
    let e = run_va(
        &c,
        &ep_s,
        Some(&req),
        Some(&rc),
        Some(&attest(&s_sk, &subject_id, &req, &rc)),
    )
    .unwrap_err();
    assert!(
        // bind_episode refuses it first; check_verification would too.
        matches!(e, LoopError::Refused(ref m) if m.contains("subject")),
        "{e}"
    );
    assert_eq!(snapshot(c.s.root()), before_subject);

    // A trusted verifier with no registered key vouches for nothing.
    config
        .verifier_keys
        .remove(&OpaqueRef::new(common::VERIFIER).unwrap());
    c.s.write_config(&config).unwrap();
    let before_nokey = snapshot(c.s.root());
    let e = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("no registered key")),
        "{e}"
    );
    assert_eq!(snapshot(c.s.root()), before_nokey);

    // The genuine evidence, under the registered key, is recorded — and the
    // outcome names the key that authenticated it.
    config.verifier_keys.insert(
        OpaqueRef::new(common::VERIFIER).unwrap(),
        verifier_key().1.clone(),
    );
    c.s.write_config(&config).unwrap();
    let out = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap();
    assert!(out
        .verification_key_id
        .is_some_and(|k| k.starts_with("ed25519:")));
}

/// G01-r22-independent-issuer / G01-r22-verifier-separation: an AUTHENTIC
/// verdict still counts only if the verifier ran what the operator pinned for
/// it. Every document below is genuinely signed by the trusted verifier's
/// registered key and joins the episode; each differs from the pin in one way
/// and is refused for that reason, with the store unchanged: another verifier
/// revision; another compute profile; a check file from the candidate's own
/// tree (candidate bytes cannot define the rubric); a suite version the
/// operator did not pin; a suite other than the one the request named; and a
/// trusted verifier with no pin at all.
///
/// Mutation: delete the pin block in `verify_check_evidence` → the substituted
/// verdicts are recorded and this fails.
#[test]
fn a_verdict_counts_only_for_what_the_operator_pinned() {
    let c = case(Some(500));
    let before = snapshot(c.s.root());
    type Alter = Box<dyn Fn(&mut Value, &mut Value)>;
    let cases: Vec<(&str, &str, Alter)> = vec![
        (
            "another verifier revision",
            "verifier revision",
            Box::new(|req, _| req["executable_digest"] = json!(format!("acf1:{}", "f".repeat(64)))),
        ),
        (
            "another compute profile",
            "compute profile",
            Box::new(|_, rc| rc["backend_profile_ref"] = json!("fabric:someone-elses-laptop")),
        ),
        (
            "a check file from the candidate's own tree",
            "candidate bytes cannot define the acceptance rubric",
            Box::new(|req, rc| {
                req["argv"] = json!(["checks/accept.ax", "t_"]);
                rc["evidence_refs"] = json!(["check-report:fixture"]);
            }),
        ),
        (
            "an unpinned suite version",
            "not a version pinned",
            Box::new(|_, rc| {
                rc["evidence_refs"] = json!([
                    "check-report:fixture",
                    format!("check-suite:acceptance@acf1:{}", "6".repeat(64))
                ])
            }),
        ),
        (
            "a suite other than the one requested",
            "not a version pinned",
            Box::new(|req, _| req["argv"] = json!(["check:lenient", "t_"])),
        ),
    ];
    for (why, reason, alter) in cases {
        let (mut req, mut rc) = (check_request(), check_receipt("passed", 1));
        alter(&mut req, &mut rc);
        let ep = verified(&c.ep, &req, &rc, "passed");
        let att = attest(&verifier_key().0, common::VERIFIER, &req, &rc);
        let e = run_va(&c, &ep, Some(&req), Some(&rc), Some(&att)).unwrap_err();
        assert!(
            matches!(e, LoopError::Refused(ref m) if m.contains(reason)),
            "{why}: expected `{reason}`: {e}"
        );
        assert_eq!(snapshot(c.s.root()), before, "{why} wrote to the store");
    }

    // A trusted, keyed verifier the operator pinned nothing for.
    let mut config = c.s.config().unwrap();
    config.verifier_pins.clear();
    c.s.write_config(&config).unwrap();
    let before = snapshot(c.s.root());
    let (req, rc) = (check_request(), check_receipt("passed", 1));
    let ep = verified(&c.ep, &req, &rc, "passed");
    let e = run_v(&c, &ep, Some(&req), Some(&rc)).unwrap_err();
    assert!(
        matches!(e, LoopError::Refused(ref m) if m.contains("no operator pin")),
        "{e}"
    );
    assert_eq!(snapshot(c.s.root()), before);
}
