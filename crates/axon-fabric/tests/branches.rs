//! B271 — logical A/B branches (G08-r22-logical-branches), fenced workspace
//! CAS publication (G11-r22-workspace-cas) and branch cancellation
//! (G08-r22-branch-cancellation), through the real submit path, a real
//! journal and a real axon-loop epoch store.

mod common;
use common::*;

use axon_fabric::branches::{BranchError, Branches, Publication};
use axon_fabric::workspace::{Quota, TrialCache, WorkspaceStore};
use axon_fabric::{submit, Journal, OpState, ResourceVector};
use axon_loop_contracts::{
    Acf1Ref, ArmId, AuthorityEpoch, OpaqueRef, OperationId, ReceiptVerification, TaskId, TrialId,
};
use serde_json::Value;

const WRITER_A: &str = "writer:a";
const WRITER_B: &str = "writer:b";
const APPROVER: &str = "approver:independent";

fn tenant() -> axon_loop_contracts::TenantId {
    axon_loop_contracts::TenantId::new("tenant-t").unwrap()
}
fn o(s: &str) -> OpaqueRef {
    OpaqueRef::new(s).unwrap()
}
fn arm(s: &str) -> ArmId {
    ArmId::new(s).unwrap()
}
fn exp_id() -> TaskId {
    TaskId::new("exp-1").unwrap()
}
fn op(s: &str) -> OperationId {
    OperationId::new(s).unwrap()
}

/// Publish the tree `{f.ax: src}` in the tenant's store.
fn version(env: &Env, name: &str, src: &str) -> Acf1Ref {
    let d = env.dir.path().join(format!("tree-{name}"));
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("f.ax"), src).unwrap();
    WorkspaceStore::open(&env.cfg(0).state_dir, &tenant())
        .unwrap()
        .import_dir(&d, &Quota::default())
        .unwrap()
}

const REGIME: ResourceVector = ResourceVector {
    model_micro_usd: 1_000,
    exec_ms: 100_000,
    verify_ms: 0,
    retries: 0,
};

struct World {
    env: Env,
    base: Acf1Ref,
    br: Branches,
}

fn world() -> World {
    let env = Env::new();
    let base = version(&env, "base", FIXTURE);
    let br = Branches::open(&env.cfg(0).state_dir, &scope());
    br.open_experiment(
        &exp_id(),
        &base,
        REGIME,
        &[
            (arm("incumbent"), o(WRITER_A)),
            (arm("challenger-1"), o(WRITER_B)),
        ],
        &[o(APPROVER)],
    )
    .unwrap();
    World { env, base, br }
}

fn run_id(w: &World, a: &str) -> TrialId {
    w.br.experiment(&exp_id())
        .unwrap()
        .branch(&arm(a))
        .unwrap()
        .run_id
        .clone()
}

/// A branch-run check of `version` (filter `filter`).
fn branch_check(w: &World, a: &str, opid: &str, v: &Acf1Ref, filter: &str) -> Value {
    let mut r = request(&w.env, opid, filter);
    r["trial_id"] = run_id(w, a).as_str().into();
    r["workspace_version_ref"] = v.as_str().into();
    r
}

fn head_files(w: &World, a: &str) -> usize {
    let key = {
        use sha2::{Digest, Sha256};
        format!("{:x}", Sha256::digest(tenant().as_str().as_bytes()))
    };
    let d = w
        .env
        .cfg(0)
        .state_dir
        .join("tenants")
        .join(&key[..32])
        .join("branches/exp-1")
        .join(a);
    std::fs::read_dir(d)
        .unwrap()
        .flatten()
        .filter(|e| e.file_name().to_string_lossy().starts_with("head-"))
        .count()
}

// ── G08: logical branches ───────────────────────────────────────────────────

#[test]
fn branches_start_from_one_frozen_base_with_independent_run_identities() {
    let w = world();
    let e = w.br.experiment(&exp_id()).unwrap();
    assert_eq!(e.base, w.base);
    assert_eq!(e.regime, REGIME);
    let (a, b) = (run_id(&w, "incumbent"), run_id(&w, "challenger-1"));
    assert_ne!(a, b, "independent run identities");
    for x in ["incumbent", "challenger-1"] {
        let h = w.br.head(&exp_id(), &arm(x)).unwrap();
        assert_eq!((h.seq, &h.version), (0, &w.base), "{x} starts at the base");
    }
    let sd = w.env.cfg(0).state_dir;
    assert_ne!(
        TrialCache::for_trial(&sd, &tenant(), &a).unwrap(),
        TrialCache::for_trial(&sd, &tenant(), &b).unwrap(),
        "no shared mutable cache"
    );
    // Write-once: the same experiment again is idempotent, a different one
    // under the same id is refused.
    let same = w.br.open_experiment(
        &exp_id(),
        &w.base,
        REGIME,
        &[
            (arm("incumbent"), o(WRITER_A)),
            (arm("challenger-1"), o(WRITER_B)),
        ],
        &[o(APPROVER)],
    );
    assert!(same.is_ok());
    let other = version(&w.env, "other", "fn x() -> i64 { 1 }\n");
    let e = match w.br.open_experiment(
        &exp_id(),
        &other,
        REGIME,
        &[
            (arm("incumbent"), o(WRITER_A)),
            (arm("challenger-1"), o(WRITER_B)),
        ],
        &[o(APPROVER)],
    ) {
        Err(e) => e,
        Ok(_) => {
            panic!("ATTACK: a different experiment under an existing id was taken for the same one")
        }
    };
    assert_eq!(e.kind(), "exists");
}

#[test]
fn an_experiment_needs_a_durable_base_two_arms_and_independent_approval() {
    let w = world();
    let x = TaskId::new("exp-2").unwrap();
    let arms = [
        (arm("incumbent"), o(WRITER_A)),
        (arm("challenger-1"), o(WRITER_B)),
    ];
    // Each refusal is read as `invalid`, and an ACCEPTANCE names the attack
    // (an `unwrap_err` on an `Ok` would not say which guard let it through).
    let invalid = |what: &str, r: Result<axon_fabric::branches::Experiment, BranchError>| match r {
        Err(e) => assert_eq!(e.kind(), "invalid", "{what}: {e}"),
        Ok(_) => panic!("ATTACK: {what} was accepted"),
    };
    // A hash is not a base.
    let hash_only = Acf1Ref::new(format!("acf1:{}", "4".repeat(64))).unwrap();
    invalid(
        "a base that is only a hash",
        w.br.open_experiment(&x, &hash_only, REGIME, &arms, &[o(APPROVER)]),
    );
    invalid(
        "an experiment with one arm",
        w.br.open_experiment(&x, &w.base, REGIME, &arms[..1], &[o(APPROVER)]),
    );
    invalid(
        "a writer that is also the approver",
        w.br.open_experiment(&x, &w.base, REGIME, &arms, &[o(WRITER_A)]),
    );
    assert!(w.br.experiment(&x).is_err(), "nothing was recorded");
}

#[test]
fn each_branch_carves_within_its_declared_regime_and_is_isolated() {
    let w = world();
    let cfg = w.env.cfg(0);
    // 60 000 exec-ms per op against a 100 000 regime: one fits, two do not.
    let s = submit(
        &branch_check(&w, "incumbent", "a-1", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    let launches = w.env.launch_records();
    let e = submit(
        &branch_check(&w, "incumbent", "a-2", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap_err();
    assert_eq!(e.kind(), "branch", "{e}");
    assert_eq!(
        w.env.launch_records(),
        launches,
        "no launch for the refused op"
    );
    // The other branch has its own regime.
    let s = submit(
        &branch_check(&w, "challenger-1", "b-1", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    assert_eq!(j.view(&op("a-2")).unwrap().state, OpState::Cancelled);
    assert!(j.view(&op("a-2")).unwrap().billing.is_none(), "released");
    assert!(w.env.journal_text().contains("\"arm\":\"incumbent\""));
}

// ── G11: workspace CAS ──────────────────────────────────────────────────────

const FIXTURE_V2: &str = "\
fn double(n: i64) -> i64 { n + n }

@[test]
fn t_ok() { assert_eq(double(2), 4) }
";
const FIXTURE_V3: &str = "\
fn double(n: i64) -> i64 { 2 * n }

@[test]
fn t_ok() { assert_eq(double(2), 4) }
";

fn publication(seq: u64, expected: &Acf1Ref, new: &Acf1Ref, by: &str) -> Publication {
    Publication {
        expected_seq: seq,
        expected_version: expected.clone(),
        new_version: new.clone(),
        verified_by: op(by),
        writer: o(WRITER_A),
        approver: o(APPROVER),
        epoch: AuthorityEpoch::new(0).unwrap(),
    }
}

fn publish(w: &World, p: &Publication) -> Result<axon_fabric::branches::Head, BranchError> {
    let cfg = w.env.cfg(0);
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    w.br.publish(&j, &cfg.epoch, &exp_id(), &arm("incumbent"), p)
}

#[test]
fn publication_requires_base_epoch_writer_exact_verified_output_and_approval() {
    let w = world();
    let cfg = w.env.cfg(0);
    let v2 = version(&w.env, "v2", FIXTURE_V2);
    // Verified ON the incumbent branch, for v2.
    let s = submit(
        &branch_check(&w, "incumbent", "ver-v2", &v2, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    // A FAILED check of v2, and a check of v2 on the OTHER branch.
    submit(
        &branch_check(&w, "incumbent", "bad-v2", &v2, "t_bad").to_string(),
        &cfg,
    )
    .ok();
    submit(
        &branch_check(&w, "challenger-1", "b-v2", &v2, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();

    let good = publication(0, &w.base, &v2, "ver-v2");
    let refusals: Vec<(&str, Publication, &str)> = vec![
        (
            "wrong writer",
            Publication {
                writer: o(WRITER_B),
                ..good.clone()
            },
            "unauthorized",
        ),
        (
            "self-approval",
            Publication {
                approver: o(WRITER_A),
                ..good.clone()
            },
            "unauthorized",
        ),
        (
            "undeclared approver",
            Publication {
                approver: o("approver:rogue"),
                ..good.clone()
            },
            "unauthorized",
        ),
        (
            "stale fencing epoch",
            Publication {
                epoch: AuthorityEpoch::new(7).unwrap(),
                ..good.clone()
            },
            "stale_epoch",
        ),
        (
            "receipt not in the journal",
            Publication {
                verified_by: op("hand-written"),
                ..good.clone()
            },
            "unverified",
        ),
        (
            "failed verification",
            Publication {
                verified_by: op("bad-v2"),
                ..good.clone()
            },
            "unverified",
        ),
        (
            "verified on another branch",
            Publication {
                verified_by: op("b-v2"),
                ..good.clone()
            },
            "unverified",
        ),
        (
            "verified other bytes",
            Publication {
                new_version: w.base.clone(),
                ..good.clone()
            },
            "unverified",
        ),
        (
            "wrong expected base",
            Publication {
                expected_seq: 3,
                ..good.clone()
            },
            "conflict",
        ),
    ];
    for (what, p, kind) in refusals {
        let e = match publish(&w, &p) {
            Err(e) => e,
            Ok(h) => panic!("ATTACK: {what} was accepted: {h:?}"),
        };
        assert_eq!(e.kind(), kind, "{what}: {e}");
        assert_eq!(head_files(&w, "incumbent"), 1, "{what}: head unchanged");
        assert_eq!(
            w.br.head(&exp_id(), &arm("incumbent")).unwrap().version,
            w.base
        );
    }
    assert!(
        !w.env.journal_text().contains("\"workspace_publication\""),
        "nothing journalled"
    );

    // All five hold: published, journalled with its CAS precondition.
    let h = publish(&w, &good).unwrap();
    assert_eq!((h.seq, &h.version), (1, &v2));
    assert_eq!(w.br.head(&exp_id(), &arm("incumbent")).unwrap(), h);
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    let pv = j.view(h.publication_op.as_ref().unwrap()).unwrap();
    assert_eq!(pv.state, OpState::Completed);
    assert_eq!(
        pv.intent.expected_version, 0,
        "the CAS precondition is journalled"
    );
    drop(j);
    // The other branch never moved.
    assert_eq!(
        w.br.head(&exp_id(), &arm("challenger-1")).unwrap().version,
        w.base
    );

    // A concurrent update forces rebase + re-verification: publishing from
    // the OLD head is a conflict, never an overwrite.
    let e = match publish(&w, &good) {
        Err(e) => e,
        Ok(h) => panic!("ATTACK: a publication from an old head was accepted: {h:?}"),
    };
    assert_eq!(e.kind(), "conflict");
    assert_eq!(head_files(&w, "incumbent"), 2);
}

#[test]
fn two_concurrent_publications_from_one_head_have_exactly_one_winner() {
    let w = world();
    let cfg = w.env.cfg(0);
    let v2 = version(&w.env, "v2", FIXTURE_V2);
    let v3 = version(&w.env, "v3", FIXTURE_V3);
    for (id, v) in [("ver-v2", &v2), ("ver-v3", &v3)] {
        let mut r = branch_check(&w, "incumbent", id, v, "t_ok");
        r["limits"]["wall_time_ms"] = 30_000.into(); // two fit the regime
        let s = submit(&r.to_string(), &cfg).unwrap();
        assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    }
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    let j = std::sync::Arc::new(j);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(2));
    let handles: Vec<_> = [(v2.clone(), "ver-v2"), (v3.clone(), "ver-v3")]
        .into_iter()
        .map(|(v, by)| {
            let (j, b, br, ep, base) = (
                j.clone(),
                barrier.clone(),
                w.br.clone(),
                cfg.epoch.clone(),
                w.base.clone(),
            );
            std::thread::spawn(move || {
                let p = Publication {
                    expected_seq: 0,
                    expected_version: base,
                    new_version: v,
                    verified_by: op(by),
                    writer: o(WRITER_A),
                    approver: o(APPROVER),
                    epoch: AuthorityEpoch::new(0).unwrap(),
                };
                b.wait();
                br.publish(&j, &ep, &exp_id(), &arm("incumbent"), &p)
            })
        })
        .collect();
    let rs: Vec<_> = handles.into_iter().map(|h| h.join().unwrap()).collect();
    let wins: Vec<_> = rs.iter().filter_map(|r| r.as_ref().ok()).collect();
    let losses: Vec<_> = rs.iter().filter_map(|r| r.as_ref().err()).collect();
    assert_eq!(wins.len(), 1, "{rs:?}");
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].kind(), "conflict", "{}", losses[0]);
    assert_eq!(head_files(&w, "incumbent"), 2);
    assert_eq!(&w.br.head(&exp_id(), &arm("incumbent")).unwrap(), wins[0]);
}

// ── G08: cancellation ───────────────────────────────────────────────────────

/// A cancelled branch refuses a publication that would otherwise SUCCEED: the
/// check passed on that branch, the writer, approver, epoch, bytes and base
/// are all right. (The cancellation test below refuses its publication for
/// an unverified output as well, so it does not show the cancelled-branch
/// check by itself.) Control: the identical publication on the same branch
/// before it is cancelled is a head, on the other branch after.
#[test]
fn a_cancelled_branch_refuses_a_publication_that_would_otherwise_succeed() {
    let w = world();
    let cfg = w.env.cfg(0);
    let v2 = version(&w.env, "v2", FIXTURE_V2);
    for (arm_name, id) in [("challenger-1", "ch-v2"), ("incumbent", "inc-v2")] {
        let s = submit(
            &branch_check(&w, arm_name, id, &v2, "t_ok").to_string(),
            &cfg,
        )
        .unwrap();
        assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    }
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    w.br.cancel(&j, &exp_id(), &arm("challenger-1"), "lost the A/B")
        .unwrap();
    let on_challenger = Publication {
        writer: o(WRITER_B),
        ..publication(0, &w.base, &v2, "ch-v2")
    };
    match w.br.publish(
        &j,
        &cfg.epoch,
        &exp_id(),
        &arm("challenger-1"),
        &on_challenger,
    ) {
        Err(e) => assert_eq!(e.kind(), "cancelled", "{e}"),
        Ok(h) => panic!("ATTACK: a cancelled branch accepted a publication: {h:?}"),
    }
    assert_eq!(
        w.br.head(&exp_id(), &arm("challenger-1")).unwrap().version,
        w.base,
        "the cancelled branch's head did not move"
    );
    // Control: the same publication on the branch that was not cancelled.
    let h =
        w.br.publish(
            &j,
            &cfg.epoch,
            &exp_id(),
            &arm("incumbent"),
            &publication(0, &w.base, &v2, "inc-v2"),
        )
        .unwrap();
    assert_eq!((h.seq, &h.version), (1, &v2));
}

fn in_flight(j: &Journal, w: &World, a: &str, id: &str, launched: bool) {
    let intent = axon_fabric::Intent {
        op: op(id),
        task_id: TaskId::new("task-1").unwrap(),
        trial_id: run_id(w, a),
        attempt_id: axon_loop_contracts::AttemptId::new("att").unwrap(),
        input_digest: axon_loop_contracts::Ref::new(format!("cl22:{}", "d".repeat(64))).unwrap(),
        config: serde_json::json!({}),
        authority_ref: "x".into(),
        authority_epoch: AuthorityEpoch::new(0).unwrap(),
        scope: scope(),
        reservation: ResourceVector {
            model_micro_usd: 10,
            exec_ms: 1_000,
            verify_ms: 0,
            retries: 0,
        },
        expected_version: 0,
    };
    j.begin(intent).unwrap();
    j.reserve(&op(id)).unwrap();
    if launched {
        j.mark_launched(&op(id)).unwrap();
    }
}

#[test]
fn cancelling_a_losing_branch_keeps_its_record_and_leaves_the_winner_alone() {
    let w = world();
    let cfg = w.env.cfg(0);
    // The loser: one UNFAVORABLE verdict on record.
    let s = submit(
        &branch_check(&w, "challenger-1", "loser-bad", &w.base, "t_bad").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Failed);
    submit(
        &branch_check(&w, "incumbent", "winner-1", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();
    let (loser_run, winner_run) = (run_id(&w, "challenger-1"), run_id(&w, "incumbent"));
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    in_flight(&j, &w, "challenger-1", "loser-launched", true);
    in_flight(&j, &w, "challenger-1", "loser-reserved", false);
    let loser = |i: &axon_fabric::Intent| i.trial_id == loser_run;
    let winner = |i: &axon_fabric::Intent| i.trial_id == winner_run;
    let before_text = std::fs::read_to_string(&w.env.journal).unwrap();
    let lb = j.usage_where(&scope(), loser).unwrap();
    let wb = j.usage_where(&scope(), winner).unwrap();
    let winner_views_before = {
        let mut v: Vec<_> = j
            .views_where(winner)
            .into_iter()
            .map(|v| (v.intent.op, format!("{:?}", v.state)))
            .collect();
        v.sort();
        v
    };

    let done =
        w.br.cancel(&j, &exp_id(), &arm("challenger-1"), "lost the A/B")
            .unwrap();
    let mut done: Vec<String> = done.into_iter().map(|o| o.to_string()).collect();
    done.sort();
    assert_eq!(done, vec!["loser-launched", "loser-reserved"]);

    // Events preserved: the journal only grew.
    let after_text = std::fs::read_to_string(&w.env.journal).unwrap();
    assert!(after_text.starts_with(&before_text));
    // The unfavorable outcome is still there.
    let bad = j.view(&op("loser-bad")).unwrap();
    assert_eq!(
        bad.outcome.as_ref().unwrap()["receipt"]["verification"],
        "failed"
    );
    // Descendants reconciled: launched keeps liability, reserved is released.
    let l = j.view(&op("loser-launched")).unwrap();
    assert_eq!(l.state, OpState::Cancelled);
    assert_eq!(l.billing, Some(axon_fabric::Billing::Unknown));
    let r = j.view(&op("loser-reserved")).unwrap();
    assert_eq!((r.state, r.billing), (OpState::Cancelled, None));
    let la = j.usage_where(&scope(), loser).unwrap();
    assert_eq!(la.held, ResourceVector::default());
    assert_eq!(
        la.liability,
        lb.liability.saturating_add_pub(ResourceVector {
            model_micro_usd: 10,
            exec_ms: 1_000,
            verify_ms: 0,
            retries: 0,
        }),
        "the launched op's liability moved from held to liability, never dropped"
    );
    // The winner is untouched.
    assert_eq!(j.usage_where(&scope(), winner).unwrap(), wb);
    let mut winner_views_after: Vec<_> = j
        .views_where(winner)
        .into_iter()
        .map(|v| (v.intent.op, format!("{:?}", v.state)))
        .collect();
    winner_views_after.sort();
    assert_eq!(winner_views_after, winner_views_before);
    drop(j);

    // The cancelled branch accepts nothing new …
    let launches = w.env.launch_records();
    let e = submit(
        &branch_check(&w, "challenger-1", "loser-again", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap_err();
    assert_eq!(e.kind(), "branch");
    assert_eq!(w.env.launch_records(), launches);
    assert!(
        !w.env.journal_text().contains("loser-again"),
        "not even an intent"
    );
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    let e =
        w.br.publish(
            &j,
            &cfg.epoch,
            &exp_id(),
            &arm("challenger-1"),
            &Publication {
                writer: o(WRITER_B),
                ..publication(0, &w.base, &w.base, "loser-bad")
            },
        )
        .unwrap_err();
    assert_eq!(e.kind(), "cancelled");
    drop(j);
    // … and the surviving branch still runs (30 000 more fits the 40 000 of
    // its regime that winner-1 left).
    let mut r = branch_check(&w, "incumbent", "winner-2", &w.base, "t_ok");
    r["limits"]["wall_time_ms"] = 30_000.into();
    let s = submit(&r.to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
}

trait AddPub {
    fn saturating_add_pub(self, o: Self) -> Self;
}
impl AddPub for ResourceVector {
    fn saturating_add_pub(self, o: Self) -> Self {
        ResourceVector {
            model_micro_usd: self.model_micro_usd + o.model_micro_usd,
            exec_ms: self.exec_ms + o.exec_ms,
            verify_ms: self.verify_ms + o.verify_ms,
            retries: self.retries + o.retries,
        }
    }
}

/// C9 round 4b, rows4b (amendment 62): once a branch is cancelled, a run of
/// it is refused before anything is journalled or launched (B271). Control:
/// the other arm still runs.
#[test]
fn a_cancelled_branch_never_runs_again() {
    let w = world();
    let cfg = w.env.cfg(0);
    let (j, _) = Journal::open(&w.env.journal).unwrap();
    w.br.cancel(&j, &exp_id(), &arm("challenger-1"), "lost the A/B")
        .unwrap();
    drop(j);
    let spawns = spawn_count(&w.env.spawns);
    let r = submit(
        &branch_check(&w, "challenger-1", "after-cancel", &w.base, "t_ok").to_string(),
        &cfg,
    );
    assert!(
        matches!(r, Err(axon_fabric::SubmitError::Branch(_)))
            && spawn_count(&w.env.spawns) == spawns,
        "ATTACK: a run of a cancelled branch was dispatched: {:?}",
        r.map(|s| s.receipt.status)
    );
    let ok = submit(
        &branch_check(&w, "incumbent", "after-cancel-ok", &w.base, "t_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        ok.receipt.verification,
        ReceiptVerification::Passed,
        "control"
    );
}
