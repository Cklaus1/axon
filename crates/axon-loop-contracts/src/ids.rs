//! Identities and references: validated string newtypes.
//!
//! Every type here validates in BOTH directions of use: `new` refuses a bad
//! value, and so does `Deserialize`, so a document cannot smuggle an id past
//! the check that a constructor would have applied. They serialize as the bare
//! string (transparent), which is the wire form in every package schema.
//!
//! An id is an opaque scoped LABEL. It is not a grant and not a content
//! identity; a `Ref` identifies content but is neither a grant nor
//! authenticated.

use crate::error::{shape, Refusal};
use crate::MAX_INTEGER;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

macro_rules! validated_string {
    ($(#[$m:meta])* $name:ident, $check:path) => {
        $(#[$m])*
        #[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
        pub struct $name(String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self, Refusal> {
                let s = s.into();
                $check(&s).map_err(|why| {
                    shape(format!(concat!(stringify!($name), " {:?}: {}"), s, why))
                })?;
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

        impl TryFrom<String> for $name {
            type Error = Refusal;
            fn try_from(s: String) -> Result<Self, Refusal> {
                $name::new(s)
            }
        }

        impl From<$name> for String {
            fn from(v: $name) -> String {
                v.0
            }
        }

        impl Serialize for $name {
            fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
                s.serialize_str(&self.0)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
                let s = String::deserialize(d)?;
                $name::new(s).map_err(serde::de::Error::custom)
            }
        }
    };
}

/// The package id rule: `^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`.
///
/// The charset is `[A-Za-z0-9._:-]`, 1..=128 bytes, AND the first byte is
/// alphanumeric — the package schemas require that, and a Rust type that
/// accepted `.x` or `:x` would parse documents the schema refuses.
fn check_id(s: &str) -> Result<(), &'static str> {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 128 {
        return Err("length must be 1..=128 bytes");
    }
    if !b[0].is_ascii_alphanumeric() {
        return Err("must start with [A-Za-z0-9]");
    }
    if !b
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
    {
        return Err("charset is [A-Za-z0-9._:-]");
    }
    Ok(())
}

validated_string!(
    /// The stable semantic task. NEVER dedups a trial.
    TaskId, check_id);
validated_string!(
    /// `incumbent` / `challenger-<n>`, scoped to one experiment.
    ArmId, check_id);
validated_string!(
    /// New for every repeated run of (task, arm).
    TrialId, check_id);
validated_string!(
    /// New for every authorized execution inside a trial, retries included.
    AttemptId, check_id);
validated_string!(
    /// One per intended effect. Same id with changed input is a conflict.
    OperationId, check_id);
validated_string!(
    /// One per physical process or VM launch.
    ExecutionId, check_id);
validated_string!(
    /// A tool/skill id inside an already-eligible candidate view.
    CandidateId, check_id);
validated_string!(PolicyId, check_id);
validated_string!(ContextId, check_id);
validated_string!(TransitionId, check_id);
validated_string!(TenantId, check_id);
validated_string!(TaskFamily, check_id);
validated_string!(RepoId, check_id);
validated_string!(WorktreeId, check_id);

fn check_hex64(h: &str) -> Result<(), &'static str> {
    if h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f')) {
        Ok(())
    } else {
        Err("digest must be 64 lowercase hex")
    }
}

fn check_ref(s: &str) -> Result<(), &'static str> {
    let (scheme, hex) = s.split_once(':').ok_or("expected <scheme>:<hex>")?;
    RefScheme::from_prefix(scheme).ok_or("scheme must be cl22, acf1 or sha256")?;
    check_hex64(hex)
}

fn check_acf1(s: &str) -> Result<(), &'static str> {
    match s.strip_prefix("acf1:") {
        Some(h) => check_hex64(h),
        None => Err("expected acf1:<hex>"),
    }
}

/// Free-form opaque label (issuer, principal, grant, executable registry
/// entry, argv-free references): 1..=512 characters, counted as Unicode
/// scalar values to agree with JSON Schema `minLength`/`maxLength`.
fn check_opaque(s: &str) -> Result<(), &'static str> {
    let n = s.chars().count();
    if (1..=512).contains(&n) {
        Ok(())
    } else {
        Err("length must be 1..=512 characters")
    }
}

fn check_currency(s: &str) -> Result<(), &'static str> {
    if s.len() == 3 && s.bytes().all(|c| c.is_ascii_uppercase()) {
        Ok(())
    } else {
        Err("currency must match ^[A-Z]{3}$")
    }
}

validated_string!(
    /// Opaque content reference `<scheme>:<64 lowercase hex>`, scheme one of
    /// `cl22` / `acf1` / `sha256`. The schemes are distinct: a `cl22:` value is
    /// never reinterpreted as an `acf1:` one or vice versa. Identifies content;
    /// it is not a grant and it is not authenticated.
    Ref, check_ref);
validated_string!(
    /// A reference that the schema pins to the ACF scheme (`acf1:<hex>`):
    /// workspace versions, executable digests, ACF policy digests.
    Acf1Ref, check_acf1);
validated_string!(
    /// An opaque 1..=512-character label (issuer, principal, grant, …). NOT a
    /// credential: a JSON issuer field authenticates nothing.
    OpaqueRef, check_opaque);
validated_string!(
    /// Descriptive 1..=512-character text that is not a reference: a branch
    /// name, a base commit spelling, a working directory, a build namespace.
    BoundedText, check_opaque);
validated_string!(
    /// ISO-4217-shaped currency code, `^[A-Z]{3}$`.
    Currency, check_currency);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefScheme {
    Cl22,
    Acf1,
    Sha256,
}

impl RefScheme {
    fn from_prefix(p: &str) -> Option<RefScheme> {
        match p {
            "cl22" => Some(RefScheme::Cl22),
            "acf1" => Some(RefScheme::Acf1),
            "sha256" => Some(RefScheme::Sha256),
            _ => None,
        }
    }
}

impl Ref {
    pub fn scheme(&self) -> RefScheme {
        let (p, _) = self.0.split_once(':').expect("validated");
        RefScheme::from_prefix(p).expect("validated")
    }
    pub fn hex(&self) -> &str {
        self.0.split_once(':').expect("validated").1
    }
}

impl From<Acf1Ref> for Ref {
    fn from(a: Acf1Ref) -> Ref {
        Ref(a.0)
    }
}

impl PartialEq<Acf1Ref> for Ref {
    fn eq(&self, other: &Acf1Ref) -> bool {
        self.0 == other.0
    }
}

/// Monotonic per-scope authority fence. Bounded to the JSON-safe integer range
/// on construction and on deserialize.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct AuthorityEpoch(u64);

impl AuthorityEpoch {
    pub fn new(v: u64) -> Result<Self, Refusal> {
        if v > MAX_INTEGER {
            return Err(shape(format!("authority epoch {v} exceeds 2^53-1")));
        }
        Ok(AuthorityEpoch(v))
    }
    pub fn get(self) -> u64 {
        self.0
    }
    /// The successor epoch, refusing to leave the JSON-safe range.
    pub fn next(self) -> Result<Self, Refusal> {
        AuthorityEpoch::new(self.0 + 1)
    }
}

impl Serialize for AuthorityEpoch {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_u64(self.0)
    }
}

impl<'de> Deserialize<'de> for AuthorityEpoch {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        AuthorityEpoch::new(u64::deserialize(d)?).map_err(serde::de::Error::custom)
    }
}

/// The operator-assigned scope every closed-loop record lives in.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub tenant_id: TenantId,
    pub task_family: TaskFamily,
}

/// The six identities that join a trial's records. All six are mandatory on
/// every post-preflight record; a failure before preflight is TASK_NOT_STARTED
/// and has no identity of this shape to fabricate.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrialIdentity {
    pub task_id: TaskId,
    pub arm_id: ArmId,
    pub trial_id: TrialId,
    pub attempt_id: AttemptId,
    pub operation_id: OperationId,
    pub execution_id: ExecutionId,
}

/// A policy's identity for PINNING: its id plus the `cl22:` digest of the
/// envelope bytes. Ordering between versions is by [`AuthorityEpoch`], never by
/// comparing these strings.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyVersion {
    pub policy_id: PolicyId,
    pub digest: Ref,
}

#[cfg(test)]
mod tests {
    /// C9 round 7, EQGATE3 (amendment 91): `AuthorityEpoch::new` is `pub` and
    /// refuses a value past 2^53-1. The refusal was exempted as UNREACHABLE (a
    /// pointer's epoch only grows by one); a caller of `new` reaches it.
    #[test]
    fn an_authority_epoch_past_the_json_safe_range_is_refused() {
        AuthorityEpoch::new(MAX_INTEGER).expect("control: the largest safe epoch");
        let got = AuthorityEpoch::new(MAX_INTEGER + 1);
        assert!(
            got.as_ref()
                .is_err_and(|e| e.to_string().contains("exceeds 2^53-1")),
            "ATTACK: an authority epoch past 2^53-1 was accepted: {got:?}"
        );
    }

    use super::*;

    #[test]
    fn id_rule_matches_the_package_pattern() {
        for ok in ["a", "A.b_c:d-e", "0", &"x".repeat(128)] {
            assert!(TaskId::new(ok).is_ok(), "{ok}");
        }
        for bad in [
            "",
            ".a",
            "-a",
            ":a",
            "_a",
            "a b",
            "a/b",
            "é",
            &"x".repeat(129),
        ] {
            assert!(TaskId::new(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn deserialize_validates_too() {
        assert!(serde_json::from_str::<TrialId>("\"ok-1\"").is_ok());
        assert!(serde_json::from_str::<TrialId>("\"bad id\"").is_err());
        assert!(
            serde_json::from_str::<Ref>(&format!("\"cl22:{}\"", "A".repeat(64))).is_err(),
            "ATTACK: a Ref deserialized from a digest that is not lowercase hex"
        );
        assert!(
            serde_json::from_str::<Acf1Ref>(&format!("\"cl22:{}\"", "a".repeat(64))).is_err(),
            "ATTACK: an Acf1Ref deserialized from a reference of another scheme"
        );
        assert!(serde_json::from_str::<AuthorityEpoch>("9007199254740992").is_err());
        assert!(serde_json::from_str::<AuthorityEpoch>("true").is_err());
    }

    #[test]
    fn ref_parts() {
        let r = Ref::new(format!("acf1:{}", "0".repeat(64))).unwrap();
        assert_eq!(r.scheme(), RefScheme::Acf1);
        assert_eq!(r.hex(), "0".repeat(64));
        assert!(Ref::new(format!("axc1:{}", "0".repeat(64))).is_err());
        assert!(Ref::new(format!("cl22:{}", "0".repeat(63))).is_err());
    }

    #[test]
    fn opaque_counts_characters_not_bytes() {
        assert!(OpaqueRef::new("é".repeat(512)).is_ok());
        assert!(OpaqueRef::new("é".repeat(513)).is_err());
        assert!(OpaqueRef::new("").is_err());
    }
}
