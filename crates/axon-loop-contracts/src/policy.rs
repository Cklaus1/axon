//! Contract A: the shortlist policy, its pin, and fenced transitions.
//!
//! A policy is a SUBTRACTIVE shortlist over an already-eligible candidate view.
//! It grants nothing, activates nothing, and cannot add a tool.

use crate::error::{semantic, shape, Refusal};
use crate::ids::{
    Acf1Ref, AuthorityEpoch, CandidateId, OpaqueRef, PolicyId, PolicyVersion, Ref, Scope,
    TransitionId,
};
use crate::{check_array, nullable, schema_tag, Contract};
use serde::{Deserialize, Serialize};

schema_tag!(PolicySchema, "axon.closed-loop.policy/1");
schema_tag!(TransitionSchema, "axon.closed-loop.transition/1");

/// The only mode in the 0.22 pilot.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyMode {
    ShortlistOnly,
}

/// `closed-loop-policy`: a versioned shortlist over an eligible candidate view
/// with a fixed control digest. Not permission, correctness or activation.
///
/// DEVIATION from contracts_proposal.md §4, in favour of the package schema
/// (which is normative): there is no `version` field and no `mechanism_test`
/// field on the envelope — the package schema is closed and has neither, so a
/// document carrying them would be refused by every other implementation. The
/// version is DERIVED ([`PolicyEnvelope::version`] = id + `cl22:` digest), and
/// `mechanism_test` lives on the transition and the episode's corpus role.
/// `parent_policy_ref` is REQUIRED and non-null in the package schema (a root
/// policy names a sentinel parent), so it is `Ref`, not `Option<Ref>`.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyEnvelope {
    pub schema: PolicySchema,
    pub policy_id: PolicyId,
    pub parent_policy_ref: Ref,
    pub scope: Scope,
    pub mode: PolicyMode,
    /// The already-eligible candidate view (tool/skill ids).
    pub candidate_set_ref: Ref,
    /// Fixed model/route/context/budget digest.
    pub controls_ref: Ref,
    /// Ordered; 1..=256 unique; must be ⊆ the candidate set (checked by
    /// [`crate::check_shortlist`], which needs the resolved set).
    pub shortlist: Vec<CandidateId>,
    pub discovery_evidence_refs: Vec<Ref>,
    /// Always `false`. A policy that claims to expand authority is refused.
    pub authority_expansion: bool,
}

impl PolicyEnvelope {
    /// The pinnable version: the id plus the `cl22:` digest of these bytes.
    pub fn version(&self) -> Result<PolicyVersion, Refusal> {
        Ok(PolicyVersion {
            policy_id: self.policy_id.clone(),
            digest: crate::digest(self)?,
        })
    }
}

impl Contract for PolicyEnvelope {
    const SCHEMA: &'static str = crate::schema::schema_text!("closed-loop-policy.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        if self.authority_expansion {
            return Err(semantic("authority_expansion must be false"));
        }
        // An empty shortlist is not expressible: the schema requires ≥1. A
        // policy with nothing to offer ABSTAINS by not being issued (a pause
        // transition), never by an empty list a consumer could read as "all".
        check_array("shortlist", &self.shortlist, 1, 256, true)?;
        check_array(
            "discovery_evidence_refs",
            &self.discovery_evidence_refs,
            0,
            256,
            true,
        )?;
        Ok(())
    }
}

/// What MiCode records when it pins a policy for a task.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyPin {
    pub version: PolicyVersion,
    pub epoch: AuthorityEpoch,
    pub pinned_at_ms: i64,
    pub ack: PinAck,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum PinAck {
    Acknowledged,
    Refused { reason: OpaqueRef },
}

impl Contract for PolicyPin {
    const SCHEMA: &'static str = crate::schema::schema_text!("local-policy-pin.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        if self.pinned_at_ms < 0 {
            return Err(shape("pinned_at_ms must be >= 0"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionKind {
    Activate,
    Rollback,
    Pause,
}

/// `closed-loop-transition`: a scoped expected-policy/epoch CAS INTENT. Not
/// authority to change active state — authentication and admission are
/// external.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyTransition {
    pub schema: TransitionSchema,
    pub transition_id: TransitionId,
    pub kind: TransitionKind,
    pub scope: Scope,
    pub expected_policy_ref: Ref,
    /// `null` iff `kind == pause`.
    #[serde(deserialize_with = "nullable")]
    pub target_policy_ref: Option<Ref>,
    pub expected_epoch: AuthorityEpoch,
    pub next_epoch: AuthorityEpoch,
    /// Required for activate/rollback.
    #[serde(deserialize_with = "nullable")]
    pub admission_ref: Option<Ref>,
    pub reason_ref: Ref,
    pub issuer_ref: OpaqueRef,
    pub mechanism_test: bool,
}

impl Contract for PolicyTransition {
    const SCHEMA: &'static str = crate::schema::schema_text!("closed-loop-transition.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        if self.next_epoch.get() < 1 {
            return Err(shape("next_epoch must be >= 1"));
        }
        if self.next_epoch.get() != self.expected_epoch.get() + 1 {
            return Err(semantic(format!(
                "non-monotonic/noncontiguous fence: next_epoch {} != expected_epoch {} + 1",
                self.next_epoch.get(),
                self.expected_epoch.get()
            )));
        }
        match self.kind {
            TransitionKind::Pause => {
                if self.target_policy_ref.is_some() {
                    return Err(shape("pause must have target_policy_ref = null"));
                }
            }
            TransitionKind::Activate | TransitionKind::Rollback => {
                if self.target_policy_ref.is_none() {
                    return Err(semantic(format!(
                        "{:?} requires target_policy_ref",
                        self.kind
                    )));
                }
                if self.admission_ref.is_none() {
                    return Err(semantic(format!("{:?} requires admission_ref", self.kind)));
                }
            }
        }
        Ok(())
    }
}

/// The closed projection from a sidecar policy digest to the supervisor's ACF
/// `policy_digest`. Structural only: production must authenticate and resolve
/// `projection_ref` before trusting the mapping.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyProjection {
    pub sidecar_policy_ref: Ref,
    pub acf_policy_digest: Acf1Ref,
    pub projection_ref: Ref,
}

impl Contract for PolicyProjection {
    const SCHEMA: &'static str = crate::schema::schema_text!("local-policy-projection.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        Ok(())
    }
}
