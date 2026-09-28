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
        // The authority `submit` records ("principal|grant"): only it may cancel.
        authority_ref: format!("{PRINCIPAL}|grant:test"),
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
    let cancel_as = |principal: &str, grant: &str| {
        std::process::Command::new(bin)
            .args(["cancel", "--journal"])
            .arg(&env.journal)
            .args(["--op", "op-pre", "--reason", "operator", "--grant-registry"])
            .arg(&env.grant_registry)
            .args(["--principal", principal, "--grant-ref", grant])
            .output()
            .unwrap()
    };
    // G03-r22-authority-intersection: holding the journal path and the op id
    // confers nothing. Another grant of the SAME principal, and a principal the
    // grant is not bound to, are both refused, and nothing is written.
    let before = std::fs::read(&env.journal).unwrap();
    for (p, g) in [
        (PRINCIPAL, "grant:open"),
        ("principal:intruder", "grant:test"),
    ] {
        let o = cancel_as(p, g);
        assert_eq!(
            o.status.code(),
            Some(7),
            "{p}|{g}: {}",
            String::from_utf8_lossy(&o.stdout)
        );
        assert_eq!(
            std::fs::read(&env.journal).unwrap(),
            before,
            "{p}|{g} wrote to the journal"
        );
    }
    // The inspect route too: an intruder learns nothing about the op.
    let o = std::process::Command::new(bin)
        .args(["status", "--journal"])
        .arg(&env.journal)
        .args(["--op", "op-pre", "--grant-registry"])
        .arg(&env.grant_registry)
        .args([
            "--principal",
            "principal:intruder",
            "--grant-ref",
            "grant:test",
        ])
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(7));
    assert!(
        !String::from_utf8_lossy(&o.stdout).contains("held"),
        "status disclosed usage"
    );
    let o = cancel_as(PRINCIPAL, "grant:test");
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
    // A properly issuer-signed, fresh, clean PASS record — generated here,
    // with a throwaway key, never a self-authored `{"counts":{"FAIL":0}}`.
    let issuer = Issuer::generate();
    qualified_linux_cfg(d, &issuer, &good_evidence(&ev_sha))
}

#[test]
fn linux_profile_eligibility_is_bound_to_the_qualified_manifest() {
    let env = Env::new();
    let manifest = lx_manifest(&"ab".repeat(32));
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
            ..Default::default()
        },
        backend::AuthorityNeeds {
            path_scoped_grant: true,
            ..Default::default()
        },
    ] {
        assert!(backend::select(&req, Some(&ok), needs).is_err());
    }
    // A registered_check IS offered (PSV) — but only an OPERATOR suite with a
    // named test is ever run: submit refuses a check that is a file of the
    // candidate's tree before anything is reserved or launched.
    let chk_json = linux_request(&env, "op-c");
    let chk =
        axon_loop_contracts::parse::<axon_loop_contracts::ComputeRequest>(&chk_json.to_string())
            .unwrap();
    assert!(
        !chk.argv[0].starts_with("check:"),
        "precondition: a candidate file"
    );
    assert!(backend::select(&chk, Some(&ok), Default::default()).is_ok());
    // (submit's suite-only refusal: tests/psv_dispatch.rs)
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
    // G13-r22-restart-matrix: NO UNOWNED WORKER. The check worker (its pid was
    // recorded) dies with the supervisor that journalled its launch. This test
    // used to reap it by hand. Mutation: drop PR_SET_PDEATHSIG in
    // axon_cortex::runner::run_limited → the worker is still alive here.
    let pid: i32 = orphan.trim().parse().expect("worker pid");
    let t0 = std::time::Instant::now();
    while std::path::Path::new(&format!("/proc/{pid}")).exists()
        && !std::fs::read_to_string(format!("/proc/{pid}/stat"))
            .map(|s| s.split_whitespace().nth(2) == Some("Z"))
            .unwrap_or(true)
    {
        if t0.elapsed().as_secs() >= 10 {
            let _ = std::process::Command::new("kill")
                .arg("-9")
                .arg(pid.to_string())
                .status();
            panic!("the check worker {pid} outlived its SIGKILLed supervisor (an unowned worker)");
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

/// Moves the scope's epoch forward — called by the submit path AFTER its
/// submit-time epoch check and reservation, immediately BEFORE the dispatch
/// recheck. Models authority changing while the request is in flight.
fn bump_between(cfg: &axon_fabric::SubmitConfig) {
    let axon_fabric::EpochSource::LoopStore { store, scope } = &cfg.epoch;
    let st = axon_loop::Store::open_dir(store).unwrap();
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

// ── linux-microvm-protected dispatch, through a STAND-IN launcher ───────────
//
// The real `scripts/fc_linux_profile.sh` needs root + jailer + the built guest
// artifacts; these tests drive the Fabric's side of the contract (argv it
// passes, result.json → receipt, --verify-result rebinding, cleanup) with a
// stand-in that writes the documented `axon-linux-microvm-result/1` shape.
// They say nothing about the VM itself — B263's qualification harness does.
// (`stand_in_launcher` lives in `common`, shared with `qualification.rs`.)

fn linux_run_request(env: &Env, op: &str, manifest_guest_axon: &str) -> serde_json::Value {
    let mut r = linux_request(env, op);
    r["job_kind"] = json!("interpreter_run");
    r["argv"] = json!(["f.ax"]);
    r["registered_executable_ref"] = json!(backend::LINUX_GUEST_AXON_ID);
    // The Linux guest has no policy channel (x1): only a grant that
    // withholds nothing is eligible.
    r["grant_ref"] = json!("grant:open");
    r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        backend::LINUX_GUEST_AXON_ID,
        manifest_guest_axon
    ));
    r
}

fn linux_submit(env: &Env, op: &str, launcher: std::path::PathBuf) -> axon_fabric::Submission {
    let guest = "cd".repeat(32);
    let manifest = lx_manifest(&guest);
    let mut lx = linux_cfg(env, &manifest, "");
    set_launcher(&mut lx, launcher);
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let mut cfg = env.cfg(0);
    cfg.linux = Some(lx);
    submit(&linux_run_request(env, op, &guest).to_string(), &cfg).unwrap()
}

#[test]
fn linux_profile_ok_run_maps_to_a_completed_receipt() {
    let env = Env::new();
    let s = linux_submit(&env, "op-lx-ok", stand_in_launcher(&env, 0, true, true, 0));
    assert_eq!(s.backend, Some("linux-microvm-protected"));
    let r = &s.receipt;
    assert_eq!(r.backend_profile_ref.as_str(), "linux-microvm-protected");
    assert_eq!(r.status, ReceiptStatus::Completed);
    assert_eq!(r.process_exit_code, Some(0));
    // The profile runs a program; it does not produce a check verdict.
    assert_eq!(r.verification, ReceiptVerification::NotRequested);
    assert!(r
        .evidence_refs
        .iter()
        .any(|e| e.as_str().starts_with("sha256-result-json:")));
    assert_eq!(r.unresolved_liability_micro, 100);
    assert_eq!(env.launch_records(), 1);
    assert_eq!(
        spawn_count(&env.spawns),
        0,
        "the host interpreter never ran"
    );
}

#[test]
fn linux_profile_failures_are_outcome_unknown_with_liability() {
    for (name, exit, bound, clean, verify) in [
        ("cleanup-incomplete", 24, true, false, 0),
        ("verify-fails", 0, true, true, 23),
        ("unbound", 23, false, true, 0),
        ("vmm-died", 21, false, true, 0),
    ] {
        let env = Env::new();
        let s = linux_submit(
            &env,
            &format!("op-{name}"),
            stand_in_launcher(&env, exit, bound, clean, verify),
        );
        let r = &s.receipt;
        assert_eq!(
            r.status,
            ReceiptStatus::OutcomeUnknown,
            "{name}: {:?}",
            s.reason
        );
        assert_eq!(r.process_exit_code, None, "{name}");
        assert_eq!(r.unresolved_liability_micro, 100, "{name}: liability kept");
        axon_loop_contracts::parse::<axon_loop_contracts::ExecutionReceipt>(
            &serde_json::to_string(r).unwrap(),
        )
        .unwrap_or_else(|e| panic!("{name}: invalid receipt {e}"));
        let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
        let u = j.scope_usage(&scope()).unwrap();
        assert_eq!(u.liability.model_micro_usd, 100, "{name}");
    }
}

#[test]
fn a_changed_manifest_makes_the_linux_profile_ineligible_with_no_launch() {
    let env = Env::new();
    let guest = "cd".repeat(32);
    let manifest = lx_manifest(&guest);
    let mut lx = linux_cfg(&env, &manifest, &"0".repeat(64)); // evidence ≠ manifest
    set_launcher(&mut lx, stand_in_launcher(&env, 0, true, true, 0));
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let mut cfg = env.cfg(0);
    cfg.linux = Some(lx.clone());
    let s = submit(&linux_run_request(&env, "op-chg", &guest).to_string(), &cfg).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Unsupported);
    assert_eq!(env.launch_records(), 0);
    assert!(!lx.out_root.join("launches").exists(), "launcher never ran");
}

// ── G6: every `required` field is a requirement ─────────────────────────────

fn journal_terminal(env: &Env, op: &str) -> Vec<String> {
    env.journal_text()
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter(|v| v["op"] == op || v["intent"]["op"] == op)
        .map(|v| v["kind"].as_str().unwrap_or("").to_string())
        .collect()
}

/// An architecture, checkpoint kind or engine no backend offers is refused
/// before any effect: an `unsupported` receipt, journalled (intent → failed →
/// outcome, never reserved or launched) so a retry returns the same answer,
/// and nothing spawned. `backend::select` used to ignore `architecture` and
/// `checkpoint_kind` entirely, so these ran on the local interpreter.
#[test]
fn unsupported_architecture_checkpoint_or_engine_is_refused_before_effects() {
    let host_arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        "aarch64"
    };
    let other_arch = if host_arch == "x86_64" {
        "aarch64"
    } else {
        "x86_64"
    };
    for (name, field, value, why) in [
        ("arch-wasm32", "architecture", "wasm32", "architecture"),
        ("arch-other", "architecture", other_arch, "architecture"),
        (
            "ckpt-ws",
            "checkpoint_kind",
            "logical_workspace",
            "checkpoint_kind",
        ),
        (
            "ckpt-fs",
            "checkpoint_kind",
            "filesystem",
            "checkpoint_kind",
        ),
        (
            "ckpt-machine",
            "checkpoint_kind",
            "machine_state",
            "checkpoint_kind",
        ),
        ("engine-wasm", "engine", "axon_wasm", "engine"),
        ("engine-native", "engine", "native_process", "engine"),
    ] {
        let env = Env::new();
        let op = format!("op-{name}");
        let mut r = request(&env, &op, "t_ok");
        r["required"]["architecture"] = json!(host_arch);
        r["required"][field] = json!(value);
        let s = submit(&r.to_string(), &env.cfg(0)).unwrap();
        assert_eq!(s.receipt.status, ReceiptStatus::Unsupported, "{name}");
        assert_eq!(
            s.receipt.verification,
            ReceiptVerification::NotRun,
            "{name}"
        );
        assert_eq!(s.backend, None, "{name}: no backend selected");
        let reason = s.reason.unwrap_or_default();
        assert!(reason.contains(why), "{name}: {reason}");
        assert_eq!(spawn_count(&env.spawns), 0, "{name}: nothing spawned");
        assert_eq!(env.launch_records(), 0, "{name}: no launch record");
        assert_eq!(
            journal_terminal(&env, &op),
            ["intent", "failed", "outcome"],
            "{name}: journalled as refused, never reserved or launched"
        );
        // The retry returns the recorded answer, still with no effect.
        let again = submit(&r.to_string(), &env.cfg(0)).unwrap();
        assert!(again.replayed, "{name}");
        assert_eq!(again.receipt, s.receipt, "{name}");
        assert_eq!(spawn_count(&env.spawns), 0, "{name}");
    }

    // The host's own architecture with no checkpoint still runs.
    let env = Env::new();
    let mut r = request(&env, "op-host", "t_ok");
    r["required"]["architecture"] = json!(host_arch);
    let s = submit(&r.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::Completed);
    assert_eq!(spawn_count(&env.spawns), 1);
}

/// The Linux profile's own architecture/checkpoint constraints are checked
/// even when it is qualified — no other backend substitutes for it.
#[test]
fn the_linux_profile_refuses_an_architecture_or_checkpoint_it_does_not_offer() {
    let env = Env::new();
    let manifest = lx_manifest(&"ab".repeat(32));
    let ok = linux_cfg(&env, &manifest, "");
    for (field, value) in [
        ("architecture", "aarch64"),
        ("architecture", "wasm32"),
        ("checkpoint_kind", "machine_state"),
    ] {
        let mut r = linux_request(&env, "op-lx-g6");
        r["job_kind"] = json!("interpreter_run");
        r["argv"] = json!(["f.ax"]);
        r["required"][field] = json!(value);
        let req = axon_loop_contracts::parse::<axon_loop_contracts::ComputeRequest>(&r.to_string())
            .unwrap();
        let e = backend::select(&req, Some(&ok), Default::default()).unwrap_err();
        assert!(e.0.contains("linux-microvm-protected"), "{}", e.0);
        assert!(e.0.contains(field), "{}", e.0);
    }
}

// ── D-C3: one acf1 canonicaliser across the cortex → fabric seam ────────────

/// The Fabric computes the SAME digests the cortex side puts in its requests,
/// for adversarial inputs too, and both equal the `cl22` canonical form of the
/// same object. And the Fabric has no canonicaliser of its own: its digest
/// functions delegate to `axon_cortex::runner::acf1_canonical_bytes` (a
/// second implementation, kept equal only by this test, is what D-C3 was).
#[test]
fn one_acf1_canonicaliser_serves_both_sides_of_the_seam() {
    for path in ["f.ax", "dir/é\"q\\\u{1}\u{7f}😀.ax", "a\u{2028}b\n.ax"] {
        let bytes = path.as_bytes();
        let fab = axon_fabric::submit::workspace_digest(path, bytes);
        let cx = axon_cortex::runner::fabric_workspace_digest(path, bytes);
        assert_eq!(fab.as_str(), cx, "{path:?}");
        let v = json!({"path": path, "sha256": sha256_hex(bytes)});
        let cl22 = axon_loop_contracts::canonical_bytes(&v).unwrap();
        assert_eq!(
            axon_cortex::runner::acf1_canonical_bytes(&[
                ("path", path),
                ("sha256", &sha256_hex(bytes))
            ]),
            cl22,
            "{path:?}: the acf1 bytes are the cl22 canonical form"
        );
    }
    let src = include_str!("../src/submit.rs");
    let fns = &src[src.find("pub fn executable_digest").unwrap()..src.find("fn opaque(").unwrap()];
    assert!(
        fns.contains("axon_cortex::runner::fabric_executable_digest")
            && fns.contains("axon_cortex::runner::fabric_workspace_digest")
            && !fns.contains("canonical_bytes")
            && !fns.contains("sha256_hex"),
        "axon-fabric's acf1 digests must delegate to the single canonicaliser:\n{fns}"
    );
}

fn sha256_hex(b: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(b))
}

/// G13-r22-unknown-reconcile: a check that TIMES OUT after its launch record
/// (it may have had effects) is never read as success, failure, or free: the
/// receipt is TimedOut with verification unknown, its whole reservation stays
/// outstanding liability (no refund, nothing "charged" as if known), the
/// episode projection is OutcomeUnknown, and a re-send with the same
/// operation id replays that answer — no exactly-once claim, no retry.
#[test]
fn a_timeout_after_launch_is_unknown_with_liability_and_is_never_retried() {
    let env = Env::new();
    let starts = env.dir.path().join("starts.log");
    let slow = env.dir.path().join("axon-timeout.sh");
    std::fs::write(
        &slow,
        format!(
            "#!/bin/sh\necho start >> '{}'\nexec sleep 300\n",
            starts.display()
        ),
    )
    .unwrap();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    write_registry(&env.registry, &slow, None);
    let mut req = request(&env, "op-timeout", "t_ok");
    req["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        "axon-test-local",
        &sha256_file(&slow)
    ));
    req["limits"]["wall_time_ms"] = json!(300);
    let s = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(s.receipt.status, ReceiptStatus::TimedOut, "{:?}", s.reason);
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    assert_eq!(s.receipt.unresolved_liability_micro, 100);
    assert_eq!(
        axon_loop_contracts::project_receipt_status(s.receipt.status),
        axon_loop_contracts::EpisodeStatus::OutcomeUnknown
    );
    let (j, _) = axon_fabric::Journal::open(&env.journal).unwrap();
    let u = j.scope_usage(&scope()).unwrap();
    assert_eq!(
        u.liability.model_micro_usd, 100,
        "outstanding, not refunded"
    );
    assert_eq!((u.held.model_micro_usd, u.charged.model_micro_usd), (0, 0));
    drop(j);
    let again = submit(&req.to_string(), &env.cfg(0)).unwrap();
    assert!(again.replayed);
    assert_eq!(again.receipt, s.receipt);
    let runs = std::fs::read_to_string(&starts).unwrap().lines().count();
    assert_eq!(runs, 1, "the timed-out check was retried");
}

/// G10-r22-trial-identity: repeated runs of ONE task and arm are distinct
/// trials and all execute — the same semantic task id never deduplicates a
/// fresh trial. A transport retry (same OperationId, same input) is the only
/// thing that replays; an authorized new execution of the same trial is a new
/// AttemptId (and op) and runs again.
#[test]
fn repeated_trials_of_one_task_are_distinct_and_only_a_transport_retry_replays() {
    let env = Env::new();
    let run = |op: &str, trial: &str, attempt: &str| {
        let mut r = request(&env, op, "t_ok");
        r["task_id"] = json!("task-same");
        r["trial_id"] = json!(trial);
        r["attempt_id"] = json!(attempt);
        submit(&r.to_string(), &env.cfg(0)).unwrap()
    };
    let t1 = run("op-t1", "trial-rep-1", "a1");
    let t2 = run("op-t2", "trial-rep-2", "a1");
    assert!(
        !t1.replayed && !t2.replayed,
        "a fresh trial was deduplicated by its task id"
    );
    assert_eq!(spawn_count(&env.spawns), 2);
    // Transport retry of trial 1: identical OperationId/input — replayed, not re-run.
    let retry = run("op-t1", "trial-rep-1", "a1");
    assert!(retry.replayed);
    assert_eq!(retry.receipt, t1.receipt);
    assert_eq!(spawn_count(&env.spawns), 2);
    // An authorized NEW execution of trial 1: new AttemptId, new op — it runs.
    let again = run("op-t1-a2", "trial-rep-1", "a2");
    assert!(!again.replayed);
    assert_eq!(spawn_count(&env.spawns), 3);
    assert_eq!(again.receipt.attempt_id.as_str(), "a2");
    // Reusing the OperationId for a different attempt is not a retry: conflict.
    let mut r = request(&env, "op-t1", "t_ok");
    r["task_id"] = json!("task-same");
    r["trial_id"] = json!("trial-rep-1");
    r["attempt_id"] = json!("a3");
    let e = submit(&r.to_string(), &env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "conflict", "{e}");
    assert_eq!(spawn_count(&env.spawns), 3);
}

/// G01-r22-nonvacuous-outcome: a whole-suite check (no filter) in which most
/// checks pass and ONE fails is Failed — a high checklist score never offsets a
/// failed requirement — and the matched count is every check that ran.
#[test]
fn one_failed_check_fails_the_whole_suite_whatever_the_rest_score() {
    let env = Env::new();
    let mut r = request(&env, "op-suite", "t_ok");
    r["argv"] = json!(["f.ax"]);
    let s = submit(&r.to_string(), &env.cfg(0)).unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "{:?}",
        s.check_report
    );
    assert_eq!(
        s.receipt.matched_checks,
        Some(2),
        "t_ok passed and t_bad failed: both matched"
    );
    assert_eq!(
        s.receipt.status,
        ReceiptStatus::Completed,
        "the process ran to completion; the verdict is what failed"
    );
}

/// A16 at the launch itself: eligibility passed, then the launcher's bytes
/// changed before dispatch. The dispatch-time recheck refuses it before any
/// launch record, and the replaced launcher never runs.
#[test]
fn a_launcher_replaced_after_eligibility_never_runs() {
    fn swap(cfg: &axon_fabric::SubmitConfig) {
        let l = &cfg.linux.as_ref().unwrap().launcher;
        std::fs::write(l, "#!/bin/sh\necho REPLACED >> /dev/null\nexit 0\n").unwrap();
    }
    let env = Env::new();
    let guest = "cd".repeat(32);
    let mut lx = linux_cfg(&env, &lx_manifest(&guest), "");
    // A private copy, so swapping it cannot disturb other tests' launchers.
    let own = env.dir.path().join("own-launcher.sh");
    std::fs::copy(stand_in_launcher(&env, 0, true, true, 0), &own).unwrap();
    std::fs::set_permissions(&own, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
    set_launcher(&mut lx, own);
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let launches = lx.out_root.join("launches");
    let mut cfg = env.cfg(0);
    cfg.linux = Some(lx);
    cfg.pre_launch_hook = Some(swap);
    // Refused by the dispatch-time recheck, before any launch record.
    let e = submit(
        &linux_run_request(&env, "op-lx-swap", &guest).to_string(),
        &cfg,
    )
    .unwrap_err();
    assert!(e.to_string().contains("RULE:launcher-pinned"), "{e}");
    assert!(!launches.exists(), "the replaced launcher ran");
    assert_eq!(env.launch_records(), 0, "no launch record");
}
