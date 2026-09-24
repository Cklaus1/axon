//! B278/B279 — the fenced, durable, per-scope active-policy pointer.
//!
//! State: `scopes/<tenant>/<family>/pointer.json` ([`PointerRecord`]). It moves
//! ONLY through [`transition`], which, under the scope's flock:
//!
//! 1. recovers a crash between journal append and publish (roll forward);
//! 2. replays an already-applied `transition_id` idempotently (same bytes ⇒
//!    same reply; same id with different bytes ⇒ conflict);
//! 3. refuses an issuer outside `config.trusted_admitters`;
//! 4. compare-and-swaps on `(expected_policy_ref, expected_epoch)` —
//!    `next_epoch = expected + 1` is already enforced by the contract parse;
//! 5. for activate/rollback, requires an ACCEPT admission record in the
//!    admission store whose target, scope, controls and mechanism-test label
//!    match, whose admitter is STILL trusted, whose plan enabled deployment,
//!    and whose target envelope is stored and not revoked; rollback further
//!    requires the target to be a previously-active predecessor;
//! 6. appends the transition + resulting state to `transitions.jsonl`
//!    (fsync), then publishes `pointer.json` atomically.
//!
//! A refused transition writes nothing. A paused scope has no active policy
//! and is spelled by the `cl22:000…0` sentinel in `expected_policy_ref`.

use crate::admission::{AdmissionRecord, Decision};
use crate::error::{refused, LoopError, Result};
use crate::store::{append_jsonl, read_json, read_jsonl, write_json_atomic, DirLock, Store};
use axon_loop_contracts::{
    AuthorityEpoch, OpaqueRef, PinAck, PolicyEnvelope, PolicyPin, PolicyTransition, Ref, Scope,
    TransitionId, TransitionKind,
};
use serde::{Deserialize, Serialize};

crate::record_tag!(PointerSchema, "axon.loop.pointer/1");
crate::record_tag!(LogSchema, "axon.loop.transition-log/1");
crate::record_tag!(RevocationSchema, "axon.loop.revocations/1");

/// A previously-active policy: the only legal rollback targets.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub policy_ref: Ref,
    pub admission_ref: Ref,
    pub activated_epoch: AuthorityEpoch,
    pub retired_epoch: AuthorityEpoch,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointerRecord {
    pub schema: PointerSchema,
    pub scope: Scope,
    pub epoch: AuthorityEpoch,
    /// `null` ⇒ paused (or never activated).
    #[serde(deserialize_with = "crate::nullable")]
    pub active_policy_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub active_admission_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub active_since_epoch: Option<AuthorityEpoch>,
    pub history: Vec<HistoryEntry>,
    #[serde(deserialize_with = "crate::nullable")]
    pub last_transition_id: Option<TransitionId>,
}

impl PointerRecord {
    pub fn initial(scope: &Scope) -> PointerRecord {
        PointerRecord {
            schema: PointerSchema,
            scope: scope.clone(),
            epoch: AuthorityEpoch::new(0).expect("0"),
            active_policy_ref: None,
            active_admission_ref: None,
            active_since_epoch: None,
            history: Vec::new(),
            last_transition_id: None,
        }
    }
    /// What `expected_policy_ref` must say to move this pointer.
    pub fn expected_ref(&self) -> Ref {
        self.active_policy_ref
            .clone()
            .unwrap_or_else(crate::null_policy_ref)
    }
}

/// One line of `transitions.jsonl`: the exact transition, its digest, the
/// state it replaced and the state it produced.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LogEntry {
    pub schema: LogSchema,
    pub transition_ref: Ref,
    pub transition: PolicyTransition,
    pub prior_epoch: AuthorityEpoch,
    pub result: PointerRecord,
    pub applied_ms: u64,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub policy_ref: Ref,
    pub reason_ref: Ref,
    pub issuer_ref: OpaqueRef,
    pub revoked_ms: u64,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RevocationList {
    pub schema: RevocationSchema,
    pub scope: Scope,
    pub revoked: Vec<Revocation>,
}

fn pointer_path(store: &Store, scope: &Scope) -> std::path::PathBuf {
    store.scope_dir(scope).join("pointer.json")
}
fn log_path(store: &Store, scope: &Scope) -> std::path::PathBuf {
    store.scope_dir(scope).join("transitions.jsonl")
}
fn revocations_path(store: &Store, scope: &Scope) -> std::path::PathBuf {
    store.scope_dir(scope).join("revocations.json")
}

/// Load (and, if a crash left the journal ahead of the pointer, roll forward)
/// the scope's pointer. Takes the scope lock.
pub fn load(store: &Store, scope: &Scope) -> Result<PointerRecord> {
    let _lock = DirLock::acquire(&store.scope_dir(scope))?;
    load_locked(store, scope)
}

fn load_locked(store: &Store, scope: &Scope) -> Result<PointerRecord> {
    let mut p = read_json::<PointerRecord>(&pointer_path(store, scope))?
        .unwrap_or_else(|| PointerRecord::initial(scope));
    if &p.scope != scope {
        return Err(LoopError::Io("pointer.json names a different scope".into()));
    }
    let log: Vec<LogEntry> = read_jsonl(&log_path(store, scope))?;
    if let Some(last) = log.last() {
        if last.result.epoch > p.epoch {
            if last.prior_epoch != p.epoch {
                return Err(LoopError::Io(format!(
                    "journal/pointer split: journal at epoch {}, pointer at {}",
                    last.result.epoch.get(),
                    p.epoch.get()
                )));
            }
            // Journalled but not published: the transition was decided and
            // made durable; finish publishing it.
            write_json_atomic(&pointer_path(store, scope), &last.result)?;
            p = last.result.clone();
        } else if last.result.epoch < p.epoch {
            return Err(LoopError::Io("pointer ahead of its journal".into()));
        }
    } else if p.epoch.get() != 0 {
        return Err(LoopError::Io("pointer has an epoch but no journal".into()));
    }
    Ok(p)
}

pub fn revocations(store: &Store, scope: &Scope) -> Result<RevocationList> {
    Ok(
        read_json::<RevocationList>(&revocations_path(store, scope))?.unwrap_or(RevocationList {
            schema: RevocationSchema,
            scope: scope.clone(),
            revoked: Vec::new(),
        }),
    )
}

pub fn is_revoked(store: &Store, scope: &Scope, policy: &Ref) -> Result<bool> {
    Ok(revocations(store, scope)?
        .revoked
        .iter()
        .any(|r| &r.policy_ref == policy))
}

/// Add a policy to the scope's revocation list (idempotent). Does not move
/// the pointer: a revoked ACTIVE policy makes [`resolve`] refuse, and the
/// operator then issues a fenced pause (or a rollback to a valid predecessor).
pub fn revoke(
    store: &Store,
    scope: &Scope,
    policy_ref: &Ref,
    reason_ref: &Ref,
    issuer: &OpaqueRef,
) -> Result<RevocationList> {
    let _lock = DirLock::acquire(&store.scope_dir(scope))?;
    if !store.config()?.admitters().contains(issuer) {
        return Err(refused(format!(
            "issuer {issuer} is not a trusted admitter"
        )));
    }
    let mut list = revocations(store, scope)?;
    if list.revoked.iter().any(|r| &r.policy_ref == policy_ref) {
        return Ok(list);
    }
    list.revoked.push(Revocation {
        policy_ref: policy_ref.clone(),
        reason_ref: reason_ref.clone(),
        issuer_ref: issuer.clone(),
        revoked_ms: crate::now_ms(),
    });
    write_json_atomic(&revocations_path(store, scope), &list)?;
    Ok(list)
}

/// What a NEW task pins. Refuses (Paused) when the scope has no active policy
/// or its active policy has been revoked — there is no unsafe fallback.
pub fn resolve(store: &Store, scope: &Scope) -> Result<(PolicyPin, PolicyEnvelope)> {
    let _lock = DirLock::acquire(&store.scope_dir(scope))?;
    let p = load_locked(store, scope)?;
    let active = p.active_policy_ref.clone().ok_or_else(|| {
        LoopError::Paused(format!(
            "scope {}/{} is paused at epoch {}",
            scope.tenant_id,
            scope.task_family,
            p.epoch.get()
        ))
    })?;
    if is_revoked(store, scope, &active)? {
        return Err(LoopError::Paused(format!(
            "active policy {active} is revoked; issue a fenced pause or rollback"
        )));
    }
    let env: PolicyEnvelope = store.get_contract("policies", &active)?;
    let pin = PolicyPin {
        version: env.version()?,
        epoch: p.epoch,
        pinned_at_ms: crate::now_ms() as i64,
        ack: PinAck::Acknowledged,
    };
    Ok((pin, env))
}

/// Apply one fenced transition. See the module docs for the full rule.
pub fn transition(store: &Store, t: &PolicyTransition) -> Result<PointerRecord> {
    let scope = &t.scope;
    let _lock = DirLock::acquire(&store.scope_dir(scope))?;
    let cur = load_locked(store, scope)?;
    let t_ref = axon_loop_contracts::digest(t)?;

    // Idempotent replay of an already-applied transition id.
    let log: Vec<LogEntry> = read_jsonl(&log_path(store, scope))?;
    if let Some(e) = log
        .iter()
        .find(|e| e.transition.transition_id == t.transition_id)
    {
        if e.transition_ref == t_ref {
            return Ok(e.result.clone());
        }
        return Err(LoopError::Conflict(format!(
            "transition id {} was already used for different content",
            t.transition_id
        )));
    }

    let config = store.config()?;
    if !config.admitters().contains(&t.issuer_ref) {
        return Err(refused(format!(
            "issuer {} is not in the trusted-admitter set",
            t.issuer_ref
        )));
    }
    if t.expected_epoch != cur.epoch {
        return Err(LoopError::Conflict(format!(
            "stale epoch: expected_epoch {} but current is {}",
            t.expected_epoch.get(),
            cur.epoch.get()
        )));
    }
    if t.expected_policy_ref != cur.expected_ref() {
        return Err(LoopError::Conflict(format!(
            "wrong expected policy: {} but active is {}",
            t.expected_policy_ref,
            cur.expected_ref()
        )));
    }
    // Belt and braces: the contract parse enforces this too.
    if t.next_epoch != cur.epoch.next()? {
        return Err(LoopError::Conflict(
            "next_epoch must be expected_epoch + 1".into(),
        ));
    }

    let mut next = cur.clone();
    next.epoch = t.next_epoch;
    next.last_transition_id = Some(t.transition_id.clone());
    let retire = |next: &mut PointerRecord| {
        if let (Some(p), Some(a), Some(since)) = (
            cur.active_policy_ref.clone(),
            cur.active_admission_ref.clone(),
            cur.active_since_epoch,
        ) {
            next.history.push(HistoryEntry {
                policy_ref: p,
                admission_ref: a,
                activated_epoch: since,
                retired_epoch: t.next_epoch,
            });
        }
    };

    match t.kind {
        TransitionKind::Pause => {
            retire(&mut next);
            next.active_policy_ref = None;
            next.active_admission_ref = None;
            next.active_since_epoch = None;
        }
        TransitionKind::Activate | TransitionKind::Rollback => {
            let target = t.target_policy_ref.clone().expect("validated by parse");
            let adm_ref = t.admission_ref.clone().expect("validated by parse");
            check_admitted_target(store, t, &target, &adm_ref, &config.admitters())?;
            if cur.active_policy_ref.as_ref() == Some(&target) {
                return Err(refused(format!("{target} is already the active policy")));
            }
            if t.kind == TransitionKind::Rollback
                && !cur.history.iter().any(|h| h.policy_ref == target)
            {
                return Err(refused(format!(
                    "rollback target {target} was never an active predecessor in this scope"
                )));
            }
            retire(&mut next);
            next.active_policy_ref = Some(target);
            next.active_admission_ref = Some(adm_ref);
            next.active_since_epoch = Some(t.next_epoch);
        }
    }

    let entry = LogEntry {
        schema: LogSchema,
        transition_ref: t_ref,
        transition: t.clone(),
        prior_epoch: cur.epoch,
        result: next.clone(),
        applied_ms: crate::now_ms(),
    };
    append_jsonl(&log_path(store, scope), &entry)?;
    write_json_atomic(&pointer_path(store, scope), &next)?;
    Ok(next)
}

fn check_admitted_target(
    store: &Store,
    t: &PolicyTransition,
    target: &Ref,
    adm_ref: &Ref,
    admitters: &std::collections::BTreeSet<OpaqueRef>,
) -> Result<()> {
    let adm: AdmissionRecord = store.get_record("admissions", adm_ref)?;
    if adm.decision != Decision::Accept {
        return Err(refused(format!(
            "admission {adm_ref} decided {:?}, not ACCEPT",
            adm.decision
        )));
    }
    if &adm.target_policy_ref != target {
        return Err(refused(format!(
            "admission {adm_ref} admits {}, not {target}",
            adm.target_policy_ref
        )));
    }
    if adm.scope != t.scope {
        return Err(refused("admission is for a different scope"));
    }
    if adm.mechanism_test != t.mechanism_test {
        return Err(refused(
            "mechanism_test label differs between admission and transition (fixture evidence laundering)",
        ));
    }
    if !admitters.contains(&adm.admitter_ref) {
        return Err(refused(format!(
            "admitter {} is no longer trusted",
            adm.admitter_ref
        )));
    }
    if !adm.deployment_enabled {
        return Err(refused("the admitted plan has deployment_enabled = false"));
    }
    if is_revoked(store, &t.scope, target)? {
        let hint = if t.kind == TransitionKind::Rollback {
            "; no safe predecessor: issue a pause instead"
        } else {
            ""
        };
        return Err(refused(format!("{target} is revoked{hint}")));
    }
    let env: PolicyEnvelope = store.get_contract("policies", target)?;
    if env.scope != t.scope || env.controls_ref != adm.controls_ref {
        return Err(refused(
            "stored target envelope scope/controls differ from its admission",
        ));
    }
    Ok(())
}

/// The append-only transition log.
pub fn log(store: &Store, scope: &Scope) -> Result<Vec<LogEntry>> {
    let _lock = DirLock::acquire(&store.scope_dir(scope))?;
    read_jsonl(&log_path(store, scope))
}
