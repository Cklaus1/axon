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
    job_with(dir, "restricted")
}

fn job_with(dir: &Path, profile: &str) -> axon_os::JobManifest {
    std::fs::write(dir.join("prog.ax"), "fn main() -> i64 { 0 }\n").unwrap();
    std::fs::write(
        dir.join("job.axjob"),
        format!(
            "program = \"prog.ax\"\nintent = \"t\"\nseed = 1\nprofile = \"{profile}\"\n\
             [grant]\nmax_label = \"internal\"\n\
             [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n"
        ),
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

// ── C9 round 7, EQGATE3 (amendment 91): what the interpreter child is built with
//
// The child starts from an EMPTY environment and gets the seed, the PATH, the
// operator's AXON_* controls (never under a hermetic profile: a virtual clock
// instead), the job's directory as its working directory, /dev/null for stdin
// and pipes for its output. Each is a builder call that builds no `Err`; each
// was removable alone with every suite green. A stand-in interpreter records
// its own state; the OUTER run poisons its own environment first, so a child
// that inherits anything shows it.

/// What the stand-in recorded when run for `profile` under a hostile
/// environment: (cwd, fds, environment).
fn child_state(
    profile: &str,
) -> (
    String,
    Vec<String>,
    std::collections::BTreeMap<String, String>,
) {
    let d = tmp(&format!("state-{profile}"));
    let m = job_with(&d, profile);
    let rec = d.join("state.txt");
    let bin = script(
        &d,
        &format!(
            "FDS=$(for i in 0 1 2; do readlink \"/proc/$$/fd/$i\"; done)\n\
             {{ pwd; echo \"FDS $FDS\" | tr '\\n' ' '; echo; env; }} > '{}'",
            rec.display()
        ),
    );
    let rt = AxonCoreRuntime::with_bin_and_timeout(bin, Duration::from_secs(5));
    let _ = supervise(&m, &d.join("job.axjob"), &m.grant.clone(), "rid-state", &rt);
    let text = std::fs::read_to_string(&rec).expect("the interpreter stand-in ran");
    let mut lines = text.lines();
    let cwd = lines.next().unwrap().to_string();
    let fds = lines
        .next()
        .unwrap()
        .trim_start_matches("FDS ")
        .split_whitespace()
        .map(String::from)
        .collect();
    let vars = lines
        .filter_map(|l| l.split_once('='))
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect();
    let _ = std::fs::remove_dir_all(&d);
    (cwd, fds, vars)
}

const HOSTILE: [(&str, &str); 7] = [
    ("HOME", "/hostile-home"),
    ("EQ_SECRET", "leak"),
    ("AXON_AUDIT_LEDGER", "/hostile/ledger"),
    ("AXON_AI_MOCK", "1"),
    ("AXON_AI_REPLAY", "/hostile/replay"),
    ("AXON_PATH", "/hostile/modules"),
    ("AXON_MAX_DEPTH", "77"),
];

#[test]
fn the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory() {
    if std::env::var("AXON_EQ_OS_CHILD").is_err() {
        // Re-run this test in a child whose environment is hostile.
        let mut c = std::process::Command::new(std::env::current_exe().unwrap());
        c.args([
            "--exact",
            "the_interpreter_child_is_built_from_an_empty_environment_and_the_jobs_directory",
            "--test-threads=1",
        ])
        .env("AXON_EQ_OS_CHILD", "1");
        for (k, v) in HOSTILE {
            c.env(k, v);
        }
        let o = c.output().unwrap();
        assert!(
            o.status.success(),
            "the inner run failed:\n{}\n{}",
            String::from_utf8_lossy(&o.stdout),
            String::from_utf8_lossy(&o.stderr)
        );
        return;
    }
    // restricted: the operator's controls are forwarded, nothing else.
    let (cwd, fds, vars) = child_state("restricted");
    assert!(
        cwd.ends_with("-state-restricted"),
        "ATTACK: the interpreter child did not run in the job's directory: {cwd}"
    );
    assert_eq!(
        vars.get("AXON_SEED").map(String::as_str),
        Some("1"),
        "ATTACK: the interpreter child did not get the job's seed: {vars:?}"
    );
    assert!(
        vars.contains_key("PATH"),
        "ATTACK: no PATH reached the child: {vars:?}"
    );
    for (k, v) in [
        ("AXON_AUDIT_LEDGER", "/hostile/ledger"),
        ("AXON_AI_MOCK", "1"),
        ("AXON_AI_REPLAY", "/hostile/replay"),
        ("AXON_PATH", "/hostile/modules"),
        ("AXON_MAX_DEPTH", "77"),
    ] {
        assert_eq!(
            vars.get(k).map(String::as_str),
            Some(v),
            "ATTACK: the operator's {k} was not forwarded to a non-hermetic job: {vars:?}"
        );
    }
    for k in ["HOME", "EQ_SECRET"] {
        assert!(
            !vars.contains_key(k),
            "ATTACK: the interpreter child inherited {k} (the environment was not cleared): {vars:?}"
        );
    }
    assert_eq!(
        fds[0], "/dev/null",
        "ATTACK: the interpreter child's stdin is not /dev/null: {fds:?}"
    );
    assert!(
        fds[1].starts_with("pipe:") && fds[2].starts_with("pipe:") && fds[1] != fds[2],
        "ATTACK: the interpreter child's output is not captured through pipes: {fds:?}"
    );
    // hermetic: none of them, and the virtual clock instead.
    let (_, _, h) = child_state("hermetic");
    for k in [
        "AXON_AUDIT_LEDGER",
        "AXON_AI_MOCK",
        "AXON_AI_REPLAY",
        "AXON_PATH",
        "AXON_MAX_DEPTH",
    ] {
        assert!(
            !h.contains_key(k),
            "ATTACK: a hermetic job inherited the operator's {k}: {h:?}"
        );
    }
    assert_eq!(
        h.get("AXON_CLOCK").map(String::as_str),
        Some("0:1"),
        "ATTACK: a hermetic job runs without the virtual clock: {h:?}"
    );
}
