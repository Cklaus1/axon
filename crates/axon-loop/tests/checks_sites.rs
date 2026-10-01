//! Amendment 64 (C9 round 4b, integrate-B): the refusal sites of
//! `axon-loop-contracts/src/checks.rs` (the cross-document joins), each judged
//! on the PRODUCTION route where it decides: `evl::evaluate` (the context
//! preflight and the ACF join), `intake::intake_episode` (the episode/policy/
//! context join) and `candidates::put_policy` (the shortlist).
//!
//! Every test is an ATTACK (one defect, every other reference recomputed so
//! the documents genuinely bind) with its CONTROL (the same trial without the
//! defect counts). Where two checks refuse the same attack (a four-cell
//! retirement in `scripts/v022_g01_mutations.py`), any refusal is accepted:
//! only the attack getting through is the failure.

mod common;
use axon_loop::evl::{EvaluationRecord, Outcome, TrialResult};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

const TRIAL: &str = "c0";

/// The challenger's first trial, typed: what an attack edits.
struct Docs {
    ep: LoopEpisode,
    ctx: ExecutionContextReceipt,
    req: ComputeRequest,
    rc: ExecutionReceipt,
    proj: PolicyProjection,
}

/// How the edited trial's references are left.
#[derive(Clone, Copy, PartialEq)]
enum Refs {
    /// Recompute the episode's context/request/receipt refs over the edited
    /// documents (the documents bind; only the edited fact is wrong).
    Bind,
    /// Keep the episode's refs (a document other than the one it names).
    Keep,
}

fn typed<T: Contract>(v: &Value) -> T {
    parse(&v.to_string()).unwrap()
}

/// A frozen world with its population assigned and the honest request; the
/// challenger's trial `c0` is `out`, every other trial passes.
fn frozen(exp: &str, out: Out) -> (World, Value) {
    let w = world();
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    for s in specs.iter_mut() {
        if s.3 == TRIAL {
            s.4 = out;
        }
    }
    assign_specs(&w.s, exp, &specs);
    let v = evl_request(exp, &w.inc, &w.cand, &specs, &EvlOpts::default());
    (w, v)
}

/// Apply `edit` to trial `c0` of `v`, then rebind its references.
fn tamper(v: &mut Value, refs: Refs, edit: impl FnOnce(&mut Docs)) {
    let t = v["trials"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|t| t["episode"]["identity"]["trial_id"] == TRIAL)
        .unwrap();
    let mut d = Docs {
        ep: typed(&t["episode"]),
        ctx: typed(&t["context"]),
        req: typed(&t["acf_request"]),
        rc: typed(&t["acf_receipt"]),
        proj: serde_json::from_value(t["projection"].clone()).unwrap(),
    };
    edit(&mut d);
    if refs == Refs::Bind {
        d.ep.context_ref = digest(&d.ctx).unwrap();
        d.ep.acf_request_ref = digest(&d.req).unwrap();
        d.ep.acf_receipt_ref = digest(&d.rc).unwrap();
    }
    let sig = if d.ctx.observed_issuer_ref.as_str() == OBSERVER {
        axon_loop_contracts::attestation::sign_document(
            &observer_key().0,
            axon_loop::evl::CONTEXT_DOMAIN,
            &d.ctx.observed_issuer_ref,
            &serde_json::to_value(&d.ctx).unwrap(),
        )
        .unwrap()
    } else {
        Value::Null
    };
    t["episode"] = json!(d.ep);
    t["context"] = json!(d.ctx);
    t["acf_request"] = json!(d.req);
    t["acf_receipt"] = json!(d.rc);
    t["projection"] = json!(d.proj);
    // An absent signature is an absent key (the request's canonical shape).
    if sig.is_null() {
        t.as_object_mut().unwrap().remove("context_signature");
    } else {
        t["context_signature"] = sig;
    }
}

/// Intake what the request delivers, then evaluate it (the production order).
/// Returns intake's refusal of `c0` (if any), and `c0`'s result, or the
/// evaluation's own refusal.
fn run(w: &World, v: &Value) -> (Option<String>, Result<TrialResult, String>) {
    let intake = intake_all(&w.s, v)
        .into_iter()
        .find(|(t, _)| t == TRIAL)
        .map(|(_, e)| e);
    let rec = axon_loop::evl::parse_request(&v.to_string())
        .and_then(|r| axon_loop::evl::evaluate(&w.s, &r));
    let res = match rec {
        Ok((rec, _)) => Ok(c0(&rec, w)),
        Err(e) => Err(e.to_string()),
    };
    (intake, res)
}

fn c0(rec: &EvaluationRecord, w: &World) -> TrialResult {
    rec.arm_for_policy(&w.cand_ref)
        .unwrap()
        .trials
        .iter()
        .find(|t| t.trial_id.as_str() == TRIAL)
        .unwrap()
        .clone()
}

/// The attack: `c0` (built as `out`) edited by `edit` must not COUNT (as a
/// pass, or as a fail for a Fail trial) in the evaluation; ATTACK if it does.
/// Control: the same world and request without the edit counts it as `out`.
fn never_counts(exp: &str, out: Out, refs: Refs, attack: &str, edit: impl FnOnce(&mut Docs)) {
    let (w, v) = frozen(&format!("{exp}-ok"), out);
    let (i, r) = run(&w, &v);
    let want = if out == Out::Pass {
        Outcome::VerifiedPass
    } else {
        Outcome::Fail
    };
    match (&i, &r) {
        (None, Ok(t)) if t.outcome == want => {}
        _ => panic!("control: the honest {exp} trial did not count: {i:?} {r:?}"),
    }
    let (w, mut v) = frozen(exp, out);
    tamper(&mut v, refs, edit);
    let (i, r) = run(&w, &v);
    if let Ok(t) = &r {
        if t.outcome == want {
            panic!(
                "ATTACK: {attack}: the trial counted ({:?}; intake: {i:?})",
                t.outcome
            );
        }
    }
    eprintln!("refused {exp}: intake {i:?}; evaluation {r:?}");
}

// ── the context preflight (`check_paired_trial_context`), on EVL ────────────

fn paths(d: &mut Docs, p: &str) {
    for f in [&mut d.ctx.expected, &mut d.ctx.observed] {
        f.read_paths = vec![BoundedText::new(p).unwrap()];
    }
}

#[test]
fn a_context_granting_a_backslash_path_never_counts() {
    never_counts(
        "cp-bs",
        Out::Pass,
        Refs::Bind,
        "a context granting a non-concrete (backslash) path",
        |d| paths(d, "src\\main.rs"),
    );
}

#[test]
fn a_context_granting_a_pattern_never_counts() {
    never_counts(
        "cp-glob",
        Out::Pass,
        Refs::Bind,
        "a context granting a glob pattern as a path",
        |d| paths(d, "src/*.rs"),
    );
}

#[test]
fn a_context_granting_a_parent_path_never_counts() {
    never_counts(
        "cp-up",
        Out::Pass,
        Refs::Bind,
        "a context granting a path that leaves the workspace",
        |d| paths(d, "../outside.rs"),
    );
}

#[test]
fn an_expired_context_never_counts() {
    never_counts(
        "cx-exp",
        Out::Pass,
        Refs::Bind,
        "a trial run under an expired preflight context",
        |d| d.ctx.expires_ms = d.ctx.created_ms + 1,
    );
}

#[test]
fn a_context_of_another_authority_epoch_never_counts() {
    never_counts(
        "cx-epoch",
        Out::Pass,
        Refs::Bind,
        "a trial preflighted under another authority epoch",
        |d| d.ctx.authority_epoch = AuthorityEpoch::new(2).unwrap(),
    );
}

#[test]
fn a_context_its_own_parent_observed_never_counts() {
    never_counts(
        "cx-echo",
        Out::Pass,
        Refs::Bind,
        "a preflight observed by the parent that expected it (an echo)",
        |d| d.ctx.expected_issuer_ref = d.ctx.observed_issuer_ref.clone(),
    );
}

#[test]
fn a_context_observed_by_an_unrecognized_observer_never_counts() {
    never_counts(
        "cx-stranger",
        Out::Pass,
        Refs::Bind,
        "a preflight observed by no recognized observer",
        |d| d.ctx.observed_issuer_ref = OpaqueRef::new("fixture:stranger").unwrap(),
    );
}

#[test]
fn a_trial_in_the_primary_checkout_never_counts() {
    never_counts(
        "cx-primary",
        Out::Pass,
        Refs::Bind,
        "a trial run in the primary integration checkout",
        |d| {
            d.ctx.expected.is_primary_worktree = true;
            d.ctx.observed.is_primary_worktree = true;
        },
    );
}

#[test]
fn a_critic_that_writes_never_counts() {
    never_counts(
        "cx-critic",
        Out::Pass,
        Refs::Bind,
        "a read-only (critic) role granted a write set",
        |d| {
            d.ctx.expected.role = axon_loop_contracts::context::Role::Critic;
            d.ctx.observed.role = axon_loop_contracts::context::Role::Critic;
        },
    );
}

#[test]
fn an_implementation_with_no_write_set_never_counts() {
    never_counts(
        "cx-nowrite",
        Out::Pass,
        Refs::Bind,
        "an implementation role with no explicit write set",
        |d| {
            d.ctx.expected.write_paths = vec![];
            d.ctx.observed.write_paths = vec![];
        },
    );
}

#[test]
fn a_context_that_did_not_bind_never_counts() {
    never_counts(
        "cx-mismatch",
        Out::Pass,
        Refs::Bind,
        "a preflight whose observed facts are not the expected ones",
        |d| d.ctx.observed.branch = BoundedText::new("another-branch").unwrap(),
    );
}

// ── the episode join (`bind_episode`), at intake and on EVL ────────────────

#[test]
fn an_episode_bound_to_another_trials_context_never_counts() {
    never_counts(
        "be-id",
        Out::Pass,
        Refs::Bind,
        "an episode bound to a context of another operation",
        |d| d.ctx.identity.operation_id = OperationId::new("another-op").unwrap(),
    );
}

#[test]
fn an_episode_under_other_controls_never_counts() {
    never_counts(
        "be-ctl",
        Out::Pass,
        Refs::Bind,
        "an episode run under other controls than its policy's",
        |d| d.ep.controls_ref = r('9'),
    );
}

#[test]
fn an_episode_of_another_authority_epoch_never_counts() {
    never_counts(
        "be-epoch",
        Out::Pass,
        Refs::Bind,
        "an episode recorded under another authority epoch",
        |d| d.ep.authority_epoch = AuthorityEpoch::new(2).unwrap(),
    );
}

#[test]
fn a_pass_checked_on_another_output_never_counts() {
    never_counts(
        "be-out",
        Out::Pass,
        Refs::Bind,
        "a pass whose checked output is not the episode's output",
        |d| {
            // The verifier's tree and the execution's output agree with each
            // other, not with the episode: only the episode join tells.
            d.ep.verification.output_workspace_ref = Some(other_acf());
            d.rc.output_workspace_ref = Some(other_acf());
        },
    );
}

// ── the ACF join (`bind_acf`), on EVL ───────────────────────────────────────

fn other_acf() -> Acf1Ref {
    Acf1Ref::new(format!("acf1:{}", "e".repeat(64))).unwrap()
}

#[test]
fn a_projection_of_another_policy_never_counts() {
    never_counts(
        "ba-proj",
        Out::Pass,
        Refs::Bind,
        "an ACF projection of another sidecar policy",
        |d| d.proj.sidecar_policy_ref = r('0'),
    );
}

#[test]
fn a_request_under_another_supervisor_policy_never_counts() {
    never_counts(
        "ba-pdig",
        Out::Pass,
        Refs::Bind,
        "an execution under a supervisor policy the projection does not map",
        |d| d.req.policy_digest = other_acf(),
    );
}

#[test]
fn a_request_other_than_the_one_the_episode_names_never_counts() {
    never_counts(
        "ba-ref",
        Out::Pass,
        Refs::Keep,
        "an execution request other than the one the episode names",
        |d| d.req.limits.wall_time_ms += 1,
    );
}

#[test]
fn a_request_of_another_task_never_counts() {
    never_counts(
        "ba-task",
        Out::Pass,
        Refs::Bind,
        "an execution request of another task",
        |d| d.req.task_id = TaskId::new("another-task").unwrap(),
    );
}

#[test]
fn a_request_of_another_trial_never_counts() {
    never_counts(
        "ba-trial",
        Out::Pass,
        Refs::Bind,
        "an execution request of another trial",
        |d| d.req.trial_id = TrialId::new("another-trial").unwrap(),
    );
}

#[test]
fn a_request_of_another_attempt_never_counts() {
    never_counts(
        "ba-att",
        Out::Pass,
        Refs::Bind,
        "an execution request of another attempt",
        |d| d.req.attempt_id = AttemptId::new("another-attempt").unwrap(),
    );
}

#[test]
fn a_request_of_another_operation_never_counts() {
    never_counts(
        "ba-op",
        Out::Pass,
        Refs::Bind,
        "an execution request of another operation",
        |d| d.req.operation_id = OperationId::new("another-op").unwrap(),
    );
}

#[test]
fn a_receipt_of_another_execution_never_counts() {
    never_counts(
        "ba-ex",
        Out::Pass,
        Refs::Bind,
        "an execution receipt of another execution",
        |d| d.rc.execution_id = ExecutionId::new("another-ex").unwrap(),
    );
}

#[test]
fn an_execution_from_another_input_never_counts() {
    never_counts(
        "ba-in",
        Out::Pass,
        Refs::Bind,
        "an execution that ran on another input workspace",
        |d| d.req.workspace_version_ref = other_acf(),
    );
}

#[test]
fn a_failure_resting_on_another_output_never_counts() {
    never_counts(
        "ba-out-f",
        Out::Fail,
        Refs::Bind,
        "a failure whose execution left another output than the episode's",
        |d| d.rc.output_workspace_ref = Some(other_acf()),
    );
}

#[test]
fn a_pass_resting_on_another_output_never_counts() {
    never_counts(
        "ba-out-p",
        Out::Pass,
        Refs::Bind,
        "a pass whose execution left another output than the one verified",
        |d| d.rc.output_workspace_ref = Some(other_acf()),
    );
}

#[test]
fn a_pass_whose_execution_failed_never_counts() {
    never_counts(
        "ba-status",
        Out::Pass,
        Refs::Bind,
        "a completed episode whose execution receipt records a failure",
        |d| {
            d.rc.status = ReceiptStatus::Failed;
            d.rc.process_exit_code = Some(1);
        },
    );
}

#[test]
fn a_completion_resting_on_a_worker_report_never_counts() {
    never_counts(
        "ba-src",
        Out::Pass,
        Refs::Bind,
        "a completion resting on an execution receipt the worker reported",
        |d| d.rc.evidence_source = axon_loop_contracts::receipt::EvidenceSource::WorkerReported,
    );
}

// ── the shortlist (`check_shortlist`), on `policy put` ──────────────────────

/// `policy put` stores a policy only if its shortlist is within the
/// registered candidate list: a shortlist naming a tool the operator never
/// registered is refused and nothing is stored. Control: a shortlist within
/// the list is stored.
#[test]
fn a_policy_shortlisting_an_unregistered_candidate_is_never_stored() {
    let w = world();
    let ok = policy("within-the-list", &["read"]);
    axon_loop::candidates::put_policy(&w.s, &ok).expect("control: a policy within the list");
    let rogue = policy("rogue", &["read", "rogue-tool"]);
    let before = snapshot(w.dir.path());
    match axon_loop::candidates::put_policy(&w.s, &rogue) {
        Ok(r) => panic!(
            "ATTACK: a policy shortlisting a candidate outside the registered list was stored as {r}"
        ),
        Err(e) => assert!(e.to_string().contains("expands eligible"), "{e}"),
    }
    assert_eq!(snapshot(w.dir.path()), before, "a refusal wrote something");
}
