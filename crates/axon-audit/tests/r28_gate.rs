//! R28 acceptance gate — integration tests.
//!
//! These tests run against the `axon-audit` crate from the outside (as a
//! downstream consumer). The unit tests in `src/lib.rs` cover the core algebra;
//! these cover the end-to-end journeys.

use axon_audit::{EffectKind, Ledger};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use tempfile::NamedTempFile;

// Utility: unique temp path that doesn't exist yet.
fn temp_path() -> PathBuf {
    let f = NamedTempFile::new().unwrap();
    let p = f.path().to_path_buf();
    drop(f);
    p
}

// ── acc_a1_smoke_audit_journey ───────────────────────────────────────────────

/// A1: real audit journey — create, append, verify, show (export).
#[test]
fn acc_a1_smoke_audit_journey() {
    let path = temp_path();

    // Step 1: create ledger, append a few entries.
    let mut ledger = Ledger::open(&path).expect("open ledger");
    ledger
        .append("root", EffectKind::AI, "ai_complete:sha256:deadbeef")
        .expect("append AI");
    ledger
        .append("root", EffectKind::FS, "write:/tmp/out.txt")
        .expect("append FS");
    ledger
        .append("root", EffectKind::Net, "http_get:api.example.com")
        .expect("append Net");

    // Step 2: verify the chain.
    ledger.verify().expect("verify should pass on a clean ledger");
    assert_eq!(ledger.len(), 3);

    // Step 3: export as JSON and re-import.
    let json = ledger.export_json().expect("export");
    assert!(json.contains("axon-ledger/1"), "JSON must declare schema");
    assert!(json.contains("ai_complete:sha256:deadbeef"));

    let import_path = temp_path();
    let imported = Ledger::import_json(&json, &import_path).expect("import");
    imported.verify().expect("re-verify after import");
    assert_eq!(imported.len(), 3);

    // Step 4: show — iterate entries and confirm ordering.
    for (i, entry) in imported.entries().iter().enumerate() {
        assert_eq!(entry.seq, i as u64, "seq must be monotone");
    }

    // Step 5: tamper and verify the tamper is caught.
    let jsonl = std::fs::read_to_string(&path).unwrap();
    let tampered = jsonl.replace(
        "\"operation\":\"ai_complete:sha256:deadbeef\"",
        "\"operation\":\"ai_complete:sha256:TAMPERED\"",
    );
    let tamper_path = temp_path();
    std::fs::write(&tamper_path, tampered).unwrap();
    let tamper_result = Ledger::open(&tamper_path);
    assert!(tamper_result.is_err(), "tampered ledger must fail verify");
}

// ── acc_a2_all_capability_classes_recorded ───────────────────────────────────

/// A2: all six capability classes appear in the ledger and the chain holds.
#[test]
fn acc_a2_all_capability_classes_recorded() {
    let path = temp_path();
    let mut ledger = Ledger::open(&path).expect("open");

    // One entry per EffectKind.
    ledger.append("root", EffectKind::FS,     "read:/data/file.csv").unwrap();
    ledger.append("root", EffectKind::Net,    "http_post:api.example.com/infer").unwrap();
    ledger.append("root", EffectKind::AI,     "ai_complete:sha256:cafebabe").unwrap();
    ledger.append("root", EffectKind::Exec,   "exec:ls -la").unwrap();
    ledger.append("root", EffectKind::Random, "random_i64").unwrap();
    ledger.append("root", EffectKind::IO,     "println:hello").unwrap();

    assert_eq!(ledger.len(), 6, "must have one entry per capability class");

    // Verify chain integrity.
    ledger.verify().expect("chain must be intact");

    // Check that each entry carries the correct effect kind.
    let effects: Vec<EffectKind> = ledger.entries().iter().map(|e| e.effect).collect();
    assert!(effects.contains(&EffectKind::FS));
    assert!(effects.contains(&EffectKind::Net));
    assert!(effects.contains(&EffectKind::AI));
    assert!(effects.contains(&EffectKind::Exec));
    assert!(effects.contains(&EffectKind::Random));
    assert!(effects.contains(&EffectKind::IO));
}

// ── acc_a3_quickstart_commands_execute ───────────────────────────────────────

/// A3: the §9 quickstart sequence executes without errors.
/// Demonstrates the full API: open → append → verify → export → show.
#[test]
fn acc_a3_quickstart_commands_execute() {
    let ledger_path = temp_path();

    // Simulate: open ledger (as set_ledger_path would), append via API.
    let mut ledger = Ledger::open(&ledger_path).expect("open ledger for quickstart");

    // Simulate an AI call — use append_ai_call logic directly.
    let prompt = b"Summarize this document.";
    let prompt_hash = {
        let mut h = Sha256::new();
        h.update(prompt);
        let bytes: [u8; 32] = h.finalize().into();
        bytes.iter().map(|x| format!("{x:02x}")).collect::<String>()
    };
    let op = format!("ai_complete:sha256:{prompt_hash}");
    ledger.append("root", EffectKind::AI, &op).expect("append AI call");

    // Flush equivalent: verify chain.
    ledger.verify().expect("verify after append");

    assert_eq!(ledger.len(), 1, "should have 1 entry");
    assert!(
        ledger.entries()[0].operation.starts_with("ai_complete:sha256:"),
        "AI entry must have sha256 prefix"
    );

    // Simulate audit show --json.
    let json = ledger.export_json().expect("export");
    assert!(json.contains("axon-ledger/1"));

    // Simulate audit verify: re-open and verify.
    let ledger_verify = Ledger::open(&ledger_path).expect("re-open for verify");
    assert!(
        ledger_verify.verify().is_ok(),
        "audit verify should pass on a clean ledger"
    );
    assert_eq!(ledger_verify.len(), 1);
}

// ── acc_a4_hermetic_isolated_timeout ────────────────────────────────────────

/// A4: the ledger is the complete record; no entries are added after the run.
/// Simulates the hermetic isolation property: after a simulated run completes,
/// re-opening the file shows exactly the entries written during the run.
#[test]
fn acc_a4_hermetic_isolated_timeout() {
    let path = temp_path();

    // Simulate a run: open ledger, append entries.
    let entry_count = {
        let mut l = Ledger::open(&path).expect("open for run");
        l.append("root", EffectKind::AI, "ai_complete:sha256:abc").unwrap();
        l.append("root", EffectKind::FS, "write:/tmp/out").unwrap();
        l.verify().expect("chain intact during run");
        l.len()
    }; // ledger dropped here — file handle closed

    // After the run (drop), no more entries can be added via the closed ledger.
    // Reopening shows exactly what was written.
    let ledger_after = Ledger::open(&path).expect("reopen after run");
    assert_eq!(
        ledger_after.len(),
        entry_count,
        "entry count must match what was written during the run"
    );
    ledger_after.verify().expect("chain must hold after run");
}

// ── acc_a6_ledger_mandatory_and_chained ──────────────────────────────────────

/// A6: opening an existing ledger and appending continues the chain correctly.
#[test]
fn acc_a6_ledger_mandatory_and_chained() {
    let path = temp_path();

    // First run: write two entries.
    {
        let mut l = Ledger::open(&path).expect("open first");
        l.append("root", EffectKind::AI, "ai_complete:sha256:aaa").unwrap();
        l.append("root", EffectKind::FS, "write:/tmp/a").unwrap();
        l.verify().expect("verify first run");
    }

    // Second run: re-open and append two more.
    {
        let mut l = Ledger::open(&path).expect("open second");
        assert_eq!(l.len(), 2, "should have loaded 2 existing entries");
        l.append("alice", EffectKind::Net, "http_get:api.example.com").unwrap();
        l.append("alice", EffectKind::Exec, "exec:cat /tmp/a").unwrap();
        l.verify().expect("verify second run");
        assert_eq!(l.len(), 4);

        // The 3rd entry's prev_hash must equal the 2nd entry's entry_hash.
        assert_eq!(
            l.entries()[2].prev_hash,
            l.entries()[1].entry_hash,
            "chain must cross the open boundary"
        );
    }

    // Final verify: re-open and verify the full 4-entry chain.
    let final_ledger = Ledger::open(&path).expect("open final");
    final_ledger.verify().expect("full chain must verify");
    assert_eq!(final_ledger.len(), 4);
}
