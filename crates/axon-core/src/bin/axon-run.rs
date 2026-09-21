//! `axon-run` — the codegen-free runner.
//!
//! Parses a `.ax` file and executes its `main` via the tree-walking
//! interpreter ([`axon_core::interp`]). Unlike the full `axon` binary this
//! target does NOT require the `codegen`/`inkwell` feature, so it builds in
//! seconds. Build it explicitly with:
//!
//! ```text
//! cargo build -p axon-core --no-default-features --bin axon-run
//! ```
//!
//! Usage: `axon-run <file.ax>`  (also accepts `axon-run run <file.ax>`).

use std::path::PathBuf;
use std::process::exit;

fn main() {
    let mut args = std::env::args().skip(1).peekable();

    // Accept an optional leading `run` subcommand for symmetry with `axon run`.
    if args.peek().map(|s| s == "run").unwrap_or(false) {
        args.next();
    }

    let Some(file) = args.next() else {
        eprintln!("usage: axon-run <file.ax>");
        exit(2);
    };

    let path = PathBuf::from(&file);
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("axon-run: cannot read {}: {e}", path.display());
            exit(2);
        }
    };

    // THE SHARED PREFLIGHT. This binary previously went straight from
    // `parse_source` to `run_program`, performing NO type check, NO capability
    // check, and NO record/replay/audit installation — so a program annotated
    // `@[contained(fs: [read("./out/")])]` that reads /etc/passwd, which
    // `axon run` refuses with E1001 and exit 2, executed here and leaked 1657
    // bytes with exit 0.
    //
    // The checks are NOT re-implemented here. There is one preflight and every
    // runner calls it; `tests/preflight_reachability.rs` fails if a binary
    // reaches `interp::run_program` without it.
    let ready = match axon_core::preflight::prepare(&src, &path.display().to_string()) {
        Ok(r) => r,
        Err(code) => exit(code),
    };

    let code = axon_core::interp::run_program(&ready.program);

    // A diverged replay exits 11 regardless of what the program returned, so a
    // diverged run cannot be made to look clean — the same guard `axon run`
    // applies, decided from state the program cannot reach.
    if let Some(d) = axon_core::replay::finish() {
        if !d.already_reported {
            eprintln!("axon-run: replay divergence: {}", d.report);
        }
        exit(axon_core::replay::REPLAY_DIVERGENCE_EXIT_CODE);
    }
    if ready.replay_mode == axon_core::replay::Mode::Recording {
        eprintln!(
            "axon-run: recorded host journal to {}",
            std::env::var(axon_core::replay::RECORD_ENV_VAR).unwrap_or_default()
        );
    }
    exit(code);
}
