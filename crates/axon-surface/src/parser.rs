//! Markdown → `GoalFile` parsing.
//!
//! Goal files use a stable section-based structure (see
//! `examples/goals/hello-goal.md`).  We extract the body of each
//! `## <Section>` heading, then pull specific fields from each.
//!
//! The parser is deliberately strict: missing required sections
//! produce an error rather than a default.  A goal file that doesn't
//! declare its `Verify` predicate is incomplete.

use crate::error::{Error, Result};
use std::collections::BTreeMap;

/// Required top-level sections for a Phase-10 goal file.
const REQUIRED: &[&str] = &[
    "Intent",
    "Inputs",
    "Outputs",
    "Score",
    "Constraints",
    "Budget",
    "Verify",
    "Redteam",
    "Effect surface",
    "Provenance",
];

/// One section of a goal file: heading + body.  Heading retains
/// case (e.g. "Score (higher is better)"), but the LOOKUP key is
/// the first word(s) up to a space or paren.
#[derive(Debug, Clone)]
pub struct Section {
    pub heading: String,
    pub body: String,
}

/// Parsed goal file.
#[derive(Debug, Clone)]
pub struct GoalFile {
    /// First-line title, e.g. "Goal: Summarize a long text into a tweet".
    pub title: String,
    /// Sections keyed by canonical name (first word(s) of heading).
    pub sections: BTreeMap<String, Section>,
}

impl GoalFile {
    pub fn parse(md: &str) -> Result<Self> {
        let title = md
            .lines()
            .next()
            .map(|l| l.trim_start_matches('#').trim().to_string())
            .unwrap_or_default();

        let mut sections: BTreeMap<String, Section> = BTreeMap::new();
        let mut cur_heading: Option<String> = None;
        let mut cur_body = String::new();

        for line in md.lines() {
            if let Some(heading) = line.strip_prefix("## ") {
                // Flush the previous section.
                if let Some(h) = cur_heading.take() {
                    let key = section_key(&h);
                    sections.insert(
                        key,
                        Section {
                            heading: h,
                            body: std::mem::take(&mut cur_body).trim().to_string(),
                        },
                    );
                }
                cur_heading = Some(heading.trim().to_string());
            } else if cur_heading.is_some() {
                cur_body.push_str(line);
                cur_body.push('\n');
            }
        }
        // Flush the last section.
        if let Some(h) = cur_heading {
            let key = section_key(&h);
            sections.insert(
                key,
                Section {
                    heading: h,
                    body: cur_body.trim().to_string(),
                },
            );
        }

        // Verify required sections. Collect ALL missing ones (Bug #3) so the
        // author sees the complete list in one error instead of discovering
        // them one re-run at a time. REQUIRED is iterated in declaration order,
        // so the message reads top-to-bottom as the file should be structured.
        let missing: Vec<String> = REQUIRED
            .iter()
            .filter(|req| !sections.contains_key(**req))
            .map(|req| (*req).to_string())
            .collect();
        if !missing.is_empty() {
            return Err(Error::MissingSections(missing));
        }

        Ok(Self { title, sections })
    }

    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections.get(name)
    }

    /// Extract `(name, type)` pairs from the `Inputs` section.
    /// Expects markdown bullets of the form:
    ///   `- \`name: type\` — description`
    pub fn inputs(&self) -> Result<Vec<(String, String)>> {
        let s = self
            .section("Inputs")
            .expect("required, validated by parse");
        parse_typed_bullets(&s.body, "Inputs")
    }

    /// Extract `(name, type)` pairs from the `Outputs` section.
    pub fn outputs(&self) -> Result<Vec<(String, String)>> {
        let s = self
            .section("Outputs")
            .expect("required, validated by parse");
        parse_typed_bullets(&s.body, "Outputs")
    }

    /// Extract the verify predicate from the `Verify` section.
    /// Expects the predicate inside an ```axon code fence with
    /// `@[verify(...)]` syntax.  Returns the predicate text inside
    /// the parens.
    pub fn verify_predicate(&self) -> Result<String> {
        let s = self.section("Verify").expect("required");
        // Look for `@[verify(...)]`.
        let body = &s.body;
        let start = body
            .find("@[verify(")
            .ok_or_else(|| Error::ExtractionFailed {
                field: "@[verify(...)]".into(),
                section: "Verify".into(),
            })?;
        // Skip past `@[verify(`.
        let after = &body[start + "@[verify(".len()..];
        // Find the matching `)]` — naive but works for the simple
        // predicate forms goal files use today.
        let end = after.find(")]").ok_or_else(|| Error::MalformedSection {
            section: "Verify".into(),
            detail: "unmatched @[verify(... ) — missing `)]`".into(),
        })?;
        Ok(after[..end].trim().to_string())
    }

    /// Extract a structured `@[contained(...)]` capability declaration from the
    /// `Effect surface` section, if the author supplied one (as plain text, no
    /// fence required). When present, `compile::emit` stamps it on the generated
    /// search loop so the prose-declared effect surface is COMPILER-ENFORCED
    /// (E1001/E1004 at `axon check`), not merely documented — the value wedge
    /// ("the prose says no network; the compiler refuses it") applied to a prose
    /// goal. Returns the full `@[contained(...)]` attribute text, or `None` when
    /// the section is free prose (the common case — surface stays advisory).
    ///
    /// Bracket-balanced (not a naive `)]` scan) because a cap list nests brackets
    /// and parens: `@[contained(fs: [write("./out/")], net: ["api"], exec: none)]`.
    pub fn contained_attr(&self) -> Option<String> {
        let body = &self.section("Effect surface")?.body;
        let start = body.find("@[contained")?;
        let mut depth = 0i32;
        let mut begun = false;
        for (i, ch) in body[start..].char_indices() {
            match ch {
                '[' => {
                    depth += 1;
                    begun = true;
                }
                ']' => {
                    depth -= 1;
                    if begun && depth == 0 {
                        return Some(body[start..start + i + 1].trim().to_string());
                    }
                }
                _ => {}
            }
        }
        None // unbalanced — treat as absent (the section stays advisory)
    }

    /// The goal's evaluation budget — the first positive integer in the
    /// `Budget` section (e.g. "Up to 20 candidate summaries per run" → 20).
    /// Drives `goal_run`'s `max_evals` so the prose Budget bounds the search.
    pub fn budget_evals(&self) -> Option<i64> {
        let s = self.section("Budget")?;
        let mut run = String::new();
        for ch in s.body.chars() {
            if ch.is_ascii_digit() {
                run.push(ch);
            } else if !run.is_empty() {
                if let Ok(n) = run.parse::<i64>() {
                    if n > 0 {
                        return Some(n);
                    }
                }
                run.clear();
            }
        }
        run.parse::<i64>().ok().filter(|n| *n > 0)
    }

    /// All author-supplied `​```axon` code blocks across the goal file, except
    /// the `Verify` section (whose block is the `@[verify(...)]` predicate,
    /// handled by [`Self::verify_predicate`]).
    ///
    /// These are concatenated, in section order, as the program's author-owned
    /// function definitions, which `compile::emit` lifts verbatim into the
    /// generated `.ax` (replacing the corresponding `TODO:` scaffolding).
    pub fn author_code(&self) -> String {
        let mut out = String::new();
        for (key, sec) in &self.sections {
            // Verify holds the @[verify(...)] predicate (handled separately), and
            // Effect surface may hold a @[contained(...)] declaration (handled by
            // `contained_attr` + stamped on the loop) — neither is lifted as a fn
            // body, so a bare attribute there can't become orphan top-level code.
            if key == "Verify" || key == "Effect surface" {
                continue;
            }
            for block in extract_axon_blocks(&sec.body) {
                let block = block.trim_end();
                if block.is_empty() {
                    continue;
                }
                if !out.is_empty() {
                    out.push_str("\n\n");
                }
                out.push_str(block);
            }
        }
        out
    }
}

/// Extract the contents of every ```axon (or ```ax) fenced code block in `body`.
fn extract_axon_blocks(body: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut in_block = false;
    let mut cur = String::new();
    for line in body.lines() {
        let t = line.trim_start();
        if !in_block {
            if t == "```axon" || t == "```ax" {
                in_block = true;
                cur.clear();
            }
            continue;
        }
        if t.starts_with("```") {
            in_block = false;
            blocks.push(std::mem::take(&mut cur));
            continue;
        }
        cur.push_str(line);
        cur.push('\n');
    }
    blocks
}

/// "Score (higher is better)" → "Score"; "Effect surface" → "Effect surface"
/// (multi-word names with no parens).  We canonicalize by taking the
/// substring up to the first `(` (if any) and trimming.
fn section_key(heading: &str) -> String {
    let s = match heading.find('(') {
        Some(i) => &heading[..i],
        None => heading,
    };
    s.trim().to_string()
}

/// Parse bullets of the form: `- \`name: type\` — description`.
/// Returns Vec<(name, type)>.
fn parse_typed_bullets(body: &str, section_name: &str) -> Result<Vec<(String, String)>> {
    let mut out = Vec::new();
    for line in body.lines() {
        let line = line.trim_start();
        let Some(rest) = line.strip_prefix("- `") else {
            continue;
        };
        let Some(end) = rest.find('`') else {
            continue;
        };
        let inner = &rest[..end];
        let Some((name, ty)) = inner.split_once(':') else {
            return Err(Error::MalformedSection {
                section: section_name.into(),
                detail: format!("bullet missing `:` between name and type: `{inner}`"),
            });
        };
        out.push((name.trim().to_string(), ty.trim().to_string()));
    }
    if out.is_empty() {
        return Err(Error::ExtractionFailed {
            field: "typed bullets".into(),
            section: section_name.into(),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"# Goal: Test sample

## Intent

Do a thing.

## Inputs

- `text: str` — input.

## Outputs

- `summary: str` — output.

## Score (higher is better)

Some scoring.

## Constraints

- A constraint.

## Budget

- 100 calls.

## Verify

```axon
@[verify(score >= 70)]
```

## Redteam

- Some adversarial test.

## Effect surface

- Just LLM calls.

## Provenance

- JSONL log.
"#;

    #[test]
    fn parses_required_sections() {
        let g = GoalFile::parse(SAMPLE).unwrap();
        assert_eq!(g.title, "Goal: Test sample");
        for req in REQUIRED {
            assert!(g.sections.contains_key(*req), "missing {req}");
        }
    }

    #[test]
    fn extracts_inputs_outputs() {
        let g = GoalFile::parse(SAMPLE).unwrap();
        let ins = g.inputs().unwrap();
        assert_eq!(ins, vec![("text".to_string(), "str".to_string())]);
        let outs = g.outputs().unwrap();
        assert_eq!(outs, vec![("summary".to_string(), "str".to_string())]);
    }

    #[test]
    fn extracts_verify_predicate() {
        let g = GoalFile::parse(SAMPLE).unwrap();
        let p = g.verify_predicate().unwrap();
        assert_eq!(p, "score >= 70");
    }

    #[test]
    fn missing_sections_lists_all_at_once() {
        // Bug #3: a goal file with only Intent is missing NINE required
        // sections. The error must name ALL of them in one message, not just
        // the first — otherwise the author fixes one, re-runs, learns of the
        // next, and so on (N-round onboarding friction).
        let bad = "# Goal\n\n## Intent\n\nFoo.\n";
        let err = GoalFile::parse(bad).unwrap_err();
        match err {
            Error::MissingSections(missing) => {
                // Every required section except Intent must be listed.
                for req in REQUIRED.iter().filter(|r| **r != "Intent") {
                    assert!(
                        missing.iter().any(|m| m == req),
                        "missing `{req}` not listed: {missing:?}"
                    );
                }
                assert!(
                    !missing.contains(&"Intent".to_string()),
                    "Intent is present, must not be listed"
                );
            }
            other => panic!("expected MissingSections, got {other:?}"),
        }
    }

    #[test]
    fn missing_sections_error_message_names_all() {
        let bad = "# Goal\n\n## Intent\n\nFoo.\n";
        let msg = GoalFile::parse(bad).unwrap_err().to_string();
        // The rendered message lists the sections so the author sees them all.
        assert!(msg.contains("Inputs"), "msg should list Inputs: {msg}");
        assert!(
            msg.contains("Provenance"),
            "msg should list Provenance: {msg}"
        );
    }

    #[test]
    fn author_code_gathers_axon_blocks_excluding_verify() {
        let md = format!("{SAMPLE}\n## Implementation\n\n```axon\nfn f() -> i64 {{ 1 }}\n```\n");
        let g = GoalFile::parse(&md).unwrap();
        let code = g.author_code();
        assert!(code.contains("fn f() -> i64 { 1 }"), "got: {code:?}");
        // The Verify section's `@[verify(...)]` block must be excluded.
        assert!(
            !code.contains("@[verify"),
            "verify predicate leaked into author_code"
        );
    }

    #[test]
    fn author_code_empty_when_no_blocks() {
        // SAMPLE has only the Verify block, which is excluded.
        let g = GoalFile::parse(SAMPLE).unwrap();
        assert_eq!(g.author_code(), "");
    }

    #[test]
    fn budget_evals_reads_first_positive_int() {
        // SAMPLE's Budget section is "- 100 calls."
        let g = GoalFile::parse(SAMPLE).unwrap();
        assert_eq!(g.budget_evals(), Some(100));
    }
}
