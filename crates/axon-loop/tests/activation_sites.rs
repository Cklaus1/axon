//! Amendment 64 (C9 round 4b, integrate-2): `check_activate`'s admission-route
//! refusals in `pointer.rs` (route 2: a comparative admission, from an ACTIVE
//! incumbent), each judged on the production route — the public
//! `pointer::transition` every caller (the CLI's `pointer transition` verb
//! included) goes through, with a GENUINE admission (`admit`, re-derived by
//! the pointer from its frozen plan and journalled evaluation).
//!
//! Each test is an ATTACK — an input exactly ONE check refuses, so with that
//! check removed the transition is applied — with its CONTROL, the honest
//! transition that is applied. The one retired site (the admission's scope,
//! M1387) accepts any of the three reasons its four cells name. Every refusal
//! is also asserted to change no byte of the store.

mod common;
use axon_loop::admission::Decision;
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

fn go(s: &Store, v: &Value) -> Result<pointer::PointerRecord, LoopError> {
    pointer::transition(s, &tparse(v))
}

fn activate(id: &str, expected: &Ref, target: &Ref, epoch: u64, adm: &Ref, mech: bool) -> Value {
    transition(
        id,
        "activate",
        expected,
        Some(target),
        epoch,
        Some(adm),
        mech,
    )
}

/// Freeze `exp` (with `edit` applied to the plan), evaluate the incumbent
/// `inc` against `cand` at `epoch` with trials of `role`, and admit it as
/// `mech`. Returns the admission ref; the admission is a real ACCEPT.
#[allow(clippy::too_many_arguments)]
fn admitted(
    w: &World,
    exp: &str,
    inc: &PolicyEnvelope,
    cand: &PolicyEnvelope,
    epoch: u64,
    role: CorpusRole,
    mech: bool,
    edit: impl FnOnce(&mut Value),
) -> Ref {
    let (inc_ref, cand_ref) = (digest(inc).unwrap(), digest(cand).unwrap());
    freeze_plan(&w.s, exp, &inc_ref, &cand_ref, edit).unwrap();
    let specs = pair(inc, cand, 2, 2, 2, Some(100), Some(50));
    let o = EvlOpts {
        epoch,
        role,
        ..Default::default()
    };
    let (_, e) = evaluate(&w.s, &evl_request(exp, inc, cand, &specs, &o)).unwrap();
    let (rec, adm) = admit(&w.s, exp, &e, ADMITTER, mech).unwrap();
    assert_eq!(
        rec.decision,
        Decision::Accept,
        "precondition: a real ACCEPT {:?}",
        rec.reasons
    );
    assert_eq!(rec.evaluated_at_epoch.get(), epoch, "precondition: epoch");
    adm
}

/// The honest activation: the world's candidate on its own admission, at the
/// current epoch, against the active incumbent.
fn control_activates(w: &World, adm: &Ref, id: &str, mech: bool) {
    let p = go(&w.s, &activate(id, &w.inc_ref, &w.cand_ref, 1, adm, mech))
        .unwrap_or_else(|e| panic!("CONTROL: the honest activation was refused: {e}"));
    assert_eq!(p.active_policy_ref.as_ref(), Some(&w.cand_ref));
}

/// M1386: an admission activates only the policy it admitted. The admission
/// is a genuine ACCEPT of `cand`; the transition names another stored policy
/// of the same scope, within the registered candidate list, never evaluated.
#[test]
fn an_admission_never_activates_a_policy_it_did_not_admit() {
    let w = world();
    let adm = accepted(&w, "exp");
    let other = policy("never-evaluated", &["read", "search"]);
    let other_ref = w.s.put_cas("policies", &other).unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &activate("a1", &w.inc_ref, &other_ref, 1, &adm, false),
            )
        },
        &["admits"],
        "a policy its admission never admitted was activated",
    );
    control_activates(&w, &adm, "a2", false);
}

fn other_scope() -> Scope {
    Scope {
        tenant_id: TenantId::new("other-tenant").unwrap(),
        task_family: TaskFamily::new("fixture-coding").unwrap(),
    }
}

/// M1387 (retired EQUIVALENT, set with M1388 and M1334): another scope's
/// admission never activates its candidate here. Scope B has its own
/// registered candidate list, incumbent-of-record and active policy at the
/// same epoch as scope A's evaluation; the transition, in B, cites A's
/// genuine ACCEPT of A's candidate. The admission's scope (M1387), the
/// incumbent it was compared against (H1, M1388) and the target envelope's
/// scope (M1334) each refuse it.
#[test]
fn an_admission_of_another_scope_never_activates_its_candidate_here() {
    let w = world();
    let adm = accepted(&w, "exp");
    // Scope B: a registered candidate list, a policy, its incumbent-of-record,
    // active at epoch 1.
    let mut doc = candidate_set_doc(&candidate_list());
    doc["scope"] = json!(other_scope());
    let c = axon_loop::candidates::CandidateSet::parse(&doc.to_string()).unwrap();
    axon_loop::candidates::put(&w.s, &c).unwrap();
    let mut p = policy("elsewhere", &["read", "search"]);
    p.scope = other_scope();
    let p_ref = w.s.put_cas("policies", &p).unwrap();
    let mut b = baseline_doc(&p_ref);
    b["scope"] = json!(other_scope());
    let b_ref =
        pointer::designate_baseline(&w.s, &pointer::parse_baseline(&b.to_string()).unwrap())
            .unwrap();
    let mut boot = transition(
        "boot-b",
        "activate",
        &null_policy_ref(),
        Some(&p_ref),
        0,
        Some(&b_ref),
        false,
    );
    boot["scope"] = json!(other_scope());
    go(&w.s, &boot).unwrap();
    let mut t = activate("x1", &p_ref, &w.cand_ref, 1, &adm, false);
    t["scope"] = json!(other_scope());
    refused(
        w.dir.path(),
        || go(&w.s, &t),
        &[
            "admission is for a different scope",
            "(H1)",
            "target envelope is for another scope",
        ],
        "another scope's admission made its candidate active here",
    );
    control_activates(&w, &adm, "a2", false);
}

/// M1388 (H1): a genuine ACCEPT whose incumbent is NOT the active policy never
/// displaces the active policy. `parked` is a stored, never-active policy;
/// its EVO child is evaluated against it at the current epoch and ACCEPTed.
#[test]
fn an_admission_against_a_parked_incumbent_never_displaces_the_active_policy() {
    let w = world();
    let parked = policy("parked", &["read", "search"]);
    w.s.put_cas("policies", &parked).unwrap();
    let (c, c_ref) = propose(&w.s, &parked, 5, "cand-of-parked");
    let adm = admitted(
        &w,
        "h1",
        &parked,
        &c,
        1,
        CorpusRole::Confirmation,
        false,
        |_| {},
    );
    refused(
        w.dir.path(),
        || go(&w.s, &activate("a1", &w.inc_ref, &c_ref, 1, &adm, false)),
        &["(H1)"],
        "an admission against a parked incumbent displaced the active policy",
    );
    // Control: an admission against the ACTIVE incumbent applies.
    let w2 = world();
    let adm2 = accepted(&w2, "exp");
    control_activates(&w2, &adm2, "a2", false);
}

/// M1389 (K1): evidence evaluated at an older epoch never activates. The
/// admission is a genuine ACCEPT against the active incumbent at epoch 1; the
/// scope is then paused and its incumbent-of-record restored (epoch 3), so the
/// incumbent is active again but the authority has moved.
#[test]
fn an_admission_evaluated_at_an_older_epoch_never_activates() {
    let w = world();
    let adm = accepted(&w, "exp");
    go(
        &w.s,
        &transition("p", "pause", &w.inc_ref, None, 1, None, false),
    )
    .unwrap();
    go(
        &w.s,
        &transition(
            "rb",
            "rollback",
            &null_policy_ref(),
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        ),
    )
    .unwrap();
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &activate("a1", &w.inc_ref, &w.cand_ref, 3, &adm, false),
            )
        },
        &["(K1)"],
        "evidence evaluated at an older epoch activated a policy",
    );
    // Control: the same admission at the epoch it was evaluated at applies.
    let w2 = world();
    let adm2 = accepted(&w2, "exp");
    control_activates(&w2, &adm2, "a2", false);
}

/// M1390: fixture (mechanism-test) evidence never activates a policy as a REAL
/// one. The admission is a genuine ACCEPT of mechanism-test trials, admitted
/// as a mechanism test; the transition drops the label.
#[test]
fn fixture_evidence_never_activates_a_policy_as_a_real_one() {
    let w = world();
    let (inc, cand) = (w.inc.clone(), w.cand.clone());
    let adm = admitted(
        &w,
        "mech",
        &inc,
        &cand,
        1,
        CorpusRole::MechanismTest,
        true,
        |_| {},
    );
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &activate("a1", &w.inc_ref, &w.cand_ref, 1, &adm, false),
            )
        },
        &["mechanism_test label differs"],
        "mechanism-test evidence activated a policy as a real one",
    );
    // Control: the same admission, its label kept, is applied (as a fixture).
    control_activates(&w, &adm, "a2", true);
}

/// M1391: a plan frozen with deployment disabled never activates its
/// candidate, however its evaluation came out.
#[test]
fn a_plan_with_deployment_disabled_never_activates_its_candidate() {
    let w = world();
    let (inc, cand) = (w.inc.clone(), w.cand.clone());
    let adm = admitted(
        &w,
        "nodeploy",
        &inc,
        &cand,
        1,
        CorpusRole::Confirmation,
        false,
        |v| v["deployment_enabled"] = json!(false),
    );
    refused(
        w.dir.path(),
        || {
            go(
                &w.s,
                &activate("a1", &w.inc_ref, &w.cand_ref, 1, &adm, false),
            )
        },
        &["deployment_enabled = false"],
        "a plan with deployment disabled made its candidate active",
    );
    let w2 = world();
    let adm2 = accepted(&w2, "exp");
    control_activates(&w2, &adm2, "a2", false);
}
