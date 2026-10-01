//! Amendment 64 (C9 round 4b, integrate-A): the refusal sites of the contract
//! crate (`canonical`, `compute`, `episode`, `ids`, `lib`, `policy`,
//! `receipt`, `schema`), each judged on the PRODUCTION route that reads the
//! attacker's bytes:
//!
//! * `intake::intake_episode` (MiCode's sidecar, its context receipt);
//! * `evl::evaluate` (the delivered trial's ACF request/receipt, the
//!   request's policies and issuers), after the producer's intake;
//! * `price::PinnedSchedule::pin` (a price schedule, untyped `rates`);
//! * `candidates::CandidateSet::parse` + `candidates::put` (a candidate list);
//! * `axon_loop_contracts::parse` + `candidates::put_policy` /
//!   `pointer::transition` (the CLI's two steps for a policy or transition).
//!
//! Every test is an ATTACK (one defect, everything else genuine) with its
//! CONTROL (the same document without the defect is accepted). Where two
//! checks refuse the same attack, each alone (a four-cell retirement in
//! `scripts/v022_g01_mutations.py`), the assertion accepts either reason:
//! only the attack getting through is the failure, and any other refusal is a
//! failure of the test (never a silent pass).

mod common;
use axon_loop::error::LoopError;
use axon_loop::intake::{cost_micro_from_micro_cents, intake_episode, IntakeInput};
use axon_loop::price::PinnedSchedule;
use axon_loop::store::Store;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

// ── digests the library itself would refuse to compute ─────────────────────

/// The `cl22` canonical text of `v` (sorted keys, no whitespace). The values
/// here hold only ASCII text without escapes, integers and floats, whose
/// canonical spelling is serde's.
fn canon(v: &Value) -> String {
    match v {
        Value::Object(o) => {
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            let body: Vec<String> = keys
                .iter()
                .map(|k| format!("{}:{}", Value::String((*k).clone()), canon(&o[*k])))
                .collect();
            format!("{{{}}}", body.join(","))
        }
        Value::Array(a) => format!("[{}]", a.iter().map(canon).collect::<Vec<_>>().join(",")),
        other => other.to_string(),
    }
}

fn cl22(v: &Value) -> String {
    let d = ring::digest::digest(&ring::digest::SHA256, canon(v).as_bytes());
    let hex: String = d.as_ref().iter().map(|b| format!("{b:02x}")).collect();
    format!("cl22:{hex}")
}

/// `ok` holds for the result, or the refusal names one of `why`; ATTACK if the
/// attack got through.
fn judged<T>(r: Result<T, LoopError>, why: &[&str], attack: &str, through: &str) {
    match r {
        Ok(_) => panic!("ATTACK: {attack}: {through}"),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "{attack}: expected one of {why:?}: {e}"
        ),
    }
}

// ── venue PRICE: `PinnedSchedule::pin` over an untyped `rates` value ────────

fn schedule(rates: Value) -> Value {
    json!({"schema": "axon.loop.price-schedule/1", "currency": "USD", "covers": ["model"],
           "rates": rates})
}

fn pin_text(doc: &Value, text: &str) -> Result<PinnedSchedule, LoopError> {
    PinnedSchedule::pin(&Ref::new(cl22(doc)).unwrap(), text)
}

fn pin(doc: &Value) -> Result<PinnedSchedule, LoopError> {
    pin_text(doc, &canon(doc))
}

fn never_pinned(r: Result<PinnedSchedule, LoopError>, why: &[&str], attack: &str) {
    judged(r, why, attack, "the price schedule was pinned");
}

/// `n` nested arrays around `inner`.
fn nest(n: usize, inner: Value) -> Value {
    (0..n).fold(inner, |v, _| json!([v]))
}

#[test]
fn control_an_honest_schedule_pins() {
    pin(&schedule(json!({"x": nest(29, json!([]))}))).unwrap();
    pin(&schedule(json!({"x": nest(30, json!(1))}))).unwrap();
    pin(&schedule(json!({"x": nest(29, json!({"k": 1}))}))).unwrap();
    pin(&schedule(json!({"x": 9_007_199_254_740_991u64}))).unwrap();
    pin(&schedule(json!({"x": -9_007_199_254_740_991i64}))).unwrap();
    pin(&schedule(json!({"x": "a".repeat(1000)}))).unwrap();
}

/// canonical.rs `parse_value` byte limit: the text, not its canonical form,
/// is over MAX_BYTES (whitespace), so nothing else refuses it.
#[test]
fn a_document_over_the_byte_limit_is_never_read() {
    let doc = schedule(json!({}));
    let text = format!("{}{}", canon(&doc), " ".repeat(MAX_BYTES));
    never_pinned(
        pin_text(&doc, &text),
        &["JSON byte limit"],
        "a price schedule over the byte limit",
    );
}

/// canonical.rs `bounded_depth`: 33 open brackets whose innermost container is
/// EMPTY, so the value walk (which counts values, not brackets) admits it.
#[test]
fn an_empty_container_nested_past_the_limit_is_never_read() {
    // root {, rates {, then 31 arrays: 33 brackets; the innermost (empty)
    // array is at value depth 32, which the walk allows.
    never_pinned(
        pin(&schedule(json!({"x": nest(30, json!([]))}))),
        &["JSON nesting limit"],
        "an empty container nested 33 deep",
    );
}

/// canonical.rs `json_tree` depth (retired against the bracket pre-scan): a
/// scalar at value depth 33.
#[test]
fn a_value_nested_past_the_limit_is_never_read() {
    never_pinned(
        pin(&schedule(json!({"x": nest(31, json!(1))}))),
        &["JSON nesting limit"],
        "a value nested 33 deep",
    );
}

/// canonical.rs `json_tree`'s object rule (retired against the value-depth
/// rule and the pre-scan): a non-empty object at value depth 32.
#[test]
fn an_object_with_keys_at_the_depth_limit_is_never_read() {
    never_pinned(
        pin(&schedule(json!({"x": nest(30, json!({"k": 1}))}))),
        &["JSON nesting limit"],
        "an object with keys at depth 32",
    );
}

#[test]
fn an_integer_past_two_to_the_53_is_never_read() {
    never_pinned(
        pin(&schedule(json!({"x": 9_007_199_254_740_992u64}))),
        &["unsafe JSON integer"],
        "an integer of 2^53",
    );
}

#[test]
fn an_integer_past_i64_is_never_read() {
    never_pinned(
        pin(&schedule(json!({"x": u64::MAX}))),
        &["unsafe JSON integer"],
        "an integer of 2^64-1",
    );
}

#[test]
fn a_float_is_never_read() {
    never_pinned(
        pin(&schedule(json!({"x": 1.5}))),
        &["float/non-integer"],
        "a float",
    );
}

/// canonical.rs `canonical_bytes` output limit (retired against the input
/// limit): a value whose canonical form is over MAX_BYTES.
#[test]
fn a_value_whose_canonical_form_is_over_the_byte_limit_is_never_digested() {
    never_pinned(
        pin(&schedule(json!({"x": "a".repeat(MAX_BYTES)}))),
        &["JSON byte limit"],
        "a value over the byte limit",
    );
}

/// ids.rs `check_currency`: the schedule's currency is typed (no schema).
#[test]
fn a_schedule_in_a_currency_that_is_no_iso_code_is_never_pinned() {
    let mut doc = schedule(json!({}));
    pin(&doc).unwrap();
    doc["currency"] = json!("usd");
    never_pinned(
        pin(&doc),
        &["currency must match"],
        "a schedule in currency \"usd\"",
    );
}

// ── venue CANDIDATES: `CandidateSet::parse` + `candidates::put` ────────────

fn candidate_set(cands: &[&str], issuer: &str) -> String {
    json!({"schema": "axon.loop.candidate-set/1", "scope": scope(), "candidates": cands,
           "issuer_ref": issuer})
    .to_string()
}

fn register_list(cands: &[&str]) -> Result<Ref, LoopError> {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let c = axon_loop::candidates::CandidateSet::parse(&candidate_set(cands, ADMITTER))?;
    axon_loop::candidates::put(&s, &c)
}

fn never_registered(cands: &[&str], why: &[&str], attack: &str) {
    judged(
        register_list(cands),
        why,
        attack,
        "the candidate list was registered",
    );
}

#[test]
fn control_an_honest_candidate_list_registers() {
    register_list(&[&"a".repeat(128), "bash", "edit", "x.y_z:1-2"]).unwrap();
}

#[test]
fn a_candidate_id_over_128_bytes_is_never_registered() {
    never_registered(
        &[&"a".repeat(129), "bash"],
        &["length must be 1..=128"],
        "a 129-byte candidate id",
    );
}

#[test]
fn a_candidate_id_not_starting_alphanumeric_is_never_registered() {
    never_registered(
        &[".x", "bash"],
        &["must start with"],
        "a candidate id starting with '.'",
    );
}

#[test]
fn a_candidate_id_outside_the_charset_is_never_registered() {
    never_registered(
        &["a b", "bash"],
        &["charset is"],
        "a candidate id with a space",
    );
}

// ── venue INTAKE: MiCode's sidecar (tests/intake.rs's shape) ───────────────

const CANDIDATES: &[&str] = &["bash", "edit", "grep", "read", "write"];

fn scope_json() -> Value {
    json!({"tenant_id": "tenant-a", "task_family": "coding"})
}

fn mi_candidate_set_ref() -> Ref {
    digest_value(&json!(CANDIDATES)).unwrap()
}

fn mi_policy() -> PolicyEnvelope {
    parse(
        &json!({
            "schema": "axon.closed-loop.policy/1",
            "policy_id": "pol-dec-1",
            "parent_policy_ref": format!("cl22:{}", "0".repeat(64)),
            "scope": scope_json(),
            "mode": "shortlist_only",
            "candidate_set_ref": mi_candidate_set_ref(),
            "controls_ref": format!("cl22:{}", "c".repeat(64)),
            "shortlist": ["grep", "read"],
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

fn facts(branch: &str) -> Value {
    json!({
        "repo_id": "fixture", "base_commit": "b".repeat(40),
        "workspace_ref": format!("acf1:{}", "a".repeat(64)),
        "branch": branch, "worktree_id": "repo", "working_directory": "/w/repo",
        "build_namespace": "shared:default-target",
        "model_ref": format!("cl22:{}", "3".repeat(64)), "role": "implementation",
        "namespace_ref": format!("cl22:{}", "4".repeat(64)),
        "read_paths": [], "write_paths": ["src/**"], "is_primary_worktree": true,
    })
}

fn context(branch: &str) -> Value {
    json!({
        "schema": "axon.closed-loop.context/1", "context_id": "ctx-1",
        "identity": identity(), "scope": scope_json(),
        "expected": facts(branch), "observed": facts(branch),
        "expected_issuer_ref": "fixture-harness", "observed_issuer_ref": "micode-host-observer",
        "observed_evidence_ref": format!("cl22:{}", "5".repeat(64)),
        "created_ms": 1_000, "expires_ms": 601_000, "authority_epoch": 0,
    })
}

fn mi_ack(p: &PolicyEnvelope) -> Value {
    json!({
        "schema": "micode.closed-loop.policy-ack/1",
        "pin": {"state": "pinned", "policy_id": p.policy_id, "policy_ref": digest(p).unwrap(),
                "controls_ref": p.controls_ref, "candidate_set_ref": p.candidate_set_ref,
                "shortlist": p.shortlist},
        "candidates": CANDIDATES,
        "candidate_set_ref": mi_candidate_set_ref(),
    })
}

/// A MiCode-shaped sidecar over `ctx` (estimated cost 124, not_run).
fn sidecar(p: &PolicyEnvelope, ctx: &Value) -> Value {
    json!({
        "schema": "axon.closed-loop.episode/1",
        "identity": identity(), "scope": scope_json(),
        "policy_ref": digest(p).unwrap(), "controls_ref": p.controls_ref,
        "context_ref": cl22(ctx), "candidate_set_ref": p.candidate_set_ref,
        "input_workspace_ref": ctx["observed"]["workspace_ref"],
        "output_workspace_ref": null,
        "acf_request_ref": axon_loop::intake::micode_not_produced_ref("acf_request_ref"),
        "acf_receipt_ref": axon_loop::intake::micode_not_produced_ref("acf_receipt_ref"),
        "source_episode_ref": format!("cl22:{}", "6".repeat(64)),
        "authority_epoch": 0,
        "status": "completed",
        "verification": {"result": "not_run", "matched_checks": 0, "issuer_ref": null,
                         "verifier_ref": null, "output_workspace_ref": null, "evidence_refs": []},
        "usage": {"state": "estimated", "cost_micro": cost_micro_from_micro_cents(Some(12_345)),
                  "unresolved_liability_micro": 0, "currency": "USD",
                  "price_schedule_ref": axon_loop::intake::micode_not_produced_ref("price_schedule_ref"),
                  "attempt_refs": [format!("cl22:{}", "7".repeat(64))]},
        "corpus_role": "mechanism_test",
        "data_use_ref": format!("cl22:{}", "d".repeat(64)),
        "projection_ref": null,
    })
}

struct Mi {
    _dir: tempfile::TempDir,
    s: Store,
    p: PolicyEnvelope,
}

fn mi() -> Mi {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config(dir.path());
    let names: Vec<CandidateId> = CANDIDATES
        .iter()
        .map(|c| CandidateId::new(*c).unwrap())
        .collect();
    let cs = axon_loop::candidates::CandidateSet::parse(
        &json!({"schema":"axon.loop.candidate-set/1","scope":scope_json(),"candidates":names,
                "issuer_ref":ADMITTER})
        .to_string(),
    )
    .unwrap();
    axon_loop::candidates::put(&s, &cs).unwrap();
    let p = mi_policy();
    axon_loop::candidates::put_policy(&s, &p).unwrap();
    Mi { _dir: dir, s, p }
}

/// Intake `ep` over `ctx` (texts as the producer wrote them).
fn intake(m: &Mi, ep: &Value, ctx: &Value) -> Result<axon_loop::intake::IntakeOutcome, LoopError> {
    intake_episode(
        &m.s,
        &IntakeInput {
            episode: &ep.to_string(),
            context: &ctx.to_string(),
            acks: &[mi_ack(&m.p).to_string()],
            projection: None,
            source_episode: None,
            verification_request: None,
            verification_receipt: None,
            verification_attestation: None,
            verification_psv_evidence: None,
        },
    )
}

/// The sidecar with `edit` applied (over the honest context, or `ctx` edited
/// by `edit_ctx`, the sidecar naming the edited context) is never recorded.
fn never_intaken(
    edit_ctx: impl FnOnce(&mut Value),
    edit: impl FnOnce(&mut Value),
    why: &[&str],
    attack: &str,
) {
    let m = mi();
    let mut ctx = context("main");
    edit_ctx(&mut ctx);
    let mut ep = sidecar(&m.p, &ctx);
    edit(&mut ep);
    judged(
        intake(&m, &ep, &ctx),
        why,
        attack,
        "the episode was recorded",
    );
}

#[test]
fn control_an_honest_sidecar_is_recorded() {
    let m = mi();
    let ctx = context("main");
    let out = intake(&m, &sidecar(&m.p, &ctx), &ctx).unwrap();
    assert!(out.recorded_now);
    // The usage variants the attacks below differ from, each honest.
    for (state, cost, liability) in [
        ("unknown", json!(null), 0),
        ("final", json!(5), 0),
        ("estimated", json!(5), 7),
    ] {
        let m = mi();
        let mut ep = sidecar(&m.p, &ctx);
        ep["usage"]["state"] = json!(state);
        ep["usage"]["cost_micro"] = cost;
        ep["usage"]["unresolved_liability_micro"] = json!(liability);
        intake(&m, &ep, &ctx).unwrap();
    }
    let m = mi();
    let mut ep = sidecar(&m.p, &ctx);
    ep["status"] = json!("outcome_unknown");
    ep["verification"]["result"] = json!("unknown");
    intake(&m, &ep, &ctx).unwrap();
    let m = mi();
    let ctx = context(&"b".repeat(512));
    intake(&m, &sidecar(&m.p, &ctx), &ctx).unwrap();
}

/// schema.rs `check_type` (red-team D1): a struct given as a positional
/// array, which typed serde accepts (and re-serializes to the SAME digest).
#[test]
fn a_struct_spelled_as_a_positional_array_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| ep["scope"] = json!(["tenant-a", "coding"]),
        &["is not of type"],
        "an episode whose scope is a positional array",
    );
}

/// schema.rs `enum` (red-team D2): a unit variant spelled `{"variant":null}`,
/// which typed serde accepts.
#[test]
fn an_enum_spelled_as_a_variant_map_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| ep["status"] = json!({"completed": null}),
        &["is not one of"],
        "an episode whose status is {\"completed\":null}",
    );
}

/// ids.rs `check_hex64` / schema.rs `anyOf` (each retired against the other).
#[test]
fn a_reference_with_uppercase_hex_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| ep["output_workspace_ref"] = json!(format!("acf1:{}", "A".repeat(64))),
        &["matches none of anyOf", "64 lowercase hex"],
        "an episode whose output_workspace_ref has uppercase hex",
    );
}

/// ids.rs `check_acf1` / schema.rs `pattern` (each retired against the other).
#[test]
fn an_acf_reference_of_another_scheme_is_never_recorded() {
    let other = json!(format!("cl22:{}", "a".repeat(64)));
    let o2 = other.clone();
    never_intaken(
        |ctx| {
            ctx["expected"]["workspace_ref"] = other.clone();
            ctx["observed"]["workspace_ref"] = other;
        },
        |ep| ep["input_workspace_ref"] = o2,
        &["does not match", "expected acf1:"],
        "an episode whose input workspace is a cl22: reference",
    );
}

/// lib.rs `check_array` cardinality / schema.rs `minItems` (each retired
/// against the other).
#[test]
fn an_episode_naming_no_attempt_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| ep["usage"]["attempt_refs"] = json!([]),
        &["fewer than 1", "expected 1..=4096"],
        "an episode with no attempt_refs",
    );
}

/// lib.rs `check_array` uniqueness / schema.rs `uniqueItems` (each retired
/// against the other).
#[test]
fn an_episode_naming_an_attempt_twice_is_never_recorded() {
    let a = format!("cl22:{}", "7".repeat(64));
    never_intaken(
        |_| {},
        |ep| ep["usage"]["attempt_refs"] = json!([a, a]),
        &["duplicate item"],
        "an episode naming one attempt twice",
    );
}

/// schema.rs `required` / the typed layer's missing-field refusal (a
/// four-cell pair).
#[test]
fn an_episode_missing_a_required_field_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| {
            ep.as_object_mut().unwrap().remove("projection_ref");
        },
        &[
            "missing required field `projection_ref`",
            "missing field `projection_ref`",
        ],
        "an episode with no projection_ref",
    );
}

/// schema.rs `additionalProperties: false` / the typed layer's
/// `deny_unknown_fields` (a four-cell pair).
#[test]
fn an_episode_with_a_field_no_schema_names_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| ep["usage"]["note"] = json!("free"),
        &["unknown field `note`"],
        "an episode whose usage carries an unknown field",
    );
}

/// episode.rs usage rule: unknown usage has no cost (retired against the
/// schema's conditional `type: null`).
#[test]
fn an_unknown_usage_with_a_cost_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| {
            ep["usage"]["state"] = json!("unknown");
            ep["usage"]["cost_micro"] = json!(5);
        },
        &["is not of type", "usage state unknown requires"],
        "an episode with unknown usage and a cost",
    );
}

/// episode.rs usage rule: final usage has a known cost.
#[test]
fn a_final_usage_without_a_cost_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| {
            ep["usage"]["state"] = json!("final");
            ep["usage"]["cost_micro"] = json!(null);
        },
        &["is not of type", "usage state final requires a known"],
        "an episode with final usage and no cost",
    );
}

/// episode.rs usage rule: final usage leaves no liability.
#[test]
fn a_final_usage_with_liability_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| {
            ep["usage"]["state"] = json!("final");
            ep["usage"]["cost_micro"] = json!(5);
            ep["usage"]["unresolved_liability_micro"] = json!(7);
        },
        &["must be 0", "usage state final requires unresolved"],
        "an episode with final usage and unresolved liability",
    );
}

/// episode.rs: an outcome-unknown episode cannot carry a verdict.
#[test]
fn an_outcome_unknown_episode_with_a_verdict_is_never_recorded() {
    never_intaken(
        |_| {},
        |ep| {
            ep["status"] = json!("outcome_unknown");
            ep["verification"]["result"] = json!("failed");
        },
        &["is not one of", "status outcome_unknown admits only"],
        "an outcome_unknown episode with a failed verdict",
    );
}

// ── venue EVL: the delivered trial's documents and the request's own ───────

fn specs(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}

fn frozen(exp: &str) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, exp, &specs(&w));
    let v = evl_request(exp, &w.inc, &w.cand, &specs(&w), &EvlOpts::default());
    (w, v)
}

/// The first challenger trial.
fn c0(v: &mut Value) -> &mut Value {
    v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        .unwrap()
}

/// Intake what `v` delivers, then evaluate it: Ok is the evaluation recorded.
fn evaluated(w: &World, v: &Value) -> Result<(axon_loop::evl::EvaluationRecord, Ref), LoopError> {
    let _ = intake_all(&w.s, v);
    axon_loop::evl::evaluate(&w.s, &axon_loop::evl::parse_request(&v.to_string())?)
}

/// The honest request with the first challenger trial's ACF request and
/// receipt edited (the episode re-bound to the edited bytes), evaluated.
fn with_acf(
    edit_req: impl FnOnce(&mut Value),
    edit_rc: impl FnOnce(&mut Value),
    edit_ep: impl FnOnce(&mut Value),
) -> Result<(axon_loop::evl::EvaluationRecord, Ref), LoopError> {
    let (w, mut v) = frozen("acf");
    let t = c0(&mut v);
    edit_req(&mut t["acf_request"]);
    edit_rc(&mut t["acf_receipt"]);
    t["episode"]["acf_request_ref"] = json!(cl22(&t["acf_request"]));
    t["episode"]["acf_receipt_ref"] = json!(cl22(&t["acf_receipt"]));
    edit_ep(&mut t["episode"]);
    evaluated(&w, &v)
}

/// The attack got through when the evaluation was recorded AND it counted
/// the attacked trial (a pass or a fail, not an Unknown): a trial a later
/// check turned into an Unknown is not the defect admitted.
fn never_evaluated(
    r: Result<(axon_loop::evl::EvaluationRecord, Ref), LoopError>,
    why: &[&str],
    attack: &str,
) {
    let r = r.and_then(|(rec, e)| {
        let c = rec
            .arms
            .iter()
            .find(|a| a.arm_id.as_str() == "challenger-1")
            .unwrap();
        if c.unknown == 0 {
            Ok((rec, e))
        } else {
            Err(LoopError::Refused(format!(
                "not counted: {:?}",
                c.unknown_kinds
            )))
        }
    });
    judged(
        r,
        why,
        attack,
        "the evaluation was recorded and counted the trial",
    );
}

/// For an outcome-unknown trial: recorded at all (it is an Unknown either way).
fn never_recorded(
    r: Result<(axon_loop::evl::EvaluationRecord, Ref), LoopError>,
    why: &[&str],
    attack: &str,
) {
    judged(r, why, attack, "the evaluation was recorded");
}

#[test]
fn control_the_honest_evaluation_and_its_acf_variants_are_recorded() {
    let (w, v) = frozen("ok");
    let (rec, _) = evaluated(&w, &v).unwrap();
    assert!(
        rec.arms.iter().all(|a| a.verified_pass == 2),
        "{:?}",
        rec.arms
    );
    // Each honest variant of the documents the attacks below edit.
    with_acf(
        |q| {
            q["argv"] = json!(vec!["x"; 128]);
            q["limits"]["cpu_millicores"] = json!(1);
            q["limits"]["memory_bytes"] = json!(9_007_199_254_740_991u64);
            q["approval_ref"] = json!("a".repeat(512));
        },
        |rc| {
            rc["verification"] = json!("passed");
            rc["matched_checks"] = json!(1);
            rc["evidence_refs"] = json!(vec!["e"; 128]);
        },
        |_| {},
    )
    .unwrap();
    with_acf(|q| q["argv"] = json!(["x".repeat(8192)]), |_| {}, |_| {}).unwrap();
    with_acf(
        |_| {},
        |rc| {
            rc["usage_state"] = json!("unknown");
            rc["cost_micro"] = json!(null);
        },
        |_| {},
    )
    .unwrap();
}

#[test]
fn an_acf_request_with_more_than_128_arguments_is_never_counted() {
    never_evaluated(
        with_acf(|q| q["argv"] = json!(vec!["x"; 129]), |_| {}, |_| {}),
        &["more than 128", "argv: 129 items"],
        "an ACF request with 129 arguments",
    );
}

#[test]
fn an_acf_request_argument_over_8192_characters_is_never_counted() {
    never_evaluated(
        with_acf(|q| q["argv"] = json!(["x".repeat(8193)]), |_| {}, |_| {}),
        &["longer than 8192", "argv item longer"],
        "an ACF request argument of 8193 characters",
    );
}

/// lib.rs `check_int` / schema.rs `minimum` (each retired against the other).
#[test]
fn an_acf_request_with_a_zero_limit_is_never_counted() {
    never_evaluated(
        with_acf(|q| q["limits"]["cpu_millicores"] = json!(0), |_| {}, |_| {}),
        &["less than the minimum", "outside 1..=2^53-1"],
        "an ACF request with cpu_millicores 0",
    );
}

/// schema.rs `maximum` (retired against the canonical integer rule and
/// `check_int`).
#[test]
fn an_acf_request_with_a_limit_past_two_to_the_53_is_never_counted() {
    never_evaluated(
        with_acf(
            |q| q["limits"]["memory_bytes"] = json!(9_007_199_254_740_992u64),
            |_| {},
            |_| {},
        ),
        &[
            "unsafe JSON integer",
            "greater than the maximum",
            "outside 1..=2^53-1",
        ],
        "an ACF request with memory_bytes 2^53",
    );
}

/// schema.rs `oneOf` (retired against ids.rs `check_opaque`).
#[test]
fn an_acf_request_with_an_empty_approval_is_never_counted() {
    never_evaluated(
        with_acf(|q| q["approval_ref"] = json!(""), |_| {}, |_| {}),
        &["matches 0 of oneOf", "length must be 1..=512"],
        "an ACF request whose approval_ref is empty",
    );
}

/// ids.rs `check_opaque`: the request's subject issuers are typed (no
/// schema), so this is the only check of an issuer's form.
#[test]
fn an_evaluation_naming_an_empty_subject_issuer_is_never_recorded() {
    let (w, mut v) = frozen("issuer");
    v["subject_issuers"] = json!([WORKER, ""]);
    never_evaluated(
        evaluated(&w, &v),
        &["length must be 1..=512"],
        "an evaluation request naming an empty subject issuer",
    );
}

// ── venue TEL: `axon-loop tel summarize` (the binary) over ACF documents ──
//
// The telemetry verb reads Fabric request/receipt pairs and episodes through
// the contracts (`contract_from_value`), then joins them; nothing on it
// re-judges a receipt's verdict, so a contract rule is the one check there.

fn tel(req: &Value) -> Result<(), LoopError> {
    let tmp = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(tmp.path(), req.to_string()).unwrap();
    let o = std::process::Command::new(env!("CARGO_BIN_EXE_axon-loop"))
        .env_remove("AXON_ATTEST_KEY")
        .args(["tel", "summarize", "--in"])
        .arg(tmp.path())
        .output()
        .unwrap();
    if o.status.success() {
        Ok(())
    } else {
        // The verb reports its refusal as JSON on stderr; read the text unescaped.
        let err = String::from_utf8_lossy(&o.stderr).replace("\\\"", "\"");
        Err(LoopError::Refused(err))
    }
}

/// One Fabric attempt (the bundle's request and receipt, priced under a
/// pinned schedule), with `edit_req` / `edit_rc` applied, summarized.
fn tel_attempt(
    edit_req: impl FnOnce(&mut Value),
    edit_rc: impl FnOnce(&mut Value),
) -> Result<(), LoopError> {
    let doc = schedule(json!({}));
    let sref = cl22(&doc);
    let mut req = bundle()["acf_request"].clone();
    req["limits"]["price_schedule_ref"] = json!(sref);
    let mut rc = bundle()["acf_receipt"].clone();
    edit_req(&mut req);
    edit_rc(&mut rc);
    tel(&json!({"schema": "axon.loop.tel-request/1", "episodes": [],
                "price_schedule": {"ref": sref, "document": doc},
                "fabric_attempts": [{"request": req, "receipt": rc}]}))
}

fn never_summarized(r: Result<(), LoopError>, why: &[&str], attack: &str) {
    judged(r, why, attack, "the telemetry was summarized");
}

/// A receipt with a passed verdict, otherwise as the fixture.
fn passed(rc: &mut Value) {
    rc["verification"] = json!("passed");
    rc["matched_checks"] = json!(1);
    rc["evidence_refs"] = json!(["e"]);
}

/// An outcome-unknown receipt.
fn outcome_unknown(rc: &mut Value) {
    rc["status"] = json!("outcome_unknown");
    rc["verification"] = json!("unknown");
    rc["process_exit_code"] = json!(null);
}

#[test]
fn control_honest_receipts_are_summarized() {
    tel_attempt(|_| {}, |_| {}).unwrap();
    tel_attempt(|_| {}, passed).unwrap();
    tel_attempt(|_| {}, |rc| rc["evidence_refs"] = json!(vec!["e"; 128])).unwrap();
    tel_attempt(|_| {}, outcome_unknown).unwrap();
    tel_attempt(
        |_| {},
        |rc| {
            rc["usage_state"] = json!("unknown");
            rc["cost_micro"] = json!(null);
        },
    )
    .unwrap();
}

#[test]
fn a_receipt_with_more_than_128_evidence_refs_is_never_counted() {
    never_summarized(
        tel_attempt(|_| {}, |rc| rc["evidence_refs"] = json!(vec!["e"; 129])),
        &["more than 128", "evidence_refs: max 128"],
        "a receipt with 129 evidence refs",
    );
}

#[test]
fn a_passed_receipt_that_did_not_complete_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                passed(rc);
                rc["status"] = json!("failed");
            },
        ),
        &[
            "must be \"completed\"",
            "verification passed requires status completed",
        ],
        "a passed receipt whose run failed",
    );
}

#[test]
fn a_passed_receipt_with_a_nonzero_exit_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                passed(rc);
                rc["process_exit_code"] = json!(1);
            },
        ),
        &[
            "must be 0",
            "verification passed requires process_exit_code 0",
        ],
        "a passed receipt with exit code 1",
    );
}

#[test]
fn a_passed_receipt_with_no_matched_check_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                passed(rc);
                rc["matched_checks"] = json!(0);
            },
        ),
        &[
            "less than the minimum",
            "verification passed requires matched_checks",
        ],
        "a passed receipt that matched no check",
    );
}

#[test]
fn a_passed_receipt_reported_by_the_worker_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                passed(rc);
                rc["evidence_source"] = json!("worker_reported");
            },
        ),
        &[
            "must be \"supervisor_observed\"",
            "verification passed requires supervisor_observed",
        ],
        "a passed receipt whose evidence the worker reported",
    );
}

#[test]
fn a_passed_receipt_with_no_evidence_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                passed(rc);
                rc["evidence_refs"] = json!([]);
            },
        ),
        &["fewer than 1", "verification passed requires evidence_refs"],
        "a passed receipt with no evidence",
    );
}

#[test]
fn an_outcome_unknown_receipt_with_a_verdict_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                outcome_unknown(rc);
                rc["verification"] = json!("failed");
            },
        ),
        &["is not one of", "status outcome_unknown admits only"],
        "an outcome_unknown receipt with a failed verdict",
    );
}

#[test]
fn an_outcome_unknown_receipt_with_an_exit_code_is_never_counted() {
    never_summarized(
        tel_attempt(
            |_| {},
            |rc| {
                outcome_unknown(rc);
                rc["process_exit_code"] = json!(0);
            },
        ),
        &[
            "is not of type",
            "status outcome_unknown requires process_exit_code null",
        ],
        "an outcome_unknown receipt with an exit code",
    );
}

#[test]
fn a_receipt_with_unknown_usage_and_a_cost_is_never_counted() {
    never_summarized(
        tel_attempt(|_| {}, |rc| rc["usage_state"] = json!("unknown")),
        &["is not of type", "usage_state unknown requires cost_micro"],
        "a receipt with unknown usage and a cost",
    );
}

// ── venue POLICY / TRANSITION: the CLI's `parse` then the store operation ──

fn policy_text(edit: impl FnOnce(&mut Value)) -> String {
    let mut v = serde_json::to_value(incumbent()).unwrap();
    v["policy_id"] = json!("pol-x");
    edit(&mut v);
    v.to_string()
}

fn put_policy_text(text: &str) -> Result<Ref, LoopError> {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p: PolicyEnvelope = parse(text).map_err(LoopError::Malformed)?;
    axon_loop::candidates::put_policy(&s, &p)?;
    Ok(digest(&p).unwrap())
}

#[test]
fn control_an_honest_policy_is_stored() {
    put_policy_text(&policy_text(|_| {})).unwrap();
}

/// schema.rs `const` (red-team D2): the policy's mode spelled
/// `{"shortlist_only":null}`, which typed serde accepts and re-serializes to
/// the digest of the conforming bytes. (The policy route: its schema has no
/// conditional, so removing `const` changes nothing else it reads.)
#[test]
fn a_policy_mode_spelled_as_a_variant_map_is_never_stored() {
    judged(
        put_policy_text(&policy_text(|v| {
            v["mode"] = json!({"shortlist_only": null})
        })),
        &["must be \"shortlist_only\""],
        "a policy whose mode is {\"shortlist_only\":null}",
        "the policy was stored",
    );
}

/// lib.rs schema tag (retired against schema.rs `const`).
#[test]
fn a_policy_of_another_schema_version_is_never_stored() {
    judged(
        put_policy_text(&policy_text(|v| {
            v["schema"] = json!("axon.closed-loop.policy/2")
        })),
        &["must be \"axon.closed-loop.policy/1\"", "schema must be"],
        "a policy of schema axon.closed-loop.policy/2",
        "the policy was stored",
    );
}

#[test]
fn a_policy_claiming_authority_expansion_is_never_stored() {
    judged(
        put_policy_text(&policy_text(|v| v["authority_expansion"] = json!(true))),
        &["must be false", "authority_expansion must be false"],
        "a policy with authority_expansion true",
        "the policy was stored",
    );
}

fn transition_of(v: &Value) -> Result<axon_loop::pointer::PointerRecord, LoopError> {
    let w = world();
    let t: PolicyTransition = parse(&v.to_string()).map_err(LoopError::Malformed)?;
    axon_loop::pointer::transition(&w.s, &t)
}

#[test]
fn control_an_honest_pause_is_applied() {
    let w = world();
    let v = transition("p-1", "pause", &w.inc_ref, None, 1, None, false);
    let t: PolicyTransition = parse(&v.to_string()).unwrap();
    axon_loop::pointer::transition(&w.s, &t).unwrap();
}

#[test]
fn a_pause_naming_a_target_is_never_applied() {
    let w = world();
    let v = transition("p-1", "pause", &w.inc_ref, Some(&w.inc_ref), 1, None, false);
    judged(
        transition_of(&v),
        &["is not of type", "pause must have target_policy_ref = null"],
        "a pause naming a target policy",
        "the transition was applied",
    );
}

// ── the episode's passed-verdict rules (episode.rs), on the TEL route ───────
//
// `tel summarize` reads episodes through the contract and joins only their
// usage, so the passed-verdict rules are the one check there (on the EVL route
// the verification join re-judges the same fields).

/// A passed episode (the fixture's validated trial) with `edit` applied,
/// summarized.
fn tel_episode(edit: impl FnOnce(&mut Value)) -> Result<(), LoopError> {
    let p = incumbent();
    let mut ep = trial(&Trial::new(&p, "task-0", "incumbent", "i0"))["episode"].clone();
    edit(&mut ep);
    tel(&json!({"schema": "axon.loop.tel-request/1", "episodes": [ep]}))
}

#[test]
fn control_an_honest_passed_episode_is_summarized() {
    tel_episode(|_| {}).unwrap();
}

#[test]
fn a_passed_episode_that_did_not_complete_is_never_counted() {
    never_summarized(
        tel_episode(|ep| ep["status"] = json!("failed")),
        &[
            "must be \"completed\"",
            "verification passed requires status completed",
        ],
        "a passed episode whose run failed",
    );
}

#[test]
fn a_passed_episode_with_no_matched_check_is_never_counted() {
    never_summarized(
        tel_episode(|ep| ep["verification"]["matched_checks"] = json!(0)),
        &[
            "less than the minimum",
            "verification passed requires matched_checks",
        ],
        "a passed episode that matched no check",
    );
}

#[test]
fn a_passed_episode_with_no_checked_output_is_never_counted() {
    never_summarized(
        tel_episode(|ep| ep["verification"]["output_workspace_ref"] = json!(null)),
        &[
            "is not of type",
            "verification passed requires output_workspace_ref",
        ],
        "a passed episode naming no checked output",
    );
}

#[test]
fn a_passed_episode_with_no_evidence_is_never_counted() {
    never_summarized(
        tel_episode(|ep| ep["verification"]["evidence_refs"] = json!([])),
        &["fewer than 1", "verification passed requires evidence_refs"],
        "a passed episode citing no evidence",
    );
}

// ── venue PLAN: `PilotPlan::parse` + `plan::register` ───────────────────────
//
// Several pilot fields are plain Rust types (`Vec<Ref>`, `Option<u64>`,
// `Option<String>`) that only the checked-in pilot schema bounds, so there the
// schema walk's keyword is the one check.

fn register_plan(edit: impl FnOnce(&mut Value)) -> Result<Ref, LoopError> {
    let w = world();
    let mut v = complete_plan("exp-p", &w.inc_ref, &w.cand_ref);
    edit(&mut v);
    axon_loop::plan::register(&w.s, &axon_loop::plan::PilotPlan::parse(&v.to_string())?)
}

fn never_planned(r: Result<Ref, LoopError>, why: &[&str], attack: &str) {
    judged(r, why, attack, "the plan was registered");
}

fn refs(n: usize) -> Vec<String> {
    (0..n).map(|i| format!("cl22:{i:064x}")).collect()
}

#[test]
fn control_an_honest_pilot_plan_registers() {
    register_plan(|v| {
        v["live_evidence"] = json!(refs(256));
        v["independent_unit"] = json!("u".repeat(512));
        v["independent_units"] = json!(1);
    })
    .unwrap();
}

#[test]
fn a_plan_with_more_than_256_live_evidence_refs_is_never_registered() {
    never_planned(
        register_plan(|v| v["live_evidence"] = json!(refs(257))),
        &["more than 256"],
        "a pilot plan with 257 live evidence refs",
    );
}

#[test]
fn a_plan_naming_a_live_evidence_ref_twice_is_never_registered() {
    let r = refs(1)[0].clone();
    never_planned(
        register_plan(|v| v["live_evidence"] = json!([r, r])),
        &["duplicate item"],
        "a pilot plan naming one live evidence ref twice",
    );
}

#[test]
fn a_plan_with_zero_independent_units_is_never_registered() {
    never_planned(
        register_plan(|v| v["independent_units"] = json!(0)),
        &["matches none of anyOf"],
        "a pilot plan with zero independent units",
    );
}

#[test]
fn a_plan_with_an_empty_independent_unit_is_never_registered() {
    never_planned(
        register_plan(|v| v["independent_unit"] = json!("")),
        &["matches none of anyOf"],
        "a pilot plan whose independent unit is empty",
    );
}

#[test]
fn a_plan_with_an_independent_unit_over_512_characters_is_never_registered() {
    never_planned(
        register_plan(|v| v["independent_unit"] = json!("u".repeat(513))),
        &["matches none of anyOf"],
        "a pilot plan whose independent unit is 513 characters",
    );
}

/// policy.rs fence (a four-cell pair with pointer.rs's own `next_epoch`
/// check): a transition that skips an epoch.
#[test]
fn a_transition_that_skips_an_epoch_is_never_applied() {
    let w = world();
    let mut v = transition("p-2", "pause", &w.inc_ref, None, 1, None, false);
    v["next_epoch"] = json!(3);
    judged(
        transition_of(&v),
        &[
            "non-monotonic/noncontiguous fence",
            "next_epoch must be expected_epoch + 1",
        ],
        "a transition from epoch 1 to epoch 3",
        "the transition was applied",
    );
}

/// policy.rs `next_epoch >= 1` (retired against the schema's minimum, the
/// fence and pointer.rs's `next_epoch` check): an epoch that goes backwards.
#[test]
fn a_transition_to_epoch_zero_is_never_applied() {
    let w = world();
    let mut v = transition("p-3", "pause", &w.inc_ref, None, 1, None, false);
    v["next_epoch"] = json!(0);
    judged(
        transition_of(&v),
        &[
            "less than the minimum",
            "next_epoch must be >= 1",
            "non-monotonic/noncontiguous fence",
            "next_epoch must be expected_epoch + 1",
        ],
        "a transition from epoch 1 to epoch 0",
        "the transition was applied",
    );
}
