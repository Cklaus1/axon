//! R51 §4 Fork 1, §7, §11 S0: the refuse-all pin. While the runtime seal is
//! incomplete (`SEAL_RUNTIME_COMPLETE == false`), every sealed run is refused,
//! whichever engine is asked for: in-crate (every run entry builds through
//! `Interp::build`) and through `axon test --seal` (exit 2, the refusal on
//! stderr, no result or summary line on stdout).
//!
//! Its own test binary: the sealed set is process-global, so setting it here
//! cannot reach any other test.

use axon_core::interp::{self, PropertyOutcome, SEALED_RUN_REFUSED, SEAL_RUNTIME_COMPLETE};
use std::path::{Path, PathBuf};
use std::process::Command;

const SUITE: &str = "fn double(x: i64) -> i64 { x * 2 }

@[test]
fn t_ok() { assert_eq(double(21), 42) }

fn t_prop(x: i64) { assert_eq(x - x, 0) }

fn gate() -> bool { true }

fn main() { println(\"ran\") }
";

fn scratch(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("r51-refuse-all-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("cand")).unwrap();
    std::fs::create_dir_all(d.join("suite")).unwrap();
    std::fs::write(
        d.join("cand/f.ax"),
        "pub fn double(x: i64) -> i64 { x * 2 }\n",
    )
    .unwrap();
    std::fs::write(
        d.join("suite/accept.ax"),
        "mod f\nuse f.{double}\n\n@[test]\nfn t_ok() { assert_eq(double(21), 42) }\n",
    )
    .unwrap();
    d
}

// S6 flips the pin; this file then goes (it stops compiling first).
const _: () = assert!(!SEAL_RUNTIME_COMPLETE);

/// Every in-crate run entry refuses a sealed run, under every engine choice.
/// Control: the same entries run the same program once the sealed set is empty.
#[test]
fn a_sealed_in_crate_run_is_refused_on_every_entry_and_engine() {
    let prog = axon_core::parse_source(SUITE).expect("parses");
    let d = scratch("in-crate");

    // Control first: unsealed, every entry runs.
    axon_core::resolver::set_sealed_module_dirs(&[]);
    assert_eq!(
        interp::run_test_fn(&prog, "t_ok"),
        Ok(()),
        "control: run_test_fn"
    );
    assert_eq!(
        interp::run_program_capturing(&prog),
        (0, "ran\n".to_string()),
        "control"
    );
    assert!(
        matches!(
            interp::run_property_test(&prog, "t_prop", 3),
            PropertyOutcome::Passed { .. }
        ),
        "control: run_property_test"
    );
    assert_eq!(
        interp::run_named_fn_as_bool(&prog, "gate"),
        Some(0),
        "control: gate"
    );

    axon_core::resolver::set_sealed_module_dirs(&[d.join("cand")]);
    for engine in [None, Some("vm"), Some("tree")] {
        // The only test here that reads the process environment (the CLI
        // test clears its children's).
        match engine {
            Some(e) => std::env::set_var("AXON_ENGINE", e),
            None => std::env::remove_var("AXON_ENGINE"),
        }
        let at = format!("AXON_ENGINE={engine:?}");
        assert_eq!(
            interp::run_test_fn(&prog, "t_ok"),
            Err(SEALED_RUN_REFUSED.to_string()),
            "{at}: run_test_fn"
        );
        // `run_program` is the same `run_program_inner` without the capture;
        // calling it by that path here would make this file a "runner" to
        // tests/preflight_reachability.rs.
        let (code, out) = interp::run_program_capturing(&prog);
        assert_eq!(
            (code, out.as_str()),
            (2, ""),
            "{at}: run_program_capturing ran sealed code"
        );
        match interp::run_property_test(&prog, "t_prop", 3) {
            PropertyOutcome::Unsupported(m) => assert_eq!(m, SEALED_RUN_REFUSED, "{at}"),
            _ => panic!("{at}: run_property_test ran a sealed run"),
        }
        assert_eq!(
            interp::run_named_fn_as_bool(&prog, "gate"),
            Some(2),
            "{at}: gate"
        );
    }
    std::env::remove_var("AXON_ENGINE");
    axon_core::resolver::set_sealed_module_dirs(&[]);
    let _ = std::fs::remove_dir_all(&d);
}

fn axon_test(d: &Path, seal: bool, engine: Option<&str>, json: bool) -> std::process::Output {
    let mut c = Command::new(env!("CARGO_BIN_EXE_axon"));
    c.current_dir(d.join("suite"))
        .arg("test")
        .arg(d.join("suite/accept.ax"))
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env(
            "AXON_PATH",
            format!("{}:{}", d.join("suite").display(), d.join("cand").display()),
        )
        .env("AXON_PATH_EXCLUSIVE", "1");
    if json {
        c.arg("--json");
    }
    if seal {
        c.arg("--seal").arg(d.join("cand"));
    }
    if let Some(e) = engine {
        c.env("AXON_ENGINE", e);
    }
    c.output().unwrap()
}

/// `axon test --seal` refuses with exit 2, the refusal on stderr and nothing
/// on stdout (no result line, no summary), with and without `AXON_ENGINE=vm`.
/// Control: the same suite, unsealed, passes with a summary.
#[test]
fn axon_test_seal_is_refused_with_no_summary_under_every_engine() {
    let d = scratch("cli");
    let control = axon_test(&d, false, None, true);
    let stdout = String::from_utf8_lossy(&control.stdout);
    assert_eq!(control.status.code(), Some(0), "control: {stdout}");
    assert!(stdout.contains("\"type\":\"summary\""), "control: {stdout}");

    for engine in [None, Some("vm"), Some("tree")] {
        for json in [true, false] {
            let out = axon_test(&d, true, engine, json);
            let (so, se) = (
                String::from_utf8_lossy(&out.stdout),
                String::from_utf8_lossy(&out.stderr),
            );
            let at = format!("AXON_ENGINE={engine:?} json={json}");
            assert_eq!(out.status.code(), Some(2), "{at}: {so}{se}");
            assert!(se.contains(SEALED_RUN_REFUSED), "{at}: stderr: {se}");
            assert!(
                !so.contains("summary") && !so.contains("t_ok"),
                "{at}: stdout: {so}"
            );
        }
    }
    let _ = std::fs::remove_dir_all(&d);
}
