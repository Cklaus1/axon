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
    /// sha256 of the Fabric (verifier) binary that built this manifest; the
    /// observer measures the INSTALLED one and must agree (§7).
    pub verifier_sha256: String,
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
        // B3: each tree digest IS the version the receipt names.
        if m.candidate.tree_digest != m.candidate.workspace_version {
            return Err(format!(
                "candidate tree_digest {} is not its workspace_version {}",
                m.candidate.tree_digest, m.candidate.workspace_version
            ));
        }
        if m.suite.tree_digest != m.suite.version {
            return Err(format!(
                "suite tree_digest {} is not its version {}",
                m.suite.tree_digest, m.suite.version
            ));
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
    // Stricter than the store's importer, on purpose (review wf_d725935a-7ed):
    // a guest input holds NO symlink and nothing the digest omits (.git,
    // .micode), so the bytes that execute are exactly the bytes digested.
    let digest = |what: &str, root: &Path| {
        let (entries, omitted) = axon_workspace_recipe::walk_tree(root, quota)
            .map_err(|e| format!("{what} input at {}: {e}", root.display()))?;
        if let Some(o) = omitted.first() {
            return Err(format!(
                "{what} input holds {}, which the digest omits: refused",
                o.path
            ));
        }
        if let Some(l) = entries
            .iter()
            .find(|e| e.kind == axon_workspace_recipe::EntryKind::Symlink)
        {
            return Err(format!(
                "{what} input holds a symlink ({}): refused",
                l.path
            ));
        }
        undigested_shape(root).map_err(|e| format!("{what} input holds {e}: refused"))?;
        Ok(axon_workspace_recipe::workspace_version_ref(
            &axon_workspace_recipe::workspace_manifest_bytes(
                &axon_workspace_recipe::manifest_entries(&entries),
            ),
        ))
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

/// The directory `mkfs.ext4` creates at the root of every input image. It is
/// accepted only there, and only EMPTY (then it is exactly what mkfs made).
const MKFS_LOST_FOUND: &str = "lost+found";

/// The permission bits (`& 0o7777`) a guest input entry may carry. The
/// WorkspaceVersion recipe records only a file's exec bit, so every other bit
/// must be one of the forms the host normalises to — the launcher's image
/// (`psv_image`: 0644/0755) or the store's read-only materialization
/// (0444/0555). All are world-readable, never group/other-writable, and carry
/// no setuid/setgid/sticky bit.
fn mode_is_normalised(dir: bool, exec: bool, mode: u32) -> bool {
    match (dir, exec) {
        (true, _) | (false, true) => mode == 0o755 || mode == 0o555,
        (false, false) => mode == 0o644 || mode == 0o444,
    }
}

/// PSV-2 (C9 certifying review): what the tree digest CANNOT see. The
/// digest is a cross-peer contract (MiCode `WORKSPACE_VERSION_RECIPE.md`,
/// computed by `micode-persist::workspace_version` too), so it is not
/// extended; instead a guest input holding anything it leaves out is
/// refused. That is:
/// * a directory holding no digested entry (an empty directory, or one
///   holding only such directories) — it is invisible to the digest, yet
///   `dir_list`/`file_exists` and module resolution can observe it;
/// * a mode other than the normalised forms ([`mode_is_normalised`]) on a
///   file or directory below the root — the digest records only the exec
///   bit, yet the unprivileged test child sees the rest.
///
/// * ANY extended attribute on ANY entry, the input root included (PSV-2,
///   C9 dev review). A POSIX ACL (`system.posix_acl_access`) keeps `st_mode`
///   a normalised 0644 while a named entry denies the test uid the file, so
///   it is invisible to the mode check as well as to the digest; the runner
///   (root) digests the bytes it can read while the child cannot. This is
///   refused at the source — the child's effective view — not by naming ACLs:
///   every attribute is refused, whatever its namespace.
///
/// The root's MODE is the input's mount point (the image's, not the
/// tree's), and an empty `lost+found` at the root is the one entry mkfs
/// adds. `Err` describes the first offender, in path order.
#[cfg(unix)]
fn undigested_shape(root: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    no_xattr(root, ".")?;
    // The number of digested entries at or below `dir`.
    fn walk(dir: &Path, prefix: &str) -> Result<usize, String> {
        let io = |e: std::io::Error| format!("an unreadable directory ({}): {e}", dir.display());
        let mut names: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
            .map_err(io)?
            .collect::<Result<_, _>>()
            .map_err(io)?;
        names.sort_by_key(|d| d.file_name());
        let mut digested = 0;
        for d in names {
            let name = d.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            let meta = std::fs::symlink_metadata(d.path()).map_err(io)?;
            no_xattr(&d.path(), &path)?;
            let mode = meta.permissions().mode() & 0o7777;
            if meta.is_dir() {
                let below = walk(&d.path(), &path)?;
                if below == 0 {
                    if prefix.is_empty() && name == MKFS_LOST_FOUND {
                        continue;
                    }
                    return Err(format!(
                        "an empty directory ({path}), which the digest cannot see"
                    ));
                }
                if !mode_is_normalised(true, false, mode) {
                    return Err(format!(
                        "{path} with mode {mode:04o}, which the digest does not record"
                    ));
                }
                digested += below;
            } else {
                // A symlink is refused by the caller; a special file by the
                // recipe's walker. Only a regular file reaches here.
                let exec = axon_workspace_recipe::is_exec(&meta);
                if meta.is_file() && !mode_is_normalised(false, exec, mode) {
                    return Err(format!(
                        "{path} with mode {mode:04o}, which the digest does not record"
                    ));
                }
                digested += 1;
            }
        }
        Ok(digested)
    }
    walk(root, "").map(|_| ())
}

/// `Err` names `shown` and its first extended attribute, if it carries any.
/// The link itself is inspected (`llistxattr`), never a target.
#[cfg(target_os = "linux")]
pub(crate) fn no_xattr(p: &Path, shown: &str) -> Result<(), String> {
    use std::os::unix::ffi::OsStrExt;
    let unreadable =
        |e: std::io::Error| format!("{shown} whose extended attributes cannot be listed ({e})");
    let c = std::ffi::CString::new(p.as_os_str().as_bytes())
        .map_err(|_| format!("{shown} whose path holds a NUL"))?;
    let mut buf: Vec<u8> = Vec::new();
    loop {
        // SAFETY: `c` is NUL-terminated; `buf` is valid for `buf.len()` bytes.
        let n = unsafe {
            libc::llistxattr(c.as_ptr(), buf.as_mut_ptr() as *mut libc::c_char, buf.len())
        };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            match e.raw_os_error() {
                // Grew between the size query and the read: ask again.
                Some(libc::ERANGE) => {
                    buf.clear();
                    continue;
                }
                // The filesystem holds no attributes at all.
                Some(libc::ENOTSUP) => return Ok(()),
                _ => return Err(unreadable(e)),
            }
        }
        let n = n as usize;
        if buf.is_empty() && n > 0 {
            buf = vec![0; n];
            continue;
        }
        let names = &buf[..n.min(buf.len())];
        return match names.split(|b| *b == 0).find(|s| !s.is_empty()) {
            None => Ok(()),
            Some(first) => Err(format!(
                "{shown} carrying extended attribute {}, which the digest cannot see",
                String::from_utf8_lossy(first)
            )),
        };
    }
}

/// Fail closed: where extended attributes cannot be listed, an input is
/// never accepted (the guest is Linux; nothing else runs the check).
#[cfg(all(unix, not(target_os = "linux")))]
pub(crate) fn no_xattr(_p: &Path, shown: &str) -> Result<(), String> {
    Err(format!(
        "{shown}, whose extended attributes this platform cannot list"
    ))
}

#[cfg(not(unix))]
fn undigested_shape(_root: &Path) -> Result<(), String> {
    Ok(())
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

/// The keyed OUTCOME token the interpreter issues under `K` for test `name`
/// (`axon test --completion-key-stdin`): for a completed pass,
/// `HMAC(K, "axon-test-completion/1\0" + name)`; for a failure the INTERPRETER
/// decided, `HMAC(K, "axon-test-failed/1\0" + name)`. Without `K`, candidate
/// code can print a result line but never its token, so it can neither forge a
/// pass nor write a failure over a genuine one (review wf_1bc28496-38e, PSV-4).
pub fn outcome_token(key: &[u8], name: &str, passed: bool) -> String {
    let mut msg = if passed {
        b"axon-test-completion/1\0".to_vec()
    } else {
        b"axon-test-failed/1\0".to_vec()
    };
    msg.extend_from_slice(name.as_bytes());
    hmac_sha256(key, &msg)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// The outcome of `test` in `axon test --json` output, decided ONLY by keyed
/// evidence: `Some(true)` for a pass, `Some(false)` for a failure, `None` when
/// no line, or more than one line, names the test, or when its token is not the
/// one `key` issues for that outcome. `None` is never a verdict.
pub fn keyed_outcome(stdout: &str, test: &str, key: &[u8]) -> Option<bool> {
    let mut lines = stdout.lines().filter_map(|l| {
        serde_json::from_str::<serde_json::Value>(l.trim())
            .ok()
            .filter(|v| v["name"].as_str() == Some(test))
    });
    let (Some(v), None) = (lines.next(), lines.next()) else {
        return None;
    };
    let passed = match v["status"].as_str() {
        Some("ok") => true,
        Some("failed") => false,
        _ => return None,
    };
    (v["completion"].as_str() == Some(outcome_token(key, test, passed).as_str())).then_some(passed)
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

/// What the guest runner says about ITSELF: sha256 of its own executable
/// (`/proc/self/exe`, i.e. `axon-psv-runner` — NOT `axon-guest-init`, which
/// is what the manifest's `guest.init_sha256` pins) and of the interpreter it
/// ran. INFORMATIONAL ONLY, never attribution: it is self-reported, and no
/// derivation compares it with anything (`psv::derive`, `check_bundle`). The
/// runner and interpreter are bound TRANSITIVELY, by the pinned
/// `rootfs_sha256` the launcher re-checks on the copy the VMM opens. (C9 dev
/// review: the field was named `init_sha256`, which read as the manifest's
/// init pin while naming a different binary.)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Runner {
    pub runner_sha256: String,
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
    pub verifier_sha256: String,
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
        let pairs: [(&str, &str, &str); 13] = [
            ("verifier_sha256", &self.verifier_sha256, &m.verifier_sha256),
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

#[cfg(all(test, target_os = "linux"))]
mod xattr_tests {
    use super::*;

    /// C9 round 2 (harness): where an entry's extended attributes cannot be
    /// LISTED, it is never read as carrying none. `llistxattr` fails on an
    /// entry that vanished between the directory read and the listing (or
    /// one it cannot reach); only `ENOTSUP` (a filesystem with no attributes
    /// at all) means "none". Control: a plain file lists none.
    #[test]
    fn an_entry_whose_attributes_cannot_be_listed_is_never_read_as_clean() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("f.ax");
        std::fs::write(&f, "x").unwrap();
        no_xattr(&f, "f.ax").expect("control: a plain file carries no attribute");
        let gone = d.path().join("vanished.ax");
        let got = no_xattr(&gone, "vanished.ax");
        assert!(
            got.is_err(),
            "ATTACK: an entry whose extended attributes could not be listed was read as \
             carrying none: {got:?}"
        );
    }
}
