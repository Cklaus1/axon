//! A model-backed [`PatchGenerator`].
//!
//! This is the production implementation of the seam: the one place a real
//! model contributes to a Cortex episode, and the narrowest interface it could
//! have. It is handed a function body and the facts Cortex observed, and it
//! returns a string. It holds no tool, chooses no action, and never learns the
//! name of the check that will adjudicate its work.
//!
//! Behind the `ai` feature, because a controller that cannot be built without a
//! model dependency would make the model load-bearing for the parts that do not
//! need one.

use crate::action::SymbolRef;
use crate::generate::{GenerationFailure, PatchConstraints, PatchGenerator, ProposedPatch};
use crate::Observation;

/// Proposes bodies by asking a language model.
pub struct AiPatchGenerator {
    /// Recorded verbatim in the episode. Not a label chosen for readability:
    /// two runs of one loop against different models must be distinguishable
    /// afterwards, and nothing downstream can recover it if it is lost here.
    model: String,
}

impl AiPatchGenerator {
    /// `model` is the identity the episode records. It is required rather than
    /// defaulted, because a default would make every episode claim to have been
    /// produced by whatever the default happened to be that week.
    pub fn new(model: impl Into<String>) -> Self {
        Self {
            model: model.into(),
        }
    }
}

/// Strip a fenced code block if the model wrapped its answer in one.
///
/// Models fence code by habit even when told not to. Treating the fence as part
/// of the body would produce a file that does not parse, and the episode would
/// report a MODEL that proposed nonsense rather than a HARNESS that mangled a
/// good answer — a misattribution that would send the next person to tune a
/// prompt instead of fixing three lines here.
fn unfence(s: &str) -> String {
    let t = s.trim();
    let Some(rest) = t.strip_prefix("```") else {
        return s.to_string();
    };
    // Drop the info string on the opening fence (```axon, ```rust, …).
    let body = match rest.split_once('\n') {
        Some((_lang, b)) => b,
        None => return s.to_string(),
    };
    match body.rfind("```") {
        Some(end) => body[..end].to_string(),
        // An opening fence with no closing one is truncated output, not a
        // code block. Returning the input unchanged lets `validate` and the
        // compiler judge it, rather than silently inventing a terminator.
        None => s.to_string(),
    }
}

impl PatchGenerator for AiPatchGenerator {
    fn id(&self) -> String {
        format!("ai:{}", self.model)
    }

    fn propose(
        &self,
        observation: &Observation,
        target: &SymbolRef,
        constraints: &PatchConstraints,
    ) -> Result<ProposedPatch, GenerationFailure> {
        let prompt = crate::generate::build_prompt(observation, target, constraints);
        match axon_ai::complete_with_model(&prompt, &self.model) {
            // `Unavailable`, not `Declined`. Every error this call can return
            // — no key, no network, a gateway refusing — is a fact about the
            // infrastructure, and reporting it as the model declining would
            // send an operator to rewrite a prompt that was never sent.
            Err(e) => Err(GenerationFailure::Unavailable(e)),
            Ok(raw) => Ok(ProposedPatch {
                body: unfence(&raw),
                generator_id: self.id(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::unfence;

    #[test]
    fn a_fenced_answer_is_unwrapped_and_an_unfenced_one_is_untouched() {
        assert_eq!(unfence("    n * 2\n"), "    n * 2\n");
        assert_eq!(unfence("```axon\n    n * 2\n```"), "    n * 2\n");
        assert_eq!(unfence("```\n    n * 2\n```\n"), "    n * 2\n");
        // Truncated output keeps its fence rather than being silently closed:
        // validate and the compiler judge it, and the episode reports a bad
        // proposal instead of a mangled good one.
        assert_eq!(unfence("```axon\n    n * 2"), "```axon\n    n * 2");
    }
}
