//! Contract D (first role): `acf-execution-receipt/1`, field for field.
//!
//! SUPERVISOR-OBSERVED PROCESS FACTS ONLY. Process completion differs from
//! verification, and this receipt is never upgraded into an
//! [`crate::ExecutionContextReceipt`] (or vice versa).

use crate::error::{shape, Refusal};
use crate::ids::{Acf1Ref, AttemptId, ExecutionId, OpaqueRef, OperationId, TaskId, TrialId};
use crate::{check_int, nullable, schema_tag, Contract};
use serde::{Deserialize, Serialize};

schema_tag!(ExecutionReceiptSchema, "acf-execution-receipt/1");

/// ACF spelling (`canceled`, `denied`, `timed_out`) — deliberately NOT the
/// sidecar's [`crate::EpisodeStatus`]. The mapping is explicit in
/// [`crate::bind_acf`], and `timed_out` never becomes success.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    Completed,
    Failed,
    Canceled,
    TimedOut,
    OutcomeUnknown,
    Denied,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptVerification {
    NotRequested,
    NotRun,
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceSource {
    SupervisorObserved,
    ProviderReported,
    WorkerReported,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptUsageState {
    Estimated,
    Metered,
    ProviderReported,
    Settled,
    Unknown,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionReceipt {
    pub schema: ExecutionReceiptSchema,
    pub operation_id: OperationId,
    pub task_id: TaskId,
    pub trial_id: TrialId,
    pub attempt_id: AttemptId,
    pub execution_id: ExecutionId,
    pub backend_profile_ref: OpaqueRef,
    pub input_workspace_ref: Acf1Ref,
    #[serde(deserialize_with = "nullable")]
    pub output_workspace_ref: Option<Acf1Ref>,
    pub policy_digest: Acf1Ref,
    pub status: ReceiptStatus,
    /// 0..=255 per the ACF schema (`u8` makes anything else unrepresentable).
    #[serde(deserialize_with = "nullable")]
    pub process_exit_code: Option<u8>,
    pub verification: ReceiptVerification,
    #[serde(deserialize_with = "nullable")]
    pub matched_checks: Option<u64>,
    pub evidence_source: EvidenceSource,
    pub evidence_refs: Vec<OpaqueRef>,
    pub usage_state: ReceiptUsageState,
    /// Unknown is `None`, never 0.
    #[serde(deserialize_with = "nullable")]
    pub cost_micro: Option<u64>,
    pub unresolved_liability_micro: u64,
}

impl Contract for ExecutionReceipt {
    fn validate(&self) -> Result<(), Refusal> {
        if let Some(m) = self.matched_checks {
            check_int("matched_checks", m, 0)?;
        }
        if let Some(c) = self.cost_micro {
            check_int("cost_micro", c, 0)?;
        }
        check_int(
            "unresolved_liability_micro",
            self.unresolved_liability_micro,
            0,
        )?;
        if self.evidence_refs.len() > 128 {
            return Err(shape("evidence_refs: max 128 items"));
        }
        if self.verification == ReceiptVerification::Passed {
            if self.status != ReceiptStatus::Completed {
                return Err(shape("verification passed requires status completed"));
            }
            if self.process_exit_code != Some(0) {
                return Err(shape("verification passed requires process_exit_code 0"));
            }
            if !matches!(self.matched_checks, Some(n) if n >= 1) {
                return Err(shape("verification passed requires matched_checks > 0"));
            }
            if self.evidence_source != EvidenceSource::SupervisorObserved {
                return Err(shape(
                    "verification passed requires supervisor_observed evidence",
                ));
            }
            if self.evidence_refs.is_empty() {
                return Err(shape("verification passed requires evidence_refs"));
            }
        }
        if self.status == ReceiptStatus::OutcomeUnknown {
            if !matches!(
                self.verification,
                ReceiptVerification::NotRun | ReceiptVerification::Unknown
            ) {
                return Err(shape(
                    "status outcome_unknown admits only verification not_run/unknown",
                ));
            }
            if self.process_exit_code.is_some() {
                return Err(shape(
                    "status outcome_unknown requires process_exit_code null",
                ));
            }
        }
        if self.usage_state == ReceiptUsageState::Unknown && self.cost_micro.is_some() {
            return Err(shape("usage_state unknown requires cost_micro = null"));
        }
        Ok(())
    }
}
