//! B277 — apply the FROZEN plan rule: ACCEPT / REJECT / INCONCLUSIVE.
//!
//! Admission is separate from evaluation (it reads a STORED evaluation by
//! ref) and from activation (it writes an admission record; only a fenced
//! [`crate::pointer::transition`] moves the pointer). It is deterministic: the
//! record carries no clock, so the same plan + evaluation + admitter always
//! produce the same `cl22:` admission ref.
//!
//! # The executable rule grammar
//!
//! The pilot schema's rule fields are free text, and the package supplies no
//! universal thresholds. This admitter executes exactly this grammar; a rule
//! it cannot parse yields INCONCLUSIVE ("not executable"), never a guess:
//!
//! | field | accepted value | meaning |
//! |---|---|---|
//! | `quality_margin` | `pass_rate_margin_ppm=<n>` | noninferiority margin on verified-pass rate, parts per million |
//! | `economic_threshold` | `min_cost_reduction_ppm=<n>` | candidate cost per assigned trial must be ≤ incumbent × (1 − n/1e6) |
//! | `budget_rule` | `max_unresolved_liability_micro=<n>` | unknown-liability tolerance across both arms |
//! | `missing_data_rule` | `unknown_bounds` | unknown/missing outcomes are bounded both ways (below) |
//! | `uncertainty_rule` | `exact_bounds` | no sampling model: decide on the worst/best-case bounds only |
//! | `multiplicity_rule` | `single_candidate` | at most one candidate may have been proposed from this plan's incumbent |
//!
//! Quality: with `n` assigned trials, `p` verified passes and `u` unknown
//! (incl. missing) outcomes: pessimistic rate = p/n (every unknown a failure),
//! optimistic rate = (p+u)/n (every unknown a pass). Noninferiority is
//! ESTABLISHED iff candidate-pessimistic ≥ incumbent-optimistic − margin;
//! inferiority is ESTABLISHED iff candidate-optimistic < incumbent-pessimistic
//! − margin (⇒ REJECT); otherwise it cannot be established (⇒ INCONCLUSIVE).
//!
//! INCONCLUSIVE also whenever: an arm's distinct-task count is below
//! `independent_units`; the unresolved liability exceeds the tolerance; either
//! arm's cost is not fully final (economics cannot be established); or a rule
//! is not executable. REJECT when inferiority is established or the known
//! costs fail the economic threshold. ACCEPT only when none of those hold.

use crate::error::{refused, Result};
use crate::evl::{ArmResult, EvaluationRecord};
use crate::evo::{Hypothesis, Verdict};
use crate::plan::PilotPlan;
use crate::store::{strict_record, Store};
use crate::tel::Total;
use axon_loop_contracts::{AuthorityEpoch, CorpusRole, OpaqueRef, PolicyEnvelope, Ref, Scope};
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

fn rule_u64(field: &str, value: &Option<String>, key: &str) -> std::result::Result<u64, String> {
    let v = value.as_deref().ok_or(format!("{field} unset"))?;
    v.strip_prefix(key)
        .and_then(|r| r.strip_prefix('='))
        .and_then(|n| n.parse::<u64>().ok())
        .filter(|n| *n <= axon_loop_contracts::MAX_INTEGER)
        .ok_or(format!(
            "{field} {v:?} is not executable (expected `{key}=<n>`)"
        ))
}

fn rule_word(field: &str, value: &Option<String>, word: &str) -> std::result::Result<(), String> {
    match value.as_deref() {
        Some(v) if v == word => Ok(()),
        Some(v) => Err(format!(
            "{field} {v:?} is not executable (expected `{word}`)"
        )),
        None => Err(format!("{field} unset")),
    }
}

fn facts(a: &ArmResult) -> Result<ArmFacts> {
    let tasks: BTreeSet<_> = a.trials.iter().map(|t| &t.task_id).collect();
    let total = match a.economics.by_currency.as_slice() {
        [] => Total::Unresolved {
            known_sum_micro: 0,
            unknown_count: a.assigned,
            unresolved_liability_micro: 0,
        },
        [c] => c.total.clone(),
        _ => return Err(refused("an arm mixes currencies; costs are not comparable")),
    };
    Ok(ArmFacts {
        policy_ref: a.policy_ref.clone(),
        assigned: a.assigned,
        distinct_tasks: tasks.len() as u64,
        verified_pass: a.verified_pass,
        fail: a.fail,
        unknown: a.unknown,
        total,
    })
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

/// Decide. Writes the admission record and a hypothesis verdict. Refuses
/// (writing nothing) when the inputs cannot be admitted at all: plan not
/// frozen/ready, wrong scope or arms, untrusted admitter, admitter = proposer
/// or subject, wrong corpus role.
pub fn admit(store: &Store, req: &AdmitRequest) -> Result<(AdmissionRecord, Ref)> {
    let (plan, plan_ref) = crate::plan::ready(store, &req.experiment_id)?;
    let eval: EvaluationRecord = crate::evl::load(store, &req.evaluation_ref)?;
    let config = store.config()?;
    if !config.admitters().contains(&req.admitter_ref) {
        return Err(refused(format!(
            "admitter {} is not in the trusted-admitter set",
            req.admitter_ref
        )));
    }
    if eval.scope != plan.scope {
        return Err(refused("evaluation and plan are for different scopes"));
    }
    let cand_ref = plan.candidate_policy_ref.clone().expect("ready");
    let inc_ref = plan.incumbent_policy_ref.clone().expect("ready");
    let controls = plan.controls_ref.clone().expect("ready");
    let cand_env: PolicyEnvelope = store.get_contract("policies", &cand_ref)?;
    if cand_env.controls_ref != controls || cand_env.scope != plan.scope {
        return Err(refused(
            "candidate envelope controls/scope differ from the frozen plan",
        ));
    }
    if eval.arms.len() != 2 {
        return Err(refused(
            "admission needs exactly an incumbent and a candidate arm",
        ));
    }
    let cand = facts(eval.arm_for_policy(&cand_ref)?)?;
    let inc = facts(eval.arm_for_policy(&inc_ref)?)?;

    let proposer = crate::evo::proposer_of(store, &plan.scope, &cand_ref)?;
    if proposer.as_ref() == Some(&req.admitter_ref) {
        return Err(refused("the proposer cannot admit its own candidate"));
    }
    if eval.subject_issuers.contains(&req.admitter_ref) || eval.evaluator_ref == req.admitter_ref {
        return Err(refused(
            "the admitter must be independent of the subject and the evaluator",
        ));
    }
    let want = if req.mechanism_test {
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

    let (decision, reasons) = decide(store, &plan, &cand, &inc)?;
    let rec = AdmissionRecord {
        schema: AdmissionSchema,
        decision,
        reasons,
        scope: plan.scope.clone(),
        experiment_id: plan.experiment_id.clone(),
        plan_ref,
        evaluation_ref: req.evaluation_ref.clone(),
        target_policy_ref: cand_ref.clone(),
        incumbent_policy_ref: inc_ref,
        controls_ref: controls,
        admitter_ref: req.admitter_ref.clone(),
        proposer_ref: proposer,
        evaluator_ref: eval.evaluator_ref.clone(),
        mechanism_test: req.mechanism_test,
        deployment_enabled: plan.deployment_enabled,
        evaluated_at_epoch: eval.authority_epoch,
        candidate: cand,
        incumbent: inc,
        evidence_refs: eval.evidence_refs.clone(),
    };
    let r = store.put_cas("admissions", &rec)?;
    let verdict = match decision {
        Decision::Accept => Verdict::Accept,
        Decision::Reject => Verdict::Reject,
        Decision::Inconclusive => Verdict::Inconclusive,
    };
    let already = crate::evo::history(store, &plan.scope)?
        .iter()
        .any(|h| matches!(h, Hypothesis::Verdict { admission_ref, .. } if admission_ref == &r));
    if !already {
        crate::evo::append_verdict(store, &plan.scope, &cand_ref, verdict, &r)?;
    }
    Ok((rec, r))
}

fn decide(
    store: &Store,
    plan: &PilotPlan,
    cand: &ArmFacts,
    inc: &ArmFacts,
) -> Result<(Decision, Vec<String>)> {
    let mut inconclusive = Vec::new();
    let mut reject = Vec::new();

    let margin = rule_u64(
        "quality_margin",
        &plan.quality_margin,
        "pass_rate_margin_ppm",
    );
    let econ = rule_u64(
        "economic_threshold",
        &plan.economic_threshold,
        "min_cost_reduction_ppm",
    );
    let tol = rule_u64(
        "budget_rule",
        &plan.budget_rule,
        "max_unresolved_liability_micro",
    );
    for r in [
        rule_word(
            "missing_data_rule",
            &plan.missing_data_rule,
            "unknown_bounds",
        ),
        rule_word("uncertainty_rule", &plan.uncertainty_rule, "exact_bounds"),
        rule_word(
            "multiplicity_rule",
            &plan.multiplicity_rule,
            "single_candidate",
        ),
    ] {
        if let Err(e) = r {
            inconclusive.push(e);
        }
    }
    if plan.multiplicity_rule.as_deref() == Some("single_candidate") {
        // Every candidate ever proposed from THIS incumbent counts, rejected and
        // inconclusive ones included: that is the attempted-candidate count
        // multiplicity must be charged for.
        let inc_ref = plan.incumbent_policy_ref.as_ref().expect("ready");
        let n = crate::evo::history(store, &plan.scope)?
            .iter()
            .filter(|h| {
                matches!(h, Hypothesis::Proposed { parent_policy_ref, .. } if parent_policy_ref == inc_ref)
            })
            .count() as u64;
        if n > 1 {
            inconclusive.push(format!(
                "multiplicity: {n} candidates were proposed from this incumbent but the plan declares single_candidate"
            ));
        }
        if n > plan.candidate_budget.unwrap_or(0) {
            inconclusive.push(format!(
                "candidate budget exceeded: {n} proposals > {}",
                plan.candidate_budget.unwrap_or(0)
            ));
        }
    }

    // Sample size.
    let min_units = plan.independent_units.unwrap_or(u64::MAX);
    for (name, a) in [("candidate", cand), ("incumbent", inc)] {
        if a.distinct_tasks < min_units {
            inconclusive.push(format!(
                "sample size: {name} arm has {} independent task(s) < plan minimum {min_units}",
                a.distinct_tasks
            ));
        }
        if a.assigned == 0 {
            inconclusive.push(format!("{name} arm has no assigned trials"));
        }
    }

    // Unknown liability.
    let liab = liability(&cand.total).saturating_add(liability(&inc.total));
    match &tol {
        Ok(t) if liab > *t => inconclusive.push(format!(
            "unresolved liability {liab} µ exceeds plan tolerance {t} µ"
        )),
        Ok(_) => {}
        Err(e) => inconclusive.push(e.clone()),
    }

    // Quality noninferiority on exact bounds (integer arithmetic, ppm).
    match &margin {
        Err(e) => inconclusive.push(e.clone()),
        Ok(m) if cand.assigned > 0 && inc.assigned > 0 => {
            let (m, nc, ni) = (*m as u128, cand.assigned as u128, inc.assigned as u128);
            // Only UNKNOWN outcomes are uncertain; a verified fail is a fail.
            let unk = |a: &ArmFacts| a.unknown as u128;
            let c_pess = cand.verified_pass as u128;
            let c_opt = c_pess + unk(cand);
            let i_pess = inc.verified_pass as u128;
            let i_opt = i_pess + unk(inc);
            // c/nc >= i/ni - m/1e6   ⇔   c*ni*1e6 + m*nc*ni >= i*nc*1e6
            let ge = |c: u128, i: u128| c * ni * 1_000_000 + m * nc * ni >= i * nc * 1_000_000;
            if ge(c_pess, i_opt) {
                // established
            } else if !ge(c_opt, i_pess) {
                reject.push(format!(
                    "quality inferiority established: candidate ≤ {c_opt}/{nc} passes vs incumbent ≥ {i_pess}/{ni} beyond margin"
                ));
            } else {
                inconclusive.push(format!(
                    "noninferiority cannot be established: candidate {c_pess}..{c_opt}/{nc}, incumbent {i_pess}..{i_opt}/{ni} passes"
                ));
            }
        }
        Ok(_) => {}
    }

    // Economics: only on fully known totals.
    match (&econ, &cand.total, &inc.total) {
        (Err(e), _, _) => inconclusive.push(e.clone()),
        (Ok(t), Total::Known { cost_micro: cc }, Total::Known { cost_micro: ic })
            if cand.assigned > 0 && inc.assigned > 0 =>
        {
            let (t, cc, ic) = (*t as u128, *cc as u128, *ic as u128);
            let (nc, ni) = (cand.assigned as u128, inc.assigned as u128);
            if t > 1_000_000 {
                inconclusive.push("economic_threshold above 100%".into());
            } else if cc * ni * 1_000_000 > ic * nc * (1_000_000 - t) {
                reject.push(format!(
                    "economic benefit not met: candidate {cc}µ/{nc} trials vs incumbent {ic}µ/{ni} trials, required reduction {t} ppm"
                ));
            }
        }
        (Ok(_), _, _) => inconclusive
            .push("economics cannot be established: an arm's cost is not fully final".into()),
    }

    Ok(if !reject.is_empty() {
        reject.extend(inconclusive);
        (Decision::Reject, reject)
    } else if !inconclusive.is_empty() {
        (Decision::Inconclusive, inconclusive)
    } else {
        (
            Decision::Accept,
            vec!["all frozen plan criteria established".into()],
        )
    })
}

/// Load a stored admission record.
pub fn load(store: &Store, r: &Ref) -> Result<AdmissionRecord> {
    store.get_record("admissions", r)
}
