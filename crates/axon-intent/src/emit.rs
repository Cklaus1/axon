//! R22 §2 — write the `{program.ax, job.axjob, job.approval}` triple to disk.
//! Thin I/O. The triple is exactly the input R21 (`axon-os`) consumes.
//!
//! The `.axjob` is R21's `JobManifest` (reused verbatim — we do not redefine the
//! manifest type); the `.approval` is the R22 token. Emission is deterministic:
//! given the same `Resolved` it writes byte-identical files (A5).

use crate::approval::{build_token, ApprovalToken};
use crate::intent::Intent;
use crate::pipeline::Resolved;
use crate::refusal::Decision;
use axon_os::manifest::{to_axjob, JobManifest};
use std::path::{Path, PathBuf};

/// The on-disk paths of an emitted job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Triple {
    pub program: PathBuf,
    pub manifest: PathBuf,
    pub approval: Option<PathBuf>,
}

/// Build the R21 `JobManifest` for a resolved job. The program path is written
/// as a bare filename (`<name>.ax`) so the manifest + program travel together
/// and R21 resolves the program relative to the manifest's directory.
pub fn build_manifest(resolved: &Resolved, intent: &Intent, name: &str) -> JobManifest {
    JobManifest {
        program: PathBuf::from(format!("{name}.ax")),
        intent: intent.goal.clone(),
        seed: intent.seed,
        grant: resolved.grant.clone(),
    }
}

/// The canonical `.axjob` text for a resolved job (deterministic).
pub fn manifest_text(resolved: &Resolved, intent: &Intent, name: &str) -> String {
    to_axjob(&build_manifest(resolved, intent, name))
}

/// The canonical approval-token JSON for a resolved job + decision.
pub fn approval_text(resolved: &Resolved, approved_by: &str, decision: Decision) -> String {
    let token = build_token(&resolved.request, approved_by, decision);
    crate::approval::to_json(&token)
}

/// Write the `<name>.ax` + `<name>.axjob` pair (the compile output; no approval
/// yet). Returns the written paths.
pub fn write_compiled(
    resolved: &Resolved,
    intent: &Intent,
    out_dir: &Path,
    name: &str,
) -> std::io::Result<Triple> {
    std::fs::create_dir_all(out_dir)?;
    let program = out_dir.join(format!("{name}.ax"));
    let manifest = out_dir.join(format!("{name}.axjob"));
    std::fs::write(&program, &resolved.draft.source)?;
    std::fs::write(&manifest, manifest_text(resolved, intent, name))?;
    Ok(Triple {
        program,
        manifest,
        approval: None,
    })
}

/// Write the `<name>.approval` token alongside an already-emitted pair.
pub fn write_approval(
    resolved: &Resolved,
    out_dir: &Path,
    name: &str,
    approved_by: &str,
    decision: Decision,
) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(out_dir)?;
    let path = out_dir.join(format!("{name}.approval"));
    std::fs::write(&path, approval_text(resolved, approved_by, decision))?;
    Ok(path)
}

/// Build the `ApprovalToken` for a resolved job (no I/O) — used by callers that
/// want the token without writing it (e.g. the R21-handoff cross-spec test).
pub fn token_for(resolved: &Resolved, approved_by: &str, decision: Decision) -> ApprovalToken {
    build_token(&resolved.request, approved_by, decision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::intent;
    use crate::pipeline::{resolve, GenMode};
    use crate::synth::MockSynth;

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
    fn manifest_text_is_a_valid_r21_manifest() {
        let r = resolve(&it(), &MockSynth::new(), GenMode::Auto, None).unwrap();
        let text = manifest_text(&r, &it(), "summarize");
        // R21 must be able to parse what R22 emits (cross-crate handoff shape).
        let parsed = axon_os::manifest::parse(&text, Path::new("/jobs"))
            .expect("R21 parses the emitted .axjob");
        assert_eq!(parsed.program, Path::new("/jobs/summarize.ax"));
        assert_eq!(parsed.grant.fs_read, vec!["./data/".to_string()]);
        assert!(parsed.grant.net.is_empty());
    }

    #[test]
    fn emission_is_deterministic() {
        let r1 = resolve(&it(), &MockSynth::new(), GenMode::Auto, None).unwrap();
        let r2 = resolve(&it(), &MockSynth::new(), GenMode::Auto, None).unwrap();
        assert_eq!(
            manifest_text(&r1, &it(), "s"),
            manifest_text(&r2, &it(), "s")
        );
        assert_eq!(
            approval_text(&r1, "alice", Decision::Approved),
            approval_text(&r2, "alice", Decision::Approved)
        );
        assert_eq!(r1.draft.source, r2.draft.source);
    }
}
