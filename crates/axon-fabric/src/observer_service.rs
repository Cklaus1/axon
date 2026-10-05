//! The OBSERVER SERVICE (`axon-observer`; operator decisions G and G1,
//! 2026-10-04; `governance/specs/v022-psv-protocol.md` amendment 68).
//!
//! Before this, Fabric executed the observer program AS THE FABRIC UID, so
//! the observer's signing key was readable by the principal the observation
//! constrains: Fabric could mint any observation. The observer is now its own
//! principal, in the custodian's shape (amendment 50):
//!
//! * it runs as its OWN uid (never the Fabric's, never root), socket-activated
//!   by systemd, and reads its config only from [`CONFIG_PATH`];
//! * its key is a file only that uid can read ([`load_key`]: owned by it,
//!   mode 0400, so a POSIX ACL granting another uid read shows in the group
//!   bits and is refused too), whose public half is in the operator observer
//!   root and in no other root ([`key_in_root`]);
//! * its only caller is uid 0: the setuid-root `axon-protected-launcher
//!   --observe`, which authenticates the observer PROGRAM by the kernel's
//!   `SCM_PIDFD` of every reply message (the custodian's
//!   [`crate::custodian::check_sender_program`], decision G1 = A) and measures
//!   the RUNNING Fabric for it;
//! * it signs ONLY what it measured: every installed digest the launch
//!   manifest names is re-measured from operator files and must be equal, or
//!   nothing is signed ([`Server::measure`]); and it signs at most ONE
//!   observation per nonce, recorded in its own 0700 store.
//!
//! What it is TOLD and cannot measure (the guest policy, the nonce and epoch,
//! the init and axon digests inside the measured rootfs, the Fabric revision)
//! is stated in amendment 68.

use crate::backend::{Clock, TrustAuthority};
use crate::custodian::{peer_uid, Mode};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// The observer's operator config (protected mode).
pub const CONFIG_PATH: &str = "/etc/axon/observer.json";
pub const CONFIG_SCHEMA: &str = "axon-observer/1";
pub const REQUEST_SCHEMA: &str = "axon-observer-request/1";
pub const REPLY_SCHEMA: &str = "axon-observer-reply/1";

/// A launch manifest is a few KiB; the bound is generous and fixed.
pub const MAX_REQUEST: u64 = 64 << 10;
/// An observation and its signature, JSON-escaped in the reply.
pub const MAX_REPLY: u64 = 64 << 10;
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Where a TEST observer measures (a test-trust `--test-config` only; a
/// protected observer measures the fixed operator paths).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct TestPaths {
    pub trust_root: PathBuf,
    pub host_config: PathBuf,
    pub helper_config: PathBuf,
}

/// `axon-observer/1`, the operator's.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObserverServiceConfig {
    pub schema: String,
    /// The uid the observer runs as (systemd `User=`): the only uid that can
    /// read its key. Never the Fabric's, never 0.
    pub observer_uid: u32,
    /// The Fabric service's uid: the principal the observation constrains.
    pub fabric_uid: u32,
    /// The only uid an observation is made for: 0, the setuid-root helper's
    /// relay (decision G1 = A).
    pub caller_uid: u32,
    /// The socket systemd listens on; an activation on any other is refused.
    pub socket: PathBuf,
    /// The observer's own store (one record per observed nonce): its uid's,
    /// 0700.
    pub store: PathBuf,
    /// The observer's PKCS#8 Ed25519 key: its uid's, 0400.
    pub key_path: PathBuf,
    /// TEST configs only; a protected config naming them is refused.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_paths: Option<TestPaths>,
}

fn plain_absolute(p: &Path) -> bool {
    p.is_absolute()
        && p.components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl ObserverServiceConfig {
    /// The config's own rules. `protected`: the observer, the Fabric and the
    /// caller are three principals. An observer running as the Fabric uid is
    /// the defect amendment 68 removes (its key would be Fabric's to read);
    /// a root observer or a root Fabric has no boundary at all; and the only
    /// caller is the root helper, which measures the running Fabric and
    /// authenticates this program for it.
    pub fn check(&self, protected: bool) -> Result<(), String> {
        if self.schema != CONFIG_SCHEMA {
            return Err(format!("schema is not {CONFIG_SCHEMA}"));
        }
        for p in [&self.socket, &self.store, &self.key_path] {
            if !plain_absolute(p) {
                return Err(format!("{} is not an absolute plain path", p.display()));
            }
        }
        if !protected {
            return Ok(());
        }
        if self.observer_uid == self.fabric_uid || self.observer_uid == 0 || self.fabric_uid == 0 {
            return Err(format!(
                "observer_uid {} / fabric_uid {}: the observer runs as its own uid, neither the \
                 Fabric's nor root, or its key is readable by the principal the observation \
                 constrains (amendment 68)",
                self.observer_uid, self.fabric_uid
            ));
        }
        if self.caller_uid != 0 {
            return Err(format!(
                "caller_uid is {}, not 0: an observation is made only for the setuid-root \
                 helper's relay, which measures the running Fabric and authenticates this \
                 program (amendment 68, G1)",
                self.caller_uid
            ));
        }
        if self.test_paths.is_some() {
            return Err(
                "test_paths is a test-config key: a protected observer measures the \
                        operator's fixed paths"
                    .into(),
            );
        }
        Ok(())
    }
}

/// Read the observer's operator config at `path` (walked from the authority's
/// base, operator-owned; [`crate::privileged_launcher::read_operator_file`]).
pub fn load_config(
    path: &Path,
    a: &crate::privileged_launcher::Authority,
) -> Result<ObserverServiceConfig, String> {
    let bytes = crate::privileged_launcher::read_operator_file(path, a)?;
    let c: ObserverServiceConfig =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    c.check(!a.test)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(c)
}

/// The observer's signing key, read through ONE `O_NOFOLLOW` open: the file
/// whose owner and mode are checked is the file whose bytes are read. It must
/// be a regular file owned by `euid` (the observer uid) and mode 0400: no
/// other non-root uid can read it, the Fabric's included, and not even its
/// owner can rewrite it. A POSIX ACL granting another uid read access needs a
/// mask that grants it, and the mask IS the group bits of `st_mode`
/// (POSIX.1e), so it is refused here too. Returns the key pair and its
/// public key (hex).
pub fn load_key(
    path: &Path,
    euid: u32,
) -> Result<(ring::signature::Ed25519KeyPair, String), String> {
    use ring::signature::KeyPair;
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|e| {
            format!(
                "observer key {}: {e} (a symlink is never followed)",
                path.display()
            )
        })?;
    let meta = f
        .metadata()
        .map_err(|e| format!("observer key {}: {e}", path.display()))?;
    if !meta.file_type().is_file() || meta.uid() != euid || meta.mode() & 0o277 != 0 {
        return Err(format!(
            "observer key {} must be a regular file owned by the observer uid {euid}, readable \
             by no other uid and writable by none (mode 0400); it is uid {} mode {:o}: a key \
             another uid can read lets that uid mint observations (amendment 68)",
            path.display(),
            meta.uid(),
            meta.mode() & 0o7777
        ));
    }
    let mut key = Vec::new();
    f.read_to_end(&mut key)
        .map_err(|e| format!("observer key {}: {e}", path.display()))?;
    let kp = ring::signature::Ed25519KeyPair::from_pkcs8(&key).map_err(|_| {
        format!(
            "observer key {} is not a PKCS#8 Ed25519 key",
            path.display()
        )
    })?;
    let public: String = kp
        .public_key()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok((kp, public))
}

/// The key's public half is in the observer root `root` and in no other
/// authority's root (ADR-002, [`crate::backend::exclusive_root_keys`]): an
/// observer signing with a key no verifier trusts is a misconfiguration
/// refused at start, and a key another root also holds is authority for both.
pub fn key_in_root(public: &str, root: &Path, operator: bool) -> Result<(), String> {
    if operator {
        crate::backend::check_operator_owned(root)?;
    }
    let keys = crate::backend::exclusive_root_keys(
        TrustAuthority::Observer,
        root,
        &crate::backend::sibling_roots(TrustAuthority::Observer, root),
        operator.then_some(Path::new("/")),
        None,
    )?;
    if !keys.iter().any(|k| k == public) {
        return Err(format!(
            "the observer key's public half {public} is not in the observer root {}: nothing it \
             signs would verify",
            root.display()
        ));
    }
    Ok(())
}

/// `YYYY-MM-DDTHH:MM:SSZ` for a Unix time (the form
/// [`crate::backend::parse_utc`] reads).
pub fn utc(t: i64) -> String {
    let days = t.div_euclid(86_400);
    let secs = t.rem_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        secs / 3600,
        secs % 3600 / 60,
        secs % 60
    )
}

/// `axon-observer-request/1`: from the root helper's relay only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    /// The launch manifest's canonical text, exactly the bytes whose sha256
    /// the observation names.
    pub manifest: String,
    /// sha256 of the RUNNING Fabric's executable, measured by the root helper
    /// from its parent's pidfd (`privileged_launcher::running_caller`).
    pub caller_sha256: String,
}

/// `axon-observer-reply/1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub schema: String,
    pub ok: bool,
    pub mode: String,
    pub observation: Option<String>,
    pub signature: Option<String>,
    pub error: Option<String>,
}

/// The observer as the helper's config names it: socket, uid, and the
/// PROGRAM pin (required: every reply message's sender is checked against it,
/// [`crate::custodian::check_sender_program`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObserverRef {
    pub socket: PathBuf,
    pub uid: u32,
    pub sha256: String,
}

/// A relayed observation: its exact bytes, its signature, and the mode of the
/// observer that made it.
#[derive(Debug, Clone)]
pub struct Observed {
    pub observation: String,
    pub signature: String,
    pub mode: Mode,
}

impl ObserverRef {
    /// Ask the observer to observe `manifest` for the running Fabric whose
    /// executable has sha256 `caller_sha256`. The listener must be the
    /// observer uid's (or root's: systemd binds an activated socket), and
    /// every byte of the reply must come from ONE process executing the pinned
    /// program.
    pub fn observe(&self, manifest: &str, caller_sha256: &str) -> Result<Observed, String> {
        let s = UnixStream::connect(&self.socket)
            .map_err(|e| format!("observer {}: {e}", self.socket.display()))?;
        let peer = peer_uid(s.as_raw_fd())?;
        if peer != self.uid && peer != 0 {
            return Err(format!(
                "the observer socket {} is served by uid {peer}, not the observer uid {} (or \
                 root's socket activation)",
                self.socket.display(),
                self.uid
            ));
        }
        let _ = s.set_read_timeout(Some(IO_TIMEOUT));
        let _ = s.set_write_timeout(Some(IO_TIMEOUT));
        // Before the request goes out, so the kernel names the sender of every
        // byte of the reply.
        crate::custodian::pass_pidfd(s.as_raw_fd())?;
        let mut body = serde_json::to_vec(&Request {
            schema: REQUEST_SCHEMA.into(),
            manifest: manifest.into(),
            caller_sha256: caller_sha256.into(),
        })
        .map_err(|e| e.to_string())?;
        body.push(b'\n');
        (&s).write_all(&body)
            .map_err(|e| format!("observer: {e}"))?;
        let _ = s.shutdown(std::net::Shutdown::Write);
        let text = crate::custodian::read_from_pinned(
            &s,
            &self.sha256,
            &self.socket,
            "observer",
            MAX_REPLY,
        )?;
        let r: Reply =
            serde_json::from_slice(&text).map_err(|e| format!("observer gave no reply: {e}"))?;
        if r.schema != REPLY_SCHEMA {
            return Err(format!("observer reply is not {REPLY_SCHEMA}"));
        }
        if !r.ok {
            return Err(format!(
                "the observer refused: {}",
                r.error.unwrap_or_default()
            ));
        }
        let mode = Mode::parse(&r.mode).unwrap_or(Mode::Dev);
        let observation = r
            .observation
            .ok_or("the observer's reply carries no observation")?;
        let signature = r
            .signature
            .ok_or("the observer's reply carries no signature")?;
        Ok(Observed {
            observation,
            signature,
            mode,
        })
    }
}

/// Where the observer measures, and how it reads the operator files there.
#[derive(Debug, Clone)]
pub struct Sources {
    pub host_config: PathBuf,
    pub helper_config: PathBuf,
    /// `Some`: protected, every config read through the operator walk.
    pub authority: Option<crate::privileged_launcher::Authority>,
}

impl Sources {
    /// The protected observer's: the operator's fixed paths.
    pub fn operator() -> Sources {
        Sources {
            host_config: PathBuf::from(crate::protected_host::PROTECTED_HOST_CONFIG),
            helper_config: PathBuf::from(crate::privileged_launcher::CONFIG_PATH),
            authority: Some(crate::privileged_launcher::Authority::production()),
        }
    }
    fn config(&self, p: &Path) -> Result<Vec<u8>, String> {
        match &self.authority {
            Some(a) => crate::privileged_launcher::read_operator_file(p, a),
            None => crate::backend::read_regular(p).map_err(|e| format!("{}: {e}", p.display())),
        }
    }
}

/// sha256 of the regular file at `p`, streamed from ONE `O_NOFOLLOW` open (a
/// rootfs is larger than any bound a whole read could take).
fn digest_of(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::OpenOptionsExt;
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(p)
        .map_err(|e| format!("cannot measure {}: {e}", p.display()))?;
    if !f.metadata().map(|m| m.is_file()).unwrap_or(false) {
        return Err(format!(
            "cannot measure {}: not a regular file",
            p.display()
        ));
    }
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| format!("cannot measure {}: {e}", p.display()))?;
    Ok(format!("{:x}", h.finalize()))
}

/// An observer serving requests.
pub struct Server {
    pub cfg: ObserverServiceConfig,
    pub mode: Mode,
    pub key: ring::signature::Ed25519KeyPair,
    /// `ed25519:<16 hex>` of the key.
    pub key_id: String,
    pub public_hex: String,
    pub sources: Sources,
    pub clock: Clock,
}

impl Server {
    fn reply(&self, r: Result<(String, String), String>) -> Reply {
        let (ok, observation, signature, error) = match r {
            Ok((o, s)) => (true, Some(o), Some(s), None),
            Err(e) => (false, None, None, Some(e)),
        };
        Reply {
            schema: REPLY_SCHEMA.into(),
            ok,
            mode: self.mode.as_str().into(),
            observation,
            signature,
            error,
        }
    }

    /// Answer one request from `peer` (the kernel-reported uid).
    pub fn answer(&self, peer: u32, request: &[u8]) -> Reply {
        self.reply(self.decide(peer, request))
    }

    /// Every installed digest the manifest names, measured here from the
    /// operator's files, must be the manifest's: the observer signs only what
    /// it measured. `caller_sha256` is the running Fabric's executable,
    /// measured by the root helper (this observer's only caller).
    fn measure(&self, m: &axon_psv::LaunchManifest, caller_sha256: &str) -> Result<(), String> {
        use serde_json::Value;
        let host = self.sources.config(&self.sources.host_config)?;
        let hv: Value = serde_json::from_slice(&host)
            .map_err(|e| format!("{}: {e}", self.sources.host_config.display()))?;
        let helper = self.sources.config(&self.sources.helper_config)?;
        let lv: Value = serde_json::from_slice(&helper)
            .map_err(|e| format!("{}: {e}", self.sources.helper_config.display()))?;
        let path = |v: &Value, ptr: &str| -> Result<PathBuf, String> {
            v.pointer(ptr)
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| format!("the operator config names no {ptr}"))
        };
        let file = |v: &Value, ptr: &str| path(v, ptr).and_then(|p| digest_of(&p));
        let artifacts = path(&hv, "/artifacts_dir")?;
        let measured: [(&str, String, &str); 9] = [
            (
                "host_config_sha256",
                axon_psv::sha256_hex(&host),
                &m.host_config_sha256,
            ),
            (
                "launcher_sha256",
                file(&hv, "/launcher/path")?,
                &m.launcher_sha256,
            ),
            (
                "firecracker_sha256",
                file(&lv, "/firecracker")?,
                &m.firecracker_sha256,
            ),
            (
                "guest.kernel_sha256",
                digest_of(&artifacts.join("vmlinux"))?,
                &m.guest.kernel_sha256,
            ),
            (
                "guest.rootfs_sha256",
                digest_of(&artifacts.join("rootfs.sqfs"))?,
                &m.guest.rootfs_sha256,
            ),
            (
                "suite.registry_sha256",
                file(&hv, "/suite_registry/path")?,
                &m.suite.registry_sha256,
            ),
            (
                "qualification_sha256",
                file(&hv, "/qualification/record")?,
                &m.qualification_sha256,
            ),
            (
                "profile_manifest_sha256",
                file(&hv, "/profile_manifest/path")?,
                &m.profile_manifest_sha256,
            ),
            (
                "verifier_sha256",
                caller_sha256.to_string(),
                &m.verifier_sha256,
            ),
        ];
        for (field, got, claimed) in measured {
            if got != claimed {
                return Err(format!(
                    "the launch manifest's {field} is {claimed}, but the observer measured {got}: \
                     it signs only what it measured"
                ));
            }
        }
        Ok(())
    }

    fn decide(&self, peer: u32, request: &[u8]) -> Result<(String, String), String> {
        if peer != self.cfg.caller_uid {
            return Err(format!(
                "uid {peer} is not the observer's caller uid {}: an observation is made only for \
                 the root helper's relay (amendment 68)",
                self.cfg.caller_uid
            ));
        }
        let r: Request =
            serde_json::from_slice(request).map_err(|e| format!("malformed request: {e}"))?;
        if r.schema != REQUEST_SCHEMA {
            return Err(format!("request schema is not {REQUEST_SCHEMA}"));
        }
        let bytes = r.manifest.as_bytes();
        let digest = axon_psv::sha256_hex(bytes);
        // Canonical, `/2`, the protected profile, its completion scheme.
        let m = axon_psv::LaunchManifest::verify(bytes, &digest)
            .map_err(|e| format!("not a protected launch manifest: {e}"))?;
        // The nonce names the record below: it is the custodian's form or
        // nothing (a path would write outside the store).
        if !is_hex(&m.observation_nonce, 32) {
            return Err(format!(
                "observation_nonce {:?} is not a nonce the custodian issues",
                m.observation_nonce
            ));
        }
        self.measure(&m, &r.caller_sha256)?;
        // One observation per nonce: the record is created, never replaced,
        // BEFORE anything is signed.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(
                self.cfg
                    .store
                    .join(format!("{}.observed", m.observation_nonce)),
            ) {
            Ok(mut f) => {
                let _ = f.write_all(digest.as_bytes());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(format!(
                    "nonce {} was already observed: one observation per nonce (amendment 68)",
                    m.observation_nonce
                ))
            }
            Err(e) => return Err(format!("observation record: {e}")),
        }
        let o = axon_psv::PreflightObservation {
            schema: axon_psv::PREFLIGHT_OBSERVATION_SCHEMA.into(),
            observer_key_id: self.key_id.clone(),
            nonce: m.observation_nonce.clone(),
            epoch: m.authority.epoch,
            observed_at: utc(self.clock.now_unix()),
            host_profile: m.backend_profile.clone(),
            fabric_revision: m.fabric_revision.clone(),
            firecracker_sha256: m.firecracker_sha256.clone(),
            launcher_sha256: m.launcher_sha256.clone(),
            host_config_sha256: m.host_config_sha256.clone(),
            guest: m.guest.clone(),
            verifier_sha256: m.verifier_sha256.clone(),
            suite_registry_sha256: m.suite.registry_sha256.clone(),
            policy_sha256: m.policy_sha256.clone(),
            intended_launch_manifest_sha256: digest,
        };
        let obytes = serde_json::to_vec(&o).map_err(|e| e.to_string())?;
        let msg = crate::backend::evidence_signing_message(TrustAuthority::Observer, &obytes);
        let sig = serde_json::json!({
            "schema": crate::backend::EVIDENCE_SIGNATURE_SCHEMA, "alg": "ed25519",
            "domain": TrustAuthority::Observer.dir_name(), "public_key": self.public_hex,
            "signature": self.key.sign(&msg).as_ref().iter()
                .map(|b| format!("{b:02x}")).collect::<String>(),
        });
        Ok((
            String::from_utf8(obytes).map_err(|e| e.to_string())?,
            sig.to_string(),
        ))
    }

    /// Serve one connection: authenticate the caller by `SO_PEERCRED`, read
    /// one bounded request line, write one reply.
    pub fn serve_one(&self, s: UnixStream) {
        let _ = s.set_read_timeout(Some(IO_TIMEOUT));
        let _ = s.set_write_timeout(Some(IO_TIMEOUT));
        let reply = match peer_uid(s.as_raw_fd()) {
            Err(e) => self.reply(Err(e)),
            Ok(peer) => {
                let mut line = Vec::new();
                let mut r = (&s).take(MAX_REQUEST);
                let mut byte = [0u8; 1];
                while let Ok(1) = r.read(&mut byte) {
                    if byte[0] == b'\n' {
                        break;
                    }
                    line.push(byte[0]);
                }
                self.answer(peer, &line)
            }
        };
        let mut out = serde_json::to_vec(&reply).unwrap_or_default();
        out.push(b'\n');
        let _ = (&s).write_all(&out);
    }

    /// Serve `listener` forever, one connection at a time.
    pub fn serve(&self, listener: &UnixListener) -> ! {
        loop {
            if let Ok((s, _)) = listener.accept() {
                self.serve_one(s);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> ObserverServiceConfig {
        ObserverServiceConfig {
            schema: CONFIG_SCHEMA.into(),
            observer_uid: 994,
            fabric_uid: 991,
            caller_uid: 0,
            socket: "/run/axon-observer/observer.sock".into(),
            store: "/var/lib/axon-observer/observed".into(),
            key_path: "/var/lib/axon-observer/key/observer.pk8".into(),
            test_paths: None,
        }
    }

    /// A94 (amendment 68): on a protected host the observer is not the
    /// Fabric. An observer config naming the Fabric uid as the observer (so
    /// the Fabric uid could read the key), a root observer, a root Fabric, or
    /// a caller other than the root helper is refused.
    #[test]
    fn an_observer_that_is_the_fabric_is_refused() {
        cfg().check(true).expect("control: three principals");
        let mut c = cfg();
        c.observer_uid = c.fabric_uid;
        let got = c.check(true);
        assert!(
            got.is_err(),
            "ATTACK: a protected observer running as the Fabric uid was accepted: the Fabric \
             uid can read the key and mint observations"
        );
        for edit in [
            (|c: &mut ObserverServiceConfig| c.observer_uid = 0) as fn(&mut ObserverServiceConfig),
            |c| c.fabric_uid = 0,
        ] {
            let mut c = cfg();
            edit(&mut c);
            assert!(c.check(true).is_err(), "a root principal: {c:?}");
        }
    }

    /// Amendment 68 (G1 = A): a protected observer observes only for uid 0,
    /// the root helper's relay. A config admitting the Fabric uid as the
    /// caller would let Fabric ask without the running-Fabric measurement and
    /// without the program check.
    #[test]
    fn a_protected_observer_observes_only_for_the_root_helper() {
        cfg().check(true).expect("control");
        let mut c = cfg();
        c.caller_uid = c.fabric_uid;
        let got = c.check(true);
        assert!(
            got.is_err(),
            "ATTACK: a protected observer config admitting the Fabric uid as its caller was \
             accepted"
        );
    }

    #[test]
    fn utc_reads_back() {
        for t in [0i64, 1_790_000_000, 1_234_567_890] {
            assert_eq!(crate::backend::parse_utc(&utc(t)), Some(t));
        }
    }
}
