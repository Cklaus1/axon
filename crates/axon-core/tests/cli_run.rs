//! End-to-end CLI tests: run the `axon` binary on real examples and assert
//! actual stdout / exit codes (the unit tests cover parse/exit; these cover the
//! whole pipeline through the interpreter, including printed output).
//!
//! Uses `CARGO_BIN_EXE_axon` (set by cargo for the codegen-free `axon` binary
//! built under `--no-default-features`).

use std::process::Command;

fn axon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon"))
}

fn ex(rel: &str) -> String {
    format!("{}/../../examples/{}", env!("CARGO_MANIFEST_DIR"), rel)
}

#[test]
fn run_hello_prints_greeting() {
    let out = axon().args(["run", &ex("hello.ax")]).output().unwrap();
    assert!(out.status.success(), "hello.ax exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Hello, Axon!"), "stdout was: {stdout:?}");
}

#[test]
fn run_comprehensive_computes_sum_to_100() {
    let out = axon().args(["run", &ex("comprehensive.ax")]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("5050"), "expected sum_to(100)=5050 in: {stdout:?}");
}

#[test]
fn run_error_handling_propagates_results() {
    let out = axon().args(["run", &ex("error_handling.ax")]).output().unwrap();
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    // sqrt_of_string("144") => Ok(12); division by zero is reported.
    assert!(stdout.contains("Ok(12)"), "stdout: {stdout:?}");
    assert!(stdout.contains("division by zero"), "stdout: {stdout:?}");
}

#[test]
fn goal_optimize_deploys() {
    let out = axon().args(["goal", &ex("goals/optimize-goal.md")]).output().unwrap();
    assert!(out.status.success(), "optimize-goal exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("best score: 100"), "stdout: {stdout:?}");
    assert!(stdout.contains("deploy gate: passed"), "stdout: {stdout:?}");
}

#[test]
fn run_typechecks_before_interpreting() {
    // An undefined-name program must be rejected (exit 2) by check-before-run,
    // not surface as a runtime panic.
    let bad = std::env::temp_dir().join("axon_cli_run_typecheck.ax");
    std::fs::write(&bad, "fn main() -> i64 { undefined_helper(1) }\n").unwrap();
    let out = axon().args(["run", bad.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&bad);
    assert_eq!(out.status.code(), Some(2), "expected type-error exit 2");
}

#[test]
fn run_exits_with_main_return_value() {
    let f = std::env::temp_dir().join("axon_cli_run_exitcode.ax");
    std::fs::write(&f, "fn main() -> i64 { 7 }\n").unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(7), "main's i64 return should be the exit code");
}

/// Parse the integer from a "best score: N (target …)" line.
fn best_score(stdout: &str) -> i64 {
    let key = "best score: ";
    let i = stdout.find(key).unwrap_or_else(|| panic!("no 'best score:' in: {stdout:?}")) + key.len();
    let rest = &stdout[i..];
    let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
    rest[..end].parse().unwrap_or_else(|_| panic!("unparseable score in: {stdout:?}"))
}

#[test]
fn cross_run_improves_with_continuation() {
    // learn-goal's per-run budget can't reach the optimum from 0; with
    // AXON_GOAL_CONTINUE the second run resumes from the first run's best input
    // (via the persisted provenance log) and scores strictly higher.
    let cache = std::env::temp_dir().join(format!("axon_xrun_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let goal = ex("goals/learn-goal.md");

    let r1 = axon()
        .args(["goal", &goal])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();
    let s1 = best_score(&String::from_utf8_lossy(&r1.stdout));

    let r2 = axon()
        .args(["goal", &goal])
        .env("XDG_CACHE_HOME", &cache)
        .env("AXON_GOAL_CONTINUE", "1")
        .output()
        .unwrap();
    let s2 = best_score(&String::from_utf8_lossy(&r2.stdout));

    let _ = std::fs::remove_dir_all(&cache);
    assert!(s2 > s1, "continuation should improve the best score: run1={s1}, run2={s2}");
}

#[test]
fn goal_iterate_converges() {
    // `--iterate N` runs the goal with cross-run continuation (after run 1) and
    // stops early when the best score plateaus. learn-goal's per-run budget can't
    // reach the optimum alone, so the score climbs run-over-run; given a generous
    // cap (12) it must converge to the target (200) and stop short of the cap.
    let cache = std::env::temp_dir().join(format!("axon_iter_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let out = axon()
        .args(["goal", "--iterate", "12", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();
    let _ = std::fs::remove_dir_all(&cache);

    // Each run prints its "best score: N" to stdout (iterate headers go to stderr).
    let stdout = String::from_utf8_lossy(&out.stdout);
    let key = "best score: ";
    let scores: Vec<i64> = stdout
        .match_indices(key)
        .map(|(i, _)| {
            let rest = &stdout[i + key.len()..];
            let end = rest.find(|c: char| !c.is_ascii_digit() && c != '-').unwrap_or(rest.len());
            rest[..end].parse().unwrap()
        })
        .collect();
    assert!(scores.len() >= 5, "should take several runs to converge: {scores:?}");
    assert!(scores.len() < 12, "should stop early on convergence, not run the full cap: {scores:?}");
    assert!(scores.windows(2).all(|w| w[1] >= w[0]), "best score is non-decreasing: {scores:?}");
    assert_eq!(*scores.last().unwrap(), 200, "should converge to the optimum: {scores:?}");

    // It should also report the solution (best score + the input that achieved it).
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("# best: score 200 at input 12"), "should report the solution: {stderr:?}");
}

#[test]
fn trace_summarizes_provenance() {
    // Produce a provenance log by iterating a goal, then `axon trace` should
    // summarize the search: the function, its eval count, and that the best
    // score was found at the optimum input (12).
    let cache = std::env::temp_dir().join(format!("axon_trace_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let _ = axon()
        .args(["goal", "--iterate", "6", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();

    let out = axon().args(["trace"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(out.status.success(), "trace exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("try_variant"), "stdout: {stdout:?}");
    assert!(stdout.contains("at input 12"), "best score is at the optimum input: {stdout:?}");
    assert!(stdout.contains("improving"), "trajectory should be improving: {stdout:?}");
}

#[test]
fn trace_json_is_machine_readable() {
    let cache = std::env::temp_dir().join(format!("axon_tracej_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&cache);
    let _ = axon()
        .args(["goal", "--iterate", "6", &ex("goals/learn-goal.md")])
        .env("XDG_CACHE_HOME", &cache)
        .env_remove("AXON_GOAL_CONTINUE")
        .output()
        .unwrap();

    let out = axon().args(["trace", "--json"]).env("XDG_CACHE_HOME", &cache).output().unwrap();
    let _ = std::fs::remove_dir_all(&cache);
    assert!(out.status.success());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.trim_start().starts_with('['), "should be a JSON array: {stdout:?}");
    assert!(stdout.contains("\"fn\":\"try_variant\""), "stdout: {stdout:?}");
    assert!(stdout.contains("\"best_input\":12"), "stdout: {stdout:?}");
    assert!(stdout.contains("\"trend\":\"improving\""), "stdout: {stdout:?}");
}

#[test]
fn trace_missing_log_exits_nonzero() {
    let out = axon()
        .args(["trace"])
        .env("XDG_CACHE_HOME", "/nonexistent-axon-cache-xyz")
        .env_remove("HOME")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1), "missing log should exit 1");
}

#[test]
fn supervised_agent_halts_on_unsafe_actions() {
    // Capability under control: the agent banks value from safe actions, then a
    // two-strike kill-switch latches on unsafe proposals and the final safe
    // action is refused.
    let out = axon().args(["run", &ex("asi/supervised_agent.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("HALTED: banked 50 value safely"), "stdout: {stdout:?}");
    assert!(stdout.contains("SKIP   finish-job"), "latched halt must refuse the safe action: {stdout:?}");
}

#[test]
fn deliberative_agent_picks_best_permitted() {
    // Constrained optimization: the agent takes the best action it is *permitted*
    // to take, declining a higher-value unsafe option and an over-budget one.
    let out = axon().args(["run", &ex("asi/deliberative_agent.ax")]).output().unwrap();
    assert!(out.status.success(), "exited {:?}", out.status.code());
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("take 'deep-analysis'"), "should pick the best permitted: {stdout:?}");
}

#[test]
fn len_works_on_arrays() {
    // Regression: `len` was typed str-only, so `len(my_array)` failed the type
    // checker even though the interpreter supports it. It now accepts slices, so
    // the idiomatic `for i in 0..len(xs)` index loop type-checks and runs.
    let f = std::env::temp_dir().join(format!("axon_len_arr_{}.ax", std::process::id()));
    std::fs::write(
        &f,
        "fn main() -> i64 {\n  let xs = [10, 20, 30]\n  let total = 0\n  \
         for i in 0..len(xs) { total = total + xs[i] }\n  total\n}\n",
    )
    .unwrap();
    let out = axon().args(["run", f.to_str().unwrap()]).output().unwrap();
    let _ = std::fs::remove_file(&f);
    assert_eq!(out.status.code(), Some(60), "len-driven array loop should sum to 60");
}

#[test]
fn run_trait_methods_dispatch() {
    // trait + impl methods + value.method() dispatch (the interpreter picks the
    // impl from the receiver's runtime type).
    let out = axon().args(["run", &ex("traits_methods.ax")]).output().unwrap();
    assert_eq!(out.status.code(), Some(49), "sq.area()+r.area() should be 49");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("Square area = 25"), "stdout: {stdout:?}");
    assert!(stdout.contains("Rect area = 24"), "stdout: {stdout:?}");
}
