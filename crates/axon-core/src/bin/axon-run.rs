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

    let program = match axon_core::parse_source(&src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("axon-run: parse error in {}:\n{e}", path.display());
            exit(1);
        }
    };

    let code = axon_core::interp::run_program(&program);
    exit(code);
}
