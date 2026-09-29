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
        // The service's own private leaves (A56).
        for d in ["runs", "nonces"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
            std::fs::set_permissions(root.join(d), std::fs::Permissions::from_mode(0o700)).unwrap();
        }
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
        std::fs::copy(&launcher, root.join("launcher.sh")).unwrap();
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
            std::fs::Permissions::from_mode(0o600),
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
    std::fs::write(h.p("launcher.sh"), "#!/bin/sh\necho '{\"exit\":0}'\n").unwrap();
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

    let h = Host::new();
    let (c, out) = h.submit(&["--check-registry", "/tmp/mine.json"]);
    assert_eq!(c, 2, "{out}");
    assert!(
        out.contains("--check-registry is not accepted on a protected host"),
        "{out}"
    );
}

/// A21: every caller flag that would configure the protected profile is
/// refused, by name, before anything else is read.
#[test]
fn every_caller_protected_flag_is_refused_by_name() {
    for flag in REFUSED_CALLER_FLAGS {
        let out = Command::new(env!("CARGO_BIN_EXE_axon-fabric"))
            .args(["submit", flag, "/tmp/x", "--request", "/nonexistent"])
            .output()
            .unwrap();
        let text = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.status.code(), Some(2), "{flag}: {text}");
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
            assert!(e.contains(why), "{f} {why}: {e}");
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

/// Where the Fabric writes its runs (`out_root`) and the custodian its nonce
/// records (`observer.nonce_store`) is operator trust material too: the leaf
/// belongs to the service, every directory above it to the operator (as for
/// the signing key). Needs root to create root-owned fixtures.
#[test]
fn the_out_root_and_nonce_store_sit_under_operator_owned_directories() {
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
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(h.p("observer.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    // A directory an agent (uid 1000) can write, and one anyone can.
    let agent = h.p("agent");
    std::fs::create_dir(&agent).unwrap();
    std::os::unix::fs::chown(&agent, Some(1000), None).unwrap();
    let open = h.p("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o777)).unwrap();

    let with = |out_root: PathBuf, nonces: PathBuf| {
        h.write_config(|v| {
            v["out_root"] = json!(out_root);
            v["observer"] = json!({
                "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
                "nonce_store": nonces,
                "max_age_s": 300,
            });
        });
        std::fs::set_permissions(h.config(), std::fs::Permissions::from_mode(0o644)).unwrap();
        ProtectedHost::for_test(&h.config(), Some(base), h.trust())
    };
    // Positive control: both under the operator's directory.
    let ph = with(h.p("runs"), h.p("nonces")).expect("operator-owned parents");
    assert_eq!(ph.linux.out_root, h.p("runs"));

    for (dir, why) in [(&agent, "not root"), (&open, "writable")] {
        let e = with(dir.join("runs"), h.p("nonces")).unwrap_err();
        assert!(e.contains(why), "out_root under {}: {e}", dir.display());
        let e = with(h.p("runs"), dir.join("nonces")).unwrap_err();
        assert!(e.contains(why), "nonce_store under {}: {e}", dir.display());
    }
}

/// C9 dev review round 1 (A56): the out_root and nonce_store LEAVES, not only
/// their parents. Each leaf sits directly under an operator-owned parent (so
/// the parent walk passes) but is agent-owned, group/other-accessible, a
/// symlink, or absent. An agent that owns or can write the leaf can swap
/// `<op>.psv-inputs` between `prepare` and the launcher's copy, or erase
/// nonce records so an observation replays. Control: the service-owned 0700
/// leaves load (`the_out_root_and_nonce_store_sit_under_operator_owned_directories`
/// and the end of this test). Needs root to create root-owned parents.
#[test]
fn the_out_root_and_nonce_store_leaves_are_the_services_own_and_private() {
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
    std::fs::write(h.p("observer.sh"), "#!/bin/sh\n").unwrap();
    std::fs::set_permissions(h.p("observer.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let with = |out_root: PathBuf, nonces: PathBuf| {
        h.write_config(|v| {
            v["out_root"] = json!(out_root);
            v["observer"] = json!({
                "command": {"path": h.p("observer.sh"), "sha256": sha256_file(&h.p("observer.sh"))},
                "nonce_store": nonces,
                "max_age_s": 300,
            });
        });
        std::fs::set_permissions(h.config(), std::fs::Permissions::from_mode(0o644)).unwrap();
        ProtectedHost::for_test(&h.config(), Some(base), h.trust())
    };
    type Breaker = (&'static str, &'static str, fn(&Path));
    let cases: [Breaker; 5] = [
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
        ("a symlinked leaf", "symlink", |p| {
            let real = p.with_extension("real");
            std::fs::rename(p, &real).unwrap();
            std::os::unix::fs::symlink(&real, p).unwrap();
        }),
        ("an absent leaf", "must exist", |p| {
            std::fs::remove_dir(p).unwrap()
        }),
    ];
    for leaf in ["runs", "nonces"] {
        for (what, why, break_it) in &cases {
            let p = h.p(leaf);
            break_it(&p);
            let got = with(h.p("runs"), h.p("nonces"));
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
            with(h.p("runs"), h.p("nonces")).expect("control: restored leaves load");
        }
    }
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
            "nonce_store": h.p("nonces"),
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
    h.write_config(|v| {
        let pin = |n: &str| json!({"path": h.p(n), "sha256": sha256_file(&h.p(n))});
        v["observer"] = json!({"command": pin("observer.sh"), "nonce_store": h.p("nonces")});
        v["grant_registry"] = pin("grants/grants.json");
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
    for d in ["runs", "nonces"] {
        std::fs::set_permissions(h.p(d), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
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
        h.p("nonces"),
    ] {
        assert!(
            listed.iter().any(|(_, p)| *p == want),
            "the probe list omits {}: {listed:?}",
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
            // the directory holding it.
            PinnedKind::SigningKey => p.parent().unwrap().to_path_buf(),
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
        t.contains("protected-host signer") && t.contains("readable by no one else"),
        "{t}"
    );
    std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
    h.write_config(|v| v["signer"]["public_key"] = json!("ab".repeat(32)));
    let t = run();
    assert!(t.contains("does not derive the pinned public_key"), "{t}");
}
