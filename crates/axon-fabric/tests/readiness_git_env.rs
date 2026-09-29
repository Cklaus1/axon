//! The caller's environment must not steer the readiness verdict. The
//! operator's `scripts/protected_verifier_ready.py` passes its own environment
//! through, and git obeys PATH (which `git` runs) and GIT_DIR / GIT_WORK_TREE
//! (which repository it answers about). This is its own test binary, with ONE
//! test, because it changes the process environment.

mod common;
mod readiness_fixture;
use readiness_fixture::*;

use serde_json::Value;
use std::os::unix::fs::PermissionsExt;

fn under(vars: &[(&str, String)], c: &Certified) -> Value {
    let saved: Vec<_> = vars
        .iter()
        .map(|(k, _)| (*k, std::env::var_os(k)))
        .collect();
    for (k, v) in vars {
        std::env::set_var(k, v);
    }
    let v = c.verdict();
    for (k, old) in saved {
        match old {
            Some(o) => std::env::set_var(k, o),
            None => std::env::remove_var(k),
        }
    }
    v
}

fn refused(v: &Value, why: &str) -> bool {
    v["status"] != "PASS" && v.to_string().contains(why)
}

#[test]
fn the_callers_path_and_git_environment_do_not_steer_the_verdict() {
    let Some(c) = certified() else { return };
    let scratch = c._d.path();
    // An honest clone, taken while the tree is still the certified one.
    let honest = scratch.join("honest");
    git(&c.repo, &["clone", "-q", ".", honest.to_str().unwrap()]);
    // The agent's code is committed, and the working tree put back to the
    // certified bytes: only git's answer about HEAD tells the two apart.
    c.commit_changed_code();
    write(&c.repo.join(CODE), "// code\n");
    let why = "changed since the certified revision";
    c.refused(why);

    // A `git` earlier on PATH that says yes to everything.
    let bin = scratch.join("fakebin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(bin.join("git"), "#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(bin.join("git"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let by_path = under(&[("PATH", path)], &c);

    // GIT_DIR / GIT_WORK_TREE naming the honest clone.
    let by_git_dir = under(
        &[
            ("GIT_DIR", honest.join(".git").display().to_string()),
            ("GIT_WORK_TREE", honest.display().to_string()),
        ],
        &c,
    );
    // Both are evaluated before either is asserted, so a failure shows both.
    assert!(
        refused(&by_path, why) && refused(&by_git_dir, why),
        "expected {why:?} under each:\n  fake git on PATH: {by_path}\n  GIT_DIR/GIT_WORK_TREE: {by_git_dir}"
    );

    // Unchanged environment afterwards: still refused on the real tree.
    c.refused(why);
}
