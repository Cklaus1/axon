//! Built-in functions pre-loaded into every Axon program.
//!
//! `BUILTINS` is a static slice of `BuiltinFn` descriptors.  The resolver
//! seeds the global scope from it, and the type-checker can query `builtin_sigs`
//! to obtain parameter/return-type information without re-parsing the table.

use std::collections::HashMap;

// ── BuiltinFn descriptor ─────────────────────────────────────────────────────

/// Metadata for a single built-in function.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinFn {
    /// Canonical function name as it appears in source code.
    pub name: &'static str,
    /// Parameter list: `(param_name, type_name)` pairs.
    pub params: &'static [(&'static str, &'static str)],
    /// Return type expressed as a source-level type string.
    pub ret: &'static str,
    /// One-line documentation string shown in diagnostics / IDE hover.
    pub doc: &'static str,
}

// ── Built-in table ───────────────────────────────────────────────────────────

/// Every built-in function available in an Axon program.
///
/// Phase 3 `assert_eq` decision: we chose **Option B** from spec §1188 — type-specific variants
/// `assert_eq_str` and `assert_eq_f64` as explicit builtins, without requiring a `T: Eq` generic
/// bound. The `assert_eq(i64, i64)` builtin handles the common integer case. Once traits are fully
/// stable in Phase 4, these can be unified under a generic `assert_eq<T: Eq>`.
///
/// Phase-1 rule: `assert_eq` and `assert_err` take `str` operands because the
/// type-checker hasn't run yet; the real generic signatures are resolved in
/// Phase 2 after type inference.
pub const BUILTINS: &[BuiltinFn] = &[
    // ── I/O ─────────────────────────────────────────────────────────────────
    BuiltinFn {
        name: "print",
        params: &[("msg", "str")],
        ret: "()",
        doc: "Write `msg` to stdout without a trailing newline.",
    },
    BuiltinFn {
        name: "println",
        params: &[("msg", "str")],
        ret: "()",
        doc: "Write `msg` to stdout followed by a newline.",
    },
    BuiltinFn {
        name: "eprint",
        params: &[("msg", "str")],
        ret: "()",
        doc: "Write `msg` to stderr without a trailing newline.",
    },
    BuiltinFn {
        name: "eprintln",
        params: &[("msg", "str")],
        ret: "()",
        doc: "Write `msg` to stderr followed by a newline.",
    },
    // ── Assertions (for #[test] functions) ──────────────────────────────────
    BuiltinFn {
        name: "assert",
        params: &[("cond", "bool")],
        ret: "()",
        doc: "Panic with a diagnostic message if `cond` is false.",
    },
    BuiltinFn {
        name: "assert_eq",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "()",
        doc: "Panic if `a != b`. Compares two i64 values.",
    },
    BuiltinFn {
        name: "assert_err",
        params: &[("tag", "bool")],
        ret: "()",
        doc: "Panic if `tag` is true (Ok). Expects the i1 tag from a Result.",
    },
    // ── String ──────────────────────────────────────────────────────────────
    BuiltinFn {
        name: "len",
        params: &[("s", "str")],
        ret: "i64",
        doc: "Return the number of bytes in `s`.",
    },
    BuiltinFn {
        name: "to_str_f64",
        params: &[("n", "f64")],
        ret: "str",
        doc: "Convert an `f64` to its string representation (\"%.6g\" format).",
    },
    BuiltinFn {
        name: "format",
        params: &[("template", "str")],
        ret: "str",
        doc: "Interpolate `template` and return the resulting string.",
    },
    BuiltinFn {
        name: "axon_concat",
        params: &[("a", "str"), ("b", "str")],
        ret: "str",
        doc: "Concatenate two strings; used internally by string interpolation.",
    },
    // ── Conversion ──────────────────────────────────────────────────────────
    BuiltinFn {
        name: "to_str",
        params: &[("n", "i64")],
        ret: "str",
        doc: "Convert an `i64` to its decimal string representation.",
    },
    BuiltinFn {
        name: "parse_int",
        params: &[("s", "str")],
        ret: "Result<i64,str>",
        doc: "Parse `s` as a base-10 integer. Returns `Err(str)` on failure.",
    },
    // ── Math ────────────────────────────────────────────────────────────────
    BuiltinFn {
        name: "abs_i32",
        params: &[("n", "i64")],
        ret: "i32",
        doc: "Absolute value of an integer (truncated to i32 range).",
    },
    BuiltinFn {
        name: "abs_f64",
        params: &[("n", "f64")],
        ret: "f64",
        doc: "Absolute value of an `f64`.",
    },
    BuiltinFn {
        name: "min_i32",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i32",
        doc: "Return the lesser of two integers (truncated to i32 range).",
    },
    BuiltinFn {
        name: "max_i32",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i32",
        doc: "Return the greater of two integers (truncated to i32 range).",
    },
    // ── Channels ────────────────────────────────────────────────────────────
    BuiltinFn {
        name: "Chan::new",
        params: &[("capacity", "i64")],
        ret: "Chan<i64>",
        doc: "Create a new bounded channel with the given capacity.",
    },
    // ── Math (Phase 3) ───────────────────────────────────────────────────────
    BuiltinFn {
        name: "sqrt",
        params: &[("n", "f64")],
        ret: "f64",
        doc: "Square root of `n`. Calls C sqrt; not comptime-evaluable.",
    },
    BuiltinFn {
        name: "pow",
        params: &[("base", "f64"), ("exp", "f64")],
        ret: "f64",
        doc: "Raise `base` to the power `exp`. Calls C pow.",
    },
    BuiltinFn {
        name: "floor",
        params: &[("n", "f64")],
        ret: "f64",
        doc: "Largest integer not greater than `n`. Calls C floor.",
    },
    BuiltinFn {
        name: "ceil",
        params: &[("n", "f64")],
        ret: "f64",
        doc: "Smallest integer not less than `n`. Calls C ceil.",
    },
    // ── Assertions (Phase 3) ────────────────────────────────────────────────
    BuiltinFn {
        name: "assert_eq_str",
        params: &[("a", "str"), ("b", "str")],
        ret: "()",
        doc: "Panic with a diff if `a != b`. Compares two str values.",
    },
    BuiltinFn {
        name: "assert_eq_f64",
        params: &[("a", "f64"), ("b", "f64")],
        ret: "()",
        doc: "Panic if `a != b`. Compares two f64 values.",
    },
    // ── Phase 4: I/O builtins ─────────────────────────────────────────────────
    BuiltinFn {
        name: "read_line",
        params: &[],
        ret: "str",
        doc: "Read one line from stdin (without the trailing newline). Blocks until newline or EOF.",
    },
    BuiltinFn {
        name: "read_file",
        params: &[("path", "str")],
        ret: "Result<str, str>",
        doc: "Read the entire contents of `path` as a UTF-8 string. Returns Ok(contents) or Err(message).",
    },
    BuiltinFn {
        name: "write_file",
        params: &[("path", "str"), ("content", "str")],
        ret: "Result<(), str>",
        doc: "Write `content` to `path`, creating or truncating the file. Returns Ok(()) or Err(message).",
    },
    // ── Phase 4: Time builtins ────────────────────────────────────────────────
    BuiltinFn {
        name: "sleep_ms",
        params: &[("ms", "i64")],
        ret: "()",
        doc: "Suspend the current thread for at least `ms` milliseconds.",
    },
    BuiltinFn {
        name: "now_ms",
        params: &[],
        ret: "i64",
        doc: "Return the current wall-clock time as milliseconds since the Unix epoch.",
    },
    // ── Phase 5: String builtins ──────────────────────────────────────────────
    BuiltinFn {
        name: "str_eq",
        params: &[("a", "str"), ("b", "str")],
        ret: "bool",
        doc: "Return true if `a` and `b` have the same content (byte-by-byte equality).",
    },
    BuiltinFn {
        name: "str_contains",
        params: &[("s", "str"), ("needle", "str")],
        ret: "bool",
        doc: "Return true if `s` contains `needle` as a substring.",
    },
    BuiltinFn {
        name: "str_starts_with",
        params: &[("s", "str"), ("prefix", "str")],
        ret: "bool",
        doc: "Return true if `s` begins with `prefix`.",
    },
    BuiltinFn {
        name: "str_ends_with",
        params: &[("s", "str"), ("suffix", "str")],
        ret: "bool",
        doc: "Return true if `s` ends with `suffix`.",
    },
    BuiltinFn {
        name: "str_slice",
        params: &[("s", "str"), ("start", "i64"), ("end", "i64")],
        ret: "str",
        doc: "Return the substring of `s` from byte index `start` (inclusive) to `end` (exclusive). Clamps indices to valid range.",
    },
    BuiltinFn {
        name: "str_index_of",
        params: &[("s", "str"), ("needle", "str")],
        ret: "i64",
        doc: "Return the byte index of the first occurrence of `needle` in `s`, or -1 if not found.",
    },
    // ── Phase 5: Conversion builtins ─────────────────────────────────────────
    BuiltinFn {
        name: "parse_float",
        params: &[("s", "str")],
        ret: "Result<f64, str>",
        doc: "Parse `s` as a 64-bit float. Returns Ok(n) or Err(message).",
    },
    BuiltinFn {
        name: "char_at",
        params: &[("s", "str"), ("i", "i64")],
        ret: "i64",
        doc: "Return the byte value (0-255) of `s` at byte index `i`, or -1 if `i` is out of range.",
    },
    // ── Phase 5: Conversion builtins (continued) ──────────────────────────────
    BuiltinFn {
        name: "to_str_bool",
        params: &[("b", "bool")],
        ret: "str",
        doc: "Convert a bool to its string representation: \"true\" or \"false\".",
    },
    // ── Phase 5: Math builtins ────────────────────────────────────────────────
    BuiltinFn {
        name: "abs_i64",
        params: &[("n", "i64")],
        ret: "i64",
        doc: "Absolute value of an i64.",
    },
    BuiltinFn {
        name: "min_i64",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i64",
        doc: "Return the lesser of two i64 values.",
    },
    BuiltinFn {
        name: "max_i64",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i64",
        doc: "Return the greater of two i64 values.",
    },
    // ── Phase 6: String builtins ──────────────────────────────────────────────
    BuiltinFn {
        name: "str_to_upper",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return a copy of `s` with all ASCII letters converted to uppercase.",
    },
    BuiltinFn {
        name: "str_to_lower",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return a copy of `s` with all ASCII letters converted to lowercase.",
    },
    BuiltinFn {
        name: "str_trim",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return `s` with leading and trailing ASCII whitespace removed.",
    },
    BuiltinFn {
        name: "str_trim_start",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return `s` with leading ASCII whitespace removed.",
    },
    BuiltinFn {
        name: "str_trim_end",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return `s` with trailing ASCII whitespace removed.",
    },
    BuiltinFn {
        name: "str_replace",
        params: &[("s", "str"), ("from", "str"), ("to", "str")],
        ret: "str",
        doc: "Return a copy of `s` with all non-overlapping occurrences of `from` replaced by `to`.",
    },
    BuiltinFn {
        name: "str_repeat",
        params: &[("s", "str"), ("n", "i64")],
        ret: "str",
        doc: "Return a string containing `s` repeated `n` times (empty string if n <= 0).",
    },
    // ── Phase 6: System builtins ──────────────────────────────────────────────
    BuiltinFn {
        name: "env_var",
        params: &[("name", "str")],
        ret: "Result<str, str>",
        doc: "Read the environment variable `name`. Returns Ok(value) or Err(\"not set\").",
    },
    BuiltinFn {
        name: "exit",
        params: &[("code", "i64")],
        ret: "()",
        doc: "Terminate the process immediately with the given exit code.",
    },
    // ── Phase 7: String utilities ─────────────────────────────────────────
    BuiltinFn {
        name: "str_len",
        params: &[("s", "str")],
        ret: "i64",
        doc: "Return the byte length of `s`.",
    },
    BuiltinFn {
        name: "str_pad_start",
        params: &[("s", "str"), ("width", "i64"), ("fill", "str")],
        ret: "str",
        doc: "Left-pad `s` with the first byte of `fill` until it is `width` bytes long. Returns `s` unchanged if already >= `width` bytes.",
    },
    BuiltinFn {
        name: "str_pad_end",
        params: &[("s", "str"), ("width", "i64"), ("fill", "str")],
        ret: "str",
        doc: "Right-pad `s` with the first byte of `fill` until it is `width` bytes long. Returns `s` unchanged if already >= `width` bytes.",
    },
    // ── Phase 7: Math completeness ────────────────────────────────────────
    BuiltinFn {
        name: "min_f64",
        params: &[("a", "f64"), ("b", "f64")],
        ret: "f64",
        doc: "Return the smaller of two `f64` values.",
    },
    BuiltinFn {
        name: "max_f64",
        params: &[("a", "f64"), ("b", "f64")],
        ret: "f64",
        doc: "Return the larger of two `f64` values.",
    },
    BuiltinFn {
        name: "clamp_i64",
        params: &[("n", "i64"), ("lo", "i64"), ("hi", "i64")],
        ret: "i64",
        doc: "Clamp `n` to the range `[lo, hi]`.",
    },
    BuiltinFn {
        name: "clamp_f64",
        params: &[("n", "f64"), ("lo", "f64"), ("hi", "f64")],
        ret: "f64",
        doc: "Clamp `n` to the range `[lo, hi]`.",
    },
    // ── Phase 7: Conversion ───────────────────────────────────────────────
    BuiltinFn {
        name: "parse_bool",
        params: &[("s", "str")],
        ret: "Result<bool, str>",
        doc: "Parse `\"true\"` or `\"false\"`. Returns `Ok(true/false)` or `Err(\"invalid bool\")`.",
    },
    // ── Phase 7: Random ───────────────────────────────────────────────────
    BuiltinFn {
        name: "random_i64",
        params: &[("lo", "i64"), ("hi", "i64")],
        ret: "i64",
        doc: "Return a pseudo-random `i64` in `[lo, hi)` using C `rand()`.",
    },
    BuiltinFn {
        name: "random_f64",
        params: &[],
        ret: "f64",
        doc: "Return a pseudo-random `f64` in `[0.0, 1.0)` using C `rand()`.",
    },
    // ── Phase 9: Numeric type conversions ────────────────────────────────────
    BuiltinFn {
        name: "i64_to_f64",
        params: &[("n", "i64")],
        ret: "f64",
        doc: "Convert an `i64` to the nearest `f64`.",
    },
    BuiltinFn {
        name: "f64_to_i64",
        params: &[("x", "f64")],
        ret: "i64",
        doc: "Truncate an `f64` to `i64` (rounds toward zero).",
    },
    // ── Phase 9: Abs / Sign ──────────────────────────────────────────────────
    BuiltinFn {
        name: "sign_i64",
        params: &[("n", "i64")],
        ret: "i64",
        doc: "Sign of `n`: -1, 0, or 1.",
    },
    // ── Phase 9: Integer math ────────────────────────────────────────────────
    BuiltinFn {
        name: "pow_i64",
        params: &[("base", "i64"), ("exp", "i64")],
        ret: "i64",
        doc: "Raise `base` to the power `exp` (non-negative `exp` only).",
    },
    BuiltinFn {
        name: "sqrt_f64",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Square root of `x`.",
    },
    BuiltinFn {
        name: "floor_f64",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Round `x` down to the nearest integer (as `f64`).",
    },
    BuiltinFn {
        name: "ceil_f64",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Round `x` up to the nearest integer (as `f64`).",
    },
    BuiltinFn {
        name: "round_f64",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Round `x` to the nearest integer, ties away from zero.",
    },
    // ── Phase 10: Additional string utilities ────────────────────────────────
    BuiltinFn {
        name: "str_count",
        params: &[("s", "str"), ("needle", "str")],
        ret: "i64",
        doc: "Return the number of non-overlapping occurrences of `needle` in `s`. Returns 0 if `needle` is empty or not found.",
    },
    BuiltinFn {
        name: "str_reverse",
        params: &[("s", "str")],
        ret: "str",
        doc: "Return a copy of `s` with its bytes in reverse order.",
    },
    // ── Phase 10: Additional conversion utilities ────────────────────────────
    BuiltinFn {
        name: "i64_to_str_radix",
        params: &[("n", "i64"), ("base", "i64")],
        ret: "str",
        doc: "Convert `n` to a string in the given base (2–36). Negative numbers are prefixed with '-'. Bases outside 2–36 return an empty string.",
    },
    // ── Phase 57: Uncertain<T> / Temporal<T> builtins ────────────────────────
    BuiltinFn {
        name: "uncertain_confidence",
        params: &[("confidence", "f64")],
        ret: "()",
        doc: "Record an implicit confidence level for the surrounding Uncertain<T> value (0.0–1.0). Used for AI-inferred values.",
    },
    BuiltinFn {
        name: "temporal_now",
        params: &[],
        ret: "i64",
        doc: "Return the current validity timestamp for Temporal<T> values (milliseconds since epoch).",
    },
    // ── Phase 68: Bitwise operations ─────────────────────────────────────────
    BuiltinFn {
        name: "bit_and",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i64",
        doc: "Bitwise AND of two i64 values (`a & b`).",
    },
    BuiltinFn {
        name: "bit_or",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i64",
        doc: "Bitwise OR of two i64 values (`a | b`).",
    },
    BuiltinFn {
        name: "bit_xor",
        params: &[("a", "i64"), ("b", "i64")],
        ret: "i64",
        doc: "Bitwise XOR of two i64 values (`a ^ b`).",
    },
    BuiltinFn {
        name: "bit_not",
        params: &[("n", "i64")],
        ret: "i64",
        doc: "Bitwise NOT (complement) of an i64 value (`~n`).",
    },
    BuiltinFn {
        name: "shl",
        params: &[("n", "i64"), ("shift", "i64")],
        ret: "i64",
        doc: "Left-shift `n` by `shift` bits (`n << shift`).",
    },
    BuiltinFn {
        name: "shr",
        params: &[("n", "i64"), ("shift", "i64")],
        ret: "i64",
        doc: "Arithmetic right-shift `n` by `shift` bits (`n >> shift`).",
    },
    // ── AI builtins ──────────────────────────────────────────────────────────────
    BuiltinFn {
        name: "ai_complete",
        params: &[("prompt", "str")],
        ret: "Result<str, str>",
        doc: "Send prompt to the Anthropic API (claude-sonnet-4-6) and return the reply. Returns Ok(reply) or Err(message). Requires ANTHROPIC_API_KEY env var.",
    },
];

// ── BuiltinSig (consumed by infer.rs) ────────────────────────────────────────

/// Lightweight signature used by the type-inference pass.
///
/// This mirrors `FnSig` from `infer.rs` but is defined here so that
/// `builtins.rs` remains self-contained until `infer.rs` is written.
#[derive(Debug, Clone, PartialEq)]
pub struct BuiltinSig {
    /// Ordered list of parameter type names (source-level strings).
    pub params: Vec<String>,
    /// Return type name (source-level string).
    pub ret: String,
}

/// Build a `HashMap` keyed by function name for O(1) lookup during inference.
///
/// Every entry in [`BUILTINS`] is included.  Callers in `infer.rs` should
/// merge this map into their global signature table at startup.
pub fn builtin_sigs() -> HashMap<String, BuiltinSig> {
    BUILTINS
        .iter()
        .map(|b| {
            let sig = BuiltinSig {
                params: b.params.iter().map(|(_, ty)| ty.to_string()).collect(),
                ret: b.ret.to_string(),
            };
            (b.name.to_string(), sig)
        })
        .collect()
}

// ── Deferred attribute names ──────────────────────────────────────────────────

/// Attribute names that are recognised by Axon's AI/runtime layer but are not
/// yet resolved by the Phase-1 compiler.  The resolver emits `I0001` for each
/// occurrence instead of an error, allowing code to compile while tooling
/// acknowledges the annotation.
pub const DEFERRED_ATTRS: &[&str] = &[
    "adaptive",
    "goal",
    "experiment",
    "agent",
    "contained",
    "corrigible",
    "sensitive",
    "verify",
    "ai",
    "layout",
    "temporal",
];

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_builtins_have_non_empty_names() {
        for b in BUILTINS {
            assert!(!b.name.is_empty(), "builtin name must not be empty");
        }
    }

    #[test]
    fn all_builtins_have_docs() {
        for b in BUILTINS {
            assert!(
                !b.doc.is_empty(),
                "builtin `{}` is missing a doc string",
                b.name
            );
        }
    }

    #[test]
    fn builtin_sigs_covers_all_builtins() {
        let sigs = builtin_sigs();
        for b in BUILTINS {
            assert!(
                sigs.contains_key(b.name),
                "builtin_sigs missing entry for `{}`",
                b.name
            );
        }
        assert_eq!(sigs.len(), BUILTINS.len(), "no duplicates expected");
    }

    #[test]
    fn builtin_sig_param_and_ret_roundtrip() {
        let sigs = builtin_sigs();
        let println_sig = sigs.get("println").expect("println must be present");
        assert_eq!(println_sig.params, vec!["str".to_string()]);
        assert_eq!(println_sig.ret, "()");

        let parse_int_sig = sigs.get("parse_int").expect("parse_int must be present");
        assert_eq!(parse_int_sig.params, vec!["str".to_string()]);
        assert_eq!(parse_int_sig.ret, "Result<i64,str>");

        let min_sig = sigs.get("min_i32").expect("min_i32 must be present");
        assert_eq!(min_sig.params, vec!["i64".to_string(), "i64".to_string()]);
    }

    #[test]
    fn deferred_attrs_non_empty_and_no_duplicates() {
        assert!(!DEFERRED_ATTRS.is_empty());
        let mut seen = std::collections::HashSet::new();
        for attr in DEFERRED_ATTRS {
            assert!(
                seen.insert(*attr),
                "duplicate deferred attr: `{attr}`"
            );
        }
    }

    #[test]
    fn known_attrs_present() {
        let set: std::collections::HashSet<&str> = DEFERRED_ATTRS.iter().copied().collect();
        for expected in &["adaptive", "goal", "agent", "verify", "ai"] {
            assert!(set.contains(expected), "expected deferred attr `{expected}` not found");
        }
    }
}
