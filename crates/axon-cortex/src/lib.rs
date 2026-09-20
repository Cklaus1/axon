//! Axon Cortex — typed contracts for the M0/M1 repair-episode slice.
//!
//! Scope is deliberately narrow: the types the package's own "first executable
//! vertical slice" needs (CX-00 §contract, CX-02 observation, CX-03 grants,
//! CX-10 evidence), not all twenty specs. Source:
//! `docs/axon_cortex_v0_7/axon-cortex-build-v0_7/schemas/PROTOCOLS.md`.
//!
//! Four rules from that document are enforced here rather than described,
//! because each is the kind of contract that decays into a comment:
//!
//! 1. **"Absent, null/None, empty and Unknown are not interchangeable."**
//!    [`Observed`] has no `Default`, and `Unknown` carries a REASON — so a
//!    missing fact cannot be silently rendered as a present one. This is the
//!    absent-vs-passed collapse the rest of this repo keeps finding, stated as
//!    a type.
//! 2. **"Enums are closed within a schema version; unknown variants refuse
//!    rather than default."** Deserialization is `deny_unknown_fields` and
//!    every enum is closed; an unrecognised variant is an error, never a
//!    fallback.
//! 3. **"Do not accept NaN/infinity or ambiguous duplicate keys."** Checked on
//!    ingest by [`parse_strict`], because serde accepts both by default.
//! 4. **"Content hashes identify artifacts; they do not authenticate
//!    issuers."** [`content_digest`] is named and documented as identity only.

use serde::{Deserialize, Serialize};

pub mod action;
#[cfg(feature = "ai")]
pub mod ai;
pub mod episode;
pub mod generate;
pub mod locate;
pub mod runner;
pub mod select;

/// `axc1:`-tagged SHA-256 over canonical bytes.
///
/// IDENTITY, not authenticity: it says two artifacts are the same artifact, and
/// says nothing about who produced either. An issuer check is a separate
/// signature/registry lookup that this crate deliberately does not pretend to
/// provide.
pub fn content_digest(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    format!("axc1:{:x}", h.finalize())
}

/// A fact the observer either knows or explicitly does not.
///
/// There is no `Default` and no `unwrap_or`-shaped convenience on purpose. A
/// partial observer "supplies unknown type/parse facts rather than fabricated
/// defaults" (PROTOCOLS.md), and the only way to make that hold under later
/// edits is to leave no cheap path from `Unknown` to a plausible value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum Observed<T> {
    Known {
        value: T,
    },
    /// Not merely absent — absent WITH A REASON, which is what makes an
    /// omission auditable instead of indistinguishable from a null.
    Unknown {
        reason: String,
    },
}

impl<T> Observed<T> {
    pub fn known(value: T) -> Self {
        Observed::Known { value }
    }
    pub fn unknown(reason: impl Into<String>) -> Self {
        Observed::Unknown {
            reason: reason.into(),
        }
    }
    /// Deliberately returns `Option` rather than a default: the caller must
    /// decide what an unknown means in ITS context. A `value_or(default)` here
    /// would re-open exactly the hole this type exists to close.
    pub fn value(&self) -> Option<&T> {
        match self {
            Observed::Known { value } => Some(value),
            Observed::Unknown { .. } => None,
        }
    }
    pub fn is_unknown(&self) -> bool {
        matches!(self, Observed::Unknown { .. })
    }
}

/// A content-addressed snapshot of the workspace region an episode may touch.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSnapshot {
    pub snapshot_id: String,
    pub parent_snapshot_id: Option<String>,
    /// Path → content digest. Sorted on canonicalisation so the snapshot's own
    /// digest is stable across map iteration order.
    pub files: Vec<(String, String)>,
    /// What the observer was allowed to look at. An episode that reasons beyond
    /// this scope is reasoning about things it never observed.
    pub observation_scope: Vec<String>,
}

impl WorkspaceSnapshot {
    /// The snapshot's identity, over canonical bytes with `files` sorted.
    pub fn digest(&self) -> String {
        let mut files = self.files.clone();
        files.sort();
        let canon = files
            .iter()
            .map(|(p, d)| format!("{p}\u{0}{d}"))
            .collect::<Vec<_>>()
            .join("\u{1}");
        // The SCOPE and the PARENT are covered too.
        //
        // Hashing `files` alone left this unable to witness most of what it is
        // presented as identifying. `observation_scope` decides what the
        // episode is entitled to reason about — its own doc says an episode
        // reasoning beyond it "is reasoning about things it never observed" —
        // and two snapshots over different scopes hashed identically whenever
        // the extra paths happened to be absent. `parent_snapshot_id` is what
        // makes a chain of states auditable rather than a set of orphans, and
        // it could be rewritten to any value with every recorded digest still
        // validating.
        //
        // The scope is sorted so the digest names the SET, not the order the
        // caller happened to pass — the same property the file list already
        // had.
        let mut scope = self.observation_scope.clone();
        scope.sort();
        content_digest(
            format!(
                "{canon}\u{2}{}\u{2}{}",
                scope.join("\u{1}"),
                self.parent_snapshot_id.as_deref().unwrap_or("<root>")
            )
            .as_bytes(),
        )
    }
}

/// One observation over a snapshot. Carries what was NOT seen alongside what
/// was — an observation with no omission report is indistinguishable from a
/// claim of completeness.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Observation {
    pub observation_id: String,
    pub snapshot_id: String,
    /// ERRORS only. A diagnostic here means the program does not compile.
    pub diagnostics: Vec<String>,
    /// WARNINGS. Separate from `diagnostics` because they mean something
    /// different: the program compiles AND the checker has something to say
    /// about it.
    ///
    /// This channel used to not exist. `observe()` filtered for
    /// `"severity":"error"` and discarded everything else, so a program with
    /// `W0005 unreachable code` was observed as `diagnostics: []`,
    /// `compiles: true` — indistinguishable from a clean one. An agent
    /// repairing code through Cortex could not see that the checker had found
    /// dead code, because "no errors" had been rendered as "nothing to report".
    ///
    /// That is the absent-vs-empty collapse this crate exists to refuse, so it
    /// is refused here: warnings are carried, not dropped, and the
    /// `compiles_cleanly` fact distinguishes a clean compile from a warned one.
    pub warnings: Vec<String>,
    pub facts: Vec<(String, Observed<String>)>,
    /// Why each omission happened. Required, not optional.
    pub omission_report: Vec<String>,
}

/// Errors from strict ingest. Every variant is a REFUSAL; none is a fallback.
#[derive(Debug, Clone, PartialEq)]
pub enum ContractError {
    /// A closed enum met a variant it does not define, or a struct met an
    /// unknown field.
    UnknownVariantOrField(String),
    /// NaN or ±infinity. Not representable in protocol JSON and not silently
    /// coerced.
    NonFiniteNumber(String),
    /// The same key twice — ambiguous, so refused rather than last-wins.
    DuplicateKey(String),
    Malformed(String),
}

impl std::fmt::Display for ContractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ContractError::UnknownVariantOrField(s) => {
                write!(f, "unknown variant or field: {s}")
            }
            ContractError::NonFiniteNumber(s) => write!(f, "non-finite number: {s}"),
            ContractError::DuplicateKey(s) => write!(f, "duplicate key: {s}"),
            ContractError::Malformed(s) => write!(f, "malformed: {s}"),
        }
    }
}

impl std::error::Error for ContractError {}

/// Parse protocol JSON under the document's stated rules.
///
/// `serde_json` accepts duplicate keys (last wins) and, with the right feature
/// flags, non-finite floats. Both are explicitly forbidden by PROTOCOLS.md, and
/// neither is caught by `deny_unknown_fields`, so they are checked here — on
/// the raw text, before typed parsing, since by then the ambiguity is gone.
pub fn parse_strict<T: for<'de> Deserialize<'de>>(json: &str) -> Result<T, ContractError> {
    if let Some(tok) = ["NaN", "Infinity", "-Infinity"]
        .iter()
        .find(|t| json.contains(**t))
    {
        return Err(ContractError::NonFiniteNumber((*tok).to_string()));
    }
    if let Some(k) = first_duplicate_key(json) {
        return Err(ContractError::DuplicateKey(k));
    }
    serde_json::from_str(json).map_err(|e| {
        let m = e.to_string();
        if m.contains("unknown field") || m.contains("unknown variant") {
            ContractError::UnknownVariantOrField(m)
        } else {
            ContractError::Malformed(m)
        }
    })
}

/// First key repeated within the SAME object, or `None`.
///
/// Tracks a stack of per-object key sets: the same key name in two sibling
/// objects is ordinary and must not be flagged, which a flat scan would get
/// wrong.
fn first_duplicate_key(json: &str) -> Option<String> {
    let b = json.as_bytes();
    let mut stack: Vec<std::collections::HashSet<String>> = Vec::new();
    let mut i = 0usize;
    let mut in_str = false;
    let mut cur = String::new();
    let mut last_string: Option<String> = None;
    while i < b.len() {
        match b[i] {
            b'\\' if in_str => i += 2,
            b'"' => {
                if in_str {
                    last_string = Some(std::mem::take(&mut cur));
                } else {
                    cur.clear();
                }
                in_str = !in_str;
                i += 1;
            }
            _ if in_str => {
                cur.push(b[i] as char);
                i += 1;
            }
            b'{' => {
                stack.push(std::collections::HashSet::new());
                i += 1;
            }
            b'}' => {
                stack.pop();
                i += 1;
            }
            b':' => {
                if let (Some(k), Some(top)) = (last_string.take(), stack.last_mut()) {
                    if !top.insert(k.clone()) {
                        return Some(k);
                    }
                }
                i += 1;
            }
            _ => i += 1,
        }
    }
    None
}

/// Canonical serialization: the bytes a digest is taken over.
///
/// `serde_json::to_string` on a struct emits declaration order, which is stable
/// for these types; the sorting that matters is inside [`WorkspaceSnapshot::digest`],
/// where the payload is a map in disguise.
pub fn to_canonical_json<T: Serialize>(v: &T) -> Result<String, ContractError> {
    serde_json::to_string(v).map_err(|e| ContractError::Malformed(e.to_string()))
}

#[cfg(test)]
mod contract_tests {
    use super::*;
    use crate::episode::{Episode, EpisodeEvent};

    fn snap() -> WorkspaceSnapshot {
        WorkspaceSnapshot {
            snapshot_id: "s1".into(),
            parent_snapshot_id: None,
            files: vec![
                ("b.ax".into(), "axc1:bb".into()),
                ("a.ax".into(), "axc1:aa".into()),
            ],
            observation_scope: vec!["./src/".into()],
        }
    }

    /// CRITICAL-PATH GATE (round-trip byte identity). C2 is the node every
    /// downstream task serialises through, so the extra proof beyond a normal
    /// regression test is that the wire form is a fixed point: parse and
    /// re-emit must reproduce the exact bytes, or two components that agree on
    /// the types can still disagree on the artifact digest.
    #[test]
    fn cxg_c2_round_trip_is_byte_identical() {
        let s = snap();
        let once = to_canonical_json(&s).unwrap();
        let back: WorkspaceSnapshot = parse_strict(&once).unwrap();
        let twice = to_canonical_json(&back).unwrap();
        assert_eq!(
            once, twice,
            "serialize→parse→serialize must be a fixed point"
        );
        assert_eq!(back, s);

        let mut ep = Episode::new("e1");
        ep.push(EpisodeEvent::Snapshot {
            snapshot_id: "s1".into(),
            snapshot_digest: s.digest(),
        });
        ep.push(EpisodeEvent::Verified {
            passed: true,
            detail: "checks green".into(),
        });
        let a = to_canonical_json(&ep).unwrap();
        let b: Episode = parse_strict(&a).unwrap();
        assert_eq!(a, to_canonical_json(&b).unwrap());
        assert_eq!(ep.digest().unwrap(), b.digest().unwrap());
    }

    /// A snapshot's identity must not depend on the order its files were
    /// listed in — otherwise the same workspace hashes two ways and the digest
    /// stops identifying anything.
    #[test]
    fn cxg_c2_snapshot_digest_is_order_independent() {
        let a = snap();
        let mut b = snap();
        b.files.reverse();
        assert_ne!(a.files, b.files, "precondition: the orders differ");
        assert_eq!(a.digest(), b.digest());
    }

    /// PROTOCOLS.md: "Absent, null/None, empty and Unknown are not
    /// interchangeable." Four distinguishable states, asserted as four
    /// distinguishable values.
    #[test]
    fn cxg_c2_unknown_is_not_absent_is_not_empty() {
        let unknown: Observed<String> = Observed::unknown("parse failed at line 3");
        let empty = Observed::known(String::new());
        assert!(unknown.is_unknown());
        assert!(!empty.is_unknown());
        assert_ne!(unknown, empty);
        // An unknown yields no value at all — not a default, not "".
        assert_eq!(unknown.value(), None);
        assert_eq!(empty.value(), Some(&String::new()));
        // ...and the REASON survives the wire, which is what makes an omission
        // auditable rather than merely absent.
        let j = to_canonical_json(&unknown).unwrap();
        let back: Observed<String> = parse_strict(&j).unwrap();
        assert_eq!(back, unknown);
        assert!(j.contains("parse failed at line 3"));
    }

    /// "Enums are closed within a schema version; unknown variants refuse
    /// rather than default."
    #[test]
    fn cxg_c2_unknown_variant_and_field_refuse() {
        let bad_variant = r#"{"state":"probably","value":"x"}"#;
        assert!(matches!(
            parse_strict::<Observed<String>>(bad_variant),
            Err(ContractError::UnknownVariantOrField(_))
        ));
        let bad_field = r#"{"snapshot_id":"s","parent_snapshot_id":null,"files":[],"observation_scope":[],"extra":1}"#;
        assert!(matches!(
            parse_strict::<WorkspaceSnapshot>(bad_field),
            Err(ContractError::UnknownVariantOrField(_))
        ));
    }

    /// "Do not accept NaN/infinity or ambiguous duplicate keys."
    #[test]
    fn cxg_c2_non_finite_and_duplicate_keys_refuse() {
        let nan = r#"{"snapshot_id":"s","score":NaN}"#;
        assert!(matches!(
            parse_strict::<WorkspaceSnapshot>(nan),
            Err(ContractError::NonFiniteNumber(_))
        ));
        let dup = r#"{"snapshot_id":"a","snapshot_id":"b","files":[],"observation_scope":[]}"#;
        assert!(matches!(
            parse_strict::<WorkspaceSnapshot>(dup),
            Err(ContractError::DuplicateKey(_))
        ));
    }

    /// The duplicate-key check must not fire on the same key in SIBLING
    /// objects, which is ordinary JSON. A checker that cries wolf on valid
    /// input gets disabled, and then it protects nothing.
    #[test]
    fn cxg_c2_repeated_keys_in_sibling_objects_are_fine() {
        let ok = r#"{"episode_id":"e","events":[{"kind":"check_run","name":"a","exit_code":0,"passed":true},{"kind":"check_run","name":"b","exit_code":1,"passed":false}]}"#;
        let ep: Episode = parse_strict(ok).expect("sibling objects may repeat keys");
        assert_eq!(ep.events.len(), 2);
    }

    /// Absent verification is not success — the same rule as an absent fact.
    #[test]
    fn cxg_c2_an_unverified_episode_is_not_ok() {
        let mut ep = Episode::new("e");
        ep.push(EpisodeEvent::CheckRun {
            name: "repro".into(),
            exit_code: 0,
            passed: true,
        });
        assert!(
            !ep.verified_ok(),
            "passing checks are not an independent verdict"
        );
        ep.push(EpisodeEvent::Verified {
            passed: false,
            detail: "verifier disagreed".into(),
        });
        assert!(!ep.verified_ok(), "a FAILED verdict is not success either");
        ep.push(EpisodeEvent::Verified {
            passed: true,
            detail: "ok".into(),
        });
        assert!(ep.verified_ok());
    }

    /// Event ORDER is part of episode identity: "checked then patched" and
    /// "patched then checked" are different claims about what was verified.
    #[test]
    fn cxg_c2_event_order_changes_episode_identity() {
        let patch = EpisodeEvent::PatchApplied {
            path: "a.ax".into(),
            before_digest: "axc1:1".into(),
            after_digest: "axc1:2".into(),
        };
        let check = EpisodeEvent::CheckRun {
            name: "t".into(),
            exit_code: 0,
            passed: true,
        };
        let mut a = Episode::new("e");
        a.push(patch.clone());
        a.push(check.clone());
        let mut b = Episode::new("e");
        b.push(check);
        b.push(patch);
        assert_ne!(a.digest().unwrap(), b.digest().unwrap());
    }

    /// A content digest identifies; it does not authenticate. Asserted so the
    /// distinction survives someone later reaching for it as a trust signal.
    #[test]
    fn cxg_c2_digest_identifies_but_does_not_authenticate() {
        let d1 = content_digest(b"same bytes");
        let d2 = content_digest(b"same bytes");
        assert_eq!(d1, d2, "identity: same bytes, same name");
        assert_ne!(d1, content_digest(b"other bytes"));
        assert!(d1.starts_with("axc1:"));
        // Nothing in this crate maps a digest to an issuer — deliberately.
    }
}
