//! The WorkspaceVersion byte recipe and tree walker — ONE implementation for
//! every side that computes a tree digest:
//! * the host: `axon-cortex` re-exports it, and `axon-fabric`'s importer walks
//!   with it;
//! * the protected guest's verdict runner (`axon-psv-runner`, in `axon-psv`;
//!   `axon-guest-init` does not link this crate), which re-digests its
//!   read-only candidate and suite inputs before executing anything
//!   (`governance/specs/v022-psv-protocol.md` §4).
//!
//! Recipe: MiCode `docs/axon-support/WORKSPACE_VERSION_RECIPE.md`, byte for
//! byte. The cross-language vector is
//! `crates/axon-fabric/tests/fixtures/workspace_version_vector.json`.
//! Dependencies are deliberately minimal (sha2, serde), so the static musl
//! guest can link it.

use serde::{Deserialize, Serialize};
use std::path::Path;

pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(bytes))
}

/// THE `acf1:` canonicaliser for the Fabric's flat identity objects — the one
/// implementation both sides of the cortex → fabric process seam use
/// (`axon_fabric::submit::{executable_digest, workspace_digest}` delegate
/// here; D-C3). It lives in this crate because it is the lowest one both
/// sides link: `axon-fabric` depends on `axon-cortex`, never the reverse.
///
/// Bytes: a JSON object of string values with keys sorted by code point, no
/// whitespace, strings escaped by the `cl22` rule (Python
/// `json.dumps(sort_keys=True, separators=(',',':'), ensure_ascii=False)`):
/// `"` `\` and C0 controls escaped (`\b \f \n \r \t` short, the rest
/// `\u00xx` lowercase), everything else — DEL and non-ASCII included — raw.
/// Key order is SORTED HERE, not inherited from a map type, so enabling
/// serde_json's `preserve_order` anywhere in the build cannot change a digest.
/// A duplicate key is a caller bug and panics.
pub fn acf1_canonical_bytes(fields: &[(&str, &str)]) -> Vec<u8> {
    fn string(s: &str, out: &mut Vec<u8>) {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        out.push(b'"');
        for &b in s.as_bytes() {
            match b {
                b'"' => out.extend_from_slice(b"\\\""),
                b'\\' => out.extend_from_slice(b"\\\\"),
                b'\n' => out.extend_from_slice(b"\\n"),
                b'\r' => out.extend_from_slice(b"\\r"),
                b'\t' => out.extend_from_slice(b"\\t"),
                0x08 => out.extend_from_slice(b"\\b"),
                0x0c => out.extend_from_slice(b"\\f"),
                0x00..=0x1f => {
                    out.extend_from_slice(b"\\u00");
                    out.push(HEX[(b >> 4) as usize]);
                    out.push(HEX[(b & 0xf) as usize]);
                }
                _ => out.push(b),
            }
        }
        out.push(b'"');
    }
    let mut sorted: Vec<&(&str, &str)> = fields.iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(b.0));
    assert!(
        sorted.windows(2).all(|w| w[0].0 != w[1].0),
        "acf1 identity object with a duplicate key"
    );
    let mut out = vec![b'{'];
    for (i, (k, v)) in sorted.into_iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        string(k, &mut out);
        out.push(b':');
        string(v, &mut out);
    }
    out.push(b'}');
    out
}

/// `"acf1:" + sha256(acf1_canonical_bytes(fields))`.
pub fn acf1_digest(fields: &[(&str, &str)]) -> String {
    format!("acf1:{}", sha256_hex(&acf1_canonical_bytes(fields)))
}

/// `acf1:` identity of a registered executable:
/// `{"registered_executable_ref","sha256"}`.
pub fn fabric_executable_digest(id: &str, sha256: &str) -> String {
    acf1_digest(&[("registered_executable_ref", id), ("sha256", sha256)])
}

/// `acf1:` identity of a single-file workspace version (the exact bytes a
/// check judged): `{"path","sha256"}`.
pub fn fabric_workspace_digest(rel_path: &str, bytes: &[u8]) -> String {
    acf1_digest(&[("path", rel_path), ("sha256", &sha256_hex(bytes))])
}

/// The `schema` value of a WorkspaceVersion identity object.
pub const WORKSPACE_VERSION_SCHEMA: &str = "axon.workspace-version/1";

/// One entry of a WorkspaceVersion manifest (B261, identity gap G8): a
/// relative `/`-separated UTF-8 path, its mode (`100644` | `100755` |
/// `120000`), content length and lowercase-hex sha256. Validating the path
/// and the tree (traversal, links, quotas …) is the importer's job
/// (`axon_fabric::workspace`); this is only the byte recipe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceManifestEntry {
    pub path: String,
    pub mode: &'static str,
    pub size: u64,
    pub sha256: String,
}

/// THE WorkspaceVersion manifest bytes (MiCode
/// `docs/axon-support/WORKSPACE_VERSION_RECIPE.md` §3, shared with
/// `micode-persist::workspace_version`): one
/// `<mode> SP <size> SP <sha256> SP <path> LF` line per entry, sorted by the
/// path's UTF-8 bytes; an empty tree is the empty byte string. It lives here,
/// beside `acf1_canonical_bytes`, for the same reason: both sides of the
/// cortex → fabric seam build it, and there must be one implementation.
pub fn workspace_manifest_bytes(entries: &[WorkspaceManifestEntry]) -> Vec<u8> {
    let mut sorted: Vec<&WorkspaceManifestEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.path.as_bytes().cmp(b.path.as_bytes()));
    let mut out = Vec::new();
    for e in sorted {
        out.extend_from_slice(e.mode.as_bytes());
        out.push(b' ');
        out.extend_from_slice(e.size.to_string().as_bytes());
        out.push(b' ');
        out.extend_from_slice(e.sha256.as_bytes());
        out.push(b' ');
        out.extend_from_slice(e.path.as_bytes());
        out.push(b'\n');
    }
    out
}

/// `acf1:` WorkspaceVersion reference of a manifest (recipe §4):
/// `acf1_digest({"manifest_sha256": sha256(manifest), "schema": "axon.workspace-version/1"})`.
pub fn workspace_version_ref(manifest: &[u8]) -> String {
    acf1_digest(&[
        ("manifest_sha256", &sha256_hex(manifest)),
        ("schema", WORKSPACE_VERSION_SCHEMA),
    ])
}

/// The WorkspaceVersion reference of a tree holding exactly one regular,
/// non-executable file — the artifact a single-file check judges.
pub fn single_file_workspace_version_ref(rel_path: &str, bytes: &[u8]) -> String {
    workspace_version_ref(&workspace_manifest_bytes(&[WorkspaceManifestEntry {
        path: rel_path.to_string(),
        mode: "100644",
        size: bytes.len() as u64,
        sha256: sha256_hex(bytes),
    }]))
}

/// D11 default: at most this many entries.
pub const MAX_ENTRIES: usize = 20_000;
/// D11 default: at most this many content bytes (files + symlink targets).
pub const MAX_BYTES: u64 = 268_435_456;
/// D11 default: at most this many `/`-separated components per path.
pub const MAX_DEPTH: usize = 32;

/// Top-level names the recipe skips (repository / MiCode runtime state).
pub const SKIPPED_TOP_LEVEL: [&str; 2] = [".git", ".micode"];

pub const MODE_FILE: &str = "100644";
pub const MODE_EXEC: &str = "100755";
pub const MODE_LINK: &str = "120000";

/// Import quota. `Default` is operator decision D11.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Quota {
    pub entries: usize,
    pub bytes: u64,
    pub depth: usize,
}

impl Default for Quota {
    fn default() -> Self {
        Quota {
            entries: MAX_ENTRIES,
            bytes: MAX_BYTES,
            depth: MAX_DEPTH,
        }
    }
}

/// Why a tree was refused. The whole tree is refused; nothing is imported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRefusal {
    Traversal(String),
    Absolute(String),
    EscapingSymlink { path: String, target: String },
    SpecialFile(String),
    NonUtf8(String),
    ControlCharacter(String),
    Duplicate(String),
    Collision { a: String, b: String },
    QuotaEntries { limit: usize },
    QuotaBytes { limit: u64 },
    QuotaDepth { path: String, limit: usize },
    Io(String),
}

impl ImportRefusal {
    /// A stable class name (one per recipe §2 row, plus `collision`).
    pub fn class(&self) -> &'static str {
        match self {
            ImportRefusal::Traversal(_) => "traversal",
            ImportRefusal::Absolute(_) => "absolute",
            ImportRefusal::EscapingSymlink { .. } => "escaping_symlink",
            ImportRefusal::SpecialFile(_) => "special_file",
            ImportRefusal::NonUtf8(_) => "non_utf8",
            ImportRefusal::ControlCharacter(_) => "control_character",
            ImportRefusal::Duplicate(_) => "duplicate",
            ImportRefusal::Collision { .. } => "collision",
            ImportRefusal::QuotaEntries { .. } => "quota_entries",
            ImportRefusal::QuotaBytes { .. } => "quota_bytes",
            ImportRefusal::QuotaDepth { .. } => "quota_depth",
            ImportRefusal::Io(_) => "io",
        }
    }
}

impl std::fmt::Display for ImportRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ImportRefusal::Traversal(p) => write!(
                f,
                "traversal: path {p:?} has an empty, `.` or `..` component"
            ),
            ImportRefusal::Absolute(p) => write!(f, "absolute path {p:?}"),
            ImportRefusal::EscapingSymlink { path, target } => {
                write!(
                    f,
                    "symlink {path:?} -> {target:?} leaves the workspace root"
                )
            }
            ImportRefusal::SpecialFile(p) => write!(f, "{p:?} is a device, FIFO or socket"),
            ImportRefusal::NonUtf8(p) => write!(f, "name under {p:?} is not UTF-8"),
            ImportRefusal::ControlCharacter(p) => {
                write!(f, "path {p:?} contains a control character")
            }
            ImportRefusal::Duplicate(p) => write!(f, "path {p:?} appears twice"),
            ImportRefusal::Collision { a, b } => write!(
                f,
                "namespace collision: {a:?} and {b:?} name the same file on a \
                 case-insensitive/normalizing filesystem (or a file is also a directory)"
            ),
            ImportRefusal::QuotaEntries { limit } => write!(f, "quota: more than {limit} entries"),
            ImportRefusal::QuotaBytes { limit } => {
                write!(f, "quota: more than {limit} content bytes")
            }
            ImportRefusal::QuotaDepth { path, limit } => {
                write!(f, "quota: {path:?} is deeper than {limit} components")
            }
            ImportRefusal::Io(e) => write!(f, "io: {e}"),
        }
    }
}

/// What an entry is. The content of a symlink is its TARGET text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntryKind {
    File { executable: bool },
    Symlink,
}

impl EntryKind {
    pub fn mode(self) -> &'static str {
        match self {
            EntryKind::File { executable: false } => MODE_FILE,
            EntryKind::File { executable: true } => MODE_EXEC,
            EntryKind::Symlink => MODE_LINK,
        }
    }
    pub fn from_mode(m: &str) -> Option<EntryKind> {
        match m {
            MODE_FILE => Some(EntryKind::File { executable: false }),
            MODE_EXEC => Some(EntryKind::File { executable: true }),
            MODE_LINK => Some(EntryKind::Symlink),
            _ => None,
        }
    }
}

/// One entry with its content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeEntry {
    pub path: String,
    pub kind: EntryKind,
    pub content: Vec<u8>,
}

/// Something deliberately NOT in the version, and why.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Omission {
    pub path: String,
    pub reason: String,
}

/// Recipe path rules: relative, `/`-separated, no empty/`.`/`..`
/// component, no control character, within the depth quota.
pub fn check_path(path: &str, quota: &Quota) -> Result<(), ImportRefusal> {
    if path.starts_with('/') {
        return Err(ImportRefusal::Absolute(path.into()));
    }
    if path
        .split('/')
        .any(|c| c.is_empty() || c == "." || c == "..")
    {
        return Err(ImportRefusal::Traversal(path.into()));
    }
    if path.chars().any(char::is_control) {
        return Err(ImportRefusal::ControlCharacter(path.into()));
    }
    if path.split('/').count() > quota.depth {
        return Err(ImportRefusal::QuotaDepth {
            path: path.into(),
            limit: quota.depth,
        });
    }
    Ok(())
}

/// Recipe symlink rule: a target that is empty or absolute, or that —
/// resolved lexically from the link's own directory — pops above the root.
pub fn check_link(path: &str, target: &[u8]) -> Result<(), ImportRefusal> {
    let esc = || ImportRefusal::EscapingSymlink {
        path: path.into(),
        target: String::from_utf8_lossy(target).into_owned(),
    };
    let t = std::str::from_utf8(target).map_err(|_| ImportRefusal::NonUtf8(path.into()))?;
    if t.is_empty() || t.starts_with('/') {
        return Err(esc());
    }
    let mut depth = path.split('/').count() - 1; // the link's own directory
    for c in t.split('/') {
        match c {
            "" | "." => {}
            ".." => depth = depth.checked_sub(1).ok_or_else(esc)?,
            _ => depth += 1,
        }
    }
    Ok(())
}

#[cfg(unix)]
pub fn is_exec(m: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    m.permissions().mode() & 0o111 != 0
}
#[cfg(not(unix))]
pub fn is_exec(_m: &std::fs::Metadata) -> bool {
    false
}

pub fn walk(
    dir: &Path,
    prefix: &str,
    quota: &Quota,
    entries: &mut Vec<TreeEntry>,
    omissions: &mut Vec<Omission>,
    bytes: &mut u64,
) -> Result<(), ImportRefusal> {
    let io = |e: std::io::Error| ImportRefusal::Io(format!("{}: {e}", dir.display()));
    let mut names: Vec<std::fs::DirEntry> = std::fs::read_dir(dir)
        .map_err(io)?
        .collect::<Result<_, _>>()
        .map_err(io)?;
    names.sort_by_key(|d| d.file_name());
    for d in names {
        let name = d.file_name();
        let Some(name) = name.to_str() else {
            return Err(ImportRefusal::NonUtf8(if prefix.is_empty() {
                ".".into()
            } else {
                prefix.into()
            }));
        };
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };
        if prefix.is_empty() && SKIPPED_TOP_LEVEL.contains(&name) {
            omissions.push(Omission {
                path,
                reason: "top-level repository/runtime state (recipe §1)".into(),
            });
            continue;
        }
        let meta = std::fs::symlink_metadata(d.path()).map_err(io)?;
        let ft = meta.file_type();
        if ft.is_dir() {
            // A directory contributes nothing itself (an empty one is
            // invisible); its entries carry its components and are checked.
            walk(&d.path(), &path, quota, entries, omissions, bytes)?;
            continue;
        }
        check_path(&path, quota)?;
        let entry = if ft.is_symlink() {
            let target = std::fs::read_link(d.path()).map_err(io)?;
            let t = target
                .to_str()
                .ok_or_else(|| ImportRefusal::NonUtf8(path.clone()))?
                .as_bytes()
                .to_vec();
            check_link(&path, &t)?;
            TreeEntry {
                path,
                kind: EntryKind::Symlink,
                content: t,
            }
        } else if ft.is_file() {
            // Refuse BEFORE reading a file that alone breaks the byte quota.
            if bytes.saturating_add(meta.len()) > quota.bytes {
                return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });
            }
            TreeEntry {
                content: std::fs::read(d.path()).map_err(io)?,
                kind: EntryKind::File {
                    executable: is_exec(&meta),
                },
                path,
            }
        } else {
            return Err(ImportRefusal::SpecialFile(path));
        };
        *bytes = bytes.saturating_add(entry.content.len() as u64);
        if *bytes > quota.bytes {
            return Err(ImportRefusal::QuotaBytes { limit: quota.bytes });
        }
        entries.push(entry);
        if entries.len() > quota.entries {
            return Err(ImportRefusal::QuotaEntries {
                limit: quota.entries,
            });
        }
    }
    Ok(())
}

/// Walk `root` (never following a symlink) into its entries and omissions,
/// applying the recipe's per-entry rules and the quota. Collision and
/// duplicate refusal is the importer's (`axon_fabric::workspace`).
pub fn walk_tree(
    root: &Path,
    quota: &Quota,
) -> Result<(Vec<TreeEntry>, Vec<Omission>), ImportRefusal> {
    let meta = std::fs::symlink_metadata(root).map_err(|e| ImportRefusal::Io(e.to_string()))?;
    if !meta.is_dir() {
        return Err(ImportRefusal::Io(format!(
            "{} is not a directory",
            root.display()
        )));
    }
    let mut entries = Vec::new();
    let mut omissions = Vec::new();
    let mut bytes: u64 = 0;
    walk(root, "", quota, &mut entries, &mut omissions, &mut bytes)?;
    Ok((entries, omissions))
}

/// The manifest entries of `entries` (content hashed here).
pub fn manifest_entries(entries: &[TreeEntry]) -> Vec<WorkspaceManifestEntry> {
    entries
        .iter()
        .map(|e| WorkspaceManifestEntry {
            path: e.path.clone(),
            mode: e.kind.mode(),
            size: e.content.len() as u64,
            sha256: sha256_hex(&e.content),
        })
        .collect()
}

/// The `acf1:` WorkspaceVersion reference of the tree at `root`: what the
/// guest compares with its launch manifest. Equal to the reference Fabric's
/// store recorded for any tree it accepted.
pub fn tree_version_ref(root: &Path, quota: &Quota) -> Result<String, ImportRefusal> {
    let (entries, _) = walk_tree(root, quota)?;
    Ok(workspace_version_ref(&workspace_manifest_bytes(
        &manifest_entries(&entries),
    )))
}
