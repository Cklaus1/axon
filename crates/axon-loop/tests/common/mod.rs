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
    store_with_config_keyed(dir, None)
}

/// A fixture operator key for keyed-ledger tests (D-015). A test premise.
pub const FIXTURE_KEY_HEX: &str =
    "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

pub fn fixture_key() -> axon_loop::store::LedgerKey {
    axon_loop::store::LedgerKey::derive(&[0x11; 32]).unwrap()
}

/// [`store_with_config`] with an explicit ledger key (`None` = unkeyed).
pub fn store_with_config_keyed(dir: &Path, key: Option<axon_loop::store::LedgerKey>) -> Store {
    operator_root();
    let s = Store::open_dir_keyed(dir, key).unwrap();
    s.write_config(&Config {
        schema: ConfigSchema,
        trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
        trusted_verifiers: vec![OpaqueRef::new(VERIFIER).unwrap()],
        trusted_observers: vec![OpaqueRef::new(OBSERVER).unwrap()],
        verifier_keys: [(OpaqueRef::new(VERIFIER).unwrap(), verifier_key().1.clone())]
            .into_iter()
            .collect(),
        verifier_pins: [(OpaqueRef::new(VERIFIER).unwrap(), verifier_pin())]
            .into_iter()
            .collect(),
        task_acceptance: task_acceptance(),
        protected_scopes: Vec::new(),
        trusted_monitors: Vec::new(),
        monitor_keys: Default::default(),
        observer_keys: [(OpaqueRef::new(OBSERVER).unwrap(), observer_key().1.clone())]
            .into_iter()
            .collect(),
    })
    .unwrap();
    register_candidates(&s);
    register_tasks(&s, 2);
    s
}

/// A configured store with NO candidate list registered (G2 negatives).
pub fn store_without_candidates(dir: &Path) -> Store {
    operator_root();
    let s = Store::open_dir(dir).unwrap();
    s.write_config(&Config {
        schema: ConfigSchema,
        trusted_admitters: vec![OpaqueRef::new(ADMITTER).unwrap()],
        trusted_verifiers: vec![OpaqueRef::new(VERIFIER).unwrap()],
        trusted_observers: vec![OpaqueRef::new(OBSERVER).unwrap()],
        verifier_keys: [(OpaqueRef::new(VERIFIER).unwrap(), verifier_key().1.clone())]
            .into_iter()
            .collect(),
        verifier_pins: [(OpaqueRef::new(VERIFIER).unwrap(), verifier_pin())]
            .into_iter()
            .collect(),
        task_acceptance: task_acceptance(),
        protected_scopes: Vec::new(),
        trusted_monitors: Vec::new(),
        monitor_keys: Default::default(),
        observer_keys: [(OpaqueRef::new(OBSERVER).unwrap(), observer_key().1.clone())]
            .into_iter()
            .collect(),
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
    /// The attempt suffix: identity `<trial>-<attempt>` (default `a1`, the
    /// fixture's issued attempt).
    pub attempt: &'a str,
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
            attempt: "a1",
        }
    }
}

/// A delivered trial `{episode, context, acf_request, acf_receipt, projection}`
/// whose references all bind.
/// The fixture verifier's Ed25519 key `(PKCS#8, public hex)`, registered for
/// [`VERIFIER`] in every fixture store (G01-r22-independent-issuer).
/// O2: the OPERATOR's trust root for this test (thread) — the step an operator
/// performs on a real host: installing the verifier's and the observer's public
/// keys under `/etc/axon/trust/{verifier,observer}/`. Test builds only
/// (`test-trust-root`); the store alone never makes a key authority. One root
/// per test thread, so a test may revoke or install a key in isolation.
pub fn operator_root() -> PathBuf {
    thread_local! {
        static ROOT: std::cell::OnceCell<PathBuf> = const { std::cell::OnceCell::new() };
    }
    let r = ROOT.with(|c| {
        c.get_or_init(|| {
            let d = tempfile::tempdir().unwrap().keep();
            for (auth, key) in [("verifier", verifier_key()), ("observer", observer_key())] {
                std::fs::create_dir_all(d.join(auth)).unwrap();
                std::fs::write(d.join(auth).join("fixture.pub"), format!("{}\n", key.1)).unwrap();
            }
            d
        })
        .clone()
    });
    axon_loop_contracts::operator_trust::set_test_root(&r);
    r
}

pub fn verifier_key() -> &'static (Vec<u8>, String) {
    static KEY: std::sync::OnceLock<(Vec<u8>, String)> = std::sync::OnceLock::new();
    KEY.get_or_init(|| axon_loop_contracts::attestation::generate().unwrap())
}

/// G32-r22-sidecar-bindings: the fixture preflight observer's key, registered
/// for [`OBSERVER`] in every fixture store; every fixture context it observed
/// carries its signature (required only by a PROTECTED-class evaluation).
pub fn observer_key() -> &'static (Vec<u8>, String) {
    static KEY: std::sync::OnceLock<(Vec<u8>, String)> = std::sync::OnceLock::new();
    KEY.get_or_init(|| axon_loop_contracts::attestation::generate().unwrap())
}

/// ADR-001 §5: the fixture's independent safety monitor and its key.
pub const MONITOR: &str = "fixture:safety-monitor";
pub fn monitor_key() -> &'static (Vec<u8>, String) {
    static KEY: std::sync::OnceLock<(Vec<u8>, String)> = std::sync::OnceLock::new();
    KEY.get_or_init(|| axon_loop_contracts::attestation::generate().unwrap())
}

/// Register [`MONITOR`] (trusted, keyed) in the store's config.
pub fn trust_monitor(s: &Store) {
    let mut cfg = s.config().unwrap();
    let m = OpaqueRef::new(MONITOR).unwrap();
    if !cfg.trusted_monitors.contains(&m) {
        cfg.trusted_monitors.push(m.clone());
    }
    cfg.monitor_keys.insert(m, monitor_key().1.clone());
    s.write_config(&cfg).unwrap();
}

/// A safety report about `trial`'s episode, as `issuer`.
pub fn safety_report(trial: &Value, finding: &str, code: Option<&str>, issuer: &str) -> Value {
    json!({
        "schema": "axon.loop.trial-safety/1",
        "scope": trial["episode"]["scope"],
        "identity": trial["episode"]["identity"],
        "finding": finding,
        "code": code,
        "issuer_ref": issuer,
        "evidence_ref": null,
    })
}

/// [`MONITOR`]'s signature over a report.
pub fn monitor_sign(report: &Value) -> Value {
    axon_loop_contracts::attestation::sign_document(
        &monitor_key().0,
        axon_loop::safety::CLEARANCE_DOMAIN,
        &OpaqueRef::new(MONITOR).unwrap(),
        report,
    )
    .unwrap()
}

/// Intake every delivered trial of `v`, then record a signed clearance from
/// the trusted, independent [`MONITOR`] for each — what a protected
/// evaluation needs before it can ACCEPT.
pub fn clear_all(s: &Store, v: &Value) {
    trust_monitor(s);
    assert!(intake_all(s, v).is_empty(), "the bundle intakes");
    for t in v["trials"].as_array().unwrap() {
        let r = safety_report(t, "clear", None, MONITOR);
        axon_loop::safety::report(s, &r.to_string(), Some(&monitor_sign(&r).to_string())).unwrap();
    }
}

/// The operator's pin for [`VERIFIER`]: the revision, profile and suite a
/// fixture verification check runs.
pub const CHECK_EXECUTABLE: &str = "axon-test-local";
/// The fixture check executable's bytes' sha256; its `executable_digest` is
/// the genuine acf1 identity over it, so a protected receipt's
/// `guest-axon-sha256` can be JOINED to the request (M4).
pub fn check_executable_sha256() -> String {
    "e".repeat(64)
}
pub fn check_executable_digest() -> String {
    axon_loop_contracts::protected_evidence::executable_digest(
        CHECK_EXECUTABLE,
        &check_executable_sha256(),
    )
}
pub const CHECK_PROFILE: &str = "fabric:local-interpreter";
pub fn check_suite() -> String {
    format!("check-suite:acceptance@acf1:{}#accept.ax", "5".repeat(64))
}
/// The acceptance check registered for every fixture task: the pinned suite's
/// `t_` test (what [`verification_check`] runs).
pub const ACCEPTANCE_CHECK: &str = "t_";
pub fn task_acceptance() -> std::collections::BTreeMap<TaskId, axon_loop::store::AcceptancePin> {
    (0..200)
        .map(|i| format!("task-{i}"))
        .chain(["disc-task".to_string()])
        .map(|t| {
            (
                TaskId::new(t).unwrap(),
                axon_loop::store::AcceptancePin {
                    check_suite: check_suite(),
                    check: ACCEPTANCE_CHECK.into(),
                },
            )
        })
        .collect()
}

pub fn verifier_pin() -> axon_loop::store::VerifierPin {
    axon_loop::store::VerifierPin {
        registered_executable_ref: CHECK_EXECUTABLE.into(),
        executable_digest: check_executable_digest(),
        backend_profiles: vec![CHECK_PROFILE.into()],
        check_suites: vec![check_suite()],
    }
}

/// The registered check (request, receipt) that verified `identity`'s output
/// `tree`, with `verification` and `matched` as Fabric would record them.
pub fn verification_check(
    identity: &TrialIdentity,
    tree: &Acf1Ref,
    verification: &str,
    matched: u64,
) -> (Value, Value) {
    let req = json!({
        "schema": "acf-compute-request/1",
        "operation_id": identity.operation_id, "task_id": identity.task_id,
        "trial_id": identity.trial_id, "attempt_id": identity.attempt_id,
        "principal_ref": "principal:acceptance-check", "grant_ref": "grant:check", "approval_ref": null,
        "job_kind": "registered_check", "registered_executable_ref": CHECK_EXECUTABLE,
        "executable_digest": check_executable_digest(),
        "workspace_version_ref": tree, "semantic_state_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "required": {"engine": "axon_interpreter", "hardware_isolation": false, "os": "none",
                     "architecture": "x86_64", "network_mode": "deny", "checkpoint_kind": "none"},
        "limits": {"cpu_millicores": 1000, "memory_bytes": 268435456, "disk_bytes": 268435456,
                   "wall_time_ms": 60000, "output_bytes": 1048576, "max_cost_micro": 100,
                   "currency_code": "USD", "price_schedule_ref": "unpriced:test"},
        "argv": ["check:acceptance", "t_"], "result_schema_ref": "cortex-check-report/1",
    });
    let rc = json!({
        "schema": "acf-execution-receipt/1",
        "operation_id": identity.operation_id, "task_id": identity.task_id,
        "trial_id": identity.trial_id, "attempt_id": identity.attempt_id,
        "execution_id": identity.execution_id, "backend_profile_ref": CHECK_PROFILE,
        "input_workspace_ref": tree, "output_workspace_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "status": "completed", "process_exit_code": 0,
        "verification": verification, "matched_checks": matched,
        "evidence_source": "supervisor_observed",
        "evidence_refs": ["cl22-report:fixture", check_suite()],
        "usage_state": "unknown", "cost_micro": null, "unresolved_liability_micro": 100,
    });
    (req, rc)
}

/// The verifier's attestation of `(req, rc)` as `issuer`, under the fixture key.
pub fn attest(issuer: &str, req: &Value, rc: &Value) -> Value {
    axon_loop_contracts::attestation::sign(
        &verifier_key().0,
        &OpaqueRef::new(issuer).unwrap(),
        &serde_json::from_value(req.clone()).unwrap(),
        &serde_json::from_value(rc.clone()).unwrap(),
        axon_loop::now_ms(),
    )
    .unwrap()
}

pub fn trial(t: &Trial) -> Value {
    let identity = TrialIdentity {
        task_id: TaskId::new(t.task).unwrap(),
        arm_id: ArmId::new(t.arm).unwrap(),
        trial_id: TrialId::new(t.trial).unwrap(),
        attempt_id: AttemptId::new(format!("{}-{}", t.trial, t.attempt)).unwrap(),
        operation_id: OperationId::new(format!("{}-{}-op", t.trial, t.attempt)).unwrap(),
        execution_id: ExecutionId::new(format!("{}-{}-ex", t.trial, t.attempt)).unwrap(),
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
    // The verification the episode cites: the registered check over the
    // episode's output tree (Pass/Fail only — an Unknown cites none).
    let tree = ep
        .output_workspace_ref
        .clone()
        .expect("the fixture episode has an output tree");
    let verdict = match t.out {
        Out::Pass => Some("passed"),
        Out::Fail => Some("failed"),
        Out::Unknown => None,
    };
    let (vreq, vrc) = verification_check(
        &ep.identity,
        &tree,
        verdict.unwrap_or("unknown"),
        ep.verification.matched_checks,
    );
    if verdict.is_some() {
        ep.verification.verifier_ref = Some(digest_value(&vrc).unwrap());
        ep.verification.evidence_refs = vec![digest_value(&vreq).unwrap()];
        ep.verification.output_workspace_ref = Some(tree.clone());
    } else {
        // An Unknown cites no verifier and delivers no verification (it used
        // to keep the bundle's placeholder verifier_ref, which intake refuses
        // — so every Unknown fixture was Unknown for the wrong reason).
        ep.verification.verifier_ref = None;
        ep.verification.evidence_refs = vec![];
    }
    ep.context_ref = digest(&ctx).unwrap();
    ep.acf_request_ref = digest(&req).unwrap();
    ep.acf_receipt_ref = digest(&rc).unwrap();
    ep.validate().unwrap();
    let (vreq, vrc, att) = if verdict.is_some() {
        let att = attest(t.verifier, &vreq, &vrc);
        (vreq, vrc, att)
    } else {
        (Value::Null, Value::Null, Value::Null)
    };
    let ctx_sig = if ctx.observed_issuer_ref.as_str() == OBSERVER {
        axon_loop_contracts::attestation::sign_document(
            &observer_key().0,
            axon_loop::evl::CONTEXT_DOMAIN,
            &ctx.observed_issuer_ref,
            &serde_json::to_value(&ctx).unwrap(),
        )
        .unwrap()
    } else {
        Value::Null
    };
    json!({"episode": ep, "context": ctx, "acf_request": req, "acf_receipt": rc, "projection": proj,
           "verification_request": vreq, "verification_receipt": vrc,
           "verification_attestation": att, "context_signature": ctx_sig})
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
    // ADR-001 D4: monetary economics are report-only until metered receipts
    // exist (every Fabric execution cost is unknown under D10). A test of the
    // cost criterion sets `min_cost_reduction_ppm` itself.
    o.insert("economic_threshold".into(), json!("report_only"));
    o.insert("uncertainty_rule".into(), json!("exact_bounds"));
    o.insert("missing_data_rule".into(), json!("unknown_bounds"));
    o.insert("multiplicity_rule".into(), json!("single_candidate"));
    // Each Fabric attempt holds its reservation as unresolved liability (D10).
    o.insert(
        "budget_rule".into(),
        json!("max_unresolved_liability_micro=100000000"),
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
    world_keyed(None)
}

/// [`world`] over a store opened with `key` (D-015 keyed ledger).
pub fn world_keyed(key: Option<axon_loop::store::LedgerKey>) -> World {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config_keyed(dir.path(), key);
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

/// MiCode's policy acknowledgement for `p`, as intake joins it by content.
pub fn ack_for(p: &PolicyEnvelope) -> Value {
    json!({
        "schema": "micode.closed-loop.policy-ack/1",
        "pin": {"state": "pinned", "policy_id": p.policy_id, "policy_ref": digest(p).unwrap(),
                "controls_ref": p.controls_ref, "candidate_set_ref": p.candidate_set_ref,
                "shortlist": p.shortlist},
        "candidates": candidate_list(),
        "candidate_set_ref": p.candidate_set_ref,
    })
}

/// Intake every delivered trial of an EVL request, the way the producer's
/// episodes reach the store in production. A trial intake REFUSES stays
/// un-intaken — evaluation then counts it Unknown, never a pass — and the
/// refusal reasons are returned so a test can pin them.
/// ADR-001 §3.6: journal the request's population (issued attempt
/// `<trial>-a1`, the fixture identity) as the independent ADMITTER, BEFORE
/// any trial is intaken. Idempotent; a test that journalled its own
/// assignment keeps it (a differing one is left for `evaluate` to refuse).
pub fn assign_request(s: &Store, v: &Value) {
    let _ = plan::assign(s, &assignment_of_request(v));
}

/// The assignment an operator would have issued for request `v`'s population.
pub fn assignment_of_request(v: &Value) -> plan::AssignmentRecord {
    let trials: Vec<Value> = v["assigned"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| {
            let mut t = a.clone();
            t["attempt_id"] = json!(format!("{}-a1", a["trial_id"].as_str().unwrap()));
            t
        })
        .collect();
    assignment_record(v["experiment_id"].as_str().unwrap(), trials)
}

pub fn assignment_record(exp: &str, trials: Vec<Value>) -> plan::AssignmentRecord {
    let rec = json!({"schema":"axon.loop.assignment/1","experiment_id":exp,"scope":scope(),
                     "issuer_ref":ADMITTER,"trials":trials});
    plan::parse_assignment(&rec.to_string()).unwrap()
}

/// Journal an assignment of `trials` (`{task_id, arm_id, trial_id,
/// attempt_id, policy_ref}`) for `exp`, issued by ADMITTER.
pub fn assign(s: &Store, exp: &str, trials: Vec<Value>) {
    // Best effort: a refused assignment (plan not frozen, a trial id reused,
    // one already journalled) leaves `evaluate` to refuse the request.
    let _ = plan::assign(s, &assignment_record(exp, trials));
}

/// Journal the standard population of `specs` BEFORE the trials are built (a
/// protected trial must be preflighted after its assignment).
pub fn assign_specs(s: &Store, exp: &str, specs: &[Spec]) {
    let trials = specs
        .iter()
        .map(|(armid, p, task, t, _, _)| {
            json!({"task_id": task, "arm_id": armid, "trial_id": t,
                   "attempt_id": format!("{t}-a1"), "policy_ref": digest(*p).unwrap()})
        })
        .collect();
    assign(s, exp, trials);
}

pub fn intake_all(s: &Store, v: &Value) -> Vec<(String, String)> {
    assign_request(s, v);
    let acks: Vec<String> = v["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let p: PolicyEnvelope = serde_json::from_value(p.clone()).unwrap();
            axon_loop::candidates::put_policy(s, &p).unwrap();
            ack_for(&p).to_string()
        })
        .collect();
    let text = |x: &Value| (!x.is_null()).then(|| x.to_string());
    let mut refused = vec![];
    for t in v["trials"].as_array().unwrap() {
        let r = axon_loop::intake::intake_episode(
            s,
            &axon_loop::intake::IntakeInput {
                episode: &t["episode"].to_string(),
                context: &t["context"].to_string(),
                acks: &acks,
                projection: None,
                source_episode: None,
                verification_request: text(&t["verification_request"]).as_deref(),
                verification_receipt: text(&t["verification_receipt"]).as_deref(),
                verification_attestation: text(&t["verification_attestation"]).as_deref(),
                verification_psv_evidence: text(&t["verification_psv_evidence"]).as_deref(),
            },
        );
        if let Err(e) = r {
            refused.push((
                t["episode"]["identity"]["trial_id"]
                    .as_str()
                    .unwrap_or("?")
                    .to_string(),
                e.to_string(),
            ));
        }
    }
    refused
}

thread_local! {
    static INTAKE_REFUSALS: std::cell::RefCell<std::collections::BTreeMap<String, String>> =
        Default::default();
}

/// Intake what the request delivers, then evaluate — the production order.
/// Each trial intake refused is remembered (per test thread) for
/// [`first_refusal`].
pub fn evaluate(s: &Store, v: &Value) -> Result<(evl::EvaluationRecord, Ref), LoopError> {
    let refused = intake_all(s, v);
    INTAKE_REFUSALS.with(|m| m.borrow_mut().extend(refused));
    evl::evaluate(s, &evl::parse_request(&v.to_string())?)
}

/// Why a trial did not count, at the FIRST layer that refused it: intake's
/// reason when evaluation found the trial never intaken (ADR-001 §8 — laundered
/// episode or context bytes now die at intake), else evaluation's own.
pub fn first_refusal(trial_id: &str, evl_reason: &str) -> String {
    if evl_reason.contains("not intaken") {
        if let Some(r) = INTAKE_REFUSALS.with(|m| m.borrow().get(trial_id).cloned()) {
            return format!("intake: {r}");
        }
    }
    evl_reason.to_string()
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

// ── B2: a GENUINE protected verification (review wf_d725935a-7ed) ──────────

/// Sign `bytes` as an OBSERVER-domain `axon-evidence-signature/2` with the
/// fixture observer's key (installed in the test operator root).
pub fn observer_sign(bytes: &[u8]) -> String {
    use axon_loop_contracts::operator_trust::{evidence_signing_message, TrustAuthority};
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_pkcs8(&observer_key().0).unwrap();
    let hexs = |b: &[u8]| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    serde_json::json!({
        "schema": "axon-evidence-signature/2", "alg": "ed25519", "domain": "observer",
        "public_key": hexs(kp.public_key().as_ref()),
        "signature": hexs(kp.sign(&evidence_signing_message(TrustAuthority::Observer, bytes)).as_ref()),
    })
    .to_string()
}

/// The fixture observer's key id.
pub fn observer_key_id() -> String {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_pkcs8(&observer_key().0).unwrap();
    axon_loop_contracts::operator_trust::key_fingerprint(kp.public_key().as_ref())
}

/// Turn a (request, receipt) verification into a GENUINE protected one: a
/// launch manifest built from the request, an observation of it signed by the
/// operator-rooted fixture observer, and the receipt's class and digest refs
/// derived from THOSE documents. `edit_m`/`edit_o` apply one defect before
/// signing (the refs follow the edited documents); returns the bundle text.
pub fn make_protected(
    req: &Value,
    rc: &mut Value,
    edit_m: impl FnOnce(&mut axon_psv::LaunchManifest),
    edit_o: impl FnOnce(&mut axon_psv::PreflightObservation),
) -> String {
    use axon_psv::*;
    let h = |c: &str| c.repeat(64);
    let suite = check_suite();
    let (id, rest) = suite
        .strip_prefix("check-suite:")
        .unwrap()
        .split_once('@')
        .unwrap();
    let (version, entry) = rest.split_once('#').unwrap();
    let mut m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: req["operation_id"].as_str().unwrap().into(),
        task_id: req["task_id"].as_str().unwrap().into(),
        trial_id: req["trial_id"].as_str().unwrap().into(),
        attempt_id: req["attempt_id"].as_str().unwrap().into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: h("d"),
        qualification_sha256: h("4"),
        host_config_sha256: h("5"),
        launcher_sha256: h("6"),
        firecracker_sha256: h("7"),
        profile_manifest_sha256: h("8"),
        guest: GuestDigests {
            kernel_sha256: h("1"),
            rootfs_sha256: h("2"),
            axon_sha256: check_executable_sha256(),
            init_sha256: h("3"),
        },
        policy_sha256: h("9"),
        suite: SuiteRef {
            id: id.into(),
            version: version.into(),
            entry: entry.into(),
            test: req["argv"][1].as_str().unwrap().into(),
            tree_digest: version.into(),
            registry_sha256: h("a"),
        },
        candidate: CandidateRef {
            workspace_version: req["workspace_version_ref"].as_str().unwrap().into(),
            tree_digest: req["workspace_version_ref"].as_str().unwrap().into(),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: h("b")[..32].into(),
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    };
    edit_m(&mut m);
    let m_bytes = m.bytes();
    let m_sha = sha256_hex(&m_bytes);
    let mut o = PreflightObservation {
        schema: PREFLIGHT_OBSERVATION_SCHEMA.into(),
        observer_key_id: observer_key_id(),
        nonce: m.observation_nonce.clone(),
        epoch: 0,
        observed_at: "2026-09-28T00:00:00Z".into(),
        host_profile: m.backend_profile.clone(),
        fabric_revision: m.fabric_revision.clone(),
        firecracker_sha256: m.firecracker_sha256.clone(),
        launcher_sha256: m.launcher_sha256.clone(),
        host_config_sha256: m.host_config_sha256.clone(),
        guest: m.guest.clone(),
        verifier_sha256: m.verifier_sha256.clone(),
        suite_registry_sha256: m.suite.registry_sha256.clone(),
        policy_sha256: m.policy_sha256.clone(),
        intended_launch_manifest_sha256: m_sha.clone(),
    };
    edit_o(&mut o);
    let o_bytes = serde_json::to_vec(&o).unwrap();
    rc["backend_profile_ref"] = serde_json::json!("linux-microvm-protected");
    let refs = rc["evidence_refs"].as_array_mut().unwrap();
    refs.retain(|e| {
        let e = e.as_str().unwrap_or("");
        !(e.starts_with("evidence-class:") || e.contains("-sha256:"))
    });
    for r in [
        "evidence-class:protected".to_string(),
        format!("launch-manifest-sha256:{m_sha}"),
        format!("preflight-observation-sha256:{}", sha256_hex(&o_bytes)),
        format!("guest-verdict-sha256:{}", h("c")),
        format!("guest-kernel-sha256:{}", m.guest.kernel_sha256),
        format!("guest-rootfs-sha256:{}", m.guest.rootfs_sha256),
        format!("guest-axon-sha256:{}", m.guest.axon_sha256),
        format!("guest-init-sha256:{}", m.guest.init_sha256),
        format!("qualification-sha256:{}", m.qualification_sha256),
    ] {
        refs.push(serde_json::json!(r));
    }
    serde_json::json!({
        "schema": "axon-psv-evidence/1",
        "launch_manifest": String::from_utf8(m_bytes).unwrap(),
        "observation": String::from_utf8(o_bytes.clone()).unwrap(),
        "observation_signature": observer_sign(&o_bytes),
    })
    .to_string()
}
