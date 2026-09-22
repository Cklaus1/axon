//! Every read command must show a member exactly what a ledger containing
//! only their own records would show.
//!
//! TWO GUARDS THAT DID NOT WORK, and why this one is shaped as it is:
//!
//! 1. The original guard grepped for the literal `Store::open(` outside
//!    store.rs. That enforces NAMING, not authorization, so `open_for_write`
//!    used on a read path was invisible to it. It reported `2 passed` while
//!    `--as bob diff --json` returned alice's payload.
//!
//! 2. My first replacement drove the real CLI and asserted the other
//!    principal's name did not appear in the output. Better, but VACUOUS for
//!    every command that aggregates: `pre-deploy`, `weekly`, `audit` and
//!    `stats` never echo a principal, so the assertion could not fail for
//!    them however unfiltered the read was. Mutation proved it — pointing
//!    `pre-deploy` back at the unfiltered handle left the test GREEN.
//!
//! So the oracle here is an EQUIVALENCE, which needs no knowledge of what a
//! command prints: run each command as bob against a ledger holding alice's
//! records and bob's, then against a ledger holding only bob's. A filtered
//! read cannot tell the two apart. An unfiltered one cannot help but.

use std::path::{Path, PathBuf};
use std::process::Command;

fn bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_axon-ledger"))
}

/// The fixture must REACH each command's filters, or the equivalence oracle
/// is vacuous for that command: two ledgers a command cannot distinguish are
/// equal to it however unfiltered its read is. Mutation found this — with a
/// single old `git_commit` for alice, unfiltering `pre-deploy`, `weekly` and
/// `audit` left the test green. So alice gets one record of every effect, with
/// the payload keys each command reads (`commit_sha`, `files`,
/// `files_touched`), timestamped inside the rolling windows.
fn records_for(principal: &str, tag: &str, now_ms: u64, sha: &str) -> Vec<String> {
    let recent = now_ms - 86_400_000; // inside both the 7d and 30d windows
    let mut v = vec![
        format!(
            r#"{{"id":"{tag}1","principal":"{principal}","effect":"git_commit","causal_parent":null,"ts_ms":{recent},"payload":{{"sha":"{sha}","subject":"{tag} work","files":["src/widget/a.rs"]}},"repo":"api"}}"#
        ),
        format!(
            r#"{{"id":"{tag}2","principal":"{principal}","effect":"agent_session","causal_parent":null,"ts_ms":{recent},"payload":{{"goal":"{tag} goal","files_touched":["src/widget/a.rs"]}},"repo":"api"}}"#
        ),
        format!(
            r#"{{"id":"{tag}3","principal":"{principal}","effect":"agent_edge","causal_parent":null,"ts_ms":{recent},"payload":{{"commit_sha":"{sha}","goal":"{tag} goal"}},"repo":"api"}}"#
        ),
        format!(
            r#"{{"id":"{tag}4","principal":"{principal}","effect":"metric_outcome","causal_parent":null,"ts_ms":{recent},"payload":{{"metric":"latency","value":{}}},"repo":"api"}}"#,
            if tag == "alice" { 42 } else { 7 }
        ),
    ];
    if tag == "alice" {
        // `weekly` picks a 7-day window when it counts >=3 sessions in the
        // last 7 days and a 30-day window otherwise. Alice alone must carry
        // the count across that threshold, so the WINDOW bob gets differs
        // between the two ledgers — otherwise the probe reading unfiltered
        // is unobservable, which is how it survived mutation.
        for n in 5..8 {
            v.push(format!(
                r#"{{"id":"alice{n}","principal":"{principal}","effect":"agent_session","causal_parent":null,"ts_ms":{recent},"payload":{{"goal":"extra"}},"repo":"api"}}"#
            ));
        }
    } else {
        // ... and bob owns a record that falls INSIDE the 30-day window but
        // OUTSIDE the 7-day one, so a changed window changes bob's own output.
        let old_ms = now_ms - 14 * 86_400_000;
        v.push(format!(
            r#"{{"id":"bob9","principal":"{principal}","effect":"git_commit","causal_parent":null,"ts_ms":{old_ms},"payload":{{"sha":"bobold","subject":"bob older work","files":["src/widget/b.rs"]}},"repo":"api"}}"#
        ));
    }
    v
}

/// A real two-commit git repo, because `pre-deploy` diffs the ledger against
/// `git log` — fabricated SHAs match nothing, so alice's edge could not
/// affect the output and the command survived mutation.
fn git_repo(dir: &Path) -> (String, String) {
    std::fs::create_dir_all(dir).unwrap();
    let git = |args: &[&str]| {
        let out = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(args)
            .output()
            .unwrap_or_else(|e| panic!("git {args:?}: {e}"));
        // A missing/failing git must FAIL this test, not skip it: a skip that
        // cannot prove its own reason is indistinguishable from a pass.
        assert!(
            out.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_string()
    };
    git(&["init", "-q"]);
    git(&["config", "user.email", "t@example.com"]);
    git(&["config", "user.name", "t"]);
    let mut shas = Vec::new();
    for n in 0..3 {
        std::fs::write(dir.join(format!("f{n}.txt")), format!("{n}")).unwrap();
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", &format!("commit {n}")]);
        shas.push(git(&["rev-parse", "HEAD"]));
    }
    (shas[1].clone(), shas[2].clone())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}

/// RBAC is INERT when no admins are configured, so a ledger without this
/// file would pass every assertion below against any implementation.
fn seed(dir: &Path, records: &[String]) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(dir.join("events.ndjson"), records.join("\n") + "\n").unwrap();
    std::fs::write(dir.join("rbac.json"), r#"{"admins":["alice@example.com"]}"#).unwrap();
}

fn run(dir: &Path, caller: &str, args: &[&str]) -> String {
    let out = Command::new(bin())
        .arg("--ledger-dir")
        .arg(dir)
        .arg("--as")
        .arg(caller)
        .args(args)
        .output()
        .unwrap_or_else(|e| panic!("run {args:?}: {e}"));
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    // `weekly` derives its window from wall-clock now, so two invocations can
    // straddle a tick. Normalising long digit runs keeps that from flaking
    // WITHOUT weakening the oracle: a leaked record changes structure and
    // content, not just a timestamp's digits.
    // Only runs of 4+ digits (timestamps, epoch ms, years) are normalised.
    // Normalising EVERY digit also flattens the record COUNTS, which is the
    // signal — the control below caught exactly that when I tried it.
    let mut out = String::new();
    let mut run = String::new();
    for c in text.chars() {
        if c.is_ascii_digit() {
            run.push(c);
            continue;
        }
        if run.len() >= 4 {
            out.push_str("<num>");
        } else {
            out.push_str(&run);
        }
        run.clear();
        out.push(c);
    }
    if run.len() >= 4 {
        out.push_str("<num>");
    } else {
        out.push_str(&run);
    }
    out
}

fn read_commands() -> Vec<Vec<&'static str>> {
    vec![
        vec!["stats"],
        vec!["stats", "--json"],
        vec!["diff", "--from", "1970-01-01", "--to", "2030-01-01"],
        vec![
            "diff",
            "--from",
            "1970-01-01",
            "--to",
            "2030-01-01",
            "--json",
        ],
        vec!["as-of", "2030-01-01"],
        vec!["as-of", "2030-01-01", "--json"],
        vec!["history", "src/widget/a.rs"],
        vec!["weekly"],
        vec!["audit", "--module", "widget"],
    ]
}

#[test]
fn a_member_sees_exactly_what_a_ledger_of_their_own_records_would_show() {
    let base = std::env::temp_dir().join(format!("axon_rdfilter_{}", std::process::id()));
    let both = base.join("both");
    let bob_only = base.join("bob_only");
    let t = now_ms();
    let repo = base.join("repo");
    let (sha_a, sha_b) = git_repo(&repo);
    let repo_s = repo.to_string_lossy().to_string();
    let alice = records_for("git:alice@example.com", "alice", t, &sha_a);
    let bob = records_for("git:bob@example.com", "bob", t, &sha_b);
    let mut all_recs = alice.clone();
    all_recs.extend(bob.clone());
    seed(&both, &all_recs);
    seed(&bob_only, &bob);

    let mut commands = read_commands();
    commands.retain(|c| c[0] != "pre-deploy");
    // Bound to the fixture, not to literals: `why` must name a sha alice
    // actually owns, and `search` a term that actually matches her records.
    // With stale literals both commands found nothing on either ledger and
    // survived mutation while appearing to be covered.
    commands.push(vec!["why", sha_a.as_str()]);
    commands.push(vec!["search", "alice"]);
    commands.push(vec!["search", "goal"]);
    commands.push(vec!["pre-deploy", "--repo", "@REPO@", "HEAD~2..HEAD"]);
    commands.push(vec![
        "pre-deploy",
        "--repo",
        "@REPO@",
        "HEAD~2..HEAD",
        "--json",
    ]);

    for args in commands {
        let args: Vec<&str> = args
            .iter()
            .map(|a| if *a == "@REPO@" { repo_s.as_str() } else { *a })
            .collect();
        let a: Vec<&str> = args.clone();
        // A command that fails to PARSE produces the same error on both
        // ledgers, so the equivalence holds vacuously. `--range` was written
        // as a flag when it is positional, and this test passed while
        // pre-deploy never ran at all. A probe must prove it reached its
        // target before its result means anything.
        let with_alice = run(&both, "bob@example.com", &a);
        assert!(
            !with_alice.contains("unexpected argument")
                && !with_alice.contains("Usage: axon-ledger")
                && !with_alice.contains("error: "),
            "`{}` did not run — the equivalence below would be vacuous:\n{with_alice}",
            args.join(" ")
        );
        let without_alice = run(&bob_only, "bob@example.com", &a);
        assert_eq!(
            with_alice,
            without_alice,
            "`{}` as a member changed when another principal's records were \
             present, so it is reading past the filter",
            args.join(" ")
        );
    }

    // CONTROL: the oracle must be capable of failing. An admin sees more when
    // alice's records are present — if THIS were also equal, the two ledgers
    // would be indistinguishable to every caller and the loop above would
    // prove nothing.
    let admin_both = run(&both, "alice@example.com", &["stats"]);
    let admin_bob_only = run(&bob_only, "alice@example.com", &["stats"]);
    assert_ne!(
        admin_both, admin_bob_only,
        "an admin must distinguish the two ledgers, else this test is vacuous"
    );

    // CONTROL: the member must still see their OWN record — a read that
    // refuses everything would satisfy every equivalence above.
    let own = run(&both, "bob@example.com", &["stats"]);
    assert!(
        own.contains("Total records:    5"),
        "bob must still see his own record:\n{own}"
    );

    let _ = std::fs::remove_dir_all(&base);
}

/// Every verb the CLI exposes must be accounted for by name.
///
/// The first version of this asserted that each ALREADY-TESTED verb still
/// appears in `--help`, which is the opposite of the direction it claimed to
/// guard: a NEW read verb left it green, and "a new read verb would be
/// unguarded" was exactly what its own doc comment said it was for. It also
/// missed one that already existed — `prune --dry-run` read through the
/// unfiltered handle and disclosed whole-ledger aggregates to a member.
///
/// So this enumerates the CLI's ACTUAL surface and requires every verb to be
/// in one of two lists. A new verb fails until someone puts it in one.
#[test]
fn every_cli_verb_is_accounted_for_as_a_read_or_not() {
    let out = std::process::Command::new(bin())
        .arg("--help")
        .output()
        .expect("help");
    let help = String::from_utf8_lossy(&out.stdout).to_string();
    let block = help
        .split("Commands:")
        .nth(1)
        .and_then(|s| s.split("Options:").next())
        .expect("a Commands block in --help");
    let verbs: Vec<String> = block
        .lines()
        .filter_map(|l| {
            let t = l.trim_start();
            if l.starts_with("  ") && !l.starts_with("      ") && !t.is_empty() {
                t.split_whitespace().next().map(String::from)
            } else {
                None
            }
        })
        .filter(|v| v != "help")
        .collect();
    assert!(
        verbs.len() >= 15,
        "parsed only {} verbs from --help; the parser has drifted and this \
         guard is vacuous: {verbs:?}",
        verbs.len()
    );

    // Reads, each exercised by the equivalence oracle above.
    const READS: &[&str] = &[
        "why",
        "diff",
        "stats",
        "search",
        "as-of",
        "history",
        "pre-deploy",
        "weekly",
        "audit",
    ];
    // Not reads. `prune`, `engineer-backfill`, `session-refresh`, `refresh`
    // and `rbac` are admin-gated (see maintenance_authorization.rs), which is
    // why they are not in the filter oracle; the rest append or serve.
    const NON_READS: &[&str] = &[
        "ingest",
        "prune",
        "engineer-backfill",
        "session-refresh",
        "webhook",
        "rbac",
        "refresh",
        "mcp",
        "watch",
    ];

    for v in &verbs {
        assert!(
            READS.contains(&v.as_str()) || NON_READS.contains(&v.as_str()),
            "CLI verb `{v}` is in neither list. If it reads records, add it to \
             the equivalence oracle above; if it does not, add it to NON_READS \
             and say why."
        );
    }
    for v in READS.iter().chain(NON_READS) {
        assert!(
            verbs.iter().any(|x| x == v),
            "`{v}` is listed here but is no longer a CLI verb — the premise drifted"
        );
    }
}

// ── A filtered handle must not be able to rewrite the whole ledger ──────────

/// `prune`, `replace_record` and `rewrite_principals` each start from
/// `self.all()` — which applies the handle's view — and write the result back
/// as the ENTIRE ledger. On a filtered handle that deletes every record the
/// caller could not see: a read restriction turning into data destruction.
///
/// No caller does this today; all three go through `open_for_write`. That is
/// the reason to pin it rather than the reason not to. The read path held the
/// same way — "every call site remembers" — right up until five of eight
/// didn't.
#[test]
fn a_filtered_handle_cannot_rewrite_the_whole_ledger() {
    use axon_ledger::store::Store;

    let dir = std::env::temp_dir().join(format!("axon_ledger_mx_{}", std::process::id()));
    let t = now_ms();
    let mut recs = records_for("git:alice@example.com", "alice", t, "aaa");
    recs.extend(records_for("git:bob@example.com", "bob", t, "bbb"));
    seed(&dir, &recs);
    let before = std::fs::read_to_string(dir.join("events.ndjson")).unwrap();

    let mut filtered = Store::open_as(&dir, Some("bob@example.com".into())).unwrap();
    // Premise: this handle really is filtered, or the refusals below prove
    // nothing about filtered handles.
    assert!(
        filtered.all().unwrap().len() < recs.len(),
        "premise: bob's handle must see fewer records than the ledger holds"
    );

    assert!(filtered.prune(t + 1).is_err(), "prune was allowed");
    assert!(
        filtered.rewrite_principals("git:", "x:").is_err(),
        "rewrite_principals was allowed"
    );

    let after = std::fs::read_to_string(dir.join("events.ndjson")).unwrap();
    assert_eq!(
        before, after,
        "the ledger was modified by a refused maintenance call"
    );

    // CONTROL: an unfiltered handle must still be able to do the work, or
    // this is a refusal rather than a guard.
    let mut open = Store::open_for_write(&dir).unwrap();
    assert!(
        open.prune(t + 1).is_ok(),
        "maintenance must still work on an unfiltered handle"
    );

    let _ = std::fs::remove_dir_all(&dir);
}
