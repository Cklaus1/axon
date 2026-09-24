//! The frozen experiment's task MANIFEST (AB9/AB10: evaluation shopping).
//!
//! A plan names its assigned tasks only by `task_manifest_ref`. Without the
//! list, an evaluation could cover any subset of tasks it liked (cherry-pick
//! the ones the candidate passed) or be re-run until it won. So the list is
//! registered here by a trusted admitter as a ledger event, and freeze
//! requires it:
//!
//! * `axon.loop.task-manifest/1` `{scope, tasks, issuer_ref}`; `tasks` is a
//!   non-empty, sorted, duplicate-free list of [`TaskId`]s;
//! * its ref is `cl22:` over the task ARRAY (one list, one ref, whoever
//!   registers it) and must equal the plan's `task_manifest_ref`;
//! * bytes live under `task-manifests/<tenant>/<family>/<hex>.json`
//!   (scope-keyed, NS3), re-checked against name and scope on every read.
//!
//! [`crate::evl::evaluate`] then requires the evaluation to assign EXACTLY
//! these tasks × both arms × the plan's `repetitions`, once per experiment.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{digest_value, OpaqueRef, Ref, Refusal, Scope, TaskId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

crate::record_tag!(TaskManifestSchema, "axon.loop.task-manifest/1");

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TaskManifest {
    pub schema: TaskManifestSchema,
    pub scope: Scope,
    pub tasks: Vec<TaskId>,
    pub issuer_ref: OpaqueRef,
}

impl TaskManifest {
    pub fn parse(text: &str) -> Result<TaskManifest> {
        let m: TaskManifest = strict_record(text)?;
        m.validate()?;
        Ok(m)
    }

    fn validate(&self) -> Result<()> {
        if self.tasks.is_empty() || self.tasks.len() > 100_000 {
            return Err(LoopError::Malformed(Refusal::Shape(
                "tasks: 1..=100000 items".into(),
            )));
        }
        if !self.tasks.windows(2).all(|w| w[0] < w[1]) {
            return Err(LoopError::Malformed(Refusal::Shape(
                "tasks must be sorted and duplicate-free (one manifest, one spelling)".into(),
            )));
        }
        Ok(())
    }

    /// `cl22:` over the task array — the plan's `task_manifest_ref`.
    pub fn manifest_ref(&self) -> Result<Ref> {
        let names: Vec<&str> = self.tasks.iter().map(TaskId::as_str).collect();
        Ok(digest_value(&serde_json::json!(names))?)
    }

    pub fn task_set(&self) -> BTreeSet<TaskId> {
        self.tasks.iter().cloned().collect()
    }
}

/// `task-manifests/<tenant>/<family>/<hex>.json` (NS3: scope-keyed, so the same list
/// registered for another scope is a second file, never an overwrite).
fn path(store: &Store, scope: &Scope, r: &Ref) -> Result<std::path::PathBuf> {
    store.scoped_cas_path("task-manifests", scope, r)
}

/// Read the record for `(scope, r)`. A store written before NS3 kept it at
/// the flat `task-manifests/<hex>.json`; that file is accepted only when its bytes
/// name THIS scope (checked by the caller), so it is read, never trusted.
fn read(tx: &Tx, scope: &Scope, r: &Ref) -> Result<Option<String>> {
    match tx.store.read_text(&path(tx.store, scope, r)?)? {
        Some(t) => Ok(Some(t)),
        None => tx.store.read_text(&tx.store.cas_path("task-manifests", r)?),
    }
}

/// Register a task manifest. Idempotent. Returns its ref.
pub fn put(store: &Store, m: &TaskManifest) -> Result<Ref> {
    m.validate()?;
    let mut tx = Tx::begin(store)?;
    if !store.config()?.admitters().contains(&m.issuer_ref) {
        return Err(refused(format!(
            "issuer {} is not a trusted admitter",
            m.issuer_ref
        )));
    }
    let r = m.manifest_ref()?;
    let registered = tx.task_manifest_event(&m.scope, &r);
    if registered && resolve(&tx, &m.scope, &r).is_ok() {
        return Ok(r);
    }
    // Scope-keyed path: replaces only this scope's file for this list (NS3);
    // with the event present it is a repair, as in `candidates::put`.
    store.write_atomic(
        &path(store, &m.scope, &r)?,
        &axon_loop_contracts::canonical_json(m)?,
    )?;
    if registered {
        return Ok(r);
    }
    tx.append(Event::TaskManifest {
        scope: m.scope.clone(),
        manifest_ref: r.clone(),
    })?;
    Ok(r)
}

/// The registered manifest for `(scope, ref)`, or a refusal.
pub fn resolve(tx: &Tx, scope: &Scope, r: &Ref) -> Result<TaskManifest> {
    if !tx.task_manifest_event(scope, r) {
        return Err(refused(format!(
            "task_manifest_ref {r} is not a registered task manifest for {}/{}: \
             register it with `tasks put` before freezing",
            scope.tenant_id, scope.task_family
        )));
    }
    let text = read(tx, scope, r)?
        .ok_or_else(|| LoopError::Io(format!("store corrupt: task manifest {r} missing")))?;
    let m: TaskManifest = strict_record(&text)
        .map_err(|e| LoopError::Io(format!("store corrupt: task manifest {r}: {e}")))?;
    if &m.manifest_ref()? != r || &m.scope != scope {
        return Err(LoopError::Io(format!(
            "store corrupt: task manifest {r} does not match its name"
        )));
    }
    Ok(m)
}
