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
//! NOT built yet (later S2 sub-slices, per the spec's own sizing note): opening a real socket
//! (`AF_VSOCK` or the TCP-loopback CI swap), the broadcast/collect fan-out loop with a deadline,
//! the listen/accept/respond loop, and the `axon-vm quorum vote --listen` / `propose --broadcast`
//! CLI flags. This module only has to be correct for whatever stream those slices eventually hand
//! it — proven here against an in-memory buffer, which exercises the identical code path a real
//! `TcpStream` would.

// Deliberately unwired to any caller yet — S2a lands the wire protocol alone, proven by its own
// unit tests below; S2b wires it into a real broadcast/listen loop (see the module doc comment
// above and R33 spec §5.2.2). Remove this allow once that caller exists.
#![allow(dead_code)]

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
}
