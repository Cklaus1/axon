//! B256: `closed-loop-profile/1` — explicit bridge-profile negotiation.
//!
//! Three wire families cross the Axon↔MiCode boundary, each at a FIXED
//! version, and until this module none of them was negotiated:
//! `cortex-policy-adapter` (`protocol_version: 1`), `axon-bridge/v0`, and the
//! `axon.closed-loop.*/1` sidecars. This module is the ONE document that says,
//! before any sidecar is exchanged, which exact schema ids, feature flags and
//! pinned adapters BOTH peers support.
//!
//! * An OFFER lists exact ids in three disjoint vocabularies (`schemas`,
//!   `features`, `adapters`) plus the subset of them the offerer `required`s.
//! * An ACCEPT selects a subset of the offer and binds itself to that exact
//!   offer by its `cl22:` digest (`offer_ref`).
//! * [`negotiate`] is the pure responder rule: the intersection of the offer
//!   with the responder's [`LocalCapabilities`], per vocabulary. An empty
//!   schema intersection, or a required id either side cannot agree, is an
//!   explicit [`Unsupported`] — NEVER a fallback to the first offered schema,
//!   to a pinned adapter, or to "whatever the peer said".
//! * [`confirm`] is the offerer's check of the accept it got back.
//! * [`negotiate_wire`] handles the peer that sends no profile at all (absent
//!   / old peer): [`Unsupported::NoOffer`] / [`Unsupported::NotAProfile`].
//!
//! Pinned adapters (`cortex-policy-adapter/1`, `axon-bridge/v0`) may be LISTED
//! and agreed like any other id, but they are NEVER INFERRED: an old peer's
//! document is not read as "speaks axon-bridge/v0", and no adapter is selected
//! because nothing else matched.
//!
//! `usage/2` (exact integer micro-cents, 1e-8, no rounding) is a FEATURE. The
//! only way to obtain a [`Usage2Permit`] is from an [`Agreement`] that
//! contains it, and an agreement only contains it when both the offer and the
//! local side list it. Nothing in this workspace emits Usage/2 today; the
//! permit is the gate that future code must pass.
//!
//! Byte-level rules: `docs/CLOSED_LOOP_PROFILE_NEGOTIATION.md`. Cross-language
//! vectors: `docs/closed-loop-profile/vectors.json`.
//!
//! Like every contract here, this authenticates nothing: `peer` names a party,
//! it does not prove that party sent the document.

use crate::error::{semantic, shape, Refusal};
use crate::{check_array, schema_tag, Contract, Ref, RefScheme};
use axon_cortex::ContractError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::BTreeSet;

/// The profile document's own schema tag.
pub const PROFILE_TAG: &str = "closed-loop-profile/1";
/// Prefix shared by every version of this profile; a document carrying
/// `closed-loop-profile/<other>` is a NEWER/OTHER profile, not an old peer.
pub const PROFILE_FAMILY_PREFIX: &str = "closed-loop-profile/";
/// The grammar of every schema / feature / adapter id.
pub const PROFILE_ID_PATTERN: &str = "^[a-z][a-z0-9.-]{0,95}/v?(0|[1-9][0-9]{0,3})$";
const PEER_PATTERN: &str = "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$";
/// Max items in each id list.
pub const MAX_IDS: usize = 64;

/// Feature: usage carried as exact integer micro-cents (1e-8 of the currency
/// unit), no rounding. Usage/1 is `cost_micro` (1e-6) with round-up
/// conversion (`axon-loop` `cost_micro_from_micro_cents`).
pub const FEATURE_USAGE2: &str = "usage/2";
/// Pinned adapter: the existing `cortex-policy-adapter` wire,
/// `protocol_version: 1`.
pub const ADAPTER_CORTEX_POLICY_V1: &str = "cortex-policy-adapter/1";
/// Pinned adapter: the existing MiCode `axon-bridge/v0` wire.
pub const ADAPTER_AXON_BRIDGE_V0: &str = "axon-bridge/v0";

/// The four closed-loop sidecar schemas this crate parses.
pub const CLOSED_LOOP_SCHEMAS: [&str; 4] = [
    "axon.closed-loop.context/1",
    "axon.closed-loop.episode/1",
    "axon.closed-loop.policy/1",
    "axon.closed-loop.transition/1",
];

/// `^[a-z][a-z0-9.-]{0,95}/v?(0|[1-9][0-9]{0,3})$`, hand-written.
pub fn is_profile_id(s: &str) -> bool {
    let Some((name, ver)) = s.split_once('/') else {
        return false;
    };
    let n = name.as_bytes();
    let name_ok = !n.is_empty()
        && n.len() <= 96
        && n[0].is_ascii_lowercase()
        && n.iter()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'-'));
    let digits = ver.strip_prefix('v').unwrap_or(ver).as_bytes();
    let ver_ok = !digits.is_empty()
        && digits.len() <= 4
        && digits.iter().all(u8::is_ascii_digit)
        && (digits[0] != b'0' || digits.len() == 1);
    name_ok && ver_ok
}

macro_rules! pattern_string {
    ($(#[$m:meta])* $name:ident, $pattern:expr) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(String);
        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self, Refusal> {
                let s = s.into();
                if crate::schema::pattern_matches($pattern, &s) != Ok(true) {
                    return Err(shape(format!(
                        concat!(stringify!($name), " {:?} does not match {}"),
                        s, $pattern
                    )));
                }
                Ok($name(s))
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }
        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                $name::new(String::deserialize(d)?).map_err(serde::de::Error::custom)
            }
        }
    };
}

pattern_string!(
    /// An exact schema / feature / adapter id. Compared BYTE FOR BYTE: no case
    /// folding, no version ranges, no "1.x is compatible with 1".
    ProfileId, PROFILE_ID_PATTERN);
pattern_string!(
    /// The party a profile document names. A label, not a credential.
    PeerId, PEER_PATTERN);

schema_tag!(ProfileSchema, "closed-loop-profile/1");
schema_tag!(OfferRole, "offer");
schema_tag!(AcceptRole, "accept");

/// What one peer offers.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileOffer {
    pub schema: ProfileSchema,
    pub role: OfferRole,
    pub peer: PeerId,
    pub schemas: Vec<ProfileId>,
    pub features: Vec<ProfileId>,
    pub adapters: Vec<ProfileId>,
    /// Ids (from any of the three lists) without which the offerer will not
    /// proceed. Each must appear in `schemas`, `features` or `adapters`.
    pub required: Vec<ProfileId>,
}

/// What the responder selects from an offer.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProfileAccept {
    pub schema: ProfileSchema,
    pub role: AcceptRole,
    pub peer: PeerId,
    /// `cl22:` digest of the exact offer this accepts.
    pub offer_ref: Ref,
    pub schemas: Vec<ProfileId>,
    pub features: Vec<ProfileId>,
    pub adapters: Vec<ProfileId>,
}

fn lists_disjoint(lists: [(&str, &[ProfileId]); 3]) -> Result<(), Refusal> {
    let mut seen: BTreeSet<&ProfileId> = BTreeSet::new();
    for (_, l) in &lists {
        for id in *l {
            if !seen.insert(id) {
                return Err(semantic(format!(
                    "id {id} appears in more than one of schemas/features/adapters"
                )));
            }
        }
    }
    for id in lists[0].1 {
        if id.as_str().starts_with(PROFILE_FAMILY_PREFIX) {
            return Err(semantic(format!(
                "{id}: the profile document is not itself a negotiable schema"
            )));
        }
    }
    Ok(())
}

impl Contract for ProfileOffer {
    const SCHEMA: &'static str = crate::schema::schema_text!("closed-loop-profile.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        check_array("offer.schemas", &self.schemas, 1, MAX_IDS, true)?;
        check_array("offer.features", &self.features, 0, MAX_IDS, true)?;
        check_array("offer.adapters", &self.adapters, 0, MAX_IDS, true)?;
        check_array("offer.required", &self.required, 0, MAX_IDS, true)?;
        lists_disjoint([
            ("schemas", &self.schemas),
            ("features", &self.features),
            ("adapters", &self.adapters),
        ])?;
        for r in &self.required {
            if !(self.schemas.contains(r) || self.features.contains(r) || self.adapters.contains(r))
            {
                return Err(semantic(format!("required {r} is not offered")));
            }
        }
        Ok(())
    }
}

impl Contract for ProfileAccept {
    const SCHEMA: &'static str = crate::schema::schema_text!("closed-loop-profile.schema.json");
    fn validate(&self) -> Result<(), Refusal> {
        check_array("accept.schemas", &self.schemas, 1, MAX_IDS, true)?;
        check_array("accept.features", &self.features, 0, MAX_IDS, true)?;
        check_array("accept.adapters", &self.adapters, 0, MAX_IDS, true)?;
        lists_disjoint([
            ("schemas", &self.schemas),
            ("features", &self.features),
            ("adapters", &self.adapters),
        ])?;
        if self.offer_ref.scheme() != RefScheme::Cl22 {
            return Err(semantic("offer_ref must be a cl22: digest"));
        }
        Ok(())
    }
}

/// What THIS side supports. Built by local configuration, never from a peer
/// document.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct LocalCapabilities {
    pub peer: PeerId,
    pub schemas: BTreeSet<ProfileId>,
    pub features: BTreeSet<ProfileId>,
    pub adapters: BTreeSet<ProfileId>,
    pub required: BTreeSet<ProfileId>,
}

fn ids(xs: &[&str]) -> Result<BTreeSet<ProfileId>, Refusal> {
    xs.iter().map(|x| ProfileId::new(*x)).collect()
}

impl LocalCapabilities {
    /// Validated constructor.
    pub fn new(
        peer: &str,
        schemas: &[&str],
        features: &[&str],
        adapters: &[&str],
        required: &[&str],
    ) -> Result<Self, Refusal> {
        let c = LocalCapabilities {
            peer: PeerId::new(peer)?,
            schemas: ids(schemas)?,
            features: ids(features)?,
            adapters: ids(adapters)?,
            required: ids(required)?,
        };
        c.validate()?;
        Ok(c)
    }

    /// What this Axon build actually implements: the four closed-loop sidecar
    /// schemas this crate parses, NO features (Usage/2 is implemented nowhere
    /// in the workspace), and the `cortex-policy-adapter/1` wire. It does not
    /// claim `axon-bridge/v0`, which is MiCode's wire, not an Axon one.
    pub fn axon_v022() -> Self {
        LocalCapabilities::new(
            "axon",
            &CLOSED_LOOP_SCHEMAS,
            &[],
            &[ADAPTER_CORTEX_POLICY_V1],
            &[],
        )
        .expect("static capabilities are valid")
    }

    pub fn validate(&self) -> Result<(), Refusal> {
        // Exactly the offer rules: at least one schema, bounded, disjoint
        // vocabularies, required ⊆ listed.
        self.offer().validate()
    }

    /// The offer this side would send: every list in ascending byte order.
    pub fn offer(&self) -> ProfileOffer {
        let v = |s: &BTreeSet<ProfileId>| s.iter().cloned().collect::<Vec<_>>();
        ProfileOffer {
            schema: ProfileSchema,
            role: OfferRole,
            peer: self.peer.clone(),
            schemas: v(&self.schemas),
            features: v(&self.features),
            adapters: v(&self.adapters),
            required: v(&self.required),
        }
    }
}

/// Which side declared a requirement that could not be agreed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Offerer,
    Local,
}

/// Every way negotiation does NOT produce an agreement. Each is explicit; none
/// carries a fallback selection.
#[derive(Clone, PartialEq, Debug)]
pub enum Unsupported {
    /// The peer sent no profile document at all (absent peer).
    NoOffer,
    /// The peer sent a document that is not a `closed-loop-profile/*` document
    /// (an old peer, e.g. a `cortex-policy-adapter` v1 request or an
    /// `axon-bridge/v0` artifact). `schema` is what it carried, for the
    /// operator only: it is NOT used to infer a pinned adapter.
    NotAProfile { schema: Option<String> },
    /// `closed-loop-profile/<n>` with n ≠ 1.
    UnknownProfileVersion(String),
    /// The document broke a strict-parse or shape/semantic rule.
    Refused(Refusal),
    /// Local capabilities are themselves invalid.
    InvalidLocal(Refusal),
    /// The offer and this side share no schema id.
    NoCommonSchema,
    /// A required id is not in the agreement.
    RequiredNotAgreed { id: String, side: Side },
    /// An accept names an offer other than the one sent.
    OfferMismatch,
    /// An accept selects an id that was not offered / is not supported locally.
    NotOffered { id: String },
    /// A caller asked for a feature the agreement does not contain.
    FeatureNotAgreed(String),
}

impl Unsupported {
    /// Stable cross-language code (the `expect.unsupported` value in
    /// `docs/closed-loop-profile/vectors.json`).
    pub fn code(&self) -> &'static str {
        match self {
            Unsupported::NoOffer => "no_offer",
            Unsupported::NotAProfile { .. } => "not_a_profile",
            Unsupported::UnknownProfileVersion(_) => "unknown_profile_version",
            Unsupported::Refused(_) => "refused",
            Unsupported::InvalidLocal(_) => "invalid_local",
            Unsupported::NoCommonSchema => "no_common_schema",
            Unsupported::RequiredNotAgreed { .. } => "required_not_agreed",
            Unsupported::OfferMismatch => "offer_mismatch",
            Unsupported::NotOffered { .. } => "not_offered",
            Unsupported::FeatureNotAgreed(_) => "feature_not_agreed",
        }
    }

    /// For [`Unsupported::Refused`], the refusal class (the vectors'
    /// `expect.refusal`).
    pub fn refusal_class(&self) -> Option<&'static str> {
        let Unsupported::Refused(r) = self else {
            return None;
        };
        Some(match r {
            Refusal::Strict(ContractError::DuplicateKey(_)) => "duplicate_key",
            Refusal::Strict(ContractError::UnknownVariantOrField(_)) => "unknown_field",
            Refusal::Strict(_) => "malformed",
            Refusal::TooLarge(_) => "too_large",
            Refusal::TooDeep => "too_deep",
            Refusal::Float => "float",
            Refusal::UnsafeInteger(_) => "unsafe_integer",
            Refusal::Shape(_) => "shape",
            Refusal::Semantic(_) => "semantic",
        })
    }
}

impl std::fmt::Display for Unsupported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Unsupported::NoOffer => write!(f, "peer sent no closed-loop profile"),
            Unsupported::NotAProfile { schema } => {
                write!(f, "peer document is not a closed-loop profile ({schema:?})")
            }
            Unsupported::UnknownProfileVersion(v) => write!(f, "unknown profile version {v}"),
            Unsupported::Refused(r) => write!(f, "profile refused: {r}"),
            Unsupported::InvalidLocal(r) => write!(f, "local capabilities invalid: {r}"),
            Unsupported::NoCommonSchema => write!(f, "no common schema"),
            Unsupported::RequiredNotAgreed { id, side } => {
                write!(f, "required {id} ({side:?}) not agreed")
            }
            Unsupported::OfferMismatch => write!(f, "accept does not bind the offer sent"),
            Unsupported::NotOffered { id } => write!(f, "accept selects un-offered {id}"),
            Unsupported::FeatureNotAgreed(id) => write!(f, "feature {id} not agreed"),
        }
    }
}

impl std::error::Error for Unsupported {}

/// The bilateral result. Constructible only by [`negotiate`] / [`confirm`],
/// so holding one means the rule ran.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Agreement {
    schemas: BTreeSet<ProfileId>,
    features: BTreeSet<ProfileId>,
    adapters: BTreeSet<ProfileId>,
}

/// Proof that both peers agreed `usage/2`. Only [`Agreement::usage2`] makes one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Usage2Permit {
    _private: (),
}

impl Agreement {
    pub fn schemas(&self) -> impl Iterator<Item = &str> {
        self.schemas.iter().map(ProfileId::as_str)
    }
    pub fn features(&self) -> impl Iterator<Item = &str> {
        self.features.iter().map(ProfileId::as_str)
    }
    pub fn adapters(&self) -> impl Iterator<Item = &str> {
        self.adapters.iter().map(ProfileId::as_str)
    }
    pub fn has_schema(&self, id: &str) -> bool {
        self.schemas.iter().any(|s| s.as_str() == id)
    }
    pub fn has_adapter(&self, id: &str) -> bool {
        self.adapters.iter().any(|s| s.as_str() == id)
    }
    pub fn require_feature(&self, id: &str) -> Result<(), Unsupported> {
        if self.features.iter().any(|s| s.as_str() == id) {
            Ok(())
        } else {
            Err(Unsupported::FeatureNotAgreed(id.to_string()))
        }
    }
    /// The only way to obtain a [`Usage2Permit`].
    pub fn usage2(&self) -> Result<Usage2Permit, Unsupported> {
        self.require_feature(FEATURE_USAGE2)?;
        Ok(Usage2Permit { _private: () })
    }
    fn contains(&self, id: &ProfileId) -> bool {
        self.schemas.contains(id) || self.features.contains(id) || self.adapters.contains(id)
    }
    fn to_accept(&self, peer: PeerId, offer_ref: Ref) -> ProfileAccept {
        let v = |s: &BTreeSet<ProfileId>| s.iter().cloned().collect::<Vec<_>>();
        ProfileAccept {
            schema: ProfileSchema,
            role: AcceptRole,
            peer,
            offer_ref,
            schemas: v(&self.schemas),
            features: v(&self.features),
            adapters: v(&self.adapters),
        }
    }
}

/// The responder's result: the document to send back, and the agreement it
/// encodes.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Accept {
    pub document: ProfileAccept,
    pub agreement: Agreement,
}

fn common(offered: &[ProfileId], local: &BTreeSet<ProfileId>) -> BTreeSet<ProfileId> {
    offered
        .iter()
        .filter(|id| local.contains(*id))
        .cloned()
        .collect()
}

fn required_agreed(
    agreement: &Agreement,
    offer_required: &[ProfileId],
    local: &LocalCapabilities,
) -> Result<(), Unsupported> {
    for (side, reqs) in [
        (Side::Offerer, offer_required.iter().collect::<Vec<_>>()),
        (Side::Local, local.required.iter().collect()),
    ] {
        for r in reqs {
            if !agreement.contains(r) {
                return Err(Unsupported::RequiredNotAgreed {
                    id: r.to_string(),
                    side,
                });
            }
        }
    }
    Ok(())
}

/// The responder rule. Pure: the accept is a function of `(offer, local)`.
///
/// Per vocabulary, select exactly the ids present in BOTH the offer and
/// `local` — never an id only one side lists. No common schema, or a required
/// id (either side's) outside the selection, is [`Unsupported`].
pub fn negotiate(offer: &ProfileOffer, local: &LocalCapabilities) -> Result<Accept, Unsupported> {
    offer.validate().map_err(Unsupported::Refused)?;
    local.validate().map_err(Unsupported::InvalidLocal)?;
    let schemas = common(&offer.schemas, &local.schemas);
    if schemas.is_empty() {
        return Err(Unsupported::NoCommonSchema);
    }
    let agreement = Agreement {
        schemas,
        features: common(&offer.features, &local.features),
        adapters: common(&offer.adapters, &local.adapters),
    };
    required_agreed(&agreement, &offer.required, local)?;
    let offer_ref = crate::digest(offer).map_err(Unsupported::Refused)?;
    Ok(Accept {
        document: agreement.to_accept(local.peer.clone(), offer_ref),
        agreement,
    })
}

/// The offerer's check of the accept it received for `offer`, which `local`
/// produced. Every accepted id must have been offered AND be locally
/// supported (per vocabulary), the accept must bind this exact offer, and
/// every requirement must be met.
pub fn confirm(
    offer: &ProfileOffer,
    accept: &ProfileAccept,
    local: &LocalCapabilities,
) -> Result<Agreement, Unsupported> {
    offer.validate().map_err(Unsupported::Refused)?;
    accept.validate().map_err(Unsupported::Refused)?;
    local.validate().map_err(Unsupported::InvalidLocal)?;
    if accept.offer_ref != crate::digest(offer).map_err(Unsupported::Refused)? {
        return Err(Unsupported::OfferMismatch);
    }
    let pick = |accepted: &[ProfileId],
                offered: &[ProfileId],
                mine: &BTreeSet<ProfileId>|
     -> Result<BTreeSet<ProfileId>, Unsupported> {
        let mut out = BTreeSet::new();
        for id in accepted {
            if !offered.contains(id) || !mine.contains(id) {
                return Err(Unsupported::NotOffered { id: id.to_string() });
            }
            out.insert(id.clone());
        }
        Ok(out)
    };
    let agreement = Agreement {
        schemas: pick(&accept.schemas, &offer.schemas, &local.schemas)?,
        features: pick(&accept.features, &offer.features, &local.features)?,
        adapters: pick(&accept.adapters, &offer.adapters, &local.adapters)?,
    };
    if agreement.schemas.is_empty() {
        return Err(Unsupported::NoCommonSchema);
    }
    required_agreed(&agreement, &offer.required, local)?;
    Ok(agreement)
}

/// [`negotiate`] over what actually arrived from the peer, which may be
/// nothing (`None`: absent peer) or an old peer's document.
///
/// Order: strict JSON ingest (duplicate / escaped-alias keys, floats, unsafe
/// integers, depth, size — refused even for an old peer's document) → the
/// `schema` field decides old-peer / other-version / this profile → the typed
/// strict parse → [`negotiate`].
pub fn negotiate_wire(
    offer: Option<&str>,
    local: &LocalCapabilities,
) -> Result<Accept, Unsupported> {
    let Some(text) = offer else {
        return Err(Unsupported::NoOffer);
    };
    let value = crate::parse_value(text).map_err(Unsupported::Refused)?;
    let Some(obj) = value.as_object() else {
        return Err(Unsupported::Refused(shape(
            "profile offer must be a JSON object",
        )));
    };
    match obj.get("schema").and_then(|s| s.as_str()) {
        Some(PROFILE_TAG) => {}
        Some(s) if s.starts_with(PROFILE_FAMILY_PREFIX) => {
            return Err(Unsupported::UnknownProfileVersion(s.to_string()));
        }
        other => {
            return Err(Unsupported::NotAProfile {
                schema: other.map(str::to_string),
            });
        }
    }
    // Decided here, before the typed parse, so which fault is reported for an
    // accept sent in place of an offer does not depend on field order.
    if obj.get("role").and_then(|r| r.as_str()) != Some("offer") {
        return Err(Unsupported::Refused(shape(
            "profile document is not role:offer",
        )));
    }
    let offer: ProfileOffer = crate::parse(text).map_err(Unsupported::Refused)?;
    negotiate(&offer, local)
}
