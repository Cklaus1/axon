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
    Begin, Billing, InputDigest, Intent, Journal, JournalError, OpKey, OpState, ResourceVector,
    ScopeKey, Settlement,
};

fn scope() -> ScopeKey {
    ScopeKey::new("task-1").unwrap()
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
        op: OpKey::new(op).unwrap(),
        input_digest: InputDigest::of(input),
        config: serde_json::json!({"profile": "process_scoped", "timeout_ms": 5000}),
        authority_ref: "grant:g-1".into(),
        scope: scope(),
        reservation: res(exec_ms),
        expected_version: 7,
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
        let o = OpKey::new(op).unwrap();
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
    let mut child = Command::new(std::env::current_exe().unwrap())
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
    let launched = OpKey::new("op-launched").unwrap();
    assert_eq!(rep.reconciled_unknown, vec![launched.clone()]);
    // The child recorded one op at each boundary before being killed.
    assert_eq!(
        rep.pending_intended,
        vec![OpKey::new("op-intended").unwrap()]
    );
    assert_eq!(
        rep.pending_reserved,
        vec![OpKey::new("op-reserved").unwrap()],
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
    assert!(matches!(
        j.complete(&launched, Billing::Unknown),
        Err(JournalError::InvalidTransition { .. })
    ));
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
    assert_eq!(
        rep.pending_intended,
        vec![OpKey::new("op-intended").unwrap()]
    );
    assert!(rep.reconciled_unknown.is_empty());
    assert_eq!(
        j.scope_usage(&scope()).unwrap().committed(),
        ResourceVector::default()
    );
    // Resumable: reserve + launch from here is legal.
    let o = OpKey::new("op-intended").unwrap();
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
    assert_eq!(
        rep.pending_reserved,
        vec![OpKey::new("op-reserved").unwrap()]
    );
    assert_eq!(j.scope_usage(&scope()).unwrap().held.exec_ms, 20);
    // Cancelling a never-launched op RELEASES it (no effect was dispatched).
    j.cancel(&OpKey::new("op-reserved").unwrap(), "operator", None)
        .unwrap();
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.committed(), ResourceVector::default(), "released: {u:?}");
    // A launched op cannot be released that way (tested elsewhere); a released
    // one cannot be relaunched.
    assert!(j
        .mark_launched(&OpKey::new("op-reserved").unwrap())
        .is_err());
}

#[test]
fn a_torn_final_line_is_truncated_and_everything_before_it_survives() {
    let dir = tempfile::tempdir().unwrap();
    let path = {
        let j = fresh(dir.path());
        j.begin(intent("op-a", b"a", 10)).unwrap();
        j.reserve(&OpKey::new("op-a").unwrap()).unwrap();
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
    assert_eq!(
        j.view(&OpKey::new("op-a").unwrap()).unwrap().state,
        OpState::Reserved
    );
    // And the journal is appendable again.
    j.mark_launched(&OpKey::new("op-a").unwrap()).unwrap();
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
    assert!(matches!(
        Journal::open(&path),
        Err(JournalError::Corrupt { .. })
    ));
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

    let err = j.begin(intent("op-a", b"input-2", 10)).unwrap_err();
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
    assert_eq!(
        last["intent"]["input_digest"],
        InputDigest::of(b"a").as_str()
    );
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
                match j.reserve(&OpKey::new(op).unwrap()) {
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

#[test]
fn every_dimension_is_a_ceiling() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let mut i = intent("op-model", b"m", 1);
    i.reservation.model_micro_usd = 1_001; // over the model ceiling only
    j.begin(i).unwrap();
    assert!(matches!(
        j.reserve(&OpKey::new("op-model").unwrap()),
        Err(JournalError::BudgetExceeded { .. })
    ));
}

#[test]
fn failed_and_cancelled_work_is_charged_or_held_never_dropped() {
    let dir = tempfile::tempdir().unwrap();
    let j = fresh(dir.path());
    let run = |op: &str| {
        let o = OpKey::new(op).unwrap();
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
        j.reserve(&OpKey::new("op-big").unwrap()),
        Err(JournalError::BudgetExceeded { .. })
    ));

    // Settling a liability with evidence converts it to a charge; the op's
    // STATE is unchanged.
    j.settle(&b, Settlement { actual: res(3) }).unwrap();
    assert_eq!(j.view(&b).unwrap().state, OpState::Failed);
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.liability.exec_ms, 20);
    assert_eq!(u.charged.exec_ms, 10);
    // A known-cost op cannot be "settled" again.
    assert!(j.settle(&a, Settlement { actual: res(0) }).is_err());
}

#[test]
fn an_unknown_outcome_can_be_settled_but_not_completed() {
    let dir = tempfile::tempdir().unwrap();
    let path = crash_at(dir.path(), "launched");
    let (j, _) = Journal::open(&path).unwrap();
    let o = OpKey::new("op-launched").unwrap();
    j.settle(&o, Settlement { actual: res(4) }).unwrap();
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
    i.scope = ScopeKey::new("nope").unwrap();
    assert!(matches!(j.begin(i), Err(JournalError::UnknownScope(_))));
    j.declare_budget(&scope(), ceiling()).unwrap(); // same: idempotent
    let mut bigger = ceiling();
    bigger.exec_ms += 1;
    assert!(matches!(
        j.declare_budget(&scope(), bigger),
        Err(JournalError::ScopeConflict { .. })
    ));
}
