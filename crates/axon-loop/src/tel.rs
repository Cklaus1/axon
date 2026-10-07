//! B270 — whole-task economics over `Usage`.
//!
//! Rules: sum FINAL costs; keep estimates separate; unknown stays unknown (a
//! total with any unknown or unresolved component is reported as
//! `{known_sum, unknown_count, unresolved_liability}`, never as a number);
//! failed/cancelled/unknown attempts are INCLUDED (the denominator is every
//! assigned record); currencies are never mixed; a usage `attempt_ref` seen
//! twice is refused (double counting) rather than summed.
//!
//! Fabric join ([`join`]): each Fabric attempt (`acf-compute-request/1` +
//! `acf-execution-receipt/1`) is keyed by its receipt's `cl22:` content Ref —
//! the same Ref a sidecar's `acf_receipt_ref` carries — and joined to the
//! sidecar usages whose `attempt_refs` name it. The identical receipt supplied
//! twice is collapsed; two DIFFERENT receipts for one `(operation_id,
//! attempt_id)` are refused (double counting). Every attempt contributes an
//! EXECUTION component alongside the model usage: under D10 that component is
//! always unknown with its reserved liability (see [`crate::price`]), and an
//! attempt a usage names with no receipt contributes an unknown component
//! too. So a joined total is never a known number today — which is the
//! truthful answer while no execution price schedule exists.
//!
//! Comparison ([`cohort_cost`], [`compare_per_trial`]): the denominator is
//! every ASSIGNED trial, never the successes or the priced records; any
//! unknown or unresolved component makes the cohort unresolved, and an
//! unresolved cohort never wins or loses a comparison.
//!
//! Tokens: `Usage` v1 has no token breakdown (uncached / cache-read /
//! cache-write / output), so none is reported or invented here — that needs an
//! episode-schema `/2`.

use crate::error::{refused, LoopError, Result};
use crate::price::{execution_cost, ExecutionCost, PinnedSchedule};
use axon_loop_contracts::{
    digest, project_receipt_status, AttemptId, ComputeRequest, Contract, Currency, EpisodeStatus,
    ExecutionReceipt, OperationId, Ref, Usage, UsageState, MAX_INTEGER,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Total {
    /// Every record final, no liability outstanding.
    Known { cost_micro: u64 },
    /// At least one record not final (estimated or unknown) or carrying
    /// liability. `known_sum_micro` is the FINAL costs only; estimates are in
    /// `estimated_sum_micro` and are counted in `unknown_count` because an
    /// estimate is not a final cost.
    Unresolved {
        known_sum_micro: u64,
        unknown_count: u64,
        unresolved_liability_micro: u64,
    },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrencySummary {
    pub currency: Currency,
    pub records: u64,
    pub final_count: u64,
    pub estimated_count: u64,
    pub unknown_count: u64,
    pub final_sum_micro: u64,
    pub estimated_sum_micro: u64,
    pub unresolved_liability_micro: u64,
    /// Records whose episode was not `completed` (failed, cancelled,
    /// outcome_unknown, refused, unsupported) — included in every sum above.
    pub non_completed_records: u64,
    pub price_schedule_refs: Vec<Ref>,
    pub total: Total,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Summary {
    pub records: u64,
    /// Assigned attempts with NO usage record at all (never delivered). Each
    /// is an unknown cost; any nonzero value makes every total unresolved.
    pub missing_records: u64,
    pub by_currency: Vec<CurrencySummary>,
}

impl Summary {
    /// The single-currency total, or `None` when it cannot be stated (no
    /// records, more than one currency).
    pub fn single_total(&self) -> Option<&Total> {
        match self.by_currency.as_slice() {
            [c] => Some(&c.total),
            _ => None,
        }
    }
}

fn add(a: u64, b: u64, what: &str) -> Result<u64> {
    let s = a
        .checked_add(b)
        .filter(|s| *s <= MAX_INTEGER)
        .ok_or_else(|| LoopError::Refused(format!("{what} exceeds 2^53-1")))?;
    Ok(s)
}

/// Summarize usages. `status` is the owning episode's status when known.
pub fn summarize<'a>(
    items: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
) -> Result<Summary> {
    summarize_with_missing(items, 0)
}

/// [`summarize`] plus `missing` assigned attempts that produced no usage at
/// all. A missing attempt is an UNKNOWN cost, never zero and never dropped:
/// it is counted in every currency's `unknown_count` and forces every total
/// to `unresolved` (J202/Q4).
pub fn summarize_with_missing<'a>(
    items: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    missing: u64,
) -> Result<Summary> {
    summarize_components(items, std::iter::empty(), missing)
}

/// [`summarize_with_missing`] plus each attempt's EXECUTION component (see
/// [`execution_component`]): what an evaluation reports per arm.
pub(crate) fn summarize_with_execution<'a>(
    model: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    execution: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    missing: u64,
) -> Result<Summary> {
    summarize_components(model, execution, missing)
}

/// One Fabric attempt's execution cost as a usage component, in the model
/// usage's currency: under D10 always UNKNOWN, holding its reservation as
/// liability (`crate::price::execution_cost`) — never zero, never omitted.
pub(crate) fn execution_component(
    model: &Usage,
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
) -> Result<Usage> {
    let crate::price::ExecutionCost::Unknown {
        reserved_liability_micro,
    } = crate::price::execution_cost(req, rc);
    Ok(Usage {
        state: UsageState::Unknown,
        cost_micro: None,
        unresolved_liability_micro: reserved_liability_micro,
        currency: model.currency.clone(),
        price_schedule_ref: model.price_schedule_ref.clone(),
        attempt_refs: vec![digest(rc)?],
    })
}

/// The shared core. `model` and `execution` are separate COMPONENTS of the
/// same attempts, so each has its own double-count check: an attempt may
/// appear once in each, never twice in either.
fn summarize_components<'a>(
    model: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    execution: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    missing: u64,
) -> Result<Summary> {
    let mut by: BTreeMap<String, CurrencySummary> = BTreeMap::new();
    let mut records = 0u64;
    let mut seen_model: BTreeSet<&Ref> = BTreeSet::new();
    let mut seen_exec: BTreeSet<&Ref> = BTreeSet::new();
    let tagged = model
        .into_iter()
        .map(|x| (x, false))
        .chain(execution.into_iter().map(|x| (x, true)));
    for ((u, status), is_exec) in tagged {
        let seen = if is_exec {
            &mut seen_exec
        } else {
            &mut seen_model
        };
        for a in &u.attempt_refs {
            if !seen.insert(a) {
                return Err(refused(format!(
                    "attempt {a} is accounted twice (duplicate usage would double-count)"
                )));
            }
        }
        records += 1;
        let c = by
            .entry(u.currency.as_str().to_string())
            .or_insert_with(|| CurrencySummary {
                currency: u.currency.clone(),
                records: 0,
                final_count: 0,
                estimated_count: 0,
                unknown_count: 0,
                final_sum_micro: 0,
                estimated_sum_micro: 0,
                unresolved_liability_micro: 0,
                non_completed_records: 0,
                price_schedule_refs: Vec::new(),
                total: Total::Known { cost_micro: 0 },
            });
        c.records += 1;
        if !c.price_schedule_refs.contains(&u.price_schedule_ref) {
            c.price_schedule_refs.push(u.price_schedule_ref.clone());
        }
        if status.is_some_and(|s| s != EpisodeStatus::Completed) {
            c.non_completed_records += 1;
        }
        c.unresolved_liability_micro = add(
            c.unresolved_liability_micro,
            u.unresolved_liability_micro,
            "unresolved liability",
        )?;
        match (u.state, u.cost_micro) {
            (UsageState::Final, Some(v)) => {
                c.final_count += 1;
                c.final_sum_micro = add(c.final_sum_micro, v, "final cost")?;
            }
            (UsageState::Estimated, Some(v)) => {
                c.estimated_count += 1;
                c.estimated_sum_micro = add(c.estimated_sum_micro, v, "estimated cost")?;
            }
            // Estimated with no number, or unknown: unknown, never 0.
            _ => c.unknown_count += 1,
        }
    }
    let mut out = Vec::new();
    for (_, mut c) in by {
        c.price_schedule_refs.sort();
        c.total = if missing == 0
            && c.estimated_count == 0
            && c.unknown_count == 0
            && c.unresolved_liability_micro == 0
        {
            Total::Known {
                cost_micro: c.final_sum_micro,
            }
        } else {
            Total::Unresolved {
                known_sum_micro: c.final_sum_micro,
                unknown_count: c.unknown_count + c.estimated_count + missing,
                unresolved_liability_micro: c.unresolved_liability_micro,
            }
        };
        out.push(c);
    }
    Ok(Summary {
        records,
        missing_records: missing,
        by_currency: out,
    })
}

// ── Fabric join ─────────────────────────────────────────────────────────────

/// One Fabric attempt: the request it ran under and the receipt it produced.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct FabricAttempt {
    pub request: ComputeRequest,
    pub receipt: ExecutionReceipt,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct Joined {
    /// Model usage AND one execution component per attempt, per currency.
    pub summary: Summary,
    pub price_schedule_ref: Ref,
    /// Distinct Fabric attempts joined (by receipt content Ref).
    pub fabric_attempts: u64,
    /// Byte-identical receipts supplied more than once, collapsed.
    pub identical_duplicates: u64,
    /// Attempt refs a usage names that no supplied receipt matches. Each is
    /// an UNKNOWN execution component, never a free one.
    pub unjoined_attempt_refs: u64,
    /// Receipts no usage names. Still counted: failed and retried attempts
    /// are part of the task's cost.
    pub unreferenced_receipts: u64,
    pub execution_cost_basis: &'static str,
}

pub const EXECUTION_COST_BASIS: &str =
    "unknown: no execution price schedule exists (D10); each attempt holds its reservation as liability";

/// Join sidecar usages to Fabric receipts per attempt ref, under one pinned
/// schedule (see the module docs). Refuses: a usage or request naming another
/// schedule or currency (G10); a request/receipt pair whose identities
/// differ; the same receipt Ref paired with a different request; two
/// different receipts for one `(operation_id, attempt_id)`; an attempt ref
/// named by two usages.
pub fn join<'a>(
    schedule: &PinnedSchedule,
    usages: impl IntoIterator<Item = (&'a Usage, Option<EpisodeStatus>)>,
    attempts: &[FabricAttempt],
    missing: u64,
) -> Result<Joined> {
    // Receipt content Ref → (attempt, its request's digest).
    let mut by_ref: BTreeMap<Ref, (&FabricAttempt, Ref)> = BTreeMap::new();
    let mut by_attempt: BTreeMap<(OperationId, AttemptId), Ref> = BTreeMap::new();
    let mut identical_duplicates = 0u64;
    for (i, fa) in attempts.iter().enumerate() {
        let (req, rc) = (&fa.request, &fa.receipt);
        req.validate()
            .map_err(|e| refused(format!("fabric_attempts[{i}].request: {e}")))?;
        rc.validate()
            .map_err(|e| refused(format!("fabric_attempts[{i}].receipt: {e}")))?;
        if req.operation_id != rc.operation_id
            || req.attempt_id != rc.attempt_id
            || req.task_id != rc.task_id
            || req.trial_id != rc.trial_id
        {
            return Err(refused(format!(
                "fabric_attempts[{i}]: receipt identity does not match its request"
            )));
        }
        schedule.check_request(req)?;
        let rref = digest(rc)?;
        let qref = digest(req)?;
        if let Some((_, q)) = by_ref.get(&rref) {
            if *q != qref {
                return Err(refused(format!(
                    "receipt {rref} is paired with two different requests"
                )));
            }
            identical_duplicates += 1;
            continue;
        }
        let key = (rc.operation_id.clone(), rc.attempt_id.clone());
        if let Some(other) = by_attempt.get(&key) {
            return Err(refused(format!(
                "attempt {}/{} is reported by two different receipts ({other}, {rref}): \
                 counting both would double-count it",
                key.0, key.1
            )));
        }
        by_attempt.insert(key, rref.clone());
        by_ref.insert(rref, (fa, qref));
    }

    let usages: Vec<(&Usage, Option<EpisodeStatus>)> = usages.into_iter().collect();
    let mut referenced: BTreeSet<&Ref> = BTreeSet::new();
    let mut exec: Vec<(Usage, Option<EpisodeStatus>)> = Vec::new();
    let mut unjoined = 0u64;
    for (u, status) in &usages {
        schedule.check_usage(u)?;
        for a in &u.attempt_refs {
            if by_ref.contains_key(a) {
                referenced.insert(a);
            } else {
                // No receipt: its execution cost is unknown AND its
                // reservation is unknown — an unresolved component with no
                // liability number, which still forbids a known total.
                unjoined += 1;
                exec.push((exec_usage(schedule, 0, a.clone()), *status));
            }
        }
    }
    let mut unreferenced = 0u64;
    for (rref, (fa, _)) in &by_ref {
        if !referenced.contains(rref) {
            unreferenced += 1;
        }
        let ExecutionCost::Unknown {
            reserved_liability_micro,
        } = execution_cost(&fa.request, &fa.receipt);
        exec.push((
            exec_usage(schedule, reserved_liability_micro, rref.clone()),
            Some(project_receipt_status(fa.receipt.status)),
        ));
    }
    let summary = summarize_components(
        usages.iter().copied(),
        exec.iter().map(|(u, s)| (u, *s)),
        missing,
    )?;
    Ok(Joined {
        summary,
        price_schedule_ref: schedule.reference().clone(),
        fabric_attempts: by_ref.len() as u64,
        identical_duplicates,
        unjoined_attempt_refs: unjoined,
        unreferenced_receipts: unreferenced,
        execution_cost_basis: EXECUTION_COST_BASIS,
    })
}

fn exec_usage(schedule: &PinnedSchedule, liability: u64, attempt: Ref) -> Usage {
    Usage {
        state: UsageState::Unknown,
        cost_micro: None,
        unresolved_liability_micro: liability,
        currency: schedule.currency().clone(),
        price_schedule_ref: schedule.reference().clone(),
        attempt_refs: vec![attempt],
    }
}

// ── cohort cost and comparison ──────────────────────────────────────────────

/// The cost of one arm's cohort, over EVERY assigned trial.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum CohortCost {
    Known {
        total_micro: u64,
        assigned: u64,
    },
    /// Some component is unknown, estimated, missing or carries liability.
    /// The unresolved records are counted here — they are not free and they
    /// do not leave the denominator.
    Unresolved {
        known_sum_micro: u64,
        unresolved_records: u64,
        unresolved_liability_micro: u64,
        assigned: u64,
    },
}

/// The cohort cost of `s` over `assigned` trials. The denominator is the
/// caller's ASSIGNED count, not anything derived from which records priced
/// or succeeded; it must be at least 1 and at least the missing count. More
/// than one currency cannot be one cost and is refused.
pub fn cohort_cost(s: &Summary, assigned: u64) -> Result<CohortCost> {
    if assigned == 0 {
        return Err(refused("a cohort with no assigned trials has no cost"));
    }
    if s.missing_records > assigned {
        return Err(refused(format!(
            "{} missing records exceed {assigned} assigned trials",
            s.missing_records
        )));
    }
    let c = match s.by_currency.as_slice() {
        [c] => c,
        [] if s.missing_records == 0 => {
            return Err(refused("no usage records: the cohort's cost is unknown"))
        }
        [] => {
            return Ok(CohortCost::Unresolved {
                known_sum_micro: 0,
                unresolved_records: s.missing_records,
                unresolved_liability_micro: 0,
                assigned,
            })
        }
        _ => return Err(refused("currencies are never mixed in one cohort cost")),
    };
    Ok(match &c.total {
        Total::Known { cost_micro } => CohortCost::Known {
            total_micro: *cost_micro,
            assigned,
        },
        Total::Unresolved {
            known_sum_micro,
            unknown_count,
            unresolved_liability_micro,
        } => CohortCost::Unresolved {
            known_sum_micro: *known_sum_micro,
            unresolved_records: *unknown_count,
            unresolved_liability_micro: *unresolved_liability_micro,
            assigned,
        },
    })
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CostOrder {
    /// The first cohort costs less per assigned trial.
    Lower,
    Equal,
    Higher,
    /// Either side is unresolved: there is no winner.
    Unresolved,
}

/// Per-assigned-trial cost order of `a` against `b`. Only two KNOWN cohorts
/// are ordered; anything unresolved is [`CostOrder::Unresolved`], so an
/// unknown cost can never be the cheaper side.
pub fn compare_per_trial(a: &CohortCost, b: &CohortCost) -> CostOrder {
    match (a, b) {
        (
            CohortCost::Known {
                total_micro: ca,
                assigned: na,
            },
            CohortCost::Known {
                total_micro: cb,
                assigned: nb,
            },
        ) => {
            let l = *ca as u128 * *nb as u128;
            let r = *cb as u128 * *na as u128;
            match l.cmp(&r) {
                std::cmp::Ordering::Less => CostOrder::Lower,
                std::cmp::Ordering::Equal => CostOrder::Equal,
                std::cmp::Ordering::Greater => CostOrder::Higher,
            }
        }
        _ => CostOrder::Unresolved,
    }
}
