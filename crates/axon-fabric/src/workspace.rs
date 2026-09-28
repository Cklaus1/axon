//! B261 — WorkspaceVersion: one content identity for a workspace TREE, an
//! immutable content-addressed store for it, a safe materializer, and fresh
//! per-trial cache directories.
//!
//! # Identity (G8)
//!
//! The reference is MiCode's `axon.workspace-version/1` recipe
//! (`docs/axon-support/WORKSPACE_VERSION_RECIPE.md`), byte for byte: a
//! manifest of `<mode> <size> <sha256> <path>\n` lines sorted by the path's
//! UTF-8 bytes, and `acf1:` over `{"manifest_sha256","schema"}`. The byte
//! recipe itself is `axon_cortex::runner::{workspace_manifest_bytes,
//! workspace_version_ref}` — one implementation for both sides of the
//! cortex → fabric seam (as with `acf1_canonical_bytes`, D-C3). The pinned
//! cross-language vector is `tests/fixtures/workspace_version_vector.json`.
//!
//! The older single-file digest (`submit::workspace_digest`,
//! `{"path","sha256"}`) is kept so historical refs still resolve.
//!
//! # Import refusals (the whole tree is refused — never truncated)
//!
//! Every class in the recipe's §2 (traversal, absolute, escaping symlink,
//! special file, non-UTF-8, control character, duplicate, quota entries /
//! bytes / depth — D11: 20 000 / 256 MiB / 32), PLUS one class the recipe
//! does not have: a **namespace collision** — two paths that name the same
//! file on a case-insensitive or normalizing filesystem (equal after Unicode
//! NFC + lowercase), or a path used both as a file and as a directory. Such a
//! tree has a well-defined recipe digest but cannot be materialized
//! faithfully everywhere, so Axon refuses to IMPORT it. The digest of every
//! tree Axon does accept is exactly the recipe's.
//!
//! Top-level `.git` and `.micode` are skipped as the recipe says; Axon
//! RECORDS each skip as an explicit [`Omission`] rather than dropping it
//! silently. Omissions travel with the stored version and with a
//! [`WorkspaceProjection`]; they are not part of the reference (the recipe's
//! preimage is the manifest alone).
//!
//! # Store
//!
//! Per tenant: `<state>/tenants/<key>/workspaces/blobs/<sha256>` and
//! `…/workspaces/versions/<hex>.manifest` plus one content-addressed record
//! per distinct omission set, `…/versions/<hex>.omissions/<sha256>.json`
//! (see [`WorkspaceStore::publish`] for why it is a set). No blob or
//! version is shared across tenants. Every file
//! is written in full to a temp name, fsynced, and PUBLISHED by a no-clobber
//! rename (`renameat2(RENAME_NOREPLACE)`; a `link`+`unlink` fallback where
//! that is unavailable): an existing object is never overwritten, and a
//! concurrent publish of the same bytes is verified, not replaced.
//!
//! # G28 — a workspace is not its context
//!
//! A [`WorkspaceProjection`] joins a scoped observation (`snapshot_ref`, a
//! hash) to a durable version (`version_ref`) with explicit omissions and an
//! observation time. Only a published version can be materialized: a
//! projection with no `version_ref` — a hash-only observation — is refused,
//! and so is a reference the store does not hold (a hash is not content).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axon_cortex::runner::{
    workspace_manifest_bytes, workspace_version_ref, WorkspaceManifestEntry,
};
// The recipe's per-entry rules, quota, entry types and walker are shared with
// the protected guest's verdict runner: one implementation (§4).
use axon_loop_contracts::{Acf1Ref, TenantId, TrialId};
use axon_workspace_recipe::{check_link, check_path, is_exec, walk_tree};
pub use axon_workspace_recipe::{
    EntryKind, ImportRefusal, Omission, Quota, TreeEntry, MAX_BYTES, MAX_DEPTH, MAX_ENTRIES,
    MODE_EXEC, MODE_FILE, MODE_LINK, SKIPPED_TOP_LEVEL,
};
use serde::{Deserialize, Serialize};

/// A validated tree: entries sorted by path bytes, plus explicit omissions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceTree {
    entries: Vec<TreeEntry>,
    omissions: Vec<Omission>,
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// The collision key: NFC, then Unicode lowercase.
fn fold(s: &str) -> String {
    icu_normalizer::ComposingNormalizerBorrowed::new_nfc()
        .normalize(s)
        .to_lowercase()
}

impl WorkspaceTree {
    /// Validate an explicit entry list (recipe §2, plus collisions).
    pub fn from_entries(
        mut entries: Vec<TreeEntry>,
        omissions: Vec<Omission>,
        quota: &Quota,
    ) -> Result<WorkspaceTree, ImportRefusal> {
        if entries.len() > quota.entries {
            return Err(ImportRefusal::QuotaEntries {
                limit: quota.entries,
            });
        }
        let mut bytes: u64 = 0;
        for e in &entries {
            check_path(&e.path, quota)?;
            if e.kind == EntryKind::Symlink {
                check_link(&e.path, &e.content)?;
            }
            bytes = bytes.saturating_add(e.content.len() as u64);
            if bytes > quota.bytes {
                return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });
            }
        }
        entries.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
        if let Some(w) = entries.windows(2).find(|w| w[0].path == w[1].path) {
            return Err(ImportRefusal::Duplicate(w[0].path.clone()));
        }
        // Namespace: every path AND every directory prefix of it, folded.
        // Two different spellings with one key collide; so does an entry that
        // is also some other entry's directory.
        let mut seen: BTreeMap<String, (String, bool)> = BTreeMap::new();
        for e in &entries {
            let comps: Vec<&str> = e.path.split('/').collect();
            for i in 1..=comps.len() {
                let spelled = comps[..i].join("/");
                let is_dir = i < comps.len();
                let key = fold(&spelled);
                match seen.get(&key) {
                    Some((prev, prev_dir)) if *prev != spelled || *prev_dir != is_dir => {
                        return Err(ImportRefusal::Collision {
                            a: prev.clone(),
                            b: spelled,
                        })
                    }
                    Some(_) => {}
                    None => {
                        seen.insert(key, (spelled, is_dir));
                    }
                }
            }
        }
        let mut omissions = omissions;
        omissions.sort();
        Ok(WorkspaceTree { entries, omissions })
    }

    /// Walk `root` (never following a symlink) and validate it.
    pub fn import_dir(root: &Path, quota: &Quota) -> Result<WorkspaceTree, ImportRefusal> {
        let (entries, omissions) = walk_tree(root, quota)?;
        WorkspaceTree::from_entries(entries, omissions, quota)
    }

    /// Only the regular file at `rel` of `root` (a single-file check's
    /// artifact). A symlink or anything else there is refused.
    pub fn import_file(
        root: &Path,
        rel: &str,
        quota: &Quota,
    ) -> Result<WorkspaceTree, ImportRefusal> {
        check_path(rel, quota)?;
        let p = root.join(rel);
        let meta =
            std::fs::symlink_metadata(&p).map_err(|e| ImportRefusal::Io(format!("{rel}: {e}")))?;
        if !meta.file_type().is_file() {
            return Err(ImportRefusal::SpecialFile(rel.into()));
        }
        if meta.len() > quota.bytes {
            return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });
        }
        let content = std::fs::read(&p).map_err(|e| ImportRefusal::Io(format!("{rel}: {e}")))?;
        WorkspaceTree::from_entries(
            vec![TreeEntry {
                path: rel.into(),
                kind: EntryKind::File {
                    executable: is_exec(&meta),
                },
                content,
            }],
            vec![],
            quota,
        )
    }

    pub fn entries(&self) -> &[TreeEntry] {
        &self.entries
    }

    pub fn omissions(&self) -> &[Omission] {
        &self.omissions
    }

    pub fn manifest_entries(&self) -> Vec<WorkspaceManifestEntry> {
        self.entries
            .iter()
            .map(|e| WorkspaceManifestEntry {
                path: e.path.clone(),
                mode: e.kind.mode(),
                size: e.content.len() as u64,
                sha256: sha256_hex(&e.content),
            })
            .collect()
    }

    /// The recipe's manifest bytes.
    pub fn manifest(&self) -> Vec<u8> {
        workspace_manifest_bytes(&self.manifest_entries())
    }

    /// The `acf1:` WorkspaceVersion reference.
    pub fn reference(&self) -> Acf1Ref {
        Acf1Ref::new(workspace_version_ref(&self.manifest())).expect("acf1 + sha256 hex")
    }
}

// ── the content-addressed store ─────────────────────────────────────────────

/// Why a store operation refused.
#[derive(Debug)]
pub enum StoreError {
    /// A projection with no `version_ref`: a hash-only observation.
    HashOnly(String),
    /// The store holds no version with this reference.
    NotPublished(String),
    /// Stored bytes do not hash to their name (tampering or corruption).
    Corrupt(String),
    /// The materialization destination already exists.
    DestinationExists(PathBuf),
    Refused(ImportRefusal),
    Io(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StoreError::HashOnly(s) => write!(
                f,
                "hash-only observation {s} names no WorkspaceVersion; it cannot be materialized"
            ),
            StoreError::NotPublished(r) => {
                write!(
                    f,
                    "no published WorkspaceVersion {r}: a hash is not content"
                )
            }
            StoreError::Corrupt(s) => write!(f, "workspace store corrupt: {s}"),
            StoreError::DestinationExists(p) => {
                write!(f, "materialization target {} already exists", p.display())
            }
            StoreError::Refused(r) => write!(f, "{r}"),
            StoreError::Io(e) => write!(f, "io: {e}"),
        }
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e.to_string())
    }
}

/// A durable version as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceVersion {
    pub reference: Acf1Ref,
    pub manifest: Vec<u8>,
    pub entries: Vec<WorkspaceManifestEntry>,
    pub omissions: Vec<Omission>,
}

/// G28: a scoped observation joined to a durable version by an explicit
/// omission/freshness projection. `version_ref: None` is a hash-only
/// observation — never materializable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkspaceProjection {
    pub snapshot_ref: String,
    pub version_ref: Option<Acf1Ref>,
    pub omissions: Vec<Omission>,
    pub observed_at_ms: u64,
}

/// The immutable, content-addressed workspace store under a Fabric state dir.
#[derive(Debug, Clone)]
pub struct WorkspaceStore {
    root: PathBuf,
}

fn ref_hex(r: &Acf1Ref) -> &str {
    r.as_str().strip_prefix("acf1:").expect("acf1 ref")
}

/// Publish `bytes` at `dest` write-once: temp file, fsync, no-clobber rename.
/// An existing `dest` is verified to hold the same bytes, never replaced.
fn publish_file(dest: &Path, bytes: &[u8]) -> Result<(), StoreError> {
    use std::io::Write;
    if dest.exists() {
        let have = std::fs::read(dest)?;
        if have != bytes {
            return Err(StoreError::Corrupt(format!(
                "{} exists with different bytes",
                dest.display()
            )));
        }
        return Ok(());
    }
    let dir = dest.parent().expect("store paths have a parent");
    std::fs::create_dir_all(dir)?;
    let tmp = dir.join(format!(
        ".tmp-{}-{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    {
        let mut f = std::fs::File::create(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    let r = rename_noreplace(&tmp, dest);
    let _ = std::fs::remove_file(&tmp);
    match r {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            // A concurrent publisher won: same name ⇒ must be same bytes.
            if std::fs::read(dest)? != bytes {
                return Err(StoreError::Corrupt(format!(
                    "{} raced with different bytes",
                    dest.display()
                )));
            }
            Ok(())
        }
        Err(e) => Err(e.into()),
    }
}

static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

#[cfg(target_os = "linux")]
fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let f = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let t = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both are valid NUL-terminated paths; AT_FDCWD is a valid dirfd.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            f.as_ptr(),
            libc::AT_FDCWD,
            t.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    if matches!(e.raw_os_error(), Some(libc::EINVAL) | Some(libc::ENOSYS)) {
        // Filesystem without RENAME_NOREPLACE: link() never clobbers either.
        return std::fs::hard_link(from, to);
    }
    Err(e)
}
#[cfg(not(target_os = "linux"))]
fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to)
}

impl WorkspaceStore {
    /// `tenant`'s store under a Fabric state dir
    /// (`<state>/tenants/<sha256(tenant)[..32]>/workspaces`). Stores are per
    /// TENANT: a reference published by one tenant does not resolve for
    /// another, however it was learned — content addressing is not a
    /// capability.
    pub fn open(state_dir: &Path, tenant: &TenantId) -> Result<WorkspaceStore, StoreError> {
        let root = tenant_dir(state_dir, tenant).join("workspaces");
        std::fs::create_dir_all(root.join("blobs"))?;
        std::fs::create_dir_all(root.join("versions"))?;
        Ok(WorkspaceStore { root })
    }

    fn blob(&self, sha: &str) -> PathBuf {
        self.root.join("blobs").join(sha)
    }
    fn manifest_path(&self, r: &Acf1Ref) -> PathBuf {
        self.root
            .join("versions")
            .join(format!("{}.manifest", ref_hex(r)))
    }
    /// The single omissions file stores written before omissions became a set.
    /// Read (never written) so those stores still load.
    fn legacy_omissions_path(&self, r: &Acf1Ref) -> PathBuf {
        self.root
            .join("versions")
            .join(format!("{}.omissions.json", ref_hex(r)))
    }
    fn omissions_dir(&self, r: &Acf1Ref) -> PathBuf {
        self.root
            .join("versions")
            .join(format!("{}.omissions", ref_hex(r)))
    }

    /// Publish a validated tree. Blobs first, then omissions, then the
    /// manifest — the manifest's appearance IS the publication.
    ///
    /// **Omissions are a SET of observations, not one per version.** They are
    /// not part of the reference (the recipe's preimage is the manifest), so
    /// the same version is legitimately imported with different skip lists:
    /// from a repository (`.git` and `.micode` omitted) and from a materialized
    /// copy (nothing to omit). Stored as one no-clobber file per version, the
    /// second import collided with the first as "exists with different bytes"
    /// and was reported as a CORRUPT store — which made Fabric's post-run
    /// candidate re-check fail for every check whose version came from a real
    /// repository, downgrading a genuine pass to `not_run`. Measured on the
    /// paired interop gate (G3): 2/2 checks passed, receipt said not_run.
    /// Each distinct set is now its own content-addressed record, so a repeat
    /// is idempotent and a different observation is added, never clobbered.
    pub fn publish(&self, tree: &WorkspaceTree) -> Result<Acf1Ref, StoreError> {
        for e in &tree.entries {
            publish_file(&self.blob(&sha256_hex(&e.content)), &e.content)?;
        }
        let r = tree.reference();
        let om = serde_json::to_vec(&tree.omissions).expect("serializable");
        publish_file(
            &self
                .omissions_dir(&r)
                .join(format!("{}.json", sha256_hex(&om))),
            &om,
        )?;
        publish_file(&self.manifest_path(&r), &tree.manifest())?;
        Ok(r)
    }

    /// Every omission any import of `r` recorded: the sorted union of the
    /// stored sets (plus a legacy single file). A version with NO omission
    /// record at all is corrupt — publication writes one before the manifest.
    fn stored_omissions(&self, r: &Acf1Ref) -> Result<Vec<Omission>, StoreError> {
        let mut files = Vec::new();
        let legacy = self.legacy_omissions_path(r);
        if legacy.is_file() {
            files.push(legacy);
        }
        match std::fs::read_dir(self.omissions_dir(r)) {
            Ok(rd) => {
                for e in rd {
                    let p = e?.path();
                    if p.extension().is_some_and(|x| x == "json") {
                        files.push(p);
                    }
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        if files.is_empty() {
            return Err(StoreError::Corrupt(format!("{r} has no omission record")));
        }
        let mut all = Vec::new();
        for f in files {
            let bytes = std::fs::read(&f)?;
            let set: Vec<Omission> = serde_json::from_slice(&bytes)
                .map_err(|e| StoreError::Corrupt(format!("omissions of {r}: {e}")))?;
            // Named by its own digest (legacy file excepted): a record whose
            // bytes do not hash to its name is not the observation it claims.
            if f.parent() == Some(self.omissions_dir(r).as_path())
                && f.file_stem().and_then(|s| s.to_str()) != Some(sha256_hex(&bytes).as_str())
            {
                return Err(StoreError::Corrupt(format!(
                    "omission record {} does not hash to its name",
                    f.display()
                )));
            }
            all.extend(set);
        }
        all.sort();
        all.dedup();
        Ok(all)
    }

    /// Import `root` and publish it.
    pub fn import_dir(&self, root: &Path, quota: &Quota) -> Result<Acf1Ref, StoreError> {
        let t = WorkspaceTree::import_dir(root, quota).map_err(StoreError::Refused)?;
        self.publish(&t)
    }

    pub fn contains(&self, r: &Acf1Ref) -> bool {
        self.manifest_path(r).is_file()
    }

    /// Load a published version, verifying the manifest hashes to its name.
    pub fn load(&self, r: &Acf1Ref) -> Result<WorkspaceVersion, StoreError> {
        let manifest = match std::fs::read(self.manifest_path(r)) {
            Ok(m) => m,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(StoreError::NotPublished(r.to_string()))
            }
            Err(e) => return Err(e.into()),
        };
        if workspace_version_ref(&manifest) != r.as_str() {
            return Err(StoreError::Corrupt(format!(
                "manifest of {r} does not hash to its reference"
            )));
        }
        let entries = parse_manifest(&manifest)
            .ok_or_else(|| StoreError::Corrupt(format!("manifest of {r} is malformed")))?;
        let omissions = self.stored_omissions(r)?;
        Ok(WorkspaceVersion {
            reference: r.clone(),
            manifest,
            entries,
            omissions,
        })
    }

    /// The version's full content, every blob re-verified against its hash.
    pub fn tree(&self, r: &Acf1Ref) -> Result<WorkspaceTree, StoreError> {
        let v = self.load(r)?;
        let mut entries = Vec::with_capacity(v.entries.len());
        for e in &v.entries {
            let content = std::fs::read(self.blob(&e.sha256))
                .map_err(|x| StoreError::Corrupt(format!("blob {}: {x}", e.sha256)))?;
            if sha256_hex(&content) != e.sha256 || content.len() as u64 != e.size {
                return Err(StoreError::Corrupt(format!(
                    "blob {} does not hold its bytes",
                    e.sha256
                )));
            }
            entries.push(TreeEntry {
                path: e.path.clone(),
                kind: EntryKind::from_mode(e.mode).expect("parsed"),
                content,
            });
        }
        // Re-validate: a stored manifest is re-checked like any import.
        let t = WorkspaceTree::from_entries(entries, v.omissions, &Quota::default())
            .map_err(StoreError::Refused)?;
        if t.reference() != *r {
            return Err(StoreError::Corrupt(format!("{r} does not re-derive")));
        }
        Ok(t)
    }

    /// Write version `r` into the NEW directory `dest`. With `read_only`,
    /// files are 0444/0555 and directories 0555 afterwards. (Mode bits are
    /// no boundary for root; callers that need to know a check left its
    /// inputs alone re-import and compare — see `submit`.)
    pub fn materialize(&self, r: &Acf1Ref, dest: &Path, read_only: bool) -> Result<(), StoreError> {
        let t = self.tree(r)?;
        if dest.exists() || std::fs::symlink_metadata(dest).is_ok() {
            return Err(StoreError::DestinationExists(dest.to_path_buf()));
        }
        std::fs::create_dir_all(dest)?;
        let mut dirs = vec![dest.to_path_buf()];
        for e in &t.entries {
            let p = dest.join(&e.path);
            if let Some(parent) = p.parent() {
                if !parent.exists() {
                    std::fs::create_dir_all(parent)?;
                    let mut d = parent.to_path_buf();
                    while d != dest && !dirs.contains(&d) {
                        dirs.push(d.clone());
                        d = d.parent().expect("under dest").to_path_buf();
                    }
                }
            }
            match e.kind {
                EntryKind::Symlink => {
                    let target = std::str::from_utf8(&e.content).expect("validated");
                    #[cfg(unix)]
                    std::os::unix::fs::symlink(target, &p)?;
                    #[cfg(not(unix))]
                    return Err(StoreError::Io(format!("no symlinks here: {target}")));
                }
                EntryKind::File { executable } => {
                    std::fs::write(&p, &e.content)?;
                    set_mode(
                        &p,
                        match (executable, read_only) {
                            (true, true) => 0o555,
                            (true, false) => 0o755,
                            (false, true) => 0o444,
                            (false, false) => 0o644,
                        },
                    )?;
                }
            }
        }
        if read_only {
            dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
            for d in dirs {
                set_mode(&d, 0o555)?;
            }
        }
        Ok(())
    }

    /// Materialize the version a projection names. A hash-only observation
    /// (`version_ref: None`) or an unpublished reference is refused and
    /// `dest` is not created.
    pub fn materialize_projection(
        &self,
        p: &WorkspaceProjection,
        dest: &Path,
        read_only: bool,
    ) -> Result<(), StoreError> {
        let r = p
            .version_ref
            .as_ref()
            .ok_or_else(|| StoreError::HashOnly(p.snapshot_ref.clone()))?;
        self.materialize(r, dest, read_only)
    }
}

#[cfg(unix)]
fn set_mode(p: &Path, mode: u32) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
}
#[cfg(not(unix))]
fn set_mode(_p: &Path, _mode: u32) -> std::io::Result<()> {
    Ok(())
}

/// Parse recipe manifest lines back into entries (strict).
fn parse_manifest(m: &[u8]) -> Option<Vec<WorkspaceManifestEntry>> {
    let text = std::str::from_utf8(m).ok()?;
    if text.is_empty() {
        return Some(vec![]);
    }
    let body = text.strip_suffix('\n')?;
    let mut out = Vec::new();
    for line in body.split('\n') {
        let mut it = line.splitn(4, ' ');
        let mode = EntryKind::from_mode(it.next()?)?.mode();
        let size: u64 = it.next()?.parse().ok()?;
        let sha = it.next()?;
        if sha.len() != 64 || !sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            return None;
        }
        out.push(WorkspaceManifestEntry {
            mode,
            size,
            sha256: sha.into(),
            path: it.next()?.into(),
        });
    }
    // Must be exactly the canonical bytes (sorted, no leading zeros …).
    (workspace_manifest_bytes(&out) == m).then_some(out)
}

/// Remove a (possibly read-only) materialized tree.
pub fn remove_tree(p: &Path) -> std::io::Result<()> {
    if std::fs::symlink_metadata(p).is_err() {
        return Ok(());
    }
    fn unlock(p: &Path) {
        if let Ok(m) = std::fs::symlink_metadata(p) {
            if m.is_dir() {
                let _ = set_mode(p, 0o755);
                if let Ok(rd) = std::fs::read_dir(p) {
                    for e in rd.flatten() {
                        unlock(&e.path());
                    }
                }
            }
        }
    }
    unlock(p);
    std::fs::remove_dir_all(p)
}

// ── per-trial caches ────────────────────────────────────────────────────────

/// `<state>/tenants/<sha256(tenant)[..32]>`.
fn tenant_dir(state_dir: &Path, tenant: &TenantId) -> PathBuf {
    state_dir
        .join("tenants")
        .join(&sha256_hex(tenant.as_str().as_bytes())[..32])
}

/// A trial's own mutable cache directories. Keyed by `TrialId`: every
/// attempt of one trial shares them; no two trials ever do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrialCache {
    pub root: PathBuf,
}

impl TrialCache {
    /// `<state>/tenants/<tenant key>/trial-caches/<sha256(trial_id)[..32]>`,
    /// created fresh (0700) on first use. Keys are hashes so no id spelling
    /// can name another trial's — or another tenant's — directory.
    pub fn for_trial(
        state_dir: &Path,
        tenant: &TenantId,
        trial: &TrialId,
    ) -> Result<TrialCache, StoreError> {
        let key = &sha256_hex(trial.as_str().as_bytes())[..32];
        let root = tenant_dir(state_dir, tenant).join("trial-caches").join(key);
        for sub in ["home", "xdg-cache", "cargo-target"] {
            std::fs::create_dir_all(root.join(sub))?;
        }
        set_mode(&root, 0o700)?;
        Ok(TrialCache { root })
    }

    /// The environment a check process of this trial runs with.
    pub fn env(&self) -> Vec<(String, String)> {
        let s = |p: PathBuf| p.to_string_lossy().into_owned();
        vec![
            ("HOME".into(), s(self.root.join("home"))),
            ("XDG_CACHE_HOME".into(), s(self.root.join("xdg-cache"))),
            ("CARGO_TARGET_DIR".into(), s(self.root.join("cargo-target"))),
        ]
    }
}
