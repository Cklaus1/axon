//! Strict ingest and the `cl22:` canonical digest.
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
use crate::{Contract, MAX_BYTES, MAX_DEPTH, MAX_INTEGER};
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

/// Strict untyped ingest: every rule of [`parse`] except the typed shape.
pub fn parse_value(json: &str) -> Result<Value, Refusal> {
    if json.len() > MAX_BYTES {
        return Err(Refusal::TooLarge(json.len()));
    }
    bounded_depth(json)?;
    let value: Value = axon_cortex::parse_strict(json).map_err(Refusal::Strict)?;
    json_tree(&value, 0)?;
    Ok(value)
}

/// The text pre-scan: count `[`/`{` outside strings, refuse past MAX_DEPTH.
/// Mirrors the reference's `_bounded_depth`; the parser still validates
/// balance and quoting.
fn bounded_depth(text: &str) -> Result<(), Refusal> {
    let (mut depth, mut quoted, mut escaped) = (0usize, false, false);
    for b in text.bytes() {
        if quoted {
            if escaped {
                escaped = false;
            } else if b == b'\\' {
                escaped = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else if b == b'"' {
            quoted = true;
        } else if b == b'[' || b == b'{' {
            depth += 1;
            if depth > MAX_DEPTH {
                return Err(Refusal::TooDeep);
            }
        } else if b == b']' || b == b'}' {
            depth = depth.saturating_sub(1);
        }
    }
    Ok(())
}

/// The value walk: mirrors the reference's `_json_tree` (keys count as one
/// level below their object, like values).
fn json_tree(v: &Value, depth: usize) -> Result<(), Refusal> {
    if depth > MAX_DEPTH {
        return Err(Refusal::TooDeep);
    }
    match v {
        Value::Null | Value::Bool(_) | Value::String(_) => Ok(()),
        Value::Number(n) => check_number(n),
        Value::Array(a) => a.iter().try_for_each(|x| json_tree(x, depth + 1)),
        Value::Object(o) => {
            if !o.is_empty() && depth + 1 > MAX_DEPTH {
                return Err(Refusal::TooDeep);
            }
            o.values().try_for_each(|x| json_tree(x, depth + 1))
        }
    }
}

fn check_number(n: &serde_json::Number) -> Result<(), Refusal> {
    if let Some(i) = n.as_i64() {
        if i.unsigned_abs() > MAX_INTEGER {
            return Err(Refusal::UnsafeInteger(n.to_string()));
        }
        Ok(())
    } else if let Some(u) = n.as_u64() {
        if u > MAX_INTEGER {
            return Err(Refusal::UnsafeInteger(n.to_string()));
        }
        Ok(())
    } else {
        // A fraction, an exponent, or an integer literal wider than u64 (which
        // serde_json without `arbitrary_precision` represents as f64). All are
        // outside the profile.
        Err(Refusal::Float)
    }
}

/// Canonical bytes of a JSON value under the `cl22` rule (see module docs).
/// Refuses what the reference refuses: floats, unsafe integers, excess depth,
/// and output over [`MAX_BYTES`].
pub fn canonical_bytes(v: &Value) -> Result<Vec<u8>, Refusal> {
    json_tree(v, 0)?;
    let mut out = Vec::new();
    write_canonical(v, &mut out);
    if out.len() > MAX_BYTES {
        return Err(Refusal::TooLarge(out.len()));
    }
    Ok(out)
}

/// Canonical bytes of any serializable contract value.
pub fn canonical_json<T: Serialize>(v: &T) -> Result<Vec<u8>, Refusal> {
    let value = serde_json::to_value(v).map_err(|e| shape(e.to_string()))?;
    canonical_bytes(&value)
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
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    crate::Ref::new(format!("cl22:{:x}", h.finalize())).expect("sha256 hex is a valid cl22 ref")
}

fn write_canonical(v: &Value, out: &mut Vec<u8>) {
    match v {
        Value::Null => out.extend_from_slice(b"null"),
        Value::Bool(true) => out.extend_from_slice(b"true"),
        Value::Bool(false) => out.extend_from_slice(b"false"),
        // Only integers reach here (json_tree ran first); Number's Display of
        // an integer is plain decimal.
        Value::Number(n) => out.extend_from_slice(n.to_string().as_bytes()),
        Value::String(s) => write_string(s, out),
        Value::Array(a) => {
            out.push(b'[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_canonical(x, out);
            }
            out.push(b']');
        }
        Value::Object(o) => {
            // Sort explicitly rather than rely on serde_json's map type, which
            // becomes insertion-ordered if any crate in the build enables
            // `preserve_order`. `str` Ord is UTF-8 byte order == code point
            // order, which is Python's `sort_keys` order.
            let mut keys: Vec<&String> = o.keys().collect();
            keys.sort();
            out.push(b'{');
            for (i, k) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(b',');
                }
                write_string(k, out);
                out.push(b':');
                write_canonical(&o[k], out);
            }
            out.push(b'}');
        }
    }
}

fn write_string(s: &str, out: &mut Vec<u8>) {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    out.push(b'"');
    for &b in s.as_bytes() {
        match b {
            b'"' => out.extend_from_slice(b"\\\""),
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'\n' => out.extend_from_slice(b"\\n"),
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\t' => out.extend_from_slice(b"\\t"),
            0x08 => out.extend_from_slice(b"\\b"),
            0x0c => out.extend_from_slice(b"\\f"),
            0x00..=0x1f => {
                out.extend_from_slice(b"\\u00");
                out.push(HEX[(b >> 4) as usize]);
                out.push(HEX[(b & 0xf) as usize]);
            }
            // Every other byte, including all UTF-8 continuation bytes, is
            // copied: ensure_ascii=False emits non-ASCII raw.
            _ => out.push(b),
        }
    }
    out.push(b'"');
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
