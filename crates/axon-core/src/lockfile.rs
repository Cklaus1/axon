//! `axon.lock` — content-addressed import lockfile (R6).
//!
//! The lockfile pins each `use`d module to a content hash, so a build either
//! sees exactly the bytes that were audited or fails. This module is the
//! pure foundation: the `axh1:` hash, the [`LockEntry`] record, and a
//! deterministic TOML serializer/parser for the narrow `axon.lock` schema.
//! No general TOML parser and no external crate — the schema is fixed
//! (`version` + an array of `[[module]]` tables of string fields), so a small
//! hand-rolled reader/writer keeps the dependency surface at zero and makes
//! the output deterministic by construction.
//!
//! CLI wiring (`axon add` / `axon lock` / `--locked`) and the import-time hash
//! check live in follow-on slices; this module owns the *format* and the
//! *hash*, which the spec (R6 §4.1/§4.2) says to settle first.

use crate::error::E1205;

/// The lockfile schema version this build writes and accepts.
pub const LOCK_VERSION: u32 = 1;

/// Algorithm tag for the content hash. Versioned so a future migration (e.g.
/// BLAKE3) is `axh2:` without ambiguity (R6 §4.1).
pub const HASH_PREFIX: &str = "axh1:";

/// Content hash of a module's **raw source bytes** — `axh1:`-tagged hex
/// SHA-256. Hashing the bytes (not the AST) is deliberate: bytes are what a
/// human reviews and what a tamper changes, and it decouples identity from
/// parser internals (an I-2 hazard). See R6 §4.1.
pub fn module_hash(source: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(source);
    format!("{HASH_PREFIX}{:x}", h.finalize())
}

/// One resolved import in `axon.lock`. Mirrors the `[[module]]` table in
/// R6 §3.2. `caps` is the module's declared capability surface (the union of
/// its fns' `@[contained]` declarations); `audit` is the hash of the
/// AI-audit record that cleared it (empty until the audit slice lands).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockEntry {
    /// The `use` path, e.g. `scorelib::metric`.
    pub name: String,
    /// `axh1:`-tagged content hash of the resolved bytes.
    pub hash: String,
    /// Where the bytes came from, e.g. `file:./vendor/scorelib/metric.ax`.
    pub source: String,
    /// Declared capability surface (allowlist strings, e.g. `fs:read(./data/)`).
    pub caps: Vec<String>,
    /// Hash of the AI-audit record that cleared this module (empty if none yet).
    pub audit: String,
}

impl LockEntry {
    /// A minimal entry: name + hash + source, no caps, no audit yet.
    pub fn new(
        name: impl Into<String>,
        hash: impl Into<String>,
        source: impl Into<String>,
    ) -> Self {
        LockEntry {
            name: name.into(),
            hash: hash.into(),
            source: source.into(),
            caps: Vec::new(),
            audit: String::new(),
        }
    }
}

/// Serialize a lockfile to its canonical TOML text. Deterministic: entries are
/// emitted in **sorted name order** and fields in a fixed order, so the
/// committed file's diff is meaningful and a re-`lock` of unchanged inputs is a
/// no-op (R6 §4.2).
pub fn write_lock(modules: &[LockEntry]) -> String {
    let mut sorted: Vec<&LockEntry> = modules.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(&b.name));

    let mut out = String::new();
    out.push_str("# axon.lock — generated; commit this file. Do not edit by hand.\n");
    out.push_str(&format!("version = {LOCK_VERSION}\n"));
    for m in sorted {
        out.push_str("\n[[module]]\n");
        out.push_str(&format!("name = {}\n", toml_str(&m.name)));
        out.push_str(&format!("hash = {}\n", toml_str(&m.hash)));
        out.push_str(&format!("source = {}\n", toml_str(&m.source)));
        out.push_str(&format!("caps = {}\n", toml_str_array(&m.caps)));
        out.push_str(&format!("audit = {}\n", toml_str(&m.audit)));
    }
    out
}

/// Parsed lockfile: the schema version plus its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockFile {
    pub version: u32,
    pub modules: Vec<LockEntry>,
}

/// Parse `axon.lock` text. Accepts exactly the schema `write_lock` produces
/// (a `version` line plus `[[module]]` tables). On any malformed input or an
/// unknown `version`, returns `Err((E1205, message))` — the caller surfaces
/// it as the coded diagnostic (R6 §6).
pub fn parse_lock(text: &str) -> Result<LockFile, (&'static str, String)> {
    let mut version: Option<u32> = None;
    let mut modules: Vec<LockEntry> = Vec::new();
    let mut cur: Option<LockEntry> = None;

    // Flush the in-progress table into `modules`, validating required fields.
    fn flush(
        cur: Option<LockEntry>,
        modules: &mut Vec<LockEntry>,
    ) -> Result<(), (&'static str, String)> {
        if let Some(e) = cur {
            if e.name.is_empty() || e.hash.is_empty() {
                return Err((
                    E1205,
                    format!(
                        "axon.lock [[module]] is missing required `name`/`hash` (got name={:?})",
                        e.name
                    ),
                ));
            }
            modules.push(e);
        }
        Ok(())
    }

    for (lineno, raw) in text.lines().enumerate() {
        let line = strip_comment(raw).trim();
        if line.is_empty() {
            continue;
        }
        if line == "[[module]]" {
            flush(cur.take(), &mut modules)?;
            cur = Some(LockEntry::new("", "", ""));
            continue;
        }
        // `key = value`
        let Some((key, val)) = line.split_once('=') else {
            return Err((
                E1205,
                format!(
                    "axon.lock line {}: expected `key = value` or `[[module]]`, got `{line}`",
                    lineno + 1
                ),
            ));
        };
        let key = key.trim();
        let val = val.trim();
        if key == "version" {
            let v: u32 = val.parse().map_err(|_| {
                (
                    E1205,
                    format!("axon.lock has a non-numeric version `{val}`"),
                )
            })?;
            if v != LOCK_VERSION {
                return Err((E1205, format!(
                    "axon.lock is version {v}, but this build only understands {LOCK_VERSION} — regenerate with `axon lock`"
                )));
            }
            version = Some(v);
            continue;
        }
        let Some(e) = cur.as_mut() else {
            return Err((
                E1205,
                format!(
                    "axon.lock line {}: `{key}` appears before any [[module]]",
                    lineno + 1
                ),
            ));
        };
        match key {
            "name" => e.name = parse_toml_str(val).map_err(tag1205)?,
            "hash" => e.hash = parse_toml_str(val).map_err(tag1205)?,
            "source" => e.source = parse_toml_str(val).map_err(tag1205)?,
            "audit" => e.audit = parse_toml_str(val).map_err(tag1205)?,
            "caps" => e.caps = parse_toml_str_array(val).map_err(tag1205)?,
            other => {
                return Err((
                    E1205,
                    format!("axon.lock line {}: unknown key `{other}`", lineno + 1),
                ));
            }
        }
    }
    flush(cur.take(), &mut modules)?;

    let Some(version) = version else {
        return Err((E1205, "axon.lock is missing a `version` line".to_string()));
    };
    Ok(LockFile { version, modules })
}

fn tag1205(msg: String) -> (&'static str, String) {
    (E1205, msg)
}

// ── Minimal TOML string helpers (just what the schema needs) ─────────────────

/// Quote a string as a TOML basic string with the escapes the schema can hit.
fn toml_str(s: &str) -> String {
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

fn toml_str_array(items: &[String]) -> String {
    let inner: Vec<String> = items.iter().map(|s| toml_str(s)).collect();
    format!("[{}]", inner.join(", "))
}

/// Parse a TOML basic string (the `"..."` form, with the escapes `toml_str`
/// emits). Returns the unescaped content.
fn parse_toml_str(val: &str) -> Result<String, String> {
    let bytes = val.as_bytes();
    if bytes.len() < 2 || bytes[0] != b'"' || bytes[bytes.len() - 1] != b'"' {
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
                Some(other) => return Err(format!("unknown escape `\\{other}` in string")),
                None => return Err("trailing backslash in string".to_string()),
            }
        } else {
            out.push(c);
        }
    }
    Ok(out)
}

/// Parse a TOML array of basic strings: `["a", "b"]`. Empty `[]` is allowed.
fn parse_toml_str_array(val: &str) -> Result<Vec<String>, String> {
    let val = val.trim();
    if !val.starts_with('[') || !val.ends_with(']') {
        return Err(format!("expected a `[...]` array, got `{val}`"));
    }
    let inner = val[1..val.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }
    // Split on top-level commas (our strings never contain commas unescaped at
    // the array level except inside quotes; split respecting quotes).
    let mut out = Vec::new();
    let mut depth_in_str = false;
    let mut esc = false;
    let mut start = 0usize;
    let bytes = inner.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if esc {
            esc = false;
            continue;
        }
        match b {
            b'\\' if depth_in_str => esc = true,
            b'"' => depth_in_str = !depth_in_str,
            b',' if !depth_in_str => {
                out.push(parse_toml_str(inner[start..i].trim())?);
                start = i + 1;
            }
            _ => {}
        }
    }
    out.push(parse_toml_str(inner[start..].trim())?);
    Ok(out)
}

/// Strip a trailing `# comment` not inside a string. Lockfiles we write only
/// have full-line comments, but tolerate trailing ones defensively.
fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    let mut esc = false;
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

    #[test]
    fn hash_is_axh1_tagged_and_stable() {
        let h1 = module_hash(b"fn main() -> i64 { 0 }");
        let h2 = module_hash(b"fn main() -> i64 { 0 }");
        assert_eq!(h1, h2, "same bytes must hash identically");
        assert!(
            h1.starts_with("axh1:"),
            "hash must carry the axh1: tag: {h1}"
        );
        // axh1: (5) + 64 hex chars.
        assert_eq!(h1.len(), 5 + 64, "axh1: + 64-hex SHA-256: {h1}");
    }

    #[test]
    fn hash_differs_on_a_one_byte_change() {
        let a = module_hash(b"fn main() -> i64 { 0 }");
        let b = module_hash(b"fn main() -> i64 { 1 }");
        assert_ne!(a, b, "a one-byte change must change the hash");
    }

    #[test]
    fn lockfile_roundtrips_deterministically() {
        let modules = vec![
            LockEntry {
                name: "scorelib::metric".into(),
                hash: module_hash(b"fn metric() -> i64 { 1 }"),
                source: "file:./vendor/scorelib/metric.ax".into(),
                caps: vec!["fs:read(./data/)".into()],
                audit: String::new(),
            },
            LockEntry::new("alib::a", module_hash(b"fn a() {}"), "file:./a.ax"),
        ];
        let text = write_lock(&modules);
        let parsed = parse_lock(&text).expect("round-trips");
        assert_eq!(parsed.version, LOCK_VERSION);
        // Parsed back == input, but in sorted name order (alib before scorelib).
        assert_eq!(parsed.modules.len(), 2);
        assert_eq!(parsed.modules[0].name, "alib::a");
        assert_eq!(parsed.modules[1].name, "scorelib::metric");
        assert_eq!(parsed.modules[1].caps, vec!["fs:read(./data/)".to_string()]);
        // Re-serializing the parse yields byte-identical text (determinism).
        assert_eq!(
            write_lock(&parsed.modules),
            text,
            "write∘parse∘write is stable"
        );
    }

    #[test]
    fn write_is_sorted_regardless_of_input_order() {
        let a = vec![
            LockEntry::new("zed", "axh1:z", "file:z"),
            LockEntry::new("abe", "axh1:a", "file:a"),
        ];
        let b = vec![
            LockEntry::new("abe", "axh1:a", "file:a"),
            LockEntry::new("zed", "axh1:z", "file:z"),
        ];
        assert_eq!(
            write_lock(&a),
            write_lock(&b),
            "input order must not affect output"
        );
    }

    #[test]
    fn malformed_lockfile_is_e1205() {
        // Unknown version.
        let (code, _) = parse_lock("version = 99\n").unwrap_err();
        assert_eq!(code, E1205);
        // Missing version entirely.
        let (code, _) = parse_lock("[[module]]\nname = \"x\"\nhash = \"axh1:y\"\n").unwrap_err();
        assert_eq!(code, E1205);
        // Garbage line.
        let (code, _) = parse_lock("version = 1\nthis is not toml\n").unwrap_err();
        assert_eq!(code, E1205);
        // key before any [[module]].
        let (code, _) = parse_lock("version = 1\nname = \"x\"\n").unwrap_err();
        assert_eq!(code, E1205);
        // [[module]] missing its hash.
        let (code, _) = parse_lock("version = 1\n[[module]]\nname = \"x\"\n").unwrap_err();
        assert_eq!(code, E1205);
    }

    #[test]
    fn caps_array_roundtrips_including_empty() {
        let modules = vec![LockEntry {
            name: "m".into(),
            hash: "axh1:h".into(),
            source: "file:m".into(),
            caps: vec!["fs:read(./a/)".into(), "net:api.x".into()],
            audit: "axh1-audit:7c".into(),
        }];
        let parsed = parse_lock(&write_lock(&modules)).unwrap();
        assert_eq!(
            parsed.modules[0].caps,
            vec!["fs:read(./a/)".to_string(), "net:api.x".to_string()]
        );
        assert_eq!(parsed.modules[0].audit, "axh1-audit:7c");
    }
}
