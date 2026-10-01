//! Package fixtures against the Rust contracts.
//!
//! `tests/fixtures/bundle.json` is the v0.22 package's
//! `fixtures/closed_loop_v022/bundle.json`, and `tests/fixtures/acf/*.json` are
//! the ACF reference pack's `contracts/examples/re*.json`, both byte-copied.
//! The schemas under `schemas/` are byte-copied from the package too (see
//! `schemas/README.md`).

use axon_loop_contracts::*;
use serde_json::Value;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    std::fs::read_to_string(root().join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn bundle() -> Value {
    parse_value(&read("tests/fixtures/bundle.json")).expect("bundle parses strictly")
}

fn member(key: &str) -> String {
    serde_json::to_string(&bundle()[key]).unwrap()
}

// ── cl22 digest parity with the package reference ───────────────────────────
//
// Expected values produced by the package's executable reference, not by this
// crate:
//
//   cd <package>/ && python3 - <<'EOF'
//   import sys; sys.path.insert(0,'tools'); import closed_loop_reference as r
//   b=r.strict_json(open('fixtures/closed_loop_v022/bundle.json','rb').read())
//   for k in ['policy','context','episode','transition','acf_request',
//             'acf_receipt','policy_projection']: print(k, r.content_ref(b[k]))
//   EOF
const PY_DIGESTS: &[(&str, &str)] = &[
    (
        "policy",
        "cl22:3a3fb239cf3f65eeca6062e4937b19ba59950d81319635c79bcea701ffb6c1a4",
    ),
    (
        "context",
        "cl22:bc864330d13bce84764f28c84f53e58b2b7c5711f6548710a3462195acf57e13",
    ),
    (
        "episode",
        "cl22:2420ee9aaa3e650f68c3854b5b80abe548c01555fc8d2de6625d07a3d21ee0bd",
    ),
    (
        "transition",
        "cl22:6cde06c6ffbdaf0b166102e9a34bc50afaab2bc08f6dfe48d7be49d38b3dcd7d",
    ),
    (
        "acf_request",
        "cl22:caa9f706872533b122bfa1a240fa9864891a6147264514c69862a13fa8b72e77",
    ),
    (
        "acf_receipt",
        "cl22:6959211b29620f78126f218df7907d93e812a83fef3f1c14ff1ea1c4176ba6b2",
    ),
    (
        "policy_projection",
        "cl22:ea8f30ecef4804f5470eeeb623e94104923234de15ca4fdd65601bfad20bade9",
    ),
];

fn typed_digest(key: &str, json: &str) -> Ref {
    match key {
        "policy" => digest(&parse::<PolicyEnvelope>(json).unwrap()),
        "context" => digest(&parse::<ExecutionContextReceipt>(json).unwrap()),
        "episode" => digest(&parse::<LoopEpisode>(json).unwrap()),
        "transition" => digest(&parse::<PolicyTransition>(json).unwrap()),
        "acf_request" => digest(&parse::<ComputeRequest>(json).unwrap()),
        "acf_receipt" => digest(&parse::<ExecutionReceipt>(json).unwrap()),
        "policy_projection" => digest(&parse::<PolicyProjection>(json).unwrap()),
        _ => unreachable!(),
    }
    .unwrap()
}

#[test]
fn cl22_digests_match_the_python_reference_byte_for_byte() {
    let b = bundle();
    for (key, want) in PY_DIGESTS {
        // Untyped: the canonicaliser alone.
        assert_eq!(
            digest_value(&b[*key]).unwrap().as_str(),
            *want,
            "{key} (value)"
        );
        // Typed: parse → Rust struct → serialize → canonicalise. This is the
        // round trip a producer takes, and it must not perturb the bytes.
        assert_eq!(
            typed_digest(key, &member(key)).as_str(),
            *want,
            "{key} (typed)"
        );
    }
}

#[test]
fn the_bundle_is_internally_bound_by_cl22() {
    // The fixture's own cross-references were produced by the reference's
    // content_ref; recomputing them here is the second, independent witness.
    let b = bundle();
    let ep = &b["episode"];
    assert_eq!(
        ep["policy_ref"],
        digest_value(&b["policy"]).unwrap().as_str()
    );
    assert_eq!(
        ep["context_ref"],
        digest_value(&b["context"]).unwrap().as_str()
    );
    assert_eq!(
        ep["acf_request_ref"],
        digest_value(&b["acf_request"]).unwrap().as_str()
    );
    assert_eq!(
        ep["acf_receipt_ref"],
        digest_value(&b["acf_receipt"]).unwrap().as_str()
    );
}

// ── fixtures that should parse do, and the reference's checks agree ─────────

struct Bundle {
    policy: PolicyEnvelope,
    context: ExecutionContextReceipt,
    episode: LoopEpisode,
    transition: PolicyTransition,
    request: ComputeRequest,
    receipt: ExecutionReceipt,
    projection: PolicyProjection,
}

fn typed() -> Bundle {
    Bundle {
        policy: parse(&member("policy")).unwrap(),
        context: parse(&member("context")).unwrap(),
        episode: parse(&member("episode")).unwrap(),
        transition: parse(&member("transition")).unwrap(),
        request: parse(&member("acf_request")).unwrap(),
        receipt: parse(&member("acf_receipt")).unwrap(),
        projection: parse(&member("policy_projection")).unwrap(),
    }
}

fn set(xs: &[&str]) -> BTreeSet<OpaqueRef> {
    xs.iter().map(|x| OpaqueRef::new(*x).unwrap()).collect()
}

fn epoch(n: u64) -> AuthorityEpoch {
    AuthorityEpoch::new(n).unwrap()
}

#[test]
fn every_bundle_member_parses_and_round_trips() {
    let t = typed();
    // Round trip: serialize → parse gives the same value.
    let again: LoopEpisode = parse(&serde_json::to_string(&t.episode).unwrap()).unwrap();
    assert_eq!(again, t.episode);
    let again: PolicyTransition = parse(&serde_json::to_string(&t.transition).unwrap()).unwrap();
    assert_eq!(again, t.transition);
    assert_eq!(t.policy.version().unwrap().digest, t.episode.policy_ref);
}

// Mirrors the package reference run over the same bundle with the same
// premises, which passes all four and reports learning_eligible False:
//   r.shortlist(b['policy'], eligible={'read','search','edit'}, candidate_set_ref=…,
//               controls_ref=…, scope=…)
//   r.preflight(b['context'], now_ms=150, current_epoch=7,
//               trusted_observers={'fixture:observer'})
//   r.bind_episode(b['episode'], b['policy'], b['context'], current_epoch=7,
//                  trusted_verifiers={'fixture:independent-verifier'},
//                  subject_issuers={'fixture:parent'})
//   r.bind_acf(b['episode'], b['acf_request'], b['acf_receipt'], b['policy_projection'])
//   r.learning_eligible(b['episode'])  # False
#[test]
fn reference_semantic_checks_pass_on_the_bundle() {
    let t = typed();
    let eligible: BTreeSet<CandidateId> = ["read", "search", "edit"]
        .iter()
        .map(|c| CandidateId::new(*c).unwrap())
        .collect();
    let sl = check_shortlist(
        &t.policy,
        &eligible,
        &t.policy.candidate_set_ref,
        &t.policy.controls_ref,
        &t.policy.scope,
    )
    .unwrap();
    assert_eq!(sl.len(), 2);
    check_paired_trial_context(&t.context, 150, epoch(7), &set(&["fixture:observer"])).unwrap();
    bind_episode(
        &t.episode,
        &t.policy,
        &t.context,
        epoch(7),
        &set(&["fixture:independent-verifier"]),
        &set(&["fixture:parent"]),
    )
    .unwrap();
    bind_acf(&t.episode, &t.request, &t.receipt, &t.projection).unwrap();
    // corpus_role is mechanism_test: excluded from learning.
    assert!(!learning_eligible(&t.episode).unwrap());
}

#[test]
fn shortlist_must_be_a_subset_of_the_eligible_view() {
    let t = typed();
    let only_read: BTreeSet<CandidateId> = [CandidateId::new("read").unwrap()].into();
    let e = check_shortlist(
        &t.policy,
        &only_read,
        &t.policy.candidate_set_ref,
        &t.policy.controls_ref,
        &t.policy.scope,
    )
    .unwrap_err();
    assert!(e.to_string().contains("expands eligible"), "{e}");
    let other = Ref::new(format!("cl22:{}", "7".repeat(64))).unwrap();
    let all: BTreeSet<CandidateId> = t.policy.shortlist.iter().cloned().collect();
    assert!(check_shortlist(
        &t.policy,
        &all,
        &other,
        &t.policy.controls_ref,
        &t.policy.scope
    )
    .is_err());
    assert!(check_shortlist(
        &t.policy,
        &all,
        &t.policy.candidate_set_ref,
        &other,
        &t.policy.scope
    )
    .is_err());
}

#[test]
fn paired_trial_requires_exact_context_equality() {
    let mut t = typed();
    let obs = set(&["fixture:observer"]);
    t.context.observed.branch = BoundedText::new("other-branch").unwrap();
    let e = check_paired_trial_context(&t.context, 150, epoch(7), &obs).unwrap_err();
    assert!(e.to_string().contains("TASK_NOT_STARTED"), "{e}");
    // The general (non-paired) check does not demand equality.
    check_context_current(&t.context, 150, epoch(7), &obs).unwrap();
}

#[test]
fn context_currency_and_roles() {
    let t = typed();
    let obs = set(&["fixture:observer"]);
    assert!(check_context_current(&t.context, 99, epoch(7), &obs).is_err());
    assert!(check_context_current(&t.context, 200, epoch(7), &obs).is_err());
    assert!(check_context_current(&t.context, 150, epoch(8), &obs).is_err());
    assert!(check_context_current(&t.context, 150, epoch(7), &set(&["someone"])).is_err());
    let mut echo = t.context.clone();
    echo.observed_issuer_ref = echo.expected_issuer_ref.clone();
    assert!(check_context_current(&echo, 150, epoch(7), &set(&["fixture:parent"])).is_err());
    let mut critic = t.context.clone();
    critic.observed.role = Role::Critic;
    assert!(check_context_current(&critic, 150, epoch(7), &obs).is_err());
    let mut primary = t.context.clone();
    primary.observed.is_primary_worktree = true;
    assert!(check_context_current(&primary, 150, epoch(7), &obs).is_err());
    let mut pattern = t.context.clone();
    pattern.observed.write_paths = vec![BoundedText::new("src/*.rs").unwrap()];
    assert!(check_context_current(&pattern, 150, epoch(7), &obs).is_err());
    for bad in ["/abs", "a/../b", "a//b", "./a", "a\\b", "c:x"] {
        assert!(concrete_path(bad).is_err(), "{bad}");
    }
    concrete_path("src/main.rs").unwrap();
}

#[test]
fn bind_episode_refuses_mismatches() {
    let t = typed();
    let ver = set(&["fixture:independent-verifier"]);
    let subj = set(&["fixture:parent"]);
    // Stale epoch.
    assert!(bind_episode(&t.episode, &t.policy, &t.context, epoch(8), &ver, &subj).is_err());
    // A subject issuer cannot verify its own work.
    assert!(bind_episode(
        &t.episode,
        &t.policy,
        &t.context,
        epoch(7),
        &ver,
        &set(&["fixture:independent-verifier"])
    )
    .is_err());
    // Any byte change in the policy breaks the policy_ref binding.
    let mut p = t.policy.clone();
    p.discovery_evidence_refs
        .push(Ref::new(format!("cl22:{}", "1".repeat(64))).unwrap());
    assert!(
        bind_episode(&t.episode, &p, &t.context, epoch(7), &ver, &subj).is_err(),
        "ATTACK: bind_episode bound an episode to a policy whose bytes it did not run"
    );
    // Identity drift.
    let mut e = t.episode.clone();
    e.identity.attempt_id = AttemptId::new("another-attempt").unwrap();
    assert!(bind_episode(&e, &t.policy, &t.context, epoch(7), &ver, &subj).is_err());
}

#[test]
fn bind_acf_refuses_lost_outcome_and_identity_drift() {
    let t = typed();
    // timed_out never becomes completed. (The episode's acf_receipt_ref pins
    // the receipt bytes, so the digest check fires first; either way refused.)
    let mut r = t.receipt.clone();
    r.status = ReceiptStatus::TimedOut;
    r.process_exit_code = None;
    assert!(bind_acf(&t.episode, &t.request, &r, &t.projection).is_err());
    assert_eq!(
        project_receipt_status(ReceiptStatus::TimedOut),
        EpisodeStatus::OutcomeUnknown
    );
    assert_eq!(
        project_receipt_status(ReceiptStatus::Canceled),
        EpisodeStatus::Cancelled
    );
    assert_eq!(
        project_receipt_status(ReceiptStatus::Denied),
        EpisodeStatus::Refused
    );
    let mut p = t.projection.clone();
    p.sidecar_policy_ref = Ref::new(format!("cl22:{}", "0".repeat(64))).unwrap();
    assert!(bind_acf(&t.episode, &t.request, &t.receipt, &p).is_err());
}

// ── intrinsic semantic rules, enforced by parse ─────────────────────────────

fn edit(key: &str, f: impl FnOnce(&mut Value)) -> String {
    let mut v = bundle()[key].clone();
    f(&mut v);
    serde_json::to_string(&v).unwrap()
}

#[test]
fn authority_expansion_must_be_false() {
    // Refused by the schema walk (`"const": false`) before typed serde...
    let j = edit("policy", |v| v["authority_expansion"] = true.into());
    let e = parse::<PolicyEnvelope>(&j).unwrap_err();
    assert!(e.to_string().contains("authority_expansion"), "{e}");
    // ...and independently by the typed layer, for values built in code.
    let mut p: PolicyEnvelope = parse(&member("policy")).unwrap();
    p.authority_expansion = true;
    assert!(matches!(p.validate(), Err(Refusal::Semantic(_))));
}

#[test]
fn shortlist_non_empty_and_unique() {
    let j = edit("policy", |v| v["shortlist"] = serde_json::json!([]));
    assert!(parse::<PolicyEnvelope>(&j).is_err());
    let j = edit("policy", |v| {
        v["shortlist"] = serde_json::json!(["read", "read"])
    });
    assert!(parse::<PolicyEnvelope>(&j).is_err());
}

#[test]
fn transition_epoch_must_be_contiguous() {
    let j = edit("transition", |v| v["next_epoch"] = 9.into());
    assert!(matches!(
        parse::<PolicyTransition>(&j),
        Err(Refusal::Semantic(_))
    ));
    let j = edit("transition", |v| v["next_epoch"] = 7.into());
    assert!(parse::<PolicyTransition>(&j).is_err());
}

#[test]
fn rollback_requires_target_and_admission() {
    let base = |f: &dyn Fn(&mut Value)| {
        edit("transition", |v| {
            v["kind"] = "rollback".into();
            f(v)
        })
    };
    assert!(parse::<PolicyTransition>(&base(&|_| {})).is_ok());
    assert!(parse::<PolicyTransition>(&base(&|v| v["admission_ref"] = Value::Null)).is_err());
    assert!(parse::<PolicyTransition>(&base(&|v| v["target_policy_ref"] = Value::Null)).is_err());
    // Pause must NOT carry a target, and may omit admission.
    let pause = edit("transition", |v| {
        v["kind"] = "pause".into();
        v["target_policy_ref"] = Value::Null;
        v["admission_ref"] = Value::Null;
    });
    assert!(parse::<PolicyTransition>(&pause).is_ok());
    let pause_t = edit("transition", |v| v["kind"] = "pause".into());
    assert!(parse::<PolicyTransition>(&pause_t).is_err());
}

#[test]
fn episode_post_preflight_requires_context_and_operation_ids() {
    let j = edit("episode", |v| {
        v.as_object_mut().unwrap().remove("context_ref");
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("episode", |v| v["context_ref"] = Value::Null);
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("episode", |v| {
        v["identity"]
            .as_object_mut()
            .unwrap()
            .remove("operation_id");
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
}

#[test]
fn verification_passed_requires_matched_checks() {
    let j = edit("episode", |v| {
        v["verification"]["matched_checks"] = 0.into()
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("episode", |v| v["status"] = "failed".into());
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("episode", |v| {
        v["verification"]["evidence_refs"] = serde_json::json!([])
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
}

#[test]
fn unknown_cost_is_null_never_zero() {
    // final requires a known cost...
    let j = edit("episode", |v| v["usage"]["cost_micro"] = Value::Null);
    assert!(parse::<LoopEpisode>(&j).is_err());
    // ...and no unresolved liability.
    let j = edit("episode", |v| {
        v["usage"]["unresolved_liability_micro"] = 1.into()
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
    // unknown requires null, and refuses 0.
    let j = edit("episode", |v| v["usage"]["state"] = "unknown".into());
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("episode", |v| {
        v["usage"]["state"] = "unknown".into();
        v["usage"]["cost_micro"] = Value::Null;
    });
    let ep: LoopEpisode = parse(&j).unwrap();
    assert_eq!(ep.usage.cost_micro, None);
    // Absent is not null: the field is required.
    let j = edit("episode", |v| {
        v["usage"]["state"] = "unknown".into();
        v["usage"].as_object_mut().unwrap().remove("cost_micro");
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
}

// ── strict-ingest mutations are refused ─────────────────────────────────────

const MEMBERS: &[&str] = &[
    "policy",
    "context",
    "episode",
    "transition",
    "acf_request",
    "acf_receipt",
    "policy_projection",
];

fn parse_member(key: &str, json: &str) -> Result<(), Refusal> {
    match key {
        "policy" => parse::<PolicyEnvelope>(json).map(drop),
        "context" => parse::<ExecutionContextReceipt>(json).map(drop),
        "episode" => parse::<LoopEpisode>(json).map(drop),
        "transition" => parse::<PolicyTransition>(json).map(drop),
        "acf_request" => parse::<ComputeRequest>(json).map(drop),
        "acf_receipt" => parse::<ExecutionReceipt>(json).map(drop),
        "policy_projection" => parse::<PolicyProjection>(json).map(drop),
        _ => unreachable!(),
    }
}

#[test]
fn unknown_field_is_refused_everywhere() {
    for key in MEMBERS {
        let j = edit(key, |v| v["sudo"] = true.into());
        let e = parse_member(key, &j).unwrap_err();
        assert!(matches!(e, Refusal::Strict(_)), "{key}: {e}");
    }
    // Nested objects are closed too.
    let j = edit("context", |v| v["observed"]["extra"] = 1.into());
    assert!(parse::<ExecutionContextReceipt>(&j).is_err());
    let j = edit("acf_request", |v| v["limits"]["gpu"] = 1.into());
    assert!(parse::<ComputeRequest>(&j).is_err());
}

#[test]
fn duplicate_and_escaped_alias_keys_are_refused() {
    for key in MEMBERS {
        let j = member(key);
        let first = j.find("\":").unwrap();
        let k_start = j[..first].rfind('"').unwrap() + 1;
        let k = &j[k_start..first];
        // Literal duplicate at the front of the object.
        let dup = format!("{{\"{k}\":null,{}", &j[1..]);
        assert!(
            matches!(
                parse_member(key, &dup),
                Err(Refusal::Strict(axon_cortex::ContractError::DuplicateKey(_)))
            ),
            "{key}: literal duplicate"
        );
        // The same key spelled with a \u escape.
        let esc = format!("\\u{:04x}{}", k.as_bytes()[0], &k[1..]);
        let alias = format!("{{\"{esc}\":null,{}", &j[1..]);
        assert!(
            matches!(
                parse_member(key, &alias),
                Err(Refusal::Strict(axon_cortex::ContractError::DuplicateKey(_)))
            ),
            "{key}: escaped alias"
        );
    }
}

#[test]
fn floats_unsafe_ints_and_bools_as_ints_are_refused() {
    let j = edit("episode", |v| v["authority_epoch"] = serde_json::json!(7.0));
    assert!(matches!(parse::<LoopEpisode>(&j), Err(Refusal::Float)));
    let j = member("context").replace("\"created_ms\":100", "\"created_ms\":1e2");
    assert!(matches!(
        parse::<ExecutionContextReceipt>(&j),
        Err(Refusal::Float)
    ));
    let j = member("acf_request").replace(
        "\"wall_time_ms\":30000",
        "\"wall_time_ms\":9007199254740992",
    );
    assert!(matches!(
        parse::<ComputeRequest>(&j),
        Err(Refusal::UnsafeInteger(_))
    ));
    let j = member("acf_request").replace(
        "\"wall_time_ms\":30000",
        "\"wall_time_ms\":99999999999999999999",
    );
    assert!(parse::<ComputeRequest>(&j).is_err());
    let j = edit("episode", |v| v["usage"]["cost_micro"] = true.into());
    assert!(matches!(parse::<LoopEpisode>(&j), Err(Refusal::Shape(_))));
    let j = edit("transition", |v| v["mechanism_test"] = 1.into());
    assert!(parse::<PolicyTransition>(&j).is_err());
    let j = edit("episode", |v| v["usage"]["cost_micro"] = (-1).into());
    assert!(parse::<LoopEpisode>(&j).is_err());
}

#[test]
fn bad_id_and_ref_charsets_are_refused() {
    for bad in ["has space", "", ".lead", "a/b", "ümlaut"] {
        let j = edit("episode", |v| v["identity"]["trial_id"] = bad.into());
        assert!(
            matches!(parse::<LoopEpisode>(&j), Err(Refusal::Shape(_))),
            "{bad:?}"
        );
    }
    let j = edit("episode", |v| {
        v["identity"]["task_id"] = "x".repeat(129).into()
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("policy", |v| v["shortlist"][0] = "bad id".into());
    assert!(parse::<PolicyEnvelope>(&j).is_err());
    let j = edit("policy", |v| {
        v["controls_ref"] = format!("axc1:{}", "2".repeat(64)).into()
    });
    assert!(parse::<PolicyEnvelope>(&j).is_err());
    // workspace refs are acf1-only.
    let j = edit("episode", |v| {
        v["input_workspace_ref"] = format!("cl22:{}", "a".repeat(64)).into()
    });
    assert!(parse::<LoopEpisode>(&j).is_err());
    let j = edit("policy", |v| {
        v["schema"] = "axon.closed-loop.policy/2".into()
    });
    assert!(parse::<PolicyEnvelope>(&j).is_err());
    let j = edit("policy", |v| v["mode"] = "rank_all".into());
    assert!(parse::<PolicyEnvelope>(&j).is_err());
}

#[test]
fn depth_and_size_limits() {
    // Well under serde_json's own recursion limit (128), so only the
    // contract's depth rule can be what refuses it.
    let deep = format!("{}1{}", "[".repeat(40), "]".repeat(40));
    let raw = member("acf_request").replace("\"argv\":[]", &format!("\"argv\":{deep}"));
    assert!(matches!(
        parse::<ComputeRequest>(&raw),
        Err(Refusal::TooDeep)
    ));
    let big = member("acf_request").replace(
        "\"argv\":[]",
        &format!("\"argv\":[\"{}\"]", "a".repeat(MAX_BYTES)),
    );
    assert!(matches!(
        parse::<ComputeRequest>(&big),
        Err(Refusal::TooLarge(_))
    ));
}

// ── ACF reference-pack examples ─────────────────────────────────────────────

#[test]
fn acf_examples_agree_with_the_reference_validator() {
    // Verdicts from the package reference:
    //   strict_json + validate_acf over contracts/examples/*.json
    let expect: &[(&str, bool)] = &[
        ("receipt_exit_zero_unverified.json", true),
        ("receipt_outcome_unknown.json", true),
        ("receipt_unknown_cost_zero.json", false),
        ("receipt_unknown_success.json", false),
        ("receipt_worker_claims_verified.json", false),
        ("receipt_zero_checks_pass.json", false),
        ("request_extra_field.json", false),
        ("request_float_budget.json", false),
        ("request_missing_trial.json", false),
        ("request_negative_budget.json", false),
        ("request_offline.json", true),
        ("request_unknown_checkpoint.json", false),
        ("request_unsafe_integer.json", false),
    ];
    for (name, ok) in expect {
        let j = read(&format!("tests/fixtures/acf/{name}"));
        let got = if name.starts_with("request") {
            parse::<ComputeRequest>(&j).map(drop)
        } else {
            parse::<ExecutionReceipt>(&j).map(drop)
        };
        assert_eq!(got.is_ok(), *ok, "{name}: {got:?}");
    }
}

// ── the checked-in schemas agree with the Rust types ────────────────────────

/// Walk schema and serialized value together: at every object the schema
/// describes, the serialized keys must equal the schema's `properties`, and
/// every schema `required` key must be present. `additionalProperties: false`
/// must hold at every such object.
fn agree(schema: &Value, value: &Value, path: &str) {
    let Some(props) = schema.get("properties").and_then(Value::as_object) else {
        return;
    };
    assert_eq!(
        schema.get("additionalProperties"),
        Some(&Value::Bool(false)),
        "{path}: schema object is not closed"
    );
    let obj = value
        .as_object()
        .unwrap_or_else(|| panic!("{path}: not an object"));
    let got: BTreeSet<&String> = obj.keys().collect();
    let want: BTreeSet<&String> = props.keys().collect();
    assert_eq!(got, want, "{path}: serialized keys != schema properties");
    for r in schema["required"].as_array().unwrap() {
        assert!(
            obj.contains_key(r.as_str().unwrap()),
            "{path}: missing required {r}"
        );
    }
    for (k, sub) in props {
        agree(sub, &obj[k], &format!("{path}.{k}"));
    }
}

#[test]
fn checked_in_schemas_agree_with_rust_serialization() {
    let t = typed();
    let cases: &[(&str, Value)] = &[
        (
            "closed-loop-policy",
            serde_json::to_value(&t.policy).unwrap(),
        ),
        (
            "closed-loop-context",
            serde_json::to_value(&t.context).unwrap(),
        ),
        (
            "closed-loop-episode",
            serde_json::to_value(&t.episode).unwrap(),
        ),
        (
            "closed-loop-transition",
            serde_json::to_value(&t.transition).unwrap(),
        ),
        (
            "acf-compute-request",
            serde_json::to_value(&t.request).unwrap(),
        ),
        (
            "acf-execution-receipt",
            serde_json::to_value(&t.receipt).unwrap(),
        ),
    ];
    for (name, value) in cases {
        let schema = parse_value(&read(&format!("schemas/{name}.schema.json"))).unwrap();
        agree(&schema, value, name);
        // The schema tag the Rust type emits is the schema's const.
        assert_eq!(
            schema["properties"]["schema"]["const"], value["schema"],
            "{name}"
        );
    }
}

#[test]
fn checked_in_schemas_are_the_package_bytes() {
    // sha256 of the package files at copy time (sha256sum over
    // <package>/schemas/json/closed-loop-*.schema.json and
    // <package>/integration/compute-fabric-v0_1-reference/contracts/*.schema.json).
    // A local edit to a schema must be a deliberate, recorded divergence.
    use sha2::{Digest, Sha256};
    let pins: &[(&str, &str)] = &[
        (
            "acf-compute-request",
            "ca63df26dca6fb6de98b883d01c58203f217e30738b828c4ba31667131dd2177",
        ),
        (
            "acf-execution-receipt",
            "f58e17e49ab1c9d10a16278e8bf66bbf0de9516d3659854d79d98f2c9b0f47f6",
        ),
        (
            "closed-loop-context",
            "4feefdbc944bcd7f8bb7e367237960323b52b683f2c7bbd2bfca35d76796b50a",
        ),
        (
            "closed-loop-episode",
            "2db4ce8e894689e7ea3f816034dd443151a5af9e79f841892a681725cf37cef6",
        ),
        (
            "closed-loop-policy",
            "f911f9053bc1dc815ac9df57a18bbb54140fd4a7a2d61687f25284fd01ff2e68",
        ),
        (
            "closed-loop-transition",
            "e19cde7515ee29fc489f95e611494e2522685e4d477d7d285f423780f3acba21",
        ),
    ];
    for (name, want) in pins {
        let bytes = std::fs::read(root().join(format!("schemas/{name}.schema.json"))).unwrap();
        assert_eq!(format!("{:x}", Sha256::digest(&bytes)), *want, "{name}");
    }
}
