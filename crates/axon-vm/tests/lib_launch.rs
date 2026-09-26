//! B262: the library launch API, exercised directly (not through the CLI).
//!
//! The live cases boot the custom `axon-guest-kernel` under Firecracker/KVM via
//! `axon_vm::run_in_firecracker`. They SKIP when firecracker, /dev/kvm or the
//! built kernel is absent — and the skip is COUNTED (target/harness-skips.log)
//! and FATAL under `AXON_HARNESS_STRICT=1`, so it never passes vacuously as a
//! "launch succeeded". Set `AXON_GUEST_KERNEL` to point at a built kernel.

use std::path::{Path, PathBuf};

use axon_vm::{
    admit, run_in_firecracker, AdmitRequest, FirecrackerBin, GuestOutcome, KernelPin, LaunchSpec,
    BACKEND_PROFILE,
};

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

/// Record a skipped live case the way `crates/axon-core/tests/cli_run.rs`
/// does: appended to the workspace `target/harness-skips.log` (which
/// `scripts/gate.sh` truncates per run and reports in its coverage notice) and
/// FATAL under `AXON_HARNESS_STRICT=1`. A bare `eprintln!` + `return` reported
/// this test GREEN on every host without the kernel artifact — a boot-and-deny
/// test that measured nothing was indistinguishable from one that passed.
fn note_skip(what: &str) {
    let log = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/harness-skips.log");
    if let Some(d) = log.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
    {
        use std::io::Write;
        let _ = writeln!(f, "{what}");
    }
    if std::env::var("AXON_HARNESS_STRICT").as_deref() == Ok("1") {
        panic!(
            "SKIPPED under AXON_HARNESS_STRICT=1: {what}\n\
             This test measured NOTHING. Provide the prerequisite or drop \
             AXON_HARNESS_STRICT."
        );
    }
}

/// Where the freestanding guest kernel is looked for, in order:
/// `AXON_GUEST_KERNEL` (explicit), then `x86_64-axon-metal/release/` under the
/// target dir THIS test was built into (so a custom `CARGO_TARGET_DIR` is
/// honoured — the binary under test lives there), then the workspace `target/`
/// (where `scripts/build-guest-image.sh` puts it by default).
fn kernel_candidates() -> Vec<PathBuf> {
    let mut v = Vec::new();
    if let Some(p) = std::env::var_os("AXON_GUEST_KERNEL") {
        v.push(PathBuf::from(p));
        return v; // explicit means explicit: no silent fallback
    }
    let rel = "x86_64-axon-metal/release/axon-guest-kernel";
    // CARGO_BIN_EXE_axon-vm = <target>/<profile>/axon-vm
    if let Some(target) = Path::new(env!("CARGO_BIN_EXE_axon-vm"))
        .parent()
        .and_then(Path::parent)
    {
        v.push(target.join(rel));
    }
    v.push(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../target")
            .join(rel),
    );
    v
}

fn live_prereqs() -> Option<PathBuf> {
    let cands = kernel_candidates();
    let kernel = cands.iter().find(|p| p.exists()).cloned();
    let fc = ["/usr/local/bin/firecracker", "/opt/firecracker/firecracker"]
        .iter()
        .any(|p| Path::new(p).exists())
        || FirecrackerBin::resolve().is_ok();
    let kvm = Path::new("/dev/kvm").exists();
    match kernel {
        Some(k) if fc && kvm => Some(k.canonicalize().unwrap()),
        _ => {
            let what = format!(
                "axon-vm lib_launch live boot (firecracker={fc}, kvm={kvm}, kernel={}; \
                 looked in {})",
                kernel.is_some(),
                cands
                    .iter()
                    .map(|p| p.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
            eprintln!("lib_launch: SKIPPED — {what}");
            note_skip(&what);
            None
        }
    }
}

fn launch(kernel: &Path, effects: &[&str]) -> axon_vm::RunResult {
    let dir = tempfile::tempdir().unwrap();
    let prog = dir.path().join("p.ax");
    std::fs::write(&prog, "fn main() { println(\"hi\") }\n").unwrap();
    let initrd = dir.path().join("initrd");
    std::fs::write(&initrd, "x").unwrap();
    let sock = dir.path().join("fc.sock");
    // D-019: the library launch goes through admission like the CLI does. The
    // test pins the digest of the very kernel it boots — a pin, not TOFU.
    let digest = hex_sha256(kernel);
    let effects: Vec<String> = effects.iter().map(|s| s.to_string()).collect();
    let admitted = admit(&AdmitRequest {
        run_id: "lib-launch-test",
        kernel,
        principal: None,
        manifest_effects: Some(&effects),
        principal_effects: None,
        effects_override: None,
        budget_tokens: None,
        source_hash: None,
        seccomp_bpf_b64: None,
        kernel_pin: KernelPin::Verify {
            expect_digest: Some(&digest),
            baseline: &dir.path().join("no-baseline"),
        },
        extended_tcb: None,
    })
    .expect("pinned kernel is admitted");
    let firecracker = FirecrackerBin::resolve().expect("firecracker resolved");
    std::env::set_var("AXON_VM_QUIET", "1");
    std::env::set_var("AXON_VM_TIMEOUT_SECS", "20");
    run_in_firecracker(&LaunchSpec {
        admitted: &admitted,
        firecracker: &firecracker,
        program: &prog,
        initrd: &initrd,
        mem_mib: 128,
        vcpus: 1,
        vsock_port: 5000,
        socket_path: &sock,
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

fn hex_sha256(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
}
