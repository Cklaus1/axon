//! Git read as DATA: the one implementation behind every answer Axon takes
//! from a repository about what a tree is (review FIELD-ORIGIN / PSV-7, C9
//! round 2). Readiness (is this tree the certified revision?), build
//! provenance (`fabric_revision`, `source_dirty`, via `build.rs`) and the
//! guest-image manifest (`axon_tree_dirty_at_build`, via the
//! `axon-provenance` helper) all use it. It depends on `std` only, so
//! `build.rs` and the helper can include it by path.
//!
//! The repository is controlled by whoever can write it, INCLUDING its own
//! `.git/config`. So:
//! * git is the operator-installed [`GIT_BIN`], the caller's environment is
//!   dropped, system and global config are not read, replace objects are off;
//! * the working tree is the directory asked about (`--work-tree`), never
//!   the one `core.worktree` names;
//! * git never fetches: a missing object is never lazily fetched from a
//!   promisor remote (which runs the repository's `core.sshCommand` as the
//!   caller), and every transport is disallowed;
//! * the repository's own config may hold only keys that cannot change what
//!   git reports or run code ([`refuse_config`]); anything else is refused,
//!   never interpreted;
//! * an object is believed only if its bytes hash to its name, and the
//!   working tree is compared with a commit by hashing the FILES, never
//!   through git's stat cache (the index is a repository file too).

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// The operator-installed git. Never resolved through the caller's PATH.
pub const GIT_BIN: &str = "/usr/bin/git";

/// A git invocation that answers about the real objects and files of the
/// repository whose working tree is `top`, and nothing else.
pub fn git_cmd(top: &Path) -> Command {
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
        // Never fetch: no lazy fetch of a missing object, and no transport.
        .env("GIT_NO_LAZY_FETCH", "1")
        .args(["-c", "protocol.allow=never"])
        .args(["-c", "core.fsmonitor=", "-c", "core.hooksPath=/dev/null"])
        .args([
            "-c",
            "core.untrackedCache=false",
            "-c",
            "advice.graftFileDeprecated=false",
        ])
        .args(["-c", "core.excludesFile=/dev/null"])
        .args(["-c", "core.attributesFile=/dev/null"])
        // Git's stat cache compares everything it can.
        .args(["-c", "core.checkStat=default", "-c", "core.trustCtime=true"])
        // Ownership is not what this trusts (every answer is re-verified or
        // refused); the repository may belong to another uid.
        .args(["-c", "safe.directory=*"])
        .arg("-C")
        .arg(top)
        .arg("--work-tree")
        .arg(top)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    c
}

/// `git args` in `top`: stdout, or why it failed.
pub fn run(top: &Path, args: &[&str]) -> Result<Vec<u8>, String> {
    let o = git_cmd(top)
        .args(args)
        .output()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    if !o.status.success() {
        return Err(format!("git {} failed", args.join(" ")));
    }
    Ok(o.stdout)
}

/// [`run`], as trimmed text.
pub fn text(top: &Path, args: &[&str]) -> Result<String, String> {
    Ok(String::from_utf8_lossy(&run(top, args)?).trim().to_string())
}

/// Is `key` (as `git config --list` prints it: section and name lowercased,
/// a subsection verbatim) one the repository may set? Only keys git does not
/// consult when answering these read-only questions, or that [`git_cmd`]
/// overrides on its command line, are. An unknown key is refused: a new git
/// setting that runs code or redirects a read must not be believed by
/// default.
fn allowed_key(key: &str) -> bool {
    let (section, rest) = key.split_once('.').unwrap_or((key, ""));
    let (sub, name) = match rest.rsplit_once('.') {
        Some((s, n)) => (Some(s), n),
        None => (None, rest),
    };
    match (section, sub, name) {
        (
            "core",
            None,
            "repositoryformatversion" | "filemode" | "bare" | "logallrefupdates" | "ignorecase"
            | "precomposeunicode" | "symlinks"
            // overridden by git_cmd:
            | "hookspath" | "fsmonitor" | "untrackedcache" | "excludesfile" | "attributesfile"
            | "checkstat" | "trustctime",
        ) => true,
        ("remote", Some(_), "url" | "pushurl" | "fetch" | "push" | "tagopt" | "prune") => true,
        ("branch", Some(_), "remote" | "merge" | "rebase" | "pushremote" | "description") => true,
        ("user", None, "name" | "email") => true,
        ("pull", None, "rebase" | "ff") | ("push", None, "default" | "autosetupremote") => true,
        // Read by git-lfs only, which runs only as a filter or a hook: a
        // filter must be configured (refused below) and hooks are disabled.
        ("lfs", _, _) => true,
        _ => false,
    }
}

/// Refuse a repository whose own configuration could change what git reports
/// about it or run code as the caller: `core.worktree`, `core.sshCommand`,
/// `extensions.*` (a partial clone), `remote.*.promisor`, `include.*`,
/// `filter.*`, `diff.*`… — anything not on [`allowed_key`]'s list. The config
/// is read with includes OFF, so an include is itself a refused key.
pub fn refuse_config(top: &Path) -> Result<(), String> {
    // Located WITHOUT asking git (git reads this config to answer anything):
    // `.git` is the git dir, or a gitfile naming it; a linked worktree's git
    // dir names the common dir in `commondir`.
    let dotgit = top.join(".git");
    let gitdir = match std::fs::symlink_metadata(&dotgit) {
        Ok(m) if m.is_dir() => dotgit,
        Ok(m) if m.is_file() => {
            let f = std::fs::read_to_string(&dotgit)
                .map_err(|e| format!("{}: {e}", dotgit.display()))?;
            let g = f
                .strip_prefix("gitdir: ")
                .map(str::trim_end)
                .ok_or(format!("{} is not a gitfile", dotgit.display()))?;
            top.join(g)
        }
        _ => return Err(format!("{} is not a git directory", dotgit.display())),
    };
    let common = match std::fs::read_to_string(gitdir.join("commondir")) {
        Ok(c) => gitdir.join(c.trim_end()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => gitdir.clone(),
        Err(e) => return Err(format!("{}: {e}", gitdir.join("commondir").display())),
    };
    let wt = gitdir.join("config.worktree");
    if std::fs::symlink_metadata(&wt).is_ok() {
        return Err(format!(
            "{} exists: per-worktree configuration is not read as data",
            wt.display()
        ));
    }
    let cfg = common.join("config");
    let path = cfg
        .to_str()
        .ok_or("the repository config path is not UTF-8")?;
    // Parsed by git OUTSIDE any repository (`-C /`, GIT_DIR unset, no
    // system/global config), so nothing in it takes effect while it is read.
    let o = Command::new(GIT_BIN)
        .env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LC_ALL", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CEILING_DIRECTORIES", "/")
        .args([
            "-C",
            "/",
            "config",
            "--file",
            path,
            "--no-includes",
            "--list",
            "-z",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| format!("{GIT_BIN}: {e}"))?;
    if !o.status.success() {
        return Err(format!("cannot read {path}: refused, never interpreted"));
    }
    let listed = o.stdout;
    for entry in listed.split(|b| *b == 0).filter(|e| !e.is_empty()) {
        let key = String::from_utf8_lossy(entry.split(|b| *b == b'\n').next().unwrap_or(entry))
            .to_string();
        if !allowed_key(&key) {
            return Err(format!(
                "the repository's own config sets {key} ({path}): repository-local git settings \
                 that can redirect what git reports or run code are refused, never interpreted"
            ));
        }
    }
    Ok(())
}

/// SHA-1, only to check that an object's bytes hash to its git name. Not
/// used for anything else.
pub fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let mut m = data.to_vec();
    m.push(0x80);
    while m.len() % 64 != 56 {
        m.push(0);
    }
    m.extend_from_slice(&((data.len() as u64).wrapping_mul(8)).to_be_bytes());
    for chunk in m.chunks(64) {
        let mut w = [0u32; 80];
        for (i, b) in chunk.chunks(4).enumerate() {
            w[i] = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for (i, wi) in w.iter().enumerate() {
            let (f, k) = match i {
                0..=19 => ((b & c) | (!b & d), 0x5A827999),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDC),
                _ => (b ^ c ^ d, 0xCA62C1D6),
            };
            let t = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(*wi);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = t;
        }
        for (x, y) in h.iter_mut().zip([a, b, c, d, e]) {
            *x = x.wrapping_add(y);
        }
    }
    let mut out = [0u8; 20];
    for (i, x) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&x.to_be_bytes());
    }
    out
}

pub fn hex20(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// The git object id of `body` stored as an object of type `ty`.
pub fn object_id(ty: &str, body: &[u8]) -> String {
    let mut m = format!("{ty} {}\0", body.len()).into_bytes();
    m.extend_from_slice(body);
    hex20(&sha1(&m))
}

/// A tree's non-tree entries: path → (mode, object id).
pub type Entries = std::collections::BTreeMap<Vec<u8>, (u32, String)>;

/// The repository's object store, read through `git cat-file --batch`, where
/// an object is accepted only if its bytes hash to the name asked for. Git
/// does not check that for a tree it reads for `diff`, so a forged loose
/// object under a certified name would otherwise be believed. A MISSING
/// object is refused, never fetched ([`git_cmd`]).
pub struct Objects {
    child: std::process::Child,
    out: std::io::BufReader<std::process::ChildStdout>,
}

impl Objects {
    pub fn open(top: &Path) -> Result<Objects, String> {
        let mut child = git_cmd(top)
            .args(["cat-file", "--batch"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| format!("{GIT_BIN}: {e}"))?;
        let out = std::io::BufReader::new(child.stdout.take().ok_or("git cat-file: no stdout")?);
        Ok(Objects { child, out })
    }

    pub fn read(&mut self, oid: &str, want: &str) -> Result<Vec<u8>, String> {
        use std::io::{BufRead, Read, Write};
        let io = |e: std::io::Error| format!("git cat-file: {e}");
        let stdin = self.child.stdin.as_mut().ok_or("git cat-file: no stdin")?;
        writeln!(stdin, "{oid}")
            .and_then(|()| stdin.flush())
            .map_err(io)?;
        let mut header = String::new();
        self.out.read_line(&mut header).map_err(io)?;
        let f: Vec<&str> = header.trim_end().split(' ').collect();
        let size = match f.as_slice() {
            [o, t, n] if *o == oid && *t == want => n.parse::<usize>().ok(),
            _ => None,
        }
        .filter(|n| *n <= 1 << 30)
        .ok_or(format!(
            "object {oid} is not a {want} in this repository's object store (a missing object \
             is refused, never fetched)"
        ))?;
        let mut body = vec![0u8; size + 1];
        self.out.read_exact(&mut body).map_err(io)?;
        body.pop();
        if object_id(want, &body) != oid {
            return Err(format!(
                "object {oid} does not hash to its name: the object store is forged or damaged, \
                 and nothing read from it is certified"
            ));
        }
        Ok(body)
    }

    /// Every non-tree entry of `commit`'s tree, as path → (mode, object id),
    /// each tree on the way verified by hash. A top-level directory named
    /// `skip` is left out.
    pub fn entries(&mut self, commit: &str, skip: Option<&[u8]>) -> Result<Entries, String> {
        let c = self.read(commit, "commit")?;
        let mut stack = vec![(
            Vec::new(),
            c.strip_prefix(b"tree ")
                .and_then(|r| r.get(..40))
                .map(|t| String::from_utf8_lossy(t).to_string())
                .ok_or(format!("commit {commit} names no tree"))?,
        )];
        let skipped = |p: &[u8]| {
            skip.is_some_and(|s| {
                p == s || (p.len() > s.len() && p.starts_with(s) && p[s.len()] == b'/')
            })
        };
        let mut out = Entries::new();
        while let Some((prefix, tree)) = stack.pop() {
            let b = self.read(&tree, "tree")?;
            let mut i = 0;
            while i < b.len() {
                let bad = || format!("tree {tree} is malformed");
                let sp = i + b[i..].iter().position(|&x| x == b' ').ok_or_else(bad)?;
                let nul = sp + b[sp..].iter().position(|&x| x == 0).ok_or_else(bad)?;
                let id = b.get(nul + 1..nul + 21).ok_or_else(bad)?;
                let mode = std::str::from_utf8(&b[i..sp])
                    .ok()
                    .and_then(|m| u32::from_str_radix(m, 8).ok())
                    .ok_or_else(bad)?;
                let mut path = prefix.clone();
                path.extend_from_slice(&b[sp + 1..nul]);
                i = nul + 21;
                if mode == 0o40000 {
                    if !skipped(&path) {
                        path.push(b'/');
                        stack.push((path, hex20(id)));
                    }
                } else if !skipped(&path) {
                    out.insert(path, (mode, hex20(id)));
                }
            }
        }
        Ok(out)
    }
}

impl Drop for Objects {
    fn drop(&mut self) {
        drop(self.child.stdin.take());
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// The first path in `tree` whose working-tree bytes (and kind, and
/// executable bit) under `top` are not that object, hashed here from the
/// file itself: neither the index nor git's view of the working tree is
/// consulted. A submodule (gitlink) cannot be verified and always differs.
#[cfg(unix)]
pub fn worktree_differs(top: &Path, tree: &Entries) -> Option<String> {
    use std::os::unix::ffi::{OsStrExt, OsStringExt};
    use std::os::unix::fs::PermissionsExt;
    tree.iter()
        .find(|(path, (mode, oid))| {
            let p = top.join(std::ffi::OsStr::from_bytes(path));
            let Ok(md) = std::fs::symlink_metadata(&p) else {
                return true;
            };
            let body = match mode {
                0o120000 if md.file_type().is_symlink() => std::fs::read_link(&p)
                    .ok()
                    .map(|t| t.into_os_string().into_vec()),
                0o100644 | 0o100755
                    if md.is_file()
                        && (md.permissions().mode() & 0o100 != 0) == (*mode == 0o100755) =>
                {
                    std::fs::read(&p).ok()
                }
                _ => None,
            };
            body.is_none_or(|b| object_id("blob", &b) != *oid)
        })
        .map(|(path, _)| String::from_utf8_lossy(path).to_string())
}

/// The nearest ancestor of `dir` holding a `.git` of any kind, and what it is.
fn find_dotgit(dir: &Path) -> Result<(PathBuf, std::fs::Metadata), String> {
    let start = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut d = start.as_path();
    loop {
        if let Ok(m) = std::fs::symlink_metadata(d.join(".git")) {
            return Ok((d.to_path_buf(), m));
        }
        d = d
            .parent()
            .ok_or_else(|| format!("{} is not in a git working tree", start.display()))?;
    }
}

/// The working tree containing `dir`, for a claim that it IS a clean commit
/// (build provenance): the nearest ancestor holding `.git`, which must be a
/// real directory. A `.git` FILE (a linked worktree or a submodule) or
/// symlink points git at a repository chosen elsewhere, and is refused: a
/// verifier or guest image is built from a plain clone.
pub fn discover(dir: &Path) -> Result<PathBuf, String> {
    let (top, m) = find_dotgit(dir)?;
    match m {
        m if m.is_dir() => Ok(top),
        _ => Err(format!(
            "{} is not a directory (a gitfile or symlink names a repository elsewhere): \
             refused, build from a plain clone",
            top.join(".git").display()
        )),
    }
}

/// [`discover`], also accepting a linked worktree's gitfile (never a
/// symlink): for a question about history only (the guest build's lineage
/// check), where it does not matter which clone asks.
pub fn discover_linked(dir: &Path) -> Result<PathBuf, String> {
    let (top, m) = find_dotgit(dir)?;
    if m.is_dir() || m.is_file() {
        Ok(top)
    } else {
        Err(format!(
            "{} is a symlink: refused",
            top.join(".git").display()
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn git(repo: &Path, args: &[&str]) {
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

    #[test]
    fn object_ids_are_git_object_ids() {
        assert_eq!(
            hex20(&sha1(b"abc")),
            "a9993e364706816aba3e25717850c26c9cd0d89d"
        );
        assert_eq!(
            hex20(&sha1(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
        let long = vec![b'a'; 1000];
        assert_eq!(
            hex20(&sha1(&long)),
            "291e9a6c66994949b57ba5e650361e98fc36b1ba"
        );
        // `git hash-object` of "hello\n" and of an empty tree.
        assert_eq!(
            object_id("blob", b"hello\n"),
            "ce013625030ba8dba906f756967f9e9ca394464a"
        );
        assert_eq!(
            object_id("tree", b""),
            "4b825dc642cb6eb9a060e54bf8d69288fbee4904"
        );
    }

    /// A committed repository configured as a partial clone whose promisor
    /// is reached through the repository's own `core.sshCommand`, which
    /// leaves a marker if it ever runs. Returns (tempdir, repo, marker, a
    /// missing object id).
    pub(crate) fn promisor_repo() -> (tempfile::TempDir, PathBuf, PathBuf, String) {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("repo");
        std::fs::create_dir_all(&r).unwrap();
        git(&r, &["init", "-q", "-b", "main"]);
        std::fs::write(r.join("a.txt"), "a\n").unwrap();
        git(&r, &["add", "-A"]);
        git(&r, &["commit", "-q", "-m", "c"]);
        let marker = d.path().join("ssh-command-ran");
        arm_promisor(&r, &marker);
        (d, r, marker, "1".repeat(40))
    }

    /// Make `r` a promisor-remote partial clone whose ssh command touches
    /// `marker`.
    pub(crate) fn arm_promisor(r: &Path, marker: &Path) {
        for (k, v) in [
            ("core.repositoryformatversion", "1".to_string()),
            ("extensions.partialClone", "origin".to_string()),
            (
                "remote.origin.url",
                "ssh://attacker.invalid/r.git".to_string(),
            ),
            ("remote.origin.promisor", "true".to_string()),
            ("remote.origin.partialclonefilter", "blob:none".to_string()),
            (
                "core.sshCommand",
                format!("touch {}; false", marker.display()),
            ),
        ] {
            git(r, &["config", k, &v]);
        }
    }

    #[test]
    fn a_git_call_on_a_promisor_repository_fetches_nothing() {
        let (_d, r, marker, missing) = promisor_repo();
        // The control: git as the repository configures it (only system and
        // global config cleared, as readiness used to run it) fetches the
        // missing object, running the repository's ssh command.
        let _ = Command::new(GIT_BIN)
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .args(["-C"])
            .arg(&r)
            .args(["cat-file", "-e", &missing])
            .stderr(Stdio::null())
            .status();
        assert!(
            marker.exists(),
            "control: an unhardened git lazily fetches through core.sshCommand"
        );
        std::fs::remove_file(&marker).unwrap();
        let st = git_cmd(&r)
            .args(["cat-file", "-e", &missing])
            .status()
            .unwrap();
        assert!(
            !marker.exists(),
            "ATTACK: git run as the verifier lazily fetched a missing object and ran the \
             repository's core.sshCommand"
        );
        assert!(!st.success(), "the missing object stays missing");
    }

    #[test]
    fn repository_config_that_redirects_or_runs_code_is_refused() {
        for (k, v) in [
            ("core.worktree", "/elsewhere"),
            ("core.sshCommand", "true"),
            ("extensions.partialClone", "origin"),
            ("remote.origin.promisor", "true"),
            ("include.path", "/tmp/x"),
            ("filter.evil.clean", "cat"),
            ("diff.evil.textconv", "cat"),
            ("core.gitProxy", "x"),
        ] {
            let d = tempfile::tempdir().unwrap();
            let r = d.path().join("repo");
            std::fs::create_dir_all(&r).unwrap();
            git(&r, &["init", "-q", "-b", "main"]);
            assert_eq!(refuse_config(&r), Ok(()), "control: a fresh repository");
            git(&r, &["config", k, v]);
            let e = refuse_config(&r).expect_err(&format!(
                "ATTACK: the repository's own config sets {k}, and it was read as data"
            ));
            assert!(e.contains(&k.to_lowercase()) || e.contains(k), "{e}");
        }
        // What a normal clone carries is accepted.
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("repo");
        std::fs::create_dir_all(&r).unwrap();
        git(&r, &["init", "-q", "-b", "main"]);
        for (k, v) in [
            ("remote.origin.url", "https://example.invalid/r.git"),
            ("remote.origin.fetch", "+refs/heads/*:refs/remotes/origin/*"),
            ("branch.main.remote", "origin"),
            ("branch.main.merge", "refs/heads/main"),
            ("core.hooksPath", "/somewhere/hooks"),
            ("lfs.https://example.invalid/r.git/info/lfs.access", "basic"),
        ] {
            git(&r, &["config", k, v]);
        }
        assert_eq!(refuse_config(&r), Ok(()));
    }

    #[test]
    fn a_gitfile_or_symlinked_git_dir_is_refused() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path().join("repo");
        std::fs::create_dir_all(r.join("src")).unwrap();
        git(&r, &["init", "-q", "-b", "main"]);
        assert_eq!(discover(&r.join("src")).unwrap(), r.canonicalize().unwrap());
        let w = d.path().join("wt");
        std::fs::create_dir_all(&w).unwrap();
        std::fs::write(
            w.join(".git"),
            format!("gitdir: {}\n", r.join(".git").display()),
        )
        .unwrap();
        let e = discover(&w).expect_err(
            "ATTACK: a gitfile naming a repository elsewhere was accepted as the build's tree",
        );
        assert!(e.contains("gitfile"), "{e}");
    }
}
