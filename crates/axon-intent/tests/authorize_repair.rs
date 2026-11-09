//! NH-R1 A2 (`tasks/unattended-auth-spec.md` §5.1): `axon-intent
//! authorize-repair`, its `--export-verify-key` and `--init-spent-store`
//! forms, driven as an operator would (subprocess).
//!
//! The cortex preview is a FAKE here: a shell script that records its
//! arguments and prints a fixed `cortex-envelope-preview/2` document. That
//! is the point of the test: the minter must judge the preview it is handed
//! (gate rows, check-digest consistency, symbol and check echo), never trust
//! it. The operator key is a TEST key derived from a fixed, public string.

mod common;
#[path = "../../axon-core/tests/script_spawn/mod.rs"]
mod script_spawn;

use axon_os::repair_envelope::{self, MintKey, VerifyKey};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A TEST operator key (public, never a real secret), as the hex
/// `AXON_APPROVAL_KEY` carries it.
const OP_KEY: &[u8] = b"nhr-r1 a2 test operator key, not a secret";

fn op_key_hex() -> String {
    OP_KEY.iter().map(|b| format!("{b:02x}")).collect()
}

fn sha256_hex(b: &[u8]) -> String {
    Sha256::digest(b)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn tmp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("axon-intent-ar-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

struct Out {
    stdout: String,
    stderr: String,
    code: i32,
}

fn intent(args: &[&str], key: bool) -> Out {
    let mut c = Command::new(env!("CARGO_BIN_EXE_axon-intent"));
    c.arg("authorize-repair").args(args);
    c.env_remove("AXON_APPROVAL_KEY");
    if key {
        c.env("AXON_APPROVAL_KEY", op_key_hex());
    }
    let o = c.output().unwrap();
    Out {
        stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        code: o.status.code().unwrap_or(-1),
    }
}

/// The check-digest input and digest as cortex computes them (§4.1).
fn check_input() -> (Value, String) {
    let input = json!({
        "argv_shape": "axon-test-local/1",
        "check_name": "check_parse",
        "check_suite_ref": null,
        "checker_exe_sha256": "c".repeat(64),
        "fence_sha256": "f".repeat(64),
        "imports_digest": format!("axsha256:{}", "b".repeat(64)),
    });
    let digest = sha256_hex(&axon_canonical::canonical_bytes(&input).unwrap());
    (input, digest)
}

fn preview(ws: &Path) -> Value {
    let (input, digest) = check_input();
    json!({
        "schema": "cortex-envelope-preview/2",
        "task": {
            "workspace": ws.display().to_string(),
            "file": "src/main.axon",
            "symbol": "parse",
            "pristine_sha256": "a".repeat(64),
        },
        "imports_digest": format!("axsha256:{}", "b".repeat(64)),
        "check_name": "check_parse",
        "check_digest_input": input,
        "check_digest": digest,
        "write_prefixes": ["src"],
        "scopes": {"fs_read": [ws.join("data").display().to_string()], "fs_write": [], "net": []},
        "fence": {"env": {"AXON_FS_READ": "data"}},
        "fence_sha256": "f".repeat(64),
        "target_grant": {"fs_read": ["data"]},
        "generator": {"kind": "ai", "model": "m-1", "provider": "openrouter", "endpoint_host": "openrouter.ai:443"},
        "gate": [
            {"id": "E1", "status": "admit", "basis": "used kinds pinned"},
            {"id": "E4", "status": "admit", "basis": "no native: capability"},
        ],
    })
}

/// A fake `cortex` that records its argv and prints `doc` (raw text, so a
/// test can hand it a duplicate key), exiting `code`.
fn fake_cortex(dir: &Path, doc: &str, code: i32) -> PathBuf {
    let doc_path = dir.join("preview.json");
    std::fs::write(&doc_path, doc).unwrap();
    let script = format!(
        "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\ncat '{}'\nexit {code}\n",
        dir.join("argv.txt").display(),
        doc_path.display()
    );
    let p = dir.join("cortex");
    common::write_executable(&p, script, 0o700);
    p
}

struct Setup {
    dir: PathBuf,
    ws: PathBuf,
    store: PathBuf,
    ledger: PathBuf,
    vkey: PathBuf,
    rkey: PathBuf,
    out: PathBuf,
    cortex: PathBuf,
}

/// Export the runner's two key files into `dir` (`verify.pub`, `record.key`).
fn export_keys(dir: &Path) -> (PathBuf, PathBuf) {
    let vkey = dir.join("verify.pub");
    let o = intent(&["--export-verify-key", vkey.to_str().unwrap()], true);
    assert_eq!(o.code, 0, "export: {}", o.stderr);
    let rkey = dir.join("record.key");
    let o = intent(&["--export-run-record-key", rkey.to_str().unwrap()], true);
    assert_eq!(o.code, 0, "export: {}", o.stderr);
    (vkey, rkey)
}

fn setup(name: &str, doc: Option<String>, code: i32) -> Setup {
    let dir = tmp(name);
    let ws = dir.join("ws");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    let store = dir.join("spent");
    let o = intent(&["--init-spent-store", store.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "init: {}", o.stderr);
    let ledger = dir.join("runs");
    let o = intent(&["--init-run-ledger", ledger.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "init: {}", o.stderr);
    let (vkey, rkey) = export_keys(&dir);
    let doc = doc.unwrap_or_else(|| preview(&ws).to_string());
    let cortex = fake_cortex(&dir, &doc, code);
    Setup {
        out: dir.join("repair.envelope"),
        dir,
        ws,
        store,
        ledger,
        vkey,
        rkey,
        cortex,
    }
}

fn mint(s: &Setup, ttl: &str, symbol: &str) -> Out {
    mint_check(s, ttl, symbol, "check_parse")
}

fn mint_check(s: &Setup, ttl: &str, symbol: &str, check: &str) -> Out {
    let argv = mint_argv(s, ttl, symbol, check);
    let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
    intent(&argv, true)
}

/// The full mint command line, as `--flag value` pairs in order.
fn mint_argv(s: &Setup, ttl: &str, symbol: &str, check: &str) -> Vec<String> {
    [
        "--cortex",
        s.cortex.to_str().unwrap(),
        "--workspace",
        s.ws.to_str().unwrap(),
        "--file",
        "src/main.axon",
        "--symbol",
        symbol,
        "--check",
        check,
        "--write-prefix",
        "src",
        "--read-prefix",
        s.ws.join("data").to_str().unwrap(),
        "--axon",
        "/usr/local/bin/axon",
        "--generator",
        "ai:m-1",
        "--spent-store",
        s.store.to_str().unwrap(),
        "--run-ledger",
        s.ledger.to_str().unwrap(),
        "--verify-key",
        s.vkey.to_str().unwrap(),
        "--run-record-key",
        s.rkey.to_str().unwrap(),
        "--by",
        "operator-a",
        "--ttl-s",
        ttl,
        "--max-proposals",
        "3",
        "--deadline-ms",
        "60000",
        "--check-timeout-ms",
        "5000",
        "--candidates",
        "2",
        "--max-steps",
        "100000",
        "--out",
        s.out.to_str().unwrap(),
    ]
    .iter()
    .map(|a| a.to_string())
    .collect()
}

/// The store's entries other than `store-id`.
fn store_records(s: &Setup) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(&s.store)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n != repair_envelope::STORE_ID_FILE)
        .collect();
    v.sort();
    v
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ── --export-verify-key ──────────────────────────────────────────────────────

#[test]
fn export_verify_key_writes_the_public_half_private_and_once() {
    let d = tmp("export");
    let p = d.join("verify.key");
    let o = intent(&["--export-verify-key", p.to_str().unwrap()], true);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let want = MintKey::derive(OP_KEY).unwrap().verify_key().to_hex();
    let got = std::fs::read_to_string(&p).unwrap();
    assert_eq!(
        got.trim_end(),
        want,
        "ATTACK: the exported verify key is not the minter's public key"
    );
    assert!(
        !got.contains(&op_key_hex()),
        "ATTACK: the operator key was written into the verify-key file"
    );
    let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "ATTACK: the verify key was exported readable by others"
    );
    // The runner loads it as a private key file.
    VerifyKey::load(&p, unsafe { libc::geteuid() }, &[]).expect("the exported key loads");
    let again = intent(&["--export-verify-key", p.to_str().unwrap()], true);
    assert_ne!(
        again.code, 0,
        "ATTACK: an existing verify-key file was overwritten"
    );
    assert_eq!(std::fs::read_to_string(&p).unwrap(), got);
}

#[test]
fn export_verify_key_without_the_operator_key_writes_nothing() {
    let d = tmp("export-nokey");
    let p = d.join("verify.key");
    let o = intent(&["--export-verify-key", p.to_str().unwrap()], false);
    assert_eq!(o.code, 2, "{}", o.stderr);
    assert!(o.stderr.contains("AXON_APPROVAL_KEY"), "{}", o.stderr);
    assert!(
        !p.exists(),
        "ATTACK: a verify key was exported without the operator key"
    );
}

// ── --init-spent-store ───────────────────────────────────────────────────────

#[test]
fn init_spent_store_creates_a_private_store_once() {
    let d = tmp("init");
    let store = d.join("spent");
    let o = intent(&["--init-spent-store", store.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let id = repair_envelope::store_id(&store, unsafe { libc::geteuid() }).unwrap();
    assert!(o.stdout.contains(&id), "{}", o.stdout);
    let mode = std::fs::metadata(&store).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let again = intent(&["--init-spent-store", store.to_str().unwrap()], false);
    assert_ne!(
        again.code, 0,
        "ATTACK: an initialised store was initialised again"
    );
    assert_eq!(
        repair_envelope::store_id(&store, unsafe { libc::geteuid() }).unwrap(),
        id
    );
}

// ── R6: --init-run-ledger, --export-run-record-key, --run-ledger ─────────────

#[test]
fn init_run_ledger_creates_a_private_ledger_once() {
    let d = tmp("initl");
    let ledger = d.join("runs");
    let o = intent(&["--init-run-ledger", ledger.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let euid = unsafe { libc::geteuid() };
    let id = axon_os::run_ledger::ledger_id(&ledger, euid).unwrap();
    assert!(o.stdout.contains(&id), "{}", o.stdout);
    let mode = std::fs::metadata(&ledger).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
    let again = intent(&["--init-run-ledger", ledger.to_str().unwrap()], false);
    assert_ne!(
        again.code, 0,
        "ATTACK: an initialised run ledger was initialised again"
    );
    assert_eq!(axon_os::run_ledger::ledger_id(&ledger, euid).unwrap(), id);
}

#[test]
fn export_run_record_key_writes_the_subkey_private_and_once() {
    let d = tmp("rrk");
    let p = d.join("run-record.key");
    let o = intent(&["--export-run-record-key", p.to_str().unwrap()], true);
    assert_eq!(o.code, 0, "{}", o.stderr);
    let want = axon_os::run_ledger::SealKey::derive(OP_KEY)
        .unwrap()
        .to_hex();
    assert_eq!(std::fs::read_to_string(&p).unwrap(), format!("{want}\n"));
    assert!(
        !o.stdout.contains(&want),
        "ATTACK: the run-record key was printed"
    );
    let mode = std::fs::metadata(&p).unwrap().permissions().mode() & 0o777;
    assert_eq!(
        mode, 0o600,
        "ATTACK: the run-record key was exported readable by others"
    );
    let again = intent(&["--export-run-record-key", p.to_str().unwrap()], true);
    assert_ne!(
        again.code, 0,
        "ATTACK: an existing run-record key file was overwritten"
    );
    let q = d.join("nokey.key");
    let o = intent(&["--export-run-record-key", q.to_str().unwrap()], false);
    assert_eq!(o.code, 2);
    assert!(
        !q.exists(),
        "ATTACK: a run-record key was exported without the operator key"
    );
}

#[test]
fn a_mint_into_an_uninitialised_run_ledger_mints_nothing() {
    let s = setup("noledger", None, 0);
    std::fs::remove_dir_all(&s.ledger).unwrap();
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "run ledger",
        "ATTACK: an envelope was minted for a run ledger that does not exist",
    );
}

// ── authorize-repair ─────────────────────────────────────────────────────────

#[test]
fn authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview() {
    let s = setup("mint", None, 0);
    let o = mint(&s, "3600", "parse");
    assert_eq!(o.code, 0, "stdout {}\nstderr {}", o.stdout, o.stderr);
    // Step 1: the preview was exec'd with the operator's task.
    let argv = std::fs::read_to_string(s.dir.join("argv.txt")).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(
        &argv[..2],
        &["grant", "--envelope-preview"],
        "ATTACK: the preview was not asked of `cortex grant --envelope-preview`"
    );
    for pair in [
        ["--file", "src/main.axon"],
        ["--symbol", "parse"],
        ["--check", "check_parse"],
        ["--write-prefix", "src"],
        ["--read-prefix", s.ws.join("data").to_str().unwrap()],
        ["--axon", "/usr/local/bin/axon"],
        ["--generator", "ai:m-1"],
    ] {
        assert!(
            argv.windows(2).any(|w| w == pair),
            "the preview did not get {pair:?}: {argv:?}"
        );
    }
    // Step 5: the envelope verifies under the exported key and carries the
    // preview's bindings.
    let json = std::fs::read_to_string(&s.out).unwrap();
    let vk = MintKey::derive(OP_KEY).unwrap().verify_key();
    let v = repair_envelope::verify(&json, &vk, now()).expect("the minted envelope verifies");
    let e = &v.envelope;
    let (_, check_digest) = check_input();
    assert_eq!(e.task.symbol, "parse");
    assert_eq!(e.task.workspace, s.ws.display().to_string());
    assert_eq!(e.task.pristine_sha256, "a".repeat(64));
    assert_eq!(
        e.task.imports_digest,
        format!("axsha256:{}", "b".repeat(64))
    );
    assert_eq!(e.check.digest, check_digest);
    assert_eq!(e.capabilities.fence_sha256, "f".repeat(64));
    assert_eq!(e.capabilities.target_grant, json!({"fs_read": ["data"]}));
    assert_eq!(e.generator.endpoint_host, "openrouter.ai:443");
    assert_eq!(e.budget.max_steps, 100_000);
    assert_eq!(e.issued_by, "operator-a");
    assert_eq!(e.expires_unix - e.issued_unix, 3600);
    // Step 4: issued into the store it names.
    let euid = unsafe { libc::geteuid() };
    assert_eq!(
        e.spent_store_id,
        repair_envelope::store_id(&s.store, euid).unwrap()
    );
    assert_eq!(
        e.run_ledger_id,
        axon_os::run_ledger::ledger_id(&s.ledger, euid).unwrap(),
        "ATTACK: the envelope does not bind the run ledger it records into"
    );
    assert_eq!(
        store_records(&s),
        vec![format!("{}.issued", e.envelope_id)],
        "ATTACK: the envelope was not issued into the store it names"
    );
    // Step 3: the cortex binary's sha256 is recorded with the envelope digest.
    let rec: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}.mint", s.out.display())).unwrap(),
    )
    .unwrap();
    assert_eq!(
        rec["cortex_sha256"],
        json!(sha256_hex(&std::fs::read(&s.cortex).unwrap())),
        "ATTACK: the minter did not record the cortex binary it invoked"
    );
    assert_eq!(rec["envelope_digest"], json!(v.digest));
    assert_eq!(rec["envelope_id"], json!(e.envelope_id));
    assert_eq!(
        rec["schema"],
        json!("axon-intent-mint/1"),
        "ATTACK: the mint record does not name its schema axon-intent-mint/1"
    );
    let b = &e.budget;
    assert_eq!(
        (
            b.max_proposals,
            b.deadline_ms,
            b.check_timeout_ms,
            b.candidates,
            b.max_steps
        ),
        (3, 60_000, 5_000, 2, 100_000),
        "ATTACK: a budget flag's value was bound to another budget field"
    );
    // The envelope is spendable exactly once.
    repair_envelope::consume(&s.store, euid, &v, now()).expect("first use");
    assert!(repair_envelope::consume(&s.store, euid, &v, now()).is_err());
}

fn refused_mints_nothing(s: &Setup, o: &Out, code: i32, why: &str, attack: &str) {
    assert_eq!(
        o.code, code,
        "{attack}: stdout {} stderr {}",
        o.stdout, o.stderr
    );
    assert!(o.stderr.contains(why), "{attack}: {}", o.stderr);
    assert!(!s.out.exists(), "{attack}");
    assert!(store_records(s).is_empty(), "{attack}");
}

#[test]
fn a_refused_gate_row_mints_nothing() {
    let mut p = preview(Path::new("/x"));
    let s0 = setup("gate-refuse", None, 0);
    p["task"]["workspace"] = json!(s0.ws.display().to_string());
    p["gate"][1] = json!({"id": "E4", "status": "refuse", "basis": "native:gfx"});
    let s = setup("gate-refuse", Some(p.to_string()), 0);
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        8,
        "E4",
        "ATTACK: an envelope was minted over a refused gate row",
    );
}

/// Security F6 (mint side): a check pin reaching the spent store would let
/// the check child rename `<id>.used` back to `<id>.issued` (a replayable
/// envelope); one reaching the verify key could replace it, one reaching the
/// run-record key could forge records; an `fs_write` pin over the run ledger
/// is refused as phase A refuses it. The minter refuses each before anything
/// is issued, whether the pin names the path, lies above it, or reaches it
/// through a symlink.
#[test]
fn f6_a_pin_reaching_the_store_keys_or_ledger_mints_nothing() {
    let cases: &[(&str, &str, &str)] = &[
        ("fs_write", "spent", "spent store"),
        ("fs_read", "spent", "spent store"),
        ("fs_write", "spent/x.used", "spent store"),
        ("fs_read", "", "spent store"),
        ("fs_write", "verify.pub", "verify key"),
        ("fs_read", "verify.pub", "verify key"),
        ("fs_write", "record.key", "run-record key"),
        ("fs_read", "record.key", "run-record key"),
        ("fs_write", "runs", "run ledger"),
        ("fs_write", "alias/x", "spent store"),
    ];
    for (i, (kind, rel, what)) in cases.iter().enumerate() {
        let name = format!("f6-{i}");
        let s0 = setup(&name, None, 0);
        let pin = if rel.is_empty() {
            s0.dir.display().to_string()
        } else {
            s0.dir.join(rel).display().to_string()
        };
        let mut p = preview(&s0.ws);
        p["scopes"][*kind] = json!([pin]);
        let s = setup(&name, Some(p.to_string()), 0);
        std::os::unix::fs::symlink(&s.store, s.dir.join("alias")).unwrap();
        let o = mint(&s, "3600", "parse");
        refused_mints_nothing(
            &s,
            &o,
            2,
            what,
            &format!("ATTACK: an envelope was minted with {kind} pin {pin} reaching its {what}"),
        );
        assert!(o.stderr.contains("overlaps scopes."), "{}", o.stderr);
    }
}

#[test]
fn an_empty_gate_mints_nothing() {
    let s0 = setup("gate-empty", None, 0);
    let mut p = preview(&s0.ws);
    p["gate"] = json!([]);
    let s = setup("gate-empty", Some(p.to_string()), 0);
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        8,
        "no gate rows",
        "ATTACK: an envelope was minted over an empty gate",
    );
}

#[test]
fn a_gate_row_of_unknown_status_mints_nothing() {
    let s0 = setup("gate-odd", None, 0);
    let mut p = preview(&s0.ws);
    p["gate"][0]["status"] = json!("Admit");
    let s = setup("gate-odd", Some(p.to_string()), 0);
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        8,
        "E1",
        "ATTACK: a gate row that is not exactly `admit` was admitted",
    );
}

#[test]
fn a_check_digest_that_does_not_match_its_input_mints_nothing() {
    let s0 = setup("digest", None, 0);
    for (what, edit) in [
        ("digest", ("check_digest", json!("d".repeat(64)))),
        ("fence", ("fence_sha256", json!("e".repeat(64)))),
        (
            "imports",
            (
                "imports_digest",
                json!(format!("axsha256:{}", "9".repeat(64))),
            ),
        ),
        ("check name", ("check_name", json!("check_other"))),
    ] {
        let mut p = preview(&s0.ws);
        p[edit.0] = edit.1;
        let s = setup("digest", Some(p.to_string()), 0);
        let o = mint(&s, "3600", "parse");
        refused_mints_nothing(
            &s,
            &o,
            2,
            "check digest",
            &format!(
                "ATTACK: a preview whose {what} disagrees with its check-digest input was minted"
            ),
        );
    }
}

#[test]
fn a_preview_for_another_symbol_or_check_mints_nothing() {
    let s = setup("echo", None, 0);
    let o = mint(&s, "3600", "other_symbol");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "symbol",
        "ATTACK: a preview for another symbol was minted",
    );
    let o = mint_check(&s, "3600", "parse", "check_other");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "check",
        "ATTACK: a preview for another check was minted",
    );
}

#[test]
fn a_cortex_binary_changed_during_its_preview_mints_nothing() {
    let s = setup("selfmod", None, 0);
    // The fake cortex appends to itself while it runs: the bytes recorded
    // would not be the bytes that answered.
    let script = std::fs::read_to_string(&s.cortex).unwrap().replacen(
        "cat '",
        "printf '# changed\\n' >> \"$0\"\ncat '",
        1,
    );
    common::write_executable(&s.cortex, script, 0o700);
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "changed while its preview ran",
        "ATTACK: an envelope was minted by a cortex binary that changed while it ran",
    );
}

/// Security F12: the cortex preview needs no operator key, so its child
/// environment never carries `AXON_APPROVAL_KEY` (only that the variable is
/// present is recorded; the value is never written).
#[test]
fn f12_the_preview_child_never_sees_the_operator_key() {
    let s = setup("f12-env", None, 0);
    let seen = s.dir.join("key-seen.txt");
    let script = std::fs::read_to_string(&s.cortex).unwrap().replacen(
        "cat '",
        &format!(
            "if [ -n \"${{AXON_APPROVAL_KEY+x}}\" ]; then echo present; else echo absent; fi > '{}'\ncat '",
            seen.display()
        ),
        1,
    );
    common::write_executable(&s.cortex, script, 0o700);
    let o = mint(&s, "3600", "parse");
    assert_eq!(o.code, 0, "control: the mint succeeds: {}", o.stderr);
    assert_eq!(
        std::fs::read_to_string(&seen).unwrap().trim(),
        "absent",
        "ATTACK: the operator key was handed to the cortex preview child"
    );
}

#[test]
fn a_malformed_preview_mints_nothing() {
    let s0 = setup("malformed", None, 0);
    let good = preview(&s0.ws).to_string();
    let dup = good.replacen("{", r#"{"gate":[],"#, 1);
    let mut unknown = preview(&s0.ws);
    unknown["extra"] = json!(1);
    let mut wrong_schema = preview(&s0.ws);
    wrong_schema["schema"] = json!("cortex-grant/1");
    for (what, doc) in [
        ("a duplicate key", dup),
        ("an unknown field", unknown.to_string()),
        ("another schema", wrong_schema.to_string()),
        ("not JSON", "gate: admit".to_string()),
    ] {
        let s = setup("malformed", Some(doc), 0);
        let o = mint(&s, "3600", "parse");
        refused_mints_nothing(
            &s,
            &o,
            2,
            "preview",
            &format!("ATTACK: a preview with {what} was minted"),
        );
    }
}

#[test]
fn a_failed_preview_mints_nothing() {
    let s0 = setup("cortex-fail", None, 0);
    let s = setup("cortex-fail", Some(preview(&s0.ws).to_string()), 3);
    let o = mint(&s, "3600", "parse");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "exit",
        "ATTACK: an envelope was minted from a preview that failed",
    );
}

#[test]
fn a_ttl_above_the_max_mints_nothing() {
    let s = setup("ttl", None, 0);
    let o = mint(&s, &(repair_envelope::MAX_TTL_S + 1).to_string(), "parse");
    refused_mints_nothing(
        &s,
        &o,
        2,
        "MAX_TTL_S",
        "ATTACK: an envelope living past MAX_TTL_S was minted",
    );
    let o = mint(&s, "0", "parse");
    refused_mints_nothing(&s, &o, 2, "--ttl-s", "ATTACK: a zero TTL was minted");
}

#[test]
fn an_existing_out_is_never_overwritten() {
    let s = setup("exists", None, 0);
    std::fs::write(&s.out, "operator's file\n").unwrap();
    let o = mint(&s, "3600", "parse");
    assert_eq!(o.code, 2, "{}", o.stderr);
    assert_eq!(
        std::fs::read_to_string(&s.out).unwrap(),
        "operator's file\n",
        "ATTACK: an existing --out file was overwritten"
    );
    assert!(
        store_records(&s).is_empty(),
        "ATTACK: an envelope was issued for an unwritable --out"
    );
}

#[test]
fn minting_without_the_operator_key_mints_nothing() {
    let s = setup("nokey", None, 0);
    let mut args: Vec<String> = Vec::new();
    for (k, v) in [
        ("--cortex", s.cortex.display().to_string()),
        ("--workspace", s.ws.display().to_string()),
        ("--file", "src/main.axon".into()),
        ("--symbol", "parse".into()),
        ("--check", "check_parse".into()),
        ("--write-prefix", "src".into()),
        ("--axon", "axon".into()),
        ("--generator", "ai:m-1".into()),
        ("--spent-store", s.store.display().to_string()),
        ("--run-ledger", s.ledger.display().to_string()),
        ("--verify-key", s.vkey.display().to_string()),
        ("--run-record-key", s.rkey.display().to_string()),
        ("--by", "operator-a".into()),
        ("--ttl-s", "60".into()),
        ("--max-proposals", "1".into()),
        ("--deadline-ms", "1".into()),
        ("--check-timeout-ms", "1".into()),
        ("--candidates", "1".into()),
        ("--max-steps", "1".into()),
        ("--out", s.out.display().to_string()),
    ] {
        args.push(k.into());
        args.push(v);
    }
    let a: Vec<&str> = args.iter().map(|s| s.as_str()).collect();
    let o = intent(&a, false);
    refused_mints_nothing(
        &s,
        &o,
        2,
        "AXON_APPROVAL_KEY",
        "ATTACK: an envelope was minted without the operator key",
    );
    assert!(
        !s.dir.join("argv.txt").exists(),
        "the preview ran before the key was checked"
    );
}

// -- refusal coverage (enable-gaps EG-4 lane I-A) -------------------------------

/// The mint command line with `flag` and its value left out.
fn mint_argv_without(s: &Setup, flag: &str) -> Vec<String> {
    let argv = mint_argv(s, "3600", "parse", "check_parse");
    let at = argv.iter().position(|a| a == flag).unwrap();
    [&argv[..at], &argv[at + 2..]].concat()
}

/// Every check that needs no preview runs before the preview does: a
/// missing `--by` or `--ttl-s` is refused by parse_args before the cortex
/// preview is exec'd. Nothing else refuses them earlier (an empty
/// `issued_by` and a zero TTL are refused only after the preview ran).
#[test]
fn a_mint_missing_a_required_flag_runs_no_preview() {
    for flag in ["--by", "--ttl-s"] {
        let s = setup("noflag", None, 0);
        let argv = mint_argv_without(&s, flag);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let o = intent(&argv, true);
        assert!(
            !s.dir.join("argv.txt").exists(),
            "ATTACK: the cortex preview ran for a mint command line without {flag}: {}",
            o.stderr
        );
        refused_mints_nothing(
            &s,
            &o,
            2,
            &format!("{flag} is required"),
            &format!("ATTACK: an envelope was minted without {flag}"),
        );
    }
}

/// parse_args's `{b} is required` and mint's `a budget flag is missing`
/// guard the same missing budget flag (a DOMINATED pair): with each budget
/// flag left out in turn, the pair refuses and nothing is minted or issued.
#[test]
fn a_missing_budget_flag_mints_nothing() {
    for flag in [
        "--max-proposals",
        "--deadline-ms",
        "--check-timeout-ms",
        "--candidates",
        "--max-steps",
    ] {
        let s = setup("nobudget", None, 0);
        let argv = mint_argv_without(&s, flag);
        let argv: Vec<&str> = argv.iter().map(String::as_str).collect();
        let o = intent(&argv, true);
        let attack = format!("ATTACK: an envelope was minted without {flag}");
        assert_eq!(
            o.code, 2,
            "{attack}: stdout {} stderr {}",
            o.stdout, o.stderr
        );
        assert!(!s.out.exists(), "{attack}");
        assert!(store_records(&s).is_empty(), "{attack}");
    }
}

/// The cortex preview child reads nothing from the minter's stdin (the
/// operator's terminal or pipe): its stdin is /dev/null.
#[test]
fn the_preview_child_gets_no_stdin() {
    use std::io::Write as _;
    let s = setup("stdin", None, 0);
    let got = s.dir.join("stdin.txt");
    let script = std::fs::read_to_string(&s.cortex).unwrap().replacen(
        "cat '",
        &format!("cat > '{}'\ncat '", got.display()),
        1,
    );
    common::write_executable(&s.cortex, script, 0o700);
    let argv = mint_argv(&s, "3600", "parse", "check_parse");
    let mut child = Command::new(env!("CARGO_BIN_EXE_axon-intent"))
        .arg("authorize-repair")
        .args(&argv)
        .env("AXON_APPROVAL_KEY", op_key_hex())
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    // The minter never reads its stdin; a write after it exited is EPIPE.
    let _ = child
        .stdin
        .take()
        .unwrap()
        .write_all(b"the operator's stdin\n");
    let o = child.wait_with_output().unwrap();
    assert_eq!(
        o.status.code(),
        Some(0),
        "control: the mint succeeds: {}",
        String::from_utf8_lossy(&o.stderr)
    );
    assert_eq!(
        std::fs::read_to_string(&got).unwrap(),
        "",
        "ATTACK: the cortex preview child read the minter's stdin"
    );
}

/// `<out>.mint` is written before the envelope is issued, so an `--out`
/// that cannot be written (its directory does not exist) issues nothing.
#[test]
fn an_out_that_cannot_be_written_issues_nothing() {
    let mut s = setup("out-absent", None, 0);
    s.out = s.dir.join("absent").join("repair.envelope");
    let o = mint(&s, "3600", "parse");
    assert!(
        store_records(&s).is_empty(),
        "ATTACK: an envelope was issued for an --out that cannot be written: {}",
        o.stderr
    );
    refused_mints_nothing(
        &s,
        &o,
        2,
        "cannot write",
        "ATTACK: an envelope was minted for an --out that cannot be written",
    );
}

// -- the real preview ----------------------------------------------------------

/// The minter against the REAL `cortex grant --envelope-preview` (built from
/// this tree with feature `unattended`) and the real interpreter: the
/// preview passes the strict parse, every gate row admits a pure file, and
/// the minted envelope verifies and pins the preview's values.
#[test]
fn the_real_cortex_preview_mints_a_verifiable_envelope() {
    let axon = script_spawn::workspace_bin(
        "AXON_BIN",
        &[
            "build",
            "-p",
            "axon-core",
            "--no-default-features",
            "--bin",
            "axon",
        ],
        "axon",
    );
    let cortex = script_spawn::workspace_bin(
        "CORTEX_UNATTENDED_BIN",
        &[
            "build",
            "-p",
            "axon-cortex",
            "--features",
            "unattended",
            "--bin",
            "cortex",
        ],
        "cortex",
    );
    let dir = tmp("real");
    let ws = dir.join("ws");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../axon-cortex/fixtures/broken.ax");
    std::fs::copy(&fixture, ws.join("src/broken.ax")).unwrap();
    let ws = std::fs::canonicalize(&ws).unwrap();
    let store = dir.join("spent");
    let o = intent(&["--init-spent-store", store.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "init: {}", o.stderr);
    let ledger = dir.join("runs");
    let o = intent(&["--init-run-ledger", ledger.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "init: {}", o.stderr);
    let (vkey, rkey) = export_keys(&dir);
    let out = dir.join("repair.envelope");
    let o = intent(
        &[
            "--cortex",
            cortex.to_str().unwrap(),
            "--workspace",
            ws.to_str().unwrap(),
            "--file",
            "src/broken.ax",
            "--symbol",
            "double",
            "--check",
            "hidden_completion",
            "--write-prefix",
            "src",
            "--axon",
            axon.to_str().unwrap(),
            "--generator",
            "ai:m-1",
            "--spent-store",
            store.to_str().unwrap(),
            "--run-ledger",
            ledger.to_str().unwrap(),
            "--verify-key",
            vkey.to_str().unwrap(),
            "--run-record-key",
            rkey.to_str().unwrap(),
            "--by",
            "operator-a",
            "--ttl-s",
            "600",
            "--max-proposals",
            "3",
            "--deadline-ms",
            "60000",
            "--check-timeout-ms",
            "5000",
            "--candidates",
            "2",
            "--max-steps",
            "100000",
            "--out",
            out.to_str().unwrap(),
        ],
        true,
    );
    assert_eq!(
        o.code, 0,
        "ATTACK: the real cortex preview did not mint: {}",
        o.stderr
    );
    let json = std::fs::read_to_string(&out).unwrap();
    let vk = MintKey::derive(OP_KEY).unwrap().verify_key();
    let v = repair_envelope::verify(&json, &vk, now()).expect("the minted envelope verifies");
    let e = &v.envelope;
    let pristine = std::fs::read(ws.join("src/broken.ax")).unwrap();
    assert_eq!(e.task.pristine_sha256, sha256_hex(&pristine));
    assert_eq!(e.task.symbol, "double");
    assert_eq!(e.check.name, "hidden_completion");
    assert!(e.task.imports_digest.starts_with("axsha256:"), "{json}");
    assert_eq!(e.capabilities.target_grant["symbol"], "double", "{json}");
    let _ = std::fs::remove_dir_all(&dir);
}
