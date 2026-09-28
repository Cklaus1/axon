//! The protected suite-verdict protocol's shared formats and derivations
//! (`governance/specs/v022-psv-protocol.md` §3–§5). ONE implementation for
//! both sides:
//! * Fabric builds the launch manifest, keeps the per-attempt secret, and
//!   re-derives the completion key from its OWN manifest;
//! * the guest verdict runner verifies the manifest it was handed, re-digests
//!   its inputs, and derives the same key for the interpreter.
//!
//! Dependencies are minimal (sha2, serde, serde_json and the workspace
//! recipe), so the static musl guest can link it.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::path::Path;

pub use axon_workspace_recipe::{sha256_hex, Quota};

pub mod runner;

pub const LAUNCH_MANIFEST_SCHEMA: &str = "axon-launch-manifest/1";
pub const GUEST_VERDICT_SCHEMA: &str = "axon-guest-verdict/1";
pub const COMPLETION_SCHEME: &str = "axon-guest-completion/1";
pub const PROTECTED_PROFILE: &str = "linux-microvm-protected";

// ── §3 launch manifest ──────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestDigests {
    pub kernel_sha256: String,
    pub rootfs_sha256: String,
    pub axon_sha256: String,
    pub init_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SuiteRef {
    pub id: String,
    pub version: String,
    pub entry: String,
    pub test: String,
    pub tree_digest: String,
    pub registry_sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateRef {
    pub workspace_version: String,
    pub tree_digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Completion {
    pub scheme: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Limits {
    pub wall_time_ms: u64,
    pub output_bytes: u64,
}

/// `axon-launch-manifest/1`. Built by Fabric per attempt; carries NO secret.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchManifest {
    pub schema: String,
    pub operation_id: String,
    pub task_id: String,
    pub trial_id: String,
    pub attempt_id: String,
    pub backend_profile: String,
    pub fabric_revision: String,
    pub qualification_sha256: String,
    pub host_config_sha256: String,
    pub launcher_sha256: String,
    pub firecracker_sha256: String,
    pub profile_manifest_sha256: String,
    pub guest: GuestDigests,
    pub policy_sha256: String,
    pub suite: SuiteRef,
    pub candidate: CandidateRef,
    pub completion: Completion,
    pub observation_nonce: String,
    pub limits: Limits,
}

/// Canonical JSON: object keys sorted by code point at EVERY level, no
/// whitespace, strings escaped by serde_json. Sorted HERE, never inherited from
/// a map type, so `preserve_order` anywhere in a build cannot change a digest.
///
/// Mutation note: deleting the sort is an EQUIVALENT mutant in today's build
/// (`preserve_order` is enabled nowhere, so the map already iterates sorted).
/// It is classified equivalent, not counted as killed; the sort is what keeps
/// that true if the feature is ever unified in.
pub fn canonical_json(v: &Value) -> Vec<u8> {
    fn write(v: &Value, out: &mut Vec<u8>) {
        match v {
            Value::Object(m) => {
                let mut keys: Vec<&String> = m.keys().collect();
                keys.sort();
                out.push(b'{');
                for (i, k) in keys.into_iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    out.extend(serde_json::to_vec(k).expect("string"));
                    out.push(b':');
                    write(&m[k], out);
                }
                out.push(b'}');
            }
            Value::Array(a) => {
                out.push(b'[');
                for (i, x) in a.iter().enumerate() {
                    if i > 0 {
                        out.push(b',');
                    }
                    write(x, out);
                }
                out.push(b']');
            }
            other => out.extend(serde_json::to_vec(other).expect("scalar")),
        }
    }
    let mut out = Vec::new();
    write(v, &mut out);
    out
}

impl LaunchManifest {
    /// The exact bytes Fabric writes to the job device; the manifest's digest
    /// is `sha256` of these bytes.
    pub fn bytes(&self) -> Vec<u8> {
        canonical_json(&serde_json::to_value(self).expect("serializable"))
    }
    pub fn digest(&self) -> String {
        sha256_hex(&self.bytes())
    }
    /// The guest's check: `bytes` are exactly the manifest Fabric named by
    /// `expected_sha256` (on the kernel command line), canonical, and a
    /// well-formed `axon-launch-manifest/1` for the protected profile.
    pub fn verify(bytes: &[u8], expected_sha256: &str) -> Result<LaunchManifest, String> {
        let got = sha256_hex(bytes);
        if got != expected_sha256 {
            return Err(format!(
                "launch manifest sha256 {got} is not the {expected_sha256} Fabric named"
            ));
        }
        let m: LaunchManifest = serde_json::from_slice(bytes)
            .map_err(|e| format!("launch manifest is not {LAUNCH_MANIFEST_SCHEMA}: {e}"))?;
        if m.schema != LAUNCH_MANIFEST_SCHEMA {
            return Err(format!(
                "launch manifest schema is not {LAUNCH_MANIFEST_SCHEMA}"
            ));
        }
        if m.backend_profile != PROTECTED_PROFILE {
            return Err(format!(
                "launch manifest is for {}, not {PROTECTED_PROFILE}",
                m.backend_profile
            ));
        }
        if m.completion.scheme != COMPLETION_SCHEME {
            return Err(format!(
                "completion scheme {} is not {COMPLETION_SCHEME}",
                m.completion.scheme
            ));
        }
        if m.bytes() != bytes {
            return Err("launch manifest bytes are not canonical".into());
        }
        Ok(m)
    }
}

// ── §4 input check ──────────────────────────────────────────────────────────

/// What the guest found on its two read-only inputs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InputCheck {
    pub candidate_tree_digest: String,
    pub suite_tree_digest: String,
    #[serde(rename = "match")]
    pub matches: bool,
}

/// Re-digest both inputs with the shared recipe and compare each with the
/// manifest. `Err` names the first input that does not match (or could not be
/// read); nothing may execute after an `Err`.
pub fn check_inputs(
    m: &LaunchManifest,
    candidate_root: &Path,
    suite_root: &Path,
    quota: &Quota,
) -> Result<InputCheck, (InputCheck, String)> {
    let digest = |what: &str, root: &Path| {
        axon_workspace_recipe::tree_version_ref(root, quota)
            .map_err(|e| format!("{what} input at {}: {e}", root.display()))
    };
    let (c, s) = (
        digest("candidate", candidate_root),
        digest("suite", suite_root),
    );
    let found = InputCheck {
        candidate_tree_digest: c.clone().unwrap_or_default(),
        suite_tree_digest: s.clone().unwrap_or_default(),
        matches: false,
    };
    let c = c.map_err(|e| (found.clone(), e))?;
    let s = s.map_err(|e| (found.clone(), e))?;
    if c != m.candidate.tree_digest {
        return Err((
            found,
            format!(
                "candidate tree is {c}, not the {} the launch manifest names",
                m.candidate.tree_digest
            ),
        ));
    }
    if s != m.suite.tree_digest {
        return Err((
            found,
            format!(
                "suite tree is {s}, not the {} the launch manifest names",
                m.suite.tree_digest
            ),
        ));
    }
    Ok(InputCheck {
        matches: true,
        ..found
    })
}

// ── §4 completion key ───────────────────────────────────────────────────────

/// HMAC-SHA256 (RFC 2104).
pub fn hmac_sha256(key: &[u8], msg: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut k = [0u8; 64];
    if key.len() > 64 {
        k[..32].copy_from_slice(&Sha256::digest(key));
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha256::new();
    inner.update(k.map(|b| b ^ 0x36));
    inner.update(msg);
    let mut outer = Sha256::new();
    outer.update(k.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    outer.finalize().into()
}

/// The binding `B` of §4: every identity a completion proof must not travel
/// between.
pub fn completion_binding(m: &LaunchManifest, launch_manifest_digest: &str) -> Vec<u8> {
    canonical_json(&serde_json::json!({
        "scheme": COMPLETION_SCHEME,
        "operation_id": m.operation_id,
        "trial_id": m.trial_id,
        "attempt_id": m.attempt_id,
        "suite_id": m.suite.id,
        "suite_version": m.suite.version,
        "entry": m.suite.entry,
        "test": m.suite.test,
        "candidate_tree_digest": m.candidate.tree_digest,
        "suite_tree_digest": m.suite.tree_digest,
        "launch_manifest_digest": launch_manifest_digest,
    }))
}

/// `K = HMAC-SHA256(S, "axon-guest-completion/1\n" || hex(sha256(B)))`. The
/// guest hands `K` to `axon test --completion-key-stdin`; Fabric derives it
/// from its own secret and its OWN manifest, never from what the guest reports.
pub fn completion_key(secret: &[u8; 32], m: &LaunchManifest) -> [u8; 32] {
    let b = completion_binding(m, &m.digest());
    let mut msg = format!("{COMPLETION_SCHEME}\n").into_bytes();
    msg.extend(sha256_hex(&b).into_bytes());
    hmac_sha256(secret, &msg)
}

// ── §5 guest verdict ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GuestStatus {
    Passed,
    Failed,
    Unknown,
    Refused,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestReport {
    pub passed: Vec<String>,
    pub failed: Vec<String>,
    /// `(test, completion token)` as `axon test --completion-key-stdin` emits.
    pub completion: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runner {
    pub init_sha256: String,
    pub axon_sha256: String,
}

/// `axon-guest-verdict/1`. The guest's `status` is a CLAIM; Fabric derives the
/// verdict (§5).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GuestVerdict {
    pub schema: String,
    pub launch_manifest_sha256: String,
    pub inputs: InputCheck,
    pub test: String,
    pub status: GuestStatus,
    pub refusal: Option<String>,
    pub exit_code: Option<i32>,
    pub report: Option<GuestReport>,
    pub runner: Runner,
    pub stdout_sha256: Option<String>,
}

impl GuestVerdict {
    pub fn bytes(&self) -> Vec<u8> {
        canonical_json(&serde_json::to_value(self).expect("serializable"))
    }
}

// ── §7 preflight observation ────────────────────────────────────────────────

pub const PREFLIGHT_OBSERVATION_SCHEMA: &str = "axon-preflight-observation/1";

/// `axon-preflight-observation/1` (ADR-002 + the O1 digests), signed by the
/// OBSERVER with an observer-domain `axon-evidence-signature/2`. Fabric
/// verifies and consumes one; it holds no observer key and cannot mint one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreflightObservation {
    pub schema: String,
    pub observer_key_id: String,
    pub nonce: String,
    pub epoch: u64,
    pub observed_at: String,
    pub host_profile: String,
    pub fabric_revision: String,
    pub firecracker_sha256: String,
    pub launcher_sha256: String,
    pub host_config_sha256: String,
    pub guest: GuestDigests,
    pub suite_registry_sha256: String,
    pub policy_sha256: String,
    pub intended_launch_manifest_sha256: String,
}

impl PreflightObservation {
    /// Every observed fact must be the one the launch manifest names, and the
    /// observation must be of THIS manifest. The first difference is named.
    pub fn joins(&self, m: &LaunchManifest, manifest_digest: &str) -> Result<(), String> {
        if self.schema != PREFLIGHT_OBSERVATION_SCHEMA {
            return Err(format!(
                "observation schema is not {PREFLIGHT_OBSERVATION_SCHEMA}"
            ));
        }
        let pairs: [(&str, &str, &str); 12] = [
            (
                "intended_launch_manifest_sha256",
                &self.intended_launch_manifest_sha256,
                manifest_digest,
            ),
            ("nonce", &self.nonce, &m.observation_nonce),
            ("host_profile", &self.host_profile, &m.backend_profile),
            ("fabric_revision", &self.fabric_revision, &m.fabric_revision),
            (
                "firecracker_sha256",
                &self.firecracker_sha256,
                &m.firecracker_sha256,
            ),
            ("launcher_sha256", &self.launcher_sha256, &m.launcher_sha256),
            (
                "host_config_sha256",
                &self.host_config_sha256,
                &m.host_config_sha256,
            ),
            (
                "guest.kernel_sha256",
                &self.guest.kernel_sha256,
                &m.guest.kernel_sha256,
            ),
            (
                "guest.rootfs_sha256",
                &self.guest.rootfs_sha256,
                &m.guest.rootfs_sha256,
            ),
            (
                "guest.axon_sha256",
                &self.guest.axon_sha256,
                &m.guest.axon_sha256,
            ),
            (
                "suite_registry_sha256",
                &self.suite_registry_sha256,
                &m.suite.registry_sha256,
            ),
            ("policy_sha256", &self.policy_sha256, &m.policy_sha256),
        ];
        for (field, observed, launch) in pairs {
            if observed != launch {
                return Err(format!(
                    "observation {field} is {observed}, but the launch names {launch}"
                ));
            }
        }
        if self.guest.init_sha256 != m.guest.init_sha256 {
            return Err(format!(
                "observation guest.init_sha256 is {}, but the launch names {}",
                self.guest.init_sha256, m.guest.init_sha256
            ));
        }
        Ok(())
    }
}
