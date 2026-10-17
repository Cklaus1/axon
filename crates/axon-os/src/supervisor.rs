//! R21 §4.2 — the run pipeline: gate → mint → run sandboxed → record. Pure
//! orchestration; all I/O is injected via `Runtime`. The supervisor itself
//! performs no I/O, which is what makes it mock-testable.

use crate::gate::{admit, Admission};
use crate::grant::EffectSet;
use crate::manifest::JobManifest;
use crate::record::{build, RawEvent, RunRecord};
use crate::runtime::{IsolationRequirement, ProgramSource, Runtime};
use crate::verdict::Verdict;

/// Run a job under the supervisor (R21 §4.2). `supervisor_grant` is the
/// authority the supervisor itself holds; the program is run under the
/// EFFECTIVE grant `manifest.grant ∩ supervisor_grant` — so a job can never
/// obtain authority the supervisor lacks. Fails closed at the gate (no
/// execution) and at runtime (the runtime maps over-reach to a fail-closed
/// verdict). Always returns a tamper-evident record.
pub fn run(
    manifest: &JobManifest,
    job_path: &std::path::Path,
    supervisor_grant: &crate::grant::Grant,
    run_id: &str,
    rt: &impl Runtime,
) -> RunRecord {
    run_requiring(
        manifest,
        job_path,
        supervisor_grant,
        run_id,
        IsolationRequirement::Any,
        rt,
    )
}

/// [`run`], for a request that states the confinement it REQUIRES (B259).
///
/// If `rt` cannot provide it, the job is refused on the `isolation` axis
/// BEFORE approval, admission, mint or run — zero runtime calls. There is no
/// fallback: a request requiring a microVM is never handed to a
/// `process_scoped` runtime (G13-r22-no-weak-fallback).
pub fn run_requiring(
    manifest: &JobManifest,
    job_path: &std::path::Path,
    supervisor_grant: &crate::grant::Grant,
    run_id: &str,
    required: IsolationRequirement,
    rt: &impl Runtime,
) -> RunRecord {
    use crate::approval::{ApprovalKey, ApprovalStatus};
    // The record seal key (G5c). Every return below passes the approval
    // decision INTO `build`, so it is sealed: it used to be stamped on the
    // record afterwards and could be rewritten freely. A malformed key is a
    // refusal, never a silent fall back to an unkeyed seal.
    let key = match ApprovalKey::from_env() {
        Ok(k) => k,
        Err(reason) => {
            let denial = RawEvent::new("denied", "approval", EffectSet::default(), "");
            return build(
                run_id,
                manifest,
                manifest.seed,
                std::slice::from_ref(&denial),
                Verdict::Denied {
                    reason,
                    axis: "approval".to_string(),
                },
                ApprovalStatus::Denied.as_str(),
                None,
            );
        }
    };
    let key = key.as_ref();
    let iso = rt.isolation();
    if !required.satisfied_by(iso) {
        let denial = RawEvent::new("denied", "isolation", EffectSet::default(), "");
        return build(
            run_id,
            manifest,
            manifest.seed,
            std::slice::from_ref(&denial),
            Verdict::Denied {
                reason: format!(
                    "request requires {required:?} isolation; this runtime is {} \
                     and is never used as a fallback for it",
                    iso.label()
                ),
                axis: "isolation".to_string(),
            },
            // Refused before authorization ran: say so rather than `unknown`.
            ApprovalStatus::NotChecked.as_str(),
            key,
        );
    }
    // 0. AUTHORIZATION — before anything else, at the point every execution
    //    path converges on.
    //
    //    This check lived in `cmd_run`, so `axon-os replay` reached execution
    //    by a different route and never performed it. REPRODUCED: a stored job
    //    marked `require_approval = true` with no token anywhere was refused by
    //    `run` with exit 8 and no side effect, then re-executed by `replay`
    //    with exit 0 and the side-effect file's mtime advancing. The public
    //    `axon_os::supervise` re-export was a third route with no gate at all.
    //
    //    Step 3 below already demonstrates the pattern: `admit` sits here and
    //    every caller gets it whether or not they remember. A check in a CALLER
    //    is opt-in per call site.
    //
    //    AR1: the entry is read ONCE, here. The token is verified against this
    //    text, the static scan judges it and the sandbox stages it; no runtime
    //    re-reads the file, so an entry rewritten after this point cannot run
    //    under a token that verified other bytes.
    let program_text = rt.load_program(&manifest.program);
    let src = program_text.as_deref().ok();
    let approval = match crate::approval::authorize(job_path, manifest, src, key, rt) {
        Ok(a) => a,
        Err(reason) => {
            let denial = RawEvent::new("denied", "approval", EffectSet::default(), "");
            return build(
                run_id,
                manifest,
                manifest.seed,
                std::slice::from_ref(&denial),
                Verdict::Denied {
                    reason,
                    axis: "approval".to_string(),
                },
                ApprovalStatus::Denied.as_str(),
                key,
            );
        }
    };
    // An unreadable program never runs. Authorization ran, so its status is
    // sealed (not `not_checked`).
    let program_text = match program_text {
        Ok(t) => t,
        Err(e) => {
            let denial = RawEvent::new("denied", "io", EffectSet::default(), "");
            return build(
                run_id,
                manifest,
                manifest.seed,
                std::slice::from_ref(&denial),
                Verdict::Denied {
                    reason: format!("cannot read program: {e}"),
                    axis: "io".to_string(),
                },
                approval.status.as_str(),
                key,
            );
        }
    };
    let program = ProgramSource {
        path: &manifest.program,
        text: &program_text,
    };
    // 1. What does the program declare it may do? (deny-by-default via the runtime)
    let declared = rt.declared_effects(&program);

    // 2. The effective grant: you cannot delegate authority you lack.
    let eff = manifest.grant.intersect(supervisor_grant);

    // 3. Static admission — fail closed BEFORE any execution.
    // The authorization decision travels with the record, so a reader can tell
    // afterward whether the run was authorized. Sealed on EVERY exit path — the
    // first version computed it and used it on none. A record field that is
    // always `unknown` is exactly the absent-vs-verified collapse this field
    // exists to close.
    if let Admission::Deny { reason, axis } = admit(&declared, &eff) {
        let denial = RawEvent::new("denied", &axis, EffectSet::default(), "");
        return build(
            run_id,
            manifest,
            manifest.seed,
            std::slice::from_ref(&denial),
            Verdict::Denied { reason, axis },
            approval.status.as_str(),
            key,
        );
    }

    // 4. Mint a Principal holding exactly the effective grant.
    let principal = rt.mint_principal(&eff);

    // 5. Run sandboxed to the effective ceiling + budget + seed, loading only
    //    the import closure the approval bound (G1e).
    let outcome = rt.run_sandboxed(
        &program,
        &principal,
        &eff,
        &eff.budget,
        manifest.seed,
        approval.required_imports.as_deref(),
    );

    // 6. Seal a tamper-evident record from the observed events + verdict.
    build(
        run_id,
        manifest,
        manifest.seed,
        &outcome.events,
        outcome.verdict,
        approval.status.as_str(),
        key,
    )
}

#[cfg(test)]
mod tests {

    /// A job path with no `.approval` sibling.
    ///
    /// These tests exercise admission and determinism, not sign-off; a manifest
    /// that does not set `require_approval` authorizes as `NotRequired`. Named
    /// rather than inlined so the intent is legible: the tests are not bypassing
    /// the gate, they are exercising the case where the gate permits.
    fn no_approval_path() -> &'static std::path::Path {
        std::path::Path::new("/nonexistent/axon-os-test-job.axjob")
    }
    use super::*;
    use crate::gate::DeclaredEffects;
    use crate::grant::{Budget, ExecPolicy, Grant, Label};
    /// Verify under the same key the supervisor sealed with (the ambient one).
    fn verify(rec: &RunRecord) -> Result<crate::record::Sealing, crate::record::VerifyMismatch> {
        let key = crate::approval::ApprovalKey::from_env().unwrap();
        crate::record::verify(rec, key.as_ref())
    }
    use crate::runtime::{MockRuntime, RunOutcome};
    use std::path::PathBuf;

    fn grant(net: bool) -> Grant {
        Grant {
            reproducible: false,
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: if net { vec!["x.com".into()] } else { vec![] },
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 100,
                tokens: 100,
                cost_micro: 100,
            },
        }
    }

    fn manifest(net_grant: bool) -> JobManifest {
        JobManifest {
            require_approval: false,
            program: PathBuf::from("/jobs/x.ax"),
            intent: "demo".into(),
            seed: 42,
            grant: grant(net_grant),
        }
    }

    fn declares(net: bool) -> DeclaredEffects {
        DeclaredEffects {
            row: EffectSet {
                fs_read: true,
                fs_write: true,
                net,
                exec: false,
            },
            max_label: Label::Internal,
        }
    }

    #[test]
    fn happy_path_admits_runs_and_records() {
        let events = vec![RawEvent::new(
            "fs_write",
            "./out/summary.txt",
            EffectSet {
                fs_write: true,
                ..Default::default()
            },
            "internal",
        )];
        let rt = MockRuntime::new(
            declares(false),
            RunOutcome {
                events,
                verdict: Verdict::Completed { value: 7 },
            },
        );
        let rec = run(
            &manifest(true),
            no_approval_path(),
            &grant(true),
            "demo",
            &rt,
        );
        assert_eq!(rec.verdict, Verdict::Completed { value: 7 });
        assert_eq!(rec.events.len(), 1);
        assert!(verify(&rec).is_ok());
        assert_eq!(rt.run_calls.get(), 1);
    }

    /// AR1: the supervisor reads the entry once; an unreadable entry is
    /// refused on the `io` axis after authorization (sealed with its status),
    /// before admission, mint or run, even when the declared effects would be
    /// admitted.
    #[test]
    fn an_unreadable_program_is_refused_on_the_io_axis_after_authorization() {
        let mut rt = MockRuntime::new(
            declares(false),
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 0 },
            },
        );
        rt.source = Err("No such file or directory (os error 2)".into());
        let rec = run(
            &manifest(true),
            no_approval_path(),
            &grant(true),
            "unreadable",
            &rt,
        );
        match &rec.verdict {
            Verdict::Denied { axis, reason } => {
                assert_eq!(axis, "io");
                assert!(reason.starts_with("cannot read program: "), "{reason}");
            }
            other => panic!("ATTACK: an unreadable program was run: {other:?}"),
        }
        assert_eq!(rec.approval, "not_required");
        assert_eq!((rt.mint_calls.get(), rt.run_calls.get()), (0, 0));
        assert!(verify(&rec).is_ok());
    }

    #[test]
    fn runtime_overreach_fails_closed() {
        // The program was admitted statically, but the runtime observed a
        // capability violation (exit-8 style) → the record is sealed Denied.
        let rt = MockRuntime::new(
            declares(false),
            RunOutcome {
                events: vec![RawEvent::new(
                    "net",
                    "evil.com",
                    EffectSet {
                        net: true,
                        ..Default::default()
                    },
                    "internal",
                )],
                verdict: Verdict::Denied {
                    reason: "runtime sandbox violation".into(),
                    axis: "net".into(),
                },
            },
        );
        let rec = run(
            &manifest(true),
            no_approval_path(),
            &grant(true),
            "demo",
            &rt,
        );
        assert!(matches!(rec.verdict, Verdict::Denied { .. }));
        assert_eq!(rec.verdict.exit_code(), 8);
        assert!(verify(&rec).is_ok());
    }

    #[test]
    fn deny_before_run_never_invokes_the_runtime() {
        // The program declares net, but neither the manifest grant nor the
        // supervisor grant has it → gate Deny ⇒ run_sandboxed is NEVER called.
        let rt = MockRuntime::new(
            declares(true),
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 0 },
            },
        );
        let rec = run(
            &manifest(false),
            no_approval_path(),
            &grant(false),
            "demo",
            &rt,
        );
        match &rec.verdict {
            Verdict::Denied { axis, .. } => assert_eq!(axis, "net"),
            other => panic!("expected Denied, got {other:?}"),
        }
        assert_eq!(rt.run_calls.get(), 0, "no execution on a denial");
        assert_eq!(rt.mint_calls.get(), 0, "no minting on a denial");
        assert!(verify(&rec).is_ok());
    }

    #[test]
    fn supervisor_grant_clamps_a_broad_job() {
        // The job grant has net; the SUPERVISOR grant does not. Even though the
        // program declares net, the effective grant withholds it → Denied.
        let rt = MockRuntime::new(
            declares(true),
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 0 },
            },
        );
        // manifest grants net, supervisor does NOT.
        let rec = run(
            &manifest(true),
            no_approval_path(),
            &grant(false),
            "demo",
            &rt,
        );
        assert!(matches!(rec.verdict, Verdict::Denied { .. }));
        assert_eq!(rt.run_calls.get(), 0);
    }

    /// The gate must fire from INSIDE `supervisor::run`, not from a caller.
    ///
    /// `cmd_run` had its own approval check, so a test that only exercises the
    /// CLI proves nothing about `replay` or the public `supervise` re-export —
    /// the two routes that reached execution without one. This drives the
    /// supervisor directly.
    #[test]
    fn supervisor_refuses_a_job_that_requires_approval_without_a_token() {
        let rt = MockRuntime::new(
            declares(false),
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 0 },
            },
        );
        let mut m = manifest(false);
        m.require_approval = true;
        let rec = run(&m, no_approval_path(), &grant(false), "demo", &rt);
        match &rec.verdict {
            Verdict::Denied { axis, reason } => {
                assert_eq!(axis, "approval", "denied on the wrong axis: {reason}");
                assert!(
                    reason.contains("approval required but missing"),
                    "the refusal must say why: {reason}"
                );
            }
            other => panic!("a job requiring approval with no token reached execution: {other:?}"),
        }
    }

    /// Control: the supervisor must not refuse everything.
    #[test]
    fn supervisor_runs_a_job_that_does_not_require_approval() {
        let rt = MockRuntime::new(
            declares(false),
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 0 },
            },
        );
        let rec = run(
            &manifest(false),
            no_approval_path(),
            &grant(false),
            "demo",
            &rt,
        );
        assert!(
            !matches!(&rec.verdict, Verdict::Denied { axis, .. } if axis == "approval"),
            "a job that does not require approval must not be denied on that axis"
        );
        assert_eq!(rec.approval, "not_required");
    }
}
