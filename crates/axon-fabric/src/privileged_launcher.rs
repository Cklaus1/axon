//! A — the privileged launcher helper (`axon-protected-launcher`; operator
//! decision A, 2026-09-29; `governance/specs/v022-psv-protocol.md`
//! amendment 45).
//!
//! Fabric runs NON-ROOT. The only thing on a protected host that needs root is
//! the launch itself (the Firecracker jailer, cgroups, a network namespace),
//! and that is done by this helper, a small compiled binary installed
//! setuid-root, executable only by the Fabric group:
//!
//! * the request is ONE fixed JSON schema on stdin, parsed with unknown
//!   fields denied: a jail id, the launch's out dir and PSV input dirs, the
//!   launch manifest digest, a timeout and the observation. Nothing in it
//!   names an executable, a manifest, an artifact or a config, and it carries
//!   no policy: the guest policy is the snapshot's `policy.json`, which must
//!   be the policy the launch manifest names (PSV-6, C9 round 4; A87);
//! * everything else comes from the operator's config at the FIXED path
//!   [`CONFIG_PATH`], ownership-walked from `/`: the pinned interpreter and
//!   launcher, the pinned profile manifest (which pins the kernel, rootfs,
//!   firecracker and jailer), the out root and a root-private staging root;
//! * the caller is authenticated by the KERNEL: its real uid must be the
//!   configured Fabric uid (and the file mode lets only the Fabric group run
//!   it at all);
//! * every path in the request must be a direct, well-formed child of the
//!   operator's out root, and is opened one component at a time with
//!   `O_NOFOLLOW` from `/`, each directory's owner and mode re-checked on its
//!   descriptor. The PSV inputs are copied from those descriptors into a
//!   root-private snapshot (regular files and directories owned by the Fabric
//!   uid only), so the root launcher never reads a path Fabric can redirect;
//! * the launcher (a bash script) is run by the pinned bash, both from their
//!   verified descriptors ([`crate::sealed_exec`]), with no shell evaluation of
//!   any request field (each is one argv word);
//! * the launch's out dir is created by the helper (root-owned while the
//!   launcher writes it, and handed to the launcher as `/dev/fd/N`), and given
//!   to the Fabric uid only after the launcher and its `--verify-result` have
//!   finished; special files are removed and set-id bits cleared on the way.
//!
//! The report on stdout names the build, the launcher and interpreter digests
//! that ran, both exits, and whether both inodes were unchanged throughout.

use serde::{Deserialize, Serialize};
use std::ffi::{CString, OsStr, OsString};
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path, PathBuf};

use crate::sealed_exec::{self, Lease, Pinned};

/// The one place the helper's configuration lives (production).
pub const CONFIG_PATH: &str = "/etc/axon/protected-launcher.json";
/// `/2` (amendment 50): the config names the observer root and the custodian.
pub const CONFIG_SCHEMA: &str = "axon-protected-launcher/2";
/// `/2` (amendment 50): the request carries the observation. `/3` (amendment
/// 54, A87): it no longer carries the guest policy, which is taken from the
/// PSV inputs snapshot and held to the launch manifest's `policy_sha256`.
pub const REQUEST_SCHEMA: &str = "axon-protected-launch-request/3";
pub const REPORT_SCHEMA: &str = "axon-protected-launch-report/1";
pub const PROBE_SCHEMA: &str = "axon-protected-launcher-probe/1";
/// Amendment 68: `--observe`, the relay to the observer service.
pub const OBSERVE_REQUEST_SCHEMA: &str = "axon-protected-observe-request/1";
pub const OBSERVE_REPORT_SCHEMA: &str = "axon-protected-observe-report/1";

/// Exit when the helper launched (the report says how it went).
pub const EXIT_LAUNCHED: i32 = 0;
/// Exit when the helper refused before launching anything.
pub const EXIT_REFUSED: i32 = 30;
/// Exit when something failed after the launch began (outcome unknown).
pub const EXIT_UNKNOWN: i32 = 31;

const PATH_ENV: &str = "/usr/sbin:/usr/bin:/sbin:/bin";
const MAX_REQUEST: u64 = 256 << 10;
const MAX_POLICY: usize = 64 << 10;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PinnedJson {
    pub path: PathBuf,
    pub sha256: String,
}

/// Amendment 79: the installed Fabric program, as the operator pins it. The
/// helper serves a launch or an `--observe` relay only for a caller whose
/// executable (hashed from its pidfd, [`running_caller`]) has this digest, and
/// the observer's `verifier_sha256` and `fabric_revision` are THESE values, not
/// what a launch manifest says.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FabricPin {
    /// Where the operator installed `axon-fabric` (informational for the
    /// helper: what is measured is the RUNNING executable's digest).
    pub path: PathBuf,
    pub sha256: String,
    /// The pinned binary's own build identity: the `fabric_revision` its
    /// `axon-fabric verifier-manifest` states, which the operator's kit READS
    /// FROM THE INSTALLED FILE at install time and the helper and observer then
    /// treat as the operator's word. Neither executes the Fabric to learn it.
    pub revision: String,
}

/// A build revision as a pin names it: the 40 hex a git commit is on a
/// production host (`unknown`, a short name or an uppercase spelling is not a
/// revision anything certified); any short printable name in a test config.
fn revision_ok(rev: &str, test: bool) -> bool {
    if test {
        !rev.is_empty()
            && rev.len() <= 64
            && rev
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    } else {
        rev.len() == 40 && rev.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    }
}

/// `axon-protected-launcher/1`, the operator's.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperConfig {
    pub schema: String,
    /// The only real uid allowed to make a request. Never 0 in production.
    pub fabric_uid: u32,
    /// Amendment 79: the Fabric program the helper serves (path, sha256,
    /// build revision), the operator's pin.
    pub fabric: FabricPin,
    /// Amendment 85, TEST-TRUST configs only: apply the private-reply-channel
    /// rule ([`reply_channel_private`]) to a test helper too. A production
    /// helper ALWAYS applies it (and refuses a stdout that is not a pipe); most
    /// test fixtures put the helper under a shell or a harness whose reader is
    /// not the helper's parent, so a test helper applies it only when asked.
    #[serde(default)]
    pub private_reply_channel: bool,
    /// The interpreter the launcher script runs under (bash).
    pub interpreter: PinnedJson,
    /// `fc_linux_profile.sh`.
    pub launcher: PinnedJson,
    /// `manifest.json`: pins the kernel, rootfs, firecracker and jailer.
    pub profile_manifest: PinnedJson,
    /// Holds `vmlinux` and `rootfs.sqfs`.
    pub artifacts_dir: PathBuf,
    pub firecracker: PathBuf,
    pub jailer: PathBuf,
    /// The Fabric service's out root (the same as the host config's).
    pub out_root: PathBuf,
    /// Root-private (0700): the PSV inputs are snapshotted here.
    pub staging_root: PathBuf,
    pub max_timeout_s: u64,
    pub max_input_bytes: u64,
    /// Amendment 50: the observation every launch requires is verified HERE,
    /// under this operator observer root.
    pub observer: HelperObserver,
    /// Amendment 50: the custodian the helper spends each launch's nonce
    /// through. Never the Fabric uid.
    pub custodian: crate::custodian::CustodianRef,
}

/// The helper's observer section (operator config).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperObserver {
    /// `/etc/axon/trust/observer` in production.
    pub root: PathBuf,
    pub max_age_s: u64,
    /// The host's attestation signer (hex): Fabric holds its private half, so
    /// the observer root must not hold it (ADR-002; the host config's
    /// `signer.public_key`, held equal by `helper_agrees`).
    pub host_signer_public_key: String,
    /// Amendment 68 (decision G1 = A): the observer SERVICE the helper's
    /// `--observe` relay asks, with its program pin. Absent: the helper
    /// relays no observation (and so no protected launch can be observed).
    #[serde(default)]
    pub service: Option<crate::observer_service::ObserverRef>,
}

/// `axon-protected-launch-request/3`: per-launch data only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LaunchRequest {
    pub schema: String,
    /// The jail id (`^[a-zA-Z0-9-]{1,60}$`).
    pub id: String,
    /// `<out_root>/<name>`: created by the helper; must not exist.
    pub out: PathBuf,
    /// `<out_root>/<inputs>/candidate`, `…/check`, `…/job`. The guest policy
    /// is `<inputs>/policy.json`, beside them (PSV-6, A87).
    pub psv_candidate: PathBuf,
    pub psv_suite: PathBuf,
    pub psv_job: PathBuf,
    pub psv_manifest_sha256: String,
    pub timeout_s: u64,
    /// Amendment 50: the exact observation bytes Fabric verified, and their
    /// observer signature (`axon-evidence-signature/2`). The helper verifies
    /// them itself and launches nothing without them.
    pub observation: String,
    pub observation_signature: String,
}

/// `axon-protected-launch-report/1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LaunchReport {
    pub schema: String,
    /// `production` or `test-trust`.
    pub build: String,
    /// Whether the launcher was started.
    pub launched: bool,
    /// Why nothing was launched, or what failed after the launch began.
    pub error: Option<String>,
    pub launcher_sha256: Option<String>,
    pub interpreter_sha256: Option<String>,
    pub launcher_exit: Option<i32>,
    pub verify_exit: Option<i32>,
    /// The launcher and interpreter inodes were the hashed ones, with their
    /// read leases held, from the hash until after `--verify-result`.
    pub unchanged: bool,
}

pub fn build_name() -> &'static str {
    if crate::backend::TEST_TRUST_BUILD {
        "test-trust"
    } else {
        "production"
    }
}

impl LaunchReport {
    fn refused(why: String) -> LaunchReport {
        LaunchReport {
            schema: REPORT_SCHEMA.into(),
            build: build_name().into(),
            launched: false,
            error: Some(why),
            launcher_sha256: None,
            interpreter_sha256: None,
            launcher_exit: None,
            verify_exit: None,
            unchanged: false,
        }
    }
}

/// Who owns operator objects, where the ownership walk starts, and whether
/// this is a test configuration.
#[derive(Debug, Clone)]
pub struct Authority {
    /// 0 in production.
    pub operator_uid: u32,
    /// `/` in production; a test's own directory in a test-trust build.
    pub walk_base: PathBuf,
    pub test: bool,
}

impl Authority {
    pub fn production() -> Authority {
        Authority {
            operator_uid: 0,
            walk_base: PathBuf::from("/"),
            test: false,
        }
    }
    fn lease(&self) -> Lease {
        // Root holds CAP_LEASE, so a production helper always gets one.
        if self.test {
            Lease::IfGranted
        } else {
            Lease::Required
        }
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn cstr(s: &OsStr) -> Result<CString, String> {
    CString::new(s.as_bytes()).map_err(|_| format!("{s:?} holds a NUL byte"))
}

fn fstat(fd: RawFd) -> Result<libc::stat, String> {
    // SAFETY: fstat into a zeroed stat on an open descriptor.
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut st) } != 0 {
        return Err(std::io::Error::last_os_error().to_string());
    }
    Ok(st)
}

fn openat(dirfd: RawFd, name: &OsStr, flags: libc::c_int) -> std::io::Result<OwnedFd> {
    let c = CString::new(name.as_bytes()).map_err(|_| std::io::Error::other("NUL in a name"))?;
    // SAFETY: openat relative to a descriptor we hold; the result is owned.
    let fd = unsafe { libc::openat(dirfd, c.as_ptr(), flags | libc::O_CLOEXEC, 0) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

const DIR_FLAGS: libc::c_int = libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW;

fn is_dir(st: &libc::stat) -> bool {
    st.st_mode & libc::S_IFMT == libc::S_IFDIR
}

/// An operator directory: owned by the operator, not group/other-writable.
fn operator_dir(p: &Path, st: &libc::stat, a: &Authority) -> Result<(), String> {
    if !is_dir(st) {
        return Err(format!("{} is not a directory", p.display()));
    }
    if st.st_uid != a.operator_uid {
        return Err(format!(
            "{} is owned by uid {}, not the operator ({})",
            p.display(),
            st.st_uid,
            a.operator_uid
        ));
    }
    if st.st_mode & 0o022 != 0 {
        return Err(format!(
            "{} is group- or other-writable (mode {:o})",
            p.display(),
            st.st_mode & 0o7777
        ));
    }
    Ok(())
}

/// Open directory `path` by walking from `a.walk_base`, one `O_NOFOLLOW`
/// component at a time. Every directory ABOVE the leaf must be an operator
/// directory (checked on its descriptor); the leaf is returned with its stat
/// for the caller's own rule.
fn walk_open(a: &Authority, path: &Path) -> Result<(OwnedFd, libc::stat), String> {
    let rel = path
        .strip_prefix(&a.walk_base)
        .map_err(|_| format!("{} is not below {}", path.display(), a.walk_base.display()))?;
    let base = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags_dir()
        .open(&a.walk_base)
        .map_err(|e| format!("{}: {e}", a.walk_base.display()))?;
    let mut fd: OwnedFd = base.into();
    let mut st = fstat(fd.as_raw_fd())?;
    let mut here = a.walk_base.clone();
    for c in rel.components() {
        let Component::Normal(name) = c else {
            return Err(format!("{} is not a plain path", path.display()));
        };
        operator_dir(&here, &st, a)?;
        here.push(name);
        fd = openat(fd.as_raw_fd(), name, DIR_FLAGS).map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                format!("{} is a symlink: never followed", here.display())
            } else {
                format!("{}: {e}", here.display())
            }
        })?;
        st = fstat(fd.as_raw_fd())?;
    }
    Ok((fd, st))
}

trait DirOpen {
    fn custom_flags_dir(&mut self) -> &mut Self;
}
impl DirOpen for std::fs::OpenOptions {
    fn custom_flags_dir(&mut self) -> &mut Self {
        use std::os::unix::fs::OpenOptionsExt;
        self.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
    }
}

/// Read an operator file: its directory chain from the walk base, and the
/// file itself, operator-owned and not group/other-writable. ONE reader for
/// the helper's config and the custodian's (amendment 50).
pub fn read_operator_file(path: &Path, a: &Authority) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let bad = |why: String| format!("{}: {why}", path.display());
    let parent = path.parent().ok_or_else(|| bad("no parent".into()))?;
    let name = path.file_name().ok_or_else(|| bad("no file name".into()))?;
    let (dir, st) = walk_open(a, parent).map_err(bad)?;
    operator_dir(parent, &st, a).map_err(bad)?;
    let f = openat(
        dir.as_raw_fd(),
        name,
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
    )
    .map_err(|e| bad(e.to_string()))?;
    let st = fstat(f.as_raw_fd()).map_err(bad)?;
    if st.st_mode & libc::S_IFMT != libc::S_IFREG {
        return Err(bad("not a regular file".into()));
    }
    if st.st_uid != a.operator_uid || st.st_mode & 0o022 != 0 {
        return Err(bad(format!(
            "must be owned by the operator ({}) and not group/other-writable; is uid {} mode {:o}",
            a.operator_uid,
            st.st_uid,
            st.st_mode & 0o7777
        )));
    }
    let mut bytes = Vec::new();
    std::fs::File::from(f)
        .take(MAX_REQUEST)
        .read_to_end(&mut bytes)
        .map_err(|e| bad(e.to_string()))?;
    Ok(bytes)
}

/// Read the operator's config ([`read_operator_file`]) and check it.
pub fn load_config(path: &Path, a: &Authority) -> Result<HelperConfig, String> {
    let bad = |why: String| format!("{}: {why}", path.display());
    let bytes = read_operator_file(path, a)?;
    let c: HelperConfig = serde_json::from_slice(&bytes).map_err(|e| bad(e.to_string()))?;
    if c.schema != CONFIG_SCHEMA {
        return Err(bad(format!("schema is not {CONFIG_SCHEMA}")));
    }
    if c.fabric_uid == 0 && !a.test {
        return Err(bad(
            "fabric_uid is 0: Fabric runs non-root, and root would need no helper".into(),
        ));
    }
    for p in [&c.interpreter, &c.launcher, &c.profile_manifest] {
        if !is_hex64(&p.sha256) {
            return Err(bad(format!(
                "{} has no lowercase sha256 pin",
                p.path.display()
            )));
        }
    }
    // Amendment 79: the Fabric program is pinned like every other program the
    // helper trusts, and its build revision is the operator's word.
    if !is_hex64(&c.fabric.sha256) {
        return Err(bad(format!(
            "fabric.sha256 must pin the installed axon-fabric program ({}): a lowercase sha256. \
             Unpinned, any program of the Fabric uid is \"the running Fabric\"",
            c.fabric.path.display()
        )));
    }
    if !revision_ok(&c.fabric.revision, a.test) {
        return Err(bad(format!(
            "fabric.revision {:?} is not the pinned program's build revision (40 lowercase hex \
             on a production host)",
            c.fabric.revision
        )));
    }
    for p in [
        &c.fabric.path,
        &c.interpreter.path,
        &c.launcher.path,
        &c.profile_manifest.path,
        &c.artifacts_dir,
        &c.firecracker,
        &c.jailer,
        &c.out_root,
        &c.staging_root,
        &c.observer.root,
        &c.custodian.socket,
    ] {
        if !p.is_absolute()
            || p.components()
                .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        {
            return Err(bad(format!(
                "{} is not an absolute plain path",
                p.display()
            )));
        }
    }
    if c.firecracker.file_name() != Some(OsStr::new("firecracker")) {
        return Err(bad("firecracker must be a file named 'firecracker'".into()));
    }
    if c.max_timeout_s == 0 || c.max_input_bytes == 0 {
        return Err(bad(
            "max_timeout_s and max_input_bytes must be positive".into()
        ));
    }
    if c.observer.max_age_s == 0 || !is_hex64(&c.observer.host_signer_public_key) {
        return Err(bad(
            "observer.max_age_s must be positive and observer.host_signer_public_key a \
             lowercase 64-hex key"
                .into(),
        ));
    }
    // A83: the nonce is not the constrained principal's to keep.
    if !a.test && (c.custodian.uid == c.fabric_uid || c.custodian.uid == 0) {
        return Err(bad(format!(
            "custodian.uid {} is the Fabric uid or root: the custodian is its own uid \
             (amendment 50)",
            c.custodian.uid
        )));
    }
    // Amendment 65: the custodian is the PROGRAM the operator pinned, checked
    // against the process that answers (custodian::check_sender_program).
    // Unpinned, any program bound on the socket as the custodian uid (or
    // systemd's socket activation of whatever ExecStart names) spends nonces.
    match &c.custodian.sha256 {
        Some(pin) if is_hex64(pin) => {}
        None if a.test => {}
        _ => {
            return Err(bad(
                "custodian.sha256 must pin the axon-custodian program (a lowercase sha256): \
                 an unpinned custodian is any program its socket's listener runs"
                    .into(),
            ))
        }
    }
    // Amendment 68 (A94): the observer service is its own uid, and the
    // PROGRAM the operator pinned (checked against every reply's sender).
    if let Some(s) = &c.observer.service {
        let plain = s.socket.is_absolute()
            && s.socket
                .components()
                .all(|c| matches!(c, Component::RootDir | Component::Normal(_)));
        if !plain || !is_hex64(&s.sha256) {
            return Err(bad(
                "observer.service names an absolute plain socket and pins the axon-observer \
                 program (a lowercase sha256)"
                    .into(),
            ));
        }
        if !a.test && (s.uid == c.fabric_uid || s.uid == 0) {
            return Err(bad(format!(
                "observer.service.uid {} is the Fabric uid or root: the observer is its own uid, \
                 or the Fabric could read its key and mint observations (amendment 68)",
                s.uid
            )));
        }
        // Amendment 79: the observer and the custodian are two principals. The
        // observer's key is not the custodian's to read, and the custodian's
        // store (the record of what was issued) is not the observer's to write.
        if !a.test && s.uid == c.custodian.uid {
            return Err(bad(format!(
                "observer.service.uid {} is the custodian's uid: the observer and the custodian \
                 are two principals, or either could read the other's key or store \
                 (amendment 79)",
                s.uid
            )));
        }
    }
    Ok(c)
}

/// A request path's name under the out root: one plain component.
fn plain_name(s: &OsStr) -> bool {
    let b = s.as_bytes();
    !b.is_empty()
        && b.len() <= 128
        && b[0] != b'.'
        && b.iter()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-' | b':'))
}

/// What a validated request names, relative to the out root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Plan {
    pub out_name: OsString,
    pub inputs_name: OsString,
}

/// The name `p` has as a DIRECT child of `root` — `p` must be exactly
/// `root/<name>`, spelled plainly (no `.`, `..`, doubled or trailing `/`).
fn child_of(root: &Path, p: &Path, what: &str) -> Result<OsString, String> {
    let outside = || {
        format!(
            "{what} {} is not a direct child of the operator's out root {}",
            p.display(),
            root.display()
        )
    };
    let name = p.file_name().ok_or_else(outside)?;
    if !plain_name(name) || root.join(name).as_os_str() != p.as_os_str() {
        return Err(outside());
    }
    Ok(name.to_os_string())
}

/// Validate a request against the operator's config. Every path must be under
/// the out root, in the fixed shape Fabric uses; nothing else is accepted.
pub fn validate_request(r: &LaunchRequest, c: &HelperConfig) -> Result<Plan, String> {
    if r.schema != REQUEST_SCHEMA {
        return Err(format!("request schema is not {REQUEST_SCHEMA}"));
    }
    let id_ok = !r.id.is_empty()
        && r.id.len() <= 60
        && r.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-');
    if !id_ok {
        return Err(format!("jail id {:?} is not [a-zA-Z0-9-]{{1,60}}", r.id));
    }
    if !is_hex64(&r.psv_manifest_sha256) {
        return Err("psv_manifest_sha256 is not a lowercase sha256".into());
    }
    if r.timeout_s == 0 || r.timeout_s > c.max_timeout_s {
        return Err(format!(
            "timeout_s {} is outside 1..={}",
            r.timeout_s, c.max_timeout_s
        ));
    }
    let out_name = child_of(&c.out_root, &r.out, "out")?;
    let mut inputs = None;
    for (p, leaf) in [
        (&r.psv_candidate, "candidate"),
        (&r.psv_suite, "check"),
        (&r.psv_job, "job"),
    ] {
        let outside = || {
            format!(
                "psv input {} is not <out root>/<inputs>/{leaf} under the operator's out root {}",
                p.display(),
                c.out_root.display()
            )
        };
        if p.file_name() != Some(OsStr::new(leaf)) {
            return Err(outside());
        }
        let parent = p.parent().ok_or_else(outside)?;
        let name = child_of(&c.out_root, parent, "psv inputs").map_err(|_| outside())?;
        if parent.join(leaf).as_os_str() != p.as_os_str() {
            return Err(outside());
        }
        match &inputs {
            None => inputs = Some(name),
            Some(n) if *n == name => {}
            Some(_) => return Err("the three psv inputs are not in one inputs dir".into()),
        }
    }
    let inputs_name = inputs.expect("three inputs");
    if inputs_name == out_name {
        return Err("the out dir and the psv inputs dir are the same".into());
    }
    Ok(Plan {
        out_name,
        inputs_name,
    })
}

/// Parse a request: exactly the schema, unknown fields refused.
pub fn parse_request(bytes: &[u8]) -> Result<LaunchRequest, String> {
    serde_json::from_slice(bytes).map_err(|e| format!("malformed request: {e}"))
}

/// The out root, opened by the ownership walk: its ancestors operator
/// directories, the root itself the Fabric uid's and private (0700).
fn open_out_root(c: &HelperConfig, a: &Authority) -> Result<OwnedFd, String> {
    let (fd, st) = walk_open(a, &c.out_root)?;
    if !is_dir(&st) || st.st_uid != c.fabric_uid || st.st_mode & 0o077 != 0 {
        return Err(format!(
            "out root {} must be a 0700 directory of the Fabric uid {} (is uid {} mode {:o})",
            c.out_root.display(),
            c.fabric_uid,
            st.st_uid,
            st.st_mode & 0o7777
        ));
    }
    Ok(fd)
}

/// A new private staging dir under the root-private staging root.
fn new_staging(c: &HelperConfig, a: &Authority, id: &str) -> Result<PathBuf, String> {
    let (_, st) = walk_open(a, &c.staging_root)?;
    operator_dir(&c.staging_root, &st, a)?;
    if st.st_mode & 0o077 != 0 {
        return Err(format!(
            "staging root {} is not private (mode {:o}; must be 0700)",
            c.staging_root.display(),
            st.st_mode & 0o7777
        ));
    }
    let mut rnd = [0u8; 8];
    ring::rand::SecureRandom::fill(&ring::rand::SystemRandom::new(), &mut rnd)
        .map_err(|_| "no system randomness".to_string())?;
    let tag: String = rnd.iter().map(|b| format!("{b:02x}")).collect();
    let dir = c.staging_root.join(format!("{id}-{tag}"));
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(&dir)
        .map_err(|e| format!("staging {}: {e}", dir.display()))?;
    Ok(dir)
}

fn names_in(fd: RawFd) -> Result<Vec<OsString>, String> {
    // The listing is of the descriptor's own directory (a magic link), so a
    // rename of the path after the open lists nothing else.
    let mut v: Vec<OsString> = std::fs::read_dir(format!("/proc/self/fd/{fd}"))
        .map_err(|e| e.to_string())?
        .map(|e| e.map(|e| e.file_name()).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;
    v.sort();
    Ok(v)
}

/// Copy the tree under `src` (a verified descriptor) into the new directory
/// `dst`. Only directories and regular files owned by `owner` are copied;
/// anything else (a symlink, a FIFO, a device, a root-owned hard link) is
/// refused. The exec bit is kept (the tree digest records it).
fn copy_tree(src: RawFd, dst: &Path, owner: u32, budget: &mut u64) -> Result<(), String> {
    for name in names_in(src)? {
        let shown = dst.join(&name);
        let fd = openat(
            src,
            &name,
            libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
        )
        .map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                format!(
                    "{} is a symlink: an input is never followed",
                    shown.display()
                )
            } else {
                format!("{}: {e}", shown.display())
            }
        })?;
        let st = fstat(fd.as_raw_fd())?;
        if st.st_uid != owner {
            return Err(format!(
                "{} is owned by uid {}, not the Fabric uid {owner}",
                shown.display(),
                st.st_uid
            ));
        }
        match st.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                std::fs::create_dir(&shown).map_err(|e| format!("{}: {e}", shown.display()))?;
                set_mode(&shown, 0o755)?;
                copy_tree(fd.as_raw_fd(), &shown, owner, budget)?;
            }
            libc::S_IFREG => {
                let size = st.st_size.max(0) as u64;
                if size > *budget {
                    return Err("the psv inputs exceed max_input_bytes".into());
                }
                *budget = budget.saturating_sub(size);
                use std::os::unix::fs::OpenOptionsExt;
                let mode = if st.st_mode & 0o111 != 0 {
                    0o755
                } else {
                    0o644
                };
                let mut out = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(mode)
                    .custom_flags(libc::O_NOFOLLOW)
                    .open(&shown)
                    .map_err(|e| format!("{}: {e}", shown.display()))?;
                use std::io::Read;
                let mut from = std::fs::File::from(fd).take(size);
                std::io::copy(&mut from, &mut out)
                    .map_err(|e| format!("{}: {e}", shown.display()))?;
                // The mode the tree digest records (the umask does not apply):
                // readable, and executable exactly when the source was.
                set_mode(&shown, mode)?;
            }
            _ => {
                return Err(format!(
                    "{} is not a regular file or directory",
                    shown.display()
                ))
            }
        }
    }
    Ok(())
}

fn set_mode(p: &Path, mode: u32) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode))
        .map_err(|e| format!("{}: {e}", p.display()))
}

/// Snapshot `<inputs>/{candidate,check,job}` and `<inputs>/policy.json` into
/// `staging`, then remove the job's two files from Fabric's dir (the secret
/// then exists once: in the root-private snapshot, removed when the launcher
/// returns).
fn snapshot_inputs(
    root: RawFd,
    inputs: &OsStr,
    staging: &Path,
    c: &HelperConfig,
) -> Result<(), String> {
    let ifd = openat(root, inputs, DIR_FLAGS).map_err(|e| format!("psv inputs: {e}"))?;
    let st = fstat(ifd.as_raw_fd())?;
    if st.st_uid != c.fabric_uid {
        return Err(format!(
            "psv inputs dir is owned by uid {}, not the Fabric uid",
            st.st_uid
        ));
    }
    snapshot_policy(ifd.as_raw_fd(), &staging.join("policy.json"), c.fabric_uid)?;
    let mut budget = c.max_input_bytes;
    for leaf in ["candidate", "check", "job"] {
        let fd = openat(ifd.as_raw_fd(), OsStr::new(leaf), DIR_FLAGS).map_err(|e| {
            if e.raw_os_error() == Some(libc::ELOOP) {
                format!("psv input {leaf} is a symlink: never followed")
            } else {
                format!("psv input {leaf}: {e}")
            }
        })?;
        let st = fstat(fd.as_raw_fd())?;
        if st.st_uid != c.fabric_uid {
            return Err(format!(
                "psv input {leaf} is owned by uid {}, not the Fabric uid",
                st.st_uid
            ));
        }
        let dst = staging.join(leaf);
        std::fs::create_dir(&dst).map_err(|e| format!("{}: {e}", dst.display()))?;
        set_mode(&dst, 0o755)?;
        copy_tree(fd.as_raw_fd(), &dst, c.fabric_uid, &mut budget)?;
        if leaf == "job" {
            for n in names_in(fd.as_raw_fd())? {
                let cn = cstr(&n)?;
                // SAFETY: unlinkat relative to the verified job descriptor.
                unsafe { libc::unlinkat(fd.as_raw_fd(), cn.as_ptr(), 0) };
            }
            // SAFETY: as above, the (now empty) job dir itself.
            unsafe { libc::unlinkat(ifd.as_raw_fd(), c"job".as_ptr(), libc::AT_REMOVEDIR) };
        }
    }
    Ok(())
}

/// Copy `<inputs>/policy.json` (a regular file of the Fabric uid, never
/// followed, at most [`MAX_POLICY`] bytes) to `dst` in the root-private
/// snapshot. What the launcher boots is `dst`; [`observed_launch`] holds it to
/// the manifest's `policy_sha256` before the nonce is spent.
fn snapshot_policy(inputs: RawFd, dst: &Path, owner: u32) -> Result<(), String> {
    let fd = openat(
        inputs,
        OsStr::new("policy.json"),
        libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
    )
    .map_err(|e| {
        if e.raw_os_error() == Some(libc::ELOOP) {
            "the psv policy.json is a symlink: never followed".to_string()
        } else {
            format!("the psv inputs hold no policy.json: {e}")
        }
    })?;
    let st = fstat(fd.as_raw_fd())?;
    if st.st_mode & libc::S_IFMT != libc::S_IFREG || st.st_uid != owner {
        return Err(format!(
            "the psv policy.json is not a regular file of the Fabric uid (uid {})",
            st.st_uid
        ));
    }
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::from(fd)
        .take(MAX_POLICY as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| format!("policy.json: {e}"))?;
    if bytes.len() > MAX_POLICY {
        return Err("the psv policy.json is too large".into());
    }
    std::fs::write(dst, &bytes).map_err(|e| format!("policy: {e}"))
}

/// Create `<out_root>/<name>` (it must not exist) and open it; it is the
/// helper's until the launch is over.
fn make_out(root: RawFd, name: &OsStr) -> Result<OwnedFd, String> {
    let cn = cstr(name)?;
    // SAFETY: mkdirat relative to the verified out root.
    if unsafe { libc::mkdirat(root, cn.as_ptr(), 0o700) } != 0 {
        return Err(format!(
            "out dir {name:?}: {} (it must be new)",
            std::io::Error::last_os_error()
        ));
    }
    let fd = openat(root, name, DIR_FLAGS).map_err(|e| format!("out dir: {e}"))?;
    let st = fstat(fd.as_raw_fd())?;
    // SAFETY: geteuid cannot fail.
    if st.st_uid != unsafe { libc::geteuid() } {
        return Err("the out dir was replaced between its creation and its open".into());
    }
    Ok(fd)
}

/// Give the finished out tree to the Fabric uid: every directory, regular
/// file and symlink (never followed) is chowned; set-id bits are cleared;
/// anything else (a device node, a FIFO, a socket), whatever put it there,
/// is removed, never handed over.
fn hand_over(dir: RawFd, uid: u32) -> Result<(), String> {
    for name in names_in(dir)? {
        let cn = cstr(&name)?;
        // SAFETY: fstatat relative to a directory only root writes.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        if unsafe { libc::fstatat(dir, cn.as_ptr(), &mut st, libc::AT_SYMLINK_NOFOLLOW) } != 0 {
            return Err(format!("{name:?}: {}", std::io::Error::last_os_error()));
        }
        let ok = match st.st_mode & libc::S_IFMT {
            libc::S_IFDIR => {
                let fd = openat(dir, &name, DIR_FLAGS).map_err(|e| e.to_string())?;
                hand_over(fd.as_raw_fd(), uid)?;
                // SAFETY: fchmod/fchown on a descriptor we hold.
                unsafe {
                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0
                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0
                }
            }
            libc::S_IFREG => {
                let fd = openat(
                    dir,
                    &name,
                    libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                )
                .map_err(|e| e.to_string())?;
                unsafe {
                    libc::fchmod(fd.as_raw_fd(), st.st_mode & 0o777) == 0
                        && libc::fchown(fd.as_raw_fd(), uid, u32::MAX) == 0
                }
            }
            libc::S_IFLNK => unsafe {
                libc::fchownat(dir, cn.as_ptr(), uid, u32::MAX, libc::AT_SYMLINK_NOFOLLOW) == 0
            },
            _ => unsafe { libc::unlinkat(dir, cn.as_ptr(), 0) == 0 },
        };
        if !ok {
            return Err(format!(
                "handing {name:?} to the Fabric uid: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

/// The pinned inputs, verified by the helper itself: the manifest at its
/// config pin, then the kernel and rootfs at the manifest's artifact pins
/// and firecracker and jailer at its engine pins, each operator-owned and
/// opened without following a symlink. Returns the verified manifest.
fn verify_inputs(c: &HelperConfig, a: &Authority) -> Result<sealed_exec::Verified, String> {
    let owner = Some(a.operator_uid);
    let manifest = sealed_exec::open_verified(
        &Pinned {
            path: c.profile_manifest.path.clone(),
            sha256: c.profile_manifest.sha256.clone(),
        },
        owner,
        a.lease(),
    )?;
    let bytes = std::fs::read(format!("/proc/self/fd/{}", manifest.fd()))
        .map_err(|e| format!("manifest: {e}"))?;
    if crate::backend::sha256_hex(&bytes) != c.profile_manifest.sha256 {
        return Err("the manifest changed while it was read".into());
    }
    let m: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|e| format!("manifest: {e}"))?;
    let pin = |ptr: &str| -> Result<String, String> {
        m.pointer(ptr)
            .and_then(|v| v.as_str())
            .filter(|s| is_hex64(s))
            .map(String::from)
            .ok_or_else(|| format!("the profile manifest pins no {ptr}"))
    };
    for (path, ptr) in [
        (c.artifacts_dir.join("vmlinux"), "/artifacts/vmlinux/sha256"),
        (
            c.artifacts_dir.join("rootfs.sqfs"),
            "/artifacts/rootfs.sqfs/sha256",
        ),
        (c.firecracker.clone(), "/engine/firecracker_sha256"),
        (c.jailer.clone(), "/engine/jailer_sha256"),
    ] {
        sealed_exec::open_verified(
            &Pinned {
                path,
                sha256: pin(ptr)?,
            },
            owner,
            Lease::IfGranted,
        )?;
    }
    Ok(manifest)
}

fn exit_code(s: std::io::Result<std::process::ExitStatus>) -> Option<i32> {
    s.ok().and_then(|s| s.code())
}

/// Serve ONE request from `caller_uid`, becoming root in every id AFTER the caller is authenticated and
/// before anything is prepared or launched (the helper binary's entry).
pub fn serve_as(
    config: &Path,
    a: &Authority,
    caller_uid: u32,
    request: &[u8],
) -> (LaunchReport, i32) {
    let c = match authenticated(config, a, caller_uid) {
        Ok((c, _)) => c,
        Err(why) => return (LaunchReport::refused(why), EXIT_REFUSED),
    };
    match prepare(&c, a, request) {
        Err(why) => (LaunchReport::refused(why), EXIT_REFUSED),
        Ok(p) => run(&c, p),
    }
}

/// The operator's config, once the caller is authenticated: its REAL uid (the
/// kernel's, never anything it says) is the configured Fabric uid. Only then
/// does the helper become root in every id. ONE gate for every operation the
/// helper performs (a launch, an observe relay).
///
/// Amendment 79: and the caller's PROGRAM is the one the operator pinned
/// (`fabric.sha256`): the digest of the executable its pidfd names
/// ([`running_caller`]). Before, any program of the Fabric uid was served and,
/// for `--observe`, named as the verifier in an observer-signed observation.
/// Returns the config and that digest.
fn authenticated(
    config: &Path,
    a: &Authority,
    caller_uid: u32,
) -> Result<(HelperConfig, String), String> {
    let c = load_config(config, a)?;
    if caller_uid != c.fabric_uid {
        return Err(format!(
            "caller uid {caller_uid} is not the configured Fabric uid {}",
            c.fabric_uid
        ));
    }
    become_root()?;
    let running = running_caller(c.fabric_uid)?;
    if running != c.fabric.sha256 {
        return Err(format!(
            "the caller runs a program with sha256 {running}, not the operator's pinned Fabric \
             {} ({}): only a caller running the installed axon-fabric file is served",
            c.fabric.sha256,
            c.fabric.path.display()
        ));
    }
    if !a.test || c.private_reply_channel {
        reply_channel_private(!a.test)?;
    }
    Ok((c, running))
}

/// Amendment 85: who can READ the reply. The helper answers on its stdout, and
/// the digest of the parent's executable (`running_caller`) is a property of
/// whatever image that pid has when `/proc/<ppid>/exe` is opened, not of how it
/// got there. EXECUTED (round-6 reviewer, 18 of 20): a Fabric-uid program that
/// is not the pinned one spawns the helper with a pipe it (or a forked worker)
/// reads, and `execve`s the pinned file; the helper measures the pinned file
/// and the reader gets a signed observation. A request is therefore served
/// only when the reply pipe has no holder but this process and its parent: the
/// worker of that attack holds the read end and is refused. A production
/// helper also refuses a stdout that is not a pipe (a file or terminal is
/// readable by every process of the uid, with no holder to count).
/// What this does NOT close is stated in amendment 85: the same-uid ways to
/// obtain the pipe afterwards (reopening `/proc/<pid>/fd/N` of the parent,
/// fds in flight, `pidfd_getfd`, ptrace, `LD_PRELOAD`).
pub fn reply_channel_private(production: bool) -> Result<(), String> {
    // SAFETY: fstat of fd 1 into a zeroed stat; getpid/getppid cannot fail.
    let (st, me, ppid) = unsafe {
        let mut st: libc::stat = std::mem::zeroed();
        if libc::fstat(1, &mut st) != 0 {
            return Err(format!(
                "the helper's stdout is not open: {}",
                std::io::Error::last_os_error()
            ));
        }
        (st, libc::getpid(), libc::getppid())
    };
    let mode = st.st_mode & libc::S_IFMT;
    if mode != libc::S_IFIFO {
        return channel_verdict(mode, &[], ppid, me, production);
    }
    // A holder that is another child of the parent mid-spawn (a pipe end not
    // yet closed by its exec) goes away within milliseconds; the attack's
    // worker does not. Judged repeatedly for a short while.
    let mut last = Ok(());
    for _ in 0..30 {
        let holders = pipe_holders(st.st_dev, st.st_ino, production)?;
        last = channel_verdict(mode, &holders, ppid, me, production);
        if last.is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    last
}

/// The decision of [`reply_channel_private`], over what the scan found.
fn channel_verdict(
    mode: libc::mode_t,
    holders: &[libc::pid_t],
    ppid: libc::pid_t,
    me: libc::pid_t,
    production: bool,
) -> Result<(), String> {
    if mode != libc::S_IFIFO {
        if production {
            return Err(
                "the helper's stdout is not a pipe: a file or terminal is readable by every \
                 process of the Fabric uid, so nothing says who reads the reply (amendment 85)"
                    .into(),
            );
        }
        return Ok(());
    }
    if let Some(p) = holders.iter().find(|p| **p != ppid && **p != me) {
        return Err(format!(
            "pid {p}, which is neither the helper nor its parent (pid {ppid}), holds the pipe the \
             reply would be written to: whoever holds it reads the observation, whatever program \
             the parent is (amendment 85)"
        ));
    }
    Ok(())
}

/// Every process with an open descriptor on the pipe (`dev`, `ino`), by
/// walking `/proc/<pid>/fd`. A process that exits during the walk is skipped;
/// one whose descriptors cannot be read is skipped by an unprivileged
/// (test-trust) helper and refuses a production (root) one.
fn pipe_holders(
    dev: libc::dev_t,
    ino: libc::ino_t,
    production: bool,
) -> Result<Vec<libc::pid_t>, String> {
    use std::os::unix::fs::MetadataExt;
    let mut out = Vec::new();
    let procs = std::fs::read_dir("/proc").map_err(|e| format!("/proc: {e}"))?;
    for e in procs.flatten() {
        let Some(pid) = e
            .file_name()
            .to_str()
            .and_then(|n| n.parse::<libc::pid_t>().ok())
        else {
            continue;
        };
        let fds = match std::fs::read_dir(format!("/proc/{pid}/fd")) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
            Err(e) if !production && e.kind() == std::io::ErrorKind::PermissionDenied => continue,
            Err(e) => return Err(format!("/proc/{pid}/fd: {e}")),
        };
        for fd in fds.flatten() {
            if let Ok(m) = std::fs::metadata(fd.path()) {
                if m.ino() == ino && m.dev() == dev {
                    out.push(pid);
                    break;
                }
            }
        }
    }
    Ok(out)
}

/// `axon-protected-observe-request/1` (amendment 68): the launch manifest
/// Fabric built, and nothing else. Which observer, and its program pin, are
/// the operator's (`observer.service`); what the running Fabric is, the
/// helper measures ([`running_caller`]).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObserveRequest {
    pub schema: String,
    /// The launch manifest's canonical text.
    pub manifest: String,
}

/// `axon-protected-observe-report/1`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ObserveReport {
    pub schema: String,
    /// `production` or `test-trust`.
    pub build: String,
    pub ok: bool,
    pub error: Option<String>,
    /// The observation's exact bytes and its observer signature.
    pub observation: Option<String>,
    pub observation_signature: Option<String>,
}

impl ObserveReport {
    pub fn refused(why: String) -> ObserveReport {
        ObserveReport {
            schema: OBSERVE_REPORT_SCHEMA.into(),
            build: build_name().into(),
            ok: false,
            error: Some(why),
            observation: None,
            observation_signature: None,
        }
    }
}

/// `--observe`: relay ONE observation request from `caller_uid` to the
/// operator's observer service (amendment 68, decision G1 = A). The caller is
/// authenticated exactly as for a launch ([`authenticated`]); the running
/// Fabric is measured from this process's parent ([`running_caller`]); the
/// observer is authenticated by the kernel's sender of every reply message
/// against the operator's program pin ([`crate::custodian::check_sender_program`]);
/// and only a protected observer's observation (a test observer's, in a
/// test-trust helper) is relayed. Nothing is launched.
pub fn serve_observe(
    config: &Path,
    a: &Authority,
    caller_uid: u32,
    request: &[u8],
) -> (ObserveReport, i32) {
    match authenticated(config, a, caller_uid)
        .and_then(|(c, running)| observe_relay(&c, a, &running, request))
    {
        Ok((observation, signature)) => (
            ObserveReport {
                schema: OBSERVE_REPORT_SCHEMA.into(),
                build: build_name().into(),
                ok: true,
                error: None,
                observation: Some(observation),
                observation_signature: Some(signature),
            },
            EXIT_LAUNCHED,
        ),
        Err(why) => (ObserveReport::refused(why), EXIT_REFUSED),
    }
}

fn observe_relay(
    c: &HelperConfig,
    a: &Authority,
    caller_sha256: &str,
    request: &[u8],
) -> Result<(String, String), String> {
    let r: ObserveRequest =
        serde_json::from_slice(request).map_err(|e| format!("malformed observe request: {e}"))?;
    if r.schema != OBSERVE_REQUEST_SCHEMA {
        return Err(format!(
            "observe request schema is not {OBSERVE_REQUEST_SCHEMA}"
        ));
    }
    let service = c
        .observer
        .service
        .as_ref()
        .ok_or("this helper's config names no observer.service: no observation is relayed")?;
    let got = service
        .observe(
            &r.manifest,
            caller_sha256,
            crate::observer_service::observe_timeout(artifact_bytes(c)),
        )
        .map_err(|e| format!("no observation: {e}"))?;
    if !custodian_mode_launches(got.mode, a.test) {
        return Err(format!(
            "the observation was made by a {} observer: never relayed for a protected launch",
            got.mode.as_str()
        ));
    }
    Ok((got.observation, got.signature))
}

/// The bytes the observer streams through SHA-256 for one observation: the
/// guest kernel and rootfs (the largest things it measures). A file that cannot
/// be sized counts as nothing: the observer then fails to measure it, which
/// refuses the observation whatever the timeout was.
fn artifact_bytes(c: &HelperConfig) -> u64 {
    ["vmlinux", "rootfs.sqfs"]
        .iter()
        .filter_map(|n| std::fs::symlink_metadata(c.artifacts_dir.join(n)).ok())
        .map(|m| m.len())
        .sum()
}

/// The RUNNING Fabric (amendment 68): this helper's parent, identified by a
/// pidfd, never by a bare pid. Its every uid must be the Fabric uid, and the
/// sha256 of the executable it is running (opened through
/// `/proc/<pid>/exe`, which only root may open across uids) is returned for
/// the observer to join to the manifest's `verifier_sha256`. Fails closed if
/// the parent cannot be identified, has exited (this process was
/// reparented), or is not the Fabric uid.
pub fn running_caller(fabric_uid: u32) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    // SAFETY: getppid cannot fail.
    let ppid = unsafe { libc::getppid() };
    // SAFETY: pidfd_open with a pid and no flags; the result is checked.
    let raw = unsafe { libc::syscall(libc::SYS_pidfd_open, ppid, 0) } as RawFd;
    if raw < 0 {
        return Err(format!(
            "the helper's parent (pid {ppid}) cannot be identified: {}",
            std::io::Error::last_os_error()
        ));
    }
    // SAFETY: a new descriptor this process owns from here on.
    let pidfd = unsafe { OwnedFd::from_raw_fd(raw) };
    let still_parent = || {
        // SAFETY: getppid cannot fail.
        crate::custodian::pidfd_pid(&pidfd) == Some(i64::from(ppid))
            && unsafe { libc::getppid() } == ppid
    };
    // A parent that exited before the pidfd was opened left this process
    // reparented: the pid no longer names the process that ran the helper.
    if !still_parent() {
        return Err(format!(
            "the helper's parent (pid {ppid}) exited: the running Fabric cannot be measured"
        ));
    }
    let status = std::fs::read_to_string(format!("/proc/{ppid}/status"))
        .map_err(|e| format!("the helper's parent (pid {ppid}): {e}"))?;
    let uids: Vec<u32> = status
        .lines()
        .find_map(|l| l.strip_prefix("Uid:"))
        .map(|v| {
            v.split_whitespace()
                .filter_map(|u| u.parse().ok())
                .collect()
        })
        .unwrap_or_default();
    let mut exe = std::fs::File::open(format!("/proc/{ppid}/exe"))
        .map_err(|e| format!("the helper's parent's executable (pid {ppid}): {e}"))?;
    // Asked again AFTER the reads: alive and still the parent then means it
    // was the same process at the reads (the pidfd pins it, not the number).
    if !still_parent() {
        return Err(format!(
            "the helper's parent (pid {ppid}) exited while it was measured"
        ));
    }
    if uids.len() != 4 || uids.iter().any(|u| *u != fabric_uid) {
        return Err(format!(
            "the helper's parent (pid {ppid}) runs as uids {uids:?}, not the Fabric uid \
             {fabric_uid}: the running Fabric is what the observation measures"
        ));
    }
    let mut h = Sha256::new();
    std::io::copy(&mut exe, &mut h)
        .map_err(|e| format!("the helper's parent's executable: {e}"))?;
    Ok(format!("{:x}", h.finalize()))
}

/// Everything verified and staged; nothing launched yet.
struct Prepared {
    req: LaunchRequest,
    launcher: sealed_exec::Verified,
    interpreter: sealed_exec::Verified,
    manifest: sealed_exec::Verified,
    staging: PathBuf,
    out: OwnedFd,
    out_name: OsString,
    root: OwnedFd,
}

fn prepare(c: &HelperConfig, a: &Authority, request: &[u8]) -> Result<Prepared, String> {
    let req = parse_request(request)?;
    let plan = validate_request(&req, c)?;
    let pin = |p: &PinnedJson| Pinned {
        path: p.path.clone(),
        sha256: p.sha256.clone(),
    };
    let owner = Some(a.operator_uid);
    let launcher = sealed_exec::open_verified(&pin(&c.launcher), owner, a.lease())?;
    let interpreter = sealed_exec::open_verified(&pin(&c.interpreter), owner, a.lease())?;
    let manifest = verify_inputs(c, a)?;
    let root = open_out_root(c, a)?;
    let staging = new_staging(c, a, &req.id)?;
    let staged = snapshot_inputs(root.as_raw_fd(), &plan.inputs_name, &staging, c)
        .and_then(|()| observed_launch(c, a, &req, &staging))
        .and_then(|()| make_out(root.as_raw_fd(), &plan.out_name));
    match staged {
        Ok(out) => Ok(Prepared {
            req,
            launcher,
            interpreter,
            manifest,
            staging,
            out,
            out_name: plan.out_name,
            root,
        }),
        Err(why) => {
            let _ = std::fs::remove_dir_all(&staging);
            Err(why)
        }
    }
}

/// Amendment 50 (negative matrix A84): no root launch without its ONE
/// observation. Over the root-private SNAPSHOT (never Fabric's dir):
/// 1. the snapshot's `job/launch-manifest.json` is the manifest the request
///    names (`psv_manifest_sha256`), which the launcher is told and the verdict
///    binds; and the snapshot's `policy.json`, the policy the launcher boots,
///    is the one that manifest names and states an effect ceiling (PSV-6, C9
///    round 4; A87: it was the request's `policy_json`, joined to nothing);
/// 2. the observation verifies under the operator's observer root, by the
///    SAME function Fabric's early check uses ([`crate::observer::verify_observation`]),
///    and joins that manifest field for field;
/// 3. the manifest's nonce is spent through the CUSTODIAN, as root: one
///    nonce, one launch. A dev custodian's spend is refused.
fn observed_launch(
    c: &HelperConfig,
    a: &Authority,
    req: &LaunchRequest,
    staging: &Path,
) -> Result<(), String> {
    let m = snapshot_manifest(staging, req)?;
    policy_at_root(staging, &m)?;
    let epoch = verify_observation_at_root(c, a, req, &m)?;
    spend_at_root(c, a, &m.observation_nonce, epoch, &req.psv_manifest_sha256)
}

/// The snapshot's launch manifest, which must be the one the request names.
fn snapshot_manifest(
    staging: &Path,
    req: &LaunchRequest,
) -> Result<axon_psv::LaunchManifest, String> {
    let bytes = std::fs::read(staging.join("job").join("launch-manifest.json"))
        .map_err(|e| format!("the snapshot has no launch-manifest.json: {e}"))?;
    let digest = crate::backend::sha256_hex(&bytes);
    if digest != req.psv_manifest_sha256 {
        return Err(format!(
            "the snapshot's launch-manifest.json has sha256 {digest}, not the request's \
             psv_manifest_sha256 {}",
            req.psv_manifest_sha256
        ));
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("launch-manifest.json: {e}"))
}

/// The snapshot's policy is the launch manifest's (whose digest is the
/// request's, and which the observation then joins field for field, its
/// `policy_sha256` included), and it states an effect ceiling
/// ([`axon_psv::protected_policy_ceiling`]). Refused BEFORE the nonce is spent.
fn policy_at_root(staging: &Path, m: &axon_psv::LaunchManifest) -> Result<(), String> {
    let bytes = std::fs::read(staging.join("policy.json"))
        .map_err(|e| format!("the snapshot has no policy.json: {e}"))?;
    axon_psv::protected_policy_ceiling(&bytes, m)
        .map(|_| ())
        .map_err(|e| format!("no launch under this policy: {e}"))
}

/// The observation, verified under the operator's observer root and joined to
/// `m` (whose digest is the request's). Returns its epoch.
fn verify_observation_at_root(
    c: &HelperConfig,
    a: &Authority,
    req: &LaunchRequest,
    m: &axon_psv::LaunchManifest,
) -> Result<u64, String> {
    use crate::backend::TrustAuthority;
    let rules = crate::observer::ObservationRules {
        trust: crate::observer::ObserverTrust {
            dir: c.observer.root.clone(),
            operator_owned: !a.test,
            separate_from: crate::backend::sibling_roots(
                TrustAuthority::Observer,
                &c.observer.root,
            ),
            host_signer_public_key: Some(c.observer.host_signer_public_key.clone()),
        },
        max_age_s: c.observer.max_age_s,
        clock: crate::backend::Clock::System,
    };
    let o = crate::observer::verify_observation(
        &rules,
        req.observation.as_bytes(),
        &req.observation_signature,
        m,
        &req.psv_manifest_sha256,
    )
    .map_err(|e| format!("no verified observation: {e}"))?;
    // "Of this epoch" at the root boundary: the observation's epoch is the
    // authority epoch the launch manifest names (`/2`, loop workstream), and
    // the custodian then holds it to the epoch the nonce was issued for.
    if o.epoch != m.authority.epoch {
        return Err(format!(
            "no verified observation: it is for epoch {}, but the launch manifest names \
             authority epoch {}",
            o.epoch, m.authority.epoch
        ));
    }
    Ok(o.epoch)
}

/// Spend `nonce` through the operator's custodian, as root.
fn spend_at_root(
    c: &HelperConfig,
    a: &Authority,
    nonce: &str,
    epoch: u64,
    manifest_sha256: &str,
) -> Result<(), String> {
    let mode = c
        .custodian
        .spend(nonce, epoch, manifest_sha256)
        .map_err(|e| format!("the observation's nonce was not spent: {e}"))?;
    if !custodian_mode_launches(mode, a.test) {
        return Err(format!(
            "the custodian that spent the nonce is a {} custodian: never a protected launch",
            mode.as_str()
        ));
    }
    Ok(())
}

/// Whose spend authorizes a root launch: a protected custodian's, or (in a
/// test-trust helper only) a test custodian's. Never a dev custodian's (D6).
pub fn custodian_mode_launches(mode: crate::custodian::Mode, test_helper: bool) -> bool {
    use crate::custodian::Mode;
    matches!(
        (mode, test_helper),
        (Mode::Protected, _) | (Mode::Test, true)
    )
}

fn run(c: &HelperConfig, p: Prepared) -> (LaunchReport, i32) {
    let fd_path = |fd: RawFd| OsString::from(format!("/dev/fd/{fd}"));
    let out = fd_path(p.out.as_raw_fd());
    let s = |x: &Path| x.as_os_str().to_os_string();
    let args: Vec<OsString> = vec![
        "--policy".into(),
        s(&p.staging.join("policy.json")),
        "--psv-candidate".into(),
        s(&p.staging.join("candidate")),
        "--psv-suite".into(),
        s(&p.staging.join("check")),
        "--psv-job".into(),
        s(&p.staging.join("job")),
        "--psv-manifest-sha".into(),
        p.req.psv_manifest_sha256.clone().into(),
        "--out".into(),
        out.clone(),
        "--manifest".into(),
        fd_path(p.manifest.fd()),
        "--artifacts-dir".into(),
        s(&c.artifacts_dir),
        "--fc-bin".into(),
        s(&c.firecracker),
        "--jailer-bin".into(),
        s(&c.jailer),
        "--timeout-s".into(),
        p.req.timeout_s.to_string().into(),
        "--id".into(),
        p.req.id.clone().into(),
    ];
    let keep = [p.out.as_raw_fd(), p.manifest.fd()];
    let mut report = LaunchReport {
        schema: REPORT_SCHEMA.into(),
        build: build_name().into(),
        launched: false,
        error: None,
        launcher_sha256: Some(p.launcher.sha256().to_string()),
        interpreter_sha256: Some(p.interpreter.sha256().to_string()),
        launcher_exit: None,
        verify_exit: None,
        unchanged: false,
    };
    let quiet = |mut cmd: std::process::Command| {
        cmd.stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .current_dir("/")
            .status()
    };
    let env = [("PATH", PATH_ENV)];
    let cmd = match sealed_exec::command(&p.launcher, Some(&p.interpreter), &args, &env, &keep) {
        Ok(cmd) => cmd,
        Err(why) => {
            let _ = std::fs::remove_dir_all(&p.staging);
            // Nothing ran: the (empty) out dir goes too.
            if let Ok(n) = cstr(&p.out_name) {
                // SAFETY: rmdir relative to the verified out root.
                unsafe { libc::unlinkat(p.root.as_raw_fd(), n.as_ptr(), libc::AT_REMOVEDIR) };
            }
            report.error = Some(why);
            return (report, EXIT_REFUSED);
        }
    };
    report.launched = true;
    report.launcher_exit = exit_code(quiet(cmd));
    // The snapshot (and its copy of the secret) goes before any further child.
    let _ = std::fs::remove_dir_all(&p.staging);
    report.verify_exit = sealed_exec::command(
        &p.launcher,
        Some(&p.interpreter),
        &["--verify-result".into(), out],
        &env,
        &[p.out.as_raw_fd()],
    )
    .ok()
    .and_then(|cmd| exit_code(quiet(cmd)));
    report.unchanged = p.launcher.unchanged().is_ok() && p.interpreter.unchanged().is_ok();
    if let Err(why) = hand_over(p.out.as_raw_fd(), c.fabric_uid).and_then(|()| {
        // SAFETY: the out dir itself, by descriptor.
        let ok = unsafe {
            libc::fchmod(p.out.as_raw_fd(), 0o700) == 0
                && libc::fchown(p.out.as_raw_fd(), c.fabric_uid, u32::MAX) == 0
        };
        ok.then_some(())
            .ok_or_else(|| std::io::Error::last_os_error().to_string())
    }) {
        report.error = Some(format!("the out dir could not be handed over: {why}"));
        return (report, EXIT_UNKNOWN);
    }
    (report, EXIT_LAUNCHED)
}

/// `--probe`: what the kernel made of this exec (the trust preflight runs it
/// as each actor). It reads nothing and launches nothing. `no_new_privs`:
/// the caller's NoNewPrivileges, which makes the kernel ignore the set-id
/// bit; `cgroup`: the cgroup the helper (and so the launcher it runs) is in,
/// which is the caller's (amendment 65).
pub fn probe(ruid: u32, euid: u32) -> String {
    let cgroup = std::fs::read_to_string("/proc/self/cgroup")
        .map(|s| s.trim().to_string())
        .unwrap_or_default();
    serde_json::json!({"schema": PROBE_SCHEMA, "build": build_name(), "ruid": ruid, "euid": euid,
                       "no_new_privs": no_new_privs(), "cgroup": cgroup})
    .to_string()
}

/// Whether this process runs with NoNewPrivileges (`PR_GET_NO_NEW_PRIVS`).
pub fn no_new_privs() -> bool {
    // SAFETY: a query with no pointer arguments.
    unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) == 1 }
}

/// Amendment 65: the helper's own executable (by `/proc/self/exe`, the file
/// the kernel executed) is setuid-root, so the kernel granted euid 0 unless
/// it ignored the bit. Not granted: refused, saying why. A helper not
/// installed setuid (a test-trust build run as its own uid) passes: whether
/// that may launch is the caller's rule (`euid != 0 && !test`).
pub fn setuid_honoured(euid: u32) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    if euid == 0 {
        return Ok(());
    }
    let m = std::fs::metadata("/proc/self/exe")
        .map_err(|e| format!("the helper cannot stat its own executable: {e}"))?;
    if m.uid() != 0 || m.mode() & libc::S_ISUID == 0 {
        return Ok(());
    }
    Err(if no_new_privs() {
        format!(
            "the helper is installed setuid-root but runs with effective uid {euid}: its caller \
             runs with NoNewPrivileges (systemd NoNewPrivileges=yes or a hardening preset, \
             setpriv --no-new-privs, a container's no-new-privileges), so the kernel ignored \
             the set-id bit. The Fabric service must not run with NoNewPrivileges (amendment 65)"
        )
    } else {
        format!(
            "the helper is installed setuid-root but runs with effective uid {euid}: the kernel \
             ignored the set-id bit (a nosuid mount, or a user namespace)"
        )
    })
}

const PR_SET_TIMERSLACK: libc::c_int = 29;

/// Set by `harden()` when `setsid()` failed (the helper is a process-group
/// leader): it is still in its caller's session.
static SESSION_NOT_LEFT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Amendment 73: the helper left its caller's session (and so its controlling
/// terminal) in `harden()`, or it refuses to launch. Checked next to
/// `setuid_honoured`, so `harden()`'s signature is unchanged.
pub fn session_left() -> Result<(), String> {
    if SESSION_NOT_LEFT.load(std::sync::atomic::Ordering::SeqCst) {
        return Err(
            "the helper could not leave its caller's session (setsid failed: it was started as a \
             process-group leader), so a terminal its caller controls could still signal it; \
             start it as an ordinary child (no job control, no setpgid)"
                .into(),
        );
    }
    Ok(())
}

/// Reset everything a setuid program inherits from its caller that could
/// steer it or its children: signal dispositions and mask, umask, working
/// directory, every descriptor above stderr, the environment, resource limits.
/// Called first, before the config or the request is read.
///
/// Amendment 73: the list is derived from the attributes execve(2),
/// credentials(7) and prctl(2) say a set-id exec PRESERVES, each recorded as
/// reset here, harmless, or covered elsewhere (the amendment's table).
pub fn harden() {
    // SAFETY: plain libc calls on process state, single-threaded at startup.
    unsafe {
        // Interval timers survive the exec (POSIX timers do not): a caller's
        // armed timer would deliver its signal to the root helper.
        let off = libc::itimerval {
            it_interval: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
            it_value: libc::timeval {
                tv_sec: 0,
                tv_usec: 0,
            },
        };
        libc::setitimer(libc::ITIMER_REAL, &off, std::ptr::null_mut());
        libc::setitimer(libc::ITIMER_VIRTUAL, &off, std::ptr::null_mut());
        libc::setitimer(libc::ITIMER_PROF, &off, std::ptr::null_mut());
        // A new session: no controlling terminal, so a terminal its caller
        // owns cannot write ^C/^\ into the helper's process group. It fails
        // for a process-group leader; that is recorded and refused by
        // `session_left` (the launch does not run in its caller's session).
        if libc::setsid() < 0 {
            SESSION_NOT_LEFT.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        for sig in 1..=libc::SIGRTMAX() {
            if sig != libc::SIGKILL && sig != libc::SIGSTOP {
                libc::signal(sig, libc::SIG_DFL);
            }
        }
        // A closed report pipe must not kill the helper mid-cleanup.
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
        let mut set: libc::sigset_t = std::mem::zeroed();
        libc::sigemptyset(&mut set);
        libc::sigprocmask(libc::SIG_SETMASK, &set, std::ptr::null_mut());
        // The launcher's own umask, as it always ran with. What it creates
        // sits in the root-private out dir and staging dir (0700) until the
        // hand-over, and the snapshot's modes are set explicitly.
        libc::umask(0o022);
        libc::chdir(c"/".as_ptr());
        libc::syscall(libc::SYS_close_range, 3u32, u32::MAX, 0u32);
        libc::prctl(
            libc::PR_SET_DUMPABLE,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
        let lim = |r, v| {
            let l = libc::rlimit {
                rlim_cur: v,
                rlim_max: v,
            };
            if libc::setrlimit(r, &l) != 0 {
                // Not root (a test-trust helper run unprivileged): the soft
                // limit up to the hard one is all it may do.
                let mut cur: libc::rlimit = std::mem::zeroed();
                if libc::getrlimit(r, &mut cur) == 0 {
                    cur.rlim_cur = if v == 0 { 0 } else { cur.rlim_max };
                    libc::setrlimit(r, &cur);
                }
            }
        };
        lim(libc::RLIMIT_CORE, 0);
        for r in [
            libc::RLIMIT_CPU,
            libc::RLIMIT_FSIZE,
            libc::RLIMIT_DATA,
            libc::RLIMIT_AS,
            libc::RLIMIT_NPROC,
        ] {
            lim(r, libc::RLIM_INFINITY);
        }
        lim(libc::RLIMIT_NOFILE, 65536);
        // The rest of the sixteen (amendment 73): the kernel's defaults.
        let lim2 = |r, soft: libc::rlim_t, hard: libc::rlim_t| {
            let l = libc::rlimit {
                rlim_cur: soft,
                rlim_max: hard,
            };
            if libc::setrlimit(r, &l) != 0 {
                let mut cur: libc::rlimit = std::mem::zeroed();
                if libc::getrlimit(r, &mut cur) == 0 {
                    cur.rlim_cur = soft.min(cur.rlim_max);
                    libc::setrlimit(r, &cur);
                }
            }
        };
        lim2(libc::RLIMIT_STACK, 8 << 20, libc::RLIM_INFINITY);
        lim2(libc::RLIMIT_RSS, libc::RLIM_INFINITY, libc::RLIM_INFINITY);
        lim2(libc::RLIMIT_MEMLOCK, 8 << 20, 8 << 20);
        lim2(libc::RLIMIT_LOCKS, libc::RLIM_INFINITY, libc::RLIM_INFINITY);
        lim2(
            libc::RLIMIT_SIGPENDING,
            libc::RLIM_INFINITY,
            libc::RLIM_INFINITY,
        );
        lim2(libc::RLIMIT_MSGQUEUE, 819200, 819200);
        lim2(libc::RLIMIT_NICE, 0, 0);
        lim2(libc::RLIMIT_RTPRIO, 0, 0);
        lim2(
            libc::RLIMIT_RTTIME,
            libc::RLIM_INFINITY,
            libc::RLIM_INFINITY,
        );
        // Scheduling and accounting attributes a fork and an exec keep.
        libc::setpriority(libc::PRIO_PROCESS, 0, 0);
        libc::syscall(
            libc::SYS_ioprio_set,
            1 as libc::c_long,
            0 as libc::c_long,
            0 as libc::c_long,
        );
        let sp = libc::sched_param { sched_priority: 0 };
        libc::sched_setscheduler(0, libc::SCHED_OTHER, &sp);
        let mut all: libc::cpu_set_t = std::mem::zeroed();
        for cpu in 0..libc::CPU_SETSIZE as usize {
            libc::CPU_SET(cpu, &mut all);
        }
        libc::sched_setaffinity(0, std::mem::size_of::<libc::cpu_set_t>(), &all);
        let _ = std::fs::write("/proc/self/oom_score_adj", "0");
        libc::prctl(
            PR_SET_TIMERSLACK,
            50_000 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
        libc::personality(0);
        libc::prctl(
            libc::PR_SET_CHILD_SUBREAPER,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
            0 as libc::c_ulong,
        );
    }
    let keys: Vec<OsString> = std::env::vars_os().map(|(k, _)| k).collect();
    for k in keys {
        std::env::remove_var(k);
    }
}

/// Become root in every id (real, effective, saved; groups dropped), so the
/// caller can no longer signal or trace the helper and the launcher's
/// children run as root throughout. Only when the effective uid is 0.
pub fn become_root() -> Result<(), String> {
    // SAFETY: id changes with constant arguments.
    unsafe {
        if libc::geteuid() != 0 {
            return Ok(());
        }
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setresgid(0, 0, 0) != 0
            || libc::setresuid(0, 0, 0) != 0
        {
            return Err(format!(
                "could not become root: {}",
                std::io::Error::last_os_error()
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cfg() -> HelperConfig {
        let p = |s: &str| PinnedJson {
            path: PathBuf::from(s),
            sha256: "a".repeat(64),
        };
        HelperConfig {
            schema: CONFIG_SCHEMA.into(),
            fabric_uid: 991,
            private_reply_channel: false,
            fabric: FabricPin {
                path: "/usr/local/bin/axon-fabric".into(),
                sha256: "f".repeat(64),
                revision: "0".repeat(40),
            },
            interpreter: p("/usr/bin/bash"),
            launcher: p("/opt/axon/fc_linux_profile.sh"),
            profile_manifest: p("/opt/axon/manifest.json"),
            artifacts_dir: "/opt/axon/guest".into(),
            firecracker: "/usr/local/bin/firecracker".into(),
            jailer: "/usr/local/bin/jailer".into(),
            out_root: "/var/lib/axon-fabric/runs".into(),
            staging_root: "/var/lib/axon-protected-launcher".into(),
            max_timeout_s: 600,
            max_input_bytes: 1 << 30,
            observer: HelperObserver {
                root: "/etc/axon/trust/observer".into(),
                max_age_s: 300,
                host_signer_public_key: "c".repeat(64),
                service: None,
            },
            custodian: crate::custodian::CustodianRef {
                socket: "/run/axon-custodian/custodian.sock".into(),
                uid: 993,
                sha256: Some("d".repeat(64)),
            },
        }
    }

    fn req() -> LaunchRequest {
        let r = "/var/lib/axon-fabric/runs";
        LaunchRequest {
            schema: REQUEST_SCHEMA.into(),
            id: "op-1".into(),
            out: format!("{r}/op-1").into(),
            psv_candidate: format!("{r}/op-1.psv-inputs/candidate").into(),
            psv_suite: format!("{r}/op-1.psv-inputs/check").into(),
            psv_job: format!("{r}/op-1.psv-inputs/job").into(),
            psv_manifest_sha256: "b".repeat(64),
            timeout_s: 60,
            observation: "{}".into(),
            observation_signature: "{}".into(),
        }
    }

    /// A: every request path must be a plainly spelled child of the
    /// operator's out root, in Fabric's fixed shape.
    #[test]
    fn a_request_naming_a_path_outside_the_operator_roots_is_refused() {
        let c = cfg();
        validate_request(&req(), &c).expect("control: Fabric's own shape");
        type Edit = fn(&mut LaunchRequest);
        let cases: [(&str, Edit); 9] = [
            ("out elsewhere", |r| r.out = "/etc/axon/x".into()),
            ("out traversal", |r| {
                r.out = "/var/lib/axon-fabric/runs/../../../etc".into()
            }),
            ("out dot", |r| {
                r.out = "/var/lib/axon-fabric/runs/./op".into()
            }),
            ("out nested", |r| {
                r.out = "/var/lib/axon-fabric/runs/a/b".into()
            }),
            ("out is the root", |r| {
                r.out = "/var/lib/axon-fabric/runs".into()
            }),
            ("candidate elsewhere", |r| {
                r.psv_candidate = "/root/candidate".into()
            }),
            ("suite traversal", |r| {
                r.psv_suite = "/var/lib/axon-fabric/runs/../../etc/check".into()
            }),
            ("job from another inputs dir", |r| {
                r.psv_job = "/var/lib/axon-fabric/runs/other.psv-inputs/job".into()
            }),
            ("suite under another leaf", |r| {
                r.psv_suite = "/var/lib/axon-fabric/runs/op-1.psv-inputs/shadow".into()
            }),
        ];
        for (what, edit) in cases {
            let mut r = req();
            edit(&mut r);
            let got = validate_request(&r, &c);
            assert!(
                got.is_err(),
                "ATTACK: a request naming a path outside the operator roots ({what}) was \
                 accepted: {got:?}"
            );
        }
    }

    /// D6 (amendment 50): a production helper launches only on a PROTECTED
    /// custodian's spend; a test custodian's counts only inside a test-trust
    /// helper, and a dev custodian's never.
    #[test]
    fn only_a_protected_custodian_authorizes_a_production_launch() {
        use crate::custodian::Mode;
        assert!(custodian_mode_launches(Mode::Protected, false), "control");
        assert!(custodian_mode_launches(Mode::Test, true), "control: a test");
        assert!(
            !custodian_mode_launches(Mode::Test, false),
            "ATTACK: a production helper accepted a test custodian's spend"
        );
        for test in [false, true] {
            assert!(
                !custodian_mode_launches(Mode::Dev, test),
                "ATTACK: a dev custodian's spend authorized a launch (test helper: {test})"
            );
        }
    }

    /// A: the request schema is fixed; an unknown field is refused, never
    /// ignored (it could be read as a knob the helper does not have).
    #[test]
    fn a_request_with_an_unknown_field_is_refused() {
        let good = serde_json::to_value(req()).unwrap();
        parse_request(good.to_string().as_bytes()).expect("control");
        for (k, v) in [
            ("launcher", serde_json::json!("/tmp/evil.sh")),
            ("manifest", serde_json::json!("/tmp/m.json")),
            ("fc_bin", serde_json::json!("/tmp/firecracker")),
            ("env", serde_json::json!({"LD_PRELOAD": "/tmp/x.so"})),
        ] {
            let mut bad = good.clone();
            bad[k] = v;
            let got = parse_request(bad.to_string().as_bytes());
            assert!(
                got.is_err(),
                "ATTACK: a request with an unknown field `{k}` was accepted: {got:?}"
            );
        }
        // A duplicated key is a second reading of one field.
        let dup = good.to_string().replacen('{', "{\"id\":\"x\",", 1);
        assert!(parse_request(dup.as_bytes()).is_err(), "duplicate key");
    }

    /// A: Fabric runs non-root; a helper config admitting uid 0 as the
    /// Fabric is refused (in production: a test config may run as root).
    #[test]
    fn a_helper_config_admitting_root_as_the_fabric_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("protected-launcher.json");
        let mut v = serde_json::json!({
            "schema": CONFIG_SCHEMA, "fabric_uid": 0,
            "fabric": {"path": "/usr/local/bin/axon-fabric", "sha256": "f".repeat(64),
                       "revision": "0".repeat(40)},
            "interpreter": {"path": "/bin/bash", "sha256": "a".repeat(64)},
            "launcher": {"path": "/opt/l.sh", "sha256": "a".repeat(64)},
            "profile_manifest": {"path": "/opt/m.json", "sha256": "a".repeat(64)},
            "artifacts_dir": "/opt/dist", "firecracker": "/opt/firecracker",
            "jailer": "/opt/jailer", "out_root": "/var/runs", "staging_root": "/var/st",
            "max_timeout_s": 60, "max_input_bytes": 1,
            "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                         "host_signer_public_key": "c".repeat(64)},
            "custodian": {"socket": "/run/axon-custodian/custodian.sock", "uid": 993,
                          "sha256": "d".repeat(64)},
        });
        let a = Authority {
            // SAFETY: geteuid cannot fail.
            operator_uid: unsafe { libc::geteuid() },
            walk_base: d.path().to_path_buf(),
            test: false,
        };
        std::fs::write(&p, v.to_string()).unwrap();
        let got = load_config(&p, &a);
        assert!(
            got.is_err(),
            "ATTACK: a helper config admitting root (uid 0) as the Fabric uid was accepted"
        );
        v["fabric_uid"] = serde_json::json!(991);
        std::fs::write(&p, v.to_string()).unwrap();
        load_config(&p, &a).expect("control: a service uid");
        // A83 (amendment 50): the custodian the helper spends through is not
        // the Fabric (nor root).
        for uid in [991, 0] {
            v["custodian"]["uid"] = serde_json::json!(uid);
            std::fs::write(&p, v.to_string()).unwrap();
            let got = load_config(&p, &a);
            assert!(
                got.is_err(),
                "ATTACK: a helper config spending through a custodian of uid {uid} (the Fabric's \
                 or root) was accepted"
            );
        }
    }

    /// A94 (amendment 68): the observer service the helper relays from is
    /// its own uid. A production helper config naming the Fabric uid (whose
    /// observer key the Fabric could then read) or root as the observer is
    /// refused. Control: a third uid.
    #[test]
    fn a_helper_config_whose_observer_service_is_the_fabric_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("protected-launcher.json");
        let mut v = serde_json::json!({
            "schema": CONFIG_SCHEMA, "fabric_uid": 991,
            "fabric": {"path": "/usr/local/bin/axon-fabric", "sha256": "f".repeat(64),
                       "revision": "0".repeat(40)},
            "interpreter": {"path": "/bin/bash", "sha256": "a".repeat(64)},
            "launcher": {"path": "/opt/l.sh", "sha256": "a".repeat(64)},
            "profile_manifest": {"path": "/opt/m.json", "sha256": "a".repeat(64)},
            "artifacts_dir": "/opt/dist", "firecracker": "/opt/firecracker",
            "jailer": "/opt/jailer", "out_root": "/var/runs", "staging_root": "/var/st",
            "max_timeout_s": 60, "max_input_bytes": 1,
            "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                         "host_signer_public_key": "c".repeat(64),
                         "service": {"socket": "/run/axon-observer/observer.sock", "uid": 994,
                                     "sha256": "e".repeat(64)}},
            "custodian": {"socket": "/run/axon-custodian/custodian.sock", "uid": 993,
                          "sha256": "d".repeat(64)},
        });
        let a = Authority {
            // SAFETY: geteuid cannot fail.
            operator_uid: unsafe { libc::geteuid() },
            walk_base: d.path().to_path_buf(),
            test: false,
        };
        std::fs::write(&p, v.to_string()).unwrap();
        load_config(&p, &a).expect("control: an observer of its own uid");
        for uid in [991, 0] {
            v["observer"]["service"]["uid"] = serde_json::json!(uid);
            std::fs::write(&p, v.to_string()).unwrap();
            let got = load_config(&p, &a);
            assert!(
                got.is_err(),
                "ATTACK: a production helper config relaying from an observer service of uid \
                 {uid} (the Fabric's or root) was accepted"
            );
        }
    }

    /// The production-shaped helper config the pin tests edit.
    fn prod_config(d: &Path) -> (PathBuf, serde_json::Value, Authority) {
        let p = d.join("protected-launcher.json");
        let v = serde_json::json!({
            "schema": CONFIG_SCHEMA, "fabric_uid": 991,
            "fabric": {"path": "/usr/local/bin/axon-fabric", "sha256": "f".repeat(64),
                       "revision": "0".repeat(40)},
            "interpreter": {"path": "/bin/bash", "sha256": "a".repeat(64)},
            "launcher": {"path": "/opt/l.sh", "sha256": "a".repeat(64)},
            "profile_manifest": {"path": "/opt/m.json", "sha256": "a".repeat(64)},
            "artifacts_dir": "/opt/dist", "firecracker": "/opt/firecracker",
            "jailer": "/opt/jailer", "out_root": "/var/runs", "staging_root": "/var/st",
            "max_timeout_s": 60, "max_input_bytes": 1,
            "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                         "host_signer_public_key": "c".repeat(64),
                         "service": {"socket": "/run/axon-observer/observer.sock", "uid": 994,
                                     "sha256": "e".repeat(64)}},
            "custodian": {"socket": "/run/axon-custodian/custodian.sock", "uid": 993,
                          "sha256": "d".repeat(64)},
        });
        let a = Authority {
            // SAFETY: geteuid cannot fail.
            operator_uid: unsafe { libc::geteuid() },
            walk_base: d.to_path_buf(),
            test: false,
        };
        std::fs::write(&p, v.to_string()).unwrap();
        (p, v, a)
    }

    /// A132 (amendments 79, 85): the helper serves a caller whose executable at
    /// that instant is the file the operator pinned (a guard against mistakes,
    /// not against same-uid code), so the config must pin it: a lowercase sha256 and the
    /// pinned binary's 40-hex build revision. A config with no pin, a pin that
    /// is not a digest, or a revision that is not a commit is refused.
    /// Control: the pinned config loads.
    #[test]
    fn a_helper_config_must_pin_the_fabric_program() {
        let d = tempfile::tempdir().unwrap();
        let (p, v, a) = prod_config(d.path());
        load_config(&p, &a).expect("control: a pinned Fabric program");
        let mut no_pin = v.clone();
        no_pin.as_object_mut().unwrap().remove("fabric");
        std::fs::write(&p, no_pin.to_string()).unwrap();
        assert!(
            load_config(&p, &a).is_err(),
            "ATTACK: a helper config with no Fabric program pin was accepted: any Fabric-uid \
             program is then the running Fabric"
        );
        for bad in ["", "F".repeat(64).as_str(), "abc", "g".repeat(64).as_str()] {
            let mut w = v.clone();
            w["fabric"]["sha256"] = serde_json::json!(bad);
            std::fs::write(&p, w.to_string()).unwrap();
            assert!(
                load_config(&p, &a).is_err(),
                "ATTACK: a helper config whose Fabric pin is {bad:?} was accepted"
            );
        }
        for bad in [
            "unknown",
            "",
            "rev",
            &"A".repeat(40),
            &"a".repeat(39),
            &"a".repeat(41),
        ] {
            let mut w = v.clone();
            w["fabric"]["revision"] = serde_json::json!(bad);
            std::fs::write(&p, w.to_string()).unwrap();
            assert!(
                load_config(&p, &a).is_err(),
                "ATTACK: a production helper config whose Fabric revision is {bad:?} was accepted"
            );
        }
    }

    /// A134 (amendment 79): the observer service and the custodian are two
    /// principals. A production helper config naming one uid for both is
    /// refused (the kit used five user NAMES; the code did not re-check the
    /// uids). Control: distinct uids.
    #[test]
    fn a_helper_config_whose_observer_is_the_custodian_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let (p, mut v, a) = prod_config(d.path());
        load_config(&p, &a).expect("control: two uids");
        v["observer"]["service"]["uid"] = serde_json::json!(993);
        std::fs::write(&p, v.to_string()).unwrap();
        assert!(
            load_config(&p, &a).is_err(),
            "ATTACK: a production helper config whose observer service and custodian are one \
             uid (993) was accepted"
        );
    }

    /// A149 (M2050/M2051, amendment 85): the reply pipe has no holder but the helper
    /// and its parent; a production helper's stdout must be a pipe. ATTACKS: a
    /// third process holding the pipe (the exec-race worker); a production
    /// stdout that is a file or a terminal. Controls: parent and helper only.
    #[test]
    fn a_reply_pipe_another_process_holds_is_refused() {
        let (me, ppid) = (100, 50);
        channel_verdict(libc::S_IFIFO, &[50, 100], ppid, me, true)
            .expect("control: parent + helper");
        channel_verdict(libc::S_IFIFO, &[], ppid, me, true).expect("control: no other holder");
        let got = channel_verdict(libc::S_IFIFO, &[50, 100, 77], ppid, me, true);
        assert!(
            got.is_err(),
            "ATTACK: a reply pipe a third process (the exec-race worker) holds was served"
        );
        for mode in [libc::S_IFREG, libc::S_IFCHR] {
            assert!(
                channel_verdict(mode, &[], ppid, me, true).is_err(),
                "ATTACK: a production helper wrote its reply to a stdout that is not a pipe"
            );
            channel_verdict(mode, &[], ppid, me, false).expect("a test helper may use a file");
        }
    }

    /// A134 (M1871): the helper sizes what the observer must hash from the
    /// artifacts it pins: an image of 3 GiB (sparse here) counts as 3 GiB, not
    /// as nothing (which would hand the observer the base timeout only).
    #[test]
    fn the_helper_sizes_what_the_observer_must_hash() {
        let d = tempfile::tempdir().unwrap();
        for (n, len) in [("vmlinux", 1u64 << 30), ("rootfs.sqfs", 2u64 << 30)] {
            let f = std::fs::File::create(d.path().join(n)).unwrap();
            f.set_len(len).unwrap();
        }
        let mut c = cfg();
        c.artifacts_dir = d.path().to_path_buf();
        let got = artifact_bytes(&c);
        assert_eq!(
            got,
            3u64 << 30,
            "ATTACK: the helper sized a 3 GiB kernel and rootfs as {got} bytes"
        );
    }

    /// A134 (amendment 79): an observation's read timeout grows with what the
    /// observer must hash. A fixed bound burned the nonce of a launch whose
    /// rootfs took longer than it to stream.
    #[test]
    fn an_observation_waits_as_long_as_its_measurement_can_take() {
        use crate::observer_service::observe_timeout;
        let small = observe_timeout(0);
        let big = observe_timeout(8 << 30);
        assert!(
            big > small + std::time::Duration::from_secs(60),
            "ATTACK: an 8 GiB image is given {big:?}, no more than an empty one's {small:?}: a \
             rootfs that takes longer than the base bound to hash burns its nonce"
        );
    }

    #[test]
    fn a_request_with_a_bad_id_digest_or_timeout_is_refused() {
        let c = cfg();
        for edit in [
            (|r: &mut LaunchRequest| r.id = "a;rm -rf /".into()) as fn(&mut LaunchRequest),
            |r| r.id = String::new(),
            // The manifest digest's format rule is retired EQUIVALENT_DID
            // (M796, four-cell record on the helper route, C9 round 4 rows2):
            // a direct case here would fail its full-suite cell while proving
            // nothing the helper route does not.
            |r| r.timeout_s = 0,
            |r| r.timeout_s = 601,
            |r| r.schema = "axon-protected-launch-request/1".into(),
        ] {
            let mut r = req();
            edit(&mut r);
            assert!(validate_request(&r, &c).is_err(), "{r:?}");
        }
    }

    /// A116 (amendment 73): every resource limit the kernel lists in
    /// `/proc/self/limits` is one `harden()` resets. A kernel that adds a
    /// seventeenth row fails here, until someone decides what the helper's
    /// value is.
    #[test]
    fn every_resource_limit_the_kernel_lists_is_one_harden_resets() {
        const ROWS: [(&str, &str); 16] = [
            ("Max cpu time", "RLIMIT_CPU"),
            ("Max file size", "RLIMIT_FSIZE"),
            ("Max data size", "RLIMIT_DATA"),
            ("Max stack size", "RLIMIT_STACK"),
            ("Max core file size", "RLIMIT_CORE"),
            ("Max resident set", "RLIMIT_RSS"),
            ("Max processes", "RLIMIT_NPROC"),
            ("Max open files", "RLIMIT_NOFILE"),
            ("Max locked memory", "RLIMIT_MEMLOCK"),
            ("Max address space", "RLIMIT_AS"),
            ("Max file locks", "RLIMIT_LOCKS"),
            ("Max pending signals", "RLIMIT_SIGPENDING"),
            ("Max msgqueue size", "RLIMIT_MSGQUEUE"),
            ("Max nice priority", "RLIMIT_NICE"),
            ("Max realtime priority", "RLIMIT_RTPRIO"),
            ("Max realtime timeout", "RLIMIT_RTTIME"),
        ];
        let limits = std::fs::read_to_string("/proc/self/limits").expect("procfs");
        let mut listed: Vec<String> = limits
            .lines()
            .skip(1)
            .map(|l| l.split("  ").next().unwrap().trim().to_string())
            .collect();
        listed.sort();
        let mut known: Vec<String> = ROWS.iter().map(|r| r.0.to_string()).collect();
        known.sort();
        assert_eq!(
            listed, known,
            "the kernel lists a limit harden() has not decided"
        );
        let src = include_str!("privileged_launcher.rs");
        let body =
            &src[src.find("pub fn harden()").unwrap()..src.find("pub fn become_root").unwrap()];
        for (row, name) in ROWS {
            assert!(
                body.contains(&format!("libc::{name}")),
                "harden() never resets {name} ({row})"
            );
        }
    }

    // ── C9 round 4c, EQGATE (amendment 81; M1907-M1913): guards expressed as an
    // OPEN FLAG. The root helper's snapshot of its inputs and its out-dir
    // hand-over refuse a symlink / an existing file by the KERNEL (O_NOFOLLOW,
    // O_EXCL, AT_SYMLINK_NOFOLLOW), so the refusal builds no Err the gate could
    // see, and each flag removed alone left the root-run suite green. Each test
    // below makes the attack the flag defeats and calls the real function.

    fn euid_() -> u32 {
        // SAFETY: geteuid cannot fail.
        unsafe { libc::geteuid() }
    }

    fn dir_fd(p: &Path) -> OwnedFd {
        std::fs::File::open(p).unwrap().into()
    }

    /// The root helper's snapshot copies a tree it owns the right to read and
    /// nothing a symlink points at. A symlink among the inputs (here to a
    /// directory of the same owner) is refused, never followed.
    #[test]
    fn the_inputs_snapshot_never_follows_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let (src, outside, dst) = (
            t.path().join("src"),
            t.path().join("outside"),
            t.path().join("dst"),
        );
        for d in [&src, &outside, &dst] {
            std::fs::create_dir(d).unwrap();
        }
        std::fs::write(src.join("f.ax"), "fn f() {}\n").unwrap();
        std::fs::write(outside.join("secret"), "not an input\n").unwrap();
        let mut budget = 1 << 20;
        copy_tree(dir_fd(&src).as_raw_fd(), &dst, euid_(), &mut budget)
            .expect("control: a tree without a symlink is copied");
        let (src2, dst2) = (t.path().join("src2"), t.path().join("dst2"));
        std::fs::create_dir(&src2).unwrap();
        std::fs::create_dir(&dst2).unwrap();
        std::os::unix::fs::symlink(&outside, src2.join("link")).unwrap();
        let got = copy_tree(dir_fd(&src2).as_raw_fd(), &dst2, euid_(), &mut budget);
        assert!(
            matches!(&got, Err(e) if e.contains("is a symlink: an input is never followed")),
            "ATTACK: the root helper's input snapshot followed a symlink among the inputs: {got:?}"
        );
        assert!(
            !dst2.join("link").exists(),
            "ATTACK: the symlink's target was copied into the snapshot"
        );
    }

    /// Every file of the snapshot is CREATED: one already there (a leftover,
    /// a planted hard link) is refused, never written through.
    #[test]
    fn the_inputs_snapshot_never_writes_over_an_existing_file() {
        let t = tempfile::tempdir().unwrap();
        let (src, dst) = (t.path().join("src"), t.path().join("dst"));
        std::fs::create_dir(&src).unwrap();
        std::fs::create_dir(&dst).unwrap();
        std::fs::write(src.join("f.ax"), "fn f() {}\n").unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, "precious\n").unwrap();
        std::fs::hard_link(&victim, dst.join("f.ax")).unwrap();
        let mut budget = 1 << 20;
        let got = copy_tree(dir_fd(&src).as_raw_fd(), &dst, euid_(), &mut budget);
        assert!(
            got.is_err() && std::fs::read_to_string(&victim).unwrap() == "precious\n",
            "ATTACK: the snapshot wrote through a file that already existed at its destination: \
             {got:?}, victim now {:?}",
            std::fs::read_to_string(&victim)
        );
    }

    /// The fact the O_NOFOLLOW on that create is DOMINATED by (the exemption's
    /// anchor): O_CREAT|O_EXCL refuses a symlink at the destination whatever it
    /// points at (open(2)), so `create_new` alone already refuses.
    #[test]
    fn a_symlink_at_a_snapshot_destination_is_refused_by_create_new_alone() {
        let t = tempfile::tempdir().unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, "precious\n").unwrap();
        let link = t.path().join("dest");
        std::os::unix::fs::symlink(&victim, &link).unwrap();
        let got = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&link);
        assert!(
            got.is_err(),
            "O_EXCL no longer refuses a symlink: the exemption's fact is gone"
        );
        assert_eq!(std::fs::read_to_string(&victim).unwrap(), "precious\n");
    }

    /// The guest policy is read without following a symlink: a `policy.json`
    /// that is one (to a file of the Fabric uid) is refused.
    #[test]
    fn the_policy_snapshot_never_follows_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let (inputs, dst) = (t.path().join("inputs"), t.path().join("policy.copy"));
        std::fs::create_dir(&inputs).unwrap();
        std::fs::write(t.path().join("other.json"), "{}").unwrap();
        std::fs::write(inputs.join("policy.json"), "{}").unwrap();
        snapshot_policy(dir_fd(&inputs).as_raw_fd(), &dst, euid_())
            .expect("control: a regular policy.json is snapshotted");
        std::fs::remove_file(inputs.join("policy.json")).unwrap();
        std::os::unix::fs::symlink(t.path().join("other.json"), inputs.join("policy.json"))
            .unwrap();
        let got = snapshot_policy(dir_fd(&inputs).as_raw_fd(), &t.path().join("p2"), euid_());
        assert!(
            matches!(&got, Err(e) if e.contains("is a symlink: never followed")),
            "ATTACK: the root helper's policy snapshot followed a symlink: {got:?}"
        );
    }

    /// An operator file (the helper's config, the custodian's) is read without
    /// following a symlink at its leaf. Control: the real file reads.
    #[test]
    fn an_operator_file_that_is_a_symlink_is_never_read() {
        let t = tempfile::tempdir().unwrap();
        let a = Authority {
            operator_uid: euid_(),
            walk_base: t.path().to_path_buf(),
            test: true,
        };
        let d = t.path().join("d");
        std::fs::create_dir(&d).unwrap();
        std::fs::write(d.join("real"), "{}").unwrap();
        std::os::unix::fs::symlink(d.join("real"), d.join("cfg")).unwrap();
        read_operator_file(&d.join("real"), &a).expect("control: the real file reads");
        let got = read_operator_file(&d.join("cfg"), &a);
        assert!(
            got.is_err(),
            "ATTACK: the root helper read an operator file through a symlink: {got:?}"
        );
    }

    /// The ownership walk's BASE is opened without following a symlink too: a
    /// `walk_base` that is one is refused (its target would be a different
    /// directory from the one whose owner the walk checks).
    #[test]
    fn a_walk_base_that_is_a_symlink_is_never_followed() {
        let t = tempfile::tempdir().unwrap();
        let real = t.path().join("real");
        std::fs::create_dir_all(real.join("sub")).unwrap();
        let ok = Authority {
            operator_uid: euid_(),
            walk_base: real.clone(),
            test: true,
        };
        walk_open(&ok, &real.join("sub")).expect("control: a real base walks");
        let link = t.path().join("linked");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let a = Authority {
            operator_uid: euid_(),
            walk_base: link.clone(),
            test: true,
        };
        let got = walk_open(&a, &link.join("sub")).map(|_| ());
        assert!(
            got.is_err(),
            "ATTACK: the ownership walk started from a symlinked base: {got:?}"
        );
    }

    /// The out dir is handed over without following a symlink (ROOT: the
    /// hand-over is a chown, and following would chown the link's target, a
    /// file outside the out dir, to the Fabric uid). Control: the link itself
    /// changes hands and its target does not.
    #[test]
    fn the_hand_over_never_follows_a_symlink() {
        if euid_() != 0 {
            eprintln!("skipped: needs root to chown to another uid");
            return;
        }
        let t = tempfile::tempdir().unwrap();
        let out = t.path().join("out");
        std::fs::create_dir(&out).unwrap();
        let victim = t.path().join("victim");
        std::fs::write(&victim, "root's\n").unwrap();
        std::os::unix::fs::symlink(&victim, out.join("link")).unwrap();
        std::fs::write(out.join("result.json"), "{}").unwrap();
        let done = hand_over(dir_fd(&out).as_raw_fd(), 12345);
        assert!(
            done.is_ok(),
            "ATTACK: the out-dir hand-over refused a symlink it hands over (its fstatat followed \
             the link): {done:?}"
        );
        use std::os::unix::fs::MetadataExt;
        assert_eq!(
            std::fs::symlink_metadata(out.join("link")).unwrap().uid(),
            12345,
            "ATTACK: the hand-over chowned a symlink's target instead of the link itself"
        );
        assert_eq!(
            std::fs::metadata(&victim).unwrap().uid(),
            0,
            "ATTACK: the hand-over followed a symlink and gave a file outside the out dir to the \
             Fabric uid"
        );
    }

    /// C9 round 7, EQGATE3 (amendment 91): the root helper makes itself
    /// NON-DUMPABLE at start (`harden`), so no unprivileged process can read
    /// its memory or ptrace it. The call builds no `Err` and at the default
    /// `fs.suid_dumpable=0` a setuid exec is already non-dumpable, which is why
    /// no suite noticed it removed. `harden` runs in a FORKED child of this
    /// test (it chdirs, closes descriptors and leaves the session), which
    /// starts dumpable and reports what `PR_GET_DUMPABLE` says afterwards.
    #[test]
    fn harden_makes_the_helper_non_dumpable() {
        // SAFETY: fork; the child calls only libc process-state functions
        // (harden is plain libc calls) and `_exit`.
        let pid = unsafe { libc::fork() };
        if pid == 0 {
            // SAFETY: as above.
            let code = unsafe {
                libc::prctl(libc::PR_SET_DUMPABLE, 1 as libc::c_ulong, 0, 0, 0);
                if libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) != 1 {
                    libc::_exit(10);
                }
                harden();
                if libc::prctl(libc::PR_GET_DUMPABLE, 0, 0, 0, 0) == 0 {
                    0
                } else {
                    11
                }
            };
            unsafe { libc::_exit(code) };
        }
        assert!(pid > 0, "setup: fork");
        let mut status = 0;
        // SAFETY: waitpid on our own child.
        unsafe { libc::waitpid(pid, &mut status, 0) };
        assert!(libc::WIFEXITED(status), "setup: the child exited");
        let code = libc::WEXITSTATUS(status);
        assert_ne!(code, 10, "setup: the child did not start dumpable");
        assert_eq!(
            code, 0,
            "ATTACK: the root helper stayed dumpable after harden: its memory is readable and \
             ptrace-attachable by the caller's uid (PR_GET_DUMPABLE != 0)"
        );
    }
}
