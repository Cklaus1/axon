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
