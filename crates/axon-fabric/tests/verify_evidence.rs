//! `axon-fabric verify-evidence` (governance/specs/v022-protected-suite-verdict.md):
//! the protected-host certification record counts only under a detached
//! `axon-evidence-signature/1` from a TRUSTED operator issuer, verified by the
//! same rules as the B263 qualification record. The keys here are generated
//! inside the test's temp dir; none is ever committed.

mod common;
use common::*;

use serde_json::json;
use std::path::Path;
use std::process::Command;

fn verify(record: &Path, issuers: &Path) -> (i32, serde_json::Value) {
    let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .args(["verify-evidence", "--record"])
        .arg(record)
        .arg("--issuers")
        .arg(issuers)
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
