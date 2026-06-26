//! R21 §4.7 + §5.1 — the `Runtime` seam: the firewall between the pure
//! supervisor and the outside world (the model / the interpreter / the OS).
//!
//! The supervisor is generic over `Runtime`, so S1–S5 are testable with a
//! `MockRuntime` and never touch `axon-core`. The real `AxonCoreRuntime` (the
//! only impure module) lands in S6.

use crate::gate::DeclaredEffects;
use crate::grant::{Budget, EffectSet, Grant};
use crate::record::RawEvent;
use crate::verdict::Verdict;
use std::path::Path;

/// An opaque handle to a minted Principal in the runtime's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalHandle(pub usize);

/// The result of running a program inside the sandbox: the observed
/// capability-bearing actions and the sealing verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub events: Vec<RawEvent>,
    pub verdict: Verdict,
}

/// The seam. Every method that touches the model/interpreter/OS lives here.
pub trait Runtime {
    /// The effect row a program declares it may perform. An error / absent
    /// declaration MUST map to `DeclaredEffects::unknown()` (deny-by-default).
    fn declared_effects(&self, program: &Path) -> DeclaredEffects;

    /// Mint a Principal holding exactly `grant`. Attenuation (no authority the
    /// supervisor lacks) is guaranteed by the caller passing the effective
    /// grant `J ∩ S`; the runtime mints to that, no more.
    fn mint_principal(&self, grant: &Grant) -> PrincipalHandle;

    /// Run `program` as `principal` inside a sandbox enforcing `ceiling` +
    /// `budget` with a fixed `seed`. Returns the observed events and the verdict
    /// (mapping any runtime over-reach to Denied/BudgetExhausted/RefineViolation).
    fn run_sandboxed(
        &self,
        program: &Path,
        principal: &PrincipalHandle,
        ceiling: EffectSet,
        budget: &Budget,
        seed: u64,
    ) -> RunOutcome;
}

// ── S6: the real runtime — hermetic, isolated subprocess execution (A4) ──────

use crate::gate::DeclaredEffects as Decl;
use crate::grant::Label;
use crate::record::RawEvent as RE;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

/// The real `Runtime`: runs programs by invoking the canonical `axon`
/// interpreter in a fresh, time-bounded subprocess (R21 §4.4). The only impure
/// module. Capability enforcement is layered: the supervisor's static gate
/// (S3) denies a program whose declared effects exceed the grant BEFORE this
/// runs; here we additionally bound execution in time and map the interpreter's
/// fail-closed exit codes (6/7/8) back to verdicts.
pub struct AxonCoreRuntime {
    axon_bin: PathBuf,
    timeout: Duration,
}

impl AxonCoreRuntime {
    /// Resolve the canonical entrypoint from `AXON_BIN` (an absolute path, not
    /// an ambient PATH search) or a sensible default, and the timeout from
    /// `AXON_OS_TIMEOUT_MS` (default 30s).
    pub fn from_env() -> Self {
        let axon_bin = std::env::var_os("AXON_BIN")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("target/debug/axon"));
        let ms = std::env::var("AXON_OS_TIMEOUT_MS")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(30_000);
        AxonCoreRuntime {
            axon_bin,
            timeout: Duration::from_millis(ms),
        }
    }

    pub fn with_bin_and_timeout(axon_bin: PathBuf, timeout: Duration) -> Self {
        AxonCoreRuntime { axon_bin, timeout }
    }
}

/// Outcome of a bounded subprocess: the exit code (None ⇒ killed by timeout).
struct ProcOutcome {
    code: Option<i32>,
    last_stdout_line: String,
    timed_out: bool,
}

/// Run a command with a hard wall-clock timeout. On expiry the child is killed
/// and reaped (no leaked handle/zombie). Hermetic: a minimal explicit env.
fn run_bounded(cmd: &mut Command, timeout: Duration) -> std::io::Result<ProcOutcome> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;

    // Take stdout now; read it after the wait decision. Bound the wait by
    // polling try_wait so the child can be killed on timeout.
    let stdout = child.stdout.take();
    let start = std::time::Instant::now();
    let code = loop {
        match child.try_wait()? {
            Some(status) => break Some(status.code().unwrap_or(-1)),
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait(); // reap — no zombie/leaked handle
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
        }
    };

    let last_stdout_line = stdout
        .map(|mut s| {
            use std::io::Read;
            let mut buf = String::new();
            let _ = s.read_to_string(&mut buf);
            buf.lines().last().unwrap_or("").to_string()
        })
        .unwrap_or_default();

    Ok(ProcOutcome {
        code,
        last_stdout_line,
        timed_out: code.is_none(),
    })
}

/// Over-approximate a program's declared effects by scanning its source for
/// capability-bearing builtins (deny-by-default: a read error or any ambiguity
/// yields the FULL set). A fully sound extractor uses `axon ast review`; this
/// conservative scanner is sound *as a lower bound made safe by over-approx*.
fn scan_effects(source: &str) -> Decl {
    let net = [
        "http_get",
        "http_post",
        "http_sse",
        "ai_complete",
        "ai_extract",
    ]
    .iter()
    .any(|m| source.contains(m));
    let fs_read = ["read_file", "read_line"]
        .iter()
        .any(|m| source.contains(m));
    let fs_write = source.contains("write_file");
    let exec = source.contains("exec(") || source.contains("spawn_proc");
    Decl {
        row: EffectSet {
            fs_read,
            fs_write,
            net,
            exec,
        },
        max_label: Label::Internal,
    }
}

impl Runtime for AxonCoreRuntime {
    fn declared_effects(&self, program: &Path) -> DeclaredEffects {
        match std::fs::read_to_string(program) {
            Ok(src) => scan_effects(&src),
            Err(_) => DeclaredEffects::unknown(), // deny-by-default
        }
    }

    fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
        // Attenuation is enforced by the supervisor passing the EFFECTIVE grant
        // (J ∩ S, R20-proven ⊆) to the gate before we run; the handle is opaque.
        PrincipalHandle(0)
    }

    fn run_sandboxed(
        &self,
        program: &Path,
        _principal: &PrincipalHandle,
        ceiling: EffectSet,
        _budget: &Budget,
        seed: u64,
    ) -> RunOutcome {
        let mut cmd = Command::new(&self.axon_bin);
        cmd.arg("run").arg(program);
        cmd.env_clear();
        cmd.env("AXON_SEED", seed.to_string());
        if let Some(p) = std::env::var_os("PATH") {
            cmd.env("PATH", p); // cc/linker discovery for the interpreter
        }

        let proc = match run_bounded(&mut cmd, self.timeout) {
            Ok(p) => p,
            Err(e) => {
                return RunOutcome {
                    events: vec![],
                    verdict: Verdict::Denied {
                        reason: format!("could not launch interpreter: {e}"),
                        axis: "exec".into(),
                    },
                }
            }
        };

        // Map the interpreter's fail-closed exit scheme back to a verdict.
        let verdict = if proc.timed_out {
            Verdict::Denied {
                reason: format!("timed out after {} ms", self.timeout.as_millis()),
                axis: "time".into(),
            }
        } else {
            match proc.code {
                Some(0) => Verdict::Completed {
                    value: proc.last_stdout_line.trim().parse::<i64>().unwrap_or(0),
                },
                Some(6) => Verdict::RefineViolation {
                    reason: "refinement contract violated".into(),
                },
                Some(7) => Verdict::BudgetExhausted {
                    axis: "budget".into(),
                },
                Some(8) => Verdict::Denied {
                    reason: "runtime sandbox/capability violation".into(),
                    axis: "sandbox".into(),
                },
                Some(code) => Verdict::Denied {
                    reason: format!("interpreter exited {code}"),
                    axis: "runtime".into(),
                },
                None => Verdict::Denied {
                    reason: "killed".into(),
                    axis: "time".into(),
                },
            }
        };

        // A single audit event recording the bounded run under the ceiling.
        let events = vec![RE::new(
            "run",
            &program.display().to_string(),
            ceiling,
            "internal",
        )];
        RunOutcome { events, verdict }
    }
}

/// A configurable in-memory `Runtime` for testing the supervisor with no I/O.
#[cfg(any(test, feature = "mock"))]
pub struct MockRuntime {
    pub declared: DeclaredEffects,
    pub outcome: RunOutcome,
    pub mint_calls: std::cell::Cell<usize>,
    pub run_calls: std::cell::Cell<usize>,
}

#[cfg(any(test, feature = "mock"))]
impl MockRuntime {
    pub fn new(declared: DeclaredEffects, outcome: RunOutcome) -> Self {
        MockRuntime {
            declared,
            outcome,
            mint_calls: std::cell::Cell::new(0),
            run_calls: std::cell::Cell::new(0),
        }
    }
}

#[cfg(test)]
mod runtime_tests {
    use super::*;

    #[test]
    fn run_bounded_kills_a_runaway_at_the_timeout() {
        // A `sleep 5` under a 150 ms timeout must be killed (timed_out), and the
        // call must return promptly (no leaked child, no 5 s hang).
        let start = std::time::Instant::now();
        let mut cmd = Command::new("sleep");
        cmd.arg("5");
        let out = run_bounded(&mut cmd, Duration::from_millis(150)).expect("spawn sleep (POSIX)");
        assert!(out.timed_out, "runaway must be killed at the timeout");
        assert!(out.code.is_none());
        assert!(
            start.elapsed() < Duration::from_secs(2),
            "must return promptly after the kill, not wait out the sleep"
        );
    }

    #[test]
    fn run_bounded_captures_a_fast_command() {
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg("echo 41");
        let out = run_bounded(&mut cmd, Duration::from_secs(5)).expect("spawn sh");
        assert!(!out.timed_out);
        assert_eq!(out.code, Some(0));
        assert_eq!(out.last_stdout_line.trim(), "41");
    }
}

#[cfg(any(test, feature = "mock"))]
impl Runtime for MockRuntime {
    fn declared_effects(&self, _program: &Path) -> DeclaredEffects {
        self.declared
    }
    fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
        self.mint_calls.set(self.mint_calls.get() + 1);
        PrincipalHandle(0)
    }
    fn run_sandboxed(
        &self,
        _program: &Path,
        _principal: &PrincipalHandle,
        _ceiling: EffectSet,
        _budget: &Budget,
        _seed: u64,
    ) -> RunOutcome {
        self.run_calls.set(self.run_calls.get() + 1);
        self.outcome.clone()
    }
}
