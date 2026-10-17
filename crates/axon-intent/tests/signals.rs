//! KG5: SIGTERM to `axon-intent` reaches the synthesizer's job, and
//! `axon-intent` still dies by SIGTERM.
//!
//! The synthesizer runs `$AXON_BIN intent compile …` as a process-group job
//! (KG1). Without forwarding, a termination signal to `axon-intent` killed only
//! `axon-intent`; the synthesizer and everything it started ran on. The script
//! below stands in for `axon`: it records its pid and a background `sleep`'s,
//! then waits. Liveness is `/proc/<pid>/cmdline` (a zombie or a reused pid reads
//! as gone); no `kill` binary (`process_control_guard.rs`).

use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

struct Proc {
    pid: i32,
    cmdline: Vec<u8>,
}

impl Proc {
    fn cmdline_of(pid: i32) -> Option<Vec<u8>> {
        std::fs::read(format!("/proc/{pid}/cmdline"))
            .ok()
            .filter(|c| !c.is_empty())
    }

    /// Wait (at most 30 s) until `pid_file` names a live process running
    /// `argv` (`None`: whatever it runs when first seen).
    fn wait_alive(pid_file: &Path, argv: Option<&[&str]>) -> Proc {
        let want: Option<Vec<u8>> =
            argv.map(|a| a.iter().flat_map(|s| s.bytes().chain([0])).collect());
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            let pid = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok());
            if let Some((pid, c)) = pid.and_then(|p| Proc::cmdline_of(p).map(|c| (p, c))) {
                if want.as_ref().is_none_or(|w| *w == c) {
                    return Proc { pid, cmdline: c };
                }
            }
            assert!(
                Instant::now() < deadline,
                "{} never named a live process",
                pid_file.display()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn alive(&self) -> bool {
        Proc::cmdline_of(self.pid).is_some_and(|c| c == self.cmdline)
    }

    fn gone_within(&self, d: Duration) -> bool {
        let deadline = Instant::now() + d;
        while self.alive() {
            if Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        true
    }
}

impl Drop for Proc {
    fn drop(&mut self) {
        if self.alive() {
            // SAFETY: plain syscall; the pid was just checked to be ours.
            unsafe { libc::kill(self.pid, libc::SIGKILL) };
        }
    }
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

/// SIGTERM, not SIGINT: a background job of a non-interactive shell (gate.sh's
/// `par`) starts with SIGINT ignored, which would make a SIGINT row vacuous.
/// `axon-intent` runs in its OWN process group, so a misdirected `killpg(0)`
/// could reach only it.
#[test]
fn kg5_sigterm_to_axon_intent_kills_the_synthesizers_job_and_it_dies_by_it() {
    let dir = std::env::temp_dir().join(format!("axon_intent_kg5_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let fake_axon = dir.join("fake-axon.sh");
    std::fs::write(
        &fake_axon,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nsleep 4722 </dev/null >/dev/null 2>&1 &\n\
             echo $! > '{}'\nwait\n",
            dir.join("synth.pid").display(),
            dir.join("gc.pid").display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &fake_axon,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    )
    .unwrap();

    let mut intent = Command::new(env!("CARGO_BIN_EXE_axon-intent"))
        .current_dir(workspace_root())
        .args(["compile", "examples/intents/summarize.intent.md", "--out"])
        .arg(dir.join("out"))
        .env_remove("AXON_AI_MOCK")
        .env("AXON_BIN", &fake_axon)
        .env("AXON_INTENT_TIMEOUT_MS", "600000")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn()
        .unwrap();
    let synth = Proc::wait_alive(&dir.join("synth.pid"), None);
    let grandchild = Proc::wait_alive(&dir.join("gc.pid"), Some(&["sleep", "4722"]));

    // SAFETY: plain syscall to a child this test spawned and has not reaped.
    assert_eq!(unsafe { libc::kill(intent.id() as i32, libc::SIGTERM) }, 0);
    let deadline = Instant::now() + Duration::from_secs(20);
    let status = loop {
        if let Some(st) = intent.try_wait().unwrap() {
            break st;
        }
        if Instant::now() >= deadline {
            let _ = intent.kill();
            let _ = intent.wait();
            panic!("axon-intent did not exit within 20 s of SIGTERM");
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(
        status.signal(),
        Some(libc::SIGTERM),
        "axon-intent must still die by the SIGTERM it was sent ({status:?})"
    );
    for (who, p) in [
        ("the synthesizer", &synth),
        ("the synthesizer's grandchild", &grandchild),
    ] {
        assert!(
            p.gone_within(Duration::from_secs(5)),
            "ATTACK: {who} (pid {}) outlived SIGTERM to axon-intent",
            p.pid
        );
    }
    let _ = std::fs::remove_dir_all(&dir);
}
