//! Operator decision E at the FREEZE (amendment 44; C9 round 3, A79):
//! `scripts/v022_freeze_manifest.py` binds a candidate's evidence only from a
//! STANDALONE CLONE, and only a guest manifest that is clean with no reasons.
//! Its refusals had no test and no row (review EQUIVALENCE, round 3). These
//! run the real script, copied from THIS tree (so a mutation of it is what
//! runs), in a scratch repository.

#[path = "common/git_attacks.rs"]
mod git_attacks;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::Bins;

use serde_json::json;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GIT: &str = "/usr/bin/git";
/// The script and the modules it loads.
const COPIED: [&str; 4] = [
    "scripts/v022_freeze_manifest.py",
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
    "scripts/guest_build_env.py",
];
const MANIFEST: &str = "profiles/linux-microvm/manifest.json";

fn git(r: &Path, args: &[&str]) {
    let st = Command::new(GIT)
        .arg("-C")
        .arg(r)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .stdout(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

/// A guest manifest whose source block is `source`.
fn manifest(source: serde_json::Value) -> String {
    json!({"artifacts": {"vmlinux": {"sha256": "1".repeat(64)},
                         "axon": {"sha256": "a".repeat(64)},
                         "axon-guest-init": {"sha256": "b".repeat(64)},
                         "axon-psv-runner": {"sha256": "c".repeat(64)}},
           "source": source})
    .to_string()
}

/// The record scripts/guest_build_env.py writes for a controlled build of
/// exactly the fixture manifest's three binaries.
fn controlled_build() -> serde_json::Value {
    let tc = "/root/.rustup/toolchains/nightly-x86_64-unknown-linux-gnu/bin";
    let base = "/var/tmp/axon-guest-build-x";
    json!({
        "schema": "axon-guest-build-env/1",
        "controlled": true,
        "toolchain": {"channel": "nightly", "cargo": format!("{tc}/cargo"),
                      "cargo_sha256": "d".repeat(64), "cargo_version": "cargo 1",
                      "rustc": format!("{tc}/rustc"), "rustc_sha256": "e".repeat(64),
                      "rustc_vV": "rustc 1\nhost: x86_64-unknown-linux-gnu"},
        "env": {"CARGO_HOME": format!("{base}/cargo-home"),
                "CARGO_TARGET_DIR": format!("{base}/target"), "HOME": base,
                "LC_ALL": "C", "PATH": format!("{tc}:/usr/bin:/bin"),
                "RUSTC": format!("{tc}/rustc")},
        "env_allowlist": ["CARGO_HOME", "CARGO_TARGET_DIR", "HOME", "LC_ALL", "PATH", "RUSTC"],
        "proxy_vars": [],
        "cargo_home": format!("{base}/cargo-home"), "cargo_home_created_empty": true,
        "target_dir": format!("{base}/target"), "target_dir_created_empty": true,
        "effective_config": {"origins": [], "foreign": [], "own_config": ".cargo/config.toml"},
        "builds": [],
        "artifacts": {"axon": "a".repeat(64), "axon-guest-init": "b".repeat(64),
                      "axon-psv-runner": "c".repeat(64)},
    })
}

fn clean_source() -> serde_json::Value {
    json!({"axon_git_rev_at_build": "0".repeat(40), "axon_tree_dirty_at_build": false,
           "axon_tree_dirty_reasons": [], "build_environment": controlled_build()})
}

/// The files a freeze reads, under `root`.
fn populate(root: &Path) {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in COPIED {
        write(
            &root.join(f),
            &std::fs::read_to_string(src.join(f)).unwrap(),
        );
    }
    for f in [
        "governance/status/v022-psv-paired-disable.json",
        "governance/specs/v022-psv-protocol.md",
        "governance/specs/v022-psv-gap-map.md",
        "governance/specs/v022-psv-negative-matrix.md",
        "governance/specs/v022-protected-suite-verdict.md",
    ] {
        write(&root.join(f), "{}\n");
    }
    write(&root.join(MANIFEST), &manifest(clean_source()));
}

/// A committed standalone clone holding the freeze inputs.
fn clone(d: &Path) -> PathBuf {
    let r = d.join("repo");
    std::fs::create_dir_all(&r).unwrap();
    git(&r, &["init", "-q", "-b", "main"]);
    populate(&r);
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "candidate"]);
    r
}

/// The rustc-wrapper variables the freeze refuses (a development sccache
/// run sets one). Each test states its own environment rather than inherit
/// the caller's, so a refusal it does not judge cannot answer first.
const WRAPPERS: [&str; 4] = [
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER",
];

/// The freeze command from `root`, with no compiler wrapper in its env.
fn freeze_cmd(root: &Path) -> Command {
    // The freeze runs git, never a binary this workspace builds.
    let mut c = script_spawn::script(
        "python3",
        root.join("scripts/v022_freeze_manifest.py"),
        Bins::NoWorkspaceBinary,
    );
    c.arg("freeze.json")
        .arg(root.join("no-micode"))
        .current_dir(root)
        .env("V022_KEEP_TMPDIR", "1");
    for v in WRAPPERS {
        c.env_remove(v);
    }
    c
}

/// Run the freeze from `root`: Ok(stdout) or Err(stderr).
fn freeze(root: &Path) -> Result<String, String> {
    let o = freeze_cmd(root).output().unwrap();
    if o.status.success() {
        Ok(String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        Err(String::from_utf8_lossy(&o.stderr).to_string())
    }
}

fn refused(root: &Path, attack: &str, why: &str) {
    let got = freeze(root);
    assert!(
        got.is_err(),
        "ATTACK: the freeze bound evidence from {attack}: {got:?}"
    );
    let e = got.unwrap_err();
    assert!(e.contains(why), "expected {why:?}: {e}");
}

#[test]
fn a_standalone_clone_with_a_clean_guest_manifest_freezes() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let got = freeze(&r);
    assert!(got.is_ok(), "control: a standalone clone freezes: {got:?}");
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(r.join("freeze.json")).unwrap()).unwrap();
    assert_eq!(m["schema"], "axon-v022-psv-freeze/1");
}

/// A freeze is never made through a compiler wrapper (sccache is for
/// development runs only): each wrapper variable alone refuses it. Control:
/// the same clone with none set freezes.
#[test]
fn a_compiler_wrapper_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for v in WRAPPERS {
        let o = freeze_cmd(&r).env(v, "sccache").output().unwrap();
        let e = String::from_utf8_lossy(&o.stderr);
        assert!(
            !o.status.success(),
            "ATTACK: a freeze was made through a compiler wrapper ({v}=sccache)"
        );
        assert!(e.contains("compiler wrapper"), "{v}: {e}");
    }
    assert!(
        freeze(&r).is_ok(),
        "control: with no wrapper the clone freezes"
    );
}

#[test]
fn a_linked_worktree_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let wt = d.path().join("wt");
    git(
        &r,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    refused(
        &wt,
        "a linked worktree (a gitfile names the repository)",
        "not a standalone clone",
    );
}

/// A79: the linked worktree's admin directory copied in as a real `.git`
/// (`commondir` naming the clone's). `os.path.isdir` passed it; the
/// repository git acts on is another one.
#[test]
fn a_linked_worktree_disguised_as_a_git_directory_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let wt = d.path().join("wt");
    git_attacks::disguise_worktree(&r, &wt);
    refused(
        &wt,
        "a linked worktree whose admin dir was placed as .git",
        "a linked worktree's git dir",
    );
}

/// `.git` a symlink to a clone's git dir: the repository is chosen
/// elsewhere, whatever git then answers.
#[test]
fn a_symlinked_git_dir_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let s = d.path().join("linked");
    std::fs::create_dir_all(&s).unwrap();
    populate(&s);
    std::os::unix::fs::symlink(r.join(".git"), s.join(".git")).unwrap();
    refused(
        &s,
        "a tree whose .git is a symlink to another clone's",
        ".git is not a real directory",
    );
}

#[test]
fn a_dirty_guest_manifest_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let mut src = clean_source();
    src["axon_tree_dirty_at_build"] = json!(true);
    write(&r.join(MANIFEST), &manifest(src));
    refused(
        &r,
        "a guest image built from a dirty tree",
        "the guest manifest is not clean",
    );
}

#[test]
fn a_guest_manifest_with_dirty_reasons_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for reasons in [json!(["untracked file: build.rs"]), json!(null)] {
        let mut src = clean_source();
        src["axon_tree_dirty_reasons"] = reasons.clone();
        write(&r.join(MANIFEST), &manifest(src));
        refused(
            &r,
            &format!("a guest manifest marked clean with reasons {reasons}"),
            "the guest manifest is not clean",
        );
    }
}

/// The freeze binds `axon_sha` from the operator's git with the caller's
/// environment dropped: a GIT_DIR naming another repository does not choose
/// which HEAD is bound. Control: the clone's own HEAD.
#[test]
fn the_callers_git_environment_does_not_choose_the_bound_revision() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    let other = d.path().join("other");
    std::fs::create_dir_all(&other).unwrap();
    git(&other, &["init", "-q", "-b", "main"]);
    git(
        &other,
        &["commit", "-q", "--allow-empty", "-m", "elsewhere"],
    );
    let head = git_attacks::rev(&r, "HEAD");
    let o = freeze_cmd(&r)
        .env("GIT_DIR", other.join(".git"))
        .status()
        .unwrap();
    assert!(o.success(), "control: the clone freezes");
    let m: serde_json::Value =
        serde_json::from_slice(&std::fs::read(r.join("freeze.json")).unwrap()).unwrap();
    assert_eq!(
        m["axon_sha"], head,
        "ATTACK: the caller's GIT_DIR chose the revision the freeze bound"
    );
}

/// C9 round 4 (FIELD-ORIGIN, PSV-2): the freeze binds only a guest image whose
/// bytes were built in the controlled environment scripts/guest_build_env.py
/// constructs. A list of wrapper variables checked in the FREEZE's own
/// environment said nothing about the environment that BUILT the image: an
/// ancestor config, RUSTC, RUSTFLAGS, a linker or a reused target dir all
/// passed it. Each uncontrolled build below is refused; the control (the
/// controlled record) freezes (a_standalone_clone_with_a_clean_guest_manifest_freezes).
#[test]
fn a_guest_image_not_built_in_the_controlled_environment_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    type Edit = Box<dyn Fn(&mut serde_json::Value)>;
    let cases: Vec<(&str, Edit)> = vec![
        (
            "a caller's RUSTC_WRAPPER reached cargo",
            Box::new(|b| {
                b["env"]["RUSTC_WRAPPER"] = json!("/usr/bin/sccache");
            }),
        ),
        (
            "a RUSTC other than the pinned toolchain's",
            Box::new(|b| {
                b["env"]["RUSTC"] = json!("/tmp/evil/rustc");
            }),
        ),
        (
            "an ancestor .cargo/config.toml named a wrapper",
            Box::new(|b| {
                b["effective_config"]["foreign"] =
                    json!(["build.rustc-wrapper = \"/w\" (from /var/tmp/.cargo/config.toml)"]);
            }),
        ),
        (
            "a reused target dir",
            Box::new(|b| {
                b["target_dir_created_empty"] = json!(false);
            }),
        ),
        (
            "a CARGO_HOME with config in it",
            Box::new(|b| {
                b["cargo_home_created_empty"] = json!(false);
            }),
        ),
        (
            "PATH led by a directory other than the pinned toolchain's",
            Box::new(|b| {
                b["env"]["PATH"] = json!("/tmp/evil/bin:/usr/bin:/bin");
            }),
        ),
        // Last: with no record at all the artifact binding refuses too, so
        // the record gate's own row is judged by the cases above.
        (
            "no build environment recorded (a bare cargo build)",
            Box::new(|b| *b = json!(null)),
        ),
    ];
    for (attack, edit) in cases {
        let mut src = clean_source();
        edit(&mut src["build_environment"]);
        write(&r.join(MANIFEST), &manifest(src));
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound a guest image not built in the controlled environment \
             ({attack}): {got:?}"
        );
        let e = got.unwrap_err();
        assert!(e.contains("controlled build environment"), "{attack}: {e}");
    }
    write(&r.join(MANIFEST), &manifest(clean_source()));
    assert!(freeze(&r).is_ok(), "control: the controlled build freezes");
}

/// The controlled build's record is joined to the manifest: an artifact the
/// manifest pins must be the bytes that build produced. Control above.
#[test]
fn a_guest_artifact_the_controlled_build_did_not_produce_does_not_freeze() {
    let d = tempfile::tempdir().unwrap();
    let r = clone(d.path());
    for name in ["axon", "axon-guest-init", "axon-psv-runner"] {
        let mut src = clean_source();
        src["build_environment"]["artifacts"][name] = json!("f".repeat(64));
        write(&r.join(MANIFEST), &manifest(src));
        let got = freeze(&r);
        assert!(
            got.is_err(),
            "ATTACK: the freeze bound a guest {name} its controlled build did not produce: {got:?}"
        );
        assert!(got
            .unwrap_err()
            .contains("not the bytes its controlled build"));
    }
}
