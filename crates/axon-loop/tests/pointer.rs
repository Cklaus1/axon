//! B258 epoch / B278 / B279: the fenced pointer.
mod common;
use axon_loop::admission::Decision;
use axon_loop::error::LoopError;
use axon_loop::{epoch, null_policy_ref, pointer};
use axon_loop_contracts::*;
use common::*;

fn setup() -> (
    tempfile::TempDir,
    axon_loop::Store,
    PolicyEnvelope,
    Ref,
    Ref,
) {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = incumbent();
    let a = seed_admission(&s, &p, Decision::Accept, false, true);
    let pr = digest(&p).unwrap();
    (d, s, p, pr, a)
}

fn activate(
    s: &axon_loop::Store,
    id: &str,
    exp: &Ref,
    target: &Ref,
    e: u64,
    a: &Ref,
) -> Result<pointer::PointerRecord, LoopError> {
    pointer::transition(
        s,
        &tparse(&transition(
            id,
            "activate",
            exp,
            Some(target),
            e,
            Some(a),
            false,
        )),
    )
}

#[test]
fn fresh_scope_is_paused_at_epoch_zero() {
    let (_d, s, ..) = setup();
    assert_eq!(epoch::current(&s, &scope()).unwrap().get(), 0);
    assert!(matches!(
        pointer::resolve(&s, &scope()),
        Err(LoopError::Paused(_))
    ));
}

#[test]
fn activate_then_resolve_pins_exact_version_and_epoch() {
    let (_d, s, p, pr, a) = setup();
    let out = activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    assert_eq!(out.epoch.get(), 1);
    assert_eq!(out.active_policy_ref.as_ref(), Some(&pr));
    let (pin, env) = pointer::resolve(&s, &scope()).unwrap();
    assert_eq!(pin.version.digest, pr);
    assert_eq!(pin.epoch.get(), 1);
    assert_eq!(env, p);
    assert_eq!(pointer::log(&s, &scope()).unwrap().len(), 1);
    epoch::require_current(&s, &scope(), AuthorityEpoch::new(1).unwrap()).unwrap();
    assert!(matches!(
        epoch::require_current(&s, &scope(), AuthorityEpoch::new(0).unwrap()),
        Err(LoopError::Conflict(_))
    ));
}

#[test]
fn refusals_change_no_bytes() {
    let (d, s, _p, pr, a) = setup();
    activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    let other = policy("other", &["read"]);
    let oref = digest(&other).unwrap();
    let oa = seed_admission(&s, &other, Decision::Accept, false, true);
    let rej = seed_admission(
        &s,
        &policy("rej", &["search"]),
        Decision::Reject,
        false,
        true,
    );
    let rejref = digest(&policy("rej", &["search"])).unwrap();
    let nodeploy_p = policy("nodeploy", &["edit"]);
    let nodeploy = seed_admission(&s, &nodeploy_p, Decision::Accept, false, false);
    let before = snapshot(d.path());

    let cases: Vec<(serde_json::Value, &str)> = vec![
        // stale epoch
        (
            transition("x1", "activate", &pr, Some(&oref), 0, Some(&oa), false),
            "conflict",
        ),
        // wrong expected ref
        (
            transition(
                "x2",
                "activate",
                &null_policy_ref(),
                Some(&oref),
                1,
                Some(&oa),
                false,
            ),
            "conflict",
        ),
        // admission missing from the store
        (
            transition("x3", "activate", &pr, Some(&oref), 1, Some(&r('7')), false),
            "refused",
        ),
        // admission for a different target
        (
            transition("x4", "activate", &pr, Some(&oref), 1, Some(&a), false),
            "refused",
        ),
        // REJECT admission
        (
            transition("x5", "activate", &pr, Some(&rejref), 1, Some(&rej), false),
            "refused",
        ),
        // mechanism-test label laundering
        (
            transition("x6", "activate", &pr, Some(&oref), 1, Some(&oa), true),
            "refused",
        ),
        // deployment disabled by the plan
        (
            transition(
                "x7",
                "activate",
                &pr,
                Some(&digest(&nodeploy_p).unwrap()),
                1,
                Some(&nodeploy),
                false,
            ),
            "refused",
        ),
        // rollback to a policy never active here
        (
            transition("x8", "rollback", &pr, Some(&oref), 1, Some(&oa), false),
            "refused",
        ),
        // re-activating the active policy
        (
            transition("x9", "activate", &pr, Some(&pr), 1, Some(&a), false),
            "refused",
        ),
    ];
    let mut n = 0;
    for (t, want) in cases {
        let e = pointer::transition(&s, &tparse(&t)).unwrap_err();
        assert_eq!(e.kind(), want, "{t}: {e}");
        n += 1;
    }
    // untrusted issuer
    let mut t = transition("x10", "activate", &pr, Some(&oref), 1, Some(&oa), false);
    t["issuer_ref"] = serde_json::json!("agent:self-promoter");
    assert_eq!(
        pointer::transition(&s, &tparse(&t)).unwrap_err().kind(),
        "refused"
    );
    // wrong scope
    let mut t = transition("x11", "pause", &pr, None, 1, None, false);
    t["scope"]["task_family"] = serde_json::json!("other-family");
    assert!(pointer::transition(&s, &tparse(&t)).is_err());
    assert_eq!(n, 9);
    assert_eq!(
        snapshot(d.path()),
        before,
        "a refused transition changed the store"
    );
}

#[test]
fn rollback_to_valid_predecessor_and_revoked_predecessor_refused() {
    let (d, s, _p, pr, a) = setup();
    activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    let c = policy("challenger", &["read", "search"]);
    let cr = digest(&c).unwrap();
    let ca = seed_admission(&s, &c, Decision::Accept, false, true);
    activate(&s, "t2", &pr, &cr, 1, &ca).unwrap();

    // Revoke the predecessor: rollback must refuse and suggest pause.
    pointer::revoke(
        &s,
        &scope(),
        &pr,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    let before = snapshot(d.path());
    let rb = tparse(&transition(
        "t3",
        "rollback",
        &cr,
        Some(&pr),
        2,
        Some(&a),
        false,
    ));
    match pointer::transition(&s, &rb) {
        Err(LoopError::Refused(m)) => assert!(m.contains("pause"), "{m}"),
        other => panic!("{other:?}"),
    }
    assert_eq!(snapshot(d.path()), before);

    // Pause is always available to a trusted admitter.
    let p = pointer::transition(
        &s,
        &tparse(&transition("t4", "pause", &cr, None, 2, None, false)),
    )
    .unwrap();
    assert_eq!(p.epoch.get(), 3);
    assert!(p.active_policy_ref.is_none());
    assert_eq!(p.history.len(), 2);
    assert!(matches!(
        pointer::resolve(&s, &scope()),
        Err(LoopError::Paused(_))
    ));

    // Out of pause, expected is the null sentinel; rollback to the still-valid challenger.
    let out = pointer::transition(
        &s,
        &tparse(&transition(
            "t5",
            "rollback",
            &null_policy_ref(),
            Some(&cr),
            3,
            Some(&ca),
            false,
        )),
    )
    .unwrap();
    assert_eq!(out.epoch.get(), 4);
    assert_eq!(pointer::resolve(&s, &scope()).unwrap().0.version.digest, cr);
}

#[test]
fn revoked_active_policy_makes_resolve_pause() {
    let (_d, s, _p, pr, a) = setup();
    activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    // an untrusted issuer cannot revoke
    assert!(pointer::revoke(&s, &scope(), &pr, &r('e'), &OpaqueRef::new(WORKER).unwrap()).is_err());
    pointer::revoke(
        &s,
        &scope(),
        &pr,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        pointer::resolve(&s, &scope()),
        Err(LoopError::Paused(_))
    ));
}

#[test]
fn replayed_transition_id_is_idempotent_and_conflicting_reuse_refused() {
    let (d, s, _p, pr, a) = setup();
    let t = tparse(&transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&a),
        false,
    ));
    let first = pointer::transition(&s, &t).unwrap();
    let before = snapshot(d.path());
    assert_eq!(pointer::transition(&s, &t).unwrap(), first);
    assert_eq!(snapshot(d.path()), before);
    let mut v = transition("t1", "pause", &pr, None, 1, None, false);
    v["reason_ref"] = serde_json::json!(r('d'));
    assert!(matches!(
        pointer::transition(&s, &tparse(&v)),
        Err(LoopError::Conflict(_))
    ));
    assert_eq!(snapshot(d.path()), before);
}

#[test]
fn concurrent_threads_exactly_one_wins() {
    let (_d, s, _p, pr, a) = setup();
    activate(&s, "t0", &null_policy_ref(), &pr, 0, &a).unwrap();
    let mut hs = Vec::new();
    for i in 0..16 {
        let s = s.clone();
        let pr = pr.clone();
        hs.push(std::thread::spawn(move || {
            let t = tparse(&transition(
                &format!("race-{i}"),
                "pause",
                &pr,
                None,
                1,
                None,
                false,
            ));
            pointer::transition(&s, &t)
        }));
    }
    let res: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    let wins = res.iter().filter(|r| r.is_ok()).count();
    let conflicts = res
        .iter()
        .filter(|r| matches!(r, Err(LoopError::Conflict(_))))
        .count();
    assert_eq!((wins, conflicts), (1, 15));
    assert_eq!(epoch::current(&s, &scope()).unwrap().get(), 2);
    assert_eq!(pointer::log(&s, &scope()).unwrap().len(), 2);
}

#[test]
fn crash_after_journal_before_publish_rolls_forward() {
    let (d, s, _p, pr, a) = setup();
    activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    let dir = s.scope_dir(&scope());
    let before_pointer = std::fs::read(dir.join("pointer.json")).unwrap();
    pointer::transition(
        &s,
        &tparse(&transition("t2", "pause", &pr, None, 1, None, false)),
    )
    .unwrap();
    // Simulate: journal line durable, pointer.json publish lost.
    std::fs::write(dir.join("pointer.json"), &before_pointer).unwrap();
    let p = pointer::load(&s, &scope()).unwrap();
    assert_eq!(p.epoch.get(), 2);
    assert!(p.active_policy_ref.is_none());
    // A torn trailing journal line (crash mid-append) is ignored.
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("transitions.jsonl"))
        .unwrap();
    std::io::Write::write_all(&mut f, b"{\"schema\":\"axon.loop.tra").unwrap();
    assert_eq!(pointer::load(&s, &scope()).unwrap().epoch.get(), 2);
    drop(d);
}

#[test]
fn hand_edited_policy_record_is_refused() {
    let (_d, s, _p, pr, a) = setup();
    activate(&s, "t1", &null_policy_ref(), &pr, 0, &a).unwrap();
    let path = s.cas_path("policies", &pr).unwrap();
    let txt = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"edit\"", "\"write\"");
    std::fs::write(&path, txt).unwrap();
    assert!(matches!(
        pointer::resolve(&s, &scope()),
        Err(LoopError::Io(_))
    ));
}
