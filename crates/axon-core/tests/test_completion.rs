//! Protected Check Isolation: `axon test --completion-key-stdin` issues a
//! completion token ONLY for a test whose body returned normally.
//!
//! The verifier (Fabric) hands a per-run secret in on stdin and counts a pass
//! only when it carries a token derived from that secret. So what this binary
//! tags is the whole affirmative-completion guarantee: tagging an `exit(0)` or
//! an `Err` return would put the early end back to counting as a pass.
//! (`governance/specs/v022-protected-check-isolation.md`, surfaces 11-13.)

use std::io::Write;
use std::process::{Command, Stdio};

const KEY: &str = "000102030405060708090a0b0c0d0e0f";
/// HMAC-SHA256(KEY, "axon-test-completion/1\0t_done"), computed independently
/// (Python `hmac`), so the vector pins the construction, not just itself.
const T_DONE: &str = "b85dea18c9ae462e99521708e782bc551d35d21d7b708f05beb5abdf9416c195";

fn run(stdin: &str) -> (i32, String) {
    // A unique file per call: the tests run in parallel in one process, and a
    // shared path let one truncate the source another child was reading.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("axon_completion_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(format!("t{}.ax", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(
        &f,
        "fn bail(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n\n}\n\
         @[test]\nfn t_done() { assert_eq(bail(0), 0) }\n\
         @[test]\nfn t_err() -> Result<i64, str> { Err(\"no\") }\n\
         @[test]\nfn t_q() -> Result<i64, str> {\n    let n = parse_int(\"x\")?\n    Ok(n)\n}\n\
         @[test]\nfn t_exit() { assert_eq(bail(1), 99) }\n",
    )
    .unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_axon"))
        .args(["test", "--json", "--jobs", "1", "--completion-key-stdin"])
        .arg(&f)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(stdin.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

fn line<'a>(out: &'a str, name: &str) -> &'a str {
    out.lines()
        .find(|l| l.contains(&format!("\"name\":\"{name}\"")))
        .unwrap_or_else(|| panic!("no result line for {name}:\n{out}"))
}

#[test]
fn only_a_completed_test_is_issued_a_completion_token() {
    let (code, out) = run(&format!("{KEY}\n"));
    assert_eq!(code, 0, "{out}");
    assert!(
        line(&out, "t_done").contains(&format!("\"completion\":\"{T_DONE}\"")),
        "{out}"
    );
    for t in ["t_err", "t_q", "t_exit"] {
        let l = line(&out, t);
        // Still reported ok (existing `axon test` semantics) — but untagged.
        assert!(l.contains("\"status\":\"ok\""), "{t}: {l}");
        assert!(
            !l.contains("completion"),
            "{t} was tagged as completed: {l}"
        );
    }
}

#[test]
fn a_missing_or_short_completion_key_is_refused_before_any_test_runs() {
    for bad in ["", "\n", "00ff\n", "not-hex-not-hex-not-hex-not-hex\n"] {
        let (code, out) = run(bad);
        assert_eq!(code, 2, "{bad:?}: {out}");
        assert!(!out.contains("t_done"), "{bad:?} ran tests: {out}");
    }
}
