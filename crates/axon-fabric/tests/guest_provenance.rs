//! The guest-image manifest's `source.axon_tree_dirty_at_build` gates
//! Fabric's qualification (RULE:manifest-clean), and psv::prepare takes the
//! guest digests from the same manifest. It used to come from PATH `git`
//! under the caller's environment with untracked files excluded, and a git
//! that could not answer read as CLEAN (review FIELD-ORIGIN, C9 round 2).
//!
//! These run the real `scripts/linux_profile_manifest.py` (and the real
//! provenance sources it compiles) in a scratch repository, the way
//! `build-guest-image.sh` does: `--snapshot` before the build, then the
//! manifest with `--pre`.

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GIT: &str = "/usr/bin/git";
/// The files the manifest step runs, copied from THIS tree (so a mutation
/// of any of them is what runs).
const COPIED: [&str; 4] = [
    "scripts/linux_profile_manifest.py",
    "crates/axon-fabric/src/git_data.rs",
    "crates/axon-fabric/src/provenance.rs",
    "crates/axon-fabric/src/bin/axon-provenance.rs",
];
const INIT_SRC: &str = "crates/axon-guest-init/src/main.rs";

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

struct Fx {
    d: tempfile::TempDir,
    repo: PathBuf,
}

/// A repository holding the manifest step and a guest-init source, plus a
/// profile dir, dummy artifacts and a dummy VMM outside it.
fn fixture(init_git: bool) -> Fx {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    for f in COPIED {
        write(
            &repo.join(f),
            &std::fs::read_to_string(src.join(f)).unwrap(),
        );
    }
    write(&repo.join(".gitignore"), "/dist/\n/target/\n");
    write(&repo.join(INIT_SRC), "fn main() {}\n");
    if init_git {
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "reviewed"]);
    }
    let pin = "KERNEL_VERSION=1\nKERNEL_URL=u\nKERNEL_TARBALL_SHA256=a\nKERNEL_CONFIG=c\n\
               KERNEL_CONFIG_SHA256=a\nKERNEL_OVERLAY=o\nKERNEL_OVERLAY_SHA256=a\n\
               BUSYBOX_PKG=b\nBUSYBOX_SHA256=a\n";
    write(&d.path().join("prof/kernel.pin"), pin);
    write(&d.path().join("prof/guest-init.sh"), "#!/bin/sh\n");
    for a in [
        "effective.config",
        "vmlinux",
        "rootfs.sqfs",
        "axon",
        "axon-guest-init",
        "axon-psv-runner",
    ] {
        write(&d.path().join("dist").join(a), a);
    }
    for b in ["fc", "jl"] {
        let p = d.path().join(b);
        write(&p, "#!/bin/sh\necho v1\n");
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    Fx { d, repo }
}

impl Fx {
    fn script(&self, args: &[&str], env: &[(&str, String)]) {
        let mut c = Command::new("python3");
        c.arg(self.repo.join("scripts/linux_profile_manifest.py"))
            .args(args)
            .current_dir(&self.repo)
            .env("FC_BIN", self.d.path().join("fc"))
            .env("JAILER_BIN", self.d.path().join("jl"))
            .stdout(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        assert!(c.status().unwrap().success(), "manifest step {args:?}");
    }
    fn pre(&self) -> String {
        self.d.path().join("pre.json").display().to_string()
    }
    fn snapshot(&self, env: &[(&str, String)]) {
        self.script(&["--snapshot", &self.pre()], env);
    }
    /// The manifest's `source` block, written with the snapshot (if any).
    fn manifest(&self, pre: bool, env: &[(&str, String)]) -> Value {
        let (dist, prof) = (self.d.path().join("dist"), self.d.path().join("prof"));
        let (dist, prof) = (dist.to_str().unwrap(), prof.to_str().unwrap());
        let p = self.pre();
        let args: Vec<&str> = if pre {
            vec!["--pre", &p, dist, prof]
        } else {
            vec![dist, prof]
        };
        self.script(&args, env);
        let m: Value = serde_json::from_slice(
            &std::fs::read(self.d.path().join("dist/manifest.json")).unwrap(),
        )
        .unwrap();
        m["source"].clone()
    }
    fn build(&self) -> Value {
        self.snapshot(&[]);
        self.manifest(true, &[])
    }
}

fn assert_dirty(src: &Value, attack: &str) {
    assert_eq!(
        src["axon_tree_dirty_at_build"],
        Value::Bool(true),
        "ATTACK: {attack}, and the guest manifest says axon_tree_dirty_at_build: false: {src}"
    );
}

#[test]
fn a_clean_committed_tree_is_clean_and_names_its_head() {
    let f = fixture(true);
    let src = f.build();
    assert_eq!(src["axon_tree_dirty_at_build"], false, "{src}");
    assert_eq!(src["axon_tree_dirty_reasons"], serde_json::json!([]));
    let head = Command::new(GIT)
        .arg("-C")
        .arg(&f.repo)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(
        src["axon_git_rev_at_build"],
        String::from_utf8_lossy(&head.stdout).trim()
    );
}

#[test]
fn a_tree_git_cannot_describe_is_dirty() {
    let f = fixture(false);
    assert_dirty(&f.build(), "no git repository at all (\"cannot tell\")");
}

#[test]
fn a_skip_worktree_edit_is_dirty() {
    let f = fixture(true);
    write(&f.repo.join(INIT_SRC), "fn main() { /* agent */ }\n");
    git(&f.repo, &["update-index", "--skip-worktree", INIT_SRC]);
    assert_dirty(
        &f.build(),
        "a skip-worktree entry hid a modified guest-init source",
    );
}

#[test]
fn an_untracked_build_script_is_dirty() {
    let f = fixture(true);
    write(
        &f.repo.join("crates/axon-guest-init/build.rs"),
        "fn main() { println!(\"cargo:rustc-cfg=feature=\\\"dev-allow-no-policy\\\"\"); }\n",
    );
    assert_dirty(&f.build(), "an untracked build.rs changes the guest build");
}

#[test]
fn repository_git_config_does_not_make_an_edit_clean() {
    let f = fixture(true);
    write(&f.repo.join(INIT_SRC), "fn main() { /* agent */ }\n");
    let mirror = f.repo.join("dist/mirror");
    write(&mirror.join(INIT_SRC), "fn main() {}\n");
    git(
        &f.repo,
        &["config", "core.worktree", mirror.to_str().unwrap()],
    );
    assert_dirty(&f.build(), "core.worktree pointed git at a clean mirror");
}

#[test]
fn a_git_on_the_callers_path_is_not_asked() {
    let f = fixture(true);
    write(&f.repo.join(INIT_SRC), "fn main() { /* agent */ }\n");
    let bin = f.d.path().join("fakebin");
    write(&bin.join("git"), "#!/bin/sh\nexit 0\n");
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = vec![(
        "PATH",
        format!(
            "{}:{}",
            bin.display(),
            std::env::var("PATH").unwrap_or_default()
        ),
    )];
    f.snapshot(&path);
    assert_dirty(
        &f.manifest(true, &path),
        "a git on the caller's PATH answered for the tree",
    );
}

#[test]
fn a_provenance_helper_that_cannot_run_is_dirty() {
    let f = fixture(true);
    let env = vec![("RUSTC", "/bin/false".to_string())];
    f.snapshot(&env);
    assert_dirty(
        &f.manifest(true, &env),
        "the provenance helper could not be built (\"cannot tell\")",
    );
}

#[test]
fn a_manifest_without_a_pre_build_snapshot_is_dirty() {
    let f = fixture(true);
    assert_dirty(
        &f.manifest(false, &[]),
        "no snapshot of the tree the artifacts were built from",
    );
}

#[test]
fn a_tree_that_moved_during_the_build_is_dirty() {
    let f = fixture(true);
    f.snapshot(&[]);
    write(
        &f.repo.join(INIT_SRC),
        "fn main() { /* built from this */ }\n",
    );
    git(&f.repo, &["commit", "-q", "-am", "moved"]);
    assert_dirty(
        &f.manifest(true, &[]),
        "the tree moved to another commit between the snapshot and the manifest",
    );
}

#[test]
fn a_tree_dirty_when_the_build_started_is_dirty() {
    let f = fixture(true);
    let extra = f.repo.join("crates/axon-guest-init/build.rs");
    write(&extra, "fn main() {}\n");
    f.snapshot(&[]);
    std::fs::remove_file(&extra).unwrap();
    assert_dirty(
        &f.manifest(true, &[]),
        "the tree was dirty when the build started and clean again by manifest time",
    );
}

#[test]
fn an_edit_during_the_build_is_dirty() {
    let f = fixture(true);
    f.snapshot(&[]);
    write(
        &f.repo.join("crates/axon-guest-init/build.rs"),
        "fn main() {}\n",
    );
    assert_dirty(
        &f.manifest(true, &[]),
        "an untracked build.rs appeared after the snapshot, before the manifest",
    );
}
