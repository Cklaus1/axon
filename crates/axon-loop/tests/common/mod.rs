//! Shared fixtures. Every document is derived from the v0.22 package bundle
//! (byte-copied in axon-loop-contracts/tests/fixtures/bundle.json), mutated
//! through the contract TYPES, with every cross-reference digest recomputed so
//! the documents genuinely bind. Fixture identities are test premises, not
//! credentials.
#![allow(dead_code)]

use axon_loop::store::{Config, ConfigSchema, Store};
use axon_loop_contracts::*;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const ADMITTER: &str = "op:admitter";
pub const VERIFIER: &str = "fixture:independent-verifier";
pub const PROPOSER: &str = "agent:proposer";
pub const EVALUATOR: &str = "evl:evaluator";
pub const WORKER: &str = "agent:worker";
/// The bundle context's independent observer (its parent is `fixture:parent`).
pub const OBSERVER: &str = "fixture:observer";

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

/// The fixture candidate view: a REAL list (sorted), whose `cl22:` every
/// fixture policy carries as `candidate_set_ref`, registered in every
/// fixture store by [`store_with_config`] (G2).
pub fn candidate_list() -> Vec<CandidateId> {
    let mut v = eligible();
    v.sort();
    v
}

pub fn candidate_set_ref() -> Ref {
    let names: Vec<String> = candidate_list().iter().map(|c| c.to_string()).collect();
    digest_value(&json!(names)).unwrap()
}

pub fn candidate_set_doc(list: &[CandidateId]) -> Value {
    json!({"schema":"axon.loop.candidate-set/1","scope":scope(),"candidates":list,"issuer_ref":ADMITTER})
}

pub fn register_candidates(s: &Store) -> Ref {
    let c = axon_loop::candidates::CandidateSet::parse(
        &candidate_set_doc(&candidate_list()).to_string(),
    )
    .unwrap();
    axon_loop::candidates::put(s, &c).unwrap()
}

pub fn policy(id: &str, shortlist: &[&str]) -> PolicyEnvelope {
    let mut p: PolicyEnvelope = member("policy");
    p.policy_id = PolicyId::new(id).unwrap();
    p.shortlist = ids(shortlist);
    p.candidate_set_ref = candidate_set_ref();
    p
}

pub fn incumbent() -> PolicyEnvelope {
    policy("incumbent", &["read", "search", "edit"])
}

pub fn store_with_config(dir: &Path) -> Store {
    let s = Store::open_dir(dir).unwrap();
    s.write_config(&Config {
        schema: ConfigSchema,
        trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
        trusted_verifiers: vec![OpaqueRef::new(VERIFIER).unwrap()],
        trusted_observers: vec![OpaqueRef::new(OBSERVER).unwrap()],
    })
    .unwrap();
    register_candidates(&s);
    register_tasks(&s, 2);
    s
}

/// A configured store with NO candidate list registered (G2 negatives).
pub fn store_without_candidates(dir: &Path) -> Store {
    let s = Store::open_dir(dir).unwrap();
    s.write_config(&Config {
        schema: ConfigSchema,
        trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
        trusted_verifiers: vec![OpaqueRef::new(VERIFIER).unwrap()],
        trusted_observers: vec![OpaqueRef::new(OBSERVER).unwrap()],
    })
    .unwrap();
    // The lock file is created on first use; create it now so no-change
    // snapshots compare data only.
    drop(axon_loop::ledger::Tx::begin(&s).unwrap());
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
    /// Preflight time; default: now (after any freeze made before the call).
    pub created_ms: Option<u64>,
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
            epoch: 1,
            verifier: VERIFIER,
            created_ms: None,
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
    ctx.created_ms = t.created_ms.unwrap_or_else(axon_loop::now_ms);
    ctx.expires_ms = ctx.created_ms + 3_600_000;

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
    ep.candidate_set_ref = t.policy.candidate_set_ref.clone();
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
    t.created_ms = Some(1_000);
    trial(&t)["episode"].clone()
}

pub fn episode_with_role(p: &PolicyEnvelope, trial_id: &str, role: CorpusRole) -> Value {
    let mut t = Trial::new(p, "disc-task", "incumbent", trial_id);
    t.role = role;
    t.created_ms = Some(1_000);
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
    o.insert("task_manifest_ref".into(), json!(task_manifest_ref(2)));
    o.insert("controls_ref".into(), json!(incumbent().controls_ref));
    o.insert("incumbent_policy_ref".into(), json!(inc));
    o.insert("candidate_policy_ref".into(), json!(cand));
    o.insert("independent_units".into(), json!(2));
    o.insert("repetitions".into(), json!(1));
    o.insert("candidate_budget".into(), json!(1));
    o.insert("independent_unit".into(), json!("task"));
    o.insert("order_rule".into(), json!("paired_tasks"));
    o.insert("cache_rule".into(), json!("not_enforced_here"));
    o.insert("quality_margin".into(), json!("pass_rate_margin_ppm=0"));
    o.insert(
        "economic_threshold".into(),
        json!("min_cost_reduction_ppm=100000"),
    );
    o.insert("uncertainty_rule".into(), json!("exact_bounds"));
    o.insert("missing_data_rule".into(), json!("unknown_bounds"));
    o.insert("multiplicity_rule".into(), json!("single_candidate"));
    o.insert(
        "budget_rule".into(),
        json!("max_unresolved_liability_micro=0"),
    );
    p
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

// ── the real flow ─────────────────────────────────────────────────────────

use axon_loop::admission::{self, AdmissionRecord};
use axon_loop::error::LoopError;
use axon_loop::plan::{self, PilotPlan};
use axon_loop::{evl, evo, null_policy_ref, pointer};

pub struct World {
    pub dir: tempfile::TempDir,
    pub s: Store,
    pub inc: PolicyEnvelope,
    pub inc_ref: Ref,
    pub baseline: Ref,
    pub cand: PolicyEnvelope,
    pub cand_ref: Ref,
}

pub fn baseline_doc(p: &Ref) -> Value {
    json!({"schema":"axon.loop.baseline/1","scope":scope(),"policy_ref":p,
           "issuer_ref":ADMITTER,"reason_ref":r('b')})
}

/// Store with config; incumbent stored, designated incumbent-of-record and
/// activated (epoch 1); one EVO candidate proposed from it.
pub fn world() -> World {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config(dir.path());
    let inc = incumbent();
    let inc_ref = s.put_cas("policies", &inc).unwrap();
    let baseline = pointer::designate_baseline(
        &s,
        &pointer::parse_baseline(&baseline_doc(&inc_ref).to_string()).unwrap(),
    )
    .unwrap();
    pointer::transition(
        &s,
        &tparse(&transition(
            "boot",
            "activate",
            &null_policy_ref(),
            Some(&inc_ref),
            0,
            Some(&baseline),
            false,
        )),
    )
    .unwrap();
    let (cand, cand_ref) = propose(&s, &inc, 3, "cand-1");
    World {
        dir,
        s,
        inc,
        inc_ref,
        baseline,
        cand,
        cand_ref,
    }
}

pub fn propose(s: &Store, parent: &PolicyEnvelope, seed: u64, id: &str) -> (PolicyEnvelope, Ref) {
    let req = evo::parse_request(
        &evo_request(
            parent,
            seed,
            id,
            vec![discovery_episode(parent, &format!("d-{id}"))],
        )
        .to_string(),
    )
    .unwrap();
    let p = evo::propose(s, &req).unwrap();
    (p.candidate, p.candidate_policy_ref)
}

fn manifest(n: usize) -> axon_loop::tasks::TaskManifest {
    let mut tasks: Vec<String> = (0..n).map(|i| format!("task-{i}")).collect();
    tasks.sort();
    axon_loop::tasks::TaskManifest::parse(
        &json!({"schema":"axon.loop.task-manifest/1","scope":scope(),"tasks":tasks,"issuer_ref":ADMITTER})
            .to_string(),
    )
    .unwrap()
}

/// The ref of the manifest `task-0 .. task-{n-1}`.
pub fn task_manifest_ref(n: usize) -> Ref {
    manifest(n).manifest_ref().unwrap()
}

/// Register the manifest `task-0 .. task-{n-1}` and return its ref.
pub fn register_tasks(s: &Store, n: usize) -> Ref {
    axon_loop::tasks::put(s, &manifest(n)).unwrap()
}

/// Register + freeze a plan over the manifest `task-0 .. task-{n-1}`.
pub fn freeze_plan_n(
    s: &Store,
    id: &str,
    inc: &Ref,
    cand: &Ref,
    n: usize,
    edit: impl FnOnce(&mut Value),
) -> Result<Ref, LoopError> {
    let mut v = complete_plan(id, inc, cand);
    v["task_manifest_ref"] = json!(register_tasks(s, n));
    edit(&mut v);
    plan::register(s, &PilotPlan::from_value(&v)?)?;
    plan::freeze(s, id)
}

/// [`freeze_plan_n`] over two tasks, the fixtures' default pairing.
pub fn freeze_plan(
    s: &Store,
    id: &str,
    inc: &Ref,
    cand: &Ref,
    edit: impl FnOnce(&mut Value),
) -> Result<Ref, LoopError> {
    freeze_plan_n(s, id, inc, cand, 2, edit)
}

/// (arm, policy, task, trial, outcome, cost)
pub type Spec<'a> = (
    &'a str,
    &'a PolicyEnvelope,
    String,
    String,
    Out,
    Option<u64>,
);

pub fn arm<'a>(
    arm: &'a str,
    p: &'a PolicyEnvelope,
    n: usize,
    pfx: &str,
    pass: usize,
    cost: Option<u64>,
) -> Vec<Spec<'a>> {
    (0..n)
        .map(|i| {
            (
                arm,
                p,
                format!("task-{i}"),
                format!("{pfx}{i}"),
                if i < pass { Out::Pass } else { Out::Fail },
                cost,
            )
        })
        .collect()
}

pub fn pair<'a>(
    inc: &'a PolicyEnvelope,
    cand: &'a PolicyEnvelope,
    n: usize,
    inc_pass: usize,
    cand_pass: usize,
    inc_cost: Option<u64>,
    cand_cost: Option<u64>,
) -> Vec<Spec<'a>> {
    let mut v = arm("incumbent", inc, n, "i", inc_pass, inc_cost);
    v.extend(arm("challenger-1", cand, n, "c", cand_pass, cand_cost));
    v
}

pub struct EvlOpts {
    pub epoch: u64,
    pub role: CorpusRole,
    pub deliver: Box<dyn Fn(&str) -> bool>,
    pub created_ms: Option<u64>,
}

impl Default for EvlOpts {
    fn default() -> Self {
        EvlOpts {
            epoch: 1,
            role: CorpusRole::Confirmation,
            deliver: Box::new(|_| true),
            created_ms: None,
        }
    }
}

pub fn evl_request(
    exp: &str,
    a: &PolicyEnvelope,
    b: &PolicyEnvelope,
    specs: &[Spec],
    o: &EvlOpts,
) -> Value {
    let mut assigned = vec![];
    let mut trials = vec![];
    for (armid, p, task, t, out, cost) in specs {
        assigned.push(json!({"task_id": task, "arm_id": armid, "trial_id": t, "policy_ref": digest(*p).unwrap()}));
        if (o.deliver)(t) {
            let mut tr = Trial::new(p, task, armid, t);
            tr.out = *out;
            tr.cost = *cost;
            tr.epoch = o.epoch;
            tr.role = o.role;
            tr.created_ms = o.created_ms;
            trials.push(trial(&tr));
        }
    }
    json!({"schema":"axon.loop.evl-request/1","experiment_id":exp,"scope":scope(),
           "evaluator_ref":EVALUATOR,"subject_issuers":[WORKER],"policies":[a, b],
           "assigned":assigned,"trials":trials})
}

pub fn evaluate(s: &Store, v: &Value) -> Result<(evl::EvaluationRecord, Ref), LoopError> {
    evl::evaluate(s, &evl::parse_request(&v.to_string())?)
}

pub fn admit(
    s: &Store,
    exp: &str,
    eval: &Ref,
    admitter: &str,
    mech: bool,
) -> Result<(AdmissionRecord, Ref), LoopError> {
    let req = admission::parse_request(
        &json!({"schema":"axon.loop.admit-request/1","experiment_id":exp,
        "evaluation_ref":eval,"admitter_ref":admitter,"mechanism_test":mech})
        .to_string(),
    )?;
    admission::admit(s, &req)
}

/// Full happy path on a world: freeze `exp`, evaluate (2 tasks, cand cheaper),
/// admit. Returns the admission ref.
pub fn accepted(w: &World, exp: &str) -> Ref {
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, a) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    assert_eq!(
        rec.decision,
        axon_loop::admission::Decision::Accept,
        "{:?}",
        rec.reasons
    );
    a
}
