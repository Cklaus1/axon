//! `benchmarks/drivers/claude_cli.sh` — the documented way to put a real model
//! behind `--generator cmd:` — run against a stub `claude`, so no model, key or
//! network is involved.
//!
//! The driver's Python lives inside one sh single-quoted string. An apostrophe
//! in one of its comments closed that string, sh ran the rest (a backticked
//! `body` became a command substitution), and every call exited 127 with no
//! body: the driver could not return a single proposal, and `cortex repair`
//! reported it as the generator declining.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use std::io::Write;
use std::process::Stdio;

const PROMPT: &str = "You are repairing one function in an Axon program.\n\n\
File: pricing.ax\n\
Function: discount\n\n\
Its body is currently:\n```\nif total > 100 { total / 10 } else { 0 }\n```\n";

/// Run the driver with `claude` on PATH replaced by a stub that prints
/// `reply` as the CLI's JSON envelope. Returns (exit code, stdout, log text).
fn run_driver(tag: &str, reply: &str) -> (Option<i32>, String, String) {
    let dir =
        std::env::temp_dir().join(format!("cortex-claude-driver-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let envelope = serde_json::json!({
        "result": reply,
        "is_error": false,
        "total_cost_usd": 0.001,
        "usage": {"input_tokens": 10, "output_tokens": 5},
        "modelUsage": {"stub-model": {}},
    })
    .to_string();
    std::fs::write(dir.join("envelope.json"), envelope).unwrap();
    let stub = dir.join("claude");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\ncat >/dev/null\ncat '{}'\n",
            dir.join("envelope.json").display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let log = dir.join("log.jsonl");
    let path = format!(
        "{}:{}",
        dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let driver =
        script_spawn::repo_root().join("crates/axon-cortex/benchmarks/drivers/claude_cli.sh");
    let mut child = script_spawn::script("sh", &driver, script_spawn::Bins::NoWorkspaceBinary)
        .env("PATH", path)
        .env("CORTEX_MODEL_LOG", &log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn sh");
    child
        .stdin
        .take()
        .unwrap()
        .write_all(PROMPT.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let log_text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        out.status.code() == Some(0),
        "driver failed: exit {:?}, stderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    (
        out.status.code(),
        String::from_utf8_lossy(&out.stdout).into_owned(),
        log_text,
    )
}

#[test]
fn the_claude_driver_returns_a_bare_body_and_logs_its_candidate() {
    let body = "if total >= 100 { total / 10 } else { 0 }";
    let (_, stdout, log) = run_driver("bare", body);
    assert_eq!(stdout, body);
    let row: serde_json::Value = serde_json::from_str(log.trim()).expect("one JSON log line");
    assert_eq!(row["candidate"], "discount");
    assert_eq!(row["extraction_status"], "raw_code");
    assert_eq!(row["body"], body);
}

#[test]
fn the_claude_driver_extracts_the_last_fence_from_prose() {
    let reply = "The boundary is wrong.\n```axon\nif total >= 100 { total / 10 } else { 0 }\n```";
    let (_, stdout, log) = run_driver("fenced", reply);
    assert_eq!(stdout, "if total >= 100 { total / 10 } else { 0 }\n");
    let row: serde_json::Value = serde_json::from_str(log.trim()).expect("one JSON log line");
    assert_eq!(row["extraction_status"], "prose_plus_fence");
}
