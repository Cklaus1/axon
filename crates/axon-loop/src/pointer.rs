//! B278/B279 — the fenced, durable, per-scope active-policy pointer.
//!
//! The pointer is a PROJECTION of the store ledger ([`crate::ledger`]): the
//! state is the last `transition` event for the scope, and
//! `scopes/<tenant>/<family>/pointer.json` is a cache the ledger verifies on
//! every open. It moves ONLY through [`transition`], which, inside one ledger
//! transaction:
//!
//! 1. replays an already-applied `transition_id` idempotently (same bytes ⇒
//!    same reply; same id with different bytes ⇒ conflict);
//! 2. refuses an issuer outside `config.trusted_admitters`;
//! 3. compare-and-swaps on `(expected_policy_ref, expected_epoch)`;
//! 4. activate — two routes, nothing else:
//!    * from PAUSED only, to the scope's incumbent-of-record, named by a
//!      journalled [`BaselineRecord`] (H2);
//!    * from an ACTIVE policy, with an admission that is RE-DERIVED, not
//!      trusted ([`crate::admission::rederive`]): journalled by `admit`, its
//!      frozen plan and journalled evaluation recomputed to the identical
//!      ACCEPT record, target = the plan's EVO candidate, the admission's
//!      incumbent = the active policy (H1), the evaluation made at the
//!      current epoch (K1), the active policy not a mechanism-test
//!      activation unless this is one too (M4);
//! 5. rollback — only to a policy that was actually active in this scope
//!    (ledger replay, not the editable projection), under the SAME
//!    admission/baseline it was active under, with the same mechanism label,
//!    not revoked;
//! 6. appends the transition to the ledger, then publishes the projection.
//!
//! A refused transition writes nothing.

use crate::admission;
use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{
    AuthorityEpoch, OpaqueRef, PinAck, PolicyEnvelope, PolicyPin, PolicyTransition, Ref, Scope,
    TransitionId, TransitionKind,
};
use serde::{Deserialize, Serialize};

crate::record_tag!(PointerSchema, "axon.loop.pointer/1");
crate::record_tag!(PointerFileSchema, "axon.loop.pointer-projection/1");
crate::record_tag!(BaselineSchema, "axon.loop.baseline/1");

/// A previously-active policy: the only legal rollback targets.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryEntry {
    pub policy_ref: Ref,
    pub admission_ref: Ref,
    pub mechanism_test: bool,
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
    /// The active policy was activated by a MECHANISM-TEST transition: it is
    /// a fixture, excluded from improvement claims, and never an incumbent
    /// for a non-mechanism activation.
    pub active_mechanism_test: bool,
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
            active_mechanism_test: false,
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

/// `pointer.json`: the projection of ledger entry `ledger_seq`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PointerFile {
    pub pointer: PointerRecord,
    pub ledger_seq: u64,
    pub entry_ref: Ref,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Revocation {
    pub policy_ref: Ref,
    pub reason_ref: Ref,
    pub issuer_ref: OpaqueRef,
    pub revoked_ms: u64,
}

/// A trusted admitter designates the scope's incumbent-of-record: the one
/// policy that may be activated from PAUSED without a comparative admission.
/// It must be a stored policy that is NOT an EVO candidate. One per scope.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaselineRecord {
    pub schema: BaselineSchema,
    pub scope: Scope,
    pub policy_ref: Ref,
    pub issuer_ref: OpaqueRef,
    pub reason_ref: Ref,
}

pub(crate) fn pointer_path(store: &Store, scope: &Scope) -> std::path::PathBuf {
    store.scope_dir(scope).join("pointer.json")
}

/// The scope's pointer (ledger replay; the projection is verified on open).
pub fn load(store: &Store, scope: &Scope) -> Result<PointerRecord> {
    Ok(Tx::begin(store)?.pointer(scope))
}

pub fn revocations(store: &Store, scope: &Scope) -> Result<Vec<Revocation>> {
    Ok(Tx::begin(store)?.revocations(scope))
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
) -> Result<Vec<Revocation>> {
    let mut tx = Tx::begin(store)?;
    if !store.config()?.admitters().contains(issuer) {
        return Err(refused(format!(
            "issuer {issuer} is not a trusted admitter"
        )));
    }
    if !tx.is_revoked(scope, policy_ref) {
        tx.append(Event::Revocation {
            scope: scope.clone(),
            revocation: Revocation {
                policy_ref: policy_ref.clone(),
                reason_ref: reason_ref.clone(),
                issuer_ref: issuer.clone(),
                revoked_ms: crate::now_ms(),
            },
        })?;
    }
    Ok(tx.revocations(scope))
}

pub fn parse_baseline(text: &str) -> Result<BaselineRecord> {
    strict_record(text)
}

/// Designate the scope's incumbent-of-record. Returns the baseline ref.
pub fn designate_baseline(store: &Store, b: &BaselineRecord) -> Result<Ref> {
    let mut tx = Tx::begin(store)?;
    let config = store.config()?;
    if !config.admitters().contains(&b.issuer_ref) {
        return Err(refused(format!(
            "issuer {} is not a trusted admitter",
            b.issuer_ref
        )));
    }
    // The incumbent-of-record is chosen by an independent admitter, never by
    // an identity that produces or judges evidence (review wf_d788c05a-be2).
    if let Some(role) = crate::admission::other_loop_role(&config, &b.issuer_ref) {
        return Err(refused(format!(
            "baseline issuer {} is also {role}; it must hold no other loop role",
            b.issuer_ref
        )));
    }
    if tx.hypotheses(&b.scope, None).iter().any(|h| {
        matches!(h, crate::evo::Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == &b.issuer_ref)
    }) {
        return Err(refused(format!(
            "baseline issuer {} is an EVO proposer (the ranker) in this scope",
            b.issuer_ref
        )));
    }
    let r = axon_loop_contracts::digest(b)?;
    if let Some((existing, _)) = tx.baseline_of(&b.scope) {
        if existing == r {
            return Ok(r);
        }
        return Err(refused("the scope already has an incumbent-of-record"));
    }
    let env: PolicyEnvelope = store.get_contract("policies", &b.policy_ref)?;
    if env.scope != b.scope {
        return Err(refused("baseline policy is for another scope"));
    }
    crate::candidates::require_shortlist(&tx, &env)?;
    if crate::evo::proposer_in(&tx, &b.scope, &b.policy_ref).is_some() {
        return Err(refused(
            "an EVO candidate cannot be an incumbent-of-record; it must be admitted",
        ));
    }
    if tx.is_revoked(&b.scope, &b.policy_ref) {
        return Err(refused("baseline policy is revoked"));
    }
    store.put_cas("baselines", b)?;
    tx.append(Event::Baseline {
        scope: b.scope.clone(),
        baseline_ref: r.clone(),
        policy_ref: b.policy_ref.clone(),
    })?;
    Ok(r)
}

/// What `resolve` gives a NEW task.
#[derive(Clone, Debug, Serialize)]
pub struct Resolved {
    pub pin: PolicyPin,
    pub policy: PolicyEnvelope,
    /// The active policy is a mechanism-test fixture: never evidence of
    /// improvement, never training data.
    pub mechanism_test: bool,
    pub admission_ref: Ref,
}

/// What a NEW task pins. Refuses (Paused) when the scope has no active policy
/// or its active policy has been revoked — there is no unsafe fallback.
pub fn resolve(store: &Store, scope: &Scope) -> Result<Resolved> {
    let tx = Tx::begin(store)?;
    let p = tx.pointer(scope);
    let active = p.active_policy_ref.clone().ok_or_else(|| {
        LoopError::Paused(format!(
            "scope {}/{} is paused at epoch {}",
            scope.tenant_id,
            scope.task_family,
            p.epoch.get()
        ))
    })?;
    if tx.is_revoked(scope, &active) {
        return Err(LoopError::Paused(format!(
            "active policy {active} is revoked; issue a fenced pause or rollback"
        )));
    }
    let env: PolicyEnvelope = store.get_contract("policies", &active)?;
    Ok(Resolved {
        pin: PolicyPin {
            version: env.version()?,
            epoch: p.epoch,
            pinned_at_ms: crate::now_ms() as i64,
            ack: PinAck::Acknowledged,
        },
        policy: env,
        mechanism_test: p.active_mechanism_test,
        admission_ref: p
            .active_admission_ref
            .clone()
            .expect("active has admission"),
    })
}

/// Apply one fenced transition. See the module docs for the full rule.
pub fn transition(store: &Store, t: &PolicyTransition) -> Result<PointerRecord> {
    let mut tx = Tx::begin(store)?;
    let scope = &t.scope;
    let t_ref = axon_loop_contracts::digest(t)?;

    // Idempotent replay of an already-applied transition id (any scope).
    for e in tx.entries() {
        if let Event::Transition {
            transition_ref,
            transition,
            result,
            ..
        } = &e.event
        {
            if transition.transition_id == t.transition_id {
                if transition_ref == &t_ref {
                    return Ok((**result).clone());
                }
                return Err(LoopError::Conflict(format!(
                    "transition id {} was already used for different content",
                    t.transition_id
                )));
            }
        }
    }

    let cur = tx.pointer(scope);
    let config = store.config()?;
    if !config.admitters().contains(&t.issuer_ref) {
        return Err(refused(format!(
            "issuer {} is not in the trusted-admitter set",
            t.issuer_ref
        )));
    }
    // G11-r22-independent-admission / rollback-revalidate: a transition —
    // activate, rollback, pause — is issued by an INDEPENDENT admitter, never
    // by an identity that also produces or judges evidence (Compute Fabric).
    if let Some(role) = crate::admission::other_loop_role(&config, &t.issuer_ref) {
        return Err(refused(format!(
            "self-promotion: issuer {} is also {role}; a transition issuer must hold no other \
             loop role",
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
    if t.next_epoch != cur.epoch.next()? {
        return Err(LoopError::Conflict(
            "next_epoch must be expected_epoch + 1".into(),
        ));
    }

    let mut next = cur.clone();
    next.epoch = t.next_epoch;
    next.last_transition_id = Some(t.transition_id.clone());
    if let (Some(p), Some(a), Some(since)) = (
        cur.active_policy_ref.clone(),
        cur.active_admission_ref.clone(),
        cur.active_since_epoch,
    ) {
        next.history.push(HistoryEntry {
            policy_ref: p,
            admission_ref: a,
            mechanism_test: cur.active_mechanism_test,
            activated_epoch: since,
            retired_epoch: t.next_epoch,
        });
    }

    match t.kind {
        TransitionKind::Pause => {
            next.active_policy_ref = None;
            next.active_admission_ref = None;
            next.active_mechanism_test = false;
            next.active_since_epoch = None;
        }
        TransitionKind::Activate | TransitionKind::Rollback => {
            let target = t.target_policy_ref.clone().expect("validated by parse");
            let adm_ref = t.admission_ref.clone().expect("validated by parse");
            if cur.active_policy_ref.as_ref() == Some(&target) {
                return Err(refused(format!("{target} is already the active policy")));
            }
            if tx.is_revoked(scope, &target) {
                let hint = if t.kind == TransitionKind::Rollback {
                    "; no safe predecessor: issue a pause instead"
                } else {
                    ""
                };
                return Err(refused(format!("{target} is revoked{hint}")));
            }
            if t.kind == TransitionKind::Rollback {
                check_rollback(&tx, &cur, t, &target, &adm_ref, &config.admitters())?;
            } else {
                check_activate(&tx, &cur, t, &target, &adm_ref, &config.admitters())?;
            }
            if config.protected_scopes.contains(scope) {
                protected_scope_gate(&tx, t, &adm_ref, &config.admitters())?;
            }
            let env: PolicyEnvelope = store.get_contract("policies", &target)?;
            // G2: every activation route (baseline, admission, rollback).
            crate::candidates::require_shortlist(&tx, &env)?;
            if env.scope != *scope {
                return Err(refused("target envelope is for another scope"));
            }
            next.active_policy_ref = Some(target);
            next.active_admission_ref = Some(adm_ref);
            next.active_mechanism_test = t.mechanism_test;
            next.active_since_epoch = Some(t.next_epoch);
        }
    }

    tx.append(Event::Transition {
        transition_ref: t_ref,
        transition: Box::new(t.clone()),
        prior: Box::new(cur),
        result: Box::new(next.clone()),
    })?;
    Ok(next)
}

fn check_rollback(
    tx: &Tx,
    cur: &PointerRecord,
    t: &PolicyTransition,
    target: &Ref,
    adm_ref: &Ref,
    admitters: &std::collections::BTreeSet<OpaqueRef>,
) -> Result<()> {
    // History comes from the ledger replay, never from the editable file.
    let h = cur
        .history
        .iter()
        .rev()
        .find(|h| &h.policy_ref == target)
        .ok_or_else(|| {
            refused(format!(
                "rollback target {target} was never an active predecessor in this scope"
            ))
        })?;
    if &h.admission_ref != adm_ref {
        return Err(refused(
            "rollback must cite the admission/baseline the predecessor was active under",
        ));
    }
    if h.mechanism_test != t.mechanism_test {
        return Err(refused(if h.mechanism_test {
            "a mechanism-test activation is not a rollback target for a non-mechanism transition"
        } else {
            "mechanism_test label differs from the predecessor's activation"
        }));
    }
    // ADR-001 §5 rollback revalidation: the predecessor's authority must
    // still hold NOW, not merely have held when it was first activated. A
    // baseline needs its issuer still trusted (as route 1 of an activation
    // does); an admission must still re-derive, its admitter still trusted.
    // If not, there is no safe predecessor: refuse, and pause instead.
    if let Ok(b) = tx.store.get_record::<BaselineRecord>("baselines", adm_ref) {
        if !admitters.contains(&b.issuer_ref) {
            return Err(refused(
                "rollback target's baseline issuer is no longer trusted; no safe predecessor: \
                 issue a pause instead",
            ));
        }
    } else {
        let adm = admission::rederive(tx, adm_ref, admitters).map_err(|e| {
            refused(format!(
                "rollback target's admission no longer holds ({e}); no safe predecessor: issue a \
                 pause instead"
            ))
        })?;
        admission::issuer_independent(tx, &adm, &t.issuer_ref)?;
        safety_still_holds(tx, &adm)?;
    }
    Ok(())
}

/// ADR-001 §5 activation revalidation, safety half: the admission fixed the
/// candidate's safety as the evaluation saw it; a violation recorded SINCE —
/// against any of the candidate's trials in that evaluation — blocks making it
/// active, on activation and on rollback alike.
fn safety_still_holds(tx: &Tx, adm: &crate::admission::AdmissionRecord) -> Result<()> {
    let (_, eval) = crate::evl::load_journalled(tx, &adm.evaluation_ref)?;
    let now = crate::safety::states(tx, &adm.scope);
    let arm = eval.arm_for_policy(&adm.target_policy_ref)?;
    for t in &arm.trials {
        let key = (
            t.task_id.as_str().to_string(),
            arm.arm_id.as_str().to_string(),
            t.trial_id.as_str().to_string(),
        );
        if let Some(crate::safety::SafetyState::Violation { code }) = now.get(&key) {
            return Err(refused(format!(
                "trial {} was reported unsafe ({code:?}) after the evaluation: the candidate \
                 cannot be made active",
                t.trial_id
            )));
        }
    }
    Ok(())
}

fn check_activate(
    tx: &Tx,
    cur: &PointerRecord,
    t: &PolicyTransition,
    target: &Ref,
    adm_ref: &Ref,
    admitters: &std::collections::BTreeSet<OpaqueRef>,
) -> Result<()> {
    // Route 1: the incumbent-of-record, from PAUSED only.
    if let Some((b_ref, b_policy)) = tx.baseline_of(&t.scope) {
        if &b_ref == adm_ref {
            if cur.active_policy_ref.is_some() {
                return Err(refused(
                    "the incumbent-of-record is activated only from paused; use rollback",
                ));
            }
            if &b_policy != target {
                return Err(refused("baseline names a different policy"));
            }
            if t.mechanism_test {
                return Err(refused("an incumbent-of-record is not a mechanism test"));
            }
            let b: BaselineRecord = tx.store.get_record("baselines", &b_ref)?;
            if !admitters.contains(&b.issuer_ref) {
                return Err(refused("baseline issuer is no longer trusted"));
            }
            return Ok(());
        }
    }
    // Route 2: a comparative admission, from an ACTIVE incumbent only.
    let active = cur.active_policy_ref.as_ref().ok_or_else(|| {
        refused("from paused only the scope's incumbent-of-record may be activated (H2)")
    })?;
    if cur.active_mechanism_test && !t.mechanism_test {
        return Err(refused(
            "the active policy is a mechanism-test fixture: it is not an incumbent for a real activation; roll back first",
        ));
    }
    let adm = admission::rederive(tx, adm_ref, admitters)?;
    admission::issuer_independent(tx, &adm, &t.issuer_ref)?;
    if &adm.target_policy_ref != target {
        return Err(refused(format!(
            "admission {adm_ref} admits {}, not {target}",
            adm.target_policy_ref
        )));
    }
    if adm.scope != t.scope {
        return Err(refused("admission is for a different scope"));
    }
    if &adm.incumbent_policy_ref != active {
        return Err(refused(format!(
            "admission compared the candidate against {}, but the active policy is {active} (H1)",
            adm.incumbent_policy_ref
        )));
    }
    if adm.evaluated_at_epoch != cur.epoch {
        return Err(LoopError::Conflict(format!(
            "stale evidence: evaluated at epoch {}, current epoch {} (K1)",
            adm.evaluated_at_epoch.get(),
            cur.epoch.get()
        )));
    }
    if adm.mechanism_test != t.mechanism_test {
        return Err(refused(
            "mechanism_test label differs between admission and transition (fixture evidence laundering)",
        ));
    }
    if !adm.deployment_enabled {
        return Err(refused("the admitted plan has deployment_enabled = false"));
    }
    safety_still_holds(tx, &adm)?;
    Ok(())
}

/// ADR-001 D3, on EVERY route that makes a policy active in a protected scope
/// — activate and rollback alike (re-audit 3: the check lived only in
/// `check_activate`, and a `mechanism_test` label skipped even that):
///
/// * a protected scope serves no mechanism-test fixture;
/// * a policy that became eligible by ADMISSION is active only on a
///   re-derived admission whose evaluation is of the protected class — so a
///   rollback to a predecessor admitted on development evidence is refused,
///   whenever the scope was protected;
/// * the incumbent-of-record is exempt, by design: it is an admitter's
///   designation, not evaluated evidence, and a protected scope needs one to
///   start (a comparative admission requires an active incumbent). Signed
///   baselines are ADR-001 §3.4.
///
/// Not covered here (ADR-001 §5, activation revalidation, pending): a policy
/// already ACTIVE when the operator protects the scope stays active until the
/// next transition.
fn protected_scope_gate(
    tx: &Tx,
    t: &PolicyTransition,
    adm_ref: &Ref,
    admitters: &std::collections::BTreeSet<OpaqueRef>,
) -> Result<()> {
    if t.mechanism_test {
        return Err(refused(
            "scope is protected: it serves no mechanism-test fixture (ADR-001 D3)",
        ));
    }
    if tx
        .store
        .get_record::<BaselineRecord>("baselines", adm_ref)
        .is_ok()
    {
        return Ok(());
    }
    let adm = admission::rederive(tx, adm_ref, admitters)?;
    let (_, eval) = crate::evl::load_journalled(tx, &adm.evaluation_ref)?;
    if eval.evaluation_class != crate::plan::EvaluationClass::Protected {
        return Err(refused(
            "scope is protected: a development-class evaluation cannot make a policy active \
             (ADR-001 D3)",
        ));
    }
    Ok(())
}

/// The scope's applied transitions, in order.
pub fn log(store: &Store, scope: &Scope) -> Result<Vec<PolicyTransition>> {
    Ok(Tx::begin(store)?
        .entries()
        .iter()
        .filter_map(|e| match &e.event {
            Event::Transition { transition, .. } if &transition.scope == scope => {
                Some((**transition).clone())
            }
            _ => None,
        })
        .collect())
}
