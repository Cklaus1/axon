//! EG-4 lane I-E: the `ai:<model>` generator's two refusal arms
//! (`generator.rs`, `AiGenerator::generate`, `cfg(feature = "ai")`), run
//! offline: `cargo test -p axon-intent --features ai --test refusal_e`.
//!
//! No request leaves the host and nothing is billed: the mock is switched off
//! only after the endpoint is pinned to a loopback listener of this process,
//! and the first case fails before any request (no API key). The single test
//! owns the process environment (it is the only test in this binary).
#![cfg(feature = "ai")]

use axon_intent::generator::{parse_spec, GenFailure};
use std::net::TcpListener;
use std::time::{Duration, Instant};

#[test]
fn an_ai_generator_s_failed_or_late_call_is_a_failure_not_a_candidate() {
    // A listener that never accepts: the kernel completes the handshake from
    // its backlog, and no answer ever comes.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::env::set_var("AXON_AI_BASE_URL", format!("http://127.0.0.1:{port}"));
    std::env::set_var("AXON_DOTENV", "/nonexistent/axon-intent-refusal-e.env");
    for k in [
        "AXON_AI_MOCK",
        "AXON_AI_PROVIDER",
        "ANTHROPIC_BASE_URL",
        "AXON_AI_API_KEY",
        "ANTHROPIC_API_KEY",
        "AXON_DOTENV_WALK",
    ] {
        std::env::remove_var(k);
    }

    // 1. No API key: axon-ai returns its error before any request.
    let g = parse_spec("ai:test-model", Duration::from_secs(20)).unwrap();
    let r = g.generate("p");
    assert!(
        matches!(&r, Err(GenFailure::Failed { reason }) if reason.contains("not set")),
        "ATTACK: an ai: generator whose call failed yielded a candidate: {r:?}"
    );

    // 2. A key, and an endpoint that never answers: the deadline.
    std::env::set_var("AXON_AI_API_KEY", "not-a-real-key");
    let g = parse_spec("ai:test-model", Duration::from_millis(300)).unwrap();
    let start = Instant::now();
    let r = g.generate("p");
    assert!(
        matches!(r, Err(GenFailure::TimedOut { after_ms: 300 }))
            && start.elapsed() < Duration::from_secs(10),
        "ATTACK: an ai: generator past its deadline yielded a candidate: {r:?}"
    );
    drop(listener);
}
