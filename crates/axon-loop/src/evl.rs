//! B276 / B265 — evaluate EXACT artifacts outside subject authority.
//!
//! Input: the assigned trials, and for each delivered trial its
//! `LoopEpisode`, the `ExecutionContextReceipt`, the ACF request + receipt +
//! policy projection, and the arm's `PolicyEnvelope`. A trial is a
//! VERIFIED PASS only if ALL of:
//!
//! * `bind_episode` succeeds (identity, scope, exact policy/context bytes,
//!   controls, input workspace, current authority epoch of the scope);
//! * `bind_acf` succeeds (exact request/receipt bytes, projection, ids,
//!   workspaces, status semantics, and B265 roles: no context/request/
//!   execution receipt standing as the verifier, completion only on a
//!   supervisor-observed receipt, a pass only over the exact bytes the
//!   execution left);
//! * the episode and context are for THIS evaluation's scope — evidence
//!   minted for another tenant is `unknown: cross-tenant`, never joined;
//! * the trial's preflight context passes the bounded paired-trial profile
//!   ([`axon_loop_contracts::check_paired_trial_context`]: expected ==
//!   observed, the observer is NOT the expecting parent, NOT a subject issuer
//!   (the verifier rule, mirrored: NS4p/NS4w) and IS in the store's
//!   `trusted_observers`, the window `created_ms <= now < expires_ms` at
//!   evaluation time, the current epoch, a non-primary worktree, concrete
//!   paths). A trial failing it is TASK_NOT_STARTED evidence — `unknown`,
//!   never a pass (AB6/AB7/AB8). Contexts must therefore be evaluated within
//!   their validity window;
//! * `verification.result == passed` with `matched_checks > 0`;
//! * the verification issuer is in the store's `trusted_verifiers` and is NOT
//!   a subject issuer (the request's list, plus the candidate's proposer).
//!
//! Anything else is `fail` (a clear negative) or `unknown` (unbound,
//! unverifiable, missing, timed out, …). Both count AGAINST quality: the
//! denominator is every ASSIGNED trial, and an assigned trial with no
//! delivered episode is `unknown: missing` — reported, not dropped.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{contract_from_value, strict_record, Store};
use crate::tel::{self, Summary};
use axon_loop_contracts::{
    bind_acf, bind_episode, check_paired_trial_context, digest, digest_value, ArmId,
    AuthorityEpoch, ComputeRequest, CorpusRole, EpisodeStatus, ExecutionContextReceipt,
    ExecutionReceipt, LoopEpisode, OpaqueRef, PolicyEnvelope, PolicyProjection, ReceiptStatus, Ref,
    Scope, TaskId, TrialId, VerificationResult,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

crate::record_tag!(EvlRequestSchema, "axon.loop.evl-request/1");
crate::record_tag!(EvaluationSchema, "axon.loop.evaluation/1");

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Assignment {
    pub task_id: TaskId,
    pub arm_id: ArmId,
    pub trial_id: TrialId,
    pub policy_ref: Ref,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliveredTrial {
    pub episode: Value,
    pub context: Value,
    pub acf_request: Value,
    pub acf_receipt: Value,
    pub projection: Value,
    /// G01-r22-independent-issuer: the VERIFICATION evidence the episode cites
    /// (`verification.verifier_ref`): the registered check's request and
    /// receipt, and the verifier's `acf-receipt-attestation/2` over them —
    /// never the trial's execution documents. A verdict (passed or failed)
    /// counts only if [`crate::intake::verify_check_evidence`] accepts them,
    /// exactly as intake does; otherwise the trial is `Unknown`.
    #[serde(default)]
    pub verification_request: Value,
    #[serde(default)]
    pub verification_receipt: Value,
    #[serde(default)]
    pub verification_attestation: Value,
    /// B2: for a PROTECTED verdict, the `axon-psv-evidence/1` bundle its joins
    /// are verified over (as intake does). Omitted when absent, so a request
    /// that never carried it keeps its canonical bytes.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub verification_psv_evidence: Value,
    /// PROTECTED class: Fabric's execution attestation
    /// (`attestation::EXECUTION_DOMAIN`) over `acf_request` and `acf_receipt`.
    /// Without it the execution's backend is only the producer's claim.
    /// Omitted when absent.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub acf_attestation: Value,
    /// G32-r22-sidecar-bindings: the preflight observer's detached signature
    /// (`axon-document-signature/1`, domain [`CONTEXT_DOMAIN`]) over `context`.
    /// Required in a PROTECTED-class evaluation. Omitted when absent, so a
    /// request that never carried it keeps its canonical bytes.
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub context_signature: Value,
}

/// Verify Fabric's execution attestation (`attestation::EXECUTION_DOMAIN`)
/// over an execution's request and receipt: by a TRUSTED verifier, under the
/// key the OPERATOR's verifier root holds for it. Returns who signed, and the
/// key. Used by EVL for a protected execution leg, and by admission.
pub fn verify_execution(
    att: &Value,
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
    config: &crate::store::Config,
) -> std::result::Result<SignedBy, String> {
    if att.is_null() {
        return Err("no Fabric execution attestation was delivered".into());
    }
    let issuer = OpaqueRef::new(
        att["issuer_ref"]
            .as_str()
            .ok_or("the execution attestation names no issuer")?,
    )
    .map_err(|e| e.to_string())?;
    if !config.verifiers().contains(&issuer) {
        return Err(format!("{issuer} is not a trusted verifier"));
    }
    let key = crate::store::Config::rooted_key(
        &config.verifier_keys,
        &issuer,
        axon_loop_contracts::operator_trust::TrustAuthority::Verifier,
    )?;
    let doc =
        axon_loop_contracts::attestation::execution_document(req, rc).map_err(|e| e.to_string())?;
    let key_id = axon_loop_contracts::attestation::verify_document(
        att,
        axon_loop_contracts::attestation::EXECUTION_DOMAIN,
        &issuer,
        &doc,
        key,
    )
    .map_err(|e| e.to_string())?;
    Ok(SignedBy {
        issuer_ref: issuer,
        key_id,
    })
}

/// The signature domain of a preflight context receipt.
pub const CONTEXT_DOMAIN: &str = "axon.closed-loop.context/1";

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvlRequest {
    pub schema: EvlRequestSchema,
    /// The FROZEN experiment this evaluation belongs to. Its arms must be
    /// exactly the plan's incumbent and candidate.
    pub experiment_id: String,
    pub scope: Scope,
    pub evaluator_ref: OpaqueRef,
    pub subject_issuers: Vec<OpaqueRef>,
    pub policies: Vec<Value>,
    pub assigned: Vec<Assignment>,
    pub trials: Vec<DeliveredTrial>,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    VerifiedPass,
    Fail,
    Unknown,
}

/// ADR-001 §5: WHY a delivered trial is Unknown — a closed set, so an
/// admission (and a reader) can tell "the check timed out" from "the evidence
/// never bound" without parsing prose. A trial never delivered is not a kind:
/// it is counted in [`ArmResult::missing`].
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnknownKind {
    /// The run hit its deadline before a verdict.
    TimedOut,
    /// The run was canceled before a verdict.
    Cancelled,
    /// A pass that matched no check.
    Unmatched,
    /// The verification evidence was not delivered, or ended without one.
    MissingEvidence,
    /// Evidence was delivered but cannot be verified: unauthenticated, an
    /// untrusted issuer, or a backend ineligible for this evaluation class.
    Unverifiable,
    /// No verification was run.
    NotRun,
    /// The evidence does not bind to this trial, arm, policy or context.
    Unbound,
}

type Judged = (Outcome, String, Option<UnknownKind>);

fn unknown(kind: UnknownKind, why: impl Into<String>) -> Judged {
    (Outcome::Unknown, why.into(), Some(kind))
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialResult {
    pub task_id: TaskId,
    pub trial_id: TrialId,
    pub outcome: Outcome,
    pub reason: String,
    /// Set exactly when `outcome` is Unknown for a DELIVERED trial.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unknown_kind: Option<UnknownKind>,
    #[serde(deserialize_with = "crate::nullable")]
    pub episode_ref: Option<Ref>,
    #[serde(deserialize_with = "crate::nullable")]
    pub corpus_role: Option<CorpusRole>,
    /// The verification evidence this outcome rests on, as authenticated:
    /// present exactly when a verdict (pass or fail) was counted, so a later
    /// audit can re-verify the attestation against the documents it names.
    /// Absent on older records and on every Unknown.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification: Option<VerificationEvidence>,
    /// ADR-001 §5: the trial's safety as recorded when this evaluation was
    /// made (`crate::safety`). Absent = unknown, so older records keep their
    /// bytes.
    #[serde(
        default,
        skip_serializing_if = "crate::safety::SafetyState::is_unknown"
    )]
    pub safety: crate::safety::SafetyState,
    /// PROTECTED class, counted trial: the observer whose signature
    /// authenticated the preflight context, and the key it verified under, so
    /// every re-derivation can require that authority still to be current.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_signed_by: Option<SignedBy>,
    /// Counted trial, any class: the observer its preflight context was
    /// admitted under (a development evaluation judges it by this name), so a
    /// re-derivation can require that observer to be trusted still.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_observer_ref: Option<OpaqueRef>,
}

/// Who signed a document, under which operator-registered key.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SignedBy {
    pub issuer_ref: OpaqueRef,
    /// `ed25519:<16 hex>`.
    pub key_id: String,
}

/// Which signed verification a counted verdict rests on.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerificationEvidence {
    pub request_ref: Ref,
    pub receipt_ref: Ref,
    pub attestation_ref: Ref,
    pub issuer_ref: OpaqueRef,
    /// The operator-registered key it verified under (`ed25519:<16 hex>`).
    pub key_id: String,
    /// PROTECTED verdict: the `axon-psv-evidence/1` bundle it was joined over
    /// (`fabric-psv-evidence/`), so admission can re-verify it from the
    /// documents rather than trust the stored record (dev round
    /// wf_336353cb-a2b, PSV-7).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub psv_evidence_ref: Option<Ref>,
    /// PROTECTED class: Fabric's execution attestation for the trial's
    /// execution leg (`acf-attestations/`), re-verified at admission.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_attestation_ref: Option<Ref>,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmResult {
    pub arm_id: ArmId,
    pub policy_ref: Ref,
    pub assigned: u64,
    pub verified_pass: u64,
    pub fail: u64,
    pub unknown: u64,
    pub missing: u64,
    /// ADR-001 §5: the delivered Unknowns by kind. `unknown` counts these and
    /// `missing` together; `assigned = verified_pass + fail + Σ + missing`.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub unknown_kinds: BTreeMap<UnknownKind, u64>,
    pub trials: Vec<TrialResult>,
    pub economics: Summary,
}

/// The stored evaluation (`evaluations/<hex>.json`).
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvaluationRecord {
    pub schema: EvaluationSchema,
    pub scope: Scope,
    pub experiment_id: String,
    pub plan_ref: Ref,
    /// Ledger sequence of the plan's freeze; every trial's context was
    /// created after that freeze.
    pub freeze_seq: u64,
    pub authority_epoch: AuthorityEpoch,
    pub evaluator_ref: OpaqueRef,
    pub trusted_verifiers: Vec<OpaqueRef>,
    pub subject_issuers: Vec<OpaqueRef>,
    pub arms: Vec<ArmResult>,
    /// ADR-001 D3: the frozen plan's evaluation class. Absent (so the record's
    /// bytes are unchanged) for a development evaluation.
    #[serde(
        default,
        skip_serializing_if = "crate::plan::EvaluationClass::is_development"
    )]
    pub evaluation_class: crate::plan::EvaluationClass,
    /// Every episode / context / receipt the evaluation read, by digest.
    pub evidence_refs: Vec<Ref>,
}

pub fn parse_request(text: &str) -> Result<EvlRequest> {
    strict_record(text)
}

/// G32-r22-sidecar-bindings: in a PROTECTED-class evaluation the preflight
/// context is authenticated, not named — its observer's detached signature
/// over exactly these bytes, under the key the operator registered in
/// `observer_keys`. A development evaluation keeps the name rule (it makes no
/// protected claim, ADR-001 D1/D3).
fn authenticated_context(
    config: &crate::store::Config,
    class: crate::plan::EvaluationClass,
    d: &Delivered,
) -> std::result::Result<(), String> {
    if class != crate::plan::EvaluationClass::Protected {
        return Ok(());
    }
    let who = &d.ctx.observed_issuer_ref;
    // O2: in the protected class the observer's key is the OPERATOR's
    // (`/etc/axon/trust/observer/`); the store may only name which one.
    let key = crate::store::Config::rooted_key(
        &config.observer_keys,
        who,
        axon_loop_contracts::operator_trust::TrustAuthority::Observer,
    )
    .map_err(|e| format!("observer {who}: a protected context cannot be authenticated: {e}"))?;
    if d.ctx_sig.is_null() {
        return Err(format!(
            "the context is not authenticated: no signature by observer {who} was presented"
        ));
    }
    let doc = serde_json::to_value(&d.ctx).map_err(|e| e.to_string())?;
    axon_loop_contracts::attestation::verify_document(&d.ctx_sig, CONTEXT_DOMAIN, who, &doc, key)
        .map(|_| ())
        .map_err(|e| format!("context signature refused: {e}"))
}

struct Delivered {
    ep: LoopEpisode,
    ep_ref: Ref,
    ctx: ExecutionContextReceipt,
    /// The trial's EXECUTION documents, or `None` for a D12 trial (see
    /// [`is_d12`]): its execution ran under local MiCode authority, so there
    /// are none, and only its acceptance check went through Fabric.
    acf: Option<(ComputeRequest, ExecutionReceipt, PolicyProjection)>,
    verification: [Value; 3],
    ctx_sig: Value,
    /// B2: the `axon-psv-evidence/1` bundle a protected verdict is joined through.
    psv: Value,
    /// PSV-7: Fabric's execution attestation, for a protected execution leg.
    acf_att: Value,
}

/// ADR-001 D12: only the acceptance CHECK goes through Fabric; the agent's own
/// execution stays under local MiCode authority, and the sidecar names its
/// execution documents with MiCode's not-produced markers. Such a trial can
/// still be JUDGED — its check evidence is Fabric's, authenticated exactly as
/// intake authenticates it — so its non-success states stay distinct
/// (G01-r22-unknown-outcome). It never COUNTS: nothing binds the verdict to
/// the arm's execution, so an authenticated pass or fail is `Unbound`.
fn is_d12(ep: &LoopEpisode) -> bool {
    ep.acf_request_ref == crate::intake::micode_not_produced_ref("acf_request_ref")
        && ep.acf_receipt_ref == crate::intake::micode_not_produced_ref("acf_receipt_ref")
}

/// Evaluate and store. Returns the record and its `cl22:` ref.
///
/// Freeze-before-outcomes (B274 G33): the experiment must already be frozen
/// in the ledger, the arms are exactly its incumbent and candidate, and a
/// trial whose preflight context was created BEFORE the freeze was recorded
/// is refused — its outcome existed before the rule did.
///
/// No evaluation shopping (AB9/AB10): exactly ONE evaluation per experiment;
/// it assigns exactly the frozen task manifest × both arms × `repetitions`;
/// trial ids are unique across every evaluation in the scope.
pub fn evaluate(store: &Store, r: &EvlRequest) -> Result<(EvaluationRecord, Ref)> {
    crate::store::check_segment("experiment id", &r.experiment_id)?;
    let mut tx = Tx::begin(store)?;
    let frozen = crate::plan::ready_in(&tx, &r.experiment_id)?;
    if frozen.plan.scope != r.scope {
        return Err(refused("evaluation scope differs from the frozen plan"));
    }
    let plan_arms: BTreeSet<Ref> = [
        frozen.plan.incumbent_policy_ref.clone().expect("frozen"),
        frozen.plan.candidate_policy_ref.clone().expect("frozen"),
    ]
    .into();
    let config = store.config()?;
    let verifiers = config.verifiers();
    if verifiers.is_empty() {
        return Err(refused(
            "no trusted verifiers configured: nothing can be verified",
        ));
    }
    let epoch = tx.pointer(&r.scope).epoch;
    let observers = config.observers();
    let now = crate::now_ms();

    let mut policies: BTreeMap<Ref, PolicyEnvelope> = BTreeMap::new();
    for (i, v) in r.policies.iter().enumerate() {
        let p: PolicyEnvelope = contract_from_value(&format!("policies[{i}]"), v)?;
        if p.scope != r.scope {
            return Err(refused(format!("policies[{i}] is for another scope")));
        }
        policies.insert(digest(&p)?, p);
    }
    let supplied: BTreeSet<Ref> = policies.keys().cloned().collect();
    if supplied != plan_arms {
        return Err(refused(
            "the evaluated policies must be exactly the frozen plan's incumbent and candidate",
        ));
    }

    // Subject issuers: the request's, plus every stored proposer of an arm policy.
    let mut subjects: BTreeSet<OpaqueRef> = r.subject_issuers.iter().cloned().collect();
    for pref in policies.keys() {
        if let Some(p) = crate::evo::proposer_in(&tx, &r.scope, pref) {
            subjects.insert(p);
        }
    }
    if subjects.contains(&r.evaluator_ref) {
        return Err(refused("the evaluator is a subject issuer"));
    }

    // AB9/AB10 — no evaluation shopping. ONE evaluation per frozen experiment,
    // covering EXACTLY the frozen task manifest × both arms × the planned
    // repetitions, with trial ids never seen in any earlier evaluation of the
    // scope. A trial that is assigned but not delivered is kept and counted
    // as `missing` (never dropped); what cannot be changed is WHICH trials
    // the population contains.
    if let Some((_, prior)) = tx.evaluations_of(&r.experiment_id).first() {
        return Err(refused(format!(
            "experiment {} already has its evaluation {prior}: one evaluation per frozen experiment \
             (a second one would let a REJECT be re-rolled or cherry-picked)",
            r.experiment_id
        )));
    }
    // ADR-001 §3.6: the population was issued BEFORE execution. The request
    // assigns exactly the journalled trials; only each trial's issued attempt
    // counts, and only if intaken after the assignment (below).
    let (assigned_seq, assignment_ref) = tx.assignment_of(&r.experiment_id).ok_or_else(|| {
        refused(format!(
            "experiment {} has no assignment journalled before execution (ADR-001 §3.6): an \
             independent admitter issues the trials and attempt ids with `plan assign` after the \
             freeze, before any trial runs",
            r.experiment_id
        ))
    })?;
    let assignment: crate::plan::AssignmentRecord =
        tx.store.get_record("assignments", &assignment_ref)?;
    let issued: BTreeMap<(TaskId, ArmId, TrialId), (axon_loop_contracts::AttemptId, Ref)> =
        assignment
            .trials
            .iter()
            .map(|t| {
                (
                    (t.task_id.clone(), t.arm_id.clone(), t.trial_id.clone()),
                    (t.attempt_id.clone(), t.policy_ref.clone()),
                )
            })
            .collect();
    let requested: BTreeMap<(TaskId, ArmId, TrialId), Ref> = r
        .assigned
        .iter()
        .map(|a| {
            (
                (a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone()),
                a.policy_ref.clone(),
            )
        })
        .collect();
    if requested.len() != issued.len()
        || requested
            .iter()
            .any(|(k, p)| issued.get(k).map(|(_, ip)| ip) != Some(p))
    {
        return Err(refused(format!(
            "the evaluation's assignment is not the one journalled before execution \
             ({assignment_ref}): the population is never chosen after outcomes exist"
        )));
    }
    let assigned_ms = tx.recorded_ms(assigned_seq);
    let trial_ids: BTreeSet<&TrialId> = r.assigned.iter().map(|a| &a.trial_id).collect();
    check_population(&tx, &frozen, &r.assigned)?;
    let assigned_keys: BTreeSet<(TaskId, ArmId, TrialId)> = requested.keys().cloned().collect();
    for prior in tx.evaluations_in(&r.scope) {
        let old: EvaluationRecord = tx.store.get_record("evaluations", &prior)?;
        if let Some(t) = old
            .arms
            .iter()
            .flat_map(|a| &a.trials)
            .find(|t| trial_ids.contains(&t.trial_id))
        {
            return Err(refused(format!(
                "trial id {} was already evaluated in {prior}: trial ids are unique for the experiment's lifetime",
                t.trial_id
            )));
        }
    }

    // ADR-001 §8: evaluation reads what the operator store ADMITTED through
    // intake, never bytes the request merely carries. Every intake check
    // (context join, ack, spend conversion, the verification evidence) sits
    // on that path; a delivered episode with no EpisodeIntake record in this
    // scope bypassed all of them.
    let intaken: BTreeMap<Ref, crate::intake::IntakeRecord> = tx
        .entries()
        .iter()
        .filter_map(|e| match &e.event {
            // Every issued trial was intaken AFTER its assignment: `plan::assign`
            // refuses a population any of whose trials was already intaken.
            Event::EpisodeIntake { scope, intake } if scope == &r.scope => {
                Some((intake.episode_ref.clone(), (**intake).clone()))
            }
            _ => None,
        })
        .collect();
    // Safety findings recorded so far: fixed into the record, so an admission
    // re-derives from exactly what the evaluation saw.
    let safety = crate::safety::states(&tx, &r.scope);
    let mut delivered: BTreeMap<(TaskId, ArmId, TrialId), Delivered> = BTreeMap::new();
    let mut evidence = BTreeSet::new();
    for (i, t) in r.trials.iter().enumerate() {
        let ep: LoopEpisode = contract_from_value(&format!("trials[{i}].episode"), &t.episode)?;
        let ctx: ExecutionContextReceipt =
            contract_from_value(&format!("trials[{i}].context"), &t.context)?;
        // A D12 trial has no execution documents; delivering some anyway is a
        // contradiction, never a second source of truth.
        let acf = if is_d12(&ep) {
            if [&t.acf_request, &t.acf_receipt, &t.projection]
                .iter()
                .any(|v| !v.is_null())
            {
                return Err(refused(format!(
                    "trials[{i}] ({}): the episode names MiCode's not-produced execution markers \
                     (D12), so acf_request, acf_receipt and projection must be null",
                    ep.identity.trial_id
                )));
            }
            None
        } else {
            Some((
                contract_from_value::<ComputeRequest>(
                    &format!("trials[{i}].acf_request"),
                    &t.acf_request,
                )?,
                contract_from_value::<ExecutionReceipt>(
                    &format!("trials[{i}].acf_receipt"),
                    &t.acf_receipt,
                )?,
                contract_from_value::<PolicyProjection>(
                    &format!("trials[{i}].projection"),
                    &t.projection,
                )?,
            ))
        };
        let key = (
            ep.identity.task_id.clone(),
            ep.identity.arm_id.clone(),
            ep.identity.trial_id.clone(),
        );
        if !assigned_keys.contains(&key) {
            return Err(refused(format!(
                "trials[{i}] ({}) was never assigned: an unassigned result cannot join the population",
                ep.identity.trial_id
            )));
        }
        let now = crate::now_ms();
        if ctx.created_ms > now {
            return Err(refused(format!(
                "trials[{i}] ({}) claims a preflight in the future ({} ms > now {now} ms)",
                ep.identity.trial_id, ctx.created_ms
            )));
        }
        // PROTECTED class: the observer-signed preflight time must follow the
        // assignment (in a development evaluation it is a producer claim).
        if frozen.evaluation_class == crate::plan::EvaluationClass::Protected
            && ctx.created_ms < assigned_ms
        {
            return Err(refused(format!(
                "trials[{i}] ({}) was preflighted at {} ms, before its assignment was issued at \
                 {assigned_ms} ms: a protected trial runs only after it is issued",
                ep.identity.trial_id, ctx.created_ms
            )));
        }
        if ctx.created_ms < frozen.freeze_ms {
            return Err(refused(format!(
                "trials[{i}] ({}) was preflighted at {} ms, before the plan froze at {} ms: outcomes that predate the rule cannot be judged by it",
                ep.identity.trial_id, ctx.created_ms, frozen.freeze_ms
            )));
        }
        let ep_ref = digest(&ep)?;
        evidence.insert(ep_ref.clone());
        evidence.insert(digest(&ctx)?);
        if let Some((req, rcpt, _)) = &acf {
            evidence.insert(digest(req)?);
            evidence.insert(digest(rcpt)?);
        }
        if delivered
            .insert(
                key,
                Delivered {
                    ep,
                    ep_ref,
                    ctx,
                    acf,
                    verification: [
                        t.verification_request.clone(),
                        t.verification_receipt.clone(),
                        t.verification_attestation.clone(),
                    ],
                    ctx_sig: t.context_signature.clone(),
                    psv: t.verification_psv_evidence.clone(),
                    acf_att: t.acf_attestation.clone(),
                },
            )
            .is_some()
        {
            return Err(refused(format!("trials[{i}]: trial delivered twice")));
        }
    }

    let mut arms: BTreeMap<ArmId, ArmResult> = BTreeMap::new();
    let mut usages: BTreeMap<ArmId, Vec<(axon_loop_contracts::Usage, Option<EpisodeStatus>)>> =
        BTreeMap::new();
    let mut exec_usages: BTreeMap<ArmId, Vec<(axon_loop_contracts::Usage, Option<EpisodeStatus>)>> =
        BTreeMap::new();
    for a in &r.assigned {
        let arm = arms.entry(a.arm_id.clone()).or_insert_with(|| ArmResult {
            arm_id: a.arm_id.clone(),
            policy_ref: a.policy_ref.clone(),
            assigned: 0,
            verified_pass: 0,
            fail: 0,
            unknown: 0,
            missing: 0,
            unknown_kinds: BTreeMap::new(),
            trials: Vec::new(),
            economics: Summary {
                records: 0,
                missing_records: 0,
                by_currency: Vec::new(),
            },
        });
        arm.assigned += 1;
        let key = (a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone());
        let mut authenticated = None;
        let (outcome, reason, kind, ep_ref, role) = match delivered.get(&key) {
            None => {
                arm.missing += 1;
                (
                    Outcome::Unknown,
                    "missing: no episode delivered".to_string(),
                    None,
                    None,
                    None,
                )
            }
            Some(d) => {
                usages
                    .entry(a.arm_id.clone())
                    .or_default()
                    .push((d.ep.usage.clone(), Some(d.ep.status)));
                // The Fabric EXECUTION is a cost component of its own: under
                // D10 unknown, with its reservation as liability. Omitting it
                // reported a partial cost as the arm's Known total (review
                // wf_d788c05a-be2). A D12 trial ran no Fabric execution.
                if let Some((req, rcpt, _)) = &d.acf {
                    exec_usages.entry(a.arm_id.clone()).or_default().push((
                        tel::execution_component(&d.ep.usage, req, rcpt)?,
                        Some(d.ep.status),
                    ));
                }
                let policy = &policies[&a.policy_ref];
                // NS4p/NS4w: a subject (request subject issuer, arm proposer)
                // never observes its own preflight, even if the operator
                // listed it as an observer — the verifier rule, mirrored.
                let ctx_check = if let Err(e) = intake_join(&intaken, d) {
                    Err(e)
                } else if d.ep.scope != r.scope || d.ctx.scope != r.scope {
                    // Cross-tenant: evidence minted for another scope never
                    // joins this one, whatever its ids say.
                    Err(format!(
                        "cross-tenant evidence: episode scope {:?} / context scope {:?}, evaluation scope {:?}",
                        d.ep.scope, d.ctx.scope, r.scope
                    ))
                } else if subjects.contains(&d.ctx.observed_issuer_ref) {
                    Err(format!(
                        "observer {} is a subject issuer: no self-observation",
                        d.ctx.observed_issuer_ref
                    ))
                } else if let Err(e) = authenticated_context(&config, frozen.evaluation_class, d) {
                    Err(e)
                } else {
                    check_paired_trial_context(&d.ctx, now, epoch, &observers)
                        .map_err(|e| e.to_string())
                };
                let issued_attempt = &issued[&key].0;
                let (o, why, kind) = if &d.ep.identity.attempt_id != issued_attempt {
                    // Only the issued attempt counts: another attempt of the
                    // same trial is never swapped in for it (best-of-k).
                    unknown(
                        UnknownKind::Unbound,
                        format!(
                            "unbound: attempt {} is not this trial's issued attempt \
                             {issued_attempt}; only the issued attempt counts",
                            d.ep.identity.attempt_id
                        ),
                    )
                } else {
                    match ctx_check {
                        Err(e) => unknown(
                            UnknownKind::Unbound,
                            format!(
                            "context not admissible (TASK_NOT_STARTED evidence, never a pass): {e}"
                        ),
                        ),
                        Ok(()) => judge(
                            d,
                            policy,
                            &a.policy_ref,
                            &Bench {
                                epoch,
                                freeze_ms: frozen.freeze_ms,
                                verifiers: &verifiers,
                                config: &config,
                                subjects: &subjects,
                                class: frozen.evaluation_class,
                            },
                            &mut authenticated,
                        ),
                    }
                };
                (o, why, kind, Some(d.ep_ref.clone()), Some(d.ep.corpus_role))
            }
        };
        match outcome {
            Outcome::VerifiedPass => arm.verified_pass += 1,
            Outcome::Fail => arm.fail += 1,
            Outcome::Unknown => arm.unknown += 1,
        }
        if let Some(k) = kind {
            *arm.unknown_kinds.entry(k).or_default() += 1;
        }
        arm.trials.push(TrialResult {
            task_id: a.task_id.clone(),
            trial_id: a.trial_id.clone(),
            outcome,
            reason,
            unknown_kind: kind,
            episode_ref: ep_ref,
            corpus_role: role,
            // Only a counted verdict cites its evidence. Load-bearing since
            // D12 judging (G01-r22-unknown-outcome): a D12 verdict is
            // authenticated and THEN demoted to Unbound, and must not cite
            // evidence as if it had counted.
            verification: authenticated
                .filter(|_| matches!(outcome, Outcome::VerifiedPass | Outcome::Fail)),
            safety: safety
                .get(&(
                    a.task_id.as_str().to_string(),
                    a.arm_id.as_str().to_string(),
                    a.trial_id.as_str().to_string(),
                ))
                .copied()
                .unwrap_or_default(),
            context_observer_ref: delivered
                .get(&key)
                .filter(|_| matches!(outcome, Outcome::VerifiedPass | Outcome::Fail))
                .map(|d| d.ctx.observed_issuer_ref.clone()),
            // A counted protected trial passed `authenticated_context` under
            // the CURRENT observer key; record which, for re-derivation.
            context_signed_by: delivered
                .get(&key)
                .filter(|_| {
                    frozen.evaluation_class == crate::plan::EvaluationClass::Protected
                        && matches!(outcome, Outcome::VerifiedPass | Outcome::Fail)
                })
                .and_then(|d| {
                    let who = &d.ctx.observed_issuer_ref;
                    config
                        .observer_keys
                        .get(who)
                        .and_then(|pk| axon_loop_contracts::attestation::key_id_of_hex(pk))
                        .map(|key_id| SignedBy {
                            issuer_ref: who.clone(),
                            key_id,
                        })
                }),
        });
    }
    for (id, arm) in arms.iter_mut() {
        let us = usages.remove(id).unwrap_or_default();
        let ex = exec_usages.remove(id).unwrap_or_default();
        arm.economics = tel::summarize_with_execution(
            us.iter().map(|(u, s)| (u, *s)),
            ex.iter().map(|(u, s)| (u, *s)),
            arm.missing,
        )?;
        // ADR-001 §5: every assigned trial is exactly one of pass, fail, a
        // KIND of unknown, or missing. A trial counted twice or not at all is
        // refused here, before anything is written.
        let kinds: u64 = arm.unknown_kinds.values().sum();
        if arm.assigned != arm.verified_pass + arm.fail + kinds + arm.missing
            || arm.unknown != kinds + arm.missing
        {
            return Err(refused(format!(
                "evaluation invariant violated for arm {id}: assigned {} ≠ pass {} + fail {} + \
                 unknown by kind {kinds} + missing {}",
                arm.assigned, arm.verified_pass, arm.fail, arm.missing
            )));
        }
    }

    let mut subject_issuers: Vec<OpaqueRef> = subjects.into_iter().collect();
    subject_issuers.sort();
    let rec = EvaluationRecord {
        schema: EvaluationSchema,
        scope: r.scope.clone(),
        experiment_id: r.experiment_id.clone(),
        plan_ref: frozen.plan_ref.clone(),
        freeze_seq: frozen.freeze_seq,
        authority_epoch: epoch,
        evaluator_ref: r.evaluator_ref.clone(),
        trusted_verifiers: verifiers.into_iter().collect(),
        subject_issuers,
        arms: arms.into_values().collect(),
        evaluation_class: frozen.evaluation_class,
        evidence_refs: evidence.into_iter().collect(),
    };
    for p in policies.values() {
        store.put_cas("policies", p)?;
    }
    // What a counted protected trial cites, so admission can re-verify it
    // from the documents: the PSV bundle, and the execution leg's request,
    // receipt and Fabric attestation.
    for t in &r.trials {
        if !t.verification_psv_evidence.is_null() {
            store.put_cas("fabric-psv-evidence", &t.verification_psv_evidence)?;
        }
        if !t.acf_attestation.is_null() {
            store.put_cas("acf-requests", &t.acf_request)?;
            store.put_cas("acf-receipts", &t.acf_receipt)?;
            store.put_cas("acf-attestations", &t.acf_attestation)?;
        }
    }
    let eref = store.put_cas("evaluations", &rec)?;
    if tx.evaluation_event(&eref).is_none() {
        tx.append(Event::Evaluation {
            scope: rec.scope.clone(),
            experiment_id: rec.experiment_id.clone(),
            evaluation_ref: eref.clone(),
            freeze_seq: frozen.freeze_seq,
            authority_epoch: epoch,
        })?;
    }
    Ok((rec, eref))
}

/// The delivered episode is one intake RECORDED in this scope. Its bytes are
/// then exactly the intaken bytes (the record is keyed by their digest), and
/// those bytes bind everything else intake joined: the context by
/// `context_ref`, the verification check by `verifier_ref` and
/// `evidence_refs` — which evaluation's own joins re-check against what is
/// delivered. So existence is the whole test; comparing the other refs again
/// here could never fire.
fn intake_join(
    intaken: &BTreeMap<Ref, crate::intake::IntakeRecord>,
    d: &Delivered,
) -> std::result::Result<(), String> {
    if intaken.contains_key(&d.ep_ref) {
        Ok(())
    } else {
        Err(format!(
            "not intaken: episode {} has no intake record in this scope, so none of intake's \
             checks ever ran on it",
            d.ep_ref
        ))
    }
}

/// What every trial of one evaluation is judged against: fixed at the start of
/// `evaluate`, identical for every arm.
struct Bench<'a> {
    epoch: AuthorityEpoch,
    /// When the plan froze: a verdict ATTESTED before it does not count.
    freeze_ms: u64,
    verifiers: &'a BTreeSet<OpaqueRef>,
    config: &'a crate::store::Config,
    subjects: &'a BTreeSet<OpaqueRef>,
    class: crate::plan::EvaluationClass,
}

fn judge(
    d: &Delivered,
    policy: &PolicyEnvelope,
    policy_ref: &Ref,
    bench: &Bench<'_>,
    authenticated: &mut Option<VerificationEvidence>,
) -> Judged {
    let Bench {
        epoch,
        freeze_ms,
        verifiers,
        config,
        subjects,
        class,
    } = *bench;
    // The subject set for THIS trial: the evaluation's (request subjects, arm
    // proposers) plus the trial's own observer — the set intake judged by, so
    // the two doors cannot disagree about who may verify.
    let subjects: &BTreeSet<OpaqueRef> = &subjects
        .iter()
        .cloned()
        .chain([d.ctx.observed_issuer_ref.clone()])
        .collect();
    if &d.ep.policy_ref != policy_ref {
        return unknown(
            UnknownKind::Unbound,
            "unbound: episode ran a different policy than its arm",
        );
    }
    if let Err(e) = bind_episode(&d.ep, policy, &d.ctx, epoch, verifiers, subjects) {
        return unknown(UnknownKind::Unbound, format!("unbound episode: {e}"));
    }
    if let Some((req, rcpt, proj)) = &d.acf {
        if let Err(e) = bind_acf(&d.ep, req, rcpt, proj) {
            return unknown(UnknownKind::Unbound, format!("unbound ACF evidence: {e}"));
        }
        // The trial's cost is stated in its execution's currency; a relabelled
        // usage would move the arm's liability into another unit.
        if d.ep.usage.currency.as_str() != req.limits.currency_code.as_str() {
            return unknown(
                UnknownKind::Unbound,
                format!(
                    "unbound: usage currency {} is not its execution request's {}",
                    d.ep.usage.currency.as_str(),
                    req.limits.currency_code.as_str()
                ),
            );
        }
    }
    // ADR-001 D3: a protected evaluation counts a trial only from protected
    // backends. The two legs prove DIFFERENT things (re-audit 3):
    // * the VERIFICATION receipt's backend is inside the receipt the verifier
    //   signed (the attestation binds its digest), so a counted protected
    //   verdict is authenticated as having run on a protected backend;
    // * the EXECUTION receipt's backend is a producer claim — nothing signs an
    //   execution receipt (under D12 MiCode produces none) — so it is only a
    //   FILTER that can make fewer trials count, never evidence that one ran
    //   there.
    // An absent verification receipt is skipped here and refused below as
    // unauthenticated whenever the episode cites a verdict.
    if class == crate::plan::EvaluationClass::Protected {
        let Some((areq, rcpt, _)) = &d.acf else {
            return unknown(
                UnknownKind::Unverifiable,
                "D12 local execution (no Fabric execution receipt) is ineligible for a protected \
                 evaluation (ADR-001 D3)",
            );
        };
        let legs = [
            ("execution", Some(rcpt.backend_profile_ref.as_str())),
            (
                "verification",
                d.verification[1]["backend_profile_ref"].as_str(),
            ),
        ];
        for (leg, b) in legs {
            if let Some(b) = b.filter(|b| !axon_loop_contracts::PROTECTED_PROFILES.contains(b)) {
                return unknown(
                    UnknownKind::Unverifiable,
                    format!(
                        "development {leg} backend {b} is ineligible for a protected evaluation \
                         (ADR-001 D3)"
                    ),
                );
            }
        }
        // …and the execution leg's protected backend must be ATTESTED by
        // Fabric under the operator's verifier root, not merely named in the
        // receipt (dev review round wf_336353cb-a2b, PSV-7).
        if let Err(e) = verify_execution(&d.acf_att, areq, rcpt, config) {
            return unknown(
                UnknownKind::Unverifiable,
                format!(
                    "the execution leg is not attested as a protected execution: {e} (ADR-001 D3)"
                ),
            );
        }
        // M4: a verdict counts in a protected evaluation only as PROTECTED
        // evidence — its receipt's class, its joins, its guest interpreter
        // (v022-psv-protocol.md §9). A backend name alone is not enough, and
        // anything less is Unverifiable here, never silently development.
        if matches!(
            d.ep.verification.result,
            VerificationResult::Passed | VerificationResult::Failed
        ) {
            let joined = serde_json::from_value::<ComputeRequest>(d.verification[0].clone())
                .map_err(|e| e.to_string())
                .and_then(|q| {
                    serde_json::from_value::<ExecutionReceipt>(d.verification[1].clone())
                        .map_err(|e| e.to_string())
                        .map(|r| (q, r))
                })
                .and_then(|(q, r)| axon_loop_contracts::protected_evidence::check(&q, &r));
            if let Err(e) = joined {
                return unknown(
                    UnknownKind::Unverifiable,
                    format!("not protected evidence: {e}"),
                );
            }
        }
    }
    let v = &d.ep.verification;
    // A verdict is evidence only if its VERIFICATION evidence joins and is
    // authenticated — the same rule intake applies, over the verification
    // check's own documents. An unauthenticated FAILURE is refused as surely
    // as a pass: forged failures could otherwise sink an arm. A CITED unknown
    // is authenticated too, so the kind it reports comes from the receipt the
    // verifier signed (a timed-out or canceled check), not from the subject.
    let mut check_receipt: Option<ExecutionReceipt> = None;
    if matches!(
        v.result,
        VerificationResult::Passed | VerificationResult::Failed
    ) || (v.result == VerificationResult::Unknown && v.verifier_ref.is_some())
    {
        let [req, rc, att] = &d.verification;
        let text = |x: &Value| (!x.is_null()).then(|| x.to_string());
        let checked = match (text(req), text(rc)) {
            (Some(q), Some(r)) => crate::intake::verify_check_evidence(
                &d.ep,
                &q,
                &r,
                text(att).as_deref(),
                config,
                subjects,
                text(&d.psv).as_deref(),
            )
            .and_then(|(q, r, a, key_id)| {
                // Authenticated by `verify` above (it is in the signed bytes).
                let issued = axon_loop_contracts::attestation::issued_ms(&a)
                    .ok_or_else(|| refused("an authenticated attestation states no issued_ms"))?;
                Ok((
                    r.clone(),
                    VerificationEvidence {
                        request_ref: digest(&q)?,
                        receipt_ref: digest(&r)?,
                        attestation_ref: digest_value(&a)?,
                        issuer_ref: v
                            .issuer_ref
                            .clone()
                            .ok_or_else(|| refused("an authenticated verdict names no issuer"))?,
                        key_id,
                        psv_evidence_ref: if d.psv.is_null() {
                            None
                        } else {
                            Some(digest_value(&d.psv)?)
                        },
                        execution_attestation_ref: if d.acf_att.is_null() {
                            None
                        } else {
                            Some(digest_value(&d.acf_att)?)
                        },
                    },
                    issued,
                ))
            })
            .map_err(|e| (UnknownKind::Unverifiable, e.to_string())),
            _ => Err((
                UnknownKind::MissingEvidence,
                "the verification check's request and receipt were not delivered".into(),
            )),
        };
        match checked {
            Err((kind, e)) => return unknown(kind, format!("unauthenticated verification: {e}")),
            Ok((_, ev, issued)) if issued < freeze_ms => {
                return unknown(
                    UnknownKind::Unverifiable,
                    format!(
                        "verdict attested at {issued} ms, before the plan froze at {freeze_ms} ms \
                         (verifier {}): an outcome that predates the rule cannot be judged by it",
                        ev.issuer_ref
                    ),
                )
            }
            Ok((r, ev, _)) => {
                check_receipt = Some(r);
                // Only a verdict is kept as the evidence a count may rest on;
                // whether it counts is decided below (a D12 verdict does not).
                if v.result != VerificationResult::Unknown {
                    *authenticated = Some(ev);
                }
            }
        }
    }
    let d12 = d.acf.is_none();
    // How the run ended, where that is known: the execution receipt, or for a
    // D12 trial the episode (MiCode folds any other ending into
    // outcome_unknown). Informational: a kind never makes a trial count.
    let run_end = match &d.acf {
        Some((_, rcpt, _)) => match rcpt.status {
            ReceiptStatus::TimedOut => Some(UnknownKind::TimedOut),
            ReceiptStatus::Canceled => Some(UnknownKind::Cancelled),
            _ => None,
        },
        None => (d.ep.status == EpisodeStatus::Cancelled).then_some(UnknownKind::Cancelled),
    };
    // What the PRODUCER states about an uncited verification (MiCode's not-run
    // reason marker): which non-success it is, never a verdict. Below how the
    // run ended (a cancellation stays a cancellation).
    let reason = crate::intake::micode_not_run_reason(v);
    let stated = reason.map(|r| match r {
        "run_timed_out" | "check_timed_out" => UnknownKind::TimedOut,
        "check_evidence_missing" => UnknownKind::MissingEvidence,
        "check_unverifiable" => UnknownKind::Unverifiable,
        _ => UnknownKind::NotRun,
    });
    let stated_why = reason
        .map(|r| format!("; the producer states {r}"))
        .unwrap_or_default();
    // A run that did not finish has no verdict that counts: a check over what
    // a cancelled or timed-out run left behind is not the trial's outcome, and
    // counting it would average the interruption away. (A pass is already
    // impossible: the contract requires `completed`; this is the failure.)
    if let (Some(k), VerificationResult::Passed | VerificationResult::Failed) = (run_end, v.result)
    {
        return unknown(
            k,
            format!(
                "the run ended {k:?} (status {:?}): a verdict about its output does not count",
                d.ep.status
            ),
        );
    }
    match v.result {
        VerificationResult::Passed => {
            let issuer_ok = v
                .issuer_ref
                .as_ref()
                .is_some_and(|i| verifiers.contains(i) && !subjects.contains(i));
            if v.matched_checks == 0 {
                unknown(
                    UnknownKind::Unmatched,
                    "vacuous: passed with zero matched checks",
                )
            } else if d12 {
                unknown(UnknownKind::Unbound, D12_NOT_COUNTED)
            } else if !issuer_ok {
                unknown(
                    UnknownKind::Unverifiable,
                    "untrusted or subject verifier cannot establish a pass",
                )
            } else {
                (
                    Outcome::VerifiedPass,
                    format!("independently verified ({} checks)", v.matched_checks),
                    None,
                )
            }
        }
        VerificationResult::Failed if d12 => unknown(UnknownKind::Unbound, D12_NOT_COUNTED),
        VerificationResult::Failed => (Outcome::Fail, "verifier reported failure".into(), None),
        VerificationResult::NotRun => unknown(
            run_end.or(stated).unwrap_or(UnknownKind::NotRun),
            format!(
                "verification not run (status {:?}{})",
                d.ep.status, stated_why
            ),
        ),
        VerificationResult::Unknown => match check_receipt {
            // The check itself did not reach a verdict, as the verifier signed.
            Some(rc) => unknown(
                match rc.status {
                    ReceiptStatus::TimedOut => UnknownKind::TimedOut,
                    ReceiptStatus::Canceled => UnknownKind::Cancelled,
                    ReceiptStatus::Completed if rc.matched_checks.unwrap_or(0) == 0 => {
                        UnknownKind::Unmatched
                    }
                    _ => UnknownKind::MissingEvidence,
                },
                format!(
                    "the verification check ended {:?} with verification {:?} and {} matched \
                     (status {:?})",
                    rc.status,
                    rc.verification,
                    rc.matched_checks.unwrap_or(0),
                    d.ep.status
                ),
            ),
            None => unknown(
                run_end.or(stated).unwrap_or(UnknownKind::MissingEvidence),
                format!(
                    "verification unknown (status {:?}{})",
                    d.ep.status, stated_why
                ),
            ),
        },
    }
}

const D12_NOT_COUNTED: &str = "D12: the verdict is authenticated, but the execution it judges ran \
     under local MiCode authority with no Fabric execution receipt, so nothing binds it to this \
     arm: not counted";

/// AB9/AB10 — the population: EXACTLY the frozen task manifest × both arms ×
/// the planned repetitions, each trial once, one policy per arm, only the
/// plan's arms. Checked when the population is ISSUED (`plan::assign`, before
/// execution) and again when it is evaluated.
pub(crate) fn check_population(
    tx: &Tx,
    frozen: &crate::plan::Frozen,
    assigned: &[Assignment],
) -> Result<()> {
    let plan_arms: BTreeSet<&Ref> = [
        frozen.plan.incumbent_policy_ref.as_ref().expect("frozen"),
        frozen.plan.candidate_policy_ref.as_ref().expect("frozen"),
    ]
    .into();
    let mut assigned_keys = BTreeSet::new();
    for a in assigned {
        if !assigned_keys.insert((a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone())) {
            return Err(refused(format!("trial {} assigned twice", a.trial_id)));
        }
        if !plan_arms.contains(&a.policy_ref) {
            return Err(refused(format!(
                "assigned arm policy {} not supplied",
                a.policy_ref
            )));
        }
    }
    let arm_policy: BTreeMap<&ArmId, &Ref> = assigned
        .iter()
        .map(|a| (&a.arm_id, &a.policy_ref))
        .collect();
    for a in assigned {
        if arm_policy[&a.arm_id] != &a.policy_ref {
            return Err(refused(format!("arm {} assigned two policies", a.arm_id)));
        }
    }
    let manifest = crate::tasks::resolve(
        tx,
        &frozen.plan.scope,
        frozen.plan.task_manifest_ref.as_ref().expect("frozen"),
    )?;
    let reps = frozen.plan.repetitions.expect("frozen");
    let mut per_arm_task: BTreeMap<(&Ref, &TaskId), u64> = BTreeMap::new();
    let mut trial_ids: BTreeSet<&TrialId> = BTreeSet::new();
    for a in assigned {
        if !trial_ids.insert(&a.trial_id) {
            return Err(refused(format!(
                "trial id {} is assigned twice in this evaluation",
                a.trial_id
            )));
        }
        *per_arm_task.entry((&a.policy_ref, &a.task_id)).or_default() += 1;
    }
    let arms_seen: BTreeSet<&Ref> = assigned.iter().map(|a| &a.policy_ref).collect();
    if arms_seen.len() != 2 {
        return Err(refused(
            "the evaluation must assign both the incumbent and the candidate arm",
        ));
    }
    for arm in &arms_seen {
        let tasks: BTreeSet<TaskId> = per_arm_task
            .keys()
            .filter(|(p, _)| p == arm)
            .map(|(_, t)| (*t).clone())
            .collect();
        if tasks != manifest.task_set() {
            return Err(refused(format!(
                "arm {arm} is assigned {} task(s), but the frozen task manifest has {}: \
                 the evaluation must cover exactly the manifest (no cherry-picked subset, no extras)",
                tasks.len(),
                manifest.tasks.len()
            )));
        }
    }
    if let Some(((arm, task), n)) = per_arm_task.iter().find(|(_, n)| **n != reps) {
        return Err(refused(format!(
            "arm {arm} task {task} is assigned {n} time(s), plan repetitions = {reps}"
        )));
    }
    Ok(())
}

/// Load a stored evaluation.
pub fn load(store: &Store, r: &Ref) -> Result<EvaluationRecord> {
    store.get_record("evaluations", r)
}

/// Load an evaluation only if the ledger journalled it (so a CAS file written
/// by hand is never evidence).
pub(crate) fn load_journalled(tx: &Tx, r: &Ref) -> Result<(u64, EvaluationRecord)> {
    let (seq, _) = tx.evaluation_event(r).ok_or_else(|| {
        refused(format!(
            "evaluation {r} was never journalled by `evl evaluate`"
        ))
    })?;
    Ok((seq, tx.store.get_record("evaluations", r)?))
}

impl EvaluationRecord {
    pub fn arm_for_policy(&self, p: &Ref) -> Result<&ArmResult> {
        let mut it = self.arms.iter().filter(|a| &a.policy_ref == p);
        let a = it
            .next()
            .ok_or_else(|| refused(format!("evaluation has no arm for policy {p}")))?;
        if it.next().is_some() {
            return Err(LoopError::Refused(format!("two arms ran policy {p}")));
        }
        Ok(a)
    }
}
