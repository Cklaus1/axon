//! `axon-psv-runner` — the trusted verdict runner INSIDE the protected guest.
//! `axon-guest-init` execs it under the guest policy. It takes NO arguments:
//! every path is a fixed guest path, and the manifest digest comes from the
//! kernel command line word `axon.psv.manifest=<sha256>`, set by Fabric
//! through the launcher. Nothing the candidate controls can redirect it.
//!
//! Exit 0 = a verdict was written (whatever it says); 3 = no verdict could be
//! written. Prints `PSV-VERDICT sha256=<hex>` on the console for the launcher
//! to cross-check against `/out/verdict.json`.

use axon_psv::runner::{run_and_emit, RunnerConfig};
use std::path::PathBuf;

/// The unprivileged identity the test runs as (the rootfs's `nobody`).
const TEST_UID: u32 = 65534;
const TEST_GID: u32 = 65534;

fn main() {
    if std::env::args().len() > 1 {
        eprintln!("axon-psv-runner takes no arguments");
        std::process::exit(3);
    }
    // The runner's own memory holds S: not dumpable, not ptrace-attachable.
    unsafe {
        libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0);
    }
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let expected = cmdline
        .split_whitespace()
        .find_map(|w| w.strip_prefix("axon.psv.manifest="))
        .unwrap_or("")
        .to_string();
    let cfg = RunnerConfig {
        manifest: PathBuf::from("/in/job/launch-manifest.json"),
        secret: PathBuf::from("/in/job/completion-secret"),
        candidate: PathBuf::from("/in/candidate"),
        suite: PathBuf::from("/in/suite"),
        out: PathBuf::from("/out"),
        axon: PathBuf::from("/usr/bin/axon"),
        runner_exe: PathBuf::from("/proc/self/exe"),
        expected_manifest_sha256: expected,
        drop: Some((TEST_UID, TEST_GID)),
        effect_ceiling: std::env::var("AXON_ALLOWED_EFFECTS").ok(),
    };
    match run_and_emit(&cfg) {
        Ok((_, sha)) => println!("PSV-VERDICT sha256={sha}"),
        Err(e) => {
            eprintln!("axon-psv-runner: could not write the verdict: {e}");
            std::process::exit(3);
        }
    }
}
