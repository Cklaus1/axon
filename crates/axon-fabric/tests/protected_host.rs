//! O1 (`governance/specs/v022-psv-protocol.md` §2): everything that defines a
//! protected run comes from the OPERATOR's host config. The caller names a
//! suite; it never points at a launcher, manifest, registry or key.
//!
//! Negative rows:
//! * A16 — a replaced launcher (pin mismatch, at load and after load);
//! * A17 — a caller registry on a protected host, or a registry that is not
//!   the pinned bytes;
//! * A21 — every caller `--linux-*` flag.
//!
//! A20 (the signing key readable by an agent) is the trust preflight's.

mod common;
use common::*;

use axon_fabric::backend::{Clock, QualificationTrust};
use axon_fabric::protected_host::{ProtectedHost, PROTECTED_HOST_CONFIG, REFUSED_CALLER_FLAGS};
use serde_json::{json, Value};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A complete, valid host: launcher, manifest, artifacts, a signed B263
/// record, a suite registry and a signer key path, all under `root`.
struct Host {
    env: Env,
    issuer: Issuer,
    root: PathBuf,
}

impl Host {
    fn new() -> Host {
        let env = Env::new();
        let root = env.dir.path().join("host");
        std::fs::create_dir_all(root.join("dist")).unwrap();
        std::fs::create_dir_all(root.join("keys")).unwrap();
        // The service's own private leaf (A56).
        std::fs::create_dir_all(root.join("runs")).unwrap();
        std::fs::set_permissions(root.join("runs"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        // The operator's directory the custodian's socket sits in (amendment
        // 50): the socket is systemd's, the store the custodian's own.
        std::fs::create_dir_all(root.join("custodian")).unwrap();
        let issuer = Issuer::generate();
        let manifest = lx_manifest(&"a".repeat(64));
        std::fs::write(root.join("manifest.json"), &manifest).unwrap();
        // Signed B263 record + trusted issuer, written under `root`.
        qualified_linux_cfg(
            &root,
            &issuer,
            &good_evidence(&sha256_file(&root.join("manifest.json"))),
        );
        let launcher = stand_in_launcher(&env, 0, true, true, 0);
        copy_executable(&launcher, root.join("launcher.sh"), 0o755);
        // A: the privileged helper the host pins (a stand-in; load never runs it).
        std::fs::write(root.join("protected-launcher"), "\x7fELF stand-in").unwrap();
        std::fs::write(root.join("executor.bin"), "#!/bin/sh\n").unwrap();
        write_registry(
            &root.join("registry.json"),
            &root.join("executor.bin"),
            None,
        );
        // The Fabric's attestation key: owned by this uid, readable by no one else.
        let rng = ring::rand::SystemRandom::new();
        let pk8 = ring::signature::Ed25519KeyPair::generate_pkcs8(&rng).unwrap();
        std::fs::write(root.join("keys/attest.pk8"), pk8.as_ref()).unwrap();
        std::fs::set_permissions(
            root.join("keys/attest.pk8"),
            std::fs::Permissions::from_mode(0o400),
        )
        .unwrap();
        let h = Host { env, issuer, root };
        h.write_config(|_| {});
        h
    }
    fn p(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }
    fn config(&self) -> PathBuf {
        self.p("protected-host.json")
    }
    /// The conforming config, then `edit`.
    fn write_config(&self, edit: impl FnOnce(&mut Value)) {
        let pin = |n: &str| json!({"path": self.p(n), "sha256": sha256_file(&self.p(n))});
        let mut v = json!({
            "schema": "axon-protected-host/1",
            "launcher": pin("launcher.sh"),
            "privileged_launcher": pin("protected-launcher"),
            "profile_manifest": pin("manifest.json"),
            "artifacts_dir": self.p("dist"),
            "qualification": {"record": self.p("evidence.json"), "signature": null,
                              "waivers": null, "max_age_s": 2_592_000},
            "suite_registry": pin("registry.json"),
            "signer": {"issuer_ref": "verifier:fabric",
                       "public_key": axon_loop_contracts::attestation::public_key_of(
                           &std::fs::read(self.p("keys/attest.pk8")).unwrap()).unwrap(),
                       "key_path": self.p("keys/attest.pk8")},
            "out_root": self.p("runs"),
        });
        edit(&mut v);
        std::fs::write(self.config(), v.to_string()).unwrap();
    }
    fn trust(&self) -> QualificationTrust {
        let mut t = QualificationTrust::for_manifest(&self.p("manifest.json"));
        t.issuers_dir = self.p("trusted_issuers");
        t.clock = Clock::FixedUnix(axon_fabric::backend::parse_utc(TEST_NOW).unwrap());
        t
    }
    fn load(&self) -> Result<ProtectedHost, String> {
        ProtectedHost::for_test(&self.config(), None, self.trust())
    }
    /// `axon-fabric submit` with the test host config and `extra` flags.
    fn submit(&self, extra: &[&str]) -> (i32, String) {
        let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
            .arg("submit")
            .args(["--request", "/nonexistent/request.json"])
            .arg("--protected-host-config")
            .arg(self.config())
            .arg("--protected-host-issuers")
            .arg(self.p("trusted_issuers"))
            .args(extra)
            .output()
            .unwrap();
        (
            out.status.code().unwrap_or(-1),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    }
}

#[test]
fn a_conforming_host_config_defines_the_whole_protected_profile() {
    let h = Host::new();
    let ph = h.load().expect("conforming");
    assert_eq!(ph.linux.launcher, h.p("launcher.sh"));
    assert_eq!(ph.linux.launcher_sha256, sha256_file(&h.p("launcher.sh")));
    assert_eq!(ph.linux.manifest, h.p("manifest.json"));
    assert_eq!(ph.suite_registry, h.p("registry.json"));
    assert_eq!(ph.suite_registry_sha256, sha256_file(&h.p("registry.json")));
    assert_eq!(ph.signer.key_path, h.p("keys/attest.pk8"));
    assert_eq!(ph.config_sha256, sha256_file(&h.config()));
    // And it is eligible: the qualification holds under this config.
    let q = ph.linux.qualification().expect("qualified");
    assert_eq!(q.launcher_sha256, sha256_file(&h.p("launcher.sh")));
    let _ = &h.issuer;
    let _ = &h.env;
}

/// A16: a launcher that is not the pinned bytes — at load, and after load
/// (eligibility and every launch re-check it).
#[test]
fn a_replaced_launcher_is_refused_at_load_and_after_load() {
    let h = Host::new();
    let ph = h.load().expect("conforming");
    write_executable(
        &h.p("launcher.sh"),
        "#!/bin/sh\necho '{\"exit\":0}'\n",
        0o755,
    );
    let e = h.load().unwrap_err();
    assert!(e.contains("launcher") && e.contains("not its pin"), "{e}");
    let e = ph.linux.qualification().unwrap_err();
    assert!(e.contains("RULE:launcher-pinned"), "{e}");
}

/// A17: the registry that DEFINES the operator suites is the pinned one; a
/// changed registry is refused, and so is any caller registry on a protected
/// host.
#[test]
fn only_the_pinned_operator_registry_defines_suites() {
    let h = Host::new();
    let mut reg: Value =
        serde_json::from_slice(&std::fs::read(h.p("registry.json")).unwrap()).unwrap();
    reg["executors"][0]["id"] = json!("planted");
    std::fs::write(h.p("registry.json"), reg.to_string()).unwrap();
    let e = h.load().unwrap_err();
    assert!(
        e.contains("suite_registry") && e.contains("not its pin"),
        "{e}"
    );

    // A caller registry on a protected host: see
    // `a_caller_registry_never_defines_suites_on_a_protected_host`.
}

/// O1/A17: on a protected host the caller's `--check-registry` never becomes
/// the suite registry. Two layers keep it out, each on its own: the flag is
/// refused by name (M139), and the protected arm takes the host config's
/// registry whatever the caller names (M141). Which one stops it is not the
/// property (four-cell, EQUIV_RECORD["M139"]); the caller's file being LOADED
/// is. It is not a registry at all, so loading it is visible by its path.
#[test]
fn a_caller_registry_never_defines_suites_on_a_protected_host() {
    let h = Host::new();
    let caller = h.env.dir.path().join("caller-registry.json");
    std::fs::write(&caller, "a caller's registry, not the operator's").unwrap();
    let req = h.p("request.json");
    std::fs::write(&req, "{}").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .arg("submit")
        .arg("--request")
        .arg(&req)
        .arg("--protected-host-config")
        .arg(h.config())
        .arg("--protected-host-issuers")
        .arg(h.p("trusted_issuers"))
        .arg("--check-registry")
        .arg(&caller)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(
        !text.contains("caller-registry.json"),
        "ATTACK: the caller's --check-registry was loaded as the suite registry on a protected \
         host: {text}"
    );
    assert_ne!(out.status.code(), Some(0), "{text}");
    assert!(
        text.contains("--check-registry is not accepted on a protected host")
            || text.contains("pins no grant_registry"),
        "{text}"
    );
}

/// A21: every caller flag that would configure the protected profile is
/// refused, by name, before anything else is read.
#[test]
fn every_caller_protected_flag_is_refused_by_name() {
    // A complete, valid development submit: without a refused flag it runs
    // (control), so a flag that is not refused BY NAME is accepted and the
    // submit goes ahead. Nothing else on this route reads these flags, so the
    // by-name refusal is the only guard (C9 round 1b: with a nonexistent
    // request file, an io error refused the call instead).
    let env = Env::new();
    let req = env.dir.path().join("request.json");
    let dev_submit = |op: &str, extra: &[&str]| {
        let mut r = request(&env, op, "t_ok");
        r["operation_id"] = json!(op);
        std::fs::write(&req, r.to_string()).unwrap();
        let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
            .arg("submit")
            .args(extra)
            .arg("--request")
            .arg(&req)
            .arg("--journal")
            .arg(&env.journal)
            .arg("--check-registry")
            .arg(&env.registry)
            .arg("--grant-registry")
            .arg(&env.grant_registry)
            .arg("--store")
            .arg(&env.store)
            .args([
                "--tenant",
                "tenant-t",
                "--family",
                "family-f",
                "--expected-epoch",
                "0",
            ])
            .arg("--workspace")
            .arg(&env.ws)
            .arg("--state")
            .arg(env.dir.path().join("fabric-state"))
            .output()
            .unwrap();
        (
            out.status.code(),
            String::from_utf8_lossy(&out.stdout).into_owned(),
        )
    };
    let (c, out) = dev_submit("op-flag-control", &[]);
    assert_eq!(
        c,
        Some(0),
        "control: the route runs without the flag: {out}"
    );
    for (i, flag) in REFUSED_CALLER_FLAGS.iter().enumerate() {
        let (c, text) = dev_submit(&format!("op-flag-{i}"), &[flag, "/tmp/x"]);
        if c == Some(0) {
            panic!("ATTACK: {flag} was accepted: a submit ran with a caller protected-profile flag: {text}");
        }
        assert_eq!(c, Some(2), "{flag}: {text}");
        assert!(
            text.contains(&format!("{flag} is not accepted"))
                && text.contains(PROTECTED_HOST_CONFIG),
            "{flag}: {text}"
        );
    }
}

#[test]
fn the_config_schema_is_exact_and_every_path_absolute() {
    let h = Host::new();
    h.write_config(|v| v["extra_launcher"] = json!("/tmp/x"));
    assert!(h.load().unwrap_err().contains("must have exactly"));

    h.write_config(|v| v["out_root"] = json!("runs"));
    assert!(h.load().unwrap_err().contains("is not absolute"));

    h.write_config(|v| v["launcher"]["sha256"] = json!("nothex"));
    assert!(h.load().unwrap_err().contains("is not a sha256"));

    h.write_config(|v| v["schema"] = json!("axon-protected-host/0"));
    assert!(h.load().unwrap_err().contains("schema"));

    // Through the CLI too: a non-conforming host config refuses everything.
    h.write_config(|v| v["out_root"] = json!("runs"));
    let (c, out) = h.submit(&[]);
    assert_eq!(c, 4, "{out}");
    assert!(out.contains("protected host:"), "{out}");
}

/// Operator ownership of every O1 path. Needs root to create root-owned
/// fixtures and chown them away.
#[test]
fn every_o1_path_must_be_operator_owned() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let h = Host::new();
    let base = h.env.dir.path();
    for d in [base.to_path_buf(), h.root.clone(), h.p("dist"), h.p("keys")] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for f in [
        "launcher.sh",
        "manifest.json",
        "registry.json",
        "evidence.json",
        "protected-host.json",
    ] {
        std::fs::set_permissions(h.p(f), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    let load = || ProtectedHost::for_test(&h.config(), Some(base), h.trust());
    load().expect("root-owned, 0755/0644");

    type Breaker = (&'static str, fn(&Path));
    let cases: [Breaker; 3] = [
        ("writable", |p| {
            std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o666)).unwrap()
        }),
        ("not root", |p| {
            std::os::unix::fs::chown(p, Some(1000), None).unwrap()
        }),
        ("symlink", |p| {
            let real = p.with_extension("real");
            std::fs::rename(p, &real).unwrap();
            std::os::unix::fs::symlink(&real, p).unwrap();
        }),
    ];
    for f in [
        "launcher.sh",
        "registry.json",
        "manifest.json",
        "evidence.json",
        "protected-host.json",
    ] {
        for (why, break_it) in &cases {
            let saved = std::fs::read(h.p(f)).unwrap();
            break_it(&h.p(f));
            let e = load().unwrap_err();
            // A symlink is refused by the symlink check and by the mode check
            // (its lstat mode is 0777), each alone (M948/M950): either reason.
            assert!(
                e.contains(why) || (*why == "symlink" && e.contains("writable")),
                "{f} {why}: {e}"
            );
            // restore
            let _ = std::fs::remove_file(h.p(f));
            let _ = std::fs::remove_file(h.p(f).with_extension("real"));
            std::fs::write(h.p(f), saved).unwrap();
            std::fs::set_permissions(h.p(f), std::fs::Permissions::from_mode(0o644)).unwrap();
            load().expect("restored");
        }
    }
    // The artifacts directory (and what is in it) is the operator's.
    std::fs::write(h.p("dist/vmlinux"), "k").unwrap();
    std::fs::set_permissions(h.p("dist/vmlinux"), std::fs::Permissions::from_mode(0o644)).unwrap();
    load().expect("root-owned artifacts");
    std::os::unix::fs::chown(h.p("dist/vmlinux"), Some(1000), None).unwrap();
    assert!(load().unwrap_err().contains("not root"));
    std::os::unix::fs::chown(h.p("dist/vmlinux"), Some(0), None).unwrap();
    std::fs::set_permissions(h.p("dist"), std::fs::Permissions::from_mode(0o777)).unwrap();
    assert!(load().unwrap_err().contains("writable"));
    std::fs::set_permissions(h.p("dist"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // The directory holding the signing key is the operator's.
    std::os::unix::fs::chown(h.p("keys"), Some(1000), None).unwrap();
    assert!(load().unwrap_err().contains("not root"));
}

/// Where the Fabric writes its runs (`out_root`) and where the custodian's
/// socket sits (`observer.custodian.socket`; amendment 50) is operator trust
/// material too: the out_root leaf belongs to the service, and every
/// directory above either to the operator (as for the signing key). An
/// agent-writable socket directory lets the agent bind a custodian of its
/// own. Needs root to create root-owned fixtures.
#[test]
fn the_out_root_and_custodian_socket_sit_under_operator_owned_directories() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let h = Host::new();
    let base = h.env.dir.path();
    for d in [base.to_path_buf(), h.root.clone(), h.p("dist"), h.p("keys")] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for f in [
        "launcher.sh",
        "manifest.json",
        "registry.json",
        "evidence.json",
    ] {
        std::fs::set_permissions(h.p(f), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    write_executable(&h.p("observer.sh"), "#!/bin/sh\n", 0o755);
    // A directory an agent (uid 1000) can write, and one anyone can.
    let agent = h.p("agent");
    std::fs::create_dir(&agent).unwrap();
    std::os::unix::fs::chown(&agent, Some(1000), None).unwrap();
    let open = h.p("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();

    let with = |out_root: PathBuf, socket: PathBuf| {
        h.write_config(|v| {
            v["out_root"] = json!(out_root);
            v["observer"] = json!({
                "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
                "custodian": {"socket": socket, "uid": 4244},
                "max_age_s": 300,
            });
        });
        std::fs::set_permissions(h.config(), std::fs::Permissions::from_mode(0o644)).unwrap();
        ProtectedHost::for_test(&h.config(), Some(base), h.trust())
    };
    let sock = |d: &Path| d.join("custodian.sock");
    // Positive control: both under the operator's directory.
    let ph = with(h.p("runs"), sock(&h.p("custodian"))).expect("operator-owned parents");
    assert_eq!(ph.linux.out_root, h.p("runs"));

    for (dir, why) in [(&agent, "not root"), (&open, "writable")] {
        // The LEAVES themselves are the service's own and private (C9 round
        // 1b): so the parent is the ONLY thing wrong, and the parent walk the
        // only refusal (the leaf check, A56, would otherwise refuse first).
        std::fs::create_dir_all(dir.join("runs")).unwrap();
        std::fs::set_permissions(dir.join("runs"), std::fs::Permissions::from_mode(0o700)).unwrap();
        let e = with(dir.join("runs"), sock(&h.p("custodian"))).unwrap_err();
        assert!(e.contains(why), "out_root under {}: {e}", dir.display());
        let e = with(h.p("runs"), sock(dir)).unwrap_err();
        assert!(
            e.contains(why),
            "custodian socket under {}: {e}",
            dir.display()
        );
    }
}

/// C9 dev review round 1 (A56): the out_root LEAF, not only its parent. The
/// leaf sits directly under an operator-owned parent (so the parent walk
/// passes) but is agent-owned, group/other-accessible, a symlink, or absent.
/// An agent that owns or can write the leaf can swap `<op>.psv-inputs`
/// between `prepare` and the launcher's copy. (The nonce store was the other
/// leaf here; since amendment 50 it is the custodian's own, never Fabric's:
/// `custodian::check_store`.) Control: the service-owned 0700 leaf loads
/// (`the_out_root_and_custodian_socket_sit_under_operator_owned_directories`
/// and the end of this test). Needs root to create root-owned parents.
#[test]
fn the_out_root_leaf_is_the_services_own_and_private() {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let h = Host::new();
    let base = h.env.dir.path();
    for d in [base.to_path_buf(), h.root.clone(), h.p("dist"), h.p("keys")] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for f in [
        "launcher.sh",
        "manifest.json",
        "registry.json",
        "evidence.json",
    ] {
        std::fs::set_permissions(h.p(f), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    write_executable(&h.p("observer.sh"), "#!/bin/sh\n", 0o755);
    let with = |out_root: PathBuf| {
        h.write_config(|v| {
            v["out_root"] = json!(out_root);
            v["observer"] = json!({
                "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
                "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244},
                "max_age_s": 300,
            });
        });
        std::fs::set_permissions(h.config(), std::fs::Permissions::from_mode(0o644)).unwrap();
        ProtectedHost::for_test(&h.config(), Some(base), h.trust())
    };
    // A symlinked leaf is a_symlinked_service_leaf_is_refused (any refusal:
    // three checks refuse it, and the symlink check is retired, M487).
    type Breaker = (&'static str, &'static str, fn(&Path));
    let cases: [Breaker; 4] = [
        ("an agent-owned leaf", "not the service uid", |p| {
            std::os::unix::fs::chown(p, Some(1000), None).unwrap()
        }),
        (
            "a group-writable leaf",
            "accessible to group or other",
            |p| std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o770)).unwrap(),
        ),
        (
            "an other-writable leaf",
            "accessible to group or other",
            |p| std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o703)).unwrap(),
        ),
        ("an absent leaf", "must exist", |p| {
            std::fs::remove_dir(p).unwrap()
        }),
    ];
    for leaf in ["runs"] {
        for (what, why, break_it) in &cases {
            let p = h.p(leaf);
            break_it(&p);
            let got = with(h.p("runs"));
            assert!(
                got.as_ref().is_err_and(|e| e.contains(why)),
                "ATTACK: {leaf} as {what} was accepted as the service's own private directory: \
                 {:?}",
                got.map(|_| ())
            );
            // restore
            let _ = std::fs::remove_file(&p);
            let _ = std::fs::remove_dir(&p);
            if p.with_extension("real").exists() {
                std::fs::rename(p.with_extension("real"), &p).unwrap();
            } else {
                std::fs::create_dir(&p).unwrap();
            }
            std::os::unix::fs::chown(&p, Some(0), None).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
            with(h.p("runs")).expect("control: restored leaves load");
        }
    }
}

/// A host whose `out_root` leaf is the service's own 0700 directory, with the
/// loader for it (root only: the fixtures are root-owned).
fn leaf_host() -> Option<(Host, PathBuf)> {
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return None;
    }
    let h = Host::new();
    let base = h.env.dir.path().to_path_buf();
    for d in [base.clone(), h.root.clone(), h.p("dist"), h.p("keys")] {
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    for f in [
        "launcher.sh",
        "manifest.json",
        "registry.json",
        "evidence.json",
    ] {
        std::fs::set_permissions(h.p(f), std::fs::Permissions::from_mode(0o644)).unwrap();
    }
    h.write_config(|v| v["out_root"] = json!(h.p("runs")));
    std::fs::set_permissions(h.config(), std::fs::Permissions::from_mode(0o644)).unwrap();
    std::fs::set_permissions(h.p("runs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    ProtectedHost::for_test(&h.config(), Some(&base), h.trust())
        .expect("control: the service's own 0700 directory loads");
    Some((h, base))
}

/// A service leaf that is a SYMLINK to the service's own 0700 directory is
/// refused (C9 round 2, harness). Three checks refuse it: the symlink check,
/// `!is_dir()` (symlink_metadata never reports a directory for a link) and the
/// mode check (a Linux link is 0777), so any refusal is accepted here and the
/// symlink check is retired against the other two (four-cell record). The
/// ATTACK fires only when the link LOADS.
#[test]
fn a_symlinked_service_leaf_is_refused() {
    let Some((h, base)) = leaf_host() else { return };
    let runs = h.p("runs");
    let real = h.p("runs-real");
    std::fs::rename(&runs, &real).unwrap();
    std::os::unix::fs::symlink(&real, &runs).unwrap();
    let got = ProtectedHost::for_test(&h.config(), Some(&base), h.trust());
    assert!(
        got.is_err(),
        "ATTACK: a symlinked service leaf was accepted as the service's own directory: {:?}",
        got.map(|_| ())
    );
}

/// A service leaf that is a REGULAR FILE (the service's own, 0600) is refused:
/// only `!is_dir()` refuses it (C9 round 2, harness). The ATTACK fires only
/// when it LOADS; a refusal for another reason is reported separately.
#[test]
fn a_regular_file_service_leaf_is_refused() {
    let Some((h, base)) = leaf_host() else { return };
    let runs = h.p("runs");
    std::fs::remove_dir(&runs).unwrap();
    std::fs::write(&runs, "").unwrap();
    std::fs::set_permissions(&runs, std::fs::Permissions::from_mode(0o600)).unwrap();
    let got = ProtectedHost::for_test(&h.config(), Some(&base), h.trust());
    assert!(
        got.is_err(),
        "ATTACK: a regular file was accepted as the service's own directory: {:?}",
        got.map(|_| ())
    );
    assert!(
        got.as_ref().is_err_and(|e| e.contains("not a directory")),
        "refused for another reason: {:?}",
        got.map(|_| ())
    );
}

/// ADR-002 key-role separation when the host config LOADS (C9 dev review
/// round 1; A57): the observer root may hold neither the host signer's public
/// key (Fabric holds its private half and signs any domain) nor a key of
/// another operator authority root. Control: a key in no other role loads.
#[test]
fn an_observer_root_sharing_a_key_with_another_role_is_refused_at_load() {
    let h = Host::new();
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    let observer_config = |v: &mut Value| {
        v["observer"] = json!({
            "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
            "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244},
        });
    };
    h.write_config(observer_config);
    let signer = axon_loop_contracts::attestation::public_key_of(
        &std::fs::read(h.p("keys/attest.pk8")).unwrap(),
    )
    .unwrap();
    let obs_root = h.p("observer");
    std::fs::create_dir_all(&obs_root).unwrap();
    let other = Issuer::generate();
    std::fs::write(
        obs_root.join("obs.pub"),
        format!("{}\n", other.public_hex()),
    )
    .unwrap();
    h.load().expect("control: an observer key in no other role");

    // The observer root holds the host signer's key.
    std::fs::write(obs_root.join("signer.pub"), format!("{signer}\n")).unwrap();
    let got = h.load();
    assert!(
        got.as_ref().is_err_and(|e| e.contains("host signer")),
        "ATTACK: an observer root holding the host signer's public key loaded: {:?}",
        got.map(|_| ())
    );
    std::fs::remove_file(obs_root.join("signer.pub")).unwrap();
    h.load().expect("restored");

    // The observer key is also a key of another authority.
    for (role, dir) in [
        ("qualification", h.p("trusted_issuers")),
        ("verifier", h.p("verifier")),
        ("admission", h.p("admission")),
        ("monitor", h.p("monitor")),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        let dup = dir.join("dup.pub");
        std::fs::write(&dup, format!("{}\n", other.public_hex())).unwrap();
        let got = h.load();
        assert!(
            got.as_ref()
                .is_err_and(|e| e.contains("key-role separation") && e.contains(role)),
            "ATTACK: an observer key that is also a {role} key loaded: {:?}",
            got.map(|_| ())
        );
        std::fs::remove_file(&dup).unwrap();
        h.load().expect("restored");
    }
}

/// FIELD-ORIGIN (C9 round 2; A67): key-role separation held on the observer
/// route only. The host signer's public key in ANY authority root but the
/// verifier's (its intended place) lets Fabric, which holds the private half,
/// mint that authority's statements; `load` refuses it for the qualification,
/// observer, admission and monitor roots. Control: in the verifier root it
/// loads.
#[test]
fn the_host_signer_key_in_any_root_but_the_verifiers_is_refused_at_load() {
    let h = Host::new();
    h.load().expect("control: a conforming host");
    let signer = axon_loop_contracts::attestation::public_key_of(
        &std::fs::read(h.p("keys/attest.pk8")).unwrap(),
    )
    .unwrap();
    for (role, dir) in [
        ("qualification", h.p("trusted_issuers")),
        ("observer", h.p("observer")),
        ("admission", h.p("admission")),
        ("monitor", h.p("monitor")),
    ] {
        std::fs::create_dir_all(&dir).unwrap();
        let planted = dir.join("signer.pub");
        std::fs::write(&planted, format!("{signer}\n")).unwrap();
        let got = h.load();
        assert!(
            got.as_ref()
                .is_err_and(|e| e.contains("host signer") && e.contains(role)),
            "ATTACK: a {role} root holding the host signer's public key loaded: {:?}",
            got.map(|_| ())
        );
        std::fs::remove_file(&planted).unwrap();
        h.load().expect("restored");
    }
    std::fs::create_dir_all(h.p("verifier")).unwrap();
    std::fs::write(h.p("verifier/signer.pub"), format!("{signer}\n")).unwrap();
    h.load()
        .expect("control: the verifier root is where the host signer's key belongs");
}

/// Fabric's host signing key.
fn fabric_key(h: &Host) -> Issuer {
    Issuer(
        ring::signature::Ed25519KeyPair::from_pkcs8(
            &std::fs::read(h.p("keys/attest.pk8")).unwrap(),
        )
        .unwrap(),
    )
}

/// FIELD-ORIGIN (C9 round 2; A67), at EVERY qualification, not only at load:
/// once the host has loaded, the host signer's public key is planted in the
/// qualification root and Fabric signs its own B263 record with the private
/// half it holds. `qualification()` must not accept it. Control: the same
/// record signed by the operator's issuer qualifies.
#[test]
fn a_b263_record_minted_with_the_host_signer_key_never_qualifies() {
    let h = Host::new();
    let ph = h.load().expect("control: a conforming host");
    ph.linux
        .qualification()
        .expect("control: the operator's record qualifies");
    let fabric = fabric_key(&h);
    fabric.trust_in(&h.p("trusted_issuers"), "planted");
    let mut ev = good_evidence(&sha256_file(&h.p("manifest.json")));
    ev["issuer_key_id"] = json!(fabric.key_id());
    fabric.write_signed(&h.p("evidence.json"), &ev);
    let got = ph.linux.qualification().map(|q| q.issuer);
    assert!(
        got.is_err(),
        "ATTACK: a B263 record minted with the host signer's key qualified the protected \
         profile: {got:?}"
    );
    assert!(got.unwrap_err().contains("host signer"));
}

/// FIELD-ORIGIN (C9 round 2; A67): a qualification key that the verifier
/// root also holds is authority for both, so `qualification()` refuses it at
/// every read (as the loop's `exclusive` does). Control: before the verifier
/// root holds it, the record qualifies.
#[test]
fn a_qualification_key_shared_with_the_verifier_root_never_qualifies() {
    let h = Host::new();
    let ph = h.load().expect("control: a conforming host");
    ph.linux.qualification().expect("control: qualifies");
    h.issuer.trust_in(&h.p("verifier"), "shared");
    let got = ph.linux.qualification().map(|q| q.issuer);
    assert!(
        got.is_err(),
        "ATTACK: a qualification key also held by the verifier root qualified the protected \
         profile: {got:?}"
    );
    let e = got.unwrap_err();
    assert!(
        e.contains("key-role separation") && e.contains("verifier"),
        "{e}"
    );
}

/// The trust preflight's probe list IS what `load` enforces (C9 dev review
/// round 1; A56 / FIELD-ORIGIN preflight coverage). `pinned_paths` (printed by
/// `axon-fabric protected-host-paths`, which the preflight runs) is checked
/// against `load` in BOTH directions on a full host (observer, grant registry,
/// artifacts): every path whose ownership `load` refuses is on the list (or is
/// an ancestor of, or a direct entry of, a listed path), and every listed path,
/// made agent-owned, is refused by `load`. The preflight used to keep its own
/// list and so missed the grant registry and grant files, the observer command,
/// `artifacts_dir` and the out_root / nonce_store directories. Needs root.
#[test]
fn the_preflight_probe_list_is_exactly_what_load_enforces() {
    use axon_fabric::protected_host::{pinned_paths, PinnedKind};
    if unsafe { libc::geteuid() } != 0 {
        eprintln!("skipped: needs root to create root-owned fixtures");
        return;
    }
    let h = Host::new();
    let base = h.env.dir.path();
    std::fs::create_dir_all(h.p("grants")).unwrap();
    write_grant_registry(
        &h.p("grants/grants.json"),
        &[("grant:test", PRINCIPAL, GRANT_FS)],
    );
    std::fs::write(h.p("dist/vmlinux"), "k").unwrap();
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    std::fs::create_dir_all(h.p("authority/loop-store")).unwrap();
    h.write_config(|v| {
        let pin = |n: &str| json!({"path": h.p(n), "sha256": sha256_file(&h.p(n))});
        v["observer"] = json!({"command": pin("observer.sh"),
                               "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244}});
        v["grant_registry"] = pin("grants/grants.json");
        v["authority_store"] = json!(h.p("authority/loop-store"));
    });
    // Everything the operator's, 0755/0644; the key and the service dirs as set.
    let mut all: Vec<PathBuf> = vec![base.to_path_buf()];
    let mut i = 0;
    while i < all.len() {
        if all[i].is_dir() && !all[i].is_symlink() {
            let mut sub: Vec<PathBuf> = std::fs::read_dir(&all[i])
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            sub.sort();
            all.extend(sub);
        }
        i += 1;
    }
    for p in &all {
        let mode = if p.is_dir() { 0o755 } else { 0o644 };
        std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode)).unwrap();
    }
    std::fs::set_permissions(h.p("runs"), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(
        h.p("keys/attest.pk8"),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let load = || ProtectedHost::for_test(&h.config(), Some(base), h.trust());
    load().expect("control: the full host loads");

    let listed = pinned_paths(&h.config()).unwrap();
    for want in [
        h.p("grants/grants.json"),
        h.p("grants/grant_test.axgrant"),
        h.p("observer.sh"),
        h.p("dist"),
        h.p("runs"),
        h.p("custodian/custodian.sock"),
        h.p("authority/loop-store"),
    ] {
        assert!(
            listed.iter().any(|(_, p)| *p == want),
            "ATTACK: the trust preflight never probes {}, which load pins: {listed:?}",
            want.display()
        );
    }
    let covered = |x: &Path| {
        listed.iter().any(|(k, p)| {
            p == x
                || p.starts_with(x)
                || (*k == PinnedKind::OperatorDir && x.parent() == Some(p.as_path()))
        })
    };
    // load ⊆ list: whatever load refuses for its ownership, the preflight probes.
    for x in &all {
        std::os::unix::fs::chown(x, Some(1000), None).unwrap();
        let refused = load().is_err();
        std::os::unix::fs::chown(x, Some(0), None).unwrap();
        assert!(
            !refused || covered(x),
            "ATTACK: load enforces the ownership of {} but the trust preflight never probes it",
            x.display()
        );
    }
    load().expect("restored");
    // list ⊆ load: every listed path is one load really enforces.
    for (k, p) in &listed {
        let target = match k {
            // The key FILE is checked when the binary reads it; load walks
            // the directory holding it. The custodian's socket is systemd's
            // (absent here); load walks the directory it is bound in.
            PinnedKind::SigningKey | PinnedKind::CustodianSocket => {
                p.parent().unwrap().to_path_buf()
            }
            // A82: the store is the loop's; load walks the directory holding it.
            PinnedKind::AuthorityStore => p.parent().unwrap().to_path_buf(),
            _ => p.clone(),
        };
        std::os::unix::fs::chown(&target, Some(1000), None).unwrap();
        let refused = load().is_err();
        std::os::unix::fs::chown(&target, Some(0), None).unwrap();
        assert!(
            refused,
            "the preflight probes {} ({}), which load does not enforce",
            target.display(),
            k.as_str()
        );
    }
}

/// On this (development) host there is no operator host config: the
/// protected profile is simply not configured — never a fallback.
#[test]
fn without_the_operator_file_the_profile_is_not_configured() {
    if Path::new(PROTECTED_HOST_CONFIG).exists() {
        eprintln!("skipped: an operator host config exists on this host");
        return;
    }
    assert!(ProtectedHost::operator().unwrap().is_none());
}

/// On a protected host the suite registry IS the operator's: with no
/// `--check-registry` at all, submit loads the host config's registry and goes
/// on to the next requirement (the grant registry), never asking the caller
/// for one. The grant registry is the operator's too (D1): this host config
/// pins none, so nothing is authorized — and the caller is not asked for one
/// (`grant_registry_authority.rs`).
#[test]
fn a_protected_host_loads_the_operators_registry_not_the_callers() {
    let h = Host::new();
    let req = h.p("request.json");
    std::fs::write(&req, "{}").unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
        .arg("submit")
        .arg("--request")
        .arg(&req)
        .arg("--protected-host-config")
        .arg(h.config())
        .arg("--protected-host-issuers")
        .arg(h.p("trusted_issuers"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("--check-registry"), "{text}");
    assert!(!text.contains("--grant-registry"), "{text}");
    assert!(text.contains("pins no grant_registry"), "{text}");
}

/// The protected signer is the host config's: a key readable by anyone else,
/// or one that does not derive the pinned public key, refuses everything.
#[test]
fn the_host_signer_key_must_be_private_and_match_its_pin() {
    let h = Host::new();
    // The host pins a (valid, empty) grant registry, so the D1 grant check
    // PASSES on this route and the signer checks are the only guard left
    // before the run goes on to read its arguments. Without a pinned registry
    // the D1 refusal would stop the call first, and removing the signer pin
    // would be refused elsewhere, not caught (C9 dev review, M140).
    std::fs::create_dir_all(h.p("grants")).unwrap();
    write_grant_registry(&h.p("grants/grants.json"), &[]);
    let grants = json!({"path": h.p("grants/grants.json"),
                        "sha256": sha256_file(&h.p("grants/grants.json"))});
    h.write_config(|v| v["grant_registry"] = grants.clone());
    let req = h.p("request.json");
    std::fs::write(&req, "{}").unwrap();
    let run = || {
        let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
            .arg("submit")
            .arg("--request")
            .arg(&req)
            .arg("--protected-host-config")
            .arg(h.config())
            .arg("--protected-host-issuers")
            .arg(h.p("trusted_issuers"))
            .output()
            .unwrap();
        String::from_utf8_lossy(&out.stdout).into_owned()
    };
    let key = h.p("keys/attest.pk8");
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o640)).unwrap();
    let t = run();
    assert!(
        !t.contains("grant_registry"),
        "setup: the D1 grant check refused first: {t}"
    );
    assert!(
        t.contains("protected-host signer") && t.contains("readable by no one else"),
        "ATTACK: a group-readable host signer key was not refused: {t}"
    );
    // rows4b (amendment 62): group-READABLE but writable by no one (0440), so
    // only the read bits refuse it (0640 is also owner-writable).
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o440)).unwrap();
    let t = run();
    assert!(
        t.contains("protected-host signer") && t.contains("readable by no one else"),
        "ATTACK: a group-readable (0440) host signer key was not refused: {t}"
    );
    // Spec §2 rule 1: mode 0400. An owner-WRITABLE key (0600) is refused too:
    // the service that holds it must not be able to replace it.
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    let t = run();
    assert!(
        t.contains("protected-host signer") && t.contains("writable by no one"),
        "ATTACK: an owner-writable (0600) signing key was accepted, but the protocol requires \
         0400: {t}"
    );
    // rows4b (amendment 62): 0400 but owned by ANOTHER uid, which could read
    // it, or replace it, as its owner.
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o400)).unwrap();
    std::os::unix::fs::chown(&key, Some(1000), None).unwrap();
    let t = run();
    assert!(
        t.contains("protected-host signer") && t.contains("must be owned by this uid"),
        "ATTACK: a host signer key owned by another uid was accepted: {t}"
    );
    std::os::unix::fs::chown(&key, Some(unsafe { libc::geteuid() }), None).unwrap();
    h.write_config(|v| {
        v["signer"]["public_key"] = json!("ab".repeat(32));
        v["grant_registry"] = grants.clone();
    });
    let t = run();
    assert!(
        !t.contains("grant_registry"),
        "setup: the D1 grant check refused first: {t}"
    );
    assert!(
        t.contains("does not derive the pinned public_key"),
        "ATTACK: a host signer key that does not derive its pin was not refused: {t}"
    );
}

/// A: the privileged helper's operator config and the host config describe
/// ONE launch path. A helper admitting another uid, writing elsewhere, or
/// running another launcher or profile manifest than the host pins would
/// execute bytes no launch manifest or observation names.
#[test]
fn the_helper_config_must_agree_with_the_host_config() {
    use axon_fabric::privileged_launcher::HelperConfig;
    use axon_fabric::protected_host::helper_agrees;
    let h = Host::new();
    let euid = unsafe { libc::geteuid() };
    // Amendment 50: a host with an observer, whose nonce the custodian issues.
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    Issuer::generate().trust_in(&h.p("observer"), "obs");
    h.write_config(|v| {
        v["observer"] = json!({
            "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
            "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244},
        });
    });
    let host = h.load().unwrap();
    let good = json!({
        "schema": "axon-protected-launcher/2", "fabric_uid": euid,
        "interpreter": {"path": "/bin/bash", "sha256": "a".repeat(64)},
        "launcher": {"path": h.p("launcher.sh"), "sha256": sha256_file(&h.p("launcher.sh"))},
        "profile_manifest": {"path": h.p("manifest.json"),
                             "sha256": sha256_file(&h.p("manifest.json"))},
        "artifacts_dir": h.p("dist"), "firecracker": "/usr/local/bin/firecracker",
        "jailer": "/usr/local/bin/jailer", "out_root": h.p("runs"),
        "staging_root": h.p("staging"), "max_timeout_s": 60, "max_input_bytes": 1,
        "observer": {"root": "/etc/axon/trust/observer", "max_age_s": 300,
                     "host_signer_public_key": host.signer.public_key},
        "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244},
    });
    let cfg = |edit: &dyn Fn(&mut Value)| -> HelperConfig {
        let mut v = good.clone();
        edit(&mut v);
        serde_json::from_value(v).unwrap()
    };
    helper_agrees(&cfg(&|_| {}), &host, euid).expect("control: one launch path");
    let why = helper_agrees(
        &cfg(&|v| v["launcher"]["sha256"] = json!("b".repeat(64))),
        &host,
        euid,
    )
    .expect_err("ATTACK: a helper config running another launcher than the host pins was accepted");
    assert!(why.contains("runs launcher"), "{why}");
    let why = helper_agrees(&cfg(&|v| v["fabric_uid"] = json!(euid + 1)), &host, euid)
        .expect_err("ATTACK: a helper config admitting another uid than Fabric's was accepted");
    assert!(why.contains("admits uid"), "{why}");
    for edit in [
        (|v: &mut Value| v["out_root"] = json!("/var/lib/elsewhere")) as fn(&mut Value),
        |v| v["profile_manifest"]["sha256"] = json!("c".repeat(64)),
    ] {
        assert!(helper_agrees(&cfg(&edit), &host, euid).is_err());
    }
    // Amendment 50: the helper spends through the custodian the host's
    // observer is issued by, never another (a socket Fabric could serve).
    for edit in [
        (|v: &mut Value| v["custodian"]["uid"] = json!(4245)) as fn(&mut Value),
        |v| v["custodian"]["socket"] = json!("/run/elsewhere/custodian.sock"),
    ] {
        let got = helper_agrees(&cfg(&edit), &host, euid);
        assert!(
            got.as_ref()
                .is_err_and(|e| e.contains("spends through custodian")),
            "ATTACK: a helper config spending through another custodian than the host's was \
             accepted: {got:?}"
        );
    }
    // …and keeps the host signer out of the root it verifies observations
    // under: it names the host's signer, never another key.
    let got = helper_agrees(
        &cfg(&|v| v["observer"]["host_signer_public_key"] = json!("d".repeat(64))),
        &host,
        euid,
    );
    assert!(
        got.as_ref().is_err_and(|e| e.contains("host signer")),
        "ATTACK: a helper config naming another host signer than the host's was accepted: {got:?}"
    );
}

/// Amendment 50 (A83): the nonce store is the CUSTODIAN's, never Fabric's. A
/// host config that still gives Fabric one (`observer.nonce_store`, the old
/// A56 shape) is refused, not read as a store Fabric keeps. Control: the
/// same config naming only the custodian loads.
#[test]
fn a_host_config_giving_fabric_a_nonce_store_is_refused() {
    let h = Host::new();
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    Issuer::generate().trust_in(&h.p("observer"), "obs");
    std::fs::create_dir_all(h.p("nonces")).unwrap();
    let with = |store: bool| {
        h.write_config(|v| {
            v["observer"] = json!({
                "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
                "custodian": {"socket": h.p("custodian/custodian.sock"), "uid": 4244},
            });
            if store {
                v["observer"]["nonce_store"] = json!(h.p("nonces"));
            }
        });
        h.load()
    };
    let got = with(true);
    assert!(
        got.is_err(),
        "ATTACK: a host config giving Fabric its own nonce store loaded: {:?}",
        got.map(|_| ())
    );
    let ob = with(false)
        .expect("control: the custodian only")
        .observer
        .unwrap();
    assert!(matches!(
        ob.custodian,
        axon_fabric::custodian::Custodian::Service(ref c) if c.uid == 4244
    ));
}
