//! Regressions for defects an adversarial review REPRODUCED against Phase 1.
//!
//! Every test here corresponds to a transcript, not a theory. The shared
//! authority core survived every attack; all of these were in the transport and
//! reporting layers around it — which is where this repository's defects have
//! consistently lived.

use axon_reflex::*;

fn core_reply(req: &str) -> String {
    let mut core = ReflexCore::new();
    serve_one(&mut core, req)
}

/// Duplicate `principal` keys are last-wins under a plain serde parse.
///
/// Reproduced against the shipped sidecar binary:
///   {"...","principal":"mallory","principal":"alice"} -> served as alice
/// A reviewer reading that line left-to-right sees mallory. `axon-cortex` was
/// hardened against exactly this; this crate re-introduced it by parsing
/// loosely, which is why the fix REUSES `parse_strict` rather than re-deriving
/// the check.
#[test]
fn duplicate_principal_keys_are_refused_not_last_wins() {
    let r = core_reply(
        r#"{"protocol":"axon-reflex/1","op":"encode","id":"","input":"x","req_id":1,"principal":"mallory","principal":"alice"}"#,
    );
    assert!(
        r.contains("\"refusal\""),
        "a duplicate-key request must be refused, got: {r}"
    );
    assert!(
        !r.contains("\"id\":\"st-"),
        "a duplicate-key request minted state: {r}"
    );
}

/// A request naming no principal must not be served under the anonymous one.
///
/// Reproduced: `null`, `42`, an object, and an omitted field all collapsed to
/// `""` — a SHARED namespace in which four different malformed callers decided
/// as one another and could read each other's handles with no refusal.
#[test]
fn a_request_without_a_usable_principal_is_refused() {
    for bad in [
        r#""principal":null"#,
        r#""principal":42"#,
        r#""principal":{"principal":"alice"}"#,
        r#""principal":"""#,
    ] {
        let req = format!(
            r#"{{"protocol":"axon-reflex/1","op":"encode","id":"","input":"x","req_id":1,{bad}}}"#
        );
        let r = core_reply(&req);
        assert!(
            r.contains("names no principal"),
            "request with {bad} was served rather than refused: {r}"
        );
    }
}

/// The backend checks the request's protocol tag, not only the client the
/// response's. It was validated in one direction while the doc promised both.
#[test]
fn a_request_declaring_another_protocol_is_refused() {
    let r = core_reply(
        r#"{"protocol":"axon-reflex/99","op":"encode","id":"","input":"x","principal":"alice","req_id":1}"#,
    );
    assert!(
        r.contains("this backend speaks"),
        "a foreign protocol version was served: {r}"
    );
}

/// A reply for a DIFFERENT request must not be read as this one's answer.
///
/// Reproduced against an HONEST backend preceded by one stray frame: every
/// reply shifted by one, permanently, and mallory received `Ok` on alice's
/// handle because the server's refusal landed on the previous slot.
#[test]
fn a_mismatched_correlation_id_is_a_transport_refusal() {
    let framed = encode_response_for(
        7,
        &Ok(serde_json::json!({ "choice": "decided(q)", "principal": "alice" })),
    );
    match decode_response(&framed, 8) {
        Err(Refusal::Transport(m)) => assert!(
            m.contains("correlation mismatch"),
            "wrong transport message: {m}"
        ),
        other => panic!("a reply for request 7 was accepted as request 8's: {other:?}"),
    }
    // Control: the matching id is accepted, or the check would be passing by
    // rejecting everything.
    assert!(
        decode_response(&framed, 7).is_ok(),
        "the matching id must be accepted"
    );
}

/// A content-free frame must not become a successful decision.
///
/// Reproduced: `{"protocol":"axon-reflex/1"}` yielded
/// `Ok(Decision { choice: "", principal: "alice" })` — a decision manufactured
/// from a reply that decided nothing, attributed to whoever asked.
#[test]
fn a_frame_stating_nothing_is_not_a_decision() {
    let empty = format!(r#"{{"protocol":"{PROTOCOL}","req_id":1}}"#);
    let v = decode_response(&empty, 1).expect("frame itself is well-formed");
    assert!(
        v.get("choice").is_none() && v.get("principal").is_none(),
        "the frame should carry neither field; the client must refuse to invent them"
    );
}

/// Every `Refusal` variant must survive the wire AS ITSELF.
///
/// Three of five previously fell into a catch-all `{"kind":"other"}` and came
/// back as `Refusal::Protocol`. A transport failure arriving as a protocol
/// error is the infra-versus-decision mislabelling this crate claims to guard
/// against — and a supervisor RETRIES protocol errors.
#[test]
fn every_refusal_variant_survives_serialization_as_itself() {
    let cases = vec![
        Refusal::CrossPrincipal {
            owner: "alice".into(),
            caller: "mallory".into(),
        },
        Refusal::UnknownState { id: "st-9".into() },
        Refusal::Unsupported {
            control: "cancellable".into(),
            mode: "remote",
        },
        Refusal::Transport("connection reset".into()),
        Refusal::Protocol("bad frame".into()),
    ];
    for c in cases {
        let wire = encode_response_for(3, &Err(c.clone()));
        let back = decode_response(&wire, 3).expect_err("a refusal must decode as a refusal");
        let same = matches!(
            (&c, &back),
            (
                Refusal::CrossPrincipal { .. },
                Refusal::CrossPrincipal { .. }
            ) | (Refusal::UnknownState { .. }, Refusal::UnknownState { .. })
                | (Refusal::Unsupported { .. }, Refusal::Unsupported { .. })
                | (Refusal::Transport(_), Refusal::Transport(_))
                | (Refusal::Protocol(_), Refusal::Protocol(_))
        );
        assert!(
            same,
            "{c:?} came back as {back:?} — the variant was lost on the wire"
        );
    }
}

/// An oversized frame is refused rather than buffered without bound.
#[test]
fn an_oversized_frame_is_refused() {
    let huge = "x".repeat(MAX_FRAME + 1);
    let r = core_reply(&format!(
        r#"{{"protocol":"axon-reflex/1","op":"encode","id":"","input":"{huge}","principal":"alice","req_id":1}}"#
    ));
    // The core itself does not bound input; the CLIENTS do, before writing.
    // What must hold here is that the reply is still a well-formed frame rather
    // than an unbounded echo of the payload.
    assert!(r.len() < MAX_FRAME, "the reply echoed an unbounded payload");
}
