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
/// Amendment 85: the whole of one connection's request must arrive within
/// this (the observer's rule, amendment 79).
pub const REQUEST_DEADLINE: Duration = Duration::from_secs(30);

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
    /// Amendment 79: the only uid that may ASK whether a nonce was issued
    /// (`check`): the observer service's, which refuses to observe a nonce this
    /// custodian never issued for that epoch. Required on a protected host;
    /// never the Fabric's, root or the custodian's own.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observer_uid: Option<u32>,
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
        // Amendment 79: the observer asks whether a nonce was issued, as its own
        // uid. Another uid's `check` would let that uid learn which nonces are
        // outstanding; the Fabric's would make the answer Fabric's.
        match self.observer_uid {
            Some(o) if o != 0 && o != self.fabric_uid && o != self.custodian_uid => {}
            _ => {
                return Err(format!(
                    "observer_uid {:?} must name the observer service's own uid: neither the \
                     Fabric's ({}), root, nor the custodian's ({}) (amendment 79)",
                    self.observer_uid, self.fabric_uid, self.custodian_uid
                ))
            }
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
    /// `issue`, `spend` or `check` (amendment 79).
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
    /// `check`: the unix time after which the nonce is no longer spendable
    /// (issue time + max age). Whatever a record keyed to the nonce is, it may
    /// be dropped after this: the custodian refuses the nonce from then on.
    #[serde(default)]
    pub expires_unix: Option<i64>,
}

/// A custodian, as a client names it: its socket and its uid (from the host
/// config for Fabric, from the helper's operator config for the helper).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct CustodianRef {
    pub socket: PathBuf,
    pub uid: u32,
    /// The custodian PROGRAM's sha256, the operator's pin (the helper's
    /// config; required there on a protected host). With a pin, every byte
    /// of a reply must come from a process executing exactly that program:
    /// the kernel names the sender of each message (`SCM_PIDFD`), and its
    /// executable is opened and hashed by descriptor
    /// ([`check_sender_program`]). Without it the custodian is only a uid.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
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
        if self.sha256.is_some() {
            // Before the request goes out, so the kernel names the sender of
            // every byte of the reply.
            pass_pidfd(s.as_raw_fd())?;
        }
        let mut body = serde_json::to_vec(req).map_err(|e| e.to_string())?;
        body.push(b'\n');
        (&s).write_all(&body)
            .map_err(|e| format!("custodian: {e}"))?;
        let _ = s.shutdown(std::net::Shutdown::Write);
        let text = match &self.sha256 {
            Some(pin) => read_from_pinned(&s, pin, &self.socket, "custodian", MAX_MESSAGE)?,
            None => {
                let mut text = Vec::new();
                (&s).take(MAX_MESSAGE)
                    .read_to_end(&mut text)
                    .map_err(|e| format!("custodian: {e}"))?;
                text
            }
        };
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

    /// Amendment 79: ask whether `nonce` is outstanding for `epoch` (issued by
    /// this custodian for that epoch, unspent, unexpired). Only the observer's
    /// uid is answered. Returns the custodian's mode and the unix time at which
    /// the nonce expires.
    pub fn check(&self, nonce: &str, epoch: u64) -> Result<(Mode, i64), String> {
        let r = self.call(&Request {
            schema: REQUEST_SCHEMA.into(),
            op: "check".into(),
            epoch,
            nonce: Some(nonce.into()),
            manifest_sha256: None,
        })?;
        let expires = r
            .expires_unix
            .ok_or("custodian's check names no expiry for the nonce")?;
        Ok((Mode::parse(&r.mode).unwrap_or(Mode::Dev), expires))
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

/// `SCM_PIDFD` (linux/socket.h): a pidfd of the process that sent a message.
const SCM_PIDFD: libc::c_int = 4;

/// Ask the kernel to attach, to every message received on `fd`, a pidfd of
/// the process that sent it (`SO_PASSPIDFD`, Linux 6.5). A kernel without it
/// cannot authenticate a pinned custodian, and the call is refused.
pub(crate) fn pass_pidfd(fd: RawFd) -> Result<(), String> {
    let one: libc::c_int = 1;
    // SAFETY: setsockopt with a c_int of the size passed.
    let r = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_PASSPIDFD,
            &one as *const libc::c_int as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if r != 0 {
        return Err(format!(
            "SO_PASSPIDFD: {} (a pinned custodian or observer is authenticated by the kernel \
             naming the sender of its reply; this kernel cannot)",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Read a reply (at most `max` bytes) whose every message the kernel
/// attributes, by a pidfd, to one process executing the program pinned by
/// `pin`. `what` names the service (`custodian`, `observer`) in refusals. ONE
/// implementation for every pinned service the helper calls (amendments 65,
/// 68).
pub(crate) fn read_from_pinned(
    s: &UnixStream,
    pin: &str,
    socket: &Path,
    what: &str,
    max: u64,
) -> Result<Vec<u8>, String> {
    use std::os::fd::{FromRawFd, OwnedFd};
    let fd = s.as_raw_fd();
    let mut text = Vec::new();
    let mut sender: Option<i64> = None;
    loop {
        let mut buf = [0u8; 1024];
        // Room for one SCM_PIDFD (and nothing truncates it silently: MSG_CTRUNC).
        let mut ctl = [0u64; 8];
        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        // SAFETY: a zeroed msghdr pointing at the buffers above.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = ctl.as_mut_ptr() as *mut libc::c_void;
        msg.msg_controllen = std::mem::size_of_val(&ctl) as _;
        // SAFETY: recvmsg into the buffers described by msg.
        let n = unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_CMSG_CLOEXEC) };
        if n < 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            return Err(format!("{what}: {e}"));
        }
        // Every received descriptor is owned (closed on drop), whatever happens.
        let mut pidfd: Option<OwnedFd> = None;
        let mut other = false;
        // SAFETY: walking the control buffer the kernel filled, with its macros.
        unsafe {
            let mut c = libc::CMSG_FIRSTHDR(&msg);
            while !c.is_null() {
                if (*c).cmsg_level == libc::SOL_SOCKET
                    && ((*c).cmsg_type == SCM_PIDFD || (*c).cmsg_type == libc::SCM_RIGHTS)
                {
                    let data = libc::CMSG_DATA(c) as *const libc::c_int;
                    let count = ((*c).cmsg_len as usize - libc::CMSG_LEN(0) as usize)
                        / std::mem::size_of::<libc::c_int>();
                    for i in 0..count {
                        let f = OwnedFd::from_raw_fd(std::ptr::read_unaligned(data.add(i)));
                        if (*c).cmsg_type == SCM_PIDFD && pidfd.is_none() {
                            pidfd = Some(f);
                        } else {
                            other = true;
                        }
                    }
                }
                c = libc::CMSG_NXTHDR(&msg, c);
            }
        }
        if n == 0 {
            break;
        }
        if msg.msg_flags & libc::MSG_CTRUNC != 0 || other {
            return Err(format!(
                "the {what}'s reply carried unexpected control data"
            ));
        }
        let pidfd = pidfd.ok_or_else(|| {
            format!(
                "the kernel named no sender for the reply on {} (SCM_PIDFD)",
                socket.display()
            )
        })?;
        let pid = check_sender_program(&pidfd, pin, sender).map_err(|why| {
            format!(
                "the {what} socket {} is not served by the pinned {what} program: {why}",
                socket.display()
            )
        })?;
        sender = Some(pid);
        text.extend_from_slice(&buf[..n as usize]);
        if text.len() as u64 > max {
            return Err(format!("{what} reply is over its bound"));
        }
    }
    Ok(text)
}

/// The pid a pidfd refers to, from the kernel's own record of it (`Pid:` in
/// its fdinfo); `None` once that process has exited.
pub(crate) fn pidfd_pid(pidfd: &std::os::fd::OwnedFd) -> Option<i64> {
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", pidfd.as_raw_fd())).ok()?;
    let pid: i64 = info
        .lines()
        .find_map(|l| l.strip_prefix("Pid:"))?
        .trim()
        .parse()
        .ok()?;
    (pid > 0).then_some(pid)
}

/// The process `pidfd` names executes the program whose sha256 is `pin`
/// (and, after its first message, it is the same process as before: `seen`).
///
/// Race-free: the pidfd pins the process, not a pid number. Its executable is
/// opened through /proc/<pid>/exe and the pidfd is asked again AFTER the open;
/// a process alive then was alive at the open, so the pid named it and no
/// recycled one. The bytes hashed are the open descriptor's: an executable
/// another uid could rewrite is refused first.
pub fn check_sender_program(
    pidfd: &std::os::fd::OwnedFd,
    pin: &str,
    seen: Option<i64>,
) -> Result<i64, String> {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let pid = pidfd_pid(pidfd).ok_or("its sender has exited")?;
    if seen.is_some_and(|s| s != pid) {
        return Err(format!(
            "its reply came from two processes ({seen:?}, {pid})"
        ));
    }
    let mut exe = std::fs::File::open(format!("/proc/{pid}/exe"))
        .map_err(|e| format!("the sender's executable (pid {pid}): {e}"))?;
    if pidfd_pid(pidfd) != Some(pid) {
        return Err(format!(
            "its sender (pid {pid}) exited while it was identified"
        ));
    }
    let m = exe.metadata().map_err(|e| e.to_string())?;
    // SAFETY: geteuid cannot fail.
    let me = unsafe { libc::geteuid() };
    if !m.is_file() || (m.uid() != 0 && m.uid() != me) || m.mode() & 0o022 != 0 {
        return Err(format!(
            "the sender's executable (pid {pid}) is owned by uid {} with mode {:o}: only a \
             program root (or this uid) owns and no other uid can write is hashed",
            m.uid(),
            m.mode() & 0o7777
        ));
    }
    let mut h = Sha256::new();
    std::io::copy(&mut exe, &mut h).map_err(|e| e.to_string())?;
    let got = h
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    if got != pin {
        return Err(format!(
            "the sender (pid {pid}) executes a program with sha256 {got}, not the operator's \
             pin {pin}"
        ));
    }
    Ok(pid)
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

/// What one decided request answers with.
enum Decided {
    Nonce(String),
    Expires(i64),
    Done,
}

/// Amendment 79: the most nonces the custodian holds OUTSTANDING (issued, unspent,
/// unexpired). Fabric can ask for a nonce as often as it likes; each is a file
/// in the custodian's store, so without a bound Fabric fills the disk and every
/// later issue (and so every launch) fails. A launch holds one nonce for the
/// seconds between its issue and its spend: this is far above any honest
/// number in flight.
pub const MAX_OUTSTANDING: usize = 1024;

/// A custodian serving requests.
pub struct Server {
    pub cfg: CustodianConfig,
    pub mode: Mode,
    pub store: NonceStore,
    pub clock: Clock,
}

impl Server {
    fn reply(&self, r: Result<Decided, String>) -> Reply {
        let (ok, nonce, error, expires_unix) = match r {
            Ok(Decided::Nonce(n)) => (true, Some(n), None, None),
            Ok(Decided::Done) => (true, None, None, None),
            Ok(Decided::Expires(t)) => (true, None, None, Some(t)),
            Err(e) => (false, None, Some(e), None),
        };
        Reply {
            schema: REPLY_SCHEMA.into(),
            ok,
            mode: self.mode.as_str().into(),
            nonce,
            error,
            expires_unix,
        }
    }

    /// Answer one request from `peer` (the kernel-reported uid).
    pub fn answer(&self, peer: u32, request: &[u8]) -> Reply {
        self.reply(self.decide(peer, request))
    }

    fn decide(&self, peer: u32, request: &[u8]) -> Result<Decided, String> {
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
                self.store
                    .issue_bounded(r.epoch, &self.clock, self.cfg.max_age_s, MAX_OUTSTANDING)
                    .map(Decided::Nonce)
            }
            "check" => {
                if self.cfg.observer_uid != Some(peer) {
                    return Err(format!(
                        "uid {peer} is not the observer's uid {:?}: only the observer asks \
                         whether a nonce was issued",
                        self.cfg.observer_uid
                    ));
                }
                let nonce = r.nonce.as_deref().ok_or("check names no nonce")?;
                self.store
                    .check(nonce, r.epoch, &self.clock, self.cfg.max_age_s)
                    .map(Decided::Expires)
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
                Ok(Decided::Done)
            }
            other => Err(format!("unknown op {other:?}")),
        }
    }

    /// Serve one connection: authenticate the caller by `SO_PEERCRED`, read
    /// one bounded request line, write one reply. The request must be complete
    /// within [`REQUEST_DEADLINE`] of the accept (amendment 85).
    pub fn serve_one(&self, s: UnixStream) {
        self.serve_one_within(s, REQUEST_DEADLINE)
    }

    /// [`Self::serve_one`] with the request's absolute `deadline`: each read
    /// waits only for what is left of it. A per-read timeout let a Fabric-uid
    /// peer dripping a byte every 20 s hold this single-threaded service for
    /// 131 s (bounded only by 4096 x 29 s), stalling every issue, check and spend.
    pub fn serve_one_within(&self, s: UnixStream, deadline: Duration) {
        let _ = s.set_write_timeout(Some(IO_TIMEOUT));
        let reply = match peer_uid(s.as_raw_fd()) {
            Err(e) => self.reply(Err(e)),
            Ok(peer) => {
                let mut line = Vec::new();
                let mut r = (&s).take(MAX_MESSAGE);
                let mut byte = [0u8; 1];
                let started = std::time::Instant::now();
                // One line: stop at the newline, never wait for EOF.
                loop {
                    let left = deadline.saturating_sub(started.elapsed());
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
            observer_uid: Some(994),
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

    /// A83 (M640): on a protected host only uid 0, the setuid-root helper,
    /// spends a nonce. The custodian's peer rule (M628) admits exactly the
    /// configured launcher uid, so a config naming any other uid would let a
    /// helper that is NOT root in every id (installed without setuid, M602)
    /// spend, and launch as its caller. A uid that is neither the Fabric's
    /// nor the custodian's, so no other rule refuses it.
    #[test]
    fn a_protected_custodian_config_lets_only_root_spend() {
        cfg().check(true).expect("control: launcher_uid 0");
        let mut c = cfg();
        c.launcher_uid = 4242;
        let got = c.check(true);
        assert!(
            got.is_err(),
            "ATTACK: a protected custodian config letting uid 4242 spend nonces was accepted"
        );
        assert!(got.unwrap_err().contains("not 0"));
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

    /// A133 (M1862, amendment 79): only the observer's uid asks whether a nonce
    /// was issued (and learns its expiry); the Fabric uid, root and a stranger
    /// are refused. Control: the observer's uid is answered, the expiry is the
    /// issue time plus the max age, and an invented nonce is refused.
    #[test]
    fn only_the_observer_checks_a_nonce() {
        let d = tempfile::tempdir().unwrap();
        let s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let n = s
            .answer(
                991,
                br#"{"schema":"axon-custodian-request/1","op":"issue","epoch":3}"#,
            )
            .nonce
            .unwrap();
        let check = |uid, nonce: &str, epoch: u64| {
            s.answer(
                uid,
                serde_json::json!({"schema": REQUEST_SCHEMA, "op": "check", "epoch": epoch,
                                   "nonce": nonce})
                .to_string()
                .as_bytes(),
            )
        };
        for uid in [991, 0, 4242] {
            let r = check(uid, &n, 3);
            assert!(
                !r.ok && r.expires_unix.is_none(),
                "ATTACK: uid {uid}, not the observer's, was told whether a nonce is outstanding: \
                 {r:?}"
            );
        }
        let r = check(994, &n, 3);
        assert!(r.ok, "control: the observer's uid is answered: {r:?}");
        assert_eq!(r.expires_unix, Some(1_000_300), "issue time + max age");
        assert!(!check(994, &"ab".repeat(16), 3).ok, "an invented nonce");
        assert!(!check(994, &n, 4).ok, "another epoch");
    }

    /// A133 (M1863): a protected custodian names the observer service's own
    /// uid for `check`: absent, root, the Fabric's or the custodian's is
    /// refused. Control: a fourth uid.
    #[test]
    fn a_protected_custodian_names_the_observer_as_its_own_uid() {
        cfg().check(true).expect("control: four principals");
        for (what, uid) in [
            ("none", None),
            ("root", Some(0)),
            ("the Fabric", Some(991)),
            ("the custodian", Some(993)),
        ] {
            let mut c = cfg();
            c.observer_uid = uid;
            assert!(
                c.check(true).is_err(),
                "ATTACK: a protected custodian config whose observer is {what} was accepted"
            );
        }
    }

    /// A134 (M1865): the custodian's store holds a record only while its nonce
    /// can be spent. Issued, then the clock past the max age, then another
    /// issue: the expired records are gone (Fabric can ask for nonces as often
    /// as it likes, so what it asks for must not accumulate). Control: the
    /// records still spendable stay.
    #[test]
    fn expired_nonce_records_do_not_accumulate() {
        let d = tempfile::tempdir().unwrap();
        let mut s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let issue = |s: &Server| {
            s.answer(
                991,
                br#"{"schema":"axon-custodian-request/1","op":"issue","epoch":3}"#,
            )
        };
        let spent = issue(&s).nonce.unwrap();
        for _ in 0..4 {
            assert!(issue(&s).ok);
        }
        let r = s.answer(
            0,
            serde_json::json!({"schema": REQUEST_SCHEMA, "op": "spend", "epoch": 3,
                               "nonce": spent, "manifest_sha256": "a".repeat(64)})
            .to_string()
            .as_bytes(),
        );
        assert!(r.ok, "setup: one nonce spent: {r:?}");
        let count = |s: &Server| std::fs::read_dir(&s.store.dir).unwrap().count();
        assert_eq!(count(&s), 5, "setup: four issued, one spent");
        s.clock = Clock::FixedUnix(1_000_000 + 100);
        assert!(issue(&s).ok);
        assert_eq!(
            count(&s),
            6,
            "control: nothing is dropped while it can be spent"
        );
        s.clock = Clock::FixedUnix(1_000_000 + 301);
        assert!(issue(&s).ok);
        assert_eq!(
            count(&s),
            2,
            "ATTACK: records of nonces past their max age (issued or spent) stay in the \
             custodian's store ({} files)",
            count(&s)
        );
    }

    /// A134 (M1866): at most MAX_OUTSTANDING issued-and-unspent nonces; the
    /// next issue is refused (Fabric cannot fill the custodian's disk), and a
    /// spend frees a slot. Control: MAX_OUTSTANDING issues succeed.
    #[test]
    fn the_custodian_bounds_the_nonces_it_holds_outstanding() {
        let d = tempfile::tempdir().unwrap();
        let s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let issue = || {
            s.answer(
                991,
                br#"{"schema":"axon-custodian-request/1","op":"issue","epoch":3}"#,
            )
        };
        let mut first = None;
        for i in 0..MAX_OUTSTANDING {
            let r = issue();
            assert!(r.ok, "control: issue {i} of {MAX_OUTSTANDING}: {r:?}");
            first = first.or(r.nonce);
        }
        let r = issue();
        assert!(
            !r.ok,
            "ATTACK: the custodian issued a nonce beyond {MAX_OUTSTANDING} outstanding: {r:?}"
        );
        let r = s.answer(
            0,
            serde_json::json!({"schema": REQUEST_SCHEMA, "op": "spend", "epoch": 3,
                               "nonce": first.unwrap(), "manifest_sha256": "a".repeat(64)})
            .to_string()
            .as_bytes(),
        );
        assert!(r.ok, "setup: a spend: {r:?}");
        assert!(issue().ok, "a spend frees a slot");
    }

    /// A150 (M2052, amendment 85): one connection has an ABSOLUTE deadline for
    /// its request, as the observer's does. A peer that sends a byte and goes
    /// silent is answered (refused as malformed) within the deadline, not held
    /// for 30 s per read. Control: a complete request is answered at once.
    #[test]
    fn a_custodian_connection_that_does_not_finish_its_request_is_cut_off() {
        let d = tempfile::tempdir().unwrap();
        let s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let run = |send: &'static [u8], silent: Duration| -> (Duration, String) {
            let (a, b) = UnixStream::pair().unwrap();
            let peer = std::thread::spawn(move || {
                let mut a = a;
                let _ = a.write_all(send);
                std::thread::sleep(silent);
                let _ = a.shutdown(std::net::Shutdown::Write);
                let mut reply = String::new();
                let _ = a.read_to_string(&mut reply);
                reply
            });
            let started = std::time::Instant::now();
            s.serve_one_within(b, Duration::from_millis(500));
            (started.elapsed(), peer.join().unwrap())
        };
        let (took, reply) = run(b"{\"sche", Duration::from_secs(3));
        assert!(
            took < Duration::from_secs(2),
            "ATTACK: a custodian connection that stopped sending held the custodian for {took:?} \
             (deadline 500 ms): {reply}"
        );
        let (took, reply) = run(b"{}\n", Duration::from_millis(0));
        assert!(
            took < Duration::from_secs(2) && reply.contains("axon-custodian-reply/1"),
            "control: {took:?} {reply}"
        );
    }

    /// C9 round 7, EQGATE3 (amendment 91; M2290): one request line is read
    /// through `take(MAX_MESSAGE)`. A peer that sends more than the bound
    /// without a newline is answered (refused as malformed) AT the bound, not
    /// when its deadline runs out: the single-threaded service is not held for
    /// the deadline by a peer that only has to send enough bytes. The deadline
    /// here is long (5 s) so only the size bound can answer sooner.
    #[test]
    fn a_custodian_request_line_is_cut_off_at_its_size_bound() {
        let d = tempfile::tempdir().unwrap();
        let s = Server {
            cfg: cfg(),
            mode: Mode::Test,
            store: NonceStore {
                dir: d.path().join("n"),
            },
            clock: Clock::FixedUnix(1_000_000),
        };
        let (a, b) = UnixStream::pair().unwrap();
        let peer = std::thread::spawn(move || {
            let mut a = a;
            let _ = a.write_all(&vec![b'x'; 3 * MAX_MESSAGE as usize]);
            let mut reply = String::new();
            let _ = a.set_read_timeout(Some(Duration::from_secs(8)));
            let _ = a.read_to_string(&mut reply);
            reply
        });
        let started = std::time::Instant::now();
        s.serve_one_within(b, Duration::from_secs(5));
        let took = started.elapsed();
        let reply = peer.join().unwrap();
        assert!(
            took < Duration::from_secs(2),
            "ATTACK: a custodian request line past the size bound was read until the deadline \
             ({took:?}): {reply}"
        );
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
