//! NH-R1 A2 (`tasks/unattended-auth-spec.md` §5.1): `axon-intent
//! authorize-repair`, its `--export-verify-key` and `--init-spent-store`
//! forms, driven as an operator would (subprocess).
//!
//! The cortex preview is a FAKE here: a shell script that records its
//! arguments and prints a fixed `cortex-envelope-preview/1` document. That
//! is the point of the test: the minter must judge the preview it is handed
//! (gate rows, check-digest consistency, symbol and check echo), never trust
//! it. The operator key is a TEST key derived from a fixed, public string.

mod common;

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
        "schema": "cortex-envelope-preview/1",
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
        "scopes": {"fs_read": ["data"], "fs_write": [], "net": []},
        "fence": {"env": {"AXON_FS_READ": "data"}},
        "fence_sha256": "f".repeat(64),
        "target_grant": {"fs_read": ["data"]},
        "generator": {"kind": "ai", "model": "m-1", "provider": "openrouter", "endpoint_host": "openrouter.ai"},
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
    out: PathBuf,
    cortex: PathBuf,
}

fn setup(name: &str, doc: Option<String>, code: i32) -> Setup {
    let dir = tmp(name);
    let ws = dir.join("ws");
    std::fs::create_dir_all(ws.join("src")).unwrap();
    let store = dir.join("spent");
    let o = intent(&["--init-spent-store", store.to_str().unwrap()], false);
    assert_eq!(o.code, 0, "init: {}", o.stderr);
    let doc = doc.unwrap_or_else(|| preview(&ws).to_string());
    let cortex = fake_cortex(&dir, &doc, code);
    Setup {
        out: dir.join("repair.envelope"),
        dir,
        ws,
        store,
        cortex,
    }
}

fn mint(s: &Setup, ttl: &str, symbol: &str) -> Out {
    mint_check(s, ttl, symbol, "check_parse")
}

fn mint_check(s: &Setup, ttl: &str, symbol: &str, check: &str) -> Out {
    intent(
        &[
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
            "data",
            "--axon",
            "/usr/local/bin/axon",
            "--generator",
            "ai:m-1",
            "--spent-store",
            s.store.to_str().unwrap(),
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
        ],
        true,
    )
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

// ── authorize-repair ─────────────────────────────────────────────────────────

#[test]
fn authorize_repair_mints_a_verifiable_issued_envelope_from_the_preview() {
    let s = setup("mint", None, 0);
    let o = mint(&s, "3600", "parse");
    assert_eq!(o.code, 0, "stdout {}\nstderr {}", o.stdout, o.stderr);
    // Step 1: the preview was exec'd with the operator's task.
    let argv = std::fs::read_to_string(s.dir.join("argv.txt")).unwrap();
    let argv: Vec<&str> = argv.lines().collect();
    assert_eq!(&argv[..2], &["grant", "--envelope-preview"]);
    for pair in [
        ["--file", "src/main.axon"],
        ["--symbol", "parse"],
        ["--check", "check_parse"],
        ["--write-prefix", "src"],
        ["--read-prefix", "data"],
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
    assert_eq!(e.generator.endpoint_host, "openrouter.ai");
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
