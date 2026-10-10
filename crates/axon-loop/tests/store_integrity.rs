//! Amendment 61 (C9 round 4b, rows4a): the store's own refusals, judged on
//! the production routes that read and write through it (`evl::evaluate`,
//! `admit`). Each test is an ATTACK by a store writer (someone who can edit
//! files under the store root, the loop's stated adversary) with its CONTROL.

mod common;
use axon_loop::admission::Decision;
use common::*;
use serde_json::{json, Value};

/// A JOURNALLED evaluation's record edited in place (its file keeps its
/// `cl22:` name): the challenger's two failures rewritten as the incumbent's
/// passes. Reading it is refused (its content no longer digests to its name),
/// so the admission never decides on the edited counts. Control: the
/// unedited record is admitted, and does not ACCEPT a challenger that failed.
#[test]
fn a_journalled_evaluation_edited_in_place_is_never_admitted() {
    for edited in [false, true] {
        let w = world();
        freeze_plan(&w.s, "ed", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
        let v = evl_request("ed", &w.inc, &w.cand, &specs, &EvlOpts::default());
        let (_, e) = evaluate(&w.s, &v).unwrap();
        if edited {
            let path =
                w.s.root()
                    .join("evaluations")
                    .join(format!("{}.json", e.hex()));
            let mut rec: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let arms = rec["arms"].as_array_mut().unwrap();
            let inc = arms
                .iter()
                .find(|a| a["arm_id"] == "incumbent")
                .unwrap()
                .clone();
            for a in arms.iter_mut().filter(|a| a["arm_id"] == "challenger-1") {
                for (k, n) in [("verified_pass", 2), ("fail", 0)] {
                    a[k] = json!(n);
                }
                let mine = a["trials"].clone();
                let mut trials = inc["trials"].clone();
                for (t, m) in trials
                    .as_array_mut()
                    .unwrap()
                    .iter_mut()
                    .zip(mine.as_array().unwrap())
                {
                    for k in ["task_id", "trial_id", "episode_ref"] {
                        t[k] = m[k].clone();
                    }
                }
                a["trials"] = trials;
            }
            std::fs::write(&path, axon_loop_contracts::canonical_json(&rec).unwrap()).unwrap();
        }
        match admit(&w.s, "ed", &e, ADMITTER, false) {
            Ok((adm, _)) if edited && adm.decision == Decision::Accept => panic!(
                "ATTACK: an evaluation record edited in the store was admitted: {:?}",
                adm.reasons
            ),
            Ok((adm, _)) => assert!(
                !edited && adm.decision != Decision::Accept,
                "{edited}: {:?} {:?}",
                adm.decision,
                adm.reasons
            ),
            Err(err) => assert!(
                edited && err.to_string().contains("is corrupt"),
                "{edited}: {err}"
            ),
        }
    }
}

/// A store directory replaced by a symlink to a directory outside the store:
/// nothing is written through it. Three checks refuse it, each alone (the
/// path guard, and `ensure_dir`'s per-component symlink and directory
/// re-checks; four-cell retirements),
/// so any refusal is accepted: only a byte landing outside the store is the
/// attack. Control: without the symlink the evaluation is stored.
#[test]
fn a_store_directory_replaced_by_a_symlink_is_never_written_through() {
    for planted in [false, true] {
        let w = world();
        let outside = tempfile::tempdir().unwrap();
        freeze_plan(&w.s, "sl", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let v = evl_request("sl", &w.inc, &w.cand, &specs, &EvlOpts::default());
        assert!(intake_all(&w.s, &v).is_empty());
        let dir = w.s.root().join("evaluations");
        if planted {
            let _ = std::fs::remove_dir_all(&dir);
            std::os::unix::fs::symlink(outside.path(), &dir).unwrap();
        }
        let r = axon_loop::evl::evaluate(
            &w.s,
            &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
        );
        let leaked = std::fs::read_dir(outside.path()).unwrap().count();
        if !planted {
            r.expect("control: the evaluation is stored");
            continue;
        }
        if leaked != 0 || r.is_ok() {
            panic!(
                "ATTACK: a store write followed a symlinked directory out of the store \
                 ({leaked} file(s) outside): {:?}",
                r.map(|(_, e)| e)
            );
        }
        // Three lstat-based checks refuse it, each alone (guard, ensure_dir's
        // symlink re-check, and its is-a-directory check): which one is not
        // the property.
        let e = r.unwrap_err().to_string();
        assert!(
            e.contains("symlink") || e.contains("is not a directory"),
            "{e}"
        );
    }
}
