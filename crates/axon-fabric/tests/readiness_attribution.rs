//! PSV-7 / FIELD-ORIGIN (C9 round 1, class c): a certification record names
//! WHO authenticated the protected run — the observer key, the verifier key,
//! the observation, the B263 qualification and the guest. Those fields were
//! checked for shape only, so an operator signing an agent-drafted record
//! certified whatever attribution it carried. Each test below starts from a
//! GENUINE certification (PASS) and, with the operator's own key, re-signs a
//! record whose attribution is well-formed but WRONG; the component must not
//! be PASS, for that reason.

mod common;
mod readiness_fixture;
use common::*;
use readiness_fixture::*;

use axon_fabric::backend::TrustAuthority;
use serde_json::{json, Value};

/// Replace the certified evidence file `file` with `v` signed by `who` for
/// `authority`, and have the OPERATOR re-sign a record binding it (`field`
/// and the bundle digest updated): everything genuine except `v`.
fn rebind(
    c: &Certified,
    file: &str,
    field: &str,
    v: &Value,
    who: &Issuer,
    authority: TrustAuthority,
) {
    write_signed_for(who, authority, &c.repo.join(file), v);
    let rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    let ev: Vec<String> = rec["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    let ev: Vec<&str> = ev.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &ev);
    let h = sha(&c.repo.join(file));
    resign(c, &c.operator, |r| {
        r[field] = json!(h);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
}

fn attack(c: &Certified, what: &str, why: &str) {
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: {what} and readiness still said PASS: {v}"
    );
    assert!(v.to_string().contains(why), "expected {why:?}: {v}");
}

// ── the key ids are rooted in the operator's observer / verifier roots ─────

#[test]
fn a_verifier_key_id_outside_the_verifier_root_is_refused() {
    let Some(c) = certified() else { return };
    let stranger = Issuer::generate();
    resign(&c, &c.operator, |r| {
        r["verifier_key_id"] = json!(stranger.key_id())
    });
    // Two checks refuse it, each alone (M338, retired EQUIVALENT_DID with a
    // four-cell record against M812, C9 round 4 rows2): the membership check
    // and launched()'s lookup of verifier_key_id in the verifier root.
    // Which one refuses is not the property; only PASS is the attack.
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the record names a verifier key the operator never trusted and readiness \
         still said PASS: {v}"
    );
    assert!(
        v.to_string()
            .contains("is not a key in the operator's verifier root")
            || v.to_string()
                .contains("names no key in the operator's verifier root"),
        "{v}"
    );
}

/// Revoking the verifier key at the operator root revokes the certification:
/// the key is looked up at decision time, not at signing time.
#[test]
fn revoking_the_verifier_key_revokes_the_certification() {
    let Some(c) = certified() else { return };
    let root = c.trust.issuers_dir.parent().unwrap().join("verifier");
    std::fs::remove_file(root.join("verifier.pub")).unwrap();
    // The membership check (M338) and launched()'s lookup of the key (M812)
    // each refuse it alone: either reason (C9 round 4, rows2).
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the verifier key was revoked at the operator root and readiness still said \
         PASS: {v}"
    );
    assert!(
        v.to_string()
            .contains("is not a key in the operator's verifier root")
            || v.to_string()
                .contains("names no key in the operator's verifier root"),
        "{v}"
    );
}

/// The observer_key_id membership check, on the attack it is written for: a
/// key the operator never trusted is named as the observer AND made the
/// observation (so the signer join agrees with the record). Two independent
/// checks refuse it: the membership check, and the observation's signature,
/// which must verify under the operator's observer root (M340) and be the
/// named key's (M339). ALL PATHS (the four-cell record for this row):
/// `attribution` has one caller and no early `Ok`, and always runs both after
/// the membership check; a signer in the root that equals observer_key_id puts
/// observer_key_id in the root. Which layer refuses is not the property, so
/// any refusal is accepted; only PASS is the attack.
#[test]
fn an_observer_key_id_outside_the_observer_root_is_refused() {
    let Some(c) = certified() else { return };
    let stranger = Issuer::generate();
    let obs = c.observed(&stranger, &"f".repeat(40));
    rebind(
        &c,
        OBSERVATION,
        "observation_sha256",
        &obs,
        &stranger,
        TrustAuthority::Observer,
    );
    resign(&c, &c.operator, |r| {
        r["observer_key_id"] = json!(stranger.key_id())
    });
    let v = c.verdict();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the record names an observer key the operator never trusted, that key made \
         the observation, and readiness still said PASS: {v}"
    );
    let v = v.to_string();
    assert!(
        v.contains("is not a key in the operator's observer root")
            || v.contains("not a trusted evidence issuer"),
        "{v}"
    );
}

/// Both keys are in the observer root, but the observation was made by the
/// other one: the record's attribution is false.
#[test]
fn an_observation_by_another_observer_key_is_refused() {
    let Some(c) = certified() else { return };
    let other = Issuer::generate();
    other.trust_in(
        &c.trust.issuers_dir.parent().unwrap().join("observer"),
        "other",
    );
    resign(&c, &c.operator, |r| {
        r["observer_key_id"] = json!(other.key_id())
    });
    attack(
        &c,
        "the record attributes the observation to a key that did not make it",
        "not the certified observer_key_id",
    );
}

// ── the observation ─────────────────────────────────────────────────────────

#[test]
fn an_observation_digest_that_names_no_evidence_file_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["observation_sha256"] = json!("6".repeat(64))
    });
    attack(
        &c,
        "the record's observation_sha256 names no certified evidence",
        "observation_sha256 names no certified evidence file",
    );
}

#[test]
fn an_observation_not_signed_by_an_observer_root_key_is_refused() {
    let Some(c) = certified() else { return };
    let agent = Issuer::generate();
    // It claims to be the operator's observer, and says everything the
    // record certifies; only the signature is not the observer's.
    let obs = c.observed(&c.observer, &"f".repeat(40));
    rebind(
        &c,
        OBSERVATION,
        "observation_sha256",
        &obs,
        &agent,
        TrustAuthority::Observer,
    );
    attack(
        &c,
        "an agent-signed observation stood in for the operator observer's",
        "not a trusted evidence issuer",
    );

    // Unsigned: no sidecar at all.
    let Some(c) = certified() else { return };
    std::fs::remove_file(sig_of(&c.repo.join(OBSERVATION))).unwrap();
    attack(&c, "an unsigned observation was accepted", "is unsigned");
}

#[test]
fn an_observation_of_another_run_is_refused() {
    let Some(c) = certified() else { return };
    let mut obs = c.observed(&c.observer, &"f".repeat(40));
    obs["guest"]["kernel_sha256"] = json!("9".repeat(64));
    rebind(
        &c,
        OBSERVATION,
        "observation_sha256",
        &obs,
        &c.observer,
        TrustAuthority::Observer,
    );
    attack(
        &c,
        "the observer saw another guest kernel than the one certified",
        "the observation records guest_kernel_sha256",
    );

    let Some(c) = certified() else { return };
    let obs = c.observed(&c.observer, &"e".repeat(40));
    rebind(
        &c,
        OBSERVATION,
        "observation_sha256",
        &obs,
        &c.observer,
        TrustAuthority::Observer,
    );
    attack(
        &c,
        "the observer saw another fabric revision than the one certified",
        "the observation records fabric_revision",
    );
}

// ── the B263 qualification record ──────────────────────────────────────────

#[test]
fn a_b263_digest_that_names_no_evidence_file_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["b263_qualification_sha256"] = json!("7".repeat(64))
    });
    attack(
        &c,
        "the record's b263_qualification_sha256 names no certified evidence",
        "b263_qualification_sha256 names no certified evidence file",
    );
}

#[test]
fn a_b263_record_not_signed_under_the_qualification_root_is_refused() {
    let Some(c) = certified() else { return };
    let agent = Issuer::generate();
    rebind(
        &c,
        B263,
        "b263_qualification_sha256",
        &b263_record(&agent),
        &agent,
        TrustAuthority::Qualification,
    );
    attack(
        &c,
        "an agent-signed B263 record stood in for the operator's qualification",
        "not a trusted evidence issuer",
    );
}

#[test]
fn a_b263_record_of_another_guest_is_refused() {
    for (i, (field, artifact, _)) in GUEST.iter().enumerate() {
        let Some(c) = certified() else { return };
        let mut b = b263_record(&c.operator);
        b["profile"]["artifacts"][artifact]["sha256"] = json!(format!("{:x}", i + 10).repeat(64));
        rebind(
            &c,
            B263,
            "b263_qualification_sha256",
            &b,
            &c.operator,
            TrustAuthority::Qualification,
        );
        attack(
            &c,
            &format!(
                "the B263 qualification qualified another {artifact} than the certified {field}"
            ),
            &format!("is not the B263-qualified {artifact}"),
        );
    }
}

#[test]
fn a_b263_record_of_another_profile_is_refused() {
    let Some(c) = certified() else { return };
    let mut b = b263_record(&c.operator);
    b["profile"]["name"] = json!("linux-microvm-dev");
    rebind(
        &c,
        B263,
        "b263_qualification_sha256",
        &b,
        &c.operator,
        TrustAuthority::Qualification,
    );
    attack(
        &c,
        "a B263 qualification of another profile stood in for the protected one",
        "does not name an axon-b263-evidence/1 record",
    );
}

// ── ADR-002 key-role separation at every readiness trust read (A67) ────────

/// FIELD-ORIGIN (C9 round 2; A67): readiness read its roots without
/// exclusivity. The verifier root holds the host signer's public key (its
/// intended place); planted in the qualification root too, Fabric (which
/// holds the private half) signs a certification record that readiness
/// accepted. A key in two roots is authority for neither. Control: the
/// genuine certification passes (the fixture asserts it).
#[test]
fn a_certification_signed_by_the_host_signer_key_is_refused() {
    let Some(c) = certified() else { return };
    c.verifier.trust_in(&c.trust.issuers_dir, "planted");
    resign(&c, &c.verifier, |_| {});
    attack(
        &c,
        "the host signer's key, planted in the qualification root, signed the certification",
        "key-role separation",
    );
}

/// FIELD-ORIGIN (C9 round 2; A67): the other authority roots readiness
/// compares against are the operator's, walked for ownership like the loop's
/// `exclusive` walks them. A monitor root the agent owns could drop a shared
/// key at will, so what it holds is not evidence of separation. Control: the
/// same root operator-owned passes.
#[test]
fn an_agent_owned_peer_root_decides_no_separation() {
    use std::os::unix::fs::PermissionsExt;
    let Some(c) = certified() else { return };
    let monitor = c.trust.issuers_dir.parent().unwrap().join("monitor");
    std::fs::create_dir_all(&monitor).unwrap();
    std::fs::set_permissions(&monitor, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(
        c.verdict()["status"],
        "PASS",
        "control: an operator-owned monitor root"
    );
    std::os::unix::fs::chown(&monitor, Some(65534), None).unwrap();
    attack(
        &c,
        "an agent-owned monitor root was read as holding no shared key",
        "not root",
    );
}

// ── the B263 record is a CURRENT qualification (review PSV-7, C9 round 3;
// A78) ─────────────────────────────────────────────────────────────────────
//
// Readiness checked the named record's signature, schema, profile and guest
// digests, and nothing Fabric's own qualification() refuses: an
// operator-signed record that FAILED, went stale, came from a dirty tree,
// named no host or qualified another engine still certified PASS. The rules
// are now ONE function (backend::accept_b263) both apply. Each attack below
// is the genuine record with one thing wrong, re-signed by the operator; the
// genuine record (the fixture's control, asserted PASS) is the control.

/// The genuine record with `edit` applied, signed by the operator and bound
/// into a re-signed certification.
///
/// The observed launch ran UNDER that record (C9 round 4, A89): the run is
/// relaunched with the manifest naming it, so each attack reaches the rule
/// it is written for and not the qualification join.
fn with_b263(c: &Certified, edit: impl FnOnce(&mut Value)) {
    let mut b = b263_record(&c.operator);
    edit(&mut b);
    rebind(
        c,
        B263,
        "b263_qualification_sha256",
        &b,
        &c.operator,
        TrustAuthority::Qualification,
    );
    relaunch(c, keep(), keep(), keep());
}

#[test]
fn a_failed_b263_record_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| {
        b["assertions"]
            .as_array_mut()
            .unwrap()
            .push(json!({"name": "a4_guest_policy_enforced", "status": "FAIL"}));
        b["counts"]["FAIL"] = json!(1);
        b["counts"]["total"] = json!(3);
    });
    attack(
        &c,
        "a B263 record with a FAIL assertion certified the protected backend",
        "has FAIL assertions",
    );
}

#[test]
fn a_b263_record_whose_result_is_not_pass_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| b["result"] = json!("FAIL"));
    attack(
        &c,
        "a B263 record whose result is FAIL certified the protected backend",
        "only PASS, or PASS_WITH_BLOCKED",
    );
}

#[test]
fn a_stale_b263_record_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| {
        b["start"] = json!("2026-07-01T00:00:00Z");
        b["end"] = json!("2026-07-01T01:00:00Z");
    });
    attack(
        &c,
        "a B263 record 89 days old certified the protected backend",
        "is stale",
    );
}

/// Currency is judged at DECISION time: the genuine record, once more than
/// 30 days old, no longer certifies, exactly as Fabric then refuses every
/// protected launch under it. Control: the same repository decided now.
#[test]
fn a_b263_qualification_that_lapsed_after_certification_is_refused() {
    let Some(c) = certified() else { return };
    let later = c.trust.clone().at(at("2026-10-29T00:00:00Z"));
    let v = axon_fabric::readiness::protected_components(&c.repo, &later)["components"]
        ["protected_backend"]
        .clone();
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the host's B263 qualification lapsed (31 days) and readiness still said PASS: {v}"
    );
    assert!(v.to_string().contains("is stale"), "{v}");
    assert_eq!(
        c.verdict()["status"],
        "PASS",
        "control: decided while current"
    );
}

#[test]
fn a_b263_record_from_a_dirty_tree_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| b["source"]["tree_dirty"] = json!(true));
    attack(
        &c,
        "a B263 record produced from a dirty source tree certified the protected backend",
        "dirty (or unstated) source tree",
    );
}

#[test]
fn a_b263_record_that_names_no_host_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| b["host"] = json!(" "));
    attack(
        &c,
        "a B263 record naming no host certified the protected backend",
        "states no host",
    );
}

/// "Another host": the record qualified a different VMM than the one the
/// observed launch ran. The observation's firecracker is joined to the
/// record's engine.
#[test]
fn a_b263_record_of_another_engine_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| {
        b["engine"]["firecracker_sha256"] = json!("5".repeat(64));
        b["host"] = json!("a laptop");
    });
    attack(
        &c,
        "a B263 record qualifying another host's firecracker certified the observed launch",
        "but the B263 qualification qualified",
    );
}

#[test]
fn a_b263_record_claiming_another_issuer_is_refused() {
    let Some(c) = certified() else { return };
    let other = Issuer::generate();
    with_b263(&c, |b| b["issuer_key_id"] = json!(other.key_id()));
    attack(
        &c,
        "a B263 record claiming another issuer certified the protected backend",
        "claims issuer_key_id",
    );
}

#[test]
fn a_certification_dated_before_its_observation_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["certified_at"] = json!("2026-09-27T00:00:00Z")
    });
    attack(
        &c,
        "a certification dated before the run it certifies was observed",
        "cannot precede what it certifies",
    );
}

#[test]
fn a_certification_dated_in_the_future_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| {
        r["certified_at"] = json!("2026-12-01T00:00:00Z")
    });
    attack(
        &c,
        "a certification dated in the future",
        "is in the future",
    );
}

#[test]
fn a_certification_whose_certified_at_is_not_a_time_is_refused() {
    let Some(c) = certified() else { return };
    resign(&c, &c.operator, |r| r["certified_at"] = json!("1999"));
    attack(
        &c,
        "a certification whose certified_at is not a time",
        "protected_backend: certified_at",
    );
}

// ── BLOCKED assertions: waived only by certified, operator-signed waivers ──

/// Add `v` (signed by `who` for the qualification domain) to the certified
/// evidence as `file`, and have the operator re-sign the record.
fn add_evidence(c: &Certified, file: &str, v: &Value, who: &Issuer) {
    write_signed_for(who, TrustAuthority::Qualification, &c.repo.join(file), v);
    let rec: Value = serde_json::from_slice(&std::fs::read(c.record()).unwrap()).unwrap();
    let mut ev: Vec<String> = rec["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .collect();
    ev.push(file.to_string());
    let evr: Vec<&str> = ev.iter().map(String::as_str).collect();
    let bundle = bundle_of(&c.repo, &evr);
    resign(c, &c.operator, |r| {
        r["evidence"] = json!(ev);
        r["evidence_bundle_sha256"] = json!(bundle);
    });
}

const WAIVER: &str = "governance/proofs/v022-protected/b263-waivers.json";

/// The genuine record with x3 BLOCKED (PASS_WITH_BLOCKED), and a waiver for
/// it signed by `who`.
fn blocked_x3_waived_by(c: &Certified, who: &Issuer) {
    with_b263(c, |b| {
        b["assertions"][1]["status"] = json!("BLOCKED");
        b["counts"]["PASS"] = json!(1);
        b["counts"]["BLOCKED"] = json!(1);
        b["result"] = json!("PASS_WITH_BLOCKED");
    });
    let waiver = json!({
        "schema": "axon-b263-waiver/1",
        "evidence_sha256": sha(&c.repo.join(B263)),
        "waivers": [{"assertion": "x3_l0_hypervisor_boundary",
                     "reason": "operator decision D2", "expires": "2026-12-31T00:00:00Z"}],
    });
    add_evidence(c, WAIVER, &waiver, who);
}

#[test]
fn a_blocked_b263_assertion_under_a_certified_operator_waiver_certifies() {
    let Some(c) = certified() else { return };
    blocked_x3_waived_by(&c, &c.operator);
    let v = c.verdict();
    assert_eq!(
        v["status"], "PASS",
        "control: an operator-waived BLOCKED x3: {v}"
    );
}

#[test]
fn a_blocked_b263_assertion_with_no_waiver_is_refused() {
    let Some(c) = certified() else { return };
    with_b263(&c, |b| {
        b["assertions"][1]["status"] = json!("BLOCKED");
        b["counts"]["PASS"] = json!(1);
        b["counts"]["BLOCKED"] = json!(1);
        b["result"] = json!("PASS_WITH_BLOCKED");
    });
    attack(
        &c,
        "a B263 record with an unwaived BLOCKED assertion certified the protected backend",
        "not covered by an issuer-signed waiver",
    );
}

#[test]
fn a_b263_waiver_not_signed_by_the_operator_is_refused() {
    let Some(c) = certified() else { return };
    let agent = Issuer::generate();
    blocked_x3_waived_by(&c, &agent);
    attack(
        &c,
        "an agent-signed waiver excused a BLOCKED B263 assertion",
        "not a trusted evidence issuer",
    );
}

/// C9 round 3 integration: readiness judges B263 currency with the SAME
/// maximum age the operator's host config sets for Fabric
/// (`qualification.max_age_s`, one reading). The fixture's record is 12.5 h
/// old at FIXTURE_NOW; a host config allowing one hour must make it stale for
/// readiness exactly as it is for Fabric's launch-time qualification.
#[test]
fn readiness_judges_b263_currency_by_the_host_configs_maximum_age() {
    let Some(c) = certified() else { return };
    let cfg = c._d.path().join("protected-host.json");
    let decide = |max_age: Value| {
        std::fs::write(
            &cfg,
            json!({"qualification": {"max_age_s": max_age}}).to_string(),
        )
        .unwrap();
        let t = c.trust.clone().with_host_config(&cfg);
        axon_fabric::readiness::protected_components(&c.repo, &t)["components"]["protected_backend"]
            .clone()
    };
    let v = decide(json!(3600));
    assert_ne!(
        v["status"], "PASS",
        "ATTACK: the host config allows one hour, the B263 record is 12.5 h old, and readiness still said PASS: {v}"
    );
    assert!(v.to_string().contains("is stale"), "{v}");
    let v = decide(json!("a week"));
    assert_ne!(
        v["status"], "PASS",
        "a malformed host max age must refuse: {v}"
    );
    assert!(v.to_string().contains("max_age_s is not a number"), "{v}");
    assert_eq!(
        decide(json!(86400))["status"],
        "PASS",
        "control: a host config allowing a day certifies the same record"
    );
}
