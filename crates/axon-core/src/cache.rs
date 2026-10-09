//! Incremental compilation cache for the Axon compiler (`axon build`).
//!
//! Cache files (`.axc`) live in `~/.cache/axon/` by default.  Each entry is
//! keyed by a SHA-256 digest over (source and imported-module bytes, target,
//! opt level, artifact kind, compiler identity) and stores the hosted
//! program's relocatable OBJECT after the opt-level IR pipeline and the
//! backend, so a hit only links (AX-34). The compiler identity
//! ([`compiler_digest`]) is a digest of the running compiler EXECUTABLE, so
//! any rebuilt compiler — committed or not — misses every older entry.
//!
//! Format of a `.axc` file (format version 2):
//! ```text
//! [0..8]       magic bytes  b"AXONCACH"
//! [8..12]      format version (u32 LE) = 2
//! [12..16]     compiler identity length  (u32 LE)
//! [16..N]      compiler identity (UTF-8, no NUL)
//! [N]          flags: bit 0 = the object links the AI runtime; others 0
//! [N+1..N+9]   object length (u64 LE)
//! [N+9..N+41]  SHA-256 of the object
//! [N+41..]     the relocatable object
//! ```
//!
//! Version 1 stored pre-optimisation LLVM bitcode with no format field; its
//! bytes 8..12 hold the identity length, never 2, so a v1 entry reads as
//! unusable and is rebuilt. An object is linked as-is, with nothing to reject
//! a damaged one before the linker (or the program) does, so the checksum is
//! what makes a truncated or bit-flipped entry a rebuild instead of a binary.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

// ── Magic / header ────────────────────────────────────────────────────────────

const MAGIC: &[u8; 8] = b"AXONCACH";

/// Bump whenever the entry layout or the meaning of its payload changes.
const FORMAT_VERSION: u32 = 2;

/// `flags` bit 0: the object links `libaxon_rt_ai.a`.
const FLAG_AI_RUNTIME: u8 = 1;

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

/// Outcome of reading a cache entry.
#[derive(Debug, PartialEq, Eq)]
pub enum CacheLookup {
    /// A valid entry: the stored object and whether it links the AI runtime.
    Hit {
        object: Vec<u8>,
        links_ai_runtime: bool,
    },
    /// No entry at this path.
    Miss,
    /// An entry exists but must not be used (old format, damaged, or another
    /// compiler's); the reason says which. The caller rebuilds and overwrites.
    Unusable(String),
}

/// Write a hosted program `object` to a `.axc` file at `path`.
///
/// Creates parent directories as needed. The entry is written to a temporary
/// file in the same directory (`<key>.<pid>.tmp.axc`, so `axon cache clean`
/// removes one a crash left behind) and renamed into place, so a concurrent
/// reader sees the old entry or the new one, never a partial write.
pub fn write_axc(
    path: &Path,
    object: &[u8],
    links_ai_runtime: bool,
    compiler_version: &str,
) -> std::io::Result<()> {
    use std::io::Write as _;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension(format!("{}.tmp.axc", std::process::id()));
    let write = || -> std::io::Result<()> {
        let ver = compiler_version.as_bytes();
        let mut f = std::io::BufWriter::new(std::fs::File::create(&tmp)?);
        f.write_all(MAGIC)?;
        f.write_all(&FORMAT_VERSION.to_le_bytes())?;
        f.write_all(&(ver.len() as u32).to_le_bytes())?;
        f.write_all(ver)?;
        f.write_all(&[if links_ai_runtime { FLAG_AI_RUNTIME } else { 0 }])?;
        f.write_all(&(object.len() as u64).to_le_bytes())?;
        f.write_all(&Sha256::digest(object))?;
        f.write_all(object)?;
        f.into_inner().map_err(|e| e.into_error())?;
        std::fs::rename(&tmp, path)
    };
    write().inspect_err(|_| {
        let _ = std::fs::remove_file(&tmp);
    })
}

/// Read a cache entry, validating the magic bytes, format version, compiler
/// identity, flags, object length and object checksum.
pub fn read_axc(path: &Path, compiler_version: &str) -> CacheLookup {
    let mut data = match std::fs::read(path) {
        Ok(d) => d,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return CacheLookup::Miss,
        Err(e) => return CacheLookup::Unusable(format!("cannot read it: {e}")),
    };
    match parse_axc(&data, compiler_version) {
        Ok((object_start, links_ai_runtime)) => {
            data.drain(..object_start);
            CacheLookup::Hit {
                object: data,
                links_ai_runtime,
            }
        }
        Err(why) => CacheLookup::Unusable(why),
    }
}

/// Validate an entry; on success return where the object starts and the
/// AI-runtime flag.
fn parse_axc(data: &[u8], compiler_version: &str) -> Result<(usize, bool), String> {
    let truncated = || "truncated entry".to_string();
    let u32_at = |at: usize| -> Option<u32> {
        Some(u32::from_le_bytes(data.get(at..at + 4)?.try_into().ok()?))
    };
    if data.get(..8) != Some(MAGIC.as_slice()) {
        return Err("not an axon cache entry (bad magic)".to_string());
    }
    let format = u32_at(8).ok_or_else(truncated)?;
    if format != FORMAT_VERSION {
        return Err(format!(
            "cache format {format}, this compiler reads format {FORMAT_VERSION}"
        ));
    }
    let ver_len = u32_at(12).ok_or_else(truncated)? as usize;
    let ver_end = 16usize.checked_add(ver_len).ok_or_else(truncated)?;
    let stored_ver = data.get(16..ver_end).ok_or_else(truncated)?;
    if stored_ver != compiler_version.as_bytes() {
        return Err("written by a different compiler".to_string());
    }
    let flags = *data.get(ver_end).ok_or_else(truncated)?;
    if flags & !FLAG_AI_RUNTIME != 0 {
        return Err(format!("unknown flags {flags:#04x}"));
    }
    let len_at = ver_end + 1;
    let obj_len = u64::from_le_bytes(
        data.get(len_at..len_at + 8)
            .ok_or_else(truncated)?
            .try_into()
            .map_err(|_| truncated())?,
    );
    let sum = data.get(len_at + 8..len_at + 40).ok_or_else(truncated)?;
    let object_start = len_at + 40;
    let object = &data[object_start..];
    if object.len() as u64 != obj_len {
        return Err(format!(
            "object is {} bytes, header says {obj_len}",
            object.len()
        ));
    }
    if Sha256::digest(object).as_slice() != sum {
        return Err("object checksum mismatch".to_string());
    }
    Ok((object_start, flags & FLAG_AI_RUNTIME != 0))
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

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "0.1.0+0123456789abcdef";

    /// Each test gets its own directory: tests run in parallel threads of one
    /// process, and the round-trip test counts `.tmp.` files beside its entry,
    /// so a sibling's in-flight `write_axc` in a shared directory would fail it.
    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("axon_cache_unit_{}_{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        dir.join(format!("{name}.axc"))
    }

    #[test]
    fn an_entry_round_trips_its_object_and_runtime_flag() {
        let p = scratch("roundtrip");
        for ai in [false, true] {
            write_axc(&p, b"\x7fELF object bytes", ai, ID).unwrap();
            assert_eq!(
                read_axc(&p, ID),
                CacheLookup::Hit {
                    object: b"\x7fELF object bytes".to_vec(),
                    links_ai_runtime: ai,
                }
            );
        }
        // The temp file was renamed into place, not left beside it.
        let leftovers = std::fs::read_dir(p.parent().unwrap())
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .count();
        assert_eq!(leftovers, 0);
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn an_absent_entry_is_a_miss() {
        assert_eq!(read_axc(&scratch("absent"), ID), CacheLookup::Miss);
    }

    /// Every way an entry can be damaged or stale must read as unusable,
    /// never as a hit carrying the wrong bytes.
    #[test]
    fn damaged_old_or_foreign_entries_are_unusable() {
        let p = scratch("damaged");
        write_axc(&p, &[7u8; 4096], false, ID).unwrap();
        let good = std::fs::read(&p).unwrap();
        let unusable = |bytes: &[u8], id: &str| -> String {
            std::fs::write(&p, bytes).unwrap();
            match read_axc(&p, id) {
                CacheLookup::Unusable(why) => why,
                other => panic!("expected unusable, got {other:?}"),
            }
        };

        // Truncated anywhere: in the header, and in the object.
        for cut in [4, 10, 20, good.len() / 2, good.len() - 1] {
            unusable(&good[..cut], ID);
        }
        // One flipped bit in the object.
        let mut flipped = good.clone();
        *flipped.last_mut().unwrap() ^= 1;
        assert!(unusable(&flipped, ID).contains("checksum"));
        // Another compiler's entry.
        assert!(unusable(&good, "0.1.0+other").contains("different compiler"));
        // A format-1 entry: magic, identity length, identity, LLVM bitcode.
        let mut v1 = MAGIC.to_vec();
        v1.extend_from_slice(&(ID.len() as u32).to_le_bytes());
        v1.extend_from_slice(ID.as_bytes());
        v1.extend_from_slice(b"BC\xc0\xde bitcode");
        assert!(unusable(&v1, ID).contains("cache format"));
        // Not a cache entry at all.
        unusable(b"garbage that is long enough to have a header", ID);
        let _ = std::fs::remove_file(&p);
    }
}
