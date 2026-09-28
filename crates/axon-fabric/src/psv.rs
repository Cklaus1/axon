//! M2 — Fabric's side of the protected suite-verdict protocol
//! (`governance/specs/v022-psv-protocol.md` §3, §5, §6).
//!
//! [`prepare`] builds the launch manifest from FABRIC's own pins:
//! * the B263 qualification;
//! * the profile manifest's artifact digests;
//! * the O1 host config and suite registry;
//! * the guest policy;
//! * the request's identities.
//!
//! It also draws a fresh 32-byte secret for this attempt. Nothing in the
//! manifest comes from the guest, the candidate or MiCode.
//!
//! [`derive`] turns what came back into a verdict. The guest's `status` is a
//! claim and is never read as the answer:
//! * the verdict must name THIS manifest, and inputs that matched it;
//! * the test output must hash to what the verdict names;
//! * the result is decided by the CERTIFIED parser (`parse_axon_test_json` +
//!   `CheckReport::verdict`) under the exact test name;
//! * a pass needs the completion token under the key Fabric derives from its
//!   OWN secret and manifest, and exit 0.
//!
//! The evidence class travels INSIDE the receipt (and so inside what an
//! attestation signs):
//! * `protected` — needs a verified preflight observation (M3). Until M3
//!   supplies one, no verdict is protected;
//! * `guest-unobserved` — a verdict produced on the guest path, not observed;
//! * `development` — a local run, which is never protected.

use axon_cortex::runner::{completion_token, parse_axon_test_json, CheckVerdict};
use axon_loop_contracts::{ComputeRequest, ReceiptVerification};
use axon_psv::{
    CandidateRef, Completion, GuestDigests, GuestStatus, GuestVerdict, LaunchManifest, Limits,
    SuiteRef, COMPLETION_SCHEME, LAUNCH_MANIFEST_SCHEMA, PROTECTED_PROFILE,
};
use std::path::{Path, PathBuf};

/// Where a verdict's evidence class is recorded in a receipt.
pub const EVIDENCE_CLASS_PREFIX: &str = "evidence-class:";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceClass {
    Protected,
    GuestUnobserved,
    Development,
}

impl EvidenceClass {
    pub fn as_str(self) -> &'static str {
        match self {
            EvidenceClass::Protected => "protected",
            EvidenceClass::GuestUnobserved => "guest-unobserved",
            EvidenceClass::Development => "development",
        }
    }
    pub fn evidence_ref(self) -> String {
        format!("{EVIDENCE_CLASS_PREFIX}{}", self.as_str())
    }
}

/// What O1 contributes to the manifest: the operator host config and the
/// registry that defined the suite. `None` off a protected host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostIdentity {
    pub config_sha256: String,
    pub suite_registry_sha256: String,
}

/// One attempt's launch: the manifest, its digest, and the secret that stays
/// with Fabric (and, on the job drive, with the trusted runner).
pub struct Launch {
    pub manifest: LaunchManifest,
    pub digest: String,
    secret: [u8; 32],
    pub job_dir: PathBuf,
    pub candidate_dir: PathBuf,
    pub suite_dir: PathBuf,
}

impl std::fmt::Debug for Launch {
    // The secret is never printed.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Launch")
            .field("digest", &self.digest)
            .field("job_dir", &self.job_dir)
            .finish_non_exhaustive()
    }
}

fn artifact(manifest: &serde_json::Value, name: &str) -> Result<String, String> {
    manifest["artifacts"][name]["sha256"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| format!("profile manifest pins no {name}"))
}

/// Everything [`prepare`] reads that is not the request.
pub struct PrepareInputs<'a> {
    pub qualification: &'a crate::backend::LinuxQualification,
    pub profile_manifest: &'a Path,
    pub host: Option<&'a HostIdentity>,
    pub policy_json: &'a str,
    pub suite_id: &'a str,
    pub suite_version: &'a str,
    pub entry: &'a str,
    pub test: &'a str,
    pub candidate_dir: &'a Path,
    pub suite_dir: &'a Path,
    /// A new, empty directory for the job drive's contents.
    pub job_dir: &'a Path,
    pub observation_nonce: &'a str,
}

/// Where [`private_inputs`] puts this operation's inputs.
pub fn private_inputs_dir(
    lx: &crate::backend::LinuxProfileConfig,
    req: &ComputeRequest,
) -> PathBuf {
    lx.out_root.join(format!(
        "{}.psv-inputs",
        crate::backend::jail_id(req.operation_id.as_str())
    ))
}

/// B3: materialize the candidate (the request's WorkspaceVersion) and the
/// suite (its registered version) from the content-addressed store — every
/// blob re-verified against its hash — into a NEW, Fabric-private (0700) dir
/// `<out_root>/<op>.psv-inputs/{candidate,check}`. Nothing is read from the
/// run dir under the caller's `--state`.
pub fn private_inputs(
    lx: &crate::backend::LinuxProfileConfig,
    state_dir: &Path,
    tenant: &axon_loop_contracts::TenantId,
    req: &ComputeRequest,
    suite_version: &axon_loop_contracts::Acf1Ref,
) -> Result<PathBuf, String> {
    use std::os::unix::fs::DirBuilderExt;
    let dir = private_inputs_dir(lx, req);
    std::fs::create_dir_all(&lx.out_root).map_err(|e| format!("out_root: {e}"))?;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("private inputs dir {}: {e}", dir.display()))?;
    let store = crate::workspace::WorkspaceStore::open(state_dir, tenant)
        .map_err(|e| format!("workspace store: {e}"))?;
    store
        .materialize(&req.workspace_version_ref, &dir.join("candidate"), true)
        .map_err(|e| format!("candidate {}: {e}", req.workspace_version_ref))?;
    store
        .materialize(suite_version, &dir.join("check"), true)
        .map_err(|e| format!("suite {suite_version}: {e}"))?;
    Ok(dir)
}

/// Build this attempt's launch manifest and secret, and write the job drive's
/// two files (`launch-manifest.json`, `completion-secret` 0400).
pub fn prepare(req: &ComputeRequest, i: &PrepareInputs<'_>) -> Result<Launch, String> {
    let q = i.qualification;
    let pm: serde_json::Value = serde_json::from_slice(
        &std::fs::read(i.profile_manifest).map_err(|e| format!("profile manifest: {e}"))?,
    )
    .map_err(|e| format!("profile manifest: {e}"))?;
    let quota = axon_psv::Quota::default();
    let tree = |what: &str, d: &Path| {
        axon_workspace_recipe::tree_version_ref(d, &quota).map_err(|e| format!("{what} tree: {e}"))
    };
    let absent = || "0".repeat(64);
    // B3: each tree IS the version the receipt will name — never merely a
    // digest of whatever bytes sit in a directory.
    let (cand_tree, suite_tree) = (
        tree("candidate", i.candidate_dir)?,
        tree("suite", i.suite_dir)?,
    );
    if cand_tree != req.workspace_version_ref.as_str() {
        return Err(format!(
            "candidate tree is {cand_tree}, not the requested {}",
            req.workspace_version_ref
        ));
    }
    if suite_tree != i.suite_version {
        return Err(format!(
            "suite tree is {suite_tree}, not the registered {}",
            i.suite_version
        ));
    }
    let manifest = LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: req.operation_id.as_str().into(),
        task_id: req.task_id.as_str().into(),
        trial_id: req.trial_id.as_str().into(),
        attempt_id: req.attempt_id.as_str().into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: env!("AXON_FABRIC_GIT_SHA").into(),
        verifier_sha256: crate::readiness::verifier_identity()["sha256"]
            .as_str()
            .unwrap_or("unknown")
            .to_string(),
        qualification_sha256: q.evidence_sha256.clone(),
        host_config_sha256: i
            .host
            .map(|h| h.config_sha256.clone())
            .unwrap_or_else(absent),
        launcher_sha256: q.launcher_sha256.clone(),
        firecracker_sha256: q.firecracker_sha256.clone(),
        profile_manifest_sha256: q.manifest_sha256.clone(),
        guest: GuestDigests {
            kernel_sha256: artifact(&pm, "vmlinux")?,
            rootfs_sha256: artifact(&pm, "rootfs.sqfs")?,
            axon_sha256: q.guest_axon_sha256.clone(),
            init_sha256: artifact(&pm, "axon-guest-init")?,
        },
        policy_sha256: axon_psv::sha256_hex(i.policy_json.as_bytes()),
        suite: SuiteRef {
            id: i.suite_id.into(),
            version: i.suite_version.into(),
            entry: i.entry.into(),
            test: i.test.into(),
            tree_digest: suite_tree,
            registry_sha256: i
                .host
                .map(|h| h.suite_registry_sha256.clone())
                .unwrap_or_else(absent),
        },
        candidate: CandidateRef {
            workspace_version: req.workspace_version_ref.as_str().into(),
            tree_digest: cand_tree,
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: i.observation_nonce.into(),
        limits: Limits {
            wall_time_ms: req.limits.wall_time_ms,
            output_bytes: req.limits.output_bytes,
        },
    };
    let mut secret = [0u8; 32];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut secret)
        .map_err(|_| "no system randomness for the completion secret".to_string())?;
    std::fs::create_dir(i.job_dir).map_err(|e| format!("job dir: {e}"))?;
    std::fs::write(i.job_dir.join("launch-manifest.json"), manifest.bytes())
        .map_err(|e| format!("job dir: {e}"))?;
    write_private(&i.job_dir.join("completion-secret"), &secret)?;
    Ok(Launch {
        digest: manifest.digest(),
        manifest,
        secret,
        job_dir: i.job_dir.to_path_buf(),
        candidate_dir: i.candidate_dir.to_path_buf(),
        suite_dir: i.suite_dir.to_path_buf(),
    })
}

fn write_private(p: &Path, bytes: &[u8]) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o400)
        .open(p)
        .and_then(|mut f| f.write_all(bytes))
        .map_err(|e| format!("completion secret: {e}"))
}

impl Launch {
    /// The key Fabric expects the guest's interpreter to have used, derived
    /// from Fabric's OWN secret and manifest.
    fn key(&self) -> [u8; 32] {
        axon_psv::completion_key(&self.secret, &self.manifest)
    }
    /// Remove the job drive's source files: the secret leaves the host
    /// filesystem once the launcher has built the read-only image from them.
    pub fn scrub(&self) {
        let _ = std::fs::remove_dir_all(&self.job_dir);
    }
    /// Remove the whole private inputs dir (candidate, suite, job,
    /// observation) once the verdict is derived.
    pub fn discard(&self) {
        if let Some(d) = self.job_dir.parent() {
            let _ = crate::workspace::remove_tree(d);
        }
    }
}

/// Fabric's verdict for one guest run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostVerdict {
    pub verification: ReceiptVerification,
    pub class: EvidenceClass,
    pub reason: Option<String>,
    /// Receipt evidence references (class, manifest, verdict, guest digests,
    /// suite).
    pub evidence: Vec<String>,
    /// The certified parser's report, when the output parsed.
    pub report: Option<serde_json::Value>,
}

/// A preflight observation Fabric has VERIFIED (M3). `derive` only asks whether
/// one exists for this launch; M3 defines and checks it.
pub struct VerifiedObservation {
    pub sha256: String,
    /// The exact observation bytes and their observer signature, carried into
    /// the `axon-psv-evidence/1` bundle the loop joins through (B2).
    pub bytes: Vec<u8>,
    pub signature: String,
}

/// The `axon-psv-evidence/1` bundle for a PROTECTED verdict: the exact launch
/// manifest, observation and observer signature the receipt's digests name
/// (`axon_loop_contracts::protected_evidence::check_bundle` verifies it).
pub fn evidence_bundle(launch: &Launch, o: &VerifiedObservation) -> serde_json::Value {
    serde_json::json!({
        "schema": axon_loop_contracts::protected_evidence::PSV_EVIDENCE_SCHEMA,
        "launch_manifest": String::from_utf8_lossy(&launch.manifest.bytes()),
        "observation": String::from_utf8_lossy(&o.bytes),
        "observation_signature": o.signature,
    })
}

/// Derive the verdict from the launcher's `out` directory (`out/out/…` is
/// what the launcher extracted from the returned drive, already bound to the
/// guest's serial digests).
pub fn derive(
    launch: &Launch,
    out_dir: &Path,
    observation: Option<&VerifiedObservation>,
) -> HostVerdict {
    let m = &launch.manifest;
    let mut evidence = vec![
        format!("launch-manifest-sha256:{}", launch.digest),
        format!("guest-kernel-sha256:{}", m.guest.kernel_sha256),
        format!("guest-rootfs-sha256:{}", m.guest.rootfs_sha256),
        format!("guest-axon-sha256:{}", m.guest.axon_sha256),
        format!("guest-init-sha256:{}", m.guest.init_sha256),
        format!("qualification-sha256:{}", m.qualification_sha256),
        // The CANONICAL suite reference, exactly as the local path records it:
        // operator pins and task acceptance compare it byte for byte
        // (`axon_loop::intake::check_pins`); the test is the request's argv.
        format!(
            "check-suite:{}@{}#{}",
            m.suite.id, m.suite.version, m.suite.entry
        ),
    ];
    let unknown = |why: String, evidence: Vec<String>, report| HostVerdict {
        verification: ReceiptVerification::Unknown,
        class: EvidenceClass::GuestUnobserved,
        reason: Some(why),
        evidence: with_class(evidence, EvidenceClass::GuestUnobserved),
        report,
    };
    let dir = out_dir.join("out");
    let vbytes = match std::fs::read(dir.join("verdict.json")) {
        Ok(b) => b,
        Err(e) => return unknown(format!("no guest verdict: {e}"), evidence, None),
    };
    evidence.push(format!(
        "guest-verdict-sha256:{}",
        axon_psv::sha256_hex(&vbytes)
    ));
    let v: GuestVerdict = match serde_json::from_slice(&vbytes) {
        Ok(v) => v,
        Err(e) => return unknown(format!("guest verdict is malformed: {e}"), evidence, None),
    };
    if v.launch_manifest_sha256 != launch.digest {
        return unknown(
            format!(
                "the guest verdict is for launch manifest {}, not this launch's {}",
                v.launch_manifest_sha256, launch.digest
            ),
            evidence,
            None,
        );
    }
    if v.status == GuestStatus::Refused {
        return unknown(
            format!(
                "the guest refused: {}",
                v.refusal.as_deref().unwrap_or("(no reason)")
            ),
            evidence,
            None,
        );
    }
    if !v.inputs.matches
        || v.inputs.candidate_tree_digest != m.candidate.tree_digest
        || v.inputs.suite_tree_digest != m.suite.tree_digest
        || v.test != m.suite.test
    {
        return unknown(
            "the guest verdict's inputs or test are not this launch's".into(),
            evidence,
            None,
        );
    }
    let out = match std::fs::read(dir.join("test-stdout")) {
        Ok(b) => b,
        Err(e) => return unknown(format!("no guest test output: {e}"), evidence, None),
    };
    if v.stdout_sha256.as_deref() != Some(axon_psv::sha256_hex(&out).as_str()) {
        return unknown(
            "the guest test output is not the bytes the verdict names".into(),
            evidence,
            None,
        );
    }
    let text = String::from_utf8_lossy(&out);
    let report = match parse_axon_test_json(&text, &m.suite.entry) {
        Ok(r) => r,
        Err(e) => {
            return unknown(
                format!("no verdict from the test output: {e}"),
                evidence,
                None,
            )
        }
    };
    let report_json = serde_json::json!({
        "passed": report.passed, "failed": report.failed, "total": report.total,
        "completion": report.completion, "exit_code": v.exit_code,
    });
    let test = &m.suite.test;
    let verification = match report.verdict(test) {
        CheckVerdict::Failed => ReceiptVerification::Failed,
        CheckVerdict::NotRun => {
            return unknown(
                format!("check `{test}` produced no verdict"),
                evidence,
                Some(report_json),
            )
        }
        CheckVerdict::Passed => {
            let want = completion_token(&launch.key(), test);
            if !report
                .completion
                .iter()
                .any(|(n, t)| n == test && *t == want)
            {
                return unknown(
                    format!(
                        "check `{test}` passed without completion evidence under this launch's \
                         key (none, or a token for another attempt, candidate, suite or test)"
                    ),
                    evidence,
                    Some(report_json),
                );
            }
            if v.exit_code != Some(0) {
                return unknown(
                    format!("check `{test}` passed but the run exited {:?}", v.exit_code),
                    evidence,
                    Some(report_json),
                );
            }
            ReceiptVerification::Passed
        }
    };
    let class = match observation {
        Some(o) => {
            evidence.push(format!("preflight-observation-sha256:{}", o.sha256));
            EvidenceClass::Protected
        }
        None => EvidenceClass::GuestUnobserved,
    };
    HostVerdict {
        verification,
        class,
        reason: None,
        evidence: with_class(evidence, class),
        report: Some(report_json),
    }
}

fn with_class(mut evidence: Vec<String>, c: EvidenceClass) -> Vec<String> {
    evidence.insert(0, c.evidence_ref());
    evidence
}
