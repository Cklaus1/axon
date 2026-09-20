//! Requirements-evidence integrity checker.
//!
//! `governance/REQUIREMENTS.md` answers "is this done?" and cites a `scripts/*.sh`
//! gate as the evidence. The 2026-09-19 coverage sweep measured that for roughly a
//! quarter of the gates it names, NOTHING EXECUTABLE INVOKES THEM: the claim is
//! real, the gate is real, and the link between them is not. A reader gets an
//! answer that is true about the gate's existence and silent about whether it has
//! ever run.
//!
//! This file closes that with a two-directional drift gate, the same shape the env
//! registry and `AXON_REFERENCE.md` already use.
//!
//! WHY A SIDECAR MANIFEST, AND WHY THE CLASS IS NOT GUESSED
//! -------------------------------------------------------
//! The naive rule "every cited script must appear by filename somewhere
//! executable" was measured wrong in four distinct ways, all present in this repo:
//!
//!   1. SELF-REFERENCE — a script whose header comment names its own filename
//!      matched the probe and was classified as wired. That alone hid 11 orphans.
//!   2. GLOB INVOCATION — `parity_all.sh` runs `for h in scripts/*_parity.sh`, so
//!      ~12 harnesses are genuinely invoked while being named nowhere.
//!   3. DOC-ONLY REFERENCE — `r23_acceptance_gate.sh` is named in a README, three
//!      specs and REQUIREMENTS.md itself, and nothing ran it.
//!   4. OPT-IN SKIP — a harness can sit under an invoked glob and still not run.
//!
//! So the execution class is DECLARED, in `governance/EVIDENCE_CLASSES.md`, one row
//! per cited script — and then VERIFIED against the invocation facts this file
//! computes from the repo. The manifest is a claim, not a label:
//!
//!   * a declared class may never be STRONGER than the observed facts support
//!     (you cannot write `strict_gate` for a script `gate.sh` does not call), and
//!   * a declared excuse may never be weaker than an observed continuous
//!     invocation (a script that became wired cannot stay labelled `orphaned`),
//!   * and every class carries its own checked witness obligation.
//!
//! Hence a human adding a requirement cannot get "verified" for free, and the class
//! cannot be guessed from the filename: it is a fact about the invocation graph.
//!
//! The manifest lives beside REQUIREMENTS.md rather than inside it because the
//! class is a property of the SCRIPT (one gate is cited by several requirements),
//! and because requirement rows are prose blobs whose status claims are owned by a
//! different process.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Classes
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    /// Invoked directly by `scripts/gate.sh`.
    StrictGate,
    /// Invoked directly by a `.github/workflows/*.yml` job.
    CiJob,
    /// Invoked directly by a Rust integration test.
    CargoTest,
    /// Matched by a glob in a script that is itself reachable from a root.
    GlobInvoked,
    /// Reachable, but the invocation site is guarded by an env flag that is not
    /// set by the invoker — it can run, and by default may not.
    Conditional,
    /// Something invokes it, but that something is reachable from no root: a human
    /// has to remember. Not continuous verification.
    ManualOperator,
    /// Evidence is a recorded measurement, not a continuous check.
    RecordedBenchmark,
    /// Needs hardware or a toolchain that is not present.
    ExternalHardware,
    /// Nothing executable invokes it.
    Orphaned,
}

impl Class {
    fn parse(s: &str) -> Option<Class> {
        Some(match s.trim() {
            "strict_gate" => Class::StrictGate,
            "ci_job" => Class::CiJob,
            "cargo_test" => Class::CargoTest,
            "glob_invoked" => Class::GlobInvoked,
            "conditional" => Class::Conditional,
            "manual_operator" => Class::ManualOperator,
            "recorded_benchmark" => Class::RecordedBenchmark,
            "external_hardware" => Class::ExternalHardware,
            "orphaned" => Class::Orphaned,
            _ => return None,
        })
    }

    pub fn name(self) -> &'static str {
        match self {
            Class::StrictGate => "strict_gate",
            Class::CiJob => "ci_job",
            Class::CargoTest => "cargo_test",
            Class::GlobInvoked => "glob_invoked",
            Class::Conditional => "conditional",
            Class::ManualOperator => "manual_operator",
            Class::RecordedBenchmark => "recorded_benchmark",
            Class::ExternalHardware => "external_hardware",
            Class::Orphaned => "orphaned",
        }
    }

    /// How strong a claim the class makes. A declaration may not out-rank the facts.
    fn rank(self) -> u8 {
        match self {
            Class::StrictGate | Class::CiJob | Class::CargoTest | Class::GlobInvoked => 4,
            Class::Conditional => 3,
            Class::ManualOperator => 2,
            Class::RecordedBenchmark | Class::ExternalHardware => 1,
            Class::Orphaned => 0,
        }
    }

    /// Does this class mean "something runs it on every change, unasked"?
    pub fn is_continuous(self) -> bool {
        self.rank() == 4
    }

    /// May a requirement claiming verification cite evidence of this class?
    ///
    /// `conditional` / `recorded_benchmark` / `external_hardware` are ACKNOWLEDGED
    /// non-continuous: permitted, because each carries a checked witness and a
    /// written reason, and reported separately so the acknowledgement stays visible.
    /// `manual_operator` and `orphaned` are not: "a human remembers to run it" is
    /// the exact failure this file exists to catch.
    pub fn may_support_a_claim(self) -> bool {
        !matches!(self, Class::Orphaned | Class::ManualOperator)
    }
}

// ---------------------------------------------------------------------------
// Invocation facts, measured from the repo
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Sh,
    Rs,
}

/// Why a `scripts/<name>.sh` occurrence does or does not count as a caller.
/// Recorded for EVERY occurrence, including the rejected ones — a violation that
/// cannot say why a script is unreachable is not actionable.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MentionKind {
    /// Counts: a real invocation in command position.
    Invocation,
    /// Counts: matched by a glob the invoker iterates.
    Glob,
    /// Rejected: the file names itself. The single largest false-positive class.
    SelfReference,
    /// Rejected: inside a `#` / `//` comment.
    Comment,
    /// Rejected: inside the text of an `echo`/`printf`.
    EchoText,
    /// Rejected: inside a `[ -f … ]` / `[[ -f … ]]` existence test.
    ExistenceTest,
    /// Rejected: named in a shell line, but not in command position.
    NotCommandPosition,
    /// Rejected: named inside a longer Rust string (a panic message, help text).
    RustMessage,
    /// Rejected: named in prose with no `scripts/` path and no command shape.
    Prose,
}

impl MentionKind {
    fn counts(self) -> bool {
        matches!(self, MentionKind::Invocation | MentionKind::Glob)
    }
    fn label(self) -> &'static str {
        match self {
            MentionKind::Invocation => "INVOCATION",
            MentionKind::Glob => "GLOB",
            MentionKind::SelfReference => "SELF-REFERENCE",
            MentionKind::Comment => "COMMENT",
            MentionKind::EchoText => "ECHO TEXT",
            MentionKind::ExistenceTest => "EXISTENCE TEST",
            MentionKind::NotCommandPosition => "NOT A COMMAND",
            MentionKind::RustMessage => "STRING MESSAGE",
            MentionKind::Prose => "PROSE",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Mention {
    pub file: String,
    pub line: usize,
    pub kind: MentionKind,
    pub text: String,
}

#[derive(Debug, Default)]
pub struct Facts {
    /// script basename -> set of (invoker path, guarded-by env vars)
    direct: BTreeMap<String, BTreeSet<(String, String)>>,
    /// script basename -> set of (invoker path, glob pattern)
    globs: BTreeMap<String, BTreeSet<(String, String)>>,
    /// repo-relative paths of executables that run without a human choosing them
    roots: BTreeSet<String>,
    /// script basenames reachable from a root
    reachable: BTreeSet<String>,
    scripts: BTreeSet<String>,
    /// script basename -> every occurrence anywhere executable, counted or not
    mentions: BTreeMap<String, Vec<Mention>>,
    root: PathBuf,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name == "target" || name == ".git" || name == "node_modules" {
            continue;
        }
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

/// Split a shell line into tokens, keeping the separators that mark the start of a
/// command. Quotes are dropped; `;`, `&&`, `||`, `|`, `(`, `)` become their own
/// tokens so "is this in command position?" is answerable.
fn sh_tokens(line: &str) -> Vec<String> {
    let mut s = String::with_capacity(line.len() + 16);
    let mut chars = line.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '"' | '\'' => s.push(' '),
            ';' | '(' | ')' => {
                s.push(' ');
                s.push(c);
                s.push(' ');
            }
            '&' | '|' => {
                s.push(' ');
                s.push(c);
                if chars.peek() == Some(&c) {
                    s.push(chars.next().unwrap());
                }
                s.push(' ');
            }
            _ => s.push(c),
        }
    }
    s.split_whitespace().map(|t| t.to_string()).collect()
}

const CMD_STARTERS: &[&str] = &[
    ";", "&&", "||", "|", "(", "&", "then", "do", "else", "bash", "sh", "source", "exec", ".", "!",
];

/// Is this token position inside the TEXT of an `echo`/`printf`? Everything after
/// such a command is a message, not a command — `gate.sh` ends by echoing
/// `./scripts/parity_all.sh` as advice to the reader, and a usage string commonly
/// contains `bash scripts/<itself>.sh`.
fn echo_text_positions(toks: &[String]) -> Vec<bool> {
    let mut out = vec![false; toks.len()];
    let mut in_echo = false;
    let mut cmd_start = true;
    for (i, t) in toks.iter().enumerate() {
        if matches!(t.as_str(), ";" | "&&" | "||" | "|" | "(" | ")" | "&") {
            in_echo = false;
            cmd_start = true;
            continue;
        }
        if cmd_start && (t == "echo" || t == "printf") {
            in_echo = true;
        }
        out[i] = in_echo;
        cmd_start = matches!(t.as_str(), "then" | "do" | "else" | "!");
    }
    out
}

/// Does this shell line INVOKE `script` (as opposed to naming it in a test, an
/// echo, or a message)? Command position is the whole point: `[[ -f
/// "$REPO/scripts/x.sh" ]]` names it and does not run it.
/// Classify the strongest occurrence of `script` on this shell line. `None` means
/// the line does not name it at all.
fn sh_mention_kind(line: &str, script: &str) -> Option<MentionKind> {
    let suffix = format!("scripts/{script}");
    let toks = sh_tokens(line);
    let echoed = echo_text_positions(&toks);
    let mut worst: Option<MentionKind> = None;
    let note = |k: MentionKind, worst: &mut Option<MentionKind>| {
        // Lower discriminant == stronger; keep the strongest seen.
        if worst.map(|w| k < w).unwrap_or(true) {
            *worst = Some(k);
        }
    };
    for (i, t) in toks.iter().enumerate() {
        if !t.ends_with(&suffix) {
            continue;
        }
        // Reject `foo_scripts/x.sh`: the char before `scripts/` must be a path sep.
        let head = &t[..t.len() - suffix.len()];
        if !(head.is_empty() || head.ends_with('/')) {
            continue;
        }
        if echoed[i] {
            note(MentionKind::EchoText, &mut worst);
        } else if i == 0 || CMD_STARTERS.contains(&toks[i - 1].as_str()) {
            note(MentionKind::Invocation, &mut worst);
        } else if matches!(toks[i - 1].as_str(), "-f" | "-e" | "-x" | "-r" | "-s") {
            note(MentionKind::ExistenceTest, &mut worst);
        } else {
            note(MentionKind::NotCommandPosition, &mut worst);
        }
    }
    worst
}

/// Glob patterns this shell line iterates or runs, e.g. `for h in scripts/*_parity.sh`.
fn sh_glob_patterns(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let toks = sh_tokens(line);
    let echoed = echo_text_positions(&toks);
    for (i, t) in toks.iter().enumerate() {
        if echoed[i] {
            continue;
        }
        let Some(pos) = t.find("scripts/") else {
            continue;
        };
        let pat = &t[pos..];
        if !pat.contains('*') || !pat.ends_with(".sh") {
            continue;
        }
        let prev_ok = i == 0
            || toks[i - 1] == "in"
            || toks[i - 1] == "ls"
            || CMD_STARTERS.contains(&toks[i - 1].as_str());
        if prev_ok {
            out.push(pat.to_string());
        }
    }
    out
}

fn glob_matches(pattern: &str, name: &str) -> bool {
    // Patterns seen here are `scripts/<pre>*<post>.sh`; single `*`.
    let pat = pattern.trim_start_matches("scripts/");
    let Some(star) = pat.find('*') else {
        return pat == name;
    };
    let (pre, post) = (&pat[..star], &pat[star + 1..]);
    name.len() >= pre.len() + post.len() && name.starts_with(pre) && name.ends_with(post)
}

/// A Rust invocation names the script as a COMPLETE string literal tail —
/// `p.push("scripts/x.sh")`, `"{}/../../scripts/x.sh"`. A mention embedded in a
/// longer message (`panic!("missing scripts/x.sh — deleted?")`, a `--help` string)
/// is not an invocation, and `src/main.rs` contains exactly that.
fn rs_mention_kind(line: &str, script: &str) -> Option<MentionKind> {
    if line.contains(&format!("scripts/{script}\"")) {
        return Some(MentionKind::Invocation);
    }
    line.contains(&format!("scripts/{script}"))
        .then_some(MentionKind::RustMessage)
}

/// Does this line name the script WITHOUT a `scripts/` path — the bare basename in
/// prose? Provenance only; a bare name is never an invocation.
fn names_bare(line: &str, script: &str) -> bool {
    let mut rest = line;
    while let Some(i) = rest.find(script) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + script.len()..].chars().next();
        let path_qualified = rest[..i].ends_with('/');
        let word_before = before
            .map(|c| c.is_ascii_alphanumeric() || c == '_')
            .unwrap_or(false);
        let word_after = after
            .map(|c| c.is_ascii_alphanumeric() || c == '_')
            .unwrap_or(false);
        if !path_qualified && !word_before && !word_after {
            return true;
        }
        rest = &rest[i + script.len()..];
    }
    false
}

/// Env vars named in enclosing `if`/`elif` conditions that the file never assigns.
/// `gate.sh`'s `$STRICT` is assigned in `gate.sh`, so `--strict` stages are NOT
/// "conditional"; `parity_all.sh`'s `$PARITY_SKIP_WASM` is not assigned, so it is.
fn guard_vars_of(line: &str, assigned: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = line.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == '$' {
            let mut j = i + 1;
            if j < bytes.len() && bytes[j] == '{' {
                j += 1;
            }
            let start = j;
            while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == '_') {
                j += 1;
            }
            let name: String = bytes[start..j].iter().collect();
            if !name.is_empty()
                && name
                    .chars()
                    .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_')
                && !assigned.contains(&name)
            {
                out.push(name);
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

pub fn scan(root: &Path) -> Facts {
    let mut f = Facts {
        root: root.to_path_buf(),
        ..Default::default()
    };

    // The script universe.
    if let Ok(rd) = fs::read_dir(root.join("scripts")) {
        for e in rd.flatten() {
            let n = e.file_name().to_string_lossy().to_string();
            if n.ends_with(".sh") {
                f.scripts.insert(n);
            }
        }
    }

    // The executable universe + the roots (things that run unasked).
    let mut files: Vec<PathBuf> = Vec::new();
    walk(&root.join("scripts"), &mut files);
    walk(&root.join(".github"), &mut files);
    walk(&root.join("crates"), &mut files);
    if root.join("dev.sh").is_file() {
        files.push(root.join("dev.sh"));
    }

    let mut execs: Vec<(String, Kind, String)> = Vec::new();
    for p in files {
        let rel = p
            .strip_prefix(root)
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let Ok(text) = fs::read_to_string(&p) else {
            continue;
        };
        if rel.ends_with(".sh") || rel.ends_with(".yml") || rel.ends_with(".yaml") {
            execs.push((rel.clone(), Kind::Sh, text));
            if rel == "scripts/gate.sh" || rel.starts_with(".github/") {
                f.roots.insert(rel);
            }
        } else if rel.ends_with(".rs") && (rel.contains("/tests/") || text.contains("#[test]")) {
            execs.push((rel.clone(), Kind::Rs, text));
            f.roots.insert(rel);
        }
    }

    for (rel, kind, text) in &execs {
        let self_name = rel.rsplit('/').next().unwrap_or(rel).to_string();
        let assigned: BTreeSet<String> = text
            .lines()
            .filter_map(|l| {
                let t = l.trim().trim_start_matches("export ");
                let eq = t.find('=')?;
                let name = &t[..eq];
                (!name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
                .then(|| name.to_string())
            })
            .collect();

        let mut guards: Vec<Vec<String>> = Vec::new();
        for (lineno, line) in text.lines().enumerate() {
            let lineno = lineno + 1;
            let t = line.trim();
            // Comments are not invocation. This is the fix for false-positive
            // class (1): a header comment naming the file it sits in. They ARE
            // recorded as mentions, because "its only mention is a comment in
            // r33:6" is the difference between a violation and an action.
            let is_comment = t.starts_with('#') || (*kind == Kind::Rs && t.starts_with("//"));
            if is_comment {
                for sc in &f.scripts {
                    let named = line.contains(&format!("scripts/{sc}")) || names_bare(line, sc);
                    if named {
                        let kind = if *sc == self_name {
                            MentionKind::SelfReference
                        } else {
                            MentionKind::Comment
                        };
                        f.mentions.entry(sc.clone()).or_default().push(Mention {
                            file: rel.clone(),
                            line: lineno,
                            kind,
                            text: t.chars().take(88).collect(),
                        });
                    }
                }
                continue;
            }
            if *kind == Kind::Sh {
                if t.starts_with("if ") || t.starts_with("elif ") {
                    guards.push(guard_vars_of(t, &assigned));
                } else if t == "fi" || t.starts_with("fi ") || t.starts_with("fi;") {
                    guards.pop();
                }
            }
            let guard_now: String = {
                let mut v: Vec<String> = guards.iter().flatten().cloned().collect();
                v.sort();
                v.dedup();
                v.join(",")
            };
            for s in &f.scripts {
                let mut mk = match kind {
                    Kind::Sh => sh_mention_kind(line, s),
                    Kind::Rs => rs_mention_kind(line, s),
                };
                if mk.is_none() && names_bare(line, s) {
                    mk = Some(MentionKind::Prose);
                }
                let Some(mut mk) = mk else { continue };
                // Self-reference is not a caller, whatever shape it takes: a
                // `usage()` line printing `bash scripts/<itself>.sh` reads as a
                // command and is still not a second process running the gate.
                if *s == self_name {
                    mk = MentionKind::SelfReference;
                }
                f.mentions.entry(s.clone()).or_default().push(Mention {
                    file: rel.clone(),
                    line: lineno,
                    kind: mk,
                    text: t.chars().take(88).collect(),
                });
                if mk == MentionKind::Invocation {
                    f.direct
                        .entry(s.clone())
                        .or_default()
                        .insert((rel.clone(), guard_now.clone()));
                }
            }
            if *kind == Kind::Sh {
                for pat in sh_glob_patterns(line) {
                    for s in &f.scripts {
                        if *s != self_name && glob_matches(&pat, s) {
                            f.globs
                                .entry(s.clone())
                                .or_default()
                                .insert((rel.clone(), pat.clone()));
                            f.mentions.entry(s.clone()).or_default().push(Mention {
                                file: rel.clone(),
                                line: lineno,
                                kind: MentionKind::Glob,
                                text: format!("matched by `{pat}`"),
                            });
                        }
                    }
                }
            }
        }
    }

    // Reachability. "Invoked by something that is itself invoked by nothing" is
    // still invoked by nothing — the ledger's probe stopped at one hop, which is
    // why it counted the R26-R29 gates (called only by the orphaned
    // axon_safety_gate.sh) as wired.
    loop {
        let mut grew = false;
        let names: Vec<String> = f.scripts.iter().cloned().collect();
        for s in names {
            if f.reachable.contains(&s) {
                continue;
            }
            let invokers: BTreeSet<String> = f
                .direct
                .get(&s)
                .into_iter()
                .flatten()
                .map(|(p, _)| p.clone())
                .chain(
                    f.globs
                        .get(&s)
                        .into_iter()
                        .flatten()
                        .map(|(p, _)| p.clone()),
                )
                .collect();
            let live = invokers.iter().any(|i| {
                f.roots.contains(i)
                    || f.reachable
                        .contains(i.rsplit('/').next().unwrap_or(i.as_str()))
            });
            if live {
                f.reachable.insert(s);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    f
}

impl Facts {
    fn invoker_is_live(&self, p: &str) -> bool {
        self.roots.contains(p) || self.reachable.contains(p.rsplit('/').next().unwrap_or(p))
    }

    /// The strongest class the FACTS support. A declaration is checked against this.
    pub fn observe(&self, script: &str) -> Class {
        let live_direct: Vec<&(String, String)> = self
            .direct
            .get(script)
            .into_iter()
            .flatten()
            .filter(|(p, _)| self.invoker_is_live(p))
            .collect();
        let live_glob: Vec<&(String, String)> = self
            .globs
            .get(script)
            .into_iter()
            .flatten()
            .filter(|(p, _)| self.invoker_is_live(p))
            .collect();

        // An unguarded live invocation is the strongest thing there is.
        for (p, g) in &live_direct {
            if !g.is_empty() {
                continue;
            }
            if p == "scripts/gate.sh" {
                return Class::StrictGate;
            }
        }
        if live_direct
            .iter()
            .any(|(p, g)| g.is_empty() && p.ends_with(".rs"))
        {
            return Class::CargoTest;
        }
        if live_direct
            .iter()
            .any(|(p, g)| g.is_empty() && (p.ends_with(".yml") || p.ends_with(".yaml")))
        {
            return Class::CiJob;
        }
        if !live_glob.is_empty() {
            return Class::GlobInvoked;
        }
        if live_direct.iter().any(|(_, g)| g.is_empty()) {
            // Reachable via an ordinary script that is itself reachable.
            return Class::GlobInvoked;
        }
        if !live_direct.is_empty() {
            return Class::Conditional; // reachable, but every site is flag-guarded
        }
        let any_edge = self
            .direct
            .get(script)
            .map(|s| !s.is_empty())
            .unwrap_or(false)
            || self
                .globs
                .get(script)
                .map(|s| !s.is_empty())
                .unwrap_or(false);
        if any_edge {
            return Class::ManualOperator;
        }
        Class::Orphaned
    }

    fn invokers_of(&self, script: &str) -> BTreeSet<String> {
        self.direct
            .get(script)
            .into_iter()
            .flatten()
            .map(|(p, _)| p.clone())
            .chain(
                self.globs
                    .get(script)
                    .into_iter()
                    .flatten()
                    .map(|(p, _)| p.clone()),
            )
            .collect()
    }

    /// The shortest chain of real invocations from a root down to `script`, e.g.
    /// `scripts/gate.sh -> scripts/r23_acceptance_gate.sh`. `None` when nothing
    /// that runs unasked can reach it.
    pub fn path_from_root(&self, script: &str) -> Option<Vec<String>> {
        let start = format!("scripts/{script}");
        let mut parent: BTreeMap<String, String> = BTreeMap::new();
        let mut seen: BTreeSet<String> = BTreeSet::new();
        seen.insert(start.clone());
        let mut queue = vec![start.clone()];
        let mut cursor = 0;
        while cursor < queue.len() {
            let node = queue[cursor].clone();
            cursor += 1;
            let Some(base) = node.strip_prefix("scripts/") else {
                continue;
            };
            for inv in self.invokers_of(base) {
                if self.roots.contains(&inv) {
                    // Walk the parent chain back down to `script`.
                    let mut chain = vec![inv, node.clone()];
                    let mut cur = node;
                    while let Some(next) = parent.get(&cur) {
                        chain.push(next.clone());
                        cur = next.clone();
                    }
                    return Some(chain);
                }
                if seen.insert(inv.clone()) {
                    parent.insert(inv.clone(), node.clone());
                    queue.push(inv);
                }
            }
        }
        None
    }

    /// One line saying why an executable is itself unreachable.
    fn why_dead(&self, file: &str) -> String {
        let Some(base) = file.strip_prefix("scripts/") else {
            return "not a script".into();
        };
        let invokers = self.invokers_of(base);
        if invokers.is_empty() {
            match self.mentions.get(base).map(|m| m.len()).unwrap_or(0) {
                0 => "orphaned: named nowhere executable".into(),
                n => format!("orphaned: {n} mention(s), none a caller"),
            }
        } else {
            format!(
                "unreachable: called only by {}",
                invokers.into_iter().collect::<Vec<_>>().join(", ")
            )
        }
    }

    /// WHY this script has the class it has, in the terms the graph computed.
    /// A flat verdict is not actionable; this is the part a reviewer reads.
    pub fn explain(&self, script: &str) -> Vec<String> {
        let mut out = Vec::new();
        if let Some(path) = self.path_from_root(script) {
            out.push(format!("path from root: {}", path.join(" -> ")));
            let guards: BTreeSet<&str> = self
                .direct
                .get(script)
                .into_iter()
                .flatten()
                .filter(|(_, g)| !g.is_empty())
                .map(|(_, g)| g.as_str())
                .collect();
            if !guards.is_empty() && self.observe(script) == Class::Conditional {
                out.push(format!(
                    "but every invocation site is guarded by: {}",
                    guards.into_iter().collect::<Vec<_>>().join(", ")
                ));
            }
            return out;
        }
        out.push("shortest path from a root: NONE".into());
        let invokers = self.invokers_of(script);
        if invokers.is_empty() {
            let ms: Vec<&Mention> = self
                .mentions
                .get(script)
                .map(|v| v.iter().collect())
                .unwrap_or_default();
            if ms.is_empty() {
                out.push(
                    "reason: named nowhere under scripts/, .github/ or crates/ —                      no mention at all"
                        .into(),
                );
            } else {
                out.push(format!(
                    "reason: {} mention(s), not one of them a caller",
                    ms.len()
                ));
                for m in ms.iter().take(6) {
                    out.push(format!(
                        "  · {}:{}  {}  \"{}\"",
                        m.file,
                        m.line,
                        m.kind.label(),
                        m.text
                    ));
                }
                if ms.len() > 6 {
                    out.push(format!("  · … and {} more", ms.len() - 6));
                }
            }
        } else {
            out.push(format!(
                "reason: reachable only via {}, itself reachable from no root",
                invokers.iter().cloned().collect::<Vec<_>>().join(", ")
            ));
            for inv in invokers.iter().take(4) {
                out.push(format!("  · {inv} — {}", self.why_dead(inv)));
                if let Some(base) = inv.strip_prefix("scripts/") {
                    for up in self.invokers_of(base).into_iter().take(3) {
                        out.push(format!("      · {up} — {}", self.why_dead(&up)));
                    }
                }
            }
            // The rejected mentions matter here too: `r28` is named in `r22` and
            // `r23`, and a reader who greps will find those and conclude it is
            // wired. Saying they are COMMENTS is the whole point.
            let rejected: Vec<&Mention> = self
                .mentions
                .get(script)
                .map(|v| v.iter().filter(|m| !m.kind.counts()).collect())
                .unwrap_or_default();
            if !rejected.is_empty() {
                out.push(format!(
                    "also named {} time(s) by nothing that runs it:",
                    rejected.len()
                ));
                for m in rejected.iter().take(4) {
                    out.push(format!(
                        "  · {}:{}  {}  \"{}\"",
                        m.file,
                        m.line,
                        m.kind.label(),
                        m.text
                    ));
                }
            }
        }
        out
    }

    /// Group the violations by the ONE upstream wiring that would resolve them.
    /// Wiring five leaves independently, when all five hang off a single gate
    /// nobody invokes, is four fixes too many — and the leaves would have become
    /// reachable on their own once the root was wired.
    pub fn root_fix_groups(&self, violating: &BTreeSet<String>) -> Vec<(String, Vec<String>)> {
        // Forward edges, restricted to the currently-unreachable subgraph.
        let mut forward: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        for s in &self.scripts {
            for inv in self.invokers_of(s) {
                forward.entry(inv).or_default().insert(s.clone());
            }
        }
        let candidates: Vec<String> = self
            .scripts
            .iter()
            .filter(|s| !self.reachable.contains(*s))
            .cloned()
            .collect();
        let covers = |c: &str| -> BTreeSet<String> {
            let mut seen: BTreeSet<String> = BTreeSet::new();
            let mut queue = vec![c.to_string()];
            while let Some(n) = queue.pop() {
                if !seen.insert(n.clone()) {
                    continue;
                }
                for d in forward.get(&format!("scripts/{n}")).into_iter().flatten() {
                    if !self.reachable.contains(d) {
                        queue.push(d.clone());
                    }
                }
            }
            seen.intersection(violating).cloned().collect()
        };

        let mut remaining: BTreeSet<String> = violating.clone();
        let mut out = Vec::new();
        while !remaining.is_empty() {
            let mut best: Option<(String, BTreeSet<String>)> = None;
            for c in &candidates {
                let hit: BTreeSet<String> = covers(c).intersection(&remaining).cloned().collect();
                if hit.is_empty() {
                    continue;
                }
                let better = match &best {
                    None => true,
                    Some((bn, bh)) => {
                        (hit.len(), std::cmp::Reverse(c.clone()))
                            > (bh.len(), std::cmp::Reverse(bn.clone()))
                    }
                };
                if better {
                    best = Some((c.clone(), hit));
                }
            }
            let Some((name, hit)) = best else { break };
            for h in &hit {
                remaining.remove(h);
            }
            out.push((format!("scripts/{name}"), hit.into_iter().collect()));
        }
        out
    }

    /// Verify a DECLARED (class, witness) against the facts. `Err` is a manifest
    /// that does not hold — in either direction.
    pub fn verify(&self, script: &str, declared: Class, witness: &str) -> Result<(), String> {
        let observed = self.observe(script);
        // `recorded_benchmark` / `external_hardware` are statements about WHY the
        // absence of a caller is acceptable, not claims about the invocation
        // graph, so the graph cannot support them and does not gate them — their
        // witness obligation below does. Every other class is a claim about
        // invocation and may not out-rank what was measured.
        let is_excuse = matches!(declared, Class::RecordedBenchmark | Class::ExternalHardware);
        if declared.rank() > observed.rank() && !is_excuse {
            return Err(format!(
                "declares `{}` but the facts only support `{}` (over-claim)",
                declared.name(),
                observed.name()
            ));
        }
        if observed.is_continuous() && declared.rank() < Class::Conditional.rank() {
            return Err(format!(
                "declares `{}` but is now invoked continuously (`{}`) — stale label, reclassify",
                declared.name(),
                observed.name()
            ));
        }
        let has_direct = |pred: &dyn Fn(&str) -> bool| {
            self.direct
                .get(script)
                .into_iter()
                .flatten()
                .any(|(p, _)| pred(p) && self.invoker_is_live(p))
        };
        match declared {
            Class::StrictGate => {
                if witness != "scripts/gate.sh" {
                    return Err("witness must be `scripts/gate.sh`".into());
                }
                if !has_direct(&|p| p == "scripts/gate.sh") {
                    return Err("scripts/gate.sh does not invoke it".into());
                }
            }
            Class::CiJob | Class::CargoTest => {
                let want = witness.to_string();
                if !has_direct(&|p| p == want) {
                    return Err(format!("witness `{witness}` does not invoke it"));
                }
            }
            Class::GlobInvoked => {
                let (inv, pat) = witness
                    .split_once("::")
                    .ok_or("witness must be `<invoker>::<glob pattern>`")?;
                let ok = self
                    .globs
                    .get(script)
                    .into_iter()
                    .flatten()
                    .any(|(p, g)| p == inv && g == pat && self.invoker_is_live(p));
                if !ok {
                    return Err(format!("no live glob edge `{witness}`"));
                }
            }
            Class::Conditional => {
                let flag = witness
                    .strip_prefix("FLAG=")
                    .ok_or("witness must be `FLAG=<ENV_VAR>`")?;
                if !self.reachable.contains(script) {
                    return Err("nothing reachable invokes it, so it cannot be conditional".into());
                }
                let mut seen = self.read_script(script).contains(flag);
                for (p, _) in self.direct.get(script).into_iter().flatten() {
                    seen |= self.read_rel(p).contains(flag);
                }
                for (p, _) in self.globs.get(script).into_iter().flatten() {
                    seen |= self.read_rel(p).contains(flag);
                }
                if !seen {
                    return Err(format!(
                        "flag `{flag}` appears in neither the script nor an invoker"
                    ));
                }
            }
            Class::ManualOperator => {
                let ok = self
                    .direct
                    .get(script)
                    .into_iter()
                    .flatten()
                    .any(|(p, _)| p == witness && !self.invoker_is_live(p));
                if !ok {
                    return Err(format!(
                        "witness `{witness}` must invoke it and must itself be reachable from no root"
                    ));
                }
            }
            Class::RecordedBenchmark => {
                let path = self.root.join(witness);
                let Ok(text) = fs::read_to_string(&path) else {
                    return Err(format!("recorded measurement `{witness}` does not exist"));
                };
                if !text.contains(script) {
                    return Err(format!("`{witness}` does not name `{script}`"));
                }
            }
            Class::ExternalHardware => {
                let tool = witness
                    .strip_prefix("NEEDS=")
                    .ok_or("witness must be `NEEDS=<tool>`")?;
                let text = self.read_script(script);
                let guarded = text.lines().any(|l| {
                    l.contains(tool)
                        && (l.contains("command -v")
                            || l.contains("which ")
                            || l.to_lowercase().contains("skip")
                            || l.contains("-x ")
                            || l.contains("-d "))
                });
                if !guarded {
                    return Err(format!(
                        "`{script}` has no skip guard mentioning `{tool}` — an absent toolchain must SKIP, not pass"
                    ));
                }
            }
            Class::Orphaned => {
                if !witness.is_empty() && witness != "-" {
                    return Err("an orphan has no witness; use `-`".into());
                }
                if self.reachable.contains(script) {
                    return Err("declared orphaned but is reachable — stale label".into());
                }
            }
        }
        Ok(())
    }

    fn read_script(&self, s: &str) -> String {
        fs::read_to_string(self.root.join("scripts").join(s)).unwrap_or_default()
    }
    fn read_rel(&self, p: &str) -> String {
        fs::read_to_string(self.root.join(p)).unwrap_or_default()
    }
}

// ---------------------------------------------------------------------------
// The register and the manifest
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct Requirement {
    pub id: String,
    pub line: usize,
    /// Does the row assert a verified/landed state?
    pub claims_verification: bool,
    pub evidence: Vec<String>,
}

pub fn parse_requirements(text: &str) -> Vec<Requirement> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').collect();
        if cells.len() < 3 {
            continue;
        }
        let id = cells[0].trim().trim_matches('*').trim().to_string();
        let is_req = id.len() >= 2
            && id.starts_with('R')
            && id[1..].chars().all(|c| c.is_ascii_digit())
            && id.len() > 1;
        if !is_req {
            continue;
        }
        let lower = t.to_lowercase();
        let claims = t.contains('✅') || lower.contains("landed") || t.contains(" PASS");
        out.push(Requirement {
            id,
            line: i + 1,
            claims_verification: claims,
            evidence: cited_scripts(t),
        });
    }
    out
}

/// Every `scripts/<name>.sh` named in a chunk of text.
pub fn cited_scripts(text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(pos) = rest.find("scripts/") {
        rest = &rest[pos + "scripts/".len()..];
        let end = rest
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.'))
            .unwrap_or(rest.len());
        let name = &rest[..end];
        if name.ends_with(".sh") && !out.iter().any(|x| x == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[derive(Debug)]
pub struct ManifestRow {
    pub script: String,
    pub class: Class,
    pub witness: String,
    pub note: String,
}

pub fn parse_manifest(text: &str) -> Result<Vec<ManifestRow>, String> {
    let mut out = Vec::new();
    for line in text.lines() {
        let t = line.trim();
        if !t.starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(|c| c.trim()).collect();
        if cells.len() < 4 || !cells[0].ends_with(".sh") {
            continue;
        }
        let class = Class::parse(cells[1])
            .ok_or_else(|| format!("{}: unknown class `{}`", cells[0], cells[1]))?;
        let witness = cells[2].trim_matches('`').to_string();
        let note = cells[3].to_string();
        if note.is_empty() || note == "-" {
            return Err(format!("{}: every row needs a written reason", cells[0]));
        }
        out.push(ManifestRow {
            script: cells[0].to_string(),
            class,
            witness,
            note,
        });
    }
    Ok(out)
}

// ---------------------------------------------------------------------------
// The check
// ---------------------------------------------------------------------------

pub struct Report {
    pub errors: Vec<String>,
    pub acknowledged: Vec<String>,
    pub table: String,
    /// Per-violation provenance: the class, and WHY the graph says so.
    pub provenance: String,
    /// One upstream wiring -> the violations it would resolve, in the order to
    /// apply them. Wire the root, recompute, do not patch leaves.
    pub root_fixes: Vec<(String, Vec<String>)>,
    pub n_requirements: usize,
    pub n_citations: usize,
}

pub fn check(root: &Path) -> Report {
    let mut errors = Vec::new();
    let mut acknowledged = Vec::new();
    let mut table = String::new();
    let mut provenance = String::new();
    let mut violating: BTreeSet<String> = BTreeSet::new();

    let reqs_path = root.join("governance/REQUIREMENTS.md");
    let man_path = root.join("governance/EVIDENCE_CLASSES.md");
    let reqs_text = fs::read_to_string(&reqs_path).unwrap_or_default();
    let man_text = fs::read_to_string(&man_path).unwrap_or_default();

    let reqs = parse_requirements(&reqs_text);
    let facts = scan(root);

    let rows = match parse_manifest(&man_text) {
        Ok(r) => r,
        Err(e) => {
            errors.push(format!("EVIDENCE_CLASSES.md: {e}"));
            Vec::new()
        }
    };
    let by_script: BTreeMap<&str, &ManifestRow> =
        rows.iter().map(|r| (r.script.as_str(), r)).collect();

    let mut cited: BTreeSet<String> = BTreeSet::new();
    let mut n_citations = 0usize;
    for r in &reqs {
        for e in &r.evidence {
            cited.insert(e.clone());
            n_citations += 1;
        }
    }

    // Non-vacuity floor. A checker that parsed nothing must not report clean.
    if reqs.is_empty() {
        errors.push("VACUOUS: parsed 0 requirement rows from REQUIREMENTS.md".into());
    }
    if cited.is_empty() {
        errors.push("VACUOUS: parsed 0 cited evidence scripts from REQUIREMENTS.md".into());
    }
    if rows.is_empty() {
        errors.push("VACUOUS: parsed 0 rows from EVIDENCE_CLASSES.md".into());
    }

    // The manifest must cover the citations exactly — both directions.
    for c in &cited {
        if !facts.scripts.contains(c) {
            errors.push(format!(
                "{c}: cited by REQUIREMENTS.md but no such file in scripts/"
            ));
        }
        if !by_script.contains_key(c.as_str()) {
            errors.push(format!(
                "{c}: cited as evidence with no row in governance/EVIDENCE_CLASSES.md \
                 (add one — the class is never inferred)"
            ));
        }
    }
    for r in &rows {
        if !cited.contains(&r.script) {
            errors.push(format!(
                "{}: has an EVIDENCE_CLASSES.md row but is cited by no requirement (stale row)",
                r.script
            ));
        }
    }

    // Each declared class must hold against the measured facts.
    for r in &rows {
        if let Err(e) = facts.verify(&r.script, r.class, &r.witness) {
            errors.push(format!("{}: {e}", r.script));
        }
    }

    // The headline rule: a verification claim may not rest on evidence nothing runs.
    table.push_str(&format!(
        "{:<6} {:<34} {:<18} {:<14} {:<13} {}\n",
        "req", "evidence", "declared", "observed", "verdict", "path from root"
    ));
    for r in &reqs {
        for e in &r.evidence {
            let declared = by_script.get(e.as_str()).map(|m| m.class);
            let observed = facts.observe(e);
            let verdict = match (r.claims_verification, declared) {
                (_, None) => "NO CLASS",
                (false, _) => "no claim",
                (true, Some(c)) if c.is_continuous() => "ok",
                (true, Some(c)) if c.may_support_a_claim() => "acknowledged",
                (true, Some(_)) => "VIOLATION",
            };
            table.push_str(&format!(
                "{:<6} {:<34} {:<18} {:<14} {:<13} {}\n",
                r.id,
                e,
                declared.map(|c| c.name()).unwrap_or("-"),
                observed.name(),
                verdict,
                facts
                    .path_from_root(e)
                    .map(|p| p.join(" -> "))
                    .unwrap_or_else(|| "NONE".into())
            ));
            if verdict == "VIOLATION" {
                let cls = declared.unwrap();
                violating.insert(e.clone());
                let why = facts.explain(e);
                provenance.push_str(&format!(
                    "{} (REQUIREMENTS.md:{}) -> scripts/{} is `{}`\n",
                    r.id,
                    r.line,
                    e,
                    cls.name()
                ));
                for l in &why {
                    provenance.push_str(&format!("      {l}\n"));
                }
                provenance.push('\n');
                errors.push(format!(
                    "{} (REQUIREMENTS.md:{}) claims verification and cites `scripts/{}`, \
                     which is `{}` — nothing runs it\n{}",
                    r.id,
                    r.line,
                    e,
                    cls.name(),
                    why.iter()
                        .map(|l| format!("      {l}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                ));
            } else if verdict == "acknowledged" {
                acknowledged.push(format!(
                    "{} cites `scripts/{}` — {} (not continuous)",
                    r.id,
                    e,
                    declared.unwrap().name()
                ));
            }
        }
    }

    let root_fixes = facts.root_fix_groups(&violating);
    Report {
        errors,
        acknowledged,
        table,
        provenance,
        root_fixes,
        n_requirements: reqs.len(),
        n_citations,
    }
}

/// Render the root-fix plan. Ordering is the point: wire the root, recompute,
/// and the leaves under it stop being violations without a second edit.
pub fn render_root_fixes(groups: &[(String, Vec<String>)]) -> String {
    if groups.is_empty() {
        return String::new();
    }
    let mut s =
        String::from("── root fixes, in dependency order (wire the root, then recompute) ──\n");
    for (i, (root, covered)) in groups.iter().enumerate() {
        s.push_str(&format!(
            "  {:>2}. {:<38} covers {}: {}\n",
            i + 1,
            root,
            covered.len(),
            covered.join(", ")
        ));
    }
    let leaves: usize = groups.iter().map(|(_, c)| c.len()).sum();
    s.push_str(&format!(
        "  {} upstream wiring(s) would clear all {} unreachable evidence script(s).\n",
        groups.len(),
        leaves
    ));
    s
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[test]
fn requirements_evidence_is_not_orphaned() {
    let root = repo_root();
    let rep = check(&root);
    println!("{}", rep.table);
    println!(
        "parsed {} requirement rows, {} evidence citations",
        rep.n_requirements, rep.n_citations
    );
    // Non-vacuity floor, stated as numbers rather than "> 0": this register has
    // three requirement tables and dozens of citations; a parser that suddenly
    // sees a handful has broken, not been cleaned up.
    assert!(
        rep.n_requirements >= 10,
        "VACUOUS: only {} requirement rows parsed",
        rep.n_requirements
    );
    assert!(
        rep.n_citations >= 20,
        "VACUOUS: only {} evidence citations parsed",
        rep.n_citations
    );
    if !rep.acknowledged.is_empty() {
        println!("\nacknowledged non-continuous evidence:");
        for a in &rep.acknowledged {
            println!("  · {a}");
        }
    }
    if !rep.provenance.is_empty() {
        println!("\n── why each violation is unreachable ──────────────────────────\n");
        println!("{}", rep.provenance);
    }
    let fixes = render_root_fixes(&rep.root_fixes);
    if !fixes.is_empty() {
        println!("{fixes}");
    }
    assert!(
        rep.errors.is_empty(),
        "requirements-evidence integrity: {} problem(s)\n{}\n\n{}",
        rep.errors.len(),
        rep.errors
            .iter()
            .map(|e| format!("  ✗ {e}"))
            .collect::<Vec<_>>()
            .join("\n"),
        fixes
    );
}

/// Helper for whoever adds a requirement: prints the class the FACTS support for
/// every cited script, which is the ceiling on what its manifest row may declare.
/// Always passes — it reports, it does not judge.
#[test]
fn evidence_class_observed_dump() {
    let root = repo_root();
    let facts = scan(&root);
    let text = fs::read_to_string(root.join("governance/REQUIREMENTS.md")).unwrap_or_default();
    let mut cited: BTreeSet<String> = BTreeSet::new();
    for r in parse_requirements(&text) {
        cited.extend(r.evidence);
    }
    println!(
        "observed ceiling per cited script ({} scripts):",
        cited.len()
    );
    for c in &cited {
        let d = facts.direct.get(c).cloned().unwrap_or_default();
        let g = facts.globs.get(c).cloned().unwrap_or_default();
        println!(
            "| {c} | {} | direct={:?} glob={:?} |",
            facts.observe(c).name(),
            d.iter()
                .map(|(p, gd)| format!("{p}[{gd}]"))
                .collect::<Vec<_>>(),
            g.iter()
                .map(|(p, pat)| format!("{p}::{pat}"))
                .collect::<Vec<_>>()
        );
    }
}

// ---------------------------------------------------------------------------
// Fixtures
//
// Each fixture is a miniature repository written under `target/` (never
// committed, so it cannot be picked up by the repo's own scanners) that isolates
// ONE of the four ways a filename probe was measured wrong, plus the classes the
// naive rule has no way to express. The classifier's answer is asserted against
// the answer a human reading that fixture would give.
// ---------------------------------------------------------------------------

struct Fixture {
    root: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Fixture {
        // Process-unique: two `cargo test` runs can share a target dir (parallel
        // worktrees, CARGO_TARGET_DIR), and a fixture that another process is
        // rebuilding underneath you is a flake that reads as a classifier bug.
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/reqev_fixtures")
            .join(format!("{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("governance")).unwrap();
        Fixture { root }
    }
    fn write(&self, rel: &str, body: &str) -> &Fixture {
        let p = self.root.join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
        self
    }
    fn facts(&self) -> Facts {
        scan(&self.root)
    }
    fn check(&self) -> Report {
        check(&self.root)
    }
}

/// CASE 1 — a DIRECT caller in command position is the real thing.
#[test]
fn fixture_direct_caller_is_wired() {
    let f = Fixture::new("direct_caller");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\n./scripts/wired.sh || exit 1\n",
    )
    .write("scripts/wired.sh", "#!/usr/bin/env bash\necho ok\n");
    let c = f.facts().observe("wired.sh");
    assert_eq!(c, Class::StrictGate, "a script gate.sh runs is wired");
    assert!(c.is_continuous());
}

/// CASE 2 — GLOB invocation is real and names no file. A filename probe calls
/// this an orphan; it is the single largest false-negative class in this repo
/// (~12 harnesses under `parity_all.sh`).
#[test]
fn fixture_glob_caller_is_wired() {
    let f = Fixture::new("glob_caller");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\n./scripts/agg.sh\n")
        .write(
            "scripts/agg.sh",
            "#!/usr/bin/env bash\nfor h in scripts/*_parity.sh; do\n  bash \"$h\"\ndone\n",
        )
        .write(
            "scripts/thing_parity.sh",
            "#!/usr/bin/env bash\necho PASS\n",
        );
    let c = f.facts().observe("thing_parity.sh");
    assert_eq!(
        c,
        Class::GlobInvoked,
        "named nowhere, but the glob really runs it"
    );
    assert!(c.is_continuous());
}

/// CASE 3 — DOC-ONLY reference is not invocation. `r23_acceptance_gate.sh` was
/// named in a README, three specs and REQUIREMENTS.md, and ran nowhere.
#[test]
fn fixture_doc_only_reference_is_orphan() {
    let f = Fixture::new("doc_only");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/documented.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("README.md", "Run `scripts/documented.sh` to verify R99.\n")
        .write(
            "governance/specs/R99.md",
            "Acceptance: scripts/documented.sh PASS\n",
        );
    assert_eq!(f.facts().observe("documented.sh"), Class::Orphaned);
}

/// CASE 4 — SELF-REFERENCE fakes a caller. The header comment of a script names
/// its own filename; the ledger's probe counted that as "referenced by
/// something" and it hid 11 orphans on its own.
#[test]
fn fixture_self_reference_only_is_orphan() {
    let f = Fixture::new("self_reference");
    // Both self-reference forms at once: the header comment (what fooled the
    // ledger's probe) and a usage line that survives comment-stripping and reads
    // as a command — `bash scripts/lonely_gate.sh` in command position.
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "scripts/lonely_gate.sh",
            "#!/usr/bin/env bash\n# lonely_gate.sh — the R99 acceptance gate\n\
         usage() { bash scripts/lonely_gate.sh --check; }\n",
        );
    assert_eq!(f.facts().observe("lonely_gate.sh"), Class::Orphaned);
}

/// CASE 4b — a comment inside an INVOKED script is still a comment.
#[test]
fn fixture_comment_in_gate_is_not_a_caller() {
    let f = Fixture::new("comment_only");
    f.write(
        "scripts/gate.sh",
        // Command-shaped, so only the comment rule can reject it.
        "#!/usr/bin/env bash\n# TODO, once it passes:\n#   bash scripts/ghost.sh --check\necho hi\n",
    )
    .write("scripts/ghost.sh", "#!/usr/bin/env bash\necho hi\n");
    assert_eq!(f.facts().observe("ghost.sh"), Class::Orphaned);
}

/// CASE 4c — naming a script in an existence test or an `echo` is not running it.
#[test]
fn fixture_mention_without_command_position_is_orphan() {
    let f = Fixture::new("mention_only");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\nif [ -f \"$ROOT/scripts/mentioned.sh\" ]; then\n  echo \"see scripts/mentioned.sh for details\"\nfi\n",
    )
    .write("scripts/mentioned.sh", "#!/usr/bin/env bash\necho hi\n");
    assert_eq!(f.facts().observe("mentioned.sh"), Class::Orphaned);
}

/// CASE 4d — the Rust half of the same rule: a script named inside a longer
/// message string is a mention. `crates/axon-core/src/main.rs` contains exactly
/// this for `wasm_parity.sh`.
#[test]
fn fixture_rust_message_mention_is_orphan() {
    let f = Fixture::new("rs_mention");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/probe.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "crates/x/tests/t.rs",
            "#[test]\nfn t() {\n    panic!(\"missing scripts/probe.sh — was it deleted?\");\n}\n",
        );
    assert_eq!(f.facts().observe("probe.sh"), Class::Orphaned);

    // …and the invoking form IS detected, so the rule is not vacuously strict.
    let g = Fixture::new("rs_invoke");
    g.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/probe.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "crates/x/tests/t.rs",
            "#[test]\nfn t() {\n    let mut p = root();\n    p.push(\"scripts/probe.sh\");\n}\n",
        );
    assert_eq!(g.facts().observe("probe.sh"), Class::CargoTest);
}

/// CASE 5 — OPT-IN: invoked, but the site is guarded by an env flag the invoker
/// does not set. Classified `conditional`, and `conditional` is NOT wired.
#[test]
fn fixture_opt_in_is_conditional_not_wired() {
    let f = Fixture::new("opt_in");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\nif [ \"${DEEP_CHECK:-0}\" = 1 ]; then\n  ./scripts/deep.sh || exit 1\nfi\n",
    )
    .write("scripts/deep.sh", "#!/usr/bin/env bash\necho ok\n");
    let facts = f.facts();
    let c = facts.observe("deep.sh");
    assert_eq!(c, Class::Conditional, "opt-in must not read as wired");
    assert!(!c.is_continuous(), "conditional is NOT wired");
    assert!(facts
        .verify("deep.sh", Class::Conditional, "FLAG=DEEP_CHECK")
        .is_ok());
    // The flag must be the real one — a made-up flag does not satisfy the class.
    assert!(facts
        .verify("deep.sh", Class::Conditional, "FLAG=NO_SUCH_FLAG")
        .is_err());
}

/// A locally-assigned variable is not an opt-in. `gate.sh`'s own `--strict` sets
/// `STRICT` itself, so its `--strict` stages are `strict_gate`, not `conditional`.
#[test]
fn fixture_locally_assigned_guard_is_not_an_opt_in() {
    let f = Fixture::new("local_guard");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\nSTRICT=0\nfor a in \"$@\"; do case $a in --strict) STRICT=1 ;; esac; done\nif [ \"$STRICT\" = 1 ]; then\n  ./scripts/extra.sh || exit 1\nfi\n",
    )
    .write("scripts/extra.sh", "#!/usr/bin/env bash\necho ok\n");
    assert_eq!(f.facts().observe("extra.sh"), Class::StrictGate);
}

/// CASE 6 — BENCHMARK: evidence is a recorded measurement, not a continuous
/// check. Permitted, but only against a witness file that exists and names it —
/// and it never counts as continuous.
#[test]
fn fixture_benchmark_is_measured_not_continuous() {
    let f = Fixture::new("benchmark");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/bench.sh", "#!/usr/bin/env bash\necho 6.0x\n")
        .write(
            "governance/measurements.md",
            "2026-09-19 scripts/bench.sh: native 6.0x faster\n",
        );
    let facts = f.facts();
    assert_eq!(
        facts.observe("bench.sh"),
        Class::Orphaned,
        "the graph says nothing runs it"
    );
    assert!(facts
        .verify(
            "bench.sh",
            Class::RecordedBenchmark,
            "governance/measurements.md"
        )
        .is_ok());
    assert!(
        !Class::RecordedBenchmark.is_continuous(),
        "a benchmark is measured, not checked"
    );
    assert!(Class::RecordedBenchmark.may_support_a_claim());
    // With no recorded measurement to point at, the excuse is not available.
    assert!(facts
        .verify("bench.sh", Class::RecordedBenchmark, "governance/nope.md")
        .is_err());
    assert!(
        facts
            .verify("bench.sh", Class::RecordedBenchmark, "scripts/gate.sh")
            .is_err(),
        "the witness file must NAME the script"
    );
}

/// External hardware: the excuse requires the script to SKIP honestly when the
/// toolchain is absent. A gate that would simply fail is not "external hardware".
#[test]
fn fixture_external_hardware_requires_a_skip_guard() {
    let f = Fixture::new("external_hw");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "scripts/hw_guarded.sh",
            "#!/usr/bin/env bash\ncommand -v qemu-system-arm >/dev/null || { echo skip; exit 0; }\n",
        )
        .write("scripts/hw_bare.sh", "#!/usr/bin/env bash\nqemu-system-arm -kernel k.elf\n");
    let facts = f.facts();
    assert!(facts
        .verify(
            "hw_guarded.sh",
            Class::ExternalHardware,
            "NEEDS=qemu-system-arm"
        )
        .is_ok());
    assert!(facts
        .verify(
            "hw_bare.sh",
            Class::ExternalHardware,
            "NEEDS=qemu-system-arm"
        )
        .is_err());
}

/// TRANSITIVE DEADNESS — "invoked by something that nothing invokes" is still
/// invoked by nothing. The ledger's probe stopped at one hop and therefore
/// counted the R26-R29 gates as wired.
#[test]
fn fixture_caller_that_nothing_calls_is_manual_not_wired() {
    let f = Fixture::new("transitive_dead");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "scripts/umbrella.sh",
            "#!/usr/bin/env bash\nbash scripts/leaf.sh\n",
        )
        .write("scripts/leaf.sh", "#!/usr/bin/env bash\necho ok\n");
    let facts = f.facts();
    let c = facts.observe("leaf.sh");
    assert_eq!(c, Class::ManualOperator);
    assert!(!c.is_continuous());
    assert!(
        !c.may_support_a_claim(),
        "a human remembering is not verification"
    );
    assert!(facts
        .verify("leaf.sh", Class::ManualOperator, "scripts/umbrella.sh")
        .is_ok());
}

/// The manifest may not OVER-CLAIM: writing `strict_gate` for a script gate.sh
/// does not call is a failure, not a label.
#[test]
fn fixture_manifest_cannot_over_claim() {
    let f = Fixture::new("over_claim");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/ghost.sh", "#!/usr/bin/env bash\necho hi\n");
    let facts = f.facts();
    let e = facts
        .verify("ghost.sh", Class::StrictGate, "scripts/gate.sh")
        .unwrap_err();
    assert!(e.contains("over-claim"), "{e}");
    assert!(facts
        .verify("ghost.sh", Class::CargoTest, "crates/x/tests/t.rs")
        .is_err());
    assert!(facts.verify("ghost.sh", Class::Orphaned, "-").is_ok());
}

/// …and it may not keep a STALE excuse once the script became wired. Drift is
/// gated in both directions, like the env registry and AXON_REFERENCE.md.
#[test]
fn fixture_manifest_cannot_keep_a_stale_excuse() {
    let f = Fixture::new("stale_label");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\n./scripts/nowwired.sh\n",
    )
    .write("scripts/nowwired.sh", "#!/usr/bin/env bash\necho hi\n")
    .write(
        "governance/measurements.md",
        "scripts/nowwired.sh once measured 2x\n",
    );
    let facts = f.facts();
    for (c, w) in [
        (Class::Orphaned, "-"),
        (Class::ManualOperator, "scripts/gate.sh"),
        (Class::RecordedBenchmark, "governance/measurements.md"),
    ] {
        let e = facts.verify("nowwired.sh", c, w).unwrap_err();
        assert!(
            e.contains("stale") || e.contains("must itself be reachable"),
            "{}: {e}",
            c.name()
        );
    }
}

// ---------------------------------------------------------------------------
// End-to-end: the register + the manifest together
// ---------------------------------------------------------------------------

const MINI_MANIFEST_HEADER: &str = "| script | class | witness | note |\n|---|---|---|---|\n";

#[test]
fn fixture_claim_on_an_orphan_fails_the_check() {
    let f = Fixture::new("e2e_orphan");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/r99_gate.sh", "#!/usr/bin/env bash\necho PASS\n")
        .write(
            "governance/REQUIREMENTS.md",
            "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R99 | Thing | ✅ Landed 100% | `scripts/r99_gate.sh` PASS |\n",
        )
        .write(
            "governance/EVIDENCE_CLASSES.md",
            &format!("{MINI_MANIFEST_HEADER}| r99_gate.sh | orphaned | `-` | nothing runs it |\n"),
        );
    let rep = f.check();
    assert_eq!(rep.n_requirements, 1);
    assert_eq!(rep.n_citations, 1);
    assert_eq!(rep.errors.len(), 1, "{:?}", rep.errors);
    assert!(rep.errors[0].contains("R99") && rep.errors[0].contains("orphaned"));
}

#[test]
fn fixture_claim_on_a_wired_gate_passes_the_check() {
    let f = Fixture::new("e2e_wired");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\n./scripts/r99_gate.sh || exit 1\n")
        .write("scripts/r99_gate.sh", "#!/usr/bin/env bash\necho PASS\n")
        .write(
            "governance/REQUIREMENTS.md",
            "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R99 | Thing | ✅ Landed 100% | `scripts/r99_gate.sh` PASS |\n",
        )
        .write(
            "governance/EVIDENCE_CLASSES.md",
            &format!(
                "{MINI_MANIFEST_HEADER}| r99_gate.sh | strict_gate | `scripts/gate.sh` | gate.sh runs it |\n"
            ),
        );
    let rep = f.check();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
}

/// A requirement that does NOT claim verification may cite whatever it likes —
/// the rule is about the CLAIM, not about tidy scripts.
#[test]
fn fixture_no_claim_no_violation() {
    let f = Fixture::new("e2e_no_claim");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/r99_gate.sh", "#!/usr/bin/env bash\necho PASS\n")
        .write(
            "governance/REQUIREMENTS.md",
            "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R99 | Thing | 📝 Draft — not built | planned: `scripts/r99_gate.sh` |\n",
        )
        .write(
            "governance/EVIDENCE_CLASSES.md",
            &format!("{MINI_MANIFEST_HEADER}| r99_gate.sh | orphaned | `-` | nothing runs it |\n"),
        );
    let rep = f.check();
    assert!(rep.errors.is_empty(), "{:?}", rep.errors);
}

/// Adding a requirement must not get "verified" for free: a cited script with no
/// manifest row is an error, not a default.
#[test]
fn fixture_unclassified_evidence_is_an_error() {
    let f = Fixture::new("e2e_unclassified");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\n./scripts/r99_gate.sh\n",
    )
    .write("scripts/r99_gate.sh", "#!/usr/bin/env bash\necho PASS\n")
    .write(
        "governance/REQUIREMENTS.md",
        "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R99 | Thing | ✅ Landed | `scripts/r99_gate.sh` PASS |\n",
    )
    .write("governance/EVIDENCE_CLASSES.md", MINI_MANIFEST_HEADER);
    let rep = f.check();
    assert!(
        rep.errors
            .iter()
            .any(|e| e.contains("no row in governance/EVIDENCE_CLASSES.md")),
        "{:?}",
        rep.errors
    );
}

/// NON-VACUITY FLOOR: parsing nothing is a failure, not a clean run. An empty
/// register with an empty manifest is the state a broken parser produces, and it
/// must never read as "no problems found".
#[test]
fn fixture_vacuous_parse_is_a_failure() {
    let f = Fixture::new("e2e_vacuous");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("governance/REQUIREMENTS.md", "# nothing here\n")
        .write("governance/EVIDENCE_CLASSES.md", MINI_MANIFEST_HEADER);
    let rep = f.check();
    assert_eq!(rep.n_requirements, 0);
    assert_eq!(
        rep.errors
            .iter()
            .filter(|e| e.starts_with("VACUOUS"))
            .count(),
        3,
        "{:?}",
        rep.errors
    );
}

/// A manifest row for a script no requirement cites is stale bookkeeping and
/// fails too — the manifest tracks the register, it does not accumulate.
#[test]
fn fixture_stale_manifest_row_is_an_error() {
    let f = Fixture::new("e2e_stale_row");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\n./scripts/r99_gate.sh\n",
    )
    .write("scripts/r99_gate.sh", "#!/usr/bin/env bash\necho PASS\n")
    .write("scripts/gone.sh", "#!/usr/bin/env bash\necho hi\n")
    .write(
        "governance/REQUIREMENTS.md",
        "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R99 | Thing | ✅ Landed | `scripts/r99_gate.sh` PASS |\n",
    )
    .write(
        "governance/EVIDENCE_CLASSES.md",
        &format!(
            "{MINI_MANIFEST_HEADER}| r99_gate.sh | strict_gate | `scripts/gate.sh` | runs |\n\
                 | gone.sh | orphaned | `-` | cited by nobody |\n"
        ),
    );
    let rep = f.check();
    assert!(
        rep.errors
            .iter()
            .any(|e| e.contains("cited by no requirement")),
        "{:?}",
        rep.errors
    );
}

/// An empty reason is a parse error: a class without a stated reason is a label.
#[test]
fn fixture_manifest_row_needs_a_reason() {
    let e = parse_manifest(&format!(
        "{MINI_MANIFEST_HEADER}| x.sh | orphaned | `-` | - |\n"
    ))
    .unwrap_err();
    assert!(e.contains("written reason"), "{e}");
    assert!(parse_manifest(&format!(
        "{MINI_MANIFEST_HEADER}| x.sh | made_up | `-` | why |\n"
    ))
    .unwrap_err()
    .contains("unknown class"));
}

/// A script named inside an `echo` is advice to the reader, not a caller.
/// `gate.sh` ends by echoing `./scripts/parity_all.sh` as a suggestion, and many
/// scripts print a usage line naming themselves.
#[test]
fn fixture_echo_text_is_not_invocation() {
    let f = Fixture::new("echo_text");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\necho \"to check that, run: bash scripts/advice.sh --all\"\n",
    )
    .write("scripts/advice.sh", "#!/usr/bin/env bash\necho hi\n");
    assert_eq!(f.facts().observe("advice.sh"), Class::Orphaned);

    // …and an echo followed by a REAL invocation on the same line still counts.
    let g = Fixture::new("echo_then_run");
    g.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\necho running && bash scripts/advice.sh\n",
    )
    .write("scripts/advice.sh", "#!/usr/bin/env bash\necho hi\n");
    assert_eq!(g.facts().observe("advice.sh"), Class::StrictGate);
}

// ---------------------------------------------------------------------------
// Provenance and dependency-order fixtures
//
// A flat verdict is not actionable. These pin that every violation says WHY, in
// the graph's own terms, and that violations sharing one upstream cause are
// reported as one fix rather than N.
// ---------------------------------------------------------------------------

/// The orphan must name the nearest thing that mentions it and why that mention
/// does not count. This is `r31_acceptance_gate.sh`'s exact shape: its only
/// mention repo-wide is a comment in another gate.
#[test]
fn fixture_provenance_names_the_rejecting_mention() {
    let f = Fixture::new("prov_comment");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\n./scripts/other_gate.sh\n",
    )
    .write(
        "scripts/other_gate.sh",
        "#!/usr/bin/env bash\n# line two\n# Follows the pattern of scripts/lonely.sh.\necho hi\n",
    )
    .write("scripts/lonely.sh", "#!/usr/bin/env bash\necho hi\n");
    let why = f.facts().explain("lonely.sh").join("\n");
    assert!(why.contains("shortest path from a root: NONE"), "{why}");
    assert!(
        why.contains("scripts/other_gate.sh:3"),
        "must cite file:line — {why}"
    );
    assert!(
        why.contains("COMMENT"),
        "must say WHY it does not count — {why}"
    );
}

/// Each rejection reason is reported as itself, not collapsed into "no caller".
#[test]
fn fixture_provenance_distinguishes_the_rejection_reasons() {
    let f = Fixture::new("prov_kinds");
    f.write(
        "scripts/gate.sh",
        "#!/usr/bin/env bash\nif [ -f \"$ROOT/scripts/a.sh\" ]; then\n  echo \"try bash scripts/b.sh\"\nfi\n",
    )
    .write("scripts/a.sh", "#!/usr/bin/env bash\necho hi\n")
    .write("scripts/b.sh", "#!/usr/bin/env bash\n# b.sh — see scripts/b.sh --help\necho hi\n")
    .write(
        "crates/x/tests/t.rs",
        "#[test]\nfn t() {\n    panic!(\"missing scripts/c.sh — deleted?\");\n}\n",
    )
    .write("scripts/c.sh", "#!/usr/bin/env bash\necho hi\n");
    let facts = f.facts();
    assert!(facts.explain("a.sh").join("\n").contains("EXISTENCE TEST"));
    let b = facts.explain("b.sh").join("\n");
    assert!(b.contains("ECHO TEXT"), "{b}");
    assert!(b.contains("SELF-REFERENCE"), "{b}");
    assert!(facts.explain("c.sh").join("\n").contains("STRING MESSAGE"));
}

/// A reachable script prints the real chain, not just "wired".
#[test]
fn fixture_provenance_prints_path_from_root() {
    let f = Fixture::new("prov_path");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\n./scripts/agg.sh\n")
        .write(
            "scripts/agg.sh",
            "#!/usr/bin/env bash\nfor h in scripts/*_parity.sh; do\n  bash \"$h\"\ndone\n",
        )
        .write("scripts/deep_parity.sh", "#!/usr/bin/env bash\necho PASS\n");
    let facts = f.facts();
    assert_eq!(
        facts.path_from_root("deep_parity.sh").unwrap().join(" -> "),
        "scripts/gate.sh -> scripts/agg.sh -> scripts/deep_parity.sh"
    );
    assert!(facts
        .explain("deep_parity.sh")
        .join("\n")
        .contains("path from root: scripts/gate.sh"));
}

/// A `manual_operator` must name its dead caller AND say why THAT is dead —
/// the R27 shape, where the chain is two deep.
#[test]
fn fixture_provenance_manual_operator_names_the_dead_caller() {
    let f = Fixture::new("prov_manual");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "scripts/umbrella.sh",
            "#!/usr/bin/env bash\nbash scripts/leaf.sh\n",
        )
        .write(
            "scripts/r_other.sh",
            // Names BOTH: the dead caller (so `why_dead` takes its mention-count
            // branch, the one the real axon_safety_gate.sh takes with 9 mentions)
            // and the leaf (so the grep-finds-it case is exercised).
            "#!/usr/bin/env bash\n# leaf.sh mirrors scripts/umbrella.sh\necho hi\n",
        )
        .write("scripts/leaf.sh", "#!/usr/bin/env bash\necho ok\n")
        .write("scripts/nowhere.sh", "#!/usr/bin/env bash\necho ok\n");
    let facts = f.facts();
    let why = facts.explain("leaf.sh").join("\n");
    assert!(why.contains("shortest path from a root: NONE"), "{why}");
    assert!(
        why.contains("reachable only via scripts/umbrella.sh"),
        "{why}"
    );
    assert!(
        why.contains("scripts/umbrella.sh — orphaned: 1 mention(s), none a caller"),
        "must explain the CALLER, including how many mentions misled a grep — {why}"
    );
    // The other branch: a script nothing mentions at all must say exactly that.
    assert!(
        facts
            .explain("nowhere.sh")
            .join("\n")
            .contains("named nowhere under scripts/"),
        "{:?}",
        facts.explain("nowhere.sh")
    );
    // The comment mention elsewhere is still surfaced: a reviewer greps and
    // finds it, so the report must say it is not a caller.
    assert!(
        why.contains("scripts/r_other.sh:2") && why.contains("COMMENT"),
        "{why}"
    );
}

/// DEPENDENCY ORDER: three violations hanging off one unwired umbrella are ONE
/// fix, not three — and the chain is followed transitively, so a leaf two hops
/// down is covered by wiring the top.
#[test]
fn fixture_root_fix_groups_one_wiring_covers_the_subtree() {
    let f = Fixture::new("root_fix");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write(
            "scripts/umbrella.sh",
            "#!/usr/bin/env bash\nbash scripts/mid.sh\n",
        )
        .write(
            "scripts/mid.sh",
            "#!/usr/bin/env bash\nbash scripts/leaf.sh\n",
        )
        .write("scripts/leaf.sh", "#!/usr/bin/env bash\necho ok\n")
        .write("scripts/solo.sh", "#!/usr/bin/env bash\necho ok\n")
        .write(
            "governance/REQUIREMENTS.md",
            "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R90 | A | ✅ Landed | `scripts/umbrella.sh` PASS |\n\
             | R91 | B | ✅ Landed | `scripts/mid.sh` PASS |\n\
             | R92 | C | ✅ Landed | `scripts/leaf.sh` PASS |\n\
             | R93 | D | ✅ Landed | `scripts/solo.sh` PASS |\n",
        )
        .write(
            "governance/EVIDENCE_CLASSES.md",
            &format!(
                "{MINI_MANIFEST_HEADER}\
                 | umbrella.sh | orphaned | `-` | nothing runs it |\n\
                 | mid.sh | manual_operator | `scripts/umbrella.sh` | dead caller |\n\
                 | leaf.sh | manual_operator | `scripts/mid.sh` | dead caller |\n\
                 | solo.sh | orphaned | `-` | nothing runs it |\n"
            ),
        );
    let rep = f.check();
    assert_eq!(rep.errors.len(), 4, "{:?}", rep.errors);
    assert_eq!(rep.root_fixes.len(), 2, "{:?}", rep.root_fixes);
    assert_eq!(rep.root_fixes[0].0, "scripts/umbrella.sh");
    assert_eq!(
        rep.root_fixes[0].1,
        vec!["leaf.sh".to_string(), "mid.sh".into(), "umbrella.sh".into()],
        "one wiring must cover the whole subtree, two hops down"
    );
    assert_eq!(rep.root_fixes[1].0, "scripts/solo.sh");
    let rendered = render_root_fixes(&rep.root_fixes);
    assert!(rendered.contains("covers 3:"), "{rendered}");
    assert!(
        rendered.contains("2 upstream wiring(s) would clear all 4"),
        "{rendered}"
    );
}

/// …and the grouping must not fire when the violations are genuinely independent.
#[test]
fn fixture_root_fix_does_not_invent_a_shared_cause() {
    let f = Fixture::new("root_fix_none");
    f.write("scripts/gate.sh", "#!/usr/bin/env bash\necho hi\n")
        .write("scripts/a_gate.sh", "#!/usr/bin/env bash\necho ok\n")
        .write("scripts/b_gate.sh", "#!/usr/bin/env bash\necho ok\n")
        .write(
            "governance/REQUIREMENTS.md",
            "| # | Req | Status | Evidence |\n|---|---|---|---|\n\
             | R90 | A | ✅ Landed | `scripts/a_gate.sh` PASS |\n\
             | R91 | B | ✅ Landed | `scripts/b_gate.sh` PASS |\n",
        )
        .write(
            "governance/EVIDENCE_CLASSES.md",
            &format!(
                "{MINI_MANIFEST_HEADER}\
                 | a_gate.sh | orphaned | `-` | nothing runs it |\n\
                 | b_gate.sh | orphaned | `-` | nothing runs it |\n"
            ),
        );
    let rep = f.check();
    assert_eq!(
        rep.root_fixes.len(),
        2,
        "two causes, two fixes: {:?}",
        rep.root_fixes
    );
}
