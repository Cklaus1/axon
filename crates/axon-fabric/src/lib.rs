//! axon-fabric — the Axon compute fabric (v0.22, M1 slices).
//!
//! * [`journal`] — the durable, append-only operation journal (B260): intent is
//!   fsynced BEFORE any effect; `Intended → Reserved → Launched → (Completed |
//!   Failed | Cancelled | OutcomeUnknown)`; the same `OperationId` with a
//!   different input digest is a CONFLICT; reopening after a crash turns a
//!   `Launched` op with no terminal record into `OutcomeUnknown` with its
//!   liability kept, never re-executed. Aggregate reservations over a combined
//!   model/exec/verify/retry budget are carved atomically.
//! * [`submit`] — the Fabric submit path: an `acf-compute-request/1` in, an
//!   `acf-execution-receipt/1` out, with authority, isolation and the
//!   registered executable checked BEFORE the journal's launch record, and the
//!   journal wrapped around the effect.
//! * [`backend`] — the execution backends and their truthful profiles.
//!
//! Identities are `axon_loop_contracts` types throughout. Storage is plain
//! files — no database dependency.

pub mod backend;
pub mod journal;
pub mod submit;

pub use journal::{
    Begin, Billing, Intent, Journal, JournalError, OpState, OpView, RecoveryReport, ResourceVector,
    ScopeUsage, Settlement, JOURNAL_SCHEMA,
};
pub use submit::{submit, EpochSource, Submission, SubmitConfig, SubmitError};
