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
//!   workspaces, status semantics);
//! * `verification.result == passed` with `matched_checks > 0`;
//! * the verification issuer is in the store's `trusted_verifiers` and is NOT
//!   a subject issuer (the request's list, plus the candidate's proposer).
//!
//! Anything else is `fail` (a clear negative) or `unknown` (unbound,
//! unverifiable, missing, timed out, …). Both count AGAINST quality: the
//! denominator is every ASSIGNED trial, and an assigned trial with no
//! delivered episode is `unknown: missing` — reported, not dropped.

use crate::error::{refused, LoopError, Result};
use crate::store::{contract_from_value, strict_record, Store};
use crate::tel::{self, Summary};
use axon_loop_contracts::{
    bind_acf, bind_episode, digest, ArmId, AuthorityEpoch, ComputeRequest, CorpusRole,
    EpisodeStatus, ExecutionContextReceipt, ExecutionReceipt, LoopEpisode, OpaqueRef,
    PolicyEnvelope, PolicyProjection, Ref, Scope, TaskId, TrialId, VerificationResult,
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
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvlRequest {
    pub schema: EvlRequestSchema,
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
    pub authority_epoch: AuthorityEpoch,
    pub evaluator_ref: OpaqueRef,
    pub trusted_verifiers: Vec<OpaqueRef>,
    pub subject_issuers: Vec<OpaqueRef>,
    pub arms: Vec<ArmResult>,
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
}

/// Evaluate and store. Returns the record and its `cl22:` ref.
pub fn evaluate(store: &Store, r: &EvlRequest) -> Result<(EvaluationRecord, Ref)> {
    let config = store.config()?;
    let verifiers = config.verifiers();
    if verifiers.is_empty() {
        return Err(refused(
            "no trusted verifiers configured: nothing can be verified",
        ));
    }
    let epoch = crate::epoch::current(store, &r.scope)?;

    let mut policies: BTreeMap<Ref, PolicyEnvelope> = BTreeMap::new();
    for (i, v) in r.policies.iter().enumerate() {
        let p: PolicyEnvelope = contract_from_value(&format!("policies[{i}]"), v)?;
        if p.scope != r.scope {
            return Err(refused(format!("policies[{i}] is for another scope")));
        }
        policies.insert(digest(&p)?, p);
    }

    // Subject issuers: the request's, plus every stored proposer of an arm policy.
    let mut subjects: BTreeSet<OpaqueRef> = r.subject_issuers.iter().cloned().collect();
    for pref in policies.keys() {
        if let Some(p) = crate::evo::proposer_of(store, &r.scope, pref)? {
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
                by_currency: Vec::new(),
            },
        });
        arm.assigned += 1;
        let key = (a.task_id.clone(), a.arm_id.clone(), a.trial_id.clone());
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
                let (o, why) = judge(d, policy, &a.policy_ref, epoch, &verifiers, &subjects);
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
        });
    }
    for (id, arm) in arms.iter_mut() {
        let us = usages.remove(id).unwrap_or_default();
        arm.economics = tel::summarize(us.iter().map(|(u, s)| (u, *s)))?;
    }

    let mut subject_issuers: Vec<OpaqueRef> = subjects.into_iter().collect();
    subject_issuers.sort();
    let rec = EvaluationRecord {
        schema: EvaluationSchema,
        scope: r.scope.clone(),
        authority_epoch: epoch,
        evaluator_ref: r.evaluator_ref.clone(),
        trusted_verifiers: verifiers.into_iter().collect(),
        subject_issuers,
        arms: arms.into_values().collect(),
        evidence_refs: evidence.into_iter().collect(),
    };
    for p in policies.values() {
        store.put_cas("policies", p)?;
    }
    let r = store.put_cas("evaluations", &rec)?;
    Ok((rec, r))
}

fn judge(
    d: &Delivered,
    policy: &PolicyEnvelope,
    policy_ref: &Ref,
    epoch: AuthorityEpoch,
    verifiers: &BTreeSet<OpaqueRef>,
    subjects: &BTreeSet<OpaqueRef>,
) -> (Outcome, String) {
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
    let v = &d.ep.verification;
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
