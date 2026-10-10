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

/// Amendment 103: each part a suite reference lacks is refused BY NAME (the
/// round-10 review found the three `ok_or_else(bad(..))` of the parser labelled
/// "not a protected-route site", though the loop's intake and Fabric's manifest
/// parse call it). The edit that replaces one with a default would otherwise be
/// refused by a LATER term under another message, with every `is_err()` green.
/// And a registry file that names no `executors` array is refused as such.
#[test]
fn a_suite_reference_or_registry_missing_a_part_is_refused_by_that_part() {
    use axon_cortex::runner::{parse_check_suite_ref, CheckRegistry};
    let v = format!("acf1:{}", "5".repeat(64));
    for (r, why) in [
        (format!("acceptance@{v}#accept.ax"), "is not check-suite:"),
        (format!("check-suite:acceptance#{v}"), "names no version"),
        (format!("check-suite:acceptance@{v}"), "names no entry"),
    ] {
        let e = parse_check_suite_ref(&r).err().unwrap_or_else(|| {
            panic!("ATTACK: {r:?} (a reference that {why}) parsed as a suite reference")
        });
        assert!(
            e.contains(why),
            "ATTACK: {r:?} was refused, but not as `{why}` (its own part was not the guard): {e}"
        );
    }
    let d = tmpdir("registry-parts");
    let f = d.join("registry.json");
    std::fs::write(&f, r#"{"schema":"cortex-check-registry/1"}"#).unwrap();
    let e = CheckRegistry::load(&f).err().unwrap_or_else(|| {
        panic!("ATTACK: a check registry with no `executors` array loaded as an empty registry")
    });
    assert!(
        e.contains("no `executors` array"),
        "ATTACK: a registry with no `executors` array was refused, but not as such: {e}"
    );
    std::fs::write(&f, r#"{"schema":"cortex-check-registry/1","executors":[]}"#).unwrap();
    CheckRegistry::load(&f).expect("control: an empty executors array is a registry");
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

/// Amendment 75: the registry FILE route. `CheckRegistry::load` is what the
/// `axon-fabric` binary calls for its suites (the operator's suite registry
/// on a protected host, M141), and it registers no suite id the one id rule
/// (`check_suite_id`) refuses: each separator on its own, including the ones
/// a reference written from the id would still read back (`#`, `:`, `/`,
/// whitespace, empty), so only the id rule stands between them and the
/// registry. Control: a plain id loads.
#[test]
fn a_registry_file_never_registers_a_suite_id_the_id_rule_refuses() {
    let dir = std::env::temp_dir().join(format!("c9r4c-suite-id-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let load = |id: &str| {
        let f = dir.join("registry.json");
        std::fs::write(
            &f,
            serde_json::json!({"schema":"cortex-check-registry/1","executors":[],"checks":[{
                "id": id, "visibility":"hidden","root":"/r","entry":"accept.ax",
                "workspace_version_ref": format!("acf1:{}", "1".repeat(64))}]})
            .to_string(),
        )
        .unwrap();
        CheckRegistry::load(&f)
    };
    let control = load("acceptance");
    let attacks: Vec<_> = ["acc#e", "a:b", "a/b", "a b", "a\tb", ""]
        .into_iter()
        .map(|id| (id, load(id)))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    let reg = control.expect("control: a plain suite id loads");
    assert!(
        reg.check("acceptance").is_some(),
        "control: and is registered"
    );
    for (id, r) in attacks {
        if let Ok(reg) = r {
            panic!(
                "ATTACK: a registry file registered the suite id {id:?}, which the id rule \
                 refuses (registered: {})",
                reg.check(id).is_some()
            );
        }
    }
}

// ── C9 round 7, EQGATE3 (amendment 91): what the local check child is built with
//
// The check child's working directory, its (empty) environment and the output
// it may buffer are builder calls and a size cap that build no `Err`; each was
// removable alone with every suite green. A stand-in "interpreter" records its
// own working directory and environment.

fn recording_exec(dir: &Path, script: &str) -> LocalInterpreterExecutor {
    let exe = dir.join("axon-recorder.sh");
    common::write_executable(&exe, script, 0o755);
    LocalInterpreterExecutor::pin_on_first_use(exe)
}

#[test]
fn the_local_check_child_gets_its_workspace_and_only_the_environment_it_is_given() {
    let dir = tmpdir("env");
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("f.ax"), FIXTURE).unwrap();
    let rec = dir.join("child-state");
    let exec = recording_exec(
        &dir,
        &format!("#!/bin/sh\n{{ pwd; env; }} > '{}'\nexit 0\n", rec.display()),
    )
    .with_clean_env()
    .with_effect_ceiling("IO,Random")
    .with_env("EQ_TRIAL_HOME", "/tmp/trial-home");
    // The recorder prints no verdict: the dispatch refuses it, which is fine;
    // what is judged is the state the child recorded.
    let _ = exec.run_checks(&CheckRequest {
        workspace: &ws,
        rel_path: "f.ax",
        filter: None,
    });
    let text = std::fs::read_to_string(&rec)
        .unwrap_or_else(|e| panic!("setup: the stand-in recorded nothing: {e}"));
    let mut lines = text.lines();
    assert_eq!(
        Path::new(lines.next().unwrap()).canonicalize().unwrap(),
        ws.canonicalize().unwrap(),
        "ATTACK: the check child did not run in its own workspace"
    );
    let vars: std::collections::BTreeMap<&str, &str> =
        lines.filter_map(|l| l.split_once('=')).collect();
    assert_eq!(
        vars.get("AXON_ALLOWED_EFFECTS"),
        Some(&"IO,Random"),
        "ATTACK: the check child did not run under the effect ceiling it was given: {vars:?}"
    );
    assert_eq!(
        vars.get("EQ_TRIAL_HOME"),
        Some(&"/tmp/trial-home"),
        "ATTACK: the check child did not get the environment it was given: {vars:?}"
    );
    let stray: Vec<&&str> = vars
        .keys()
        .filter(|k| {
            ![
                "AXON_ALLOWED_EFFECTS",
                "EQ_TRIAL_HOME",
                "PWD",
                "SHLVL",
                "_",
                "OLDPWD",
            ]
            .contains(k)
        })
        .collect();
    assert!(
        stray.is_empty(),
        "ATTACK: the check child inherited the launcher's environment: {stray:?}"
    );
}

fn peak_rss_kb() -> u64 {
    std::fs::read_to_string("/proc/self/status")
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))
        .and_then(|v| v.split_whitespace().next()?.parse().ok())
        .expect("VmHWM")
}

/// C9 round 7 (eqgate3): run `test` again ALONE in a child of this test binary
/// and say whether THIS process is that child. Peak RSS (`VmHWM`) is a property
/// of the whole process, and a sibling test thread that allocates would be read
/// as the growth under test: a suite run found the flood test failing on a
/// neighbour's memory. Usage: `if !alone("name") { return; }` first.
fn alone(test: &str) -> bool {
    if std::env::var("AXON_EQ_ALONE").as_deref() == Ok(test) {
        return true;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--test-threads=1", "--nocapture"])
        .env("AXON_EQ_ALONE", test)
        .output()
        .unwrap();
    assert!(
        o.status.success() && String::from_utf8_lossy(&o.stdout).contains("1 passed"),
        "{test} failed when run alone:\n{}\n{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    false
}

#[test]
fn the_local_check_child_cannot_make_the_launcher_buffer_its_whole_output() {
    if !alone("the_local_check_child_cannot_make_the_launcher_buffer_its_whole_output") {
        return;
    }
    let dir = tmpdir("cap");
    let ws = dir.join("ws");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("f.ax"), FIXTURE).unwrap();
    let exec =
        recording_exec(&dir, "#!/bin/sh\nhead -c 400000000 /dev/zero\n").with_max_output(4096);
    let before = peak_rss_kb();
    let got = exec.run_checks(&CheckRequest {
        workspace: &ws,
        rel_path: "f.ax",
        filter: None,
    });
    assert!(
        got.is_err(),
        "ATTACK: a flood past the capture bound yielded a verdict: {got:?}"
    );
    let grew = peak_rss_kb().saturating_sub(before);
    assert!(
        grew < 120_000,
        "ATTACK: the launcher buffered the check's flood ({grew} kB of peak growth for a 4 KiB bound)"
    );
}
