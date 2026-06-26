//! R21 §4.2 — the run pipeline: gate → mint → run sandboxed → record. Pure
//! orchestration; all I/O is injected via `Runtime`. The supervisor itself
//! performs no I/O, which is what makes it mock-testable.

use crate::gate::{admit, Admission};
use crate::grant::EffectSet;
use crate::manifest::JobManifest;
use crate::record::{build, RawEvent, RunRecord};
use crate::runtime::Runtime;
use crate::verdict::Verdict;

/// Run a job under the supervisor (R21 §4.2). `supervisor_grant` is the
/// authority the supervisor itself holds; the program is run under the
/// EFFECTIVE grant `manifest.grant ∩ supervisor_grant` — so a job can never
/// obtain authority the supervisor lacks. Fails closed at the gate (no
/// execution) and at runtime (the runtime maps over-reach to a fail-closed
/// verdict). Always returns a tamper-evident record.
pub fn run(
    manifest: &JobManifest,
    supervisor_grant: &crate::grant::Grant,
    run_id: &str,
    rt: &impl Runtime,
) -> RunRecord {
    // 1. What does the program declare it may do? (deny-by-default via the runtime)
    let declared = rt.declared_effects(&manifest.program);

    // 2. The effective grant: you cannot delegate authority you lack.
    let eff = manifest.grant.intersect(supervisor_grant);

    // 3. Static admission — fail closed BEFORE any execution.
    if let Admission::Deny { reason, axis } = admit(&declared, &eff) {
        let denial = RawEvent::new("denied", &axis, EffectSet::default(), "");
        return build(
            run_id,
            manifest,
            manifest.seed,
            std::slice::from_ref(&denial),
            Verdict::Denied { reason, axis },
        );
    }

    // 4. Mint a Principal holding exactly the effective grant.
    let principal = rt.mint_principal(&eff);

    // 5. Run sandboxed to the effective ceiling + budget + seed.
    let outcome = rt.run_sandboxed(
        &manifest.program,
        &principal,
        eff.effect_set(),
        &eff.budget,
        manifest.seed,
    );

    // 6. Seal a tamper-evident record from the observed events + verdict.
    build(
        run_id,
        manifest,
        manifest.seed,
        &outcome.events,
        outcome.verdict,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::DeclaredEffects;
    use crate::grant::{Budget, ExecPolicy, Grant, Label};
    use crate::record::verify;
    use crate::runtime::{MockRuntime, RunOutcome};
    use std::path::PathBuf;

    fn grant(net: bool) -> Grant {
        Grant {
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
        let rec = run(&manifest(true), &grant(true), "demo", &rt);
        assert_eq!(rec.verdict, Verdict::Completed { value: 7 });
        assert_eq!(rec.events.len(), 1);
        assert!(verify(&rec).is_ok());
        assert_eq!(rt.run_calls.get(), 1);
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
        let rec = run(&manifest(true), &grant(true), "demo", &rt);
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
        let rec = run(&manifest(false), &grant(false), "demo", &rt);
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
        let rec = run(&manifest(true), &grant(false), "demo", &rt);
        assert!(matches!(rec.verdict, Verdict::Denied { .. }));
        assert_eq!(rt.run_calls.get(), 0);
    }
}
