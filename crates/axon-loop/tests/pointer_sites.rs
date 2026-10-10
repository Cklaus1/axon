//! Amendment 64 (C9 round 4b, integrate-C): the pointer's refusal sites
//! (`pointer.rs`), each judged on the production route — the public
//! `designate_baseline`, `transition`, `revoke` and `resolve` operations every
//! caller (the CLI verbs included) goes through.
//!
//! Each test is an ATTACK — an input exactly ONE check refuses, so with that
//! check removed the operation goes through — with its CONTROL. Where two
//! checks refuse the same input (a four-cell retirement) the test accepts
//! either reason; only the operation going through is the failure. Every
//! refusal is also asserted to change no byte of the store.

mod common;
use axon_loop::error::LoopError;
use axon_loop::{null_policy_ref, pointer, Store};
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};

/// `f` is refused for one of `why`, writing nothing; ATTACK if it went through.
fn refused<T: std::fmt::Debug>(
    dir: &std::path::Path,
    f: impl FnOnce() -> Result<T, LoopError>,
    why: &[&str],
    attack: &str,
) {
    let before = snapshot(dir);
    match f() {
        Ok(v) => panic!("ATTACK: {attack} ({v:?})"),
        Err(e) => assert!(
            why.iter().any(|y| e.to_string().contains(y)),
            "{attack}: expected one of {why:?}: {e}"
        ),
    }
    assert_eq!(
        snapshot(dir),
        before,
        "{attack}: the refusal changed the store"
    );
}

#[allow(clippy::too_many_arguments)]
fn tr(
    id: &str,
    kind: &str,
    expected: &Ref,
    target: Option<&Ref>,
    epoch: u64,
    adm: Option<&Ref>,
    mech: bool,
    issuer: &str,
) -> PolicyTransition {
    let mut v = transition(id, kind, expected, target, epoch, adm, mech);
    v["issuer_ref"] = json!(issuer);
    tparse(&v)
}

fn go(s: &Store, t: &PolicyTransition) -> Result<pointer::PointerRecord, LoopError> {
    pointer::transition(s, t)
}

fn baseline(s: &Store, v: &Value) -> Result<Ref, LoopError> {
    pointer::designate_baseline(s, &pointer::parse_baseline(&v.to_string())?)
}

fn baseline_by(p: &Ref, issuer: &str) -> Value {
    let mut b = baseline_doc(p);
    b["issuer_ref"] = json!(issuer);
    b
}

fn admitter() -> OpaqueRef {
    OpaqueRef::new(ADMITTER).unwrap()
}

/// A configured store holding the incumbent, with no baseline yet.
fn fresh() -> (tempfile::TempDir, Store, Ref) {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc_ref = s.put_cas("policies", &incumbent()).unwrap();
    (d, s, inc_ref)
}

fn add_admitter(s: &Store, who: &str) {
    let mut cfg = s.config().unwrap();
    cfg.trusted_admitters.push(OpaqueRef::new(who).unwrap());
    s.write_config(&cfg).unwrap();
}

/// The world with the candidate activated on its admission (epoch 2).
fn cand_active() -> (World, Ref) {
    let w = world();
    let adm = accepted(&w, "exp");
    go(
        &w.s,
        &tr(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
            ADMITTER,
        ),
    )
    .unwrap();
    (w, adm)
}

/// The world paused at epoch 2 (the incumbent-of-record retired).
fn paused() -> World {
    let w = world();
    go(
        &w.s,
        &tr("p", "pause", &w.inc_ref, None, 1, None, false, ADMITTER),
    )
    .unwrap();
    w
}

fn other_scope() -> Scope {
    Scope {
        tenant_id: TenantId::new("other-tenant").unwrap(),
        task_family: TaskFamily::new("fixture-coding").unwrap(),
    }
}

/// A policy of ANOTHER scope, whose candidate list is genuinely registered for
/// that scope (so only the scope comparison can refuse it).
fn other_scope_policy(s: &Store) -> Ref {
    let mut doc = candidate_set_doc(&candidate_list());
    doc["scope"] = json!(other_scope());
    let c = axon_loop::candidates::CandidateSet::parse(&doc.to_string()).unwrap();
    axon_loop::candidates::put(s, &c).unwrap();
    let mut p = policy("elsewhere", &["read", "search"]);
    p.scope = other_scope();
    s.put_cas("policies", &p).unwrap()
}

// ── revoke ─────────────────────────────────────────────────────────────────

/// M1318: only a trusted admitter revokes.
#[test]
fn an_untrusted_issuer_never_revokes() {
    let w = world();
    refused(
        w.dir.path(),
        || {
            pointer::revoke(
                &w.s,
                &scope(),
                &w.inc_ref,
                &r('e'),
                &OpaqueRef::new("op:stranger").unwrap(),
            )
        },
        &["is not a trusted admitter"],
        "an untrusted issuer revoked the active policy",
    );
    pointer::revoke(&w.s, &scope(), &w.inc_ref, &r('e'), &admitter())
        .expect("control: a trusted admitter revokes");
}

// ── designate_baseline ─────────────────────────────────────────────────────

/// M1319: only a trusted admitter designates the incumbent-of-record.
#[test]
fn an_untrusted_issuer_never_designates_the_incumbent_of_record() {
    let (d, s, inc_ref) = fresh();
    refused(
        d.path(),
        || baseline(&s, &baseline_by(&inc_ref, "op:stranger")),
        &["is not a trusted admitter"],
        "an untrusted issuer designated the incumbent-of-record",
    );
    baseline(&s, &baseline_doc(&inc_ref)).expect("control");
}

/// M1320: an EVO proposer in the scope never designates the incumbent-of-record,
/// even listed as an admitter.
#[test]
fn an_evo_proposer_never_designates_the_incumbent_of_record() {
    let (d, s, inc_ref) = fresh();
    add_admitter(&s, PROPOSER);
    propose(&s, &incumbent(), 1, "c");
    refused(
        d.path(),
        || baseline(&s, &baseline_by(&inc_ref, PROPOSER)),
        &["is an EVO proposer (the ranker) in this scope"],
        "an EVO proposer designated the incumbent-of-record",
    );
    baseline(&s, &baseline_doc(&inc_ref)).expect("control: an independent admitter");
}

/// M1321: a scope has ONE incumbent-of-record.
#[test]
fn a_second_incumbent_of_record_is_never_designated() {
    let w = world();
    let other =
        w.s.put_cas("policies", &policy("other", &["read"]))
            .unwrap();
    refused(
        w.dir.path(),
        || baseline(&w.s, &baseline_doc(&other)),
        &["already has an incumbent-of-record"],
        "a second incumbent-of-record replaced the scope's",
    );
    assert_eq!(
        baseline(&w.s, &baseline_doc(&w.inc_ref)).unwrap(),
        w.baseline,
        "control: the same designation is idempotent"
    );
}

/// M1322: the incumbent-of-record's policy is of the baseline's scope.
#[test]
fn a_policy_of_another_scope_is_never_the_incumbent_of_record() {
    let (d, s, inc_ref) = fresh();
    let elsewhere = other_scope_policy(&s);
    refused(
        d.path(),
        || baseline(&s, &baseline_doc(&elsewhere)),
        &["baseline policy is for another scope"],
        "a policy of another scope was designated incumbent-of-record",
    );
    baseline(&s, &baseline_doc(&inc_ref)).expect("control");
}

/// M1323: an EVO candidate is admitted, never designated.
#[test]
fn an_evo_candidate_is_never_the_incumbent_of_record() {
    let (d, s, _) = fresh();
    let (_, cref) = propose(&s, &incumbent(), 1, "c");
    refused(
        d.path(),
        || baseline(&s, &baseline_doc(&cref)),
        &["an EVO candidate cannot be an incumbent-of-record"],
        "an EVO candidate was designated incumbent-of-record",
    );
}

/// M1324: a revoked policy is never designated.
#[test]
fn a_revoked_policy_is_never_the_incumbent_of_record() {
    let (d, s, inc_ref) = fresh();
    pointer::revoke(&s, &scope(), &inc_ref, &r('e'), &admitter()).unwrap();
    refused(
        d.path(),
        || baseline(&s, &baseline_doc(&inc_ref)),
        &["baseline policy is revoked"],
        "a revoked policy was designated incumbent-of-record",
    );
}

// ── resolve ────────────────────────────────────────────────────────────────

/// M1325: a revoked ACTIVE policy is never pinned by a new task.
#[test]
fn a_revoked_active_policy_is_never_resolved() {
    let w = world();
    pointer::resolve(&w.s, &scope()).expect("control: the active policy resolves");
    pointer::revoke(&w.s, &scope(), &w.inc_ref, &r('e'), &admitter()).unwrap();
    match pointer::resolve(&w.s, &scope()) {
        Ok(r) => panic!(
            "ATTACK: a new task pinned a revoked active policy ({})",
            r.pin.version.digest
        ),
        Err(e) => assert!(e.to_string().contains("is revoked"), "{e}"),
    }
}

// ── transition: the fence ──────────────────────────────────────────────────

/// M1326: a transition id is used for one content only.
#[test]
fn a_transition_id_is_never_reused_for_other_content() {
    let w = world();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr("boot", "pause", &w.inc_ref, None, 1, None, false, ADMITTER),
            )
        },
        &["was already used for different content"],
        "a transition id reused for different content was applied",
    );
}

/// M1327: only a trusted admitter issues a transition.
#[test]
fn an_untrusted_issuer_never_moves_the_pointer() {
    let w = world();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "p",
                    "pause",
                    &w.inc_ref,
                    None,
                    1,
                    None,
                    false,
                    "op:stranger",
                ),
            )
        },
        &["is not in the trusted-admitter set"],
        "an untrusted issuer paused the scope",
    );
    go(
        &w.s,
        &tr("p", "pause", &w.inc_ref, None, 1, None, false, ADMITTER),
    )
    .expect("control");
}

/// M1328: a transition issuer holds no other loop role (Compute Fabric here).
#[test]
fn a_trusted_verifier_never_moves_the_pointer() {
    let w = world();
    add_admitter(&w.s, VERIFIER);
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr("p", "pause", &w.inc_ref, None, 1, None, false, VERIFIER),
            )
        },
        &["self-promotion: issuer"],
        "Compute Fabric (a trusted verifier listed as admitter) paused the scope",
    );
}

/// M1329 / M1330 (retired EQUIVALENT, each other's pair): a transition built on
/// a stale epoch. `next_epoch == expected_epoch + 1` is a parse rule, so the
/// stale-epoch check and the next-epoch check refuse exactly the same inputs.
#[test]
fn a_transition_on_a_stale_epoch_is_refused() {
    let w = world();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr("p", "pause", &w.inc_ref, None, 0, None, false, ADMITTER),
            )
        },
        &["stale epoch", "next_epoch must be expected_epoch + 1"],
        "a transition built on a stale epoch was applied",
    );
}

/// M1331: a transition names the policy actually active.
#[test]
fn a_transition_expecting_another_policy_is_refused() {
    let w = world();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr("p", "pause", &w.cand_ref, None, 1, None, false, ADMITTER),
            )
        },
        &["wrong expected policy"],
        "a transition expecting a policy that is not active was applied",
    );
}

// ── transition: activate / rollback ───────────────────────────────────────

/// M1332: the active policy is never activated again (here by a rollback to
/// itself, which every other rollback rule accepts).
#[test]
fn the_active_policy_is_never_activated_again() {
    let (w, _) = cand_active();
    let b = w.baseline.clone();
    go(
        &w.s,
        &tr(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&b),
            false,
            ADMITTER,
        ),
    )
    .unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "rb2",
                    "rollback",
                    &w.inc_ref,
                    Some(&w.inc_ref),
                    3,
                    Some(&b),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["is already the active policy"],
        "the active policy was activated again over itself",
    );
}

/// M1333: a revoked policy is never made active.
#[test]
fn a_revoked_candidate_is_never_activated() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::revoke(&w.s, &scope(), &w.cand_ref, &r('e'), &admitter()).unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "a1",
                    "activate",
                    &w.inc_ref,
                    Some(&w.cand_ref),
                    1,
                    Some(&adm),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["is revoked"],
        "a revoked candidate was activated",
    );
}

/// M1334 (retired EQUIVALENT, pair with M1322): a policy of another scope made
/// active through the incumbent-of-record route. Its designation (M1322) and
/// the activation's envelope check each refuse it.
#[test]
fn a_policy_of_another_scope_is_never_activated() {
    let (d, s, _) = fresh();
    let elsewhere = other_scope_policy(&s);
    let why = [
        "baseline policy is for another scope",
        "target envelope is for another scope",
    ];
    let b = match baseline(&s, &baseline_doc(&elsewhere)) {
        Ok(b) => b,
        Err(e) => {
            assert!(why.iter().any(|y| e.to_string().contains(y)), "{e}");
            return;
        }
    };
    refused(
        d.path(),
        || {
            go(
                &s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&elsewhere),
                    0,
                    Some(&b),
                    false,
                    ADMITTER,
                ),
            )
        },
        &why,
        "a policy of another scope was made active",
    );
}

/// M1335: the incumbent-of-record's issuer is independent NOW: it has since
/// become Compute Fabric, so its baseline is not activated.
#[test]
fn a_baseline_whose_issuer_became_a_verifier_is_never_activated() {
    let w = paused();
    add_admitter(&w.s, "op:admitter2");
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_verifiers.push(admitter());
    w.s.write_config(&cfg).unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&w.inc_ref),
                    2,
                    Some(&w.baseline),
                    false,
                    "op:admitter2",
                ),
            )
        },
        &["is now also"],
        "a baseline whose issuer is now Compute Fabric was activated",
    );
}

/// M1336: the incumbent-of-record's issuer is no EVO proposer in the scope NOW.
#[test]
fn a_baseline_whose_issuer_became_a_proposer_is_never_activated() {
    let w = paused();
    add_admitter(&w.s, "op:admitter2");
    let mut req = evo_request(
        &w.inc,
        9,
        "by-admitter",
        vec![discovery_episode(&w.inc, "d-x")],
    );
    req["proposer_ref"] = json!(ADMITTER);
    axon_loop::evo::propose(
        &w.s,
        &axon_loop::evo::parse_request(&req.to_string()).unwrap(),
    )
    .unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&w.inc_ref),
                    2,
                    Some(&w.baseline),
                    false,
                    "op:admitter2",
                ),
            )
        },
        &["is an EVO proposer in this scope"],
        "a baseline whose issuer is now an EVO proposer was activated",
    );
}

/// M1337: a rollback cites the authority its predecessor was active under.
#[test]
fn a_rollback_cites_its_predecessors_own_authority() {
    let (w, adm) = cand_active();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "rb",
                    "rollback",
                    &w.cand_ref,
                    Some(&w.inc_ref),
                    2,
                    Some(&adm),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["rollback must cite the admission/baseline the predecessor was active under"],
        "a rollback citing another authority than its predecessor's was applied",
    );
}

/// M1338: a rollback keeps its predecessor's mechanism-test label.
#[test]
fn a_rollback_keeps_its_predecessors_label() {
    let (w, _) = cand_active();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "rb",
                    "rollback",
                    &w.cand_ref,
                    Some(&w.inc_ref),
                    2,
                    Some(&w.baseline),
                    true,
                    ADMITTER,
                ),
            )
        },
        &["mechanism_test label differs from the predecessor's activation"],
        "a rollback relabelled its predecessor as a mechanism test",
    );
}

/// M1339: a violation recorded against the candidate's trial AFTER its
/// admission blocks its activation.
#[test]
fn a_candidate_reported_unsafe_after_admission_is_never_activated() {
    let w = world();
    freeze_plan(&w.s, "exp", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let v = evl_request("exp", &w.inc, &w.cand, &specs, &EvlOpts::default());
    let (_, e) = evaluate(&w.s, &v).unwrap();
    let (rec, adm) = admit(&w.s, "exp", &e, ADMITTER, false).unwrap();
    assert_eq!(rec.decision, axon_loop::admission::Decision::Accept);
    let t = v["trials"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["episode"]["identity"]["arm_id"] == "challenger-1")
        .unwrap()
        .clone();
    let rep = safety_report(&t, "violation", Some("policy_violation"), OBSERVER);
    axon_loop::safety::report(&w.s, &rep.to_string(), None).unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "a1",
                    "activate",
                    &w.inc_ref,
                    Some(&w.cand_ref),
                    1,
                    Some(&adm),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["after the evaluation: the candidate cannot be made active"],
        "a candidate reported unsafe after its admission was activated",
    );
}

/// M1340: the incumbent-of-record is activated only from paused.
#[test]
fn the_incumbent_of_record_is_activated_only_from_paused() {
    let (w, _) = cand_active();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr("b", "activate", &w.cand_ref, Some(&w.inc_ref), 2, Some(&w.baseline), false, ADMITTER),
            )
        },
        &["activated only from paused"],
        "the incumbent-of-record was activated over an active policy (a rollback without its rules)",
    );
}

/// M1341: the incumbent-of-record route activates the baseline's own policy.
#[test]
fn the_baseline_never_activates_another_policy() {
    let w = paused();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&w.cand_ref),
                    2,
                    Some(&w.baseline),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["baseline names a different policy"],
        "an unadmitted candidate was activated on the incumbent-of-record's baseline",
    );
}

/// M1342: the incumbent-of-record is never a mechanism-test activation.
#[test]
fn the_incumbent_of_record_is_never_a_mechanism_test() {
    let w = paused();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&w.inc_ref),
                    2,
                    Some(&w.baseline),
                    true,
                    ADMITTER,
                ),
            )
        },
        &["an incumbent-of-record is not a mechanism test"],
        "the incumbent-of-record was activated as a mechanism test",
    );
}

/// M1343: the incumbent-of-record's issuer is still trusted.
#[test]
fn a_baseline_whose_issuer_is_no_longer_trusted_is_never_activated() {
    let w = paused();
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters = vec![OpaqueRef::new("op:admitter2").unwrap()];
    w.s.write_config(&cfg).unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "b",
                    "activate",
                    &null_policy_ref(),
                    Some(&w.inc_ref),
                    2,
                    Some(&w.baseline),
                    false,
                    "op:admitter2",
                ),
            )
        },
        &["baseline issuer is no longer trusted"],
        "a baseline whose issuer is no longer trusted was activated",
    );
}

/// M1344: a mechanism-test fixture is no incumbent for a REAL activation.
#[test]
fn a_mechanism_test_fixture_is_no_incumbent_for_a_real_activation() {
    let w = world();
    freeze_plan(&w.s, "mech", &w.inc_ref, &w.cand_ref, |_| {}).unwrap();
    let specs = pair(&w.inc, &w.cand, 2, 2, 2, Some(100), Some(50));
    let (_, e) = evaluate(
        &w.s,
        &evl_request(
            "mech",
            &w.inc,
            &w.cand,
            &specs,
            &EvlOpts {
                role: CorpusRole::MechanismTest,
                ..Default::default()
            },
        ),
    )
    .unwrap();
    let (_, madm) = admit(&w.s, "mech", &e, ADMITTER, true).unwrap();
    go(
        &w.s,
        &tr(
            "m1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&madm),
            true,
            ADMITTER,
        ),
    )
    .unwrap();
    // A real experiment against the fixture, at the current epoch.
    let (c2, c2_ref) = propose(&w.s, &w.cand, 11, "cand-2");
    freeze_plan(&w.s, "real", &w.cand_ref, &c2_ref, |_| {}).unwrap();
    let mut specs = arm("incumbent", &w.cand, 2, "ri", 2, Some(100));
    specs.extend(arm("challenger-1", &c2, 2, "rc", 2, Some(50)));
    let opts = EvlOpts {
        epoch: 2,
        ..Default::default()
    };
    let (_, e2) = evaluate(&w.s, &evl_request("real", &w.cand, &c2, &specs, &opts)).unwrap();
    let (rec, adm2) = admit(&w.s, "real", &e2, ADMITTER, false).unwrap();
    assert_eq!(
        rec.decision,
        axon_loop::admission::Decision::Accept,
        "{:?}",
        rec.reasons
    );
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &tr(
                    "a2",
                    "activate",
                    &w.cand_ref,
                    Some(&c2_ref),
                    2,
                    Some(&adm2),
                    false,
                    ADMITTER,
                ),
            )
        },
        &["the active policy is a mechanism-test fixture"],
        "a real activation took a mechanism-test fixture as its incumbent",
    );
}
