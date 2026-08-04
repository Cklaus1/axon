//! R33 — JSON (de)serialization + file I/O for `VoteRequest`/`VoteResponse`.
//!
//! This is the I/O seam: `logic.rs` stays pure (no filesystem), this module owns
//! every `fs::` call the quorum CLI subcommands need. A malformed vote file on
//! disk (hand-edited, truncated, or from an incompatible version) must produce a
//! clear `io::Error`, never a panic — an unsupervised host walking a directory of
//! peer votes cannot afford to crash on one bad file (BUILD_PROTOCOL.md Gate 4
//! "adversarial: malformed input ... fail gracefully").

use std::fs;
use std::io;
use std::path::Path;

use super::logic::{VoteRequest, VoteResponse};

/// Write a `VoteRequest` to `path` as pretty JSON.
pub fn write_vote_request(req: &VoteRequest, path: &Path) -> io::Result<()> {
    let json = serde_json::to_string_pretty(req)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, json)
}

/// Read a `VoteRequest` from `path`. A malformed/missing file is a clear `io::Error`.
pub fn read_vote_request(path: &Path) -> io::Result<VoteRequest> {
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed VoteRequest at {}: {e}", path.display()),
        )
    })
}

/// Write a `VoteResponse` to `path` as pretty JSON.
pub fn write_vote_response(resp: &VoteResponse, path: &Path) -> io::Result<()> {
    let json = serde_json::to_string_pretty(resp)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    fs::write(path, json)
}

/// Read a `VoteResponse` from `path`. A malformed/missing file is a clear `io::Error`.
pub fn read_vote_response(path: &Path) -> io::Result<VoteResponse> {
    let s = fs::read_to_string(path)?;
    serde_json::from_str(&s).map_err(|e| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("malformed VoteResponse at {}: {e}", path.display()),
        )
    })
}

/// Collect every `.vote` response file from `dir`, sorted by filename for
/// determinism (spec A5 — the same vote set in any collection order must
/// produce the same `QuorumResult`; sorting removes directory-iteration-order
/// as a source of nondeterminism).
///
/// `n_hint` is the operator-expected fleet size, used only to pre-size the
/// output `Vec` — it does not filter or truncate; every `.vote` file present is
/// read and returned (or the whole call fails with a clear `io::Error`).
///
/// A malformed vote file produces `Err(io::Error)` (kind `InvalidData`) —
/// never a panic — so one corrupt file can't take down the whole quorum check.
pub fn collect_responses(dir: &Path, n_hint: usize) -> io::Result<Vec<VoteResponse>> {
    let mut paths: Vec<_> = fs::read_dir(dir)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|e| e.path())
        .filter(|p| p.extension().map(|ext| ext == "vote").unwrap_or(false))
        .collect();
    paths.sort();

    let mut out = Vec::with_capacity(n_hint.max(paths.len()));
    for p in &paths {
        out.push(read_vote_response(p)?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::quorum::logic::{VoteRequest, VoteResponse};

    fn tmp_dir(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "axon-r33-io-test-{tag}-{}-{}",
            std::process::id(),
            tag.len() // cheap extra uniqueness without a new dep
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// Round-trip: write then read must reproduce the same `VoteRequest`.
    #[test]
    fn vote_request_json_round_trip() {
        let dir = tmp_dir("req-roundtrip");
        let path = dir.join("req.json");
        let req = VoteRequest {
            run_id: "run-abc-123".to_string(),
            prog_hash: "sha256:deadbeef".to_string(),
            voter_tcb: "axtcb1-ext:cafef00d".to_string(),
            proposed_action: "wire_transfer(amount=1000000)".to_string(),
            timestamp_ms: 1_700_000_000_000,
        };
        write_vote_request(&req, &path).expect("write must succeed");
        let read_back = read_vote_request(&path).expect("read must succeed");
        assert_eq!(req.run_id, read_back.run_id);
        assert_eq!(req.prog_hash, read_back.prog_hash);
        assert_eq!(req.voter_tcb, read_back.voter_tcb);
        assert_eq!(req.proposed_action, read_back.proposed_action);
        assert_eq!(req.timestamp_ms, read_back.timestamp_ms);
        let _ = fs::remove_dir_all(&dir);
    }

    /// Round-trip: write then read must reproduce the same `VoteResponse`.
    #[test]
    fn vote_response_json_round_trip() {
        let dir = tmp_dir("resp-roundtrip");
        let path = dir.join("resp.json");
        let resp = VoteResponse {
            voter_tcb: "axtcb1-ext:cafef00d".to_string(),
            run_id: "run-abc-123".to_string(),
            approved: true,
            reason: "policy score 82 >= threshold 75".to_string(),
            lineage_root: "principal-alpha".to_string(),
        };
        write_vote_response(&resp, &path).expect("write must succeed");
        let read_back = read_vote_response(&path).expect("read must succeed");
        assert_eq!(resp.voter_tcb, read_back.voter_tcb);
        assert_eq!(resp.run_id, read_back.run_id);
        assert_eq!(resp.approved, read_back.approved);
        assert_eq!(resp.reason, read_back.reason);
        assert_eq!(resp.lineage_root, read_back.lineage_root);
        let _ = fs::remove_dir_all(&dir);
    }

    /// `collect_responses` reads every `.vote` file in the dir, sorted, ignoring
    /// non-`.vote` files.
    #[test]
    fn collect_responses_reads_all_vote_files_sorted() {
        let dir = tmp_dir("collect-sorted");
        for (i, approved) in [(0, true), (1, false), (2, true)] {
            let resp = VoteResponse {
                voter_tcb: "axtcb1-ext:same".to_string(),
                run_id: "run-x".to_string(),
                approved,
                reason: String::new(),
                lineage_root: format!("principal-{i}"),
            };
            write_vote_response(&resp, &dir.join(format!("voter-{i}.vote"))).unwrap();
        }
        // A non-.vote file must be ignored, not read as a vote.
        fs::write(dir.join("notes.txt"), "not a vote").unwrap();

        let votes = collect_responses(&dir, 3).expect("collect must succeed");
        assert_eq!(
            votes.len(),
            3,
            "must read exactly the 3 .vote files, ignoring notes.txt"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// Gate-4 adversarial case: a malformed `.vote` file produces a clear
    /// `io::Error`, not a panic, and the error identifies the offending file.
    #[test]
    fn collect_responses_malformed_file_is_io_error_not_panic() {
        let dir = tmp_dir("collect-malformed");
        let good = VoteResponse {
            voter_tcb: "axtcb1-ext:same".to_string(),
            run_id: "run-x".to_string(),
            approved: true,
            reason: String::new(),
            lineage_root: "principal-0".to_string(),
        };
        write_vote_response(&good, &dir.join("voter-0.vote")).unwrap();
        // Hand-corrupt a second file: not valid JSON at all.
        fs::write(dir.join("voter-1.vote"), b"{ this is not json !! ").unwrap();

        let result = collect_responses(&dir, 2);
        assert!(
            result.is_err(),
            "a malformed vote file must produce Err, not a panic and not a silently-skipped vote"
        );
        let err = result.unwrap_err();
        assert_eq!(
            err.kind(),
            io::ErrorKind::InvalidData,
            "must be a clear InvalidData error"
        );
        assert!(
            err.to_string().contains("voter-1.vote") || err.to_string().contains("malformed"),
            "error must point at the offending file or say 'malformed': {err}"
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
