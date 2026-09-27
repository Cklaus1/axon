//! ADR-001 §5: per-attempt trial SAFETY, and the veto it feeds.
//!
//! A safety finding is evidence ABOUT one intaken trial attempt, reported
//! separately from the episode (the episode is the subject's own account).
//! Trust is asymmetric, because the two findings move a decision in
//! opposite directions:
//!
//! * a VIOLATION can only block. It is accepted from a trusted monitor, or
//!   from a subject of the trial itself — any party may incriminate its own
//!   arm. A stranger's report is refused, so no outsider can veto a
//!   candidate by assertion.
//! * a CLEARANCE can help a candidate through. It counts only if a trusted
//!   monitor that is NOT a subject of the trial signed it, under the key
//!   the operator registered for that monitor (`monitor_keys`). A named but
//!   unsigned clearance is refused: a name is not authentication (FG-050).
//!
//! Evaluation captures each trial's state AS OF the evaluation
//! ([`state_of`]), so an admission re-derives identically later. There,
//! any candidate-arm violation VETOES the candidate before any utility is
//! weighed, and an Unknown safety state blocks ACCEPT in a protected-class
//! evaluation (`admission::decide`).

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{OpaqueRef, Ref, Scope, TrialIdentity};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

crate::record_tag!(SafetySchema, "axon.loop.trial-safety/1");

/// The domain a monitor's clearance signature is made for: a signature over
/// any other kind of document can never be presented as one.
pub const CLEARANCE_DOMAIN: &str = "axon.loop.trial-safety/1";

/// What went wrong. Closed: a report names one of these or is refused.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViolationCode {
    /// Acted outside the task's granted scope (paths, tools, network).
    ScopeViolation,
    /// Read, printed or exfiltrated a secret.
    SecretExposure,
    /// Destroyed or corrupted work it was not asked to change.
    DestructiveAction,
    /// Tried to weaken the harness, its checks or its monitors.
    OversightEvasion,
    /// Broke an operator policy not covered above.
    PolicyViolation,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Finding {
    Violation,
    Clear,
}

/// One monitor's (or one self-incriminating subject's) finding about one
/// attempt of one intaken trial.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SafetyReport {
    pub schema: SafetySchema,
    pub scope: Scope,
    pub identity: TrialIdentity,
    pub finding: Finding,
    /// Required for a violation, null for a clearance.
    #[serde(deserialize_with = "crate::nullable")]
    pub code: Option<ViolationCode>,
    pub issuer_ref: OpaqueRef,
    /// `cl22:` of whatever the finding rests on (a transcript, a log), or null.
    #[serde(deserialize_with = "crate::nullable")]
    pub evidence_ref: Option<Ref>,
}

/// A trial's safety as evaluation records it.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SafetyState {
    /// No accepted finding: nothing vouches for this trial either way.
    #[default]
    Unknown,
    /// An authenticated, independent monitor cleared it (and nothing
    /// incriminates it).
    Clear,
    /// An accepted violation. Always wins over a clearance.
    Violation { code: ViolationCode },
}

impl SafetyState {
    pub fn is_unknown(&self) -> bool {
        *self == SafetyState::Unknown
    }
}

/// Record a safety report. Refuses, writing nothing, unless the trial was
/// intaken in the scope and the issuer may say what it says (module docs).
pub fn report(store: &Store, text: &str, signature: Option<&str>) -> Result<(SafetyReport, u64)> {
    let r: SafetyReport = strict_record(text)?;
    if (r.finding == Finding::Violation) != r.code.is_some() {
        return Err(refused(
            "a violation names exactly one code; a clearance names none",
        ));
    }
    let config = store.config()?;
    let mut tx = Tx::begin(store)?;
    let intaken = tx
        .entries()
        .iter()
        .find_map(|e| match &e.event {
            Event::EpisodeIntake { scope, intake }
                if scope == &r.scope && intake.identity == r.identity =>
            {
                Some((**intake).clone())
            }
            _ => None,
        })
        .ok_or_else(|| {
            refused(format!(
                "no intaken trial {:?} in this scope: a finding must be about recorded evidence",
                r.identity.trial_id
            ))
        })?;
    // The trial's subjects: its observer and its policy's proposer — the
    // set intake and evaluation judge verification by.
    let mut subjects = vec![intaken.observed_issuer_ref.clone()];
    subjects.extend(crate::evo::proposer_in(&tx, &r.scope, &intaken.policy_ref));
    let is_monitor = config.trusted_monitors.contains(&r.issuer_ref);
    let is_subject = subjects.contains(&r.issuer_ref);
    let mut key_id = None;
    match r.finding {
        Finding::Violation => {
            if !is_monitor && !is_subject {
                return Err(refused(format!(
                    "{} is neither a trusted monitor nor a subject of this trial: only they may \
                     report it unsafe",
                    r.issuer_ref
                )));
            }
        }
        Finding::Clear => {
            if !is_monitor || is_subject {
                return Err(refused(format!(
                    "a clearance must come from a trusted monitor independent of the trial; {} is \
                     not one",
                    r.issuer_ref
                )));
            }
            let key = config.monitor_keys.get(&r.issuer_ref).ok_or_else(|| {
                refused(format!(
                    "monitor {} has no registered key, so its clearance cannot be authenticated",
                    r.issuer_ref
                ))
            })?;
            let sig = signature.ok_or_else(|| {
                refused("a clearance is not authenticated: no monitor signature was presented")
            })?;
            let sig: Value = axon_loop_contracts::parse_value(sig)?;
            let doc = serde_json::to_value(&r).map_err(|e| LoopError::Io(e.to_string()))?;
            axon_loop_contracts::attestation::verify_document(
                &sig,
                CLEARANCE_DOMAIN,
                &r.issuer_ref,
                &doc,
                key,
            )
            .map_err(|e| refused(format!("clearance signature refused: {e}")))?;
            key_id = axon_loop_contracts::attestation::key_id_of_hex(key);
        }
    }
    for e in tx.entries() {
        if let Event::SafetyReport { report, .. } = &e.event {
            if **report == r {
                return Ok((r, e.seq));
            }
        }
    }
    let seq = tx.append(Event::SafetyReport {
        scope: r.scope.clone(),
        report: Box::new(r.clone()),
        key_id,
    })?;
    Ok((r, seq))
}

/// Every intaken trial's safety in `scope`, from the reports recorded so far
/// in `tx`, keyed by (task, arm, trial): any attempt's violation marks the
/// trial.
pub fn states(tx: &Tx, scope: &Scope) -> BTreeMap<(String, String, String), SafetyState> {
    let mut m: BTreeMap<(String, String, String), SafetyState> = BTreeMap::new();
    for e in tx.entries() {
        let Event::SafetyReport {
            scope: s, report, ..
        } = &e.event
        else {
            continue;
        };
        if s != scope {
            continue;
        }
        let id = &report.identity;
        let key = (
            id.task_id.as_str().to_string(),
            id.arm_id.as_str().to_string(),
            id.trial_id.as_str().to_string(),
        );
        let cur = m.entry(key).or_default();
        *cur = match (*cur, report.finding, report.code) {
            (SafetyState::Violation { .. }, _, _) => *cur,
            (_, Finding::Violation, Some(code)) => SafetyState::Violation { code },
            (_, Finding::Clear, _) => SafetyState::Clear,
            // report() refuses a violation without a code; never recorded.
            (other, Finding::Violation, None) => other,
        };
    }
    m
}
