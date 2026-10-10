//! Amendment 64 (C9 round 4b, integrate-E): refusal sites whose exemption
//! rested only on ANOTHER check refusing the same input first. Under the
//! strict ruling each needs its own row or a four-cell retirement against the
//! check that dominates it, judged on the PRODUCTION route.
//!
//! Every test is an ATTACK with its CONTROL. Where two (or more) checks refuse
//! the same attack, the assertion accepts any of their reasons: only the
//! attack getting through panics, with its own `ATTACK:` text.

mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

fn specs_of(w: &World) -> Vec<Spec<'_>> {
    pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50))
}

/// Four-cell set (evl's issued-attempt lookup; the population-equality check
/// M108): a trial the request names and delivers, but the admitter never
/// issued (c0 swapped for c0b after outcomes existed), is never judged.
/// Control: the issued population is evaluated.
#[test]
fn a_trial_requested_and_delivered_but_never_issued_is_never_judged() {
    let w = world();
    freeze_plan(&w.s, "ni", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = specs_of(&w);
    assign_specs(&w.s, "ni", &specs);
    let mut picked = specs.clone();
    picked.iter_mut().find(|s| s.3 == "c0").unwrap().3 = "c0b".into();
    let v = evl_request("ni", &w.inc, &w.cand, &picked, &EvlOpts::default());
    intake_all(&w.s, &v);
    let before = snapshot(w.dir.path());
    match axon_loop::evl::evaluate(
        &w.s,
        &axon_loop::evl::parse_request(&v.to_string()).unwrap(),
    ) {
        Ok((rec, _)) => panic!(
            "ATTACK: a trial requested and delivered but never issued was judged: {:?}",
            rec.arms
        ),
        Err(e) => assert!(
            ["not the one journalled before execution", "never issued in"]
                .iter()
                .any(|y| e.to_string().contains(y)),
            "{e}"
        ),
    }
    assert_eq!(snapshot(w.dir.path()), before, "a refusal wrote something");
    // CONTROL (a fresh store): the issued population is evaluated.
    let w = world();
    freeze_plan(&w.s, "ni", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    assign_specs(&w.s, "ni", &specs_of(&w));
    let honest = evl_request("ni", &w.inc, &w.cand, &specs_of(&w), &EvlOpts::default());
    let (rec, _) = evaluate(&w.s, &honest).unwrap();
    assert!(
        rec.arms.iter().all(|a| a.verified_pass == 2),
        "{:?}",
        rec.arms
    );
}

/// A ledger holding a line that is not an entry (a junk line written into the
/// middle of `ledger.jsonl`) is corruption: the next operation that reads the
/// ledger is refused, naming the line, never served from the entries around
/// it. (Only a torn LAST line, an append whose fsync never returned, is
/// ignored.) Control: the same operation on the untouched ledger succeeds.
#[test]
fn a_ledger_with_a_line_that_is_not_an_entry_is_never_read() {
    for planted in [false, true] {
        let w = world();
        freeze_plan(&w.s, "j1", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        if planted {
            let p = w.s.root().join("ledger.jsonl");
            let text = std::fs::read_to_string(&p).unwrap();
            let mut lines: Vec<&str> = text.lines().collect();
            assert!(
                lines.len() >= 2,
                "the fixture ledger has entries around the junk"
            );
            lines.insert(1, r#"{"junk":true}"#);
            std::fs::write(&p, lines.join("\n") + "\n").unwrap();
        }
        // The admitter's next journalled act reads the ledger first.
        let trials = specs_of(&w)
            .iter()
            .map(|(armid, p, task, t, _, _)| {
                json!({"task_id": task, "arm_id": armid, "trial_id": t,
                       "attempt_id": format!("{t}-a1"), "policy_ref": digest(*p).unwrap()})
            })
            .collect();
        let r = axon_loop::plan::assign(&w.s, &assignment_record("j1", trials));
        match (planted, r) {
            (true, Ok(_)) => panic!(
                "ATTACK: a ledger holding a line that is not an entry was read as the ledger"
            ),
            (true, Err(e)) => assert!(e.to_string().contains("line 2"), "{e}"),
            (false, r) => {
                r.unwrap();
            }
        }
    }
}

/// Four-cell pair (strict_record's closed-canonical-shape check; check_name's
/// digest re-check, M978): an evaluation record EDITED in place in the store
/// (the challenger's failures rewritten as passes) and written back in an
/// alternative encoding (each outcome as `{"verified_pass": null}`) is never
/// admitted. Control: the unedited record is admitted, and does not ACCEPT a
/// challenger that failed.
#[test]
fn a_stored_record_edited_and_re_encoded_is_never_admitted() {
    for edited in [false, true] {
        let w = world();
        freeze_plan(&w.s, "re", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
        let v = evl_request("re", &w.inc, &w.cand, &specs, &EvlOpts::default());
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
                a["verified_pass"] = json!(2);
                a["fail"] = json!(0);
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
                    // The alternative encoding of the unit variant.
                    t["outcome"] = json!({ "verified_pass": null });
                }
                a["trials"] = trials;
            }
            std::fs::write(&path, serde_json::to_vec(&rec).unwrap()).unwrap();
        }
        match admit(&w.s, "re", &e, ADMITTER, false) {
            Ok((adm, _)) if edited && adm.decision == Decision::Accept => panic!(
                "ATTACK: an evaluation record edited in the store and re-encoded was admitted: {:?}",
                adm.reasons
            ),
            Ok((adm, _)) => assert!(
                !edited && adm.decision != Decision::Accept,
                "{edited}: {:?} {:?}",
                adm.decision,
                adm.reasons
            ),
            Err(err) => assert!(
                edited
                    && ["is corrupt", "closed canonical shape"]
                        .iter()
                        .any(|y| err.to_string().contains(y)),
                "{edited}: {err}"
            ),
        }
    }
}

/// Intake ONE trial `t` of request `v` (the population journalled and the
/// policies stored first, as `intake_all` does), returning intake's verdict.
fn intake_one(
    s: &axon_loop::store::Store,
    v: &Value,
    t: &Value,
    ack_edit: impl Fn(String) -> String,
) -> Result<u64, LoopError> {
    assign_request(s, v);
    let acks: Vec<String> = v["policies"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            let p: PolicyEnvelope = serde_json::from_value(p.clone()).unwrap();
            axon_loop::candidates::put_policy(s, &p).unwrap();
            ack_edit(ack_for(&p).to_string())
        })
        .collect();
    let text = |x: &Value| (!x.is_null()).then(|| x.to_string());
    axon_loop::intake::intake_episode(
        s,
        &axon_loop::intake::IntakeInput {
            episode: &t["episode"].to_string(),
            context: &t["context"].to_string(),
            acks: &acks,
            projection: None,
            source_episode: None,
            verification_request: text(&t["verification_request"]).as_deref(),
            verification_receipt: text(&t["verification_receipt"]).as_deref(),
            verification_attestation: text(&t["verification_attestation"]).as_deref(),
            verification_psv_evidence: text(&t["verification_psv_evidence"]).as_deref(),
        },
    )
    .map(|o| o.ledger_seq)
}

/// Four-cell set (intake's cl22 policy-reference check; the store's cl22
/// rule for a CAS path; check_name's digest re-check, M978; bind_episode's
/// policy byte binding, M965): an episode naming the policy it ran by a
/// NON-cl22 alias of the stored policy's digest (`acf1:<the same hex>`) is
/// never intaken: no stored record is named but by the cl22 digest of its
/// content. Control: the episode naming the policy by its cl22 name intakes.
#[test]
fn an_episode_naming_its_policy_by_a_non_cl22_alias_is_never_intaken() {
    for alias in [false, true] {
        let w = world();
        freeze_plan(&w.s, "al", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let v = evl_request("al", &w.inc, &w.cand, &specs_of(&w), &EvlOpts::default());
        let mut t = v["trials"][0].clone();
        let name = t["episode"]["policy_ref"].as_str().unwrap().to_string();
        let aliased = format!("acf1:{}", name.strip_prefix("cl22:").unwrap());
        if alias {
            t["episode"]["policy_ref"] = json!(aliased);
        }
        // The producer's acknowledgement pins the policy by the name the
        // episode uses (both are the producer's documents).
        let ack = |a: String| if alias { a.replace(&name, &aliased) } else { a };
        match (alias, intake_one(&w.s, &v, &t, ack)) {
            (true, Ok(r)) => panic!(
                "ATTACK: an episode naming its policy by a non-cl22 alias was intaken at seq {r}"
            ),
            (true, Err(e)) => {
                assert!(
                    [
                        "is not a cl22: policy reference",
                        "references must be cl22:",
                        "is not a policy this store knows",
                        "is corrupt: content digests",
                        "policy/context byte mismatch",
                    ]
                    .iter()
                    .any(|y| e.to_string().contains(y)),
                    "{e}"
                );
            }
            (false, r) => {
                r.unwrap();
            }
        }
    }
}

/// Four-cell set (evl's vacuous-pass check; the episode contract's
/// passed-requires-matched-checks rule; the check receipt's same rule): a
/// verdict of PASSED over ZERO matched checks, its check receipt saying the
/// same and genuinely attested by the verifier, never counts as a verified
/// pass. Control: the same trial with its two matched checks counts.
#[test]
fn a_pass_over_zero_matched_checks_never_counts() {
    for vacuous in [false, true] {
        let w = world();
        freeze_plan(&w.s, "vz", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        let specs = specs_of(&w);
        assign_specs(&w.s, "vz", &specs);
        let mut v = evl_request("vz", &w.inc, &w.cand, &specs, &EvlOpts::default());
        if vacuous {
            for t in v["trials"].as_array_mut().unwrap() {
                t["episode"]["verification"]["matched_checks"] = json!(0);
                t["verification_receipt"]["matched_checks"] = json!(0);
                t["episode"]["verification"]["verifier_ref"] =
                    json!(digest_value(&t["verification_receipt"]).unwrap());
                t["verification_attestation"] = attest(
                    VERIFIER,
                    &t["verification_request"],
                    &t["verification_receipt"],
                );
            }
        }
        // The contract schemas refuse the request outright; with them gone
        // the contract code refuses each episode at intake; with that gone,
        // evaluation counts the trial Unknown (vacuous). Any of them holds.
        match (vacuous, evaluate(&w.s, &v)) {
            (true, Err(e)) => assert!(e.to_string().contains("matched_checks"), "{e}"),
            (true, Ok((rec, _))) => {
                let passes: u64 = rec.arms.iter().map(|a| a.verified_pass).sum();
                if passes > 0 {
                    panic!("ATTACK: a pass over zero matched checks counted ({passes} passes)")
                }
            }
            (false, r) => {
                let (rec, _) = r.unwrap();
                let passes: u64 = rec.arms.iter().map(|a| a.verified_pass).sum();
                assert_eq!(passes, 4, "{:?}", rec.arms);
            }
        }
    }
}
