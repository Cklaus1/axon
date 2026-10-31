//! R6 §6 tests for L1: seal tamper per field, chain splice/reorder/truncation,
//! cross-protocol pairs, write failures, concurrency, and every C1 finding
//! seeded once. Keys here are TEST keys derived from a fixed, public string.

use super::*;
use crate::repair_envelope::{self as re, RepairEnvelope};
use serde_json::json;
use std::os::unix::fs::PermissionsExt;

/// A fixed TEST operator key (public, never a real secret).
const OP_KEY: &[u8] = b"nh-r1 test operator key, not a secret";
const NOW: u64 = 1_800_000_000;

fn key() -> SealKey {
    SealKey::derive(OP_KEY).expect("test key")
}

fn me() -> u32 {
    // SAFETY: geteuid cannot fail.
    unsafe { libc::geteuid() }
}

fn tmpdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!(
        "axon-os-r6-{tag}-{}-{}",
        std::process::id(),
        re::new_id().unwrap()
    ));
    std::fs::create_dir(&d).unwrap();
    std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}

fn envelope(id: &str, store_id: &str, ledger_id: &str) -> RepairEnvelope {
    serde_json::from_value(json!({
        "schema": re::ENVELOPE_SCHEMA,
        "envelope_id": id,
        "spent_store_id": store_id,
        "run_ledger_id": ledger_id,
        "issued_by": "operator",
        "issued_unix": NOW - 10,
        "expires_unix": NOW + 3_600,
        "task": {"workspace": "/srv/ws", "file": "src/main.axon", "symbol": "parse",
                 "pristine_sha256": "a".repeat(64),
                 "imports_digest": format!("axsha256:{}", "b".repeat(64))},
        "write_prefixes": ["src"],
        "scopes": {"fs_read": [], "fs_write": [], "net": []},
        "capabilities": {"fence_sha256": "c".repeat(64),
                         "target_grant": {"symbol": "parse"}},
        "check": {"name": "test_parse", "digest": "d".repeat(64)},
        "budget": {"max_proposals": 4, "deadline_ms": 60_000, "check_timeout_ms": 10_000,
                   "candidates": 3, "max_steps": 8},
        "generator": {"kind": "ai", "model": "claude-x", "provider": "anthropic",
                      "endpoint_host": "api.anthropic.com"},
        "decision": re::DECISION,
        "sig": "",
    }))
    .expect("envelope fixture")
}

/// A ledger, a spent store, and envelopes issued + consumed into them.
struct Fx {
    ledger: PathBuf,
    spent: PathBuf,
    ledger_id: String,
    store_id: String,
}

impl Fx {
    fn new(tag: &str) -> Fx {
        let ledger = tmpdir(&format!("{tag}-l"));
        let spent = tmpdir(&format!("{tag}-s"));
        let ledger_id = init_ledger(&ledger, me()).unwrap();
        let store_id = re::init_store(&spent, me()).unwrap();
        Fx {
            ledger,
            spent,
            ledger_id,
            store_id,
        }
    }

    /// A verified envelope, issued and consumed (as phase B would).
    fn run(&self) -> VerifiedEnvelope {
        let id = re::new_id().unwrap();
        let env = envelope(&id, &self.store_id, &self.ledger_id);
        let v = VerifiedEnvelope {
            envelope: env,
            digest: format!("cl22:{}", "f".repeat(64)),
        };
        re::issue(&self.spent, me(), &self.store_id, &id, NOW).unwrap();
        re::consume(&self.spent, me(), &v, NOW).unwrap();
        v
    }

    fn append(&self, v: &VerifiedEnvelope, body: Body) -> Result<Head, String> {
        append(&self.ledger, me(), &key(), v, body, NOW)
    }

    fn audit(&self) -> Vec<Finding> {
        audit(&self.ledger, &self.spent, me(), &key(), &BTreeMap::new())
            .unwrap()
            .findings
    }

    fn lines(&self) -> Vec<String> {
        std::fs::read_to_string(self.ledger.join(LEDGER_FILE))
            .unwrap()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn set_lines(&self, lines: &[String]) {
        let mut text = lines.join("\n");
        if !lines.is_empty() {
            text.push('\n');
        }
        std::fs::write(self.ledger.join(LEDGER_FILE), text).unwrap();
    }
}

impl Drop for Fx {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.ledger);
        let _ = std::fs::remove_dir_all(&self.spent);
    }
}

fn tree(entries: &[(&str, char)]) -> Tree {
    entries
        .iter()
        .map(|(p, c)| (p.to_string(), c.to_string().repeat(64)))
        .collect()
}

fn started(t: Tree) -> Body {
    Body::Started(Started {
        enforcement: json!([{"id": "E1", "status": "admit", "basis": "pinned"}]),
        pinned_tree_before: t,
        budget: budget(),
    })
}

fn budget() -> Budget {
    serde_json::from_value(json!({"max_proposals": 4, "deadline_ms": 60_000,
        "check_timeout_ms": 10_000, "candidates": 3, "max_steps": 8}))
    .unwrap()
}

fn write(path: &str, before: Option<char>, after: Option<char>) -> WriteEntry {
    WriteEntry {
        site: "apply".into(),
        path: path.into(),
        before_sha256: before.map(|c| c.to_string().repeat(64)),
        after_sha256: after.map(|c| c.to_string().repeat(64)),
    }
}

fn terminal(
    exit: i64,
    accepted: Option<(&str, char)>,
    writes: Vec<WriteEntry>,
    after: Tree,
) -> Body {
    Body::Terminal(Terminal {
        outcome: if exit == 0 { "repaired" } else { "failed" }.into(),
        exit_code: exit,
        stopped_by: None,
        accepted: accepted.map(|(p, c)| Accepted {
            path: p.into(),
            sha256: c.to_string().repeat(64),
        }),
        accepted_refusal: None,
        writes,
        pinned_tree_after: after,
        episode_digest: Some(format!("cl22:{}", "9".repeat(64))),
        budget: budget(),
    })
}

/// One complete, clean run: started + terminal (exit 0, src/a 1 -> 2).
fn clean_run(fx: &Fx) -> VerifiedEnvelope {
    let v = fx.run();
    fx.append(&v, started(tree(&[("src/a", '1')]))).unwrap();
    fx.append(
        &v,
        terminal(
            0,
            Some(("src/a", '2')),
            vec![write("src/a", Some('1'), Some('2'))],
            tree(&[("src/a", '2')]),
        ),
    )
    .unwrap();
    v
}

fn names(f: &[Finding]) -> Vec<&'static str> {
    f.iter().map(|f| f.finding).collect()
}

// ── the clean path ───────────────────────────────────────────────────────────

#[test]
fn a_clean_ledger_audits_clean_and_the_head_is_the_last_record() {
    let fx = Fx::new("clean");
    clean_run(&fx);
    let v2 = clean_run(&fx);
    assert_eq!(
        fx.audit(),
        vec![],
        "ATTACK: a clean ledger produced findings"
    );
    let lines = fx.lines();
    assert_eq!(lines.len(), 4);
    let (last, d) = parse_record(&lines[3], &key()).unwrap();
    assert_eq!(last.seq, 3);
    assert_eq!(last.envelope_id, v2.envelope.envelope_id);
    let head = parse_head(
        &std::fs::read_to_string(fx.ledger.join(HEAD_FILE)).unwrap(),
        &key(),
    )
    .unwrap();
    assert_eq!((head.seq, head.record_digest), (3, d));
    // every record chains to the one before it
    let mut prev = None;
    for (i, l) in lines.iter().enumerate() {
        let (r, d) = parse_record(l, &key()).unwrap();
        assert_eq!((r.seq, r.prev.clone()), (i as u64, prev));
        prev = Some(d);
    }
}

// ── seal tamper per field ────────────────────────────────────────────────────

#[test]
fn a_tampered_field_breaks_the_seal() {
    let fx = Fx::new("tamper");
    clean_run(&fx);
    let lines = fx.lines();
    for (i, line) in lines.iter().enumerate() {
        let v: Value = serde_json::from_str(line).unwrap();
        for field in v.as_object().unwrap().keys().filter(|k| *k != "seal") {
            let mut t = v.clone();
            t[field] = match &v[field] {
                Value::Null => json!("x"),
                Value::String(s) => json!(format!("{s}0")),
                Value::Number(n) => json!(n.as_u64().unwrap_or(0) + 1),
                _ => json!(null),
            };
            let bytes = axon_canonical::canonical_bytes(&t).unwrap();
            let e = parse_record(std::str::from_utf8(&bytes).unwrap(), &key()).expect_err(
                &format!("ATTACK: record {i} with {field} changed kept its seal"),
            );
            // `schema` is refused before the seal is checked.
            assert!(e.contains("seal") || field == "schema", "{field}: {e}");
        }
    }
    // a seal under another key (the axon-os record subkey) does not verify
    let other = SealKey(axon_attest::hmac_sha256(
        OP_KEY,
        crate::RECORD_DOMAIN.as_bytes(),
    ));
    let e = parse_record(&lines[0], &other)
        .expect_err("ATTACK: a run record verified under the axon-os record subkey");
    assert!(e.contains("seal"), "{e}");
}

#[test]
fn a_record_line_in_another_spelling_is_refused() {
    let fx = Fx::new("spell");
    clean_run(&fx);
    let line = &fx.lines()[0];
    // reordered keys, same value, same seal: not the canonical line
    let v: Value = serde_json::from_str(line).unwrap();
    let pretty = serde_json::to_string_pretty(&v).unwrap();
    let e = parse_record(&pretty, &key()).expect_err("ATTACK: a non-canonical line was accepted");
    assert!(e.contains("canonical form"), "{e}");
    // a dropped null field, re-sealed: the normal form is every field present
    let mut t = v.clone();
    t.as_object_mut().unwrap().remove("outcome");
    t.as_object_mut().unwrap().remove("seal");
    let seal = key().seal(&t).unwrap();
    t["seal"] = json!(seal);
    let bytes = axon_canonical::canonical_bytes(&t).unwrap();
    let e = parse_record(std::str::from_utf8(&bytes).unwrap(), &key())
        .expect_err("ATTACK: a record missing a null field was accepted");
    assert!(e.contains("every field is present"), "{e}");
}

// ── cross-protocol ───────────────────────────────────────────────────────────

#[test]
fn a_head_never_reads_as_a_record_and_the_reverse() {
    let fx = Fx::new("xproto");
    clean_run(&fx);
    let head = std::fs::read_to_string(fx.ledger.join(HEAD_FILE)).unwrap();
    let e = parse_record(head.trim_end(), &key()).expect_err("ATTACK: a head read as a record");
    // named by the schema check itself, not by a later typed-parse error
    // that happens to list `schema` among the expected fields
    assert!(
        e.contains("schema is not"),
        "ATTACK: a head was judged as a record by: {e}"
    );
    let line = &fx.lines()[0];
    let e = parse_head(line, &key()).expect_err("ATTACK: a record read as a head");
    assert!(
        e.contains("schema is not"),
        "ATTACK: a record was judged as a head by: {e}"
    );
}

#[test]
fn the_subkey_is_not_any_other_operator_derivation() {
    let k = key();
    for (what, domain) in [
        ("the axon-os record subkey", crate::RECORD_DOMAIN),
        ("the /3 token subkey", crate::TOKEN_DOMAIN),
        ("the envelope seed", re::ENVELOPE_SEED_DOMAIN),
    ] {
        assert_ne!(
            k.0,
            axon_attest::hmac_sha256(OP_KEY, domain.as_bytes()),
            "ATTACK: the run-record subkey equals {what}"
        );
    }
    assert!(
        SealKey::derive(b"short").is_err(),
        "ATTACK: a run-record key was derived from a short operator key"
    );
    assert_eq!(format!("{k:?}"), "SealKey(<redacted>)");
}

// ── chain splice, reorder, truncation ────────────────────────────────────────

#[test]
fn a_reordered_ledger_is_a_chain_break() {
    let fx = Fx::new("reorder");
    clean_run(&fx);
    let mut l = fx.lines();
    l.swap(0, 1);
    fx.set_lines(&l);
    assert!(
        names(&fx.audit()).contains(&"chain-break"),
        "ATTACK: a reordered ledger audited without a chain-break"
    );
}

#[test]
fn a_record_spliced_from_another_ledger_is_a_chain_break() {
    let a = Fx::new("splice-a");
    let b = Fx::new("splice-b");
    clean_run(&a);
    clean_run(&b);
    let mut l = a.lines();
    l[1] = b.lines()[1].clone();
    a.set_lines(&l);
    assert!(
        names(&a.audit()).contains(&"chain-break"),
        "ATTACK: a record spliced in from another ledger audited without a chain-break"
    );
}

#[test]
fn a_truncated_ledger_is_found_by_the_head_and_the_spent_store() {
    let fx = Fx::new("trunc");
    clean_run(&fx);
    clean_run(&fx);
    let l = fx.lines();
    fx.set_lines(&l[..2]);
    let n = names(&fx.audit());
    assert!(
        n.contains(&"head-beyond-ledger"),
        "ATTACK: a truncated ledger audited without head-beyond-ledger: {n:?}"
    );
    assert!(
        n.contains(&"consumed-no-record"),
        "ATTACK: a consumed envelope with no record went unreported: {n:?}"
    );
}

#[test]
fn a_torn_last_line_refuses_the_next_append() {
    let fx = Fx::new("torn");
    let v = clean_run(&fx);
    let mut text = std::fs::read_to_string(fx.ledger.join(LEDGER_FILE)).unwrap();
    text.push_str("{\"schema\":\"cortex-run-rec");
    std::fs::write(fx.ledger.join(LEDGER_FILE), text).unwrap();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: an append after a torn record was accepted");
    assert!(
        e.contains("torn"),
        "ATTACK: a torn record was refused only by a later check: {e}"
    );
}

#[test]
fn a_head_that_disagrees_refuses_the_next_append() {
    let fx = Fx::new("headdis");
    clean_run(&fx);
    let v = clean_run(&fx);
    let l = fx.lines();
    fx.set_lines(&l[..2]);
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: an append onto a ledger behind its head was accepted");
    assert!(e.contains("head does not name"), "{e}");
}

#[test]
fn a_crash_between_append_and_head_is_an_unacknowledged_tail_and_appends_resume() {
    let fx = Fx::new("crash");
    let v = fx.run();
    fx.append(&v, started(tree(&[("src/a", '1')]))).unwrap();
    let head0 = std::fs::read(fx.ledger.join(HEAD_FILE)).unwrap();
    fx.append(
        &v,
        terminal(
            0,
            Some(("src/a", '2')),
            vec![write("src/a", Some('1'), Some('2'))],
            tree(&[("src/a", '2')]),
        ),
    )
    .unwrap();
    // the crash: the head never moved past seq 0
    std::fs::write(fx.ledger.join(HEAD_FILE), &head0).unwrap();
    assert_eq!(
        names(&fx.audit()),
        vec!["unacknowledged-tail"],
        "ATTACK: an unacknowledged record went unreported"
    );
    // the next append accepts a head one behind, and moves it
    clean_run(&fx);
    assert_eq!(fx.audit(), vec![]);
}

#[test]
fn a_ledger_with_records_and_no_head_is_an_unacknowledged_tail() {
    let fx = Fx::new("nohead");
    clean_run(&fx);
    std::fs::remove_file(fx.ledger.join(HEAD_FILE)).unwrap();
    assert!(names(&fx.audit()).contains(&"unacknowledged-tail"));
}

// ── the sink ─────────────────────────────────────────────────────────────────

#[test]
fn a_ledger_inits_once_and_an_envelope_for_another_ledger_is_refused() {
    let fx = Fx::new("init");
    let e = init_ledger(&fx.ledger, me()).expect_err("ATTACK: a ledger was initialised twice");
    assert!(e.contains(LEDGER_ID_FILE), "{e}");
    let mut v = fx.run();
    v.envelope.run_ledger_id = re::new_id().unwrap();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: an envelope for another ledger recorded here");
    assert!(e.contains("records into"), "{e}");
    assert!(fx.lines().is_empty());
}

#[test]
fn a_ledger_others_can_reach_is_refused() {
    let fx = Fx::new("perm");
    std::fs::set_permissions(&fx.ledger, std::fs::Permissions::from_mode(0o770)).unwrap();
    let v = fx.run();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: a run ledger its group can write was appended to");
    assert!(e.contains("run ledger"), "{e}");
    std::fs::set_permissions(&fx.ledger, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(
        fx.ledger.join(LEDGER_FILE),
        std::fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: a ledger file others can read was appended to");
    assert!(e.contains("run ledger file"), "{e}");
}

#[test]
fn a_failed_started_append_writes_nothing_and_the_spent_store_names_it() {
    let fx = Fx::new("fail");
    let v = fx.run();
    // the write failure: the ledger file is gone
    std::fs::remove_file(fx.ledger.join(LEDGER_FILE)).unwrap();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: a started append to a missing ledger file succeeded");
    mark_unrecorded(&fx.spent, me(), &v.envelope.envelope_id, &e, NOW).unwrap();
    assert!(
        mark_unrecorded(&fx.spent, me(), &v.envelope.envelope_id, "again", NOW).is_err(),
        "ATTACK: a failed-append marker was overwritten"
    );
    std::fs::write(fx.ledger.join(LEDGER_FILE), b"").unwrap();
    std::fs::set_permissions(
        fx.ledger.join(LEDGER_FILE),
        std::fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let f = fx.audit();
    assert_eq!(names(&f), vec!["consumed-no-record"]);
    assert!(
        f[0].detail.contains("started append failed"),
        "ATTACK: a failed started append read as truncation: {}",
        f[0].detail
    );
}

#[test]
fn record_bodies_are_checked_before_they_are_written() {
    let fx = Fx::new("body");
    let v = fx.run();
    // exit 0 with no accepted artifact and no refusal
    let e = fx
        .append(&v, terminal(0, None, vec![], Tree::new()))
        .expect_err("ATTACK: an exit-0 terminal record without its accepted artifact was written");
    assert!(e.contains("exactly one of accepted"), "{e}");
    // exit 1 with an accepted artifact
    let e = fx
        .append(&v, terminal(1, Some(("src/a", '2')), vec![], Tree::new()))
        .expect_err("ATTACK: a failed run's terminal record carried an accepted artifact");
    assert!(e.contains("carries an accepted"), "{e}");
    // a write entry with neither digest
    let e = fx
        .append(
            &v,
            terminal(1, None, vec![write("src/a", None, None)], Tree::new()),
        )
        .expect_err("ATTACK: a write entry with no digests was recorded");
    assert!(e.contains("malformed"), "{e}");
    assert!(fx.lines().is_empty());
}

#[test]
fn four_parallel_runs_append_eight_records_on_one_valid_chain() {
    let fx = Fx::new("conc");
    let envs: Vec<VerifiedEnvelope> = (0..4).map(|_| fx.run()).collect();
    std::thread::scope(|s| {
        for v in &envs {
            let fx = &fx;
            s.spawn(move || {
                fx.append(v, started(tree(&[("src/a", '1')]))).unwrap();
                fx.append(v, terminal(1, None, vec![], tree(&[("src/a", '1')])))
                    .unwrap();
            });
        }
    });
    let lines = fx.lines();
    assert_eq!(lines.len(), 8, "ATTACK: a concurrent append was lost");
    let ids: BTreeSet<String> = lines
        .iter()
        .map(|l| parse_record(l, &key()).unwrap().0.envelope_id)
        .collect();
    let want: BTreeSet<String> = envs
        .iter()
        .map(|v| v.envelope.envelope_id.clone())
        .collect();
    assert_eq!(ids, want);
    assert_eq!(
        fx.audit(),
        vec![],
        "ATTACK: concurrent appends broke the chain"
    );
}

// ── C1: every finding seeded once ────────────────────────────────────────────

#[test]
fn c1_started_no_terminal() {
    let fx = Fx::new("snt");
    let v = fx.run();
    fx.append(&v, started(Tree::new())).unwrap();
    assert_eq!(names(&fx.audit()), vec!["started-no-terminal"]);
}

#[test]
fn c1_terminal_no_started() {
    let fx = Fx::new("tns");
    let v = fx.run();
    fx.append(&v, terminal(1, None, vec![], Tree::new()))
        .unwrap();
    assert_eq!(names(&fx.audit()), vec!["terminal-no-started"]);
}

#[test]
fn c1_record_not_consumed() {
    let fx = Fx::new("rnc");
    let v = fx.run();
    fx.append(&v, started(Tree::new())).unwrap();
    fx.append(&v, terminal(1, None, vec![], Tree::new()))
        .unwrap();
    std::fs::remove_file(fx.spent.join(format!("{}.used", v.envelope.envelope_id))).unwrap();
    assert_eq!(
        names(&fx.audit()),
        vec!["record-not-consumed"],
        "ATTACK: a record with no consumed envelope went unreported"
    );
}

#[test]
fn c1_duplicate_run() {
    let fx = Fx::new("dup");
    let v = clean_run(&fx);
    fx.append(&v, started(tree(&[("src/a", '1')]))).unwrap();
    assert!(
        names(&fx.audit()).contains(&"duplicate-run"),
        "ATTACK: one envelope recorded twice went unreported"
    );
}

#[test]
fn c1_seal_break() {
    let fx = Fx::new("seal");
    clean_run(&fx);
    let mut l = fx.lines();
    l[1] = l[1].replace("\"repaired\"", "\"repaire0\"");
    fx.set_lines(&l);
    assert!(
        names(&fx.audit()).contains(&"seal-break"),
        "ATTACK: a rewritten outcome audited without a seal-break"
    );
}

#[test]
fn c1_accepted_mismatch() {
    let fx = Fx::new("acc");
    let v = fx.run();
    fx.append(&v, started(tree(&[("src/a", '1')]))).unwrap();
    fx.append(
        &v,
        terminal(
            0,
            Some(("src/a", '3')),
            vec![write("src/a", Some('1'), Some('2'))],
            tree(&[("src/a", '2')]),
        ),
    )
    .unwrap();
    assert_eq!(
        names(&fx.audit()),
        vec!["accepted-mismatch"],
        "ATTACK: an accepted digest no write produced went unreported"
    );
}

#[test]
fn c1_a_visible_copy_does_not_count_as_the_accepted_write() {
    let fx = Fx::new("vis");
    let v = fx.run();
    fx.append(&v, started(tree(&[("src/a", '1')]))).unwrap();
    let mut vis = write("src/a", Some('2'), Some('3'));
    vis.site = VISIBLE_SITE.into();
    fx.append(
        &v,
        terminal(
            0,
            Some(("src/a", '3')),
            vec![write("src/a", Some('1'), Some('2')), vis],
            tree(&[("src/a", '2')]),
        ),
    )
    .unwrap();
    assert!(
        names(&fx.audit()).contains(&"accepted-mismatch"),
        "ATTACK: a visible copy stood in for the accepted write"
    );
}

#[test]
fn c1_unattributed_change() {
    let fx = Fx::new("unattr");
    let v = fx.run();
    fx.append(&v, started(tree(&[("src/a", '1'), ("src/b", '5')])))
        .unwrap();
    fx.append(
        &v,
        terminal(1, None, vec![], tree(&[("src/a", '1'), ("src/b", '6')])),
    )
    .unwrap();
    let f = fx.audit();
    assert_eq!(
        names(&f),
        vec!["unattributed-change"],
        "ATTACK: a pinned-tree change no write explains went unreported"
    );
    assert!(f[0].detail.starts_with("src/b"), "{}", f[0].detail);
}

#[test]
fn c1_envelope_mismatch() {
    let fx = Fx::new("envmis");
    let v = clean_run(&fx);
    let mut supplied = BTreeMap::new();
    supplied.insert(
        v.envelope.envelope_id.clone(),
        format!("cl22:{}", "0".repeat(64)),
    );
    let f = audit(&fx.ledger, &fx.spent, me(), &key(), &supplied)
        .unwrap()
        .findings;
    assert_eq!(
        names(&f),
        vec!["envelope-mismatch"],
        "ATTACK: records naming another envelope digest went unreported"
    );
}

/// Security F10: a `.used` record C1 cannot read (garbled, not a file, or
/// naming no `envelope_digest`) skipped the store-side digest check with no
/// finding. It is its own finding, whether or not the run was recorded.
#[test]
fn c1_an_unreadable_used_record_is_a_finding() {
    for (case, tamper) in [("garbled", 0u8), ("a directory", 1), ("no digest", 2)] {
        let fx = Fx::new(&format!("used-{tamper}"));
        let v = clean_run(&fx);
        let used = fx.spent.join(format!("{}.used", v.envelope.envelope_id));
        match tamper {
            0 => std::fs::write(&used, "{not json\n").unwrap(),
            1 => {
                std::fs::remove_file(&used).unwrap();
                std::fs::create_dir(&used).unwrap();
            }
            _ => std::fs::write(&used, "{\"spent_unix\":1}\n").unwrap(),
        }
        assert_eq!(
            names(&fx.audit()),
            vec!["used-unreadable"],
            "ATTACK: a {case} .used record skipped the digest check with no finding"
        );
    }
}

#[test]
fn every_finding_name_is_distinct() {
    let set: BTreeSet<&str> = FINDINGS.iter().copied().collect();
    assert_eq!(set.len(), FINDINGS.len());
}

// ── one attack per refusal site ──────────────────────────────────────────────

/// The clean run's started and terminal records, as typed values.
fn clean_records() -> (RunRecord, RunRecord) {
    let fx = Fx::new("recs");
    clean_run(&fx);
    let l = fx.lines();
    (
        parse_record(&l[0], &key()).unwrap().0,
        parse_record(&l[1], &key()).unwrap().0,
    )
}

#[test]
fn each_malformed_record_is_refused() {
    let (s, t) = clean_records();
    validate_record(&s).expect("control: the clean started record");
    validate_record(&t).expect("control: the clean terminal record");
    type Edit = fn(&mut RunRecord);
    let cases: [(&str, bool, Edit); 26] = [
        ("another schema", true, |r| {
            r.schema = "cortex-run-record/2".into()
        }),
        ("another task family", true, |r| {
            r.task_family = "cortex-repair".into()
        }),
        ("a path-shaped envelope id", true, |r| {
            r.envelope_id = "../x".into()
        }),
        ("an envelope digest with no cl22 prefix", true, |r| {
            r.envelope_digest = format!("sha256:{}", "f".repeat(64))
        }),
        (
            "a cl22 envelope digest that is not 64 lowercase hex",
            true,
            |r| r.envelope_digest = format!("cl22:{}", "F".repeat(64)),
        ),
        ("a null prev after seq 0", true, |r| r.seq = 5),
        ("a terminal field on a started record", true, |r| {
            r.outcome = Some("repaired".into())
        }),
        ("a started enforcement that is not an array", true, |r| {
            r.enforcement = Some(json!({"E1": "admit"}))
        }),
        ("a started record with no pinned tree", true, |r| {
            r.pinned_tree_before = None
        }),
        ("a pinned tree entry with a malformed digest", true, |r| {
            r.pinned_tree_before = Some(tree(&[("src/a", 'Z')]))
        }),
        ("a started field on a terminal record", false, |r| {
            r.enforcement = Some(json!([]))
        }),
        ("an empty outcome", false, |r| {
            r.outcome = Some(String::new())
        }),
        // the accepted artifact stays: with the exit code read as 0 the
        // record would be valid, so only the exit-code refusal stops it
        ("no exit code", false, |r| r.exit_code = None),
        (
            "exit 0 with both an accepted artifact and a refusal",
            false,
            |r| r.accepted_refusal = Some("settle refused".into()),
        ),
        (
            "exit 0 with neither an accepted artifact nor a refusal",
            false,
            |r| r.accepted = None,
        ),
        ("an accepted artifact with a malformed digest", false, |r| {
            if let Some(a) = &mut r.accepted {
                a.sha256 = "x".into();
            }
        }),
        ("an accepted artifact with an empty path", false, |r| {
            if let Some(a) = &mut r.accepted {
                a.path = String::new();
            }
        }),
        ("no write journal", false, |r| r.writes = None),
        ("a write with an empty site", false, |r| {
            if let Some(w) = &mut r.writes {
                w[0].site = String::new();
            }
        }),
        ("a write with an empty path", false, |r| {
            if let Some(w) = &mut r.writes {
                w[0].path = String::new();
            }
        }),
        ("a write with a malformed before digest", false, |r| {
            if let Some(w) = &mut r.writes {
                w[0].before_sha256 = Some("Z".repeat(64));
            }
        }),
        ("a write with a malformed after digest", false, |r| {
            if let Some(w) = &mut r.writes {
                w[0].after_sha256 = Some("Z".repeat(64));
            }
        }),
        ("a write with neither digest", false, |r| {
            if let Some(w) = &mut r.writes {
                w[0].before_sha256 = None;
                w[0].after_sha256 = None;
            }
        }),
        ("no pinned tree after", false, |r| {
            r.pinned_tree_after = None
        }),
        ("an after-tree entry with an empty path", false, |r| {
            r.pinned_tree_after = Some([(String::new(), "2".repeat(64))].into())
        }),
        (
            "an exit-1 record carrying an accepted artifact",
            false,
            |r| r.exit_code = Some(1),
        ),
    ];
    for (what, on_started, edit) in cases {
        let mut r = if on_started { s.clone() } else { t.clone() };
        edit(&mut r);
        assert!(
            validate_record(&r).is_err(),
            "ATTACK: a record with {what} validated"
        );
    }
    // exit 27 (accepted by the check, then refused by settle) carries
    // exactly one of the two, like exit 0.
    let mut r = t.clone();
    r.exit_code = Some(27);
    validate_record(&r).unwrap_or_else(|e| {
        panic!("ATTACK: an exit-27 record with its accepted artifact was refused: {e}")
    });
}

#[test]
fn malformed_seals_are_refused_by_name() {
    let (s, _) = clean_records();
    let v = serde_json::to_value(&s).unwrap();
    let mut t = v.clone();
    t["seal"] = json!(7);
    let e = key().check_seal(&t).unwrap_err();
    assert!(
        e.contains("not a string"),
        "ATTACK: a non-string seal was reported as: {e}"
    );
    let mut t = v.clone();
    t.as_object_mut().unwrap().remove("seal");
    let e = key().check_seal(&t).unwrap_err();
    assert!(
        e.contains("no `seal`"),
        "ATTACK: a missing seal was reported as: {e}"
    );
    // The same tag in uppercase hex: one seal has one spelling.
    let mut t = v.clone();
    let upper = s.seal.strip_prefix(SEAL_PREFIX).unwrap().to_uppercase();
    t["seal"] = json!(format!("{SEAL_PREFIX}{upper}"));
    let e = key()
        .check_seal(&t)
        .expect_err("ATTACK: an uppercase-hex seal verified");
    assert!(
        e.contains("64 lowercase hex"),
        "ATTACK: a malformed seal was refused only by the MAC compare: {e}"
    );
    key()
        .check_seal(&v)
        .expect("control: the record's own seal");
}

#[test]
fn a_run_record_key_that_is_not_lower_hex_is_refused() {
    let k = key().to_hex();
    assert_eq!(SealKey::from_hex(&k).unwrap(), key());
    assert!(
        SealKey::from_hex(&k.to_uppercase()).is_err(),
        "ATTACK: an uppercase-hex run-record key loaded"
    );
}

#[test]
fn a_head_with_a_malformed_record_digest_is_refused() {
    let mut h = json!({
        "schema": HEAD_SCHEMA,
        "ledger_id": "00112233445566778899aabbccddeeff",
        "seq": 0,
        "record_digest": "cl22:abc",
    });
    h["seal"] = json!(key().seal(&h).unwrap());
    assert!(
        parse_head(&h.to_string(), &key()).is_err(),
        "ATTACK: a sealed head with a malformed record digest was read"
    );
}

#[test]
fn a_ledger_id_that_is_not_32_hex_is_refused() {
    let fx = Fx::new("badid");
    std::fs::write(fx.ledger.join(LEDGER_ID_FILE), "../other\n").unwrap();
    assert!(
        ledger_id(&fx.ledger, me()).is_err(),
        "ATTACK: a path-shaped ledger id was read"
    );
}

#[test]
fn a_head_naming_another_ledger_refuses_the_next_append() {
    let fx = Fx::new("headid");
    let v = clean_run(&fx);
    let p = fx.ledger.join(HEAD_FILE);
    let mut h: Value = serde_json::from_str(&std::fs::read_to_string(&p).unwrap()).unwrap();
    h.as_object_mut().unwrap().remove("seal");
    h["ledger_id"] = json!("ffeeddccbbaa99887766554433221100");
    h["seal"] = json!(key().seal(&h).unwrap());
    std::fs::write(&p, format!("{h}\n")).unwrap();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: an append under a head copied from another ledger was accepted");
    assert!(e.contains("names ledger"), "{e}");
}

#[test]
fn a_path_shaped_id_is_never_marked_unrecorded() {
    let fx = Fx::new("markid");
    assert!(
        mark_unrecorded(&fx.spent, me(), "../escape", "x", NOW).is_err(),
        "ATTACK: a path-shaped envelope id was marked unrecorded"
    );
    assert!(!fx
        .spent
        .parent()
        .unwrap()
        .join("escape.unrecorded")
        .exists());
}

#[test]
fn an_envelope_for_another_ledger_is_never_recorded_here() {
    let fx = Fx::new("otherledger");
    let mut v = fx.run();
    v.envelope.run_ledger_id = "ffeeddccbbaa99887766554433221100".into();
    let e = fx
        .append(&v, started(Tree::new()))
        .expect_err("ATTACK: a run was recorded into a ledger its envelope does not name");
    assert!(e.contains("records into"), "{e}");
    assert!(fx.lines().is_empty());
}

#[test]
fn the_run_ledger_constants_are_the_documented_ones() {
    // Literal pins (R6 §3, §4): a consistent edit of a constant is invisible
    // to every test that reads it through the const.
    assert_eq!(
        RUN_RECORD_DOMAIN, "axon-approval repair-run-record v1",
        "ATTACK: RUN_RECORD_DOMAIN changed"
    );
    assert_eq!(
        RECORD_SCHEMA, "cortex-run-record/1",
        "ATTACK: RECORD_SCHEMA changed"
    );
    assert_eq!(
        HEAD_SCHEMA, "cortex-run-ledger-head/1",
        "ATTACK: HEAD_SCHEMA changed"
    );
    assert_eq!(SEAL_PREFIX, "axrun1:", "ATTACK: SEAL_PREFIX changed");
    assert_eq!(
        TASK_FAMILY, "cortex-repair-unattended",
        "ATTACK: TASK_FAMILY changed"
    );
    assert_eq!(
        [LEDGER_ID_FILE, LEDGER_FILE, HEAD_FILE, HEAD_TMP],
        [
            "ledger-id",
            "ledger.jsonl",
            "ledger.head",
            "ledger.head.tmp"
        ],
        "ATTACK: a run ledger file name changed"
    );
    assert_eq!(ACCEPTED_EXITS, [0, 27], "ATTACK: ACCEPTED_EXITS changed");
    assert_eq!(
        (RUN_LEDGER.dir, RUN_LEDGER_FILE.dir, RUN_RECORD_KEY.dir),
        (true, false, false),
        "ATTACK: a run ledger private-path kind changed"
    );
    assert_eq!(
        (RUN_LEDGER.what, RUN_LEDGER_FILE.what, RUN_RECORD_KEY.what),
        ("run ledger", "run ledger file", "run-record key"),
        "ATTACK: a run ledger private-path label changed"
    );
}
