//! B260 (first half) — durable operation journal and aggregate reservations.
//!
//! G13-r22-journal-before-effect, G13-r22-aggregate-reservation and
//! G13-r22-unknown-reconcile, against the real file store. The restart cases
//! use a REAL second process: this test binary re-executes itself as a child
//! (`crash_child`, ignored in normal runs), the child writes to the journal up
//! to a named boundary, prints READY, and is killed with SIGKILL by the
//! parent, which then reopens the same file.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;

use axon_fabric::{
    Begin, Billing, Intent, Journal, JournalError, OpState, ResourceVector, Settlement,
};
use axon_loop_contracts::{
    AttemptId, AuthorityEpoch, OperationId, Ref, Scope, TaskFamily, TaskId, TenantId, TrialId,
};

fn opid(s: impl Into<String>) -> OperationId {
    OperationId::new(s).unwrap()
}

fn digest_of(bytes: &[u8]) -> Ref {
    use sha2::{Digest, Sha256};
    Ref::new(format!("sha256:{:x}", Sha256::digest(bytes))).unwrap()
}

fn scope() -> Scope {
    Scope {
        tenant_id: TenantId::new("tenant-1").unwrap(),
        task_family: TaskFamily::new("task-1").unwrap(),
    }
}

fn ceiling() -> ResourceVector {
    ResourceVector {
        model_micro_usd: 1_000,
        exec_ms: 100,
        verify_ms: 100,
        retries: 10,
    }
}

fn res(exec_ms: u64) -> ResourceVector {
    ResourceVector {
        model_micro_usd: 10,
        exec_ms,
        verify_ms: 1,
        retries: 0,
    }
}

fn intent(op: &str, input: &[u8], exec_ms: u64) -> Intent {
    Intent {
        op: OperationId::new(op).unwrap(),
        task_id: TaskId::new("task-1").unwrap(),
        trial_id: TrialId::new("trial-1").unwrap(),
        attempt_id: AttemptId::new("attempt-1").unwrap(),
        input_digest: digest_of(input),
        authority_epoch: AuthorityEpoch::new(3).unwrap(),
        config: serde_json::json!({"profile": "process_scoped", "timeout_ms": 5000}),
        authority_ref: "grant:g-1".into(),
        scope: scope(),
        reservation: res(exec_ms),
        expected_version: 7,
    }
}

fn receipt(origin: &str, sequence: u64, actual: ResourceVector) -> Settlement {
    Settlement {
        origin: origin.into(),
        sequence,
        actual,
    }
}

fn fresh(dir: &Path) -> Journal {
    let (j, rep) = Journal::open(dir.join("ops.journal")).unwrap();
    assert!(rep.reconciled_unknown.is_empty());
    j.declare_budget(&scope(), ceiling()).unwrap();
    j
}

// ── the child side of the restart tests ─────────────────────────────────────

/// Not a test on its own: the body the restart tests run in a CHILD process.
#[test]
#[ignore = "child-process helper for the SIGKILL restart tests; run by them, not directly"]
fn crash_child() {
    let path = PathBuf::from(std::env::var("FABRIC_CHILD_JOURNAL").unwrap());
    let stage = std::env::var("FABRIC_CHILD_STAGE").unwrap();
    let (j, _) = Journal::open(&path).unwrap();
    j.declare_budget(&scope(), ceiling()).unwrap();
    for (op, upto) in [
        ("op-intended", "intent"),
        ("op-reserved", "reserved"),
        ("op-launched", "launched"),
    ] {
        let o = opid(op);
        j.begin(intent(op, op.as_bytes(), 20)).unwrap();
        if upto != "intent" {
            j.reserve(&o).unwrap();
        }
        if upto == "launched" {
            j.mark_launched(&o).unwrap();
        }
        if upto == stage {
            break;
        }
    }
    println!("READY");
    std::io::stdout().flush().unwrap();
    // Wait to be killed. Never returns normally within the parent's timeout.
    std::thread::sleep(std::time::Duration::from_secs(120));
    panic!("the parent should have killed this child");
}

/// Run `crash_child` in a real child process up to `stage`, then SIGKILL it.
fn crash_at(dir: &Path, stage: &str) -> PathBuf {
    let path = dir.join("ops.journal");
    let mut child = Command::new(axon_fabric::readiness::running_image())
        .args([
            "crash_child",
            "--exact",
            "--ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env("FABRIC_CHILD_JOURNAL", &path)
        .env("FABRIC_CHILD_STAGE", stage)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let pid = child.id();
    // libtest prints `test crash_child ... ` without a newline before the
    // body runs, so the marker arrives at the END of a line, not alone on one.
    let mut lines = BufReader::new(child.stdout.take().unwrap()).lines();
    let ready = lines.any(|l| l.map(|l| l.trim_end().ends_with("READY")).unwrap_or(false));
    if !ready {
        let _ = child.kill();
        let _ = child.wait();
        panic!("child never reached {stage}");
    }
    // Child::kill is SIGKILL on unix: no destructors, no flush, no unlock.
    child.kill().unwrap();
    let status = child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(libc_sigkill()),
        "child {pid} must die by SIGKILL"
    );
    path
}

fn libc_sigkill() -> i32 {
    9
}

// ── restart / reconciliation ────────────────────────────────────────────────

#[test]
fn sigkill_after_launch_reconciles_to_outcome_unknown_with_liability_kept() {
    let dir = tempfile::tempdir().unwrap();
    let path = crash_at(dir.path(), "launched");

    let (j, rep) = Journal::open(&path).unwrap();
    let launched = opid("op-launched");
    assert_eq!(rep.reconciled_unknown, vec![launched.clone()]);
    // The child recorded one op at each boundary before being killed.
    assert_eq!(rep.pending_intended, vec![opid("op-intended")]);
    assert_eq!(
        rep.pending_reserved,
        vec![opid("op-reserved")],
        "reserved-but-never-launched stays reserved: no effect, budget held"
    );
    let v = j.view(&launched).unwrap();
    assert_eq!(v.state, OpState::OutcomeUnknown);
    assert!(v.launched);
    assert_eq!(v.billing, Some(Billing::Unknown));

    // Liability kept: one op held (reserved), one op unknown (liability).
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.held.exec_ms, 20);
    assert_eq!(u.liability.exec_ms, 20);
    assert_eq!(
        u.committed().exec_ms,
        40,
        "a restart must not refund anything"
    );

    // Never silently re-executed: the op cannot be relaunched or completed.
    assert!(matches!(
        j.mark_launched(&launched),
        Err(JournalError::InvalidTransition { .. })
    ));
    assert!(
        matches!(
            j.complete(&launched, Billing::Unknown),
            Err(JournalError::InvalidTransition { .. })
        ),
        "ATTACK: an op whose outcome is unknown was completed"
    );
    // Re-submitting the identical request reports the recorded state rather
    // than starting over.
    match j.begin(intent("op-launched", b"op-launched", 20)).unwrap() {
        Begin::AlreadyRecorded(v) => assert_eq!(v.state, OpState::OutcomeUnknown),
        other => panic!("{other:?}"),
    }

    // The reconciliation itself is durable: a second reopen finds nothing new.
    drop(j);
    let (j2, rep2) = Journal::open(&path).unwrap();
    assert!(rep2.reconciled_unknown.is_empty());
    assert_eq!(j2.view(&launched).unwrap().state, OpState::OutcomeUnknown);
    assert_eq!(j2.scope_usage(&scope()).unwrap().committed().exec_ms, 40);
}

#[test]
fn sigkill_after_intent_only_leaves_an_intended_op_with_no_reservation() {
    let dir = tempfile::tempdir().unwrap();
    let path = crash_at(dir.path(), "intent");
    let (j, rep) = Journal::open(&path).unwrap();
    assert_eq!(rep.pending_intended, vec![opid("op-intended")]);
    assert!(rep.reconciled_unknown.is_empty());
    assert_eq!(
        j.scope_usage(&scope()).unwrap().committed(),
        ResourceVector::default()
    );
    // Resumable: reserve + launch from here is legal.
    let o = opid("op-intended");
    j.reserve(&o).unwrap();
    j.mark_launched(&o).unwrap();
    j.complete(&o, Billing::Known(res(5))).unwrap();
    assert_eq!(j.scope_usage(&scope()).unwrap().charged.exec_ms, 5);
}

#[test]
fn sigkill_after_reserve_keeps_the_budget_held() {
    let dir = tempfile::tempdir().unwrap();
    let path = crash_at(dir.path(), "reserved");
    let (j, rep) = Journal::open(&path).unwrap();
    assert_eq!(rep.pending_reserved, vec![opid("op-reserved")]);
    assert_eq!(j.scope_usage(&scope()).unwrap().held.exec_ms, 20);
    // Cancelling a never-launched op RELEASES it (no effect was dispatched).
    j.cancel(&opid("op-reserved"), "operator", None).unwrap();
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.committed(), ResourceVector::default(), "released: {u:?}");
    // A launched op cannot be released that way (tested elsewhere); a released
    // one cannot be relaunched.
    assert!(j.mark_launched(&opid("op-reserved")).is_err());
}

#[test]
fn a_torn_final_line_is_truncated_and_everything_before_it_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = {
        let j = fresh(dir.path());
        j.begin(intent("op-a", b"a", 10)).unwrap();
        j.reserve(&opid("op-a")).unwrap();
        j.path().to_path_buf()
    };
    let before = std::fs::metadata(&path).unwrap().len();
    // A crash in the middle of write_all: a prefix of a record, no newline.
    let torn: &[u8] = b"{\"seq\":99,\"kind\":\"launch";
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(torn)
        .unwrap();
    let (j, rep) = Journal::open(&path).unwrap();
    assert_eq!(rep.torn_tail_bytes, torn.len() as u64);
    assert_eq!(std::fs::metadata(&path).unwrap().len(), before);
    assert_eq!(j.view(&opid("op-a")).unwrap().state, OpState::Reserved);
    // And the journal is appendable again.
    j.mark_launched(&opid("op-a")).unwrap();
}

#[test]
fn a_corrupt_interior_record_refuses_the_whole_journal() {
    let dir = tempfile::tempdir().unwrap();
    let path = {
        let j = fresh(dir.path());
        j.begin(intent("op-a", b"a", 10)).unwrap();
        j.path().to_path_buf()
    };
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<&str> = text.lines().collect();
    lines[1] = "{\"seq\":2,\"kind\":\"garbage\"}";
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    match Journal::open(&path) {
        Err(JournalError::Corrupt { line: 2, .. }) => {}
        Err(e) => panic!("wrong error {e}"),
        Ok(_) => panic!("a corrupt journal must not open"),
    }
}

#[test]
fn a_record_that_violates_the_state_machine_is_corruption() {
    // e.g. an appended `launched` for an op that was never reserved.
    let dir = tempfile::tempdir().unwrap();
    let path = {
        let j = fresh(dir.path());
        j.begin(intent("op-a", b"a", 10)).unwrap();
        j.path().to_path_buf()
    };
    let n = std::fs::read_to_string(&path).unwrap().lines().count();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(
            format!(
                "{{\"seq\":{},\"kind\":\"launched\",\"op\":\"op-a\"}}\n",
                n + 1
            )
            .as_bytes(),
        )
        .unwrap();
    assert!(
        matches!(Journal::open(&path), Err(JournalError::Corrupt { .. })),
        "ATTACK: a launched record for an op that was never reserved was accepted"
    );
}

#[test]
fn a_second_writer_is_locked_out() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    match Journal::open(j.path()) {
        Err(JournalError::Locked(_)) => {}
        Err(e) => panic!("wrong error {e}"),
        Ok(_) => panic!("two writers on one journal"),
    }
}

/// The flock belongs to the open file description, which a fork shares with its
/// child until that child execs. Here a forked child holds the lock's
/// description for 1.5 s (a loaded host's fork-to-exec window, longer than the
/// old 500 ms retry) after the parent dropped its journal: reopening must wait
/// the holder out, not report `Locked` for a holder that is about to let go.
/// Control: `a_second_writer_is_locked_out` (a holder that persists).
#[test]
fn a_lock_held_only_by_a_forks_inherited_description_is_waited_out() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let path = j.path().to_path_buf();
    // SAFETY: the child calls only async-signal-safe functions (usleep, _exit).
    let pid = unsafe { libc::fork() };
    if pid == 0 {
        unsafe {
            libc::usleep(1_500_000);
            libc::_exit(0);
        }
    }
    assert!(pid > 0, "setup: fork failed");
    drop(j);
    let r = Journal::open(&path);
    unsafe {
        libc::waitpid(pid, std::ptr::null_mut(), 0);
    }
    if let Err(JournalError::Locked(_)) = r {
        panic!(
            "ATTACK: a reopen was refused Locked by a holder that lasted only a loaded host's \
             fork-to-exec window"
        );
    }
    assert!(r.is_ok(), "setup: the reopen failed for another reason");
}

// ── identity / conflict ─────────────────────────────────────────────────────

#[test]
fn same_operation_id_with_a_different_input_digest_is_a_conflict() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    assert_eq!(
        j.begin(intent("op-a", b"input-1", 10)).unwrap(),
        Begin::Recorded
    );
    let len = j.len();

    let err = match j.begin(intent("op-a", b"input-2", 10)) {
        Err(e) => e,
        Ok(b) => panic!("ATTACK: the same operation id with a different input was accepted: {b:?}"),
    };
    assert!(matches!(err, JournalError::Conflict { .. }), "{err}");
    // Any other field of the immutable request conflicts too.
    let mut other = intent("op-a", b"input-1", 10);
    other.expected_version = 8;
    assert!(matches!(j.begin(other), Err(JournalError::Conflict { .. })));
    assert_eq!(j.len(), len, "a refused begin writes nothing");

    // Identical replay is idempotent.
    match j.begin(intent("op-a", b"input-1", 10)).unwrap() {
        Begin::AlreadyRecorded(v) => assert_eq!(v.state, OpState::Intended),
        other => panic!("{other:?}"),
    }
    assert_eq!(j.len(), len);

    // The conflict survives a restart (the binding is durable).
    let p = j.path().to_path_buf();
    drop(j);
    let (j, _) = Journal::open(&p).unwrap();
    assert!(matches!(
        j.begin(intent("op-a", b"input-2", 10)),
        Err(JournalError::Conflict { .. })
    ));
}

#[test]
fn intent_is_on_disk_before_begin_returns() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    j.begin(intent("op-a", b"a", 10)).unwrap();
    let text = std::fs::read_to_string(j.path()).unwrap();
    let last: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert_eq!(last["kind"], "intent");
    assert_eq!(last["intent"]["op"], "op-a");
    assert_eq!(last["intent"]["input_digest"], digest_of(b"a").as_str());
    assert_eq!(last["intent"]["authority_ref"], "grant:g-1");
    assert_eq!(last["intent"]["expected_version"], 7);
    assert_eq!(last["intent"]["reservation"]["exec_ms"], 10);
}

// ── aggregate reservations ──────────────────────────────────────────────────

#[test]
fn concurrent_reservations_cannot_overspend() {
    let dir = tempfile::tempdir().unwrap();
    let j = Arc::new(fresh(dir.path()));
    // Ceiling exec_ms = 100, each op wants 10: exactly 10 of 40 can succeed.
    let handles: Vec<_> = (0..40)
        .map(|i| {
            let j = Arc::clone(&j);
            std::thread::spawn(move || {
                let op = format!("op-{i}");
                j.begin(intent(&op, op.as_bytes(), 10)).unwrap();
                match j.reserve(&opid(op)) {
                    Ok(()) => true,
                    Err(JournalError::BudgetExceeded { .. }) => false,
                    Err(e) => panic!("{e}"),
                }
            })
        })
        .collect();
    let won = handles
        .into_iter()
        .map(|h| h.join().unwrap())
        .filter(|w| *w)
        .count();
    assert_eq!(won, 10);
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.held.exec_ms, 100);
    assert!(u.committed().exec_ms <= ceiling().exec_ms);

    // Rebuilt identically from disk.
    let p = j.path().to_path_buf();
    drop(j);
    let (j, _) = Journal::open(&p).unwrap();
    assert_eq!(j.scope_usage(&scope()).unwrap().held.exec_ms, 100);
}

/// D-C5: the committed ≤ ceiling check is axon-os `ResourceLedger::carve`,
/// and the refusal names the dimension carve refused — for EACH of the four.
/// A refused reserve writes nothing.
#[test]
fn each_dimension_is_carved_through_the_axon_os_ledger() {
    for (dim, set) in [
        (
            "model_micro_usd",
            (|r: &mut ResourceVector| r.model_micro_usd = 1_001) as fn(&mut _),
        ),
        ("exec_ms", |r: &mut ResourceVector| r.exec_ms = 101),
        ("verify_ms", |r: &mut ResourceVector| r.verify_ms = 101),
        ("retries", |r: &mut ResourceVector| r.retries = 11),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let j = fresh(dir.path());
        let mut i = intent("op-over", dim.as_bytes(), 1);
        set(&mut i.reservation);
        j.begin(i).unwrap();
        let before = j.len();
        match j.reserve(&opid("op-over")) {
            Err(JournalError::BudgetExceeded { dimension, .. }) => {
                assert_eq!(dimension, dim, "carve's refused axis, by name")
            }
            other => panic!("{dim}: {other:?}"),
        }
        assert_eq!(j.len(), before, "{dim}: nothing written");
        assert_eq!(j.view(&opid("op-over")).unwrap().state, OpState::Intended);
    }
}

/// A parallel `used + want > cap` comparison and `carve` are MEANT to agree,
/// so behaviour cannot tell them apart; the structure is what D-C5 is about.
/// The reservation path must call `ResourceLedger::carve`, and the old
/// parallel `fits_within` must not come back.
#[test]
fn the_reservation_check_is_resource_ledger_carve() {
    let src = include_str!("../src/journal.rs");
    let body = &src[src.find("fn carve_within(").unwrap()..src.find("fn saturating_add(").unwrap()];
    assert!(
        body.contains("ResourceLedger::new(") && body.contains(".carve(Carve {"),
        "carve_within must decide through axon-os ResourceLedger::carve:\n{body}"
    );
    assert!(
        !src.contains("fn fits_within"),
        "no parallel ceiling algebra"
    );
    let reserved = &src[src.find("Rec::Reserved { op } =>").unwrap()..];
    let reserved = &reserved[..reserved.find("Rec::Launched").unwrap()];
    assert!(reserved.contains("carve_within("), "{reserved}");
}

/// `ResourceLedger::carve` alone would ADMIT an overflowing carve at a cap of
/// u64::MAX (its check saturates) and then overflow its unchecked add. The
/// journal refuses it, with nothing written and no panic.
#[test]
fn an_overflowing_reservation_is_refused_not_wrapped() {
    let dir = tempfile::tempdir().unwrap();
    let (j, _) = Journal::open(dir.path().join("j")).unwrap();
    let max = ResourceVector {
        model_micro_usd: u64::MAX,
        exec_ms: u64::MAX,
        verify_ms: u64::MAX,
        retries: u64::MAX,
    };
    j.declare_budget(&scope(), max).unwrap();
    let mut a = intent("op-a", b"a", 0);
    a.reservation.exec_ms = u64::MAX;
    j.begin(a).unwrap();
    j.reserve(&opid("op-a")).unwrap();
    let mut b = intent("op-b", b"b", 0);
    b.reservation.exec_ms = 1;
    j.begin(b).unwrap();
    let before = j.len();
    assert!(matches!(
        j.reserve(&opid("op-b")),
        Err(JournalError::BudgetExceeded {
            dimension: "exec_ms",
            ..
        })
    ));
    assert_eq!(j.len(), before, "nothing written");
    assert_eq!(j.scope_usage(&scope()).unwrap().held.exec_ms, u64::MAX);
}

#[test]
fn every_dimension_is_a_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let mut i = intent("op-model", b"m", 1);
    i.reservation.model_micro_usd = 1_001; // over the model ceiling only
    j.begin(i).unwrap();
    assert!(matches!(
        j.reserve(&opid("op-model")),
        Err(JournalError::BudgetExceeded { .. })
    ));
}

#[test]
fn failed_and_cancelled_work_is_charged_or_held_never_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let run = |op: &str| {
        let o = opid(op);
        j.begin(intent(op, op.as_bytes(), 20)).unwrap();
        j.reserve(&o).unwrap();
        j.mark_launched(&o).unwrap();
        o
    };

    // Failed with a known cost: charged that cost.
    let a = run("op-fail-known");
    j.fail(&a, "exit 1", Billing::Known(res(7))).unwrap();
    // Failed with an unknown cost: the whole reservation stays as liability.
    let b = run("op-fail-unknown");
    j.fail(&b, "timeout", Billing::Unknown).unwrap();
    // Cancelled after launch: billing is mandatory.
    let c = run("op-cancel-launched");
    assert!(matches!(
        j.cancel(&c, "operator", None),
        Err(JournalError::InvalidTransition { .. })
    ));
    j.cancel(&c, "operator", Some(Billing::Unknown)).unwrap();

    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.charged.exec_ms, 7);
    assert_eq!(u.liability.exec_ms, 40);
    assert_eq!(u.held.exec_ms, 0);
    assert_eq!(u.committed().exec_ms, 47);

    // Liability blocks new work exactly like a charge does: 47 + 60 > 100.
    j.begin(intent("op-big", b"big", 60)).unwrap();
    assert!(matches!(
        j.reserve(&opid("op-big")),
        Err(JournalError::BudgetExceeded { .. })
    ));

    // Settling a liability with evidence converts it to a charge; the op's
    // STATE is unchanged.
    j.settle(&b, receipt("meter-1", 1, res(3))).unwrap();
    assert_eq!(j.view(&b).unwrap().state, OpState::Failed);
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.liability.exec_ms, 20);
    assert_eq!(u.charged.exec_ms, 10);
    // A known-cost op cannot be "settled" again.
    assert!(
        j.settle(&a, receipt("meter-1", 2, res(0))).is_err(),
        "ATTACK: an op whose cost is known was settled"
    );
}

#[test]
fn an_unknown_outcome_can_be_settled_but_not_completed() {
    let dir = tempfile::tempdir().unwrap();
    let path = crash_at(dir.path(), "launched");
    let (j, _) = Journal::open(&path).unwrap();
    let o = opid("op-launched");
    j.settle(&o, receipt("meter-1", 1, res(4))).unwrap();
    let v = j.view(&o).unwrap();
    assert_eq!(
        v.state,
        OpState::OutcomeUnknown,
        "cost evidence is not outcome evidence"
    );
    assert_eq!(v.billing, Some(Billing::Known(res(4))));
}

#[test]
fn an_undeclared_scope_or_a_redeclared_ceiling_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let mut i = intent("op-x", b"x", 1);
    i.scope.task_family = TaskFamily::new("nope").unwrap();
    assert!(
        matches!(j.begin(i), Err(JournalError::UnknownScope(_))),
        "ATTACK: an intent in a scope no ceiling was declared for was accepted"
    );
    j.declare_budget(&scope(), ceiling()).unwrap(); // same: idempotent
    let mut bigger = ceiling();
    bigger.exec_ms += 1;
    assert!(matches!(
        j.declare_budget(&scope(), bigger),
        Err(JournalError::ScopeConflict { .. })
    ));
}

// ── G13-r22-billing-settlement ──────────────────────────────────────────────
//
// Duplicate accounting receipts are idempotent only under identical
// origin/sequence/content; unresolved usage stays unknown with its full
// reservation as liability.

/// An op launched with reservation `res(exec_ms)` that failed with an
/// UNKNOWN cost.
fn unknown_cost_op(j: &Journal, op: &str, exec_ms: u64) -> OperationId {
    let o = opid(op);
    j.begin(intent(op, op.as_bytes(), exec_ms)).unwrap();
    j.reserve(&o).unwrap();
    j.mark_launched(&o).unwrap();
    j.fail(&o, "timeout", Billing::Unknown).unwrap();
    o
}

#[test]
fn g13_unknown_billing_keeps_the_full_reservation_as_liability_never_zero() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ops.journal");
    {
        let j = fresh(dir.path());
        let o = unknown_cost_op(&j, "op-unknown", 30);
        assert_eq!(j.view(&o).unwrap().billing, Some(Billing::Unknown));
        let u = j.scope_usage(&scope()).unwrap();
        // EVERY dimension of the reservation is held, none of it charged.
        assert_eq!(u.liability, res(30));
        assert_eq!(u.charged, ResourceVector::default());
        assert_eq!(u.committed(), res(30));
        // 30 + 71 > 100: the unknown cost blocks work exactly as a charge
        // would. Were unknown treated as 0, this 71 would fit.
        j.begin(intent("op-next", b"next", 71)).unwrap();
        assert!(matches!(
            j.reserve(&opid("op-next")),
            Err(JournalError::BudgetExceeded { .. })
        ));
    }
    // The liability survives a reopen: it is replayed, not re-derived as 0.
    let (j, _) = Journal::open(&path).unwrap();
    assert_eq!(j.scope_usage(&scope()).unwrap().liability, res(30));
}

#[test]
fn g13_identical_settlement_receipt_is_idempotent() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ops.journal");
    {
        let j = fresh(dir.path());
        let o = unknown_cost_op(&j, "op-a", 20);
        assert_eq!(
            j.settle(&o, receipt("meter-1", 7, res(3))).unwrap(),
            axon_fabric::journal::Settle::Recorded
        );
        let n = j.len();
        // The identical receipt again: nothing written, same answer.
        assert_eq!(
            j.settle(&o, receipt("meter-1", 7, res(3))).unwrap(),
            axon_fabric::journal::Settle::AlreadySettled
        );
        assert_eq!(j.len(), n, "an identical duplicate writes nothing");
        let v = j.view(&o).unwrap();
        assert!(!v.disputed());
        assert_eq!(v.billing, Some(Billing::Known(res(3))));
        assert_eq!(j.scope_usage(&scope()).unwrap().charged, res(3));
    }
    let (j, _) = Journal::open(&path).unwrap();
    assert_eq!(
        j.settle(&opid("op-a"), receipt("meter-1", 7, res(3)))
            .unwrap(),
        axon_fabric::journal::Settle::AlreadySettled,
        "idempotence holds across a reopen"
    );
}

#[test]
fn g13_duplicate_receipt_with_different_content_is_refused_and_recorded() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ops.journal");
    let a = opid("op-a");
    let b = opid("op-b");
    {
        let j = fresh(dir.path());
        unknown_cost_op(&j, "op-a", 20);
        unknown_cost_op(&j, "op-b", 10);
        j.settle(&a, receipt("meter-1", 7, res(3))).unwrap();

        // Same origin + sequence, different content.
        let n = j.len();
        assert!(matches!(
            j.settle(&a, receipt("meter-1", 7, res(50))),
            Err(JournalError::SettlementConflict { .. })
        ));
        assert_eq!(j.len(), n + 1, "the refused receipt is RECORDED");
        let v = j.view(&a).unwrap();
        assert!(v.disputed());
        assert_eq!(
            v.billing,
            Some(Billing::Unknown),
            "a dispute is not a known cost"
        );
        assert_eq!(v.settlement.as_ref().unwrap().actual, res(3));

        // A second, differently-numbered receipt for the same op is also a
        // conflict — it would double-count the op.
        assert!(matches!(
            j.settle(&a, receipt("meter-1", 8, res(3))),
            Err(JournalError::SettlementConflict { .. })
        ));
        // Reusing receipt meter-1#7 for ANOTHER op is a conflict too.
        assert!(matches!(
            j.settle(&b, receipt("meter-1", 7, res(3))),
            Err(JournalError::SettlementConflict { .. })
        ));
        let vb = j.view(&b).unwrap();
        assert!(vb.disputed());
        assert_eq!(vb.billing, Some(Billing::Unknown));
        assert!(vb.settlement.is_none());

        // The identical original receipt is still idempotent.
        let n = j.len();
        assert_eq!(
            j.settle(&a, receipt("meter-1", 7, res(3))).unwrap(),
            axon_fabric::journal::Settle::AlreadySettled
        );
        assert_eq!(j.len(), n);
    }
    // Conservative accounting, and it replays identically from disk: a is
    // held at max(reservation 20, claims 3, 50, 3) = 50 exec_ms; b at
    // max(reservation 10, claim 3) = 10. Nothing is "charged".
    let (j, _) = Journal::open(&path).unwrap();
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.charged, ResourceVector::default());
    assert_eq!(u.liability.exec_ms, 60);
    assert_eq!(j.view(&a).unwrap().disputes.len(), 2);
    assert_eq!(j.view(&b).unwrap().disputes.len(), 1);
}

#[test]
fn g13_settlement_without_origin_is_refused_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let o = unknown_cost_op(&j, "op-a", 20);
    let n = j.len();
    assert!(
        matches!(
            j.settle(&o, receipt("", 1, res(3))),
            Err(JournalError::InvalidSettlement(_))
        ),
        "ATTACK: a settlement receipt with no origin was accepted"
    );
    assert_eq!(j.len(), n);
    assert_eq!(j.view(&o).unwrap().billing, Some(Billing::Unknown));
}

#[test]
fn g13_a_journal_holding_a_duplicate_settlement_line_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ops.journal");
    {
        let j = fresh(dir.path());
        let o = unknown_cost_op(&j, "op-a", 20);
        j.settle(&o, receipt("meter-1", 7, res(3))).unwrap();
    }
    // Append a second copy of the settled line with the next sequence: the
    // live path never writes one, so the journal is refused, not trusted.
    let text = std::fs::read_to_string(&path).unwrap();
    let last = text.lines().last().unwrap();
    let mut v: serde_json::Value = serde_json::from_str(last).unwrap();
    assert_eq!(v["kind"], "settled");
    v["seq"] = serde_json::json!(v["seq"].as_u64().unwrap() + 1);
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap();
    writeln!(f, "{v}").unwrap();
    drop(f);
    assert!(matches!(
        Journal::open(&path),
        Err(JournalError::Corrupt { .. })
    ));
}

/// C9 dev review round 1 (the `operator()` stat class): only NotFound means
/// "no journal". A journal path that cannot be stat'ed (ENOTDIR, ELOOP) used
/// to read as "nothing recorded", so `status`/`cancel` answered for an op
/// they could not see instead of refusing. Control: an absent journal is
/// `None`.
#[test]
fn only_a_missing_journal_is_no_journal() {
    let d = tempfile::tempdir().unwrap();
    let file = d.path().join("file");
    std::fs::write(&file, "x").unwrap();
    let lp = d.path().join("loop");
    std::os::unix::fs::symlink(&lp, &lp).unwrap();
    for (what, p) in [
        ("ENOTDIR", file.join("ops.journal")),
        ("ELOOP", lp.join("ops.journal")),
    ] {
        let got = axon_fabric::Journal::open_unreconciled(&p);
        assert!(
            got.is_err(),
            "ATTACK: a journal that cannot be stat'ed ({what}) was read as no journal: {:?}",
            got.map(|j| j.is_some())
        );
    }
    assert!(
        axon_fabric::Journal::open_unreconciled(d.path().join("absent"))
            .unwrap()
            .is_none()
    );
}

/// C9 round 7, EQGATE3 (amendment 91): a journal whose sequence numbers skip
/// is refused. The sibling check (a duplicate settlement) was killed; this one
/// was exempted as "not a verdict property" and survived being removed: a gap
/// is a line dropped from the middle (an op's reservation, a cancel), which
/// would replay as a journal in which it never happened.
#[test]
fn g13_a_journal_with_a_gap_in_its_sequence_is_corrupt() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ops.journal");
    {
        let j = fresh(dir.path());
        let o = unknown_cost_op(&j, "op-a", 20);
        j.settle(&o, receipt("meter-1", 7, res(3))).unwrap();
    }
    Journal::open(&path).expect("control: the journal opens");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut lines: Vec<String> = text.lines().map(String::from).collect();
    let last = lines.len() - 1;
    let mut v: serde_json::Value = serde_json::from_str(&lines[last]).unwrap();
    v["seq"] = serde_json::json!(v["seq"].as_u64().unwrap() + 5);
    lines[last] = v.to_string();
    std::fs::write(&path, lines.join("\n") + "\n").unwrap();
    match Journal::open(&path) {
        Err(JournalError::Corrupt { reason, .. }) => {
            assert!(reason.contains("sequence"), "{reason}")
        }
        other => panic!(
            "ATTACK: a journal whose sequence skips was opened: {:?}",
            other.map(|_| ())
        ),
    }
}
