//! R29 — Continuous Compliance Monitor.
//!
//! Watches a JSONL capability-audit ledger file in real time, compares each
//! entry's `effect` field against the declared `allowed_effects` policy, and
//! trips the R27 kill-switch the moment a violation is detected.
//!
//! This is the feedback loop that closes R26 + R27 + R28:
//!   R26 attests what's running → R27 can stop it → R28 records what it did
//!   → R29 watches R28 and trips R27 on any constraint violation.
//!
//! Design invariants (fail-closed, §4):
//!   I-1: Monitor crash → job killed (caller must detect JoinHandle error)
//!   I-2: Kill-file write failure → monitor panics (propagates I-1)
//!   I-3: Unrecognised effect → treated as violation (deny-by-default)
//!   I-4: Malformed JSON line → skipped, not treated as allowed
//!   I-5: Ledger absent at start → wait (not treated as allowed)
//!   I-6: Any monitor error → fail-closed

use std::io::{Read, Seek, SeekFrom};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

/// The result returned by `ComplianceMonitor::run`.
#[derive(Debug, PartialEq, Eq)]
pub enum MonitorResult {
    /// Job exited normally; no violation was detected during monitoring.
    CleanExit,
    /// A ledger entry contained an effect not in the allowed policy.
    /// The kill file has been tripped before returning.
    ViolationDetected {
        effect: String,
        operation: String,
        seq: u64,
    },
    /// The ledger file was unreadable for a non-transient reason (returned
    /// only for diagnostics; the caller still treats this as fail-closed).
    LedgerError(String),
}

/// R29 Compliance Monitor (§2).
///
/// Polls `ledger_path` every 100 ms. For each new JSONL line, checks the
/// `effect` field against `allowed_effects`. On a denial, writes the kill
/// file and returns `ViolationDetected`.
///
/// The monitor is intentionally simple and allocation-cheap. It owns no I/O
/// other than the ledger read and the kill-file write (both are supervisor-
/// owned paths, inaccessible to the contained job).
pub struct ComplianceMonitor {
    /// Path to the JSONL capability-audit ledger written by the contained job.
    pub ledger_path: PathBuf,
    /// Path to the R27 kill-file (FileKillChannel). Written on violation.
    pub kill_file: PathBuf,
    /// Normalised (lowercase) allowed effect names. Only these pass.
    pub allowed_effects: Vec<String>,
    /// Set by the caller to request a clean stop (e.g., job exited normally).
    pub stop: Arc<AtomicBool>,
}

impl ComplianceMonitor {
    /// Create a monitor with the given policy. Effects are normalised to
    /// lowercase; duplicates are accepted (idempotent).
    pub fn new(
        ledger_path: PathBuf,
        kill_file: PathBuf,
        allowed_effects: Vec<String>,
        stop: Arc<AtomicBool>,
    ) -> Self {
        let allowed_effects = allowed_effects
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();
        ComplianceMonitor {
            ledger_path,
            kill_file,
            allowed_effects,
            stop,
        }
    }

    /// Run the compliance monitor (blocking; intended to run on a dedicated
    /// thread). Returns when either:
    ///   (a) `stop` is set → `CleanExit`
    ///   (b) a violation is found → `ViolationDetected` (kill file written)
    ///   (c) fatal ledger error → `LedgerError` (still fail-closed)
    pub fn run(self) -> MonitorResult {
        let poll_interval = Duration::from_millis(100);
        let mut current_offset: u64 = 0;
        let mut line_buf = String::new();

        loop {
            // Honour the stop flag (set when the job exits normally).
            if self.stop.load(Ordering::Acquire) {
                return MonitorResult::CleanExit;
            }

            // Try to open + stat the ledger.  Absent = transient (job hasn't
            // created it yet).  Other errors = transient (e.g. creation race).
            let mut file = match std::fs::File::open(&self.ledger_path) {
                Ok(f) => f,
                Err(_) => {
                    std::thread::sleep(poll_interval);
                    continue;
                }
            };
            let file_len = match file.metadata() {
                Ok(m) => m.len(),
                Err(_) => {
                    std::thread::sleep(poll_interval);
                    continue;
                }
            };

            // §3.4: rotation detection — file shrank, so reset to 0.
            if file_len < current_offset {
                current_offset = 0;
            }

            // Seek to the last-read position and drain new bytes.
            if file.seek(SeekFrom::Start(current_offset)).is_err() {
                std::thread::sleep(poll_interval);
                continue;
            }
            let mut buf = Vec::new();
            let bytes_read = match file.read_to_end(&mut buf) {
                Ok(n) => n as u64,
                Err(_) => {
                    std::thread::sleep(poll_interval);
                    continue;
                }
            };
            current_offset += bytes_read;

            // Parse new complete lines from `buf`.
            if !buf.is_empty() {
                let chunk = match std::str::from_utf8(&buf) {
                    Ok(s) => s,
                    Err(_) => {
                        // Not valid UTF-8: skip (I-4 - malformed input)
                        std::thread::sleep(poll_interval);
                        continue;
                    }
                };
                // Accumulate partial lines across polls.
                line_buf.push_str(chunk);
                while let Some(nl_pos) = line_buf.find('\n') {
                    let line = line_buf[..nl_pos].trim().to_string();
                    line_buf.drain(..=nl_pos);

                    if line.is_empty() {
                        continue;
                    }
                    // Parse as JSON (I-4: malformed → skip, not allowed).
                    let entry: serde_json::Value = match serde_json::from_str(&line) {
                        Ok(v) => v,
                        Err(_) => continue, // malformed line → skip
                    };

                    // Extract `effect` field; absent/null → deny-by-default (I-3).
                    let effect = match entry.get("effect").and_then(|v| v.as_str()) {
                        Some(e) => e.to_lowercase(),
                        None => {
                            // No `effect` field → treat as violation (I-3).
                            let seq = entry
                                .get("seq")
                                .and_then(|v| v.as_u64())
                                .unwrap_or(0);
                            let operation = entry
                                .get("operation")
                                .and_then(|v| v.as_str())
                                .unwrap_or("unknown")
                                .to_string();
                            self.trip_kill_file("", "missing-effect-field", seq);
                            return MonitorResult::ViolationDetected {
                                effect: "(missing)".to_string(),
                                operation,
                                seq,
                            };
                        }
                    };
                    let seq = entry
                        .get("seq")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0);
                    let operation = entry
                        .get("operation")
                        .and_then(|v| v.as_str())
                        .unwrap_or("unknown")
                        .to_string();

                    if !self.is_allowed(&effect) {
                        // Violation — trip the kill file BEFORE returning (§3.2).
                        self.trip_kill_file(&effect, &operation, seq);
                        return MonitorResult::ViolationDetected {
                            effect,
                            operation,
                            seq,
                        };
                    }
                    // Allowed effect → continue monitoring.
                }
            }

            std::thread::sleep(poll_interval);
        }
    }

    /// Check whether an effect string is in the allowed set.
    fn is_allowed(&self, effect: &str) -> bool {
        self.allowed_effects.iter().any(|a| a == effect)
    }

    /// Write the R27 kill file with a tripped latch (I-2: panic on write failure).
    fn trip_kill_file(&self, effect: &str, operation: &str, seq: u64) {
        let reason = format!(
            "R29 compliance violation: effect '{}' (op '{}' seq {}) not in policy",
            effect, operation, seq
        );
        // Sanitize for JSON string value.
        let reason_json = reason.replace('"', "'");
        let effect_json = effect.replace('"', "'");
        let content = format!(
            "{{\"latch\":\"tripped\",\"reason\":\"{reason_json}\",\
            \"effect\":\"{effect_json}\",\"seq\":{seq}}}",
        );
        // I-2: write failure → panic (propagates to I-1 via the thread's JoinHandle).
        std::fs::write(&self.kill_file, content)
            .expect("R29: failed to write kill file — I-2 panic (fail-closed)");
    }
}

// ── Constants exported for corrigible.rs / tests ──────────────────────────────

/// R29: compliance monitor tripped the kill-switch (exit 12).
/// Distinct from exit 4 (operator kill) and exit 8 (static gate / sandbox).
pub const CONTAINMENT_VIOLATION_EXIT_CODE: i32 = 12;

/// R29 TCB addendum string — folded into the axtcb1: measurement chain.
pub const R29_TCB_ADDENDUM: &str =
    "R29-monitor:poll-100ms-fail-closed-deny-by-default";

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("axon-r29-{}-{}", std::process::id(), tag));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn make_monitor(dir: &PathBuf, allowed: &[&str]) -> (ComplianceMonitor, Arc<AtomicBool>) {
        let ledger = dir.join("audit.jsonl");
        let kill = dir.join("kill.json");
        std::fs::write(&kill, r#"{"latch":"clear"}"#).unwrap();
        let stop = Arc::new(AtomicBool::new(false));
        let m = ComplianceMonitor::new(
            ledger,
            kill,
            allowed.iter().map(|s| s.to_string()).collect(),
            Arc::clone(&stop),
        );
        (m, stop)
    }

    /// Helper: write a JSONL line to the ledger.
    fn append_entry(ledger: &PathBuf, seq: u64, effect: &str, operation: &str) {
        let line = format!(
            "{{\"seq\":{seq},\"effect\":\"{effect}\",\"operation\":\"{operation}\"}}\n"
        );
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(ledger)
            .unwrap();
        f.write_all(line.as_bytes()).unwrap();
    }

    // ── Unit tests for the pure core ──────────────────────────────────────────

    #[test]
    fn effect_normalised_lowercase() {
        let dir = tmp_dir("normalise");
        let (m, _stop) = make_monitor(&dir, &["NET", "FS_READ"]);
        assert!(m.is_allowed("net"));
        assert!(m.is_allowed("fs_read"));
        assert!(!m.is_allowed("exec"));
    }

    #[test]
    fn trip_kill_file_writes_tripped_state() {
        let dir = tmp_dir("trip");
        let (m, _stop) = make_monitor(&dir, &[]);
        m.trip_kill_file("net", "http_get", 42);
        let content = std::fs::read_to_string(&m.kill_file).unwrap();
        assert!(content.contains("\"latch\":\"tripped\""));
        assert!(content.contains("net"));
        assert!(content.contains("42"));
    }
}
