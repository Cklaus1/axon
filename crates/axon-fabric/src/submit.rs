//! The Fabric submit path: `acf-compute-request/1` in, `acf-execution-receipt/1`
//! out.
//!
//! # Order of operations (each step a refusal, never a downgrade)
//!
//! 1. **Parse** the request strictly (`axon_loop_contracts::parse`); the
//!    all-zero `policy_digest` placeholder is refused. Then the **grant**:
//!    `grant_ref` is resolved, for `principal_ref`, from the operator's
//!    [`GrantRegistry`] to a real `axon_os::Grant` (file pinned by sha256,
//!    parsed by axon-os's own manifest parser). An unknown ref, a principal
//!    the ref is not bound to, or a changed grant file is refused before the
//!    journal is opened — nothing recorded, nothing spawned.
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
//! 6. **Supervisor admission** through axon-os `supervise_requiring` under
//!    THAT grant and its `require_approval` policy (token: the grant file's
//!    `.approval` sibling, verified by axon-os against the program + grant),
//!    with the program's scanned effect row and the request's isolation
//!    requirement — approval → isolation → intersect → admit, as for every
//!    axon-os job. The check then runs under `AXON_ALLOWED_EFFECTS` DERIVED
//!    from the admitted grant ([`crate::grants::effect_ceiling`]), never from
//!    an operator flag. The Linux profile receives the SAME ceiling as its
//!    guest policy (`--policy`, `axon-vm-mmds/1`), but a grant that withholds
//!    an effect is eligible there only when the signed qualification shows
//!    B263 x1 (the guest policy channel) as PASS.
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
use crate::grants::{GrantRegistry, ResolvedGrant};
use crate::journal::{Begin, Billing, Intent, Journal, JournalError, OpState, ResourceVector};
use crate::workspace::{self, Quota, TrialCache, WorkspaceStore, WorkspaceTree};

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
                // The authority store must EXIST and be CONFIGURED before its
                // epoch means anything. `Store::open_dir` creates a missing
                // root, and a store with no pointer answers epoch 0 — so an
                // absent, unmounted or mistyped store silently became a fresh
                // one at epoch 0 and authorized every request expecting 0.
                // Measured (G16-r22-peer-failure-matrix): with the store moved
                // away, a submit ran its check, at submit AND at the dispatch
                // recheck. An operator-configured store always has its
                // `config.json`; one without it is not an authority.
                if !store.join("config.json").is_file() {
                    return Err(format!(
                        "authority store {} is unavailable (no axon-loop config.json); an absent \
                         store is not epoch 0",
                        store.display()
                    ));
                }
                let st = axon_loop::Store::open_dir(store).map_err(|e| e.to_string())?;
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

/// The protected profile's one job: an operator suite with a named test.
pub const PROTECTED_SUITE_ONLY: &str = "the protected profile judges only an operator suite \
     (`check:<id>`) with a named test: there is no launch manifest for a candidate file or an \
     execution";

/// Why a protected host with no observer launches no protected check.
pub const NO_OBSERVER: &str = "this protected host configures no observer: a protected launch \
                               requires a preflight observation, so nothing is launched";

/// Operator configuration for one submit. Nothing here comes from the request.
#[derive(Debug, Clone)]
pub struct SubmitConfig {
    pub journal: PathBuf,
    pub registry: CheckRegistry,
    pub epoch: EpochSource,
    /// The epoch the caller was authorized under.
    pub expected_epoch: AuthorityEpoch,
    /// The directory a request's `argv[0]` file is resolved under when the
    /// request names a HISTORICAL single-file ref or a one-file
    /// WorkspaceVersion of that file (the latter is copied into the store
    /// before anything reads it).
    pub workspace: PathBuf,
    /// The Fabric state dir: the WorkspaceVersion store
    /// (`<state>/workspaces`), per-trial caches (`<state>/trial-caches`) and
    /// per-operation materializations (`<state>/runs`).
    pub state_dir: PathBuf,
    /// Aggregate ceiling for the scope (declared idempotently in the journal).
    pub budget: ResourceVector,
    /// The operator's grant registry: the ONLY source of a request's
    /// authority. The interpreter effect ceiling and the Linux-profile
    /// eligibility (B263 x1/x2) are derived from the grant resolved here.
    pub grants: GrantRegistry,
    /// Where the Linux microVM profile's launcher and evidence live.
    pub linux: Option<crate::backend::LinuxProfileConfig>,
    /// O1: the protected host config's identity, bound into every launch
    /// manifest. `None` off a protected host.
    pub protected_host: Option<crate::psv::HostIdentity>,
    /// M3: the operator's preflight observer. `None`: no observation, so no
    /// guest verdict is ever `protected` (it is `guest-unobserved`).
    pub observer: Option<crate::observer::ObserverConfig>,
    /// Test seam: called after the submit-time checks and the reservation,
    /// immediately before the dispatch-time epoch recheck. `None` in
    /// production (the CLI never sets it).
    pub pre_launch_hook: Option<fn(&SubmitConfig)>,
    /// Test seam (B280, G13-r22-restart-matrix): called at each journal effect
    /// boundary, so a crash child can die at exactly that point and a real
    /// restart can be tested from it. `None` in production (the CLI never
    /// sets it).
    pub fault_hook: Option<fn(Boundary)>,
}

/// The journal effect boundaries a restart can happen at (G13-r22-restart-matrix).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Boundary {
    /// `begin` recorded the intent; nothing reserved.
    AfterIntent,
    /// The reservation is carved; the launch record is not yet written.
    AfterReserve,
    /// The launch record is durable; the effect has not been dispatched.
    AfterLaunchRecord,
    /// The terminal record is durable; the receipt/outcome is not.
    AfterTerminal,
}

impl Boundary {
    pub fn name(self) -> &'static str {
        match self {
            Boundary::AfterIntent => "after_intent",
            Boundary::AfterReserve => "after_reserve",
            Boundary::AfterLaunchRecord => "after_launch_record",
            Boundary::AfterTerminal => "after_terminal",
        }
    }
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
    /// What the recorded operation actually RAN under, from its journal
    /// intent (for a replay too): the backend and the effect ceiling of the
    /// grant it was admitted with. `None` when nothing was launched. Anything
    /// that vouches for the result (the verifier's attestation) decides from
    /// this, never from the configuration of the call that asked.
    pub ran_under: Option<RanUnder>,
    /// B2: for a PROTECTED verdict, the `axon-psv-evidence/1` bundle its joins
    /// are verified over (delivered to intake with the receipt).
    pub psv_evidence: Option<Value>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RanUnder {
    pub backend: String,
    pub effect_ceiling: String,
    /// The receipt's evidence class (`psv::EvidenceClass`), which the
    /// attestation rule decides on (v022-psv-protocol.md §6).
    pub evidence_class: String,
}

fn ran_under_of(intent: &crate::journal::Intent) -> Option<RanUnder> {
    Some(RanUnder {
        backend: intent.config["backend"].as_str()?.to_string(),
        effect_ceiling: intent.config["grant"]["effect_ceiling"]
            .as_str()?
            .to_string(),
        // A replay is never signed (signing::REPLAYED); its class is not
        // re-derived from the journal.
        evidence_class: "unknown".into(),
    })
}

/// A refusal that never reached the journal's launch record.
#[derive(Debug)]
pub enum SubmitError {
    Malformed(String),
    Conflict(String),
    StaleEpoch {
        expected: u64,
        current: String,
    },
    Unregistered(String),
    /// `grant_ref` did not resolve to an operator grant for `principal_ref`.
    Unauthorized(String),
    /// The request's run is a logical branch that refuses it (cancelled, or
    /// its declared regime is exhausted) — B271.
    Branch(String),
    Journal(JournalError),
    /// The Fabric's workspace store / state dir failed (I/O, corruption).
    Workspace(String),
}

impl SubmitError {
    pub fn kind(&self) -> &'static str {
        match self {
            SubmitError::Malformed(_) => "malformed",
            SubmitError::Conflict(_) => "conflict",
            SubmitError::StaleEpoch { .. } => "stale_epoch",
            SubmitError::Unregistered(_) => "unregistered",
            SubmitError::Unauthorized(_) => "unauthorized",
            SubmitError::Branch(_) => "branch",
            SubmitError::Journal(_) => "journal",
            SubmitError::Workspace(_) => "workspace",
        }
    }
    /// CLI exit code.
    pub fn exit_code(&self) -> i32 {
        match self {
            SubmitError::Malformed(_) => 3,
            SubmitError::Conflict(_) => 5,
            SubmitError::StaleEpoch { .. } => 6,
            SubmitError::Unregistered(_) => 4,
            SubmitError::Unauthorized(_) => 7,
            SubmitError::Branch(_) => 9,
            SubmitError::Journal(_) | SubmitError::Workspace(_) => 2,
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
            SubmitError::Unauthorized(s) => write!(f, "unauthorized: {s}"),
            SubmitError::Branch(s) => write!(f, "branch: {s}"),
            SubmitError::Journal(e) => write!(f, "journal: {e}"),
            SubmitError::Workspace(e) => write!(f, "workspace store: {e}"),
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

/// The all-zero `policy_digest` a caller writes when it has no policy to
/// name. Refused: a receipt must not claim a governing policy that is none.
pub const PLACEHOLDER_POLICY_DIGEST: &str =
    "acf1:0000000000000000000000000000000000000000000000000000000000000000";

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// `acf1:` identity of a registered executable. Delegates to THE acf1
/// canonicaliser, `axon_cortex::runner::acf1_canonical_bytes`, which the
/// cortex side of the seam builds its requests with — one implementation,
/// not two kept equal by a test (D-C3).
pub fn executable_digest(id: &str, e: &RegisteredExecutable) -> Acf1Ref {
    Acf1Ref::new(axon_cortex::runner::fabric_executable_digest(id, &e.sha256))
        .expect("acf1 + sha256 hex")
}

/// `acf1:` identity of a single-file workspace version (same canonicaliser).
pub fn workspace_digest(rel_path: &str, bytes: &[u8]) -> Acf1Ref {
    Acf1Ref::new(axon_cortex::runner::fabric_workspace_digest(
        rel_path, bytes,
    ))
    .expect("acf1 + sha256 hex")
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
    /// The workspace the run LEFT (re-imported after it). `None` when
    /// nothing was launched or the backend produces no workspace.
    output: Option<Acf1Ref>,
}

fn receipt(req: &ComputeRequest, backend: &str, o: Obs) -> ExecutionReceipt {
    let Obs {
        status,
        exit,
        verification,
        matched,
        evidence,
        liability_micro,
        output,
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
        output_workspace_ref: output,
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

/// Where a request's target bytes live for the run.
#[derive(Debug)]
enum Bound {
    /// HISTORICAL single-file ref (`{"path","sha256"}`): the check reads the
    /// operator workspace in place.
    Legacy,
    /// A WorkspaceVersion from the store, materialized privately for this
    /// operation: `dir` is the copy, removed when the guard drops.
    Version { version: Acf1Ref, dir: RunDir },
}

/// A per-operation directory under `<state>/runs`, removed on drop — on
/// every path, refusal or not.
#[derive(Debug)]
struct RunDir(PathBuf);

impl RunDir {
    fn new(state: &Path, op: &str) -> Result<RunDir, SubmitError> {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        // A random component (C9 round 4c, amendment 71): pid and sequence
        // alone repeat after a crash and restart with the same pid (PID 1 in
        // a container), and the leftover dir of the crashed process then
        // refused every retry of the same operation until the sequence
        // number passed it. With 64 random bits a name never meets a
        // leftover, and no other uid can aim a planted dir at it.
        let mut rnd = [0u8; 8];
        ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut rnd)
            .map_err(|_| SubmitError::Workspace("no randomness for the run dir name".into()))?;
        let suffix = format!(
            "-{}",
            rnd.iter().map(|b| format!("{b:02x}")).collect::<String>()
        );
        let key = format!(
            "{}-{}-{}{suffix}",
            &sha256_hex(op.as_bytes())[..16],
            std::process::id(),
            SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        );
        let runs = state.join("runs");
        std::fs::create_dir_all(&runs).map_err(|e| SubmitError::Workspace(e.to_string()))?;
        RunDir::create_new(&runs, &key)
    }

    /// The run's own dir is created NEW (C9 round 4b): an existing dir at
    /// the name, a leftover or one planted there, is never reused, so
    /// nothing materialized into it meets stale files. The random
    /// component of the name makes that refusal a collision, not a retry's
    /// fate. (A function of its own so a test can meet a name that exists:
    /// the random name cannot be aimed at.)
    fn create_new(runs: &Path, key: &str) -> Result<RunDir, SubmitError> {
        let p = runs.join(key);
        std::fs::create_dir(&p)
            .map_err(|e| SubmitError::Workspace(format!("{}: {e}", p.display())))?;
        Ok(RunDir(p))
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = workspace::remove_tree(&self.0);
    }
}

/// What a request's `argv` resolved to.
#[derive(Debug)]
struct Target {
    /// The file the interpreter runs: the candidate's `argv[0]`, or a check
    /// suite's entry.
    file: String,
    filter: Option<String>,
    /// The CANDIDATE's bytes.
    bound: Bound,
    /// A registered check suite (`argv[0] = "check:<id>"`): its own
    /// WorkspaceVersion, materialized read-only beside — never inside — the
    /// candidate.
    suite: Option<Suite>,
}

#[derive(Debug)]
struct Suite {
    id: String,
    version: Acf1Ref,
    /// `check-suite:<id>@<version>#<entry>`, written once by the one writer
    /// (which refuses a suite whose reference would read as another: A81).
    reference: String,
}

impl Target {
    /// The directory the interpreter reads `file` under.
    fn dir(&self, cfg: &SubmitConfig) -> PathBuf {
        match (&self.suite, &self.bound) {
            (Some(_), Bound::Version { dir, .. }) => dir.0.join("check"),
            (_, Bound::Legacy) => cfg.workspace.clone(),
            (None, Bound::Version { dir, .. }) => dir.0.join("candidate"),
        }
    }

    /// For a check suite, the text the admission probe scans: the entry
    /// with each `mod NAME` line replaced by the candidate's `NAME.ax` (the
    /// module the suite will actually load through `AXON_PATH`). A module
    /// the candidate does not hold stays a `mod` line, which scans as EVERY
    /// effect (deny-by-default) — never as none. `None` for a plain file.
    fn scan_source(&self) -> Option<String> {
        let (Some(_), Some(cand)) = (&self.suite, self.candidate_dir()) else {
            return None;
        };
        let Bound::Version { dir, .. } = &self.bound else {
            return None;
        };
        let entry = std::fs::read_to_string(dir.0.join("check").join(&self.file)).ok()?;
        let mut out = String::new();
        for line in entry.lines() {
            let name = line.trim().strip_prefix("mod ").map(str::trim);
            let inlined = name
                .filter(|n| !n.is_empty() && n.chars().all(|c| c.is_alphanumeric() || c == '_'))
                .and_then(|n| {
                    // The module the runtime will load: first match along the
                    // same path `AXON_PATH` gets, suite before candidate.
                    [dir.0.join("check"), cand.clone()]
                        .iter()
                        .map(|d| d.join(format!("{n}.ax")))
                        .find(|p| std::fs::symlink_metadata(p).is_ok_and(|m| m.is_file()))
                        .and_then(|p| std::fs::read_to_string(p).ok())
                });
            match inlined {
                Some(module) => out.push_str(&module),
                None => out.push_str(line),
            }
            out.push('\n');
        }
        Some(out)
    }

    /// `AXON_PATH` for the run. A check suite's own directory comes FIRST:
    /// a module the operator's suite ships (a helper, a fixture) resolves to
    /// the suite's file, so a candidate holding a same-named module cannot
    /// shadow the rubric's code. The candidate follows, reachable only as the
    /// modules the suite does not define. A plain file sees the candidate.
    fn module_path(&self) -> String {
        let join = |ds: &[PathBuf]| {
            ds.iter()
                .map(|d| d.to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join(":")
        };
        match (&self.suite, &self.bound) {
            (Some(_), Bound::Version { dir, .. }) => {
                join(&[dir.0.join("check"), dir.0.join("candidate")])
            }
            _ => self.candidate_dir().map(|d| join(&[d])).unwrap_or_default(),
        }
    }

    /// The private copy of the candidate, when there is one.
    fn candidate_dir(&self) -> Option<PathBuf> {
        match &self.bound {
            Bound::Version { dir, .. } => Some(dir.0.join("candidate")),
            Bound::Legacy => None,
        }
    }
}

/// What the Fabric saw AFTER the run.
struct PostRun {
    output: Option<Acf1Ref>,
    /// Why the verdict cannot stand for the candidate, if it cannot.
    problem: Option<String>,
}

/// Re-derive the workspace the run left (the receipt's
/// `output_workspace_ref`), and whether the verdict still names the
/// candidate: the output must equal the input, and a check suite's own
/// bytes must be what was registered. An output version is PUBLISHED, so a
/// receipt's output ref is always retrievable.
fn post_run(req: &ComputeRequest, cfg: &SubmitConfig, t: &Target) -> PostRun {
    let input = &req.workspace_version_ref;
    let mut problem = None;
    if let (Some(s), Bound::Version { dir, .. }) = (&t.suite, &t.bound) {
        match WorkspaceTree::import_dir(&dir.0.join("check"), &Quota::default()) {
            Ok(tr) if tr.reference() == s.version => {}
            Ok(tr) => {
                problem = Some(format!(
                    "check suite `{}` changed during the run ({} → {})",
                    s.id,
                    s.version,
                    tr.reference()
                ))
            }
            Err(e) => {
                problem = Some(format!(
                    "check suite `{}` unreadable after the run: {e}",
                    s.id
                ))
            }
        }
    }
    let output = match &t.bound {
        Bound::Legacy => std::fs::read(cfg.workspace.join(&t.file))
            .ok()
            .map(|b| workspace_digest(&t.file, &b)),
        Bound::Version { dir, .. } => {
            let tree = WorkspaceTree::import_dir(&dir.0.join("candidate"), &Quota::default());
            match tree.map_err(|e| e.to_string()).and_then(|tr| {
                WorkspaceStore::open(&cfg.state_dir, &cfg.epoch.scope().tenant_id)
                    .and_then(|st| st.publish(&tr))
                    .map_err(|e| e.to_string())
            }) {
                Ok(r) => Some(r),
                Err(e) => {
                    problem.get_or_insert(format!("candidate unreadable after the run: {e}"));
                    None
                }
            }
        }
    };
    match &output {
        Some(o) if o == input => {}
        Some(o) => {
            problem.get_or_insert(format!(
                "the run changed the candidate ({input} → {o}); the verdict names neither"
            ));
        }
        None => {
            problem.get_or_insert("no output workspace could be observed".into());
        }
    }
    PostRun { output, problem }
}

/// `argv` for a registered check is `[file]` or `[file, filter]`; for an
/// interpreter run it is `[program.ax]`. The file must be a plain relative
/// path, and `workspace_version_ref` must name the bytes the run will read:
///
/// 1. a WorkspaceVersion the store holds — materialized privately, and the
///    file must be a regular-file entry of it;
/// 2. the one-file WorkspaceVersion of the workspace file — the file is
///    imported (copied) into the store first and the run reads THAT copy,
///    so the bytes judged are the bytes hashed;
/// 3. the historical single-file digest of the workspace file (kept so old
///    refs still resolve; the run reads the workspace in place).
///
/// Anything else is a conflict. Every refusal happens before admission.
fn check_target(req: &ComputeRequest, cfg: &SubmitConfig) -> Result<Target, SubmitError> {
    let (file, filter) = match (req.job_kind, req.argv.as_slice()) {
        (JobKind::RegisteredCheck, [f]) => (f.clone(), None),
        (JobKind::RegisteredCheck, [f, flt]) => (f.clone(), Some(flt.clone())),
        (JobKind::RegisteredCheck, _) => {
            return Err(SubmitError::Malformed(
                "registered_check argv must be [file] or [file, filter]".into(),
            ))
        }
        (JobKind::InterpreterRun, [f]) => (f.clone(), None),
        (JobKind::InterpreterRun, _) => {
            return Err(SubmitError::Malformed(
                "interpreter_run argv must be [program.ax]".into(),
            ))
        }
    };
    if let Some(id) = file.strip_prefix("check:") {
        return check_suite_target(req, cfg, id, filter);
    }
    let p = Path::new(&file);
    if p.is_absolute()
        || p.components()
            .any(|c| !matches!(c, std::path::Component::Normal(_)))
    {
        return Err(SubmitError::Malformed(format!(
            "argv file {file:?} must be a plain relative path inside the workspace"
        )));
    }
    let store = WorkspaceStore::open(&cfg.state_dir, &cfg.epoch.scope().tenant_id)
        .map_err(|e| SubmitError::Workspace(e.to_string()))?;
    let want = &req.workspace_version_ref;
    let version = if store.contains(want) {
        want.clone()
    } else {
        let bytes = std::fs::read(cfg.workspace.join(p))
            .map_err(|e| SubmitError::Malformed(format!("cannot read {file}: {e}")))?;
        if want.as_str() == axon_cortex::runner::single_file_workspace_version_ref(&file, &bytes) {
            // Copy the file into the store and judge the COPY: re-derive the
            // ref from what was stored, not from what was read above.
            let tree = WorkspaceTree::import_file(&cfg.workspace, &file, &Quota::default())
                .map_err(|e| SubmitError::Malformed(format!("workspace import refused: {e}")))?;
            let r = store
                .publish(&tree)
                .map_err(|e| SubmitError::Workspace(e.to_string()))?;
            if r != *want {
                return Err(SubmitError::Conflict(format!(
                    "workspace_version_ref {want} does not match the imported bytes of {file} ({r})"
                )));
            }
            r
        } else if *want == workspace_digest(&file, &bytes) {
            return Ok(Target {
                file,
                filter,
                bound: Bound::Legacy,
                suite: None,
            });
        } else {
            return Err(SubmitError::Conflict(format!(
                "workspace_version_ref {want} names neither a published WorkspaceVersion \
                 nor the bytes of {file}"
            )));
        }
    };
    let v = store
        .load(&version)
        .map_err(|e| SubmitError::Conflict(e.to_string()))?;
    refuse_links(
        v.entries
            .iter()
            .map(|e| (e.path.as_str(), e.mode == workspace::MODE_LINK)),
        "the candidate",
    )?;
    if !v
        .entries
        .iter()
        .any(|e| e.path == file && e.mode != workspace::MODE_LINK)
    {
        return Err(SubmitError::Malformed(format!(
            "argv file {file:?} is not a regular file of WorkspaceVersion {version}"
        )));
    }
    let dir = RunDir::new(&cfg.state_dir, req.operation_id.as_str())?;
    store
        .materialize(&version, &dir.0.join("candidate"), false)
        .map_err(|e| SubmitError::Conflict(e.to_string()))?;
    Ok(Target {
        file,
        filter,
        bound: Bound::Version { version, dir },
        suite: None,
    })
}

/// 32 bytes from the OS RNG for one run's completion secret.
fn fresh_completion_key() -> Result<Vec<u8>, SubmitError> {
    use std::io::Read;
    let mut k = vec![0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut k))
        .map_err(|e| {
            SubmitError::Workspace(format!("no randomness for the completion key: {e}"))
        })?;
    Ok(k)
}

/// A check Fabric may vouch for runs over plain files and directories only.
/// Import checks a link's target LEXICALLY, and a chain of links (one to a
/// parent, the next through it) resolves outside the tree once materialized —
/// so a candidate could make its modules load from the trial cache or any
/// absolute path, and the same tree ref pass or fail on bytes outside it
/// (v0.22 G01 final re-audit, executed). Refused before anything is launched.
fn refuse_links<'a>(
    paths: impl Iterator<Item = (&'a str, bool)>,
    what: &str,
) -> Result<(), SubmitError> {
    for (path, is_link) in paths {
        if is_link {
            return Err(SubmitError::Malformed(format!(
                "{what} holds a symbolic link ({path}): a check runs only over plain files, so \
                 nothing outside the tree can decide its verdict"
            )));
        }
    }
    Ok(())
}

/// `argv = ["check:<id>", filter?]`: the operator-registered check suite
/// `<id>` judges the candidate, which must be a PUBLISHED WorkspaceVersion.
/// The suite root is imported at dispatch and must still be the version the
/// operator pinned; it is materialized read-only into `<run>/check`, the
/// candidate into `<run>/candidate`, and the suite reaches the candidate
/// only through `AXON_PATH`. Neither directory contains the other.
fn check_suite_target(
    req: &ComputeRequest,
    cfg: &SubmitConfig,
    id: &str,
    filter: Option<String>,
) -> Result<Target, SubmitError> {
    if req.job_kind != JobKind::RegisteredCheck {
        return Err(SubmitError::Malformed(
            "a registered check suite runs only as registered_check".into(),
        ));
    }
    let c = cfg
        .registry
        .check(id)
        .ok_or_else(|| SubmitError::Unregistered(format!("check suite `{id}` is not registered")))?
        .clone();
    let store = WorkspaceStore::open(&cfg.state_dir, &cfg.epoch.scope().tenant_id)
        .map_err(|e| SubmitError::Workspace(e.to_string()))?;
    let cand = &req.workspace_version_ref;
    if !store.contains(cand) {
        return Err(SubmitError::Conflict(format!(
            "check suite `{id}` judges a published WorkspaceVersion; {cand} is not one"
        )));
    }
    let cv = store
        .load(cand)
        .map_err(|e| SubmitError::Conflict(e.to_string()))?;
    refuse_links(
        cv.entries
            .iter()
            .map(|e| (e.path.as_str(), e.mode == workspace::MODE_LINK)),
        "the candidate",
    )?;
    let tree = WorkspaceTree::import_dir(&c.root, &Quota::default())
        .map_err(|e| SubmitError::Unregistered(format!("check suite `{id}` refused: {e}")))?;
    refuse_links(
        tree.entries().iter().map(|e| {
            (
                e.path.as_str(),
                matches!(e.kind, workspace::EntryKind::Symlink),
            )
        }),
        &format!("check suite `{id}`"),
    )?;
    if tree.reference().as_str() != c.workspace_version_ref {
        return Err(SubmitError::Unregistered(format!(
            "check suite `{id}` is {} on disk, not the registered {}",
            tree.reference(),
            c.workspace_version_ref
        )));
    }
    if !tree
        .entries()
        .iter()
        .any(|e| e.path == c.entry && matches!(e.kind, workspace::EntryKind::File { .. }))
    {
        return Err(SubmitError::Unregistered(format!(
            "check suite `{id}` has no regular file `{}`",
            c.entry
        )));
    }
    let version = store
        .publish(&tree)
        .map_err(|e| SubmitError::Workspace(e.to_string()))?;
    // The registered suite that judged the candidate travels in the receipt
    // (and so under the verifier's attestation): a consumer can require an
    // operator-pinned suite, never a file the subject wrote
    // (G01-r22-verifier-separation). The ENTRY file is part of that identity:
    // the check registry is caller-named, so without it a registry could run
    // another file of the pinned suite tree under the same id@version
    // (re-audit 3).
    let reference = axon_cortex::runner::check_suite_ref(id, version.as_str(), &c.entry)
        .map_err(|e| SubmitError::Unregistered(format!("check suite `{id}` refused: {e}")))?;
    let dir = RunDir::new(&cfg.state_dir, req.operation_id.as_str())?;
    store
        .materialize(cand, &dir.0.join("candidate"), false)
        .map_err(|e| SubmitError::Conflict(e.to_string()))?;
    store
        .materialize(&version, &dir.0.join("check"), true)
        .map_err(|e| SubmitError::Workspace(e.to_string()))?;
    Ok(Target {
        file: c.entry,
        filter,
        bound: Bound::Version {
            version: cand.clone(),
            dir,
        },
        suite: Some(Suite {
            id: id.to_string(),
            version,
            reference,
        }),
    })
}

/// An axon-os `Runtime` that performs NO effect: it states the selected
/// backend's isolation and the program's SCANNED effect row
/// (`axon_os::runtime::scan_effects`, deny-by-default: unreadable ⇒ every
/// effect). `supervise_requiring` over it answers one question through the
/// code path every axon-os job takes — may this program run on this backend
/// under THIS grant, with THIS approval policy? The real effect runs later,
/// after the journal's launch record, bounded by the same grant's ceiling.
struct AdmissionProbe(axon_os::Isolation, Option<String>);

impl axon_os::Runtime for AdmissionProbe {
    fn isolation(&self) -> axon_os::Isolation {
        self.0
    }
    fn declared_effects(&self, p: &Path) -> axon_os::DeclaredEffects {
        if let Some(src) = &self.1 {
            return axon_os::runtime::scan_effects(src);
        }
        match std::fs::read_to_string(p) {
            Ok(src) => axon_os::runtime::scan_effects(&src),
            Err(_) => axon_os::DeclaredEffects::unknown(),
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

/// Whether a request's cost limit exceeds the grant's budget. A limit that does not fit an `i64` exceeds
/// every budget (amendment 110: that arm was `map_or(true, ..)` inline, and `false` kept every suite green).
fn exceeds_budget(max_cost_micro: u64, cap: i64) -> bool {
    i64::try_from(max_cost_micro).map_or(true, |c| c > cap)
}

/// Admit `program` under the resolved grant. Returns the axon-os approval
/// status on success.
fn supervisor_admits(
    req: &ComputeRequest,
    profile: &Profile,
    grant: &ResolvedGrant,
    program: &Path,
    scan_source: Option<String>,
) -> Result<String, String> {
    use axon_os::IsolationRequirement;
    let requirement =
        if req.required.hardware_isolation && req.required.os == axon_loop_contracts::Os::Linux {
            IsolationRequirement::MicroVm
        } else if req.required.hardware_isolation {
            IsolationRequirement::HardwareIsolated
        } else {
            IsolationRequirement::Any
        };
    // The request may not spend more than its grant allows.
    let cap = grant.grant().budget.cost_micro;
    if exceeds_budget(req.limits.max_cost_micro, cap) {
        return Err(format!(
            "limits.max_cost_micro {} exceeds grant `{}` budget.cost_micro {cap}",
            req.limits.max_cost_micro, grant.grant_ref
        ));
    }
    let manifest = grant.manifest_for(program, format!("fabric {}", req.operation_id));
    let rt = AdmissionProbe(profile.isolation, scan_source);
    // The job path is the GRANT FILE: its `.approval` sibling is the sign-off
    // token axon-os verifies against (program, grant).
    let rec = axon_os::supervise_requiring(
        &manifest,
        &grant.path,
        grant.grant(),
        req.operation_id.as_str(),
        requirement,
        &rt,
    );
    match rec.verdict {
        axon_os::Verdict::Completed { .. } => Ok(rec.approval),
        other => Err(format!("axon-os supervisor refused: {other:?}")),
    }
}

/// The host interpreter executor, pinned to exactly the resolved entry and
/// bounded by the request's limits and the ADMITTED grant's effect ceiling.
fn host_executor(
    exe: &RegisteredExecutable,
    req: &ComputeRequest,
    ceiling: &str,
    cache: &TrialCache,
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
        .with_max_output(req.limits.output_bytes as usize)
        // Nothing of the launcher's environment steers a verdict (re-audit
        // 3): only the ceiling, the trial cache and the module path below.
        .with_clean_env();
    // Always set — `""` is deny-every-effect, never "no ceiling".
    let mut local = local
        .with_effect_ceiling(ceiling)
        // Modules resolve ONLY from the module path set below — never from
        // the trial cache's `~/.axon/lib` (under the caller-named state dir)
        // or the interpreter's own library (re-audit 5, executed).
        .with_env("AXON_PATH_EXCLUSIVE", "1");
    // The trial's own fresh HOME / XDG_CACHE_HOME / CARGO_TARGET_DIR: no
    // two trials share a mutable cache.
    for (k, v) in cache.env() {
        local = local.with_env(k, v);
    }
    Ok(local)
}

/// Submit one request. See the module docs for the order of operations.
pub fn submit(req_json: &str, cfg: &SubmitConfig) -> Result<Submission, SubmitError> {
    // 1. Parse.
    let req: ComputeRequest =
        axon_loop_contracts::parse(req_json).map_err(|e| SubmitError::Malformed(e.to_string()))?;
    let input_digest: Ref =
        axon_loop_contracts::digest(&req).map_err(|e| SubmitError::Malformed(e.to_string()))?;
    if req.policy_digest.as_str() == PLACEHOLDER_POLICY_DIGEST {
        return Err(SubmitError::Malformed(
            "policy_digest is the all-zero placeholder; name the governing policy".into(),
        ));
    }
    // Every run path — the run directory, the trial caches, and the module
    // path a check resolves through — lives under the caller-named state dir,
    // and AXON_PATH is a ':'-separated list. A ':' in the state dir would split
    // it into directories the CALLER chose, ahead of the operator's suite
    // (re-audit 5, executed: a signed pass for a candidate the suite fails).
    // Refused before anything is written or launched.
    if cfg.state_dir.as_os_str().to_string_lossy().contains(':') {
        return Err(SubmitError::Malformed(format!(
            "state dir {} contains ':', the module-path separator: a run under it could not \
             keep its module path to the directories it names",
            cfg.state_dir.display()
        )));
    }

    // 4a (first, so a refusal touches nothing — not even the journal file).
    // D1: on a PROTECTED host the registry is the operator's pinned one and
    // nothing else — whatever put `cfg.grants` there, a registry whose bytes
    // are not the pin (or a host that pins none) authorizes nothing. This
    // also fixes the guest effect policy, which is derived from the grant.
    if let Some(h) = &cfg.protected_host {
        match &h.grant_registry_sha256 {
            None => {
                return Err(SubmitError::Unauthorized(
                    crate::protected_host::NO_GRANT_REGISTRY.to_string(),
                ))
            }
            Some(pin) if !pin.eq_ignore_ascii_case(cfg.grants.sha256()) => {
                return Err(SubmitError::Unauthorized(format!(
                    "on a protected host the grant registry is the operator's (sha256 {pin}); \
                     this one is sha256 {}",
                    cfg.grants.sha256()
                )))
            }
            Some(_) => {}
        }
    }
    // The request's grant, from the operator's registry. Unknown ref, a
    // principal it is not bound to, or a changed grant file: refused.
    let grant = cfg
        .grants
        .resolve(req.grant_ref.as_str(), req.principal_ref.as_str())
        .map_err(SubmitError::Unauthorized)?;

    let (journal, _recovery) = Journal::open(&cfg.journal)?;
    let scope = cfg.epoch.scope().clone();
    journal.declare_budget(&scope, cfg.budget)?;

    // 2. Dedup by operation id.
    //
    // An op recorded but NEVER LAUNCHED (Intended / Reserved) is an ORPHAN, not
    // "in flight": every submit holds this journal's exclusive lock for its
    // whole run, and we hold it now, so no live submit owns it. The launch
    // record is written before any effect, so it has had no effect either —
    // running it now is its FIRST execution, not a repeat. It is RESUMED
    // through every check below (authority, branch, backend, admission). It
    // used to be reported as "in flight (another submit owns it)" forever,
    // with its budget held, which named an owner that did not exist
    // (G13-r22-restart-matrix). Launched-but-unfinished ops were already
    // reconciled to OutcomeUnknown by `Journal::open` and are never re-run.
    let mut resume: Option<OpState> = None;
    if let Some(v) = journal.view(&req.operation_id) {
        if v.intent.input_digest != input_digest {
            return Err(SubmitError::Conflict(format!(
                "operation {} is already recorded with input {}, not {}",
                req.operation_id, v.intent.input_digest, input_digest
            )));
        }
        match v.state {
            OpState::Intended | OpState::Reserved => {
                // Its intent was recorded under an authority epoch; a caller
                // authorized under a different one cannot adopt it.
                if v.intent.authority_epoch != cfg.expected_epoch {
                    journal.cancel(
                        &req.operation_id,
                        "orphaned before launch under a superseded authority epoch",
                        None,
                    )?;
                    return Err(SubmitError::StaleEpoch {
                        expected: cfg.expected_epoch.get(),
                        current: format!(
                            "{} (the orphaned intent's epoch; cancelled, released)",
                            v.intent.authority_epoch.get()
                        ),
                    });
                }
                resume = Some(v.state);
            }
            _ => return Ok(replayed(&req, &v)),
        }
    }

    // 3. Authority epoch at submit.
    let current = cfg.epoch.current();
    if current.as_ref().ok() != Some(&cfg.expected_epoch) {
        return Err(SubmitError::StaleEpoch {
            expected: cfg.expected_epoch.get(),
            current: current.map(|e| e.get().to_string()).unwrap_or_else(|e| e),
        });
    }

    // 3b. Logical branch (B271): a run id that is a branch's is bound by that
    //     branch — refused outright once the branch is cancelled.
    let branches = crate::branches::Branches::open(&cfg.state_dir, &scope);
    let branch = branches
        .branch_of_run(&req.trial_id)
        .map_err(|e| SubmitError::Workspace(e.to_string()))?;
    if let Some((exp, br)) = &branch {
        if branches.is_cancelled(&exp.experiment_id, &br.arm_id) {
            // A pre-launch orphan of a cancelled branch is released here: a
            // crash inside `Branches::cancel` (marker written, ops not yet
            // cancelled) must not leave its reservation held until someone
            // happens to re-run the cancel. Nothing launched, so nothing to bill.
            if resume.is_some() {
                journal.cancel(
                    &req.operation_id,
                    "orphaned before launch in a cancelled branch",
                    None,
                )?;
            }
            return Err(SubmitError::Branch(format!(
                "branch {}/{} is cancelled",
                exp.experiment_id, br.arm_id
            )));
        }
    }

    // 4. Backend selection. A request nothing satisfies gets an
    //    `unsupported` receipt — journalled (intent + failed, never launched)
    //    so a retry returns the same answer.
    let needs = backend::AuthorityNeeds {
        guest_policy_channel: crate::grants::restricts_effects(grant.grant()),
        path_scoped_grant: crate::grants::is_path_scoped(grant.grant()),
        reproducible: grant.grant().reproducible,
    };
    // The interpreter effect ceiling the ADMITTED grant induces. The Linux
    // profile carries it into the guest as its policy, which must fit the
    // guest cmdline — a policy that does not is refused like any other
    // unsatisfiable requirement, before anything is launched.
    let ceiling = crate::grants::effect_ceiling(grant.grant());
    let selected = backend::select(&req, cfg.linux.as_ref(), needs).and_then(|p| {
        // A PROTECTED host runs nothing outside the protected profile: a local
        // dispatch would run workload code in the host's own privilege domain,
        // beside the protected attempts' custody (dev review round
        // wf_7cb5856d-806, PSV-3).
        if cfg.protected_host.is_some() && p.id != backend::LINUX_MICROVM_PROTECTED.id {
            return Err(backend::Unsupported(format!(
                "this is a protected host: only {} runs here, not {}",
                backend::LINUX_MICROVM_PROTECTED.id,
                p.id
            )));
        }
        // A protected launch needs a preflight observation: a protected host
        // whose config has no observer section could only ever produce
        // `guest-unobserved` verdicts, so it launches nothing (PSV-6, C9
        // certifying review). Refused here, before any reservation or launch,
        // whatever the job kind (C9 dev review round 1; A54).
        if cfg.protected_host.is_some()
            && cfg.observer.is_none()
            && p.id == backend::LINUX_MICROVM_PROTECTED.id
        {
            return Err(backend::Unsupported(NO_OBSERVER.to_string()));
        }
        if p.id == backend::LINUX_MICROVM_PROTECTED.id {
            backend::GuestPolicy::for_grant(&req, &ceiling).map(|g| (p, Some(g)))
        } else {
            Ok((p, None))
        }
    });
    let (profile, guest_policy) = match selected {
        Ok(s) => s,
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
                    output: None,
                },
            );
            record_unlaunched(&journal, &req, &input_digest, cfg, &scope, &why, &r)?;
            return Ok(Submission {
                receipt: r,
                check_report: None,
                replayed: false,
                backend: None,
                reason: Some(why),
                ran_under: None,
                psv_evidence: None,
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
    let target = check_target(&req, cfg)?;
    // The protected profile judges ONLY an operator suite, through the trusted
    // guest runner (PSV). A candidate-named file is never a protected check.
    if profile.id == backend::LINUX_MICROVM_PROTECTED.id
        && (target.suite.is_none() || target.filter.is_none())
    {
        return Err(SubmitError::Unregistered(format!(
            "{} runs a registered_check only as an operator suite (`check:<id>`) with a named \
             test",
            profile.id
        )));
    }

    // 6. Supervisor admission (axon-os).
    let program = target.dir(cfg).join(&target.file);
    let approval = match supervisor_admits(&req, &profile, &grant, &program, target.scan_source()) {
        Ok(a) => a,
        Err(why) => {
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
                    output: None,
                },
            );
            record_unlaunched(&journal, &req, &input_digest, cfg, &scope, &why, &r)?;
            return Ok(Submission {
                receipt: r,
                check_report: None,
                replayed: false,
                backend: Some(profile.id),
                reason: Some(why),
                ran_under: None,
                psv_evidence: None,
            });
        }
    };

    // 7. Journal around the effect.
    let intent = Intent {
        op: req.operation_id.clone(),
        task_id: req.task_id.clone(),
        trial_id: req.trial_id.clone(),
        attempt_id: req.attempt_id.clone(),
        input_digest: input_digest.clone(),
        config: json!({
            "backend": profile.id,
            "limits": req.limits,
            "grant": {
                "grant_ref": grant.grant_ref,
                "sha256": grant.sha256,
                // D1: WHICH registry authorized this op (the caller's, in
                // development; the operator's pin, on a protected host).
                "registry_sha256": cfg.grants.sha256(),
                "effect_ceiling": ceiling,
                "reproducible": grant.grant().reproducible,
                "approval": approval,
            },
            // Which bytes the run reads: a stored WorkspaceVersion, or the
            // operator workspace in place (historical single-file ref).
            "workspace": match &target.bound {
                Bound::Version { version, .. } => json!({"workspace_version_ref": version}),
                Bound::Legacy => json!({"legacy_single_file": target.file}),
            },
            "branch": branch.as_ref().map(|(e, b)| json!({
                "experiment": e.experiment_id, "arm": b.arm_id,
            })),
            // The suite's identity only — never its bytes.
            "check_suite": target.suite.as_ref().map(|s| json!({
                "id": s.id, "workspace_version_ref": s.version, "entry": target.file,
            })),
        }),
        authority_ref: format!("{}|{}", req.principal_ref, req.grant_ref),
        authority_epoch: cfg.expected_epoch,
        scope: scope.clone(),
        reservation: reservation(&req),
        expected_version: 0,
    };
    if resume.is_none() {
        if let Begin::AlreadyRecorded(v) = journal.begin(intent)? {
            // Raced with a concurrent submit of the same op: never run twice.
            return Ok(replayed(&req, &v));
        }
        fault(cfg, Boundary::AfterIntent);
    }
    match &branch {
        // A resumed orphan that already holds its reservation keeps it.
        _ if resume == Some(OpState::Reserved) => {}
        // A branch's ops carve from the scope AND within the branch's regime.
        Some((exp, br)) => {
            let run = br.run_id.clone();
            let sc = scope.clone();
            if let Err(e) = journal.reserve_within(&req.operation_id, exp.regime, move |i| {
                i.scope == sc && i.trial_id == run
            }) {
                journal.cancel(&req.operation_id, &format!("not reserved: {e}"), None)?;
                return Err(SubmitError::Branch(format!(
                    "branch {}/{} regime refuses the reservation: {e}",
                    exp.experiment_id, br.arm_id
                )));
            }
        }
        None => journal.reserve(&req.operation_id)?,
    }

    fault(cfg, Boundary::AfterReserve);
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
    let run_dir = target.dir(cfg);
    let (file, filter) = (target.file.clone(), target.filter.clone());
    let is_linux = profile.id == backend::LINUX_MICROVM_PROTECTED.id;
    // Re-verify the executable immediately before the launch record: a host
    // binary against its registry pin, the Linux profile against its
    // qualification (manifest vs evidence).
    let qualified = if is_linux {
        match cfg.linux.as_ref().expect("configured").qualification() {
            Ok(q) => Some(q),
            Err(e) => {
                journal.cancel(
                    &req.operation_id,
                    &format!("profile no longer qualified: {e}"),
                    None,
                )?;
                return Err(SubmitError::Unregistered(e));
            }
        }
    } else {
        None
    };
    // A fresh completion secret per run: the check cannot know it, so it
    // cannot forge the evidence that its test completed.
    let completion_key = fresh_completion_key()?;
    let local = if is_linux {
        None
    } else {
        // Past the reservation, every refusal CANCELS (released: nothing
        // was launched) — a `?` here would strand the reservation as held.
        let built =
            TrialCache::for_trial(&cfg.state_dir, &cfg.epoch.scope().tenant_id, &req.trial_id)
                .map_err(|e| SubmitError::Workspace(e.to_string()))
                .and_then(|cache| host_executor(&exe, &req, &ceiling, &cache));
        let mut l = match built {
            Ok(l) => l,
            Err(e) => {
                journal.cancel(&req.operation_id, &format!("not launched: {e}"), None)?;
                return Err(e);
            }
        };
        // A check suite reaches the candidate ONLY as a module path, after
        // its own directory; the operator's ambient AXON_PATH is never
        // inherited.
        l = l
            .with_env("AXON_PATH", target.module_path())
            .with_completion_key(completion_key.clone());
        // A registered suite runs with the CANDIDATE sealed: its modules may
        // use builtins and their own names, never a name the operator's suite
        // defines (Protected Check Isolation; E0004).
        if let (Some(_), Some(cand)) = (&target.suite, target.candidate_dir()) {
            l = l.with_sealed_dir(cand);
        }
        Some(l)
    };
    if let Some(Err(e)) = local.as_ref().map(|l| l.verify()) {
        journal.cancel(&req.operation_id, &format!("executable changed: {e}"), None)?;
        return Err(SubmitError::Unregistered(e.to_string()));
    }

    journal.mark_launched(&req.operation_id)?;
    fault(cfg, Boundary::AfterLaunchRecord);
    let liability = req.limits.max_cost_micro;
    let mut psv_evidence: Option<Value> = None;
    let (r, report, reason) = match profile.id {
        id if id == backend::LOCAL_INTERPRETER.id => {
            let res = local.expect("host backend").run_checks(&CheckRequest {
                workspace: &run_dir,
                rel_path: &file,
                filter: filter.as_deref(),
            });
            let seen = post_run(&req, cfg, &target);
            // The suite reference written when the suite was resolved.
            let suite = target.suite.as_ref().map(|s| s.reference.clone());
            let (mut r, report, reason) = local_receipt(
                &req,
                &journal,
                res,
                filter.as_deref(),
                liability,
                seen,
                suite,
                Some(&completion_key),
            )?;
            // A local run is DEVELOPMENT evidence, whatever it verified
            // (v022-psv-protocol.md §6): never protected.
            r.evidence_refs.insert(
                0,
                opaque(crate::psv::EvidenceClass::Development.evidence_ref()),
            );
            (r, report, reason)
        }
        // EVERY launch on the protected profile is this one: launch manifest,
        // custodian nonce, preflight observation (PSV-6; A54). There is no
        // other arm for it, and `run_linux_profile` cannot launch without the
        // manifest.
        id if id == backend::LINUX_MICROVM_PROTECTED.id => {
            let lx = cfg.linux.as_ref().expect("selected only when configured");
            let policy = guest_policy
                .as_ref()
                .expect("built when the profile was selected");
            let q = qualified.as_ref().expect("qualified at dispatch");
            // The protected profile judges only an operator suite with a named
            // test. The pre-reservation check above (M187) refuses anything
            // else first; this is the same rule where the launch is built, so
            // a request that reaches the arm without one is a structured
            // refusal (NotRun, nothing launched), never a crash (an
            // `expect("checked above")` panicked here when that check was
            // removed: C9 round 1b, M187).
            let spec = match (target.suite.as_ref(), filter.as_deref()) {
                (Some(s), Some(t)) => Ok((s.id.clone(), s.version.clone(), t.to_string())),
                _ => Err(PROTECTED_SUITE_ONLY.to_string()),
            };
            let epoch = cfg.expected_epoch.get();
            // B3 (review wf_d725935a-7ed): the launch inputs are NEVER read
            // from the run dir under the caller-supplied --state (the caller
            // owns its parent and could swap the trees before this point).
            // They are re-materialized from the content-addressed store, which
            // re-verifies every blob against its hash, into a Fabric-private
            // 0700 dir under the operator-owned out_root; `prepare` then
            // requires each tree to BE the registered/requested version.
            let inputs = spec.clone().and_then(|(_, version, _)| {
                crate::psv::private_inputs(
                    lx,
                    &cfg.state_dir,
                    &cfg.epoch.scope().tenant_id,
                    &req,
                    &version,
                )
            });
            // M3: the custodian's nonce goes INTO the manifest the observer
            // then observes. Fabric is ISSUED it by the custodian (its own
            // uid on a protected host) and never spends it: the privileged
            // launcher spends it at the root boundary (amendment 50).
            let nonce = match &cfg.observer {
                Some(o) => o.custodian.issue(epoch, &o.clock),
                None => Ok("none".to_string()),
            };
            let prepared = inputs
                .and_then(|dir| nonce.map(|n| (dir, n)))
                .and_then(|(dir, nonce)| spec.map(|s| (dir, nonce, s)))
                .and_then(|(dir, nonce, (suite_id, suite_version, test))| {
                    crate::psv::prepare(
                        &req,
                        &crate::psv::PrepareInputs {
                            qualification: q,
                            profile_manifest: &lx.manifest,
                            host: cfg.protected_host.as_ref(),
                            policy_json: policy.json(),
                            suite_id: &suite_id,
                            suite_version: suite_version.as_str(),
                            entry: &file,
                            test: &test,
                            candidate_dir: &dir.join("candidate"),
                            suite_dir: &dir.join("check"),
                            job_dir: &dir.join("job"),
                            observation_nonce: &nonce,
                            authority_epoch: epoch,
                            scope: cfg.epoch.scope(),
                        },
                    )
                });
            // The observation, verified BEFORE anything is launched; a
            // refusal launches nothing.
            let observed = prepared.and_then(|launch| match &cfg.observer {
                None => Ok((launch, None)),
                Some(o) => crate::observer::observe(
                    o,
                    &launch.manifest,
                    &launch.digest,
                    &launch.job_dir.join("launch-manifest.json"),
                    epoch,
                    &launch.job_dir.with_file_name("observation"),
                )
                .map_err(|e| format!("preflight observation refused: {e}"))
                .and_then(|v| {
                    // The epoch may move while the observer runs: re-read it
                    // now, and launch only if it is still the one observed
                    // (dev review round wf_336353cb-a2b, PSV-6).
                    match cfg.epoch.current() {
                        Ok(now) if now == cfg.expected_epoch => Ok((launch, Some(v))),
                        now => Err(format!(
                            "preflight observation refused: the authority epoch is now {} but \
                             the observation is for {epoch}",
                            now.map(|e| e.get().to_string()).unwrap_or_else(|e| e)
                        )),
                    }
                }),
            });
            match observed {
                Ok((launch, observation)) => {
                    let res = backend::run_linux_profile(lx, &req, &launch, observation.as_ref());
                    launch.scrub();
                    let hv = crate::psv::derive(&launch, &res.out_dir, observation.as_ref());
                    let guest_verdict = hv.guest_verdict.clone();
                    // The receipt FIRST: `psv_receipt` downgrades an
                    // inadmissible launch to guest-unobserved whatever `derive`
                    // saw, so only the FINAL receipt knows the class.
                    let out = psv_receipt(&req, &journal, res, q, hv, liability);
                    // B2: a protected verdict travels with the exact documents
                    // its joins are verified over, including the guest verdict's
                    // own bytes, which the loop joins to the receipt (bundle /2).
                    // ONLY a protected verdict: intake refuses a bundle beside a
                    // receipt that claims no protected evidence, so an observed
                    // launch whose verdict is not protected carries none and is
                    // recorded as the unknown it is (PSV-4, C9 certifying review).
                    //
                    // The class of the FINAL receipt is the one gate. It was
                    // `derive`'s class, taken before the downgrade, so an
                    // unbound, cleanup-incomplete or died-after-verdict launch
                    // shipped a bundle beside a guest-unobserved receipt (PSV-4
                    // and PSV-5, C9 dev review round 1; A55).
                    //
                    // A protected verdict always carries its bytes: `derive`
                    // attaches them only on the fully verified path, where the
                    // observation is what makes it protected. So a second "bytes
                    // present" condition here would be the class check restated.
                    // It was, after the C9 merge, and neither half could then be
                    // killed on its own (M311 survived). Should the bytes ever be
                    // absent, the bundle carries none and intake's guest-verdict
                    // join (M299) refuses it: fail closed, not a silent second
                    // gate.
                    let final_class =
                        crate::psv::EvidenceClass::of_outcome(out.as_ref().ok().map(|(r, _, _)| r));
                    if let (Some(o), crate::psv::EvidenceClass::Protected) =
                        (&observation, final_class)
                    {
                        let v = guest_verdict.as_deref().unwrap_or_default();
                        psv_evidence = Some(crate::psv::evidence_bundle(&launch, o, v));
                    }
                    launch.discard();
                    out?
                }
                Err(why) => {
                    // Nothing launched: the private inputs (and any secret
                    // already written) go now.
                    let _ =
                        crate::workspace::remove_tree(&crate::psv::private_inputs_dir(lx, &req));
                    let why = if why.starts_with("preflight observation refused") {
                        why
                    } else {
                        format!("launch manifest not built: {why}")
                    };
                    journal.fail(&req.operation_id, &why, Billing::Unknown)?;
                    (
                        receipt(
                            &req,
                            id,
                            Obs {
                                status: ReceiptStatus::Failed,
                                exit: None,
                                verification: ReceiptVerification::NotRun,
                                matched: None,
                                evidence: vec![],
                                liability_micro: liability,
                                output: None,
                            },
                        ),
                        None,
                        Some(why),
                    )
                }
            }
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
                        output: None,
                    },
                ),
                None,
                Some(why),
            )
        }
    };
    fault(cfg, Boundary::AfterTerminal);
    record_receipt(&journal, &req, &r, report.as_ref(), reason.as_deref())?;
    let evidence_class = r
        .evidence_refs
        .iter()
        .find_map(|e| e.as_str().strip_prefix(crate::psv::EVIDENCE_CLASS_PREFIX))
        .unwrap_or("none")
        .to_string();
    Ok(Submission {
        receipt: r,
        check_report: report,
        replayed: false,
        backend: Some(profile.id),
        reason,
        ran_under: Some(RanUnder {
            backend: profile.id.to_string(),
            effect_ceiling: ceiling.clone(),
            evidence_class,
        }),
        psv_evidence,
    })
}

type Outcome = (ExecutionReceipt, Option<Value>, Option<String>);

#[allow(clippy::too_many_arguments)]
fn local_receipt(
    req: &ComputeRequest,
    journal: &Journal,
    res: std::io::Result<CheckReport>,
    filter: Option<&str>,
    liability: u64,
    seen: PostRun,
    suite: Option<String>,
    completion_key: Option<&[u8]>,
) -> Result<Outcome, SubmitError> {
    let id = backend::LOCAL_INTERPRETER.id;
    let PostRun { output, problem } = seen;
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
            let mut evidence = vec![opaque(format!(
                "cl22-report:{}",
                axon_loop_contracts::digest_value(&report_json)
                    .map(|r| r.to_string())
                    .unwrap_or_default()
            ))];
            evidence.extend(suite.map(opaque));
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
            // Protected Check Isolation, affirmative completion evidence: a
            // pass needs the interpreter's completion token for every test it
            // rests on — proof that the test BODY returned normally. An
            // `exit(0)`, an `Err` return, or any escape that ends the test
            // early issues no token, so it is Unknown however it got there;
            // the token is keyed per run and handed over on stdin, so the
            // program cannot forge it.
            let mut incomplete = None;
            if let (ReceiptVerification::Passed, Some(key)) = (verification, completion_key) {
                let names: Vec<&str> = match filter {
                    Some(n) => vec![n],
                    None => rep.passed.iter().map(String::as_str).collect(),
                };
                if let Some(n) = names.into_iter().find(|n| {
                    let want = axon_cortex::runner::completion_token(key, n);
                    !rep.completion.iter().any(|(a, t)| a == n && *t == want)
                }) {
                    verification = ReceiptVerification::Unknown;
                    incomplete = Some(format!(
                        "check `{n}` passed without completion evidence: its body did not \
                         provably return (an exit, an Err return or an escape ended it early)"
                    ));
                }
            }
            if problem.is_some()
                && matches!(
                    verification,
                    ReceiptVerification::Passed | ReceiptVerification::Failed
                )
            {
                // The judged bytes are not the bytes the run left behind (or
                // the check's own inputs moved): no verdict about either.
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
                        output,
                    },
                ),
                Some(report_json),
                problem.or(incomplete),
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
                        // The suite that was running is still recorded: an
                        // honest "unknown" names what it was unknown about.
                        evidence: suite.clone().map(opaque).into_iter().collect(),
                        liability_micro: liability,
                        output: output.clone(),
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
                        // The suite that was running is still recorded: an
                        // honest "unknown" names what it was unknown about.
                        evidence: suite.clone().map(opaque).into_iter().collect(),
                        liability_micro: liability,
                        output,
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
    q: &backend::LinuxQualification,
    liability: u64,
) -> Result<Outcome, SubmitError> {
    let id = backend::LINUX_MICROVM_PROTECTED.id;
    let mut evidence: Vec<OpaqueRef> = res.evidence.iter().map(|e| opaque(e.clone())).collect();
    // What the run was qualified BY travels with its receipt: the signed
    // record, its issuer, and the boundary caveat (D2) — so a reader of the
    // receipt cannot mistake a caveated qualification for an unqualified one.
    evidence.push(opaque(format!(
        "qualification-evidence-sha256:{}",
        q.evidence_sha256
    )));
    evidence.push(opaque(format!("qualification-issuer:{}", q.issuer)));
    evidence.push(opaque(format!("qualification-host:{}", q.host)));
    evidence.push(opaque(format!("qualification-caveat:{}", q.caveat)));
    for w in &q.waived {
        evidence.push(opaque(format!("qualification-waived:{w}")));
    }
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
                output: None,
            },
        ),
        None,
        Some(res.reason),
    ))
}

/// The receipt of a protected-profile CHECK: the launcher's outcome, then
/// Fabric's own verdict (`psv::derive`), never the guest's claim.
fn psv_receipt(
    req: &ComputeRequest,
    journal: &Journal,
    res: backend::LinuxRun,
    q: &backend::LinuxQualification,
    hv: crate::psv::HostVerdict,
    liability: u64,
) -> Result<Outcome, SubmitError> {
    let (mut r, _, reason) = linux_receipt(req, journal, res.clone(), q, liability)?;
    // A launch that did not complete admissibly has no verdict at all.
    let launched_ok = matches!(res.outcome, backend::LinuxOutcome::Ok { .. });
    let (verification, reason) = if launched_ok {
        (hv.verification, hv.reason.or(reason))
    } else {
        (r.verification, reason)
    };
    r.verification = verification;
    // The exact named check had a verdict: one matched check (the receipt
    // contract requires it for a pass; review wf_d725935a-7ed).
    if matches!(
        verification,
        ReceiptVerification::Passed | ReceiptVerification::Failed
    ) {
        r.matched_checks = Some(1);
    }
    r.evidence_refs.extend(hv.evidence.into_iter().map(opaque));
    // Operator decision B: a protected verdict needs the pinned PRIVILEGED
    // launcher in its chain. A launch Fabric ran itself (the development
    // route) is never protected, whatever else it carries (A, amendment 45).
    let privileged = crate::backend::attests_protected(res.route);
    if !launched_ok || !privileged {
        // Whatever derive saw, an inadmissible launch is never protected.
        r.evidence_refs
            .retain(|e| !e.as_str().starts_with(crate::psv::EVIDENCE_CLASS_PREFIX));
        r.evidence_refs.insert(
            0,
            opaque(crate::psv::EvidenceClass::GuestUnobserved.evidence_ref()),
        );
    }
    let report = hv.report.map(|mut j| {
        j["verification"] = serde_json::json!(verification);
        j
    });
    Ok((r, report, reason))
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
        config: json!({"backend": null, "grant": {
            "grant_ref": req.grant_ref, "registry_sha256": cfg.grants.sha256(),
        }}),
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

fn fault(cfg: &SubmitConfig, b: Boundary) {
    if let Some(h) = cfg.fault_hook {
        h(b);
    }
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
                ran_under: v.launched.then(|| ran_under_of(&v.intent)).flatten(),
                // Not journalled: a replay carries no bundle (and is never signed).
                psv_evidence: None,
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
            output: None,
        },
    );
    Submission {
        receipt: r,
        check_report: None,
        replayed: true,
        backend: None,
        reason: Some(why.to_string()),
        ran_under: None,
        psv_evidence: None,
    }
}

/// Build a `Scope` from CLI strings.
pub fn scope(tenant: &str, family: &str) -> Result<Scope, String> {
    Ok(Scope {
        tenant_id: TenantId::new(tenant).map_err(|e| e.to_string())?,
        task_family: TaskFamily::new(family).map_err(|e| e.to_string())?,
    })
}

#[cfg(test)]
mod tests {
    /// Amendment 110: the grant's cost budget, at the boundary and past `i64`. Equal passes, one over refuses,
    /// and a limit that cannot be an `i64` refuses (it exceeds every budget).
    #[test]
    fn a_cost_limit_over_the_grants_budget_is_refused_including_one_that_overflows_i64() {
        assert!(!super::exceeds_budget(0, 0), "zero against a zero budget");
        assert!(!super::exceeds_budget(1000, 1000), "equal to the budget");
        assert!(
            super::exceeds_budget(1001, 1000),
            "ATTACK: one micro over the budget was admitted"
        );
        assert!(!super::exceeds_budget(999, 1000));
        assert!(
            super::exceeds_budget(i64::MAX as u64 + 1, i64::MAX),
            "ATTACK: a cost limit that does not fit an i64 was admitted"
        );
        assert!(
            super::exceeds_budget(u64::MAX, i64::MAX),
            "ATTACK: u64::MAX against the largest budget"
        );
        assert!(
            !super::exceeds_budget(i64::MAX as u64, i64::MAX),
            "the largest representable, equal"
        );
    }

    use super::*;

    /// Amendment 95 (eqgate4): a run dir is created NEW. A directory already at
    /// the name (a leftover, or one planted) is refused, never reused, and what
    /// it holds is untouched; a name whose parent is missing is refused too
    /// (`create_dir_all` would make the parents and accept both).
    #[test]
    fn a_run_dir_is_never_made_over_an_existing_directory() {
        let d = std::env::temp_dir().join(format!("axon-rundir-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("runs")).unwrap();
        // CONTROL: a fresh name is created, and removed when the guard drops.
        let made = RunDir::create_new(&d.join("runs"), "fresh").expect("control: a fresh name");
        assert!(d.join("runs/fresh").is_dir());
        drop(made);
        assert!(!d.join("runs/fresh").exists(), "control: removed on drop");
        // ATTACK 1: the name already holds a directory with a stale file.
        std::fs::create_dir(d.join("runs/leftover")).unwrap();
        std::fs::write(d.join("runs/leftover/stale"), "x").unwrap();
        match RunDir::create_new(&d.join("runs"), "leftover") {
            Ok(r) => {
                std::mem::forget(r);
                panic!("ATTACK: a run dir was made over a directory that already existed");
            }
            Err(e) => assert!(format!("{e:?}").contains("File exists"), "{e:?}"),
        }
        assert!(d.join("runs/leftover/stale").is_file());
        // ATTACK 2: a missing parent is not made on the way.
        match RunDir::create_new(&d.join("runs/absent"), "child") {
            Ok(r) => {
                std::mem::forget(r);
                panic!("ATTACK: a run dir was made under parents that did not exist");
            }
            Err(e) => assert!(format!("{e:?}").contains("No such file"), "{e:?}"),
        }
        assert!(!d.join("runs/absent").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
