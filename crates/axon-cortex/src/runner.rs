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
const POLICY_FILES: &[&str] = &["axon.lock", ".axon-policy", "gate.sh", "profile.rs"];

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
}

pub struct Runner {
    pub axon_bin: PathBuf,
    pub workspace: PathBuf,
    pub episode: Episode,
    /// The previous snapshot's id, so each new snapshot records its parent and
    /// an episode's state chain is auditable rather than a set of orphans.
    last_snapshot_id: Option<String>,
}

impl Runner {
    pub fn new(axon_bin: impl Into<PathBuf>, workspace: impl Into<PathBuf>) -> Self {
        Runner {
            axon_bin: axon_bin.into(),
            workspace: workspace.into(),
            episode: Episode::new("cortex-repair-1"),
            last_snapshot_id: None,
        }
    }

    /// Snapshot the workspace region, content-addressed.
    pub fn snapshot(&mut self, scope: &[&str]) -> std::io::Result<WorkspaceSnapshot> {
        let mut files = Vec::new();
        for rel in scope {
            let p = self.workspace.join(rel);
            if p.is_file() {
                let bytes = std::fs::read(&p)?;
                files.push(((*rel).to_string(), content_digest(&bytes)));
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
        let out = std::process::Command::new(&self.axon_bin)
            .arg("check")
            .arg(self.workspace.join(target))
            .output();
        let (diagnostics, warnings, facts, omissions) = match out {
            Ok(o) => {
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
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
                self.episode.push(EpisodeEvent::CheckRun {
                    name: "claim_done".to_string(),
                    exit_code: 0,
                    passed: claim.done,
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
        // Every (workspace state, action) pair already tried. A SET, not just
        // the previous step: the loop alternates patch → claim → patch, so a
        // last-step comparison cannot see a two-step cycle and the episode
        // would spin until the budget ran out — reporting "out of budget" for
        // what is really "going in circles".
        let mut seen: Vec<(String, String, usize)> = Vec::new();
        let mut ctx = crate::select::SelectionContext::default();
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
        let baseline_passing: Vec<String> = match self.check_outcomes(&target.path, "") {
            Ok((_, passing)) => passing,
            Err(e) => {
                self.episode.push(EpisodeEvent::CheckRun {
                    name: format!("baseline UNAVAILABLE: {e}"),
                    exit_code: -1,
                    passed: false,
                });
                Vec::new()
            }
        };
        let mut effective: Option<EditGrant> = grant.cloned();
        let mut rejected: Vec<crate::generate::RejectedAttempt> = Vec::new();
        let mut last_applied: Option<String> = None;
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
        for step in 1..=budget {
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
            let action = match crate::select::select_action_with(&obs, target, &ctx) {
                crate::select::Selection::Act(a) => a,
                crate::select::Selection::Blocked(reason) => {
                    return EpisodeOutcome::Blocked {
                        steps: step,
                        reason,
                    }
                }
            };

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
                    let constraints = crate::generate::PatchConstraints {
                        symbol: symbol.clone(),
                        max_bytes: 4096,
                        current_body,
                        rejected: rejected.clone(),
                    };
                    let proposal = match gen.propose(&obs, target, &constraints) {
                        Ok(p) => p,
                        Err(why) => {
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
                    self.episode.push(EpisodeEvent::CheckRun {
                        name: format!("patch_proposed_by:{}", proposal.generator_id),
                        exit_code: 0,
                        passed: true,
                    });
                    last_applied = Some(proposal.body.clone());
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
                        push_rejected(
                            &mut rejected,
                            body,
                            crate::generate::RejectionReason::CheckRefused,
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

            match &outcome {
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
                        push_rejected(
                            &mut rejected,
                            body,
                            crate::generate::RejectionReason::DidNotCompile,
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
                if self.verify(true, hidden_check, &target.path) {
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
                    let still_failing: Vec<String> = match self.check_outcomes(&target.path, "") {
                        Ok((failing, _)) => failing,
                        Err(e) => {
                            self.episode.push(EpisodeEvent::Verified {
                                passed: false,
                                detail: format!(
                                    "claim_done=true: hidden check `{hidden_check}` passed, but \
                                     the file could not be re-checked afterwards, so nothing was \
                                     established about the rest of it: {e}"
                                ),
                            });
                            return EpisodeOutcome::Blocked {
                                steps: step,
                                reason: format!("the file could not be re-checked: {e}"),
                            };
                        }
                    };
                    if still_failing.is_empty() {
                        return EpisodeOutcome::VerifiedDone { steps: step };
                    }
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
                        self.episode.push(EpisodeEvent::Verified {
                            passed: false,
                            detail: format!(
                                "claim_done=true: hidden check `{hidden_check}` passed and \
                                 nothing regressed, but {} check(s) that were ALREADY failing \
                                 still fail: {}",
                                still_failing.len(),
                                still_failing.join(", ")
                            ),
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
                             still fail ({}){}",
                            still_failing.len(),
                            still_failing.join(", "),
                            if regressed.is_empty() {
                                String::new()
                            } else {
                                // Worth naming separately: leaving a failure
                                // alone and CREATING one are different
                                // mistakes with different remedies.
                                format!(
                                    " — and this episode BROKE {} that passed before it",
                                    regressed.join(", ")
                                )
                            }
                        ),
                    });
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
                // check's output: the generator learns its answer was rejected,
                // not what the grader wanted.
                if let Some(body) = last_applied.take() {
                    push_rejected(
                        &mut rejected,
                        body,
                        crate::generate::RejectionReason::CheckRefused,
                    );
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
    fn run_tests_json(
        &self,
        rel_path: &str,
        filter: Option<&str>,
    ) -> std::io::Result<(Vec<String>, Vec<String>, usize)> {
        let mut cmd = std::process::Command::new(&self.axon_bin);
        cmd.arg("test")
            .arg(self.workspace.join(rel_path))
            .arg("--json");
        if let Some(f) = filter {
            cmd.arg("--filter").arg(f);
        }
        let out = cmd.output()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let (mut failed, mut passed, mut total) = (Vec::new(), Vec::new(), None);
        for line in text.lines() {
            let line = line.trim();
            if !line.starts_with('{') {
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
                "ok" => passed.push(name.to_string()),
                "failed" => failed.push(name.to_string()),
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
        Ok((failed, passed, total))
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
        if POLICY_FILES.iter().any(|p| target_path.ends_with(p)) {
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
                });
                return false;
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
        });
        ok
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
fn push_rejected(
    rejected: &mut Vec<crate::generate::RejectedAttempt>,
    body: String,
    reason: crate::generate::RejectionReason,
) {
    if !rejected.iter().any(|r| r.body == body) {
        rejected.push(crate::generate::RejectedAttempt { body, reason });
    }
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
