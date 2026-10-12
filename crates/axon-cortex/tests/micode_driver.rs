//! `benchmarks/drivers/micode.sh` — the `cmd:` generator that reaches a model
//! through the `micode` CLI — run against a stub `micode`, so no model, grant
//! or network is involved.
//!
//! The stub records how it was invoked. That is the point of the test: the
//! driver's guarantees are about the invocation (no tools, no skills, the
//! requested model, `ANTHROPIC_BASE_URL` unset, a scratch working directory
//! that is removed afterwards), and a wrong invocation still returns a body.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use std::io::Write;
use std::path::Path;
use std::process::Stdio;

const PROMPT: &str = "You are repairing one function in an Axon program.\n\n\
File: pricing.ax\n\
Function: discount\n\n\
Its body is currently:\n```\nif total > 100 { total / 10 } else { 0 }\n```\n";

struct Run {
    code: Option<i32>,
    stdout: String,
    log: String,
    /// What the stub saw: argv, one per line, then `cwd=`, `base_url=`, `mode_file=`.
    seen: String,
}

fn events(reply: &str) -> String {
    let mut lines = vec![
        serde_json::json!({"type": "session_start", "model": "stub-model", "micode_version": "0.0.0"}),
        serde_json::json!({"type": "turn_start", "turn": 0}),
    ];
    // Streamed in pieces, as micode does.
    for chunk in reply.as_bytes().chunks(7) {
        lines.push(serde_json::json!({"type": "assistant_delta",
                                      "text": String::from_utf8_lossy(chunk)}));
    }
    lines.push(serde_json::json!({"type": "turn_end", "usage": {
        "input_tokens": 10, "output_tokens": 5,
        "cache_read_input_tokens": 3, "cache_creation_input_tokens": 2}}));
    lines.iter().map(|v| v.to_string() + "\n").collect()
}

/// Run the driver with `micode` on PATH replaced by a stub that prints
/// `stream` and exits `exit`.
fn run_driver(tag: &str, stream: &str, exit: i32) -> Run {
    let dir =
        std::env::temp_dir().join(format!("cortex-micode-driver-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("stream.jsonl"), stream).unwrap();
    let seen = dir.join("seen.txt");
    let stub = dir.join("micode");
    std::fs::write(
        &stub,
        format!(
            "#!/bin/sh\n\
             for a in \"$@\"; do printf '%s\\n' \"$a\"; done > '{seen}'\n\
             printf 'cwd=%s\\nbase_url=%s\\n' \"$PWD\" \"${{ANTHROPIC_BASE_URL-<unset>}}\" >> '{seen}'\n\
             if [ -f .micode/agents/generate.md ]; then echo mode_file=present >> '{seen}'; fi\n\
             cat '{stream}'\n\
             exit {exit}\n",
            seen = seen.display(),
            stream = dir.join("stream.jsonl").display(),
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
    let driver = script_spawn::repo_root().join("crates/axon-cortex/benchmarks/drivers/micode.sh");
    let mut child = script_spawn::script("sh", &driver, script_spawn::Bins::NoWorkspaceBinary)
        .env("PATH", path)
        .env("TMPDIR", &dir)
        .env("CORTEX_MODEL_LOG", &log)
        .env("CORTEX_MODEL", "stub-requested")
        .env("ANTHROPIC_BASE_URL", "http://127.0.0.1:9/gateway")
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
    let run = Run {
        code: out.status.code(),
        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
        log: std::fs::read_to_string(&log).unwrap_or_default(),
        seen: std::fs::read_to_string(&seen).unwrap_or_default(),
    };
    // The scratch directory the driver ran micode from lives under TMPDIR,
    // i.e. under `dir`; it must be gone.
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.starts_with("cortex-micode-gen"))
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        leftovers.is_empty(),
        "scratch dir left behind: {leftovers:?}"
    );
    run
}

#[test]
fn the_micode_driver_returns_the_body_and_invokes_micode_without_tools() {
    let body = "if total >= 100 { total / 10 } else { 0 }";
    let r = run_driver("bare", &events(body), 0);
    assert_eq!(r.code, Some(0), "log: {}", r.log);
    assert_eq!(r.stdout, body);
    let args: Vec<&str> = r.seen.lines().collect();
    for want in [
        "exec",
        "--no-skills",
        "--mode",
        "generate",
        "provider.model=stub-requested",
        "jsonl",
        "base_url=<unset>",
        "mode_file=present",
    ] {
        assert!(args.contains(&want), "stub did not see `{want}`: {args:?}");
    }
    let cwd = args.iter().find_map(|l| l.strip_prefix("cwd=")).unwrap();
    assert!(
        Path::new(cwd)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("cortex-micode-gen"),
        "micode ran outside the scratch dir: {cwd}"
    );
    let row: serde_json::Value = serde_json::from_str(r.log.trim()).expect("one JSON log line");
    assert_eq!(row["candidate"], "discount");
    assert_eq!(row["extraction_status"], "raw_code");
    assert_eq!(row["transport"], "micode");
    assert_eq!(row["model"][0], "stub-model");
    assert_eq!(row["input_tokens"], 10);
    assert_eq!(row["output_tokens"], 5);
    assert_eq!(row["cache_read"], 3);
    assert_eq!(row["cache_create"], 2);
    assert!(row["cost_usd"].is_null());
}

#[test]
fn the_micode_driver_extracts_the_last_fence_from_prose() {
    let reply = "The boundary is wrong.\n```axon\nif total >= 100 { total / 10 } else { 0 }\n```";
    let r = run_driver("fenced", &events(reply), 0);
    assert_eq!(r.code, Some(0));
    assert_eq!(r.stdout, "if total >= 100 { total / 10 } else { 0 }\n");
    let row: serde_json::Value = serde_json::from_str(r.log.trim()).expect("one JSON log line");
    assert_eq!(row["extraction_status"], "prose_plus_fence");
}

#[test]
fn a_failed_micode_call_exits_non_zero_and_still_logs_its_tokens() {
    let mut stream = events("partial");
    stream.push_str(
        &(serde_json::json!({"type": "session_error", "code": "rate_limited"}).to_string() + "\n"),
    );
    let r = run_driver("error", &stream, 1);
    assert_ne!(r.code, Some(0));
    assert!(
        r.stdout.is_empty(),
        "a failed call returned a body: {}",
        r.stdout
    );
    let row: serde_json::Value = serde_json::from_str(r.log.trim()).expect("one JSON log line");
    assert_eq!(row["extraction_status"], "call_failed");
    assert_eq!(row["input_tokens"], 10);
    assert!(row["session_errors"][0]
        .as_str()
        .unwrap()
        .contains("rate_limited"));
}
