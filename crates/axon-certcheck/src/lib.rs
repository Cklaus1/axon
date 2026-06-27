//! axon-certcheck (R23) — proof certificates + a minimal, solver-free checker
//! that gets Z3 out of the trusted base. See
//! `governance/specs/R23-proof-certificates.md`.
//!
//! The TRUSTED artifact is the **checker**: `check(obligation, cert)` re-derives
//! the verdict from the certificate with NO SMT solver in its dependency closure
//! (std + sha2 + serde only). Z3 lives only behind the `smt` feature, in
//! `emit::prove`, where it is an UNTRUSTED oracle that merely *suggests*
//! certificates. Soundness over completeness: the checker returns `Valid` only
//! after itself deriving the contradiction; otherwise `Unsupported`/`Invalid`,
//! never a false `Valid` (§12, I-1).

pub mod certificate;
pub mod checker;
pub mod cli;
pub mod eval;
pub mod obligation;
pub mod policy;
pub mod synth;

#[cfg(feature = "smt")]
pub mod emit;

pub use certificate::{FactOp, LinFact, ProofCertificate, Refutation};
pub use checker::{check, CheckResult, MAX_CERT_NODES};
pub use obligation::{Formula, Obligation, Op, ParseErr, Sort, Term, Var};
pub use policy::{require_certificates, RequireOutcome};

/// Parse an obligation from its `.obl` JSON form (§5.1).
pub fn parse_obligation(s: &str) -> Result<Obligation, ParseErr> {
    Obligation::parse(s)
}

/// Parse a certificate from its `.cert` JSON form (§5.1).
pub fn parse_certificate(s: &str) -> Result<ProofCertificate, ParseErr> {
    ProofCertificate::parse(s)
}
