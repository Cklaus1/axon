//! The creative step, kept outside the control loop.
//!
//! Cortex can observe, select, authorize, execute and verify — everything
//! except invent the content of a repair. That gap was reported honestly as
//! [`crate::runner::EpisodeOutcome::NeedsInput`], and this is the seam that
//! fills it.
//!
//! The shape is deliberate: a generator contributes ONE thing, a function body,
//! and contributes it as DATA. It does not choose actions, does not touch the
//! filesystem, and is never handed a tool. Everything it proposes re-enters the
//! same typed pipeline as any other action — authorized against the same grant,
//! executed through the same witness, adjudicated by the same hidden check.
//!
//! That division is the whole point. A model driving the loop can do anything
//! the loop can do; a model filling a hole in the loop can only propose text
//! that the loop then has to accept on its own terms.

use crate::action::SymbolRef;
use crate::{Observation, Observed};

/// What a proposal must respect. Passed to the generator so the constraints are
/// stated rather than assumed, and checked by the caller afterwards regardless
/// — a generator that ignores them is a possibility the loop has to survive.
#[derive(Debug, Clone)]
pub struct PatchConstraints {
    /// The symbol whose body is being replaced. A proposal for anything else is
    /// out of scope by construction.
    pub symbol: SymbolRef,
    /// Upper bound on the proposed body. Not a security control — the grant is
    /// that — but a bound on the obviously-wrong.
    pub max_bytes: usize,
    /// The body as it stands, the exact bytes a patch would replace.
    ///
    /// Supplied BY Cortex rather than read by the generator. A generator that
    /// opened the file itself would be a second filesystem agent with its own
    /// view of the workspace, and a disagreement between the two views would
    /// have no arbiter. Here there is nothing to disagree about: it is handed
    /// what Cortex would overwrite, and that is the whole of its read access.
    pub current_body: String,
    /// What has already been tried in this episode and rejected, oldest first.
    ///
    /// Without this a generator is asked the same question repeatedly with no
    /// way to know its last answer was wrong, so a deterministic one repeats
    /// itself and a model retries the same idea in different words. Three
    /// attempts at one idea is a worse use of a budget than one attempt each at
    /// three.
    pub rejected: Vec<RejectedAttempt>,
    /// What else the generator may read: the file it is repairing, minus the
    /// hidden check — or, when that cannot be shown, why not.
    pub context: GenContext,
}

/// The material a generator gets beyond the body.
///
/// The body alone was not enough to write a fix: a model shown
/// `Reward { name: name, score: max, max: max }` without the signature could
/// not know `score` was a parameter, and one shown `s.strategy == 1` without
/// the struct could not know what `1` meant. The material is the RANKER's
/// ([`crate::rank::build_view`]), built by the same function with the same
/// size cap and the same leak guard, so there is one definition of "what a
/// model may see of this file", not one per model call.
#[derive(Debug, Clone)]
pub enum GenContext {
    /// The file with the hidden check cut out (or, over the size cap, the
    /// target alone), the source of the VISIBLE failing checks, and the target's
    /// signature; plus what those visible checks report on the file as it
    /// stands (`visible_now`, `None` when they could not be run).
    View {
        view: crate::rank::RankView,
        visible_now: Option<String>,
    },
    /// The view could not be built — the hidden check's name appears outside
    /// its own definition, or no visible failing check is defined in the
    /// file. The prompt is the body-only one, and the caller reports `reason`.
    BodyOnly { reason: String },
}

impl GenContext {
    /// Why the body-only prompt is in use; `None` when the view is.
    pub fn fallback_reason(&self) -> Option<&str> {
        match self {
            GenContext::View { .. } => None,
            GenContext::BodyOnly { reason } => Some(reason),
        }
    }
}

/// Why a previous proposal did not stick.
///
/// The CLASS of rejection; anything more is in [`RejectedAttempt::feedback`],
/// and the line between the two is the hidden check. Nothing about the HIDDEN
/// check reaches a generator — not its name, not its source, not its verdict
/// beyond "rejected", not a byte of its output: a generator that could read why
/// it failed the adjudicator would be writing against the grader, and a result
/// graded by something the author can read stops being evidence. The VISIBLE
/// checks are a different matter. The generator is already shown their source
/// (they are what the operator wants fixed, and a plain coding agent runs them
/// freely), so what they print and assert against an attempt tells it nothing
/// the grader knows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RejectionReason {
    /// The patched file stopped compiling. The feedback carries the compiler's
    /// first diagnostics, from compiling the file with the hidden check cut out.
    DidNotCompile,
    /// It compiled, and the check that adjudicates completion refused it. The
    /// hidden check's name and output are withheld; the feedback carries what
    /// the VISIBLE checks report on the attempt, from a run of the file with
    /// the hidden check cut out.
    CheckRefused,
    /// Every check passed, but `axon check` on the patched file did not exit
    /// 0: it drew a diagnostic of some severity (a note such as `let mut`
    /// counts). A file the checker objects to is not a finished repair. The
    /// feedback carries those diagnostics, from checking the file with the
    /// hidden check cut out.
    CheckerObjected,
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RejectionReason::DidNotCompile => write!(f, "it did not compile"),
            RejectionReason::CheckRefused => write!(f, "it compiled but did not fix the problem"),
            RejectionReason::CheckerObjected => write!(
                f,
                "it passed the checks, but `axon check` reported diagnostics on the file"
            ),
        }
    }
}

/// A body that was already tried and rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedAttempt {
    pub body: String,
    pub reason: RejectionReason,
    /// What the visible side of the workspace said about it — compiler
    /// diagnostics or visible-check results — bounded to
    /// [`MAX_FEEDBACK_BYTES`]. Gathered from a file that does not contain
    /// the hidden check, so none of it can come from the hidden check. `None`
    /// when nothing could be gathered (and then nothing is invented).
    pub feedback: Option<String>,
}

/// Upper bound on the feedback carried per rejected attempt.
pub const MAX_FEEDBACK_BYTES: usize = 2048;

/// A generated body, plus WHO generated it.
///
/// `generator_id` is required, not optional, and is recorded in the episode
/// from the first version. The moment two generators exist, the only
/// interesting question is which one produced a given outcome, and an episode
/// that did not record it cannot answer that retrospectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedPatch {
    pub body: String,
    pub generator_id: String,
}

/// Why no proposal was produced.
///
/// Three reasons, not one, because they imply different remedies — the same
/// discipline `EpisodeOutcome` applies to how an episode ends. "It did not
/// work" would collapse a missing API key, a model declining, and a malformed
/// answer into one unactionable fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationFailure {
    /// No generator could be reached — offline, unconfigured, out of quota.
    /// An infrastructure fact, not a statement about the task.
    Unavailable(String),
    /// A generator was reached and declined to propose.
    Declined(String),
    /// Something was returned and it is not usable — empty, oversized, or
    /// otherwise failing the stated constraints.
    Invalid(String),
}

impl std::fmt::Display for GenerationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationFailure::Unavailable(s) => write!(f, "generator unavailable: {s}"),
            GenerationFailure::Declined(s) => write!(f, "generator declined: {s}"),
            GenerationFailure::Invalid(s) => write!(f, "proposal invalid: {s}"),
        }
    }
}

/// Supplies the one thing the control loop cannot.
pub trait PatchGenerator {
    /// Stable identity of this generator — model name and version, or the name
    /// of a deterministic strategy. Recorded in the episode, so two runs of the
    /// same loop with different generators are distinguishable afterwards.
    fn id(&self) -> String;

    /// Propose a body for `target`, given what was observed.
    ///
    /// Receives the observation rather than the file, so a generator sees what
    /// Cortex saw — including the omissions and the Unknown facts. Handing it
    /// the raw workspace would make it a second observer with a different view,
    /// and then a disagreement between them would have no arbiter.
    fn propose(
        &self,
        observation: &Observation,
        target: &SymbolRef,
        constraints: &PatchConstraints,
    ) -> Result<ProposedPatch, GenerationFailure>;
}

/// Check a proposal against its constraints.
///
/// Run by the CALLER, after the generator returns, on every proposal. A
/// generator that respects the constraints passes this trivially; one that does
/// not is caught here rather than at the filesystem. The bound is re-checked
/// rather than trusted because "the constraints were passed in" is not evidence
/// they were honoured.
pub fn validate(
    patch: &ProposedPatch,
    constraints: &PatchConstraints,
) -> Result<(), GenerationFailure> {
    if patch.generator_id.trim().is_empty() {
        return Err(GenerationFailure::Invalid(
            "proposal carries no generator_id; an unattributable patch cannot be recorded"
                .to_string(),
        ));
    }
    if patch.body.trim().is_empty() {
        // An empty body is not a minimal repair. It would delete the function's
        // behaviour while looking like a successful proposal.
        return Err(GenerationFailure::Invalid(format!(
            "empty body proposed for `{}`",
            constraints.symbol.symbol
        )));
    }
    if patch.body.len() > constraints.max_bytes {
        return Err(GenerationFailure::Invalid(format!(
            "proposed body is {} bytes, limit is {}",
            patch.body.len(),
            constraints.max_bytes
        )));
    }
    Ok(())
}

/// What a generator is asked.
///
/// Built from the body, the observed facts, and — when [`GenContext::View`] —
/// the ranker's view of the file: the file with the hidden check cut out, the
/// visible failing checks' source, what those checks report now, and for each
/// rejected attempt what the visible checks or the compiler said about it.
/// NEVER from the hidden check, which decides whether the result is accepted
/// and would stop being evidence the moment the thing under test could read
/// it. Every part shown is either cut from a file the hidden check was removed
/// from, or produced by running such a file.
pub fn build_prompt(obs: &Observation, target: &SymbolRef, c: &PatchConstraints) -> String {
    let mut facts = String::new();
    for (k, v) in &obs.facts {
        let rendered = match v {
            Observed::Known { value } => value.clone(),
            // An unknown fact is passed on AS unknown. Dropping it would
            // present a partial observation as a complete one, which is
            // the collapse `Observed` exists to prevent.
            Observed::Unknown { reason } => format!("unknown ({reason})"),
        };
        facts.push_str(&format!("- {k}: {rendered}\n"));
    }
    // What has already failed, oldest first. A model told only the current
    // state proposes the same thing again; the whole reason this list is
    // threaded through the constraints is so a retry can be a different
    // IDEA rather than the same one reworded.
    let mut history = String::new();
    for (i, r) in c.rejected.iter().enumerate() {
        history.push_str(&format!(
            "\nAttempt {} was REJECTED because {}:\n```\n{}\n```\n",
            i + 1,
            r.reason,
            r.body
        ));
        if let Some(f) = &r.feedback {
            let label = match r.reason {
                RejectionReason::DidNotCompile => "The compiler reported",
                RejectionReason::CheckRefused => {
                    "With that attempt applied, the visible tests reported"
                }
                RejectionReason::CheckerObjected => "`axon check` reported",
            };
            history.push_str(&format!("{label}:\n```\n{}\n```\n", f.trim_end()));
        }
    }
    if !history.is_empty() {
        history = format!(
            "\nAlready tried in this episode — do NOT propose any of these \
             again, and prefer a materially different approach:\n{history}"
        );
    }
    let context = match &c.context {
        GenContext::BodyOnly { .. } => String::new(),
        GenContext::View { view, visible_now } => {
            let signature = view
                .candidates
                .iter()
                .find(|(n, _)| *n == target.symbol)
                .map(|(_, s)| s.as_str())
                .unwrap_or("");
            let what = if view.is_complete() {
                "The file, with one check that is not shown to you removed".to_string()
            } else {
                format!("From the file ({})", view.source_note)
            };
            let names: Vec<&str> = view.failing.iter().map(|(n, _)| n.as_str()).collect();
            let tests: Vec<&str> = view.failing.iter().map(|(_, b)| b.trim_end()).collect();
            let now = visible_now
                .as_deref()
                .map(|t| {
                    format!(
                        "What the visible tests report on the file as it stands:\n```\n{}\n```\n\n",
                        t.trim_end()
                    )
                })
                .unwrap_or_default();
            format!(
                "Signature: {signature}\n\n\
                 {what}:\n```axon\n{source}\n```\n\n\
                 Failing visible test(s): {names}\n```axon\n{tests}\n```\n\n\
                 {now}",
                source = view.source.trim_end(),
                names = names.join(", "),
                tests = tests.join("\n\n"),
            )
        }
    };
    format!(
        "You are repairing one function in an Axon program.\n\n\
         File: {path}\n\
         Function: {symbol}\n\
         {context_sep}{context}\
         Its body is currently:\n\
         ```\n{body}\n```\n\n\
         What a checker observed about the file:\n{facts}\
         {history}\n\
         The function is believed to be semantically wrong. Return the \
         REPLACEMENT BODY only — the text between the function's braces, \
         with no braces, no signature, and no explanation. Keep it under \
         {max} bytes. Preserve the existing indentation style.",
        path = target.path,
        symbol = target.symbol,
        context_sep = if context.is_empty() { "\n" } else { "" },
        body = c.current_body,
        history = history,
        max = c.max_bytes,
    )
}

/// Proposes one fixed body, supplied by the operator.
///
/// Not a test double — it is reachable from the CLI as `--generator
/// literal:BODY`, and it is the honest way to apply a known fix: the patch goes
/// through the same grant check, the same witness and the same hidden check as
/// a model's would, instead of being hand-edited into the file where none of
/// those run. "I already know the answer" is a reason to skip the model, not a
/// reason to skip the adjudication.
///
/// It also makes the loop's success path reachable without a network, which is
/// why the CLI's exit-0 contract can be tested at all.
pub struct LiteralGenerator {
    body: String,
}

impl LiteralGenerator {
    pub fn new(body: impl Into<String>) -> Self {
        Self { body: body.into() }
    }
}

impl PatchGenerator for LiteralGenerator {
    fn id(&self) -> String {
        // Names the mechanism, not the content. The body is already recorded
        // in the episode as a before/after digest pair, and repeating it here
        // would put the patch text in the attribution field.
        "literal@1".to_string()
    }

    fn propose(
        &self,
        _observation: &Observation,
        _target: &SymbolRef,
        _constraints: &PatchConstraints,
    ) -> Result<ProposedPatch, GenerationFailure> {
        Ok(ProposedPatch {
            body: self.body.clone(),
            generator_id: self.id(),
        })
    }
}

/// How much output is read from a generator before giving up on it.
///
/// Generous enough that no honest generator meets it, and bounded so a runaway
/// one cannot exhaust memory and take the episode record with it.
const MAX_GENERATOR_READ: usize = 1 << 20;

/// Asks a program the operator names.
///
/// The prompt goes to its stdin; the proposed body is its stdout. That is the
/// whole protocol, and it exists so Cortex can be driven by whatever model the
/// operator already has — a vendor CLI, a local server, a shell script — with
/// no credentials inside this process and no provider baked into this crate.
///
/// ## Why this is not the thing this crate refuses to do
///
/// Cortex's central discipline is that a MODEL never chooses an action and
/// never holds a tool. That is intact here. The command is named by the
/// OPERATOR on the command line, under their own authority, before any model
/// is involved; a model cannot select it, cannot change it, and cannot reach
/// past it. What crosses the boundary in that direction is a prompt, and what
/// comes back is text that re-enters the same typed pipeline as any other
/// proposal — validated, authorized against the same grant, and adjudicated by
/// the same hidden check.
///
/// The operator is trusting their own program, which they already run. They
/// are not granting a model anything.
pub struct CommandGenerator {
    program: String,
}

impl CommandGenerator {
    /// How long a generator gets to answer.
    ///
    /// `--budget` bounds STEPS, not wall time, and no budget applies to a
    /// child process at all — so without a deadline one program that never
    /// exits stops the loop forever.
    const TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

    /// The deadline in force, honouring `AXON_CORTEX_GENERATOR_TIMEOUT_MS`.
    ///
    /// The override exists so the deadline can be TESTED. A 120-second wait is
    /// not something a suite can afford, and an untested deadline is exactly
    /// the class of check that turns out never to fire. A malformed value is
    /// ignored in favour of the default rather than parsed to zero, which
    /// would disable every generator.
    fn timeout() -> std::time::Duration {
        std::env::var("AXON_CORTEX_GENERATOR_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|ms| *ms > 0)
            .map(std::time::Duration::from_millis)
            .unwrap_or(Self::TIMEOUT)
    }

    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
        }
    }
}

impl PatchGenerator for CommandGenerator {
    fn id(&self) -> String {
        // The program as written, so the episode records WHICH generator ran.
        // Not the resolved absolute path: what the operator typed is what they
        // will recognise, and two scripts sharing a basename stay
        // distinguishable.
        format!("cmd:{}", self.program)
    }

    fn propose(
        &self,
        observation: &Observation,
        target: &SymbolRef,
        constraints: &PatchConstraints,
    ) -> Result<ProposedPatch, GenerationFailure> {
        use std::io::{Read, Write};
        let prompt = build_prompt(observation, target, constraints);
        let mut child = std::process::Command::new(&self.program)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            // Could not be STARTED: missing, not executable, wrong path. An
            // infrastructure fact about the operator's setup, not a statement
            // about the task — the same class as a missing API key.
            .map_err(|e| {
                GenerationFailure::Unavailable(format!("cannot run `{}`: {e}", self.program))
            })?;

        // ALL THREE PIPES ARE SERVICED CONCURRENTLY.
        //
        // Writing the prompt in full and only then reading deadlocks against
        // any child that produces output before consuming its input: both
        // sides block on a full pipe buffer (64 KiB on Linux). Polling for
        // exit without draining stdout has the same failure — measured, a
        // program emitting 200 KB before reading stdin sat until the deadline
        // instead of answering.
        let bytes = prompt.into_bytes();
        let mut stdin = child.stdin.take();
        let writer = std::thread::spawn(move || {
            if let Some(mut w) = stdin.take() {
                let _ = w.write_all(&bytes);
                // Dropped here, closing the pipe: a generator reading to EOF
                // would otherwise wait forever for input already written.
            }
        });
        let drain = |pipe: Option<std::process::ChildStdout>| {
            std::thread::spawn(move || {
                let mut buf = Vec::new();
                if let Some(p) = pipe {
                    // CAPPED. `validate` applies `max_bytes` after the read
                    // completes, so it bounds what is APPLIED and never what
                    // is buffered; a child writing gigabytes would abort the
                    // process on allocation failure and take the episode
                    // record with it.
                    let _ = p.take(MAX_GENERATOR_READ as u64).read_to_end(&mut buf);
                }
                buf
            })
        };
        let out_t = drain(child.stdout.take());
        let mut err_pipe = child.stderr.take();
        let err_t = std::thread::spawn(move || {
            let mut buf = Vec::new();
            if let Some(p) = err_pipe.take() {
                let _ = p.take(MAX_GENERATOR_READ as u64).read_to_end(&mut buf);
            }
            buf
        });

        // A DEADLINE. `--budget` bounds STEPS, not wall time, and no budget
        // applies to a child process — so without this, one program that never
        // exits stops the loop forever.
        let limit = Self::timeout();
        let deadline = std::time::Instant::now() + limit;
        let status = loop {
            match child.try_wait() {
                Ok(Some(st)) => break st,
                Err(e) => {
                    return Err(GenerationFailure::Unavailable(format!(
                        "`{}` could not be waited on: {e}",
                        self.program
                    )))
                }
                Ok(None) => {
                    if std::time::Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(GenerationFailure::Unavailable(format!(
                            "`{}` did not answer within {}ms",
                            self.program,
                            limit.as_millis()
                        )));
                    }
                    std::thread::sleep(std::time::Duration::from_millis(20));
                }
            }
        };
        let stdout = out_t.join().unwrap_or_default();
        let stderr = err_t.join().unwrap_or_default();
        let _ = writer.join();

        if !status.success() {
            // It RAN and refused. Declined, not Unavailable: the remedy is the
            // prompt or the task, not the installation.
            let why = String::from_utf8_lossy(&stderr);
            let why = why.trim();
            return Err(GenerationFailure::Declined(format!(
                "`{}` exited {}{}",
                self.program,
                status.code().unwrap_or(-1),
                if why.is_empty() {
                    String::new()
                } else {
                    format!(": {}", why.lines().next().unwrap_or(""))
                }
            )));
        }
        Ok(ProposedPatch {
            body: String::from_utf8_lossy(&stdout).to_string(),
            generator_id: self.id(),
        })
    }
}
