//! `axon-os` — the Axon OS containment supervisor (R21).
//!
//! Run an untrusted Axon program under a declared, proven capability + budget
//! grant; enforce it; audit it into a tamper-evident record; replay it; fail
//! closed on any over-reach. See `governance/specs/R21-axon-os-supervisor.md`.
//!
//! Build status: S1 (data model + manifest parse/validate) landed. The pure
//! core (`manifest`/`grant`/`verdict`) carries no I/O; later slices add the
//! gate (S3), record (S4), supervisor over a `Runtime` seam (S5), the real
//! `AxonCoreRuntime` (S6), and the CLI (S7).

pub mod gate;
pub mod grant;
pub mod manifest;
pub mod record;
pub mod verdict;

pub use gate::{admit, Admission, DeclaredEffects};
pub use grant::{Budget, EffectSet, ExecPolicy, Grant, Label};
pub use manifest::{parse as parse_manifest, JobManifest};
pub use record::{
    build as build_record, verify as verify_record, AuditEvent, RawEvent, RunRecord, VerifyMismatch,
};
pub use verdict::Verdict;
