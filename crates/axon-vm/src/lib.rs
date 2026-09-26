//! axon-vm library target (B262).
//!
//! The `axon-vm` binary used to be the only target in this crate, so nothing
//! else could launch a microVM without shelling out to it. The Firecracker
//! launch path now lives here and the CLI calls it; the CLI's observable
//! behaviour is pinned by `tests/cli_parity.rs` (goldens captured from the
//! pre-extraction binary).
//!
//! Read [`firecracker::BACKEND_PROFILE`] before depending on this: it is a
//! jailer-less Firecracker running the custom Axon guest kernel, NOT Linux, and
//! it is not qualified as a protected microVM.

pub mod admit;
pub mod firecracker;

pub use admit::{admit, AdmitError, AdmitRequest, AdmittedLaunch, ExtendedTcb, KernelPin};
pub use firecracker::{
    run_in_firecracker, BackendProfile, FirecrackerBin, GuestOutcome, LaunchSpec, MmdsPayload,
    RunResult, BACKEND_PROFILE,
};
