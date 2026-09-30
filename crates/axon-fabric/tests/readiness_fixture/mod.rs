#![allow(dead_code)]
//! The certified-repository fixture shared by the readiness tests: a
//! repository whose protected_backend is genuinely PASS under a test trust
//! root, which each test then attacks.

use crate::common::*;
use axon_fabric::backend::{parse_utc, Clock, TrustAuthority};
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
    /// The key in the operator's OBSERVER root that signed the observation.
    pub observer: Issuer,
    /// The key in the operator's VERIFIER root the record names.
    pub verifier: Issuer,
}

/// The certified observation and B263 record (repository paths).
pub const OBSERVATION: &str = "governance/proofs/v022-protected/observation.json";
pub const B263: &str = "governance/proofs/v022-protected/b263.json";
/// The certified guest: (record field, B263 artifact, digest).
pub const GUEST: [(&str, &str, &str); 3] = [
    (
        "guest_kernel_sha256",
        "vmlinux",
        "2222222222222222222222222222222222222222222222222222222222222222",
    ),
    (
        "guest_image_sha256",
        "rootfs.sqfs",
        "1111111111111111111111111111111111111111111111111111111111111111",
    ),
    (
        "guest_runtime_sha256",
        "axon",
        "3333333333333333333333333333333333333333333333333333333333333333",
    ),
];

/// The fixture's decision time. Readiness is pinned to it, so the genuine
/// B263 record below stays CURRENT (its `end` within Fabric's 30 days).
pub const FIXTURE_NOW: &str = "2026-09-28T12:00:00Z";
/// When the run was observed, and when the operator certified it.
pub const OBSERVED_AT: &str = "2026-09-28T00:00:00Z";
pub const CERTIFIED_AT: &str = "2026-09-28T06:00:00Z";
/// The engine the observed launch ran, and the B263 record qualified.
pub const FC_SHA: &str = "8888888888888888888888888888888888888888888888888888888888888888";
pub const JAILER_SHA: &str = "6666666666666666666666666666666666666666666666666666666666666666";

pub fn at(t: &str) -> Clock {
    Clock::FixedUnix(parse_utc(t).unwrap())
}

/// An observation of the certified run by `observer` (fabric revision `rev`).
pub fn observation(observer: &Issuer, rev: &str) -> Value {
    json!({
        "schema": "axon-preflight-observation/1",
        "observer_key_id": observer.key_id(),
        "nonce": "0".repeat(32), "epoch": 1, "observed_at": OBSERVED_AT,
        "host_profile": "linux-microvm-protected", "fabric_revision": rev,
        "firecracker_sha256": FC_SHA, "launcher_sha256": "9".repeat(64),
        "host_config_sha256": "a".repeat(64),
        "guest": {"kernel_sha256": GUEST[0].2, "rootfs_sha256": GUEST[1].2,
                  "axon_sha256": GUEST[2].2, "init_sha256": "b".repeat(64)},
        "verifier_sha256": "c".repeat(64), "suite_registry_sha256": "d".repeat(64),
        "policy_sha256": "e".repeat(64), "intended_launch_manifest_sha256": "f".repeat(64),
    })
}

/// A GENUINE B263 record qualifying the certified guest, issued under
/// `issuer`: one Fabric's own qualification rules accept (PASS, no FAIL, a
/// PASS counted, fresh at [`FIXTURE_NOW`], a clean tree, a host, a caveat,
/// engine digests, the issuer it is signed by). The review found the
/// fixture's record had none of these and still certified (C9 round 3).
pub fn b263_record(issuer: &Issuer) -> Value {
    json!({
        "schema": "axon-b263-evidence/1",
        "work_package": "B263",
        "issuer_key_id": issuer.key_id(),
        "host": "WSL2-nested",
        "caveat": "Nested virtualization; the L0 hypervisor is outside the qualified boundary.",
        "source": {"axon_git_rev": "0".repeat(40), "tree_dirty": false},
        "engine": {"firecracker": "Firecracker v1.10.1", "firecracker_sha256": FC_SHA,
                   "jailer": "Jailer v1.10.1", "jailer_sha256": JAILER_SHA},
        "profile": {"name": "linux-microvm-protected", "manifest_sha256": "7".repeat(64),
                    "artifacts": {
                        "vmlinux": {"sha256": GUEST[0].2},
                        "rootfs.sqfs": {"sha256": GUEST[1].2},
                        "axon": {"sha256": GUEST[2].2}}},
        "assertions": [
            {"name": "a1_boot_runs_ax_expected_stdout", "status": "PASS"},
            {"name": "x3_l0_hypervisor_boundary", "status": "PASS"}
        ],
        "counts": {"total": 2, "PASS": 2, "FAIL": 0, "BLOCKED": 0},
        "result": "PASS",
        "start": "2026-09-27T23:00:00Z",
        "end": "2026-09-27T23:30:00Z",
    })
}

/// Write `v` to `path` and its `authority`-domain signature by `who` to `.sig`.
pub fn write_signed_for(who: &Issuer, authority: TrustAuthority, path: &Path, v: &Value) {
    let bytes = serde_json::to_vec_pretty(v).unwrap();
    write(path, std::str::from_utf8(&bytes).unwrap());
    std::fs::write(sig_of(path), who.sign_for(authority, &bytes)).unwrap();
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
    // Generated output is ignored as in the real repository (an allowlisted
    // path must also pass readiness's untracked listing, which can only add).
    write(&repo.join(".gitignore"), "/target/\n/dist/\n.env\n");
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
    // The fabric revision the observation names: a fixed commit id (the
    // record certifies the same one).
    let rev = "f".repeat(40);
    let operator = Issuer::generate();
    let observer = Issuer::generate();
    let verifier = Issuer::generate();
    write_signed_for(
        &observer,
        TrustAuthority::Observer,
        &repo.join(OBSERVATION),
        &observation(&observer, &rev),
    );
    write_signed_for(
        &operator,
        TrustAuthority::Qualification,
        &repo.join(B263),
        &b263_record(&operator),
    );
    git(&repo, &["add", "-A"]);
    git(&repo, &["commit", "-q", "-m", "certified tree"]);

    std::fs::create_dir_all(d.path().join("trust")).unwrap();
    std::fs::set_permissions(
        d.path().join("trust"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    for (dir, who, name) in [
        ("qualification", &operator, "operator"),
        ("observer", &observer, "observer"),
        ("verifier", &verifier, "verifier"),
    ] {
        let root = d.path().join("trust").join(dir);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o755)).unwrap();
        who.trust_in(&root, name);
        std::fs::set_permissions(
            root.join(format!("{name}.pub")),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
    }
    let root = d.path().join("trust/qualification");
    let trust = ReadinessTrust::test(d.path(), &root).at(at(FIXTURE_NOW));

    let c = Certified {
        repo,
        trust,
        operator,
        observer,
        verifier,
        _d: d,
    };
    let ev = "governance/proofs/v022-protected/run-evidence.md";
    let evidence = [ev, PREFLIGHT, OBSERVATION, B263];
    let pf = sha(&c.repo.join(PREFLIGHT));
    let h = head(&c.repo);
    let rec = json!({
        "schema": CERT_SCHEMA, "component": "protected_backend",
        "host_profile": "linux-microvm-protected", "qualification_profile": "linux-microvm-protected",
        "psv_spec_sha256": sha(&c.repo.join("governance/specs/v022-protected-suite-verdict.md")),
        "axon_sha": h, "micode_sha": "a".repeat(40), "fabric_revision": rev,
        GUEST[1].0: GUEST[1].2, GUEST[0].0: GUEST[0].2, GUEST[2].0: GUEST[2].2,
        "suite": {"id": "acceptance", "version": "v1", "entry": "accept.ax", "test": "t_ok", "digest": "4".repeat(64)},
        "candidate_tree_ref": format!("acf1:{}", "5".repeat(64)),
        "observer_key_id": c.observer.key_id(),
        "observation_sha256": sha(&c.repo.join(OBSERVATION)),
        "verifier_key_id": c.verifier.key_id(),
        "b263_qualification_sha256": sha(&c.repo.join(B263)),
        "evidence": evidence, "evidence_bundle_sha256": bundle_of(&c.repo, &evidence),
        "readiness_verifier_sha256": verifier_identity()["sha256"],
        "trust_preflight_sha256": pf,
        "certified_at": CERTIFIED_AT,
    });
    c.operator.write_signed(&c.record(), &rec);
    c.commit("certification (governance only)");
    let v = c.verdict();
    assert_eq!(v["status"], "PASS", "the positive control must pass: {v}");
    Some(c)
}

/// The bundle digest over `evidence`'s files, in order.
pub fn bundle_of(repo: &Path, evidence: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let cat: String = evidence.iter().map(|e| sha(&repo.join(e))).collect();
    format!("{:x}", Sha256::digest(cat.as_bytes()))
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
