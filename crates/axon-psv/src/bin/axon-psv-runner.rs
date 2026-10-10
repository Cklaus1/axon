//! `axon-psv-runner` — the trusted verdict runner INSIDE the protected guest.
//! `axon-guest-init` execs it under the guest policy. It takes NO arguments:
//! every path is a fixed guest path, and the manifest digest comes from the
//! kernel command line word `axon.psv.manifest=<sha256>`, set by Fabric
//! through the launcher. Nothing the candidate controls can redirect it.
//!
//! Exit 0 = a verdict was written (whatever it says); 3 = no verdict could be
//! written. Prints `PSV-VERDICT sha256=<hex>` on the console for the launcher
//! to cross-check against `/out/verdict.json`.

use axon_psv::runner::{policy_from_cmdline, run_and_emit, RunnerConfig};
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

/// The guest's fixed configuration, from the kernel command line (a pure
/// function so a test can read the VALUES production hands the runner: the
/// uid the check runs as and the manifest digest it is held to, neither of
/// which any test of `run_and_emit` observes, since those pass their own).
fn guest_config(cmdline: &str) -> RunnerConfig {
    let expected = cmdline
        .split_whitespace()
        .find_map(|w| w.strip_prefix("axon.psv.manifest="))
        .unwrap_or("")
        .to_string();
    RunnerConfig {
        manifest: PathBuf::from("/in/job/launch-manifest.json"),
        secret: PathBuf::from("/in/job/completion-secret"),
        candidate: PathBuf::from("/in/candidate"),
        suite: PathBuf::from("/in/suite"),
        out: PathBuf::from("/out"),
        axon: PathBuf::from("/usr/bin/axon"),
        runner_exe: PathBuf::from("/proc/self/exe"),
        expected_manifest_sha256: expected,
        drop: Some((TEST_UID, TEST_GID)),
        // The policy axon-guest-init enforces, from the same cmdline: the
        // runner holds it to the manifest's policy_sha256 (PSV-6, A87).
        policy: policy_from_cmdline(cmdline),
    }
}

fn run_guest() -> Result<(), String> {
    let cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    let cfg = guest_config(&cmdline);
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

    /// Amendment 95 (eqgate4): the guest runs the check as `nobody` (65534),
    /// never as root, and holds the manifest to the digest the kernel command
    /// line names, never to an empty one. Both are VALUES in `guest_config`,
    /// handed to primitives that are rowed (setuid, the digest compare) but
    /// whose argument no other test observes: the runner suite passes its own.
    #[test]
    fn the_guest_runs_the_check_as_nobody_and_holds_the_manifest_to_the_cmdline_digest() {
        let digest = "ab".repeat(32);
        let cfg = guest_config(&format!("console=ttyS0 axon.psv.manifest={digest} quiet"));
        assert_eq!(
            cfg.drop,
            Some((65534, 65534)),
            "ATTACK: the guest runner would run the check as another identity than nobody: {:?}",
            cfg.drop
        );
        assert_eq!(
            cfg.expected_manifest_sha256, digest,
            "ATTACK: the guest runner is not held to the manifest digest the kernel command line names"
        );
        // A command line that names no digest yields an EMPTY pin, which no
        // manifest hashes to: the runner then refuses every manifest.
        assert_eq!(guest_config("console=ttyS0").expected_manifest_sha256, "");
        assert_eq!(cfg.manifest, PathBuf::from("/in/job/launch-manifest.json"));
    }

    /// Amendment 98 (eqgate5): EVERY path `guest_config` hands the runner is a
    /// production VALUE no test of `run_and_emit` observes (those pass their
    /// own). Each is a decision about WHERE the guest looks, and a swapped one
    /// moves a trust boundary: the completion secret read from a place the
    /// candidate can write (the candidate then chooses S), the interpreter that
    /// is handed S replaced by the candidate's own file, the verdict written
    /// into the candidate's tree. Host-side digests (axon_sha256, runner_sha256)
    /// and the MAC would notice some of these AFTER the fact; no test did.
    #[test]
    fn every_path_the_guest_hands_the_runner_is_the_documented_one() {
        use base64::Engine;
        let policy = b"the guest policy bytes".to_vec();
        let b64 = base64::engine::general_purpose::STANDARD.encode(&policy);
        let cfg = guest_config(&format!(
            "axon.policy={b64} axon.psv.manifest={}",
            "cd".repeat(32)
        ));
        let p = |s: &str| PathBuf::from(s);
        assert_eq!(
            cfg.manifest,
            p("/in/job/launch-manifest.json"),
            "ATTACK: guest_config manifest path"
        );
        assert_eq!(
            cfg.secret,
            p("/in/job/completion-secret"),
            "ATTACK: guest_config secret path: the completion secret would be read from another place"
        );
        assert_eq!(
            cfg.candidate,
            p("/in/candidate"),
            "ATTACK: guest_config candidate path"
        );
        assert_eq!(cfg.suite, p("/in/suite"), "ATTACK: guest_config suite path");
        assert_eq!(
            cfg.out,
            p("/out"),
            "ATTACK: guest_config out path: the verdict would be written elsewhere"
        );
        assert_eq!(
            cfg.axon,
            p("/usr/bin/axon"),
            "ATTACK: guest_config interpreter path: the interpreter that is handed the secret"
        );
        assert_eq!(
            cfg.runner_exe,
            p("/proc/self/exe"),
            "ATTACK: guest_config runner_exe path: the identity the verdict reports"
        );
        assert_eq!(
            cfg.policy,
            Some(policy),
            "ATTACK: guest_config policy: the guest policy the runner holds the manifest to"
        );
        // The secret lives in the job directory beside the manifest (the job
        // directory's xattr check judges both).
        assert_eq!(cfg.secret.parent(), cfg.manifest.parent());
    }

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

    /// C9 round 7, EQGATE3 (amendment 91): the real `set_non_dumpable` makes the
    /// runner non-dumpable (it protects the completion secret S). The
    /// `start` test above stubs the call; nothing observed the call itself, and
    /// replacing its body with `0` left every suite green. The process starts
    /// dumpable here, so a no-op is visible in `PR_GET_DUMPABLE`.
    #[test]
    fn the_real_prctl_makes_the_runner_non_dumpable() {
        let get = || unsafe { libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) };
        // SAFETY: prctl with no pointers.
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1 as libc::c_ulong, 0, 0, 0) };
        assert_eq!(get(), 1, "setup: the test process starts dumpable");
        assert_eq!(set_non_dumpable(), 0, "the prctl reports success");
        let after = get();
        // SAFETY: restore for the rest of the test process.
        unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 1 as libc::c_ulong, 0, 0, 0) };
        assert_eq!(
            after, 0,
            "ATTACK: set_non_dumpable left the runner dumpable (PR_GET_DUMPABLE = {after}): the \
             completion secret's memory is readable by the candidate's uid"
        );
    }
}
