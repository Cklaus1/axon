//! The operator example configs (`profiles/protected-host/*.example`) are
//! loaded through the PRODUCTION loaders, with production rules (`test:
//! false`), so an example cannot drift from what the code reads: a field the
//! code now requires (amendment 65's `custodian.sha256`) and an example that
//! lacks it fail here, not on the operator's host.
//!
//! The only test liberty is the operator: the files are copied into a private
//! directory owned by this uid, which is the authority's operator and walk
//! base (a repository checkout is not root-owned from `/`). Every rule
//! `load_config` applies after reading the file is the production one.
//!
//! The host config (`protected-host.json.example`) is not loaded here: its
//! loader verifies every pin against the pinned files themselves, which do
//! not exist off a deployed host. `scripts/test_operator_deploy.sh` holds its
//! shape to the deployed config that the production loader accepted; here it
//! is held to the helper example on the fields `helper_agrees` compares.

use axon_fabric::privileged_launcher::{self, Authority};

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::{script, Bins};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

fn example(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../profiles/protected-host")
        .join(name)
}

/// A private operator directory holding `bytes` at `name` (0644), and the
/// production-rule authority whose operator is this uid.
fn operator_copy(dir: &Path, name: &str, bytes: &[u8]) -> (PathBuf, Authority) {
    std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    let p = dir.join(name);
    std::fs::write(&p, bytes).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
    // SAFETY: geteuid cannot fail.
    let uid = unsafe { libc::geteuid() };
    (
        p,
        Authority {
            operator_uid: uid,
            walk_base: dir.to_path_buf(),
            test: false,
        },
    )
}

#[test]
fn the_operator_examples_load_through_the_production_loaders() {
    // The helper config: privileged_launcher::load_config, production rules.
    let bytes = std::fs::read(example("protected-launcher.json.example")).unwrap();
    let d = tempfile::tempdir().unwrap();
    let (p, a) = operator_copy(d.path(), "protected-launcher.json", &bytes);
    let helper = privileged_launcher::load_config(&p, &a).unwrap_or_else(|e| {
        panic!("protected-launcher.json.example is refused by load_config: {e}")
    });
    assert!(
        helper.custodian.sha256.is_some(),
        "the example pins the custodian program"
    );

    // ATTACK (the drift this test exists for): the example without the
    // amendment-65 custodian program pin is what a pre-amendment kit wrote;
    // the production rule refuses it, so this test speaks when an example
    // lacks a field the code requires.
    let mut v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    v["custodian"].as_object_mut().unwrap().remove("sha256");
    let d2 = tempfile::tempdir().unwrap();
    let (p2, a2) = operator_copy(
        d2.path(),
        "protected-launcher.json",
        &serde_json::to_vec_pretty(&v).unwrap(),
    );
    let got = privileged_launcher::load_config(&p2, &a2);
    assert!(
        matches!(&got, Err(e) if e.contains("custodian.sha256 must pin the axon-custodian program")),
        "ATTACK: a helper config with no custodian program pin was accepted under production rules: {got:?}"
    );

    // The custodian config: custodian::load_config, production rules.
    let cbytes = std::fs::read(example("custodian.json.example")).unwrap();
    let d3 = tempfile::tempdir().unwrap();
    let (p3, a3) = operator_copy(d3.path(), "custodian.json", &cbytes);
    let cust = axon_fabric::custodian::load_config(&p3, &a3)
        .unwrap_or_else(|e| panic!("custodian.json.example is refused by load_config: {e}"));

    // One custodian across the three examples: the helper's socket and uid
    // (what helper_agrees compares), the custodian's own, and the host's
    // observer.custodian (which carries no program pin: amendment 65).
    assert_eq!(helper.custodian.socket, cust.socket);
    assert_eq!(helper.custodian.uid, cust.custodian_uid);
    assert_eq!(helper.fabric_uid, cust.fabric_uid);
    let host: serde_json::Value =
        serde_json::from_slice(&std::fs::read(example("protected-host.json.example")).unwrap())
            .unwrap();
    assert_eq!(
        host["schema"],
        axon_fabric::protected_host::PROTECTED_HOST_SCHEMA
    );
    assert_eq!(
        host["observer"]["custodian"],
        serde_json::json!({"socket": helper.custodian.socket, "uid": helper.custodian.uid}),
        "the host example's custodian is the helper example's socket and uid, and nothing else"
    );
    assert_eq!(host["out_root"], serde_json::json!(helper.out_root));
    assert_eq!(
        host["observer"]["max_age_s"],
        serde_json::json!(helper.observer.max_age_s)
    );

    // Amendment 68: the host example names NO observer program (a production
    // Fabric refuses `observer.command`); the observation comes from the
    // observer SERVICE the helper example pins, whose own example loads under
    // the production rules and names the same socket, uid and Fabric.
    assert!(
        host["observer"].get("command").is_none() && host["observer"].get("interpreter").is_none(),
        "the host example names an observer program: {}",
        host["observer"]
    );
    let obytes = std::fs::read(example("observer.json.example")).unwrap();
    let d4 = tempfile::tempdir().unwrap();
    let (p4, a4) = operator_copy(d4.path(), "observer.json", &obytes);
    let obs = axon_fabric::observer_service::load_config(&p4, &a4)
        .unwrap_or_else(|e| panic!("observer.json.example is refused by load_config: {e}"));
    let svc = helper
        .observer
        .service
        .as_ref()
        .expect("the helper example pins the observer service (observer.service)");
    assert_eq!(svc.socket, obs.socket);
    assert_eq!(svc.uid, obs.observer_uid);
    // Amendment 79: the custodian example answers `check` for the observer's
    // uid, and the helper example pins the Fabric program it serves.
    assert_eq!(cust.observer_uid, Some(obs.observer_uid));
    assert!(
        helper.fabric.path.is_absolute() && helper.fabric.revision.len() == 40,
        "the helper example pins the Fabric program: {:?}",
        helper.fabric
    );
    let mut nofab: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    nofab.as_object_mut().unwrap().remove("fabric");
    let d5 = tempfile::tempdir().unwrap();
    let (p5, a5) = operator_copy(
        d5.path(),
        "protected-launcher.json",
        &serde_json::to_vec_pretty(&nofab).unwrap(),
    );
    assert!(
        privileged_launcher::load_config(&p5, &a5).is_err(),
        "ATTACK: a helper config with no Fabric program pin was accepted under production rules"
    );
    assert_eq!(obs.fabric_uid, helper.fabric_uid);
    assert_ne!(obs.observer_uid, cust.custodian_uid);
    // ATTACK (A94): the example with the observer running as the Fabric uid
    // is what an in-uid observer amounts to; the production rule refuses it.
    let mut ov: serde_json::Value = serde_json::from_slice(&obytes).unwrap();
    ov["observer_uid"] = serde_json::json!(helper.fabric_uid);
    let d5 = tempfile::tempdir().unwrap();
    let (p5, a5) = operator_copy(
        d5.path(),
        "observer.json",
        &serde_json::to_vec_pretty(&ov).unwrap(),
    );
    let got = axon_fabric::observer_service::load_config(&p5, &a5);
    assert!(
        matches!(&got, Err(e) if e.contains("the observer runs as its own uid")),
        "ATTACK: an observer config naming the Fabric uid as the observer was accepted under production rules: {got:?}"
    );
}

/// Amendment 92: no test script may run the operator kit (or any `--apply`) outside `ns_run`, the
/// private-namespace helper that proves its isolation first. The incident this closes: a guard-removal
/// experiment made a kit refusal test a real root `--apply` on the dev host (M2265).
#[test]
fn no_test_script_runs_the_operator_kit_outside_the_namespace_helper() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let run = |args: &[&str]| {
        script(
            "python3",
            root.join("scripts/opkit_ns_drift.py"),
            Bins::NoWorkspaceBinary,
        )
        .args(args)
        .arg(&root)
        .output()
        .unwrap()
    };
    let o = run(&[]);
    assert!(
        o.status.success(),
        "ATTACK: a test script runs the kit outside ns_run:\n{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let o = run(&["--selftest"]);
    assert!(
        o.status.success(),
        "ATTACK: the drift check ACCEPTED an unwrapped --apply:\n{}",
        String::from_utf8_lossy(&o.stdout)
    );
}

/// Amendment 111: a count of the drift gate's must-flag shapes or controls typed into a document goes stale (amendment 105 and
/// the matrix row A236 quoted 88 and 22 long after the self-test carried more). Every such quote under governance/, scripts/ and
/// crates/ must now equal what `--selftest` prints, and `--check-quoted-counts` is the check: it must hold on the real tree
/// (and read at least one quote, or it checks nothing), and it must REFUSE a stale quote planted in each of the three trees.
#[test]
fn every_quoted_selftest_count_is_the_selftests_own() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let run = |args: &[&str], at: &Path| {
        script(
            "python3",
            root.join("scripts/opkit_ns_drift.py"),
            Bins::NoWorkspaceBinary,
        )
        .args(args)
        .arg(at)
        .output()
        .unwrap()
    };
    let o = run(&["--selftest"], &root);
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(o.status.success(), "setup: --selftest: {out}");
    // "selftest: ok (<N> must-flag shapes refused, <M> controls accepted, ...)"
    let num_before = |hay: &str, what: &str| -> u64 {
        let at = hay
            .find(what)
            .unwrap_or_else(|| panic!("setup: no {what:?} in {hay:?}"));
        hay[..at]
            .trim_end()
            .rsplit(|c: char| !c.is_ascii_digit())
            .next()
            .unwrap()
            .parse()
            .unwrap_or_else(|_| panic!("setup: no number before {what:?} in {hay:?}"))
    };
    let (must, ctl) = (
        num_before(&out, " must-flag shapes refused"),
        num_before(&out, " controls accepted"),
    );
    assert!(must > 0 && ctl > 0, "setup: {out}");
    // a stale quote planted in each tree is refused, by name; the right numbers are accepted
    for (rel, tree) in [
        ("governance/specs/x.md", "governance"),
        ("scripts/y.sh", "scripts"),
        ("crates/z/tests/w.rs", "crates"),
    ] {
        for (doc, stale) in [
            (
                format!(
                    "it carries {} must-flag shapes and {ctl} controls",
                    must - 1
                ),
                true,
            ),
            (
                format!(
                    "it carries {must} must-flag shapes and {} controls",
                    ctl + 1
                ),
                true,
            ),
            (
                format!("it carries {must} must-flag shapes and {ctl} controls"),
                false,
            ),
        ] {
            let d = tempfile::tempdir().unwrap();
            let f = d.path().join(rel);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, format!("# {doc}\n")).unwrap();
            let o = run(&["--check-quoted-counts"], d.path());
            let out = String::from_utf8_lossy(&o.stdout).to_string();
            if stale {
                assert!(
                    !o.status.success() && out.contains(rel),
                    "ATTACK: a stale quoted count under {tree}/ ({doc:?}) was accepted by --check-quoted-counts: {out}"
                );
            } else {
                assert!(
                    o.status.success(),
                    "control: the self-test's own counts quoted under {tree}/ were refused: {out}"
                );
            }
        }
    }
    // the real documents
    let o = run(&["--check-quoted-counts"], &root);
    let out = String::from_utf8_lossy(&o.stdout).to_string();
    assert!(
        o.status.success(),
        "ATTACK: a document quotes a must-flag or control count that is not the self-test's ({must} and {ctl}):\n{out}"
    );
    assert!(
        out.contains(&format!("{must} must-flag shapes, {ctl} controls")),
        "ATTACK: --check-quoted-counts derived other counts than --selftest prints ({must}, {ctl}): {out}"
    );
    let quotes: u64 = out
        .rsplit("; ")
        .next()
        .and_then(|t| t.split(' ').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or(0);
    assert!(
        quotes >= 1,
        "ATTACK: --check-quoted-counts read no quote at all, so it checked nothing: {out}"
    );
}

/// Amendment 92 (M2266-M2269): the helper REFUSES when its proof fails: a destination that is not a
/// tmpfs, the host's own mount namespace, a canary that shows through to the host, and `ns_run`
/// starting its command anyway. Every attack points the assertion at scratch directories.
#[test]
fn the_namespace_helper_refuses_when_its_proof_fails() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let o = script(
        "bash",
        root.join("scripts/test_opkit_ns.sh"),
        Bins::NoWorkspaceBinary,
    )
    .output()
    .unwrap();
    if o.status.code() == Some(77) {
        eprintln!("SKIP: test_opkit_ns.sh needs root and unshare");
        return;
    }
    assert!(
        o.status.success(),
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
}
