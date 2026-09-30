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
//!   launch manifest digest, the guest policy bytes and a timeout. Nothing
//!   in it names an executable, a manifest, an artifact or a config;
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
/// `/2` (amendment 50): the request carries the observation.
pub const REQUEST_SCHEMA: &str = "axon-protected-launch-request/2";
pub const REPORT_SCHEMA: &str = "axon-protected-launch-report/1";
pub const PROBE_SCHEMA: &str = "axon-protected-launcher-probe/1";

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

/// `axon-protected-launcher/1`, the operator's.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HelperConfig {
    pub schema: String,
    /// The only real uid allowed to make a request. Never 0 in production.
    pub fabric_uid: u32,
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
}

/// `axon-protected-launch-request/1`: per-launch data only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct LaunchRequest {
    pub schema: String,
    /// The jail id (`^[a-zA-Z0-9-]{1,60}$`).
    pub id: String,
    /// `<out_root>/<name>`: created by the helper; must not exist.
    pub out: PathBuf,
    /// `<out_root>/<inputs>/candidate`, `…/check`, `…/job`.
    pub psv_candidate: PathBuf,
    pub psv_suite: PathBuf,
    pub psv_job: PathBuf,
    pub psv_manifest_sha256: String,
    /// The exact `axon-vm-mmds/1` bytes (the launcher validates them).
    pub policy_json: String,
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
    for p in [
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
    if r.policy_json.len() > MAX_POLICY {
        return Err("policy_json is too large".into());
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

/// Snapshot `<inputs>/{candidate,check,job}` into `staging`, then remove the
/// job's two files from Fabric's dir (the secret then exists once: in the
/// root-private snapshot, removed when the launcher returns).
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
    let c = match load_config(config, a) {
        Ok(c) => c,
        Err(why) => return (LaunchReport::refused(why), EXIT_REFUSED),
    };
    if caller_uid != c.fabric_uid {
        return (
            LaunchReport::refused(format!(
                "caller uid {caller_uid} is not the configured Fabric uid {}",
                c.fabric_uid
            )),
            EXIT_REFUSED,
        );
    }
    if let Err(why) = become_root() {
        return (LaunchReport::refused(why), EXIT_REFUSED);
    }
    match prepare(&c, a, request) {
        Err(why) => (LaunchReport::refused(why), EXIT_REFUSED),
        Ok(p) => run(&c, p),
    }
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
        .and_then(|()| {
            std::fs::write(staging.join("policy.json"), req.policy_json.as_bytes())
                .map_err(|e| format!("policy: {e}"))
        })
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
///    binds;
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
/// as each actor). It reads nothing and launches nothing.
pub fn probe(ruid: u32, euid: u32) -> String {
    serde_json::json!({"schema": PROBE_SCHEMA, "build": build_name(), "ruid": ruid, "euid": euid})
        .to_string()
}

/// Reset everything a setuid program inherits from its caller that could
/// steer it or its children: signal dispositions and mask, umask, working
/// directory, every descriptor above stderr, the environment, resource limits.
/// Called first, before the config or the request is read.
pub fn harden() {
    // SAFETY: plain libc calls on process state, single-threaded at startup.
    unsafe {
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
            },
            custodian: crate::custodian::CustodianRef {
                socket: "/run/axon-custodian/custodian.sock".into(),
                uid: 993,
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
            policy_json: "{}".into(),
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
            "interpreter": {"path": "/bin/bash", "sha256": "a".repeat(64)},
            "launcher": {"path": "/opt/l.sh", "sha256": "a".repeat(64)},
            "profile_manifest": {"path": "/opt/m.json", "sha256": "a".repeat(64)},
            "artifacts_dir": "/opt/dist", "firecracker": "/opt/firecracker",
            "jailer": "/opt/jailer", "out_root": "/var/runs", "staging_root": "/var/st",
            "max_timeout_s": 60, "max_input_bytes": 1,
            "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                         "host_signer_public_key": "c".repeat(64)},
            "custodian": {"socket": "/run/axon-custodian/custodian.sock", "uid": 993},
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

    #[test]
    fn a_request_with_a_bad_id_digest_or_timeout_is_refused() {
        let c = cfg();
        for edit in [
            (|r: &mut LaunchRequest| r.id = "a;rm -rf /".into()) as fn(&mut LaunchRequest),
            |r| r.id = String::new(),
            |r| r.psv_manifest_sha256 = "B".repeat(64),
            |r| r.timeout_s = 0,
            |r| r.timeout_s = 601,
            |r| r.schema = "axon-protected-launch-request/1".into(),
        ] {
            let mut r = req();
            edit(&mut r);
            assert!(validate_request(&r, &c).is_err(), "{r:?}");
        }
    }
}
