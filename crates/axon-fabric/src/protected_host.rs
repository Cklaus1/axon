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
//! * the output root;
//! * the operator GRANT registry (pinned; every grant file it names
//!   operator-owned). It is the only source of a request's authority and of
//!   the guest effect policy derived from it: `axon-fabric` refuses a caller
//!   `--grant-registry` on a protected host, on every route (D1). A host
//!   config without one authorizes nothing.
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
    /// M3: the preflight observer, if this host has one.
    pub observer: Option<crate::observer::ObserverConfig>,
    /// D1: the operator's grant registry and its pin. `None`: this host pins
    /// no grant registry, so nothing is authorized on it ([`Self::grants`]).
    pub grant_registry: Option<(PathBuf, String)>,
}

/// Why a protected host authorizes nothing when its config pins no grant
/// registry.
pub const NO_GRANT_REGISTRY: &str =
    "this protected host's config pins no grant_registry: nothing is authorized on it";

impl ProtectedHost {
    /// The operator's grant registry — the ONLY authority source on a
    /// protected host. Read once and parsed only at its pin.
    pub fn grants(&self) -> Result<crate::grants::GrantRegistry, String> {
        let (path, pin) = self
            .grant_registry
            .as_ref()
            .ok_or_else(|| NO_GRANT_REGISTRY.to_string())?;
        crate::grants::GrantRegistry::load_pinned(path, pin)
    }
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
    /// not a protected host, and the profile is simply not configured. Any
    /// OTHER failure to tell (EACCES, ENOTDIR, ELOOP, …) is refused: it is not
    /// evidence that this is a development host.
    #[cfg(unix)]
    pub fn operator() -> Result<Option<Self>, String> {
        let p = Path::new(PROTECTED_HOST_CONFIG);
        if !is_configured(p)? {
            return Ok(None);
        }
        Self::load(
            p,
            Some(Path::new("/")),
            QualificationTrust::operator(),
            crate::observer::ObserverTrust::operator(),
        )
        .map(Some)
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
        let mut observers =
            crate::observer::ObserverTrust::for_test(&trust.issuers_dir.join("../observer"));
        // The test's qualification root is the issuers dir itself.
        for (a, dir) in &mut observers.separate_from {
            if *a == crate::backend::TrustAuthority::Qualification {
                *dir = trust.issuers_dir.clone();
            }
        }
        Self::load(config, owned_below, trust, observers)
    }

    #[cfg(unix)]
    fn load(
        config: &Path,
        owned_below: Option<&Path>,
        mut trust: QualificationTrust,
        mut observer_trust: crate::observer::ObserverTrust,
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
        // `observer` (M3) and `grant_registry` (D1) are the optional sections.
        keys.retain(|k| *k != "observer" && *k != "grant_registry");
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
        // The Fabric service writes its runs under `out_root`, and the
        // custodian its nonce records under `nonce_store`: neither leaf is the
        // operator's, but WHERE it sits is, exactly as for the signing key. An
        // agent-writable ancestor could swap either for a directory it
        // controls (pre-planted run outputs; erased nonce records, so an
        // observation replays). The LEAF is the service's own, and private:
        // an agent-owned or group/other-accessible leaf allows the same swap
        // one level down (C9 dev review round 1; A56).
        let parent_owned = |p: &Path| -> Result<(), String> {
            let dir = p
                .parent()
                .ok_or_else(|| bad(format!("{} has no parent directory", p.display())))?;
            owned(dir, false).map_err(bad)
        };
        let leaf_owned = |p: &Path| -> Result<(), String> {
            match owned_below {
                Some(_) => service_leaf(p).map_err(bad),
                None => Ok(()),
            }
        };
        let out_root = path_at("/out_root")?;
        parent_owned(&out_root)?;
        leaf_owned(&out_root)?;
        let observer = match v.get("observer") {
            None | Some(Value::Null) => None,
            Some(ob) => {
                let (command, command_sha256) = pinned("observer/command")?;
                let nonces = path_at("/observer/nonce_store")?;
                parent_owned(&nonces)?;
                leaf_owned(&nonces)?;
                // ADR-002: the observer root shares no key with the host
                // signer or another authority root (A57).
                observer_trust.host_signer_public_key = Some(signer.public_key.clone());
                observer_trust.check_separation().map_err(bad)?;
                Some(crate::observer::ObserverConfig {
                    command,
                    command_sha256,
                    trust: observer_trust,
                    nonces: crate::observer::NonceStore { dir: nonces },
                    max_age_s: match ob.get("max_age_s") {
                        None | Some(Value::Null) => crate::observer::DEFAULT_OBSERVATION_MAX_AGE_S,
                        Some(n) => n
                            .as_u64()
                            .ok_or_else(|| bad("observer.max_age_s is not a number".into()))?,
                    },
                    clock: crate::backend::Clock::System,
                })
            }
        };
        // D1: the grant registry is pinned and operator-owned, and so is every
        // grant file it names (and so the directory an `.approval` token sits
        // in): the caller writes none of them.
        let grant_registry = match v.get("grant_registry") {
            None | Some(Value::Null) => None,
            Some(_) => {
                let (path, pin) = pinned("grant_registry")?;
                let reg = crate::grants::GrantRegistry::load_pinned(&path, &pin).map_err(bad)?;
                for g in reg.grant_files() {
                    owned(g, false).map_err(bad)?;
                }
                Some((path, pin))
            }
        };

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
            observer,
            grant_registry,
        })
    }
}

/// What a path the host config pins IS, for the trust preflight
/// (`scripts/trust_root_preflight.sh`), which probes each one with real
/// attempts under every actor UID.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinnedKind {
    /// Operator-owned: nobody but the operator may create, write or chmod it
    /// (the host config itself, every pinned file, every grant file).
    OperatorFile,
    /// An operator-owned directory whose direct entries are operator-owned
    /// too (`artifacts_dir`).
    OperatorDir,
    /// The attestation signing key: readable by the Fabric UID alone (A20);
    /// the directory above it is operator-owned.
    SigningKey,
    /// A directory the Fabric SERVICE owns, private (`out_root`,
    /// `observer.nonce_store`); the directory above it is operator-owned.
    ServiceDir,
}

impl PinnedKind {
    pub fn as_str(self) -> &'static str {
        match self {
            PinnedKind::OperatorFile => "operator-file",
            PinnedKind::OperatorDir => "operator-dir",
            PinnedKind::SigningKey => "signing-key",
            PinnedKind::ServiceDir => "service-dir",
        }
    }
}

/// EVERY path an `axon-protected-host/1` config at `config` pins, with what
/// it is — the list `ProtectedHost::load` ownership-walks, printed by
/// `axon-fabric protected-host-paths` so the trust preflight probes exactly
/// these (C9 dev review round 1: the preflight kept its own list and missed
/// the grant registry and its grant files, the observer command,
/// `artifacts_dir` and the out_root / nonce_store directories). It reads the
/// config and the grant registry it names; it verifies NO pin and authorizes
/// nothing. `tests/protected_host.rs` holds it to `load` in both directions.
pub fn pinned_paths(config: &Path) -> Result<Vec<(PinnedKind, PathBuf)>, String> {
    let bad = |why: String| format!("{}: {why}", config.display());
    let bytes = std::fs::read(config).map_err(|e| bad(e.to_string()))?;
    let v: Value = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
    if v["schema"] != PROTECTED_HOST_SCHEMA {
        return Err(bad(format!("schema is not {PROTECTED_HOST_SCHEMA}")));
    }
    let path_at = |ptr: &str| -> Result<PathBuf, String> {
        let p = v
            .pointer(ptr)
            .and_then(Value::as_str)
            .map(PathBuf::from)
            .ok_or_else(|| bad(format!("{ptr} is not a string")))?;
        if !p.is_absolute() {
            return Err(bad(format!("{ptr} is not absolute")));
        }
        Ok(p)
    };
    let present = |ptr: &str| !matches!(v.pointer(ptr), None | Some(Value::Null));
    use PinnedKind::*;
    let mut out = vec![(OperatorFile, config.to_path_buf())];
    for ptr in [
        "/launcher/path",
        "/profile_manifest/path",
        "/suite_registry/path",
        "/qualification/record",
    ] {
        out.push((OperatorFile, path_at(ptr)?));
    }
    for ptr in ["/qualification/signature", "/qualification/waivers"] {
        if present(ptr) {
            out.push((OperatorFile, path_at(ptr)?));
        }
    }
    out.push((OperatorDir, path_at("/artifacts_dir")?));
    out.push((SigningKey, path_at("/signer/key_path")?));
    out.push((ServiceDir, path_at("/out_root")?));
    if present("/observer") {
        out.push((OperatorFile, path_at("/observer/command/path")?));
        out.push((ServiceDir, path_at("/observer/nonce_store")?));
    }
    if present("/grant_registry") {
        let reg = path_at("/grant_registry/path")?;
        let grants = crate::grants::GrantRegistry::load(&reg).map_err(bad)?;
        out.extend(
            grants
                .grant_files()
                .map(|g| (OperatorFile, g.to_path_buf())),
        );
        out.push((OperatorFile, reg));
    }
    Ok(out)
}

/// Whether the operator's host config exists. Only NotFound means "not a
/// protected host"; any other error is refused rather than read as one (C9
/// dev review round 1).
#[cfg(unix)]
fn is_configured(p: &Path) -> Result<bool, String> {
    match std::fs::symlink_metadata(p) {
        Ok(_) => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(format!(
            "{}: {e}: cannot tell whether this is a protected host, so nothing runs",
            p.display()
        )),
    }
}

/// A leaf the Fabric SERVICE owns (`out_root`, `nonce_store`): it exists, is a
/// real directory (not a symlink), belongs to this process's euid, and no
/// group or other has any access (0700).
#[cfg(unix)]
fn service_leaf(p: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(p)
        .map_err(|e| format!("{}: {e} (the service's directory must exist)", p.display()))?;
    if m.file_type().is_symlink() {
        return Err(format!("{} is a symlink: never redirected", p.display()));
    }
    if !m.is_dir() {
        return Err(format!("{} is not a directory", p.display()));
    }
    // SAFETY: geteuid has no preconditions and cannot fail.
    let euid = unsafe { libc::geteuid() };
    if m.uid() != euid {
        return Err(format!(
            "{} is owned by uid {}, not the service uid {euid}: another uid could swap what is \
             in it",
            p.display(),
            m.uid()
        ));
    }
    if m.mode() & 0o077 != 0 {
        return Err(format!(
            "{} is accessible to group or other (mode {:o}); it must be 0700",
            p.display(),
            m.mode() & 0o7777
        ));
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    /// C9 dev review round 1: `operator()` read ANY stat error as "not a
    /// protected host" and ran in development mode (caller registries and
    /// signer). Only NotFound means that.
    #[test]
    fn only_a_missing_host_config_means_not_a_protected_host() {
        let d = tempfile::tempdir().unwrap();
        let file = d.path().join("file");
        std::fs::write(&file, "x").unwrap();
        let lp = d.path().join("loop");
        std::os::unix::fs::symlink(&lp, &lp).unwrap();
        for (what, p) in [
            ("ENOTDIR", file.join("protected-host.json")),
            ("ELOOP", lp.join("protected-host.json")),
        ] {
            let got = is_configured(&p);
            assert!(
                got.is_err(),
                "ATTACK: a host config that cannot be stat'ed ({what}) was read as \
                 'not a protected host': {got:?}"
            );
        }
        assert_eq!(is_configured(&d.path().join("absent.json")), Ok(false));
        assert_eq!(is_configured(&file), Ok(true));
    }
}
