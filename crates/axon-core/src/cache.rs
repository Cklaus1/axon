//! Incremental compilation cache for the Axon compiler (`axon build`).
//!
//! Cache files (`.axc`) live in `~/.cache/axon/` by default.  Each entry is
//! keyed by a SHA-256 digest over (source bytes, compiler identity) and
//! stores the LLVM bitcode for the compiled module.  The compiler identity
//! ([`compiler_identity`]) is a digest of the running compiler EXECUTABLE, so
//! any rebuilt compiler — committed or not — misses every older entry.
//!
//! Format of a `.axc` file:
//! ```text
//! [0..8]   magic bytes  b"AXONCACH"
//! [8..12]  version string length  (u32 LE)
//! [12..N]  compiler version string (UTF-8, no NUL)
//! [N..]    LLVM bitcode
//! ```

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

// ── Magic / header ────────────────────────────────────────────────────────────

const MAGIC: &[u8; 8] = b"AXONCACH";

// ── Public API ────────────────────────────────────────────────────────────────

/// Compute the cache key (hex SHA-256) for the given source bytes and compiler
/// version string.  The key is stable across runs as long as the inputs are.
pub fn cache_key(source: &[u8], compiler_version: &str) -> String {
    let mut h = Sha256::new();
    h.update(source);
    h.update(compiler_version.as_bytes());
    format!("{:x}", h.finalize())
}

/// Hex SHA-256 of the running compiler EXECUTABLE: the compiler-identity half
/// of the cache key.
///
/// The version string alone cannot tell two builds apart (its git SHA does not
/// move for uncommitted edits), and the executable's path + size + mtime can
/// collide for builds that differ only in a constant (same size) when the
/// mtime is normalised (reproducible builds, copies that preserve it). The
/// bytes of the executable are the build, so they are what gets hashed.
///
/// Hashing a dev-build compiler (~200 MB) costs ~0.2 s, more than a cache hit
/// saves on a small program, so the digest is memoised in `memo_dir` under a
/// fingerprint of the executable's inode metadata. On Unix that includes the
/// device, inode and the CHANGE time (ctime), which no `touch`, mtime
/// normalisation or copy can set back: replacing the executable's bytes in
/// place moves ctime, and replacing the file moves the inode. Elsewhere the
/// executable is hashed on every call.
///
/// Returns `None` if the executable cannot be located or read; the caller must
/// then not use the cache at all, since no key could tell this compiler apart.
pub fn compiler_digest(memo_dir: &Path) -> Option<String> {
    let exe = std::env::current_exe().ok()?;
    let md = std::fs::metadata(&exe).ok()?;
    let memo = exe_fingerprint(&exe, &md).map(|fp| memo_dir.join(format!("compiler-{fp}.id")));
    if let Some(memo) = &memo {
        if let Ok(d) = std::fs::read_to_string(memo) {
            if d.len() == 64 && d.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Some(d);
            }
        }
    }
    let mut h = Sha256::new();
    std::io::copy(&mut std::fs::File::open(&exe).ok()?, &mut h).ok()?;
    let digest = format!("{:x}", h.finalize());
    if let Some(memo) = &memo {
        // A memo that cannot be written only costs the next run a re-hash.
        let _ = std::fs::create_dir_all(memo_dir).and_then(|()| std::fs::write(memo, &digest));
    }
    Some(digest)
}

/// Fingerprint of the executable file itself (path + inode metadata), the
/// memo key for [`compiler_digest`]. `None` where ctime/inode are unavailable.
#[cfg(unix)]
fn exe_fingerprint(exe: &Path, md: &std::fs::Metadata) -> Option<String> {
    use std::os::unix::fs::MetadataExt;
    let mut h = Sha256::new();
    h.update(exe.as_os_str().as_encoded_bytes());
    for v in [
        md.dev(),
        md.ino(),
        md.size(),
        md.mtime() as u64,
        md.mtime_nsec() as u64,
        md.ctime() as u64,
        md.ctime_nsec() as u64,
    ] {
        h.update(v.to_le_bytes());
    }
    Some(format!("{:x}", h.finalize()))
}

#[cfg(not(unix))]
fn exe_fingerprint(_exe: &Path, _md: &std::fs::Metadata) -> Option<String> {
    None
}

/// Return the default cache directory: `~/.cache/axon/`.
///
/// Falls back to `/tmp/axon-cache/` when `$HOME` is not set.
pub fn default_cache_dir() -> PathBuf {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".cache")
        .join("axon")
}

/// Return the full path for a cache entry with `key` inside `dir`.
pub fn cache_path(key: &str, dir: &Path) -> PathBuf {
    dir.join(format!("{key}.axc"))
}

/// Write LLVM `bitcode` to a `.axc` file at `path`.
///
/// Creates parent directories as needed.  Silently overwrites existing files.
pub fn write_axc(path: &Path, bitcode: &[u8], compiler_version: &str) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut f = std::fs::File::create(path)?;
    f.write_all(MAGIC)?;
    let ver = compiler_version.as_bytes();
    f.write_all(&(ver.len() as u32).to_le_bytes())?;
    f.write_all(ver)?;
    f.write_all(bitcode)?;
    Ok(())
}

/// Read LLVM bitcode from a `.axc` file, validating the magic bytes and
/// compiler version.
///
/// Returns `None` if the file is absent, corrupt, or was written by a
/// different compiler version (E0906 scenario — caller logs the warning).
pub fn read_axc(path: &Path, compiler_version: &str) -> Option<Vec<u8>> {
    let data = std::fs::read(path).ok()?;
    if data.len() < 12 {
        return None;
    }
    if data[..8] != *MAGIC {
        return None;
    }
    let ver_len = u32::from_le_bytes(data[8..12].try_into().ok()?) as usize;
    let body_start = 12 + ver_len;
    if data.len() < body_start {
        return None;
    }
    let stored_ver = std::str::from_utf8(&data[12..body_start]).ok()?;
    if stored_ver != compiler_version {
        return None; // different compiler version — cache miss
    }
    Some(data[body_start..].to_vec())
}

/// Remove `.axc` files from `dir`.
///
/// If `older_than_secs` is `Some(n)`, only files whose last-access time is
/// older than `n` seconds are removed.  If it is `None`, all entries are
/// removed.
///
/// Returns `(removed, errors)` counts.
pub fn clean_cache(dir: &Path, older_than_secs: Option<u64>) -> (usize, usize) {
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return (0, 0),
    };

    let mut removed = 0usize;
    let mut errors = 0usize;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("axc") {
            continue;
        }

        if let Some(max_age_secs) = older_than_secs {
            if let Ok(meta) = entry.metadata() {
                // Use modification time as a proxy (access time is unreliable
                // on many Linux filesystems with `relatime`).
                if let Ok(modified) = meta.modified() {
                    if let Ok(age) = modified.elapsed() {
                        if age.as_secs() < max_age_secs {
                            continue; // recently modified — keep
                        }
                    }
                }
            }
        }

        if std::fs::remove_file(&path).is_ok() {
            removed += 1;
        } else {
            errors += 1;
        }
    }

    (removed, errors)
}
