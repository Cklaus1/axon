//! IG4 - the check->repair loop: the only way a generated program reaches
//! approval.
//!
//! Round 0 asks the [`Generator`] for a program from the language card, the
//! intent and its visible examples. Every candidate then goes through
//! [`evaluate_candidate`]:
//!
//! 1. fences stripped, the text written to a private scratch directory;
//! 2. `axon check --json`: an `error` diagnostic (or a non-zero exit) rejects
//!    the candidate and its `axon-diag/1` lines become the feedback (capped at
//!    [`FEEDBACK_CAP_BYTES`]); warnings never reject;
//! 3. the full pipeline gates ([`pipeline::resolve_draft`]: confidence, the
//!    least-privilege grant, the admissibility proof); a refusal is fed back;
//! 4. every visible example run in the axon-os sandbox with the CEILING's
//!    allowlists ([`AxonCoreRuntime::run_sandboxed_capture`]): only the run's
//!    input fixtures are staged next to the program (never an expected
//!    output), and each mismatch is fed back as a diff.
//!
//! A candidate is accepted only when 2-4 are all clean; at most
//! [`MAX_REPAIR_ROUNDS`] repair rounds follow round 0. A capability refusal
//! (the gates refusing an axis the ceiling forbids, or the sandbox refusing a
//! path or host during an example) is fed back once; the second ends the loop
//! (Q4). The loop never edits a candidate and never returns one that did not
//! pass every step: on exhaustion there is no program, only the transcript
//! ([`TRANSCRIPT_SCHEMA`]).
//!
//! Arm (b) of the benchmark is round 0 of this loop: `transcript.rounds[0]`
//! carries the first candidate and its full evaluation, so no second
//! generation call exists.

use crate::generator::Generator;
use crate::intent::{self, Example, Intent};
use crate::pipeline::{self, Resolved};
use crate::refusal::Refusal;
use crate::synth::{meta_from_source, strip_fences, ProgramDraft};
use axon_os::grant::{Budget, ExecPolicy, Grant};
use axon_os::runtime::{AxonCoreRuntime, ProgramSource, SandboxOutcome};
use axon_os::verdict::Verdict;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

/// The transcript's schema tag.
pub const TRANSCRIPT_SCHEMA: &str = "axon-intent-gen/1";
/// Repair rounds after round 0 (so at most 1 + 3 generation calls).
pub const MAX_REPAIR_ROUNDS: u32 = 3;
/// The most diagnostic (or example-diff) text fed back per round (PREREG-gencontext).
pub const FEEDBACK_CAP_BYTES: usize = 2048;
/// Deadline of one `axon check`.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(60);
/// Wall deadline of one example run in the sandbox.
pub const EXAMPLE_TIMEOUT: Duration = Duration::from_secs(20);
/// The file name a candidate is staged under.
pub const CANDIDATE_FILE: &str = "main.ax";

/// What the loop needs besides the intent and the generator.
#[derive(Debug, Clone)]
pub struct LoopConfig {
    /// The `axon` binary that checks and runs candidates (absolute).
    pub axon_bin: PathBuf,
    /// The language card (`axon card`), verbatim; its sha256 is recorded.
    pub card: String,
    /// Repair rounds after round 0; values above [`MAX_REPAIR_ROUNDS`] are
    /// clamped to it.
    pub max_repair_rounds: u32,
    pub check_timeout: Duration,
    pub example_timeout: Duration,
}

impl LoopConfig {
    pub fn new(axon_bin: PathBuf, card: String) -> Self {
        LoopConfig {
            axon_bin,
            card,
            max_repair_rounds: MAX_REPAIR_ROUNDS,
            check_timeout: CHECK_TIMEOUT,
            example_timeout: EXAMPLE_TIMEOUT,
        }
    }
}

/// A capability refusal (Q4): fed back once, the second ends the loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RefusalClass {
    /// The gates refused: the program needs an axis the ceiling forbids.
    StaticAxis,
    /// The sandbox refused a path or host while an example ran.
    RuntimeScope,
}

/// Where a candidate stopped (or `Accepted`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Stage {
    /// `axon check` reported an error.
    Check,
    /// The pipeline gates refused it.
    Gates,
    /// An example run did not match.
    Examples,
    /// Every step was clean.
    Accepted,
    /// The checker or the sandbox could not run (not a property of the program).
    Infrastructure,
}

/// One example run's result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExampleResult {
    pub example: String,
    /// 1-based run number within the example.
    pub run: usize,
    pub passed: bool,
    /// How the run ended (`exit 0`, `faulted: ...`, `timed out after N ms`).
    pub ended: String,
    /// Mismatches against the expectation (empty when `passed`).
    pub diffs: Vec<String>,
    /// The last lines of stderr.
    pub stderr_tail: String,
}

/// The evaluation of one candidate (steps 1-4).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CandidateReport {
    pub stage: Stage,
    /// The program after fence stripping, exactly as checked and run.
    pub source: String,
    pub fences_stripped: bool,
    /// `axon check` passed (exit 0, no error diagnostic).
    pub check_ok: bool,
    /// The `axon-diag/1` error lines `axon check` printed.
    pub diagnostics: Vec<String>,
    /// How many warnings `axon check` printed (never a rejection).
    pub warnings: usize,
    /// The gates' refusal, legible.
    pub gate_refusal: Option<String>,
    pub examples: Vec<ExampleResult>,
    pub refusal_class: Option<RefusalClass>,
    /// What the next round's prompt tells the generator (None when accepted).
    pub feedback: Option<String>,
    /// Infrastructure failure detail (stage `Infrastructure`).
    pub infrastructure: Option<String>,
}

/// One round of the loop in the transcript.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RoundRecord {
    /// 0 = the first candidate (arm b).
    pub round: u32,
    pub prompt_sha256: String,
    /// The generator's answer, verbatim.
    pub candidate: Option<String>,
    pub tokens: Option<u64>,
    pub generator_failure: Option<String>,
    pub wall_ms: u64,
    pub report: Option<CandidateReport>,
}

/// How the loop ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum LoopOutcome {
    Accepted,
    /// Every round was used without a clean candidate.
    Exhausted,
    /// A second capability refusal ended the loop (Q4).
    RefusedTwice,
    /// The generator produced no candidate (timeout, error).
    GeneratorFailed,
    /// The checker or the sandbox could not run.
    InfrastructureFailed,
}

/// The `axon-intent-gen/1` transcript: everything the loop saw and decided.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Transcript {
    pub schema: &'static str,
    pub generator: String,
    pub card_sha256: String,
    pub intent_goal: String,
    pub max_repair_rounds: u32,
    pub rounds: Vec<RoundRecord>,
    pub refusal_classes: Vec<RefusalClass>,
    pub outcome: LoopOutcome,
    pub wall_ms: u64,
    /// Sum of reported tokens; `None` unless every call reported them.
    pub tokens: Option<u64>,
}

impl Transcript {
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("the transcript serialises")
    }
}

/// The loop's result: the gated, accepted program (ready for approval), or a
/// refusal - and always the transcript.
pub struct LoopResult {
    pub resolved: Result<Resolved, Refusal>,
    pub transcript: Transcript,
}

/// Run the check->repair loop (IG4).
pub fn run(intent: &Intent, generator: &dyn Generator, cfg: &LoopConfig) -> LoopResult {
    let start = Instant::now();
    let repairs = cfg.max_repair_rounds.min(MAX_REPAIR_ROUNDS);
    let mut rounds: Vec<RoundRecord> = Vec::new();
    let mut classes: Vec<RefusalClass> = Vec::new();
    let mut tokens: Option<u64> = Some(0);
    let mut previous: Option<(String, String)> = None; // (source, feedback)
    let mut outcome = LoopOutcome::Exhausted;
    let mut accepted: Option<Resolved> = None;

    for round in 0..=repairs {
        let prompt = match &previous {
            None => round0_prompt(intent, &cfg.card),
            Some((src, fb)) => repair_prompt(intent, &cfg.card, src, fb),
        };
        let t = Instant::now();
        let mut rec = RoundRecord {
            round,
            prompt_sha256: sha256_hex(prompt.as_bytes()),
            candidate: None,
            tokens: None,
            generator_failure: None,
            wall_ms: 0,
            report: None,
        };
        let gen = generator.generate(&prompt);
        let generation = match gen {
            Ok(g) => g,
            Err(f) => {
                rec.generator_failure = Some(f.to_string());
                rec.wall_ms = ms(t.elapsed());
                rounds.push(rec);
                tokens = None;
                outcome = LoopOutcome::GeneratorFailed;
                break;
            }
        };
        tokens = match (tokens, generation.tokens) {
            (Some(a), Some(b)) => Some(a + b),
            _ => None,
        };
        rec.tokens = generation.tokens;
        let (report, resolved) = evaluate_candidate(intent, &generation.text, cfg);
        rec.candidate = Some(generation.text);
        rec.wall_ms = ms(t.elapsed());
        let stage = report.stage;
        if let Some(c) = report.refusal_class {
            classes.push(c);
        }
        let feedback = report.feedback.clone();
        let source = report.source.clone();
        rec.report = Some(report);
        rounds.push(rec);
        match stage {
            Stage::Accepted => {
                outcome = LoopOutcome::Accepted;
                accepted = resolved;
                break;
            }
            Stage::Infrastructure => {
                outcome = LoopOutcome::InfrastructureFailed;
                break;
            }
            _ => {}
        }
        if classes.len() >= 2 {
            outcome = LoopOutcome::RefusedTwice;
            break;
        }
        previous = Some((source, feedback.unwrap_or_default()));
    }

    let transcript = Transcript {
        schema: TRANSCRIPT_SCHEMA,
        generator: generator.id(),
        card_sha256: sha256_hex(cfg.card.as_bytes()),
        intent_goal: intent.goal.clone(),
        max_repair_rounds: repairs,
        rounds,
        refusal_classes: classes,
        outcome,
        wall_ms: ms(start.elapsed()),
        tokens,
    };
    let resolved = match accepted {
        Some(r) if outcome == LoopOutcome::Accepted => Ok(r),
        _ => Err(Refusal::LowConfidence {
            reason: refusal_reason(&transcript),
            score: 0,
        }),
    };
    LoopResult {
        resolved,
        transcript,
    }
}

fn refusal_reason(t: &Transcript) -> String {
    let n = t.rounds.len();
    let last = t.rounds.last();
    let detail = last
        .and_then(|r| {
            r.generator_failure.clone().or_else(|| {
                r.report
                    .as_ref()
                    .and_then(|p| p.infrastructure.clone().or_else(|| p.feedback.clone()))
            })
        })
        .unwrap_or_default();
    let first_line = detail.lines().next().unwrap_or("").to_string();
    let what = match t.outcome {
        LoopOutcome::Accepted => "accepted",
        LoopOutcome::Exhausted => "no candidate passed check, gates and examples",
        LoopOutcome::RefusedTwice => "a second capability refusal ended generation",
        LoopOutcome::GeneratorFailed => "the generator produced no candidate",
        LoopOutcome::InfrastructureFailed => "the checker or the sandbox could not run",
    };
    format!("generation refused after {n} round(s): {what}; last: {first_line}")
}

/// Evaluate one generator answer (steps 1-4). Returns the report and, when
/// the candidate is accepted, the gated result ready for approval. Public so
/// the benchmark grades arm (b) and hidden examples through the same code.
pub fn evaluate_candidate(
    intent: &Intent,
    answer: &str,
    cfg: &LoopConfig,
) -> (CandidateReport, Option<Resolved>) {
    let (source, fences_stripped) = strip_fences(answer);
    let mut rep = CandidateReport {
        stage: Stage::Check,
        source: source.clone(),
        fences_stripped,
        check_ok: false,
        diagnostics: Vec::new(),
        warnings: 0,
        gate_refusal: None,
        examples: Vec::new(),
        refusal_class: None,
        feedback: None,
        infrastructure: None,
    };
    let infra = |mut rep: CandidateReport, why: String| {
        rep.stage = Stage::Infrastructure;
        rep.infrastructure = Some(why);
        (rep, None)
    };

    // 1-2. stage + check.
    let scratch = match Scratch::new("check") {
        Ok(s) => s,
        Err(e) => return infra(rep, format!("cannot create a scratch dir: {e}")),
    };
    let check = match check_source(&cfg.axon_bin, &source, scratch.path(), cfg.check_timeout) {
        Ok(c) => c,
        Err(e) => return infra(rep, e),
    };
    drop(scratch);
    rep.check_ok = check.ok;
    rep.diagnostics = check.errors.clone();
    rep.warnings = check.warnings;
    if !check.ok {
        rep.feedback = Some(format!(
            "`axon check` rejected the program:\n{}",
            cap_lines(&check.errors, FEEDBACK_CAP_BYTES)
        ));
        return (rep, None);
    }

    // 3. the pipeline gates.
    rep.stage = Stage::Gates;
    let draft = ProgramDraft {
        source: source.clone(),
        generated: true,
        fences_stripped,
        meta: meta_from_source(&source, true, Vec::new()),
    };
    let resolved = match pipeline::resolve_draft(intent, draft, None) {
        Ok(r) => r,
        Err(r) => {
            rep.refusal_class = match r {
                Refusal::GrantExceedsCeiling { .. } | Refusal::NotAdmissible { .. } => {
                    Some(RefusalClass::StaticAxis)
                }
                _ => None,
            };
            rep.gate_refusal = Some(r.legible());
            rep.feedback = Some(format!(
                "The admission gates refused the program (the authority below is imposed from \
                 outside and never widens; change the program, not its authority):\n{}",
                r.legible()
            ));
            return (rep, None);
        }
    };

    // 4. the visible examples, in the sandbox with the ceiling's allowlists.
    rep.stage = Stage::Examples;
    let grant = ceiling_grant(intent);
    let rt = AxonCoreRuntime::with_bin_and_timeout(cfg.axon_bin.clone(), cfg.example_timeout);
    let mut failed = false;
    for ex in &intent.examples {
        match run_example(&rt, ex, &source, &grant, &intent.budget, intent.seed) {
            Ok(results) => {
                for r in &results {
                    if !r.passed {
                        failed = true;
                    }
                }
                if results
                    .iter()
                    .any(|r| r.ended.starts_with(SANDBOX_REFUSAL_PREFIX))
                {
                    rep.refusal_class = Some(RefusalClass::RuntimeScope);
                }
                rep.examples.extend(results);
            }
            Err(e) => return infra(rep, e),
        }
    }
    if failed {
        let mut lines = Vec::new();
        for r in rep.examples.iter().filter(|r| !r.passed) {
            lines.push(format!(
                "example `{}` run {}: {}",
                r.example, r.run, r.ended
            ));
            for d in &r.diffs {
                lines.push(format!("  {d}"));
            }
            if !r.stderr_tail.is_empty() {
                lines.push(format!("  stderr: {}", r.stderr_tail));
            }
        }
        rep.feedback = Some(format!(
            "The program type-checks but did not do what the examples expect:\n{}",
            cap_lines(&lines, FEEDBACK_CAP_BYTES)
        ));
        return (rep, None);
    }
    rep.stage = Stage::Accepted;
    (rep, Some(resolved))
}

/// The result of `axon check --json` on one source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Exit 0 and no `error` diagnostic.
    pub ok: bool,
    /// The `axon-diag/1` lines of severity `error` (or the raw output when the
    /// checker failed without one).
    pub errors: Vec<String>,
    pub warnings: usize,
}

/// Write `source` to `dir`/[`CANDIDATE_FILE`] and run `axon check --json` on
/// it. `Err` = the checker could not run (an infrastructure failure).
pub fn check_source(
    axon_bin: &Path,
    source: &str,
    dir: &Path,
    timeout: Duration,
) -> Result<CheckReport, String> {
    let file = dir.join(CANDIDATE_FILE);
    std::fs::write(&file, source).map_err(|e| format!("cannot stage the candidate: {e}"))?;
    let mut cmd = Command::new(axon_bin);
    cmd.arg("check")
        .arg("--json")
        .arg(CANDIDATE_FILE)
        .current_dir(dir)
        .env_clear();
    if let Some(p) = std::env::var_os("PATH") {
        cmd.env("PATH", p);
    }
    let run = axon_os::run_bounded_job(&mut cmd, timeout, None, 1 << 20)
        .map_err(|e| format!("cannot run {} check: {e}", axon_bin.display()))?;
    if run.timed_out {
        return Err(format!("`axon check` timed out after {} ms", ms(timeout)));
    }
    let mut errors = Vec::new();
    let mut warnings = 0;
    for line in run.stderr.lines().chain(run.stdout.lines()) {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line.trim()) else {
            continue;
        };
        if v.get("schema").and_then(|s| s.as_str()) != Some("axon-diag/1") {
            continue;
        }
        match v.get("severity").and_then(|s| s.as_str()) {
            Some("error") => errors.push(line.trim().to_string()),
            Some("warning") => warnings += 1,
            _ => {}
        }
    }
    let ok = run.code == Some(0) && errors.is_empty();
    if !ok && errors.is_empty() {
        // Rejected without a structured diagnostic: feed back what it printed.
        let raw: Vec<String> = run
            .stderr
            .lines()
            .chain(run.stdout.lines())
            .filter(|l| !l.trim().is_empty())
            .map(str::to_string)
            .collect();
        errors = if raw.is_empty() {
            vec![format!(
                "`axon check` exited {:?} with no diagnostic",
                run.code
            )]
        } else {
            raw
        };
    }
    Ok(CheckReport {
        ok,
        errors,
        warnings,
    })
}

/// The grant examples run under: the ceiling's allowlists (Q5), so the loop
/// accepts only what the runtime will allow under the intent's authority.
pub fn ceiling_grant(intent: &Intent) -> Grant {
    let c = &intent.cap_ceiling;
    Grant {
        fs_read: c.fs_read.clone(),
        fs_write: c.fs_write.clone(),
        net: c.net.clone(),
        exec: c.exec,
        max_label: c.max_label,
        budget: intent.budget,
        reproducible: false,
    }
}

const SANDBOX_REFUSAL_PREFIX: &str = "refused by the sandbox";

/// Run one example: a fresh private directory holding the program and, before
/// each run, that run's input fixtures; runs share the directory (stateful
/// examples). Expected outputs are never written there. `Err` = the sandbox
/// could not start a run.
fn run_example(
    rt: &AxonCoreRuntime,
    ex: &Example,
    source: &str,
    grant: &Grant,
    budget: &Budget,
    seed: u64,
) -> Result<Vec<ExampleResult>, String> {
    let dir = Scratch::new("example").map_err(|e| format!("cannot create a scratch dir: {e}"))?;
    let program = dir.path().join(CANDIDATE_FILE);
    std::fs::write(&program, source).map_err(|e| format!("cannot stage the program: {e}"))?;
    // The directories the ceiling lets the program write into exist, empty, as
    // they would where the job runs (`write_file` does not create parents).
    for prefix in &grant.fs_write {
        if !prefix.ends_with('/') {
            continue;
        }
        if let Some(rel) = intent::normalized_example_path(prefix) {
            if !rel.is_empty() {
                std::fs::create_dir_all(dir.path().join(rel))
                    .map_err(|e| format!("cannot create {prefix}: {e}"))?;
            }
        }
    }
    let mut results = Vec::new();
    for (i, run) in ex.runs.iter().enumerate() {
        for f in &run.inputs {
            let rel = intent::normalized_example_path(&f.path)
                .ok_or_else(|| format!("fixture path `{}` is not relative", f.path))?;
            let p = dir.path().join(rel);
            if let Some(parent) = p.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("cannot stage {}: {e}", f.path))?;
            }
            std::fs::write(&p, &f.contents).map_err(|e| format!("cannot stage {}: {e}", f.path))?;
        }
        let cap = rt.run_sandboxed_capture(
            &ProgramSource {
                path: &program,
                text: source,
            },
            grant,
            budget,
            seed,
        );
        let mut diffs = Vec::new();
        let ended = match &cap.outcome {
            SandboxOutcome::NotRun { reason } => {
                return Err(format!("the sandbox could not start the run: {reason}"))
            }
            SandboxOutcome::CompileFailed { diagnostic } => {
                diffs.push(format!(
                    "expected exit {}, the program did not compile",
                    run.exit_code
                ));
                format!("did not compile: {diagnostic}")
            }
            SandboxOutcome::Exited { code } => {
                if *code != run.exit_code {
                    diffs.push(format!("exit: expected {}, got {code}", run.exit_code));
                }
                format!("exit {code}")
            }
            SandboxOutcome::Faulted { verdict, code } => {
                diffs.push(format!(
                    "expected exit {}, the run stopped before `main` returned",
                    run.exit_code
                ));
                match verdict {
                    Verdict::Denied { axis, reason } if axis == "sandbox" => {
                        format!("{SANDBOX_REFUSAL_PREFIX}: {reason}")
                    }
                    v => format!("faulted (status {code:?}): {}", v.legible()),
                }
            }
            SandboxOutcome::TimedOut { after_ms } => {
                diffs.push(format!(
                    "expected exit {}, the run was killed",
                    run.exit_code
                ));
                format!("timed out after {after_ms} ms")
            }
        };
        for f in &run.outputs {
            let got = intent::normalized_example_path(&f.path)
                .map(|rel| dir.path().join(rel))
                .and_then(|p| std::fs::read(p).ok());
            match got {
                None => diffs.push(format!(
                    "output {}: expected {}, the file does not exist",
                    f.path,
                    quote(&f.contents)
                )),
                Some(bytes) if bytes != f.contents.as_bytes() => diffs.push(format!(
                    "output {}: expected {}, got {}",
                    f.path,
                    quote(&f.contents),
                    quote(&String::from_utf8_lossy(&bytes))
                )),
                Some(_) => {}
            }
        }
        if let Some(want) = &run.stdout {
            if &cap.stdout != want || cap.stdout_omitted > 0 {
                diffs.push(format!(
                    "stdout: expected {}, got {}",
                    quote(want),
                    quote(&cap.stdout)
                ));
            }
        }
        let stderr_tail = tail_lines(&cap.stderr, 5);
        let passed = diffs.is_empty();
        results.push(ExampleResult {
            example: ex.name.clone(),
            run: i + 1,
            passed,
            ended,
            diffs,
            stderr_tail,
        });
        if !passed {
            break; // later runs depend on this one's files
        }
    }
    Ok(results)
}

// -- prompts ------------------------------------------------------------------

fn axis(list: &[String]) -> String {
    if list.is_empty() {
        "nothing".into()
    } else {
        list.join(", ")
    }
}

/// The task statement shared by every prompt: goal, inputs, outputs, the
/// ceiling in words with its exact prefixes, the budget and the examples.
pub fn task_statement(intent: &Intent) -> String {
    let c = &intent.cap_ceiling;
    let bullets = |xs: &[String]| {
        if xs.is_empty() {
            "- (none stated)\n".to_string()
        } else {
            xs.iter().map(|x| format!("- {x}\n")).collect()
        }
    };
    let exec = match c.exec {
        ExecPolicy::None => "no",
        _ => "yes",
    };
    let mut s = format!(
        "# Task\n{}\n\n## Inputs\n{}\n## Outputs\n{}\n## Authority\nImposed from outside by the \
         sandbox; the program declares none of it. Write paths with exactly these prefixes.\n\
         - may read files under: {}\n- may write files under: {}\n- may reach hosts: {}\n\
         - may start processes: {exec}\n- highest data label: {}\n\n## Budget\n- calls: {}\n\
         - tokens: {}\n- cost_micro: {}\n",
        intent.goal,
        bullets(&intent.inputs),
        bullets(&intent.outputs),
        axis(&c.fs_read),
        axis(&c.fs_write),
        axis(&c.net),
        c.max_label.as_str(),
        intent.budget.calls,
        intent.budget.tokens,
        intent.budget.cost_micro,
    );
    if !intent.examples.is_empty() {
        s.push_str(
            "\nThe program runs in its own directory; each example stages the inputs \
             there, runs it, then compares the files, exit status and stdout.\n\n",
        );
        s.push_str(&intent::render_examples(&intent.examples));
    }
    s
}

const ANSWER_RULES: &str = "## Answer\nReply with the complete Axon program and nothing else \
(bare source or one ```axon block). `fn main() -> i64` returns the exit status; never call \
`exit()`.\n";

/// Round 0: card + task + answer rules.
pub fn round0_prompt(intent: &Intent, card: &str) -> String {
    format!("{card}\n\n{}\n{ANSWER_RULES}", task_statement(intent))
}

/// A repair round: round 0's text, the previous candidate and its feedback.
pub fn repair_prompt(intent: &Intent, card: &str, previous: &str, feedback: &str) -> String {
    format!(
        "{}\n## Your previous program\n```axon\n{previous}\n```\n\n## What went wrong\n{feedback}\n\n\
         Fix the program. Reply with the complete corrected program only.\n",
        round0_prompt(intent, card)
    )
}

// -- small helpers --------------------------------------------------------------

fn ms(d: Duration) -> u64 {
    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Lines joined until `cap` bytes; the rest summarised as a count.
fn cap_lines(lines: &[String], cap: usize) -> String {
    let mut out = String::new();
    for (i, l) in lines.iter().enumerate() {
        if out.len() + l.len() + 1 > cap {
            out.push_str(&format!("({} more line(s) omitted)\n", lines.len() - i));
            break;
        }
        out.push_str(l);
        out.push('\n');
    }
    out
}

fn quote(s: &str) -> String {
    const MAX: usize = 400;
    if s.len() <= MAX {
        return serde_json::to_string(s).expect("a string serialises");
    }
    let mut end = MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}... ({} bytes)",
        serde_json::to_string(&s[..end]).expect("a string serialises"),
        s.len()
    )
}

fn tail_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

/// A private (0700) scratch directory, removed on drop.
struct Scratch(PathBuf);

impl Scratch {
    fn new(kind: &str) -> std::io::Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        use std::sync::atomic::{AtomicU64, Ordering};
        static N: AtomicU64 = AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let p = std::env::temp_dir().join(format!(
            "axon-intent-{kind}-{}-{}-{nanos:x}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new().mode(0o700).create(&p)?;
        Ok(Scratch(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
