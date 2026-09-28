//! Operator direction (2026-09-27): "an agent with full write access to the
//! repo cannot make protected_backend turn PASS by changing any combination of
//! source, scripts, manifests, proof files, or local keys."
//!
//! The verdict is `axon_fabric::readiness` (the operator-installed
//! `axon-fabric verify-readiness`); the repository is only evidence. Each test
//! starts from a GENUINELY certified repository (PASS under a trust root the
//! test controls), applies one attack an agent with repository write access
//! can make, and asserts the component is no longer PASS for THAT reason.
//!
//! The test trust root (`ReadinessTrust::test`) exists only in test-trust
//! builds, which report `build: "test-trust"` and are never accepted by
//! readiness; the production `ReadinessTrust::operator()` is exercised last.

mod common;
use common::*;

use axon_fabric::readiness::{protected_components, ReadinessTrust, CERT_SCHEMA};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

const GATES: [&str; 3] = [
    "G13-r22-profile-qualification",
    "G13-r22-profile-eligibility",
    "G13-r22-guest-truth",
];

fn git(repo: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn head(repo: &Path) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn sha(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

struct Certified {
    _d: tempfile::TempDir,
    repo: PathBuf,
    trust: ReadinessTrust,
    operator: Issuer,
}

impl Certified {
    fn verdict(&self) -> Value {
        protected_components(&self.repo, &self.trust)["components"]["protected_backend"].clone()
    }
    fn record(&self) -> PathBuf {
        self.repo
            .join("governance/proofs/v022-protected/protected_backend.json")
    }
    fn commit(&self, msg: &str) {
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-q", "-m", msg]);
    }
    /// The component is NOT PASS, and the reason names `why`.
    fn refused(&self, why: &str) {
        let v = self.verdict();
        assert_ne!(v["status"], "PASS", "{v}");
        assert!(v.to_string().contains(why), "expected {why:?}: {v}");
    }
}

/// A repository whose protected_backend is genuinely certified: the gate rows
/// and a v2 record signed by the operator key in the test trust root.
fn certified() -> Option<Certified> {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: the test trust root must be root-owned");
        return None;
    }
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "-q", "-b", "main"]);
    let gates: Vec<Value> = GATES.iter().map(|g| json!({"gate_id": g})).collect();
    write(
        &repo.join("governance/cortex_gate_execution_registry.json"),
        &json!({"gates": gates}).to_string(),
    );
    write(
        &repo.join("governance/specs/v022-protected-suite-verdict.md"),
        "# PSV\n",
    );
    write(&repo.join("crates/axon-fabric/src/lib.rs"), "// code\n");
    write(
        &repo.join("scripts/protected_verifier_ready.py"),
        "# script\n",
    );
    write(&repo.join("profiles/linux-microvm/manifest.json"), "{}\n");
    write(
        &repo.join("governance/proofs/v022-protected/run-evidence.md"),
        "protected run\n",
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "certified tree"]);

    let root = d.path().join("trust/qualification");
    std::fs::create_dir_all(&root).unwrap();
    for p in [d.path().join("trust"), root.clone()] {
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    let operator = Issuer::generate();
    operator.trust_in(&root, "operator");
    std::fs::set_permissions(
        root.join("operator.pub"),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let trust = ReadinessTrust::test(d.path(), &root);

    let c = Certified {
        repo,
        trust,
        operator,
        _d: d,
    };
    let ev = "governance/proofs/v022-protected/run-evidence.md";
    use sha2::{Digest, Sha256};
    let bundle = format!("{:x}", Sha256::digest(sha(&c.repo.join(ev)).as_bytes()));
    let h = head(&c.repo);
    let rec = json!({
        "schema": CERT_SCHEMA, "component": "protected_backend",
        "host_profile": "linux-microvm-protected", "qualification_profile": "linux-microvm-protected",
        "psv_spec_sha256": sha(&c.repo.join("governance/specs/v022-protected-suite-verdict.md")),
        "axon_sha": h, "micode_sha": "a".repeat(40), "fabric_revision": h,
        "guest_image_sha256": "1".repeat(64), "guest_kernel_sha256": "2".repeat(64),
        "guest_runtime_sha256": "3".repeat(64),
        "suite": {"id": "acceptance", "version": "v1", "entry": "accept.ax", "test": "t_ok", "digest": "4".repeat(64)},
        "candidate_tree_ref": format!("acf1:{}", "5".repeat(64)),
        "observer_key_id": "ed25519:0000000000000000", "observation_sha256": "6".repeat(64),
        "verifier_key_id": "ed25519:1111111111111111", "b263_qualification_sha256": "7".repeat(64),
        "evidence": [ev], "evidence_bundle_sha256": bundle, "certified_at": "2026-09-28T00:00:00Z",
    });
    c.operator.write_signed(&c.record(), &rec);
    c.commit("certification (governance only)");
    let v = c.verdict();
    assert_eq!(v["status"], "PASS", "the positive control must pass: {v}");
    Some(c)
}

fn resign(c: &Certified, who: &Issuer, edit: impl FnOnce(&mut Value)) {
    let mut rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    edit(&mut rec);
    who.write_signed(&c.record(), &rec);
}

#[test]
fn a_repository_signed_or_planted_key_is_not_authority() {
    let Some(c) = certified() else { return };
    // The agent signs the record with its own key, plants that key in the
    // repository's trusted_issuers AND lists it as expected.
    let agent = Issuer::generate();
    agent.trust_in(&c.repo.join("governance/trusted_issuers"), "agent");
    write(
        &c.repo.join("governance/status/trust-expectations.json"),
        &json!({"qualification_issuers": [agent.key_id()]}).to_string(),
    );
    resign(&c, &agent, |_| {});
    c.commit("agent: my key, my signature");
    c.refused("not a trusted evidence issuer");
}

#[test]
fn the_repository_may_narrow_the_issuers_never_add() {
    let Some(c) = certified() else { return };
    write(
        &c.repo.join("governance/status/trust-expectations.json"),
        &json!({"qualification_issuers": ["ed25519:ffffffffffffffff"]}).to_string(),
    );
    c.commit("narrow away the operator");
    c.refused("not one this repository expects");
}

#[test]
fn any_change_to_source_scripts_or_manifests_invalidates_it() {
    for (file, label) in [
        ("crates/axon-fabric/src/lib.rs", "source"),
        ("scripts/protected_verifier_ready.py", "script"),
        ("profiles/linux-microvm/manifest.json", "manifest"),
        (
            "profiles/linux-microvm/trusted_issuers/agent.pub",
            "local key",
        ),
    ] {
        let Some(c) = certified() else { return };
        write(&c.repo.join(file), "changed by the agent\n");
        // Uncommitted first, then committed: neither counts.
        c.refused("changed since the certified revision");
        c.commit(label);
        c.refused("changed since the certified revision");
    }
}

#[test]
fn a_changed_spec_evidence_or_record_invalidates_it() {
    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/specs/v022-protected-suite-verdict.md"),
        "# PSV v2\n",
    );
    c.refused("certifies another version");

    let Some(c) = certified() else { return };
    write(
        &c.repo
            .join("governance/proofs/v022-protected/run-evidence.md"),
        "forged\n",
    );
    c.refused("evidence bundle changed");

    let Some(c) = certified() else { return };
    let mut bytes = std::fs::read(c.record()).unwrap();
    let s = String::from_utf8(bytes.clone())
        .unwrap()
        .replace("t_ok", "t_ko");
    bytes = s.into_bytes();
    std::fs::write(c.record(), bytes).unwrap();
    c.refused("does not verify");

    // A re-signed record for ANOTHER component does not count for this one.
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["component"] = json!("pci_on_protected_backend")
    });
    c.refused("not a linux-microvm-protected certification of protected_backend");
}

#[test]
fn gates_and_proofs_without_the_certification_are_not_pass() {
    let Some(c) = certified() else { return };
    std::fs::remove_file(c.record()).unwrap();
    c.refused("no protected-host certification record");
    assert_eq!(c.verdict()["status"], "PARTIAL");
}

/// The production trust: the operator root on this host, fixed, with no flag
/// to choose it. On a development host (no root, or a root the checking
/// process could write) a genuinely signed record still authorizes nothing.
#[test]
fn production_authority_is_the_operator_root_only() {
    let Some(c) = certified() else { return };
    let v = protected_components(&c.repo, &ReadinessTrust::operator());
    assert_eq!(v["trust_root"], "/etc/axon/trust/qualification");
    let pb = &v["components"]["protected_backend"];
    assert_ne!(pb["status"], "PASS", "{pb}");
    // Absent here: the refusal names the missing operator path, and there is
    // no fallback to anything in the repository.
    assert!(pb.to_string().contains("/etc/axon"), "{pb}");
    // This test build carries the test trust constructors, and says so.
    assert_eq!(v["build"], "test-trust");
}

/// Registering results must not invalidate the certification: a change under
/// `governance/` alone, uncommitted or committed, keeps it (regression: a
/// trimmed `git status` once misread the first dirty path's name).
#[test]
fn a_governance_only_change_keeps_the_certification() {
    let Some(c) = certified() else { return };
    write(
        &c.repo.join("governance/status/v022-pci.json"),
        "{\"state\":1}\n",
    );
    assert_eq!(c.verdict()["status"], "PASS", "{}", c.verdict());
    c.commit("status update");
    assert_eq!(c.verdict()["status"], "PASS", "{}", c.verdict());
}

/// The certification binds THIS repository's history: an operator-signed
/// record naming a revision this tree does not descend from counts for
/// nothing (and a revision it cannot even compare with is never "unchanged").
#[test]
fn a_certification_of_another_history_is_not_this_one() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["axon_sha"] = json!("0123456789abcdef0123456789abcdef01234567")
    });
    c.refused("is not an ancestor of this tree");
}

/// The trust root itself must be operator-owned: a genuinely signed record
/// verifies under a root the checker could not trust as authority.
#[test]
fn a_trust_root_that_is_not_operator_owned_authorizes_nothing() {
    let Some(c) = certified() else { return };
    let root = c.trust.issuers_dir.clone();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o777)).unwrap();
    c.refused("group- or other-writable");
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(c.verdict()["status"], "PASS");

    std::os::unix::fs::chown(root.join("operator.pub"), Some(1000), None).unwrap();
    c.refused("not root");
    std::os::unix::fs::chown(root.join("operator.pub"), Some(0), None).unwrap();

    std::os::unix::fs::symlink(root.join("operator.pub"), root.join("alias.pub")).unwrap();
    c.refused("symlink");
}
