//! R22 §3.2 — the `Synthesizer` seam the MOCK path uses, plus the pure source
//! analysis every candidate goes through (`strip_fences`, `meta_from_source`).
//!
//! - `MockSynth` is the deterministic test/audit path (`AXON_AI_MOCK=1`; no
//!   network, no key); a given `(intent, seed)` yields a byte-identical draft
//!   (A5). Its "type-check" (an entry, balanced braces, no stub body) is a
//!   MOCK-ONLY heuristic: it never stands in for `axon check` outside the mock.
//! - Real generation is the check→repair loop (`crate::genloop`, IG4) behind a
//!   `crate::generator::Generator`; every candidate it accepts passed the real
//!   `axon check`, the gates and the intent's examples. (IG5 removed the
//!   single-shot `RealSynthesizer` that shelled out to `axon intent compile`.)
//!
//! Everything downstream (`grant_infer`/`admit`/`confidence`/`approval`) is a
//! pure function of `(Intent, ProgramDraft)`.

use crate::intent::Intent;
use axon_core::ast::{walk_expr, Expr, Item, Literal, Program};
use axon_core::builtins::{builtin_write_path_args, AdmissionAxes};
use axon_core::capabilities::program_admission_axes;
use axon_os::gate::DeclaredEffects;

/// The synthesizer's output: the candidate program + the confidence inputs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProgramDraft {
    pub source: String,
    /// true = synthesizer-authored; false = lifted verbatim from the intent's
    /// fenced ```axon block.
    pub generated: bool,
    /// whether ```axon fences were stripped from the model output.
    pub fences_stripped: bool,
    pub meta: SynthMeta,
}

/// The confidence inputs extracted from a candidate program (R22 §3.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthMeta {
    pub typechecks: bool,
    pub declared: DeclaredEffects,
    /// Every admission axis the program uses, from the compiler's one table
    /// (`axon_core::builtins::builtin_admission_axes`); every axis when the
    /// source does not parse. `declared` is its projection onto the grant's
    /// four axes (`env` rides on `fs_read`).
    pub axes: AdmissionAxes,
    pub has_entry: bool,
    /// outputs the program actually writes (cross-checked vs the intent).
    pub resolved_outputs: Vec<String>,
    /// fuzzy resolutions the synthesizer flagged (each lowers confidence).
    pub uncertainty_notes: Vec<String>,
}

/// A synthesis failure (model error / empty output). Distinct from a typecheck
/// failure, which is recorded in `SynthMeta` and handled by the confidence gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SynthErr {
    pub reason: String,
}

/// The seam (R22 §5.1). Every call to a model / process lives behind this.
pub trait Synthesizer {
    /// Synthesize an `.ax` program source from the intent. The single edge to a
    /// model / subprocess.
    fn synthesize(&self, intent: &Intent) -> Result<String, SynthErr>;

    /// Type-check a candidate source and extract its `SynthMeta` (declared
    /// effects, entry presence, the outputs it writes). The mock derives it
    /// structurally.
    fn typecheck(&self, source: &str) -> SynthMeta;
}

// ── pure source analysis (shared by mock + real meta extraction) ─────────────

/// Strip an accidental wrapping ```axon … ``` fence from model output. Returns
/// `(stripped_source, did_strip)`.
pub fn strip_fences(src: &str) -> (String, bool) {
    let t = src.trim();
    if let Some(rest) = t.strip_prefix("```axon") {
        if let Some(body) = rest.rsplit_once("```") {
            return (body.0.trim().to_string(), true);
        }
    }
    if let Some(rest) = t.strip_prefix("```") {
        if let Some(body) = rest.rsplit_once("```") {
            return (body.0.trim().to_string(), true);
        }
    }
    (src.to_string(), false)
}

/// The axon-os scanner, re-exported: `pipeline.rs` infers the GRANT from its
/// result and `prove_admissible` checks the program against that grant, so
/// there is one scanner, in one place (AUDIT T54 removed a drifted second
/// copy from this crate). [`meta_from_source`] calls the same projection on
/// its own parse.
pub use axon_os::runtime::{scan_effects, try_scan_effects};

/// Every root of code in a parsed program: fn and method bodies, module
/// `let` initializers, refinement predicates.
fn code_roots(program: &Program) -> Vec<&Expr> {
    let mut roots = Vec::new();
    for item in &program.items {
        match item {
            Item::FnDef(f) => roots.push(&f.body),
            Item::ImplBlock(b) => roots.extend(b.methods.iter().map(|m| &m.body)),
            Item::LetDef { value, .. } => roots.push(&**value),
            Item::RefineDef(r) => roots.push(&*r.predicate),
            _ => {}
        }
    }
    roots
}

/// The concrete files a program writes: each string-literal path argument a
/// builtin WRITES (the per-argument table,
/// `axon_core::builtins::builtin_write_path_args`) and leaves behind as a
/// file — so not `dir_create`'s directory, and not `file_rename`'s source,
/// which the rename removes. Used to cross-check against the intent's declared
/// outputs (scope-expansion guard).
///
/// IG0 (F12 c). This took the next string literal after the text
/// `write_file(`, so `write_file(out, "report: " + t)` resolved to the
/// "output" `report: ` and was refused as outputs-mismatch. On the AST, a
/// target that is not a literal is UNRESOLVED — not listed here, and not a
/// refusal: the examples check where it lands. A source that does not parse
/// resolves nothing (the type check refuses it).
pub fn resolved_write_targets(source: &str) -> Vec<String> {
    let Ok(program) = axon_core::parse_source(source) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    for root in code_roots(&program) {
        walk_expr(root, &mut |e| {
            let Expr::Call { callee, args, .. } = e else {
                return;
            };
            let Expr::Ident(name) = &**callee else {
                return;
            };
            if name == "dir_create" {
                return;
            }
            for i in builtin_write_path_args(name) {
                if name == "file_rename" && i == 0 {
                    continue;
                }
                if let Some(Expr::Literal(Literal::Str(path))) = args.get(i) {
                    if !path.is_empty() && !out.contains(path) {
                        out.push(path.clone());
                    }
                }
            }
        });
    }
    out
}

/// Does the source declare an entry point (an `fn main` item)?
pub fn has_entry(source: &str) -> bool {
    axon_core::parse_source(source).is_ok_and(|p| {
        p.items
            .iter()
            .any(|i| matches!(i, Item::FnDef(f) if f.name == "main"))
    })
}

/// Is the program empty, or a stub — no fn or method with a statement in its
/// body?
///
/// IG0 (F12 d). This was a substring test for `TODO` / `todo!` /
/// `unimplemented`, so a complete program with the comment `// TODO: handle a
/// missing report more gracefully` was refused as empty-or-stub-body. On the
/// AST a comment is never a stub. A source that does not parse is not a stub:
/// the type check refuses it, with the reason that is actually true.
pub fn is_stub_body(source: &str) -> bool {
    if source.trim().is_empty() {
        return true;
    }
    let Ok(program) = axon_core::parse_source(source) else {
        return false;
    };
    let bodies = program.items.iter().flat_map(|i| match i {
        Item::FnDef(f) => vec![&f.body],
        Item::ImplBlock(b) => b.methods.iter().map(|m| &m.body).collect(),
        _ => Vec::new(),
    });
    !bodies
        .into_iter()
        .any(|b| !matches!(b, Expr::Block(stmts) if stmts.is_empty()))
}

/// Structurally derive `SynthMeta` from a source WITHOUT a subprocess (the mock
/// path + the real path's fallback). `typechecks` is supplied by the caller (the
/// real path runs `axon check`; the mock asserts a balanced-brace heuristic);
/// a source that does not parse never type-checks.
pub fn meta_from_source(source: &str, typechecks: bool, notes: Vec<String>) -> SynthMeta {
    let parsed = axon_core::parse_source(source).ok();
    let axes = parsed
        .as_ref()
        .map(program_admission_axes)
        .unwrap_or(AdmissionAxes::ALL);
    SynthMeta {
        typechecks: typechecks && parsed.is_some(),
        declared: match parsed {
            Some(_) => axon_os::runtime::declared_from_axes(axes),
            None => DeclaredEffects::unknown(),
        },
        axes,
        has_entry: has_entry(source),
        resolved_outputs: resolved_write_targets(source),
        uncertainty_notes: notes,
    }
}

// ── MockSynth — the deterministic test/audit synthesizer (no I/O) ─────────────

/// A deterministic, in-memory synthesizer for tests + reproducible audit. It is
/// a pure function of the intent (and an optional override source), so the whole
/// pipeline is byte-reproducible (A5). It NEVER touches a network or a key.
#[derive(Default)]
pub struct MockSynth {
    /// If set, `synthesize` returns this verbatim (lets a test force a specific
    /// program — e.g. an adversarial one declaring more than it was inferred).
    pub override_source: Option<String>,
    /// Forced typecheck result for the override (default: a balanced-brace check).
    pub force_typechecks: Option<bool>,
    /// Notes to attach to the meta (uncertainty markers).
    pub notes: Vec<String>,
}

impl MockSynth {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn with_source(src: impl Into<String>) -> Self {
        MockSynth {
            override_source: Some(src.into()),
            ..Self::default()
        }
    }
}

/// The canonical deterministic program the mock synthesizes for a summarize-shape
/// intent: read the first input, write the first output. Pure function of the
/// intent's first input/output paths (A5). This mirrors the shipped
/// `summarize.ax` so the mock journey matches the real one.
///
/// An UNDER-SPECIFIED intent (no concrete output to produce) yields a stub the
/// confidence gate refuses — the deterministic stand-in for "the model could not
/// produce a real program from a vague goal" (the `vague.intent.md` headline
/// negative demo; fail closed, never best-effort).
pub fn mock_program(intent: &Intent) -> String {
    if intent.outputs.is_empty() {
        return format!(
            "// synthesized by axon-intent (mock, seed={}) — UNDER-SPECIFIED INTENT\nfn main() -> i64 {{\n    // the intent did not state what to produce; nothing to synthesize\n}}\n",
            intent.seed,
        );
    }
    let input = intent
        .inputs
        .first()
        .cloned()
        .unwrap_or_else(|| "./data/report.txt".to_string());
    let output = intent
        .outputs
        .first()
        .cloned()
        .unwrap_or_else(|| "./out/summary.txt".to_string());
    format!(
        "// synthesized by axon-intent (mock, seed={}) from: {}\nfn main() -> i64 {{\n    match read_file(\"{input}\") {{\n        Ok(text) => {{\n            let n = len(text)\n            match write_file(\"{output}\", \"report length: {{to_str(n)}} bytes\") {{\n                Ok(_) => 0\n                Err(_) => 1\n            }}\n        }}\n        Err(_) => 2\n    }}\n}}\n",
        intent.seed, intent.goal,
    )
}

impl Synthesizer for MockSynth {
    fn synthesize(&self, intent: &Intent) -> Result<String, SynthErr> {
        if let Some(src) = &self.override_source {
            return Ok(src.clone());
        }
        Ok(mock_program(intent))
    }

    fn typecheck(&self, source: &str) -> SynthMeta {
        // The mock's "type-check" is a deterministic structural heuristic: a
        // program with an entry and balanced braces "type-checks". A forced
        // value (set by a test) overrides it.
        let typechecks = self.force_typechecks.unwrap_or_else(|| {
            has_entry(source) && braces_balanced(source) && !is_stub_body(source)
        });
        meta_from_source(source, typechecks, self.notes.clone())
    }
}

/// A trivial balanced-brace check: the mock's stand-in for a type-check, never
/// used outside the mock.
fn braces_balanced(s: &str) -> bool {
    let mut depth: i64 = 0;
    for c in s.chars() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth < 0 {
                    return false;
                }
            }
            _ => {}
        }
    }
    depth == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent;

    const VALID: &str = r#"# Intent
Summarize ./data/report.txt into ./out/summary.txt.

## Inputs
- ./data/report.txt

## Outputs
- ./out/summary.txt

## Allowed
- fs_read: ./data/
- fs_write: ./out/
- net: none
- exec: none
- max_label: internal

## Budget
- calls: 100
- tokens: 50000
- cost_micro: 1000000
"#;

    fn it() -> Intent {
        intent::parse(VALID).unwrap()
    }

    #[test]
    fn strip_fences_removes_an_axon_fence() {
        let (s, did) = strip_fences("```axon\nfn main() -> i64 { 0 }\n```");
        assert!(did);
        assert!(s.starts_with("fn main"));
        let (s2, did2) = strip_fences("fn main() -> i64 { 0 }");
        assert!(!did2);
        assert_eq!(s2, "fn main() -> i64 { 0 }");
    }

    #[test]
    fn scan_effects_finds_fs_axes() {
        let d = scan_effects("fn f(x: str, y: str) { read_file(x)\n write_file(y, x) }");
        assert!(d.row.fs_read && d.row.fs_write && !d.row.net && !d.row.exec);
    }

    #[test]
    fn scan_effects_here_is_the_axon_os_scanner_not_a_second_copy() {
        // AUDIT T54 (P4-OS-03). This crate used to keep its own copy of the
        // scanner, and that copy still had the pre-T4 `source.contains("exec(")`
        // test — which `exec (…)`, one space, parses identically and evades.
        // These four assertions all FAIL against the old local copy and pass
        // against the axon-os implementation, so they pin the dedup itself:
        // re-introducing a local scanner reverts them.
        assert!(
            scan_effects("fn f() { exec (\"id\", []) }").row.exec,
            "whitespace-separated exec must be detected (the T4 evasion)"
        );
        assert!(
            scan_effects("fn f() { env_var(\"HOME\") }").row.fs_read,
            "env_var is an ambient read the local copy omitted"
        );
        assert!(
            scan_effects("fn f() { goal_run(\"m\", 0, 10) }").row.net,
            "goal_run re-calls @[adaptive] metrics, which may ai_complete"
        );
        let imported = scan_effects("mod helper\nuse helper.{go}\nfn f() -> i64 { go() }");
        assert!(
            imported.row.exec && imported.row.net && imported.row.fs_write,
            "a mod import puts the effects in a file we did not read — the honest \
             answer is the full set, not `none found here`"
        );
    }

    #[test]
    fn resolved_write_targets_extracts_literal_paths() {
        let t = resolved_write_targets(
            "fn f(x: str, y: str) {\n write_file(\"./out/a.txt\", x)\n append_file(\"./out/b.txt\", y)\n file_copy(\"./in.txt\", \"./out/c.txt\")\n file_rename(\"./out/tmp\", \"./out/d.txt\")\n dir_create(\"./out/\")\n}",
        );
        assert_eq!(
            t,
            vec![
                "./out/a.txt".to_string(),
                "./out/b.txt".to_string(),
                "./out/c.txt".to_string(),
                "./out/d.txt".to_string()
            ],
            "every written file, not a copy's source, a rename's source or a directory"
        );
    }

    /// IG0 F12 (c): a write target that is not a literal is UNRESOLVED, never
    /// the next string literal the text happens to hold.
    #[test]
    fn a_non_literal_write_target_is_unresolved_not_the_next_literal() {
        let src = "fn main() -> i64 {\n    let out = \"./out/summary.txt\"\n    match write_file(out, \"report: \" + \"x\") { Ok(_) => 0, Err(_) => 1 }\n}\n";
        assert!(
            resolved_write_targets(src).is_empty(),
            "ATTACK: a non-literal write target resolved to a string literal: {:?}",
            resolved_write_targets(src)
        );
        // In a comment or a string, `write_file(` is no call at all.
        let quoted = "// write_file(\"./etc/x\", y)\nfn main() -> i64 {\n    println(\"write_file(\\\"./etc/y\\\", z)\")\n    0\n}\n";
        assert!(resolved_write_targets(quoted).is_empty());
    }

    /// IG0 F12 (d): a comment is never a stub; an empty body still is.
    #[test]
    fn a_todo_comment_is_not_a_stub_but_an_empty_body_is() {
        let src = "// TODO: handle a missing report more gracefully\nfn main() -> i64 {\n    println(\"unimplemented todo!\")\n    0\n}\n";
        assert!(
            !is_stub_body(src),
            "ATTACK: a complete program with a TODO comment was read as a stub"
        );
        assert!(is_stub_body(""));
        assert!(is_stub_body("fn main() {}\n"));
        assert!(has_entry(src));
        assert!(!has_entry("// fn main\nfn helper() -> i64 { 1 }\n"));
    }

    /// IG0 F12 (a): the meta's effect set comes from the one table, so an
    /// `append_file` program declares the `fs_write` it needs.
    #[test]
    fn meta_declares_append_file_as_a_write() {
        let m = meta_from_source(
            "fn main() -> i64 {\n    match append_file(\"./out/log.txt\", \"x\") { Ok(_) => 0, Err(_) => 1 }\n}\n",
            true,
            vec![],
        );
        assert!(
            m.declared.row.fs_write && m.axes.fs_write,
            "ATTACK: an append_file program was admitted without fs_write: {m:?}"
        );
        assert!(!m.declared.row.net && !m.declared.row.exec && !m.declared.row.fs_read);
        assert_eq!(m.resolved_outputs, vec!["./out/log.txt".to_string()]);
        let bad = meta_from_source("fn main( {", true, vec![]);
        assert!(
            !bad.typechecks,
            "a source that does not parse never type-checks"
        );
    }

    #[test]
    fn mock_is_deterministic_per_intent() {
        let s = MockSynth::new();
        let a = s.synthesize(&it()).unwrap();
        let b = s.synthesize(&it()).unwrap();
        assert_eq!(a, b, "mock synthesis is byte-identical (A5 building block)");
        assert!(a.contains("read_file(\"./data/report.txt\")"));
        assert!(a.contains("write_file(\"./out/summary.txt\""));
    }

    #[test]
    fn mock_meta_typechecks_a_well_formed_program() {
        let s = MockSynth::new();
        let src = s.synthesize(&it()).unwrap();
        let meta = s.typecheck(&src);
        assert!(meta.typechecks && meta.has_entry);
        assert_eq!(meta.resolved_outputs, vec!["./out/summary.txt".to_string()]);
        assert!(meta.declared.row.fs_read && meta.declared.row.fs_write);
        assert!(!meta.declared.row.net);
    }
}
