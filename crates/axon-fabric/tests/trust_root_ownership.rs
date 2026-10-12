//! The operator-ownership walk (`operator_trust::check_owned_chain`), judged
//! where it decides: a genuinely certified repository (readiness PASS under a
//! root-owned test trust root) whose trust root is then made agent-writable
//! in ONE way. Every key and signature stays genuine, so the ownership walk is
//! the only thing that can refuse it (C9 round 4b, amendment 61: the walk's
//! three refusals had no mutation rows). Each test restores the root and
//! checks the control: PASS again.

mod common;
mod readiness_fixture;
use readiness_fixture::*;
use std::os::unix::fs::PermissionsExt;

fn attack(c: &Certified, what: &str, why: &str) {
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: {what}, and readiness still certified PASS: {v}"
    );
    assert!(v.to_string().contains(why), "expected {why:?}: {v}");
}

fn control(c: &Certified) {
    assert_eq!(
        c.verdict()["status"],
        "PASS",
        "control: the restored root certifies"
    );
}

/// Mode: a group/other-writable trust root (any uid could add a key).
#[test]
fn an_other_writable_trust_root_authorizes_nothing() {
    let Some(c) = certified() else { return };
    control(&c);
    let root = c.trust.issuers_dir.clone();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    attack(
        &c,
        "the trust root is other-writable",
        "group- or other-writable",
    );
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    control(&c);
}

/// Owner: a key file owned by another uid (that uid could replace the key).
#[test]
fn a_trust_root_key_owned_by_another_uid_authorizes_nothing() {
    let Some(c) = certified() else { return };
    let key = c.trust.issuers_dir.join("operator.pub");
    std::os::unix::fs::chown(&key, Some(1000), None).unwrap();
    attack(&c, "a trust-root key is owned by uid 1000", "not root");
    std::os::unix::fs::chown(&key, Some(0), None).unwrap();
    control(&c);
}

/// Symlink: a key entry that is a symlink (it could be redirected). On Linux
/// a symlink's own mode is 0777, so the mode refusal also stands; the
/// assertion accepts either reason (the two are a four-cell pair, M948/M950).
#[test]
fn a_symlinked_trust_root_key_authorizes_nothing() {
    let Some(c) = certified() else { return };
    let root = c.trust.issuers_dir.clone();
    let alias = root.join("alias.pub");
    std::os::unix::fs::symlink(root.join("operator.pub"), &alias).unwrap();
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: a trust-root key is a symlink, and readiness still certified PASS: {v}"
    );
    assert!(
        v.to_string().contains("symlink") || v.to_string().contains("other-writable"),
        "{v}"
    );
    std::fs::remove_file(&alias).unwrap();
    control(&c);
}
