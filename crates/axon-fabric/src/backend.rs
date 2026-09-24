//! Execution backends and their TRUTHFUL profiles.
//!
//! A backend is chosen by what it IS, never by what the request would like it
//! to be. Two exist:
//!
//! * [`LocalInterpreter`] — `process_scoped/local-interpreter`: the
//!   registered Axon interpreter as a child process on this host (via
//!   `axon_cortex::runner::LocalInterpreterExecutor`). No hardware isolation.
//! * [`FirecrackerAxonKernel`] — the `axon-vm` library launch path, labelled
//!   with `axon_vm::BACKEND_PROFILE` (`axon-metal-fc-nojailer`): KVM, but no
//!   jailer, and the guest is the custom Axon kernel — NOT Linux, and not a
//!   complete execution environment (it demonstrates the syscall gate; it
//!   does not run the program's tests). It therefore cannot produce a
//!   verification verdict and says so.
//!
//! A request requiring `hardware_isolation: true` with `os: linux` is refused
//! by BOTH, and by selection: [`select`] returns `Unsupported` before anything
//! is journalled beyond intent. There is no fallback from one to the other.

use axon_loop_contracts::{ComputeRequest, Engine, JobKind, NetworkMode, Os};
use axon_os::runtime::Isolation;

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
    /// Qualified as a protected profile (B263). No backend here is.
    pub qualified_protected: bool,
    pub isolation: Isolation,
    /// Whether this backend can produce a check VERDICT (run tests and
    /// report which passed). A backend that cannot must never report one.
    pub produces_verdict: bool,
}

pub const LOCAL_INTERPRETER: Profile = Profile {
    id: axon_cortex::runner::LocalInterpreterExecutor::PROFILE,
    engine: Engine::AxonInterpreter,
    enclosure: "process_scoped",
    guest_kind: "none",
    os: Os::None,
    hardware_isolation: false,
    qualified_protected: false,
    isolation: Isolation::ProcessScoped,
    produces_verdict: true,
};

pub const FIRECRACKER_AXON_KERNEL: Profile = Profile {
    id: axon_vm::BACKEND_PROFILE.id,
    engine: Engine::AxonInterpreter,
    enclosure: "kvm_microvm",
    guest_kind: "axon_kernel_demo",
    os: Os::None,
    hardware_isolation: true,
    qualified_protected: axon_vm::BACKEND_PROFILE.qualified_protected,
    isolation: Isolation::KvmMicroVmUnqualified,
    produces_verdict: false,
};

/// Which backends this build offers, in preference order.
pub const ALL: &[Profile] = &[LOCAL_INTERPRETER, FIRECRACKER_AXON_KERNEL];

/// Why no backend was selected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unsupported(pub String);

/// Pick the backend that satisfies EVERY requirement, or refuse.
///
/// Rules, each a refusal rather than a downgrade:
/// * `os: linux` needs a Linux guest — none exists here (B263 blocked);
/// * `hardware_isolation: true` needs a hardware-isolated backend, and when
///   combined with Linux a QUALIFIED one;
/// * `network_mode: brokered` needs a broker — none exists;
/// * a `registered_check` needs a backend that produces a verdict.
pub fn select(req: &ComputeRequest) -> Result<Profile, Unsupported> {
    let r = &req.required;
    if r.os == Os::Linux {
        return Err(Unsupported(
            "os=linux requires a Linux guest profile; none is qualified on this host \
             (B263 blocked: no Linux guest image, no jailer). The Axon-kernel microVM is \
             not Linux and is never substituted for it"
                .into(),
        ));
    }
    if r.network_mode == NetworkMode::Brokered {
        return Err(Unsupported("network_mode=brokered: no egress broker exists".into()));
    }
    let mut why = Vec::new();
    for p in ALL {
        if p.engine != r.engine {
            why.push(format!("{}: engine {:?}", p.id, p.engine));
            continue;
        }
        if r.hardware_isolation && !p.hardware_isolation {
            why.push(format!("{}: no hardware isolation", p.id));
            continue;
        }
        if !r.hardware_isolation && p.hardware_isolation {
            // Not a refusal of the request — just not the preferred match.
            continue;
        }
        if req.job_kind == JobKind::RegisteredCheck && !p.produces_verdict {
            why.push(format!(
                "{}: cannot produce a check verdict (the Axon guest kernel does not run tests)",
                p.id
            ));
            continue;
        }
        return Ok(*p);
    }
    Err(Unsupported(format!(
        "no backend satisfies the request: {}",
        if why.is_empty() {
            "none offered".to_string()
        } else {
            why.join("; ")
        }
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profiles_are_truthful() {
        assert!(!LOCAL_INTERPRETER.hardware_isolation);
        assert_eq!(LOCAL_INTERPRETER.isolation.label(), "process_scoped");
        assert_eq!(FIRECRACKER_AXON_KERNEL.id, "axon-metal-fc-nojailer");
        assert_eq!(FIRECRACKER_AXON_KERNEL.os, Os::None, "the Axon kernel is not Linux");
        assert!(!FIRECRACKER_AXON_KERNEL.qualified_protected);
        assert!(!FIRECRACKER_AXON_KERNEL.produces_verdict);
        for p in ALL {
            assert!(!p.qualified_protected, "{}: nothing is qualified yet", p.id);
        }
    }
}
