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
//! ```
//!
//! Every record replace is tmp + fsync + rename + directory fsync, so a reader
//! sees the old bytes or the new bytes and never a torn file. Every append is
//! write + fsync. Scope and experiment ids are validated `[A-Za-z0-9._:-]`
//! with an alphanumeric first byte BEFORE any filesystem call, so they cannot
//! name `..`, contain `/` or be absolute. The root is canonicalized on open;
//! below it every path component is checked with `symlink_metadata` and every
//! open uses `O_NOFOLLOW`, so a symlinked store file or directory is refused
//! (exit 2), never followed out of the store.

use crate::error::{LoopError, Result};
use axon_loop_contracts::{OpaqueRef, Ref, RefScheme, Refusal, Scope};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Component, Path, PathBuf};
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
    /// Independent preflight observers whose context receipts EVL accepts
    /// (`check_context_current`). Empty ⇒ no context is trusted, so no trial
    /// is a verified pass (fail closed); it never locks out pause/rollback.
    /// Always serialized, so an explicit `[]` round-trips (NS4b: it used to be
    /// dropped on re-serialization and then refused as non-canonical, exit 3
    /// on every writing verb). A config written before this field existed
    /// still parses: [`Store::config`] reads the absent field as `[]`.
    #[serde(default)]
    pub trusted_observers: Vec<OpaqueRef>,
    /// v0.22 G01-r22-independent-issuer: the Ed25519 PUBLIC key (64 hex) the
    /// operator registered for each trusted verifier. Membership in
    /// `trusted_verifiers` names who may vouch; this is what lets a vouching
    /// be AUTHENTICATED — verification evidence is accepted only with an
    /// `acf-receipt-attestation/1` that verifies under the key registered for
    /// its issuer. A trusted verifier with no key here can vouch for nothing
    /// (fail closed). Absent in a config written before this field: read as
    /// `{}`; always serialized.
    #[serde(default)]
    pub verifier_keys: std::collections::BTreeMap<OpaqueRef, String>,
    /// v0.22 G01-r22-independent-issuer / G01-r22-verifier-separation: WHAT
    /// each trusted verifier must have run for its verdict to count. A
    /// genuinely signed receipt from another verifier revision, another
    /// compute profile, or another suite — or from a file in the subject's own
    /// tree instead of an operator-registered suite — is refused. A trusted
    /// verifier with no pin vouches for nothing (fail closed). Absent in an
    /// older config: read as `{}`; always serialized.
    #[serde(default)]
    pub verifier_pins: std::collections::BTreeMap<OpaqueRef, VerifierPin>,
    /// v0.22 G01: the acceptance check the operator registered for each TASK —
    /// which suite, and which test in it, decides the task. Without this the
    /// requester chooses the test (a trivially passing one) or another task's
    /// suite, and a genuinely signed verdict still answers the wrong question.
    /// A task with no entry has no verifiable acceptance (fail closed). Interim
    /// operator authority until the frozen plan carries it (ADR-001 §3).
    /// Absent in an older config: read as `{}`; always serialized.
    #[serde(default)]
    pub task_acceptance: std::collections::BTreeMap<axon_loop_contracts::TaskId, AcceptancePin>,
}

/// One task's operator-registered acceptance check (see [`Config::task_acceptance`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcceptancePin {
    /// The suite and version, as Fabric records it: `check-suite:<id>@<acf1 version>`.
    pub check_suite: String,
    /// The exact test in that suite whose verdict is the task's.
    pub check: String,
}

/// One trusted verifier's operator pin (see [`Config::verifier_pins`]).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifierPin {
    /// The registered executable the check must run (`registered_executable_ref`).
    pub registered_executable_ref: String,
    /// Its registry-derived digest (`executable_digest`): the verifier REVISION.
    pub executable_digest: String,
    /// The backend profiles (`backend_profile_ref`) a verdict may come from.
    pub backend_profiles: Vec<String>,
    /// The registered suites a verdict may come from, as Fabric records them
    /// in the receipt: `check-suite:<id>@<acf1 version>`.
    pub check_suites: Vec<String>,
}

impl Config {
    pub fn admitters(&self) -> BTreeSet<OpaqueRef> {
        self.trusted_admitters.iter().cloned().collect()
    }
    pub fn verifiers(&self) -> BTreeSet<OpaqueRef> {
        self.trusted_verifiers.iter().cloned().collect()
    }
    pub fn observers(&self) -> BTreeSet<OpaqueRef> {
        self.trusted_observers.iter().cloned().collect()
    }
}

#[derive(Clone, Debug)]
pub struct Store {
    /// Canonical (symlink-resolved) root. Every store path is built from this
    /// and checked by [`Store::guard`], so a lock or record can only ever land
    /// inside the one canonical store directory.
    root: PathBuf,
    /// D-015: the ledger MAC key, when the operator configured one (see
    /// [`crate::ledger`]'s threat model). `None` = the historical unkeyed store.
    key: Option<LedgerKey>,
}

/// The operator key source, shared with `axon-vm`'s attestation: hex, at least
/// 16 bytes. There is deliberately no ephemeral fallback: a per-process key
/// cannot verify what a previous process wrote, so "no key" means an unkeyed
/// store, stated as such, never a key that silently authenticates nothing.
pub const LEDGER_KEY_ENV: &str = "AXON_ATTEST_KEY";

/// The ledger MAC key: `HMAC-SHA256(operator_key, domain)`, so the ledger's
/// MACs are never interchangeable with any other use of the operator key.
/// `Debug` is redacted.
#[derive(Clone)]
pub struct LedgerKey([u8; 32]);

impl std::fmt::Debug for LedgerKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LedgerKey(<redacted>)")
    }
}

impl LedgerKey {
    /// Derive the ledger key from operator key bytes (>= 16 bytes).
    pub fn derive(operator_key: &[u8]) -> Result<LedgerKey> {
        if operator_key.len() < 16 {
            return Err(LoopError::Usage(format!(
                "{LEDGER_KEY_ENV} is shorter than 16 bytes: refusing to key the ledger with it"
            )));
        }
        Ok(LedgerKey(axon_attest::hmac_sha256(
            operator_key,
            b"axon-loop ledger key v1",
        )))
    }

    /// `HMAC-SHA256(ledger_key, data)`, the primitive `axon-audit` keys its chain with.
    pub fn mac(&self, data: &[u8]) -> [u8; 32] {
        axon_attest::hmac_sha256(&self.0, data)
    }

    /// The key from [`LEDGER_KEY_ENV`]: unset or blank ⇒ `None` (unkeyed);
    /// set but not hex, or shorter than 16 bytes ⇒ a refusal (exit 2), never a
    /// silent fall back to unkeyed.
    pub fn from_env() -> Result<Option<LedgerKey>> {
        match std::env::var(LEDGER_KEY_ENV) {
            Ok(v) if !v.trim().is_empty() => {
                let bytes = decode_hex(v.trim()).ok_or_else(|| {
                    LoopError::Usage(format!("{LEDGER_KEY_ENV} is not valid hex"))
                })?;
                LedgerKey::derive(&bytes).map(Some)
            }
            Ok(_) | Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(LoopError::Usage(format!(
                "{LEDGER_KEY_ENV} is not valid hex"
            ))),
        }
    }
}

pub(crate) fn decode_hex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(s.get(i..i + 2)?, 16).ok())
        .collect()
}

static TMP_SEQ: AtomicU64 = AtomicU64::new(0);

/// Validate a single store path SEGMENT (experiment id, tenant, family, CAS
/// kind) BEFORE it touches the filesystem: the package id rule, which has no
/// `/`, cannot be `.`/`..` and cannot be absolute.
pub fn check_segment(what: &str, s: &str) -> Result<()> {
    axon_loop_contracts::TaskId::new(s)
        .map(|_| ())
        .map_err(|_| {
            LoopError::Usage(format!(
                "bad {what} {s:?}: must match [A-Za-z0-9][A-Za-z0-9._:-]{{0,127}}"
            ))
        })
}

fn nofollow() -> OpenOptions {
    let mut o = OpenOptions::new();
    o.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC);
    o
}

fn symlink_err(p: &Path) -> LoopError {
    LoopError::Io(format!(
        "symlink in store at {}: store files and directories must be real",
        p.display()
    ))
}

fn map_open(p: &Path, e: std::io::Error) -> LoopError {
    // ELOOP: the final component is a symlink and O_NOFOLLOW refused it.
    if e.raw_os_error() == Some(libc::ELOOP) {
        symlink_err(p)
    } else {
        LoopError::Io(format!("{}: {e}", p.display()))
    }
}

impl Store {
    /// Open a store, keyed by [`LEDGER_KEY_ENV`] when the operator set it.
    pub fn open_dir(root: impl Into<PathBuf>) -> Result<Store> {
        Self::open_dir_keyed(root, LedgerKey::from_env()?)
    }

    /// Open a store with an explicit ledger key (`None` = unkeyed). The key
    /// decides how the ledger is verified; the files never do.
    pub fn open_dir_keyed(root: impl Into<PathBuf>, key: Option<LedgerKey>) -> Result<Store> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let root = fs::canonicalize(&root)?;
        Ok(Store { root, key })
    }

    pub fn ledger_key(&self) -> Option<&LedgerKey> {
        self.key.as_ref()
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Refuse a path that leaves the store or crosses a symlink anywhere
    /// below the root. Components that do not exist yet are fine.
    pub fn guard(&self, p: &Path) -> Result<()> {
        let rel = p
            .strip_prefix(&self.root)
            .map_err(|_| LoopError::Io(format!("{} is outside the store", p.display())))?;
        let mut cur = self.root.clone();
        for c in rel.components() {
            match c {
                Component::Normal(n) => cur.push(n),
                _ => {
                    return Err(LoopError::Io(format!(
                        "non-normal store path {}",
                        p.display()
                    )))
                }
            }
            match fs::symlink_metadata(&cur) {
                Ok(m) if m.file_type().is_symlink() => return Err(symlink_err(&cur)),
                Ok(_) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Create `dir` (and missing parents) inside the store, one real directory
    /// at a time; refuses if any component is (or races into) a symlink.
    pub fn ensure_dir(&self, dir: &Path) -> Result<()> {
        self.guard(dir)?;
        let rel = dir.strip_prefix(&self.root).expect("guarded");
        let mut cur = self.root.clone();
        for c in rel.components() {
            cur.push(c);
            match fs::create_dir(&cur) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
            let m = fs::symlink_metadata(&cur)?;
            if m.file_type().is_symlink() {
                return Err(symlink_err(&cur));
            }
            if !m.is_dir() {
                return Err(LoopError::Io(format!(
                    "{} is not a directory",
                    cur.display()
                )));
            }
        }
        Ok(())
    }

    /// Read a store file; `None` if absent. Never follows a symlink.
    pub fn read_text(&self, p: &Path) -> Result<Option<String>> {
        self.guard(p)?;
        match nofollow().read(true).open(p) {
            Ok(mut f) => {
                let mut s = String::new();
                f.read_to_string(&mut s)?;
                Ok(Some(s))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(map_open(p, e)),
        }
    }

    /// tmp + fsync + rename + directory fsync, inside the store only.
    pub fn write_atomic(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| LoopError::Io(format!("{} has no parent", path.display())))?;
        self.ensure_dir(dir)?;
        self.guard(path)?;
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
            let mut f = nofollow()
                .create_new(true)
                .write(true)
                .open(&tmp)
                .map_err(|e| map_open(&tmp, e))?;
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

    pub fn write_json<T: Serialize>(&self, path: &Path, v: &T) -> Result<()> {
        let mut bytes = serde_json::to_vec_pretty(v).map_err(|e| LoopError::Io(e.to_string()))?;
        bytes.push(b'\n');
        self.write_atomic(path, &bytes)
    }

    /// Append one JSON line and fsync it (and the directory, on creation).
    pub fn append_jsonl<T: Serialize>(&self, path: &Path, v: &T) -> Result<()> {
        let dir = path
            .parent()
            .ok_or_else(|| LoopError::Io("no parent".into()))?;
        self.ensure_dir(dir)?;
        self.guard(path)?;
        let created = fs::symlink_metadata(path).is_err();
        let mut line = serde_json::to_vec(v).map_err(|e| LoopError::Io(e.to_string()))?;
        line.push(b'\n');
        let mut f = nofollow()
            .create(true)
            .append(true)
            .open(path)
            .map_err(|e| map_open(path, e))?;
        f.write_all(&line)?;
        f.sync_all()?;
        if created {
            fsync_dir(dir)?;
        }
        Ok(())
    }

    /// Read a JSONL file strictly; a missing file is empty. A torn LAST line
    /// (a crash mid-append, before its fsync returned) is ignored — that
    /// append was never acknowledged. A malformed line elsewhere is corruption.
    pub fn read_jsonl<T: Serialize + DeserializeOwned>(&self, path: &Path) -> Result<Vec<T>> {
        let Some(s) = self.read_text(path)? else {
            return Ok(Vec::new());
        };
        parse_jsonl(path, &s)
    }

    pub fn read_json<T: Serialize + DeserializeOwned>(&self, path: &Path) -> Result<Option<T>> {
        match self.read_text(path)? {
            Some(s) => strict_record(&s)
                .map(Some)
                .map_err(|e| LoopError::Io(format!("{}: {e}", path.display()))),
            None => Ok(None),
        }
    }

    fn lock_file(&self, rel: &[&str], shared: bool) -> Result<Lock> {
        let mut p = self.root.join("locks");
        for (i, seg) in rel.iter().enumerate() {
            check_segment("lock name", seg)?;
            if i + 1 == rel.len() {
                p.push(format!("{seg}.lock"));
            } else {
                p.push(seg);
            }
        }
        self.ensure_dir(p.parent().expect("has parent"))?;
        self.guard(&p)?;
        let f = nofollow()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&p)
            .map_err(|e| map_open(&p, e))?;
        if shared {
            f.lock_shared()?;
        } else {
            f.lock()?;
        }
        Ok(Lock { _f: f })
    }

    /// The ONE store lock, `<canonical root>/locks/root.lock`. Every
    /// operation that reads or writes the ledger holds it exclusively for its
    /// whole duration (see [`crate::ledger::Tx`]); there is no second lock, so
    /// there is no lock order to get wrong and no lock can be taken on a path
    /// outside the canonical store.
    pub fn lock_root(&self, shared: bool) -> Result<Lock> {
        self.lock_file(&["root"], shared)
    }

    /// The operator config. Absent ⇒ both trusted sets EMPTY, so every
    /// admission, transition and verified outcome is refused (fail closed).
    pub fn config(&self) -> Result<Config> {
        let p = self.root.join("config.json");
        match self.read_text(&p)? {
            Some(s) => {
                // A pre-observer config omits the field; it means the same
                // as an explicit `[]`, so it is read as one and the strict
                // canonical check then applies to the whole record.
                let mut v = axon_loop_contracts::parse_value(&s)?;
                if let Some(o) = v.as_object_mut() {
                    o.entry("trusted_observers")
                        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
                    o.entry("verifier_keys")
                        .or_insert_with(|| serde_json::Value::Object(Default::default()));
                    o.entry("verifier_pins")
                        .or_insert_with(|| serde_json::Value::Object(Default::default()));
                    o.entry("task_acceptance")
                        .or_insert_with(|| serde_json::Value::Object(Default::default()));
                }
                strict_record(&serde_json::to_string(&v).map_err(|e| LoopError::Io(e.to_string()))?)
            }
            None => Ok(Config {
                schema: ConfigSchema,
                trusted_admitters: Vec::new(),
                trusted_verifiers: Vec::new(),
                trusted_observers: Vec::new(),
                verifier_keys: Default::default(),
                verifier_pins: Default::default(),
                task_acceptance: Default::default(),
            }),
        }
    }

    pub fn write_config(&self, c: &Config) -> Result<()> {
        self.write_json(&self.root.join("config.json"), c)
    }

    pub fn scope_dir(&self, scope: &Scope) -> PathBuf {
        // TenantId/TaskFamily are validated newtypes: no `/`, never `..`.
        self.root
            .join("scopes")
            .join(scope.tenant_id.as_str())
            .join(scope.task_family.as_str())
    }

    /// `plans/<id>`; the id is validated BEFORE any path is built.
    pub fn plan_dir(&self, experiment_id: &str) -> Result<PathBuf> {
        check_segment("experiment id", experiment_id)?;
        Ok(self.root.join("plans").join(experiment_id))
    }

    /// Content-addressed path for a `cl22:` record kind.
    pub fn cas_path(&self, kind: &str, r: &Ref) -> Result<PathBuf> {
        check_segment("record kind", kind)?;
        if r.scheme() != RefScheme::Cl22 {
            return Err(crate::error::refused(format!(
                "{kind} references must be cl22:, got {r}"
            )));
        }
        Ok(self.root.join(kind).join(format!("{}.json", r.hex())))
    }

    /// `<kind>/<tenant>/<family>/<hex>.json`: a record whose name is a digest
    /// of SCOPE-INDEPENDENT content (a candidate list, a task manifest) but
    /// whose bytes name a scope. Keying the path by scope means the identical
    /// list registered for two scopes is two files, never one file the second
    /// registration overwrites (NS3).
    pub fn scoped_cas_path(&self, kind: &str, scope: &Scope, r: &Ref) -> Result<PathBuf> {
        let flat = self.cas_path(kind, r)?;
        Ok(self
            .root
            .join(kind)
            .join(scope.tenant_id.as_str())
            .join(scope.task_family.as_str())
            .join(flat.file_name().expect("cas file name")))
    }

    /// Store a record under its own `cl22:` digest. Idempotent.
    pub fn put_cas<T: Serialize>(&self, kind: &str, v: &T) -> Result<Ref> {
        let r = axon_loop_contracts::digest(v)?;
        let p = self.cas_path(kind, &r)?;
        self.guard(&p)?;
        if fs::symlink_metadata(&p).is_err() {
            let bytes = axon_loop_contracts::canonical_json(v)?;
            self.write_atomic(&p, &bytes)?;
        }
        Ok(r)
    }

    fn read_cas(&self, kind: &str, r: &Ref) -> Result<String> {
        let p = self.cas_path(kind, r)?;
        self.read_text(&p)?
            .ok_or_else(|| crate::error::refused(format!("no {kind} record {r}")))
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

    /// Load one of this crate's own records through [`strict_record`], with
    /// the same digest re-check.
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

/// A flock held until drop. Separate `File` handles are separate open file
/// descriptions, so this serialises threads of one process as well as
/// separate processes.
pub struct Lock {
    _f: File,
}

fn fsync_dir(dir: &Path) -> Result<()> {
    File::open(dir)?.sync_all()?;
    Ok(())
}

fn parse_jsonl<T: Serialize + DeserializeOwned>(path: &Path, s: &str) -> Result<Vec<T>> {
    let complete = s.ends_with('\n');
    let lines: Vec<&str> = s.lines().collect();
    let mut out = Vec::with_capacity(lines.len());
    for (i, line) in lines.iter().enumerate() {
        let last = i + 1 == lines.len();
        match strict_record::<T>(line) {
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
