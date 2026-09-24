//! B262: the library launch API, exercised directly (not through the CLI).
//!
//! The live cases boot the custom `axon-guest-kernel` under Firecracker/KVM via
//! `axon_vm::run_in_firecracker`. They SKIP (with a printed reason, and the
//! skip is visible in the output) when firecracker, /dev/kvm or the built
//! kernel is absent — they never pass vacuously as a "launch succeeded".

use std::path::{Path, PathBuf};

use axon_vm::{run_in_firecracker, GuestOutcome, LaunchSpec, MmdsPayload, BACKEND_PROFILE};

#[test]
fn backend_profile_is_truthful_about_what_it_is() {
    let p = BACKEND_PROFILE;
    assert_eq!(p.id, "axon-metal-fc-nojailer");
    assert_eq!(p.engine, "firecracker");
    assert!(!p.jailer, "axon-vm never invokes jailer");
    assert!(!p.linux_guest, "the guest is the custom Axon kernel");
    assert_eq!(p.guest_os, None);
    assert!(!p.guest.to_ascii_lowercase().contains("linux"));
    assert!(
        !p.qualified_protected,
        "nothing has physically qualified this profile (B263)"
    );
    assert!(
        !p.satisfies_protected_linux_microvm(),
        "must never satisfy a protected-Linux-microVM requirement"
    );
    // The label a consumer sees must name the missing enclosure.
    assert!(p.enclosure.contains("no jailer"));
}

#[test]
fn run_result_ok_describes_the_guest() {
    use axon_vm::RunResult;
    let r = |exit_code, outcome| RunResult { exit_code, outcome };
    assert!(r(0, GuestOutcome::Exited(0)).ok());
    assert!(!r(3, GuestOutcome::Exited(3)).ok());
    assert!(!r(8, GuestOutcome::Violation).ok());
    assert!(!r(124, GuestOutcome::Timeout).ok());
    assert!(!r(0, GuestOutcome::Timeout).ok());
}

fn live_prereqs() -> Option<PathBuf> {
    let kernel = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/x86_64-axon-metal/release/axon-guest-kernel");
    let fc = ["/usr/local/bin/firecracker", "/opt/firecracker/firecracker"]
        .iter()
        .any(|p| Path::new(p).exists())
        || axon_vm::firecracker::which_firecracker().is_ok();
    if !fc || !Path::new("/dev/kvm").exists() || !kernel.exists() {
        eprintln!(
            "lib_launch: SKIPPED live boot (firecracker={fc}, kvm={}, kernel={})",
            Path::new("/dev/kvm").exists(),
            kernel.exists()
        );
        return None;
    }
    Some(kernel.canonicalize().unwrap())
}

fn launch(kernel: &Path, effects: &[&str]) -> axon_vm::RunResult {
    let dir = tempfile::tempdir().unwrap();
    let prog = dir.path().join("p.ax");
    std::fs::write(&prog, "fn main() { println(\"hi\") }\n").unwrap();
    let initrd = dir.path().join("initrd");
    std::fs::write(&initrd, "x").unwrap();
    let sock = dir.path().join("fc.sock");
    let mmds = MmdsPayload {
        schema: "axon-vm-mmds/1".into(),
        run_id: "lib-launch-test".into(),
        principal: None,
        allowed_effects: Some(effects.iter().map(|s| s.to_string()).collect()),
        budget_tokens: None,
        source_hash: None,
        seccomp_bpf_b64: None,
    };
    std::env::set_var("AXON_VM_QUIET", "1");
    std::env::set_var("AXON_VM_TIMEOUT_SECS", "20");
    run_in_firecracker(&LaunchSpec {
        program: &prog,
        kernel,
        initrd: &initrd,
        mem_mib: 128,
        vcpus: 1,
        vsock_port: 5000,
        socket_path: &sock,
        mmds: &mmds,
        principal_mem_mib: None,
    })
    .expect("launcher drove the Firecracker API")
}

/// Grant covers what the guest does → a clean exit 0 reported by the guest.
/// Then withhold FS → the in-guest gate refuses the substrate's `openat` and the
/// library reports a Violation (exit 8), not ok. One test so the two boots do
/// not race on the process-wide env vars above.
#[test]
fn live_boot_through_the_library_reports_the_guest_verdict() {
    let Some(kernel) = live_prereqs() else { return };

    let ok = launch(&kernel, &["IO", "FS"]);
    assert_eq!(ok.outcome, GuestOutcome::Exited(0), "{ok:?}");
    assert!(ok.ok());

    let denied = launch(&kernel, &["IO"]);
    assert_eq!(denied.outcome, GuestOutcome::Violation, "{denied:?}");
    assert_eq!(denied.exit_code, 8);
    assert!(!denied.ok());
}
