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

/// Seed with an OS identity holding real (authenticated) admin authority.
fn seed_auth(dir: &Path, claimed_admins: &str, authenticated_admins: &str) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("events.ndjson"), RECS).unwrap();
    std::fs::write(
        dir.join("rbac.json"),
        format!(
            "{{\"admins\":[{claimed_admins}],\"authenticated_admins\":[{authenticated_admins}]}}"
        ),
    )
    .unwrap();
}

/// This process's real OS identity, discovered INDEPENDENTLY of the code under
/// test (`id -un`), so a fixture built from it cannot agree with the
/// implementation merely by sharing its bug.
fn real_os_identity() -> String {
    let out = Command::new("id").arg("-un").output().expect("id -un");
    assert!(out.status.success(), "id -un failed");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

fn run_env(dir: &Path, caller: Option<&str>, env: &[(&str, &str)], args: &[&str]) -> (i32, String) {
    let mut c = Command::new(bin());
    c.arg("--ledger-dir").arg(dir);
    if let Some(who) = caller {
        c.arg("--as").arg(who);
    }
    for (k, v) in env {
        c.env(k, v);
    }
    let out = c.args(args).output().unwrap();
    (
        out.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&out.stdout).to_string() + &String::from_utf8_lossy(&out.stderr),
    )
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

/// CONTROL: an AUTHENTICATED admin must still be able to do the work.
///
/// Authority now comes from the OS identity listed in `authenticated_admins`,
/// not from `--as`, so this control names the real one. Without it the fix
/// could be "refuse every maintenance command" and every negative test would
/// still pass.
#[test]
fn an_authenticated_admin_can_still_run_maintenance() {
    let d = std::env::temp_dir().join(format!("axon_mx_admin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let me = real_os_identity();
    seed_auth(&d, "\"alice@example.com\"", &format!("\"{me}\""));
    // No `--as` at all: authority is the OS identity, nothing is claimed.
    let (code, out) = run_env(
        &d,
        None,
        &[],
        &["prune", "--older-than", "2099-01-01", "--yes"],
    );
    assert_eq!(
        code, 0,
        "an authenticated admin must still be able to prune:\n{out}"
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// The explicit development escape, and the fact that it is not entered by
/// accident.
///
/// A claimed identity may carry authority ONLY when the operator deliberately
/// arms `AXON_LEDGER_DEV_IMPERSONATE=1`. Values that are merely truthy-looking
/// must not arm it, or "explicit" degrades into "whatever was in the
/// environment".
#[test]
fn claimed_identity_carries_authority_only_under_the_armed_dev_escape() {
    let d = std::env::temp_dir().join(format!("axon_mx_dev_{}", std::process::id()));
    let prune = ["prune", "--older-than", "2099-01-01", "--yes"];

    // ARMED: the claim is honoured. This is the escape working as designed.
    let _ = std::fs::remove_dir_all(&d);
    seed(&d, "\"alice@example.com\"");
    let (code, out) = run_env(
        &d,
        Some("alice@example.com"),
        &[("AXON_LEDGER_DEV_IMPERSONATE", "1")],
        &prune,
    );
    assert_eq!(
        code, 0,
        "the armed dev escape must honour the claim:\n{out}"
    );

    // NOT ARMED, in each way it could be mistaken for armed.
    for value in ["0", "true", "yes", "", "1 "] {
        let _ = std::fs::remove_dir_all(&d);
        seed(&d, "\"alice@example.com\"");
        let before = std::fs::read_to_string(d.join("events.ndjson")).unwrap();
        let (code, out) = run_env(
            &d,
            Some("alice@example.com"),
            &[("AXON_LEDGER_DEV_IMPERSONATE", value)],
            &prune,
        );
        assert_ne!(
            code, 0,
            "AXON_LEDGER_DEV_IMPERSONATE={value:?} must not arm the escape:\n{out}"
        );
        assert_eq!(
            before,
            std::fs::read_to_string(d.join("events.ndjson")).unwrap(),
            "the ledger changed under a non-armed escape value {value:?}"
        );
    }
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

// ── KNOWN GAPS, pinned rather than silent ────────────────────────────────
//
// The two tests below document behavior this session decided NOT to change,
// with the repro that proves it. They exist so a future "fix" to
// `requires_admin`/`resolve_caller` cannot silently narrow or widen this
// trust boundary without someone reading why it is shaped this way.

/// A caller-supplied identity string must never, by itself, confer admin
/// authority — through ANY surface that accepts one.
///
/// REPRODUCED before the fix, on a two-principal ledger with
/// `admins = ["alice@example.com"]`: `--as alice@example.com prune
/// --older-than 2099-01-01 --yes`, run by any caller, reported
/// "Pruned 2; 0 remain" and emptied the file. The gate compared rbac.admins
/// against whatever the caller typed.
///
/// Every way the caller can name themselves is covered here, because fixing
/// only the flag would leave the environment variable doing the same job:
/// `--as`, `$AXON_PRINCIPAL`, and `$USER`/`$LOGNAME` (which are NOT an OS
/// identity — a caller sets them freely, which is exactly why the real uid is
/// what gets consulted).
#[test]
fn no_caller_supplied_identity_grants_admin_authority() {
    let d = std::env::temp_dir().join(format!("axon_mx_claim_{}", std::process::id()));
    let prune = ["prune", "--older-than", "2099-01-01", "--yes"];

    // Each surface names the identity it is trying to become, AND the fixture
    // makes that identity a genuine admin — otherwise the refusal could come
    // from "nobody is an admin here" rather than from the claim being
    // powerless, and the test would pass against a broken implementation.
    //
    // MEASURED: the first version of the $USER case seeded only `admins`,
    // leaving `authenticated_admins` empty. Mutating `authenticated_principal`
    // to read $USER instead of the real uid left this test GREEN, because the
    // empty list refused it anyway. The mutation is caught only once the
    // forged identity would otherwise WORK.
    /// label, claimed identity, env overrides, claimed admins, authenticated admins
    type Surface = (
        &'static str,
        Option<&'static str>,
        Vec<(&'static str, &'static str)>,
        &'static str,
        &'static str,
    );
    let surfaces: Vec<Surface> = vec![
        (
            "--as flag",
            Some("alice@example.com"),
            vec![],
            "\"alice@example.com\"",
            "",
        ),
        (
            "AXON_PRINCIPAL",
            None,
            vec![("AXON_PRINCIPAL", "alice@example.com")],
            "\"alice@example.com\"",
            "",
        ),
        (
            "forged $USER/$LOGNAME",
            Some("alice@example.com"),
            vec![("USER", "alice"), ("LOGNAME", "alice")],
            "\"alice@example.com\"",
            // `alice` IS an authenticated admin here. If the implementation
            // took identity from $USER, the forgery would succeed.
            "\"alice\"",
        ),
    ];

    for (label, claim, env, claimed_admins, auth_admins) in surfaces {
        let _ = std::fs::remove_dir_all(&d);
        seed_auth(&d, claimed_admins, auth_admins);
        let before = std::fs::read_to_string(d.join("events.ndjson")).unwrap();

        let (code, out) = run_env(&d, claim, &env, &prune);
        assert_ne!(
            code, 0,
            "{label}: a claimed identity was granted admin:\n{out}"
        );
        assert!(
            out.contains("does not grant authority"),
            "{label}: refused, but not for lack of authority — a refusal for \
             another reason is not this gate working:\n{out}"
        );
        assert_eq!(
            before,
            std::fs::read_to_string(d.join("events.ndjson")).unwrap(),
            "{label}: the ledger changed despite the refusal"
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}

/// The OS identity itself must be what is checked, not merely "some string
/// that is not `--as`".
///
/// Names an OS identity that is NOT this process's, and confirms the refusal;
/// paired with `an_authenticated_admin_can_still_run_maintenance`, which names
/// one that IS. Together they show the gate reads the real uid rather than
/// accepting or refusing everything.
#[test]
fn an_authenticated_identity_that_is_not_an_admin_is_refused() {
    let d = std::env::temp_dir().join(format!("axon_mx_notadmin_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let me = real_os_identity();
    let not_me = format!("definitely-not-{me}");
    seed_auth(&d, "", &format!("\"{not_me}\""));
    let before = std::fs::read_to_string(d.join("events.ndjson")).unwrap();

    let (code, out) = run_env(
        &d,
        None,
        &[],
        &["prune", "--older-than", "2099-01-01", "--yes"],
    );
    assert_ne!(code, 0, "a non-admin OS identity was granted admin:\n{out}");
    assert_eq!(
        before,
        std::fs::read_to_string(d.join("events.ndjson")).unwrap()
    );
    let _ = std::fs::remove_dir_all(&d);
}

/// Appending verbs are deliberately NOT admin-gated (commit 9c86b41: "whether
/// an arbitrary caller may append is a real question and a different one").
/// A disproof review demonstrated the consequence: a member can write a
/// record ATTRIBUTED to another principal.
///
/// Pinned as a KNOWN GAP, not fixed here — closing it (should appends be
/// self-attributed only? admin-cosigned? unrestricted, as today?) is a
/// policy decision, and this session's scope was the rewrite/delete path
/// that had NO check at all, not appends that were considered and left open.
#[test]
fn a_member_can_append_a_record_attributed_to_another_principal() {
    let d = std::env::temp_dir().join(format!("axon_mx_forge_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let sess_dir = d.join("sess");
    std::fs::create_dir_all(&sess_dir).unwrap();
    std::fs::write(
        sess_dir.join("s1.jsonl"),
        r#"{"type":"user","message":{"role":"user","content":"hello"},"timestamp":"2026-09-20T10:00:00Z","sessionId":"s1"}"#,
    )
    .unwrap();
    std::fs::write(d.join("events.ndjson"), "").unwrap();
    std::fs::write(d.join("rbac.json"), r#"{"admins":["alice@example.com"]}"#).unwrap();

    let (code, out) = run(
        &d,
        "bob@example.com",
        &[
            "ingest",
            "session",
            sess_dir.join("s1.jsonl").to_str().unwrap(),
            "--engineer",
            "alice@example.com",
        ],
    );
    assert_eq!(code, 0, "documented: ingest is not admin-gated:\n{out}");

    let ledger = std::fs::read_to_string(d.join("events.ndjson")).unwrap();
    assert!(
        ledger.contains("\"principal\":\"alice@example.com\""),
        "documented: bob wrote a record attributed to alice:\n{ledger}"
    );

    let _ = std::fs::remove_dir_all(&d);
}

// ── Where the boundary actually is ──────────────────────────────────────────

/// THE BOUNDARY, stated and then demonstrated: axon-ledger's RBAC constrains
/// what the LEDGER CLI will do, and cannot constrain what a process with write
/// access to the ledger file can do by other means.
///
/// This matters for reading the tests above correctly. They prove a caller
/// cannot obtain admin authority THROUGH axon-ledger by asserting an identity.
/// They do not — and no userspace check could — prevent someone who already
/// holds OS write permission on `events.ndjson` from simply writing to it.
/// This test performs exactly that bypass with `std::fs` and no CLI at all,
/// so the limit is executable rather than a claim in a comment.
///
/// The enforcement for THAT case is the filesystem: a ledger whose directory
/// is not writable by a principal is not modifiable by them, whatever the CLI
/// does. RBAC is the narrower, in-process layer for callers who share that
/// access — a team ledger on a shared box, a CI account several people drive —
/// which is precisely the case where the `--as` hole was exploitable and is
/// now closed.
#[test]
fn rbac_does_not_and_cannot_bind_a_writer_who_bypasses_the_cli() {
    let d = std::env::temp_dir().join(format!("axon_mx_bound_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    let me = real_os_identity();
    // A ledger where the caller is NOT an admin by any route: not claimed,
    // not authenticated. The CLI must refuse them.
    seed_auth(&d, "\"alice@example.com\"", "\"definitely-not-me\"");

    let (code, out) = run_env(
        &d,
        None,
        &[],
        &["prune", "--older-than", "2099-01-01", "--yes"],
    );
    assert_ne!(code, 0, "premise: the CLI must refuse this caller:\n{out}");
    assert!(
        !std::fs::read_to_string(d.join("events.ndjson"))
            .unwrap()
            .is_empty(),
        "premise: the ledger must still be intact after the refusal"
    );

    // The same destructive outcome, without the CLI. Same process, same uid,
    // same file — no authority check exists on this path because there is no
    // axon-ledger in it.
    std::fs::write(d.join("events.ndjson"), "").unwrap();
    assert!(
        std::fs::read_to_string(d.join("events.ndjson"))
            .unwrap()
            .is_empty(),
        "the ledger file is writable by this process, which is the boundary \
         being documented: RBAC governs the CLI, the filesystem governs the file"
    );

    // And the converse, so the boundary is not merely an excuse: the OS layer
    // is real. A directory this process cannot write is not writable through
    // ANY path — checked only when the test is not running as root, since
    // root is exempt from file permission bits and the check would be
    // vacuous rather than reassuring.
    if me != "root" {
        let locked = d.join("locked");
        std::fs::create_dir_all(&locked).unwrap();
        std::fs::write(locked.join("events.ndjson"), "x").unwrap();
        let mut perms = std::fs::metadata(&locked).unwrap().permissions();
        std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o500);
        std::fs::set_permissions(&locked, perms).unwrap();
        assert!(
            std::fs::write(locked.join("new.ndjson"), "y").is_err(),
            "a non-writable directory must reject writes — the OS layer this \
             boundary defers to has to actually hold"
        );
    } else {
        eprintln!(
            "boundary test: the filesystem-enforcement half did NOT run — this \
             process is root, which bypasses permission bits, so the assertion \
             would pass without testing anything. The CLI-bypass half above DID run."
        );
    }
    let _ = std::fs::remove_dir_all(&d);
}
