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

/// Over-approximate a program's declared effects.
///
/// AUDIT T54 (finding P4-OS-03). This was a SECOND, independent copy of
/// `axon_os::runtime::scan_effects` — and it had drifted badly. It still read
/// `source.contains("exec(")`, the exact substring test T4 replaced in the
/// axon-os copy because `exec (name, args)` — one space — parses identically and
/// evades it. It also omitted `env_var` from `fs_read` and the entire
/// `goal_run*` family from `net`, and had no handling for `mod` imports, so a
/// program whose effects lived in an imported module scanned as "no effects".
///
/// That matters here because this result is load-bearing: `pipeline.rs` infers
/// the GRANT from it and then `prove_admissible` checks the program against that
/// grant. An under-approximating scan yields an admissibility proof for a
/// program that performs effects the proof says it does not — the runtime would
/// still refuse it, but the gateway would have told the operator it was fine.
///
/// `axon-intent` already depends on `axon-os`, so the duplication bought
/// nothing. Now there is one implementation and one place to fix.
pub use axon_os::runtime::scan_effects;

/// The concrete file paths a program writes — the first string literal argument
/// of each `write_file(...)` call. Used to cross-check against the intent's
/// declared outputs (scope-expansion guard).
pub fn resolved_write_targets(source: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(idx) = rest.find("write_file(") {
        rest = &rest[idx + "write_file(".len()..];
        // the first string literal argument
        if let Some(open) = rest.find('"') {
            let after = &rest[open + 1..];
            if let Some(close) = after.find('"') {
                let path = after[..close].to_string();
                if !path.is_empty() && !out.contains(&path) {
                    out.push(path);
                }
            }
        }
    }
    out
}

/// Does the source declare an entry point (`fn main`)?
pub fn has_entry(source: &str) -> bool {
    source.contains("fn main")
}

/// Is the body empty or a bare stub (no real statements / a `TODO`)?
pub fn is_stub_body(source: &str) -> bool {
    let s = source.trim();
    if s.is_empty() {
        return true;
    }
    if s.contains("TODO") || s.contains("todo!") || s.contains("unimplemented") {
        return true;
    }
    // A `fn main` whose body is empty `{}` or just `{ 0 }` with no work and no
    // other functions is a stub.
    false
}

/// Structurally derive `SynthMeta` from a source WITHOUT a subprocess (the mock
/// path + the real path's fallback). `typechecks` is supplied by the caller (the
/// real path runs `axon check`; the mock asserts a balanced-brace heuristic).
pub fn meta_from_source(source: &str, typechecks: bool, notes: Vec<String>) -> SynthMeta {
    SynthMeta {
        typechecks,
        declared: scan_effects(source),
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
            "// synthesized by axon-intent (mock, seed={}) — UNDER-SPECIFIED INTENT\nfn main() -> i64 {{\n    // TODO: the intent did not state what to produce; nothing to synthesize\n    0\n}}\n",
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
        let d = scan_effects("read_file(x); write_file(y)");
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
        let imported = scan_effects("mod helper\nfn f() -> i64 { helper::go() }");
        assert!(
            imported.row.exec && imported.row.net && imported.row.fs_write,
            "a mod import puts the effects in a file we did not read — the honest \
             answer is the full set, not `none found here`"
        );
    }

    #[test]
    fn resolved_write_targets_extracts_literal_paths() {
        let t = resolved_write_targets(
            "write_file(\"./out/a.txt\", x); write_file(\"./out/b.txt\", y)",
        );
        assert_eq!(
            t,
            vec!["./out/a.txt".to_string(), "./out/b.txt".to_string()]
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
