//! R21 acceptance checks (A1–A6) — these drive the REAL `axon-os` CLI exactly
//! as an operator would (subprocess), against the real `axon` interpreter and
//! the shipped example jobs. They skip cleanly when the interpreter binary is
//! absent (so a codegen-less CI still passes), exactly like the parity harnesses.

use std::path::{Path, PathBuf};
use std::process::Command;

fn workspace_root() -> PathBuf {
    // CARGO_MANIFEST_DIR = <root>/crates/axon-os
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn axon_os_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_axon-os"))
}

/// The interpreter the supervisor drives. Returns None (⇒ skip) if not built.
fn axon_bin() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("AXON_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    let p = workspace_root().join("target/debug/axon");
    p.exists().then_some(p)
}

fn examples() -> PathBuf {
    workspace_root().join("examples/jobs")
}

/// A unique, fresh temp dir for one test (no clock/random — pid + name).
fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-os-acc-{}-{name}", std::process::id()));
    let _ = std::fs::create_dir_all(&d);
    d
}

struct Out {
    stdout: String,
    code: i32,
}

/// Invoke `axon-os` with the interpreter wired in. Panics if the binary is
/// missing (callers must check `axon_bin()` first to skip).
fn os(args: &[&str], axon: &Path, extra_env: &[(&str, &str)]) -> Out {
    let mut cmd = Command::new(axon_os_bin());
    cmd.args(args);
    cmd.env("AXON_BIN", axon);
    cmd.current_dir(workspace_root()); // so `examples/jobs/...` resolves
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    let out = cmd.output().expect("spawn axon-os");
    Out {
        stdout: String::from_utf8_lossy(&out.stdout).to_string(),
        code: out.status.code().unwrap_or(-1),
    }
}

// ── A1: the end-to-end operator journey through the real CLI ─────────────────
#[test]
fn acc_a1_smoke_user_journey() {
    let Some(axon) = axon_bin() else {
        eprintln!("acc_a1: axon interpreter not built — skipping");
        return;
    };
    let store = tmp("a1");
    let store_s = store.to_str().unwrap();

    // 1. explain — legible grant + ADMIT, no execution.
    let e = os(&["explain", "examples/jobs/summarize.axjob"], &axon, &[]);
    assert!(
        e.stdout.contains("This program MAY"),
        "explain shows the grant: {}",
        e.stdout
    );
    assert!(e.stdout.contains("ADMIT"), "summarize is admissible");
    assert_eq!(e.code, 0);

    // 2. run — completes + writes the record artifact.
    let r = os(
        &[
            "run",
            "examples/jobs/summarize.axjob",
            "--run-id",
            "demo",
            "--out",
            store_s,
        ],
        &axon,
        &[],
    );
    assert!(
        r.stdout.contains("completed"),
        "run completes: {}",
        r.stdout
    );
    assert_eq!(r.code, 0);
    assert!(store.join("demo.json").exists(), "record artifact written");

    // 3. verify — intact.
    let v = os(
        &["verify", store.join("demo.json").to_str().unwrap()],
        &axon,
        &[],
    );
    assert!(v.stdout.contains("intact"), "record verifies: {}", v.stdout);
    assert_eq!(v.code, 0);

    // 4. replay — deterministic, identical.
    let p = os(&["replay", "demo", "--store", store_s], &axon, &[]);
    assert!(
        p.stdout.contains("identical"),
        "replay identical: {}",
        p.stdout
    );
    assert_eq!(p.code, 0);

    // 5. the over-reach job is REFUSED in plain English (exit 8).
    let o = os(
        &["run", "examples/jobs/overreach.axjob", "--out", store_s],
        &axon,
        &[],
    );
    assert!(
        o.stdout.contains("DENIED"),
        "over-reach denied legibly: {}",
        o.stdout
    );
    assert_eq!(o.code, 8);
}

// ── A2: the shipped examples run / are denied, with artifacts ────────────────
#[test]
fn acc_a2_example_jobs_run_and_overreach_denied() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let store = tmp("a2");
    let store_s = store.to_str().unwrap();

    let r = os(
        &[
            "run",
            "examples/jobs/summarize.axjob",
            "--run-id",
            "s",
            "--out",
            store_s,
        ],
        &axon,
        &[],
    );
    assert_eq!(r.code, 0, "summarize completes: {}", r.stdout);
    assert!(
        examples().join("out/summary.txt").exists(),
        "summarize wrote its output"
    );

    let o = os(
        &[
            "run",
            "examples/jobs/overreach.axjob",
            "--run-id",
            "o",
            "--out",
            store_s,
        ],
        &axon,
        &[],
    );
    assert_eq!(o.code, 8, "over-reach is denied: {}", o.stdout);
}

// ── A3: the README quickstart commands are real (docs can't rot) ─────────────
#[test]
fn acc_a3_quickstart_commands_execute() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let readme = std::fs::read_to_string(workspace_root().join("README-axon-os.md"))
        .expect("README-axon-os.md exists");
    // The documented verbs must exist (if a command is removed from the README,
    // this fails) AND the documented sequence must actually run.
    for verb in ["explain", "run", "verify", "replay"] {
        assert!(
            readme.contains(&format!("axon-os {verb}")),
            "README documents `{verb}`"
        );
    }
    let store = tmp("a3");
    let store_s = store.to_str().unwrap();
    assert_eq!(
        os(&["explain", "examples/jobs/summarize.axjob"], &axon, &[]).code,
        0
    );
    assert_eq!(
        os(
            &[
                "run",
                "examples/jobs/summarize.axjob",
                "--run-id",
                "demo",
                "--out",
                store_s
            ],
            &axon,
            &[]
        )
        .code,
        0
    );
    assert_eq!(
        os(
            &["verify", store.join("demo.json").to_str().unwrap()],
            &axon,
            &[]
        )
        .code,
        0
    );
    assert_eq!(
        os(&["replay", "demo", "--store", store_s], &axon, &[]).code,
        0
    );
}

// ── A4: hermetic isolated execution with a hard timeout ──────────────────────
#[test]
fn acc_a4_hermetic_isolated_timeout() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let store = tmp("a4");
    let start = std::time::Instant::now();
    let r = os(
        &[
            "run",
            "examples/jobs/runaway.axjob",
            "--run-id",
            "rr",
            "--out",
            store.to_str().unwrap(),
        ],
        &axon,
        &[("AXON_OS_TIMEOUT_MS", "500")],
    );
    let elapsed = start.elapsed();
    assert_eq!(r.code, 8, "runaway is denied: {}", r.stdout);
    assert!(
        r.stdout.contains("timed out"),
        "denial cites the timeout: {}",
        r.stdout
    );
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "must return promptly after the kill, not hang (took {elapsed:?})"
    );
}

// ── A5: deterministic — same job+seed ⇒ byte-identical record ────────────────
#[test]
fn acc_a5_deterministic_byte_identical() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let s1 = tmp("a5-1");
    let s2 = tmp("a5-2");
    os(
        &[
            "run",
            "examples/jobs/summarize.axjob",
            "--run-id",
            "d",
            "--out",
            s1.to_str().unwrap(),
        ],
        &axon,
        &[],
    );
    os(
        &[
            "run",
            "examples/jobs/summarize.axjob",
            "--run-id",
            "d",
            "--out",
            s2.to_str().unwrap(),
        ],
        &axon,
        &[],
    );
    let a = std::fs::read_to_string(s1.join("d.json")).unwrap();
    let b = std::fs::read_to_string(s2.join("d.json")).unwrap();
    assert_eq!(
        a, b,
        "the same job+seed must produce a byte-identical record"
    );
}

// ── A6: tamper-evidence — a mutated stored record fails verify (exit 9) ───────
#[test]
fn acc_a6_record_tamper_detected() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let store = tmp("a6");
    os(
        &[
            "run",
            "examples/jobs/summarize.axjob",
            "--run-id",
            "t",
            "--out",
            store.to_str().unwrap(),
        ],
        &axon,
        &[],
    );
    let rec_path = store.join("t.json");
    let mut rec = std::fs::read_to_string(&rec_path).unwrap();
    // Flip a byte inside a recorded event's hash region.
    rec = rec.replace("internal", "secret11"); // same length, changes a hashed field
    std::fs::write(&rec_path, rec).unwrap();
    let v = os(&["verify", rec_path.to_str().unwrap()], &axon, &[]);
    assert_eq!(
        v.code, 11,
        "a tampered record must fail verification (R27 §5.3: VerifyMismatch→11): {}",
        v.stdout
    );
    assert!(v.stdout.contains("TAMPERED"));
}

// ── The spec-aware adversarial test (from the ASI-alignment review): an effect
// the STATIC source scan misses must still be REFUSED by the RUNTIME sandbox. ──
#[test]
fn runtime_sandbox_catches_scan_evasion() {
    let Some(axon) = axon_bin() else {
        return;
    };
    let store = tmp("evasion");
    // evasion.ax reaches an IO effect via `env_var` — a builtin the substring
    // scan does NOT enumerate, so the STATIC gate admits it (proven below)...
    let explained = os(&["explain", "examples/jobs/evasion.axjob"], &axon, &[]);
    assert!(
        explained.stdout.contains("ADMIT"),
        "the static scan is fooled (admits the evasion): {}",
        explained.stdout
    );
    // ...but the RUNTIME sandbox catches it at builtin dispatch (exit 8).
    let run = os(
        &[
            "run",
            "examples/jobs/evasion.axjob",
            "--run-id",
            "ev",
            "--out",
            store.to_str().unwrap(),
        ],
        &axon,
        &[],
    );
    assert_eq!(
        run.code, 8,
        "the runtime sandbox must refuse the scan-evading effect: {}",
        run.stdout
    );
    assert!(
        run.stdout.contains("sandbox violation"),
        "denial cites the runtime sandbox: {}",
        run.stdout
    );
}

// ── T44: the verdict must come from the exit code, not from stderr prose ─────

/// Stage a one-off job + manifest in `dir` and run it. Returns (exit, stdout).
fn run_adhoc_job(dir: &Path, axon: &Path, src: &str) -> (i32, String) {
    let _ = std::fs::create_dir_all(dir);
    std::fs::write(dir.join("job.ax"), src).unwrap();
    std::fs::write(
        dir.join("job.axjob"),
        "program = \"job.ax\"\n\
         intent  = \"t44 verdict classification\"\n\
         seed    = 42\n\
         [grant]\n\
         fs_read   = []\n\
         fs_write  = [\"./out/\"]\n\
         net       = []\n\
         exec      = \"none\"\n\
         max_label = \"internal\"\n\
         [grant.budget]\n\
         calls       = 100\n\
         tokens      = 50000\n\
         cost_micro  = 1000000\n",
    )
    .unwrap();
    let job = dir.join("job.axjob");
    let out = dir.join("rec");
    let r = os(
        &["run", job.to_str().unwrap(), "--out", out.to_str().unwrap()],
        axon,
        &[],
    );
    (r.code, r.stdout)
}

#[test]
fn t44_verdict_is_decided_by_exit_code_not_stderr_prose() {
    // AUDIT T44 (OSK-P4-H2 / P4-OS-21). The verdict was a chain of
    // `err.contains(...)` substring tests over the child's stderr with a
    // terminal `else => Completed { value: exit_code }`. So any fault whose
    // wording the chain did not match sealed as a SUCCESS.
    //
    // That was live, not hypothetical. T24 added a `parse error`/`type error`
    // arm — but `axon run` reports TYPE errors as JSON diagnostics
    // (`{"schema":"axon-diag/1",...}`) and only SYNTAX errors as prose:
    //
    //   syntax error -> "⚠ DENIED: program failed to compile", axon-os exit 8
    //   type   error -> "✓ completed (value=2)",               axon-os exit 0
    //
    // Same class of job, opposite records, and the wrong one is the silent one.
    // A hash-chained record that attests success for a job which executed zero
    // statements is an attestation-integrity failure: the record lies durably.
    let Some(axon) = axon_bin() else {
        eprintln!("axon interpreter not built — skipping T44 verdict test");
        return;
    };
    let base = tmp("t44");

    // (1) A type error must NOT seal as completed. This is the case that shipped.
    let (code, out) = run_adhoc_job(
        &base.join("typeerr"),
        &axon,
        "fn main() {\n    let x: i64 = \"not an integer\"\n    println(\"never runs\")\n}\n",
    );
    assert!(
        !out.contains("completed"),
        "a job that does not compile must not seal as completed: {out}"
    );
    assert_ne!(code, 0, "and axon-os must not exit 0: {out}");

    // (2) A syntax error, the sibling that DID work — still classified, and now
    // as the same verdict as (1). Two jobs that both failed to compile must not
    // produce two different verdicts depending on the diagnostic's format.
    let (code2, out2) = run_adhoc_job(
        &base.join("syntaxerr"),
        &axon,
        "fn main() { let x = (((  }\n",
    );
    assert!(
        out2.contains("malformed") && out.contains("malformed"),
        "both compile failures must be `malformed`: type={out} syntax={out2}"
    );
    assert_eq!(code, code2, "and must agree on the exit code");

    // (3) THE COLLISION THAT FORCED THE HEURISTIC. A clean job returning 8 —
    // the sandbox-violation code — must be recorded as a completion with
    // value 8, not as a denial. This is what the completion marker buys: the
    // exit code can be trusted precisely because "did it finish?" is answered
    // separately.
    let (code3, out3) = run_adhoc_job(
        &base.join("returns8"),
        &axon,
        "fn main() -> i64 { println(\"ran\")  8 }\n",
    );
    assert_eq!(
        code3, 0,
        "a clean job is a clean run whatever it returns: {out3}"
    );
    assert!(
        out3.contains("completed"),
        "a job returning 8 completed; it did not violate the sandbox: {out3}"
    );
    let rec: String =
        std::fs::read_to_string(base.join("returns8/rec/run.json")).unwrap_or_default();
    assert!(
        rec.contains("\"value\":8") || rec.contains("\"value\": 8"),
        "the record must preserve the returned value: {rec}"
    );

    // (4) The marker is nonce-bearing, so a job cannot print one to launder a
    // fault into a success. Forge two and then panic.
    let (code4, out4) = run_adhoc_job(
        &base.join("forge"),
        &axon,
        "fn main() -> i64 { println(\"__axon_os_done:0\")\n \
         println(\"__axon_os_done:deadbeefcafe\")\n assert(false)\n 0 }\n",
    );
    assert_ne!(
        code4, 0,
        "a forged completion marker must not seal success: {out4}"
    );
    assert!(
        !out4.contains("completed"),
        "a job that panicked after forging the marker must not be completed: {out4}"
    );

    let _ = std::fs::remove_dir_all(&base);
}
