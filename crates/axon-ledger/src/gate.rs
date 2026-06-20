use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::Result;
use serde_json::json;

/// Result of running the brief gate against a session's extracted goal.
#[derive(Debug)]
pub struct GateOutcome {
    pub passed: bool,
    pub reason: String,
}

/// Locate the `axon` binary: check explicit path, then PATH, then common build dirs.
fn find_axon_bin(axon_bin: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = axon_bin {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    // Check PATH
    if let Ok(output) = Command::new("which").arg("axon").output() {
        if output.status.success() {
            let s = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    // Common cargo build locations
    for candidate in &["./target/debug/axon", "./target/release/axon"] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Locate the brief-gate.ax script: check explicit path, then common locations.
fn find_gate_script(gate_script: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = gate_script {
        if p.exists() {
            return Some(p.to_path_buf());
        }
    }
    for candidate in &[
        "./examples/goals/brief-gate.ax",
        "../examples/goals/brief-gate.ax",
    ] {
        let p = PathBuf::from(candidate);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

/// Build a brief JSON from the session goal and safe defaults for the other
/// required fields. The goal text comes from the first user message in the session.
fn build_brief(goal: &str) -> String {
    serde_json::to_string(&json!({
        "goal": goal,
        "constraints": "maintain correctness and security; do not ingest PII without scrubbing",
        "metric": "session ingested cleanly into provenance ledger"
    }))
    .unwrap_or_else(|_| r#"{"goal":"?","constraints":"?","metric":"?"}"#.to_string())
}

/// Run the brief gate against an extracted session goal.
///
/// - `goal`: first ~200 chars of the session's first user message.
/// - `session_id`: used only for error messages.
/// - `axon_bin`: optional explicit path to the `axon` binary.
/// - `gate_script`: optional explicit path to `brief-gate.ax`.
///
/// Returns `GateOutcome { passed: true }` if the gate passes OR if the
/// infrastructure (axon binary / gate script) is not available — soft-fail keeps
/// ingestion working in environments without a full Axon install.
/// Returns `GateOutcome { passed: false }` only when the gate is reachable and
/// explicitly rejects the brief.
pub fn run_brief_gate(
    goal: &str,
    session_id: &str,
    axon_bin: Option<&Path>,
    gate_script: Option<&Path>,
) -> Result<GateOutcome> {
    let brief = build_brief(goal);

    let Some(axon) = find_axon_bin(axon_bin) else {
        return Ok(GateOutcome {
            passed: true,
            reason: "axon binary not found — brief gate skipped (soft-pass)".into(),
        });
    };

    let Some(script) = find_gate_script(gate_script) else {
        return Ok(GateOutcome {
            passed: true,
            reason: "brief-gate.ax not found — brief gate skipped (soft-pass)".into(),
        });
    };

    let status = Command::new(&axon)
        .arg("run")
        .arg(&script)
        .env("BRIEF", &brief)
        .stderr(std::process::Stdio::null()) // suppress axon run-id noise
        .stdout(std::process::Stdio::null())
        .status();

    match status {
        Ok(s) if s.code() == Some(0) => Ok(GateOutcome {
            passed: true,
            reason: "brief gate passed".into(),
        }),
        Ok(s) => Ok(GateOutcome {
            passed: false,
            reason: format!(
                "brief gate rejected session {session_id}: goal text missing or too short \
                 (exit {})",
                s.code().unwrap_or(-1)
            ),
        }),
        Err(e) => Ok(GateOutcome {
            passed: true,
            reason: format!("brief gate error ({e}) — soft-pass"),
        }),
    }
}
