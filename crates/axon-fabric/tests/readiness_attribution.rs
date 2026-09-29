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
    attack(
        &c,
        "the record names a verifier key the operator never trusted",
        "is not a key in the operator's verifier root",
    );
}

/// Revoking the verifier key at the operator root revokes the certification:
/// the key is looked up at decision time, not at signing time.
#[test]
fn revoking_the_verifier_key_revokes_the_certification() {
    let Some(c) = certified() else { return };
    let root = c.trust.issuers_dir.parent().unwrap().join("verifier");
    std::fs::remove_file(root.join("verifier.pub")).unwrap();
    attack(
        &c,
        "the verifier key was revoked at the operator root",
        "is not a key in the operator's verifier root",
    );
}

#[test]
fn an_observer_key_id_outside_the_observer_root_is_refused() {
    let Some(c) = certified() else { return };
    let stranger = Issuer::generate();
    resign(&c, &c.operator, |r| {
        r["observer_key_id"] = json!(stranger.key_id())
    });
    attack(
        &c,
        "the record names an observer key the operator never trusted",
        "is not a key in the operator's observer root",
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
    let obs = observation(&c.observer, &"f".repeat(40));
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
    let mut obs = observation(&c.observer, &"f".repeat(40));
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
    let obs = observation(&c.observer, &"e".repeat(40));
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
        &b263_record(),
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
        let mut b = b263_record();
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
    let mut b = b263_record();
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
