//! B273 / B281 — one bounded EVO shortlist candidate + hypothesis memory.
//!
//! Inputs: the incumbent `PolicyEnvelope`, the already-eligible candidate set,
//! and episodes. Only DISCOVERY/TUNING episodes for which
//! `learning_eligible() == true` AND whose verification issuer is a
//! configured trusted verifier that is not the proposer (no self-label loop,
//! N4/N5) feed the proposer; a confirmation or
//! reporting episode in the input REFUSES the whole request (protected
//! evidence must never reach a proposer, even to be ignored), and
//! mechanism-test or ineligible episodes are excluded and reported.
//!
//! Mutation is ONE edit inside the incumbent shortlist: remove one entry
//! (never to empty) or swap two entries. It can never add an id, and the
//! candidate inherits scope/mode/candidate set/controls unchanged with
//! `authority_expansion: false`. The edit is chosen by a seeded
//! deterministic order over the whole (finite) mutation space; the first
//! edit whose resulting shortlist was NOT previously tried in this scope
//! (hypothesis history, incumbent included) wins — regularized: an identical
//! intervention is never re-proposed under a new id.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::{strict_record, Store};
use axon_loop_contracts::{
    check_shortlist, digest, learning_eligible, BoundedText, CandidateId, Contract, CorpusRole,
    LoopEpisode, OpaqueRef, PolicyEnvelope, PolicyId, Ref, Scope,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

crate::record_tag!(EvoRequestSchema, "axon.loop.evo-request/1");
crate::record_tag!(HypothesisSchema, "axon.loop.hypothesis/1");

/// `evo propose` input (this crate's record; nested contracts are
/// schema-checked by `axon_loop_contracts::parse`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvoRequest {
    pub schema: EvoRequestSchema,
    pub incumbent: serde_json::Value,
    pub eligible: Vec<CandidateId>,
    pub episodes: Vec<serde_json::Value>,
    pub seed: u64,
    pub new_policy_id: PolicyId,
    pub proposer_ref: OpaqueRef,
    pub rationale: BoundedText,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Mutation {
    Remove { candidate: CandidateId },
    Swap { a: CandidateId, b: CandidateId },
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Excluded {
    pub episode_ref: Ref,
    pub reason: String,
}

#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Accept,
    Reject,
    Inconclusive,
}

/// One line of `hypotheses.jsonl`. History is append-only: a verdict is a
/// NEW line, never an edit of the proposal.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum Hypothesis {
    Proposed {
        schema: HypothesisSchema,
        candidate_policy_ref: Ref,
        parent_policy_ref: Ref,
        /// The intervention's identity: the resulting shortlist over the fixed
        /// candidate set and controls. Wording/policy id does not change it.
        intervention: Vec<CandidateId>,
        mutation: Mutation,
        rationale: BoundedText,
        proposer_ref: OpaqueRef,
        seed: u64,
        discovery_evidence_refs: Vec<Ref>,
        excluded: Vec<Excluded>,
        proposed_ms: u64,
    },
    Verdict {
        schema: HypothesisSchema,
        candidate_policy_ref: Ref,
        verdict: Verdict,
        admission_ref: Ref,
        /// A mechanism-test verdict is a fixture, never improvement evidence.
        mechanism_test: bool,
        decided_ms: u64,
    },
}

#[derive(Clone, Debug, Serialize)]
pub struct Proposal {
    pub candidate: PolicyEnvelope,
    pub candidate_policy_ref: Ref,
    pub hypothesis: Hypothesis,
}

/// The scope's hypothesis history (ledger replay).
pub fn history(store: &Store, scope: &Scope) -> Result<Vec<Hypothesis>> {
    Ok(Tx::begin(store)?.hypotheses(scope, None))
}

/// The recorded proposer of an EVO candidate, if it was proposed here.
pub(crate) fn proposer_in(tx: &Tx, scope: &Scope, candidate: &Ref) -> Option<OpaqueRef> {
    tx.hypotheses(scope, None)
        .into_iter()
        .find_map(|h| match h {
            Hypothesis::Proposed {
                candidate_policy_ref,
                proposer_ref,
                ..
            } if &candidate_policy_ref == candidate => Some(proposer_ref),
            _ => None,
        })
}

/// splitmix64 — a fixed, documented PRNG so a seed means the same thing on
/// every platform and version of this crate.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// The complete one-edit mutation space, in a fixed canonical order.
fn mutation_space(list: &[CandidateId]) -> Vec<Mutation> {
    let mut m = Vec::new();
    if list.len() > 1 {
        for c in list {
            m.push(Mutation::Remove {
                candidate: c.clone(),
            });
        }
    }
    for i in 0..list.len() {
        for j in i + 1..list.len() {
            m.push(Mutation::Swap {
                a: list[i].clone(),
                b: list[j].clone(),
            });
        }
    }
    m
}

fn apply(list: &[CandidateId], m: &Mutation) -> Vec<CandidateId> {
    match m {
        Mutation::Remove { candidate } => {
            list.iter().filter(|c| *c != candidate).cloned().collect()
        }
        Mutation::Swap { a, b } => list
            .iter()
            .map(|c| {
                if c == a {
                    b.clone()
                } else if c == b {
                    a.clone()
                } else {
                    c.clone()
                }
            })
            .collect(),
    }
}

/// Parse an `evo propose` request strictly.
pub fn parse_request(text: &str) -> Result<EvoRequest> {
    strict_record(text)
}

/// Generate, store and record ONE candidate. Nothing is written on refusal.
pub fn propose(store: &Store, req: &EvoRequest) -> Result<Proposal> {
    let incumbent: PolicyEnvelope = crate::store::contract_from_value("incumbent", &req.incumbent)?;
    let scope = incumbent.scope.clone();
    let eligible: BTreeSet<CandidateId> = req.eligible.iter().cloned().collect();
    if eligible.len() != req.eligible.len() {
        return Err(LoopError::Malformed(axon_loop_contracts::Refusal::Shape(
            "eligible: duplicate candidate id".into(),
        )));
    }
    check_shortlist(
        &incumbent,
        &eligible,
        &incumbent.candidate_set_ref,
        &incumbent.controls_ref,
        &scope,
    )?;
    let incumbent_ref = digest(&incumbent)?;

    let verifiers = store.config()?.verifiers();
    // B281: only eligible discovery/tuning evidence feeds the proposer.
    let mut evidence = Vec::new();
    let mut excluded = Vec::new();
    for (i, v) in req.episodes.iter().enumerate() {
        let ep: LoopEpisode = crate::store::contract_from_value(&format!("episodes[{i}]"), v)?;
        let r = digest(&ep)?;
        if matches!(
            ep.corpus_role,
            CorpusRole::Confirmation | CorpusRole::Reporting
        ) {
            return Err(refused(format!(
                "episodes[{i}] ({r}) is {:?}-role evidence: protected outcomes never reach the proposer",
                ep.corpus_role
            )));
        }
        if ep.scope != scope || ep.candidate_set_ref != incumbent.candidate_set_ref {
            return Err(refused(format!(
                "episodes[{i}] is from a different scope or candidate view"
            )));
        }
        let issuer = ep.verification.issuer_ref.as_ref();
        let reason = if ep.corpus_role == CorpusRole::MechanismTest {
            Some("mechanism_test episodes never feed learning")
        } else if !learning_eligible(&ep)? {
            Some("not learning-eligible (needs completed, independently verified, final usage)")
        } else if issuer == Some(&req.proposer_ref) {
            Some("verified by the proposer itself: self-labelled evidence never feeds learning")
        } else if !issuer.is_some_and(|i| verifiers.contains(i)) {
            Some("verification issuer is not a configured trusted verifier")
        } else {
            None
        };
        match reason {
            Some(why) => excluded.push(Excluded {
                episode_ref: r,
                reason: why.into(),
            }),
            None => {
                if !evidence.contains(&r) {
                    evidence.push(r)
                }
            }
        }
    }
    if evidence.is_empty() {
        return Err(refused(
            "no learning-eligible discovery/tuning episode: nothing to propose from",
        ));
    }
    if evidence.len() > 256 {
        return Err(refused("more than 256 discovery evidence refs"));
    }

    let mut tx = Tx::begin(store)?;
    // G2: the eligible set is the REGISTERED candidate list for the
    // incumbent's view, never only the caller's claim. A caller list that
    // differs from it is refused (it could widen the mutation space).
    let registered = crate::candidates::resolve(&tx, &scope, &incumbent.candidate_set_ref)?;
    if registered.eligible() != eligible {
        return Err(refused(
            "eligible differs from the registered candidate list for the incumbent's candidate_set_ref",
        ));
    }
    crate::candidates::require_shortlist(&tx, &incumbent)?;
    let past: Vec<Hypothesis> = tx.hypotheses(&scope, None);
    let mut tried: BTreeSet<Vec<CandidateId>> = past
        .iter()
        .filter_map(|h| match h {
            Hypothesis::Proposed { intervention, .. } => Some(intervention.clone()),
            _ => None,
        })
        .collect();
    tried.insert(incumbent.shortlist.clone());

    let mut space = mutation_space(&incumbent.shortlist);
    // Seeded Fisher–Yates over the canonical order.
    let mut st = req.seed;
    for i in (1..space.len()).rev() {
        let j = (splitmix64(&mut st) % (i as u64 + 1)) as usize;
        space.swap(i, j);
    }
    let (mutation, shortlist) = space
        .into_iter()
        .map(|m| {
            let s = apply(&incumbent.shortlist, &m);
            (m, s)
        })
        .find(|(_, s)| !tried.contains(s))
        .ok_or_else(|| {
            refused("mutation space exhausted: every one-edit shortlist was already tried")
        })?;

    let candidate = PolicyEnvelope {
        schema: incumbent.schema,
        policy_id: req.new_policy_id.clone(),
        parent_policy_ref: incumbent_ref.clone(),
        scope: scope.clone(),
        mode: incumbent.mode,
        candidate_set_ref: incumbent.candidate_set_ref.clone(),
        controls_ref: incumbent.controls_ref.clone(),
        shortlist: shortlist.clone(),
        discovery_evidence_refs: evidence.clone(),
        authority_expansion: false,
    };
    candidate.validate()?;
    check_shortlist(
        &candidate,
        &eligible,
        &incumbent.candidate_set_ref,
        &incumbent.controls_ref,
        &scope,
    )?;
    let candidate_ref = store.put_cas("policies", &candidate)?;
    store.put_cas("policies", &incumbent)?;
    let hypothesis = Hypothesis::Proposed {
        schema: HypothesisSchema,
        candidate_policy_ref: candidate_ref.clone(),
        parent_policy_ref: incumbent_ref,
        intervention: shortlist,
        mutation,
        rationale: req.rationale.clone(),
        proposer_ref: req.proposer_ref.clone(),
        seed: req.seed,
        discovery_evidence_refs: evidence,
        excluded,
        proposed_ms: crate::now_ms(),
    };
    tx.append(Event::Hypothesis {
        scope: scope.clone(),
        hypothesis: Box::new(hypothesis.clone()),
    })?;
    Ok(Proposal {
        candidate,
        candidate_policy_ref: candidate_ref,
        hypothesis,
    })
}
