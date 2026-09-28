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
    // The receipt is a valid acf-execution-receipt: intake parses it
    // (matched_checks >= 1 for a pass; review wf_d725935a-7ed).
    let rt = serde_json::to_string(&s.receipt).unwrap();
    let back: axon_loop_contracts::ExecutionReceipt =
        axon_loop_contracts::parse(&rt).expect("the guest-path receipt parses as a contract");
    assert_eq!(back.matched_checks, Some(1));
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
#[test]
fn the_verify_step_inherits_nothing_from_the_caller() {
    std::env::set_var("PSV_VERIFY_ENV_PROBE", "leak");
    let w = World::new();
    let lx = w.lx("", "");
    let out_root = lx.out_root.clone();
    let s = w.submit_with(lx, "op-psv-env", "t_psv_ok");
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        s.reason
    );
    let seen = std::fs::read_to_string(out_root.join("op-psv-env/verify-env-leaked")).unwrap();
    assert_eq!(seen, "no");
    // The per-attempt secret is already gone when the verify step runs.
    let secret =
        std::fs::read_to_string(out_root.join("op-psv-env/verify-secret-present")).unwrap();
    assert_eq!(
        secret, "no",
        "the secret outlived the launch into the verify step"
    );
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

// ── M3: the preflight observation ───────────────────────────────────────────
//
// The stand-in observer composes the observation FROM the launch manifest and
// signs it with `axon-fabric sign-evidence` (the operator tool). It proves the
// PROTOCOL — domain, root, joins, freshness, nonce — never a measurement.

use axon_fabric::backend::Clock;
use axon_fabric::observer::{NonceStore, ObserverConfig, ObserverTrust};

struct ObserverKey {
    pk8: PathBuf,
    key_id: String,
}

fn observer_key(dir: &Path, name: &str, trust_in: &[&Path]) -> ObserverKey {
    use ring::signature::KeyPair;
    let rng = ring::rand::SystemRandom::new();
    let doc = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
    let kp = ring::signature::Ed25519KeyPair::from_pkcs8(doc.as_ref()).unwrap();
    let pk8 = dir.join(format!("{name}.pk8"));
    std::fs::write(&pk8, doc.as_ref()).unwrap();
    let hexpk = hex(kp.public_key().as_ref());
    for d in trust_in {
        std::fs::create_dir_all(d).unwrap();
        std::fs::write(d.join(format!("{name}.pub")), format!("{hexpk}\n")).unwrap();
    }
    use sha2::{Digest, Sha256};
    let key_id = format!(
        "ed25519:{}",
        &hex(&Sha256::digest(kp.public_key().as_ref()))[..16]
    );
    ObserverKey { pk8, key_id }
}

impl World {
    fn observer_roots(&self) -> PathBuf {
        self.env.dir.path().join("observer_keys")
    }
    /// An observer program: `mode` applies ONE defect; `key` signs, as
    /// `authority`.
    fn observer(&self, mode: &str, key: &ObserverKey, authority: &str) -> ObserverConfig {
        let d = self.env.dir.path();
        let script = d.join(format!("observer-{mode}-{authority}.sh"));
        std::fs::write(
            &script,
            format!(
                r#"#!/bin/sh
while [ $# -gt 0 ]; do case "$1" in --manifest) M="$2"; shift 2;; --out) O="$2"; shift 2;; *) shift;; esac; done
[ "{mode}" = exit ] && exit 1
if [ "{mode}" = replay ]; then
    cp "{prev}" "$O/observation.json" && cp "{prev}.sig" "$O/observation.json.sig"; exit $?
fi
python3 - "$M" "$O/observation.json" "{mode}" "{kid}" <<'PY'
import json, sys, hashlib, datetime
m_path, out, mode, kid = sys.argv[1:]
raw = open(m_path, "rb").read(); m = json.loads(raw)
now = datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
o = {{"schema": "axon-preflight-observation/1", "observer_key_id": kid,
     "nonce": m["observation_nonce"], "epoch": 0, "observed_at": now,
     "host_profile": m["backend_profile"], "fabric_revision": m["fabric_revision"],
     "firecracker_sha256": m["firecracker_sha256"], "launcher_sha256": m["launcher_sha256"],
     "host_config_sha256": m["host_config_sha256"], "guest": m["guest"],
     "verifier_sha256": m["verifier_sha256"],
     "suite_registry_sha256": m["suite"]["registry_sha256"], "policy_sha256": m["policy_sha256"],
     "intended_launch_manifest_sha256": hashlib.sha256(raw).hexdigest()}}
if mode == "stale": o["observed_at"] = "2020-01-01T00:00:00Z"
if mode == "other-manifest": o["intended_launch_manifest_sha256"] = "0" * 64
if mode == "kernel": o["guest"] = dict(o["guest"], kernel_sha256="9" * 64)
if mode == "epoch": o["epoch"] = 7
if mode == "nonce-forged": o["nonce"] = "ab" * 16
if mode == "claims-other-key": o["observer_key_id"] = "ed25519:0000000000000000"
if mode == "verifier": o["verifier_sha256"] = "7" * 64
json.dump(o, open(out, "w"))
PY
{fabric} sign-evidence --record "$O/observation.json" --key {key} --authority {authority} >/dev/null || exit 1
# Keep this genuine signed observation, so a later test can REPLAY it.
cp "$O/observation.json" "{prev}"; cp "$O/observation.json.sig" "{prev}.sig"
"#,
                kid = key.key_id,
                prev = d.join("prev-observation.json").display(),
                fabric = env!("CARGO_BIN_EXE_axon-fabric"),
                key = key.pk8.display(),
            ),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::os::unix::fs::PermissionsExt::from_mode(0o755))
            .unwrap();
        ObserverConfig {
            command_sha256: sha256_file(&script),
            command: script,
            trust: ObserverTrust::for_test(&self.observer_roots()),
            nonces: NonceStore {
                dir: d.join("custodian-nonces"),
            },
            max_age_s: 300,
            clock: Clock::System,
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

#[test]
fn a_verified_observation_makes_the_guest_verdict_protected() {
    let w = World::new();
    let key = observer_key(w.env.dir.path(), "obs", &[&w.observer_roots()]);
    let s = w.submit_observed(w.observer("", &key, "observer"), "op-obs-ok");
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
    axon_loop_contracts::protected_evidence::check_bundle(&req, &s.receipt, &bundle.to_string())
        .expect("the loop joins what Fabric launched and observed");
    // …and a single byte of the manifest changed breaks it.
    let mut bad = bundle.clone();
    bad["launch_manifest"] =
        serde_json::json!(format!("{} ", bad["launch_manifest"].as_str().unwrap()));
    assert!(axon_loop_contracts::protected_evidence::check_bundle(
        &req,
        &s.receipt,
        &bad.to_string()
    )
    .is_err());
    // The nonce was spent.
    let used = std::fs::read_dir(w.env.dir.path().join("custodian-nonces"))
        .unwrap()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "used"))
        .count();
    assert_eq!(used, 1);
}

/// Each defect refuses the LAUNCH (nothing runs), for its own reason, and is
/// never protected.
#[test]
fn every_defective_observation_refuses_the_launch() {
    for (mode, authority, key_in, why) in [
        // A9: another authority domain (the key IS a trusted observer).
        ("", "qualification", "observer", "is for authority"),
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
        // A nonce the manifest does not name fails the join before the store
        // (the store's own "never issued" is `a_nonce_authorizes_exactly_one_launch`).
        (
            "nonce-forged",
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
        ("exit", "observer", "observer", "observer exited"),
        // §7: the observer measures the INSTALLED verifier; another one refuses.
        ("verifier", "observer", "observer", "verifier_sha256"),
    ] {
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
            r.contains("preflight observation refused") && r.contains(why),
            "{op}: {r}"
        );
        assert!(!launched(&w, &op), "{op}: launched");
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
    assert!(st
        .consume("../x", 3, &c, 60)
        .unwrap_err()
        .contains("not one"));
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
    assert!(s.reason.unwrap().contains("not its pin"));
    assert!(!launched(&w, "op-obs-pin"));
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
    cfg.pre_launch_hook = Some(swap);
    let s = submit(
        &w.request("op-psv-b3", "check:acc", "t_psv_fail")
            .to_string(),
        &cfg,
    )
    .unwrap();
    assert_eq!(
        s.receipt.verification,
        ReceiptVerification::Failed,
        "{:?}",
        s.reason
    );
}
