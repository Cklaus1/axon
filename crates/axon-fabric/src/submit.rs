//! The Fabric submit path: `acf-compute-request/1` in, `acf-execution-receipt/1`
//! out.
//!
//! # Order of operations (each step a refusal, never a downgrade)
//!
//! 1. **Parse** the request strictly (`axon_loop_contracts::parse`).
//! 2. **Dedup** against the journal: the same `operation_id` with the same
//!    input digest returns the RECORDED receipt, never re-executes; with a
//!    different digest it is a CONFLICT — nothing is written or spawned.
//! 3. **Authority epoch** (submit-time): the axon-loop store's current epoch
//!    for the scope must equal the caller's `expected_epoch`.
//! 4. **Backend selection** from what each backend IS ([`crate::backend`]).
//!    A request no backend satisfies is refused (receipt `unsupported`).
//! 5. **Registered executable**: `registered_executable_ref` is a registry
//!    ID; the path + sha256 come from the operator's `CheckRegistry`, and
//!    `executable_digest` must equal the registry-derived digest. argv is
//!    DATA (a file and a filter) — it never names a program.
//! 6. **Supervisor admission** through axon-os `supervise_requiring` with
//!    the request's isolation requirement and a restricted grant, so the
//!    same authority boundary (approval → isolation → intersect → admit)
//!    every axon-os job crosses decides here too.
//! 7. **Journal**: intent (fsynced) → reserve (atomic budget carve) →
//!    **epoch recheck + executable re-verify** → launch record → effect →
//!    terminal record → receipt recorded.
//!
//! A refusal at 1–6 writes NO launch record and spawns nothing. A refusal at
//! the pre-launch recheck (7) cancels the reservation (released: nothing was
//! launched) and spawns nothing.

use std::path::{Path, PathBuf};

use axon_cortex::runner::{
    CheckExecutor, CheckRegistry, CheckReport, CheckRequest, LocalInterpreterExecutor,
    RegisteredExecutable,
};
use axon_loop_contracts::{
    Acf1Ref, AuthorityEpoch, ComputeRequest, EvidenceSource, ExecutionId, ExecutionReceipt,
    JobKind, OpaqueRef, ReceiptStatus, ReceiptUsageState, ReceiptVerification, Ref, Scope,
    TaskFamily, TenantId,
};
use serde_json::{json, Value};

use crate::backend::{self, Profile};
use crate::journal::{Begin, Billing, Intent, Journal, JournalError, OpState, ResourceVector};

/// Where the current authority epoch comes from.
#[derive(Debug, Clone)]
pub enum EpochSource {
    /// The axon-loop store (`<store>/scopes/<tenant>/<family>/pointer.json`),
    /// read through the `axon_loop` library (which rolls forward a journalled
    /// transition before answering).
    LoopStore { store: PathBuf, scope: Scope },
}

impl EpochSource {
    pub fn current(&self) -> Result<AuthorityEpoch, String> {
        match self {
            EpochSource::LoopStore { store, scope } => {
                let st = axon_loop::Store::open(store).map_err(|e| e.to_string())?;
                axon_loop::epoch::current(&st, scope).map_err(|e| e.to_string())
            }
        }
    }

    pub fn scope(&self) -> &Scope {
        match self {
            EpochSource::LoopStore { scope, .. } => scope,
        }
    }
}

/// Operator configuration for one submit. Nothing here comes from the request.
#[derive(Debug, Clone)]
pub struct SubmitConfig {
    pub journal: PathBuf,
    pub registry: CheckRegistry,
    pub epoch: EpochSource,
    /// The epoch the caller was authorized under.
    pub expected_epoch: AuthorityEpoch,
    /// The directory a request's `argv[0]` file is resolved under.
    pub workspace: PathBuf,
    /// Aggregate ceiling for the scope (declared idempotently in the journal).
    pub budget: ResourceVector,
    /// Interpreter effect ceiling for the check (`AXON_ALLOWED_EFFECTS`),
    /// e.g. `Some("IO")`. `None` = no ceiling. The Linux profile has NO
    /// guest policy channel (B263 limitation x1), so a request carrying a
    /// ceiling is never eligible for it.
    pub effect_ceiling: Option<String>,
    /// Where the Linux microVM profile's launcher and evidence live.
    pub linux: Option<crate::backend::LinuxProfileConfig>,
    /// The caller's grant scopes filesystem paths (the Linux profile cannot
    /// preserve that, B263 x2).
    pub path_scoped_grant: bool,
    /// Test seam: called after the submit-time checks and the reservation,
    /// immediately before the dispatch-time epoch recheck. `None` in
    /// production (the CLI never sets it).
    pub pre_launch_hook: Option<fn(&SubmitConfig)>,
}

/// What a submit produced.
#[derive(Debug, Clone)]
pub struct Submission {
    pub receipt: ExecutionReceipt,
    /// The parsed check report (registered checks that produced a verdict).
    pub check_report: Option<Value>,
    /// True when this call returned a PREVIOUSLY recorded receipt.
    pub replayed: bool,
    pub backend: Option<&'static str>,
    /// Why no verdict/effect happened, when relevant.
    pub reason: Option<String>,
}

/// A refusal that never reached the journal's launch record.
#[derive(Debug)]
pub enum SubmitError {
    Malformed(String),
    Conflict(String),
    StaleEpoch { expected: u64, current: String },
    Unregistered(String),
    Journal(JournalError),
}

impl SubmitError {
    pub fn kind(&self) -> &'static str {
        match self {
            SubmitError::Malformed(_) => "malformed",
            SubmitError::Conflict(_) => "conflict",
            SubmitError::StaleEpoch { .. } => "stale_epoch",
            SubmitError::Unregistered(_) => "unregistered",
            SubmitError::Journal(_) => "journal",
        }
    }
    /// CLI exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            SubmitError::Malformed(_) => 3,
            SubmitError::Conflict(_) => 5,
            SubmitError::StaleEpoch { .. } => 6,
            SubmitError::Unregistered(_) => 4,
            SubmitError::Journal(_) => 2,
        }
    }
}

impl std::fmt::Display for SubmitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmitError::Malformed(s) => write!(f, "malformed request: {s}"),
            SubmitError::Conflict(s) => write!(f, "conflict: {s}"),
            SubmitError::StaleEpoch { expected, current } => write!(
                f,
                "stale authority epoch: request authorized at {expected}, store says {current}"
            ),
            SubmitError::Unregistered(s) => write!(f, "unregistered executable: {s}"),
            SubmitError::Journal(e) => write!(f, "journal: {e}"),
        }
    }
}

impl From<JournalError> for SubmitError {
    fn from(e: JournalError) -> Self {
        match e {
            JournalError::Conflict { op, .. } => SubmitError::Conflict(format!(
                "operation {op} is already recorded with a different request"
            )),
            other => SubmitError::Journal(other),
        }
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// `acf1:` identity of a registered executable — MUST equal
/// `axon_cortex::runner::fabric_executable_digest` (asserted in tests).
pub fn executable_digest(id: &str, e: &RegisteredExecutable) -> Acf1Ref {
    let v = json!({"registered_executable_ref": id, "sha256": e.sha256});
    let canon = axon_loop_contracts::canonical_bytes(&v).expect("small object");
    Acf1Ref::new(format!("acf1:{}", sha256_hex(&canon))).expect("hex")
}

/// `acf1:` identity of a single-file workspace version.
pub fn workspace_digest(rel_path: &str, bytes: &[u8]) -> Acf1Ref {
    let v = json!({"path": rel_path, "sha256": sha256_hex(bytes)});
    let canon = axon_loop_contracts::canonical_bytes(&v).expect("small object");
    Acf1Ref::new(format!("acf1:{}", sha256_hex(&canon))).expect("hex")
}

fn opaque(s: impl Into<String>) -> OpaqueRef {
    let mut s: String = s.into();
    if s.chars().count() > 512 {
        s = s.chars().take(509).collect::<String>() + "...";
    }
    if s.is_empty() {
        s = "-".into();
    }
    OpaqueRef::new(s).expect("bounded")
}

/// Reservation carved for a request: its declared limits.
fn reservation(req: &ComputeRequest) -> ResourceVector {
    ResourceVector {
        model_micro_usd: req.limits.max_cost_micro,
        exec_ms: req.limits.wall_time_ms,
        verify_ms: 0,
        retries: 0,
    }
}

/// What the supervisor observed about one execution.
struct Obs {
    status: ReceiptStatus,
    exit: Option<i32>,
    verification: ReceiptVerification,
    matched: Option<u64>,
    evidence: Vec<OpaqueRef>,
    liability_micro: u64,
}

fn receipt(req: &ComputeRequest, backend: &str, o: Obs) -> ExecutionReceipt {
    let Obs {
        status,
        exit,
        verification,
        matched,
        evidence,
        liability_micro,
    } = o;
    ExecutionReceipt {
        schema: Default::default(),
        operation_id: req.operation_id.clone(),
        task_id: req.task_id.clone(),
        trial_id: req.trial_id.clone(),
        attempt_id: req.attempt_id.clone(),
        execution_id: ExecutionId::new(format!("exec-{}", req.operation_id)).unwrap_or_else(|_| {
            ExecutionId::new(format!(
                "exec-{}",
                &sha256_hex(req.operation_id.as_str().as_bytes())[..32]
            ))
            .expect("hex id")
        }),
        backend_profile_ref: opaque(backend),
        input_workspace_ref: req.workspace_version_ref.clone(),
        output_workspace_ref: None,
        policy_digest: req.policy_digest.clone(),
        status,
        process_exit_code: exit.and_then(|c| u8::try_from(c).ok()),
        verification,
        matched_checks: matched,
        evidence_source: EvidenceSource::SupervisorObserved,
        evidence_refs: evidence,
        // Nothing here meters cost. Unknown is None, never 0.
        usage_state: ReceiptUsageState::Unknown,
        cost_micro: None,
        unresolved_liability_micro: liability_micro,
    }
}

/// Validate the request's executable identity against the registry.
fn resolve_executable(
    req: &ComputeRequest,
    reg: &CheckRegistry,
) -> Result<RegisteredExecutable, SubmitError> {
    let id = req.registered_executable_ref.as_str();
    let e = reg
        .get(id)
        .map_err(|e| SubmitError::Unregistered(e.to_string()))?
        .clone();
    let want = executable_digest(id, &e);
    if req.executable_digest != want {
        return Err(SubmitError::Unregistered(format!(
            "executable_digest {} does not match the registry's {} for `{id}`",
            req.executable_digest, want
        )));
    }
    Ok(e)
}

/// `argv` for a registered check is `[file]` or `[file, filter]`. The file
/// must be relative, inside the workspace, and its bytes must hash to
/// `workspace_version_ref`.
fn check_target(req: &ComputeRequest, ws: &Path) -> Result<(String, Option<String>), SubmitError> {
    let (file, filter) = match req.argv.as_slice() {
        [f] => (f.clone(), None),
        [f, flt] => (f.clone(), Some(flt.clone())),
        _ => {
            return Err(SubmitError::Malformed(
                "registered_check argv must be [file] or [file, filter]".into(),
            ))
        }
    };
    let p = Path::new(&file);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(SubmitError::Malformed(format!(
            "argv file {file:?} must be a plain relative path inside the workspace"
        )));
    }
    let bytes = std::fs::read(ws.join(p))
        .map_err(|e| SubmitError::Malformed(format!("cannot read {file}: {e}")))?;
    let got = workspace_digest(&file, &bytes);
    if req.workspace_version_ref != got {
        return Err(SubmitError::Conflict(format!(
            "workspace_version_ref {} does not match the bytes of {file} ({got})",
            req.workspace_version_ref
        )));
    }
    Ok((file, filter))
}

/// The axon-os authority boundary, applied to this request: approval →
/// isolation requirement → intersect → admit, via `supervise_requiring`.
/// The "job" is a pure placeholder program: the check's own effects are
/// bounded by the interpreter ceiling at dispatch; what this decides is
/// whether THIS runtime may serve THIS requirement under THIS grant.
/// An axon-os `Runtime` that performs NO effect: it states the selected
/// backend's isolation and declares a pure program. `supervise_requiring`
/// over it answers exactly one question — may this backend serve this
/// requirement under this grant? — through the same code path every axon-os
/// job takes. The real effect runs later, after the journal's launch record.
struct AdmissionProbe(axon_os::Isolation);

impl axon_os::Runtime for AdmissionProbe {
    fn isolation(&self) -> axon_os::Isolation {
        self.0
    }
    fn declared_effects(&self, _p: &Path) -> axon_os::DeclaredEffects {
        axon_os::DeclaredEffects {
            row: axon_os::EffectSet::default(),
            max_label: axon_os::Label::Public,
        }
    }
    fn mint_principal(&self, _g: &axon_os::Grant) -> axon_os::PrincipalHandle {
        axon_os::PrincipalHandle(0)
    }
    fn run_sandboxed(
        &self,
        _p: &Path,
        _h: &axon_os::PrincipalHandle,
        _g: &axon_os::Grant,
        _b: &axon_os::Budget,
        _s: u64,
    ) -> axon_os::RunOutcome {
        axon_os::RunOutcome {
            events: vec![],
            verdict: axon_os::Verdict::Completed { value: 0 },
        }
    }
}

fn supervisor_admits(
    req: &ComputeRequest,
    profile: &Profile,
    scratch: &Path,
) -> Result<(), String> {
    use axon_os::IsolationRequirement;
    let requirement =
        if req.required.hardware_isolation && req.required.os == axon_loop_contracts::Os::Linux {
            IsolationRequirement::MicroVm
        } else if req.required.hardware_isolation {
            IsolationRequirement::HardwareIsolated
        } else {
            IsolationRequirement::Any
        };
    // Never read: the probe runtime declares the program's effects itself.
    let job = scratch.join("fabric-admission.ax");
    let manifest = axon_os::JobManifest {
        program: job.clone(),
        intent: format!("fabric {}", req.operation_id),
        seed: 1,
        grant: axon_os::profile::Profile::Restricted.default_grant(
            axon_os::Label::Internal,
            axon_os::Budget {
                calls: 1,
                tokens: 0,
                cost_micro: req.limits.max_cost_micro as i64,
            },
        ),
        require_approval: false,
    };
    let rt = AdmissionProbe(profile.isolation);
    let rec = axon_os::supervise_requiring(
        &manifest,
        &scratch.join("fabric-admission.axjob"),
        &manifest.grant.clone(),
        req.operation_id.as_str(),
        requirement,
        &rt,
    );
    match rec.verdict {
        axon_os::Verdict::Completed { .. } => Ok(()),
        other => Err(format!("axon-os supervisor refused: {other:?}")),
    }
}

/// The host interpreter executor, pinned to exactly the resolved entry and
/// bounded by the request's limits and the operator's effect ceiling.
fn host_executor(
    exe: &RegisteredExecutable,
    req: &ComputeRequest,
    cfg: &SubmitConfig,
) -> Result<LocalInterpreterExecutor, SubmitError> {
    let mut r = CheckRegistry::new();
    r.register_expected(
        axon_cortex::runner::LOCAL_AXON_TEST_ID,
        &exe.path,
        &exe.sha256,
    )
    .map_err(|e| SubmitError::Unregistered(e.to_string()))?;
    let local = LocalInterpreterExecutor::from_registry(&r)
        .map_err(|e| SubmitError::Unregistered(e.to_string()))?
        .with_timeout(std::time::Duration::from_millis(req.limits.wall_time_ms))
        .with_max_output(req.limits.output_bytes as usize);
    Ok(match &cfg.effect_ceiling {
        Some(c) => local.with_effect_ceiling(c.clone()),
        None => local,
    })
}

/// Submit one request. See the module docs for the order of operations.
pub fn submit(req_json: &str, cfg: &SubmitConfig) -> Result<Submission, SubmitError> {
    // 1. Parse.
    let req: ComputeRequest =
        axon_loop_contracts::parse(req_json).map_err(|e| SubmitError::Malformed(e.to_string()))?;
    let input_digest: Ref =
        axon_loop_contracts::digest(&req).map_err(|e| SubmitError::Malformed(e.to_string()))?;

    let (journal, _recovery) = Journal::open(&cfg.journal)?;
    let scope = cfg.epoch.scope().clone();
    journal.declare_budget(&scope, cfg.budget)?;

    // 2. Dedup by operation id.
    if let Some(v) = journal.view(&req.operation_id) {
        if v.intent.input_digest != input_digest {
            return Err(SubmitError::Conflict(format!(
                "operation {} is already recorded with input {}, not {}",
                req.operation_id, v.intent.input_digest, input_digest
            )));
        }
        return Ok(replayed(&req, &v));
    }

    // 3. Authority epoch at submit.
    let current = cfg.epoch.current();
    if current.as_ref().ok() != Some(&cfg.expected_epoch) {
        return Err(SubmitError::StaleEpoch {
            expected: cfg.expected_epoch.get(),
            current: current.map(|e| e.get().to_string()).unwrap_or_else(|e| e),
        });
    }

    // 4. Backend selection. A request nothing satisfies gets an
    //    `unsupported` receipt — journalled (intent + failed, never launched)
    //    so a retry returns the same answer.
    let needs = backend::AuthorityNeeds {
        guest_policy_channel: cfg.effect_ceiling.is_some(),
        path_scoped_grant: cfg.path_scoped_grant,
    };
    let profile = match backend::select(&req, cfg.linux.as_ref(), needs) {
        Ok(p) => p,
        Err(backend::Unsupported(why)) => {
            let r = receipt(
                &req,
                "none",
                Obs {
                    status: ReceiptStatus::Unsupported,
                    exit: None,
                    verification: ReceiptVerification::NotRun,
                    matched: None,
                    evidence: vec![],
                    liability_micro: 0,
                },
            );
            record_unlaunched(&journal, &req, &input_digest, cfg, &scope, &why, &r)?;
            return Ok(Submission {
                receipt: r,
                check_report: None,
                replayed: false,
                backend: None,
                reason: Some(why),
            });
        }
    };

    // 5. Registered executable + workspace binding. For the local backend the
    //    executable is a host file pinned in the operator registry; for the
    //    Linux profile it is the interpreter INSIDE the pinned rootfs, whose
    //    sha256 is the qualified manifest's `artifacts.axon`.
    let exe = if profile.id == backend::LINUX_MICROVM_PROTECTED.id {
        let q = cfg
            .linux
            .as_ref()
            .expect("selected only when configured")
            .qualification()
            .map_err(SubmitError::Unregistered)?;
        let id = req.registered_executable_ref.as_str();
        if id != backend::LINUX_GUEST_AXON_ID {
            return Err(SubmitError::Unregistered(format!(
                "{} runs only `{}` (the rootfs interpreter), not `{id}`",
                profile.id,
                backend::LINUX_GUEST_AXON_ID
            )));
        }
        let e = RegisteredExecutable {
            path: PathBuf::from("/usr/bin/axon (guest rootfs)"),
            sha256: q.guest_axon_sha256,
        };
        if req.executable_digest != executable_digest(id, &e) {
            return Err(SubmitError::Unregistered(format!(
                "executable_digest does not match the qualified guest interpreter for `{id}`"
            )));
        }
        e
    } else {
        resolve_executable(&req, &cfg.registry)?
    };
    let target = match req.job_kind {
        JobKind::RegisteredCheck => Some(check_target(&req, &cfg.workspace)?),
        JobKind::InterpreterRun => {
            if req.argv.len() != 1 {
                return Err(SubmitError::Malformed(
                    "interpreter_run argv must be [program.ax]".into(),
                ));
            }
            let (f, _) = check_target(&req, &cfg.workspace)?;
            Some((f, None))
        }
    };

    // 6. Supervisor admission (axon-os).
    let scratch = cfg
        .journal
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    if let Err(why) = supervisor_admits(&req, &profile, &scratch) {
        let r = receipt(
            &req,
            profile.id,
            Obs {
                status: ReceiptStatus::Denied,
                exit: None,
                verification: ReceiptVerification::NotRun,
                matched: None,
                evidence: vec![],
                liability_micro: 0,
            },
        );
        record_unlaunched(&journal, &req, &input_digest, cfg, &scope, &why, &r)?;
        return Ok(Submission {
            receipt: r,
            check_report: None,
            replayed: false,
            backend: Some(profile.id),
            reason: Some(why),
        });
    }

    // 7. Journal around the effect.
    let intent = Intent {
        op: req.operation_id.clone(),
        task_id: req.task_id.clone(),
        trial_id: req.trial_id.clone(),
        attempt_id: req.attempt_id.clone(),
        input_digest: input_digest.clone(),
        config: json!({"backend": profile.id, "limits": req.limits}),
        authority_ref: format!("{}|{}", req.principal_ref, req.grant_ref),
        authority_epoch: cfg.expected_epoch,
        scope: scope.clone(),
        reservation: reservation(&req),
        expected_version: 0,
    };
    if let Begin::AlreadyRecorded(v) = journal.begin(intent)? {
        // Raced with a concurrent submit of the same op: never run twice.
        return Ok(replayed(&req, &v));
    }
    journal.reserve(&req.operation_id)?;

    if let Some(h) = cfg.pre_launch_hook {
        h(cfg);
    }
    // Dispatch-time recheck, immediately before the launch record.
    let now = cfg.epoch.current();
    if now.as_ref().ok() != Some(&cfg.expected_epoch) {
        journal.cancel(&req.operation_id, "stale authority epoch at dispatch", None)?;
        return Err(SubmitError::StaleEpoch {
            expected: cfg.expected_epoch.get(),
            current: now.map(|e| e.get().to_string()).unwrap_or_else(|e| e),
        });
    }
    let (file, filter) = target.expect("set above");
    let is_linux = profile.id == backend::LINUX_MICROVM_PROTECTED.id;
    // Re-verify the executable immediately before the launch record: a host
    // binary against its registry pin, the Linux profile against its
    // qualification (manifest vs evidence).
    if is_linux {
        if let Err(e) = cfg.linux.as_ref().expect("configured").qualification() {
            journal.cancel(
                &req.operation_id,
                &format!("profile no longer qualified: {e}"),
                None,
            )?;
            return Err(SubmitError::Unregistered(e));
        }
    }
    let local = if is_linux {
        None
    } else {
        Some(host_executor(&exe, &req, cfg)?)
    };
    if let Some(Err(e)) = local.as_ref().map(|l| l.verify()) {
        journal.cancel(&req.operation_id, &format!("executable changed: {e}"), None)?;
        return Err(SubmitError::Unregistered(e.to_string()));
    }

    journal.mark_launched(&req.operation_id)?;
    let liability = req.limits.max_cost_micro;
    let (r, report, reason) = match profile.id {
        id if id == backend::LOCAL_INTERPRETER.id => {
            let res = local.expect("host backend").run_checks(&CheckRequest {
                workspace: &cfg.workspace,
                rel_path: &file,
                filter: filter.as_deref(),
            });
            local_receipt(&req, &journal, res, filter.as_deref(), liability)?
        }
        id if id == backend::LINUX_MICROVM_PROTECTED.id => {
            let lx = cfg.linux.as_ref().expect("selected only when configured");
            let res = backend::run_linux_profile(lx, &cfg.workspace.join(&file), &req);
            linux_receipt(&req, &journal, res, liability)?
        }
        other => {
            // Selection returned a backend with no dispatcher: say so, keep
            // the liability — the launch record exists.
            let why = format!("backend {other} has no dispatcher");
            journal.fail(&req.operation_id, &why, Billing::Unknown)?;
            (
                receipt(
                    &req,
                    other,
                    Obs {
                        status: ReceiptStatus::Failed,
                        exit: None,
                        verification: ReceiptVerification::NotRun,
                        matched: None,
                        evidence: vec![],
                        liability_micro: liability,
                    },
                ),
                None,
                Some(why),
            )
        }
    };
    record_receipt(&journal, &req, &r, report.as_ref(), reason.as_deref())?;
    Ok(Submission {
        receipt: r,
        check_report: report,
        replayed: false,
        backend: Some(profile.id),
        reason,
    })
}

type Outcome = (ExecutionReceipt, Option<Value>, Option<String>);

fn local_receipt(
    req: &ComputeRequest,
    journal: &Journal,
    res: std::io::Result<CheckReport>,
    filter: Option<&str>,
    liability: u64,
) -> Result<Outcome, SubmitError> {
    let id = backend::LOCAL_INTERPRETER.id;
    match res {
        Ok(rep) => {
            // Verification is judged for the NAMED check (the filter), and
            // an empty match is not_run — never passed.
            let (verification, matched) = match filter {
                Some(name) => {
                    let m = rep
                        .passed
                        .iter()
                        .chain(rep.failed.iter())
                        .filter(|n| *n == name)
                        .count() as u64;
                    let v = if m == 0 {
                        ReceiptVerification::NotRun
                    } else if rep.failed.iter().any(|n| n == name) {
                        ReceiptVerification::Failed
                    } else {
                        ReceiptVerification::Passed
                    };
                    (v, m)
                }
                None => {
                    let m = (rep.passed.len() + rep.failed.len()) as u64;
                    let v = if m == 0 {
                        ReceiptVerification::NotRun
                    } else if rep.failed.is_empty() {
                        ReceiptVerification::Passed
                    } else {
                        ReceiptVerification::Failed
                    };
                    (v, m)
                }
            };
            let report_json = json!({
                "schema": "cortex-check-report/1",
                "failed": rep.failed,
                "passed": rep.passed,
                "total": rep.total,
                "exit_code": rep.exit_code,
            });
            let evidence = vec![opaque(format!(
                "cl22-report:{}",
                axon_loop_contracts::digest_value(&report_json)
                    .map(|r| r.to_string())
                    .unwrap_or_default()
            ))];
            // The process ran to completion (a summary was produced). The
            // exit code is the check run's, and verification is separate.
            let status = ReceiptStatus::Completed;
            let exit = rep.exit_code;
            let mut verification = verification;
            if verification == ReceiptVerification::Passed && exit != Some(0) {
                // Contract: passed requires exit 0. A nonzero exit with no
                // failing named test is not evidence of a pass.
                verification = ReceiptVerification::Unknown;
            }
            journal.complete(&req.operation_id, Billing::Unknown)?;
            Ok((
                receipt(
                    req,
                    id,
                    Obs {
                        status,
                        exit,
                        verification,
                        matched: Some(matched),
                        evidence,
                        liability_micro: liability,
                    },
                ),
                Some(report_json),
                None,
            ))
        }
        Err(e) if e.kind() == std::io::ErrorKind::TimedOut => {
            let why = e.to_string();
            journal.fail(&req.operation_id, &why, Billing::Unknown)?;
            Ok((
                receipt(
                    req,
                    id,
                    Obs {
                        status: ReceiptStatus::TimedOut,
                        exit: None,
                        verification: ReceiptVerification::Unknown,
                        matched: None,
                        evidence: vec![],
                        liability_micro: liability,
                    },
                ),
                None,
                Some(why),
            ))
        }
        Err(e) => {
            // Launched but no verdict (spawn failure after the launch record,
            // or no summary): failed, verification unknown, liability kept.
            let why = e.to_string();
            journal.fail(&req.operation_id, &why, Billing::Unknown)?;
            Ok((
                receipt(
                    req,
                    id,
                    Obs {
                        status: ReceiptStatus::Failed,
                        exit: None,
                        verification: ReceiptVerification::Unknown,
                        matched: None,
                        evidence: vec![],
                        liability_micro: liability,
                    },
                ),
                None,
                Some(why),
            ))
        }
    }
}

fn linux_receipt(
    req: &ComputeRequest,
    journal: &Journal,
    res: backend::LinuxRun,
    liability: u64,
) -> Result<Outcome, SubmitError> {
    let id = backend::LINUX_MICROVM_PROTECTED.id;
    let evidence: Vec<OpaqueRef> = res.evidence.iter().map(|e| opaque(e.clone())).collect();
    let (status, exit, verification) = match res.outcome {
        backend::LinuxOutcome::Ok { workload_exit } => (
            ReceiptStatus::Completed,
            Some(workload_exit),
            ReceiptVerification::NotRequested,
        ),
        backend::LinuxOutcome::WorkloadFailed { workload_exit } => (
            ReceiptStatus::Completed,
            Some(workload_exit),
            ReceiptVerification::NotRequested,
        ),
        backend::LinuxOutcome::TimedOut => {
            (ReceiptStatus::TimedOut, None, ReceiptVerification::Unknown)
        }
        backend::LinuxOutcome::Refused => {
            (ReceiptStatus::Denied, None, ReceiptVerification::NotRun)
        }
        backend::LinuxOutcome::Unknown => (
            ReceiptStatus::OutcomeUnknown,
            None,
            ReceiptVerification::Unknown,
        ),
    };
    match status {
        ReceiptStatus::OutcomeUnknown => {
            journal.fail(&req.operation_id, &res.reason, Billing::Unknown)?
        }
        ReceiptStatus::Completed => journal.complete(&req.operation_id, Billing::Unknown)?,
        _ => journal.fail(&req.operation_id, &res.reason, Billing::Unknown)?,
    }
    Ok((
        receipt(
            req,
            id,
            Obs {
                status,
                exit,
                verification,
                matched: None,
                evidence,
                liability_micro: liability,
            },
        ),
        None,
        Some(res.reason),
    ))
}

/// Journal a request that was refused BEFORE launch: intent, then a failed
/// terminal (never reserved, so nothing is held), then its receipt.
fn record_unlaunched(
    journal: &Journal,
    req: &ComputeRequest,
    input_digest: &Ref,
    cfg: &SubmitConfig,
    scope: &Scope,
    why: &str,
    r: &ExecutionReceipt,
) -> Result<(), SubmitError> {
    let intent = Intent {
        op: req.operation_id.clone(),
        task_id: req.task_id.clone(),
        trial_id: req.trial_id.clone(),
        attempt_id: req.attempt_id.clone(),
        input_digest: input_digest.clone(),
        config: json!({"backend": null}),
        authority_ref: format!("{}|{}", req.principal_ref, req.grant_ref),
        authority_epoch: cfg.expected_epoch,
        scope: scope.clone(),
        reservation: reservation(req),
        expected_version: 0,
    };
    journal.begin(intent)?;
    journal.fail(
        &req.operation_id,
        why,
        Billing::Known(ResourceVector::default()),
    )?;
    record_receipt(journal, req, r, None, Some(why))
}

fn record_receipt(
    journal: &Journal,
    req: &ComputeRequest,
    r: &ExecutionReceipt,
    report: Option<&Value>,
    reason: Option<&str>,
) -> Result<(), SubmitError> {
    journal.record_outcome(
        &req.operation_id,
        json!({"receipt": r, "check_report": report, "reason": reason}),
    )?;
    Ok(())
}

fn replayed(req: &ComputeRequest, v: &crate::journal::OpView) -> Submission {
    if let Some(o) = &v.outcome {
        if let Ok(r) = serde_json::from_value::<ExecutionReceipt>(o["receipt"].clone()) {
            return Submission {
                receipt: r,
                check_report: o.get("check_report").filter(|x| !x.is_null()).cloned(),
                replayed: true,
                backend: None,
                reason: o.get("reason").and_then(|x| x.as_str()).map(String::from),
            };
        }
    }
    // Recorded but with no outcome yet: in flight, or reconciled after a
    // crash. Never re-executed; report what the journal knows.
    let (status, why) = match v.state {
        OpState::OutcomeUnknown => (
            ReceiptStatus::OutcomeUnknown,
            "reconciled after restart: launched with no terminal record",
        ),
        OpState::Cancelled => (ReceiptStatus::Canceled, "cancelled"),
        OpState::Failed => (ReceiptStatus::Failed, "failed"),
        OpState::Completed => (
            ReceiptStatus::OutcomeUnknown,
            "completed but no receipt recorded",
        ),
        OpState::Intended | OpState::Reserved | OpState::Launched => (
            ReceiptStatus::OutcomeUnknown,
            "in flight (another submit owns it)",
        ),
    };
    let liability = if v.launched {
        v.intent.reservation.model_micro_usd
    } else {
        0
    };
    let r = receipt(
        req,
        "journal",
        Obs {
            status,
            exit: None,
            verification: if status == ReceiptStatus::OutcomeUnknown {
                ReceiptVerification::Unknown
            } else {
                ReceiptVerification::NotRun
            },
            matched: None,
            evidence: vec![],
            liability_micro: liability,
        },
    );
    Submission {
        receipt: r,
        check_report: None,
        replayed: true,
        backend: None,
        reason: Some(why.to_string()),
    }
}

/// Build a `Scope` from CLI strings.
pub fn scope(tenant: &str, family: &str) -> Result<Scope, String> {
    Ok(Scope {
        tenant_id: TenantId::new(tenant).map_err(|e| e.to_string())?,
        task_family: TaskFamily::new(family).map_err(|e| e.to_string())?,
    })
}
