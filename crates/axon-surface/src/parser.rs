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

        // Verify required sections.
        for req in REQUIRED {
            if !sections.contains_key(*req) {
                return Err(Error::MissingSection((*req).to_string()));
            }
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
        let s = self.section("Inputs").expect("required, validated by parse");
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
            if key == "Verify" {
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
    fn missing_section_errors() {
        let bad = "# Goal\n\n## Intent\n\nFoo.\n";
        let err = GoalFile::parse(bad).unwrap_err();
        match err {
            Error::MissingSection(s) => assert_eq!(s, "Inputs"),
            other => panic!("expected MissingSection, got {other:?}"),
        }
    }

    #[test]
    fn author_code_gathers_axon_blocks_excluding_verify() {
        let md = format!(
            "{SAMPLE}\n## Implementation\n\n```axon\nfn f() -> i64 {{ 1 }}\n```\n"
        );
        let g = GoalFile::parse(&md).unwrap();
        let code = g.author_code();
        assert!(code.contains("fn f() -> i64 { 1 }"), "got: {code:?}");
        // The Verify section's `@[verify(...)]` block must be excluded.
        assert!(!code.contains("@[verify"), "verify predicate leaked into author_code");
    }

    #[test]
    fn author_code_empty_when_no_blocks() {
        // SAMPLE has only the Verify block, which is excluded.
        let g = GoalFile::parse(SAMPLE).unwrap();
        assert_eq!(g.author_code(), "");
    }
}
