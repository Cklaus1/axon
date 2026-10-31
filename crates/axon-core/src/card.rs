//! `axon card` - the language card a model reads before writing Axon (IG2).
//!
//! Three parts: a curated syntax section (condensed from
//! `spec/axon-for-llms.md`), the program rules a generated program must follow,
//! and every builtin's signature from `BUILTINS`, rendered by the same
//! [`BuiltinFn::signature`] `axon reference` prints. The core groups add each
//! builtin's first doc sentence. `BUILTINS` has no module field, so the groups
//! are the name lists in [`GROUPS`]; a test holds them to cover every builtin
//! exactly once, and another type-checks every fenced example below.
//!
//! The card is a pure function of this binary's tables: no clock, no
//! environment, no version string, so its sha256 changes only when its text
//! does.

use crate::builtins::{BuiltinFn, BUILTINS};

/// Sections 1 and 2: syntax and program rules. Every ```` ```axon ```` block
/// is a whole program that `axon check` accepts (tested).
const CURATED: &str = r#"# Axon language card

Axon is statically typed, with no null and no exceptions. A program is ONE `.ax` file whose entry point is
`fn main() -> i64`. Every builtin is in global scope: there are no imports. Write plain Axon only - no
Markdown around it, no explanation.

## 1. Syntax

```axon
// Items (`fn`, `type`) live at the top level; statements live inside a fn body.
// No semicolons: one statement per line.
type Point = { x: i64, y: i64 }
type Shape = Circle { r: f64 } | Square { side: f64 }

// The last expression of a block is its value; `return` is only for an early exit.
fn add(a: i64, b: i64) -> i64 { a + b }

fn area(s: Shape) -> f64 {
    // `match` is an expression and must be exhaustive (or end with `_ =>`).
    match s {
        Shape::Circle { r } => 3.14159 * r * r
        Shape::Square { side } => side * side
    }
}

fn main() -> i64 {
    let x = 1                     // variables are mutable by default
    x = x + 1                     // reassign WITHOUT `let`
    let label = if x > 1 { "big" } else { "small" }
    let total = 0
    for i in 0..3 { total = total + i }        // i = 0, 1, 2
    while total < 10 { total = total + 1 }
    let p = Point { x: 1, y: 2 }
    // `{expr}` inside a string literal is interpolated; `{{` and `}}` are literal braces.
    println("{label} {to_str(p.x + p.y)} {to_str(add(total, 1))}")
    println(to_str_f64(area(Shape::Circle { r: 1.0 })))
    0
}
```

`Option` and `Result` replace null and exceptions. `?` returns an `Err` early, only inside a fn that returns
`Result`:

```axon
fn find(xs: &[i64], target: i64) -> Option<i64> {
    for i in 0..len(xs) {
        if xs[i] == target { return Some(i) }
    }
    None
}

fn doubled(s: str) -> Result<i64, str> {
    let n = parse_int(s)?
    Ok(n * 2)
}

fn main() -> i64 {
    let xs = [3, 5, 7]
    match find(&xs, 5) {
        Some(i) => println("found at {to_str(i)}")
        None => println("absent")
    }
    match doubled("21") {
        Ok(n) => {
            println(to_str(n))
            0
        }
        Err(e) => {
            eprintln("bad number: {e}")
            1
        }
    }
}
```

Arrays are values. Pass one to a fn as a read-only borrow `&[T]` (call with `&a`); to change the caller's array
in place declare `&mut [T]` and call with `&mut a`. A `str` or struct is MOVED by `let t = s`: using `s` again is
an error, so borrow with `ref t = s` instead.

```axon
fn bump(xs: &mut [i64]) {
    for i in 0..len(xs) { xs[i] = xs[i] + 1 }
}

fn sum(xs: &[i64]) -> i64 {
    let s = 0
    for i in 0..len(xs) { s = s + xs[i] }
    s
}

fn main() -> i64 {
    let a = [1, 2, 3]
    bump(&mut a)
    let parts = str_split("x,y,z", ",")
    let joined = str_join(parts, "+")
    println("{to_str(sum(&a))} {joined}")
    0
}
```

Rules the checker enforces:
- Keep an expression on one line, or END the line with the operator: a line that STARTS with `+`, `&&`, ... does
  not continue the previous one.
- `i64` and `f64` never mix silently: convert with `i64_to_f64` / `f64_to_i64` (or `x as f64`). `/` and `%` on
  `i64` are integer division and remainder. `+` concatenates two `str`; build text with `to_str(..)` or
  interpolation.
- Strings are immutable UTF-8. `str_slice(s, a, b)` takes BYTE indices; use `str_char_slice` for characters.
- `len(x)`: the table below lists `len(s: str) -> i64`, but the checker also accepts an array (`[T]` or `&[T]`):
  on a `str` it is the BYTE count, on an array the element count.
- `mut` is not a keyword (it is ignored); `&mut` is.
- A generic fn or a builtin is not a value: wrap it in a lambda, `|n| abs_i64(n)`.

## 2. Program rules

- `main` returns the process exit code: `0` for success, non-zero for failure. Never call `exit()`.
- File builtins return `Result`: match every one. On `Err`, report with `eprintln` and return a non-zero code.
- Write each path with EXACTLY the prefix spelling the task gives: allowed `./out/` means `"./out/result.txt"`.
- The capability ceiling is imposed from outside. Declare no `@[contained]` or other capability attribute, and
  touch only what the task allows: no network, no processes, no environment variables unless it says so.

```axon
fn main() -> i64 {
    match read_file("./data/input.txt") {
        Ok(text) => {
            let lines = str_split(text, "\n")
            let out = ""
            for i in 0..len(lines) {
                out = out + str_to_upper(lines[i]) + "\n"
            }
            match write_file("./out/result.txt", out) {
                Ok(_) => 0
                Err(e) => {
                    eprintln("cannot write: {e}")
                    1
                }
            }
        }
        Err(e) => {
            eprintln("cannot read: {e}")
            2
        }
    }
}
```
"#;

/// The builtin groups: (heading, core, names). A core group prints each
/// builtin's first doc sentence after its signature; the rest print the
/// signature only. Every `BUILTINS` entry is in exactly one group (tested).
pub const GROUPS: &[(&str, bool, &[&str])] = &[
    (
        "file",
        true,
        &[
            "append_file",
            "dir_create",
            "dir_list",
            "file_copy",
            "file_exists",
            "file_rename",
            "file_size",
            "read_file",
            "read_line",
            "write_file",
        ],
    ),
    (
        "string",
        true,
        &[
            "axon_concat",
            "char_at",
            "char_code",
            "char_is_alpha",
            "char_is_digit",
            "char_is_space",
            "chr",
            "format",
            "len",
            "re_captures",
            "re_find",
            "re_find_all",
            "re_is_match",
            "re_replace_all",
            "re_split",
            "str_char_at",
            "str_chars",
            "str_char_slice",
            "str_cmp",
            "str_contains",
            "str_count",
            "str_digits_only",
            "str_ends_with",
            "str_eq",
            "str_index_of",
            "str_join",
            "str_len",
            "str_len_chars",
            "str_pad_end",
            "str_pad_start",
            "str_repeat",
            "str_replace",
            "str_reverse",
            "str_slice",
            "str_split",
            "str_starts_with",
            "str_to_lower",
            "str_to_upper",
            "str_trim",
            "str_trim_end",
            "str_trim_start",
        ],
    ),
    (
        "collection",
        true,
        &[
            "arr_all",
            "arr_any",
            "arr_argmax_f64",
            "arr_argmax_i64",
            "arr_argmin_f64",
            "arr_argmin_i64",
            "arr_chunk",
            "arr_concat",
            "arr_contains",
            "arr_count_if",
            "arr_drop",
            "arr_drop_while",
            "arr_enumerate",
            "arr_filter",
            "arr_find",
            "arr_flatten",
            "arr_fold",
            "arr_group_by",
            "arr_index_of",
            "arr_map",
            "arr_max_by",
            "arr_max_f64",
            "arr_max_i64",
            "arr_mean_f64",
            "arr_mean_i64",
            "arr_min_by",
            "arr_min_f64",
            "arr_min_i64",
            "arr_partition",
            "arr_push",
            "arr_range",
            "arr_repeat",
            "arr_reverse",
            "arr_sort_by",
            "arr_std_f64",
            "arr_sum_by",
            "arr_sum_by_f64",
            "arr_sum_f64",
            "arr_sum_i64",
            "arr_take",
            "arr_take_while",
            "arr_unique",
            "arr_zip",
            "arr_zip_with",
            "dict_each",
            "dict_filter",
            "dict_from_pairs",
            "dict_from_str",
            "dict_get",
            "dict_get_or",
            "dict_has",
            "dict_inc",
            "dict_keys",
            "dict_len",
            "dict_map_values",
            "dict_merge",
            "dict_new",
            "dict_remove",
            "dict_set",
            "dict_to_json",
            "dict_to_pairs",
            "dict_to_str",
            "dict_try_from_str",
            "dict_values",
        ],
    ),
    (
        "conversion",
        true,
        &[
            "as_f64",
            "as_i16",
            "as_i32",
            "as_i64",
            "as_i8",
            "as_u16",
            "as_u32",
            "as_u64",
            "as_u8",
            "base64_decode",
            "base64_encode",
            "f64_to_i64",
            "hex_decode",
            "hex_encode",
            "i64_to_f64",
            "i64_to_str_radix",
            "parse_bool",
            "parse_bool_or",
            "parse_float",
            "parse_float_or",
            "parse_int",
            "parse_int_or",
            "parse_int_radix",
            "to_str",
            "to_str_bool",
            "to_str_f64",
        ],
    ),
    (
        "math",
        true,
        &[
            "abs_f64",
            "abs_i32",
            "abs_i64",
            "bit_and",
            "bit_not",
            "bit_or",
            "bit_xor",
            "ceil",
            "ceil_f64",
            "clamp_f64",
            "clamp_i64",
            "decimal_abs",
            "decimal_div",
            "decimal_from_str",
            "decimal_neg",
            "decimal_round",
            "decimal_to_str",
            "exp",
            "floor",
            "floor_f64",
            "ln",
            "log10",
            "max_f64",
            "max_i32",
            "max_i64",
            "min_f64",
            "min_i32",
            "min_i64",
            "pow",
            "pow_i64",
            "random_f64",
            "random_i64",
            "round_f64",
            "shl",
            "shr",
            "sign_i64",
            "sqrt",
            "sqrt_f64",
            "srand",
            "wrapping_add",
            "wrapping_div",
            "wrapping_mul",
            "wrapping_rem",
            "wrapping_sub",
        ],
    ),
    (
        "json",
        false,
        &[
            "json_arr_f64",
            "json_arr_from_f64",
            "json_arr_from_i64",
            "json_arr_from_str",
            "json_arr_i64",
            "json_arr_str",
            "json_at",
            "json_from_pairs",
            "json_get_i64",
            "json_get_json",
            "json_get_str",
            "json_keys",
            "json_len",
            "json_parse",
            "json_path_f64",
            "json_path_i64",
            "json_path_json",
            "json_path_str",
            "json_stringify",
        ],
    ),
    (
        "console, time and process",
        false,
        &[
            "cli", "env_var", "eprint", "eprintln", "exec", "exit", "now_ms", "now_ns", "print",
            "println", "sleep_ms",
        ],
    ),
    (
        "testing",
        false,
        &[
            "assert",
            "assert_eq",
            "assert_eq_f64",
            "assert_eq_str",
            "assert_err",
        ],
    ),
    (
        "network, database and AI",
        false,
        &[
            "agent_detect_loop",
            "agent_trace_len",
            "agent_uncertainty",
            "ai_complete",
            "ai_cost_spent",
            "ai_extract_uncertain_f64",
            "ai_extract_uncertain_i64",
            "http_get",
            "http_post",
            "http_sse",
            "http_sse_post",
            "llm_alive",
            "llm_complete",
            "llm_open",
            "llm_spent",
            "sql_query",
        ],
    ),
    (
        "goals",
        false,
        &[
            "goal_best_input",
            "goal_best_inputs",
            "goal_best_inputs_f64",
            "goal_best_score",
            "goal_clear",
            "goal_continue",
            "goal_count",
            "goal_eval",
            "goal_history",
            "goal_run",
            "goal_run_categorical",
            "goal_run_constrained",
            "goal_run_multistart",
            "goal_run_random",
            "kernel_goal_best_score",
            "kernel_goal_budget_left",
            "kernel_goal_create",
            "kernel_goal_run",
            "kernel_goal_spent",
        ],
    ),
    (
        "uncertainty, probability and time-validity (`E`, `Var`, `P`: refinement predicates only)",
        false,
        &[
            "E",
            "Var",
            "P",
            "beta_cdf",
            "beta_mean",
            "beta_sample",
            "beta_variance",
            "categorical_cdf",
            "categorical_mean",
            "categorical_sample",
            "categorical_variance",
            "gaussian_cdf",
            "gaussian_pdf",
            "gaussian_sample",
            "temporal_at",
            "temporal_confidence",
            "temporal_is_valid",
            "temporal_new",
            "temporal_now",
            "uncertain_confidence",
            "uncertain_deterministic",
            "uncertain_dyn_f64",
            "uncertain_dyn_i64",
            "uncertain_new",
            "uncertain_new_f64",
        ],
    ),
    (
        "concurrency, capabilities, supervision and storage",
        false,
        &[
            "Chan::new",
            "atomic_cas_i64",
            "atomic_fetch_add_i64",
            "atomic_load_i64",
            "atomic_store_i64",
            "corrigible_halt",
            "corrigible_halted",
            "dstore_apply",
            "dstore_clear",
            "dstore_open",
            "dstore_value",
            "dstore_version",
            "host_await",
            "host_await_opt",
            "host_await_val",
            "host_await_val_opt",
            "principal_activate",
            "principal_authorize",
            "principal_budget_remaining",
            "principal_can_mint",
            "principal_current_name",
            "principal_holds",
            "principal_mint",
            "principal_root",
            "principal_spend",
            "sandbox_create",
            "sandbox_create_scoped",
            "sandbox_run",
            "scheduler_done_count",
            "scheduler_failed",
            "scheduler_failed_count",
            "scheduler_restart",
            "scheduler_result",
            "scheduler_run",
            "scheduler_spawn",
            "supervisor_alive",
            "supervisor_new",
            "supervisor_restarts",
            "supervisor_run",
            "supervisor_supervise",
            "tee_attest_measurement",
            "tee_in_enclave",
            "tee_seal",
            "tee_unseal",
        ],
    ),
    (
        "systems and bare metal",
        false,
        &[
            "bpf_get_smp_processor_id",
            "bpf_ktime_get_ns",
            "bpf_map_lookup_elem",
            "bpf_map_value_add",
            "fn_addr",
            "hlt",
            "lidt",
            "port_in_u8",
            "port_out_u8",
            "ptr_from_addr",
            "sti",
            "volatile_load_u16",
            "volatile_load_u32",
            "volatile_load_u64",
            "volatile_load_u8",
            "volatile_store_u16",
            "volatile_store_u32",
            "volatile_store_u64",
            "volatile_store_u8",
            "zephyr_console_putc",
        ],
    ),
];

/// The first sentence of a doc string: up to the first `.` that ends the
/// text or is followed by whitespace, outside backticks (so `e.g.` inside
/// code and `0.5` do not cut it).
pub fn first_sentence(doc: &str) -> &str {
    let b = doc.as_bytes();
    let mut code = false;
    for (i, &c) in b.iter().enumerate() {
        match c {
            b'`' => code = !code,
            b'.' if !code && (i + 1 == b.len() || b[i + 1].is_ascii_whitespace()) => {
                return &doc[..=i];
            }
            _ => {}
        }
    }
    doc.trim_end()
}

fn find(name: &str) -> Option<&'static BuiltinFn> {
    BUILTINS.iter().find(|b| b.name == name)
}

/// The whole card. Deterministic: the same binary prints the same bytes.
pub fn card() -> String {
    let mut out = String::from(CURATED);
    out.push_str(&format!("\n## 3. Builtins ({})\n", BUILTINS.len()));
    out.push_str(
        "\nEvery builtin, by signature. `T`, `K`, `V` are type parameters. The first five groups add the first \
         sentence of each builtin's documentation.\n",
    );
    for (heading, core, names) in GROUPS {
        out.push_str(&format!("\n### {heading}\n\n"));
        for name in names.iter() {
            // A name the table lacks is a test failure, never a card line.
            let Some(b) = find(name) else { continue };
            if *core {
                let doc = first_sentence(b.doc).replace('\n', " ");
                out.push_str(&format!("- `{}` - {doc}\n", b.signature()));
            } else {
                out.push_str(&format!("- `{}`\n", b.signature()));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_groups_cover_every_builtin_exactly_once() {
        let mut seen: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for (_, _, names) in GROUPS {
            for n in names.iter() {
                *seen.entry(n).or_default() += 1;
            }
        }
        let unknown: Vec<&&str> = seen.keys().filter(|n| find(n).is_none()).collect();
        assert!(
            unknown.is_empty(),
            "ATTACK: a card group names no builtin: {unknown:?}"
        );
        let twice: Vec<(&&str, &usize)> = seen.iter().filter(|(_, c)| **c > 1).collect();
        assert!(
            twice.is_empty(),
            "ATTACK: a builtin is in two card groups: {twice:?}"
        );
        let missing: Vec<&str> = BUILTINS
            .iter()
            .map(|b| b.name)
            .filter(|n| !seen.contains_key(n))
            .collect();
        assert!(
            missing.is_empty(),
            "ATTACK: a builtin is missing from the card: {missing:?}"
        );
    }

    #[test]
    fn every_card_example_type_checks() {
        let text = card();
        let mut blocks = Vec::new();
        let mut cur: Option<String> = None;
        for line in text.lines() {
            match (&mut cur, line) {
                (None, "```axon") => cur = Some(String::new()),
                (Some(_), "```") => blocks.push(cur.take().unwrap()),
                (Some(b), l) => {
                    b.push_str(l);
                    b.push('\n');
                }
                _ => {}
            }
        }
        assert!(cur.is_none(), "an unclosed ```axon block");
        assert!(
            blocks.len() >= 4,
            "the card's examples went missing: {}",
            blocks.len()
        );
        for src in &blocks {
            let errors: Vec<String> = crate::check_pipeline(src, "card.ax")
                .into_iter()
                .filter(|d| d.severity == "error")
                .map(|d| format!("{} {}:{} {}", d.code, d.line, d.col, d.message))
                .collect();
            assert!(
                errors.is_empty(),
                "ATTACK: a card example does not type-check: {errors:?}\n{src}"
            );
        }
    }

    #[test]
    fn the_card_is_deterministic_and_signs_every_builtin() {
        let a = card();
        assert_eq!(a, card());
        for b in BUILTINS {
            assert!(
                a.contains(&format!("- `{}`", b.signature())),
                "ATTACK: the card omits the signature of `{}`",
                b.name
            );
        }
        assert!(
            a.contains("- `read_file(path: str) -> Result<str, str>` - Read the entire contents of `path` as a UTF-8 string."),
            "ATTACK: a core-group builtin lost its first doc sentence"
        );
        assert!(
            !a.contains("@[contained("),
            "the card teaches no ceiling syntax"
        );
    }

    #[test]
    fn first_sentence_stops_at_the_first_sentence_end_outside_code() {
        assert_eq!(first_sentence("Read it. Then more."), "Read it.");
        assert_eq!(
            first_sentence("Write `fn f() { 0 }. ` here. More."),
            "Write `fn f() { 0 }. ` here.",
            "ATTACK: a full stop inside backticks cut a card doc sentence"
        );
        assert_eq!(
            first_sentence("Returns 0.5 or 1.0. Done."),
            "Returns 0.5 or 1.0."
        );
        assert_eq!(first_sentence("No full stop"), "No full stop");
    }
}
