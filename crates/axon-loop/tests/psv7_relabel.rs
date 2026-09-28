//! PSV-7 (dev review round wf_336353cb-a2b, executed there to ACCEPT and
//! activation): development-class verdicts relabelled as a PROTECTED
//! evaluation in the mutable store. The record, the ledger (unkeyed by
//! default) and the CAS are all writable by a store writer, so a protected
//! decision re-verifies every counted verdict from its own documents under the
//! operator roots, and a relabelled development verdict does not re-verify.
//! Adapted from the reviewer's reproduction; the forged ledger append is
//! theirs.
mod common;
use axon_loop::admission::Decision;
use axon_loop::ledger::Event;
use axon_loop::pointer;
use common::*;
use serde_json::json;

#[test]
fn development_verdicts_relabelled_protected_are_refused() {
    // Control: the same development evaluation, in its own (unprotected)
    // class, is accepted: the fixture decides, not a defect in it.
    let w = world();
    freeze_plan(&w.s, "c", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    assign_specs(&w.s, "c", &specs);
    let v = evl_request("c", &w.inc, &w.cand, &specs, &EvlOpts::default());
    clear_all(&w.s, &v);
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (adm, _) = admit(&w.s, "c", &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);

    let w = world();
    freeze_plan(&w.s, "x", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    assign_specs(&w.s, "x", &specs);
    let v = evl_request("x", &w.inc, &w.cand, &specs, &EvlOpts::default());
    clear_all(&w.s, &v);
    let (rec, _e) = evaluate(&w.s, &v).unwrap();
    assert_eq!(
        rec.evaluation_class,
        axon_loop::plan::EvaluationClass::Development
    );
    let mut cfg = w.s.config().unwrap();
    cfg.protected_scopes.push(scope());
    w.s.write_config(&cfg).unwrap();
    // A store writer relabels the record PROTECTED; key ids are public.
    let obs_kid = axon_loop_contracts::attestation::key_id_of_hex(&observer_key().1).unwrap();
    let mut j = serde_json::to_value(&rec).unwrap();
    j["evaluation_class"] = json!("protected");
    for arm in j["arms"].as_array_mut().unwrap() {
        for t in arm["trials"].as_array_mut().unwrap() {
            if t["verification"].is_object() {
                t["context_signed_by"] = json!({"issuer_ref": OBSERVER, "key_id": obs_kid});
            }
        }
    }
    let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(j).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    let root = w.s.root().to_path_buf();
    forged_append(
        &root,
        Event::Evaluation {
            scope: scope(),
            experiment_id: "x".into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
    );
    match admit(&w.s, "x", &fe, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains("does not re-verify"), "{e}"),
        Ok((adm, adm_ref)) => {
            assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
            let t = transition(
                "a1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm_ref),
                false,
            );
            assert!(pointer::transition(&w.s, &tparse(&t)).is_err());
        }
    }
}
