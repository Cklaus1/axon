//! The operator's grant registry: `grant_ref` → a real `axon_os::Grant`.
//!
//! A request's `grant_ref` is an opaque string. It becomes authority only by
//! being looked up HERE, in a file the operator wrote, the same way
//! `CheckRegistry` turns `registered_executable_ref` into a pinned binary:
//!
//! ```json
//! {"schema": "axon-fabric-grant-registry/1",
//!  "grants": [{"grant_ref": "grant:ci", "principal_ref": "principal:ci",
//!              "path": "ci.axgrant", "sha256": "<hex of the file's bytes>"}]}
//! ```
//!
//! `path` is relative to the registry file. The grant file is an axon-os
//! `.axjob` WITHOUT a `program` line (`profile`, `require_approval`,
//! `[grant]`, `[grant.budget]`), parsed by `axon_os::parse_manifest` itself —
//! so a misspelled profile is refused, an omitted dimension takes the
//! profile's default, `reproducible` follows the profile, and
//! `require_approval` is the job policy, exactly as for any axon-os job. No
//! second grant parser exists.
//!
//! Binding: the file's sha256 must equal the registry's, and the request's
//! `principal_ref` must equal the entry's. The registry binds a principal to
//! a grant; it does NOT authenticate that the caller IS that principal (a
//! `principal_ref` is a claim, as `AXON_PRINCIPAL` is).

use std::path::{Path, PathBuf};

use axon_os::{Grant, JobManifest};

pub const GRANT_REGISTRY_SCHEMA: &str = "axon-fabric-grant-registry/1";

/// The placeholder program the grant file is parsed with. Replaced by the
/// request's program before admission; a grant file that names its own
/// `program` is refused (it would otherwise override this one).
const PLACEHOLDER_PROGRAM: &str = "fabric-request.ax";

/// A grant resolved from the registry.
#[derive(Debug, Clone)]
pub struct ResolvedGrant {
    pub grant_ref: String,
    pub principal_ref: String,
    /// The grant file. Its `.approval` sibling is the sign-off token
    /// `axon_os` verifies (`approval::authorize`).
    pub path: PathBuf,
    pub sha256: String,
    /// The parsed policy: `grant` + `require_approval`. `program` is a
    /// placeholder until [`ResolvedGrant::manifest_for`].
    manifest: JobManifest,
}

impl ResolvedGrant {
    pub fn grant(&self) -> &Grant {
        &self.manifest.grant
    }
    pub fn require_approval(&self) -> bool {
        self.manifest.require_approval
    }
    /// The axon-os job manifest for running `program` under this grant.
    pub fn manifest_for(&self, program: &Path, intent: String) -> JobManifest {
        JobManifest {
            program: program.to_path_buf(),
            intent,
            ..self.manifest.clone()
        }
    }
}

#[derive(Debug, Clone)]
struct Entry {
    grant_ref: String,
    principal_ref: String,
    path: PathBuf,
    sha256: String,
}

/// The loaded registry. Entries are resolved (read + digest-checked +
/// parsed) at lookup, so a grant file edited after load is refused.
#[derive(Debug, Clone, Default)]
pub struct GrantRegistry {
    entries: Vec<Entry>,
    /// sha256 of the exact registry bytes that were parsed. Recorded in every
    /// operation's intent, so what authorized an op is auditable, and — on a
    /// protected host — compared with the operator's pin.
    sha256: String,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

impl GrantRegistry {
    /// A registry the CALLER names (development: no protected host). Its
    /// digest is still recorded ([`GrantRegistry::sha256`]).
    pub fn load(file: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(file)
            .map_err(|e| format!("cannot read grant registry {}: {e}", file.display()))?;
        Self::parse(file, &bytes)
    }

    /// The OPERATOR's registry, pinned by the protected-host config: the bytes
    /// are read ONCE, and parsed only if they are exactly the pinned ones.
    pub fn load_pinned(file: &Path, pin: &str) -> Result<Self, String> {
        let bytes = std::fs::read(file)
            .map_err(|e| format!("cannot read grant registry {}: {e}", file.display()))?;
        let got = sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(pin) {
            return Err(format!(
                "grant registry {} has sha256 {got}, not its pin {pin}",
                file.display()
            ));
        }
        Self::parse(file, &bytes)
    }

    /// sha256 of the registry bytes this was parsed from.
    pub fn sha256(&self) -> &str {
        &self.sha256
    }

    /// Every grant file the registry names (for the operator-ownership walk).
    pub fn grant_files(&self) -> impl Iterator<Item = &Path> {
        self.entries.iter().map(|e| e.path.as_path())
    }

    fn parse(file: &Path, bytes: &[u8]) -> Result<Self, String> {
        let sha256 = sha256_hex(bytes);
        let v: serde_json::Value = serde_json::from_slice(bytes)
            .map_err(|e| format!("grant registry {} is not JSON: {e}", file.display()))?;
        if v.get("schema").and_then(|s| s.as_str()) != Some(GRANT_REGISTRY_SCHEMA) {
            return Err(format!(
                "grant registry {} must have schema `{GRANT_REGISTRY_SCHEMA}`",
                file.display()
            ));
        }
        let base = file.parent().unwrap_or(Path::new("."));
        let mut entries = Vec::new();
        for e in v
            .get("grants")
            .and_then(|x| x.as_array())
            .ok_or("grant registry has no `grants` array")?
        {
            let field = |k: &str| {
                e.get(k)
                    .and_then(|x| x.as_str())
                    .map(String::from)
                    .ok_or_else(|| format!("grant registry entry missing `{k}`"))
            };
            let grant_ref = field("grant_ref")?;
            if entries.iter().any(|x: &Entry| x.grant_ref == grant_ref) {
                return Err(format!("grant registry names `{grant_ref}` twice"));
            }
            let sha256 = field("sha256")?.to_ascii_lowercase();
            if sha256.len() != 64 || !sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!("grant `{grant_ref}`: sha256 must be 64 hex"));
            }
            entries.push(Entry {
                grant_ref,
                principal_ref: field("principal_ref")?,
                path: base.join(field("path")?),
                sha256,
            });
        }
        Ok(GrantRegistry { entries, sha256 })
    }

    /// Resolve `grant_ref` for `principal_ref`. Every failure is a refusal:
    /// unknown ref, principal not bound to it, unreadable file, digest
    /// mismatch, or a grant file axon-os refuses to parse.
    pub fn resolve(&self, grant_ref: &str, principal_ref: &str) -> Result<ResolvedGrant, String> {
        let e = self
            .entries
            .iter()
            .find(|e| e.grant_ref == grant_ref)
            .ok_or_else(|| format!("grant_ref `{grant_ref}` is not in the grant registry"))?;
        if e.principal_ref != principal_ref {
            return Err(format!(
                "grant `{grant_ref}` is bound to principal `{}`, not `{principal_ref}`",
                e.principal_ref
            ));
        }
        let bytes = std::fs::read(&e.path)
            .map_err(|err| format!("grant `{grant_ref}`: {}: {err}", e.path.display()))?;
        let found = sha256_hex(&bytes);
        if found != e.sha256 {
            return Err(format!(
                "grant `{grant_ref}`: {} has sha256 {found}, the registry pins {}",
                e.path.display(),
                e.sha256
            ));
        }
        let text = String::from_utf8(bytes)
            .map_err(|_| format!("grant `{grant_ref}`: file is not UTF-8"))?;
        let base = e.path.parent().unwrap_or(Path::new("."));
        let src = format!("program = \"{PLACEHOLDER_PROGRAM}\"\n{text}");
        let manifest = axon_os::parse_manifest(&src, base)
            .map_err(|v| format!("grant `{grant_ref}` refused by axon-os: {v:?}"))?;
        if manifest.program != base.join(PLACEHOLDER_PROGRAM) {
            return Err(format!(
                "grant `{grant_ref}`: a grant file must not name a `program` — the program \
                 is the request's"
            ));
        }
        Ok(ResolvedGrant {
            grant_ref: e.grant_ref.clone(),
            principal_ref: e.principal_ref.clone(),
            path: e.path.clone(),
            sha256: e.sha256.clone(),
            manifest,
        })
    }
}

/// The interpreter effect ceiling (`AXON_ALLOWED_EFFECTS`) a grant induces.
///
/// The SAME mapping `axon_os::runtime` uses when it wraps a job in
/// `sandbox_create_scoped` (`wrap_in_sandbox`): net ⇒ `Net,AI`; any of
/// fs_read / fs_write / exec ⇒ `IO`; exec ⇒ `Exec`. A grant that withholds
/// every axis yields `""`, which the interpreter reads as DENY EVERY EFFECT —
/// never as "no ceiling".
pub fn effect_ceiling(g: &Grant) -> String {
    let s = g.effect_set();
    let mut tags: Vec<&str> = Vec::new();
    if s.net {
        tags.push("Net");
        tags.push("AI");
    }
    if s.fs_read || s.fs_write || s.exec {
        tags.push("IO");
    }
    if s.exec {
        tags.push("Exec");
    }
    tags.join(",")
}

/// Does the grant withhold any effect axis? (Then enforcing it needs a policy
/// channel into wherever the program runs.)
pub fn restricts_effects(g: &Grant) -> bool {
    let s = g.effect_set();
    !(s.fs_read && s.fs_write && s.net && s.exec)
}

/// Does the grant scope filesystem paths or hosts (anything other than
/// nothing or `*`)?
pub fn is_path_scoped(g: &Grant) -> bool {
    let scoped = |v: &[String]| !(v.is_empty() || (v.len() == 1 && v[0] == "*"));
    scoped(&g.fs_read) || scoped(&g.fs_write) || scoped(&g.net)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_os::{Budget, ExecPolicy, Label};

    fn g(fs: bool, net: bool, exec: bool) -> Grant {
        Grant {
            reproducible: false,
            fs_read: if fs { vec!["*".into()] } else { vec![] },
            fs_write: vec![],
            net: if net { vec!["*".into()] } else { vec![] },
            exec: if exec {
                ExecPolicy::Any
            } else {
                ExecPolicy::None
            },
            max_label: Label::Internal,
            budget: Budget {
                calls: 0,
                tokens: 0,
                cost_micro: 0,
            },
        }
    }

    #[test]
    fn the_ceiling_mirrors_the_axon_os_sandbox_wrapper() {
        assert_eq!(effect_ceiling(&g(false, false, false)), "");
        assert_eq!(effect_ceiling(&g(true, false, false)), "IO");
        assert_eq!(effect_ceiling(&g(false, true, false)), "Net,AI");
        assert_eq!(effect_ceiling(&g(false, false, true)), "IO,Exec");
        assert_eq!(effect_ceiling(&g(true, true, true)), "Net,AI,IO,Exec");
    }
}
