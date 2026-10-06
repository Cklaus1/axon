//! C9 round 5 (FIELD-ORIGIN, major-adjacent 1; amendment 80): the host-side
//! production binaries have no constructed build environment, so what cargo
//! shows the build script -- a rustc wrapper, a workspace wrapper, rustflags, a
//! configured linker -- is recorded in the build identity and REFUSES a
//! production build (release profile, no test-trust feature).
//!
//! The refusal is driven through the REAL `build.rs` (copied, with the three
//! sources it includes, into a one-crate scratch package and built by cargo):
//! a unit test of the helper alone would not notice `build.rs` forgetting to
//! call it.

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

/// A scratch package whose build script is axon-fabric's own.
fn package(d: &Path) -> PathBuf {
    let p = d.join("fx");
    for f in [
        "build.rs",
        "src/build_state.rs",
        "src/git_data.rs",
        "src/provenance.rs",
    ] {
        write(
            &p.join(f),
            &std::fs::read_to_string(root().join(f)).unwrap(),
        );
    }
    write(
        &p.join("Cargo.toml"),
        "[package]\nname = \"fx\"\nversion = \"0.0.0\"\nedition = \"2021\"\n\
         [features]\ntest-trust-root = []\n[workspace]\n",
    );
    write(
        &p.join("src/main.rs"),
        "fn main() { println!(\"[{}]\", env!(\"AXON_FABRIC_BUILD_STATE\")); }\n",
    );
    write(
        &p.join("rust-toolchain.toml"),
        &std::fs::read_to_string(root().join("../../rust-toolchain.toml")).unwrap(),
    );
    p
}

fn host() -> String {
    let o = Command::new("rustc").arg("-vV").output().unwrap();
    String::from_utf8_lossy(&o.stdout)
        .lines()
        .find_map(|l| l.strip_prefix("host: ").map(str::to_string))
        .unwrap()
}

/// `cargo build [args]` in `p` with only `env` of the wrapper family set.
fn build(p: &Path, args: &[&str], env: &[(&str, String)]) -> Output {
    let mut c = Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    c.arg("build").args(args).current_dir(p);
    for v in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "RUSTFLAGS",
        "CARGO_ENCODED_RUSTFLAGS",
        "CARGO_BUILD_RUSTFLAGS",
        "CARGO_BUILD_RUSTC_WRAPPER",
        "CARGO_TARGET_DIR",
    ] {
        c.env_remove(v);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// What the built fixture reports as its build state.
fn reported(p: &Path, profile: &str) -> String {
    let o = Command::new(p.join("target").join(profile).join("fx"))
        .output()
        .unwrap();
    String::from_utf8_lossy(&o.stdout).trim().to_string()
}

fn wrapper(d: &Path) -> String {
    use std::os::unix::fs::PermissionsExt;
    let w = d.join("wrapper");
    write(&w, "#!/bin/sh\nexec \"$@\"\n");
    std::fs::set_permissions(&w, std::fs::Permissions::from_mode(0o755)).unwrap();
    w.display().to_string()
}

/// Each way of putting something between the sources and the bytes refuses a
/// PRODUCTION build. Control: the same build with none of them builds, and its
/// identity records an empty state.
#[test]
fn a_production_build_under_a_wrapper_rustflags_or_linker_does_not_happen() {
    let d = tempfile::tempdir().unwrap();
    let p = package(d.path());
    let w = wrapper(d.path());
    let linker = format!("target.{}.linker=\"/usr/bin/cc\"", host());
    type Attack<'a> = (&'a str, Vec<&'a str>, Vec<(&'a str, String)>);
    let attacks: Vec<Attack> = vec![
        ("RUSTC_WRAPPER", vec![], vec![("RUSTC_WRAPPER", w.clone())]),
        (
            "RUSTC_WORKSPACE_WRAPPER",
            vec![],
            vec![("RUSTC_WORKSPACE_WRAPPER", w.clone())],
        ),
        (
            "RUSTFLAGS",
            vec![],
            vec![("RUSTFLAGS", "--cfg evil".into())],
        ),
        (
            "CARGO_BUILD_RUSTFLAGS",
            vec![],
            vec![("CARGO_BUILD_RUSTFLAGS", "--cfg evil".into())],
        ),
        ("a configured linker", vec!["--config", &linker], vec![]),
    ];
    for (attack, extra, env) in attacks {
        let mut args = vec!["--release"];
        args.extend(extra);
        let o = build(&p, &args, &env);
        assert!(
            !o.status.success(),
            "ATTACK: a production build ran under {attack}:\n{}",
            text(&o)
        );
        assert!(
            text(&o).contains("refusing a production build"),
            "{attack}: {}",
            text(&o)
        );
    }
    let o = build(&p, &["--release"], &[]);
    assert!(
        o.status.success(),
        "control: a clean production build builds: {}",
        text(&o)
    );
    assert_eq!(reported(&p, "release"), "[]");
}

/// A build that is NOT a production one (debug, or release with the test-trust
/// feature a development `cargo test` unifies in) still builds under a wrapper
/// or flags -- and RECORDS it, so the identity never says "nothing".
#[test]
fn a_development_build_under_a_wrapper_records_it_in_its_identity() {
    let d = tempfile::tempdir().unwrap();
    let p = package(d.path());
    let w = wrapper(d.path());
    let o = build(&p, &[], &[("RUSTC_WRAPPER", w.clone())]);
    assert!(
        o.status.success(),
        "a debug build under a wrapper builds: {}",
        text(&o)
    );
    let got = reported(&p, "debug");
    assert_eq!(got, format!("[RUSTC_WRAPPER={w}]"), "recorded verbatim");
    let o = build(
        &p,
        &["--release", "--features", "test-trust-root"],
        &[("RUSTFLAGS", "--cfg evil".into())],
    );
    assert!(
        o.status.success(),
        "a test-trust release build under flags builds: {}",
        text(&o)
    );
    assert!(
        reported(&p, "release").contains("CARGO_ENCODED_RUSTFLAGS=--cfg"),
        "the flags are recorded: {}",
        reported(&p, "release")
    );
}

/// The helper's own decisions, and the identity field the kit and the readiness
/// pin read.
#[test]
fn the_build_state_is_recorded_in_the_verifier_identity() {
    use axon_fabric::build_state::{refusal, state};
    let got = |pairs: &'static [(&'static str, &'static str)]| {
        state(&move |n| {
            pairs
                .iter()
                .find(|(k, _)| *k == n)
                .map(|(_, v)| v.to_string())
        })
    };
    assert_eq!(got(&[]), "");
    assert_eq!(got(&[("RUSTC_WRAPPER", "")]), "", "empty is none");
    assert_eq!(
        got(&[("RUSTC_LINKER", "/l"), ("RUSTC_WRAPPER", "/w")]),
        "RUSTC_WRAPPER=/w;RUSTC_LINKER=/l"
    );
    assert!(refusal("RUSTC_WRAPPER=/w", "release", true).is_none());
    assert!(refusal("RUSTC_WRAPPER=/w", "release", false).is_some());
    assert!(refusal("", "release", false).is_none());
    assert!(refusal("RUSTC_WRAPPER=/w", "debug", false).is_none());
    let id = axon_fabric::readiness::verifier_identity();
    assert!(
        id["build_state"].is_string(),
        "the identity names the build state: {id}"
    );
}
