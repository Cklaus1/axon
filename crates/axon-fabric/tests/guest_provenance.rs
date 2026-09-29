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
/// The PCI-certified revision as the real script spells it.
const PCI: &str = "PCI_CERTIFIED = \"31413ca7\"";

fn rev_of(r: &Path, what: &str) -> String {
    let o = Command::new(GIT)
        .arg("-C")
        .arg(r)
        .args(["rev-parse", what])
        .output()
        .unwrap();
    format!(
        "PCI_CERTIFIED = \"{}\"",
        String::from_utf8(o.stdout).unwrap().trim()
    )
}

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
    write(&repo.join(".gitignore"), "/dist/\n/target/\n");
    // The manifest binds the PCI lineage (HEAD descends from the certified
    // revision): here the certified revision is this repository's first
    // commit, substituted into the copied script.
    let mut pci = PCI.to_string();
    if init_git {
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "-A"]);
        git(&repo, &["commit", "-q", "-m", "PCI-certified base"]);
        pci = rev_of(&repo, "HEAD");
    }
    for f in COPIED {
        let text = std::fs::read_to_string(src.join(f)).unwrap();
        write(&repo.join(f), &text.replace(PCI, &pci));
    }
    write(&repo.join(INIT_SRC), "fn main() {}\n");
    if init_git {
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

impl Fx {
    /// `--descends REV`: did the lineage check pass?
    fn descends(&self, rev: &str, env: &[(&str, String)]) -> bool {
        let mut c = Command::new("python3");
        c.arg(self.repo.join("scripts/linux_profile_manifest.py"))
            .args(["--descends", rev])
            .current_dir(&self.repo)
            .stderr(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        c.status().unwrap().success()
    }
    fn rev(&self, what: &str) -> String {
        let o = Command::new(GIT)
            .arg("-C")
            .arg(&self.repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example"])
            .args(["rev-parse", what])
            .output()
            .unwrap();
        String::from_utf8(o.stdout).unwrap().trim().to_string()
    }
    /// HEAD becomes an orphan commit with the same tree: it descends from
    /// nothing. Returns the certified (old HEAD) revision.
    fn orphan_head(&self) -> String {
        let certified = self.rev("HEAD");
        let o = Command::new(GIT)
            .arg("-C")
            .arg(&self.repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example"])
            .args(["commit-tree", "HEAD^{tree}", "-m", "unrelated history"])
            .output()
            .unwrap();
        let orphan = String::from_utf8(o.stdout).unwrap().trim().to_string();
        git(&self.repo, &["update-ref", "refs/heads/main", &orphan]);
        certified
    }
}

#[test]
fn the_lineage_check_accepts_a_descendant() {
    let f = fixture(true);
    let certified = f.rev("HEAD");
    write(&f.repo.join(INIT_SRC), "fn main() { /* later */ }\n");
    git(&f.repo, &["commit", "-q", "-am", "later"]);
    assert!(f.descends(&certified, &[]), "a descendant descends");
    let certified = f.orphan_head();
    assert!(!f.descends(&certified, &[]), "an orphan does not");
}

#[test]
fn grafted_ancestry_does_not_pass_the_lineage_check() {
    let f = fixture(true);
    let certified = f.orphan_head();
    let orphan = f.rev("HEAD");
    write(
        &f.repo.join(".git/info/grafts"),
        &format!("{orphan} {certified}\n"),
    );
    assert!(
        !f.descends(&certified, &[]),
        "ATTACK: grafted ancestry passed the guest build's PCI lineage check"
    );
}

#[test]
fn a_git_on_the_callers_path_does_not_answer_the_lineage_check() {
    let f = fixture(true);
    let certified = f.orphan_head();
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
    assert!(
        !f.descends(&certified, &path),
        "ATTACK: a git on the caller's PATH answered the guest build's PCI lineage check"
    );
}

/// A linked worktree (a `.git` FILE): its lineage is still answered (history
/// does not depend on which clone asks), but it is never described as a
/// clean build tree — a verifier or guest image is built from a plain clone.
#[test]
fn a_linked_worktree_passes_the_lineage_check_but_is_never_clean() {
    let f = fixture(true);
    let head = f.rev("HEAD");
    let wt = f.d.path().join("wt");
    git(&f.repo, &["worktree", "add", "-q", wt.to_str().unwrap()]);
    let linked = Fx {
        d: tempfile::tempdir().unwrap(),
        repo: wt.clone(),
    };
    assert!(
        linked.descends(&head, &[]),
        "control: lineage from a worktree"
    );
    let snap = f.d.path().join("wt-pre.json");
    linked.script(&["--snapshot", snap.to_str().unwrap()], &[]);
    let p: Value = serde_json::from_slice(&std::fs::read(&snap).unwrap()).unwrap();
    assert!(
        p["dirty"].as_array().is_some_and(|d| d
            .iter()
            .any(|r| r.as_str().is_some_and(|r| r.contains("gitfile")))),
        "ATTACK: a gitfile naming a repository elsewhere was accepted as the build's tree: {p}"
    );
}

/// Fabric's qualification also requires the B263 evidence record's
/// `source.tree_dirty` to be false. It came from PATH git with untracked
/// files excluded, and a git that failed read as clean (`bool(None)`). It is
/// the same provenance now; b263_qualify.sh needs KVM to run end to end, so
/// this checks its wiring.
#[test]
fn the_b263_evidence_tree_dirty_is_the_rust_provenance() {
    let s = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../scripts/b263_qualify.sh"),
    )
    .unwrap();
    assert!(
        s.contains("prov = lpm.provenance()")
            && s.contains(r#""tree_dirty": bool(prov["dirty"]) or prov["revision"] == "unknown","#)
            && !s.contains(r#"["git", "status""#),
        "ATTACK: the B263 evidence's tree_dirty is not the hardened provenance (PATH git, \
         untracked files excluded, cannot-tell read as clean)"
    );
}

// ── Operator decision C (amendment 44, A70): git-ignore excuses nothing in
// the guest manifest either. Only the operator's allowlist
// (/etc/axon/provenance-allowlist) could, and none is installed here.

#[test]
fn a_gitignored_build_input_is_dirty() {
    let f = fixture(true);
    // `/target/` is ignored by the reviewed .gitignore; cargo reads a
    // `target/`-free build input as readily from any ignored path, e.g. a
    // crate-local `.cargo/config.toml` the tree's own rule hides.
    write(&f.repo.join(".gitignore"), "/dist/\n/target/\n.cargo/\n");
    git(&f.repo, &["commit", "-q", "-am", "ignore rules"]);
    write(
        &f.repo.join("crates/axon-guest-init/.cargo/config.toml"),
        "[build]\nrustflags = [\"--cfg\", \"feature=\\\"dev-allow-no-policy\\\"\"]\n",
    );
    assert_dirty(
        &f.build(),
        "a .gitignored .cargo/config.toml turned on the guest's no-policy bypass",
    );
}

#[test]
fn an_info_exclude_hidden_build_script_is_dirty() {
    let f = fixture(true);
    write(&f.repo.join(".git/info/exclude"), "build.rs\n");
    write(
        &f.repo.join("crates/axon-guest-init/build.rs"),
        "fn main() { println!(\"cargo:rustc-cfg=feature=\\\"dev-allow-no-policy\\\"\"); }\n",
    );
    assert_dirty(
        &f.build(),
        "info/exclude hid an untracked guest-init build.rs",
    );
}

// ── Operator decision E (amendment 44, A71): the manifest binds the PCI
// lineage, asked the protected way. The build's early `--descends` check is
// a development check and makes nothing clean.

#[test]
fn a_head_that_does_not_descend_from_the_pci_certification_is_dirty() {
    let f = fixture(true);
    // HEAD becomes an orphan with the same tree: the certified base is not
    // its ancestor, whatever the shell's early check did or did not run.
    let o = Command::new(GIT)
        .arg("-C")
        .arg(&f.repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(["commit-tree", "HEAD^{tree}", "-m", "unrelated history"])
        .output()
        .unwrap();
    let orphan = String::from_utf8(o.stdout).unwrap().trim().to_string();
    git(&f.repo, &["update-ref", "refs/heads/main", &orphan]);
    let src = f.build();
    assert_dirty(
        &src,
        "a HEAD that does not descend from the PCI-certified revision",
    );
    assert!(src.to_string().contains("PCI lineage"), "{src}");
}
