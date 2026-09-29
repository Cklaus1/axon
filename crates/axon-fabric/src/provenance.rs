//! Build provenance (`fabric_revision`, `source_dirty`) for the readiness
//! verifier, computed by `build.rs` (which includes this file and
//! `git_data.rs` by path), by the `axon-provenance` helper that writes the
//! guest-image manifest's `axon_tree_dirty_at_build`, and unit-tested from
//! the library: ONE implementation.
//!
//! `source_dirty: false` is the operator's machine evidence that the installed
//! verifier came from a clean, reviewed tree (readiness refuses a dirty
//! production verifier). It used to come from PATH `git` under the ambient
//! environment and `status --porcelain --untracked-files=no`, i.e. from state
//! the repository writer controls (review FIELD-ORIGIN, C9 round 1, class d);
//! and then from a hardened git that still honoured the repository's own
//! `.git/config` (`core.worktree` pointed status at a clean mirror,
//! `core.checkStat=minimal` + `core.trustCtime=false` hid a same-size edit;
//! C9 round 2).
//!
//! Git is run as [`crate::git_data`] runs it. The tree is DIRTY — never
//! "probably clean" — when any of these holds, each named in the reasons:
//! * git cannot answer (no repository, no HEAD, an error), or `.git` is not a
//!   real directory (a gitfile or symlink names a repository elsewhere);
//! * the repository's own config sets anything that could redirect what git
//!   reports or run code ([`crate::git_data::refuse_config`]);
//! * a `refs/replace/` ref or an `info/grafts` file exists;
//! * an index entry is skip-worktree or assume-unchanged;
//! * `status` shows any change, staged or not, or ANY untracked file that is
//!   not ignored;
//! * an untracked file is ignored by a rule that is not in a TRACKED
//!   `.gitignore` (`info/exclude`, an untracked `.gitignore`, a config
//!   `core.excludesFile`): only the reviewed tree may say what to ignore;
//! * any file of HEAD's tree, hashed from its bytes, is not HEAD's object.

use crate::git_data::{self, run, text};
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub use crate::git_data::GIT_BIN;

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

/// The top of the working tree containing `dir` ([`git_data::discover`]:
/// the nearest `.git`, which must be a real directory; never what git's
/// `core.worktree` or a gitfile says).
pub fn toplevel(dir: &Path) -> Result<PathBuf, String> {
    git_data::discover(dir)
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
    // The repository's own config is refused before anything it says is
    // believed (core.worktree, filter drivers, a promisor remote…).
    if let Err(e) = git_data::refuse_config(&top) {
        return unknown(e);
    }
    let revision = match text(&top, &["rev-parse", "--verify", "HEAD^{commit}"]) {
        Ok(r) => r,
        Err(e) => return unknown(e),
    };
    let watch = ["HEAD", "index", "packed-refs", "refs", "info", "config"]
        .iter()
        .filter_map(|p| text(&top, &["rev-parse", "--git-path", p]).ok())
        .map(|p| top.join(p))
        .collect();
    let mut dirty = dirty_reasons(&top);
    // HEAD's committed bytes against the FILES, hashed here: git's stat
    // cache (the index, a repository file) is not consulted.
    if let Some(why) = head_bytes_differ(&top, &revision) {
        dirty.push(why);
    }
    Provenance {
        revision,
        dirty,
        watch,
    }
}

/// Does HEAD of the tree containing `dir` descend from `rev`? Answered by
/// the same git as [`provenance`]: the repository's config refused unless
/// inert, replace objects off, and an `info/grafts` file (which rewrites
/// ancestry and which git still honours) refused. Err says why not.
pub fn descends_from(dir: &Path, rev: &str) -> Result<(), String> {
    if rev.is_empty() || rev.starts_with('-') {
        return Err(format!("{rev:?} is not a revision"));
    }
    let top = git_data::discover_linked(dir)?;
    git_data::refuse_config(&top)?;
    let g = text(&top, &["rev-parse", "--git-path", "info/grafts"])?;
    if std::fs::symlink_metadata(top.join(&g)).is_ok() {
        return Err(format!(
            "{g} rewrites ancestry: no lineage is read through it"
        ));
    }
    let st = git_data::git_cmd(&top)
        .args(["merge-base", "--is-ancestor", rev, "HEAD"])
        .status()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("HEAD does not descend from {rev}"))
    }
}

/// Why the working tree at `top` is not byte-for-byte `revision`'s tree, if
/// it is not (or cannot be shown to be).
pub fn head_bytes_differ(top: &Path, revision: &str) -> Option<String> {
    let tree = git_data::Objects::open(top).and_then(|mut o| o.entries(revision, None));
    match tree {
        Err(e) => Some(format!("cannot tell: {e}")),
        #[cfg(unix)]
        Ok(t) => git_data::worktree_differs(top, &t)
            .map(|p| format!("{p}: its bytes are not HEAD's committed bytes")),
        #[cfg(not(unix))]
        Ok(_) => Some("cannot tell: the working tree cannot be hashed on this platform".into()),
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
    let mut c = git_data::git_cmd(top);
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
    use std::process::Command;

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
        // The skip-worktree refusal (M346) and the byte comparison (M451)
        // each refuse this alone (M346's four-cell record): either reason.
        let p = provenance(&r);
        assert!(
            !p.dirty.is_empty(),
            "ATTACK: a skip-worktree entry hid a modified source file, and the build provenance \
             still says source_dirty: false"
        );
        assert!(
            p.dirty
                .iter()
                .any(|w| w.contains("skip-worktree") || w.contains("not HEAD's committed bytes")),
            "{:?}",
            p.dirty
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
        // HEAD replaced by another commit (same tree, other history), as a
        // real replacement would be. (A self-replacement is a cycle that git
        // refuses to resolve when replace objects are honoured, which tested
        // git's cycle check rather than this refusal.)
        let fake = Command::new(GIT_BIN)
            .arg("-C")
            .arg(&r)
            .args(["-c", "user.name=t", "-c", "user.email=t@example"])
            .args(["commit-tree", "HEAD^{tree}", "-p", "HEAD", "-m", "fake"])
            .output()
            .unwrap();
        let fake = String::from_utf8(fake.stdout).unwrap();
        let fake = fake.trim();
        git(&r, &["update-ref", &format!("refs/replace/{head}"), fake]);
        assert_dirty(&r, "a replace ref rewrote what HEAD names", "replace ref");

        let (_d, r) = repo();
        std::fs::write(r.join(".git/info/grafts"), "").unwrap();
        assert_dirty(&r, "a grafts file rewrote ancestry", "rewrites ancestry");
    }

    // ── C9 round 2 (FIELD-ORIGIN): the repository's own .git/config and the
    // index's stat cache must not make a modified tree read clean.

    fn set_mtime(f: &Path, t: std::time::SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(f)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }

    /// An mtime well before the index was written, so git never treats the
    /// entry as "racily clean" (and re-reads the file for that reason).
    fn old_mtime() -> std::time::SystemTime {
        std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1 << 30)
    }

    /// A same-size edit whose mtime is put back to what the index recorded.
    fn edit_same_size_keep_mtime(r: &Path) {
        let f = r.join("src/lib.rs");
        set_mtime(&f, old_mtime());
        git(r, &["update-index", "--refresh"]);
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        std::fs::write(&f, "// agentcod\n").unwrap();
        assert_eq!(std::fs::metadata(&f).unwrap().len(), 12, "same size");
        set_mtime(&f, mtime);
    }

    #[test]
    fn core_worktree_pointing_at_a_clean_mirror_is_dirty() {
        let (_d, r) = repo();
        std::fs::write(r.join("src/lib.rs"), "// the agent's code\n").unwrap();
        // A clean copy of HEAD in the ignored target/, and the repository's
        // own config pointing git's working tree at it.
        let mirror = r.join("target/mirror");
        std::fs::create_dir_all(mirror.join("src")).unwrap();
        std::fs::write(mirror.join("src/lib.rs"), "// reviewed\n").unwrap();
        std::fs::write(mirror.join(".gitignore"), "/target\n").unwrap();
        git(&r, &["config", "core.worktree", mirror.to_str().unwrap()]);
        assert_dirty(
            &r,
            "core.worktree pointed git at a clean mirror of HEAD",
            "core.worktree",
        );
    }

    #[test]
    fn stat_cache_knobs_in_the_repository_config_do_not_hide_an_edit() {
        let (_d, r) = repo();
        git(&r, &["config", "core.trustCtime", "false"]);
        git(&r, &["config", "core.checkStat", "minimal"]);
        // The control: git as the repository configures it reads clean.
        let st = std::process::Command::new(GIT_BIN)
            .arg("-C")
            .arg(&r)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(st.stdout.is_empty(), "control: a clean tree");
        edit_same_size_keep_mtime(&r);
        let st = std::process::Command::new(GIT_BIN)
            .arg("-C")
            .arg(&r)
            .args(["status", "--porcelain"])
            .output()
            .unwrap();
        assert!(
            st.stdout.is_empty(),
            "control: git as the repository configures it reads the edit clean"
        );
        assert_dirty(
            &r,
            "core.checkStat=minimal + core.trustCtime=false hid a same-size edit with its mtime \
             restored",
            "src/lib.rs",
        );
    }

    /// Rewrite the index entry for `path` so its cached stat data matches
    /// the file as it is NOW, while its object id stays HEAD's: git's stat
    /// cache then answers "unchanged" without reading the file. The index
    /// is a repository file; anyone who can write the tree can write it.
    fn forge_index_stat(r: &Path, path: &str) {
        use std::os::unix::fs::MetadataExt;
        git(r, &["update-index", "--index-version", "2"]);
        let md = std::fs::symlink_metadata(r.join(path)).unwrap();
        let ix = r.join(".git/index");
        let mut b = std::fs::read(&ix).unwrap();
        assert_eq!(&b[..4], b"DIRC");
        assert_eq!(u32::from_be_bytes(b[4..8].try_into().unwrap()), 2);
        let n = u32::from_be_bytes(b[8..12].try_into().unwrap());
        let mut i = 12;
        let mut found = false;
        for _ in 0..n {
            let flags = u16::from_be_bytes(b[i + 60..i + 62].try_into().unwrap());
            let len = usize::from(flags & 0x0fff);
            let name = b[i + 62..i + 62 + len].to_vec();
            if name == path.as_bytes() {
                let put = |b: &mut Vec<u8>, at: usize, v: u64| {
                    b[i + at..i + at + 4].copy_from_slice(&(v as u32).to_be_bytes())
                };
                put(&mut b, 0, md.ctime() as u64);
                put(&mut b, 4, md.ctime_nsec() as u64);
                put(&mut b, 8, md.mtime() as u64);
                put(&mut b, 12, md.mtime_nsec() as u64);
                put(&mut b, 16, md.dev());
                put(&mut b, 20, md.ino());
                put(&mut b, 28, u64::from(md.uid()));
                put(&mut b, 32, u64::from(md.gid()));
                put(&mut b, 36, md.size());
                found = true;
            }
            i += (62 + len + 8) / 8 * 8;
        }
        assert!(found, "{path} is in the index");
        let body = b.len() - 20;
        if b[body..].iter().any(|x| *x != 0) {
            let h = crate::git_data::sha1(&b[..body]);
            b[body..].copy_from_slice(&h);
        }
        std::fs::write(&ix, &b).unwrap();
    }

    #[test]
    fn a_forged_index_stat_entry_does_not_hide_an_edit() {
        let (_d, r) = repo();
        std::fs::write(r.join("src/lib.rs"), "// the agent's longer code\n").unwrap();
        // An old mtime, so the entry is not "racily clean" (newer than the
        // index), which would make git re-read the file.
        set_mtime(&r.join("src/lib.rs"), old_mtime());
        forge_index_stat(&r, "src/lib.rs");
        // The control: git's own status is fooled by the forged cache.
        let st = run(
            &r,
            &["status", "--porcelain=v1", "-z", "--untracked-files=all"],
        )
        .unwrap();
        assert!(
            st.is_empty(),
            "control: the forged stat entry makes git's status read clean: {}",
            String::from_utf8_lossy(&st)
        );
        assert_dirty(
            &r,
            "an index entry forged to match the modified file's stat data hid the edit",
            "bytes are not HEAD's",
        );
    }

    #[test]
    fn a_filter_driver_in_the_repository_config_never_runs() {
        let (d, r) = repo();
        std::fs::write(r.join(".gitattributes"), "src/lib.rs filter=evil\n").unwrap();
        git(&r, &["add", "-A"]);
        git(&r, &["commit", "-q", "-m", "attributes"]);
        let marker = d.path().join("filter-ran");
        // The clean filter answers with the reviewed bytes, whatever the
        // file holds, and leaves a marker: code the repository chose.
        git(
            &r,
            &[
                "config",
                "filter.evil.clean",
                &format!("touch {}; printf '// reviewed\\n'", marker.display()),
            ],
        );
        // A same-size edit: the size alone does not tell git it changed, so
        // it cleans the file to compare (and the filter answers "reviewed").
        std::fs::write(r.join("src/lib.rs"), "// agentcod\n").unwrap();
        let p = provenance(&r);
        assert!(
            !marker.exists(),
            "ATTACK: build provenance ran the repository's filter driver as the builder"
        );
        assert!(!p.dirty.is_empty(), "the edit is dirty: {p:?}");
    }
}
