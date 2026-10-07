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
    // The run is launched with the stranger making (and signing) the
    // preflight observation, and the record names that observation and the
    // stranger as observer_key_id: the receipt, manifest and record agree,
    // so every amendment-57 run join holds and only the observer key's
    // membership in the operator's observer root differs (C9 round 4
    // pdfix; before it the observation alone was replaced and the receipt
    // join refused the joint attack first, class e).
    relaunch_observed_by(&c, &stranger, keep(), keep(), keep());
    rebundle(&c, |r| r["observer_key_id"] = json!(stranger.key_id()));
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

/// The attested receipt and the record AGREE on an observation digest that
/// no certified evidence file has (the run is relaunched with the receipt
/// naming it): every run join holds, and only `named` can refuse it. Before
/// amendment 59 the receipt still named the genuine observation, so the
/// amendment-57 receipt join refused the attack first (class e).
#[test]
fn an_observation_digest_that_names_no_evidence_file_is_refused() {
    let Some(c) = certified() else { return };
    let none = "6".repeat(64);
    let ghost = none.clone();
    relaunch(
        &c,
        keep(),
        keep(),
        Box::new(move |rc: &mut Value| {
            for e in rc["evidence_refs"].as_array_mut().unwrap() {
                if e.as_str()
                    .unwrap()
                    .starts_with("preflight-observation-sha256:")
                {
                    *e = json!(format!("preflight-observation-sha256:{ghost}"));
                }
            }
        }),
    );
    rebundle(&c, |r| r["observation_sha256"] = json!(none));
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

/// The run was LAUNCHED (and honestly observed) with another guest kernel,
/// or by another fabric revision, than the record certifies: the manifest,
/// the observation and the attested receipt all agree with each other, so
/// every amendment-57 run join holds and only the observation-vs-record
/// comparison can refuse it (amendment 59; before it the observation alone
/// was edited and the receipt join refused first, class e).
#[test]
fn an_observation_of_another_run_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        Box::new(|m: &mut axon_psv::LaunchManifest| m.guest.kernel_sha256 = "9".repeat(64)),
        keep(),
        keep(),
    );
    attack(
        &c,
        "the observer saw another guest kernel than the one certified",
        "the observation records guest_kernel_sha256",
    );

    let Some(c) = certified() else { return };
    relaunch(
        &c,
        Box::new(|m: &mut axon_psv::LaunchManifest| m.fabric_revision = "e".repeat(40)),
        keep(),
        keep(),
    );
    attack(
        &c,
        "the observer saw another fabric revision than the one certified",
        "the observation records fabric_revision",
    );
}

// ── the B263 qualification record ──────────────────────────────────────────

/// The run was launched under a qualification digest that no certified
/// evidence file has, and the record certifies that same digest: the receipt,
/// manifest and record agree, so the amendment-57 qualification join holds.
/// The evidence list ends with the genuine (operator-signed) B263 record, so
/// a lookup that settled for SOME evidence file instead of the named one
/// would find a valid qualification: only `named` refuses (amendment 59).
#[test]
fn a_b263_digest_that_names_no_evidence_file_is_refused() {
    let Some(c) = certified() else { return };
    relaunch(
        &c,
        Box::new(|m: &mut axon_psv::LaunchManifest| m.qualification_sha256 = "7".repeat(64)),
        keep(),
        keep(),
    );
    let mut ev: Vec<String> = serde_json::from_slice::<Value>(&std::fs::read(c.record()).unwrap())
        .unwrap()["evidence"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.as_str().unwrap().to_string())
        .filter(|e| e != B263)
        .collect();
    ev.push(B263.to_string());
    // The order first (`rebundle` digests the list the record holds).
    resign(&c, &c.operator, |r| r["evidence"] = json!(ev));
    rebundle(&c, |r| {
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
    // The run ran under the agent's record (amendment 59): every run join
    // holds, so only the qualification-root signature check refuses it.
    relaunch(&c, keep(), keep(), keep());
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
        // Launched under THAT record (amendment 59): the qualification join
        // holds, and only the artifact comparison refuses it.
        relaunch(&c, keep(), keep(), keep());
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
    // Launched under THAT record (amendment 59): the qualification join
    // holds, and only the schema/profile check refuses it.
    relaunch(&c, keep(), keep(), keep());
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

// ── C9 round 4b, rows4b (amendment 62): accept_b263's rules on BOTH routes ──
//
// `backend::accept_b263` is ONE implementation applied by Fabric before a
// protected launch (`LinuxProfileConfig::qualification`, reached through
// submit) and by readiness to the record a certification names. Round 4b
// (EQUIVALENCE) found most of its rules with no mutation row. Each test below
// presents ONE defect, signed by the trusted issuer, to BOTH production
// routes; the attack is either route accepting it. Fabric's record is the
// genuine qualification-test record (common::good_evidence), readiness's the
// genuine certified one (readiness_fixture::b263_record).

use axon_loop_contracts::ReceiptStatus;

const FAB_GUEST: &str = "cdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcdcd";

/// Fabric's route: a protected submit on a host whose B263 record is the
/// genuine one with `edit` applied (the second argument is the host's decision
/// time), signed by the trusted issuer, or by `signer` when given (the record
/// then names that signer's key unless `edit` names another), with `waiver`
/// as an issuer-signed waiver file (`"evidence_sha256": "BIND"` is bound to
/// the record). `Ok(reason)`: refused before anything ran; `Err(state)`:
/// launched.
fn fabric_route(
    edit: impl FnOnce(&mut Value, &str),
    waiver: Option<Value>,
    signer: Option<&Issuer>,
) -> Result<String, String> {
    let env = Env::new();
    let d = env.dir.path();
    let m = d.join("manifest.json");
    std::fs::write(&m, full_lx_manifest(FAB_GUEST)).unwrap();
    let issuer = Issuer::generate();
    let mut ev = good_evidence(&sha256_file(&m));
    if let Some(s) = signer {
        ev["issuer_key_id"] = json!(s.key_id());
    }
    edit(&mut ev, TEST_NOW);
    let mut lx = qualified_linux_cfg(d, &issuer, &ev);
    if let Some(s) = signer {
        let bytes = std::fs::read(&lx.evidence).unwrap();
        std::fs::write(sig_of(&lx.evidence), s.sign(&bytes)).unwrap();
    }
    set_launcher(&mut lx, stand_in_launcher(&env, 0, true, true, 0));
    std::fs::create_dir_all(&lx.out_root).unwrap();
    if let Some(mut w) = waiver {
        if w["evidence_sha256"] == "BIND" {
            w["evidence_sha256"] = json!(sha256_file(&lx.evidence));
        }
        let p = d.join("waivers.json");
        issuer.write_signed(&p, &w);
        lx.waivers = Some(p);
    }
    let candidate = psv_suite(&env);
    let mut r = request(&env, "op-b263-rule", "t_psv_ok");
    as_protected_check(&mut r, &candidate, FAB_GUEST);
    r["grant_ref"] = json!("grant:open");
    let mut cfg = env.cfg(0);
    cfg.linux = Some(lx);
    let s = axon_fabric::submit(&r.to_string(), &cfg).unwrap();
    if s.receipt.status == ReceiptStatus::Unsupported && env.launch_records() == 0 {
        Ok(s.reason.unwrap_or_default())
    } else {
        Err(format!("{:?} ({:?})", s.receipt.status, s.reason))
    }
}

/// Readiness's route: the certification's protected_backend verdict.
fn readiness_route(c: &Certified) -> Result<String, String> {
    let v = c.verdict();
    if v["status"] == "PASS" {
        Err(v.to_string())
    } else {
        Ok(v.to_string())
    }
}

/// The attack is EITHER route accepting; then each refusal names `why`.
fn both_refuse(
    what: &str,
    why: &str,
    fabric: Result<String, String>,
    readiness: Option<Result<String, String>>,
) {
    let mut accepted = vec![];
    if let Err(e) = &fabric {
        accepted.push(format!("Fabric's qualification (submit launched: {e})"));
    }
    if let Some(Err(e)) = &readiness {
        accepted.push(format!("readiness (PASS: {e})"));
    }
    assert!(
        accepted.is_empty(),
        "ATTACK: {what}, and it was accepted by {accepted:?}"
    );
    for r in std::iter::once(fabric).chain(readiness) {
        let r = r.unwrap();
        assert!(r.contains(why), "expected {why:?}: {r}");
    }
}

/// The record with x3 BLOCKED (PASS_WITH_BLOCKED) on each route's own record.
fn x3_blocked_on(b: &mut Value) {
    let i = b["assertions"]
        .as_array()
        .unwrap()
        .iter()
        .position(|a| a["name"] == "x3_l0_hypervisor_boundary")
        .unwrap();
    b["assertions"][i]["status"] = json!("BLOCKED");
    let pass = b["counts"]["PASS"].as_u64().unwrap() - 1;
    b["counts"]["PASS"] = json!(pass);
    b["counts"]["BLOCKED"] = json!(1);
    b["result"] = json!("PASS_WITH_BLOCKED");
}

/// Readiness with x3 BLOCKED and, when given, `waiver` (an operator-signed
/// waiver file; `"evidence_sha256": "BIND"` binds it to the record).
fn readiness_blocked(c: &Certified, waiver: Option<Value>) -> Result<String, String> {
    with_b263(c, x3_blocked_on);
    if let Some(mut w) = waiver {
        if w["evidence_sha256"] == "BIND" {
            w["evidence_sha256"] = json!(sha(&c.repo.join(B263)));
        }
        add_evidence(c, WAIVER, &w, &c.operator);
    }
    readiness_route(c)
}

fn x3_waiver(reason: &str, expires: &str, bind: &str) -> Value {
    json!({"schema": "axon-b263-waiver/1", "evidence_sha256": bind,
           "waivers": [{"assertion": "x3_l0_hypervisor_boundary",
                        "reason": reason, "expires": expires}]})
}

/// Both routes accept the honest PASS_WITH_BLOCKED + waiver, so each waiver
/// attack below differs from an accepted case in its one fact.
#[test]
fn control_a_blocked_x3_under_a_bound_reasoned_unexpired_waiver_qualifies_on_both_routes() {
    let w = x3_waiver("operator decision D2", "2026-12-31T00:00:00Z", "BIND");
    let f = fabric_route(|b, _| x3_blocked_on(b), Some(w.clone()), None);
    assert!(f.is_err(), "control: Fabric must launch: {f:?}");
    if let Some(c) = certified() {
        let r = readiness_blocked(&c, Some(w));
        assert!(r.is_err(), "control: readiness must PASS: {r:?}");
    }
}

/// RULE:issuer-trusted (operator_trust::verify_evidence_signature): a record
/// signed by a key outside the qualification root, NAMING that key (so the
/// issuer-claimed rule agrees), qualifies nothing.
#[test]
fn a_b263_record_signed_by_an_untrusted_key_naming_itself_qualifies_nothing() {
    let rogue = Issuer::generate();
    let f = fabric_route(|_, _| {}, None, Some(&rogue));
    let r = certified().map(|c| {
        rebind(
            &c,
            B263,
            "b263_qualification_sha256",
            &b263_record(&rogue),
            &rogue,
            TrustAuthority::Qualification,
        );
        relaunch(&c, keep(), keep(), keep());
        readiness_route(&c)
    });
    both_refuse(
        "a B263 record signed by a key no qualification root holds, naming that key",
        "not a trusted evidence issuer",
        f,
        r,
    );
}

/// RULE:issuer-claimed: signed by the trusted issuer, naming another key.
#[test]
fn a_b263_record_naming_another_issuer_qualifies_nothing_on_either_route() {
    let other = Issuer::generate();
    let f = fabric_route(
        |b, _| b["issuer_key_id"] = json!(other.key_id()),
        None,
        None,
    );
    let r = certified().map(|c| {
        with_b263(&c, |b| b["issuer_key_id"] = json!(other.key_id()));
        readiness_route(&c)
    });
    both_refuse(
        "a B263 record claims another issuer than the key that signed it",
        "claims issuer_key_id",
        f,
        r,
    );
}

/// RULE:pass-count.
#[test]
fn a_b263_record_counting_no_pass_qualifies_nothing_on_either_route() {
    let f = fabric_route(|b, _| b["counts"]["PASS"] = json!(0), None, None);
    let r = certified().map(|c| {
        with_b263(&c, |b| b["counts"]["PASS"] = json!(0));
        readiness_route(&c)
    });
    both_refuse(
        "a B263 record counting no PASS assertion",
        "no PASS assertion",
        f,
        r,
    );
}

/// RULE:blocked-count.
#[test]
fn a_b263_record_whose_blocked_count_disagrees_qualifies_nothing_on_either_route() {
    let f = fabric_route(|b, _| b["counts"]["BLOCKED"] = json!(1), None, None);
    let r = certified().map(|c| {
        with_b263(&c, |b| b["counts"]["BLOCKED"] = json!(1));
        readiness_route(&c)
    });
    both_refuse(
        "a B263 record whose counts.BLOCKED disagrees with its assertions",
        "disagrees with the",
        f,
        r,
    );
}

/// RULE:blocked-unwaived.
#[test]
fn an_unwaived_blocked_assertion_qualifies_nothing_on_either_route() {
    let f = fabric_route(|b, _| x3_blocked_on(b), None, None);
    let r = certified().map(|c| readiness_blocked(&c, None));
    both_refuse(
        "a BLOCKED assertion with no waiver",
        "not covered by an issuer-signed waiver",
        f,
        r,
    );
}

/// RULE:waiver-reason.
#[test]
fn a_waiver_stating_no_reason_qualifies_nothing_on_either_route() {
    let w = x3_waiver(" ", "2026-12-31T00:00:00Z", "BIND");
    let f = fabric_route(|b, _| x3_blocked_on(b), Some(w.clone()), None);
    let r = certified().map(|c| readiness_blocked(&c, Some(w)));
    both_refuse(
        "a waiver states no reason for the BLOCKED assertion",
        "states no reason",
        f,
        r,
    );
}

/// RULE:waiver-expiry.
#[test]
fn an_expired_waiver_qualifies_nothing_on_either_route() {
    let w = x3_waiver("operator decision D2", "2026-09-01T00:00:00Z", "BIND");
    let f = fabric_route(|b, _| x3_blocked_on(b), Some(w.clone()), None);
    let r = certified().map(|c| readiness_blocked(&c, Some(w)));
    both_refuse(
        "a waiver that expired before the decision",
        "has expired",
        f,
        r,
    );
}

/// RULE:waiver-bound (parse_waivers, both routes).
#[test]
fn a_waiver_bound_to_another_record_qualifies_nothing_on_either_route() {
    let other = "e".repeat(64);
    let w = x3_waiver("operator decision D2", "2026-12-31T00:00:00Z", &other);
    let f = fabric_route(|b, _| x3_blocked_on(b), Some(w.clone()), None);
    let r = certified().map(|c| readiness_blocked(&c, Some(w)));
    both_refuse(
        "a waiver bound to another B263 record",
        "not transferable",
        f,
        r,
    );
}

/// RULE:end-not-future. A record dated after the decision time.
#[test]
fn a_b263_record_ending_in_the_future_qualifies_nothing_on_either_route() {
    let f = fabric_route(|b, _| b["end"] = json!("2026-09-25T12:00:01Z"), None, None);
    let r = certified().map(|c| {
        with_b263(&c, |b| b["end"] = json!("2026-09-28T12:00:01Z"));
        readiness_route(&c)
    });
    // RULE:end-fresh refuses the same record too (a negative age read as
    // unsigned is huge), so either reason; only acceptance is the attack.
    let mut accepted = vec![];
    if let Err(e) = &f {
        accepted.push(format!("Fabric (submit launched: {e})"));
    }
    if let Some(Err(e)) = &r {
        accepted.push(format!("readiness (PASS: {e})"));
    }
    assert!(
        accepted.is_empty(),
        "ATTACK: a B263 record whose end is after the decision time, and it was accepted by \
         {accepted:?}"
    );
    for x in std::iter::once(f).chain(r) {
        let x = x.unwrap();
        assert!(x.contains("in the future") || x.contains("is stale"), "{x}");
    }
}

/// RULE:engine-digests: a record that does not identify the jailer. (On
/// Fabric's route the manifest's engine pin also refuses it; readiness joins
/// only the firecracker digest to the observation.)
#[test]
fn a_b263_record_naming_no_jailer_qualifies_nothing_on_either_route() {
    let f = fabric_route(
        |b, _| {
            b["engine"].as_object_mut().unwrap().remove("jailer_sha256");
        },
        None,
        None,
    );
    let r = certified().map(|c| {
        with_b263(&c, |b| {
            b["engine"].as_object_mut().unwrap().remove("jailer_sha256");
        });
        readiness_route(&c)
    });
    let mut accepted = vec![];
    if let Err(e) = &f {
        accepted.push(format!("Fabric (submit launched: {e})"));
    }
    if let Some(Err(e)) = &r {
        accepted.push(format!("readiness (PASS: {e})"));
    }
    assert!(
        accepted.is_empty(),
        "ATTACK: a B263 record that does not identify the VMM's jailer, and it was accepted by \
         {accepted:?}"
    );
    if let Some(r) = r {
        assert!(r.unwrap().contains("lacks engine"));
    }
}

/// RULE:caveat.
#[test]
fn a_b263_record_stating_no_caveat_qualifies_nothing_on_either_route() {
    let f = fabric_route(|b, _| b["caveat"] = json!(""), None, None);
    let r = certified().map(|c| {
        with_b263(&c, |b| b["caveat"] = json!(" "));
        readiness_route(&c)
    });
    both_refuse("a B263 record stating no caveat", "states no caveat", f, r);
}
