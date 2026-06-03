//! `passes.manifest` — the graduated-pass manifest (R10 §3 / §4.5).
//!
//! This is the **graduation firewall** that makes a self-improving compiler
//! safe: a learned optimization pass may only run if it is in this manifest,
//! and a pass enters the manifest only by `graduate`, which requires a green
//! verification record AND **multi-sig of ≥2 distinct root Principals** (I-12).
//! The compiler proposes; humans (a quorum authority) dispose. Discovery and
//! proposal are unprivileged — only graduation grants execution.
//!
//! Structurally: "a pass NOT in the manifest never runs" (R10 §4.5). This
//! module owns the manifest *format* and the *graduation gate*; the
//! verification it consumes is produced by [`crate::improve`]. Like
//! [`crate::lockfile`], the TOML is hand-rolled and deterministic (zero deps,
//! sorted order) so the committed manifest's diff is the security review.

use crate::error::E1404;

/// Manifest schema version this build writes and accepts.
pub const MANIFEST_VERSION: u32 = 1;

/// Algorithm-tagged content hash of a pass definition (`axp1:` = Axon Pass v1).
pub const PASS_HASH_PREFIX: &str = "axp1:";
/// Algorithm-tagged content hash of a verification record (`axv1:`).
pub const VERIFY_HASH_PREFIX: &str = "axv1:";
/// Algorithm-tagged content hash of a corpus (`axc1:`).
pub const CORPUS_HASH_PREFIX: &str = "axc1:";

/// Minimum distinct root-Principal signatures required to graduate a pass.
/// The compiler cannot graduate its own passes — at least two *different*
/// authorities must sign (R10 §4.5, ROADMAP §7 multi-sig update path).
pub const MIN_GRADUATION_SIGS: usize = 2;

fn tagged_hash(prefix: &str, bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("{prefix}{:x}", h.finalize())
}

/// Content hash of a pass definition's bytes (`axp1:`-tagged SHA-256). A pass is
/// identified by *what it is*, so a changed pass is a different id and must be
/// re-verified and re-graduated.
pub fn pass_hash(definition: &[u8]) -> String {
    tagged_hash(PASS_HASH_PREFIX, definition)
}

/// Content hash of a verification record (`axv1:`-tagged). Pins the exact record
/// that cleared the pass; the manifest references it so the audit trail is
/// tamper-evident.
pub fn verify_hash(record_bytes: &[u8]) -> String {
    tagged_hash(VERIFY_HASH_PREFIX, record_bytes)
}

/// Content hash of the corpus the pass was verified against (`axc1:`-tagged).
/// Computed over the corpus members' bytes in a fixed order so two identical
/// corpora hash identically (the "verified against the whole corpus, named"
/// guarantee, R10 §4.2).
pub fn corpus_hash(members: &[Vec<u8>]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update((members.len() as u64).to_le_bytes());
    for m in members {
        h.update((m.len() as u64).to_le_bytes());
        h.update(m);
    }
    format!("{CORPUS_HASH_PREFIX}{:x}", h.finalize())
}

/// Recorded perf status of a graduated pass (mirrors [`crate::improve::PerfStatus`]
/// but as the stable on-disk string). A pass may graduate `unmeasured`; only a
/// pass whose G4 timing actually ran and won may claim `faster`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PerfClaim {
    Unmeasured,
    Faster,
}

impl PerfClaim {
    fn as_str(&self) -> &'static str {
        match self {
            PerfClaim::Unmeasured => "unmeasured",
            PerfClaim::Faster => "faster",
        }
    }
    fn parse(s: &str) -> Option<Self> {
        match s {
            "unmeasured" => Some(PerfClaim::Unmeasured),
            "faster" => Some(PerfClaim::Faster),
            _ => None,
        }
    }
}

/// One graduated pass (R10 §3 `[[pass]]`). Content-addressed and carrying its
/// verification provenance + the quorum that graduated it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PassEntry {
    /// `axp1:` content hash of the pass definition — its identity.
    pub id: String,
    /// Human-readable name (e.g. `fold_const_add`).
    pub name: String,
    /// `axv1:` hash of the verification record that cleared it.
    pub verified: String,
    /// `axc1:` hash of the corpus it was verified against.
    pub corpus_hash: String,
    /// The root Principals who signed off (multi-sig; ≥2 distinct to graduate).
    pub graduated_by: Vec<String>,
    /// Whether the pass earned the `faster` claim (G4) or is unmeasured.
    pub perf_status: PerfClaim,
    /// How this pass was DISCOVERED — `static` (deterministic detector), `mock`,
    /// or `ai:<model>` (a live model proposed it). Recorded so a multi-sig
    /// reviewer at graduation time can apply higher scrutiny to AI-origin passes
    /// (red-team must-fix #3: the human gate needs visibility into proposal
    /// origin). Defaults to `static` when absent (back-compat with older
    /// manifests that predate AI discovery).
    pub proposed_by: String,
}

/// Why a graduation was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraduationError {
    pub code: &'static str,
    pub message: String,
}

/// Attempt to build a graduated [`PassEntry`] — the I-12 gate. Refuses with
/// **E1404** unless at least [`MIN_GRADUATION_SIGS`] *distinct* root Principals
/// signed. Distinctness matters: one authority signing twice is not a quorum,
/// and self-graduation (the compiler signing as itself) cannot reach two
/// distinct human/root signatures. A green verification record is a
/// precondition the caller must establish (passing `verify` is necessary, not
/// sufficient — this gate is the second, human half).
#[allow(clippy::too_many_arguments)]
pub fn graduate(
    id: impl Into<String>,
    name: impl Into<String>,
    verified: impl Into<String>,
    corpus_hash: impl Into<String>,
    signers: &[String],
    perf_status: PerfClaim,
    proposed_by: impl Into<String>,
) -> Result<PassEntry, GraduationError> {
    let distinct: std::collections::BTreeSet<&String> = signers.iter().collect();
    if distinct.len() < MIN_GRADUATION_SIGS {
        return Err(GraduationError {
            code: E1404,
            message: format!(
                "graduation requires multi-sig of ≥{MIN_GRADUATION_SIGS} distinct root Principals, \
                 got {} distinct signature(s) {:?} — the compiler cannot graduate its own passes; \
                 a quorum of root authorities must sign (I-12)",
                distinct.len(),
                signers,
            ),
        });
    }
    let proposed_by = proposed_by.into();
    Ok(PassEntry {
        id: id.into(),
        name: name.into(),
        verified: verified.into(),
        corpus_hash: corpus_hash.into(),
        graduated_by: signers.to_vec(),
        perf_status,
        proposed_by: if proposed_by.is_empty() { "static".to_string() } else { proposed_by },
    })
}

/// The graduated-pass set. A pass executes iff it is present here.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Manifest {
    pub version: u32,
    pub passes: Vec<PassEntry>,
}

impl Manifest {
    pub fn new() -> Self {
        Manifest { version: MANIFEST_VERSION, passes: Vec::new() }
    }

    /// Whether a pass id is graduated — i.e. permitted to run. The single
    /// authority on execution permission (R10 §4.5: not in the manifest → never
    /// runs).
    pub fn is_graduated(&self, pass_id: &str) -> bool {
        self.passes.iter().any(|p| p.id == pass_id)
    }

    /// Add a graduated entry (id-unique; re-graduating an id replaces it).
    pub fn insert(&mut self, entry: PassEntry) {
        self.passes.retain(|p| p.id != entry.id);
        self.passes.push(entry);
    }

    /// Remove a graduated pass by id (the `revert` reversibility guarantee,
    /// gate-3). Returns true if it was present.
    pub fn revert(&mut self, pass_id: &str) -> bool {
        let before = self.passes.len();
        self.passes.retain(|p| p.id != pass_id);
        self.passes.len() != before
    }
}

/// Serialize the manifest to canonical TOML (deterministic: passes in sorted-id
/// order, fixed field order). The committed manifest diff *is* the security
/// review of what gained execution permission.
pub fn write_manifest(m: &Manifest) -> String {
    let mut sorted: Vec<&PassEntry> = m.passes.iter().collect();
    sorted.sort_by(|a, b| a.id.cmp(&b.id));

    let mut out = String::new();
    out.push_str("# passes.manifest — graduated optimization passes (R10). Commit this file.\n");
    out.push_str("# A pass NOT in this manifest never runs. Entries are added only by\n");
    out.push_str("# `axon improve graduate`, which requires multi-sig of root Principals.\n");
    out.push_str(&format!("version = {MANIFEST_VERSION}\n"));
    for p in sorted {
        out.push_str("\n[[pass]]\n");
        out.push_str(&format!("id = {}\n", q(&p.id)));
        out.push_str(&format!("name = {}\n", q(&p.name)));
        out.push_str(&format!("verified = {}\n", q(&p.verified)));
        out.push_str(&format!("corpus_hash = {}\n", q(&p.corpus_hash)));
        out.push_str(&format!("graduated_by = {}\n", q_array(&p.graduated_by)));
        out.push_str(&format!("perf_status = {}\n", q(p.perf_status.as_str())));
        out.push_str(&format!("proposed_by = {}\n", q(&p.proposed_by)));
    }
    out
}

/// Parse a `passes.manifest`. Accepts exactly the schema `write_manifest`
/// produces. On malformed input / unknown version, returns
/// `Err((E1405, message))` — a malformed manifest is a TCB-attestation failure
/// (R10 §4.6), so the caller refuses to run rather than guessing.
pub fn parse_manifest(text: &str) -> Result<Manifest, (&'static str, String)> {
    use crate::error::E1405;
    let mut version: Option<u32> = None;
    let mut passes: Vec<PassEntry> = Vec::new();
    let mut cur: Option<PassEntry> = None;

    fn flush(cur: Option<PassEntry>, passes: &mut Vec<PassEntry>) -> Result<(), (&'static str, String)> {
        if let Some(p) = cur {
            if p.id.is_empty() || p.name.is_empty() {
                return Err((E1405, format!(
                    "passes.manifest [[pass]] missing required `id`/`name` (got id={:?})",
                    p.id
                )));
            }
            passes.push(p);
        }
        Ok(())
    }

    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[pass]]" {
            flush(cur.take(), &mut passes)?;
            cur = Some(PassEntry {
                id: String::new(),
                name: String::new(),
                verified: String::new(),
                corpus_hash: String::new(),
                graduated_by: Vec::new(),
                perf_status: PerfClaim::Unmeasured,
                proposed_by: "static".to_string(), // back-compat default if the key is absent
            });
            continue;
        }
        let Some((key, val)) = line.split_once('=') else {
            return Err((E1405, format!(
                "passes.manifest line {}: expected `key = value` or `[[pass]]`, got `{line}`",
                lineno + 1
            )));
        };
        let (key, val) = (key.trim(), val.trim());
        if key == "version" {
            let v: u32 = val.parse().map_err(|_| {
                (E1405, format!("passes.manifest has a non-numeric version `{val}`"))
            })?;
            if v != MANIFEST_VERSION {
                return Err((E1405, format!(
                    "passes.manifest is version {v}, this build understands {MANIFEST_VERSION} — regenerate"
                )));
            }
            version = Some(v);
            continue;
        }
        let Some(p) = cur.as_mut() else {
            return Err((E1405, format!(
                "passes.manifest line {}: `{key}` appears before any [[pass]]",
                lineno + 1
            )));
        };
        match key {
            "id" => p.id = parse_str(val).map_err(tag)?,
            "name" => p.name = parse_str(val).map_err(tag)?,
            "verified" => p.verified = parse_str(val).map_err(tag)?,
            "corpus_hash" => p.corpus_hash = parse_str(val).map_err(tag)?,
            "graduated_by" => p.graduated_by = parse_str_array(val).map_err(tag)?,
            "perf_status" => {
                let s = parse_str(val).map_err(tag)?;
                p.perf_status = PerfClaim::parse(&s).ok_or_else(|| {
                    (E1405, format!("unknown perf_status `{s}` (expected unmeasured|faster)"))
                })?;
            }
            "proposed_by" => p.proposed_by = parse_str(val).map_err(tag)?,
            other => return Err((E1405, format!("passes.manifest line {}: unknown key `{other}`", lineno + 1))),
        }
    }
    flush(cur.take(), &mut passes)?;

    let Some(version) = version else {
        return Err((E1405, "passes.manifest is missing a `version` line".to_string()));
    };
    Ok(Manifest { version, passes })
}

fn tag(msg: String) -> (&'static str, String) {
    (crate::error::E1405, msg)
}

// ── Minimal deterministic TOML string helpers (shared shape with lockfile) ───

fn q(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn q_array(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| q(s)).collect();
    format!("[{}]", inner.join(", "))
}

fn parse_str(val: &str) -> Result<String, String> {
    let b = val.as_bytes();
    if b.len() < 2 || b[0] != b'"' || b[b.len() - 1] != b'"' {
        return Err(format!("expected a quoted string, got `{val}`"));
    }
    let inner = &val[1..val.len() - 1];
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some(other) => return Err(format!("unknown escape `\\{other}`")),
                None => return Err("trailing backslash".to_string()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

fn parse_str_array(val: &str) -> Result<Vec<String>, String> {
    let val = val.trim();
    if !val.starts_with('[') || !val.ends_with(']') {
        return Err(format!("expected a `[...]` array, got `{val}`"));
    }
    let inner = val[1..val.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    let (mut in_str, mut esc, mut start) = (false, false, 0usize);
    for (i, &b) in inner.as_bytes().iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        match b {
            b'\\' if in_str => esc = true,
            b'"' => in_str = !in_str,
            b',' if !in_str => {
                out.push(parse_str(inner[start..i].trim())?);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(parse_str(inner[start..].trim())?);
    Ok(out)
}

fn strip_comment(line: &str) -> &str {
    let (mut in_str, mut esc) = (false, false);
    for (i, c) in line.char_indices() {
        if esc {
            esc = false;
            continue;
        }
        match c {
            '\\' if in_str => esc = true,
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry() -> PassEntry {
        graduate(
            pass_hash(b"fold_const_add v1"),
            "fold_const_add",
            verify_hash(b"record bytes"),
            corpus_hash(&[b"a".to_vec(), b"b".to_vec()]),
            &["principal:root-a".into(), "principal:root-b".into()],
            PerfClaim::Faster,
            "static",
        )
        .expect("two distinct signers graduate")
    }

    #[test]
    fn graduation_requires_multisig() {
        let id = pass_hash(b"p");
        let v = verify_hash(b"r");
        let c = corpus_hash(&[b"x".to_vec()]);

        // Zero signers — refused.
        let e = graduate(&id, "p", &v, &c, &[], PerfClaim::Unmeasured, "static").unwrap_err();
        assert_eq!(e.code, E1404);

        // One signer — refused (no quorum).
        let e = graduate(&id, "p", &v, &c, &["principal:root-a".into()], PerfClaim::Unmeasured, "static")
            .unwrap_err();
        assert_eq!(e.code, E1404);

        // Same authority signing twice — still not two DISTINCT principals.
        let e = graduate(
            &id, "p", &v, &c,
            &["principal:root-a".into(), "principal:root-a".into()],
            PerfClaim::Unmeasured,
            "static",
        )
        .unwrap_err();
        assert_eq!(e.code, E1404, "self-graduation via a duplicate signer must be refused");

        // Two distinct root principals — graduates.
        let ok = graduate(
            &id, "p", &v, &c,
            &["principal:root-a".into(), "principal:root-b".into()],
            PerfClaim::Unmeasured,
            "static",
        );
        assert!(ok.is_ok(), "≥2 distinct signers must graduate: {ok:?}");
    }

    #[test]
    fn manifest_membership_is_execution_permission() {
        let mut m = Manifest::new();
        let e = entry();
        let id = e.id.clone();
        assert!(!m.is_graduated(&id), "ungraduated pass must not be permitted");
        m.insert(e);
        assert!(m.is_graduated(&id), "graduated pass is permitted");
        // revert removes execution permission (gate-3 reversibility).
        assert!(m.revert(&id));
        assert!(!m.is_graduated(&id), "reverted pass must lose permission");
        assert!(!m.revert(&id), "reverting an absent pass returns false");
    }

    #[test]
    fn manifest_roundtrips_deterministically() {
        let mut m = Manifest::new();
        m.insert(entry());
        m.insert(
            graduate(
                pass_hash(b"another"),
                "dce",
                verify_hash(b"r2"),
                corpus_hash(&[b"z".to_vec()]),
                &["principal:root-a".into(), "principal:root-c".into()],
                PerfClaim::Unmeasured,
                "static",
            )
            .unwrap(),
        );
        let text = write_manifest(&m);
        let parsed = parse_manifest(&text).expect("round-trips");
        assert_eq!(parsed.version, MANIFEST_VERSION);
        assert_eq!(parsed.passes.len(), 2);
        // Re-serialize is byte-identical (determinism).
        assert_eq!(write_manifest(&parsed), text);
        // Hashes carry their tags.
        assert!(parsed.passes.iter().all(|p| p.id.starts_with("axp1:")));
    }

    /// Red-team must-fix #3: the discovery origin (`proposed_by`) round-trips
    /// through serialize→parse, so a multi-sig reviewer sees AI provenance; and
    /// an OLD manifest lacking the key still parses (defaulting to `static`).
    #[test]
    fn proposed_by_roundtrips_and_defaults_to_static() {
        let e = graduate(
            pass_hash(b"ai-pass"),
            "fold-arith-identities",
            verify_hash(b"r"),
            corpus_hash(&[b"c".to_vec()]),
            &["principal:root-a".into(), "principal:root-b".into()],
            PerfClaim::Unmeasured,
            "ai:claude-opus-4-8",
        )
        .unwrap();
        assert_eq!(e.proposed_by, "ai:claude-opus-4-8");
        let mut m = Manifest::new();
        m.insert(e);
        let text = write_manifest(&m);
        assert!(text.contains("proposed_by = \"ai:claude-opus-4-8\""), "origin serialized: {text}");
        let parsed = parse_manifest(&text).expect("round-trips");
        assert_eq!(parsed.passes[0].proposed_by, "ai:claude-opus-4-8");

        // Back-compat: a manifest predating the field parses with the default.
        let old = "version = 1\n\n[[pass]]\nid = \"axp1:x\"\nname = \"identity\"\n\
                   verified = \"axv1:y\"\ncorpus_hash = \"axc1:z\"\n\
                   graduated_by = [\"a\", \"b\"]\nperf_status = \"unmeasured\"\n";
        let p = parse_manifest(old).expect("old manifest still parses");
        assert_eq!(p.passes[0].proposed_by, "static", "absent proposed_by defaults to static");
    }

    #[test]
    fn malformed_manifest_is_e1405() {
        use crate::error::E1405;
        assert_eq!(parse_manifest("version = 99\n").unwrap_err().0, E1405);
        assert_eq!(parse_manifest("[[pass]]\nname = \"x\"\n").unwrap_err().0, E1405); // missing version+id
        assert_eq!(parse_manifest("version = 1\nid = \"x\"\n").unwrap_err().0, E1405); // key before [[pass]]
        assert_eq!(
            parse_manifest("version = 1\n[[pass]]\nid = \"a\"\nname = \"n\"\nperf_status = \"bogus\"\n")
                .unwrap_err()
                .0,
            E1405
        );
    }

    #[test]
    fn corpus_hash_is_order_sensitive_and_stable() {
        let a = corpus_hash(&[b"one".to_vec(), b"two".to_vec()]);
        let b = corpus_hash(&[b"one".to_vec(), b"two".to_vec()]);
        let c = corpus_hash(&[b"two".to_vec(), b"one".to_vec()]);
        assert_eq!(a, b, "same corpus hashes identically");
        assert_ne!(a, c, "member order is part of corpus identity");
        assert!(a.starts_with("axc1:"));
    }
}
