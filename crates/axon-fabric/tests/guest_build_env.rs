//! The protected guest image is built in an environment the build CONSTRUCTS
//! (scripts/guest_build_env.py), not one it filters by a list of names
//! (C9 round 4, FIELD-ORIGIN / PSV-2 / EQUIVALENCE; two reviewers built
//! through an ancestor `.cargo/config.toml`, a dotted `build.rustc-wrapper`,
//! RUSTC/RUSTFLAGS and a reused target dir, and the old guard passed each).
//!
//! These run the PRODUCTION route: `scripts/build-guest-image.sh
//! --build-env-only`, the same `gcargo_begin` every guest build starts with,
//! and the recorded `guest_build_env.py cargo` step, from a scratch checkout of
//! this tree's files.

#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::{repo_root, Bins};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Output;

/// The files the controlled-build step reads, copied from THIS tree.
const COPIED: [&str; 4] = [
    "scripts/build-guest-image.sh",
    "scripts/guest_build_env.py",
    "rust-toolchain.toml",
    ".cargo/config.toml",
];

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

fn chmod_x(p: &Path) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// A checkout at `<parent>/repo` holding the build's files and a one-crate
/// workspace (no dependencies, so a fresh CARGO_HOME builds it offline).
fn checkout(parent: &Path) -> PathBuf {
    let r = parent.join("repo");
    for f in COPIED {
        write(
            &r.join(f),
            &std::fs::read_to_string(repo_root().join(f)).unwrap(),
        );
    }
    write(
        &r.join("Cargo.toml"),
        "[workspace]\nmembers = [\"tiny\"]\nresolver = \"2\"\n",
    );
    write(
        &r.join("tiny/Cargo.toml"),
        "[package]\nname = \"tiny\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&r.join("tiny/src/main.rs"), "fn main() {}\n");
    r
}

/// `build-guest-image.sh --build-env-only <out>`: the controlled environment
/// alone, recorded in `<out>/build-env.json`.
fn build_env_only(repo: &Path, out: &Path, env: &[(&str, String)]) -> Output {
    let mut c = script_spawn::script(
        "bash",
        repo.join("scripts/build-guest-image.sh"),
        Bins::BuildsItsOwn,
    );
    c.arg("--build-env-only").arg(out).current_dir(repo);
    for (k, v) in env {
        c.env(k, v);
    }
    c.output().unwrap()
}

fn record(out: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(out.join("build-env.json")).unwrap()).unwrap()
}

/// Remove the fresh CARGO_HOME / target dir a successful begin created.
fn discard(rec: &Value) {
    if let Some(t) = rec["target_dir"].as_str() {
        let base = Path::new(t).parent().unwrap();
        if base
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with("axon-guest-build-"))
        {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// cargo's EFFECTIVE configuration, as it resolves it from the build's
/// directory, may hold nothing the guest build would use -- from an ancestor
/// directory's config, in the dotted-key spelling, or committed to the tree's
/// own config. Control: the tree's own config (wasm-only rustflags) passes.
#[test]
fn a_cargo_config_setting_the_guest_build_would_use_is_refused() {
    let cases: [(&str, &str, bool); 5] = [
        (
            "an ancestor .cargo/config.toml naming a rustc wrapper",
            "[build]\nrustc-wrapper = \"/usr/bin/sccache\"\n",
            false,
        ),
        (
            "an ancestor config in the dotted-key spelling",
            "build.rustc-wrapper = \"/usr/bin/sccache\"\n",
            false,
        ),
        (
            "an ancestor config naming a linker for the guest's target",
            "[target.x86_64-unknown-linux-musl]\nlinker = \"/tmp/evil-cc\"\n",
            false,
        ),
        (
            "an ancestor config adding rustflags",
            "[build]\nrustflags = [\"--cfg\", \"evil\"]\n",
            false,
        ),
        (
            "the tree's own config naming a rustc",
            "build.rustc = \"/tmp/evil-rustc\"\n",
            true,
        ),
    ];
    for (attack, cfg, own) in cases {
        let d = tempfile::tempdir().unwrap();
        let r = checkout(d.path());
        if own {
            let p = r.join(".cargo/config.toml");
            // At the top, so the dotted key is not inside the file's tables.
            let s = std::fs::read_to_string(&p).unwrap();
            write(&p, &format!("{cfg}{s}"));
        } else {
            write(&d.path().join(".cargo/config.toml"), cfg);
        }
        let out = d.path().join("out");
        let o = build_env_only(&r, &out, &[]);
        if o.status.success() {
            discard(&record(&out));
        }
        assert!(
            !o.status.success(),
            "ATTACK: the guest build ran under a cargo config setting it would use ({attack}):\n{}",
            text(&o)
        );
        assert!(
            text(&o).contains("effective configuration"),
            "{attack}: {}",
            text(&o)
        );
    }
    // Control: the tree's own config alone.
    let d = tempfile::tempdir().unwrap();
    let r = checkout(d.path());
    let out = d.path().join("out");
    let o = build_env_only(&r, &out, &[]);
    assert!(
        o.status.success(),
        "control: the tree's own config builds: {}",
        text(&o)
    );
    let rec = record(&out);
    discard(&rec);
    assert_eq!(rec["effective_config"]["foreign"], serde_json::json!([]));
}

/// Cargo runs in the environment the build constructs, never the caller's:
/// a RUSTC_WRAPPER (and friends) exported by whoever launched the build does
/// not reach the compile. Control: the step builds, into the fresh target dir.
#[test]
fn a_callers_compiler_wrapper_does_not_reach_the_guest_builds_cargo() {
    let d = tempfile::tempdir().unwrap();
    let r = checkout(d.path());
    let out = d.path().join("out");
    let o = build_env_only(&r, &out, &[]);
    assert!(o.status.success(), "setup: {}", text(&o));
    let marker = d.path().join("wrapper-ran");
    let wrapper = d.path().join("wrapper");
    write(
        &wrapper,
        &format!(
            "#!/bin/sh\n: > '{}'\nshift\nexec \"$@\"\n",
            marker.display()
        ),
    );
    chmod_x(&wrapper);
    let rec_path = out.join("build-env.json");
    let mut c = script_spawn::script(
        "python3",
        r.join("scripts/guest_build_env.py"),
        Bins::BuildsItsOwn,
    );
    c.arg("cargo")
        .arg(&rec_path)
        .args(["--", "build", "--offline", "-p", "tiny"])
        .current_dir(&r);
    for v in [
        "RUSTC_WRAPPER",
        "RUSTC_WORKSPACE_WRAPPER",
        "CARGO_BUILD_RUSTC_WRAPPER",
    ] {
        c.env(v, &wrapper);
    }
    c.env("RUSTC", "/bin/false")
        .env("RUSTFLAGS", "--cfg evil")
        .env("CARGO_BUILD_RUSTC", "/bin/false");
    let b = c.output().unwrap();
    let rec = record(&out);
    let built = Path::new(rec["target_dir"].as_str().unwrap()).join("debug/tiny");
    let (ran, exists) = (marker.exists(), built.exists());
    discard(&rec);
    assert!(
        !ran,
        "ATTACK: a caller's RUSTC_WRAPPER reached the guest build's cargo:\n{}",
        text(&b)
    );
    assert!(
        b.status.success() && exists,
        "control: the controlled step builds, into the fresh target dir:\n{}",
        text(&b)
    );
}

/// The toolchain is the one rust-toolchain.toml pins, as rustup resolves it
/// under a CLEARED environment: a caller's RUSTUP_HOME holding a toolchain of
/// the same name does not choose the compiler. Control: the recorded rustc
/// is the invoking user's real toolchain.
#[test]
fn a_callers_rustup_home_does_not_choose_the_guest_toolchain() {
    let d = tempfile::tempdir().unwrap();
    let r = checkout(d.path());
    let chan = std::fs::read_to_string(r.join("rust-toolchain.toml"))
        .unwrap()
        .lines()
        .find_map(|l| {
            l.trim().strip_prefix("channel").map(|v| {
                v.split('=')
                    .nth(1)
                    .unwrap()
                    .trim()
                    .trim_matches('"')
                    .to_string()
            })
        })
        .unwrap();
    let fake_home = d.path().join("fake-rustup");
    let bin = fake_home.join(format!("toolchains/{chan}-x86_64-unknown-linux-gnu/bin"));
    for tool in ["cargo", "rustc"] {
        write(
            &bin.join(tool),
            "#!/bin/sh\ncase \"$*\" in *config*) exit 0;; esac\necho \"fake 1.0 (planted)\"\n",
        );
        chmod_x(&bin.join(tool));
    }
    let out = d.path().join("out");
    let o = build_env_only(
        &r,
        &out,
        &[
            ("RUSTUP_HOME", fake_home.display().to_string()),
            ("RUSTUP_TOOLCHAIN", chan.clone()),
        ],
    );
    let rec = if o.status.success() {
        record(&out)
    } else {
        Value::Null
    };
    discard(&rec);
    let rustc = rec["toolchain"]["rustc"].as_str().unwrap_or("");
    assert!(
        !rustc.starts_with(fake_home.to_str().unwrap()),
        "ATTACK: a caller's RUSTUP_HOME chose the guest build's compiler ({rustc})"
    );
    assert!(
        o.status.success() && Path::new(rustc).is_file(),
        "control: the pinned toolchain resolves: {}",
        text(&o)
    );
}
