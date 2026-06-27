//! `certcheck` CLI entrypoint (R23 §5.2). Thin: delegates to `cli::run`, which
//! dispatches check / explain / prove and returns the exit code. The trusted
//! `check` path reaches no solver (A4).

use std::process::ExitCode;

fn main() -> ExitCode {
    axon_certcheck::cli::run(std::env::args().collect())
}
