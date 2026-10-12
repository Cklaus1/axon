//! `cortex repair --label-log`: one `cortex-label/1` row per run, and nothing
//! about the run changes because of it.
//!
//! The fixture is `ambiguous.ax`: `alpha` and `beta` tie under the one visible
//! check, `alpha` sorts first, `beta` is broken. `literal:\n    n + 2\n` is
//! wrong for `alpha` and right for `beta`, so a walked run attempts both and
//! the hidden check names `beta`.

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use std::path::{Path, PathBuf};

const FIX: &str = "literal:\n    n + 2\n";

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

/// A fresh workspace holding `ambiguous.ax`, unique per test name.
fn workspace(name: &str) -> PathBuf {
    let ws = std::env::temp_dir().join(format!("cortex_labels_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("fixtures/ambiguous.ax");
    std::fs::copy(src, ws.join("ambiguous.ax")).unwrap();
    ws
}

/// `cortex repair --json` on `ambiguous.ax`: (exit, verdict, stderr).
fn run(ws: &Path, env_log: Option<&Path>, extra: &[&str]) -> (i32, serde_json::Value, String) {
    let mut c = common::cortex();
    c.args(["repair", "--workspace"])
        .arg(ws)
        .args(["--file", "ambiguous.ax", "--check", "hidden_completion"])
        .arg("--axon")
        .arg(axon_bin())
        .args(["--write-prefix", "ambiguous.ax", "--json"])
        .args(extra);
    if let Some(p) = env_log {
        c.env("CORTEX_LABEL_LOG", p);
    }
    let out = c.output().expect("run cortex");
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let v = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("no verdict ({e}): {stdout}{stderr}"));
    (out.status.code().unwrap_or(-1), v, stderr)
}

fn rows(p: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(p)
        .unwrap_or_else(|e| panic!("{}: {e}", p.display()))
        .lines()
        .map(|l| serde_json::from_str(l).expect("each line is one JSON object"))
        .collect()
}

fn names(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

#[test]
fn a_verified_run_labels_the_repaired_symbol_and_the_walk() {
    let ws = workspace("verified");
    let before = std::fs::read(ws.join("ambiguous.ax")).unwrap();
    let log = ws.join("labels.jsonl");
    let (code, v, err) = run(
        &ws,
        None,
        &["--generator", FIX, "--label-log", log.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{err}{v}");
    let rows = rows(&log);
    assert_eq!(rows.len(), 1, "one row per run: {rows:?}");
    let r = &rows[0];
    assert_eq!(r["schema"], "cortex-label/1");
    assert_eq!(
        r["truth"], "beta",
        "truth = the symbol the hidden check accepted"
    );
    assert_eq!(
        names(&r["attempted"]),
        ["alpha", "beta"],
        "attempts in walk order"
    );
    assert_eq!(r["outcome"], "verified_done");
    assert_eq!(r["exit_code"], 0);
    assert_eq!(r["file"], "ambiguous.ax");
    assert_eq!(r["check"], "hidden_completion");
    assert_eq!(
        r["file_sha256"],
        sha256(&before),
        "the digest is of the file BEFORE the run, not the repaired one"
    );
    assert_eq!(r["symbol"], serde_json::Value::Null);
    assert_eq!(
        r["generator"], "literal",
        "a literal body never enters the log"
    );
    assert!(!r.to_string().contains("n + 2"), "{r}");
    let loc = &r["localization"];
    assert_eq!(names(&loc["failing"]), ["visible"]);
    assert_eq!(loc["candidates"], 3);
    assert_eq!(names(&loc["eligible"]), ["alpha", "beta"]);
    let ranked: Vec<&str> = loc["ranked"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| {
            assert!(x["score"].is_f64(), "scores are recorded: {x}");
            x["symbol"].as_str().unwrap()
        })
        .collect();
    assert_eq!(ranked, ["alpha", "beta"]);
    assert_eq!(r["ranker"]["used"], "spectrum");
    assert_eq!(names(&r["ranker"]["order"]), ["alpha", "beta"]);
    let props: Vec<(String, u64)> = r["proposals"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["symbol"].as_str().unwrap().to_string(),
                p["proposals"].as_u64().unwrap(),
            )
        })
        .collect();
    assert_eq!(props, [("alpha".to_string(), 1), ("beta".to_string(), 1)]);
    assert_eq!(r["source"], serde_json::Value::Null, "source is opt-in");
    assert_eq!(r["cortex"]["version"], env!("CARGO_PKG_VERSION"));
    assert_eq!(
        r["cortex"]["sha256"],
        sha256(&std::fs::read(env!("CARGO_BIN_EXE_cortex")).unwrap()),
        "the build that wrote the row is identified by its bytes"
    );

    // --label-source: the text as FOUND, and a second run appends.
    std::fs::write(ws.join("ambiguous.ax"), &before).unwrap();
    let (code, _, err) = run(
        &ws,
        None,
        &[
            "--generator",
            FIX,
            "--label-log",
            log.to_str().unwrap(),
            "--label-source",
        ],
    );
    assert_eq!(code, 0, "{err}");
    let rows = self::rows(&log);
    assert_eq!(rows.len(), 2, "rows are appended, never rewritten");
    assert_eq!(rows[1]["source"].as_str().unwrap().as_bytes(), &before[..]);
}

#[test]
fn a_failed_run_has_no_truth() {
    let ws = workspace("failed");
    let log = ws.join("labels.jsonl");
    let (code, v, err) = run(
        &ws,
        None,
        &[
            "--generator",
            FIX,
            "--candidates",
            "1",
            "--label-log",
            log.to_str().unwrap(),
        ],
    );
    assert_ne!(code, 0, "{err}{v}");
    let rows = rows(&log);
    assert_eq!(rows.len(), 1);
    let r = &rows[0];
    assert_eq!(r["truth"], serde_json::Value::Null, "{r}");
    assert_eq!(r["exit_code"], code);
    assert_eq!(r["outcome"], v["outcome"]);
    assert_eq!(names(&r["attempted"]), ["alpha"]);
}

#[test]
fn an_unwritable_log_changes_neither_exit_code_nor_verdict() {
    let ws = workspace("unwritable");
    let log = ws.join("no/such/dir/labels.jsonl");
    let (code, v, err) = run(
        &ws,
        None,
        &["--generator", FIX, "--label-log", log.to_str().unwrap()],
    );
    assert_eq!(code, 0, "{err}{v}");
    assert_eq!(v["outcome"], "verified_done");
    assert_eq!(v["symbol"], "beta");
    let lines: Vec<&str> = err
        .lines()
        .filter(|l| l.starts_with("label log "))
        .collect();
    assert_eq!(lines.len(), 1, "exactly one stderr line: {err}");
    assert!(lines[0].contains("cannot append"), "{err}");

    // The failed-run case too: the exit code is the episode's, not the log's.
    let ws = workspace("unwritable_failed");
    let (code1, v1, err1) = run(
        &ws,
        None,
        &[
            "--generator",
            FIX,
            "--candidates",
            "1",
            "--label-log",
            log.to_str().unwrap(),
        ],
    );
    let (code2, v2, _) = run(&ws, None, &["--generator", FIX, "--candidates", "1"]);
    assert_eq!(code1, code2, "{err1}");
    assert_eq!(v1["outcome"], v2["outcome"]);
}

#[test]
fn the_flag_beats_the_environment_and_neither_means_off() {
    let ws = workspace("precedence");
    let env_log = ws.join("env.jsonl");
    let flag_log = ws.join("flag.jsonl");
    let (code, _, err) = run(
        &ws,
        Some(&env_log),
        &[
            "--generator",
            FIX,
            "--label-log",
            flag_log.to_str().unwrap(),
        ],
    );
    assert_eq!(code, 0, "{err}");
    assert_eq!(rows(&flag_log).len(), 1);
    assert!(
        !env_log.exists(),
        "the flag wins; the env log must not be written"
    );

    let ws = workspace("env_only");
    let env_log = ws.join("env.jsonl");
    let (code, _, err) = run(&ws, Some(&env_log), &["--generator", FIX]);
    assert_eq!(code, 0, "{err}");
    assert_eq!(
        rows(&env_log).len(),
        1,
        "the environment is used without a flag"
    );

    let ws = workspace("off");
    let (code, _, err) = run(&ws, None, &["--generator", FIX]);
    assert_eq!(code, 0, "{err}");
    let stray: Vec<_> = std::fs::read_dir(&ws)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n != "ambiguous.ax")
        .collect();
    assert!(stray.is_empty(), "off writes nothing: {stray:?}");
}

#[test]
fn label_flags_are_validated_before_anything_runs() {
    let ws = workspace("usage");
    let (code, err) = {
        let out = common::cortex()
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args(["--file", "ambiguous.ax", "--check", "hidden_completion"])
            .arg("--axon")
            .arg(axon_bin())
            .args(["--generator", FIX, "--label-source"])
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stderr).to_string(),
        )
    };
    assert_eq!(
        code, 2,
        "--label-source without a log is a usage error: {err}"
    );
    let out = common::cortex()
        .args(["repair", "--workspace"])
        .arg(&ws)
        .args(["--file", "ambiguous.ax", "--check", "hidden_completion"])
        .args(["--label-log", ""])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(2),
        "an empty --label-log is a usage error"
    );
}
