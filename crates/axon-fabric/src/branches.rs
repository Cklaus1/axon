//! B271 — logical A/B branches with fenced workspace publication.
//!
//! # Logical branches (G08-r22-logical-branches)
//!
//! An experiment is opened ONCE (write-once record) over a frozen durable
//! base — a WorkspaceVersion already published in the tenant's store — and
//! one declared resource regime shared by every branch. Each branch
//! (incumbent, challenger-N) gets an independent run identity: its own
//! `TrialId` (so its own per-trial caches) and its own budget, carved from
//! the scope's aggregate budget AND within the regime by
//! `Journal::reserve_within` (`ResourceVector::carve_within`). This is
//! logical branching, not a RAM fork: nothing is shared between branches
//! but the immutable base.
//!
//! # Workspace publication — CAS (G11-r22-workspace-cas)
//!
//! A branch's head is a chain of write-once files `head-<seq>.json`
//! (`seq` 0 is the base). Publishing `seq+1` requires, all checked before
//! the head file is created:
//!
//! * the expected base: the caller's `expected_seq`/`expected_version` must
//!   be the current head — journalled as `Intent::expected_version`;
//! * the current fencing epoch (the loop store's, equal to the caller's);
//! * the branch's declared writer;
//! * the EXACT verified output: a Fabric operation recorded in this journal,
//!   on this branch's run, `completed` + `passed`, whose receipt's input and
//!   output are both the new version (a hand-written receipt is not in the
//!   journal and cannot be used);
//! * an independent approval: an approver the experiment declared, never the
//!   writer.
//!
//! The head file is created by a no-clobber rename: of two concurrent
//! publishers from the same head exactly one wins, and the other gets a
//! conflict — it must rebase onto the new head and be re-verified. A head is
//! never overwritten.
//!
//! # Cancellation (G08-r22-branch-cancellation)
//!
//! Cancelling a branch writes a write-once marker, then reconciles every op
//! of the branch: a reserved-but-unlaunched op is released, a launched one is
//! cancelled with its liability KEPT (`Billing::Unknown`). Nothing is removed:
//! the journal is append-only, so the branch's events, outcomes (including an
//! unfavorable verdict) and usage remain. Other branches are untouched. A
//! cancelled branch accepts no new operation and no publication.

use std::path::{Path, PathBuf};

use axon_loop_contracts::{
    Acf1Ref, ArmId, AuthorityEpoch, ExecutionReceipt, OpaqueRef, OperationId, ReceiptStatus,
    ReceiptVerification, Ref, Scope, TaskId, TrialId,
};
use serde::{Deserialize, Serialize};

use crate::journal::{Begin, Billing, Intent, Journal, JournalError, OpState, ResourceVector};
use crate::submit::EpochSource;
use crate::workspace::WorkspaceStore;

pub const EXPERIMENT_SCHEMA: &str = "axon-fabric-experiment/1";
pub const HEAD_SCHEMA: &str = "axon-fabric-branch-head/1";

/// Why a branch operation refused. Every refusal leaves the heads, the
/// branch markers and the journal's launch records as they were.
#[derive(Debug)]
pub enum BranchError {
    /// A malformed or inconsistent request.
    Invalid(String),
    /// The experiment already exists with different content.
    Exists(String),
    /// Not an experiment / branch of this tenant.
    Unknown(String),
    /// The expected head is not the current head: rebase and re-verify.
    Conflict(String),
    StaleEpoch(String),
    Unauthorized(String),
    /// The offered verification does not bind the new version.
    Unverified(String),
    Cancelled(String),
    Journal(JournalError),
    Io(String),
}

impl std::fmt::Display for BranchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BranchError::Invalid(s) => write!(f, "invalid: {s}"),
            BranchError::Exists(s) => write!(f, "exists: {s}"),
            BranchError::Unknown(s) => write!(f, "unknown: {s}"),
            BranchError::Conflict(s) => write!(f, "conflict: {s}"),
            BranchError::StaleEpoch(s) => write!(f, "stale epoch: {s}"),
            BranchError::Unauthorized(s) => write!(f, "unauthorized: {s}"),
            BranchError::Unverified(s) => write!(f, "unverified: {s}"),
            BranchError::Cancelled(s) => write!(f, "cancelled: {s}"),
            BranchError::Journal(e) => write!(f, "journal: {e}"),
            BranchError::Io(s) => write!(f, "io: {s}"),
        }
    }
}

impl BranchError {
    pub fn kind(&self) -> &'static str {
        match self {
            BranchError::Invalid(_) => "invalid",
            BranchError::Exists(_) => "exists",
            BranchError::Unknown(_) => "unknown",
            BranchError::Conflict(_) => "conflict",
            BranchError::StaleEpoch(_) => "stale_epoch",
            BranchError::Unauthorized(_) => "unauthorized",
            BranchError::Unverified(_) => "unverified",
            BranchError::Cancelled(_) => "cancelled",
            BranchError::Journal(_) => "journal",
            BranchError::Io(_) => "io",
        }
    }
}

impl From<JournalError> for BranchError {
    fn from(e: JournalError) -> Self {
        BranchError::Journal(e)
    }
}

impl From<std::io::Error> for BranchError {
    fn from(e: std::io::Error) -> Self {
        BranchError::Io(e.to_string())
    }
}

/// One logical branch as declared at open.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub arm_id: ArmId,
    /// The branch's independent run identity (its ops carry this TrialId).
    pub run_id: TrialId,
    /// The only principal that may publish this branch's head.
    pub writer: OpaqueRef,
}

/// The write-once experiment record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Experiment {
    pub schema: String,
    pub experiment_id: TaskId,
    pub scope: Scope,
    /// The frozen durable base every branch starts from.
    pub base: Acf1Ref,
    /// One declared regime for every branch (a fair comparison).
    pub regime: ResourceVector,
    pub branches: Vec<Branch>,
    /// Who may approve a publication (never a branch's own writer).
    pub approvers: Vec<OpaqueRef>,
}

impl Experiment {
    pub fn branch(&self, arm: &ArmId) -> Result<&Branch, BranchError> {
        self.branches
            .iter()
            .find(|b| &b.arm_id == arm)
            .ok_or_else(|| BranchError::Unknown(format!("no branch {arm}")))
    }
}

/// A branch head: the version, its sequence number, and what published it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub schema: String,
    pub seq: u64,
    pub version: Acf1Ref,
    /// `None` for seq 0 (the base).
    pub publication_op: Option<OperationId>,
}

/// Where branch state lives (per tenant, like the workspace store).
#[derive(Debug, Clone)]
pub struct Branches {
    root: PathBuf,
    state_dir: PathBuf,
    scope: Scope,
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

/// Write `bytes` to `dest` only if it does not exist (fsynced temp +
/// no-clobber rename). `Ok(false)` if something is already there.
fn create_once(dest: &Path, bytes: &[u8]) -> Result<bool, BranchError> {
    use std::io::Write;
    let dir = dest.parent().expect("has a parent");
    std::fs::create_dir_all(dir)?;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let tmp = dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let r = std::fs::hard_link(&tmp, dest);
    let _ = std::fs::remove_file(&tmp);
    match r {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(e) => Err(e.into()),
    }
}

impl Branches {
    pub fn open(state_dir: &Path, scope: &Scope) -> Branches {
        let key = &sha256_hex(scope.tenant_id.as_str().as_bytes())[..32];
        Branches {
            root: state_dir.join("tenants").join(key).join("branches"),
            state_dir: state_dir.to_path_buf(),
            scope: scope.clone(),
        }
    }

    fn exp_dir(&self, exp: &TaskId) -> PathBuf {
        self.root.join(exp.as_str())
    }
    fn arm_dir(&self, exp: &TaskId, arm: &ArmId) -> PathBuf {
        self.exp_dir(exp).join(arm.as_str())
    }
    fn run_index(&self, run: &TrialId) -> PathBuf {
        self.root
            .join("runs")
            .join(&sha256_hex(run.as_str().as_bytes())[..32])
    }

    /// Open an experiment. Refused unless `base` is published in the
    /// tenant's store, there are ≥ 2 distinct arms, every writer is distinct
    /// from every approver, and no run id is already some branch's.
    /// Reopening with IDENTICAL content is idempotent; anything else exists.
    pub fn open_experiment(
        &self,
        experiment_id: &TaskId,
        base: &Acf1Ref,
        regime: ResourceVector,
        arms: &[(ArmId, OpaqueRef)],
        approvers: &[OpaqueRef],
    ) -> Result<Experiment, BranchError> {
        let store = WorkspaceStore::open(&self.state_dir, &self.scope.tenant_id)
            .map_err(|e| BranchError::Io(e.to_string()))?;
        if !store.contains(base) {
            return Err(BranchError::Invalid(format!(
                "base {base} is not a published WorkspaceVersion of this tenant"
            )));
        }
        if arms.len() < 2 {
            return Err(BranchError::Invalid(
                "an A/B experiment has ≥ 2 arms".into(),
            ));
        }
        let mut seen = std::collections::BTreeSet::new();
        for (a, w) in arms {
            if !seen.insert(a) {
                return Err(BranchError::Invalid(format!("arm {a} declared twice")));
            }
            if approvers.contains(w) {
                return Err(BranchError::Invalid(format!(
                    "writer {w} is also an approver: approval must be independent"
                )));
            }
        }
        if approvers.is_empty() {
            return Err(BranchError::Invalid(
                "no approvers: nothing could publish".into(),
            ));
        }
        let branches = arms
            .iter()
            .map(|(a, w)| {
                Ok(Branch {
                    arm_id: a.clone(),
                    run_id: TrialId::new(format!("br.{experiment_id}.{a}"))
                        .map_err(|e| BranchError::Invalid(e.to_string()))?,
                    writer: w.clone(),
                })
            })
            .collect::<Result<Vec<_>, BranchError>>()?;
        let exp = Experiment {
            schema: EXPERIMENT_SCHEMA.into(),
            experiment_id: experiment_id.clone(),
            scope: self.scope.clone(),
            base: base.clone(),
            regime,
            branches,
            approvers: approvers.to_vec(),
        };
        let bytes = serde_json::to_vec_pretty(&exp).expect("serializable");
        let file = self.exp_dir(experiment_id).join("experiment.json");
        if !create_once(&file, &bytes)? {
            if std::fs::read(&file)? == bytes {
                return Ok(exp);
            }
            return Err(BranchError::Exists(format!(
                "experiment {experiment_id} exists with a different base/regime/arms"
            )));
        }
        for b in &exp.branches {
            let idx = serde_json::to_vec(&(experiment_id, &b.arm_id)).expect("serializable");
            if !create_once(&self.run_index(&b.run_id), &idx)?
                && std::fs::read(self.run_index(&b.run_id))? != idx
            {
                return Err(BranchError::Exists(format!(
                    "run id {} already belongs to another branch",
                    b.run_id
                )));
            }
            let head0 = Head {
                schema: HEAD_SCHEMA.into(),
                seq: 0,
                version: base.clone(),
                publication_op: None,
            };
            create_once(
                &self.arm_dir(experiment_id, &b.arm_id).join("head-0.json"),
                &serde_json::to_vec(&head0).expect("serializable"),
            )?;
        }
        Ok(exp)
    }

    pub fn experiment(&self, exp: &TaskId) -> Result<Experiment, BranchError> {
        let b = std::fs::read(self.exp_dir(exp).join("experiment.json"))
            .map_err(|_| BranchError::Unknown(format!("no experiment {exp}")))?;
        let e: Experiment =
            serde_json::from_slice(&b).map_err(|e| BranchError::Io(format!("experiment: {e}")))?;
        if e.scope != self.scope {
            return Err(BranchError::Unknown(format!(
                "experiment {exp} is another scope's"
            )));
        }
        Ok(e)
    }

    /// The branch whose run id is `run`, if any (with its experiment).
    pub fn branch_of_run(
        &self,
        run: &TrialId,
    ) -> Result<Option<(Experiment, Branch)>, BranchError> {
        let Ok(b) = std::fs::read(self.run_index(run)) else {
            return Ok(None);
        };
        let (exp, arm): (TaskId, ArmId) =
            serde_json::from_slice(&b).map_err(|e| BranchError::Io(format!("run index: {e}")))?;
        let e = self.experiment(&exp)?;
        let br = e.branch(&arm)?.clone();
        Ok(Some((e, br)))
    }

    /// The current head (highest `seq`).
    pub fn head(&self, exp: &TaskId, arm: &ArmId) -> Result<Head, BranchError> {
        let dir = self.arm_dir(exp, arm);
        let mut best: Option<u64> = None;
        for e in std::fs::read_dir(&dir)
            .map_err(|_| BranchError::Unknown(format!("no branch {exp}/{arm}")))?
            .flatten()
        {
            let n = e.file_name();
            let Some(seq) = n
                .to_str()
                .and_then(|s| s.strip_prefix("head-"))
                .and_then(|s| s.strip_suffix(".json"))
                .and_then(|s| s.parse::<u64>().ok())
            else {
                continue;
            };
            best = Some(best.map_or(seq, |b| b.max(seq)));
        }
        let seq =
            best.ok_or_else(|| BranchError::Unknown(format!("branch {exp}/{arm} has no head")))?;
        let h: Head = serde_json::from_slice(&std::fs::read(dir.join(format!("head-{seq}.json")))?)
            .map_err(|e| BranchError::Io(format!("head: {e}")))?;
        Ok(h)
    }

    pub fn is_cancelled(&self, exp: &TaskId, arm: &ArmId) -> bool {
        self.arm_dir(exp, arm).join("cancelled.json").exists()
    }

    /// Publish `new_version` as the branch's next head. See the module docs
    /// for the five preconditions; every refusal leaves the head unchanged.
    pub fn publish(
        &self,
        journal: &Journal,
        epoch: &EpochSource,
        exp_id: &TaskId,
        arm: &ArmId,
        req: &Publication,
    ) -> Result<Head, BranchError> {
        let exp = self.experiment(exp_id)?;
        let br = exp.branch(arm)?.clone();
        if self.is_cancelled(exp_id, arm) {
            return Err(BranchError::Cancelled(format!(
                "branch {exp_id}/{arm} was cancelled"
            )));
        }
        // Writer and approval.
        if req.writer != br.writer {
            return Err(BranchError::Unauthorized(format!(
                "{} is not the writer of {exp_id}/{arm}",
                req.writer
            )));
        }
        if req.approver == req.writer || !exp.approvers.contains(&req.approver) {
            return Err(BranchError::Unauthorized(format!(
                "approver {} is not an independent declared approver",
                req.approver
            )));
        }
        // Fencing epoch.
        let now = epoch.current().map_err(BranchError::StaleEpoch)?;
        if now != req.epoch {
            return Err(BranchError::StaleEpoch(format!(
                "publication authorized at {}, store says {}",
                req.epoch.get(),
                now.get()
            )));
        }
        // The exact verified output.
        let store = WorkspaceStore::open(&self.state_dir, &self.scope.tenant_id)
            .map_err(|e| BranchError::Io(e.to_string()))?;
        if !store.contains(&req.new_version) {
            return Err(BranchError::Unverified(format!(
                "{} is not a published WorkspaceVersion of this tenant",
                req.new_version
            )));
        }
        let v = journal.view(&req.verified_by).ok_or_else(|| {
            BranchError::Unverified(format!(
                "operation {} is not in this journal: a receipt must be the Fabric's own",
                req.verified_by
            ))
        })?;
        if v.intent.scope != self.scope || v.intent.trial_id != br.run_id {
            return Err(BranchError::Unverified(format!(
                "operation {} did not run on branch {exp_id}/{arm}",
                req.verified_by
            )));
        }
        let rc: ExecutionReceipt = v
            .outcome
            .as_ref()
            .and_then(|o| serde_json::from_value(o["receipt"].clone()).ok())
            .ok_or_else(|| BranchError::Unverified("the operation recorded no receipt".into()))?;
        if v.state != OpState::Completed
            || rc.status != ReceiptStatus::Completed
            || rc.verification != ReceiptVerification::Passed
        {
            return Err(BranchError::Unverified(format!(
                "operation {} did not verify ({:?}/{:?})",
                req.verified_by, rc.status, rc.verification
            )));
        }
        if rc.input_workspace_ref != req.new_version
            || rc.output_workspace_ref.as_ref() != Some(&req.new_version)
        {
            return Err(BranchError::Unverified(format!(
                "operation {} verified {} → {:?}, not {}",
                req.verified_by, rc.input_workspace_ref, rc.output_workspace_ref, req.new_version
            )));
        }
        // Expected base.
        let head = self.head(exp_id, arm)?;
        if head.seq != req.expected_seq || head.version != req.expected_version {
            return Err(BranchError::Conflict(format!(
                "branch {exp_id}/{arm} is at #{} {}, not the expected #{} {}: rebase and re-verify",
                head.seq, head.version, req.expected_seq, req.expected_version
            )));
        }
        // Journal the publication with its CAS precondition, then create the
        // next head write-once.
        let op = OperationId::new(format!(
            "pub.{}.{}.{}",
            &sha256_hex(exp_id.as_str().as_bytes())[..12],
            arm,
            head.seq + 1
        ))
        .map_err(|e| BranchError::Invalid(e.to_string()))?;
        let intent = Intent {
            op: op.clone(),
            task_id: exp_id.clone(),
            trial_id: br.run_id.clone(),
            attempt_id: axon_loop_contracts::AttemptId::new(format!("publish-{}", head.seq + 1))
                .expect("valid"),
            input_digest: Ref::new(format!(
                "sha256:{}",
                sha256_hex(
                    format!(
                        "{}|{}|{}",
                        req.expected_version, req.new_version, req.verified_by
                    )
                    .as_bytes()
                )
            ))
            .expect("sha256 ref"),
            config: serde_json::json!({
                "workspace_publication": {
                    "experiment": exp_id, "arm": arm,
                    "expected_version": req.expected_version,
                    "new_version": req.new_version,
                    "verified_by": req.verified_by,
                    "writer": req.writer, "approver": req.approver,
                }
            }),
            authority_ref: format!("{}|{}", req.writer, req.approver),
            authority_epoch: req.epoch,
            scope: self.scope.clone(),
            reservation: ResourceVector::default(),
            expected_version: req.expected_seq,
        };
        match journal.begin(intent) {
            Ok(Begin::Recorded) => {}
            Err(JournalError::Conflict { .. }) | Ok(Begin::AlreadyRecorded(_)) => {
                return Err(BranchError::Conflict(format!(
                    "publication #{} of {exp_id}/{arm} was already attempted",
                    head.seq + 1
                )))
            }
            Err(e) => return Err(e.into()),
        }
        journal.reserve(&op)?;
        journal.mark_launched(&op)?;
        let next = Head {
            schema: HEAD_SCHEMA.into(),
            seq: head.seq + 1,
            version: req.new_version.clone(),
            publication_op: Some(op.clone()),
        };
        let file = self
            .arm_dir(exp_id, arm)
            .join(format!("head-{}.json", next.seq));
        if !create_once(&file, &serde_json::to_vec(&next).expect("serializable"))? {
            journal.fail(
                &op,
                "lost the head CAS to a concurrent publication",
                Billing::Known(ResourceVector::default()),
            )?;
            return Err(BranchError::Conflict(format!(
                "head #{} of {exp_id}/{arm} was published concurrently: rebase and re-verify",
                next.seq
            )));
        }
        journal.complete(&op, Billing::Known(ResourceVector::default()))?;
        journal.record_outcome(&op, serde_json::json!({ "head": next }))?;
        Ok(next)
    }

    /// Cancel a branch. See the module docs. Returns the ops it reconciled.
    pub fn cancel(
        &self,
        journal: &Journal,
        exp_id: &TaskId,
        arm: &ArmId,
        reason: &str,
    ) -> Result<Vec<OperationId>, BranchError> {
        let exp = self.experiment(exp_id)?;
        let br = exp.branch(arm)?.clone();
        create_once(
            &self.arm_dir(exp_id, arm).join("cancelled.json"),
            serde_json::json!({"reason": reason}).to_string().as_bytes(),
        )?;
        let mut done = Vec::new();
        for v in journal.views_where(|i| i.scope == self.scope && i.trial_id == br.run_id) {
            let op = v.intent.op.clone();
            match v.state {
                OpState::Intended | OpState::Reserved => {
                    journal.cancel(&op, &format!("branch cancelled: {reason}"), None)?
                }
                // May have had its effect: liability kept, never refunded.
                OpState::Launched => journal.cancel(
                    &op,
                    &format!("branch cancelled: {reason}"),
                    Some(Billing::Unknown),
                )?,
                _ => continue,
            }
            done.push(op);
        }
        Ok(done)
    }
}

/// A publication request (every field is the caller's CLAIM; `publish`
/// checks each against durable state).
#[derive(Debug, Clone)]
pub struct Publication {
    pub expected_seq: u64,
    pub expected_version: Acf1Ref,
    pub new_version: Acf1Ref,
    /// The Fabric operation (in this journal) that verified `new_version`.
    pub verified_by: OperationId,
    pub writer: OpaqueRef,
    pub approver: OpaqueRef,
    pub epoch: AuthorityEpoch,
}

#[cfg(test)]
mod create_once_tests {
    use super::*;

    /// C9 round 4c, EQGATE (amendment 81; M1939): `create_once` is the no-clobber
    /// write every branch record goes through (`hard_link` refuses an existing
    /// name). It answers `Ok(false)` for a name already taken and must leave the
    /// existing bytes alone: a plain rename would replace a branch head another
    /// writer had just advanced.
    #[test]
    fn create_once_never_replaces_what_is_there() {
        let t = tempfile::tempdir().unwrap();
        let dest = t.path().join("d").join("head.json");
        assert!(create_once(&dest, b"first").unwrap(), "control: a new name is created");
        let again = create_once(&dest, b"second").unwrap();
        assert!(
            !again && std::fs::read(&dest).unwrap() == b"first",
            "ATTACK: create_once replaced an existing branch record (returned {again}, now {:?})",
            std::fs::read(&dest)
        );
    }
}
