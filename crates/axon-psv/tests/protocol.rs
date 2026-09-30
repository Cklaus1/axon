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
        verifier_sha256: "d".repeat(64),
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
            // B3: a manifest's tree digest IS its version.
            tree_digest: format!("acf1:{}", "b".repeat(64)),
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: format!("acf1:{}", "d".repeat(64)),
            tree_digest: format!("acf1:{}", "d".repeat(64)),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        authority: AuthorityRef {
            epoch: 0,
            tenant_id: "tenant-t".into(),
            task_family: "family-f".into(),
        },
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
            runner_sha256: "1".repeat(64),
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

/// B3: a manifest whose tree digest is not the version it names is refused by
/// the guest, whatever else is right.
#[test]
fn a_tree_digest_must_be_the_version_it_names() {
    for edit in [
        |m: &mut LaunchManifest| m.candidate.tree_digest = format!("acf1:{}", "9".repeat(64)),
        |m: &mut LaunchManifest| m.suite.tree_digest = format!("acf1:{}", "9".repeat(64)),
    ] {
        let mut m = manifest();
        edit(&mut m);
        let e = LaunchManifest::verify(&m.bytes(), &m.digest()).unwrap_err();
        assert!(e.contains("tree_digest") && e.contains("is not its"), "{e}");
    }
}

/// Review wf_d725935a-7ed: a guest input holds no symlink and nothing the
/// digest omits (.git/.micode), so the bytes that run are the bytes digested.
#[test]
fn inputs_with_links_or_omitted_entries_are_refused() {
    let d = tempfile::tempdir().unwrap();
    let (cand, suite) = (d.path().join("cand"), d.path().join("suite"));
    tree(&cand, &[("f.ax", "fn main() {}\n")]);
    tree(&suite, &[("accept.ax", "@[test] fn t_ok() {}\n")]);
    let q = Quota::default();
    let mut m = manifest();
    m.candidate.tree_digest = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    m.suite.tree_digest = axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap();
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok());

    std::os::unix::fs::symlink("f.ax", cand.join("g.ax")).unwrap();
    // The manifest NAMES the tree with the link (the store's importer accepts
    // one, and the digest recipe records it), so the digest join agrees and
    // the no-symlink rule is the only thing that refuses it.
    let mut with_link = m.clone();
    with_link.candidate.tree_digest = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    assert_ne!(with_link.candidate.tree_digest, m.candidate.tree_digest);
    let (_, e) = check_inputs(&with_link, &cand, &suite, &q)
        .expect_err("ATTACK: a candidate holding a symlink was accepted under a digest naming it");
    assert!(e.contains("symlink"), "{e}");
    std::fs::remove_file(cand.join("g.ax")).unwrap();

    tree(
        &suite,
        &[(".git/accept.ax", "@[test] fn t_ok() { assert(true) }\n")],
    );
    let (_, e) = check_inputs(&m, &cand, &suite, &q).unwrap_err();
    assert!(e.contains(".git") && e.contains("omits"), "{e}");
}

/// PSV-2 (C9 certifying review): the tree digest is a CROSS-PEER contract
/// (MiCode's WORKSPACE_VERSION_RECIPE.md), and it cannot see an empty
/// directory or any mode bit but exec. So the guest refuses an input holding
/// either — each case first shows the digest is UNCHANGED (the check it used
/// to be is blind to it), then that the input check refuses it, naming it.
/// Controls: the normalised forms (launcher 0644/0755, store read-only
/// 0444/0555) and mkfs's empty root `lost+found` are accepted.
#[test]
fn inputs_holding_what_the_digest_cannot_see_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let chmod =
        |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    let d = tempfile::tempdir().unwrap();
    let (cand, suite) = (d.path().join("cand"), d.path().join("suite"));
    tree(
        &cand,
        &[("f.ax", "fn main() {}\n"), ("lib/h.ax", "fn h() {}\n")],
    );
    tree(&suite, &[("accept.ax", "@[test] fn t_ok() {}\n")]);
    // Normalised explicitly: the test does not depend on the umask.
    for (p, m) in [
        (cand.join("f.ax"), 0o644),
        (cand.join("lib"), 0o755),
        (cand.join("lib/h.ax"), 0o644),
        (suite.join("accept.ax"), 0o644),
    ] {
        chmod(&p, m);
    }
    let q = Quota::default();
    let mut m = manifest();
    m.candidate.tree_digest = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    m.suite.tree_digest = axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap();
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok());
    let blind = |root: &Path, want: &str| {
        assert_eq!(
            axon_workspace_recipe::tree_version_ref(root, &q).unwrap(),
            want,
            "the digest sees this case: it is not one it is blind to"
        );
    };
    // The panic names the refusal that did not happen, so a kill says which.
    let refused = |want: &str| match check_inputs(&m, &cand, &suite, &q) {
        Ok(v) => panic!("accepted, but must refuse with {want:?}: {v:?}"),
        Err((found, e)) => {
            assert!(!found.matches);
            assert_eq!(e, want);
        }
    };

    // An empty directory — named like a module, the reviewer's probe.
    std::fs::create_dir(cand.join("g.ax")).unwrap();
    blind(&cand, &m.candidate.tree_digest);
    refused(
        "candidate input holds an empty directory (g.ax), which the digest cannot see: refused",
    );
    std::fs::remove_dir(cand.join("g.ax")).unwrap();
    // …or one holding only empty directories.
    std::fs::create_dir_all(cand.join("lib/x/y")).unwrap();
    blind(&cand, &m.candidate.tree_digest);
    refused(
        "candidate input holds an empty directory (lib/x/y), which the digest cannot see: refused",
    );
    std::fs::remove_dir_all(cand.join("lib/x")).unwrap();
    // …and in the suite too.
    std::fs::create_dir(suite.join("planted")).unwrap();
    blind(&suite, &m.suite.tree_digest);
    refused("suite input holds an empty directory (planted), which the digest cannot see: refused");
    std::fs::remove_dir(suite.join("planted")).unwrap();
    // `lost+found` is exempt only EMPTY and only at the root.
    std::fs::create_dir(cand.join("lib/lost+found")).unwrap();
    refused("candidate input holds an empty directory (lib/lost+found), which the digest cannot see: refused");
    std::fs::remove_dir(cand.join("lib/lost+found")).unwrap();

    // A mode the digest does not record: file and directory.
    for (p, mode, want) in [
        ("f.ax", 0o000, "f.ax with mode 0000"),
        ("f.ax", 0o600, "f.ax with mode 0600"),
        ("f.ax", 0o666, "f.ax with mode 0666"),
        ("f.ax", 0o744, "f.ax with mode 0744"),
        ("f.ax", 0o4755, "f.ax with mode 4755"),
        ("lib", 0o700, "lib with mode 0700"),
        ("lib", 0o1777, "lib with mode 1777"),
        ("lib/h.ax", 0o640, "lib/h.ax with mode 0640"),
    ] {
        let path = cand.join(p);
        let before = std::fs::metadata(&path).unwrap().permissions().mode() & 0o7777;
        chmod(&path, mode);
        // 0744 and 4755 set the exec bit on a 0644 file, which the digest
        // DOES record; every other case leaves the digest unchanged.
        if !matches!(mode, 0o744 | 0o4755) {
            blind(&cand, &m.candidate.tree_digest);
        }
        refused(&format!(
            "candidate input holds {want}, which the digest does not record: refused"
        ));
        chmod(&path, before);
    }
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok());

    // Controls. mkfs's EMPTY root lost+found (0700, as mkfs makes it).
    std::fs::create_dir(cand.join("lost+found")).unwrap();
    chmod(&cand.join("lost+found"), 0o700);
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok());
    // …but not a non-empty one: it is then an ordinary 0700 directory.
    std::fs::write(cand.join("lost+found/#12"), "x").unwrap();
    chmod(&cand.join("lost+found/#12"), 0o644);
    let (_, e) = check_inputs(&m, &cand, &suite, &q).unwrap_err();
    assert!(e.contains("lost+found with mode 0700"), "{e}");
    std::fs::remove_dir_all(cand.join("lost+found")).unwrap();
    // The store's read-only materialization (0444 files, 0555 dirs), and the
    // root's own mode (the mount point) is not the tree's.
    chmod(&cand.join("f.ax"), 0o444);
    chmod(&cand.join("lib/h.ax"), 0o444);
    chmod(&cand.join("lib"), 0o555);
    chmod(&cand, 0o700);
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok());
    chmod(&cand, 0o755);
    chmod(&cand.join("lib"), 0o755);
}

/// Set extended attribute `name` on `p` (not following a link). A filesystem
/// that cannot hold it makes this test FAIL, never pass: a skipped plant
/// would leave the refusal it guards untested.
#[cfg(target_os = "linux")]
fn plant_xattr(p: &Path, name: &str, value: &[u8]) {
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (cp, cn) = (c(p.to_str().unwrap()), c(name));
    let r = unsafe {
        libc::lsetxattr(
            cp.as_ptr(),
            cn.as_ptr(),
            value.as_ptr() as *const libc::c_void,
            value.len(),
            0,
        )
    };
    assert_eq!(
        r,
        0,
        "cannot set {name} on {} ({}): the filesystem under TMPDIR must hold \
         user xattrs and POSIX ACLs; this is a FAILURE, not a skip",
        p.display(),
        std::io::Error::last_os_error()
    );
}

#[cfg(target_os = "linux")]
fn remove_xattr(p: &Path, name: &str) {
    let c = |s: &str| std::ffi::CString::new(s).unwrap();
    let (cp, cn) = (c(p.to_str().unwrap()), c(name));
    assert_eq!(unsafe { libc::lremovexattr(cp.as_ptr(), cn.as_ptr()) }, 0);
}

/// A POSIX access ACL (`system.posix_acl_access`, version 2) that leaves the
/// mode bits a normalised 0644/0755 while denying uid 65534 — the test
/// child's uid — everything: a named `user:65534:---` entry.
fn acl_denying_nobody(owner_perm: u16, rest_perm: u16) -> Vec<u8> {
    let mut a = 2u32.to_le_bytes().to_vec();
    for (tag, perm, id) in [
        (0x01u16, owner_perm, u32::MAX), // USER_OBJ
        (0x02, 0, 65534),                // USER nobody: ---
        (0x04, rest_perm, u32::MAX),     // GROUP_OBJ
        (0x10, rest_perm, u32::MAX),     // MASK
        (0x20, rest_perm, u32::MAX),     // OTHER
    ] {
        a.extend(tag.to_le_bytes());
        a.extend(perm.to_le_bytes());
        a.extend(id.to_le_bytes());
    }
    a
}

/// PSV-2 (C9 dev review): an extended attribute is what the tree digest
/// cannot see AND what the mode check cannot see. A POSIX ACL entry
/// `user:65534:---` on a 0644 file leaves `st_mode` 0644, so the input was
/// accepted with the digest unchanged, while the unprivileged test child was
/// denied the file (a correct candidate driven to a genuine KEYED Failed).
/// The guest now refuses ANY entry carrying ANY extended attribute —
/// including the input root — and names it. Each case first shows the
/// digest (and the mode) are unchanged by the plant.
#[cfg(target_os = "linux")]
#[test]
fn inputs_carrying_an_extended_attribute_are_refused() {
    use std::os::unix::fs::PermissionsExt;
    let chmod =
        |p: &Path, m: u32| std::fs::set_permissions(p, std::fs::Permissions::from_mode(m)).unwrap();
    let d = tempfile::tempdir().unwrap();
    let (cand, suite) = (d.path().join("cand"), d.path().join("suite"));
    tree(
        &cand,
        &[("f.ax", "fn main() {}\n"), ("lib/h.ax", "fn h() {}\n")],
    );
    tree(
        &suite,
        &[
            ("accept.ax", "@[test] fn t_ok() {}\n"),
            ("expected.txt", "42\n"),
        ],
    );
    for (p, m) in [
        (cand.join("f.ax"), 0o644),
        (cand.join("lib"), 0o755),
        (cand.join("lib/h.ax"), 0o644),
        (suite.join("accept.ax"), 0o644),
        (suite.join("expected.txt"), 0o644),
    ] {
        chmod(&p, m);
    }
    let q = Quota::default();
    let mut m = manifest();
    m.candidate.tree_digest = axon_workspace_recipe::tree_version_ref(&cand, &q).unwrap();
    m.suite.tree_digest = axon_workspace_recipe::tree_version_ref(&suite, &q).unwrap();
    assert!(check_inputs(&m, &cand, &suite, &q).is_ok(), "control");
    let mode = |p: &Path| std::fs::metadata(p).unwrap().permissions().mode() & 0o7777;

    let acl_file = acl_denying_nobody(6, 4);
    let acl_dir = acl_denying_nobody(7, 5);
    for (root, rel, name, value, what) in [
        // The reviewer's reproduction: an ACL on a suite fixture.
        (
            &suite,
            "expected.txt",
            "system.posix_acl_access",
            &acl_file,
            "suite",
        ),
        (
            &cand,
            "f.ax",
            "system.posix_acl_access",
            &acl_file,
            "candidate",
        ),
        (
            &cand,
            "lib",
            "system.posix_acl_access",
            &acl_dir,
            "candidate",
        ),
        (
            &cand,
            "lib",
            "system.posix_acl_default",
            &acl_dir,
            "candidate",
        ),
        (
            &cand,
            "lib/h.ax",
            "user.planted",
            &b"x".to_vec(),
            "candidate",
        ),
        // The input ROOT itself.
        (&cand, "", "system.posix_acl_access", &acl_dir, "candidate"),
        (&suite, "", "user.planted", &b"x".to_vec(), "suite"),
    ] {
        let p = if rel.is_empty() {
            root.to_path_buf()
        } else {
            root.join(rel)
        };
        let before = mode(&p);
        plant_xattr(&p, name, value);
        assert_eq!(mode(&p), before, "the plant changed the mode bits");
        let want_digest = if what == "suite" {
            &m.suite.tree_digest
        } else {
            &m.candidate.tree_digest
        };
        assert_eq!(
            &axon_workspace_recipe::tree_version_ref(root, &q).unwrap(),
            want_digest,
            "the digest sees {name}: it is not a case it is blind to"
        );
        let shown = if rel.is_empty() { "." } else { rel };
        let want = format!(
            "{what} input holds {shown} carrying extended attribute {name}, \
             which the digest cannot see: refused"
        );
        match check_inputs(&m, &cand, &suite, &q) {
            Ok(v) => panic!(
                "ATTACK: an input carrying {name} on {shown:?} was ACCEPTED with an unchanged \
                 digest, so the child's view differs from what was checked: {v:?}"
            ),
            Err((found, e)) => {
                assert!(!found.matches);
                assert_eq!(e, want);
            }
        }
        remove_xattr(&p, name);
        assert!(
            check_inputs(&m, &cand, &suite, &q).is_ok(),
            "control after removing {name}"
        );
    }
}
