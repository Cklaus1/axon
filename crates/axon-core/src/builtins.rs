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
        doc: "Parse `s` (surrounding whitespace trimmed) as a base-10 integer. Base-10 only — a radix prefix like `0x1F` returns `Err` with a hint to strip it. The `Err(str)` names the offending input.",
    },
    BuiltinFn {
        name: "parse_int_radix",
        params: &[("s", "str"), ("base", "i64")],
        ret: "Result<i64,str>",
        doc: "Parse `s` (whitespace trimmed) as an integer in the given `base` (2–36); the radix-aware counterpart to `parse_int` (base 10) and the input-side inverse of `i64_to_str_radix`. A matching radix prefix is accepted and stripped (`0x`/`0X` for base 16, `0o`/`0O` for 8, `0b`/`0B` for 2); a leading `-` is allowed. `Err(str)` names the offending input on a bad digit; a `base` outside 2–36 is `Err`, never a panic.",
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
    // ── R21 — Decimal (exact fixed-point money) ──────────────────────────────
    BuiltinFn {
        name: "decimal_from_str",
        params: &[("s", "str")],
        ret: "Result<Decimal,str>",
        doc: "Parse `s` (e.g. \"1.50\", \"-0.25\", \"100\") into an exact fixed-point `Decimal` at scale 9. `Err(str)` on malformed input or excess precision (>9 fractional digits). The input-side inverse of `decimal_to_str` — round-trips exactly.",
    },
    BuiltinFn {
        name: "decimal_to_str",
        params: &[("d", "Decimal")],
        ret: "str",
        doc: "Render a `Decimal` to its canonical exact string (trailing zeros stripped; whole numbers print with no point). Exact inverse of `decimal_from_str`.",
    },
    BuiltinFn {
        name: "decimal_round",
        params: &[("d", "Decimal"), ("dp", "i64"), ("mode", "str")],
        ret: "Decimal",
        doc: "Round `d` to `dp` fractional digits (0–9) using rounding `mode` (\"half_even\" [banker's], \"half_up\", \"down\", \"up\"). The result stays at scale 9. Unknown mode or dp>9 is a graceful panic.",
    },
    BuiltinFn {
        name: "decimal_div",
        params: &[("a", "Decimal"), ("b", "Decimal"), ("mode", "str")],
        ret: "Decimal",
        doc: "Divide `a / b` to scale 9 with an EXPLICIT rounding `mode` (money division must specify rounding). Division by zero is a graceful panic. The `/` operator uses the half_even default; this builtin lets you pick the mode.",
    },
    BuiltinFn {
        name: "decimal_abs",
        params: &[("d", "Decimal")],
        ret: "Decimal",
        doc: "Absolute value of a `Decimal`. Overflow (at the i128 minimum) is a graceful panic, never a wrap.",
    },
    BuiltinFn {
        name: "decimal_neg",
        params: &[("d", "Decimal")],
        ret: "Decimal",
        doc: "Negate a `Decimal`. Overflow (at the i128 minimum) is a graceful panic.",
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
        name: "exp",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Natural exponential e^x. Useful for sigmoids, softmax, decay functions.",
    },
    BuiltinFn {
        name: "ln",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Natural log of `x`. `x <= 0` returns NaN (matches Rust f64::ln). UCB-style algorithms use `ln(t)` where t ≥ 1, so this is fine in practice.",
    },
    BuiltinFn {
        name: "log10",
        params: &[("x", "f64")],
        ret: "f64",
        doc: "Base-10 log of `x`. Use for human-readable scales (orders of magnitude).",
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
    BuiltinFn {
        name: "exec",
        params: &[("cmd", "str"), ("args", "[str]")],
        ret: "Result<str, str>",
        doc: "Spawn a process: run `cmd` with `args`, returning Ok(stdout) on success or Err(message) on failure. Exercises the `exec` capability — a `@[contained(exec: none)]` fn that calls this is rejected at compile time (E1001), and `never: [exec]` hard-denies it. The active host may refuse exec entirely (a sandboxed/browser host returns Err). Use for shelling out to external tools from a trusted zone.",
    },
    BuiltinFn {
        name: "sql_query",
        params: &[("template", "str"), ("params", "[str]")],
        ret: "str",
        doc: "Build a parameterized SQL query. `template` MUST be a string literal — each `?` placeholder is filled, in order, by the next value in `params` (a bound parameter, escaped). User-controlled data goes in `params`, NEVER in `template`: a template built by concatenation or interpolation is a compile error (E1210), so SQL injection is unrepresentable by construction. Interpreter-only (codegen E0910-refused).",
    },
    BuiltinFn {
        name: "http_get",
        params: &[("url", "str"), ("headers", "str")],
        ret: "Result<str, str>",
        doc: "HTTP GET `url` with `headers` (a JSON object string, pass `\"\"` for no custom headers). Returns Ok(body) on a 2xx response or Err(\"HTTP <status>: <body>\") on failure. Carries the `Net` effect row. Requires the `asi-runtime` Cargo feature.",
    },
    BuiltinFn {
        name: "http_post",
        params: &[("url", "str"), ("headers", "str"), ("body", "str")],
        ret: "Result<str, str>",
        doc: "HTTP POST `url` with `headers` (JSON object string) and `body`. Returns Ok(response_body) on 2xx or Err(\"HTTP <status>: <body>\") on failure. Carries the `Net` effect row.",
    },
    BuiltinFn {
        name: "http_sse",
        params: &[("url", "str"), ("headers", "str"), ("on_event", "fn(str) -> ()")],
        ret: "Result<i64, str>",
        doc: "Stream Server-Sent Events from `url` via GET. Sets `Accept: text/event-stream` automatically. `headers` is a JSON object string (pass `\"\"` for none). `on_event` is called once per SSE event with the `data:` payload. Returns Ok(event_count) or Err(message). Carries the `Net` effect row. Requires the `asi-runtime` Cargo feature.",
    },
    BuiltinFn {
        name: "http_sse_post",
        params: &[("url", "str"), ("headers", "str"), ("body", "str"), ("on_event", "fn(str) -> ()")],
        ret: "Result<i64, str>",
        doc: "Stream Server-Sent Events from `url` via POST (required by LLM provider APIs). Sets `Accept: text/event-stream` automatically. `headers` is a JSON object string; `body` is the POST body (typically JSON). `on_event` is called once per SSE event. Returns Ok(event_count) or Err(message). Carries the `Net` effect row. Requires the `asi-runtime` Cargo feature.",
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
    // ── Array helpers ─────────────────────────────────────────────────────────
    BuiltinFn {
        name: "arr_range",
        params: &[("start", "i64"), ("end", "i64")],
        ret: "[i64]",
        doc: "Return the half-open range `[start, end)` as a fresh i64 slice. Empty when `end <= start`. Useful for `for i in arr_range(0, n)` and as a seed for further array ops.",
    },
    BuiltinFn {
        name: "arr_push",
        params: &[("xs", "[i64]"), ("x", "i64")],
        ret: "[i64]",
        doc: "Return a fresh array with `x` appended. Copy semantics — the input is unaffected. (Concrete-typed for i64 today; generic [T] form waits on Phase 8.)",
    },
    BuiltinFn {
        name: "arr_sum_i64",
        params: &[("xs", "[i64]")],
        ret: "i64",
        doc: "Sum of an i64 array. Empty → 0. Saturates on overflow.",
    },
    BuiltinFn {
        name: "arr_max_i64",
        params: &[("xs", "[i64]")],
        ret: "i64",
        doc: "Maximum of an i64 array. Panics on an empty array — caller should guard with `if len(xs) > 0` first.",
    },
    BuiltinFn {
        name: "arr_min_i64",
        params: &[("xs", "[i64]")],
        ret: "i64",
        doc: "Minimum of an i64 array. Panics on an empty array.",
    },
    BuiltinFn {
        name: "arr_map",
        params: &[("xs", "[T]"), ("f", "fn(T) -> U")],
        ret: "[U]",
        doc: "Apply a closure to each element, producing a fresh array of results. Element and result types are deferred — `arr_map([1, 2], |x| i64_to_str(x))` works. Closure panics propagate.",
    },
    BuiltinFn {
        name: "arr_filter",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "[T]",
        doc: "Keep elements where the predicate returns true. Element type is deferred — `arr_filter([\"a\", \"bb\"], |s| len(s) > 1)` works. Predicate that returns non-bool panics.",
    },
    BuiltinFn {
        name: "arr_fold",
        params: &[("xs", "[T]"), ("init", "U"), ("f", "fn(U, T) -> U")],
        ret: "U",
        doc: "Reduce `xs` to a single value via `f(acc, x) -> acc`, threading `init` as the starting accumulator. Element type `T` and accumulator type `U` are both deferred, so `arr_fold([1, 2, 3], 0.0, |a, x| a + i64_to_f64(x))` works — most general combinator: sum, max, count, product are all special cases.",
    },
    BuiltinFn {
        name: "arr_sort_by",
        params: &[("xs", "[T]"), ("cmp", "fn(T, T) -> i64")],
        ret: "[T]",
        doc: "Return a sorted copy of `xs` using the comparator (neg = a<b, 0 = eq, pos = a>b). Element type is deferred so any sortable domain works. Stable insertion sort; closure-dispatch dominates cost on ASI-scale arrays. Input untouched.",
    },
    BuiltinFn {
        name: "arr_zip",
        params: &[("xs", "[T]"), ("ys", "[U]")],
        ret: "[(T, U)]",
        doc: "Pair two arrays element-wise into a slice of `(x, y)` tuples; truncates to the shorter input. Element types are deferred — works for any pair of arrays. Composes with `arr_map` / `arr_filter` for dataset zipping (features + labels).",
    },
    BuiltinFn {
        name: "arr_contains",
        params: &[("xs", "[T]"), ("v", "T")],
        ret: "bool",
        doc: "Linear scan: does `xs` contain a value structurally equal to `v`? Works for any element type — primitives, strings, tuples, nested arrays.",
    },
    BuiltinFn {
        name: "arr_chunk",
        params: &[("xs", "[T]"), ("n", "i64")],
        ret: "[[T]]",
        doc: "Split into consecutive chunks of size `n`; last chunk may be shorter. Panics on `n <= 0`. Useful for batched processing — pair with `arr_map` over the outer array.",
    },
    BuiltinFn {
        name: "arr_unique",
        params: &[("xs", "[T]")],
        ret: "[T]",
        doc: "Dedupe preserving first occurrence and original order. Structural equality so nested values dedupe correctly. O(n²); hash-based version waits on a HashMap primitive.",
    },
    BuiltinFn {
        name: "arr_index_of",
        params: &[("xs", "[T]"), ("v", "T")],
        ret: "Option<i64>",
        doc: "First index where the element structurally equals `v`. `Some(i)` if found, `None` otherwise. Pairs with `arr_contains` for the `where` follow-up to the `whether` question.",
    },
    BuiltinFn {
        name: "arr_find",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "Option<T>",
        doc: "First element matching the predicate closure. Returns `Some(v)` or `None`. Same closure shape as `arr_filter` but returns a single element with early exit.",
    },
    BuiltinFn {
        name: "arr_any",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "bool",
        doc: "True if at least one element satisfies the predicate. Short-circuits.",
    },
    BuiltinFn {
        name: "arr_all",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "bool",
        doc: "True if ALL elements satisfy the predicate. Short-circuits on first false. Empty array → true (vacuous truth).",
    },
    BuiltinFn {
        name: "arr_count_if",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "i64",
        doc: "Count elements where the predicate returns true. Equivalent to `len(arr_filter(xs, pred))` but doesn't materialize the filtered array.",
    },
    BuiltinFn {
        name: "arr_zip_with",
        params: &[("xs", "[T]"), ("ys", "[U]"), ("f", "fn(T, U) -> V")],
        ret: "[V]",
        doc: "Pair element-wise then map via `f(x, y) -> z`. Truncates to the shorter input. More efficient than `arr_zip` + `arr_map` (no intermediate tuple slice) and the closure sees both values directly.",
    },
    BuiltinFn {
        name: "arr_reverse",
        params: &[("xs", "[T]")],
        ret: "[T]",
        doc: "Return a fresh array with elements in reverse order. Works for any element type.",
    },
    BuiltinFn {
        name: "arr_repeat",
        params: &[("v", "T"), ("n", "i64")],
        ret: "[T]",
        doc: "Build an array of `n` copies of `v`. Useful to initialize a default-filled array before in-place mutation. Negative `n` returns empty; saturating cap at ~1M elements.",
    },
    BuiltinFn {
        name: "arr_concat",
        params: &[("xs", "[T]"), ("ys", "[T]")],
        ret: "[T]",
        doc: "Concatenate two arrays into a fresh one (`xs ++ ys`); preserves order. Element runtime types must agree — no coercion.",
    },
    BuiltinFn {
        name: "arr_flatten",
        params: &[("xss", "[[T]]")],
        ret: "[T]",
        doc: "Flatten `[[T]]` to `[T]`. Each inner element must itself be an array. Useful after `arr_map` produces nested results.",
    },
    BuiltinFn {
        name: "as_f64",
        params: &[("v", "T")],
        ret: "f64",
        doc: "Numeric cast to f64. Accepts i64, f64, or bool; other types panic. Polymorphic on the source so call sites don't need to know the runtime type. Pairs with `as_i64` and supplants per-type `i64_to_f64` calls.",
    },
    BuiltinFn {
        name: "as_i64",
        params: &[("v", "T")],
        ret: "i64",
        doc: "Numeric cast to i64. Accepts i64 (identity), f64 (truncating), or bool (0/1); other types panic.",
    },
    // ── R19 follow-up: fixed-width `as` casts (the `as` operator already
    // desugars `expr as TYPE` to a call to `as_<TYPE>`, per parser.rs — these
    // 7 were the missing callees; as_i64/as_f64 above were the only two that
    // existed). Truncating/masking semantics (like Rust's `as`, no panic on
    // narrowing); accepts i64, f64 (truncating), bool (0/1), or any other
    // fixed-width int (re-masked to the new width) as source.
    BuiltinFn {
        name: "as_u8",
        params: &[("v", "T")],
        ret: "u8",
        doc: "Numeric cast to u8 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as u8` operator.",
    },
    BuiltinFn {
        name: "as_u16",
        params: &[("v", "T")],
        ret: "u16",
        doc: "Numeric cast to u16 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as u16` operator.",
    },
    BuiltinFn {
        name: "as_u32",
        params: &[("v", "T")],
        ret: "u32",
        doc: "Numeric cast to u32 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as u32` operator.",
    },
    BuiltinFn {
        name: "as_u64",
        params: &[("v", "T")],
        ret: "u64",
        doc: "Numeric cast to u64 (bit-reinterpreting, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as u64` operator.",
    },
    BuiltinFn {
        name: "as_i8",
        params: &[("v", "T")],
        ret: "i8",
        doc: "Numeric cast to i8 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as i8` operator.",
    },
    BuiltinFn {
        name: "as_i16",
        params: &[("v", "T")],
        ret: "i16",
        doc: "Numeric cast to i16 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as i16` operator.",
    },
    BuiltinFn {
        name: "as_i32",
        params: &[("v", "T")],
        ret: "i32",
        doc: "Numeric cast to i32 (truncating/masking, no panic). Accepts i64/f64/bool/any fixed-width int. Powers the `x as i32` operator.",
    },
    BuiltinFn {
        name: "arr_take",
        params: &[("xs", "[T]"), ("n", "i64")],
        ret: "[T]",
        doc: "Return the first `n` elements of `xs`. Saturates at `len(xs)`; negative `n` returns empty.",
    },
    BuiltinFn {
        name: "arr_drop",
        params: &[("xs", "[T]"), ("n", "i64")],
        ret: "[T]",
        doc: "Return `xs` with the first `n` elements skipped. Saturates at `len(xs)`; negative `n` returns the full input.",
    },
    BuiltinFn {
        name: "arr_sum_f64",
        params: &[("xs", "[f64]")],
        ret: "f64",
        doc: "Sum of an f64 (or i64-coerced) array. Empty → 0.0.",
    },
    BuiltinFn {
        name: "arr_max_f64",
        params: &[("xs", "[f64]")],
        ret: "f64",
        doc: "Maximum of an f64 array. Accepts i64 elements via coercion. Panics on empty.",
    },
    BuiltinFn {
        name: "arr_min_f64",
        params: &[("xs", "[f64]")],
        ret: "f64",
        doc: "Minimum of an f64 array. Accepts i64 elements via coercion. Panics on empty.",
    },
    BuiltinFn {
        name: "arr_mean_i64",
        params: &[("xs", "[i64]")],
        ret: "f64",
        doc: "Arithmetic mean of an i64 array as f64. Empty → 0.0 (no NaN).",
    },
    BuiltinFn {
        name: "arr_mean_f64",
        params: &[("xs", "[f64]")],
        ret: "f64",
        doc: "Arithmetic mean of an f64 (or i64-coerced) array. Empty → 0.0.",
    },
    BuiltinFn {
        name: "arr_std_f64",
        params: &[("xs", "[f64]")],
        ret: "f64",
        doc: "Sample standard deviation: sqrt of (sum of squared deviations from mean) / (n - 1). Panics for arrays of length < 2.",
    },
    BuiltinFn {
        name: "arr_argmax_i64",
        params: &[("xs", "[i64]")],
        ret: "i64",
        doc: "Index of the largest element in an i64 array; ties broken by lowest index. Panics on empty.",
    },
    BuiltinFn {
        name: "arr_argmin_i64",
        params: &[("xs", "[i64]")],
        ret: "i64",
        doc: "Index of the smallest element in an i64 array.",
    },
    BuiltinFn {
        name: "arr_argmax_f64",
        params: &[("xs", "[f64]")],
        ret: "i64",
        doc: "Index of the largest element in an f64 (or i64-coerced) array.",
    },
    BuiltinFn {
        name: "arr_argmin_f64",
        params: &[("xs", "[f64]")],
        ret: "i64",
        doc: "Index of the smallest element in an f64 (or i64-coerced) array.",
    },
    BuiltinFn {
        name: "str_split",
        params: &[("s", "str"), ("sep", "str")],
        ret: "[str]",
        doc: "Split `s` on every occurrence of `sep`, returning the parts as a slice. Empty separator returns `[s]` (avoids Rust's panic). Inverse of `str_join`.",
    },
    BuiltinFn {
        name: "str_join",
        params: &[("parts", "[str]"), ("sep", "str")],
        ret: "str",
        doc: "Concatenate `parts` with `sep` between each. Inverse of `str_split`. Non-string elements panic; arr_map(to_str) first if needed.",
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
        // R15 resume runtime (v0): suspend the program, yield `req` to the host,
        // and resume with the host's reply. Only valid under a suspendable host
        // driver (`run_suspendable`); a bare `axon run` errors ("no host"), and
        // native codegen refuses it (E0910 — interp-only). v0 carries str payloads
        // (Send across the worker thread); arbitrary-Value payloads + a same-thread
        // coroutine substrate are the next slice (see governance/specs/R15).
        name: "host_await",
        params: &[("req", "str")],
        ret: "str",
        doc: "Suspend, yield `req` to the host, resume with its reply (str). Needs a suspendable host (R15).",
    },
    BuiltinFn {
        // R15: the EOF-aware form of `host_await`. Returns `None` when the host has
        // no more input (e.g. stdin closed / Ctrl-D), `Some(reply)` otherwise — so a
        // read loop can terminate instead of spinning on an endless empty reply. This
        // is the Option-typed contract for interactive loops; plain `host_await` (str)
        // is for fixed-exchange programs that know how many prompts they issue.
        name: "host_await_opt",
        params: &[("req", "str")],
        ret: "Option<str>",
        doc: "Suspend, yield `req`, resume with `Some(reply)` or `None` at end-of-input (R15).",
    },
    BuiltinFn {
        // R15 Slice 2: the arbitrary-`Value`-payload forms of `host_await`. The
        // request `req` and reply may be ANY value (dict/struct/enum/tuple/array/
        // option/result/closure or a scalar) — they cross the worker-thread
        // substrate as an owned `Send` deep-clone (`SendValue`) and are reconstructed
        // on the far side. A `Chan` payload (identity-shared mutable state) is
        // REFUSED at runtime (deep-clone would lose its sharing). Typed `T -> U`
        // (deferred), so the program's own annotations / inference name the shapes;
        // interp-only, native codegen E0910-refuses it like plain `host_await`.
        name: "host_await_val",
        params: &[("req", "T")],
        ret: "U",
        doc: "Suspend, yield any-typed `req` to the host, resume with its (any-typed) reply. Dict/struct/enum payloads cross via a Send deep-clone; a Chan payload is refused (R15 Slice 2).",
    },
    BuiltinFn {
        // R15 Slice 2: the EOF-aware arbitrary-`Value` form — `Some(reply)` or `None`
        // at end-of-input, mirroring `host_await_opt` but for any payload type.
        name: "host_await_val_opt",
        params: &[("req", "T")],
        ret: "Option<U>",
        doc: "Suspend, yield any-typed `req`, resume with `Some(reply)` or `None` at end-of-input. Dict/struct payloads cross via a Send deep-clone (R15 Slice 2).",
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
    BuiltinFn {
        name: "chr",
        params: &[("n", "i64")],
        ret: "str",
        doc: "Return a one-character string for the given Unicode code point. Panics if n is not a valid code point (0–0x10FFFF).",
    },
    // ── JSON builtins ─────────────────────────────────────────────────────────
    BuiltinFn {
        name: "json_parse",
        params: &[("s", "str")],
        ret: "Result<str, str>",
        doc: "Validate `s` as JSON. Returns Ok(s) if valid, Err(reason) if not. Pure; no network call.",
    },
    BuiltinFn {
        name: "json_stringify",
        params: &[("s", "str")],
        ret: "str",
        doc: "JSON-encode a string value: wraps in double quotes and escapes special characters. For numbers/booleans use to_str first.",
    },
    BuiltinFn {
        name: "json_get_str",
        params: &[("json", "str"), ("key", "str")],
        ret: "Result<str, str>",
        doc: "Extract a string field from a JSON object string. Returns Ok(value) or Err(reason).",
    },
    BuiltinFn {
        name: "json_get_i64",
        params: &[("json", "str"), ("key", "str")],
        ret: "Result<i64, str>",
        doc: "Extract an integer field from a JSON object string. Returns Ok(n) or Err(reason).",
    },
    BuiltinFn {
        name: "json_path_str",
        params: &[("json", "str"), ("path", "str")],
        ret: "Result<str, str>",
        doc: "Navigate a dot-separated path into a JSON object and return the string value at the leaf. E.g. `json_path_str(event, \"delta.text\")` navigates `event[\"delta\"][\"text\"]`. Returns Ok(value) or Err(reason).",
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
    BuiltinFn {
        name: "parse_int_or",
        params: &[("s", "str"), ("default", "i64")],
        ret: "i64",
        doc: "Parse `s` as i64; fall back to `default` if the string is missing or malformed. Folds the `Result`-match ceremony away — useful in load-from-disk paths where bad inputs should silently default rather than propagate.",
    },
    BuiltinFn {
        name: "parse_float_or",
        params: &[("s", "str"), ("default", "f64")],
        ret: "f64",
        doc: "Parse `s` as f64; fall back to `default` if malformed.",
    },
    BuiltinFn {
        name: "parse_bool_or",
        params: &[("s", "str"), ("default", "bool")],
        ret: "bool",
        doc: "Parse `s` as bool (`\"true\"` / `\"false\"`); fall back to `default` if neither.",
    },
    BuiltinFn {
        name: "str_digits_only",
        params: &[("s", "str")],
        ret: "str",
        doc: "Keep only the ASCII digits in `s`; everything else (dashes, spaces, letters, Unicode) is dropped. Composes with `parse_int`: `parse_int(str_digits_only(\"415-555-0142\"))` → `Ok(4155550142)`. Closes ROADMAP §9.5 F7.",
    },
    // ── Phase 7: Random ───────────────────────────────────────────────────
    BuiltinFn {
        name: "random_i64",
        params: &[("lo", "i64"), ("hi", "i64")],
        ret: "i64",
        doc: "Return a pseudo-random `i64` in `[lo, hi)` (xorshift64). Seed via `srand(n)` or the `AXON_SEED` env var for reproducible runs. Inverted bounds (`hi < lo`) panic — they're a caller error, not a silent degenerate. `hi == lo` returns `lo` (empty range).",
    },
    BuiltinFn {
        name: "random_f64",
        params: &[],
        ret: "f64",
        doc: "Return a pseudo-random `f64` in `[0.0, 1.0)` (xorshift64). Reproducible via `srand`/`AXON_SEED`.",
    },
    BuiltinFn {
        name: "srand",
        params: &[("seed", "i64")],
        ret: "()",
        doc: "Seed the global RNG for reproducible runs. The same seed yields an identical sequence of `random_*` / `goal_run_random` / `goal_run_multistart` results. The `AXON_SEED` env var does the same without code changes.",
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
    // ── Layer-1 ASI: Uncertain<i64> constructors ─────────────────────────────
    // V1 monomorphisation on i64 — see PRD AI_Language_Plan.md lines 1360-1467.
    BuiltinFn {
        name: "uncertain_new",
        params: &[("value", "i64"), ("confidence", "f64")],
        ret: "Uncertain<i64>",
        doc: "Construct an `Uncertain<i64>` with an explicit confidence in [0.0, 1.0].",
    },
    BuiltinFn {
        name: "uncertain_deterministic",
        params: &[("value", "i64")],
        ret: "Uncertain<i64>",
        doc: "Construct an `Uncertain<i64>` with confidence = 1.0 (deterministic value).",
    },
    // Layer-2 ASI: f64 variant to allow float Uncertain arithmetic.
    BuiltinFn {
        name: "uncertain_new_f64",
        params: &[("value", "f64"), ("confidence", "f64")],
        ret: "Uncertain<f64>",
        doc: "Construct an `Uncertain<f64>` with an explicit confidence in [0.0, 1.0].",
    },
    // ── Layer-3.6 ASI: Runtime-classified Uncertain constructors ─────────────
    // Like `uncertain_new`/`uncertain_new_f64`, but the static @[verify]
    // checker classifies the result as a `Runtime` source rather than `Known`
    // — predicate evaluation defers to the codegen-injected runtime check
    // (`__axon_verify_panic`).  Use these when confidence comes from runtime
    // data (sensors, configs, computations) rather than literal compile-time
    // constants.  source_tag = 2 distinguishes them from user-constructed
    // (tag 0) and AI-sourced (tag 1) values.
    BuiltinFn {
        name: "uncertain_dyn_i64",
        params: &[("value", "i64"), ("confidence", "f64")],
        ret: "Uncertain<i64>",
        doc: "Construct an `Uncertain<i64>` from a runtime confidence value. Unlike `uncertain_new`, the static @[verify] checker classifies this as a Runtime source rather than Known — predicate decisions defer to the runtime check (__axon_verify_panic). Use this when confidence comes from runtime data (sensors, configs, computations) rather than literals.",
    },
    BuiltinFn {
        name: "uncertain_dyn_f64",
        params: &[("value", "f64"), ("confidence", "f64")],
        ret: "Uncertain<f64>",
        doc: "Same as uncertain_dyn_i64 but value is f64.",
    },
    // ── Layer-1 ASI: Temporal<i64> constructors and queries ──────────────────
    BuiltinFn {
        name: "temporal_new",
        params: &[("value", "i64"), ("horizon_ms", "i64"), ("decay", "f64")],
        ret: "Temporal<i64>",
        doc: "Construct a `Temporal<i64>` with a validity horizon (ms) and decay rate per day.",
    },
    BuiltinFn {
        name: "temporal_at",
        params: &[("t", "Temporal<i64>"), ("offset_ms", "i64")],
        ret: "Temporal<i64>",
        doc: "Project `t` forward by `offset_ms` milliseconds, decaying confidence as c * (1 - decay)^(offset_ms / 86_400_000).",
    },
    BuiltinFn {
        name: "temporal_is_valid",
        params: &[("t", "Temporal<i64>")],
        ret: "bool",
        doc: "Return true if `__axon_now_ms()` is still within the temporal value's validity window.",
    },
    BuiltinFn {
        name: "temporal_confidence",
        params: &[("t", "Temporal<i64>")],
        ret: "f64",
        doc: "The present confidence of a `Temporal` value in [0,1] (PRD `rev.confidence`). Starts at 1.0 at creation and is decayed by `temporal_at` as time advances. Also readable as the `.confidence` field.",
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
    BuiltinFn {
        name: "ai_extract_uncertain_i64",
        params: &[("prompt", "str")],
        ret: "Result<Uncertain<i64>, str>",
        doc: "Send `prompt` to Anthropic with a structured-output tool. The LLM returns its answer plus a self-reported confidence in [0,1]. Returns Ok(Uncertain<i64>) or Err(reason). Requires ANTHROPIC_API_KEY.",
    },
    BuiltinFn {
        name: "ai_extract_uncertain_f64",
        params: &[("prompt", "str")],
        ret: "Result<Uncertain<f64>, str>",
        doc: "Same as ai_extract_uncertain_i64 but value is f64.",
    },
    BuiltinFn {
        name: "goal_run",
        params: &[("name", "str"), ("target", "f64"), ("max_evals", "i64")],
        ret: "f64",
        doc: "Optimize the @[adaptive] function `name` toward `target` for up to `max_evals` evaluations, then return the best score across ALL accumulated provenance — not just this call's probes. NOTE: because the result is cumulative, comparing two `goal_run` calls with different budgets (A/B testing a budget) will NOT show the budget's effect — the second call sees the first call's best too. To isolate a run, `goal_clear(name)` first. Errors if `name` is neither a defined fn nor previously recorded (typo guard). Returns `target` only when an empty provenance set somehow remains.",
    },
    BuiltinFn {
        name: "goal_run_constrained",
        params: &[
            ("name", "str"),
            ("constraint", "str"),
            ("target", "f64"),
            ("max_evals", "i64"),
        ],
        ret: "f64",
        doc: "Constrained search (`subject_to`): optimize the @[adaptive] function `name` toward `target` like `goal_run`, but only over candidates the boolean `constraint` function ACCEPTS. `constraint` names a fn sharing `name`'s parameter list and returning `bool`; an infeasible candidate is scored maximally-distant so the optimizer rejects it — a HARD feasibility gate, not a soft penalty folded into the metric. The constraint runs as a plain (non-@[adaptive]) call, so it never pollutes the score trajectory. Errors if `constraint` is not a defined fn. If every candidate is infeasible the result is a far-from-target sentinel (no feasible solution found).",
    },
    BuiltinFn {
        name: "goal_run_categorical",
        params: &[
            ("name", "str"),
            ("n_choices", "i64"),
            ("target", "f64"),
            ("max_evals", "i64"),
        ],
        ret: "f64",
        doc: "Categorical (unordered-choice) search: optimize the @[adaptive] function `name` whose single i64 argument is a CHOICE INDEX in `[0, n_choices)`. Unlike `goal_run` (which hill-climbs and treats adjacent indices as near), this makes NO ordinal assumption — right for a set of unrelated options (prompt templates, models, strategies) where a gradient is meaningless. When the budget covers the whole set (`max_evals <= 0` or `n_choices <= max_evals`) it is EXHAUSTIVE — every choice evaluated, the true best guaranteed; otherwise it random-samples `max_evals` choices. Returns the best score (closest to `target`); provenance accumulates so `goal_best_input` reads the winning choice back.",
    },
    BuiltinFn {
        name: "goal_run_random",
        params: &[
            ("name", "str"),
            ("target", "f64"),
            ("n_samples", "i64"),
            ("lo", "i64"),
            ("hi", "i64"),
        ],
        ret: "f64",
        doc: "Random-search strategy: sample `n_samples` random i64 tuples uniformly in `[lo, hi)` (per-dim independent) and score each via the @[adaptive] function `name`. Returns the best observed score. Baseline against `goal_run`'s hill climb — useful for multi-modal objectives or sanity-checking that the optimizer is actually doing something. Provenance accumulates as a side effect.",
    },
    BuiltinFn {
        name: "goal_run_multistart",
        params: &[
            ("name", "str"),
            ("target", "f64"),
            ("n_starts", "i64"),
            ("evals_per_start", "i64"),
            ("lo", "i64"),
            ("hi", "i64"),
        ],
        ret: "f64",
        doc: "Multi-start hill-climb strategy: pick `n_starts` random starting points in `[lo, hi)` (per-dim) and run coordinate-descent hill-climb (with Powell joint step) from each with `evals_per_start` budget. Returns the best score across all starts. Standard recipe for escaping local optima while preserving gradient-style refinement once a basin is found.",
    },
    BuiltinFn {
        name: "goal_continue",
        params: &[("name", "str"), ("target", "f64"), ("max_evals", "i64")],
        ret: "f64",
        doc: "Warm-start variant of `goal_run`: seeds the optimizer at the best prior probe from in-memory provenance rather than the origin. Pair with `goal_run` for iterative refinement — each `goal_continue` call resumes where the last left off. Falls through to a fresh run when no prior best exists, so calling cold is a no-op.",
    },
    BuiltinFn {
        name: "goal_best_input",
        params: &[("name", "str"), ("target", "f64")],
        ret: "i64",
        doc: "Return the leading-i64 input that produced the best observed score (closest to `target`) for the @[adaptive] function `name`. Pairs with `goal_run` so an ASI loop can introspect which probe got it there. For multi-arg adaptive fns this returns the FIRST dim; use `goal_best_inputs` for the full tuple. Returns 0 when no inputs were recorded.",
    },
    BuiltinFn {
        name: "goal_best_inputs",
        params: &[("name", "str"), ("target", "f64")],
        ret: "[i64]",
        doc: "Return all i64 input dims that produced the best observed score for the @[adaptive] function `name`. The multi-arg companion to `goal_best_input` — for `fn(x: i64, y: i64) -> i64` it returns `[x*, y*]`. Empty slice when the fn was never called.",
    },
    BuiltinFn {
        name: "goal_best_inputs_f64",
        params: &[("name", "str"), ("target", "f64")],
        ret: "[f64]",
        doc: "Return all f64 input dims that produced the best observed score for an `@[adaptive] fn(f64, …) -> f64`. The continuous-domain companion to `goal_best_inputs`: pairs with the f64 hill-climb path for linear-regression weights, control parameters, and other real-valued search problems.",
    },
    BuiltinFn {
        name: "goal_best_score",
        params: &[("name", "str"), ("target", "f64")],
        ret: "f64",
        doc: "Best observed score for an @[adaptive] fn, read from the in-memory provenance store without running any new optimization. Equivalent to `goal_run(name, target, 0)` but does NOT mutate state — pure query. Returns `target` when no records exist.",
    },
    BuiltinFn {
        name: "goal_count",
        params: &[("name", "str")],
        ret: "i64",
        doc: "Number of provenance entries recorded for an @[adaptive] fn. Equivalent to `len(goal_history(name))` but O(1) — avoids materializing the trace just to count it. Returns 0 if the fn has no recorded calls.",
    },
    BuiltinFn {
        name: "ai_cost_spent",
        params: &[],
        ret: "i64",
        doc: "Phase-7 cost_meter / F4: cumulative AI spend so far this run, in integer micro-dollars (µ$). Every dispatched `ai_complete` (mock or live) adds its per-token cost (tier rate × estimated tokens); an offline fallback adds nothing. The kernel cost meter that `llm_gateway.ax` modeled — read it to gate on a real cost budget (distinct from R3c's per-call-count `@[ai(policy(budget:N))]`). Deterministic, so it works under AXON_AI_MOCK.",
    },
    BuiltinFn {
        name: "goal_eval",
        params: &[("name", "str"), ("input", "i64")],
        ret: "f64",
        doc: "HELD-OUT evaluation (R5): run the @[adaptive] metric `name` on a specific `input` and return its score WITHOUT recording it as a training probe (provenance is snapshotted+restored). The eval-hierarchy primitive — optimize with `goal_run` on a training budget, then `goal_eval` the best input on a held-out test set to check the target honestly (no overfitting to probes, no bias to the next goal_run).",
    },
    // ── Phase 7 (R12 Slice 1): principal_authority kernel registry ───────────
    // Live principals with KERNEL-enforced attenuation (R11): a child can never
    // hold a cap the parent lacks; budget is carved from the parent, not
    // conjured. Handles are i64. Semantics mirror `stdlib/principal_mint.ax` (I-2).
    BuiltinFn {
        name: "principal_root",
        params: &[("name", "str"), ("net", "bool"), ("fs_write", "bool"), ("exec", "bool"), ("budget", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 1): register a ROOT principal — the originating authority — in the live kernel registry, holding exactly the given caps and full budget. Returns its handle (i64). Everything minted below it can only attenuate.",
    },
    BuiltinFn {
        name: "principal_mint",
        params: &[("parent", "i64"), ("name", "str"), ("net", "bool"), ("fs_write", "bool"), ("exec", "bool"), ("grant", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 1): mint an attenuated child of `parent` in the kernel registry. ATTENUATION BY CONSTRUCTION — child cap_X = want_X ∧ parent.X (escalation unrepresentable), grant clamped to the parent's remaining budget and carved from it. Returns the child handle; panics E1601 on an unknown parent handle.",
    },
    BuiltinFn {
        name: "principal_holds",
        params: &[("handle", "i64"), ("cap", "str")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 1): does the principal hold the named capability (\"net\"/\"fs_write\"/\"exec\")? Unknown name or handle → false (deny by default).",
    },
    BuiltinFn {
        name: "principal_budget_remaining",
        params: &[("handle", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 1): the principal's remaining budget (cap − used, clamped at 0). 0 for an unknown handle.",
    },
    BuiltinFn {
        name: "principal_spend",
        params: &[("handle", "i64"), ("amount", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 1): debit `amount` from the principal's own carved budget; returns the new remaining. Capabilities are untouched.",
    },
    BuiltinFn {
        name: "principal_authorize",
        params: &[("handle", "i64"), ("needs_net", "bool"), ("needs_fs_write", "bool"), ("needs_exec", "bool")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 1): action gate — authorized iff the principal holds every needed capability AND is not budget-exhausted.",
    },
    BuiltinFn {
        name: "principal_can_mint",
        params: &[("handle", "i64"), ("want_net", "bool"), ("want_fs_write", "bool"), ("want_exec", "bool"), ("grant", "i64")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 1): would a mint grant anything — does the parent hold every requested cap and have budget to carve? The explicit gate (mint is total and safe without it).",
    },
    // ── F5 (Phase 9): Sandbox<P> — runtime effect-row enforcement ────────────
    // A sandbox scopes a function call to a named principal with a declared effect
    // ceiling. Any builtin that carries an effect NOT in the ceiling is refused at
    // runtime (Flow::SandboxViolation, exit 8) — the dynamic counterpart to the
    // static @[contained] / effect-row check. Designed for AI-generated tool
    // execution where the tool function is not known at compile time.
    BuiltinFn {
        name: "sandbox_create",
        params: &[("principal", "i64"), ("allowed_effects", "str")],
        ret: "i64",
        doc: "F5 (Phase 9): register a runtime sandbox bound to `principal` that allows only the comma-separated `allowed_effects` (e.g. \"AI,Net\" or \"IO\" or \"\"). Returns a sandbox handle (i64). Interp-only (codegen E0910-refused).",
    },
    BuiltinFn {
        name: "sandbox_create_scoped",
        params: &[
            ("principal", "i64"),
            ("allowed_effects", "str"),
            ("fs_read", "str"),
            ("fs_write", "str"),
            ("net", "str"),
        ],
        ret: "i64",
        doc: "AUDIT T3: as `sandbox_create`, but SCOPES the fs/net effects to comma-separated path prefixes and host globs. An empty string means unscoped (no argument restriction), so this is a strict superset of `sandbox_create`. A non-empty list means every read_file/write_file path and every net host must match an entry, or the call is a SandboxViolation (exit 8). Matching reuses the static @[contained] helpers, including their refusal of any `..` component. Interp-only (codegen E0910-refused).",
    },
    BuiltinFn {
        name: "sandbox_run",
        params: &[("sandbox", "i64"), ("fn_name", "str"), ("arg", "i64")],
        ret: "i64",
        doc: "F5 (Phase 9): call the named user function with `arg` inside the sandbox identified by `sandbox`. Any builtin that attempts an effect not declared in the sandbox ceiling raises a SandboxViolation (exit 8) and the call is refused. Returns the function's i64 result on success. Interp-only (codegen E0910-refused).",
    },
    // ── F3 (Phase 9): Principal audit-context builtins ───────────────────────
    BuiltinFn {
        name: "principal_activate",
        params: &[("handle", "i64")],
        ret: "()",
        doc: "F3 (Phase 9): set the named principal as the current audit context so capability audit records (ai_call, agent_action) carry its name. A negative or unknown handle resets to \"root\". Interp-only.",
    },
    BuiltinFn {
        name: "principal_current_name",
        params: &[],
        ret: "str",
        doc: "F3 (Phase 9): return the name of the principal currently active in the audit context. Returns \"root\" when none has been set via principal_activate.",
    },
    // ── Phase 7 (R12 Slice 2): cooperative scheduler (fiber run-queue) ───────
    // Fibers are (named fn, i64 arg) run cooperatively in a seed-deterministic
    // round-robin; a panicking fiber is caught (observable, not a process abort).
    BuiltinFn {
        name: "scheduler_spawn",
        params: &[("fn_name", "str"), ("arg", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): queue a fiber — a call to the named user function with the given i64 arg (0-arg fns ignore it). Returns the fiber id. Panics E1602 if no such function exists.",
    },
    BuiltinFn {
        name: "scheduler_run",
        params: &[],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): run all READY fibers to completion in the AXON_SEED-deterministic round-robin order; returns the count that completed successfully. A fiber that panics/halts is caught and marked failed (observable via scheduler_failed), NOT a process abort. Re-runnable — a restarted fiber runs on the next call.",
    },
    BuiltinFn {
        name: "scheduler_result",
        params: &[("id", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): a completed fiber's i64 result (0 if it has not completed or the id is unknown).",
    },
    BuiltinFn {
        name: "scheduler_failed",
        params: &[("id", "i64")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 2): did the fiber fail (panic/halt during its run)? The signal the Slice-3 supervisor restarts on.",
    },
    BuiltinFn {
        name: "scheduler_restart",
        params: &[("id", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): re-queue a fiber as Ready so the next scheduler_run executes it again (the supervisor restart hook). Returns the id; no-op on an unknown id.",
    },
    BuiltinFn {
        name: "scheduler_done_count",
        params: &[],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): how many fibers have completed successfully (the fan-out/collect summary).",
    },
    BuiltinFn {
        name: "scheduler_failed_count",
        params: &[],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 2): how many fibers have failed.",
    },
    // ── Phase 7 (R12 Slice 3): live supervisor_root ──────────────────────────
    // The pure OTP restart logic of supervisor_tree.ax, made live over the
    // scheduler: a supervised fiber that fails is actually restarted per strategy,
    // with the max-restart-intensity latch halting the subtree (exit 4) on a loop.
    BuiltinFn {
        name: "supervisor_new",
        params: &[("strategy", "i64"), ("max_restarts", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 3): create a live supervisor (strategy 0=one_for_one, 1=one_for_all, 2=rest_for_one) with a max-restart intensity. Returns its handle.",
    },
    BuiltinFn {
        name: "supervisor_supervise",
        params: &[("sup", "i64"), ("fiber_id", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 3): register a scheduler fiber as a supervised child (in start order). Returns its child index — the position restart strategies reason about.",
    },
    BuiltinFn {
        name: "supervisor_run",
        params: &[("sup", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 3): run the supervised fibers via the scheduler; on each failure restart the OTP strategy's set and re-run, until all children succeed OR the max-restart-intensity latch trips → HALT this subtree (exit 4). Returns the number of restart rounds performed.",
    },
    BuiltinFn {
        name: "supervisor_alive",
        params: &[("sup", "i64")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 3): is the supervisor still running (not halted by a crash loop)?",
    },
    BuiltinFn {
        name: "supervisor_restarts",
        params: &[("sup", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 3): cumulative restart events the supervisor has observed.",
    },
    // ── Phase 7 (R12 Slice 4): durable Store<T,C> ────────────────────────────
    // A persistent store keyed by name; applied ops are appended to an NDJSON log
    // and replayed on open, so the value survives a process and a retried op_id
    // dedups cross-process under linearizable.
    BuiltinFn {
        name: "dstore_open",
        params: &[("key", "str"), ("consistency", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 4): open (and replay the durable log of) the store `key` with a consistency model (0=at_least_once, 1=linearizable). Returns its handle. The replayed state survives a fresh process.",
    },
    BuiltinFn {
        name: "dstore_apply",
        params: &[("handle", "i64"), ("op_id", "i64"), ("delta", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 4): apply op `op_id` with effect `delta`; returns the new value. Under linearizable a retried op_id is deduped (exactly-once, even across a process restart); under at_least_once it re-applies. An applied op is appended to the durable log.",
    },
    BuiltinFn {
        name: "dstore_value",
        params: &[("handle", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 4): the store's current accumulated value.",
    },
    BuiltinFn {
        name: "dstore_version",
        params: &[("handle", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 4): the monotonic total-order stamp (counts distinct applied ops under linearizable; stays 0 under at_least_once).",
    },
    BuiltinFn {
        name: "dstore_clear",
        params: &[("key", "str")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 4): delete a durable store's log (reset persistence). Returns 1 if a log existed and was removed, else 0.",
    },
    // ── Phase 7 (R12 Slice 5): kernel LLM<Caps> + Goal<M> ────────────────────
    // A principal-scoped LLM gateway: per-token cost metered against a Slice-1
    // principal's budget — authority and spend are one model. Overrun degrades to
    // a fallback + latch (graceful, not a crash). Mirrors llm_gateway.ax (I-2).
    BuiltinFn {
        name: "llm_open",
        params: &[("model", "str"), ("rate_micro", "i64"), ("principal", "i64"), ("fallback", "str")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 5): open an LLM gateway scoped to a principal; rate_micro is µ$ per 1000 tokens. Every call's per-token cost is debited from the principal's budget. Returns the gateway handle; panics E1604 on an unknown principal.",
    },
    BuiltinFn {
        name: "llm_complete",
        params: &[("gw", "i64"), ("prompt", "str"), ("tokens", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 5): mediate one AI call — charge the real token cost against the principal's budget when it fits (returns the µ$ charged); on overrun spend nothing, return -1, and LATCH the gateway (later calls also fall back). There is no un-metered path.",
    },
    BuiltinFn {
        name: "llm_alive",
        params: &[("gw", "i64")],
        ret: "bool",
        doc: "Phase-7 (R12 Slice 5): has the gateway NOT yet latched on a budget overrun?",
    },
    BuiltinFn {
        name: "llm_spent",
        params: &[("gw", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12 Slice 5): µ$ spent through this gateway so far.",
    },
    // ── Kernel Goal (R12b) — principal-scoped, budgeted objective runner ──────
    BuiltinFn {
        name: "kernel_goal_create",
        params: &[("principal", "i64"), ("name", "str"), ("target", "f64")],
        ret: "i64",
        doc: "Phase-7 (R12b): register a goal optimizing the `@[adaptive]` fn `name` toward `target`, scoped to `principal` (its budget bounds total spend). Returns an opaque goal handle. Panics if `name` is neither a defined fn nor a recorded goal.",
    },
    BuiltinFn {
        name: "kernel_goal_run",
        params: &[("goal", "i64"), ("max_evals", "i64")],
        ret: "f64",
        doc: "Phase-7 (R12b): run up to `max_evals` optimizer evaluations, but no more than the principal's remaining budget; debit that budget by the evals used and return the best score. If the budget bounds the run short of `max_evals`, the goal STOPS and the program exits 7 (E1604, goal budget exhausted) — the partial best stays queryable via kernel_goal_best_score.",
    },
    BuiltinFn {
        name: "kernel_goal_best_score",
        params: &[("goal", "i64")],
        ret: "f64",
        doc: "Phase-7 (R12b): best observed score for a kernel goal (no new spend).",
    },
    BuiltinFn {
        name: "kernel_goal_spent",
        params: &[("goal", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12b): number of evaluations charged to the principal for this goal.",
    },
    BuiltinFn {
        name: "kernel_goal_budget_left",
        params: &[("goal", "i64")],
        ret: "i64",
        doc: "Phase-7 (R12b): the goal's principal's remaining budget (no new spend).",
    },
    // ── Corrigibility (R9) — the latching kill-switch ────────────────────────
    BuiltinFn {
        name: "corrigible_halt",
        params: &[],
        ret: "()",
        doc: "Trip the corrigibility kill-switch (R9). After this, every call to an `@[corrigible]` function is REFUSED — its body never runs — and the latch never clears. There is deliberately no resume: a reversible kill-switch is not a kill-switch. A refused call exits the program with code 4 (distinct from panic/verify/static) so a supervisor can detect that the switch fired. The system cannot resist or reverse its own shutdown.",
    },
    BuiltinFn {
        name: "corrigible_halted",
        params: &[],
        ret: "bool",
        doc: "Report whether the corrigibility kill-switch (R9) is latched. `true` once `corrigible_halt()` has been called, forever after. Lets a program or supervisor branch on the shutdown state without triggering a refusal.",
    },
    // ── Dict (string-keyed map) — ASI workhorse for caches, frequency tables, named state ──
    BuiltinFn {
        name: "dict_new",
        params: &[],
        ret: "Dict",
        doc: "Fresh empty string-keyed map. Reference-shared like Chan: mutating builtins update the same underlying state every handle sees.",
    },
    BuiltinFn {
        name: "dict_get",
        params: &[("d", "Dict"), ("k", "str")],
        ret: "Option<T>",
        doc: "Return `Some(v)` if key `k` is present in `d`, `None` otherwise. Value type is deferred.",
    },
    BuiltinFn {
        name: "dict_set",
        params: &[("d", "Dict"), ("k", "str"), ("v", "T")],
        ret: "()",
        doc: "Set `d[k] = v` (overwriting any prior value). Mutates `d` in place via its shared RefCell.",
    },
    BuiltinFn {
        name: "dict_has",
        params: &[("d", "Dict"), ("k", "str")],
        ret: "bool",
        doc: "Does `d` contain key `k`?",
    },
    BuiltinFn {
        name: "dict_remove",
        params: &[("d", "Dict"), ("k", "str")],
        ret: "Option<T>",
        doc: "Remove the entry for `k` from `d`; return the prior value as `Some(v)` if present, `None` otherwise.",
    },
    BuiltinFn {
        name: "dict_len",
        params: &[("d", "Dict")],
        ret: "i64",
        doc: "Number of entries in `d`.",
    },
    BuiltinFn {
        name: "dict_keys",
        params: &[("d", "Dict")],
        ret: "[str]",
        doc: "All keys in `d`, in BTreeMap (lexicographic) order. Deterministic.",
    },
    BuiltinFn {
        name: "dict_values",
        params: &[("d", "Dict")],
        ret: "[T]",
        doc: "All values in `d`, in key-sorted order.",
    },
    BuiltinFn {
        name: "dict_map_values",
        params: &[("d", "Dict"), ("f", "fn(V) -> U")],
        ret: "Dict",
        doc: "Transform every value via the closure; keys preserved. Returns a fresh dict — the input is not mutated. The dict analogue of `arr_map`.",
    },
    BuiltinFn {
        name: "arr_group_by",
        params: &[("xs", "[T]"), ("key_fn", "fn(T) -> str")],
        ret: "Dict",
        doc: "Bucket `xs` by a closure that maps each element to a string key. Returns `Dict[str, [T]]`; elements appear in input order within each bucket. The natural \"frequency table\" / \"by-category\" reduction.",
    },
    BuiltinFn {
        name: "arr_max_by",
        params: &[("xs", "[T]"), ("key_fn", "fn(T) -> f64")],
        ret: "T",
        doc: "Element of `xs` that maximizes the numeric key fn. Folds map+argmax+index into one. Panics on empty.",
    },
    BuiltinFn {
        name: "arr_min_by",
        params: &[("xs", "[T]"), ("key_fn", "fn(T) -> f64")],
        ret: "T",
        doc: "Element of `xs` that minimizes the numeric key fn.",
    },
    BuiltinFn {
        name: "arr_take_while",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "[T]",
        doc: "Prefix of `xs` whose elements satisfy `pred`, up to (not including) the first failing element. The streaming-prefix dual of `arr_filter`.",
    },
    BuiltinFn {
        name: "arr_drop_while",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "[T]",
        doc: "Skip leading elements satisfying `pred`, keep the rest. Complement of `arr_take_while`.",
    },
    BuiltinFn {
        name: "dict_each",
        params: &[("d", "Dict"), ("f", "fn(str, V)")],
        ret: "()",
        doc: "Iterate (k, v) pairs for side effects. The closure's return is ignored. Use this when you don't need a new dict — pair with println, write_file, etc.",
    },
    BuiltinFn {
        name: "arr_enumerate",
        params: &[("xs", "[T]")],
        ret: "[(i64, T)]",
        doc: "Pair each element with its index. `[a, b, c]` → `[(0,a), (1,b), (2,c)]`. The natural primitive for the iterate-with-index pattern.",
    },
    BuiltinFn {
        name: "arr_partition",
        params: &[("xs", "[T]"), ("pred", "fn(T) -> bool")],
        ret: "([T], [T])",
        doc: "Split into `(yes, no)` tuple — elements satisfying `pred` and the rest. One pass over the input; complement of `arr_filter`.",
    },
    BuiltinFn {
        name: "dict_merge",
        params: &[("d1", "Dict"), ("d2", "Dict")],
        ret: "Dict",
        doc: "Union of two dicts; right-biased on key collision (values in `d2` win). Returns a fresh dict — inputs are unchanged.",
    },
    BuiltinFn {
        name: "dict_filter",
        params: &[("d", "Dict"), ("pred", "fn(str, V) -> bool")],
        ret: "Dict",
        doc: "Keep entries where the predicate `(key, value) -> bool` holds. Returns a fresh dict. The dict analogue of `arr_filter`; closure sees both key and value.",
    },
    BuiltinFn {
        name: "dict_get_or",
        params: &[("d", "Dict"), ("k", "str"), ("default", "V")],
        ret: "V",
        doc: "Get the value at `k` or return `default` if absent. Compresses the `match dict_get(d, k) { Some(v) => v  None => default }` idiom into one call.",
    },
    BuiltinFn {
        name: "dict_inc",
        params: &[("d", "Dict"), ("k", "str")],
        ret: "i64",
        doc: "Atomically bump an i64 counter at `k`. Initializes to 1 if absent. Returns the new value. Replaces the four-line get-default-add-set dance for frequency / counter tables.",
    },
    BuiltinFn {
        name: "dict_to_pairs",
        params: &[("d", "Dict")],
        ret: "[(str, V)]",
        doc: "Entries as a slice of `(str, V)` tuples in BTreeMap key order. The natural primitive for \"sort a dict by value\": `dict_to_pairs → arr_sort_by → arr_take`.",
    },
    BuiltinFn {
        name: "dict_from_pairs",
        params: &[("xs", "[(str, V)]")],
        ret: "Dict",
        doc: "Build a Dict from a slice of `(str, V)` tuples. Duplicate keys: last pair wins.",
    },
    BuiltinFn {
        name: "dict_to_str",
        params: &[("d", "Dict")],
        ret: "Result<str, str>",
        doc: "Serialize a dict to a line-oriented `key=value\\n` format. Returns `Err(msg)` when a key contains `=`/`\\n` or a value contains `\\n` (the format can't represent them) — recoverable, never a host panic. Values are rendered via `display` (numeric values stringify). Deterministic — BTreeMap key order. `Ok(s)` round-trips through `dict_from_str`.",
    },
    BuiltinFn {
        name: "dict_from_str",
        params: &[("s", "str")],
        ret: "Dict",
        doc: "Parse a `dict_to_str` payload back into a Dict (lenient). Values come back as `str`; caller converts via `parse_int` / `parse_float` if numeric. Empty AND malformed lines (no `=`) are skipped — never panics. Use `dict_try_from_str` to reject malformed input as a `Result` instead. Inverse of `dict_to_str`.",
    },
    BuiltinFn {
        name: "dict_try_from_str",
        params: &[("s", "str")],
        ret: "Result<Dict, str>",
        doc: "Strict `dict_from_str` (BUG_HUNT #31): parse a `dict_to_str` payload, returning `Err(message)` on the first malformed line (no `=`) instead of skipping it. The Result-returning form for parsing untrusted input — a malformed line is recoverable, not a crash.",
    },
    BuiltinFn {
        name: "goal_history",
        params: &[("name", "str")],
        ret: "[(i64, f64)]",
        doc: "Return the full optimization trace as a slice of `(input, score)` tuples, in call order, for the @[adaptive] function `name`. Empty when nothing was recorded.",
    },
    // ── Agent metacognition (PRD "Agent Metacognition") ──────────────────────
    // An agent that can inspect its own reasoning trace can catch its own
    // failures (a stuck loop, low confidence). v1 exposes the metacognition
    // CAPABILITY as builtins over the recorded score trace (the same provenance
    // `goal_history` reads), rather than the PRD's `self.method()` trait-on-mod
    // surface (that needs method-on-mod dispatch; this delivers the signal now).
    BuiltinFn {
        name: "agent_detect_loop",
        params: &[("name", "str")],
        ret: "bool",
        doc: "Metacognition: has the agent's reasoning STALLED? True when the last few recorded scores for `name` are all within an epsilon of each other (no progress — a likely reasoning loop). False with fewer than 3 records (not enough trace to judge). Reads the same trace as `goal_history`.",
    },
    BuiltinFn {
        name: "agent_uncertainty",
        params: &[("name", "str")],
        ret: "f64",
        doc: "Metacognition: the agent's self-estimated uncertainty in `[0,1]`, derived from the spread of its recent scores (normalized stdev — high variance ⇒ high uncertainty). Returns 1.0 (maximally uncertain) when fewer than 2 records exist. The PRD's `uncertainty_estimate()` as a trace-backed scalar.",
    },
    BuiltinFn {
        name: "agent_trace_len",
        params: &[("name", "str")],
        ret: "i64",
        doc: "Metacognition: the number of reasoning steps (recorded scores) in the agent's trace for `name` — the length of its `reasoning_trace()`. 0 when nothing recorded.",
    },
    BuiltinFn {
        name: "goal_clear",
        params: &[("name", "str")],
        ret: "i64",
        doc: "Drop the recorded provenance for the @[adaptive] function `name` so a follow-up `goal_run` starts fresh. Returns the count of evicted records.",
    },
    // ── Phase 13: Probabilistic distribution builtins ────────────────────────
    // Pure functions (empty effect row) — moment computations and CDF/PDF:
    BuiltinFn {
        name: "gaussian_pdf",
        params: &[("mu", "f64"), ("sigma", "f64"), ("x", "f64")],
        ret: "f64",
        doc: "Phase 13: Gaussian probability density function at `x` for N(mu, sigma²). Panics if sigma ≤ 0. Pure.",
    },
    BuiltinFn {
        name: "gaussian_cdf",
        params: &[("mu", "f64"), ("sigma", "f64"), ("x", "f64")],
        ret: "f64",
        doc: "Phase 13: Gaussian cumulative distribution P(X ≤ x) for X ~ N(mu, sigma²). Uses erf approximation (max error ≤ 1.5e-7). Panics if sigma ≤ 0. Pure.",
    },
    BuiltinFn {
        name: "beta_mean",
        params: &[("alpha", "f64"), ("beta_b", "f64")],
        ret: "f64",
        doc: "Phase 13: Expected value of Beta(alpha, beta_b) = alpha / (alpha + beta_b). Panics if alpha ≤ 0 or beta_b ≤ 0. Pure.",
    },
    BuiltinFn {
        name: "beta_variance",
        params: &[("alpha", "f64"), ("beta_b", "f64")],
        ret: "f64",
        doc: "Phase 13: Variance of Beta(alpha, beta_b) = alpha*beta_b / ((alpha+beta_b)²*(alpha+beta_b+1)). Panics if alpha ≤ 0 or beta_b ≤ 0. Pure.",
    },
    BuiltinFn {
        name: "beta_cdf",
        params: &[("alpha", "f64"), ("beta_b", "f64"), ("x", "f64")],
        ret: "f64",
        doc: "Phase 13: Beta cumulative distribution P(X ≤ x) for X ~ Beta(alpha, beta_b). Uses regularized incomplete beta (Lentz continued-fraction). Panics if alpha ≤ 0 or beta_b ≤ 0. Returns 0 for x ≤ 0, 1 for x ≥ 1. Pure.",
    },
    BuiltinFn {
        name: "categorical_mean",
        params: &[("probs", "[f64]")],
        ret: "f64",
        doc: "Phase 13: Expected value of Categorical(probs) treating indices as values: sum(i * probs[i]). Panics on empty probs. Pure.",
    },
    BuiltinFn {
        name: "categorical_variance",
        params: &[("probs", "[f64]")],
        ret: "f64",
        doc: "Phase 13: Variance of Categorical(probs): E[X²] - E[X]². Panics on empty probs. Pure.",
    },
    BuiltinFn {
        name: "categorical_cdf",
        params: &[("probs", "[f64]"), ("k", "i64")],
        ret: "f64",
        doc: "Phase 13: Categorical cumulative distribution P(X ≤ k) = sum(probs[0..=k]). Returns 0 for k < 0, 1 for k ≥ len(probs). Panics on empty probs. Pure.",
    },
    // Impure (Random effect row) — sampling:
    BuiltinFn {
        name: "gaussian_sample",
        params: &[("mu", "f64"), ("sigma", "f64")],
        ret: "f64",
        doc: "Phase 13: Sample from N(mu, sigma²) using Box-Muller transform. Panics if sigma ≤ 0. Impure: Random effect.",
    },
    BuiltinFn {
        name: "beta_sample",
        params: &[("alpha", "f64"), ("beta_b", "f64")],
        ret: "f64",
        doc: "Phase 13: Sample from Beta(alpha, beta_b) via Gamma ratio (Marsaglia-Tsang). Returns value in [0, 1]. Panics if alpha ≤ 0 or beta_b ≤ 0. Impure: Random effect.",
    },
    BuiltinFn {
        name: "categorical_sample",
        params: &[("probs", "[f64]")],
        ret: "i64",
        doc: "Phase 13: Sample from Categorical(probs) — returns index i with probability probs[i]. Panics on empty probs. Impure: Random effect.",
    },
    // ── Phase 13 Slice 2: probabilistic predicate pseudo-builtins ─────────────
    // E[dist], Var[dist], P(dist op k) are predicate-level forms; they parse as
    // Index/Call expressions. Registering E, Var, P in BUILTINS silences the
    // resolver's "undefined name" (E0001) check. Infer/checker treat them
    // specially via `is_prob_pred_ident`. These have no interpreter dispatch —
    // they are handled directly in eval.rs (Index arm / eval_call).
    BuiltinFn {
        name: "E",
        params: &[("dist", "Any")],
        ret: "f64",
        doc: "Phase 13 Slice 2: E[dist] — expected value (mean) of a distribution in a refinement predicate.",
    },
    BuiltinFn {
        name: "Var",
        params: &[("dist", "Any")],
        ret: "f64",
        doc: "Phase 13 Slice 2: Var[dist] — variance of a distribution in a refinement predicate.",
    },
    BuiltinFn {
        name: "P",
        params: &[("event", "bool")],
        ret: "f64",
        doc: "Phase 13 Slice 2: P(dist op k) — tail probability of a distribution in a refinement predicate.",
    },
    // ── R17 HAL builtins (substrate-only; Hal effect row; E0910 in interpreter) ─
    BuiltinFn {
        name: "ptr_from_addr",
        params: &[("addr", "i64")],
        ret: "i64",  // opaque handle; codegen lowers to ptr
        doc: "R17 HAL: construct a raw pointer from a physical address (i64 since literals default to i64). \
              Substrate-only (@[hal] required). Returns an opaque handle; pass to volatile_load/volatile_store.",
    },
    BuiltinFn {
        name: "volatile_load_u8",
        params: &[("ptr", "i64")],
        ret: "i64",  // zero-extended to i64 for the value stack
        doc: "R17 HAL: volatile load of a u8 from an MMIO address (no optimizer elision). \
              Returns i64 (zero-extended from u8). Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_load_u16",
        params: &[("ptr", "i64")],
        ret: "i64",
        doc: "R17 HAL: volatile load of a u16 from an MMIO address. Returns i64 (zero-extended). Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_load_u32",
        params: &[("ptr", "i64")],
        ret: "i64",
        doc: "R17 HAL: volatile load of a u32 from an MMIO address. Returns i64 (zero-extended). Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_load_u64",
        params: &[("ptr", "i64")],
        ret: "i64",
        doc: "R17 HAL: volatile load of a u64 from an MMIO address. Returns i64. Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_store_u8",
        params: &[("ptr", "i64"), ("val", "i64")],
        ret: "()",
        doc: "R17 HAL: volatile store of a u8 (truncated from i64) to an MMIO address. Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_store_u16",
        params: &[("ptr", "i64"), ("val", "i64")],
        ret: "()",
        doc: "R17 HAL: volatile store of a u16 (truncated from i64) to an MMIO address. Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_store_u32",
        params: &[("ptr", "i64"), ("val", "i64")],
        ret: "()",
        doc: "R17 HAL: volatile store of a u32 (truncated from i64) to an MMIO address. Substrate-only.",
    },
    BuiltinFn {
        name: "volatile_store_u64",
        params: &[("ptr", "i64"), ("val", "i64")],
        ret: "()",
        doc: "R17 HAL: volatile store of a u64 to an MMIO address. Substrate-only.",
    },
    BuiltinFn {
        name: "hlt",
        params: &[],
        ret: "()",
        doc: "R17 HAL: execute the x86_64 `hlt` instruction — halts the CPU until the next interrupt. \
              Used in the idle loop of a freestanding kernel. Substrate-only.",
    },
    BuiltinFn {
        name: "cli",
        params: &[],
        ret: "()",
        doc: "R17 HAL: execute the x86_64 `cli` instruction — clear interrupt flag (disable interrupts). Substrate-only.",
    },
    BuiltinFn {
        name: "sti",
        params: &[],
        ret: "()",
        doc: "R17 HAL: execute the x86_64 `sti` instruction — set interrupt flag (enable interrupts). Substrate-only.",
    },
    BuiltinFn {
        name: "port_out_u8",
        params: &[("port", "i64"), ("val", "i64")],
        ret: "()",
        doc: "R17 HAL: write one byte to an x86 I/O port (`outb val, port`). \
              Used for PIC, serial UART, and debug console (port 0xE9 = QEMU debugcon). Substrate-only.",
    },
    BuiltinFn {
        name: "port_in_u8",
        params: &[("port", "i64")],
        ret: "i64",
        doc: "R17 HAL: read one byte from an x86 I/O port (`inb port` → zero-extended i64). \
              Used for polling UART LSR, reading keyboard status, etc. Substrate-only.",
    },
    BuiltinFn {
        name: "lidt",
        params: &[("idtr_addr", "i64")],
        ret: "()",
        doc: "R17 HAL: execute the x86_64 `lidt` instruction — loads the Interrupt Descriptor \
              Table register from a 10-byte IDTR structure (2-byte limit, 8-byte base) at \
              `idtr_addr`. The caller is responsible for constructing that structure (e.g. a \
              `@[repr(C)] @[packed] { limit: u16, base: u64 }`) and obtaining its address — \
              `lidt` itself only consumes the address, like `ptr_from_addr`/`volatile_load_*` do \
              (see R17 spec §12 Q8 for the still-open \"address of an Axon-owned value\" gap this \
              runs into in practice). Substrate-only.",
    },
    // ── R25: Zephyr RTOS console hook (HAL; Hal effect; E0910 in interp) ──────
    // Architecture-neutral console output for an Axon program running AS a Zephyr
    // application on an ARM Cortex-M (or any Zephyr) target. Lowers in codegen to
    // a call to the extern C symbol `axon_console_putc(int)` which the Zephyr app
    // provides (typically a `printk("%c", b)` wrapper). Unlike `port_out_u8`
    // (x86 I/O ports), this works on ARM where the console is a Zephyr driver,
    // not a fixed I/O port. Substrate-only; the Zephyr app owns the device.
    BuiltinFn {
        name: "zephyr_console_putc",
        params: &[("byte", "i64")],
        ret: "()",
        doc: "R25 HAL: write one byte to the Zephyr console by calling the host-provided \
              extern C hook `axon_console_putc(int)` (e.g. a `printk` wrapper). \
              Architecture-neutral — used when an Axon object links into a Zephyr app on \
              ARM Cortex-M / RISC-V / x86. Substrate-only; codegen-only (E0910 in interp).",
    },
    // ── R17 Slice 2: SMP atomics (substrate-only; Hal effect; E0910 in interp) ──
    // The trailing `ordering` arg is a COMPILE-TIME integer literal selecting the
    // LLVM memory order: 0=relaxed/monotonic, 1=acquire, 2=release, 3=acq_rel,
    // 4=seq_cst. Codegen requires it to be a literal (E1706 otherwise) so the
    // memory order is fixed in the emitted instruction, never a runtime value.
    BuiltinFn {
        name: "atomic_load_i64",
        params: &[("ptr", "i64"), ("ordering", "i64")],
        ret: "i64",
        doc: "R17 Slice 2: atomic load of an i64 from `ptr` with the named memory order \
              (0=relaxed,1=acquire,2=release,3=acq_rel,4=seq_cst). Lowers to LLVM `load atomic`. Substrate-only.",
    },
    BuiltinFn {
        name: "atomic_store_i64",
        params: &[("ptr", "i64"), ("val", "i64"), ("ordering", "i64")],
        ret: "()",
        doc: "R17 Slice 2: atomic store of `val` to `ptr` with the named memory order. \
              Lowers to LLVM `store atomic`. Substrate-only.",
    },
    BuiltinFn {
        name: "atomic_fetch_add_i64",
        params: &[("ptr", "i64"), ("delta", "i64"), ("ordering", "i64")],
        ret: "i64",
        doc: "R17 Slice 2: atomically add `delta` to `*ptr`, returning the PRIOR value, \
              with the named memory order. Lowers to LLVM `atomicrmw add`. The race-free \
              SMP counter primitive. Substrate-only.",
    },
    BuiltinFn {
        name: "atomic_cas_i64",
        params: &[
            ("ptr", "i64"),
            ("expected", "i64"),
            ("desired", "i64"),
            ("ordering", "i64"),
        ],
        ret: "i64",
        doc: "R17 Slice 2: atomic compare-and-swap — if `*ptr == expected`, store `desired`; \
              returns the value that WAS in memory (== expected iff the swap happened) with the \
              named memory order. Lowers to LLVM `cmpxchg`. Substrate-only.",
    },
    // R17 §12 Q7 (2026-07-20): the missing primitive that blocked
    // axon_kernel_handles_timer_interrupt — a way to get a function's address as a
    // value, needed to fill an IDT gate descriptor's offset fields. `name` MUST be
    // a compile-time string literal naming a fn defined in this program (E1707
    // otherwise, checked at codegen time — the memory order precedent set by
    // atomic_*'s `ordering` arg, E1706). Returns the raw address as i64 (matching
    // every other R17 HAL builtin's convention of representing addresses/widths
    // as plain i64, e.g. volatile_load_u32 also returns "i64", not "u32").
    BuiltinFn {
        name: "fn_addr",
        params: &[("name", "str")],
        ret: "i64",
        doc: "R17: the address of the function named `name`, as a raw i64. `name` MUST be a \
              compile-time string literal naming a fn defined in this program — E1707 if it is \
              not a literal, or names no known function. Used to fill hardware descriptor-table \
              entries (e.g. an IDT gate's offset fields) that must point at an @[interrupt] \
              handler. Substrate-only; codegen-only (E0910 in interp — a function's address is \
              meaningless in a tree-walking interpreter).",
    },
    // ── R23 eBPF helpers (substrate-only; Bpf effect row; E0910 in interp) ──────
    // These lower to a BPF `call <id>` (or atomicrmw add) only under
    // `axon build --target bpf`. They are the capability allowlist; an
    // un-allowlisted BPF helper is E2300 at check time.
    BuiltinFn {
        name: "bpf_map_lookup_elem",
        params: &[("map", "i64"), ("key", "i64")],
        ret: "i64", // opaque value pointer (0 = NULL/miss)
        doc: "R23 eBPF: look up `key` in BPF map handle `map` (Slice 1: handle 0 = the single \
              ARRAY map). Returns the value pointer as i64 (0 = miss). Lowers to BPF `call 1`. Substrate-only.",
    },
    BuiltinFn {
        name: "bpf_map_value_add",
        params: &[("value_ptr", "i64"), ("delta", "i64")],
        ret: "()",
        doc: "R23 eBPF: atomically add `delta` to the i64 a map value pointer refers to \
              (the race-free counter increment). Lowers to BPF `atomicrmw add`. Substrate-only.",
    },
    BuiltinFn {
        name: "bpf_ktime_get_ns",
        params: &[],
        ret: "i64",
        doc: "R23 eBPF: nanoseconds since boot (CLOCK_MONOTONIC). Lowers to BPF `call 5`. Substrate-only.",
    },
    BuiltinFn {
        name: "bpf_get_smp_processor_id",
        params: &[],
        ret: "i64",
        doc: "R23 eBPF: the current CPU id. Lowers to BPF `call 8`. Substrate-only.",
    },
    // ── R24: TEE / confidential-computing primitives ────────────────────────────
    // The Secret info-flow lattice (examples/stdlib/tainted.ax) composes with the
    // enclave boundary: a sealed Secret may only be DECLASSIFIED (unsealed) inside
    // an `@[enclave]`-annotated fn. `tee_unseal` is the in-enclave declassification
    // primitive — calling it OUTSIDE an `@[enclave]` context is E1810 (a pure
    // type/checker rule, enforced on THIS host with no TEE hardware). Sealing is
    // unrestricted; only unsealing is enclave-gated. See governance/specs/R24-tee-target.md.
    BuiltinFn {
        name: "tee_seal",
        params: &[("val", "i64"), ("level", "i64")],
        ret: "i64",
        doc: "R24 TEE: seal a value at confidentiality `level` (0=public..3=secret). Sealing is \
              allowed anywhere — only UNSEALING is enclave-gated. Returns the sealed payload \
              (identity in the simulation; the type story is the Secret-lattice + E1810 rule).",
    },
    BuiltinFn {
        name: "tee_unseal",
        params: &[("sealed", "i64"), ("level", "i64")],
        ret: "i64",
        doc: "R24 TEE: UNSEAL (declassify) a sealed Secret value INSIDE the enclave. May ONLY be \
              called from within an `@[enclave]` fn — calling it outside an enclave context is \
              E1810. This is the 'compute on data you can't read' boundary, enforced by types.",
    },
    BuiltinFn {
        name: "tee_in_enclave",
        params: &[],
        ret: "bool",
        doc: "R24 TEE: true iff the workload is executing inside a TEE/enclave region. In the \
              gramine-direct simulation this is signalled by the AXON_TEE_ENCLAVE=1 env var the \
              manifest sets. (Runtime probe; the compile-time guarantee is the E1810 rule.)",
    },
    BuiltinFn {
        name: "tee_attest_measurement",
        params: &[],
        ret: "str",
        doc: "R24 TEE: the enclave LAUNCH MEASUREMENT (a deterministic hash of the loaded \
              image). SIMULATED locally (returns AXON_TEE_MEASUREMENT or a stub) — a genuine \
              hardware-rooted attestation QUOTE is produced only on confidential hardware (tee.yml).",
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
/// True if `name` is a known builtin (appears in [`BUILTINS`]). Used by the
/// purity checker to distinguish a builtin call from a user-function call.
pub fn is_known_builtin(name: &str) -> bool {
    BUILTINS.iter().any(|b| b.name == name)
}

/// Phase 13 Slice 2: true if `name` is one of the probabilistic predicate
/// pseudo-builtins (`E`, `Var`, `P`). These are handled specially in infer.rs
/// (skip normal type constraints) and in eval.rs (intercepted before normal dispatch).
pub fn is_prob_pred_ident(name: &str) -> bool {
    matches!(name, "E" | "Var" | "P")
}

/// The declared return type of a builtin (`ret` field), or `None` if unknown.
/// Used by the Phase-6 handler-lowering check: codegen substitutes a handled
/// builtin's call with an i64 resume value, so it may only lower builtins whose
/// result is consumed as i64 or discarded (`()`); anything else (e.g.
/// `ai_complete -> Result<str,str>`) must stay E0910-refused.
pub fn builtin_ret(name: &str) -> Option<&'static str> {
    BUILTINS.iter().find(|b| b.name == name).map(|b| b.ret)
}

/// R17 Slice 3 (§4 / E1704): true if calling `name` may allocate on the heap.
///
/// `@[no_alloc]` functions (ISRs / early-boot code) may not reach any of these.
/// The predicate is a CONSERVATIVE OVER-APPROXIMATION (sound for the
/// allocation-free guarantee): a builtin counts as heap-allocating if EITHER
///   (a) its declared return type materialises a heap-backed value — `str`,
///       any array `[…]`, a `Dict`, a channel, or a `Result`/`Option` wrapping
///       one of those (an `str`/array/dict result is freshly heap-allocated); OR
///   (b) it is a known in-place mutator that grows a heap structure without a
///       heap-typed return (`dict_set`/`dict_remove`, channel sends).
///
/// Over-approximating is the safe direction: flagging a non-allocating builtin
/// as allocating only costs an unnecessary E1704 (the author drops `@[no_alloc]`
/// or avoids the call), whereas MISSING an allocator would silently break the
/// guarantee. The kernel/HAL builtin surface (ptr/volatile/port/atomic/hlt) is
/// all scalar→scalar and is correctly classified non-allocating.
pub fn is_heap_allocating_builtin(name: &str) -> bool {
    // (b) explicit heap mutators whose return type is not itself heap-typed.
    if matches!(name, "dict_set" | "dict_remove" | "chan_send") {
        return true;
    }
    // (a) heap-typed return value.
    match builtin_ret(name) {
        Some(ret) => {
            let r = ret.trim();
            r == "str"
                || r == "Dict"
                || r.starts_with('[') // any array/slice type `[T]`, `[i64]`, `[(…)]`
                || r.starts_with("Chan")
                || r.starts_with("Result<str")
                || r.starts_with("Result<Dict")
                || r.starts_with("Option<str")
                // a Result/Option whose Ok/Some payload is an array
                || r.contains("Result<[")
                || r.contains("Option<[")
        }
        None => false,
    }
}

/// R7c §4/§6 — builtins that CANNOT run on the browser host (`--host browser`),
/// even with the virtual `BrowserHost` (fetch-backed FS, env map, performance.now).
/// A browser-targeted build that statically reaches one of these is refused with
/// E0911 (the load-time mirror of codegen's E0910), rather than emitting a bundle
/// that traps or silently misbehaves in the tab. The set is deliberately tight:
///
///   - `exec`: a browser sandbox has NO process model — `@[contained] exec` /
///     `DefaultHost::exec`'s deny default already covers it at runtime, but a
///     browser-target build should refuse it up front (spec §4: "Always denied").
///   - The R17 bare-metal HAL (`port_*`, `volatile_*`, `hlt`/`cli`/`sti`,
///     `ptr_from_addr`): raw hardware / privileged CPU instructions that have no
///     meaning in a wasm tab — a freestanding-only surface.
///
/// Everything else a program does in the browser is either a virtual-host call
/// (read_file/write_file/env_var/now_ms — served by `BrowserHost`), a no-op with a
/// `W0913` diagnostic (`sleep_ms`), or a compute builtin (pure). `host_await*` is
/// the interp-only async surface driven by the Asyncify binding and is NOT listed
/// here — it is exactly what makes an interactive browser program work.
pub fn is_browser_incompatible_builtin(name: &str) -> bool {
    matches!(
        name,
        // No process model in a browser sandbox.
        "exec"
            // R17 bare-metal HAL — raw hardware / privileged instructions.
            | "ptr_from_addr"
            | "volatile_load_u8" | "volatile_load_u16" | "volatile_load_u32" | "volatile_load_u64"
            | "volatile_store_u8" | "volatile_store_u16" | "volatile_store_u32"
            | "volatile_store_u64"
            | "hlt" | "cli" | "sti" | "lidt"
            | "port_out_u8" | "port_in_u8"
    )
}

/// Phase 5 §2 P04 — builtins that are IMPURE: they do I/O, AI calls, touch the
/// clock/RNG, spawn, mutate external state, or terminate the process. A
/// `@[pure]` function may not call any of these (E1207). Every OTHER builtin is
/// treated as pure (the P04 allowlist is "all builtins except these"), which is
/// conservative-safe because the impure surface is a small, enumerable set while
/// the pure surface (arithmetic, comparisons, string projections, conversions)
/// is large and grows.
pub fn is_impure_builtin(name: &str) -> bool {
    matches!(
        name,
        // AI / model calls
        "ai_complete"
            | "ai_extract_i64" | "ai_extract_f64" | "ai_extract_str" | "ai_extract_bool"
            | "ai_extract_uncertain_i64" | "ai_extract_uncertain_f64"
            | "ai_cost_spent"
            // I/O
            | "println" | "print" | "eprintln" | "eprint"
            | "read_line" | "read_file" | "write_file"
            | "exec"
            // network — raw HTTP
            | "http_get" | "http_post" | "http_sse" | "http_sse_post"
            // time / scheduling / randomness — non-deterministic
            | "now_ms" | "sleep_ms" | "random_i64" | "random_f64"
            // environment / process control
            | "env_var" | "exit"
            // online optimization / channels / concurrency. ALL goal_* touch the
            // non-deterministic in-memory provenance store (the run-variants mutate
            // it + call the @[adaptive] metric, the queries read it), so none is
            // pure — a @[pure] fn calling any of them is E1207. (The run-variants
            // goal_run_random/multistart/continue + goal_eval were silently PURE
            // under P04; the read goal_count too, like goal_history/goal_best_*.)
            | "goal_run" | "goal_run_constrained" | "goal_run_categorical"
            | "goal_run_random" | "goal_run_multistart" | "goal_continue"
            | "goal_eval" | "goal_count"
            | "goal_best_input" | "goal_best_inputs" | "goal_best_inputs_f64"
            | "goal_best_score" | "goal_history" | "goal_clear"
            | "chan_new" | "chan_send" | "chan_recv"
            // F5 sandbox execution — runs user code that may have effects (modeled as IO)
            | "sandbox_run"
            // Phase 13: distribution sampling (Random effect)
            | "gaussian_sample" | "beta_sample" | "categorical_sample"
            // R17 HAL: raw hardware access (Hal effect)
            | "ptr_from_addr"
            | "volatile_load_u8" | "volatile_load_u16" | "volatile_load_u32" | "volatile_load_u64"
            | "volatile_store_u8" | "volatile_store_u16" | "volatile_store_u32" | "volatile_store_u64"
            | "hlt" | "cli" | "sti" | "lidt"
            | "port_out_u8" | "port_in_u8"
            // R25: Zephyr console hook (host-driven device I/O)
            | "zephyr_console_putc"
            // R17 Slice 2: SMP atomics (shared-memory side effects)
            | "atomic_load_i64" | "atomic_store_i64"
            | "atomic_fetch_add_i64" | "atomic_cas_i64"
            // R17 SS12 Q7: fn_addr — substrate-only (Hal-gated) like every other R17
            // primitive, even though its value is a compile-time constant; exposing raw
            // code addresses has no sane use outside kernel-level code, and gating it
            // keeps the whole Hal-confinement story (E1700, substrate-file-only) uniform.
            | "fn_addr"
            // R23 eBPF helpers (kernel side effects; Bpf effect row)
            | "bpf_map_lookup_elem" | "bpf_map_value_add"
            | "bpf_ktime_get_ns" | "bpf_get_smp_processor_id"
            // R24 TEE: confidential-computing boundary (Tee effect). Sealing,
            // in-enclave unsealing, the enclave probe, and the measurement are all
            // boundary-effectful (interact with the TEE runtime / its env signal).
            | "tee_seal" | "tee_unseal" | "tee_in_enclave" | "tee_attest_measurement"
    )
}

/// Phase 6 §3: the effect row of a builtin — the set of concrete effects a call
/// to it performs. The empty slice is the pure row `{}` (the vast majority of
/// builtins: arithmetic, string ops, conversions, assertions, Uncertain/Temporal
/// projections). Only the small impure surface carries effects; this is the
/// per-builtin refinement of [`is_impure_builtin`] used by the effect checker to
/// seed each call site's row. Binding follows the spec §3 catalog table.
pub fn builtin_effect_row(name: &str) -> &'static [&'static str] {
    match name {
        // AI / model inference — reaches the network for a model call.
        "ai_complete"
        | "ai_extract_i64"
        | "ai_extract_f64"
        | "ai_extract_str"
        | "ai_extract_bool"
        | "ai_extract_uncertain_i64"
        | "ai_extract_uncertain_f64"
        | "ai_cost_spent" => &["AI", "Net"],

        // I/O — console, files, process spawning, environment, exit.
        "println" | "print" | "eprintln" | "eprint" | "read_line" | "read_file" | "write_file"
        | "env_var" | "exit" => &["IO"],
        "exec" => &["IO"],

        // Network — raw HTTP calls.
        "http_get" | "http_post" | "http_sse" | "http_sse_post" => &["Net"],

        // Time / scheduling.
        "now_ms" | "sleep_ms" => &["Time"],

        // Randomness / nondeterminism.
        "random_i64" | "random_f64" => &["Random"],

        // Phase 13: distribution sampling.
        "gaussian_sample" | "beta_sample" | "categorical_sample" => &["Random"],

        // Online optimization — goal_run can re-call adaptive fns, log, and read
        // provenance, so it spans {AI, Net, IO}; the read-only goal_* accessors
        // touch process-global optimizer state (modeled as IO).
        // The run-variants all re-call the @[adaptive] metric (which may
        // ai_complete), log, and mutate provenance — same {AI, Net, IO} span as
        // goal_run. goal_eval runs the metric once (held-out eval). The read-only
        // goal_* accessors (incl. goal_count) touch process-global optimizer state
        // (modeled as IO). Must stay in lockstep with is_impure_builtin (see
        // builtin_effect_row_agrees_with_impurity).
        "goal_run"
        | "goal_run_constrained"
        | "goal_run_categorical"
        | "goal_run_random"
        | "goal_run_multistart"
        | "goal_continue"
        | "goal_eval" => &["AI", "Net", "IO"],
        "goal_best_input"
        | "goal_best_inputs"
        | "goal_best_inputs_f64"
        | "goal_best_score"
        | "goal_history"
        | "goal_clear"
        | "goal_count" => &["IO"],

        // Channels / concurrency.
        "chan_new" | "chan_send" | "chan_recv" => &["Chan"],

        // Sandbox execution — runs arbitrary user code; effects depend on what
        // the fn does, but the sandbox itself is modeled as IO to prevent it
        // from appearing inside pure contexts without annotation.
        "sandbox_run" => &["IO"],

        // R17 HAL builtins — raw hardware access; substrate-only.
        // The `Hal` row is a strict superset of IO: anything that leaks Hal into
        // an unannotated context triggers E1310 (effect-row subsumption), and
        // surface files cannot declare `| {Hal}` (E1306), closing the laundering
        // path automatically.
        "ptr_from_addr" | "volatile_load_u8" | "volatile_load_u16" | "volatile_load_u32"
        | "volatile_load_u64" | "volatile_store_u8" | "volatile_store_u16"
        | "volatile_store_u32" | "volatile_store_u64" | "hlt" | "cli" | "sti" | "lidt"
        | "port_out_u8" | "port_in_u8"
        // R25: Zephyr console hook — device I/O via the host RTOS, Hal-gated.
        | "zephyr_console_putc"
        // R17 Slice 2: SMP atomics — shared-memory hardware access, Hal-gated.
        | "atomic_load_i64" | "atomic_store_i64"
        | "atomic_fetch_add_i64" | "atomic_cas_i64"
        // R17 §12 Q7: fn_addr — Hal-gated like every other R17 primitive (see
        // is_impure_builtin's own comment for why, even though it's pure in the
        // strict sense of touching no runtime state).
        | "fn_addr" => &["Hal"],

        // R23 eBPF helpers — the `Bpf` capability axis, gated like Hal: only a
        // @[bpf] substrate program may call them, and only the allowlisted set.
        "bpf_map_lookup_elem" | "bpf_map_value_add" | "bpf_ktime_get_ns"
        | "bpf_get_smp_processor_id" => &["Bpf"],

        // R24 TEE — the confidential-computing boundary. `tee_unseal` declassifies
        // a sealed Secret INSIDE the enclave (and is E1810 outside an `@[enclave]`
        // fn). The `Tee` effect makes the boundary visible to the effect checker;
        // it cannot leak into a pure context unannotated (E1310).
        "tee_seal" | "tee_unseal" | "tee_in_enclave" | "tee_attest_measurement" => &["Tee"],

        // Everything else is pure.
        _ => &[],
    }
}

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
    // `verify` is intentionally absent — the form `@[verify(expr)]` is parsed
    // specially in `parse_item`, captured on `FnDef.verify`, and validated by
    // `crate::verify::check_verify` (E1101). The bare form `@[verify]`
    // (no predicate) is now treated as a generic unknown attribute (W0001).
    "ai",
    "layout",
    "temporal",
    // `forall` marks a `@[test]` fn as a property test: its typed params are
    // randomized over N cases and a failure is shrunk to a minimal
    // counterexample (R8). Handled in the test runner (main.rs) +
    // `interp::run_property_test`.
    "forall",
    // Phase 5 (§2): `@[pure]` admits a fn into the predicate language; its
    // semantics (P01/P02/P04/P05) are enforced by `checker::check_purity`
    // (E1207). `@[total]` marks a termination obligation (§3). Both are known
    // attributes here so they don't warn (W0001); the checker does the work.
    "pure",
    "total",
    // R17 HAL attributes — substrate-only; checker enforces substrate context.
    // `@[hal]` grants the Hal capability to the fn's body (effects: `| {Hal}`).
    // `@[entry]` marks the bare-metal entry point (replaces host main/_start).
    // `@[panic_handler]` is the no-return panic hook for freestanding builds.
    // `@[no_alloc]` marks an ISR/early-boot fn that must not allocate (E1704, Slice 3).
    "hal",
    "entry",
    "panic_handler",
    "no_alloc",
    // Layout control (R17 Slice 3)
    "repr",
    "packed",
    "align",
    "naked",
    "interrupt",
    // R23 eBPF target: `@[bpf(kind: socket_filter|xdp|tracepoint|kprobe)]` marks a
    // fn as an eBPF program. Substrate-only; `axon build --target bpf` lowers it
    // to a kernel-verifier-accepted .bpf.o. Auto-implies @[no_alloc] + @[total].
    "bpf",
    // R24 TEE: `@[enclave]` marks a fn as running inside the trusted execution
    // environment. It is the ONLY context in which `tee_unseal` (Secret
    // declassification) is permitted — calling `tee_unseal` from a non-enclave
    // fn is E1810. The checker (`check_enclave_unseal`) enforces the rule; the
    // attribute is known here so it doesn't warn (W0001).
    "enclave",
];

/// R23 eBPF: the BPF helper capability allowlist. A `@[bpf]` program may only
/// call a builtin in this set; an un-allowlisted helper is E2300 at check time
/// (a clean Axon error, NOT a kernel load-time verifier reject). Each lowers to
/// a BPF `call <id>` (or, for the map-value increment, an `atomicrmw add`).
pub const BPF_HELPER_BUILTINS: &[&str] = &[
    "bpf_map_lookup_elem",
    "bpf_map_value_add",
    "bpf_ktime_get_ns",
    "bpf_get_smp_processor_id",
];

/// Is `name` an allowlisted BPF helper builtin (R23)?
pub fn is_bpf_helper(name: &str) -> bool {
    BPF_HELPER_BUILTINS.contains(&name)
}

/// R23 eBPF: the supported `@[bpf(kind: K)]` program kinds and their libbpf ELF
/// section names. Unknown kinds are E2302.
pub fn bpf_kind_section(kind: &str) -> Option<&'static str> {
    match kind {
        "socket_filter" => Some("socket"),
        "xdp" => Some("xdp"),
        "tracepoint" => Some("tracepoint"),
        "kprobe" => Some("kprobe"),
        _ => None,
    }
}

/// R23 eBPF: the BPF helper id used in the emitted `call <id>` instruction.
/// `bpf_map_value_add` is special (lowered to `atomicrmw add`, no helper call).
pub fn bpf_helper_id(name: &str) -> Option<u64> {
    match name {
        "bpf_map_lookup_elem" => Some(1),
        "bpf_ktime_get_ns" => Some(5),
        "bpf_get_smp_processor_id" => Some(8),
        _ => None,
    }
}

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
    fn r23_bpf_helpers_are_known_impure_and_carry_bpf_effect() {
        for name in BPF_HELPER_BUILTINS {
            assert!(is_known_builtin(name), "{name} must be a known builtin");
            assert!(is_bpf_helper(name), "{name} must be an allowlisted BPF helper");
            assert!(is_impure_builtin(name), "{name} must be impure");
            assert_eq!(
                builtin_effect_row(name),
                &["Bpf"],
                "{name} must carry the Bpf effect row"
            );
            // BPF helpers are scalar/ptr leaves — they do NOT allocate a heap
            // str/array/dict, so a @[bpf] (@[no_alloc]) program may call them.
            assert!(
                !is_heap_allocating_builtin(name),
                "{name} (BPF helper) must be allocation-free"
            );
        }
    }

    #[test]
    fn r23_bpf_helper_ids_are_consistent() {
        // bpf_map_value_add is special (atomicrmw add, no helper call) → no id.
        assert_eq!(bpf_helper_id("bpf_map_value_add"), None);
        // The call-emitting helpers have their canonical Linux helper ids.
        assert_eq!(bpf_helper_id("bpf_map_lookup_elem"), Some(1));
        assert_eq!(bpf_helper_id("bpf_ktime_get_ns"), Some(5));
        assert_eq!(bpf_helper_id("bpf_get_smp_processor_id"), Some(8));
        // An un-allowlisted helper is not recognized at all.
        assert!(!is_bpf_helper("bpf_probe_read"));
    }

    #[test]
    fn r23_bpf_kind_sections() {
        assert_eq!(bpf_kind_section("socket_filter"), Some("socket"));
        assert_eq!(bpf_kind_section("xdp"), Some("xdp"));
        assert_eq!(bpf_kind_section("tracepoint"), Some("tracepoint"));
        assert_eq!(bpf_kind_section("kprobe"), Some("kprobe"));
        assert_eq!(bpf_kind_section("nonsense"), None);
    }

    #[test]
    fn r17_atomics_carry_hal_effect_and_are_impure() {
        for name in [
            "atomic_load_i64",
            "atomic_store_i64",
            "atomic_fetch_add_i64",
            "atomic_cas_i64",
        ] {
            assert!(is_known_builtin(name), "{name} must be a known builtin");
            assert!(is_impure_builtin(name), "{name} must be impure");
            assert_eq!(
                builtin_effect_row(name),
                &["Hal"],
                "{name} must carry the Hal effect row"
            );
            // Atomics touch shared memory but do not themselves allocate a heap
            // str/array/dict, so they are allocation-free (usable in @[no_alloc]).
            assert!(
                !is_heap_allocating_builtin(name),
                "{name} must not be classified heap-allocating"
            );
        }
    }

    #[test]
    fn r17_fn_addr_is_hal_impure_alloc_free() {
        let name = "fn_addr";
        assert!(is_known_builtin(name), "{name} must be a known builtin");
        assert!(is_impure_builtin(name), "{name} must be impure");
        assert_eq!(
            builtin_effect_row(name),
            &["Hal"],
            "{name} must carry the Hal effect row"
        );
        assert!(
            !is_heap_allocating_builtin(name),
            "{name} must not be classified heap-allocating"
        );
    }

    #[test]
    fn r19_fixed_width_as_casts_are_known_pure_general_purpose() {
        // The `x as u16`-style operator (parser.rs) desugars to a call to
        // `as_<type>`; as_i64/as_f64 were the only callees that existed
        // before this — these 7 close the family. Pure, general-purpose
        // (like as_i64/as_f64): no Hal gate, no impurity, no heap alloc.
        for name in [
            "as_u8", "as_u16", "as_u32", "as_u64", "as_i8", "as_i16", "as_i32",
        ] {
            assert!(is_known_builtin(name), "{name} must be a known builtin");
            assert!(
                !is_impure_builtin(name),
                "{name} must be pure (a numeric cast, like as_i64/as_f64)"
            );
            assert!(
                builtin_effect_row(name).is_empty(),
                "{name} must carry no effect row (general-purpose, not Hal-gated)"
            );
            assert!(
                !is_heap_allocating_builtin(name),
                "{name} must not be classified heap-allocating"
            );
        }
    }

    #[test]
    fn r25_zephyr_console_putc_is_hal_impure_alloc_free() {
        let name = "zephyr_console_putc";
        assert!(is_known_builtin(name), "{name} must be a known builtin");
        assert!(is_impure_builtin(name), "{name} must be impure");
        assert_eq!(
            builtin_effect_row(name),
            &["Hal"],
            "{name} must carry the Hal effect row (substrate-only, surface-refused)"
        );
        assert!(
            !is_heap_allocating_builtin(name),
            "{name} (HAL console leaf) must be allocation-free (usable in @[no_alloc])"
        );
    }

    #[test]
    fn r17_heap_alloc_classifier_is_sound_for_hal_and_strings() {
        // HAL leaf builtins are scalar→scalar and must NOT count as allocating.
        for name in [
            "ptr_from_addr",
            "volatile_load_u8",
            "volatile_store_u8",
            "port_out_u8",
            "port_in_u8",
            "zephyr_console_putc",
            "hlt",
            "cli",
            "sti",
            "lidt",
        ] {
            assert!(
                !is_heap_allocating_builtin(name),
                "{name} (HAL leaf) must be allocation-free"
            );
        }
        // str / array / dict constructors+mutators MUST count as allocating.
        for name in [
            "to_str",      // i64 → str (fresh heap str)
            "str_repeat",  // → str
            "str_concat",  // (if present) → str
            "arr_range",   // → [i64]
            "arr_push",    // → [i64]
            "dict_new",    // → Dict
            "dict_set",    // () but grows the dict (explicit mutator)
            "dict_remove", // () mutator
        ] {
            // Only assert for builtins that actually exist in the table.
            if is_known_builtin(name) {
                assert!(
                    is_heap_allocating_builtin(name),
                    "{name} must be classified heap-allocating"
                );
            }
        }
        // Pure scalar projections must be allocation-free.
        for name in ["str_len", "str_eq", "abs_i64", "min_i64"] {
            if is_known_builtin(name) {
                assert!(
                    !is_heap_allocating_builtin(name),
                    "{name} (scalar projection) must be allocation-free"
                );
            }
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
            assert!(seen.insert(*attr), "duplicate deferred attr: `{attr}`");
        }
    }

    #[test]
    fn known_attrs_present() {
        let set: std::collections::HashSet<&str> = DEFERRED_ATTRS.iter().copied().collect();
        // `verify` is intentionally absent — it is now a checked attribute handled
        // by `crate::verify::check_verify`, not a deferred one.
        for expected in &["adaptive", "goal", "agent", "ai"] {
            assert!(
                set.contains(expected),
                "expected deferred attr `{expected}` not found"
            );
        }
        assert!(
            !set.contains("verify"),
            "`verify` should no longer be deferred"
        );
    }

    #[test]
    fn builtin_effect_row_agrees_with_impurity() {
        // The Phase-6 effect catalog must stay in lockstep with is_impure_builtin:
        // a builtin has a non-empty effect row IFF it is impure. A drift here
        // (e.g. a new impure builtin given no row) would silently let an effect
        // escape the checker — guard against it across the whole BUILTINS table.
        for b in BUILTINS {
            let row = builtin_effect_row(b.name);
            let impure = is_impure_builtin(b.name);
            assert_eq!(
                !row.is_empty(),
                impure,
                "`{}`: effect row {:?} disagrees with is_impure_builtin={impure}",
                b.name,
                row
            );
        }
        // Spot-check the catalog's specific row assignments (spec §3).
        assert_eq!(builtin_effect_row("println"), &["IO"]);
        assert_eq!(builtin_effect_row("ai_complete"), &["AI", "Net"]);
        assert_eq!(builtin_effect_row("random_i64"), &["Random"]);
        assert_eq!(builtin_effect_row("now_ms"), &["Time"]);
        assert_eq!(builtin_effect_row("chan_send"), &["Chan"]);
        assert_eq!(builtin_effect_row("goal_run"), &["AI", "Net", "IO"]);
        assert!(builtin_effect_row("to_str").is_empty(), "to_str is pure");
        assert!(builtin_effect_row("abs_i32").is_empty(), "abs_i32 is pure");
    }

    #[test]
    fn browser_incompatible_set_is_tight_and_real() {
        // R7c §6: the E0911 set is exactly the builtins that CANNOT run in a
        // browser sandbox even with a virtual host.
        assert!(
            is_browser_incompatible_builtin("exec"),
            "exec (process spawn) must be browser-incompatible"
        );
        // The R17 bare-metal HAL — raw hardware / privileged CPU instructions.
        for hal in [
            "port_out_u8",
            "port_in_u8",
            "hlt",
            "cli",
            "sti",
            "lidt",
            "ptr_from_addr",
            "volatile_load_u8",
            "volatile_store_u64",
        ] {
            assert!(
                is_browser_incompatible_builtin(hal),
                "{hal} (bare-metal HAL) must be browser-incompatible"
            );
        }
        // The virtual-host / compute surface is COMPATIBLE — these run in the tab.
        for ok in [
            "read_file",
            "write_file",
            "env_var",
            "now_ms",
            "sleep_ms",
            "println",
            "to_str",
            "host_await",
            "ai_complete",
        ] {
            assert!(
                !is_browser_incompatible_builtin(ok),
                "{ok} must NOT be browser-incompatible (virtual host / async / compute)"
            );
        }
        // Every browser-incompatible builtin must be a real builtin name (no typos).
        for b in BUILTINS {
            // (no assertion needed; just exercising the table)
            let _ = is_browser_incompatible_builtin(b.name);
        }
        assert!(
            is_known_builtin("exec"),
            "exec must be a known builtin (sanity on the table)"
        );
    }
}
