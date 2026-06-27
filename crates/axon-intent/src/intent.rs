//! R22 §3.1 — parse + validate an `.intent.md` into an `Intent`. Pure (no I/O).
//!
//! The `.intent.md` is prose plus named sections (`# Intent`, `## Inputs`,
//! `## Outputs`, `## Allowed`, `## Budget`, `## Seed`). The `## Allowed` section
//! is the human's explicit upper bound on authority — a `CapCeiling`. A
//! malformed intent fails closed → `Refusal::Malformed` (exit 2). A MISSING
//! `## Allowed` is fail-closed too: no ceiling ⇒ refuse (we never default to
//! "allow everything"; the human must state what they permit).

use crate::refusal::Refusal;
use axon_os::grant::{Budget, ExecPolicy, Host, Label, PathPrefix};

/// The intent's explicit upper bound on authority (R22 §3.1). The inferred grant
/// can never exceed this.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapCeiling {
    pub fs_read: Vec<PathPrefix>,
    pub fs_write: Vec<PathPrefix>,
    pub net: Vec<Host>,
    pub exec: ExecPolicy,
    pub max_label: Label,
}

/// A parsed, validated intent (R22 §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Intent {
    pub goal: String,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub cap_ceiling: CapCeiling,
    pub budget: Budget,
    pub seed: u64,
    /// An author-supplied fenced ```axon block, lifted verbatim if present (the
    /// non-generated path). Empty ⇒ the synthesizer authors the program.
    pub fenced_axon: Option<String>,
}

fn bad(reason: impl Into<String>) -> Refusal {
    Refusal::Malformed {
        reason: reason.into(),
    }
}

/// A header-delimited section of the markdown: its level-2 (`##`) title (lower-
/// cased) → the lines beneath it, until the next header.
struct Sections {
    /// The first prose paragraph beneath `# Intent` (the goal sentence).
    goal: String,
    /// `## <name>` (lowercased) → raw body lines.
    named: Vec<(String, Vec<String>)>,
    /// The first fenced ```axon block body, if any.
    fenced_axon: Option<String>,
}

/// Split markdown into the `# Intent` goal line, the `## …` sections, and the
/// first fenced ```axon block. Pure string work.
fn split_sections(src: &str) -> Result<Sections, Refusal> {
    let mut goal = String::new();
    let mut named: Vec<(String, Vec<String>)> = Vec::new();
    let mut fenced_axon: Option<String> = None;

    let mut saw_intent_header = false;
    let mut in_intent_prose = false;
    let mut cur: Option<usize> = None; // index into `named` of the current section
    let mut in_fence = false;
    let mut fence_is_axon = false;
    let mut fence_buf = String::new();

    for raw in src.lines() {
        let line = raw.trim_end();
        // Fenced code blocks are opaque to header parsing.
        if let Some(rest) = line.trim_start().strip_prefix("```") {
            if in_fence {
                if fence_is_axon && fenced_axon.is_none() {
                    fenced_axon = Some(fence_buf.clone());
                }
                in_fence = false;
                fence_is_axon = false;
                fence_buf.clear();
            } else {
                in_fence = true;
                fence_is_axon = rest.trim() == "axon";
            }
            continue;
        }
        if in_fence {
            if fence_is_axon {
                fence_buf.push_str(raw);
                fence_buf.push('\n');
            }
            continue;
        }

        if let Some(title) = line.strip_prefix("## ") {
            named.push((title.trim().to_lowercase(), Vec::new()));
            cur = Some(named.len() - 1);
            in_intent_prose = false;
            continue;
        }
        if let Some(title) = line.strip_prefix("# ") {
            if title.trim().eq_ignore_ascii_case("intent") {
                saw_intent_header = true;
                in_intent_prose = true;
            }
            cur = None;
            continue;
        }

        if in_intent_prose {
            let t = line.trim();
            if !t.is_empty() {
                if !goal.is_empty() {
                    goal.push(' ');
                }
                goal.push_str(t);
            } else if !goal.is_empty() {
                in_intent_prose = false; // goal = first paragraph only
            }
            continue;
        }
        if let Some(i) = cur {
            named[i].1.push(line.to_string());
        }
    }

    if !saw_intent_header {
        return Err(bad("missing `# Intent` header"));
    }
    Ok(Sections {
        goal,
        named,
        fenced_axon,
    })
}

/// Parse `- ` bullet lines from a section body (trimmed, non-empty).
fn bullets(body: &[String]) -> Vec<String> {
    body.iter()
        .filter_map(|l| {
            let t = l.trim();
            t.strip_prefix("- ").or_else(|| t.strip_prefix("* "))
        })
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Parse a `## Allowed` body: `- key: value` lines. `fs_read`/`fs_write`/`net`
/// may repeat or be comma-separated; `net: none` ⇒ empty.
fn parse_allowed(body: &[String]) -> Result<CapCeiling, Refusal> {
    let mut fs_read: Vec<String> = Vec::new();
    let mut fs_write: Vec<String> = Vec::new();
    let mut net: Vec<String> = Vec::new();
    let mut exec: Option<ExecPolicy> = None;
    let mut max_label: Option<Label> = None;

    for b in bullets(body) {
        let (key, val) = b
            .split_once(':')
            .ok_or_else(|| bad(format!("`## Allowed`: expected `key: value`, got `{b}`")))?;
        let (key, val) = (key.trim(), val.trim());
        let items = || -> Vec<String> {
            if val.eq_ignore_ascii_case("none") || val.is_empty() {
                Vec::new()
            } else {
                val.split(',').map(|s| s.trim().to_string()).collect()
            }
        };
        match key {
            "fs_read" => fs_read.extend(items()),
            "fs_write" => fs_write.extend(items()),
            "net" => net.extend(items()),
            "exec" => {
                exec = Some(ExecPolicy::parse(val).ok_or_else(|| {
                    bad(format!(
                        "`## Allowed`: exec must be \"none\" or \"any\", got `{val}`"
                    ))
                })?)
            }
            "max_label" => {
                max_label = Some(Label::parse(val).ok_or_else(|| {
                    bad(format!(
                        "`## Allowed`: max_label must be public|internal|secret, got `{val}`"
                    ))
                })?)
            }
            other => return Err(bad(format!("`## Allowed`: unknown key `{other}`"))),
        }
    }

    let fs_read = validate_prefixes(fs_read, "fs_read")?;
    let fs_write = validate_prefixes(fs_write, "fs_write")?;
    let net = net.into_iter().filter(|s| !s.is_empty()).collect();
    Ok(CapCeiling {
        fs_read,
        fs_write,
        net,
        exec: exec.unwrap_or(ExecPolicy::None),
        max_label: max_label.unwrap_or(Label::Internal),
    })
}

/// Parse a `## Budget` body: `- calls: N`, `- tokens: N`, `- cost_micro: N`.
fn parse_budget(body: &[String]) -> Result<Budget, Refusal> {
    let mut calls = 0i64;
    let mut tokens = 0i64;
    let mut cost_micro = 0i64;
    for b in bullets(body) {
        let (key, val) = b
            .split_once(':')
            .ok_or_else(|| bad(format!("`## Budget`: expected `key: value`, got `{b}`")))?;
        let n = val
            .trim()
            .parse::<i64>()
            .map_err(|_| bad(format!("`## Budget`: `{}` is not an integer", val.trim())))?;
        match key.trim() {
            "calls" => calls = n,
            "tokens" => tokens = n,
            "cost_micro" => cost_micro = n,
            other => return Err(bad(format!("`## Budget`: unknown key `{other}`"))),
        }
    }
    Ok(Budget {
        calls: nonneg(calls, "calls")?,
        tokens: nonneg(tokens, "tokens")?,
        cost_micro: nonneg(cost_micro, "cost_micro")?,
    })
}

/// Parse + validate an `.intent.md`. Fail closed (`Refusal::Malformed`) on any
/// missing `# Intent`/`## Allowed`, traversal path, bad enum, or negative budget.
pub fn parse(src: &str) -> Result<Intent, Refusal> {
    let sections = split_sections(src)?;
    if sections.goal.trim().is_empty() {
        return Err(bad(
            "`# Intent` body is empty (state the objective in one sentence)",
        ));
    }

    let find = |name: &str| -> Option<&Vec<String>> {
        sections
            .named
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v)
    };

    // `## Allowed` is REQUIRED — no ceiling ⇒ fail closed (never allow-all).
    let allowed = find("allowed")
        .ok_or_else(|| bad("missing `## Allowed` (state the caps you permit; no default-allow)"))?;
    let cap_ceiling = parse_allowed(allowed)?;

    let inputs = find("inputs").map(|b| bullets(b)).unwrap_or_default();
    let outputs = find("outputs").map(|b| bullets(b)).unwrap_or_default();
    let budget = match find("budget") {
        Some(b) => parse_budget(b)?,
        None => Budget {
            calls: 0,
            tokens: 0,
            cost_micro: 0,
        },
    };
    let seed = match find("seed") {
        Some(b) => {
            let one = bullets(b);
            match one.first() {
                Some(s) => s
                    .parse::<u64>()
                    .map_err(|_| bad(format!("`## Seed`: `{s}` is not a u64")))?,
                None => 42,
            }
        }
        None => 42,
    };

    Ok(Intent {
        goal: sections.goal.trim().to_string(),
        inputs,
        outputs,
        cap_ceiling,
        budget,
        seed,
        fenced_axon: sections.fenced_axon,
    })
}

// ── small pure helpers ───────────────────────────────────────────────────────

/// Reject path-traversal prefixes (`..` component) — fail closed (mirrors E1001
/// and R21's manifest validation).
fn validate_prefixes(prefixes: Vec<String>, axis: &str) -> Result<Vec<String>, Refusal> {
    let mut out = Vec::new();
    for p in prefixes {
        if p.is_empty() {
            continue;
        }
        if p.split(['/', '\\']).any(|c| c == "..") {
            return Err(bad(format!(
                "`## Allowed` {axis}: path `{p}` contains a `..` component (traversal denied)"
            )));
        }
        out.push(p);
    }
    Ok(out)
}

fn nonneg(n: i64, what: &str) -> Result<i64, Refusal> {
    if n < 0 {
        Err(bad(format!("`## Budget` {what} must be \u{2265} 0")))
    } else {
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

## Seed
- 42
"#;

    #[test]
    fn parses_the_valid_example() {
        let i = parse(VALID).expect("valid intent parses");
        assert!(i.goal.starts_with("Summarize"));
        assert_eq!(i.inputs, vec!["./data/report.txt".to_string()]);
        assert_eq!(i.outputs, vec!["./out/summary.txt".to_string()]);
        assert_eq!(i.cap_ceiling.fs_read, vec!["./data/".to_string()]);
        assert_eq!(i.cap_ceiling.fs_write, vec!["./out/".to_string()]);
        assert!(i.cap_ceiling.net.is_empty());
        assert_eq!(i.cap_ceiling.exec, ExecPolicy::None);
        assert_eq!(i.cap_ceiling.max_label, Label::Internal);
        assert_eq!(i.budget.tokens, 50000);
        assert_eq!(i.seed, 42);
        assert!(i.fenced_axon.is_none());
    }

    // R22 §7 `intent_rejects_malformed`: each named negative case.
    #[test]
    fn intent_rejects_malformed() {
        // no `# Intent`
        assert!(matches!(
            parse("## Allowed\n- exec: none\n"),
            Err(Refusal::Malformed { .. })
        ));
        // no `## Allowed` (fail-closed: must state permitted caps)
        let no_allowed: String = VALID
            .lines()
            .take_while(|l| !l.starts_with("## Allowed"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(parse(&no_allowed), Err(Refusal::Malformed { .. })));
        // `..` traversal in a cap path
        assert!(matches!(
            parse(&VALID.replace("./data/", "../etc/")),
            Err(Refusal::Malformed { .. })
        ));
        // bad exec
        assert!(matches!(
            parse(&VALID.replace("exec: none", "exec: root")),
            Err(Refusal::Malformed { .. })
        ));
        // negative budget (`50000` is unique to the tokens line)
        assert!(matches!(
            parse(&VALID.replace("50000", "-1")),
            Err(Refusal::Malformed { .. })
        ));
        // bad max_label
        assert!(matches!(
            parse(&VALID.replace("max_label: internal", "max_label: ultra")),
            Err(Refusal::Malformed { .. })
        ));
    }

    #[test]
    fn lifts_a_fenced_axon_block() {
        let src = format!("{VALID}\n## Program\n```axon\nfn main() -> i64 {{ 0 }}\n```\n");
        let i = parse(&src).expect("parses with a fenced block");
        let block = i.fenced_axon.expect("the axon block is lifted");
        assert!(block.contains("fn main()"));
    }

    #[test]
    fn net_ceiling_can_list_hosts() {
        let i = parse(&VALID.replace("net: none", "net: api.example.com, *.trusted.io"))
            .expect("parses host list");
        assert_eq!(
            i.cap_ceiling.net,
            vec!["api.example.com".to_string(), "*.trusted.io".to_string()]
        );
    }
}
