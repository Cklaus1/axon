//! The repair-episode runner — the package's "first executable vertical slice".
//!
//! Sequence, from `build/BUILD_PLAN.md`: snapshot → partial observation → legal
//! action catalog → select action → generate a patch → revalidate
//! grant/payload/snapshot → apply in a COPY → run checks → independently verify
//! → save evidence → replay.
//!
//! What makes this a slice rather than scaffolding: every step runs against the
//! real `axon` binary and a real temporary workspace. The checks are actual
//! process exits, not simulated verdicts.
//!
//! The authority rules implemented here come from CX-03's negative gates, which
//! state what must be REFUSED — the useful half of a capability spec:
//!
//! * `G03-forgery` — unknown / wrong-principal / stale-snapshot grants refuse
//!   **without a side effect**. The "without a side effect" is the load-bearing
//!   part: refusing after writing is not refusing.
//! * `G03-payload` — a valid edit grant paired with a traversal or policy-file
//!   path still refuses. Authority to edit is not authority to edit anything.
//! * `G03-done` — a DONE claim with failing hidden checks cannot close the
//!   task, "regardless of confidence". Confidence is not evidence.
//! * `G02-partial` — a broken fixture still yields a useful partial
//!   observation, and the observer never fabricates.

use crate::action::CortexAction;
use crate::episode::{Episode, EpisodeEvent};
use crate::{content_digest, Observation, Observed, WorkspaceSnapshot};
use std::path::{Path, PathBuf};

/// Why an action was refused. Every variant is a refusal with a reason a
/// reviewer can act on; there is no generic `Denied`.
#[derive(Debug, Clone, PartialEq)]
pub enum Refusal {
    UnknownGrant(String),
    WrongPrincipal { expected: String, got: String },
    StaleSnapshot { expected: String, got: String },
    PathOutsideGrant(String),
    PathTraversal(String),
    PolicyFile(String),
    NotInCatalog(String),
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::UnknownGrant(g) => write!(f, "unknown grant `{g}`"),
            Refusal::WrongPrincipal { expected, got } => {
                write!(f, "grant belongs to `{expected}`, not `{got}`")
            }
            Refusal::StaleSnapshot { expected, got } => {
                write!(f, "grant pinned snapshot {expected}, workspace is {got}")
            }
            Refusal::PathOutsideGrant(p) => write!(f, "`{p}` is outside the granted region"),
            Refusal::PathTraversal(p) => write!(f, "`{p}` contains a `..` component"),
            Refusal::PolicyFile(p) => write!(f, "`{p}` is a policy file and is never editable"),
            Refusal::NotInCatalog(a) => write!(f, "action `{a}` is not in the legal catalog"),
        }
    }
}

/// An edit grant: authority to change ONE region, pinned to ONE snapshot.
#[derive(Debug, Clone)]
pub struct EditGrant {
    pub grant_id: String,
    pub principal: String,
    /// The snapshot this grant was issued against. A workspace that has moved
    /// on invalidates it — authority does not survive the state it was granted
    /// over.
    pub snapshot_id: String,
    /// Path prefixes this grant may write, relative to the workspace root.
    /// Empty denies everything, matching the house convention that landed
    /// 2026-09-19 (`""` denies, `"*"` is unrestricted).
    pub write_prefixes: Vec<String>,
}

/// Paths no grant may ever edit, however broad. The policy cannot authorise
/// edits to the policy (`G00-authority`).
/// Exact file names that are never writable, whatever a grant says.
const POLICY_FILES: &[&str] = &["axon.lock", ".axon-policy", "gate.sh", "profile.rs"];

/// Path PREFIXES that are never writable, whatever a grant says.
///
/// `POLICY_FILES` alone was four basenames matched with `ends_with`, and
/// measured through the policy adapter with the broadest possible grant
/// (`--write-prefix '*'`) it protected exactly one real file:
///
/// ```text
/// scripts/gate.sh                                 refuse   <-- the basename
/// scripts/gate_verdict_is_read.sh                 allow    <-- a gate script
/// AXON-COMPLETENESS.json                          allow    <-- evaluation data
/// governance/cortex_gate_execution_registry.json  allow    <-- the gate registry
/// crates/axon-cortex/src/locate.rs                allow    <-- the localizer
/// ```
///
/// CX-00's `G00-authority` simulates "a learner attempts to edit gate code,
/// evaluation data, policy or signer credentials". That was satisfied for
/// `gate.sh` and nominally for three other names.
///
/// A PREFIX set, rather than more basenames: the grant machinery in this same
/// function already reasons in path prefixes, so this reuses the idiom instead
/// of adding a second matching rule. Cortex repairs `.ax` symbols; these trees
/// are policy and evidence surfaces, not repair targets.
const POLICY_PREFIXES: &[&str] = &["scripts/", "governance/", ".github/"];

/// Exact paths that are never writable — evidence artifacts that live at the
/// repository root and so have no protecting prefix.
const POLICY_PATHS: &[&str] = &["AXON-COMPLETENESS.json", "AXON-COMPLETENESS.md"];

/// Proof that a specific action passed authorization.
///
/// The field is private and there is no public constructor, so the only way to
/// hold one is to have been given it by [`Runner::authorize_action`]. That is
/// what makes `execute` unable to run an unauthorized action: not a check
/// inside execute that could be forgotten or reordered, but a value the caller
/// cannot produce without passing the gate.
///
/// It borrows the action rather than copying it, so the thing executed is
/// necessarily the thing authorized — a copy could drift between the two calls.
#[derive(Debug)]
pub struct Authorized<'a> {
    action: &'a CortexAction,
}

impl<'a> Authorized<'a> {
    pub fn action(&self) -> &'a CortexAction {
        self.action
    }
}

/// What executing an action actually did.
///
/// Every variant is a distinct observed outcome. There is no `Ok`/`Err` pair,
/// because "the symbol was not found" and "the check failed" are different
/// facts that a caller and an auditor both need to tell apart, and collapsing
/// them into one error string is how a missing target comes to read as a
/// failing test.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecOutcome {
    /// Read a symbol's body. No side effect.
    Inspected { symbol: String, body: String },
    /// The named symbol is not in the file. NOT an error and NOT a failure:
    /// the action could not be carried out, which is a third thing.
    SymbolNotFound { symbol: String, path: String },
    /// A named check ran. `matched` is how many tests the name selected — 0
    /// means the check does not exist, which is never a pass.
    CheckRan {
        name: String,
        passed: bool,
        matched: usize,
    },
    /// A symbol body was replaced.
    Patched { path: String, symbol: String },
    /// A completion claim was recorded. Deliberately effect-free: the claim is
    /// evidence for `verify()` to adjudicate, not an action that closes
    /// anything by itself.
    Claimed { rationale: String },
    /// The action could not be attempted at all, with the reason.
    Failed(String),
    /// A named check was stopped at a process bound (time or output) and so
    /// produced no verdict. On a candidate's patched file that rejects the
    /// candidate (R2); anywhere else it blocks like [`ExecOutcome::Failed`].
    CheckUnfinished { name: String, why: String },
}

/// Find a function body in Axon source: the text between the brace that opens
/// `fn <symbol>` and its matching close.
///
/// Returns None when the symbol is absent — the caller reports that as
/// `SymbolNotFound` rather than as an empty body, because an empty body is a
/// real thing a function can have.
fn symbol_body(src: &str, symbol: &str) -> Option<(usize, usize)> {
    // The open paren is load-bearing. Without it `fn fib` matches `fn fib_rec`
    // defined earlier in the file, and every caller of this function then
    // operates on the WRONG body: Inspect reads it, the generator is shown it,
    // and the patch overwrites it. Measured on the corpus: 5 collisions across
    // 4 files (`fib`/`fib_rec`, `belief_map`/`belief_map_index`,
    // `approx_eq`/`approx_eq_b`, …), so the selected function was unrepairable
    // by construction and a working one took the damage.
    //
    // It also re-opened the grader-not-patchable guard, which compares the
    // symbol by exact string while the write resolved by prefix.
    //
    // `locate.rs` already spelled its needle with the paren; the two modules
    // disagreed about what "the body of X" means.
    let needle = format!("fn {symbol}(");
    let at = src.find(&needle)?;
    let open = src[at..].find('{')? + at;
    let mut depth = 0usize;
    for (i, c) in src[open..].char_indices() {
        match c {
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some((open + 1, open + i));
                }
            }
            _ => {}
        }
    }
    None
}

/// How a bounded repair episode ended.
///
/// Every variant is a REASON, not a status code. "It stopped" is not a result a
/// caller can act on, and an episode that ends without saying why is the same
/// absent-vs-empty collapse the observer refuses one layer down.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EpisodeOutcome {
    /// A completion claim was made AND independently verified.
    VerifiedDone { steps: usize },
    /// The loop ran out of steps. Distinct from being stuck: more budget might
    /// have finished it, and conflating the two would hide a tuning problem as
    /// a capability problem.
    BudgetExhausted { steps: usize },
    /// Two consecutive steps left the workspace unchanged AND chose the same
    /// action. Continuing cannot help, so the loop stops and says so instead of
    /// spinning to the budget.
    NoProgress { steps: usize, action: String },
    /// The adjudicating check passed and nothing regressed, but the file was
    /// already failing other checks and still is.
    ///
    /// Its own outcome because the patch is KEPT: it did what it was asked,
    /// and the remaining failures pre-date the episode. Reporting it as
    /// success would claim a clean file that is not clean; reporting it as
    /// futility blamed a generator that had succeeded, and discarded its work.
    AdjudicatedNotClean {
        steps: usize,
        still_failing: Vec<String>,
    },
    /// The target does not exist in the file.
    ///
    /// Its own variant because the remedies differ, and the two codes it used
    /// to borrow were both actively misleading: a mistyped `--symbol` reported
    /// as `Blocked` sends an operator to debug a checker that ran fine, and as
    /// `NoProgress` it blames a generator that was never asked.
    NoSuchSymbol {
        steps: usize,
        symbol: String,
        path: String,
    },
    /// Selection could not choose, carrying the observer's reason.
    Blocked { steps: usize, reason: String },
    /// Authority refused the chosen action.
    Refused { steps: usize, reason: String },
    /// The loop needs something it cannot produce — today, a patch body. Said
    /// plainly rather than dressed as a failure: the system is not broken, it
    /// is out of scope without a generator.
    NeedsInput { steps: usize, what: String },
    /// The episode stopped asking its generator because the visible verdict
    /// was not moving (`crate::earlystop`), or the run stopped before any
    /// episode because no single function can account for the visible
    /// failures. A suspicion that the defect is beyond a one-body edit, not a
    /// proof: more budget MIGHT still have finished it, which is why it is
    /// not `BudgetExhausted`, and the generator was not at fault, which is why
    /// it is not `NoProgress`.
    OutOfScopeSuspected { steps: usize, reason: String },
    /// R3: the walk's hard budget (`--max-proposals` or `--deadline-ms`) ran
    /// out, so no generator is asked anything again. Distinct from
    /// `BudgetExhausted` (one episode's steps), which may also be a pause.
    WalkBudgetExhausted { steps: usize, by: WalkLimit },
}

/// What the visible checks said about ONE rejected proposal, kept for the early
/// stop (`crate::earlystop`). It is read from the same visible run whose
/// output the generator is shown, never from the hidden check.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AttemptSignal {
    /// Which proposal of this candidate it was, counting from 1.
    pub proposal: usize,
    pub rejection: crate::generate::RejectionReason,
    /// The visible checks that failed with the attempt applied (the hidden
    /// check is cut out of the file that ran). `None` when no test run was
    /// made or it could not be gathered: a compile failure, or a visible run
    /// that was refused.
    pub visible_failing: Option<Vec<String>>,
}

/// What one candidate's episode has learned so far, kept so a walk can stop
/// the episode after a number of proposals and resume it later exactly where
/// it stopped.
///
/// `cortex repair` walks its candidates breadth-first: every candidate gets ONE
/// proposal before any candidate gets a second. Restarting an episode for the
/// second pass would throw away what it learned — the rejected bodies, the
/// refused claim, the visited states — so the generator would be asked its
/// first question again with an empty history, and the step budget would be
/// spent twice. Carrying this instead makes a paused-then-resumed episode take
/// the same steps, see the same history and stop for the same reasons as one
/// that ran straight through. Only the ORDER across candidates changes.
///
/// Resuming assumes the target file holds the bytes the episode was paused
/// on. The walk guarantees it: every attempt starts from the file as the run
/// found it, and an episode undoes each rejected patch before it pauses.
#[derive(Debug, Default)]
pub struct CandidateProgress {
    /// Pause before asking the generator for proposal number `limit + 1`.
    /// `None` runs to the step budget, as an unpaused episode does.
    pub proposal_limit: Option<usize>,
    /// Stop, instead of asking for the next proposal, once this many compiled
    /// proposals in a row left the visible verdict unchanged
    /// ([`crate::earlystop`]). `None` and `Some(0)` never stop.
    pub inert_limit: Option<usize>,
    seen: Vec<(String, String, usize)>,
    ctx: crate::select::SelectionContext,
    rejected: Vec<crate::generate::RejectedAttempt>,
    last_applied: Option<String>,
    baseline_passing: Option<Vec<String>>,
    /// The checks failing when this candidate's episode first started (the
    /// hidden check included), read from the same run as `baseline_passing`.
    baseline_failing: Option<Vec<String>>,
    /// What this candidate's generator is shown beyond the body; built at its
    /// first proposal.
    gen_context: Option<crate::generate::GenContext>,
    steps: usize,
    proposals: usize,
    paused: bool,
    attempts: Vec<AttemptSignal>,
}

impl CandidateProgress {
    /// Steps of the budget this candidate has used, across every resumption.
    pub fn steps(&self) -> usize {
        self.steps
    }
    /// The checks that were failing when this candidate's episode first
    /// started, hidden check included; `None` before it started or when that
    /// baseline could not be taken.
    pub fn baseline_failing(&self) -> Option<&[String]> {
        self.baseline_failing.as_deref()
    }
    /// What this candidate's generator was shown beyond the body; `None` when
    /// no proposal was asked for.
    pub fn generator_context(&self) -> Option<&crate::generate::GenContext> {
        self.gen_context.as_ref()
    }
    /// Times the generator has been asked for this candidate.
    pub fn proposals(&self) -> usize {
        self.proposals
    }
    /// The last run stopped at `proposal_limit`, not for a reason of its own:
    /// its `BudgetExhausted` means "paused", and resuming may still finish it.
    pub fn paused(&self) -> bool {
        self.paused
    }

    /// What the visible checks said about each of this candidate's rejected
    /// proposals, in order, across every resumption.
    pub fn attempts(&self) -> &[AttemptSignal] {
        &self.attempts
    }
}

pub struct Runner {
    /// The interpreter `observe` runs `axon check` with. NOT the check
    /// executor: registered checks go through `check_executor`, which spawns
    /// only a registry-pinned binary.
    pub axon_bin: PathBuf,
    pub workspace: PathBuf,
    pub episode: Episode,
    /// The previous snapshot's id, so each new snapshot records its parent and
    /// an episode's state chain is auditable rather than a set of orphans.
    last_snapshot_id: Option<String>,
    /// B264: the seam every check dispatch goes through.
    check_executor: Box<dyn CheckExecutor>,
    /// Limits on the `axon check` processes this runner spawns itself.
    checker_limits: ExecLimits,
    /// R3: the walk's hard budget, shared by every episode this runner
    /// drives (both passes of a `cortex repair` walk).
    walk: WalkBudget,
    /// R1: the static grant of the target being repaired. A candidate for
    /// that symbol reaching anything outside it is refused before it runs.
    /// `None`: no admission check (no grant was derived).
    static_grant: Option<crate::fence::StaticGrant>,
}

/// R3: which hard limit of a walk ran out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkLimit {
    /// `--max-proposals`: generator calls across the whole walk.
    Proposals,
    /// `--deadline-ms`: wall clock from cortex start.
    Deadline,
}

impl WalkLimit {
    /// The slug `cortex repair` reports as `detail` and `exhausted_by`.
    pub fn as_str(self) -> &'static str {
        match self {
            WalkLimit::Proposals => "proposals",
            WalkLimit::Deadline => "deadline",
        }
    }
}

/// R3: the hard budget of a walk — total generator calls and a wall-clock
/// deadline — checked before every generator call of every episode one
/// [`Runner`] drives.
///
/// Separate from [`CandidateProgress::proposal_limit`], which PAUSES one
/// candidate after pass 1's single proposal so the next can be tried: that
/// limit is a walk ORDER, and exhausting it means "resume later". This one is
/// a ceiling, and exhausting it means no generator is asked anything again.
/// Unset limits are unlimited, so a walk without them runs as before.
#[derive(Debug, Clone, Default)]
pub struct WalkBudget {
    pub max_proposals: Option<usize>,
    pub deadline: Option<std::time::Instant>,
    used: usize,
    exhausted_by: Option<WalkLimit>,
}

impl WalkBudget {
    pub fn new(max_proposals: Option<usize>, deadline: Option<std::time::Instant>) -> Self {
        WalkBudget {
            max_proposals,
            deadline,
            ..WalkBudget::default()
        }
    }

    /// Generator calls started so far, refused or not.
    pub fn proposals_used(&self) -> usize {
        self.used
    }

    /// The limit that stopped the walk; `None` while none has.
    pub fn exhausted_by(&self) -> Option<WalkLimit> {
        self.exhausted_by
    }

    /// Time left before the deadline; `None` without one.
    pub fn remaining(&self) -> Option<std::time::Duration> {
        self.deadline
            .map(|d| d.saturating_duration_since(std::time::Instant::now()))
    }

    /// Whether the deadline has passed (`false` without one).
    pub fn deadline_passed(&self) -> bool {
        self.remaining().is_some_and(|r| r.is_zero())
    }

    /// Admit one more generator call, or say which limit forbids it (and
    /// remember it). `min_time` is how long the generator needs once started
    /// because it cannot be cut short ([`crate::generate::PatchGenerator::
    /// uncancellable_for`]): a call with less time left than that is not
    /// started.
    pub fn admit(&mut self, min_time: Option<std::time::Duration>) -> Result<(), WalkLimit> {
        match self.exhausted(min_time) {
            Some(by) => Err(by),
            None => {
                self.used += 1;
                Ok(())
            }
        }
    }

    /// The limit that forbids another generator call needing `min_time`, if
    /// one does — remembered, so the walk reports what stopped it. The walk
    /// driver asks it before starting an episode, so no baseline checks run
    /// for an episode that could not ask its generator anything.
    pub fn exhausted(&mut self, min_time: Option<std::time::Duration>) -> Option<WalkLimit> {
        if self.exhausted_by.is_none() {
            self.exhausted_by = if self.max_proposals.is_some_and(|m| self.used >= m) {
                Some(WalkLimit::Proposals)
            } else {
                self.remaining()
                    .filter(|r| r.is_zero() || min_time.is_some_and(|t| *r < t))
                    .map(|_| WalkLimit::Deadline)
            };
        }
        self.exhausted_by
    }

    /// Record that the deadline stopped the walk outside [`WalkBudget::admit`]
    /// (a generator call cut at it).
    fn expire(&mut self) {
        self.exhausted_by.get_or_insert(WalkLimit::Deadline);
    }
}

/// The capture bound on one stream of an `axon check` the runner spawns.
pub const CHECKER_MAX_OUTPUT: usize = 1 << 20;
/// R1: the wall-clock limit on an `axon caps` run when the runner has no
/// checker timeout of its own.
pub const CAPS_WALL_DEFAULT: std::time::Duration = std::time::Duration::from_secs(60);

impl Runner {
    /// Convenience constructor kept for existing callers.
    ///
    /// The check executor is a [`LocalInterpreterExecutor`] over `axon_bin`
    /// registered LAZILY: the binary is resolved and its sha256 pinned on the
    /// FIRST check dispatch, and every later dispatch is refused if it has
    /// changed. Lazily because callers that never run a check (the policy
    /// adapter authorizes and exits) should not pay for hashing an interpreter
    /// binary. A caller that needs the pin taken BEFORE anything else happens —
    /// the `cortex` CLI does — registers explicitly and uses
    /// [`Runner::with_check_executor`].
    pub fn new(axon_bin: impl Into<PathBuf>, workspace: impl Into<PathBuf>) -> Self {
        let axon_bin = axon_bin.into();
        let exec = LocalInterpreterExecutor::pin_on_first_use(axon_bin.clone());
        Self::with_check_executor(axon_bin, workspace, Box::new(exec))
    }

    /// Construct with an explicit, already-registered check executor.
    pub fn with_check_executor(
        axon_bin: impl Into<PathBuf>,
        workspace: impl Into<PathBuf>,
        check_executor: Box<dyn CheckExecutor>,
    ) -> Self {
        Runner {
            axon_bin: axon_bin.into(),
            workspace: workspace.into(),
            episode: Episode::new("cortex-repair-1"),
            last_snapshot_id: None,
            check_executor,
            checker_limits: ExecLimits {
                max_output: Some(CHECKER_MAX_OUTPUT),
                ..ExecLimits::default()
            },
            walk: WalkBudget::default(),
            static_grant: None,
        }
    }

    /// R3: bound every episode this runner drives from now on by `budget`.
    pub fn set_walk_budget(&mut self, budget: WalkBudget) {
        self.walk = budget;
    }

    /// R3: the walk budget and what has been spent of it.
    pub fn walk_budget(&self) -> &WalkBudget {
        &self.walk
    }

    /// R1: admit candidates for `grant.symbol` only within `grant` (`None`:
    /// no admission check). Set per target, before its episode.
    pub fn set_static_grant(&mut self, grant: Option<crate::fence::StaticGrant>) {
        self.static_grant = grant;
    }

    /// R1d: run every test of `rel_path` through this runner's executor (the
    /// fenced one) and return `(test, kind, message)` for each failure the
    /// fence caused ([`crate::fence::fence_violation`]). Run on the PRISTINE
    /// file, any entry means the fence would refuse what the original code
    /// already does — the run must stop rather than repair under it.
    pub fn fence_conflicts(
        &self,
        rel_path: &str,
    ) -> std::io::Result<Vec<(String, String, String)>> {
        let report = self.check_executor.run_checks(&CheckRequest {
            workspace: &self.workspace,
            rel_path,
            filter: None,
        })?;
        Ok(report
            .messages
            .iter()
            .filter_map(|(test, msg)| {
                crate::fence::fence_violation(msg).map(|kind| (test.clone(), kind, msg.clone()))
            })
            .collect())
    }

    /// R3: [`WalkBudget::exhausted`] on this runner's walk budget.
    pub fn walk_exhausted(&mut self, min_time: Option<std::time::Duration>) -> Option<WalkLimit> {
        self.walk.exhausted(min_time)
    }

    /// Kill every `axon check` this runner spawns (observation, the
    /// visible-copy compile, the acceptance check) at `wall` (R2). The
    /// registered checks are bounded by their executor's own limits.
    pub fn set_checker_timeout(&mut self, wall: std::time::Duration) {
        self.checker_limits.wall = Some(wall);
    }

    /// `axon check <file>` as a bounded job (R2): its own process group, the
    /// runner's wall-clock limit, at most [`CHECKER_MAX_OUTPUT`] bytes per
    /// stream. Returns (stdout+stderr, exit code). A run stopped at a bound
    /// is an `Err` carrying a [`BoundExceeded`].
    fn run_axon_check(
        &self,
        file: &Path,
        cwd: Option<&Path>,
    ) -> std::io::Result<(String, Option<i32>)> {
        let mut cmd = std::process::Command::new(&self.axon_bin);
        if let Some(d) = cwd {
            cmd.current_dir(d);
        }
        cmd.arg("check").arg(file);
        let (text, code, truncated) = run_limited(&mut cmd, &self.checker_limits, None)?;
        if truncated {
            return Err(BoundExceeded::output(CHECKER_MAX_OUTPUT));
        }
        Ok((text, code))
    }

    /// Which executor adjudicates this runner's checks (for evidence/logs).
    pub fn check_executor_id(&self) -> String {
        self.check_executor.id()
    }

    /// Snapshot the workspace region, content-addressed.
    pub fn snapshot(&mut self, scope: &[&str]) -> std::io::Result<WorkspaceSnapshot> {
        let mut files = Vec::new();
        for rel in scope {
            let p = self.workspace.join(rel);
            if p.is_file() {
                let bytes = std::fs::read(&p)?;
                files.push(((*rel).to_string(), content_digest(&bytes)));
            } else {
                // AN ABSENT FILE IS RECORDED AS ABSENT, not omitted.
                //
                // Skipping it silently made two different workspaces share an
                // id: a scope of ["a.ax"] with `a.ax` deleted produced exactly
                // the same empty file list — and therefore the same
                // snapshot_id — as a scope of ["b.ax"] with `b.ax` deleted, or
                // as an empty scope. A grant pinned to the first state was
                // accepted as current in the second, so `StaleSnapshot` could
                // not fire between two genuinely different states.
                //
                // It also meant deleting the file under repair moved nothing
                // in the recorded evidence. That is this crate's first rule —
                // an absence must not render as a present fact — broken in the
                // function that computes the evidence.
                files.push(((*rel).to_string(), "<absent>".to_string()));
            }
        }
        files.sort();
        // Derived from content, never a constant. A grant is pinned to the
        // state it was granted over, so if the id does not move when the
        // workspace moves, `StaleSnapshot` can never fire on a real pair of
        // snapshots and the check is decoration.
        let snapshot_id = content_digest(
            files
                .iter()
                .map(|(p, d)| format!("{p}\u{0}{d}\n"))
                .collect::<String>()
                .as_bytes(),
        );
        let snap = WorkspaceSnapshot {
            snapshot_id,
            parent_snapshot_id: self.last_snapshot_id.take(),
            files,
            observation_scope: scope.iter().map(|s| s.to_string()).collect(),
        };
        self.last_snapshot_id = Some(snap.snapshot_id.clone());
        self.episode.push(EpisodeEvent::Snapshot {
            snapshot_id: snap.snapshot_id.clone(),
            snapshot_digest: snap.digest(),
        });
        Ok(snap)
    }

    /// Partial observation: run the real checker and record what it says.
    ///
    /// `G02-partial` — when the program does not compile, the observer must
    /// still return something useful and must NOT fabricate. So a fact it
    /// cannot determine is `Observed::Unknown` WITH A REASON, and every
    /// omission is reported. An observation that silently dropped what it
    /// could not see would be indistinguishable from a complete one.
    pub fn observe(&mut self, snap: &WorkspaceSnapshot, target: &str) -> Observation {
        let out = self.run_axon_check(&self.workspace.join(target), None);
        let (diagnostics, warnings, facts, omissions) = match out {
            Ok((text, _)) => {
                let diags: Vec<String> = text
                    .lines()
                    .filter(|l| l.contains("\"severity\":\"error\""))
                    .map(|l| l.to_string())
                    .collect();
                // Warnings were previously discarded by the error-only filter,
                // which made a program carrying `W0005 unreachable code`
                // indistinguishable from a clean one. They are a different
                // signal, not a lesser one: the program compiles AND the
                // checker found something.
                let warns: Vec<String> = text
                    .lines()
                    .filter(|l| l.contains("\"severity\":\"warning\""))
                    .map(|l| l.to_string())
                    .collect();
                let compiles = diags.is_empty();
                let mut facts = vec![(
                    "compiles".to_string(),
                    Observed::known(compiles.to_string()),
                )];
                // `compiles` alone cannot express "builds, but the checker
                // objected". A repair agent needs that distinction to know
                // whether it is done.
                facts.push((
                    "compiles_cleanly".to_string(),
                    Observed::known((compiles && warns.is_empty()).to_string()),
                ));
                facts.push((
                    "warning_count".to_string(),
                    Observed::known(warns.len().to_string()),
                ));
                // The CODES, not just a count — a consumer choosing what to do
                // next needs to know it is W0005 (dead code) rather than
                // E0302 (dropped Result), and re-parsing raw JSON at every
                // call site is how that knowledge gets lost.
                let mut codes: Vec<String> = warns
                    .iter()
                    .filter_map(|l| l.split("\"code\":\"").nth(1))
                    .filter_map(|r| r.split('"').next())
                    .map(|c| c.to_string())
                    .collect();
                codes.sort();
                codes.dedup();
                facts.push((
                    "warning_codes".to_string(),
                    if warns.is_empty() {
                        Observed::known(String::new())
                    } else if codes.is_empty() {
                        // Warnings exist but carry no parseable code. Unknown
                        // WITH A REASON beats an empty string that reads as
                        // "no codes".
                        Observed::unknown("warnings present but none carried a `code` field")
                    } else {
                        Observed::known(codes.join(","))
                    },
                ));
                let mut omissions = Vec::new();
                // The type of the target symbol is only knowable once the
                // program type-checks. Unknown WITH A REASON rather than a
                // fabricated "i64".
                if compiles {
                    facts.push(("target_type_resolved".into(), Observed::known("yes".into())));
                } else {
                    facts.push((
                        "target_type_resolved".into(),
                        Observed::unknown("program does not type-check; types unresolved"),
                    ));
                    omissions.push(
                        "target_type_resolved: withheld because the program does not type-check"
                            .to_string(),
                    );
                }
                (diags, warns, facts, omissions)
            }
            Err(e) => (
                Vec::new(),
                // NOT an empty warning list: the checker did not run, so
                // "no warnings" is unknown rather than observed. The omission
                // below carries the reason.
                Vec::new(),
                vec![(
                    "compiles".to_string(),
                    Observed::unknown(format!("checker could not run: {e}")),
                )],
                vec![
                    format!("compiles: checker unavailable ({e})"),
                    format!("warnings: not observed, checker unavailable ({e})"),
                ],
            ),
        };
        let obs = Observation {
            observation_id: "o1".into(),
            snapshot_id: snap.snapshot_id.clone(),
            diagnostics,
            warnings,
            facts,
            omission_report: omissions,
        };
        self.episode.push(EpisodeEvent::Observed {
            observation_id: obs.observation_id.clone(),
            fact_count: obs.facts.len(),
            omission_count: obs.omission_report.len(),
        });
        obs
    }

    /// Authorize a TYPED action. This is the real boundary; the `&str` form
    /// below exists for the edge where a request arrives as JSON from another
    /// process and must be parsed before it can be trusted.
    ///
    /// The authority a variant needs is a property of the variant, so the
    /// nonsense combinations the string API could express are not decided here
    /// — they cannot be built. `ClaimDone` carries no path, so no path check is
    /// skipped for it; there is nothing to skip.
    pub fn authorize_action<'a>(
        &mut self,
        action: &'a CortexAction,
        grant: Option<&EditGrant>,
        principal: &str,
        current_snapshot: &WorkspaceSnapshot,
    ) -> Result<Authorized<'a>, Refusal> {
        let r = self.check_typed_authority(action, grant, principal, current_snapshot);
        match r {
            // BOTH halves recorded. `ActionDenied`'s own doc claims it is
            // "recorded with the same weight as an allowed one", and the
            // weight was zero on one side: `ActionAllowed` was defined,
            // documented, and never constructed, so an episode could show what
            // it was stopped from doing and never what it was permitted to do.
            // `grant_id` was written at three production sites and read
            // nowhere — this is the field it existed for.
            Ok(()) => {
                self.episode.push(EpisodeEvent::ActionAllowed {
                    action: action.name().to_string(),
                    // An action needing no grant records the ABSENCE rather
                    // than borrowing a name: "no grant was required" and "a
                    // grant authorised this" are different facts.
                    grant_id: grant
                        .map(|g| g.grant_id.clone())
                        .unwrap_or_else(|| "<none required>".to_string()),
                    target_digest: action
                        .write_target()
                        .map(|p| content_digest(p.as_bytes()))
                        .unwrap_or_default(),
                });
                Ok(Authorized { action })
            }
            Err(why) => {
                self.episode.push(EpisodeEvent::ActionDenied {
                    action: action.name().to_string(),
                    reason: why.to_string(),
                });
                Err(why)
            }
        }
    }

    /// Carry out an authorized action. Closes the loop: observe -> select ->
    /// authorize -> EXECUTE -> verify.
    ///
    /// Takes [`Authorized`], not a bare action. An unauthorized action is not
    /// rejected here — it cannot be passed here, because the caller has no way
    /// to build the witness except by clearing the gate. That is the same move
    /// as the typed catalog: push the failure from a runtime check that must be
    /// remembered to a value that cannot be produced.
    pub fn execute(&mut self, auth: Authorized<'_>) -> ExecOutcome {
        match auth.action() {
            CortexAction::Inspect { target } => {
                let p = self.workspace.join(&target.path);
                let src = match std::fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(e) => {
                        return ExecOutcome::Failed(format!("cannot read {}: {e}", target.path))
                    }
                };
                match symbol_body(&src, &target.symbol) {
                    Some((a, b)) => ExecOutcome::Inspected {
                        symbol: target.symbol.clone(),
                        body: src[a..b].to_string(),
                    },
                    None => ExecOutcome::SymbolNotFound {
                        symbol: target.symbol.clone(),
                        path: target.path.clone(),
                    },
                }
            }
            CortexAction::RunCheck { check } => {
                match self.run_named_check(&check.name, &check.path) {
                    Ok((passed, matched)) => ExecOutcome::CheckRan {
                        name: check.name.clone(),
                        passed,
                        matched,
                    },
                    Err(e) if bound_exceeded(&e).is_some() => ExecOutcome::CheckUnfinished {
                        name: check.name.clone(),
                        why: e.to_string(),
                    },
                    Err(e) => ExecOutcome::Failed(format!("check could not run: {e}")),
                }
            }
            CortexAction::PatchSymbolBody {
                symbol,
                proposed_body,
            } => {
                let p = self.workspace.join(&symbol.path);
                let src = match std::fs::read_to_string(&p) {
                    Ok(s) => s,
                    Err(e) => {
                        return ExecOutcome::Failed(format!("cannot read {}: {e}", symbol.path))
                    }
                };
                let Some((a, b)) = symbol_body(&src, &symbol.symbol) else {
                    // NOT a silent no-op. The old apply_patch returned
                    // Ok(false) when its needle was absent, which a caller
                    // could read as "applied, nothing changed".
                    return ExecOutcome::SymbolNotFound {
                        symbol: symbol.symbol.clone(),
                        path: symbol.path.clone(),
                    };
                };
                let after = format!("{}{}{}", &src[..a], proposed_body, &src[b..]);
                if let Err(e) = std::fs::write(&p, &after) {
                    return ExecOutcome::Failed(format!("cannot write {}: {e}", symbol.path));
                }
                self.episode.push(EpisodeEvent::PatchApplied {
                    path: symbol.path.clone(),
                    before_digest: content_digest(src.as_bytes()),
                    after_digest: content_digest(after.as_bytes()),
                });
                ExecOutcome::Patched {
                    path: symbol.path.clone(),
                    symbol: symbol.symbol.clone(),
                }
            }
            CortexAction::ClaimDone { claim } => {
                // No filesystem effect, by construction: the variant carries no
                // path and this arm touches nothing. A claim is evidence for
                // verify() to adjudicate, not an act that closes the task.
                self.episode.push(EpisodeEvent::Claimed {
                    rationale: claim.rationale.clone(),
                });
                ExecOutcome::Claimed {
                    rationale: claim.rationale.clone(),
                }
            }
        }
    }

    /// Drive a bounded repair episode: observe → select → authorize → execute,
    /// re-observing after each step, until the work is verified done or the
    /// loop can honestly say why it stopped.
    ///
    /// `hidden_check` is the independent adjudicator for a completion claim. A
    /// claim is never self-certifying: when selection proposes `ClaimDone`, the
    /// loop executes it and then asks `verify()`, and a failed verification does
    /// NOT end the episode — it is evidence that the claim was wrong.
    ///
    /// Two distinct stopping conditions that a single "it stopped" would hide:
    ///
    /// * `BudgetExhausted` — more steps might have finished it. A tuning
    ///   problem.
    /// * `NoProgress` — the workspace did not change and the same action was
    ///   chosen again, so more steps cannot help. A capability problem.
    ///
    /// Conflating them is how a loop that is stuck gets read as a loop that was
    /// merely rushed.
    pub fn run_episode(
        &mut self,
        target: &crate::action::SymbolRef,
        grant: Option<&EditGrant>,
        principal: &str,
        hidden_check: &str,
        budget: usize,
        generator: Option<&dyn crate::generate::PatchGenerator>,
    ) -> EpisodeOutcome {
        self.run_episode_resumable(
            target,
            grant,
            principal,
            hidden_check,
            budget,
            generator,
            &mut CandidateProgress::default(),
        )
    }

    /// [`run_episode`](Self::run_episode), continuing from `progress` and
    /// stopping early at `progress.proposal_limit`.
    ///
    /// `budget` is the candidate's TOTAL step budget, not a fresh allowance:
    /// a resumed episode continues counting from `progress.steps()`. A pause is
    /// reported as `BudgetExhausted` with `progress.paused()` set, and happens
    /// only at the point an unpaused episode would ask the generator for its
    /// next body — so nothing the episode has done is left half-adjudicated.
    #[allow(clippy::too_many_arguments)]
    pub fn run_episode_resumable(
        &mut self,
        target: &crate::action::SymbolRef,
        grant: Option<&EditGrant>,
        principal: &str,
        hidden_check: &str,
        budget: usize,
        generator: Option<&dyn crate::generate::PatchGenerator>,
        progress: &mut CandidateProgress,
    ) -> EpisodeOutcome {
        // Every (workspace state, action) pair already tried. A SET, not just
        // the previous step: the loop alternates patch → claim → patch, so a
        // last-step comparison cannot see a two-step cycle and the episode
        // would spin until the budget ran out — reporting "out of budget" for
        // what is really "going in circles".
        progress.paused = false;
        let seen = &mut progress.seen;
        // The episode NAMES this run. Every Runner was built with the same
        // constant, so the id could not tell two repairs apart — which is what
        // an id is for. Derived from the target and the adjudicator, so it is
        // deterministic (a replay of the same request produces the same id)
        // and distinguishing (a different target or a different grader does
        // not).
        self.episode.episode_id = format!(
            "cortex-repair:{}:{}:{}",
            target.path, target.symbol, hidden_check
        );
        // A visible copy is removed after each run, but a crash mid-run leaves
        // it beside the file. Swept once, when the episode first starts. It
        // never reaches the evidence either way: snapshots cover only the
        // target path and every check names one file, never a directory.
        if progress.baseline_passing.is_none() {
            sweep_stale_visible_copies(&self.workspace, &target.path);
        }
        let ctx = &mut progress.ctx;
        // Attempts that did not stick, and the body most recently applied.
        // Carried across steps so the generator is not asked the same question
        // with no record of what its last answer was.
        // The grant, with its state-pin advanced across transitions THIS
        // episode performed. Everything that confers authority — the principal,
        // the write prefixes, the grant id — is copied once and never touched.
        //
        // The pin exists so authority issued over state A cannot be spent on a
        // state the granter never saw. Refusing to advance it at all does not
        // achieve that; it makes a second edit impossible, so no repair can
        // take more than one attempt. And the property it buys survives the
        // narrowing: a change the episode did NOT make leaves the workspace
        // digest different from the one it recorded, and the next step is
        // refused as stale. The check goes from "anything moved" to "something
        // other than us moved", which is what it was for.
        // Which checks PASSED before anything was touched.
        //
        // The adjudicator is necessary but not sufficient: it is one test, and
        // a patch can satisfy it while breaking others. Measured with a
        // deliberately wrong generator, 4 of 68 runs exited 0 with a file that
        // still failed — every one of them a patch to the WRONG function that
        // the single hidden check happened to accept.
        //
        // Captured once, at the start, because the question is whether THIS
        // episode broke something that worked, not whether the file was
        // perfect to begin with.
        //
        // An error here empties the set, which would silently disable the
        // "this episode BROKE X" attribution below. It is recorded rather than
        // swallowed: a run that could not establish its baseline must not
        // later imply it found no regressions.
        //
        // A resumed episode keeps the baseline it took when it first started:
        // the file is back at those bytes, and re-recording an unavailable
        // baseline would log one failure twice.
        let baseline_passing: Vec<String> = match &progress.baseline_passing {
            Some(b) => b.clone(),
            None => {
                let b = match self.check_outcomes(&target.path, "") {
                    Ok((failing, passing)) => {
                        progress.baseline_failing = Some(failing);
                        passing
                    }
                    Err(e) => {
                        self.episode.push(EpisodeEvent::CheckRun {
                            name: format!("baseline UNAVAILABLE: {e}"),
                            exit_code: -1,
                            passed: false,
                        });
                        Vec::new()
                    }
                };
                progress.baseline_passing = Some(b.clone());
                b
            }
        };
        let mut effective: Option<EditGrant> = grant.cloned();
        let rejected = &mut progress.rejected;
        let last_applied = &mut progress.last_applied;
        // The file as this candidate found it. Every proposal is a replacement
        // for THIS, not for whatever the previous proposal left behind.
        let candidate_base: Option<String> =
            std::fs::read_to_string(self.workspace.join(&target.path)).ok();
        // …and the state-pin that went with it. Undoing a patch must put the
        // grant back where it was, not re-pin it to NOW: re-pinning advanced a
        // grant whose workspace had already moved before the episode began,
        // laundering a stale grant into a current one. The suite caught it —
        // a row that must refuse returned VerifiedDone.
        let base_pin: Option<String> = effective.as_ref().map(|g| g.snapshot_id.clone());
        for step in progress.steps + 1..=budget {
            progress.steps = step;
            let snap = match self.snapshot(&[target.path.as_str()]) {
                Ok(s) => s,
                Err(e) => {
                    return EpisodeOutcome::Blocked {
                        steps: step,
                        reason: format!("cannot snapshot {}: {e}", target.path),
                    }
                }
            };
            let obs = self.observe(&snap, &target.path);
            let action = match crate::select::select_action_with(&obs, target, ctx) {
                crate::select::Selection::Act(a) => a,
                crate::select::Selection::Blocked(reason) => {
                    return EpisodeOutcome::Blocked {
                        steps: step,
                        reason,
                    }
                }
            };

            // THE EARLY STOP, at the one point an episode spends a proposal and
            // before the pause: the previous proposal has been adjudicated and
            // undone and the next step would be a model call. Read only from
            // what the visible checks said about this candidate's own rejected
            // proposals.
            if let CortexAction::PatchSymbolBody { proposed_body, .. } = &action {
                if proposed_body.is_empty() {
                    if let (Some(limit), Some(base)) =
                        (progress.inert_limit, progress.baseline_failing.as_ref())
                    {
                        let visible: Vec<String> = base
                            .iter()
                            .filter(|n| *n != hidden_check)
                            .cloned()
                            .collect();
                        if crate::earlystop::should_stop(limit, &visible, &progress.attempts) {
                            progress.steps = step - 1;
                            return EpisodeOutcome::OutOfScopeSuspected {
                                steps: step - 1,
                                reason: crate::earlystop::unmoved_reason(
                                    &target.symbol,
                                    limit,
                                    &visible,
                                ),
                            };
                        }
                    }
                }
            }
            // THE PAUSE, at the same point. The file is back at the candidate's
            // base and nothing is left half-decided. The step is handed back: it
            // has not been taken. The early stop is read first, so a candidate
            // that must stop is not paused for a resumption that would stop it.
            if let CortexAction::PatchSymbolBody { proposed_body, .. } = &action {
                if proposed_body.is_empty()
                    && progress
                        .proposal_limit
                        .is_some_and(|cap| progress.proposals >= cap)
                {
                    progress.steps = step - 1;
                    progress.paused = true;
                    return EpisodeOutcome::BudgetExhausted { steps: step - 1 };
                }
            }

            // Stuck detection BEFORE acting: identical state plus identical
            // choice means the previous step achieved nothing and this one will
            // achieve the same.
            // The stuck-check has to know what the episode has LEARNED, not
            // only where it is. Undoing a rejected patch returns the workspace
            // to a state already visited, so a plain (state, action) pair made
            // the loop stop the moment the generator was about to try its
            // second idea — futility declared on a cycle the reset created.
            //
            // The rejected-attempt count is the missing coordinate: same state,
            // same action, but strictly more known-bad answers is a different
            // situation. It stays a real futility check because `rejected` is
            // deduplicated — a generator that repeats itself does not grow it,
            // so the pair repeats and the episode stops.
            let point = (
                snap.snapshot_id.clone(),
                action.name().to_string(),
                rejected.len(),
            );
            if seen.contains(&point) {
                return EpisodeOutcome::NoProgress {
                    steps: step,
                    action: action.name().to_string(),
                };
            }
            seen.push(point);

            // The creative step. Selection proposes a patch with an EMPTY body;
            // filling it is the one thing the control loop cannot do itself.
            // The generator contributes DATA — it is never handed a tool, never
            // chooses an action, and what it returns re-enters the same typed
            // pipeline as anything else.
            let action = if let CortexAction::PatchSymbolBody {
                symbol,
                proposed_body,
            } = &action
            {
                if proposed_body.is_empty() {
                    // AUTHORITY FIRST, before the generator is asked. The
                    // question "may this principal edit this path, under a
                    // grant still valid for this state" does not depend on the
                    // body, so asking it afterwards would spend a model call on
                    // an edit that could never be applied — and then report the
                    // result as NeedsInput, which blames the missing content
                    // for what is really a missing permission.
                    //
                    // Dry, so the probe does not put an authorization event in
                    // the episode for an action that has not been built yet.
                    if let Err(why) =
                        self.authorize_action_dry(&action, effective.as_ref(), principal, &snap)
                    {
                        return EpisodeOutcome::Refused {
                            steps: step,
                            reason: why.to_string(),
                        };
                    }
                    let Some(gen) = generator else {
                        return EpisodeOutcome::NeedsInput {
                            steps: step,
                            what: format!(
                                "a patch body for `{}` and no generator was supplied",
                                target.symbol
                            ),
                        };
                    };
                    // THE WALK BUDGET (R3), before anything is spent on the
                    // call: once it is out no generator is asked anything,
                    // in this episode or any later one. The step is handed
                    // back, as at the pause: it has not been taken.
                    if let Err(by) = self.walk.admit(gen.uncancellable_for()) {
                        progress.steps = step - 1;
                        return EpisodeOutcome::WalkBudgetExhausted {
                            steps: step - 1,
                            by,
                        };
                    }
                    // What the generator is allowed to see, assembled by
                    // Cortex through its own typed read rather than by the
                    // generator opening the file. If the symbol cannot be
                    // located the request stops here: proposing a replacement
                    // for a body nobody could read is guessing.
                    let current_body =
                        match std::fs::read_to_string(self.workspace.join(&symbol.path))
                            .ok()
                            .and_then(|src| {
                                symbol_body(&src, &symbol.symbol)
                                    .map(|(a, b)| src[a..b].to_string())
                            }) {
                            Some(b) => b,
                            // Not Blocked: the environment is fine and the
                            // checker ran. The target simply is not there,
                            // which has its own answer and its own remedy.
                            None => {
                                return EpisodeOutcome::NoSuchSymbol {
                                    steps: step,
                                    symbol: symbol.symbol.clone(),
                                    path: symbol.path.clone(),
                                }
                            }
                        };
                    // The view is built once per candidate, from the file as
                    // the candidate found it: every proposal replaces the same
                    // body of the same file, so the material does not change.
                    if progress.gen_context.is_none() {
                        progress.gen_context = Some(self.generator_context(
                            &target.path,
                            &symbol.symbol,
                            hidden_check,
                            candidate_base.as_deref(),
                            progress.baseline_failing.as_deref(),
                        ));
                    }
                    let constraints = crate::generate::PatchConstraints {
                        symbol: symbol.clone(),
                        max_bytes: 4096,
                        current_body,
                        rejected: rejected.clone(),
                        context: progress.gen_context.clone().expect("built just above"),
                        deadline: self.walk.deadline,
                    };
                    progress.proposals += 1;
                    let proposal = match gen.propose(&obs, target, &constraints) {
                        Ok(p) => p,
                        Err(why) => {
                            // Cut at the walk's deadline: the walk's own
                            // limit, not a generator that failed to answer.
                            if self.walk.deadline_passed() {
                                self.walk.expire();
                                return EpisodeOutcome::WalkBudgetExhausted {
                                    steps: step,
                                    by: WalkLimit::Deadline,
                                };
                            }
                            // A generator that cannot or will not propose is
                            // NeedsInput, not Blocked: authority and environment
                            // are fine, the missing piece is content.
                            return EpisodeOutcome::NeedsInput {
                                steps: step,
                                what: format!("{why} (generator `{}`)", gen.id()),
                            };
                        }
                    };
                    // Re-checked rather than trusted. "The constraints were
                    // passed in" is not evidence they were honoured.
                    if let Err(why) = crate::generate::validate(&proposal, &constraints) {
                        return EpisodeOutcome::NeedsInput {
                            steps: step,
                            what: format!("{why} (generator `{}`)", proposal.generator_id),
                        };
                    }
                    // WHO proposed it, recorded before it is applied. Once two
                    // generators exist the only interesting question is which
                    // produced a given outcome, and an episode that did not
                    // record it cannot answer that later.
                    self.episode.push(EpisodeEvent::Proposed {
                        generator_id: proposal.generator_id.clone(),
                    });
                    // R1b — ADMISSION, before the candidate executes. The
                    // patched text is walked IN MEMORY by the same
                    // classifier `@[contained]` uses; a body reaching a
                    // capability the original target did not hold never
                    // reaches a check. A rejected attempt, not a terminal:
                    // the generator is told which kinds were refused and the
                    // next proposal runs. A candidate that does not compile
                    // goes on to meet that failure where every other one
                    // does (`axon test` aborts on a type error, so nothing of
                    // it executes).
                    if let Some(grant) = self
                        .static_grant
                        .as_ref()
                        .filter(|g| g.symbol == symbol.symbol)
                    {
                        let path = self.workspace.join(&symbol.path);
                        let Some(patched) = std::fs::read_to_string(&path).ok().and_then(|src| {
                            symbol_body(&src, &symbol.symbol).map(|(a, b)| {
                                format!("{}{}{}", &src[..a], proposal.body, &src[b..])
                            })
                        }) else {
                            return EpisodeOutcome::NoSuchSymbol {
                                steps: step,
                                symbol: symbol.symbol.clone(),
                                path: symbol.path.clone(),
                            };
                        };
                        let base_dir = path.parent().unwrap_or(&self.workspace).to_path_buf();
                        let wall = self.checker_limits.wall.unwrap_or(CAPS_WALL_DEFAULT);
                        match crate::fence::caps_of_text(
                            &self.axon_bin,
                            &patched,
                            &base_dir,
                            Some(&symbol.symbol),
                            wall,
                        ) {
                            Ok(caps) => {
                                let excess = grant.excess(&caps);
                                if !excess.is_empty() {
                                    self.episode.push(EpisodeEvent::ActionDenied {
                                        action: action.name().to_string(),
                                        reason: format!("capability:{}", excess.join(",")),
                                    });
                                    progress.attempts.push(AttemptSignal {
                                        proposal: progress.proposals,
                                        rejection:
                                            crate::generate::RejectionReason::CapabilityDenied,
                                        visible_failing: None,
                                    });
                                    push_rejected(
                                        rejected,
                                        proposal.body,
                                        crate::generate::RejectionReason::CapabilityDenied,
                                        Some(crate::feedback::Feedback::refused(excess)),
                                    );
                                    continue;
                                }
                            }
                            Err(crate::fence::CapsError::DidNotCompile(_)) => {}
                            // The walker could not be run: nothing is known
                            // about what the candidate reaches, so it does not
                            // run either. An environment fact.
                            Err(crate::fence::CapsError::Failed(why)) => {
                                return EpisodeOutcome::Blocked {
                                    steps: step,
                                    reason: format!(
                                        "the candidate's capabilities could not be established: {why}"
                                    ),
                                };
                            }
                        }
                    }
                    *last_applied = Some(proposal.body.clone());
                    CortexAction::PatchSymbolBody {
                        symbol: symbol.clone(),
                        proposed_body: proposal.body,
                    }
                } else {
                    action.clone()
                }
            } else {
                action
            };

            // THE GRADER IS NOT PATCHABLE. Defence in depth: ranking already
            // refuses to nominate a test, and an operator can still name one
            // with --symbol. Rewriting the check that decides whether the work
            // is done to `assert(true)` passes every subsequent adjudication
            // while the defect stands — the strongest possible false success,
            // and the exact inverse of what this loop claims to provide.
            if let CortexAction::PatchSymbolBody { symbol, .. } = &action {
                if symbol.symbol == hidden_check {
                    return EpisodeOutcome::Refused {
                        steps: step,
                        reason: format!(
                            "`{}` is the check that adjudicates this repair; \
                             patching it would grade the work against bytes the \
                             patch just wrote",
                            symbol.symbol
                        ),
                    };
                }
            }

            let auth = match self.authorize_action(&action, effective.as_ref(), principal, &snap) {
                Ok(a) => a,
                Err(why) => {
                    return EpisodeOutcome::Refused {
                        steps: step,
                        reason: why.to_string(),
                    }
                }
            };
            // The refusal described the state BEFORE this edit. Once a patch
            // lands, that description is stale: the loop must re-observe and
            // re-claim rather than keep patching on the strength of a verdict
            // about code that no longer exists. Left sticky, a CORRECT repair
            // is followed by another patch and the episode never converges.
            if matches!(action, CortexAction::PatchSymbolBody { .. }) {
                ctx.claim_refused = false;
                // The check's answer described the code BEFORE this edit, and
                // the edit is the whole point. Left stale, a SUCCESSFUL patch
                // is followed by another patch — the generator repeats itself,
                // nothing changes, and the loop reports going in circles on a
                // repair that had already worked.
                ctx.last_check = crate::select::VisibleCheck::NotRun;
            }
            // The bytes as they stand, kept only for the one action that can
            // damage them. A controller that can leave a workspace worse than
            // it found it and walk away is not a safety mechanism, and that is
            // exactly what a generator proposing an unparseable body did:
            // exit non-zero with the file no longer compiling.
            let rollback = if matches!(action, CortexAction::PatchSymbolBody { .. }) {
                std::fs::read_to_string(self.workspace.join(&target.path)).ok()
            } else {
                None
            };
            let outcome = self.execute(auth);
            // What execute REPORTED, acted on. These used to be discarded, so
            // an unwritable file and a mistyped symbol both came back as the
            // loop "going in circles" or as a broken checker — verdicts about
            // the generator and about the environment, for failures that were
            // neither.
            // The check's ANSWER feeds the next selection. Without this the
            // loop ran a check, saw an unchanged workspace, chose the same
            // action, and stopped as NoProgress — so a file that compiles with
            // warnings could never reach a patch at all.
            if let ExecOutcome::CheckRan {
                passed, matched, ..
            } = &outcome
            {
                use crate::select::VisibleCheck;
                // Zero matched is neither a pass nor a failure: it is no
                // answer, and it needs its OWN state. Folding it into "not yet
                // run" left the loop choosing the same action against an
                // unchanged workspace until stuck-detection fired — the
                // warned-file stall, still open for the 72.6% of symbols whose
                // name matches no test.
                ctx.last_check = match (*matched > 0, *passed) {
                    (false, _) => VisibleCheck::NoSuchCheck,
                    (true, true) => VisibleCheck::Passed,
                    (true, false) => VisibleCheck::Failed,
                };
                // A failing check is also the moment the PREVIOUS patch is
                // known not to have worked. Without this the check-driven
                // patch path recorded no rejection at all, so the generator
                // was asked the same question with an empty history every time
                // and a deterministic one repeated itself until the budget ran
                // out.
                if ctx.last_check == VisibleCheck::Failed {
                    if let Some(body) = last_applied.take() {
                        let (feedback, visible_failing) = self.visible_feedback_full(
                            &target.path,
                            hidden_check,
                            VisibleRun::Tests,
                        );
                        progress.attempts.push(AttemptSignal {
                            proposal: progress.proposals,
                            rejection: crate::generate::RejectionReason::CheckRefused,
                            visible_failing,
                        });
                        push_rejected(
                            rejected,
                            body,
                            crate::generate::RejectionReason::CheckRefused,
                            feedback,
                        );
                        // UNDO it here too. This is the path a file compiling
                        // with warnings takes, and it is the majority of real
                        // files — so without this, patches accumulated exactly
                        // as they did before the undo was introduced, and the
                        // generator was shown its own rejected proposal as
                        // "the body currently" while being told that same body
                        // was rejected.
                        if restore_base(&self.workspace.join(&target.path), &candidate_base)
                            .is_err()
                        {
                            return EpisodeOutcome::Blocked {
                                steps: step,
                                reason: format!(
                                    "a rejected patch could not be undone in {}",
                                    target.path
                                ),
                            };
                        }
                        if let (Some(g), Some(pin)) = (effective.as_mut(), base_pin.as_ref()) {
                            g.snapshot_id = pin.clone();
                        }
                        // The check described the patched file; the file is no
                        // longer that file.
                        ctx.last_check = VisibleCheck::NotRun;
                    }
                }
            }

            // R2: the symbol's check ran past a process bound ON A CANDIDATE.
            // The candidate's code is what ran too long or said too much, so
            // the candidate is rejected and undone exactly as a failing check
            // rejects it; the run goes on to the next proposal. On the
            // pristine file it is still an environment fact (Blocked below).
            if let ExecOutcome::CheckUnfinished { name, why } = &outcome {
                if let Some(body) = last_applied.take() {
                    self.episode.push(EpisodeEvent::CheckRun {
                        name: format!("{name} (unfinished: {why})"),
                        exit_code: -1,
                        passed: false,
                    });
                    let feedback =
                        self.visible_feedback(&target.path, hidden_check, VisibleRun::Tests);
                    progress.attempts.push(AttemptSignal {
                        proposal: progress.proposals,
                        rejection: crate::generate::RejectionReason::TimedOut,
                        visible_failing: None,
                    });
                    push_rejected(
                        rejected,
                        body,
                        crate::generate::RejectionReason::CheckRefused,
                        feedback,
                    );
                    if restore_base(&self.workspace.join(&target.path), &candidate_base).is_err() {
                        return EpisodeOutcome::Blocked {
                            steps: step,
                            reason: format!(
                                "a candidate whose check did not finish could not be undone in {}",
                                target.path
                            ),
                        };
                    }
                    if let (Some(g), Some(pin)) = (effective.as_mut(), base_pin.as_ref()) {
                        g.snapshot_id = pin.clone();
                    }
                    ctx.last_check = crate::select::VisibleCheck::NotRun;
                    continue;
                }
            }
            match &outcome {
                ExecOutcome::CheckUnfinished { why, .. } => {
                    return EpisodeOutcome::Blocked {
                        steps: step,
                        reason: format!("the check did not finish: {why}"),
                    }
                }
                ExecOutcome::Failed(why) => {
                    return EpisodeOutcome::Blocked {
                        steps: step,
                        reason: format!("the action could not be carried out: {why}"),
                    }
                }
                ExecOutcome::SymbolNotFound { symbol, path } => {
                    return EpisodeOutcome::NoSuchSymbol {
                        steps: step,
                        symbol: symbol.clone(),
                        path: path.clone(),
                    }
                }
                _ => {}
            }

            if let Some(before) = rollback {
                // "Worse" is defined narrowly and mechanically: it compiled,
                // and now it does not. NOT "more warnings" or "fewer tests
                // passing" — those are judgements, and a rollback rule built on
                // a judgement would start reverting repairs that were working.
                let re = self.snapshot(&[target.path.as_str()]).ok();
                // Only a KNOWN failure to compile triggers a revert. An
                // observation that could not tell is not evidence of damage,
                // and undoing a patch on "I could not check" would discard
                // work on the strength of a checker that did not run.
                //
                // Nothing tests that branch, because nothing can reach it: a
                // patch is only selected after a completion claim was refused,
                // which requires the file to have compiled, and an Unknown
                // observation blocks the episode before any action is chosen.
                // It is written for the direction it should fail in if that
                // ever stops being true.
                let broke = re
                    .as_ref()
                    .map(|s| self.observe(s, &target.path))
                    .map(|o| observed_compiles(&o) == Some(false))
                    .unwrap_or(false);
                if broke {
                    // The diagnostics, from the patched file with the hidden
                    // check cut out, read before the restore below.
                    let feedback = if last_applied.is_some() {
                        self.visible_feedback(&target.path, hidden_check, VisibleRun::Compile)
                    } else {
                        None
                    };
                    // Checked, not assumed. The event below asserts the file
                    // now holds these bytes; recording that after an ignored
                    // write would make the evidence record claim something
                    // about the filesystem it never verified — in the one
                    // event whose whole purpose is that a revert is recorded
                    // rather than performed silently.
                    if let Err(e) = std::fs::write(self.workspace.join(&target.path), &before) {
                        return EpisodeOutcome::Blocked {
                            steps: step,
                            reason: format!(
                                "a patch broke the build and {} could not be restored: {e}",
                                target.path
                            ),
                        };
                    }
                    if let Some(body) = last_applied.take() {
                        progress.attempts.push(AttemptSignal {
                            proposal: progress.proposals,
                            rejection: crate::generate::RejectionReason::DidNotCompile,
                            visible_failing: None,
                        });
                        push_rejected(
                            rejected,
                            body,
                            crate::generate::RejectionReason::DidNotCompile,
                            feedback,
                        );
                    }
                    self.episode.push(EpisodeEvent::PatchReverted {
                        path: target.path.clone(),
                        reason: "the patched file no longer compiles".to_string(),
                        restored_digest: content_digest(before.as_bytes()),
                    });
                    // The claim stands refused: the code is back to compiling
                    // and still wrong, which is where it was.
                    //
                    // Honestly: no test distinguishes this line today, and
                    // neutralising it changes nothing observable. The restore
                    // puts the file back byte-for-byte, so the cycle detector
                    // sees the same (state, action) pair and stops the episode
                    // before a second claim can be made either way. It is kept
                    // because it states the invariant the cycle detector
                    // happens to enforce — that the loop must not go back to
                    // claiming done on code it was just told is wrong — rather
                    // than leaving that resting on a coincidence of digests.
                    ctx.claim_refused = true;
                    // The pin follows the RESTORED state, not the patched one.
                    // Left pointing at bytes that were undone, every later step
                    // would be refused as stale — the episode would punish
                    // itself for cleaning up after a bad proposal.
                    if let (Some(g), Ok(now)) =
                        (effective.as_mut(), self.snapshot(&[target.path.as_str()]))
                    {
                        g.snapshot_id = now.snapshot_id;
                    }
                    continue;
                }
            }

            // The episode's own transition, recorded once the step's final
            // state is settled. A foreign edit in the window between the write
            // and this line is not the threat being guarded: the pin protects a
            // grant issued over a state the granter saw, against a workspace
            // that has since moved on.
            if matches!(action, CortexAction::PatchSymbolBody { .. }) {
                if let (Some(g), Ok(now)) =
                    (effective.as_mut(), self.snapshot(&[target.path.as_str()]))
                {
                    g.snapshot_id = now.snapshot_id;
                }
            }

            // A claim is adjudicated, never accepted. If verification holds the
            // episode is done; if it does not, the loop keeps going and the
            // next iteration's stuck-detection decides whether that is futile.
            if matches!(action, CortexAction::ClaimDone { .. }) {
                let _ = outcome;
                // Set when every check passed but `axon check` objected: the
                // undo below then records the attempt as `CheckerObjected`
                // with the checker's diagnostics as its feedback.
                let mut objected = false;
                // R2: set when a spawn of the acceptance stopped at a process
                // bound (time or output) on the candidate's file. That is a
                // fact about the CANDIDATE's code — it ran too long or said too
                // much — so it rejects the candidate like a refused claim and
                // never blocks the run. The generator is told only what the
                // visible run said: which acceptance spawn hit the bound is
                // the grader's business.
                // R5: the bytes about to be graded, read BEFORE the hidden
                // check spawns. Acceptance holds only for these bytes: after
                // the last acceptance spawn the file is read again, and any
                // difference fails closed. A file that cannot be read here
                // could not be graded as anything in particular.
                let graded = match std::fs::read(self.workspace.join(&target.path)) {
                    Ok(bytes) => sha256_hex(&bytes),
                    Err(e) => {
                        return EpisodeOutcome::Blocked {
                            steps: step,
                            reason: format!(
                                "{} could not be read before grading: {e}",
                                target.path
                            ),
                        };
                    }
                };
                let (verified, bound) =
                    self.verify_run(true, hidden_check, &target.path, Some(graded.clone()));
                let mut unfinished = bound.is_some();
                if verified {
                    'accept: {
                        // The adjudicator accepted. That is a witness, not the
                        // whole truth: it is ONE test, and a patch can satisfy it
                        // while the file still fails others.
                        //
                        // Measured with a deliberately wrong generator, 4 of 68
                        // runs exited 0 with a file that still failed — every one
                        // a patch to the WRONG function that the single hidden
                        // check happened to accept. A regression guard alone did
                        // not close it: in the surviving case nothing REGRESSED,
                        // the episode simply left failures it was there to fix.
                        //
                        // So the rule is the one an operator can act on: verified
                        // means NO check in the file is failing. A file that still
                        // fails checks has not been repaired, whatever the
                        // adjudicator says about its own test.
                        // FAIL CLOSED. `unwrap_or_default()` sat here and turned
                        // "the checks could not be run" into "nothing is failing"
                        // — on the ONLY path that can return VerifiedDone. That is
                        // the exact collapse `check_outcomes`' own doc forbids ten
                        // lines from where it is written, and it made the
                        // strictness fail OPEN: any second spawn that died (the
                        // binary replaced mid-run, an OOM kill) produced exit 0 on
                        // a file whose other checks all still failed.
                        let still_failing: Vec<String> = match self.check_outcomes(&target.path, "")
                        {
                            Ok((failing, _)) => failing,
                            Err(e) if bound_exceeded(&e).is_some() => {
                                self.episode.push(EpisodeEvent::Verified {
                                    passed: false,
                                    detail: format!(
                                    "claim_done=true: hidden check `{hidden_check}` passed, but \
                                     the re-check of the file did not finish, so the candidate \
                                     is rejected: {e}"
                                ),
                                    graded_sha256: None,
                                });
                                unfinished = true;
                                break 'accept;
                            }
                            Err(e) => {
                                self.episode.push(EpisodeEvent::Verified {
                                    passed: false,
                                    detail: format!(
                                    "claim_done=true: hidden check `{hidden_check}` passed, but \
                                     the file could not be re-checked afterwards, so nothing was \
                                     established about the rest of it: {e}"
                                ),
                                    graded_sha256: None,
                                });
                                return EpisodeOutcome::Blocked {
                                    steps: step,
                                    reason: format!("the file could not be re-checked: {e}"),
                                };
                            }
                        };
                        if still_failing.is_empty() {
                            // THE CHECKER HAS THE LAST WORD. `axon test` runs a
                            // file `axon check` refuses: `check` exits 2 on ANY
                            // diagnostic, a note included, and a test run does
                            // not. Measured on the dev corpus (drop-stmt-02, 4 of
                            // 4 reruns): a generator shown the file wrote
                            // `let mut s = start`, every test passed, this
                            // returned VerifiedDone, and the file it left failed
                            // `axon check` with the I0002 note "`mut` is not an
                            // Axon keyword". A repair the checker objects to is
                            // not finished, so it is rejected like any other and
                            // the generator is shown what the checker said.
                            match self.checker_exit_clean(&target.path) {
                                Ok(true) => match self.still_graded(&target.path, &graded) {
                                    Ok(()) => return EpisodeOutcome::VerifiedDone { steps: step },
                                    // R5: the deciding event above named bytes
                                    // that are no longer the file; this one,
                                    // appended after it, is the verdict now.
                                    Err(why) => {
                                        self.episode.push(EpisodeEvent::Verified {
                                            passed: false,
                                            detail: format!(
                                                "claim_done=true: hidden check `{hidden_check}` \
                                                 passed, but the workspace changed during \
                                                 acceptance: {why}"
                                            ),
                                            graded_sha256: None,
                                        });
                                    }
                                },
                                Ok(false) => {
                                    self.episode.push(EpisodeEvent::Verified {
                                        passed: false,
                                        detail: format!(
                                        "claim_done=true: hidden check `{hidden_check}` passed \
                                         and no check fails, but `axon check` on {} did not \
                                         exit 0 (it reported diagnostics)",
                                        target.path
                                    ),
                                        graded_sha256: None,
                                    });
                                    objected = true;
                                }
                                Err(e) if bound_exceeded(&e).is_some() => {
                                    self.episode.push(EpisodeEvent::Verified {
                                        passed: false,
                                        detail: format!(
                                        "claim_done=true: hidden check `{hidden_check}` passed \
                                         and no check fails, but `axon check` on the file did \
                                         not finish, so the candidate is rejected: {e}"
                                    ),
                                        graded_sha256: None,
                                    });
                                    unfinished = true;
                                }
                                // FAIL CLOSED, as for the re-check above: a
                                // checker that could not run established nothing.
                                Err(e) => {
                                    self.episode.push(EpisodeEvent::Verified {
                                        passed: false,
                                        detail: format!(
                                        "claim_done=true: hidden check `{hidden_check}` passed, \
                                         but `axon check` could not be run on the file, so its \
                                         acceptance was not established: {e}"
                                    ),
                                        graded_sha256: None,
                                    });
                                    return EpisodeOutcome::Blocked {
                                        steps: step,
                                        reason: format!("`axon check` could not be run: {e}"),
                                    };
                                }
                            }
                        } else {
                            let regressed: Vec<String> = still_failing
                                .iter()
                                .filter(|n| baseline_passing.contains(n))
                                .cloned()
                                .collect();
                            // NOTHING REGRESSED. The patch satisfied the adjudicator
                            // and broke nothing; the remaining failures were already
                            // there when the episode started, and this loop was never
                            // asked about them.
                            //
                            // Undoing it here discarded a repair that had WORKED, then
                            // recorded the correct body as `CheckRefused` — which the
                            // prompt renders as "it compiled but did not fix the
                            // problem" and adds to a do-not-propose list. The loop
                            // steered a competent generator away from the right answer
                            // and then reported exit 21, "the generator is not
                            // helping", about a generator that had already succeeded.
                            //
                            // So this is a terminal outcome of its own: the patch is
                            // KEPT, and the pre-existing failures are named.
                            if regressed.is_empty() {
                                // R5: `check_outcomes` was this path's last
                                // acceptance spawn; the patch is kept only if
                                // the file is still the bytes graded.
                                if let Err(why) = self.still_graded(&target.path, &graded) {
                                    self.episode.push(EpisodeEvent::Verified {
                                        passed: false,
                                        detail: format!(
                                            "claim_done=true: hidden check `{hidden_check}` \
                                             passed, but the workspace changed during \
                                             acceptance: {why}"
                                        ),
                                        graded_sha256: None,
                                    });
                                    break 'accept;
                                }
                                self.episode.push(EpisodeEvent::Verified {
                                    passed: false,
                                    detail: format!(
                                    "claim_done=true: hidden check `{hidden_check}` passed and \
                                     nothing regressed, but {} check(s) that were ALREADY \
                                     failing still fail: {}",
                                    still_failing.len(),
                                    still_failing.join(", ")
                                ),
                                    graded_sha256: Some(graded.clone()),
                                });
                                return EpisodeOutcome::AdjudicatedNotClean {
                                    steps: step,
                                    still_failing,
                                };
                            }
                            self.episode.push(EpisodeEvent::Verified {
                                passed: false,
                                detail: format!(
                                    "hidden check `{hidden_check}` passed, but {} check(s) \
                                 still fail ({}) — and this episode BROKE {} that passed \
                                 before it",
                                    still_failing.len(),
                                    still_failing.join(", "),
                                    regressed.join(", ")
                                ),
                                graded_sha256: None,
                            });
                        }
                    }
                }
                // The claim was refused. That is the signal the code compiles
                // and is still wrong — the only state in which proposing a
                // repair is justified rather than a guess.
                ctx.claim_refused = true;
                // UNDO the rejected patch. Each proposal replaces the SAME
                // body — the one this candidate started from — so the
                // rejected-attempts list and `current_body` describe the same
                // question. Leaving it applied made the next proposal a
                // replacement for code the last proposal had broken, and a
                // generator editing back toward the original then re-created
                // an earlier state and tripped the cycle detector on a cycle
                // the loop had manufactured.
                // What the VISIBLE checks say about the attempt (or, when the
                // checks all passed and `axon check` objected, what the
                // checker said), read before the undo below puts the file back.
                let (reason, what) = if objected {
                    (
                        crate::generate::RejectionReason::CheckerObjected,
                        VisibleRun::Diagnostics,
                    )
                } else {
                    (
                        crate::generate::RejectionReason::CheckRefused,
                        VisibleRun::Tests,
                    )
                };
                let (feedback, visible_failing) = if last_applied.is_some() {
                    let (text, failing) =
                        self.visible_feedback_full(&target.path, hidden_check, what);
                    // `axon check` objected only after every check passed, so
                    // no check was failing; a test run was not made for it.
                    (text, if objected { Some(Vec::new()) } else { failing })
                } else {
                    (None, None)
                };
                {
                    if restore_base(&self.workspace.join(&target.path), &candidate_base).is_err() {
                        return EpisodeOutcome::Blocked {
                            steps: step,
                            reason: format!(
                                "a rejected patch could not be undone in {}",
                                target.path
                            ),
                        };
                    }
                    // The pin the candidate STARTED with. Restoring it keeps
                    // a grant that was stale stale, and a grant that was
                    // current current — the file is back at exactly the state
                    // both statements were about.
                    if let (Some(g), Some(pin)) = (effective.as_mut(), base_pin.as_ref()) {
                        g.snapshot_id = pin.clone();
                    }
                }
                // The claim covered whatever was last applied, so that body is
                // now a known-insufficient attempt. Recorded WITHOUT the
                // hidden check's output: the generator learns its answer was
                // rejected and what the VISIBLE checks said about it (gathered
                // above, before the undo), not what the grader wanted.
                if let Some(body) = last_applied.take() {
                    progress.attempts.push(AttemptSignal {
                        proposal: progress.proposals,
                        rejection: if unfinished {
                            crate::generate::RejectionReason::TimedOut
                        } else {
                            reason.clone()
                        },
                        visible_failing,
                    });
                    push_rejected(rejected, body, reason, feedback);
                }
            }
        }
        EpisodeOutcome::BudgetExhausted { steps: budget }
    }

    /// Which checks FAIL and which PASS, excluding the one that adjudicates.
    ///
    /// Both halves, because localization ranks by spectrum: a function only
    /// failing checks reach is strong evidence, one every check reaches is
    /// weak. Reporting only the failures throws away the denominator and
    /// leaves every helper looking equally suspicious.
    ///
    /// An `Err` here is "the checks could not run", which is NOT "the checks
    /// passed". The two collapse into one the moment an error is reported as
    /// an empty list, and an empty list is exactly what a caller reads as a
    /// healthy file.
    pub fn check_outcomes(
        &self,
        rel_path: &str,
        hidden: &str,
    ) -> std::io::Result<(Vec<String>, Vec<String>)> {
        let (failed, passed, _) = self.run_tests_json(rel_path, None)?;
        Ok((
            failed.into_iter().filter(|n| n != hidden).collect(),
            passed.into_iter().filter(|n| n != hidden).collect(),
        ))
    }

    /// Run `axon test` and read its MACHINE-READABLE output.
    ///
    /// This used to parse the human transcript, and every shape that transcript
    /// can take which the parser did not expect was a silent wrong answer
    /// rather than an error:
    ///
    /// * `test NAME [should_fail] ... FAILED` — the annotation is printed ONLY
    ///   on the failure branch, so the name came out as `NAME [should_fail]`,
    ///   the `== hidden` comparison never matched, and the ADJUDICATING check
    ///   was fed to localization as ordinary evidence.
    /// * the classifier looked for `FAILED` before `ok` anywhere in the line,
    ///   so a PASSING test named `test_FAILED_path` was recorded as failed.
    /// * test bodies run in-process and their stdout is not captured, so a
    ///   `print` without a trailing newline glues the next result line to it
    ///   and that test vanishes from the run.
    ///
    /// Each of those is a different bug with one cause: the transcript is for
    /// people, and its shape may change for reasons that have nothing to do
    /// with this caller. `--json` is the contract meant to be parsed. A test
    /// body could still print a line that happens to be a matching JSON
    /// object; nothing here can prevent that, and it is a far narrower target
    /// than a line beginning with "test ".
    ///
    /// Returns `(failed, passed, total)`. `total` comes from the run's own
    /// summary, so "the filter matched nothing" is distinguishable from "every
    /// test passed" — those are byte-identical in the human transcript, and
    /// both exit 0.
    ///
    /// B264: this is now the ONE place a check is dispatched, and it goes
    /// through the [`CheckExecutor`] seam rather than spawning `axon_bin`
    /// directly. The parsing contract above lives in
    /// [`parse_axon_test_json`]; the executor decides WHAT runs.
    fn run_tests_json(
        &self,
        rel_path: &str,
        filter: Option<&str>,
    ) -> std::io::Result<(Vec<String>, Vec<String>, usize)> {
        let report = self.check_executor.run_checks(&CheckRequest {
            workspace: &self.workspace,
            rel_path,
            filter,
        })?;
        Ok((report.failed, report.passed, report.total))
    }

    /// Run ONE named check and report how many tests the name matched.    /// Run ONE named check and report how many tests the name matched.
    ///
    /// `run_check` passes the name to the episode record but not to the
    /// command, so it runs the whole file and reports a named result — the same
    /// defect `verify()` had. This selects, and returns the match count so a
    /// zero-match run can be told from a pass.
    fn run_named_check(&mut self, name: &str, rel_path: &str) -> std::io::Result<(bool, usize)> {
        let (failed, passed, _) = self.run_tests_json(rel_path, Some(name))?;
        // `--filter` is a SUBSTRING match, so it can select more than the
        // check asked for. The count reported is what actually ran under that
        // name, and a filter matching nothing exits 0 with an `ok` summary —
        // zero matched tests is not a pass.
        let matched = failed.len() + passed.len();
        // A DIFFERENT question from the hidden check's, and it took a
        // regression to make that clear. The adjudicator is an exact test name
        // the operator supplied, so its verdict must come from that exact name
        // (`verdict_for`). This check is named after the SYMBOL under repair,
        // which is not a test name at all — it is a filter selecting the tests
        // that touch that symbol, and the question is whether any of THEM
        // failed.
        //
        // Applying the exact-name rule here made the verdict permanently
        // false, because no test is ever called `gcounter_increment`. The loop
        // then ran the check, learned nothing, and went in circles.
        //
        // Zero matched is still not a pass: a filter matching nothing exits 0
        // and prints an `ok` summary.
        let ok = matched > 0 && failed.is_empty();
        self.episode.push(EpisodeEvent::CheckRun {
            name: name.to_string(),
            exit_code: i32::from(!ok),
            passed: ok,
        });
        Ok((ok, matched))
    }

    /// Run the adjudicating check and report `(passed, how many matched)`.
    ///
    /// Public because the decision that needs it belongs to the CALLER: an
    /// adjudicator that already passes cannot witness a repair, and that has to
    /// be established BEFORE an episode starts rather than discovered by an
    /// episode that then reports success for doing nothing.
    ///
    /// The match count is returned rather than folded into the boolean because
    /// "nothing matched" and "everything that matched failed" are different
    /// facts. A filter matching nothing exits 0 and prints an `ok` summary.
    pub fn run_hidden_check(&self, rel_path: &str, name: &str) -> std::io::Result<Option<bool>> {
        let (failed, passed, _) = self.run_tests_json(rel_path, Some(name))?;
        // `None` means NO TEST BY THAT NAME RAN — a different fact from "it
        // ran and failed", and collapsing them is how a `--check` naming an
        // ordinary function came to be treated as a failing adjudicator and
        // allowed the run to proceed. The size of the filtered set is
        // irrelevant here: a substring filter can run several tests, none of
        // them the one named.
        if passed.iter().any(|n| n == name) {
            Ok(Some(true))
        } else if failed.iter().any(|n| n == name) {
            Ok(Some(false))
        } else {
            Ok(None)
        }
    }

    /// [`Runner::visible_run`] on the file as it is on disk now — the attempt
    /// still applied. `None` when nothing could be gathered: a rejection
    /// without feedback is still a rejection, and nothing is invented for it.
    fn visible_feedback(
        &self,
        rel_path: &str,
        hidden: &str,
        what: VisibleRun,
    ) -> Option<crate::feedback::Feedback> {
        self.visible_feedback_full(rel_path, hidden, what).0
    }

    /// [`Runner::visible_feedback`] and, for a test run, the names of the
    /// visible checks that FAILED in it. The two come from the same run: the
    /// early stop reads the verdicts the generator is shown, and no extra run
    /// is made for them.
    fn visible_feedback_full(
        &self,
        rel_path: &str,
        hidden: &str,
        what: VisibleRun,
    ) -> (Option<crate::feedback::Feedback>, Option<Vec<String>>) {
        let Ok(now) = std::fs::read_to_string(self.workspace.join(rel_path)) else {
            return (None, None);
        };
        match self.visible_run(rel_path, &now, hidden, what) {
            Ok((fb, failing)) => (Some(fb), failing),
            Err(_) => (None, None),
        }
    }

    /// Whether `axon check` on the file under repair, as it is on disk now,
    /// exits 0. That is the acceptance an operator (and the benchmark grader)
    /// applies, and it is stricter than "no error diagnostics": `check` exits
    /// 2 on any diagnostic, a note included. `Err` when the checker could not
    /// be run or was killed, so a missing answer is never read as a clean one.
    fn checker_exit_clean(&self, rel_path: &str) -> std::io::Result<bool> {
        let (_, code) = self.run_axon_check(&self.workspace.join(rel_path), None)?;
        match code {
            Some(c) => Ok(c == 0),
            None => Err(std::io::Error::other("the checker was killed by a signal")),
        }
    }

    /// Run `text` — the file under repair as it is now, or a patched version
    /// of it — with the hidden check CUT OUT, and say what came back.
    ///
    /// This is how a generator learns what its attempt did without learning
    /// anything from the grader. Filtering the hidden check's result out of an
    /// ordinary run is not enough: the interpreter runs every test before it
    /// prints any result, so what the test bodies PRINT is not attributed to a
    /// test, and a filter by name would let the hidden check's own output
    /// through. A run of a file in which the hidden check does not exist
    /// cannot produce any of it. The same holds for compiler diagnostics,
    /// which point into the hidden check when a body change breaks a call in
    /// it.
    ///
    /// The copy is written beside the file (imports resolve as they would for
    /// it) under a dot-name; every string of what comes back has the copy's
    /// path and name replaced by the real ones (R4: no pid reaches a prompt),
    /// and the copy is removed before returning. Tests run through the
    /// registered [`CheckExecutor`], with its limits, since the patched code
    /// is a candidate's. `Err` when the name still appears in the cut file
    /// (the same refusal [`crate::rank::build_view`] makes) or when nothing
    /// could be run; the caller then shows nothing rather than a guess.
    fn visible_run(
        &self,
        rel_path: &str,
        text: &str,
        hidden: &str,
        what: VisibleRun,
    ) -> Result<(crate::feedback::Feedback, Option<Vec<String>>), String> {
        use crate::feedback::Feedback;
        let cut = crate::rank::without_check(text, hidden);
        if crate::rank::mentions(&cut, hidden) {
            return Err(
                "the hidden check's name appears outside its own definition, so the \
                 file cannot be run without it"
                    .into(),
            );
        }
        let rel = Path::new(rel_path);
        let name = rel
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .ok_or_else(|| format!("{rel_path} names no file"))?;
        let copy_name = format!(".cortex-visible-{}-{name}", std::process::id());
        let rel_copy = rel.with_file_name(&copy_name);
        let rel_copy_str = rel_copy.to_string_lossy().into_owned();
        let abs = self.workspace.join(&rel_copy);
        std::fs::write(&abs, &cut).map_err(|e| format!("cannot write a visible copy: {e}"))?;
        let abs_str = abs.to_string_lossy().into_owned();
        // Every string shown passes here: the copy's names become the real
        // ones, and — belt and braces, since the hidden check is not in the
        // file that ran — a string that names it anyway is dropped with the
        // item that carries it rather than shown.
        let clean = |s: &str| -> Option<String> {
            let s = s.replace(&abs_str, rel_path).replace(&copy_name, &name);
            (!crate::rank::mentions(&s, hidden)).then_some(s)
        };
        // The names of the visible checks that failed, for a test run only.
        let mut failing: Option<Vec<String>> = None;
        let ran = match what {
            VisibleRun::Tests => match self.check_executor.run_checks(&CheckRequest {
                workspace: &self.workspace,
                rel_path: &rel_copy_str,
                filter: None,
            }) {
                Ok(r) => {
                    failing = Some(r.failed.clone());
                    let failed: Vec<(String, Option<String>)> = r
                        .failed
                        .iter()
                        .map(|n| {
                            let m = r
                                .messages
                                .iter()
                                .find(|(m, _)| m == n)
                                .map(|(_, m)| m.clone());
                            (n.clone(), m)
                        })
                        .collect();
                    Ok(Feedback::from_tests(
                        &failed,
                        r.passed.len(),
                        &r.output,
                        &clean,
                    ))
                }
                // R2: the VISIBLE run stopping at a bound is the attempt's
                // own visible fact, and the generator is told it.
                Err(e) if bound_exceeded(&e).is_some() => Ok(Feedback::timed_out(&format!(
                    "the visible tests did not finish: {e}"
                ))),
                Err(e) => Err(format!("the visible tests could not be run: {e}")),
            },
            VisibleRun::Compile | VisibleRun::Diagnostics => self
                .run_axon_check(&abs, Some(&self.workspace))
                .map(|(text, _)| {
                    Feedback::from_check_output(
                        &text,
                        matches!(what, VisibleRun::Diagnostics),
                        &clean,
                    )
                })
                .map_err(|e| format!("the checker could not be run: {e}")),
        };
        let _ = std::fs::remove_file(&abs);
        Ok((ran?, failing))
    }

    /// What the generator for `symbol` may see: the ranker's view of the file
    /// as the candidate found it (hidden check cut out, the visible failing
    /// checks, the target's signature) and what the visible checks report on
    /// it — or the reason none of it can be shown.
    fn generator_context(
        &self,
        rel_path: &str,
        symbol: &str,
        hidden: &str,
        base: Option<&str>,
        failing: Option<&[String]>,
    ) -> crate::generate::GenContext {
        use crate::generate::GenContext;
        let Some(src) = base else {
            return GenContext::BodyOnly {
                reason: format!("{rel_path} could not be read when the candidate started"),
            };
        };
        let Some(failing) = failing else {
            return GenContext::BodyOnly {
                reason: "the failing checks could not be established when the candidate \
                         started"
                    .into(),
            };
        };
        let visible: Vec<String> = failing.iter().filter(|n| *n != hidden).cloned().collect();
        let eligible = [symbol.to_string()];
        match crate::rank::build_view(&crate::rank::RankInput {
            file: rel_path,
            source: src,
            hidden_check: hidden,
            failing: &visible,
            eligible: &eligible,
        }) {
            Ok(view) => GenContext::View {
                view,
                visible_now: self
                    .visible_run(rel_path, src, hidden, VisibleRun::Tests)
                    .ok()
                    .map(|(text, _)| text),
            },
            Err(reason) => GenContext::BodyOnly { reason },
        }
    }

    /// Side-effect-free twin of [`Runner::authorize_action`], for a caller that
    /// must decide without recording an episode event.
    pub fn authorize_action_dry(
        &self,
        action: &CortexAction,
        grant: Option<&EditGrant>,
        principal: &str,
        current: &WorkspaceSnapshot,
    ) -> Result<(), Refusal> {
        self.check_typed_authority(action, grant, principal, current)
    }

    fn check_typed_authority(
        &self,
        action: &CortexAction,
        grant: Option<&EditGrant>,
        principal: &str,
        current: &WorkspaceSnapshot,
    ) -> Result<(), Refusal> {
        // No catalog membership test: the type IS the catalog. An action
        // outside it cannot reach this function.
        if !action.requires_write_authority() {
            return Ok(());
        }
        // `Some` for exactly the variants that write. A non-writing action
        // returning a path here would mean the type and the authority rule
        // disagree, which is worth failing loudly rather than defaulting.
        let target_path = action.write_target().ok_or_else(|| {
            Refusal::NotInCatalog(format!(
                "{} requires write authority but names no write target",
                action.name()
            ))
        })?;
        let g = grant.ok_or_else(|| Refusal::UnknownGrant("<none>".into()))?;
        if g.principal != principal {
            return Err(Refusal::WrongPrincipal {
                expected: g.principal.clone(),
                got: principal.to_string(),
            });
        }
        if g.snapshot_id != current.snapshot_id {
            return Err(Refusal::StaleSnapshot {
                expected: g.snapshot_id.clone(),
                got: current.snapshot_id.clone(),
            });
        }
        if target_path.split('/').any(|s| s == "..") {
            return Err(Refusal::PathTraversal(target_path.to_string()));
        }
        let normalized = target_path.trim_start_matches("./");
        if POLICY_FILES.iter().any(|p| target_path.ends_with(p))
            || POLICY_PREFIXES.iter().any(|p| normalized.starts_with(p))
            || POLICY_PATHS.contains(&normalized)
        {
            return Err(Refusal::PolicyFile(target_path.to_string()));
        }
        let covered = g.write_prefixes.iter().any(|p| {
            p == "*"
                || (!p.is_empty()
                    && (target_path == p
                        || target_path.starts_with(&format!("{}/", p.trim_end_matches('/')))))
        });
        if !covered {
            return Err(Refusal::PathOutsideGrant(target_path.to_string()));
        }
        Ok(())
    }

    /// Independent verification, out-of-band from whatever proposed the patch.
    ///
    /// `G03-done`: a DONE claim cannot close the task while hidden checks fail,
    /// "regardless of confidence". `claimed_done` is deliberately an input that
    /// is IGNORED for the verdict — it is recorded so a reviewer can see that a
    /// confident wrong claim was made and overruled.
    pub fn verify(&mut self, claimed_done: bool, hidden_check: &str, rel_path: &str) -> bool {
        self.verify_run(claimed_done, hidden_check, rel_path, None)
            .0
    }

    /// [`Runner::verify`], and the process bound that stopped the hidden
    /// check's run when one did (R2: the candidate is then rejected, never
    /// the run blocked). `graded_sha256` (R5) is the digest of the file read
    /// before this check spawns; it is recorded on the event only when that
    /// event passes, the one a verified run is decided by.
    fn verify_run(
        &mut self,
        claimed_done: bool,
        hidden_check: &str,
        rel_path: &str,
        graded_sha256: Option<String>,
    ) -> (bool, Option<Bound>) {
        // THE VERDICT COMES FROM THE NAMED TEST, and from nothing else.
        //
        // This used to read the exit status of `axon test --filter NAME` over
        // the whole substring-matched set, which asks a different question
        // from the pre-flight that decides whether the check can witness
        // anything at all. The two disagreed exactly when `--check` named
        // something that is not itself a test: the pre-flight saw no verdict
        // for that name and let the run proceed, and this saw a filtered set
        // in which everything passed and returned true.
        //
        // Measured: `--check helper` on a file with a passing `t_helper_ok`
        // and a failing `t_main` exited 0 with `verified_done after 1 step`,
        // having written zero bytes, with the defect untouched. That is the
        // false success this whole module exists to prevent, reached through
        // the one function that can grant it.
        let (failed, passed, _) = match self.run_tests_json(rel_path, Some(hidden_check)) {
            Ok(v) => v,
            Err(e) => {
                self.episode.push(EpisodeEvent::Verified {
                    passed: false,
                    detail: format!("hidden check `{hidden_check}` DID NOT RUN: {e}"),
                    graded_sha256: None,
                });
                return (false, bound_exceeded(&e));
            }
        };
        let ran = failed.len() + passed.len();
        let ok = claimed_done && verdict_for(hidden_check, &failed, &passed);
        self.episode.push(EpisodeEvent::Verified {
            passed: ok,
            // `claim_done=` is carried in every case so a reviewer can see a
            // claim WAS made and overruled, rather than inferring it from the
            // verdict. Confidence is evidence of nothing; that confidence was
            // asserted is still worth recording.
            detail: format!(
                "claim_done={claimed_done}: {}",
                if !claimed_done {
                    "no completion was claimed".to_string()
                } else if ok {
                    format!("hidden check `{hidden_check}` passed ({ran} test(s) ran)")
                } else if !passed.iter().any(|n| n == hidden_check)
                    && !failed.iter().any(|n| n == hidden_check)
                {
                    // The SAME vocabulary a checker that could not be spawned
                    // gets, because it is the same fact: no verdict was
                    // produced for the name asked about. Calling it FAILED
                    // would send an operator to debug a repair when the check
                    // they named does not exist.
                    format!(
                        "hidden check `{hidden_check}` DID NOT RUN ({ran} test(s) \
                         matched the filter, none of them that name)"
                    )
                } else {
                    format!("hidden check `{hidden_check}` FAILED")
                }
            ),
            graded_sha256: if ok { graded_sha256 } else { None },
        });
        (ok, None)
    }

    /// R5: `Ok` when `rel_path` still holds the bytes whose digest is
    /// `graded`; otherwise why not. Called after the last acceptance spawn,
    /// so a file changed while it was being accepted (by the checks
    /// themselves or by anything else) is never the file that is accepted.
    fn still_graded(&self, rel_path: &str, graded: &str) -> Result<(), String> {
        match std::fs::read(self.workspace.join(rel_path)) {
            Ok(bytes) => {
                let now = sha256_hex(&bytes);
                if now == graded {
                    Ok(())
                } else {
                    Err(format!(
                        "{rel_path} was sha256 {graded} when graded, {now} after"
                    ))
                }
            }
            Err(e) => Err(format!(
                "{rel_path} could not be re-read after grading: {e}"
            )),
        }
    }

    /// Prepare a resettable COPY of a fixture directory. The original is never
    /// the thing edited.
    pub fn stage_copy(src: &Path, dst: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for e in std::fs::read_dir(src)? {
            let e = e?;
            if e.file_type()?.is_file() {
                std::fs::copy(e.path(), dst.join(e.file_name()))?;
            }
        }
        Ok(())
    }
}

/// Remove `.cortex-visible-*-<name>` files left beside `rel_path` by a run
/// that crashed before its own cleanup. Only that exact prefix and suffix, only
/// in that directory, and never a copy whose pid is this process or (on unix)
/// a process still alive: that copy belongs to a run in flight. A removal that
/// fails is reported once on stderr and the run goes on: a stale copy is never
/// read by the episode.
fn sweep_stale_visible_copies(workspace: &Path, rel_path: &str) {
    let abs = workspace.join(rel_path);
    let (Some(dir), Some(name)) = (abs.parent(), abs.file_name()) else {
        return;
    };
    let suffix = format!("-{}", name.to_string_lossy());
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let mut failed = None;
    for entry in entries.flatten() {
        let n = entry.file_name().to_string_lossy().into_owned();
        let Some(mid) = n
            .strip_prefix(".cortex-visible-")
            .and_then(|r| r.strip_suffix(suffix.as_str()))
        else {
            continue;
        };
        if mid.is_empty() || !mid.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        if visible_copy_owner_alive(mid) {
            continue;
        }
        if !entry.file_type().map(|t| t.is_file()).unwrap_or(false) {
            continue;
        }
        if let Err(e) = std::fs::remove_file(entry.path()) {
            failed.get_or_insert(format!("{}: {e}", entry.path().display()));
        }
    }
    if let Some(f) = failed {
        eprintln!("cortex: could not remove a stale visible copy ({f})");
    }
}

/// Whether the pid written in a visible copy's name is this process or one
/// still running. A pid that does not fit a `pid_t` names no process.
fn visible_copy_owner_alive(pid: &str) -> bool {
    let Ok(pid) = pid.parse::<u32>() else {
        return false;
    };
    if pid == std::process::id() {
        return true;
    }
    #[cfg(unix)]
    {
        let Ok(pid) = libc::pid_t::try_from(pid) else {
            return false;
        };
        // Signal 0 checks existence only. EPERM: alive, owned by someone else.
        // SAFETY: kill with signal 0 sends nothing.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        alive || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
    }
    #[cfg(not(unix))]
    {
        false
    }
}

/// Put a file back to the bytes a candidate started from.
///
/// Extracted because the undo existed on ONE of the two paths that reject a
/// patch — the claim-refused one — and not on the check-driven path that a
/// file compiling with warnings actually takes. Patches accumulated exactly as
/// before on the majority of real files, while the commit message said
/// otherwise. One mechanism, both call sites.
fn restore_base(path: &std::path::Path, base: &Option<String>) -> std::io::Result<()> {
    match base {
        Some(bytes) => std::fs::write(path, bytes),
        // Unreadable at episode start. Nothing to restore TO, and inventing
        // something would be worse than leaving it alone; the patch paths fail
        // earlier in this case anyway.
        None => Ok(()),
    }
}

/// Record a rejected attempt, once.
///
/// Deduplicated on the body, because the count of rejections is a coordinate
/// of the stuck-check: a generator that proposes the same thing twice must not
/// look like progress. Recorded once, the pair repeats and the episode stops.
/// The feedback plays no part in it: the same body with different visible
/// output is still the same idea.
fn push_rejected(
    rejected: &mut Vec<crate::generate::RejectedAttempt>,
    body: String,
    reason: crate::generate::RejectionReason,
    feedback: Option<crate::feedback::Feedback>,
) {
    if !rejected.iter().any(|r| r.body == body) {
        rejected.push(crate::generate::RejectedAttempt {
            body,
            reason,
            feedback,
        });
    }
}

/// What a visible run is asked.
#[derive(Clone, Copy)]
enum VisibleRun {
    /// `axon test` on the file: verdicts, failure messages, printed output.
    Tests,
    /// `axon check` on the file: the first error diagnostics.
    Compile,
    /// `axon check` on the file: the first diagnostics of ANY severity, for an
    /// attempt rejected because the checker did not exit 0.
    Diagnostics,
}

/// The verdict for exactly the check that was named.
///
/// `--filter` is a SUBSTRING match, so asking for `hidden_completion` also
/// runs `hidden_completion_edge`. Reading the verdict as "nothing in the
/// filtered set failed" therefore graded the repair against a test nobody
/// named — and in the other direction, a sibling failing for its own reasons
/// would veto a correct repair.
///
/// Absent from both lists is `false`: a check that did not run did not pass.
/// Same rule as the zero-match case, for the same reason — an absent verdict
/// must never be reported as a favourable one.
fn verdict_for(name: &str, failed: &[String], passed: &[String]) -> bool {
    passed.iter().any(|n| n == name) && !failed.iter().any(|n| n == name)
}

/// Whether the observation established that the file compiles — and `None`
/// when the checker could not say.
///
/// An `Option`, not a `bool`: `Unknown` is neither a yes nor a no, and folding
/// it into either would make "the checker did not run" indistinguishable from
/// an answer. The rollback rule needs both ends of that distinction, because
/// the two imply opposite actions.
fn observed_compiles(obs: &Observation) -> Option<bool> {
    obs.facts
        .iter()
        .find(|(k, _)| k == "compiles")
        .and_then(|(_, v)| match v {
            Observed::Known { value } => Some(value == "true"),
            Observed::Unknown { .. } => None,
        })
}

// ── B264: the registered-check seam ─────────────────────────────────────────

/// What a check dispatch asks for. There is deliberately NO executable and NO
/// argv here: the caller names a file and (optionally) a test filter, which are
/// DATA passed as single argv values. Which program runs, and with which flags,
/// is fixed by the [`CheckExecutor`] and its registration — a model-produced or
/// caller-produced string can never become the program that adjudicates.
#[derive(Debug, Clone, Copy)]
pub struct CheckRequest<'a> {
    pub workspace: &'a Path,
    pub rel_path: &'a str,
    pub filter: Option<&'a str>,
}

/// What a check dispatch produced. `total` is the run's OWN summary count, so
/// "the filter matched nothing" (`total == 0`, or no entry for a name) stays
/// distinguishable from "everything passed".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// `(test name, completion token)` for each passing test whose body
    /// COMPLETED, as `axon test --completion-key-stdin` reports it. A caller
    /// that supplied its own key checks the token with [`completion_token`];
    /// otherwise the executor already has (absent token ⇒ not in `passed`).
    pub completion: Vec<(String, String)>,
    pub failed: Vec<String>,
    pub passed: Vec<String>,
    pub total: usize,
    /// The check process's own exit status, when a process was observed.
    /// Never used to infer a verdict.
    pub exit_code: Option<i32>,
    /// `(test name, failure message)` for failed tests that carried one.
    /// Never used to infer a verdict.
    pub messages: Vec<(String, String)>,
    /// Every line of the run's output that is not a JSON record: what the
    /// test bodies printed, and the interpreter's own diagnostics. NOT
    /// attributed to a test — the interpreter runs every test before printing
    /// any result — so it is only ever shown for a run that contains no
    /// hidden check. Never used to infer a verdict.
    pub output: Vec<String>,
}

/// The verdict for ONE named check. Three-valued on purpose: a check with no
/// verdict (it matched nothing, or it did not run) is never `Passed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckVerdict {
    Passed,
    Failed,
    /// No test by that exact name produced a verdict.
    NotRun,
}

impl CheckReport {
    /// The verdict for exactly `name`. Mandatory-check semantics: absent is
    /// `NotRun`, and a name that both passed and failed (a substring filter can
    /// select duplicates) is `Failed`.
    pub fn verdict(&self, name: &str) -> CheckVerdict {
        if self.failed.iter().any(|n| n == name) {
            CheckVerdict::Failed
        } else if self.passed.iter().any(|n| n == name) {
            CheckVerdict::Passed
        } else {
            CheckVerdict::NotRun
        }
    }
}

/// The seam through which the Runner runs a registered check (B264).
///
/// An `Err` means NO VERDICT WAS PRODUCED — refused before spawning, could not
/// spawn, or the run did not complete. It is never an empty-but-successful
/// report; callers already treat `Err` as "did not run" / exit 22.
pub trait CheckExecutor: Send {
    /// A stable, human-readable identity (profile + pinned digest) for evidence.
    fn id(&self) -> String;
    fn run_checks(&self, req: &CheckRequest<'_>) -> std::io::Result<CheckReport>;
}

/// An executable pinned by canonical path AND sha256 at registration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredExecutable {
    /// Canonical absolute path (symlinks resolved at registration, so swapping
    /// a symlink afterwards cannot redirect the spawn).
    pub path: PathBuf,
    /// Lowercase hex sha256 of the file's bytes at registration.
    pub sha256: String,
}

/// Why a registered check refused to run. Every refusal happens BEFORE any
/// process is spawned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckRefusal {
    /// The registry has no entry for the requested executor id.
    Unregistered { id: String },
    /// The executable could not be resolved or read.
    Unresolvable { path: PathBuf, reason: String },
    /// The bytes at the registered path are not the bytes that were registered.
    DigestChanged {
        path: PathBuf,
        registered: String,
        found: String,
    },
}

impl std::fmt::Display for CheckRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckRefusal::Unregistered { id } => {
                write!(
                    f,
                    "check executor `{id}` is not registered; refusing to run"
                )
            }
            CheckRefusal::Unresolvable { path, reason } => write!(
                f,
                "check executable {} cannot be resolved: {reason}",
                path.display()
            ),
            CheckRefusal::DigestChanged {
                path,
                registered,
                found,
            } => write!(
                f,
                "check executable {} changed since registration (registered sha256 \
                 {registered}, now {found}); refusing to run it",
                path.display()
            ),
        }
    }
}

impl From<CheckRefusal> for std::io::Error {
    fn from(r: CheckRefusal) -> Self {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, r.to_string())
    }
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// Resolve `exe` the way `Command::new` would (a bare name searches PATH),
/// then canonicalize. Done ONCE, at registration.
fn resolve_executable(exe: &Path) -> Result<PathBuf, CheckRefusal> {
    let unresolvable = |reason: String| CheckRefusal::Unresolvable {
        path: exe.to_path_buf(),
        reason,
    };
    let candidate = if exe.components().count() == 1 && !exe.as_os_str().is_empty() {
        let path_var = std::env::var_os("PATH").unwrap_or_default();
        std::env::split_paths(&path_var)
            .map(|d| d.join(exe))
            .find(|p| p.is_file())
            .ok_or_else(|| unresolvable("not found on PATH".into()))?
    } else {
        exe.to_path_buf()
    };
    let canon = candidate
        .canonicalize()
        .map_err(|e| unresolvable(e.to_string()))?;
    if !canon.is_file() {
        return Err(unresolvable("not a regular file".into()));
    }
    Ok(canon)
}

/// Cheap identity of a file's current bytes: (dev, ino, size, mtime, ctime).
/// `ctime` cannot be set by an unprivileged process and changes on every
/// write, chmod or rename, so an unchanged stamp means the pinned sha256 still
/// describes the file; any change forces a full re-hash. (Measured: a
/// debug-build sha256 of the 79 MB debug interpreter takes ~1.1 s, which per
/// check dispatch was not acceptable.)
type Stamp = Option<[i128; 7]>;

#[cfg(unix)]
fn file_stamp(p: &Path) -> Stamp {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::metadata(p).ok()?;
    Some([
        m.dev() as i128,
        m.ino() as i128,
        m.size() as i128,
        m.mtime() as i128,
        m.mtime_nsec() as i128,
        m.ctime() as i128,
        m.ctime_nsec() as i128,
    ])
}
#[cfg(not(unix))]
fn file_stamp(_p: &Path) -> Stamp {
    None // no fast path: always re-hash
}

/// A trusted registry of check executables: id → (canonical path, sha256).
///
/// Entries come from two places only: an operator-supplied registry FILE whose
/// digests are stated up front (and verified on load), or an explicit
/// `register_pinned` call that pins whatever the path holds at that moment.
/// Neither takes its program from a check request.
#[derive(Debug, Clone, Default)]
pub struct CheckRegistry {
    entries: Vec<(String, RegisteredExecutable)>,
    checks: Vec<RegisteredCheck>,
}

/// Whether a registered check's SOURCE may be shown to the subject (B264).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckVisibility {
    /// The subject may see the check's bytes (e.g. to reproduce a failure).
    Visible,
    /// The subject never sees the check's bytes: the Fabric materializes
    /// them into a SEPARATE read-only WorkspaceVersion that is never part of
    /// the candidate's workspace.
    Hidden,
}

/// A registered check suite: a directory pinned by its WorkspaceVersion
/// reference, and the `.ax` entry file in it the interpreter runs. The
/// candidate is reached through `AXON_PATH` (`mod <name>` in the entry), so
/// the check's bytes and the candidate's never share a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegisteredCheck {
    pub id: String,
    pub visibility: CheckVisibility,
    pub root: PathBuf,
    pub entry: String,
    /// `acf1:` WorkspaceVersion of `root` the operator pinned. The Fabric
    /// imports `root` at dispatch and refuses the check if it differs.
    pub workspace_version_ref: String,
}

/// A registered check suite's id, as `check:<id>` and
/// `check-suite:<id>@<version>#<entry>` carry it: non-empty, and none of the
/// separators those forms use (`@`, `#`, `:`), no `/`, no whitespace or control
/// character. ONE parser for every side (Fabric's registry, the loop's store
/// pins and `check_pins`), so no two sides read one reference as two suites
/// (C9 round 2, PSV-5: the loop's `split('@')` read `acceptance@x@...` as the
/// suite `acceptance` while the manifest named `acceptance@x`).
pub fn check_suite_id(id: &str) -> Result<&str, String> {
    if id.is_empty()
        || id
            .chars()
            .any(|c| matches!(c, '@' | '#' | ':' | '/') || c.is_whitespace() || c.is_control())
    {
        return Err(format!(
            "check suite id {id:?} is not an id: it must be non-empty, with no `@`, `#`, `:`, `/`, \
             whitespace or control character"
        ));
    }
    Ok(id)
}

/// `check-suite:<id>@<version>#<entry>` as `(id, version, entry)`, the id by
/// [`check_suite_id`] and a version with no `@` or `#`, so the split is the
/// only one there is.
pub fn parse_check_suite_ref(r: &str) -> Result<(&str, &str, &str), String> {
    let bad = |why: &str| format!("check suite reference {r:?} {why}");
    let rest = r
        .strip_prefix("check-suite:")
        .ok_or_else(|| bad("is not check-suite:<id>@<version>#<entry>"))?;
    let (id, rest) = rest
        .split_once('@')
        .ok_or_else(|| bad("names no version"))?;
    let (version, entry) = rest.split_once('#').ok_or_else(|| bad("names no entry"))?;
    check_suite_id(id).map_err(|e| bad(&e))?;
    if version.is_empty() || version.contains(['@', '#']) || entry.is_empty() {
        return Err(bad(
            "is ambiguous: its version holds a separator, or its version or entry is empty",
        ));
    }
    Ok((id, version, entry))
}

/// The one way a suite reference is written (Fabric's receipt and manifest,
/// the loop's join), the inverse of [`parse_check_suite_ref`]. It writes only
/// a reference that reads back as exactly `(id, version, entry)`: a version
/// holding `#` or `@` (or a bad id, or an empty part) formats into a string
/// the parser reads as ANOTHER suite, so it is refused here rather than
/// written (C9 round 3, PSV-5; A81). The entry is the tail after the version's
/// `#`, so with the id and version separator-free every reference has one
/// reading.
pub fn check_suite_ref(id: &str, version: &str, entry: &str) -> Result<String, String> {
    check_suite_id(id)?;
    let r = format!("check-suite:{id}@{version}#{entry}");
    if parse_check_suite_ref(&r)? != (id, version, entry) {
        return Err(format!(
            "check suite (id {id:?}, version {version:?}, entry {entry:?}) has no reference of \
             its own: {r:?} reads as another suite"
        ));
    }
    Ok(r)
}

/// The id the local interpreter executor is registered under.
pub const LOCAL_AXON_TEST_ID: &str = "axon-test-local";

impl CheckRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolve `exe`, hash it, and pin it under `id` (trust on registration).
    pub fn register_pinned(&mut self, id: &str, exe: &Path) -> Result<(), CheckRefusal> {
        let path = resolve_executable(exe)?;
        let bytes = std::fs::read(&path).map_err(|e| CheckRefusal::Unresolvable {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        self.insert(
            id,
            RegisteredExecutable {
                path,
                sha256: sha256_hex(&bytes),
            },
        );
        Ok(())
    }

    /// Register `exe` under `id` only if its bytes hash to `expected_sha256`
    /// (the operator stated the digest out of band).
    pub fn register_expected(
        &mut self,
        id: &str,
        exe: &Path,
        expected_sha256: &str,
    ) -> Result<(), CheckRefusal> {
        let path = resolve_executable(exe)?;
        let bytes = std::fs::read(&path).map_err(|e| CheckRefusal::Unresolvable {
            path: path.clone(),
            reason: e.to_string(),
        })?;
        let found = sha256_hex(&bytes);
        let want = expected_sha256.trim().to_ascii_lowercase();
        if found != want {
            return Err(CheckRefusal::DigestChanged {
                path,
                registered: want,
                found,
            });
        }
        self.insert(
            id,
            RegisteredExecutable {
                path,
                sha256: found,
            },
        );
        Ok(())
    }

    /// A registered check suite by id.
    pub fn check(&self, id: &str) -> Option<&RegisteredCheck> {
        self.checks.iter().find(|c| c.id == id)
    }

    /// The checks whose source MAY be shown to the subject. A context
    /// builder uses this list; a hidden check is never in it.
    pub fn subject_visible_checks(&self) -> impl Iterator<Item = &RegisteredCheck> {
        self.checks
            .iter()
            .filter(|c| c.visibility == CheckVisibility::Visible)
    }

    /// Register a check suite (operator input). Its id must parse as one
    /// ([`check_suite_id`]): every registration, from a file or a library
    /// caller, flows through here, so no suite Fabric runs can carry an id the
    /// loop would read differently.
    pub fn register_check(&mut self, c: RegisteredCheck) -> Result<(), String> {
        // The id (by [`check_suite_id`]) AND the whole reference it will be
        // written as: it must read back as this suite. A version holding `#`
        // or `@`, registered through the library (the file loader also
        // requires acf1 hex), would not (C9 round 3, PSV-5; A81).
        check_suite_ref(&c.id, &c.workspace_version_ref, &c.entry)?;
        self.checks.retain(|k| k.id != c.id);
        self.checks.push(c);
        Ok(())
    }

    /// Load `{"schema":"cortex-check-registry/1","executors":[{"id","path","sha256"}]}`,
    /// plus an optional `"checks":[{"id","visibility":"visible"|"hidden","root","entry","workspace_version_ref"}]`.
    /// Every entry is verified against its stated digest; one bad entry fails
    /// the whole load (a partially trusted registry is not a registry).
    pub fn load(file: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(file)
            .map_err(|e| format!("cannot read check registry {}: {e}", file.display()))?;
        let v: serde_json::Value = serde_json::from_str(&text)
            .map_err(|e| format!("check registry {} is not JSON: {e}", file.display()))?;
        if v.get("schema").and_then(|s| s.as_str()) != Some("cortex-check-registry/1") {
            return Err(format!(
                "check registry {} must have schema `cortex-check-registry/1`",
                file.display()
            ));
        }
        let base = file.parent().unwrap_or(Path::new("."));
        let mut reg = CheckRegistry::new();
        for e in v
            .get("executors")
            .and_then(|x| x.as_array())
            .ok_or("check registry has no `executors` array")?
        {
            let field = |k: &str| {
                e.get(k)
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| format!("check registry entry missing `{k}`"))
            };
            let (id, path, sha) = (field("id")?, field("path")?, field("sha256")?);
            let p = Path::new(path);
            let p = if p.is_relative() && p.components().count() > 1 {
                base.join(p)
            } else {
                p.to_path_buf()
            };
            reg.register_expected(id, &p, sha)
                .map_err(|r| r.to_string())?;
        }
        for c in v
            .get("checks")
            .map(|x| {
                x.as_array()
                    .ok_or("check registry `checks` is not an array")
            })
            .transpose()?
            .into_iter()
            .flatten()
        {
            let field = |k: &str| {
                c.get(k)
                    .and_then(|x| x.as_str())
                    .ok_or_else(|| format!("check registry check missing `{k}`"))
            };
            let visibility = match field("visibility")? {
                "visible" => CheckVisibility::Visible,
                "hidden" => CheckVisibility::Hidden,
                other => {
                    return Err(format!(
                        "check visibility must be `visible` or `hidden`, not `{other}`"
                    ))
                }
            };
            let root = Path::new(field("root")?);
            let root = if root.is_relative() {
                base.join(root)
            } else {
                root.to_path_buf()
            };
            let r = field("workspace_version_ref")?;
            let hex = r.strip_prefix("acf1:").unwrap_or("");
            if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
                return Err(format!(
                    "check workspace_version_ref `{r}` is not acf1:<hex>"
                ));
            }
            let id = field("id")?;
            if reg.check(id).is_some() {
                return Err(format!("check id `{id}` registered twice"));
            }
            reg.register_check(RegisteredCheck {
                id: id.to_string(),
                visibility,
                root,
                entry: field("entry")?.to_string(),
                workspace_version_ref: r.to_string(),
            })?;
        }
        Ok(reg)
    }

    fn insert(&mut self, id: &str, e: RegisteredExecutable) {
        self.entries.retain(|(k, _)| k != id);
        self.entries.push((id.to_string(), e));
    }

    pub fn get(&self, id: &str) -> Result<&RegisteredExecutable, CheckRefusal> {
        self.entries
            .iter()
            .find(|(k, _)| k == id)
            .map(|(_, e)| e)
            .ok_or_else(|| CheckRefusal::Unregistered { id: id.to_string() })
    }
}

enum Pin {
    /// Registered and pinned; the stamp is the file identity the digest was
    /// last verified at.
    Pinned(RegisteredExecutable, Stamp),
    /// `Runner::new` compatibility: resolve + pin on first dispatch.
    Deferred(PathBuf),
    /// Registration failed; every dispatch refuses with this reason.
    Failed(CheckRefusal),
}

/// The one real [`CheckExecutor`]: the local tree-walking interpreter,
/// `<registered axon> test <file> --json [--filter F]`.
///
/// Profile: `process_scoped/local-interpreter`. It runs as a child process of
/// the Runner with the Runner's own authority — it is NOT a sandbox and must
/// never be reported as microVM-isolated.
pub struct LocalInterpreterExecutor {
    pin: std::sync::Mutex<Pin>,
    limits: ExecLimits,
}

/// Optional process limits for [`LocalInterpreterExecutor`]. All default to
/// off, which is the pre-existing behaviour.
#[derive(Debug, Clone, Default)]
struct ExecLimits {
    wall: Option<std::time::Duration>,
    max_output: Option<usize>,
    /// Value for `AXON_ALLOWED_EFFECTS` (the interpreter's own effect
    /// ceiling). `None` leaves it unset (no ceiling).
    effect_ceiling: Option<String>,
    /// Extra environment for the check process (set, never inherited
    /// selectively): the Fabric's per-trial cache dirs and module path.
    env: Vec<(String, String)>,
    /// Start the check process from an EMPTY environment, so only what is
    /// set explicitly here reaches it.
    clean_env: bool,
    /// A caller-supplied completion secret, written to the check's stdin
    /// (never its environment, which `/proc/self/environ` would expose). The
    /// caller then adjudicates the tokens itself (the Fabric does), so the
    /// report is returned RAW. `None` — the default — means the executor
    /// draws a fresh key for every dispatch and enforces completion itself:
    /// see [`LocalInterpreterExecutor::run_checks`].
    completion_key: Option<Vec<u8>>,
    /// Module directories the check runs SEALED (`axon test --seal`): code
    /// from them cannot reach any name the rest of the program defines.
    sealed: Vec<std::path::PathBuf>,
}

impl LocalInterpreterExecutor {
    pub const PROFILE: &'static str = "process_scoped/local-interpreter";

    /// Use the registry's `axon-test-local` entry. An absent entry is a
    /// refusal NOW, not at first dispatch.
    pub fn from_registry(reg: &CheckRegistry) -> Result<Self, CheckRefusal> {
        let e = reg.get(LOCAL_AXON_TEST_ID)?.clone();
        let stamp = file_stamp(&e.path);
        Ok(LocalInterpreterExecutor {
            pin: std::sync::Mutex::new(Pin::Pinned(e, stamp)),
            limits: ExecLimits::default(),
        })
    }

    /// Kill the check process at this wall-clock deadline; the dispatch then
    /// returns an `ErrorKind::TimedOut` error (no verdict).
    pub fn with_timeout(mut self, wall: std::time::Duration) -> Self {
        self.limits.wall = Some(wall);
        self
    }

    /// Retain at most this many bytes of each output stream. A stream that
    /// exceeded it cannot be parsed reliably, so a truncated run yields no
    /// verdict rather than a partial one.
    pub fn with_max_output(mut self, bytes: usize) -> Self {
        self.limits.max_output = Some(bytes.max(2));
        self
    }

    /// Run under `AXON_ALLOWED_EFFECTS=<ceiling>` — the interpreter's effect
    /// ceiling (process_scoped enforcement, not an OS boundary).
    pub fn with_effect_ceiling(mut self, ceiling: impl Into<String>) -> Self {
        self.limits.effect_ceiling = Some(ceiling.into());
        self
    }

    /// Run the check from an EMPTY environment: nothing of the launcher's
    /// reaches it but the effect ceiling and what [`Self::with_env`] sets.
    /// Interpreter behaviour is steered by ambient `AXON_*` variables
    /// (`AXON_STRICT` alone turns a passing check into no verdict), and a
    /// verdict someone vouches for must not depend on who launched it.
    pub fn with_clean_env(mut self) -> Self {
        self.limits.clean_env = true;
        self
    }

    /// Ask the interpreter for affirmative completion evidence under a key
    /// the CALLER holds: it reads `key` from stdin before any program code
    /// runs and tags each test whose body COMPLETED with
    /// [`completion_token`]. The report is then returned raw — a pass without
    /// a token stays in `passed` — because the caller has said it adjudicates
    /// the tokens. Without this, the executor keys every dispatch itself and
    /// enforces completion (the default, fail-closed).
    pub fn with_completion_key(mut self, key: Vec<u8>) -> Self {
        self.limits.completion_key = Some(key);
        self
    }

    /// Run the check with every module under `dir` SEALED (`axon test
    /// --seal`): the candidate under test may use builtins and its own names,
    /// never a name the operator's suite defines (Protected Check Isolation).
    pub fn with_sealed_dir(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.limits.sealed.push(dir.into());
        self
    }

    /// Set `key=value` in the check process's environment. Used by the
    /// Fabric to give each trial its own fresh `HOME` / `XDG_CACHE_HOME` /
    /// `CARGO_TARGET_DIR`, and a hidden check its `AXON_PATH`.
    pub fn with_env(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.limits.env.push((key.into(), value.into()));
        self
    }

    /// Re-verify the registered executable (path + sha256) WITHOUT spawning
    /// anything. A caller that journals a launch record calls this
    /// immediately before, so a changed binary is refused before the launch
    /// is recorded.
    pub fn verify(&self) -> Result<PathBuf, CheckRefusal> {
        self.verified_path()
    }

    /// Compatibility path for [`Runner::new`]: resolve and pin on first use.
    pub fn pin_on_first_use(exe: PathBuf) -> Self {
        LocalInterpreterExecutor {
            pin: std::sync::Mutex::new(Pin::Deferred(exe)),
            limits: ExecLimits::default(),
        }
    }

    /// Resolve the pin (if deferred) and verify the file is still the
    /// registered bytes. Returns the path to spawn. Spawns NOTHING.
    fn verified_path(&self) -> Result<PathBuf, CheckRefusal> {
        let mut pin = self.pin.lock().unwrap_or_else(|p| p.into_inner());
        if let Pin::Deferred(exe) = &*pin {
            let mut reg = CheckRegistry::new();
            *pin = match reg.register_pinned(LOCAL_AXON_TEST_ID, exe) {
                Ok(()) => {
                    let e = reg.get(LOCAL_AXON_TEST_ID)?.clone();
                    let s = file_stamp(&e.path);
                    Pin::Pinned(e, s)
                }
                Err(r) => Pin::Failed(r),
            };
        }
        match &mut *pin {
            Pin::Failed(r) => Err(r.clone()),
            Pin::Deferred(_) => unreachable!("resolved above"),
            Pin::Pinned(e, stamp) => {
                let now = file_stamp(&e.path);
                if now.is_some() && now == *stamp {
                    return Ok(e.path.clone());
                }
                let bytes = std::fs::read(&e.path).map_err(|err| CheckRefusal::Unresolvable {
                    path: e.path.clone(),
                    reason: err.to_string(),
                })?;
                let found = sha256_hex(&bytes);
                if found != e.sha256 {
                    return Err(CheckRefusal::DigestChanged {
                        path: e.path.clone(),
                        registered: e.sha256.clone(),
                        found,
                    });
                }
                *stamp = now;
                Ok(e.path.clone())
            }
        }
    }
}

impl CheckExecutor for LocalInterpreterExecutor {
    fn id(&self) -> String {
        let pin = self.pin.lock().unwrap_or_else(|p| p.into_inner());
        match &*pin {
            Pin::Pinned(e, _) => {
                format!("{}:{}@sha256:{}", Self::PROFILE, e.path.display(), e.sha256)
            }
            Pin::Deferred(p) => format!("{}:{} (unpinned)", Self::PROFILE, p.display()),
            Pin::Failed(r) => format!("{}: refused ({r})", Self::PROFILE),
        }
    }

    /// Every dispatch carries a completion key. When the caller supplied none
    /// (every Cortex caller), a FRESH random key is drawn for this dispatch
    /// and enforced here: a test counts as passed only if it reported `ok`
    /// AND carries the token for its name under that key. An `ok` without
    /// it is moved to `failed`.
    ///
    /// Plain `axon test` reports a test that ended early — `exit(0)` from the
    /// code under test — as `ok`, by design (the program asked to stop). Read
    /// as a pass, a candidate body `exit(0)` "passed" every check it was
    /// adjudicated by and `cortex repair` exited 0 with the defect replaced
    /// by an exit. The token is issued only for a body that RETURNED, it is
    /// keyed per dispatch over stdin before program code runs, so the
    /// program can neither earn it by stopping nor forge it by printing a
    /// result line. Absent is never passed.
    fn run_checks(&self, req: &CheckRequest<'_>) -> std::io::Result<CheckReport> {
        // Refusal happens here, before any Command is built.
        let exe = self.verified_path()?;
        let (key, enforce) = match &self.limits.completion_key {
            Some(k) => (k.clone(), false),
            None => (fresh_completion_key()?, true),
        };
        let mut cmd = std::process::Command::new(&exe);
        // The check runs IN its own workspace. It used to inherit the
        // launcher's cwd, so a suite's relative runtime read (a fixture, an
        // expected value) resolved OUTSIDE the tree the verifier pinned and
        // digests — wherever the launcher happened to be — and a candidate
        // with a write grant planted the answer there (PCI, executed through
        // Fabric: a signed-off pass for a wrong candidate).
        cmd.current_dir(req.workspace)
            .arg("test")
            .arg(req.workspace.join(req.rel_path))
            .arg("--json");
        if let Some(f) = req.filter {
            cmd.arg("--filter").arg(f);
        }
        cmd.arg("--completion-key-stdin");
        for d in &self.limits.sealed {
            cmd.arg("--seal").arg(d);
        }
        if self.limits.clean_env {
            cmd.env_clear();
        }
        if let Some(c) = &self.limits.effect_ceiling {
            cmd.env("AXON_ALLOWED_EFFECTS", c);
        }
        for (k, v) in &self.limits.env {
            cmd.env(k, v);
        }
        let (text, code, truncated) = run_limited(&mut cmd, &self.limits, Some(&key))?;
        if truncated {
            return Err(BoundExceeded::output(self.limits.max_output.unwrap_or(0)));
        }
        let mut report = parse_axon_test_json(&text, req.rel_path)?;
        report.exit_code = code;
        if enforce {
            require_completion(&mut report, &key);
        }
        Ok(report)
    }
}

/// A fresh 32-byte completion key from the OS. No randomness, no verdict: a
/// predictable key is one a program could forge tokens under.
fn fresh_completion_key() -> std::io::Result<Vec<u8>> {
    use std::io::Read;
    let mut k = vec![0u8; 32];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut k))
        .map_err(|e| std::io::Error::other(format!("no randomness for the completion key: {e}")))?;
    Ok(k)
}

/// Keep in `passed` only the names that carry their completion token under
/// `key`; every other `ok` is moved to `failed`. Per NAME, so a forged
/// token-less `ok` line printed beside the real, tokened one changes
/// nothing, and a forged line alone never makes a pass.
fn require_completion(report: &mut CheckReport, key: &[u8]) {
    let completion = &report.completion;
    let (kept, unproven): (Vec<String>, Vec<String>) = std::mem::take(&mut report.passed)
        .into_iter()
        .partition(|name| {
            let want = completion_token(key, name);
            completion.iter().any(|(n, t)| n == name && *t == want)
        });
    report.passed = kept;
    report.failed.extend(unproven);
}

/// Which process bound stopped a spawn that therefore produced no verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bound {
    /// Killed (with its whole process group) at its wall-clock limit.
    Time,
    /// Wrote more than its capture bound, so what it said cannot be read.
    Output,
}

/// The error payload of a spawn stopped at a [`Bound`]. Carried inside the
/// `std::io::Error` the dispatch returns, so a caller can tell "the candidate
/// ran past a limit" (a rejected attempt) from "the checker could not run"
/// (an environment fault) with [`bound_exceeded`].
#[derive(Debug)]
pub struct BoundExceeded {
    pub bound: Bound,
    detail: String,
}

impl std::fmt::Display for BoundExceeded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.detail)
    }
}

impl std::error::Error for BoundExceeded {}

impl BoundExceeded {
    fn output(max: usize) -> std::io::Error {
        std::io::Error::other(BoundExceeded {
            bound: Bound::Output,
            detail: format!(
                "check output exceeded the {max}-byte capture bound; a truncated run \
                 yields no verdict"
            ),
        })
    }
}

/// The bound that stopped the spawn `e` reports, if a bound did.
pub fn bound_exceeded(e: &std::io::Error) -> Option<Bound> {
    e.get_ref()
        .and_then(|inner| inner.downcast_ref::<BoundExceeded>())
        .map(|b| b.bound)
}

/// Drain one pipe of a check on its own thread: keep at most `cap` bytes,
/// count the rest, and give up once `stop` is set, so a descendant holding
/// the pipe open cannot hold the caller (R2). Returns (kept, truncated?).
fn drain_bounded<R>(
    pipe: Option<R>,
    cap: Option<usize>,
    stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
) -> std::thread::JoinHandle<(Vec<u8>, bool)>
where
    R: std::io::Read + std::os::fd::AsRawFd + Send + 'static,
{
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut total = 0usize;
        let mut buf = [0u8; 8192];
        if let Some(mut r) = pipe {
            let fd = r.as_raw_fd();
            while axon_jobctl::wait_readable(fd, &stop) {
                match r.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        total += n;
                        let room = cap.map(|c| c.saturating_sub(kept.len())).unwrap_or(n);
                        kept.extend_from_slice(&buf[..n.min(room)]);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        (kept, cap.is_some_and(|c| total > c))
    })
}

/// `axon caps <file> --json [--base-dir D] [--fn NAME]` (R1) as a bounded job:
/// its own process group, `wall`, at most [`CHECKER_MAX_OUTPUT`] bytes per
/// stream, an EMPTY environment like the checks it fences. It executes no
/// workspace code. Returns (stdout+stderr, exit code); read it with
/// [`crate::fence::parse_caps`].
pub fn run_axon_caps(
    axon: &Path,
    file: &Path,
    base_dir: Option<&Path>,
    fn_name: Option<&str>,
    wall: std::time::Duration,
) -> std::io::Result<(String, Option<i32>)> {
    let mut cmd = std::process::Command::new(axon);
    cmd.arg("caps").arg(file).arg("--json");
    if let Some(d) = base_dir {
        cmd.arg("--base-dir").arg(d);
    }
    if let Some(f) = fn_name {
        cmd.arg("--fn").arg(f);
    }
    cmd.env_clear();
    let limits = ExecLimits {
        wall: Some(wall),
        max_output: Some(CHECKER_MAX_OUTPUT),
        ..ExecLimits::default()
    };
    let (text, code, truncated) = run_limited(&mut cmd, &limits, None)?;
    if truncated {
        return Err(BoundExceeded::output(CHECKER_MAX_OUTPUT));
    }
    Ok((text, code))
}

/// Spawn `cmd` as its own process-group JOB, drain both pipes (bounded, if
/// asked), enforce the wall-clock deadline (if asked). Returns
/// (stdout+stderr, exit code, truncated?). `completion_key`, when given, is
/// written to the child's stdin; otherwise stdin is `/dev/null`.
///
/// R2: a check runs candidate code, and candidate code can spawn. Killing
/// only the direct child at the deadline left a grandchild running, and the
/// unbounded pipe joins then waited for THAT to exit — one backgrounded
/// `sleep` holding the pipes stalled the whole loop. The job is a group
/// leader; at the deadline, and also after a normal exit (a check's
/// leftovers have no business running on), the whole group is killed with
/// `killpg` while the leader is still an unreaped zombie, so its group id
/// cannot have been reused. The drains poll a stop flag and are joined
/// within [`axon_jobctl::DRAIN_GRACE`].
fn run_limited(
    cmd: &mut std::process::Command,
    limits: &ExecLimits,
    completion_key: Option<&[u8]>,
) -> std::io::Result<(String, Option<i32>, bool)> {
    use std::process::Stdio;
    use std::sync::atomic::AtomicBool;
    use std::sync::Arc;
    // NO UNOWNED WORKER (v0.22 G13-r22-restart-matrix). A supervisor that dies
    // — SIGKILL, OOM, a host reboot of the process — used to leave its check
    // running with nobody to collect its verdict or bill it: measured, the
    // worker of a SIGKILLed `axon-fabric submit` survived until a test reaped it
    // by hand. The journal already records such an op as OutcomeUnknown with its
    // liability kept; the worker now dies with the supervisor, so "unknown" is
    // not also "still running". The getppid() check closes the race where the
    // parent dies between fork and prctl.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::process::CommandExt;
        let parent = unsafe { libc::getpid() };
        unsafe {
            cmd.pre_exec(move || {
                if libc::prctl(
                    libc::PR_SET_PDEATHSIG,
                    libc::SIGKILL as libc::c_ulong,
                    0,
                    0,
                    0,
                ) != 0
                {
                    return Err(std::io::Error::last_os_error());
                }
                if libc::getppid() != parent {
                    libc::_exit(137);
                }
                Ok(())
            });
        }
    }
    cmd.stdin(if completion_key.is_some() {
        Stdio::piped()
    } else {
        Stdio::null()
    })
    .stdout(Stdio::piped())
    .stderr(Stdio::piped());
    let mut job = axon_jobctl::Job::spawn(cmd)?;
    if let (Some(key), Some(mut stdin)) = (completion_key, job.child_mut().stdin.take()) {
        use std::io::Write;
        let hex: String = key.iter().map(|b| format!("{b:02x}")).collect();
        // Written then closed: the program finds stdin at EOF. 65 bytes, far
        // below a pipe buffer, so this cannot block on a child that never reads.
        let _ = writeln!(stdin, "{hex}");
    }
    let cap = limits.max_output;
    let stop = Arc::new(AtomicBool::new(false));
    let out_h = drain_bounded(job.child_mut().stdout.take(), cap, Arc::clone(&stop));
    let err_h = drain_bounded(job.child_mut().stderr.take(), cap, Arc::clone(&stop));
    let start = std::time::Instant::now();
    let watched = loop {
        match job.has_exited() {
            Ok(true) => break Ok(false),
            Ok(false) if limits.wall.is_some_and(|w| start.elapsed() >= w) => break Ok(true),
            Ok(false) => std::thread::sleep(std::time::Duration::from_millis(20)),
            Err(e) => break Err(e),
        }
    };
    // The whole group, on every path, before the reap (see above).
    job.kill();
    let reaped = job.reap();
    let mut joined =
        axon_jobctl::join_within(axon_jobctl::DRAIN_GRACE, vec![out_h, err_h], &stop).into_iter();
    // A drain that could not be joined lost what it read: the output is
    // incomplete, which is the truncated case (no verdict).
    let (o, ot) = joined.next().flatten().unwrap_or((Vec::new(), true));
    let (e, et) = joined.next().flatten().unwrap_or((Vec::new(), true));
    let timed_out = watched?;
    let status = reaped?;
    if timed_out {
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            BoundExceeded {
                bound: Bound::Time,
                detail: format!(
                    "check killed at its {} ms wall-clock limit",
                    limits.wall.map(|w| w.as_millis()).unwrap_or(0)
                ),
            },
        ));
    }
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&o),
        String::from_utf8_lossy(&e)
    );
    Ok((text, status.code(), ot || et))
}

/// Parse `axon test --json` output. See `Runner::run_tests_json` for why the
/// machine-readable form and why a missing summary is an error.
/// The completion token `axon test --completion-key-stdin` issues for a test
/// that completed: HMAC-SHA256(key, "axon-test-completion/1\0" + name), hex.
/// The interpreter computes the same (axon-core main.rs `completion_token`).
pub fn completion_token(key: &[u8], name: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let (mut ipad, mut opad) = ([0x36u8; 64], [0x5cu8; 64]);
    for i in 0..64 {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }
    let mut msg = b"axon-test-completion/1\0".to_vec();
    msg.extend_from_slice(name.as_bytes());
    let inner = Sha256::new()
        .chain_update(ipad)
        .chain_update(&msg)
        .finalize();
    let outer = Sha256::new()
        .chain_update(opad)
        .chain_update(inner)
        .finalize();
    outer.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn parse_axon_test_json(text: &str, rel_path: &str) -> std::io::Result<CheckReport> {
    let (mut failed, mut passed, mut total) = (Vec::new(), Vec::new(), None);
    let mut completion = Vec::new();
    let (mut messages, mut output) = (Vec::new(), Vec::new());
    for raw in text.lines() {
        let line = raw.trim();
        if !line.starts_with('{') {
            output.push(raw.to_string());
            continue;
        }
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) == Some("summary") {
            total = v.get("total").and_then(|t| t.as_u64()).map(|t| t as usize);
            continue;
        }
        let (Some(name), Some(status)) = (
            v.get("name").and_then(|n| n.as_str()),
            v.get("status").and_then(|s| s.as_str()),
        ) else {
            continue;
        };
        match status {
            "ok" => {
                if let Some(c) = v.get("completion").and_then(|c| c.as_str()) {
                    completion.push((name.to_string(), c.to_string()));
                }
                passed.push(name.to_string())
            }
            "failed" => {
                if let Some(m) = v.get("message").and_then(|m| m.as_str()) {
                    messages.push((name.to_string(), m.to_string()));
                }
                failed.push(name.to_string())
            }
            // An unknown status is neither. Guessing which it resembles is
            // how a new status becomes a silent wrong answer.
            _ => {}
        }
    }
    // No summary means the run did not finish — a compile error, a crash,
    // a missing binary. That is an ERROR, never "no tests failed".
    let Some(total) = total else {
        return Err(std::io::Error::other(format!(
            "`axon test --json` produced no summary for {rel_path}; the run \
             did not complete, which is not the same as nothing failing"
        )));
    };
    Ok(CheckReport {
        completion,
        failed,
        passed,
        total,
        exit_code: None,
        messages,
        output,
    })
}

// ── fabric-2: check dispatch through the Fabric submit path ─────────────────

/// Where a Fabric-dispatched check goes. Every field is OPERATOR input (the
/// cortex command line), never model output.
#[derive(Debug, Clone)]
pub struct FabricDispatch {
    /// Trusted registry naming `axon-fabric` (the submit binary) and
    /// `axon-test-local` (the interpreter the Fabric runs the check with).
    pub registry_file: PathBuf,
    /// The Fabric's operation journal.
    pub journal: PathBuf,
    /// The axon-loop store that holds the authority epoch.
    pub store: PathBuf,
    pub tenant: String,
    pub family: String,
    /// The epoch this caller was authorized under. The Fabric refuses the
    /// dispatch unless the store's CURRENT epoch equals it — at submit and
    /// again immediately before launch.
    pub expected_epoch: u64,
    /// The operator's `axon-fabric-grant-registry/1` file. The Fabric
    /// resolves `grant_ref` for `principal_ref` from it; the check runs under
    /// the effect ceiling that grant induces.
    pub grant_registry: PathBuf,
    /// Who the checks run as (a claim the grant registry must bind to
    /// `grant_ref`). Operator input; never defaulted.
    pub principal_ref: String,
    pub grant_ref: String,
    /// `acf1:<hex>` digest of the governing policy. Operator input; the
    /// all-zero placeholder is refused here and by the Fabric.
    pub policy_digest: String,
    pub task_id: String,
}

/// The registry id of the Fabric submit binary.
pub const FABRIC_SUBMIT_ID: &str = "axon-fabric";

/// A [`CheckExecutor`] that dispatches every check as an
/// `acf-compute-request/1` through the REGISTERED `axon-fabric submit`
/// binary. The Fabric resolves the interpreter from its own registry, checks
/// the authority epoch at submit and again immediately before launch,
/// journals intent → reserve → launch → terminal around the effect, and
/// returns an `acf-execution-receipt/1` together with the check report.
///
/// The seam is the binary, not a library call: `axon-fabric` depends on this
/// crate, so linking it here would be a cycle. The binary is itself pinned by
/// sha256, so what adjudicates is still never chosen by request text.
///
/// Each dispatch gets a fresh operation id, and its `workspace_version_ref`
/// is the one-file WorkspaceVersion of the checked file (B261), which the
/// Fabric copies into its store and judges. A report is accepted only when
/// the receipt's `input_workspace_ref` AND `output_workspace_ref` are that
/// same version: the receipt names the artifact it judged, and the artifact
/// did not move under the verdict.
pub struct FabricSubmitExecutor {
    cfg: FabricDispatch,
    submit_bin: std::sync::Mutex<Pin>,
    interpreter_sha256: String,
    seq: std::sync::atomic::AtomicU64,
    nonce: String,
    /// Every receipt returned so far, for evidence (`cortex --json`).
    receipts: std::sync::Mutex<Vec<serde_json::Value>>,
}

impl FabricSubmitExecutor {
    pub const PROFILE: &'static str = "fabric-submit";

    pub fn new(cfg: FabricDispatch) -> Result<Self, String> {
        for (name, v) in [
            ("principal_ref", &cfg.principal_ref),
            ("grant_ref", &cfg.grant_ref),
        ] {
            if v.trim().is_empty() {
                return Err(format!("fabric dispatch needs a non-empty {name}"));
            }
        }
        let hex = cfg.policy_digest.strip_prefix("acf1:").unwrap_or("");
        if hex.len() != 64 || !hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return Err(format!(
                "policy_digest `{}` is not `acf1:` + 64 lowercase hex",
                cfg.policy_digest
            ));
        }
        if hex.bytes().all(|b| b == b'0') {
            return Err(
                "policy_digest is the all-zero placeholder; name the governing policy".into(),
            );
        }
        let reg = CheckRegistry::load(&cfg.registry_file)?;
        let fab = reg
            .get(FABRIC_SUBMIT_ID)
            .map_err(|e| e.to_string())?
            .clone();
        let interp = reg
            .get(LOCAL_AXON_TEST_ID)
            .map_err(|e| e.to_string())?
            .sha256
            .clone();
        let stamp = file_stamp(&fab.path);
        let seed = format!("{}-{:?}", std::process::id(), std::time::SystemTime::now());
        let nonce = sha256_hex(seed.as_bytes())[..12].to_string();
        Ok(FabricSubmitExecutor {
            cfg,
            submit_bin: std::sync::Mutex::new(Pin::Pinned(fab, stamp)),
            interpreter_sha256: interp,
            seq: std::sync::atomic::AtomicU64::new(0),
            nonce,
            receipts: std::sync::Mutex::new(Vec::new()),
        })
    }

    /// The receipts every dispatch so far produced, in order.
    pub fn receipts(&self) -> Vec<serde_json::Value> {
        self.receipts
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
    }

    fn submit_path(&self) -> Result<PathBuf, CheckRefusal> {
        let mut pin = self.submit_bin.lock().unwrap_or_else(|p| p.into_inner());
        let Pin::Pinned(e, stamp) = &mut *pin else {
            unreachable!("FabricSubmitExecutor is constructed pinned")
        };
        let now = file_stamp(&e.path);
        if now.is_some() && now == *stamp {
            return Ok(e.path.clone());
        }
        let bytes = std::fs::read(&e.path).map_err(|err| CheckRefusal::Unresolvable {
            path: e.path.clone(),
            reason: err.to_string(),
        })?;
        let found = sha256_hex(&bytes);
        if found != e.sha256 {
            return Err(CheckRefusal::DigestChanged {
                path: e.path.clone(),
                registered: e.sha256.clone(),
                found,
            });
        }
        *stamp = now;
        Ok(e.path.clone())
    }

    fn request(&self, req: &CheckRequest<'_>) -> std::io::Result<serde_json::Value> {
        let bytes = std::fs::read(req.workspace.join(req.rel_path))?;
        let n = self.seq.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut argv = vec![req.rel_path.to_string()];
        if let Some(f) = req.filter {
            argv.push(f.to_string());
        }
        Ok(serde_json::json!({
            "schema": "acf-compute-request/1",
            "operation_id": format!("cortex-{}-{n}", self.nonce),
            "task_id": self.cfg.task_id,
            "trial_id": format!("trial-{}", self.nonce),
            "attempt_id": format!("attempt-{n}"),
            "principal_ref": self.cfg.principal_ref,
            "grant_ref": self.cfg.grant_ref,
            "approval_ref": null,
            "job_kind": "registered_check",
            "registered_executable_ref": LOCAL_AXON_TEST_ID,
            "executable_digest": fabric_executable_digest(LOCAL_AXON_TEST_ID, &self.interpreter_sha256),
            "workspace_version_ref": single_file_workspace_version_ref(req.rel_path, &bytes),
            "semantic_state_ref": null,
            "policy_digest": self.cfg.policy_digest,
            "required": {
                "engine": "axon_interpreter",
                "hardware_isolation": false,
                "os": "none",
                "architecture": "x86_64",
                "network_mode": "deny",
                "checkpoint_kind": "none"
            },
            "limits": {
                "cpu_millicores": 1000,
                "memory_bytes": 1u64 << 30,
                "disk_bytes": 1u64 << 30,
                "wall_time_ms": 120_000,
                "output_bytes": 4u64 << 20,
                "max_cost_micro": 1,
                "currency_code": "USD",
                "price_schedule_ref": "unpriced:local-interpreter"
            },
            "argv": argv,
            "result_schema_ref": "cortex-check-report/1"
        }))
    }
}

// The `acf1:` canonicaliser and the WorkspaceVersion byte recipe live in
// `axon-workspace-recipe`, the one crate the host (cortex, fabric) AND the
// guest verdict runner link, so both compute a tree digest with the same
// code (v022-psv-protocol.md §4). Re-exported so every existing caller is
// unchanged.
pub use axon_workspace_recipe::{
    acf1_canonical_bytes, acf1_digest, fabric_executable_digest, fabric_workspace_digest,
    single_file_workspace_version_ref, workspace_manifest_bytes, workspace_version_ref,
    WorkspaceManifestEntry, WORKSPACE_VERSION_SCHEMA,
};

impl CheckExecutor for FabricSubmitExecutor {
    fn id(&self) -> String {
        format!(
            "{}:{}@epoch:{}",
            Self::PROFILE,
            self.cfg.journal.display(),
            self.cfg.expected_epoch
        )
    }

    fn run_checks(&self, req: &CheckRequest<'_>) -> std::io::Result<CheckReport> {
        use std::io::Write;
        let exe = self.submit_path()?;
        let request = self.request(req)?;
        let c = &self.cfg;
        let mut child = std::process::Command::new(&exe)
            .arg("submit")
            .arg("--request")
            .arg("-")
            .arg("--journal")
            .arg(&c.journal)
            .arg("--check-registry")
            .arg(&c.registry_file)
            .arg("--grant-registry")
            .arg(&c.grant_registry)
            .arg("--store")
            .arg(&c.store)
            .arg("--tenant")
            .arg(&c.tenant)
            .arg("--family")
            .arg(&c.family)
            .arg("--expected-epoch")
            .arg(c.expected_epoch.to_string())
            .arg("--workspace")
            .arg(req.workspace)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()?;
        child
            .stdin
            .take()
            .expect("piped")
            .write_all(request.to_string().as_bytes())?;
        let out = child.wait_with_output()?;
        let text = String::from_utf8_lossy(&out.stdout);
        let v: serde_json::Value = serde_json::from_str(text.trim()).map_err(|e| {
            std::io::Error::other(format!(
                "axon-fabric submit exited {:?} without a result ({e}): {}",
                out.status.code(),
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        })?;
        if let Some(r) = v.get("receipt").filter(|r| !r.is_null()) {
            self.receipts
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .push(r.clone());
        }
        if v.get("schema").and_then(|s| s.as_str()) != Some("axon-fabric-submit/1") {
            return Err(std::io::Error::other(format!(
                "axon-fabric refused the check: {}",
                v.get("reason")
                    .and_then(|r| r.as_str())
                    .unwrap_or("no reason")
            )));
        }
        // The receipt must bind THIS candidate: the version sent in, and the
        // same version left behind. A verdict about other bytes — or one
        // whose output moved under it — is no verdict about the candidate.
        let sent = &request["workspace_version_ref"];
        let rc = &v["receipt"];
        if &rc["input_workspace_ref"] != sent || &rc["output_workspace_ref"] != sent {
            return Err(std::io::Error::other(format!(
                "Fabric receipt does not bind the candidate {sent}: input {}, output {}",
                rc["input_workspace_ref"], rc["output_workspace_ref"]
            )));
        }
        // Fabric's VERDICT decides, not the raw report: a report can list the
        // named test as passed while Fabric recorded Unknown — no completion
        // evidence, a nonzero exit, a moved output. Reading `passed` straight
        // from the report let Cortex accept exactly the passes Fabric refused
        // (PCI candidate-2 review). Only `passed` or `failed` is a verdict.
        match rc["verification"].as_str() {
            Some("passed") | Some("failed") => {}
            other => {
                return Err(std::io::Error::other(format!(
                    "Fabric recorded no verdict ({}): {}",
                    other.unwrap_or("absent"),
                    v.get("reason").and_then(|r| r.as_str()).unwrap_or("")
                )))
            }
        }
        // No check report ⇒ no verdict (refused, timed out, outcome unknown…).
        let rep = v
            .get("check_report")
            .filter(|r| !r.is_null())
            .ok_or_else(|| {
                std::io::Error::other(format!(
                    "Fabric receipt status {} carries no check report: {}",
                    v["receipt"]["status"],
                    v.get("reason").and_then(|r| r.as_str()).unwrap_or("")
                ))
            })?;
        let names = |k: &str| -> Vec<String> {
            rep[k]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default()
        };
        Ok(CheckReport {
            completion: Vec::new(),
            failed: names("failed"),
            passed: names("passed"),
            total: rep["total"].as_u64().unwrap_or(0) as usize,
            exit_code: rep["exit_code"].as_i64().map(|c| c as i32),
            messages: Vec::new(),
            output: Vec::new(),
        })
    }
}

#[cfg(test)]
mod walk_budget_tests {
    use super::{WalkBudget, WalkLimit};
    use std::time::{Duration, Instant};

    #[test]
    fn the_proposal_cap_admits_exactly_n_calls_and_remembers_why() {
        let mut b = WalkBudget::new(Some(2), None);
        assert_eq!(b.admit(None), Ok(()));
        assert_eq!(b.admit(None), Ok(()));
        assert_eq!(b.admit(None), Err(WalkLimit::Proposals));
        assert_eq!(b.proposals_used(), 2);
        assert_eq!(b.exhausted_by(), Some(WalkLimit::Proposals));
    }

    /// An `ai:` call cannot be cut at the deadline, so it is not started
    /// with less than its own timeout left; a cuttable one still is.
    #[test]
    fn an_uncancellable_call_is_not_started_without_its_time_left() {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut cuttable = WalkBudget::new(None, Some(deadline));
        assert_eq!(cuttable.admit(None), Ok(()));
        let mut ai = WalkBudget::new(None, Some(deadline));
        assert_eq!(
            ai.admit(Some(Duration::from_secs(30))),
            Err(WalkLimit::Deadline)
        );
        assert_eq!(ai.proposals_used(), 0);
    }

    #[test]
    fn unset_limits_admit_everything() {
        let mut b = WalkBudget::default();
        for _ in 0..100 {
            assert_eq!(b.admit(Some(Duration::from_secs(30))), Ok(()));
        }
        assert_eq!(b.exhausted_by(), None);
        assert!(!b.deadline_passed());
    }
}
