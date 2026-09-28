//! Build provenance for `axon-fabric` (governance/specs/v022-protected-suite-verdict.md):
//! the installed readiness verifier reports WHAT it is — its source revision,
//! whether that tree was dirty, the compiler and profile — so a certification
//! can bind it and replacing the verifier is not a silent change of authority.
//! Falls back to "unknown" without git; a production verifier must be clean.

use std::process::Command;

fn out(cmd: &str, args: &[&str]) -> Option<String> {
    let o = Command::new(cmd).args(args).output().ok()?;
    o.status
        .success()
        .then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
}

fn main() {
    let sha = out("git", &["rev-parse", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = out("git", &["status", "--porcelain", "--untracked-files=no"])
        .map(|s| !s.is_empty())
        .unwrap_or(true);
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let rustc_v = out(&rustc, &["-V"]).unwrap_or_else(|| "unknown".into());
    println!("cargo:rustc-env=AXON_FABRIC_GIT_SHA={sha}");
    println!("cargo:rustc-env=AXON_FABRIC_GIT_DIRTY={dirty}");
    println!("cargo:rustc-env=AXON_FABRIC_RUSTC={rustc_v}");
    println!(
        "cargo:rustc-env=AXON_FABRIC_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
    println!(
        "cargo:rustc-env=AXON_FABRIC_TARGET={}",
        std::env::var("TARGET").unwrap_or_default()
    );
    if let Some(gd) = out("git", &["rev-parse", "--git-dir"]) {
        println!("cargo:rerun-if-changed={gd}/HEAD");
        println!("cargo:rerun-if-changed={gd}/index");
    }
    println!("cargo:rerun-if-changed=src");
}
