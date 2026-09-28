//! The AUTHORITATIVE protected-readiness decision
//! (governance/specs/v022-protected-suite-verdict.md).
//!
//! `protected_backend`, `g01_on_protected_backend` and `pci_on_protected_backend`
//! are decided HERE, by a binary the operator installs and pins
//! (`/etc/axon/trust/verifier.json`), never by a repository script: an agent
//! with full write access to the repository can change its sources, scripts,
//! manifests, proof files and local keys, but not the installed verifier and
//! not the operator's trust root. The repository is read as DATA only: the
//! evidence it claims. Authority comes from `/etc/axon/trust/qualification/`.
//!
//! A component is PASS only if ALL of these hold:
//! * its gate rows are registered and its proof documents exist;
//! * its protected-host certification record (`axon-v022-protected-certification/2`)
//!   binds every required field, certifies THIS spec, a revision this tree
//!   descends from with nothing outside `governance/` changed since, and an
//!   evidence bundle that still hashes to what was certified;
//! * that record's detached QUALIFICATION-domain `axon-evidence-signature/2` verifies under a key in
//!   the operator's qualification root — owned by root along its whole path,
//!   no symlinks, no group/other write, and not writable by this process;
//! * the signer is among the repository's expected issuers, if it lists any
//!   (the repository may narrow, never add).

use crate::backend::{TrustAuthority, TEST_TRUST_BUILD};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const READINESS_SCHEMA: &str = "axon-fabric-readiness/1";
pub const CERT_SCHEMA: &str = "axon-v022-protected-certification/2";
pub const PROTECTED_PROFILE: &str = "linux-microvm-protected";
const PSV_SPEC: &str = "governance/specs/v022-protected-suite-verdict.md";
const REGISTRY: &str = "governance/cortex_gate_execution_registry.json";
const CERT_DIR: &str = "governance/proofs/v022-protected";
const TRUST_EXPECTATIONS: &str = "governance/status/trust-expectations.json";

/// The fields a certification must bind, so any later change invalidates it.
pub const CERT_FIELDS: [&str; 22] = [
    "schema",
    "component",
    "host_profile",
    "qualification_profile",
    "psv_spec_sha256",
    "axon_sha",
    "micode_sha",
    "fabric_revision",
    "guest_image_sha256",
    "guest_kernel_sha256",
    "guest_runtime_sha256",
    "suite",
    "candidate_tree_ref",
    "observer_key_id",
    "observation_sha256",
    "verifier_key_id",
    "b263_qualification_sha256",
    "evidence",
    "evidence_bundle_sha256",
    "readiness_verifier_sha256",
    "trust_preflight_sha256",
    "certified_at",
];

/// (component, gates it needs, proof documents it needs)
pub const COMPONENTS: [(&str, &[&str], &[&str]); 3] = [
    (
        "protected_backend",
        &[
            "G13-r22-profile-qualification",
            "G13-r22-profile-eligibility",
            "G13-r22-guest-truth",
        ],
        &[],
    ),
    (
        "g01_on_protected_backend",
        &["G01-r22-registered-check", "G01-r22-verifier-separation"],
        &["governance/proofs/v022-g01-microvm/REGISTRATION.md"],
    ),
    (
        "pci_on_protected_backend",
        &["G03-r22-trial-isolation", "G03-r22-physical-isolation"],
        &["governance/proofs/v022-pci-microvm/CERTIFICATION.md"],
    ),
];

/// Where authority comes from. Production: the operator's qualification root,
/// checked from `/` and required unwritable by this process.
#[derive(Debug, Clone)]
pub struct ReadinessTrust {
    pub issuers_dir: PathBuf,
    ownership_base: PathBuf,
    require_unwritable: bool,
}

impl ReadinessTrust {
    pub fn operator() -> ReadinessTrust {
        ReadinessTrust {
            issuers_dir: TrustAuthority::Qualification.operator_dir(),
            ownership_base: PathBuf::from("/"),
            require_unwritable: true,
        }
    }

    /// TESTS ONLY: a temp root (its ancestors are not operator-owned, and the
    /// test runs as its owner). A build with this carries `TEST_TRUST_BUILD`
    /// and reports `build: "test-trust"`, which readiness never accepts.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn test(base: &Path, issuers_dir: &Path) -> ReadinessTrust {
        ReadinessTrust {
            issuers_dir: issuers_dir.to_path_buf(),
            ownership_base: base.to_path_buf(),
            require_unwritable: false,
        }
    }

    fn check(&self) -> Result<(), String> {
        #[cfg(unix)]
        {
            crate::backend::check_owned_from_pub(&self.ownership_base, &self.issuers_dir)?;
            if self.require_unwritable {
                let mut paths = vec![self.issuers_dir.clone()];
                let mut p = self.issuers_dir.clone();
                while let Some(parent) = p.parent().map(Path::to_path_buf) {
                    paths.push(parent.clone());
                    p = parent;
                }
                if let Ok(rd) = std::fs::read_dir(&self.issuers_dir) {
                    paths.extend(rd.filter_map(|e| e.ok().map(|e| e.path())));
                }
                for q in paths {
                    if writable_by_me(&q) {
                        return Err(format!(
                            "{} is writable by the process running this check: an agent-writable \
                             trust root authorizes nothing",
                            q.display()
                        ));
                    }
                }
            }
            Ok(())
        }
        #[cfg(not(unix))]
        Err("operator trust cannot be checked on this platform".into())
    }
}

#[cfg(unix)]
fn writable_by_me(p: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let Ok(c) = std::ffi::CString::new(p.as_os_str().as_bytes()) else {
        return true;
    };
    // SAFETY: a valid NUL-terminated path; access() only reads it.
    unsafe { libc::access(c.as_ptr(), libc::W_OK) == 0 }
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(format!("{:x}", Sha256::digest(&b)))
}

fn git(repo: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .map_err(|e| format!("git: {e}"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

fn is_hex(v: &Value, n: usize) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == n
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}

/// The certification of `component` in `repo`: Ok(issuer) or Err(why).
fn certification(repo: &Path, component: &str, trust: &ReadinessTrust) -> Result<String, String> {
    let rec = repo.join(CERT_DIR).join(format!("{component}.json"));
    if !rec.exists() {
        return Err(format!(
            "{CERT_DIR}/{component}.json: no protected-host certification record (earned only on the \
             protected host)"
        ));
    }
    let doc: Value = serde_json::from_slice(
        &std::fs::read(&rec).map_err(|e| format!("{}: {e}", rec.display()))?,
    )
    .map_err(|e| format!("{component}: record is not JSON: {e}"))?;
    let missing: Vec<&str> = CERT_FIELDS
        .iter()
        .copied()
        .filter(|k| doc.get(k).is_none())
        .collect();
    if doc["schema"] != CERT_SCHEMA || !missing.is_empty() {
        return Err(format!(
            "{component}: not a {CERT_SCHEMA} record (missing {missing:?})"
        ));
    }
    if doc["component"] != component
        || doc["host_profile"] != PROTECTED_PROFILE
        || doc["qualification_profile"] != PROTECTED_PROFILE
    {
        return Err(format!(
            "{component}: not a {PROTECTED_PROFILE} certification of {component}"
        ));
    }
    for k in [
        "psv_spec_sha256",
        "guest_image_sha256",
        "guest_kernel_sha256",
        "guest_runtime_sha256",
        "observation_sha256",
        "b263_qualification_sha256",
        "evidence_bundle_sha256",
        "readiness_verifier_sha256",
        "trust_preflight_sha256",
    ] {
        if !is_hex(&doc[k], 64) {
            return Err(format!("{component}: {k} is not a sha256"));
        }
    }
    for k in ["axon_sha", "micode_sha", "fabric_revision"] {
        if !is_hex(&doc[k], 40) {
            return Err(format!("{component}: {k} is not a full commit id"));
        }
    }
    let suite = &doc["suite"];
    if ["id", "version", "entry", "test", "digest"]
        .iter()
        .any(|k| suite[k].as_str().is_none_or(str::is_empty))
    {
        return Err(format!(
            "{component}: suite must name id, version, entry, test and digest"
        ));
    }
    // Bound to THIS spec, and to THIS code.
    if doc["psv_spec_sha256"].as_str() != Some(sha256_file(&repo.join(PSV_SPEC))?.as_str()) {
        return Err(format!(
            "{component}: certifies another version of {PSV_SPEC}"
        ));
    }
    let certified = doc["axon_sha"].as_str().expect("checked");
    if !git(repo, &["merge-base", "--is-ancestor", certified, "HEAD"])?.0 {
        return Err(format!(
            "{component}: axon_sha {certified} is not an ancestor of this tree"
        ));
    }
    let (ok, changed) = git(repo, &["diff", "--name-only", certified, "HEAD"])?;
    // Behind the ancestor check above (so no test reaches it): a git failure
    // here — e.g. a damaged object store — is never read as "no change".
    if !ok {
        return Err(format!(
            "{component}: cannot compare this tree with axon_sha {certified}: nothing unverified \
             is assumed unchanged"
        ));
    }
    // `-z`: NUL-separated `XY path` entries, exactly as git wrote them (a
    // trimmed line-mode output loses the status column of the first entry).
    let dirty = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["status", "--porcelain", "-z", "--untracked-files=all"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    let dirty = String::from_utf8_lossy(&dirty.stdout).to_string();
    let outside: Vec<String> = changed
        .lines()
        .map(str::to_string)
        .chain(
            dirty
                .split('\0')
                .filter(|e| e.len() > 3)
                .map(|e| e[3..].to_string()),
        )
        .filter(|f| !f.is_empty() && !f.starts_with("governance/"))
        .collect();
    if let Some(f) = outside.first() {
        return Err(format!(
            "{component}: {} file(s) outside governance/ changed since the certified revision \
             (e.g. {f})",
            outside.len()
        ));
    }
    let ev = doc["evidence"]
        .as_array()
        .filter(|a| !a.is_empty())
        .ok_or(format!("{component}: no evidence listed"))?;
    let mut concat = String::new();
    let mut preflight = None;
    for e in ev {
        let p = e
            .as_str()
            .ok_or(format!("{component}: evidence entries are paths"))?;
        let h = sha256_file(&repo.join(p))?;
        if doc["trust_preflight_sha256"].as_str() == Some(h.as_str()) {
            preflight = Some(repo.join(p));
        }
        concat.push_str(&h);
    }
    // The executable trust-root preflight (real write attempts under the
    // service UIDs, scripts/trust_root_preflight.sh) is part of the certified
    // evidence, and must be a PROTECTED-mode run that passed. A dev-mode run
    // proves the mechanism and certifies nothing.
    let pf = preflight.ok_or(format!(
        "{component}: trust_preflight_sha256 names no certified evidence file"
    ))?;
    let pf: Value = serde_json::from_slice(&std::fs::read(&pf).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{component}: trust preflight report: {e}"))?;
    if pf["schema"] != TRUST_PREFLIGHT_SCHEMA
        || pf["mode"] != "protected"
        || pf["verdict"] != "PASS"
    {
        return Err(format!(
            "{component}: the trust preflight is not a passing protected-mode {TRUST_PREFLIGHT_SCHEMA} \
             run (mode {}, verdict {})",
            pf["mode"], pf["verdict"]
        ));
    }
    use sha2::{Digest, Sha256};
    if doc["evidence_bundle_sha256"].as_str()
        != Some(format!("{:x}", Sha256::digest(concat.as_bytes())).as_str())
    {
        return Err(format!(
            "{component}: the evidence bundle changed since it was certified"
        ));
    }
    // The verifier deciding NOW is the one the certification was made with:
    // replacing the installed verifier invalidates it. A production verifier
    // is built from a clean tree.
    let me = verifier_identity();
    if doc["readiness_verifier_sha256"] != me["sha256"] {
        return Err(format!(
            "{component}: certified with readiness verifier {}, but this verifier is {}",
            doc["readiness_verifier_sha256"], me["sha256"]
        ));
    }
    if !TEST_TRUST_BUILD && me["source_dirty"] == true {
        return Err(format!(
            "{component}: this verifier was built from a dirty tree"
        ));
    }
    // Authority: the operator's root, never the repository.
    trust.check()?;
    let mut sig = rec.as_os_str().to_owned();
    sig.push(".sig");
    let issuer = crate::backend::verify_operator_evidence(
        &rec,
        Path::new(&sig),
        &trust.issuers_dir,
        TrustAuthority::Qualification,
    )?;
    let exp = repo.join(TRUST_EXPECTATIONS);
    if exp.exists() {
        let v: Value = serde_json::from_slice(&std::fs::read(&exp).map_err(|e| e.to_string())?)
            .map_err(|e| format!("{TRUST_EXPECTATIONS}: {e}"))?;
        if let Some(list) = v["qualification_issuers"].as_array() {
            if !list.iter().any(|x| x.as_str() == Some(issuer.as_str())) {
                return Err(format!(
                    "{component}: issuer {issuer} is not one this repository expects (it may \
                     narrow, never add)"
                ));
            }
        }
    }
    Ok(issuer)
}

/// Schema of `scripts/trust_root_preflight.sh`'s report.
pub const TRUST_PREFLIGHT_SCHEMA: &str = "axon-trust-preflight/1";

/// WHAT is deciding: this binary's own digest and build provenance (build.rs).
/// Recorded in every verdict, and bound by a certification
/// (`readiness_verifier_sha256`), so replacing the installed verifier is a
/// visible change of authority, never a silent one.
pub fn verifier_identity() -> Value {
    let sha = std::env::current_exe()
        .ok()
        .and_then(|p| sha256_file(&p).ok())
        .unwrap_or_else(|| "unknown".into());
    json!({
        "sha256": sha,
        "build": if TEST_TRUST_BUILD { "test-trust" } else { "production" },
        "fabric_revision": env!("AXON_FABRIC_GIT_SHA"),
        "source_dirty": env!("AXON_FABRIC_GIT_DIRTY") == "true",
        "rustc": env!("AXON_FABRIC_RUSTC"),
        "profile": env!("AXON_FABRIC_PROFILE"),
        "target": env!("AXON_FABRIC_TARGET"),
    })
}

/// The three protected components' verdicts for `repo`.
pub fn protected_components(repo: &Path, trust: &ReadinessTrust) -> Value {
    let registered: Vec<String> = std::fs::read(repo.join(REGISTRY))
        .ok()
        .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
        .and_then(|v| v["gates"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .filter_map(|g| g["gate_id"].as_str().map(str::to_string))
        .collect();
    let mut out = serde_json::Map::new();
    for (name, gates, proofs) in COMPONENTS {
        let mut missing: Vec<String> = gates
            .iter()
            .filter(|g| !registered.iter().any(|r| r == *g))
            .map(|g| g.to_string())
            .chain(
                proofs
                    .iter()
                    .filter(|p| !repo.join(p).exists())
                    .map(|p| p.to_string()),
            )
            .collect();
        let have = gates.len() + proofs.len() - missing.len();
        let cert = certification(repo, name, trust);
        let status = match (&cert, missing.is_empty()) {
            (Ok(_), true) => "PASS",
            _ if have > 0 || cert.is_ok() => "PARTIAL",
            _ => "NOT_RUN",
        };
        let mut c = json!({"status": status});
        match cert {
            Ok(issuer) => c["protected_host_certification_issuer"] = json!(issuer),
            Err(e) => missing.push(e),
        }
        if !missing.is_empty() {
            c["missing"] = json!(missing);
        }
        out.insert(name.to_string(), c);
    }
    json!({
        "schema": READINESS_SCHEMA,
        // A build carrying the test trust constructors never earns readiness.
        "build": if TEST_TRUST_BUILD { "test-trust" } else { "production" },
        "trust_root": trust.issuers_dir,
        "verifier": verifier_identity(),
        "components": out,
    })
}
