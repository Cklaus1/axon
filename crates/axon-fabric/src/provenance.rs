//! Build provenance (`fabric_revision`, `source_dirty`) for the readiness
//! verifier, computed by `build.rs` (which includes this file by path) and
//! unit-tested from the library.
//!
//! `source_dirty: false` is the operator's machine evidence that the installed
//! verifier came from a clean, reviewed tree (readiness refuses a dirty
//! production verifier). It used to come from PATH `git` under the ambient
//! environment and `status --porcelain --untracked-files=no`, i.e. from state
//! the repository writer controls: a skip-worktree or assume-unchanged entry
//! hides a modified file from porcelain, untracked files (a `.cargo/config.toml`
//! that changes the build) were excluded outright, and replace refs or grafts
//! rewrite what `HEAD` means (review FIELD-ORIGIN, C9 round 1, class d).
//!
//! Here git is the operator-installed `/usr/bin/git`, with the caller's
//! environment dropped, replace objects off, and system/global config and the
//! repository's code-running settings overridden (the same invocation as
//! `readiness`'s). The tree is DIRTY — never "probably clean" — when any of
//! these holds, each named in the returned reasons:
//! * git cannot answer (no repository, no HEAD, an error);
//! * a `refs/replace/` ref or an `info/grafts` file exists;
//! * an index entry is skip-worktree or assume-unchanged;
//! * `status` shows any change, staged or not, or ANY untracked file that is
//!   not ignored;
//! * an untracked file is ignored by a rule that is not in a TRACKED
//!   `.gitignore` (`info/exclude`, an untracked `.gitignore`, a config
//!   `core.excludesFile`): only the reviewed tree may say what to ignore.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The operator-installed git; never resolved through PATH.
pub const GIT_BIN: &str = "/usr/bin/git";

/// A git invocation that answers about the real objects and files of the
/// repository containing `dir`, and nothing else.
pub fn git_cmd(dir: &Path) -> Command {
    let mut c = Command::new(GIT_BIN);
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_NO_REPLACE_OBJECTS", "1")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("--no-replace-objects")
        .args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"])
        .args(["-c", "core.untrackedCache=false"])
        .args(["-c", "core.excludesFile=/dev/null"])
        .args(["-c", "advice.graftFileDeprecated=false"])
        .args(["-c", "safe.directory=*"])
        .arg("-C")
        .arg(dir)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    c
}

fn run(dir: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let o = git_cmd(dir)
        .args(args)
        .output()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    if !o.status.success() {
        return Err(format!("git {} failed", args.join(" ")));
    }
    Ok(o.stdout)
}

fn text(dir: &Path, args: &[&str]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&run(dir, args)?).trim().to_string())
}

fn nul_list(b: &[u8]) -> Vec<String> {
    b.split(|x| *x == 0)
        .filter(|e| !e.is_empty())
        .map(|e| String::from_utf8_lossy(e).to_string())
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Provenance {
    /// The commit id HEAD names, or "unknown".
    pub revision: String,
    /// Why the tree is dirty; empty means clean.
    pub dirty: Vec<String>,
    /// Paths a rebuild must watch (git's HEAD, index, refs, info/).
    pub watch: Vec<PathBuf>,
}

/// The top of the working tree containing `dir`.
pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    match text(dir, &["rev-parse", "--show-toplevel"]) {
        Ok(t) if !t.is_empty() => Ok(PathBuf::from(t)),
        _ => Err("not in a git working tree".into()),
    }
}

/// The provenance of the repository containing `dir`.
pub fn provenance(dir: &Path) -> Provenance {
    let unknown = |why: String| Provenance {
        revision: "unknown".into(),
        dirty: vec![why],
        watch: Vec::new(),
    };
    let top = match toplevel(dir) {
        Ok(t) => t,
        Err(e) => return unknown(e),
    };
    let revision = match text(&top, &["rev-parse", "--verify", "HEAD^{commit}"]) {
        Ok(r) => r,
        Err(e) => return unknown(e),
    };
    let watch = ["HEAD", "index", "packed-refs", "refs", "info"]
        .iter()
        .filter_map(|p| text(&top, &["rev-parse", "--git-path", p]).ok())
        .map(|p| top.join(p))
        .collect();
    Provenance {
        revision,
        dirty: dirty_reasons(&top),
        watch,
    }
}

/// Every reason the working tree at `top` is not exactly its HEAD commit.
pub fn dirty_reasons(top: &Path) -> Vec<String> {
    let mut why = Vec::new();
    let mut check = |r: Result<Option<String>, String>| match r {
        Ok(Some(w)) => why.push(w),
        Ok(None) => {}
        Err(e) => why.push(format!("cannot tell: {e}")),
    };
    check(
        text(
            top,
            &["for-each-ref", "--format=%(refname)", "refs/replace/"],
        )
        .map(|r| {
            r.lines()
                .next()
                .map(|r| format!("replace ref {r} rewrites what an object id names"))
        }),
    );
    check(
        text(top, &["rev-parse", "--git-path", "info/grafts"]).map(|g| {
            std::fs::symlink_metadata(top.join(&g))
                .is_ok()
                .then(|| format!("{g} rewrites ancestry"))
        }),
    );
    // `-v`: `S` marks skip-worktree, a lowercase tag assume-unchanged. Either
    // tells git not to compare the file, which porcelain then does not show.
    check(run(top, &["ls-files", "-z", "-v"]).map(|b| {
        nul_list(&b).into_iter().find_map(|e| {
            let tag = *e.as_bytes().first()?;
            let path = e.get(2..).unwrap_or("");
            if tag == b'S' || tag == b's' {
                Some(format!("{path} is marked skip-worktree"))
            } else if tag.is_ascii_lowercase() {
                Some(format!("{path} is marked assume-unchanged"))
            } else {
                None
            }
        })
    }));
    check(
        run(
            top,
            &[
                "status",
                "--porcelain=v1",
                "-z",
                "--untracked-files=all",
                "--ignore-submodules=none",
            ],
        )
        .map(|b| {
            nul_list(&b)
                .first()
                .map(|e| format!("uncommitted change or untracked file: {e}"))
        }),
    );
    // Untracked files that ARE ignored: only a rule from a tracked (and, by
    // the status check above, unmodified) .gitignore may hide them.
    check(ignored_by_untracked_rule(top));
    why
}

fn ignored_by_untracked_rule(top: &Path) -> Result<Option<String>, String> {
    let ignored = nul_list(&run(
        top,
        &[
            "ls-files",
            "-z",
            "--others",
            "--ignored",
            "--exclude-standard",
            "--directory",
        ],
    )?);
    if ignored.is_empty() {
        return Ok(None);
    }
    let tracked: std::collections::BTreeSet<String> = nul_list(&run(top, &["ls-files", "-z"])?)
        .into_iter()
        .collect();
    let mut c = git_cmd(top);
    c.args(["check-ignore", "-v", "-z", "--no-index", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    let mut child = c.spawn().map_err(|e| format!("{GIT_BIN}: {e}"))?;
    {
        use std::io::Write;
        let mut stdin = child.stdin.take().ok_or("git check-ignore: no stdin")?;
        for p in &ignored {
            stdin
                .write_all(p.as_bytes())
                .and_then(|()| stdin.write_all(b"\0"))
                .map_err(|e| format!("git check-ignore: {e}"))?;
        }
    }
    let out = child
        .wait_with_output()
        .map_err(|e| format!("git check-ignore: {e}"))?;
    // Records of four NUL-terminated fields: source, line, pattern, path.
    let f: Vec<&[u8]> = out.stdout.split(|x| *x == 0).collect();
    let mut matched = 0;
    for r in f.chunks(4).filter(|r| r.len() == 4) {
        let (source, path) = (
            String::from_utf8_lossy(r[0]).to_string(),
            String::from_utf8_lossy(r[3]).to_string(),
        );
        if path.is_empty() {
            continue;
        }
        matched += 1;
        if source.is_empty() || !tracked.contains(&source) {
            return Ok(Some(format!(
                "{path} is ignored by a rule outside the tracked tree ({})",
                if source.is_empty() {
                    "unknown source"
                } else {
                    &source
                }
            )));
        }
    }
    if matched < ignored.len() {
        return Err("git check-ignore did not account for every ignored path".into());
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(repo: &Path, args: &[&str]) {
        let st = Command::new(GIT_BIN)
            .arg("-C")
            .arg(repo)
            .args(["-c", "user.name=t", "-c", "user.email=t@example"])
            .args(args)
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(st.success(), "git {args:?}");
    }

    /// A committed repository: `src/lib.rs`, and a tracked `.gitignore`
    /// that ignores `/target`.
    fn repo() -> (tempfile::TempDir, PathBuf) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("repo");
        std::fs::create_dir_all(r.join("src")).unwrap();
        git(&r, &["init", "-q", "-b", "main"]);
        std::fs::write(r.join("src/lib.rs"), "// reviewed\n").unwrap();
        std::fs::write(r.join(".gitignore"), "/target\n").unwrap();
        git(&r, &["add", "-A"]);
        git(&r, &["commit", "-q", "-m", "reviewed"]);
        (d, r)
    }

    fn assert_dirty(r: &Path, attack: &str, why: &str) {
        let p = provenance(r);
        assert!(
            !p.dirty.is_empty(),
            "ATTACK: {attack}, and the build provenance still says source_dirty: false"
        );
        assert!(
            p.dirty.iter().any(|w| w.contains(why)),
            "expected {why:?}: {:?}",
            p.dirty
        );
    }

    #[test]
    fn a_clean_tree_is_clean_and_names_its_head() {
        let (_d, r) = repo();
        std::fs::create_dir_all(r.join("target/debug")).unwrap();
        std::fs::write(r.join("target/debug/x"), "build output\n").unwrap();
        let p = provenance(&r);
        assert_eq!(p.dirty, Vec::<String>::new());
        assert_eq!(p.revision.len(), 40);
        // From a subdirectory too (build.rs runs in the crate directory).
        assert_eq!(provenance(&r.join("src")).dirty, Vec::<String>::new());
    }

    #[test]
    fn a_skip_worktree_or_assume_unchanged_change_is_dirty() {
        let (_d, r) = repo();
        std::fs::write(r.join("src/lib.rs"), "// the agent's code\n").unwrap();
        git(&r, &["update-index", "--skip-worktree", "src/lib.rs"]);
        assert_dirty(
            &r,
            "a skip-worktree entry hid a modified source file",
            "skip-worktree",
        );

        let (_d, r) = repo();
        std::fs::write(r.join("src/lib.rs"), "// the agent's code\n").unwrap();
        git(&r, &["update-index", "--assume-unchanged", "src/lib.rs"]);
        assert_dirty(
            &r,
            "an assume-unchanged entry hid a modified source file",
            "assume-unchanged",
        );
    }

    #[test]
    fn an_untracked_file_is_dirty() {
        let (_d, r) = repo();
        std::fs::create_dir_all(r.join(".cargo")).unwrap();
        std::fs::write(r.join(".cargo/config.toml"), "[build]\nrustflags = []\n").unwrap();
        assert_dirty(
            &r,
            "an untracked .cargo/config.toml changes the build",
            "untracked file",
        );
    }

    #[test]
    fn a_file_ignored_by_an_untracked_rule_is_dirty() {
        // info/exclude: repository-local, never reviewed.
        let (_d, r) = repo();
        std::fs::create_dir_all(r.join(".cargo")).unwrap();
        std::fs::write(r.join(".cargo/config.toml"), "[build]\n").unwrap();
        let ex = r.join(".git/info/exclude");
        std::fs::create_dir_all(ex.parent().unwrap()).unwrap();
        std::fs::write(&ex, ".cargo/\n").unwrap();
        assert_dirty(
            &r,
            "info/exclude hid an untracked .cargo/config.toml",
            "outside the tracked tree",
        );

        // An untracked .gitignore that ignores itself and its directory.
        let (_d, r) = repo();
        std::fs::create_dir_all(r.join(".cargo")).unwrap();
        std::fs::write(r.join(".cargo/config.toml"), "[build]\n").unwrap();
        std::fs::write(r.join(".cargo/.gitignore"), "*\n").unwrap();
        assert_dirty(
            &r,
            "a self-ignoring .gitignore hid an untracked .cargo/config.toml",
            "outside the tracked tree",
        );
    }

    #[test]
    fn a_replace_ref_or_graft_is_dirty() {
        let (_d, r) = repo();
        let head = text(&r, &["rev-parse", "HEAD"]).unwrap();
        git(&r, &["update-ref", &format!("refs/replace/{head}"), &head]);
        assert_dirty(&r, "a replace ref rewrote what HEAD names", "replace ref");

        let (_d, r) = repo();
        std::fs::write(r.join(".git/info/grafts"), "").unwrap();
        assert_dirty(&r, "a grafts file rewrote ancestry", "rewrites ancestry");
    }
}
