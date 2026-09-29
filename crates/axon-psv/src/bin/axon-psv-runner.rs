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

/// Why the runner refuses when it cannot make itself non-dumpable.
const NOT_NON_DUMPABLE: &str =
    "could not make the runner non-dumpable: refusing to read the completion secret";

fn main() {
    if let Err(why) = start(std::env::args().len(), set_non_dumpable, run_guest) {
        eprintln!("axon-psv-runner: {why}");
        std::process::exit(3);
    }
}

/// The real `prctl(PR_SET_DUMPABLE, 0)`: 0 on success.
fn set_non_dumpable() -> libc::c_int {
    // SAFETY: prctl(PR_SET_DUMPABLE, 0) takes no pointers.
    unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) }
}

/// The order the runner keeps: no arguments; then NON-DUMPABLE, or nothing
/// else happens; only then `proceed` (which reads S). The two steps are
/// parameters so a test can make the prctl fail (it cannot be provoked
/// otherwise) and see that `proceed` is never reached (PSV-3, C9 certifying
/// review: the result used to be discarded, and the runner went on holding S
/// in a dumpable process). Matches `axon test --completion-key-stdin`, which
/// exits when the same call fails.
fn start(
    nargs: usize,
    make_non_dumpable: fn() -> libc::c_int,
    proceed: impl FnOnce() -> Result<(), String>,
) -> Result<(), String> {
    if nargs > 1 {
        return Err("axon-psv-runner takes no arguments".into());
    }
    // The runner's own memory holds S: not dumpable, not ptrace-attachable.
    if make_non_dumpable() != 0 {
        return Err(format!(
            "{NOT_NON_DUMPABLE} ({})",
            std::io::Error::last_os_error()
        ));
    }
    proceed()
}

fn run_guest() -> Result<(), String> {
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
        Ok((_, sha)) => {
            println!("PSV-VERDICT sha256={sha}");
            Ok(())
        }
        Err(e) => Err(format!("could not write the verdict: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    /// PSV-3 (C9 certifying review): a runner that cannot make itself
    /// non-dumpable refuses, for that reason, and never reaches the step that
    /// reads the completion secret. Control: when the prctl succeeds it does.
    #[test]
    fn a_runner_that_cannot_become_non_dumpable_never_reads_the_secret() {
        let reached = Cell::new(false);
        let e = match start(
            1,
            || -1,
            || {
                reached.set(true);
                Ok(())
            },
        ) {
            Ok(()) => panic!("the prctl failed and the runner proceeded to read S"),
            Err(e) => e,
        };
        assert!(e.starts_with(NOT_NON_DUMPABLE), "{e}");
        assert!(!reached.get(), "went on to read S in a dumpable process");

        // Control: the prctl succeeds, and the runner proceeds.
        start(
            1,
            || 0,
            || {
                reached.set(true);
                Ok(())
            },
        )
        .unwrap();
        assert!(reached.get());
        // …and the argument refusal still comes first.
        reached.set(false);
        let e = start(
            2,
            || 0,
            || {
                reached.set(true);
                Ok(())
            },
        )
        .unwrap_err();
        assert_eq!(e, "axon-psv-runner takes no arguments");
        assert!(!reached.get());
    }
}
