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
    /// The key in the operator's VERIFIER root the record names: the host
    /// signer that attests the run's receipt (its PKCS#8 signs attestations).
    pub verifier: Issuer,
    pub verifier_pkcs8: Vec<u8>,
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

/// The run's documents in the certified evidence (C9 round 4, A88/A89): the
/// request, and Fabric's `axon-fabric submit` output (the receipt, its
/// attestation by the verifier key, the `axon-psv-evidence/2` bundle).
pub const REQUEST: &str = "governance/proofs/v022-protected/request.json";
pub const RUN: &str = "governance/proofs/v022-protected/run.json";
/// The registered check executable, and the host signer's issuer ref.
pub const REGISTERED: &str = "axon-test-protected";
pub const FABRIC_ISSUER: &str = "fabric:protected-host";

/// The suite tree the launch ran (its version IS its tree digest) and the
/// candidate tree.
pub fn suite_tree() -> String {
    format!("acf1:{}", "4".repeat(64))
}
pub fn candidate_tree() -> String {
    format!("acf1:{}", "5".repeat(64))
}

/// The record's `suite`, exactly as the launch ran it.
pub fn certified_suite() -> Value {
    json!({"id": "acceptance", "version": suite_tree(), "entry": "accept.ax",
           "test": "t_ok", "digest": suite_tree()})
}

/// The launch manifest Fabric built for the certified run, under the B263
/// record whose sha256 is `qualification`.
pub fn manifest(qualification: &str) -> axon_psv::LaunchManifest {
    use axon_psv::*;
    let h = |c: &str| c.repeat(64);
    LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op-1".into(),
        task_id: "task-1".into(),
        trial_id: "trial-1".into(),
        attempt_id: "attempt-1".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: h("c"),
        qualification_sha256: qualification.into(),
        host_config_sha256: h("a"),
        launcher_sha256: h("9"),
        firecracker_sha256: FC_SHA.into(),
        profile_manifest_sha256: h("7"),
        guest: GuestDigests {
            kernel_sha256: GUEST[0].2.into(),
            rootfs_sha256: GUEST[1].2.into(),
            axon_sha256: GUEST[2].2.into(),
            init_sha256: h("b"),
        },
        policy_sha256: h("e"),
        suite: SuiteRef {
            id: "acceptance".into(),
            version: suite_tree(),
            entry: "accept.ax".into(),
            test: "t_ok".into(),
            tree_digest: suite_tree(),
            registry_sha256: h("d"),
        },
        candidate: CandidateRef {
            workspace_version: candidate_tree(),
            tree_digest: candidate_tree(),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "0".repeat(32),
        authority: AuthorityRef {
            epoch: 1,
            tenant_id: "tenant-1".into(),
            task_family: "family-1".into(),
        },
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    }
}

/// The observer's observation of the launch `m` (as `observer`).
pub fn observation_of(observer: &Issuer, m: &axon_psv::LaunchManifest) -> Value {
    json!({
        "schema": "axon-preflight-observation/1",
        "observer_key_id": observer.key_id(),
        "nonce": m.observation_nonce, "epoch": m.authority.epoch, "observed_at": OBSERVED_AT,
        "host_profile": m.backend_profile, "fabric_revision": m.fabric_revision,
        "firecracker_sha256": m.firecracker_sha256, "launcher_sha256": m.launcher_sha256,
        "host_config_sha256": m.host_config_sha256,
        "guest": {"kernel_sha256": m.guest.kernel_sha256, "rootfs_sha256": m.guest.rootfs_sha256,
                  "axon_sha256": m.guest.axon_sha256, "init_sha256": m.guest.init_sha256},
        "verifier_sha256": m.verifier_sha256,
        "suite_registry_sha256": m.suite.registry_sha256,
        "policy_sha256": m.policy_sha256,
        "intended_launch_manifest_sha256": m.digest(),
    })
}

/// One edit to each run document, applied before it is signed (the
/// documents that follow it are derived from the edited one).
pub type EditManifest<'a> = Box<dyn FnOnce(&mut axon_psv::LaunchManifest) + 'a>;
pub type EditValue<'a> = Box<dyn FnOnce(&mut Value) + 'a>;

/// Write the certified run's documents into `repo`, as Fabric and the
/// observer produce them for a launch under the B263 record at [`B263`]:
/// the observation (signed by `observer`), the request, and the run output
/// whose receipt names this launch and is attested by the verifier key
/// (`verifier_pkcs8`). `edit_m`, `edit_o` and `edit_rc` each apply one
/// defect before signing.
pub fn write_launch(
    repo: &Path,
    observer: &Issuer,
    verifier_pkcs8: &[u8],
    edit_m: EditManifest,
    edit_o: EditValue,
    edit_rc: EditValue,
) {
    use axon_psv::*;
    let mut m = manifest(&sha(&repo.join(B263)));
    edit_m(&mut m);
    let m_bytes = m.bytes();
    let m_sha = sha256_hex(&m_bytes);
    let mut o = observation_of(observer, &m);
    edit_o(&mut o);
    write_signed_for(
        observer,
        TrustAuthority::Observer,
        &repo.join(OBSERVATION),
        &o,
    );
    let o_bytes = std::fs::read(repo.join(OBSERVATION)).unwrap();
    let v = GuestVerdict {
        schema: GUEST_VERDICT_SCHEMA.into(),
        launch_manifest_sha256: m_sha.clone(),
        inputs: InputCheck {
            candidate_tree_digest: m.candidate.tree_digest.clone(),
            suite_tree_digest: m.suite.tree_digest.clone(),
            matches: true,
        },
        test: m.suite.test.clone(),
        status: GuestStatus::Passed,
        refusal: None,
        exit_code: Some(0),
        report: None,
        runner: Runner {
            runner_sha256: "b".repeat(64),
            axon_sha256: m.guest.axon_sha256.clone(),
        },
        stdout_sha256: None,
        policy_sha256: m.policy_sha256.clone(),
    };
    let v_bytes = v.bytes();
    let req = json!({
        "schema": "acf-compute-request/1",
        "operation_id": m.operation_id, "task_id": m.task_id,
        "trial_id": m.trial_id, "attempt_id": m.attempt_id,
        "principal_ref": "principal:acceptance-check", "grant_ref": "grant:check", "approval_ref": null,
        "job_kind": "registered_check", "registered_executable_ref": REGISTERED,
        "executable_digest": axon_loop_contracts::protected_evidence::executable_digest(
            REGISTERED, &m.guest.axon_sha256),
        "workspace_version_ref": m.candidate.workspace_version, "semantic_state_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "required": {"engine": "axon_interpreter", "hardware_isolation": true, "os": "none",
                     "architecture": "x86_64", "network_mode": "deny", "checkpoint_kind": "none"},
        "limits": {"cpu_millicores": 1000, "memory_bytes": 268435456, "disk_bytes": 268435456,
                   "wall_time_ms": 60000, "output_bytes": 1048576, "max_cost_micro": 100,
                   "currency_code": "USD", "price_schedule_ref": "unpriced:test"},
        "argv": [format!("check:{}", m.suite.id), m.suite.test], "result_schema_ref": "cortex-check-report/1",
    });
    let mut rc = json!({
        "schema": "acf-execution-receipt/1",
        "operation_id": m.operation_id, "task_id": m.task_id,
        "trial_id": m.trial_id, "attempt_id": m.attempt_id,
        "execution_id": "exec-1", "backend_profile_ref": PROTECTED_PROFILE,
        "input_workspace_ref": m.candidate.workspace_version, "output_workspace_ref": null,
        "policy_digest": format!("acf1:{}", "c".repeat(64)),
        "status": "completed", "process_exit_code": 0,
        "verification": "passed", "matched_checks": 1,
        "evidence_source": "supervisor_observed",
        "evidence_refs": [
            "evidence-class:protected",
            format!("launch-manifest-sha256:{m_sha}"),
            format!("preflight-observation-sha256:{}", sha256_hex(&o_bytes)),
            format!("guest-verdict-sha256:{}", sha256_hex(&v_bytes)),
            format!("guest-kernel-sha256:{}", m.guest.kernel_sha256),
            format!("guest-rootfs-sha256:{}", m.guest.rootfs_sha256),
            format!("guest-axon-sha256:{}", m.guest.axon_sha256),
            format!("guest-init-sha256:{}", m.guest.init_sha256),
            format!("qualification-sha256:{}", m.qualification_sha256),
            format!("check-suite:{}@{}#{}", m.suite.id, m.suite.version, m.suite.entry),
        ],
        "usage_state": "unknown", "cost_micro": null, "unresolved_liability_micro": 100,
    });
    edit_rc(&mut rc);
    let parsed_req: axon_loop_contracts::ComputeRequest =
        axon_loop_contracts::parse(&req.to_string()).unwrap();
    let parsed_rc: axon_loop_contracts::ExecutionReceipt =
        axon_loop_contracts::parse(&rc.to_string()).unwrap();
    let att = axon_loop_contracts::attestation::sign(
        verifier_pkcs8,
        &axon_loop_contracts::OpaqueRef::new(FABRIC_ISSUER).unwrap(),
        &parsed_req,
        &parsed_rc,
        parse_utc(OBSERVED_AT).unwrap() as u64 * 1000 + 60_000,
    )
    .unwrap();
    let run = json!({
        "schema": "axon-fabric-submit/1",
        "receipt": rc, "check_report": null, "replayed": false,
        "backend": PROTECTED_PROFILE, "reason": null,
        "receipt_attestation": att, "attestation_withheld": null,
        "psv_evidence": {
            "schema": "axon-psv-evidence/2",
            "launch_manifest": String::from_utf8(m_bytes).unwrap(),
            "observation": String::from_utf8(o_bytes.clone()).unwrap(),
            "observation_signature": std::fs::read_to_string(sig_of(&repo.join(OBSERVATION))).unwrap(),
            "guest_verdict": String::from_utf8(v_bytes).unwrap(),
        },
    });
    write(
        &repo.join(REQUEST),
        &serde_json::to_string_pretty(&req).unwrap(),
    );
    write(
        &repo.join(RUN),
        &serde_json::to_string_pretty(&run).unwrap(),
    );
}

/// No edit.
pub fn keep<T>() -> Box<dyn FnOnce(&mut T)> {
    Box::new(|_| {})
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
    let (verifier_pkcs8, _) = axon_loop_contracts::attestation::generate().unwrap();
    let verifier = Issuer(ring::signature::Ed25519KeyPair::from_pkcs8(&verifier_pkcs8).unwrap());
    write_signed_for(
        &operator,
        TrustAuthority::Qualification,
        &repo.join(B263),
        &b263_record(&operator),
    );
    write_launch(&repo, &observer, &verifier_pkcs8, keep(), keep(), keep());
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
        verifier_pkcs8,
        _d: d,
    };
    let ev = "governance/proofs/v022-protected/run-evidence.md";
    let evidence = [ev, PREFLIGHT, OBSERVATION, B263, REQUEST, RUN];
    let pf = sha(&c.repo.join(PREFLIGHT));
    let h = head(&c.repo);
    let rec = json!({
        "schema": CERT_SCHEMA, "component": "protected_backend",
        "host_profile": "linux-microvm-protected", "qualification_profile": "linux-microvm-protected",
        "psv_spec_sha256": sha(&c.repo.join("governance/specs/v022-protected-suite-verdict.md")),
        "axon_sha": h, "micode_sha": "a".repeat(40), "fabric_revision": rev,
        GUEST[1].0: GUEST[1].2, GUEST[0].0: GUEST[0].2, GUEST[2].0: GUEST[2].2,
        "suite": certified_suite(),
        "candidate_tree_ref": candidate_tree(),
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

impl Certified {
    /// The genuine observation, re-attributed to `observer` and fabric
    /// revision `rev` (for attacks on the observation itself).
    pub fn observed(&self, observer: &Issuer, rev: &str) -> Value {
        let mut o: Value =
            serde_json::from_slice(&std::fs::read(self.repo.join(OBSERVATION)).unwrap()).unwrap();
        o["observer_key_id"] = json!(observer.key_id());
        o["fabric_revision"] = json!(rev);
        o
    }
}

/// The run is launched again under the B263 record now at [`B263`], with
/// one defect per edit, and the OPERATOR re-signs a record binding the new
/// observation, the B263 record and the evidence bundle: everything genuine
/// except the edits.
pub fn relaunch(c: &Certified, edit_m: EditManifest, edit_o: EditValue, edit_rc: EditValue) {
    write_launch(
        &c.repo,
        &c.observer,
        &c.verifier_pkcs8,
        edit_m,
        edit_o,
        edit_rc,
    );
    rebundle(c, |r| {
        r["observation_sha256"] = json!(sha(&c.repo.join(OBSERVATION)));
        r["b263_qualification_sha256"] = json!(sha(&c.repo.join(B263)));
    });
}

/// The operator re-signs the record with `edit` applied and the evidence
/// bundle digest recomputed over the evidence files as they are now.
pub fn rebundle(c: &Certified, edit: impl FnOnce(&mut Value)) {
    let rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    let ev: Vec<String> = rec["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    let evr: Vec<&str> = ev.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &evr);
    resign(c, &c.operator, |r| {
        edit(r);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
}
