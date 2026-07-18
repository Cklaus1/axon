//! R34 — Incremental attestation: rolling hash chain over `axon-vm run` invocations.
//!
//! Extends the R31 `axtcb1-ext:` boot measurement into an append-only, per-run
//! hash chain (`governance/specs/R34-incremental-attestation.md` §4.2). Every
//! chain link binds: the previous chain tip, the program's SHA-256, a run-id,
//! and a timestamp, so removing / substituting / reordering any run is detected
//! by `ChainStore::verify`.
//!
//! Preimage (spec §4.2, byte-for-byte, MUST NOT be altered without a version
//! bump to `axon-run-v2\n`):
//! ```text
//! preimage = b"axon-run-v1\n"       // 12 bytes — version tag (length-extension guard)
//!         || prev_chain_bytes        // 32 bytes — decoded from prev_hash's hex body
//!         || prog_hash_bytes         // 32 bytes — decoded from prog_hash's hex body
//!         || run_id.as_bytes()       // variable — UTF-8 run-id string
//!         || timestamp_ms.to_le_bytes() // 8 bytes — u64 little-endian
//! entry_hash = "axtcb1-run:" + hex(sha256(preimage))
//! ```
//! `prev_hash` is either the R31 genesis (`"axtcb1-ext:" + hex`) for seq 0, or a
//! previous entry's `entry_hash` (`"axtcb1-run:" + hex`) for seq > 0 — exactly
//! one of those two prefixes is stripped before decoding; this module is total
//! (never panics) even over a corrupted/adversarial prefix or hex body: an
//! unrecognized prefix or malformed hex decodes to a deterministic all-zero
//! sentinel, which simply fails the downstream hash comparison in `verify()`
//! rather than aborting the process on attacker-controlled file content.

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Protocol version tag — prepended (not appended) so a length-extension attack
/// cannot forge a valid next link from a known `entry_hash` alone (spec §4.2).
const VERSION_TAG: &[u8] = b"axon-run-v1\n";

/// One link in the rolling hash chain (spec §3.1, simplified field set).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainEntry {
    pub seq: u64,
    pub run_id: String,
    pub prog_hash: String,
    pub timestamp_ms: u64,
    pub prev_hash: String,
    pub entry_hash: String,
}

/// Decode the 32-byte body of a `prev_hash`/genesis string after stripping the
/// `"axtcb1-run:"` or `"axtcb1-ext:"` prefix. Total: any unrecognized prefix or
/// malformed/short hex decodes to `[0u8; 32]` rather than panicking — a
/// verifier still catches this deterministically via the resulting hash
/// mismatch, so no adversarial file content can abort the process.
fn decode_prev_bytes(prev_hash: &str) -> [u8; 32] {
    let body = prev_hash
        .strip_prefix("axtcb1-run:")
        .or_else(|| prev_hash.strip_prefix("axtcb1-ext:"))
        .unwrap_or(prev_hash);
    let mut out = [0u8; 32];
    if let Ok(bytes) = hex::decode(body) {
        if bytes.len() == 32 {
            out.copy_from_slice(&bytes);
        }
    }
    out
}

/// Decode a hex-encoded SHA-256 digest string to 32 bytes; `[0u8; 32]` on any
/// malformed input (same total-function discipline as `decode_prev_bytes`).
fn decode_hash_hex(hash_hex: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    if let Ok(bytes) = hex::decode(hash_hex) {
        if bytes.len() == 32 {
            out.copy_from_slice(&bytes);
        }
    }
    out
}

/// SHA-256 of `bytes`, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    hex::encode(digest)
}

/// SHA-256 of a file's contents, lowercase hex.
pub fn sha256_file(path: &Path) -> io::Result<String> {
    let bytes = fs::read(path)?;
    Ok(sha256_hex(&bytes))
}

/// Compute the next chain link's `entry_hash` per spec §4.2, byte-for-byte.
///
/// `prev_hash_hex` is the previous tip (`"axtcb1-ext:…"` for genesis or
/// `"axtcb1-run:…"` for a prior entry); `prog_hash_hex` is `sha256_file`'s
/// output for the program source. Pure and total — same inputs always produce
/// the same output, and malformed inputs never panic (see module docs).
pub fn compute_entry_hash(
    prev_hash_hex: &str,
    prog_hash_hex: &str,
    run_id: &str,
    timestamp_ms: u64,
) -> String {
    let prev_bytes = decode_prev_bytes(prev_hash_hex);
    let prog_bytes = decode_hash_hex(prog_hash_hex);

    let mut preimage = Vec::with_capacity(VERSION_TAG.len() + 32 + 32 + run_id.len() + 8);
    preimage.extend_from_slice(VERSION_TAG);
    preimage.extend_from_slice(&prev_bytes);
    preimage.extend_from_slice(&prog_bytes);
    preimage.extend_from_slice(run_id.as_bytes());
    preimage.extend_from_slice(&timestamp_ms.to_le_bytes());

    let digest: [u8; 32] = Sha256::digest(&preimage).into();
    format!("axtcb1-run:{}", hex::encode(digest))
}

/// An append-only JSONL chain file, one `ChainEntry` per line.
pub struct ChainStore {
    pub path: PathBuf,
}

impl ChainStore {
    pub fn new(path: &Path) -> Self {
        ChainStore { path: path.to_path_buf() }
    }

    /// Read every line, parsed. Each element is `(line_index, parse_result)` so
    /// a malformed line can be reported at its real position instead of being
    /// silently skipped (Gate 4 case 8: malformed JSON is a clear error, not a
    /// panic and not a silent drop).
    fn read_lines(&self) -> io::Result<Vec<(u64, Result<ChainEntry, String>)>> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let f = File::open(&self.path)?;
        let reader = BufReader::new(f);
        let mut out = Vec::new();
        for (i, line) in reader.lines().enumerate() {
            let line = line?;
            if line.trim().is_empty() {
                continue;
            }
            match serde_json::from_str::<ChainEntry>(&line) {
                Ok(e) => out.push((i as u64, Ok(e))),
                Err(e) => out.push((i as u64, Err(e.to_string()))),
            }
        }
        Ok(out)
    }

    /// The last entry's `(next_seq, entry_hash)`, or `(0, genesis_hash)` if the
    /// chain file is empty or missing.
    pub fn last_entry(&self, genesis_hash: &str) -> io::Result<(u64, String)> {
        let lines = self.read_lines()?;
        match lines.last() {
            None => Ok((0, genesis_hash.to_string())),
            Some((_idx, Ok(e))) => Ok((e.seq + 1, e.entry_hash.clone())),
            Some((idx, Err(msg))) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("malformed chain line {idx}: {msg}"),
            )),
        }
    }

    /// Append one entry via `O_APPEND | O_CREAT` — never truncates or rewrites
    /// an existing line (append-only discipline, spec §4.4).
    pub fn append(&self, entry: &ChainEntry) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let line = serde_json::to_string(entry)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    /// Verify the whole chain from `genesis_hash`, recomputing every link.
    ///
    /// `Ok(count)` = every entry's `prev_hash` links to its predecessor's
    /// `entry_hash` (or `genesis_hash` for seq 0) AND its `entry_hash` matches
    /// the recomputed formula. `Err(seq)` names the FIRST broken seq (never the
    /// last) — a tamper later in the file must not mask one earlier.
    pub fn verify(&self, genesis_hash: &str) -> Result<u64, u64> {
        let lines = match self.read_lines() {
            Ok(v) => v,
            Err(_) => return Err(0),
        };
        if lines.is_empty() {
            return Ok(0);
        }

        let mut entries = Vec::with_capacity(lines.len());
        for (idx, parsed) in lines {
            match parsed {
                Ok(e) => entries.push(e),
                // Malformed JSON mid-file: report the line index as the break
                // point immediately — must NOT fall through to verify_entries
                // with a truncated/empty Vec, which would silently report
                // "verifies OK" over a corrupted file (I-9).
                Err(_) => return Err(idx),
            }
        }
        verify_entries(&entries, genesis_hash)
    }

    /// Export the full chain as a self-contained [`ChainExport`] (R34 Slice 4,
    /// spec §5.4): everything an auditor needs to call [`verify_export`]
    /// without a live VM — just this struct plus the program source files.
    // Not yet called from main.rs — the `axon-vm chain export`/`verify-export`
    // CLI subcommands are R34 Slice 6 (separate, depends on this slice), not
    // yet landed. Exercised by tests below; dead_code would otherwise fire on
    // this binary crate since nothing outside #[cfg(test)] calls it yet.
    #[allow(dead_code)]
    pub fn export(
        &self,
        vm_id: &str,
        genesis_hash: &str,
        exported_at_ms: u64,
    ) -> io::Result<ChainExport> {
        let lines = self.read_lines()?;
        let mut entries = Vec::with_capacity(lines.len());
        for (idx, parsed) in lines {
            match parsed {
                Ok(e) => entries.push(e),
                Err(msg) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("malformed chain line {idx}: {msg}"),
                    ));
                }
            }
        }
        let head = entries
            .last()
            .map(|e| e.entry_hash.clone())
            .unwrap_or_else(|| genesis_hash.to_string());
        Ok(ChainExport {
            schema: CHAIN_EXPORT_SCHEMA.to_string(),
            vm_id: vm_id.to_string(),
            boot_root: genesis_hash.to_string(),
            head,
            exported_at_ms,
            entries,
        })
    }
}

/// Schema tag for [`ChainExport`] (spec §5.4).
pub const CHAIN_EXPORT_SCHEMA: &str = "axon-chain-export/1";

/// A self-contained, JSON-serializable export of a full chain (R34 Slice 4,
/// spec §5.4) — an auditor needs only this struct plus the program source
/// files to call [`verify_export`]; no live VM or `ChainStore` required.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChainExport {
    pub schema: String,
    pub vm_id: String,
    pub boot_root: String,
    pub head: String,
    pub exported_at_ms: u64,
    pub entries: Vec<ChainEntry>,
}

/// Shared verification core (spec §4.3) — the ONE mechanism both
/// [`ChainStore::verify`] (reads from a live `chain.jsonl`) and
/// [`verify_export`] (reads from an already-parsed [`ChainExport`]) delegate
/// to, so the two call paths cannot drift apart (no second mechanism, I-2).
fn verify_entries(entries: &[ChainEntry], genesis_hash: &str) -> Result<u64, u64> {
    if entries.is_empty() {
        return Ok(0);
    }
    let mut prev = genesis_hash.to_string();
    let mut count = 0u64;
    for entry in entries {
        if entry.prev_hash != prev {
            return Err(entry.seq);
        }
        let recomputed =
            compute_entry_hash(&prev, &entry.prog_hash, &entry.run_id, entry.timestamp_ms);
        if recomputed != entry.entry_hash {
            return Err(entry.seq);
        }
        prev = entry.entry_hash.clone();
        count += 1;
    }
    Ok(count)
}

/// Verify an exported chain (spec §5.4/§4.3) — the auditor-side counterpart of
/// [`ChainStore::verify`], operating on an already-parsed [`ChainExport`]
/// (e.g. read back from `chain_export.json`) instead of a live chain file. No
/// live VM or filesystem access to the original chain is required — this is
/// what makes an export "self-contained" per the spec.
///
/// `Ok(count)` = every entry verifies against [`ChainExport::boot_root`] and
/// the recomputed tip equals [`ChainExport::head`]. `Err(seq)` names the FIRST
/// broken seq, same contract as [`ChainStore::verify`].
// Not yet called from main.rs — see the note on `ChainStore::export` above
// (R34 Slice 6, CLI wiring, not yet landed). Exercised by tests below.
#[allow(dead_code)]
pub fn verify_export(export: &ChainExport) -> Result<u64, u64> {
    let count = verify_entries(&export.entries, &export.boot_root)?;
    let recomputed_head = export
        .entries
        .last()
        .map(|e| e.entry_hash.clone())
        .unwrap_or_else(|| export.boot_root.clone());
    if recomputed_head != export.head {
        // The tip itself was tampered with (or the export is stale) even
        // though every individual link recomputes correctly — treat this as
        // breaking at the entry count (one past the last verified link), the
        // same "name the point it broke" discipline as every other Err(seq).
        return Err(count);
    }
    Ok(count)
}

// ── Tests (Gate 2/4 — every case below was seen RED before chain.rs existed) ──

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const GENESIS: &str = "axtcb1-ext:0000000000000000000000000000000000000000000000000000000000000001";
    const PROG_A: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const PROG_B: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

    /// Case 1: `compute_entry_hash` is deterministic for identical inputs.
    #[test]
    fn entry_hash_deterministic() {
        let h1 = compute_entry_hash(GENESIS, PROG_A, "run-1", 1_000);
        let h2 = compute_entry_hash(GENESIS, PROG_A, "run-1", 1_000);
        assert_eq!(h1, h2, "same inputs must produce byte-identical entry_hash");
        assert!(h1.starts_with("axtcb1-run:"), "entry_hash must carry the axtcb1-run: prefix");
    }

    /// Case 2: different `prog_hash` -> different `entry_hash` (no truncation/collision).
    #[test]
    fn different_prog_hash_different_entry_hash() {
        let h_a = compute_entry_hash(GENESIS, PROG_A, "run-1", 1_000);
        let h_b = compute_entry_hash(GENESIS, PROG_B, "run-1", 1_000);
        assert_ne!(h_a, h_b, "different prog_hash must produce a different entry_hash");
    }

    /// R31 composition: genesis is fed in with the `axtcb1-ext:` prefix and the
    /// resulting link still carries the `axtcb1-run:` prefix distinctly.
    #[test]
    fn chain_composes_with_r31() {
        assert!(GENESIS.starts_with("axtcb1-ext:"), "test genesis must use the R31 prefix");
        let h = compute_entry_hash(GENESIS, PROG_A, "run-0", 0);
        assert!(h.starts_with("axtcb1-run:"));
        assert_ne!(h, GENESIS, "the run-chain link must differ from the genesis root");
    }

    /// Case 3: append 3 entries, verify -> Ok(3).
    #[test]
    fn verify_ok_three_entries() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));

        let mut prev = GENESIS.to_string();
        for i in 0..3u64 {
            let prog_hash = sha256_hex(format!("program-{i}").as_bytes());
            let run_id = format!("run-{i}");
            let ts = 1_000 + i;
            let entry_hash = compute_entry_hash(&prev, &prog_hash, &run_id, ts);
            store
                .append(&ChainEntry {
                    seq: i,
                    run_id,
                    prog_hash,
                    timestamp_ms: ts,
                    prev_hash: prev.clone(),
                    entry_hash: entry_hash.clone(),
                })
                .unwrap();
            prev = entry_hash;
        }

        assert_eq!(store.verify(GENESIS), Ok(3), "3 well-formed entries must verify OK");
    }

    /// Case 4: corrupt seq 1's `entry_hash` -> Err(1), the FIRST broken link,
    /// not the last (seq 2's own link would also look wrong as a side effect,
    /// but the report must name seq 1).
    #[test]
    fn verify_detects_tampered_entry_hash() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chain.jsonl");
        let store = ChainStore::new(&path);

        let mut prev = GENESIS.to_string();
        let mut entries = Vec::new();
        for i in 0..3u64 {
            let prog_hash = sha256_hex(format!("program-{i}").as_bytes());
            let run_id = format!("run-{i}");
            let ts = 2_000 + i;
            let entry_hash = compute_entry_hash(&prev, &prog_hash, &run_id, ts);
            let entry = ChainEntry {
                seq: i,
                run_id,
                prog_hash,
                timestamp_ms: ts,
                prev_hash: prev.clone(),
                entry_hash: entry_hash.clone(),
            };
            store.append(&entry).unwrap();
            entries.push(entry);
            prev = entry_hash;
        }

        // Rewrite the file with seq 1's entry_hash flipped (one hex digit).
        let mut lines: Vec<String> = fs::read_to_string(&path)
            .unwrap()
            .lines()
            .map(|s| s.to_string())
            .collect();
        let mut tampered: ChainEntry = serde_json::from_str(&lines[1]).unwrap();
        let mut bad_hash = tampered.entry_hash.clone();
        // Flip the last hex character.
        let last = bad_hash.pop().unwrap();
        bad_hash.push(if last == '0' { '1' } else { '0' });
        tampered.entry_hash = bad_hash;
        lines[1] = serde_json::to_string(&tampered).unwrap();
        fs::write(&path, lines.join("\n") + "\n").unwrap();

        assert_eq!(
            store.verify(GENESIS),
            Err(1),
            "tampering seq 1's entry_hash must break at seq 1, not seq 2 or later"
        );
    }

    /// Case 5: empty file, verify -> Ok(0).
    #[test]
    fn verify_empty_chain_ok() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("does-not-exist.jsonl"));
        assert_eq!(store.verify(GENESIS), Ok(0), "missing/empty chain file verifies trivially OK");
    }

    /// Case 6: wrong genesis -> Err(0) (breaks at the very first link).
    #[test]
    fn verify_wrong_genesis_breaks_at_zero() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));

        let prog_hash = sha256_hex(b"program-0");
        let entry_hash = compute_entry_hash(GENESIS, &prog_hash, "run-0", 5_000);
        store
            .append(&ChainEntry {
                seq: 0,
                run_id: "run-0".to_string(),
                prog_hash,
                timestamp_ms: 5_000,
                prev_hash: GENESIS.to_string(),
                entry_hash,
            })
            .unwrap();

        let wrong_genesis = "axtcb1-ext:ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
        assert_eq!(
            store.verify(wrong_genesis),
            Err(0),
            "a genesis mismatch must break at seq 0"
        );
    }

    /// Case 7: known SHA-256 test vectors — confirms real hashing, not merely
    /// internally-consistent hashing. Both values independently reproduced via
    /// `printf '' | sha256sum` / `printf 'abc' | sha256sum` before writing this
    /// test (not copied from an unverified source).
    #[test]
    fn sha256_known_test_vector() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    /// Case 8: malformed JSON line mid-file -> clear error, not a panic.
    #[test]
    fn verify_malformed_json_line_is_clear_error() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("chain.jsonl");

        let prog_hash = sha256_hex(b"program-0");
        let entry_hash = compute_entry_hash(GENESIS, &prog_hash, "run-0", 6_000);
        let good_line = serde_json::to_string(&ChainEntry {
            seq: 0,
            run_id: "run-0".to_string(),
            prog_hash,
            timestamp_ms: 6_000,
            prev_hash: GENESIS.to_string(),
            entry_hash,
        })
        .unwrap();

        fs::write(&path, format!("{good_line}\nTHIS IS NOT JSON AT ALL\n")).unwrap();

        let store = ChainStore::new(&path);
        // Must return a clear Err, never panic (the test itself not panicking
        // is the proof — a real panic would abort the test process).
        let result = store.verify(GENESIS);
        assert!(result.is_err(), "malformed JSON line must be a clear Err, not a silent pass");
    }

    /// `ChainStore::last_entry` on an empty store returns the genesis as seq 0.
    #[test]
    fn last_entry_empty_store_is_genesis() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));
        assert_eq!(store.last_entry(GENESIS).unwrap(), (0, GENESIS.to_string()));
    }

    /// `sha256_file` matches `sha256_hex` over the same bytes.
    #[test]
    fn sha256_file_matches_sha256_hex() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("prog.ax");
        fs::write(&path, b"fn main() {}\n").unwrap();
        assert_eq!(sha256_file(&path).unwrap(), sha256_hex(b"fn main() {}\n"));
    }

    // ── R34 Slice 4: export / import (spec §5.4) ──────────────────────────────

    fn build_three_entry_chain(store: &ChainStore) -> Vec<ChainEntry> {
        let mut prev = GENESIS.to_string();
        let mut entries = Vec::new();
        for i in 0..3u64 {
            let prog_hash = sha256_hex(format!("export-program-{i}").as_bytes());
            let run_id = format!("export-run-{i}");
            let ts = 3_000 + i;
            let entry_hash = compute_entry_hash(&prev, &prog_hash, &run_id, ts);
            let entry = ChainEntry {
                seq: i,
                run_id,
                prog_hash,
                timestamp_ms: ts,
                prev_hash: prev.clone(),
                entry_hash: entry_hash.clone(),
            };
            store.append(&entry).unwrap();
            entries.push(entry);
            prev = entry_hash;
        }
        entries
    }

    /// `chain_exported_and_imported` (spec §5.4): export -> serialize to JSON
    /// -> re-read the JSON -> `verify_export` over the entries with `boot_root`
    /// from the JSON -> `Ok`. The export is self-contained: a verifier needs
    /// only the JSON, not a live `ChainStore`.
    #[test]
    fn chain_exported_and_imported() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));
        let entries = build_three_entry_chain(&store);

        let export = store.export("vm-alpha", GENESIS, 1_751_000_000_000).unwrap();
        assert_eq!(export.schema, CHAIN_EXPORT_SCHEMA);
        assert_eq!(export.vm_id, "vm-alpha");
        assert_eq!(export.boot_root, GENESIS);
        assert_eq!(export.head, entries.last().unwrap().entry_hash);
        assert_eq!(export.entries, entries);

        // Round-trip through JSON, as an auditor with only the exported file
        // (not a live ChainStore) would.
        let json = serde_json::to_string(&export).unwrap();
        let reimported: ChainExport = serde_json::from_str(&json).unwrap();
        assert_eq!(reimported, export, "JSON round-trip must be lossless");

        assert_eq!(
            verify_export(&reimported),
            Ok(3),
            "a genuine, untampered export must verify OK over exactly its own entries"
        );
    }

    /// An export of an empty chain has `head == boot_root` and verifies
    /// trivially (mirrors `verify_empty_chain_ok` for the live-store path).
    #[test]
    fn empty_chain_export_verifies_ok() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));
        let export = store.export("vm-empty", GENESIS, 1_751_000_000_000).unwrap();
        assert_eq!(export.head, GENESIS);
        assert!(export.entries.is_empty());
        assert_eq!(verify_export(&export), Ok(0));
    }

    /// Tampering an exported entry's `entry_hash` after the fact must be
    /// caught by `verify_export`, at the same seq the live-store path would
    /// report (mirrors `verify_detects_tampered_entry_hash`).
    #[test]
    fn verify_export_detects_tampered_entry() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));
        build_three_entry_chain(&store);
        let mut export = store.export("vm-alpha", GENESIS, 1_751_000_000_000).unwrap();

        let mut bad_hash = export.entries[1].entry_hash.clone();
        let last = bad_hash.pop().unwrap();
        bad_hash.push(if last == '0' { '1' } else { '0' });
        export.entries[1].entry_hash = bad_hash;

        assert_eq!(
            verify_export(&export),
            Err(1),
            "tampering seq 1's entry_hash in the export must break at seq 1"
        );
    }

    /// A tampered `head` (the claimed tip doesn't match what the entries
    /// actually recompute to) must be caught even when every individual
    /// entry link is internally consistent — otherwise a malicious exporter
    /// could truncate entries and claim a stale/forged tip.
    #[test]
    fn verify_export_detects_tampered_head() {
        let dir = tempdir().unwrap();
        let store = ChainStore::new(&dir.path().join("chain.jsonl"));
        build_three_entry_chain(&store);
        let mut export = store.export("vm-alpha", GENESIS, 1_751_000_000_000).unwrap();

        let mut bad_head = export.head.clone();
        let last = bad_head.pop().unwrap();
        bad_head.push(if last == '0' { '1' } else { '0' });
        export.head = bad_head;

        assert_eq!(
            verify_export(&export),
            Err(3),
            "a forged head must be caught even though every individual link recomputes cleanly"
        );
    }
}
