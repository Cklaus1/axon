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
    ExecutionReceipt, LoopEpisode, OpaqueRef, PolicyEnvelope, PolicyProjection, Ref, Scope, TaskId,
    TrialId, VerificationResult,
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
    /// receipt, and the verifier's `acf-receipt-attestation/1` over them —
    /// never the trial's execution documents. A verdict (passed or failed)
    /// counts only if [`crate::intake::verify_check_evidence`] accepts them,
    /// exactly as intake does; otherwise the trial is `Unknown`.
    #[serde(default)]
    pub verification_request: Value,
    #[serde(default)]
    pub verification_receipt: Value,
    #[serde(default)]
    pub verification_attestation: Value,
}

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

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialResult {
    pub task_id: TaskId,
    pub trial_id: TrialId,
    pub outcome: Outcome,
    pub reason: String,
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

struct Delivered {
    ep: LoopEpisode,
    ep_ref: Ref,
    ctx: ExecutionContextReceipt,
    req: ComputeRequest,
    rcpt: ExecutionReceipt,
    proj: PolicyProjection,
    verification: [Value; 3],
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

    let mut assigned_keys = BTreeSet::new();
    for a in &r.assigned {
        if !assigned_keys.insert((a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone())) {
            return Err(refused(format!("trial {} assigned twice", a.trial_id)));
        }
        if !policies.contains_key(&a.policy_ref) {
            return Err(refused(format!(
                "assigned arm policy {} not supplied",
                a.policy_ref
            )));
        }
    }
    let arm_policy: BTreeMap<&ArmId, &Ref> = r
        .assigned
        .iter()
        .map(|a| (&a.arm_id, &a.policy_ref))
        .collect();
    for a in &r.assigned {
        if arm_policy[&a.arm_id] != &a.policy_ref {
            return Err(refused(format!("arm {} assigned two policies", a.arm_id)));
        }
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
    let manifest = crate::tasks::resolve(
        &tx,
        &frozen.plan.scope,
        frozen.plan.task_manifest_ref.as_ref().expect("frozen"),
    )?;
    let reps = frozen.plan.repetitions.expect("frozen");
    let mut per_arm_task: BTreeMap<(&Ref, &TaskId), u64> = BTreeMap::new();
    let mut trial_ids: BTreeSet<&TrialId> = BTreeSet::new();
    for a in &r.assigned {
        if !trial_ids.insert(&a.trial_id) {
            return Err(refused(format!(
                "trial id {} is assigned twice in this evaluation",
                a.trial_id
            )));
        }
        *per_arm_task.entry((&a.policy_ref, &a.task_id)).or_default() += 1;
    }
    let arms_seen: BTreeSet<&Ref> = r.assigned.iter().map(|a| &a.policy_ref).collect();
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
            Event::EpisodeIntake { scope, intake } if scope == &r.scope => {
                Some((intake.episode_ref.clone(), (**intake).clone()))
            }
            _ => None,
        })
        .collect();
    let mut delivered: BTreeMap<(TaskId, ArmId, TrialId), Delivered> = BTreeMap::new();
    let mut evidence = BTreeSet::new();
    for (i, t) in r.trials.iter().enumerate() {
        let ep: LoopEpisode = contract_from_value(&format!("trials[{i}].episode"), &t.episode)?;
        let ctx: ExecutionContextReceipt =
            contract_from_value(&format!("trials[{i}].context"), &t.context)?;
        let req: ComputeRequest =
            contract_from_value(&format!("trials[{i}].acf_request"), &t.acf_request)?;
        let rcpt: ExecutionReceipt =
            contract_from_value(&format!("trials[{i}].acf_receipt"), &t.acf_receipt)?;
        let proj: PolicyProjection =
            contract_from_value(&format!("trials[{i}].projection"), &t.projection)?;
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
        if ctx.created_ms < frozen.freeze_ms {
            return Err(refused(format!(
                "trials[{i}] ({}) was preflighted at {} ms, before the plan froze at {} ms: outcomes that predate the rule cannot be judged by it",
                ep.identity.trial_id, ctx.created_ms, frozen.freeze_ms
            )));
        }
        let ep_ref = digest(&ep)?;
        for d in [&ep_ref, &digest(&ctx)?, &digest(&req)?, &digest(&rcpt)?] {
            evidence.insert(d.clone());
        }
        if delivered
            .insert(
                key,
                Delivered {
                    ep,
                    ep_ref,
                    ctx,
                    req,
                    rcpt,
                    proj,
                    verification: [
                        t.verification_request.clone(),
                        t.verification_receipt.clone(),
                        t.verification_attestation.clone(),
                    ],
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
    for a in &r.assigned {
        let arm = arms.entry(a.arm_id.clone()).or_insert_with(|| ArmResult {
            arm_id: a.arm_id.clone(),
            policy_ref: a.policy_ref.clone(),
            assigned: 0,
            verified_pass: 0,
            fail: 0,
            unknown: 0,
            missing: 0,
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
        let (outcome, reason, ep_ref, role) = match delivered.get(&key) {
            None => {
                arm.missing += 1;
                (
                    Outcome::Unknown,
                    "missing: no episode delivered".to_string(),
                    None,
                    None,
                )
            }
            Some(d) => {
                usages
                    .entry(a.arm_id.clone())
                    .or_default()
                    .push((d.ep.usage.clone(), Some(d.ep.status)));
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
                } else {
                    check_paired_trial_context(&d.ctx, now, epoch, &observers)
                        .map_err(|e| e.to_string())
                };
                let (o, why) = match ctx_check {
                    Err(e) => (
                        Outcome::Unknown,
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
                            verifiers: &verifiers,
                            config: &config,
                            subjects: &subjects,
                            class: frozen.evaluation_class,
                        },
                        &mut authenticated,
                    ),
                };
                (o, why, Some(d.ep_ref.clone()), Some(d.ep.corpus_role))
            }
        };
        match outcome {
            Outcome::VerifiedPass => arm.verified_pass += 1,
            Outcome::Fail => arm.fail += 1,
            Outcome::Unknown => arm.unknown += 1,
        }
        arm.trials.push(TrialResult {
            task_id: a.task_id.clone(),
            trial_id: a.trial_id.clone(),
            outcome,
            reason,
            episode_ref: ep_ref,
            corpus_role: role,
            // Only a counted verdict cites its evidence: an outcome demoted
            // after authentication (a vacuous or untrusted pass) cites none.
            verification: authenticated
                .filter(|_| matches!(outcome, Outcome::VerifiedPass | Outcome::Fail)),
        });
    }
    for (id, arm) in arms.iter_mut() {
        let us = usages.remove(id).unwrap_or_default();
        arm.economics = tel::summarize_with_missing(us.iter().map(|(u, s)| (u, *s)), arm.missing)?;
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
) -> (Outcome, String) {
    let Bench {
        epoch,
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
        return (
            Outcome::Unknown,
            "unbound: episode ran a different policy than its arm".into(),
        );
    }
    if let Err(e) = bind_episode(&d.ep, policy, &d.ctx, epoch, verifiers, subjects) {
        return (Outcome::Unknown, format!("unbound episode: {e}"));
    }
    if let Err(e) = bind_acf(&d.ep, &d.req, &d.rcpt, &d.proj) {
        return (Outcome::Unknown, format!("unbound ACF evidence: {e}"));
    }
    // ADR-001 D3: a protected evaluation counts a trial only if every receipt
    // it rests on came from a protected backend. The local interpreter is
    // development-only — its results are recorded, never protected evidence.
    if class == crate::plan::EvaluationClass::Protected {
        let backends = [
            Some(d.rcpt.backend_profile_ref.as_str().to_string()),
            d.verification[1]["backend_profile_ref"]
                .as_str()
                .map(str::to_string),
        ];
        for b in backends.into_iter().flatten() {
            if !axon_loop_contracts::PROTECTED_PROFILES.contains(&b.as_str()) {
                return (
                    Outcome::Unknown,
                    format!(
                        "development backend {b} is ineligible for a protected evaluation \
                         (ADR-001 D3)"
                    ),
                );
            }
        }
    }
    let v = &d.ep.verification;
    // A verdict is evidence only if its VERIFICATION evidence joins and is
    // authenticated — the same rule intake applies, over the verification
    // check's own documents. An unauthenticated FAILURE is refused as surely
    // as a pass: forged failures could otherwise sink an arm.
    if matches!(
        v.result,
        VerificationResult::Passed | VerificationResult::Failed
    ) {
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
            )
            .and_then(|(q, r, a, key_id)| {
                Ok(VerificationEvidence {
                    request_ref: digest(&q)?,
                    receipt_ref: digest(&r)?,
                    attestation_ref: digest_value(&a)?,
                    issuer_ref: v
                        .issuer_ref
                        .clone()
                        .ok_or_else(|| refused("an authenticated verdict names no issuer"))?,
                    key_id,
                })
            })
            .map_err(|e| e.to_string()),
            _ => Err("the verification check's request and receipt were not delivered".into()),
        };
        match checked {
            Err(e) => {
                return (
                    Outcome::Unknown,
                    format!("unauthenticated verification: {e}"),
                )
            }
            Ok(ev) => *authenticated = Some(ev),
        }
    }
    match v.result {
        VerificationResult::Passed => {
            let issuer_ok = v
                .issuer_ref
                .as_ref()
                .is_some_and(|i| verifiers.contains(i) && !subjects.contains(i));
            if v.matched_checks == 0 {
                (
                    Outcome::Unknown,
                    "vacuous: passed with zero matched checks".into(),
                )
            } else if !issuer_ok {
                (
                    Outcome::Unknown,
                    "untrusted or subject verifier cannot establish a pass".into(),
                )
            } else {
                (
                    Outcome::VerifiedPass,
                    format!("independently verified ({} checks)", v.matched_checks),
                )
            }
        }
        VerificationResult::Failed => (Outcome::Fail, "verifier reported failure".into()),
        VerificationResult::NotRun => (
            Outcome::Unknown,
            format!("verification not run (status {:?})", d.ep.status),
        ),
        VerificationResult::Unknown => (
            Outcome::Unknown,
            format!("verification unknown (status {:?})", d.ep.status),
        ),
    }
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
