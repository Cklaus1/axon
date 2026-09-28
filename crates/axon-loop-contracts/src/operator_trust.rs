//! The operator's trust roots — ONE implementation for every crate that
//! decides authority (Fabric and the loop). Lives here, the lowest crate both
//! link.
//!
//! `/etc/axon/trust/<authority>/*.pub`, one root PER AUTHORITY. Every path
//! component from `/` is root-owned, not group/other-writable, and not a
//! symlink. No repository file, store config, CLI flag or environment variable
//! can name another root or add a key (operator directions 2026-09-27/28;
//! v022-psv-protocol.md §8, O2).
//!
//! A TEST root exists only in builds with the `test-trust-root` feature
//! ([`set_test_root`]); production builds cannot contain the setter.

use std::path::{Path, PathBuf};

pub const OPERATOR_TRUST_ROOT: &str = "/etc/axon/trust";

/// One trust root PER AUTHORITY, so a key trusted for one purpose never becomes
/// valid for another: `/etc/axon/trust/<authority>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustAuthority {
    /// B263 qualification records and protected-host certifications.
    Qualification,
    /// ADR-002 preflight observations.
    Observer,
    /// Fabric verifier (receipt attestation) keys.
    Verifier,
    /// Admission / transition authority.
    Admission,
}

impl TrustAuthority {
    pub const ALL: [TrustAuthority; 4] = [
        TrustAuthority::Qualification,
        TrustAuthority::Observer,
        TrustAuthority::Verifier,
        TrustAuthority::Admission,
    ];
    pub fn parse(s: &str) -> Option<TrustAuthority> {
        TrustAuthority::ALL.into_iter().find(|a| a.dir_name() == s)
    }
    pub fn dir_name(self) -> &'static str {
        match self {
            TrustAuthority::Qualification => "qualification",
            TrustAuthority::Observer => "observer",
            TrustAuthority::Verifier => "verifier",
            TrustAuthority::Admission => "admission",
        }
    }
    /// The fixed, absolute production path.
    pub fn operator_dir(self) -> PathBuf {
        Path::new(OPERATOR_TRUST_ROOT).join(self.dir_name())
    }
}

/// The operator-ownership walk from `base` down to `path`: every component
/// root-owned, not group/other-writable, not a symlink; with `entries`, a
/// directory's entries too.
#[cfg(unix)]
pub fn check_owned_chain(base: &Path, dir: &Path, entries: bool) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if !dir.is_absolute() {
        return Err(format!(
            "{} is not an absolute path: a trust root is a fixed host path",
            dir.display()
        ));
    }
    let check = |p: &Path| -> Result<(), String> {
        let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if m.file_type().is_symlink() {
            return Err(format!(
                "{} is a symlink: a trust root is never redirected",
                p.display()
            ));
        }
        if m.uid() != 0 {
            return Err(format!(
                "{} is owned by uid {}, not root: an agent-writable trust root authorizes nothing",
                p.display(),
                m.uid()
            ));
        }
        if m.mode() & 0o022 != 0 {
            return Err(format!(
                "{} is group- or other-writable (mode {:o}): it authorizes nothing",
                p.display(),
                m.mode() & 0o7777
            ));
        }
        Ok(())
    };
    let rel = dir
        .strip_prefix(base)
        .map_err(|_| format!("{} is not below {}", dir.display(), base.display()))?;
    let mut p = base.to_path_buf();
    check(&p)?;
    for c in rel.components() {
        p.push(c);
        check(&p)?;
    }
    if entries && std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            check(&e.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
}

/// The `*.pub` keys under `dir`: each exactly one 64-hex-char Ed25519 public
/// key, returned lowercase. An absent or empty root holds nothing.
pub fn keys_in(dir: &Path) -> Result<Vec<String>, String> {
    let mut keys = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.extension().and_then(|e| e.to_str()) != Some("pub") {
                continue;
            }
            let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            let t = t.trim().to_ascii_lowercase();
            if t.len() != 64 || !t.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(format!(
                    "trusted key {} is not a 64-hex-char Ed25519 public key",
                    p.display()
                ));
            }
            keys.push(t);
        }
    }
    Ok(keys)
}

#[cfg(feature = "test-trust-root")]
thread_local! {
    static TEST_ROOT: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}

/// TESTS ONLY (`test-trust-root`): use `dir/<authority>/` instead of the
/// operator root on THIS THREAD, ownership unchecked. Per thread, so each test
/// has its own operator root (one may revoke or install a key without touching
/// another's); any other thread still sees the production root and fails
/// closed. A production build does not contain it.
#[cfg(feature = "test-trust-root")]
pub fn set_test_root(dir: &Path) {
    TEST_ROOT.with(|r| *r.borrow_mut() = Some(dir.to_path_buf()));
}

/// Where `a`'s keys are trusted in THIS process, and whether that root must be
/// operator-owned (always, except under a test root).
pub fn authority_root(a: TrustAuthority) -> (PathBuf, bool) {
    #[cfg(feature = "test-trust-root")]
    if let Some(r) = TEST_ROOT.with(|r| r.borrow().clone()) {
        return (r.join(a.dir_name()), false);
    }
    (a.operator_dir(), true)
}

/// `Ok` iff the operator's root for `a` holds `key_hex`. The store (or any
/// other mutable config) may NAME a key; only this root makes it authority.
pub fn rooted(a: TrustAuthority, key_hex: &str) -> Result<(), String> {
    let (dir, owned) = authority_root(a);
    #[cfg(unix)]
    if owned {
        check_owned_chain(Path::new("/"), &dir, true)?;
    }
    #[cfg(not(unix))]
    if owned {
        return Err("operator ownership cannot be checked on this platform".into());
    }
    let want = key_hex.trim().to_ascii_lowercase();
    if keys_in(&dir)?.contains(&want) {
        Ok(())
    } else {
        Err(format!(
            "key {}… is not in the operator's {} root ({}): a store may name a key, only the \
             operator root makes it authority",
            want.get(..16).unwrap_or(&want),
            a.dir_name(),
            dir.display()
        ))
    }
}
