//! R33.S2a — vsock wire protocol (governance/specs/R33-cross-vm-safety-quorum.md §5.2.2).
//!
//! This is the FIRST, smallest sub-slice of S2: the length-prefixed JSON framing that a real
//! transport (raw `AF_VSOCK`, or a TCP-loopback CI stand-in) will carry `VoteRequest`/
//! `VoteResponse` over. It is deliberately transport-agnostic — every function here takes a
//! generic `Read`/`Write`, so the SAME code path is exercised whether the underlying stream is a
//! real vsock socket or a plain `TcpStream` (no separate mock needed for this layer).
//!
//! Framing matches `crates/axon-core/src/interp.rs`'s existing `vsock_send_recv` convention
//! exactly (same repo, same substrate family, no reason to diverge): a 4-byte little-endian `u32`
//! length prefix, then that many raw bytes. A length of 0 is the EOF/absent-reply sentinel
//! (`read_frame`/`read_json_frame` return `Ok(None)`), matching `vsock_send_recv`'s own
//! `Ok(None)` case. `interp.rs`'s function can't be called directly (different crate, private,
//! and hardcoded to the guest-connects-to-host direction) — this module replicates the wire
//! FORMAT, not the function, which is the only thing S2's design (§5.2.2) actually needs shared.
//!
//! S2b (landed) adds [`connect_and_round_trip`]: the proposer side, over a real TCP-loopback
//! socket (the §5.2.2 CI stand-in — real `AF_VSOCK` is a later, explicitly-gated swap of just the
//! `connect` call, same wire format). One connection, one request, one response, deadline-bounded
//! (fail-closed: timeout or connection failure ⇒ `Err`, treated by callers exactly like a missing
//! `.vote` file — no vote from that peer, not a hard failure of the whole quorum).
//!
//! S2c (landed) adds [`broadcast_and_collect`]: N-peer fan-out, one thread per peer so the whole
//! broadcast's wall-clock stays bounded by `deadline` regardless of peer count, feeding straight
//! into `logic::check_quorum`'s existing `&[VoteResponse]` + `required_n` shape.
//!
//! S2d (landed) adds [`respond_once`]: the voter side, single-shot accept/read/respond (no daemon
//! loop) — now backing `axon-vm quorum vote --listen PORT` in `main.rs`.
//!
//! S2e (landed) wires [`broadcast_and_collect`] into `axon-vm quorum propose --broadcast` in
//! `main.rs` — every function in this module now has a real, non-test caller.
//!
//! NOT built yet: real `AF_VSOCK` (this whole module is still the §5.2.2 TCP-loopback CI
//! stand-in — swapping `connect`/`bind` for the raw `libc::socket(AF_VSOCK, ...)` calls
//! `interp.rs`'s `vsock_send_recv` already demonstrates is the one remaining piece, same wire
//! format either way).

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{self, Read, Write};

/// Writes one length-prefixed frame: a 4-byte little-endian length, then `payload` verbatim.
pub fn write_frame(w: &mut impl Write, payload: &[u8]) -> io::Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "frame payload exceeds u32::MAX bytes",
        )
    })?;
    w.write_all(&len.to_le_bytes())?;
    w.write_all(payload)?;
    w.flush()
}

/// Reads one length-prefixed frame. `Ok(None)` means a zero-length frame arrived (the EOF/
/// absent-reply sentinel, matching `interp.rs`'s `vsock_send_recv`) — not an error, since a peer
/// legitimately signaling "no reply" is a normal outcome this protocol defines, not a fault.
pub fn read_frame(r: &mut impl Read) -> io::Result<Option<Vec<u8>>> {
    let mut lbuf = [0u8; 4];
    r.read_exact(&mut lbuf)?;
    let len = u32::from_le_bytes(lbuf) as usize;
    if len == 0 {
        return Ok(None);
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(Some(buf))
}

/// Serializes `val` as JSON and writes it as one frame.
pub fn write_json_frame<T: Serialize>(w: &mut impl Write, val: &T) -> io::Result<()> {
    let bytes =
        serde_json::to_vec(val).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    write_frame(w, &bytes)
}

/// Reads one frame and deserializes it as `T`. `Ok(None)` propagates the EOF/absent-reply
/// sentinel from [`read_frame`] (a peer that has nothing to send, not malformed input).
pub fn read_json_frame<T: DeserializeOwned>(r: &mut impl Read) -> io::Result<Option<T>> {
    match read_frame(r)? {
        None => Ok(None),
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
    }
}

/// S2b — proposer side: connect to `addr` over TCP, send `req` as one JSON frame, read back one
/// JSON frame as the response. `deadline` bounds BOTH the connect and the read (fail-closed on
/// spec §4.4's A4 intent: a peer that doesn't respond in time contributes no vote, the same
/// semantics `quorum::io::collect_responses` already has for a missing `.vote` file). `Ok(None)`
/// means the peer responded with the EOF sentinel (a real "no vote" answer, not a fault); a
/// timeout or connection failure is `Err` — callers collecting from multiple peers treat both
/// `Ok(None)` and `Err` as "this peer contributed no vote," matching the file-based path's
/// already-established fail-closed-on-absence policy.
///
/// TCP today, not real `AF_VSOCK` — this is deliberately the §5.2.2 CI-loopback stand-in; a real
/// vsock connect is a later, explicitly-gated sub-slice (swapping this function's `TcpStream::
/// connect_timeout` for the raw `libc::socket(AF_VSOCK, ...)` call `interp.rs`'s `vsock_send_recv`
/// already demonstrates, same wire format either way).
pub fn connect_and_round_trip<Req: Serialize, Resp: DeserializeOwned>(
    addr: std::net::SocketAddr,
    req: &Req,
    deadline: std::time::Duration,
) -> io::Result<Option<Resp>> {
    let mut stream = std::net::TcpStream::connect_timeout(&addr, deadline)?;
    stream.set_read_timeout(Some(deadline))?;
    stream.set_write_timeout(Some(deadline))?;
    write_json_frame(&mut stream, req)?;
    read_json_frame(&mut stream)
}

/// S2c — proposer side: broadcast `req` to every address in `peers` and collect whatever
/// `VoteResponse`s come back within `deadline`, feeding directly into [`super::logic::
/// check_quorum`] (which takes `&[VoteResponse]` + the operator-configured `required_n` — NOT
/// `peers.len()`, so a peer that contributes nothing here is simply absent from the count, exactly
/// as `check_quorum`'s own doc comment already specifies for the file-based path).
///
/// Each peer is contacted from its OWN thread so the total wall-clock is bounded by `deadline`
/// itself, not `deadline * peers.len()` — a sequential loop would let one slow/unreachable peer at
/// the front silently inflate the whole broadcast's latency past what §4.4's deadline is supposed
/// to guarantee. A peer whose [`connect_and_round_trip`] returns `Err` (timeout, refused
/// connection) or `Ok(None)` (the peer's own EOF sentinel) contributes NO entry to the result —
/// both are "no vote from this peer," never a hard error for the whole broadcast; a single
/// unreachable peer must not be able to block quorum outright (that's exactly the failure mode
/// the deadline + fail-closed-on-absence design exists to avoid).
pub fn broadcast_and_collect(
    peers: &[std::net::SocketAddr],
    req: &crate::quorum::logic::VoteRequest,
    deadline: std::time::Duration,
) -> Vec<crate::quorum::logic::VoteResponse> {
    let handles: Vec<_> = peers
        .iter()
        .copied()
        .map(|addr| {
            let req = req.clone();
            std::thread::spawn(move || connect_and_round_trip(addr, &req, deadline))
        })
        .collect();

    handles
        .into_iter()
        .filter_map(|h| h.join().ok()) // a panicked collector thread contributes nothing either
        .filter_map(|result| result.ok().flatten())
        .collect()
}

/// S2d — voter side: accept exactly ONE inbound connection on `listener`, read one `VoteRequest`
/// JSON frame, hand it to `respond`, and write back whatever `VoteResponse` `respond` returns (or
/// the EOF sentinel if `respond` returns `None` — mirrors [`connect_and_round_trip`]'s own `Ok
/// (None)` convention: a deliberate "no vote" answer is a legitimate outcome, not a fault).
///
/// Deliberately single-shot, not a daemon loop: the CLI command this backs (`axon-vm quorum vote
/// --listen PORT`) is meant to be invoked once per expected proposal, the same invocation
/// granularity `propose`/`vote`/`check` already have in the file-based path — an external
/// orchestrator (systemd, a shell script, `axon-web`'s CLI-wrapping pattern) owns retry/repeat
/// policy, not this function. `listener.accept()` blocks with no internal timeout — a legitimate
/// scope boundary for a "wait for the one expected connection" primitive, not an oversight; a
/// caller that needs a bounded wait wraps this call with its own timeout.
pub fn respond_once(
    listener: &std::net::TcpListener,
    respond: impl FnOnce(
        crate::quorum::logic::VoteRequest,
    ) -> Option<crate::quorum::logic::VoteResponse>,
) -> io::Result<()> {
    let (mut stream, _) = listener.accept()?;
    let req: crate::quorum::logic::VoteRequest =
        read_json_frame(&mut stream)?.ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "peer sent the EOF sentinel instead of a VoteRequest",
            )
        })?;
    match respond(req) {
        Some(resp) => write_json_frame(&mut stream, &resp),
        None => write_frame(&mut stream, b""),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quorum::logic::{VoteRequest, VoteResponse};
    use std::io::Cursor;

    #[test]
    fn frame_round_trips_arbitrary_bytes() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"hello quorum").unwrap();
        let mut cursor = Cursor::new(buf);
        let got = read_frame(&mut cursor).unwrap();
        assert_eq!(got, Some(b"hello quorum".to_vec()));
    }

    #[test]
    fn frame_round_trips_empty_payload_distinctly_from_eof_sentinel() {
        // An explicit empty (but present) payload is length-prefixed 0 on the wire — same bytes
        // as the EOF sentinel. This is a deliberate protocol property (mirrors interp.rs): the
        // wire format cannot distinguish "peer sent zero bytes" from "peer sent nothing"; callers
        // that need to send a real empty value must wrap it in a JSON frame instead (e.g. `""`),
        // which is never zero bytes long on the wire.
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").unwrap();
        let mut cursor = Cursor::new(buf);
        let got = read_frame(&mut cursor).unwrap();
        assert_eq!(got, None);
    }

    #[test]
    fn read_frame_on_truncated_stream_is_an_io_error_not_a_panic() {
        // Length prefix claims 100 bytes but only 2 are present.
        let mut buf = 100u32.to_le_bytes().to_vec();
        buf.extend_from_slice(b"ab");
        let mut cursor = Cursor::new(buf);
        let result = read_frame(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn read_frame_on_empty_stream_is_an_io_error_not_a_panic() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        let result = read_frame(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn json_frame_round_trips_a_real_vote_request() {
        let req = VoteRequest {
            run_id: "run-123".to_string(),
            prog_hash: "abc123".to_string(),
            voter_tcb: "axtcb1:deadbeef".to_string(),
            proposed_action: "deploy".to_string(),
            timestamp_ms: 1_234_567_890,
        };
        let mut buf = Vec::new();
        write_json_frame(&mut buf, &req).unwrap();
        let mut cursor = Cursor::new(buf);
        let got: Option<VoteRequest> = read_json_frame(&mut cursor).unwrap();
        assert_eq!(got, Some(req));
    }

    #[test]
    fn json_frame_round_trips_a_real_vote_response() {
        let resp = VoteResponse {
            voter_tcb: "axtcb1:deadbeef".to_string(),
            run_id: "run-123".to_string(),
            approved: true,
            reason: "policy: risk within bounds".to_string(),
            lineage_root: "principal-a".to_string(),
        };
        let mut buf = Vec::new();
        write_json_frame(&mut buf, &resp).unwrap();
        let mut cursor = Cursor::new(buf);
        let got: Option<VoteResponse> = read_json_frame(&mut cursor).unwrap();
        assert_eq!(got, Some(resp));
    }

    #[test]
    fn json_frame_eof_sentinel_propagates_as_none_not_a_deserialize_error() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"").unwrap(); // zero-length frame, the EOF sentinel
        let mut cursor = Cursor::new(buf);
        let got: io::Result<Option<VoteRequest>> = read_json_frame(&mut cursor);
        assert_eq!(got.unwrap(), None);
    }

    #[test]
    fn malformed_json_frame_is_an_io_error_not_a_panic() {
        let mut buf = Vec::new();
        write_frame(&mut buf, b"{not valid json").unwrap();
        let mut cursor = Cursor::new(buf);
        let result: io::Result<Option<VoteRequest>> = read_json_frame(&mut cursor);
        assert!(result.is_err());
    }

    #[test]
    fn multiple_frames_on_one_stream_read_back_in_order() {
        // The eventual broadcast/collect loop will read multiple sequential frames off one
        // connection (or multiple connections) — confirm the reader doesn't over-consume.
        let mut buf = Vec::new();
        write_frame(&mut buf, b"first").unwrap();
        write_frame(&mut buf, b"second").unwrap();
        let mut cursor = Cursor::new(buf);
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"first".to_vec()));
        assert_eq!(read_frame(&mut cursor).unwrap(), Some(b"second".to_vec()));
    }

    // ── S2b: real-socket round trip (TCP loopback) ──────────────────────────────────────
    // These spin up an ad-hoc std::net::TcpListener on 127.0.0.1:0 (OS-assigned port) as a
    // throwaway test-local "voter" — not a new production listen/accept/respond primitive
    // (that's the separate, larger S2c work the module doc comment still lists as open). The
    // point of these tests is proving connect_and_round_trip works over a REAL socket, not just
    // an in-memory Cursor (which S2a's tests above already fully covered).

    use std::net::TcpListener;
    use std::time::Duration;

    // Delegates to the real production `respond_once` (S2d) rather than duplicating its
    // accept/read/respond/write logic — so every test in this file that uses this helper
    // (S2a-c, written before `respond_once` existed) also exercises `respond_once` itself.
    fn spawn_one_shot_voter(
        respond: impl FnOnce(VoteRequest) -> Option<VoteResponse> + Send + 'static,
    ) -> std::net::SocketAddr {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            respond_once(&listener, respond).unwrap();
        });
        addr
    }

    fn a_vote_request() -> VoteRequest {
        VoteRequest {
            run_id: "run-s2b".to_string(),
            prog_hash: "abc123".to_string(),
            voter_tcb: "axtcb1:deadbeef".to_string(),
            proposed_action: "deploy".to_string(),
            timestamp_ms: 1_234_567_890,
        }
    }

    #[test]
    fn connect_and_round_trip_over_real_tcp_socket_returns_the_voters_response() {
        let expected = VoteResponse {
            voter_tcb: "axtcb1:deadbeef".to_string(),
            run_id: "run-s2b".to_string(),
            approved: true,
            reason: "policy: risk within bounds".to_string(),
            lineage_root: "principal-a".to_string(),
        };
        let resp_clone = expected.clone();
        let addr = spawn_one_shot_voter(move |req| {
            assert_eq!(req, a_vote_request());
            Some(resp_clone)
        });

        let got: Option<VoteResponse> =
            connect_and_round_trip(addr, &a_vote_request(), Duration::from_secs(5)).unwrap();
        assert_eq!(got, Some(expected));
    }

    #[test]
    fn connect_and_round_trip_eof_sentinel_from_a_real_voter_is_none_not_an_error() {
        let addr = spawn_one_shot_voter(|_req| None); // voter sends the EOF sentinel
        let got: io::Result<Option<VoteResponse>> =
            connect_and_round_trip(addr, &a_vote_request(), Duration::from_secs(5));
        assert_eq!(got.unwrap(), None);
    }

    #[test]
    fn connect_and_round_trip_to_a_dead_port_is_an_io_error_not_a_panic() {
        // Bind and immediately drop, so the OS-assigned port is (almost certainly) refusing
        // connections when we try it — a peer that's simply not there, matching the "channel
        // error ⇒ vote absent" fail-closed policy this function's own doc comment describes.
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let got: io::Result<Option<VoteResponse>> =
            connect_and_round_trip(addr, &a_vote_request(), Duration::from_millis(500));
        assert!(got.is_err());
    }

    // ── S2c: N-peer broadcast/collect fan-out ───────────────────────────────────────────

    fn a_vote_response(voter_tcb: &str, approved: bool, lineage_root: &str) -> VoteResponse {
        VoteResponse {
            voter_tcb: voter_tcb.to_string(),
            run_id: "run-s2c".to_string(),
            approved,
            reason: "fixture".to_string(),
            lineage_root: lineage_root.to_string(),
        }
    }

    #[test]
    fn broadcast_and_collect_gathers_every_responsive_peers_vote() {
        let r1 = a_vote_response("axtcb1:aaa", true, "principal-a");
        let r2 = a_vote_response("axtcb1:aaa", true, "principal-b");
        let r3 = a_vote_response("axtcb1:aaa", false, "principal-c");
        let addrs = [
            spawn_one_shot_voter({
                let r = r1.clone();
                move |_| Some(r)
            }),
            spawn_one_shot_voter({
                let r = r2.clone();
                move |_| Some(r)
            }),
            spawn_one_shot_voter({
                let r = r3.clone();
                move |_| Some(r)
            }),
        ];

        let mut got = broadcast_and_collect(
            &addrs,
            &VoteRequest {
                run_id: "run-s2c".to_string(),
                prog_hash: "abc123".to_string(),
                voter_tcb: "axtcb1:aaa".to_string(),
                proposed_action: "deploy".to_string(),
                timestamp_ms: 1,
            },
            Duration::from_secs(5),
        );
        got.sort_by(|a, b| a.lineage_root.cmp(&b.lineage_root));
        assert_eq!(got, vec![r1, r2, r3]);
    }

    #[test]
    fn broadcast_and_collect_drops_unreachable_peers_without_blocking_the_others() {
        // One real voter, one dead port (bind-then-drop) — the dead peer must contribute NOTHING
        // to the result, not an error that aborts the whole broadcast (§4.4 fail-closed-on-
        // absence: a single unreachable peer must not be able to block quorum outright).
        let r1 = a_vote_response("axtcb1:aaa", true, "principal-a");
        let live_addr = spawn_one_shot_voter({
            let r = r1.clone();
            move |_| Some(r)
        });
        let dead_listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let dead_addr = dead_listener.local_addr().unwrap();
        drop(dead_listener);

        let got = broadcast_and_collect(
            &[live_addr, dead_addr],
            &a_vote_request(),
            Duration::from_secs(2),
        );
        assert_eq!(got, vec![r1]);
    }

    #[test]
    fn broadcast_and_collect_wall_clock_does_not_scale_with_peer_count() {
        // 4 dead peers, each individually bounded by `deadline` — if broadcast_and_collect were
        // sequential, this would take ~4x deadline; parallelized (one thread per peer), it should
        // take roughly ONE deadline's worth of wall-clock. Generous margin (3x a single deadline)
        // to stay robust under this host's own contention, while still catching an accidental
        // regression to a sequential loop (which would take ~4x, comfortably over the margin).
        let deadline = Duration::from_millis(300);
        let mut dead_addrs = Vec::new();
        for _ in 0..4 {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            dead_addrs.push(listener.local_addr().unwrap());
            drop(listener);
        }

        let start = std::time::Instant::now();
        let got = broadcast_and_collect(&dead_addrs, &a_vote_request(), deadline);
        let elapsed = start.elapsed();

        assert!(got.is_empty());
        assert!(
            elapsed < deadline * 3,
            "broadcast_and_collect took {elapsed:?} for 4 dead peers at deadline {deadline:?} — \
             looks sequential, not parallel"
        );
    }

    // ── S2d: respond_once (voter side) ──────────────────────────────────────────────────
    // The happy path (a valid VoteRequest in, a VoteResponse or EOF sentinel out) is already
    // exercised end-to-end by every connect_and_round_trip/broadcast_and_collect test above, all
    // of which go through spawn_one_shot_voter -> respond_once. This section covers the one path
    // those don't: a malformed peer that never sends a valid request at all.

    #[test]
    fn respond_once_on_a_peer_that_sends_the_eof_sentinel_instead_of_a_request_is_an_io_error() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || respond_once(&listener, |_req| None));

        let mut stream = std::net::TcpStream::connect(addr).unwrap();
        write_frame(&mut stream, b"").unwrap(); // EOF sentinel where a VoteRequest was expected

        let result = handle.join().unwrap();
        assert!(result.is_err());
    }
}
