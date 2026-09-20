//! R21 §5.2 — the `axon-os` command surface: explain / run / verify / replay.
//! Thin I/O shell over the pure core + the `Runtime`. Output is human-legible
//! (not just exit codes); every subcommand has `--help`.

use crate::gate::{admit, Admission};
use crate::grant::{Budget, ExecPolicy, Grant, Label};
use crate::manifest::{parse as parse_manifest, JobManifest};
use crate::monitor::{ComplianceMonitor, MonitorResult, CONTAINMENT_VIOLATION_EXIT_CODE};
use crate::record::{from_json, to_json, verify};
use crate::runtime::{AxonCoreRuntime, Runtime};
use crate::{replay, supervisor};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};

const USAGE: &str = "\
axon-os — run untrusted Axon programs under a proven capability bound

USAGE:
    axon-os explain <job.axjob>
    axon-os run     <job.axjob> [--run-id ID] [--out DIR]
                    [--killable] [--coalition ROOT]
                    [--monitor <effects>] [--ledger <path>]
    axon-os verify  <record.json>
    axon-os replay  <run-id> [--store DIR]
    axon-os kill    <run-id> [--store DIR] [--reason REASON]    (R27)
    axon-os status  [--store DIR] [--latest] [--json]           (R27)
    axon-os audit verify --ledger PATH           (R28)
    axon-os audit show   --ledger PATH [--json]  (R28)

Every subcommand accepts --help. Exit codes: 0 ok, 2 usage/malformed,
4 halted (kill-switch), 6 refine, 7 budget, 8 capability/denied,
9 resource-bound, 10 coalition-bound, 11 tamper/divergence,
12 containment-violation (R29 monitor).

R29 flags for `run`:
  --monitor <effects>   Comma-separated allowed effects (e.g. fs_read,fs_write).
                        Enables the continuous compliance monitor (R29).
  --ledger  <path>      Path to the JSONL capability audit ledger to watch.";

/// The supervisor's own authority. Default is broad (the manifest is the bound);
/// `"".into()` is the universal path ancestor and `"*"` the universal host.
fn broad_supervisor_grant() -> Grant {
    Grant {
        reproducible: false,
        // The supervisor's own authority is not a reproducibility posture.
        fs_read: vec!["".into()],
        fs_write: vec!["".into()],
        net: vec!["*".into()],
        exec: ExecPolicy::Any,
        max_label: Label::Secret,
        budget: Budget {
            calls: i64::MAX,
            tokens: i64::MAX,
            cost_micro: i64::MAX,
        },
    }
}

/// The legible, quantified grant rendering (R21 §4.4 / §5.2) — the human-facing
/// "may / may NOT / budget / ceiling" block.
fn legible_grant(g: &Grant) -> String {
    let mut may = Vec::new();
    let mut not = Vec::new();
    if g.fs_read.is_empty() {
        not.push("read files".to_string());
    } else {
        may.push(format!("read {}", g.fs_read.join(", ")));
    }
    if g.fs_write.is_empty() {
        not.push("write files".to_string());
    } else {
        may.push(format!("write {}", g.fs_write.join(", ")));
    }
    if g.net.is_empty() {
        not.push("use the network".to_string());
    } else {
        may.push(format!("reach {}", g.net.join(", ")));
    }
    if matches!(g.exec, ExecPolicy::Any) {
        may.push("spawn processes".to_string());
    } else {
        not.push("spawn processes".to_string());
    }
    format!(
        "  This program MAY: {}\n  It may NOT: {}\n  Budget: \u{2264} {} calls / {} tokens / {} \u{b5}$\n  Confidentiality ceiling: {}",
        if may.is_empty() { "(nothing)".into() } else { may.join("; ") },
        if not.is_empty() { "(no restrictions)".into() } else { not.join(", ") },
        g.budget.calls,
        g.budget.tokens,
        g.budget.cost_micro,
        g.max_label.as_str(),
    )
}

fn read_manifest(path: &Path) -> Result<JobManifest, String> {
    let src = std::fs::read_to_string(path)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    let base = path.parent().unwrap_or(Path::new("."));
    let mut m = parse_manifest(&src, base).map_err(|v| v.legible())?;
    // Absolutize the program path so the run event + saved manifest + replay
    // all reference the SAME path regardless of cwd (deterministic replay).
    m.program = std::fs::canonicalize(&m.program).unwrap_or_else(|_| {
        std::env::current_dir()
            .map(|d| d.join(&m.program))
            .unwrap_or(m.program.clone())
    });
    Ok(m)
}

/// Entry point. Returns the process exit code.
pub fn run(args: Vec<String>) -> ExitCode {
    let a: Vec<&str> = args.iter().skip(1).map(|s| s.as_str()).collect();
    match a.as_slice() {
        [] | ["--help"] | ["-h"] | ["help"] => {
            println!("{USAGE}");
            ExitCode::from(0)
        }
        ["explain", "--help"] => {
            help("explain <job.axjob>  — show the legible grant + gate verdict; no execution")
        }
        ["run", "--help"] => help(
            "run <job.axjob> [--run-id ID] [--out DIR] [--killable] [--monitor EFFECTS] [--ledger PATH]  — gate→run→record"
        ),
        ["verify", "--help"] => {
            help("verify <record.json>  — recompute the hash chain; detect tamper")
        }
        ["replay", "--help"] => {
            help("replay <run-id> [--store DIR]  — verify + re-run + assert identical")
        }
        ["kill", "--help"] => {
            help("kill <run-id> [--store DIR] [--reason REASON]  — R27: trip the supervisor kill-latch (exit 0)")
        }
        ["status", "--help"] => {
            help("status [--store DIR] [--latest] [--json]  — R27: show latch state + ledger totals")
        }
        ["audit", "--help"] | ["audit"] => {
            help("audit verify --ledger PATH  |  audit show --ledger PATH [--json]  — R28 capability ledger")
        }
        ["explain", job] => cmd_explain(Path::new(job)),
        ["run", rest @ ..] => cmd_run(rest),
        ["verify", record] => cmd_verify(Path::new(record)),
        ["replay", rest @ ..] => cmd_replay(rest),
        ["kill", rest @ ..] => cmd_kill(rest),
        ["status", rest @ ..] => cmd_status(rest),
        ["audit", rest @ ..] => cmd_audit(rest),
        _ => {
            eprintln!("axon-os: unrecognized invocation\n\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

fn help(line: &str) -> ExitCode {
    println!("axon-os {line}");
    ExitCode::from(0)
}

fn cmd_explain(job: &Path) -> ExitCode {
    let manifest = match read_manifest(job) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = AxonCoreRuntime::from_env();
    let declared = rt.declared_effects(&manifest.program);
    let eff = manifest.grant.intersect(&broad_supervisor_grant());
    println!("Intent: {}", manifest.intent);
    println!("{}", legible_grant(&eff));
    match admit(&declared, &eff) {
        Admission::Admit => {
            println!("  Gate: \u{2713} ADMIT (declared effects are within the grant)");
            ExitCode::from(0)
        }
        Admission::Deny { reason, axis } => {
            println!("  Gate: \u{26a0} DENY ({reason}) [axis: {axis}]");
            ExitCode::from(8)
        }
    }
}

fn cmd_run(rest: &[&str]) -> ExitCode {
    let mut job: Option<&str> = None;
    let mut run_id = "run".to_string();
    let mut out = PathBuf::from(".");
    // R27: --killable enables manual kill via `axon-os kill`.
    let mut killable = false;
    let mut _coalition: Option<String> = None; // R27: coalition root (future use)
                                               // R29: --monitor enables the continuous compliance monitor.
    let mut monitor_effects: Option<String> = None; // comma-sep allowed effects
    let mut monitor_ledger: Option<PathBuf> = None; // JSONL ledger to watch
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--run-id" if i + 1 < rest.len() => {
                run_id = rest[i + 1].to_string();
                i += 2;
            }
            "--out" if i + 1 < rest.len() => {
                out = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            "--killable" => {
                killable = true;
                i += 1;
            }
            "--coalition" if i + 1 < rest.len() => {
                _coalition = Some(rest[i + 1].to_string());
                i += 2;
            }
            "--monitor" if i + 1 < rest.len() => {
                monitor_effects = Some(rest[i + 1].to_string());
                i += 2;
            }
            "--ledger" if i + 1 < rest.len() => {
                monitor_ledger = Some(PathBuf::from(rest[i + 1]));
                i += 2;
            }
            s if !s.starts_with("--") && job.is_none() => {
                job = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-os run: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(job) = job else {
        eprintln!("axon-os run: missing <job.axjob>");
        return ExitCode::from(2);
    };
    let job_path = Path::new(job);
    let manifest = match read_manifest(job_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os: {e}");
            return ExitCode::from(2);
        }
    };
    // R22 handoff: if an approval token sits next to the manifest, it MUST
    // verify (the program + grant unedited since approval) or the run is refused
    // BEFORE any execution — fail closed (exit 8).
    // Two INDEPENDENT facts, never inferred from each other (triage OSK-P4-H8):
    //   * `manifest.require_approval` — JOB POLICY: must this job be signed off?
    //   * the token on disk           — RUNTIME EVIDENCE: was it?
    //
    //   not required + no token         → run   ("Approval: not required")
    //   required     + no/invalid token → exit 8
    //   required     + valid token      → run   ("Approval: required and verified")
    //
    // Absent policy means NOT required, so existing jobs keep working; an
    // operator who wants the gate says so with `require_approval = true`.
    let approval_path = job_path.with_extension("approval");
    if approval_path.exists() {
        let token = std::fs::read_to_string(&approval_path).unwrap_or_default();
        let program_src = std::fs::read_to_string(&manifest.program).unwrap_or_default();
        if let Err(reason) = crate::approval::verify_approval(&token, &program_src, &manifest.grant)
        {
            // An INVALID token is a failure whether or not policy required
            // one. Note what that does and does not mean: `verify_approval`
            // re-hashes an UNKEYED sha256 over public inputs, so this proves
            // the program and grant are byte-identical to whatever the token's
            // author hashed — NOT that a particular human approved anything.
            // Anyone who can write this file can mint a token that verifies.
            println!("\u{26a0} DENIED: {reason}");
            return ExitCode::from(8);
        }
        if manifest.require_approval {
            println!("Approval: required and verified (program + grant unedited since sign-off)");
        } else {
            println!("Approval: verified (not required by this job)");
        }
        // Say what the check is WORTH, not just that it passed. The digest is
        // unkeyed and every input to it is public, so it establishes integrity
        // since sign-off and nothing about WHO signed. An operator reading
        // "verified" beside a named approver would otherwise reasonably infer
        // authentication that is not happening.
        println!(
            "  (integrity only: the approval digest is unkeyed, so this shows \
             the program and grant are unedited — not who approved them)"
        );
    } else if manifest.require_approval {
        println!(
            "\u{26a0} DENIED: approval required but missing — this job sets \
             `require_approval = true` and there is no token at {}",
            approval_path.display()
        );
        return ExitCode::from(8);
    } else {
        // An ABSENT token is not a verified one. The gate is opt-in by the
        // presence of the very artifact it checks (triage OSK-P4-H8), and until
        // now an unapproved run printed nothing about sign-off and archived a
        // record with no approval field at all — so a reviewer reading that
        // record could not tell "signed off" from "nobody checked". That is the
        // absent-vs-verified collapse, in the artifact whose whole job is to be
        // the evidence.
        //
        // This says so; it deliberately does NOT refuse. Making a missing token
        // exit 8 is a policy change (the finding's own fix sketch proposes
        // driving it from risk level or a manifest field), and that is the
        // operator's decision, not this function's.
        // NOT a warning. The job did not ask for approval and is behaving
        // exactly as configured; "⚠ NOT APPROVED" read as a degraded or unsafe
        // state and would train operators to ignore the line. State the policy,
        // so the output tracks configuration rather than treating absence as
        // inherently abnormal.
        println!("Approval: not required");
    }

    // ── Kill-file setup (R27 + R29) ───────────────────────────────────────────
    // R27: --killable creates a kill file for operator-driven `axon-os kill`.
    // R29: --monitor creates a kill file for the compliance monitor thread.
    // Both use AXON_KILL_FILE; --monitor takes precedence if both are set.
    //
    // We ensure the output dir exists first (needed for both R27 and R29).
    if killable || monitor_effects.is_some() {
        if let Err(e) = std::fs::create_dir_all(&out) {
            eprintln!("axon-os run: cannot create {}: {e}", out.display());
            return ExitCode::from(2);
        }
    }

    // ONE authoritative kill channel per run (decided 2026-09-19, triage
    // OSK-P4-H6). This used to require `monitor_effects.is_none()`, so
    // `--killable --monitor` created NO `<run>.kill` at all — monitoring
    // silently disabled killability, and `axon-os kill` wrote a file nothing
    // polled while printing the tripped banner.
    //
    // R27 and R29 now SHARE `<out>/<run_id>.kill`: one path, two writers.
    let kill_file_path = if killable || monitor_effects.is_some() {
        let kf = out.join(format!("{run_id}.kill"));
        let _ = std::fs::write(&kf, r#"{"latch":"clear"}"#);
        std::env::set_var("AXON_KILL_FILE", &kf);
        // Record WHERE the channel is, at run START — not in the run record,
        // which is written when the run ENDS and so cannot help a live
        // `axon-os kill`. Reconstructing the path by convention breaks the
        // moment `run --out A` is killed with `kill --store B`, and that is
        // exactly the duplicated-truth pattern these findings keep coming from.
        let ptr = out.join(format!("{run_id}.chan"));
        let abs = std::fs::canonicalize(&kf).unwrap_or_else(|_| kf.clone());
        let _ = std::fs::write(&ptr, format!("{{\"kill_file\":\"{}\"}}", abs.display()));
        Some(kf)
    } else {
        None
    };

    // R29: --monitor shares the ONE kill channel created above rather than
    // minting `<run>.monitor.kill`. Two latches for one run meant every reader
    // had to know which flag combination produced which name — and `kill` and
    // `status` each reconstructed only one of them.
    let monitor_state = if let Some(effects_str) = &monitor_effects {
        let kill_file = kill_file_path
            .clone()
            .expect("the shared kill channel is created whenever --monitor is set");
        // AXON_KILL_FILE is already pointed at it; set again is harmless and
        // keeps this branch readable on its own.
        std::env::set_var("AXON_KILL_FILE", &kill_file);

        let ledger = monitor_ledger.clone().unwrap_or_else(|| {
            // Default: <out>/<run-id>.audit.jsonl
            out.join(format!("{run_id}.audit.jsonl"))
        });

        // Point the CHILD at the same ledger the monitor watches.
        //
        // `--ledger` configured the watcher and nothing else, so the program
        // under supervision never learned where to append — it writes through
        // `AXON_AUDIT_LEDGER`, which only AUDIT T23 forwards, and only when the
        // operator happens to have exported it. Measured:
        //
        //   run --monitor IO,Net --ledger P                  -> P never created
        //   AXON_AUDIT_LEDGER=P run --monitor IO,Net --ledger P -> 5 entries
        //
        // So by default the R29 compliance monitor polled a file nobody wrote,
        // saw no effects, denied nothing, and the run reported success. A
        // monitor that cannot observe is not a weaker monitor, it is none — and
        // it looked identical to a clean run.
        //
        // Set rather than defaulted, and set unconditionally: if `--ledger` and
        // an exported `AXON_AUDIT_LEDGER` disagreed, the writer and the watcher
        // would use different files, which is the same inert monitor with an
        // extra way to reach it. The flag is the authority. This mirrors
        // `AXON_KILL_FILE` above, set the same way for the same reason.
        std::env::set_var("AXON_AUDIT_LEDGER", &ledger);
        let allowed: Vec<String> = effects_str
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let stop = Arc::new(AtomicBool::new(false));
        let violation = Arc::new(AtomicBool::new(false));
        let stop_clone = Arc::clone(&stop);
        let violation_clone = Arc::clone(&violation);
        let kill_file_clone = kill_file.clone();

        let monitor = ComplianceMonitor::new(ledger, kill_file_clone, allowed, stop_clone);

        let monitor_thread = std::thread::spawn(move || {
            let result = monitor.run();
            if matches!(result, MonitorResult::ViolationDetected { .. }) {
                violation_clone.store(true, Ordering::Release);
            }
            result
        });

        Some((stop, violation, monitor_thread, kill_file))
    } else {
        if !killable {
            std::env::remove_var("AXON_KILL_FILE");
        }
        None
    };

    // ── Run the job (blocks until completion or monitor/kill-switch kill) ─────
    let rt = AxonCoreRuntime::from_env();
    let sup = broad_supervisor_grant();
    let rec = supervisor::run(&manifest, &sup, &run_id, &rt);

    // ── R27: clean up the kill file env var after the run ────────────────────
    if kill_file_path.is_some() {
        std::env::remove_var("AXON_KILL_FILE");
    }

    // ── R29: stop monitor thread and check violation flag ─────────────────────
    let containment_violation = if let Some((stop, violation, thread, kill_file)) = monitor_state {
        // Signal the monitor to stop (clean exit when job finishes normally).
        stop.store(true, Ordering::Release);
        // Join the thread. Panic on join = monitor crashed = fail-closed (I-1/I-6).
        let thread_result = thread.join();
        let v = violation.load(Ordering::Acquire);
        // If the thread panicked (I-1), treat as violation.
        let panicked = thread_result.is_err();
        if panicked {
            // I-1: monitor crash → ensure kill file is tripped.
            let _ = std::fs::write(
                &kill_file,
                r#"{"latch":"tripped","reason":"R29 monitor panic — fail-closed"}"#,
            );
        }
        // Clean up env var.
        std::env::remove_var("AXON_KILL_FILE");
        v || panicked
    } else {
        false
    };

    if let Err(e) = std::fs::create_dir_all(&out) {
        eprintln!("axon-os run: cannot create {}: {e}", out.display());
        return ExitCode::from(2);
    }
    let rec_path = out.join(format!("{run_id}.json"));
    let _ = std::fs::write(&rec_path, to_json(&rec));
    // Save a manifest copy with the ABSOLUTE program path so `replay` reproduces
    // it regardless of the store directory (deterministic replay).
    let _ = std::fs::write(
        out.join(format!("{run_id}.axjob")),
        crate::manifest::to_axjob(&manifest),
    );
    println!(
        "{}  (run-id: {run_id}, record: {})",
        rec.verdict.legible(),
        rec_path.display()
    );

    // R29: if the compliance monitor detected a violation, return exit 12
    // regardless of the job's own verdict.
    if containment_violation {
        eprintln!("axon-os: R29 CONTAINMENT VIOLATION — effect constraint exceeded; job killed");
        return ExitCode::from(CONTAINMENT_VIOLATION_EXIT_CODE as u8);
    }
    ExitCode::from(rec.verdict.exit_code() as u8)
}

fn cmd_verify(record: &Path) -> ExitCode {
    let src = match std::fs::read_to_string(record) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("axon-os verify: cannot read {}: {e}", record.display());
            return ExitCode::from(2);
        }
    };
    let rec = match from_json(&src) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("axon-os verify: {}", e.detail);
            return ExitCode::from(2);
        }
    };
    match verify(&rec) {
        Ok(()) => {
            println!(
                "\u{2713} intact ({} events, digest {})",
                rec.events.len(),
                rec.record_digest
            );
            ExitCode::from(0)
        }
        Err(e) => {
            println!("\u{2717} TAMPERED: {}", e.detail);
            // VerifyMismatch → exit 11 (R27 §5.3: freed 9 for RESOURCE_BOUND_EXIT_CODE).
            ExitCode::from(
                crate::verdict::Verdict::VerifyMismatch { detail: e.detail }.exit_code() as u8,
            )
        }
    }
}

fn cmd_replay(rest: &[&str]) -> ExitCode {
    let mut run_id: Option<&str> = None;
    let mut store = PathBuf::from(".");
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--store" if i + 1 < rest.len() => {
                store = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            s if !s.starts_with("--") && run_id.is_none() => {
                run_id = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-os replay: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(run_id) = run_id else {
        eprintln!("axon-os replay: missing <run-id>");
        return ExitCode::from(2);
    };
    let rec_path = store.join(format!("{run_id}.json"));
    let man_path = store.join(format!("{run_id}.axjob"));
    let stored = match std::fs::read_to_string(&rec_path)
        .ok()
        .and_then(|s| from_json(&s).ok())
    {
        Some(r) => r,
        None => {
            eprintln!("axon-os replay: no valid record at {}", rec_path.display());
            return ExitCode::from(2);
        }
    };
    let manifest = match read_manifest(&man_path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon-os replay: {e}");
            return ExitCode::from(2);
        }
    };
    let rt = AxonCoreRuntime::from_env();
    match replay::replay(&stored, &manifest, &broad_supervisor_grant(), &rt) {
        Ok(_) => {
            println!("\u{2713} replay identical (deterministic; record verified)");
            ExitCode::from(0)
        }
        Err(e) => {
            println!("\u{2717} {}", e.detail);
            ExitCode::from(
                crate::verdict::Verdict::VerifyMismatch { detail: e.detail }.exit_code() as u8,
            )
        }
    }
}

// ── R27: kill / status ───────────────────────────────────────────────────────

/// R27 §5.1: trip the supervisor kill-latch for a running job.
/// `axon-os kill <run-id> [--store DIR] [--reason REASON]`
///
/// Writes the tripped state to `$store/$run_id.kill` (the file the running
/// supervisor polls). The running job's `run_bounded` loop detects the trip and
/// kills the subprocess with exit code 4 (`HALTED_EXIT_CODE`).
/// Resolve a run's AUTHORITATIVE kill channel.
///
/// Prefers the pointer written at run START (`<store>/<run-id>.chan`) over
/// reconstructing `<store>/<run-id>.kill` by convention. The pointer is what
/// makes `run --out A` killable via `kill --store A` when the conventional
/// guess would be wrong, and it is a single truth rather than a naming rule
/// every reader has to re-derive.
///
/// Returns `(path, from_pointer)`. Falling back is legitimate — a kill can be
/// ARMED before the run starts — but the caller must be able to say which
/// happened, because a guessed path that nothing polls is exactly the failure
/// this replaces.
fn resolve_kill_channel(store: &Path, run_id: &str) -> (PathBuf, bool) {
    let ptr = store.join(format!("{run_id}.chan"));
    if let Ok(txt) = std::fs::read_to_string(&ptr) {
        // Deliberately a substring read, not a JSON parse: this file is written
        // by the same binary one function away, and a parser dependency here
        // would be a third thing to keep in step.
        if let Some(rest) = txt.split("\"kill_file\":\"").nth(1) {
            if let Some(p) = rest.split('"').next() {
                if !p.is_empty() {
                    return (PathBuf::from(p), true);
                }
            }
        }
    }
    (store.join(format!("{run_id}.kill")), false)
}

fn cmd_kill(rest: &[&str]) -> ExitCode {
    let mut run_id: Option<&str> = None;
    let mut store = PathBuf::from(".");
    let mut reason = "operator shutdown".to_string();
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--store" if i + 1 < rest.len() => {
                store = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            "--reason" if i + 1 < rest.len() => {
                reason = rest[i + 1].to_string();
                i += 2;
            }
            s if !s.starts_with("--") && run_id.is_none() => {
                run_id = Some(s);
                i += 1;
            }
            _ => {
                eprintln!("axon-os kill: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }
    let Some(run_id) = run_id else {
        eprintln!("axon-os kill: missing <run-id>");
        return ExitCode::from(2);
    };
    // AUDIT T26 (finding OSK-P4-H6 / O020): this reconstructed ONE path by
    // naming convention. A run started with `--killable --monitor` never
    // creates `<run>.kill` (cli.rs:273 requires `monitor_effects.is_none()`) —
    // it has `<run>.monitor.kill` instead. So `axon-os kill` wrote a file
    // nothing polls, printed the tripped banner and exited 0, leaving the
    // operator believing a run had been killed when it had not.
    //
    // Trip EVERY kill latch that exists for this run. Writing an already-clear
    // latch is harmless; missing the live one is not.
    // The recorded channel first; the legacy names remain as a fallback so a
    // store written by an older axon-os still works.
    let (recorded, from_ptr) = resolve_kill_channel(&store, run_id);
    let candidates = if from_ptr {
        vec![recorded]
    } else {
        vec![recorded, store.join(format!("{run_id}.monitor.kill"))]
    };
    let existing: Vec<&PathBuf> = candidates.iter().filter(|p| p.exists()).collect();
    // Nothing to trip: fall back to the conventional path so a kill armed
    // BEFORE the run starts still works (the pre-arm case), but say so.
    let pre_armed = existing.is_empty();
    let targets: Vec<&PathBuf> = if pre_armed {
        eprintln!(
            "axon-os kill: no existing kill latch for run `{run_id}` — \
             writing {} (a run that has not started yet will pick it up)",
            candidates[0].display()
        );
        vec![&candidates[0]]
    } else {
        existing
    };
    let kill_path = targets[0].clone();
    let content = format!(
        "{{\"latch\":\"tripped\",\"reason\":\"{}\"}}",
        reason.replace('"', "'")
    );
    for t in targets.iter().skip(1) {
        let _ = std::fs::write(t, &content); // trip every live latch
    }
    match std::fs::write(&kill_path, &content) {
        Ok(()) => {
            // The invariant: NEVER report TRIPPED unless the run's authoritative
            // channel actually existed to be tripped. Writing a latch for a run
            // that has not started is a legitimate PRE-ARM, but saying "killed"
            // there tells an operator a job was stopped when no such job was
            // ever running — the same absent-vs-done collapse these findings
            // keep turning up, in the one command whose whole purpose is to
            // stop something.
            if pre_armed {
                println!(
                    "\u{1f512} kill ARMED for run `{run_id}` (reason: {reason}) — no run \
                     was live to stop; a run starting with this id will pick it up"
                );
            } else {
                println!("\u{1f6d1} kill tripped for run `{run_id}` (reason: {reason})");
            }
            ExitCode::from(0)
        }
        Err(e) => {
            eprintln!(
                "axon-os kill: cannot write kill file {}: {e}",
                kill_path.display()
            );
            ExitCode::from(2)
        }
    }
}

/// R27 §5.1: show the latch state + ledger totals for a run.
/// `axon-os status [--store DIR] [--latest] [--json]`
fn cmd_status(rest: &[&str]) -> ExitCode {
    let mut store = PathBuf::from(".");
    let mut run_id: Option<String> = None;
    let mut latest_only = false;
    let mut json_out = false;
    let mut i = 0;
    while i < rest.len() {
        match rest[i] {
            "--store" if i + 1 < rest.len() => {
                store = PathBuf::from(rest[i + 1]);
                i += 2;
            }
            "--latest" => {
                // Restrict the report to the most recently modified latch.
                // This arm used to be a no-op: the flag was accepted, documented
                // in `--help`, and changed nothing, because the DEFAULT already
                // reported only the newest latch. An accepted flag that does
                // nothing is indistinguishable from one that works.
                latest_only = true;
                i += 1;
            }
            "--json" => {
                json_out = true;
                i += 1;
            }
            s if !s.starts_with("--") && run_id.is_none() => {
                run_id = Some(s.to_string());
                i += 1;
            }
            _ => {
                eprintln!("axon-os status: bad argument `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }

    // If no run_id given, report EVERY run in the store.
    //
    // This reported only the most recently modified latch, with nothing to say
    // it was one of many. Measured on a store holding 8 latches of which 4 were
    // TRIPPED by R29 containment violations, `status` printed a single
    //
    //     ✓ run `pre.monitor`: latch = clear
    //
    // and exited 0 — four killed runs invisible behind one green tick about an
    // unrelated run. An operator's status view that hides tripped latches is
    // the one thing it must not do.
    if run_id.is_none() {
        return status_all(&store, latest_only, json_out);
    }

    // If no run_id given, look for any .kill file in the store directory.
    let kill_path = if let Some(rid) = &run_id {
        // Same resolver `kill` uses. `status` reconstructing its own path is
        // how the two could report different things about one run.
        resolve_kill_channel(&store, rid).0
    } else {
        // Find the most recent .kill file.
        let found = std::fs::read_dir(&store)
            .ok()
            .and_then(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter(|e| e.path().extension().is_some_and(|x| x == "kill"))
                    .max_by_key(|e| e.metadata().and_then(|m| m.modified()).ok())
            })
            .map(|e| e.path());
        match found {
            Some(p) => p,
            None => {
                eprintln!("axon-os status: no .kill file found in {}", store.display());
                return ExitCode::from(2);
            }
        }
    };

    let state = match std::fs::read_to_string(&kill_path) {
        Ok(s) => s,
        Err(_) => "{\"latch\":\"clear\"}".to_string(),
    };
    let run_id_str = run_id.as_deref().unwrap_or(
        kill_path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown"),
    );
    let tripped = state.contains("\"latch\":\"tripped\"");
    if json_out {
        println!(
            "{{\"run_id\":\"{run_id_str}\",\"latch\":\"{}\",\"kill_file\":\"{}\"}}",
            if tripped { "tripped" } else { "clear" },
            kill_path.display()
        );
    } else {
        let symbol = if tripped { "\u{1f6d1}" } else { "\u{2713}" };
        println!(
            "{symbol} run `{run_id_str}`: latch = {}",
            if tripped { "TRIPPED" } else { "clear" }
        );
    }
    ExitCode::from(0)
}

/// Report every run in the store, or just the newest under `--latest`.
///
/// Sorted newest-first so the common "what just happened" reading is the top
/// line, and every other run is still on the screen. A TRIPPED latch is counted
/// in the summary, because a list is only an improvement over one line if the
/// reader does not have to scan it to learn whether anything was killed.
fn status_all(store: &Path, latest_only: bool, json_out: bool) -> ExitCode {
    let mut runs: Vec<(PathBuf, std::time::SystemTime)> = match std::fs::read_dir(store) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().is_some_and(|x| x == "kill"))
            .filter_map(|e| {
                let t = e.metadata().and_then(|m| m.modified()).ok()?;
                Some((e.path(), t))
            })
            .collect(),
        Err(_) => Vec::new(),
    };
    if runs.is_empty() {
        eprintln!("axon-os status: no .kill file found in {}", store.display());
        return ExitCode::from(2);
    }
    // Newest first: sort by time, then reverse, which clippy prefers to a
    // hand-written comparator.
    runs.sort_by_key(|r| r.1);
    runs.reverse();
    if latest_only {
        runs.truncate(1);
    }

    let mut tripped_count = 0usize;
    let mut rows: Vec<String> = Vec::new();
    for (path, _) in &runs {
        let state =
            std::fs::read_to_string(path).unwrap_or_else(|_| "{\"latch\":\"clear\"}".into());
        let tripped = state.contains("\"latch\":\"tripped\"");
        if tripped {
            tripped_count += 1;
        }
        let rid = path
            .file_stem()
            .and_then(|x| x.to_str())
            .unwrap_or("unknown");
        if json_out {
            rows.push(format!(
                "{{\"run_id\":\"{rid}\",\"latch\":\"{}\",\"kill_file\":\"{}\"}}",
                if tripped { "tripped" } else { "clear" },
                path.display()
            ));
        } else {
            let symbol = if tripped { "\u{1f6d1}" } else { "\u{2713}" };
            rows.push(format!(
                "{symbol} run `{rid}`: latch = {}",
                if tripped { "TRIPPED" } else { "clear" }
            ));
        }
    }

    if json_out {
        println!(
            "{{\"runs\":[{}],\"total\":{},\"tripped\":{}}}",
            rows.join(","),
            runs.len(),
            tripped_count
        );
    } else {
        for r in &rows {
            println!("{r}");
        }
        if !latest_only {
            println!("  {} run(s), {} TRIPPED", runs.len(), tripped_count);
        }
    }
    ExitCode::from(0)
}

// ── R28: audit verify / show ─────────────────────────────────────────────────

/// R28: verify or show the capability audit ledger.
/// `axon-os audit verify --ledger PATH`
/// `axon-os audit show   --ledger PATH [--json]`
fn cmd_audit(rest: &[&str]) -> ExitCode {
    let mut i = 0;
    let subcommand = if rest.is_empty() {
        eprintln!("axon-os audit: missing subcommand (verify | show)");
        return ExitCode::from(2);
    } else {
        let s = rest[0];
        i += 1;
        s
    };

    let mut ledger_path: Option<PathBuf> = None;
    let mut json_out = false;
    while i < rest.len() {
        match rest[i] {
            "--ledger" if i + 1 < rest.len() => {
                ledger_path = Some(PathBuf::from(rest[i + 1]));
                i += 2;
            }
            "--json" => {
                json_out = true;
                i += 1;
            }
            _ => {
                eprintln!("axon-os audit: unknown flag `{}`", rest[i]);
                return ExitCode::from(2);
            }
        }
    }

    let path = match ledger_path {
        Some(p) => p,
        None => {
            eprintln!("axon-os audit: missing --ledger PATH");
            return ExitCode::from(2);
        }
    };

    // An ABSENT ledger is not a verified ledger.
    //
    // `Ledger::open` is open-OR-CREATE: a missing path yields an empty chain so
    // a writer can start appending, which is right for the writer and wrong
    // here. `verify()` then accepts the empty chain and this printed
    //
    //     ✓ ledger intact (0 entries, chain verified)     exit 0
    //
    // for a file that had never existed. An audit verifier certifying a chain
    // that is not there is the one answer it must never give.
    //
    // Fail closed, with exit 11, for the reason axon-audit already gives about a
    // missing chain tip: "deleting it is exactly what an attacker who truncated
    // the ledger would do next". An absent ledger and a deleted one are the same
    // bytes on disk.
    if !path.exists() {
        eprintln!(
            "axon-os audit: no ledger at {} — nothing to verify. An absent ledger \
             is indistinguishable from a deleted one, so this is a failure, not a pass.",
            path.display()
        );
        return ExitCode::from(11);
    }

    let ledger = match axon_audit::Ledger::open(&path) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("axon-os audit: {e}");
            // exit 11 = tamper/divergence (reuse the verify-mismatch code)
            return ExitCode::from(11);
        }
    };

    match subcommand {
        "verify" => match ledger.verify() {
            Ok(()) if ledger.is_empty() => {
                // A present-but-empty ledger is not tampered, but "chain
                // verified" claims something was checked. Nothing was.
                println!("\u{2713} ledger present but EMPTY (0 entries) — nothing to verify");
                ExitCode::from(0)
            }
            Ok(()) => {
                println!(
                    "\u{2713} ledger intact ({} entries, chain verified)",
                    ledger.len()
                );
                ExitCode::from(0)
            }
            Err(e) => {
                println!("\u{2717} LEDGER TAMPERED: {e}");
                ExitCode::from(11)
            }
        },
        "show" => {
            if json_out {
                match ledger.export_json() {
                    Ok(j) => {
                        println!("{j}");
                        ExitCode::from(0)
                    }
                    Err(e) => {
                        eprintln!("axon-os audit show: {e}");
                        ExitCode::from(2)
                    }
                }
            } else {
                println!("Ledger: {} entries", ledger.len());
                for entry in ledger.entries() {
                    println!(
                        "  seq={} ts={} principal={} effect={:?} op={}",
                        entry.seq, entry.ts_ms, entry.principal, entry.effect, entry.operation
                    );
                }
                ExitCode::from(0)
            }
        }
        _ => {
            eprintln!("axon-os audit: unknown subcommand `{subcommand}` (use verify | show)");
            ExitCode::from(2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legible_grant_lists_may_and_may_not() {
        let g = Grant {
            reproducible: false,
            fs_read: vec!["./data/".into()],
            fs_write: vec![],
            net: vec![],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 100,
                tokens: 50000,
                cost_micro: 1000000,
            },
        };
        let s = legible_grant(&g);
        assert!(s.contains("read ./data/"));
        assert!(s.contains("may NOT") && s.contains("use the network"));
        assert!(s.contains("internal"));
    }

    #[test]
    fn broad_grant_is_a_superset_of_a_narrow_job() {
        let job = Grant {
            reproducible: false,
            fs_read: vec!["./data/".into()],
            fs_write: vec!["./out/".into()],
            net: vec!["a.x.com".into()],
            exec: ExecPolicy::None,
            max_label: Label::Internal,
            budget: Budget {
                calls: 5,
                tokens: 5,
                cost_micro: 5,
            },
        };
        let eff = job.intersect(&broad_supervisor_grant());
        // The broad supervisor grant leaves the job's narrower bound intact.
        assert_eq!(eff.fs_read, vec!["./data/".to_string()]);
        assert_eq!(eff.net, vec!["a.x.com".to_string()]);
        assert!(eff.is_subset_of(&job) && job.is_subset_of(&eff));
    }
}
