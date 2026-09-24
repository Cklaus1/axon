//! B269 (Axon side) — intake of a MiCode `axon.closed-loop.episode/1` sidecar.
//!
//! MiCode writes, per task, under `<repo>/.micode/axon/closed-loop/`:
//! `episodes/<hex>.json` (the sidecar), `context/<hex>.json` (the
//! `axon.closed-loop.context/1` preflight receipt the sidecar's `context_ref`
//! names) and `policy-ack/<hex>.json` (`micode.closed-loop.policy-ack/1`, the
//! sidecar's `projection_ref`). [`intake_episode`] joins those bytes to this
//! store and records ONE ledger event, or refuses and writes nothing.
//!
//! What is checked, in order (every failure is a refusal, never a repair):
//!
//! 1. the sidecar and the context receipt parse under the contracts crate's
//!    strict [`axon_loop_contracts::parse`] (schema, closed objects, no
//!    duplicate keys, no floats; exit 3);
//! 2. the receipt is the one the sidecar names (`cl22:` of its bytes);
//! 3. the context BOUND (expected == observed) and the status is not
//!    `refused`: MiCode writes a `refused` sidecar for a TASK_NOT_STARTED
//!    task, and the contracts say a pre-preflight failure is NOT a
//!    `LoopEpisode` — it is refused here rather than counted as an outcome;
//! 4. the `policy_ref` is a policy THIS store holds (re-digested on read),
//!    not MiCode's explicit "not produced" marker (an incumbent-fallback or
//!    unconfigured task ran under no Axon policy), and not revoked;
//! 5. [`axon_loop_contracts::bind_episode`] at the scope's CURRENT authority
//!    epoch: identity, scope, policy/context bytes, controls and candidate
//!    view, input workspace;
//! 6. the policy acknowledgement (REQUIRED): its `cl22:` is the sidecar's
//!    `projection_ref`, it records `pinned` for exactly this policy id, ref and
//!    shortlist, and its candidate list digests to the policy's
//!    `candidate_set_ref` and contains the shortlist. It is required because
//!    `bind_episode` compares policy BYTES only against the policy the episode
//!    itself names: an episode re-pointed at a different stored policy with the
//!    same controls and candidate view would bind. The ack is MiCode's record
//!    of the shortlist that was actually applied;
//! 7. optionally the canonical MiCode episode (`--source-episode`): its
//!    `cl22:` is the sidecar's `source_episode_ref`, and a KNOWN
//!    `usage.cost_micro` equals its `cost.micro_cents` converted 1e-8 → 1e-6
//!    rounding UP ([`cost_micro_from_micro_cents`]). An unknown (`null`) cost
//!    stays `null` — it is never compared as, or recorded as, `0`.
//!
//! Idempotent on the sidecar's bytes; the same trial identity with DIFFERENT
//! bytes is a conflict (exit 5). Nothing here authenticates MiCode: the
//! observer and issuer fields name a party, they do not prove one.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::Store;
use axon_loop_contracts::{
    bind_episode, check_shortlist, digest, digest_value, parse, parse_value, AuthorityEpoch,
    CandidateId, CorpusRole, EpisodeStatus, ExecutionContextReceipt, LoopEpisode, OpaqueRef,
    PolicyEnvelope, PolicyId, Ref, RefScheme, Refusal, Scope, TrialIdentity, UsageState,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;

crate::record_tag!(IntakeSchema, "axon.loop.episode-intake/1");

/// The schema tag MiCode's policy acknowledgement carries.
pub const ACK_SCHEMA: &str = "micode.closed-loop.policy-ack/1";

/// MiCode `MicroCents` (1e-8 USD) → wire `cost_micro` (1e-6 USD), rounding UP
/// so a spend is never under-reported. `None` (unknown) stays `None`.
pub fn cost_micro_from_micro_cents(micro_cents: Option<u64>) -> Option<u64> {
    micro_cents.map(|mc| mc / 100 + u64::from(mc % 100 != 0))
}

/// MiCode's `cl22:` over a named absence (`loop_sidecar::not_produced_ref`).
pub fn micode_not_produced_ref(field: &str) -> Ref {
    digest_value(&json!({"not_produced_by": "micode", "field": field}))
        .expect("two strings always canonicalise")
}

/// The ledger record of one intaken episode.
#[derive(Clone, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IntakeRecord {
    pub schema: IntakeSchema,
    pub scope: Scope,
    pub identity: TrialIdentity,
    /// `cl22:` of the sidecar; `episodes/<hex>.json` holds its bytes.
    pub episode_ref: Ref,
    /// `cl22:` of the context receipt; `contexts/<hex>.json` holds its bytes.
    pub context_ref: Ref,
    pub policy_ref: Ref,
    pub policy_id: PolicyId,
    pub authority_epoch: AuthorityEpoch,
    pub corpus_role: CorpusRole,
    pub status: EpisodeStatus,
    pub usage_state: UsageState,
    /// µ-USD; `null` = unknown, never 0.
    #[serde(deserialize_with = "crate::nullable")]
    pub cost_micro: Option<u64>,
    pub source_episode_ref: Ref,
    /// `cl22:` of MiCode's policy acknowledgement (= the sidecar's
    /// `projection_ref`).
    pub ack_ref: Ref,
    /// Whether the canonical episode was presented and the unit conversion
    /// cross-checked.
    pub source_episode_checked: bool,
    /// The observer the receipt NAMES (not authenticated).
    pub observed_issuer_ref: OpaqueRef,
    /// Why this context would NOT qualify under the bounded paired-trial
    /// profile (`check_paired_trial_context`, evaluated at the receipt's own
    /// `created_ms` and epoch, with the named observer as the only premise —
    /// so this is the STRUCTURAL part only: primary checkout, pattern paths,
    /// role write sets). `null` = structurally qualifies. Recorded, not
    /// enforced: admission decides what a non-qualifying context may feed.
    #[serde(deserialize_with = "crate::nullable")]
    pub trial_profile_refusal: Option<String>,
}

/// What `intake_episode` returns.
#[derive(Clone, Debug, Serialize)]
pub struct IntakeOutcome {
    pub record: IntakeRecord,
    /// The ledger seq holding the record.
    pub ledger_seq: u64,
    /// `false` when these exact bytes were already recorded (idempotent replay).
    pub recorded_now: bool,
}

/// The documents an intake reads. Texts, so the caller owns all file I/O.
pub struct IntakeInput<'a> {
    pub episode: &'a str,
    pub context: &'a str,
    pub ack: &'a str,
    pub source_episode: Option<&'a str>,
}

fn semantic(what: &str) -> impl Fn(Refusal) -> LoopError + '_ {
    move |r| match r {
        Refusal::Semantic(s) => refused(format!("{what}: {s}")),
        other => LoopError::Malformed(other),
    }
}

pub fn intake_episode(store: &Store, input: &IntakeInput<'_>) -> Result<IntakeOutcome> {
    // 1. Strict parse — refusal before any lock or store access.
    let ep: LoopEpisode = parse(input.episode).map_err(semantic("episode"))?;
    let ctx: ExecutionContextReceipt = parse(input.context).map_err(semantic("context"))?;
    let episode_ref = digest(&ep)?;

    // 2. The receipt is the one the sidecar names.
    let ctx_ref = digest(&ctx)?;
    if ctx_ref != ep.context_ref {
        return Err(refused(format!(
            "context receipt digests to {ctx_ref}, but the episode names {}",
            ep.context_ref
        )));
    }
    // 3. Only a bound, started task is an episode.
    if ctx.expected != ctx.observed {
        return Err(refused(
            "TASK_NOT_STARTED: the context receipt did not bind (expected != observed); \
             a pre-preflight refusal is not an episode",
        ));
    }
    if ep.status == EpisodeStatus::Refused {
        return Err(refused(
            "status refused: a TASK_NOT_STARTED sidecar is not a post-preflight episode",
        ));
    }
    // 4. The policy must be one this store issued.
    if ep.policy_ref.scheme() != RefScheme::Cl22 {
        return Err(refused(format!(
            "policy_ref {} is not a cl22: policy reference",
            ep.policy_ref
        )));
    }
    for field in ["policy_ref", "controls_ref", "candidate_set_ref"] {
        let marker = micode_not_produced_ref(field);
        let named = match field {
            "policy_ref" => &ep.policy_ref,
            "controls_ref" => &ep.controls_ref,
            _ => &ep.candidate_set_ref,
        };
        if *named == marker {
            return Err(refused(format!(
                "{field} is MiCode's not-produced marker: the task ran under no Axon-issued \
                 policy (not configured, or it abstained to the incumbent); nothing to record"
            )));
        }
    }

    let mut tx = Tx::begin(store)?;

    let policy: PolicyEnvelope = match store.get_contract("policies", &ep.policy_ref) {
        Ok(p) => p,
        Err(LoopError::Refused(_)) => {
            return Err(refused(format!(
                "policy {} is not a policy this store knows",
                ep.policy_ref
            )))
        }
        Err(e) => return Err(e),
    };
    if tx.is_revoked(&ep.scope, &ep.policy_ref) {
        return Err(refused(format!("policy {} is revoked", ep.policy_ref)));
    }

    // 5. The cross-document join at the scope's current epoch.
    let current_epoch = tx.pointer(&ep.scope).epoch;
    let config = store.config()?;
    let subject: BTreeSet<OpaqueRef> = [ctx.observed_issuer_ref.clone()].into_iter().collect();
    bind_episode(
        &ep,
        &policy,
        &ctx,
        current_epoch,
        &config.verifiers(),
        &subject,
    )
    .map_err(semantic("bind"))?;

    // 6. The acknowledgement, when presented.
    let ack_ref = check_ack(input.ack, &ep, &policy)?;

    // 7. The canonical episode and the unit conversion, when presented.
    if let Some(text) = input.source_episode {
        check_source_episode(text, &ep)?;
    }

    // Idempotency / identity conflict, against the ledger — AFTER every
    // check, so a replay is never a way around one (a re-intake with a
    // `--source-episode` that does not join is refused, not replayed).
    for e in tx.entries() {
        if let Event::EpisodeIntake { intake, .. } = &e.event {
            if intake.episode_ref == episode_ref {
                return Ok(IntakeOutcome {
                    record: (**intake).clone(),
                    ledger_seq: e.seq,
                    recorded_now: false,
                });
            }
            if intake.scope == ep.scope && intake.identity == ep.identity {
                return Err(LoopError::Conflict(format!(
                    "trial identity already recorded as {} with different bytes",
                    intake.episode_ref
                )));
            }
        }
    }

    // Record: bytes first (content-addressed, idempotent), then the ledger.
    store.put_cas("episodes", &ep)?;
    store.put_cas("contexts", &ctx)?;
    let record = IntakeRecord {
        schema: IntakeSchema,
        scope: ep.scope.clone(),
        identity: ep.identity.clone(),
        episode_ref,
        context_ref: ctx_ref,
        policy_ref: ep.policy_ref.clone(),
        policy_id: policy.policy_id.clone(),
        authority_epoch: ep.authority_epoch,
        corpus_role: ep.corpus_role,
        status: ep.status,
        usage_state: ep.usage.state,
        cost_micro: ep.usage.cost_micro,
        source_episode_ref: ep.source_episode_ref.clone(),
        ack_ref,
        source_episode_checked: input.source_episode.is_some(),
        observed_issuer_ref: ctx.observed_issuer_ref.clone(),
        trial_profile_refusal: axon_loop_contracts::check_paired_trial_context(
            &ctx,
            ctx.created_ms,
            ctx.authority_epoch,
            &subject,
        )
        .err()
        .map(|r| r.to_string()),
    };
    let seq = tx.append(Event::EpisodeIntake {
        scope: ep.scope.clone(),
        intake: Box::new(record.clone()),
    })?;
    Ok(IntakeOutcome {
        record,
        ledger_seq: seq,
        recorded_now: true,
    })
}

fn check_ack(text: &str, ep: &LoopEpisode, policy: &PolicyEnvelope) -> Result<Ref> {
    let v = parse_value(text).map_err(semantic("ack"))?;
    let obj = v.as_object().ok_or_else(|| shape("ack: not an object"))?;
    let keys: BTreeSet<&str> = obj.keys().map(String::as_str).collect();
    let want: BTreeSet<&str> = ["schema", "pin", "candidates", "candidate_set_ref"]
        .into_iter()
        .collect();
    if keys != want {
        return Err(shape(format!(
            "ack: fields {keys:?}, expected exactly {want:?}"
        )));
    }
    if obj["schema"] != ACK_SCHEMA {
        return Err(shape(format!("ack: schema must be {ACK_SCHEMA:?}")));
    }
    let ack_ref = digest_value(&v)?;
    if ep.projection_ref.as_ref() != Some(&ack_ref) {
        return Err(refused(format!(
            "ack digests to {ack_ref}, but the episode's projection_ref is {:?}",
            ep.projection_ref.as_ref().map(Ref::as_str)
        )));
    }
    let pin = &obj["pin"];
    if pin["state"] != "pinned" {
        return Err(refused(format!(
            "ack records pin state {}, not pinned",
            pin["state"]
        )));
    }
    if pin["policy_ref"] != ep.policy_ref.as_str() || pin["policy_id"] != policy.policy_id.as_str()
    {
        return Err(refused(
            "ack pins a different policy than the episode names",
        ));
    }
    let shortlist: Vec<&str> = policy.shortlist.iter().map(|c| c.as_str()).collect();
    if pin["shortlist"] != json!(shortlist) {
        return Err(refused(
            "ack's shortlist is not the stored policy's shortlist",
        ));
    }
    if obj["candidate_set_ref"] != ep.candidate_set_ref.as_str() {
        return Err(refused(
            "ack's candidate_set_ref differs from the episode's",
        ));
    }
    let cands = &obj["candidates"];
    if digest_value(cands)? != ep.candidate_set_ref {
        return Err(refused(
            "ack's candidate list does not digest to its candidate_set_ref",
        ));
    }
    let eligible: BTreeSet<CandidateId> = cands
        .as_array()
        .ok_or_else(|| shape("ack: candidates is not an array"))?
        .iter()
        .map(|c| {
            c.as_str()
                .and_then(|s| CandidateId::new(s).ok())
                .ok_or_else(|| shape(format!("ack: candidate {c} is not a candidate id")))
        })
        .collect::<Result<_>>()?;
    check_shortlist(
        policy,
        &eligible,
        &ep.candidate_set_ref,
        &ep.controls_ref,
        &ep.scope,
    )
    .map_err(semantic("ack shortlist"))?;
    Ok(ack_ref)
}

fn check_source_episode(text: &str, ep: &LoopEpisode) -> Result<()> {
    let v: Value = parse_value(text).map_err(semantic("source episode"))?;
    let r = digest_value(&v)?;
    if r != ep.source_episode_ref {
        return Err(refused(format!(
            "source episode digests to {r}, but the sidecar names {}",
            ep.source_episode_ref
        )));
    }
    let mc = v
        .pointer("/cost/micro_cents")
        .and_then(Value::as_u64)
        .ok_or_else(|| shape("source episode: cost.micro_cents is not an unsigned integer"))?;
    if let Some(c) = ep.usage.cost_micro {
        let want = cost_micro_from_micro_cents(Some(mc)).expect("some");
        if c != want {
            return Err(refused(format!(
                "unit conversion: canonical episode spent {mc} micro-cents (1e-8) = \
                 {want} cost_micro (1e-6, rounded up), sidecar says {c}"
            )));
        }
    }
    Ok(())
}

fn shape(s: impl Into<String>) -> LoopError {
    LoopError::Malformed(Refusal::Shape(s.into()))
}
