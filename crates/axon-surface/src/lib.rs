//! Phase-10 prose-to-AST compiler — `axon-surface`.
//!
//! Turns a structured-prose goal file (`*.md`) into the typed-AST
//! `*.ax` artifact a non-programmer can review and approve.
//!
//! # Phase 10 v0 (this crate)
//!
//! Deterministic scaffolding compiler.  Parses required sections
//! (`Intent`, `Inputs`, `Outputs`, `Score`, `Constraints`, `Budget`,
//! `Verify`, `Redteam`, `Effect surface`, `Provenance`).  Emits a
//! `.ax` skeleton with:
//!   - file-header comment derived from `Intent`
//!   - function signatures derived from `Inputs` / `Outputs`
//!   - `@[verify(...)]` annotation derived from `Verify` section's
//!     embedded predicate
//!   - placeholder bodies (`TODO:`) for test sets, candidates,
//!     and score logic — author fills these in, until later phases
//!     ship the LLM-driven full compiler.
//!
//! # Phase 10 v1+ (future work)
//!
//! - LLM-driven extraction: read the prose section bodies, generate
//!   real test sets / candidate variants / score logic.
//! - Round-trip verification: re-render the .ax back to prose, diff
//!   against original .md, surface drift.
//! - Multi-format input: YAML / TOML alternatives to markdown.
//!
//! # Module layout
//!
//! - [`parser`] — parses `.md` into a typed `GoalFile` struct.
//! - [`compile`] — emits `.ax` source from a `GoalFile`.
//! - [`error`] — `Error` + `Result` types.

pub mod compile;
pub mod error;
pub mod parser;

pub use error::{Error, Result};
pub use parser::{GoalFile, Section};
