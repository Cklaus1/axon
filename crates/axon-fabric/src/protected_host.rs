//! O1 — the protected host's configuration is the OPERATOR's
//! (`governance/specs/v022-psv-protocol.md` §2).
//!
//! Everything that defines a protected run is resolved from ONE file,
//! [`PROTECTED_HOST_CONFIG`], operator-owned from `/` down:
//! * the launcher (pinned by sha256);
//! * the profile manifest (pinned);
//! * the guest artifacts;
//! * the B263 qualification record and its age limit;
//! * the operator suite registry (pinned);
//! * the attestation signer;
//! * the output root.
//!
//! The caller NAMES a registered suite. It never points at a launcher, a
//! manifest, a registry or a key, and `axon-fabric submit` refuses every flag
//! that would.
//!
//! A test configuration exists only behind the `test-trust-root` feature
//! ([`ProtectedHost::for_test`]).

use crate::backend::{
    sha256_file, sha256_hex, LinuxProfileConfig, QualificationTrust, DEFAULT_EVIDENCE_MAX_AGE_S,
};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The one place a protected host's configuration lives.
pub const PROTECTED_HOST_CONFIG: &str = "/etc/axon/protected-host.json";
pub const PROTECTED_HOST_SCHEMA: &str = "axon-protected-host/1";

/// Every `--linux-*` / registry flag `submit` refuses when it would configure
/// the protected profile (A21). The operator's file is the only source.
pub const REFUSED_CALLER_FLAGS: [&str; 9] = [
    "--linux-trusted-issuers",
    "--linux-launcher",
    "--linux-manifest",
    "--linux-artifacts",
    "--linux-evidence",
    "--linux-evidence-sig",
    "--linux-waivers",
    "--linux-out-root",
    "--linux-evidence-max-age-s",
];

/// The attestation signer named by the host config (never by a registry).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignerSpec {
    pub issuer_ref: String,
    pub public_key: String,
    pub key_path: PathBuf,
}

/// A validated `axon-protected-host/1`.
#[derive(Debug, Clone)]
pub struct ProtectedHost {
    /// sha256 of the exact config bytes (bound into the launch manifest).
    pub config_sha256: String,
    pub linux: LinuxProfileConfig,
    pub suite_registry: PathBuf,
    pub suite_registry_sha256: String,
    pub signer: SignerSpec,
}

const KEYS: [&str; 8] = [
    "artifacts_dir",
    "launcher",
    "out_root",
    "profile_manifest",
    "qualification",
    "schema",
    "signer",
    "suite_registry",
];

impl ProtectedHost {
    /// The production configuration: [`PROTECTED_HOST_CONFIG`], every pinned
    /// path operator-owned. `Ok(None)` when the file does not exist — this is
    /// not a protected host, and the profile is simply not configured.
    #[cfg(unix)]
    pub fn operator() -> Result<Option<Self>, String> {
        let p = Path::new(PROTECTED_HOST_CONFIG);
        if std::fs::symlink_metadata(p).is_err() {
            return Ok(None);
        }
        Self::load(p, Some(Path::new("/")), QualificationTrust::operator()).map(Some)
    }

    /// TESTS ONLY. With `owned_below`, operator ownership is checked from that
    /// base down (a temp dir's ancestors are not root-owned); without it,
    /// ownership is not checked at all and only the pins and schema are. The
    /// qualification trust is the test's own, never the operator root.
    #[cfg(all(unix, any(test, feature = "test-trust-root")))]
    pub fn for_test(
        config: &Path,
        owned_below: Option<&Path>,
        trust: QualificationTrust,
    ) -> Result<Self, String> {
        Self::load(config, owned_below, trust)
    }

    #[cfg(unix)]
    fn load(
        config: &Path,
        owned_below: Option<&Path>,
        mut trust: QualificationTrust,
    ) -> Result<Self, String> {
        use crate::backend::check_owned_chain;
        let owned = |p: &Path, entries: bool| -> Result<(), String> {
            match owned_below {
                Some(base) => check_owned_chain(base, p, entries),
                None => Ok(()),
            }
        };
        let bad = |why: String| format!("{}: {why}", config.display());
        owned(config, false).map_err(bad)?;
        let bytes = std::fs::read(config).map_err(|e| bad(e.to_string()))?;
        let v: Value = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
        if v["schema"] != PROTECTED_HOST_SCHEMA {
            return Err(bad(format!("schema is not {PROTECTED_HOST_SCHEMA}")));
        }
        let mut keys: Vec<&str> = v
            .as_object()
            .map(|o| o.keys().map(String::as_str).collect())
            .unwrap_or_default();
        keys.sort_unstable();
        if keys != KEYS {
            return Err(bad(format!("must have exactly {KEYS:?}; has {keys:?}")));
        }
        let path_at = |ptr: &str| -> Result<PathBuf, String> {
            let s = v
                .pointer(ptr)
                .and_then(Value::as_str)
                .ok_or_else(|| bad(format!("{ptr} is not a string")))?;
            let p = PathBuf::from(s);
            if !p.is_absolute() {
                return Err(bad(format!(
                    "{ptr} = {s} is not absolute: a protected path is a fixed host path"
                )));
            }
            Ok(p)
        };
        let pin_at = |ptr: &str| -> Result<String, String> {
            v.pointer(ptr)
                .and_then(Value::as_str)
                .filter(|h| h.len() == 64 && h.bytes().all(|b| b.is_ascii_hexdigit()))
                .map(str::to_ascii_lowercase)
                .ok_or_else(|| bad(format!("{ptr} is not a sha256")))
        };
        // A pinned file: operator-owned and exactly the pinned bytes.
        let pinned = |what: &str| -> Result<(PathBuf, String), String> {
            let p = path_at(&format!("/{what}/path"))?;
            let pin = pin_at(&format!("/{what}/sha256"))?;
            owned(&p, false).map_err(bad)?;
            let got = sha256_file(&p).map_err(bad)?;
            if got != pin {
                return Err(bad(format!(
                    "{what} {} has sha256 {got}, not its pin {pin}",
                    p.display()
                )));
            }
            Ok((p, pin))
        };
        let (launcher, launcher_sha256) = pinned("launcher")?;
        let (manifest, _) = pinned("profile_manifest")?;
        let (suite_registry, suite_registry_sha256) = pinned("suite_registry")?;
        let artifacts_dir = path_at("/artifacts_dir")?;
        owned(&artifacts_dir, true).map_err(bad)?;

        let q = &v["qualification"];
        let record = path_at("/qualification/record")?;
        owned(&record, false).map_err(bad)?;
        let signature = match q.get("signature") {
            None | Some(Value::Null) => None,
            Some(_) => Some(path_at("/qualification/signature")?),
        };
        let waivers = match q.get("waivers") {
            None | Some(Value::Null) => None,
            Some(_) => Some(path_at("/qualification/waivers")?),
        };
        for p in signature.iter().chain(waivers.iter()) {
            owned(p, false).map_err(bad)?;
        }
        trust.max_age_s = match q.get("max_age_s") {
            None | Some(Value::Null) => DEFAULT_EVIDENCE_MAX_AGE_S,
            Some(n) => n
                .as_u64()
                .ok_or_else(|| bad("qualification.max_age_s is not a number".into()))?,
        };

        let sg = &v["signer"];
        let signer = SignerSpec {
            issuer_ref: sg["issuer_ref"]
                .as_str()
                .ok_or_else(|| bad("signer.issuer_ref is not a string".into()))?
                .to_string(),
            public_key: sg["public_key"]
                .as_str()
                .ok_or_else(|| bad("signer.public_key is not a string".into()))?
                .to_string(),
            key_path: path_at("/signer/key_path")?,
        };
        // The key itself belongs to the Fabric service UID (checked when it is
        // read); every directory above it is the operator's.
        if let Some(dir) = signer.key_path.parent() {
            owned(dir, false).map_err(bad)?;
        }
        let out_root = path_at("/out_root")?;

        Ok(ProtectedHost {
            config_sha256: sha256_hex(&bytes),
            linux: LinuxProfileConfig {
                launcher,
                launcher_sha256,
                manifest,
                artifacts_dir: Some(artifacts_dir),
                evidence: record,
                evidence_signature: signature,
                waivers,
                trust,
                out_root,
            },
            suite_registry,
            suite_registry_sha256,
            signer,
        })
    }
}
