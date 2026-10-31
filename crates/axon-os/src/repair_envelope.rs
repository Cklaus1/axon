//! `axon-approval/4`: the repair envelope (NH-R1, `tasks/unattended-auth-spec.md` §4-§5).
//!
//! An operator-signed AUTHORITY for one unattended `cortex repair` run. It
//! accepts nothing: a repair made under it still needs the existing
//! per-program `/3` approval to deploy (§5.7).
//!
//! # Keys (§3, §4.2)
//!
//! The signature is Ed25519. Its seed is the G5 derivation
//! `HMAC-SHA256(AXON_APPROVAL_KEY, ENVELOPE_SEED_DOMAIN)` (the construction
//! [`crate::approval::ApprovalKey::derive`] uses for its subkeys). The
//! operator holds the [`MintKey`]; the runner holds only a [`VerifyKey`] (a
//! 32-byte public key, type-level unable to sign) and never the master key,
//! so it can neither mint an envelope nor derive the `/3` token or record
//! subkeys.
//!
//! # Verification order (§4)
//!
//! 1. strict parse (`axon_canonical::parse_value`: duplicate keys, floats,
//!    unsafe integers, depth, size);
//! 2. remove `sig`;
//! 3. `canonical_bytes`;
//! 4. Ed25519 verify of `ENVELOPE_SIG_DOMAIN ‖ 0x00 ‖ canonical bytes`;
//! 5. only then the typed parse (`deny_unknown_fields` on every struct) and
//!    the intrinsic and time checks.
//!
//! `envelope_digest = cl22(canonical bytes of the whole envelope, sig included)`.
//!
//! # Single use (§5.5)
//!
//! The spent store is a private directory holding `store-id` (written once by
//! [`init_store`]) and one `<envelope_id>.issued` per minted envelope
//! ([`issue`]). [`consume`] renames `.issued` to `.used` without replacing
//! (`RENAME_NOREPLACE`), so of any number of concurrent consumers exactly one
//! wins, then records `{envelope_digest, spent_unix, pid}` and fsyncs the file
//! and the directory. HONEST LIMIT: the store is same-uid; it holds only
//! because the enforcement gate (§6) refuses every confinement escape.

use crate::private_path::{check_private, Private};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::io::Write;
use std::path::{Component, Path, PathBuf};

/// The envelope schema. `/2` is reserved (R24) and `/3` is the program
/// approval token, so the envelope is `/4`.
pub const ENVELOPE_SCHEMA: &str = "axon-approval/4";

/// The G5 derivation label of the envelope's Ed25519 seed:
/// `seed = HMAC-SHA256(operator_key, ENVELOPE_SEED_DOMAIN)`.
pub const ENVELOPE_SEED_DOMAIN: &str = "axon-approval repair-envelope v1";

/// The context every envelope signature is made over: the signed message is
/// `ENVELOPE_SIG_DOMAIN ‖ 0x00 ‖ canonical bytes`, so an envelope signature is
/// never a signature of any other protocol's bytes, even under a reused key.
pub const ENVELOPE_SIG_DOMAIN: &str = "axon-approval/4 repair-envelope signature";

/// The prefix of the `sig` field: `axenv1:` + 128 lowercase hex.
pub const SIG_PREFIX: &str = "axenv1:";

/// The longest validity window an envelope may carry (`expires - issued`).
pub const MAX_TTL_S: u64 = 86_400;

/// How far in the future `issued_unix` may lie (clock skew between minter and runner).
pub const ISSUED_SKEW_S: u64 = 60;

/// The only `decision` an envelope can carry.
pub const DECISION: &str = "authorize-operate";

/// The only generator kind v1 admits (§10 Q2: `ai:` generators only).
pub const GENERATOR_KIND: &str = "ai";

/// The spent store's identity file.
pub const STORE_ID_FILE: &str = "store-id";

/// The runner's verify-key file: owner = euid, no group/other access, a
/// regular file, never a symlink (§4.2).
pub const VERIFY_KEY: Private = Private {
    what: "repair-envelope verify key",
    whose: "the runner's",
    dir: false,
    takeover: "substitute a key whose envelopes it mints itself",
    exposure: "replace the key with one it can sign for",
};

/// The spent store (§5.5): a private directory.
pub const SPENT_STORE: Private = Private {
    what: "repair-envelope spent store",
    whose: "the runner's",
    dir: true,
    takeover: "issue envelopes or erase their spent records",
    exposure: "plant `.issued` records or erase `.used` ones (a spent envelope spends again)",
};

/// The task an envelope authorizes repairing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Task {
    /// Canonical absolute path of the workspace.
    pub workspace: String,
    /// Workspace-relative, normalized entry file.
    pub file: String,
    /// The symbol under repair (required).
    pub symbol: String,
    /// Lowercase-hex sha256 of the entry file bytes at mint.
    pub pristine_sha256: String,
    /// The import-closure digest `/3` binds (`axsha256:` + 64 hex).
    pub imports_digest: String,
}

/// The check child's pins for every kind the pristine program uses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Scopes {
    pub fs_read: Vec<String>,
    pub fs_write: Vec<String>,
    pub net: Vec<String>,
}

/// The pinned capabilities (§4, §5.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Capabilities {
    /// `Fence::sha256()` of the pinned fence (lowercase hex).
    pub fence_sha256: String,
    /// The static grant of `task.symbol`, as cortex serializes its
    /// `StaticGrant`. Opaque here: the runner compares it by its cl22 bytes
    /// against the grant it derives (phase B step 9), so any field the
    /// derived grant lacks is a mismatch.
    pub target_grant: Value,
}

/// The hidden check (§4.1).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Check {
    pub name: String,
    /// Lowercase-hex sha256 of the cl22 check-digest input.
    pub digest: String,
}

/// The run's limits. Every field is required and at least 1; a CLI value may
/// only narrow them (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Budget {
    pub max_proposals: u64,
    pub deadline_ms: u64,
    pub check_timeout_ms: u64,
    pub candidates: u64,
    pub max_steps: u64,
}

/// The patch generator; `provider` and `endpoint_host` are compared against
/// what axon-ai resolves.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub kind: String,
    pub model: String,
    pub provider: String,
    pub endpoint_host: String,
}

/// An `axon-approval/4` repair envelope.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RepairEnvelope {
    pub schema: String,
    /// 32 lowercase hex, OS RNG at mint: the run identity, single-use.
    pub envelope_id: String,
    /// The id of the runner's spent store this envelope was issued into.
    pub spent_store_id: String,
    /// The `ledger-id` of the run ledger the run records into (R6 §4 sink).
    pub run_ledger_id: String,
    /// Operator label: recorded, not trusted.
    pub issued_by: String,
    pub issued_unix: u64,
    pub expires_unix: u64,
    pub task: Task,
    /// Cortex's patch authority: normalized relative paths.
    pub write_prefixes: Vec<String>,
    pub scopes: Scopes,
    pub capabilities: Capabilities,
    pub check: Check,
    pub budget: Budget,
    pub generator: Generator,
    pub decision: String,
    /// `axenv1:` + hex Ed25519 signature (see the module docs).
    pub sig: String,
}

/// A verified envelope and its digest.
#[derive(Debug, Clone, PartialEq)]
pub struct VerifiedEnvelope {
    pub envelope: RepairEnvelope,
    /// `cl22:` digest of the whole envelope, `sig` included.
    pub digest: String,
}

// ── keys ─────────────────────────────────────────────────────────────────────

/// The runner's key: an Ed25519 PUBLIC key. It has no signing method; the
/// runner cannot mint (S3, type level):
///
/// ```compile_fail
/// let k = axon_os::repair_envelope::VerifyKey::from_hex(&"0".repeat(64)).unwrap();
/// let _ = k.sign(b"x");
/// ```
#[derive(Clone, PartialEq, Eq)]
pub struct VerifyKey([u8; 32]);

impl std::fmt::Debug for VerifyKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "VerifyKey({})", self.to_hex())
    }
}

impl VerifyKey {
    /// A key from 64 lowercase hex characters.
    pub fn from_hex(s: &str) -> Result<VerifyKey, String> {
        if !is_lower_hex(s, 64) {
            return Err("repair-envelope verify key is not 64 lowercase hex characters".into());
        }
        let b = decode_hex(s).ok_or("repair-envelope verify key is not hex")?;
        let mut k = [0u8; 32];
        k.copy_from_slice(&b);
        Ok(VerifyKey(k))
    }

    /// Lowercase hex of the public key.
    pub fn to_hex(&self) -> String {
        hex(&self.0)
    }

    /// Load the runner's key file (§4.2): owned by `euid`, mode `& 0o077 == 0`,
    /// a regular file and not a symlink, and outside every path in `outside`
    /// (the canonical workspace and every canonical prefix; compared after
    /// resolving the key file's directory).
    pub fn load(path: &Path, euid: u32, outside: &[PathBuf]) -> Result<VerifyKey, String> {
        let text = read_key_file(path, euid, &VERIFY_KEY, outside)?;
        VerifyKey::from_hex(text.trim_end_matches('\n'))
    }
}

/// Read a runner key file (§4.2; the verify key and R6's run-record key):
/// private to `euid` ([`check_private`]) and outside every path in `outside`,
/// compared after resolving the key file's directory.
pub(crate) fn read_key_file(
    path: &Path,
    euid: u32,
    p: &Private,
    outside: &[PathBuf],
) -> Result<String, String> {
    check_private(path, euid, p)?;
    let what = p.what;
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let name = path
        .file_name()
        .ok_or_else(|| format!("{what} {} names no file", path.display()))?;
    let real = std::fs::canonicalize(parent)
        .map_err(|e| format!("{what} {}: {e}", path.display()))?
        .join(name);
    for root in outside {
        if real.starts_with(root) {
            return Err(format!(
                "{what} {} lies inside {}: a key the repaired code can reach is not the \
                 operator's",
                real.display(),
                root.display()
            ));
        }
    }
    std::fs::read_to_string(path).map_err(|e| format!("{what} {}: {e}", path.display()))
}

/// The operator's minting key, derived from `AXON_APPROVAL_KEY`. `Debug` is redacted.
pub struct MintKey(ring::signature::Ed25519KeyPair);

impl std::fmt::Debug for MintKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("MintKey(<redacted>)")
    }
}

impl MintKey {
    /// `seed = HMAC-SHA256(operator_key, ENVELOPE_SEED_DOMAIN)`; operator key >= 16 bytes.
    pub fn derive(operator_key: &[u8]) -> Result<MintKey, String> {
        if operator_key.len() < 16 {
            return Err(format!(
                "{} is shorter than 16 bytes: refusing to mint repair envelopes with it",
                crate::approval::APPROVAL_KEY_ENV
            ));
        }
        let seed = axon_attest::hmac_sha256(operator_key, ENVELOPE_SEED_DOMAIN.as_bytes());
        let kp = ring::signature::Ed25519KeyPair::from_seed_unchecked(&seed)
            .map_err(|_| "repair-envelope seed is not an Ed25519 seed".to_string())?;
        Ok(MintKey(kp))
    }

    /// The key from `AXON_APPROVAL_KEY` (hex). Unset or blank is an error:
    /// an envelope cannot be minted without the operator key.
    pub fn from_env() -> Result<MintKey, String> {
        MintKey::derive(&operator_key()?)
    }

    /// The public half, for `--export-verify-key`.
    pub fn verify_key(&self) -> VerifyKey {
        use ring::signature::KeyPair;
        let mut k = [0u8; 32];
        k.copy_from_slice(self.0.public_key().as_ref());
        VerifyKey(k)
    }

    /// Sign `env` (its `sig` is ignored and replaced). Refuses an envelope
    /// that fails the intrinsic checks. Returns the envelope's canonical JSON.
    pub fn sign(&self, env: &RepairEnvelope) -> Result<String, String> {
        validate(env)?;
        // A struct serializes to an object: the map is taken by value, so the
        // `sig` slot is replaced without a second (unreachable) kind check.
        let Value::Object(mut obj) =
            serde_json::to_value(env).map_err(|e| format!("repair envelope: {e}"))?
        else {
            return Err("repair envelope is not an object".into());
        };
        obj.remove("sig");
        let mut v = Value::Object(obj);
        let msg = signed_message(&v)?;
        let sig = format!("{SIG_PREFIX}{}", hex(self.0.sign(&msg).as_ref()));
        if let Value::Object(o) = &mut v {
            o.insert("sig".into(), Value::String(sig));
        }
        let bytes =
            axon_canonical::canonical_bytes(&v).map_err(|e| format!("repair envelope: {e}"))?;
        String::from_utf8(bytes).map_err(|e| format!("repair envelope: {e}"))
    }
}

/// The operator key from `AXON_APPROVAL_KEY` (hex): the input to every
/// operator-side derivation here (the envelope seed, R6's run-record subkey).
/// Unset or blank is an error.
pub fn operator_key() -> Result<Vec<u8>, String> {
    let env = crate::approval::APPROVAL_KEY_ENV;
    let v = std::env::var(env).map_err(|_| format!("{env} is not set: only the operator mints"))?;
    let v = v.trim();
    if v.is_empty() {
        return Err(format!("{env} is not set: only the operator mints"));
    }
    decode_hex(v).ok_or_else(|| format!("{env} is not valid hex"))
}

/// A fresh 32-lowercase-hex identifier from the OS RNG (envelope and store ids).
pub fn new_id() -> Result<String, String> {
    use ring::rand::SecureRandom;
    let mut b = [0u8; 16];
    ring::rand::SystemRandom::new()
        .fill(&mut b)
        .map_err(|_| "the OS random source failed".to_string())?;
    Ok(hex(&b))
}

fn signed_message(unsigned: &Value) -> Result<Vec<u8>, String> {
    let body =
        axon_canonical::canonical_bytes(unsigned).map_err(|e| format!("repair envelope: {e}"))?;
    let mut msg = Vec::with_capacity(ENVELOPE_SIG_DOMAIN.len() + 1 + body.len());
    msg.extend_from_slice(ENVELOPE_SIG_DOMAIN.as_bytes());
    msg.push(0);
    msg.extend_from_slice(&body);
    Ok(msg)
}

// ── verify ───────────────────────────────────────────────────────────────────

/// Verify an envelope's bytes under the runner's key at `now_unix` (§4, §5.3
/// steps 2, 4 and the first three rules of 5). Every refusal is an `Err`;
/// nothing here touches the filesystem.
pub fn verify(json: &str, key: &VerifyKey, now_unix: u64) -> Result<VerifiedEnvelope, String> {
    // 1. strict parse
    let mut v = axon_canonical::parse_value(json).map_err(|e| format!("repair envelope: {e}"))?;
    let schema = v.get("schema").and_then(Value::as_str).unwrap_or("");
    if schema != ENVELOPE_SCHEMA {
        if schema.starts_with("axon-approval/") {
            return Err(format!(
                "an {schema} approval token is not a repair envelope (want {ENVELOPE_SCHEMA})"
            ));
        }
        return Err(format!(
            "repair envelope schema is {schema:?}, not {ENVELOPE_SCHEMA}"
        ));
    }
    let digest = axon_canonical::digest_value(&v).map_err(|e| format!("repair envelope: {e}"))?;
    // 2. remove sig
    let sig = match v.as_object_mut().and_then(|o| o.remove("sig")) {
        Some(Value::String(s)) => s,
        Some(_) => return Err("repair envelope `sig` is not a string".into()),
        None => return Err("repair envelope has no `sig`".into()),
    };
    let sig_hex = sig
        .strip_prefix(SIG_PREFIX)
        .ok_or_else(|| format!("repair envelope `sig` does not start with {SIG_PREFIX}"))?;
    if !is_lower_hex(sig_hex, 128) {
        return Err("repair envelope `sig` is not 128 lowercase hex characters".into());
    }
    let sig_bytes = decode_hex(sig_hex).ok_or("repair envelope `sig` is not hex")?;
    // 3. canonical bytes, 4. signature
    let msg = signed_message(&v)?;
    ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key.0)
        .verify(&msg, &sig_bytes)
        .map_err(|_| {
            "repair envelope signature does not verify under the runner's key".to_string()
        })?;
    // 5. typed parse, only now
    // `v` is an object (step 1 read its `schema`); were it not, the typed
    // parse below would refuse it.
    if let Some(o) = v.as_object_mut() {
        o.insert("sig".into(), Value::String(sig));
    }
    let envelope: RepairEnvelope =
        serde_json::from_value(v).map_err(|e| format!("repair envelope: {e}"))?;
    validate(&envelope)?;
    check_time(&envelope, now_unix)?;
    Ok(VerifiedEnvelope { envelope, digest })
}

/// The time rules that need no run limits: `issued ≤ now + ISSUED_SKEW_S`,
/// `now < expires`. (`expires - issued ≤ MAX_TTL_S` is intrinsic.)
pub fn check_time(env: &RepairEnvelope, now_unix: u64) -> Result<(), String> {
    if env.issued_unix > now_unix.saturating_add(ISSUED_SKEW_S) {
        return Err(format!(
            "repair envelope is not yet valid: issued_unix {} is more than {ISSUED_SKEW_S} s after \
             now ({now_unix})",
            env.issued_unix
        ));
    }
    if now_unix >= env.expires_unix {
        return Err(format!(
            "repair envelope expired at {} (now {now_unix})",
            env.expires_unix
        ));
    }
    Ok(())
}

impl RepairEnvelope {
    /// §5.3 step 5's last rule: the run, at its effective deadline plus the
    /// runner's hard grace, ends before the envelope expires.
    pub fn fits_before_expiry(
        &self,
        now_unix: u64,
        deadline_ms: u64,
        hard_grace_ms: u64,
    ) -> Result<(), String> {
        let end_ms =
            u128::from(now_unix) * 1000 + u128::from(deadline_ms) + u128::from(hard_grace_ms);
        if end_ms > u128::from(self.expires_unix) * 1000 {
            return Err(format!(
                "repair envelope expires at {} but the run may last until {} ms (deadline {} ms + \
                 grace {} ms from now {now_unix})",
                self.expires_unix, end_ms, deadline_ms, hard_grace_ms
            ));
        }
        Ok(())
    }

    /// The run principal (§5.2).
    pub fn principal(&self) -> String {
        format!("envelope:{}", self.envelope_id)
    }

    /// §5.2 no-widen comparisons. Absent CLI values take the envelope's; a
    /// present one must not exceed it. CLI write prefixes must each lie
    /// inside (component-wise, after normalization) an envelope prefix; none
    /// given means the envelope's.
    pub fn narrow(&self, cli: &Requested) -> Result<Effective, String> {
        let b = &self.budget;
        let pick = |name: &str, c: Option<u64>, e: u64| -> Result<u64, String> {
            match c {
                None => Ok(e),
                Some(c) if c <= e => Ok(c),
                Some(c) => Err(format!(
                    "{name} {c} exceeds the envelope's {e}: unattended limits only narrow"
                )),
            }
        };
        let write_prefixes = if cli.write_prefixes.is_empty() {
            self.write_prefixes.clone()
        } else {
            let mut out = Vec::new();
            for p in &cli.write_prefixes {
                let n = normalized_rel(p).ok_or_else(|| {
                    format!("--write-prefix {p:?} is not a normalized relative path")
                })?;
                if !self.write_prefixes.iter().any(|e| rel_within(&n, e)) {
                    return Err(format!(
                        "--write-prefix {p:?} is not inside any envelope write prefix ({})",
                        self.write_prefixes.join(", ")
                    ));
                }
                out.push(n);
            }
            out
        };
        Ok(Effective {
            max_proposals: pick("--max-proposals", cli.max_proposals, b.max_proposals)?,
            deadline_ms: pick("--deadline-ms", cli.deadline_ms, b.deadline_ms)?,
            check_timeout_ms: pick(
                "--check-timeout-ms",
                cli.check_timeout_ms,
                b.check_timeout_ms,
            )?,
            candidates: pick("--candidates", cli.candidates, b.candidates)?,
            max_steps: pick("--budget", cli.max_steps, b.max_steps)?,
            write_prefixes,
        })
    }

    /// §5.2 generator rule: equal, including the resolved provider and endpoint host.
    pub fn check_generator(
        &self,
        kind: &str,
        model: &str,
        provider: &str,
        endpoint_host: &str,
    ) -> Result<(), String> {
        let g = &self.generator;
        for (what, want, got) in [
            ("kind", g.kind.as_str(), kind),
            ("model", g.model.as_str(), model),
            ("provider", g.provider.as_str(), provider),
            ("endpoint host", g.endpoint_host.as_str(), endpoint_host),
        ] {
            if want != got {
                return Err(format!(
                    "generator {what} {got:?} is not the envelope's {want:?}"
                ));
            }
        }
        Ok(())
    }
}

/// What the unattended command line asks for (`None` = flag absent).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Requested {
    pub max_proposals: Option<u64>,
    pub deadline_ms: Option<u64>,
    pub check_timeout_ms: Option<u64>,
    pub candidates: Option<u64>,
    pub max_steps: Option<u64>,
    pub write_prefixes: Vec<String>,
}

/// The limits the run uses: each ≤ the envelope's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effective {
    pub max_proposals: u64,
    pub deadline_ms: u64,
    pub check_timeout_ms: u64,
    pub candidates: u64,
    pub max_steps: u64,
    pub write_prefixes: Vec<String>,
}

/// The intrinsic (no-I/O) checks: every field's shape, and the relations
/// between fields that need no filesystem.
pub fn validate(e: &RepairEnvelope) -> Result<(), String> {
    if e.schema != ENVELOPE_SCHEMA {
        return Err(format!(
            "repair envelope schema is {:?}, not {ENVELOPE_SCHEMA}",
            e.schema
        ));
    }
    if !is_lower_hex(&e.envelope_id, 32) {
        return Err("envelope_id is not 32 lowercase hex characters".into());
    }
    if !is_lower_hex(&e.spent_store_id, 32) {
        return Err("spent_store_id is not 32 lowercase hex characters".into());
    }
    if !is_lower_hex(&e.run_ledger_id, 32) {
        return Err("run_ledger_id is not 32 lowercase hex characters".into());
    }
    if e.issued_by.trim().is_empty() {
        return Err("issued_by is empty".into());
    }
    if e.expires_unix <= e.issued_unix {
        return Err("expires_unix is not after issued_unix".into());
    }
    // Saturating: the order check above is the ONLY refusal of an envelope
    // expiring before its issue (never an overflow panic standing in for it).
    if e.expires_unix.saturating_sub(e.issued_unix) > MAX_TTL_S {
        return Err(format!(
            "repair envelope lives {} s, more than MAX_TTL_S ({MAX_TTL_S})",
            e.expires_unix - e.issued_unix
        ));
    }
    let t = &e.task;
    if normalized_abs(&t.workspace).is_none() {
        return Err(format!(
            "task.workspace {:?} is not a normalized absolute path",
            t.workspace
        ));
    }
    if normalized_rel(&t.file).as_deref() != Some(t.file.as_str()) {
        return Err(format!(
            "task.file {:?} is not a normalized relative path",
            t.file
        ));
    }
    if t.symbol.is_empty() {
        return Err("task.symbol is empty: an unattended repair names its symbol".into());
    }
    if !is_lower_hex(&t.pristine_sha256, 64) {
        return Err("task.pristine_sha256 is not 64 lowercase hex characters".into());
    }
    if !t
        .imports_digest
        .strip_prefix("axsha256:")
        .is_some_and(|h| is_lower_hex(h, 64))
    {
        return Err("task.imports_digest is not axsha256: + 64 lowercase hex".into());
    }
    if e.write_prefixes.is_empty() {
        return Err("write_prefixes is empty".into());
    }
    for p in &e.write_prefixes {
        if normalized_rel(p).as_deref() != Some(p.as_str()) {
            return Err(format!(
                "write prefix {p:?} is not a normalized relative path"
            ));
        }
    }
    for (kind, list) in [
        ("fs_read", &e.scopes.fs_read),
        ("fs_write", &e.scopes.fs_write),
    ] {
        for s in list {
            if normalized_rel(s).as_deref() != Some(s.as_str())
                && normalized_abs(s).as_deref() != Some(s.as_str())
            {
                return Err(format!(
                    "scopes.{kind} entry {s:?} is not a normalized path"
                ));
            }
        }
    }
    for h in &e.scopes.net {
        if h.is_empty() || h.contains('*') || h.chars().any(|c| c.is_whitespace() || c == '/') {
            return Err(format!("scopes.net entry {h:?} is not one host"));
        }
    }
    let file_abs = format!("{}/{}", t.workspace.trim_end_matches('/'), t.file);
    for s in &e.scopes.fs_write {
        let abs = if s.starts_with('/') {
            s.clone()
        } else {
            format!("{}/{s}", t.workspace.trim_end_matches('/'))
        };
        if abs_overlap(&abs, &file_abs) {
            return Err(format!(
                "scopes.fs_write entry {s:?} reaches the task file {:?}: the check program may \
                 never write the code under repair",
                t.file
            ));
        }
    }
    if !is_lower_hex(&e.capabilities.fence_sha256, 64) {
        return Err("capabilities.fence_sha256 is not 64 lowercase hex characters".into());
    }
    if !e.capabilities.target_grant.is_object() {
        return Err("capabilities.target_grant is not an object".into());
    }
    if e.check.name.is_empty() {
        return Err("check.name is empty".into());
    }
    if !is_lower_hex(&e.check.digest, 64) {
        return Err("check.digest is not 64 lowercase hex characters".into());
    }
    let b = &e.budget;
    for (name, n) in [
        ("max_proposals", b.max_proposals),
        ("deadline_ms", b.deadline_ms),
        ("check_timeout_ms", b.check_timeout_ms),
        ("candidates", b.candidates),
        ("max_steps", b.max_steps),
    ] {
        if n == 0 {
            return Err(format!("budget.{name} is 0: every limit is at least 1"));
        }
    }
    let g = &e.generator;
    if g.kind != GENERATOR_KIND {
        return Err(format!(
            "generator.kind {:?} is not {GENERATOR_KIND:?} (v1 admits ai: generators only)",
            g.kind
        ));
    }
    for (name, v) in [
        ("model", &g.model),
        ("provider", &g.provider),
        ("endpoint_host", &g.endpoint_host),
    ] {
        if v.is_empty() {
            return Err(format!("generator.{name} is empty"));
        }
    }
    if e.decision != DECISION {
        return Err(format!("decision {:?} is not {DECISION:?}", e.decision));
    }
    Ok(())
}

// ── the spent store ──────────────────────────────────────────────────────────

/// Initialise an EMPTY private store: writes `store-id` once (create_new) and
/// returns it. A store that already has an id is refused.
pub fn init_store(dir: &Path, euid: u32) -> Result<String, String> {
    check_private(dir, euid, &SPENT_STORE)?;
    let id = new_id()?;
    let p = dir.join(STORE_ID_FILE);
    write_new(&p, format!("{id}\n").as_bytes()).map_err(|e| {
        format!(
            "spent store {}: cannot create {STORE_ID_FILE}: {e}",
            dir.display()
        )
    })?;
    sync_dir(dir)?;
    Ok(id)
}

/// The store's id, after checking the store is private.
pub fn store_id(dir: &Path, euid: u32) -> Result<String, String> {
    check_private(dir, euid, &SPENT_STORE)?;
    let p = dir.join(STORE_ID_FILE);
    let m = std::fs::symlink_metadata(&p)
        .map_err(|e| format!("spent store {}: no {STORE_ID_FILE}: {e}", dir.display()))?;
    if !m.is_file() {
        return Err(format!(
            "spent store {}: {STORE_ID_FILE} is not a regular file",
            dir.display()
        ));
    }
    let id = std::fs::read_to_string(&p)
        .map_err(|e| format!("spent store {}: {STORE_ID_FILE}: {e}", dir.display()))?;
    let id = id.trim_end_matches('\n');
    if !is_lower_hex(id, 32) {
        return Err(format!(
            "spent store {}: {STORE_ID_FILE} is not 32 lowercase hex",
            dir.display()
        ));
    }
    Ok(id.to_string())
}

/// The minter's step (§5.1 step 4): `<envelope_id>.issued`, create_new. An id
/// already issued or used is refused.
pub fn issue(
    dir: &Path,
    euid: u32,
    store: &str,
    envelope_id: &str,
    issued_unix: u64,
) -> Result<(), String> {
    if !is_lower_hex(envelope_id, 32) {
        return Err("envelope_id is not 32 lowercase hex characters".into());
    }
    let have = store_id(dir, euid)?;
    if have != store {
        return Err(format!(
            "spent store {} has id {have}, not {store}",
            dir.display()
        ));
    }
    if std::fs::symlink_metadata(dir.join(format!("{envelope_id}.used"))).is_ok() {
        return Err(format!("envelope {envelope_id} was already used"));
    }
    let rec = serde_json::json!({"envelope_id": envelope_id, "issued_unix": issued_unix});
    write_new(
        &dir.join(format!("{envelope_id}.issued")),
        format!("{rec}\n").as_bytes(),
    )
    .map_err(|e| format!("envelope {envelope_id}: cannot record it as issued: {e}"))?;
    sync_dir(dir)
}

/// Spend a verified envelope (§5.5): the LAST check of phase B. Exactly one
/// of any number of concurrent consumers succeeds.
pub fn consume(dir: &Path, euid: u32, v: &VerifiedEnvelope, now_unix: u64) -> Result<(), String> {
    let id = &v.envelope.envelope_id;
    let have = store_id(dir, euid)?;
    if have != v.envelope.spent_store_id {
        return Err(format!(
            "spent store {} has id {have}, but the envelope was issued into {}",
            dir.display(),
            v.envelope.spent_store_id
        ));
    }
    let issued = dir.join(format!("{id}.issued"));
    let used = dir.join(format!("{id}.used"));
    match std::fs::symlink_metadata(&issued) {
        Ok(m) if m.is_file() => {}
        Ok(_) => {
            return Err(format!(
                "envelope {id}: its .issued record is not a regular file"
            ))
        }
        Err(_) => {
            if std::fs::symlink_metadata(&used).is_ok() {
                return Err(format!("envelope {id} was already used"));
            }
            return Err(format!("envelope {id} was never issued into this store"));
        }
    }
    rename_noreplace(&issued, &used).map_err(|e| {
        if std::fs::symlink_metadata(&used).is_ok() {
            format!("envelope {id} was already used")
        } else {
            format!("envelope {id}: cannot spend it: {e}")
        }
    })?;
    let rec = serde_json::json!({
        "envelope_digest": v.digest,
        "spent_unix": now_unix,
        "pid": std::process::id(),
    });
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .truncate(true)
        .open(&used)
        .map_err(|e| format!("envelope {id}: spent, but its record cannot be written: {e}"))?;
    f.write_all(format!("{rec}\n").as_bytes())
        .and_then(|_| f.sync_all())
        .map_err(|e| format!("envelope {id}: spent, but its record cannot be written: {e}"))?;
    sync_dir(dir)
}

fn rename_noreplace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    let a = std::ffi::CString::new(from.as_os_str().as_bytes())?;
    let b = std::ffi::CString::new(to.as_os_str().as_bytes())?;
    // SAFETY: both are valid NUL-terminated paths; AT_FDCWD resolves them as given.
    let rc = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            a.as_ptr(),
            libc::AT_FDCWD,
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

pub(crate) fn write_new(p: &Path, bytes: &[u8]) -> std::io::Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(p)?;
    f.write_all(bytes)?;
    f.sync_all()
}

fn sync_dir(dir: &Path) -> Result<(), String> {
    std::fs::File::open(dir)
        .and_then(|d| d.sync_all())
        .map_err(|e| format!("spent store {}: fsync: {e}", dir.display()))
}

// ── helpers ──────────────────────────────────────────────────────────────────

pub(crate) fn is_lower_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub(crate) fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
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

/// `s` itself when it is a normalized relative path: non-empty components,
/// none `.` or `..`, no `*`, no NUL, not absolute, no trailing slash.
fn normalized_rel(s: &str) -> Option<String> {
    if s.is_empty() || s.starts_with('/') || s.contains('*') || s.contains('\0') {
        return None;
    }
    if s.split('/').any(|c| c.is_empty() || c == "." || c == "..") {
        return None;
    }
    Some(s.to_string())
}

/// `s` itself when it is a normalized absolute path (`/` or `/a/b`).
fn normalized_abs(s: &str) -> Option<String> {
    if s == "/" {
        return Some(s.into());
    }
    let rest = s.strip_prefix('/')?;
    normalized_rel(rest).map(|_| s.to_string())
}

/// `a` is `b` or lies under it, component-wise.
fn rel_within(a: &str, b: &str) -> bool {
    Path::new(a).starts_with(Path::new(b))
}

/// Two absolute paths overlap when either lies within the other.
fn abs_overlap(a: &str, b: &str) -> bool {
    let (a, b) = (Path::new(a), Path::new(b));
    let norm = |p: &Path| {
        p.components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
    };
    !norm(a) || !norm(b) || a.starts_with(b) || b.starts_with(a)
}

#[cfg(test)]
mod tests;
