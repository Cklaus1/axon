//! axon-fabric — the Axon compute fabric (v0.22, M1 first slices).
//!
//! What exists here today:
//!
//! * [`journal`] — a durable, append-only operation journal (B260, first half):
//!   intent is recorded and fsynced BEFORE any effect, operations move through
//!   `Intended → Reserved → Launched → (Completed | Failed | Cancelled |
//!   OutcomeUnknown)`, the same operation id with a different request is a
//!   CONFLICT, and reopening after a crash RECONCILES: a `Launched` operation
//!   with no terminal record becomes `OutcomeUnknown` with its liability kept.
//!   It is never silently re-executed.
//! * aggregate reservations over a combined model/exec/verify/retry budget,
//!   carved atomically under the journal lock and rebuilt from the journal on
//!   reopen, so a restart cannot free budget.
//!
//! What does NOT exist yet (see the crate README in the final report): no
//! launcher is driven from here, no epoch/fence, no workspace store. Storage is
//! plain files — no database dependency.
//!
//! Identity types are a deliberate placeholder: [`ids::OpKey`] is the single
//! adapter point where `axon-loop-contracts`' `OperationId` replaces it.

pub mod ids;
pub mod journal;

pub use ids::{InputDigest, OpKey, ScopeKey};
pub use journal::{
    Begin, Billing, Intent, Journal, JournalError, OpState, OpView, RecoveryReport, ResourceVector,
    ScopeUsage, Settlement, JOURNAL_SCHEMA,
};
