//! The one error type. Every variant is a REFUSAL; none is a fallback.

use axon_cortex::ContractError;

#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    /// Duplicate / escaped-alias key, unknown field or variant, or malformed
    /// JSON, as reported by `axon_cortex::parse_strict`.
    Strict(ContractError),
    /// Input (or canonical output) exceeds [`crate::MAX_BYTES`].
    TooLarge(usize),
    /// Container nesting exceeds [`crate::MAX_DEPTH`].
    TooDeep,
    /// A JSON number with a fraction or exponent, or one too large for an
    /// integer. The profile has no floats anywhere.
    Float,
    /// An integer with |n| > 2^53 − 1.
    UnsafeInteger(String),
    /// The document parsed but does not have the contract's shape (field
    /// types, patterns, cardinalities, schema-level conditionals).
    Shape(String),
    /// A shape-valid document that fails a no-I/O semantic check.
    Semantic(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Strict(e) => write!(f, "strict JSON: {e}"),
            Refusal::TooLarge(n) => write!(f, "JSON byte limit: {n} bytes"),
            Refusal::TooDeep => write!(f, "JSON nesting limit"),
            Refusal::Float => write!(f, "float/non-integer JSON number"),
            Refusal::UnsafeInteger(n) => write!(f, "unsafe JSON integer: {n}"),
            Refusal::Shape(s) => write!(f, "shape: {s}"),
            Refusal::Semantic(s) => write!(f, "semantic: {s}"),
        }
    }
}

impl std::error::Error for Refusal {}

pub(crate) fn shape(msg: impl Into<String>) -> Refusal {
    Refusal::Shape(msg.into())
}

pub(crate) fn semantic(msg: impl Into<String>) -> Refusal {
    Refusal::Semantic(msg.into())
}
