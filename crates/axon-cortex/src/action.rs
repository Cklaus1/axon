//! Typed actions — what a Cortex principal may ask for, and with what.
//!
//! The catalog used to be five bare strings plus a generic `target_path`, so
//! every action could technically receive every parameter. The code ignored the
//! meaningless combinations, which is not the same as their being impossible:
//!
//! ```text
//! claim_done  + target_path = "../../etc/shadow"
//! run_check   + proposed_body = "…"
//! inspect     + a write payload
//! ```
//!
//! Each of those is a sentence the old API could say. Ignoring a field is a
//! runtime decision that has to be gotten right at every call site and re-read
//! by every reviewer; not having the field is a property of the type.
//!
//! So the parameters an action accepts are determined by the ACTION, and the
//! three states above are now unrepresentable rather than unhandled. The
//! corresponding refusals move from runtime `NotInCatalog` toward compile-time
//! impossibility, which is the direction worth travelling: a refusal that never
//! has to fire is stronger than one that does.
//!
//! `search` is deliberately absent. There is no search capability whose
//! semantics are strong enough to deserve the name yet, and adding the variant
//! now would be a promise the runtime cannot keep.

/// A symbol inside a file — the unit `Inspect` and `PatchSymbolBody` address.
///
/// Both the path and the symbol are required. A patch that names a file but no
/// symbol is a whole-file write wearing a symbol-edit's name, and the grant
/// semantics differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SymbolRef {
    /// Workspace-relative path.
    pub path: String,
    pub symbol: String,
}

/// A named check to run. Carries the file it belongs to so the runner does not
/// have to guess which target a check name refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckRef {
    pub name: String,
    pub path: String,
}

/// A claim that the task is complete.
///
/// It carries NO path, deliberately. Claiming done is not an edit and must not
/// be able to name a file to touch — `verify()` adjudicates the claim
/// independently, and a claim that could carry a filesystem target would invite
/// exactly the confusion `G03-done` exists to prevent.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionClaim {
    pub done: bool,
    /// Why the principal believes it is done. Recorded as evidence; it is not
    /// an input to the verdict. Confidence is not evidence.
    pub rationale: String,
}

/// The legal action catalog, as a closed type.
///
/// The two states below are not merely rejected at runtime — they do not
/// compile. These are `compile_fail` doctests, so `cargo test` proves the
/// claim rather than the comment asserting it.
///
/// A check cannot carry an edit payload:
///
/// ```compile_fail
/// use axon_cortex::action::{CortexAction, CheckRef};
/// let _ = CortexAction::RunCheck {
///     check: CheckRef { name: "c".into(), path: "a.ax".into() },
///     proposed_body: "fn f() {}".into(),   // no such field
/// };
/// ```
///
/// A completion claim cannot name a file to touch:
///
/// ```compile_fail
/// use axon_cortex::action::{CortexAction, CompletionClaim};
/// let _ = CortexAction::ClaimDone {
///     claim: CompletionClaim { done: true, rationale: "r".into() },
///     target_path: "../../etc/shadow".into(),   // no such field
/// };
/// ```
///
/// And an action outside the catalog cannot be spelled at all:
///
/// ```compile_fail
/// use axon_cortex::action::CortexAction;
/// let _ = CortexAction::DeleteEverything { path: "/".into() };
/// ```
///
/// There is no `Unknown` variant and no string escape hatch: an action outside
/// this set cannot be built at the typed boundary at all. `Refusal::NotInCatalog`
/// remains for the STRING edge — where a request arrives as JSON from another
/// process and has to be parsed — which is the only place an unknown action can
/// still appear.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CortexAction {
    /// Read a symbol. Needs no grant: reading is not editing.
    Inspect { target: SymbolRef },
    /// Run a named check. No write authority, and no patch payload — a check
    /// that could carry a body would be an edit by another name.
    RunCheck { check: CheckRef },
    /// Replace one symbol's body. The only variant with a write target.
    PatchSymbolBody {
        symbol: SymbolRef,
        proposed_body: String,
    },
    /// Claim completion. No filesystem effect is reachable from here.
    ClaimDone { claim: CompletionClaim },
}

impl CortexAction {
    /// The catalog name, for episode records and audit. Deriving this from the
    /// variant means the recorded name cannot drift from the action taken.
    pub fn name(&self) -> &'static str {
        match self {
            CortexAction::Inspect { .. } => "inspect",
            CortexAction::RunCheck { .. } => "run_check",
            CortexAction::PatchSymbolBody { .. } => "patch_symbol_body",
            CortexAction::ClaimDone { .. } => "claim_done",
        }
    }

    /// The path this action would WRITE, if any.
    ///
    /// `Some` for exactly one variant. The path checks — traversal, policy
    /// file, grant coverage — are asked of this, so for every other action
    /// there is no path to check rather than a path that is checked and
    /// ignored. That is the difference between "we do not apply the rule here"
    /// and "the rule has nothing to apply to".
    pub fn write_target(&self) -> Option<&str> {
        match self {
            CortexAction::PatchSymbolBody { symbol, .. } => Some(symbol.path.as_str()),
            CortexAction::Inspect { .. }
            | CortexAction::RunCheck { .. }
            | CortexAction::ClaimDone { .. } => None,
        }
    }

    /// Whether this action needs write authority at all.
    ///
    /// Kept distinct from `write_target().is_some()` on purpose: they agree
    /// today, and a future effectful action would have to state its answer to
    /// BOTH rather than inheriting one from the other.
    pub fn requires_write_authority(&self) -> bool {
        matches!(self, CortexAction::PatchSymbolBody { .. })
    }
}
