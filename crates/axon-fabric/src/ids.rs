//! Opaque, validated identifiers used as journal keys.
//!
//! ADAPTER POINT (axon-loop-contracts): `OpKey` and `ScopeKey` stand in for the
//! contracts crate's `OperationId` / task-or-experiment id, which another lane
//! is building concurrently. They are intentionally NOT a second definition of
//! those types: they carry no structure, only a validated string, so switching
//! means implementing `From<contracts::OperationId> for OpKey` (or replacing
//! this module's types with re-exports) — nothing else in the crate inspects
//! the string. Do not add fields or parsing here.

use serde::{Deserialize, Serialize};

fn valid_key(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b':'))
}

macro_rules! opaque_key {
    ($name:ident, $what:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
        #[serde(try_from = "String", into = "String")]
        pub struct $name(String);

        impl $name {
            pub fn new(s: impl Into<String>) -> Result<Self, String> {
                let s = s.into();
                if valid_key(&s) {
                    Ok($name(s))
                } else {
                    Err(format!(
                        concat!("invalid ", $what, " {:?}: 1-128 chars of [A-Za-z0-9._:-]"),
                        s
                    ))
                }
            }
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
        impl TryFrom<String> for $name {
            type Error = String;
            fn try_from(s: String) -> Result<Self, String> {
                $name::new(s)
            }
        }
        impl From<$name> for String {
            fn from(k: $name) -> String {
                k.0
            }
        }
        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }
    };
}

opaque_key!(OpKey, "operation id");
opaque_key!(ScopeKey, "budget scope id");

/// `sha256:<64 lowercase hex>` digest of an operation's immutable input.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct InputDigest(String);

impl InputDigest {
    pub fn of(bytes: &[u8]) -> Self {
        use sha2::{Digest, Sha256};
        InputDigest(format!("sha256:{:x}", Sha256::digest(bytes)))
    }
    pub fn parse(s: &str) -> Result<Self, String> {
        let ok = s.strip_prefix("sha256:").is_some_and(|h| {
            h.len() == 64
                && h.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        });
        if ok {
            Ok(InputDigest(s.to_string()))
        } else {
            Err(format!(
                "invalid input digest {s:?}: want sha256:<64 lowercase hex>"
            ))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for InputDigest {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        InputDigest::parse(&s)
    }
}
impl From<InputDigest> for String {
    fn from(d: InputDigest) -> String {
        d.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_validated() {
        assert!(OpKey::new("op-1:a.b_c").is_ok());
        for bad in ["", "has space", "slash/no", "nl\n", &"x".repeat(129)] {
            assert!(OpKey::new(bad).is_err(), "{bad:?}");
        }
        assert!(serde_json::from_str::<OpKey>("\"bad key\"").is_err());
    }

    #[test]
    fn digests_are_validated() {
        let d = InputDigest::of(b"abc");
        assert!(InputDigest::parse(d.as_str()).is_ok());
        assert!(InputDigest::parse("sha256:ABC").is_err());
        assert!(InputDigest::parse(&d.as_str().replace("sha256", "md5")).is_err());
    }
}
