//! Schema-driven shape walk: ONE rule for what a document may look like,
//! applied to the parsed `Value` BEFORE typed serde.
//!
//! Why it exists (red-team D1–D3). serde's derived `Deserialize` is more
//! liberal than the wire contract in ways `deny_unknown_fields` does not touch:
//!
//! * a struct also deserializes from a positional JSON ARRAY (`visit_seq`), so
//!   `"scope":["t","f"]` — and even a whole policy as a 10-element array — was
//!   accepted, and because the typed value re-serializes as an object the
//!   non-conforming bytes got the SAME `cl22:` digest as the conforming ones;
//! * a unit enum also deserializes from `{"variant":null}`;
//! * an internally tagged unit variant ignores sibling fields.
//!
//! Patching each type's visitor would be one fix per type and per future type.
//! Instead every [`crate::Contract`] names its JSON Schema (the checked-in
//! package bytes, or a crate-local schema for the two contracts the package
//! does not define) and this module validates the value against it. The
//! validator implements exactly the keyword subset the checked-in schemas use;
//! any other keyword is REFUSED (fail closed), and a test walks every
//! checked-in schema to prove none uses an unsupported keyword or pattern.

use crate::error::{shape, Refusal};
use axon_cortex::ContractError;
use serde_json::{Map, Value};

/// Keywords that carry no validation meaning.
const ANNOTATIONS: &[&str] = &["$schema", "$id", "title", "description"];

/// Keywords this validator enforces.
const SUPPORTED: &[&str] = &[
    "type",
    "properties",
    "additionalProperties",
    "required",
    "const",
    "enum",
    "pattern",
    "minLength",
    "maxLength",
    "minimum",
    "maximum",
    "items",
    "minItems",
    "maxItems",
    "uniqueItems",
    "anyOf",
    "oneOf",
    "allOf",
    "if",
    "then",
];

/// Validate `value` against `schema` (both JSON). `Ok` iff it conforms.
pub fn validate_against(schema: &Value, value: &Value) -> Result<(), Refusal> {
    walk(schema, value, "$")
}

fn fail(path: &str, msg: impl std::fmt::Display) -> Refusal {
    shape(format!("{path}: {msg}"))
}

fn walk(schema: &Value, v: &Value, path: &str) -> Result<(), Refusal> {
    let s = match schema {
        Value::Bool(true) => return Ok(()),
        Value::Bool(false) => return Err(fail(path, "schema `false` admits nothing")),
        Value::Object(s) => s,
        _ => return Err(fail(path, "malformed schema node")),
    };
    for k in s.keys() {
        if !SUPPORTED.contains(&k.as_str()) && !ANNOTATIONS.contains(&k.as_str()) {
            return Err(fail(path, format!("unsupported schema keyword {k:?}")));
        }
    }
    if let Some(t) = s.get("type") {
        check_type(t, v, path)?;
    }
    if let Some(c) = s.get("const") {
        if c != v {
            return Err(fail(path, format!("must be {c}")));
        }
    }
    if let Some(Value::Array(opts)) = s.get("enum") {
        if !opts.contains(v) {
            return Err(fail(
                path,
                format!("{v} is not one of {}", Value::Array(opts.clone())),
            ));
        }
    }
    match v {
        Value::String(st) => string_rules(s, st, path)?,
        Value::Number(n) => number_rules(s, n, path)?,
        Value::Array(a) => array_rules(s, a, path)?,
        Value::Object(o) => object_rules(s, o, path)?,
        Value::Null | Value::Bool(_) => {}
    }
    if let Some(Value::Array(subs)) = s.get("allOf") {
        for sub in subs {
            walk(sub, v, path)?;
        }
    }
    if let Some(Value::Array(subs)) = s.get("anyOf") {
        if !subs.iter().any(|sub| walk(sub, v, path).is_ok()) {
            return Err(fail(path, "matches none of anyOf"));
        }
    }
    if let Some(Value::Array(subs)) = s.get("oneOf") {
        let n = subs.iter().filter(|sub| walk(sub, v, path).is_ok()).count();
        if n != 1 {
            return Err(fail(
                path,
                format!("matches {n} of oneOf, expected exactly 1"),
            ));
        }
    }
    if let Some(cond) = s.get("if") {
        if walk(cond, v, path).is_ok() {
            if let Some(then) = s.get("then") {
                walk(then, v, path)?;
            }
        }
    }
    Ok(())
}

fn type_matches(t: &str, v: &Value) -> Result<bool, String> {
    Ok(match t {
        "object" => v.is_object(),
        "array" => v.is_array(),
        "string" => v.is_string(),
        "boolean" => v.is_boolean(),
        "null" => v.is_null(),
        // Floats never reach here (strict ingest refuses them); a Number is
        // an integer. A boolean is NOT an integer.
        "integer" => v.is_i64() || v.is_u64(),
        other => return Err(format!("unsupported type {other:?}")),
    })
}

fn check_type(t: &Value, v: &Value, path: &str) -> Result<(), Refusal> {
    let names: Vec<&str> = match t {
        Value::String(one) => vec![one.as_str()],
        Value::Array(many) => many.iter().filter_map(Value::as_str).collect(),
        _ => return Err(fail(path, "malformed `type`")),
    };
    for n in &names {
        let matched = type_matches(n, v).map_err(|e| fail(path, e))?;
        if matched {
            return Ok(());
        }
    }
    Err(fail(path, format!("{} is not of type {}", kind(v), t)))
}

fn kind(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "integer",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

fn usize_kw(s: &Map<String, Value>, k: &str) -> Option<usize> {
    s.get(k).and_then(Value::as_u64).map(|n| n as usize)
}

fn string_rules(s: &Map<String, Value>, st: &str, path: &str) -> Result<(), Refusal> {
    // Code points, as JSON Schema (and the Python reference) count length.
    let n = st.chars().count();
    if let Some(min) = usize_kw(s, "minLength") {
        if n < min {
            return Err(fail(path, format!("shorter than {min}")));
        }
    }
    if let Some(max) = usize_kw(s, "maxLength") {
        if n > max {
            return Err(fail(path, format!("longer than {max}")));
        }
    }
    if let Some(p) = s.get("pattern") {
        let p = p.as_str().ok_or_else(|| fail(path, "malformed pattern"))?;
        let ok = pattern_matches(p, st).map_err(|e| fail(path, e))?;
        if !ok {
            return Err(fail(path, format!("{st:?} does not match {p:?}")));
        }
    }
    Ok(())
}

/// The five anchored patterns the checked-in schemas use, as hand-written
/// matchers (no regex dependency). Any other pattern is refused rather than
/// silently treated as matching.
pub(crate) fn pattern_matches(p: &str, st: &str) -> Result<bool, String> {
    let hex64 =
        |h: &str| h.len() == 64 && h.bytes().all(|c| matches!(c, b'0'..=b'9' | b'a'..=b'f'));
    Ok(match p {
        "^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$" => {
            let b = st.as_bytes();
            !b.is_empty()
                && b.len() <= 128
                && b[0].is_ascii_alphanumeric()
                && b.iter()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b':' | b'-'))
        }
        "^(cl22|acf1|sha256):[0-9a-f]{64}$" => st
            .split_once(':')
            .is_some_and(|(sch, h)| matches!(sch, "cl22" | "acf1" | "sha256") && hex64(h)),
        "^acf1:[0-9a-f]{64}$" => st.strip_prefix("acf1:").is_some_and(hex64),
        "^[A-Z]{3}$" => st.len() == 3 && st.bytes().all(|c| c.is_ascii_uppercase()),
        crate::profile::PROFILE_ID_PATTERN => crate::profile::is_profile_id(st),
        other => return Err(format!("unsupported pattern {other:?}")),
    })
}

fn as_i128(n: &serde_json::Number) -> Option<i128> {
    n.as_i64()
        .map(i128::from)
        .or_else(|| n.as_u64().map(i128::from))
}

fn number_rules(s: &Map<String, Value>, n: &serde_json::Number, path: &str) -> Result<(), Refusal> {
    let Some(x) = as_i128(n) else {
        return Err(fail(path, "non-integer number"));
    };
    let bound = |k: &str| s.get(k).and_then(Value::as_number).and_then(as_i128);
    if let Some(min) = bound("minimum") {
        if x < min {
            return Err(fail(path, format!("{x} is less than the minimum of {min}")));
        }
    }
    if let Some(max) = bound("maximum") {
        if x > max {
            return Err(fail(
                path,
                format!("{x} is greater than the maximum of {max}"),
            ));
        }
    }
    Ok(())
}

fn array_rules(s: &Map<String, Value>, a: &[Value], path: &str) -> Result<(), Refusal> {
    if let Some(min) = usize_kw(s, "minItems") {
        if a.len() < min {
            return Err(fail(path, format!("{} items, fewer than {min}", a.len())));
        }
    }
    if let Some(max) = usize_kw(s, "maxItems") {
        if a.len() > max {
            return Err(fail(path, format!("{} items, more than {max}", a.len())));
        }
    }
    if s.get("uniqueItems") == Some(&Value::Bool(true)) {
        for (i, x) in a.iter().enumerate() {
            if a[..i].contains(x) {
                return Err(fail(path, "duplicate item"));
            }
        }
    }
    if let Some(items) = s.get("items") {
        for (i, x) in a.iter().enumerate() {
            walk(items, x, &format!("{path}[{i}]"))?;
        }
    }
    Ok(())
}

fn object_rules(s: &Map<String, Value>, o: &Map<String, Value>, path: &str) -> Result<(), Refusal> {
    let props = s.get("properties").and_then(Value::as_object);
    if let Some(Value::Array(req)) = s.get("required") {
        for r in req.iter().filter_map(Value::as_str) {
            if !o.contains_key(r) {
                return Err(fail(path, format!("missing required field `{r}`")));
            }
        }
    }
    match s.get("additionalProperties") {
        None | Some(Value::Bool(true)) => {}
        Some(Value::Bool(false)) => {
            for k in o.keys() {
                if !props.is_some_and(|p| p.contains_key(k)) {
                    // Same classification the typed layer uses for an unknown
                    // field, so callers see one kind of refusal for it.
                    return Err(Refusal::Strict(ContractError::UnknownVariantOrField(
                        format!("{path}: unknown field `{k}`"),
                    )));
                }
            }
        }
        Some(_) => return Err(fail(path, "unsupported additionalProperties form")),
    }
    if let Some(p) = props {
        for (k, sub) in p {
            if let Some(x) = o.get(k) {
                walk(sub, x, &format!("{path}.{k}"))?;
            }
        }
    }
    Ok(())
}

/// Parse a checked-in schema text. The texts are compile-time constants from
/// this crate, so a failure is a build defect, reported as a refusal rather
/// than a panic so a broken schema can never be read as "anything goes".
pub(crate) fn load(text: &str) -> Result<Value, Refusal> {
    serde_json::from_str(text).map_err(|e| shape(format!("checked-in schema unreadable: {e}")))
}

macro_rules! schema_text {
    ($file:literal) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/schemas/", $file))
    };
}
pub(crate) use schema_text;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn keywords_outside_the_subset_fail_closed() {
        let s = json!({"type": "string", "format": "email"});
        assert!(validate_against(&s, &json!("x")).is_err());
        let s = json!({"type": "string", "pattern": "^x+$"});
        assert!(validate_against(&s, &json!("x")).is_err());
    }

    #[test]
    fn structural_rules() {
        let obj = json!({"type":"object","additionalProperties":false,
            "properties":{"a":{"type":"string"}},"required":["a"]});
        assert!(validate_against(&obj, &json!({"a":"x"})).is_ok());
        assert!(
            validate_against(&obj, &json!(["x"])).is_err(),
            "ATTACK: validate_against took an array for an object"
        );
        // The walker's own keyword rules (LIBRARY_PRIMITIVE M1223, M1224,
        // M1214, amendment 64): every production document is also refused by
        // the typed layer, so only this direct test needs each rule.
        assert!(
            validate_against(&obj, &json!({})).is_err(),
            "ATTACK: validate_against admitted an object missing a required field"
        );
        assert!(
            validate_against(&obj, &json!({"a":"x","b":1})).is_err(),
            "ATTACK: validate_against admitted a field additionalProperties:false closes"
        );
        let e = json!({"enum":["a","b"]});
        assert!(validate_against(&e, &json!("a")).is_ok());
        assert!(validate_against(&e, &json!({"a":null})).is_err());
        let i = json!({"type":"integer","minimum":0});
        assert!(
            validate_against(&i, &json!(true)).is_err(),
            "ATTACK: validate_against took a boolean for an integer"
        );
        assert!(validate_against(&i, &json!(-1)).is_err());
        let one = json!({"oneOf":[{"type":"integer"},{"type":"integer","minimum":5}]});
        assert!(
            validate_against(&one, &json!(7)).is_err(),
            "ATTACK: validate_against admitted a value matching two oneOf branches"
        );
        assert!(validate_against(&one, &json!(1)).is_ok());
    }

    /// C9 round 4c, EQGATE (amendment 81; M1941-M1942): the hand-written
    /// matchers behind `pattern_matches`, the reference schemes among them.
    #[test]
    fn reference_patterns_are_exact() {
        let hex = "a".repeat(64);
        let any = json!({"type":"string","pattern":"^(cl22|acf1|sha256):[0-9a-f]{64}$"});
        for ok in ["cl22", "acf1", "sha256"] {
            assert!(
                validate_against(&any, &json!(format!("{ok}:{hex}"))).is_ok(),
                "{ok}"
            );
        }
        assert!(
            validate_against(&any, &json!(format!("md5:{hex}"))).is_err(),
            "ATTACK: validate_against admitted a reference of a scheme the pattern does not name"
        );
        let acf = json!({"type":"string","pattern":"^acf1:[0-9a-f]{64}$"});
        assert!(validate_against(&acf, &json!(format!("acf1:{hex}"))).is_ok());
        assert!(
            validate_against(&acf, &json!(format!("cl22:{hex}"))).is_err(),
            "ATTACK: validate_against admitted a cl22 reference where only acf1 is named"
        );
    }

    /// C9 round 7, EQGATE3 (amendment 91): the walker's own refusal of a
    /// non-integer number. It was exempted as UNREACHABLE (`parse_value` refuses
    /// a float before any walk); the walker is `pub(crate)` and reached directly.
    #[test]
    fn the_walker_refuses_a_non_integer_number() {
        let s = json!({"type": "number", "minimum": 0});
        let float = serde_json::Number::from_f64(1.5).unwrap();
        let got = number_rules(s.as_object().unwrap(), &float, "$.x");
        assert!(
            got.as_ref()
                .is_err_and(|e| e.to_string().contains("non-integer number")),
            "ATTACK: number_rules took a float for an integer: {got:?}"
        );
        number_rules(s.as_object().unwrap(), &serde_json::Number::from(3), "$.x")
            .expect("control: an integer passes");
    }

    #[test]
    fn profile_id_pattern_is_exact() {
        let s = json!({"type": "string", "pattern": crate::profile::PROFILE_ID_PATTERN});
        for ok in [
            "axon.closed-loop.policy/1",
            "usage/2",
            "axon-bridge/v0",
            "cortex-policy-adapter/1",
            "a/9999",
        ] {
            assert!(validate_against(&s, &json!(ok)).is_ok(), "{ok}");
        }
        for bad in [
            "",
            "/1",
            "Usage/2",
            "usage/02",
            "usage/10000",
            "usage/",
            "usage/2 ",
            "usage_x/2",
            "1usage/2",
            "usage/v",
            "usage/2/3",
            "usage/-1",
        ] {
            assert!(
                validate_against(&s, &json!(bad)).is_err(),
                "ATTACK: validate_against admitted {bad:?}, which its pattern does not match"
            );
        }
        let long_ok = format!("{}/1", "a".repeat(96));
        let too_long = format!("{}/1", "a".repeat(97));
        assert!(validate_against(&s, &json!(long_ok)).is_ok());
        assert!(validate_against(&s, &json!(too_long)).is_err());
    }
}
