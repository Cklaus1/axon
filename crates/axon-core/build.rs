//! Build script: capture the git short SHA (+ dirty flag) at compile time so
//! `axon --version` can report a reproducible build identity (BUG_HUNT #30).
//!
//! Emits `AXON_GIT_SHA`, consumed via `env!` in `main.rs`. Falls back to
//! "unknown" when git is unavailable (e.g. a source tarball) — the build must
//! never fail just because there's no `.git`.

use std::process::Command;

fn main() {
    println!("cargo:rustc-env=AXON_GIT_SHA={}", git_describe());

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

/// `<short-sha>` or `<short-sha>-dirty`, or `unknown` if git isn't available.
fn git_describe() -> String {
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
