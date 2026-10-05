//! The operator example configs (`profiles/protected-host/*.example`) are
//! loaded through the PRODUCTION loaders, with production rules (`test:
//! false`), so an example cannot drift from what the code reads: a field the
//! code now requires (amendment 65's `custodian.sha256`) and an example that
//! lacks it fail here, not on the operator's host.
//!
//! The only test liberty is the operator: the files are copied into a private
//! directory owned by this uid, which is the authority's operator and walk
//! base (a repository checkout is not root-owned from `/`). Every rule
//! `load_config` applies after reading the file is the production one.
//!
//! The host config (`protected-host.json.example`) is not loaded here: its
//! loader verifies every pin against the pinned files themselves, which do
//! not exist off a deployed host. `scripts/test_operator_deploy.sh` holds its
//! shape to the deployed config that the production loader accepted; here it
//! is held to the helper example on the fields `helper_agrees` compares.

use axon_fabric::privileged_launcher::{self, Authority};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../profiles/protected-host")
        .join(name)
}

/// A private operator directory holding `bytes` at `name` (0644), and the
/// production-rule authority whose operator is this uid.
fn operator_copy(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, Authority) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    // SAFETY: geteuid cannot fail.
    let uid = unsafe { libc::geteuid() };
    (
        p,
        Authority {
            operator_uid: uid,
            walk_base: dir.to_path_buf(),
            test: false,
        },
    )
}

#[test]
fn the_operator_examples_load_through_the_production_loaders() {
    // The helper config: privileged_launcher::load_config, production rules.
    let bytes = std::fs::read(example("protected-launcher.json.example")).unwrap();
    let d = tempfile::tempdir().unwrap();
    let (p, a) = operator_copy(d.path(), "protected-launcher.json", &bytes);
    let helper = privileged_launcher::load_config(&p, &a).unwrap_or_else(|e| {
        panic!("protected-launcher.json.example is refused by load_config: {e}")
    });
    assert!(
        helper.custodian.sha256.is_some(),
        "the example pins the custodian program"
    );

    // ATTACK (the drift this test exists for): the example without the
    // amendment-65 custodian program pin is what a pre-amendment kit wrote;
    // the production rule refuses it, so this test speaks when an example
    // lacks a field the code requires.
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["custodian"].as_object_mut().unwrap().remove("sha256");
    let d2 = tempfile::tempdir().unwrap();
    let (p2, a2) = operator_copy(
        d2.path(),
        "protected-launcher.json",
        &serde_json::to_vec_pretty(&v).unwrap(),
    );
    let got = privileged_launcher::load_config(&p2, &a2);
    assert!(
        matches!(&got, Err(e) if e.contains("custodian.sha256 must pin the axon-custodian program")),
        "ATTACK: a helper config with no custodian program pin was accepted under production rules: {got:?}"
    );

    // The custodian config: custodian::load_config, production rules.
    let cbytes = std::fs::read(example("custodian.json.example")).unwrap();
    let d3 = tempfile::tempdir().unwrap();
    let (p3, a3) = operator_copy(d3.path(), "custodian.json", &cbytes);
    let cust = axon_fabric::custodian::load_config(&p3, &a3)
        .unwrap_or_else(|e| panic!("custodian.json.example is refused by load_config: {e}"));

    // One custodian across the three examples: the helper's socket and uid
    // (what helper_agrees compares), the custodian's own, and the host's
    // observer.custodian (which carries no program pin: amendment 65).
    assert_eq!(helper.custodian.socket, cust.socket);
    assert_eq!(helper.custodian.uid, cust.custodian_uid);
    assert_eq!(helper.fabric_uid, cust.fabric_uid);
    let host: serde_json::Value =
        serde_json::from_slice(&std::fs::read(example("protected-host.json.example")).unwrap())
            .unwrap();
    assert_eq!(
        host["schema"],
        axon_fabric::protected_host::PROTECTED_HOST_SCHEMA
    );
    assert_eq!(
        host["observer"]["custodian"],
        serde_json::json!({"socket": helper.custodian.socket, "uid": helper.custodian.uid}),
        "the host example's custodian is the helper example's socket and uid, and nothing else"
    );
    assert_eq!(host["out_root"], serde_json::json!(helper.out_root));
    assert_eq!(
        host["observer"]["max_age_s"],
        serde_json::json!(helper.observer.max_age_s)
    );
}
