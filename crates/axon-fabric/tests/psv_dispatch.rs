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
        let env = Env::new();
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
        std::fs::write(&manifest, text).unwrap();
        World {
            env,
            issuer: Issuer::generate(),
            candidate,
            manifest,
        }
    }
    fn lx(&self, tamper: &str, extra: &str) -> LinuxProfileConfig {
        let d = self.env.dir.path();
        let mut lx = qualified_linux_cfg(
            d,
            &self.issuer,
            &good_evidence(&sha256_file(&self.manifest)),
        );
        let script = d.join(format!("psv-launcher-{tamper}.sh"));
        std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nexec {} __psv-host-guest --axon {} --tamper '{tamper}' {extra} \"$@\"\n",
                env!("CARGO_BIN_EXE_axon-fabric"),
                axon_bin().display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
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
        cfg.linux = Some(lx);
        submit(&self.request(op, "check:acc", test).to_string(), &cfg).unwrap()
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
    let s = w.submit_with(w.lx("", ""), "op-psv-ok", "t_psv_ok");
    assert_eq!(s.backend, Some("linux-microvm-protected"));
    assert_eq!(s.receipt.status, ReceiptStatus::Completed, "{:?}", s.reason);
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    // Never protected without an observation (A14).
    assert_eq!(class(&s), "guest-unobserved");
    assert_eq!(
        s.ran_under.as_ref().unwrap().evidence_class,
        "guest-unobserved"
    );
    let r = refs(&s);
    for want in [
        "launch-manifest-sha256:",
        "guest-verdict-sha256:",
        &format!("guest-axon-sha256:{GUEST}"),
        &format!("guest-kernel-sha256:{}", "1".repeat(64)),
        &format!("guest-rootfs-sha256:{}", "2".repeat(64)),
        "suite:acc@acf1:",
    ] {
        assert!(
            r.iter().any(|e| e.starts_with(want)),
            "missing {want}: {r:?}"
        );
    }
    assert!(
        r.iter().any(|e| e.ends_with("#accept.ax/t_psv_ok")),
        "{r:?}"
    );
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
    // says failed.
    let s = w.submit_with(w.lx("claim-pass", ""), "op-psv-claim", "t_psv_fail");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "{:?}",
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
    ] {
        let w = World::new();
        let s = w.submit_with(w.lx(tamper, ""), &format!("op-psv-{tamper}"), "t_psv_ok");
        assert_eq!(
            s.receipt.verification,
            ReceiptVerification::Unknown,
            "{tamper}"
        );
        let reason = s.reason.clone().unwrap_or_default();
        assert!(reason.contains(why), "{tamper}: {reason}");
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
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    assert!(s.reason.unwrap().contains("without completion evidence"));
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
    cfg.linux = Some(lx);
    let e = submit(
        &w.request("op-psv-cand", "f.ax", "t_psv_ok").to_string(),
        &cfg,
    )
    .unwrap_err();
    assert!(e.to_string().contains("only as an operator suite"), "{e}");
    assert_eq!(w.env.launch_records(), 0);
    assert!(
        std::fs::read_dir(&launches).unwrap().next().is_none(),
        "nothing launched"
    );
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

/// Passed needs exit 0: the named test passing (with a VALID token) while a
/// sibling the substring filter also ran fails is Unknown.
#[test]
fn a_valid_token_with_a_failing_run_is_not_a_pass() {
    let w = World::new();
    let s = w.submit_with(w.lx("", ""), "op-psv-pair", "t_psv_pair");
    assert_eq!(s.receipt.verification, ReceiptVerification::Unknown);
    let r = s.reason.unwrap();
    assert!(r.contains("passed but the run exited"), "{r}");
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
