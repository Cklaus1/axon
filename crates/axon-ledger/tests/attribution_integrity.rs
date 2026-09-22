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
    // ledger_refresh is admin-gated now, so the server must hold authority
    // for the write to happen at all — this test is about the STAMP, not the
    // gate (which `a_privileged_mcp_tool_requires_the_same_authority_as_the_cli_verb`
    // covers). Without this the write is refused and the test would assert
    // stamping on an empty ledger.
    std::fs::write(
        d.join("L").join("rbac.json"),
        format!("{{\"admins\":[\"alice@example.com\"],\"authenticated_admins\":[\"{me}\"]}}"),
    )
    .unwrap();
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

// ── The MCP surface must not be a way around the admin gate ────────────────

/// A privileged operation must require the same authority whichever surface
/// reaches it.
///
/// The admin gate lived in `main()` before CLI dispatch, and the MCP server
/// dispatches tool calls internally, so the same operation reached over
/// stdio JSON-RPC skipped it entirely. REPRODUCED: with
/// `authenticated_admins = ["nobody-real"]`, the CLI `refresh --repo R` was
/// refused and left the ledger at 2 records, while
/// `tools/call ledger_refresh {repo: R}` returned ok and wrote 193.
///
/// The previous pass had classified `Commands::Mcp` as "appends new records;
/// does not rewrite or reattribute" — classifying the SERVER rather than the
/// operations it exposes.
#[test]
fn a_privileged_mcp_tool_requires_the_same_authority_as_the_cli_verb() {
    let d = setup("mcpgate");
    // Authority belongs to someone who is not this process.
    std::fs::write(
        d.join("L").join("rbac.json"),
        r#"{"admins":["alice@example.com"],"authenticated_admins":["nobody-real"]}"#,
    )
    .unwrap();
    std::fs::write(
        d.join("L").join("events.ndjson"),
        "{\"id\":\"a1\",\"principal\":\"git:alice@example.com\",\"effect\":\"git_commit\",\
         \"causal_parent\":null,\"ts_ms\":1000,\"payload\":{\"sha\":\"a\"}}\n",
    )
    .unwrap();
    let before = std::fs::read_to_string(d.join("L").join("events.ndjson")).unwrap();

    let repo = d.join("repo");
    std::fs::create_dir_all(&repo).unwrap();
    let git = |args: &[&str]| {
        Command::new("git")
            .arg("-C")
            .arg(&repo)
            .args(args)
            .output()
            .unwrap();
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "attacker@evil.test"]);
    git(&["config", "user.name", "x"]);
    std::fs::write(repo.join("f.txt"), "y").unwrap();
    git(&["add", "-A"]);
    git(&["commit", "-q", "-m", "forged"]);

    // PREMISE: the CLI verb is refused for this caller.
    let cli = Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .args(["refresh", "--repo"])
        .arg(&repo)
        .output()
        .unwrap();
    assert_ne!(
        cli.status.code(),
        Some(0),
        "premise: CLI refresh must be refused"
    );

    // The same operation through MCP must be refused too.
    let frames = format!(
        "{{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"tools/call\",\"params\":{{\"name\":\"ledger_refresh\",\"arguments\":{{\"repo\":\"{}\"}}}}}}\n",
        repo.display()
    );
    let mut child = Command::new(bin())
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
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();

    assert!(
        stdout.contains("restricted to an admin"),
        "MCP performed a privileged operation without authority:\n{stdout}"
    );
    assert_eq!(
        before,
        std::fs::read_to_string(d.join("L").join("events.ndjson")).unwrap(),
        "the ledger was modified through MCP despite the caller having no authority"
    );

    // CONTROL: a READ tool is not gated — gating reads would be a different
    // product, and would hide a filter regression behind a refusal.
    let read_frame = "{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"ledger_stats\",\"arguments\":{}}}\n";
    let mut child = Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .arg("mcp")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(read_frame.as_bytes())
        .unwrap();
    drop(child.stdin.take());
    let out = child.wait_with_output().unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(
        !stdout.contains("restricted to an admin"),
        "a read tool must not be admin-gated:\n{stdout}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// `replace_record` must stamp too, or an update erases the writer.
///
/// `Store::append` is the choke point for a record being BORN, not for its
/// bytes changing. `session-refresh` rewrites a record through
/// `replace_record`, which wrote the caller's struct verbatim — and because
/// `recorded_by` is `skip_serializing_if = "Option::is_none"`, and
/// `ingest_session` returns an UNSTAMPED original while appending a stamped
/// clone, the field vanished entirely. The refreshed record became
/// indistinguishable from a pre-attribution legacy record.
///
/// Mutation found that this was fixed but untested: removing the stamp from
/// `replace_record` left the suite green.
#[test]
fn refreshing_a_record_does_not_erase_its_writer() {
    let d = setup("refresh");
    let me = real_os_identity();
    std::fs::write(
        d.join("L").join("rbac.json"),
        format!("{{\"admins\":[],\"authenticated_admins\":[\"{me}\"]}}"),
    )
    .unwrap();
    std::fs::write(
        d.join("L").join("events.ndjson"),
        "{\"id\":\"sess-1\",\"principal\":\"agent:victim@example.com\",\
         \"effect\":\"agent_session\",\"causal_parent\":null,\
         \"ts_ms\":1767225600000,\"payload\":{\"session_id\":\"aaaabbbbccccdddd\",\
         \"turn_count\":0,\"goal\":\"original\"},\"recorded_by\":\"ci-bot\"}\n",
    )
    .unwrap();
    std::fs::write(
        d.join("sess").join("aaaabbbbccccdddd.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"do the thing"},"timestamp":"2026-01-01T00:00:00Z","sessionId":"aaaabbbbccccdddd"}"#,
    )
    .unwrap();

    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(d.join("L"))
        .arg("session-refresh")
        .arg(d.join("sess"))
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "premise: the refresh must run: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let recs = records(&d);
    assert_eq!(recs.len(), 1);
    assert!(
        recs[0].get("recorded_by").is_some(),
        "the refresh ERASED the writer — the record is now indistinguishable \
         from a pre-attribution legacy record: {:?}",
        recs[0]
    );
    assert_eq!(
        recs[0]["recorded_by"], me,
        "the writer must name whoever actually rewrote the bytes: {:?}",
        recs[0]
    );
    let _ = std::fs::remove_dir_all(&d);
}
