//! `axon check --time-passes` / `axon build --time-passes` (AX-36).
//!
//! The report is a contract for tools: one `time: <phase> <ms>` line per phase
//! on stderr, in the order the phases ran, `total` last, and nothing at all
//! without the flag. The build tests return early when the binary was built
//! without the `codegen` feature, like the native tests in `cli_run.rs`.

use std::path::PathBuf;
use std::process::{Command, Output};

fn axon() -> Command {
    Command::new(env!("CARGO_BIN_EXE_axon"))
}

fn tmp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!("axon_timepasses_{}_{name}", std::process::id()))
}

fn write_src(name: &str, src: &str) -> PathBuf {
    let f = tmp(&format!("{name}.ax"));
    std::fs::write(&f, src).expect("write temp source");
    f
}

const SRC: &str = "fn helper(x: i64) -> i64 { x * 2 + 1 }\n\
                   fn main() -> i64 {\n    println(to_str(helper(20)))\n    0\n}\n";

/// The `(phase, ms)` pairs of a text report, asserting every `time:` line is
/// well formed: a whitespace-free name and milliseconds with three decimals.
fn parse_report(stderr: &str) -> Vec<(String, f64)> {
    stderr
        .lines()
        .filter(|l| l.starts_with("time: "))
        .map(|l| {
            let fields: Vec<&str> = l.split(' ').collect();
            assert_eq!(fields.len(), 3, "`time: <phase> <ms>` expected: {l:?}");
            let ms = fields[2];
            assert!(
                ms.split_once('.').is_some_and(|(_, frac)| frac.len() == 3),
                "milliseconds carry three decimals: {l:?}"
            );
            (fields[1].to_string(), ms.parse().expect("numeric ms"))
        })
        .collect()
}

/// `names` appear in the report in this relative order, each exactly once.
fn assert_phases_in_order(report: &[(String, f64)], names: &[&str]) {
    let mut last = None;
    for name in names {
        let at: Vec<usize> = report
            .iter()
            .enumerate()
            .filter(|(_, (n, _))| n == name)
            .map(|(i, _)| i)
            .collect();
        assert_eq!(at.len(), 1, "phase `{name}` once, report: {report:?}");
        assert!(
            last.is_none_or(|l| l < at[0]),
            "phase `{name}` out of order, report: {report:?}"
        );
        last = Some(at[0]);
    }
}

/// `total` is the last line and no smaller than the phases it contains.
fn assert_total_bounds_phases(report: &[(String, f64)]) {
    let (last, total) = report.last().expect("non-empty report");
    assert_eq!(last, "total", "total is last: {report:?}");
    let sum: f64 = report[..report.len() - 1].iter().map(|(_, ms)| ms).sum();
    // Each value is rounded to 0.001 ms.
    let slack = 0.001 * report.len() as f64;
    assert!(
        sum <= total + slack,
        "phases are leaves, so they cannot sum past the total: {sum} > {total}"
    );
}

const CHECK_PHASES: &[&str] = &[
    "read",
    "parse",
    "imports",
    "resolve",
    "fill_captures",
    "infer",
    "checker",
    "borrow",
    "capabilities",
    "effects",
    "lint",
    "total",
];

#[test]
fn check_time_passes_reports_every_front_end_phase() {
    let f = write_src("check", SRC);
    let out = axon()
        .args(["check", "--time-passes"])
        .arg(&f)
        .output()
        .expect("spawn axon check");
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "check passes: {stderr}");
    let report = parse_report(&stderr);
    assert_phases_in_order(&report, CHECK_PHASES);
    assert_total_bounds_phases(&report);
    // Timing goes to stderr only; stdout stays what it was.
    assert!(out.stdout.is_empty(), "stdout untouched: {:?}", out.stdout);
}

#[test]
fn check_without_the_flag_prints_no_timing() {
    let f = write_src("check_off", SRC);
    let out = axon()
        .arg("check")
        .arg(&f)
        .output()
        .expect("spawn axon check");
    let _ = std::fs::remove_file(&f);
    assert!(out.status.success());
    // Success is silent (Unix convention); timing must not change that.
    assert!(
        out.stderr.is_empty(),
        "no output without --time-passes: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}

#[test]
fn check_time_passes_still_reports_when_the_program_has_errors() {
    let f = write_src("check_err", "fn main() -> i64 {\n    undefined_name\n}\n");
    let out = axon()
        .args(["check", "--time-passes"])
        .arg(&f)
        .output()
        .expect("spawn axon check");
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2), "type error exits 2: {stderr}");
    let report = parse_report(&stderr);
    assert_phases_in_order(&report, &["parse", "resolve", "infer", "total"]);
}

#[test]
fn check_json_time_passes_is_one_schema_object() {
    let f = write_src("check_json", SRC);
    let out = axon()
        .args(["check", "--json", "--time-passes"])
        .arg(&f)
        .output()
        .expect("spawn axon check");
    let _ = std::fs::remove_file(&f);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 1, "one JSON line: {stderr}");
    let line = lines[0];
    assert!(
        line.starts_with("{\"schema\":\"axon-time-passes/1\",\"command\":\"check\",\"phases\":["),
        "{line}"
    );
    for phase in CHECK_PHASES.iter().filter(|p| **p != "total") {
        assert!(
            line.contains(&format!("{{\"name\":\"{phase}\",\"ms\":")),
            "missing `{phase}`: {line}"
        );
    }
    assert!(
        line.contains("\"total_ms\":") && line.ends_with('}'),
        "{line}"
    );
}

fn codegen_absent(out: &Output) -> bool {
    String::from_utf8_lossy(&out.stderr)
        .contains("requires building axon with the `codegen` feature")
}

#[test]
fn build_time_passes_reports_front_end_and_codegen_stages() {
    let f = write_src("build", SRC);
    let bin = tmp("build_bin");
    let out = axon()
        .args(["build", "--no-cache", "--time-passes"])
        .arg(&f)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn axon build");
    let _ = std::fs::remove_file(&f);
    if codegen_absent(&out) {
        return;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build succeeds: {stderr}");
    let ran = Command::new(&bin).output().expect("run built binary");
    let _ = std::fs::remove_file(&bin);
    assert_eq!(String::from_utf8_lossy(&ran.stdout), "41\n");
    let report = parse_report(&stderr);
    assert_phases_in_order(
        &report,
        &[
            "parse",
            "resolve",
            "infer",
            "checker",
            "effects",
            "mono",
            "ir_gen",
            "ir_verify",
            "target_init",
            "backend",
            "link",
            "total",
        ],
    );
    assert_total_bounds_phases(&report);
}

#[test]
fn build_time_passes_times_the_ir_pipeline_when_optimising() {
    let f = write_src("build_opt", SRC);
    let bin = tmp("build_opt_bin");
    let out = axon()
        .args(["build", "--no-cache", "--release", "--time-passes"])
        .arg(&f)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn axon build");
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&bin);
    if codegen_absent(&out) {
        return;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "build succeeds: {stderr}");
    let report = parse_report(&stderr);
    assert_phases_in_order(&report, &["ir_gen", "ir_opt", "backend", "link", "total"]);
}

#[test]
fn build_without_the_flag_prints_no_timing() {
    let f = write_src("build_off", SRC);
    let bin = tmp("build_off_bin");
    let out = axon()
        .args(["build", "--no-cache"])
        .arg(&f)
        .arg("-o")
        .arg(&bin)
        .output()
        .expect("spawn axon build");
    let _ = std::fs::remove_file(&f);
    let _ = std::fs::remove_file(&bin);
    if codegen_absent(&out) {
        return;
    }
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    assert!(
        !stderr.lines().any(|l| l.starts_with("time: ")),
        "no timing without --time-passes: {stderr}"
    );
}
