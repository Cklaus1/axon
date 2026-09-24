//! D-015 (D-C1): the ledger keyed on `axon-audit`'s primitive.
//!
//! Each attack is the red-team rt4i row of the same name, done here by a
//! writer who has the store directory but NOT the operator key. Under a key
//! every one is refused (exit 2) and the refusal changes no byte; the last
//! test pins what is still out of model (R1) so the doc cannot drift from it.
mod common;
use axon_loop::error::LoopError;
use axon_loop::ledger::{Entry, Event, Head};
use axon_loop::{pointer, Store};
use axon_loop_contracts::*;
use common::*;
use std::path::Path;

fn read_ledger(root: &Path) -> Vec<Entry> {
    std::fs::read_to_string(root.join("ledger.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect()
}

fn write_ledger(root: &Path, l: &[Entry]) {
    let mut s = String::new();
    for e in l {
        s.push_str(&serde_json::to_string(e).unwrap());
        s.push('\n');
    }
    std::fs::write(root.join("ledger.jsonl"), s).unwrap();
}

/// What a key-less forger can do: re-chain, and recompute the head's
/// digest. It can copy an old MAC but cannot compute a new one.
fn forge_head(root: &Path, l: &[Entry], mac: Option<String>) {
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

fn forged_append(root: &Path, event: Event) {
    let mut l = read_ledger(root);
    let last = l.last().unwrap().clone();
    l.push(Entry {
        schema: Default::default(),
        seq: last.seq + 1,
        prev: digest(&last).unwrap(),
        recorded_ms: last.recorded_ms + 1,
        event,
        mac: last.mac.clone(), // the best a key-less writer can do
    });
    write_ledger(root, &l);
}

/// Rewrite `pointer.json` as the projection of the (forged) ledger, as the
/// rt4i F1 row does, so only ledger authentication can catch the forgery.
fn reproject(w: &World) {
    let l = read_ledger(w.s.root());
    let (seq, res) = l
        .iter()
        .rev()
        .find_map(|e| match &e.event {
            Event::Transition { result, .. } => Some((e.seq, (**result).clone())),
            _ => None,
        })
        .unwrap();
    std::fs::write(
        w.s.scope_dir(&scope()).join("pointer.json"),
        serde_json::to_vec_pretty(&pointer::PointerFile {
            pointer: res,
            ledger_seq: seq,
            entry_ref: digest(&l[seq as usize - 1]).unwrap(),
        })
        .unwrap(),
    )
    .unwrap();
}

fn keyed_store(root: &Path) -> Store {
    Store::open_dir_keyed(root, Some(fixture_key())).unwrap()
}

fn assert_corrupt_unchanged(root: &Path, f: impl FnOnce() -> Result<(), LoopError>) {
    let before = snapshot(root);
    let e = f().expect_err("must be refused");
    assert_eq!(e.exit_code(), 2, "{e}");
    assert_eq!(snapshot(root), before, "refusal changed the store: {e}");
}

fn load(s: &Store) -> Result<(), LoopError> {
    pointer::load(s, &scope()).map(|_| ())
}

/// A forged transition to `cand` at epoch 2, well-chained.
fn forged_transition(w: &World, adm: &Ref) -> Event {
    let l = read_ledger(w.s.root());
    let (prior, tr) = l
        .iter()
        .rev()
        .find_map(|e| match &e.event {
            Event::Transition {
                result, transition, ..
            } => Some(((**result).clone(), (**transition).clone())),
            _ => None,
        })
        .unwrap();
    let mut t2 = tr.clone();
    t2.transition_id = TransitionId::new("forged").unwrap();
    t2.expected_epoch = AuthorityEpoch::new(1).unwrap();
    t2.next_epoch = AuthorityEpoch::new(2).unwrap();
    t2.expected_policy_ref = w.inc_ref.clone();
    t2.target_policy_ref = Some(w.cand_ref.clone());
    t2.admission_ref = Some(adm.clone());
    let mut res = prior.clone();
    res.epoch = AuthorityEpoch::new(2).unwrap();
    res.active_policy_ref = Some(w.cand_ref.clone());
    Event::Transition {
        transition_ref: digest(&t2).unwrap(),
        transition: Box::new(t2),
        prior: Box::new(prior),
        result: Box::new(res),
    }
}

#[test]
fn keyed_store_round_trips_through_every_verb() {
    let w = world_keyed(Some(fixture_key()));
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "act",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    let reopened = keyed_store(w.s.root());
    assert_eq!(pointer::load(&reopened, &scope()).unwrap().epoch.get(), 2);
    assert!(read_ledger(w.s.root()).iter().all(|e| e.mac.is_some()));
}

/// F1: one forged, well-chained transition + recomputed head + projection.
#[test]
fn f1_forged_append_with_recomputed_head_is_detected_under_a_key() {
    let w = world_keyed(Some(fixture_key()));
    let adm = accepted(&w, "exp");
    let root = w.s.root().to_path_buf();
    let old_head: Head =
        serde_json::from_slice(&std::fs::read(root.join("ledger.head")).unwrap()).unwrap();
    forged_append(&root, forged_transition(&w, &adm));
    reproject(&w);
    // with the head rewritten (a copied MAC, and no MAC at all) …
    forge_head(&root, &read_ledger(&root), old_head.mac.clone());
    assert_corrupt_unchanged(&root, || load(&keyed_store(&root)));
    forge_head(&root, &read_ledger(&root), None);
    assert_corrupt_unchanged(&root, || load(&keyed_store(&root)));
}

/// F1, head left alone: the forged line is one past the head, so it would be
/// ROLLED FORWARD as a crash tail (projected and re-headed). The per-entry
/// MAC is what stops that.
#[test]
fn f1_forged_line_is_not_rolled_forward_under_a_key() {
    let w = world_keyed(Some(fixture_key()));
    let adm = accepted(&w, "exp");
    let root = w.s.root().to_path_buf();
    forged_append(&root, forged_transition(&w, &adm));
    assert_corrupt_unchanged(&root, || load(&keyed_store(&root)));
}

/// F2 (rt4i row F2): a genuine REJECT-bound evaluation exists; the forger
/// hand-writes a better EvaluationRecord (content-addressed, so it names
/// itself), journals ONE `evaluation` line, rewrites the head, and then runs
/// the REAL admit verb. Unkeyed that mints an ACCEPT; keyed it is refused
/// before admission reads anything.
#[test]
fn f2_forged_evaluation_line_is_detected_before_admission() {
    let w = world_keyed(Some(fixture_key()));
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let (rec, _) = evaluate(
        &w.s,
        &evl_request(
            "exp",
            &w.inc,
            &w.cand,
            &pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50)),
            &EvlOpts::default(),
        ),
    )
    .unwrap();
    let root = w.s.root().to_path_buf();
    let mut v = serde_json::to_value(&rec).unwrap();
    for arm in v["arms"].as_array_mut().unwrap() {
        if arm["arm_id"] != "incumbent" {
            arm["verified_pass"] = arm["assigned"].clone();
            arm["fail"] = 0.into();
        }
    }
    let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(v).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    forged_append(
        &root,
        Event::Evaluation {
            scope: scope(),
            experiment_id: "exp".into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: AuthorityEpoch::new(1).unwrap(),
        },
    );
    forge_head(&root, &read_ledger(&root), None);
    let s = keyed_store(&root);
    assert_corrupt_unchanged(&root, || admit(&s, "exp", &fe, ADMITTER, false).map(|_| ()));
}

fn activated_then_revoked(key: Option<axon_loop::store::LedgerKey>) -> (World, Vec<u8>) {
    let w = world_keyed(key);
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &tparse(&transition(
            "act",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    let head_before_revoke = std::fs::read(w.s.root().join("ledger.head")).unwrap();
    pointer::revoke(
        &w.s,
        &scope(),
        &w.cand_ref,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    assert!(
        pointer::resolve(&w.s, &scope()).is_err(),
        "revoked ⇒ paused"
    );
    (w, head_before_revoke)
}

/// R2: drop the revocation line and rewrite the head to the new last entry.
#[test]
fn r2_truncate_and_rewrite_head_is_detected_under_a_key() {
    let (w, _) = activated_then_revoked(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let mut l = read_ledger(&root);
    let revoked = l.pop().unwrap();
    assert!(matches!(revoked.event, Event::Revocation { .. }));
    write_ledger(&root, &l);
    // copy the only authenticated head MAC the forger has, and try none
    let cur: Head =
        serde_json::from_slice(&std::fs::read(root.join("ledger.head")).unwrap()).unwrap();
    forge_head(&root, &l, cur.mac);
    assert_corrupt_unchanged(&root, || load(&keyed_store(&root)));
    forge_head(&root, &l, None);
    assert_corrupt_unchanged(&root, || load(&keyed_store(&root)));
}

/// The key decides, never the file: a wrong key, no key on a keyed store,
/// and a key on an unkeyed store are all refused (no silent downgrade).
#[test]
fn the_key_not_the_file_decides_how_the_ledger_is_verified() {
    let w = world_keyed(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let other = axon_loop::store::LedgerKey::derive(&[0x22; 32]).unwrap();
    assert_corrupt_unchanged(&root, || {
        load(&Store::open_dir_keyed(&root, Some(other)).unwrap())
    });
    assert_corrupt_unchanged(&root, || load(&Store::open_dir_keyed(&root, None).unwrap()));
    let u = world();
    let uroot = u.s.root().to_path_buf();
    assert_corrupt_unchanged(&uroot, || load(&keyed_store(&uroot)));
    assert!(axon_loop::store::LedgerKey::derive(&[0x33; 15]).is_err());
}

/// Through the binary: `AXON_ATTEST_KEY` is the key source, and a malformed
/// value is refused rather than silently running unkeyed.
#[test]
fn binary_reads_the_operator_key_from_axon_attest_key() {
    let bin = env!("CARGO_BIN_EXE_axon-loop");
    let key = axon_loop::store::LedgerKey::derive(
        &(0..32)
            .map(|i| u8::from_str_radix(&FIXTURE_KEY_HEX[2 * i..2 * i + 2], 16).unwrap())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    let w = world_keyed(Some(key));
    let run = |k: Option<&str>| {
        let mut c = std::process::Command::new(bin);
        c.env_remove("AXON_ATTEST_KEY");
        if let Some(k) = k {
            c.env("AXON_ATTEST_KEY", k);
        }
        c.arg("--store")
            .arg(w.s.root())
            .args(["pointer", "show", "--tenant", "fixture-tenant"])
            .args(["--family", "fixture-coding"])
            .output()
            .unwrap()
            .status
            .code()
            .unwrap()
    };
    assert_eq!(run(Some(FIXTURE_KEY_HEX)), 0);
    assert_eq!(run(None), 2, "keyed ledger, no key: refused");
    assert_eq!(run(Some("zz")), 2, "not hex: refused, not unkeyed");
    assert_eq!(run(Some("0011")), 2, "too short: refused");
}

/// R1 is OPEN and this pins it: restoring the GENUINE earlier head (the
/// whole-store snapshot restore, reduced to its essential step) is served.
/// Every byte is authentic; only freshness is wrong, and that needs a
/// monotonic witness outside the store (ledger.rs threat model). If this
/// starts failing, R1 was closed: update the doc, README and completeness row.
#[test]
fn r1_restoring_a_genuine_older_keyed_state_is_still_out_of_model() {
    let (w, head_before_revoke) = activated_then_revoked(Some(fixture_key()));
    let root = w.s.root().to_path_buf();
    let mut l = read_ledger(&root);
    l.pop();
    write_ledger(&root, &l);
    std::fs::write(root.join("ledger.head"), head_before_revoke).unwrap();
    let s = keyed_store(&root);
    assert!(
        pointer::resolve(&s, &scope()).is_ok(),
        "R1 closed? the revoked policy is no longer served after a genuine-state restore"
    );
}

/// Controls: the SAME attacks succeed on an unkeyed store, so the keyed
/// tests above refuse a real forgery and not a malformed one.
#[test]
fn controls_the_same_attacks_succeed_unkeyed() {
    // F1
    let w = world();
    let adm = accepted(&w, "exp");
    let root = w.s.root().to_path_buf();
    let ev = forged_transition(&w, &adm);
    forged_append(&root, ev);
    forge_head(&root, &read_ledger(&root), None);
    reproject(&w);
    let p = pointer::load(&Store::open_dir_keyed(&root, None).unwrap(), &scope()).unwrap();
    assert_eq!(p.active_policy_ref, Some(w.cand_ref.clone()), "F1 control");
    // R2
    let (w, _) = activated_then_revoked(None);
    let root = w.s.root().to_path_buf();
    let mut l = read_ledger(&root);
    l.pop();
    write_ledger(&root, &l);
    forge_head(&root, &l, None);
    let s = Store::open_dir_keyed(&root, None).unwrap();
    assert!(pointer::resolve(&s, &scope()).is_ok(), "R2 control");
}
