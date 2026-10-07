//! B270 — the pinned price schedule, and what may be priced under it.
//!
//! A price schedule is pinned by CONTENT: the caller names the `cl22:` Ref it
//! expects and supplies the document; [`PinnedSchedule::pin`] refuses a
//! document whose canonical digest is not that Ref. Nothing is priced under a
//! schedule named only by a string.
//!
//! # G10: one schedule identity across the ACF request and the sidecar
//!
//! `acf-compute-request/1` carries `limits.price_schedule_ref` as an
//! `OpaqueRef` (the ACF reference contract's type, never extended in place);
//! the episode sidecar's `usage.price_schedule_ref` is a content `Ref`. They
//! are joined here, and only here: the request's opaque string must BE the
//! pinned schedule's `cl22:` Ref string, byte for byte — no normalisation, no
//! other scheme (operator decision D9). A request or usage naming any other
//! schedule, or another currency, is refused.
//!
//! # D10: there is no execution price schedule
//!
//! A schedule that claims to cover execution is refused: no operator-signed
//! execution schedule exists, and pricing execution under an unsigned one
//! would be inventing a number. Execution cost is therefore always
//! [`ExecutionCost::Unknown`], carrying the conservative liability (the
//! request's reservation, or anything larger the receipt reports) — never
//! zero. G10-r22-full-task-cost stays NOT_RUN until that schedule exists.
//!
//! Money is µ-units (1e-6) of the schedule's currency (D9).

use crate::error::{refused, LoopError, Result};
use axon_loop_contracts::{
    ComputeRequest, Currency, ExecutionReceipt, OpaqueRef, Ref, RefScheme, Refusal, Usage,
};
use serde::{Deserialize, Serialize};

pub const PRICE_SCHEDULE_SCHEMA: &str = "axon.loop.price-schedule/1";

/// What a schedule prices.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Coverage {
    /// Model inference (tokens), priced by the provider's rates.
    Model,
    /// Execution resources (CPU, memory, disk, checks). REFUSED (D10).
    Execution,
}

/// The schedule document. `rates` is opaque to this module: pinning fixes WHICH
/// rates were in force, it does not evaluate them.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PriceScheduleDoc {
    pub schema: String,
    pub currency: Currency,
    pub covers: Vec<Coverage>,
    pub rates: serde_json::Value,
}

/// A schedule whose content was checked against its `cl22:` Ref.
#[derive(Clone, PartialEq, Eq, Debug, Serialize)]
pub struct PinnedSchedule {
    reference: Ref,
    currency: Currency,
    covers: Vec<Coverage>,
}

impl PinnedSchedule {
    /// Pin `doc_json` as the schedule `expected`. Refuses: a non-`cl22` Ref, a
    /// document that does not hash to it, a wrong schema, an empty or
    /// repeated coverage list, and any execution coverage (D10).
    pub fn pin(expected: &Ref, doc_json: &str) -> Result<PinnedSchedule> {
        if expected.scheme() != RefScheme::Cl22 {
            return Err(refused(format!(
                "price schedule must be pinned by a cl22 content Ref, got {expected}"
            )));
        }
        let value = axon_loop_contracts::parse_value(doc_json)?;
        let actual = axon_loop_contracts::digest_value(&value)?;
        if actual != *expected {
            return Err(refused(format!(
                "price schedule content is {actual}, not the pinned {expected}"
            )));
        }
        let doc: PriceScheduleDoc = serde_json::from_value(value)
            .map_err(|e| LoopError::Malformed(Refusal::Shape(format!("price schedule: {e}"))))?;
        if doc.schema != PRICE_SCHEDULE_SCHEMA {
            return Err(LoopError::Malformed(Refusal::Shape(format!(
                "price schedule schema must be {PRICE_SCHEDULE_SCHEMA}"
            ))));
        }
        if doc.covers.is_empty() {
            return Err(refused("price schedule covers nothing"));
        }
        let mut covers = doc.covers.clone();
        covers.sort_by_key(|c| *c as u8);
        covers.dedup();
        if covers.len() != doc.covers.len() {
            return Err(refused("price schedule repeats a coverage"));
        }
        if covers.contains(&Coverage::Execution) {
            return Err(refused(
                "price schedule claims execution coverage: no operator-signed execution \
                 price schedule exists (D10), so execution cost stays unknown",
            ));
        }
        Ok(PinnedSchedule {
            reference: expected.clone(),
            currency: doc.currency,
            covers,
        })
    }

    pub fn reference(&self) -> &Ref {
        &self.reference
    }

    pub fn currency(&self) -> &Currency {
        &self.currency
    }

    pub fn covers(&self, c: Coverage) -> bool {
        self.covers.contains(&c)
    }

    /// G10: the request's `limits.price_schedule_ref` (an `OpaqueRef`) must be
    /// exactly this schedule's `cl22:` Ref, in this schedule's currency.
    pub fn check_request(&self, req: &ComputeRequest) -> Result<()> {
        let r = resolve_opaque(&req.limits.price_schedule_ref)?;
        if r != self.reference {
            return Err(refused(format!(
                "operation {}: limits.price_schedule_ref {} is not the pinned schedule {}",
                req.operation_id, req.limits.price_schedule_ref, self.reference
            )));
        }
        if req.limits.currency_code != self.currency {
            return Err(refused(format!(
                "operation {}: currency {} is not the pinned schedule's {}",
                req.operation_id, req.limits.currency_code, self.currency
            )));
        }
        Ok(())
    }

    /// G10: a sidecar usage must name this schedule, in its currency.
    pub fn check_usage(&self, u: &Usage) -> Result<()> {
        if u.price_schedule_ref != self.reference {
            return Err(refused(format!(
                "usage.price_schedule_ref {} is not the pinned schedule {}",
                u.price_schedule_ref, self.reference
            )));
        }
        if u.currency != self.currency {
            return Err(refused(format!(
                "usage currency {} is not the pinned schedule's {}",
                u.currency, self.currency
            )));
        }
        Ok(())
    }
}

/// Resolve an ACF `OpaqueRef` to the content Ref it must spell: a valid
/// `cl22:` Ref string, unchanged. Anything else is refused, never coerced.
pub fn resolve_opaque(o: &OpaqueRef) -> Result<Ref> {
    let r = Ref::new(o.as_str()).map_err(|_| {
        refused(format!(
            "price_schedule_ref {o} is not a content Ref (it must be the pinned schedule's cl22 Ref)"
        ))
    })?;
    if r.scheme() != RefScheme::Cl22 || r.as_str() != o.as_str() {
        return Err(refused(format!(
            "price_schedule_ref {o} is not a cl22 content Ref"
        )));
    }
    Ok(r)
}

/// The execution cost of one Fabric attempt. There is no `Known` variant:
/// with no execution price schedule (D10) nothing can make it known.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum ExecutionCost {
    Unknown { reserved_liability_micro: u64 },
}

/// Execution cost of `receipt` under `req` (D10): unknown, holding the
/// largest of the reservation (`limits.max_cost_micro`), the receipt's
/// unresolved liability and any cost the receipt reports — a reported cost
/// is not a price under a pinned schedule, but it is never allowed to LOWER
/// what is held.
pub fn execution_cost(req: &ComputeRequest, receipt: &ExecutionReceipt) -> ExecutionCost {
    let held = req
        .limits
        .max_cost_micro
        .max(receipt.unresolved_liability_micro)
        .max(receipt.cost_micro.unwrap_or(0));
    ExecutionCost::Unknown {
        reserved_liability_micro: held,
    }
}
