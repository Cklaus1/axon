//! Semantic checks that need no I/O.
//!
//! Mirrors the package's `tools/closed_loop_reference.py` (`shortlist`,
//! `preflight`, `bind_episode`, `bind_acf`, `concrete_path`,
//! `learning_eligible`). Intrinsic single-document rules (authority_expansion,
//! epoch contiguity, rollback/activate refs, passed ⇒ matched_checks > 0,
//! final ⇒ known cost) run inside [`crate::parse`] via `Contract::validate`;
//! the functions here are the cross-document rules.
//!
//! Caller-supplied "trusted" sets are TEST PREMISES, not authentication:
//! production must authenticate the observer/verifier/admitter and resolve
//! their evidence before calling these. Passing never qualifies a runtime.

use crate::compute::ComputeRequest;
use crate::context::{ExecutionContextReceipt, Role};
use crate::episode::{EpisodeStatus, LoopEpisode, UsageState, VerificationResult};
use crate::error::{semantic, Refusal};
use crate::ids::{AuthorityEpoch, CandidateId, OpaqueRef, Ref, Scope};
use crate::policy::{PolicyEnvelope, PolicyProjection};
use crate::receipt::{ExecutionReceipt, ReceiptStatus};
use crate::{digest, Contract};
use std::collections::BTreeSet;

/// Apply a shortlist policy to an already-authorized candidate view.
///
/// Refuses a wrong scope or candidate view, control drift, and any shortlist
/// entry outside `eligible` (a shortlist can only SUBTRACT). The schema already
/// forbids an empty shortlist, so the result is non-empty; a policy with
/// nothing to offer abstains by being paused, never by an empty list a
/// consumer could misread as "everything". Every actual effect must still
/// recheck authority afterwards.
pub fn check_shortlist<'p>(
    policy: &'p PolicyEnvelope,
    eligible: &BTreeSet<CandidateId>,
    candidate_set_ref: &Ref,
    controls_ref: &Ref,
    scope: &Scope,
) -> Result<&'p [CandidateId], Refusal> {
    policy.validate()?;
    if &policy.scope != scope || &policy.candidate_set_ref != candidate_set_ref {
        return Err(semantic("wrong scope or eligible-candidate view"));
    }
    if &policy.controls_ref != controls_ref {
        return Err(semantic("pilot control drift"));
    }
    if let Some(c) = policy.shortlist.iter().find(|c| !eligible.contains(*c)) {
        return Err(semantic(format!(
            "shortlist expands eligible candidates: {c}"
        )));
    }
    if policy.shortlist.is_empty() {
        return Err(semantic("empty shortlist: abstain by pausing instead"));
    }
    Ok(&policy.shortlist)
}

/// A path that is a concrete, canonical, relative permission — not a pattern.
pub fn concrete_path(path: &str) -> Result<(), Refusal> {
    if path.is_empty() || path.contains('\\') || path.contains('\0') {
        return Err(semantic(format!("invalid concrete path {path:?}")));
    }
    if path.contains(['*', '?', '[', ']', ':']) {
        return Err(semantic(format!(
            "unproved pattern/drive path is not a concrete permission: {path:?}"
        )));
    }
    if path.starts_with('/')
        || path
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(semantic(format!(
            "unsafe or noncanonical relative path {path:?}"
        )));
    }
    Ok(())
}

/// Context currency and role rules, without the paired-trial equality: the
/// window `created_ms <= now_ms < expires_ms`, the current epoch, an observer
/// distinct from the expecting parent and among the recognized premises, a
/// non-primary worktree, concrete paths, verifier/critic read-only, and an
/// explicit non-empty write set for implementation/documentation.
pub fn check_context_current(
    ctx: &ExecutionContextReceipt,
    now_ms: u64,
    current_epoch: AuthorityEpoch,
    trusted_observers: &BTreeSet<OpaqueRef>,
) -> Result<(), Refusal> {
    ctx.validate()?;
    if !(ctx.created_ms <= now_ms && now_ms < ctx.expires_ms) {
        return Err(semantic("context future/expired"));
    }
    if ctx.authority_epoch != current_epoch {
        return Err(semantic("stale authority epoch"));
    }
    if ctx.observed_issuer_ref == ctx.expected_issuer_ref {
        return Err(semantic("parent echo is not independent observation"));
    }
    if !trusted_observers.contains(&ctx.observed_issuer_ref) {
        return Err(semantic("unrecognized observer premise"));
    }
    let o = &ctx.observed;
    if o.is_primary_worktree {
        return Err(semantic("primary integration checkout is not a trial"));
    }
    for p in o.read_paths.iter().chain(&o.write_paths) {
        concrete_path(p.as_str())?;
    }
    if matches!(o.role, Role::Critic | Role::Verifier) && !o.write_paths.is_empty() {
        return Err(semantic(
            "read-only/verifier role cannot write subject workspace",
        ));
    }
    if matches!(o.role, Role::Implementation | Role::Documentation) && o.write_paths.is_empty() {
        return Err(semantic(
            "implementation write set must be explicit and nonempty",
        ));
    }
    Ok(())
}

/// The bounded paired-trial profile's preflight (reference `preflight`):
/// everything in [`check_context_current`] PLUS exact equality of expected and
/// observed facts. A mismatch is TASK_NOT_STARTED — no episode may be
/// fabricated for it. General implementation-worker ancestry rules are NOT
/// replaced by this; exact equality is this profile only.
pub fn check_paired_trial_context(
    ctx: &ExecutionContextReceipt,
    now_ms: u64,
    current_epoch: AuthorityEpoch,
    trusted_observers: &BTreeSet<OpaqueRef>,
) -> Result<(), Refusal> {
    ctx.validate()?;
    if ctx.expected != ctx.observed {
        return Err(semantic("TASK_NOT_STARTED: EXECUTION_CONTEXT_MISMATCH"));
    }
    check_context_current(ctx, now_ms, current_epoch, trusted_observers)
}

/// Join an episode to the exact policy and context bytes it claims.
pub fn bind_episode(
    episode: &LoopEpisode,
    policy: &PolicyEnvelope,
    ctx: &ExecutionContextReceipt,
    current_epoch: AuthorityEpoch,
    trusted_verifiers: &BTreeSet<OpaqueRef>,
    subject_issuers: &BTreeSet<OpaqueRef>,
) -> Result<(), Refusal> {
    episode.validate()?;
    policy.validate()?;
    ctx.validate()?;
    if episode.identity != ctx.identity {
        return Err(semantic(
            "task/arm/trial/attempt/operation/execution mismatch",
        ));
    }
    if episode.scope != ctx.scope || episode.scope != policy.scope {
        return Err(semantic("cross-scope episode"));
    }
    if episode.policy_ref != digest(policy)? || episode.context_ref != digest(ctx)? {
        return Err(semantic("policy/context byte mismatch"));
    }
    if episode.controls_ref != policy.controls_ref
        || episode.candidate_set_ref != policy.candidate_set_ref
    {
        return Err(semantic("effective control/candidate view mismatch"));
    }
    if episode.input_workspace_ref != ctx.observed.workspace_ref {
        return Err(semantic("wrong input workspace"));
    }
    if episode.authority_epoch != current_epoch || ctx.authority_epoch != current_epoch {
        return Err(semantic("stale result authority"));
    }
    let v = &episode.verification;
    if v.result == VerificationResult::Passed {
        let issuer = v.issuer_ref.as_ref();
        if !issuer.is_some_and(|i| trusted_verifiers.contains(i) && !subject_issuers.contains(i)) {
            return Err(semantic(
                "subject or unknown verifier cannot establish outcome",
            ));
        }
        if v.output_workspace_ref != episode.output_workspace_ref {
            return Err(semantic("checked output is not candidate output"));
        }
    }
    Ok(())
}

/// The explicit ACF → sidecar status projection. `timed_out` is
/// `outcome_unknown`, never success.
pub fn project_receipt_status(s: ReceiptStatus) -> EpisodeStatus {
    match s {
        ReceiptStatus::Completed => EpisodeStatus::Completed,
        ReceiptStatus::Failed => EpisodeStatus::Failed,
        ReceiptStatus::Canceled => EpisodeStatus::Cancelled,
        ReceiptStatus::Denied => EpisodeStatus::Refused,
        ReceiptStatus::Unsupported => EpisodeStatus::Unsupported,
        ReceiptStatus::OutcomeUnknown | ReceiptStatus::TimedOut => EpisodeStatus::OutcomeUnknown,
    }
}

/// Structural join of an episode to its ACF request and receipt through a
/// policy projection. Production must authenticate/resolve the projection
/// first; this is shape and identity only.
pub fn bind_acf(
    episode: &LoopEpisode,
    request: &ComputeRequest,
    receipt: &ExecutionReceipt,
    projection: &PolicyProjection,
) -> Result<(), Refusal> {
    episode.validate()?;
    request.validate()?;
    receipt.validate()?;
    if projection.sidecar_policy_ref != episode.policy_ref {
        return Err(semantic("wrong policy projection subject"));
    }
    if request.policy_digest != projection.acf_policy_digest
        || receipt.policy_digest != projection.acf_policy_digest
    {
        return Err(semantic("wrong supervisor policy projection"));
    }
    if episode.acf_request_ref != digest(request)? || episode.acf_receipt_ref != digest(receipt)? {
        return Err(semantic("Fabric reference mismatch"));
    }
    let id = &episode.identity;
    if request.task_id != id.task_id || receipt.task_id != id.task_id {
        return Err(semantic("Fabric identity mismatch: task_id"));
    }
    if request.trial_id != id.trial_id || receipt.trial_id != id.trial_id {
        return Err(semantic("Fabric identity mismatch: trial_id"));
    }
    if request.attempt_id != id.attempt_id || receipt.attempt_id != id.attempt_id {
        return Err(semantic("Fabric identity mismatch: attempt_id"));
    }
    if request.operation_id != id.operation_id || receipt.operation_id != id.operation_id {
        return Err(semantic("Fabric identity mismatch: operation_id"));
    }
    if receipt.execution_id != id.execution_id {
        return Err(semantic("Fabric execution mismatch"));
    }
    if request.workspace_version_ref != episode.input_workspace_ref
        || receipt.input_workspace_ref != episode.input_workspace_ref
    {
        return Err(semantic("Fabric input mismatch"));
    }
    if receipt.output_workspace_ref != episode.output_workspace_ref {
        return Err(semantic("Fabric output mismatch"));
    }
    if project_receipt_status(receipt.status) != episode.status {
        return Err(semantic("Fabric outcome semantics lost"));
    }
    Ok(())
}

/// Whether an episode may feed learning: discovery/tuning corpus, completed,
/// independently verified, and FINAL usage (estimated usage is not final
/// comparative evidence).
pub fn learning_eligible(episode: &LoopEpisode) -> Result<bool, Refusal> {
    episode.validate()?;
    use crate::episode::CorpusRole::*;
    Ok(matches!(episode.corpus_role, Discovery | Tuning)
        && episode.status == EpisodeStatus::Completed
        && episode.verification.result == VerificationResult::Passed
        && episode.usage.state == UsageState::Final)
}
