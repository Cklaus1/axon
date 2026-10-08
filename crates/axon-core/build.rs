//! Build script: capture the git short SHA (+ dirty flag) at compile time so
//! `axon --version` can report a reproducible build identity (BUG_HUNT #30).
//!
//! Emits `AXON_GIT_SHA`, consumed via `env!` in `main.rs`. Falls back to
//! "unknown" when git is unavailable (e.g. a source tarball) — the build must
//! never fail just because there's no `.git`.
//!
//! AX-50. A source copy (`git archive | tar -x`) placed inside ANOTHER git
//! repository used to report that outer repo's commit, usually `-dirty`: git
//! walks up to the nearest `.git`. So git is only trusted when its toplevel IS
//! this workspace's root; otherwise the identity is "unknown". A packager that
//! knows the real identity sets `AXON_GIT_SHA` in the build environment and it
//! is embedded verbatim.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-env-changed=AXON_GIT_SHA");
    let sha = match std::env::var("AXON_GIT_SHA") {
        Ok(s) if !s.trim().is_empty() => s.trim().to_string(),
        _ => git_describe(),
    };
    println!("cargo:rustc-env=AXON_GIT_SHA={sha}");

    // Re-run when HEAD moves or the index changes, so the embedded SHA stays
    // current without a manual `touch`. Best-effort: these paths may not exist
    // in a tarball, and a missing rerun-if path is simply ignored by cargo.
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");

    // AUDIT T38. Editing a tracked source file makes the working tree DIRTY
    // without touching .git/index (that only moves on `git add`), so with only
    // the two paths above this script never re-ran and `AXON_GIT_SHA` kept
    // reporting the last CLEAN sha. `axon --version` then claimed a build
    // identity it did not have — and, far worse, `axon build`'s incremental
    // cache is keyed on that string, so a recompiled compiler with different
    // codegen served the PREVIOUS compiler's cached object. A real codegen fix
    // silently did not take effect until the .ax source happened to change.
    // Watching src/ costs two `git` invocations per source edit.
    println!("cargo:rerun-if-changed=src");
}

/// `<short-sha>` or `<short-sha>-dirty`, or `unknown` if git isn't available
/// or the enclosing repository is not this workspace (AX-50).
fn git_describe() -> String {
    if !git_owns_workspace() {
        return "unknown".to_string();
    }
    let sha = run_git(&["rev-parse", "--short", "HEAD"]);
    match sha {
        Some(s) if !s.is_empty() => {
            if is_dirty() {
                format!("{s}-dirty")
            } else {
                s
            }
        }
        _ => "unknown".to_string(),
    }
}

/// True if git's toplevel (run from the crate dir) is the workspace root, i.e.
/// `CARGO_MANIFEST_DIR/../..`. Both sides are canonicalised so symlinked
/// checkouts still match.
fn git_owns_workspace() -> bool {
    let Some(manifest) = std::env::var_os("CARGO_MANIFEST_DIR") else {
        return false;
    };
    let Ok(root) = Path::new(&manifest).join("../..").canonicalize() else {
        return false;
    };
    let Some(top) = run_git(&["rev-parse", "--show-toplevel"]) else {
        return false;
    };
    PathBuf::from(top).canonicalize().is_ok_and(|t| t == root)
}

/// True if the working tree has uncommitted changes. Conservative: any error
/// (no git, not a repo) reports "not dirty" so we never falsely flag a clean
/// tarball build.
fn is_dirty() -> bool {
    run_git(&["status", "--porcelain"])
        .map(|s| !s.is_empty())
        .unwrap_or(false)
}

/// Run `git <args>` and return trimmed stdout, or None on any failure.
fn run_git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}
