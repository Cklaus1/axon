//! The authenticated writer of a record must be unforgeable.
//!
//! REPRODUCED before this existed: a non-admin could write a record
//! attributed to another principal, and nothing recorded who actually did it.
//!
//!   --as bob@example.com ingest session s1.jsonl --engineer alice@example.com
//!   → exit 0, events.ndjson gains "principal":"alice@example.com"
//!
//! and through MCP `ledger_refresh {engineer: "carol@..."}`, which
//! authenticates no identity at all. The forged records then landed inside
//! the victim's RBAC view and nobody else's: `--as alice stats` showed them,
//! `--as bob stats` showed none.
//!
//! THE FIX IS NOT TO RESTRICT THE SUBJECT. `--engineer` exists because a CI
//! account legitimately ingests many engineers' sessions; gating it on admin
//! would break the normal ingest path. The two axes are separated instead:
//! `principal` remains the SUBJECT (caller-chosen, as before) and
//! `recorded_by` is the ACTOR, stamped by `Store::append` from the
//! OS-authenticated identity, overwriting anything the caller supplied.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon-ledger")
}

fn real_os_identity() -> String {
    let out = Command::new("id").arg("-un").output().expect("id -un");
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn setup(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("axon_attr_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(d.join("L")).unwrap();
    std::fs::create_dir_all(d.join("sess")).unwrap();
    std::fs::write(d.join("L").join("events.ndjson"), "").unwrap();
    std::fs::write(
        d.join("L").join("rbac.json"),
        r#"{"admins":["alice@example.com"],"authenticated_admins":["nobody-real"]}"#,
    )
    .unwrap();
    std::fs::write(
        d.join("sess").join("s1.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"hi"},"timestamp":"2026-09-20T10:00:00Z","sessionId":"s1"}"#,
    )
    .unwrap();
    d
}

fn records(dir: &Path) -> Vec<serde_json::Value> {
    std::fs::read_to_string(dir.join("L").join("events.ndjson"))
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("record parses"))
        .collect()
}

/// A member may still name the SUBJECT, and the ACTOR is recorded regardless.
#[test]
fn an_ingested_record_records_who_actually_wrote_it() {
    let d = setup("ingest");
    let me = real_os_identity();

    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .arg("--as")
        .arg("bob@example.com")
        .args(["ingest", "session"])
        .arg(d.join("sess").join("s1.jsonl"))
        .args(["--engineer", "alice@example.com"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "premise: ingest must still work for a member — gating the subject \
         would break the normal path: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let recs = records(&d);
    assert_eq!(recs.len(), 1, "expected exactly one record: {recs:?}");
    // The SUBJECT is what the caller asked for. That is allowed.
    assert_eq!(recs[0]["principal"], "alice@example.com");
    // The ACTOR is the OS identity, which the caller never supplied.
    assert_eq!(
        recs[0]["recorded_by"], me,
        "the record does not say who actually wrote it: {:?}",
        recs[0]
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The stamp OVERWRITES a caller-supplied value. Without this the field would
/// be a suggestion rather than evidence.
///
/// Goes through the library rather than the CLI because the CLI offers no way
/// to set the field — which is itself the point: the only way to attempt the
/// forgery is to construct the record directly, and even that does not
/// survive `append`.
#[test]
fn a_caller_supplied_writer_is_overwritten_not_trusted() {
    use axon_ledger::model::{Effect, LedgerRecord};
    use axon_ledger::store::Store;

    let d = setup("forge");
    let me = real_os_identity();
    let mut store = Store::open_for_write(&d.join("L")).unwrap();

    let forged = LedgerRecord {
        id: "f1".into(),
        principal: "alice@example.com".into(),
        effect: Effect::GitCommit,
        causal_parent: None,
        ts_ms: 1000,
        payload: serde_json::json!({"sha": "aaa"}),
        repo: None,
        recorded_by: Some("someone-i-am-not".into()),
    };
    store.append(&forged).unwrap();

    let recs = records(&d);
    assert_eq!(recs.len(), 1);
    assert_eq!(
        recs[0]["recorded_by"], me,
        "a caller-supplied writer survived the append: {:?}",
        recs[0]
    );
    assert_ne!(recs[0]["recorded_by"], "someone-i-am-not");
    let _ = std::fs::remove_dir_all(&d);
}

/// The MCP surface authenticates no identity, so it is the one that most
/// needs the stamp — and it gets it for free by going through the same
/// choke point.
#[test]
fn the_mcp_write_path_is_stamped_too() {
    let d = setup("mcp");
    let me = real_os_identity();
    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        let ok = Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
        assert!(ok.status.success(), "git {args:?} failed");
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "alice@example.com"]);
    git(&["config", "user.name", "a"]);
    std::fs::write(repo.join("f.txt"), "x").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "forged"]);

    let frames = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\",\"params\":{{}}}}\n\
         {{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{{\"name\":\"ledger_refresh\",\"arguments\":{{\"repo\":\"{}\",\"session_dir\":\"{}\",\"engineer\":\"carol@example.com\"}}}}}}\n",
        repo.display(),
        d.join("sess").display()
    );
    let mut child = std::process::Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .arg("mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(frames.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let _ = child.wait_with_output().unwrap();

    let recs = records(&d);
    assert!(
        !recs.is_empty(),
        "premise: the MCP tool must actually have written something"
    );
    for r in &recs {
        assert_eq!(
            r["recorded_by"], me,
            "an MCP-written record is not stamped with the real writer: {r:?}"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// Records written before the field existed must still load. The field is
/// `Option` with serde `default` for the same reason `repo` is.
#[test]
fn a_record_written_before_this_field_existed_still_loads() {
    let d = setup("compat");
    // RBAC off for this one: the question is whether an OLD record
    // DESERIALIZES, and leaving RBAC on filtered the record out for a caller
    // who is not its principal — a green-for-the-wrong-reason failure that
    // said "not counted" when the record had loaded perfectly well.
    std::fs::remove_file(d.join("L").join("rbac.json")).unwrap();
    std::fs::write(
        d.join("L").join("events.ndjson"),
        "{\"id\":\"old1\",\"principal\":\"git:alice@example.com\",\"effect\":\"git_commit\",\
         \"causal_parent\":null,\"ts_ms\":1000,\"payload\":{\"sha\":\"aaa\"}}\n",
    )
    .unwrap();
    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .args(["stats"])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "a pre-existing record without `recorded_by` failed to load: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("Total records:    1"),
        "the old record was not counted: {}",
        String::from_utf8_lossy(&out.stdout)
    );
    let _ = std::fs::remove_dir_all(&d);
}
