//! ceilings (`tasks/ceilings-spec.md`): the operator's bounds on a run.
//!
//! Each test runs the `axon` binary this test run built, with every ambient
//! bound variable cleared first, so the environment of the caller never
//! decides what is bounded.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const BOUND_VARS: [&str; 8] = [
    "AXON_ALLOWED_EFFECTS",
    "AXON_ALLOWED_FS_READ",
    "AXON_ALLOWED_FS_WRITE",
    "AXON_ALLOWED_NET",
    "AXON_BUDGET_TOKENS",
    "AXON_AI_MOCK",
    "AXON_ENGINE",
    "AXON_MAX_DEPTH",
];

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon_ceiling_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// `axon <args..> <dir>/p.ax` with every bound variable cleared, then `vars`
/// set; the provenance cache is kept inside `dir`.
fn run_with(dir: &Path, args: &[&str], src: &str, vars: &[(&str, &str)]) -> Output {
    let prog = dir.join("p.ax");
    std::fs::write(&prog, src).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon"));
    cmd.args(args)
        .arg(&prog)
        .current_dir(dir)
        .env("XDG_CACHE_HOME", dir.join("cache"))
        .env_remove("AXON_CEILING")
        .env_remove("AXON_MAX_STEPS")
        .env_remove("AXON_WALL_MS");
    for v in BOUND_VARS {
        cmd.env_remove(v);
    }
    for (k, v) in vars {
        cmd.env(k, v);
    }
    cmd.output().unwrap()
}

fn run(dir: &Path, src: &str, vars: &[(&str, &str)]) -> Output {
    run_with(dir, &["run"], src, vars)
}

fn text(o: &Output) -> String {
    String::from_utf8_lossy(&o.stdout).to_string() + &String::from_utf8_lossy(&o.stderr)
}

// ── CE-7: `goal_*` dispatch demands only `IO` ───────────────────────────────

const PURE_METRIC: &str = r#"
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let d = x - 7
    d * d
}
"#;

const AI_METRIC: &str = r#"
@[adaptive(metric: cost, target: 0.0)]
fn measured(x: i64) -> i64 {
    let r = ai_complete("score this")
    let d = x - 7
    d * d
}
"#;

const GOAL_MAIN: &str = r#"
fn main() {
    let best = goal_run("measured", 0.0, 4)
    println("best=" + to_str_f64(best))
}
"#;

/// g02: the program makes an `IO`-only sandbox and calls `goal_run` in it.
const GOAL_IN_SANDBOX: &str = r#"
fn job(x: i64) -> i64 {
    let best = goal_run("measured", 0.0, 4)
    println("best=" + to_str_f64(best))
    0
}

fn main() {
    let p = principal_root("p", true, false, false, 100)
    let sb = sandbox_create(p, "IO")
    let r = sandbox_run(sb, "job", 0)
    println("r=" + to_str(r))
}
"#;

/// RED before CE-7: `goal_run` was refused (exit 8) for demanding `AI`,
/// although its metric `(x-7)^2` never calls a model.
#[test]
fn goal_run_pure_metric_runs_without_ai_grant() {
    let d = tmp("ce7_pure_ambient");
    let src = format!("{PURE_METRIC}{GOAL_MAIN}");
    let o = run(&d, &src, &[("AXON_ALLOWED_EFFECTS", "IO")]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "goal_run with a pure metric must run under AXON_ALLOWED_EFFECTS=IO: {}",
        text(&o)
    );
    assert!(text(&o).contains("best="), "{}", text(&o));
}

/// RED before CE-7: the same refusal inside a sandbox the program made itself.
#[test]
fn goal_run_pure_metric_runs_in_program_sandbox() {
    let d = tmp("ce7_pure_sandbox");
    let src = format!("{PURE_METRIC}{GOAL_IN_SANDBOX}");
    let o = run(&d, &src, &[("AXON_AI_MOCK", "1")]);
    assert_eq!(
        o.status.code(),
        Some(0),
        "goal_run with a pure metric must run in an IO sandbox: {}",
        text(&o)
    );
    assert!(text(&o).contains("r=0"), "{}", text(&o));
}

/// Guard: a metric that DOES call a model is still refused without `AI`; the
/// refusal is at the metric's `ai_complete`.
#[test]
fn goal_run_ai_metric_still_refused_without_ai_grant() {
    let d = tmp("ce7_ai_ambient");
    let src = format!("{AI_METRIC}{GOAL_MAIN}");
    let o = run(
        &d,
        &src,
        &[("AXON_ALLOWED_EFFECTS", "IO"), ("AXON_AI_MOCK", "1")],
    );
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: a goal_run metric reached a model without an AI grant: {t}"
    );
    assert!(
        t.contains("builtin `ai_complete` requires effect `AI`"),
        "ATTACK: a goal_run metric reached a model without an AI grant: {t}"
    );
}

/// Guard: the same inside a program-made `IO` sandbox.
#[test]
fn goal_run_ai_metric_refused_in_program_sandbox() {
    let d = tmp("ce7_ai_sandbox");
    let src = format!("{AI_METRIC}{GOAL_IN_SANDBOX}");
    let o = run(&d, &src, &[("AXON_AI_MOCK", "1")]);
    let t = text(&o);
    assert_eq!(
        o.status.code(),
        Some(8),
        "ATTACK: a sandboxed goal_run metric reached a model: {t}"
    );
    assert!(
        t.contains("builtin `ai_complete` requires effect `AI`"),
        "ATTACK: a sandboxed goal_run metric reached a model: {t}"
    );
}
