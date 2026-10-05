//! The verifier's identity is the digest of the image it is RUNNING, whatever
//! happens to the file it was started from (C9 shardflake).
//!
//! `cargo_test_shards.py` runs several `cargo test` shards at once against one
//! target dir. Any of them can find the tree "changed" (a touched `.git/index`
//! is enough: `build.rs` watches it) and RELINK the test binary, which replaces
//! the file under the siblings still running it. `std::env::current_exe()` then
//! names `.../readiness-HASH (deleted)`; the digest read failed, the identity
//! became `"unknown"`, and a certified fixture's positive control failed with
//! "readiness_verifier_sha256 is not a sha256" -- on gpumaster only, only
//! sometimes, with the same code passing serially.
//!
//! This test makes the interleaving deterministic: a COPY of this binary is
//! started as a child, the child's file is unlinked (exactly what a relink
//! does) and only then asks for its identity. It must still be the digest of
//! the bytes that child is running.

use std::process::Command;

const CHILD: &str = "AXON_VERIFIER_IDENTITY_CHILD";

fn sha256_of(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

/// Runs only in the child copy: unlink the file this process was started from,
/// then report the identity and the running image's digest.
#[test]
fn child_reports_identity_after_its_file_is_unlinked() {
    if std::env::var_os(CHILD).is_none() {
        return;
    }
    let path = std::env::current_exe().unwrap();
    // `current_exe()` is read BEFORE the unlink: the point is that the path no
    // longer opens afterwards.
    std::fs::remove_file(&path).unwrap();
    assert!(
        std::fs::read(&path).is_err(),
        "the unlinked file must be gone"
    );
    let id = axon_fabric::readiness::verifier_identity();
    println!("IDENTITY_SHA={}", id["sha256"].as_str().unwrap());
}

#[test]
fn the_identity_survives_replacement_of_the_executable_file() {
    if std::env::var_os(CHILD).is_some() {
        return;
    }
    let me = std::fs::read(std::env::current_exe().unwrap()).unwrap();
    let want = sha256_of(&me);
    let d = tempfile::tempdir().unwrap();
    let copy = d.path().join("verifier-copy");
    std::fs::copy(std::env::current_exe().unwrap(), &copy).unwrap();
    let out = Command::new(&copy)
        .args([
            "child_reports_identity_after_its_file_is_unlinked",
            "--exact",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD, "1")
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "child failed: {stdout}\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let got = stdout
        .lines()
        .find_map(|l| l.split("IDENTITY_SHA=").nth(1))
        .map(str::trim)
        .unwrap_or_else(|| panic!("child printed no identity: {stdout}"));
    assert_eq!(
        got, want,
        "ATTACK: a verifier whose file was replaced under it reported {got:?}, \
         not the digest of the image it is running"
    );
}
