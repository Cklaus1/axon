//! The LocalSidecar server: one JSON frame per line on stdin, one on stdout.
//!
//! It holds NO authority logic of its own — every request goes through
//! `axon_reflex::serve_one`, the same function the embedded mode and the
//! remote server use. That is deliberate. Three copies of a principal check is
//! three places for it to drift, and this repository's engine-parity work is a
//! long record of precisely that: the same control honoured in one place and
//! silently ignored in another, with nothing to notice.

use std::io::{BufRead, Write};

fn main() {
    let mut core = axon_reflex::ReflexCore::new();
    let stdin = std::io::stdin();
    let mut out = std::io::stdout();
    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        if line.trim().is_empty() {
            continue;
        }
        let resp = axon_reflex::serve_one(&mut core, &line);
        if writeln!(out, "{resp}").is_err() || out.flush().is_err() {
            // The client went away. Nothing was decided and nothing should be
            // claimed; exiting quietly is the honest outcome.
            break;
        }
    }
}
