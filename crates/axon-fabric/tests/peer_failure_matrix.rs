//! B280 / G16-r22-peer-failure-matrix, Fabric side: peer outage, replayed
//! messages, stale epochs and schema mismatch preserve local authority and
//! unknown status, and reconnecting does not duplicate activation, billing or
//! task effects. The axon-loop half (partial export, replayed transitions,
//! episode schema) is in `axon-loop/tests/intake.rs`
//! (`a_partial_export_is_refused_then_the_complete_one_is_recorded_once`,
//! `an_episode_of_another_schema_version_is_refused`) and `axon-loop/tests/pointer.rs`
//! (`a_replayed_activation_after_authority_moved_does_not_reactivate`).
//!
//! "Peer" here is the authority peer Fabric depends on at submit and at
//! dispatch — the axon-loop epoch store — and the caller that re-sends a
//! request after a restart or a lost reply.

mod common;
use common::*;

use axon_fabric::{submit, Journal, SubmitError};
use axon_loop_contracts::ReceiptStatus;
use serde_json::json;

fn usage(env: &Env) -> axon_fabric::journal::ScopeUsage {
    let (j, _) = Journal::open(&env.journal).unwrap();
    j.scope_usage(&scope()).unwrap()
}

fn journal_lines(env: &Env) -> usize {
    env.journal_text().lines().count()
}

/// The authority peer is DOWN at submit: its epoch cannot be read, so the
/// request is refused as stale — never run on assumed authority — and nothing
/// is journalled or spawned.
#[test]
fn an_unreadable_epoch_store_at_submit_refuses_and_records_nothing() {
    let env = Env::new();
    let down = env.store.with_extension("down");
    std::fs::rename(&env.store, &down).unwrap();
    let before = journal_lines(&env);
    // rows4b (amendment 62): an absent store must not read as a fresh one at
    // epoch 0, which would authorize this request (expecting 0).
    let e = match submit(&request(&env, "op-outage", "t_ok").to_string(), &env.cfg(0)) {
        Ok(s) => panic!(
            "ATTACK: with the authority store unavailable the request was authorized at epoch 0: \
             {:?}",
            s.receipt.status
        ),
        Err(e) => e,
    };
    assert!(matches!(e, SubmitError::StaleEpoch { .. }), "{e}");
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
    assert!(
        !env.journal_text().contains("op-outage"),
        "an unauthorized request left a journal record"
    );
    assert!(
        journal_lines(&env) <= before + 2,
        "only the header/budget may be written"
    );
    std::fs::rename(&down, &env.store).unwrap();
}

/// The peer goes DOWN between submit and dispatch: the dispatch-time recheck
/// cannot confirm the epoch, so the reservation is cancelled (released —
/// nothing launched) and nothing runs. A re-send after the peer returns gets
/// the recorded cancellation, not a silent re-execution.
#[test]
fn an_outage_between_submit_and_dispatch_launches_nothing_and_stays_explicit() {
    let env = Env::new();
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(take_store_down);
    let req = request(&env, "op-mid-outage", "t_ok").to_string();
    let e = submit(&req, &cfg).unwrap_err();
    assert!(matches!(e, SubmitError::StaleEpoch { .. }), "{e}");
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
    std::fs::rename(env.store.with_extension("down"), &env.store).unwrap();
    assert_eq!(
        usage(&env).held.model_micro_usd,
        0,
        "released: nothing launched"
    );
    let s = submit(&req, &env.cfg(0)).unwrap();
    assert!(s.replayed);
    assert_eq!(s.receipt.status, ReceiptStatus::Canceled);
    assert_eq!(
        spawn_count(&env.spawns),
        0,
        "a cancelled op is not re-run on reconnect"
    );
}

fn take_store_down(cfg: &axon_fabric::SubmitConfig) {
    let axon_fabric::EpochSource::LoopStore { store, .. } = &cfg.epoch;
    std::fs::rename(store, store.with_extension("down")).unwrap();
}

/// Reconnect: a caller whose reply was lost re-sends the SAME request — while
/// the authority peer is down, and again after it is back, and through the
/// real CLI process. Every re-send gets the recorded receipt; the check ran
/// once, the budget was committed once.
#[test]
fn a_resent_request_after_an_outage_replays_and_never_duplicates() {
    let env = Env::new();
    let req = request(&env, "op-once", "t_ok");
    let first = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(first.receipt.status, ReceiptStatus::Completed);
    let committed = usage(&env).committed();
    assert_eq!(spawn_count(&env.spawns), 1);

    let down = env.store.with_extension("down");
    std::fs::rename(&env.store, &down).unwrap();
    let during = submit(&req.to_string(), &env.cfg(0)).unwrap();
    std::fs::rename(&down, &env.store).unwrap();
    assert!(during.replayed);
    assert_eq!(
        during.receipt, first.receipt,
        "the recorded answer, even with the peer down"
    );

    let path = env.dir.path().join("req.json");
    std::fs::write(&path, req.to_string()).unwrap();
    let out = std::process::Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .arg("submit")
        .arg("--request")
        .arg(&path)
        .arg("--journal")
        .arg(&env.journal)
        .arg("--check-registry")
        .arg(&env.registry)
        .arg("--grant-registry")
        .arg(&env.grant_registry)
        .arg("--store")
        .arg(&env.store)
        .args([
            "--tenant",
            "tenant-t",
            "--family",
            "family-f",
            "--expected-epoch",
            "0",
        ])
        .args(["--budget-micro", "1000", "--budget-exec-ms", "1000000"])
        .args(["--budget-verify-ms", "1000000", "--budget-retries", "100"])
        .arg("--workspace")
        .arg(&env.ws)
        .arg("--state")
        .arg(env.dir.path().join("fabric-state"))
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stdout)
    );
    let v: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(v["replayed"], true);
    assert_eq!(v["receipt"], serde_json::to_value(&first.receipt).unwrap());

    assert_eq!(spawn_count(&env.spawns), 1, "the task effect ran once");
    assert_eq!(usage(&env).committed(), committed, "billing committed once");
    assert_eq!(env.launch_records(), 1);
}

/// Stale epoch after reconnect: authority moved while the caller was away. A
/// NEW request under the old epoch is refused with nothing recorded; the op
/// recorded under the old epoch keeps its recorded answer (local authority
/// preserved) and is not re-run under the new one.
#[test]
fn authority_that_moved_during_an_outage_is_not_assumed() {
    let env = Env::new();
    let old = request(&env, "op-old-epoch", "t_ok");
    let first = submit(&old.to_string(), &env.cfg(0)).unwrap();
    env.bump_epoch();
    let e = submit(&request(&env, "op-after", "t_ok").to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(e, SubmitError::StaleEpoch { .. }), "{e}");
    assert!(!env.journal_text().contains("op-after"));
    let again = submit(&old.to_string(), &env.cfg(0)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.receipt, first.receipt);
    assert_eq!(spawn_count(&env.spawns), 1);
}

/// Schema mismatch from a peer: a request of another version, or one carrying
/// a field this version does not define, is refused before anything is
/// journalled — never read as the nearest version it resembles.
#[test]
fn a_request_of_another_schema_version_is_refused_before_the_journal() {
    let env = Env::new();
    for (why, mutate) in [
        (
            "another version",
            Box::new(|r: &mut serde_json::Value| r["schema"] = json!("acf-compute-request/2"))
                as Box<dyn Fn(&mut serde_json::Value)>,
        ),
        (
            "an unknown field",
            Box::new(|r: &mut serde_json::Value| r["priority"] = json!("high")),
        ),
    ] {
        let mut r = request(&env, &format!("op-{}", why.replace(' ', "-")), "t_ok");
        mutate(&mut r);
        let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
        assert_eq!(e.kind(), "malformed", "{why}: {e}");
    }
    assert!(
        !env.journal_text().contains("op-another"),
        "nothing journalled"
    );
    assert!(!env.journal_text().contains("op-an-unknown"));
    assert_eq!(spawn_count(&env.spawns), 0);
}
