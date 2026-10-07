//! B259 — the legacy process adapter (`AxonCoreRuntime`) is truthful.
//!
//! Driven through the REAL `AxonCoreRuntime::run_sandboxed` and the real
//! `supervisor`, with the interpreter replaced by a small script (`AXON_BIN`
//! is just a path) that records what it was handed. That makes the staging
//! directory and the capture bound observable without depending on what the
//! interpreter prints.

mod common;
use std::path::{Path, PathBuf};
use std::time::Duration;

use axon_os::manifest::parse;
use axon_os::runtime::AxonCoreRuntime;
use axon_os::verdict::Verdict;
use axon_os::{supervise, supervise_requiring, IsolationRequirement, Runtime};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-os-legacy-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn script(dir: &Path, body: &str) -> PathBuf {
    let p = dir.join("fake-axon.sh");
    common::write_executable(&p, format!("#!/bin/sh\n{body}\n"), 0o755);
    p
}

fn job(dir: &Path) -> axon_os::JobManifest {
    std::fs::write(dir.join("prog.ax"), "fn main() -> i64 { 0 }\n").unwrap();
    std::fs::write(
        dir.join("job.axjob"),
        "program = \"prog.ax\"\nintent = \"t\"\nseed = 1\nprofile = \"restricted\"\n\
         [grant]\nmax_label = \"internal\"\n\
         [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n",
    )
    .unwrap();
    parse(
        &std::fs::read_to_string(dir.join("job.axjob")).unwrap(),
        dir,
    )
    .unwrap()
}

/// G13-r22-no-weak-fallback: a request REQUIRING a microVM is refused by the
/// process-scoped adapter with ZERO effects — no staging dir, no spawn.
#[test]
fn a_microvm_required_request_is_refused_by_the_process_adapter() {
    let d = tmp("microvm");
    let m = job(&d);
    let marker = d.join("SPAWNED");
    let stage_root = d.join("stage");
    std::fs::create_dir_all(&stage_root).unwrap();
    let bin = script(&d, &format!("touch '{}'", marker.display()));
    let rt = AxonCoreRuntime::with_bin_and_timeout(bin, Duration::from_secs(5))
        .with_staging_root(stage_root.clone());
    assert_eq!(rt.isolation().label(), "process_scoped");

    let rec = supervise_requiring(
        &m,
        &d.join("job.axjob"),
        &m.grant.clone(),
        "rid-microvm",
        IsolationRequirement::MicroVm,
        &rt,
    );
    match &rec.verdict {
        Verdict::Denied { axis, reason } => {
            assert_eq!(axis, "isolation");
            assert!(reason.contains("process_scoped"), "{reason}");
        }
        other => panic!("must be refused on the isolation axis, got {other:?}"),
    }
    assert!(!marker.exists(), "refusal must spawn nothing");
    assert_eq!(
        std::fs::read_dir(&stage_root).unwrap().count(),
        0,
        "refusal must stage nothing"
    );

    // The contrast: the SAME runtime with no requirement does run it.
    let _ = supervise(&m, &d.join("job.axjob"), &m.grant.clone(), "rid-any", &rt);
    assert!(marker.exists(), "premise: the runtime can spawn");
    let _ = std::fs::remove_dir_all(&d);
}

/// The wrapper is staged in a fresh 0700 directory (not a guessable name in
/// shared /tmp) and that directory is gone after the run.
#[test]
fn the_wrapper_is_staged_privately_and_removed() {
    let d = tmp("staging");
    let m = job(&d);
    let log = d.join("seen.log");
    let stage_root = d.join("stage");
    std::fs::create_dir_all(&stage_root).unwrap();
    // $1 = "run", $2 = wrapper path.
    let bin = script(
        &d,
        &format!(
            "echo \"$2\" > '{log}'; stat -c %a \"$(dirname \"$2\")\" >> '{log}'; \
             stat -c %a \"$2\" >> '{log}'",
            log = log.display()
        ),
    );
    let rt = AxonCoreRuntime::with_bin_and_timeout(bin, Duration::from_secs(5))
        .with_staging_root(stage_root.clone());
    let _ = supervise(&m, &d.join("job.axjob"), &m.grant.clone(), "rid", &rt);

    let seen = std::fs::read_to_string(&log).expect("the interpreter stand-in ran");
    let mut lines = seen.lines();
    let wrapper = PathBuf::from(lines.next().unwrap());
    assert_eq!(lines.next(), Some("700"), "staging dir mode");
    assert_eq!(lines.next(), Some("600"), "wrapper file mode");
    assert!(wrapper.starts_with(&stage_root), "{}", wrapper.display());
    let dir = wrapper.parent().unwrap();
    assert!(
        dir.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("axon-os-run-"),
        "{}",
        dir.display()
    );
    assert!(!wrapper.exists() && !dir.exists(), "removed after the run");
    // And the old shared-/tmp name is not used.
    assert!(!std::env::temp_dir()
        .join(format!("axon-os-wrap-{}-prog.ax", std::process::id()))
        .exists());
    let _ = std::fs::remove_dir_all(&d);
}

/// Captured output is bounded, and bounding it does not lose the completion
/// marker (the tail is kept), so a chatty job is still sealed as completed.
#[test]
fn captured_output_is_bounded_and_the_verdict_survives_it() {
    let d = tmp("bounded");
    let m = job(&d);
    // Flood stdout AND stderr, then print the wrapper's completion marker
    // (read out of the wrapper, as the real wrapper would print it).
    let bin = script(
        &d,
        "head -c 4194304 /dev/zero | tr '\\0' y >&2\n\
         head -c 4194304 /dev/zero | tr '\\0' x\necho\n\
         grep -o '__axon_os_done:[0-9a-f]*' \"$2\"\nexit 0",
    );
    let rt = AxonCoreRuntime::with_bin_and_timeout(bin, Duration::from_secs(30))
        .with_max_capture(8 * 1024);
    let rec = supervise(&m, &d.join("job.axjob"), &m.grant.clone(), "rid", &rt);
    assert_eq!(
        rec.verdict,
        Verdict::Completed { value: 0 },
        "the marker is in the retained tail"
    );
    let _ = std::fs::remove_dir_all(&d);
}
