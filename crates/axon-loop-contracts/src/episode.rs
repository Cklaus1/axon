//! Contract B: the post-preflight episode sidecar.
//!
//! The sidecar REFERENCES the original episode (`source_episode_ref`) and the
//! ACF request/receipt; it never replaces them. `axon-cortex`'s `Episode` and
//! its `axc1:` digest are unchanged.
//!
//! Post-preflight means context and operation identities are mandatory: both
//! `context_ref` and every id in `identity` are required, non-null fields, so
//! a record without them does not deserialize. A failure BEFORE preflight is
//! TASK_NOT_STARTED, owned by the existing refusal recorder — it must not be
//! fabricated into a `LoopEpisode`.

use crate::error::{shape, Refusal};
use crate::ids::{Acf1Ref, AuthorityEpoch, Currency, OpaqueRef, Ref, Scope, TrialIdentity};
use crate::{check_array, check_int, nullable, schema_tag, Contract};
use serde::{Deserialize, Serialize};

schema_tag!(EpisodeSchema, "axon.closed-loop.episode/1");

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EpisodeStatus {
    Completed,
    Failed,
    Cancelled,
    OutcomeUnknown,
    Refused,
    Unsupported,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VerificationResult {
    NotRun,
    Passed,
    Failed,
    Unknown,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorpusRole {
    Discovery,
    Tuning,
    Confirmation,
    Reporting,
    MechanismTest,
}

/// Independent verification of the candidate output. A JSON `passed` does not
/// authenticate the verifier; [`crate::bind_episode`] checks the issuer against
/// caller-supplied premises.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EpisodeVerification {
    pub result: VerificationResult,
    pub matched_checks: u64,
    #[serde(deserialize_with = "nullable")]
    pub issuer_ref: Option<OpaqueRef>,
    #[serde(deserialize_with = "nullable")]
    pub verifier_ref: Option<Ref>,
    #[serde(deserialize_with = "nullable")]
    pub output_workspace_ref: Option<Acf1Ref>,
    pub evidence_refs: Vec<Ref>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UsageState {
    Final,
    Estimated,
    Unknown,
}

/// TEL usage. Money is µ-units of `currency` (1e-6), NOT MiCode's MicroCents
/// (1e-8); any join must convert explicitly.
///
/// UNKNOWN COST IS `None`, NEVER 0: `state: unknown` requires `cost_micro:
/// null`, and `state: final` requires a known cost with no unresolved
/// liability.
///
/// DEVIATION from contracts_proposal.md §8, in favour of the closed package
/// schema: `attempt_refs` are content `Ref`s (1..=4096, unique), not
/// `AttemptId`s, and the proposal's `model` / `execution` token/resource
/// breakdowns are absent — the schema has no such fields, so emitting them
/// would produce documents every other implementation refuses. They belong in
/// a negotiated `/2` of the episode schema.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub state: UsageState,
    #[serde(deserialize_with = "nullable")]
    pub cost_micro: Option<u64>,
    pub unresolved_liability_micro: u64,
    pub currency: Currency,
    pub price_schedule_ref: Ref,
    pub attempt_refs: Vec<Ref>,
}

impl Usage {
    pub(crate) fn validate(&self) -> Result<(), Refusal> {
        if let Some(c) = self.cost_micro {
            check_int("usage.cost_micro", c, 0)?;
        }
        check_int(
            "usage.unresolved_liability_micro",
            self.unresolved_liability_micro,
            0,
        )?;
        check_array("usage.attempt_refs", &self.attempt_refs, 1, 4096, true)?;
        match self.state {
            UsageState::Unknown if self.cost_micro.is_some() => {
                Err(shape("usage state unknown requires cost_micro = null"))
            }
            UsageState::Final if self.cost_micro.is_none() => Err(shape(
                "usage state final requires a known cost_micro (unknown is not 0)",
            )),
            UsageState::Final if self.unresolved_liability_micro != 0 => Err(shape(
                "usage state final requires unresolved_liability_micro = 0",
            )),
            _ => Ok(()),
        }
    }
}

/// `closed-loop-episode`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LoopEpisode {
    pub schema: EpisodeSchema,
    pub identity: TrialIdentity,
    pub scope: Scope,
    pub policy_ref: Ref,
    pub controls_ref: Ref,
    pub context_ref: Ref,
    pub candidate_set_ref: Ref,
    pub input_workspace_ref: Acf1Ref,
    #[serde(deserialize_with = "nullable")]
    pub output_workspace_ref: Option<Acf1Ref>,
    pub acf_request_ref: Ref,
    pub acf_receipt_ref: Ref,
    /// `cl22:`/`sha256:` digest of the original episode. An `axc1:` Cortex
    /// digest is not one of the profile's schemes, so a producer references
    /// the Cortex episode's bytes via `sha256:` of the same canonical bytes.
    pub source_episode_ref: Ref,
    pub authority_epoch: AuthorityEpoch,
    pub status: EpisodeStatus,
    pub verification: EpisodeVerification,
    pub usage: Usage,
    pub corpus_role: CorpusRole,
    pub data_use_ref: Ref,
    #[serde(deserialize_with = "nullable")]
    pub projection_ref: Option<Ref>,
}

impl Contract for LoopEpisode {
    fn validate(&self) -> Result<(), Refusal> {
        let v = &self.verification;
        check_int("verification.matched_checks", v.matched_checks, 0)?;
        check_array("verification.evidence_refs", &v.evidence_refs, 0, 256, true)?;
        self.usage.validate()?;
        if v.result == VerificationResult::Passed {
            if self.status != EpisodeStatus::Completed {
                return Err(shape("verification passed requires status completed"));
            }
            if v.matched_checks == 0 {
                return Err(shape("verification passed requires matched_checks > 0"));
            }
            if self.output_workspace_ref.is_none()
                || v.output_workspace_ref.is_none()
                || v.issuer_ref.is_none()
                || v.verifier_ref.is_none()
            {
                return Err(shape(
                    "verification passed requires output_workspace_ref, issuer_ref, verifier_ref and a checked output",
                ));
            }
            if v.evidence_refs.is_empty() {
                return Err(shape("verification passed requires evidence_refs"));
            }
        }
        if self.status == EpisodeStatus::OutcomeUnknown
            && !matches!(
                v.result,
                VerificationResult::NotRun | VerificationResult::Unknown
            )
        {
            return Err(shape(
                "status outcome_unknown admits only verification not_run/unknown",
            ));
        }
        Ok(())
    }
}
