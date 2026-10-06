//! G01-r22-independent-issuer, producer side, through the real `axon-fabric`
//! process. Fabric signs the receipts it issues so a consumer can
//! AUTHENTICATE a verdict instead of trusting an issuer name — and the signing
//! authority is the OPERATOR's: the signer lives in the check registry, not in
//! any per-call flag, and Fabric signs only receipts whose workload could not
//! have read the key.

mod common;
use common::*;

use axon_fabric::workspace::{Quota, WorkspaceStore, WorkspaceTree};
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

/// An operator-registered suite `acceptance` (outside the candidate tree):
/// `(root, workspace_version_ref)`.
fn suite(env: &Env) -> (PathBuf, String) {
    let root = env.dir.path().join("suites/acceptance");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("accept.ax"),
        "mod f\nuse f.{double}\n\n@[test]\nfn accept_double() { assert_eq(double(21), 42) }\n",
    )
    .unwrap();
    let r = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    (root, r)
}

/// A request that runs the registered suite over the candidate, published as
/// a WorkspaceVersion in the CLI's state dir.
fn suite_request(env: &Env, op: &str) -> Value {
    let candidate = WorkspaceStore::open(&env.dir.path().join("fabric-state"), &tenant())
        .unwrap()
        .import_dir(&env.ws, &Quota::default())
        .unwrap();
    let mut r = request(env, op, "t_ok");
    r["argv"] = json!(["check:acceptance", "accept_double"]);
    r["workspace_version_ref"] = json!(candidate.as_str());
    r
}

/// A copy of the env's check registry carrying the suite and `signer` (or none).
fn registry_with(env: &Env, name: &str, signer: Option<Value>) -> PathBuf {
    let mut v: Value = serde_json::from_slice(&std::fs::read(&env.registry).unwrap()).unwrap();
    let (root, r) = suite(env);
    v["checks"] = json!([{"id": "acceptance", "visibility": "hidden", "root": root,
                          "entry": "accept.ax", "workspace_version_ref": r}]);
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
        assert_eq!(mode, 0o400);
    }
    assert_ne!(keygen(&key).0, 0, "keygen never overwrites a key");
    let signer = |path: &Path, pk: &str| json!({"issuer_ref": "fabric:verifier", "key_path": path, "public_key": pk});
    let issuer = OpaqueRef::new("fabric:verifier").unwrap();

    // Signed: an effect-free check, the operator's signer.
    let reg = registry_with(&env, "reg-signed.json", Some(signer(&key, &pk)));
    let req = suite_request(&env, "op-signed");
    let (c, out) = fabric(&submit_args(&env, &reg, &pure), Some(&req.to_string()));
    assert_eq!(c, 0, "{out}");
    let rq: ComputeRequest = serde_json::from_value(req.clone()).unwrap();
    let rc: ExecutionReceipt = serde_json::from_value(out["receipt"].clone()).unwrap();
    let att = &out["receipt_attestation"];
    let key_id = axon_loop_contracts::attestation::verify(att, &issuer, &rq, &rc, &pk)
        .expect("the attestation verifies under the pinned key");
    assert_eq!(att["key_id"], key_id.as_str());
    assert_eq!(att["operation_id"], "op-signed");
    assert_eq!(out["receipt"]["verification"], "passed", "{out}");
    assert!(
        out["receipt"]["evidence_refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| {
                let e = e.as_str().unwrap();
                e.starts_with("check-suite:acceptance@") && e.ends_with("#accept.ax")
            }),
        "the receipt names the suite's ENTRY file too: {out}"
    );

    // A check file of the candidate's OWN tree: candidate bytes cannot define
    // the rubric, so the verifier does not vouch for it (the receipt is still
    // an answer, unattested).
    let (c, out) = fabric(
        &submit_args(&env, &reg, &pure),
        Some(&request(&env, "op-own-file", "t_ok").to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["receipt_attestation"], json!(null));
    assert!(
        out["attestation_withheld"]
            .as_str()
            .is_some_and(|r| r.contains("not an operator-registered suite")),
        "{out}"
    );
    let (_, other_pk) = axon_loop_contracts::attestation::generate().unwrap();
    assert!(
        axon_loop_contracts::attestation::verify(att, &issuer, &rq, &rc, &other_pk).is_err(),
        "and under no other key"
    );

    // Withheld: the grant lets the workload read files, so it could have read
    // the key. The receipt is still an answer — just not an attested one.
    let (c, out) = fabric(
        &submit_args(&env, &reg, &env.grant_registry),
        Some(&suite_request(&env, "op-effectful").to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["receipt_attestation"], json!(null));
    assert!(
        out["attestation_withheld"]
            .as_str()
            .is_some_and(|r| r.contains("could have read the signing key")),
        "{out}"
    );

    // Not a verification: an interpreter_run is execution, not a verdict, so the
    // verifier identity does not sign it.
    let mut run = request(&env, "op-run", "t_ok");
    run["job_kind"] = json!("interpreter_run");
    run["argv"] = json!(["f.ax"]);
    let (c, out) = fabric(&submit_args(&env, &reg, &pure), Some(&run.to_string()));
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["receipt_attestation"], json!(null));
    assert!(
        out["attestation_withheld"]
            .as_str()
            .is_some_and(|r| r.contains("not a registered_check")),
        "{out}"
    );

    // A REPLAY decides from what the op ran under (journalled), not from the
    // grant registry the replaying call supplies: the op above ran with file
    // effects, so presenting the effect-free registry now does not get it
    // signed.
    let (c, out) = fabric(
        &submit_args(&env, &reg, &pure),
        Some(&suite_request(&env, "op-effectful").to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["replayed"], json!(true), "{out}");
    assert_eq!(out["receipt_attestation"], json!(null), "{out}");

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
    chmod(&junk, 0o400);
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

fn tenant() -> axon_loop_contracts::TenantId {
    axon_loop_contracts::TenantId::new("tenant-t").unwrap()
}

/// A registered suite that produced NO verdict (here: it does not compile, so
/// `axon test` emits no summary) still records which suite was running, so an
/// honest "unknown" names what it was unknown about and can be pinned.
/// Mutation: emit `evidence: vec![]` in local_receipt's no-summary branch →
/// the check-suite ref is missing and this fails.
#[test]
fn a_suite_run_without_a_verdict_still_names_its_suite() {
    let env = Env::new();
    let root = env.dir.path().join("suites/broken");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("accept.ax"), "fn (\n").unwrap();
    let r = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut v: Value = serde_json::from_slice(&std::fs::read(&env.registry).unwrap()).unwrap();
    v["checks"] = json!([{"id": "broken", "visibility": "hidden", "root": root,
                          "entry": "accept.ax", "workspace_version_ref": r}]);
    let reg = env.dir.path().join("reg-broken.json");
    std::fs::write(&reg, v.to_string()).unwrap();
    let mut req = suite_request(&env, "op-broken");
    req["argv"] = json!(["check:broken", "anything"]);
    let (c, out) = fabric(
        &submit_args(&env, &reg, &env.grant_registry),
        Some(&req.to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["receipt"]["verification"], "unknown", "{out}");
    assert!(
        out["receipt"]["evidence_refs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e.as_str().unwrap().starts_with("check-suite:broken@")),
        "{out}"
    );
}

/// The signing oracle (G01 re-audit 2): the journal is the CALLER's to name,
/// so a replay returns whatever receipt that file holds. Before this, a replay
/// whose journal said "ran effect-free" was signed — so a caller who wrote a
/// journal holding a fabricated verdict got the verifier's signature on it.
/// Now no replay is signed: not a genuine one, and not a forged one.
#[test]
fn a_replay_is_never_signed_not_even_a_genuine_one() {
    let env = Env::new();
    std::fs::create_dir_all(env.dir.path().join("grants-pure")).unwrap();
    let pure = env.dir.path().join("grants-pure").join("grants.json");
    write_grant_registry(&pure, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
    let key = env.dir.path().join("issuer.pk8");
    let pk = keygen(&key).1["public_key"].as_str().unwrap().to_string();
    let reg = registry_with(
        &env,
        "reg.json",
        Some(json!({"issuer_ref": "fabric:verifier", "key_path": key, "public_key": pk})),
    );
    let req = suite_request(&env, "op-r").to_string();

    // Positive control: the run itself is signed.
    let (c, first) = fabric(&submit_args(&env, &reg, &pure), Some(&req));
    assert_eq!(c, 0, "{first}");
    assert_eq!(first["replayed"], json!(false));
    assert!(first["receipt_attestation"].is_object(), "{first}");

    // A journal the caller wrote: the genuine one with the verdict flipped.
    // Fabric replays the fabricated receipt — and does not vouch for it.
    let forged = env.dir.path().join("forged.journal");
    let text = std::fs::read_to_string(&env.journal).unwrap();
    assert!(text.contains(r#""verification":"passed""#), "{text}");
    std::fs::write(
        &forged,
        text.replace(r#""verification":"passed""#, r#""verification":"failed""#),
    )
    .unwrap();
    let mut args = submit_args(&env, &reg, &pure);
    let j = args.iter().position(|a| a == "--journal").unwrap() + 1;
    args[j] = forged.display().to_string();
    let (c, out) = fabric(&args, Some(&req));
    assert_eq!(c, 0, "{out}");
    assert_eq!(out["replayed"], json!(true), "{out}");
    assert_eq!(
        out["receipt"]["verification"], "failed",
        "the replay served the caller's bytes"
    );
    assert_eq!(out["receipt_attestation"], json!(null), "{out}");

    // The genuine replay: same receipt, not re-signed.
    let (c, again) = fabric(&submit_args(&env, &reg, &pure), Some(&req));
    assert_eq!(c, 0, "{again}");
    assert_eq!(again["replayed"], json!(true));
    assert_eq!(again["receipt"], first["receipt"]);
    assert_eq!(again["receipt_attestation"], json!(null), "{again}");
    assert_eq!(
        again["attestation_withheld"],
        json!(axon_fabric::signing::REPLAYED)
    );
}

/// Re-audit 3: the check process inherited the launcher's WHOLE environment,
/// so ambient `AXON_*` variables steered the verdict the verifier then signed.
/// `AXON_STRICT=1` turns an unused `Result` from a warning into a type error:
/// here it is set in Fabric's own environment, and the operator's suite —
/// which drops a Result — must still pass, because the check runs from an
/// empty environment. Mutation: drop `.with_clean_env()` → no summary, red.
#[test]
fn the_launchers_environment_does_not_steer_a_signed_verdict() {
    let env = Env::new();
    std::fs::create_dir_all(env.dir.path().join("grants-pure")).unwrap();
    let pure = env.dir.path().join("grants-pure").join("grants.json");
    write_grant_registry(&pure, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
    let root = env.dir.path().join("suites/lenient-on-results");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("accept.ax"),
        "mod f\nuse f.{double}\n\nfn probe() -> Result<i64, str> { Ok(1) }\n\n\
         @[test]\nfn accept_double() {\n    probe()\n    assert_eq(double(21), 42)\n}\n",
    )
    .unwrap();
    let sref = WorkspaceTree::import_dir(&root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value = serde_json::from_slice(&std::fs::read(&env.registry).unwrap()).unwrap();
    reg["checks"] = json!([{"id": "acceptance", "visibility": "hidden", "root": root,
                            "entry": "accept.ax", "workspace_version_ref": sref}]);
    let reg_path = env.dir.path().join("reg-env.json");
    std::fs::write(&reg_path, reg.to_string()).unwrap();

    use std::io::Write;
    let submit_under = |strict: bool, op: &str| -> Value {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_axon-fabric"));
        cmd.args(submit_args(&env, &reg_path, &pure));
        if strict {
            cmd.env("AXON_STRICT", "1");
        } else {
            cmd.env_remove("AXON_STRICT");
        }
        let mut child = cmd
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut pipe = child.stdin.take().unwrap();
        pipe.write_all(suite_request(&env, op).to_string().as_bytes())
            .unwrap();
        drop(pipe);
        serde_json::from_slice(&child.wait_with_output().unwrap().stdout).unwrap()
    };
    // Control (C9 round 2): the same submission with no AXON_STRICT in
    // Fabric's environment passes, so a difference below is the variable.
    let out = submit_under(false, "op-env-control");
    assert_eq!(out["receipt"]["verification"], "passed", "control: {out}");
    let out = submit_under(true, "op-env");
    assert_eq!(
        out["receipt"]["verification"], "passed",
        "ATTACK: AXON_STRICT in the launcher's environment steered the signed verdict: {out}"
    );
}

/// Re-audit 5 (mutation reviewer): Fabric signs a local check because the
/// admitted grant's effect ceiling is EMPTY — the INTENDED ceiling — and no
/// test noticed if the executor did not actually apply it. The candidate here
/// calls a file builtin the admission scan does not flag, aimed at the
/// signing key: the ceiling must stop it at run time, so the key is never
/// copied and no pass comes out.
///
/// Mutation: skip applying an empty ceiling in the executor → the key is
/// copied and the check passes, signed → red.
#[test]
fn the_empty_ceiling_is_applied_not_just_intended() {
    let env = Env::new();
    std::fs::create_dir_all(env.dir.path().join("grants-pure")).unwrap();
    let pure = env.dir.path().join("grants-pure").join("grants.json");
    write_grant_registry(&pure, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
    let key = env.dir.path().join("issuer.pk8");
    let pk = keygen(&key).1["public_key"].as_str().unwrap().to_string();
    let reg = registry_with(
        &env,
        "reg.json",
        Some(json!({"issuer_ref": "fabric:verifier", "key_path": key, "public_key": pk})),
    );
    let leak = env.dir.path().join("leaked.pk8");
    std::fs::write(
        env.ws.join("f.ax"),
        format!(
            "fn double(n: i64) -> i64 {{\n    let _ = file_copy(\"{}\", \"{}\")\n    n * 2\n}}\n",
            key.display(),
            leak.display()
        ),
    )
    .unwrap();
    let (c, out) = fabric(
        &submit_args(&env, &reg, &pure),
        Some(&suite_request(&env, "op-exfil").to_string()),
    );
    assert_eq!(c, 0, "{out}");
    assert!(
        !leak.exists(),
        "the check workload copied the signing key: {out}"
    );
    assert_ne!(out["receipt"]["verification"], "passed", "{out}");
}

/// C9 round 4c, EQGATE (amendment 81; M1919): `keygen` CREATES the key file
/// (`create_new`, mode 0400). A key already at `--out` is refused and left
/// untouched: overwriting an issuer's signing key would silently replace the
/// identity every attestation it signed was made under.
#[test]
fn keygen_never_overwrites_an_existing_key_file() {
    let env = Env::new();
    let out = env.dir.path().join("issuer.pk8");
    let (code, _) = keygen(&out);
    assert_eq!(code, 0, "control: a fresh path generates a key");
    let first = std::fs::read(&out).unwrap();
    let (code, _) = keygen(&out);
    assert!(
        code != 0 && std::fs::read(&out).unwrap() == first,
        "ATTACK: keygen overwrote an existing key file (exit {code})"
    );
}
