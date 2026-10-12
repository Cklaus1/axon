//! The development bypass is NOT in a default build.
//!
//! Linux copies unrecognised `NAME=value` kernel-cmdline words into init's
//! environment, so a runtime `AXON_GUEST_ALLOW_NO_POLICY=1` would let anyone who
//! can append one word to the cmdline start the guest with no effect ceiling,
//! no token cap and no seccomp. The bypass now lives behind the non-default
//! cargo feature `dev-allow-no-policy`; this test drives the REAL binary built
//! with default features and asserts it refuses — and that the target program
//! was never executed (absence of the effect, not just a message).
//!
//! Deliberately NOT `cfg`-gated on the feature: a build that has the bypass
//! must turn this RED rather than compile the test away.

use std::process::Command;

#[test]
fn default_build_refuses_with_no_policy_even_with_the_bypass_variable_set() {
    // Precondition: the host's own cmdline carries no policy word. The binary
    // reads /proc/cmdline; if this host's did carry one the test would not be
    // testing the no-policy path. Fail loudly rather than skip.
    let host_cmdline = std::fs::read_to_string("/proc/cmdline").unwrap_or_default();
    assert!(
        !host_cmdline.contains("axon.policy="),
        "host /proc/cmdline carries axon.policy= — this test cannot exercise the \
         no-policy path here"
    );

    let dir = std::env::temp_dir().join(format!("axon-guest-init-bypass-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let marker = dir.join("target-ran");
    let _ = std::fs::remove_file(&marker);

    let out = Command::new(env!("CARGO_BIN_EXE_axon-guest-init"))
        .arg("/bin/sh")
        .arg("-c")
        .arg(format!("echo ran > '{}'", marker.display()))
        .env("AXON_GUEST_ALLOW_NO_POLICY", "1")
        .output()
        .expect("spawn axon-guest-init");
    let stderr = String::from_utf8_lossy(&out.stderr);

    let ran = marker.exists();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(
        !ran,
        "the target program RAN with no policy — the bypass is compiled into the \
         default build. stderr: {stderr}"
    );
    assert_eq!(out.status.code(), Some(1), "stderr: {stderr}");
    assert!(stderr.contains("REFUSING"), "stderr: {stderr}");
    assert!(stderr.contains("policy ABSENT"), "stderr: {stderr}");
    assert!(
        !stderr.contains("BYPASS"),
        "the default build must not acknowledge the bypass variable: {stderr}"
    );
}

/// The same property at the artefact level, the check an image build can make
/// without booting anything: a default-features binary does not contain the
/// bypass variable's NAME at all, because the only code that spells it is
/// compiled out. (`scripts/build-guest-image.sh` applies this to the musl
/// release binary it installs into the B263 rootfs.)
#[test]
fn default_build_binary_does_not_contain_the_bypass_variable_name() {
    let bin = std::fs::read(env!("CARGO_BIN_EXE_axon-guest-init")).expect("read binary");
    let needle = b"AXON_GUEST_ALLOW_NO_POLICY";
    assert!(
        !bin.windows(needle.len()).any(|w| w == needle),
        "the default-features axon-guest-init binary contains `AXON_GUEST_ALLOW_NO_POLICY` \
         — the no-policy bypass is compiled in"
    );
}
