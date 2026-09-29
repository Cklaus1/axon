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
#[cfg(unix)]
use crate::git_data::worktree_differs;
use crate::git_data::{Objects, GIT_BIN};
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
///
/// Three roots are read: QUALIFICATION (who may certify), and the OBSERVER
/// and VERIFIER roots that the record's attribution (`observer_key_id`,
/// `verifier_key_id`) must name keys in, at decision time.
#[derive(Debug, Clone)]
pub struct ReadinessTrust {
    pub issuers_dir: PathBuf,
    observer_dir: PathBuf,
    verifier_dir: PathBuf,
    ownership_base: PathBuf,
    require_unwritable: bool,
}

impl ReadinessTrust {
    pub fn operator() -> ReadinessTrust {
        ReadinessTrust {
            issuers_dir: TrustAuthority::Qualification.operator_dir(),
            observer_dir: TrustAuthority::Observer.operator_dir(),
            verifier_dir: TrustAuthority::Verifier.operator_dir(),
            ownership_base: PathBuf::from("/"),
            require_unwritable: true,
        }
    }

    /// TESTS ONLY: a temp root (its ancestors are not operator-owned, and the
    /// test runs as its owner). The observer and verifier roots are
    /// `issuers_dir`'s siblings `observer/` and `verifier/`, as under
    /// `/etc/axon/trust/`. A build with this carries `TEST_TRUST_BUILD` and
    /// reports `build: "test-trust"`, which readiness never accepts.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn test(base: &Path, issuers_dir: &Path) -> ReadinessTrust {
        let sib = |n: &str| issuers_dir.parent().unwrap_or(base).join(n);
        ReadinessTrust {
            issuers_dir: issuers_dir.to_path_buf(),
            observer_dir: sib("observer"),
            verifier_dir: sib("verifier"),
            ownership_base: base.to_path_buf(),
            require_unwritable: false,
        }
    }

    fn check(&self) -> Result<(), String> {
        #[cfg(unix)]
        {
            for dir in [&self.issuers_dir, &self.observer_dir, &self.verifier_dir] {
                crate::backend::check_owned_from_pub(&self.ownership_base, dir)?;
                if self.require_unwritable {
                    let mut paths = vec![dir.clone()];
                    let mut p = dir.clone();
                    while let Some(parent) = p.parent().map(Path::to_path_buf) {
                        paths.push(parent.clone());
                        p = parent;
                    }
                    if let Ok(rd) = std::fs::read_dir(dir) {
                        paths.extend(rd.filter_map(|e| e.ok().map(|e| e.path())));
                    }
                    for q in paths {
                        if writable_by_me(&q) {
                            return Err(format!(
                                "{} is writable by the process running this check: an \
                                 agent-writable trust root authorizes nothing",
                                q.display()
                            ));
                        }
                    }
                }
            }
            Ok(())
        }
        #[cfg(not(unix))]
        Err("operator trust cannot be checked on this platform".into())
    }
}

impl ReadinessTrust {
    /// Every authority root readiness knows: its three, and the others as
    /// `issuers_dir`'s siblings (the `/etc/axon/trust/<authority>` layout).
    fn roots(&self) -> Vec<(TrustAuthority, PathBuf)> {
        let mut roots =
            crate::backend::sibling_roots(TrustAuthority::Qualification, &self.issuers_dir);
        for (a, dir) in &mut roots {
            match a {
                TrustAuthority::Observer => *dir = self.observer_dir.clone(),
                TrustAuthority::Verifier => *dir = self.verifier_dir.clone(),
                _ => {}
            }
        }
        roots.push((TrustAuthority::Qualification, self.issuers_dir.clone()));
        roots
    }

    /// The keys (hex) of the root at `dir` NOW, read EXCLUSIVELY: refused
    /// whole if any is also held by another authority root, which is walked
    /// from the ownership base and must be readable (C9 round 2,
    /// FIELD-ORIGIN; A67). The verifier root holds the host signer's key, so
    /// that key in the qualification root (Fabric minting a certification or
    /// a B263 record) is refused here too. Every readiness trust read comes
    /// through this.
    fn exclusive_keys(&self, dir: &Path) -> Result<Vec<String>, String> {
        let roots = self.roots();
        let a = roots
            .iter()
            .find(|(_, d)| d == dir)
            .map(|(a, _)| *a)
            .ok_or_else(|| format!("{} is not one of readiness's trust roots", dir.display()))?;
        crate::backend::exclusive_root_keys(a, dir, &roots, Some(&self.ownership_base), None)
    }

    /// The key ids (`ed25519:<16 hex>`) of the keys in an operator root now.
    fn key_ids(&self, dir: &Path) -> Result<Vec<String>, String> {
        Ok(self
            .keys(dir)?
            .iter()
            .map(|k| axon_loop_contracts::operator_trust::key_fingerprint(k))
            .collect())
    }

    /// The raw public keys in an operator root now.
    fn keys(&self, dir: &Path) -> Result<Vec<Vec<u8>>, String> {
        Ok(self
            .exclusive_keys(dir)?
            .iter()
            .filter_map(|h| crate::backend::hex_decode(h))
            .collect())
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

/// Every repository file readiness decides on is read ONCE, as a regular
/// file, never through a symlink ([`crate::backend::read_regular`]).
fn read_once(p: &Path) -> Result<Vec<u8>, String> {
    crate::backend::read_regular(p)
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

fn sha256_file(p: &Path) -> Result<String, String> {
    Ok(sha256_hex(&read_once(p)?))
}

/// A git invocation that answers about THIS repository's real objects and
/// nothing else ([`crate::git_data::git_cmd`], the one implementation build
/// provenance shares): the caller's environment dropped, replace refs off,
/// system and global config unread, the working tree the one asked about,
/// no fetch of any kind, and the repository-local settings that run code or
/// answer from a cache overridden on the command line. The repository's own
/// config is refused unless it is inert ([`refuse_git_spoofing`]).
fn git_cmd(repo: &Path) -> Result<std::process::Command, String> {
    let top = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    Ok(crate::git_data::git_cmd(&top))
}

fn git(repo: &Path, args: &[&str]) -> Result<(bool, String), String> {
    let out = git_cmd(repo)?
        .args(args)
        .output()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    Ok((
        out.status.success(),
        String::from_utf8_lossy(&out.stdout).trim().to_string(),
    ))
}

/// Repository-local git state that makes git report something other than
/// the objects and files that are there. Any of it present: refused, never
/// interpreted.
fn refuse_git_spoofing(repo: &Path, component: &str) -> Result<(), String> {
    // The repository's own .git/config: a promisor remote with a
    // core.sshCommand, core.worktree, a filter driver… (review PSV-7, C9
    // round 2). Refused before git is asked anything else.
    let top = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    crate::git_data::refuse_config(&top).map_err(|e| format!("{component}: {e}"))?;
    let (ok, replaced) = git(
        repo,
        &["for-each-ref", "--format=%(refname)", "refs/replace/"],
    )?;
    if !ok {
        return Err(format!("{component}: cannot list refs/replace/"));
    }
    if let Some(r) = replaced.lines().next() {
        return Err(format!(
            "{component}: the repository has refs/replace/ object replacements (e.g. {r}): git \
             would report another object's content under a certified name, so no certification \
             applies"
        ));
    }
    let (ok, grafts) = git(repo, &["rev-parse", "--git-path", "info/grafts"])?;
    if !ok || grafts.is_empty() {
        return Err(format!(
            "{component}: cannot locate the repository's info/grafts"
        ));
    }
    let grafts = repo.join(grafts);
    if std::fs::symlink_metadata(&grafts).is_ok() {
        return Err(format!(
            "{component}: the repository has an info/grafts file ({}): grafted parents rewrite \
             ancestry, so no certification applies",
            grafts.display()
        ));
    }
    // `-v`: `S` marks skip-worktree, a lowercase tag assume-unchanged. Either
    // tells git not to look at the file, which is exactly what is certified.
    let (ok, index) = git(repo, &["ls-files", "-z", "-v"])?;
    if !ok {
        return Err(format!("{component}: cannot read the index"));
    }
    for e in index.split('\0').filter(|e| e.len() > 2) {
        let (tag, path) = (e.as_bytes()[0], &e[2..]);
        if tag == b'S' || tag == b's' {
            return Err(format!(
                "{component}: index entry {path} is marked skip-worktree: git does not compare it \
                 with the working tree, so no certification applies"
            ));
        }
        if tag.is_ascii_lowercase() {
            return Err(format!(
                "{component}: index entry {path} is marked assume-unchanged: git does not compare \
                 it with the working tree, so no certification applies"
            ));
        }
    }
    Ok(())
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
    // ONE read of the record. Every field below is checked on these bytes,
    // and the operator signature is verified over these bytes: never over a
    // second read of the path, which can be other bytes (review PSV-7).
    let rec_bytes = read_once(&rec).map_err(|e| format!("{component}: record {e}"))?;
    let doc: Value = serde_json::from_slice(&rec_bytes)
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
    refuse_git_spoofing(repo, component)?;
    if !git(repo, &["merge-base", "--is-ancestor", certified, "HEAD"])?.0 {
        return Err(format!(
            "{component}: axon_sha {certified} is not an ancestor of this tree"
        ));
    }
    let (ok, changed) = git(
        repo,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            certified,
            "HEAD",
        ],
    )?;
    // Behind the ancestor check above (so no test reaches it): a git failure
    // here — e.g. a damaged object store — is never read as "no change".
    if !ok {
        return Err(format!(
            "{component}: cannot compare this tree with axon_sha {certified}: nothing unverified \
             is assumed unchanged"
        ));
    }
    // Staged (index vs HEAD) and untracked files. The working tree itself is
    // compared below from the file bytes, not through git's porcelain view.
    let (ok_staged, staged) = git(
        repo,
        &[
            "diff-index",
            "--cached",
            "--no-renames",
            "--name-only",
            "-z",
            "HEAD",
        ],
    )?;
    let (ok_untracked, untracked) =
        git(repo, &["ls-files", "-z", "--others", "--exclude-standard"])?;
    if !ok_staged || !ok_untracked {
        return Err(format!(
            "{component}: cannot read the index or the working tree: nothing unverified is \
             assumed unchanged"
        ));
    }
    let mut outside: Vec<String> = [changed, staged, untracked]
        .iter()
        .flat_map(|l| l.split('\0').map(str::to_string).collect::<Vec<_>>())
        .filter(|f| !f.is_empty() && !f.starts_with("governance/"))
        .collect();
    // The same question answered from objects verified by hash: the certified
    // tree (named by the signed axon_sha), HEAD's tree, and the working-tree
    // bytes, outside governance/.
    if outside.is_empty() {
        let (ok, head) = git(repo, &["rev-parse", "--verify", "HEAD^{commit}"])?;
        if !ok {
            return Err(format!("{component}: this tree has no HEAD commit"));
        }
        let top = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
        let mut objects = Objects::open(&top)?;
        let want = objects
            .entries(certified, Some(b"governance"))
            .map_err(|e| format!("{component}: {e}"))?;
        let have = objects
            .entries(&head, Some(b"governance"))
            .map_err(|e| format!("{component}: {e}"))?;
        outside.extend(
            want.keys()
                .chain(have.keys())
                .filter(|p| want.get(*p) != have.get(*p))
                .map(|p| String::from_utf8_lossy(p).to_string()),
        );
        #[cfg(unix)]
        outside.extend(worktree_differs(repo, &want));
        #[cfg(not(unix))]
        return Err(format!(
            "{component}: the working tree cannot be verified on this platform"
        ));
    }
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
    // Each evidence file is read ONCE: the digest bound into the bundle and
    // the bytes parsed below (preflight, observation, B263 record) are one
    // buffer.
    let mut concat = String::new();
    let mut evidence: Vec<Evidence> = Vec::new();
    for e in ev {
        let p = e
            .as_str()
            .ok_or(format!("{component}: evidence entries are paths"))?;
        let b = read_once(&repo.join(p)).map_err(|e| format!("{component}: evidence {e}"))?;
        let h = sha256_hex(&b);
        concat.push_str(&h);
        evidence.push((repo.join(p), h, b));
    }
    // The executable trust-root preflight (real write attempts under the
    // service UIDs, scripts/trust_root_preflight.sh) is part of the certified
    // evidence, and must be a PROTECTED-mode run that passed. A dev-mode run
    // proves the mechanism and certifies nothing.
    let (_, _, pf) = named(&evidence, component, &doc, "trust_preflight_sha256")?;
    let pf: Value = serde_json::from_slice(pf)
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
    // Authority: the operator's roots, never the repository.
    trust.check()?;
    attribution(component, &doc, trust, &evidence)?;
    let issuer = axon_loop_contracts::operator_trust::verify_evidence_signature(
        "evidence",
        &rec_bytes,
        &crate::backend::read_signature("evidence", &sidecar(&rec))?,
        &trust.keys(&trust.issuers_dir)?,
        TrustAuthority::Qualification,
    )?;
    let exp = repo.join(TRUST_EXPECTATIONS);
    // Only a MISSING list means "no narrowing". `exists()` also answers false
    // for a dangling symlink or any stat error, which would silently drop the
    // narrowing; anything else that is present is read (and a non-regular
    // file refused) by `read_once`.
    let narrowed = match std::fs::symlink_metadata(&exp) {
        Ok(_) => true,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
        Err(e) => return Err(format!("{TRUST_EXPECTATIONS}: {e}")),
    };
    if narrowed {
        let v: Value = serde_json::from_slice(&read_once(&exp)?)
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

fn sidecar(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

/// A certified evidence file, read once: (path, sha256, bytes).
type Evidence = (PathBuf, String, Vec<u8>);

/// The certified evidence file whose digest is the record's `field`.
fn named<'a>(
    evidence: &'a [Evidence],
    component: &str,
    doc: &Value,
    field: &str,
) -> Result<&'a Evidence, String> {
    evidence
        .iter()
        .find(|(_, h, _)| doc[field].as_str() == Some(h.as_str()))
        .ok_or(format!(
            "{component}: {field} names no certified evidence file"
        ))
}

/// The record's ATTRIBUTION, joined to the operator's roots and to the
/// certified evidence at decision time (review PSV-7 / FIELD-ORIGIN, class
/// c). The record says who observed the protected run, who verified it, which
/// B263 qualification it ran under and which guest; each of those statements
/// must be true of something this verifier can check, not merely well-formed:
///
/// * `observer_key_id` and `verifier_key_id` are keys in the operator's
///   observer and verifier roots NOW;
/// * `observation_sha256` names a certified evidence file whose detached
///   OBSERVER-domain signature verifies under the observer root, signed by
///   `observer_key_id`, observing this profile, this `fabric_revision` and
///   these guest digests;
/// * `b263_qualification_sha256` names a certified evidence file that is a
///   QUALIFICATION-signed `axon-b263-evidence/1` record of this profile, whose
///   qualified artifacts are the certified guest (`vmlinux` = kernel,
///   `rootfs.sqfs` = image, `axon` = runtime).
fn attribution(
    component: &str,
    doc: &Value,
    trust: &ReadinessTrust,
    evidence: &[Evidence],
) -> Result<(), String> {
    use axon_loop_contracts::operator_trust::verify_evidence_signature;
    let s = |k: &str| doc[k].as_str().unwrap_or("");
    for (field, dir, root) in [
        ("observer_key_id", &trust.observer_dir, "observer"),
        ("verifier_key_id", &trust.verifier_dir, "verifier"),
    ] {
        if !trust.key_ids(dir)?.iter().any(|k| k == s(field)) {
            return Err(format!(
                "{component}: {field} {} is not a key in the operator's {root} root ({}): the \
                 record names who authenticated the run, and that must be a key the operator \
                 trusts for it",
                s(field),
                dir.display()
            ));
        }
    }
    let guest = [
        ("guest_kernel_sha256", "vmlinux"),
        ("guest_image_sha256", "rootfs.sqfs"),
        ("guest_runtime_sha256", "axon"),
    ];

    // The observation: signed by the named observer key, under the observer
    // root, and observing what the record certifies.
    let (obs_path, _, obs) = named(evidence, component, doc, "observation_sha256")?;
    let signer = verify_evidence_signature(
        "observation",
        obs,
        &crate::backend::read_signature("observation", &sidecar(obs_path))?,
        &trust.keys(&trust.observer_dir)?,
        TrustAuthority::Observer,
    )
    .map_err(|e| format!("{component}: {e}"))?;
    if signer != s("observer_key_id") {
        return Err(format!(
            "{component}: the observation is signed by {signer}, not the certified \
             observer_key_id {}",
            s("observer_key_id")
        ));
    }
    let o: axon_psv::PreflightObservation = serde_json::from_slice(obs)
        .map_err(|e| format!("{component}: the observation is malformed: {e}"))?;
    let observed = [
        ("host_profile", o.host_profile.as_str(), PROTECTED_PROFILE),
        ("fabric_revision", &o.fabric_revision, s("fabric_revision")),
        (
            "guest_kernel_sha256",
            &o.guest.kernel_sha256,
            s("guest_kernel_sha256"),
        ),
        (
            "guest_image_sha256",
            &o.guest.rootfs_sha256,
            s("guest_image_sha256"),
        ),
        (
            "guest_runtime_sha256",
            &o.guest.axon_sha256,
            s("guest_runtime_sha256"),
        ),
    ];
    if let Some((k, got, want)) = observed.iter().find(|(_, got, want)| got != want) {
        return Err(format!(
            "{component}: the observation records {k} {got}, but the record certifies {want}"
        ));
    }

    // The B263 qualification: operator-signed, of this profile, and of this
    // guest.
    let (b_path, _, b) = named(evidence, component, doc, "b263_qualification_sha256")?;
    verify_evidence_signature(
        "B263 qualification record",
        b,
        &crate::backend::read_signature("B263 qualification record", &sidecar(b_path))?,
        &trust.keys(&trust.issuers_dir)?,
        TrustAuthority::Qualification,
    )
    .map_err(|e| format!("{component}: {e}"))?;
    let q: Value = serde_json::from_slice(b)
        .map_err(|e| format!("{component}: the B263 qualification record: {e}"))?;
    if q["schema"] != "axon-b263-evidence/1" || q["profile"]["name"] != PROTECTED_PROFILE {
        return Err(format!(
            "{component}: b263_qualification_sha256 does not name an axon-b263-evidence/1 \
             record of {PROTECTED_PROFILE}"
        ));
    }
    for (k, artifact) in guest {
        let qualified = q["profile"]["artifacts"][artifact]["sha256"].as_str();
        if qualified != Some(s(k)) {
            return Err(format!(
                "{component}: {k} {} is not the B263-qualified {artifact} ({})",
                s(k),
                qualified.unwrap_or("absent")
            ));
        }
    }
    Ok(())
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
