//! R21 §4.7 + §5.1 — the `Runtime` seam: the firewall between the pure
//! supervisor and the outside world (the model / the interpreter / the OS).
//!
//! The supervisor is generic over `Runtime`, so S1–S5 are testable with a
//! `MockRuntime` and never touch `axon-core`. The real `AxonCoreRuntime` (the
//! only impure module) lands in S6.

use crate::gate::DeclaredEffects;
use crate::grant::{Budget, EffectSet, Grant};
use crate::record::RawEvent;
use crate::verdict::Verdict;
use std::path::Path;

/// An opaque handle to a minted Principal in the runtime's registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalHandle(pub usize);

/// The result of running a program inside the sandbox: the observed
/// capability-bearing actions and the sealing verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOutcome {
    pub events: Vec<RawEvent>,
    pub verdict: Verdict,
}

/// The seam. Every method that touches the model/interpreter/OS lives here.
pub trait Runtime {
    /// The effect row a program declares it may perform. An error / absent
    /// declaration MUST map to `DeclaredEffects::unknown()` (deny-by-default).
    fn declared_effects(&self, program: &Path) -> DeclaredEffects;

    /// Mint a Principal holding exactly `grant`. Attenuation (no authority the
    /// supervisor lacks) is guaranteed by the caller passing the effective
    /// grant `J ∩ S`; the runtime mints to that, no more.
    fn mint_principal(&self, grant: &Grant) -> PrincipalHandle;

    /// Run `program` as `principal` inside a sandbox enforcing `ceiling` +
    /// `budget` with a fixed `seed`. Returns the observed events and the verdict
    /// (mapping any runtime over-reach to Denied/BudgetExhausted/RefineViolation).
    fn run_sandboxed(
        &self,
        program: &Path,
        principal: &PrincipalHandle,
        ceiling: EffectSet,
        budget: &Budget,
        seed: u64,
    ) -> RunOutcome;
}

/// A configurable in-memory `Runtime` for testing the supervisor with no I/O.
#[cfg(any(test, feature = "mock"))]
pub struct MockRuntime {
    pub declared: DeclaredEffects,
    pub outcome: RunOutcome,
    pub mint_calls: std::cell::Cell<usize>,
    pub run_calls: std::cell::Cell<usize>,
}

#[cfg(any(test, feature = "mock"))]
impl MockRuntime {
    pub fn new(declared: DeclaredEffects, outcome: RunOutcome) -> Self {
        MockRuntime {
            declared,
            outcome,
            mint_calls: std::cell::Cell::new(0),
            run_calls: std::cell::Cell::new(0),
        }
    }
}

#[cfg(any(test, feature = "mock"))]
impl Runtime for MockRuntime {
    fn declared_effects(&self, _program: &Path) -> DeclaredEffects {
        self.declared
    }
    fn mint_principal(&self, _grant: &Grant) -> PrincipalHandle {
        self.mint_calls.set(self.mint_calls.get() + 1);
        PrincipalHandle(0)
    }
    fn run_sandboxed(
        &self,
        _program: &Path,
        _principal: &PrincipalHandle,
        _ceiling: EffectSet,
        _budget: &Budget,
        _seed: u64,
    ) -> RunOutcome {
        self.run_calls.set(self.run_calls.get() + 1);
        self.outcome.clone()
    }
}
