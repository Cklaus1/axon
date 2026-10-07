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
    /// Safety monitors whose clearance a PROTECTED evaluation rests on
    /// (review wf_1bc28496-38e, PSV-7).
    Monitor,
}

impl TrustAuthority {
    pub const ALL: [TrustAuthority; 5] = [
        TrustAuthority::Qualification,
        TrustAuthority::Observer,
        TrustAuthority::Verifier,
        TrustAuthority::Admission,
        TrustAuthority::Monitor,
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
            TrustAuthority::Monitor => "monitor",
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
/// key, returned lowercase. An absent (NotFound) or empty root holds nothing.
/// Any OTHER failure to list it (EACCES, ENOTDIR, ELOOP, ...) refuses: a root
/// that cannot be read is not a root that holds no key (C9 round 2, PSV-6;
/// A67). Callers that decide key-role separation rely on this.
pub fn keys_in(dir: &Path) -> Result<Vec<String>, String> {
    let mut keys = Vec::new();
    let listed = match std::fs::read_dir(dir) {
        Ok(rd) => Some(rd),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(format!(
                "trust root {} cannot be read ({e}): what it holds cannot be known",
                dir.display()
            ))
        }
    };
    if let Some(rd) = listed {
        let mut paths = rd
            .map(|e| e.map(|e| e.path()))
            .collect::<Result<Vec<PathBuf>, _>>()
            .map_err(|e| format!("trust root {}: {e}", dir.display()))?;
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

/// The keys held by `a`'s operator root, after the ownership walk (always,
/// except under a test root), lowercase hex.
fn root_keys_hex(a: TrustAuthority) -> Result<Vec<String>, String> {
    let (dir, owned) = authority_root(a);
    #[cfg(unix)]
    if owned {
        check_owned_chain(Path::new("/"), &dir, true)?;
    }
    #[cfg(not(unix))]
    if owned {
        return Err("operator ownership cannot be checked on this platform".into());
    }
    keys_in(&dir)
}

/// ADR-002 at the ROOTS, not at the store (C9 round 1, PSV-6): a key is
/// authority for `a` only if no OTHER operator root holds it too. Otherwise
/// whoever holds that key for one purpose (Fabric's verifier signer, say) can
/// mint statements of another (an observation). An absent root holds nothing;
/// a present root that fails the ownership walk refuses, since what it holds
/// cannot be known.
pub fn exclusive(a: TrustAuthority, key_hex: &str) -> Result<(), String> {
    let want = key_hex.trim().to_ascii_lowercase();
    for b in TrustAuthority::ALL {
        if b == a {
            continue;
        }
        let (dir, _) = authority_root(b);
        if matches!(std::fs::symlink_metadata(&dir), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        if root_keys_hex(b)?.contains(&want) {
            return Err(format!(
                "key {}… is in the operator's {} root AND its {} root: one key never holds two \
                 authorities (ADR-002), so it is authority for neither",
                want.get(..16).unwrap_or(&want),
                a.dir_name(),
                b.dir_name()
            ));
        }
    }
    Ok(())
}

/// `Ok` iff the operator's root for `a` holds `key_hex`, and no other
/// operator root does ([`exclusive`]). The store (or any other mutable
/// config) may NAME a key; only this root makes it authority.
pub fn rooted(a: TrustAuthority, key_hex: &str) -> Result<(), String> {
    let want = key_hex.trim().to_ascii_lowercase();
    if root_keys_hex(a)?.contains(&want) {
        exclusive(a, &want)
    } else {
        Err(format!(
            "key {}… is not in the operator's {} root ({}): a store may name a key, only the \
             operator root makes it authority",
            want.get(..16).unwrap_or(&want),
            a.dir_name(),
            authority_root(a).0.display()
        ))
    }
}

// ── axon-evidence-signature/2: ONE implementation (Fabric and the loop) ─────

/// `/2`: DOMAIN-SEPARATED. The signed message is
/// `axon-evidence-signature/2\n<authority>\n<exact bytes>`, and the signature
/// names its authority, so a key trusted for one purpose never validates a
/// statement of another.
pub const EVIDENCE_SIGNATURE_SCHEMA: &str = "axon-evidence-signature/2";

/// The exact message an `axon-evidence-signature/2` for `authority` signs.
pub fn evidence_signing_message(authority: TrustAuthority, bytes: &[u8]) -> Vec<u8> {
    let mut m = format!("{EVIDENCE_SIGNATURE_SCHEMA}\n{}\n", authority.dir_name()).into_bytes();
    m.extend_from_slice(bytes);
    m
}

/// `ed25519:<first 16 hex of sha256(public key)>`.
pub fn key_fingerprint(pk: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let h: String = Sha256::digest(pk)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    format!("ed25519:{}", &h[..16])
}

fn hex_bytes(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

/// Verify the detached signature `sig_text` over `bytes` for `authority`
/// under one of `trusted` (32-byte Ed25519 keys). Returns the signer's
/// fingerprint. Each refusal is its own rule.
pub fn verify_evidence_signature(
    what: &str,
    bytes: &[u8],
    sig_text: &str,
    trusted: &[Vec<u8>],
    authority: TrustAuthority,
) -> Result<String, String> {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let sv: serde_json::Value =
        serde_json::from_str(sig_text).map_err(|e| format!("{what} signature is not JSON: {e}"))?;
    if sv["schema"] != EVIDENCE_SIGNATURE_SCHEMA || sv["alg"] != "ed25519" {
        return Err(format!(
            "{what} signature is not {EVIDENCE_SIGNATURE_SCHEMA} with alg ed25519"
        ));
    }
    // RULE:authority-domain
    if sv["domain"] != authority.dir_name() {
        return Err(format!(
            "{what} signature is for authority {}, not {}: a key trusted for one purpose \
             never validates another",
            sv["domain"],
            authority.dir_name()
        ));
    }
    let pk = sv["public_key"]
        .as_str()
        .and_then(hex_bytes)
        .filter(|k| k.len() == 32)
        .ok_or(format!("{what} signature has no 32-byte public_key"))?;
    let sig = sv["signature"]
        .as_str()
        .and_then(hex_bytes)
        .filter(|s| s.len() == 64)
        .ok_or(format!("{what} signature has no 64-byte signature"))?;
    // RULE:issuer-trusted
    if !trusted.contains(&pk) {
        return Err(format!(
            "{what} is signed by {}, which is not a trusted evidence issuer",
            key_fingerprint(&pk)
        ));
    }
    // RULE:signature-verifies
    if UnparsedPublicKey::new(&ED25519, &pk)
        .verify(&evidence_signing_message(authority, bytes), &sig)
        .is_err()
    {
        return Err(format!(
            "{what} signature does not verify under {}: the bytes are not the ones the issuer \
             signed",
            key_fingerprint(&pk)
        ));
    }
    Ok(key_fingerprint(&pk))
}

/// The keys `a`'s operator root trusts in THIS process, as raw bytes, after
/// the ownership walk (always, except under a test root). A root holding a
/// key that another operator root also holds is refused whole ([`exclusive`]).
pub fn rooted_keys(a: TrustAuthority) -> Result<Vec<Vec<u8>>, String> {
    let keys = root_keys_hex(a)?;
    for k in &keys {
        exclusive(a, k)?;
    }
    Ok(keys.iter().filter_map(|h| hex_bytes(h)).collect())
}
