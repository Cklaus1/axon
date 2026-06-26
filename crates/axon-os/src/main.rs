//! `axon-os` CLI entrypoint (R21 §5.2). Thin: delegates to `cli::run`, which
//! dispatches explain / run / verify / replay and returns the exit code.

use std::process::ExitCode;

fn main() -> ExitCode {
    axon_os::cli::run(std::env::args().collect())
}
