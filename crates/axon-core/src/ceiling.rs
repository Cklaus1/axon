//! ceilings CE-2 (`tasks/ceilings-spec.md`): `axon-ceiling/1`, one
//! operator-authored file that bounds a run.
//!
//! The file is supplied with `--ceiling FILE` (on `run`/`session`/`goal`/`test`)
//! or `AXON_CEILING=FILE`, and read ONCE per process, before any program code
//! runs. It is an operator file, not R38's public embedding contract: its schema
//! may change under a version bump (NH-C1 A1).
//!
//! ```json
//! { "schema": "axon-ceiling/1",
//!   "effects": ["IO", "AI"],
//!   "fs_read": ["/srv/job/in/"], "fs_write": ["/srv/job/out/"],
//!   "net": ["api.anthropic.com"],
//!   "ai_tokens": 200000, "max_depth": 6000 }
//! ```
//!
//! Rules (spec §8.4), all failing closed with **E1007** / exit 2:
//! - every key is required; `null` means explicitly unbounded, except
//!   `fs_write`, which must be a list (`[]` = no writes);
//! - an unknown key, a duplicate key, a malformed value, an effect name that is
//!   not an effect, or an unreadable file is refused, so a typo like `stpes`
//!   can never disarm a ceiling;
//! - the file must not lie inside its own `fs_write` scope, so a run cannot
//!   rewrite the ceiling of the next run.
//!
//! Each field NARROWS the existing mechanism it maps to (the ambient sandbox,
//! the AI endpoint pin, the run token cap, the recursion depth); the narrowest
//! of the environment variable and the ceiling wins, and the program can widen
//! neither.
//!
//! The parser is hand-written: the default build does not link `serde_json`,
//! and the lenient extractors in `interp/provenance.rs` are not fit for an
//! authority file.

use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// The schema tag a ceiling file must carry.
pub const SCHEMA: &str = "axon-ceiling/1";

/// Every key of `axon-ceiling/1`, in documentation order. Each key enters this
/// list in the commit that enforces it, so no build accepts a key it ignores.
pub const KEYS: &[&str] = &[
    "schema",
    "effects",
    "fs_read",
    "fs_write",
    "net",
    "ai_tokens",
    "max_depth",
];

/// A parsed, validated ceiling. `None` in a bound means the file said `null`
/// (explicitly unbounded) or, for a scope list, contained `"*"`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ceiling {
    /// The canonical path the file was read from.
    pub path: PathBuf,
    /// Lowercase hex sha256 of the file's bytes, recorded in `run_start`.
    pub sha256: String,
    pub effects: Option<Vec<String>>,
    pub fs_read: Option<Vec<String>>,
    pub fs_write: Option<Vec<String>>,
    pub net: Option<Vec<String>>,
    pub ai_tokens: Option<i64>,
    pub max_depth: Option<usize>,
}

/// One JSON value of the flat schema.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Null,
    Int(u64),
    Str(String),
    List(Vec<String>),
}

struct P<'a> {
    s: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.s.len() && matches!(self.s[self.i], b' ' | b'\t' | b'\n' | b'\r') {
            self.i += 1;
        }
    }

    fn peek(&mut self) -> Option<u8> {
        self.ws();
        self.s.get(self.i).copied()
    }

    fn eat(&mut self, c: u8) -> Result<(), String> {
        match self.peek() {
            Some(x) if x == c => {
                self.i += 1;
                Ok(())
            }
            Some(x) => Err(format!(
                "expected `{}` at byte {}, found `{}`",
                c as char, self.i, x as char
            )),
            None => Err(format!("expected `{}`, found end of file", c as char)),
        }
    }

    fn string(&mut self) -> Result<String, String> {
        self.eat(b'"')?;
        let mut out: Vec<u8> = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return Err("unterminated string".to_string());
            };
            self.i += 1;
            match c {
                b'"' => break,
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return Err("unterminated escape".to_string());
                    };
                    self.i += 1;
                    match e {
                        b'"' | b'\\' | b'/' => out.push(e),
                        b'n' => out.push(b'\n'),
                        b't' => out.push(b'\t'),
                        b'r' => out.push(b'\r'),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'u' => {
                            let hex = self
                                .s
                                .get(self.i..self.i + 4)
                                .and_then(|h| std::str::from_utf8(h).ok())
                                .and_then(|h| u32::from_str_radix(h, 16).ok())
                                .ok_or("malformed \\u escape")?;
                            self.i += 4;
                            let ch = char::from_u32(hex)
                                .ok_or("a \\u escape outside the basic plane is not accepted")?;
                            let mut buf = [0u8; 4];
                            out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                        }
                        _ => return Err(format!("unknown escape `\\{}`", e as char)),
                    }
                }
                c if c < 0x20 => return Err("control character inside a string".to_string()),
                c => out.push(c),
            }
        }
        String::from_utf8(out).map_err(|_| "a string is not UTF-8".to_string())
    }

    fn value(&mut self) -> Result<Val, String> {
        match self.peek() {
            Some(b'n') => {
                if self.s[self.i..].starts_with(b"null") {
                    self.i += 4;
                    Ok(Val::Null)
                } else {
                    Err(format!("unexpected token at byte {}", self.i))
                }
            }
            Some(b'"') => Ok(Val::Str(self.string()?)),
            Some(b'[') => {
                self.i += 1;
                let mut items = Vec::new();
                if self.peek() == Some(b']') {
                    self.i += 1;
                    return Ok(Val::List(items));
                }
                loop {
                    if self.peek() != Some(b'"') {
                        return Err("a list may hold only strings".to_string());
                    }
                    items.push(self.string()?);
                    match self.peek() {
                        Some(b',') => self.i += 1,
                        Some(b']') => {
                            self.i += 1;
                            return Ok(Val::List(items));
                        }
                        _ => return Err("expected `,` or `]` in a list".to_string()),
                    }
                }
            }
            Some(c) if c.is_ascii_digit() => {
                let start = self.i;
                while self.i < self.s.len() && self.s[self.i].is_ascii_digit() {
                    self.i += 1;
                }
                if matches!(self.s.get(self.i), Some(b'.' | b'e' | b'E')) {
                    return Err("a bound must be a whole number".to_string());
                }
                let digits = std::str::from_utf8(&self.s[start..self.i]).unwrap_or("");
                if digits.len() > 1 && digits.starts_with('0') {
                    return Err(format!("a number with a leading zero: `{digits}`"));
                }
                digits
                    .parse::<u64>()
                    .map(Val::Int)
                    .map_err(|_| format!("`{digits}` is out of range"))
            }
            Some(b'-') => Err("a bound may not be negative".to_string()),
            Some(c) => Err(format!(
                "unexpected `{}` at byte {} (values are null, a whole number, a string or a list of strings)",
                c as char, self.i
            )),
            None => Err("unexpected end of file".to_string()),
        }
    }

    fn object(&mut self) -> Result<Vec<(String, Val)>, String> {
        self.eat(b'{')?;
        let mut out: Vec<(String, Val)> = Vec::new();
        if self.peek() == Some(b'}') {
            self.i += 1;
        } else {
            loop {
                let k = self.string()?;
                self.eat(b':')?;
                let v = self.value()?;
                if out.iter().any(|(x, _)| *x == k) {
                    return Err(format!("duplicate key `{k}`"));
                }
                out.push((k, v));
                match self.peek() {
                    Some(b',') => self.i += 1,
                    Some(b'}') => {
                        self.i += 1;
                        break;
                    }
                    _ => return Err("expected `,` or `}` after a value".to_string()),
                }
            }
        }
        if self.peek().is_some() {
            return Err(format!("trailing data at byte {}", self.i));
        }
        Ok(out)
    }
}

/// A scope list: `"*"` means unbounded (the `parse_scope_list` spelling the
/// environment variables use); an empty entry is refused.
fn scope_list(key: &str, v: Val) -> Result<Option<Vec<String>>, String> {
    match v {
        Val::Null => Ok(None),
        Val::List(items) => {
            if items.iter().any(|x| x.trim().is_empty()) {
                return Err(format!("`{key}` holds an empty entry"));
            }
            let items: Vec<String> = items.into_iter().map(|x| x.trim().to_string()).collect();
            if items.iter().any(|x| x == "*") {
                return Ok(None);
            }
            Ok(Some(items))
        }
        _ => Err(format!("`{key}` must be a list of strings or null")),
    }
}

fn whole(key: &str, v: Val) -> Result<Option<u64>, String> {
    match v {
        Val::Null => Ok(None),
        Val::Int(n) => Ok(Some(n)),
        _ => Err(format!("`{key}` must be a whole number or null")),
    }
}

/// Parse and validate a ceiling's bytes. `path` is the file's canonical
/// location (used for the self-overlap rule), `cwd` resolves relative
/// `fs_write` prefixes the way the interpreter resolves relative paths.
pub fn parse(bytes: &[u8], path: &Path, cwd: &Path) -> Result<Ceiling, String> {
    let fields = P { s: bytes, i: 0 }.object()?;
    for (k, _) in &fields {
        if !KEYS.contains(&k.as_str()) {
            return Err(format!(
                "unknown key `{k}` (the keys are: {})",
                KEYS.join(", ")
            ));
        }
    }
    let missing: Vec<&str> = KEYS
        .iter()
        .copied()
        .filter(|k| !fields.iter().any(|(x, _)| x == k))
        .collect();
    if !missing.is_empty() {
        return Err(format!(
            "missing key(s) {} - every key is required; write `null` for explicitly unbounded",
            missing
                .iter()
                .map(|k| format!("`{k}`"))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let mut c = Ceiling {
        path: path.to_path_buf(),
        sha256: sha256_hex(bytes),
        effects: None,
        fs_read: None,
        fs_write: None,
        net: None,
        ai_tokens: None,
        max_depth: None,
    };
    for (k, v) in fields {
        match k.as_str() {
            "schema" => {
                if v != Val::Str(SCHEMA.to_string()) {
                    return Err(format!("`schema` must be \"{SCHEMA}\""));
                }
            }
            "effects" => {
                c.effects = match v {
                    Val::Null => None,
                    Val::List(items) => {
                        if let Some(bad) = items
                            .iter()
                            .find(|e| !crate::builtins::is_grantable_effect(e))
                        {
                            return Err(format!(
                                "`effects` names `{bad}`, which is not an effect (valid: {})",
                                crate::builtins::GRANTABLE_EFFECTS.join(", ")
                            ));
                        }
                        Some(items)
                    }
                    _ => return Err("`effects` must be a list of effect names or null".into()),
                }
            }
            "fs_read" => c.fs_read = scope_list("fs_read", v)?,
            "fs_write" => {
                if v == Val::Null {
                    return Err(
                        "`fs_write` must be a list (`[]` = no writes); null is not accepted".into(),
                    );
                }
                c.fs_write = scope_list("fs_write", v)?;
            }
            "net" => c.net = scope_list("net", v)?,
            "ai_tokens" => {
                c.ai_tokens = match whole("ai_tokens", v)? {
                    Some(n) => Some(i64::try_from(n).map_err(|_| "`ai_tokens` is out of range")?),
                    None => None,
                }
            }
            "max_depth" => {
                c.max_depth = match whole("max_depth", v)? {
                    Some(0) => return Err("`max_depth` must be at least 1".into()),
                    Some(n) => Some(usize::try_from(n).map_err(|_| "`max_depth` is out of range")?),
                    None => None,
                }
            }
            _ => unreachable!("unknown keys were refused above"),
        }
    }
    refuse_self_overlap(&c, cwd)?;
    Ok(c)
}

/// The ceiling file may not lie inside its own `fs_write` scope (spec CE-2
/// "Self-protection"): a run could otherwise rewrite the ceiling of the next.
fn refuse_self_overlap(c: &Ceiling, cwd: &Path) -> Result<(), String> {
    let Some(write) = &c.fs_write else {
        return Err(
            "`fs_write` is unbounded (`*`), which covers the ceiling file itself".to_string(),
        );
    };
    let file = c.path.to_string_lossy().to_string();
    for p in write {
        let abs = if p.starts_with('/') {
            PathBuf::from(p)
        } else {
            cwd.join(p)
        };
        let dir_suffix = if p.ends_with('/') { "/" } else { "" };
        let mut forms = vec![abs.to_string_lossy().to_string()];
        if let Ok(canon) = std::fs::canonicalize(&abs) {
            forms.push(format!("{}{dir_suffix}", canon.to_string_lossy()));
        }
        if forms
            .iter()
            .any(|f| crate::capabilities::path_is_under(&file, f))
        {
            return Err(format!(
                "the ceiling file {file} lies inside its own `fs_write` scope `{p}` - a run could rewrite it"
            ));
        }
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Read and validate the ceiling at `raw` (a path as the operator wrote it).
pub fn load(raw: &Path) -> Result<Ceiling, String> {
    let cwd =
        std::env::current_dir().map_err(|e| format!("cannot read the working directory: {e}"))?;
    let canon =
        std::fs::canonicalize(raw).map_err(|e| format!("cannot read {}: {e}", raw.display()))?;
    let bytes = std::fs::read(&canon).map_err(|e| format!("cannot read {}: {e}", raw.display()))?;
    parse(&bytes, &canon, &cwd)
}

/// The run's ceiling, read once from `AXON_CEILING`. `None` when it is unset.
///
/// A set but invalid ceiling is E1007: the message goes to stderr and the
/// process exits 2 before any program code runs (the executing verbs call this
/// first, and every interpreter reads it while it is built, so no path runs a
/// program under a ceiling it could not read).
pub fn active() -> Option<&'static Ceiling> {
    static CEILING: LazyLock<Option<Ceiling>> = LazyLock::new(|| {
        let raw = std::env::var_os("AXON_CEILING")?;
        match load(Path::new(&raw)) {
            Ok(c) => Some(c),
            Err(why) => {
                eprintln!(
                    "error[E1007]: ceiling {}: {why}; refusing to run",
                    Path::new(&raw).display()
                );
                std::process::exit(2);
            }
        }
    });
    CEILING.as_ref()
}

/// The narrower of two path-prefix scopes (`None` = unbounded). A prefix
/// survives when it lies under some prefix of the other list, so every path
/// the result admits is admitted by both.
pub(crate) fn meet_paths(a: Option<Vec<String>>, b: Option<&Vec<String>>) -> Option<Vec<String>> {
    use crate::capabilities::path_is_under;
    match (a, b) {
        (None, None) => None,
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b.clone()),
        (Some(a), Some(b)) => {
            let mut out: Vec<String> = Vec::new();
            for x in a.iter().filter(|x| b.iter().any(|g| path_is_under(x, g))) {
                if !out.contains(x) {
                    out.push(x.clone());
                }
            }
            for y in b.iter().filter(|y| a.iter().any(|g| path_is_under(y, g))) {
                if !out.contains(y) {
                    out.push(y.clone());
                }
            }
            Some(out)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = r#"{ "schema": "axon-ceiling/1", "effects": ["IO"], "fs_read": null,
        "fs_write": [], "net": ["api.anthropic.com"], "ai_tokens": 100, "max_depth": null }"#;

    fn p(src: &str) -> Result<Ceiling, String> {
        parse(
            src.as_bytes(),
            Path::new("/c/ceiling.json"),
            Path::new("/w"),
        )
    }

    #[test]
    fn a_full_ceiling_parses() {
        let c = p(FULL).expect("valid");
        assert_eq!(c.effects, Some(vec!["IO".to_string()]));
        assert_eq!(c.fs_write, Some(vec![]));
        assert_eq!(c.ai_tokens, Some(100));
        assert_eq!(c.max_depth, None);
        assert_eq!(c.sha256.len(), 64);
    }

    #[test]
    fn malformed_values_are_refused() {
        for (from, to) in [
            (r#""ai_tokens": 100"#, r#""ai_tokens": -1"#),
            (r#""ai_tokens": 100"#, r#""ai_tokens": 1.5"#),
            (r#""ai_tokens": 100"#, r#""ai_tokens": "100""#),
            (r#""effects": ["IO"]"#, r#""effects": ["IOO"]"#),
            (r#""effects": ["IO"]"#, r#""effects": [1]"#),
            (r#""max_depth": null"#, r#""max_depth": 0"#),
            (
                r#""schema": "axon-ceiling/1""#,
                r#""schema": "axon-ceiling/2""#,
            ),
            (r#""net": ["api.anthropic.com"]"#, r#""net": [""]"#),
        ] {
            let src = FULL.replace(from, to);
            assert!(p(&src).is_err(), "accepted: {src}");
        }
        assert!(p(&format!("{FULL} x")).is_err(), "trailing data accepted");
        assert!(
            p(&FULL.replace(r#""net":"#, r#""net": null, "net":"#)).is_err(),
            "duplicate key accepted"
        );
    }

    #[test]
    fn meet_paths_keeps_the_narrower_prefix() {
        let a = Some(vec!["/srv/".to_string(), "/tmp/x/".to_string()]);
        let b = vec!["/srv/in/".to_string()];
        assert_eq!(meet_paths(a, Some(&b)), Some(vec!["/srv/in/".to_string()]));
        assert_eq!(meet_paths(None, Some(&b)), Some(b.clone()));
        assert_eq!(
            meet_paths(Some(vec!["/etc/".into()]), Some(&b)),
            Some(vec![])
        );
    }
}
