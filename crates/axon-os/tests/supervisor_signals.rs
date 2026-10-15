//! G2d — a termination signal to `axon-os run` reaches the job.
//!
//! The job runs in its own process group (so one `killpg` reaches all of it),
//! which also keeps the terminal's Ctrl-C and a `kill` of the supervisor from
//! reaching it. Before G2d, SIGTERM to `axon-os run` killed the supervisor
//! alone: the job ran on, re-parented to init, with nothing left to kill it at
//! its timeout or on its latch, no record, and its staging directory left
//! behind.
//!
//! Driven through the real binary, with the interpreter replaced by a script
//! (`AXON_BIN` is just a path) that starts a grandchild and then waits.

mod common;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-os-signals-{}-{tag}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A job whose interpreter stand-in starts the grandchild `sleep <gc>` and then
/// becomes `sleep <leader>`, writing both pids down.
fn job(d: &Path, leader: &str, gc: &str) -> PathBuf {
    std::fs::write(d.join("prog.ax"), "fn main() -> i64 { 0 }\n").unwrap();
    std::fs::write(
        d.join("job.axjob"),
        "program = \"prog.ax\"\nintent = \"t\"\nseed = 1\nprofile = \"restricted\"\n\
         [grant]\nmax_label = \"internal\"\n\
         [grant.budget]\ncalls = 1\ntokens = 1\ncost_micro = 0\n",
    )
    .unwrap();
    common::write_executable(
        &d.join("fake-axon.sh"),
        format!(
            "#!/bin/sh\nsleep {gc} </dev/null >/dev/null 2>&1 &\necho $! > '{}'\n\
             echo $$ > '{}'\nexec sleep {leader}\n",
            d.join("grandchild.pid").display(),
            d.join("leader.pid").display()
        ),
        0o755,
    );
    d.join("job.axjob")
}

/// The job's leader and grandchild, once both are running. Dropping them
/// kills whichever is left, so a failing test leaks neither.
fn running_job(d: &Path, leader: &str, gc: &str) -> (common::Tracked, common::Tracked) {
    (
        common::Tracked::wait_alive(&d.join("leader.pid"), &["sleep", leader]),
        common::Tracked::wait_alive(&d.join("grandchild.pid"), &["sleep", gc]),
    )
}

/// `axon-os run` on `job`, in its OWN process group, so a misdirected
/// `killpg(0)` in its handler reaches only it (and a bystander a test puts
/// there), never the test runner.
fn axon_os_run(
    d: &Path,
    job: &Path,
    setup: impl Fn() -> std::io::Result<()> + Send + Sync + 'static,
) -> Child {
    let stage = d.join("stage");
    std::fs::create_dir_all(&stage).unwrap();
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_axon-os"));
    cmd.arg("run")
        .arg(job)
        .args(["--run-id", "sig", "--out"])
        .arg(d.join("out"))
        .env("AXON_BIN", d.join("fake-axon.sh"))
        .env("TMPDIR", &stage)
        .env("AXON_OS_TIMEOUT_MS", "120000")
        .env_remove("AXON_KILL_FILE")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    // SAFETY: `setup` is async-signal-safe (a `signal` call at most).
    unsafe { cmd.pre_exec(setup) };
    cmd.spawn().expect("spawn axon-os")
}

/// Wait at most `d` for `child`; on expiry SIGKILL it and fail.
fn exit_within(child: &mut Child, d: Duration) -> ExitStatus {
    let deadline = Instant::now() + d;
    loop {
        if let Some(st) = child.try_wait().unwrap() {
            return st;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("axon-os did not exit within {d:?} of the signal");
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn signal(pid: u32, sig: libc::c_int) {
    // SAFETY: plain syscall to a child this test spawned and has not reaped.
    assert_eq!(unsafe { libc::kill(pid as i32, sig) }, 0);
}

#[test]
fn sigterm_to_axon_os_run_kills_the_job_and_still_seals_its_record() {
    let d = tmp("sigterm");
    let job = job(&d, "3640", "3641");
    let mut os = axon_os_run(&d, &job, || Ok(()));
    // A bystander in axon-os's OWN group: only the job's group may be killed.
    let mut bystander = Command::new("sleep")
        .arg("3642")
        .process_group(os.id() as i32)
        .spawn()
        .expect("spawn bystander");
    let (leader, grandchild) = running_job(&d, "3640", "3641");

    signal(os.id(), libc::SIGTERM);
    let status = exit_within(&mut os, Duration::from_secs(30));
    let bystander_state = bystander.try_wait().expect("poll bystander");
    let _ = bystander.kill();
    let _ = bystander.wait();
    let mut stdout = String::new();
    std::io::Read::read_to_string(&mut os.stdout.take().unwrap(), &mut stdout).unwrap();

    assert_eq!(
        status.code(),
        Some(128 + libc::SIGTERM),
        "ATTACK: SIGTERM did not end `axon-os run` through its own exit path \
         ({status:?}; stdout: {stdout})"
    );
    for (who, p) in [("leader", &leader), ("grandchild", &grandchild)] {
        assert!(
            p.gone_within(Duration::from_secs(5)),
            "ATTACK: the job's {who} (pid {}) outlived the supervisor's SIGTERM",
            p.pid
        );
    }
    let record = std::fs::read_to_string(d.join("out").join("sig.json"))
        .expect("ATTACK: no run record after SIGTERM");
    assert!(
        record.contains("Halted") && record.contains("SIGTERM"),
        "the record must say the run was halted by SIGTERM: {record}"
    );
    assert_eq!(
        std::fs::read_dir(d.join("stage")).unwrap().count(),
        0,
        "ATTACK: the run's staging directory outlived the SIGTERM"
    );
    assert!(
        bystander_state.is_none(),
        "ATTACK: a process in the supervisor's own group was killed ({bystander_state:?})"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `nohup axon-os run …`: a SIGHUP that was ignored when the supervisor
/// started stays ignored, and the job runs on.
#[test]
fn an_ignored_sighup_stays_ignored() {
    let d = tmp("nohup");
    let job = job(&d, "3643", "3644");
    let mut os = axon_os_run(&d, &job, || {
        // SAFETY: `signal` is async-signal-safe.
        unsafe { libc::signal(libc::SIGHUP, libc::SIG_IGN) };
        Ok(())
    });
    let (leader, grandchild) = running_job(&d, "3643", "3644");

    signal(os.id(), libc::SIGHUP);
    std::thread::sleep(Duration::from_millis(500));
    let after_hup = os.try_wait().unwrap();
    let job_alive = leader.alive() && grandchild.alive();

    signal(os.id(), libc::SIGTERM);
    let status = exit_within(&mut os, Duration::from_secs(30));
    assert!(
        after_hup.is_none() && job_alive,
        "ATTACK: an ignored SIGHUP stopped the run (supervisor: {after_hup:?}, \
         job alive: {job_alive})"
    );
    assert_eq!(status.code(), Some(128 + libc::SIGTERM));
    assert!(grandchild.gone_within(Duration::from_secs(5)));
    let _ = std::fs::remove_dir_all(&d);
}
