//! Amendment 64 (C9 round 4b, integrate-C): every refusal site of the ledger
//! (`ledger.rs`), each judged on the production route: a store tampered on
//! disk, then opened by the loop operation a caller runs (`pointer::load`,
//! `pointer::revocations`; every verb begins with the same `Tx::begin`).
//!
//! Each test is an ATTACK — one tampering that exactly ONE check refuses, so
//! with that check removed the tampered store is served — with its CONTROL
//! (the untampered store opens). Where two checks refuse the same tampering
//! (a four-cell retirement), the test accepts either reason; only the
//! tampered store being served is the failure.

mod common;
use axon_loop::error::LoopError;
use axon_loop::ledger::{Entry, Event, Head};
use axon_loop::pointer::{self, PointerFile, Revocation};
use axon_loop::store::LedgerKey;
use axon_loop::Store;
use axon_loop_contracts::*;
use common::*;
use std::path::Path;

const DEPENDENT: &[&str] = &[
    "plans",
    "admissions",
    "evaluations",
    "baselines",
    "scopes",
    "episodes",
    "contexts",
    "candidate-sets",
    "task-manifests",
];

fn ledger(root: &Path) -> Vec<Entry> {
    read_ledger(root)
}

/// Re-chain every entry from `from` on (what a writer without the key, or of
/// an unkeyed store, can do).
fn rechain(l: &mut [Entry], from: usize) {
    for i in from.max(1)..l.len() {
        l[i].prev = digest(&l[i - 1]).unwrap();
    }
}

fn head(root: &Path, l: &[Entry], mac: Option<String>) {
    let h = Head {
        schema: Default::default(),
        seq: l.len() as u64,
        entry_ref: digest(l.last().unwrap()).unwrap(),
        mac,
    };
    std::fs::write(
        root.join("ledger.head"),
        serde_json::to_vec_pretty(&h).unwrap(),
    )
    .unwrap();
}

fn read_head(root: &Path) -> Head {
    serde_json::from_slice(&std::fs::read(root.join("ledger.head")).unwrap()).unwrap()
}

fn anchor(root: &Path, first: &Entry) {
    std::fs::write(
        root.join("ledger.anchor"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "schema": "axon.loop.ledger-anchor/1",
            "first_entry_ref": digest(first).unwrap(),
        }))
        .unwrap(),
    )
    .unwrap();
}

/// Rewrite `pointer.json` as the projection of the (edited) ledger, so the
/// projection check sees a consistent store and only the check under test
/// can refuse.
fn reproject(s: &Store, l: &[Entry]) {
    let (i, res) = l
        .iter()
        .enumerate()
        .rev()
        .find_map(|(i, e)| match &e.event {
            Event::Transition { result, .. } => Some((i, (**result).clone())),
            _ => None,
        })
        .unwrap();
    std::fs::write(
        s.scope_dir(&scope()).join("pointer.json"),
        serde_json::to_vec_pretty(&PointerFile {
            pointer: res,
            ledger_seq: l[i].seq,
            entry_ref: digest(&l[i]).unwrap(),
        })
        .unwrap(),
    )
    .unwrap();
}

fn revocation_event(w: &World) -> Event {
    Event::Revocation {
        scope: scope(),
        revocation: Revocation {
            policy_ref: w.inc_ref.clone(),
            reason_ref: r('e'),
            issuer_ref: OpaqueRef::new(ADMITTER).unwrap(),
            revoked_ms: 1,
        },
    }
}

fn chained(after: &Entry, event: Event) -> Entry {
    Entry {
        schema: Default::default(),
        seq: after.seq + 1,
        prev: digest(after).unwrap(),
        recorded_ms: after.recorded_ms + 1,
        event,
        mac: None,
    }
}

/// The store is refused as corrupt for `why`; ATTACK if it was served.
fn refused<T: std::fmt::Debug>(r: Result<T, LoopError>, why: &[&str], attack: &str) {
    match r {
        Ok(v) => panic!("ATTACK: {attack}: the store was served ({v:?})"),
        Err(e) => {
            assert_eq!(e.exit_code(), 2, "{attack}: {e}");
            assert!(
                why.iter().any(|y| e.to_string().contains(y)),
                "{attack}: expected one of {why:?}: {e}"
            );
        }
    }
}

fn load(s: &Store) -> Result<pointer::PointerRecord, LoopError> {
    pointer::load(s, &scope())
}

fn keyed(root: &Path, key: Option<LedgerKey>) -> Store {
    Store::open_dir_keyed(root, key).unwrap()
}

// ── keyed mode: entry and head MACs ────────────────────────────────────────

/// M1300: a line appended by a writer without the key, head left alone, is
/// not rolled forward: only the ENTRY's MAC can refuse it (the head is then
/// re-written, and authenticated, by the roll-forward itself).
#[test]
fn an_unauthenticated_line_is_never_rolled_forward_under_a_key() {
    let w = world_keyed(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    load(&keyed(&root, Some(fixture_key()))).expect("control: the genuine keyed store opens");
    let mut l = ledger(&root);
    let last = l.last().unwrap().clone();
    l.push(chained(&last, revocation_event(&w)));
    write_ledger(&root, &l);
    refused(
        pointer::revocations(&keyed(&root, Some(fixture_key())), &scope()),
        &["is not authenticated under the operator key"],
        "an unauthenticated ledger line was rolled forward under the operator key",
    );
}

/// M1301: a keyed ledger opened WITHOUT the key, its head's MAC stripped: only
/// the entries' MACs still say the ledger is keyed.
#[test]
fn a_keyed_ledger_is_never_read_without_its_key() {
    let w = world_keyed(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let mut h = read_head(&root);
    load(&keyed(&root, Some(fixture_key()))).expect("control: opened with its key");
    h.mac = None;
    std::fs::write(
        root.join("ledger.head"),
        serde_json::to_vec_pretty(&h).unwrap(),
    )
    .unwrap();
    refused(
        load(&keyed(&root, None)),
        &["ledger is keyed but no key is configured"],
        "a keyed ledger (head MAC stripped) was read without its key",
    );
}

/// M1302: truncation plus a head rewritten to the new last entry (R2): every
/// remaining entry is genuinely authenticated, so only the head's MAC (the
/// authenticated COUNT) can refuse it.
#[test]
fn a_truncated_keyed_ledger_with_a_rewritten_head_is_refused() {
    let w = world_keyed(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    let dropped = l.pop().unwrap();
    assert!(
        matches!(dropped.event, Event::Hypothesis { .. }),
        "setup: the last entry is the EVO proposal, not a projected transition"
    );
    write_ledger(&root, &l);
    let old_mac = read_head(&root).mac;
    head(&root, &l, old_mac);
    refused(
        load(&keyed(&root, Some(fixture_key()))),
        &["ledger head is not authenticated under the operator key"],
        "a keyed ledger truncated under a rewritten head was served",
    );
}

/// M1303: a keyed ledger whose ENTRIES were stripped of their MACs (re-chained,
/// the anchor and the projection rewritten to match) but whose head still
/// carries one, opened without the key: only the head says it was keyed.
#[test]
fn a_keyed_head_is_never_read_without_its_key() {
    let w = world_keyed(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    for e in l.iter_mut() {
        e.mac = None;
    }
    rechain(&mut l, 1);
    write_ledger(&root, &l);
    anchor(&root, &l[0]);
    reproject(&w.s, &l);
    head(&root, &l, None);
    load(&keyed(&root, None)).expect("control: the fully stripped store reads unkeyed (R-out)");
    let keyed_mac = Some("00".repeat(32));
    head(&root, &l, keyed_mac);
    refused(
        load(&keyed(&root, None)),
        &["ledger head is keyed but no key is configured"],
        "a keyed ledger head was read without its key",
    );
}

// ── the chain ──────────────────────────────────────────────────────────────

/// M1304: an entry renumbered in place (chain, head and projection rewritten to
/// match): only the seq check refuses.
#[test]
fn a_renumbered_ledger_entry_is_refused() {
    let w = world();
    let root = w.s.root().to_path_buf();
    load(&w.s).expect("control");
    let mut l = ledger(&root);
    assert!(matches!(l[1].event, Event::TaskManifest { .. }), "setup");
    l[1].seq = 9;
    rechain(&mut l, 2);
    write_ledger(&root, &l);
    head(&root, &l, None);
    reproject(&w.s, &l);
    refused(
        load(&w.s),
        &["ledger seq 9 at line 2"],
        "a ledger entry whose seq is not its position was served",
    );
}

/// M1305: an entry edited in place, nothing re-chained: the head still names
/// the (unchanged) last entry, the projection is untouched, so only the chain
/// check refuses.
#[test]
fn an_edited_ledger_entry_breaks_the_chain() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    l[1].recorded_ms += 1;
    write_ledger(&root, &l);
    refused(
        load(&w.s),
        &["ledger chain broken at seq 3"],
        "a ledger edited in place (chain broken) was served",
    );
}

/// M1306: the ledger truncated with its head left alone. With the check gone
/// the head names an entry that is not there; the store must be REFUSED as
/// corrupt (exit 2), never crash the operation or serve the prefix.
#[test]
fn a_ledger_truncated_below_its_head_is_refused_as_corrupt() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    l.pop();
    write_ledger(&root, &l);
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| load(&w.s)));
    match r {
        Err(_) => panic!(
            "ATTACK: a ledger truncated below its head was not refused as store corruption \
             (the operation crashed)"
        ),
        Ok(r) => refused(
            r,
            &["ledger truncated: head at seq 5, ledger has 4"],
            "a ledger truncated below its head was served",
        ),
    }
}

/// M1307: the LAST entry replaced by another well-chained one, head left
/// alone: only the head's entry_ref names the difference.
#[test]
fn a_replaced_last_entry_does_not_match_its_head() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    l.last_mut().unwrap().recorded_ms += 1;
    write_ledger(&root, &l);
    refused(
        load(&w.s),
        &["ledger head does not match its entry"],
        "a ledger whose last entry is not the one its head acknowledges was served",
    );
}

/// M1308: two lines appended past the head (a crash leaves at most one): only
/// this check refuses; with it gone the two unacknowledged revocations are
/// served.
#[test]
fn two_unacknowledged_lines_are_never_served() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    let a = chained(l.last().unwrap(), revocation_event(&w));
    let b = chained(&a, revocation_event(&w));
    l.push(a);
    l.push(b);
    write_ledger(&root, &l);
    refused(
        pointer::revocations(&w.s, &scope()),
        &["ledger more than one entry ahead of its head"],
        "two ledger lines past the head were served",
    );
}

/// M1309 (retired EQUIVALENT, set with M1310 and M1311): the head deleted and
/// the revocation behind it dropped. The head-missing arm, the anchor and the
/// dependent directories each refuse it; with all three removed the revoked
/// policy is served again.
#[test]
fn a_ledger_truncated_with_its_head_deleted_is_refused() {
    let w = world();
    let root = w.s.root().to_path_buf();
    pointer::revoke(
        &w.s,
        &scope(),
        &w.inc_ref,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    assert!(
        pointer::resolve(&w.s, &scope()).is_err(),
        "control: revoked"
    );
    let mut l = ledger(&root);
    assert!(matches!(l.pop().unwrap().event, Event::Revocation { .. }));
    write_ledger(&root, &l);
    std::fs::remove_file(root.join("ledger.head")).unwrap();
    match pointer::resolve(&w.s, &scope()) {
        Ok(r) => panic!(
            "ATTACK: a ledger truncated with its head deleted served a revoked policy ({:?})",
            r.admission_ref
        ),
        Err(e) => {
            assert_eq!(e.exit_code(), 2, "{e}");
            assert!(
                [
                    "ledger head missing",
                    "ledger.anchor",
                    "a fence is never reissued"
                ]
                .iter()
                .any(|y| e.to_string().contains(y)),
                "{e}"
            );
        }
    }
}

fn delete_history(root: &Path, dirs: &[&str]) {
    std::fs::remove_file(root.join("ledger.jsonl")).unwrap();
    std::fs::remove_file(root.join("ledger.head")).unwrap();
    for d in dirs {
        let _ = std::fs::remove_dir_all(root.join(d));
    }
}

/// M1310 (retired EQUIVALENT, pair with M1312): ledger, head and every
/// dependent directory deleted, the anchor kept: the anchor's presence and its
/// first-entry binding each refuse it; with both gone epoch 1 is reissued.
#[test]
fn a_deleted_history_is_refused_while_its_anchor_remains() {
    let w = world();
    let root = w.s.root().to_path_buf();
    delete_history(&root, DEPENDENT);
    refused(
        load(&w.s),
        &["ledger.anchor"],
        "a store whose ledger was deleted (anchor kept) reissued its history",
    );
}

/// M1311: ledger, head and the anchor deleted, one dependent directory kept
/// (the projection's `scopes/` gone, so no projection check speaks): only the
/// dependent-directory rule refuses.
#[test]
fn a_deleted_history_is_refused_while_a_dependent_directory_remains() {
    let w = world();
    let root = w.s.root().to_path_buf();
    std::fs::remove_file(root.join("ledger.anchor")).unwrap();
    delete_history(&root, &["scopes"]);
    assert!(root.join("baselines").exists(), "setup");
    refused(
        load(&w.s),
        &["ledger missing but"],
        "a store whose ledger, head and anchor were deleted reissued its history",
    );
}

/// M1312: the ledger replaced by another, fully consistent one (first entry
/// edited, re-chained, head and projection rewritten): only the anchor's
/// first-entry binding refuses.
#[test]
fn a_replaced_ledger_does_not_match_its_anchor() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let mut l = ledger(&root);
    l[0].recorded_ms += 1;
    rechain(&mut l, 1);
    write_ledger(&root, &l);
    head(&root, &l, None);
    reproject(&w.s, &l);
    refused(
        load(&w.s),
        &["ledger.anchor names a different first entry"],
        "a ledger replaced by another consistent ledger was served",
    );
}

// ── the projection ─────────────────────────────────────────────────────────

/// M1313: a scope (tenant) directory replaced by a symlink to an empty
/// directory outside the store. Nothing is read through it, so only the
/// projection walk's own symlink rule refuses.
#[test]
fn a_symlinked_tenant_directory_is_refused() {
    let w = world();
    let elsewhere = tempfile::tempdir().unwrap();
    let link = w.s.root().join("scopes").join("other-tenant");
    std::os::unix::fs::symlink(elsewhere.path(), &link).unwrap();
    refused(
        load(&w.s),
        &["store corrupt: symlink"],
        "a tenant directory that is a symlink was accepted in the projection",
    );
}

/// M1314 (retired EQUIVALENT, pair with M979): the scope's family directory
/// replaced by a symlink to an identical copy: the projection walk and the
/// store's path guard (reading pointer.json through it) each refuse it.
#[test]
fn a_symlinked_family_directory_is_refused() {
    let w = world();
    let elsewhere = tempfile::tempdir().unwrap();
    let fam = w.s.scope_dir(&scope());
    let copy = elsewhere.path().join("fam");
    std::fs::create_dir(&copy).unwrap();
    std::fs::copy(fam.join("pointer.json"), copy.join("pointer.json")).unwrap();
    std::fs::remove_dir_all(&fam).unwrap();
    std::os::unix::fs::symlink(&copy, &fam).unwrap();
    refused(
        load(&w.s),
        &["symlink"],
        "a scope family directory that is a symlink was read as the projection",
    );
}

/// M1315: a pointer.json for a scope the ledger never transitioned.
#[test]
fn a_projection_with_no_transition_is_refused() {
    let w = world();
    let other =
        w.s.root()
            .join("scopes")
            .join("other-tenant")
            .join("other-family");
    std::fs::create_dir_all(&other).unwrap();
    std::fs::copy(
        w.s.scope_dir(&scope()).join("pointer.json"),
        other.join("pointer.json"),
    )
    .unwrap();
    refused(
        load(&w.s),
        &["has no transition in the ledger"],
        "a pointer.json no ledger transition projects was accepted",
    );
}

/// M1316: the scope's pointer.json deleted.
#[test]
fn a_missing_projection_is_refused() {
    let w = world();
    std::fs::remove_file(w.s.scope_dir(&scope()).join("pointer.json")).unwrap();
    refused(
        load(&w.s),
        &["is missing"],
        "a scope whose pointer.json was deleted was served",
    );
}

/// M1317: the scope's pointer.json edited (its seq).
#[test]
fn an_edited_projection_is_refused() {
    let w = world();
    let p = w.s.scope_dir(&scope()).join("pointer.json");
    let mut f: PointerFile = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    f.ledger_seq += 1;
    std::fs::write(&p, serde_json::to_vec_pretty(&f).unwrap()).unwrap();
    refused(
        load(&w.s),
        &["is not the ledger's projection"],
        "an edited pointer.json was accepted as the ledger's projection",
    );
}
