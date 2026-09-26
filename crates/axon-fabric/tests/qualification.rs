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

fn run_request(env: &Env, op: &str) -> Value {
    let mut r = request(env, op, "t_ok");
    r["required"]["hardware_isolation"] = json!(true);
    r["required"]["os"] = json!("linux");
    r["job_kind"] = json!("interpreter_run");
    r["argv"] = json!(["f.ax"]);
    r["registered_executable_ref"] = json!(backend::LINUX_GUEST_AXON_ID);
    r["grant_ref"] = json!("grant:open");
    r["executable_digest"] = json!(axon_cortex::runner::fabric_executable_digest(
        backend::LINUX_GUEST_AXON_ID,
        GUEST
    ));
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
        World::with_manifest(&lx_manifest(GUEST))
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
        lx.launcher = stand_in_launcher(&self.env, 0, true, true, 0);
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
    let mut m: Value = serde_json::from_str(&lx_manifest(GUEST)).unwrap();
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
    w.assert_refused(lx, "no trusted evidence issuer is configured");
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
    let mut m: Value = serde_json::from_str(&lx_manifest(GUEST)).unwrap();
    m.as_object_mut().unwrap().remove("engine");
    let w = World::with_manifest(&m.to_string());
    w.assert_refused(w.cfg(&w.evidence()), "pins no engine");
}

#[test]
fn engine_digests_differing_from_the_manifest_pins_are_refused() {
    let mut m: Value = serde_json::from_str(&lx_manifest(GUEST)).unwrap();
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
    let mut m: Value = serde_json::from_str(&lx_manifest(GUEST)).unwrap();
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
    std::fs::copy(&fixture, d.join("evidence.json")).unwrap();
    let issuer = Issuer::generate();
    issuer.trust_in(&d.join("trusted_issuers"), "operator");
    let mut trust = backend::QualificationTrust::for_manifest(&manifest);
    trust.issuers_dir = d.join("trusted_issuers");
    // Judged at a time it would still be fresh, so freshness is not the reason.
    trust.clock = backend::Clock::FixedUnix(backend::parse_utc("2026-09-24T09:00:00Z").unwrap());
    let lx = LinuxProfileConfig {
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: d.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: d.join("lx-out"),
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
    let manifest = repo().join("profiles/linux-microvm/manifest.json");
    let ev: Value = serde_json::from_slice(&std::fs::read(&fixture).unwrap()).unwrap();
    assert_eq!(ev["counts"]["BLOCKED"], 2);
    assert_eq!(ev["counts"]["FAIL"], 0);
    assert_eq!(ev["profile"]["manifest_sha256"], sha256_file(&manifest));
    std::fs::copy(&fixture, d.join("evidence.json")).unwrap();
    let issuer = Issuer::generate();
    issuer.trust_in(&d.join("trusted_issuers"), "operator");
    let mut trust = backend::QualificationTrust::for_manifest(&manifest);
    trust.issuers_dir = d.join("trusted_issuers");
    trust.clock = backend::Clock::FixedUnix(backend::parse_utc("2026-09-26T01:00:00Z").unwrap());
    let lx = LinuxProfileConfig {
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: d.join("evidence.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: d.join("lx-out"),
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
        launcher: stand_in_launcher(&env, 0, true, true, 0),
        manifest,
        artifacts_dir: None,
        evidence: Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/b263-20260924T080432Z.json"),
        evidence_signature: None,
        waivers: None,
        trust,
        out_root: env.dir.path().join("lx-out"),
    };
    std::fs::create_dir_all(&lx.out_root).unwrap();
    let w = World {
        env,
        issuer: Issuer::generate(),
        manifest_sha: String::new(),
    };
    w.assert_refused(lx, "no trusted evidence issuer is configured");
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
