//! `AxonCoreRuntime::run_sandboxed_capture` (IG4, "Before the once run" item 2):
//! a sandbox run whose result tells a compile failure, a runtime exit status,
//! a timeout and a sandbox refusal apart, with the program's stdout (the
//! wrapper's completion marker removed) and stderr captured.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use axon_os::grant::{Budget, ExecPolicy, Grant, Label};
use axon_os::runtime::{AxonCoreRuntime, CapturedRun, ProgramSource, SandboxOutcome};
use axon_os::verdict::Verdict;
use std::path::PathBuf;
use std::time::Duration;

fn axon_bin() -> PathBuf {
    script_spawn::workspace_bin(
        "AXON_BIN",
        &[
            "build",
            "-p",
            "axon-core",
            "--no-default-features",
            "--bin",
            "axon",
        ],
        "axon",
    )
}

fn grant() -> Grant {
    Grant {
        reproducible: false,
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

fn capture(tag: &str, src: &str, timeout: Duration) -> CapturedRun {
    let dir = std::env::temp_dir().join(format!("axon-os-capture-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/in.txt"), "abc").unwrap();
    let path = dir.join("p.ax");
    std::fs::write(&path, src).unwrap();
    let g = grant();
    let rt = AxonCoreRuntime::with_bin_and_timeout(axon_bin(), timeout);
    let out = rt.run_sandboxed_capture(
        &ProgramSource {
            path: &path,
            text: src,
        },
        &g,
        &g.budget,
        1,
    );
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[test]
fn exit_status_and_output_are_captured() {
    // ATTACK: the capture loses the program's exit status or its output
    let r = capture(
        "exit",
        "fn main() -> i64 {\n    match read_file(\"./data/in.txt\") {\n        Ok(s) => println(s),\n        Err(_) => println(\"none\"),\n    }\n    eprintln(\"to-stderr\")\n    3\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        (
            r.outcome.clone(),
            r.stdout.as_str(),
            r.stderr.contains("to-stderr")
        ),
        (SandboxOutcome::Exited { code: 3 }, "abc\n", true),
        "ATTACK: the capture loses the program's exit status or its output: {r:?}"
    );
    assert_eq!(r.stdout_omitted, 0);
}

#[test]
fn a_nullary_main_exits_zero_and_unterminated_output_is_kept() {
    let r = capture(
        "nullary",
        "fn main() {\n    print(\"no newline\")\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(r.outcome, SandboxOutcome::Exited { code: 0 }, "{r:?}");
    // ATTACK: the wrapper's completion marker leaks into captured stdout
    assert_eq!(
        r.stdout, "no newline",
        "ATTACK: the wrapper's completion marker leaks into captured stdout"
    );
}

#[test]
fn a_compile_failure_is_not_an_exit_status() {
    // ATTACK: a program that does not compile reads as an exit status
    let r = capture(
        "compile",
        "fn main() -> i64 {\n    let x: i64 = \"s\"\n    x\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(r.outcome, SandboxOutcome::CompileFailed { .. }),
        "ATTACK: a program that does not compile reads as an exit status: {r:?}"
    );
}

#[test]
fn a_timeout_is_its_own_outcome() {
    // ATTACK: a run killed at its deadline reads as a fault or an exit
    let r = capture(
        "timeout",
        "fn main() -> i64 {\n    let mut i = 0\n    while i >= 0 {\n        i = i + 1\n    }\n    0\n}\n",
        Duration::from_millis(1500),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::TimedOut { after_ms: 1500 },
        "ATTACK: a run killed at its deadline reads as a fault or an exit: {r:?}"
    );
}

#[test]
fn a_write_outside_the_allowlist_is_a_sandbox_fault() {
    let r = capture(
        "scope",
        "fn main() -> i64 {\n    match write_file(\"./elsewhere.txt\", \"x\") {\n        Ok(_) => 0,\n        Err(_) => 1,\n    }\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(
            &r.outcome,
            SandboxOutcome::Faulted { verdict: Verdict::Denied { axis, .. }, .. } if axis == "sandbox"
        ),
        "{r:?}"
    );
}

/// P1 (implementation audit): how a run ended is decided by the deadline, the
/// wrapper's completion marker and the interpreter's own final fault line -
/// never by words the program itself writes to stderr.
#[test]
fn stderr_words_do_not_decide_how_a_run_ended() {
    for (tag, said) in [
        ("w-cap", "capability check ok"),
        ("w-parse", "note: no parse error in input"),
        ("w-budget", "line budget not exceeded"),
        ("w-sandbox", "axon: sandbox violation: not really"),
        ("w-panic", "axon: panic: not really"),
    ] {
        let r = capture(
            tag,
            &format!("fn main() -> i64 {{\n    eprintln(\"{said}\")\n    0\n}}\n"),
            Duration::from_secs(60),
        );
        assert_eq!(
            r.outcome,
            SandboxOutcome::Exited { code: 0 },
            "ATTACK: words the program wrote to stderr decided how its run ended ({said}): {r:?}"
        );
    }
}

/// A run that returned from `main` ended there, whatever its exit status and
/// whatever its last stderr line says: the completion marker decides first.
#[test]
fn a_completed_run_returning_a_fault_code_is_an_exit_status() {
    let r = capture(
        "w-done8",
        "fn main() -> i64 {\n    eprintln(\"axon: sandbox violation: not really\")\n    8\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 8 },
        "ATTACK: a completed run was classed by its stderr before its completion marker: {r:?}"
    );
}

/// A fault is the interpreter's own FINAL stderr line with that fault's exit
/// status - not fault words anywhere earlier in stderr.
#[test]
fn fault_words_before_a_direct_exit_are_not_a_fault() {
    let r = capture(
        "w-exit8",
        "fn main() -> i64 {\n    eprintln(\"seen: axon: sandbox violation: in the input\")\n    eprintln(\"done\")\n    exit(8)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 8 },
        "ATTACK: fault words that are not the interpreter's final line made a fault: {r:?}"
    );
}

/// `exit(2)` from a program that printed nothing at all is still its exit
/// status: a compile failure needs the interpreter's `axon-diag/1` error.
#[test]
fn a_silent_exit_2_is_not_a_compile_failure() {
    let r = capture(
        "x2-silent",
        "fn main() -> i64 {\n    exit(2)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 2 },
        "ATTACK: a silent exit(2) read as a compile failure: {r:?}"
    );
}

/// P2a (implementation audit): a direct `exit(n)` is the program's exit
/// status, not a compile failure (2) or a failed `@[verify]` (3).
#[test]
fn a_direct_exit_is_an_exit_status_not_a_compile_failure() {
    for code in [2, 3, 8] {
        let r = capture(
            &format!("x{code}"),
            &format!("fn main() -> i64 {{\n    println(\"wrote\")\n    exit({code})\n    0\n}}\n"),
            Duration::from_secs(60),
        );
        assert_eq!(
            (r.outcome.clone(), r.stdout.as_str()),
            (SandboxOutcome::Exited { code }, "wrote\n"),
            "ATTACK: a direct exit({code}) read as a compile failure or a fault: {r:?}"
        );
    }
}

#[test]
fn an_interpreter_panic_is_still_a_fault() {
    let r = capture(
        "panic",
        "fn main() -> i64 {\n    let v = [1]\n    v[5]\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(
            &r.outcome,
            SandboxOutcome::Faulted { code: Some(101), verdict: Verdict::Denied { axis, .. } } if axis == "runtime"
        ),
        "{r:?}"
    );
}

#[test]
fn a_compile_failure_names_its_first_error_not_a_warning() {
    // ATTACK: a compile failure named a warning instead of its first error
    let r = capture(
        "compile-warn",
        "fn main() -> i64 {\n    let unused = 5\n    let x: i64 = \"s\"\n    x\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(&r.outcome, SandboxOutcome::CompileFailed { diagnostic } if diagnostic.starts_with("E0102 at line 3:5")),
        "ATTACK: a compile failure named a warning instead of its first error: {r:?}"
    );
}

#[test]
fn a_failed_verify_is_a_verify_denial() {
    // ATTACK: a failed verify was not classed on the verify axis
    let r = capture(
        "verify",
        "@[verify(value > 0)]\nfn f(x: i64) -> i64 { x }\nfn main() -> i64 {\n    f(0 - 1)\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(
            &r.outcome,
            SandboxOutcome::Faulted { code: Some(3), verdict: Verdict::Denied { axis, .. } } if axis == "verify"
        ),
        "ATTACK: a failed verify was not classed on the verify axis: {r:?}"
    );
}
