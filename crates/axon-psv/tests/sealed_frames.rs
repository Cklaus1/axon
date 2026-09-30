//! C9 round 3 (amendment 46): candidate bytes cannot end, rewrite or steer
//! the operator's check from BELOW it, through the REAL runner
//! (`axon_psv::runner::run`) and the real interpreter, with the effect
//! ceiling Fabric derives for the developer grant. The host re-derives each
//! outcome under its own key, exactly as `psv::derive` does.
//!
//! * PSV-1: a sealed candidate's effect handler answered or aborted builtins
//!   the OPERATOR's closure performed (keyed pass for the wrong answer 7).
//! * PSV-1, RNG: an operator closure the candidate runs drew from the
//!   operator's stream, so the candidate chose how far it advanced.
//! * PSV-3: a `?` on a type-confused `None` ended the operator's test before
//!   its assert, and a genuine completion token was minted.

use axon_psv::runner::{run, RunnerConfig};
use axon_psv::*;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn axon() -> PathBuf {
    let p = std::env::var_os("AXON_BIN")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let t = std::env::var_os("CARGO_TARGET_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target"));
            t.join("debug/axon")
        });
    assert!(
        p.exists(),
        "these tests need the real interpreter at {} — build it with \
         `cargo build -p axon-core --no-default-features --bin axon` (or set AXON_BIN). \
         This is a FAILURE, not a skip.",
        p.display()
    );
    p
}

/// What the guest decided, the host's keyed outcome, and the test's stdout.
struct Seen {
    status: GuestStatus,
    host: Option<bool>,
    stdout: String,
}

/// Run `test` of `suite` (entry `main.ax`, plus `files` beside it) against
/// `candidate` (`sol.ax`) through the real runner.
fn check(suite: &str, files: &[(&str, &str)], candidate: &str, test: &str) -> Seen {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let (cand, suite_dir, job, out) = (
        d.path().join("candidate"),
        d.path().join("suite"),
        d.path().join("job"),
        d.path().join("out"),
    );
    for p in [&cand, &suite_dir, &job, &out] {
        std::fs::create_dir(p).unwrap();
    }
    std::fs::write(cand.join("sol.ax"), candidate).unwrap();
    std::fs::write(suite_dir.join("main.ax"), suite).unwrap();
    for (name, body) in files {
        std::fs::write(suite_dir.join(name), body).unwrap();
    }
    let secret: [u8; 32] = std::array::from_fn(|i| (i as u8).wrapping_mul(37).wrapping_add(11));
    let sp = job.join("completion-secret");
    std::fs::write(&sp, secret).unwrap();
    let q = Quota::default();
    let sv = axon_workspace_recipe::tree_version_ref(&suite_dir, &q).unwrap();
    let cv = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    let m = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op".into(),
        task_id: "t".into(),
        trial_id: "tr".into(),
        attempt_id: "a".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: "d".repeat(64),
        qualification_sha256: "1".repeat(64),
        host_config_sha256: "2".repeat(64),
        launcher_sha256: "3".repeat(64),
        firecracker_sha256: "4".repeat(64),
        profile_manifest_sha256: "5".repeat(64),
        guest: GuestDigests {
            kernel_sha256: "6".repeat(64),
            rootfs_sha256: "7".repeat(64),
            axon_sha256: "8".repeat(64),
            init_sha256: "9".repeat(64),
        },
        policy_sha256: "a".repeat(64),
        suite: SuiteRef {
            id: "s".into(),
            version: sv.clone(),
            entry: "main.ax".into(),
            test: test.into(),
            tree_digest: sv,
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: cv.clone(),
            tree_digest: cv,
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        authority: AuthorityRef {
            epoch: 0,
            tenant_id: "tenant-t".into(),
            task_family: "family-f".into(),
        },
        limits: Limits {
            wall_time_ms: 120_000,
            output_bytes: 1 << 20,
        },
    };
    std::fs::write(job.join("launch-manifest.json"), m.bytes()).unwrap();
    let axon = axon();
    let cfg = RunnerConfig {
        manifest: job.join("launch-manifest.json"),
        secret: sp,
        candidate: cand,
        suite: suite_dir,
        out,
        axon: axon.clone(),
        runner_exe: axon,
        expected_manifest_sha256: m.digest(),
        drop: None,
        // The developer-profile grant's ceiling as Fabric derives it (Exec is
        // then stripped by the runner).
        effect_ceiling: Some("Net,AI,IO,Exec".into()),
    };
    let v = run(&cfg);
    let stdout = std::fs::read_to_string(cfg.out.join("test-stdout")).unwrap_or_default();
    let host = keyed_outcome(&stdout, test, &completion_key(&secret, &m));
    Seen {
        status: v.status,
        host,
        stdout,
    }
}

/// The operator's listener asserts the value it is handed; the second suite
/// checks it against the operator's fixture file.
const SUITE_PRINT: &str = r#"mod sol
use sol.{visit}

@[test]
fn accept() {
    visit(|v: i64| {
        println("listener saw {to_str(v)}")
        assert_eq(v, 42)
    })
}
"#;
const SUITE_FIXTURE: &str = r#"mod sol
use sol.{visit}

@[test]
fn accept() {
    visit(|v: i64| {
        let raw = match read_file("expected.txt") { Ok(s) => s  Err(e) => "" }
        let want = match parse_int(str_trim(raw)) { Ok(n) => n  Err(e) => -1 }
        assert_eq(v, want)
    })
}
"#;
const FIXTURE: &[(&str, &str)] = &[("expected.txt", "42\n")];
const GOOD: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    cb(42)\n}\n";
const WRONG: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    cb(7)\n}\n";
/// The review's abort variant: the arm returns without resuming at the
/// listener's `println`, skipping its assert.
const ABORT: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    let _ = with handler { on IO(p) => 0 } {\n        cb(7)\n        0\n    }\n}\n";
/// The review's resume variant: the arm answers the operator's `read_file`
/// with bytes the candidate chose.
const RESUME: &str = "pub fn visit(cb: fn(i64) -> ()) {\n    with handler { on IO(p) => resume(Ok(\"7\")) } {\n        cb(7)\n    }\n}\n";

/// Controls: each suite passes the right answer and fails 7 with a keyed
/// verdict, so the attacks below meet a live check.
#[test]
fn the_listener_checks_discriminate() {
    for (suite, files) in [(SUITE_PRINT, &[][..]), (SUITE_FIXTURE, FIXTURE)] {
        let s = check(suite, files, GOOD, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Passed, Some(true)),
            "control: {}",
            s.stdout
        );
        let s = check(suite, files, WRONG, "accept");
        assert_eq!(
            (s.status, s.host),
            (GuestStatus::Failed, Some(false)),
            "control: {}",
            s.stdout
        );
    }
}

#[test]
fn a_sealed_handler_cannot_abort_the_operators_listener() {
    let s = check(SUITE_PRINT, &[], ABORT, "accept");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: the candidate's handler aborted the operator's listener and 7 got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    assert_eq!(s.host, Some(false), "a keyed FAILURE: {}", s.stdout);
}

#[test]
fn a_sealed_handler_cannot_answer_the_operators_fixture_read() {
    let s = check(SUITE_FIXTURE, FIXTURE, RESUME, "accept");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: the candidate's handler chose the operator's fixture bytes and 7 got a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    assert_eq!(s.host, Some(false), "a keyed FAILURE: {}", s.stdout);
}

/// PSV-1, RNG: the operator hands the candidate a `tick` that runs the
/// operator's random search. Under a Fabric ceiling the stream is reached by
/// `goal_run_random` (row `{AI,Net,IO}`), the route C8 steered. The draw is
/// refused, so the check fails closed; before, the tick advanced the
/// operator's stream by however many times the candidate called it.
#[test]
fn operator_code_the_candidate_runs_cannot_draw_the_operators_stream() {
    let suite = r#"mod sol
use sol.{warm}

@[adaptive]
fn probe(x: i64) -> i64 { x }

@[test]
fn accept() {
    warm(|| { let _ = goal_run_random("probe", 1000000.0, 1, 0, 1000000) })
    let found = goal_run_random("probe", 1000000.0, 1, 0, 1000000)
    println("operator challenge {to_str(found)}")
}
"#;
    let steer = "pub fn warm(tick: fn() -> ()) { tick() tick() tick() tick() tick() }\n";
    let s = check(suite, &[], steer, "accept");
    assert!(
        s.stdout.contains("operator's random stream") && s.host != Some(true),
        "ATTACK: an operator closure the candidate ran five times drew from the operator's stream: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // Control: the operator's own draws outside any sealed call still work.
    let idle = "pub fn warm(tick: fn() -> ()) { }\n";
    let s = check(suite, &[], idle, "accept");
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
}

/// PSV-3, the review's exact candidate and suite.
const SUITE_Q: &str = r#"mod sol
use sol.{solve}

@[test]
fn t_solve() -> Result<i64, str> {
    let v = solve(21)?
    assert_eq(v, 42)
    Ok(v)
}
"#;

#[test]
fn a_test_ended_early_by_a_confused_question_mark_gets_no_token() {
    let confusing = "pub fn solve(x: i64) -> Result<i64, str> {\n    let d = dict_new()\n    let n: Option<i64> = None\n    dict_set(d, \"k\", n)\n    match dict_get(d, \"k\") {\n        Some(v) => v\n        None => Err(\"unreachable\")\n    }\n}\n";
    let s = check(SUITE_Q, &[], confusing, "t_solve");
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "ATTACK: a test that ended early at `?` got a completion token and a keyed pass: {:?} {:?} {}",
        s.status,
        s.host,
        s.stdout
    );
    // Controls: an honest pass is Passed and keyed; an honest Err is no pass.
    let s = check(
        SUITE_Q,
        &[],
        "pub fn solve(x: i64) -> Result<i64, str> { Ok(x * 2) }\n",
        "t_solve",
    );
    assert_eq!(
        (s.status, s.host),
        (GuestStatus::Passed, Some(true)),
        "control: {}",
        s.stdout
    );
    let s = check(
        SUITE_Q,
        &[],
        "pub fn solve(x: i64) -> Result<i64, str> { Err(\"no\") }\n",
        "t_solve",
    );
    assert!(
        s.status != GuestStatus::Passed && s.host != Some(true),
        "control: an honest Err is not a pass: {:?} {:?}",
        s.status,
        s.host
    );
}
