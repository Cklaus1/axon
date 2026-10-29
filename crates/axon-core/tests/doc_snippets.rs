//! Every ```` ```axon ```` block in the LLM-facing docs checks, and runs when it
//! is a program (docs-truth DT-7, O-GAP-09).
//!
//! A model learns Axon from these files and copies their blocks verbatim. At
//! 4a6436da 31 of 78 blocks did not pass `axon check`, including the guide's own
//! goal example (E1504). Nothing ran them, so nothing noticed.
//!
//! The fence info string carries the contract (GitHub highlights on its first
//! word, so rendering is unchanged):
//! * `axon` - a whole program. `axon check` exits 0; with `fn main(` `axon run`
//!   exits 0; with `@[test` `axon test` exits 0.
//! * `axon exits N` - as above, but `axon run` must exit exactly N (a
//!   `fn main() -> i64` example, a verify breach, a panic).
//! * `axon fragment` - statements, wrapped as `fn main() {\n<body>\n}`, then the
//!   same rules. Combines with `exits N`.
//! * `axon fails E1001 [E....]` - meant to be refused: `axon check` exits
//!   non-zero and every listed code is on an error-severity line.
//! * any other word after `axon` is an unknown marker and is reported.
//!
//! Each block runs in its own temp dir with `AXON_PATH` and every other ambient
//! `AXON_*` variable removed, `AXON_SEED=42`, `AXON_AI_MOCK=1`, `HOME` and
//! `XDG_CACHE_HOME` pointed at the temp dir, stdin null, and a 20 s deadline.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The docs a model reads first (CLAUDE.md:247-254, `spec/axon-for-llms.md`).
const DOCS: &[&str] = &[
    "spec/axon-for-llms.md",
    "spec/language-tour.md",
    "spec/stdlib.md",
    "README.md",
    "CLAUDE.md",
    "examples/asi/README.md",
];

const DEADLINE: Duration = Duration::from_secs(20);

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// One fenced block whose info string starts with `axon`.
struct Block {
    /// 1-based line of the opening fence.
    line: usize,
    /// The info string after `axon` (the marker words).
    marker: String,
    body: String,
}

/// The fenced blocks of `text` whose info word is `axon`. CommonMark fences: a
/// fence opens on 3+ backticks or tildes (after indentation) and closes only on
/// the same character, at least as many, with nothing else on the line. A
/// longer outer fence hides the blocks it quotes. The opener's indent is
/// stripped from the body (a block inside a list item).
fn axon_blocks(text: &str) -> Vec<Block> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim_start();
        let indent = line.len() - trimmed.len();
        let fence_ch = trimmed.chars().next().filter(|c| *c == '`' || *c == '~');
        let run = fence_ch.map_or(0, |c| trimmed.chars().take_while(|x| *x == c).count());
        if run < 3 {
            i += 1;
            continue;
        }
        let ch = fence_ch.unwrap();
        let info = trimmed[run..].trim();
        let mut body = Vec::new();
        let mut j = i + 1;
        while j < lines.len() {
            let t = lines[j].trim();
            let n = t.chars().take_while(|x| *x == ch).count();
            if n >= run && n == t.len() {
                break;
            }
            let l = lines[j];
            let strip = l.len() - l.trim_start_matches(' ').len();
            body.push(&l[strip.min(indent)..]);
            j += 1;
        }
        let mut words = info.split_whitespace();
        if words.next() == Some("axon") {
            out.push(Block {
                line: i + 1,
                marker: words.collect::<Vec<_>>().join(" "),
                body: body.join("\n"),
            });
        }
        i = j + 1;
    }
    out
}

/// What a marker asks for.
struct Want {
    fragment: bool,
    exits: i32,
    /// `Some(codes)` for `fails ...`.
    fails: Option<Vec<String>>,
}

fn parse_marker(marker: &str) -> Result<Want, String> {
    let mut w = Want { fragment: false, exits: 0, fails: None };
    let mut words = marker.split_whitespace().peekable();
    let mut exits_seen = false;
    while let Some(word) = words.next() {
        match word {
            "fragment" if !w.fragment => w.fragment = true,
            "exits" if !exits_seen => {
                exits_seen = true;
                w.exits = words
                    .next()
                    .and_then(|n| n.parse().ok())
                    .ok_or_else(|| format!("`exits` needs a number in `{marker}`"))?;
            }
            "fails" if w.fails.is_none() => {
                let mut codes = Vec::new();
                while let Some(c) = words.peek() {
                    if c.len() == 5 && c.starts_with('E') && c[1..].bytes().all(|b| b.is_ascii_digit())
                    {
                        codes.push(words.next().unwrap().to_string());
                    } else {
                        break;
                    }
                }
                if codes.is_empty() {
                    return Err(format!("`fails` needs at least one E-code in `{marker}`"));
                }
                w.fails = Some(codes);
            }
            other => return Err(format!("unknown marker `{other}` in `axon {marker}`")),
        }
    }
    if w.fails.is_some() && exits_seen {
        return Err(format!("`fails` and `exits` cannot combine in `axon {marker}`"));
    }
    Ok(w)
}

struct Run {
    code: Option<i32>,
    out: String,
}

/// `axon <verb> snippet.ax` in `dir`, hermetic, with a deadline.
fn axon(verb: &str, dir: &Path) -> Run {
    let out = std::fs::File::create(dir.join(format!("{verb}.out"))).unwrap();
    let err = out.try_clone().unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.args([verb, "snippet.ax"])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(Stdio::from(out))
        .stderr(Stdio::from(err));
    for (k, _) in std::env::vars_os() {
        if k.to_string_lossy().starts_with("AXON_") {
            cmd.env_remove(k);
        }
    }
    cmd.env("AXON_SEED", "42")
        .env("AXON_AI_MOCK", "1")
        .env("HOME", dir)
        .env("XDG_CACHE_HOME", dir);
    let mut child = cmd.spawn().unwrap();
    let start = Instant::now();
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break Some(s);
        }
        if start.elapsed() > DEADLINE {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let out = String::from_utf8_lossy(&std::fs::read(dir.join(format!("{verb}.out"))).unwrap())
        .into_owned();
    Run { code: status.and_then(|s| s.code()), out }
}

/// The error-severity codes in `axon check`'s NDJSON diagnostics.
fn error_codes(out: &str) -> Vec<String> {
    out.lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["severity"] == "error")
        .filter_map(|v| v["code"].as_str().map(str::to_string))
        .collect()
}

/// The first error a reader needs: an error diagnostic's code and message, or
/// the first non-diagnostic line (a run's panic text).
fn first_error(out: &str) -> String {
    for l in out.lines() {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(l) {
            if v["severity"] == "error" {
                return format!(
                    "{} {}",
                    v["code"].as_str().unwrap_or("?"),
                    v["message"].as_str().unwrap_or("")
                );
            }
        } else if !l.trim().is_empty() {
            return l.trim().to_string();
        }
    }
    String::from("(no output)")
}

fn rc(r: &Run) -> String {
    r.code.map_or_else(|| format!("killed after {DEADLINE:?}"), |c| c.to_string())
}

/// Every block of `text` that breaks its marker's contract, as
/// `doc:line axon <marker>: <verb> rc N, <first error>`.
fn audit(doc: &str, text: &str, tag: &str) -> Vec<String> {
    let mut bad = Vec::new();
    for (n, b) in axon_blocks(text).iter().enumerate() {
        let sep = if b.marker.is_empty() { "" } else { " " };
        let at = format!("{doc}:{} `axon{sep}{}`", b.line, b.marker);
        let want = match parse_marker(&b.marker) {
            Ok(w) => w,
            Err(e) => {
                bad.push(format!("{at}: {e}"));
                continue;
            }
        };
        let src = if want.fragment { format!("fn main() {{\n{}\n}}\n", b.body) } else { format!("{}\n", b.body) };
        let dir = std::env::temp_dir().join(format!(
            "axon_doc_snippets_{}_{tag}_{n}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("snippet.ax"), &src).unwrap();
        let check = axon("check", &dir);
        if let Some(codes) = &want.fails {
            let got = error_codes(&check.out);
            let missing: Vec<&String> = codes.iter().filter(|c| !got.contains(c)).collect();
            if check.code == Some(0) || !missing.is_empty() {
                bad.push(format!(
                    "{at}: check rc {}, expected a refusal with {codes:?}, missing {missing:?}; {}",
                    rc(&check),
                    first_error(&check.out)
                ));
            }
        } else if check.code != Some(0) {
            bad.push(format!("{at}: check rc {}, {}", rc(&check), first_error(&check.out)));
        } else {
            if src.contains("fn main(") {
                let run = axon("run", &dir);
                if run.code != Some(want.exits) {
                    bad.push(format!(
                        "{at}: run rc {} (expected {}), {}",
                        rc(&run),
                        want.exits,
                        first_error(&run.out)
                    ));
                }
            } else if want.exits != 0 {
                bad.push(format!("{at}: `exits {}` on a block with no `fn main(`", want.exits));
            }
            if src.contains("@[test") {
                let test = axon("test", &dir);
                if test.code != Some(0) {
                    bad.push(format!("{at}: test rc {}, {}", rc(&test), first_error(&test.out)));
                }
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
    bad
}

#[test]
fn every_axon_block_in_the_llm_facing_docs_checks() {
    let root = repo_root();
    let mut bad = Vec::new();
    let mut blocks = 0;
    for (k, doc) in DOCS.iter().enumerate() {
        let text = std::fs::read_to_string(root.join(doc))
            .unwrap_or_else(|e| panic!("cannot read {doc}: {e}"));
        blocks += axon_blocks(&text).len();
        bad.extend(audit(doc, &text, &format!("d{k}")));
    }
    assert!(blocks > 40, "the extractor found only {blocks} axon blocks in {DOCS:?}");
    assert!(
        bad.is_empty(),
        "ATTACK: a doc snippet that does not check or run was accepted.\n\
         {} block(s) break their fence marker (docs-truth DT-7; markers in this file's header):\n  {}",
        bad.len(),
        bad.join("\n  ")
    );
}

/// The marker rules, on planted blocks: each bad case must be reported and
/// each honest one must not, so a pass above is a judgement.
#[test]
fn the_marker_rules_hold_on_planted_blocks() {
    let contained = "@[contained(fs: [], net: [], exec: none)]\n\
                     fn peek() -> i64 {\n    let _s = read_file(\"/etc/passwd\")\n    1\n}\n\n\
                     fn main() { println(to_str(peek())) }";
    let cases: &[(&str, &str, &str, bool)] = &[
        ("unmarked top-level statement", "axon", "let x = 1\nprintln(to_str(x))", true),
        ("`fails E1001` on a clean block", "axon fails E1001", "fn main() { println(\"hi\") }", true),
        ("`fails E9999` on an E1001 block", "axon fails E9999", contained, true),
        ("`fails E1001` on an E1001 block", "axon fails E1001", contained, false),
        ("fine fragment", "axon fragment", "let x = 2\nprintln(to_str(x * 21))", false),
        ("program that panics at run", "axon", "fn main() { assert(false) }", true),
        ("unknown marker", "axon illustrative", "fn main() { println(\"hi\") }", true),
        ("`exits 3` on a program that exits 0", "axon exits 3", "fn main() { println(\"ok\") }", true),
        ("`exits 45` on main returning 45", "axon exits 45", "fn main() -> i64 { 45 }", false),
        ("failing @[test]", "axon", "@[test]\nfn t() { assert_eq(1, 2) }", true),
        (
            "4-backtick quote of a broken block",
            "text",
            "```axon\nlet top = 1\n```",
            false,
        ),
    ];
    let mut md = String::new();
    let mut lines = Vec::new();
    for (_, info, body, _) in cases {
        md.push_str("Some prose.\n\n");
        lines.push(md.lines().count() + 1);
        let fence = if body.contains("```") { "````" } else { "```" };
        md.push_str(&format!("{fence}{info}\n{body}\n{fence}\n\n"));
    }
    let bad = audit("planted.md", &md, "planted");
    for ((name, _, _, reported), line) in cases.iter().zip(&lines) {
        let hit = bad.iter().any(|b| b.starts_with(&format!("planted.md:{line} ")));
        if *reported {
            assert!(hit, "ATTACK: a planted {name} was accepted (line {line}); reports:\n{bad:#?}");
        } else {
            assert!(!hit, "control: the planted {name} was reported (line {line}):\n{bad:#?}");
        }
    }
    assert_eq!(
        bad.len(),
        cases.iter().filter(|c| c.3).count(),
        "one report per bad block: {bad:#?}"
    );
}
