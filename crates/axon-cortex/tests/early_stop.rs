//! The early stop (`axon_cortex::earlystop`): give up sooner on a defect one
//! function body cannot repair, without giving up on one it can.
//!
//! Every row has its control: the same run with `--early-stop off` (or
//! `--inert-limit 0`) must go on to spend what the rule saved, and an
//! in-scope defect must still be repaired under the default.

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use std::path::{Path, PathBuf};

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

/// A workspace holding `src` as `f.ax`, and a `cmd:` generator that logs every
/// request as `FUNCTION:PRIORS` and answers with the body `cases` selects
/// (a `case` arm list over `$f:$n`, `n` = rejected attempts in the prompt).
fn fixture(name: &str, src: &str, cases: &str) -> (PathBuf, PathBuf) {
    let ws = std::env::temp_dir().join(format!("cortex_early_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&ws);
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("f.ax"), src).unwrap();
    let gen = ws.join("gen.sh");
    common::write_executable(
        &gen,
        format!(
            "#!/bin/sh\n\
             p=$(cat)\n\
             f=$(printf '%s\\n' \"$p\" | sed -n 's/^Function: //p')\n\
             n=$(printf '%s\\n' \"$p\" | grep -c 'was REJECTED')\n\
             echo \"$f:$n\" >> \"$(dirname \"$0\")/calls.log\"\n\
             case \"$f:$n\" in\n\
             {cases}\n\
             esac\n"
        ),
        0o755,
    );
    (ws, gen)
}

/// (exit, verdict, stderr, calls)
fn run(ws: &Path, gen: &Path, extra: &[&str]) -> (i32, serde_json::Value, String, Vec<String>) {
    let out = common::cortex()
        .args(["repair", "--workspace"])
        .arg(ws)
        .args(["--file", "f.ax", "--check", "hidden", "--axon"])
        .arg(axon_bin())
        .arg("--generator")
        .arg(format!("cmd:{}", gen.display()))
        .args(["--write-prefix", "f.ax", "--ranker", "spectrum", "--json"])
        .args(extra)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    let stderr = String::from_utf8_lossy(&out.stderr).to_string();
    let v: serde_json::Value = serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("no verdict ({e}): {stdout}{stderr}"));
    let calls = std::fs::read_to_string(ws.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect();
    (out.status.code().unwrap_or(-1), v, stderr, calls)
}

fn strings(v: &serde_json::Value) -> Vec<String> {
    v.as_array()
        .expect("an array")
        .iter()
        .map(|s| s.as_str().unwrap().to_string())
        .collect()
}

/// Two defects in two bodies, one failing visible check for each. No function
/// is reached by both, so no one-body edit can make both pass.
const TWO_BODIES: &str = "\
fn add(a: i64, b: i64) -> i64 { a - b }
fn dbl(n: i64) -> i64 { n }
@[test]
fn vis_add() { assert_eq(add(2, 3), 5) }
@[test]
fn vis_dbl() { assert_eq(dbl(2), 4) }
@[test]
fn hidden() { assert_eq(add(2, 3), 5) assert_eq(dbl(2), 4) }
fn main() { println(to_str(add(1, 1))) }
";

/// The generator answers a body that is wrong for `dbl` and `add` alike and
/// different on every prior count, so a candidate never trips the cycle
/// detector and only a budget or the early stop ends it.
const ALWAYS_WRONG: &str = "*) printf '\\n    a + n + %s\\n' $((n + 10)) ;;";

/// C-ES1: with no function reached by every failing visible check the run ends
/// before any model call, with its own outcome and exit code, and the file is
/// as found. With the rule off, the generator IS asked.
#[test]
fn early_stop_no_common_function_ends_before_any_model_call() {
    let (ws, gen) = fixture("two_bodies", TWO_BODIES, ALWAYS_WRONG);
    let before = std::fs::read_to_string(ws.join("f.ax")).unwrap();
    let (code, v, err, calls) = run(&ws, &gen, &[]);
    assert_eq!(code, 28, "{err}{v}");
    assert_eq!(v["outcome"], "out_of_scope_suspected");
    assert_eq!(v["exit_code"], 28);
    assert!(
        calls.is_empty(),
        "the generator must not be asked at all: {calls:?}"
    );
    assert!(
        v["detail"]
            .as_str()
            .unwrap()
            .contains("no single function is reached by all 2 failing visible check(s)"),
        "{v}"
    );
    assert_eq!(strings(&v["early_stop"]["visible_failing"]).len(), 2);
    assert_eq!(v["early_stop"]["reached_by_all"], serde_json::json!([]));
    assert_eq!(v["generator_context"], serde_json::json!([]));
    assert_eq!(v["attempted"], serde_json::json!([]));
    assert_eq!(
        std::fs::read_to_string(ws.join("f.ax")).unwrap(),
        before,
        "an early stop must leave the file as found"
    );

    // THE CONTROL: off, the walk runs and spends model calls.
    let (code, v, err, calls) = run(&ws, &gen, &["--early-stop", "off"]);
    assert_ne!(code, 28, "{err}{v}");
    assert_ne!(v["outcome"], "out_of_scope_suspected");
    assert!(!calls.is_empty(), "off must ask the generator: {err}");
    assert_eq!(
        std::fs::read_to_string(ws.join("f.ax")).unwrap(),
        before,
        "a failed run restores the file"
    );
}

/// C-ES2: candidates that not every failing check reaches are not asked.
///
/// `b` is the broken one; `a` is reached by one of the two failing checks
/// only. The generator is wrong for `b` on its first proposal and right on
/// its second, so the breadth walk gives `a` a proposal in between unless
/// the rule has pruned it.
#[test]
fn early_stop_prunes_candidates_not_every_failing_check_reaches() {
    let src = "\
fn a(n: i64) -> i64 { n * 2 }
fn b(n: i64) -> i64 { n }
@[test]
fn vis1() { assert_eq(b(1), 2) assert_eq(a(1), 2) }
@[test]
fn vis2() { assert_eq(b(3), 6) }
@[test]
fn hidden() { assert_eq(b(1), 2) assert_eq(b(3), 6) assert_eq(a(1), 2) }
fn main() { println(to_str(a(1))) }
";
    let cases = "b:1) printf '\\n    n * 2\\n' ;;\n*) printf '\\n    n + %s\\n' $((n + 10)) ;;";
    let (ws, gen) = fixture("prune", src, cases);
    let (code, v, err, calls) = run(&ws, &gen, &[]);
    assert_eq!(code, 0, "{err}{v}");
    assert_eq!(v["symbol"], "b");
    assert_eq!(calls, ["b:0", "b:1"], "`a` must never be asked: {err}");
    assert!(strings(&v["early_stop"]["pruned"]).contains(&"a".to_string()));
    assert!(err.contains("not tried, because not every failing check reaches them"));

    // THE CONTROL: off, `a` gets the proposal the breadth walk owes it.
    let (ws, gen) = fixture("prune_off", src, cases);
    let (code, v, err, calls) = run(&ws, &gen, &["--early-stop", "off"]);
    assert_eq!(code, 0, "{err}{v}");
    assert!(
        calls.iter().any(|c| c.starts_with("a:")),
        "with the rule off `a` is walked: {calls:?}"
    );
    assert_eq!(v["early_stop"]["pruned"], serde_json::json!([]));
}

/// One broken body whose visible check keeps failing exactly as before no
/// matter what the generator writes.
const UNMOVED: &str = "\
fn f(n: i64) -> i64 { n }
@[test]
fn vis() { assert_eq(f(1), 2) }
@[test]
fn hidden() { assert_eq(f(1), 2) assert_eq(f(5), 26) }
fn main() { println(to_str(f(1))) }
";

/// C-ES3: two compiled proposals that leave the failing visible check where it
/// was end the candidate. `--inert-limit 0` and `--early-stop off` are the
/// controls: the same generator then runs on past the second proposal.
#[test]
fn early_stop_stops_a_candidate_whose_visible_verdict_does_not_move() {
    let cases = "*) printf '\\n    n + %s\\n' $((n + 10)) ;;";
    let (ws, gen) = fixture("unmoved", UNMOVED, cases);
    let before = std::fs::read_to_string(ws.join("f.ax")).unwrap();
    let (code, v, err, calls) = run(&ws, &gen, &[]);
    assert_eq!(code, 28, "{err}{v}");
    assert_eq!(v["outcome"], "out_of_scope_suspected");
    assert_eq!(
        calls,
        ["f:0", "f:1"],
        "two proposals, not the budget: {err}"
    );
    assert!(
        v["detail"]
            .as_str()
            .unwrap()
            .contains("exactly as they were"),
        "{v}"
    );
    assert_eq!(strings(&v["early_stop"]["stopped_candidates"]), ["f"]);
    assert_eq!(
        std::fs::read_to_string(ws.join("f.ax")).unwrap(),
        before,
        "an early stop must leave the file as found"
    );

    for extra in [&["--inert-limit", "0"][..], &["--early-stop", "off"][..]] {
        let (ws, gen) = fixture("unmoved_ctl", UNMOVED, cases);
        let (code, v, err, calls) = run(&ws, &gen, extra);
        assert_ne!(code, 28, "{extra:?}: {err}{v}");
        assert!(
            calls.len() > 2,
            "{extra:?}: without the rule the generator is asked past two: {calls:?}"
        );
    }

    // The limit is a number the operator sets.
    let (ws, gen) = fixture("unmoved_one", UNMOVED, cases);
    let (code, _, err, calls) = run(&ws, &gen, &["--inert-limit", "1"]);
    assert_eq!(code, 28, "{err}");
    assert_eq!(calls, ["f:0"]);
}

/// C-ES4: an in-scope defect is still repaired. Here the first two proposals
/// MOVE the visible verdict (the visible check passes, the hidden one still
/// refuses) and the third is right; the rule must not read that as stuck.
#[test]
fn early_stop_does_not_stop_a_candidate_whose_verdict_moves() {
    let cases = "f:0) printf '\\n    n * 2\\n' ;;\n\
                 f:1) printf '\\n    n + 1\\n' ;;\n\
                 f:2) printf '\\n    n * n + 1\\n' ;;\n\
                 *) printf '\\n    n\\n' ;;";
    let (ws, gen) = fixture("moves", UNMOVED, cases);
    let (code, v, err, calls) = run(&ws, &gen, &[]);
    assert_eq!(code, 0, "{err}{v}");
    assert_eq!(v["outcome"], "verified_done");
    assert_eq!(calls, ["f:0", "f:1", "f:2"]);
    assert_eq!(v["early_stop"]["stopped_candidates"], serde_json::json!([]));
}

/// C-ES5: the flags are validated before anything runs.
#[test]
fn early_stop_flags_are_validated() {
    let (ws, gen) = fixture("flags", UNMOVED, "*) printf 'n' ;;");
    for extra in [
        &["--early-stop", "maybe"][..],
        &["--inert-limit", "x"][..],
        &["--early-stop", "off", "--inert-limit", "2"][..],
    ] {
        let out = common::cortex()
            .args(["repair", "--workspace"])
            .arg(&ws)
            .args(["--file", "f.ax", "--check", "hidden", "--axon"])
            .arg(axon_bin())
            .arg("--generator")
            .arg(format!("cmd:{}", gen.display()))
            .args(extra)
            .output()
            .unwrap();
        assert_eq!(out.status.code(), Some(2), "{extra:?}");
    }
    assert!(!ws.join("calls.log").exists(), "usage errors ask no model");
}
