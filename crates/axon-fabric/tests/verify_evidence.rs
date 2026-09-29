//! `axon-fabric verify-evidence` (governance/specs/v022-protected-suite-verdict.md):
//! the protected-host certification record counts only under a detached
//! `axon-evidence-signature/2` (qualification domain) from a TRUSTED operator issuer, verified by the
//! same rules as the B263 qualification record. The keys here are generated
//! inside the test's temp dir; none is ever committed.

mod common;
use common::*;

use serde_json::json;
use std::path::Path;
use std::process::Command;

fn verify(record: &Path, issuers: &Path) -> (i32, serde_json::Value) {
    verify_as(record, issuers, "qualification")
}

fn verify_as(record: &Path, issuers: &Path, authority: &str) -> (i32, serde_json::Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .args(["verify-evidence", "--record"])
        .arg(record)
        .arg("--issuers")
        .arg(issuers)
        .args(["--authority", authority])
        .output()
        .unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or(serde_json::Value::Null);
    (out.status.code().unwrap_or(-1), v)
}

#[test]
fn only_a_trusted_operator_signature_over_the_exact_bytes_verifies() {
    let d = tempfile::tempdir().unwrap();
    let issuers = d.path().join("trusted_issuers");
    std::fs::create_dir(&issuers).unwrap();
    let rec = d.path().join("protected_backend.json");
    let record = json!({"schema":"axon-v022-protected-certification/1",
                        "component":"protected_backend","host_profile":"linux-microvm-protected"});
    let operator = Issuer::generate();
    operator.write_signed(&rec, &record);

    // No trusted issuer configured (the committed state today): refused.
    let (c, v) = verify(&rec, &issuers);
    assert_eq!(c, 4, "{v}");
    assert!(
        v["reason"]
            .as_str()
            .unwrap()
            .contains("no trusted evidence issuer"),
        "{v}"
    );

    // Signed, but by a key the operator does not trust: refused.
    Issuer::generate().trust_in(&issuers, "someone-else");
    let (c, v) = verify(&rec, &issuers);
    assert_eq!(c, 4, "{v}");
    assert!(
        v["reason"]
            .as_str()
            .unwrap()
            .contains("not a trusted evidence issuer"),
        "{v}"
    );

    // Trusted: verifies, and names the issuer.
    operator.trust_in(&issuers, "operator");
    let (c, v) = verify(&rec, &issuers);
    assert_eq!(c, 0, "{v}");
    assert_eq!(v["verified"], true, "{v}");
    // A caller-chosen root verifies nothing on the operator's behalf, and the
    // output says so: it names the root and the build, and is not authoritative.
    assert_eq!(
        v["authoritative"], false,
        "ATTACK: a caller-chosen --issuers root was reported as authoritative: {v}"
    );
    assert_eq!(v["trust_root"], json!(issuers), "{v}");
    assert_eq!(v["operator_root"], "/etc/axon/trust/qualification", "{v}");
    assert_eq!(v["build"], "test-trust", "{v}");
    assert!(v["non_authoritative_because"].is_string(), "{v}");
    assert!(v["verifier"]["sha256"].is_string(), "{v}");

    // Any change to the record's bytes breaks it.
    let mut bytes = std::fs::read(&rec).unwrap();
    bytes.push(b'\n');
    std::fs::write(&rec, bytes).unwrap();
    let (c, v) = verify(&rec, &issuers);
    assert_eq!(c, 4, "{v}");
    assert!(
        v["reason"].as_str().unwrap().contains("does not verify"),
        "{v}"
    );

    // Unsigned: refused.
    std::fs::remove_file(d.path().join("protected_backend.json.sig")).unwrap();
    let (c, v) = verify(&rec, &issuers);
    assert_eq!(c, 4, "{v}");
    assert!(v["reason"].as_str().unwrap().contains("unsigned"), "{v}");
}

/// The authority is IN the signed message, for every authority — not only
/// qualification. A signature made for one authority, with its `domain` field
/// relabelled to another, does not verify there; the same key signing for the
/// right authority does. (A verifier that always checked the qualification
/// message survived every qualification-only test.)
#[test]
fn each_authority_verifies_only_its_own_domain_message() {
    use axon_fabric::backend::TrustAuthority;
    let d = tempfile::tempdir().unwrap();
    let issuers = d.path().join("trusted");
    let rec = d.path().join("record.json");
    std::fs::write(&rec, br#"{"statement":"x"}"#).unwrap();
    let key = Issuer::generate();
    key.trust_in(&issuers, "operator");
    let bytes = std::fs::read(&rec).unwrap();
    for want in TrustAuthority::ALL {
        for made in TrustAuthority::ALL {
            let mut sig: serde_json::Value =
                serde_json::from_str(&key.sign_for(made, &bytes)).unwrap();
            sig["domain"] = json!(want.dir_name()); // relabel (a no-op when made == want)
            std::fs::write(sig_of(&rec), sig.to_string()).unwrap();
            let (c, v) = verify_as(&rec, &issuers, want.dir_name());
            if made == want {
                assert_eq!(c, 0, "{made:?} for {want:?}: {v}");
            } else {
                assert_eq!(c, 4, "{made:?} relabelled as {want:?}: {v}");
                assert!(
                    v["reason"].as_str().unwrap().contains("does not verify"),
                    "{v}"
                );
            }
            // Unrelabelled, the field alone refuses it.
            if made != want {
                std::fs::write(sig_of(&rec), key.sign_for(made, &bytes)).unwrap();
                let (c, v) = verify_as(&rec, &issuers, want.dir_name());
                assert_eq!(c, 4, "{v}");
                assert!(
                    v["reason"].as_str().unwrap().contains("is for authority"),
                    "{v}"
                );
            }
        }
    }
}
