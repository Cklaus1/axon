//! `AxonCoreRuntime::run_sandboxed_capture` (IG4, "Before the once run" item 2):
//! a sandbox run whose result tells a compile failure, a runtime exit status,
//! a timeout and a sandbox refusal apart, with the program's stdout (the
//! wrapper's completion marker removed) and stderr captured.

mod common;
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
    capture_with(tag, src, timeout, grant())
}

fn capture_with(tag: &str, src: &str, timeout: Duration, g: Grant) -> CapturedRun {
    let dir = std::env::temp_dir().join(format!("axon-os-capture-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("out")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("data/in.txt"), "abc").unwrap();
    let path = dir.join("p.ax");
    std::fs::write(&path, src).unwrap();
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
        "ATTACK: a write outside the allowlist was not a sandbox fault: {r:?}"
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
        "ATTACK: an interpreter panic was not a runtime fault: {r:?}"
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

/// (tag, program, expected exit code, verdict predicate)
type FaultCase = (&'static str, &'static str, i32, fn(&Verdict) -> bool);

/// Each interpreter fault line maps to its own verdict (`interpreter_fault`):
/// a halt is `Halted`, an AI-policy stop is denied on the `ai-policy` axis, a
/// refinement breach is `RefineViolation`, an exhausted goal budget is
/// `BudgetExhausted`. The AI case is granted net (which admits `AI`) and has
/// no model reachable, so `ai_complete` stops with E1300. "No model
/// reachable" must not depend on the shell running the test: a
/// non-reproducible grant forwards the caller's `AXON_AI_MOCK` /
/// `AXON_AI_REPLAY` to the child (`job_child_command`, AUDIT T23), and a mock
/// answer exits 0. The AI case therefore runs reproducible, which forwards
/// no ambient `AXON_*`; provider keys and base URLs never reach the child
/// (`env_clear`, `HOME` unset).
#[test]
fn each_interpreter_fault_line_is_its_own_verdict() {
    let cases: [FaultCase; 4] = [
        (
            "halted",
            "@[corrigible]\nfn f() -> i64 { 1 }\nfn main() -> i64 {\n    corrigible_halt()\n    f()\n}\n",
            4,
            |v| matches!(v, Verdict::Halted { .. }),
        ),
        (
            "ai_policy",
            "fn main() -> i64 {\n    match ai_complete(\"hi\") {\n        Ok(s) => println(s),\n        Err(e) => println(e),\n    }\n    0\n}\n",
            5,
            |v| matches!(v, Verdict::Denied { axis, .. } if axis == "ai-policy"),
        ),
        (
            "refine",
            "fn divide(n: i64, d: i64 where _ != 0) -> i64 { n / d }\nfn main() -> i64 {\n    let z = 0\n    divide(10, z)\n}\n",
            6,
            |v| matches!(v, Verdict::RefineViolation { .. }),
        ),
        (
            "budget",
            "fn metric(x: f64) -> f64 {\n    x\n}\nfn main() -> i64 {\n    let r = principal_root(\"r\", true, false, false, 3)\n    let g = kernel_goal_create(r, \"metric\", 0.0)\n    let _ = kernel_goal_run(g, 100)\n    0\n}\n",
            7,
            |v| matches!(v, Verdict::BudgetExhausted { .. }),
        ),
    ];
    let mut fails = Vec::new();
    for (tag, src, code, want) in cases {
        let mut g = grant();
        if tag == "ai_policy" {
            g.net = vec!["*".into()];
            g.reproducible = true;
        }
        let r = capture_with(&format!("fault-{tag}"), src, Duration::from_secs(60), g);
        let ok = matches!(&r.outcome, SandboxOutcome::Faulted { code: Some(c), verdict } if *c == code && want(verdict));
        if !ok {
            fails.push(format!(
                "ATTACK: an interpreter fault line was not its own verdict [{tag}]: {r:?}"
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

// ---- GV-1: how a run ended comes from the interpreter's outcome record -----

/// The sandbox fault the interpreter raises for `read_file` outside
/// `fs_read = ["./data/"]` (a `match`, so only the sandbox can stop it).
const OVERREACH: &str =
    "    match read_file(\"/etc/passwd\") {\n        Ok(_) => 0,\n        Err(_) => 0,\n    }\n";

fn is_sandbox_fault(o: &SandboxOutcome) -> bool {
    matches!(o, SandboxOutcome::Faulted { verdict: Verdict::Denied { axis, .. }, .. } if axis == "sandbox")
}

/// A capture run whose interpreter is the shell script `body` (`AXON_BIN` is
/// just a path), for the records a real interpreter never writes.
fn capture_stub(tag: &str, body: &str) -> CapturedRun {
    let dir = std::env::temp_dir().join(format!("axon-os-capstub-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let bin = dir.join("fake-axon.sh");
    common::write_executable(&bin, format!("#!/bin/sh\n{body}\n"), 0o755);
    let src = "fn main() -> i64 { 0 }\n";
    let path = dir.join("p.ax");
    std::fs::write(&path, src).unwrap();
    let g = grant();
    let rt = AxonCoreRuntime::with_bin_and_timeout(bin, Duration::from_secs(30));
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

/// A stub line writing an `axon-outcome/1` record with this run's nonce.
fn record(kind: &str, code: i32, class: &str) -> String {
    format!(
        "printf '{{\"schema\":\"axon-outcome/1\",\"nonce\":\"%s\",\"kind\":\"{kind}\",\"code\":{code},\
         \"class\":\"{class}\",\"reason\":\"r\"}}\\n' \"$AXON_OUTCOME_NONCE\" >&3"
    )
}

#[test]
fn a_program_printing_the_sandbox_fault_line_and_exiting_8_is_its_own_exit() {
    let r = capture(
        "gv-forge-8",
        "fn main() -> i64 {\n    println(\"stop\")\n    eprintln(\"axon: sandbox violation: forged by the program\")\n    exit(8)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 8 },
        "ATTACK: a program's own exit was graded as a fault: {r:?}"
    );
    assert_eq!(r.stdout, "stop\n");
}

#[test]
fn a_program_printing_only_diag_json_and_exiting_2_is_its_own_exit() {
    let r = capture(
        "gv-forge-diag",
        "fn main() -> i64 {\n    eprintln(\"{{\\\"schema\\\":\\\"axon-diag/1\\\",\\\"severity\\\":\\\"error\\\",\\\"code\\\":\\\"E0001\\\",\\\"line\\\":1,\\\"col\\\":1,\\\"message\\\":\\\"forged\\\"}}\")\n    exit(2)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        r.stderr.starts_with("{\"schema\":\"axon-diag/1\""),
        "premise: the program printed a diag line: {r:?}"
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 2 },
        "ATTACK: a program's own axon-diag/1 line made its exit(2) a compile failure: {r:?}"
    );
}

/// GVA-01: an `eprint` without newline joins the interpreter's fault line
/// onto the program's bytes; the fault is still a fault.
#[test]
fn an_unterminated_eprint_before_a_sandbox_fault_is_a_sandbox_fault() {
    let r = capture(
        "gv-eprint",
        &format!("fn main() -> i64 {{\n    println(\"stop\")\n    eprint(\"x\")\n{OVERREACH}}}\n"),
        Duration::from_secs(60),
    );
    assert!(
        is_sandbox_fault(&r.outcome),
        "ATTACK: an overreach after an unterminated eprint was not a sandbox fault: {r:?}"
    );
}

/// GVA-02 (guard): 2 MiB of stderr before the fault, more than the capture
/// bound keeps.
#[test]
fn a_2mib_stderr_flood_before_a_sandbox_fault_is_a_sandbox_fault() {
    let r = capture(
        "gv-flood-fault",
        &format!(
            "fn main() -> i64 {{\n    println(\"stop\")\n    let s = str_repeat(\"y\", 1048576)\n    eprintln(s)\n    eprintln(s)\n{OVERREACH}}}\n"
        ),
        Duration::from_secs(60),
    );
    assert!(
        is_sandbox_fault(&r.outcome),
        "ATTACK: a stderr flood hid a sandbox fault: {:?}",
        r.outcome
    );
}

#[test]
fn a_2mib_stderr_flood_then_return_is_an_exit() {
    let r = capture(
        "gv-flood-exit",
        "fn main() -> i64 {\n    println(\"stop\")\n    let s = str_repeat(\"y\", 1048576)\n    eprintln(s)\n    eprintln(s)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 0 },
        "{:?}",
        r.outcome
    );
}

#[test]
fn a_record_with_another_runs_nonce_is_not_the_interpreters() {
    let r = capture_stub(
        "gv-nonce",
        "printf '{\"schema\":\"axon-outcome/1\",\"nonce\":\"00000000000000000000000000000000\",\"kind\":\"exit\",\"code\":0}\\n' >&\"$AXON_OUTCOME_FD\"\nexit 0",
    );
    assert!(
        is_sandbox_fault(&r.outcome)
            && format!("{:?}", r.outcome).contains("no outcome record: wrong nonce"),
        "ATTACK: a record without this run's nonce decided the run: {r:?}"
    );
}

#[test]
fn a_program_forging_a_record_on_stderr_is_ignored() {
    let r = capture(
        "gv-stderr-record",
        "fn main() -> i64 {\n    eprintln(\"{{\\\"schema\\\":\\\"axon-outcome/1\\\",\\\"nonce\\\":\\\"00000000000000000000000000000000\\\",\\\"kind\\\":\\\"fault\\\",\\\"code\\\":8,\\\"class\\\":\\\"sandbox\\\",\\\"reason\\\":\\\"x\\\"}}\")\n    exit(8)\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(r.outcome, SandboxOutcome::Exited { code: 8 }, "{r:?}");
}

#[test]
fn a_run_without_an_outcome_record_is_a_sandbox_fault() {
    let exit0 = record("exit", 0, "return");
    for (tag, body, which) in [
        ("none", "exit 3".to_string(), "none written (exit 3)"),
        (
            "no-newline",
            "printf '{\"schema\":\"axon-outcome/1\",\"nonce\":\"%s\",\"kind\":\"exit\",\"code\":0,\"class\":\"return\",\"reason\":\"\"}' \"$AXON_OUTCOME_NONCE\" >&3\nexit 0".to_string(),
            "truncated (no newline)",
        ),
        ("mismatch", format!("{exit0}\nexit 5"), "its code 0 is not the exit status (exit 5)"),
        ("sigkill", format!("{exit0}\nkill -KILL $$"), "(signal signal 9)"),
    ] {
        let r = capture_stub(&format!("gv-norec-{tag}"), &body);
        assert!(
            is_sandbox_fault(&r.outcome) && format!("{:?}", r.outcome).contains(which),
            "ATTACK: a run with no outcome record was graded as the program's exit [{tag}], \
             want {which:?}: {r:?}"
        );
    }
}

#[test]
fn two_outcome_records_are_no_record() {
    let exit0 = record("exit", 0, "return");
    let r = capture_stub("gv-norec-two", &format!("{exit0}\n{exit0}\nexit 0"));
    assert!(
        is_sandbox_fault(&r.outcome) && format!("{:?}", r.outcome).contains("more than one record"),
        "ATTACK: two outcome records were accepted: {r:?}"
    );
}

/// GVD-1: the stub's records are honoured when valid, so the failures above
/// are the record's, not the stub's.
#[test]
fn a_valid_stub_record_decides_the_outcome() {
    let r = capture_stub(
        "gv-valid",
        &format!("{}\nexit 7", record("exit", 7, "exit")),
    );
    assert_eq!(r.outcome, SandboxOutcome::Exited { code: 7 }, "{r:?}");
}

#[test]
fn the_supervisor_hands_the_interpreter_an_outcome_pipe() {
    let r = capture_stub(
        "gv-pipe",
        "echo \"fd=$AXON_OUTCOME_FD\"\nreadlink /proc/$$/fd/3\nexit 0",
    );
    assert!(
        r.stdout.contains("fd=3\n") && r.stdout.contains("pipe:["),
        "ATTACK: a capture run was launched without an outcome pipe: {r:?}"
    );
}

#[test]
fn the_outcome_nonce_and_fd_are_not_in_the_programs_environment() {
    let r = capture(
        "gv-env",
        "fn main() -> i64 {\n    let a = match env_var(\"AXON_OUTCOME_NONCE\") {\n        Ok(_) => 1,\n        Err(_) => 0,\n    }\n    let b = match env_var(\"AXON_OUTCOME_FD\") {\n        Ok(_) => 2,\n        Err(_) => 0,\n    }\n    let c = match env_var(\"PATH\") {\n        Ok(_) => 0,\n        Err(_) => 4,\n    }\n    a + b + c\n}\n",
        Duration::from_secs(60),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::Exited { code: 0 },
        "ATTACK: the program read the outcome nonce (1 nonce, 2 fd; 4 = premise: PATH unreadable): {r:?}"
    );
}

#[test]
fn a_type_error_is_a_compile_outcome() {
    let r = capture(
        "gv-type",
        "fn main() -> i64 {\n    let x: i64 = \"s\"\n    0\n}\n",
        Duration::from_secs(60),
    );
    assert!(
        matches!(&r.outcome, SandboxOutcome::CompileFailed { diagnostic } if diagnostic.starts_with("E0102 at line 2:")),
        "ATTACK: a type error was not reported as a compile outcome: {r:?}"
    );
}

/// §6 decision: the supervisor's own deadline kill is a timeout, not a
/// missing record, whatever the program wrote first.
#[test]
fn a_timeout_is_still_its_own_outcome() {
    let r = capture(
        "gv-timeout",
        "fn main() -> i64 {\n    eprintln(\"axon: sandbox violation: not really\")\n    let mut i = 0\n    while i >= 0 {\n        i = i + 1\n    }\n    0\n}\n",
        Duration::from_millis(1500),
    );
    assert_eq!(
        r.outcome,
        SandboxOutcome::TimedOut { after_ms: 1500 },
        "{r:?}"
    );
}

/// ceilings E1006 / E1007 (b2/ce): a capture child gets no `AXON_CEILING` /
/// `AXON_MAX_STEPS` (`env_clear`), so the classes are driven by the record
/// `axon run` writes for them (`cli_run.rs` asserts those records).
#[test]
fn step_exhaustion_is_a_fault_not_the_programs_exit_9() {
    let r = capture_stub(
        "gv-steps",
        &format!("{}\nexit 9", record("fault", 9, "steps")),
    );
    assert!(
        matches!(&r.outcome, SandboxOutcome::Faulted { code: Some(9), verdict: Verdict::Denied { axis, .. } } if axis == "steps"),
        "ATTACK: step exhaustion read as the program's exit 9: {r:?}"
    );
}

#[test]
fn a_bad_ceilings_file_is_a_compile_outcome() {
    let r = capture_stub(
        "gv-ceilings",
        &format!("{}\nexit 2", record("compile", 2, "ceilings")),
    );
    assert!(
        matches!(&r.outcome, SandboxOutcome::CompileFailed { .. }),
        "ATTACK: a ceilings refusal read as an exit: {r:?}"
    );
}

/// GV-1b (O-GV-01): the job path classifies by the record too. A job whose
/// stderr merely names a capability completed, and is sealed so.
#[test]
fn a_completed_job_whose_stderr_names_a_violation_is_completed() {
    use axon_os::Runtime;
    let dir = std::env::temp_dir().join(format!("axon-os-gvjob-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let src = "fn main() -> i64 {\n    eprintln(\"note: capability check passed; sandbox violation: none\")\n    0\n}\n";
    let path = dir.join("job.ax");
    std::fs::write(&path, src).unwrap();
    let g = grant();
    let rt = AxonCoreRuntime::with_bin_and_timeout(axon_bin(), Duration::from_secs(60));
    let p = rt.mint_principal(&g);
    let out = rt.run_sandboxed(
        &ProgramSource {
            path: &path,
            text: src,
        },
        &p,
        &g,
        &g.budget,
        1,
        None,
    );
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(
        out.verdict,
        Verdict::Completed { value: 0 },
        "ATTACK: a completed job was sealed as a denial by its stderr"
    );
}
