//! Choosing the next typed action from an observation.
//!
//! The runner could observe and it could authorize, but nothing decided WHAT to
//! do. Selection is the missing link between them, and it is the point where
//! the observer's honesty has to be honoured rather than quietly discarded.
//!
//! The rule that shapes this module: **an observation that could not determine
//! the state must not produce an action.** `Observed::Unknown` exists so a fact
//! the checker could not establish is distinguishable from one it established
//! as false; a selector that treated "unknown" as "fine" would collapse the two
//! at the exact moment the distinction pays for itself — when deciding whether
//! to claim the work is done.
//!
//! So there is no default action and no fallback. Either the state is known and
//! a specific action follows from it, or selection is [`Selection::Blocked`]
//! with the reason the observation gave.

use crate::action::{CheckRef, CompletionClaim, CortexAction, SymbolRef};
use crate::{Observation, Observed};

/// The outcome of choosing. Not `Option<CortexAction>`: a `None` would say
/// nothing about WHY, and "no action available" is a state a caller has to be
/// able to act on and log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selection {
    Act(CortexAction),
    /// Nothing can be chosen, and here is why. Carrying the reason is the same
    /// discipline `Observed::Unknown` applies one layer down.
    Blocked(String),
}

impl Selection {
    pub fn action(&self) -> Option<&CortexAction> {
        match self {
            Selection::Act(a) => Some(a),
            Selection::Blocked(_) => None,
        }
    }
}

fn fact<'a>(obs: &'a Observation, key: &str) -> Option<&'a Observed<String>> {
    obs.facts.iter().find(|(k, _)| k == key).map(|(_, v)| v)
}

/// Choose the next action for `target`, given what was actually observed.
///
/// Three known states, three different actions, and one refusal:
///
/// | observed | action | why |
/// |---|---|---|
/// | does not compile | `Inspect` | you cannot patch what you have not read, and the selector cannot invent a patch body |
/// | compiles, warnings | `RunCheck` | it builds, but the checker objected — behaviour is worth confirming before anything is claimed |
/// | compiles cleanly | `ClaimDone` | nothing left that the observation can see |
/// | state unknown | `Blocked` | the checker could not say, and a guess here is how an unverified claim gets made |
///
/// `Inspect` rather than `PatchSymbolBody` for the failing case is deliberate.
/// A patch needs a body, and a selector that invented one would be fabricating
/// the very thing the episode exists to produce. Selection picks the next
/// INVESTIGATIVE step; generating the body is a separate, model-driven move.
pub fn select_action(obs: &Observation, target: &SymbolRef) -> Selection {
    let compiles = match fact(obs, "compiles") {
        Some(Observed::Known { value }) => value == "true",
        Some(Observed::Unknown { reason }) => {
            return Selection::Blocked(format!(
                "cannot choose an action: `compiles` is unknown ({reason})"
            ))
        }
        None => {
            return Selection::Blocked(
                "cannot choose an action: the observation carries no `compiles` fact".to_string(),
            )
        }
    };

    if !compiles {
        return Selection::Act(CortexAction::Inspect {
            target: target.clone(),
        });
    }

    // It compiles. Whether it compiles CLEANLY decides between confirming
    // behaviour and claiming completion — which is exactly why the observer
    // carries warnings separately instead of reporting "no errors" as "nothing
    // to report".
    match fact(obs, "compiles_cleanly") {
        Some(Observed::Known { value }) if value == "true" => {
            Selection::Act(CortexAction::ClaimDone {
                claim: CompletionClaim {
                    done: true,
                    rationale: format!(
                        "{} compiles with no diagnostics and no warnings",
                        target.path
                    ),
                },
            })
        }
        Some(Observed::Known { .. }) => Selection::Act(CortexAction::RunCheck {
            check: CheckRef {
                // The hidden check is named by the caller's convention; the
                // selector does not invent a check name it cannot know exists.
                // Naming the symbol keeps the choice traceable to the target.
                name: target.symbol.clone(),
                path: target.path.clone(),
            },
        }),
        Some(Observed::Unknown { reason }) => Selection::Blocked(format!(
            "compiles, but cleanliness is unknown ({reason}) — refusing to \
             choose between confirming behaviour and claiming completion"
        )),
        None => Selection::Blocked(
            "compiles, but the observation carries no `compiles_cleanly` fact".to_string(),
        ),
    }
}
