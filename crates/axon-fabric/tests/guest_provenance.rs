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

#[path = "common/exec.rs"]
mod exec;
use exec::write_executable;
#[path = "common/git_attacks.rs"]
mod git_attacks;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;
use script_spawn::Bins;
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const GIT: &str = "/usr/bin/git";
/// The files the manifest step runs, copied from THIS tree (so a mutation
/// of any of them is what runs).
const COPIED: [&str; 6] = [
    "scripts/linux_profile_manifest.py",
    "scripts/guest_build_env.py",
    "rust-toolchain.toml",
    "crates/axon-fabric/src/git_data.rs",
    "crates/axon-fabric/src/provenance.rs",
    "crates/axon-fabric/src/bin/axon-provenance.rs",
];
const INIT_SRC: &str = "crates/axon-guest-init/src/main.rs";
/// The PCI-certified revision as the real script spells it.
const PCI: &str = "PCI_CERTIFIED = \"31413ca7abb6ff730e1b63718d4304c7a8402675\"";

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
        write_executable(&d.path().join(b), "#!/bin/sh\necho v1\n", 0o755);
    }
    Fx { d, repo }
}

impl Fx {
    fn script(&self, args: &[&str], env: &[(&str, String)]) {
        // The manifest step compiles its own provenance helper and runs that.
        let mut c = script_spawn::script(
            "python3",
            self.repo.join("scripts/linux_profile_manifest.py"),
            Bins::BuildsItsOwn,
        );
        c.args(args)
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
    std::fs::create_dir_all(&bin).unwrap();
    write_executable(&bin.join("git"), "#!/bin/sh\nexit 0\n", 0o755);
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
    // The helper's source no longer compiles (committed, so nothing else in
    // the tree is dirty): the answer is "cannot tell", which is dirty.
    write(
        &f.repo.join("crates/axon-fabric/src/bin/axon-provenance.rs"),
        "fn main() { this does not compile }\n",
    );
    git(
        &f.repo,
        &["commit", "-q", "-am", "a helper that cannot build"],
    );
    f.snapshot(&[]);
    assert_dirty(
        &f.manifest(true, &[]),
        "the provenance helper could not be built (\"cannot tell\")",
    );
}

/// The helper that DECIDES whether the tree is clean is compiled by the
/// pinned toolchain's own rustc (scripts/guest_build_env.py's resolution),
/// never `$RUSTC`: a caller's RUSTC could compile a helper that always says
/// clean (C9 round 4, FIELD-ORIGIN). Here RUSTC names a "compiler" whose
/// output reports a clean tree for this dirty one.
#[test]
fn a_callers_rustc_does_not_build_the_provenance_helper() {
    let f = fixture(true);
    write(&f.repo.join(INIT_SRC), "fn main() { /* uncommitted */ }\n");
    let head = f.rev("HEAD");
    // The helper that "compiler" emits: a clean answer, whatever the tree.
    let lying = f.d.path().join("lying-helper");
    write_executable(
        &lying,
        format!(
            "#!/bin/sh\nL=\"\"\nif [ \"$1\" = --lineage ]; then \
             L=\",\\\"lineage\\\":{{\\\"rev\\\":\\\"$2\\\",\\\"descends\\\":true}}\"; fi\n\
             echo \"{{\\\"schema\\\":\\\"axon-provenance/1\\\",\\\"revision\\\":\\\"{head}\\\",\\\"dirty\\\":[]$L}}\"\n"
        ),
        0o755,
    );
    let fake = f.d.path().join("fake-rustc");
    let used = f.d.path().join("fake-rustc-used");
    write_executable(
        &fake,
        format!(
            "#!/bin/sh\n: > '{}'\nwhile [ \"$1\" != -o ]; do shift; done\ncp '{}' \"$2\"\nchmod 0755 \"$2\"\n",
            used.display(),
            lying.display()
        ),
        0o755,
    );
    let env = vec![("RUSTC", fake.display().to_string())];
    f.snapshot(&env);
    let src = f.manifest(true, &env);
    assert!(
        src["axon_tree_dirty_at_build"] == Value::Bool(true) && !used.exists(),
        "ATTACK: a caller's RUSTC built the provenance helper and it called a dirty tree \
         clean (rustc used: {}): {src}",
        used.exists()
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
        // The manifest step compiles its own provenance helper and runs that.
        let mut c = script_spawn::script(
            "python3",
            self.repo.join("scripts/linux_profile_manifest.py"),
            Bins::BuildsItsOwn,
        );
        c.args(["--descends", rev])
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
    std::fs::create_dir_all(&bin).unwrap();
    write_executable(&bin.join("git"), "#!/bin/sh\nexit 0\n", 0o755);
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

/// The DEVELOPMENT lineage answer (`axon-provenance --descends`, which the
/// guest build's early check wraps, and `provenance::descends_from` for a
/// library consumer) runs under its CALLER's environment. It accepts a linked
/// worktree (history does not depend on which clone asks), so decision E's
/// repository-identity rule (the common dir must be `top/.git`), which
/// refuses a GIT_DIR on every protected path, is not on this one: git_cmd's
/// cleared environment is the only thing that keeps a GIT_DIR naming another
/// repository from answering (C9 round 3, rows; M284).
#[test]
fn the_callers_git_dir_does_not_answer_the_development_lineage_check() {
    let f = fixture(true);
    // A clone whose HEAD IS the certified revision, taken before the
    // repository's history is replaced by an orphan.
    let honest = f.d.path().join("honest");
    let st = Command::new(GIT)
        .args(["clone", "-q"])
        .arg(&f.repo)
        .arg(&honest)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "setup: clone");
    let certified = f.orphan_head();
    let ask = |env: &[(&str, &Path)]| {
        let mut c = Command::new(env!("CARGO_BIN_EXE_axon-provenance"));
        c.args(["--descends", &certified])
            .arg(&f.repo)
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        for (k, v) in env {
            c.env(k, v);
        }
        c.status().unwrap().success()
    };
    assert!(!ask(&[]), "control: the orphan does not descend");
    let git_dir = honest.join(".git");
    assert!(
        !ask(&[("GIT_DIR", &git_dir)]),
        "ATTACK: the caller's GIT_DIR named another repository and it answered the development \
         lineage check (HEAD descends from {certified})"
    );
}

/// The guest build names the PCI-certified revision by an ABBREVIATION
/// (`PCI_CERTIFIED = "31413ca7"`). git resolves a name to a REF before an
/// abbreviated hash, so a branch the repository holds, named like that
/// abbreviation and pointing at HEAD, made an orphan HEAD "descend" from it,
/// on the protected answer the manifest binds (`--lineage`) and the build's
/// early check (`--descends`) alike (C9 round 3, rows; M642). A revision is
/// named by its hash only. Control: the abbreviation of a real ancestor
/// descends.
#[test]
fn a_branch_named_like_the_certified_abbreviation_does_not_answer_the_lineage() {
    let f = fixture(true);
    let certified = f.rev("HEAD");
    let short = certified[..8].to_string();
    let ask = |args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_axon-provenance"))
            .args(args)
            .arg(&f.repo)
            .stderr(Stdio::null())
            .output()
            .unwrap();
        let lin = serde_json::from_slice::<Value>(&o.stdout)
            .ok()
            .map(|v| v["lineage"]["descends"].clone());
        (o.status.success(), lin)
    };
    // The protected answer takes only a full id (A93); the development check
    // still resolves an abbreviation, by hash.
    let control = (
        ask(&["--descends", &short]),
        ask(&["--lineage", &certified]),
    );
    assert_eq!(
        (control.0 .0, control.1 .1.clone()),
        (true, Some(Value::Bool(true))),
        "control: the certified abbreviation of HEAD itself descends"
    );
    f.orphan_head();
    git(&f.repo, &["branch", &short, "HEAD"]);
    let (dev, protected) = (ask(&["--descends", &short]), ask(&["--lineage", &short]));
    assert!(
        !dev.0 && protected.1 == Some(Value::Bool(false)),
        "ATTACK: a branch named like the certified revision's abbreviation made an orphan HEAD \
         descend from it (--descends exit ok: {}, --lineage descends: {:?})",
        dev.0,
        protected.1
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
        p["dirty"].as_array().is_some_and(|d| d.iter().any(|r| r
            .as_str()
            .is_some_and(|r| r.contains("gitfile") || r.contains("linked worktree")))),
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

/// Decision E by what git acts on (review PSV-7 / FIELD-ORIGIN, C9 round 3;
/// A79): the guest helper gave a linked worktree whose admin directory was
/// copied in as a real `.git` a clean, PCI-descending answer. Control: the
/// clone it came from is clean.
#[test]
fn a_linked_worktree_disguised_as_a_git_directory_is_never_clean() {
    let f = fixture(true);
    let wt = f.d.path().join("wt");
    git_attacks::disguise_worktree(&f.repo, &wt);
    let snap = |repo: &Path, name: &str| -> Value {
        let p = f.d.path().join(name);
        Fx {
            d: tempfile::tempdir().unwrap(),
            repo: repo.to_path_buf(),
        }
        .script(&["--snapshot", p.to_str().unwrap()], &[]);
        serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap()
    };
    let control = snap(&f.repo, "clone-pre.json");
    assert_eq!(
        control["dirty"],
        serde_json::json!([]),
        "control: {control}"
    );
    let p = snap(&wt, "wt-pre.json");
    assert!(
        p["dirty"].as_array().is_some_and(|d| d
            .iter()
            .any(|r| r.as_str().is_some_and(|r| r.contains("linked worktree")))),
        "ATTACK: a linked worktree's admin dir placed as .git was accepted as the build's tree: {p}"
    );
}

/// A80: the guest manifest's PCI lineage is read from hash-checked objects.
/// HEAD becomes D, whose parent C is an orphan (so HEAD does NOT descend from
/// the PCI-certified base); C's loose object is then rewritten under its own
/// name to name the base as its parent. `git merge-base --is-ancestor`
/// believed it and the manifest read clean. Control: before the forgery the
/// manifest is dirty for lineage.
#[test]
fn a_forged_ancestor_object_does_not_pass_the_pci_lineage() {
    let f = fixture(true);
    let base = f.rev("HEAD~1");
    let c = git_attacks::commit_tree(&f.repo, &[], "C");
    let d = git_attacks::commit_tree(&f.repo, &[&c], "D");
    git(&f.repo, &["update-ref", "refs/heads/main", &d]);
    let src = f.build();
    assert!(
        src.to_string().contains("PCI lineage"),
        "control: an unforged non-descendant is dirty for lineage: {src}"
    );
    git_attacks::forge_parent(&f.repo, &c, &base);
    assert_dirty(
        &f.build(),
        "a forged ancestor object made HEAD descend from the PCI-certified revision",
    );
}

// ── C9 round 4b, rows4b (amendment 62): how the certified revision is
// NAMED. `descends` peels a name to the commit it names; each refusal on the
// way had no row. Asked through the built `axon-provenance`, on the
// development check (`--descends`) and the protected answer the manifest
// binds (`--lineage`).

/// (`--descends` exit ok, `--lineage` descends) for `rev` in `dir`.
fn lineage_of(dir: &Path, rev: &str) -> (bool, Option<Value>) {
    let ask = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_axon-provenance"))
            .args(args)
            .arg(dir)
            .stderr(Stdio::null())
            .output()
            .unwrap()
    };
    let dev = ask(&["--descends", rev]).status.success();
    let o = ask(&["--lineage", rev]);
    let lin = serde_json::from_slice::<Value>(&o.stdout)
        .ok()
        .map(|v| v["lineage"]["descends"].clone());
    (dev, lin)
}

fn git_out(r: &Path, args: &[&str], stdin: Option<&[u8]>) -> String {
    use std::io::Write;
    let mut c = Command::new(GIT)
        .arg("-C")
        .arg(r)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    c.stdin
        .take()
        .unwrap()
        .write_all(stdin.unwrap_or(b""))
        .unwrap();
    let o = c.wait_with_output().unwrap();
    assert!(o.status.success(), "git {args:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

fn assert_no_lineage(got: (bool, Option<Value>), what: &str) {
    assert!(
        !got.0 && got.1 == Some(Value::Bool(false)),
        "ATTACK: {what} (--descends exit ok: {}, --lineage descends: {:?})",
        got.0,
        got.1
    );
}

/// A certified "revision" that names a BLOB is no commit, and no history
/// descends from it. Control: HEAD's own id descends.
#[test]
fn a_certified_revision_naming_a_blob_is_no_lineage() {
    let f = fixture(true);
    let head = f.rev("HEAD");
    assert_eq!(
        lineage_of(&f.repo, &head),
        (true, Some(Value::Bool(true))),
        "control"
    );
    let blob = git_out(
        &f.repo,
        &["hash-object", "-w", "--stdin"],
        Some(b"not a commit\n"),
    );
    assert_no_lineage(
        lineage_of(&f.repo, &blob),
        "a certified revision naming a blob answered the lineage",
    );
}

/// A tag chain deeper than the peel limit is refused, not read as descent.
/// Control: one annotated tag of HEAD descends.
#[test]
fn a_tag_chain_deeper_than_the_peel_limit_is_no_lineage() {
    let f = fixture(true);
    git(&f.repo, &["tag", "-a", "t-ok", "-m", "t", "HEAD"]);
    let one = f.rev("t-ok");
    assert_eq!(
        lineage_of(&f.repo, &one),
        (true, Some(Value::Bool(true))),
        "control: a tag of HEAD"
    );
    let certified = f.orphan_head();
    let mut prev = certified;
    for i in 0..9 {
        let name = format!("deep-{i}");
        git(
            &f.repo,
            &[
                "-c",
                "advice.nestedTag=false",
                "tag",
                "-a",
                &name,
                "-m",
                "t",
                &prev,
            ],
        );
        prev = f.rev(&name);
    }
    assert_no_lineage(
        lineage_of(&f.repo, &prev),
        "nine nested tags of a revision the orphan HEAD does not descend from answered the \
         lineage",
    );
}

/// An abbreviation that names TWO objects is refused, never resolved to
/// whichever one sorts first. Here it names HEAD and a blob made to share
/// its prefix (and sort after it). Control: the same abbreviation before the
/// blob exists descends.
#[test]
fn an_ambiguous_abbreviation_is_no_lineage() {
    let f = fixture(true);
    let head = f.rev("HEAD");
    let short = &head[..4];
    // The development check resolves a unique abbreviation (the protected
    // answer takes only a full id, A93, so this arm is the dev route's).
    assert!(
        lineage_of(&f.repo, short).0,
        "control: a unique abbreviation of HEAD"
    );
    let content = (0u64..)
        .map(|n| format!("collide {n}\n"))
        .find(|c| {
            let id = axon_fabric::git_data::object_id("blob", c.as_bytes());
            id.starts_with(short) && id.as_str() > head.as_str()
        })
        .unwrap();
    git_out(
        &f.repo,
        &["hash-object", "-w", "--stdin"],
        Some(content.as_bytes()),
    );
    assert_no_lineage(
        lineage_of(&f.repo, short),
        "an abbreviation naming two objects answered the lineage from one of them",
    );
}

/// A `.git` SYMLINK names a repository elsewhere: the tree beside it is no
/// clone, and is never described as a clean build of that repository's HEAD,
/// even with every file identical. Control: the clone itself is clean.
#[test]
fn a_symlinked_git_dir_is_never_a_clean_build_tree() {
    let f = fixture(true);
    let prov = |dir: &Path| -> Value {
        let o = Command::new(env!("CARGO_BIN_EXE_axon-provenance"))
            .arg(dir)
            .stderr(Stdio::null())
            .output()
            .unwrap();
        serde_json::from_slice(&o.stdout).unwrap()
    };
    let clean = prov(&f.repo);
    assert_eq!(clean["dirty"], serde_json::json!([]), "control: {clean}");
    let w = f.d.path().join("not-a-clone");
    let st = Command::new("cp")
        .arg("-a")
        .arg(&f.repo)
        .arg(&w)
        .status()
        .unwrap();
    assert!(st.success());
    std::fs::remove_dir_all(w.join(".git")).unwrap();
    std::os::unix::fs::symlink(f.repo.join(".git"), w.join(".git")).unwrap();
    let p = prov(&w);
    assert!(
        p["dirty"].as_array().is_some_and(|d| !d.is_empty()),
        "ATTACK: a tree whose .git is a symlink to another repository was described as a clean \
         build of its HEAD: {p}"
    );
    assert!(p.to_string().contains("gitfile or symlink"), "{p}");
}

/// A93 (C9 round 4b, rows4b; amendment 62): the guest manifest certified its
/// PCI lineage against an ABBREVIATION (`31413ca7`, 32 bits). An abbreviation
/// names whichever object a repository makes match it: an orphan commit whose
/// id is brute-forced to share it, in a clone that does not hold the real
/// certified commit, is the ONE object it names, and the orphan HEAD
/// "descended" from the certified revision. 32 bits is an offline search; here
/// the same attack at 16 bits. A protected lineage names the certified
/// revision by its whole hash. Control: the full id of an ancestor descends.
#[test]
fn an_abbreviated_certified_revision_never_answers_the_protected_lineage() {
    let f = fixture(true);
    let certified = f.rev("HEAD");
    assert_eq!(
        lineage_of(&f.repo, &certified).1,
        Some(Value::Bool(true)),
        "control: the full certified id"
    );
    let short = certified[..4].to_string();
    // An orphan commit over the same tree whose id begins like the certified
    // revision's (and is not it).
    let tree = f.rev("HEAD^{tree}");
    let body = (0u64..)
        .map(|n| {
            format!(
                "tree {tree}\nauthor a <a@example> 0 +0000\ncommitter a <a@example> 0 +0000\n\n\
                 unrelated history {n}\n"
            )
        })
        .find(|b| {
            let id = axon_fabric::git_data::object_id("commit", b.as_bytes());
            id.starts_with(&short) && id != certified
        })
        .unwrap();
    let orphan = git_out(
        &f.repo,
        &["hash-object", "-t", "commit", "-w", "--stdin"],
        Some(body.as_bytes()),
    );
    git(&f.repo, &["update-ref", "refs/heads/main", &orphan]);
    // A clone that does not hold the certified commit at all.
    git(&f.repo, &["reflog", "expire", "--expire=now", "--all"]);
    git(&f.repo, &["gc", "-q", "--prune=now"]);
    let got = lineage_of(&f.repo, &short);
    assert_ne!(
        got.1,
        Some(Value::Bool(true)),
        "ATTACK: an orphan HEAD brute-forced to share the certified revision's abbreviation \
         answered the protected PCI lineage"
    );
    assert_eq!(
        lineage_of(&f.repo, &certified).1,
        Some(Value::Bool(false)),
        "the full certified id is not in this clone"
    );
}
