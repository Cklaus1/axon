//! `axon-intent` CLI entrypoint (R22 §5.2). Thin: delegates to `cli::run`, which
//! dispatches compile / review / approve / emit and returns the exit code.

use std::process::ExitCode;

fn main() -> ExitCode {
    axon_intent::cli::run(std::env::args().collect())
}
