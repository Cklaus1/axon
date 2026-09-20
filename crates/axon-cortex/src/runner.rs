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
        let out = std::process::Command::new(&self.axon_bin)
            .arg("test")
            .arg(self.workspace.join(rel_path))
            .output();
        // Three outcomes, not two. A check that could not RUN is not a check
        // that failed, and neither is a pass — collapsing "unobserved" into
        // either one is how an absent verification comes to read as a result.
        // The VERDICT is fail-closed for both non-pass cases; only the recorded
        // evidence distinguishes them, which is precisely who needs to know.
        let (hidden_passed, outcome) = match out {
            Ok(o) if o.status.success() => (true, "passed".to_string()),
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
