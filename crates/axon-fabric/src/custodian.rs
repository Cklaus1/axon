//! The CUSTODIAN (`axon-custodian`; operator decision D6, amendment 50).
//!
//! The observation nonce constrains Fabric: one nonce, one launch. So Fabric
//! must not be the one who issues it, keeps its record or decides it was used.
//! The custodian is a separate component, running as its OWN uid on a
//! protected host, socket-activated by systemd:
//!
//! * it ISSUES a fresh 128-bit nonce, bound to an epoch, to the configured
//!   Fabric uid only;
//! * it keeps every record in its own store: a directory owned by the
//!   custodian uid, mode 0700, that no other uid can read or write;
//! * it SPENDS a nonce only for the configured launcher uid (0: the setuid-root
//!   `axon-protected-launcher`, after it has verified the observation), and a
//!   nonce spends exactly once.
//!
//! Each caller is authenticated by the KERNEL (`SO_PEERCRED` on the accepted
//! connection), never by anything it says. Each client authenticates the
//! custodian the same way: the peer that bound the socket must be the
//! custodian uid, or root (systemd binds a socket-activated listener).
//!
//! Three modes, stated in every reply:
//! * `protected`: socket-activated, operator config at [`CONFIG_PATH`];
//! * `test`: a test-trust build's `--test-config FILE` (tests only);
//! * `dev`: a manual `axon-custodian --dev` launch. A dev custodian is NEVER
//!   protected: the privileged launcher refuses a spend it answers.
//!
//! The in-process [`crate::observer::NonceStore`] Fabric may still use in
//! development ([`Custodian::InProcess`]) is Fabric's own and so never reaches
//! a root launch: the launcher spends only through the custodian its operator
//! config names, which never issued those nonces.

use crate::backend::Clock;
use crate::observer::NonceStore;
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::fd::{AsRawFd, RawFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

/// The custodian's operator config (protected mode).
pub const CONFIG_PATH: &str = "/etc/axon/custodian.json";
pub const CONFIG_SCHEMA: &str = "axon-custodian/1";
pub const REQUEST_SCHEMA: &str = "axon-custodian-request/1";
pub const REPLY_SCHEMA: &str = "axon-custodian-reply/1";

const MAX_MESSAGE: u64 = 4096;
const IO_TIMEOUT: Duration = Duration::from_secs(30);

/// Which custodian answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Socket-activated, under the operator's config.
    Protected,
    /// A test-trust build's `--test-config` custodian (tests only).
    Test,
    /// A manual `--dev` launch: never protected.
    Dev,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Protected => "protected",
            Mode::Test => "test",
            Mode::Dev => "dev",
        }
    }
    pub fn parse(s: &str) -> Option<Mode> {
        match s {
            "protected" => Some(Mode::Protected),
            "test" => Some(Mode::Test),
            "dev" => Some(Mode::Dev),
            _ => None,
        }
    }
}

/// `axon-custodian/1`, the operator's.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustodianConfig {
    pub schema: String,
    /// The uid the custodian runs as (systemd `User=`). Never Fabric's, never 0.
    pub custodian_uid: u32,
    /// The only uid a nonce is issued to.
    pub fabric_uid: u32,
    /// The only uid that may spend a nonce: 0, the setuid-root helper.
    pub launcher_uid: u32,
    /// The socket systemd listens on (`ListenStream=`); the custodian refuses
    /// an activation on any other.
    pub socket: PathBuf,
    /// The custodian's own store: its uid's, 0700.
    pub store: PathBuf,
    /// How long an issued nonce stays spendable.
    pub max_age_s: u64,
}

impl CustodianConfig {
    /// The config's own rules. `protected`: the three roles are three
    /// principals — a nonce issued, stored or spent by the uid it constrains
    /// is no constraint (amendment 50; negative matrix A83).
    pub fn check(&self, protected: bool) -> Result<(), String> {
        if self.schema != CONFIG_SCHEMA {
            return Err(format!("schema is not {CONFIG_SCHEMA}"));
        }
        for p in [&self.socket, &self.store] {
            if !plain_absolute(p) {
                return Err(format!("{} is not an absolute plain path", p.display()));
            }
        }
        if self.max_age_s == 0 {
            return Err("max_age_s must be positive".into());
        }
        if !protected {
            return Ok(());
        }
        if self.custodian_uid == self.fabric_uid {
            return Err(format!(
                "custodian_uid {} is the Fabric uid: the nonce would be issued, stored and spent \
                 by the principal it constrains",
                self.custodian_uid
            ));
        }
        if self.custodian_uid == 0 || self.fabric_uid == 0 {
            return Err("neither the custodian nor the Fabric runs as root".into());
        }
        if self.launcher_uid != 0 {
            return Err(format!(
                "launcher_uid is {}, not 0: a nonce is spent only by the setuid-root \
                 privileged launcher",
                self.launcher_uid
            ));
        }
        Ok(())
    }
}

fn plain_absolute(p: &Path) -> bool {
    p.is_absolute()
        && p.components()
            .all(|c| matches!(c, Component::RootDir | Component::Normal(_)))
}

fn is_hex(s: &str, n: usize) -> bool {
    s.len() == n && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The custodian's store must be ITS OWN: a real directory owned by the uid
/// the custodian runs as, with no group or other access. Another owner, or a
/// group Fabric is in, could plant `<nonce>.issued` records (a nonce the
/// custodian never issued) or erase `.used` ones (a spent nonce spends again).
pub fn check_store(dir: &Path, euid: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let m = std::fs::symlink_metadata(dir)
        .map_err(|e| format!("nonce store {}: {e} (it must exist)", dir.display()))?;
    if m.file_type().is_symlink() || !m.is_dir() {
        return Err(format!(
            "nonce store {} is not a directory (a symlink is never followed)",
            dir.display()
        ));
    }
    if m.uid() != euid {
        return Err(format!(
            "nonce store {} is owned by uid {}, not the custodian's ({euid}): its owner could \
             write nonce records",
            dir.display(),
            m.uid()
        ));
    }
    if m.mode() & 0o077 != 0 {
        return Err(format!(
            "nonce store {} is accessible to group or other (mode {:o}; must be 0700): another \
             uid could plant or erase nonce records",
            dir.display(),
            m.mode() & 0o7777
        ));
    }
    Ok(())
}

/// Read the custodian's operator config at `path` (walked from the authority's
/// base, operator-owned; [`crate::privileged_launcher::read_operator_file`]).
pub fn load_config(
    path: &Path,
    a: &crate::privileged_launcher::Authority,
) -> Result<CustodianConfig, String> {
    let bytes = crate::privileged_launcher::read_operator_file(path, a)?;
    let c: CustodianConfig =
        serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    c.check(!a.test)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(c)
}

/// The uid of the process on the other end of `fd`, as the kernel recorded it
/// (for an accepted connection: the caller at `connect`; for a client: whoever
/// called `listen`).
pub fn peer_uid(fd: RawFd) -> Result<u32, String> {
    // SAFETY: getsockopt into a zeroed ucred of the size passed.
    let mut cred: libc::ucred = unsafe { std::mem::zeroed() };
    let mut len = std::mem::size_of::<libc::ucred>() as libc::socklen_t;
    let r = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PEERCRED,
            &mut cred as *mut libc::ucred as *mut libc::c_void,
            &mut len,
        )
    };
    if r != 0 {
        return Err(format!("SO_PEERCRED: {}", std::io::Error::last_os_error()));
    }
    Ok(cred.uid)
}

/// `axon-custodian-request/1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    /// `issue` or `spend`.
    pub op: String,
    pub epoch: u64,
    #[serde(default)]
    pub nonce: Option<String>,
    /// `spend`: the launch manifest the nonce is spent for (recorded).
    #[serde(default)]
    pub manifest_sha256: Option<String>,
}

/// `axon-custodian-reply/1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Reply {
    pub schema: String,
    pub ok: bool,
    pub mode: String,
    pub nonce: Option<String>,
    pub error: Option<String>,
}

/// A custodian, as a client names it: its socket and its uid (from the host
/// config for Fabric, from the helper's operator config for the helper).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustodianRef {
    pub socket: PathBuf,
    pub uid: u32,
}

impl CustodianRef {
    /// One request, one reply. The custodian is authenticated by the kernel:
    /// the socket's listener was bound by the custodian uid (or by root, for
    /// systemd's socket activation). A listener any other uid bound (Fabric
    /// answering its own requests) is refused before a byte is sent.
    pub fn call(&self, req: &Request) -> Result<Reply, String> {
        let s = UnixStream::connect(&self.socket)
            .map_err(|e| format!("custodian {}: {e}", self.socket.display()))?;
        let peer = peer_uid(s.as_raw_fd())?;
        if peer != self.uid && peer != 0 {
            return Err(format!(
                "the custodian socket {} is served by uid {peer}, not the custodian uid {} (or \
                 root's socket activation)",
                self.socket.display(),
                self.uid
            ));
        }
        let _ = s.set_read_timeout(Some(IO_TIMEOUT));
        let _ = s.set_write_timeout(Some(IO_TIMEOUT));
        let mut body = serde_json::to_vec(req).map_err(|e| e.to_string())?;
        body.push(b'\n');
        (&s).write_all(&body)
            .map_err(|e| format!("custodian: {e}"))?;
        let _ = s.shutdown(std::net::Shutdown::Write);
        let mut text = Vec::new();
        (&s).take(MAX_MESSAGE)
            .read_to_end(&mut text)
            .map_err(|e| format!("custodian: {e}"))?;
        let r: Reply =
            serde_json::from_slice(&text).map_err(|e| format!("custodian gave no reply: {e}"))?;
        if r.schema != REPLY_SCHEMA || Mode::parse(&r.mode).is_none() {
            return Err("custodian reply has another schema".into());
        }
        if !r.ok {
            return Err(format!(
                "custodian refused: {}",
                r.error.unwrap_or_default()
            ));
        }
        Ok(r)
    }

    /// A fresh nonce for `epoch`, and the mode of the custodian that issued it.
    pub fn issue(&self, epoch: u64) -> Result<(String, Mode), String> {
        let r = self.call(&Request {
            schema: REQUEST_SCHEMA.into(),
            op: "issue".into(),
            epoch,
            nonce: None,
            manifest_sha256: None,
        })?;
        let nonce = r
            .nonce
            .filter(|n| is_hex(n, 32))
            .ok_or("custodian issued no well-formed nonce")?;
        Ok((nonce, Mode::parse(&r.mode).unwrap_or(Mode::Dev)))
    }

    /// Spend `nonce` (issued for `epoch`) on the launch of `manifest_sha256`.
    pub fn spend(&self, nonce: &str, epoch: u64, manifest_sha256: &str) -> Result<Mode, String> {
        let r = self.call(&Request {
            schema: REQUEST_SCHEMA.into(),
            op: "spend".into(),
            epoch,
            nonce: Some(nonce.into()),
            manifest_sha256: Some(manifest_sha256.into()),
        })?;
        Ok(Mode::parse(&r.mode).unwrap_or(Mode::Dev))
    }
}

/// Where Fabric gets its nonce.
#[derive(Debug, Clone)]
pub enum Custodian {
    /// The custodian service: Fabric holds only this client connection.
    Service(CustodianRef),
    /// DEVELOPMENT ONLY: Fabric's own in-process store. Never protected: the
    /// privileged launcher spends through its operator-configured custodian,
    /// which never issued these nonces.
    InProcess(NonceStore),
}

impl Custodian {
    /// A fresh nonce for `epoch`.
    pub fn issue(&self, epoch: u64, clock: &Clock) -> Result<String, String> {
        match self {
            Custodian::Service(c) => c.issue(epoch).map(|(n, _)| n),
            Custodian::InProcess(s) => s.issue(epoch, clock),
        }
    }
}

/// A custodian serving requests.
pub struct Server {
    pub cfg: CustodianConfig,
    pub mode: Mode,
    pub store: NonceStore,
    pub clock: Clock,
}

impl Server {
    fn reply(&self, r: Result<Option<String>, String>) -> Reply {
        let (ok, nonce, error) = match r {
            Ok(n) => (true, n, None),
            Err(e) => (false, None, Some(e)),
        };
        Reply {
            schema: REPLY_SCHEMA.into(),
            ok,
            mode: self.mode.as_str().into(),
            nonce,
            error,
        }
    }

    /// Answer one request from `peer` (the kernel-reported uid).
    pub fn answer(&self, peer: u32, request: &[u8]) -> Reply {
        self.reply(self.decide(peer, request))
    }

    fn decide(&self, peer: u32, request: &[u8]) -> Result<Option<String>, String> {
        let r: Request =
            serde_json::from_slice(request).map_err(|e| format!("malformed request: {e}"))?;
        if r.schema != REQUEST_SCHEMA {
            return Err(format!("request schema is not {REQUEST_SCHEMA}"));
        }
        match r.op.as_str() {
            "issue" => {
                if peer != self.cfg.fabric_uid {
                    return Err(format!(
                        "uid {peer} is not the Fabric uid {}: a nonce is issued to Fabric only",
                        self.cfg.fabric_uid
                    ));
                }
                self.store.issue(r.epoch, &self.clock).map(Some)
            }
            "spend" => {
                if peer != self.cfg.launcher_uid {
                    return Err(format!(
                        "uid {peer} is not the launcher uid {}: a nonce is spent only at the \
                         root launch boundary",
                        self.cfg.launcher_uid
                    ));
                }
                let nonce = r.nonce.as_deref().ok_or("spend names no nonce")?;
                let manifest = r
                    .manifest_sha256
                    .as_deref()
                    .filter(|m| is_hex(m, 64))
                    .ok_or("spend names no launch manifest sha256")?;
                self.store
                    .consume(nonce, r.epoch, &self.clock, self.cfg.max_age_s)?;
                // Which launch spent it: an audit record over the `.used` the
                // atomic rename made (it decides nothing, and creates nothing:
                // only a spent nonce has a record to annotate).
                let _ = std::fs::OpenOptions::new()
                    .write(true)
                    .truncate(true)
                    .open(self.store.dir.join(format!("{nonce}.used")))
                    .and_then(|mut f| {
                        f.write_all(
                            serde_json::json!({
                                "epoch": r.epoch,
                                "spent_unix": self.clock.now_unix(),
                                "manifest_sha256": manifest,
                            })
                            .to_string()
                            .as_bytes(),
                        )
                    });
                Ok(None)
            }
            other => Err(format!("unknown op {other:?}")),
        }
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
                let mut r = (&s).take(MAX_MESSAGE);
                let mut byte = [0u8; 1];
                // One line: stop at the newline, never wait for EOF.
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

    /// Serve `listener` forever, one connection at a time (so two spends of
    /// one nonce are also serialized by the process, not only by the rename).
    pub fn serve(&self, listener: &UnixListener) -> ! {
        loop {
            if let Ok((s, _)) = listener.accept() {
                self.serve_one(s);
            }
        }
    }
}

/// The listener systemd passed (socket activation: `LISTEN_PID` is this
/// process, `LISTEN_FDS` is 1, the socket is fd 3), bound at exactly `socket`.
pub fn activated_listener(socket: &Path) -> Result<UnixListener, String> {
    use std::os::fd::FromRawFd;
    let pid = std::env::var("LISTEN_PID").unwrap_or_default();
    let fds = std::env::var("LISTEN_FDS").unwrap_or_default();
    if pid != std::process::id().to_string() || fds != "1" {
        return Err(
            "not socket-activated (LISTEN_PID/LISTEN_FDS): a protected custodian is started by \
             its systemd socket unit only; use --dev for a manual development launch"
                .into(),
        );
    }
    // SAFETY: fstat on fd 3, which systemd passes.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(3, &mut st) } != 0 || st.st_mode & libc::S_IFMT != libc::S_IFSOCK {
        return Err("fd 3 is not the activated socket".into());
    }
    // SAFETY: fd 3 is ours from here on.
    let l = unsafe { UnixListener::from_raw_fd(3) };
    let at = l
        .local_addr()
        .map_err(|e| format!("activated socket: {e}"))?;
    if at.as_pathname() != Some(socket) {
        return Err(format!(
            "the activated socket is {:?}, not the configured {}",
            at.as_pathname(),
            socket.display()
        ));
    }
    Ok(l)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> CustodianConfig {
        CustodianConfig {
            schema: CONFIG_SCHEMA.into(),
            custodian_uid: 993,
            fabric_uid: 991,
            launcher_uid: 0,
            socket: "/run/axon-custodian/custodian.sock".into(),
            store: "/var/lib/axon-custodian/nonces".into(),
            max_age_s: 300,
        }
    }

    /// A83: on a protected host the custodian is not the Fabric. A config
    /// naming the Fabric uid as the custodian is refused; so is a root
    /// custodian and a spender other than the root helper.
    #[test]
    fn a_custodian_that_is_the_fabric_is_refused() {
        cfg().check(true).expect("control: three principals");
        let mut c = cfg();
        c.custodian_uid = c.fabric_uid;
        let got = c.check(true);
        assert!(
            got.is_err(),
            "ATTACK: a custodian running as the Fabric uid was accepted: the nonce is issued, \
             stored and spent by the principal it constrains"
        );
        let mut c = cfg();
        c.launcher_uid = c.fabric_uid;
        assert!(c.check(true).is_err(), "a Fabric spender");
        let mut c = cfg();
        c.custodian_uid = 0;
        assert!(c.check(true).is_err(), "a root custodian");
    }

    /// Amendment 50: Fabric is issued a nonce; only the launcher uid spends it.
    #[test]
    fn only_fabric_is_issued_and_only_the_launcher_spends() {
        let d = tempfile::tempdir().unwrap();
        let s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let issue = |uid| {
            s.answer(
                uid,
                br#"{"schema":"axon-custodian-request/1","op":"issue","epoch":3}"#,
            )
        };
        let r = issue(4242);
        assert!(
            !r.ok,
            "ATTACK: a uid that is not the Fabric was issued a nonce: {r:?}"
        );
        let r = issue(991);
        assert!(r.ok, "control: {r:?}");
        let n = r.nonce.unwrap();
        let spend = |uid| {
            s.answer(
                uid,
                serde_json::json!({"schema": REQUEST_SCHEMA, "op": "spend", "epoch": 3,
                                   "nonce": n, "manifest_sha256": "a".repeat(64)})
                .to_string()
                .as_bytes(),
            )
        };
        let r = spend(991);
        assert!(
            !r.ok,
            "ATTACK: the Fabric spent a nonce itself (a spend outside the root launch boundary): \
             {r:?}"
        );
        let r = spend(0);
        assert!(r.ok, "control: the root helper spends: {r:?}");
        let r = spend(0);
        assert!(!r.ok, "a nonce spends once: {r:?}");
        assert_eq!(r.mode, "test");
    }

    /// The store is the custodian's own and private.
    #[test]
    fn a_nonce_store_others_can_reach_is_refused() {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir().unwrap();
        let st = d.path().join("n");
        std::fs::create_dir(&st).unwrap();
        // SAFETY: geteuid cannot fail.
        let me = unsafe { libc::geteuid() };
        std::fs::set_permissions(&st, std::fs::Permissions::from_mode(0o700)).unwrap();
        check_store(&st, me).expect("control: the custodian's own 0700 store");
        std::fs::set_permissions(&st, std::fs::Permissions::from_mode(0o770)).unwrap();
        let got = check_store(&st, me);
        assert!(
            got.is_err(),
            "ATTACK: a nonce store its group can write (the Fabric's group) was accepted"
        );
        std::fs::set_permissions(&st, std::fs::Permissions::from_mode(0o700)).unwrap();
        let got = check_store(&st, me.wrapping_add(1));
        assert!(
            got.is_err(),
            "ATTACK: a nonce store another uid owns was accepted as the custodian's own"
        );
    }
}
