//! R21 §3.1 — the `.axjob` job manifest: parse + validate. Pure (no I/O).
//!
//! Hand-rolled parser for exactly the flat `.axjob` schema (top-level keys +
//! `[grant]` + `[grant.budget]`), per the spec's "no new heavy deps" rule. A
//! malformed manifest fails closed → `Verdict::Malformed` (exit 2). The program
//! path is resolved relative to the manifest dir but NOT stat'd here (existence
//! is an I/O check the supervisor/runtime performs in a later slice).

use crate::grant::{Budget, ExecPolicy, Grant, Label};
use crate::verdict::Verdict;
use std::path::{Path, PathBuf};

/// A parsed, validated job manifest (R21 §3.1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobManifest {
    pub program: PathBuf,
    pub intent: String,
    pub seed: u64,
    pub grant: Grant,
}

/// Convenience for a `Malformed` verdict carrying a reason.
fn bad(reason: impl Into<String>) -> Verdict {
    Verdict::Malformed {
        reason: reason.into(),
    }
}

/// Parse + validate `.axjob` source. `base_dir` is the directory the manifest
/// lives in; the `program` field is resolved relative to it (pure path join).
pub fn parse(src: &str, base_dir: &Path) -> Result<JobManifest, Verdict> {
    // ── tokenize into (section, key, raw_value) lines ───────────────────────
    let mut section = String::new();
    let mut program: Option<String> = None;
    let mut intent: Option<String> = None;
    let mut seed: Option<u64> = None;
    let mut fs_read: Option<Vec<String>> = None;
    let mut fs_write: Option<Vec<String>> = None;
    let mut net: Option<Vec<String>> = None;
    let mut exec: Option<ExecPolicy> = None;
    let mut max_label: Option<Label> = None;
    let mut calls: Option<i64> = None;
    let mut tokens: Option<i64> = None;
    let mut cost_micro: Option<i64> = None;

    for (lineno, raw) in src.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(sec) = line.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
            section = sec.trim().to_string();
            continue;
        }
        let (key, val) = line
            .split_once('=')
            .ok_or_else(|| bad(format!("line {}: expected `key = value`", lineno + 1)))?;
        let (key, val) = (key.trim(), val.trim());
        let where_ = || format!("line {} (`{key}`)", lineno + 1);

        match (section.as_str(), key) {
            ("", "program") => program = Some(parse_str(val).ok_or_else(|| bad(where_()))?),
            ("", "intent") => intent = Some(parse_str(val).ok_or_else(|| bad(where_()))?),
            ("", "seed") => {
                seed = Some(
                    parse_int(val)
                        .ok_or_else(|| bad(where_()))?
                        .try_into()
                        .map_err(|_| {
                            bad(format!("{}: seed must be a non-negative u64", where_()))
                        })?,
                )
            }
            ("grant", "fs_read") => fs_read = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            ("grant", "fs_write") => fs_write = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            ("grant", "net") => net = Some(parse_arr(val).ok_or_else(|| bad(where_()))?),
            ("grant", "exec") => {
                let s = parse_str(val).ok_or_else(|| bad(where_()))?;
                exec = Some(ExecPolicy::parse(&s).ok_or_else(|| {
                    bad(format!("{}: exec must be \"none\" or \"any\"", where_()))
                })?)
            }
            ("grant", "max_label") => {
                let s = parse_str(val).ok_or_else(|| bad(where_()))?;
                max_label = Some(Label::parse(&s).ok_or_else(|| {
                    bad(format!(
                        "{}: max_label must be public|internal|secret",
                        where_()
                    ))
                })?)
            }
            ("grant.budget", "calls") => calls = Some(parse_int(val).ok_or_else(|| bad(where_()))?),
            ("grant.budget", "tokens") => {
                tokens = Some(parse_int(val).ok_or_else(|| bad(where_()))?)
            }
            ("grant.budget", "cost_micro") => {
                cost_micro = Some(parse_int(val).ok_or_else(|| bad(where_()))?)
            }
            (sec, k) => {
                return Err(bad(format!(
                    "{}: unknown key `{k}` in section `[{sec}]`",
                    where_()
                )))
            }
        }
    }

    // ── required fields ─────────────────────────────────────────────────────
    let program = program.ok_or_else(|| bad("missing `program`"))?;
    if !program.ends_with(".ax") {
        return Err(bad("`program` must be a .ax file"));
    }
    let intent = intent.unwrap_or_default();
    let seed = seed.unwrap_or(42);
    let exec = exec.ok_or_else(|| bad("missing `grant.exec`"))?;
    let max_label = max_label.ok_or_else(|| bad("missing `grant.max_label`"))?;
    let fs_read = validate_prefixes(fs_read.unwrap_or_default(), "fs_read")?;
    let fs_write = validate_prefixes(fs_write.unwrap_or_default(), "fs_write")?;
    let net = net.unwrap_or_default();

    let budget = Budget {
        calls: nonneg(calls.unwrap_or(0), "budget.calls")?,
        tokens: nonneg(tokens.unwrap_or(0), "budget.tokens")?,
        cost_micro: nonneg(cost_micro.unwrap_or(0), "budget.cost_micro")?,
    };

    Ok(JobManifest {
        program: base_dir.join(program),
        intent,
        seed,
        grant: Grant {
            fs_read,
            fs_write,
            net,
            exec,
            max_label,
            budget,
        },
    })
}

/// Serialize a manifest back to `.axjob`, with `program` as its (already
/// resolved, absolute) path — so a saved copy re-parses to the SAME program
/// regardless of the directory it is later read from (deterministic replay).
/// Lossy on `intent` only (quotes → apostrophes); intent is not hashed.
pub fn to_axjob(m: &JobManifest) -> String {
    let g = &m.grant;
    let list = |xs: &[String]| {
        xs.iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "program = \"{}\"\nintent = \"{}\"\nseed = {}\n[grant]\nfs_read = [{}]\nfs_write = [{}]\nnet = [{}]\nexec = \"{}\"\nmax_label = \"{}\"\n[grant.budget]\ncalls = {}\ntokens = {}\ncost_micro = {}\n",
        m.program.display(),
        m.intent.replace('"', "'"),
        m.seed,
        list(&g.fs_read),
        list(&g.fs_write),
        list(&g.net),
        g.exec.as_str(),
        g.max_label.as_str(),
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
    )
}

// ── small pure helpers ──────────────────────────────────────────────────────

fn strip_comment(line: &str) -> &str {
    // A `#` outside a quoted string starts a comment. The schema has no `#`
    // inside string values in practice; honor quotes defensively.
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn parse_str(val: &str) -> Option<String> {
    let v = val.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Some(v[1..v.len() - 1].to_string())
    } else {
        None
    }
}

fn parse_int(val: &str) -> Option<i64> {
    val.trim().parse::<i64>().ok()
}

fn parse_arr(val: &str) -> Option<Vec<String>> {
    let v = val.trim();
    let inner = v.strip_prefix('[')?.strip_suffix(']')?.trim();
    if inner.is_empty() {
        return Some(Vec::new());
    }
    let mut out = Vec::new();
    for elem in inner.split(',') {
        out.push(parse_str(elem.trim())?);
    }
    Some(out)
}

/// Reject path-traversal prefixes (`..` component) — fail closed (mirrors E1001).
fn validate_prefixes(prefixes: Vec<String>, axis: &str) -> Result<Vec<String>, Verdict> {
    for p in &prefixes {
        if p.is_empty() {
            return Err(bad(format!("{axis}: empty path prefix")));
        }
        if p.split(['/', '\\']).any(|c| c == "..") {
            return Err(bad(format!(
                "{axis}: path `{p}` contains a `..` component (traversal denied)"
            )));
        }
    }
    Ok(prefixes)
}

fn nonneg(n: i64, what: &str) -> Result<i64, Verdict> {
    if n < 0 {
        Err(bad(format!("{what} must be \u{2265} 0")))
    } else {
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
program = "summarize.ax"
intent  = "Summarize ./data/report.txt into ./out/; no network."
seed    = 42
[grant]
fs_read   = ["./data/"]
fs_write  = ["./out/"]
net       = []
exec      = "none"
max_label = "internal"
[grant.budget]
calls       = 100
tokens      = 50000
cost_micro  = 1000000
"#;

    fn p(src: &str) -> Result<JobManifest, Verdict> {
        parse(src, Path::new("/jobs"))
    }

    #[test]
    fn parses_the_valid_example() {
        let m = p(VALID).expect("valid manifest parses");
        assert_eq!(m.program, Path::new("/jobs/summarize.ax"));
        assert_eq!(m.seed, 42);
        assert_eq!(m.grant.fs_read, vec!["./data/".to_string()]);
        assert_eq!(m.grant.fs_write, vec!["./out/".to_string()]);
        assert!(m.grant.net.is_empty());
        assert_eq!(m.grant.exec, ExecPolicy::None);
        assert_eq!(m.grant.max_label, Label::Internal);
        assert_eq!(m.grant.budget.tokens, 50000);
        assert!(m.intent.starts_with("Summarize"));
    }

    // The named negative cases from R21 §7 `manifest_rejects_malformed`.
    #[test]
    fn manifest_rejects_malformed() {
        // non-.ax program
        assert!(matches!(
            p(&VALID.replace("summarize.ax", "summarize.py")),
            Err(Verdict::Malformed { .. })
        ));
        // bad exec
        assert!(matches!(
            p(&VALID.replace("\"none\"", "\"root\"")),
            Err(Verdict::Malformed { .. })
        ));
        // path traversal in fs_write
        assert!(matches!(
            p(&VALID.replace("\"./out/\"", "\"../etc/\"")),
            Err(Verdict::Malformed { .. })
        ));
        // negative budget (`50000` appears only on the tokens line)
        assert!(matches!(
            p(&VALID.replace("50000", "-1")),
            Err(Verdict::Malformed { .. })
        ));
        // missing program
        let no_prog: String = VALID
            .lines()
            .filter(|l| !l.trim_start().starts_with("program"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(matches!(p(&no_prog), Err(Verdict::Malformed { .. })));
        // bad max_label
        assert!(matches!(
            p(&VALID.replace("\"internal\"", "\"ultra\"")),
            Err(Verdict::Malformed { .. })
        ));
        // unknown key
        assert!(matches!(
            p(&format!("{VALID}\nmystery = \"x\"")),
            Err(Verdict::Malformed { .. })
        ));
    }

    #[test]
    fn empty_grant_and_defaults() {
        let src = "program = \"pure.ax\"\n[grant]\nexec = \"none\"\nmax_label = \"public\"\n";
        let m = p(src).expect("empty grant ok");
        assert_eq!(m.seed, 42); // default
        assert!(m.grant.fs_read.is_empty() && m.grant.net.is_empty());
        assert_eq!(m.grant.budget.calls, 0);
    }
}
