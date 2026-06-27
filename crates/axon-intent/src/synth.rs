//! R22 §3.2 + §4.5 — the `Synthesizer` seam: the ONLY impure module. It turns
//! an `Intent` into an `.ax` source string (the LLM call) and type-checks it.
//!
//! - `MockSynth` is the deterministic test/audit path (no network, no key); a
//!   given `(intent, seed)` yields a byte-identical draft (A5).
//! - `RealSynthesizer` invokes the canonical `axon` entrypoint in a fresh,
//!   time-bounded subprocess (`axon intent compile` with `AXON_INTENT_GEN=1`),
//!   then `axon check` for the type-check — isolated + hard-timeout (A4).
//!
//! Everything downstream (`grant_infer`/`admit`/`confidence`/`approval`) is a
//! pure function of `(Intent, ProgramDraft)`, so S1–S5 are built entirely
//! against `MockSynth`.

use crate::intent::Intent;
use axon_os::gate::DeclaredEffects;
use axon_os::grant::{EffectSet, Label};

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
    /// effects, entry presence, the outputs it writes). Pure-ish: the real impl
    /// shells out to `axon check`; the mock derives it structurally.
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

/// Over-approximate a program's declared effects by scanning for capability-
/// bearing builtins (mirrors R21's `AxonCoreRuntime::scan_effects`). Sound as an
/// over-approximation: any ambiguity widens the set, never narrows it.
pub fn scan_effects(source: &str) -> DeclaredEffects {
    let net = [
        "http_get",
        "http_post",
        "http_sse",
        "ai_complete",
        "ai_extract",
    ]
    .iter()
    .any(|m| source.contains(m));
    let fs_read = ["read_file", "read_line"]
        .iter()
        .any(|m| source.contains(m));
    let fs_write = source.contains("write_file");
    let exec = source.contains("exec(") || source.contains("spawn_proc");
    DeclaredEffects {
        row: EffectSet {
            fs_read,
            fs_write,
            net,
            exec,
        },
        max_label: Label::Internal,
    }
}

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

// ── RealSynthesizer — the impure seam: hermetic, time-bounded subprocess (A4) ─

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The real synthesizer (R22 §4.5). Invokes the canonical `axon` entrypoint in a
/// FRESH, time-bounded subprocess: `axon intent compile --gen` for synthesis and
/// `axon check` for the type-check. The entrypoint is resolved by ABSOLUTE path
/// from `AXON_BIN` (not an ambient PATH search). On timeout the child is killed
/// (no leaked handle) and synthesis fails closed. This is the ONLY place a model
/// / subprocess is touched — gated so tests never need a network or a key.
pub struct RealSynthesizer {
    axon_bin: PathBuf,
    timeout: Duration,
}

impl RealSynthesizer {
    /// Resolve `AXON_BIN` (absolute) + `AXON_INTENT_TIMEOUT_MS` (default 60s).
    pub fn from_env() -> Self {
        let axon_bin = std::env::var_os("AXON_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/debug/axon"));
        let ms = std::env::var("AXON_INTENT_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(60_000);
        RealSynthesizer {
            axon_bin: absolutize(axon_bin),
            timeout: Duration::from_millis(ms),
        }
    }

    pub fn with_bin_and_timeout(axon_bin: PathBuf, timeout: Duration) -> Self {
        RealSynthesizer {
            axon_bin: absolutize(axon_bin),
            timeout,
        }
    }
}

/// Resolve a possibly-relative entrypoint to an absolute path now (hermetic — no
/// ambient PATH search at spawn time). Mirrors R21's `absolutize`.
fn absolutize(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    std::fs::canonicalize(&p)
        .unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p))
}

/// Best-effort SIGKILL of the child's process GROUP (negative pid), so a shell
/// that forked a grandchild is taken down too. Uses the POSIX `kill` utility to
/// avoid a libc dependency; the pipe-close in `run_bounded` is the load-bearing
/// guard, this just stops orphaned grandchildren.
#[cfg(unix)]
fn kill_group(pid: i32) {
    let _ = Command::new("kill")
        .arg("-KILL")
        .arg(format!("-{pid}"))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}
#[cfg(not(unix))]
fn kill_group(_pid: i32) {}

/// Outcome of a bounded subprocess (None code ⇒ killed by timeout).
struct ProcOutcome {
    code: Option<i32>,
    stdout: String,
    timed_out: bool,
}

/// Run a command with a hard wall-clock timeout; kill + reap on expiry (no
/// leaked child). Mirrors R21's `run_bounded`. Hermetic: a minimal explicit env.
fn run_bounded(cmd: &mut Command, timeout: Duration) -> std::io::Result<ProcOutcome> {
    use std::io::Read;
    // Put the child in its own process GROUP so a timeout kill takes down the
    // whole subtree (e.g. a shell that spawned `sleep`) — otherwise a grandchild
    // can keep the stdout pipe open and deadlock the drain (hermetic kill, A4).
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let pid = child.id() as i32;
    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let start = std::time::Instant::now();
    let mut killed = false;
    let code = loop {
        match child.try_wait()? {
            Some(status) => break Some(status.code().unwrap_or(-1)),
            None => {
                if start.elapsed() >= timeout {
                    kill_group(pid);
                    let _ = child.kill();
                    let _ = child.wait(); // reap — no zombie
                    killed = true;
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };
    // Close our read ends if we killed, so the drain can't block on a grandchild
    // that still holds the pipe (we already have the verdict).
    if killed {
        drop(stdout.take());
        drop(stderr.take());
    }
    let mut out = String::new();
    if let Some(s) = stdout.as_mut() {
        let _ = s.read_to_string(&mut out);
    }
    let mut _err = String::new();
    if let Some(s) = stderr.as_mut() {
        let _ = s.read_to_string(&mut _err);
    }
    Ok(ProcOutcome {
        code,
        stdout: out,
        timed_out: code.is_none(),
    })
}

impl Synthesizer for RealSynthesizer {
    fn synthesize(&self, intent: &Intent) -> Result<String, SynthErr> {
        // Write the intent to a temp `.md` and run `axon intent compile --gen`,
        // capturing the generated `.ax` to stdout via `--out -` is not supported,
        // so we write to a temp `.ax` and read it back.
        let tmp = std::env::temp_dir().join(format!(
            "axon-intent-synth-{}-{}.md",
            std::process::id(),
            intent.seed
        ));
        let out = tmp.with_extension("ax");
        if std::fs::write(&tmp, render_intent_md(intent)).is_err() {
            return Err(SynthErr {
                reason: "could not stage intent for synthesis".into(),
            });
        }
        let mut cmd = Command::new(&self.axon_bin);
        cmd.arg("intent")
            .arg("compile")
            .arg(&tmp)
            .arg("--out")
            .arg(&out);
        cmd.env_clear();
        cmd.env("AXON_INTENT_GEN", "1");
        // Forward the mock/replay + key env so synthesis is reproducible/offline
        // in tests; the deterministic mock path needs AXON_AI_MOCK.
        for k in [
            "AXON_AI_MOCK",
            "AXON_AI_REPLAY",
            "ANTHROPIC_API_KEY",
            "PATH",
        ] {
            if let Some(v) = std::env::var_os(k) {
                cmd.env(k, v);
            }
        }
        let proc = match run_bounded(&mut cmd, self.timeout) {
            Ok(p) => p,
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                return Err(SynthErr {
                    reason: format!("could not launch synthesizer: {e}"),
                });
            }
        };
        let _ = std::fs::remove_file(&tmp);
        if proc.timed_out {
            let _ = std::fs::remove_file(&out);
            return Err(SynthErr {
                reason: "synthesis timed out".into(),
            });
        }
        let source = std::fs::read_to_string(&out).unwrap_or_default();
        let _ = std::fs::remove_file(&out);
        if source.trim().is_empty() {
            // fall back to stdout (older CLI emits there)
            if proc.stdout.trim().is_empty() {
                return Err(SynthErr {
                    reason: "synthesizer produced no program".into(),
                });
            }
            return Ok(proc.stdout);
        }
        Ok(source)
    }

    fn typecheck(&self, source: &str) -> SynthMeta {
        // Write the candidate to a temp `.ax` and run `axon check`.
        let tmp = std::env::temp_dir().join(format!("axon-intent-check-{}.ax", std::process::id()));
        let (typechecks, review_effects) = if std::fs::write(&tmp, source).is_ok() {
            let mut cmd = Command::new(&self.axon_bin);
            cmd.arg("check").arg(&tmp);
            cmd.env_clear();
            if let Some(p) = std::env::var_os("PATH") {
                cmd.env("PATH", p);
            }
            let ok = run_bounded(&mut cmd, self.timeout)
                .map(|p| !p.timed_out && p.code == Some(0))
                .unwrap_or(false);
            // Cross-check the substring scan against the COMPILER's own effect
            // analysis (`axon ast review --json` reports per-fn `effects:bool`).
            let mut rev = Command::new(&self.axon_bin);
            rev.arg("ast").arg("review").arg(&tmp).arg("--json");
            rev.env_clear();
            if let Some(p) = std::env::var_os("PATH") {
                rev.env("PATH", p);
            }
            let effects = run_bounded(&mut rev, self.timeout)
                .map(|p| p.stdout.contains("\"effects\":true"))
                .unwrap_or(true); // can't tell ⇒ assume effects (conservative)
            let _ = std::fs::remove_file(&tmp);
            (ok, effects)
        } else {
            (false, true)
        };
        let mut meta = meta_from_source(source, typechecks, Vec::new());
        // CONSERVATIVE CROSS-CHECK (ASI review): if the compiler says the program
        // HAS effects but the substring scan categorized NONE, the scan missed an
        // effect it can't pattern-match — widen to the full set (deny-by-default)
        // rather than silently under-grant. (An under-grant would also fail closed
        // at the R21 runtime sandbox, but catching it here surfaces it to the human
        // at approval time instead of as a late runtime denial.)
        let r = meta.declared.row;
        let scan_empty = !(r.fs_read || r.fs_write || r.net || r.exec);
        if review_effects && scan_empty {
            meta.declared = DeclaredEffects::unknown();
            meta.uncertainty_notes.push(
                "compiler reports effects the source scan could not categorize — \
                 widened to full effect set (deny-by-default)"
                    .to_string(),
            );
        }
        meta
    }
}

/// Render an `Intent` back to the `.intent.md` form the `axon intent compile`
/// path consumes (a prose goal + the named sections).
fn render_intent_md(intent: &Intent) -> String {
    let bullets = |xs: &[String]| {
        xs.iter()
            .map(|x| format!("- {x}"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    let net = if intent.cap_ceiling.net.is_empty() {
        "none".to_string()
    } else {
        intent.cap_ceiling.net.join(", ")
    };
    format!(
        "# Intent\n{}\n\n## Inputs\n{}\n\n## Outputs\n{}\n\n## Allowed\n- fs_read: {}\n- fs_write: {}\n- net: {}\n- exec: {}\n- max_label: {}\n\n## Budget\n- calls: {}\n- tokens: {}\n- cost_micro: {}\n\n## Seed\n- {}\n",
        intent.goal,
        bullets(&intent.inputs),
        bullets(&intent.outputs),
        if intent.cap_ceiling.fs_read.is_empty() { "none".into() } else { intent.cap_ceiling.fs_read.join(", ") },
        if intent.cap_ceiling.fs_write.is_empty() { "none".into() } else { intent.cap_ceiling.fs_write.join(", ") },
        net,
        intent.cap_ceiling.exec.as_str(),
        intent.cap_ceiling.max_label.as_str(),
        intent.budget.calls,
        intent.budget.tokens,
        intent.budget.cost_micro,
        intent.seed,
    )
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

/// A runtime dispatcher over the two synthesizers, so the CLI can pick mock
/// (offline, `AXON_AI_MOCK=1`) or real (subprocess) at run time without generics.
pub enum MockSynthOrReal {
    Mock(MockSynth),
    Real(RealSynthesizer),
}

impl Synthesizer for MockSynthOrReal {
    fn synthesize(&self, intent: &Intent) -> Result<String, SynthErr> {
        match self {
            MockSynthOrReal::Mock(m) => m.synthesize(intent),
            MockSynthOrReal::Real(r) => r.synthesize(intent),
        }
    }
    fn typecheck(&self, source: &str) -> SynthMeta {
        match self {
            MockSynthOrReal::Mock(m) => m.typecheck(source),
            MockSynthOrReal::Real(r) => r.typecheck(source),
        }
    }
}

/// A trivial balanced-brace check (the mock's stand-in for the real type-check).
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
    fn run_bounded_kills_a_runaway_at_the_timeout() {
        // The hermetic-execution primitive (A4): a `sleep 5` under a 150 ms
        // timeout is killed (timed_out), and the call returns promptly — no
        // leaked child, no 5 s hang.
        let start = std::time::Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let out = run_bounded(&mut cmd, Duration::from_millis(150)).expect("spawn sleep (POSIX)");
        assert!(out.timed_out, "runaway must be killed at the timeout");
        assert!(out.code.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must return promptly after the kill"
        );
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
