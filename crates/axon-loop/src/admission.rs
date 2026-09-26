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
    let total = a
        .economics
        .single_total()
        .cloned()
        .unwrap_or(Total::Unresolved {
            known_sum_micro: 0,
            unknown_count: a.assigned,
            unresolved_liability_micro: 0,
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
    if eval.scope != plan.scope || eval.experiment_id != plan.experiment_id {
        return Err(refused("evaluation belongs to a different experiment"));
    }
    if eval.plan_ref != frozen.plan_ref || eval.freeze_seq != frozen.freeze_seq {
        return Err(refused(
            "evaluation was not made under this frozen plan (plan frozen after the evaluation?)",
        ));
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
        return Err(refused("the proposer cannot admit its own candidate"));
    }
    if eval.subject_issuers.contains(admitter) || &eval.evaluator_ref == admitter {
        return Err(refused(
            "the admitter must be independent of the subject and the evaluator",
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

    // Economics: only on fully known totals (a missing trial is unknown).
    match (&cand.total, &inc.total) {
        (Total::Known { cost_micro: cc }, Total::Known { cost_micro: ic })
            if cand.assigned > 0 && inc.assigned > 0 =>
        {
            let (t, cc, ic) = (
                rules.min_cost_reduction_ppm as u128,
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
        (
            Decision::Accept,
            vec!["all frozen plan criteria established".into()],
        )
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
