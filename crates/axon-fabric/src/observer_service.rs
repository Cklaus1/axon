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
//! What it is TOLD and cannot measure (the guest policy and the epoch) is
//! stated in amendment 68 and, after the review that found the observer
//! countersigning Fabric-authored values, amendment 79: the Fabric program's
//! digest and build revision are the operator's PIN in the helper config
//! ([`crate::privileged_launcher::FabricPin`]), the guest's init and axon
//! digests are what the measured profile manifest names, and a nonce is
//! observed only if the custodian issued it for that epoch.

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
/// Amendment 79: the whole of one connection's request must arrive within
/// this, not each read within it (a peer dripping a byte per read held the
/// single-threaded service for as long as it liked).
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);
/// The slowest SHA-256 rate the read timeout allows for, in bytes per second.
/// Far below any measured rate (1 GB/s with SHA extensions, 0.2 GB/s without),
/// so a timeout derived from it is not the property under test.
const MIN_HASH_RATE: u64 = 25 << 20;
/// A profile manifest is a few KiB of JSON; the bound is generous and fixed.
const MAX_PROFILE_MANIFEST: u64 = 1 << 20;

/// Amendment 79: how long the helper waits for the observer's reply, given
/// the bytes the observer streams through SHA-256 for it (the guest kernel and
/// rootfs). It was a fixed 30 s: a large image on a cold cache could take
/// longer, the client gave up, and the observer, which creates the nonce's
/// record before it signs, had burned the nonce.
pub fn observe_timeout(artifact_bytes: u64) -> Duration {
    IO_TIMEOUT + Duration::from_secs(artifact_bytes / MIN_HASH_RATE)
}

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
    pub fn observe(
        &self,
        manifest: &str,
        caller_sha256: &str,
        timeout: Duration,
    ) -> Result<Observed, String> {
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
        let _ = s.set_read_timeout(Some(timeout));
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

/// The regular file at `p`, opened through ONE `O_NOFOLLOW` open: what the
/// observer measures is the file it opened, never one a path was swapped to.
fn open_measured(p: &Path) -> Result<std::fs::File, String> {
    use std::os::unix::fs::OpenOptionsExt;
    let f = std::fs::OpenOptions::new()
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
    Ok(f)
}

/// sha256 of the regular file at `p`, streamed (a rootfs is larger than any
/// bound a whole read could take).
fn digest_of(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = open_measured(p)?;
    let mut h = Sha256::new();
    std::io::copy(&mut f, &mut h).map_err(|e| format!("cannot measure {}: {e}", p.display()))?;
    Ok(format!("{:x}", h.finalize()))
}

/// The regular file at `p` (at most `max` bytes): its bytes and their sha256.
fn read_measured(p: &Path, max: u64) -> Result<(Vec<u8>, String), String> {
    let f = open_measured(p)?;
    let mut bytes = Vec::new();
    f.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("cannot measure {}: {e}", p.display()))?;
    if bytes.len() as u64 > max {
        return Err(format!("cannot measure {}: over {max} bytes", p.display()));
    }
    let digest = axon_psv::sha256_hex(&bytes);
    Ok((bytes, digest))
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
    /// How long one connection has to deliver its whole request
    /// ([`REQUEST_DEADLINE`]).
    pub request_deadline: Duration,
}

/// The operator's two configs as the observer reads them for one request.
struct Operator {
    host: Vec<u8>,
    hv: serde_json::Value,
    lv: serde_json::Value,
}

/// Amendment 79: whether a nonce from a custodian in mode `custodian` may be
/// observed by an observer in mode `observer`. A protected observer takes a
/// protected custodian's nonce only (a test or dev custodian is a process its
/// operator did not install); a non-protected observer takes any but a dev
/// custodian's.
fn custodian_admissible(observer: Mode, custodian: Mode) -> bool {
    crate::privileged_launcher::custodian_mode_launches(custodian, observer != Mode::Protected)
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

    fn operator(&self) -> Result<Operator, String> {
        use serde_json::Value;
        let host = self.sources.config(&self.sources.host_config)?;
        let hv: Value = serde_json::from_slice(&host)
            .map_err(|e| format!("{}: {e}", self.sources.host_config.display()))?;
        let helper = self.sources.config(&self.sources.helper_config)?;
        let lv: Value = serde_json::from_slice(&helper)
            .map_err(|e| format!("{}: {e}", self.sources.helper_config.display()))?;
        Ok(Operator { host, hv, lv })
    }

    /// Every digest or identity the manifest names that the observer can learn
    /// from an operator-held source, learned there, must be the manifest's:
    /// the observer signs only what it measured or the operator pinned.
    /// `caller_sha256` is the running Fabric's executable, measured by the
    /// root helper (this observer's only caller); the pin in the helper config
    /// is the Fabric program, and its build revision, by the operator's word
    /// (amendment 79). What stays the principal's word is stated in
    /// amendment 79: the guest policy digest and the authority epoch.
    fn measure(
        &self,
        m: &axon_psv::LaunchManifest,
        op: &Operator,
        caller_sha256: &str,
    ) -> Result<(), String> {
        use serde_json::Value;
        let (hv, lv) = (&op.hv, &op.lv);
        let path = |v: &Value, ptr: &str| -> Result<PathBuf, String> {
            v.pointer(ptr)
                .and_then(Value::as_str)
                .map(PathBuf::from)
                .ok_or_else(|| format!("the operator config names no {ptr}"))
        };
        let text = |v: &Value, ptr: &str| -> Result<String, String> {
            v.pointer(ptr)
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| format!("the operator config names no {ptr}"))
        };
        let file = |v: &Value, ptr: &str| path(v, ptr).and_then(|p| digest_of(&p));
        let artifacts = path(hv, "/artifacts_dir")?;
        // The profile manifest, read ONCE: its digest is the measurement, and
        // the digests it NAMES for the guest's init and axon are the ones the
        // manifest's claims are held to (they live inside the measured rootfs).
        let (pm_bytes, pm_digest) =
            read_measured(&path(hv, "/profile_manifest/path")?, MAX_PROFILE_MANIFEST)?;
        let pm: Value = serde_json::from_slice(&pm_bytes)
            .map_err(|e| format!("the profile manifest is not JSON: {e}"))?;
        let named = |name: &str| -> Result<String, String> {
            pm.pointer(&format!("/artifacts/{name}/sha256"))
                .and_then(Value::as_str)
                .map(String::from)
                .ok_or_else(|| format!("the profile manifest pins no {name}"))
        };
        // The running Fabric must be the program the operator pinned.
        let pinned = text(lv, "/fabric/sha256")?;
        if caller_sha256 != pinned {
            return Err(format!(
                "the running Fabric has sha256 {caller_sha256}, not the operator's pinned \
                 program {pinned}: an observation names the installed Fabric only"
            ));
        }
        let measured: [(&str, String, &str); 12] = [
            (
                "host_config_sha256",
                axon_psv::sha256_hex(&op.host),
                &m.host_config_sha256,
            ),
            (
                "launcher_sha256",
                file(hv, "/launcher/path")?,
                &m.launcher_sha256,
            ),
            (
                "firecracker_sha256",
                file(lv, "/firecracker")?,
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
                file(hv, "/suite_registry/path")?,
                &m.suite.registry_sha256,
            ),
            (
                "qualification_sha256",
                file(hv, "/qualification/record")?,
                &m.qualification_sha256,
            ),
            (
                "profile_manifest_sha256",
                pm_digest,
                &m.profile_manifest_sha256,
            ),
            ("verifier_sha256", pinned.clone(), &m.verifier_sha256),
            (
                "fabric_revision",
                text(lv, "/fabric/revision")?,
                &m.fabric_revision,
            ),
            ("guest.axon_sha256", named("axon")?, &m.guest.axon_sha256),
            (
                "guest.init_sha256",
                named("axon-guest-init")?,
                &m.guest.init_sha256,
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

    /// Amendment 79: drop the records of nonces the custodian no longer
    /// honours (each carries the time it expires), so the store a Fabric-driven
    /// request stream writes to is bounded by what the custodian holds
    /// outstanding. A record that does not parse (a crash between its creation
    /// and its write) is dropped a day after its mtime.
    fn prune(&self) {
        let now = self.clock.now_unix();
        let Ok(dir) = std::fs::read_dir(&self.cfg.store) else {
            return;
        };
        for e in dir.flatten() {
            if !e.file_name().to_string_lossy().ends_with(".observed") {
                continue;
            }
            let expires = std::fs::read(e.path())
                .ok()
                .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
                .and_then(|r| r["expires_unix"].as_i64())
                .or_else(|| {
                    e.metadata()
                        .and_then(|m| m.modified())
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_secs() as i64 + 86_400)
                });
            if expires.is_none_or(|t| t < now) {
                let _ = std::fs::remove_file(e.path());
            }
        }
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
        let op = self.operator()?;
        // Amendment 79: only a nonce the custodian issued for this epoch, and
        // BEFORE anything is hashed or recorded: the observer never asks the
        // Fabric (which writes the manifest) whether its nonce is real, and a
        // request stream of invented nonces costs a custodian round trip, not a
        // rootfs hash and a record each.
        let custodian: crate::custodian::CustodianRef = serde_json::from_value(
            op.lv
                .get("custodian")
                .cloned()
                .ok_or("the helper config names no custodian")?,
        )
        .map_err(|e| format!("the helper config's custodian: {e}"))?;
        // The custodian is authenticated by its UID here (the kernel's
        // SO_PEERCRED of whoever bound the socket: the custodian's, or root's
        // activation), not by its program pin: this service runs as its own
        // uid and cannot open another uid's /proc/<pid>/exe, which is how a
        // pinned program is measured. The PROGRAM is authenticated where it
        // matters, by the root helper at the spend (amendment 65). A custodian
        // impostor of the custodian uid could say yes to a nonce nobody issued,
        // and then fail every spend: an observation nothing launches on.
        let custodian = crate::custodian::CustodianRef {
            sha256: None,
            ..custodian
        };
        let (cmode, expires) = custodian
            .check(&m.observation_nonce, m.authority.epoch)
            .map_err(|e| format!("the custodian does not honour this nonce: {e}"))?;
        if !custodian_admissible(self.mode, cmode) {
            return Err(format!(
                "the nonce comes from a {} custodian, which a {} observer does not take",
                cmode.as_str(),
                self.mode.as_str()
            ));
        }
        self.measure(&m, &op, &r.caller_sha256)?;
        self.prune();
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
                let _ = f.write_all(
                    serde_json::json!({"manifest_sha256": digest, "expires_unix": expires})
                        .to_string()
                        .as_bytes(),
                );
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
    /// one bounded request line, write one reply. The request must be COMPLETE
    /// within [`Server::request_deadline`] of the accept (amendment 79): each
    /// read waits only for what is left of it, so a slow drip is cut off.
    pub fn serve_one(&self, s: UnixStream) {
        let _ = s.set_write_timeout(Some(IO_TIMEOUT));
        let reply = match peer_uid(s.as_raw_fd()) {
            Err(e) => self.reply(Err(e)),
            Ok(peer) => {
                let mut line = Vec::new();
                let mut r = (&s).take(MAX_REQUEST);
                let mut byte = [0u8; 1];
                let started = std::time::Instant::now();
                loop {
                    let left = self.request_deadline.saturating_sub(started.elapsed());
                    if left.is_zero() {
                        break;
                    }
                    let _ = s.set_read_timeout(Some(left));
                    match r.read(&mut byte) {
                        Ok(1) if byte[0] == b'\n' => break,
                        Ok(1) => line.push(byte[0]),
                        _ => break,
                    }
                }
                // An incomplete line is a malformed request.
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

    /// A133 (M1862, M1863): a protected observer takes a nonce only from a PROTECTED
    /// custodian (a test or dev custodian is not a program its operator
    /// installed); a test observer takes a test or protected one; nobody takes
    /// a dev custodian's.
    #[test]
    fn a_protected_observer_takes_only_a_protected_custodians_nonce() {
        assert!(
            custodian_admissible(Mode::Protected, Mode::Protected),
            "control"
        );
        assert!(custodian_admissible(Mode::Test, Mode::Test), "control");
        assert!(custodian_admissible(Mode::Test, Mode::Protected), "control");
        assert!(
            !custodian_admissible(Mode::Protected, Mode::Test),
            "ATTACK: a protected observer observed a nonce a test custodian issued"
        );
        for o in [Mode::Protected, Mode::Test] {
            assert!(
                !custodian_admissible(o, Mode::Dev),
                "ATTACK: a {} observer observed a nonce a dev custodian issued",
                o.as_str()
            );
        }
    }

    /// A134 (M1867): one connection has an ABSOLUTE deadline for its request.
    /// A peer that sends a byte and then nothing (or a byte per read, forever)
    /// held the single-threaded service for as long as it liked; each read's
    /// timeout was all there was. Here the deadline is 500 ms and the peer
    /// goes silent for 3 s: the connection is answered (refused as malformed)
    /// within 2 s. Control: a request that arrives in time is read to its end.
    #[test]
    fn a_connection_that_does_not_finish_its_request_is_cut_off() {
        use ring::signature::KeyPair;
        let pkcs8 =
            ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
                .unwrap();
        let key = ring::signature::Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap();
        let public_hex = key
            .public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let server = Server {
            cfg: cfg(),
            mode: Mode::Test,
            key,
            key_id: "k".into(),
            public_hex,
            sources: Sources {
                host_config: "/nonexistent/host.json".into(),
                helper_config: "/nonexistent/helper.json".into(),
                authority: None,
            },
            clock: Clock::System,
            request_deadline: Duration::from_millis(500),
        };
        let run = |send: &'static [u8], then_silent: Duration| -> (Duration, String) {
            let (a, b) = UnixStream::pair().unwrap();
            let peer = std::thread::spawn(move || {
                let mut a = a;
                let _ = a.write_all(send);
                std::thread::sleep(then_silent);
                let _ = a.shutdown(std::net::Shutdown::Write);
                let mut reply = String::new();
                let _ = a.read_to_string(&mut reply);
                reply
            });
            let started = std::time::Instant::now();
            server.serve_one(b);
            let took = started.elapsed();
            (took, peer.join().unwrap())
        };
        let (took, reply) = run(b"{\"sche", Duration::from_secs(3));
        assert!(
            took < Duration::from_secs(2),
            "ATTACK: a connection that stopped sending held the observer for {took:?} (deadline \
             500 ms): {reply}"
        );
        let (took, reply) = run(b"{}\n", Duration::from_millis(0));
        assert!(
            took < Duration::from_secs(2) && reply.contains("axon-observer-reply/1"),
            "control: a complete request is answered at once: {took:?} {reply}"
        );
    }

    #[test]
    fn utc_reads_back() {
        for t in [0i64, 1_790_000_000, 1_234_567_890] {
            assert_eq!(crate::backend::parse_utc(&utc(t)), Some(t));
        }
    }

    // ── C9 round 4c, EQGATE (amendment 81; M1914-M1915): the observer reads
    // its signing key and measures the installed kernel/rootfs through ONE
    // `O_NOFOLLOW` open. Removed alone, the whole suite stayed green.

    fn euid_() -> u32 {
        // SAFETY: geteuid cannot fail.
        unsafe { libc::geteuid() }
    }

    /// The observer's key is never read through a symlink: a link at the key
    /// path (to a key the observer owns) is refused. Control: the real path loads.
    #[test]
    fn the_observer_key_is_never_read_through_a_symlink() {
        use std::os::unix::fs::PermissionsExt;
        let t = tempfile::tempdir().unwrap();
        let pk8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
            .unwrap();
        let real = t.path().join("real.pk8");
        std::fs::write(&real, pk8.as_ref()).unwrap();
        std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o400)).unwrap();
        load_key(&real, euid_()).expect("control: the real key loads");
        let link = t.path().join("link.pk8");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let got = load_key(&link, euid_()).map(|(_, pk)| pk);
        assert!(
            got.is_err(),
            "ATTACK: the observer read its signing key through a symlink: {got:?}"
        );
    }

    /// The installed kernel/rootfs is measured without following a symlink:
    /// a link (to a regular file) is not "the file", it is refused.
    #[test]
    fn an_installed_artifact_that_is_a_symlink_is_never_measured() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("rootfs.sqfs");
        std::fs::write(&real, b"image").unwrap();
        digest_of(&real).expect("control: the real file is measured");
        let link = t.path().join("link.sqfs");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let got = digest_of(&link);
        assert!(
            got.is_err(),
            "ATTACK: the observer measured an installed artifact through a symlink: {got:?}"
        );
    }
}
