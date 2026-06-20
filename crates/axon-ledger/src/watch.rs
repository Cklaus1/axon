use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::ingest::edge::infer_edges;
use crate::ingest::session::{ingest_session, GateOptions};
use crate::store::Store;

/// Watch a directory for new Claude Code session JSONL files and ingest them automatically.
///
/// Polls `watch_dir` every `interval` seconds. For each new `.jsonl` file found,
/// calls `ingest_session` then `infer_edges`. Already-ingested files are skipped via
/// the ledger's own dedup logic.
///
/// Returns only on error — intended to run as a blocking loop or in a background thread.
pub fn watch_sessions(
    watch_dir: &Path,
    ledger_dir: &Path,
    interval: Duration,
    gate: &GateOptions,
    verbose: bool,
) -> Result<()> {
    // Track files we've already attempted this run (suppresses re-logging skips)
    let mut seen: HashSet<PathBuf> = HashSet::new();
    let mut store = Store::open(ledger_dir)?;

    // Seed seen with files already present so we only react to new arrivals
    for entry in fs::read_dir(watch_dir)? {
        let path = entry?.path();
        if path.extension().and_then(|e| e.to_str()) == Some("jsonl") {
            seen.insert(path);
        }
    }

    if verbose {
        eprintln!("[ledger-watch] watching {} (interval {}s)", watch_dir.display(), interval.as_secs());
        eprintln!("[ledger-watch] {} existing session files suppressed", seen.len());
    }

    loop {
        thread::sleep(interval);

        let entries = match fs::read_dir(watch_dir) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[ledger-watch] read_dir error: {e}");
                continue;
            }
        };

        let mut new_ingested = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            if seen.contains(&path) {
                continue;
            }
            seen.insert(path.clone());

            match ingest_session(&path, &mut store, gate, None) {
                Ok(Some(r)) => {
                    if verbose {
                        let goal = r.payload.get("goal").and_then(|v| v.as_str()).unwrap_or("?");
                        eprintln!("[ledger-watch] ingested {} — goal: {}", &r.id[..8], goal);
                    }
                    new_ingested += 1;
                }
                Ok(None) => {} // already in store
                Err(e) => {
                    eprintln!("[ledger-watch] skipped {}: {e}", path.display());
                }
            }
        }

        if new_ingested > 0 {
            match infer_edges(&mut store) {
                Ok(n) => {
                    if verbose {
                        eprintln!("[ledger-watch] inferred {n} new edges after {new_ingested} new sessions");
                    }
                }
                Err(e) => eprintln!("[ledger-watch] edge inference error: {e}"),
            }
        }
    }
}
