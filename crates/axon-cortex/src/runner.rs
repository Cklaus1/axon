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

/// The legal action catalog. Denial-first: an action absent from this list is
/// refused, rather than allowed because nothing forbade it.
pub const LEGAL_ACTIONS: &[&str] = &[
    "inspect",
    "search",
    "patch_symbol_body",
    "run_check",
    "claim_done",
];

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
    let needle = format!("fn {symbol}");
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
            Ok(()) => Ok(Authorized { action }),
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
        let mut seen: Vec<(String, String)> = Vec::new();
        let mut ctx = crate::select::SelectionContext::default();
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
            let point = (snap.snapshot_id.clone(), action.name().to_string());
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
                    let Some(gen) = generator else {
                        return EpisodeOutcome::NeedsInput {
                            steps: step,
                            what: format!(
                                "a patch body for `{}` and no generator was supplied",
                                target.symbol
                            ),
                        };
                    };
                    let constraints = crate::generate::PatchConstraints {
                        symbol: symbol.clone(),
                        max_bytes: 4096,
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

            let auth = match self.authorize_action(&action, grant, principal, &snap) {
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
            }
            let outcome = self.execute(auth);

            // A claim is adjudicated, never accepted. If verification holds the
            // episode is done; if it does not, the loop keeps going and the
            // next iteration's stuck-detection decides whether that is futile.
            if matches!(action, CortexAction::ClaimDone { .. }) {
                let _ = outcome;
                if self.verify(true, hidden_check, &target.path) {
                    return EpisodeOutcome::VerifiedDone { steps: step };
                }
                // The claim was refused. That is the signal the code compiles
                // and is still wrong — the only state in which proposing a
                // repair is justified rather than a guess.
                ctx.claim_refused = true;
            }
        }
        EpisodeOutcome::BudgetExhausted { steps: budget }
    }

    /// Run ONE named check and report how many tests the name matched.
    ///
    /// `run_check` passes the name to the episode record but not to the
    /// command, so it runs the whole file and reports a named result — the same
    /// defect `verify()` had. This selects, and returns the match count so a
    /// zero-match run can be told from a pass.
    fn run_named_check(&mut self, name: &str, rel_path: &str) -> std::io::Result<(bool, usize)> {
        let out = std::process::Command::new(&self.axon_bin)
            .arg("test")
            .arg(self.workspace.join(rel_path))
            .arg("--filter")
            .arg(name)
            .output()?;
        let text = format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        let matched = text
            .split("running ")
            .nth(1)
            .and_then(|r| r.split(' ').next())
            .and_then(|n| n.parse::<usize>().ok())
            .unwrap_or(0);
        // A filter matching nothing exits 0. Zero matched tests is not a pass.
        let passed = out.status.success() && matched > 0;
        self.episode.push(EpisodeEvent::CheckRun {
            name: name.to_string(),
            exit_code: out.status.code().unwrap_or(-1),
            passed,
        });
        Ok((passed, matched))
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

    /// Validate an action against the catalog and the grant, BEFORE any write.
    ///
    /// Every refusal here happens with no side effect, which is what
    /// `G03-forgery` actually demands — refusing after writing is not refusing.
    pub fn authorize(
        &mut self,
        action: &str,
        grant: Option<&EditGrant>,
        principal: &str,
        current_snapshot: &WorkspaceSnapshot,
        target_path: &str,
    ) -> Result<(), Refusal> {
        let r = self.check_authority(action, grant, principal, current_snapshot, target_path);
        if let Err(ref why) = r {
            self.episode.push(EpisodeEvent::ActionDenied {
                action: action.to_string(),
                reason: why.to_string(),
            });
        }
        r
    }

    /// The authority decision WITHOUT recording it. Same code path as
    /// `authorize`; exists so callers can ask "would this be allowed?" without
    /// writing a denial into the episode.
    pub fn authorize_dry(
        &self,
        action: &str,
        grant: Option<&EditGrant>,
        principal: &str,
        current: &WorkspaceSnapshot,
        target_path: &str,
    ) -> Result<(), Refusal> {
        self.check_authority(action, grant, principal, current, target_path)
    }

    fn check_authority(
        &self,
        action: &str,
        grant: Option<&EditGrant>,
        principal: &str,
        current: &WorkspaceSnapshot,
        target_path: &str,
    ) -> Result<(), Refusal> {
        if !LEGAL_ACTIONS.contains(&action) {
            return Err(Refusal::NotInCatalog(action.to_string()));
        }
        if action != "patch_symbol_body" {
            return Ok(());
        }
        let g = grant.ok_or_else(|| Refusal::UnknownGrant("<none>".into()))?;
        if g.principal != principal {
            return Err(Refusal::WrongPrincipal {
                expected: g.principal.clone(),
                got: principal.to_string(),
            });
        }
        // Authority does not survive the state it was granted over.
        if g.snapshot_id != current.snapshot_id {
            return Err(Refusal::StaleSnapshot {
                expected: g.snapshot_id.clone(),
                got: current.snapshot_id.clone(),
            });
        }
        // Payload checks run even with a VALID grant: authority to edit is not
        // authority to edit anything (`G03-payload`).
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

    /// Apply a patch to the COPY. Callers must have passed `authorize` first;
    /// this re-reads the file and records both digests so the evidence shows
    /// what actually changed rather than what was intended.
    pub fn apply_patch(
        &mut self,
        rel_path: &str,
        find: &str,
        replace: &str,
    ) -> std::io::Result<bool> {
        let p = self.workspace.join(rel_path);
        let before = std::fs::read_to_string(&p)?;
        if !before.contains(find) {
            return Ok(false);
        }
        let after = before.replacen(find, replace, 1);
        std::fs::write(&p, &after)?;
        self.episode.push(EpisodeEvent::PatchApplied {
            path: rel_path.to_string(),
            before_digest: content_digest(before.as_bytes()),
            after_digest: content_digest(after.as_bytes()),
        });
        Ok(true)
    }

    /// Run a registered check — a real process, with its real exit code.
    pub fn run_check(&mut self, name: &str, rel_path: &str) -> std::io::Result<bool> {
        let out = std::process::Command::new(&self.axon_bin)
            .arg("test")
            .arg(self.workspace.join(rel_path))
            .output()?;
        let code = out.status.code().unwrap_or(-1);
        let passed = out.status.success();
        self.episode.push(EpisodeEvent::CheckRun {
            name: name.to_string(),
            exit_code: code,
            passed,
        });
        Ok(passed)
    }

    /// Independent verification, out-of-band from whatever proposed the patch.
    ///
    /// `G03-done`: a DONE claim cannot close the task while hidden checks fail,
    /// "regardless of confidence". `claimed_done` is deliberately an input that
    /// is IGNORED for the verdict — it is recorded so a reviewer can see that a
    /// confident wrong claim was made and overruled.
    pub fn verify(&mut self, claimed_done: bool, hidden_check: &str, rel_path: &str) -> bool {
        // `hidden_check` used to appear ONLY in the detail string: the command
        // was `axon test <file>`, so every verification ran the whole file and
        // then reported that a NAMED check had passed. The evidence asserted
        // something the run never evaluated individually. Selecting it is what
        // makes the name mean anything.
        let out = std::process::Command::new(&self.axon_bin)
            .arg("test")
            .arg(self.workspace.join(rel_path))
            .arg("--filter")
            .arg(hidden_check)
            .output();
        // Three outcomes, not two. A check that could not RUN is not a check
        // that failed, and neither is a pass — collapsing "unobserved" into
        // either one is how an absent verification comes to read as a result.
        // The VERDICT is fail-closed for both non-pass cases; only the recorded
        // evidence distinguishes them, which is precisely who needs to know.
        let (hidden_passed, outcome) = match out {
            Ok(o) if o.status.success() => {
                // Filtering introduces a hole that running the whole file did
                // not have: `axon test --filter nope` matches nothing, runs
                // zero tests and exits 0 — "test result: ok. 0 passed, 0
                // failed". Taken at face value that is a hidden check reporting
                // PASSED while never existing, which is worse than the
                // mislabelling this change set out to fix.
                //
                // So a zero-test run is NOT a pass. It is the same
                // "unobserved" outcome as a checker that could not start, and
                // it is fail-closed for the same reason.
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&o.stdout),
                    String::from_utf8_lossy(&o.stderr)
                );
                let ran = text
                    .split("running ")
                    .nth(1)
                    .and_then(|r| r.split(' ').next())
                    .and_then(|n| n.parse::<usize>().ok())
                    // 0 is the FAIL-CLOSED default and the choice is
                    // deliberate: if the count cannot be read, we have not
                    // observed that the check ran, so it is treated as not
                    // run. `unwrap_or(1)` would read an unparseable line as
                    // "something ran" and let a pass through on output we did
                    // not understand.
                    //
                    // A mutation to 1 SURVIVES the suite, and that is honest
                    // rather than a gap: `axon test` always emits
                    // "running N tests", so this default is unreachable today.
                    // It is kept because the direction matters the moment that
                    // output format changes — which is not a contract — and a
                    // future reader must not "simplify" it to fail-open.
                    .unwrap_or(0);
                if ran == 0 {
                    (
                        false,
                        format!(
                            "DID NOT RUN: no test matched `{hidden_check}` in {rel_path} \
                             (a filter that matches nothing exits 0 and is not a pass)"
                        ),
                    )
                } else {
                    (true, format!("passed ({ran} test(s) matched)"))
                }
            }
            Ok(o) => (false, format!("FAILED (exit {:?})", o.status.code())),
            Err(e) => (false, format!("DID NOT RUN: {e}")),
        };
        let detail = format!(
            "hidden check `{hidden_check}` {outcome}; claim_done={claimed_done} (claim does not affect the verdict)"
        );
        self.episode.push(EpisodeEvent::Verified {
            passed: hidden_passed,
            detail,
        });
        hidden_passed
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
