//! Anti-drift probe for `parse_help` (`AXON_FOR_RLM.md` §1 + §3).
//!
//! `parse_help`'s unit tests feed it message strings *by hand*. That proves the
//! table maps a message to the right hint; it proves nothing about whether the
//! parser still emits that message. If a parser refactor reworded
//! `unexpected token: Ident("c"), expected Eq`, every unit test would stay green
//! and every hint would silently stop firing — the exact failure the feature
//! exists to prevent, reintroduced invisibly.
//!
//! So this file goes the other way: it runs the **real pipeline** over real
//! source and asserts the hint comes out. It is the test that fails on drift.
//!
//! It also records the negative cases, which are the ones a reader is most
//! likely to re-add from the spec's list without re-probing: `;` is *accepted*
//! by Axon, and `const`/`var` never reach the parse tier at all.

use axon_core::check_pipeline;

/// Run the check pipeline and return the first diagnostic's `(code, help)`.
fn first_diag(src: &str) -> (String, Option<String>) {
    let diags = check_pipeline(src, "probe.ax");
    let d = diags.first().expect("expected at least one diagnostic");
    (d.code.clone(), d.help.clone())
}

#[test]
fn mut_still_reaches_the_parse_tier_and_still_gets_help() {
    // The measured dominant failure. If this test fails, either the parser
    // reworded its message or `let mut` started parsing — check which before
    // touching parse_help.rs.
    let (code, help) = first_diag("fn main() -> i64 {\n    let mut count = 0\n    0\n}\n");
    assert_eq!(code, "E0000", "must still be a PARSE error");
    let help = help.expect("`let mut` must carry a fix hint through the real pipeline");
    assert!(help.contains("`mut` is not an Axon keyword"), "{help}");
    assert!(
        help.contains("let count = "),
        "must echo the real name: {help}"
    );
}

#[test]
fn the_parse_diagnostic_now_carries_a_real_line_and_column() {
    // §1's sibling defect: this diagnostic reported line 0 because the pipeline
    // parsed with the unlocated `parse_source`. A hint that cannot say *where*
    // is only half a repair.
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let mut count = 0\n    0\n}\n",
        "probe.ax",
    );
    let d = diags.first().unwrap();
    assert_eq!(d.line, 2, "the bad token is on line 2, got {}", d.line);
    assert!(d.col > 0, "column must be resolved, got {}", d.col);
}

#[test]
fn help_survives_into_the_emitted_json() {
    // The whole point is that a machine consumer can read it. Assert on the
    // wire format, not just the struct field.
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let mut count = 0\n    0\n}\n",
        "probe.ax",
    );
    let json = diags.first().unwrap().json();
    assert!(json.contains("\"schema\":\"axon-diag/1\""), "{json}");
    assert!(json.contains("\"code\":\"E0000\""), "{json}");
    assert!(
        json.contains("\"help\":"),
        "help must be a first-class field: {json}"
    );
    assert!(json.contains("\"line\":2"), "{json}");
}

#[test]
fn arrow_walrus_and_def_still_reach_the_parse_tier() {
    for (label, src, needle) in [
        (
            "let-arrow",
            "fn main() -> i64 {\n    let c -> i64 = 0\n    0\n}\n",
            "let c: i64",
        ),
        (
            "walrus",
            "fn main() -> i64 {\n    let c := 0\n    0\n}\n",
            "`:=` is not Axon",
        ),
        (
            "def",
            "def main():\n    return 0\n",
            "Axon declares functions with `fn`",
        ),
        (
            "function",
            "function main() {\n    return 0\n}\n",
            "Axon declares functions with `fn`",
        ),
    ] {
        let (code, help) = first_diag(src);
        assert_eq!(code, "E0000", "{label} must still be a parse error");
        let help = help.unwrap_or_else(|| panic!("{label} must carry a fix hint"));
        assert!(help.contains(needle), "{label}: {help}");
    }
}

#[test]
fn a_semicolon_is_accepted_so_it_gets_no_parse_help() {
    // `AXON_FOR_RLM.md` §1 lists `;` as a habit to target. Probing the compiler
    // shows Axon *accepts* it — `let c = 0;` produces only an unused-variable
    // warning. A `;` row in the table would be an arm keyed on a message that
    // is never emitted: dead code that reads as coverage.
    let diags = check_pipeline("fn main() -> i64 {\n    let c = 0;\n    0\n}\n", "probe.ax");
    assert!(
        !diags.iter().any(|d| d.code == "E0000"),
        "`;` is accepted; if this now fails, Axon's grammar changed and `;` \
         becomes a legitimate parse_help row: {diags:?}",
    );
}

#[test]
fn const_and_var_do_not_reach_the_parse_tier() {
    // Also on §1's list, and also not a parse error: `const`/`var` lex as plain
    // identifiers, so they fail at name resolution instead. Recorded here so
    // the next reader does not add parse_help arms for them and wonder why the
    // arms never fire. They are handled at their own tier.
    for src in [
        "fn main() -> i64 {\n    const c = 0\n    0\n}\n",
        "fn main() -> i64 {\n    var c = 0\n    0\n}\n",
    ] {
        let diags = check_pipeline(src, "probe.ax");
        assert!(
            !diags.iter().any(|d| d.code == "E0000"),
            "const/var reach the RESOLVE tier, not the parse tier: {diags:?}",
        );
    }
}

#[test]
fn a_valid_program_produces_no_parse_help() {
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let count = 0\n    count\n}\n",
        "probe.ax",
    );
    assert!(
        diags.iter().all(|d| d.help.is_none() || d.code != "E0000"),
        "{diags:?}"
    );
}
