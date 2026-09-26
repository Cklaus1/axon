//! Axon Cortex v0.22 closed-loop shared contracts.
//!
//! PURE TYPES. No I/O, no authentication, no storage, no budget arithmetic,
//! no admission. What is here:
//!
//! * closed serde types for the package's `axon.closed-loop.*/1` family
//!   (policy, transition, context, episode) and the byte-preserved ACF
//!   `acf-compute-request/1` / `acf-execution-receipt/1` contracts;
//! * [`parse`]: strict ingest (duplicate / escaped-alias keys refused via
//!   `axon_cortex::parse_strict`; no floats; |int| ≤ 2^53−1; depth ≤ 32;
//!   input ≤ 1 MiB; `deny_unknown_fields`; required-nullable fields must be
//!   PRESENT — `null` is a value, absence is a refusal);
//! * [`digest`]: the package's `cl22:` sorted-key canonical digest;
//! * [`checks`]: the semantic checks that need no I/O;
//! * [`profile`]: `closed-loop-profile/1`, the B256 offer/accept negotiation
//!   (explicit `Unsupported`, never a fallback). Deliberately NOT re-exported
//!   at the crate root: consumers glob-import the root, and generic names
//!   like `Accept`/`Side` must not appear in their scope unasked.
//!
//! The JSON Schemas these types agree with are checked in under `schemas/`.
//! A JSON issuer field authenticates nothing, and passing a check here is never
//! runtime qualification.
//!
//! `axon-cortex` must never depend on this crate.

pub mod canonical;
pub mod checks;
pub mod compute;
pub mod context;
pub mod episode;
pub mod error;
pub mod ids;
pub mod policy;
pub mod profile;
pub mod receipt;
pub mod schema;

pub use canonical::{
    canonical_bytes, canonical_json, digest, digest_value, parse, parse_bytes, parse_value,
};
pub use checks::{
    bind_acf, bind_episode, check_context_current, check_paired_trial_context, check_shortlist,
    concrete_path, learning_eligible, project_receipt_status,
};
pub use compute::{
    Architecture, CheckpointKind, ComputeRequest, Engine, JobKind, Limits, NetworkMode, Os,
    Required,
};
pub use context::{ContextFacts, ExecutionContextReceipt, Role};
pub use episode::{
    CorpusRole, EpisodeStatus, EpisodeVerification, LoopEpisode, Usage, UsageState,
    VerificationResult,
};
pub use error::Refusal;
pub use ids::{
    Acf1Ref, ArmId, AttemptId, AuthorityEpoch, BoundedText, CandidateId, ContextId, Currency,
    ExecutionId, OpaqueRef, OperationId, PolicyId, PolicyVersion, Ref, RefScheme, RepoId, Scope,
    TaskFamily, TaskId, TenantId, TransitionId, TrialId, TrialIdentity, WorktreeId,
};
pub use policy::{
    PinAck, PolicyEnvelope, PolicyMode, PolicyPin, PolicyProjection, PolicyTransition,
    TransitionKind,
};
pub use receipt::{
    EvidenceSource, ExecutionReceipt, ReceiptStatus, ReceiptUsageState, ReceiptVerification,
};

/// 2^53 − 1: the largest integer every JSON implementation represents exactly.
pub const MAX_INTEGER: u64 = 9_007_199_254_740_991;
/// Input (and canonical output) byte limit: 1 MiB.
pub const MAX_BYTES: usize = 1_048_576;
/// Container nesting limit.
pub const MAX_DEPTH: usize = 32;

/// A top-level closed-loop contract: deserializable, and carrying the
/// schema-level conditionals and intrinsic semantic rules serde cannot express.
/// [`parse`] runs [`Contract::validate`] after typed deserialization; code that
/// builds a value directly should call it before emitting or digesting.
pub trait Contract: serde::de::DeserializeOwned + serde::Serialize {
    /// The JSON Schema text this contract's wire form must satisfy. [`parse`]
    /// validates the untyped document against it BEFORE typed serde, so serde's
    /// extra accepted encodings (struct-as-array, enum-as-object) never reach
    /// the typed layer.
    const SCHEMA: &'static str;
    fn validate(&self) -> Result<(), Refusal>;
}

/// Deserialize a REQUIRED field whose value may be `null`.
///
/// serde's default for `Option<T>` treats a missing key as `None`. The package
/// schemas list these fields as required, so "I did not say" must be refused
/// rather than read as "unknown". Using `deserialize_with` removes serde's
/// implicit default.
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(d)
}

/// A const schema tag, e.g. `"axon.closed-loop.policy/1"`: serializes as the
/// string and deserializes only from exactly that string.
macro_rules! schema_tag {
    ($name:ident, $tag:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        pub struct $name;
        impl $name {
            pub const TAG: &'static str = $tag;
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str($tag)
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                if s == $tag {
                    Ok($name)
                } else {
                    Err(serde::de::Error::custom(format!(
                        "schema must be {:?}, got {:?}",
                        $tag, s
                    )))
                }
            }
        }
    };
}
pub(crate) use schema_tag;

/// Array cardinality + uniqueness, as the schemas state them.
pub(crate) fn check_array<T: PartialEq>(
    what: &str,
    items: &[T],
    min: usize,
    max: usize,
    unique: bool,
) -> Result<(), Refusal> {
    if items.len() < min || items.len() > max {
        return Err(error::shape(format!(
            "{what}: {} items, expected {min}..={max}",
            items.len()
        )));
    }
    if unique {
        for (i, a) in items.iter().enumerate() {
            if items[..i].contains(a) {
                return Err(error::shape(format!("{what}: duplicate item")));
            }
        }
    }
    Ok(())
}

/// An integer field bounded to `min..=MAX_INTEGER`.
pub(crate) fn check_int(what: &str, v: u64, min: u64) -> Result<(), Refusal> {
    if v < min || v > MAX_INTEGER {
        return Err(error::shape(format!("{what}: {v} outside {min}..=2^53-1")));
    }
    Ok(())
}
