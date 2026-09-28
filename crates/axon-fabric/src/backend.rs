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
    // any other target on this profile.
    job_kinds: &[JobKind::InterpreterRun, JobKind::RegisteredCheck],
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
}

/// Default ceiling on the age of a qualification record: 30 days.
pub const DEFAULT_EVIDENCE_MAX_AGE_S: u64 = 30 * 24 * 3600;
/// `/2`: DOMAIN-SEPARATED. The signed message is
/// `axon-evidence-signature/2\n<authority>\n<exact bytes>`, and the signature
/// names its authority, so a key trusted for one purpose never validates a
/// statement of another, whatever directory it sits in. `/1` (bytes alone,
/// no domain) is refused: nothing operator-signed under it exists.
pub const EVIDENCE_SIGNATURE_SCHEMA: &str = "axon-evidence-signature/2";

/// The exact message an `axon-evidence-signature/2` for `authority` signs.
pub fn evidence_signing_message(authority: TrustAuthority, bytes: &[u8]) -> Vec<u8> {
    let mut m = format!("{EVIDENCE_SIGNATURE_SCHEMA}\n{}\n", authority.dir_name()).into_bytes();
    m.extend_from_slice(bytes);
    m
}
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

/// The operator's trust root, OUTSIDE any repository: root/custodian-owned,
/// never writable by an agent. The repository may declare the key ids it
/// expects, but it can never add authority (operator direction 2026-09-27:
/// "the repo should never be able to redefine both what counts as a valid
/// signature and which keys are trusted").
pub const OPERATOR_TRUST_ROOT: &str = "/etc/axon/trust";

/// One trust root PER AUTHORITY, so a key trusted for one purpose never becomes
/// valid for another: `/etc/axon/trust/<authority>/`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrustAuthority {
    /// B263 qualification records and protected-host certifications.
    Qualification,
    /// ADR-002 preflight observations.
    Observer,
    /// Fabric verifier (receipt attestation) keys.
    Verifier,
    /// Admission / transition authority.
    Admission,
}

impl TrustAuthority {
    pub const ALL: [TrustAuthority; 4] = [
        TrustAuthority::Qualification,
        TrustAuthority::Observer,
        TrustAuthority::Verifier,
        TrustAuthority::Admission,
    ];
    pub fn parse(s: &str) -> Option<TrustAuthority> {
        TrustAuthority::ALL.into_iter().find(|a| a.dir_name() == s)
    }
    pub fn dir_name(self) -> &'static str {
        match self {
            TrustAuthority::Qualification => "qualification",
            TrustAuthority::Observer => "observer",
            TrustAuthority::Verifier => "verifier",
            TrustAuthority::Admission => "admission",
        }
    }
    /// The fixed, absolute production path.
    pub fn operator_dir(self) -> PathBuf {
        Path::new(OPERATOR_TRUST_ROOT).join(self.dir_name())
    }
}

/// Whether this build carries the TEST-ONLY trust constructors.
pub const TEST_TRUST_BUILD: bool = cfg!(any(test, feature = "test-trust-root"));

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
        }
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
    use std::os::unix::fs::MetadataExt;
    if !dir.is_absolute() {
        return Err(format!(
            "{} is not an absolute path: a trust root is a fixed host path",
            dir.display()
        ));
    }
    let check = |p: &Path| -> Result<(), String> {
        let m = std::fs::symlink_metadata(p).map_err(|e| format!("{}: {e}", p.display()))?;
        if m.file_type().is_symlink() {
            return Err(format!(
                "{} is a symlink: a trust root is never redirected",
                p.display()
            ));
        }
        if m.uid() != 0 {
            return Err(format!(
                "{} is owned by uid {}, not root: an agent-writable trust root authorizes nothing",
                p.display(),
                m.uid()
            ));
        }
        if m.mode() & 0o022 != 0 {
            return Err(format!(
                "{} is group- or other-writable (mode {:o}): it authorizes nothing",
                p.display(),
                m.mode() & 0o7777
            ));
        }
        Ok(())
    };
    let rel = dir
        .strip_prefix(base)
        .map_err(|_| format!("{} is not below {}", dir.display(), base.display()))?;
    let mut p = base.to_path_buf();
    check(&p)?;
    for c in rel.components() {
        p.push(c);
        check(&p)?;
    }
    if entries && std::fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir()) {
        for e in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            check(&e.map_err(|e| e.to_string())?.path())?;
        }
    }
    Ok(())
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

pub(crate) fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(format!("{:x}", Sha256::digest(&b)))
}

pub(crate) fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

fn hex_decode(s: &str) -> Option<Vec<u8>> {
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

/// Load the trusted issuer public keys. A malformed key file is an error
/// (fail closed), never skipped; no keys at all is an error too.
fn trusted_issuers(dir: &Path) -> Result<Vec<Vec<u8>>, String> {
    let mut keys = Vec::new();
    if let Ok(rd) = std::fs::read_dir(dir) {
        let mut paths: Vec<PathBuf> = rd.filter_map(|e| e.ok().map(|e| e.path())).collect();
        paths.sort();
        for p in paths {
            if p.extension().and_then(|e| e.to_str()) != Some("pub") {
                continue;
            }
            let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
            match hex_decode(&t) {
                Some(k) if k.len() == 32 => keys.push(k),
                _ => {
                    return Err(format!(
                        "trusted issuer key {} is not a 64-hex-char Ed25519 public key",
                        p.display()
                    ))
                }
            }
        }
    }
    if keys.is_empty() {
        return Err(format!(
            "no trusted evidence issuer is configured ({} holds no *.pub key); unsigned or \
             self-authored evidence cannot qualify the profile",
            dir.display()
        ));
    }
    Ok(keys)
}

fn fingerprint(pk: &[u8]) -> String {
    format!("ed25519:{}", &sha256_hex(pk)[..16])
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
    use ring::signature::{UnparsedPublicKey, ED25519};
    let sig_file = match std::fs::read_to_string(sig_path) {
        Ok(t) => Some(t),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("{what} signature {}: {e}", sig_path.display())),
    };
    // RULE:unsigned
    if sig_file.is_none() {
        return Err(format!(
            "{what} is unsigned: no detached signature at {}",
            sig_path.display()
        ));
    }
    let mut issuer = String::from("unsigned");
    if let Some(t) = sig_file {
        let sv: serde_json::Value =
            serde_json::from_str(&t).map_err(|e| format!("{what} signature is not JSON: {e}"))?;
        if sv["schema"] != EVIDENCE_SIGNATURE_SCHEMA || sv["alg"] != "ed25519" {
            return Err(format!(
                "{what} signature is not {EVIDENCE_SIGNATURE_SCHEMA} with alg ed25519"
            ));
        }
        // RULE:authority-domain
        if sv["domain"] != authority.dir_name() {
            return Err(format!(
                "{what} signature is for authority {}, not {}: a key trusted for one purpose \
                 never validates another",
                sv["domain"],
                authority.dir_name()
            ));
        }
        let pk = sv["public_key"]
            .as_str()
            .and_then(hex_decode)
            .filter(|k| k.len() == 32)
            .ok_or(format!("{what} signature has no 32-byte public_key"))?;
        let sig = sv["signature"]
            .as_str()
            .and_then(hex_decode)
            .filter(|s| s.len() == 64)
            .ok_or(format!("{what} signature has no 64-byte signature"))?;
        // RULE:issuer-trusted
        if !trusted.contains(&pk) {
            return Err(format!(
                "{what} is signed by {}, which is not a trusted evidence issuer",
                fingerprint(&pk)
            ));
        }
        // RULE:signature-verifies
        if UnparsedPublicKey::new(&ED25519, &pk)
            .verify(&evidence_signing_message(authority, bytes), &sig)
            .is_err()
        {
            return Err(format!("{what} signature does not verify under {}: the bytes are not the ones the issuer signed", fingerprint(&pk)));
        }
        issuer = fingerprint(&pk);
    }
    Ok(issuer)
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
    let bytes = std::fs::read(record).map_err(|e| format!("evidence {}: {e}", record.display()))?;
    let trusted = trusted_issuers(issuers_dir)?;
    verify_detached("evidence", &bytes, sig, &trusted, authority)
}

fn sidecar_sig(p: &Path) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(".sig");
    PathBuf::from(s)
}

struct Waiver {
    reason: String,
    expires: Option<i64>,
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
    /// A changed manifest is ineligible — never "probably fine".
    pub fn qualification(&self) -> Result<LinuxQualification, String> {
        let launcher_sha256 = self.launcher_pinned()?;
        let manifest_sha256 = sha256_file(&self.manifest)?;
        let ev_bytes = std::fs::read(&self.evidence)
            .map_err(|e| format!("evidence {}: {e}", self.evidence.display()))?;
        let evidence_sha256 = sha256_hex(&ev_bytes);
        // Authenticity first: nothing in an unauthenticated record is read
        // as a claim.
        if self.trust.operator_owned {
            check_operator_owned(&self.trust.issuers_dir)?;
        }
        let trusted = trusted_issuers(&self.trust.issuers_dir)?;
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
        // RULE:issuer-claimed: the record names the key it is issued under, and
        // that is the key that verified it — a record signed by one trusted
        // issuer cannot pass as another's.
        if ev["issuer_key_id"].as_str() != Some(issuer.as_str()) {
            return Err(format!(
                "evidence record claims issuer_key_id {} but is signed by {issuer}",
                ev["issuer_key_id"]
            ));
        }
        let now = self.trust.clock.now_unix();

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
        let mut waivers = std::collections::BTreeMap::<String, Waiver>::new();
        if let (false, Some(wp)) = (blocked.is_empty(), &self.waivers) {
            let wb = std::fs::read(wp).map_err(|e| format!("waivers {}: {e}", wp.display()))?;
            verify_detached(
                "waiver file",
                &wb,
                &sidecar_sig(wp),
                &trusted,
                TrustAuthority::Qualification,
            )?;
            let w: serde_json::Value =
                serde_json::from_slice(&wb).map_err(|e| format!("waivers are not JSON: {e}"))?;
            if w["schema"] != WAIVER_SCHEMA {
                return Err(format!("waiver file schema is not {WAIVER_SCHEMA}"));
            }
            // RULE:waiver-bound
            if w["evidence_sha256"].as_str() != Some(evidence_sha256.as_str()) {
                return Err("waiver file is bound to a different evidence record; a waiver is not transferable".into());
            }
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
        }
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
        if (now - end) as u64 > self.trust.max_age_s {
            return Err(format!(
                "evidence end {end_s} is stale (older than {} s)",
                self.trust.max_age_s
            ));
        }

        // ── Engine, source, host ────────────────────────────────────────────
        let m: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&self.manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("manifest is not JSON: {e}"))?;
        let eng = &ev["engine"];
        // RULE:engine-digests
        if !is_hex64(&eng["firecracker_sha256"]) || !is_hex64(&eng["jailer_sha256"]) {
            return Err("evidence record lacks engine.firecracker_sha256 / engine.jailer_sha256; an unidentified VMM qualifies nothing".into());
        }
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
        // RULE:tree-clean
        if ev["source"]["tree_dirty"] != serde_json::Value::Bool(false) {
            return Err("evidence was produced from a dirty (or unstated) source tree".into());
        }
        // RULE:manifest-clean
        if m["source"]["axon_tree_dirty_at_build"] != serde_json::Value::Bool(false) {
            return Err("manifest artifacts were built from a dirty (or unstated) tree".into());
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
            host,
            caveat,
            end: end_s,
            firecracker_sha256: eng["firecracker_sha256"].as_str().unwrap_or("").into(),
            jailer_sha256: eng["jailer_sha256"].as_str().unwrap_or("").into(),
            waived: blocked,
            guest_policy_channel,
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
                "{}: job_kind {:?} unsupported — the profile runs one program with `axon run` and \
                 reports its exit; it does not run registered checks or produce a verdict",
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
    if let Ok(s) = sha256_file(&rj) {
        evidence.push(format!("sha256-result-json:{s}"));
    }
    let r: Option<serde_json::Value> = std::fs::read_to_string(&rj)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok());
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

/// Run one `interpreter_run` through the Linux profile launcher, delivering
/// `policy` to the guest with `--policy FILE`.
pub fn run_linux_profile(
    lx: &LinuxProfileConfig,
    program: &Path,
    req: &ComputeRequest,
    policy: &GuestPolicy,
    psv: Option<&crate::psv::Launch>,
) -> LinuxRun {
    let out = lx.out_root.join(req.operation_id.as_str());
    // Beside `--out`, never in it: the launcher requires a new/empty out dir.
    let policy_file = lx
        .out_root
        .join(format!("{}.policy.json", req.operation_id.as_str()));
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
        return LinuxRun {
            // The launcher was never invoked: nothing was acquired.
            outcome: LinuxOutcome::Refused,
            reason: format!(
                "could not write the guest policy {}: {e}",
                policy_file.display()
            ),
            evidence: vec![],
            out_dir: out,
        };
    }
    let policy_ref = format!(
        "guest-policy-sha256:{}",
        sha256_hex(policy.json().as_bytes())
    );
    let timeout_s = req.limits.wall_time_ms.div_ceil(1000).max(1);
    let mut cmd = std::process::Command::new(&lx.launcher);
    cmd.arg("--policy").arg(&policy_file);
    match psv {
        // PSV: the candidate, the suite and the job are three separate
        // read-only drives; the manifest digest is Fabric's.
        Some(l) => cmd
            .arg("--psv-candidate")
            .arg(&l.candidate_dir)
            .arg("--psv-suite")
            .arg(&l.suite_dir)
            .arg("--psv-job")
            .arg(&l.job_dir)
            .arg("--psv-manifest-sha")
            .arg(&l.digest),
        None => cmd.arg("--program").arg(program),
    };
    cmd.arg("--out")
        .arg(&out)
        .arg("--manifest")
        .arg(&lx.manifest)
        .arg("--timeout-s")
        .arg(timeout_s.to_string())
        .arg("--id")
        .arg(jail_id(req.operation_id.as_str()))
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin:/usr/local/bin");
    if let Some(a) = &lx.artifacts_dir {
        cmd.arg("--artifacts-dir").arg(a);
    }
    let exit = match cmd
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
    {
        Ok(s) => s.code(),
        Err(e) => {
            return LinuxRun {
                // The launch record exists but the launcher never started:
                // nothing was acquired, yet we cannot prove that from here.
                outcome: LinuxOutcome::Unknown,
                reason: format!("could not run the launcher: {e}"),
                evidence: vec![policy_ref],
                out_dir: out,
            };
        }
    };
    let launcher = lx.launcher.clone();
    let out2 = out.clone();
    let mut verify = move || {
        std::process::Command::new(&launcher)
            .arg("--verify-result")
            .arg(&out2)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
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
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn jail_ids_fit_the_launcher_pattern() {
        let id = jail_id("cortex-abc-1");
        assert!(id.len() <= 60);
        assert!(id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    }
}
