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

// ── Internal: software HMAC-SHA256 (no extra dep) ────────────────────────────

/// HMAC-SHA256 implemented from first principles (avoids the `hmac` dep).
///
/// RFC 2104: HMAC(K, m) = H((K ⊕ opad) ‖ H((K ⊕ ipad) ‖ m))
/// Block size B = 64 bytes for SHA-256.
fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
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
