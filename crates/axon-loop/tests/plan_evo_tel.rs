//! B274 plan register, B273/B281 EVO, B270 TEL.
mod common;
use axon_loop::error::LoopError;
use axon_loop::evo::{self, Hypothesis};
use axon_loop::plan::{self, PilotPlan};
use axon_loop::tel::{self, Total};
use axon_loop_contracts::*;
use common::*;

fn template() -> String {
    include_str!("fixtures/pilot-template.json").to_string()
}

#[test]
fn template_registers_but_never_freezes_or_starts() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = PilotPlan::parse(&template()).unwrap();
    assert_eq!(p.unset_fields().len(), 24);
    plan::register(&s, &p).unwrap();
    let before = snapshot(d.path());
    match plan::freeze(&s, "operator-must-configure") {
        Err(LoopError::NotReady(m)) => assert!(m.contains("task_manifest_ref"), "{m}"),
        o => panic!("{o:?}"),
    }
    assert_eq!(snapshot(d.path()), before, "refused freeze wrote");
    assert!(matches!(
        plan::ready(&s, "operator-must-configure"),
        Err(LoopError::NotReady(_))
    ));
    let v = plan::show(&s, "operator-must-configure").unwrap();
    assert!(v.frozen_ref.is_none());
    assert!(v
        .start_blockers
        .iter()
        .any(|b| b == "operator_approved is false"));
}

#[test]
fn plan_strictness() {
    // missing required-nullable field
    let mut v: serde_json::Value = serde_json::from_str(&template()).unwrap();
    v.as_object_mut().unwrap().remove("budget_rule");
    assert!(PilotPlan::from_value(&v).is_err());
    // unknown field
    let mut v: serde_json::Value = serde_json::from_str(&template()).unwrap();
    v["extra"] = serde_json::json!(1);
    assert!(PilotPlan::from_value(&v).is_err());
    // wrong mutable surface
    let mut v: serde_json::Value = serde_json::from_str(&template()).unwrap();
    v["mutable_surface"] = serde_json::json!("permissions");
    assert!(PilotPlan::from_value(&v).is_err());
    // duplicate key
    let dup = template().replacen(
        "\"runtime_ready\": false,",
        "\"runtime_ready\": false, \"runtime_ready\": true,",
        1,
    );
    assert!(PilotPlan::parse(&dup).is_err());
    // float
    let mut v: serde_json::Value = serde_json::from_str(&template()).unwrap();
    v["repetitions"] = serde_json::json!(1.5);
    assert!(PilotPlan::from_value(&v).is_err());
}

#[test]
fn frozen_plan_is_immutable_and_digest_stable() {
    let w = world();
    let v = complete_plan("exp1", &w.inc_ref, &w.cand_ref);
    let p = PilotPlan::from_value(&v).unwrap();
    let reg = plan::register(&w.s, &p).unwrap();
    let fr = plan::freeze(&w.s, "exp1").unwrap();
    assert_eq!(reg, fr);
    assert_eq!(
        plan::freeze(&w.s, "exp1").unwrap(),
        fr,
        "freeze is idempotent"
    );
    assert_eq!(plan::ready(&w.s, "exp1").unwrap().plan_ref, fr);
    let before = snapshot(w.dir.path());
    let mut v2 = v.clone();
    v2["quality_margin"] = serde_json::json!("pass_rate_margin_ppm=500000");
    assert!(matches!(
        plan::register(&w.s, &PilotPlan::from_value(&v2).unwrap()),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);
    // tamper on disk ⇒ corrupt
    let path = w.s.cas_path("plans", &fr).unwrap();
    let t = std::fs::read_to_string(&path)
        .unwrap()
        .replace("pass_rate_margin_ppm=0", "pass_rate_margin_ppm=9");
    std::fs::write(&path, t).unwrap();
    assert!(matches!(plan::ready(&w.s, "exp1"), Err(LoopError::Io(_))));
}

#[test]
fn ready_refuses_aliasing() {
    let w = world();
    let mut v = complete_plan("alias", &w.inc_ref, &w.cand_ref);
    v["reporting_manifest_ref"] = v["confirmation_manifest_ref"].clone();
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    plan::freeze(&w.s, "alias").unwrap();
    match plan::ready(&w.s, "alias") {
        Err(LoopError::NotReady(m)) => assert!(m.contains("aliasing"), "{m}"),
        o => panic!("{o:?}"),
    }
}

#[test]
fn freeze_refuses_candidate_equal_incumbent() {
    let w = world();
    let v = complete_plan("same", &w.inc_ref, &w.inc_ref);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    assert!(matches!(
        plan::freeze(&w.s, "same"),
        Err(LoopError::NotReady(_))
    ));
}

fn propose_eps(
    s: &axon_loop::Store,
    seed: u64,
    id: &str,
    eps: Vec<serde_json::Value>,
) -> Result<evo::Proposal, LoopError> {
    let req = evo::parse_request(&evo_request(&incumbent(), seed, id, eps).to_string())?;
    evo::propose(s, &req)
}

#[test]
fn evo_proposes_a_bounded_deterministic_candidate() {
    let inc = incumbent();
    let run = |seed| {
        let d = tempfile::tempdir().unwrap();
        let s = store_with_config(d.path());
        let p = propose_eps(&s, seed, "cand-1", vec![discovery_episode(&inc, "d1")]).unwrap();
        (p.candidate, p.candidate_policy_ref)
    };
    let (c1, r1) = run(7);
    let (c2, r2) = run(7);
    assert_eq!((c1.clone(), r1), (c2, r2), "same seed, same candidate");
    assert!(!c1.authority_expansion);
    assert_eq!(c1.parent_policy_ref, digest(&inc).unwrap());
    assert_eq!(c1.candidate_set_ref, inc.candidate_set_ref);
    assert_eq!(c1.controls_ref, inc.controls_ref);
    assert!(
        c1.shortlist.iter().all(|c| inc.shortlist.contains(c)),
        "no added id"
    );
    assert_ne!(c1.shortlist, inc.shortlist);
    assert_eq!(c1.discovery_evidence_refs.len(), 1);
}

#[test]
fn evo_regularizes_history_and_exhausts() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let mut seen = std::collections::BTreeSet::new();
    // 3 removals + 3 swaps = 6 distinct one-edit interventions; same seed every time.
    for i in 0..6 {
        let p = propose_eps(
            &s,
            42,
            &format!("cand-{i}"),
            vec![discovery_episode(&inc, "d1")],
        )
        .unwrap();
        assert!(
            seen.insert(p.candidate.shortlist.clone()),
            "re-proposed a tried intervention"
        );
    }
    let before = snapshot(d.path());
    assert!(matches!(
        propose_eps(&s, 42, "cand-7", vec![discovery_episode(&inc, "d1")]),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(d.path()), before);
    let h = evo::history(&s, &scope()).unwrap();
    assert_eq!(
        h.iter()
            .filter(|h| matches!(h, Hypothesis::Proposed { .. }))
            .count(),
        6
    );
}

#[test]
fn evo_refuses_protected_roles_and_excludes_ineligible() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let before = snapshot(d.path());
    for role in [CorpusRole::Confirmation, CorpusRole::Reporting] {
        let e = propose_eps(
            &s,
            1,
            "c",
            vec![
                discovery_episode(&inc, "d1"),
                episode_with_role(&inc, "p1", role),
            ],
        )
        .unwrap_err();
        assert!(matches!(e, LoopError::Refused(_)), "{e}");
    }
    // mechanism-test only ⇒ nothing eligible ⇒ refused
    assert!(matches!(
        propose_eps(
            &s,
            1,
            "c",
            vec![episode_with_role(&inc, "m1", CorpusRole::MechanismTest)]
        ),
        Err(LoopError::Refused(_))
    ));
    assert_eq!(snapshot(d.path()), before);
    // mixed: mechanism-test is excluded and reported, discovery feeds
    let p = propose_eps(
        &s,
        1,
        "c",
        vec![
            discovery_episode(&inc, "d1"),
            episode_with_role(&inc, "m1", CorpusRole::MechanismTest),
        ],
    )
    .unwrap();
    match p.hypothesis {
        Hypothesis::Proposed {
            excluded,
            discovery_evidence_refs,
            ..
        } => {
            assert_eq!(excluded.len(), 1);
            assert_eq!(discovery_evidence_refs.len(), 1);
        }
        _ => unreachable!(),
    }
}

#[test]
fn evo_refuses_incumbent_outside_eligible_set() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let before = snapshot(d.path());
    let inc = incumbent();
    let mut v = evo_request(&inc, 1, "c", vec![discovery_episode(&inc, "d1")]);
    v["eligible"] = serde_json::json!(["read", "search"]);
    let req = evo::parse_request(&v.to_string()).unwrap();
    assert!(evo::propose(&s, &req).is_err());
    // authority_expansion in the incumbent is refused at parse
    let mut v = evo_request(&inc, 1, "c", vec![discovery_episode(&inc, "d1")]);
    v["incumbent"]["authority_expansion"] = serde_json::json!(true);
    let req = evo::parse_request(&v.to_string()).unwrap();
    assert!(evo::propose(&s, &req).is_err());
    assert_eq!(snapshot(d.path()), before);
}

fn usage(state: UsageState, cost: Option<u64>, liab: u64, attempt: char) -> Usage {
    Usage {
        state,
        cost_micro: cost,
        unresolved_liability_micro: liab,
        currency: Currency::new("USD").unwrap(),
        price_schedule_ref: r('d'),
        attempt_refs: vec![r(attempt)],
    }
}

#[test]
fn tel_keeps_unknown_unknown_and_counts_failures() {
    let a = usage(UsageState::Final, Some(10), 0, '1');
    let b = usage(UsageState::Final, Some(5), 0, '2');
    let s = tel::summarize([
        (&a, Some(EpisodeStatus::Completed)),
        (&b, Some(EpisodeStatus::Failed)),
    ])
    .unwrap();
    assert_eq!(s.by_currency[0].total, Total::Known { cost_micro: 15 });
    assert_eq!(s.by_currency[0].non_completed_records, 1);

    let c = usage(UsageState::Unknown, None, 40, '3');
    let e = usage(UsageState::Estimated, Some(7), 0, '4');
    let s = tel::summarize([
        (&a, Some(EpisodeStatus::Completed)),
        (&c, Some(EpisodeStatus::Cancelled)),
        (&e, Some(EpisodeStatus::OutcomeUnknown)),
    ])
    .unwrap();
    assert_eq!(
        s.by_currency[0].total,
        Total::Unresolved {
            known_sum_micro: 10,
            unknown_count: 2,
            unresolved_liability_micro: 40
        }
    );
    assert_eq!(s.by_currency[0].estimated_sum_micro, 7);
    assert_eq!(s.records, 3);

    // duplicate attempt ⇒ refused, not double counted
    assert!(tel::summarize([(&a, None), (&a, None)]).is_err());
    // currencies never mixed
    let mut eur = usage(UsageState::Final, Some(1), 0, '5');
    eur.currency = Currency::new("EUR").unwrap();
    assert_eq!(
        tel::summarize([(&a, None), (&eur, None)])
            .unwrap()
            .by_currency
            .len(),
        2
    );
}
