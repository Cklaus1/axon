#![allow(dead_code)]
//! Test executables whose bytes are written by a SEPARATE process.
//!
//! A test that writes (or `std::fs::copy`s) an executable in-process holds a
//! write fd on it for a moment. Under the parallel test runner a sibling test
//! thread can fork in that moment, and its child inherits the fd until it
//! execs. An exec of the file then fails with ETXTBSY ("Text file busy"), and
//! a "no writer" check (a read lease) sees a writer. Both have flaked the full
//! suite. Here the final path is only ever written by `cp`, a child process,
//! so no write fd to it exists in the test process at all.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// A staging path outside every fixture directory, so a staged file never
/// appears in a directory a test lists or hashes.
fn staging_path() -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "axon-test-exec-stage-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ))
}

/// `cp src dst` in a child process, then set `dst`'s mode. `dst` is created,
/// or (like `std::fs::write`) truncated and rewritten in place.
fn cp(src: &Path, dst: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    let st = std::process::Command::new("cp")
        .arg("--")
        .arg(src)
        .arg(dst)
        .status()
        .unwrap();
    assert!(st.success(), "cp {} {}: {st}", src.display(), dst.display());
    std::fs::set_permissions(dst, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// Write `bytes` to `path` with `mode`, through a separate process.
pub fn write_executable(path: &Path, bytes: impl AsRef<[u8]>, mode: u32) {
    let staged = staging_path();
    std::fs::write(&staged, bytes).unwrap();
    cp(&staged, path, mode);
    std::fs::remove_file(&staged).unwrap();
}

/// Copy `src` to `dst` with `mode`, through a separate process.
pub fn copy_executable(src: impl AsRef<Path>, dst: impl AsRef<Path>, mode: u32) {
    cp(src.as_ref(), dst.as_ref(), mode);
}

/// G2a/G2c/G2d: a process a test started, known by pid AND command line
/// (`/proc/<pid>/cmdline`), so a pid the kernel has reused is never taken for
/// it, nor killed by the cleanup. A zombie's cmdline reads empty, so a killed
/// but unreaped process counts as gone. Dropping it SIGKILLs the process if it
/// is still there, so a failing test leaks nothing.
pub struct Tracked {
    pub pid: i32,
    cmdline: Vec<u8>,
}

impl Tracked {
    /// Wait (at most 20 s) until `pid_file` names a live process running
    /// exactly `argv`.
    pub fn wait_alive(pid_file: &Path, argv: &[&str]) -> Tracked {
        let mut cmdline = Vec::new();
        for a in argv {
            cmdline.extend_from_slice(a.as_bytes());
            cmdline.push(0);
        }
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        loop {
            let pid = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|s| s.trim().parse::<i32>().ok());
            if let Some(pid) = pid.filter(|&p| runs(p, &cmdline)) {
                return Tracked { pid, cmdline };
            }
            assert!(
                std::time::Instant::now() < deadline,
                "{} never named a live `{}`",
                pid_file.display(),
                argv.join(" ")
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    pub fn alive(&self) -> bool {
        runs(self.pid, &self.cmdline)
    }

    /// Gone within `d` (a SIGKILL is delivered asynchronously).
    pub fn gone_within(&self, d: std::time::Duration) -> bool {
        let deadline = std::time::Instant::now() + d;
        while self.alive() {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        true
    }
}

impl Drop for Tracked {
    fn drop(&mut self) {
        if self.alive() {
            // SAFETY: plain syscall; the pid was just checked to be ours.
            unsafe { libc::kill(self.pid, libc::SIGKILL) };
        }
    }
}

/// Is `pid` a live process whose command line is exactly `cmdline`?
fn runs(pid: i32, cmdline: &[u8]) -> bool {
    std::fs::read(format!("/proc/{pid}/cmdline")).is_ok_and(|c| c == cmdline)
}
