//! R6: the record-only repair run ledger (`tasks/r6-ledger-spec.md` §3-§5).
//!
//! Every unattended `cortex repair` run (NH-R1) appends two records here: a
//! `started` record after the envelope is consumed and before any untrusted
//! code runs, and a `terminal` record after the verdict (also from the E3
//! watchdog path). The ledger grants nothing, gates nothing and changes no
//! exit code; [`audit`] (the C1 `axon-intent audit-runs` verb) reads it back.
//!
//! # Records (§3)
//!
//! `cortex-run-record/1`, one canonical-JSON line each (JSONL). The chain:
//! `seq` counts from 0 and `prev` is the `cl22:` digest of the previous line
//! (null at seq 0). `envelope_digest` binds the operator-signed envelope, so
//! a record is never self-authorized: attended runs (no envelope) write none.
//!
//! # Seal (§4)
//!
//! `seal = "axrun1:" + hex HMAC-SHA256(subkey, canonical bytes without seal)`
//! where `subkey = HMAC-SHA256(AXON_APPROVAL_KEY, RUN_RECORD_DOMAIN)` (the G5
//! derivation). The runner holds only this subkey ([`SealKey`]): it can seal
//! records but cannot mint an envelope, a `/3` token or an axon-os record.
//! `ledger.head` is sealed the same way under its own schema
//! (`cortex-run-ledger-head/1`), so a head never reads as a record.
//!
//! # Sink and crash consistency (§4)
//!
//! A dedicated private directory (`--run-ledger DIR`, checked like the spent
//! store) holding `ledger-id` (written once by [`init_ledger`] and bound in the
//! envelope as `run_ledger_id`), `ledger.jsonl` and `ledger.head`. [`append`]
//! runs under an exclusive `flock` on `ledger.jsonl`: append + fsync, then
//! replace `ledger.head` via temp + rename, then fsync the directory.
//!
//! HONEST LIMIT (§4 threat model): rolling the ledger back together with its
//! head is not detected by the ledger alone; C1 cross-checks the spent store,
//! which catches truncation while the store survives. Both directories are
//! same-uid (NH-R1 §5.5).

use crate::private_path::{check_private, Private};
use crate::repair_envelope::{
    decode_hex, hex, is_lower_hex, read_key_file, write_new, Budget, Generator, Task,
    VerifiedEnvelope,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// The run-record subkey's G5 label (§4): distinct from the axon-os record
/// subkey (`"axon-os record v1"`), the `/3` token subkey and the envelope seed.
pub const RUN_RECORD_DOMAIN: &str = "axon-approval repair-run-record v1";

/// The record schema.
pub const RECORD_SCHEMA: &str = "cortex-run-record/1";

/// The head schema: a head is sealed under the same subkey, so its schema
/// keeps it from reading as a record (and the reverse).
pub const HEAD_SCHEMA: &str = "cortex-run-ledger-head/1";

/// The seal's prefix.
pub const SEAL_PREFIX: &str = "axrun1:";

/// Every record's `task_family`.
pub const TASK_FAMILY: &str = "cortex-repair-unattended";

/// The ledger's identity file (bound in the envelope as `run_ledger_id`).
pub const LEDGER_ID_FILE: &str = "ledger-id";

/// The records, one canonical JSON line each.
pub const LEDGER_FILE: &str = "ledger.jsonl";

/// The sealed `{seq, record_digest}` of the last acknowledged record.
pub const HEAD_FILE: &str = "ledger.head";

const HEAD_TMP: &str = "ledger.head.tmp";

/// The E5 journal site of visible-copy create/remove pairs; excluded from
/// the `accepted-mismatch` join (§5).
pub const VISIBLE_SITE: &str = "visible";

/// The exit codes whose terminal record carries an `accepted` artifact (§3).
pub const ACCEPTED_EXITS: [i64; 2] = [0, 27];

/// The ledger directory: private to the runner.
pub const RUN_LEDGER: Private = Private {
    what: "run ledger",
    whose: "the runner's",
    dir: true,
    takeover: "write run records under the runner's name or erase them",
    exposure: "plant, reorder or erase run records",
};

/// A file inside the ledger (`ledger.jsonl`, `ledger.head`, `ledger-id`).
pub const RUN_LEDGER_FILE: Private = Private {
    what: "run ledger file",
    whose: "the runner's",
    dir: false,
    takeover: "rewrite the run records",
    exposure: "rewrite the run records",
};

/// The runner's run-record key file (next to its verify key).
pub const RUN_RECORD_KEY: Private = Private {
    what: "run-record key",
    whose: "the runner's",
    dir: false,
    takeover: "seal records for runs that never happened",
    exposure: "read the key and seal forged records",
};

/// The C1 finding names (§5), each distinct. `terminal-no-started`,
/// `envelope-mismatch` and `used-unreadable` are the build's additions (see
/// the spec's `[REVISED: ...]` notes); `tree-over-cap` is EG-2's (a pinned
/// tree the walk capped: the operator has to look).
pub const FINDINGS: [&str; 14] = [
    "consumed-no-record",
    "started-no-terminal",
    "terminal-no-started",
    "record-not-consumed",
    "duplicate-run",
    "seal-break",
    "chain-break",
    "unacknowledged-tail",
    "head-beyond-ledger",
    "accepted-mismatch",
    "unattributed-change",
    "envelope-mismatch",
    "used-unreadable",
    "tree-over-cap",
];

/// EG-2: the value of a regular file the walk did not hash (its length over
/// the hash bytes left, or a length change while read): `over-cap:<len>`.
pub const OVER_CAP: &str = "over-cap:";

/// EG-2: the reserved tree key (never a path) of a walk that stopped early;
/// its value is `over-cap:manifest` or `over-cap:time`, and later entries
/// are absent.
pub const TREE_STOP_KEY: &str = "";

// ── the subkey ───────────────────────────────────────────────────────────────

/// The run-record HMAC subkey. It seals and checks records; it can derive
/// nothing else. `Debug` is redacted.
#[derive(Clone, PartialEq, Eq)]
pub struct SealKey([u8; 32]);

impl std::fmt::Debug for SealKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SealKey(<redacted>)")
    }
}

impl SealKey {
    /// `subkey = HMAC-SHA256(operator_key, RUN_RECORD_DOMAIN)`; operator key >= 16 bytes.
    pub fn derive(operator_key: &[u8]) -> Result<SealKey, String> {
        if operator_key.len() < 16 {
            return Err(format!(
                "{} is shorter than 16 bytes: refusing to derive the run-record key from it",
                crate::approval::APPROVAL_KEY_ENV
            ));
        }
        Ok(SealKey(axon_attest::hmac_sha256(
            operator_key,
            RUN_RECORD_DOMAIN.as_bytes(),
        )))
    }

    /// The subkey derived from `AXON_APPROVAL_KEY` (the operator side).
    pub fn from_env() -> Result<SealKey, String> {
        SealKey::derive(&crate::repair_envelope::operator_key()?)
    }

    /// A key from 64 lowercase hex characters.
    pub fn from_hex(s: &str) -> Result<SealKey, String> {
        if !is_lower_hex(s, 64) {
            return Err("run-record key is not 64 lowercase hex characters".into());
        }
        let b = decode_hex(s).ok_or("run-record key is not hex")?;
        let mut k = [0u8; 32];
        k.copy_from_slice(&b);
        Ok(SealKey(k))
    }

    /// Lowercase hex, for `--export-run-record-key`.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    /// Load the runner's key file: private to `euid` and outside every path
    /// in `outside` (the rule of [`crate::repair_envelope::VerifyKey::load`]).
    pub fn load(path: &Path, euid: u32, outside: &[PathBuf]) -> Result<SealKey, String> {
        let text = read_key_file(path, euid, &RUN_RECORD_KEY, outside)?;
        SealKey::from_hex(text.trim_end_matches('\n'))
    }

    fn key(&self) -> ring::hmac::Key {
        ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.0)
    }

    /// The seal of `unsealed` (a value with no `seal` field).
    pub fn seal(&self, unsealed: &Value) -> Result<String, String> {
        let bytes =
            axon_canonical::canonical_bytes(unsealed).map_err(|e| format!("run record: {e}"))?;
        let tag = ring::hmac::sign(&self.key(), &bytes);
        Ok(format!("{SEAL_PREFIX}{}", hex(tag.as_ref())))
    }

    /// Check `v`'s `seal` over `v` without it (constant-time compare).
    pub fn check_seal(&self, v: &Value) -> Result<(), String> {
        let mut v = v.clone();
        let seal = match v.as_object_mut().and_then(|o| o.remove("seal")) {
            Some(Value::String(s)) => s,
            Some(_) => return Err("`seal` is not a string".into()),
            None => return Err("no `seal`".into()),
        };
        let tag = seal
            .strip_prefix(SEAL_PREFIX)
            .filter(|t| is_lower_hex(t, 64))
            .and_then(decode_hex)
            .ok_or_else(|| format!("`seal` is not {SEAL_PREFIX} + 64 lowercase hex"))?;
        let bytes = axon_canonical::canonical_bytes(&v).map_err(|e| format!("run record: {e}"))?;
        ring::hmac::verify(&self.key(), &bytes, &tag)
            .map_err(|_| "the seal does not verify under the run-record key".to_string())
    }
}

// ── records ──────────────────────────────────────────────────────────────────

/// When a record was written (§3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Started,
    Terminal,
}

/// The graded artifact (`accepted_artifact`'s `{path, sha256}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Accepted {
    pub path: String,
    /// Hex sha256, no prefix.
    pub sha256: String,
}

/// One NH-R1 E5 journal entry. A create has no `before`, a delete no `after`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WriteEntry {
    pub site: String,
    /// Workspace-relative.
    pub path: String,
    pub before_sha256: Option<String>,
    pub after_sha256: Option<String>,
}

/// A manifest of every file under the write prefixes and fs_write scopes:
/// workspace-relative path -> hex sha256.
pub type Tree = BTreeMap<String, String>;

/// A `cortex-run-record/1` record. Every field is always present (null
/// allowed): [`parse_record`] refuses any other spelling of the same record.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunRecord {
    pub schema: String,
    pub seq: u64,
    pub prev: Option<String>,
    pub kind: Kind,
    pub envelope_id: String,
    pub envelope_digest: String,
    pub task_family: String,
    pub task: Task,
    /// Started only: the NH-R1 §5.6 enforcement rows.
    pub enforcement: Option<Value>,
    /// Started only.
    pub pinned_tree_before: Option<Tree>,
    /// Terminal only.
    pub outcome: Option<String>,
    pub exit_code: Option<i64>,
    pub stopped_by: Option<String>,
    /// Terminal, exit 0 or 27 only: the graded artifact ...
    pub accepted: Option<Accepted>,
    /// ... or, instead, why `settle_artifact` refused it.
    pub accepted_refusal: Option<String>,
    pub writes: Option<Vec<WriteEntry>>,
    pub pinned_tree_after: Option<Tree>,
    pub episode_digest: Option<String>,
    /// The effective (narrowed) limits.
    pub budget: Budget,
    pub generator: Generator,
    pub recorded_unix: u64,
    pub seal: String,
}

/// What the runner knows at `started`.
#[derive(Debug, Clone, PartialEq)]
pub struct Started {
    pub enforcement: Value,
    pub pinned_tree_before: Tree,
    pub budget: Budget,
}

/// What the runner knows at `terminal`.
#[derive(Debug, Clone, PartialEq)]
pub struct Terminal {
    pub outcome: String,
    pub exit_code: i64,
    pub stopped_by: Option<String>,
    pub accepted: Option<Accepted>,
    pub accepted_refusal: Option<String>,
    pub writes: Vec<WriteEntry>,
    pub pinned_tree_after: Tree,
    pub episode_digest: Option<String>,
    pub budget: Budget,
}

/// A record body for [`append`].
#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Started(Started),
    Terminal(Terminal),
}

/// The sealed head: the last acknowledged record.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Head {
    pub schema: String,
    pub ledger_id: String,
    pub seq: u64,
    pub record_digest: String,
    pub seal: String,
}

fn is_cl22(s: &str) -> bool {
    s.strip_prefix("cl22:").is_some_and(|h| is_lower_hex(h, 64))
}

/// The pinned-tree grammar (R6 §3, widened by EG-2): a path maps to a
/// 64-hex sha256 or `over-cap:<len>` (decimal, no leading zero); the
/// reserved key `""` maps to `over-cap:manifest` or `over-cap:time`.
fn check_tree(name: &str, t: &Tree) -> Result<(), String> {
    for (p, d) in t {
        let ok = if p == TREE_STOP_KEY {
            d == "over-cap:manifest" || d == "over-cap:time"
        } else {
            is_lower_hex(d, 64) || d.strip_prefix(OVER_CAP).is_some_and(is_decimal_len)
        };
        if !ok {
            return Err(format!(
                "{name}: entry {p:?} is not a path and a 64-hex sha256 or over-cap:<len>, nor \
                 the stop marker \"\" and over-cap:manifest|time"
            ));
        }
    }
    Ok(())
}

/// A decimal byte length: digits only, no leading zero (`0` itself is one).
fn is_decimal_len(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_digit())
        && (s == "0" || !s.starts_with('0'))
        && s.parse::<u64>().is_ok()
}

/// EG-2: why a pinned tree is capped, or `None` for a complete walk whose
/// every file was hashed: the stop marker's value and the count of
/// `over-cap:` paths.
fn tree_cap(t: &Tree) -> Option<String> {
    let over = t
        .iter()
        .filter(|(p, d)| p.as_str() != TREE_STOP_KEY && d.starts_with(OVER_CAP))
        .count();
    let stop = t.get(TREE_STOP_KEY);
    if stop.is_none() && over == 0 {
        return None;
    }
    Some(format!(
        "{}; {over} over-cap path(s)",
        match stop {
            Some(v) => format!("the walk stopped ({v}): later paths are absent"),
            None => "the walk completed".into(),
        }
    ))
}

/// The intrinsic record checks (no I/O).
pub fn validate_record(r: &RunRecord) -> Result<(), String> {
    if r.schema != RECORD_SCHEMA {
        return Err(format!(
            "run record schema is {:?}, not {RECORD_SCHEMA}",
            r.schema
        ));
    }
    if r.task_family != TASK_FAMILY {
        return Err(format!(
            "task_family is {:?}, not {TASK_FAMILY}",
            r.task_family
        ));
    }
    if !is_lower_hex(&r.envelope_id, 32) {
        return Err("envelope_id is not 32 lowercase hex characters".into());
    }
    if !is_cl22(&r.envelope_digest) {
        return Err("envelope_digest is not a cl22 digest".into());
    }
    match (&r.prev, r.seq) {
        (None, 0) => {}
        (Some(p), s) if s > 0 && is_cl22(p) => {}
        _ => return Err("prev is null exactly at seq 0, else a cl22 digest".into()),
    }
    let started_only = r.enforcement.is_some() || r.pinned_tree_before.is_some();
    let terminal_only = r.outcome.is_some()
        || r.exit_code.is_some()
        || r.stopped_by.is_some()
        || r.accepted.is_some()
        || r.accepted_refusal.is_some()
        || r.writes.is_some()
        || r.pinned_tree_after.is_some()
        || r.episode_digest.is_some();
    match r.kind {
        Kind::Started => {
            if terminal_only {
                return Err("a started record carries a terminal field".into());
            }
            if !r.enforcement.as_ref().is_some_and(Value::is_array) {
                return Err("a started record's enforcement is not an array of rows".into());
            }
            check_tree(
                "pinned_tree_before",
                r.pinned_tree_before
                    .as_ref()
                    .ok_or("a started record has no pinned_tree_before")?,
            )?;
        }
        Kind::Terminal => {
            if started_only {
                return Err("a terminal record carries a started field".into());
            }
            if r.outcome.as_deref().is_none_or(str::is_empty) {
                return Err("a terminal record has no outcome".into());
            }
            let code = r.exit_code.ok_or("a terminal record has no exit_code")?;
            let graded = r.accepted.is_some() as u8 + r.accepted_refusal.is_some() as u8;
            if ACCEPTED_EXITS.contains(&code) {
                if graded != 1 {
                    return Err(format!(
                        "exit {code}: exactly one of accepted and accepted_refusal is set"
                    ));
                }
            } else if graded != 0 {
                return Err(format!("exit {code} carries an accepted artifact"));
            }
            if let Some(a) = &r.accepted {
                if a.path.is_empty() || !is_lower_hex(&a.sha256, 64) {
                    return Err("accepted is not a path and a 64-hex sha256".into());
                }
            }
            for w in r.writes.as_ref().ok_or("a terminal record has no writes")? {
                let digest_ok =
                    |d: &Option<String>| d.as_deref().is_none_or(|d| is_lower_hex(d, 64));
                if w.site.is_empty()
                    || w.path.is_empty()
                    || !digest_ok(&w.before_sha256)
                    || !digest_ok(&w.after_sha256)
                    || (w.before_sha256.is_none() && w.after_sha256.is_none())
                {
                    return Err(format!("write {:?} at {:?} is malformed", w.site, w.path));
                }
            }
            check_tree(
                "pinned_tree_after",
                r.pinned_tree_after
                    .as_ref()
                    .ok_or("a terminal record has no pinned_tree_after")?,
            )?;
        }
    }
    Ok(())
}

/// Parse one ledger line: strict parse, seal, typed parse, normal form (the
/// line is exactly the canonical bytes of the typed record), intrinsic checks.
/// Returns the record and its `cl22:` digest (seal included).
pub fn parse_record(line: &str, key: &SealKey) -> Result<(RunRecord, String), String> {
    let v = axon_canonical::parse_value(line).map_err(|e| format!("run record: {e}"))?;
    if v.get("schema").and_then(Value::as_str) != Some(RECORD_SCHEMA) {
        return Err(format!("run record: schema is not {RECORD_SCHEMA}"));
    }
    key.check_seal(&v).map_err(|e| format!("run record: {e}"))?;
    let r: RunRecord = serde_json::from_value(v.clone()).map_err(|e| format!("run record: {e}"))?;
    normal_form(&r, &v)?;
    validate_record(&r)?;
    let bytes = axon_canonical::canonical_bytes(&v).map_err(|e| format!("run record: {e}"))?;
    if bytes != line.as_bytes() {
        return Err("run record: the line is not its canonical form".into());
    }
    Ok((r, axon_canonical::cl22(&bytes)))
}

fn normal_form<T: Serialize>(typed: &T, v: &Value) -> Result<(), String> {
    let back = serde_json::to_value(typed).map_err(|e| format!("run record: {e}"))?;
    if &back != v {
        return Err("run record: a field is missing (every field is present, null allowed)".into());
    }
    Ok(())
}

fn parse_head(text: &str, key: &SealKey) -> Result<Head, String> {
    let v = axon_canonical::parse_value(text.trim_end_matches('\n'))
        .map_err(|e| format!("ledger head: {e}"))?;
    if v.get("schema").and_then(Value::as_str) != Some(HEAD_SCHEMA) {
        return Err(format!("ledger head: schema is not {HEAD_SCHEMA}"));
    }
    key.check_seal(&v)
        .map_err(|e| format!("ledger head: {e}"))?;
    let h: Head = serde_json::from_value(v).map_err(|e| format!("ledger head: {e}"))?;
    if !is_cl22(&h.record_digest) {
        return Err("ledger head: record_digest is not a cl22 digest".into());
    }
    Ok(h)
}

fn sealed_line<T: Serialize>(key: &SealKey, typed: &T) -> Result<(Vec<u8>, String), String> {
    let Value::Object(mut obj) = serde_json::to_value(typed).map_err(|e| e.to_string())? else {
        return Err("a record serializes to an object".into());
    };
    obj.remove("seal");
    let mut v = Value::Object(obj);
    let seal = key.seal(&v)?;
    if let Some(o) = v.as_object_mut() {
        o.insert("seal".into(), Value::String(seal.clone()));
    }
    let bytes = axon_canonical::canonical_bytes(&v).map_err(|e| format!("run record: {e}"))?;
    Ok((bytes, seal))
}

// ── the sink ─────────────────────────────────────────────────────────────────

/// Initialise an EMPTY private ledger directory: `ledger-id` (create_new) and
/// an empty `ledger.jsonl`. A ledger that already has an id is refused.
pub fn init_ledger(dir: &Path, euid: u32) -> Result<String, String> {
    check_private(dir, euid, &RUN_LEDGER)?;
    let id = crate::repair_envelope::new_id()?;
    write_new(&dir.join(LEDGER_ID_FILE), format!("{id}\n").as_bytes()).map_err(|e| {
        format!(
            "run ledger {}: cannot create {LEDGER_ID_FILE}: {e}",
            dir.display()
        )
    })?;
    write_new(&dir.join(LEDGER_FILE), b"").map_err(|e| {
        format!(
            "run ledger {}: cannot create {LEDGER_FILE}: {e}",
            dir.display()
        )
    })?;
    sync_dir(dir)?;
    Ok(id)
}

/// The ledger's id, after checking the directory and the id file are private.
pub fn ledger_id(dir: &Path, euid: u32) -> Result<String, String> {
    check_private(dir, euid, &RUN_LEDGER)?;
    let p = dir.join(LEDGER_ID_FILE);
    check_private(&p, euid, &RUN_LEDGER_FILE)?;
    let id = std::fs::read_to_string(&p)
        .map_err(|e| format!("run ledger {}: {LEDGER_ID_FILE}: {e}", dir.display()))?;
    let id = id.trim_end_matches('\n');
    if !is_lower_hex(id, 32) {
        return Err(format!(
            "run ledger {}: {LEDGER_ID_FILE} is not 32 lowercase hex",
            dir.display()
        ));
    }
    Ok(id.to_string())
}

fn open_ledger_file(dir: &Path, euid: u32, append: bool) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let p = dir.join(LEDGER_FILE);
    check_private(&p, euid, &RUN_LEDGER_FILE)?;
    std::fs::OpenOptions::new()
        .read(true)
        .append(append)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&p)
        .map_err(|e| format!("run ledger {}: {LEDGER_FILE}: {e}", dir.display()))
}

fn read_all(f: &mut std::fs::File, dir: &Path) -> Result<String, String> {
    let mut text = String::new();
    f.read_to_string(&mut text)
        .map_err(|e| format!("run ledger {}: {LEDGER_FILE}: {e}", dir.display()))?;
    Ok(text)
}

fn read_head(dir: &Path, euid: u32, key: &SealKey) -> Result<Option<Head>, String> {
    let p = dir.join(HEAD_FILE);
    if std::fs::symlink_metadata(&p).is_err() {
        return Ok(None);
    }
    check_private(&p, euid, &RUN_LEDGER_FILE)?;
    let text = std::fs::read_to_string(&p)
        .map_err(|e| format!("run ledger {}: {HEAD_FILE}: {e}", dir.display()))?;
    parse_head(&text, key).map(Some)
}

/// The head may name the last record, or the one before it (a crash between
/// the append and the head replace); anything else stops the append.
fn head_agrees(head: Option<&Head>, last: Option<&(RunRecord, String)>) -> Result<(), String> {
    match (head, last) {
        (None, None) => Ok(()),
        (None, Some((r, _))) if r.seq == 0 => Ok(()),
        (Some(h), Some((r, d))) if h.seq == r.seq && &h.record_digest == d => Ok(()),
        (Some(h), Some((r, _)))
            if h.seq + 1 == r.seq && r.prev.as_deref() == Some(h.record_digest.as_str()) =>
        {
            Ok(())
        }
        _ => Err(
            "the run ledger's head does not name its last record: refusing to append \
             (audit it with `axon-intent audit-runs`)"
                .into(),
        ),
    }
}

/// Append one record for the verified envelope `v` (§4 crash consistency).
/// Refused: a ledger whose id is not the envelope's `run_ledger_id`, a torn
/// or unsealed last line, a head that disagrees with the ledger.
pub fn append(
    dir: &Path,
    euid: u32,
    key: &SealKey,
    v: &VerifiedEnvelope,
    body: Body,
    now_unix: u64,
) -> Result<Head, String> {
    use std::os::unix::io::AsRawFd;
    let id = ledger_id(dir, euid)?;
    if id != v.envelope.run_ledger_id {
        return Err(format!(
            "run ledger {} has id {id}, but the envelope records into {}",
            dir.display(),
            v.envelope.run_ledger_id
        ));
    }
    let mut f = open_ledger_file(dir, euid, true)?;
    // SAFETY: flock on a descriptor this function owns; blocking, so
    // concurrent runs serialize their appends.
    if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX) } != 0 {
        return Err(format!(
            "run ledger {}: cannot lock {LEDGER_FILE}: {}",
            dir.display(),
            std::io::Error::last_os_error()
        ));
    }
    let text = read_all(&mut f, dir)?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(format!(
            "run ledger {}: {LEDGER_FILE} ends in a torn record: refusing to append",
            dir.display()
        ));
    }
    let last = match text.lines().next_back() {
        Some(line) => Some(parse_record(line, key)?),
        None => None,
    };
    let head = read_head(dir, euid, key)?;
    if let Some(h) = &head {
        if h.ledger_id != id {
            return Err(format!(
                "run ledger {}: its head names ledger {}",
                dir.display(),
                h.ledger_id
            ));
        }
    }
    head_agrees(head.as_ref(), last.as_ref())?;
    let (seq, prev) = match last {
        Some((r, d)) => (r.seq + 1, Some(d)),
        None => (0, None),
    };
    let e = &v.envelope;
    let mut rec = RunRecord {
        schema: RECORD_SCHEMA.into(),
        seq,
        prev,
        kind: Kind::Started,
        envelope_id: e.envelope_id.clone(),
        envelope_digest: v.digest.clone(),
        task_family: TASK_FAMILY.into(),
        task: e.task.clone(),
        enforcement: None,
        pinned_tree_before: None,
        outcome: None,
        exit_code: None,
        stopped_by: None,
        accepted: None,
        accepted_refusal: None,
        writes: None,
        pinned_tree_after: None,
        episode_digest: None,
        budget: e.budget.clone(),
        generator: e.generator.clone(),
        recorded_unix: now_unix,
        seal: String::new(),
    };
    match body {
        Body::Started(s) => {
            rec.enforcement = Some(s.enforcement);
            rec.pinned_tree_before = Some(s.pinned_tree_before);
            rec.budget = s.budget;
        }
        Body::Terminal(t) => {
            rec.kind = Kind::Terminal;
            rec.outcome = Some(t.outcome);
            rec.exit_code = Some(t.exit_code);
            rec.stopped_by = t.stopped_by;
            rec.accepted = t.accepted;
            rec.accepted_refusal = t.accepted_refusal;
            rec.writes = Some(t.writes);
            rec.pinned_tree_after = Some(t.pinned_tree_after);
            rec.episode_digest = t.episode_digest;
            rec.budget = t.budget;
        }
    }
    validate_record(&rec)?;
    let (mut line, _) = sealed_line(key, &rec)?;
    let digest = axon_canonical::cl22(&line);
    line.push(b'\n');
    f.write_all(&line)
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("run ledger {}: cannot append: {e}", dir.display()))?;
    let mut head = Head {
        schema: HEAD_SCHEMA.into(),
        ledger_id: id,
        seq,
        record_digest: digest,
        seal: String::new(),
    };
    let (mut hline, seal) = sealed_line(key, &head)?;
    head.seal = seal;
    hline.push(b'\n');
    let tmp = dir.join(HEAD_TMP);
    match std::fs::remove_file(&tmp) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(format!("run ledger {}: {HEAD_TMP}: {e}", dir.display())),
    }
    write_new(&tmp, &hline)
        .and_then(|_| std::fs::rename(&tmp, dir.join(HEAD_FILE)))
        .map_err(|e| {
            format!(
                "run ledger {}: cannot replace {HEAD_FILE}: {e}",
                dir.display()
            )
        })?;
    sync_dir(dir)?;
    Ok(head)
}

/// The spent store's marker of a failed `started` append (§3): the run is
/// refused with exit 29 before work begins, and `<id>.unrecorded` (create_new,
/// beside the `.used` record) says why, so C1's `consumed-no-record` finding
/// for it names the failure instead of reading as truncation.
pub fn mark_unrecorded(
    spent: &Path,
    euid: u32,
    envelope_id: &str,
    reason: &str,
    now_unix: u64,
) -> Result<(), String> {
    crate::repair_envelope::store_id(spent, euid)?;
    if !is_lower_hex(envelope_id, 32) {
        return Err("envelope_id is not 32 lowercase hex characters".into());
    }
    let rec = serde_json::json!({ "reason": reason, "marked_unix": now_unix });
    write_new(
        &spent.join(format!("{envelope_id}.unrecorded")),
        format!("{rec}\n").as_bytes(),
    )
    .map_err(|e| format!("envelope {envelope_id}: cannot mark the failed run record: {e}"))?;
    sync_dir(spent)
}

fn unrecorded_reason(spent: &Path, envelope_id: &str) -> Option<String> {
    let t = std::fs::read_to_string(spent.join(format!("{envelope_id}.unrecorded"))).ok()?;
    let v: Value = serde_json::from_str(t.trim_end()).ok()?;
    v.get("reason")?.as_str().map(str::to_string)
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("run ledger {}: fsync: {e}", dir.display()))
}

// ── C1 audit (§5) ────────────────────────────────────────────────────────────

/// One C1 finding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Finding {
    /// One of [`FINDINGS`].
    pub finding: &'static str,
    pub seq: Option<u64>,
    pub envelope_id: Option<String>,
    pub detail: String,
}

fn finding(
    name: &'static str,
    seq: Option<u64>,
    id: Option<&str>,
    detail: impl Into<String>,
) -> Finding {
    debug_assert!(FINDINGS.contains(&name));
    Finding {
        finding: name,
        seq,
        envelope_id: id.map(str::to_string),
        detail: detail.into(),
    }
}

/// The spent store's consumed envelopes: id -> the `envelope_digest` its
/// `.used` record names (None when that record is unreadable).
fn consumed(spent: &Path, euid: u32) -> Result<BTreeMap<String, Option<String>>, String> {
    crate::repair_envelope::store_id(spent, euid)?;
    let mut out = BTreeMap::new();
    let entries =
        std::fs::read_dir(spent).map_err(|e| format!("spent store {}: {e}", spent.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("spent store {}: {e}", spent.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(id) = name.strip_suffix(".used") else {
            continue;
        };
        let digest = std::fs::read_to_string(entry.path())
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(t.trim_end()).ok())
            .and_then(|v| v.get("envelope_digest")?.as_str().map(str::to_string));
        out.insert(id.to_string(), digest);
    }
    Ok(out)
}

/// What C1 read: every record that verified, by envelope id in ledger order,
/// and the findings.
#[derive(Debug, Clone, PartialEq)]
pub struct Audit {
    pub runs: BTreeMap<String, Vec<RunRecord>>,
    /// Consumed envelope ids in the spent store.
    pub consumed: BTreeSet<String>,
    pub findings: Vec<Finding>,
}

/// C1: verify the chain, every seal and the head; join the records against
/// the spent store and the supplied envelopes (`envelope_id -> digest`).
/// `Err` only when a directory cannot be read as a private ledger or store.
pub fn audit(
    ledger: &Path,
    spent: &Path,
    euid: u32,
    key: &SealKey,
    envelopes: &BTreeMap<String, String>,
) -> Result<Audit, String> {
    let id = ledger_id(ledger, euid)?;
    let mut f = open_ledger_file(ledger, euid, false)?;
    let text = read_all(&mut f, ledger)?;
    let used = consumed(spent, euid)?;
    let mut out = Vec::new();

    // The chain and the seals.
    let lines: Vec<&str> = text.split_terminator('\n').collect();
    let torn = !text.is_empty() && !text.ends_with('\n');
    let mut digests: Vec<Option<String>> = Vec::with_capacity(lines.len());
    let mut runs: BTreeMap<String, Vec<RunRecord>> = BTreeMap::new();
    for (i, line) in lines.iter().enumerate() {
        let seq = i as u64;
        let prev = digests.last().cloned().flatten();
        if torn && i + 1 == lines.len() {
            out.push(finding(
                "chain-break",
                Some(seq),
                None,
                "the last line is torn",
            ));
        }
        match parse_record(line, key) {
            Ok((r, d)) => {
                if r.seq != seq || r.prev != prev {
                    out.push(finding(
                        "chain-break",
                        Some(seq),
                        Some(&r.envelope_id),
                        format!(
                            "line {i} carries seq {} prev {:?}; the chain wants seq {seq} prev {prev:?}",
                            r.seq, r.prev
                        ),
                    ));
                }
                digests.push(Some(d));
                runs.entry(r.envelope_id.clone()).or_default().push(r);
            }
            Err(e) => {
                out.push(finding("seal-break", Some(seq), None, e));
                digests.push(
                    axon_canonical::parse_value(line)
                        .ok()
                        .and_then(|v| axon_canonical::digest_value(&v).ok()),
                );
            }
        }
    }

    // The head.
    let n = lines.len() as u64;
    match read_head(ledger, euid, key) {
        Ok(None) if n > 0 => out.push(finding(
            "unacknowledged-tail",
            Some(0),
            None,
            format!("no {HEAD_FILE}: {n} record(s) unacknowledged"),
        )),
        Ok(None) => {}
        Ok(Some(h)) => {
            if h.ledger_id != id {
                out.push(finding(
                    "chain-break",
                    None,
                    None,
                    format!("the head names ledger {}, not {id}", h.ledger_id),
                ));
            }
            if h.seq >= n {
                out.push(finding(
                    "head-beyond-ledger",
                    Some(h.seq),
                    None,
                    format!(
                        "the head names seq {} but the ledger holds {n} record(s)",
                        h.seq
                    ),
                ));
            } else {
                if digests[h.seq as usize].as_deref() != Some(h.record_digest.as_str()) {
                    out.push(finding(
                        "chain-break",
                        Some(h.seq),
                        None,
                        "the head's record_digest is not the digest of the record it names",
                    ));
                }
                if h.seq + 1 < n {
                    out.push(finding(
                        "unacknowledged-tail",
                        Some(h.seq + 1),
                        None,
                        format!("{} record(s) past the head", n - h.seq - 1),
                    ));
                }
            }
        }
        Err(e) => out.push(finding("seal-break", None, None, e)),
    }

    // The join.
    for (eid, recs) in &runs {
        let eid_s = Some(eid.as_str());
        let started: Vec<&RunRecord> = recs.iter().filter(|r| r.kind == Kind::Started).collect();
        let terminal: Vec<&RunRecord> = recs.iter().filter(|r| r.kind == Kind::Terminal).collect();
        if started.len() > 1 || terminal.len() > 1 {
            out.push(finding(
                "duplicate-run",
                recs.first().map(|r| r.seq),
                eid_s,
                format!(
                    "{} started, {} terminal record(s)",
                    started.len(),
                    terminal.len()
                ),
            ));
        }
        match (started.is_empty(), terminal.is_empty()) {
            (false, true) => out.push(finding(
                "started-no-terminal",
                Some(started[0].seq),
                eid_s,
                "the run was killed or crashed",
            )),
            (true, false) => out.push(finding(
                "terminal-no-started",
                Some(terminal[0].seq),
                eid_s,
                "a terminal record with no started record",
            )),
            _ => {}
        }
        let digests_named: BTreeSet<&str> =
            recs.iter().map(|r| r.envelope_digest.as_str()).collect();
        match used.get(eid) {
            None => out.push(finding(
                "record-not-consumed",
                recs.first().map(|r| r.seq),
                eid_s,
                "no consumed envelope in the spent store",
            )),
            Some(Some(ud)) if digests_named.iter().any(|d| d != ud) => out.push(finding(
                "envelope-mismatch",
                recs.first().map(|r| r.seq),
                eid_s,
                format!("the spent store consumed {ud}, the records name {digests_named:?}"),
            )),
            Some(_) => {}
        }
        if let Some(sd) = envelopes.get(eid) {
            if digests_named.iter().any(|d| d != sd) {
                out.push(finding(
                    "envelope-mismatch",
                    recs.first().map(|r| r.seq),
                    eid_s,
                    format!("the supplied envelope is {sd}, the records name {digests_named:?}"),
                ));
            }
        }
        // EG-2: a capped pinned tree is a finding of its own, one per tree.
        for r in started.iter().chain(&terminal) {
            let (name, tree) = match (&r.pinned_tree_before, &r.pinned_tree_after) {
                (Some(t), _) => ("pinned_tree_before", t),
                (None, Some(t)) => ("pinned_tree_after", t),
                (None, None) => continue,
            };
            if let Some(why) = tree_cap(tree) {
                out.push(finding(
                    "tree-over-cap",
                    Some(r.seq),
                    eid_s,
                    format!("{name}: {why}"),
                ));
            }
        }
        for t in &terminal {
            let writes = t.writes.as_deref().unwrap_or_default();
            if let Some(a) = &t.accepted {
                let last = writes
                    .iter()
                    .rev()
                    .find(|w| w.path == a.path && w.site != VISIBLE_SITE);
                if last.and_then(|w| w.after_sha256.as_deref()) != Some(a.sha256.as_str()) {
                    out.push(finding(
                        "accepted-mismatch",
                        Some(t.seq),
                        eid_s,
                        format!(
                            "accepted {} sha256 {} is not the last write's after_sha256 ({:?})",
                            a.path,
                            a.sha256,
                            last.and_then(|w| w.after_sha256.clone())
                        ),
                    ));
                }
            }
            let (Some(before), Some(after)) = (
                started.first().and_then(|s| s.pinned_tree_before.as_ref()),
                t.pinned_tree_after.as_ref(),
            ) else {
                continue;
            };
            let mut last_after: BTreeMap<&str, Option<&str>> = BTreeMap::new();
            for w in writes {
                last_after.insert(w.path.as_str(), w.after_sha256.as_deref());
            }
            // EG-2 (EGA-4): a stopped walk's `after` lacks every later path,
            // so a path `before` has and a stopped `after` lacks is no
            // deletion; the stop marker itself is never joined. A path
            // valued `over-cap:<len>` is still joined.
            let stopped = after.contains_key(TREE_STOP_KEY);
            let paths: BTreeSet<&String> = before
                .keys()
                .chain(after.keys())
                .filter(|p| p.as_str() != TREE_STOP_KEY)
                .collect();
            for p in paths {
                let (b, a) = (before.get(p), after.get(p));
                if stopped && a.is_none() {
                    continue;
                }
                if b != a && last_after.get(p.as_str()) != Some(&a.map(String::as_str)) {
                    out.push(finding(
                        "unattributed-change",
                        Some(t.seq),
                        eid_s,
                        format!("{p}: {b:?} -> {a:?} with no write that explains it"),
                    ));
                }
            }
        }
    }
    for (eid, digest) in &used {
        // Security F10: a `.used` record C1 cannot read skips the store-side
        // digest check above; that is a finding of its own, never silent.
        if digest.is_none() {
            out.push(finding(
                "used-unreadable",
                None,
                Some(eid),
                "the spent store's .used record is unreadable or names no envelope_digest",
            ));
        }
        if !runs.contains_key(eid) {
            out.push(finding(
                "consumed-no-record",
                None,
                Some(eid),
                match unrecorded_reason(spent, eid) {
                    Some(r) => format!("the started append failed (exit 29, nothing ran): {r}"),
                    None => "a consumed envelope with no run record (truncation)".into(),
                },
            ));
        }
    }
    Ok(Audit {
        runs,
        consumed: used.into_keys().collect(),
        findings: out,
    })
}

#[cfg(test)]
mod tests;
