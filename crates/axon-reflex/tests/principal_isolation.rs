//! The Phase 1 cross-mode invariant.
//!
//! > A decision is made under exactly one principal, and no `StateHandle` is
//! > reachable across principals. Cross-principal reuse is a REFUSAL, never a
//! > cache miss.
//!
//! Asserted identically in Embedded, LocalSidecar and RemoteService. Principal
//! isolation was chosen over latency or throughput because it is the property
//! most likely to break when work moves out of the caller's address space:
//! same-process code can reach another principal's state through a shared map,
//! a sidecar through a shared session, a hosted service through a shared cache
//! key. Each mode fails it differently, which is exactly why one test has to
//! cover all three.

use axon_reflex::*;
use std::io::{Read, Write};

/// A minimal HTTP/1.1 server for the RemoteService mode, backed by the SAME
/// `serve_one` the sidecar uses.
///
/// Real socket, real HTTP framing, real process boundary between the assertion
/// and the authority check — mocking the transport would leave the mode
/// untested precisely where it differs from the others.
fn spawn_remote() -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
    let addr = listener.local_addr().expect("addr").to_string();
    std::thread::spawn(move || {
        let mut core = ReflexCore::new();
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            let mut buf = Vec::new();
            let mut chunk = [0u8; 4096];
            // Read until the body is complete per Content-Length. A read-to-end
            // would block: the client keeps the connection open for the reply.
            loop {
                let Ok(n) = s.read(&mut chunk) else { return };
                if n == 0 {
                    break;
                }
                buf.extend_from_slice(&chunk[..n]);
                let raw = String::from_utf8_lossy(&buf).to_string();
                if let Some((head, body)) = raw.split_once("\r\n\r\n") {
                    let len: usize = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("Content-Length: ")
                                .and_then(|v| v.trim().parse().ok())
                        })
                        .unwrap_or(0);
                    if body.len() >= len {
                        let resp = serve_one(&mut core, body);
                        let http = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\n\
                             Content-Length: {}\r\nConnection: close\r\n\r\n{}",
                            resp.len(),
                            resp
                        );
                        let _ = s.write_all(http.as_bytes());
                        let _ = s.flush();
                        break;
                    }
                }
            }
        }
    });
    addr
}

fn sidecar_exe() -> String {
    env!("CARGO_BIN_EXE_axon-reflex-sidecar").to_string()
}

/// Run the invariant against one backend. Returns the mode name so the caller
/// can prove every mode was actually exercised.
fn assert_invariant(b: &mut dyn ReflexBackend) -> &'static str {
    let mode = b.mode().name();
    let alice = PrincipalScope::new("alice");
    let mallory = PrincipalScope::new("mallory");

    // PRECONDITION: the owner can use its own state. Without this the test
    // could pass against a backend that refuses everything — a refusal is only
    // meaningful if the permitted case actually works.
    let h = b
        .encode_state("some state", &alice)
        .expect("encode as alice");
    let ok = b
        .decide(&h, "q", &alice)
        .unwrap_or_else(|e| panic!("[{mode}] the owning principal must be served, got {e}"));
    // The previous assertion here compared `ok.principal` to "alice" — the
    // literal that produced it — and was VACUOUS in all three modes: the client
    // stamped the caller's own scope onto the result, so no mutation of the
    // authority core, the wire, or the server could make it fail. Assert the
    // SERVER-DERIVED content instead: `choice` is computed by the core and
    // cannot be manufactured from the request.
    assert_eq!(
        ok.choice, "decided(q)",
        "[{mode}] the decision must carry the backend's computed choice, not a \
         value the client manufactured from its own request"
    );
    assert_eq!(
        ok.principal, "alice",
        "[{mode}] attribution must match the principal the backend stated"
    );

    // THE INVARIANT: another principal presenting the same handle.
    match b.decide(&h, "q", &mallory) {
        Err(Refusal::CrossPrincipal { owner, caller }) => {
            assert_eq!(owner, "alice", "[{mode}] refusal names the wrong owner");
            assert_eq!(caller, "mallory", "[{mode}] refusal names the wrong caller");
        }
        // This is the failure the distinction exists for. A miss invites the
        // caller to re-encode and proceed, so the boundary is crossed silently
        // and nothing records that it happened.
        Err(Refusal::UnknownState { .. }) => panic!(
            "[{mode}] cross-principal reuse reported as a CACHE MISS. A miss is retryable and \
             leaves no trace; an authority violation must be a refusal."
        ),
        Err(e) => panic!("[{mode}] expected CrossPrincipal, got {e}"),
        Ok(d) => panic!(
            "[{mode}] PRINCIPAL ISOLATION VIOLATED: mallory obtained a decision on alice's \
             state, attributed to `{}`",
            d.principal
        ),
    }

    // Release is authority-bearing too: a foreign principal must not be able
    // to destroy state it cannot read.
    match b.release_state(&h, &mallory) {
        Err(Refusal::CrossPrincipal { .. }) => {}
        other => panic!("[{mode}] release must refuse a foreign principal, got {other:?}"),
    }

    // And the owner's state survived the attempt — a refusal that also had a
    // side effect would be a denial-of-service dressed as a check.
    b.decide(&h, "q", &alice)
        .unwrap_or_else(|e| panic!("[{mode}] owner's state was damaged by a refused request: {e}"));

    mode
}

// ONE TEST PER MODE, not one test over three modes.
//
// The combined version aborted at the first mode: a mutation removing the
// authority check failed at `[embedded]` and never reached the sidecar or the
// remote service, so those two modes were unproven while the suite looked like
// it covered them. Per-mode tests mean a mutation must be caught THREE times
// independently, and a failure names the mode that broke.

#[test]
fn principal_isolation_holds_in_embedded_mode() {
    assert_eq!(assert_invariant(&mut EmbeddedBackend::new()), "embedded");
}

#[test]
fn principal_isolation_holds_in_local_sidecar_mode() {
    let mut b = SidecarBackend::spawn(&sidecar_exe()).expect("spawn sidecar");
    assert_eq!(assert_invariant(&mut b), "local-sidecar");
}

#[test]
fn principal_isolation_holds_in_remote_service_mode() {
    let mut b = RemoteBackend::new(spawn_remote());
    assert_eq!(assert_invariant(&mut b), "remote-service");
}

/// Coverage guard: every mode the enum defines must have a test above.
///
/// Without this, adding a fourth mode would leave it silently unexercised
/// while the suite still reported three green modes — the same
/// absent-vs-verified collapse the per-mode split exists to prevent.
#[test]
fn every_deployment_mode_has_an_isolation_test() {
    // Derived from `Mode::all()`, which is an exhaustive `match` — adding a
    // variant fails to COMPILE rather than silently leaving a mode untested.
    // The previous version listed the modes as a hand-written literal beside a
    // doc comment promising that a fourth mode could not slip through, which is
    // exactly what it could not prevent.
    let src = include_str!("principal_isolation.rs");
    for m in Mode::all() {
        let fn_name = format!(
            "fn principal_isolation_holds_in_{}_mode",
            m.name().replace('-', "_")
        );
        assert!(
            src.contains(&fn_name),
            "mode `{}` is declared by the enum but has no test named `{fn_name}`",
            m.name()
        );
    }
}

/// A genuine miss must stay a miss. If everything became `CrossPrincipal` the
/// invariant test above would pass for the wrong reason.
#[test]
fn an_unknown_handle_is_a_miss_and_not_an_authority_violation() {
    let mut b = EmbeddedBackend::new();
    let alice = PrincipalScope::new("alice");
    let ghost = StateHandle {
        id: "st-does-not-exist".into(),
        principal: "alice".into(),
    };
    match b.decide(&ghost, "q", &alice) {
        Err(Refusal::UnknownState { id }) => assert_eq!(id, "st-does-not-exist"),
        other => panic!("a nonexistent handle must be a miss, got {other:?}"),
    }
}

/// The two refusals ARE distinguishable, and that is a deliberate trade-off.
///
/// This test was previously named "a refusal does not become an oracle" while
/// its body asserted that the oracle exists — a name a grep would report as a
/// property nobody has. Both orderings of the existence and ownership checks
/// yield two distinguishable refusals, so the ordering rationale that used to
/// sit in `lib.rs` was simply wrong and has been removed.
///
/// What is true: ids are sequential and global across principals, so a caller
/// can enumerate them and learn which exist and who owns each, because the
/// refusal carries `owner`. That is the price of an auditable refusal, and it
/// is stated here rather than denied.
#[test]
fn a_cross_principal_refusal_names_the_owner_and_is_therefore_an_existence_oracle() {
    let mut b = EmbeddedBackend::new();
    let alice = PrincipalScope::new("alice");
    let mallory = PrincipalScope::new("mallory");
    let h = b.encode_state("s", &alice).unwrap();

    let real = b.decide(&h, "q", &mallory).unwrap_err();
    let fake = b
        .decide(
            &StateHandle {
                id: "st-999999".into(),
                principal: "alice".into(),
            },
            "q",
            &mallory,
        )
        .unwrap_err();

    // They ARE different, and that is the documented trade-off: naming the
    // owner is what makes the refusal auditable. What must not happen is the
    // reverse — an existing foreign handle reported as a miss, which would let
    // the caller re-encode and proceed.
    assert!(matches!(real, Refusal::CrossPrincipal { .. }));
    assert!(matches!(fake, Refusal::UnknownState { .. }));
}
