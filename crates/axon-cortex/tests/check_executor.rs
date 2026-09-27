//! B264 — the registered CheckExecutor seam.
//!
//! What is asserted, against the REAL interpreter where a verdict is involved:
//!
//! * the production path (`Runner` → `run_tests_json` → `CheckExecutor`) runs
//!   the registered binary and produces the same verdicts as before;
//! * an UNREGISTERED executor and a DIGEST-CHANGED executable are refused
//!   before anything is spawned — proved by a wrapper whose only observable
//!   effect is creating a marker file, which must not exist afterwards;
//! * the CLI refuses a registry whose stated digest does not match (exit 22,
//!   marker absent), and accepts one that does;
//! * an empty match on a mandatory check is `NotRun`, never `Passed`, and
//!   `verify` fails closed on it.

use std::path::{Path, PathBuf};
use std::process::Command;

use axon_cortex::runner::{
    CheckExecutor, CheckRefusal, CheckRegistry, CheckReport, CheckRequest, CheckVerdict,
    LocalInterpreterExecutor, Runner, LOCAL_AXON_TEST_ID,
};

fn axon_bin() -> PathBuf {
    // Order: `AXON_BIN`, then the target directory this test's own `cortex`
    // binary was built into (how a custom CARGO_TARGET_DIR is honoured), then
    // the workspace `target/debug/`. It FAILS, never skips, when none exists,
    // and an `AXON_BIN` naming a missing file is an error, not a cue to fall
    // back — a fallback that cannot say which binary it ran is an unlogged
    // substitution.
    if let Some(p) = std::env::var_os("AXON_BIN") {
        let p = PathBuf::from(p);
        assert!(p.exists(), "AXON_BIN={} does not exist", p.display());
        return p;
    }
    let own = PathBuf::from(env!("CARGO_BIN_EXE_cortex"))
        .parent()
        .expect("profile dir")
        .join("axon");
    let workspace = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("target/debug/axon");
    let bin = if own.exists() { own } else { workspace };
    assert!(
        bin.exists(),
        "needs the interpreter at {} (or set AXON_BIN); build it with \
         `cargo build -p axon-core --no-default-features --bin axon`. Failing \
         rather than skipping: a test that did not run is not a test that passed.",
        bin.display()
    );
    bin
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "cortex-checkexec-{tag}-{}-{:?}",
        std::process::id(),
        std::thread::current().id()
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

const FIXTURE: &str = "\
fn double(n: i64) -> i64 { n + 2 }

@[test]
fn t_passes() { assert_eq(double(0), 2) }

@[test]
fn hidden_completion() { assert_eq(double(3), 6) }
";

/// A wrapper around the real interpreter whose ONE extra effect is creating
/// `marker`. If the marker exists, the wrapper was spawned.
fn marker_wrapper(dir: &Path, marker: &Path) -> PathBuf {
    let p = dir.join("axon-wrapper.sh");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\ntouch '{}'\nexec '{}' \"$@\"\n",
            marker.display(),
            axon_bin().display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn sha256_of(p: &Path) -> String {
    let out = Command::new("sha256sum").arg(p).output().unwrap();
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .next()
        .unwrap()
        .to_string()
}

#[test]
fn the_production_path_runs_the_registered_interpreter() {
    let ws = tmpdir("prod");
    std::fs::write(ws.join("f.ax"), FIXTURE).unwrap();
    let mut reg = CheckRegistry::new();
    reg.register_pinned(LOCAL_AXON_TEST_ID, &axon_bin())
        .unwrap();
    let exec = LocalInterpreterExecutor::from_registry(&reg).unwrap();
    assert!(exec.id().starts_with("process_scoped/local-interpreter:"));
    assert!(exec.id().contains("@sha256:"));
    let r = Runner::with_check_executor(axon_bin(), &ws, Box::new(exec));

    // The two public entry points that go through run_tests_json.
    assert_eq!(
        r.run_hidden_check("f.ax", "hidden_completion").unwrap(),
        Some(false)
    );
    assert_eq!(r.run_hidden_check("f.ax", "t_passes").unwrap(), Some(true));
    let (failing, passing) = r.check_outcomes("f.ax", "hidden_completion").unwrap();
    assert!(failing.is_empty(), "{failing:?}");
    assert_eq!(passing, vec!["t_passes".to_string()]);
    let _ = std::fs::remove_dir_all(&ws);
}

#[test]
fn an_unregistered_executor_is_refused_before_anything_spawns() {
    let empty = CheckRegistry::new();
    match LocalInterpreterExecutor::from_registry(&empty) {
        Err(CheckRefusal::Unregistered { id }) => assert_eq!(id, LOCAL_AXON_TEST_ID),
        Err(other) => panic!("wrong refusal: {other:?}"),
        Ok(_) => panic!("an empty registry must not yield an executor"),
    }
}

#[test]
fn a_digest_changed_executable_is_refused_with_zero_effects() {
    let dir = tmpdir("digest");
    std::fs::write(dir.join("f.ax"), FIXTURE).unwrap();
    let marker = dir.join("SPAWNED");
    let wrapper = marker_wrapper(&dir, &marker);

    let mut reg = CheckRegistry::new();
    reg.register_pinned(LOCAL_AXON_TEST_ID, &wrapper).unwrap();
    let exec = LocalInterpreterExecutor::from_registry(&reg).unwrap();
    let req = CheckRequest {
        workspace: &dir,
        rel_path: "f.ax",
        filter: Some("hidden_completion"),
    };

    // Sanity: unchanged, it runs (and so DOES create the marker) — which is
    // what makes the marker's absence below mean something.
    let rep = exec.run_checks(&req).expect("registered + unchanged runs");
    assert_eq!(rep.verdict("hidden_completion"), CheckVerdict::Failed);
    assert!(marker.exists(), "the wrapper's effect is observable");
    std::fs::remove_file(&marker).unwrap();

    // Swap the bytes behind the registered path (same path, new content).
    let evil = std::fs::read_to_string(&wrapper)
        .unwrap()
        .replace("exec ", "echo '{\"type\":\"summary\",\"total\":1}'; exec ");
    std::fs::write(&wrapper, evil).unwrap();

    let err = exec
        .run_checks(&req)
        .expect_err("changed bytes must be refused");
    assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(
        err.to_string().contains("changed since registration"),
        "{err}"
    );
    assert!(!marker.exists(), "a refused check must spawn NOTHING");

    // And through the Runner: `verify` fails closed and says DID NOT RUN.
    let mut r = Runner::with_check_executor(&wrapper, &dir, Box::new(exec));
    assert!(!r.verify(true, "hidden_completion", "f.ax"));
    assert!(!marker.exists(), "still nothing spawned");
    let _ = std::fs::remove_dir_all(&dir);
}

/// `Runner::new` (kept for the policy adapter and existing callers) pins on the
/// FIRST dispatch and refuses a later change the same way.
#[test]
fn runner_new_pins_on_first_use_and_refuses_a_later_swap() {
    let dir = tmpdir("lazy");
    std::fs::write(dir.join("f.ax"), FIXTURE).unwrap();
    let marker = dir.join("SPAWNED");
    let wrapper = marker_wrapper(&dir, &marker);
    let r = Runner::new(&wrapper, &dir);
    assert_eq!(r.run_hidden_check("f.ax", "t_passes").unwrap(), Some(true));
    std::fs::remove_file(&marker).unwrap();
    std::fs::write(&wrapper, "#!/bin/sh\ntouch /dev/null\n").unwrap();
    assert!(r.run_hidden_check("f.ax", "t_passes").is_err());
    assert!(!marker.exists());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_registry_file_must_state_the_right_digest() {
    let dir = tmpdir("regfile");
    let reg_path = dir.join("registry.json");
    let axon = axon_bin().canonicalize().unwrap();
    let good = sha256_of(&axon);

    std::fs::write(
        &reg_path,
        serde_json::json!({
            "schema": "cortex-check-registry/1",
            "executors": [{"id": LOCAL_AXON_TEST_ID, "path": axon, "sha256": good}],
        })
        .to_string(),
    )
    .unwrap();
    let reg = CheckRegistry::load(&reg_path).expect("correct digest loads");
    assert_eq!(reg.get(LOCAL_AXON_TEST_ID).unwrap().sha256, good);

    std::fs::write(
        &reg_path,
        serde_json::json!({
            "schema": "cortex-check-registry/1",
            "executors": [{"id": LOCAL_AXON_TEST_ID, "path": axon, "sha256": "00".repeat(32)}],
        })
        .to_string(),
    )
    .unwrap();
    let e = CheckRegistry::load(&reg_path).expect_err("wrong digest refuses the load");
    assert!(e.contains("changed since registration"), "{e}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn cortex(args: &[&str]) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_cortex"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

/// The production binary: a registry naming the wrapper with a WRONG digest is
/// refused at startup (exit 22) and the wrapper never runs; the right digest
/// runs it.
#[test]
fn cli_refuses_a_mismatched_registry_before_spawning() {
    let dir = tmpdir("cli");
    std::fs::write(dir.join("f.ax"), FIXTURE).unwrap();
    let marker = dir.join("SPAWNED");
    let wrapper = marker_wrapper(&dir, &marker);
    let reg_path = dir.join("registry.json");
    let write_reg = |sha: &str| {
        std::fs::write(
            &reg_path,
            serde_json::json!({
                "schema": "cortex-check-registry/1",
                "executors": [{"id": LOCAL_AXON_TEST_ID, "path": wrapper, "sha256": sha}],
            })
            .to_string(),
        )
        .unwrap()
    };
    let ws = dir.to_str().unwrap();
    let rp = reg_path.to_str().unwrap();
    let args = [
        "locate",
        "--workspace",
        ws,
        "--file",
        "f.ax",
        "--check",
        "hidden_completion",
        "--check-registry",
        rp,
        "--axon",
        "/bin/false",
        "--json",
    ];

    write_reg(&"ab".repeat(32));
    let (code, text) = cortex(&args);
    assert_eq!(code, 22, "{text}");
    assert!(text.contains("changed since registration"), "{text}");
    assert!(!marker.exists(), "refused registry must spawn nothing");

    write_reg(&sha256_of(&wrapper));
    let (code, text) = cortex(&args);
    assert_eq!(code, 0, "{text}");
    assert!(marker.exists(), "the REGISTERED binary ran, not --axon");
    assert!(text.contains("cortex-locate/1"), "{text}");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_empty_mandatory_match_is_not_run_never_passed() {
    // Pure report semantics.
    let empty = CheckReport {
        completion: Vec::new(),
        failed: vec![],
        passed: vec![],
        total: 0,
        exit_code: None,
    };
    assert_eq!(empty.verdict("hidden_completion"), CheckVerdict::NotRun);
    let sibling_only = CheckReport {
        completion: Vec::new(),
        failed: vec![],
        passed: vec!["hidden_completion_edge".into()],
        total: 1,
        exit_code: None,
    };
    assert_eq!(
        sibling_only.verdict("hidden_completion"),
        CheckVerdict::NotRun
    );
    let both = CheckReport {
        completion: Vec::new(),
        failed: vec!["x".into()],
        passed: vec!["x".into()],
        total: 2,
        exit_code: None,
    };
    assert_eq!(both.verdict("x"), CheckVerdict::Failed);

    // Against the real interpreter: a filter matching nothing exits 0 with an
    // `ok` summary of total 0 — and that must still read as NotRun.
    let ws = tmpdir("empty");
    std::fs::write(ws.join("f.ax"), FIXTURE).unwrap();
    let mut reg = CheckRegistry::new();
    reg.register_pinned(LOCAL_AXON_TEST_ID, &axon_bin())
        .unwrap();
    let exec = LocalInterpreterExecutor::from_registry(&reg).unwrap();
    let rep = exec
        .run_checks(&CheckRequest {
            workspace: &ws,
            rel_path: "f.ax",
            filter: Some("no_such_check_anywhere"),
        })
        .unwrap();
    assert_eq!(rep.total, 0);
    assert_eq!(rep.verdict("no_such_check_anywhere"), CheckVerdict::NotRun);
    let mut r = Runner::with_check_executor(axon_bin(), &ws, Box::new(exec));
    assert!(
        !r.verify(true, "no_such_check_anywhere", "f.ax"),
        "an empty match must never verify"
    );
    let _ = std::fs::remove_dir_all(&ws);
}
