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
//! Limitations of the Linux profile that are REFUSALS here (B263 x1/x2): it
//! has no guest policy channel, so a request that needs an effect ceiling
//! delivered into the guest is ineligible; and it does not preserve
//! path-scoped grants, so a path-scoped grant is ineligible.

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
    job_kinds: &[JobKind::InterpreterRun],
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
    /// `profiles/linux-microvm/manifest.json`.
    pub manifest: PathBuf,
    /// The built artifacts (`dist/guest-linux`), if not the launcher default.
    pub artifacts_dir: Option<PathBuf>,
    /// The qualification evidence record (`axon-b263-evidence/1`).
    pub evidence: PathBuf,
    /// Parent of per-operation `--out` directories.
    pub out_root: PathBuf,
}

/// The facts eligibility is decided from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinuxQualification {
    pub manifest_sha256: String,
    pub evidence_manifest_sha256: String,
    pub guest_axon_sha256: String,
}

fn sha256_file(p: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let b = std::fs::read(p).map_err(|e| format!("{}: {e}", p.display()))?;
    Ok(format!("{:x}", Sha256::digest(&b)))
}

impl LinuxProfileConfig {
    /// Eligible only if the manifest in use is byte-identical to the one the
    /// evidence record qualified, and that record has zero FAIL assertions. A
    /// changed manifest is ineligible — never "probably fine".
    pub fn qualification(&self) -> Result<LinuxQualification, String> {
        let manifest_sha256 = sha256_file(&self.manifest)?;
        let ev: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&self.evidence)
                .map_err(|e| format!("evidence {}: {e}", self.evidence.display()))?,
        )
        .map_err(|e| format!("evidence is not JSON: {e}"))?;
        if ev["schema"] != "axon-b263-evidence/1" {
            return Err("evidence record schema is not axon-b263-evidence/1".into());
        }
        if ev["profile"]["name"] != LINUX_MICROVM_PROTECTED.id {
            return Err("evidence record is for a different profile".into());
        }
        if ev["counts"]["FAIL"].as_u64() != Some(0) {
            return Err("evidence record has FAIL assertions (or none counted)".into());
        }
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
        let m: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(&self.manifest).map_err(|e| e.to_string())?,
        )
        .map_err(|e| format!("manifest is not JSON: {e}"))?;
        let guest_axon_sha256 = m["artifacts"]["axon"]["sha256"]
            .as_str()
            .ok_or("manifest has no artifacts.axon.sha256")?
            .to_string();
        Ok(LinuxQualification {
            manifest_sha256,
            evidence_manifest_sha256,
            guest_axon_sha256,
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
        lx.qualification()
            .map_err(|why| Unsupported(format!("{} ineligible: {why}", p.id)))?;
        if !p.job_kinds.contains(&req.job_kind) {
            return Err(Unsupported(format!(
                "{}: job_kind {:?} unsupported — the profile runs one program with `axon run` and \
                 reports its exit; it does not run registered checks or produce a verdict",
                p.id, req.job_kind
            )));
        }
        if needs.guest_policy_channel {
            return Err(Unsupported(format!(
                "{}: the request needs an effect ceiling inside the guest, and this profile has no \
                 guest policy channel (B263 x1)",
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

/// Run one `interpreter_run` through the Linux profile launcher.
pub fn run_linux_profile(
    lx: &LinuxProfileConfig,
    program: &Path,
    req: &ComputeRequest,
) -> LinuxRun {
    let out = lx.out_root.join(req.operation_id.as_str());
    let timeout_s = req.limits.wall_time_ms.div_ceil(1000).max(1);
    let mut cmd = std::process::Command::new(&lx.launcher);
    cmd.arg("--program")
        .arg(program)
        .arg("--out")
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
                evidence: vec![],
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
    let (outcome, reason, evidence) = interpret_linux_result(exit, &out, &mut verify);
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
        assert!(!LINUX_MICROVM_PROTECTED
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
