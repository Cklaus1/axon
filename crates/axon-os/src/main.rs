//! `axon-os` CLI entrypoint (R21 §5.2). The full command surface
//! (run/replay/verify/explain) lands in slice S7; S1 ships a stub that
//! validates a manifest and prints its parsed grant, so the binary is exercised
//! end-to-end from the first slice.

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 3 || args[1] != "check-manifest" {
        eprintln!("usage: axon-os check-manifest <job.axjob>   (S1; full CLI in S7)");
        return ExitCode::from(2);
    }
    let path = Path::new(&args[2]);
    let src = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("axon-os: cannot read {}: {e}", path.display());
            return ExitCode::from(2);
        }
    };
    let base = path.parent().unwrap_or(Path::new("."));
    match axon_os::parse_manifest(&src, base) {
        Ok(m) => {
            println!(
                "\u{2713} manifest ok: program={} exec={} max_label={} budget(calls={},tokens={},cost_micro={})",
                m.program.display(),
                m.grant.exec.as_str(),
                m.grant.max_label.as_str(),
                m.grant.budget.calls,
                m.grant.budget.tokens,
                m.grant.budget.cost_micro,
            );
            ExitCode::from(0)
        }
        Err(v) => {
            eprintln!("axon-os: {}", v.legible());
            ExitCode::from(v.exit_code() as u8)
        }
    }
}
