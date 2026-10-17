//! `axon-intent` CLI entrypoint (R22 §5.2). Thin: delegates to `cli::run`, which
//! dispatches compile / review / approve / emit and returns the exit code.

use std::process::ExitCode;

fn main() -> ExitCode {
    // KG5: a termination signal first SIGKILLs the synthesizer's process-group
    // job, then kills `axon-intent` by that same signal. Before this, the
    // synthesizer and its descendants ran on after `axon-intent` died.
    axon_jobctl::forward_termination_signals(axon_jobctl::OnSignal::Die);
    axon_intent::cli::run(std::env::args().collect())
}
