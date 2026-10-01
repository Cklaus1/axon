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

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use std::path::{Path, PathBuf};
use std::process::Command;

use axon_cortex::runner::{
    CheckExecutor, CheckRefusal, CheckRegistry, CheckReport, CheckRequest, CheckVerdict,
    LocalInterpreterExecutor, Runner, LOCAL_AXON_TEST_ID,
};

fn axon_bin() -> PathBuf {
    // The interpreter as cargo has made it current for THIS tree, never a
    // stale `target/debug/axon` (tests/script_spawn::workspace_bin).
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
    common::write_executable(
        &p,
        format!(
            "#!/bin/sh\ntouch '{}'\nexec '{}' \"$@\"\n",
            marker.display(),
            axon_bin().display()
        ),
        0o755,
    );
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
    common::write_executable(&wrapper, "#!/bin/sh\ntouch /dev/null\n", 0o755);
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

// ── C9 round 2, LOOP workstream: one suite-id parser (PSV-5, M384) ──────────

/// A check suite id holding a separator of `check-suite:<id>@<version>#<entry>`
/// is never registered, by a library caller or from a registry file: the
/// loop's pins would read `check-suite:acceptance@x@<v>#e` as suite
/// `acceptance` while Fabric's manifest named `acceptance@x`. Control: a plain
/// id registers.
#[test]
fn a_check_suite_id_holding_a_reference_separator_is_never_registered() {
    use axon_cortex::runner::{CheckVisibility, RegisteredCheck};
    let check = |id: &str| RegisteredCheck {
        id: id.into(),
        visibility: CheckVisibility::Hidden,
        root: "/nowhere".into(),
        entry: "accept.ax".into(),
        workspace_version_ref: format!("acf1:{}", "1".repeat(64)),
    };
    let mut reg = CheckRegistry::new();
    reg.register_check(check("acceptance"))
        .expect("control: a plain id registers");
    for id in ["acceptance@x", "acc#e", "a:b", "a/b", "a b", ""] {
        if reg.register_check(check(id)).is_ok() {
            panic!("ATTACK: check suite id {id:?} holding a reference separator was REGISTERED");
        }
        assert!(reg.check(id).is_none(), "{id:?}");
    }
    let dir = std::env::temp_dir().join(format!("c9r2-suite-id-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let f = dir.join("r.json");
    std::fs::write(
        &f,
        serde_json::json!({"schema":"cortex-check-registry/1","executors":[],"checks":[{
            "id":"acceptance@x","visibility":"hidden","root":"/r","entry":"h.ax",
            "workspace_version_ref": format!("acf1:{}", "1".repeat(64))}]})
        .to_string(),
    )
    .unwrap();
    let loaded = CheckRegistry::load(&f);
    let _ = std::fs::remove_dir_all(&dir);
    match loaded {
        Ok(_) => panic!("ATTACK: check suite id \"acceptance@x\" was REGISTERED from a file"),
        Err(e) => assert!(e.contains("is not an id"), "{e}"),
    }
}

/// The one parser reads a reference the one way it can be read, and refuses
/// every reference with a second reading (a separator in the version).
#[test]
fn a_check_suite_reference_parses_one_way_or_not_at_all() {
    use axon_cortex::runner::{check_suite_ref, parse_check_suite_ref};
    let v = format!("acf1:{}", "5".repeat(64));
    let r =
        check_suite_ref("acceptance", &v, "accept.ax").expect("a plain suite has its reference");
    assert_eq!(
        parse_check_suite_ref(&r).unwrap(),
        ("acceptance", v.as_str(), "accept.ax")
    );
    for bad in [
        format!("check-suite:acceptance@x@{v}#accept.ax"),
        format!("check-suite:@{v}#accept.ax"),
        "check-suite:acceptance@#accept.ax".to_string(),
        format!("check-suite:acceptance@{v}#"),
        format!("check-suite:acceptance@{v}"),
        format!("check:acceptance@{v}#accept.ax"),
    ] {
        assert!(parse_check_suite_ref(&bad).is_err(), "{bad}");
    }
}

// ── C9 round 3, loop workstream (PSV-5, A81): a reference is written only if
// it reads back as the suite it was written for ──────────────────────────────

/// The writer refuses a suite whose reference the one parser reads as another
/// suite: (version `V#x`, entry `accept.ax`) formats to
/// `check-suite:acceptance@V#x#accept.ax`, which reads as (version `V`, entry
/// `x#accept.ax`). Control: the pin's own reading writes and round-trips.
#[test]
fn a_suite_reference_is_written_only_if_it_reads_back_as_that_suite() {
    use axon_cortex::runner::{check_suite_ref, parse_check_suite_ref};
    let v = format!("acf1:{}", "5".repeat(64));
    let own = check_suite_ref("acceptance", &v, "x#accept.ax")
        .expect("control: an entry holding `#` has one reading");
    assert_eq!(
        parse_check_suite_ref(&own).unwrap(),
        ("acceptance", v.as_str(), "x#accept.ax")
    );
    for (ver, entry) in [
        (format!("{v}#x"), "accept.ax"),
        (format!("{v}@x"), "accept.ax"),
    ] {
        if let Ok(r) = check_suite_ref("acceptance", &ver, entry) {
            panic!(
                "ATTACK: suite version {ver:?} entry {entry:?} was written as {r}, which the \
                 parser reads as {:?}: the writer made a second reading",
                parse_check_suite_ref(&r)
            );
        }
    }
    assert!(
        check_suite_ref("acceptance", &v, "").is_err(),
        "empty entry"
    );
    assert!(
        check_suite_ref("acceptance", "", "accept.ax").is_err(),
        "empty version"
    );
}

/// A library registration of a suite whose version holds a reference
/// separator is refused: its reference would read as another suite (the file
/// loader also requires acf1 hex). Control: the plain version registers.
#[test]
fn a_suite_version_holding_a_reference_separator_is_never_registered() {
    use axon_cortex::runner::{CheckVisibility, RegisteredCheck};
    let v = format!("acf1:{}", "1".repeat(64));
    let check = |version: String| RegisteredCheck {
        id: "acceptance".into(),
        visibility: CheckVisibility::Hidden,
        root: "/nowhere".into(),
        entry: "accept.ax".into(),
        workspace_version_ref: version,
    };
    let mut reg = CheckRegistry::new();
    reg.register_check(check(v.clone()))
        .expect("control: a plain version registers");
    for bad in [format!("{v}#x"), format!("{v}@x")] {
        if reg.register_check(check(bad.clone())).is_ok() {
            panic!("ATTACK: suite version {bad:?} holding a reference separator was REGISTERED");
        }
    }
}
