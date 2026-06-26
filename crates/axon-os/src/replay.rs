//! R21 §4.6 — deterministic replay + record verification. Pure core; the
//! `Runtime` and the record store are injected.
//!
//! `replay` (1) verifies the stored record's integrity, then (2) re-executes
//! the job with the RECORDED seed and asserts the new record is byte-identical
//! to the stored one. Divergence or tamper → `VerifyMismatch` (exit 9). This is
//! the deterministic-audit guarantee: a recorded run can be reproduced and
//! proven to be exactly what happened.

use crate::manifest::JobManifest;
use crate::record::{to_json, verify, RunRecord, VerifyMismatch};
use crate::runtime::Runtime;
use crate::supervisor;

/// Re-run a stored record's job and assert the result is byte-identical.
///
/// `stored` is the record loaded from disk; `manifest` is the job it ran (the
/// caller loads both — replay does no I/O itself). `supervisor_grant` is the
/// supervisor's current authority. Returns the freshly-produced record on a
/// match, or `VerifyMismatch` on tamper / divergence.
pub fn replay(
    stored: &RunRecord,
    manifest: &JobManifest,
    supervisor_grant: &crate::grant::Grant,
    rt: &impl Runtime,
) -> Result<RunRecord, VerifyMismatch> {
    // 1. The stored record must be intact before we trust it.
    verify(stored)?;

    // 2. Re-execute with the recorded seed (the manifest carries it) under the
    //    stored run-id, then compare byte-for-byte.
    let fresh = supervisor::run(manifest, supervisor_grant, &stored.run_id, rt);
    if to_json(&fresh) == to_json(stored) {
        Ok(fresh)
    } else {
        Err(VerifyMismatch {
            detail: "replay diverged from the recorded run (non-deterministic or tampered)"
                .to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gate::DeclaredEffects;
    use crate::grant::{Budget, EffectSet, ExecPolicy, Grant, Label};
    use crate::record::RawEvent;
    use crate::runtime::{MockRuntime, RunOutcome};
    use crate::verdict::Verdict;
    use std::path::PathBuf;

    fn grant() -> Grant {
        Grant {
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 10,
                tokens: 10,
                cost_micro: 10,
            },
        }
    }

    fn manifest() -> JobManifest {
        JobManifest {
            program: PathBuf::from("/jobs/x.ax"),
            intent: "demo".into(),
            seed: 42,
            grant: grant(),
        }
    }

    fn rt() -> MockRuntime {
        MockRuntime::new(
            DeclaredEffects {
                row: EffectSet {
                    fs_read: true,
                    fs_write: true,
                    net: false,
                    exec: false,
                },
                max_label: Label::Internal,
            },
            RunOutcome {
                events: vec![RawEvent::new(
                    "fs_write",
                    "./out/x",
                    EffectSet {
                        fs_write: true,
                        ..Default::default()
                    },
                    "internal",
                )],
                verdict: Verdict::Completed { value: 3 },
            },
        )
    }

    #[test]
    fn replay_reproduces_and_verifies() {
        let stored = supervisor::run(&manifest(), &grant(), "demo", &rt());
        // A faithful (same MockRuntime) replay reproduces byte-identically.
        let out = replay(&stored, &manifest(), &grant(), &rt());
        assert!(out.is_ok(), "deterministic replay must match");
    }

    #[test]
    fn replay_rejects_a_tampered_record() {
        let mut stored = supervisor::run(&manifest(), &grant(), "demo", &rt());
        stored.events[0].target = "./out/EVIL".into(); // tamper
        let out = replay(&stored, &manifest(), &grant(), &rt());
        assert!(out.is_err(), "a tampered record must fail before re-run");
    }

    #[test]
    fn replay_detects_divergence() {
        // Store under one runtime, replay under a DIFFERENT one (different
        // verdict) → byte mismatch → VerifyMismatch.
        let stored = supervisor::run(&manifest(), &grant(), "demo", &rt());
        let divergent = MockRuntime::new(
            DeclaredEffects {
                row: EffectSet {
                    fs_read: true,
                    fs_write: true,
                    net: false,
                    exec: false,
                },
                max_label: Label::Internal,
            },
            RunOutcome {
                events: vec![],
                verdict: Verdict::Completed { value: 999 }, // different
            },
        );
        let out = replay(&stored, &manifest(), &grant(), &divergent);
        assert!(out.is_err(), "divergent replay must be caught");
    }
}
