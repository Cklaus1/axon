//! B277 — apply the FROZEN plan rule: ACCEPT / REJECT / INCONCLUSIVE.
//!
//! Admission is separate from evaluation (it reads a STORED evaluation by
//! ref) and from activation (it writes an admission record; only a fenced
//! [`crate::pointer::transition`] moves the pointer). It is deterministic: the
//! record carries no clock, so the same plan + evaluation + admitter always
//! produce the same `cl22:` admission ref.
//!
//! The rule grammar is [`crate::rules`] (a plan whose rules do not parse
//! cannot be frozen). Quality: with `n` assigned trials, `p` verified passes
//! and `u` unknown (incl. missing) outcomes, pessimistic rate = p/n,
//! optimistic = (p+u)/n. Noninferiority is ESTABLISHED iff
//! candidate-pessimistic ≥ incumbent-optimistic − margin AND the candidate
//! has at least one verified pass (a 0/n candidate is never accepted, G01
//! nonvacuous); inferiority is ESTABLISHED iff candidate-optimistic <
//! incumbent-pessimistic − margin (⇒ REJECT); otherwise INCONCLUSIVE.
//!
//! INCONCLUSIVE also whenever: the arms were not assigned the same task set
//! (paired design); an arm has fewer distinct tasks than `independent_units`;
//! more candidates were proposed from the incumbent than the plan allows;
//! the unresolved liability exceeds the tolerance; or either arm's cost is
//! not fully final (a missing trial is an unknown cost). REJECT when
//! inferiority is established or the known costs fail the economic
//! threshold. ACCEPT only when none of those hold.
//!
//! # Re-derivation
//!
//! [`derive`] is a pure function of (frozen plan, journalled evaluation,
//! hypothesis history as of the evaluation, admitter, mechanism flag). `admit`
//! stores its result and journals it; activation ([`rederive`]) recomputes it
//! from the ledger and requires the IDENTICAL record — so an admission file
//! written by hand, or one whose plan/evaluation does not exist, is refused
//! (A6, O1/O2).

use crate::error::{refused, LoopError, Result};
use crate::evl::{ArmResult, EvaluationRecord};
use crate::evo::{Hypothesis, Verdict};
use crate::ledger::{Event, Tx};
use crate::plan::Frozen;
use crate::rules::{Rules, PPM};
use crate::store::{strict_record, Store};
use crate::tel::Total;
use axon_loop_contracts::{AuthorityEpoch, CorpusRole, OpaqueRef, Ref, Scope};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

crate::record_tag!(AdmitRequestSchema, "axon.loop.admit-request/1");
crate::record_tag!(AdmissionSchema, "axon.loop.admission/1");

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitRequest {
    pub schema: AdmitRequestSchema,
    pub experiment_id: String,
    pub evaluation_ref: Ref,
    pub admitter_ref: OpaqueRef,
    pub mechanism_test: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Decision {
    Accept,
    Reject,
    Inconclusive,
    /// ADR-001 §5: a safety violation in the candidate's arm. Decided before
    /// any quality or economic criterion, which cannot offset it.
    Vetoed,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArmFacts {
    pub policy_ref: Ref,
    pub assigned: u64,
    pub distinct_tasks: u64,
    pub verified_pass: u64,
    pub fail: u64,
    pub unknown: u64,
    pub total: Total,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmissionRecord {
    pub schema: AdmissionSchema,
    pub decision: Decision,
    pub reasons: Vec<String>,
    pub scope: Scope,
    pub experiment_id: String,
    pub plan_ref: Ref,
    pub evaluation_ref: Ref,
    pub target_policy_ref: Ref,
    pub incumbent_policy_ref: Ref,
    pub controls_ref: Ref,
    pub admitter_ref: OpaqueRef,
    #[serde(deserialize_with = "crate::nullable")]
    pub proposer_ref: Option<OpaqueRef>,
    pub evaluator_ref: OpaqueRef,
    pub mechanism_test: bool,
    pub deployment_enabled: bool,
    pub evaluated_at_epoch: AuthorityEpoch,
    pub candidate: ArmFacts,
    pub incumbent: ArmFacts,
    pub evidence_refs: Vec<Ref>,
}

pub fn parse_request(text: &str) -> Result<AdmitRequest> {
    strict_record(text)
}

fn facts(a: &ArmResult) -> ArmFacts {
    let tasks: BTreeSet<_> = a.trials.iter().map(|t| &t.task_id).collect();
    // Not ONE currency: no total can be stated, and its liability is never
    // invented as 0 — every currency's unresolved liability still counts
    // against the plan's tolerance (review wf_8aad6d16-ad6, executed: a
    // relabelled currency dropped the arm's whole execution liability).
    let total = a.economics.single_total().cloned().unwrap_or_else(|| {
        let by = &a.economics.by_currency;
        Total::Unresolved {
            known_sum_micro: 0,
            unknown_count: a
                .assigned
                .max(by.iter().map(|c| c.records).sum::<u64>() + a.economics.missing_records),
            unresolved_liability_micro: by
                .iter()
                .map(|c| c.unresolved_liability_micro)
                .fold(0, u64::saturating_add),
        }
    });
    ArmFacts {
        policy_ref: a.policy_ref.clone(),
        assigned: a.assigned,
        distinct_tasks: tasks.len() as u64,
        verified_pass: a.verified_pass,
        fail: a.fail,
        unknown: a.unknown,
        total,
    }
}

fn liability(t: &Total) -> u64 {
    match t {
        Total::Known { .. } => 0,
        Total::Unresolved {
            unresolved_liability_micro,
            ..
        } => *unresolved_liability_micro,
    }
}

/// G11-r22-independent-admission: the loop role, other than admitter, that
/// `who` also holds in the operator's config — a trusted verifier (Compute
/// Fabric's issuer), a context observer or a safety monitor — if any. An
/// identity that produces or judges evidence must not also admit a policy or
/// move the pointer: Fabric could be listed as an admitter and self-promote.
/// Used for the admitter of an admission AND the issuer of every transition.
pub(crate) fn other_loop_role(
    config: &crate::store::Config,
    who: &OpaqueRef,
) -> Option<&'static str> {
    if config.verifiers().contains(who) {
        Some("a trusted verifier (Compute Fabric)")
    } else if config.observers().contains(who) {
        Some("a context observer")
    } else if config.trusted_monitors.contains(who) {
        Some("a safety monitor")
    } else {
        None
    }
}

/// G11-r22-independent-admission: issuing the ACTIVATION (or a rollback) of a
/// candidate is the promotion act, so its issuer is held to the admitter's
/// independence: not the candidate's proposer (the ranker) — nor any EVO
/// proposer on record in the scope, since proposer names are not registered —
/// not the evaluator, and not a subject issuer of the evaluation the
/// admission rests on. (The other loop roles are refused for every
/// transition by [`other_loop_role`].)
pub(crate) fn issuer_independent(tx: &Tx, adm: &AdmissionRecord, who: &OpaqueRef) -> Result<()> {
    let proposer = adm.proposer_ref.as_ref() == Some(who)
        || tx
            .hypotheses(&adm.scope, None)
            .iter()
            .any(|h| matches!(h, Hypothesis::Proposed { proposer_ref, .. } if proposer_ref == who));
    let (_, eval) = crate::evl::load_journalled(tx, &adm.evaluation_ref)?;
    let role = if proposer {
        Some("an EVO proposer (the ranker)")
    } else if &adm.evaluator_ref == who {
        Some("the evaluator")
    } else if eval.subject_issuers.contains(who) {
        Some("a subject issuer")
    } else {
        None
    };
    match role {
        Some(role) => Err(refused(format!(
            "self-promotion: transition issuer {who} is {role} of the admission it activates; \
             the issuer must be independent of the candidate's proposer, evaluator and subjects"
        ))),
        None => Ok(()),
    }
}

/// The pure admission function. Refuses (Err) inputs that cannot be admitted
/// at all; otherwise returns the complete record.
pub(crate) struct Inputs<'a> {
    pub frozen: &'a Frozen,
    pub eval_ref: &'a Ref,
    pub eval: &'a EvaluationRecord,
    pub eval_seq: u64,
    pub admitter: &'a OpaqueRef,
    pub mechanism_test: bool,
}

/// A clearance counts in a protected decision only if its STORED monitor
/// signature verifies over the report, under the key the operator's monitor
/// root holds for its issuer. The ledger's `key_id` is only a string a store
/// writer can copy (dev review round wf_7cb5856d-806, PSV-7).
fn clearance_verifies(
    tx: &Tx,
    config: &crate::store::Config,
    report: &crate::safety::SafetyReport,
    signature_ref: Option<&Ref>,
) -> bool {
    let Some(r) = signature_ref else {
        return false;
    };
    let Ok(pk) = crate::store::Config::rooted_key(
        &config.monitor_keys,
        &report.issuer_ref,
        axon_loop_contracts::operator_trust::TrustAuthority::Monitor,
    ) else {
        return false;
    };
    let Some(sig) = tx
        .store
        .get_cas_text("clearance-signatures", r)
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
    else {
        return false;
    };
    let Ok(doc) = serde_json::to_value(report) else {
        return false;
    };
    axon_loop_contracts::attestation::verify_document(
        &sig,
        crate::safety::CLEARANCE_DOMAIN,
        &report.issuer_ref,
        &doc,
        pk,
    )
    .is_ok()
}

/// Every arm's counters are exactly its trials' outcomes, and the trials are
/// exactly the frozen plan's population (tasks × repetitions per arm).
fn check_arm_grounding(
    tx: &Tx,
    frozen: &Frozen,
    eval: &crate::evl::EvaluationRecord,
) -> Result<()> {
    use crate::evl::Outcome;
    let mut population = Vec::new();
    for arm in &eval.arms {
        let n = |o: Outcome| arm.trials.iter().filter(|t| t.outcome == o).count() as u64;
        let missing = arm
            .trials
            .iter()
            .filter(|t| t.episode_ref.is_none())
            .count() as u64;
        if arm.assigned != arm.trials.len() as u64
            || arm.verified_pass != n(Outcome::VerifiedPass)
            || arm.fail != n(Outcome::Fail)
            || arm.unknown != n(Outcome::Unknown)
            || arm.missing != missing
        {
            return Err(refused(format!(
                "arm {}'s counts are not its trials' outcomes: a protected decision counts only \
                 trials it re-verifies",
                arm.arm_id
            )));
        }
        for t in &arm.trials {
            population.push(crate::evl::Assignment {
                task_id: t.task_id.clone(),
                arm_id: arm.arm_id.clone(),
                trial_id: t.trial_id.clone(),
                policy_ref: arm.policy_ref.clone(),
            });
        }
    }
    crate::evl::check_population(tx, frozen, &population)
}

/// A PROTECTED decision rests on verdicts, not on the stored record's word:
/// every counted verdict is re-verified from its own stored documents,
/// exactly as intake verified it. The attestation is checked under the
/// operator's verifier root, and the PSV bundle under the operator's observer
/// root with every join. The record, the ledger and the CAS are all writable
/// by a store writer; the signatures are not (dev review round
/// wf_336353cb-a2b, PSV-7: a development evaluation relabelled protected
/// reached ACCEPT and activation).
fn reverify_protected(
    tx: &Tx,
    config: &crate::store::Config,
    eval: &crate::evl::EvaluationRecord,
    arm: &crate::evl::ArmResult,
    t: &crate::evl::TrialResult,
    v: &crate::evl::VerificationEvidence,
    rc: &axon_loop_contracts::ExecutionReceipt,
) -> Result<crate::evl::SignedBy> {
    let fail = |e: String| {
        refused(format!(
            "trial {}'s protected verdict does not re-verify from its stored documents ({e}): \
             it does not count",
            t.trial_id
        ))
    };
    if !axon_loop_contracts::protected_evidence::claims_protected(rc) {
        return Err(fail("its receipt does not claim protected evidence".into()));
    }
    let ep_ref = t
        .episode_ref
        .as_ref()
        .ok_or_else(|| fail("the trial cites no episode".into()))?;
    let ep: axon_loop_contracts::LoopEpisode = tx.store.get_contract("episodes", ep_ref)?;
    if ep.identity.trial_id != t.trial_id || ep.identity.task_id != t.task_id {
        return Err(fail("its episode is another trial's".into()));
    }
    // …ran THIS arm's policy (dev review round wf_bf757240-925: a forged
    // record swapped the arms' policy_refs).
    if ep.policy_ref != arm.policy_ref {
        return Err(fail(format!(
            "its episode ran policy {}, not its arm's {}",
            ep.policy_ref, arm.policy_ref
        )));
    }
    let text = |kind: &str, r: &Ref| tx.store.get_cas_text(kind, r);
    let req_text = text("fabric-requests", &v.request_ref)?;
    let rc_text = text("fabric-receipts", &v.receipt_ref)?;
    let att_text = text("fabric-attestations", &v.attestation_ref)?;
    let psv = v
        .psv_evidence_ref
        .as_ref()
        .map(|r| text("fabric-psv-evidence", r))
        .transpose()?;
    let subjects: BTreeSet<OpaqueRef> = eval.subject_issuers.iter().cloned().collect();
    let (_, verified_rc, _, signed_key) = crate::intake::verify_check_evidence(
        &ep,
        &req_text,
        &rc_text,
        Some(&att_text),
        config,
        &subjects,
        psv.as_deref(),
    )
    .map_err(|e| fail(e.to_string()))?;
    // The record's attribution IS the signer that just re-verified (C9 round
    // 1, PSV-5, class c): the verdict was authenticated by the episode's
    // issuer under `signed_key`, and a record naming any other identity, even
    // one the operator root also holds, names the wrong authenticator.
    if ep.verification.issuer_ref.as_ref() != Some(&v.issuer_ref) || signed_key != v.key_id {
        return Err(fail(format!(
            "the record attributes it to {} under {}, but it re-verifies as signed by {} under \
             {signed_key}",
            v.issuer_ref,
            v.key_id,
            ep.verification
                .issuer_ref
                .as_ref()
                .map(|i| i.to_string())
                .unwrap_or_else(|| "no issuer".into()),
        )));
    }
    // The recorded outcome IS the verdict the verifier signed (dev review
    // round wf_bf757240-925: a signed FAILED verdict counted as a pass).
    use axon_loop_contracts::ReceiptVerification as RV;
    match (t.outcome, &verified_rc.verification) {
        (crate::evl::Outcome::VerifiedPass, RV::Passed)
        | (crate::evl::Outcome::Fail, RV::Failed) => {}
        (o, r) => {
            return Err(fail(format!(
                "its recorded outcome {o:?} is not the signed verdict {r:?}"
            )))
        }
    }
    // …and its preflight context carries the observer's signature, stored
    // and re-verified under the operator's observer root (not the
    // `context_signed_by` string).
    let ctx_text = text("contexts", &ep.context_ref)?;
    let ctx: serde_json::Value =
        serde_json::from_str(&ctx_text).map_err(|e| fail(format!("context: {e}")))?;
    let sig_ref = t
        .context_signature_ref
        .as_ref()
        .ok_or_else(|| fail("it cites no context signature".into()))?;
    let sig: serde_json::Value = serde_json::from_str(&text("context-signatures", sig_ref)?)
        .map_err(|e| fail(format!("context signature: {e}")))?;
    let who = OpaqueRef::new(ctx["observed_issuer_ref"].as_str().unwrap_or_default())
        .map_err(|e| fail(format!("context observer: {e}")))?;
    if !config.observers().contains(&who) {
        return Err(fail(format!(
            "its context observer {who} is one the operator no longer trusts"
        )));
    }
    let key = crate::store::Config::rooted_key(
        &config.observer_keys,
        &who,
        axon_loop_contracts::operator_trust::TrustAuthority::Observer,
    )
    .map_err(|e| {
        fail(format!(
            "its protected context is not authenticated: observer {who} is not held with that key by the operator root: {e}"
        ))
    })?;
    let ctx_key = axon_loop_contracts::attestation::verify_document(
        &sig,
        crate::evl::CONTEXT_DOMAIN,
        &who,
        &ctx,
        key,
    )
    .map_err(|e| fail(format!("context signature: {e}")))?;
    // …and the record says it was admitted under THAT observer (C9 round 1,
    // PSV-5). Its `context_signed_by` is joined to the returned signer in
    // `derive`, after the attribution's own presence and root checks.
    if t.context_observer_ref.as_ref() != Some(&who) {
        return Err(fail(format!(
            "the record says its context was admitted under observer {:?}, but it was observed \
             by {who}",
            t.context_observer_ref
        )));
    }
    // …and its execution leg, from its own documents (PSV-7).
    let att_ref = v
        .execution_attestation_ref
        .as_ref()
        .ok_or_else(|| fail("it cites no execution attestation".into()))?;
    let areq: axon_loop_contracts::ComputeRequest =
        serde_json::from_str(&text("acf-requests", &ep.acf_request_ref)?)
            .map_err(|e| fail(format!("execution request: {e}")))?;
    let arc: axon_loop_contracts::ExecutionReceipt =
        serde_json::from_str(&text("acf-receipts", &ep.acf_receipt_ref)?)
            .map_err(|e| fail(format!("execution receipt: {e}")))?;
    let att: serde_json::Value = serde_json::from_str(&text("acf-attestations", att_ref)?)
        .map_err(|e| fail(format!("execution attestation: {e}")))?;
    crate::evl::verify_execution(&att, &areq, &arc, config).map_err(fail)?;
    Ok(crate::evl::SignedBy {
        issuer_ref: who,
        key_id: ctx_key,
    })
}

pub(crate) fn derive(
    tx: &Tx,
    i: Inputs,
    admitters: &BTreeSet<OpaqueRef>,
) -> Result<AdmissionRecord> {
    let Inputs {
        frozen,
        eval_ref,
        eval,
        eval_seq,
        admitter,
        mechanism_test,
    } = i;
    let plan = &frozen.plan;
    if !admitters.contains(admitter) {
        return Err(refused(format!(
            "admitter {admitter} is not in the trusted-admitter set"
        )));
    }
    // G11-r22-independent-admission: no SELF-PROMOTION by another loop role.
    // Checked here, so every re-derivation (activation, rollback) re-applies
    // it. The proposer (the RANKER), subject and evaluator are refused below.
    if let Some(role) = other_loop_role(&tx.store.config()?, admitter) {
        return Err(refused(format!(
            "self-promotion: admitter {admitter} is also {role}; an admitter must hold no other \
             loop role"
        )));
    }
    // Every binding field is checked, and named, separately.
    if eval.scope != plan.scope {
        return Err(refused("binding: evaluation scope differs from the plan's"));
    }
    if eval.experiment_id != plan.experiment_id {
        return Err(refused(
            "binding: evaluation belongs to a different experiment",
        ));
    }
    if eval.plan_ref != frozen.plan_ref {
        return Err(refused(
            "binding: evaluation was made under a different plan",
        ));
    }
    if eval.freeze_seq != frozen.freeze_seq {
        return Err(refused(
            "binding: evaluation was not made under this freeze (plan re-frozen after the evaluation?)",
        ));
    }
    if eval.evaluation_class != frozen.evaluation_class {
        return Err(refused(
            "binding: the evaluation's class is not the frozen plan's",
        ));
    }
    // A PROTECTED decision counts only trials it re-verifies (below): every
    // arm's counters must BE its trials' outcomes, and the trials must be
    // exactly the frozen plan's population, so no stored number and no
    // dropped trial decides it (dev review round wf_7cb5856d-806, PSV-7).
    if eval.evaluation_class == crate::plan::EvaluationClass::Protected {
        check_arm_grounding(tx, frozen, eval)?;
    }
    if eval_seq <= frozen.freeze_seq {
        return Err(refused("evaluation predates the freeze"));
    }
    let rules = Rules::parse(plan).map_err(LoopError::NotReady)?;
    let cand_ref = plan.candidate_policy_ref.clone().expect("frozen");
    let inc_ref = plan.incumbent_policy_ref.clone().expect("frozen");
    let controls = plan.controls_ref.clone().expect("frozen");
    crate::plan::check_candidate(tx, plan, &inc_ref, &cand_ref)?;
    if eval.arms.len() != 2 {
        return Err(refused(
            "admission needs exactly an incumbent and a candidate arm",
        ));
    }
    let cand_arm = eval.arm_for_policy(&cand_ref)?;
    let inc_arm = eval.arm_for_policy(&inc_ref)?;

    let proposer = crate::evo::proposer_in(tx, &plan.scope, &cand_ref)
        .ok_or_else(|| refused("candidate has no EVO proposer on record"))?;
    if &proposer == admitter {
        return Err(refused(
            "self-promotion: the proposer (ranker) cannot admit its own candidate",
        ));
    }
    if eval.subject_issuers.contains(admitter) {
        return Err(refused(
            "the admitter must be independent of the subject: it is a subject issuer",
        ));
    }
    if &eval.evaluator_ref == admitter {
        return Err(refused(
            "the admitter must be independent of the evaluator: it is the evaluator",
        ));
    }
    let want = if mechanism_test {
        CorpusRole::MechanismTest
    } else {
        CorpusRole::Confirmation
    };
    for arm in &eval.arms {
        for t in &arm.trials {
            if let Some(role) = t.corpus_role {
                if role != want {
                    return Err(refused(format!(
                        "trial {} has corpus role {role:?}; this admission accepts only {want:?} evidence",
                        t.trial_id
                    )));
                }
            }
        }
    }
    // A verdict counts only while its verifier still holds (re-audit 4). The
    // evaluation recorded which issuer and which registered key authenticated
    // each counted verdict; a verifier the operator has since untrusted, or
    // re-keyed, vouches for nothing any more — at admission and at every
    // re-derivation (activation, rollback), so revocation is not retroactive
    // only to what has not happened yet.
    let config = tx.store.config()?;
    let verifiers = config.verifiers();
    // Who re-verifiably signed each counted protected trial's context.
    let mut context_signers = std::collections::BTreeMap::new();
    for arm in &eval.arms {
        for t in &arm.trials {
            if !matches!(
                t.outcome,
                crate::evl::Outcome::VerifiedPass | crate::evl::Outcome::Fail
            ) {
                continue;
            }
            let v = t.verification.as_ref().ok_or_else(|| {
                refused(format!(
                    "trial {} counts a verdict but the evaluation does not record which \
                     verifier authenticated it (it predates verdict citation): re-evaluate",
                    t.trial_id
                ))
            })?;
            // O2: for a PROTECTED decision the verifier's key must still be in
            // the operator's verifier root, not merely in the store.
            let key_now = if eval.evaluation_class == crate::plan::EvaluationClass::Protected {
                crate::store::Config::rooted_key(
                    &config.verifier_keys,
                    &v.issuer_ref,
                    axon_loop_contracts::operator_trust::TrustAuthority::Verifier,
                )
                .ok()
            } else {
                config.verifier_keys.get(&v.issuer_ref)
            }
            .and_then(|pk| axon_loop_contracts::attestation::key_id_of_hex(pk));
            if !verifiers.contains(&v.issuer_ref)
                || eval.subject_issuers.contains(&v.issuer_ref)
                || key_now.as_deref() != Some(v.key_id.as_str())
            {
                return Err(refused(format!(
                    "trial {}'s verdict was authenticated by {} under {}, which the operator no \
                     longer trusts with that key: its verdict no longer counts",
                    t.trial_id, v.issuer_ref, v.key_id
                )));
            }
            // ...and what it ran must still be what the operator PINS for it
            // (ADR-001 §5: "verifier keys and pins"; rollback: "profile
            // qualification"). The check's own documents, stored at intake.
            let req: axon_loop_contracts::ComputeRequest =
                tx.store.get_contract("fabric-requests", &v.request_ref)?;
            let rc: axon_loop_contracts::ExecutionReceipt =
                tx.store.get_contract("fabric-receipts", &v.receipt_ref)?;
            crate::intake::check_pins(&config, &v.issuer_ref, &t.task_id, &req, &rc).map_err(
                |e| {
                    refused(format!(
                        "trial {}'s verdict is no longer pinned by the operator ({e}): its \
                         verdict no longer counts",
                        t.trial_id
                    ))
                },
            )?;
            if eval.evaluation_class == crate::plan::EvaluationClass::Protected {
                let signer = reverify_protected(tx, &config, eval, arm, t, v, &rc)?;
                context_signers.insert((arm.arm_id.clone(), t.trial_id.clone()), signer);
            }
        }
    }
    // Every counted trial's context was admitted under a trusted observer;
    // that observer must be trusted still, in every class (review
    // wf_8aad6d16-ad6: a development-class observer withdrawn after admission
    // did not block activation).
    let observers_now = config.observers();
    for arm in &eval.arms {
        for t in &arm.trials {
            if let Some(o) = &t.context_observer_ref {
                if !observers_now.contains(o) {
                    return Err(refused(format!(
                        "trial {}'s context was admitted under observer {o}, which the operator \
                         no longer trusts: its verdict no longer counts",
                        t.trial_id
                    )));
                }
            }
        }
    }
    // A PROTECTED decision also rests on each trial's clearance and on each
    // counted trial's authenticated preflight context. Both are re-checked
    // against the CURRENT operator authority at every derivation, as the
    // verdict's verifier is above: a monitor or observer untrusted, or
    // re-keyed, since then vouches for nothing (review wf_d788c05a-be2).
    if eval.evaluation_class == crate::plan::EvaluationClass::Protected {
        let observers = config.observers();
        for arm in &eval.arms {
            for t in &arm.trials {
                if matches!(
                    t.outcome,
                    crate::evl::Outcome::VerifiedPass | crate::evl::Outcome::Fail
                ) && !t.context_signed_by.as_ref().is_some_and(|c| {
                    observers.contains(&c.issuer_ref)
                        && crate::store::Config::rooted_key(
                            &config.observer_keys,
                            &c.issuer_ref,
                            axon_loop_contracts::operator_trust::TrustAuthority::Observer,
                        )
                        .ok()
                        .and_then(|pk| axon_loop_contracts::attestation::key_id_of_hex(pk))
                        .as_deref()
                            == Some(c.key_id.as_str())
                }) {
                    return Err(refused(format!(
                        "trial {}'s protected context is not authenticated by an observer the \
                         operator still trusts with the key it was verified under: its verdict \
                         no longer counts",
                        t.trial_id
                    )));
                }
                // …and the attribution IS the signer that re-verified it (C9
                // round 1, PSV-5): a second observer the operator root also
                // holds is still the wrong authenticator.
                if let (Some(c), Some(signer)) = (
                    &t.context_signed_by,
                    context_signers.get(&(arm.arm_id.clone(), t.trial_id.clone())),
                ) {
                    if c != signer {
                        return Err(refused(format!(
                            "trial {}'s context is attributed to {} under {}, but it re-verifies \
                             as signed by {} under {}: its verdict does not count",
                            t.trial_id, c.issuer_ref, c.key_id, signer.issuer_ref, signer.key_id
                        )));
                    }
                }
                if t.safety == crate::safety::SafetyState::Clear
                    && !tx.entries().iter().any(|e| {
                        e.seq <= eval_seq
                            && matches!(&e.event, Event::SafetyReport { scope, report, key_id, signature_ref }
                                if scope == &eval.scope
                                    && report.finding == crate::safety::Finding::Clear
                                    && report.identity.task_id == t.task_id
                                    && report.identity.arm_id == arm.arm_id
                                    && report.identity.trial_id == t.trial_id
                                    && config.trusted_monitors.contains(&report.issuer_ref)
                                    && key_id.is_some()
                                    && crate::store::Config::rooted_key(
                                        &config.monitor_keys,
                                        &report.issuer_ref,
                                        axon_loop_contracts::operator_trust::TrustAuthority::Monitor,
                                    )
                                    .ok()
                                    .and_then(|pk| axon_loop_contracts::attestation::key_id_of_hex(pk))
                                        == *key_id
                                    && clearance_verifies(tx, &config, report, signature_ref.as_ref()))
                    })
                {
                    return Err(refused(format!(
                        "trial {}'s clearance is not from a monitor the operator still trusts with \
                         the key it was signed under: the trial is no longer cleared",
                        t.trial_id
                    )));
                }
            }
        }
    }
    // Hypothesis history AS OF the evaluation, so the count cannot change
    // between `admit` and a later re-derivation.
    let proposals = tx
        .hypotheses(&plan.scope, Some(eval_seq))
        .iter()
        .filter(|h| matches!(h, Hypothesis::Proposed { parent_policy_ref, .. } if parent_policy_ref == &inc_ref))
        .count() as u64;

    let (decision, reasons) = decide(&rules, proposals, cand_arm, inc_arm, eval.evaluation_class);
    Ok(AdmissionRecord {
        schema: AdmissionSchema,
        decision,
        reasons,
        scope: plan.scope.clone(),
        experiment_id: plan.experiment_id.clone(),
        plan_ref: frozen.plan_ref.clone(),
        evaluation_ref: eval_ref.clone(),
        target_policy_ref: cand_ref,
        incumbent_policy_ref: inc_ref,
        controls_ref: controls,
        admitter_ref: admitter.clone(),
        proposer_ref: Some(proposer),
        evaluator_ref: eval.evaluator_ref.clone(),
        mechanism_test,
        deployment_enabled: plan.deployment_enabled,
        evaluated_at_epoch: eval.authority_epoch,
        candidate: facts(cand_arm),
        incumbent: facts(inc_arm),
        evidence_refs: eval.evidence_refs.clone(),
    })
}

/// Decide, store and journal. Refuses (writing nothing) when the inputs
/// cannot be admitted at all.
pub fn admit(store: &Store, req: &AdmitRequest) -> Result<(AdmissionRecord, Ref)> {
    crate::store::check_segment("experiment id", &req.experiment_id)?;
    let mut tx = Tx::begin(store)?;
    let frozen = crate::plan::ready_in(&tx, &req.experiment_id)?;
    let (eval_seq, eval) = crate::evl::load_journalled(&tx, &req.evaluation_ref)?;
    let admitters = store.config()?.admitters();
    let rec = derive(
        &tx,
        Inputs {
            frozen: &frozen,
            eval_ref: &req.evaluation_ref,
            eval: &eval,
            eval_seq,
            admitter: &req.admitter_ref,
            mechanism_test: req.mechanism_test,
        },
        &admitters,
    )?;
    let r = store.put_cas("admissions", &rec)?;
    if tx.admission_event(&r).is_none() {
        // ONE append: the decision and its hypothesis verdict (the verdict
        // is read back from this entry by `Tx::hypotheses`). A kill -9 leaves
        // the store at pre or post, never between the two (CW1).
        tx.append(Event::Admission {
            scope: rec.scope.clone(),
            experiment_id: rec.experiment_id.clone(),
            admission_ref: r.clone(),
            target_policy_ref: rec.target_policy_ref.clone(),
            decision: rec.decision,
            mechanism_test: rec.mechanism_test,
        })?;
    }
    Ok((rec, r))
}

/// For activation: the admission must have been journalled by `admit`, and
/// recomputing it from the ledger's frozen plan + journalled evaluation must
/// give the IDENTICAL record, decided ACCEPT, by a still-trusted admitter.
pub(crate) fn rederive(
    tx: &Tx,
    adm_ref: &Ref,
    admitters: &BTreeSet<OpaqueRef>,
) -> Result<AdmissionRecord> {
    if tx.admission_event(adm_ref).is_none() {
        return Err(refused(format!(
            "admission {adm_ref} was never journalled by `admit`"
        )));
    }
    let stored: AdmissionRecord = tx.store.get_record("admissions", adm_ref)?;
    let frozen = crate::plan::frozen_in(tx, &stored.experiment_id)?
        .ok_or_else(|| refused("admission's plan is not frozen"))?;
    let (eval_seq, eval) = crate::evl::load_journalled(tx, &stored.evaluation_ref)?;
    let again = derive(
        tx,
        Inputs {
            frozen: &frozen,
            eval_ref: &stored.evaluation_ref,
            eval: &eval,
            eval_seq,
            admitter: &stored.admitter_ref,
            mechanism_test: stored.mechanism_test,
        },
        admitters,
    )?;
    if again != stored {
        return Err(refused(format!(
            "admission {adm_ref} does not re-derive from its plan and evaluation"
        )));
    }
    if again.decision != Decision::Accept {
        return Err(refused(format!(
            "admission {adm_ref} decided {:?}, not ACCEPT",
            again.decision
        )));
    }
    Ok(again)
}

fn decide(
    rules: &Rules,
    proposals: u64,
    cand_arm: &ArmResult,
    inc_arm: &ArmResult,
    class: crate::plan::EvaluationClass,
) -> (Decision, Vec<String>) {
    // ADR-001 §5, the VETO STAGE, before any utility: one unsafe attempt in
    // the candidate's arm decides, whatever its quality or cost.
    let vetoes: Vec<String> = cand_arm
        .trials
        .iter()
        .filter_map(|t| match t.safety {
            crate::safety::SafetyState::Violation { code } => Some(format!(
                "safety veto: trial {} ({:?}) was reported unsafe: {code:?}",
                t.trial_id, t.task_id
            )),
            _ => None,
        })
        .collect();
    if !vetoes.is_empty() {
        return (Decision::Vetoed, vetoes);
    }
    let cand = facts(cand_arm);
    let inc = facts(inc_arm);
    let mut inconclusive = Vec::new();
    let mut reject = Vec::new();

    // Unknown safety blocks ACCEPT where the evidence must be protected: no
    // authenticated independent monitor cleared the trial. (A development
    // evaluation promotes nothing in a protected scope anyway — D3.)
    if class == crate::plan::EvaluationClass::Protected {
        let unknown = cand_arm
            .trials
            .iter()
            .filter(|t| t.safety != crate::safety::SafetyState::Clear)
            .count();
        if unknown > 0 {
            inconclusive.push(format!(
                "safety unknown: {unknown} candidate trial(s) have no authenticated independent \
                 clearance, which a protected evaluation requires"
            ));
        }
    }

    if proposals > 1 {
        inconclusive.push(format!(
            "multiplicity: {proposals} candidates were proposed from this incumbent but the plan declares single_candidate"
        ));
    }
    if proposals > rules.candidate_budget {
        inconclusive.push(format!(
            "candidate budget exceeded: {proposals} proposals > {}",
            rules.candidate_budget
        ));
    }

    // Paired design (order_rule = paired_tasks): the same assigned task set.
    let ct: BTreeSet<_> = cand_arm.trials.iter().map(|t| &t.task_id).collect();
    let it: BTreeSet<_> = inc_arm.trials.iter().map(|t| &t.task_id).collect();
    if ct != it {
        inconclusive.push(format!(
            "unpaired: arms were assigned different task sets ({} vs {} tasks, {} shared)",
            ct.len(),
            it.len(),
            ct.intersection(&it).count()
        ));
    }

    // Sample size, cluster unit = task.
    for (name, a) in [("candidate", &cand), ("incumbent", &inc)] {
        if a.distinct_tasks < rules.independent_units {
            inconclusive.push(format!(
                "sample size: {name} arm has {} independent task(s) < plan minimum {}",
                a.distinct_tasks, rules.independent_units
            ));
        }
        if a.assigned == 0 {
            inconclusive.push(format!("{name} arm has no assigned trials"));
        }
    }

    // An arm whose costs are not in ONE currency has no statable cost or
    // liability: it is never decided as if they were known.
    for (who, arm) in [("candidate", cand_arm), ("incumbent", inc_arm)] {
        let n = arm.economics.by_currency.len();
        if n != 1 && arm.assigned > arm.missing {
            inconclusive.push(format!(
                "the {who} arm's economics span {n} currencies: its cost and liability cannot be \
                 stated in one unit"
            ));
        }
    }
    // Unknown liability.
    let liab = liability(&cand.total).saturating_add(liability(&inc.total));
    if liab > rules.max_liability_micro {
        inconclusive.push(format!(
            "unresolved liability {liab} µ exceeds plan tolerance {} µ",
            rules.max_liability_micro
        ));
    }

    // Quality noninferiority on exact bounds (integer arithmetic, ppm).
    if cand.assigned > 0 && inc.assigned > 0 {
        let (m, nc, ni) = (
            rules.margin_ppm as u128,
            cand.assigned as u128,
            inc.assigned as u128,
        );
        let p = PPM as u128;
        let c_pess = cand.verified_pass as u128;
        let c_opt = c_pess + cand.unknown as u128;
        let i_pess = inc.verified_pass as u128;
        let i_opt = i_pess + inc.unknown as u128;
        // c/nc >= i/ni - m/1e6   ⇔   c*ni*1e6 + m*nc*ni >= i*nc*1e6
        let ge = |c: u128, i: u128| c * ni * p + m * nc * ni >= i * nc * p;
        if ge(c_pess, i_opt) && c_pess > 0 {
            // established, and nonvacuous
        } else if !ge(c_opt, i_pess) {
            reject.push(format!(
                "quality inferiority established: candidate ≤ {c_opt}/{nc} passes vs incumbent ≥ {i_pess}/{ni} beyond margin"
            ));
        } else if c_opt == 0 {
            reject.push("candidate has no verified pass at all".into());
        } else {
            inconclusive.push(format!(
                "noninferiority cannot be established: candidate {c_pess}..{c_opt}/{nc}, incumbent {i_pess}..{i_opt}/{ni} passes"
            ));
        }
    }

    // Economics: only on fully known totals (a missing trial is unknown), and
    // not at all under `report_only` (ADR-001 D4): then they are recorded in
    // the arm facts and decide nothing.
    match (rules.min_cost_reduction_ppm, &cand.total, &inc.total) {
        (None, _, _) => {}
        (Some(t), Total::Known { cost_micro: cc }, Total::Known { cost_micro: ic })
            if cand.assigned > 0 && inc.assigned > 0 =>
        {
            let (t, cc, ic) = (
                t as u128,
                *cc as u128,
                *ic as u128,
            );
            let (nc, ni, p) = (cand.assigned as u128, inc.assigned as u128, PPM as u128);
            if cc * ni * p > ic * nc * (p - t) {
                reject.push(format!(
                    "economic benefit not met: candidate {cc}µ/{nc} trials vs incumbent {ic}µ/{ni} trials, required reduction {t} ppm"
                ));
            }
        }
        _ => inconclusive.push(
            "economics cannot be established: an arm's cost is not fully final (unknown, estimated, liability or missing trial)".into(),
        ),
    }

    if !reject.is_empty() {
        reject.extend(inconclusive);
        (Decision::Reject, reject)
    } else if !inconclusive.is_empty() {
        (Decision::Inconclusive, inconclusive)
    } else {
        let mut reasons = vec!["all frozen plan criteria established".to_string()];
        if rules.min_cost_reduction_ppm.is_none() {
            reasons.push(
                "economics report-only (ADR-001 D4): recorded in the arm facts, not assessed"
                    .into(),
            );
        }
        (Decision::Accept, reasons)
    }
}

/// Load a stored admission record.
pub fn load(store: &Store, r: &Ref) -> Result<AdmissionRecord> {
    store.get_record("admissions", r)
}

impl From<Decision> for Verdict {
    fn from(d: Decision) -> Verdict {
        match d {
            Decision::Accept => Verdict::Accept,
            Decision::Reject => Verdict::Reject,
            Decision::Inconclusive => Verdict::Inconclusive,
            Decision::Vetoed => Verdict::Vetoed,
        }
    }
}

#[cfg(test)]
mod decide_tests {
    use super::*;
    use crate::evl::ArmResult;
    use serde_json::json;

    fn arm(id: &str, pass: u64, cost_each: u64) -> ArmResult {
        let usages: Vec<axon_loop_contracts::Usage> = (0..2)
            .map(|i| {
                serde_json::from_value(json!({
                    "state": "final", "cost_micro": cost_each, "unresolved_liability_micro": 0,
                    "currency": "USD", "price_schedule_ref": format!("cl22:{}", "d".repeat(64)),
                    "attempt_refs": [format!("cl22:{:064x}", i + if id == "c" { 100 } else { 0 })],
                }))
                .unwrap()
            })
            .collect();
        ArmResult {
            arm_id: axon_loop_contracts::ArmId::new(id).unwrap(),
            policy_ref: Ref::new(format!(
                "cl22:{}",
                if id == "c" { "c" } else { "a" }.repeat(64)
            ))
            .unwrap(),
            assigned: 2,
            verified_pass: pass,
            fail: 2 - pass,
            unknown: 0,
            missing: 0,
            unknown_kinds: Default::default(),
            trials: vec![],
            economics: crate::tel::summarize_with_missing(
                usages
                    .iter()
                    .map(|u| (u, Some(axon_loop_contracts::EpisodeStatus::Completed))),
                0,
            )
            .unwrap(),
        }
    }

    fn rules(min_cost: Option<u64>) -> Rules {
        Rules {
            margin_ppm: 0,
            min_cost_reduction_ppm: min_cost,
            max_liability_micro: 0,
            independent_units: 0,
            candidate_budget: 1,
        }
    }

    /// The cost criterion over KNOWN totals (reachable once metered attempt
    /// receipts exist, ADR-001 D4): a candidate not cheaper by the frozen
    /// reduction is rejected with the reason stated; `report_only` lets cost
    /// decide nothing.
    #[test]
    fn the_cost_rule_decides_only_when_it_is_frozen_and_known() {
        let dev = crate::plan::EvaluationClass::Development;
        let (d, r) = decide(
            &rules(Some(100_000)),
            1,
            &arm("c", 2, 95),
            &arm("i", 2, 100),
            dev,
        );
        assert_eq!(d, Decision::Reject, "{r:?}");
        assert!(
            r.iter().any(|x| x.contains("economic benefit not met")),
            "{r:?}"
        );
        let (d, r) = decide(
            &rules(Some(100_000)),
            1,
            &arm("c", 2, 50),
            &arm("i", 2, 100),
            dev,
        );
        assert_eq!(d, Decision::Accept, "{r:?}");
        let (d, r) = decide(&rules(None), 1, &arm("c", 2, 95), &arm("i", 2, 100), dev);
        assert_eq!(
            d,
            Decision::Accept,
            "report_only: cost decides nothing: {r:?}"
        );
    }

    /// An arm whose costs span two currencies has no statable cost or
    /// liability: INCONCLUSIVE, and its liability is summed, never 0.
    #[test]
    fn a_multi_currency_arm_is_never_decided_as_known() {
        let dev = crate::plan::EvaluationClass::Development;
        let mut c = arm("c", 2, 50);
        let usage = |cur: &str, state: &str, cost: Option<u64>, liab: u64, n: u32| {
            serde_json::from_value::<axon_loop_contracts::Usage>(json!({
                "state": state, "cost_micro": cost, "unresolved_liability_micro": liab,
                "currency": cur, "price_schedule_ref": format!("cl22:{}", "d".repeat(64)),
                "attempt_refs": [format!("cl22:{:064x}", n)],
            }))
            .unwrap()
        };
        // Only-guard case first (C9 round 1b, M117): two currencies, both
        // FINAL, no liability, economics report-only. No liability tolerance
        // and no economics rule can make this arm inconclusive; only the
        // currency-span rule stops it being decided as if its costs had one
        // unit.
        let mut both_final = arm("c", 2, 50);
        let fin = [
            usage("USD", "final", Some(50), 0, 996),
            usage("EUR", "final", Some(50), 0, 997),
        ];
        both_final.economics = crate::tel::summarize_with_missing(
            fin.iter()
                .map(|u| (u, Some(axon_loop_contracts::EpisodeStatus::Completed))),
            0,
        )
        .unwrap();
        assert_eq!(both_final.economics.by_currency.len(), 2);
        let (d, r) = decide(&rules(None), 1, &both_final, &arm("i", 2, 100), dev);
        if d != Decision::Inconclusive {
            panic!("ATTACK: a two-currency arm was decided as known: {d:?} {r:?}");
        }
        assert!(r.iter().any(|x| x.contains("span 2 currencies")), "{r:?}");
        let us = [
            usage("USD", "final", Some(50), 0, 998),
            usage("EUR", "unknown", None, 900_000, 999),
        ];
        c.economics = crate::tel::summarize_with_missing(
            us.iter()
                .map(|u| (u, Some(axon_loop_contracts::EpisodeStatus::Completed))),
            0,
        )
        .unwrap();
        assert_eq!(c.economics.by_currency.len(), 2);
        match facts(&c).total {
            Total::Unresolved {
                unresolved_liability_micro,
                ..
            } => assert_eq!(unresolved_liability_micro, 900_000),
            t => panic!("a two-currency arm was stated as {t:?}"),
        }
        let (d, r) = decide(&rules(None), 1, &c, &arm("i", 2, 100), dev);
        assert_eq!(d, Decision::Inconclusive, "{r:?}");
        assert!(r.iter().any(|x| x.contains("span 2 currencies")), "{r:?}");
    }
}
