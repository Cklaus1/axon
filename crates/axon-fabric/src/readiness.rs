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

use crate::backend::{Clock, TrustAuthority, DEFAULT_EVIDENCE_MAX_AGE_S, TEST_TRUST_BUILD};
use crate::git_data::{load_allowlist, tree_differs, AllowlistSource, Objects, GIT_BIN};
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
    /// The operator's provenance allowlist (decision C): the only thing that
    /// may excuse an object in the working tree that is not in the
    /// certified tree.
    allowlist: AllowlistSource,
    /// Decision time: a B263 qualification is current only if its `end` is
    /// within `max_age_s` of NOW, as Fabric's own launch check requires
    /// (review PSV-7, C9 round 3; A78). Production: the system clock and the
    /// maximum age the operator's host config sets for Fabric (the same
    /// reading, [`crate::protected_host::qualification_max_age_s`]). An Err
    /// here (an unreadable, non-operator-owned or malformed host config)
    /// refuses every certification rather than falling back to a default.
    clock: Clock,
    max_age_s: Result<u64, String>,
}

/// The B263 maximum age the host config at `path` sets. Only a MISSING config
/// means Fabric's default; a present one must pass the operator-ownership walk
/// and is read once, as a regular file.
fn host_max_age_s(base: &Path, path: &Path) -> Result<u64, String> {
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(DEFAULT_EVIDENCE_MAX_AGE_S)
        }
        Err(e) => return Err(format!("{}: {e}", path.display())),
        Ok(_) => {}
    }
    crate::backend::check_owned_from_pub(base, path)?;
    let bytes = crate::backend::read_regular(path)?;
    let v: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    crate::protected_host::qualification_max_age_s(&v)
}

impl ReadinessTrust {
    pub fn operator() -> ReadinessTrust {
        ReadinessTrust {
            issuers_dir: TrustAuthority::Qualification.operator_dir(),
            observer_dir: TrustAuthority::Observer.operator_dir(),
            verifier_dir: TrustAuthority::Verifier.operator_dir(),
            ownership_base: PathBuf::from("/"),
            require_unwritable: true,
            allowlist: AllowlistSource::operator(),
            clock: Clock::System,
            max_age_s: host_max_age_s(
                Path::new("/"),
                Path::new(crate::protected_host::PROTECTED_HOST_CONFIG),
            ),
        }
    }

    /// TESTS ONLY: a temp root (its ancestors are not operator-owned, and the
    /// test runs as its owner). The observer and verifier roots are
    /// `issuers_dir`'s siblings `observer/` and `verifier/`, as under
    /// `/etc/axon/trust/`, and the provenance allowlist is
    /// `provenance-allowlist` beside `trust/`, as `/etc/axon/provenance-allowlist`
    /// is beside `/etc/axon/trust/`. A build with this carries
    /// `TEST_TRUST_BUILD` and reports `build: "test-trust"`, which readiness
    /// never accepts.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn test(base: &Path, issuers_dir: &Path) -> ReadinessTrust {
        let trust = issuers_dir.parent().unwrap_or(base);
        let sib = |n: &str| trust.join(n);
        ReadinessTrust {
            issuers_dir: issuers_dir.to_path_buf(),
            observer_dir: sib("observer"),
            verifier_dir: sib("verifier"),
            ownership_base: base.to_path_buf(),
            require_unwritable: false,
            allowlist: AllowlistSource::test(
                base,
                &trust.parent().unwrap_or(base).join("provenance-allowlist"),
            ),
            clock: Clock::System,
            max_age_s: Ok(DEFAULT_EVIDENCE_MAX_AGE_S),
        }
    }

    /// TESTS ONLY: judge B263 currency with the maximum age the host config
    /// at `path` sets (ownership walked from this trust's `ownership_base`).
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn with_host_config(mut self, path: &Path) -> ReadinessTrust {
        self.max_age_s = host_max_age_s(&self.ownership_base, path);
        self
    }

    /// TESTS ONLY: decide as of `clock`.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn at(mut self, clock: Clock) -> ReadinessTrust {
        self.clock = clock;
        self
    }

    /// Where this trust reads the provenance allowlist from.
    pub fn allowlist_path(&self) -> &Path {
        self.allowlist.path()
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

/// `git args` in `repo`: its stdout, or a refusal naming `component` when
/// git fails. The ONE place a git failure is decided (C9 round 4, rows2): a
/// failed answer is never read as an empty one ("no replace refs", "no
/// change", "no index entry"), whichever question it answered.
fn git(repo: &Path, component: &str, args: &[&str]) -> Result<String, String> {
    let out = git_cmd(repo)?
        .args(args)
        .output()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{component}: git {} failed: nothing unverified is assumed unchanged",
            args.first().copied().unwrap_or("")
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Repository-local git state that makes git report something other than
/// the objects and files that are there. Any of it present: refused, never
/// interpreted.
fn refuse_git_spoofing(repo: &Path, component: &str) -> Result<(), String> {
    // A protected answer comes from a STANDALONE CLONE (operator decision E,
    // amendment 44): `.git` must be a real directory at the top of `repo`. A
    // gitfile (a linked worktree, a submodule) or a symlink names a
    // repository chosen elsewhere, and is refused like build provenance
    // refuses it.
    let top = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    let found = crate::git_data::discover(&top).map_err(|e| format!("{component}: {e}"))?;
    if found != top {
        return Err(format!(
            "{component}: {} is not the top of a standalone clone (its repository is {})",
            top.display(),
            found.display()
        ));
    }
    // The repository's own .git/config: a promisor remote with a
    // core.sshCommand, core.worktree, a filter driver… (review PSV-7, C9
    // round 2). Refused before git is asked anything else.
    // The config of the repository git DISCOVERS from `repo` (`found`): the
    // one every git call below reads. Equal to `top` once the rule above
    // holds; naming `found` keeps each rule the only check of its own fact
    // (C9 round 4, rows2: a missing `.git` at `top` was refused by both).
    crate::git_data::refuse_config(&found).map_err(|e| format!("{component}: {e}"))?;
    let replaced = git(
        repo,
        component,
        &["for-each-ref", "--format=%(refname)", "refs/replace/"],
    )?;
    if let Some(r) = replaced.lines().next() {
        return Err(format!(
            "{component}: the repository has refs/replace/ object replacements (e.g. {r}): git \
             would report another object's content under a certified name, so no certification \
             applies"
        ));
    }
    let grafts = git(repo, component, &["rev-parse", "--git-path", "info/grafts"])?;
    if grafts.is_empty() {
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
    let index = git(repo, component, &["ls-files", "-z", "-v"])?;
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
    // Ancestry from hash-checked objects (the one implementation build and
    // guest provenance use), never git's unverified commit walk.
    let top = std::fs::canonicalize(repo).map_err(|e| format!("{}: {e}", repo.display()))?;
    if let Err(e) = crate::git_data::descends(&top, certified) {
        return Err(format!(
            "{component}: axon_sha {certified} is not an ancestor of this tree ({e})"
        ));
    }
    let changed = git(
        repo,
        component,
        &[
            "diff",
            "--no-renames",
            "--name-only",
            "-z",
            certified,
            "HEAD",
        ],
    )?;
    // Staged (index vs HEAD) and untracked files. The working tree itself is
    // compared below from the file bytes, not through git's porcelain view.
    let staged = git(
        repo,
        component,
        &[
            "diff-index",
            "--cached",
            "--no-renames",
            "--name-only",
            "-z",
            "HEAD",
        ],
    )?;
    let untracked = git(
        repo,
        component,
        &["ls-files", "-z", "--others", "--exclude-standard"],
    )?;
    let mut outside: Vec<String> = [changed, staged, untracked]
        .iter()
        .flat_map(|l| l.split('\0').map(str::to_string).collect::<Vec<_>>())
        .filter(|f| !f.is_empty() && !f.starts_with("governance/"))
        .collect();
    // The same question answered from objects verified by hash: the certified
    // tree (named by the signed axon_sha), HEAD's tree, and the working tree
    // AS A FILESYSTEM outside governance/ (decision C, amendment 44): every
    // object counts, git-ignore rules excuse nothing, and only the operator's
    // provenance allowlist excuses generated material.
    if outside.is_empty() {
        let head = git(repo, component, &["rev-parse", "--verify", "HEAD^{commit}"])?;
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
        let allow = load_allowlist(&trust.allowlist);
        outside.extend(tree_differs(&top, &want, Some(b"governance"), &allow));
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
///   `rootfs.sqfs` = image, `axon` = runtime), and that is a CURRENT
///   qualification by Fabric's own rules ([`crate::backend::accept_b263`]:
///   PASS, no FAIL, fresh at decision time, clean tree, a host, engine
///   digests), whose firecracker is the observed one;
/// * the run was observed no later than `certified_at`, which is not in the
///   future, and the B263 record was ALSO current at `observed_at` (issued
///   before the run, within the maximum age of it);
/// * the run itself is in the certified evidence and joined to every other
///   attribution field ([`launched`]).
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
    let (b_path, b_sha, b) = named(evidence, component, doc, "b263_qualification_sha256")?;
    let b_issuer = verify_evidence_signature(
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
    // ...and a CURRENT qualification by the rules Fabric launches under, at
    // decision time (review PSV-7, C9 round 3; A78): the ONE implementation,
    // [`crate::backend::accept_b263`]. Its waivers are certified evidence
    // too: a qualification-signed waiver file bound to this record.
    let now = trust.clock.now_unix();
    let max_age_s = trust
        .max_age_s
        .clone()
        .map_err(|e| format!("{component}: the host config's qualification maximum age: {e}"))?;
    let b263 = crate::backend::accept_b263(&q, &b_issuer, now, max_age_s, || {
        certified_waivers(component, evidence, b_sha, trust)
    })
    .map_err(|e| {
        format!("{component}: the B263 record is not a current qualification Fabric launches under: {e}")
    })?;
    // The observed launch ran the engine that was qualified.
    if o.firecracker_sha256 != b263.firecracker_sha256 {
        return Err(format!(
            "{component}: the observation records firecracker {}, but the B263 qualification \
             qualified {}",
            o.firecracker_sha256, b263.firecracker_sha256
        ));
    }
    // Time: the run was observed before it was certified, and the
    // certification is not from the future.
    let certified_at = crate::backend::parse_utc(s("certified_at")).ok_or(format!(
        "{component}: certified_at {:?} is not a YYYY-MM-DDTHH:MM:SSZ time",
        s("certified_at")
    ))?;
    let observed_at = crate::backend::parse_utc(&o.observed_at).ok_or(format!(
        "{component}: the observation's observed_at {:?} is not a YYYY-MM-DDTHH:MM:SSZ time",
        o.observed_at
    ))?;
    if observed_at > certified_at {
        return Err(format!(
            "{component}: the run was observed at {} but certified at {}: a certification \
             cannot precede what it certifies",
            o.observed_at,
            s("certified_at")
        ));
    }
    // ...and the B263 record was a CURRENT qualification WHEN THE RUN WAS
    // OBSERVED, not only now (C9 round 4, PSV-7; A89): issued (`end`) no
    // later than observed_at and within the maximum age of it, by the same
    // rules ([`crate::backend::accept_b263`]). A record issued after the run
    // qualified nothing that ran.
    crate::backend::accept_b263(&q, &b_issuer, observed_at, max_age_s, || {
        certified_waivers(component, evidence, b_sha, trust)
    })
    .map_err(|e| {
        format!(
            "{component}: the B263 record was not a current qualification when the run was \
             observed ({}): {e}",
            o.observed_at
        )
    })?;
    if certified_at > now {
        return Err(format!(
            "{component}: certified_at {} is in the future",
            s("certified_at")
        ));
    }
    launched(component, doc, trust, evidence, &o)
}

/// The schema of `axon-fabric submit`'s output: the attested receipt, its
/// `acf-receipt-attestation/2` and the run's `axon-psv-evidence/2` bundle.
pub const RUN_OUTPUT_SCHEMA: &str = "axon-fabric-submit/1";
/// The schema of the compute request that run answered.
pub const REQUEST_SCHEMA: &str = "acf-compute-request/1";

/// The ONE certified evidence file whose schema is `schema`. None, or more
/// than one, is refused: which document the run was is never guessed.
fn the_one<'a>(
    evidence: &'a [Evidence],
    component: &str,
    schema: &str,
    what: &str,
) -> Result<&'a [u8], String> {
    let found: Vec<&Evidence> = evidence
        .iter()
        .filter(|(_, _, b)| serde_json::from_slice::<Value>(b).is_ok_and(|v| v["schema"] == schema))
        .collect();
    match found.as_slice() {
        [(_, _, b)] => Ok(b),
        [] => Err(format!(
            "{component}: the certified evidence carries no {what} ({schema}): the record's \
             run attribution has no document to be joined to"
        )),
        many => Err(format!(
            "{component}: the certified evidence carries {} {what}s ({schema}): which one was \
             the run is not stated",
            many.len()
        )),
    }
}

/// The value of the receipt's ONE evidence ref starting `prefix`.
fn one_ref<'a>(rc: &'a axon_loop_contracts::ExecutionReceipt, prefix: &str) -> Option<&'a str> {
    let mut it = rc
        .evidence_refs
        .iter()
        .filter_map(|e| e.as_str().strip_prefix(prefix));
    match (it.next(), it.next()) {
        (Some(v), None) => Some(v),
        _ => None,
    }
}

/// The record's RUN attribution, joined to VERIFIED documents (C9 round 4,
/// PSV-7 / FIELD-ORIGIN class c; A88, A89). The certified evidence must carry
/// the run itself: Fabric's `axon-fabric-submit/1` output (the receipt, its
/// `acf-receipt-attestation/2`, and the `axon-psv-evidence/2` bundle holding
/// the exact launch manifest) and the `acf-compute-request/1` it answered.
/// Then:
///
/// * `verifier_key_id` is the key that SIGNED the receipt attestation: the
///   verifier root's key under that id verifies it over this request and
///   receipt ([`axon_loop_contracts::attestation::verify`]);
/// * that receipt is protected evidence
///   ([`axon_loop_contracts::protected_evidence::check`]) naming THIS launch:
///   the bundle's manifest, the certified observation (`observation_sha256`),
///   the bundle's guest verdict and the manifest's qualification, with the
///   manifest's operation, task, trial, attempt, candidate and test;
/// * the certified observation joins that manifest field for field
///   ([`axon_psv::PreflightObservation::joins`]: its
///   `intended_launch_manifest_sha256`, nonce, verifier, host config, suite
///   registry, policy, launcher, engine and guest);
/// * the record's `b263_qualification_sha256` is the manifest's
///   `qualification_sha256`: the B263 record the observed launch RAN UNDER
///   (Fabric computed it from the record its operator host config pins), not
///   another current one — for another host, or issued later;
/// * the record's `suite` (id, version, entry, test, digest = tree digest)
///   and `candidate_tree_ref` are the manifest's.
///
/// `micode_sha` stays operator-attested: no document a protected run
/// produces carries a MiCode revision.
fn launched(
    component: &str,
    doc: &Value,
    trust: &ReadinessTrust,
    evidence: &[Evidence],
    o: &axon_psv::PreflightObservation,
) -> Result<(), String> {
    use axon_loop_contracts::{attestation, protected_evidence, OpaqueRef};
    let s = |k: &str| doc[k].as_str().unwrap_or("");
    let run: Value = serde_json::from_slice(the_one(
        evidence,
        component,
        RUN_OUTPUT_SCHEMA,
        "Fabric run output",
    )?)
    .map_err(|e| format!("{component}: the certified run output: {e}"))?;
    let req: axon_loop_contracts::ComputeRequest = axon_loop_contracts::parse_bytes(the_one(
        evidence,
        component,
        REQUEST_SCHEMA,
        "compute request",
    )?)
    .map_err(|e| format!("{component}: the certified compute request: {e}"))?;
    let rc: axon_loop_contracts::ExecutionReceipt =
        axon_loop_contracts::parse(&run["receipt"].to_string())
            .map_err(|e| format!("{component}: the certified run's receipt: {e}"))?;

    // verifier_key_id: the verifier root's key under that id signed the
    // attestation of THIS receipt answering THIS request.
    let key = trust
        .exclusive_keys(&trust.verifier_dir)?
        .into_iter()
        .find(|k| attestation::key_id_of_hex(k).as_deref() == Some(s("verifier_key_id")))
        .ok_or(format!(
            "{component}: verifier_key_id {} names no key in the operator's verifier root",
            s("verifier_key_id")
        ))?;
    let att = &run["receipt_attestation"];
    let issuer = OpaqueRef::new(att["issuer_ref"].as_str().unwrap_or(""))
        .map_err(|e| format!("{component}: the receipt attestation names no issuer: {e}"))?;
    attestation::verify(att, &issuer, &req, &rc, &key).map_err(|e| {
        format!(
            "{component}: the certified receipt attestation is not verifier_key_id {}'s \
             attestation of the certified receipt: {e}",
            s("verifier_key_id")
        )
    })?;
    protected_evidence::check(&req, &rc)
        .map_err(|e| format!("{component}: the attested receipt is not protected evidence: {e}"))?;

    // The launch manifest, as the run's bundle carries it.
    let b: protected_evidence::PsvEvidence = serde_json::from_value(run["psv_evidence"].clone())
        .map_err(|e| {
            format!("{component}: the certified run carries no psv evidence bundle: {e}")
        })?;
    if b.schema != protected_evidence::PSV_EVIDENCE_SCHEMA {
        return Err(format!(
            "{component}: the run's evidence bundle is not {}",
            protected_evidence::PSV_EVIDENCE_SCHEMA
        ));
    }
    let m_sha = sha256_hex(b.launch_manifest.as_bytes());
    let m = axon_psv::LaunchManifest::verify(b.launch_manifest.as_bytes(), &m_sha)
        .map_err(|e| format!("{component}: the run's launch manifest: {e}"))?;

    // The attested receipt names this launch's documents...
    let v_sha = sha256_hex(b.guest_verdict.as_bytes());
    let refs = [
        ("launch-manifest-sha256:", m_sha.as_str()),
        ("preflight-observation-sha256:", s("observation_sha256")),
        ("guest-verdict-sha256:", v_sha.as_str()),
        ("qualification-sha256:", m.qualification_sha256.as_str()),
    ];
    if let Some((p, want)) = refs.iter().find(|(p, want)| one_ref(&rc, p) != Some(*want)) {
        return Err(format!(
            "{component}: the attested receipt names {p}{}, but the certified run's is {want}",
            one_ref(&rc, p).unwrap_or("(none, or more than one)")
        ));
    }
    // ...and is the manifest's request, trial and candidate.
    let ids = [
        ("operation_id", &m.operation_id, req.operation_id.as_str()),
        ("task_id", &m.task_id, req.task_id.as_str()),
        ("trial_id", &m.trial_id, req.trial_id.as_str()),
        ("attempt_id", &m.attempt_id, req.attempt_id.as_str()),
        (
            "operation_id (receipt)",
            &m.operation_id,
            rc.operation_id.as_str(),
        ),
        ("task_id (receipt)", &m.task_id, rc.task_id.as_str()),
        ("trial_id (receipt)", &m.trial_id, rc.trial_id.as_str()),
        (
            "attempt_id (receipt)",
            &m.attempt_id,
            rc.attempt_id.as_str(),
        ),
        (
            "candidate",
            &m.candidate.workspace_version,
            req.workspace_version_ref.as_str(),
        ),
        (
            "candidate (receipt)",
            &m.candidate.workspace_version,
            rc.input_workspace_ref.as_str(),
        ),
        (
            "test",
            &m.suite.test,
            req.argv.get(1).map(String::as_str).unwrap_or(""),
        ),
    ];
    if let Some((k, manifest, other)) = ids.iter().find(|(_, a, b)| a.as_str() != *b) {
        return Err(format!(
            "{component}: the launch manifest's {k} is {manifest}, but the attested \
             request/receipt names {other}"
        ));
    }

    // The certified observation is of THIS manifest, field for field.
    o.joins(&m, &m_sha).map_err(|e| {
        format!("{component}: the certified observation is not of the run's launch: {e}")
    })?;

    // The record's attribution is the launch's.
    if m.qualification_sha256 != s("b263_qualification_sha256") {
        return Err(format!(
            "{component}: the observed launch ran under B263 qualification {}, but the record \
             certifies {}",
            m.qualification_sha256,
            s("b263_qualification_sha256")
        ));
    }
    let suite = [
        ("id", &m.suite.id),
        ("version", &m.suite.version),
        ("entry", &m.suite.entry),
        ("test", &m.suite.test),
        ("digest", &m.suite.tree_digest),
    ];
    if let Some((k, launched)) = suite
        .iter()
        .find(|(k, launched)| doc["suite"][k].as_str() != Some(launched.as_str()))
    {
        return Err(format!(
            "{component}: the record's suite {k} is {}, but the observed launch ran {launched}",
            doc["suite"][k]
        ));
    }
    if s("candidate_tree_ref") != m.candidate.tree_digest {
        return Err(format!(
            "{component}: the record's candidate_tree_ref is {}, but the observed launch ran \
             candidate {}",
            s("candidate_tree_ref"),
            m.candidate.tree_digest
        ));
    }
    Ok(())
}

/// The waivers among the certified evidence for the B263 record whose sha256
/// is `b263_sha`: every `axon-b263-waiver/1` file, each verified under the
/// operator's qualification root and bound to that record (Fabric's
/// RULE:waiver-bound). None ⇒ no waiver.
fn certified_waivers(
    component: &str,
    evidence: &[Evidence],
    b263_sha: &str,
    trust: &ReadinessTrust,
) -> Result<crate::backend::Waivers, String> {
    let mut all = crate::backend::Waivers::new();
    for (p, _, bytes) in evidence {
        let is_waiver = serde_json::from_slice::<Value>(bytes)
            .is_ok_and(|w| w["schema"] == crate::backend::WAIVER_SCHEMA);
        if !is_waiver {
            continue;
        }
        axon_loop_contracts::operator_trust::verify_evidence_signature(
            "waiver file",
            bytes,
            &crate::backend::read_signature("waiver file", &sidecar(p))?,
            &trust.keys(&trust.issuers_dir)?,
            TrustAuthority::Qualification,
        )
        .map_err(|e| format!("{component}: {e}"))?;
        all.extend(crate::backend::parse_waivers(bytes, b263_sha)?);
    }
    Ok(all)
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

#[cfg(test)]
mod tests {
    use super::*;

    /// C9 round 2 (harness): a trust root the process running readiness can
    /// WRITE authorizes nothing. Production requires it (`operator()`), and
    /// `check()` enforces it through `writable_by_me` over the root, its
    /// ancestors and its entries. Here the roots are root-owned and 0700, so
    /// every OTHER trust check passes and only the writability check refuses:
    /// the process that made them (the test, as root) can write them.
    #[cfg(unix)]
    #[test]
    fn a_trust_root_this_process_can_write_authorizes_nothing() {
        assert!(
            ReadinessTrust::operator().require_unwritable,
            "ATTACK: the production readiness trust accepts a trust root the verifier can write"
        );
        if unsafe { libc::geteuid() } != 0 {
            eprintln!("skipped: the trust roots must be root-owned");
            return;
        }
        let d = tempfile::tempdir().unwrap();
        let issuers = d.path().join("qualification");
        for r in ["qualification", "observer", "verifier"] {
            std::fs::create_dir(d.path().join(r)).unwrap();
        }
        let mut t = ReadinessTrust::test(d.path(), &issuers);
        t.check()
            .expect("control: the same roots pass every other trust check");
        t.require_unwritable = true;
        let got = t.check();
        assert!(
            got.is_err(),
            "ATTACK: a trust root writable by the process running readiness authorized: {got:?}"
        );
    }
}
