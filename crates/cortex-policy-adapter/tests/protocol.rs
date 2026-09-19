//! The adapter had NO tests. It built clean, and nothing checked that a refusal
//! was a refusal — which for a policy boundary is the failure that matters:
//! every wrong answer here is either an authority leak or a denial of service,
//! and both look like a working program from outside.
//!
//! Each case below is distinguished by its REASON, not merely by allow/refuse.
//! A first pass of this test used `action: "edit"`, which is not in Cortex's
//! catalog, so all four decision cases returned the same NotInCatalog refusal
//! and appeared to pass while exercising one code path.

use std::io::Write;
use std::process::{Command, Stdio};

fn decide(args: &[&str], request: &str) -> (i32, String) {
    let exe = env!("CARGO_BIN_EXE_cortex-policy-adapter");
    let mut c = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn adapter");
    c.stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
        .expect("write request");
    let out = c.wait_with_output().expect("wait");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

fn req(action: &str, target: &str, snapshot: &str) -> String {
    format!(
        r#"{{"protocol_version":1,"action":"{action}","principal":"agent","target_path":"{target}","snapshot_id":"{snapshot}"}}"#
    )
}

#[test]
fn the_policy_boundary_allows_and_refuses_for_distinct_reasons() {
    // Writable action inside the granted prefix.
    let (rc, out) = decide(
        &["--principal", "agent", "--write-prefix", "crates/"],
        &req("patch_symbol_body", "crates/foo.rs", "s1"),
    );
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains(r#""decision":"allow""#), "{out}");

    // Outside the prefix.
    let (_, out) = decide(
        &["--principal", "agent", "--write-prefix", "crates/"],
        &req("patch_symbol_body", "docs/x.md", "s1"),
    );
    assert!(out.contains(r#""decision":"refuse""#), "{out}");
    assert!(
        out.contains("outside"),
        "refusal must name the prefix: {out}"
    );

    // NO prefix granted. Cortex's convention: an empty list denies everything,
    // rather than an absent restriction meaning no restriction.
    let (_, out) = decide(
        &["--principal", "agent"],
        &req("patch_symbol_body", "crates/foo.rs", "s1"),
    );
    assert!(
        out.contains(r#""decision":"refuse""#),
        "empty prefix must deny: {out}"
    );

    // The grant was pinned to a snapshot the workspace has moved past.
    let (_, out) = decide(
        &[
            "--principal",
            "agent",
            "--write-prefix",
            "crates/",
            "--grant-snapshot",
            "sOLD",
        ],
        &req("patch_symbol_body", "crates/foo.rs", "sNEW"),
    );
    assert!(out.contains(r#""decision":"refuse""#), "{out}");
    assert!(
        out.contains("snapshot"),
        "a stale grant must refuse FOR THAT REASON, not as a generic denial: {out}"
    );

    // A read-only action needs no write prefix.
    let (_, out) = decide(
        &["--principal", "agent"],
        &req("inspect", "docs/x.md", "s1"),
    );
    assert!(out.contains(r#""decision":"allow""#), "{out}");

    // An action absent from the catalog is refused rather than permitted
    // because nothing forbade it.
    let (_, out) = decide(
        &["--principal", "agent", "--write-prefix", "crates/"],
        &req("edit", "crates/foo.rs", "s1"),
    );
    assert!(out.contains("catalog"), "{out}");
}

#[test]
fn a_broken_request_fails_without_deciding() {
    // Exit 2 with NO decision on stdout. A malformed request means nothing was
    // decided; printing a refusal would report a broken adapter as strict
    // policy, and the caller could not tell the two apart.
    for (args, body, why) in [
        (
            vec!["--principal", "agent"],
            r#"{"protocol_version":99,"action":"inspect","principal":"a","target_path":"x","snapshot_id":"s"}"#,
            "protocol version mismatch",
        ),
        (
            vec!["--principal", "agent"],
            "not json",
            "malformed request",
        ),
    ] {
        let (rc, out) = decide(&args, body);
        assert_eq!(rc, 2, "{why}: expected exit 2, got {rc}: {out}");
        assert!(
            !out.contains(r#""decision""#),
            "{why}: a failure must not emit a decision: {out}"
        );
    }
}
