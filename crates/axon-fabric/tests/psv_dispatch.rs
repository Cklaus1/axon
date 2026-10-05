//! M2 — the Fabric dispatches an operator suite to the protected profile and
//! derives the verdict ITSELF (v022-psv-protocol.md §3, §5, §6).
//!
//! The launcher is a test-trust-only stand-in (`axon-fabric __psv-host-guest`)
//! that runs the REAL trusted runner and the REAL interpreter on the host, and
//! applies at most one forgery to what it returns. Each negative test names
//! the check that must refuse it; the guest's `status` claim is never the
//! answer.
//!
//! Every guest-path verdict here is `guest-unobserved`, never `protected`:
//! there is no preflight observation until M3 (A14).

mod common;
use common::*;

use axon_fabric::backend::{self, LinuxProfileConfig};
use axon_fabric::submit;
use axon_fabric::workspace::{Quota, WorkspaceStore, WorkspaceTree};
use axon_loop_contracts::{Acf1Ref, ReceiptStatus, ReceiptVerification};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";
/// Its test names differ from the candidate fixture's own `t_ok`/`t_bad`: a
/// sealed candidate defining a name the suite defines is refused (PCI, E0004).
const SUITE: &str = "mod f\nuse f.{double}\n\n@[test]\nfn t_psv_ok() { assert_eq(double(21), 42) }\n\n@[test]\nfn t_psv_fail() { assert_eq(double(1), 3) }\n\n@[test]\nfn t_psv_pair() { assert_eq(double(1), 2) }\n\n@[test]\nfn t_psv_pair_breaks() { assert_eq(double(1), 5) }\n";

struct World {
    env: Env,
    issuer: Issuer,
    candidate: Acf1Ref,
    manifest: PathBuf,
    /// A: what the privileged helper re-verifies (the manifest pins them).
    inputs: HelperInputs,
    /// Amendment 50: the custodian that issues and spends the nonce.
    custodian: TestCustodian,
    /// The observer every helper-route launch gets unless the test sets one
    /// (the helper launches nothing without an observation).
    auto_observer: std::sync::OnceLock<ObserverKey>,
}

/// A profile manifest that pins everything a launch manifest names.
fn full_manifest() -> String {
    let mut m: Value = serde_json::from_str(&lx_manifest(GUEST)).unwrap();
    for (n, c) in [
        ("vmlinux", '1'),
        ("rootfs.sqfs", '2'),
        ("axon-guest-init", '3'),
    ] {
        m["artifacts"][n] = json!({"sha256": c.to_string().repeat(64)});
    }
    m.to_string()
}

impl World {
    fn new() -> World {
        Self::with_manifest(&full_manifest())
    }
    fn with_manifest(text: &str) -> World {
        Self::with(text, None)
    }
    /// The candidate's `f.ax` is `src` instead of the fixture's.
    fn with_candidate(src: &str) -> World {
        Self::with(&full_manifest(), Some(src))
    }
    fn with(text: &str, candidate_src: Option<&str>) -> World {
        let env = Env::new();
        if let Some(src) = candidate_src {
            std::fs::write(env.ws.join("f.ax"), src).unwrap();
        }
        let suite_root = env.dir.path().join("suites/acc");
        std::fs::create_dir_all(&suite_root).unwrap();
        std::fs::write(suite_root.join("accept.ax"), SUITE).unwrap();
        let suite_ref = WorkspaceTree::import_dir(&suite_root, &Quota::default())
            .unwrap()
            .reference()
            .to_string();
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&env.registry).unwrap()).unwrap();
        reg["checks"] = json!([{
            "id": "acc", "visibility": "hidden", "root": suite_root,
            "entry": "accept.ax", "workspace_version_ref": suite_ref,
        }]);
        std::fs::write(&env.registry, reg.to_string()).unwrap();
        let candidate = WorkspaceStore::open(&env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&env.ws, &Quota::default())
            .unwrap();
        let manifest = env.dir.path().join("manifest.json");
        let inputs = helper_inputs(&env.dir.path().join("helper-inputs"));
        std::fs::write(&manifest, inputs.pin_manifest(text)).unwrap();
        let custodian = start_custodian(env.dir.path());
        World {
            env,
            issuer: Issuer::generate(),
            candidate,
            manifest,
            inputs,
            custodian,
            auto_observer: std::sync::OnceLock::new(),
        }
    }
    /// The launch through the (test-trust) privileged helper: the route a
    /// protected host takes (A).
    fn lx(&self, tamper: &str, extra: &str) -> LinuxProfileConfig {
        let mut lx = self.lx_direct(tamper, extra);
        let name = format!("protected-launcher-{}.json", &lx.launcher_sha256[..16]);
        let cfg = write_helper_config(
            self.env.dir.path(),
            &self.inputs,
            &lx.launcher,
            &self.manifest,
            &lx.out_root,
            &name,
        );
        use_helper(&mut lx, &cfg);
        lx
    }
    /// The DIRECT (development) route: Fabric runs the launcher itself.
    fn lx_direct(&self, tamper: &str, extra: &str) -> LinuxProfileConfig {
        let d = self.env.dir.path();
        let mut ev = good_evidence(&sha256_file(&self.manifest));
        self.inputs.pin_evidence(&mut ev);
        let mut lx = qualified_linux_cfg(d, &self.issuer, &ev);
        let script = d.join(format!(
            "psv-launcher-{tamper}-{}.sh",
            &axon_fabric::backend::jail_id(extra)[4..12]
        ));
        write_executable(
            &script,
            format!(
                "#!/bin/sh\nexec {} __psv-host-guest --axon {} --tamper '{tamper}' {extra} \"$@\"\n",
                env!("CARGO_BIN_EXE_axon-fabric"),
                axon_bin().display()
            ),
            0o755,
        );
        set_launcher(&mut lx, script);
        std::fs::create_dir_all(&lx.out_root).unwrap();
        lx
    }
    fn request(&self, op: &str, argv0: &str, test: &str) -> Value {
        let mut r = request(&self.env, op, test);
        r["required"]["hardware_isolation"] = json!(true);
        r["required"]["os"] = json!("linux");
        r["job_kind"] = json!("registered_check");
        r["argv"] = json!([argv0, test]);
        r["registered_executable_ref"] = json!(backend::LINUX_GUEST_AXON_ID);
        r["grant_ref"] = json!("grant:open");
        r["workspace_version_ref"] = json!(self.candidate.as_str());
        r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
            backend::LINUX_GUEST_AXON_ID,
            GUEST
        ));
        r
    }
    fn submit_with(&self, lx: LinuxProfileConfig, op: &str, test: &str) -> axon_fabric::Submission {
        let mut cfg = self.env.cfg(0);
        // Amendment 50: the privileged helper launches nothing without an
        // observation, so a helper-route launch is an OBSERVED one.
        if lx.privileged.is_some() {
            cfg.observer = Some(self.auto_observer());
        }
        cfg.linux = Some(lx);
        submit(&self.request(op, "check:acc", test).to_string(), &cfg).unwrap()
    }
}

impl World {
    /// An honest observer (its own key), for a helper-route launch whose
    /// test is not about the observation.
    fn auto_observer(&self) -> ObserverConfig {
        let key = self.auto_observer.get_or_init(|| {
            observer_key(self.env.dir.path(), "auto-obs", &[&self.observer_roots()])
        });
        self.observer("", key, "observer")
    }
}

fn refs(s: &axon_fabric::Submission) -> Vec<String> {
    s.receipt
        .evidence_refs
        .iter()
        .map(|e| e.as_str().to_string())
        .collect()
}

fn class(s: &axon_fabric::Submission) -> String {
    refs(s)
        .iter()
        .find_map(|e| e.strip_prefix("evidence-class:").map(str::to_string))
        .unwrap_or_default()
}

#[test]
fn an_operator_suite_passes_through_the_guest_path_as_guest_unobserved() {
    let w = World::new();
    // The DIRECT route: the privileged helper launches nothing without an
    // observation at all (amendment 50; privileged_launcher.rs).
    let s = w.submit_with(w.lx_direct("", ""), "op-psv-ok", "t_psv_ok");
    assert_eq!(s.backend, Some("linux-microvm-protected"));
    assert_eq!(s.receipt.status, ReceiptStatus::Completed, "{:?}", s.reason);
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    // The receipt is a valid acf-execution-receipt: intake parses it
    // (matched_checks >= 1 for a pass; review wf_d725935a-7ed).
    let rt = serde_json::to_string(&s.receipt).unwrap();
    let back: axon_loop_contracts::ExecutionReceipt =
        axon_loop_contracts::parse(&rt).expect("the guest-path receipt parses as a contract");
    assert_eq!(back.matched_checks, Some(1));
    // Never protected without an observation (A14): derive's None arm (M186)
    // and psv_receipt's downgrade of a route that does not attest (M606)
    // each hold it alone here (four-cell record, C9 round 3).
    assert_eq!(
        class(&s),
        "guest-unobserved",
        "ATTACK: a verdict made without an observation was classed protected"
    );
    assert_eq!(
        s.ran_under.as_ref().unwrap().evidence_class,
        "guest-unobserved"
    );
    let r = refs(&s);
    for want in [
        "launch-manifest-sha256:",
        "guest-verdict-sha256:",
        &format!("guest-axon-sha256:{GUEST}"),
        &format!("guest-kernel-sha256:{}", w.inputs.kernel_sha),
        &format!("guest-rootfs-sha256:{}", w.inputs.rootfs_sha),
        "check-suite:acc@acf1:",
    ] {
        assert!(
            r.iter().any(|e| e.starts_with(want)),
            "missing {want}: {r:?}"
        );
    }
    // The canonical suite reference, exactly once (the test is the argv).
    let suites: Vec<&String> = r.iter().filter(|e| e.starts_with("check-suite:")).collect();
    assert_eq!(suites.len(), 1, "{r:?}");
    assert!(suites[0].ends_with("#accept.ax"), "{r:?}");
    assert!(
        !r.iter().any(|e| e.starts_with("preflight-observation")),
        "{r:?}"
    );
    // The attestation rule signs it, AS guest-unobserved.
    let req =
        axon_loop_contracts::parse(&w.request("op-psv-ok", "check:acc", "t_psv_ok").to_string())
            .unwrap();
    assert_eq!(
        axon_fabric::signing::attestation_decision(&req, false, s.ran_under.as_ref()),
        Ok(())
    );
    // The secret did not stay on the host filesystem.
    assert!(no_file_named(w.env.dir.path(), "completion-secret"));
}

fn no_file_named(dir: &Path, name: &str) -> bool {
    std::fs::read_dir(dir).unwrap().flatten().all(|e| {
        let p = e.path();
        if p.file_name().is_some_and(|n| n == name) {
            return false;
        }
        !std::fs::symlink_metadata(&p).unwrap().is_dir() || no_file_named(&p, name)
    })
}

/// A correct candidate prints a FAILURE line for the registered test over its
/// own genuine, keyed pass. It holds no K, so the line has no failure token:
/// no verdict, never a (protected) Failed — the reviewer's reproduction
/// (review wf_1bc28496-38e, PSV-4).
#[test]
fn a_candidate_cannot_write_a_failure_over_a_genuine_pass() {
    let w = World::with_candidate(
        "fn double(n: i64) -> i64 {\n    let o = chr(123)\n    let c = chr(125)\n    \
         println(o + \"\\\"name\\\":\\\"t_psv_ok\\\",\\\"status\\\":\\\"failed\\\",\\\"duration_ms\\\":0,\\\"message\\\":\\\"forged\\\"\" + c)\n    \
         n * 2\n}\n",
    );
    let s = w.submit_with(w.lx("", ""), "op-psv-splice", "t_psv_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        s.reason
    );
    assert!(
        s.reason
            .clone()
            .unwrap_or_default()
            .contains("keyed failure evidence"),
        "{:?}",
        s.reason
    );
    assert_ne!(class(&s), "protected");
}

/// A failure reported with a CLEAN exit is not a failure verdict (§5).
#[test]
fn a_failure_with_a_clean_exit_is_not_a_verdict() {
    let w = World::new();
    let s = w.submit_with(w.lx("exit0", ""), "op-psv-exit0", "t_psv_fail");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        s.reason
    );
    assert!(s.reason.unwrap().contains("failed but the run exited"));
}

#[test]
fn a_failing_test_is_failed_whatever_the_guest_claims() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-psv-fail", "t_psv_fail");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "{:?}",
        s.reason
    );
    // The guest claims a pass: the certified parser reads the output, and it
    // says failed. The claim must not move the verdict AT ALL: a failing test
    // that became Unknown (no verdict) has had its failure suppressed by the
    // guest, even where the later pass checks still refuse a counted pass
    // (C9 round 1b: the guest's claim is the attack, the parser the only
    // guard that keeps the verdict Failed).
    let s = w.submit_with(w.lx("claim-pass", ""), "op-psv-claim", "t_psv_fail");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "ATTACK: the guest's claim of a pass steered a failing test's verdict: {:?}",
        s.reason
    );
}

/// Each forgery of the returned evidence leaves NO verdict (Unknown), for its
/// own stated reason — and never a protected class.
#[test]
fn every_forgery_of_the_returned_evidence_is_unknown_for_its_own_reason() {
    for (tamper, why) in [
        ("stdout", "not the bytes the verdict names"),
        ("forge", "without completion evidence"),
        ("other-manifest", "not this launch's"),
        ("inputs", "inputs or test are not this launch's"),
        ("swap-after", "not the one the launcher bound"),
        ("forge-fail", "keyed failure evidence"),
    ] {
        let w = World::new();
        let s = w.submit_with(w.lx(tamper, ""), &format!("op-psv-{tamper}"), "t_psv_ok");
        assert_eq!(
            s.receipt.verification,
            ReceiptVerification::Unknown,
            "{tamper}"
        );
        let reason = s.reason.clone().unwrap_or_default();
        // "forge" is a single pass line whose token is not K's. The completion
        // check (M183) and the keyed result line (M312) are the same HMAC
        // under K, and either refuses it alone: on this route which one names
        // the reason is not the property (four-cell, EQUIV_RECORD["M183"]).
        let also = if tamper == "forge" {
            "result line"
        } else {
            why
        };
        assert!(
            reason.contains(why) || reason.contains(also),
            "{tamper}: {reason}"
        );
        assert_ne!(class(&s), "protected", "{tamper}");
    }
}

/// A11: a GENUINE pass from a previous attempt, re-labelled for this launch,
/// does not verify under this launch's key.
#[test]
fn a_previous_attempts_genuine_pass_does_not_replay() {
    let w = World::new();
    let first = w.lx("", "");
    let out_root = first.out_root.clone();
    let s = w.submit_with(first, "op-psv-a1", "t_psv_ok");
    assert_eq!(s.receipt.verification, ReceiptVerification::Passed);
    let prev = out_root.join("op-psv-a1");
    assert!(prev.join("out/test-stdout").exists());
    let lx = w.lx("replay", &format!("--replay-from {}", prev.display()));
    let s = w.submit_with(lx, "op-psv-a2", "t_psv_ok");
    assert_ne!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "ATTACK: a previous attempt's genuine pass was counted as this launch's pass: {:?}",
        s.reason
    );
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    // WHICH check refuses is not the property on this route: the completion
    // token (M183) and the keyed result line (M312) are the same HMAC under
    // this launch's key, and each refuses the replay alone (four-cell,
    // EQUIV_RECORD["M183"]). Either reason is accepted.
    let why = s.reason.unwrap_or_default();
    assert!(
        why.contains("without completion evidence") || why.contains("result line"),
        "{why}"
    );
}

/// An inadmissible launch (the VMM died) has no verdict and no protected class.
#[test]
fn an_inadmissible_launch_has_no_verdict() {
    let w = World::new();
    let s = w.submit_with(w.lx("vmm-died", ""), "op-psv-died", "t_psv_ok");
    assert_ne!(s.receipt.verification, ReceiptVerification::Passed);
    assert_ne!(s.receipt.verification, ReceiptVerification::Failed);
    assert_eq!(class(&s), "guest-unobserved");
}

/// A check that is a file of the candidate's tree is never a protected check:
/// refused before anything is reserved or launched.
#[test]
fn only_an_operator_suite_runs_on_the_protected_profile() {
    let w = World::new();
    let mut cfg = w.env.cfg(0);
    let lx = w.lx("", "");
    let launches = lx.out_root.clone();
    // Observed like every helper-route launch (`submit_with`): without an
    // observer the helper refuses for want of an observation, so the attack
    // never reached the operator-suite rule this test judges (amendment 50
    // left this hand-built config unobserved; C9 round 4 four-cell run).
    if lx.privileged.is_some() {
        cfg.observer = Some(w.auto_observer());
    }
    cfg.linux = Some(lx);
    let got = submit(
        &w.request("op-psv-cand", "f.ax", "t_psv_ok").to_string(),
        &cfg,
    );
    assert!(
        !launched(&w, "op-psv-cand")
            && std::fs::read_dir(&launches).unwrap().next().is_none()
            && !matches!(&got, Ok(s) if s.receipt.verification == ReceiptVerification::Passed),
        "ATTACK: a candidate file was launched as a check on the protected profile: {:?}",
        got.as_ref().map(|s| (&s.receipt.verification, &s.reason))
    );
    // Two independent layers refuse it: the pre-reservation check (M187) and
    // the protected arm, which builds no launch manifest without an operator
    // suite (M400). Which one refuses is not the property (four-cell,
    // EQUIV_RECORD["M187"]); with M187 removed the op is reserved and
    // journalled before the arm refuses it, so the launch-record count is not
    // asserted on this route.
    match got {
        Err(e) => assert!(e.to_string().contains("only as an operator suite"), "{e}"),
        Ok(s) => {
            assert_eq!(s.receipt.verification, ReceiptVerification::NotRun);
            assert!(
                s.reason
                    .as_deref()
                    .is_some_and(|r| r.contains(axon_fabric::submit::PROTECTED_SUITE_ONLY)),
                "{:?}",
                s.reason
            );
        }
    }
}

/// A profile manifest that does not pin the guest's kernel/rootfs/init builds
/// no launch manifest: no launch, no verdict.
#[test]
fn an_unpinned_guest_builds_no_launch() {
    let w = World::with_manifest(&lx_manifest(GUEST));
    let s = w.submit_with(w.lx("", ""), "op-psv-unpinned", "t_psv_ok");
    assert_eq!(s.receipt.verification, ReceiptVerification::NotRun);
    assert!(s.reason.unwrap().contains("profile manifest pins no"));
}

/// A local check is DEVELOPMENT evidence: stamped as such, and signed only
/// as such.
#[test]
fn a_local_check_is_development_evidence() {
    let w = World::new();
    let mut r = request(&w.env, "op-local", "t_psv_ok");
    r["argv"] = json!(["check:acc", "t_psv_ok"]);
    r["workspace_version_ref"] = json!(w.candidate.as_str());
    r["grant_ref"] = json!("grant:open");
    let s = submit(&r.to_string(), &w.env.cfg(0)).unwrap();
    assert_eq!(s.backend, Some(backend::LOCAL_INTERPRETER.id));
    // It really judged (so the class is on a real verdict, not a failed run).
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(class(&s), "development");
    assert_eq!(s.ran_under.unwrap().evidence_class, "development");
}

/// PSV-3 (dev review round wf_7cb5856d-806): on a PROTECTED host a request
/// the local backend would take is refused before anything runs. Control: the
/// same request on a non-protected host runs locally
/// (`a_local_check_is_development_evidence`).
#[test]
fn a_protected_host_runs_nothing_outside_the_protected_profile() {
    let w = World::new();
    let mut r = request(&w.env, "op-local-ph", "t_psv_ok");
    r["argv"] = json!(["check:acc", "t_psv_ok"]);
    r["workspace_version_ref"] = json!(w.candidate.as_str());
    r["grant_ref"] = json!("grant:open");
    let mut cfg = w.env.cfg(0);
    cfg.protected_host = Some(axon_fabric::psv::HostIdentity {
        config_sha256: "1".repeat(64),
        suite_registry_sha256: "2".repeat(64),
        // The operator's pin is this env's registry: the refusal below is the
        // protected-profile rule, not D1's registry check.
        grant_registry_sha256: Some(cfg.grants.sha256().to_string()),
    });
    let s = submit(&r.to_string(), &cfg).unwrap();
    assert_eq!(
        s.receipt.status,
        ReceiptStatus::Unsupported,
        "{:?}",
        s.reason
    );
    assert!(
        s.reason
            .clone()
            .unwrap_or_default()
            .contains("protected host"),
        "{:?}",
        s.reason
    );
    assert!(s.ran_under.is_none());
}

fn tenant() -> axon_loop_contracts::TenantId {
    axon_loop_contracts::TenantId::new("tenant-t").unwrap()
}

/// A1 through the Fabric: the candidate changes under the guest; the guest
/// refuses before running anything, and the Fabric reports WHY.
#[test]
fn a_candidate_changed_under_the_guest_is_refused_there() {
    let w = World::new();
    let s = w.submit_with(w.lx("candidate-changed", ""), "op-psv-cand-chg", "t_psv_ok");
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(r.contains("the guest refused: candidate tree is"), "{r}");
}

/// M180 on the route where it is the only guard: the guest's verdict says
/// REFUSED, but every other field is a genuine pass (this launch's manifest,
/// inputs and test; the keyed output and its digest; exit 0). Nothing after the
/// refusal check reads the status, so without it the Fabric would decide the
/// outcome from the output and override the guest's refusal.
#[test]
fn a_guest_refusal_stands_even_over_a_genuine_passing_run() {
    let w = World::new();
    let s = w.submit_with(
        w.lx("refused-after-pass", ""),
        "op-psv-refused-pass",
        "t_psv_ok",
    );
    assert_ne!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "ATTACK: the guest refused, and the Fabric counted its run as Passed: {:?}",
        s.reason
    );
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(
        r.contains("the guest refused: the runner refused after the run"),
        "{r}"
    );
}

/// Passed needs exit 0: a genuine pass (VALID token) whose run is reported
/// as exiting non-zero is Unknown.
#[test]
fn a_valid_token_with_a_failing_run_is_not_a_pass() {
    let w = World::new();
    let s = w.submit_with(w.lx("exit", ""), "op-psv-exit", "t_psv_ok");
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(r.contains("passed but the run exited"), "{r}");
}

/// B1 through the Fabric: the candidate fixture defines its own `@[test]
/// fn t_ok`; naming it as the acceptance test yields NO verdict (the guest
/// never collects a candidate's test), never a Passed.
#[test]
fn a_candidate_cannot_supply_the_acceptance_test_through_fabric() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-psv-candtest", "t_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        s.reason
    );
    assert!(s.reason.unwrap().contains("produced no verdict"));
}

/// The launcher's `--verify-result` runs with a CLEARED environment: nothing
/// of the caller's (PATH, …) reaches the pinned launcher (review
/// wf_d725935a-7ed).
///
/// Both routes (C9 round 2, rows): through the privileged helper the verify
/// step is the helper's child, and the helper has already dropped its whole
/// environment (`harden`) and removed its snapshot (M543); on the DIRECT
/// route Fabric runs `--verify-result` itself, from a process that still holds
/// the caller's environment, so there `sealed_exec::command`'s envp (M228) and
/// Fabric's own `psv.scrub()` (M229) are the only guards.
#[test]
fn the_verify_step_inherits_nothing_from_the_caller() {
    std::env::set_var("PSV_VERIFY_ENV_PROBE", "leak");
    let w = World::new();
    for (route, lx, op) in [
        ("privileged", w.lx("", ""), "op-psv-env"),
        ("direct", w.lx_direct("", ""), "op-psv-env-direct"),
    ] {
        let out_root = lx.out_root.clone();
        let s = w.submit_with(lx, op, "t_psv_ok");
        assert_eq!(
            s.receipt.verification,
            ReceiptVerification::Passed,
            "{route}: {:?}",
            s.reason
        );
        let seen =
            std::fs::read_to_string(out_root.join(format!("{op}/verify-env-leaked"))).unwrap();
        assert_eq!(
            seen, "no",
            "ATTACK: {route}: the caller's environment reached the verify step"
        );
        // The per-attempt secret is already gone when the verify step runs.
        let secret =
            std::fs::read_to_string(out_root.join(format!("{op}/verify-secret-present"))).unwrap();
        assert_eq!(
            secret, "no",
            "ATTACK: {route}: the secret outlived the launch into the verify step"
        );
    }
}

/// A GENUINE, valid verdict from a launch the launcher could not bind (exit
/// 27) counts for nothing: an inadmissible launch has no verdict.
#[test]
fn a_valid_verdict_from_an_unbound_launch_counts_for_nothing() {
    let w = World::new();
    let s = w.submit_with(w.lx("unbound", ""), "op-psv-unbound", "t_psv_ok");
    assert_ne!(s.receipt.verification, ReceiptVerification::Passed);
    assert_eq!(class(&s), "guest-unobserved");
}

/// PSV-6 (C9 round 4; A87): the policy the guest runs under is the policy
/// the launch manifest names, and the receipt binds the one that ran.
/// * (G) the launcher embedded ANOTHER policy (a wider ceiling) than the one
///   the manifest names: the in-guest runner refuses it, on the helper route
///   and on the direct route alike, and there is no verdict;
/// * (V) a genuine passing run whose verdict names another policy: Fabric's
///   join of the verdict's `policy_sha256` to the manifest's refuses it.
///
/// Control: the manifest's own policy, observed, through the helper: a
/// protected pass.
#[test]
fn a_guest_under_a_policy_the_manifest_does_not_name_yields_no_verdict() {
    let w = World::new();
    for (route, lx) in [
        ("helper", w.lx("policy", "")),
        ("direct", w.lx_direct("policy", "")),
    ] {
        let s = w.submit_with(lx, &format!("op-psv-policy-{route}"), "t_psv_ok");
        assert!(
            s.receipt.verification == ReceiptVerification::Unknown && class(&s) != "protected",
            "ATTACK: {route}: a guest booted under a policy the launch manifest does not name \
             yielded a verdict: {:?} {:?}",
            s.receipt.verification,
            s.reason
        );
        assert!(
            s.reason.clone().unwrap_or_default().contains("policy"),
            "{route}: {:?}",
            s.reason
        );
    }
    let s = w.submit_with(w.lx("verdict-policy", ""), "op-psv-vpolicy", "t_psv_ok");
    assert!(
        s.receipt.verification == ReceiptVerification::Unknown && class(&s) != "protected",
        "ATTACK: a guest verdict naming another policy than the manifest's was counted: {:?} {:?}",
        s.receipt.verification,
        s.reason
    );
    assert!(
        s.reason
            .clone()
            .unwrap_or_default()
            .contains("the guest ran policy"),
        "{:?}",
        s.reason
    );
    let s = w.submit_with(w.lx("", ""), "op-psv-policy-ok", "t_psv_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "control: {:?}",
        s.reason
    );
    assert_eq!(class(&s), "protected");
}

// ── M3: the preflight observation ───────────────────────────────────────────
//
// The stand-in observer composes the observation FROM the launch manifest and
// signs it with `axon-fabric sign-evidence` (the operator tool). It proves the
// PROTOCOL — domain, root, joins, freshness, nonce — never a measurement.

use axon_fabric::backend::Clock;
use axon_fabric::custodian::Custodian;
use axon_fabric::observer::{NonceStore, ObserverConfig, ObserverTrust};

impl World {
    fn observer_roots(&self) -> PathBuf {
        self.env.dir.path().join("observer_keys")
    }
    /// An observer program: `mode` applies ONE defect; `key` signs, as
    /// `authority`.
    fn observer(&self, mode: &str, key: &ObserverKey, authority: &str) -> ObserverConfig {
        let d = self.env.dir.path();
        let script = observer_script(d, mode, key, authority);
        ObserverConfig {
            command_sha256: sha256_file(&script),
            command: script,
            // D: the script observer runs under the pinned bash, from its
            // verified descriptor.
            interpreter: Some(bash_pin()),
            exec_owner: Some(unsafe { libc::geteuid() }),
            trust: ObserverTrust::for_test(&self.observer_roots()),
            // Amendment 50: Fabric holds only a client of the custodian.
            custodian: Custodian::Service(self.custodian.client()),
            max_age_s: 300,
            clock: Clock::System,
            // The in-uid program route: a test-trust stand-in (amendment 68).
            relay: None,
        }
    }
    fn submit_observed(&self, ob: ObserverConfig, op: &str) -> axon_fabric::Submission {
        let mut cfg = self.env.cfg(0);
        cfg.linux = Some(self.lx("", ""));
        cfg.observer = Some(ob);
        submit(&self.request(op, "check:acc", "t_psv_ok").to_string(), &cfg).unwrap()
    }
}

fn launched(w: &World, op: &str) -> bool {
    w.env.dir.path().join("lx-out").join(op).exists()
}

/// PSV-6 (dev review round wf_336353cb-a2b, executed there): the authority
/// epoch moves WHILE the observer runs. The observation was made under the
/// old epoch, so the launch is refused and nothing runs. Control: the same
/// observer, with no epoch change, gives a protected Passed.
#[test]
fn an_epoch_that_moves_while_the_observer_runs_refuses_the_launch() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let d = w.env.dir.path().to_path_buf();
    let s = std::thread::scope(|sc| {
        let h =
            sc.spawn(|| w.submit_observed(w.observer("wait", &key, "observer"), "op-obs-epoch"));
        let mut n = 0;
        while !d.join("observer-waiting").exists() && n < 400 {
            std::thread::sleep(std::time::Duration::from_millis(50));
            n += 1;
        }
        assert!(d.join("observer-waiting").exists(), "the observer started");
        w.env.bump_epoch();
        std::fs::write(d.join("observer-go"), "").unwrap();
        h.join().unwrap()
    });
    assert_ne!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_ne!(class(&s), "protected");
    let why = s.reason.clone().unwrap_or_default();
    assert!(why.contains("authority epoch is now 1"), "{why}");
    // Control: no epoch change.
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let d = w.env.dir.path().to_path_buf();
    std::fs::write(d.join("observer-go"), "").unwrap();
    let s = w.submit_observed(w.observer("wait", &key, "observer"), "op-obs-epoch-ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(class(&s), "protected");
}

#[test]
fn a_verified_observation_makes_the_guest_verdict_protected() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    // On a protected HOST (its O1 identity set): a hostless library launch
    // writes all-zero host and registry digests, which the loop refuses (A64).
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(w.lx("", ""));
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-obs-ok", "check:acc", "t_psv_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(class(&s), "protected");
    assert!(refs(&s)
        .iter()
        .any(|e| e.starts_with("preflight-observation-sha256:")));
    let req =
        axon_loop_contracts::parse(&w.request("op-obs-ok", "check:acc", "t_psv_ok").to_string())
            .unwrap();
    assert_eq!(
        axon_fabric::signing::attestation_decision(&req, false, s.ran_under.as_ref()),
        Ok(())
    );
    // B2 end to end: the bundle Fabric emits satisfies the LOOP's own join
    // check, under an operator observer root holding this observer's key.
    let bundle = s
        .psv_evidence
        .clone()
        .expect("a protected verdict carries its bundle");
    let root = w.env.dir.path().join("loop-operator-root");
    std::fs::create_dir_all(root.join("observer")).unwrap();
    for e in std::fs::read_dir(w.observer_roots()).unwrap().flatten() {
        std::fs::copy(e.path(), root.join("observer").join(e.file_name())).unwrap();
    }
    axon_loop_contracts::operator_trust::set_test_root(&root);
    let epoch = w.env.cfg(0).expected_epoch.get();
    // The store-trusted observers the loop narrows the operator root to: this
    // observer, under its key.
    let observers: std::collections::BTreeMap<_, _> = std::fs::read_dir(w.observer_roots())
        .unwrap()
        .flatten()
        .map(|e| {
            (
                axon_loop_contracts::OpaqueRef::new("fixture:observer").unwrap(),
                std::fs::read_to_string(e.path())
                    .unwrap()
                    .trim()
                    .to_string(),
            )
        })
        .collect();
    assert_eq!(observers.len(), 1, "one observer key in the fixture root");
    axon_loop_contracts::protected_evidence::check_bundle(
        &req,
        &s.receipt,
        &bundle.to_string(),
        epoch,
        &scope(),
        &observers,
    )
    .expect("the loop joins what Fabric launched and observed");
    // Under another authority epoch it does not (PSV-6, dev round 1).
    let e = axon_loop_contracts::protected_evidence::check_bundle(
        &req,
        &s.receipt,
        &bundle.to_string(),
        epoch + 1,
        &scope(),
        &observers,
    )
    .unwrap_err();
    assert!(e.contains("authority epoch"), "{e}");
    // A82: Fabric's manifest names the scope it launched for, and the loop
    // joins it: the same bundle does not count in another scope.
    let mut other = scope();
    other.tenant_id = axon_loop_contracts::TenantId::new("other-tenant").unwrap();
    let e = axon_loop_contracts::protected_evidence::check_bundle(
        &req,
        &s.receipt,
        &bundle.to_string(),
        epoch,
        &other,
        &observers,
    )
    .unwrap_err();
    assert!(e.contains("launch manifest is for tenant"), "{e}");
    // A64: the same launch with no operator host config (a library caller)
    // names no host, and the loop does not take it as protected evidence.
    let hostless = w.submit_observed(w.observer("", &key, "observer"), "op-obs-nohost");
    let hreq = axon_loop_contracts::parse(
        &w.request("op-obs-nohost", "check:acc", "t_psv_ok")
            .to_string(),
    )
    .unwrap();
    let e = axon_loop_contracts::protected_evidence::check_bundle(
        &hreq,
        &hostless.receipt,
        &hostless.psv_evidence.clone().expect("a bundle").to_string(),
        epoch,
        &scope(),
        &observers,
    )
    .unwrap_err();
    assert!(e.contains("host_config_sha256 is all zeros"), "{e}");
    // …and a single byte of the manifest changed breaks it.
    let mut bad = bundle.clone();
    bad["launch_manifest"] =
        serde_json::json!(format!("{} ", bad["launch_manifest"].as_str().unwrap()));
    assert!(axon_loop_contracts::protected_evidence::check_bundle(
        &req,
        &s.receipt,
        &bad.to_string(),
        epoch,
        &scope(),
        &observers,
    )
    .is_err());
    // Each launch spent its own nonce (the host launch and the hostless one).
    let used = std::fs::read_dir(w.env.dir.path().join("custodian-nonces"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "used"))
        .count();
    assert_eq!(
        used, 2,
        "ATTACK: a verified observation's nonce was not spent, so it can authorize another launch"
    );
}

/// One defect each: (observer mode, signing authority, root holding the key,
/// the reason a refusal names; `|` separates alternatives).
const DEFECTIVE_OBSERVATIONS: [(&str, &str, &str, &str); 12] = [
    // A9: another authority domain (the key IS a trusted observer). The
    // domain field (M152) and the domain-separated message (M153) each
    // refuse it alone (M152's four-cell record), so either reason.
    (
        "",
        "qualification",
        "observer",
        "is for authority|does not verify",
    ),
    // A key the observer root does not hold (only the qualification root).
    (
        "",
        "observer",
        "qualification",
        "not a trusted evidence issuer",
    ),
    ("stale", "observer", "observer", "old (max 300s)"),
    ("epoch", "observer", "observer", "for epoch 7"),
    (
        "other-manifest",
        "observer",
        "observer",
        "intended_launch_manifest_sha256",
    ),
    ("kernel", "observer", "observer", "guest.kernel_sha256"),
    // A nonce the manifest does not name, never issued: the manifest join
    // and the store's "never issued" each refuse it alone, so either
    // reason is correct here (C9 round 1).
    (
        "nonce-forged",
        "observer",
        "observer",
        "observation nonce is|never issued",
    ),
    // C9 round 1 (M201): a nonce the custodian really issued (same epoch,
    // same age) for ANOTHER launch. The store would consume it, so the
    // manifest-nonce join is the only guard.
    (
        "nonce-issued-elsewhere",
        "observer",
        "observer",
        "observation nonce is",
    ),
    (
        "claims-other-key",
        "observer",
        "observer",
        "but is signed by",
    ),
    // An observer that exits leaving NO observation: its exit status and the
    // missing file each refuse it alone (C9 round 4, rows2), so either reason.
    (
        "exit",
        "observer",
        "observer",
        "observer exited|observation.json: No such file",
    ),
    // C9 round 4 (rows2, M811): the observer leaves a GENUINE signed
    // observation and then exits non-zero. Only its exit status refuses it:
    // the observation verifies and joins.
    ("disown", "observer", "observer", "observer exited"),
    // §7: the observer measures the INSTALLED verifier; another one refuses.
    ("verifier", "observer", "observer", "verifier_sha256"),
];

/// Each defect refuses the LAUNCH (nothing runs), for its own reason, and is
/// never protected.
#[test]
fn every_defective_observation_refuses_the_launch() {
    for (mode, authority, key_in, why) in DEFECTIVE_OBSERVATIONS {
        let w = World::new();
        let d = w.env.dir.path().to_path_buf();
        let roots: Vec<PathBuf> = match key_in {
            "observer" => vec![w.observer_roots()],
            _ => {
                // The observer root holds a LEGITIMATE observer — just not
                // this key, which only the qualification root trusts.
                observer_key(&d, "real-observer", &[&w.observer_roots()]);
                vec![d.join("trusted_issuers")]
            }
        };
        let rr: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let key = observer_key(&d, "obs", &rr);
        let op = format!("op-obs-{mode}-{authority}-{key_in}");
        let s = w.submit_observed(w.observer(mode, &key, authority), &op);
        assert_eq!(s.receipt.verification, ReceiptVerification::NotRun, "{op}");
        let r = s.reason.clone().unwrap_or_default();
        assert!(
            r.contains("preflight observation refused") && why.split('|').any(|w| r.contains(w)),
            "{op}: {r}"
        );
        assert!(!launched(&w, &op), "{op}: launched");
        assert_ne!(class(&s), "protected", "{op}");
    }
}

/// The same defects on the DIRECT route (C9 round 3, rows). Through the
/// privileged helper the root boundary re-verifies the observation and spends
/// its nonce (amendment 50), so there Fabric's own preflight is a fail-fast
/// in front of it. Fabric running the launcher itself (a development host, or
/// a library consumer composing its own profile) has no helper behind it:
/// Fabric's preflight is the ONLY check between an observation the operator's
/// observer root refuses and a launch. Such a launch is never protected
/// (M606), but it must not RUN either.
#[test]
fn every_defective_observation_launches_nothing_on_the_direct_route() {
    for (mode, authority, key_in, why) in DEFECTIVE_OBSERVATIONS {
        let w = World::new();
        let d = w.env.dir.path().to_path_buf();
        let roots: Vec<PathBuf> = match key_in {
            "observer" => vec![w.observer_roots()],
            _ => {
                observer_key(&d, "real-observer", &[&w.observer_roots()]);
                vec![d.join("trusted_issuers")]
            }
        };
        let rr: Vec<&Path> = roots.iter().map(PathBuf::as_path).collect();
        let key = observer_key(&d, "obs", &rr);
        let op = format!("op-obs-{mode}-{authority}-{key_in}-direct");
        let mut cfg = w.env.cfg(0);
        cfg.linux = Some(w.lx_direct("", ""));
        cfg.observer = Some(w.observer(mode, &key, authority));
        let s = submit(&w.request(&op, "check:acc", "t_psv_ok").to_string(), &cfg).unwrap();
        assert!(
            s.receipt.verification == ReceiptVerification::NotRun && !launched(&w, &op),
            "ATTACK: {op}: a defective observation launched on the direct route ({:?}): {:?}",
            s.receipt.verification,
            s.reason
        );
        let r = s.reason.clone().unwrap_or_default();
        assert!(
            r.contains("preflight observation refused") && why.split('|').any(|w| r.contains(w)),
            "{op}: {r}"
        );
        assert_ne!(class(&s), "protected", "{op}");
    }
}

/// A15: a GENUINE, correctly signed observation of an earlier launch,
/// presented again, authorizes nothing: it observed another manifest (and its
/// nonce is spent).
#[test]
fn an_earlier_observation_does_not_authorize_another_launch() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let s = w.submit_observed(w.observer("", &key, "observer"), "op-obs-first");
    assert_eq!(class(&s), "protected");
    assert!(w.env.dir.path().join("prev-observation.json.sig").exists());
    let s = w.submit_observed(w.observer("replay", &key, "observer"), "op-obs-second");
    assert_eq!(s.receipt.verification, ReceiptVerification::NotRun);
    let r = s.reason.unwrap();
    assert!(
        r.contains("preflight observation refused")
            && r.contains("intended_launch_manifest_sha256"),
        "{r}"
    );
    assert!(!launched(&w, "op-obs-second"));
}

/// The nonce store: issued once, consumed once, of its epoch, within its age.
#[test]
fn a_nonce_authorizes_exactly_one_launch() {
    let d = tempfile::tempdir().unwrap();
    let st = NonceStore {
        dir: d.path().join("n"),
    };
    let c = Clock::FixedUnix(1_000_000);
    let n = st.issue(3, &c).unwrap();
    // A path-shaped nonce names a record OUTSIDE the store. One is planted
    // there that passes every other check (this epoch, fresh, unused; the
    // store dir exists, so the path resolves), so the format check is the
    // only guard on this route (C9 round 1b, M199).
    let outside = d.path().join("x.issued");
    std::fs::write(&outside, r#"{"epoch":3,"issued_unix":1000000}"#).unwrap();
    let got = st.consume("../x", 3, &c, 60);
    assert!(
        got.is_err() && outside.exists(),
        "ATTACK: a path-shaped nonce was consumed from outside the custodian's store: {got:?}"
    );
    assert!(got.unwrap_err().contains("not one"));
    assert!(st.consume(&n, 4, &c, 60).unwrap_err().contains("epoch"));
    assert!(st
        .consume(&n, 3, &Clock::FixedUnix(1_000_061), 60)
        .unwrap_err()
        .contains("old"));
    st.consume(&n, 3, &c, 60).unwrap();
    assert!(st
        .consume(&n, 3, &c, 60)
        .unwrap_err()
        .contains("already used"));
    assert!(st
        .consume(&"cd".repeat(16), 3, &c, 60)
        .unwrap_err()
        .contains("never issued"));
}

/// The observer program is pinned: other bytes are refused before it runs.
#[test]
fn an_unpinned_observer_is_refused() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let ob = w.observer("", &key, "observer");
    let mut text = std::fs::read_to_string(&ob.command).unwrap();
    text.push_str("\n# changed\n");
    std::fs::write(&ob.command, text).unwrap();
    let s = w.submit_observed(ob, "op-obs-pin");
    // The changed program still produces a valid, signed observation, so the
    // pin is the only guard on this route (C9 round 1b).
    assert!(
        !launched(&w, "op-obs-pin") && s.receipt.verification != ReceiptVerification::Passed,
        "ATTACK: an unpinned observer program ran and its observation authorized a launch \
         ({:?}, class {})",
        s.receipt.verification,
        class(&s)
    );
    assert!(
        s.reason.clone().unwrap().contains("not its pin"),
        "{:?}",
        s.reason
    );
}

/// A2 through the Fabric: the operator suite changes under the guest; the
/// guest refuses before running anything, and the Fabric reports WHY.
#[test]
fn a_suite_changed_under_the_guest_is_refused_there() {
    let w = World::new();
    let s = w.submit_with(w.lx("suite-changed", ""), "op-psv-suite-chg", "t_psv_ok");
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(r.contains("the guest refused: suite tree is"), "{r}");
}

/// A3 through the Fabric: a test the registered suite does not define yields
/// no verdict (never a pass), whatever the request names.
#[test]
fn a_test_the_suite_does_not_define_has_no_verdict() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-psv-absent", "t_psv_absent");
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(r.contains("produced no verdict"), "{r}");
}

/// B3 (review wf_d725935a-7ed, the reviewer's own attack): between
/// materialization and launch, the RUN DIR under the caller's --state is
/// rewritten so the failing test passes. The launch never reads it — the
/// inputs are re-materialized from the verified store into a private dir —
/// so the verdict is still Failed.
#[test]
fn a_run_dir_swapped_under_the_callers_state_changes_nothing() {
    fn swap(cfg: &axon_fabric::SubmitConfig) {
        let runs = cfg.state_dir.join("runs");
        for e in std::fs::read_dir(&runs).unwrap().flatten() {
            let p = e.path().join("check/accept.ax");
            if p.exists() {
                let _ = std::fs::set_permissions(
                    &p,
                    std::os::unix::fs::PermissionsExt::from_mode(0o644),
                );
                let _ = std::fs::set_permissions(
                    p.parent().unwrap(),
                    std::os::unix::fs::PermissionsExt::from_mode(0o755),
                );
                std::fs::write(
                    &p,
                    "mod f\nuse f.{double}\n\n@[test]\nfn t_psv_fail() { assert(true) }\n",
                )
                .unwrap();
            }
        }
    }
    let w = World::new();
    let mut cfg = w.env.cfg(0);
    cfg.linux = Some(w.lx("", ""));
    cfg.observer = Some(w.auto_observer());
    cfg.pre_launch_hook = Some(swap);
    let s = submit(
        &w.request("op-psv-b3", "check:acc", "t_psv_fail")
            .to_string(),
        &cfg,
    )
    .unwrap();
    // The verdict itself is the property: without the swap this run is
    // Failed, so any other verdict (a pass, or no verdict at all) is the
    // caller's swap reaching the launch.
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "ATTACK: a run dir swapped under the caller's state changed the verdict: {:?}",
        s.reason
    );
}

// ── C9 certifying-review hardening (PSV-3, PSV-4, PSV-6) ────────────────────

impl World {
    /// This env's config AS a protected host: the O1 identity is set, and the
    /// operator's grant-registry pin is this env's registry (so D1 admits).
    fn protected_cfg(&self) -> axon_fabric::SubmitConfig {
        let mut cfg = self.env.cfg(0);
        cfg.protected_host = Some(axon_fabric::psv::HostIdentity {
            config_sha256: "1".repeat(64),
            suite_registry_sha256: "2".repeat(64),
            grant_registry_sha256: Some(cfg.grants.sha256().to_string()),
        });
        cfg
    }
}

/// PSV-6 (C9 certifying review): a protected host whose config has no
/// observer section launches NO protected check. Before this, the launch ran
/// and derived a `guest-unobserved` verdict. The refusal names the missing
/// observer and happens before anything is reserved or launched. Control:
/// the same protected host WITH an observer launches, and its verdict is
/// protected, so the refusal is the observer rule and nothing else.
#[test]
fn a_protected_host_with_no_observer_launches_nothing() {
    // The DIRECT route first (C9 round 3, rows): a library consumer composing
    // a protected host with a launcher Fabric runs itself. No privileged
    // helper stands behind it (the helper refuses a launch with no
    // observation, amendment 50), so the observer rule is the only guard
    // here, and without it the launch runs and derives the guest-unobserved
    // verdict this rule exists to prevent.
    let w = World::new();
    let mut cfg = w.protected_cfg();
    let lx = w.lx_direct("", "");
    let out_root = lx.out_root.clone();
    cfg.linux = Some(lx);
    assert!(cfg.observer.is_none());
    let s = submit(
        &w.request("op-ph-noobs-direct", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert!(
        s.receipt.status == ReceiptStatus::Unsupported
            && std::fs::read_dir(&out_root).unwrap().next().is_none(),
        "ATTACK: a protected host with no observer launched a protected-profile check \
         (direct route): {:?} {:?} {:?}",
        s.receipt.status,
        s.receipt.verification,
        s.reason
    );
    assert_eq!(s.reason.as_deref(), Some(axon_fabric::submit::NO_OBSERVER));
    assert_eq!(w.env.launch_records(), 0, "no launch record");

    // Through the privileged helper (the route a protected host's own config
    // takes), which would also refuse (M620): the observer rule answers first.
    let w = World::new();
    let mut cfg = w.protected_cfg();
    let lx = w.lx("", "");
    let out_root = lx.out_root.clone();
    cfg.linux = Some(lx);
    assert!(cfg.observer.is_none());
    let s = submit(
        &w.request("op-ph-noobs", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.status,
        ReceiptStatus::Unsupported,
        "{:?}",
        s.reason
    );
    assert_eq!(s.receipt.verification, ReceiptVerification::NotRun);
    assert_eq!(s.reason.as_deref(), Some(axon_fabric::submit::NO_OBSERVER));
    assert!(s.ran_under.is_none(), "nothing ran");
    assert!(s.psv_evidence.is_none());
    assert_eq!(w.env.launch_records(), 0, "no launch record");
    assert!(
        std::fs::read_dir(&out_root).unwrap().next().is_none(),
        "nothing launched, no private inputs materialized"
    );

    // Control: the same protected host with an observer.
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(w.lx("", ""));
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-ph-obs", "check:acc", "t_psv_ok").to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(class(&s), "protected");
}

/// PSV-6 (C9 dev review round 1; A54): A49 for an EXECUTION. An
/// `interpreter_run` on a protected host launched with no manifest, no nonce
/// and no observation, even when the host HAD an observer, and Fabric then
/// attested it as a protected execution that EVL counts. Now the protected
/// profile runs only the observed check path: with or without an observer
/// section the execution is refused before anything is reserved, the
/// observer never runs, no nonce is issued, nothing launches, and nothing
/// would be attested. Control: the same host with an observer still runs an
/// operator-suite check, observed and protected.
#[test]
fn a_protected_host_launches_no_execution_with_or_without_an_observer() {
    for with_observer in [false, true] {
        let w = World::new();
        let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
        let mut cfg = w.protected_cfg();
        let lx = w.lx("", "");
        let out_root = lx.out_root.clone();
        cfg.linux = Some(lx);
        if with_observer {
            cfg.observer = Some(w.observer("", &key, "observer"));
        }
        let nonces = w.env.dir.path().join("custodian-nonces");
        let op = format!("op-ph-exec-{with_observer}");
        let mut r = w.request(&op, "f.ax", "t_psv_ok");
        r["job_kind"] = json!("interpreter_run");
        r["argv"] = json!(["f.ax"]);
        let text = r.to_string();
        let got = submit(&text, &cfg);
        let launched_any = std::fs::read_dir(&out_root).unwrap().next().is_some();
        assert!(
            !launched_any && w.env.launch_records() == 0,
            "ATTACK: an interpreter_run launched on a protected host (observer section: \
             {with_observer}) with no launch manifest, nonce or observation: {got:?}"
        );
        let s = got.expect("refused as unsupported, not an error");
        assert_eq!(
            s.receipt.status,
            ReceiptStatus::Unsupported,
            "{:?}",
            s.reason
        );
        assert!(s.ran_under.is_none(), "nothing ran");
        assert!(s.psv_evidence.is_none());
        assert!(
            !w.env.dir.path().join("prev-observation.json").exists(),
            "the observer never ran"
        );
        assert!(
            std::fs::read_dir(&nonces).map_or(true, |mut d| d.next().is_none()),
            "no nonce was issued"
        );
        let req = axon_loop_contracts::parse(&text).unwrap();
        assert!(
            axon_fabric::signing::execution_attestation_decision(
                &req,
                &s.receipt,
                s.replayed,
                s.ran_under.as_ref()
            )
            .is_err(),
            "ATTACK: Fabric would attest the refused execution"
        );
    }
    // Control: the observed check path on the same kind of host.
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(w.lx("", ""));
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-ph-exec-ctl", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert_eq!(class(&s), "protected");
}

/// The protected profile's declaration offers no execution: `select` never
/// picks it for an `interpreter_run` (PSV-6, A54), whatever the evidence.
#[test]
fn the_protected_profile_is_never_selected_for_an_execution() {
    let w = World::new();
    let lx = w.lx("", "");
    let mut r = w.request("op-sel-exec", "f.ax", "t_psv_ok");
    r["job_kind"] = json!("interpreter_run");
    r["argv"] = json!(["f.ax"]);
    let req: axon_loop_contracts::ComputeRequest =
        axon_loop_contracts::parse(&r.to_string()).unwrap();
    let got = backend::select(&req, Some(&lx), backend::AuthorityNeeds::default());
    assert!(
        got.is_err(),
        "ATTACK: the protected profile was selected for an interpreter_run: {:?}",
        got.map(|p| p.id)
    );
    // Control: the same request as an operator-suite check is selected.
    let req: axon_loop_contracts::ComputeRequest =
        axon_loop_contracts::parse(&w.request("op-sel-chk", "check:acc", "t_psv_ok").to_string())
            .unwrap();
    assert_eq!(
        backend::select(&req, Some(&lx), backend::AuthorityNeeds::default())
            .unwrap()
            .id,
        backend::LINUX_MICROVM_PROTECTED.id
    );
}

/// PSV-4 (C9 certifying review): an OBSERVED launch whose verdict is not
/// protected (here a forged pass, which derives `guest-unobserved` Unknown)
/// carries NO `axon-psv-evidence/1` bundle. Intake refuses a bundle beside a
/// receipt that claims no protected evidence, so emitting one turned an
/// honest unknown into a refused verification. Control: the observed genuine
/// pass is protected and carries its bundle.
#[test]
fn an_observed_verdict_that_is_not_protected_carries_no_bundle() {
    for tamper in ["forge", "other-manifest", "vmm-died"] {
        let w = World::new();
        let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
        let mut cfg = w.env.cfg(0);
        cfg.linux = Some(w.lx(tamper, ""));
        cfg.observer = Some(w.observer("", &key, "observer"));
        let op = format!("op-obs-np-{tamper}");
        let s = submit(&w.request(&op, "check:acc", "t_psv_ok").to_string(), &cfg).unwrap();
        assert!(launched(&w, &op), "{tamper}: the observed launch ran");
        assert_ne!(class(&s), "protected", "{tamper}");
        assert_ne!(
            s.receipt.verification,
            ReceiptVerification::Passed,
            "{tamper}"
        );
        assert!(
            s.psv_evidence.is_none(),
            "{tamper}: a bundle travels with a verdict that is not protected ({})",
            class(&s)
        );
    }
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let s = w.submit_observed(w.observer("", &key, "observer"), "op-obs-np-ok");
    assert_eq!(class(&s), "protected", "{:?}", s.reason);
    assert!(s.psv_evidence.is_some(), "a protected verdict carries it");
}

/// PSV-4 / PSV-5 (C9 dev review round 1; A55): the bundle is decided from the
/// FINAL receipt. Each launch here is OBSERVED and returns a GENUINE, keyed
/// pass, so `derive` alone says protected; the launch itself was not
/// admissible (the launcher could not bind it, could not clean up, or the
/// runner died after the verdict), so `psv_receipt` downgrades it to
/// guest-unobserved. The bundle used to be taken from `derive`'s class, before
/// that downgrade, and so shipped beside a receipt that claims nothing
/// protected. Control: the admissible observed launch keeps its bundle
/// (`an_observed_verdict_that_is_not_protected_carries_no_bundle`).
#[test]
fn an_inadmissible_observed_launch_is_never_protected_and_carries_no_bundle() {
    for tamper in ["unbound", "cleanup-incomplete", "runner-died-after-verdict"] {
        let w = World::new();
        let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
        let mut cfg = w.env.cfg(0);
        cfg.linux = Some(w.lx(tamper, ""));
        cfg.observer = Some(w.observer("", &key, "observer"));
        let op = format!("op-obs-inadm-{tamper}");
        let text = w.request(&op, "check:acc", "t_psv_ok").to_string();
        let s = submit(&text, &cfg).unwrap();
        assert!(launched(&w, &op), "{tamper}: the observed launch ran");
        let got = class(&s);
        let ran = s.ran_under.as_ref().map(|r| r.evidence_class.clone());
        assert_eq!(
            got,
            "guest-unobserved",
            "ATTACK: {tamper}: an inadmissible observed launch was classed {got} (ran_under \
             {ran:?}, bundle {})",
            s.psv_evidence.is_some()
        );
        assert_eq!(ran.as_deref(), Some("guest-unobserved"), "{tamper}");
        assert!(
            !axon_loop_contracts::protected_evidence::claims_protected(&s.receipt),
            "{tamper}"
        );
        assert!(
            s.psv_evidence.is_none(),
            "ATTACK: {tamper}: a bundle travels beside a receipt downgraded to guest-unobserved"
        );
        assert_ne!(
            s.receipt.verification,
            ReceiptVerification::Passed,
            "{tamper}"
        );
    }
}

/// PSV-6 key-role separation at Fabric (C9 dev review round 1; A57). The
/// observation is genuinely signed by a key in the observer root, but that
/// key is ALSO the host's attestation signer (whose private half Fabric
/// holds), or ALSO in another operator authority root: either way Fabric
/// could mint the observation itself. `observe` refuses, whatever the host
/// config said when it loaded (the roots are read at every observation), and
/// nothing launches. Control: the same observer with a key in no other role
/// launches, observed and protected.
#[test]
fn an_observer_key_that_holds_another_role_is_refused_at_every_observation() {
    for role in [
        "host-signer",
        "verifier",
        "qualification",
        "admission",
        "monitor",
    ] {
        let w = World::new();
        let d = w.env.dir.path().to_path_buf();
        let key = observer_key(&d, "obs", &[&w.observer_roots()]);
        let mut ob = w.observer("", &key, "observer");
        if role == "host-signer" {
            ob.trust.host_signer_public_key = Some(key.public_hex.clone());
        } else {
            let other = ob
                .trust
                .separate_from
                .iter()
                .find(|(a, _)| a.dir_name() == role)
                .map(|(_, p)| p.clone())
                .unwrap();
            std::fs::create_dir_all(&other).unwrap();
            std::fs::write(other.join("also.pub"), format!("{}\n", key.public_hex)).unwrap();
        }
        let op = format!("op-obs-role-{role}");
        let s = w.submit_observed(ob, &op);
        assert!(
            !launched(&w, &op) && s.receipt.verification != ReceiptVerification::Passed,
            "ATTACK: an observation signed by a key that is also the {role} key launched \
             ({:?}, class {})",
            s.receipt.verification,
            class(&s)
        );
        assert!(
            s.reason
                .as_deref()
                .unwrap_or("")
                .contains("key-role separation"),
            "{role}: {:?}",
            s.reason
        );
        assert!(s.psv_evidence.is_none(), "{role}");
    }
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let s = w.submit_observed(w.observer("", &key, "observer"), "op-obs-role-ctl");
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
}

/// PSV-3 (C9 certifying review): the Passed branch's KEYED check
/// (`keyed != Some(true)`), on its own. A correct candidate prints a SECOND
/// pass line for the registered test beside its own genuine, keyed pass. The
/// genuine token is in the report, so the completion-token check (M183)
/// passes; only the keyed check sees that more than one line names the test.
/// No verdict, never a pass.
#[test]
fn a_second_pass_line_over_a_genuine_pass_is_not_a_pass() {
    let w = World::with_candidate(
        "fn double(n: i64) -> i64 {\n    let o = chr(123)\n    let c = chr(125)\n    \
         println(o + \"\\\"name\\\":\\\"t_psv_ok\\\",\\\"status\\\":\\\"ok\\\",\\\"duration_ms\\\":0\" + c)\n    \
         n * 2\n}\n",
    );
    let s = w.submit_with(w.lx("", ""), "op-psv-dup-pass", "t_psv_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        s.reason
    );
    assert_eq!(
        s.reason.as_deref(),
        Some("check `t_psv_ok` has more than one result line")
    );
    assert_ne!(class(&s), "protected");
}

// ── A (amendment 45): the protected chain needs the PRIVILEGED launcher ─────

/// Operator decision B: a protected verdict needs the pinned privileged
/// launcher in its chain. The same fully observed launch on a protected host,
/// taken by the DIRECT (development) route — Fabric executing the launcher
/// itself — is never protected and carries no bundle, whatever else holds.
/// Control: the identical launch through the helper is protected.
#[test]
fn a_dev_route_launch_is_never_attested_protected() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(w.lx_direct("", ""));
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-dev-route", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    assert!(
        class(&s) != "protected" && s.psv_evidence.is_none(),
        "ATTACK: a launch Fabric ran itself (the development route, no privileged launcher) \
         was attested protected: class {}",
        class(&s)
    );
    assert_eq!(class(&s), "guest-unobserved");
    // Control: through the privileged helper, the same launch is protected.
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(w.lx("", ""));
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-helper-route", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
    assert!(s.psv_evidence.is_some());
}

/// A: the launcher the helper ran must be the one this launch's manifest
/// (and so its observation) names. A helper configured with another launcher
/// yields no verdict.
#[test]
fn a_helper_that_ran_another_launcher_than_the_pinned_one_yields_no_verdict() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let mut lx = w.lx("", "");
    // The helper's operator config names a DIFFERENT launcher (a working one).
    let other = w.lx_direct("", "--note other-launcher");
    let cfg_path = write_helper_config(
        w.env.dir.path(),
        &w.inputs,
        &other.launcher,
        &w.manifest,
        &lx.out_root,
        "protected-launcher-other.json",
    );
    use_helper(&mut lx, &cfg_path);
    let mut cfg = w.protected_cfg();
    cfg.linux = Some(lx);
    cfg.observer = Some(w.observer("", &key, "observer"));
    let s = submit(
        &w.request("op-other-launcher", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert!(
        s.receipt.verification != ReceiptVerification::Passed && class(&s) != "protected",
        "ATTACK: a launch by a launcher other than the one the manifest pins was accepted: {:?} \
         class {}",
        s.receipt.verification,
        class(&s)
    );
    assert!(s.reason.unwrap_or_default().contains("not the pinned"));
}

/// D6 / amendment 50: a Fabric that keeps its own nonces (the in-process
/// development custodian) never reaches a root launch: the privileged
/// launcher spends only through the custodian its operator config names,
/// which never issued them. Control: the custodian service's nonce launches
/// and is protected.
#[test]
fn a_nonce_fabric_issued_itself_never_authorizes_a_root_launch() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let mut ob = w.observer("", &key, "observer");
    ob.custodian = Custodian::InProcess(NonceStore {
        dir: w.env.dir.path().join("fabric-own-nonces"),
    });
    let s = w.submit_observed(ob, "op-own-nonce");
    assert!(
        !launched(&w, "op-own-nonce") && class(&s) != "protected",
        "ATTACK: a nonce Fabric issued itself (the in-process dev custodian) authorized a root \
         launch ({:?}, class {})",
        s.receipt.verification,
        class(&s)
    );
    assert!(
        s.reason
            .clone()
            .unwrap_or_default()
            .contains("never issued"),
        "{:?}",
        s.reason
    );
    let s = w.submit_observed(w.observer("", &key, "observer"), "op-service-nonce");
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
}

/// PSV-4 (M775; C9 round 4 fix wave, rows2): a check the guest ran but that
/// produced NO result line (the suite does not define the requested test) is
/// never receipted as a verdict. The CheckVerdict::NotRun arm is the only
/// refusal: the run exited, the verdict is bound and joined, so every other
/// check passes it. Control: the defined test passes.
#[test]
fn a_check_that_produced_no_verdict_is_never_receipted_as_one() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-psv-notrun", "t_psv_absent");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Unknown,
        "ATTACK: a check that produced no verdict was receipted as {:?}",
        s.receipt.verification
    );
    assert!(
        s.reason
            .as_deref()
            .unwrap_or("")
            .contains("produced no verdict"),
        "{:?}",
        s.reason
    );
    let s = w.submit_with(w.lx("", ""), "op-psv-notrun-ok", "t_psv_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "control: {:?}",
        s.reason
    );
}

// ── C9 round 4b, rows4b (amendment 62): the privileged helper's REPORT and
// the protected executable ──────────────────────────────────────────────────
//
// Fabric acts on the root helper's report: it must name this protocol
// (schema), say the launch finished (exit EXIT_LAUNCHED, no error), and say
// that neither the launcher nor its interpreter changed, and the helper Fabric
// verified must itself be unchanged. Round 4b (EQUIVALENCE) removed all five
// refusals at once with the whole axon-fabric suite green. Each test below
// reaches ONE of them through submit() on the helper route, with a control
// through the same machinery that is protected.

/// A stand-in for the privileged helper: an ELF trampoline (the helper route
/// execs a verified BINARY; a script is refused) that runs the REAL test-trust
/// helper, passes its report through `sed`, exits with `exit` (else the
/// helper's own status), and with `touch_self` changes its own mode (and so its
/// ctime) while the launch is in flight. The mode really changes: this
/// host's chmod skips the syscall when the mode is already the one asked.
fn forging_helper(
    w: &World,
    name: &str,
    sed: &str,
    exit: Option<i32>,
    touch_self: bool,
) -> axon_fabric::sealed_exec::Pinned {
    let d = w.env.dir.path();
    let tramp = d.join(format!("helper-{name}"));
    let script = d.join(format!("helper-{name}.sh"));
    let rc = exit.map_or_else(|| "$rc".to_string(), |c| c.to_string());
    let touch = if touch_self {
        format!("/bin/chmod 0750 '{}'\n", tramp.display())
    } else {
        String::new()
    };
    std::fs::write(
        &script,
        format!(
            "out=$('{}' \"$@\")\nrc=$?\n{touch}printf '%s\\n' \"$out\" | /bin/sed -e '{sed}'\nexit {rc}\n",
            helper_pin().path.display()
        ),
    )
    .unwrap();
    let c = d.join(format!("helper-{name}.c"));
    std::fs::write(
        &c,
        format!(
            "#include <unistd.h>\nint main(int argc, char **argv) {{\n  char *a[64]; int n = 0;\n  \
             a[n++] = \"sh\"; a[n++] = \"{}\";\n  for (int i = 1; i < argc && n < 63; i++) a[n++] = \
             argv[i];\n  a[n] = 0;\n  execv(\"/bin/sh\", a);\n  return 127;\n}}\n",
            script.display()
        ),
    )
    .unwrap();
    let st = std::process::Command::new("cc")
        .arg("-o")
        .arg(&tramp)
        .arg(&c)
        .status()
        .expect("cc: the stand-in helper is compiled by the test");
    assert!(st.success(), "the stand-in helper did not compile");
    axon_fabric::sealed_exec::Pinned {
        sha256: sha256_file(&tramp),
        path: tramp,
    }
}

impl World {
    /// The helper route with the helper replaced by `helper`.
    fn lx_helper(&self, helper: axon_fabric::sealed_exec::Pinned) -> LinuxProfileConfig {
        let mut lx = self.lx("", "");
        lx.privileged.as_mut().unwrap().helper = helper;
        lx
    }
}

/// Submit on the helper route through `helper`; the attack is a verdict that
/// is Passed or protected.
fn forged_report(
    w: &World,
    helper: axon_fabric::sealed_exec::Pinned,
    op: &str,
    what: &str,
) -> axon_fabric::Submission {
    let s = w.submit_with(w.lx_helper(helper), op, "t_psv_ok");
    assert!(
        s.receipt.verification != ReceiptVerification::Passed && class(&s) != "protected",
        "ATTACK: {what}, and Fabric derived a verdict from the launch: {:?} class {}",
        s.receipt.verification,
        class(&s)
    );
    s
}

/// Control for every forged-report test: the trampoline passing the real
/// helper's report through unchanged gives the protected verdict.
fn pass_through_helper_is_protected(w: &World, op: &str) {
    let s = w.submit_with(
        w.lx_helper(forging_helper(w, &format!("same-{op}"), "", None, false)),
        op,
        "t_psv_ok",
    );
    assert_eq!(
        (s.receipt.verification, class(&s).as_str()),
        (ReceiptVerification::Passed, "protected"),
        "control: the pass-through stand-in helper: {:?}",
        s.reason
    );
}

#[test]
fn a_helper_report_of_another_schema_yields_no_verdict() {
    let w = World::new();
    pass_through_helper_is_protected(&w, "op-rep-schema-ok");
    let h = forging_helper(
        &w,
        "schema",
        "s#axon-protected-launch-report/1#axon-protected-launch-report/0#",
        None,
        false,
    );
    let s = forged_report(
        &w,
        h,
        "op-rep-schema",
        "the privileged helper's report names another schema",
    );
    assert!(
        s.reason.unwrap_or_default().contains("has another schema"),
        "refused for the schema"
    );
}

#[test]
fn a_helper_error_after_the_launch_yields_no_verdict() {
    let w = World::new();
    pass_through_helper_is_protected(&w, "op-rep-err-ok");
    // The report says the launch began and then failed (the out dir could
    // not be handed over); the helper's exit is still EXIT_LAUNCHED.
    let h = forging_helper(
        &w,
        "error",
        "s#\"error\":null#\"error\":\"the out dir could not be handed over: forged\"#",
        None,
        false,
    );
    let s = forged_report(
        &w,
        h,
        "op-rep-err",
        "the privileged helper reported an error after the launch",
    );
    assert!(
        s.reason.unwrap_or_default().contains("after the launch"),
        "refused for the post-launch error"
    );
}

#[test]
fn a_helper_exit_other_than_launched_yields_no_verdict() {
    let w = World::new();
    pass_through_helper_is_protected(&w, "op-rep-exit-ok");
    // The report is clean; the helper exits EXIT_UNKNOWN.
    let h = forging_helper(
        &w,
        "exit",
        "",
        Some(axon_fabric::privileged_launcher::EXIT_UNKNOWN),
        false,
    );
    let s = forged_report(
        &w,
        h,
        "op-rep-exit",
        "the privileged helper exited EXIT_UNKNOWN after the launch",
    );
    assert!(
        s.reason.unwrap_or_default().contains("after the launch"),
        "refused for the exit status"
    );
}

/// D (same bytes): the launcher changes its own inode during the
/// `--verify-result` run (a new mode and ctime), so the REAL helper reports
/// `unchanged: false`. The bytes that ran are no longer the bytes pinned.
#[test]
fn a_launcher_that_changed_during_the_launch_yields_no_verdict() {
    let w = World::new();
    let d = w.env.dir.path();
    let mut lx = w.lx_direct("", "--note self-changing");
    let guest = format!(
        "{} __psv-host-guest --axon {} --tamper ''",
        env!("CARGO_BIN_EXE_axon-fabric"),
        axon_bin().display()
    );
    let script = d.join("psv-launcher-self-changing.sh");
    write_executable(
        &script,
        format!(
            "#!/bin/sh\nif [ \"$1\" = --verify-result ]; then\n  {guest} \"$@\"\n  rc=$?\n  \
             /bin/chmod 0750 '{}'\n  exit $rc\nfi\nexec {guest} \"$@\"\n",
            script.display()
        ),
        0o755,
    );
    set_launcher(&mut lx, script.clone());
    let cfg = write_helper_config(
        d,
        &w.inputs,
        &lx.launcher,
        &w.manifest,
        &lx.out_root,
        "protected-launcher-self-changing.json",
    );
    use_helper(&mut lx, &cfg);
    let s = w.submit_with(lx, "op-launcher-changed", "t_psv_ok");
    assert!(
        s.receipt.verification != ReceiptVerification::Passed && class(&s) != "protected",
        "ATTACK: the launcher changed during the launch (the helper reported unchanged=false), \
         and Fabric derived a verdict from it: {:?} class {}",
        s.receipt.verification,
        class(&s)
    );
    assert!(
        s.reason
            .unwrap_or_default()
            .contains("changed during the launch"),
        "refused for the change"
    );
    let s = w.submit_with(w.lx("", ""), "op-launcher-changed-ok", "t_psv_ok");
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
}

/// D: the helper Fabric verified and executed changes during the launch; its
/// report (passed through untouched) cannot speak for bytes that are no
/// longer the ones verified.
#[test]
fn a_helper_that_changed_during_the_launch_yields_no_verdict() {
    let w = World::new();
    pass_through_helper_is_protected(&w, "op-helper-changed-ok");
    let h = forging_helper(&w, "self-changing", "", None, true);
    let s = forged_report(
        &w,
        h,
        "op-helper-changed",
        "the privileged helper changed during the launch",
    );
    assert!(
        s.reason
            .unwrap_or_default()
            .contains("changed during the launch"),
        "refused for the change"
    );
}

/// The protected profile runs ONE executable, the interpreter in the
/// qualified rootfs: a request naming another executable id (with the digest
/// that id would have) is refused, so no receipt names an executable the
/// guest did not run. Control: the same request naming the guest interpreter.
#[test]
fn a_protected_request_naming_another_executable_is_refused() {
    let w = World::new();
    let mut cfg = w.env.cfg(0);
    cfg.observer = Some(w.auto_observer());
    cfg.linux = Some(w.lx("", ""));
    let mut r = w.request("op-other-exe", "check:acc", "t_psv_ok");
    r["registered_executable_ref"] = json!("axon");
    r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest("axon", GUEST));
    match submit(&r.to_string(), &cfg) {
        Ok(s) => panic!(
            "ATTACK: a protected request naming executable `axon` was run: {:?} class {}",
            s.receipt.verification,
            class(&s)
        ),
        Err(e) => assert!(
            e.to_string().contains("runs only"),
            "refused for the executable id: {e}"
        ),
    }
    let s = submit(
        &w.request("op-other-exe-ok", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
}

/// ...and its executable_digest must be the qualified guest interpreter's.
#[test]
fn a_protected_request_with_another_executable_digest_is_refused() {
    let w = World::new();
    let mut cfg = w.env.cfg(0);
    cfg.observer = Some(w.auto_observer());
    cfg.linux = Some(w.lx("", ""));
    let mut r = w.request("op-other-digest", "check:acc", "t_psv_ok");
    r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        backend::LINUX_GUEST_AXON_ID,
        &"ab".repeat(32)
    ));
    match submit(&r.to_string(), &cfg) {
        Ok(s) => panic!(
            "ATTACK: a protected request naming another guest interpreter digest was run: {:?} \
             class {}",
            s.receipt.verification,
            class(&s)
        ),
        Err(e) => assert!(
            e.to_string()
                .contains("does not match the qualified guest interpreter"),
            "refused for the digest: {e}"
        ),
    }
    let s = submit(
        &w.request("op-other-digest-ok", "check:acc", "t_psv_ok")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
}

/// The operator suite judges only at the version the operator registered: a
/// suite edited on disk after registration (here, a lenient test under the
/// same name) is refused, and nothing is launched. Control: the registered
/// suite is protected.
#[test]
fn a_suite_edited_after_registration_never_judges() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-suite-edit-ok", "t_psv_ok");
    assert_eq!(class(&s), "protected", "control: {:?}", s.reason);
    let suite = w.env.dir.path().join("suites/acc/accept.ax");
    std::fs::write(
        &suite,
        SUITE.replace("assert_eq(double(1), 3)", "assert_eq(double(1), 2)"),
    )
    .unwrap();
    let mut cfg = w.env.cfg(0);
    cfg.observer = Some(w.auto_observer());
    cfg.linux = Some(w.lx("", ""));
    match submit(
        &w.request("op-suite-edit", "check:acc", "t_psv_fail")
            .to_string(),
        &cfg,
    ) {
        Ok(s) => panic!(
            "ATTACK: a suite edited after its registration judged the candidate: {:?} class {}",
            s.receipt.verification,
            class(&s)
        ),
        Err(e) => assert!(
            e.to_string().contains("not the registered"),
            "refused for the edit: {e}"
        ),
    }
}

/// argv is `[check:<id>, test]` and nothing else: an element the run would
/// ignore is refused, so no attested request carries an argument that did
/// not take part in the run.
#[test]
fn a_protected_check_with_an_extra_argument_is_refused() {
    let w = World::new();
    let mut cfg = w.env.cfg(0);
    cfg.observer = Some(w.auto_observer());
    cfg.linux = Some(w.lx("", ""));
    let mut r = w.request("op-argv-extra", "check:acc", "t_psv_ok");
    r["argv"] = json!(["check:acc", "t_psv_ok", "; sh evil.ax"]);
    match submit(&r.to_string(), &cfg) {
        Ok(s) => panic!(
            "ATTACK: a check whose argv carried an element the run ignored was run: {:?} class {}",
            s.receipt.verification,
            class(&s)
        ),
        Err(e) => assert_eq!(e.kind(), "malformed", "{e}"),
    }
}
