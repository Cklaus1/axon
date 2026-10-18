//! R1 — a repair candidate cannot acquire a capability the original code did
//! not hold (invariant I-12).
//!
//! Two layers, both derived from `axon caps` (the walker `@[contained]` uses,
//! so the fence cannot disagree with the language's own containment) on the
//! PRISTINE file, before any candidate exists:
//!
//! - the STATIC GRANT, per repair target: the capabilities and effects the
//!   target fn transitively reaches. A candidate body is run through the same
//!   walker in memory (nothing written) and refused before it executes if it
//!   reaches anything outside it ([`StaticGrant::excess`]).
//! - the RUNTIME FENCE, per file: the environment every spawn that executes
//!   workspace code carries ([`Fence::env`]) — the interpreter's effect ceiling
//!   narrowed to the free set plus what the pristine file uses, and the
//!   ambient filesystem/network scopes set EMPTY (deny all) for every kind the
//!   pristine file does not use.
//!
//! Nothing here is a fabricated `axon_os::Grant`: what is recorded is what was
//! derived, from which basis, and the exact environment applied.

use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The schema `axon caps --json` answers with.
pub const CAPS_SCHEMA: &str = "axon-caps/1";

/// Effects always granted: console IO, randomness, time, channels and pure
/// code. Everything else enters the ceiling only if the pristine file uses it.
pub const FREE_EFFECTS: &[&str] = &["Chan", "IO", "Pure", "Random", "Time"];

/// The ambient scope variables, by the capability label that keeps each one
/// unset. Unset = unrestricted; set-but-empty = deny all of that kind.
pub const SCOPE_VARS: &[(&str, &str)] = &[
    ("fs:read", "AXON_ALLOWED_FS_READ"),
    ("fs:write", "AXON_ALLOWED_FS_WRITE"),
    ("net", "AXON_ALLOWED_NET"),
];

/// The interpreter's effect ceiling variable.
pub const EFFECTS_VAR: &str = "AXON_ALLOWED_EFFECTS";

/// One `axon caps --json` answer.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Caps {
    pub capabilities: Vec<String>,
    pub effects: Vec<String>,
    /// The walker met something it cannot classify: deny by default.
    pub unknown: bool,
}

/// Why `axon caps` produced no answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CapsError {
    /// Exit 2: the program does not parse or type-check (or names no such fn).
    /// For a candidate that is the compile failure it would meet anyway.
    DidNotCompile(String),
    /// Anything else: the walker could not be run or answered nonsense. An
    /// environment fact, never a verdict about the code.
    Failed(String),
}

impl std::fmt::Display for CapsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CapsError::DidNotCompile(s) => write!(f, "it does not compile: {s}"),
            CapsError::Failed(s) => write!(f, "`axon caps` could not be run: {s}"),
        }
    }
}

/// Read `axon caps --json`'s output and exit code.
pub fn parse_caps(text: &str, code: Option<i32>) -> Result<Caps, CapsError> {
    match code {
        Some(0) => {}
        Some(2) => return Err(CapsError::DidNotCompile(bounded(text.trim()))),
        other => {
            return Err(CapsError::Failed(format!(
                "exit {other:?}: {}",
                bounded(text.trim())
            )))
        }
    }
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with('{'))
        .ok_or_else(|| CapsError::Failed("no JSON line in its output".into()))?;
    let v: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| CapsError::Failed(format!("unreadable answer ({e})")))?;
    if v["schema"] != CAPS_SCHEMA {
        return Err(CapsError::Failed(format!(
            "answered schema {}, not {CAPS_SCHEMA}",
            v["schema"]
        )));
    }
    serde_json::from_value(v).map_err(|e| CapsError::Failed(format!("unreadable answer ({e})")))
}

/// `axon caps` on a file on disk (`fn_name`: one fn, transitively).
pub fn caps_of_file(
    axon: &Path,
    file: &Path,
    fn_name: Option<&str>,
    wall: std::time::Duration,
) -> Result<Caps, CapsError> {
    let (text, code) = crate::runner::run_axon_caps(axon, file, None, fn_name, wall)
        .map_err(|e| CapsError::Failed(e.to_string()))?;
    parse_caps(&text, code)
}

/// `axon caps` on program TEXT that is not in the workspace — a candidate,
/// before anything is written. The text goes to a private temporary file
/// (removed afterwards); its imports resolve against `base_dir`, the
/// directory of the file it would replace.
pub fn caps_of_text(
    axon: &Path,
    text: &str,
    base_dir: &Path,
    fn_name: Option<&str>,
    wall: std::time::Duration,
) -> Result<Caps, CapsError> {
    use std::io::Write;
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let tmp = std::env::temp_dir().join(format!(
        "cortex-caps-{}-{nanos}-{}.ax",
        std::process::id(),
        SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let written = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .and_then(|mut f| f.write_all(text.as_bytes()));
    let answer = match written {
        Ok(()) => crate::runner::run_axon_caps(axon, &tmp, Some(base_dir), fn_name, wall)
            .map_err(|e| CapsError::Failed(e.to_string()))
            .and_then(|(out, code)| parse_caps(&out, code)),
        Err(e) => Err(CapsError::Failed(format!(
            "the candidate could not be staged at {}: {e}",
            tmp.display()
        ))),
    };
    let _ = std::fs::remove_file(&tmp);
    answer
}

/// What a grant rests on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Basis {
    /// `axon caps` classified everything it reached.
    AxonCaps,
    /// It met something it cannot classify: no fence is derived from it.
    Unknown,
}

impl Basis {
    pub fn as_str(&self) -> &'static str {
        match self {
            Basis::AxonCaps => "axon-caps",
            Basis::Unknown => "unknown",
        }
    }
}

/// The runtime fence of one pristine file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fence {
    pub basis: Basis,
    /// The pristine file's capabilities and effects, as `axon caps` said.
    pub capabilities: Vec<String>,
    pub effects: Vec<String>,
    /// Exactly the environment applied to every spawn that executes workspace
    /// code. Empty under [`Basis::Unknown`]: nothing is narrowed on a basis
    /// that could not see everything — and empty when not applied at all.
    pub env: BTreeMap<String, String>,
    /// False when the executor the checks run on does not take the fence
    /// (the Fabric executor): then `env` is empty, never a claim.
    pub applied: bool,
}

impl Fence {
    /// Derive the fence from the pristine file's `axon caps` answer.
    pub fn from_pristine(caps: &Caps) -> Fence {
        let mut env = BTreeMap::new();
        let basis = if caps.unknown {
            Basis::Unknown
        } else {
            let effects: BTreeSet<&str> = FREE_EFFECTS
                .iter()
                .copied()
                .chain(caps.effects.iter().map(String::as_str))
                .collect();
            env.insert(
                EFFECTS_VAR.to_string(),
                effects.into_iter().collect::<Vec<_>>().join(","),
            );
            for (label, var) in SCOPE_VARS {
                if !caps.capabilities.iter().any(|c| c == label) {
                    env.insert((*var).to_string(), String::new());
                }
            }
            Basis::AxonCaps
        };
        Fence {
            basis,
            capabilities: caps.capabilities.clone(),
            effects: caps.effects.clone(),
            env,
            applied: true,
        }
    }

    /// This fence, recorded as NOT applied: its environment is dropped so
    /// nothing reports a narrowing that did not happen.
    pub fn not_applied(mut self) -> Fence {
        self.env.clear();
        self.applied = false;
        self
    }

    /// sha256 of the sorted `NAME=value\n` lines of [`Fence::env`].
    pub fn sha256(&self) -> String {
        let lines: String = self.env.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        crate::label::sha256_hex(lines.as_bytes())
    }

    /// The `grant` object of `cortex-repair/1` and `cortex-grant/1`, with the
    /// per-target static grants beside the file's fence.
    pub fn to_json(&self, targets: &[StaticGrant]) -> serde_json::Value {
        serde_json::json!({
            "capabilities": self.capabilities,
            "effects": self.effects,
            "basis": self.basis.as_str(),
            "applied": self.applied,
            "fence_env": self.env,
            "fence_sha256": self.sha256(),
            "targets": targets.iter().map(StaticGrant::to_json).collect::<Vec<_>>(),
        })
    }

    /// The same record as an episode event.
    pub fn event(&self, targets: &[StaticGrant]) -> crate::episode::EpisodeEvent {
        crate::episode::EpisodeEvent::CapabilityGrant {
            basis: self.basis.as_str().to_string(),
            capabilities: self.capabilities.clone(),
            effects: self.effects.clone(),
            fence_env: self.env.clone(),
            fence_sha256: self.sha256(),
            targets: targets
                .iter()
                .map(|g| match g.basis {
                    Basis::Unknown => format!("{}=?", g.symbol),
                    Basis::AxonCaps => format!(
                        "{}={}",
                        g.symbol,
                        g.capabilities.iter().cloned().collect::<Vec<_>>().join(",")
                    ),
                })
                .collect(),
        }
    }
}

/// The static grant of one repair target.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticGrant {
    pub symbol: String,
    pub basis: Basis,
    pub capabilities: BTreeSet<String>,
    /// Every effect the target reaches (free ones included, for the record).
    pub effects: BTreeSet<String>,
}

impl StaticGrant {
    pub fn from_caps(symbol: &str, caps: &Caps) -> StaticGrant {
        StaticGrant {
            symbol: symbol.to_string(),
            basis: if caps.unknown {
                Basis::Unknown
            } else {
                Basis::AxonCaps
            },
            capabilities: caps.capabilities.iter().cloned().collect(),
            effects: caps.effects.iter().cloned().collect(),
        }
    }

    /// The kinds a candidate reaches that this grant does not hold: its
    /// capability labels, then `effect:NAME` for a non-free effect not already
    /// named by a capability (`exec` ↔ `Exec`, `net` ↔ `Net`), or `unknown`
    /// when the walker could not classify the candidate (deny by default).
    /// Empty = admitted. A grant on an [`Basis::Unknown`] basis admits
    /// everything: there is no static fence for that target.
    pub fn excess(&self, candidate: &Caps) -> Vec<String> {
        if self.basis == Basis::Unknown {
            return Vec::new();
        }
        if candidate.unknown {
            return vec!["unknown".to_string()];
        }
        let mut out: Vec<String> = candidate
            .capabilities
            .iter()
            .filter(|c| !self.capabilities.contains(*c))
            .cloned()
            .collect();
        for e in &candidate.effects {
            let named = matches!(
                (
                    e.as_str(),
                    out.iter().any(|c| c == "exec"),
                    out.iter().any(|c| c == "net")
                ),
                ("Exec", true, _) | ("Net", _, true)
            );
            if !FREE_EFFECTS.contains(&e.as_str()) && !self.effects.contains(e) && !named {
                out.push(format!("effect:{e}"));
            }
        }
        out.sort();
        out.dedup();
        out
    }

    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "symbol": self.symbol,
            "basis": self.basis.as_str(),
            "capabilities": self.capabilities,
            "effects": self.effects,
        })
    }
}

/// The fenced kind a test failure message names, when the failure is the
/// fence refusing something (the interpreter's sandbox scope or effect
/// ceiling): `fs:read`, `fs:write`, `net` or `effect:NAME`. `None` for any
/// other failure.
pub fn fence_violation(message: &str) -> Option<String> {
    if message.contains("the active sandbox restricts it to") {
        return Some(
            if message.contains("is not permitted to write") {
                "fs:write"
            } else if message.contains("is not permitted to read") {
                "fs:read"
            } else {
                "net"
            }
            .to_string(),
        );
    }
    if message.contains("not in the active sandbox's allowed set") {
        let eff = message
            .split_once("requires effect `")
            .and_then(|(_, rest)| rest.split_once('`'))
            .map(|(e, _)| e)
            .unwrap_or("?");
        return Some(format!("effect:{eff}"));
    }
    None
}

fn bounded(s: &str) -> String {
    const MAX: usize = 600;
    if s.len() <= MAX {
        return s.to_string();
    }
    let mut end = MAX;
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}…", &s[..end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn caps(c: &[&str], e: &[&str], unknown: bool) -> Caps {
        Caps {
            capabilities: c.iter().map(|s| s.to_string()).collect(),
            effects: e.iter().map(|s| s.to_string()).collect(),
            unknown,
        }
    }

    #[test]
    fn the_fence_denies_every_scope_the_pristine_file_does_not_use() {
        let f = Fence::from_pristine(&caps(&["fs:read"], &["IO", "Net"], false));
        assert_eq!(
            f.env["AXON_ALLOWED_EFFECTS"],
            "Chan,IO,Net,Pure,Random,Time"
        );
        assert_eq!(f.env.get("AXON_ALLOWED_FS_READ"), None, "used: left unset");
        assert_eq!(
            f.env["AXON_ALLOWED_FS_WRITE"], "",
            "unused: set EMPTY, deny all"
        );
        assert_eq!(f.env["AXON_ALLOWED_NET"], "");
        assert_eq!(f.basis, Basis::AxonCaps);
        assert_eq!(f.sha256().len(), 64);
    }

    #[test]
    fn an_unknown_pristine_file_gets_no_fence() {
        let f = Fence::from_pristine(&caps(&[], &["IO"], true));
        assert!(f.env.is_empty());
        assert_eq!(f.basis, Basis::Unknown);
    }

    #[test]
    fn a_candidate_reaching_past_its_grant_is_named_by_kind() {
        let g = StaticGrant::from_caps("double", &caps(&[], &["IO"], false));
        assert!(g.excess(&caps(&[], &["IO", "Random"], false)).is_empty());
        assert_eq!(
            g.excess(&caps(&["fs:write"], &["IO"], false)),
            vec!["fs:write"]
        );
        assert_eq!(g.excess(&caps(&["exec"], &["Exec"], false)), vec!["exec"]);
        assert_eq!(g.excess(&caps(&[], &["AI"], false)), vec!["effect:AI"]);
        assert_eq!(g.excess(&caps(&[], &[], true)), vec!["unknown"]);
        let open = StaticGrant::from_caps("double", &caps(&[], &[], true));
        assert!(open.excess(&caps(&["net"], &[], false)).is_empty());
    }

    #[test]
    fn a_sandbox_refusal_names_its_kind() {
        assert_eq!(
            fence_violation(
                "builtin `write_file` is not permitted to write path `/x`: the active \
                 sandbox restricts it to [] (principal handle 0)"
            )
            .as_deref(),
            Some("fs:write")
        );
        assert_eq!(
            fence_violation(
                "builtin `exec` requires effect `Exec` which is not in the active \
                 sandbox's allowed set {\"IO\"} (principal handle 0)"
            )
            .as_deref(),
            Some("effect:Exec")
        );
        assert_eq!(fence_violation("assertion failed: 3 != 4"), None);
    }

    #[test]
    fn caps_output_is_read_strictly() {
        let ok = parse_caps(
            "{\"schema\":\"axon-caps/1\",\"file\":\"-\",\"fn\":null,\"capabilities\":[\"net\"],\"effects\":[\"Net\"],\"unknown\":false}\n",
            Some(0),
        )
        .unwrap();
        assert_eq!(ok.capabilities, vec!["net"]);
        assert!(matches!(
            parse_caps("E0001 boom", Some(2)),
            Err(CapsError::DidNotCompile(_))
        ));
        assert!(matches!(
            parse_caps("{\"schema\":\"x\"}", Some(0)),
            Err(CapsError::Failed(_))
        ));
        assert!(matches!(parse_caps("", None), Err(CapsError::Failed(_))));
    }
}
