//! PSV-7 / FIELD-ORIGIN (C9 round 4, class c; A88, A89): a certification
//! record's run attribution is joined to the RUN, not only to its format.
//!
//! Round 4 found that readiness joined `observer_key_id` to the observation's
//! signer but checked `verifier_key_id` only for membership in the verifier
//! root, `suite` only for non-empty fields and `candidate_tree_ref` only for
//! format; and that the certified B263 record was never joined to the
//! qualification the observed launch ran under (the launch manifest was not
//! in the evidence), nor required to have been current when the run was
//! observed. The certified evidence now carries the run (Fabric's submit
//! output: receipt, receipt attestation, `axon-psv-evidence/2` bundle; and
//! the request), and each field is joined to a verified document.
//!
//! Every test starts from a GENUINE certification (the fixture asserts PASS)
//! and, with the operator's own key, re-signs a record whose attribution is
//! well-formed but WRONG in one respect. Each is decided by readiness's
//! production decision (`protected_components`), never a helper.

mod common;
mod readiness_fixture;
use common::*;
use readiness_fixture::*;

use axon_fabric::backend::TrustAuthority;
use serde_json::{json, Value};

fn attack(c: &Certified, what: &str, why: &str) {
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: {what} and readiness still said PASS: {v}"
    );
    assert!(v.to_string().contains(why), "expected {why:?}: {v}");
}

fn pass(c: &Certified, what: &str) {
    let v = c.verdict();
    assert_eq!(v["status"], "PASS", "control: {what}: {v}");
}

/// Edit the certified run output in place (no re-attestation) and have the
/// operator re-sign a record over the changed evidence.
fn rewrite_run(c: &Certified, edit: impl FnOnce(&mut Value)) {
    let p = c.repo.join(RUN);
    let mut run: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    edit(&mut run);
    write(&p, &serde_json::to_string_pretty(&run).unwrap());
    rebundle(c, |_| {});
}

fn with_evidence(c: &Certified, edit: impl FnOnce(&mut Vec<String>)) {
    let rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    let mut ev: Vec<String> = rec["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    edit(&mut ev);
    let evr: Vec<&str> = ev.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &evr);
    resign(c, &c.operator, |r| {
        r["evidence"] = json!(ev);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
}

// ── A88: verifier_key_id is the key that attested the run ──────────────────

/// Two keys in the operator's verifier root; the run's receipt was attested
/// by one, the record names the other. Membership alone accepted it. Control:
/// the same root, the record naming the key that signed.
#[test]
fn the_verifier_key_id_is_the_key_that_attested_the_run() {
    let Some(c) = certified() else { return };
    let other = Issuer::generate();
    other.trust_in(
        &c.trust.issuers_dir.parent().unwrap().join("verifier"),
        "other",
    );
    pass(
        &c,
        "a second verifier key in the root, the record naming the signer",
    );
    resign(&c, &c.operator, |r| {
        r["verifier_key_id"] = json!(other.key_id())
    });
    attack(
        &c,
        "the record names a verifier key that did not attest the run",
        "is not verifier_key_id",
    );
}

/// The attestation vouches for other bytes than the certified receipt: the
/// receipt was changed after the verifier signed it.
#[test]
fn a_receipt_the_verifier_did_not_attest_is_refused() {
    let Some(c) = certified() else { return };
    rewrite_run(&c, |run| run["receipt"]["execution_id"] = json!("exec-2"));
    attack(
        &c,
        "the certified receipt is not the one the verifier key attested",
        "is not verifier_key_id",
    );
}

/// The attested receipt must itself be protected evidence: a guest-unobserved
/// receipt names every digest and is attested, but is not a protected run.
#[test]
fn an_attested_receipt_that_is_not_protected_evidence_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        keep(),
        keep(),
        Box::new(|rc| {
            rc["evidence_refs"][0] = json!("evidence-class:guest-unobserved");
        }),
    );
    attack(
        &c,
        "an attested guest-unobserved receipt certified a protected run",
        "is not protected evidence",
    );
}

/// The attested receipt names ANOTHER launch manifest than the one the run's
/// bundle carries (and the certified observation observed).
#[test]
fn an_attested_receipt_of_another_launch_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        keep(),
        keep(),
        Box::new(|rc| {
            rc["evidence_refs"][1] = json!(format!("launch-manifest-sha256:{}", "1".repeat(64)));
        }),
    );
    attack(
        &c,
        "the verifier attested a receipt of another launch",
        "the attested receipt names launch-manifest-sha256:",
    );
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        keep(),
        keep(),
        Box::new(|rc| {
            rc["evidence_refs"][2] =
                json!(format!("preflight-observation-sha256:{}", "1".repeat(64)));
        }),
    );
    attack(
        &c,
        "the verifier attested a receipt citing another observation than the certified one",
        "the attested receipt names preflight-observation-sha256:",
    );
}

/// The attested receipt is for another trial than the launch.
#[test]
fn an_attested_receipt_of_another_trial_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        keep(),
        keep(),
        Box::new(|rc| rc["trial_id"] = json!("trial-2")),
    );
    attack(
        &c,
        "the verifier attested a receipt of another trial than the launch",
        "the launch manifest's trial_id (receipt) is trial-1",
    );
}

// ── the certified observation is of THIS launch ────────────────────────────

/// The observation's suite registry, verifier and host config (and the
/// manifest it says it observed) are joined to the run's launch manifest.
/// Before, only profile, revision and guest were compared with anything.
#[test]
fn the_certified_observation_is_of_the_runs_launch() {
    for (field, why) in [
        ("host_config_sha256", "observation host_config_sha256 is"),
        (
            "suite_registry_sha256",
            "observation suite_registry_sha256 is",
        ),
        ("verifier_sha256", "observation verifier_sha256 is"),
        (
            "intended_launch_manifest_sha256",
            "observation intended_launch_manifest_sha256 is",
        ),
    ] {
        let Some(c) = certified() else { return };
        relaunch(
            &c,
            keep(),
            Box::new(move |o| o[field] = json!("0a".repeat(32))),
            keep(),
        );
        attack(
            &c,
            &format!("the certified observation's {field} is not the run's launch's"),
            why,
        );
    }
}

// ── A89: the B263 record is the one the observed launch RAN UNDER ──────────

fn b263_for_host(c: &Certified, host: &str) -> Value {
    let mut b = b263_record(&c.operator);
    b["host"] = json!(host);
    b
}

/// The launch ran under the genuine B263 record; the operator certified a
/// different, equally current record (another host's). Control: had the
/// launch run under that record, it certifies.
#[test]
fn a_b263_record_the_launch_did_not_run_under_is_refused() {
    let Some(c) = certified() else { return };
    write_signed_for(
        &c.operator,
        TrustAuthority::Qualification,
        &c.repo.join(B263),
        &b263_for_host(&c, "another-host"),
    );
    rebundle(&c, |r| {
        r["b263_qualification_sha256"] = json!(sha(&c.repo.join(B263)))
    });
    attack(
        &c,
        "a current B263 record for another host certified a launch made under a different record",
        "the observed launch ran under B263 qualification",
    );
    relaunch(&c, keep(), keep(), keep());
    pass(&c, "the launch ran under the certified record");
}

/// The launch manifest names a qualification other than the certified B263
/// record (the receipt agrees with the manifest).
#[test]
fn a_launch_under_another_qualification_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        Box::new(|m| m.qualification_sha256 = "6".repeat(64)),
        keep(),
        keep(),
    );
    attack(
        &c,
        "a launch under another B263 qualification was certified under this one",
        "the observed launch ran under B263 qualification",
    );
}

/// The certified B263 record was issued (its `end`) AFTER the run was
/// observed: current at decision time, but it qualified nothing that ran.
/// Control: the genuine record ended before the run.
#[test]
fn a_b263_record_issued_after_the_run_is_refused() {
    let Some(c) = certified() else { return };
    let mut b = b263_record(&c.operator);
    b["start"] = json!("2026-09-28T00:30:00Z");
    b["end"] = json!("2026-09-28T01:00:00Z");
    write_signed_for(
        &c.operator,
        TrustAuthority::Qualification,
        &c.repo.join(B263),
        &b,
    );
    relaunch(&c, keep(), keep(), keep());
    attack(
        &c,
        "a B263 record issued after the run was observed certified it",
        "was not a current qualification when the run was observed",
    );
}

// ── the record's suite and candidate are the launch's ──────────────────────

#[test]
fn the_record_suite_is_the_suite_the_launch_ran() {
    for (k, v) in [
        ("id", json!("other-suite")),
        ("version", json!(format!("acf1:{}", "8".repeat(64)))),
        ("entry", json!("other.ax")),
        ("test", json!("t_other")),
        ("digest", json!(format!("acf1:{}", "8".repeat(64)))),
    ] {
        let Some(c) = certified() else { return };
        resign(&c, &c.operator, |r| r["suite"][k] = v);
        attack(
            &c,
            &format!("the record certifies a suite {k} the launch did not run"),
            &format!("the record's suite {k} is"),
        );
    }
}

#[test]
fn the_record_candidate_is_the_candidate_the_launch_ran() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["candidate_tree_ref"] = json!(format!("acf1:{}", "8".repeat(64)))
    });
    attack(
        &c,
        "the record certifies a candidate the launch did not run",
        "the record's candidate_tree_ref is",
    );
}

// ── the run is in the certified evidence, exactly once ─────────────────────

#[test]
fn a_record_whose_evidence_lacks_the_run_is_refused() {
    let Some(c) = certified() else { return };
    with_evidence(&c, |ev| ev.retain(|e| e != RUN));
    attack(
        &c,
        "a record with no run in its evidence certified the run",
        "carries no Fabric run output",
    );
    let Some(c) = certified() else { return };
    with_evidence(&c, |ev| ev.retain(|e| e != REQUEST));
    attack(
        &c,
        "a record with no request in its evidence certified the run",
        "carries no compute request",
    );
}

/// Two run outputs: which one was the certified run is not stated, so
/// neither is chosen. The genuine one is listed first.
#[test]
fn a_record_carrying_two_runs_is_refused() {
    let Some(c) = certified() else { return };
    let second = "governance/proofs/v022-protected/run-2.json";
    let mut run: Value = serde_json::from_slice(&std::fs::read(c.repo.join(RUN)).unwrap()).unwrap();
    run["receipt"]["execution_id"] = json!("exec-2");
    write(&c.repo.join(second), &run.to_string());
    with_evidence(&c, |ev| ev.push(second.to_string()));
    attack(
        &c,
        "a record carrying two run outputs certified the run",
        "carries 2 Fabric run outputs",
    );
}
