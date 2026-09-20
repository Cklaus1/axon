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
    // EPIPE is a LEGITIMATE outcome here, not a test failure, and treating it
    // as one made these tests flaky (measured: 1 failure in 5 isolated runs of
    // the snapshot test, 1 in 12 of the whole file, and one strict-gate
    // failure). The cases that race are exactly the ones where the adapter
    // refuses WITHOUT reading the request — a missing grant snapshot is
    // detected from argv, so the child can exit before it ever consumes stdin
    // and the parent's write loses the race.
    //
    // Narrow on purpose: only BrokenPipe is tolerated. Any other write error
    // still panics, because swallowing them would hide a real defect behind
    // the same silence this file exists to prevent. The assertions that matter
    // — the exit code and an EMPTY stdout — are made on the output below
    // either way, so nothing is skipped when the write is cut short.
    if let Err(e) = c
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
    {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "writing the request failed for a reason other than the adapter \
             exiting early: {e}"
        );
    }
    let out = c.wait_with_output().expect("wait");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    (out.status.code().unwrap_or(-1), text)
}

fn req(action: &str, target: &str, snapshot: &str) -> String {
    // Carries a `symbol`, because an edit must name what it edits. The adapter
    // now parses the request into a typed CortexAction, and
    // `patch_symbol_body` without a symbol is refused rather than authorised
    // against the path alone — see `an_edit_that_does_not_name_its_symbol_is_refused`.
    format!(
        r#"{{"protocol_version":1,"action":"{action}","principal":"agent","target_path":"{target}","snapshot_id":"{snapshot}","symbol":"add"}}"#
    )
}

/// Requests WITHOUT a symbol, for the negative case.
fn req_no_symbol(action: &str, target: &str, snapshot: &str) -> String {
    format!(
        r#"{{"protocol_version":1,"action":"{action}","principal":"agent","target_path":"{target}","snapshot_id":"{snapshot}"}}"#
    )
}

#[test]
fn the_policy_boundary_allows_and_refuses_for_distinct_reasons() {
    // Every case below now supplies `--grant-snapshot` explicitly.
    //
    // Five of these six omitted it, and relied on the adapter defaulting the grant's basis to the
    // request's own snapshot. So the test that proves the policy boundary allows and refuses was
    // itself running with StaleSnapshot structurally disabled — the guarantee was absent from the
    // very suite that existed to demonstrate it. Requiring the flag turned all five red at once,
    // which is the clearest evidence available that the default was load-bearing.
    //
    // Where staleness is not the point, the grant's basis equals the request's; the stale case
    // below sets them apart deliberately.
    // Writable action inside the granted prefix.
    let (rc, out) = decide(
        &[
            "--principal",
            "agent",
            "--write-prefix",
            "crates/",
            "--grant-snapshot",
            "s1",
        ],
        &req("patch_symbol_body", "crates/foo.rs", "s1"),
    );
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains(r#""decision":"allow""#), "{out}");

    // Outside the prefix.
    let (_, out) = decide(
        &[
            "--principal",
            "agent",
            "--write-prefix",
            "crates/",
            "--grant-snapshot",
            "s1",
        ],
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
        &["--principal", "agent", "--grant-snapshot", "s1"],
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
        &["--principal", "agent", "--grant-snapshot", "s1"],
        &req("inspect", "docs/x.md", "s1"),
    );
    assert!(out.contains(r#""decision":"allow""#), "{out}");

    // An action absent from the catalog is refused rather than permitted
    // because nothing forbade it.
    let (_, out) = decide(
        &[
            "--principal",
            "agent",
            "--write-prefix",
            "crates/",
            "--grant-snapshot",
            "s1",
        ],
        &req("edit", "crates/foo.rs", "s1"),
    );
    assert!(out.contains("catalog"), "{out}");
}

#[test]
fn a_broken_request_fails_without_deciding() {
    // Exit 2 with NO decision on stdout. A malformed request means nothing was
    // decided; printing a refusal would report a broken adapter as strict
    // policy, and the caller could not tell the two apart.
    //
    // Every case below supplies BOTH required flags. Without them the run
    // exits 2 at the missing-flag gate, which is before stdin is read, before
    // the JSON is parsed and before the version is checked — so the earlier
    // version of this test reached none of the paths it named, and passed
    // with the version check and the malformed-JSON arm deleted.
    let ok_flags = vec!["--principal", "agent", "--grant-snapshot", "s1"];
    for (body, why, expect) in [
        (
            r#"{"protocol_version":99,"action":"inspect","principal":"a","target_path":"x","snapshot_id":"s"}"#,
            "protocol version mismatch",
            "protocol version",
        ),
        ("not json", "malformed request", "malformed request"),
        (
            // DUPLICATE KEYS. `serde_json` resolves these last-wins, so the
            // request a human reads and the request the machine decides on are
            // different documents. Measured before this was refused: with
            // `intruder` and a traversal path FIRST and benign values last,
            // the adapter answered `allow`.
            r#"{"protocol_version":1,"action":"patch_symbol_body","symbol":"f","principal":"intruder","principal":"agent","target_path":"../../etc/shadow","target_path":"ok.ax","snapshot_id":"s1"}"#,
            "duplicate keys are ambiguous, not last-wins",
            "malformed request",
        ),
    ] {
        let (rc, out) = decide(&ok_flags, body);
        assert_eq!(rc, 2, "{why}: expected exit 2, got {rc}: {out}");
        assert!(
            !out.contains(r#""decision""#),
            "{why}: a failure must not emit a decision: {out}"
        );
        // The REASON, not merely the exit code — this file's header says each
        // case is distinguished by why it failed, and asserting only the code
        // is what let all of them fail for the same unrelated reason.
        assert!(
            out.contains(expect),
            "{why}: must fail for its own reason, got: {out}"
        );
    }
}

/// Every field of the grant is REQUIRED, including the third one.
///
/// The module doc enumerates this class for `--grant-snapshot` and
/// `--write-prefix` and concludes "Both now fail closed". `--principal`, the
/// third field of the same grant, still defaulted to `agent` — so a write
/// could be authorised in the name of a principal no operator ever named, and
/// the wrong-principal refusal was unreachable for anyone who guessed the
/// default. Every existing test passed `--principal` explicitly, so nothing
/// exercised it.
#[test]
fn a_grant_with_no_principal_is_refused_not_defaulted() {
    let body = r#"{"protocol_version":1,"action":"inspect","principal":"agent","target_path":"x","snapshot_id":"s1"}"#;
    let (rc, out) = decide(&["--grant-snapshot", "s1"], body);
    assert_eq!(rc, 2, "a missing principal must fail closed: {out}");
    assert!(
        out.contains("--principal is required"),
        "and say which flag: {out}"
    );
    assert!(
        !out.contains(r#""decision""#),
        "nothing was decided, so nothing may be reported: {out}"
    );
}

/// An `allow` says WHAT WAS CHECKED.
///
/// Cortex returns Ok immediately for any action needing no write authority —
/// before the principal, staleness, traversal and policy-file checks. So an
/// `inspect` naming a principal the grant does not belong to, a snapshot that
/// never existed and a `..` path produced the same bytes as a granted write.
/// "The grant authorised this" and "no authority question was asked" were one
/// token.
#[test]
fn an_allow_distinguishes_granted_from_unchecked() {
    let flags = vec![
        "--principal",
        "agent",
        "--grant-snapshot",
        "s1",
        "--write-prefix",
        "ok.ax",
    ];

    let (rc, out) = decide(
        &flags,
        r#"{"protocol_version":1,"action":"inspect","principal":"nobody-at-all","target_path":"../../../../etc/shadow","snapshot_id":"never-existed"}"#,
    );
    assert_eq!(rc, 0, "{out}");
    assert!(out.contains(r#""decision":"allow""#), "{out}");
    assert!(
        out.contains("no write authority required") && out.contains("NOT evaluated"),
        "an unchecked allow must say so: {out}"
    );

    // And a real grant check says the opposite, so the two are never confused.
    let (rc2, out2) = decide(
        &flags,
        r#"{"protocol_version":1,"action":"patch_symbol_body","symbol":"f","principal":"agent","target_path":"ok.ax","snapshot_id":"s1"}"#,
    );
    assert_eq!(rc2, 0, "{out2}");
    assert!(
        out2.contains(r#""decision":"allow""#) && out2.contains("granted"),
        "a granted write must be distinguishable from an unchecked one: {out2}"
    );
    assert!(!out2.contains("NOT evaluated"), "{out2}");
}

/// Stdout ONLY, kept separate from stderr on purpose.
///
/// `decide` merges the two, which is fine for tests that assert on a reason string but useless
/// here: the whole claim is that nothing reaches STDOUT, while the explanation goes to stderr.
/// Merged output cannot tell "emitted no decision" from "emitted one and also complained".
fn stdout_of(args: &[&str], request: &str) -> (i32, String) {
    let exe = env!("CARGO_BIN_EXE_cortex-policy-adapter");
    let mut c = Command::new(exe)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn adapter");
    // EPIPE is a LEGITIMATE outcome here, not a test failure, and treating it
    // as one made these tests flaky (measured: 1 failure in 5 isolated runs of
    // the snapshot test, 1 in 12 of the whole file, and one strict-gate
    // failure). The cases that race are exactly the ones where the adapter
    // refuses WITHOUT reading the request — a missing grant snapshot is
    // detected from argv, so the child can exit before it ever consumes stdin
    // and the parent's write loses the race.
    //
    // Narrow on purpose: only BrokenPipe is tolerated. Any other write error
    // still panics, because swallowing them would hide a real defect behind
    // the same silence this file exists to prevent. The assertions that matter
    // — the exit code and an EMPTY stdout — are made on the output below
    // either way, so nothing is skipped when the write is cut short.
    if let Err(e) = c
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(request.as_bytes())
    {
        assert_eq!(
            e.kind(),
            std::io::ErrorKind::BrokenPipe,
            "writing the request failed for a reason other than the adapter \
             exiting early: {e}"
        );
    }
    let out = c.wait_with_output().expect("wait");
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string(),
    )
}

/// **A missing grant snapshot must not be interpretable as "authority is current".**
///
/// The adapter used to default the grant's snapshot to the one carried by the REQUEST. Grant and
/// current were then equal by construction, `Refusal::StaleSnapshot` was unreachable, and the
/// module docs described a property that did not hold. The bypass needed no malice and no unusual
/// input — just an omitted flag.
///
/// It survived because the existing staleness test always PASSES `--grant-snapshot`. A guarantee
/// tested only on the path that supplies it is not tested at all.
///
/// Exit non-zero with no decision on stdout, not a refusal: a misconfigured adapter reported as a
/// strict policy is how a broken setup gets recorded as a result.
#[test]
fn a_missing_grant_snapshot_is_an_infrastructure_failure_not_a_current_grant() {
    let (code, stdout) = stdout_of(
        &["--principal", "agent", "--write-prefix", "src/"],
        &req("patch_symbol_body", "src/x.rs", "snap-CURRENT"),
    );

    assert_ne!(
        code, 0,
        "the adapter succeeded with no grant basis supplied"
    );
    assert!(
        stdout.trim().is_empty(),
        "a decision reached stdout without any grant basis: {stdout}"
    );
}

/// The control: the SAME request with a grant basis supplied still decides normally, so the test
/// above proves a missing grant is rejected rather than that the adapter stopped working.
#[test]
fn the_same_request_with_a_grant_basis_still_decides() {
    let (code, stdout) = stdout_of(
        &[
            "--principal",
            "agent",
            "--write-prefix",
            "src/",
            "--grant-snapshot",
            "snap-CURRENT",
        ],
        &req("patch_symbol_body", "src/x.rs", "snap-CURRENT"),
    );
    assert_eq!(code, 0, "a well-formed grant should decide: {stdout}");
    assert!(
        stdout.contains("\"decision\":\"allow\""),
        "expected an allow for an in-scope path under a current grant: {stdout}"
    );
}

/// An edit that does not name what it edits cannot be authorised.
///
/// The protocol carried `action` and `target_path` but no `symbol`, so the
/// adapter authorised `patch_symbol_body` knowing only the FILE. The grant
/// check could answer "that path is in range" while the request named no
/// symbol at all — a coherent-looking allow for an edit with no stated subject.
///
/// Typed actions make the question answerable: `PatchSymbolBody` requires a
/// `SymbolRef`, so the adapter must either supply one or refuse. This is not
/// added strictness; it is a check that previously had nothing to check.
#[test]
fn an_edit_that_does_not_name_its_symbol_is_refused() {
    let args = &[
        "--principal",
        "agent",
        "--grant-snapshot",
        "s1",
        "--write-prefix",
        "crates/",
    ];

    // Control FIRST: the same request WITH a symbol is allowed. Without this,
    // the refusal below could be explained by the grant, the prefix, or the
    // snapshot rather than by the missing symbol.
    let (rc, out) = decide(args, &req("patch_symbol_body", "crates/foo.rs", "s1"));
    assert_eq!(rc, 0, "{out}");
    assert!(
        out.contains(r#""decision":"allow""#),
        "a named edit inside the grant must still be allowed: {out}"
    );

    // The same request, minus the symbol.
    let (rc, out) = decide(
        args,
        &req_no_symbol("patch_symbol_body", "crates/foo.rs", "s1"),
    );
    assert_eq!(
        rc, 0,
        "a refusal is a decision, not an infrastructure failure"
    );
    assert!(
        out.contains(r#""decision":"refuse""#),
        "an edit with no named symbol must be refused: {out}"
    );
    assert!(
        out.contains("does not name what it edits"),
        "the refusal must say WHY — a generic denial here is indistinguishable \
         from a grant or path problem: {out}"
    );

    // Read-only actions are unaffected: they need no symbol to be coherent.
    let (rc, out) = decide(args, &req_no_symbol("inspect", "docs/x.md", "s1"));
    assert_eq!(rc, 0, "{out}");
    assert!(
        out.contains(r#""decision":"allow""#),
        "inspect does not edit, so it needs no symbol: {out}"
    );
}
