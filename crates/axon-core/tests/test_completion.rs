//! Protected Check Isolation: `axon test --completion-key-stdin` issues a
//! completion token ONLY for a pass the interpreter decided itself: a plain
//! body that returned normally, a `should_fail` body whose failure it
//! observed, or a property all of whose cases completed.
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

const SRC: &str = "fn bail(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n\n}\n\
     @[test]\nfn t_done() { assert_eq(bail(0), 0) }\n\
     @[test]\nfn t_err() -> Result<i64, str> { Err(\"no\") }\n\
     @[test]\nfn t_q() -> Result<i64, str> {\n    let n = parse_int(\"x\")?\n    Ok(n)\n}\n\
     @[test]\nfn t_exit() { assert_eq(bail(1), 99) }\n";

fn run(stdin: &str) -> (i32, String) {
    run_src(SRC, stdin)
}

fn run_src(src: &str, stdin: &str) -> (i32, String) {
    // A unique file per call: the tests run in parallel in one process, and a
    // shared path let one truncate the source another child was reading.
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let d = std::env::temp_dir().join(format!("axon_completion_{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    let f = d.join(format!("t{}.ax", N.fetch_add(1, Ordering::Relaxed)));
    std::fs::write(&f, src).unwrap();
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
    // R51 §3 (S0b): an `Err` return or a `?` exit FAILS the test (veto kept
    // it `ok`, untagged), so the run exits 3 (a test failed).
    assert_eq!(code, 3, "{out}");
    assert!(
        line(&out, "t_done").contains(&format!("\"completion\":\"{T_DONE}\"")),
        "{out}"
    );
    for t in ["t_err", "t_q"] {
        let l = line(&out, t);
        assert!(l.contains("\"status\":\"failed\""), "{t}: {l}");
        assert!(l.contains("did not complete"), "{t}: {l}");
    }
    // `exit(0)` below the test still reports ok — but untagged.
    let l = line(&out, "t_exit");
    assert!(l.contains("\"status\":\"ok\""), "t_exit: {l}");
    assert!(
        !l.contains("completion"),
        "t_exit was tagged as completed: {l}"
    );
}

#[test]
fn a_missing_or_short_completion_key_is_refused_before_any_test_runs() {
    for bad in ["", "\n", "00ff\n", "not-hex-not-hex-not-hex-not-hex\n"] {
        let (code, out) = run(bad);
        assert_eq!(code, 2, "{bad:?}: {out}");
        assert!(!out.contains("t_done"), "{bad:?} ran tests: {out}");
    }
}

/// HMAC-SHA256(KEY, "axon-test-completion/1\0" + name), computed with Python
/// `hmac` (which reproduces `T_DONE` above).
const SF_PANICS: &str = "1c17829caa07c3329ca7574fcf04c9922d96f16873d7350eccc56971964031ec";
const SF_Q: &str = "c3aac3ac2fb46b4e2142b42b14afe3c8dda2689c501af28cf814b2ffeddd57ca";
const P_HOLDS: &str = "249607ce7c3f2a42257bd8a0c92cc4b76fe4bf352501b17668ace2e5fa8ddac6";

#[test]
fn a_rejected_should_fail_and_a_completed_property_are_tokened_an_exit_is_not() {
    // Without a token a keyed caller (cortex repair, Fabric) counts the pass as
    // failed, so every file with a should_fail or @[forall] test could never be
    // verified clean. A should_fail pass is tokened only when the program
    // REJECTED the run (a panic, a `?` on `Err`); one an `exit(n)` stopped
    // passes untokened. A property is tokened only when every case completed;
    // a case ending on `Err` fails it, as it fails a plain test.
    let src = "fn bail(n: i64) -> i64 {\n    if n == 1 { exit(1) }\n    if n > 1 { exit(0) }\n    n\n}\n\
         fn cand(a: i64) -> Result<i64, str> { Err(\"no\") }\n\
         @[test(should_fail)]\nfn sf_panics() { assert_eq(bail(0), 1) }\n\
         @[test(should_fail)]\nfn sf_q() -> Result<i64, str> {\n    let v = cand(1)?\n    Ok(v)\n}\n\
         @[test(should_fail)]\nfn sf_exit0() { assert_eq(bail(2), 1) }\n\
         @[test(should_fail)]\nfn sf_exit1() { assert_eq(bail(1), 1) }\n\
         @[test]\n@[forall(n: 20)]\nfn p_holds(a: i64) { assert(wrapping_add(a, 0) == a) }\n\
         @[test]\n@[forall(n: 20)]\nfn p_ret(a: i64) {\n    if a != 12345 { return }\n    assert(false)\n}\n\
         @[test]\n@[forall(n: 20)]\nfn p_exit(a: i64) { assert_eq(bail(2), a) }\n\
         @[test]\n@[forall(n: 20)]\nfn p_q(a: i64) -> Result<i64, str> {\n    let v = cand(a)?\n    assert(v == 999)\n    Ok(v)\n}\n";
    let (code, out) = run_src(src, &format!("{KEY}\n"));
    assert_eq!(code, 3, "{out}");
    for (t, tok) in [("sf_panics", SF_PANICS), ("sf_q", SF_Q), ("p_holds", P_HOLDS)] {
        let l = line(&out, t);
        assert!(l.contains("\"status\":\"ok\""), "{t}: {l}");
        assert!(l.contains(&format!("\"completion\":\"{tok}\"")), "{t}: {l}");
    }
    for t in ["sf_exit1", "p_ret"] {
        let l = line(&out, t);
        assert!(l.contains("\"status\":\"ok\""), "{t}: {l}");
        assert!(!l.contains("completion"), "{t} was tokened: {l}");
    }
    for t in ["sf_exit0", "p_exit", "p_q"] {
        let l = line(&out, t);
        assert!(l.contains("\"status\":\"failed\""), "{t}: {l}");
    }
}
