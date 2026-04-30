//! Incremental compilation cache for the Axon compiler (`axon build`).
//!
//! Cache files (`.axc`) live in `~/.cache/axon/` by default.  Each entry is
//! keyed by a SHA-256 digest over (source bytes, compiler version string) and
//! stores the LLVM bitcode for the compiled module.
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
