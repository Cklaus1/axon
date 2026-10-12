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

pub mod scripted;

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

/// The `cortex` binary with the ranking step pinned to spectrum.
///
/// Without this, a run that localizes its own target would consult the default
/// model ranker whenever the environment running the suite holds
/// `OPENROUTER_API_KEY` — a network call, a spend, and a candidate order that
/// is no longer the one these rows assert. The ranker has its own hermetic
/// tests (`tests/rank.rs`); the rows that check its CLI wiring set the
/// variables back explicitly. `CORTEX_LABEL_LOG` is cleared for the same
/// reason: a log named by the developer's shell must not collect test runs.
pub fn cortex() -> std::process::Command {
    let mut c = std::process::Command::new(env!("CARGO_BIN_EXE_cortex"));
    c.env_remove("OPENROUTER_API_KEY")
        .env_remove("CORTEX_LABEL_LOG")
        .env("CORTEX_RANKER", "spectrum")
        .env("CORTEX_AUDIT", "off");
    c
}
