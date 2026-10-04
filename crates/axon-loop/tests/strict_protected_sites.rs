//! Amendment 64 (C9 round 4b, integrate-E): the store's digest re-check of a
//! stored document's exact TEXT (`Store::get_cas_text`), judged on the route
//! that reads it: a PROTECTED admission re-verifying its counted trials from
//! the stored verification documents. Its exemption rested on the reader
//! authenticating the text downstream; under the strict ruling it is a
//! four-cell pair with that authentication (the attestation signature, M02).

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

/// Every delivered trial executed and verified on the protected backend (as
/// `protected_class.rs` builds it: an observed, attested execution; a
/// verdict joined through its PSV bundle and re-attested).
fn on_protected_backend(v: &mut Value) {
    for t in v["trials"].as_array_mut().unwrap() {
        observed_protected_execution(t);
        if t["verification_receipt"].is_object() {
            t["verification_receipt"]["backend_profile_ref"] = json!(PROTECTED);
            let req = t["verification_request"].clone();
            let epoch = t["episode"]["authority_epoch"].as_u64().unwrap();
            let bundle = make_protected(
                &req,
                &mut t["verification_receipt"],
                |m| m.authority.epoch = epoch,
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

/// Four-cell pair (get_cas_text's digest re-check; the verification
/// attestation's signature check, M02): a protected evaluation whose stored
/// verification attestations are EDITED in place after it was recorded (each
/// signature's first byte changed) is never admitted on them: the admission
/// re-reads each by its name, and a text that no longer digests to its name
/// is refused. Control: the unedited store ACCEPTs.
#[test]
fn a_stored_attestation_edited_in_place_is_never_admitted_on() {
    for edited in [false, true] {
        let w = world();
        protect(&w.s);
        pin_protected_backend(&w.s);
        freeze_plan(&w.s, "ea", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        assign_specs(&w.s, "ea", &specs);
        let mut v = evl_request("ea", &w.inc, &w.cand, &specs, &EvlOpts::default());
        on_protected_backend(&mut v);
        clear_all(&w.s, &v);
        let (_, e) = evaluate(&w.s, &v).unwrap();
        if edited {
            let dir = w.s.root().join("fabric-attestations");
            let mut n = 0;
            for f in std::fs::read_dir(&dir).unwrap() {
                let p = f.unwrap().path();
                let mut a: Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
                let sig = a["signature"].as_str().unwrap().to_string();
                let flipped = if sig.starts_with('0') { "1" } else { "0" };
                a["signature"] = json!(format!("{flipped}{}", &sig[1..]));
                std::fs::write(&p, canonical_json(&a).unwrap()).unwrap();
                n += 1;
            }
            assert!(n > 0, "the protected evaluation stored its attestations");
        }
        match admit(&w.s, "ea", &e, ADMITTER, false) {
            Ok((adm, _)) if edited && adm.decision == Decision::Accept => panic!(
                "ATTACK: a stored verification attestation edited in place was admitted on: {:?}",
                adm.reasons
            ),
            Ok((adm, _)) => assert!(
                !edited && adm.decision == Decision::Accept,
                "{edited}: {:?} {:?}",
                adm.decision,
                adm.reasons
            ),
            Err(err) => assert!(
                edited
                    && ["does not digest to its name", "does not verify under"]
                        .iter()
                        .any(|y| err.to_string().contains(y)),
                "{edited}: {err}"
            ),
        }
    }
}
