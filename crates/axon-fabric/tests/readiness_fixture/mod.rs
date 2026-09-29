#![allow(dead_code)]
//! The certified-repository fixture shared by the readiness tests: a
//! repository whose protected_backend is genuinely PASS under a test trust
//! root, which each test then attacks.

use crate::common::*;
use axon_fabric::readiness::{
    protected_components, verifier_identity, ReadinessTrust, CERT_SCHEMA, TRUST_PREFLIGHT_SCHEMA,
};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const PREFLIGHT: &str = "governance/proofs/v022-protected/trust-preflight.json";

pub const GATES: [&str; 3] = [
    "G13-r22-profile-qualification",
    "G13-r22-profile-eligibility",
    "G13-r22-guest-truth",
];

pub fn git(repo: &Path, args: &[&str]) {
    let st = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

pub fn head(repo: &Path) -> String {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

pub fn sha(p: &Path) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(std::fs::read(p).unwrap()))
}

pub fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

pub struct Certified {
    pub _d: tempfile::TempDir,
    pub repo: PathBuf,
    pub trust: ReadinessTrust,
    pub operator: Issuer,
}

impl Certified {
    pub fn verdict(&self) -> Value {
        protected_components(&self.repo, &self.trust)["components"]["protected_backend"].clone()
    }
    pub fn record(&self) -> PathBuf {
        self.repo
            .join("governance/proofs/v022-protected/protected_backend.json")
    }
    pub fn commit(&self, msg: &str) {
        git(&self.repo, &["add", "-A"]);
        git(&self.repo, &["commit", "-q", "-m", msg]);
    }
    /// The component is NOT PASS, and the reason names `why`.
    pub fn refused(&self, why: &str) {
        let v = self.refused_any();
        assert!(v.to_string().contains(why), "expected {why:?}: {v}");
    }
    /// The component is NOT PASS, for any reason: for an attack that several
    /// independent layers refuse, each alone (a four-cell record), so no
    /// assertion may depend on WHICH layer refused it (C9 round 1).
    #[allow(dead_code)]
    pub fn refused_any(&self) -> Value {
        let v = self.verdict();
        assert_ne!(
            v["status"], "PASS",
            "ATTACK: certified PASS despite the attack: {v}"
        );
        v
    }
}

/// A repository whose protected_backend is genuinely certified: the gate rows
/// and a v2 record signed by the operator key in the test trust root.
pub fn certified() -> Option<Certified> {
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
    write(
        &repo.join(PREFLIGHT),
        &json!({"schema": TRUST_PREFLIGHT_SCHEMA, "mode": "protected", "verdict": "PASS"})
            .to_string(),
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
    let pf = sha(&c.repo.join(PREFLIGHT));
    let bundle = format!(
        "{:x}",
        Sha256::digest(format!("{}{pf}", sha(&c.repo.join(ev))).as_bytes())
    );
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
        "evidence": [ev, PREFLIGHT], "evidence_bundle_sha256": bundle,
        "readiness_verifier_sha256": verifier_identity()["sha256"],
        "trust_preflight_sha256": pf,
        "certified_at": "2026-09-28T00:00:00Z",
    });
    c.operator.write_signed(&c.record(), &rec);
    c.commit("certification (governance only)");
    let v = c.verdict();
    assert_eq!(v["status"], "PASS", "the positive control must pass: {v}");
    Some(c)
}

pub fn resign(c: &Certified, who: &Issuer, edit: impl FnOnce(&mut Value)) {
    let mut rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    edit(&mut rec);
    who.write_signed(&c.record(), &rec);
}

/// `git` in `repo`, returning stdout exactly (binary-safe); asserts success.
pub fn git_raw(repo: &Path, args: &[&str]) -> Vec<u8> {
    let o = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .output()
        .unwrap();
    assert!(
        o.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    o.stdout
}

pub fn git_out(repo: &Path, args: &[&str]) -> String {
    String::from_utf8(git_raw(repo, args))
        .unwrap()
        .trim()
        .to_string()
}

/// The code file the fixture certifies.
pub const CODE: &str = "crates/axon-fabric/src/lib.rs";

impl Certified {
    /// The revision the operator certified (`axon_sha` in the signed record).
    pub fn certified_sha(&self) -> String {
        let rec: Value = serde_json::from_slice(&std::fs::read(self.record()).unwrap()).unwrap();
        rec["axon_sha"].as_str().unwrap().to_string()
    }
    /// The agent changes the certified code and commits it. Returns (the
    /// certified-tree HEAD before, the agent's commit).
    pub fn commit_changed_code(&self) -> (String, String) {
        let before = head(&self.repo);
        write(&self.repo.join(CODE), "// the agent's code\n");
        self.commit("agent: changed code");
        (before, head(&self.repo))
    }
    pub fn worktree_code(&self) -> String {
        std::fs::read_to_string(self.repo.join(CODE)).unwrap()
    }
}
