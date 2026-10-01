//! The mutation and paired-disable harnesses stamp their evidence with a
//! COMMIT, so they must run on exactly that commit and say which registry they
//! ran (C9 round 4, EQUIVALENCE (5)).
//!
//! Round 4 executed the hole: with uncommitted edits to
//! scripts/v022_freeze_manifest.py (M650's guarded file) and
//! profiles/linux-microvm/guest-init.sh, `v022_g01_mutations.py --only=M650`
//! wrote commit 1b687d95, all_killed true, because the dirty check looked at
//! crates/ only -- and 24 rows guard files outside crates/, and the registry
//! and markers live in scripts/. Shards recorded no registry, so --merge and
//! --join could not tell a run made from a locally edited registry.
//!
//! Each test runs the REAL harness script, copied from this tree into a
//! scratch repository, through its command line.

mod script_spawn;
use script_spawn::{repo_root, Bins};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const HARNESS: [&str; 4] = [
    "scripts/v022_g01_mutations.py",
    "scripts/v022_attack_markers.py",
    "scripts/v022_paired_disable.py",
    "scripts/lib_bounded_run.sh",
];

fn scratch(tag: &str) -> PathBuf {
    static N: AtomicU64 = AtomicU64::new(0);
    let d = std::env::temp_dir().join(format!(
        "axon-harness-integrity-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn write(p: &Path, s: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, s).unwrap();
}

fn git(r: &Path, args: &[&str]) -> String {
    let o = Command::new("/usr/bin/git")
        .arg("-C")
        .arg(r)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .output()
        .unwrap();
    assert!(o.status.success(), "git {args:?}: {o:?}");
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}

/// A committed scratch repository holding the harness and a guarded file
/// outside crates/ (as guest-init.sh is for ten rows).
fn repo(tag: &str) -> PathBuf {
    let r = scratch(tag);
    git(&r, &["init", "-q", "-b", "main"]);
    for f in HARNESS {
        write(
            &r.join(f),
            &std::fs::read_to_string(repo_root().join(f)).unwrap(),
        );
    }
    write(
        &r.join("profiles/linux-microvm/guest-init.sh"),
        "#!/bin/sh\n",
    );
    write(&r.join(".gitignore"), "__pycache__/\n/target/\n");
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "candidate"]);
    r
}

/// `python3 <harness> <args>` in `r`.
fn harness(r: &Path, script: &str, args: &[&str]) -> Output {
    script_spawn::script("python3", r.join(script), Bins::BuildsItsOwn)
        .args(args)
        .current_dir(r)
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap()
}

/// An inline Python program run in `r` with the harness modules importable.
fn py(r: &Path, program: &str) -> String {
    let o = Command::new("python3")
        .args(["-c", program])
        .current_dir(r)
        .env("PYTHONPATH", r.join("scripts"))
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .output()
        .unwrap();
    assert!(o.status.success(), "setup program failed: {o:?}");
    String::from_utf8(o.stdout).unwrap()
}

fn text(o: &Output) -> String {
    format!(
        "{}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    )
}

/// The ways a tree can differ from its commit outside crates/.
/// A named way to make the tree dirty.
type DirtyCase = (&'static str, Box<dyn Fn()>);

fn dirty_cases(r: &Path) -> Vec<DirtyCase> {
    let (a, b, c) = (r.to_path_buf(), r.to_path_buf(), r.to_path_buf());
    vec![
        (
            "an uncommitted edit to a guarded file in profiles/",
            Box::new(move || {
                write(
                    &a.join("profiles/linux-microvm/guest-init.sh"),
                    "#!/bin/sh\n# edited\n",
                )
            }),
        ),
        (
            "an uncommitted edit to the attack-marker registry",
            Box::new(move || {
                let p = b.join("scripts/v022_attack_markers.py");
                let s = std::fs::read_to_string(&p).unwrap();
                write(&p, &format!("{s}\n# edited\n"));
            }),
        ),
        (
            "an untracked file in scripts/",
            Box::new(move || write(&c.join("scripts/extra.py"), "x = 1\n")),
        ),
    ]
}

/// A mutation run refuses any uncommitted change in the tree, not only under
/// crates/. Control: the clean tree gets past the check (and then stops on
/// the missing workspace, which is not this check).
#[test]
fn a_mutation_run_refuses_a_tree_with_an_uncommitted_change_outside_crates() {
    let args = ["--scope=psv", "--only=M650", "out.json"];
    let clean = repo("g01-clean");
    let o = harness(&clean, HARNESS[0], &args);
    assert!(
        !text(&o).contains("uncommitted changes"),
        "control: a clean tree is not refused as dirty: {}",
        text(&o)
    );
    for i in 0..3 {
        let r = repo("g01-dirty");
        let (what, make) = dirty_cases(&r).remove(i);
        make();
        let o = harness(&r, HARNESS[0], &args);
        assert!(
            !o.status.success() && text(&o).contains("uncommitted changes in the tree"),
            "ATTACK: a mutation run proceeded on a tree with an uncommitted change outside \
             crates ({what}):\n{}",
            text(&o)
        );
        let _ = std::fs::remove_dir_all(&r);
    }
    let _ = std::fs::remove_dir_all(&clean);
}

/// The same for a paired-disable run (and each of its shards).
#[test]
fn a_paired_disable_run_refuses_a_tree_with_an_uncommitted_change_outside_crates() {
    let args = ["--shard=0/1", "out.json"];
    let clean = repo("pd-clean");
    let o = harness(&clean, HARNESS[2], &args);
    assert!(
        !text(&o).contains("uncommitted changes"),
        "control: a clean tree is not refused as dirty: {}",
        text(&o)
    );
    for i in 0..3 {
        let r = repo("pd-dirty");
        let (what, make) = dirty_cases(&r).remove(i);
        make();
        let o = harness(&r, HARNESS[2], &args);
        assert!(
            !o.status.success() && text(&o).contains("uncommitted changes in the tree"),
            "ATTACK: a paired-disable run proceeded on a tree with an uncommitted change \
             outside crates ({what}):\n{}",
            text(&o)
        );
        let _ = std::fs::remove_dir_all(&r);
    }
    let _ = std::fs::remove_dir_all(&clean);
}

/// Mutation shards of the `binding` scope, as a run at HEAD writes them;
/// shard 1 claims `blobs` as its registry when given.
const MERGE_SHARDS: &str = r#"
import json, sys, v022_g01_mutations as m
head = sys.argv[1]; alt = sys.argv[2] == "alt"; out = sys.argv[3]
rows = [r for r in m.MUTATIONS if m.in_scope(r[0], "binding")]
for k in (0, 1):
    blobs = m.registry_blobs()
    if alt and k == 1:
        blobs = {f: "0" * 40 for f in blobs}
    d = {"schema": "axon-v022-mutation-run/3", "gate": "binding", "scope": "binding",
         "commit": head, "registry_blobs": blobs, "tree_clean": True, "toolchain": {},
         "shard": {"index": k, "of": 2}, "only": None, "all_killed": True,
         "mutations": [{"id": r[0], **m.row_digest(r), "result": "killed", "baseline": "passed"}
                       for i, r in enumerate(rows) if i % 2 == k]}
    json.dump(d, open(f"{out}/s{k}.json", "w"))
"#;

/// --merge refuses a shard made from a registry other than this tree's.
/// Control: two shards of this registry merge.
#[test]
fn a_merge_refuses_a_shard_made_from_another_registry() {
    let r = repo("merge");
    // Shards and output OUTSIDE the tree, which must stay clean.
    let out = scratch("merge-out");
    let head = git(&r, &["rev-parse", "HEAD"]);
    let p = |f: &str| out.join(f).display().to_string();
    let run = |alt: &str| {
        let prog = format!(
            "import sys; sys.argv = ['x', {head:?}, {alt:?}, {:?}]\n{MERGE_SHARDS}",
            out.display().to_string()
        );
        py(&r, &prog);
        harness(
            &r,
            HARNESS[0],
            &["--merge", &p("merged.json"), &p("s0.json"), &p("s1.json")],
        )
    };
    let o = run("alt");
    assert!(
        !o.status.success() && text(&o).contains("registry/marker blobs"),
        "ATTACK: --merge accepted a shard made from another registry:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_file(out.join("merged.json"));
    let o = run("same");
    // (The merged verdict itself fails here: the scratch tree holds none of
    // the guards the retirement records name. The merge is what is judged.)
    assert!(
        out.join("merged.json").exists() && !text(&o).contains("refused:"),
        "control: shards of this registry merge: {}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&out);
}

/// Paired-disable shards, as a run at HEAD writes them.
const JOIN_SHARDS: &str = r#"
import json, sys, v022_g01_mutations as m, v022_paired_disable as pd
head = sys.argv[1]; alt = sys.argv[2] == "alt"; out = sys.argv[3]
u = sorted(set(m.EQUIVALENT_DID) | set(m.STALE_REFACTORED), key=lambda r: int(r[1:]))
for k in (0, 1):
    blobs = m.registry_blobs()
    if alt and k == 1:
        blobs = {f: "0" * 40 for f in blobs}
    sel = [x for i, x in enumerate(u) if i % 2 == k]
    d = {"schema": "axon-v022-paired-disable/2", "commit": head, "all_hold": True,
         "shard": f"{k}/2", "selected": sel, "registry_blobs": blobs, "tree_clean": True,
         "records": [{"mutation": x, "holds": True, "commit": head,
                      "edits_sha256": pd.current_edits_digest(x)} for x in sel]}
    json.dump(d, open(f"{out}/j{k}.json", "w"))
"#;

/// --join refuses a shard made from a registry other than this tree's.
/// Control: two shards of this registry join.
#[test]
fn a_join_refuses_a_shard_made_from_another_registry() {
    let r = repo("join");
    let out = scratch("join-out");
    let head = git(&r, &["rev-parse", "HEAD"]);
    let p = |f: &str| out.join(f).display().to_string();
    let run = |alt: &str| {
        let prog = format!(
            "import sys; sys.argv = ['x', {head:?}, {alt:?}, {:?}]\n{JOIN_SHARDS}",
            out.display().to_string()
        );
        py(&r, &prog);
        harness(
            &r,
            HARNESS[2],
            &["--join", &p("joined.json"), &p("j0.json"), &p("j1.json")],
        )
    };
    let o = run("alt");
    assert!(
        !o.status.success() && text(&o).contains("registry/marker blobs"),
        "ATTACK: --join accepted a shard made from another registry:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_file(out.join("joined.json"));
    let o = run("same");
    assert!(
        o.status.success() && out.join("joined.json").exists(),
        "control: shards of this registry join: {}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_dir_all(&out);
}

/// A kept paired-disable record is stale once ANY file of its owner package
/// changes, not only the files a list named (C9 round 4: M602's argument rests
/// on bin/axon-custodian.rs, M186's on backend.rs, neither on the list).
/// Control: at its own commit the record is current.
#[test]
fn a_kept_record_is_stale_once_its_owner_package_changes() {
    let r = repo("stale");
    // M245's guard is in axon-loop: a one-crate workspace of that name.
    write(
        &r.join("Cargo.toml"),
        "[workspace]\nmembers = [\"crates/axon-loop\"]\nresolver = \"2\"\n",
    );
    write(
        &r.join("crates/axon-loop/Cargo.toml"),
        "[package]\nname = \"axon-loop\"\nversion = \"0.0.0\"\nedition = \"2021\"\n",
    );
    write(&r.join("crates/axon-loop/src/lib.rs"), "// v1\n");
    write(&r.join("crates/axon-loop/src/helper.rs"), "// v1\n");
    git(&r, &["add", "-A"]);
    git(&r, &["commit", "-q", "-m", "owner package"]);
    let at = git(&r, &["rev-parse", "HEAD"]);
    let status = r.parent().unwrap().join(format!(
        "{}-status.json",
        r.file_name().unwrap().to_string_lossy()
    ));
    let prog = format!(
        "import json, v022_paired_disable as pd\n\
         json.dump({{'commit': {at:?}, 'records': [{{'mutation': 'M245', 'commit': {at:?}, \
         'holds': True, 'edits_sha256': pd.current_edits_digest('M245')}}]}}, \
         open({:?}, 'w'))\n",
        status.display().to_string()
    );
    py(&r, &prog);
    let check = || harness(&r, HARNESS[2], &["--check-stale", status.to_str().unwrap()]);
    let o = check();
    assert!(
        o.status.success() && text(&o).contains("current M245"),
        "control: at its own commit the record is current: {}",
        text(&o)
    );
    // A file of the owner package the old list never named.
    write(
        &r.join("crates/axon-loop/src/helper.rs"),
        "// v2: the all-paths argument rested on this\n",
    );
    git(&r, &["commit", "-q", "-am", "helper changed"]);
    let o = check();
    assert!(
        !o.status.success() && text(&o).contains("STALE M245"),
        "ATTACK: a kept paired-disable record stayed current after its owner package \
         changed:\n{}",
        text(&o)
    );
    let _ = std::fs::remove_dir_all(&r);
    let _ = std::fs::remove_file(&status);
}
