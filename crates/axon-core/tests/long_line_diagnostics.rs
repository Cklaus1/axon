//! compilebench AX-68: `axon run` preflight on a long chain of `try` terms.
//!
//! `try` is not a name, so each of the 30,000 terms of the finding's repro is
//! one `E0001`, and every diagnostic carried the whole line, 120,000 bytes, and
//! a run of spaces as long as its offset: 5.7 GB on native, a heap abort on
//! wasm32. The caret block is now a bounded window of the line.

use axon_core::check_pipeline;

/// The finding's program, with `terms` terms on one line.
fn try_chain(terms: usize) -> String {
    format!(
        "fn main() -> i64 {{\n    let x = {}1\n    0\n}}\n",
        "try ".repeat(terms)
    )
}

#[test]
fn a_long_line_of_errors_costs_linear_memory() {
    let terms = 3_000;
    let src = try_chain(terms);
    let diags = check_pipeline(&src, "try.ax");
    let errors = diags.iter().filter(|d| d.code == "E0001").count();
    assert!(errors >= terms, "{errors} E0001 for {terms} terms");
    // Whole lines would be about 2 x 12,000 bytes per diagnostic: 72 MB.
    let caret_bytes: usize = diags.iter().map(|d| d.caret.len()).sum();
    assert!(
        caret_bytes < diags.len() * 1_000,
        "{caret_bytes} bytes of caret for {} diagnostics",
        diags.len()
    );
    // A caret far along the line shows the text around it, under the marks.
    let far = diags
        .iter()
        .find(|d| d.code == "E0001" && d.col > 6_000)
        .unwrap();
    assert!(
        far.caret.contains("try try") && far.caret.contains('^'),
        "{}",
        far.caret
    );
    assert!(far.caret.starts_with("   2 │ …"), "{}", far.caret);
}

#[test]
fn a_short_line_keeps_its_whole_caret_block() {
    let src = "fn main() -> i64 {\n    let x = try 1\n    0\n}\n";
    let diags = check_pipeline(src, "short.ax");
    let d = diags.iter().find(|d| d.code == "E0001").unwrap();
    assert_eq!(d.caret, "   2 │     let x = try 1\n     │     ^^^");
}
