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
            axon_bin: absolutize(axon_bin),
            timeout: Duration::from_millis(ms),
        }
    }

    pub fn with_bin_and_timeout(axon_bin: PathBuf, timeout: Duration) -> Self {
        AxonCoreRuntime {
            axon_bin: absolutize(axon_bin),
            timeout,
        }
    }
}

/// Resolve a possibly-relative entrypoint to an absolute path NOW (against the
/// current dir), so a later `current_dir` change on the child can't break the
/// interpreter lookup (hermetic — no ambient PATH search at spawn time).
fn absolutize(p: PathBuf) -> PathBuf {
    if p.is_absolute() {
        return p;
    }
    std::fs::canonicalize(&p)
        .unwrap_or_else(|_| std::env::current_dir().map(|d| d.join(&p)).unwrap_or(p))
}

/// Outcome of a bounded subprocess: the exit code (None ⇒ killed by timeout).
struct ProcOutcome {
    code: Option<i32>,
    stderr: String,
    timed_out: bool,
    /// R27/R29: true if the process was killed because the kill file latch was
    /// tripped (by `axon-os kill` for R27, or by the R29 ComplianceMonitor).
    killed_by_latch: bool,
}

/// Run a command with a hard wall-clock timeout. On expiry the child is killed
/// and reaped (no leaked handle/zombie). Hermetic: a minimal explicit env.
/// stdout + stderr are captured (the interpreter prints fault diagnostics to
/// stderr, which is how we distinguish a FAULT exit from a program that simply
/// `return`s an integer — the two would otherwise collide on the exit code).
///
/// `kill_file`: if `Some(path)`, poll the file every 100 ms (R27/R29 kill-switch).
/// When the file contains `"latch":"tripped"`, SIGKILL the child immediately.
/// Returns `killed_by_latch = true` (→ `Verdict::Halted`, exit 4 for R27;
/// the R29 monitor overrides to exit 12 via `containment_violation` in cmd_run).
fn run_bounded(
    cmd: &mut Command,
    timeout: Duration,
    kill_file: Option<&std::path::Path>,
) -> std::io::Result<ProcOutcome> {
    let mut child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let start = std::time::Instant::now();
    let mut killed_by_latch = false;
    let code = loop {
        match child.try_wait()? {
            Some(status) => break Some(status.code().unwrap_or(-1)),
            None => {
                if start.elapsed() >= timeout {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                // R27/R29 poll-before-progress: check the kill file if set.
                if let Some(kf) = kill_file {
                    if is_kill_file_tripped(kf) {
                        killed_by_latch = true;
                        let _ = child.kill();
                        let _ = child.wait();
                        break Some(4); // HALTED_EXIT_CODE (R27); R29 overrides to 12 in cmd_run
                    }
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
    };

    let read_all = |s: Option<std::process::ChildStdout>| -> String {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut s) = s {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    };
    let read_err = |s: Option<std::process::ChildStderr>| -> String {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut s) = s {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    };
    let _ = read_all(stdout); // drained so the child isn't blocked on a full pipe
    let stderr = read_err(stderr);

    Ok(ProcOutcome {
        code,
        stderr,
        timed_out: code.is_none(),
        killed_by_latch,
    })
}

/// R27/R29: read the kill file and return true if the latch is tripped.
fn is_kill_file_tripped(path: &std::path::Path) -> bool {
    match std::fs::read_to_string(path) {
        Ok(s) => s.contains("\"latch\":\"tripped\"") || s.contains("\"latch\": \"tripped\""),
        Err(_) => false,
    }
}

/// Extract the first `axon:` FAULT line from stderr (the human-facing reason),
/// skipping the run-id stamp and informational lines (SMT discharge summaries,
/// the mint certificate-checked notice) that aren't faults; fall back to `default`.
fn first_axon_line(stderr: &str, default: &str) -> String {
    let is_info = |l: &str| {
        l.starts_with("axon: run-id") || l.starts_with("axon: SMT") || l.starts_with("axon: mint")
    };
    stderr
        .lines()
        .find(|l| l.starts_with("axon:") && !is_info(l))
        .map(|l| l.trim_start_matches("axon:").trim().to_string())
        .unwrap_or_else(|| default.to_string())
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

/// Wrap a user program so it runs inside a RUNTIME sandbox enforcing `ceiling`.
/// Renames the user's `fn main` to `__job_entry` and appends an orchestrator
/// `main` that mints a principal and `sandbox_run`s the entry under the effect
/// ceiling. The interpreter then enforces the AI/Net/IO ceiling at builtin
/// dispatch (SandboxViolation → exit 8) — sound against renaming/indirection,
/// unlike the static source scan. The grant axes map to the interpreter's
/// coarse sandbox tags: net→{Net,AI} (model calls are AI+Net); any of
/// fs_read/fs_write/exec→IO (the sandbox's single IO bucket — finer fs-vs-exec
/// distinctions are enforced only by the static gate, a documented coarseness).
fn wrap_in_sandbox(src: &str, ceiling: EffectSet, budget: &Budget) -> String {
    let mut tags: Vec<&str> = Vec::new();
    if ceiling.net {
        tags.push("Net");
        tags.push("AI");
    }
    if ceiling.fs_read || ceiling.fs_write || ceiling.exec {
        tags.push("IO");
    }
    let csv = tags.join(",");
    // `sandbox_run(sb, fn, arg)` calls `fn(arg)`, so the entry must take one i64.
    // Axon `main` is nullary — inject an unused i64 param when renaming.
    let renamed = src
        .replace("fn main()", "fn __job_entry(_axon_arg: i64)")
        .replace("fn main ()", "fn __job_entry(_axon_arg: i64)");
    format!(
        "{renamed}\n// \u{2500}\u{2500} axon-os runtime sandbox wrapper \u{2500}\u{2500}\nfn main() -> i64 {{\n    let __p = principal_root(\"job\", {net}, {fsw}, {exec}, {budget})\n    let __sb = sandbox_create(__p, \"{csv}\")\n    sandbox_run(__sb, \"__job_entry\", 0)\n}}\n",
        net = ceiling.net,
        fsw = ceiling.fs_write,
        exec = ceiling.exec,
        budget = budget.calls.max(0),
    )
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
        budget: &Budget,
        seed: u64,
    ) -> RunOutcome {
        // RUNTIME ENFORCEMENT (the sound fence). Rather than running the program
        // raw, wrap it: mint a principal + `sandbox_run` it inside an effect
        // ceiling. The interpreter then refuses ANY builtin whose effect row
        // (AI/Net/IO) exceeds the ceiling — SandboxViolation, exit 8 — at the
        // builtin-dispatch level, so an effect cannot be hidden by renaming or
        // indirection the way the static-gate source scan could be fooled. The
        // static gate is a best-effort PRE-check; THIS is what actually contains.
        let src = match std::fs::read_to_string(program) {
            Ok(s) => s,
            Err(e) => {
                return RunOutcome {
                    events: vec![],
                    verdict: Verdict::Denied {
                        reason: format!("cannot read program: {e}"),
                        axis: "io".into(),
                    },
                }
            }
        };
        let wrapper_src = wrap_in_sandbox(&src, ceiling, budget);
        let wrapper_path = std::env::temp_dir().join(format!(
            "axon-os-wrap-{}-{}.ax",
            std::process::id(),
            program
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("job")
        ));
        if std::fs::write(&wrapper_path, &wrapper_src).is_err() {
            return RunOutcome {
                events: vec![],
                verdict: Verdict::Denied {
                    reason: "cannot stage sandbox wrapper".into(),
                    axis: "io".into(),
                },
            };
        }

        let mut cmd = Command::new(&self.axon_bin);
        cmd.arg("run").arg(&wrapper_path);
        cmd.env_clear();
        cmd.env("AXON_SEED", seed.to_string());
        if let Some(p) = std::env::var_os("PATH") {
            cmd.env("PATH", p); // cc/linker discovery for the interpreter
        }
        // Relative paths in the program resolve against the job's directory, so
        // an example runs the same wherever it is invoked from (hermetic).
        if let Some(dir) = program.parent() {
            if !dir.as_os_str().is_empty() {
                cmd.current_dir(dir);
            }
        }

        // R27/R29: if AXON_KILL_FILE is set, poll it during run_bounded.
        // R27 uses this for operator kill (`axon-os kill`); R29 uses it for the
        // compliance monitor. Both write `{"latch":"tripped"}` to stop the job.
        let kill_file_env = std::env::var_os("AXON_KILL_FILE").map(std::path::PathBuf::from);
        let kill_file = kill_file_env.as_deref();
        let proc_res = run_bounded(&mut cmd, self.timeout, kill_file);
        let _ = std::fs::remove_file(&wrapper_path); // best-effort cleanup
        let proc = match proc_res {
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

        // Map the run to a verdict. Faults are detected by the interpreter's
        // STDERR diagnostics, NOT by the raw exit code — because `axon run`
        // propagates `main`'s integer return value as the exit code, which would
        // otherwise collide with the carved fault codes (a program returning 7
        // is NOT budget exhaustion). A clean run ⇒ Completed{value = exit code}.
        let err = &proc.stderr;
        let verdict = if proc.killed_by_latch {
            // Kill file tripped: R27 operator kill → Halted (exit 4).
            // R29 monitor kill → cmd_run overrides the final exit code to 12
            // via the `containment_violation` flag (checked after supervisor::run).
            Verdict::Halted { reason: "kill-switch tripped by supervisor".into() }
        } else if proc.timed_out {
            Verdict::Denied {
                reason: format!("timed out after {} ms", self.timeout.as_millis()),
                axis: "time".into(),
            }
        } else if err.contains("sandbox violation")
            || err.contains("SandboxViolation")
            || err.contains("not permitted by @[contained]")
            || err.contains("capability")
        {
            Verdict::Denied {
                reason: first_axon_line(err, "runtime capability/sandbox violation"),
                axis: "sandbox".into(),
            }
        } else if err.contains("budget") && (err.contains("exhaust") || err.contains("exceeded")) {
            Verdict::BudgetExhausted {
                axis: "budget".into(),
            }
        } else if err.contains("refinement violated") || err.contains("REFINE") {
            Verdict::RefineViolation {
                reason: first_axon_line(err, "refinement contract violated"),
            }
        } else if err.contains("axon: panic") {
            Verdict::Denied {
                reason: first_axon_line(err, "interpreter panic"),
                axis: "runtime".into(),
            }
        } else {
            // No fault diagnostic ⇒ a clean run; the exit code is main's return.
            Verdict::Completed {
                value: proc.code.unwrap_or(0) as i64,
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
        let out = run_bounded(&mut cmd, Duration::from_millis(150), None)
            .expect("spawn sleep (POSIX)");
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
        let out = run_bounded(&mut cmd, Duration::from_secs(5), None).expect("spawn sh");
        assert!(!out.timed_out);
        assert_eq!(out.code, Some(0));
    }

    #[test]
    fn run_bounded_trips_latch_via_kill_file() {
        // R27/R29: when the kill file is tripped mid-run, the subprocess is killed
        // with killed_by_latch=true.
        let dir = std::env::temp_dir().join(format!("axon-r27r29-rt-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let kill_path = dir.join("latch.json");
        // Start clear.
        std::fs::write(&kill_path, r#"{"latch":"clear"}"#).unwrap();
        // Run a long sleep.
        let mut cmd = Command::new("sleep");
        cmd.arg("10");
        let kill_path_clone = kill_path.clone();
        // Trip the latch from a background thread after 100ms.
        let _handle = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(100));
            let _ = std::fs::write(&kill_path_clone, r#"{"latch":"tripped","reason":"test"}"#);
        });
        let start = std::time::Instant::now();
        let out = run_bounded(&mut cmd, Duration::from_secs(10), Some(&kill_path)).unwrap();
        assert!(out.killed_by_latch, "must be killed by latch");
        assert_eq!(out.code, Some(4), "killed_by_latch returns code 4");
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "must return quickly after latch trip"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
