#![allow(dead_code)]
//! Repository attacks shared by the integration tests (`tests/common`) and
//! the crate's own `git_data` tests (included by `#[path]`), so there is one
//! copy (review PSV-7 / FIELD-ORIGIN, C9 round 3): a linked worktree
//! disguised as a real `.git` directory (A79), and a forged ancestor object
//! (A80).

use std::path::{Path, PathBuf};
use std::process::Command;

const GIT: &str = "/usr/bin/git";

fn git(repo: &Path, args: &[&str]) {
    let st = Command::new(GIT)
        .arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(args)
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(st.success(), "git {args:?}");
}

/// `git worktree add --detach wt` from `main`, then the attack of review
/// PSV-7 / FIELD-ORIGIN (C9 round 3): the worktree's gitfile replaced by
/// a COPY of its admin directory, `commondir` pointing at `main/.git`.
/// `.git` is now a real directory, and git still reads main's objects,
/// refs and config.
pub fn disguise_worktree(main: &Path, wt: &Path) {
    git(
        main,
        &["worktree", "add", "-q", "--detach", wt.to_str().unwrap()],
    );
    let gitfile = std::fs::read_to_string(wt.join(".git")).unwrap();
    let admin = PathBuf::from(gitfile.strip_prefix("gitdir: ").unwrap().trim_end());
    std::fs::remove_file(wt.join(".git")).unwrap();
    let st = Command::new("cp")
        .arg("-a")
        .arg(&admin)
        .arg(wt.join(".git"))
        .status()
        .unwrap();
    assert!(st.success(), "cp -a the admin dir");
    let common = std::fs::canonicalize(main.join(".git")).unwrap();
    std::fs::write(wt.join(".git/commondir"), format!("{}\n", common.display())).unwrap();
    assert!(std::fs::symlink_metadata(wt.join(".git")).unwrap().is_dir());
}

/// Rewrite the LOOSE object of `commit` in place (same name) so that its
/// parent is `parent`: the forged-ancestor attack (review FIELD-ORIGIN,
/// C9 round 3). `git fsck` would report a hash-path mismatch; git's
/// commit walk does not look.
pub fn forge_parent(repo: &Path, commit: &str, parent: &str) {
    let o = Command::new(GIT)
        .arg("-C")
        .arg(repo)
        .args(["cat-file", "commit", commit])
        .output()
        .unwrap();
    let body = String::from_utf8(o.stdout).unwrap();
    let mut lines: Vec<String> = body
        .split('\n')
        .filter(|l| !l.starts_with("parent "))
        .map(String::from)
        .collect();
    lines.insert(1, format!("parent {parent}"));
    let forged = lines.join("\n");
    let path = repo
        .join(".git/objects")
        .join(&commit[..2])
        .join(&commit[2..]);
    let st = Command::new("python3")
        .args([
            "-c",
            "import sys,zlib,os;b=sys.stdin.buffer.read();p=sys.argv[1];os.chmod(p,0o644);\
             open(p,'wb').write(zlib.compress(b'commit %d\\0' % len(b) + b))",
        ])
        .arg(&path)
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut c| {
            use std::io::Write;
            c.stdin.take().unwrap().write_all(forged.as_bytes())?;
            c.wait()
        })
        .unwrap();
    assert!(st.success(), "forge {commit}");
}

/// `git commit-tree` of HEAD's tree with `parents`, as a new commit id.
pub fn commit_tree(repo: &Path, parents: &[&str], msg: &str) -> String {
    let mut c = Command::new(GIT);
    c.arg("-C")
        .arg(repo)
        .args(["-c", "user.name=t", "-c", "user.email=t@example"])
        .args(["commit-tree", "HEAD^{tree}", "-m", msg]);
    for p in parents {
        c.args(["-p", p]);
    }
    String::from_utf8(c.output().unwrap().stdout)
        .unwrap()
        .trim()
        .to_string()
}

pub fn rev(repo: &Path, what: &str) -> String {
    let o = Command::new(GIT)
        .arg("-C")
        .arg(repo)
        .args(["rev-parse", what])
        .output()
        .unwrap();
    String::from_utf8(o.stdout).unwrap().trim().to_string()
}
