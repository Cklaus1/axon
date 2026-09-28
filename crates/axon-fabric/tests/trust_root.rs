//! The protected profile's trust root is the OPERATOR's, outside any
//! repository: `/etc/axon/trust`, root-owned, not group/other-writable, no
//! symlinks. A caller cannot choose it, and a repository directory (agent
//! mutable) never counts in production (operator direction 2026-09-27).

use axon_fabric::backend::{
    check_operator_owned, check_operator_owned_below, QualificationTrust, TrustAuthority,
    OPERATOR_TRUST_ROOT,
};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

fn chmod(p: &Path, mode: u32) {
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

#[test]
fn production_trust_is_the_operator_root_and_must_be_operator_owned() {
    let t = QualificationTrust::operator();
    assert_eq!(
        t.issuers_dir,
        Path::new(OPERATOR_TRUST_ROOT).join("qualification")
    );
    assert!(t.operator_owned);
    // The repository constructor is development-only: never operator-owned.
    assert!(
        !QualificationTrust::for_manifest(Path::new("profiles/linux-microvm/manifest.json"))
            .operator_owned
    );
}

/// Ownership is checked on the directory and every entry. This test needs to
/// create root-owned files and to chown them away, so it runs only as root.
#[test]
fn an_agent_writable_root_authorizes_nothing() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let d = tempfile::tempdir().unwrap();
    let root = d.path().join("qualification_issuers");
    std::fs::create_dir(&root).unwrap();
    chmod(&root, 0o755);
    let key = root.join("operator.pub");
    std::fs::write(&key, "ab".repeat(32)).unwrap();
    chmod(&key, 0o644);
    check_operator_owned_below(d.path(), &root).expect("root-owned, 0755/0644");

    chmod(&key, 0o666);
    assert!(check_operator_owned_below(d.path(), &root)
        .unwrap_err()
        .contains("writable"));
    chmod(&key, 0o644);

    chmod(&root, 0o777);
    assert!(check_operator_owned_below(d.path(), &root)
        .unwrap_err()
        .contains("writable"));
    chmod(&root, 0o755);

    std::os::unix::fs::chown(&key, Some(1000), None).unwrap();
    assert!(check_operator_owned_below(d.path(), &root)
        .unwrap_err()
        .contains("not root"));
    std::os::unix::fs::chown(&key, Some(0), None).unwrap();

    std::os::unix::fs::symlink(&key, root.join("alias.pub")).unwrap();
    assert!(check_operator_owned_below(d.path(), &root)
        .unwrap_err()
        .contains("symlink"));
}

#[test]
fn a_caller_cannot_choose_the_protected_trust_root() {
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .args([
            "submit",
            "--request",
            "-",
            "--journal",
            "/nonexistent/j",
            "--check-registry",
            "/nonexistent/c",
            "--grant-registry",
            "/nonexistent/g",
            "--store",
            "/nonexistent/s",
            "--tenant",
            "t",
            "--family",
            "f",
            "--linux-launcher",
            "/x",
            "--linux-manifest",
            "/x",
            "--linux-evidence",
            "/x",
            "--linux-out-root",
            "/x",
            "--linux-trusted-issuers",
            "/tmp/mine",
        ])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!out.status.success());
    assert!(
        text.contains("--linux-trusted-issuers is not accepted"),
        "{text}"
    );
}

/// One root PER AUTHORITY, each a fixed absolute path, so a key trusted for
/// one purpose never becomes valid for another; and a relative path is never
/// a trust root.
#[test]
fn each_authority_has_its_own_fixed_root() {
    let dirs: Vec<std::path::PathBuf> = [
        TrustAuthority::Qualification,
        TrustAuthority::Observer,
        TrustAuthority::Verifier,
        TrustAuthority::Admission,
    ]
    .iter()
    .map(|a| a.operator_dir())
    .collect();
    for d in &dirs {
        assert!(
            d.is_absolute() && d.starts_with(OPERATOR_TRUST_ROOT),
            "{}",
            d.display()
        );
    }
    let unique: std::collections::BTreeSet<_> = dirs.iter().collect();
    assert_eq!(unique.len(), dirs.len(), "authorities share a root");
    assert!(
        check_operator_owned(Path::new("profiles/linux-microvm/trusted_issuers"))
            .unwrap_err()
            .contains("not an absolute path")
    );
}
