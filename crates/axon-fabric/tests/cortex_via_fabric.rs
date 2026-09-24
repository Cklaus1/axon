//! A real production caller: the `cortex` binary with `--fabric-journal`, whose
//! every check dispatch goes through the registry-pinned `axon-fabric submit`
//! binary, the axon-loop epoch store, the journal, and the real interpreter.
//!
//! The `cortex` binary is built by `cargo build -p axon-cortex --bins`; this
//! test FAILS (never skips) if it is absent, because a production-caller test
//! that silently skips proves nothing.

mod common;
use common::*;

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;

fn cortex_bin() -> PathBuf {
    workspace_bin(
        "cortex",
        "CORTEX_BIN",
        "run `cargo build -p axon-cortex --bins` first",
    )
}

const BROKEN: &str = "\
fn double(n: i64) -> i64 { n + 2 }

@[test]
fn t_small() { assert_eq(double(2), 4) }

@[test]
fn hidden_completion() { assert_eq(double(3), 6) }
";

struct Setup {
    env: Env,
    registry: PathBuf,
}

fn setup() -> Setup {
    let env = Env::new();
    std::fs::write(env.ws.join("broken.ax"), BROKEN).unwrap();
    let registry = env.dir.path().join("cortex-registry.json");
    let fabric = PathBuf::from(env!("CARGO_BIN_EXE_axon-fabric"));
    write_registry(&registry, &env.exe, Some(&fabric));
    Setup { env, registry }
}

fn cortex(s: &Setup, args: &[&str], epoch: u64) -> (i32, Value, String) {
    let e = &s.env;
    let out = Command::new(cortex_bin())
        .args(args)
        .arg("--workspace")
        .arg(&e.ws)
        .arg("--check-registry")
        .arg(&s.registry)
        .arg("--fabric-journal")
        .arg(&e.journal)
        .arg("--fabric-store")
        .arg(&e.store)
        .args(["--fabric-tenant", "tenant-t", "--fabric-family", "family-f"])
        .arg("--fabric-epoch")
        .arg(epoch.to_string())
        .arg("--fabric-grant-registry")
        .arg(&e.grant_registry)
        .args([
            "--fabric-principal",
            PRINCIPAL,
            "--fabric-grant-ref",
            "grant:test",
        ])
        .arg("--fabric-policy-digest")
        .arg(format!("acf1:{}", "c".repeat(64)))
        // --axon is ignored under a registry; point it somewhere useless to
        // prove the registered binary is what runs.
        .args(["--axon", "/bin/false", "--json"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let v = stdout
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l).ok())
        .unwrap_or(Value::Null);
    (
        out.status.code().unwrap_or(-1),
        v,
        format!("{stdout}{}", String::from_utf8_lossy(&out.stderr)),
    )
}

fn journal_ops(journal: &Path) -> Vec<Value> {
    std::fs::read_to_string(journal)
        .unwrap_or_default()
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l).ok())
        .collect()
}

#[test]
fn cortex_repair_adjudicates_through_the_fabric() {
    let s = setup();
    let (code, v, text) = cortex(
        &s,
        &[
            "repair",
            "--file",
            "broken.ax",
            "--symbol",
            "double",
            "--check",
            "hidden_completion",
            "--write-prefix",
            "broken.ax",
            "--generator",
            "literal:n * 2",
        ],
        0,
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(v["outcome"], "verified_done", "{text}");

    // Every check the loop ran went through the Fabric and has a receipt.
    let receipts = v["receipts"]
        .as_array()
        .expect("receipts under --fabric-journal");
    assert!(
        receipts.len() >= 3,
        "pre-flight + check + verify at least: {text}"
    );
    for r in receipts {
        assert_eq!(r["schema"], "acf-execution-receipt/1");
        assert_eq!(r["backend_profile_ref"], "process_scoped/local-interpreter");
        assert_eq!(r["evidence_source"], "supervisor_observed");
        assert_eq!(r["cost_micro"], Value::Null);
    }
    // The last dispatch is the adjudication of the repaired bytes: passed.
    let last = receipts.last().unwrap();
    assert_eq!(last["verification"], "passed", "{last}");
    assert_eq!(last["status"], "completed");
    // And its workspace ref binds the REPAIRED file, not the broken one.
    let repaired = std::fs::read(s.env.ws.join("broken.ax")).unwrap();
    assert_eq!(
        last["input_workspace_ref"],
        axon_cortex::runner::fabric_workspace_digest("broken.ax", &repaired)
    );
    assert_ne!(
        receipts[0]["input_workspace_ref"], last["input_workspace_ref"],
        "the pre-flight judged different bytes"
    );

    // The journal saw launch-before-effect for every one, and the spawns of
    // the registered interpreter equal the launches (no bypass).
    let recs = journal_ops(&s.env.journal);
    let launches = recs.iter().filter(|r| r["kind"] == "launched").count();
    assert_eq!(launches, receipts.len());
    // Every `axon test` (a CHECK) went through a Fabric launch. `axon check`
    // is the Runner's observation step (diagnostics, not a verdict) and is
    // not a registered check, so it is counted separately.
    let log = std::fs::read_to_string(&s.env.spawns).unwrap();
    let tests = log.lines().filter(|l| *l == "test").count();
    let others: Vec<&str> = log
        .lines()
        .filter(|l| *l != "test" && *l != "check")
        .collect();
    assert_eq!(tests, launches, "every check spawn is a journalled launch");
    assert!(others.is_empty(), "unexpected invocations: {others:?}");
}

#[test]
fn cortex_under_a_stale_epoch_runs_nothing() {
    let s = setup();
    s.env.bump_epoch(); // authority moved to 1; the run claims 0
    let (code, _v, text) = cortex(
        &s,
        &[
            "locate",
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
        ],
        0,
    );
    assert_eq!(code, 22, "an unrunnable check is exit 22: {text}");
    assert!(text.contains("stale authority epoch"), "{text}");
    assert_eq!(spawn_count(&s.env.spawns), 0, "no interpreter spawned");
    assert_eq!(s.env.launch_records(), 0, "no launch record");
}

#[test]
fn cortex_locate_through_the_fabric_reports_receipts() {
    let s = setup();
    let (code, v, text) = cortex(
        &s,
        &[
            "locate",
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
        ],
        0,
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(v["schema"], "cortex-locate/1");
    assert_eq!(v["receipts"].as_array().map(|a| a.len()), Some(1), "{v}");
    assert_eq!(v["passing"], serde_json::json!(["t_small"]));
}

/// D-016: fabric mode takes principal, grant_ref and policy digest as
/// EXPLICIT operator input. It used to hard-code principal `cortex:repair`
/// and an all-zero policy digest. Absent any of them — or with the zero
/// placeholder — cortex refuses to start: nothing spawned, no journal.
#[test]
fn cortex_fabric_mode_refuses_to_start_without_explicit_authority() {
    let flags = [
        "--fabric-grant-registry",
        "--fabric-principal",
        "--fabric-grant-ref",
        "--fabric-policy-digest",
    ];
    for missing in flags {
        let s = setup();
        let e = &s.env;
        let mut cmd = Command::new(cortex_bin());
        cmd.args([
            "locate",
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
        ])
        .arg("--workspace")
        .arg(&e.ws)
        .arg("--check-registry")
        .arg(&s.registry)
        .arg("--fabric-journal")
        .arg(&e.journal)
        .arg("--fabric-store")
        .arg(&e.store)
        .args(["--fabric-tenant", "tenant-t", "--fabric-family", "family-f"])
        .args(["--fabric-epoch", "0"]);
        let gr = e.grant_registry.display().to_string();
        let digest = format!("acf1:{}", "c".repeat(64));
        for (f, v) in [
            ("--fabric-grant-registry", gr.as_str()),
            ("--fabric-principal", PRINCIPAL),
            ("--fabric-grant-ref", "grant:test"),
            ("--fabric-policy-digest", digest.as_str()),
        ] {
            if f != missing {
                cmd.args([f, v]);
            }
        }
        let out = cmd.output().unwrap();
        let err = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(2), "{missing}: {err}");
        assert!(err.contains(missing), "{missing}: {err}");
        assert_eq!(spawn_count(&e.spawns), 0, "{missing}: nothing spawned");
        assert!(!e.journal.exists(), "{missing}: no journal");
    }

    // The zero placeholder is refused at construction (exit 22, nothing run).
    let s = setup();
    let e = &s.env;
    let out = Command::new(cortex_bin())
        .args([
            "locate",
            "--file",
            "broken.ax",
            "--check",
            "hidden_completion",
        ])
        .arg("--workspace")
        .arg(&e.ws)
        .arg("--check-registry")
        .arg(&s.registry)
        .arg("--fabric-journal")
        .arg(&e.journal)
        .arg("--fabric-store")
        .arg(&e.store)
        .args(["--fabric-tenant", "tenant-t", "--fabric-family", "family-f"])
        .args(["--fabric-epoch", "0"])
        .arg("--fabric-grant-registry")
        .arg(&e.grant_registry)
        .args([
            "--fabric-principal",
            PRINCIPAL,
            "--fabric-grant-ref",
            "grant:test",
        ])
        .arg("--fabric-policy-digest")
        .arg(format!("acf1:{}", "0".repeat(64)))
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(22), "{err}");
    assert!(err.contains("placeholder"), "{err}");
    assert_eq!(spawn_count(&e.spawns), 0, "nothing spawned");
    assert!(!e.journal.exists(), "no journal");
}
