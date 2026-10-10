//! Launch admission (D-019): the policy gates a microVM launch must pass, held
//! in the library so that EVERY launcher goes through them.
//!
//! Before this module, `run_in_firecracker` applied no gate at all. The four
//! checks below lived only in the `axon-vm run` CLI (`cmd_run`), so a library
//! caller — the reason the library target exists (B262) — could boot an
//! unattested kernel with a `null` effect grant. Now the launch API takes an
//! [`AdmittedLaunch`], which only [`admit`] can construct, and the CLI calls
//! [`admit`] too. The checks were MOVED here, not copied: there is one
//! implementation of each.
//!
//! In order (the order is observable — each prints before the next runs):
//!
//! 1. **Narrow-only override.** `AXON_VM_ALLOWED_EFFECTS` may only narrow a
//!    manifest's effect union, never widen it (R36 §S0).
//! 2. **No null grant.** With no manifest, no principal and no override there
//!    is no grant at all, and the launch is refused rather than sending `null`
//!    to the guest (T48 / OSK-P7-C3). An EMPTY grant is not a null grant: it is
//!    deny-all, the same reading `""` has everywhere else in this repository
//!    (`AXON_ALLOWED_EFFECTS`, `sandbox_create_scoped`).
//! 3. **Kernel attestation, no TOFU.** The kernel is measured and compared with
//!    a PINNED digest (`expect_digest`, else the baseline file). No pin is a
//!    refusal (P7-KRN-05). [`KernelPin::DevBypass`] (`--no-attest`) is the only
//!    bypass and it warns.
//! 4. **Extended TCB, no TOFU** (only when requested). The host stack is
//!    measured and verified against a pinned `axtcb1-ext` (T52 / P4-OS-11).
//!
//! The effect grant that passed these gates is baked into the
//! [`MmdsPayload`] the admission carries, and the kernel that was attested is
//! the kernel the launch boots — a caller cannot pair an admission with a
//! different grant or a different image.
//!
//! [`AdmittedLaunch`] has private fields, so it cannot be built by hand:
//!
//! ```compile_fail
//! use axon_vm::admit::AdmittedLaunch;
//! use axon_vm::MmdsPayload;
//! let forged = AdmittedLaunch {
//!     kernel: std::path::PathBuf::from("/tmp/unattested"),
//!     mmds: MmdsPayload {
//!         schema: "axon-vm-mmds/1".into(),
//!         run_id: "forged".into(),
//!         principal: None,
//!         allowed_effects: vec!["Exec".into()],
//!         budget_tokens: None,
//!         source_hash: None,
//!         seccomp_bpf_b64: None,
//!     },
//! };
//! ```

use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use axon_attest::{measure_host_stack, measure_kernel, verify_extended};

use crate::firecracker::MmdsPayload;

/// Exit code for an attestation refusal (kernel or extended TCB): "the software
/// about to run is not the software that was blessed".
pub const ATTESTATION_EXIT_CODE: i32 = 10;
/// Exit code for a policy-provenance refusal (widening override, null grant).
pub const POLICY_EXIT_CODE: i32 = 2;
/// Exit code for an extended-TCB measurement failure (component missing).
pub const EXTENDED_TCB_MEASURE_FAIL_EXIT_CODE: i32 = 12;

/// How the kernel is to be attested.
#[derive(Debug, Clone, Copy)]
pub enum KernelPin<'a> {
    /// Verify against `expect_digest` if given, else the digest in `baseline`.
    /// Neither present is a refusal — never trust-on-first-use.
    Verify {
        expect_digest: Option<&'a str>,
        baseline: &'a Path,
    },
    /// `--no-attest`: skip kernel attestation, with a warning. Development only.
    DevBypass,
}

/// The extended-TCB gate (R31/T52), when requested.
#[derive(Debug, Clone, Copy)]
pub struct ExtendedTcb<'a> {
    pub axon_os: &'a Path,
    pub axon_audit: Option<&'a Path>,
    /// An operator-supplied `axtcb1-ext:` pin; outranks `baseline`.
    pub expect: Option<&'a str>,
    /// File holding the pinned `axtcb1-ext:` value.
    pub baseline: &'a Path,
}

/// Everything admission decides on. The effect sources are passed in rather
/// than read from the environment so the rules are testable; the CLI passes
/// `AXON_VM_ALLOWED_EFFECTS` as `effects_override`.
#[derive(Debug, Clone, Copy)]
pub struct AdmitRequest<'a> {
    pub run_id: &'a str,
    pub kernel: &'a Path,
    pub principal: Option<&'a str>,
    /// The `.axmeta` manifest's `effect_union`, if a manifest declared one.
    pub manifest_effects: Option<&'a [String]>,
    /// The registered principal's `allowed_effects`, if a principal was given.
    pub principal_effects: Option<&'a [String]>,
    /// Raw comma-separated override (`AXON_VM_ALLOWED_EFFECTS`). `Some("")` is
    /// an explicit deny-all, not "unset".
    pub effects_override: Option<&'a str>,
    pub budget_tokens: Option<u64>,
    pub source_hash: Option<&'a str>,
    pub seccomp_bpf_b64: Option<&'a str>,
    pub kernel_pin: KernelPin<'a>,
    pub extended_tcb: Option<ExtendedTcb<'a>>,
}

/// Proof that a launch passed [`admit`]. Private fields: the only constructor
/// is [`admit`] (see the `compile_fail` example in the module docs).
#[derive(Debug)]
pub struct AdmittedLaunch {
    kernel: PathBuf,
    mmds: MmdsPayload,
}

impl AdmittedLaunch {
    /// The kernel that was attested — the one the launch boots.
    pub fn kernel(&self) -> &Path {
        &self.kernel
    }
    /// The boot policy delivered to the guest, carrying the admitted grant.
    pub fn mmds(&self) -> &MmdsPayload {
        &self.mmds
    }
    /// The admitted effect grant. Empty means deny-all.
    pub fn allowed_effects(&self) -> &[String] {
        &self.mmds.allowed_effects
    }
}

/// Why a launch was refused. `Display` is the exact text `axon-vm run` prints
/// after its `axon-vm: ` prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdmitError {
    /// The override named effects the manifest does not grant.
    OverrideWidens {
        not_granted: Vec<String>,
        manifest_grants: Vec<String>,
    },
    /// No manifest, no principal, no override: nothing grants anything.
    NoEffectGrant,
    /// Kernel attestation failed (mismatch or no pin).
    KernelAttestation(String),
    /// `--extended-tcb` with nothing pinned to verify against.
    ExtendedTcbUnpinned { measured: String },
    /// The measured extended TCB did not match the pin.
    ExtendedTcbMismatch {
        detail: String,
        expected: String,
        measured: String,
    },
    /// A required TCB component could not be measured.
    ExtendedTcbMeasure(String),
}

pub const NO_EFFECT_GRANT_MSG: &str = concat!(
    "no effect grant: the program has no `.axmeta` manifest ",
    "(`axon build --emit-manifest`), no `--principal` was given, and ",
    "AXON_VM_ALLOWED_EFFECTS is unset. Refusing to launch rather than ",
    "sending a null policy to the guest"
);

impl AdmitError {
    /// The process exit code `axon-vm run` uses for this refusal.
    pub fn exit_code(&self) -> i32 {
        match self {
            AdmitError::OverrideWidens { .. } | AdmitError::NoEffectGrant => POLICY_EXIT_CODE,
            AdmitError::KernelAttestation(_)
            | AdmitError::ExtendedTcbUnpinned { .. }
            | AdmitError::ExtendedTcbMismatch { .. } => ATTESTATION_EXIT_CODE,
            AdmitError::ExtendedTcbMeasure(_) => EXTENDED_TCB_MEASURE_FAIL_EXIT_CODE,
        }
    }
}

impl fmt::Display for AdmitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdmitError::OverrideWidens {
                not_granted,
                manifest_grants,
            } => write!(
                f,
                "AXON_VM_ALLOWED_EFFECTS may only narrow the manifest's \
                 effect grant, not widen it. Not in the manifest: {}. Manifest grants: {}.",
                not_granted.join(", "),
                manifest_grants.join(", ")
            ),
            AdmitError::NoEffectGrant => f.write_str(NO_EFFECT_GRANT_MSG),
            AdmitError::KernelAttestation(e) => f.write_str(e),
            AdmitError::ExtendedTcbUnpinned { measured } => write!(
                f,
                "--extended-tcb requires a pinned expectation. Measured {measured} \
                 but have nothing to verify it against.\n                           Pin it once:  axon-vm attest --kernel <path> --extended-tcb --pin-extended\n                           Or pass:      --expect-axtcb1-ext {measured}"
            ),
            AdmitError::ExtendedTcbMismatch {
                detail,
                expected,
                measured,
            } => write!(
                f,
                "EXTENDED TCB MISMATCH: {detail}\n  expected {expected}\n  measured {measured}"
            ),
            AdmitError::ExtendedTcbMeasure(e) => {
                write!(f, "extended TCB measurement failed: {e}")
            }
        }
    }
}

impl std::error::Error for AdmitError {}

/// Run every launch gate, in order, and on success return the admission the
/// launch API requires. Nothing is launched here; a refusal has no effect
/// beyond the diagnostics printed to stderr (and the baseline files are only
/// ever READ — the run path never establishes a pin).
pub fn admit(req: &AdmitRequest<'_>) -> Result<AdmittedLaunch, AdmitError> {
    // 1 + 2: the effect grant.
    let allowed_effects = resolve_effect_grant(
        req.manifest_effects,
        req.principal_effects,
        req.effects_override,
    )?;

    // 3: kernel attestation (R26/T32), no trust-on-first-use.
    match req.kernel_pin {
        KernelPin::DevBypass => {
            measure_and_attest_inner(req.kernel, true, None, Path::new(""))
                .map_err(|e| AdmitError::KernelAttestation(e.to_string()))?;
        }
        KernelPin::Verify {
            expect_digest,
            baseline,
        } => {
            measure_and_attest_inner(req.kernel, false, expect_digest, baseline)
                .map_err(|e| AdmitError::KernelAttestation(e.to_string()))?;
        }
    }

    // 4: extended TCB (R31/T52), no trust-on-first-use.
    if let Some(x) = req.extended_tcb {
        check_extended_tcb(req.kernel, &x)?;
    }

    Ok(AdmittedLaunch {
        kernel: req.kernel.to_path_buf(),
        mmds: MmdsPayload {
            schema: "axon-vm-mmds/1".to_string(),
            run_id: req.run_id.to_string(),
            principal: req.principal.map(str::to_string),
            allowed_effects,
            budget_tokens: req.budget_tokens,
            source_hash: req.source_hash.map(str::to_string),
            seccomp_bpf_b64: req.seccomp_bpf_b64.map(str::to_string),
        },
    })
}

/// Checks 1 and 2. An explicit override is the grant when it narrows the
/// manifest (or when there is no manifest to narrow); otherwise the manifest,
/// then the principal. None of the three is a refusal.
fn resolve_effect_grant(
    manifest: Option<&[String]>,
    principal: Option<&[String]>,
    override_raw: Option<&str>,
) -> Result<Vec<String>, AdmitError> {
    if let Some(raw) = override_raw {
        let forced: Vec<String> = raw
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();
        // Checked ONLY against a manifest. With no manifest the override is the
        // sole grant and there is nothing to be a subset of.
        if let Some(union) = manifest {
            let extra = effects_not_granted_by(&forced, union);
            if !extra.is_empty() {
                return Err(AdmitError::OverrideWidens {
                    not_granted: extra,
                    manifest_grants: union.to_vec(),
                });
            }
        }
        return Ok(forced);
    }
    manifest
        .or(principal)
        .map(<[String]>::to_vec)
        .ok_or(AdmitError::NoEffectGrant)
}

fn check_extended_tcb(kernel: &Path, x: &ExtendedTcb<'_>) -> Result<(), AdmitError> {
    let ext = measure_host_stack(kernel, Some(x.axon_os), x.axon_audit)
        .map_err(|e| AdmitError::ExtendedTcbMeasure(e.to_string()))?;
    // An expectation is REQUIRED; its absence is a refusal rather than TOFU
    // against a user-writable file (T52).
    let expected = x.expect.map(str::to_string).or_else(|| {
        fs::read_to_string(x.baseline)
            .ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    });
    let Some(expected) = expected else {
        return Err(AdmitError::ExtendedTcbUnpinned {
            measured: ext.axtcb1_ext,
        });
    };
    match verify_extended(&ext, &expected) {
        Ok(()) => {
            eprintln!(
                "✓ extended TCB verified against pin: {} (4/4 components)",
                ext.axtcb1_ext
            );
            Ok(())
        }
        Err(e) => Err(AdmitError::ExtendedTcbMismatch {
            detail: e.to_string(),
            expected,
            measured: ext.axtcb1_ext,
        }),
    }
}

/// Effects in `forced` that the manifest's `union` does not grant.
///
/// Empty means the override is a subset — a narrowing, which is what
/// `AXON_VM_ALLOWED_EFFECTS` is for. Anything returned is an attempted WIDENING
/// of a program's own signed grant by an environment variable, which R36 §S0
/// names as a fail-open policy-provenance default.
pub fn effects_not_granted_by(forced: &[String], union: &[String]) -> Vec<String> {
    forced
        .iter()
        .filter(|e| !union.contains(e))
        .cloned()
        .collect()
}

/// Where the pinned extended-TCB (`axtcb1-ext:`) baseline lives (AUDIT T52).
/// Sibling of [`kernel_baseline_path`], same no-trust-on-first-use rule.
pub fn extended_baseline_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.axon/axtcb1_ext_baseline", home))
}

/// Path of the on-disk kernel baseline pin (`~/.axon/kernel_baseline.sha256`).
pub fn kernel_baseline_path() -> PathBuf {
    let home = std::env::var("HOME").unwrap_or_default();
    PathBuf::from(format!("{}/.axon/kernel_baseline.sha256", home))
}

/// Measure the kernel at `kernel_path` and verify it against a PINNED expected
/// digest — either `expect_digest` (operator-supplied, strongest) or the stored
/// baseline at `baseline_path`.
///
/// - Mismatch: `Err` (kernel tampered / wrong image).
/// - **No pin at all: also a refusal.** There is deliberately no trust-on-first-use
///   here. TOFU against a user-writable file is not a gate: an attacker who can
///   swap the kernel can also `rm` the baseline, and the next boot would silently
///   bless the tampered image as the new baseline (P7-KRN-05). Establish a
///   baseline explicitly with `axon-vm attest --kernel <path> --pin-baseline`.
/// - `no_attest = true`: prints a WARNING and short-circuits to `Ok` (dev mode).
///   This is the ONLY bypass. `AXON_CI_NO_KVM=1` used to disable the gate here as
///   well and no longer does.
///
/// Uses `axon_attest::measure_kernel`, so the digest is byte-identical with what
/// `axon-vm attest` records.
pub fn measure_and_attest_inner(
    kernel_path: &Path,
    no_attest: bool,
    expect_digest: Option<&str>,
    baseline_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if no_attest {
        eprintln!("[axon-vm] WARNING: --no-attest: skipping attestation (dev mode only)");
        return Ok(());
    }

    // Kernel must exist before we can measure it.
    if !kernel_path.exists() {
        return Err(format!("kernel not found: {}", kernel_path.display()).into());
    }

    let measurement = measure_kernel(kernel_path)?;
    let digest_hex = hex::encode(measurement.digest);

    // An operator-supplied pin wins over the on-disk baseline: it does not depend
    // on a file the attacker can reach.
    let (expected, source) = match expect_digest {
        Some(d) => (d.trim().to_string(), "--expect-digest"),
        None => match fs::read_to_string(baseline_path) {
            Ok(b) => (b.trim().to_string(), "baseline"),
            Err(_) => {
                eprintln!("[axon-vm] ATTESTATION FAILED: no pinned kernel baseline");
                eprintln!("[axon-vm]   measured: {digest_hex}");
                eprintln!(
                    "[axon-vm]   expected: (none — {} is absent)",
                    baseline_path.display()
                );
                eprintln!(
                    "[axon-vm]   pin it explicitly:  axon-vm attest --kernel {} --pin-baseline",
                    kernel_path.display()
                );
                eprintln!("[axon-vm]   or pass:            --expect-digest <sha256>");
                eprintln!("[axon-vm]   or, for dev only:   --no-attest");
                return Err(
                    "attestation failed: no pinned baseline (refusing to trust on first use)"
                        .into(),
                );
            }
        },
    };

    if expected != digest_hex {
        eprintln!("[axon-vm] ATTESTATION FAILED: kernel digest mismatch");
        eprintln!("[axon-vm]   expected: {expected} ({source})");
        eprintln!("[axon-vm]   got:      {digest_hex}");
        return Err("attestation failed: kernel tampered".into());
    }
    eprintln!(
        "[axon-vm] attestation OK: digest {} ({source})",
        &digest_hex[..16]
    );

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axon_attest::measure_kernel_bytes;

    fn v(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    struct Fixture {
        _dir: tempfile::TempDir,
        kernel: PathBuf,
        digest: String,
        absent_baseline: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::tempdir().unwrap();
        let kernel = dir.path().join("vmlinuz");
        let bytes = b"admit-test-kernel";
        fs::write(&kernel, bytes).unwrap();
        let digest = hex::encode(measure_kernel_bytes(bytes).digest);
        let absent_baseline = dir.path().join("no-such-baseline");
        Fixture {
            _dir: dir,
            kernel,
            digest,
            absent_baseline,
        }
    }

    /// A request that passes every gate: a manifest grant and a matching pin.
    fn ok_req<'a>(f: &'a Fixture, manifest: &'a [String]) -> AdmitRequest<'a> {
        AdmitRequest {
            run_id: "r1",
            kernel: &f.kernel,
            principal: None,
            manifest_effects: Some(manifest),
            principal_effects: None,
            effects_override: None,
            budget_tokens: None,
            source_hash: None,
            seccomp_bpf_b64: None,
            kernel_pin: KernelPin::Verify {
                expect_digest: Some(&f.digest),
                baseline: &f.absent_baseline,
            },
            extended_tcb: None,
        }
    }

    #[test]
    fn pinned_kernel_and_manifest_grant_are_admitted() {
        let f = fixture();
        let m = v(&["IO", "FS"]);
        let a = admit(&ok_req(&f, &m)).expect("admitted");
        assert_eq!(a.allowed_effects(), &m[..]);
        assert_eq!(a.kernel(), f.kernel.as_path());
        assert_eq!(a.mmds().run_id, "r1");
        assert_eq!(a.mmds().schema, "axon-vm-mmds/1");
    }

    #[test]
    fn baseline_file_pin_is_admitted() {
        let f = fixture();
        let base = f.kernel.with_file_name("baseline");
        fs::write(&base, format!("{}\n", f.digest)).unwrap();
        let m = v(&["IO"]);
        let mut r = ok_req(&f, &m);
        r.kernel_pin = KernelPin::Verify {
            expect_digest: None,
            baseline: &base,
        };
        assert!(admit(&r).is_ok());
    }

    #[test]
    fn null_grant_is_refused() {
        let f = fixture();
        let m = v(&[]);
        let mut r = ok_req(&f, &m);
        r.manifest_effects = None;
        r.principal_effects = None;
        r.effects_override = None;
        let e = admit(&r).unwrap_err();
        assert_eq!(e, AdmitError::NoEffectGrant);
        assert_eq!(e.exit_code(), 2);
        assert_eq!(e.to_string(), NO_EFFECT_GRANT_MSG);
    }

    #[test]
    fn empty_grant_is_deny_all_not_null() {
        let f = fixture();
        // An explicitly empty manifest union: admitted, grants nothing.
        let m = v(&[]);
        let a = admit(&ok_req(&f, &m)).expect("empty grant admitted");
        assert!(a.allowed_effects().is_empty());
        let json = serde_json::to_value(a.mmds()).unwrap();
        assert_eq!(json["allowed_effects"], serde_json::json!([]));

        // An explicitly empty override with no manifest: same.
        let mut r = ok_req(&f, &m);
        r.manifest_effects = None;
        r.effects_override = Some("");
        let a = admit(&r).expect("empty override admitted");
        assert!(a.allowed_effects().is_empty());
    }

    #[test]
    fn principal_grant_is_used_when_there_is_no_manifest() {
        let f = fixture();
        let m = v(&[]);
        let p = v(&["AI"]);
        let mut r = ok_req(&f, &m);
        r.manifest_effects = None;
        r.principal_effects = Some(&p);
        assert_eq!(admit(&r).unwrap().allowed_effects(), &p[..]);
    }

    #[test]
    fn widening_override_is_refused_and_narrowing_is_admitted() {
        let f = fixture();
        let m = v(&["IO"]);
        let mut r = ok_req(&f, &m);
        r.effects_override = Some("IO,Net");
        let e = admit(&r).unwrap_err();
        assert_eq!(
            e,
            AdmitError::OverrideWidens {
                not_granted: v(&["Net"]),
                manifest_grants: v(&["IO"]),
            }
        );
        assert_eq!(e.exit_code(), 2);

        let m2 = v(&["IO", "FS"]);
        let mut r = ok_req(&f, &m2);
        r.effects_override = Some("FS");
        assert_eq!(admit(&r).unwrap().allowed_effects(), &v(&["FS"])[..]);
    }

    #[test]
    fn unpinned_kernel_is_refused_no_tofu() {
        let f = fixture();
        let m = v(&["IO"]);
        let mut r = ok_req(&f, &m);
        r.kernel_pin = KernelPin::Verify {
            expect_digest: None,
            baseline: &f.absent_baseline,
        };
        let e = admit(&r).unwrap_err();
        assert!(
            matches!(e, AdmitError::KernelAttestation(ref s) if s.contains("no pinned baseline"))
        );
        assert_eq!(e.exit_code(), 10);
        assert!(
            !f.absent_baseline.exists(),
            "admission must never establish a pin"
        );
    }

    #[test]
    fn wrong_expect_digest_is_refused() {
        let f = fixture();
        let m = v(&["IO"]);
        let wrong = "0".repeat(64);
        let mut r = ok_req(&f, &m);
        r.kernel_pin = KernelPin::Verify {
            expect_digest: Some(&wrong),
            baseline: &f.absent_baseline,
        };
        let e = admit(&r).unwrap_err();
        assert_eq!(
            e,
            AdmitError::KernelAttestation("attestation failed: kernel tampered".into())
        );
        assert_eq!(e.exit_code(), 10);
    }

    #[test]
    fn extended_tcb_without_pin_is_refused_and_with_pin_admitted() {
        let f = fixture();
        let os = f.kernel.with_file_name("axon-os");
        fs::write(&os, b"os").unwrap();
        let ext_base = f.kernel.with_file_name("ext-baseline");
        let m = v(&["IO"]);
        let mut r = ok_req(&f, &m);
        r.extended_tcb = Some(ExtendedTcb {
            axon_os: &os,
            axon_audit: None,
            expect: None,
            baseline: &ext_base,
        });
        let e = admit(&r).unwrap_err();
        let AdmitError::ExtendedTcbUnpinned { measured } = e.clone() else {
            panic!("expected unpinned refusal, got {e:?}")
        };
        assert_eq!(e.exit_code(), 10);
        assert!(!ext_base.exists(), "admission must never establish a pin");

        // Pinned to what was measured → admitted.
        r.extended_tcb = Some(ExtendedTcb {
            axon_os: &os,
            axon_audit: None,
            expect: Some(&measured),
            baseline: &ext_base,
        });
        assert!(admit(&r).is_ok());

        // Tamper a component → mismatch against the same pin.
        fs::write(&os, b"os-TAMPERED").unwrap();
        let e = admit(&r).unwrap_err();
        assert!(matches!(e, AdmitError::ExtendedTcbMismatch { .. }), "{e:?}");
        assert_eq!(e.exit_code(), 10);

        // Missing component → measure failure, exit 12.
        fs::remove_file(&os).unwrap();
        let e = admit(&r).unwrap_err();
        assert!(matches!(e, AdmitError::ExtendedTcbMeasure(_)), "{e:?}");
        assert_eq!(e.exit_code(), 12);
    }

    #[test]
    fn dev_bypass_is_the_only_way_past_an_unpinned_kernel() {
        let f = fixture();
        let m = v(&["IO"]);
        let mut r = ok_req(&f, &m);
        r.kernel_pin = KernelPin::DevBypass;
        assert!(admit(&r).is_ok());
    }

    #[test]
    fn a_null_grant_payload_does_not_deserialize() {
        // `allowed_effects` is no longer optional: a `null` grant cannot even be
        // represented as an MmdsPayload.
        let r: Result<MmdsPayload, _> = serde_json::from_str(
            r#"{"schema":"axon-vm-mmds/1","run_id":"r","principal":null,"allowed_effects":null,
               "budget_tokens":null,"source_hash":null,"seccomp_bpf_b64":null}"#,
        );
        assert!(r.is_err());
    }
}
