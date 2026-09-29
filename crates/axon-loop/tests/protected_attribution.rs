//! O2 for the RECORDED attribution of a protected decision. A counted
//! protected trial records WHO authenticated its verdict (`verification`
//! issuer + key id) and its preflight context (`context_signed_by`), so every
//! re-derivation can require that authority to be current. Admission also
//! re-verifies the documents themselves (`reverify_protected`), but that
//! re-verification never compares its signer with the recorded attribution:
//! the only thing standing between a store writer and an ACCEPTed record that
//! names, as its authority, an identity the operator root never held, or none
//! at all, is the attribution check in `admission::derive`. Each test forges
//! exactly one attribution on an otherwise GENUINE protected evaluation
//! (every document still verifies under the real, operator-rooted signer),
//! with an in-world positive control: the unforged record ACCEPTs.
//!
//! C9 equivalence re-audit: these are the killing tests for M104, M209 and
//! M210, which had been retired as equivalent to `reverify_protected`.

mod common;
use axon_loop::admission::Decision;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

const PROTECTED: &str = "linux-microvm-protected";

fn protect(s: &axon_loop::store::Store) {
    let mut cfg = s.config().unwrap();
    cfg.protected_scopes.push(scope());
    s.write_config(&cfg).unwrap();
}

fn pin_protected_backend(s: &axon_loop::store::Store) {
    let mut cfg = s.config().unwrap();
    cfg.verifier_pins
        .get_mut(&OpaqueRef::new(VERIFIER).unwrap())
        .unwrap()
        .backend_profiles
        .push(PROTECTED.into());
    s.write_config(&cfg).unwrap();
}

/// Every delivered trial genuinely on the protected backend: an attested
/// execution leg and a protected verdict with its PSV bundle.
fn on_protected_backend(v: &mut Value) {
    for t in v["trials"].as_array_mut().unwrap() {
        t["acf_receipt"]["backend_profile_ref"] = json!(PROTECTED);
        t["episode"]["acf_receipt_ref"] = json!(digest_value(&t["acf_receipt"]).unwrap());
        t["acf_attestation"] = attest_execution(VERIFIER, &t["acf_request"], &t["acf_receipt"]);
        if t["verification_receipt"].is_object() {
            t["verification_receipt"]["backend_profile_ref"] = json!(PROTECTED);
            let req = t["verification_request"].clone();
            let epoch = t["episode"]["authority_epoch"].as_u64().unwrap();
            let bundle = make_protected(
                &req,
                &mut t["verification_receipt"],
                |_| {},
                |o| o.epoch = epoch,
            );
            t["verification_psv_evidence"] = serde_json::from_str(&bundle).unwrap();
            t["episode"]["verification"]["verifier_ref"] =
                json!(digest_value(&t["verification_receipt"]).unwrap());
            t["verification_attestation"] = attest(
                VERIFIER,
                &t["verification_request"],
                &t["verification_receipt"],
            );
        }
    }
}

fn specs_for(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}

/// A GENUINE protected evaluation, honestly made by EVL and every trial
/// cleared; its unforged record ACCEPTs (the positive control).
fn genuine(exp: &str) -> (World, axon_loop::evl::EvaluationRecord) {
    let w = world();
    protect(&w.s);
    pin_protected_backend(&w.s);
    trust_monitor(&w.s);
    freeze_plan(&w.s, exp, &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, exp, &specs_for(&w));
    let mut v = evl_request(exp, &w.inc, &w.cand, &specs_for(&w), &EvlOpts::default());
    on_protected_backend(&mut v);
    clear_all(&w.s, &v);
    let (rec, e) = evaluate(&w.s, &v).unwrap();
    let (adm, _) = admit(&w.s, exp, &e, ADMITTER, false).unwrap();
    assert_eq!(adm.decision, Decision::Accept, "control: {:?}", adm.reasons);
    (w, rec)
}

/// A store writer edits each COUNTED trial of the genuine record, puts it in
/// the CAS and journals it with a forged ledger append.
fn forge_counted(
    w: &World,
    exp: &str,
    rec: &axon_loop::evl::EvaluationRecord,
    mut edit: impl FnMut(&mut Value),
) -> Ref {
    let mut j = serde_json::to_value(rec).unwrap();
    for arm in j["arms"].as_array_mut().unwrap() {
        for t in arm["trials"].as_array_mut().unwrap() {
            if t["verification"].is_object() {
                edit(t);
            }
        }
    }
    let forged: axon_loop::evl::EvaluationRecord = serde_json::from_value(j).unwrap();
    let fe = w.s.put_cas("evaluations", &forged).unwrap();
    forged_append(
        w.s.root(),
        axon_loop::ledger::Event::Evaluation {
            scope: scope(),
            experiment_id: exp.into(),
            evaluation_ref: fe.clone(),
            freeze_seq: forged.freeze_seq,
            authority_epoch: forged.authority_epoch,
        },
    );
    fe
}

fn refused(w: &World, exp: &str, fe: &Ref, why: &str) {
    match admit(&w.s, exp, fe, ADMITTER, false) {
        Err(e) => assert!(e.to_string().contains(why), "{e}"),
        Ok((adm, _)) => panic!(
            "a forged attribution was admitted: {:?} {:?}",
            adm.decision, adm.reasons
        ),
    }
}

fn key_id(pk: &str) -> String {
    axon_loop_contracts::attestation::key_id_of_hex(pk).unwrap()
}

/// M104: a counted protected trial whose record carries NO context
/// attribution (the store writer deleted `context_signed_by`) does not count,
/// although its context signature still re-verifies under the real observer.
#[test]
fn a_protected_verdict_without_its_context_attribution_does_not_count() {
    let (w, rec) = genuine("attr-absent");
    let fe = forge_counted(&w, "attr-absent", &rec, |t| {
        t.as_object_mut().unwrap().remove("context_signed_by");
    });
    refused(
        &w,
        "attr-absent",
        &fe,
        "protected context is not authenticated",
    );
}

/// M210: a counted protected trial whose context is ATTRIBUTED to an observer
/// the store trusts and keys, but whose key the operator's observer root never
/// held, does not count.
#[test]
fn a_context_attributed_to_an_observer_the_operator_root_never_held_does_not_count() {
    const PLANTED: &str = "agent:planted-observer";
    let (_, pk) = axon_loop_contracts::attestation::generate().unwrap();
    let (w, rec) = genuine("attr-observer");
    let mut cfg = w.s.config().unwrap();
    let o = OpaqueRef::new(PLANTED).unwrap();
    cfg.trusted_observers.push(o.clone());
    cfg.observer_keys.insert(o, pk.clone());
    w.s.write_config(&cfg).unwrap();
    let fe = forge_counted(&w, "attr-observer", &rec, |t| {
        t["context_signed_by"] = json!({"issuer_ref": PLANTED, "key_id": key_id(&pk)});
    });
    refused(
        &w,
        "attr-observer",
        &fe,
        "protected context is not authenticated",
    );
}

/// M209: a counted protected verdict ATTRIBUTED to a verifier the store
/// trusts, keys and pins, but whose key the operator's verifier root never
/// held, does not count — although the verdict's own attestation still
/// re-verifies under the real, rooted verifier.
#[test]
fn a_verdict_attributed_to_a_verifier_the_operator_root_never_held_does_not_count() {
    const PLANTED: &str = "agent:planted-verifier";
    let (_, pk) = axon_loop_contracts::attestation::generate().unwrap();
    let (w, rec) = genuine("attr-verifier");
    let mut cfg = w.s.config().unwrap();
    let v = OpaqueRef::new(PLANTED).unwrap();
    let pin = cfg.verifier_pins[&OpaqueRef::new(VERIFIER).unwrap()].clone();
    cfg.trusted_verifiers.push(v.clone());
    cfg.verifier_keys.insert(v.clone(), pk.clone());
    cfg.verifier_pins.insert(v, pin);
    w.s.write_config(&cfg).unwrap();
    let fe = forge_counted(&w, "attr-verifier", &rec, |t| {
        t["verification"]["issuer_ref"] = json!(PLANTED);
        t["verification"]["key_id"] = json!(key_id(&pk));
    });
    refused(&w, "attr-verifier", &fe, "no longer trusts with that key");
}
