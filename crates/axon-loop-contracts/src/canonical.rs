//! Strict ingest and the `cl22:` canonical digest.
//!
//! The writer and the untyped strict parser live in the leaf crate
//! `axon-canonical` (NH-R1 §4: `axon-os` needs them, and this crate depends on
//! `axon-cortex`, so they could not stay here without a cycle); this module
//! re-exports them in this crate's [`Refusal`] and keeps the typed [`parse`].
//!
//! # The `cl22:` rule
//!
//! `"cl22:" + lowercase-hex(SHA-256(canonical bytes))`, where the canonical
//! bytes are the package reference's
//! `json.dumps(v, sort_keys=True, separators=(',', ':'), ensure_ascii=False)`
//! (tools/closed_loop_reference.py `canonical`), UTF-8 encoded:
//!
//! * object keys sorted by Unicode code point (== UTF-8 byte order), no
//!   whitespace anywhere;
//! * strings emitted verbatim except `"` `\\` and C0 controls: `\b \f \n \r \t`
//!   get their short escapes, every other byte below 0x20 is `\u00xx` with
//!   lowercase hex. Non-ASCII (including U+2028/9) and DEL are emitted raw —
//!   `ensure_ascii=False` — with no Unicode normalization;
//! * integers in plain decimal; no floats exist in this profile.
//!
//! This is deliberately NOT `axon_cortex::to_canonical_json`, which emits
//! declaration order and therefore is not a cross-language canonical form. It
//! is also NOT `axc1:` or `acf1:`: those remain distinct schemes and none is
//! reinterpreted as another.
//!
//! A digest is IDENTITY, not authenticity: equal digests say two documents are
//! the same bytes, and nothing about who produced either.

use crate::error::{shape, Refusal};
use crate::{Contract, MAX_BYTES};
use serde::Serialize;
use serde_json::Value;

/// Parse a closed-loop contract under the profile's strict JSON rules.
///
/// Order: byte limit → nesting pre-scan (so the parser never recurses past the
/// limit) → `axon_cortex::parse_strict` (duplicate and escaped-alias keys, as
/// DECODED keys) → number/depth walk (no floats, |n| ≤ 2^53−1) → typed serde
/// (`deny_unknown_fields`, validated newtypes, closed enums; `true` is never an
/// integer) → the type's schema-level and intrinsic semantic checks.
pub fn parse<T: Contract>(json: &str) -> Result<T, Refusal> {
    let value = parse_value(json)?;
    // Shape first, from the checked-in schema: an object where the schema says
    // object (never a positional array), a string where it says enum/const
    // (never `{"variant":null}`), no ignored fields — at every nesting level
    // (red-team D1-D3). Typed serde then runs as a second, narrower layer.
    let schema = crate::schema::load(T::SCHEMA)?;
    crate::schema::validate_against(&schema, &value)?;
    let typed: T = serde_json::from_value(value).map_err(|e| {
        let m = e.to_string();
        if m.contains("unknown field") || m.contains("unknown variant") {
            Refusal::Strict(axon_cortex::ContractError::UnknownVariantOrField(m))
        } else {
            shape(m)
        }
    })?;
    typed.validate()?;
    Ok(typed)
}

/// [`parse`] over raw bytes, refusing invalid UTF-8.
pub fn parse_bytes<T: Contract>(bytes: &[u8]) -> Result<T, Refusal> {
    if bytes.len() > MAX_BYTES {
        return Err(Refusal::TooLarge(bytes.len()));
    }
    let text = std::str::from_utf8(bytes).map_err(|e| shape(format!("invalid UTF-8: {e}")))?;
    parse(text)
}

/// Strict untyped ingest: every rule of [`parse`] except the typed shape
/// (`axon_canonical::parse_value`, in this crate's [`Refusal`]).
pub fn parse_value(json: &str) -> Result<Value, Refusal> {
    Ok(axon_canonical::parse_value(json)?)
}

/// Canonical bytes of a JSON value under the `cl22` rule (see module docs).
/// Refuses what the reference refuses: floats, unsafe integers, excess depth,
/// and output over [`MAX_BYTES`].
pub fn canonical_bytes(v: &Value) -> Result<Vec<u8>, Refusal> {
    Ok(axon_canonical::canonical_bytes(v)?)
}

/// Canonical bytes of any serializable contract value.
pub fn canonical_json<T: Serialize>(v: &T) -> Result<Vec<u8>, Refusal> {
    Ok(axon_canonical::canonical_json(v)?)
}

/// `cl22:` digest of a contract value.
pub fn digest<T: Serialize>(v: &T) -> Result<crate::Ref, Refusal> {
    Ok(cl22(&canonical_json(v)?))
}

/// `cl22:` digest of an untyped JSON value.
pub fn digest_value(v: &Value) -> Result<crate::Ref, Refusal> {
    Ok(cl22(&canonical_bytes(v)?))
}

fn cl22(bytes: &[u8]) -> crate::Ref {
    crate::Ref::new(axon_canonical::cl22(bytes)).expect("sha256 hex is a valid cl22 ref")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canon(json: &str) -> String {
        String::from_utf8(canonical_bytes(&parse_value(json).unwrap()).unwrap()).unwrap()
    }

    // Expected strings produced by the reference:
    //   python3 -c 'import json;print(json.dumps(V,sort_keys=True,
    //     separators=(",",":"),ensure_ascii=False))'
    #[test]
    fn matches_python_dumps_escaping_and_ordering() {
        assert_eq!(
            canon(r#"{"b":1,"a":[true,null,-5],"é":"x","Z":{}}"#),
            r#"{"Z":{},"a":[true,null,-5],"b":1,"é":"x"}"#
        );
        assert_eq!(
            canon(r#"["q\"b\\n\n\r\t\b\f\u0001\u001f\u007f é😀/"]"#),
            "[\"q\\\"b\\\\n\\n\\r\\t\\b\\f\\u0001\\u001f\u{7f}\u{2028}é😀/\"]"
        );
    }

    #[test]
    fn digest_of_known_value() {
        // python3 -c 'import hashlib;print(hashlib.sha256(b"{\"a\":1}").hexdigest())'
        assert_eq!(
            digest_value(&parse_value(r#"{ "a" : 1 }"#).unwrap())
                .unwrap()
                .as_str(),
            "cl22:015abd7f5cc57a2dd94b7590f04ad8084273905ee33ec5cebeae62276a97f862"
        );
    }

    #[test]
    fn strict_ingest_refusals() {
        assert!(matches!(parse_value("1.0"), Err(Refusal::Float)));
        assert!(matches!(parse_value("1e2"), Err(Refusal::Float)));
        assert!(matches!(
            parse_value("[18446744073709551616]"),
            Err(Refusal::Float)
        ));
        assert!(matches!(
            parse_value("9007199254740992"),
            Err(Refusal::UnsafeInteger(_))
        ));
        assert!(matches!(
            parse_value("-9007199254740992"),
            Err(Refusal::UnsafeInteger(_))
        ));
        assert!(parse_value("-9007199254740991").is_ok());
        assert!(matches!(
            parse_value(r#"{"a":1,"a":2}"#),
            Err(Refusal::Strict(axon_cortex::ContractError::DuplicateKey(_)))
        ));
        assert!(parse_value(r#""\ud800""#).is_err());
        let ok = format!("{}{}", "[".repeat(32), "]".repeat(32));
        assert!(parse_value(&ok).is_ok());
        let deep = format!("{}{}", "[".repeat(33), "]".repeat(33));
        assert!(matches!(parse_value(&deep), Err(Refusal::TooDeep)));
        // Brackets inside strings are not nesting.
        assert!(parse_value(&format!("\"{}\"", "[".repeat(100))).is_ok());
        let big = format!("\"{}\"", "a".repeat(MAX_BYTES));
        assert!(matches!(parse_value(&big), Err(Refusal::TooLarge(_))));
    }
}
