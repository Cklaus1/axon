//! Red team round 2 (`.axon-v022/redteam/axon-loop.md`): one regression per
//! DEFECT row. Every refusal asserts the store bytes did not change.
mod common;
use axon_loop::admission::{AdmissionRecord, Decision};
use axon_loop::error::LoopError;
use axon_loop::plan::{self, PilotPlan};
use axon_loop::{evo, null_policy_ref, pointer, Store};
use axon_loop_contracts::*;
use common::*;
use serde_json::json;

fn t(v: serde_json::Value) -> PolicyTransition {
    tparse(&v)
}

fn refused_unchanged<T: std::fmt::Debug>(
    w: &World,
    f: impl FnOnce() -> Result<T, LoopError>,
) -> LoopError {
    let before = snapshot(w.dir.path());
    let e = f().expect_err("must refuse");
    assert_eq!(
        snapshot(w.dir.path()),
        before,
        "refusal changed the store: {e}"
    );
    e
}

// ── root cause 1: activation re-derives, never trusts ─────────────────────

/// A6: a forged ACCEPT stored under its own correct cl22 name.
#[test]
fn a6_forged_admission_record_is_refused() {
    let w = world();
    let adm = accepted(&w, "exp");
    let mut forged: AdmissionRecord = axon_loop::admission::load(&w.s, &adm).unwrap();
    forged.plan_ref = r('c');
    forged.evaluation_ref = r('d');
    let fref = w.s.put_cas("admissions", &forged).unwrap();
    let e = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&fref),
                false,
            )),
        )
    });
    assert!(e.to_string().contains("never journalled"), "{e}");
    // A REJECT turned into ACCEPT (valid CAS name, journalled ref is the REJECT's)
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (mut rec, _) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Reject);
    rec.decision = Decision::Accept;
    let fref = w.s.put_cas("admissions", &rec).unwrap();
    refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&fref),
                false,
            )),
        )
    });
}

/// O1/O2: a hand-built candidate ADDING a tool, never proposed by EVO.
#[test]
fn o1_o2_non_evo_or_widening_candidate_cannot_be_frozen() {
    let w = world();
    let mut wide = policy("wide", &["read", "search", "edit", "write"]);
    wide.parent_policy_ref = w.inc_ref.clone();
    let wref = w.s.put_cas("policies", &wide).unwrap();
    let v = complete_plan("wideexp", &w.inc_ref, &wref);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    let e = refused_unchanged(&w, || plan::freeze(&w.s, "wideexp"));
    assert!(e.to_string().contains("not produced by EVO"), "{e}");
    // A non-EVO but narrowing candidate is refused for the same reason.
    let narrow = policy("narrow", &["read"]);
    let nref = w.s.put_cas("policies", &narrow).unwrap();
    plan::register(
        &w.s,
        &PilotPlan::from_value(&complete_plan("narrowexp", &w.inc_ref, &nref)).unwrap(),
    )
    .unwrap();
    refused_unchanged(&w, || plan::freeze(&w.s, "narrowexp"));
}

// ── root cause 2: the pointer is the ledger's projection ───────────────────

/// A7b: pointer.json edited at the same epoch to a different active policy.
#[test]
fn a7b_hand_edited_pointer_is_corrupt() {
    let w = world();
    let p = w.s.scope_dir(&scope()).join("pointer.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["pointer"]["active_policy_ref"] = json!(w.cand_ref);
    std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let e = pointer::resolve(&w.s, &scope()).unwrap_err();
    assert!(matches!(e, LoopError::Io(_)) && e.exit_code() == 2, "{e}");
    assert!(e.to_string().contains("projection"), "{e}");
}

/// F3b: history hand-extended with a never-active policy.
#[test]
fn f3b_injected_history_is_corrupt() {
    let w = world();
    let p = w.s.scope_dir(&scope()).join("pointer.json");
    let mut v: serde_json::Value = serde_json::from_slice(&std::fs::read(&p).unwrap()).unwrap();
    v["pointer"]["history"] = json!([{"policy_ref": w.cand_ref, "admission_ref": r('a'), "mechanism_test": false,
        "activated_epoch": 0, "retired_epoch": 1}]);
    std::fs::write(&p, serde_json::to_vec_pretty(&v).unwrap()).unwrap();
    let e = pointer::transition(
        &w.s,
        &t(transition(
            "rb",
            "rollback",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&r('a')),
            false,
        )),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
}

/// A7c: deleting the history must not reissue an epoch.
#[test]
fn a7c_deleting_ledger_and_pointer_is_detected() {
    let w = world();
    let root = w.s.root().to_path_buf();
    std::fs::remove_file(root.join("ledger.jsonl")).unwrap();
    std::fs::remove_file(root.join("ledger.head")).unwrap();
    std::fs::remove_file(w.s.scope_dir(&scope()).join("pointer.json")).unwrap();
    let e = pointer::transition(
        &w.s,
        &t(transition(
            "x",
            "activate",
            &null_policy_ref(),
            Some(&w.inc_ref),
            0,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
    assert!(pointer::load(&w.s, &scope()).is_err());
    // deleting only the ledger (head remains) or only the head is detected too
    let w = world();
    std::fs::remove_file(w.s.root().join("ledger.jsonl")).unwrap();
    assert_eq!(pointer::load(&w.s, &scope()).unwrap_err().exit_code(), 2);
    let w = world();
    std::fs::remove_file(w.s.root().join("ledger.head")).unwrap();
    assert_eq!(pointer::load(&w.s, &scope()).unwrap_err().exit_code(), 2);
    // truncating the ledger by one line is detected by the head
    let w = world();
    let lp = w.s.root().join("ledger.jsonl");
    let txt = std::fs::read_to_string(&lp).unwrap();
    let mut lines: Vec<&str> = txt.lines().collect();
    lines.pop();
    std::fs::write(&lp, lines.join("\n") + "\n").unwrap();
    assert_eq!(pointer::load(&w.s, &scope()).unwrap_err().exit_code(), 2);
}

// ── root cause 3: the admission binds the incumbent it replaced ────────────

/// H1: a genuine ACCEPT whose incumbent `P` is NOT the active policy cannot
/// displace the active policy. Setup: the baseline `inc` is active; `P` is a
/// stored, never-active policy; candidate `C` is an EVO mutation of `P`;
/// C vs P is evaluated at the current epoch and ACCEPTed. Without the H1
/// check, `C` would replace `inc`, which it was never compared against.
#[test]
fn h1_admission_incumbent_must_be_the_active_policy() {
    let w = world();
    let p = policy("parked", &["read", "search"]);
    let pref = w.s.put_cas("policies", &p).unwrap();
    let (c, cref) = propose(&w.s, &p, 5, "cand-of-parked");
    freeze_plan(&w.s, "h1", &pref, &cref, |_| {}).unwrap();
    let specs = pair(&p, &c, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("h1", &p, &c, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, adm) = admit(&w.s, "h1", &e, ADMITTER, false).unwrap();
    assert_eq!(
        rec.decision,
        Decision::Accept,
        "precondition: a real ACCEPT {:?}",
        rec.reasons
    );
    assert_eq!(
        rec.evaluated_at_epoch.get(),
        1,
        "precondition: current epoch"
    );
    let err = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&cref),
                1,
                Some(&adm),
                false,
            )),
        )
    });
    assert!(err.to_string().contains("(H1)"), "{err}");
}

/// H2: from paused, only the incumbent-of-record may be activated.
#[test]
fn h2_candidate_not_activatable_from_paused() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
    )
    .unwrap();
    let e = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &null_policy_ref(),
                Some(&w.cand_ref),
                2,
                Some(&adm),
                false,
            )),
        )
    });
    assert!(e.to_string().contains("H2"), "{e}");
    // and never from the null sentinel of a fresh scope
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let iref = s.put_cas("policies", &inc).unwrap();
    assert!(pointer::transition(
        &s,
        &t(transition(
            "a",
            "activate",
            &null_policy_ref(),
            Some(&iref),
            0,
            Some(&r('a')),
            false
        ))
    )
    .is_err());
}

// ── root cause 4: freeze orders outcomes ───────────────────────────────────

/// G7: plan shopping after outcomes exist.
#[test]
fn g7_plan_shopping_is_refused() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 10, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 10, 10, 9, Some(100), Some(50));
    specs.truncate(20);
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    assert_eq!(
        admit(&w.s, "exp", &e, ADMITTER, false).unwrap().0.decision,
        Decision::Reject
    );
    // a laxer plan for the same candidate cannot be frozen
    let v = complete_plan("lax", &w.inc_ref, &w.cand_ref);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    let err = refused_unchanged(&w, || plan::freeze(&w.s, "lax"));
    assert!(
        err.to_string().contains("one experiment per candidate"),
        "{err}"
    );
    // and the stored evaluation cannot be admitted under another experiment
    assert!(admit(&w.s, "lax", &e, ADMITTER, false).is_err());
}

/// G7 (ordering): trials preflighted before the freeze are refused.
#[test]
fn g7_trials_before_freeze_are_refused() {
    let w = world();
    let before_freeze = axon_loop::now_ms().saturating_sub(60_000);
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let o = EvlOpts {
        created_ms: Some(before_freeze),
        ..Default::default()
    };
    let e = refused_unchanged(&w, || {
        evaluate(&w.s, &evl_request("exp", &w.inc, &w.cand, &specs, &o))
    });
    assert!(e.to_string().contains("before the plan froze"), "{e}");
}

/// G6: deleting a freeze leaves a trace; the id stays frozen.
#[test]
fn g6_freeze_is_permanent() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    // There is no frozen.json to delete any more; the freeze is a ledger event.
    assert!(!w.s.root().join("plans/exp/frozen.json").exists());
    let mut lax = complete_plan("exp", &w.inc_ref, &w.cand_ref);
    lax["quality_margin"] = json!("pass_rate_margin_ppm=100000");
    refused_unchanged(&w, || {
        plan::register(&w.s, &PilotPlan::from_value(&lax).unwrap())
    });
    // deleting the plan's CAS bytes is corruption, not an unfreeze
    let fr = plan::ready(&w.s, "exp").unwrap().plan_ref;
    std::fs::remove_file(w.s.cas_path("plans", &fr).unwrap()).unwrap();
    assert_eq!(plan::ready(&w.s, "exp").unwrap_err().exit_code(), 2);
}

/// K1: evidence evaluated at an older epoch cannot activate.
#[test]
fn k1_stale_evaluation_epoch_refused() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
    )
    .unwrap();
    pointer::transition(
        &w.s,
        &t(transition(
            "rb",
            "rollback",
            &null_policy_ref(),
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap();
    let e = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                3,
                Some(&adm),
                false,
            )),
        )
    });
    assert!(
        matches!(e, LoopError::Conflict(_)) && e.to_string().contains("K1"),
        "{e}"
    );
}

// ── root cause 5 / 7: the rule grammar ─────────────────────────────────────

fn freeze_err(field: &str, value: &str) -> LoopError {
    let w = world();
    let mut v = complete_plan("exp", &w.inc_ref, &w.cand_ref);
    v[field] = json!(value);
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    refused_unchanged(&w, || plan::freeze(&w.s, "exp"))
}

/// I3, I4, I7, I8, I13, I14: non-executable or unbounded rules cannot freeze.
#[test]
fn i3_i4_i7_i8_i13_i14_rules_must_be_executable() {
    for (f, v) in [
        ("quality_margin", "pass_rate_margin_ppm=+5"), // I3
        ("quality_margin", "pass_rate_margin_ppm=05"), // I4
        ("quality_margin", "pass_rate_margin_ppm=9007199254740991"), // I7
        ("quality_margin", "pass_rate_margin_ppm=2000000"), // I8
        ("quality_margin", "pass_rate_margin_ppm=1000000"), // exactly 100%
        ("economic_threshold", "min_cost_reduction_ppm=010"),
        ("budget_rule", "max_unresolved_liability_micro=+0"),
        ("order_rule", "anything goes"),    // I13
        ("cache_rule", "anything goes"),    // I13
        ("independent_unit", "repository"), // I14
        ("independent_unit", "trial"),      // I14
    ] {
        let e = freeze_err(f, v);
        assert!(matches!(e, LoopError::NotReady(_)), "{f}={v}: {e}");
    }
}

/// J201: 0/10 can never be accepted, whatever the (bounded) margin.
#[test]
fn j201_zero_pass_candidate_never_accepted() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 10, |v| {
        v["quality_margin"] = json!("pass_rate_margin_ppm=999999")
    })
    .unwrap();
    let specs = pair(&w.inc, &w.cand, 10, 10, 0, Some(100), Some(10));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, _) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_ne!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
}

/// J109: arms on disjoint task sets. Since the task manifest is frozen, an
/// evaluation whose arms do not both cover exactly the manifest is REFUSED
/// (stronger than the earlier INCONCLUSIVE); the admission-side "unpaired"
/// check remains as defence in depth.
#[test]
fn j109_unpaired_task_sets_refused() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let mut specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    for s in specs.iter_mut().skip(2) {
        s.2 = s.2.replace("task", "easy");
    }
    let e = refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
        )
    });
    assert!(e.to_string().contains("task manifest"), "{e}");
}

// ── root cause 6: missing trial = unknown cost ─────────────────────────────

/// J202 / Q4: a missing candidate trial makes economics unknown.
#[test]
fn j202_q4_missing_trial_is_unknown_cost() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 4, |v| {
        v["quality_margin"] = json!("pass_rate_margin_ppm=500000")
    })
    .unwrap();
    let specs = pair(&w.inc, &w.cand, 4, 3, 4, Some(100), Some(120));
    let o = EvlOpts {
        deliver: Box::new(|t| t != "c3"),
        ..Default::default()
    };
    let (rec, e) = evaluate(&w.s, &evl_request("exp", &w.inc, &w.cand, &specs, &o)).unwrap();
    let c = rec.arm_for_policy(&w.cand_ref).unwrap();
    assert!(
        matches!(
            c.economics.single_total(),
            Some(axon_loop::tel::Total::Unresolved {
                unknown_count: 1,
                ..
            })
        ),
        "{:?}",
        c.economics
    );
    let (adm, _) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_ne!(adm.decision, Decision::Accept, "{:?}", adm.reasons);
    assert!(
        adm.reasons
            .iter()
            .any(|r| r.contains("economics cannot be established")),
        "{:?}",
        adm.reasons
    );
}

// ── root cause 8: EVO evidence ─────────────────────────────────────────────

/// N4 / N5: self-verified or untrusted-verifier discovery evidence is excluded.
#[test]
fn n4_n5_evo_needs_trusted_non_proposer_verifier() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    for who in [PROPOSER, "anyone:at-all"] {
        let mut tr = Trial::new(&inc, "disc-task", "incumbent", "x");
        tr.role = CorpusRole::Discovery;
        tr.verifier = who;
        tr.created_ms = Some(1_000);
        let req = evo::parse_request(
            &evo_request(&inc, 3, "c", vec![trial(&tr)["episode"].clone()]).to_string(),
        )
        .unwrap();
        let before = snapshot(d.path());
        assert!(
            matches!(evo::propose(&s, &req), Err(LoopError::Refused(_))),
            "{who}"
        );
        assert_eq!(snapshot(d.path()), before);
    }
}

// ── root cause 9: mechanism tests are labelled and quarantined ─────────────

/// M4: mechanism activation is labelled in the pin + hypothesis, and is not
/// an incumbent/rollback target for real transitions.
#[test]
fn m4_mechanism_activation_is_labelled_and_quarantined() {
    let w = world();
    freeze_plan(&w.s, "mech", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let o = EvlOpts {
        role: CorpusRole::MechanismTest,
        ..Default::default()
    };
    let (_, e) = evaluate(&w.s, &evl_request("mech", &w.inc, &w.cand, &specs, &o)).unwrap();
    let (rec, adm) = admit(&w.s, "mech", &e, ADMITTER, true).unwrap();
    assert_eq!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
    let h = evo::history(&w.s, &scope()).unwrap();
    assert!(h.iter().any(|x| matches!(
        x,
        evo::Hypothesis::Verdict {
            mechanism_test: true,
            ..
        }
    )));
    // mechanism admission cannot drive a real activation
    refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                false,
            )),
        )
    });
    pointer::transition(
        &w.s,
        &t(transition(
            "m",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            true,
        )),
    )
    .unwrap();
    let r = pointer::resolve(&w.s, &scope()).unwrap();
    assert!(r.mechanism_test, "resolve must label a mechanism-test pin");
    // back to the real incumbent, then the fixture is not a rollback target
    pointer::transition(
        &w.s,
        &t(transition(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        )),
    )
    .unwrap();
    refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "rb2",
                "rollback",
                &w.inc_ref,
                Some(&w.cand_ref),
                3,
                Some(&adm),
                false,
            )),
        )
    });
}

// ── root cause 10: filesystem ──────────────────────────────────────────────

/// D1 / D8 / D9: ids are validated before any filesystem call.
#[test]
fn d1_d8_d9_bad_ids_touch_nothing() {
    let outer = tempfile::tempdir().unwrap();
    let store = outer.path().join("store");
    let s = store_with_config(&store);
    let before = snapshot(outer.path());
    let abs = outer.path().join("abs/deep");
    for id in [
        "../../outside/evil",
        abs.to_str().unwrap(),
        "a/b",
        ".",
        "..",
        "",
    ] {
        assert!(
            matches!(plan::freeze(&s, id), Err(LoopError::Usage(_))),
            "{id}"
        );
        assert!(
            matches!(plan::show(&s, id), Err(LoopError::Usage(_))),
            "{id}"
        );
    }
    assert_eq!(snapshot(outer.path()), before);
    assert!(!outer.path().join("outside").exists() && !abs.exists());
    // D9: the only lock is <canonical store>/locks/root.lock
    let s2 = Store::open(store.join(".").join("..").join("store")).unwrap();
    assert_eq!(s2.root(), s.root());
}

#[cfg(unix)]
fn symlink(a: &std::path::Path, b: &std::path::Path) {
    std::os::unix::fs::symlink(a, b).unwrap();
}

/// E1: a symlinked ledger is refused, and nothing is appended outside.
#[test]
fn e1_symlinked_ledger_refused() {
    let w = world();
    let outside = tempfile::NamedTempFile::new().unwrap();
    let lp = w.s.root().join("ledger.jsonl");
    std::fs::copy(&lp, outside.path()).unwrap();
    let before_out = std::fs::read(outside.path()).unwrap();
    std::fs::remove_file(&lp).unwrap();
    symlink(outside.path(), &lp);
    let e = pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
    assert_eq!(std::fs::read(outside.path()).unwrap(), before_out);
}

/// E4: a symlinked config.json is not read.
#[test]
fn e4_symlinked_config_refused() {
    let w = world();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(outside.path(), json!({"schema":"axon.loop.config/1","trusted_admitters":["evil:me"],"trusted_verifiers":[]}).to_string()).unwrap();
    let cp = w.s.root().join("config.json");
    std::fs::remove_file(&cp).unwrap();
    symlink(outside.path(), &cp);
    let e = pointer::revoke(
        &w.s,
        &scope(),
        &w.inc_ref,
        &r('e'),
        &OpaqueRef::new("evil:me").unwrap(),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
}

/// E5: a symlinked scope directory is refused.
#[test]
fn e5_symlinked_scope_dir_refused() {
    let w = world();
    let outside = tempfile::tempdir().unwrap();
    let tenant = w.s.root().join("scopes").join("fixture-tenant");
    let moved = outside.path().join("t");
    std::fs::rename(&tenant, &moved).unwrap();
    symlink(&moved, &tenant);
    let before = snapshot(outside.path());
    let e = pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
    )
    .unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
    assert_eq!(snapshot(outside.path()), before);
}

// ── guards that other checks shadow: tested in isolation ───────────────────

/// A6 (re-derivation, isolated): even a forged record that IS journalled —
/// someone with store write access appended a well-chained ledger entry for
/// it — is refused, because it does not re-derive from its plan + evaluation.
#[test]
fn a6_journalled_forgery_does_not_rederive() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (mut rec, _) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Reject);
    rec.decision = Decision::Accept;
    rec.reasons = vec!["forged".into()];
    let fref = w.s.put_cas("admissions", &rec).unwrap();
    {
        let mut tx = axon_loop::ledger::Tx::begin(&w.s).unwrap();
        tx.append(axon_loop::ledger::Event::Admission {
            scope: scope(),
            experiment_id: "exp".into(),
            admission_ref: fref.clone(),
            target_policy_ref: w.cand_ref.clone(),
            decision: Decision::Accept,
            mechanism_test: false,
        })
        .unwrap();
    }
    let err = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "a",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&fref),
                false,
            )),
        )
    });
    assert!(err.to_string().contains("does not re-derive"), "{err}");
}

/// M4 (isolated): with a mechanism fixture ACTIVE, a genuine ACCEPT whose
/// incumbent IS that fixture still cannot drive a real activation.
#[test]
fn m4_mechanism_fixture_is_not_a_real_incumbent() {
    let w = world();
    freeze_plan(&w.s, "mech", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let o = EvlOpts {
        role: CorpusRole::MechanismTest,
        ..Default::default()
    };
    let (_, e) = evaluate(&w.s, &evl_request("mech", &w.inc, &w.cand, &specs, &o)).unwrap();
    let (_, madm) = admit(&w.s, "mech", &e, ADMITTER, true).unwrap();
    pointer::transition(
        &w.s,
        &t(transition(
            "m",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&madm),
            true,
        )),
    )
    .unwrap();
    // a real candidate compared against the (fixture) active policy
    let (c2, c2ref) = propose(&w.s, &w.cand, 7, "cand-2");
    freeze_plan(&w.s, "real", &w.cand_ref, &c2ref, |_| {}).unwrap();
    let mut specs = pair(&w.cand, &c2, 2, 2, 2, Some(100), Some(50));
    for s in specs.iter_mut() {
        s.3 = format!("real-{}", s.3); // trial ids are unique for the scope's lifetime
    }
    let o = EvlOpts {
        epoch: 2,
        ..Default::default()
    };
    let (_, e2) = evaluate(&w.s, &evl_request("real", &w.cand, &c2, &specs, &o)).unwrap();
    let (rec, adm2) = admit(&w.s, "real", &e2, ADMITTER, false).unwrap();
    assert_eq!(
        rec.decision,
        Decision::Accept,
        "precondition {:?}",
        rec.reasons
    );
    let err = refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "r",
                "activate",
                &w.cand_ref,
                Some(&c2ref),
                2,
                Some(&adm2),
                false,
            )),
        )
    });
    assert!(err.to_string().contains("mechanism-test fixture"), "{err}");
}

/// J201 (isolated): both arms 0/n — the margin arithmetic alone would call
/// that noninferior; a candidate with no verified pass is never ACCEPTed.
#[test]
fn j201_zero_vs_zero_never_accepted() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 0, 0, Some(100), Some(10));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, _) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_ne!(rec.decision, Decision::Accept, "{:?}", rec.reasons);
}

/// N4 (isolated): the proposer is excluded even if an operator ALSO lists it
/// as a trusted verifier.
#[test]
fn n4_proposer_excluded_even_when_trusted_as_verifier() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let mut cfg = s.config().unwrap();
    cfg.trusted_verifiers
        .push(OpaqueRef::new(PROPOSER).unwrap());
    s.write_config(&cfg).unwrap();
    let inc = incumbent();
    let mut tr = Trial::new(&inc, "disc-task", "incumbent", "x");
    tr.role = CorpusRole::Discovery;
    tr.verifier = PROPOSER;
    tr.created_ms = Some(1_000);
    let req = evo::parse_request(
        &evo_request(&inc, 3, "c", vec![trial(&tr)["episode"].clone()]).to_string(),
    )
    .unwrap();
    let e = evo::propose(&s, &req).unwrap_err();
    assert!(matches!(e, LoopError::Refused(_)), "{e}");
}

/// C10 (round-3 crash finding): the two crash windows of the fixed order —
/// killed after the ledger append only, and killed after the projection but
/// before the head — both roll forward. (The pre-fix order wrote the head
/// FIRST, so a kill between head and projection left a committed head with a
/// stale projection, reported as corruption: 4/400 kill -9 runs. That window
/// no longer exists; the kill -9 harness is what exercises it.)
#[test]
fn c10_crash_windows_between_append_projection_and_head_recover() {
    for keep_projection in [false, true] {
        let w = world();
        let root = w.s.root().to_path_buf();
        let ptr = w.s.scope_dir(&scope()).join("pointer.json");
        let (head0, ptr0) = (
            std::fs::read(root.join("ledger.head")).unwrap(),
            std::fs::read(&ptr).unwrap(),
        );
        pointer::transition(
            &w.s,
            &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
        )
        .unwrap();
        std::fs::write(root.join("ledger.head"), &head0).unwrap();
        if !keep_projection {
            std::fs::write(&ptr, &ptr0).unwrap();
        }
        let p = pointer::load(&w.s, &scope()).unwrap();
        assert_eq!(
            (p.epoch.get(), p.active_policy_ref.is_none()),
            (2, true),
            "keep_projection={keep_projection}"
        );
    }
}

/// CW1 / CR1 / CR2 (independent round 3): the admission decision and its
/// hypothesis verdict are ONE ledger entry. Dropping the last ledger line (a
/// kill -9 before the head moved) returns the store exactly to the pre-admit
/// state, and a retry records BOTH together — the verdict is never lost.
#[test]
fn cw1_admit_is_one_ledger_entry() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 0, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let root = w.s.root().to_path_buf();
    let lp = root.join("ledger.jsonl");
    let (pre_ledger, pre_head) = (
        std::fs::read(&lp).unwrap(),
        std::fs::read(root.join("ledger.head")).unwrap(),
    );
    let n0 = pre_ledger.iter().filter(|b| **b == b'\n').count();
    let (rec, adm) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Reject);
    let n1 = std::fs::read(&lp)
        .unwrap()
        .iter()
        .filter(|b| **b == b'\n')
        .count();
    assert_eq!(n1, n0 + 1, "admit must append exactly one ledger entry");
    let verdicts = |w: &World| {
        evo::history(&w.s, &scope())
            .unwrap()
            .into_iter()
            .filter(|h| {
                matches!(
                    h,
                    evo::Hypothesis::Verdict {
                        verdict: evo::Verdict::Reject,
                        ..
                    }
                )
            })
            .count()
    };
    assert_eq!(verdicts(&w), 1);
    // kill -9 before the head moved: the only possible intermediate state
    std::fs::write(&lp, &pre_ledger).unwrap();
    std::fs::write(root.join("ledger.head"), &pre_head).unwrap();
    assert_eq!(verdicts(&w), 0, "pre-state: no decision, no verdict");
    let (_, adm2) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(adm2, adm);
    assert_eq!(
        verdicts(&w),
        1,
        "retry records decision and verdict together"
    );
}

/// AB6 / AB7 / AB8 (independent round 3): EVL runs the paired-trial context
/// checks on every trial. An expired window, expected != observed, a parent
/// echo, an untrusted observer, or no configured observers at all makes the
/// trial `unknown` — never a verified pass.
#[test]
fn ab6_ab7_ab8_context_checks_gate_every_trial() {
    fn tweak(t: &mut serde_json::Value, f: &dyn Fn(&mut ExecutionContextReceipt)) {
        let mut ctx: ExecutionContextReceipt = parse(&t["context"].to_string()).unwrap();
        f(&mut ctx);
        let mut ep: LoopEpisode = parse(&t["episode"].to_string()).unwrap();
        ep.context_ref = digest(&ctx).unwrap();
        t["context"] = serde_json::to_value(&ctx).unwrap();
        t["episode"] = serde_json::to_value(&ep).unwrap();
    }
    type Tweak = Box<dyn Fn(&mut ExecutionContextReceipt)>;
    let cases: Vec<(&str, Tweak)> = vec![
        ("AB6 expired", Box::new(|c| c.expires_ms = c.created_ms + 1)),
        (
            "AB7 mismatch",
            Box::new(|c| c.expected.is_primary_worktree = !c.observed.is_primary_worktree),
        ),
        (
            "AB8 parent echo",
            Box::new(|c| c.observed_issuer_ref = c.expected_issuer_ref.clone()),
        ),
        (
            "untrusted observer",
            Box::new(|c| c.observed_issuer_ref = OpaqueRef::new("anyone:at-all").unwrap()),
        ),
    ];
    for (name, f) in cases {
        let w = world();
        freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(3));
        let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
        let mut v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
        for t in v["trials"].as_array_mut().unwrap() {
            tweak(t, &*f);
        }
        let (rec, _) = evaluate(&w.s, &v).unwrap();
        for a in &rec.arms {
            assert_eq!(
                a.verified_pass, 0,
                "{name}: a failing context produced a pass"
            );
            assert!(
                a.trials
                    .iter()
                    .all(|t| t.reason.contains("context not admissible")),
                "{name}: {:?}",
                a.trials
            );
        }
    }
    // No observers configured at all: fail closed.
    let w = world();
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_observers.clear();
    w.s.write_config(&cfg).unwrap();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    std::thread::sleep(std::time::Duration::from_millis(3));
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (rec, _) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    assert!(rec.arms.iter().all(|a| a.verified_pass == 0));
}

/// AB9 / AB10 (independent round 3): evaluation shopping. A REJECTed
/// experiment cannot be re-evaluated — not with a cherry-picked subset of the
/// tasks the candidate passed (AB9), not by re-rolling trial ids (AB10) — and
/// the REJECT stands: nothing can activate the candidate.
#[test]
fn ab9_ab10_one_evaluation_per_experiment_the_reject_stands() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 4, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 4, 4, 2, Some(100), Some(50));
    let (_, e1) = evaluate(
        &w.s,
        &evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default()),
    )
    .unwrap();
    let (rec, a1) = admit(&w.s, "exp", &e1, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, Decision::Reject);
    // AB9: only the tasks the candidate passed
    let cherry: Vec<_> = specs
        .iter()
        .filter(|s| s.2 == "task-0" || s.2 == "task-1")
        .cloned()
        .collect();
    let e = refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &cherry, &EvlOpts::default()),
        )
    });
    assert!(
        e.to_string()
            .contains("one evaluation per frozen experiment"),
        "{e}"
    );
    // AB10: the full manifest again, fresh trial ids, candidate now 4/4
    let mut reroll = pair(&w.inc, &w.cand, 4, 4, 4, Some(100), Some(50));
    for s in reroll.iter_mut() {
        s.3 = format!("r{}", s.3);
    }
    refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &reroll, &EvlOpts::default()),
        )
    });
    // the REJECT stands
    refused_unchanged(&w, || {
        pointer::transition(
            &w.s,
            &t(transition(
                "ab9",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&a1),
                false,
            )),
        )
    });
}

/// AB9 (first evaluation): even the FIRST evaluation must cover exactly the
/// frozen manifest × both arms × `repetitions`; missing deliveries are kept
/// as `missing`, never dropped.
#[test]
fn ab9_the_single_evaluation_covers_exactly_the_manifest() {
    let w = world();
    freeze_plan_n(&w.s, "exp", &w.inc_ref, &w.cand_ref, 4, |_| {}).unwrap();
    let full = pair(&w.inc, &w.cand, 4, 4, 2, Some(100), Some(50));
    // subset of tasks
    let subset: Vec<_> = full.iter().filter(|s| s.2 != "task-3").cloned().collect();
    let e = refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &subset, &EvlOpts::default()),
        )
    });
    assert!(e.to_string().contains("cover exactly the manifest"), "{e}");
    // an extra task
    let mut extra = full.clone();
    extra.push((
        "challenger-1",
        &w.cand,
        "task-9".into(),
        "c9".into(),
        Out::Pass,
        Some(50),
    ));
    refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &extra, &EvlOpts::default()),
        )
    });
    // a repeated task (repetitions = 1)
    let mut rep = full.clone();
    rep.push((
        "challenger-1",
        &w.cand,
        "task-0".into(),
        "c0b".into(),
        Out::Pass,
        Some(50),
    ));
    let e = refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp", &w.inc, &w.cand, &rep, &EvlOpts::default()),
        )
    });
    assert!(e.to_string().contains("repetitions"), "{e}");
    // undelivered trials are fine and counted as missing
    let o = EvlOpts {
        deliver: Box::new(|t| t != "c3"),
        ..Default::default()
    };
    let (rec, _) = evaluate(&w.s, &evl_request("exp", &w.inc, &w.cand, &full, &o)).unwrap();
    assert_eq!(rec.arm_for_policy(&w.cand_ref).unwrap().missing, 1);
}

/// AB10: trial ids are unique across the scope's evaluations — an old trial
/// id cannot be replayed into a new experiment.
#[test]
fn ab10_trial_ids_never_reused_across_experiments() {
    let w = world();
    let _ = accepted(&w, "exp"); // uses trial ids i0,i1,c0,c1
    let (c2, c2ref) = propose(&w.s, &w.inc, 9, "cand-2");
    freeze_plan(&w.s, "exp2", &w.inc_ref, &c2ref, |v| {
        v["candidate_budget"] = json!(2)
    })
    .unwrap();
    let specs = pair(&w.inc, &c2, 2, 2, 2, Some(100), Some(50));
    let e = refused_unchanged(&w, || {
        evaluate(
            &w.s,
            &evl_request("exp2", &w.inc, &c2, &specs, &EvlOpts::default()),
        )
    });
    assert!(
        e.to_string()
            .contains("unique for the experiment's lifetime"),
        "{e}"
    );
}

/// AB9 (freeze): a plan cannot be frozen without its registered task list.
#[test]
fn ab9_freeze_requires_the_registered_manifest() {
    let w = world();
    let mut v = complete_plan("nomanifest", &w.inc_ref, &w.cand_ref);
    v["task_manifest_ref"] = json!(r('1'));
    plan::register(&w.s, &PilotPlan::from_value(&v).unwrap()).unwrap();
    let e = refused_unchanged(&w, || plan::freeze(&w.s, "nomanifest"));
    assert!(
        e.to_string().contains("not a registered task manifest"),
        "{e}"
    );
}

/// R3 (independent round 3, best effort): a store that ever had a ledger keeps
/// `ledger.anchor`; deleting the ledger, head and EVERY dependent directory no
/// longer silently reissues epoch 1 — unless the anchor is deleted too, which
/// is out of model (documented in ledger.rs). A ledger swapped for another
/// store's is also detected by the anchor.
#[test]
fn r3_anchor_prevents_silent_epoch_reissue() {
    let w = world();
    let root = w.s.root().to_path_buf();
    assert!(root.join("ledger.anchor").exists());
    std::fs::remove_file(root.join("ledger.jsonl")).unwrap();
    std::fs::remove_file(root.join("ledger.head")).unwrap();
    for d in [
        "plans",
        "admissions",
        "evaluations",
        "baselines",
        "scopes",
        "episodes",
        "contexts",
        "candidate-sets",
        "task-manifests",
    ] {
        let _ = std::fs::remove_dir_all(root.join(d));
    }
    let e = pointer::load(&w.s, &scope()).unwrap_err();
    assert!(
        e.exit_code() == 2 && e.to_string().contains("ledger.anchor"),
        "{e}"
    );

    // a ledger replaced by another store's (both files, consistent) is caught
    let w1 = world();
    let other = tempfile::tempdir().unwrap();
    let s2 = store_with_config(other.path());
    for f in ["ledger.jsonl", "ledger.head"] {
        std::fs::copy(s2.root().join(f), w1.s.root().join(f)).unwrap();
    }
    let e = pointer::load(&w1.s, &scope()).unwrap_err();
    assert_eq!(e.exit_code(), 2, "{e}");
}
