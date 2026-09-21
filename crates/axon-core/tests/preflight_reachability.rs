//! No runner may execute a program without passing the shared preflight.
//!
//! `axon-run` went straight from `parse_source` to `interp::run_program`: no
//! type check, no capability check, no record/replay, no audit ledger. A
//! program annotated `@[contained(fs: [read("./out/")], net: [], exec: none)]`
//! whose body reads `/etc/passwd` was refused by `axon run` with E1001 and
//! exit 2, and leaked 1657 bytes under `axon-run` with exit 0.
//!
//! It was also the interpreter side of the wasm and codegen parity harnesses,
//! so those compared codegen against an interpreter that enforced nothing —
//! an execution-authority bypass AND a corrupted test oracle.
//!
//! Adding the three missing calls to `axon-run` would have fixed `axon-run`.
//! This guard is what stops the FOURTH runner being written without them.

use std::path::{Path, PathBuf};

fn rs_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                let n = p
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                if n != "target" && !n.starts_with('.') {
                    stack.push(p);
                }
            } else if p.extension().and_then(|x| x.to_str()) == Some("rs") {
                out.push(p);
            }
        }
    }
    out
}

/// Any file that EXECUTES a program must also reach the preflight.
///
/// Scoped to binaries and their modules. The interpreter's own internals call
/// `run_program` recursively and are not runners; they are excluded by name,
/// and the exclusion list is short and explicit so it cannot quietly grow.
#[test]
fn every_runner_reaches_the_shared_preflight() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .to_path_buf();

    // Files that legitimately call run_program without preflighting: the
    // interpreter itself, and the CLI, which runs the richer located pipeline
    // (run_check_pipeline_located + lockfile + cert gate) before executing.
    const EXEMPT: &[&str] = &[
        "axon-core/src/interp.rs",
        "axon-core/src/interp/",
        "axon-core/src/main.rs",
        "axon-core/src/lib.rs",
        "axon-wasm/src/lib.rs",
    ];

    let mut offenders = Vec::new();
    let mut checked = 0usize;
    for p in rs_files(&root) {
        let rel = p
            .strip_prefix(&root)
            .unwrap_or(&p)
            .to_string_lossy()
            .to_string();
        if EXEMPT.iter().any(|e| rel.contains(e)) {
            continue;
        }
        let Ok(src) = std::fs::read_to_string(&p) else {
            continue;
        };
        // Match the CANONICAL entry, not a bare name. `run_program(` alone also
        // matched `axon-guest-kernel`'s own `run_program(policy: &Policy)` — a
        // bare-metal kernel function with nothing to do with the interpreter.
        // A guard that cries wolf on unrelated code is one people learn to
        // silence.
        if !src.contains("interp::run_program(") {
            continue;
        }
        // A test fixture that drives the interpreter is not a runner.
        if src.contains("#[cfg(test)]") && !rel.contains("/bin/") {
            continue;
        }
        checked += 1;
        if !src.contains("preflight::prepare") {
            offenders.push(rel);
        }
    }

    // Guard the guard: if nothing matched, this test would pass while
    // asserting nothing about anything.
    assert!(
        checked >= 1,
        "no runner was examined — this test's premise is gone, not satisfied"
    );
    assert!(
        offenders.is_empty(),
        "these runners execute a program WITHOUT the shared preflight, so they \
         enforce no capability policy and install no record/replay/audit. Call \
         axon_core::preflight::prepare() — do not re-implement the checks:\n{}",
        offenders.join("\n")
    );
}
