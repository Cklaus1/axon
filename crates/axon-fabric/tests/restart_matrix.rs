//! B280 / G13-r22-restart-matrix — a REAL process dies at every important
//! effect boundary of `submit`, and a REAL restart (a fresh `Journal::open`
//! and a re-submit of the same operation) must leave:
//!
//! * no silent repeated consequential operation — the check worker is spawned
//!   at most once per operation, ever;
//! * no op that names an owner that does not exist — a pre-launch orphan is
//!   RESUMED (its first execution), a launched one is OutcomeUnknown;
//! * financial obligations reconciled or explicitly pending — a released
//!   reservation only where nothing was launched, liability kept otherwise.
//!
//! The process death is `std::process::abort()` inside a crash child at the
//! boundary (via `SubmitConfig::fault_hook`), so the journal on disk is exactly
//! what that point leaves behind. The no-unowned-worker half of the gate is
//! `submit.rs::sigkill_after_launch_reconciles_to_outcome_unknown_with_liability`.

mod common;
use common::*;

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use axon_fabric::journal::OpState;
use axon_fabric::{submit, Boundary, SubmitError};
use axon_loop_contracts::{OperationId, ReceiptStatus, ReceiptVerification};

fn op(s: &str) -> OperationId {
    OperationId::new(s).unwrap()
}

/// The child's config: `Env::cfg` rebuilt from the env's paths.
fn cfg_at(root: &Path, epoch: u64) -> axon_fabric::SubmitConfig {
    axon_fabric::SubmitConfig {
        journal: root.join("ops.journal"),
        registry: axon_cortex::runner::CheckRegistry::load(&root.join("registry.json")).unwrap(),
        epoch: axon_fabric::EpochSource::LoopStore {
            store: root.join("loop-store"),
            scope: scope(),
        },
        expected_epoch: axon_loop_contracts::AuthorityEpoch::new(epoch).unwrap(),
        workspace: root.join("ws"),
        state_dir: root.join("fabric-state"),
        budget: axon_fabric::ResourceVector {
            model_micro_usd: 1_000,
            exec_ms: 1_000_000,
            verify_ms: 1_000_000,
            retries: 100,
        },
        grants: axon_fabric::GrantRegistry::load(&root.join("grants/grants.json")).unwrap(),
        linux: None,
        protected_host: None,
        observer: None,
        pre_launch_hook: None,
        fault_hook: Some(die_at_the_named_boundary),
    }
}

fn die_at_the_named_boundary(b: Boundary) {
    if std::env::var("FABRIC_RESTART_AT").ok().as_deref() == Some(b.name()) {
        // A real, abrupt process death: no unwinding, no destructors, no flush.
        std::process::abort();
    }
}

/// The crash child: submits the request, dying at `FABRIC_RESTART_AT`.
#[test]
#[ignore = "driven as a subprocess by crash_submit_at"]
fn restart_child() {
    let root = PathBuf::from(std::env::var("FABRIC_RESTART_ROOT").unwrap());
    let req = std::fs::read_to_string(root.join("req.json")).unwrap();
    let _ = submit(&req, &cfg_at(&root, 0));
    // Reaching here means the boundary was never hit: the parent asserts the abort.
}

/// Run a real submitting process that dies at `boundary`. Returns its exit status.
fn crash_submit_at(env: &Env, req: &serde_json::Value, boundary: Boundary) {
    std::fs::write(env.dir.path().join("req.json"), req.to_string()).unwrap();
    let st = Command::new(std::env::current_exe().unwrap())
        .args(["restart_child", "--exact", "--ignored", "--test-threads=1"])
        .env("FABRIC_RESTART_ROOT", env.dir.path())
        .env("FABRIC_RESTART_AT", boundary.name())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        st.signal(),
        Some(libc::SIGABRT),
        "the child did not die at {} ({st:?})",
        boundary.name()
    );
}

fn held(env: &Env) -> u64 {
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    j.scope_usage(&scope()).unwrap().held.model_micro_usd
}

/// Died after intent / after reserve: nothing launched, so the restart RESUMES
/// the op to completion — its first execution — and a further re-submit
/// replays it without running anything. Before this change the re-submit said
/// "in flight (another submit owns it)" forever, with the budget held.
#[test]
fn a_pre_launch_crash_is_resumed_once_and_never_repeated() {
    for boundary in [Boundary::AfterIntent, Boundary::AfterReserve] {
        let env = Env::new();
        let o = format!("op-{}", boundary.name().replace('_', "-"));
        let req = request(&env, &o, "t_ok");
        crash_submit_at(&env, &req, boundary);
        assert_eq!(
            spawn_count(&env.spawns),
            0,
            "{}: nothing may have run",
            boundary.name()
        );
        assert_eq!(env.launch_records(), 0, "{}", boundary.name());
        let (j, rep) = axon_fabric::Journal::open(&env.journal).unwrap();
        let pending = if boundary == Boundary::AfterIntent {
            &rep.pending_intended
        } else {
            &rep.pending_reserved
        };
        assert_eq!(
            pending,
            &vec![op(&o)],
            "{}: the orphan is explicitly pending",
            boundary.name()
        );
        drop(j);

        let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
        assert_eq!(
            s.receipt.status,
            ReceiptStatus::Completed,
            "{}",
            boundary.name()
        );
        assert_eq!(
            s.receipt.verification,
            ReceiptVerification::Passed,
            "{}",
            boundary.name()
        );
        assert!(
            !s.replayed,
            "{}: a resumed orphan RUNS; it is not a replay",
            boundary.name()
        );
        assert_eq!(
            spawn_count(&env.spawns),
            1,
            "{}: executed exactly once",
            boundary.name()
        );
        assert_eq!(env.launch_records(), 1, "{}", boundary.name());
        assert_eq!(
            held(&env),
            0,
            "{}: nothing left held after the resume",
            boundary.name()
        );

        let again = submit(&req.to_string(), &env.cfg(0)).unwrap();
        assert!(again.replayed, "{}", boundary.name());
        assert_eq!(again.receipt, s.receipt, "{}", boundary.name());
        assert_eq!(
            spawn_count(&env.spawns),
            1,
            "{}: never repeated",
            boundary.name()
        );
    }
}

/// Died after the launch record, before the effect: whether the effect ran is
/// not knowable from the journal, so it is OutcomeUnknown with the liability
/// kept — and it is NEVER re-run, even though (here) it provably did not run.
#[test]
fn a_crash_after_the_launch_record_is_unknown_never_rerun() {
    let env = Env::new();
    let req = request(&env, "op-launched", "t_ok");
    crash_submit_at(&env, &req, Boundary::AfterLaunchRecord);
    assert_eq!(env.launch_records(), 1);
    let (j, rep) = axon_fabric::Journal::open(&env.journal).unwrap();
    assert_eq!(rep.reconciled_unknown, vec![op("op-launched")]);
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(
        u.liability.model_micro_usd, 100,
        "liability kept, never zero"
    );
    assert_eq!(u.held.model_micro_usd, 0);
    drop(j);
    for _ in 0..2 {
        let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
        assert!(s.replayed);
        assert_eq!(s.receipt.status, ReceiptStatus::OutcomeUnknown);
        assert_eq!(s.receipt.unresolved_liability_micro, 100);
    }
    assert_eq!(spawn_count(&env.spawns), 0, "a launched op is never re-run");
}

/// Died after the terminal record, before the receipt: the op ran once and is
/// reported as outcome_unknown ("completed but no receipt recorded") — an
/// explicit gap, never a fabricated receipt and never a second run.
#[test]
fn a_crash_between_terminal_and_receipt_is_explicit_and_never_rerun() {
    let env = Env::new();
    let req = request(&env, "op-terminal", "t_ok");
    crash_submit_at(&env, &req, Boundary::AfterTerminal);
    assert_eq!(spawn_count(&env.spawns), 1);
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    assert_eq!(
        j.view(&op("op-terminal")).unwrap().state,
        OpState::Completed
    );
    assert!(j.view(&op("op-terminal")).unwrap().outcome.is_none());
    drop(j);
    let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert!(s.replayed);
    assert_eq!(s.receipt.status, ReceiptStatus::OutcomeUnknown);
    assert!(s.reason.is_none() || s.reason.as_deref() != Some(""));
    assert_eq!(spawn_count(&env.spawns), 1, "never re-run");
}

/// A pre-launch orphan cannot be adopted under a DIFFERENT authority epoch:
/// it is cancelled (released — nothing ran) and the re-submit is refused as
/// stale, rather than running old work under new authority.
#[test]
fn an_orphan_is_not_resumed_under_a_superseded_epoch() {
    let env = Env::new();
    let req = request(&env, "op-stale-orphan", "t_ok");
    crash_submit_at(&env, &req, Boundary::AfterReserve);
    assert_eq!(held(&env), 100);
    let now = env.bump_epoch();
    let e = match submit(&req.to_string(), &env.cfg(now)) {
        Ok(s) => panic!(
            "ATTACK: an orphan intended under a superseded epoch was resumed under the new one: \
             {:?}",
            s.receipt.status
        ),
        Err(e) => e,
    };
    assert!(matches!(e, SubmitError::StaleEpoch { .. }), "{e}");
    assert_eq!(held(&env), 0, "the orphan's reservation is released");
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    assert_eq!(
        j.view(&op("op-stale-orphan")).unwrap().state,
        OpState::Cancelled
    );
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

/// A crash INSIDE `Branches::cancel` leaves the cancelled marker written and a
/// branch op still Reserved (the exact on-disk state of that window, made here
/// by a real crash of the op's own submit after reserve, then the marker).
/// Recovery converges from either side: a re-submit of the orphan releases it
/// and is refused (the branch is cancelled), and re-running the cancel is
/// idempotent on the marker and finishes the job.
#[test]
fn a_crash_inside_branch_cancellation_converges_on_restart() {
    use axon_fabric::branches::Branches;
    use axon_loop_contracts::{ArmId, OpaqueRef, TaskId};
    let env = Env::new();
    let d = env.dir.path().join("tree-base");
    std::fs::create_dir_all(&d).unwrap();
    std::fs::write(d.join("f.ax"), FIXTURE).unwrap();
    let tenant = axon_loop_contracts::TenantId::new("tenant-t").unwrap();
    let base = axon_fabric::workspace::WorkspaceStore::open(&env.cfg(0).state_dir, &tenant)
        .unwrap()
        .import_dir(&d, &axon_fabric::workspace::Quota::default())
        .unwrap();
    let br = Branches::open(&env.cfg(0).state_dir, &scope());
    let exp = TaskId::new("exp-r").unwrap();
    let arm = ArmId::new("challenger-1").unwrap();
    br.open_experiment(
        &exp,
        &base,
        axon_fabric::ResourceVector {
            model_micro_usd: 1_000,
            exec_ms: 100_000,
            verify_ms: 0,
            retries: 0,
        },
        &[
            (
                ArmId::new("incumbent").unwrap(),
                OpaqueRef::new("writer:a").unwrap(),
            ),
            (arm.clone(), OpaqueRef::new("writer:b").unwrap()),
        ],
        &[OpaqueRef::new("approver:independent").unwrap()],
    )
    .unwrap();
    let run = br
        .experiment(&exp)
        .unwrap()
        .branch(&arm)
        .unwrap()
        .run_id
        .clone();
    let mut req = request(&env, "op-branch-orphan", "t_ok");
    req["trial_id"] = run.as_str().into();
    req["workspace_version_ref"] = base.as_str().into();
    crash_submit_at(&env, &req, Boundary::AfterReserve);
    assert_eq!(held(&env), 100);

    // The cancel crashed after its marker: write exactly that.
    let marker_file = walk_arm_dir(&env.cfg(0).state_dir, arm.as_str()).join("cancelled.json");
    std::fs::write(&marker_file, br#"{"reason":"crashed mid-cancel"}"#).unwrap();
    assert!(br.is_cancelled(&exp, &arm));
    assert_eq!(
        held(&env),
        100,
        "the crash window: marker written, op still held"
    );

    // Recovery 1: re-submitting the orphan releases it and is refused.
    let e = submit(&req.to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(e, SubmitError::Branch(_)), "{e}");
    assert_eq!(held(&env), 0, "the orphan's reservation is released");
    // Recovery 2: re-running the cancel is idempotent and changes nothing now.
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    let done = br.cancel(&j, &exp, &arm, "retry after crash").unwrap();
    assert!(done.is_empty(), "{done:?}");
    assert_eq!(
        j.view(&op("op-branch-orphan")).unwrap().state,
        OpState::Cancelled
    );
    assert_eq!(spawn_count(&env.spawns), 0);
}

/// The arm's directory under the branches root (its layout is private to the
/// crate; located by name).
fn walk_arm_dir(root: &Path, arm: &str) -> PathBuf {
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                if p.file_name().and_then(|n| n.to_str()) == Some(arm) {
                    return p;
                }
                stack.push(p);
            }
        }
    }
    panic!("no arm dir {arm} under {}", root.display())
}
