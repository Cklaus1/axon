//! Execution backends and their TRUTHFUL profiles.
//!
//! A backend is chosen by what it IS, never by what the request would like it
//! to be, and there is no fallback between them.
//!
//! | id | engine / enclosure / guest | eligible for |
//! |---|---|---|
//! | `process_scoped/local-interpreter` | registered interpreter as a child process, no hardware isolation | `registered_check`, `hardware_isolation=false`, `os=none` |
//! | `axon-metal-fc-nojailer` | `axon-vm` lib: Firecracker, NO jailer, custom Axon guest kernel (not Linux) | **nothing** — the guest kernel demonstrates the syscall gate and does not execute programs (K5 remaining work), so it can neither run a job nor produce a verdict |
//! | `linux-microvm-protected` | `scripts/fc_linux_profile.sh`: Firecracker under jailer, pinned Linux 6.1 guest, empty netns, host cgroups (B263) | `interpreter_run`, `hardware_isolation=true`, `os=linux` — and ONLY while its manifest sha256 equals the qualification evidence record's |
//!
//! Every Linux-profile launch carries the admitted grant's effect ceiling to
//! the guest as an `axon-vm-mmds/1` policy file (`--policy FILE`; the launcher
//! puts it on the kernel cmdline as `axon.policy=<base64>`, which
//! `axon-guest-init` reads). An empty ceiling is `allowed_effects: []` —
//! deny-all, never "unrestricted". A policy too large for the guest cmdline is
//! refused before anything is launched ([`GuestPolicy::for_grant`]).
//!
//! Limitations of the Linux profile that are REFUSALS here (B263 x1/x2):
//! delivering a policy is not the same as the qualification having SHOWN the
//! guest enforces it, so a request that needs an effect ceiling inside the
//! guest is eligible ONLY when the signed evidence records
//! `x1_guest_policy_channel` as PASS (a waived BLOCKED x1 does not count); and
//! the profile does not preserve path-scoped grants, so a path-scoped grant is
//! ineligible.

use std::path::{Path, PathBuf};

use axon_loop_contracts::{
    Architecture, CheckpointKind, ComputeRequest, Engine, JobKind, NetworkMode, Os,
};
use axon_os::Isolation;

/// A backend's self-description, mirroring `acf-backend-profile/1`'s fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Profile {
    /// Goes into `ExecutionReceipt.backend_profile_ref`.
    pub id: &'static str,
    pub engine: Engine,
    /// `acf-backend-profile/1` enclosure vocabulary.
    pub enclosure: &'static str,
    /// `acf-backend-profile/1` guest_kind vocabulary.
    pub guest_kind: &'static str,
    pub os: Os,
    pub hardware_isolation: bool,
    pub isolation: Isolation,
    /// Job kinds this backend can actually carry out.
    pub job_kinds: &'static [JobKind],
    /// Instruction-set architectures the executed program runs on.
    pub architectures: &'static [Architecture],
    /// Checkpoint kinds this backend can take. None of them checkpoints
    /// anything today, so each offers only `CheckpointKind::None`.
    pub checkpoint_kinds: &'static [CheckpointKind],
}

/// The architecture the host interpreter runs on: the one this Fabric was
/// built for. A host with no `Architecture` counterpart offers none.
const HOST_ARCH: &[Architecture] = if cfg!(target_arch = "x86_64") {
    &[Architecture::X86_64]
} else if cfg!(target_arch = "aarch64") {
    &[Architecture::Aarch64]
} else {
    &[]
};

/// No backend takes a checkpoint of any kind.
const NO_CHECKPOINT: &[CheckpointKind] = &[CheckpointKind::None];

pub const LOCAL_INTERPRETER: Profile = Profile {
    id: axon_cortex::runner::LocalInterpreterExecutor::PROFILE,
    engine: Engine::AxonInterpreter,
    enclosure: "process_scoped",
    guest_kind: "none",
    os: Os::None,
    hardware_isolation: false,
    isolation: Isolation::ProcessScoped,
    job_kinds: &[JobKind::RegisteredCheck],
    architectures: HOST_ARCH,
    checkpoint_kinds: NO_CHECKPOINT,
};

pub const FIRECRACKER_AXON_KERNEL: Profile = Profile {
    id: axon_vm::BACKEND_PROFILE.id,
    engine: Engine::AxonInterpreter,
    enclosure: "kvm_microvm",
    guest_kind: "axon_kernel_demo",
    os: Os::None,
    hardware_isolation: true,
    isolation: Isolation::KvmMicroVmUnqualified,
    // The Axon guest kernel does not execute programs yet.
    job_kinds: &[],
    // `x86_64-axon-metal`.
    architectures: &[Architecture::X86_64],
    checkpoint_kinds: NO_CHECKPOINT,
};

pub const LINUX_MICROVM_PROTECTED: Profile = Profile {
    id: "linux-microvm-protected",
    engine: Engine::AxonInterpreter,
    enclosure: "kvm_microvm",
    guest_kind: "linux_init",
    os: Os::Linux,
    hardware_isolation: true,
    isolation: Isolation::LinuxMicroVmProtected,
    // `registered_check` only for an OPERATOR suite (`check:<id>`), run by the
    // trusted guest runner (PSV, v022-psv-protocol.md §4); `submit` refuses
    // any other target on this profile. NOTHING ELSE: every launch here goes
    // through the launch manifest, the custodian nonce and the preflight
    // observation, and an `interpreter_run` has no such path. It used to be
    // offered, and launched with no observation at all (PSV-6, C9 dev review
    // round 1; A54).
    job_kinds: &[JobKind::RegisteredCheck],
    // The pinned guest is x86_64 (`profiles/linux-microvm/manifest.json`:
    // `x86_64-unknown-linux-musl` interpreter, x86_64 kernel config).
    architectures: &[Architecture::X86_64],
    checkpoint_kinds: NO_CHECKPOINT,
};

/// Registry id of the interpreter INSIDE the Linux guest rootfs. Its sha256 is
/// the manifest's `artifacts.axon` pin, not a host file.
pub const LINUX_GUEST_AXON_ID: &str = "axon-linux-guest";

pub const ALL: &[Profile] = &[
    LOCAL_INTERPRETER,
    FIRECRACKER_AXON_KERNEL,
    LINUX_MICROVM_PROTECTED,
];

/// Operator configuration for the Linux microVM profile.
#[derive(Debug, Clone)]
pub struct LinuxProfileConfig {
    /// `scripts/fc_linux_profile.sh` (must run as root).
    pub launcher: PathBuf,
    /// The launcher's pinned sha256 (O1, `v022-psv-protocol.md` §2). The
    /// launcher writes the result and the guest digests a receipt rests on, so
    /// it is an authority, not a helper: any other bytes make the profile
    /// ineligible (RULE:launcher-pinned). Eligibility is re-decided at
    /// dispatch, immediately before the launch record; the window after that
    /// is closed by the launcher being operator-owned (O1), not by a re-hash.
    pub launcher_sha256: String,
    /// `profiles/linux-microvm/manifest.json`.
    pub manifest: PathBuf,
    /// The built artifacts (`dist/guest-linux`), if not the launcher default.
    pub artifacts_dir: Option<PathBuf>,
    /// The qualification evidence record (`axon-b263-evidence/1`).
    pub evidence: PathBuf,
    /// Detached issuer signature over the EXACT evidence bytes
    /// (`axon-evidence-signature/2`). `None` ⇒ `<evidence>.sig`.
    pub evidence_signature: Option<PathBuf>,
    /// Issuer-signed waivers (`axon-b263-waiver/1`) for BLOCKED assertions,
    /// signed the same way at `<waivers>.sig`. `None` ⇒ no waivers, so any
    /// BLOCKED assertion makes the profile ineligible.
    pub waivers: Option<PathBuf>,
    /// Who may issue evidence, how old it may be, and what "now" is.
    pub trust: QualificationTrust,
    /// Parent of per-operation `--out` directories.
    pub out_root: PathBuf,
    /// D: the uid that must own the launcher (and interpreter) Fabric itself
    /// executes on the DIRECT route. `None`: any owner (development only; the
    /// direct route never attests a protected verdict).
    pub exec_owner: Option<u32>,
    /// D: the interpreter the launcher script runs under on the DIRECT route
    /// (pinned; executed from its verified descriptor). `None`: `/bin/bash`,
    /// hashed where it is executed (development only).
    pub interpreter: Option<crate::sealed_exec::Pinned>,
    /// A: the privileged helper that launches on a protected host (Fabric is
    /// non-root there). `None`: the DIRECT development route, which never
    /// yields protected evidence ([`LaunchRoute::may_attest_protected`]).
    pub privileged: Option<PrivilegedRoute>,
}

/// A: how Fabric reaches the privileged helper.
#[derive(Debug, Clone)]
pub struct PrivilegedRoute {
    /// `axon-protected-launcher`, pinned in the protected-host config.
    pub helper: crate::sealed_exec::Pinned,
    /// Its owner (root on a protected host).
    pub owner: u32,
    /// TEST-TRUST helper builds only: `--test-config FILE`. A production
    /// helper refuses the flag; a production Fabric never sends it.
    pub test_config: Option<PathBuf>,
}

/// Which route a Linux-profile launch took.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaunchRoute {
    /// Fabric executed the launcher itself (development and tests).
    Direct,
    /// Through the privileged helper; `test_build` when the helper says it is
    /// a test-trust build.
    Privileged { test_build: bool },
}

impl LaunchRoute {
    /// Operator decision B: a protected verdict needs the pinned PRIVILEGED
    /// launcher in its chain. The direct route never qualifies, and a
    /// test-trust helper qualifies only inside a test-trust Fabric (whose
    /// trust roots are test roots anyway).
    pub fn may_attest_protected(self, fabric_test_build: bool) -> bool {
        match self {
            LaunchRoute::Direct => false,
            LaunchRoute::Privileged { test_build } => !test_build || fabric_test_build,
        }
    }

    /// The route a helper's report puts a launch on. Anything but a report
    /// naming the `production` build is a test build: an unknown or missing
    /// name never reads as production (PSV-4, C9 round 3).
    pub fn of_report(report: &crate::privileged_launcher::LaunchReport) -> LaunchRoute {
        LaunchRoute::Privileged {
            test_build: report.build != "production",
        }
    }
}

/// THE decision whether a launch on `route` may attest a protected verdict in
/// THIS Fabric build (`psv_receipt` and the verifier manifest both call it).
/// `TEST_TRUST_BUILD` is true in every `cargo test` build, so this call's
/// production value is observable only from a production build: the verifier
/// manifest reports it (`launch_routes_attesting_protected`), and
/// tests/privileged_launcher.rs builds one to read it (PSV-4, C9 round 3).
pub fn attests_protected(route: LaunchRoute) -> bool {
    route.may_attest_protected(TEST_TRUST_BUILD)
}

/// Default ceiling on the age of a qualification record: 30 days.
pub const DEFAULT_EVIDENCE_MAX_AGE_S: u64 = 30 * 24 * 3600;
/// `/2`: DOMAIN-SEPARATED. The signed message is
/// `axon-evidence-signature/2\n<authority>\n<exact bytes>`, and the signature
/// names its authority, so a key trusted for one purpose never validates a
/// statement of another, whatever directory it sits in. `/1` (bytes alone,
/// no domain) is refused: nothing operator-signed under it exists.
pub use axon_loop_contracts::operator_trust::{
    evidence_signing_message, EVIDENCE_SIGNATURE_SCHEMA,
};

pub const WAIVER_SCHEMA: &str = "axon-b263-waiver/1";

/// The time source freshness and waiver expiry are judged against. Injectable
/// so a test can pin "now"; production uses the system clock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    System,
    /// Seconds since the Unix epoch.
    FixedUnix(i64),
}

impl Clock {
    pub fn now_unix(&self) -> i64 {
        match self {
            Clock::System => std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0),
            Clock::FixedUnix(t) => *t,
        }
    }
}

/// The operator's trust roots, OUTSIDE any repository: one implementation,
/// shared with the loop (`axon_loop_contracts::operator_trust`).
pub use axon_loop_contracts::operator_trust::{TrustAuthority, OPERATOR_TRUST_ROOT};

/// Whether this build carries the TEST-ONLY trust constructors.
pub const TEST_TRUST_BUILD: bool = cfg!(any(test, feature = "test-trust-root"));

/// Whether a `verify-evidence` answer is AUTHORITATIVE (review FIELD-ORIGIN,
/// C9 round 1). `--issuers` is the caller's choice, so "verified" alone says
/// only that some key in a directory the caller named signed the bytes. The
/// answer speaks for the operator only when this is a production build, the
/// root is the operator's own root for the authority, and that root passes the
/// ownership walk (`owned`). `Err` is why it is not. Each condition is its own
/// guard: in a test-trust build the first alone decides, so each is tested on
/// the inputs where it is the only one that can refuse.
pub fn verify_evidence_authority(
    test_trust_build: bool,
    issuers: &Path,
    operator_root: &Path,
    owned: Result<(), String>,
) -> Result<(), String> {
    if test_trust_build {
        return Err("this is a test-trust build".into());
    }
    if issuers != operator_root {
        return Err("the trust root is the caller's --issuers, not the operator's root".into());
    }
    owned.map_err(|e| format!("the operator root fails the ownership walk: {e}"))
}

/// The trust root for qualification evidence. Only PUBLIC keys live here; the
/// signing key is held by the operator (decision D6) and never by this tree.
#[derive(Debug, Clone)]
pub struct QualificationTrust {
    /// Directory of `*.pub` files, each the 64-hex-char Ed25519 public key of
    /// a trusted evidence issuer. Absent or empty ⇒ nothing is trusted.
    pub issuers_dir: PathBuf,
    /// An evidence record whose `end` is older than this is stale.
    pub max_age_s: u64,
    pub clock: Clock,
    /// The directory and every key in it must be operator-owned (see
    /// [`check_operator_owned`]). True for [`QualificationTrust::operator`], the
    /// only constructor the `axon-fabric` CLI uses.
    pub operator_owned: bool,
    /// The protected host's attestation signer (`signer.public_key`, hex),
    /// set when the host config loads. Fabric holds its private half, so the
    /// qualification root must never hold it (C9 round 2; A67).
    pub host_signer_public_key: Option<String>,
}

impl QualificationTrust {
    /// PRODUCTION: `/etc/axon/trust/qualification/`, operator-owned. There is
    /// no fallback: an absent or unreadable root is NOT QUALIFIED.
    pub fn operator() -> QualificationTrust {
        QualificationTrust {
            issuers_dir: TrustAuthority::Qualification.operator_dir(),
            max_age_s: DEFAULT_EVIDENCE_MAX_AGE_S,
            clock: Clock::System,
            operator_owned: true,
            host_signer_public_key: None,
        }
    }

    /// TESTS ONLY (feature `test-trust-root`): `<manifest dir>/trusted_issuers`,
    /// with no ownership requirement. Absent from production builds.
    #[cfg(any(test, feature = "test-trust-root"))]
    pub fn for_manifest(manifest: &Path) -> QualificationTrust {
        QualificationTrust {
            issuers_dir: manifest
                .parent()
                .unwrap_or(Path::new("."))
                .join("trusted_issuers"),
            max_age_s: DEFAULT_EVIDENCE_MAX_AGE_S,
            clock: Clock::System,
            operator_owned: false,
            host_signer_public_key: None,
        }
    }

    /// The issuer keys this trust accepts NOW: the qualification root read
    /// exclusively ([`exclusive_root_keys`]) against its sibling roots and
    /// the host signer, at every read, not only when the host config loaded.
    pub fn trusted_keys(&self) -> Result<Vec<Vec<u8>>, String> {
        trusted_issuers(
            TrustAuthority::Qualification,
            &self.issuers_dir,
            self.operator_owned.then_some(Path::new("/")),
            self.host_signer_public_key.as_deref(),
        )
    }
}

/// An operator-owned trust directory. It must be an ABSOLUTE path; every
/// component from `/` down to it, the directory itself and every entry in it
/// must be a real file or directory (no symlink anywhere on the path), owned
/// by root, and writable by neither group nor other. Anything else — including
/// an absent directory — authorizes nothing.
#[cfg(unix)]
pub fn check_operator_owned(dir: &Path) -> Result<(), String> {
    check_owned_from(Path::new("/"), dir)
}

/// TESTS ONLY: [`check_operator_owned`] from `base` down (a temp dir's
/// ancestors are not operator-owned).
#[cfg(all(unix, any(test, feature = "test-trust-root")))]
pub fn check_operator_owned_below(base: &Path, dir: &Path) -> Result<(), String> {
    check_owned_from(base, dir)
}

/// [`check_operator_owned`] from `base` (crate-internal: `readiness` passes
/// `/` in production and a test base only in test-trust builds).
#[cfg(unix)]
pub(crate) fn check_owned_from_pub(base: &Path, dir: &Path) -> Result<(), String> {
    check_owned_from(base, dir)
}

#[cfg(unix)]
fn check_owned_from(base: &Path, dir: &Path) -> Result<(), String> {
    check_owned_chain(base, dir, true)
}

/// The operator-ownership walk from `base` down to `path`, every component
/// root-owned, not group/other-writable, not a symlink; with `entries`, a
/// directory's entries too. A FILE (a pinned launcher, a registry) is checked
/// with its whole chain and nothing listed.
#[cfg(unix)]
pub(crate) fn check_owned_chain(base: &Path, dir: &Path, entries: bool) -> Result<(), String> {
    axon_loop_contracts::operator_trust::check_owned_chain(base, dir, entries)
}

#[cfg(not(unix))]
pub fn check_operator_owned(dir: &Path) -> Result<(), String> {
    Err(format!(
        "{}: operator ownership cannot be checked on this platform",
        dir.display()
    ))
}

/// The facts eligibility is decided from — and that a receipt carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxQualification {
    /// The launcher that will run it, at its pin.
    pub launcher_sha256: String,
    pub manifest_sha256: String,
    pub evidence_manifest_sha256: String,
    pub guest_axon_sha256: String,
    /// sha256 of the exact evidence bytes the issuer signed.
    pub evidence_sha256: String,
    /// `ed25519:<first 16 hex of sha256(public key)>` of the issuer.
    pub issuer: String,
    pub host: String,
    /// The record's caveat (e.g. the D2 nested-virtualisation caveat).
    pub caveat: String,
    /// The record's `end`, as written.
    pub end: String,
    pub firecracker_sha256: String,
    pub jailer_sha256: String,
    /// BLOCKED assertions admitted only under an issuer-signed waiver.
    pub waived: Vec<String>,
    /// The signed record shows [`X1_GUEST_POLICY_CHANNEL`] as `PASS`: the
    /// guest was qualified as enforcing the policy the Fabric delivers. A
    /// BLOCKED x1 — waived or not — and an absent x1 are both `false`.
    pub guest_policy_channel: bool,
}

/// The B263 assertion that qualifies the guest policy channel (ACF-G25).
pub const X1_GUEST_POLICY_CHANNEL: &str = "x1_guest_policy_channel";

/// Read `p` ONCE, as a regular file, not through a symlink. The bytes returned
/// are the only bytes a caller may verify, hash, parse or decide on: a second
/// read of the same path can return different bytes (a FIFO serves each open
/// something new; a rename swaps the file between two opens), so "the bytes
/// the signature covers" and "the bytes the decision used" must be one buffer
/// (review PSV-7, C9 round 1). The open is non-blocking so a FIFO with no
/// writer cannot hang the caller; anything but a regular file is refused.
pub fn read_regular(p: &Path) -> Result<Vec<u8>, String> {
    use std::io::Read;
    /// Evidence, records and signatures are small; nothing legitimate is
    /// bigger, and an unbounded read is a denial of service.
    const MAX: u64 = 256 << 20;
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        o.custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC);
    }
    let f = o.open(p).map_err(|e| {
        #[cfg(unix)]
        let symlink = e.raw_os_error() == Some(libc::ELOOP);
        #[cfg(not(unix))]
        let symlink = false;
        if symlink {
            format!(
                "{} is a symlink: evidence is read only from a regular file",
                p.display()
            )
        } else {
            format!("{}: {e}", p.display())
        }
    })?;
    let md = f.metadata().map_err(|e| format!("{}: {e}", p.display()))?;
    if !md.is_file() {
        return Err(format!(
            "{} is not a regular file: a FIFO, device or directory can serve different bytes to \
             each read, so nothing read from it is evidence",
            p.display()
        ));
    }
    let mut b = Vec::new();
    f.take(MAX + 1)
        .read_to_end(&mut b)
        .map_err(|e| format!("{}: {e}", p.display()))?;
    if b.len() as u64 > MAX {
        return Err(format!("{} is larger than {MAX} bytes", p.display()));
    }
    Ok(b)
}

pub(crate) fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(format!("{:x}", Sha256::digest(&b)))
}

pub(crate) fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

pub(crate) fn hex_decode(s: &str) -> Option<Vec<u8>> {
    let s = s.trim();
    if !s.len().is_multiple_of(2) || !s.bytes().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}

fn is_hex64(v: &serde_json::Value) -> bool {
    v.as_str()
        .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit()))
}

fn non_empty(v: &serde_json::Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

/// `YYYY-MM-DDTHH:MM:SSZ` (the evidence harness's format) → Unix seconds.
/// Anything else is refused rather than guessed at.
pub fn parse_utc(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 20
        || b[4] != b'-'
        || b[7] != b'-'
        || b[10] != b'T'
        || b[13] != b':'
        || b[16] != b':'
        || b[19] != b'Z'
    {
        return None;
    }
    let n = |r: std::ops::Range<usize>| -> Option<i64> {
        let t = &s[r];
        t.bytes()
            .all(|c| c.is_ascii_digit())
            .then(|| t.parse().ok())
            .flatten()
    };
    let (y, mo, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (h, mi, se) = (n(11..13)?, n(14..16)?, n(17..19)?);
    if !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || se > 59 {
        return None;
    }
    // Days from civil (Howard Hinnant).
    let y2 = if mo <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + se)
}

/// Every other authority's root, as `dir`'s SIBLINGS named by authority:
/// the `/etc/axon/trust/<authority>/` layout, so for an operator root these
/// are exactly the other operator roots the loop's
/// `operator_trust::exclusive` reads. Derived from `TrustAuthority::ALL`,
/// never a hand-written list.
pub fn sibling_roots(a: TrustAuthority, dir: &Path) -> Vec<(TrustAuthority, PathBuf)> {
    let up = dir.parent().unwrap_or(Path::new("/nonexistent"));
    TrustAuthority::ALL
        .into_iter()
        .filter(|b| *b != a)
        .map(|b| (b, up.join(b.dir_name())))
        .collect()
}

/// ADR-002 key-role separation at EVERY Fabric trust-root read (C9 round 2,
/// PSV-6 + FIELD-ORIGIN; A67), decided the loop's way
/// (`operator_trust::exclusive`): the keys of `a`'s root `dir`, refused whole
/// if any of them is
/// * the host signer's public key (`host_signer`) and `a` is not the verifier
///   authority: Fabric holds that key's private half and signs any domain;
/// * held by another authority's root in `peers`: a key in two roots is
///   authority for both.
///
/// A peer is ABSENT only when it does not exist (NotFound). A present peer is
/// walked for operator ownership (from `owned_from`, when given) and read with
/// `keys_in`, which refuses a root it cannot list, so an unreadable root is
/// never read as holding no key. `dir` itself is read with `keys_in` too; its
/// ownership is the caller's walk.
pub(crate) fn exclusive_root_keys(
    a: TrustAuthority,
    dir: &Path,
    peers: &[(TrustAuthority, PathBuf)],
    owned_from: Option<&Path>,
    host_signer: Option<&str>,
) -> Result<Vec<String>, String> {
    use axon_loop_contracts::operator_trust::keys_in;
    let mine = keys_in(dir)?;
    let fp = |k: &str| {
        hex_decode(k)
            .map(|b| axon_loop_contracts::attestation::key_fingerprint(&b))
            .unwrap_or_else(|| k.to_string())
    };
    if let Some(signer) = host_signer.filter(|_| a != TrustAuthority::Verifier) {
        let signer = signer.trim().to_ascii_lowercase();
        if mine.contains(&signer) {
            return Err(format!(
                "the {} root {} holds the host signer's public key {}: Fabric holds that key's \
                 private half, so it could mint {} evidence (ADR-002 key-role separation)",
                a.dir_name(),
                dir.display(),
                fp(&signer),
                a.dir_name()
            ));
        }
    }
    for (b, peer) in peers {
        if *b == a {
            continue;
        }
        if matches!(std::fs::symlink_metadata(peer), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            continue;
        }
        #[cfg(unix)]
        if let Some(base) = owned_from {
            check_owned_chain(base, peer, true)?;
        }
        let theirs = keys_in(peer)?;
        if let Some(k) = mine.iter().find(|k| theirs.contains(k)) {
            return Err(format!(
                "key {} is in the {} root {} AND the {} root {}: a key serves one authority \
                 (ADR-002 key-role separation)",
                fp(k),
                a.dir_name(),
                dir.display(),
                b.dir_name(),
                peer.display()
            ));
        }
    }
    Ok(mine)
}

/// Load the trusted issuer public keys of `a`'s root `dir`, exclusively
/// ([`exclusive_root_keys`] against `dir`'s sibling roots). A malformed key
/// file is an error (fail closed), never skipped; no keys at all is an error
/// too.
fn trusted_issuers(
    a: TrustAuthority,
    dir: &Path,
    owned_from: Option<&Path>,
    host_signer: Option<&str>,
) -> Result<Vec<Vec<u8>>, String> {
    let keys: Vec<Vec<u8>> =
        exclusive_root_keys(a, dir, &sibling_roots(a, dir), owned_from, host_signer)?
            .iter()
            .filter_map(|h| hex_decode(h))
            .collect();
    if keys.is_empty() {
        return Err(format!(
            "no trusted evidence issuer is configured ({} holds no *.pub key); unsigned or \
             self-authored evidence cannot qualify the profile",
            dir.display()
        ));
    }
    Ok(keys)
}

/// Where the peers of a root named only by path are walked from: an
/// operator root (`/etc/axon/trust/<a>`) has operator peers, walked from
/// `/`; any other directory (a test root, a caller's `--issuers`) is not
/// the operator's and its peers are read unwalked.
fn operator_walk(a: TrustAuthority, dir: &Path) -> Option<&'static Path> {
    (dir == a.operator_dir()).then_some(Path::new("/"))
}

/// Verify a detached `axon-evidence-signature/2` over `bytes`. Returns the
/// issuer fingerprint. Each refusal is its own rule.
fn verify_detached(
    what: &str,
    bytes: &[u8],
    sig_path: &Path,
    trusted: &[Vec<u8>],
    authority: TrustAuthority,
) -> Result<String, String> {
    let sig_file = read_signature(what, sig_path)?;
    // The cryptographic rules are the loop's too: one implementation.
    axon_loop_contracts::operator_trust::verify_evidence_signature(
        what, bytes, &sig_file, trusted, authority,
    )
}

/// The detached signature at `sig_path`, read ONCE ([`read_regular`]). Absent
/// is its own refusal (RULE:unsigned).
pub fn read_signature(what: &str, sig_path: &Path) -> Result<String, String> {
    // RULE:unsigned
    if std::fs::symlink_metadata(sig_path).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Err(format!(
            "{what} is unsigned: no detached signature at {}",
            sig_path.display()
        ));
    }
    let b = read_regular(sig_path).map_err(|e| format!("{what} signature {e}"))?;
    String::from_utf8(b)
        .map_err(|_| format!("{what} signature {} is not UTF-8", sig_path.display()))
}

/// Verify an operator-signed evidence document (e.g. a protected-host
/// certification record): its detached `axon-evidence-signature/2` (`sig`)
/// over the EXACT bytes of `record`, under a key in `issuers_dir`. The same
/// rules as the B263 qualification record: unsigned, untrusted, malformed or
/// non-verifying all refuse, and no configured issuer at all refuses. Returns
/// the issuer fingerprint.
pub fn verify_operator_evidence(
    record: &Path,
    sig: &Path,
    issuers_dir: &Path,
    authority: TrustAuthority,
) -> Result<String, String> {
    let bytes = read_regular(record).map_err(|e| format!("evidence {e}"))?;
    let trusted = trusted_issuers(
        authority,
        issuers_dir,
        operator_walk(authority, issuers_dir),
        None,
    )?;
    verify_detached("evidence", &bytes, sig, &trusted, authority)
}

/// [`verify_operator_evidence_bytes`] with the signature TEXT already read
/// too: the caller keeps exactly the signature it verified (e.g. to carry it
/// into a bundle) instead of reading the file a second time.
pub fn verify_operator_evidence_signed(
    what: &str,
    bytes: &[u8],
    sig_text: &str,
    issuers_dir: &Path,
    authority: TrustAuthority,
) -> Result<String, String> {
    let trusted = trusted_issuers(
        authority,
        issuers_dir,
        operator_walk(authority, issuers_dir),
        None,
    )?;
    axon_loop_contracts::operator_trust::verify_evidence_signature(
        what, bytes, sig_text, &trusted, authority,
    )
}

/// [`verify_operator_evidence`] over bytes the caller has ALREADY read, so the
/// bytes verified are exactly the bytes it goes on to use.
pub fn verify_operator_evidence_bytes(
    what: &str,
    bytes: &[u8],
    sig: &Path,
    issuers_dir: &Path,
    authority: TrustAuthority,
) -> Result<String, String> {
    let trusted = trusted_issuers(
        authority,
        issuers_dir,
        operator_walk(authority, issuers_dir),
        None,
    )?;
    verify_detached(what, bytes, sig, &trusted, authority)
}

fn sidecar_sig(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

pub(crate) struct Waiver {
    reason: String,
    expires: Option<i64>,
}

/// BLOCKED-assertion waivers by assertion name.
pub(crate) type Waivers = std::collections::BTreeMap<String, Waiver>;

/// A waiver file's waivers, refused unless it is an [`WAIVER_SCHEMA`] file
/// bound to the evidence record whose sha256 is `evidence_sha256`
/// (RULE:waiver-bound). The caller has verified its signature.
pub(crate) fn parse_waivers(wb: &[u8], evidence_sha256: &str) -> Result<Waivers, String> {
    let w: serde_json::Value =
        serde_json::from_slice(wb).map_err(|e| format!("waivers are not JSON: {e}"))?;
    if w["schema"] != WAIVER_SCHEMA {
        return Err(format!("waiver file schema is not {WAIVER_SCHEMA}"));
    }
    // RULE:waiver-bound
    if w["evidence_sha256"].as_str() != Some(evidence_sha256) {
        return Err(
            "waiver file is bound to a different evidence record; a waiver is not transferable"
                .into(),
        );
    }
    let mut waivers = Waivers::new();
    for x in w["waivers"]
        .as_array()
        .ok_or("waiver file has no waivers list")?
    {
        let name = non_empty(&x["assertion"]).ok_or("a waiver names no assertion")?;
        waivers.insert(
            name,
            Waiver {
                reason: non_empty(&x["reason"]).unwrap_or_default(),
                expires: x["expires"].as_str().and_then(parse_utc),
            },
        );
    }
    Ok(waivers)
}

/// What a B263 record that [`accept_b263`] accepts states.
#[derive(Debug, Clone)]
pub struct AcceptedB263 {
    pub host: String,
    pub caveat: String,
    /// `end`, as written and as Unix seconds.
    pub end: String,
    pub end_unix: i64,
    pub firecracker_sha256: String,
    pub jailer_sha256: String,
    /// The BLOCKED assertions its waivers cover.
    pub waived: Vec<String>,
    /// [`X1_GUEST_POLICY_CHANNEL`] is PASS.
    pub guest_policy_channel: bool,
}

/// THE rules that make a signed `axon-b263-evidence/1` record a CURRENT
/// qualification: one implementation, applied by Fabric before a protected
/// launch ([`LinuxProfileConfig::qualification`]) and by readiness to the
/// record a certification names (review PSV-7, C9 round 3; A78). Readiness
/// used to check only the record's signature, schema, profile and guest
/// digests, so a FAIL, stale, dirty-tree or host-less record Fabric refuses
/// certified PASS.
///
/// `issuer` is the key that verified the record's signature (the caller
/// checks the signature, schema and profile, each under its own trust).
/// `now` is the decision time; `waivers` loads the verified waivers for the
/// record's BLOCKED assertions and is called only when there are some.
pub(crate) fn accept_b263(
    ev: &serde_json::Value,
    issuer: &str,
    now: i64,
    max_age_s: u64,
    waivers: impl FnOnce() -> Result<Waivers, String>,
) -> Result<AcceptedB263, String> {
    // RULE:issuer-claimed: the record names the key it is issued under, and
    // that is the key that verified it — a record signed by one trusted
    // issuer cannot pass as another's.
    if ev["issuer_key_id"].as_str() != Some(issuer) {
        return Err(format!(
            "evidence record claims issuer_key_id {} but is signed by {issuer}",
            ev["issuer_key_id"]
        ));
    }

    // ── Verdict ────────────────────────────────────────────────────────
    let assertions = ev["assertions"]
        .as_array()
        .ok_or("evidence record has no assertions list")?;
    let with = |st: &str| -> Vec<String> {
        assertions
            .iter()
            .filter(|a| a["status"] == st)
            .map(|a| a["name"].as_str().unwrap_or("?").to_string())
            .collect()
    };
    let blocked = with("BLOCKED");
    let failed = with("FAIL");
    let guest_policy_channel = with("PASS").iter().any(|n| n == X1_GUEST_POLICY_CHANNEL);
    // RULE:fail-zero
    if ev["counts"]["FAIL"].as_u64() != Some(0) || !failed.is_empty() {
        return Err(format!(
            "evidence record has FAIL assertions (or none counted): {failed:?}"
        ));
    }
    // RULE:pass-count
    if ev["counts"]["PASS"].as_u64().unwrap_or(0) == 0 {
        return Err(
            "evidence record counts no PASS assertion; an empty run qualifies nothing".into(),
        );
    }
    // RULE:blocked-count
    if ev["counts"]["BLOCKED"].as_u64() != Some(blocked.len() as u64) {
        return Err(format!(
            "evidence counts.BLOCKED {} disagrees with the {} BLOCKED assertion(s)",
            ev["counts"]["BLOCKED"],
            blocked.len()
        ));
    }
    let result = ev["result"].as_str().unwrap_or("");
    // RULE:result
    if !((result == "PASS" && blocked.is_empty())
        || (result == "PASS_WITH_BLOCKED" && !blocked.is_empty()))
    {
        return Err(format!("evidence result is {result:?}; only PASS, or PASS_WITH_BLOCKED with every BLOCKED waived, qualifies"));
    }

    // ── Waivers for BLOCKED assertions ──────────────────────────────────
    let waivers = if blocked.is_empty() {
        Waivers::new()
    } else {
        waivers()?
    };
    for name in &blocked {
        // RULE:blocked-unwaived
        if !waivers.contains_key(name) {
            return Err(format!(
                "BLOCKED assertion {name} is not covered by an issuer-signed waiver"
            ));
        }
        // RULE:waiver-reason
        if waivers.get(name).is_some_and(|w| w.reason.is_empty()) {
            return Err(format!("the waiver for {name} states no reason"));
        }
        // RULE:waiver-expiry
        if waivers
            .get(name)
            .is_some_and(|w| w.expires.is_none_or(|t| now >= t))
        {
            return Err(format!(
                "the waiver for {name} has expired (or states no parseable expiry)"
            ));
        }
    }

    // ── Freshness ───────────────────────────────────────────────────────
    let end_s = ev["end"].as_str().unwrap_or("").to_string();
    let end = parse_utc(&end_s).ok_or(format!(
        "evidence end {end_s:?} is not a YYYY-MM-DDTHH:MM:SSZ time"
    ))?;
    // RULE:end-not-future
    if end > now {
        return Err(format!("evidence end {end_s} is in the future"));
    }
    // RULE:end-fresh
    if (now - end) as u64 > max_age_s {
        return Err(format!(
            "evidence end {end_s} is stale (older than {max_age_s} s)"
        ));
    }

    // ── Engine, source, host ────────────────────────────────────────────
    let eng = &ev["engine"];
    // RULE:engine-digests
    if !is_hex64(&eng["firecracker_sha256"]) || !is_hex64(&eng["jailer_sha256"]) {
        return Err("evidence record lacks engine.firecracker_sha256 / engine.jailer_sha256; an unidentified VMM qualifies nothing".into());
    }
    // RULE:tree-clean
    if ev["source"]["tree_dirty"] != serde_json::Value::Bool(false) {
        return Err("evidence was produced from a dirty (or unstated) source tree".into());
    }
    let host = non_empty(&ev["host"]).unwrap_or_default();
    // RULE:host
    if host.is_empty() {
        return Err("evidence record states no host".into());
    }
    let caveat = non_empty(&ev["caveat"]).unwrap_or_default();
    // RULE:caveat
    if caveat.is_empty() {
        return Err(
            "evidence record states no caveat; say what the boundary excludes, even if nothing"
                .into(),
        );
    }
    Ok(AcceptedB263 {
        host,
        caveat,
        end: end_s,
        end_unix: end,
        firecracker_sha256: eng["firecracker_sha256"].as_str().unwrap_or("").into(),
        jailer_sha256: eng["jailer_sha256"].as_str().unwrap_or("").into(),
        waived: blocked,
        guest_policy_channel,
    })
}

impl LinuxProfileConfig {
    /// RULE:launcher-pinned: the launcher's bytes are the pinned ones.
    pub fn launcher_pinned(&self) -> Result<String, String> {
        let got = sha256_file(&self.launcher)?;
        if got != self.launcher_sha256 {
            return Err(format!(
                "launcher {} has sha256 {got}, not its pin {}: a replaced launcher could report \
                 any digest and any exit (RULE:launcher-pinned)",
                self.launcher.display(),
                self.launcher_sha256
            ));
        }
        Ok(got)
    }

    /// Eligible only if EVERY one of these holds (fail closed on each):
    ///
    /// * the evidence bytes carry a detached Ed25519 signature that verifies
    ///   under a key in `trust.issuers_dir`;
    /// * `result` is `PASS` with no BLOCKED assertion, or `PASS_WITH_BLOCKED`
    ///   where every BLOCKED assertion is covered by an unexpired, reasoned,
    ///   issuer-signed waiver bound to these exact evidence bytes; `FAIL == 0`,
    ///   `PASS > 0`, and `counts.BLOCKED` agrees with the assertions;
    /// * `end` is not in the future and not older than `trust.max_age_s`;
    /// * the engine digests (firecracker, jailer) are recorded, and equal the
    ///   manifest's `engine` pins when the manifest has them;
    /// * neither the evidence tree nor the manifest's build tree was dirty;
    /// * `host` and `caveat` are stated (the caveat travels into receipts);
    /// * the manifest in use is byte-identical to the one qualified.
    ///
    /// A changed manifest is ineligible — never "probably fine". The record's
    /// own rules (verdict, waivers, freshness, engine digests, source tree,
    /// host, caveat) are [`accept_b263`], which readiness applies too.
    pub fn qualification(&self) -> Result<LinuxQualification, String> {
        let launcher_sha256 = self.launcher_pinned()?;
        // ONE read of the manifest: the bytes hashed (and compared with the
        // qualified manifest_sha256) are the bytes parsed below.
        let manifest_bytes = read_regular(&self.manifest).map_err(|e| format!("manifest {e}"))?;
        let manifest_sha256 = sha256_hex(&manifest_bytes);
        let ev_bytes = read_regular(&self.evidence).map_err(|e| format!("evidence {e}"))?;
        let evidence_sha256 = sha256_hex(&ev_bytes);
        // Authenticity first: nothing in an unauthenticated record is read
        // as a claim.
        if self.trust.operator_owned {
            check_operator_owned(&self.trust.issuers_dir)?;
        }
        let trusted = self.trust.trusted_keys()?;
        let sig_path = self
            .evidence_signature
            .clone()
            .unwrap_or_else(|| sidecar_sig(&self.evidence));
        let issuer = verify_detached(
            "evidence record",
            &ev_bytes,
            &sig_path,
            &trusted,
            TrustAuthority::Qualification,
        )?;

        let ev: serde_json::Value =
            serde_json::from_slice(&ev_bytes).map_err(|e| format!("evidence is not JSON: {e}"))?;
        if ev["schema"] != "axon-b263-evidence/1" {
            return Err("evidence record schema is not axon-b263-evidence/1".into());
        }
        if ev["profile"]["name"] != LINUX_MICROVM_PROTECTED.id {
            return Err("evidence record is for a different profile".into());
        }
        let now = self.trust.clock.now_unix();
        // The record's own qualifying rules: ONE implementation, shared with
        // readiness (review PSV-7, C9 round 3; A78).
        let b = accept_b263(&ev, &issuer, now, self.trust.max_age_s, || {
            let Some(wp) = &self.waivers else {
                return Ok(Waivers::new());
            };
            let wb = read_regular(wp).map_err(|e| format!("waivers {e}"))?;
            verify_detached(
                "waiver file",
                &wb,
                &sidecar_sig(wp),
                &trusted,
                TrustAuthority::Qualification,
            )?;
            parse_waivers(&wb, &evidence_sha256)
        })?;

        // ── Engine pins and the build tree (this host's manifest) ─────────
        let m: serde_json::Value = serde_json::from_slice(&manifest_bytes)
            .map_err(|e| format!("manifest is not JSON: {e}"))?;
        let eng = &ev["engine"];
        let pin = &m["engine"];
        // RULE:engine-pin — REQUIRED. It used to apply only "when the manifest
        // has an engine block", so a manifest without one qualified any VMM the
        // evidence named. Every manifest linux_profile_manifest.py writes now
        // pins both engines (S3-2), so a missing or malformed pin is refused.
        if !is_hex64(&pin["firecracker_sha256"]) || !is_hex64(&pin["jailer_sha256"]) {
            return Err("manifest pins no engine (engine.firecracker_sha256 / engine.jailer_sha256); evidence cannot be bound to a known VMM".into());
        }
        if pin["firecracker_sha256"] != eng["firecracker_sha256"]
            || pin["jailer_sha256"] != eng["jailer_sha256"]
        {
            return Err("evidence engine digests differ from the manifest's engine pins".into());
        }
        // RULE:manifest-clean
        if m["source"]["axon_tree_dirty_at_build"] != serde_json::Value::Bool(false) {
            return Err("manifest artifacts were built from a dirty (or unstated) tree".into());
        }

        // ── Manifest identity ───────────────────────────────────────────────
        let evidence_manifest_sha256 = ev["profile"]["manifest_sha256"]
            .as_str()
            .ok_or("evidence record has no profile.manifest_sha256")?
            .to_string();
        if evidence_manifest_sha256 != manifest_sha256 {
            return Err(format!(
                "manifest sha256 {manifest_sha256} differs from the qualified {evidence_manifest_sha256}; \
                 a changed manifest is not the qualified profile"
            ));
        }
        let guest_axon_sha256 = m["artifacts"]["axon"]["sha256"]
            .as_str()
            .ok_or("manifest has no artifacts.axon.sha256")?
            .to_string();
        Ok(LinuxQualification {
            launcher_sha256,
            manifest_sha256,
            evidence_manifest_sha256,
            guest_axon_sha256,
            evidence_sha256,
            issuer,
            host: b.host,
            caveat: b.caveat,
            end: b.end,
            firecracker_sha256: b.firecracker_sha256,
            jailer_sha256: b.jailer_sha256,
            waived: b.waived,
            guest_policy_channel: b.guest_policy_channel,
        })
    }
}

/// What the caller's authority needs from the backend beyond the request.
#[derive(Debug, Clone, Copy, Default)]
pub struct AuthorityNeeds {
    /// An effect ceiling must be delivered INTO the execution (the guest).
    pub guest_policy_channel: bool,
    /// The grant scopes filesystem paths, which must be preserved.
    pub path_scoped_grant: bool,
    /// The grant is `reproducible` (axon-os `hermetic`): the run must not see
    /// ambient `AXON_*` variables and must use the virtual clock. No backend
    /// here can guarantee that — the host interpreter executor inherits the
    /// Fabric's environment, and the Linux guest's is not policed — so such a
    /// grant is refused rather than run non-reproducibly.
    pub reproducible: bool,
}

/// Why no backend was selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

/// Does `p` offer the request's architecture and checkpoint kind? Checked for
/// the one backend a request's other requirements single out — never used to
/// pick a different one.
fn offers(p: &Profile, req: &ComputeRequest) -> Result<(), Unsupported> {
    let r = &req.required;
    if !p.architectures.contains(&r.architecture) {
        return Err(Unsupported(format!(
            "{}: architecture {:?} unsupported (offers {:?})",
            p.id, r.architecture, p.architectures
        )));
    }
    if !p.checkpoint_kinds.contains(&r.checkpoint_kind) {
        return Err(Unsupported(format!(
            "{}: checkpoint_kind {:?} unsupported (offers {:?}); no backend here checkpoints",
            p.id, r.checkpoint_kind, p.checkpoint_kinds
        )));
    }
    Ok(())
}

/// Pick the backend that satisfies EVERY requirement, or refuse. `linux` is
/// `None` when the operator configured no Linux profile.
pub fn select(
    req: &ComputeRequest,
    linux: Option<&LinuxProfileConfig>,
    needs: AuthorityNeeds,
) -> Result<Profile, Unsupported> {
    let r = &req.required;
    if needs.reproducible {
        return Err(Unsupported(
            "the grant is reproducible (hermetic), and no backend here can withhold the ambient \
             environment or impose the virtual clock; refused rather than run non-reproducibly"
                .into(),
        ));
    }
    if r.network_mode == NetworkMode::Brokered {
        return Err(Unsupported(
            "network_mode=brokered: no egress broker exists".into(),
        ));
    }
    if r.engine != Engine::AxonInterpreter {
        return Err(Unsupported(format!(
            "engine {:?}: only the Axon interpreter is offered",
            r.engine
        )));
    }
    if r.hardware_isolation && r.os == Os::Linux {
        // ONLY the qualified Linux profile. Nothing substitutes for it.
        let p = LINUX_MICROVM_PROTECTED;
        offers(&p, req)?;
        let lx = linux.ok_or_else(|| {
            Unsupported(format!(
                "hardware_isolation+os=linux requires {}; it is not configured here",
                p.id
            ))
        })?;
        let q = lx
            .qualification()
            .map_err(|why| Unsupported(format!("{} ineligible: {why}", p.id)))?;
        if !p.job_kinds.contains(&req.job_kind) {
            return Err(Unsupported(format!(
                "{}: job_kind {:?} unsupported — the protected profile runs only an operator \
                 suite check, through the observed launch path; it runs no unobserved execution",
                p.id, req.job_kind
            )));
        }
        // Lifted by the SIGNED EVIDENCE, never by a code constant: the policy
        // is always delivered, but only a PASS x1 shows the guest enforces it.
        if needs.guest_policy_channel && !q.guest_policy_channel {
            return Err(Unsupported(format!(
                "{}: the request needs an effect ceiling inside the guest, and the qualification \
                 evidence does not show {X1_GUEST_POLICY_CHANNEL} as PASS (B263 x1)",
                p.id
            )));
        }
        if needs.path_scoped_grant {
            return Err(Unsupported(format!(
                "{}: the grant is path-scoped, and this profile does not preserve path scopes \
                 (B263 x2)",
                p.id
            )));
        }
        return Ok(p);
    }
    if r.os == Os::Linux {
        return Err(Unsupported(
            "os=linux without hardware_isolation: no Linux process backend is offered".into(),
        ));
    }
    if r.hardware_isolation {
        let p = FIRECRACKER_AXON_KERNEL;
        return Err(Unsupported(format!(
            "hardware_isolation with os=none: the only such backend, {}, boots the custom Axon \
             guest kernel, which does not execute programs yet; nothing substitutes for it",
            p.id
        )));
    }
    let p = LOCAL_INTERPRETER;
    offers(&p, req)?;
    if needs.path_scoped_grant {
        // The host interpreter's only policy input is `AXON_ALLOWED_EFFECTS`,
        // a set of coarse effect names: it cannot carry a path or host
        // allowlist, so admitting a scoped grant here would enforce a wider
        // one than was admitted.
        return Err(Unsupported(format!(
            "{}: the grant scopes paths or hosts, and this backend enforces only coarse effect \
             axes (AXON_ALLOWED_EFFECTS), not allowlists",
            p.id
        )));
    }
    if !p.job_kinds.contains(&req.job_kind) {
        return Err(Unsupported(format!(
            "{}: job_kind {:?} unsupported (it runs registered checks only)",
            p.id, req.job_kind
        )));
    }
    Ok(p)
}

/// What the Linux launcher reported, after output rebinding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinuxOutcome {
    Ok {
        workload_exit: i32,
    },
    WorkloadFailed {
        workload_exit: i32,
    },
    TimedOut,
    /// Refused before anything was acquired.
    Refused,
    /// VMM died, output not bound, cleanup incomplete, verify failed, or the
    /// launcher could not be run/understood: the effect may have happened.
    Unknown,
}

#[derive(Debug, Clone)]
pub struct LinuxRun {
    pub outcome: LinuxOutcome,
    pub reason: String,
    pub evidence: Vec<String>,
    pub out_dir: PathBuf,
    pub route: LaunchRoute,
}

/// Jail id for an operation: `fab-` + 16 hex of sha256(op id).
pub fn jail_id(op: &str) -> String {
    use sha2::{Digest, Sha256};
    format!(
        "fab-{}",
        &format!("{:x}", Sha256::digest(op.as_bytes()))[..16]
    )
}

/// Map a finished launcher run in `out` to an outcome. `verify` re-runs the
/// launcher's `--verify-result` (independent rebinding of the output).
pub fn interpret_linux_result(
    exit: Option<i32>,
    out: &Path,
    verify: &mut dyn FnMut() -> Option<i32>,
) -> (LinuxOutcome, String, Vec<String>) {
    let rj = out.join("result.json");
    let mut evidence = Vec::new();
    // ONE read: the digest recorded as evidence is of the bytes interpreted.
    let bytes = read_regular(&rj).ok();
    if let Some(b) = &bytes {
        evidence.push(format!("sha256-result-json:{}", sha256_hex(b)));
    }
    let r: Option<serde_json::Value> = bytes.and_then(|b| serde_json::from_slice(&b).ok());
    let Some(r) = r else {
        return (
            LinuxOutcome::Unknown,
            format!("launcher exited {exit:?} with no readable result.json"),
            evidence,
        );
    };
    if r["schema"] != "axon-linux-microvm-result/1" {
        return (
            LinuxOutcome::Unknown,
            "result.json has the wrong schema".into(),
            evidence,
        );
    }
    if let Some(s) = r["outputs"]["stdout"]["sha256"].as_str() {
        evidence.push(format!("sha256-guest-stdout:{s}"));
    }
    // Cleanup incomplete ⇒ live resources may remain ⇒ unknown, liability kept.
    let cleanup_complete = r["cleanup"]["complete"].as_bool();
    let status = r["status"].as_str().unwrap_or("");
    match exit {
        Some(22) => {
            return (
                LinuxOutcome::Refused,
                format!("launch refused: {status}"),
                evidence,
            )
        }
        Some(24) => {
            return (
                LinuxOutcome::Unknown,
                format!(
                    "cleanup incomplete, left behind: {}",
                    r["cleanup"]["left_behind"]
                ),
                evidence,
            )
        }
        _ => {}
    }
    if cleanup_complete != Some(true) {
        return (
            LinuxOutcome::Unknown,
            format!("cleanup not confirmed complete (launcher exit {exit:?})"),
            evidence,
        );
    }
    match exit {
        Some(20) => (
            LinuxOutcome::TimedOut,
            "wall clock expired".into(),
            evidence,
        ),
        Some(0) | Some(10) => {
            if r["output_bound"].as_bool() != Some(true) {
                return (
                    LinuxOutcome::Unknown,
                    "result.json says the output is not bound".into(),
                    evidence,
                );
            }
            match verify() {
                Some(0) => {}
                other => {
                    return (
                        LinuxOutcome::Unknown,
                        format!("--verify-result did not re-bind the output (exit {other:?})"),
                        evidence,
                    )
                }
            }
            let Some(w) = r["workload_exit"].as_i64() else {
                return (LinuxOutcome::Unknown, "no workload_exit".into(), evidence);
            };
            let w = w as i32;
            if exit == Some(0) && w == 0 {
                (LinuxOutcome::Ok { workload_exit: 0 }, "ok".into(), evidence)
            } else if exit == Some(10) && w != 0 {
                (
                    LinuxOutcome::WorkloadFailed { workload_exit: w },
                    format!("workload exited {w}"),
                    evidence,
                )
            } else {
                (
                    LinuxOutcome::Unknown,
                    format!("launcher exit {exit:?} disagrees with workload_exit {w}"),
                    evidence,
                )
            }
        }
        other => (
            LinuxOutcome::Unknown,
            format!("launcher exit {other:?} ({status}): the VMM ended without a bound result"),
            evidence,
        ),
    }
}

/// Schema of the policy the guest reads (`axon-guest-init`).
pub const GUEST_POLICY_SCHEMA: &str = "axon-vm-mmds/1";
/// The guest refuses a kernel cmdline longer than this (x86
/// `COMMAND_LINE_SIZE` 2048 minus the terminator and a truncation margin —
/// `axon-guest-init`'s `CMDLINE_MAX_SAFE`).
pub const GUEST_CMDLINE_MAX_SAFE: usize = 2048 - 2;
/// Bytes of the cmdline NOT available to the policy word: the launcher's own
/// boot args (`BOOT_ARGS` in `scripts/fc_linux_profile.sh`) plus what
/// Firecracker appends (one `virtio_mmio.device=…` word per device).
pub const LAUNCHER_CMDLINE_RESERVE: usize = 512;
/// The longest ` axon.policy=<base64>` word (separator included) admitted.
pub const GUEST_POLICY_WORD_MAX: usize = GUEST_CMDLINE_MAX_SAFE - LAUNCHER_CMDLINE_RESERVE;

/// An `axon-vm-mmds/1` policy that FITS the guest's kernel cmdline. The only
/// constructor checks the size, so [`run_linux_profile`] cannot be handed a
/// policy the guest kernel would truncate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestPolicy {
    json: String,
}

impl GuestPolicy {
    /// The policy for `req` under the admitted grant's effect `ceiling` (the
    /// same `AXON_ALLOWED_EFFECTS` value the host executor receives). `""` is
    /// `allowed_effects: []` — deny every effect, never "unrestricted".
    pub fn for_grant(req: &ComputeRequest, ceiling: &str) -> Result<GuestPolicy, Unsupported> {
        let payload = axon_vm::firecracker::MmdsPayload {
            schema: GUEST_POLICY_SCHEMA.into(),
            run_id: req.operation_id.as_str().into(),
            principal: Some(req.principal_ref.as_str().into()),
            allowed_effects: ceiling
                .split(',')
                .filter(|e| !e.is_empty())
                .map(String::from)
                .collect(),
            budget_tokens: None,
            source_hash: None,
            seccomp_bpf_b64: None,
        };
        let json = serde_json::to_string(&payload)
            .map_err(|e| Unsupported(format!("guest policy does not serialize: {e}")))?;
        // Measured with the encoding the guest decodes (standard padded base64).
        let word = axon_vm::firecracker::embed_policy_in_cmdline("", &payload).len();
        if word > GUEST_POLICY_WORD_MAX {
            return Err(Unsupported(format!(
                "{}: the guest policy needs a {word}-byte cmdline word, over the \
                 {GUEST_POLICY_WORD_MAX}-byte budget (guest cmdline limit {GUEST_CMDLINE_MAX_SAFE} \
                 minus {LAUNCHER_CMDLINE_RESERVE} reserved); the kernel would truncate it",
                LINUX_MICROVM_PROTECTED.id
            )));
        }
        Ok(GuestPolicy { json })
    }

    /// The exact bytes written to the `--policy` file.
    pub fn json(&self) -> &str {
        &self.json
    }
}

/// Launch one PSV check through the Linux profile launcher, delivering
/// `policy` to the guest with `--policy FILE`.
///
/// `psv` is REQUIRED: it is the launch manifest (with the custodian nonce the
/// observer observed) and the three drives. There is no program-only launch:
/// the launcher's `--program` mode ran an `interpreter_run` on the protected
/// profile with no manifest, no nonce and no observation (PSV-6, C9 dev review
/// round 1; A54), so nothing here can build one.
///
/// `observation` travels to the privileged launcher, which verifies it itself
/// and spends its nonce (amendment 50): without one the helper launches
/// nothing. The direct (development) route never counts and needs none.
pub fn run_linux_profile(
    lx: &LinuxProfileConfig,
    req: &ComputeRequest,
    policy: &GuestPolicy,
    psv: &crate::psv::Launch,
    observation: Option<&crate::psv::VerifiedObservation>,
) -> LinuxRun {
    match &lx.privileged {
        Some(h) => run_privileged(lx, h, req, policy, psv, observation),
        None => run_direct(lx, req, policy, psv),
    }
}

const LAUNCH_PATH: &str = "/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin";

fn guest_policy_ref(policy: &GuestPolicy) -> String {
    format!(
        "guest-policy-sha256:{}",
        sha256_hex(policy.json().as_bytes())
    )
}

/// A: the launch through the privileged helper. Fabric (non-root) sends the
/// per-launch request; the helper, from its operator config, verifies and
/// runs the pinned launcher as root and reports. Fabric then requires the
/// report to name the launcher its own manifest pins.
fn run_privileged(
    lx: &LinuxProfileConfig,
    h: &PrivilegedRoute,
    req: &ComputeRequest,
    policy: &GuestPolicy,
    psv: &crate::psv::Launch,
    observation: Option<&crate::psv::VerifiedObservation>,
) -> LinuxRun {
    use crate::privileged_launcher as pl;
    use std::io::{Read, Write};
    let out = lx.out_root.join(req.operation_id.as_str());
    let route = LaunchRoute::Privileged { test_build: true };
    let refused = |reason: String| LinuxRun {
        outcome: LinuxOutcome::Refused,
        reason,
        evidence: vec![],
        out_dir: out.clone(),
        route,
    };
    let helper = match crate::sealed_exec::open_verified(
        &h.helper,
        Some(h.owner),
        crate::sealed_exec::Lease::IfGranted,
    ) {
        Ok(v) => v,
        Err(e) => return refused(format!("privileged launcher: {e}")),
    };
    let request = pl::LaunchRequest {
        schema: pl::REQUEST_SCHEMA.into(),
        id: jail_id(req.operation_id.as_str()),
        out: out.clone(),
        psv_candidate: psv.candidate_dir.clone(),
        psv_suite: psv.suite_dir.clone(),
        psv_job: psv.job_dir.clone(),
        psv_manifest_sha256: psv.digest.clone(),
        policy_json: policy.json().to_string(),
        timeout_s: req.limits.wall_time_ms.div_ceil(1000).max(1),
        // Absent: empty, and the helper refuses (no launch without one).
        observation: observation
            .map(|o| String::from_utf8_lossy(&o.bytes).into_owned())
            .unwrap_or_default(),
        observation_signature: observation.map(|o| o.signature.clone()).unwrap_or_default(),
    };
    let mut args: Vec<std::ffi::OsString> = vec![];
    if let Some(t) = &h.test_config {
        args.push("--test-config".into());
        args.push(t.clone().into_os_string());
    }
    let cmd = crate::sealed_exec::command(&helper, None, &args, &[("PATH", LAUNCH_PATH)], &[]);
    let child = cmd.and_then(|mut c| {
        c.stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())
    });
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return refused(format!("privileged launcher did not start: {e}")),
    };
    let body = serde_json::to_vec(&request).unwrap_or_default();
    if let Some(mut i) = child.stdin.take() {
        let _ = i.write_all(&body);
    }
    let mut text = Vec::new();
    if let Some(o) = child.stdout.take() {
        let _ = o.take(1 << 20).read_to_end(&mut text);
    }
    let status = child.wait().ok().and_then(|s| s.code());
    let policy_ref = guest_policy_ref(policy);
    let unknown = |reason: String, route| LinuxRun {
        outcome: LinuxOutcome::Unknown,
        reason,
        evidence: vec![policy_ref.clone()],
        out_dir: out.clone(),
        route,
    };
    let report: pl::LaunchReport = match serde_json::from_slice(&text) {
        Ok(r) => r,
        Err(e) => {
            return unknown(
                format!("privileged launcher (exit {status:?}) gave no report: {e}"),
                route,
            )
        }
    };
    let route = LaunchRoute::of_report(&report);
    if report.schema != pl::REPORT_SCHEMA {
        return unknown(
            "privileged launcher report has another schema".into(),
            route,
        );
    }
    if !report.launched {
        let why = report.error.unwrap_or_default();
        return match status {
            Some(pl::EXIT_REFUSED) => LinuxRun {
                outcome: LinuxOutcome::Refused,
                reason: format!("privileged launcher refused: {why}"),
                evidence: vec![],
                out_dir: out.clone(),
                route,
            },
            _ => unknown(
                format!("privileged launcher exited {status:?}: {why}"),
                route,
            ),
        };
    }
    // The job's source (the secret) was consumed by the helper; its dir goes.
    psv.scrub();
    if status != Some(pl::EXIT_LAUNCHED) || report.error.is_some() {
        return unknown(
            format!(
                "privileged launcher exited {status:?} after the launch: {}",
                report.error.unwrap_or_default()
            ),
            route,
        );
    }
    // The launcher the helper ran is the one this launch's manifest pins
    // (the launch manifest's and the observation's launcher_sha256).
    if report.launcher_sha256.as_deref() != Some(lx.launcher_sha256.as_str()) {
        return unknown(
            format!(
                "the privileged launcher ran launcher {:?}, not the pinned {}",
                report.launcher_sha256, lx.launcher_sha256
            ),
            route,
        );
    }
    if !report.unchanged || helper.unchanged().is_err() {
        return unknown(
            "the launcher, its interpreter or the helper changed during the launch".into(),
            route,
        );
    }
    let verify_exit = report.verify_exit;
    let (outcome, reason, mut evidence) =
        interpret_linux_result(report.launcher_exit, &out, &mut || verify_exit);
    evidence.push(policy_ref);
    LinuxRun {
        outcome,
        reason,
        evidence,
        out_dir: out,
        route,
    }
}

/// The DIRECT route (development and tests): Fabric executes the pinned
/// launcher itself, same-byte (D), under the pinned (or dev) interpreter. It
/// never yields protected evidence.
fn run_direct(
    lx: &LinuxProfileConfig,
    req: &ComputeRequest,
    policy: &GuestPolicy,
    psv: &crate::psv::Launch,
) -> LinuxRun {
    use crate::sealed_exec::{self, Lease, Pinned};
    let out = lx.out_root.join(req.operation_id.as_str());
    let route = LaunchRoute::Direct;
    // Beside `--out`, never in it: the launcher requires a new/empty out dir.
    let policy_file = lx
        .out_root
        .join(format!("{}.policy.json", req.operation_id.as_str()));
    let refused = |reason: String| LinuxRun {
        // The launcher was never invoked: nothing was acquired.
        outcome: LinuxOutcome::Refused,
        reason,
        evidence: vec![],
        out_dir: out.clone(),
        route,
    };
    // D: the launcher and its interpreter, opened and verified ONCE; these
    // descriptors are what runs, for the launch and for --verify-result.
    let launcher = match sealed_exec::open_verified(
        &Pinned {
            path: lx.launcher.clone(),
            sha256: lx.launcher_sha256.clone(),
        },
        lx.exec_owner,
        Lease::IfGranted,
    ) {
        Ok(v) => v,
        Err(e) => return refused(format!("launcher: {e}")),
    };
    let interp_pin = match &lx.interpreter {
        Some(p) => Ok(p.clone()),
        None => {
            let p = PathBuf::from("/bin/bash");
            sha256_file(&p).map(|sha256| Pinned { path: p, sha256 })
        }
    };
    let interpreter = match interp_pin
        .and_then(|p| sealed_exec::open_verified(&p, lx.exec_owner, Lease::IfGranted))
    {
        Ok(v) => v,
        Err(e) => return refused(format!("launcher interpreter: {e}")),
    };
    let written = std::fs::create_dir_all(&lx.out_root).and_then(|()| {
        use std::io::Write as _;
        // create_new: a pre-existing file (or symlink) is not ours to reuse.
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&policy_file)?
            .write_all(policy.json().as_bytes())
    });
    if let Err(e) = written {
        return refused(format!(
            "could not write the guest policy {}: {e}",
            policy_file.display()
        ));
    }
    let policy_ref = guest_policy_ref(policy);
    let timeout_s = req.limits.wall_time_ms.div_ceil(1000).max(1);
    let o = |p: &Path| p.as_os_str().to_os_string();
    let mut args: Vec<std::ffi::OsString> = vec!["--policy".into(), o(&policy_file)];
    // PSV: the candidate, the suite and the job are three separate read-only
    // drives; the manifest digest is Fabric's.
    args.extend([
        "--psv-candidate".into(),
        o(&psv.candidate_dir),
        "--psv-suite".into(),
        o(&psv.suite_dir),
        "--psv-job".into(),
        o(&psv.job_dir),
        "--psv-manifest-sha".into(),
        psv.digest.clone().into(),
        "--out".into(),
        o(&out),
        "--manifest".into(),
        o(&lx.manifest),
        "--timeout-s".into(),
        timeout_s.to_string().into(),
        "--id".into(),
        jail_id(req.operation_id.as_str()).into(),
    ]);
    // The launcher runs from a descriptor, so it cannot find its repository
    // from `$0`: name the artifacts dir it would have defaulted to.
    let artifacts = lx.artifacts_dir.clone().or_else(|| {
        lx.launcher
            .parent()
            .and_then(Path::parent)
            .map(|r| r.join("dist/guest-linux"))
    });
    if let Some(a) = artifacts {
        args.extend(["--artifacts-dir".into(), o(&a)]);
    }
    let env = [("PATH", LAUNCH_PATH)];
    // Nothing of the caller's environment (PATH, …) steers the pinned
    // launcher, for the launch and the verify step alike (review
    // wf_d725935a-7ed): the child runs under exactly `env`.
    let quiet = |cmd: Result<std::process::Command, String>| {
        cmd.and_then(|mut c| {
            c.stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map_err(|e| e.to_string())
        })
    };
    let exit = match quiet(sealed_exec::command(
        &launcher,
        Some(&interpreter),
        &args,
        &env,
        &[],
    )) {
        Ok(s) => s.code(),
        Err(e) => {
            return LinuxRun {
                // The launch record exists but the launcher never started:
                // nothing was acquired, yet we cannot prove that from here.
                outcome: LinuxOutcome::Unknown,
                reason: format!("could not run the launcher: {e}"),
                evidence: vec![policy_ref],
                out_dir: out,
                route,
            };
        }
    };
    // The job drive's source (the per-attempt secret) leaves the host the
    // moment the launcher returns — before any further child runs.
    psv.scrub();
    let out2 = out.clone();
    let mut verify = || {
        quiet(sealed_exec::command(
            &launcher,
            Some(&interpreter),
            &["--verify-result".into(), out2.clone().into_os_string()],
            &env,
            &[],
        ))
        .ok()
        .and_then(|s| s.code())
    };
    let (outcome, reason, mut evidence) = interpret_linux_result(exit, &out, &mut verify);
    evidence.push(policy_ref);
    LinuxRun {
        outcome,
        reason,
        evidence,
        out_dir: out,
        route,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `verify-evidence`'s `authoritative` flag, one condition at a time, each
    /// on the inputs where it is the ONLY one that can refuse. The CLI always
    /// runs as a test-trust build under test, so the production conditions are
    /// unreachable there (verify_evidence.rs checks the integrated answer).
    #[test]
    fn verify_evidence_is_authoritative_only_for_the_operators_owned_root_in_production() {
        let op = Path::new("/etc/axon/trust/qualification");
        let caller = Path::new("/tmp/caller-issuers");
        assert_eq!(verify_evidence_authority(false, op, op, Ok(())), Ok(()));
        let why = verify_evidence_authority(true, op, op, Ok(())).expect_err(
            "ATTACK: a test-trust build reported its answer as authoritative for the operator's \
             own, owned root",
        );
        assert!(why.contains("test-trust build"), "{why}");
        let why = verify_evidence_authority(false, caller, op, Ok(())).expect_err(
            "ATTACK: a caller-chosen --issuers root was reported as authoritative in a \
             production build",
        );
        assert!(why.contains("the caller's --issuers"), "{why}");
        let why = verify_evidence_authority(false, op, op, Err("group-writable".into()))
            .expect_err(
                "ATTACK: an operator root failing the ownership walk was reported as \
                 authoritative",
            );
        assert!(why.contains("ownership walk: group-writable"), "{why}");
    }

    /// ADR-001 D3: the loop counts a protected evaluation only on a backend in
    /// `PROTECTED_PROFILES`. That list is the loop's; the profiles are Fabric's.
    /// A renamed or added profile must be classified here, not by accident.
    #[test]
    fn protected_profiles_are_exactly_the_microvm_profile() {
        let listed: Vec<&str> = ALL
            .iter()
            .map(|p| p.id)
            .filter(|id| axon_loop_contracts::PROTECTED_PROFILES.contains(id))
            .collect();
        assert_eq!(listed, [LINUX_MICROVM_PROTECTED.id]);
        assert_eq!(
            axon_loop_contracts::PROTECTED_PROFILES,
            [LINUX_MICROVM_PROTECTED.id],
            "a protected profile the backend registry does not define"
        );
    }

    #[test]
    fn profiles_are_truthful() {
        const { assert!(!LOCAL_INTERPRETER.hardware_isolation) };
        assert_eq!(LOCAL_INTERPRETER.isolation.label(), "process_scoped");
        assert_eq!(FIRECRACKER_AXON_KERNEL.id, "axon-metal-fc-nojailer");
        assert_eq!(
            FIRECRACKER_AXON_KERNEL.os,
            Os::None,
            "the Axon kernel is not Linux"
        );
        assert!(FIRECRACKER_AXON_KERNEL.job_kinds.is_empty());
        const { assert!(!axon_vm::BACKEND_PROFILE.linux_guest) };
        assert_eq!(LINUX_MICROVM_PROTECTED.os, Os::Linux);
        // PSV (v022-psv-protocol.md §4): the profile runs registered checks,
        // but `submit` admits only an OPERATOR suite with a named test, judged
        // by the trusted guest runner (tests/psv_dispatch.rs).
        assert!(LINUX_MICROVM_PROTECTED
            .job_kinds
            .contains(&JobKind::RegisteredCheck));
    }

    /// A/B: only the privileged route attests protected, and a test-trust
    /// helper only inside a test-trust Fabric.
    #[test]
    fn only_a_production_helper_attests_protected_in_a_production_fabric() {
        assert!(
            !LaunchRoute::Privileged { test_build: true }.may_attest_protected(false),
            "ATTACK: a test-trust helper (which takes --test-config, a caller-chosen config) \
             attested a protected verdict in a production Fabric"
        );
        assert!(
            !LaunchRoute::Direct.may_attest_protected(false)
                && !LaunchRoute::Direct.may_attest_protected(true),
            "ATTACK: the direct (development) route may attest a protected verdict"
        );
        assert!(LaunchRoute::Privileged { test_build: false }.may_attest_protected(false));
        assert!(LaunchRoute::Privileged { test_build: true }.may_attest_protected(true));
    }

    /// PSV-4 (C9 round 3): the route a production Fabric derives from a
    /// helper's report. A report of a test-trust helper (which obeys
    /// `--test-config`, a config of the caller's choosing) must put the launch
    /// on a TEST route, and so never attest protected in a production Fabric
    /// (`may_attest_protected(false)`: the production value of
    /// `TEST_TRUST_BUILD`, which no `cargo test` build has). Any build name
    /// other than exactly `production` is a test build.
    #[test]
    fn a_test_trust_helpers_report_never_puts_a_launch_on_a_production_route() {
        let report = |build: &str| crate::privileged_launcher::LaunchReport {
            schema: crate::privileged_launcher::REPORT_SCHEMA.into(),
            build: build.into(),
            launched: true,
            error: None,
            launcher_sha256: None,
            interpreter_sha256: None,
            launcher_exit: Some(0),
            verify_exit: Some(0),
            unchanged: true,
        };
        for build in ["test-trust", "", "Production", "production ", "unknown"] {
            let route = LaunchRoute::of_report(&report(build));
            assert!(
                !route.may_attest_protected(false),
                "ATTACK: a helper reporting build {build:?} put the launch on a route that \
                 attests protected in a production Fabric ({route:?})"
            );
        }
        assert!(
            LaunchRoute::of_report(&report("production")).may_attest_protected(false),
            "control: a production helper's launch may attest protected"
        );
    }

    #[test]
    fn jail_ids_fit_the_launcher_pattern() {
        let id = jail_id("cortex-abc-1");
        assert!(id.len() <= 60);
        assert!(id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    }
}
