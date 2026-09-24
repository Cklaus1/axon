//! Axon Cortex v0.22 closed loop — the Axon-core lane.
//!
//! Durable, file-backed owners for the parts of the loop that the package
//! assumes exist and the source did not have (reconciliation.md §"greenfield
//! owners"):
//!
//! * [`pointer`] / [`epoch`] — the per-scope fenced active-policy pointer and
//!   its authority epoch (B258 epoch half, B278, B279). The ONLY way the
//!   pointer moves is [`pointer::transition`]: compare-and-swap on
//!   `(expected_policy_ref, expected_epoch)`, `next_epoch = expected + 1`,
//!   journalled before it is published.
//! * [`plan`] — the experiment register: a `closed-loop-pilot/1` plan,
//!   frozen by its `cl22:` digest (B274).
//! * [`evo`] — one bounded shortlist candidate from discovery evidence, with
//!   regularized hypothesis history (B273, B281).
//! * [`evl`] — evaluation of exact artifacts; unknown is never pass (B276).
//! * [`admission`] — the frozen plan rule applied deterministically:
//!   ACCEPT / REJECT / INCONCLUSIVE (B277).
//! * [`tel`] — whole-task economics over `Usage`; unknown stays unknown (B270).
//! * [`intake`] — a MiCode episode sidecar joined to a stored policy and its
//!   context receipt, recorded in the ledger (B269 Axon side).
//!
//! Every contract type comes from `axon-loop-contracts`; this crate adds only
//! its own STORE records (pointer, admission, evaluation, hypothesis, plan).
//!
//! What none of this is: authentication. Every "trusted" set here
//! (admitters, verifiers) is an operator-configured PREMISE read from the
//! store's `config.json`; an `issuer_ref` in a JSON document names a party, it
//! does not prove the document came from that party.

pub mod admission;
pub mod candidates;
pub mod epoch;
pub mod error;
pub mod evl;
pub mod evo;
pub mod intake;
pub mod ledger;
pub mod plan;
pub mod pointer;
pub mod rules;
pub mod store;
pub mod tasks;
pub mod tel;

pub use error::LoopError;
pub use store::Store;

/// Deserialize a REQUIRED field whose value may be `null` (a missing key is a
/// refusal, not `None`). Same rule as the contracts crate.
pub(crate) fn nullable<'de, D, T>(d: D) -> Result<Option<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: serde::Deserialize<'de>,
{
    <Option<T> as serde::Deserialize>::deserialize(d)
}

/// A string-valued const schema tag for this crate's own records.
macro_rules! record_tag {
    ($name:ident, $tag:literal) => {
        #[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
        pub struct $name;
        impl $name {
            pub const TAG: &'static str = $tag;
        }
        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(
                &self,
                s: S,
            ) -> ::std::result::Result<S::Ok, S::Error> {
                s.serialize_str($tag)
            }
        }
        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(
                d: D,
            ) -> ::std::result::Result<Self, D::Error> {
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
pub(crate) use record_tag;

/// The null-policy sentinel `cl22:000…0`: the `expected_policy_ref` of a
/// transition out of a PAUSED (or never-activated) pointer, and the package's
/// root-policy `parent_policy_ref`. A paused pointer has no active policy; the
/// transition schema requires a non-null `expected_policy_ref`, so "nothing"
/// needs a spelling, and this is the one the package fixtures already use.
pub fn null_policy_ref() -> axon_loop_contracts::Ref {
    axon_loop_contracts::Ref::new(format!("cl22:{}", "0".repeat(64))).expect("valid sentinel")
}

/// Milliseconds since the Unix epoch (wall clock; only used for display
/// timestamps, never for ordering — ordering is by authority epoch).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
