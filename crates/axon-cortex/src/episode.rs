//! Episode events — the evidence trail for one repair attempt (CX-10).
//!
//! An episode is append-only and replayable. Each event names what happened and
//! what it happened TO (by digest), so a replay can prove it re-ran the same
//! episode rather than a similar one.

use crate::{content_digest, ContractError};
use serde::{Deserialize, Serialize};

/// One step in a repair episode. Closed enum: an unrecognised kind refuses.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EpisodeEvent {
    /// The workspace region the episode may touch, pinned by digest.
    Snapshot {
        snapshot_id: String,
        snapshot_digest: String,
    },
    /// What the observer saw, and what it could not.
    Observed {
        observation_id: String,
        fact_count: usize,
        omission_count: usize,
    },
    /// An action the catalog ALLOWED, with the grant that authorised it.
    ActionAllowed {
        action: String,
        grant_id: String,
        target_digest: String,
    },
    /// An action the catalog REFUSED. Recorded with the same weight as an
    /// allowed one: an episode that logs only what it did cannot show what it
    /// was stopped from doing, which is the half a reviewer needs.
    ActionDenied { action: String, reason: String },
    /// A patch applied to the COPY, never to the original.
    PatchApplied {
        path: String,
        before_digest: String,
        after_digest: String,
    },
    /// A registered check, with its real exit code.
    ///
    /// A PROCESS RAN. Nothing else belongs in this variant. Two events used to
    /// be pushed here that were not checks at all — a candidate's own claim of
    /// completion, and the identity of whichever generator proposed a patch —
    /// each with `exit_code: 0, passed: true` invented, because the variant had
    /// no other shape to carry them. `Claimed` and `Proposed` exist so that is
    /// no longer necessary.
    CheckRun {
        name: String,
        exit_code: i32,
        passed: bool,
    },
    /// The candidate asserted it is finished. NOT evidence that it is.
    ///
    /// Carries no exit code and no `passed`, deliberately: nothing ran, so
    /// there is nothing to report an outcome for. `verify()` adjudicates the
    /// claim against a check the candidate cannot modify, and that adjudication
    /// is a separate `Verified` event.
    ///
    /// This used to be pushed as `CheckRun { name: "claim_done", exit_code: 0,
    /// passed: claim.done }` — a self-report rendered as a check that ran and
    /// returned success. The crate's own first rule says a missing fact cannot
    /// be silently rendered as a present one; this is that rule applied to the
    /// crate's own evidence trail.
    Claimed { rationale: String },
    /// WHO proposed a patch, recorded before it is applied.
    ///
    /// Provenance, not a verdict — so it carries neither an exit code nor a
    /// `passed` flag. Once more than one generator exists, the only interesting
    /// question about an outcome is which one produced it, and an episode that
    /// did not record this cannot answer it later.
    Proposed { generator_id: String },
    /// A patch UNDONE because applying it made the file stop compiling.
    ///
    /// Recorded distinctly from `PatchApplied`, and never by deleting that
    /// event: the episode is append-only, so the record must show that
    /// something was tried and withdrawn rather than quietly showing a
    /// workspace that was never touched. An episode that hid its reverts would
    /// make a generator that breaks the build indistinguishable from one that
    /// never proposes anything.
    PatchReverted {
        path: String,
        /// Why it was withdrawn, in the terms the observation gave.
        reason: String,
        /// The digest the file was restored TO — the same bytes as the
        /// corresponding `PatchApplied`'s `before_digest`, so the pair can be
        /// matched up by a reader who was not there.
        restored_digest: String,
    },
    /// The independent verifier's verdict — produced out-of-band from whatever
    /// proposed the patch.
    Verified { passed: bool, detail: String },
}

/// An append-only episode.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Episode {
    pub episode_id: String,
    pub events: Vec<EpisodeEvent>,
}

impl Episode {
    pub fn new(episode_id: impl Into<String>) -> Self {
        Episode {
            episode_id: episode_id.into(),
            events: Vec::new(),
        }
    }

    pub fn push(&mut self, e: EpisodeEvent) {
        self.events.push(e);
    }

    /// Identity of the whole episode, over its events in order.
    ///
    /// Order is part of the identity: the same events in a different sequence
    /// are a different episode, because "checked then patched" and "patched
    /// then checked" are different claims about what was verified.
    pub fn digest(&self) -> Result<String, ContractError> {
        // The ID is covered. It is documented as "identity of the whole
        // episode" and hashed only the events, so the one field that could
        // distinguish two runs was outside the thing that attests them — and
        // every Runner was constructed with the same constant id besides, so
        // two repairs of different files with coincidentally identical event
        // sequences were indistinguishable by both.
        let body = crate::to_canonical_json(&(&self.episode_id, &self.events))?;
        Ok(content_digest(body.as_bytes()))
    }

    /// Did an independent verifier pass this episode?
    ///
    /// THE LAST `Verified` event is the verdict. Absent verification is NOT
    /// success — an episode with no `Verified` event returns `false`, the same
    /// absent-vs-passed rule the contracts enforce for observations.
    ///
    /// This used to be `.any(|e| matches!(e, Verified { passed: true, .. }))`,
    /// which reported an episode verified when an EARLIER, SUPERSEDED check had
    /// passed. REPRODUCED against `fixtures/pre_existing_failure.ax`:
    ///
    /// ```text
    /// outcome      = AdjudicatedNotClean { still_failing: ["visible_other"] }
    /// verified_ok  = true                       <-- exit 27, reported verified
    ///   Verified { passed: false, "hidden check FAILED" }
    ///   Verified { passed: true,  "hidden check passed" }
    ///   Verified { passed: false, "...1 check(s) ALREADY failing still fail" }
    /// ```
    ///
    /// The two positives are not independent verdicts that happen to share a
    /// variant. `Runner::verify` asks about ONE hidden check; the `ClaimDone`
    /// branch then re-asks over the WHOLE file and is documented as
    /// authoritative over it — "a file that still fails checks has not been
    /// repaired, whatever the adjudicator says about its own test". `.any()`
    /// picked the superseded narrower positive over the authoritative later
    /// negative.
    ///
    /// NOT `.all()`. An early exploratory failure followed by a later pass — a
    /// claim refused once and then correctly re-claimed — is a genuinely
    /// verified episode, and `.all()` would reject it. The rule is ordering,
    /// not conjunction, which is consistent with `digest()`'s own statement
    /// that EVENT ORDER IS PART OF EPISODE IDENTITY: "checked then patched" and
    /// "patched then checked" are different claims.
    ///
    /// The contract this restores is stated by the crate's own test
    /// `cli_ships_evidence_a_reader_can_verify`: "its own verdict agrees with
    /// the exit code — two independent readers of the same run must not be able
    /// to disagree."
    pub fn verified_ok(&self) -> bool {
        self.events
            .iter()
            .rev()
            .find_map(|e| match e {
                EpisodeEvent::Verified { passed, .. } => Some(*passed),
                _ => None,
            })
            .unwrap_or(false)
    }

    /// Every action the catalog refused. Reviewers read this first.
    pub fn denials(&self) -> Vec<&str> {
        self.events
            .iter()
            .filter_map(|e| match e {
                EpisodeEvent::ActionDenied { action, .. } => Some(action.as_str()),
                _ => None,
            })
            .collect()
    }
}
