//! The directory store and its durability primitives.
//!
//! ```text
//! <root>/
//!   config.json                         operator premises (trusted admitters/verifiers)
//!   policies/<hex>.json                 PolicyEnvelope, file name = cl22 hex of its bytes
//!   admissions/<hex>.json               AdmissionRecord, file name = cl22 hex
//!   evaluations/<hex>.json              EvaluationRecord, file name = cl22 hex
//!   plans/<experiment_id>/plan.json     registered closed-loop-pilot/1 plan
//!   plans/<experiment_id>/frozen.json   freeze record {plan_ref}; present ⇒ immutable
//!   scopes/<tenant>/<family>/
//!     pointer.json                      PointerRecord {active_policy_ref, epoch, history}
//!     transitions.jsonl                 append-only log of applied transitions
//!     revocations.json                  per-scope revocation list
//!     hypotheses.jsonl                  append-only EVO hypothesis history
//!     lock                              flock(2) target serialising every writer
//! ```
//!
//! Every record replace is tmp + fsync + rename + directory fsync, so a reader
//! sees the old bytes or the new bytes and never a torn file. Every append is
//! write + fsync. Scope and experiment ids are validated `[A-Za-z0-9._:-]`
//! with an alphanumeric first byte, so they cannot name `..` or contain `/`.

use crate::error::{LoopError, Result};
use axon_loop_contracts::{OpaqueRef, Ref, RefScheme, Refusal, Scope};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

crate::record_tag!(ConfigSchema, "axon.loop.config/1");

/// Operator premises. Not authentication: a name in these sets is who the
/// operator SAYS may admit/verify; production must authenticate the caller
/// before presenting its identity here.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Config {
    pub schema: ConfigSchema,
    pub trusted_admitters: Vec<OpaqueRef>,
    pub trusted_verifiers: Vec<OpaqueRef>,
}

impl Config {
    pub fn admitters(&self) -> BTreeSet<OpaqueRef> {
        self.trusted_admitters.iter().cloned().collect()
    }
    pub fn verifiers(&self) -> BTreeSet<OpaqueRef> {
        self.trusted_verifiers.iter().cloned().collect()
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    root: PathBuf,
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Store> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Store { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// The operator config. Absent ⇒ both trusted sets EMPTY, so every
    /// admission, transition and verified outcome is refused (fail closed).
    pub fn config(&self) -> Result<Config> {
        let p = self.root.join("config.json");
        match fs::read_to_string(&p) {
            Ok(s) => strict_record(&s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Config {
                schema: ConfigSchema,
                trusted_admitters: Vec::new(),
                trusted_verifiers: Vec::new(),
            }),
            Err(e) => Err(e.into()),
        }
    }

    pub fn write_config(&self, c: &Config) -> Result<()> {
        write_json_atomic(&self.root.join("config.json"), c)
    }

    pub fn scope_dir(&self, scope: &Scope) -> PathBuf {
        self.root
            .join("scopes")
            .join(scope.tenant_id.as_str())
            .join(scope.task_family.as_str())
    }

    pub fn plan_dir(&self, experiment_id: &str) -> PathBuf {
        self.root.join("plans").join(experiment_id)
    }

    /// Content-addressed path for a `cl22:` record kind.
    pub fn cas_path(&self, kind: &str, r: &Ref) -> Result<PathBuf> {
        if r.scheme() != RefScheme::Cl22 {
            return Err(crate::error::refused(format!(
                "{kind} references must be cl22:, got {r}"
            )));
        }
        Ok(self.root.join(kind).join(format!("{}.json", r.hex())))
    }

    /// Store a record under its own `cl22:` digest. Idempotent: the same bytes
    /// land at the same path.
    pub fn put_cas<T: Serialize>(&self, kind: &str, v: &T) -> Result<Ref> {
        let r = axon_loop_contracts::digest(v)?;
        let p = self.cas_path(kind, &r)?;
        if !p.exists() {
            let bytes = axon_loop_contracts::canonical_json(v)?;
            write_bytes_atomic(&p, &bytes)?;
        }
        Ok(r)
    }

    fn read_cas(&self, kind: &str, r: &Ref) -> Result<String> {
        let p = self.cas_path(kind, r)?;
        match fs::read_to_string(&p) {
            Ok(s) => Ok(s),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                Err(crate::error::refused(format!("no {kind} record {r}")))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Load a stored package contract (e.g. a `PolicyEnvelope`) under the
    /// contracts crate's schema-checked `parse`, and RE-CHECK that its bytes
    /// still digest to its name (a hand-edited file is refused, not trusted).
    pub fn get_contract<T: axon_loop_contracts::Contract>(&self, kind: &str, r: &Ref) -> Result<T> {
        let s = self.read_cas(kind, r)?;
        let v: T = axon_loop_contracts::parse(&s)
            .map_err(|e| LoopError::Io(format!("{kind} record {r} does not parse: {e}")))?;
        check_name(kind, r, &v)?;
        Ok(v)
    }

    /// Load one of this crate's own records (admission, evaluation) through
    /// [`strict_record`], with the same digest re-check.
    pub fn get_record<T: Serialize + DeserializeOwned>(&self, kind: &str, r: &Ref) -> Result<T> {
        let s = self.read_cas(kind, r)?;
        let v: T = strict_record(&s)
            .map_err(|e| LoopError::Io(format!("{kind} record {r} does not parse: {e}")))?;
        check_name(kind, r, &v)?;
        Ok(v)
    }
}

fn check_name<T: Serialize>(kind: &str, r: &Ref, v: &T) -> Result<()> {
    {
        let d = axon_loop_contracts::digest(v)?;
        if &d != r {
            return Err(LoopError::Io(format!(
                "{kind} record {r} is corrupt: content digests to {d}"
            )));
        }
        Ok(())
    }
}

/// Strict ingest for this crate's OWN records (not package contracts, which
/// go through `axon_loop_contracts::parse` and its schemas).
///
/// `parse_value` applies the profile's JSON rules (duplicate/escaped-alias
/// keys, no floats, safe integers, depth, size). Typed serde then applies
/// `deny_unknown_fields` and the validated newtypes. Finally the typed value
/// is re-serialized and its canonical bytes compared with the input's: serde
/// also accepts a struct written as a positional array or an enum written as
/// `{"variant":null}`, and those alternative encodings do not round-trip, so
/// they are refused here instead of being silently normalized.
pub fn strict_record<T: Serialize + DeserializeOwned>(text: &str) -> Result<T> {
    let v = axon_loop_contracts::parse_value(text)?;
    let t: T = serde_json::from_value(v.clone())
        .map_err(|e| LoopError::Malformed(Refusal::Shape(e.to_string())))?;
    let back = serde_json::to_value(&t).map_err(|e| LoopError::Io(e.to_string()))?;
    if axon_loop_contracts::canonical_bytes(&back)? != axon_loop_contracts::canonical_bytes(&v)? {
        return Err(LoopError::Malformed(Refusal::Shape(
            "record is not in its closed canonical shape (alternative encoding, reordered set, or defaulted field)".into(),
        )));
    }
    Ok(t)
}

/// Re-parse an already-strict `Value` as a package contract through the
/// contracts crate's schema-checked [`axon_loop_contracts::parse`].
pub fn contract_from_value<T: axon_loop_contracts::Contract>(
    what: &str,
    v: &serde_json::Value,
) -> Result<T> {
    let text = serde_json::to_string(v).map_err(|e| LoopError::Io(e.to_string()))?;
    axon_loop_contracts::parse(&text).map_err(|e| match e {
        Refusal::Semantic(s) => LoopError::Refused(format!("{what}: {s}")),
        other => LoopError::Malformed(Refusal::Shape(format!("{what}: {other}"))),
    })
}

/// Exclusive advisory lock (flock) on `<dir>/lock`, held until drop. Separate
/// `File` handles are separate open file descriptions, so this serialises
/// threads of one process as well as separate processes.
pub struct DirLock {
    _f: File,
}

impl DirLock {
    pub fn acquire(dir: &Path) -> Result<DirLock> {
        fs::create_dir_all(dir)?;
        let f = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(dir.join("lock"))?;
        f.lock()?;
        Ok(DirLock { _f: f })
    }
}

fn fsync_dir(dir: &Path) -> Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

/// tmp + fsync + rename + directory fsync.
pub fn write_bytes_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| LoopError::Io(format!("{} has no parent", path.display())))?;
    fs::create_dir_all(dir)?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| LoopError::Io(format!("bad path {}", path.display())))?;
    let tmp = dir.join(format!(
        ".{name}.tmp.{}.{}",
        std::process::id(),
        TMP_SEQ.fetch_add(1, Ordering::Relaxed)
    ));
    let res = (|| -> Result<()> {
        let mut f = OpenOptions::new().create_new(true).write(true).open(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        fs::rename(&tmp, path)?;
        fsync_dir(dir)
    })();
    if res.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    res
}

pub fn write_json_atomic<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(v).map_err(|e| LoopError::Io(e.to_string()))?;
    bytes.push(b'\n');
    write_bytes_atomic(path, &bytes)
}

/// Append one JSON line and fsync it (and the directory, on first creation).
pub fn append_jsonl<T: Serialize>(path: &Path, v: &T) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| LoopError::Io("no parent".into()))?;
    fs::create_dir_all(dir)?;
    let created = !path.exists();
    let mut line = serde_json::to_vec(v).map_err(|e| LoopError::Io(e.to_string()))?;
    line.push(b'\n');
    let mut f = OpenOptions::new().create(true).append(true).open(path)?;
    f.write_all(&line)?;
    f.sync_all()?;
    if created {
        fsync_dir(dir)?;
    }
    Ok(())
}

/// Read a JSONL file strictly; a missing file is empty. A torn LAST line (a
/// crash mid-append, before its fsync returned) is ignored — that append was
/// never acknowledged. A malformed line anywhere else is corruption.
pub fn read_jsonl<T: Serialize + DeserializeOwned>(path: &Path) -> Result<Vec<T>> {
    let s = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let complete = s.ends_with('\n');
    let lines: Vec<&str> = s.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        let parsed = strict_record::<T>(line);
        match parsed {
            Ok(v) => out.push(v),
            Err(_) if last && !complete => break,
            Err(e) => {
                return Err(LoopError::Io(format!(
                    "{} line {}: {e}",
                    path.display(),
                    i + 1
                )))
            }
        }
    }
    Ok(out)
}

pub fn read_json<T: Serialize + DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    match fs::read_to_string(path) {
        Ok(s) => strict_record(&s)
            .map(Some)
            .map_err(|e| LoopError::Io(format!("{}: {e}", path.display()))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
