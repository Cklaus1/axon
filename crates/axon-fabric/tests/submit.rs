//! The Fabric submit path, against the REAL interpreter, a REAL axon-loop
//! epoch store and a REAL journal file. B280-subset fault cases included.

mod common;
use common::*;

use axon_fabric::backend::{self, LinuxProfileConfig};
use axon_fabric::{submit, OpState, SubmitError};
use axon_loop_contracts::{
    EvidenceSource, OperationId, ReceiptStatus, ReceiptUsageState, ReceiptVerification,
};
use serde_json::json;

fn op(s: &str) -> OperationId {
    OperationId::new(s).unwrap()
}

#[test]
fn digests_agree_between_cortex_and_the_fabric() {
    let e = axon_cortex::runner::RegisteredExecutable {
        path: "/x".into(),
        sha256: "ab".repeat(32),
    };
    assert_eq!(
        axon_fabric::submit::executable_digest("axon-test-local", &e).as_str(),
        axon_cortex::runner::fabric_executable_digest("axon-test-local", &"ab".repeat(32))
    );
    assert_eq!(
        axon_fabric::submit::workspace_digest("dir/f.ax", b"bytes").as_str(),
        axon_cortex::runner::fabric_workspace_digest("dir/f.ax", b"bytes")
    );
}

#[test]
fn a_registered_check_runs_and_emits_a_supervisor_observed_receipt() {
    let env = Env::new();
    let s = submit(&request(&env, "op-pass", "t_ok").to_string(), &env.cfg(0)).unwrap();
    let r = &s.receipt;
    assert_eq!(s.backend, Some(backend::LOCAL_INTERPRETER.id));
    assert_eq!(
        r.backend_profile_ref.as_str(),
        "process_scoped/local-interpreter"
    );
    assert_eq!(r.status, ReceiptStatus::Completed);
    assert_eq!(r.verification, ReceiptVerification::Passed);
    assert_eq!(r.process_exit_code, Some(0));
    assert_eq!(r.matched_checks, Some(1));
    assert_eq!(r.evidence_source, EvidenceSource::SupervisorObserved);
    assert_eq!(r.usage_state, ReceiptUsageState::Unknown);
    assert_eq!(r.cost_micro, None, "unmetered cost is None, never 0");
    assert_eq!(r.unresolved_liability_micro, 100, "liability kept");
    // The receipt validates against its contract.
    axon_loop_contracts::parse::<axon_loop_contracts::ExecutionReceipt>(
        &serde_json::to_string(r).unwrap(),
    )
    .expect("receipt is a valid acf-execution-receipt/1");
    assert_eq!(spawn_count(&env.spawns), 1);
    assert_eq!(env.launch_records(), 1);

    // A failing named check is failed, not passed; an absent one is not_run.
    let f = submit(&request(&env, "op-fail", "t_bad").to_string(), &env.cfg(0)).unwrap();
    assert_eq!(f.receipt.verification, ReceiptVerification::Failed);
    let n = submit(
        &request(&env, "op-none", "no_such_test").to_string(),
        &env.cfg(0),
    )
    .unwrap();
    assert_eq!(n.receipt.verification, ReceiptVerification::NotRun);
    assert_eq!(n.receipt.matched_checks, Some(0));
}

// ── B280 subset ─────────────────────────────────────────────────────────────

#[test]
fn duplicate_op_same_input_returns_the_same_receipt_without_re_execution() {
    let env = Env::new();
    let req = request(&env, "op-dup", "t_ok").to_string();
    let first = submit(&req, &env.cfg(0)).unwrap();
    let journal_after_first = env.journal_text();
    assert_eq!(spawn_count(&env.spawns), 1);

    let again = submit(&req, &env.cfg(0)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.receipt, first.receipt, "byte-identical receipt");
    assert_eq!(spawn_count(&env.spawns), 1, "no second spawn");
    assert_eq!(env.journal_text(), journal_after_first, "nothing appended");
}

#[test]
fn same_op_different_input_is_a_conflict_with_zero_effects() {
    let env = Env::new();
    submit(&request(&env, "op-x", "t_ok").to_string(), &env.cfg(0)).unwrap();
    let before = env.journal_text();
    let spawns = spawn_count(&env.spawns);

    let err = submit(&request(&env, "op-x", "t_bad").to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(err, SubmitError::Conflict(_)), "{err}");
    assert_eq!(spawn_count(&env.spawns), spawns, "no spawn");
    assert_eq!(env.journal_text(), before, "no journal record");
}

#[test]
fn epoch_mismatch_at_submit_is_refused_with_zero_effects() {
    let env = Env::new();
    env.bump_epoch(); // store now at 1
    let err = submit(&request(&env, "op-stale", "t_ok").to_string(), &env.cfg(0)).unwrap_err();
    assert!(
        matches!(err, SubmitError::StaleEpoch { expected: 0, .. }),
        "{err}"
    );
    assert_eq!(spawn_count(&env.spawns), 0, "no process spawned");
    assert_eq!(env.launch_records(), 0, "no launch record");
    assert!(
        !env.journal_text().contains("op-stale"),
        "no intent recorded either"
    );
    // At the current epoch it runs.
    let ok = submit(&request(&env, "op-fresh", "t_ok").to_string(), &env.cfg(1)).unwrap();
    assert_eq!(ok.receipt.status, ReceiptStatus::Completed);
}

#[test]
fn a_request_naming_an_unregistered_or_mismatched_executable_is_refused() {
    let env = Env::new();
    let mut r = request(&env, "op-unreg", "t_ok");
    r["registered_executable_ref"] = json!("/bin/sh");
    let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(e, SubmitError::Unregistered(_)), "{e}");
    let mut r = request(&env, "op-digest", "t_ok");
    r["executable_digest"] = json!(format!("acf1:{}", "0".repeat(64)));
    let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
    assert!(matches!(e, SubmitError::Unregistered(_)), "{e}");
    // argv never names a program.
    let mut r = request(&env, "op-argv", "t_ok");
    r["argv"] = json!(["/etc/passwd"]);
    assert!(submit(&r.to_string(), &env.cfg(0)).is_err());
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);
}

#[test]
fn cancellation_before_launch_releases_after_launch_keeps_liability() {
    let env = Env::new();
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    let sc = scope();
    j.declare_budget(&sc, env.cfg(0).budget).unwrap();
    let mk = |id: &str| axon_fabric::Intent {
        op: op(id),
        task_id: axon_loop_contracts::TaskId::new("task-1").unwrap(),
        trial_id: axon_loop_contracts::TrialId::new("trial-1").unwrap(),
        attempt_id: axon_loop_contracts::AttemptId::new("attempt-1").unwrap(),
        input_digest: axon_loop_contracts::Ref::new(format!("cl22:{}", "1".repeat(64))).unwrap(),
        config: json!({}),
        authority_ref: "g".into(),
        authority_epoch: axon_loop_contracts::AuthorityEpoch::new(0).unwrap(),
        scope: sc.clone(),
        reservation: axon_fabric::ResourceVector {
            model_micro_usd: 100,
            exec_ms: 10,
            verify_ms: 0,
            retries: 0,
        },
        expected_version: 0,
    };
    j.begin(mk("op-pre")).unwrap();
    j.reserve(&op("op-pre")).unwrap();
    j.begin(mk("op-post")).unwrap();
    j.reserve(&op("op-post")).unwrap();
    j.mark_launched(&op("op-post")).unwrap();
    assert_eq!(j.scope_usage(&sc).unwrap().held.model_micro_usd, 200);

    // Live cancel of the LAUNCHED op (the supervisor still holds the journal):
    // a cancel acknowledgement is not cleanup and carries no cost evidence,
    // so the whole reservation stays as unresolved liability.
    j.cancel(
        &op("op-post"),
        "operator",
        Some(axon_fabric::Billing::Unknown),
    )
    .unwrap();
    // A launched op cannot be "released" as if nothing happened.
    let u = j.scope_usage(&sc).unwrap();
    assert_eq!(u.liability.model_micro_usd, 100);
    assert_eq!(u.held.model_micro_usd, 100, "op-pre still held");
    drop(j);

    // The never-launched op, cancelled through the CLI as an operator would:
    // released, nothing billed.
    let bin = env!("CARGO_BIN_EXE_axon-fabric");
    let o = std::process::Command::new(bin)
        .args(["cancel", "--journal"])
        .arg(&env.journal)
        .args(["--op", "op-pre", "--reason", "operator"])
        .output()
        .unwrap();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pre: serde_json::Value = serde_json::from_slice(&o.stdout).unwrap();
    assert_eq!(pre["state"], "cancelled");
    assert_eq!(pre["billing"], serde_json::Value::Null, "released");
    assert_eq!(pre["scope_usage"]["held"]["model_micro_usd"], 0);
    assert_eq!(
        pre["scope_usage"]["liability"]["model_micro_usd"], 100,
        "the launched op's liability survives restart"
    );

    // And a launched op whose process died with the supervisor is not
    // cancellable after the fact: reopen reconciled it; that is an outcome
    // nobody observed, not a cancel.
}

// ── backend eligibility ─────────────────────────────────────────────────────

fn linux_request(env: &Env, op: &str) -> serde_json::Value {
    let mut r = request(env, op, "t_ok");
    r["required"]["hardware_isolation"] = json!(true);
    r["required"]["os"] = json!("linux");
    r
}

#[test]
fn hardware_isolation_linux_is_refused_without_a_qualified_profile() {
    let env = Env::new();
    // No Linux profile configured: unsupported, never downgraded.
    let s = submit(&linux_request(&env, "op-lx").to_string(), &env.cfg(0)).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Unsupported);
    assert!(s.reason.unwrap().contains("linux-microvm-protected"));
    assert_eq!(spawn_count(&env.spawns), 0);
    assert_eq!(env.launch_records(), 0);

    // hardware_isolation with os=none: the Axon-kernel VM cannot run it and
    // nothing substitutes.
    let mut r = request(&env, "op-hw", "t_ok");
    r["required"]["hardware_isolation"] = json!(true);
    let s = submit(&r.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Unsupported);
    assert!(s.reason.unwrap().contains("axon-metal-fc-nojailer"));
    assert_eq!(spawn_count(&env.spawns), 0);
}

fn linux_cfg(env: &Env, manifest_bytes: &str, evidence_sha: &str) -> LinuxProfileConfig {
    let d = env.dir.path();
    std::fs::write(d.join("manifest.json"), manifest_bytes).unwrap();
    let ev_sha = if evidence_sha.is_empty() {
        sha256_file(&d.join("manifest.json"))
    } else {
        evidence_sha.to_string()
    };
    std::fs::write(
        d.join("evidence.json"),
        json!({"schema":"axon-b263-evidence/1","counts":{"FAIL":0},
               "profile":{"name":"linux-microvm-protected","manifest_sha256":ev_sha}})
        .to_string(),
    )
    .unwrap();
    LinuxProfileConfig {
        launcher: d.join("no-launcher.sh"),
        manifest: d.join("manifest.json"),
        artifacts_dir: None,
        evidence: d.join("evidence.json"),
        out_root: d.join("lx-out"),
    }
}

#[test]
fn linux_profile_eligibility_is_bound_to_the_qualified_manifest() {
    let env = Env::new();
    let manifest = json!({"artifacts":{"axon":{"sha256":"ab".repeat(32)}}}).to_string();
    let req = axon_loop_contracts::parse::<axon_loop_contracts::ComputeRequest>(
        &{
            let mut r = linux_request(&env, "op-e");
            r["job_kind"] = json!("interpreter_run");
            r["argv"] = json!(["f.ax"]);
            r
        }
        .to_string(),
    )
    .unwrap();
    let ok = linux_cfg(&env, &manifest, "");
    assert_eq!(
        backend::select(&req, Some(&ok), Default::default())
            .unwrap()
            .id,
        "linux-microvm-protected"
    );
    // A changed manifest (evidence names a different sha) is ineligible.
    let changed = linux_cfg(&env, &manifest, &"0".repeat(64));
    let e = backend::select(&req, Some(&changed), Default::default()).unwrap_err();
    assert!(e.0.contains("differs from the qualified"), "{}", e.0);
    // x1: a guest effect ceiling is refused; x2: a path-scoped grant is refused.
    let ok = linux_cfg(&env, &manifest, "");
    for needs in [
        backend::AuthorityNeeds {
            guest_policy_channel: true,
            path_scoped_grant: false,
        },
        backend::AuthorityNeeds {
            guest_policy_channel: false,
            path_scoped_grant: true,
        },
    ] {
        assert!(backend::select(&req, Some(&ok), needs).is_err());
    }
    // A registered_check is not something this profile can adjudicate.
    let chk = axon_loop_contracts::parse::<axon_loop_contracts::ComputeRequest>(
        &linux_request(&env, "op-c").to_string(),
    )
    .unwrap();
    assert!(backend::select(&chk, Some(&ok), Default::default()).is_err());
    // And the process-scoped backend is never chosen for it.
    assert!(backend::select(&req, None, Default::default()).is_err());
}

#[test]
fn linux_result_mapping_never_reports_unbound_or_uncleaned_output_as_done() {
    let d = tempfile::tempdir().unwrap();
    let write =
        |v: serde_json::Value| std::fs::write(d.path().join("result.json"), v.to_string()).unwrap();
    let base = |status: &str, bound: bool, complete: bool, w: i64| {
        json!({"schema":"axon-linux-microvm-result/1","status":status,"output_bound":bound,
               "workload_exit":w,"outputs":{"stdout":{"sha256":"aa"}},
               "cleanup":{"complete":complete,"left_behind":[]}})
    };
    let verify_ok = &mut || Some(0);
    let verify_bad = &mut || Some(23);
    use backend::LinuxOutcome::*;

    write(base("ok", true, true, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(0), d.path(), verify_ok).0,
        Ok { workload_exit: 0 }
    );
    // --verify-result disagrees: unknown, not ok.
    assert_eq!(
        backend::interpret_linux_result(Some(0), d.path(), verify_bad).0,
        Unknown
    );
    // Cleanup incomplete (24, or complete=false): unknown.
    write(base("cleanup-incomplete", true, false, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(24), d.path(), verify_ok).0,
        Unknown
    );
    write(base("ok", true, false, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(0), d.path(), verify_ok).0,
        Unknown
    );
    // Output unbound (23), VMM died (21): unknown.
    write(base("output-unbound", false, true, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(23), d.path(), verify_ok).0,
        Unknown
    );
    write(base("vmm-died", false, true, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(21), d.path(), verify_ok).0,
        Unknown
    );
    write(base("workload-failed", true, true, 3));
    assert_eq!(
        backend::interpret_linux_result(Some(10), d.path(), verify_ok).0,
        WorkloadFailed { workload_exit: 3 }
    );
    write(base("timeout", false, true, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(20), d.path(), verify_ok).0,
        TimedOut
    );
    write(base("launch-refused", false, true, 0));
    assert_eq!(
        backend::interpret_linux_result(Some(22), d.path(), verify_ok).0,
        Refused
    );
    std::fs::remove_file(d.path().join("result.json")).unwrap();
    assert_eq!(
        backend::interpret_linux_result(Some(0), d.path(), verify_ok).0,
        Unknown
    );
}

#[test]
fn the_submitted_ops_state_is_visible_after_reopen() {
    let env = Env::new();
    submit(&request(&env, "op-v", "t_ok").to_string(), &env.cfg(0)).unwrap();
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    let v = j.view(&op("op-v")).unwrap();
    assert_eq!(v.state, OpState::Completed);
    assert!(v.launched);
    assert!(v.outcome.is_some(), "the receipt is journalled");
}

/// A registered "interpreter" that records its spawn, then blocks until killed.
fn slow_wrapper(env: &Env, marker: &std::path::Path) -> std::path::PathBuf {
    let p = env.dir.path().join("axon-slow.sh");
    std::fs::write(
        &p,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 300\n",
            marker.display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

#[test]
fn sigkill_after_launch_reconciles_to_outcome_unknown_with_liability() {
    let env = Env::new();
    let started = env.dir.path().join("started");
    let slow = slow_wrapper(&env, &started);
    write_registry(&env.registry, &slow, None);
    let mut req = request(&env, "op-kill", "t_ok");
    req["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        "axon-test-local",
        &sha256_file(&slow)
    ));
    let req_path = env.dir.path().join("req.json");
    std::fs::write(&req_path, req.to_string()).unwrap();

    // A REAL submitting process, SIGKILLed once its effect has started.
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .arg("submit")
        .arg("--request")
        .arg(&req_path)
        .arg("--journal")
        .arg(&env.journal)
        .arg("--check-registry")
        .arg(&env.registry)
        .arg("--store")
        .arg(&env.store)
        .args([
            "--tenant",
            "tenant-t",
            "--family",
            "family-f",
            "--expected-epoch",
            "0",
            "--budget-micro",
            "1000",
            "--budget-exec-ms",
            "1000000",
            "--budget-verify-ms",
            "1000000",
            "--budget-retries",
            "100",
        ])
        .arg("--workspace")
        .arg(&env.ws)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .unwrap();
    let t0 = std::time::Instant::now();
    while !started.exists() {
        assert!(t0.elapsed().as_secs() < 30, "the effect never started");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    child.kill().unwrap(); // SIGKILL
    let st = child.wait().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(st.signal(), Some(9));
    assert_eq!(
        env.launch_records(),
        1,
        "the launch was journalled BEFORE the effect"
    );

    // Reopen: OutcomeUnknown, liability kept, never re-executed.
    let (j, rep) = axon_fabric::Journal::open(&env.journal).unwrap();
    assert_eq!(rep.reconciled_unknown, vec![op("op-kill")]);
    assert_eq!(
        j.view(&op("op-kill")).unwrap().state,
        OpState::OutcomeUnknown
    );
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.liability.model_micro_usd, 100);
    assert_eq!(u.held.model_micro_usd, 0);
    drop(j);

    // Resubmitting the identical request returns outcome_unknown and does
    // NOT run the effect again.
    let orphan = std::fs::read_to_string(&started).unwrap();
    std::fs::remove_file(&started).unwrap();
    let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert!(s.replayed);
    assert_eq!(s.receipt.status, ReceiptStatus::OutcomeUnknown);
    assert_eq!(s.receipt.process_exit_code, None);
    assert_eq!(s.receipt.unresolved_liability_micro, 100);
    assert!(!started.exists(), "no re-execution");
    // Reap the orphaned child of the killed launcher (its pid was recorded).
    let _ = std::process::Command::new("kill")
        .arg("-9")
        .arg(orphan.trim())
        .status();
}

/// Moves the scope's epoch forward — called by the submit path AFTER its
/// submit-time epoch check and reservation, immediately BEFORE the dispatch
/// recheck. Models authority changing while the request is in flight.
fn bump_between(cfg: &axon_fabric::SubmitConfig) {
    let axon_fabric::EpochSource::LoopStore { store, scope } = &cfg.epoch;
    let st = axon_loop::Store::open(store).unwrap();
    let cur = axon_loop::epoch::current(&st, scope).unwrap().get();
    let p = axon_loop::pointer::load(&st, scope).unwrap();
    let t: axon_loop_contracts::PolicyTransition = axon_loop_contracts::parse(
        &json!({
            "schema": "axon.closed-loop.transition/1",
            "transition_id": format!("race-{cur}"),
            "kind": "pause",
            "scope": scope,
            "expected_policy_ref": p.expected_ref(),
            "target_policy_ref": null,
            "expected_epoch": cur,
            "next_epoch": cur + 1,
            "admission_ref": null,
            "reason_ref": format!("cl22:{}", "e".repeat(64)),
            "issuer_ref": ADMITTER,
            "mechanism_test": true
        })
        .to_string(),
    )
    .unwrap();
    axon_loop::pointer::transition(&st, &t).unwrap();
}

#[test]
fn an_epoch_change_between_submit_and_launch_is_refused_before_the_launch_record() {
    let env = Env::new();
    let mut cfg = env.cfg(0);
    cfg.pre_launch_hook = Some(bump_between);
    let err = submit(&request(&env, "op-moved", "t_ok").to_string(), &cfg).unwrap_err();
    assert!(
        matches!(err, SubmitError::StaleEpoch { expected: 0, .. }),
        "{err}"
    );
    assert_eq!(spawn_count(&env.spawns), 0, "no process spawned");
    assert_eq!(env.launch_records(), 0, "no launch record");
    // The intent + reservation were recorded; the reservation is RELEASED
    // (cancelled before launch), so no budget stays held.
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    let v = j.view(&op("op-moved")).unwrap();
    assert_eq!(v.state, OpState::Cancelled);
    assert!(!v.launched);
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(u.committed(), axon_fabric::ResourceVector::default());
}
