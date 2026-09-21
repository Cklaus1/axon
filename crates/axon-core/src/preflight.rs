//! The one preflight every runner must pass before executing a program.
//!
//! WHY THIS EXISTS. `axon run` and the `axon-run` binary both interpret Axon
//! programs, and only one of them checked anything. REPRODUCED on a program
//! annotated `@[contained(fs: [read("./out/")], net: [], exec: none)]` whose
//! body reads `/etc/passwd`:
//!
//! ```text
//! axon run contained.ax   -> E1001 ... not permitted by @[contained]   exit 2
//! axon-run contained.ax   -> LEAKED 1657 bytes                         exit 0
//! ```
//!
//! `axon-run`'s whole body was `parse_source` then `interp::run_program`: no
//! type checking, no capability checking, no record/replay installation, no
//! audit ledger. `AXON_ALLOWED_EFFECTS` *was* honoured, because it is read
//! inside the interpreter — and that is what made the rest look fine. One
//! ambient control working makes the silence read as enforcement.
//!
//! That is two defects, not one. It is an execution-authority bypass, and it
//! is a corrupted test oracle: `axon-run` is the interpreter side of the wasm
//! and codegen parity harnesses, so those compared codegen against an
//! interpreter that enforced nothing, and a capability-refusal divergence was
//! invisible to them.
//!
//! THE FIX IS A SHARED PATH, NOT A DUPLICATED CHECK. Adding three calls to
//! `axon-run` would have fixed `axon-run` and left the fourth runner to be
//! written without them. Every runner calls [`prepare`], and
//! `tests/preflight_reachability.rs` fails if a binary reaches
//! `interp::run_program` without it.

use crate::ast::Program;

/// What a runner needs after preflight succeeds.
pub struct Ready {
    pub program: Program,
    /// The record/replay mode this run is operating under, so the caller can
    /// finish the journal at exit.
    pub replay_mode: crate::replay::Mode,
}

/// Parse, check, and install execution controls — or return the exit code the
/// runner should terminate with, diagnostics already printed.
///
/// The check is [`crate::check_pipeline`], which is the same function the CLI
/// uses and which already runs capability (`@[contained]`) checking alongside
/// resolution, inference and the semantic rules. Nothing is re-implemented
/// here; the point is that there is exactly one way in.
pub fn prepare(src: &str, file: &str) -> Result<Ready, i32> {
    // 1. PARSE + TYPE + CAPABILITY. `check_pipeline` reports every stage's
    //    diagnostics, including E1001 from `capabilities::check_capabilities`.
    let diags = crate::check_pipeline(src, file);
    let errors: Vec<_> = diags.iter().filter(|d| d.severity == "error").collect();
    if !errors.is_empty() {
        for d in &errors {
            eprintln!("{}", d.json());
        }
        return Err(2);
    }

    // The program is re-parsed rather than threaded out of `check_pipeline`,
    // which returns diagnostics by design. Parsing twice costs microseconds and
    // keeps this function from needing a second, subtly different pipeline —
    // a second pipeline is the defect this module exists to remove.
    let program = match crate::parse_source(src) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("axon: parse error in {file}:\n{e}");
            return Err(1);
        }
    };

    // 2. AUDIT LEDGER, before anything capability-bearing can run.
    if let Ok(ledger_path) = std::env::var("AXON_AUDIT_LEDGER") {
        axon_audit::set_ledger_path(std::path::Path::new(&ledger_path));
    }

    // 3. RECORD / REPLAY. A runner that silently ignored AXON_RECORD left an
    //    ABSENT journal, which an operator cannot distinguish from a run that
    //    touched nothing; under AXON_REPLAY it performed the effects for real
    //    while printing the replayed output.
    let replay_mode = match crate::replay::install_from_env() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("axon: {e}");
            return Err(2);
        }
    };

    Ok(Ready {
        program,
        replay_mode,
    })
}
