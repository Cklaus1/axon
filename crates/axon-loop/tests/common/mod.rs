//! Shared fixtures. Every document is derived from the v0.22 package bundle
//! (byte-copied in axon-loop-contracts/tests/fixtures/bundle.json), mutated
//! through the contract TYPES, with every cross-reference digest recomputed so
//! the documents genuinely bind. Fixture identities are test premises, not
//! credentials.
#![allow(dead_code)]

use axon_loop::admission::{AdmissionRecord, AdmissionSchema, ArmFacts, Decision};
use axon_loop::store::{Config, ConfigSchema, Store};
use axon_loop::tel::Total;
use axon_loop_contracts::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ADMITTER: &str = "op:admitter";
pub const VERIFIER: &str = "fixture:independent-verifier";
pub const PROPOSER: &str = "agent:proposer";
pub const EVALUATOR: &str = "evl:evaluator";
pub const WORKER: &str = "agent:worker";

pub fn bundle() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../axon-loop-contracts/tests/fixtures/bundle.json");
    parse_value(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn member<T: Contract>(k: &str) -> T {
    parse(&serde_json::to_string(&bundle()[k]).unwrap()).unwrap()
}

pub fn scope() -> Scope {
    Scope {
        tenant_id: TenantId::new("fixture-tenant").unwrap(),
        task_family: TaskFamily::new("fixture-coding").unwrap(),
    }
}

pub fn r(c: char) -> Ref {
    Ref::new(format!("cl22:{}", c.to_string().repeat(64))).unwrap()
}

pub fn cid(s: &str) -> CandidateId {
    CandidateId::new(s).unwrap()
}

pub fn ids(xs: &[&str]) -> Vec<CandidateId> {
    xs.iter().map(|x| cid(x)).collect()
}

pub fn eligible() -> Vec<CandidateId> {
    ids(&["read", "search", "edit", "write"])
}

pub fn policy(id: &str, shortlist: &[&str]) -> PolicyEnvelope {
    let mut p: PolicyEnvelope = member("policy");
    p.policy_id = PolicyId::new(id).unwrap();
    p.shortlist = ids(shortlist);
    p
}

pub fn incumbent() -> PolicyEnvelope {
    policy("incumbent", &["read", "search", "edit"])
}

pub fn store_with_config(dir: &Path) -> Store {
    let s = Store::open(dir).unwrap();
    s.write_config(&Config {
        schema: ConfigSchema,
        trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
        trusted_verifiers: vec![OpaqueRef::new(VERIFIER).unwrap()],
    })
    .unwrap();
    s
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Out {
    Pass,
    Fail,
    Unknown,
}

pub struct Trial<'a> {
    pub policy: &'a PolicyEnvelope,
    pub task: &'a str,
    pub arm: &'a str,
    pub trial: &'a str,
    pub role: CorpusRole,
    pub out: Out,
    pub cost: Option<u64>,
    pub liability: u64,
    pub epoch: u64,
    pub verifier: &'a str,
}

impl<'a> Trial<'a> {
    pub fn new(policy: &'a PolicyEnvelope, task: &'a str, arm: &'a str, trial: &'a str) -> Self {
        Trial {
            policy,
            task,
            arm,
            trial,
            role: CorpusRole::Confirmation,
            out: Out::Pass,
            cost: Some(100),
            liability: 0,
            epoch: 0,
            verifier: VERIFIER,
        }
    }
}

/// A delivered trial `{episode, context, acf_request, acf_receipt, projection}`
/// whose references all bind.
pub fn trial(t: &Trial) -> Value {
    let identity = TrialIdentity {
        task_id: TaskId::new(t.task).unwrap(),
        arm_id: ArmId::new(t.arm).unwrap(),
        trial_id: TrialId::new(t.trial).unwrap(),
        attempt_id: AttemptId::new(format!("{}-a1", t.trial)).unwrap(),
        operation_id: OperationId::new(format!("{}-op", t.trial)).unwrap(),
        execution_id: ExecutionId::new(format!("{}-ex", t.trial)).unwrap(),
    };
    let epoch = AuthorityEpoch::new(t.epoch).unwrap();
    let pref = digest(t.policy).unwrap();

    let mut ctx: ExecutionContextReceipt = member("context");
    ctx.identity = identity.clone();
    ctx.authority_epoch = epoch;

    let mut req: ComputeRequest = member("acf_request");
    req.task_id = identity.task_id.clone();
    req.trial_id = identity.trial_id.clone();
    req.attempt_id = identity.attempt_id.clone();
    req.operation_id = identity.operation_id.clone();

    let mut rc: ExecutionReceipt = member("acf_receipt");
    rc.task_id = identity.task_id.clone();
    rc.trial_id = identity.trial_id.clone();
    rc.attempt_id = identity.attempt_id.clone();
    rc.operation_id = identity.operation_id.clone();
    rc.execution_id = identity.execution_id.clone();

    let mut proj: PolicyProjection = member("policy_projection");
    proj.sidecar_policy_ref = pref.clone();

    let mut ep: LoopEpisode = member("episode");
    ep.identity = identity;
    ep.policy_ref = pref;
    ep.authority_epoch = epoch;
    ep.corpus_role = t.role;
    ep.verification.issuer_ref = Some(OpaqueRef::new(t.verifier).unwrap());
    ep.usage.attempt_refs = vec![digest(&t.trial.to_string()).unwrap()];
    ep.usage.unresolved_liability_micro = t.liability;
    match t.cost {
        Some(c) if t.liability == 0 => {
            ep.usage.state = UsageState::Final;
            ep.usage.cost_micro = Some(c);
        }
        Some(c) => {
            ep.usage.state = UsageState::Estimated;
            ep.usage.cost_micro = Some(c);
        }
        None => {
            ep.usage.state = UsageState::Unknown;
            ep.usage.cost_micro = None;
        }
    }
    match t.out {
        Out::Pass => {}
        Out::Fail => {
            ep.verification.result = VerificationResult::Failed;
            ep.verification.evidence_refs = vec![];
        }
        Out::Unknown => {
            ep.status = EpisodeStatus::OutcomeUnknown;
            ep.verification.result = VerificationResult::Unknown;
            rc.status = ReceiptStatus::TimedOut;
            rc.process_exit_code = None;
        }
    }
    ep.context_ref = digest(&ctx).unwrap();
    ep.acf_request_ref = digest(&req).unwrap();
    ep.acf_receipt_ref = digest(&rc).unwrap();
    ep.validate().unwrap();
    json!({"episode": ep, "context": ctx, "acf_request": req, "acf_receipt": rc, "projection": proj})
}

/// A learning-eligible discovery episode (for EVO input).
pub fn discovery_episode(p: &PolicyEnvelope, trial_id: &str) -> Value {
    let mut t = Trial::new(p, "disc-task", "incumbent", trial_id);
    t.role = CorpusRole::Discovery;
    trial(&t)["episode"].clone()
}

pub fn episode_with_role(p: &PolicyEnvelope, trial_id: &str, role: CorpusRole) -> Value {
    let mut t = Trial::new(p, "disc-task", "incumbent", trial_id);
    t.role = role;
    trial(&t)["episode"].clone()
}

pub fn evo_request(inc: &PolicyEnvelope, seed: u64, new_id: &str, episodes: Vec<Value>) -> Value {
    json!({
        "schema": "axon.loop.evo-request/1",
        "incumbent": inc,
        "eligible": eligible(),
        "episodes": episodes,
        "seed": seed,
        "new_policy_id": new_id,
        "proposer_ref": PROPOSER,
        "rationale": "drop a rarely useful tool from the shortlist"
    })
}

/// A complete, approved plan in the executable rule grammar.
pub fn complete_plan(id: &str, inc: &Ref, cand: &Ref) -> Value {
    let mut p: Value =
        serde_json::from_str(include_str!("../fixtures/pilot-template.json")).unwrap();
    let o = p.as_object_mut().unwrap();
    o.insert("experiment_id".into(), json!(id));
    o.insert("runtime_ready".into(), json!(true));
    o.insert("deployment_enabled".into(), json!(true));
    o.insert("operator_approved".into(), json!(true));
    for (k, c) in [
        ("task_manifest_ref", '1'),
        ("repository_split_ref", '3'),
        ("discovery_manifest_ref", '4'),
        ("confirmation_manifest_ref", '5'),
        ("reporting_manifest_ref", '6'),
        ("analysis_method_ref", '7'),
        ("data_use_ref", 'f'),
        ("rollback_policy_ref", '8'),
        ("approval_ref", '9'),
    ] {
        o.insert(k.into(), json!(r(c)));
    }
    o.insert("controls_ref".into(), json!(incumbent().controls_ref));
    o.insert("incumbent_policy_ref".into(), json!(inc));
    o.insert("candidate_policy_ref".into(), json!(cand));
    o.insert("independent_units".into(), json!(2));
    o.insert("repetitions".into(), json!(1));
    o.insert("candidate_budget".into(), json!(1));
    o.insert("independent_unit".into(), json!("task"));
    o.insert("quality_margin".into(), json!("pass_rate_margin_ppm=0"));
    o.insert(
        "economic_threshold".into(),
        json!("min_cost_reduction_ppm=100000"),
    );
    o.insert("uncertainty_rule".into(), json!("exact_bounds"));
    o.insert("missing_data_rule".into(), json!("unknown_bounds"));
    o.insert("multiplicity_rule".into(), json!("single_candidate"));
    o.insert("order_rule".into(), json!("alternate"));
    o.insert("cache_rule".into(), json!("cold"));
    o.insert(
        "budget_rule".into(),
        json!("max_unresolved_liability_micro=0"),
    );
    p
}

/// Seed an admission record directly (pointer tests that are not about the
/// admission rule itself). Returns its ref.
pub fn seed_admission(
    store: &Store,
    target: &PolicyEnvelope,
    decision: Decision,
    mechanism_test: bool,
    deployment_enabled: bool,
) -> Ref {
    store.put_cas("policies", target).unwrap();
    let t = digest(target).unwrap();
    let facts = ArmFacts {
        policy_ref: t.clone(),
        assigned: 1,
        distinct_tasks: 1,
        verified_pass: 1,
        fail: 0,
        unknown: 0,
        total: Total::Known { cost_micro: 1 },
    };
    let rec = AdmissionRecord {
        schema: AdmissionSchema,
        decision,
        reasons: vec!["seeded by test".into()],
        scope: scope(),
        experiment_id: "seed".into(),
        plan_ref: r('9'),
        evaluation_ref: r('8'),
        target_policy_ref: t,
        incumbent_policy_ref: r('0'),
        controls_ref: target.controls_ref.clone(),
        admitter_ref: OpaqueRef::new(ADMITTER).unwrap(),
        proposer_ref: None,
        evaluator_ref: OpaqueRef::new(EVALUATOR).unwrap(),
        mechanism_test,
        deployment_enabled,
        evaluated_at_epoch: AuthorityEpoch::new(0).unwrap(),
        candidate: facts.clone(),
        incumbent: facts,
        evidence_refs: vec![],
    };
    store.put_cas("admissions", &rec).unwrap()
}

pub fn transition(
    id: &str,
    kind: &str,
    expected: &Ref,
    target: Option<&Ref>,
    expected_epoch: u64,
    admission: Option<&Ref>,
    mechanism_test: bool,
) -> Value {
    json!({
        "schema": "axon.closed-loop.transition/1",
        "transition_id": id,
        "kind": kind,
        "scope": scope(),
        "expected_policy_ref": expected,
        "target_policy_ref": target,
        "expected_epoch": expected_epoch,
        "next_epoch": expected_epoch + 1,
        "admission_ref": admission,
        "reason_ref": r('e'),
        "issuer_ref": ADMITTER,
        "mechanism_test": mechanism_test
    })
}

pub fn tparse(v: &Value) -> PolicyTransition {
    parse(&serde_json::to_string(v).unwrap()).unwrap()
}

/// Snapshot every file under a directory (path → bytes) for no-change asserts.
pub fn snapshot(dir: &Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut m = std::collections::BTreeMap::new();
    fn walk(d: &Path, m: &mut std::collections::BTreeMap<PathBuf, Vec<u8>>) {
        if let Ok(rd) = std::fs::read_dir(d) {
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(&p, m);
                } else if p.file_name().is_some_and(|n| n != "lock") {
                    m.insert(p.clone(), std::fs::read(&p).unwrap());
                }
            }
        }
    }
    walk(dir, &mut m);
    m
}
