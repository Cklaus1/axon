//! G01-r22-independent-issuer, producer side, through the real `axon-fabric`
//! process. Fabric signs the receipts it issues so a consumer can
//! AUTHENTICATE a verdict instead of trusting an issuer name — and the signing
//! authority is the OPERATOR's: the signer lives in the check registry, not in
//! any per-call flag, and Fabric signs only receipts whose workload could not
//! have read the key.

mod common;
use common::*;

use axon_loop_contracts::{ComputeRequest, ExecutionReceipt, OpaqueRef};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

/// A check grant with NO effect axis: the workload can read nothing, so it
/// cannot reach the signing key.
const GRANT_PURE: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";

fn fabric(args: &[String], stdin: Option<&str>) -> (i32, Value) {
    use std::io::Write;
    let mut child = std::process::Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .args(args)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut pipe = child.stdin.take().unwrap();
    if let Some(s) = stdin {
        pipe.write_all(s.as_bytes()).unwrap();
    }
    drop(pipe);
    let out = child.wait_with_output().unwrap();
    let v = serde_json::from_slice(&out.stdout).unwrap_or(Value::Null);
    (out.status.code().unwrap(), v)
}

fn keygen(out: &Path) -> (i32, Value) {
    fabric(
        &["keygen".into(), "--out".into(), out.display().to_string()],
        None,
    )
}

/// A copy of the env's check registry carrying `signer` (or none).
fn registry_with(env: &Env, name: &str, signer: Option<Value>) -> PathBuf {
    let mut v: Value = serde_json::from_slice(&std::fs::read(&env.registry).unwrap()).unwrap();
    if let Some(s) = signer {
        v["signer"] = s;
    }
    let p = env.dir.path().join(name);
    std::fs::write(&p, v.to_string()).unwrap();
    p
}

fn submit_args(env: &Env, registry: &Path, grants: &Path) -> Vec<String> {
    let p = |x: &Path| x.display().to_string();
    vec![
        "submit".into(),
        "--request".into(),
        "-".into(),
        "--journal".into(),
        p(&env.journal),
        "--check-registry".into(),
        p(registry),
        "--grant-registry".into(),
        p(grants),
        "--store".into(),
        p(&env.store),
        "--tenant".into(),
        "tenant-t".into(),
        "--family".into(),
        "family-f".into(),
        "--expected-epoch".into(),
        "0".into(),
        "--workspace".into(),
        p(&env.ws),
        "--state".into(),
        p(&env.dir.path().join("fabric-state")),
    ]
}

fn chmod(p: &Path, mode: u32) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
}

/// The whole producer contract, on one provisioned key.
#[test]
fn fabric_signs_as_the_operators_signer_and_only_when_the_workload_cannot_reach_the_key() {
    let env = Env::new();
    // Its own directory: grant files are written beside their registry.
    std::fs::create_dir_all(env.dir.path().join("grants-pure")).unwrap();
    let pure = env.dir.path().join("grants-pure").join("grants.json");
    write_grant_registry(&pure, &[("grant:test", PRINCIPAL, GRANT_PURE)]);

    // keygen: owner-only, never over an existing file, prints the public key.
    let key = env.dir.path().join("issuer.pk8");
    let (c, gen) = keygen(&key);
    assert_eq!(c, 0, "{gen}");
    let pk = gen["public_key"].as_str().unwrap().to_string();
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&key).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    assert_ne!(keygen(&key).0, 0, "keygen never overwrites a key");
    let signer = |path: &Path, pk: &str| json!({"issuer_ref": "fabric:verifier", "key_path": path, "public_key": pk});
    let issuer = OpaqueRef::new("fabric:verifier").unwrap();

    // Signed: an effect-free check, the operator's signer.
    let reg = registry_with(&env, "reg-signed.json", Some(signer(&key, &pk)));
    let req = request(&env, "op-signed", "t_ok");
    let (c, out) = fabric(&submit_args(&env, &reg, &pure), Some(&req.to_string()));
    assert_eq!(c, 0, "{out}");
    let rq: ComputeRequest = serde_json::from_value(req.clone()).unwrap();
    let rc: ExecutionReceipt = serde_json::from_value(out["receipt"].clone()).unwrap();
    let att = &out["receipt_attestation"];
    let key_id = axon_loop_contracts::attestation::verify(att, &issuer, &rq, &rc, &pk)
        .expect("the attestation verifies under the pinned key");
    assert_eq!(att["key_id"], key_id.as_str());
    assert_eq!(att["operation_id"], "op-signed");
    let (_, other_pk) = axon_loop_contracts::attestation::generate().unwrap();
    assert!(
        axon_loop_contracts::attestation::verify(att, &issuer, &rq, &rc, &other_pk).is_err(),
        "and under no other key"
    );

    // Withheld: the grant lets the workload read files, so it could have read
    // the key. The receipt is still an answer — just not an attested one.
    let (c, out) = fabric(
        &submit_args(&env, &reg, &env.grant_registry),
        Some(&request(&env, "op-effectful", "t_ok").to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["receipt_attestation"], json!(null));
    assert!(
        out["attestation_withheld"]
            .as_str()
            .is_some_and(|r| r.contains("could have read the signing key")),
        "{out}"
    );

    // No signer configured: unattested, nothing withheld.
    let plain = registry_with(&env, "reg-plain.json", None);
    let (c, out) = fabric(
        &submit_args(&env, &plain, &pure),
        Some(&request(&env, "op-plain", "t_ok").to_string()),
    );
    assert_eq!(c, 0);
    assert_eq!(out["receipt_attestation"], json!(null));
    assert_eq!(out["attestation_withheld"], json!(null));

    // Every signer defect refuses before any work (exit 4, nothing spawned).
    let open_key = env.dir.path().join("open.pk8");
    keygen(&open_key);
    chmod(&open_key, 0o644);
    let open_pk =
        axon_loop_contracts::attestation::public_key_of(&std::fs::read(&open_key).unwrap())
            .unwrap();
    let junk = env.dir.path().join("junk.pk8");
    std::fs::write(&junk, b"not pkcs8").unwrap();
    chmod(&junk, 0o600);
    let mut extra = signer(&key, &pk);
    extra["note"] = json!("x");
    let mut missing = signer(&key, &pk);
    missing.as_object_mut().unwrap().remove("public_key");
    let spawns = spawn_count(&env.spawns);
    for (i, (why, bad)) in [
        ("world-readable key", signer(&open_key, &open_pk)),
        (
            "key does not derive the pinned key",
            signer(&key, &other_pk),
        ),
        ("not a PKCS#8 key", signer(&junk, &pk)),
        (
            "absent key file",
            signer(&env.dir.path().join("nope.pk8"), &pk),
        ),
        ("extra field", extra),
        ("missing field", missing),
    ]
    .into_iter()
    .enumerate()
    {
        let reg = registry_with(&env, &format!("reg-bad-{i}.json"), Some(bad));
        let (c, out) = fabric(
            &submit_args(&env, &reg, &pure),
            Some(&request(&env, &format!("op-bad-{i}"), "t_ok").to_string()),
        );
        assert_eq!(c, 4, "{why}: {out}");
        assert_eq!(out["kind"], "unregistered", "{why}: {out}");
    }
    assert_eq!(
        spawn_count(&env.spawns),
        spawns,
        "a refused signer ran nothing"
    );
}
