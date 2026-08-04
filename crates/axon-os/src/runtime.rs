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
    /// AUDIT T3: takes the full `Grant`, not just its induced `EffectSet`.
    /// `effect_set()` reduces the grant to four booleans, discarding the path
    /// prefixes and host allowlists — so the ceiling could only ever express
    /// "may write SOMEWHERE", never "may write ./out/". The scoped runtime
    /// needs the allowlists themselves.
    fn run_sandboxed(
        &self,
        program: &Path,
        principal: &PrincipalHandle,
        grant: &Grant,
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
    /// AUDIT T44: stdout was drained (T25) but DISCARDED. It carries the
    /// wrapper's result sentinel, which is what lets the verdict be decided by
    /// the exit code instead of by matching prose in stderr.
    stdout: String,
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

    let stdout_pipe = child.stdout.take();
    let stderr_pipe = child.stderr.take();

    // AUDIT T25 (finding OSK-P4-H3 / O019). These handles were taken here but
    // not READ until after the try_wait/timeout loop below, so a child emitting
    // more than a pipe buffer (~64 KiB) blocked writing while the parent blocked
    // waiting for it to exit — a deadlock broken only by the wall-clock timeout.
    // Observed: a 20,000-line program that runs in 0.09s standalone was sealed
    // as `Denied{axis:"time"}` after 30s, blaming the job for being slow when it
    // was fast.
    //
    // Drain both pipes on their own threads, concurrently with the wait. The
    // timeout and kill-file polling below are untouched; only the reads move.
    let out_h = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut s) = stdout_pipe {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
    let err_h = std::thread::spawn(move || {
        use std::io::Read;
        let mut buf = String::new();
        if let Some(mut s) = stderr_pipe {
            let _ = s.read_to_string(&mut buf);
        }
        buf
    });
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

    // Join the drainers: both pipes are at EOF once the child has exited.
    let stdout = out_h.join().unwrap_or_default(); // drained concurrently (T25)
    let stderr = err_h.join().unwrap_or_default();

    Ok(ProcOutcome {
        code,
        stdout,
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
/// Did the sandbox wrapper run to completion? (AUDIT T44.) True only if stdout
/// carries the exact marker for THIS run — the nonce makes it unforgeable by the
/// job, which never sees the generated wrapper.
fn ran_to_completion(stdout: &str, nonce: &str) -> bool {
    let expect = format!("{DONE_SENTINEL}{nonce}");
    stdout.lines().any(|l| l.trim() == expect)
}

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
    // AUDIT T4 (finding OSK-P4-H1). The previous implementation tested
    // `source.contains("exec(")`, which `exec (name, args)` — one space — parses
    // identically and evades. That is an UNDER-approximation, the opposite of
    // this function's documented deny-by-default contract, and it mattered
    // because runtime collapsed fs and exec into one bucket, making this scan
    // the sole exec control. (T8 has since given exec its own runtime effect, so
    // this is no longer load-bearing — but a gate whose result is shown to a
    // human as an approval assertion must not state falsehoods either way.)
    //
    // A module import means the effects live in a file we are not reading, so
    // the honest answer is the full set, not "none found here".
    if calls_name(source, "mod") || source.contains("\nmod ") || source.starts_with("mod ") {
        return Decl {
            row: EffectSet {
                fs_read: true,
                fs_write: true,
                net: true,
                exec: true,
            },
            max_label: Label::Internal,
        };
    }
    let any = |names: &[&str]| names.iter().any(|m| calls_name(source, m));
    let net = any(&[
        "http_get",
        "http_post",
        "http_sse",
        "http_sse_post",
        "ai_complete",
        "ai_extract_i64",
        "ai_extract_f64",
        "ai_extract_str",
        "ai_extract_bool",
        "ai_extract_uncertain_i64",
        "ai_extract_uncertain_f64",
        // The goal_run family re-calls @[adaptive] metrics, which may ai_complete.
        "goal_run",
        "goal_run_constrained",
        "goal_run_categorical",
        "goal_run_random",
        "goal_run_multistart",
        "goal_continue",
        "goal_eval",
    ]);
    let fs_read = any(&["read_file", "read_line", "env_var"]);
    let fs_write = any(&["write_file"]);
    let exec = any(&["exec", "spawn_proc"]);
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

/// True if `source` appears to CALL `name` — the identifier on a word boundary,
/// followed by optional whitespace and `(`. Deliberately over-approximates:
/// occurrences inside comments or string literals count, because a false
/// positive only narrows the grant (safe) while a false negative widens it.
fn calls_name(source: &str, name: &str) -> bool {
    let bytes = source.as_bytes();
    let is_word = |b: u8| b.is_ascii_alphanumeric() || b == b'_';
    let mut from = 0usize;
    while let Some(rel) = source[from..].find(name) {
        let start = from + rel;
        let end = start + name.len();
        from = start + 1;
        // Must not be a suffix of a longer identifier (e.g. `my_exec`),
        // and must not be a prefix of one (e.g. `exec_report`).
        if start > 0 && is_word(bytes[start - 1]) {
            continue;
        }
        let mut i = end;
        if i < bytes.len() && is_word(bytes[i]) {
            continue;
        }
        // `mod foo` is a declaration, not a call — accept a bare keyword.
        if name == "mod" {
            return true;
        }
        while i < bytes.len() && (bytes[i] as char).is_whitespace() {
            i += 1;
        }
        if i < bytes.len() && bytes[i] == b'(' {
            return true;
        }
    }
    false
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
///
/// AUDIT T8: that coarseness was not merely imprecise, it was unsound for `exec`.
/// Collapsing exec into the same `IO` bucket as fs meant any job granted
/// `fs_read` or `fs_write` was granted arbitrary process spawn at runtime, and
/// the ONLY thing separating `exec: "none"` from spawn was the static source
/// scan — which a single space (`exec (`) defeats. The interpreter now requires
/// an explicit `Exec` tag for process spawning, so exec is emitted here only
/// when the grant actually carries it.
/// Prefix of the completion marker the sandbox wrapper prints (AUDIT T44).
/// Carries a per-run nonce, so a job cannot forge it: the nonce lives only in
/// the generated wrapper, which is staged outside any granted fs_read prefix.
const DONE_SENTINEL: &str = "__axon_os_done:";

/// A per-run nonce for the completion marker. Not derived from the job's seed —
/// the job can read `AXON_SEED` from its own environment, so a seed-derived
/// marker would be forgeable by the very code whose completion it attests.
fn done_nonce() -> String {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{:x}{:x}", t, std::process::id())
}

fn wrap_in_sandbox(src: &str, grant: &Grant, budget: &Budget, nonce: &str) -> String {
    let ceiling = grant.effect_set();
    let mut tags: Vec<&str> = Vec::new();
    if ceiling.net {
        tags.push("Net");
        tags.push("AI");
    }
    if ceiling.fs_read || ceiling.fs_write || ceiling.exec {
        tags.push("IO");
    }
    if ceiling.exec {
        tags.push("Exec");
    }
    let csv = tags.join(",");
    // AUDIT T3: pass the ALLOWLISTS through, not just the booleans they induce.
    // `axon-os explain` renders "This program MAY: write ./out/" to the human
    // approving the run; before this the prefixes were dropped here and the
    // runtime enforced only "may write somewhere", so that sentence was false.
    let esc = |v: &[String]| {
        v.iter()
            .map(|x| x.replace('\\', "\\\\").replace('"', "\\\""))
            .collect::<Vec<_>>()
            .join(",")
    };
    let fs_read_csv = esc(&grant.fs_read);
    let fs_write_csv = esc(&grant.fs_write);
    let net_csv = esc(&grant.net);
    // `sandbox_run(sb, fn, arg)` calls `fn(arg)`, so the entry must take one i64.
    // Axon `main` is nullary — inject an unused i64 param when renaming.
    let renamed = src
        .replace("fn main()", "fn __job_entry(_axon_arg: i64)")
        .replace("fn main ()", "fn __job_entry(_axon_arg: i64)");
    // AUDIT T44 (OSK-P4-H2 / P4-OS-21). `main` returns the job's value and
    // `axon run` propagates it as the PROCESS EXIT CODE, so a job returning 8
    // was indistinguishable from a sandbox violation. That collision is why the
    // verdict was inferred from stderr PROSE — and the prose drifted (see the
    // classifier). Emit a completion MARKER after the job returns instead: its
    // presence says "the job ran to completion", so the exit code can then be
    // read as main's return value, and its ABSENCE says the run stopped short,
    // so the exit code is a fault. Same shape as the T33 guest-kernel sentinel.
    //
    // The marker carries the job's value only implicitly (via the exit code) —
    // printing `to_str(__v)` would panic for the common `fn main()` job, whose
    // renamed entry returns unit.
    format!(
        "{renamed}\n// \u{2500}\u{2500} axon-os runtime sandbox wrapper \u{2500}\u{2500}\nfn main() -> i64 {{\n    let __p = principal_root(\"job\", {net}, {fsw}, {exec}, {budget})\n    let __sb = sandbox_create_scoped(__p, \"{csv}\", \"{fsr_l}\", \"{fsw_l}\", \"{net_l}\")\n    let __r = sandbox_run(__sb, \"__job_entry\", 0)\n    println(\"{sentinel}{nonce}\")\n    __r\n}}\n",
        sentinel = DONE_SENTINEL,
        nonce = nonce,
        net = ceiling.net,
        fsw = ceiling.fs_write,
        exec = ceiling.exec,
        budget = budget.calls.max(0),
        fsr_l = fs_read_csv,
        fsw_l = fs_write_csv,
        net_l = net_csv,
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
        grant: &Grant,
        budget: &Budget,
        seed: u64,
    ) -> RunOutcome {
        let ceiling = grant.effect_set();
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
        let nonce = done_nonce();
        let wrapper_src = wrap_in_sandbox(&src, grant, budget, &nonce);
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
        // AUDIT T23 (findings OSK-P4-H4 / O017 / O018). `env_clear` is correct
        // and stays — but the allowlist omitted every AXON_* control an operator
        // legitimately sets, so they were silently dropped:
        //
        //   AXON_AUDIT_LEDGER — the R28 capability-audit ledger was NEVER
        //     written for a job run under the supervisor: exactly the execution
        //     path where an operator most wants that trail.
        //   AXON_AI_MOCK / AXON_AI_REPLAY — the deterministic stub and the
        //     record/replay cache were inert here, so a job fell through to the
        //     live path (observed: it then hit the AI-policy refusal, exit 5),
        //     making replay-reproducibility non-functional through axon-os,
        //     which is where it is meant to hold.
        //
        // Silent in every case: the operator sets the variable, the run
        // succeeds, and the artifact simply never appears.
        for key in [
            "AXON_AUDIT_LEDGER",
            "AXON_AI_MOCK",
            "AXON_AI_REPLAY",
            "AXON_PATH",
            "AXON_MAX_DEPTH",
        ] {
            if let Some(v) = std::env::var_os(key) {
                cmd.env(key, v);
            }
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
            Verdict::Halted {
                reason: "kill-switch tripped by supervisor".into(),
            }
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
        } else if err.contains("ai policy") || err.contains("E1300") {
            // AUDIT T24 (O016/O018): observed sealing as Completed{value:5}.
            // Exit 5 is AI_POLICY_EXIT_CODE — a CARVED fault code — so the
            // record stored a fault code in the `value` field of a SUCCESS
            // verdict, and a reader could not tell "returned 5" from "refused
            // by AI policy".
            Verdict::Denied {
                reason: first_axon_line(err, "AI policy refused the call"),
                axis: "ai-policy".into(),
            }
        } else if err.contains("parse error") || err.contains("type error") {
            // AUDIT T24 (O016): a program that does not compile executes ZERO
            // statements, yet sealed as `✓ completed (value=2)` because the
            // interpreter reports it as `error: parse error: ...` — no `axon:`
            // fault line, so none of the arms above matched and it fell through
            // to Completed. The tamper-evident record then attested success for
            // a job that never ran.
            Verdict::Malformed {
                reason: first_axon_line(err, "program failed to compile"),
            }
        } else {
            // AUDIT T44 (OSK-P4-H2 / P4-OS-21). This arm used to be
            // `Completed { value: exit_code }` unconditionally, so ANY fault
            // whose wording the chain above did not match sealed as a success.
            // That is not hypothetical drift — it was live. T24 added the
            // `parse error`/`type error` arm above, but `axon run` reports TYPE
            // errors as JSON diagnostics (`{"schema":"axon-diag/1",…}`) and only
            // SYNTAX errors as prose, so:
            //
            //   syntax error in a job -> "⚠ DENIED: program failed to compile", exit 8
            //   type   error in a job -> "✓ completed (value=2)",                exit 0
            //
            // Same class of job, opposite records, and the wrong one is the
            // silent one. A record that attests success for a job which executed
            // zero statements is an attestation-integrity failure — the record
            // lies, and it is hash-chained, so it lies durably.
            //
            // The exit code now decides. It is unambiguous because the wrapper
            // returns 0 and reports the job's value via RESULT_SENTINEL
            // (see `wrap_in_sandbox`) — the collision that forced the stderr
            // heuristic in the first place is gone. stderr is still read, but
            // only to phrase the human-readable `reason`.
            if ran_to_completion(&proc.stdout, &nonce) {
                // The wrapper's tail ran, so the job returned normally and the
                // exit code IS its return value — including values that collide
                // with carved fault codes, which is exactly what the old stderr
                // heuristic could not express.
                Verdict::Completed {
                    value: proc.code.unwrap_or(0) as i64,
                }
            } else {
                match proc.code.unwrap_or(-1) {
                    // Exit 0 with no completion marker means the wrapper's own tail
                    // never ran. Something stopped the job short. Refusing to seal
                    // that as a success is the whole point of this arm.
                    0 => Verdict::Denied {
                        reason: first_axon_line(
                            err,
                            "run produced no completion marker — the sandbox wrapper did not \
                         finish, so the job's outcome is unknown",
                        ),
                        axis: "runtime".into(),
                    },
                    2 => Verdict::Malformed {
                        reason: first_axon_line(err, "program failed to compile"),
                    },
                    3 => Verdict::Denied {
                        reason: first_axon_line(err, "@[verify] postcondition failed"),
                        axis: "verify".into(),
                    },
                    4 => Verdict::Halted {
                        reason: first_axon_line(err, "corrigibility kill-switch tripped"),
                    },
                    5 => Verdict::Denied {
                        reason: first_axon_line(err, "AI policy refused the call"),
                        axis: "ai-policy".into(),
                    },
                    6 => Verdict::RefineViolation {
                        reason: first_axon_line(err, "refinement contract violated"),
                    },
                    7 => Verdict::BudgetExhausted {
                        axis: "budget".into(),
                    },
                    8 => Verdict::Denied {
                        reason: first_axon_line(err, "runtime capability/sandbox violation"),
                        axis: "sandbox".into(),
                    },
                    // Any other non-zero exit is a fault we have no name for. It is
                    // NOT a completion. Naming it honestly beats sealing a lie.
                    other => Verdict::Denied {
                        reason: first_axon_line(
                            err,
                            &format!("interpreter exited {other} with no recognised diagnostic"),
                        ),
                        axis: "runtime".into(),
                    },
                }
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
        _grant: &Grant,
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
        let out =
            run_bounded(&mut cmd, Duration::from_millis(150), None).expect("spawn sleep (POSIX)");
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

    #[test]
    fn scan_effects_is_not_evaded_by_whitespace_or_module_imports() {
        // AUDIT T4 (OSK-P4-H1). `source.contains("exec(")` was defeated by a
        // single space, so a job declaring exec:"none" scanned clean while
        // calling exec. The scan's result is rendered to a human as an approval
        // assertion, so an under-approximation makes the UI state a falsehood.
        assert!(
            scan_effects("fn f() { exec(\"id\", []) }").row.exec,
            "baseline: exec( must be detected"
        );
        for evasion in [
            "fn f() { exec (\"id\", []) }",
            "fn f() { exec\t(\"id\", []) }",
            "fn f() { exec\n        (\"id\", []) }",
        ] {
            assert!(
                scan_effects(evasion).row.exec,
                "whitespace before `(` must not evade the exec scan: {evasion}"
            );
        }
        // Word boundaries: a longer identifier must NOT trip the exec axis.
        assert!(
            !scan_effects("fn f() { my_exec(1) }").row.exec,
            "`my_exec` is not `exec`"
        );
        assert!(
            !scan_effects("fn f() { exec_report(1) }").row.exec,
            "`exec_report` is not `exec`"
        );
        // A module import puts effects in a file this scanner never reads, so
        // the honest answer is the full set, not "nothing found here".
        let m = scan_effects("mod util\nfn main() { util.go() }").row;
        assert!(
            m.exec && m.net && m.fs_read && m.fs_write,
            "a mod import must deny-by-default to the full effect set"
        );
        // Previously-invisible capability builtins are now classified.
        assert!(scan_effects("fn f() { env_var(\"X\") }").row.fs_read);
        assert!(scan_effects("fn f() { goal_run(\"g\", 1) }").row.net);
        let sse = scan_effects("fn f() { http_sse_post(\"u\", \"b\") }").row;
        assert!(sse.net);
        // A pure program stays pure.
        let pure = scan_effects("fn add(a: i64, b: i64) -> i64 { a + b }").row;
        assert!(!pure.exec && !pure.net && !pure.fs_read && !pure.fs_write);
    }
}
