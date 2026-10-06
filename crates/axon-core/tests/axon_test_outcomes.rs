//! C9 round 4c, EQGATE (amendment 81; M1936-M1938): what `axon test` reports
//! as a FAILURE. In the guest, `axon test` is the entry that issues the
//! completion evidence for a protected check, and its per-test outcome is
//! `(passed, Some(message))`: three of its failure arms build their message
//! with `Some(format!(..))` rather than an `Err`, so the refusal-coverage gate
//! did not see them, and no test ran them.
//!
//! One program holds a test of each shape; every test but `ok` must be
//! reported `failed`, and `ok` must still be reported `ok`.

mod common;
use std::process::Command;

const PROGRAM: &str = r#"
@[test]
fn ok() { assert(true) }

@[test]
fn plain_failure() { assert(false) }

@[test(should_fail)]
fn expected_panic_that_does_not_happen() { assert(true) }

@[test(should_fail)]
@[forall(n: 5)]
fn expected_failing_property_that_holds(x: i64) { assert(true) }

@[test]
@[forall(n: 5)]
fn failing_property(x: i64) { assert(false) }
"#;

/// name -> (status, message), from `axon test --json`.
fn outcomes() -> std::collections::BTreeMap<String, (String, String)> {
    let d = std::env::temp_dir().join(format!("axon-test-outcomes-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join("t.ax");
    std::fs::write(&f, PROGRAM).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axon"))
        .current_dir(&d)
        .arg("test")
        .arg(&f)
        .arg("--json")
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .output()
        .unwrap();
    let mut m = std::collections::BTreeMap::new();
    for l in String::from_utf8_lossy(&out.stdout).lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(l) else {
            continue;
        };
        if let Some(n) = v["name"].as_str() {
            m.insert(
                n.to_string(),
                (
                    v["status"].as_str().unwrap_or("").to_string(),
                    v["message"].as_str().unwrap_or("").to_string(),
                ),
            );
        }
    }
    assert!(
        !out.status.success(),
        "a run with failing tests exits non-zero: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let _ = std::fs::remove_dir_all(&d);
    m
}

#[test]
fn a_passing_test_is_reported_ok() {
    let m = outcomes();
    assert_eq!(
        m.get("ok").map(|x| x.0.as_str()),
        Some("ok"),
        "control: {m:?}"
    );
    assert_eq!(
        m.get("plain_failure").map(|x| x.0.as_str()),
        Some("failed"),
        "control: a plain failing test is failed: {m:?}"
    );
}

#[test]
fn a_should_fail_test_that_does_not_panic_is_a_failure() {
    let m = outcomes();
    let got = m.get("expected_panic_that_does_not_happen");
    assert!(
        got.is_some_and(|(s, msg)| s == "failed" && msg.contains("completed without panicking")),
        "ATTACK: a should_fail test that completed without panicking was reported as passing: \
         {got:?}"
    );
}

#[test]
fn a_should_fail_property_that_holds_is_a_failure() {
    let m = outcomes();
    let got = m.get("expected_failing_property_that_holds");
    assert!(
        got.is_some_and(|(s, msg)| s == "failed" && msg.contains("held over")),
        "ATTACK: a should_fail property that held over every case was reported as passing: {got:?}"
    );
}

#[test]
fn a_property_that_fails_is_a_failure() {
    let m = outcomes();
    let got = m.get("failing_property");
    assert!(
        got.is_some_and(|(s, msg)| s == "failed" && msg.contains("property failed")),
        "ATTACK: a property that failed on a counterexample was reported as passing: {got:?}"
    );
}
