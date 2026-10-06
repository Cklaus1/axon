//! Build provenance for `axon-fabric` (governance/specs/v022-protected-suite-verdict.md):
//! the installed readiness verifier reports WHAT it is — its source revision,
//! whether that tree was dirty, the compiler and profile — so a certification
//! can bind it and replacing the verifier is not a silent change of authority.
//!
//! `fabric_revision` / `source_dirty` come from `src/provenance.rs` over
//! `src/git_data.rs` (the operator's `/usr/bin/git`, environment dropped,
//! replace objects off, the repository's own config refused unless it is
//! inert, never fetching; dirty on any skip-worktree/assume-unchanged entry,
//! replace ref, graft, change, untracked file, file ignored by a rule outside
//! the tracked tree, or file whose bytes are not HEAD's). Without git the
//! revision is "unknown" and the tree is dirty; a production verifier must be
//! clean.
//!
//! "Clean" is the one source-tree rule (`git_data::tree_differs`, operator
//! decisions C and E, amendment 44): every filesystem object in the tree
//! counts, `.gitignore`d or not, unless the operator's root-owned
//! `/etc/axon/provenance-allowlist` excuses it; and the tree must be a
//! standalone clone, never a linked worktree. So a certified verifier is
//! built from a standalone clone, with the allowlist installed (for an
//! in-tree `target/`) or `CARGO_TARGET_DIR` outside the tree. The allowlist
//! is not watched below: a changed allowlist is seen at the next re-run.

#[path = "src/build_state.rs"]
#[allow(dead_code)]
mod build_state;
#[path = "src/git_data.rs"]
#[allow(dead_code)]
mod git_data;
#[path = "src/provenance.rs"]
#[allow(dead_code)]
mod provenance;

use std::process::Command;

fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    let dir = std::path::PathBuf::from(
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| ".".into()),
    );
    let p = provenance::provenance(&dir);
    // A release build is what an operator installs: say why it is dirty.
    if std::env::var("PROFILE").as_deref() == Ok("release") {
        for why in &p.dirty {
            println!("cargo:warning=axon-fabric source_dirty: {why}");
        }
    }
    // What stands between the sources and the bytes (round 5): recorded in the
    // identity, and a production build under any of it does not happen.
    let profile = std::env::var("PROFILE").unwrap_or_default();
    let bstate = build_state::state(&|n| std::env::var(n).ok());
    for n in build_state::WATCHED {
        println!("cargo:rerun-if-env-changed={n}");
    }
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_TEST_TRUST_ROOT");
    if let Some(why) = build_state::refusal(
        &bstate,
        &profile,
        std::env::var_os("CARGO_FEATURE_TEST_TRUST_ROOT").is_some(),
    ) {
        eprintln!("error: {why}");
        std::process::exit(1);
    }
    println!("cargo:rustc-env=AXON_FABRIC_BUILD_STATE={bstate}");
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let rustc_v = out(&rustc, &["-V"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=AXON_FABRIC_GIT_SHA={}", p.revision);
    println!(
        "cargo:rustc-env=AXON_FABRIC_GIT_DIRTY={}",
        !p.dirty.is_empty()
    );
    println!("cargo:rustc-env=AXON_FABRIC_RUSTC={rustc_v}");
    println!(
        "cargo:rustc-env=AXON_FABRIC_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=AXON_FABRIC_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    // Re-derive whenever anything the answer depends on may have changed: the
    // git state, and the whole working tree (a change in ANOTHER crate, or an
    // untracked file, makes the tree dirty without touching this crate; the
    // old rule watched only this crate's src/, so such a build kept a stale
    // `source_dirty: false`). Each top-level entry is watched recursively,
    // except `.git` (its state is watched above) and an in-tree cargo target
    // directory, which every build writes. Residual: a NEW top-level entry is
    // seen at the next re-run, not by itself.
    for w in &p.watch {
        println!("cargo:rerun-if-changed={}", w.display());
    }
    let out_dir = std::env::var_os("OUT_DIR").map(std::path::PathBuf::from);
    if let Ok(top) = provenance::toplevel(&dir) {
        if let Ok(rd) = std::fs::read_dir(&top) {
            for e in rd.flatten() {
                let p = e.path();
                let is_target = out_dir.as_ref().is_some_and(|o| o.starts_with(&p));
                if e.file_name() != ".git" && !is_target {
                    println!("cargo:rerun-if-changed={}", p.display());
                }
            }
        }
    }
    println!("cargo:rerun-if-changed=src");
}
