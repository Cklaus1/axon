//! Amendment 103 (C9 round 11, eqgate6): every VALUE `git_data::git_cmd` hands git is
//! asserted, not only that the keys exist. Round 10 (EQUIVALENCE) changed single values of
//! this builder (`GIT_TERMINAL_PROMPT`, the locale, the system/global config switches, the
//! untracked-cache setting, the stdio) and every suite stayed green, because the rows that
//! credit the builder removed a whole group of lines and no test read what the builder built.
//! The environment and the arguments are read from the `Command` itself; the two streams
//! (stdin, stderr) are read from the descriptors git's own child process holds, in a test binary
//! of its own so that moving this process's fds 0 and 2 disturbs no other test.

use axon_fabric::git_data::{git_cmd, GIT_BIN};
use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::process::{Command, Stdio};

fn built(top: &Path) -> Command {
    git_cmd(top)
}

#[test]
fn git_is_run_with_exactly_this_environment() {
    let top = Path::new("/tmp/a-repository");
    let c = built(top);
    assert_eq!(
        GIT_BIN, "/usr/bin/git",
        "ATTACK: the operator-installed git is not /usr/bin/git"
    );
    assert_eq!(
        c.get_program(),
        "/usr/bin/git",
        "ATTACK: git_cmd runs a git other than the operator's /usr/bin/git"
    );
    let envs: BTreeMap<String, Option<String>> = c
        .get_envs()
        .map(|(k, v)| {
            (
                k.to_string_lossy().into_owned(),
                v.map(|v| v.to_string_lossy().into_owned()),
            )
        })
        .collect();
    for (key, want) in [
        ("PATH", "/usr/bin:/bin"),
        ("LC_ALL", "C"),
        ("GIT_NO_REPLACE_OBJECTS", "1"),
        ("GIT_CONFIG_NOSYSTEM", "1"),
        ("GIT_CONFIG_GLOBAL", "/dev/null"),
        ("GIT_OPTIONAL_LOCKS", "0"),
        ("GIT_TERMINAL_PROMPT", "0"),
        ("GIT_NO_LAZY_FETCH", "1"),
    ] {
        assert_eq!(
            envs.get(key).and_then(|v| v.as_deref()),
            Some(want),
            "ATTACK: git_cmd env {key} is not {want:?}: {envs:?}"
        );
    }
    assert_eq!(
        envs.len(),
        8,
        "ATTACK: git_cmd sets an environment variable the amendment-103 assertions do not name: {envs:?}"
    );
}

#[test]
fn git_is_run_with_exactly_these_arguments() {
    let top = Path::new("/tmp/a-repository");
    let args: Vec<OsString> = built(top).get_args().map(|a| a.to_os_string()).collect();
    let want: Vec<OsString> = [
        "--no-replace-objects",
        "-c",
        "protocol.allow=never",
        "-c",
        "core.fsmonitor=",
        "-c",
        "core.hooksPath=/dev/null",
        "-c",
        "core.untrackedCache=false",
        "-c",
        "advice.graftFileDeprecated=false",
        "-c",
        "core.excludesFile=/dev/null",
        "-c",
        "core.attributesFile=/dev/null",
        "-c",
        "core.checkStat=default",
        "-c",
        "core.trustCtime=true",
        "-c",
        "safe.directory=*",
        "-C",
        "/tmp/a-repository",
        "--work-tree",
        "/tmp/a-repository",
    ]
    .into_iter()
    .map(OsString::from)
    .collect();
    assert_eq!(args.len(), want.len(), "ATTACK: git_cmd's argument count changed: {args:?}");
    for (i, (got, want)) in args.iter().zip(&want).enumerate() {
        assert_eq!(
            got, want,
            "ATTACK: git_cmd argument {i} is {got:?}, not {want:?}: {args:?}"
        );
    }
}

/// The streams: git's stdin is /dev/null and its stderr is /dev/null, whatever this process's
/// own are. The test points its own fds 0 and 2 at pipes, then has git run a shell alias that
/// lists the descriptors it inherited.
#[test]
fn git_inherits_neither_stdin_nor_stderr() {
    let d = tempfile::tempdir().unwrap();
    let repo = d.path().join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let ok = Command::new(GIT_BIN)
        .arg("-C")
        .arg(&repo)
        .args(["init", "-q"])
        .stdout(Stdio::null())
        .status()
        .unwrap()
        .success();
    assert!(ok, "setup: git init");
    let mut fds = [0i32; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "setup: pipe");
    let (saved_in, saved_err) = unsafe { (libc::dup(0), libc::dup(2)) };
    unsafe {
        libc::dup2(fds[0], 0);
        libc::dup2(fds[1], 2);
    }
    let out = git_cmd(&repo.canonicalize().unwrap())
        .args(["-c", "alias.fds=!ls -l /proc/self/fd/0 /proc/self/fd/2", "fds"])
        .output();
    unsafe {
        libc::dup2(saved_in, 0);
        libc::dup2(saved_err, 2);
        libc::close(saved_in);
        libc::close(saved_err);
        libc::close(fds[0]);
        libc::close(fds[1]);
    }
    let out = out.expect("setup: git runs the alias");
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(out.status.success(), "setup: the alias ran: {text} / {:?}", out.status);
    let line = |n: u32| {
        text.lines()
            .find(|l| l.contains(&format!("/proc/self/fd/{n} ->")))
            .unwrap_or_else(|| panic!("setup: no line for fd {n}: {text}"))
            .to_string()
    };
    assert!(
        line(0).ends_with("-> /dev/null"),
        "ATTACK: git inherits this process's stdin: {}",
        line(0)
    );
    assert!(
        line(2).ends_with("-> /dev/null"),
        "ATTACK: git inherits this process's stderr: {}",
        line(2)
    );
}
