//! R22 §3.1 — parse + validate an `.intent.md` into an `Intent`.
//!
//! The `.intent.md` is prose plus named sections (`# Intent`, `## Inputs`,
//! `## Outputs`, `## Allowed`, `## Budget`, `## Seed`, `## Examples`). The
//! `## Allowed` section is the human's explicit upper bound on authority — a
//! `CapCeiling`. A malformed intent fails closed → `Refusal::Malformed` (exit
//! 2). A MISSING `## Allowed` is fail-closed too: no ceiling ⇒ refuse (we never
//! default to "allow everything"; the human must state what they permit).
//!
//! [`parse`] is pure. [`parse_with_base`] additionally resolves `## Examples`
//! fixtures written as `from <path>` under the intent file's directory — the
//! only file reads parsing ever does (IG1).

use crate::refusal::Refusal;
use axon_os::grant::{Budget, ExecPolicy, Host, Label, PathPrefix};
use std::path::{Component, Path, PathBuf};

/// IG1: the largest fixture (input contents, expected output contents or
/// expected stdout) an example may carry, inline or from a file.
pub const FIXTURE_CAP_BYTES: usize = 64 * 1024;

/// One file of an example run: where it lives (relative to the program's
/// directory) and its exact contents.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileFixture {
    pub path: String,
    pub contents: String,
}

/// One run of the program inside an example (IG1). A stateful example has
/// two or more runs; each run sees the files the previous runs left behind.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExampleRun {
    /// Staged before the run (each path under an `fs_read` prefix).
    pub inputs: Vec<FileFixture>,
    /// Expected after the run, exact contents (each under an `fs_write` prefix).
    pub outputs: Vec<FileFixture>,
    /// Expected process exit status (default 0).
    pub exit_code: i32,
    /// Expected stdout, exact, when stated.
    pub stdout: Option<String>,
}

/// A named example: the loop's visible acceptance check (IG1).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub name: String,
    pub runs: Vec<ExampleRun>,
}

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
    /// IG1: the `## Examples` section (empty when absent).
    pub examples: Vec<Example>,
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
    /// The `## Examples` body VERBATIM with 1-based line numbers, fences and
    /// their contents included (IG1 fixtures live in ```fixture fences).
    examples: Option<Vec<(usize, String)>>,
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
    let mut examples: Option<Vec<(usize, String)>> = None;
    let mut in_examples = false;

    for (idx, raw) in src.lines().enumerate() {
        let line = raw.trim_end();
        if in_examples {
            if let Some(body) = examples.as_mut() {
                // Headers end the section below; everything else is kept.
                let header = !in_fence
                    && !line.trim_start().starts_with("```")
                    && (line.starts_with("## ") || line.starts_with("# "));
                if !header {
                    body.push((idx + 1, raw.to_string()));
                }
            }
        }
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
            let title = title.trim().to_lowercase();
            in_examples = title == "examples";
            if in_examples {
                if examples.is_some() {
                    return Err(bad(format!(
                        "line {}: a second `## Examples` section (state every example in one)",
                        idx + 1
                    )));
                }
                examples = Some(Vec::new());
            }
            named.push((title, Vec::new()));
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
            in_examples = false;
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
        examples,
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
/// Pure: an example fixture written as `from <path>` is refused here, because
/// resolving it needs the intent file's directory ([`parse_with_base`]).
pub fn parse(src: &str) -> Result<Intent, Refusal> {
    parse_with_base(src, None)
}

/// [`parse`], resolving `## Examples` fixtures written as `from <path>` under
/// `base` (the intent file's directory). Only paths inside `base` are read:
/// absolute paths, `..` components and symlinks are refused, and every fixture
/// is capped at [`FIXTURE_CAP_BYTES`].
pub fn parse_with_base(src: &str, base: Option<&Path>) -> Result<Intent, Refusal> {
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

    let examples = match &sections.examples {
        Some(body) => parse_examples(body, &cap_ceiling, base)?,
        None => Vec::new(),
    };

    Ok(Intent {
        goal: sections.goal.trim().to_string(),
        inputs,
        outputs,
        cap_ceiling,
        budget,
        seed,
        fenced_axon: sections.fenced_axon,
        examples,
    })
}

// ── IG1: `## Examples` ───────────────────────────────────────────────────────

/// Where a fixture's bytes come from, before resolution.
enum FixtureSrc {
    /// The next lines are a ```fixture fence.
    Fence,
    /// A JSON string literal on the same line (exact bytes, any content).
    Json(String),
    /// A file under the intent's directory.
    File(String),
}

fn line_bad(line: usize, reason: impl std::fmt::Display) -> Refusal {
    bad(format!("`## Examples` line {line}: {reason}"))
}

/// Split `<path>: …` / `<path> from <rel>` after the `input`/`output` keyword.
fn fixture_head(rest: &str, line: usize) -> Result<(String, FixtureSrc), Refusal> {
    if let Some((path, rel)) = rest.split_once(" from ") {
        return Ok((
            path.trim().to_string(),
            FixtureSrc::File(rel.trim().to_string()),
        ));
    }
    let Some((path, tail)) = rest.split_once(':') else {
        return Err(line_bad(
            line,
            format!("expected `<path>:` (then a ```fixture fence), `<path>: \"json\"` or `<path> from <file>`, got `{rest}`"),
        ));
    };
    let tail = tail.trim();
    let src = if tail.is_empty() {
        FixtureSrc::Fence
    } else {
        FixtureSrc::Json(tail.to_string())
    };
    Ok((path.trim().to_string(), src))
}

/// Normalise a RELATIVE example path to its components: `./out//x` and
/// `out/x` are the same file. `None` for an absolute path, a `..` component,
/// a backslash or an empty path.
fn rel_components(p: &str) -> Option<Vec<&str>> {
    if p.starts_with('/') || p.contains('\\') {
        return None;
    }
    let comps: Vec<&str> = p
        .split('/')
        .filter(|c| !c.is_empty() && *c != ".")
        .collect();
    if comps.is_empty() || comps.contains(&"..") {
        return None;
    }
    Some(comps)
}

/// Is `path` (relative) under one of `prefixes`, component-wise?
fn under_any(path: &[&str], prefixes: &[String]) -> bool {
    prefixes.iter().any(|pre| match rel_components(pre) {
        Some(pc) => path.len() >= pc.len() && path[..pc.len()] == pc[..],
        None => false,
    })
}

/// Check an example file path against the ceiling axis it must lie under.
fn example_path(
    path: &str,
    prefixes: &[String],
    axis: &str,
    line: usize,
) -> Result<String, Refusal> {
    let comps = rel_components(path).ok_or_else(|| {
        line_bad(
            line,
            format!("`{path}` must be a relative path without `..` (it is staged in the program's directory)"),
        )
    })?;
    if !under_any(&comps, prefixes) {
        return Err(line_bad(
            line,
            format!(
                "`{path}` is not under any `## Allowed` {axis} prefix ({})",
                if prefixes.is_empty() {
                    "none".to_string()
                } else {
                    prefixes.join(", ")
                }
            ),
        ));
    }
    Ok(path.to_string())
}

fn cap_check(contents: String, what: &str, line: usize) -> Result<String, Refusal> {
    if contents.len() > FIXTURE_CAP_BYTES {
        return Err(line_bad(
            line,
            format!(
                "{what} is {} bytes, over the {FIXTURE_CAP_BYTES}-byte fixture cap",
                contents.len()
            ),
        ));
    }
    Ok(contents)
}

/// Read a `from <rel>` fixture under `base`, never outside it: `rel` must be
/// relative without `..`, no component may be a symlink, the target must be a
/// regular file within the cap.
fn read_fixture_file(rel: &str, base: Option<&Path>, line: usize) -> Result<String, Refusal> {
    let Some(base) = base else {
        return Err(line_bad(
            line,
            format!(
                "`from {rel}` needs the intent file's directory; parse the intent from its file"
            ),
        ));
    };
    let rel_path = Path::new(rel);
    if rel.is_empty()
        || rel_path.is_absolute()
        || rel_path
            .components()
            .any(|c| !matches!(c, Component::Normal(_) | Component::CurDir))
    {
        return Err(line_bad(
            line,
            format!("fixture file `{rel}` must be a relative path inside the intent's directory (no absolute path, no `..`)"),
        ));
    }
    let mut cur: PathBuf = base.to_path_buf();
    for c in rel_path.components() {
        let Component::Normal(name) = c else { continue };
        cur.push(name);
        let meta = std::fs::symlink_metadata(&cur)
            .map_err(|e| line_bad(line, format!("fixture file `{rel}`: {e}")))?;
        if meta.file_type().is_symlink() {
            return Err(line_bad(
                line,
                format!(
                    "fixture file `{rel}`: `{}` is a symlink (refused)",
                    cur.display()
                ),
            ));
        }
    }
    let meta = std::fs::symlink_metadata(&cur)
        .map_err(|e| line_bad(line, format!("fixture file `{rel}`: {e}")))?;
    if !meta.is_file() {
        return Err(line_bad(
            line,
            format!("fixture file `{rel}` is not a regular file"),
        ));
    }
    if meta.len() > FIXTURE_CAP_BYTES as u64 {
        return Err(line_bad(
            line,
            format!(
                "fixture file `{rel}` is {} bytes, over the {FIXTURE_CAP_BYTES}-byte fixture cap",
                meta.len()
            ),
        ));
    }
    let bytes =
        std::fs::read(&cur).map_err(|e| line_bad(line, format!("fixture file `{rel}`: {e}")))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| line_bad(line, format!("fixture file `{rel}` is not UTF-8")))?;
    cap_check(text, &format!("fixture file `{rel}`"), line)
}

/// Parse the `## Examples` body (IG1). Grammar, one item per line:
///
/// ```text
/// ### <example name>             starts an example
/// #### run <anything>            starts a run (optional for a one-run example)
/// - input <path>:                then a ```fixture fence: the file's contents
/// - input <path>: "<json>"       contents as one JSON string (exact bytes)
/// - input <path> from <file>     contents of <file> under the intent's directory
/// - output <path>: … / from …    expected file contents after the run (exact)
/// - stdout: … / from …           expected stdout (exact)
/// - exit: <n>                    expected exit status (default 0)
/// ```
///
/// A fence's contents are its lines, each ending in `\n`.
fn parse_examples(
    body: &[(usize, String)],
    ceiling: &CapCeiling,
    base: Option<&Path>,
) -> Result<Vec<Example>, Refusal> {
    // Which slot a pending fence fills.
    enum Slot {
        Input(String),
        Output(String),
        Stdout,
    }
    let mut examples: Vec<Example> = Vec::new();
    let mut exit_set: Vec<bool> = Vec::new(); // per run of the current example
    let mut pending: Option<(Slot, usize)> = None;
    let mut i = 0;

    fn cur_run<'a>(
        examples: &'a mut [Example],
        exit_set: &mut Vec<bool>,
        line: usize,
    ) -> Result<&'a mut ExampleRun, Refusal> {
        let ex = examples
            .last_mut()
            .ok_or_else(|| line_bad(line, "an item before the first `### <example name>`"))?;
        if ex.runs.is_empty() {
            ex.runs.push(ExampleRun::default());
            exit_set.push(false);
        }
        Ok(ex.runs.last_mut().expect("a run exists"))
    }

    while i < body.len() {
        let (line, raw) = (&body[i].0, &body[i].1);
        let line = *line;
        let t = raw.trim();
        i += 1;

        if let Some(info) = t.strip_prefix("```") {
            let Some((slot, owner)) = pending.take() else {
                return Err(line_bad(
                    line,
                    "a fence with no `- input|output|stdout …:` item before it",
                ));
            };
            if info.trim() != "fixture" {
                return Err(line_bad(
                    line,
                    format!(
                        "fixture contents go in a ```fixture fence, not ```{} (an ```axon fence is lifted as the program)",
                        info.trim()
                    ),
                ));
            }
            let mut contents = String::new();
            let mut closed = false;
            while i < body.len() {
                let (_, l) = &body[i];
                i += 1;
                if l.trim_start().starts_with("```") {
                    closed = true;
                    break;
                }
                contents.push_str(l);
                contents.push('\n');
            }
            if !closed {
                return Err(line_bad(line, "unclosed ```fixture fence"));
            }
            let contents = cap_check(contents, "fixture", line)?;
            let run = cur_run(&mut examples, &mut exit_set, owner)?;
            match slot {
                Slot::Input(path) => run.inputs.push(FileFixture { path, contents }),
                Slot::Output(path) => run.outputs.push(FileFixture { path, contents }),
                Slot::Stdout => run.stdout = Some(contents),
            }
            continue;
        }
        if t.is_empty() {
            continue;
        }
        if let Some((_, owner)) = &pending {
            return Err(line_bad(
                line,
                format!("line {owner} ends in `:` but is not followed by a ```fixture fence"),
            ));
        }
        if let Some(name) = t.strip_prefix("#### ") {
            if !name.trim().to_lowercase().starts_with("run") {
                return Err(line_bad(line, format!("expected `#### run …`, got `{t}`")));
            }
            let ex = examples.last_mut().ok_or_else(|| {
                line_bad(line, "`#### run` before the first `### <example name>`")
            })?;
            ex.runs.push(ExampleRun::default());
            exit_set.push(false);
            continue;
        }
        if let Some(name) = t.strip_prefix("### ") {
            let name = name.trim().to_string();
            if name.is_empty() {
                return Err(line_bad(line, "an example needs a name"));
            }
            if examples.iter().any(|e| e.name == name) {
                return Err(line_bad(line, format!("duplicate example name `{name}`")));
            }
            if let Some(prev) = examples.last() {
                if prev.runs.is_empty() {
                    return Err(bad(format!(
                        "`## Examples`: example `{}` has no runs",
                        prev.name
                    )));
                }
            }
            examples.push(Example {
                name,
                runs: Vec::new(),
            });
            exit_set.clear();
            continue;
        }
        let Some(item) = t.strip_prefix("- ").or_else(|| t.strip_prefix("* ")) else {
            return Err(line_bad(
                line,
                format!("expected `### <name>`, `#### run`, or a `- input|output|stdout|exit` item, got `{t}`"),
            ));
        };
        let item = item.trim();
        let (slot, src) = if let Some(rest) = item.strip_prefix("input ") {
            let (path, src) = fixture_head(rest, line)?;
            let path = example_path(&path, &ceiling.fs_read, "fs_read", line)?;
            (Slot::Input(path), src)
        } else if let Some(rest) = item.strip_prefix("output ") {
            let (path, src) = fixture_head(rest, line)?;
            let path = example_path(&path, &ceiling.fs_write, "fs_write", line)?;
            (Slot::Output(path), src)
        } else if let Some(rest) = item.strip_prefix("stdout") {
            let rest = rest.trim_start();
            let src = if let Some(rel) = rest.strip_prefix("from ") {
                FixtureSrc::File(rel.trim().to_string())
            } else if let Some(tail) = rest.strip_prefix(':') {
                let tail = tail.trim();
                if tail.is_empty() {
                    FixtureSrc::Fence
                } else {
                    FixtureSrc::Json(tail.to_string())
                }
            } else {
                return Err(line_bad(line, format!("expected `- stdout:`, `- stdout: \"json\"` or `- stdout from <file>`, got `{t}`")));
            };
            (Slot::Stdout, src)
        } else if let Some(rest) = item.strip_prefix("exit:") {
            let n: i32 = rest
                .trim()
                .parse()
                .ok()
                .filter(|n| (0..=255).contains(n))
                .ok_or_else(|| {
                    line_bad(
                        line,
                        format!("exit must be an integer 0..=255, got `{}`", rest.trim()),
                    )
                })?;
            let run = cur_run(&mut examples, &mut exit_set, line)?;
            run.exit_code = n;
            let set = exit_set.last_mut().expect("cur_run pushed a run");
            if *set {
                return Err(line_bad(line, "a second `exit:` in one run"));
            }
            *set = true;
            continue;
        } else {
            return Err(line_bad(
                line,
                format!("unknown example item `{item}` (expected input, output, stdout or exit)"),
            ));
        };
        // Duplicates within one run are a contradiction, not an override.
        {
            let run = cur_run(&mut examples, &mut exit_set, line)?;
            let dup = match &slot {
                Slot::Input(p) => run.inputs.iter().any(|f| same_file(&f.path, p)),
                Slot::Output(p) => run.outputs.iter().any(|f| same_file(&f.path, p)),
                Slot::Stdout => run.stdout.is_some(),
            };
            if dup {
                return Err(line_bad(
                    line,
                    format!("`{item}` repeats an item of the same run"),
                ));
            }
        }
        let contents = match src {
            FixtureSrc::Fence => {
                pending = Some((slot, line));
                continue;
            }
            FixtureSrc::Json(j) => {
                let s: String = serde_json::from_str(&j).map_err(|e| {
                    line_bad(line, format!("`{j}` is not a JSON string literal: {e}"))
                })?;
                cap_check(s, "fixture", line)?
            }
            FixtureSrc::File(rel) => read_fixture_file(&rel, base, line)?,
        };
        let run = cur_run(&mut examples, &mut exit_set, line)?;
        match slot {
            Slot::Input(path) => run.inputs.push(FileFixture { path, contents }),
            Slot::Output(path) => run.outputs.push(FileFixture { path, contents }),
            Slot::Stdout => run.stdout = Some(contents),
        }
    }
    if let Some((_, owner)) = pending {
        return Err(line_bad(
            owner,
            "ends in `:` but no ```fixture fence follows",
        ));
    }
    if let Some(last) = examples.last() {
        if last.runs.is_empty() {
            return Err(bad(format!(
                "`## Examples`: example `{}` has no runs",
                last.name
            )));
        }
    }
    if examples.is_empty() {
        return Err(bad(
            "`## Examples` is present but states no `### <example name>` (omit the section instead)",
        ));
    }
    Ok(examples)
}

/// Two example paths name the same file (`./out/x` = `out//x`).
pub fn same_file(a: &str, b: &str) -> bool {
    match (rel_components(a), rel_components(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// The relative path of an example file with `.`/empty components removed
/// (`./out//x` → `out/x`), or `None` for a path parsing would have refused.
pub fn normalized_example_path(p: &str) -> Option<String> {
    rel_components(p).map(|c| c.join("/"))
}

/// Render examples back to the `## Examples` grammar [`parse_examples`] reads
/// (round-trips; also the form the generation prompt shows the model).
pub fn render_examples(examples: &[Example]) -> String {
    fn content(out: &mut String, head: &str, contents: &str) {
        let fence_ok = (contents.is_empty() || contents.ends_with('\n'))
            && !contents.lines().any(|l| l.trim_start().starts_with("```"));
        if fence_ok {
            out.push_str(&format!("{head}:\n```fixture\n{contents}```\n"));
        } else {
            let json = serde_json::to_string(contents).expect("a string serialises");
            out.push_str(&format!("{head}: {json}\n"));
        }
    }
    let mut out = String::from("## Examples\n");
    for ex in examples {
        out.push_str(&format!("\n### {}\n", ex.name));
        for (n, run) in ex.runs.iter().enumerate() {
            out.push_str(&format!("#### run {}\n", n + 1));
            for f in &run.inputs {
                content(&mut out, &format!("- input {}", f.path), &f.contents);
            }
            for f in &run.outputs {
                content(&mut out, &format!("- output {}", f.path), &f.contents);
            }
            if let Some(s) = &run.stdout {
                content(&mut out, "- stdout", s);
            }
            out.push_str(&format!("- exit: {}\n", run.exit_code));
        }
    }
    out
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

    // ── IG1: `## Examples` ──────────────────────────────────────────────────

    const EXAMPLES: &str = r#"
## Examples

### counts lines
- input ./data/report.txt:
```fixture
a
## not a header inside a fence
b
```
- output out/summary.txt: "2 lines"
- stdout: "done\n"

### keeps a running total
#### run 1
- input ./data/report.txt: "1"
- output ./out/summary.txt: "1"
#### run 2
- input ./data/report.txt: "2"
- output ./out//summary.txt: "3"
- exit: 3
"#;

    fn with_examples(ex: &str) -> String {
        format!("{VALID}{ex}")
    }

    fn malformed_reason(src: &str) -> String {
        match parse(src) {
            Err(Refusal::Malformed { reason }) => reason,
            other => panic!("expected Malformed, got {other:?}"),
        }
    }

    #[test]
    fn examples_parse_and_round_trip_including_a_two_run_example() {
        let i = parse(&with_examples(EXAMPLES)).expect("examples parse");
        assert_eq!(i.examples.len(), 2);
        let one = &i.examples[0];
        assert_eq!(one.name, "counts lines");
        assert_eq!(one.runs.len(), 1);
        assert_eq!(
            one.runs[0].inputs,
            vec![FileFixture {
                path: "./data/report.txt".into(),
                contents: "a\n## not a header inside a fence\nb\n".into(),
            }]
        );
        assert_eq!(one.runs[0].outputs[0].contents, "2 lines");
        assert_eq!(one.runs[0].stdout.as_deref(), Some("done\n"));
        assert_eq!(one.runs[0].exit_code, 0);
        let two = &i.examples[1];
        assert_eq!(two.runs.len(), 2);
        assert_eq!(two.runs[1].exit_code, 3);
        assert_eq!(two.runs[1].outputs[0].contents, "3");
        // The header-looking fixture line did not end the section or the intent.
        assert_eq!(i.seed, 42);

        let rendered = render_examples(&i.examples);
        let again = parse(&with_examples(&format!("\n{rendered}"))).expect("rendered parses");
        assert_eq!(again.examples, i.examples, "render → parse is the identity");
    }

    #[test]
    fn an_intent_without_examples_still_parses() {
        assert!(parse(VALID).unwrap().examples.is_empty());
    }

    #[test]
    fn example_paths_outside_the_ceiling_or_escaping_are_refused() {
        // ATTACK: an example path escaping the ceiling parsed clean
        for (path, why) in [
            ("/etc/passwd", "relative path"),
            ("../data/x", "relative path"),
            ("./data/../out/x", "relative path"),
            ("data\\x", "relative path"),
            ("./elsewhere/x", "fs_read prefix"),
            ("./out/x", "fs_read prefix"),
        ] {
            let src = with_examples(&format!("\n## Examples\n### e\n- input {path}: \"x\"\n"));
            let r = malformed_reason(&src);
            assert!(
                r.contains(why) && r.contains("line "),
                "ATTACK: an example path escaping the ceiling parsed clean: `{path}` → {r}"
            );
        }
        // An output must lie under fs_write, not fs_read.
        let r = malformed_reason(&with_examples(
            "\n## Examples\n### e\n- output ./data/x: \"x\"\n",
        ));
        assert!(r.contains("fs_write prefix"), "{r}");
    }

    #[test]
    fn fixture_files_resolve_only_inside_the_intent_directory() {
        // ATTACK: a fixture file outside the intent directory was read
        let dir = std::env::temp_dir().join(format!("ig1-fixtures-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("fx")).unwrap();
        std::fs::write(dir.join("fx/in.txt"), "inside\n").unwrap();
        let outside = dir.with_extension("outside");
        std::fs::write(&outside, "SECRET\n").unwrap();
        std::os::unix::fs::symlink(&outside, dir.join("fx/link.txt")).unwrap();
        std::os::unix::fs::symlink(dir.join("fx"), dir.join("fxdir")).unwrap();
        std::fs::write(dir.join("fx/big.txt"), "x".repeat(FIXTURE_CAP_BYTES + 1)).unwrap();

        let ok = with_examples("\n## Examples\n### e\n- input ./data/a from fx/in.txt\n");
        let i = parse_with_base(&ok, Some(&dir)).expect("an in-directory fixture resolves");
        assert_eq!(i.examples[0].runs[0].inputs[0].contents, "inside\n");
        // Pure `parse` reads no file at all.
        assert!(malformed_reason(&ok).contains("intent file's directory"));

        let abs = format!("{}", outside.display());
        for rel in [
            abs.as_str(),
            "../x",
            "fx/../fx/in.txt",
            "fx/link.txt",
            "fxdir/in.txt",
        ] {
            let src = with_examples(&format!(
                "\n## Examples\n### e\n- input ./data/a from {rel}\n"
            ));
            match parse_with_base(&src, Some(&dir)) {
                Err(Refusal::Malformed { reason }) => assert!(
                    !reason.contains("SECRET") && reason.contains("line "),
                    "{reason}"
                ),
                other => panic!(
                    "ATTACK: a fixture file outside the intent directory was read: `{rel}` → {other:?}"
                ),
            }
        }
        let big = with_examples("\n## Examples\n### e\n- input ./data/a from fx/big.txt\n");
        match parse_with_base(&big, Some(&dir)) {
            Err(Refusal::Malformed { reason }) => {
                assert!(reason.contains("fixture cap"), "{reason}")
            }
            other => panic!("a fixture over the cap must be refused: {other:?}"),
        }
        let _ = std::fs::remove_dir_all(&dir);
        let _ = std::fs::remove_file(&outside);
    }

    #[test]
    fn an_inline_fixture_over_the_cap_is_refused() {
        // ATTACK: a fixture over the cap parsed clean
        let big = "y\n".repeat(FIXTURE_CAP_BYTES / 2 + 1);
        let src = with_examples(&format!(
            "\n## Examples\n### e\n- input ./data/a:\n```fixture\n{big}```\n"
        ));
        let r = malformed_reason(&src);
        assert!(
            r.contains("fixture cap"),
            "ATTACK: a fixture over the cap parsed clean: {r}"
        );
    }

    #[test]
    fn malformed_examples_name_the_offending_line() {
        for (body, needle) in [
            (
                "### e\n- input ./data/a:\n```axon\nfn main() -> i64 { 0 }\n```\n",
                "```fixture",
            ),
            (
                "### e\n- input ./data/a:\n- exit: 0\n",
                "not followed by a ```fixture",
            ),
            ("### e\n- exit: 300\n", "0..=255"),
            ("### e\n- exit: 1\n- exit: 2\n", "second `exit:`"),
            ("- exit: 0\n", "before the first"),
            ("### e\n- frobnicate x\n", "unknown example item"),
            ("### e\n- input ./data/a: notjson\n", "JSON string"),
            (
                "### e\n- input ./data/a: \"1\"\n- input data/a: \"2\"\n",
                "repeats",
            ),
            ("### e\n#### step\n", "#### run"),
        ] {
            let r = malformed_reason(&with_examples(&format!("\n## Examples\n{body}")));
            assert!(r.contains(needle) && r.contains("line "), "{body:?} → {r}");
        }
        for (body, needle) in [
            ("### e\n", "has no runs"),
            ("", "states no"),
            (
                "### e\n- exit: 0\n## Examples\n### f\n- exit: 0\n",
                "second `## Examples`",
            ),
        ] {
            let r = malformed_reason(&with_examples(&format!("\n## Examples\n{body}")));
            assert!(r.contains(needle), "{body:?} → {r}");
        }
    }
}
