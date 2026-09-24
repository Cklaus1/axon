//! B270 — whole-task economics over `Usage`.
//!
//! Rules: sum FINAL costs; keep estimates separate; unknown stays unknown (a
//! total with any unknown or unresolved component is reported as
//! `{known_sum, unknown_count, unresolved_liability}`, never as a number);
//! failed/cancelled/unknown attempts are INCLUDED (the denominator is every
//! assigned record); currencies are never mixed; a usage `attempt_ref` seen
//! twice is refused (double counting) rather than summed.
//!
//! Tokens: `Usage` v1 has no token breakdown (uncached / cache-read /
//! cache-write / output), so none is reported or invented here — that needs an
//! episode-schema `/2`.

use crate::error::{refused, LoopError, Result};
use axon_loop_contracts::{Currency, EpisodeStatus, Ref, Usage, UsageState, MAX_INTEGER};
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
    pub by_currency: Vec<CurrencySummary>,
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
    let mut seen: BTreeSet<&Ref> = BTreeSet::new();
    let mut by: BTreeMap<String, CurrencySummary> = BTreeMap::new();
    let mut records = 0u64;
    for (u, status) in items {
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
        c.total = if c.estimated_count == 0
            && c.unknown_count == 0
            && c.unresolved_liability_micro == 0
        {
            Total::Known {
                cost_micro: c.final_sum_micro,
            }
        } else {
            Total::Unresolved {
                known_sum_micro: c.final_sum_micro,
                unknown_count: c.unknown_count + c.estimated_count,
                unresolved_liability_micro: c.unresolved_liability_micro,
            }
        };
        out.push(c);
    }
    Ok(Summary {
        records,
        by_currency: out,
    })
}
