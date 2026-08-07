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
fn mut_no_longer_reaches_the_parse_tier_at_all() {
    // Was `mut_still_reaches_the_parse_tier_and_still_gets_help`. M5 made
    // `let mut` COMPILE, so the parse error this pinned no longer exists — the
    // test correctly went red and is retargeted rather than deleted, so the
    // history of the decision stays visible. `parse_help`'s `mut` row is now
    // unreachable from real source and is kept only for a hand-fed message.
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let mut count = 0\n    0\n}\n",
        "probe.ax",
    );
    assert!(
        !diags.iter().any(|d| d.code == "E0000"),
        "`let mut` must no longer be a parse error: {diags:?}"
    );
}

#[test]
fn the_parse_diagnostic_now_carries_a_real_line_and_column() {
    // §1's sibling defect: this diagnostic reported line 0 because the pipeline
    // parsed with the unlocated `parse_source`. A hint that cannot say *where*
    // is only half a repair.
    let diags = check_pipeline("fn main() -> i64 {\n    let c := 0\n    0\n}\n", "probe.ax");
    let d = diags.first().unwrap();
    assert_eq!(d.line, 2, "the bad token is on line 2, got {}", d.line);
    assert!(d.col > 0, "column must be resolved, got {}", d.col);
}

#[test]
fn help_survives_into_the_emitted_json() {
    // The whole point is that a machine consumer can read it. Assert on the
    // wire format, not just the struct field.
    let diags = check_pipeline("fn main() -> i64 {\n    let c := 0\n    0\n}\n", "probe.ax");
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
fn const_and_var_get_foreign_keyword_help_at_the_resolve_tier() {
    // The other half of `const_and_var_do_not_reach_the_parse_tier`: they are
    // not parse errors, so `parse_help` cannot serve them, but they are just as
    // unrepairable and `AXON_FOR_RLM.md` §1 names both.
    //
    // The keyword check runs AHEAD of the spelling suggestion because the
    // suggestion actively misleads: `const` is within 3 edits of real builtins,
    // so the reader used to be told "did you mean `cos`?".
    for (src, kw) in [
        ("fn main() -> i64 {\n    const c = 0\n    0\n}\n", "const"),
        ("fn main() -> i64 {\n    var c = 0\n    0\n}\n", "var"),
    ] {
        let diags = check_pipeline(src, "probe.ax");
        let d = diags
            .iter()
            .find(|d| d.code == "E0001")
            .unwrap_or_else(|| panic!("`{kw}` must produce E0001: {diags:?}"));
        let help = d
            .help
            .as_ref()
            .unwrap_or_else(|| panic!("`{kw}` must carry a fix hint"));
        assert!(
            help.contains(&format!("`{kw}` is not an Axon keyword")),
            "{help}"
        );
        assert!(help.contains("let NAME"), "must show the Axon form: {help}");
        assert!(
            !help.contains("did you mean"),
            "the misleading spelling suggestion must not win: {help}"
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

#[test]
fn a_character_literal_is_named_and_pointed_at_str_eq() {
    // Measured 2026-08-06: this single construct caused ALL THREE remaining
    // failures in the RLM fluency gate, in six of six runs, in both repair arms.
    // It is a LEXER rejection, so it reaches `parse_help` with no `expected …`
    // clause — the arm keys on the message instead.
    let (code, help) = first_diag(
        "fn main() -> i64 {\n    let c = char_at(\"ab\", 0)\n    if c == ' ' { 1 } else { 0 }\n}\n",
    );
    assert_eq!(code, "E0000");
    let help = help.expect("a character literal must carry a fix hint");
    assert!(help.contains("Axon has no character literals"), "{help}");
    assert!(
        help.contains("char_at"),
        "must name the replacement: {help}"
    );
}

#[test]
fn the_character_literal_advice_actually_compiles() {
    // The strongest test in this file, and the one whose absence let WRONG
    // advice ship. The first version of that help said `char_at` returns a `str`
    // and told the reader to write `str_eq(c, " ")`. `char_at` returns the byte
    // value as an `i64`, so a reader who followed the advice got a fresh type
    // error — worse than no advice. Every assertion in the test above still
    // passed, because they only checked which WORDS appeared.
    //
    // A fix hint is advice someone will act on, so the test is: write the code
    // the hint recommends, and require it to type-check.
    for (label, src) in [
        (
            "numeric comparison",
            "fn main() -> i64 {\n    let s = \"a b\"\n    if char_at(s, 1) == 32 { println(\"sp\") }\n    0\n}\n",
        ),
        (
            "one-character slice",
            "fn main() -> i64 {\n    let s = \"a b\"\n    if str_eq(str_slice(s, 1, 2), \" \") { println(\"sp\") }\n    0\n}\n",
        ),
    ] {
        let diags = check_pipeline(src, "advice.ax");
        let errors: Vec<_> = diags.iter().filter(|d| d.severity == "error").collect();
        assert!(
            errors.is_empty(),
            "the hint recommends `{label}`, which must compile: {errors:?}"
        );
    }
}

#[test]
fn a_lexer_error_now_reports_the_real_line() {
    // The lexer's offset was discarded, so every lexer-tier diagnostic claimed
    // line 1 col 1 wherever the bad character actually was. Same "a hint that
    // cannot say where is half a repair" defect as the parse tier, one tier down.
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let c = char_at(\"ab\", 0)\n    if c == ' ' { 1 } else { 0 }\n}\n",
        "probe.ax",
    );
    let d = diags.first().unwrap();
    assert_eq!(d.line, 3, "the quote is on line 3, got line {}", d.line);
}

#[test]
fn python_logical_operators_are_named() {
    for (src, word, op) in [
        (
            "fn main() -> i64 {\n    let a = true\n    if a or a { 1 } else { 0 }\n}\n",
            "or",
            "||",
        ),
        (
            "fn main() -> i64 {\n    let a = true\n    if a and a { 1 } else { 0 }\n}\n",
            "and",
            "&&",
        ),
    ] {
        let (code, help) = first_diag(src);
        assert_eq!(code, "E0000", "{word} must still be a parse error");
        let help = help.unwrap_or_else(|| panic!("`{word}` must carry a fix hint"));
        assert!(help.contains(word) && help.contains(op), "{help}");
    }
}

#[test]
fn the_library_and_cli_check_pipelines_agree_on_a_corpus() {
    // O-RLM-12. `lib::check_pipeline` and `main::run_check_pipeline_located` run
    // the same passes and carry a code comment saying they must stay in sync.
    // They had drifted: `check_pipeline` dropped every resolver diagnostic's
    // `fix`, so library consumers saw no help where the CLI showed it. A comment
    // is not a test, and the drift was found by accident.
    //
    // The CLI half is exercised through the binary (the library cannot call
    // `main`), and the comparison is on the fields both are supposed to carry.
    let corpus: &[(&str, &str)] = &[
        ("mut", "fn main() -> i64 {\n    let mut c = 0\n    0\n}\n"),
        (
            "type-err",
            "fn f() -> i64 { \"s\" }\nfn main() -> i64 { 0 }\n",
        ),
        ("unknown", "fn main() -> i64 {\n    nope()\n}\n"),
        ("const", "fn main() -> i64 {\n    const c = 0\n    0\n}\n"),
        (
            "arity",
            "fn f(a: i64) -> i64 { a }\nfn main() -> i64 { f(1, 2) }\n",
        ),
    ];

    let mut compared = 0usize;
    for (name, src) in corpus {
        let lib: Vec<(String, Option<String>)> = check_pipeline(src, "probe.ax")
            .iter()
            .filter(|d| d.severity == "error")
            .map(|d| (d.code.clone(), d.help.clone()))
            .collect();
        if lib.is_empty() {
            continue;
        }

        let f = std::env::temp_dir().join(format!("axon_pipe_{name}_{}.ax", std::process::id()));
        std::fs::write(&f, src).unwrap();
        let out = std::process::Command::new(env!("CARGO_BIN_EXE_axon"))
            .arg("check")
            .arg(&f)
            .output()
            .expect("spawn axon check");
        let _ = std::fs::remove_file(&f);
        let stderr = String::from_utf8_lossy(&out.stderr);

        for (code, help) in &lib {
            assert!(
                stderr.contains(&format!("\"code\":\"{code}\"")),
                "{name}: the library reports {code} and the CLI does not:\n{stderr}"
            );
            // BOTH directions. A one-way assertion ("the CLI must carry what
            // the library carries") does not catch the drift that actually
            // happened, which was the library dropping help the CLI had —
            // mutation-testing this caught the test, not the code.
            let cli_line = stderr
                .lines()
                .find(|l| l.contains(&format!("\"code\":\"{code}\"")))
                .unwrap_or("");
            let cli_has_help = cli_line.contains("\"help\":");
            assert_eq!(
                help.is_some(),
                cli_has_help,
                "{name}: library help for {code} = {}, CLI help = {cli_has_help}. \
                 The two check pipelines have drifted.\n{stderr}",
                help.is_some()
            );
        }
        compared += 1;
    }
    assert!(
        compared >= 4,
        "corpus matched too little to prove anything: {compared}"
    );
}

#[test]
fn a_named_fn_passed_to_a_higher_order_builtin_is_refused_at_check() {
    // M4. `arr_map([1,2,3], double)` passed `axon check` and then PANICKED at
    // run with "undefined identifier `double`" — a check/run soundness
    // divergence, and the interpreter is this project's reference oracle, so the
    // checker accepting it was the bug. Passing a named fn to a higher-order
    // builtin is the first thing a model writes.
    let src = "fn double(x: i64) -> i64 { x * 2 }\n\
               fn main() -> i64 {\n    let ys = arr_map([1, 2, 3], double)\n    \
               println(to_str(ys[0]))\n    0\n}\n";
    let diags = check_pipeline(src, "probe.ax");
    let d = diags
        .iter()
        .find(|d| d.code == "E0306")
        .unwrap_or_else(|| panic!("must be refused at check time: {diags:?}"));
    assert!(d.message.contains("passed by name"), "{}", d.message);
    let help = d.help.as_ref().expect("must name the working form");
    assert!(help.contains("|x| double(x)"), "{help}");
}

#[test]
fn the_lambda_form_is_still_accepted() {
    // The refusal must not over-reach: wrapping in a lambda is the documented
    // fix, so it has to keep compiling. Without this, M4's guard could refuse
    // every higher-order call and still pass its own test.
    let src = "fn double(x: i64) -> i64 { x * 2 }\n\
               fn main() -> i64 {\n    let ys = arr_map([1, 2, 3], |x| double(x))\n    \
               println(to_str(ys[0]))\n    0\n}\n";
    let errs: Vec<_> = check_pipeline(src, "probe.ax")
        .into_iter()
        .filter(|d| d.severity == "error")
        .collect();
    assert!(
        errs.is_empty(),
        "the lambda form must still compile: {errs:?}"
    );
}

#[test]
fn let_mut_is_accepted_as_a_no_op() {
    // M5. Rust's `mut` claims a binding is reassignable; every Axon local
    // already is. Accepting it asserts nothing false, unlike `def` or `const`.
    // Authorised by the user, overturning AXON_FOR_RLM §3 on evidence §3 did not
    // have: three other channels are MEASURED failures on this defect.
    let src = "fn main() -> i64 {\n    let mut count = 0\n    count = count + 1\n    \
               println(to_str(count))\n    0\n}\n";
    let errs: Vec<_> = check_pipeline(src, "probe.ax")
        .into_iter()
        .filter(|d| d.severity == "error")
        .collect();
    assert!(errs.is_empty(), "`let mut` must now compile: {errs:?}");
}

#[test]
fn a_binding_actually_named_mut_still_works() {
    // The acceptance is guarded on a FOLLOWING identifier, so `let mut = 5`
    // binds a variable called `mut` rather than being silently eaten. Without
    // the guard this program would lose its binding and fail on the use.
    let src = "fn main() -> i64 {\n    let mut = 5\n    println(to_str(mut))\n    0\n}\n";
    let errs: Vec<_> = check_pipeline(src, "probe.ax")
        .into_iter()
        .filter(|d| d.severity == "error")
        .collect();
    assert!(
        errs.is_empty(),
        "a binding named `mut` must still parse: {errs:?}"
    );
}

#[test]
fn accepting_mut_still_tells_the_reader_it_did_nothing() {
    // M5's INFO. Accepting `let mut` silently would leave a reader believing the
    // keyword meant something. It compiles, and says so — severity `note`, so a
    // host filtering on errors is unaffected while a human still learns.
    let diags = check_pipeline(
        "fn main() -> i64 {\n    let mut count = 0\n    count = count + 1\n    0\n}\n",
        "probe.ax",
    );
    let note = diags
        .iter()
        .find(|d| d.code == "I0002")
        .unwrap_or_else(|| panic!("accepting `mut` must emit a note: {diags:?}"));
    assert_eq!(note.severity, "note", "must not be an error or a warning");
    assert_eq!(
        note.line, 2,
        "must point at the `mut`, got line {}",
        note.line
    );
    assert!(note.help.is_some(), "must say what to write instead");
    // And it must not have become an error by accident.
    assert!(
        !diags.iter().any(|d| d.severity == "error"),
        "the program must still compile: {diags:?}"
    );
}

#[test]
fn a_stale_mut_note_does_not_leak_into_the_next_parse() {
    // The premise matters, and the first version of this test got it wrong: on
    // the SUCCESS path the pipeline drains the sink, so a missing clear changes
    // nothing and the mutation survived. The clear is only load-bearing when a
    // parse ACCEPTS a `mut` and then FAILS later — the drain never runs, and the
    // note is left stranded for whatever parses next on this thread.
    //
    // So: parse a program that does exactly that, then a clean one, and require
    // the clean one to carry no note. Removing the clear fails this.
    let poisoned = "fn main() -> i64 {\n    let mut c = 0\n    let d := 1\n    0\n}\n";
    let first = check_pipeline(poisoned, "poisoned.ax");
    assert!(
        first.iter().any(|d| d.code == "E0000"),
        "fixture must fail to parse AFTER the mut: {first:?}"
    );

    let clean = check_pipeline("fn main() -> i64 {\n    let c = 0\n    c\n}\n", "clean.ax");
    assert!(
        !clean.iter().any(|d| d.code == "I0002"),
        "a note leaked from the previous failed parse: {clean:?}"
    );
}

#[test]
fn string_plus_string_concatenates() {
    // N2a. `interp/value.rs:681` has implemented `(Add, Str, Str)` all along —
    // only the CHECKER refused it, so the evaluator arm was unreachable. This is
    // "stop refusing string concat", not "add string concat".
    //
    // It matters because `result = result + ch` is the accumulator every model
    // writes, and it is the wall behind `let mut` in the one R9 task that still
    // fails.
    let src = "fn main() -> i64 {\n    let a = \"a\"\n    let b = a + \"b\"\n    \
               println(b)\n    0\n}\n";
    let errs: Vec<_> = check_pipeline(src, "probe.ax")
        .into_iter()
        .filter(|d| d.severity == "error")
        .collect();
    assert!(errs.is_empty(), "`str + str` must type-check: {errs:?}");
}

#[test]
fn plus_still_refuses_mixed_and_nonsense_operands() {
    // The permission must be narrow. A `+` that accepts anything is not
    // concatenation, it is an untyped operator, and that would be a soundness
    // regression rather than a feature.
    for (label, src) in [
        ("str + int", "fn main() -> i64 {\n    let x = \"a\" + 1\n    0\n}\n"),
        ("int + str", "fn main() -> i64 {\n    let x = 1 + \"a\"\n    0\n}\n"),
        ("str - str", "fn main() -> i64 {\n    let x = \"a\" - \"b\"\n    0\n}\n"),
        ("bool + bool", "fn main() -> i64 {\n    let x = true + false\n    0\n}\n"),
    ] {
        let errs: Vec<_> = check_pipeline(src, "probe.ax")
            .into_iter()
            .filter(|d| d.severity == "error")
            .collect();
        assert!(!errs.is_empty(), "{label} must still be refused");
    }
}
