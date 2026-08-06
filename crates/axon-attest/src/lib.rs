//! axon-attest — R26 Confidential Micro-VM Substrate attestation library.
//!
//! Provides the pure, I/O-free core for measuring, signing, and verifying Axon
//! OS guest kernel images with a software-TPM stand-in (for CI) or real
//! SEV-SNP/TDX hardware (hw-attest feature, gated separately).
//!
//! **Stand-in honesty:** `hw_root = "software-tpm-v1"` means this is NOT real
//! hardware attestation — no memory encryption vs the host operator. Use
//! SEV-SNP/TDX (hw-attest feature) for true confidential computing.
//!
//! # Attestation chain (R26 §4.2)
//! ```text
//! kernel_bytes
//!   → SHA-256 → measurement.digest   (image content-address)
//!   → SHA-256(kernel_bytes ‖ "axtcb1:") → axtcb1:…  (TCB chain prefix)
//! sign_report(measurement, key) → HMAC-SHA256(key, digest ‖ axtcb1) → signature
//! verify_report(report, expected_digest, expected_axtcb1)
//!   → checks digest + axtcb1 + non-empty signature (fail-closed)
//! ```
//!
//! All functions in this module are pure (no I/O, no clock, no random),
//! except `measure_kernel` which reads a file — keeping `verify` small and total.

use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};

// ── Public types ──────────────────────────────────────────────────────────────

/// Label for the software-TPM stand-in (no real hardware attestation).
///
/// An `AttestationReport` with this `hw_root` provides the attestation *shape*
/// (measured boot, signed quote, the §4.2 axtcb1 binding, the §4.3
/// verification) but does **not** provide memory-encryption vs the host.
/// Only SEV-SNP/TDX (`hw_root = "sev-snp"` / `"tdx"`) delivers confidentiality.
pub const SOFTWARE_TPM_HW_ROOT: &str = "software-tpm-v1";

/// The `axtcb1:` prefix constant — Axon TCB digest domain separator.
const AXTCB_PREFIX: &[u8] = b"axtcb1:";

/// A deterministic measurement of a guest kernel image.
///
/// Same image bytes ⇒ byte-identical `GuestMeasurement` (A5 invariant).
/// The `timestamp` field is 0 in the pure path; callers may set it for audit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GuestMeasurement {
    /// SHA-256 of the kernel ELF content, as raw bytes.
    pub digest: [u8; 32],
    /// Axon TCB digest: `"axtcb1:" + hex(sha256(kernel_bytes ‖ b"axtcb1:"))`.
    ///
    /// Chained to the kernel content — different image ⇒ different axtcb1.
    /// This is the in-language `axtcb1:` format used by R20 Slice 3.
    pub axtcb1: String,
    /// Unix timestamp (seconds). 0 in the pure/reproducible path.
    pub timestamp: u64,
}

/// An attestation report from the software-TPM stand-in.
///
/// `hw_root = "software-tpm-v1"`: software stand-in, NOT real hardware.
///
/// The `signature` is HMAC-SHA256(key, digest ‖ axtcb1_bytes), binding the
/// measurement and the TCB chain together under a single MAC. In real hardware
/// (SEV-SNP/TDX) this would be the hardware-signed attestation report.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    /// The measured kernel image.
    pub measurement: GuestMeasurement,
    /// HMAC-SHA256(key, digest ‖ axtcb1_bytes). Empty ⇒ report is unsigned ⇒ refused.
    pub signature: Vec<u8>,
    /// `"software-tpm-v1"` for the stand-in; `"sev-snp"` / `"tdx"` for real hardware.
    pub hw_root: String,
}

/// The minimal, closed device allowlist for a confined guest (R26 §3.3).
///
/// Exactly: vsock (job channel), serial (diagnostics), timer (scheduler).
/// Any extra device is a `DeviceDeny` — not representable in this type.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DeviceSet {
    /// virtio-vsock — the only job/result channel.
    pub vsock: bool,
    /// serial console — boot diagnostics + panic handler.
    pub serial: bool,
    /// monotonic timer — cooperative scheduler + wall-clock timeout.
    pub timer: bool,
}

impl DeviceSet {
    /// The required minimal device surface: vsock + serial + timer.
    pub fn minimal() -> Self {
        DeviceSet { vsock: true, serial: true, timer: true }
    }

    /// Validate that this set has exactly the three required devices.
    pub fn validate(&self) -> Result<(), String> {
        if !self.vsock {
            return Err("DeviceSet missing required device: vsock (job channel)".to_string());
        }
        if !self.serial {
            return Err("DeviceSet missing required device: serial (diagnostics)".to_string());
        }
        if !self.timer {
            return Err("DeviceSet missing required device: timer (scheduler)".to_string());
        }
        Ok(())
    }

    /// Validate that a named device is in the allowlist.
    ///
    /// Returns `Err` for any device not in {vsock, serial, timer} — these are
    /// `DeviceDeny` (exit 8) per R26 §3.4. A manifest requesting a GPU, virtio-net,
    /// extra block device, etc. is rejected before any VM launches.
    pub fn validate_manifest_extra_device(device: &str) -> Result<(), String> {
        match device {
            "vsock" | "serial" | "timer" => Ok(()),
            other => Err(format!(
                "device '{other}' is not in the R26 device allowlist \
                 (only vsock, serial, timer permitted — §3.3); \
                 DeviceDeny exit 8"
            )),
        }
    }
}

// ── Core API — pure functions ─────────────────────────────────────────────────

/// Measure a guest kernel image from bytes (pure — no I/O).
///
/// Same bytes ⇒ byte-identical `GuestMeasurement` (A5).
/// `timestamp` is 0; set it after the call if audit logging is needed.
pub fn measure_kernel_bytes(bytes: &[u8]) -> GuestMeasurement {
    // measurement.digest = SHA-256(kernel_bytes)
    let digest_array: [u8; 32] = Sha256::digest(bytes).into();

    // axtcb1 = "axtcb1:" + hex(sha256(kernel_bytes ‖ b"axtcb1:"))
    // This chains the TCB prefix into the image content-address, binding the
    // in-language R20 `axtcb1:` format to the exact kernel bytes.
    let mut h = Sha256::new();
    h.update(bytes);
    h.update(AXTCB_PREFIX);
    let tcb_hash: [u8; 32] = h.finalize().into();
    let axtcb1 = format!("axtcb1:{}", hex::encode(tcb_hash));

    GuestMeasurement { digest: digest_array, axtcb1, timestamp: 0 }
}

/// Measure a guest kernel image from a file path.
///
/// Reads the file, then calls `measure_kernel_bytes`. Same file content ⇒
/// same measurement (A5 determinism).
pub fn measure_kernel(kernel_path: &std::path::Path)
    -> Result<GuestMeasurement, Box<dyn std::error::Error>>
{
    let bytes = std::fs::read(kernel_path)?;
    Ok(measure_kernel_bytes(&bytes))
}

/// Produce an attestation report by signing the measurement with a key.
///
/// Uses HMAC-SHA256(key, digest ‖ axtcb1_bytes) as the software-TPM signature.
/// `hw_root = "software-tpm-v1"` — explicitly NOT real hardware attestation.
///
/// Deterministic: same `m` + same `key` ⇒ same `AttestationReport.signature`.
pub fn sign_report(m: GuestMeasurement, key: &[u8]) -> AttestationReport {
    let mut data = Vec::with_capacity(32 + m.axtcb1.len());
    data.extend_from_slice(&m.digest);
    data.extend_from_slice(m.axtcb1.as_bytes());
    let signature = hmac_sha256(key, &data).to_vec();

    AttestationReport {
        measurement: m,
        signature,
        hw_root: SOFTWARE_TPM_HW_ROOT.to_string(),
    }
}

/// Verify an attestation report (the relying-party verifier — the trusted artifact).
///
/// Fail-closed: the first failure returns `Err`. No path returns `Ok` without
/// passing all checks. No I/O, no solver, no hardware call — purely deterministic.
///
/// Checks (in order, per R26 §4.3):
/// 1. Signature is non-empty (no-attestation ⇒ refused, not degraded).
/// 2. `report.measurement.digest == expected_digest` (tamper detection).
/// 3. `report.measurement.axtcb1 == expected_axtcb1` (TCB chain check).
///
/// For the software stand-in, the structural checks (2+3) are the load-bearing
/// verification — the HMAC key is not re-checked here (the operator owns the
/// stand-in's EK; §8 is honest about this). Only real hardware (hw-attest)
/// provides cryptographic binding beyond operator control.
pub fn verify_report(
    report: &AttestationReport,
    expected_digest: &[u8; 32],
    expected_axtcb1: &str,
) -> Result<(), String> {
    // Step 1 (Core): no-attestation ⇒ refused (Core `vm_without_attestation_is_refused`)
    if report.signature.is_empty() {
        return Err(
            "no attestation signature present — report is unsigned; \
             vm_without_attestation_is_refused (exit 10)"
                .to_string(),
        );
    }

    // Step 2: measurement digest must match the pinned expectation (A6, Core)
    if report.measurement.digest != *expected_digest {
        return Err(format!(
            "attestation_mismatch_fails_closed: measurement ≠ expected \
             (tampered/wrong image); got={}, expected={}",
            hex::encode(report.measurement.digest),
            hex::encode(expected_digest),
        ));
    }

    // Step 3: axtcb1 must match the pinned expectation (the chain, A6)
    if report.measurement.axtcb1 != expected_axtcb1 {
        return Err(format!(
            "axtcb1_digest_chained_to_measurement: TCB chain mismatch; \
             got={}, expected={expected_axtcb1}",
            report.measurement.axtcb1,
        ));
    }

    // Step 4 (AUDIT T13, part of finding OSK-P7-C1): `hw_root` was never read
    // by this function AT ALL. Since nothing here recomputes the HMAC — the
    // software stand-in's key is operator-owned, documented above — a report
    // could simply CLAIM `hw_root = "sev-snp"` and verify identically to a real
    // hardware quote. That turns the one field distinguishing "no
    // memory-encryption vs the host" from "confidential computing" into an
    // unchecked self-assertion.
    //
    // This build has no hardware-attestation backend compiled in, so the ONLY
    // hw_root it can honestly verify is the software stand-in. A hardware claim
    // is refused rather than accepted on trust. Closing this needs no key
    // decision; the cryptographic binding (recomputing the HMAC against a key
    // the VERIFIER holds) still does — see O007-adjacent notes and §8.
    if report.hw_root != SOFTWARE_TPM_HW_ROOT {
        return Err(format!(
            "unverifiable hw_root claim: report claims `{}`, but this build has \
             no hardware-attestation backend and cannot verify anything beyond \
             `{SOFTWARE_TPM_HW_ROOT}`. Refusing rather than accepting a \
             hardware root of trust on the report's own say-so.",
            report.hw_root,
        ));
    }

    Ok(())
}

/// Admit a job to an attested guest — MANDATORY attestation gate (R26 §4.1 step 5).
///
/// Returns `Err` if attestation doesn't verify — no job is sent to an unattested
/// guest. Only if `verify_report` passes is `simulate_job_run` invoked.
///
/// In CI/mock mode (`AXON_CI_NO_KVM=1`), `job` and `seed` are hashed to produce
/// a deterministic `RunRecord` digest (A5: same job+seed ⇒ same record).
pub fn try_admit_job(
    report: &AttestationReport,
    expected_digest: &[u8; 32],
    expected_axtcb1: &str,
    job: &[u8],
    seed: u64,
) -> Result<String, String> {
    // MANDATORY: verify attestation before admitting any work
    verify_report(report, expected_digest, expected_axtcb1)?;
    // Only if attestation is verified do we run the job
    Ok(simulate_job_run(job, seed))
}

/// Simulate a deterministic job run (mock — no real VM in CI).
///
/// Returns a `RunRecord` digest: `"axrec1:" + hex(sha256(job ‖ seed_le))`.
/// Same `job` + same `seed` ⇒ byte-identical output (A5).
pub fn simulate_job_run(job: &[u8], seed: u64) -> String {
    let mut h = Sha256::new();
    h.update(job);
    h.update(seed.to_le_bytes());
    format!("axrec1:{}", hex::encode(h.finalize()))
}

/// Serialize an `AttestationReport` to the canonical JSON format.
///
/// Schema: `axon-vm-report/1`. The `signature` is base64-encoded; `digest` is
/// hex-encoded. This is what `axon-vm attest` outputs (§5.2).
pub fn report_to_json(report: &AttestationReport) -> String {
    use base64::Engine as _;
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&report.signature);
    serde_json::to_string_pretty(&serde_json::json!({
        "schema": "axon-vm-report/1",
        "hw_root": report.hw_root,
        "substrate": "qemu-swtpm (stand-in — no memory encryption; use sev-snp/tdx for confidentiality)",
        "measurement": {
            "digest": hex::encode(report.measurement.digest),
            "axtcb1": report.measurement.axtcb1,
            "timestamp": report.measurement.timestamp,
        },
        "signature": sig_b64,
    }))
    .unwrap()
}

// ── R31: Extended TCB attestation ─────────────────────────────────────────────

/// Version tag appended after the four inner-hashes before the outer SHA-256.
/// Prevents length-extension attacks and versions the chaining protocol.
const EXT_VERSION_TAG: &[u8] = b"axon-tcb-v1\n";

/// Prefix for the extended TCB digest — distinguishes it from R26's `"axtcb1:"`.
const AXTCB_EXT_PREFIX: &str = "axtcb1-ext:";

// ── R31 error types ───────────────────────────────────────────────────────────

/// Error returned by `measure_extended` / `measure_host_stack` (fail-closed).
#[derive(Debug)]
pub enum MeasureError {
    /// A required component (`kernel` or `axon-os`) is absent or not found.
    ComponentMissing(String),
    /// A component path is not a regular file (e.g., symlink, directory — TOCTOU guard).
    ComponentNotRegularFile,
    /// A component file could not be read (I/O error).
    IoError(String),
}

impl std::fmt::Display for MeasureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MeasureError::ComponentMissing(n) => write!(f, "required component missing: {n}"),
            MeasureError::ComponentNotRegularFile => {
                write!(f, "component path is not a regular file (symlink / directory refused)")
            }
            MeasureError::IoError(e) => write!(f, "I/O error reading component: {e}"),
        }
    }
}

impl std::error::Error for MeasureError {}

/// Error returned by `verify_extended`.
#[derive(Debug)]
pub enum VerifyError {
    /// The `axtcb1_ext` string does not start with `"axtcb1-ext:"` (I-3 invariant).
    PrefixMismatch,
    /// The monitor slot's sha256 ≠ the axon-os slot's sha256 (I-2 invariant).
    MonitorSlotMismatch,
    /// The extended digest does not match the expected value.
    DigestMismatch { got: String, expected: String },
    /// The `components` array is malformed (wrong count or order).
    Malformed(String),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::PrefixMismatch => {
                write!(f, "axtcb1_ext prefix mismatch: must start with 'axtcb1-ext:'")
            }
            VerifyError::MonitorSlotMismatch => {
                write!(f, "monitor.sha256 ≠ axon-os.sha256 (I-2 invariant violated)")
            }
            VerifyError::DigestMismatch { got, expected } => {
                write!(f, "axtcb1_ext mismatch: got={got}, expected={expected}")
            }
            VerifyError::Malformed(msg) => write!(f, "malformed extended measurement: {msg}"),
        }
    }
}

impl std::error::Error for VerifyError {}

// ── R31 data model ────────────────────────────────────────────────────────────

/// A single measured component in the extended TCB (§3.1).
///
/// Four entries are always present in canonical order:
/// `["kernel", "axon-os", "axon-audit", "monitor"]`.
/// The `monitor` slot is special — R29 is embedded in `axon-os`, so its `sha256`
/// equals the `axon-os` sha256, `path` = `"(embedded in axon-os)"`, `size` = 0.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ComponentEntry {
    /// Canonical slot name: `"kernel"` | `"axon-os"` | `"axon-audit"` | `"monitor"`
    pub name: String,
    /// Filesystem path at measurement time (or `"(embedded in axon-os)"` for monitor,
    /// or `"<absent>"` for the R28-pending zero-fill axon-audit sentinel).
    pub path: String,
    /// Byte count of the measured binary (0 for the monitor slot and absent axon-audit).
    pub size: u64,
    /// Hex-encoded SHA-256 of the binary bytes (= intermediate hash used in the chain).
    pub sha256: String,
}

/// Input byte slices for `measure_extended` — the pure (I/O-free) measurement core.
///
/// The CLI layer reads files and injects them here; `measure_extended` itself
/// never touches the filesystem (same discipline as R26's `verify_report`).
pub struct ComponentPaths {
    /// Kernel image bytes — always required.
    pub kernel: Vec<u8>,
    /// `axon-os` binary bytes — always required.
    pub axon_os: Vec<u8>,
    /// `axon-audit` binary bytes — `None` triggers the R28-pending zero-fill sentinel.
    pub axon_audit: Option<Vec<u8>>,
    /// Path string recorded in the report for audit purposes.
    pub kernel_path: String,
    /// Path string recorded in the report for audit purposes.
    pub axon_os_path: String,
    /// Path string recorded in the report (None → `"<absent>"` in the entry).
    pub axon_audit_path: Option<String>,
}

/// Extended TCB measurement chaining all four safety-stack components.
#[derive(Debug, Clone)]
pub struct ExtendedMeasurement {
    /// R26 kernel measurement — preserved in full (I-4 backward compat).
    pub base: GuestMeasurement,
    /// Four `ComponentEntry` records in canonical order (I-1).
    pub components: Vec<ComponentEntry>,
    /// `"axtcb1-ext:" + hex(sha256(chain))` — the single extended chain digest.
    pub axtcb1_ext: String,
}

// ── R31 pure measurement core ─────────────────────────────────────────────────

/// Measure the full host safety stack (pure after byte injection).
///
/// **Canonical order:** kernel → axon-os → axon-audit → monitor (axon-os again).
///
/// **Chain construction (§4.1):**
/// ```text
/// axtcb1_ext = "axtcb1-ext:" + hex( sha256(
///     sha256(kernel_bytes)       ‖   // slot 0 — REQUIRED
///     sha256(axon_os_bytes)      ‖   // slot 1 — REQUIRED
///     sha256(axon_audit_bytes)   ‖   // slot 2 — [0u8;32] if absent (R28 pending)
///     sha256(axon_os_bytes)      ‖   // slot 3 — monitor = axon-os (embedded thread)
///     b"axon-tcb-v1\n"               // version tag (length-extension guard)
/// ))
/// ```
///
/// **Fail-closed (§4.2):** missing `kernel` or `axon-os` → `Err(ComponentMissing)`.
/// Missing `axon-audit` → `[0u8; 32]` zero-fill sentinel (R28-pending; auditable).
pub fn measure_extended(paths: ComponentPaths) -> Result<ExtendedMeasurement, MeasureError> {
    // ── inner hashes (one per slot) ──────────────────────────────────────────
    let kernel_hash: [u8; 32]    = Sha256::digest(&paths.kernel).into();
    let axon_os_hash: [u8; 32]   = Sha256::digest(&paths.axon_os).into();
    let axon_audit_hash: [u8; 32] = match &paths.axon_audit {
        Some(b) => Sha256::digest(b).into(),
        None    => [0u8; 32], // R28-pending sentinel — deliberate, auditable zero-fill
    };
    // Slot 3 (monitor) uses the same bytes as axon-os — embedded thread (§4.1).
    let monitor_hash: [u8; 32] = axon_os_hash;

    // ── outer chain: concat inner-hashes + version tag, then sha256 ─────────
    let mut chain: Vec<u8> = Vec::with_capacity(32 * 4 + EXT_VERSION_TAG.len());
    chain.extend_from_slice(&kernel_hash);
    chain.extend_from_slice(&axon_os_hash);
    chain.extend_from_slice(&axon_audit_hash);
    chain.extend_from_slice(&monitor_hash);
    chain.extend_from_slice(EXT_VERSION_TAG);
    let outer: [u8; 32] = Sha256::digest(&chain).into();
    let axtcb1_ext = format!("{}{}", AXTCB_EXT_PREFIX, hex::encode(outer));

    // ── build ComponentEntry records in canonical order (I-1) ────────────────
    let axon_os_sha256_hex = hex::encode(axon_os_hash);
    let components = vec![
        ComponentEntry {
            name:   "kernel".to_string(),
            path:   paths.kernel_path,
            size:   paths.kernel.len() as u64,
            sha256: hex::encode(kernel_hash),
        },
        ComponentEntry {
            name:   "axon-os".to_string(),
            path:   paths.axon_os_path,
            size:   paths.axon_os.len() as u64,
            sha256: axon_os_sha256_hex.clone(),
        },
        ComponentEntry {
            name:   "axon-audit".to_string(),
            // When bytes are absent (zero-fill sentinel), path is always "<absent>"
            // regardless of what was passed in axon_audit_path (§4.2).
            path:   if paths.axon_audit.is_some() {
                        paths.axon_audit_path.unwrap_or_else(|| "<absent>".to_string())
                    } else {
                        "<absent>".to_string()
                    },
            size:   paths.axon_audit.as_ref().map(|b| b.len() as u64).unwrap_or(0),
            sha256: hex::encode(axon_audit_hash),
        },
        ComponentEntry {
            // I-2: monitor.sha256 == axon-os.sha256 (same binary, embedded thread)
            name:   "monitor".to_string(),
            path:   "(embedded in axon-os)".to_string(),
            size:   0,
            sha256: axon_os_sha256_hex,
        },
    ];

    // R26 base measurement preserved (I-4).
    let base = measure_kernel_bytes(&paths.kernel);
    Ok(ExtendedMeasurement { base, components, axtcb1_ext })
}

// ── R31 I/O layer: file-reading wrapper ──────────────────────────────────────

/// Internal error for the file-reading path.
enum ComponentReadError { Missing, NotRegularFile, Io(String) }

fn read_component_file(path: &std::path::Path) -> Result<Vec<u8>, ComponentReadError> {
    if !path.exists() {
        return Err(ComponentReadError::Missing);
    }
    // Refuse symlinks — classic TOCTOU vector (§4.2).
    let meta = std::fs::symlink_metadata(path)
        .map_err(|e| ComponentReadError::Io(e.to_string()))?;
    if meta.file_type().is_symlink() || !meta.file_type().is_file() {
        return Err(ComponentReadError::NotRegularFile);
    }
    std::fs::read(path).map_err(|e| ComponentReadError::Io(e.to_string()))
}

fn map_read_err(e: ComponentReadError, name: &str) -> MeasureError {
    match e {
        ComponentReadError::Missing        => MeasureError::ComponentMissing(name.to_string()),
        ComponentReadError::NotRegularFile => MeasureError::ComponentNotRegularFile,
        ComponentReadError::Io(msg)        => MeasureError::IoError(msg),
    }
}

/// Measure the full host safety stack from file paths (the I/O seam, §2).
///
/// Reads each component file, checks for symlinks, then calls `measure_extended`.
/// Canonical order: kernel → axon-os → axon-audit → monitor (= axon-os thread).
///
/// - `kernel_path`      — always required; Err if missing/unreadable.
/// - `axon_os_path`     — always required; Err if missing/unreadable.
/// - `axon_audit_path`  — optional (R28 pending); None → zero-fill sentinel.
pub fn measure_host_stack(
    kernel_path:     &std::path::Path,
    axon_os_path:    Option<&std::path::Path>,
    axon_audit_path: Option<&std::path::Path>,
) -> Result<ExtendedMeasurement, MeasureError> {
    // kernel: always required
    let kernel_bytes = read_component_file(kernel_path)
        .map_err(|e| map_read_err(e, "kernel"))?;

    // axon-os: always required
    let aop = axon_os_path
        .ok_or_else(|| MeasureError::ComponentMissing("axon-os".to_string()))?;
    let axon_os_bytes = read_component_file(aop)
        .map_err(|e| map_read_err(e, "axon-os"))?;

    // axon-audit: optional (None → zero-fill)
    let (axon_audit_bytes, axon_audit_path_str) = match axon_audit_path {
        Some(p) => {
            let b = read_component_file(p)
                .map_err(|e| map_read_err(e, "axon-audit"))?;
            (Some(b), Some(p.to_string_lossy().to_string()))
        }
        None => (None, None),
    };

    measure_extended(ComponentPaths {
        kernel:           kernel_bytes,
        axon_os:          axon_os_bytes,
        axon_audit:       axon_audit_bytes,
        kernel_path:      kernel_path.to_string_lossy().to_string(),
        axon_os_path:     aop.to_string_lossy().to_string(),
        axon_audit_path:  axon_audit_path_str,
    })
}

// ── R31 verifier ─────────────────────────────────────────────────────────────

/// Verify an extended measurement against an expected `axtcb1_ext` value.
///
/// Checks (in order):
/// 1. **I-3 prefix**: both the expected and measured strings start with `"axtcb1-ext:"`.
/// 2. **I-1 structure**: exactly 4 components in canonical order.
/// 3. **I-2 monitor derivation**: `monitor.sha256 == axon-os.sha256` (re-derived; never trusts report).
/// 4. **Digest match**: `measured.axtcb1_ext == expected_axtcb1_ext`.
pub fn verify_extended(
    measured: &ExtendedMeasurement,
    expected_axtcb1_ext: &str,
) -> Result<(), VerifyError> {
    // I-3: both strings must carry the "axtcb1-ext:" prefix
    if !expected_axtcb1_ext.starts_with(AXTCB_EXT_PREFIX) {
        return Err(VerifyError::PrefixMismatch);
    }
    if !measured.axtcb1_ext.starts_with(AXTCB_EXT_PREFIX) {
        return Err(VerifyError::PrefixMismatch);
    }

    // I-1: exactly 4 components in canonical slot order
    const CANONICAL: [&str; 4] = ["kernel", "axon-os", "axon-audit", "monitor"];
    if measured.components.len() != 4 {
        return Err(VerifyError::Malformed(format!(
            "expected 4 components, got {}", measured.components.len()
        )));
    }
    for (i, (entry, &expected_name)) in measured.components.iter().zip(CANONICAL.iter()).enumerate() {
        if entry.name != expected_name {
            return Err(VerifyError::Malformed(format!(
                "component[{i}]: expected '{expected_name}', got '{}'", entry.name
            )));
        }
    }

    // I-2: re-derive monitor's expected hash from axon-os entry; never trust the report's claim
    let axon_os_sha256  = &measured.components[1].sha256;
    let monitor_sha256  = &measured.components[3].sha256;
    if monitor_sha256 != axon_os_sha256 {
        return Err(VerifyError::MonitorSlotMismatch);
    }

    // Digest equality check (the load-bearing gate)
    if measured.axtcb1_ext != expected_axtcb1_ext {
        return Err(VerifyError::DigestMismatch {
            got:      measured.axtcb1_ext.clone(),
            expected: expected_axtcb1_ext.to_string(),
        });
    }

    Ok(())
}

/// Serialize an `AttestationReport` + `ExtendedMeasurement` to JSON (schema `axon-vm-report/2`).
///
/// The R26 `axtcb1` and `digest` fields are **preserved** alongside the new
/// `axtcb1_ext` and `components` (I-4). A relying party that only cares about
/// R26 can still check `axtcb1`; an R31 relying party additionally checks
/// `axtcb1_ext` and `components`.
pub fn report_to_json_extended(report: &AttestationReport, ext: &ExtendedMeasurement) -> String {
    use base64::Engine as _;
    let sig_b64 = base64::engine::general_purpose::STANDARD.encode(&report.signature);
    let components_json: Vec<serde_json::Value> = ext.components.iter().map(|c| {
        serde_json::json!({
            "name":   c.name,
            "path":   c.path,
            "size":   c.size,
            "sha256": c.sha256,
        })
    }).collect();
    serde_json::to_string_pretty(&serde_json::json!({
        "schema": "axon-vm-report/2",
        "hw_root": report.hw_root,
        "substrate": "qemu-swtpm (stand-in — no memory encryption; use sev-snp/tdx for confidentiality)",
        "measurement": {
            "digest":     hex::encode(report.measurement.digest),
            "axtcb1":     report.measurement.axtcb1,
            "axtcb1_ext": ext.axtcb1_ext,
            "components": components_json,
            "timestamp":  report.measurement.timestamp,
        },
        "signature": sig_b64,
    }))
    .unwrap()
}

// ── Internal: software HMAC-SHA256 (no extra dep) ────────────────────────────

/// HMAC-SHA256 implemented from first principles (avoids the `hmac` dep).
///
/// RFC 2104: HMAC(K, m) = H((K ⊕ opad) ‖ H((K ⊕ ipad) ‖ m))
/// Block size B = 64 bytes for SHA-256.
/// HMAC-SHA256, hand-rolled over `sha2` (the workspace has no `hmac` crate and
/// this avoids adding one for ~20 lines of RFC 2104).
///
/// **Exported** so `axon-audit` can key its ledger chain with the same
/// primitive rather than growing a second copy — duplicated crypto is how the
/// T54 class of defect starts, and this crate is the natural owner.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;
    // Normalise key: hash if longer than block size, zero-pad if shorter.
    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let h: [u8; 32] = Sha256::digest(key).into();
        k[..32].copy_from_slice(&h);
    } else {
        k[..key.len()].copy_from_slice(key);
    }
    // ipad = 0x36 repeated; opad = 0x5c repeated.
    let mut ipad = k;
    let mut opad = k;
    for b in &mut ipad { *b ^= 0x36; }
    for b in &mut opad { *b ^= 0x5c; }
    // Inner hash: H(ipad ‖ data)
    let mut inner = Sha256::new();
    inner.update(ipad);
    inner.update(data);
    let inner_hash = inner.finalize();
    // Outer hash: H(opad ‖ inner_hash)
    let mut outer = Sha256::new();
    outer.update(opad);
    outer.update(inner_hash);
    outer.finalize().into()
}

// ── Tests — all 10 R26 §0 acceptance checks ───────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Stable test key for the software-TPM stand-in in these unit tests.
    /// In production, this would be an ephemeral per-session key.
    const TEST_KEY: &[u8] = b"axon-r26-software-tpm-test-key-do-not-use-in-production";

    // ─ A5 ──────────────────────────────────────────────────────────────────────
    /// Same image ⇒ byte-identical measurement; same job+seed ⇒ byte-identical record.
    #[test]
    fn acc_a5_measurement_and_record_byte_identical() {
        let kernel = b"axon-os-r26-test-kernel-bytes-for-a5";

        // Measurement is deterministic
        let m1 = measure_kernel_bytes(kernel);
        let m2 = measure_kernel_bytes(kernel);
        assert_eq!(m1.digest, m2.digest, "same image bytes must produce identical digest");
        assert_eq!(m1.axtcb1, m2.axtcb1, "same image bytes must produce identical axtcb1");
        assert_eq!(m1.timestamp, 0, "pure measurement timestamp must be 0");

        // Signature is deterministic (same measurement + same key)
        let r1 = sign_report(m1.clone(), TEST_KEY);
        let r2 = sign_report(m1.clone(), TEST_KEY);
        assert_eq!(r1.signature, r2.signature, "same inputs must produce identical signature");

        // In-guest record is deterministic (same job + same seed)
        let job = b"summarize --input ./data/ --out ./out/";
        let rec1 = simulate_job_run(job, 42u64);
        let rec2 = simulate_job_run(job, 42u64);
        assert_eq!(rec1, rec2, "same job+seed must produce byte-identical RunRecord");
        assert!(rec1.starts_with("axrec1:"), "record must use axrec1: prefix");

        // Different seed ⇒ different record
        let rec_diff = simulate_job_run(job, 99u64);
        assert_ne!(rec1, rec_diff, "different seeds must produce different records");
    }

    // ─ Core: attestation_mismatch_fails_closed ─────────────────────────────────
    /// For each failure mode, `verify_report` returns the correct refusal; no path ⇒ Ok.
    #[test]
    fn attestation_mismatch_fails_closed() {
        let kernel = b"genuine-axon-os-kernel-content";
        let m = measure_kernel_bytes(kernel);
        let correct_digest = m.digest;
        let correct_axtcb1 = m.axtcb1.clone();
        let report = sign_report(m, TEST_KEY);

        // (a) Wrong expected digest ⇒ must fail
        let mut wrong_digest = correct_digest;
        wrong_digest[0] ^= 0xff;
        let r = verify_report(&report, &wrong_digest, &correct_axtcb1);
        assert!(r.is_err(), "wrong expected digest must be refused");
        let msg = r.unwrap_err();
        assert!(
            msg.contains("measurement") || msg.contains("mismatch") || msg.contains("≠"),
            "error must describe the measurement mismatch: {msg}"
        );

        // (b) Wrong expected axtcb1 ⇒ must fail
        let r2 = verify_report(&report, &correct_digest, "axtcb1:deadbeef00000000");
        assert!(r2.is_err(), "wrong expected axtcb1 must be refused");
        let msg2 = r2.unwrap_err();
        assert!(
            msg2.contains("axtcb1") || msg2.contains("TCB") || msg2.contains("mismatch"),
            "error must describe the TCB chain mismatch: {msg2}"
        );

        // (c) Correct values ⇒ must succeed
        let r3 = verify_report(&report, &correct_digest, &correct_axtcb1);
        assert!(r3.is_ok(), "correct report must pass verification: {:?}", r3.err());
    }

    // ─ Core: vm_without_attestation_is_refused ─────────────────────────────────
    /// A VM with no attestation is REFUSED — not degraded (R26 §4.3 step 1, Core).
    #[test]
    fn vm_without_attestation_is_refused() {
        let kernel = b"some-kernel-bytes-for-no-attest-test";
        let m = measure_kernel_bytes(kernel);
        let digest = m.digest;
        let axtcb1 = m.axtcb1.clone();

        // Simulate a report with no signature (no attestation was produced)
        let mut unsigned_report = sign_report(m.clone(), TEST_KEY);
        unsigned_report.signature = Vec::new(); // empty = unsigned / no attestation

        // verify_report must refuse
        let result = verify_report(&unsigned_report, &digest, &axtcb1);
        assert!(result.is_err(), "unsigned report (no attestation) must be refused");
        let msg = result.unwrap_err();
        assert!(
            msg.contains("no attestation") || msg.contains("signature") || msg.contains("unsigned"),
            "error must mention missing/absent attestation: {msg}"
        );

        // try_admit_job also refuses (no job enters an unattested guest)
        let job_result = try_admit_job(&unsigned_report, &digest, &axtcb1, b"test-job", 0);
        assert!(
            job_result.is_err(),
            "try_admit_job must refuse when attestation is absent — job must NOT enter an unattested guest"
        );
    }

    // ─ Core: axtcb1_digest_chained_to_measurement ──────────────────────────────
    /// The `axtcb1:` TCB digest is chained to kernel content — one chain, not two stories.
    #[test]
    fn axtcb1_digest_chained_to_measurement() {
        let kernel = b"axon-os-kernel-with-r20-r23-bundled";
        let m = measure_kernel_bytes(kernel);

        // axtcb1 must carry the correct prefix
        assert!(
            m.axtcb1.starts_with("axtcb1:"),
            "axtcb1 must start with 'axtcb1:' prefix"
        );
        // axtcb1 hex body is 64 chars (32 bytes)
        let hex_body = m.axtcb1.strip_prefix("axtcb1:").unwrap();
        assert_eq!(hex_body.len(), 64, "axtcb1 hex body must be 64 hex chars (sha256)");

        // Different kernel content ⇒ different axtcb1 AND different digest (the chain)
        let other_kernel = b"different-kernel-different-tcb";
        let m2 = measure_kernel_bytes(other_kernel);
        assert_ne!(m.axtcb1, m2.axtcb1, "different kernel must produce different axtcb1");
        assert_ne!(m.digest, m2.digest, "different kernel must produce different digest");

        // The axtcb1 is verified by verify_report — a mismatched axtcb1 fails the chain check
        let report = sign_report(m.clone(), TEST_KEY);
        let wrong_axtcb1 = m2.axtcb1.clone(); // axtcb1 from a different image
        let r = verify_report(&report, &m.digest, &wrong_axtcb1);
        assert!(
            r.is_err(),
            "wrong axtcb1 (from different image) must fail — the chain must be enforced, \
             not two disconnected stories"
        );

        // Correct axtcb1 passes
        assert!(
            verify_report(&report, &m.digest, &m.axtcb1).is_ok(),
            "correct axtcb1 chained to correct measurement must pass"
        );
    }

    // ─ Core: tampered_guest_image_fails_attestation ────────────────────────────
    /// Flip one byte of the image ⇒ different measurement ⇒ relying party rejects.
    #[test]
    fn tampered_guest_image_fails_attestation() {
        let genuine = b"genuine-axon-os-kernel-elf-image-content-r26-test";
        let m_genuine = measure_kernel_bytes(genuine);
        let expected_digest = m_genuine.digest;
        let expected_axtcb1 = m_genuine.axtcb1.clone();

        // Tamper: flip one byte (e.g. injected backdoor, patch, or corrupted image)
        let mut tampered = genuine.to_vec();
        tampered[4] ^= 0x01;
        let m_tampered = measure_kernel_bytes(&tampered);

        // Tampered image must produce different digest and different axtcb1
        assert_ne!(
            m_tampered.digest, expected_digest,
            "tampered image must produce a different measurement digest"
        );
        assert_ne!(
            m_tampered.axtcb1, expected_axtcb1,
            "tampered image must produce a different axtcb1"
        );

        // A report of the tampered image, checked against the genuine expected values, fails
        let tampered_report = sign_report(m_tampered, TEST_KEY);
        let r = verify_report(&tampered_report, &expected_digest, &expected_axtcb1);
        assert!(
            r.is_err(),
            "tampered guest image must fail attestation — \
             report of tampered image must be refused by relying party"
        );
        let msg = r.unwrap_err();
        assert!(
            msg.contains("measurement") || msg.contains("mismatch") || msg.contains("tampered"),
            "error must explain the measurement mismatch: {msg}"
        );
    }

    // ─ A1: smoke boot→attest→admit→run journey (mock mode) ────────────────────
    /// Full journey through the attestation pipeline using mock data (no KVM required).
    #[test]
    fn acc_a1_smoke_boot_attest_run_journey() {
        // Step 1: measure (axon-vm measure)
        let kernel = b"mock-axon-os-kernel-for-a1-journey";
        let m = measure_kernel_bytes(kernel);
        assert!(m.axtcb1.starts_with("axtcb1:"), "measurement must carry axtcb1: prefix");
        let expected_digest = m.digest;
        let expected_axtcb1 = m.axtcb1.clone();

        // Step 2: attest (axon-vm attest → sign with software-TPM)
        let report = sign_report(m, TEST_KEY);
        assert_eq!(
            report.hw_root, SOFTWARE_TPM_HW_ROOT,
            "stand-in must be labeled as software-tpm-v1"
        );
        assert!(
            report.hw_root.contains("software-tpm"),
            "stand-in must explicitly name itself — no memory encryption vs host operator"
        );

        // Step 3: verify (axon-vm verify — relying party)
        let ok = verify_report(&report, &expected_digest, &expected_axtcb1);
        assert!(ok.is_ok(), "genuine image must pass attestation: {:?}", ok.err());

        // Step 4: admit + run (axon-vm run — only after verified)
        let job = b"summarize --input ./data/ --out ./out/";
        let record = try_admit_job(&report, &expected_digest, &expected_axtcb1, job, 42u64);
        assert!(record.is_ok(), "attested job must be admitted: {:?}", record.err());
        let rec = record.unwrap();
        assert!(rec.starts_with("axrec1:"), "record must have axrec1: prefix");

        // Step 5: tampered image ⇒ refused (axon-vm run with tampered image → exit 10, job never ran)
        let mut tampered = b"mock-axon-os-kernel-for-a1-journey".to_vec();
        tampered[5] ^= 0xff;
        let m_tampered = measure_kernel_bytes(&tampered);
        let tampered_report = sign_report(m_tampered, TEST_KEY);
        let refused = verify_report(&tampered_report, &expected_digest, &expected_axtcb1);
        assert!(
            refused.is_err(),
            "tampered image must be refused — job must never enter an unverified guest"
        );
        // Job must NOT be admitted when attestation fails
        let refused_job =
            try_admit_job(&tampered_report, &expected_digest, &expected_axtcb1, job, 42u64);
        assert!(
            refused_job.is_err(),
            "try_admit_job must refuse when attestation fails (tampered image)"
        );
    }

    // ─ A2: real job runs inside the attested guest (mock guest) ───────────────
    /// The same R21 job runs inside an attested guest and returns a deterministic record.
    #[test]
    fn acc_a2_guest_image_runs_real_job() {
        // The guest image bundles the R21 supervisor + R23 cert checker (R26 §4.6)
        let kernel = b"axon-os-with-r21-supervisor-r23-certcheck-bundled";
        let m = measure_kernel_bytes(kernel);
        let expected_digest = m.digest;
        let expected_axtcb1 = m.axtcb1.clone();

        // Produce attestation report
        let report = sign_report(m, TEST_KEY);

        // Verify attestation — this is MANDATORY before any job is admitted
        assert!(
            verify_report(&report, &expected_digest, &expected_axtcb1).is_ok(),
            "attestation must verify before job admission"
        );

        // Admit and run the R21 job (same summarize.axjob as the R21 demo)
        let job = b"summarize: reads ./data/, writes ./out/, no net (R21 job)";
        let record = try_admit_job(&report, &expected_digest, &expected_axtcb1, job, 1u64)
            .expect("attested job must run");
        assert!(record.starts_with("axrec1:"), "record must use axrec1: prefix");

        // A5: same job + same seed ⇒ byte-identical record
        let record2 = try_admit_job(&report, &expected_digest, &expected_axtcb1, job, 1u64)
            .expect("second run must also succeed");
        assert_eq!(record, record2, "same job+seed must produce byte-identical RunRecord (A5)");

        // Different seed ⇒ different record
        let record_diff = try_admit_job(&report, &expected_digest, &expected_axtcb1, job, 2u64)
            .expect("different-seed run must also succeed");
        assert_ne!(record, record_diff, "different seeds must produce different records");

        // Overreach variant: a job with net effects would be in-guest-denied
        // (in real R21+R23 pipeline; here we verify the mock gate works)
        let overreach_job = b"net: tries to reach external host (overreach)";
        // In mock mode both are just hash-based, but the gate should work
        let overreach_record =
            try_admit_job(&report, &expected_digest, &expected_axtcb1, overreach_job, 1u64)
                .expect("mock admits any bytes for A2; real R21 would deny net");
        assert!(overreach_record.starts_with("axrec1:"), "even mock overreach produces a record");
        assert_ne!(record, overreach_record, "different jobs must produce different records");
    }

    // ─ A3: quickstart commands execute ─────────────────────────────────────────
    /// Verifies that all library operations referenced in the quickstart work correctly.
    #[test]
    fn acc_a3_quickstart_commands_execute() {
        // Quickstart step 2: axon-vm measure → produces deterministic axmeas1: digest
        let kernel = b"axon-os-quickstart-test-kernel";
        let m = measure_kernel_bytes(kernel);
        assert!(m.axtcb1.starts_with("axtcb1:"), "measure must produce axtcb1: prefix");
        let digest_hex = hex::encode(m.digest);
        assert_eq!(digest_hex.len(), 64, "hex digest must be 64 chars");

        // Quickstart step 3: axon-vm attest → sign_report
        let report = sign_report(m.clone(), TEST_KEY);
        assert_eq!(report.hw_root, SOFTWARE_TPM_HW_ROOT);
        assert!(!report.signature.is_empty(), "attest must produce a non-empty signature");

        // Quickstart step 4: axon-vm verify → verify_report
        let ok = verify_report(&report, &m.digest, &m.axtcb1);
        assert!(ok.is_ok(), "verify must succeed for genuine image");

        // JSON output round-trips correctly (what axon-vm attest emits)
        let json_str = report_to_json(&report);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .expect("report_to_json must produce valid JSON");
        assert_eq!(parsed["hw_root"], SOFTWARE_TPM_HW_ROOT);
        assert_eq!(parsed["schema"], "axon-vm-report/1");
        assert!(
            parsed["measurement"]["axtcb1"].as_str().unwrap().starts_with("axtcb1:"),
            "JSON measurement.axtcb1 must carry the axtcb1: prefix"
        );
        assert!(
            parsed["substrate"].as_str().unwrap().contains("no memory encryption"),
            "JSON must contain the stand-in caveat about no memory encryption"
        );

        // Quickstart step 5: axon-vm run → try_admit_job
        let job = b"summarize.axjob";
        let rec = try_admit_job(&report, &m.digest, &m.axtcb1, job, 0u64)
            .expect("quickstart run must succeed");
        assert!(rec.starts_with("axrec1:"), "run must produce axrec1: record");

        // Quickstart step 6: axon-vm run tampered → refused (exit 10, job never ran)
        let mut tampered_kernel = kernel.to_vec();
        tampered_kernel[0] ^= 0xff;
        let m_tampered = measure_kernel_bytes(&tampered_kernel);
        let tampered_report = sign_report(m_tampered, TEST_KEY);
        let refused = verify_report(&tampered_report, &m.digest, &m.axtcb1);
        assert!(refused.is_err(), "tampered image must be refused (quickstart step 6)");
    }

    // ─ A4: device surface is minimal and isolated ───────────────────────────────
    /// Guest has only vsock + serial + timer; any extra device is DeviceDeny (exit 8).
    #[test]
    fn acc_a4_device_surface_minimal_and_isolated() {
        // The minimal set is exactly {vsock, serial, timer}
        let allowed = DeviceSet::minimal();
        assert!(allowed.vsock, "vsock (job channel) must be in the minimal set");
        assert!(allowed.serial, "serial (diagnostics) must be in the minimal set");
        assert!(allowed.timer, "timer (scheduler) must be in the minimal set");
        assert!(allowed.validate().is_ok(), "minimal DeviceSet must validate");

        // Named devices in the allowlist are accepted
        assert!(DeviceSet::validate_manifest_extra_device("vsock").is_ok());
        assert!(DeviceSet::validate_manifest_extra_device("serial").is_ok());
        assert!(DeviceSet::validate_manifest_extra_device("timer").is_ok());

        // Devices outside the allowlist are DeviceDeny
        for extra in &["gpu", "virtio-net", "block", "virtio-rng", "pci-passthrough",
                        "virtio-balloon", "nvme", "usb", "virtio-console-multiport"]
        {
            let r = DeviceSet::validate_manifest_extra_device(extra);
            assert!(
                r.is_err(),
                "device '{extra}' must be DeviceDeny (exit 8) — not in the §3.3 allowlist"
            );
            let msg = r.unwrap_err();
            assert!(
                msg.contains("DeviceDeny") || msg.contains("allowlist") || msg.contains("permitted"),
                "error for '{extra}' must explain the allowlist violation: {msg}"
            );
        }

        // A DeviceSet missing required devices fails validation
        let missing_vsock = DeviceSet { vsock: false, serial: true, timer: true };
        assert!(missing_vsock.validate().is_err(), "missing vsock must fail validation");
        let missing_timer = DeviceSet { vsock: true, serial: true, timer: false };
        assert!(missing_timer.validate().is_err(), "missing timer must fail validation");
    }

    // ─ A6: attestation is mandatory and chained ─────────────────────────────────
    /// Attestation MUST be verified before any job is admitted; axtcb1 is in the chain.
    #[test]
    fn acc_a6_attestation_mandatory_and_chained() {
        let kernel = b"axon-os-for-a6-mandatory-attest-test";
        let m = measure_kernel_bytes(kernel);

        // Without attestation (empty signature), job admission is refused
        let mut unsigned = sign_report(m.clone(), TEST_KEY);
        unsigned.signature = Vec::new();
        let r_unsigned = try_admit_job(&unsigned, &m.digest, &m.axtcb1, b"any-job", 0);
        assert!(
            r_unsigned.is_err(),
            "job must NOT be admitted without a valid attestation signature"
        );

        // With correct attestation, job is admitted
        let signed = sign_report(m.clone(), TEST_KEY);
        let r_signed = try_admit_job(&signed, &m.digest, &m.axtcb1, b"any-job", 0);
        assert!(r_signed.is_ok(), "job must be admitted after valid attestation: {:?}", r_signed.err());

        // The axtcb1 prefix is present and chained (A6 + Core)
        assert!(
            signed.measurement.axtcb1.starts_with("axtcb1:"),
            "axtcb1: prefix must be in the attestation measurement"
        );

        // If the axtcb1 pinned expectation is wrong, admission fails (chain enforced)
        let wrong_axtcb1 = format!("axtcb1:{}", "0".repeat(64));
        let r_wrong = try_admit_job(&signed, &m.digest, &wrong_axtcb1, b"any-job", 0);
        assert!(
            r_wrong.is_err(),
            "wrong axtcb1 expectation must fail — the chain is mandatory"
        );

        // Attestation without any job (just verify) also works
        let r_verify = verify_report(&signed, &m.digest, &m.axtcb1);
        assert!(r_verify.is_ok(), "standalone verify_report must succeed for genuine report");
    }

    // ══════════════════════════════════════════════════════════════════════════
    // R31 acceptance checks (§0 — every name here is normative and gated)
    // ══════════════════════════════════════════════════════════════════════════

    /// Helper: produce a valid `ComponentPaths` for unit tests (no filesystem).
    fn test_component_paths() -> ComponentPaths {
        ComponentPaths {
            kernel:           b"kernel-bytes-r31-test".to_vec(),
            axon_os:          b"axon-os-bytes-r31-test".to_vec(),
            axon_audit:       Some(b"axon-audit-bytes-r31-test".to_vec()),
            kernel_path:      "dist/guest/vmlinuz".to_string(),
            axon_os_path:     "target/release/axon-os".to_string(),
            axon_audit_path:  Some("target/release/axon-audit-writer".to_string()),
        }
    }

    // ─ A2: byte-identical across runs ─────────────────────────────────────────
    /// Same byte slices → same `axtcb1_ext`, twice in the same process.
    #[test]
    fn acc_a2_byte_identical_across_runs() {
        let m1 = measure_extended(test_component_paths()).unwrap();
        let m2 = measure_extended(test_component_paths()).unwrap();
        assert_eq!(
            m1.axtcb1_ext, m2.axtcb1_ext,
            "same bytes must produce byte-identical axtcb1_ext on every invocation"
        );
        assert!(m1.axtcb1_ext.starts_with("axtcb1-ext:"), "must carry axtcb1-ext: prefix");
        // Digest hex body is 64 hex chars (sha256 = 32 bytes)
        let hex_body = m1.axtcb1_ext.strip_prefix("axtcb1-ext:").unwrap();
        assert_eq!(hex_body.len(), 64, "axtcb1_ext hex body must be 64 hex chars");
    }

    // ─ A4: hermetic / isolated — pure function, no panics on valid inputs ─────
    /// `measure_extended` is total on valid inputs; produces no panics even with
    /// large synthetic bytes.  No syscalls beyond the injected slices.
    #[test]
    fn acc_a4_hermetic_isolated_timeout() {
        // Empty bytes edge case: still valid (0-byte kernel is unusual but must not panic)
        let empty = ComponentPaths {
            kernel:           vec![],
            axon_os:          vec![],
            axon_audit:       None,
            kernel_path:      "dist/guest/vmlinuz".to_string(),
            axon_os_path:     "target/release/axon-os".to_string(),
            axon_audit_path:  None,
        };
        let m = measure_extended(empty).unwrap();
        assert!(m.axtcb1_ext.starts_with("axtcb1-ext:"), "even empty inputs must produce a valid digest");
        assert_eq!(m.components.len(), 4);

        // Large synthetic bytes: must not panic and must be deterministic
        let large_kernel  = vec![0xABu8; 1024 * 64];
        let large_axon_os = vec![0xCDu8; 1024 * 64];
        let large = ComponentPaths {
            kernel:           large_kernel.clone(),
            axon_os:          large_axon_os.clone(),
            axon_audit:       Some(vec![0xEFu8; 1024 * 64]),
            kernel_path:      "dist/guest/vmlinuz".to_string(),
            axon_os_path:     "target/release/axon-os".to_string(),
            axon_audit_path:  Some("target/release/axon-audit-writer".to_string()),
        };
        let m_large1 = measure_extended(large).unwrap();
        let large2 = ComponentPaths {
            kernel:           large_kernel,
            axon_os:          large_axon_os,
            axon_audit:       Some(vec![0xEFu8; 1024 * 64]),
            kernel_path:      "dist/guest/vmlinuz".to_string(),
            axon_os_path:     "target/release/axon-os".to_string(),
            axon_audit_path:  Some("target/release/axon-audit-writer".to_string()),
        };
        let m_large2 = measure_extended(large2).unwrap();
        assert_eq!(m_large1.axtcb1_ext, m_large2.axtcb1_ext, "large-byte determinism");
    }

    // ─ A5: tampered component detected ────────────────────────────────────────
    /// Flipping one byte in any slot changes `axtcb1_ext`.  Tests kernel,
    /// axon-os, axon-audit, and monitor (implicit via axon-os) — ≥3 distinct
    /// tamper-detected outcomes required by the gate.
    #[test]
    fn acc_a5_tampered_component_detected() {
        let base_ext = measure_extended(test_component_paths()).unwrap().axtcb1_ext;

        // Tamper slot 0: kernel
        let tampered_kernel = {
            let mut b = b"kernel-bytes-r31-test".to_vec();
            b[0] ^= 0x01;
            b
        };
        let m_k = measure_extended(ComponentPaths {
            kernel: tampered_kernel,
            ..test_component_paths()
        }).unwrap();
        assert_ne!(m_k.axtcb1_ext, base_ext, "tampered kernel must change axtcb1_ext (slot 0)");

        // Tamper slot 1: axon-os (also affects slot 3 monitor — two slots)
        let tampered_axon_os = {
            let mut b = b"axon-os-bytes-r31-test".to_vec();
            b[0] ^= 0x01;
            b
        };
        let m_a = measure_extended(ComponentPaths {
            axon_os: tampered_axon_os,
            ..test_component_paths()
        }).unwrap();
        assert_ne!(m_a.axtcb1_ext, base_ext, "tampered axon-os must change axtcb1_ext (slot 1+3)");
        assert_ne!(m_a.axtcb1_ext, m_k.axtcb1_ext, "kernel tamper ≠ axon-os tamper");

        // Tamper slot 2: axon-audit (real binary path; zero-fill path is tested separately)
        let tampered_axon_audit = {
            let mut b = b"axon-audit-bytes-r31-test".to_vec();
            b[0] ^= 0x01;
            b
        };
        let m_au = measure_extended(ComponentPaths {
            axon_audit: Some(tampered_axon_audit),
            ..test_component_paths()
        }).unwrap();
        assert_ne!(m_au.axtcb1_ext, base_ext, "tampered axon-audit must change axtcb1_ext (slot 2)");

        // Anti-vacuous: all three tamper outcomes are distinct
        let distinct: std::collections::HashSet<&str> = [
            m_k.axtcb1_ext.as_str(),
            m_a.axtcb1_ext.as_str(),
            m_au.axtcb1_ext.as_str(),
        ].into();
        assert_eq!(distinct.len(), 3, "all three tamper outcomes must be distinct digests");
    }

    // ─ A6: canonical chaining order matters ───────────────────────────────────
    /// Swapping kernel and axon-os bytes produces a different `axtcb1_ext`.
    /// Proves that canonical order is enforced — reordering silently produces a
    /// different digest (not silently the same).
    #[test]
    fn acc_a6_chaining_order_canonical() {
        // Canonical: kernel=A, axon-os=B
        let m_canonical = measure_extended(ComponentPaths {
            kernel:           b"component-A".to_vec(),
            axon_os:          b"component-B".to_vec(),
            axon_audit:       Some(b"component-C".to_vec()),
            kernel_path:      "k".to_string(),
            axon_os_path:     "ao".to_string(),
            axon_audit_path:  Some("aa".to_string()),
        }).unwrap();

        // Swapped: kernel=B, axon-os=A (same byte sets, different slot assignment)
        let m_swapped = measure_extended(ComponentPaths {
            kernel:           b"component-B".to_vec(),
            axon_os:          b"component-A".to_vec(),
            axon_audit:       Some(b"component-C".to_vec()),
            kernel_path:      "k".to_string(),
            axon_os_path:     "ao".to_string(),
            axon_audit_path:  Some("aa".to_string()),
        }).unwrap();

        assert_ne!(
            m_canonical.axtcb1_ext, m_swapped.axtcb1_ext,
            "swapping kernel and axon-os bytes must produce a different digest (order is canonical)"
        );

        // Components are always in the canonical named order regardless of inputs
        let canonical_names = ["kernel", "axon-os", "axon-audit", "monitor"];
        for (entry, expected_name) in m_canonical.components.iter().zip(canonical_names.iter()) {
            assert_eq!(&entry.name, expected_name, "component names must be in canonical order");
        }
    }

    // ─ Core: missing_component_fails_closed ───────────────────────────────────
    /// Required components (kernel, axon-os) absent → `Err(ComponentMissing)`.
    /// Optional axon-audit absent → zero-fill sentinel (NOT an error).
    #[test]
    fn missing_component_fails_closed() {
        // This test exercises the pure API: we use measure_host_stack for the
        // filesystem cases, and measure_extended for the zero-fill case.

        // Zero-fill path: axon-audit = None → zero-fill, not an error
        let m = measure_extended(ComponentPaths {
            kernel:           b"k".to_vec(),
            axon_os:          b"ao".to_vec(),
            axon_audit:       None,
            kernel_path:      "k-path".to_string(),
            axon_os_path:     "ao-path".to_string(),
            axon_audit_path:  None,
        }).unwrap();
        // Confirm zero-fill sentinel appears in components
        let audit_entry = m.components.iter().find(|c| c.name == "axon-audit").unwrap();
        assert_eq!(audit_entry.sha256, "0".repeat(64), "absent axon-audit must be zero-filled");
        assert_eq!(audit_entry.path, "<absent>", "absent axon-audit must record '<absent>'");
        assert_eq!(audit_entry.size, 0, "absent axon-audit size must be 0");

        // measure_extended always succeeds (byte-level API; no I/O fails here)
        // measure_host_stack is the I/O path that returns ComponentMissing for missing files.
        // We test that via a nonexistent path:
        let tmp = std::env::temp_dir();
        let missing_kernel = tmp.join("axon-r31-definitely-does-not-exist-kernel.bin");
        let missing_axon_os = tmp.join("axon-r31-definitely-does-not-exist-axon-os.bin");

        // Kernel missing → Err(ComponentMissing("kernel"))
        let r = measure_host_stack(&missing_kernel, Some(&missing_axon_os), None);
        assert!(r.is_err(), "missing kernel must return Err");
        let msg = r.unwrap_err().to_string();
        assert!(msg.contains("kernel"), "error must mention 'kernel': {msg}");

        // Kernel exists but axon-os missing → Err(ComponentMissing("axon-os"))
        let kernel_tmp = tmp.join(format!("axon-r31-test-kernel-{}.bin", std::process::id()));
        std::fs::write(&kernel_tmp, b"test-kernel").unwrap();
        let r2 = measure_host_stack(&kernel_tmp, Some(&missing_axon_os), None);
        assert!(r2.is_err(), "missing axon-os must return Err");
        let msg2 = r2.unwrap_err().to_string();
        assert!(msg2.contains("axon-os"), "error must mention 'axon-os': {msg2}");
        let _ = std::fs::remove_file(&kernel_tmp);

        // axon-os path = None → Err(ComponentMissing("axon-os"))
        let r3 = measure_host_stack(&missing_kernel, None, None);
        assert!(r3.is_err(), "axon-os = None must return Err");
    }

    // ─ Core: missing_component_zero_fill_is_auditable ─────────────────────────
    /// Missing `axon-audit` → zero-fill sentinel is auditable; differs from full-stack.
    #[test]
    fn missing_component_zero_fill_is_auditable() {
        let with_audit = measure_extended(test_component_paths()).unwrap();
        let without_audit = measure_extended(ComponentPaths {
            axon_audit: None,
            ..test_component_paths()
        }).unwrap();

        // axtcb1_ext values must differ (zero-fill ≠ real audit hash)
        assert_ne!(
            with_audit.axtcb1_ext, without_audit.axtcb1_ext,
            "zero-fill sentinel must produce a different digest than the real axon-audit hash"
        );
        // The sentinel entry is auditable
        let audit_entry = without_audit.components.iter().find(|c| c.name == "axon-audit").unwrap();
        assert_eq!(audit_entry.sha256, "0".repeat(64), "zero-fill sha256 must be 64 zeros");
        assert_eq!(audit_entry.path, "<absent>");
        // The measurement is still valid (R28-pending window)
        assert!(without_audit.axtcb1_ext.starts_with("axtcb1-ext:"));
    }

    // ─ Core: component_version_in_report ──────────────────────────────────────
    /// A well-formed `ExtendedMeasurement` has exactly 4 components in canonical
    /// order; monitor.sha256 == axon-os.sha256; monitor.size == 0.
    #[test]
    fn component_version_in_report() {
        let m = measure_extended(test_component_paths()).unwrap();

        // Exactly 4 components
        assert_eq!(m.components.len(), 4, "must have exactly 4 component entries");

        // Canonical names in order
        let names: Vec<&str> = m.components.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["kernel", "axon-os", "axon-audit", "monitor"]);

        // All entries have non-empty name, path, sha256
        for c in &m.components {
            assert!(!c.name.is_empty(),   "component name must be non-empty");
            assert!(!c.path.is_empty(),   "component path must be non-empty");
            assert!(!c.sha256.is_empty(), "component sha256 must be non-empty");
            assert_eq!(c.sha256.len(), 64, "sha256 must be 64 hex chars");
        }

        // I-2: monitor.sha256 == axon-os.sha256
        assert_eq!(
            m.components[3].sha256, m.components[1].sha256,
            "monitor.sha256 must equal axon-os.sha256 (I-2)"
        );

        // Monitor slot: path == "(embedded in axon-os)", size == 0
        assert_eq!(m.components[3].path, "(embedded in axon-os)");
        assert_eq!(m.components[3].size, 0);

        // JSON round-trip: report_to_json_extended produces schema axon-vm-report/2
        let kernel    = b"kernel-bytes-r31-test";
        let base_meas = measure_kernel_bytes(kernel);
        let fake_report = sign_report(base_meas, b"test-key");
        let json_str = report_to_json_extended(&fake_report, &m);
        let parsed: serde_json::Value = serde_json::from_str(&json_str)
            .expect("report_to_json_extended must produce valid JSON");
        assert_eq!(parsed["schema"], "axon-vm-report/2", "extended schema must be axon-vm-report/2");
        let comps = parsed["measurement"]["components"].as_array().unwrap();
        assert_eq!(comps.len(), 4, "JSON must have 4 components");
        assert_eq!(comps[3]["sha256"], comps[1]["sha256"], "JSON monitor.sha256 must equal axon-os.sha256");
        assert!(parsed["measurement"]["axtcb1"].as_str().unwrap().starts_with("axtcb1:"),
            "R26 axtcb1 must be present in schema/2");
        assert!(parsed["measurement"]["axtcb1_ext"].as_str().unwrap().starts_with("axtcb1-ext:"),
            "axtcb1_ext must be present in schema/2");
    }

    // ─ Core: r26_baseline_backward_compatible ─────────────────────────────────
    /// R26 measurement + `report_to_json` still works unchanged; schema stays `/1`.
    /// `measure_host_stack` produces schema `/2` with both `axtcb1` and `axtcb1_ext`.
    #[test]
    fn r26_baseline_backward_compatible() {
        // R26 path: measure_kernel_bytes → sign_report → verify_report → report_to_json
        let kernel = b"r26-backward-compat-kernel";
        let m = measure_kernel_bytes(kernel);
        assert!(m.axtcb1.starts_with("axtcb1:"), "R26 axtcb1 prefix must be 'axtcb1:'");

        let report = sign_report(m.clone(), TEST_KEY);
        assert!(verify_report(&report, &m.digest, &m.axtcb1).is_ok(), "R26 verify must pass");

        // report_to_json (R26) still produces schema /1 with no extended fields
        let json_str = report_to_json(&report);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["schema"], "axon-vm-report/1", "R26 schema must remain axon-vm-report/1");
        assert!(parsed["measurement"].get("axtcb1_ext").is_none(),
            "R26 JSON must NOT contain axtcb1_ext");
        assert!(parsed["measurement"].get("components").is_none(),
            "R26 JSON must NOT contain components");

        // R31 extended path: measure_extended produces schema /2 with BOTH axtcb1 and axtcb1_ext
        let ext = measure_extended(test_component_paths()).unwrap();
        assert!(ext.base.axtcb1.starts_with("axtcb1:"),
            "ExtendedMeasurement.base.axtcb1 must carry R26 prefix");
        assert!(ext.axtcb1_ext.starts_with("axtcb1-ext:"),
            "ExtendedMeasurement.axtcb1_ext must carry R31 prefix");
        // The two digests are never the same value (different prefix strings)
        assert_ne!(ext.base.axtcb1, ext.axtcb1_ext, "axtcb1 and axtcb1_ext must never be equal");

        // report_to_json_extended produces schema /2 with both fields
        let ext_report = sign_report(ext.base.clone(), TEST_KEY);
        let ext_json = report_to_json_extended(&ext_report, &ext);
        let ext_parsed: serde_json::Value = serde_json::from_str(&ext_json).unwrap();
        assert_eq!(ext_parsed["schema"], "axon-vm-report/2");
        assert!(ext_parsed["measurement"]["axtcb1"].as_str().unwrap().starts_with("axtcb1:"));
        assert!(ext_parsed["measurement"]["axtcb1_ext"].as_str().unwrap().starts_with("axtcb1-ext:"));
    }

    // ─ Verify: prefix mismatch and monitor mismatch are detected ──────────────
    #[test]
    fn verify_extended_catches_prefix_and_monitor_invariants() {
        let m = measure_extended(test_component_paths()).unwrap();

        // Wrong prefix on expected value → PrefixMismatch
        let r = verify_extended(&m, "axtcb1:not-the-right-prefix");
        assert!(matches!(r, Err(VerifyError::PrefixMismatch)),
            "expected PrefixMismatch, got {r:?}");

        // Correct expected → Ok
        assert!(verify_extended(&m, &m.axtcb1_ext).is_ok(), "correct expected must pass");

        // Wrong expected (right prefix, wrong digest) → DigestMismatch
        let wrong = format!("axtcb1-ext:{}", "a".repeat(64));
        assert!(matches!(
            verify_extended(&m, &wrong),
            Err(VerifyError::DigestMismatch { .. })
        ), "wrong digest must give DigestMismatch");

        // Mutated monitor sha256 → MonitorSlotMismatch
        let mut bad = m.clone();
        bad.components[3].sha256 = "b".repeat(64);
        assert!(matches!(
            verify_extended(&bad, &bad.axtcb1_ext),
            Err(VerifyError::MonitorSlotMismatch)
        ), "monitor sha256 tamper must give MonitorSlotMismatch");
    }

    // ─ A1: smoke extended TCB journey (unit-level; filesystem + pure) ──────────
    /// `measure_host_stack` on real temp files → 4 components, axtcb1_ext starts correctly.
    /// Tamped copy of axon-os → different axtcb1_ext → verify_extended rejects.
    #[test]
    fn acc_a1_smoke_extended_tcb_journey() {
        let tmp = std::env::temp_dir();
        let pid = std::process::id();

        let kernel_path = tmp.join(format!("axon-r31-a1-kernel-{pid}.bin"));
        let axon_os_path = tmp.join(format!("axon-r31-a1-axon-os-{pid}.bin"));
        let audit_path = tmp.join(format!("axon-r31-a1-axon-audit-{pid}.bin"));

        std::fs::write(&kernel_path, b"r31-a1-kernel-bytes").unwrap();
        std::fs::write(&axon_os_path, b"r31-a1-axon-os-bytes").unwrap();
        std::fs::write(&audit_path, b"r31-a1-audit-bytes").unwrap();

        // Step 1: measure → 4 components, axtcb1_ext present
        let m = measure_host_stack(&kernel_path, Some(&axon_os_path), Some(&audit_path))
            .expect("measure_host_stack must succeed on valid files");
        assert_eq!(m.components.len(), 4, "must have 4 components");
        assert!(m.axtcb1_ext.starts_with("axtcb1-ext:"), "must have axtcb1-ext: prefix");
        assert_eq!(m.components[3].sha256, m.components[1].sha256, "monitor sha256 == axon-os sha256");

        // Step 2: verify against the measured value → Ok
        assert!(
            verify_extended(&m, &m.axtcb1_ext).is_ok(),
            "verify_extended must pass for a self-consistent measurement"
        );

        // Step 3: tamper axon-os (flip one byte) → different axtcb1_ext → verify rejects
        let tampered_os_path = tmp.join(format!("axon-r31-a1-axon-os-tampered-{pid}.bin"));
        let mut os_bytes = b"r31-a1-axon-os-bytes".to_vec();
        os_bytes[4] ^= 0xFF;
        std::fs::write(&tampered_os_path, &os_bytes).unwrap();

        let m_tampered = measure_host_stack(&kernel_path, Some(&tampered_os_path), Some(&audit_path))
            .expect("measure_host_stack must succeed on tampered file too");
        assert_ne!(m.axtcb1_ext, m_tampered.axtcb1_ext, "tampered axon-os must change axtcb1_ext");

        // Verify tampered measurement against original expected → must fail
        let r = verify_extended(&m_tampered, &m.axtcb1_ext);
        assert!(r.is_err(), "original expected must reject tampered measurement");

        // Clean up
        for p in [&kernel_path, &axon_os_path, &audit_path, &tampered_os_path] {
            let _ = std::fs::remove_file(p);
        }
    }

    // ─ A3: R31 quickstart operations execute (extended path) ──────────────────
    /// Library operations referenced in the R31 §8 quickstart (extended TCB path).
    /// The R26 `acc_a3_quickstart_commands_execute` (above) covers the R26 path.
    #[test]
    fn acc_a3_extended_tcb_quickstart_commands_execute() {
        let m = measure_extended(test_component_paths()).unwrap();

        // Step 1: axtcb1_ext starts with the right prefix
        assert!(m.axtcb1_ext.starts_with("axtcb1-ext:"), "step 1: axtcb1-ext: prefix");

        // Step 2: components | length == 4
        assert_eq!(m.components.len(), 4, "step 2: 4 components");

        // Step 5: monitor.sha256 == axon-os.sha256
        assert_eq!(
            m.components.iter().find(|c| c.name == "monitor").unwrap().sha256,
            m.components.iter().find(|c| c.name == "axon-os").unwrap().sha256,
            "step 5: monitor.sha256 == axon-os.sha256"
        );

        // Step 6: tampered axon-os → different axtcb1_ext
        let tampered = measure_extended(ComponentPaths {
            axon_os: b"TAMPERED".to_vec(),
            ..test_component_paths()
        }).unwrap();
        assert_ne!(m.axtcb1_ext, tampered.axtcb1_ext, "step 6: tamper detected");

        // report_to_json_extended round-trips to valid JSON with schema /2
        let base_m = measure_kernel_bytes(b"quickstart-kernel");
        let fake_report = sign_report(base_m, b"quickstart-test-key");
        let json_str = report_to_json_extended(&fake_report, &m);
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        assert_eq!(parsed["schema"], "axon-vm-report/2");
        assert_eq!(parsed["measurement"]["components"].as_array().unwrap().len(), 4);
    }

    // ─ Additional: HMAC determinism ─────────────────────────────────────────────
    #[test]
    fn hmac_sha256_deterministic() {
        let key = b"test-key";
        let data = b"test-data";
        let h1 = hmac_sha256(key, data);
        let h2 = hmac_sha256(key, data);
        assert_eq!(h1, h2, "HMAC must be deterministic");
        // Different data ⇒ different HMAC
        let h3 = hmac_sha256(key, b"other-data");
        assert_ne!(h1, h3, "HMAC over different data must differ");
        // Different key ⇒ different HMAC
        let h4 = hmac_sha256(b"other-key", data);
        assert_ne!(h1, h4, "HMAC with different key must differ");
    }
}
#[cfg(test)]
mod hw_root_verification_tests {
    use super::*;

    /// AUDIT T13 (finding OSK-P7-C1). `verify_report` never read `hw_root`, so a
    /// software stand-in could claim `sev-snp` and verify exactly like a real
    /// hardware quote — making the single field that distinguishes "no memory
    /// encryption vs the host" from "confidential computing" an unchecked
    /// self-assertion.
    #[test]
    fn a_forged_hardware_root_claim_is_refused() {
        let kernel = b"fake kernel bytes for measurement";
        let report = sign_report(measure_kernel_bytes(kernel), b"test-ek");
        let digest = report.measurement.digest;
        let tcb = report.measurement.axtcb1.clone();

        // The honest stand-in verifies.
        assert!(
            verify_report(&report, &digest, &tcb).is_ok(),
            "an unmodified software-stand-in report must verify"
        );

        // The SAME report, claiming a hardware root of trust, must NOT.
        for forged in ["sev-snp", "tdx", "totally-real-tpm"] {
            let mut liar = report.clone();
            liar.hw_root = forged.to_string();
            let err = verify_report(&liar, &digest, &tcb)
                .expect_err(&format!("a forged hw_root `{forged}` must be refused"));
            assert!(
                err.contains("unverifiable hw_root"),
                "the refusal must name the reason, got: {err}"
            );
        }
    }
}
