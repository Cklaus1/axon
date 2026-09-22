//! A member must not be able to rewrite or delete another principal's records.
//!
//! REPRODUCED. Two-principal ledger, `rbac.json = {"admins":["alice@..."]}`,
//! bob a member whose `stats` correctly showed him 1 of 2 records:
//!
//!   prune --older-than 2099-01-01 --yes    exit 0, "Pruned 2; 0 remain"
//!                                          alice's records destroyed
//!   engineer-backfill --email bob@...      exit 0, "2/2 records -> bob"
//!                                          then stats showed 2, and
//!                                          `diff --json` returned alice's
//!                                          payload verbatim
//!
//! So this was escalation, not only destruction. `rbac` was consulted at
//! exactly two lines in the whole CLI, both inside `search`: the read paths
//! were filtered and nothing asked who the caller WAS before letting them
//! rewrite the file.
//!
//! Found by an adversarial review of the read-path fix that preceded it. That
//! fix closed the reads and I treated maintenance as safe because it used an
//! unfiltered handle by design; the follow-up guarded the store API against a
//! FILTERED handle and its control asserted "an unfiltered handle must still
//! be able to prune" — correct for the API, and exactly the wrong place to
//! stop asking.

use std::path::Path;
use std::process::Command;

fn bin() -> &'static str {
    env!("CARGO_BIN_EXE_axon-ledger")
}

const RECS: &str = concat!(
    r#"{"id":"a1","principal":"agent:alice@example.com","effect":"git_commit","causal_parent":null,"ts_ms":1000,"payload":{"sha":"aaa","subject":"secret alice work"}}"#,
    "\n",
    r#"{"id":"b1","principal":"agent:bob@example.com","effect":"git_commit","causal_parent":null,"ts_ms":2000,"payload":{"sha":"bbb","subject":"bob work"}}"#,
    "\n"
);

fn seed(dir: &Path, admins: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("events.ndjson"), RECS).unwrap();
    std::fs::write(dir.join("rbac.json"), format!("{{\"admins\":[{admins}]}}")).unwrap();
}

fn run(dir: &Path, caller: &str, args: &[&str]) -> (i32, String) {
    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(dir)
        .arg("--as")
        .arg(caller)
        .args(args)
        .output()
        .unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
}

/// Every command that rewrites existing records, run as a member.
fn destructive() -> Vec<Vec<&'static str>> {
    vec![
        vec!["prune", "--older-than", "2099-01-01", "--yes"],
        vec!["prune", "--older-than", "2099-01-01", "--dry-run"],
        vec!["engineer-backfill", "--email", "bob@example.com"],
        vec![
            "engineer-backfill",
            "--email",
            "bob@example.com",
            "--dry-run",
        ],
        vec!["rbac", "list"],
    ]
}

#[test]
fn a_member_cannot_rewrite_or_delete_other_principals_records() {
    let d = std::env::temp_dir().join(format!("axon_mx_authz_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);

    for args in destructive() {
        seed(&d, "\"alice@example.com\"");
        let before = std::fs::read_to_string(d.join("events.ndjson")).unwrap();

        // Premise: bob really is a restricted member here, or a refusal
        // proves nothing about members.
        let (_, stats) = run(&d, "bob@example.com", &["stats"]);
        assert!(
            stats.contains("Total records:    1"),
            "premise: bob must see only his own record:\n{stats}"
        );

        let (code, out) = run(&d, "bob@example.com", &args);
        assert_ne!(
            code,
            0,
            "`{}` succeeded for a member:\n{out}",
            args.join(" ")
        );
        assert!(
            out.contains("restricted to an admin"),
            "`{}` failed, but not for lack of authority — a refusal for the \
             wrong reason is not enforcement:\n{out}",
            args.join(" ")
        );
        let after = std::fs::read_to_string(d.join("events.ndjson")).unwrap();
        assert_eq!(
            before,
            after,
            "`{}` modified the ledger despite being refused",
            args.join(" ")
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// CONTROL: an admin must still be able to do the work.
#[test]
fn an_admin_can_still_run_maintenance() {
    let d = std::env::temp_dir().join(format!("axon_mx_admin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    seed(&d, "\"alice@example.com\"");
    let (code, out) = run(
        &d,
        "alice@example.com",
        &["prune", "--older-than", "2099-01-01", "--yes"],
    );
    assert_eq!(code, 0, "an admin must still be able to prune:\n{out}");
    let _ = std::fs::remove_dir_all(&d);
}

/// CONTROL: RBAC is INERT with no admins configured, and must stay that way —
/// otherwise every existing single-user ledger stops working at upgrade.
#[test]
fn a_ledger_with_no_admins_configured_is_unrestricted() {
    let d = std::env::temp_dir().join(format!("axon_mx_inert_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    seed(&d, "");
    let (code, out) = run(
        &d,
        "anyone@example.com",
        &["prune", "--older-than", "2099-01-01", "--yes"],
    );
    assert_eq!(
        code, 0,
        "with no admin list, the scheme is off and maintenance must work:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// CONTROL: reads are NOT admin-gated — they are filtered instead. Gating
/// them would be a different product, and would hide a filter regression
/// behind a refusal.
#[test]
fn reads_remain_open_to_members() {
    let d = std::env::temp_dir().join(format!("axon_mx_reads_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    seed(&d, "\"alice@example.com\"");
    for args in [
        vec!["stats"],
        vec!["diff", "--from", "1970-01-01", "--to", "2030-01-01"],
    ] {
        let (code, out) = run(&d, "bob@example.com", &args);
        assert_eq!(
            code,
            0,
            "`{}` must remain open to a member:\n{out}",
            args.join(" ")
        );
        assert!(
            !out.contains("alice@example.com"),
            "`{}` is open to members, so it must still be FILTERED:\n{out}",
            args.join(" ")
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// `replace_record` is guarded at the store API alongside `prune` and
/// `rewrite_principals`, but the test that pinned that guard exercised only
/// the other two: deleting its `require_unfiltered` call left the suite
/// green. Closing the gap the review named.
#[test]
fn replace_record_also_refuses_a_filtered_handle() {
    use axon_ledger::model::{Effect, LedgerRecord};
    use axon_ledger::store::Store;

    let d = std::env::temp_dir().join(format!("axon_mx_repl_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    seed(&d, "\"alice@example.com\"");
    let before = std::fs::read_to_string(d.join("events.ndjson")).unwrap();

    let mut filtered = Store::open_as(&d, Some("bob@example.com".into())).unwrap();
    assert_eq!(
        filtered.all().unwrap().len(),
        1,
        "premise: this handle must be filtered"
    );

    let rec = LedgerRecord {
        id: "a1".into(),
        principal: "agent:bob@example.com".into(),
        effect: Effect::GitCommit,
        causal_parent: None,
        ts_ms: 1000,
        payload: serde_json::json!({"sha": "zzz"}),
        repo: None,
    };
    assert!(
        filtered.replace_record("a1", &rec).is_err(),
        "replace_record was allowed through a filtered handle"
    );
    assert_eq!(
        before,
        std::fs::read_to_string(d.join("events.ndjson")).unwrap(),
        "the ledger changed despite the refusal"
    );
    let _ = std::fs::remove_dir_all(&d);
}
