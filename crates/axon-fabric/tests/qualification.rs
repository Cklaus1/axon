//! FG-042 / D-020 / ACF-G05 / G13-r22-profile-qualification: the Linux
//! microVM profile is eligible ONLY on issuer-signed, fresh, clean, complete
//! evidence. Each negative below differs from an accepted record in ONE fact,
//! is re-signed by the trusted issuer where the fact is inside the record (so
//! the signature rule cannot be what refuses it), and must be refused by
//! `select()` AND by `submit()` with no journal launch record and no launcher
//! run. The keys are generated here, per test; none is in the repository.

mod common;
use common::*;

use axon_fabric::backend::{self, LinuxProfileConfig};
use axon_fabric::submit;
use axon_loop_contracts::{ComputeRequest, ReceiptStatus};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

const GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

/// The protected profile runs only an operator-suite check (PSV-6, A54), so
/// qualification is exercised on one.
fn run_request(env: &Env, op: &str) -> Value {
    let candidate = psv_suite(env);
    let mut r = request(env, op, "t_psv_ok");
    as_protected_check(&mut r, &candidate, GUEST);
    r["grant_ref"] = json!("grant:open");
    r
}

/// A test world: an Env, a trusted issuer, a clean manifest, and a config
/// built from `evidence` (the caller mutates it first).
struct World {
    env: Env,
    issuer: Issuer,
    manifest_sha: String,
}

impl World {
    fn new() -> World {
        World::with_manifest(&full_lx_manifest(GUEST))
    }
    fn with_manifest(manifest: &str) -> World {
        let env = Env::new();
        let m = env.dir.path().join("manifest.json");
        std::fs::write(&m, manifest).unwrap();
        let manifest_sha = sha256_file(&m);
        World {
            env,
            issuer: Issuer::generate(),
            manifest_sha,
        }
    }
    fn evidence(&self) -> Value {
        good_evidence(&self.manifest_sha)
    }
    fn dir(&self) -> &Path {
        self.env.dir.path()
    }
    /// `evidence` signed by the trusted issuer.
    fn cfg(&self, evidence: &Value) -> LinuxProfileConfig {
        let mut lx = qualified_linux_cfg(self.dir(), &self.issuer, evidence);
        set_launcher(&mut lx, stand_in_launcher(&self.env, 0, true, true, 0));
        std::fs::create_dir_all(&lx.out_root).unwrap();
        lx
    }
    /// Write an `axon-b263-waiver/1` for `lx`'s evidence, signed by `by`.
    fn waive(&self, lx: &mut LinuxProfileConfig, by: &Issuer, waivers: Value, bind: Option<&str>) {
        let ev_sha = sha256_file(&lx.evidence);
        let p = self.dir().join("waivers.json");
        by.write_signed(
            &p,
            &json!({"schema":"axon-b263-waiver/1",
                    "evidence_sha256": bind.unwrap_or(&ev_sha),
                    "waivers": waivers}),
        );
        lx.waivers = Some(p);
    }
    fn req(&self, op: &str) -> ComputeRequest {
        axon_loop_contracts::parse::<ComputeRequest>(&run_request(&self.env, op).to_string())
            .unwrap()
    }
    fn launcher_runs(&self, lx: &LinuxProfileConfig) -> bool {
        lx.out_root.join("launches").exists()
    }

    /// Refused by `select()` AND by `submit()`: Unsupported receipt, no
    /// launch record in the journal, the launcher never ran.
    fn assert_refused(&self, lx: LinuxProfileConfig, why: &str) {
        let e = backend::select(&self.req("op-sel"), Some(&lx), Default::default())
            .expect_err("select must refuse");
        assert!(
            e.0.contains("linux-microvm-protected ineligible"),
            "{}",
            e.0
        );
        assert!(e.0.contains(why), "expected {why:?} in: {}", e.0);
        let mut cfg = self.env.cfg(0);
        cfg.linux = Some(lx.clone());
        let s = submit(&run_request(&self.env, "op-q").to_string(), &cfg).unwrap();
        assert_eq!(
            s.receipt.status,
            ReceiptStatus::Unsupported,
            "{:?}",
            s.reason
        );
        assert!(
            s.reason.as_deref().unwrap_or("").contains(why),
            "{:?}",
            s.reason
        );
        assert_eq!(s.backend, None);
        assert_eq!(self.env.launch_records(), 0, "no journal launch record");
        assert!(!self.launcher_runs(&lx), "the launcher never ran");
    }

    fn assert_accepted(&self, lx: LinuxProfileConfig) -> axon_fabric::Submission {
        let q = lx.qualification().expect("qualified");
        assert_eq!(q.caveat, TEST_CAVEAT);
        assert_eq!(
            backend::select(&self.req("op-sel"), Some(&lx), Default::default())
                .unwrap()
                .id,
            "linux-microvm-protected"
        );
        let mut cfg = self.env.cfg(0);
        cfg.linux = Some(lx.clone());
        let s = submit(&run_request(&self.env, "op-q").to_string(), &cfg).unwrap();
        assert_eq!(s.receipt.status, ReceiptStatus::Completed, "{:?}", s.reason);
        assert_eq!(self.env.launch_records(), 1);
        assert!(self.launcher_runs(&lx));
        let ev: Vec<&str> = s.receipt.evidence_refs.iter().map(|e| e.as_str()).collect();
        // The D2 caveat, the issuer and the signed record travel with the receipt.
        assert!(
            ev.contains(&format!("qualification-caveat:{TEST_CAVEAT}").as_str()),
            "{ev:?}"
        );
        assert!(ev.contains(&format!("qualification-issuer:{}", q.issuer).as_str()));
        assert!(
            ev.contains(&format!("qualification-evidence-sha256:{}", q.evidence_sha256).as_str())
        );
        s
    }
}

fn x3_blocked(ev: &mut Value) {
    ev["assertions"][2]["status"] = json!("BLOCKED");
    ev["counts"]["PASS"] = json!(2);
    ev["counts"]["BLOCKED"] = json!(1);
    ev["result"] = json!("PASS_WITH_BLOCKED");
}

fn x3_waiver(expires: &str, reason: &str) -> Value {
    json!([{"assertion":"x3_l0_hypervisor_boundary","reason":reason,"expires":expires}])
}

// ── accepted ────────────────────────────────────────────────────────────────

#[test]
fn a_signed_fresh_clean_pass_record_is_accepted_and_its_caveat_reaches_the_receipt() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    let q = lx.qualification().unwrap();
    assert!(q.issuer.starts_with("ed25519:"));
    assert_eq!(q.host, "WSL2-nested");
    assert_eq!(q.firecracker_sha256, TEST_FC_SHA);
    assert!(q.waived.is_empty());
    w.assert_accepted(lx);
}

#[test]
fn matching_manifest_engine_pins_are_accepted() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m["engine"] = json!({"firecracker_sha256": TEST_FC_SHA, "jailer_sha256": TEST_JAILER_SHA});
    let w = World::with_manifest(&m.to_string());
    w.assert_accepted(w.cfg(&w.evidence()));
}

#[test]
fn an_x3_blocked_record_under_an_issuer_signed_unexpired_waiver_is_accepted() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver(
            "2026-12-31T00:00:00Z",
            "operator decision D7: L0 accepted on WSL2",
        ),
        None,
    );
    let s = w.assert_accepted(lx);
    assert!(s
        .receipt
        .evidence_refs
        .iter()
        .any(|e| e.as_str() == "qualification-waived:x3_l0_hypervisor_boundary"));
}

// ── refused: signature and issuer ───────────────────────────────────────────

#[test]
fn an_unsigned_record_is_refused() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    std::fs::remove_file(sig_of(&lx.evidence)).unwrap();
    w.assert_refused(lx, "unsigned");
}

#[test]
fn a_signature_that_does_not_verify_is_refused() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    let sp = sig_of(&lx.evidence);
    let mut s: Value = serde_json::from_str(&std::fs::read_to_string(&sp).unwrap()).unwrap();
    let mut sig = s["signature"].as_str().unwrap().to_string();
    let flipped = if sig.starts_with('0') { "1" } else { "0" };
    sig.replace_range(0..1, flipped);
    s["signature"] = json!(sig);
    std::fs::write(&sp, s.to_string()).unwrap();
    w.assert_refused(lx, "does not verify");
}

#[test]
fn a_record_signed_by_an_untrusted_key_is_refused() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    // Re-sign the same bytes with a key nobody trusts.
    let rogue = Issuer::generate();
    let bytes = std::fs::read(&lx.evidence).unwrap();
    std::fs::write(sig_of(&lx.evidence), rogue.sign(&bytes)).unwrap();
    w.assert_refused(lx, "not a trusted evidence issuer");
}

#[test]
fn no_trusted_issuer_configured_refuses_even_a_signed_record() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    std::fs::remove_dir_all(&lx.trust.issuers_dir).unwrap();
    // Two rules refuse it, each alone (rows4b, four-cell record): no key
    // configured, and the signer not being a trusted issuer.
    w.assert_refused(lx, "trusted evidence issuer");
}

// ── refused: verdict ────────────────────────────────────────────────────────

#[test]
fn a_blocked_assertion_without_a_waiver_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    w.assert_refused(w.cfg(&ev), "not covered by an issuer-signed waiver");
}

#[test]
fn counts_blocked_1_disagreeing_with_the_assertions_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["counts"]["BLOCKED"] = json!(1);
    w.assert_refused(w.cfg(&ev), "disagrees with the");
}

#[test]
fn a_fail_count_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["counts"]["FAIL"] = json!(1);
    w.assert_refused(w.cfg(&ev), "FAIL assertions");
}

#[test]
fn a_record_with_no_pass_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["counts"]["PASS"] = json!(0);
    w.assert_refused(w.cfg(&ev), "no PASS assertion");
}

#[test]
fn a_result_other_than_pass_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["result"] = json!("PASS_WITH_BLOCKED");
    w.assert_refused(w.cfg(&ev), "only PASS");
}

// ── refused: waivers ────────────────────────────────────────────────────────

#[test]
fn an_expired_waiver_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver("2026-09-25T11:59:59Z", "D7"),
        None,
    );
    w.assert_refused(lx, "has expired");
}

#[test]
fn a_waiver_without_a_reason_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver("2026-12-31T00:00:00Z", " "),
        None,
    );
    w.assert_refused(lx, "states no reason");
}

#[test]
fn a_waiver_bound_to_other_evidence_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    let other = "e".repeat(64);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver("2026-12-31T00:00:00Z", "D7"),
        Some(&other),
    );
    w.assert_refused(lx, "not transferable");
}

#[test]
fn a_waiver_signed_by_an_untrusted_key_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &Issuer::generate(),
        x3_waiver("2026-12-31T00:00:00Z", "D7"),
        None,
    );
    w.assert_refused(lx, "waiver file is signed by");
}

#[test]
fn an_unsigned_waiver_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver("2026-12-31T00:00:00Z", "D7"),
        None,
    );
    std::fs::remove_file(sig_of(lx.waivers.as_ref().unwrap())).unwrap();
    w.assert_refused(lx, "waiver file is unsigned");
}

// ── refused: freshness ──────────────────────────────────────────────────────

#[test]
fn a_stale_end_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["end"] = json!("2026-08-01T00:00:00Z"); // > 30 d before TEST_NOW
    w.assert_refused(w.cfg(&ev), "is stale");
}

#[test]
fn a_future_end_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["end"] = json!("2026-09-25T12:00:01Z");
    w.assert_refused(w.cfg(&ev), "in the future");
}

// ── refused: engine, source, host ───────────────────────────────────────────

#[test]
fn missing_engine_digests_are_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["engine"]
        .as_object_mut()
        .unwrap()
        .remove("jailer_sha256");
    w.assert_refused(w.cfg(&ev), "lacks engine");
}

#[test]
fn a_manifest_that_pins_no_engine_is_refused() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m.as_object_mut().unwrap().remove("engine");
    let w = World::with_manifest(&m.to_string());
    // The pin rule and the pin-equality rule each refuse it alone (rows4b,
    // four-cell record): a missing pin never equals the record's digest.
    w.assert_refused(w.cfg(&w.evidence()), "engine");
}

#[test]
fn engine_digests_differing_from_the_manifest_pins_are_refused() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m["engine"] = json!({"firecracker_sha256": "f".repeat(64), "jailer_sha256": TEST_JAILER_SHA});
    let w = World::with_manifest(&m.to_string());
    w.assert_refused(
        w.cfg(&w.evidence()),
        "differ from the manifest's engine pins",
    );
}

#[test]
fn evidence_from_a_dirty_tree_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["source"]["tree_dirty"] = json!(true);
    w.assert_refused(w.cfg(&ev), "dirty (or unstated) source tree");
}

#[test]
fn a_manifest_built_from_a_dirty_tree_is_refused() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m["source"]["axon_tree_dirty_at_build"] = json!(true);
    let w = World::with_manifest(&m.to_string());
    w.assert_refused(w.cfg(&w.evidence()), "built from a dirty");
}

#[test]
fn a_record_without_a_host_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev.as_object_mut().unwrap().remove("host");
    w.assert_refused(w.cfg(&ev), "states no host");
}

#[test]
fn a_record_without_a_caveat_is_refused() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["caveat"] = json!("");
    w.assert_refused(w.cfg(&ev), "states no caveat");
}

// ── the real B263 record and the committed profile ──────────────────────────

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// The actual qualifying record (`.axon-v022/evidence/b263/20260924T080432Z.json`,
/// 32 PASS / 0 FAIL / 4 BLOCKED, unsigned) against the manifest it NAMES —
/// pinned as a fixture, because the committed manifest has since been rebuilt
/// (Stage 3 L5b) and a record must be judged against its own manifest. Refused as unsigned; and signing it would not be enough, because its
/// four BLOCKED assertions are unwaived.
#[test]
fn the_real_b263_record_is_refused() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/b263-20260924T080432Z.json");
    let env = Env::new();
    let d = env.dir.path();
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/manifest-at-b263-20260924T080432Z.json");
    let ev: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    assert_eq!(ev["counts"]["BLOCKED"], 4);
    assert_eq!(ev["profile"]["manifest_sha256"], sha256_file(&manifest));
    let issuer = Issuer::generate();
    // The historical record predates RULE:issuer-claimed. Its copy names the
    // key that signs it here, so this test still pins its ORIGINAL refusal.
    let mut rec: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    rec["issuer_key_id"] = serde_json::json!(issuer.key_id());
    std::fs::write(
        d.join("evidence.json"),
        serde_json::to_vec_pretty(&rec).unwrap(),
    )
    .unwrap();
    issuer.trust_in(&d.join("trusted_issuers"), "operator");
    let mut trust = backend::QualificationTrust::for_manifest(&manifest);
    trust.issuers_dir = d.join("trusted_issuers");
    // Judged at a time it would still be fresh, so freshness is not the reason.
    trust.clock = backend::Clock::FixedUnix(backend::parse_utc("2026-09-24T09:00:00Z").unwrap());
    let lx = LinuxProfileConfig {
        launcher_sha256: sha256_file(&stand_in_launcher(&env, 0, true, true, 0)),
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: d.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: d.join("lx-out"),
        exec_owner: None,
        interpreter: None,
        privileged: None,
    };
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let w = World {
        env,
        issuer,
        manifest_sha: String::new(),
    };
    w.assert_refused(lx.clone(), "unsigned");
    // Even signed by a trusted key, the exact bytes are not qualifying.
    let bytes = std::fs::read(&lx.evidence).unwrap();
    std::fs::write(sig_of(&lx.evidence), w.issuer.sign(&bytes)).unwrap();
    let e = lx.qualification().unwrap_err();
    assert!(e.contains("x1_guest_policy_channel"), "{e}");
    assert!(e.contains("not covered by an issuer-signed waiver"), "{e}");
}

/// The Stage-3 re-qualification (`.axon-v022/evidence/b263/20260926T002631Z.json`,
/// 39 PASS / 0 FAIL / 2 BLOCKED: x3 host boundary, x4 trusted issuer), judged
/// against the COMMITTED manifest it names. Refused unsigned; and signed by a
/// trusted key it is still refused, because x3/x4 are unwaived — closing it
/// needs the operator's signature AND, on WSL2, an x3 waiver (S3-6, D7).
#[test]
fn the_stage3_requalification_record_is_refused_until_signed_and_waived() {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/b263-20260926T002631Z.json");
    let env = Env::new();
    let d = env.dir.path();
    // The manifest AS IT WAS when this record was made (the committed one has
    // since moved on to the PSV image, 06ec49e3): the record names its bytes.
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/manifest-at-b263-20260926T002631Z.json");
    let ev: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    assert_eq!(ev["counts"]["BLOCKED"], 2);
    assert_eq!(ev["counts"]["FAIL"], 0);
    assert_eq!(ev["profile"]["manifest_sha256"], sha256_file(&manifest));
    let issuer = Issuer::generate();
    // The historical record predates RULE:issuer-claimed. Its copy names the
    // key that signs it here, so this test still pins its ORIGINAL refusal.
    let mut rec: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    rec["issuer_key_id"] = serde_json::json!(issuer.key_id());
    std::fs::write(
        d.join("evidence.json"),
        serde_json::to_vec_pretty(&rec).unwrap(),
    )
    .unwrap();
    issuer.trust_in(&d.join("trusted_issuers"), "operator");
    let mut trust = backend::QualificationTrust::for_manifest(&manifest);
    trust.issuers_dir = d.join("trusted_issuers");
    trust.clock = backend::Clock::FixedUnix(backend::parse_utc("2026-09-26T01:00:00Z").unwrap());
    let lx = LinuxProfileConfig {
        launcher_sha256: sha256_file(&stand_in_launcher(&env, 0, true, true, 0)),
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: d.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: d.join("lx-out"),
        exec_owner: None,
        interpreter: None,
        privileged: None,
    };
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let w = World {
        env,
        issuer,
        manifest_sha: String::new(),
    };
    w.assert_refused(lx.clone(), "unsigned");
    let bytes = std::fs::read(&lx.evidence).unwrap();
    std::fs::write(sig_of(&lx.evidence), w.issuer.sign(&bytes)).unwrap();
    let e = lx.qualification().unwrap_err();
    assert!(e.contains("not covered by an issuer-signed waiver"), "{e}");
    assert!(
        !e.contains("x1_guest_policy_channel"),
        "x1 must now PASS: {e}"
    );
}

/// The committed repository state: `profiles/linux-microvm/trusted_issuers/`
/// holds no key, so the default configuration refuses protected dispatch
/// until the operator installs an issuer key and signs a re-qualification
/// (S3-6). That refusal is the intended outcome, not a regression.
#[test]
fn the_committed_profile_has_no_trusted_issuer_so_protected_dispatch_is_refused() {
    let manifest = repo().join("profiles/linux-microvm/manifest.json");
    let trust = backend::QualificationTrust::for_manifest(&manifest);
    assert!(trust
        .issuers_dir
        .ends_with("profiles/linux-microvm/trusted_issuers"));
    let keys = std::fs::read_dir(&trust.issuers_dir)
        .map(|rd| {
            rd.filter(|e| {
                e.as_ref()
                    .is_ok_and(|e| e.path().extension().is_some_and(|x| x == "pub"))
            })
            .count()
        })
        .unwrap_or(0);
    assert_eq!(
        keys, 0,
        "a trusted issuer key was committed; S3-6 must say so here"
    );
    let env = Env::new();
    let lx = LinuxProfileConfig {
        launcher_sha256: sha256_file(&stand_in_launcher(&env, 0, true, true, 0)),
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/b263-20260924T080432Z.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: env.dir.path().join("lx-out"),
        exec_owner: None,
        interpreter: None,
        privileged: None,
    };
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let w = World {
        env,
        issuer: Issuer::generate(),
        manifest_sha: String::new(),
    };
    // Either rule (rows4b, four-cell record): no key configured or, with that
    // rule removed, the next one this unsigned fixture meets (RULE:unsigned);
    // the property is that the profile is ineligible.
    w.assert_refused(lx, "ineligible:");
}

#[test]
fn only_the_documented_time_format_parses() {
    assert_eq!(backend::parse_utc("1970-01-01T00:00:00Z"), Some(0));
    assert_eq!(
        backend::parse_utc("2026-09-24T08:04:59Z"),
        Some(1_790_237_099)
    );
    for bad in [
        "2026-09-24 08:04:59Z",
        "2026-09-24T08:04:59",
        "2026-13-01T00:00:00Z",
        "",
    ] {
        assert_eq!(backend::parse_utc(bad), None, "{bad}");
    }
}

/// RULE:issuer-claimed: a qualification record names the key it is issued
/// under, and only that key's signature counts — a record signed by one trusted
/// issuer cannot pass as another's, and an unnamed issuer qualifies nothing.
/// Mutation: drop the claim check in `qualification()` → both are accepted.
#[test]
fn a_record_counts_only_under_the_issuer_it_names() {
    let w = World::new();
    let d = w.env.dir.path();
    let issuer = &w.issuer;
    let mut ev = good_evidence(&w.manifest_sha);
    ev["issuer_key_id"] = serde_json::json!("ed25519:0000000000000000");
    let lx = qualified_linux_cfg(d, issuer, &ev);
    let e = lx.qualification().unwrap_err();
    assert!(e.contains("claims issuer_key_id"), "{e}");
    ev["issuer_key_id"] = serde_json::Value::Null;
    let lx = qualified_linux_cfg(d, issuer, &ev);
    let e = lx.qualification().unwrap_err();
    assert!(e.contains("claims issuer_key_id null"), "{e}");
    ev.as_object_mut().unwrap().remove("issuer_key_id");
    let lx = qualified_linux_cfg(d, issuer, &ev);
    lx.qualification()
        .expect("the helper names the signing key: qualified");
}

// ── C9 round 4b, rows4b (amendment 62): the qualification rules only Fabric
// applies (the record's identity and its binding to THIS host's manifest),
// each through select() and submit() with an ATTACK assertion. The rules
// shared with readiness are judged on both routes in readiness_attribution.rs.

impl World {
    /// The attack is select() choosing the protected profile or submit()
    /// launching under `lx`; then the refusal must be `why`'s.
    fn assert_attack_refused(&self, lx: LinuxProfileConfig, what: &str, why: &str) {
        let sel = backend::select(&self.req("op-sel-attack"), Some(&lx), Default::default());
        let mut cfg = self.env.cfg(0);
        cfg.linux = Some(lx.clone());
        let s = submit(&run_request(&self.env, "op-q-attack").to_string(), &cfg).unwrap();
        assert!(
            sel.is_err()
                && s.receipt.status == ReceiptStatus::Unsupported
                && self.env.launch_records() == 0,
            "ATTACK: {what}, and the protected profile qualified it: select {:?}, submit {:?} \
             ({:?})",
            sel.map(|p| p.id),
            s.receipt.status,
            s.reason
        );
        self.assert_refused(lx, why);
    }
}

#[test]
fn a_record_of_another_schema_never_qualifies() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["schema"] = json!("axon-b263-evidence/0");
    w.assert_attack_refused(
        w.cfg(&ev),
        "an issuer-signed record of another schema",
        "schema is not axon-b263-evidence/1",
    );
}

#[test]
fn a_record_for_another_profile_never_qualifies() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["profile"]["name"] = json!("linux-microvm-dev");
    w.assert_attack_refused(
        w.cfg(&ev),
        "an issuer-signed record qualifying another profile",
        "for a different profile",
    );
}

#[test]
fn a_manifest_pinning_no_engine_never_qualifies() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m.as_object_mut().unwrap().remove("engine");
    let w = World::with_manifest(&m.to_string());
    // Either rule refuses it (four-cell record against the pin equality).
    w.assert_attack_refused(
        w.cfg(&w.evidence()),
        "a host manifest that pins no VMM engine",
        "engine",
    );
}

#[test]
fn engine_digests_other_than_the_manifests_pins_never_qualify() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m["engine"] = json!({"firecracker_sha256": "f".repeat(64), "jailer_sha256": TEST_JAILER_SHA});
    let w = World::with_manifest(&m.to_string());
    w.assert_attack_refused(
        w.cfg(&w.evidence()),
        "a record qualifying another firecracker than the host manifest pins",
        "differ from the manifest's engine pins",
    );
}

#[test]
fn a_manifest_built_from_a_dirty_tree_never_qualifies() {
    let mut m: Value = serde_json::from_str(&full_lx_manifest(GUEST)).unwrap();
    m["source"]["axon_tree_dirty_at_build"] = json!(true);
    let w = World::with_manifest(&m.to_string());
    w.assert_attack_refused(
        w.cfg(&w.evidence()),
        "a host manifest whose artifacts were built from a dirty tree",
        "built from a dirty",
    );
}

/// The record qualified ANOTHER manifest: the one in use is not the one
/// qualified (a changed manifest is never "probably fine").
#[test]
fn a_record_qualifying_another_manifest_never_qualifies_this_one() {
    let w = World::new();
    let mut ev = w.evidence();
    ev["profile"]["manifest_sha256"] = json!("7".repeat(64));
    w.assert_attack_refused(
        w.cfg(&ev),
        "a record qualifying another profile manifest",
        "a changed manifest is not the qualified profile",
    );
}

/// RULE:waiver-schema: an issuer-signed file of another schema is not a
/// waiver, even bound to this record.
#[test]
fn a_waiver_file_of_another_schema_waives_nothing() {
    let w = World::new();
    let mut ev = w.evidence();
    x3_blocked(&mut ev);
    let mut lx = w.cfg(&ev);
    w.waive(
        &mut lx,
        &w.issuer,
        x3_waiver("2026-12-31T00:00:00Z", "D7"),
        None,
    );
    let p = lx.waivers.clone().unwrap();
    let mut v: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["schema"] = json!("axon-b263-waiver/0");
    w.issuer.write_signed(&p, &v);
    w.assert_attack_refused(
        lx,
        "an issuer-signed file of another schema waived a BLOCKED assertion",
        "waiver file schema is not",
    );
}

/// An EMPTY qualification root (the directory exists, holds no key) trusts
/// nothing: no record signed by anyone qualifies.
#[test]
fn an_empty_qualification_root_qualifies_nothing() {
    let w = World::new();
    let lx = w.cfg(&w.evidence());
    for e in std::fs::read_dir(&lx.trust.issuers_dir).unwrap() {
        std::fs::remove_file(e.unwrap().path()).unwrap();
    }
    let sel = backend::select(&w.req("op-sel-empty"), Some(&lx), Default::default());
    let mut cfg = w.env.cfg(0);
    cfg.linux = Some(lx.clone());
    let s = submit(&run_request(&w.env, "op-q-empty").to_string(), &cfg).unwrap();
    assert!(
        sel.is_err() && s.receipt.status == ReceiptStatus::Unsupported,
        "ATTACK: a qualification root holding no key qualified a signed record: select {:?}, \
         submit {:?}",
        sel.map(|p| p.id),
        s.receipt.status
    );
    // Two rules refuse it, each alone: no key configured, and the signer not
    // being a trusted issuer (four-cell record). Either reason.
    let r = s.reason.unwrap_or_default();
    assert!(
        r.contains("no trusted evidence issuer is configured")
            || r.contains("not a trusted evidence issuer"),
        "{r}"
    );
}

/// RULE:end-not-future, on the input where it is the only rule: an operator
/// who sets no practical maximum age (u64::MAX) makes the freshness rule
/// admit any age, and a record dated after the decision would then read as
/// fresh (its negative age, as an unsigned number, is below the maximum).
#[test]
fn a_record_from_the_future_never_qualifies_even_with_no_maximum_age() {
    let w = World::new();
    let mut lx = w.cfg(&w.evidence());
    lx.trust.max_age_s = u64::MAX;
    lx.qualification()
        .expect("control: the genuine record, no maximum age");
    let mut ev = w.evidence();
    ev["end"] = json!("2026-09-25T12:00:01Z");
    let mut lx = w.cfg(&ev);
    lx.trust.max_age_s = u64::MAX;
    w.assert_attack_refused(
        lx,
        "a record whose end is after the decision time, under no maximum age",
        "in the future",
    );
}

/// Amendment 65 (M1486): the B263 record states the host the run was MEASURED
/// on. Every protected receipt carries `qualification-host:<host>` verbatim,
/// and the qualification harness wrote the constant "WSL2-nested" whatever
/// host it ran on. The record's identity comes from scripts/b263_host.py,
/// which b263_qualify.sh's record writer calls: its `host` must name this
/// host's hostname and machine-id (read here independently), and an operator
/// label is a prefix to the measurement, never a replacement for it.
#[test]
fn the_b263_record_states_the_host_it_ran_on() {
    let scripts = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../scripts");
    let ident = |label: Option<&str>| -> Value {
        let mut c = std::process::Command::new("python3");
        c.arg("-B").arg(scripts.join("b263_host.py"));
        c.env_remove("B263_HOST_LABEL").env_remove("B263_CAVEAT");
        if let Some(l) = label {
            c.env("B263_HOST_LABEL", l);
        }
        let o = c.output().unwrap();
        assert!(o.status.success(), "setup: b263_host.py: {o:?}");
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let hostname = std::fs::read_to_string("/proc/sys/kernel/hostname")
        .unwrap()
        .trim()
        .to_string();
    let machine_id = std::fs::read_to_string("/etc/machine-id")
        .map(|s| s.trim().to_string())
        .unwrap_or_else(|_| "unreadable".into());
    for (label, v) in [(None, ident(None)), (Some("lab-7"), ident(Some("lab-7")))] {
        let host = v["host"].as_str().unwrap_or("");
        assert!(
            host.contains(&format!("hostname {hostname}"))
                && host.contains(&format!("machine-id {machine_id}")),
            "ATTACK: the B263 record states a host it did not measure ({host:?}; this host is \
             {hostname}, machine-id {machine_id}), label {label:?}"
        );
        assert_eq!(
            v["facts"]["machine_id"].as_str(),
            Some(machine_id.as_str()).filter(|m| *m != "unreadable")
        );
        if let Some(l) = label {
            assert!(
                host.starts_with(l),
                "the operator's label prefixes it: {host}"
            );
        }
        assert!(!v["caveat"].as_str().unwrap_or("").is_empty(), "{v}");
    }
    // The record writer takes both from it (not a constant of its own).
    let harness = std::fs::read_to_string(scripts.join("b263_qualify.sh")).unwrap();
    assert!(
        harness.contains("\"host\": hi[\"host\"]")
            && harness.contains("\"caveat\": hi[\"caveat\"]")
            && !harness.contains("WSL2-nested"),
        "ATTACK: b263_qualify.sh writes a host or caveat that b263_host.py did not measure"
    );
}
