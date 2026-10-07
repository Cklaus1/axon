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

    // A symlink is refused by the symlink check and, its lstat mode being
    // 0777 on Linux, by the mode check, each alone (M948 retired against
    // M950, amendment 61): either reason.
    std::os::unix::fs::symlink(&key, root.join("alias.pub")).unwrap();
    let e = check_operator_owned_below(d.path(), &root).unwrap_err();
    assert!(e.contains("symlink") || e.contains("writable"), "{e}");
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

/// PSV-6 (C9 round 2; A67): key-role separation read an UNREADABLE authority
/// root as holding no key. `keys_in` answered an empty list on ANY `read_dir`
/// error, so for the non-root Fabric UID a verifier root it cannot list
/// (mode 000) hid the observer key it shares, and separation passed. Only
/// NotFound is absent now; anything else refuses, on the observer route and
/// the qualification route alike. The decision is made as uid 65534 (the
/// preflight requires a non-root Fabric UID): as root, this test re-runs
/// itself in a child with that uid. Controls: readable roots with the key
/// shared are refused, and with no shared key pass, as that uid too.
#[test]
fn an_unreadable_authority_root_is_never_read_as_holding_no_key() {
    use axon_fabric::observer::ObserverTrust;
    const CHILD: &str = "AXON_TEST_UNREADABLE_ROOT_BASE";
    const NAME: &str = "an_unreadable_authority_root_is_never_read_as_holding_no_key";
    if let Ok(base) = std::env::var(CHILD) {
        let base = Path::new(&base);
        let uid = unsafe { libc::geteuid() };
        // Control: this uid reads the roots it may read.
        ObserverTrust::for_test(&base.join("ctl/observer"))
            .check_separation()
            .expect("control: readable roots, no shared key");
        QualificationTrust::for_manifest(&base.join("ctl/manifest.json"))
            .trusted_keys()
            .expect("control: a readable qualification root, no shared key");
        let got = ObserverTrust::for_test(&base.join("shared/observer")).check_separation();
        assert!(
            got.is_err(),
            "ATTACK: an observer key shared with an UNREADABLE verifier root passed key-role \
             separation as uid {uid}: {got:?}"
        );
        assert!(got.unwrap_err().contains("cannot be read"));
        let got = QualificationTrust::for_manifest(&base.join("shared/manifest.json"))
            .trusted_keys()
            .map(|k| k.len());
        assert!(
            got.is_err(),
            "ATTACK: a qualification key shared with an UNREADABLE verifier root was trusted \
             as uid {uid}: {got:?}"
        );
        return;
    }
    let d = tempfile::tempdir().unwrap();
    chmod(d.path(), 0o755);
    let key = |n: u8| format!("{}\n", format!("{n:02x}").repeat(32));
    // `shared`: the verifier root also holds the observer key (1) and the
    // qualification key (3); `ctl`: it holds neither.
    for (layout, verifier_keys) in [("ctl", [2u8, 4]), ("shared", [1, 3])] {
        let top = d.path().join(layout);
        for (dir, keys) in [
            ("observer", vec![1u8]),
            ("trusted_issuers", vec![3]),
            ("verifier", verifier_keys.to_vec()),
        ] {
            std::fs::create_dir_all(top.join(dir)).unwrap();
            chmod(&top.join(dir), 0o755);
            for k in keys {
                let f = top.join(dir).join(format!("k{k}.pub"));
                std::fs::write(&f, key(k)).unwrap();
                chmod(&f, 0o644);
            }
        }
        chmod(&top, 0o755);
    }
    // Control: readable, the shared key is refused on both routes.
    let shared = d.path().join("shared");
    assert!(ObserverTrust::for_test(&shared.join("observer"))
        .check_separation()
        .is_err_and(|e| e.contains("key-role separation")));
    assert!(
        QualificationTrust::for_manifest(&shared.join("manifest.json"))
            .trusted_keys()
            .is_err_and(|e| e.contains("key-role separation"))
    );
    chmod(&shared.join("verifier"), 0o000);
    let out = if unsafe { libc::geteuid() } == 0 {
        use std::os::unix::process::CommandExt;
        std::process::Command::new(axon_fabric::readiness::running_image())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD, d.path())
            .uid(65534)
            .gid(65534)
            .output()
            .unwrap()
    } else {
        std::process::Command::new(axon_fabric::readiness::running_image())
            .args(["--exact", NAME, "--nocapture", "--test-threads=1"])
            .env(CHILD, d.path())
            .output()
            .unwrap()
    };
    chmod(&shared.join("verifier"), 0o755);
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        out.status.success() && text.contains("1 passed"),
        "the uid-65534 child:\n{text}"
    );
}
