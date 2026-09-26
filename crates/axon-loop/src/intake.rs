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
//! 6. the policy acknowledgement (REQUIRED), located BY CONTENT (G6): among
//!    the acks presented (MiCode's `policy-ack/` directory, or one file),
//!    exactly one DISTINCT ack whose `pin.policy_ref` is the episode's
//!    `policy_ref` and whose `candidate_set_ref` is the episode's. None ⇒
//!    refused ("no ack"); two different ones ⇒ refused ("ambiguous ack").
//!    It must record `pinned` for exactly this policy id, ref and shortlist,
//!    and its candidate list must digest to the `candidate_set_ref` and
//!    contain the shortlist. It is required because `bind_episode` compares
//!    policy BYTES only against the policy the episode itself names: an
//!    episode re-pointed at a different stored policy with the same controls
//!    and candidate view would bind. The ack is MiCode's record of the
//!    shortlist that was actually applied. And `projection_ref` means a
//!    `PolicyProjection` (sidecar policy → ACF policy digest), never the ack:
//!    `null` is accepted (MiCode submits no ACF job); a non-null ref must be a
//!    presented `PolicyProjection` whose `cl22:` is that ref and whose
//!    `sidecar_policy_ref` is the episode's `policy_ref`, or the episode is
//!    refused;
//! 7. optionally the canonical MiCode episode (`--source-episode`): its
//!    `cl22:` is the sidecar's `source_episode_ref`, and its spend
//!    ([`source_episode_spend`], read through the pinned migration table)
//!    AGREES with `usage.cost_micro`: a known spend converts 1e-8 → 1e-6
//!    rounding UP ([`cost_micro_from_micro_cents`]) to exactly the sidecar's
//!    figure, and an unknown one is unknown on both sides. Neither side's
//!    known figure may stand against the other's unknown — that is a fact
//!    dropped or invented between the two documents. An unknown cost is never
//!    compared as, or recorded as, `0`.
//!
//! 8. the verification evidence (v0.22 G3, D12). Under D12 only MiCode's
//!    acceptance CHECK runs through Fabric; the agent's own execution stays
//!    under local authority, so `acf_request_ref` / `acf_receipt_ref` remain
//!    MiCode's not-produced markers and `bind_acf` does not apply. What CAN
//!    join is the check: when `verification.verifier_ref` is non-null, the
//!    Fabric `registered_check` request and receipt are REQUIRED
//!    (`--verification-request` / `--verification-receipt`) and must be exactly
//!    the documents the sidecar names (`verifier_ref` = cl22 of the receipt,
//!    `evidence_refs` = [cl22 of the request]); the check IS the attempt's
//!    Fabric operation (same task/trial/attempt/operation ids, and the
//!    receipt's execution id is the sidecar's); it ran on the episode's OUTPUT
//!    tree (request workspace = receipt input = `verification.output_workspace_ref`
//!    = `output_workspace_ref`); its evidence is supervisor-observed; and the
//!    sidecar's result and `matched_checks` are the receipt's, with a check that
//!    did not complete yielding `unknown`. For ANY cited result the issuer must
//!    be a trusted verifier that is not the subject, and the check must not
//!    have run as the subject's principal. And the issuer must be
//!    AUTHENTICATED, not merely named: an `acf-receipt-attestation/1`
//!    (`--verification-attestation`) must verify under the Ed25519 key the
//!    operator registered for that issuer in `verifier_keys`, over exactly this
//!    receipt's and request's cl22 (G01-r22-independent-issuer,
//!    G32-r22-sidecar-bindings). A copied issuer name, a sidecar's claim, or a
//!    validly self-signed attestation under any other key authenticates
//!    nothing; a trusted verifier with no registered key can vouch for nothing. Evidence presented for a sidecar that
//!    names none is refused rather than ignored. That Fabric actually journaled
//!    the operation is NOT checked here (this crate does not read the Fabric
//!    journal); `axon-fabric status --op` is the witness, and the paired interop
//!    gate asks it.
//!
//! Idempotent on the sidecar's bytes; the same trial identity with DIFFERENT
//! bytes is a conflict (exit 5). Nothing here authenticates MiCode: the
//! observer and issuer fields name a party, they do not prove one.

use crate::error::{refused, LoopError, Result};
use crate::ledger::{Event, Tx};
use crate::store::Store;
use axon_loop_contracts::{
    bind_episode, check_shortlist, digest, digest_value, parse, parse_value, AuthorityEpoch,
    CandidateId, ComputeRequest, CorpusRole, EpisodeStatus, EvidenceSource,
    ExecutionContextReceipt, ExecutionReceipt, JobKind, LoopEpisode, OpaqueRef, PolicyEnvelope,
    PolicyId, PolicyProjection, ReceiptStatus, ReceiptVerification, Ref, RefScheme, Refusal, Scope,
    TrialIdentity, UsageState, VerificationResult,
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
    /// `cl22:` of MiCode's policy acknowledgement, located by content.
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
    /// Step 8: `cl22:` of the Fabric check receipt / request the verification
    /// rests on (`fabric-receipts/`, `fabric-requests/`). ABSENT — not null —
    /// when the sidecar names no verifier, so records written before G3 keep
    /// their exact bytes and digests.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_receipt_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_request_ref: Option<Ref>,
    /// `cl22:` of the verifier's attestation (`fabric-attestations/`) and the
    /// id of the operator-registered key it verified under: WHICH signature
    /// authenticated this verdict, so it can be re-checked later. Absent, like
    /// the refs above, when the sidecar names no verifier.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_attestation_ref: Option<Ref>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verification_key_id: Option<String>,
}

/// What `intake_episode` returns.
#[derive(Clone, Debug, Serialize)]
pub struct IntakeOutcome {
    pub record: IntakeRecord,
    /// The ledger seq holding the record.
    pub ledger_seq: u64,
    /// `false` when these exact bytes were already recorded (idempotent replay).
    pub recorded_now: bool,
    /// The id (fingerprint) of the operator-registered key that authenticated
    /// the verification evidence, when the sidecar cites any.
    pub verification_key_id: Option<String>,
}

/// The documents an intake reads. Texts, so the caller owns all file I/O.
pub struct IntakeInput<'a> {
    pub episode: &'a str,
    pub context: &'a str,
    /// Candidate acknowledgements; the one that belongs to this episode is
    /// selected by content (see module docs, step 6).
    pub acks: &'a [String],
    /// The `PolicyProjection` bytes, required iff `projection_ref` is non-null.
    pub projection: Option<&'a str>,
    pub source_episode: Option<&'a str>,
    /// Step 8: the Fabric `registered_check` request and receipt, required iff
    /// the sidecar names a `verification.verifier_ref`.
    pub verification_request: Option<&'a str>,
    pub verification_receipt: Option<&'a str>,
    /// Step 8: the issuer's `acf-receipt-attestation/1` over that receipt and
    /// request, required with them.
    pub verification_attestation: Option<&'a str>,
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
    // The subject: the episode's observer AND the policy's proposer, if one is
    // on record — the same set EVL judges by, so neither door lets the agent
    // that proposed a policy verify it.
    let subject: BTreeSet<OpaqueRef> = [ctx.observed_issuer_ref.clone()]
        .into_iter()
        .chain(crate::evo::proposer_in(&tx, &ep.scope, &ep.policy_ref))
        .collect();
    bind_episode(
        &ep,
        &policy,
        &ctx,
        current_epoch,
        &config.verifiers(),
        &subject,
    )
    .map_err(semantic("bind"))?;

    // 6. The acknowledgement, located by content; 6b the projection.
    let ack_ref = check_ack(select_ack(input.acks, &ep)?, &ep, &policy)?;
    check_projection(input.projection, &ep)?;

    // 7. The canonical episode and the unit conversion, when presented.
    if let Some(text) = input.source_episode {
        check_source_episode(text, &ep)?;
    }

    // 8. The verification evidence (G3 under D12).
    let verification = check_verification(input, &ep, &config, &subject)?;

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
                    verification_key_id: verification.as_ref().map(|v| v.3.clone()),
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
    let mut attestation_ref = None;
    if let Some((req, rc, att, _)) = &verification {
        store.put_cas("fabric-requests", req)?;
        store.put_cas("fabric-receipts", rc)?;
        attestation_ref = Some(store.put_cas("fabric-attestations", att)?);
    }
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
        verification_receipt_ref: verification
            .as_ref()
            .map(|(_, rc, _, _)| digest(rc))
            .transpose()?,
        verification_request_ref: verification
            .as_ref()
            .map(|(req, _, _, _)| digest(req))
            .transpose()?,
        verification_attestation_ref: attestation_ref,
        verification_key_id: verification.as_ref().map(|v| v.3.clone()),
    };
    let seq = tx.append(Event::EpisodeIntake {
        scope: ep.scope.clone(),
        intake: Box::new(record.clone()),
    })?;
    Ok(IntakeOutcome {
        record,
        ledger_seq: seq,
        recorded_now: true,
        verification_key_id: verification.map(|v| v.3),
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

/// Step 6: exactly one DISTINCT ack joins this episode by content. Files that
/// are not acks, or acks for another policy/view, are skipped; an ack that
/// matches but is malformed is still returned so `check_ack` refuses it.
fn select_ack<'a>(acks: &'a [String], ep: &LoopEpisode) -> Result<&'a str> {
    let mut found: Vec<(Ref, &str)> = Vec::new();
    for text in acks {
        let Ok(v) = parse_value(text) else { continue };
        if v.get("schema").and_then(Value::as_str) != Some(ACK_SCHEMA) {
            continue;
        }
        let pin_ref = v.pointer("/pin/policy_ref").and_then(Value::as_str);
        let csr = v.get("candidate_set_ref").and_then(Value::as_str);
        if pin_ref != Some(ep.policy_ref.as_str()) || csr != Some(ep.candidate_set_ref.as_str()) {
            continue;
        }
        let r = digest_value(&v)?;
        if !found.iter().any(|(x, _)| *x == r) {
            found.push((r, text.as_str()));
        }
    }
    match found.as_slice() {
        [] => Err(refused(format!(
            "no ack: none of the {} presented acknowledgement(s) pins policy {} over view {}",
            acks.len(),
            ep.policy_ref,
            ep.candidate_set_ref
        ))),
        [(_, t)] => Ok(t),
        many => Err(refused(format!(
            "ambiguous ack: {} different acknowledgements pin policy {} over view {}",
            many.len(),
            ep.policy_ref,
            ep.candidate_set_ref
        ))),
    }
}

/// Step 6b: `projection_ref` is a PolicyProjection, not the ack.
fn check_projection(text: Option<&str>, ep: &LoopEpisode) -> Result<()> {
    let Some(want) = &ep.projection_ref else {
        return Ok(());
    };
    let text = text.ok_or_else(|| {
        refused(format!(
            "projection_ref {want} names a PolicyProjection, but none was presented (--projection)"
        ))
    })?;
    let proj: PolicyProjection = parse(text).map_err(semantic("projection"))?;
    let r = digest(&proj)?;
    if &r != want {
        return Err(refused(format!(
            "projection digests to {r}, but the episode's projection_ref is {want}"
        )));
    }
    if proj.sidecar_policy_ref != ep.policy_ref {
        return Err(refused(
            "projection maps a different sidecar policy than the episode names",
        ));
    }
    Ok(())
}

/// Step 8 (see module docs). `Some((request, receipt))` when the sidecar
/// names a verifier and every join holds; `None` when it names none.
fn check_verification(
    input: &IntakeInput<'_>,
    ep: &LoopEpisode,
    config: &crate::store::Config,
    subject: &BTreeSet<OpaqueRef>,
) -> Result<Option<(ComputeRequest, ExecutionReceipt, Value, String)>> {
    let v = &ep.verification;
    let Some(vref) = &v.verifier_ref else {
        if input.verification_request.is_some()
            || input.verification_receipt.is_some()
            || input.verification_attestation.is_some()
        {
            return Err(refused(
                "verification evidence was presented, but the sidecar names no verifier_ref: \
                 evidence the episode does not cite is not attached to it",
            ));
        }
        return Ok(None);
    };
    let (Some(req_text), Some(rc_text)) = (input.verification_request, input.verification_receipt)
    else {
        return Err(refused(format!(
            "verifier_ref {vref} names a Fabric check receipt, but the check's request and receipt \
             were not both presented (--verification-request, --verification-receipt)"
        )));
    };
    verify_check_evidence(
        ep,
        req_text,
        rc_text,
        input.verification_attestation,
        config,
        subject,
    )
    .map(Some)
}

/// The verification join and its authentication, shared by intake (step 8)
/// and EVL judging: `ep.verification` cites a Fabric registered check, and
/// `(req_text, rc_text, att_text)` must be exactly that check's request,
/// receipt and the verifier's attestation. Returns the typed documents, the
/// attestation and the authenticating key id. Everything a verdict rests on
/// is checked here, in one place, for every consumer:
///
/// * the documents are the ones cited (digests), and never a context or
///   execution document standing in for a verification;
/// * the issuer is a trusted verifier independent of the subject, AUTHENTICATED
///   by its attestation under the operator-registered key;
/// * the check is what the operator PINNED for that verifier: its revision
///   (registered executable + digest), a pinned compute profile, and an
///   operator-registered suite (`check:<id>`, recorded by Fabric as
///   `check-suite:<id>@<version>`) at a pinned version — never a file of the
///   subject's own tree, whose bytes the subject controls;
/// * it is this attempt's operation, on the episode's output tree, observed by
///   the supervisor, run as a principal other than the subject, and the
///   sidecar's result and matched count are the receipt's.
pub fn verify_check_evidence(
    ep: &LoopEpisode,
    req_text: &str,
    rc_text: &str,
    att_text: Option<&str>,
    config: &crate::store::Config,
    subject: &BTreeSet<OpaqueRef>,
) -> Result<(ComputeRequest, ExecutionReceipt, Value, String)> {
    let v = &ep.verification;
    let vref = v
        .verifier_ref
        .as_ref()
        .ok_or_else(|| refused("the episode cites no verifier_ref"))?;
    let trusted_verifiers = config.verifiers();
    let verifier_keys = &config.verifier_keys;
    let req: ComputeRequest = parse(req_text).map_err(semantic("verification request"))?;
    let rc: ExecutionReceipt = parse(rc_text).map_err(semantic("verification receipt"))?;
    let (req_ref, rc_ref) = (digest(&req)?, digest(&rc)?);
    if &rc_ref != vref {
        return Err(refused(format!(
            "verification receipt digests to {rc_ref}, but the sidecar's verifier_ref is {vref}"
        )));
    }
    if v.evidence_refs != [req_ref.clone()] {
        return Err(refused(
            "verification evidence_refs must be exactly [cl22 of the check request]",
        ));
    }
    // The ONLY intake guard against an episode citing its own execution
    // documents as the verification (re-audit 4 corrected re-audit 3's "X18
    // is equivalent": bind_episode has no role rule; bind_acf does, but intake
    // never calls it). EVL's bind_acf would still refuse to COUNT such an
    // episode; this keeps it from being recorded at all.
    for r in [&req_ref, &rc_ref] {
        if [&ep.context_ref, &ep.acf_request_ref, &ep.acf_receipt_ref].contains(&r) {
            return Err(refused(
                "role upgrade: a context or execution document stands as the verification",
            ));
        }
    }
    // Independence, for EVERY cited result (bind_episode enforces it only for
    // `passed`): the issuer is one the operator trusts and not the subject,
    // and the check did not run as the subject either.
    if !v
        .issuer_ref
        .as_ref()
        .is_some_and(|i| trusted_verifiers.contains(i) && !subject.contains(i))
    {
        return Err(refused(
            "the verification issuer is not a trusted verifier independent of the subject",
        ));
    }
    // Authentication, not naming: the issuer's own signature over exactly
    // these two documents, under the key the operator registered for it.
    let issuer = v.issuer_ref.as_ref().expect("checked just above");
    let key = verifier_keys.get(issuer).ok_or_else(|| {
        refused(format!(
            "verifier {issuer} is trusted but has no registered key in verifier_keys: its \
             evidence cannot be authenticated, so it vouches for nothing"
        ))
    })?;
    let att_text = att_text.ok_or_else(|| {
        refused(format!(
            "the verification is not authenticated: no acf-receipt-attestation from {issuer} \
             was presented (--verification-attestation)"
        ))
    })?;
    let att = parse_value(att_text).map_err(semantic("verification attestation"))?;
    let key_id = axon_loop_contracts::attestation::verify(&att, issuer, &req, &rc, key)
        .map_err(|e| refused(format!("verification attestation refused: {e}")))?;
    // What the verifier ran must be what the operator pinned for it.
    let pin = config.verifier_pins.get(issuer).ok_or_else(|| {
        refused(format!(
            "verifier {issuer} has no operator pin (revision, profile, suite): its verdict cannot \
             be tied to what the operator trusts it to run"
        ))
    })?;
    if req.registered_executable_ref.as_str() != pin.registered_executable_ref
        || req.executable_digest.as_str() != pin.executable_digest
    {
        return Err(refused(format!(
            "verifier revision: the check ran {} ({}), not the pinned {} ({})",
            req.registered_executable_ref,
            req.executable_digest,
            pin.registered_executable_ref,
            pin.executable_digest
        )));
    }
    if !pin
        .backend_profiles
        .iter()
        .any(|p| p == rc.backend_profile_ref.as_str())
    {
        return Err(refused(format!(
            "compute profile: the verdict came from {}, not a profile pinned for verifier {issuer}",
            rc.backend_profile_ref
        )));
    }
    let entry = req.argv.first().map(String::as_str).unwrap_or("");
    let Some(suite_id) = entry.strip_prefix("check:") else {
        return Err(refused(format!(
            "rubric: the check ran {entry:?}, a file of the candidate's own tree — candidate \
             bytes cannot define the acceptance rubric; a verification must run an \
             operator-registered suite (check:<id>)"
        )));
    };
    let suites: Vec<&str> = rc
        .evidence_refs
        .iter()
        .map(OpaqueRef::as_str)
        .filter(|e| e.starts_with("check-suite:"))
        .collect();
    let recorded = match suites.as_slice() {
        [one] => *one,
        _ => {
            return Err(refused(
                "rubric: the receipt does not record exactly one check suite version",
            ))
        }
    };
    if !recorded.starts_with(&format!("check-suite:{suite_id}@"))
        || !pin.check_suites.iter().any(|p| p == recorded)
    {
        return Err(refused(format!(
            "rubric: suite {recorded} is not a version pinned for verifier {issuer}"
        )));
    }
    // ...and it must be THIS task's acceptance check, as the operator
    // registered it: that suite at that version, that exact test.
    let acc = config
        .task_acceptance
        .get(&ep.identity.task_id)
        .ok_or_else(|| {
            refused(format!(
                "acceptance: task {} has no operator-registered acceptance check, so no verdict \
                 can decide it",
                ep.identity.task_id
            ))
        })?;
    let acc_suite = acc
        .check_suite
        .strip_prefix("check-suite:")
        .and_then(|x| x.split('@').next())
        .unwrap_or("");
    if req.argv != [format!("check:{acc_suite}"), acc.check.clone()] || recorded != acc.check_suite
    {
        return Err(refused(format!(
            "acceptance: the check ran {:?} ({recorded}), not task {}'s registered acceptance \
             check {} in {}",
            req.argv, ep.identity.task_id, acc.check, acc.check_suite
        )));
    }
    if subject.contains(&req.principal_ref) {
        return Err(refused(format!(
            "the check ran as principal {}, the subject itself: a task cannot verify itself",
            req.principal_ref
        )));
    }
    if req.job_kind != JobKind::RegisteredCheck {
        return Err(refused(
            "the verification request is not a registered_check",
        ));
    }
    let id = &ep.identity;
    let same = req.task_id == id.task_id
        && rc.task_id == id.task_id
        && req.trial_id == id.trial_id
        && rc.trial_id == id.trial_id
        && req.attempt_id == id.attempt_id
        && rc.attempt_id == id.attempt_id
        && req.operation_id == id.operation_id
        && rc.operation_id == id.operation_id
        && rc.execution_id == id.execution_id;
    if !same {
        return Err(refused(
            "the check is not this attempt's Fabric operation (task/trial/attempt/operation/\
             execution ids differ)",
        ));
    }
    let checked = Some(&req.workspace_version_ref);
    if Some(&rc.input_workspace_ref) != checked
        || v.output_workspace_ref.as_ref() != checked
        || ep.output_workspace_ref.as_ref() != checked
    {
        return Err(refused(
            "the check did not run on the episode's output tree (request workspace, receipt \
             input, verification.output_workspace_ref and output_workspace_ref must agree)",
        ));
    }
    if rc.evidence_source != EvidenceSource::SupervisorObserved {
        return Err(refused(
            "the check receipt is not supervisor-observed: a reported result verifies nothing",
        ));
    }
    let from_receipt = match (rc.status, rc.verification) {
        (ReceiptStatus::Completed, ReceiptVerification::Passed) => VerificationResult::Passed,
        (ReceiptStatus::Completed, ReceiptVerification::Failed) => VerificationResult::Failed,
        (
            ReceiptStatus::Completed
            | ReceiptStatus::Failed
            | ReceiptStatus::Canceled
            | ReceiptStatus::Denied
            | ReceiptStatus::Unsupported
            | ReceiptStatus::OutcomeUnknown
            | ReceiptStatus::TimedOut,
            ReceiptVerification::NotRequested
            | ReceiptVerification::NotRun
            | ReceiptVerification::Passed
            | ReceiptVerification::Failed
            | ReceiptVerification::Unknown,
        ) => VerificationResult::Unknown,
    };
    if v.result != from_receipt {
        return Err(refused(format!(
            "the sidecar says verification {:?}, the check receipt says {from_receipt:?}",
            v.result
        )));
    }
    if v.matched_checks != rc.matched_checks.unwrap_or(0) {
        return Err(refused("matched_checks differs from the check receipt's"));
    }
    Ok((req, rc, att, key_id))
}

/// The canonical MiCode episode's spend in micro-cents (1e-8 USD), read through
/// the SAME pinned migration table MiCode's `ResourceCostWire` applies
/// (G16-r22-negotiation: old episodes stay readable, nothing is reinterpreted):
///
/// * `cost.spend_micro_cents` present (MiCode since the adapter): `null` is
///   unknown, a number is the spend — including a genuine `0`;
/// * absent, the legacy `cost.micro_cents`: `null` is unknown, a non-zero
///   number is the spend (only the v0.22 producer wrote one), and `0` is
///   UNKNOWN — MiCode's v014 base never wrote the field, so every episode it
///   produced carries the default `0` whatever its task cost. Reading that as
///   a known zero would make every historical episode free.
pub fn source_episode_spend(v: &Value) -> Result<Option<u64>> {
    let number = |ptr: &str| -> Result<Option<Option<u64>>> {
        match v.pointer(ptr) {
            None => Ok(None),
            Some(Value::Null) => Ok(Some(None)),
            Some(x) => x.as_u64().map(|n| Some(Some(n))).ok_or_else(|| {
                shape(format!(
                    "source episode: {ptr} is not an unsigned integer or null"
                ))
            }),
        }
    };
    if let Some(spend) = number("/cost/spend_micro_cents")? {
        return Ok(spend);
    }
    match number("/cost/micro_cents")? {
        Some(legacy) => Ok(legacy.filter(|&c| c != 0)),
        None => Err(shape(
            "source episode: cost states neither spend_micro_cents nor micro_cents",
        )),
    }
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
    let spend = source_episode_spend(&v)?;
    match (spend, ep.usage.cost_micro) {
        (None, None) => {}
        (Some(mc), None) => {
            return Err(refused(format!(
                "unit conversion: canonical episode {} records a spend of {mc} micro-cents but \
                 the sidecar's cost is unknown — a known figure was dropped between the two \
                 documents; source-episode join refused, nothing recorded",
                ep.source_episode_ref
            )));
        }
        (None, Some(c)) => {
            return Err(refused(format!(
                "unit conversion: canonical episode {} records NO known spend (null, or the \
                 default 0 of an episode written before the spend producer) but the sidecar \
                 says {c} cost_micro — the MiCode producer did not state the spend on the \
                 canonical episode (interop gap G1). Axon does not substitute either figure; \
                 fix the producer. Source-episode join refused, nothing recorded",
                ep.source_episode_ref
            )));
        }
        (Some(mc), Some(c)) => {
            let want = cost_micro_from_micro_cents(Some(mc)).expect("some");
            if c != want {
                // G1: never repaired here. The join is refused; the message names
                // which side's figure is the likely producer gap so the operator
                // does not chase a wire bug.
                let hint = if mc == 0 && c > 0 {
                    " — the canonical episode states ZERO spend while the sidecar \
                 records a metered cost (interop gap G1). Axon does not \
                 substitute either figure; fix the producer"
                } else {
                    " — the two figures disagree under the 1e-8 → 1e-6 round-up rule"
                };
                return Err(refused(format!(
                    "unit conversion: canonical episode {} records {mc} micro-cents (1e-8) = \
                 {want} cost_micro (1e-6, rounded up), but the sidecar says {c}{hint}; \
                 source-episode join refused, nothing recorded",
                    ep.source_episode_ref
                )));
            }
        }
    }
    Ok(())
}

fn shape(s: impl Into<String>) -> LoopError {
    LoopError::Malformed(Refusal::Shape(s.into()))
}
