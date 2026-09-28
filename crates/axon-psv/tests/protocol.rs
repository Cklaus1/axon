//! The PSV protocol pieces M1 rests on, each tested for what it must
//! DISCRIMINATE (v022-psv-gap-map.md A1, A2, A3, A11), not just for the
//! happy path.

use axon_psv::*;
use serde_json::json;
use std::path::Path;

fn manifest() -> LaunchManifest {
    LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op-1".into(),
        task_id: "task-1".into(),
        trial_id: "trial-1".into(),
        attempt_id: "attempt-1".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        qualification_sha256: "1".repeat(64),
        host_config_sha256: "2".repeat(64),
        launcher_sha256: "3".repeat(64),
        firecracker_sha256: "4".repeat(64),
        profile_manifest_sha256: "5".repeat(64),
        guest: GuestDigests {
            kernel_sha256: "6".repeat(64),
            rootfs_sha256: "7".repeat(64),
            axon_sha256: "8".repeat(64),
            init_sha256: "9".repeat(64),
        },
        policy_sha256: "a".repeat(64),
        suite: SuiteRef {
            id: "acceptance".into(),
            version: format!("acf1:{}", "b".repeat(64)),
            entry: "accept.ax".into(),
            test: "t_ok".into(),
            tree_digest: String::new(),
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: format!("acf1:{}", "d".repeat(64)),
            tree_digest: String::new(),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    }
}

fn tree(root: &Path, files: &[(&str, &str)]) {
    for (p, c) in files {
        let p = root.join(p);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, c).unwrap();
    }
}

#[test]
fn manifest_bytes_are_canonical_and_verify_only_under_their_own_digest() {
    let m = manifest();
    let b = m.bytes();
    // Canonical: keys sorted at every level, no whitespace.
    let text = String::from_utf8(b.clone()).unwrap();
    assert!(text.starts_with("{\"attempt_id\":"), "{text}");
    assert!(
        !text.contains('\n') && !text.contains(": ") && !text.contains(", "),
        "{text}"
    );
    let pos = |k: &str| text.find(&format!("\"{k}\":")).unwrap();
    assert!(
        pos("kernel_sha256") < pos("rootfs_sha256"),
        "nested keys sorted"
    );
    assert_eq!(LaunchManifest::verify(&b, &m.digest()).unwrap(), m);

    // Another digest than the one Fabric named: refused.
    let e = LaunchManifest::verify(&b, &"0".repeat(64)).unwrap_err();
    assert!(e.contains("is not the"), "{e}");

    // Same content, non-canonical bytes (pretty-printed): refused even under
    // their own digest.
    let pretty = serde_json::to_vec_pretty(&m).unwrap();
    let e = LaunchManifest::verify(&pretty, &sha256_hex(&pretty)).unwrap_err();
    assert!(e.contains("not canonical"), "{e}");

    // An unknown field, the wrong profile, the wrong scheme: refused.
    let mut v = serde_json::to_value(&m).unwrap();
    v["extra"] = json!("x");
    let b2 = canonical_json(&v);
    assert!(LaunchManifest::verify(&b2, &sha256_hex(&b2))
        .unwrap_err()
        .contains("unknown field"));
    for (ptr, val, why) in [
        (
            "/backend_profile",
            "local-interpreter",
            "not linux-microvm-protected",
        ),
        ("/schema", "axon-launch-manifest/0", "schema"),
        ("/completion/scheme", "none", "completion scheme"),
    ] {
        let mut v = serde_json::to_value(&m).unwrap();
        *v.pointer_mut(ptr).unwrap() = json!(val);
        let b3 = canonical_json(&v);
        let e = LaunchManifest::verify(&b3, &sha256_hex(&b3)).unwrap_err();
        assert!(e.contains(why), "{ptr}: {e}");
    }
}

/// A1 / A2: the guest executes nothing unless BOTH inputs hash to what the
/// manifest names; a mismatch names which input.
#[test]
fn inputs_must_match_the_manifest_each_one_named() {
    let d = tempfile::tempdir().unwrap();
    let (cand, suite) = (d.path().join("cand"), d.path().join("suite"));
    tree(&cand, &[("f.ax", "fn main() {}\n")]);
    tree(&suite, &[("accept.ax", "@[test] fn t_ok() {}\n")]);
    let q = Quota::default();
    let mut m = manifest();
    m.candidate.tree_digest = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    m.suite.tree_digest = axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap();
    let ok = check_inputs(&m, &cand, &suite, &q).unwrap();
    assert!(ok.matches);
    assert_eq!(ok.candidate_tree_digest, m.candidate.tree_digest);

    // A1: the candidate changed after sealing.
    std::fs::write(cand.join("f.ax"), "fn main() { 1 }\n").unwrap();
    let (found, e) = check_inputs(&m, &cand, &suite, &q).unwrap_err();
    assert!(e.starts_with("candidate tree is"), "{e}");
    assert!(!found.matches);
    std::fs::write(cand.join("f.ax"), "fn main() {}\n").unwrap();

    // A2: the suite is another one.
    tree(&suite, &[("extra.ax", "")]);
    let (_, e) = check_inputs(&m, &cand, &suite, &q).unwrap_err();
    assert!(e.starts_with("suite tree is"), "{e}");

    // The inputs swapped: refused (the candidate is checked against the
    // candidate digest, not "either").
    std::fs::remove_file(suite.join("extra.ax")).unwrap();
    let (_, e) = check_inputs(&m, &suite, &cand, &q).unwrap_err();
    assert!(e.starts_with("candidate tree is"), "{e}");

    // A missing input is a refusal, never an empty digest that might match.
    let (_, e) = check_inputs(&m, &d.path().join("absent"), &suite, &q).unwrap_err();
    assert!(e.contains("candidate input"), "{e}");
}

/// A3 / A11: a completion proof is bound to every identity it must not
/// travel between. Changing ANY one of them — or the secret — changes the key.
#[test]
fn the_completion_key_moves_with_every_bound_identity_and_the_secret() {
    let s = [7u8; 32];
    let base = manifest();
    let k0 = completion_key(&s, &base);
    assert_eq!(k0, completion_key(&s, &base), "deterministic");
    assert_ne!(k0, completion_key(&[8u8; 32], &base), "secret");

    type Edit = fn(&mut LaunchManifest);
    let edits: [(&str, Edit); 11] = [
        ("operation", |m| m.operation_id.push('x')),
        ("trial", |m| m.trial_id.push('x')),
        ("attempt", |m| m.attempt_id.push('x')),
        ("suite id", |m| m.suite.id.push('x')),
        ("suite version", |m| m.suite.version.push('x')),
        ("entry", |m| m.suite.entry.push('x')),
        ("test", |m| m.suite.test.push('x')),
        ("candidate tree", |m| m.candidate.tree_digest.push('x')),
        ("suite tree", |m| m.suite.tree_digest.push('x')),
        // Any other manifest field moves the manifest digest, which is bound.
        ("guest kernel (via manifest digest)", |m| {
            m.guest.kernel_sha256.push('x')
        }),
        ("observation nonce (via manifest digest)", |m| {
            m.observation_nonce.push('x')
        }),
    ];
    for (what, edit) in edits {
        let mut m = base.clone();
        edit(&mut m);
        assert_ne!(k0, completion_key(&s, &m), "{what} did not move the key");
    }
}

/// RFC 4231 test case 2 pins the HMAC.
#[test]
fn hmac_matches_rfc_4231() {
    let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");

    let hex: String = mac.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
    );
}

#[test]
fn a_guest_verdict_round_trips_and_refuses_unknown_fields() {
    let v = GuestVerdict {
        schema: GUEST_VERDICT_SCHEMA.into(),
        launch_manifest_sha256: "0".repeat(64),
        inputs: InputCheck {
            candidate_tree_digest: "acf1:x".into(),
            suite_tree_digest: "acf1:y".into(),
            matches: true,
        },
        test: "t_ok".into(),
        status: GuestStatus::Passed,
        refusal: None,
        exit_code: Some(0),
        report: Some(GuestReport {
            passed: vec!["t_ok".into()],
            failed: vec![],
            completion: vec![("t_ok".into(), "tok".into())],
        }),
        runner: Runner {
            init_sha256: "1".repeat(64),
            axon_sha256: "2".repeat(64),
        },
        stdout_sha256: None,
    };
    let b = v.bytes();
    assert!(String::from_utf8(b.clone())
        .unwrap()
        .contains("\"match\":true"));
    assert_eq!(serde_json::from_slice::<GuestVerdict>(&b).unwrap(), v);
    let mut j: serde_json::Value = serde_json::from_slice(&b).unwrap();
    j["status_override"] = json!("passed");
    assert!(serde_json::from_value::<GuestVerdict>(j).is_err());
}

/// The binding object is exactly §4's: every identity named explicitly, not
/// only through the manifest digest (a later manifest change must not be able
/// to drop one silently).
#[test]
fn the_completion_binding_names_every_identity_explicitly() {
    let m = manifest();
    let b: serde_json::Value = serde_json::from_slice(&completion_binding(&m, "MD")).unwrap();
    assert_eq!(
        b,
        json!({
            "scheme": COMPLETION_SCHEME,
            "operation_id": m.operation_id, "trial_id": m.trial_id, "attempt_id": m.attempt_id,
            "suite_id": m.suite.id, "suite_version": m.suite.version,
            "entry": m.suite.entry, "test": m.suite.test,
            "candidate_tree_digest": m.candidate.tree_digest,
            "suite_tree_digest": m.suite.tree_digest,
            "launch_manifest_digest": "MD",
        })
    );
}
