//! The creative step, kept outside the control loop.
//!
//! Cortex can observe, select, authorize, execute and verify — everything
//! except invent the content of a repair. That gap was reported honestly as
//! [`crate::runner::EpisodeOutcome::NeedsInput`], and this is the seam that
//! fills it.
//!
//! The shape is deliberate: a generator contributes ONE thing, a function body,
//! and contributes it as DATA. It does not choose actions, does not touch the
//! filesystem, and is never handed a tool. Everything it proposes re-enters the
//! same typed pipeline as any other action — authorized against the same grant,
//! executed through the same witness, adjudicated by the same hidden check.
//!
//! That division is the whole point. A model driving the loop can do anything
//! the loop can do; a model filling a hole in the loop can only propose text
//! that the loop then has to accept on its own terms.

use crate::action::SymbolRef;
use crate::Observation;

/// What a proposal must respect. Passed to the generator so the constraints are
/// stated rather than assumed, and checked by the caller afterwards regardless
/// — a generator that ignores them is a possibility the loop has to survive.
#[derive(Debug, Clone)]
pub struct PatchConstraints {
    /// The symbol whose body is being replaced. A proposal for anything else is
    /// out of scope by construction.
    pub symbol: SymbolRef,
    /// Upper bound on the proposed body. Not a security control — the grant is
    /// that — but a bound on the obviously-wrong.
    pub max_bytes: usize,
}

/// A generated body, plus WHO generated it.
///
/// `generator_id` is required, not optional, and is recorded in the episode
/// from the first version. The moment two generators exist, the only
/// interesting question is which one produced a given outcome, and an episode
/// that did not record it cannot answer that retrospectively.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedPatch {
    pub body: String,
    pub generator_id: String,
}

/// Why no proposal was produced.
///
/// Three reasons, not one, because they imply different remedies — the same
/// discipline `EpisodeOutcome` applies to how an episode ends. "It did not
/// work" would collapse a missing API key, a model declining, and a malformed
/// answer into one unactionable fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GenerationFailure {
    /// No generator could be reached — offline, unconfigured, out of quota.
    /// An infrastructure fact, not a statement about the task.
    Unavailable(String),
    /// A generator was reached and declined to propose.
    Declined(String),
    /// Something was returned and it is not usable — empty, oversized, or
    /// otherwise failing the stated constraints.
    Invalid(String),
}

impl std::fmt::Display for GenerationFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GenerationFailure::Unavailable(s) => write!(f, "generator unavailable: {s}"),
            GenerationFailure::Declined(s) => write!(f, "generator declined: {s}"),
            GenerationFailure::Invalid(s) => write!(f, "proposal invalid: {s}"),
        }
    }
}

/// Supplies the one thing the control loop cannot.
pub trait PatchGenerator {
    /// Stable identity of this generator — model name and version, or the name
    /// of a deterministic strategy. Recorded in the episode, so two runs of the
    /// same loop with different generators are distinguishable afterwards.
    fn id(&self) -> String;

    /// Propose a body for `target`, given what was observed.
    ///
    /// Receives the observation rather than the file, so a generator sees what
    /// Cortex saw — including the omissions and the Unknown facts. Handing it
    /// the raw workspace would make it a second observer with a different view,
    /// and then a disagreement between them would have no arbiter.
    fn propose(
        &self,
        observation: &Observation,
        target: &SymbolRef,
        constraints: &PatchConstraints,
    ) -> Result<ProposedPatch, GenerationFailure>;
}

/// Check a proposal against its constraints.
///
/// Run by the CALLER, after the generator returns, on every proposal. A
/// generator that respects the constraints passes this trivially; one that does
/// not is caught here rather than at the filesystem. The bound is re-checked
/// rather than trusted because "the constraints were passed in" is not evidence
/// they were honoured.
pub fn validate(
    patch: &ProposedPatch,
    constraints: &PatchConstraints,
) -> Result<(), GenerationFailure> {
    if patch.generator_id.trim().is_empty() {
        return Err(GenerationFailure::Invalid(
            "proposal carries no generator_id; an unattributable patch cannot be recorded"
                .to_string(),
        ));
    }
    if patch.body.trim().is_empty() {
        // An empty body is not a minimal repair. It would delete the function's
        // behaviour while looking like a successful proposal.
        return Err(GenerationFailure::Invalid(format!(
            "empty body proposed for `{}`",
            constraints.symbol.symbol
        )));
    }
    if patch.body.len() > constraints.max_bytes {
        return Err(GenerationFailure::Invalid(format!(
            "proposed body is {} bytes, limit is {}",
            patch.body.len(),
            constraints.max_bytes
        )));
    }
    Ok(())
}
