//! B258 epoch / B278 / B279: the fenced pointer, as a projection of the ledger.
mod common;
use axon_loop::error::LoopError;
use axon_loop::{epoch, null_policy_ref, pointer};
use axon_loop_contracts::*;
use common::*;
use serde_json::json;

fn t(v: serde_json::Value) -> PolicyTransition {
    tparse(&v)
}

#[test]
fn fresh_scope_is_paused_at_epoch_zero() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    assert_eq!(epoch::current(&s, &scope()).unwrap().get(), 0);
    assert!(matches!(
        pointer::resolve(&s, &scope()),
        Err(LoopError::Paused(_))
    ));
}

#[test]
fn baseline_activation_then_admitted_candidate() {
    let w = world();
    let r = pointer::resolve(&w.s, &scope()).unwrap();
    assert_eq!(
        (r.pin.version.digest.clone(), r.pin.epoch.get()),
        (w.inc_ref.clone(), 1)
    );
    assert!(!r.mechanism_test);
    let adm = accepted(&w, "exp");
    let out = pointer::transition(
        &w.s,
        &t(transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    assert_eq!(out.epoch.get(), 2);
    let r = pointer::resolve(&w.s, &scope()).unwrap();
    assert_eq!(r.pin.version.digest, w.cand_ref);
    assert_eq!(r.policy, w.cand);
    epoch::require_current(&w.s, &scope(), AuthorityEpoch::new(2).unwrap()).unwrap();
    assert!(matches!(
        epoch::require_current(&w.s, &scope(), AuthorityEpoch::new(1).unwrap()),
        Err(LoopError::Conflict(_))
    ));
}

#[test]
fn refusals_change_no_bytes() {
    let w = world();
    let adm = accepted(&w, "exp");
    let before = snapshot(w.dir.path());
    let cases = vec![
        (
            transition(
                "x1",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                0,
                Some(&adm),
                false,
            ),
            "conflict",
        ),
        (
            transition(
                "x2",
                "activate",
                &null_policy_ref(),
                Some(&w.cand_ref),
                1,
                Some(&adm),
                false,
            ),
            "conflict",
        ),
        (
            transition(
                "x3",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&r('7')),
                false,
            ),
            "refused",
        ),
        (
            transition(
                "x4",
                "activate",
                &w.inc_ref,
                Some(&w.inc_ref),
                1,
                Some(&adm),
                false,
            ),
            "refused",
        ),
        (
            transition(
                "x5",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                true,
            ),
            "refused",
        ),
        (
            transition(
                "x6",
                "rollback",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&adm),
                false,
            ),
            "refused",
        ),
        (
            transition(
                "x7",
                "activate",
                &w.inc_ref,
                Some(&w.cand_ref),
                1,
                Some(&w.baseline),
                false,
            ),
            "refused",
        ),
    ];
    for (v, want) in cases {
        let e = pointer::transition(&w.s, &t(v.clone())).unwrap_err();
        assert_eq!(e.kind(), want, "{v}: {e}");
    }
    let mut v = transition(
        "x8",
        "activate",
        &w.inc_ref,
        Some(&w.cand_ref),
        1,
        Some(&adm),
        false,
    );
    v["issuer_ref"] = json!("agent:self-promoter");
    assert_eq!(
        pointer::transition(&w.s, &t(v)).unwrap_err().kind(),
        "refused"
    );
    assert_eq!(
        snapshot(w.dir.path()),
        before,
        "a refused transition changed the store"
    );
}

#[test]
fn rollback_to_valid_predecessor_and_revoked_predecessor_refused() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &t(transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    // rollback must cite the baseline the incumbent was active under
    assert!(matches!(
        pointer::transition(
            &w.s,
            &t(transition(
                "rb0",
                "rollback",
                &w.cand_ref,
                Some(&w.inc_ref),
                2,
                Some(&adm),
                false
            ))
        ),
        Err(LoopError::Refused(_))
    ));
    pointer::revoke(
        &w.s,
        &scope(),
        &w.inc_ref,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    let before = snapshot(w.dir.path());
    match pointer::transition(
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
    ) {
        Err(LoopError::Refused(m)) => assert!(m.contains("pause"), "{m}"),
        o => panic!("{o:?}"),
    }
    assert_eq!(snapshot(w.dir.path()), before);
    let p = pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.cand_ref, None, 2, None, false)),
    )
    .unwrap();
    assert_eq!(
        (
            p.epoch.get(),
            p.active_policy_ref.is_none(),
            p.history.len()
        ),
        (3, true, 2)
    );
    // from paused: rollback to the still-valid candidate under its own admission
    pointer::transition(
        &w.s,
        &t(transition(
            "rb2",
            "rollback",
            &null_policy_ref(),
            Some(&w.cand_ref),
            3,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.cand_ref
    );
}

#[test]
fn good_rollback_after_regression() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &t(transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
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
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref
    );
}

#[test]
fn revoked_active_policy_makes_resolve_pause() {
    let w = world();
    assert!(pointer::revoke(
        &w.s,
        &scope(),
        &w.inc_ref,
        &r('e'),
        &OpaqueRef::new(WORKER).unwrap()
    )
    .is_err());
    pointer::revoke(
        &w.s,
        &scope(),
        &w.inc_ref,
        &r('e'),
        &OpaqueRef::new(ADMITTER).unwrap(),
    )
    .unwrap();
    assert!(matches!(
        pointer::resolve(&w.s, &scope()),
        Err(LoopError::Paused(_))
    ));
}

#[test]
fn replayed_transition_id_is_idempotent_and_conflicting_reuse_refused() {
    let w = world();
    let boot = t(transition(
        "boot",
        "activate",
        &null_policy_ref(),
        Some(&w.inc_ref),
        0,
        Some(&w.baseline),
        false,
    ));
    let before = snapshot(w.dir.path());
    assert_eq!(pointer::transition(&w.s, &boot).unwrap().epoch.get(), 1);
    assert_eq!(snapshot(w.dir.path()), before);
    let mut v = transition("boot", "pause", &w.inc_ref, None, 1, None, false);
    v["reason_ref"] = json!(r('d'));
    assert!(matches!(
        pointer::transition(&w.s, &t(v)),
        Err(LoopError::Conflict(_))
    ));
    assert_eq!(snapshot(w.dir.path()), before);
}

#[test]
fn concurrent_threads_exactly_one_wins() {
    let w = world();
    let mut hs = Vec::new();
    for i in 0..16 {
        let s = w.s.clone();
        let pr = w.inc_ref.clone();
        hs.push(std::thread::spawn(move || {
            pointer::transition(
                &s,
                &tparse(&transition(
                    &format!("race-{i}"),
                    "pause",
                    &pr,
                    None,
                    1,
                    None,
                    false,
                )),
            )
        }));
    }
    let res: Vec<_> = hs.into_iter().map(|h| h.join().unwrap()).collect();
    let wins = res.iter().filter(|r| r.is_ok()).count();
    let conflicts = res
        .iter()
        .filter(|r| matches!(r, Err(LoopError::Conflict(_))))
        .count();
    assert_eq!((wins, conflicts), (1, 15));
    assert_eq!(epoch::current(&w.s, &scope()).unwrap().get(), 2);
    assert_eq!(pointer::log(&w.s, &scope()).unwrap().len(), 2);
}

#[test]
fn crash_between_ledger_append_and_publish_rolls_forward() {
    let w = world();
    let root = w.s.root().to_path_buf();
    let head = std::fs::read(root.join("ledger.head")).unwrap();
    let ptr = std::fs::read(w.s.scope_dir(&scope()).join("pointer.json")).unwrap();
    pointer::transition(
        &w.s,
        &t(transition("p", "pause", &w.inc_ref, None, 1, None, false)),
    )
    .unwrap();
    // ledger line durable, head + projection lost
    std::fs::write(root.join("ledger.head"), &head).unwrap();
    std::fs::write(w.s.scope_dir(&scope()).join("pointer.json"), &ptr).unwrap();
    let p = pointer::load(&w.s, &scope()).unwrap();
    assert_eq!((p.epoch.get(), p.active_policy_ref.is_none()), (2, true));
    // a torn trailing ledger line is ignored
    let mut f = std::fs::OpenOptions::new()
        .append(true)
        .open(root.join("ledger.jsonl"))
        .unwrap();
    std::io::Write::write_all(&mut f, b"{\"schema\":\"axon.loop.led").unwrap();
    assert_eq!(pointer::load(&w.s, &scope()).unwrap().epoch.get(), 2);
}

#[test]
fn hand_edited_policy_record_is_refused() {
    let w = world();
    let path = w.s.cas_path("policies", &w.inc_ref).unwrap();
    let txt = std::fs::read_to_string(&path)
        .unwrap()
        .replace("\"edit\"", "\"write\"");
    std::fs::write(&path, txt).unwrap();
    assert!(matches!(
        pointer::resolve(&w.s, &scope()),
        Err(LoopError::Io(_))
    ));
}

#[test]
fn baseline_rules() {
    let w = world();
    // one per scope
    let other = policy("other", &["read"]);
    let oref = w.s.put_cas("policies", &other).unwrap();
    assert!(pointer::designate_baseline(
        &w.s,
        &pointer::parse_baseline(&baseline_doc(&oref).to_string()).unwrap()
    )
    .is_err());
    // an EVO candidate cannot be a baseline
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let (_, cref) = propose(&s, &incumbent(), 1, "c");
    assert!(matches!(
        pointer::designate_baseline(
            &s,
            &pointer::parse_baseline(&baseline_doc(&cref).to_string()).unwrap()
        ),
        Err(LoopError::Refused(_))
    ));
}

/// B280 / G16-r22-peer-failure-matrix: a peer that reconnects and REPLAYS an
/// activation it already sent — after authority has moved on — gets the
/// recorded result of that transition back, and nothing is re-activated: the
/// pointer stays where the later transition left it, byte for byte.
#[test]
fn a_replayed_activation_after_authority_moved_does_not_reactivate() {
    let w = world();
    let boot = t(transition(
        "boot",
        "activate",
        &null_policy_ref(),
        Some(&w.inc_ref),
        0,
        Some(&w.baseline),
        false,
    ));
    // Authority moves on: a pause at epoch 1 -> 2.
    let pause = t(transition(
        "pause-1", "pause", &w.inc_ref, None, 1, None, false,
    ));
    assert_eq!(pointer::transition(&w.s, &pause).unwrap().epoch.get(), 2);
    let after_pause = snapshot(w.dir.path());
    // The reconnecting peer replays its old activation.
    let replay = pointer::transition(&w.s, &boot).unwrap();
    assert_eq!(
        replay.epoch.get(),
        1,
        "the recorded result of THAT transition"
    );
    assert_eq!(
        epoch::current(&w.s, &scope()).unwrap().get(),
        2,
        "not re-activated"
    );
    assert!(matches!(
        pointer::resolve(&w.s, &scope()),
        Err(LoopError::Paused(_))
    ));
    assert_eq!(
        snapshot(w.dir.path()),
        after_pause,
        "the replay wrote nothing"
    );
    // A NEW transition built on the peer's stale view is refused.
    let stale = t(transition(
        "reactivate",
        "activate",
        &w.inc_ref,
        Some(&w.inc_ref),
        1,
        Some(&w.baseline),
        false,
    ));
    assert!(pointer::transition(&w.s, &stale).is_err());
    assert_eq!(snapshot(w.dir.path()), after_pause);
}

fn die_at_the_named_append_stage(stage: &'static str) {
    if std::env::var("LOOP_CRASH_AT").ok().as_deref() == Some(stage) {
        std::process::abort();
    }
}

/// The crash child: applies the transition in `LOOP_CRASH_T` to the store at
/// `LOOP_CRASH_STORE`, dying inside the ledger append at `LOOP_CRASH_AT`.
#[test]
#[ignore = "driven as a subprocess by the restart test below"]
fn transition_crash_child() {
    axon_loop::ledger::set_fault_hook(die_at_the_named_append_stage);
    let s = axon_loop::store::Store::open_dir(std::env::var("LOOP_CRASH_STORE").unwrap()).unwrap();
    let v: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(std::env::var("LOOP_CRASH_T").unwrap()).unwrap(),
    )
    .unwrap();
    let _ = pointer::transition(&s, &t(v));
}

/// B280 / G13-r22-restart-matrix, axon-loop side: a REAL process dies inside a
/// pointer transition — after its ledger entry is durable but before the
/// projection, and after the projection but before the head — and a real
/// restart rolls forward to exactly one applied transition. Re-sending the
/// same transition returns that result and applies nothing twice.
/// (`crash_between_ledger_append_and_publish_rolls_forward` models the same
/// window by rewriting files; this kills the process there.)
#[test]
fn a_real_crash_inside_a_transition_rolls_forward_exactly_once() {
    for stage in ["after_ledger_append", "after_projection"] {
        let w = world();
        let v = transition("p-crash", "pause", &w.inc_ref, None, 1, None, false);
        let tf = w.dir.path().join("t.json");
        std::fs::write(&tf, v.to_string()).unwrap();
        let st = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "transition_crash_child",
                "--exact",
                "--ignored",
                "--test-threads=1",
            ])
            .env("LOOP_CRASH_STORE", w.s.root())
            .env("LOOP_CRASH_T", &tf)
            .env("LOOP_CRASH_AT", stage)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(
            st.signal(),
            Some(6),
            "{stage}: the child did not die there ({st:?})"
        );
        // Restart: a fresh store handle reads what the dead process left.
        let s = axon_loop::store::Store::open_dir(w.s.root()).unwrap();
        let p = pointer::load(&s, &scope()).unwrap();
        assert_eq!(
            (p.epoch.get(), p.active_policy_ref.is_none()),
            (2, true),
            "{stage}: rolled forward"
        );
        let again = pointer::transition(&s, &t(v.clone())).unwrap();
        assert_eq!(again.epoch.get(), 2, "{stage}: the recorded result");
        let applied = std::fs::read_to_string(s.root().join("ledger.jsonl"))
            .unwrap()
            .lines()
            .filter(|l| l.contains("\"p-crash\""))
            .count();
        assert_eq!(applied, 1, "{stage}: applied exactly once");
        assert_eq!(epoch::current(&s, &scope()).unwrap().get(), 2, "{stage}");
    }
}

/// G11-r22-rollback-revalidate / independent-admission: a rollback is issued
/// by an INDEPENDENT trusted admitter. Refused, writing nothing: an issuer
/// outside the trusted-admitter set, and a trusted admitter that is also a
/// trusted verifier (Compute Fabric) — it could otherwise move the pointer it
/// produces evidence for. Honest control: the same rollback from the
/// independent admitter succeeds.
///
/// Mutation: drop the issuer role check in `pointer::transition` → red.
#[test]
fn a_rollback_needs_an_independent_trusted_issuer() {
    let w = world();
    let adm = accepted(&w, "exp");
    pointer::transition(
        &w.s,
        &t(transition(
            "a1",
            "activate",
            &w.inc_ref,
            Some(&w.cand_ref),
            1,
            Some(&adm),
            false,
        )),
    )
    .unwrap();
    let mut cfg = w.s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(VERIFIER).unwrap());
    w.s.write_config(&cfg).unwrap();
    let rollback = |issuer: &str| {
        let mut v = transition(
            "rb",
            "rollback",
            &w.cand_ref,
            Some(&w.inc_ref),
            2,
            Some(&w.baseline),
            false,
        );
        v["issuer_ref"] = json!(issuer);
        pointer::transition(&w.s, &t(v))
    };
    let before = snapshot(w.dir.path());
    for (issuer, why) in [
        ("op:stranger", "not in the trusted-admitter set"),
        (VERIFIER, "a trusted verifier (Compute Fabric)"),
    ] {
        match rollback(issuer) {
            Err(LoopError::Refused(m)) => assert!(m.contains(why), "{issuer}: {m}"),
            o => panic!("{issuer} rolled back: {o:?}"),
        }
    }
    assert_eq!(
        snapshot(w.dir.path()),
        before,
        "a refused rollback wrote something"
    );
    rollback(ADMITTER).unwrap();
    assert_eq!(
        pointer::resolve(&w.s, &scope()).unwrap().pin.version.digest,
        w.inc_ref
    );
}

/// G11-r22-independent-admission: issuing the activation of a candidate IS
/// the promotion act. Its proposer (the ranker), its evaluator and a subject
/// issuer cannot issue it — nor a rollback to it — even when each is listed as
/// a trusted admitter; an independent admitter can (review wf_d788c05a-be2).
#[test]
fn no_proposer_evaluator_or_subject_issues_a_promotion() {
    let w = world();
    let adm = accepted(&w, "exp");
    let mut cfg = w.s.config().unwrap();
    for who in [PROPOSER, EVALUATOR, WORKER] {
        cfg.trusted_admitters.push(OpaqueRef::new(who).unwrap());
    }
    w.s.write_config(&cfg).unwrap();
    let go = |kind: &str, expected: &Ref, target: &Ref, epoch: u64, a: &Ref, issuer: &str| {
        let id = format!("{kind}-{epoch}-{}", issuer.replace(':', "-"));
        let mut v = transition(&id, kind, expected, Some(target), epoch, Some(a), false);
        v["issuer_ref"] = json!(issuer);
        pointer::transition(&w.s, &t(v))
    };
    let refused_all = |kind: &str, expected: &Ref, target: &Ref, epoch: u64| {
        let before = snapshot(w.dir.path());
        for (who, why) in [
            (PROPOSER, "an EVO proposer (the ranker)"),
            (EVALUATOR, "the evaluator"),
            (WORKER, "a subject issuer"),
        ] {
            match go(kind, expected, target, epoch, &adm, who) {
                Err(LoopError::Refused(m)) => assert!(
                    m.contains("self-promotion: transition issuer") && m.contains(why),
                    "{kind} by {who}: {m}"
                ),
                o => panic!("{who} issued the {kind}: {o:?}"),
            }
        }
        assert_eq!(
            snapshot(w.dir.path()),
            before,
            "a refused {kind} wrote something"
        );
    };
    refused_all("activate", &w.inc_ref, &w.cand_ref, 1);
    go("activate", &w.inc_ref, &w.cand_ref, 1, &adm, ADMITTER).unwrap();
    go(
        "rollback",
        &w.cand_ref,
        &w.inc_ref,
        2,
        &w.baseline,
        ADMITTER,
    )
    .unwrap();
    // Back to the candidate: a rollback re-derives the same admission.
    refused_all("rollback", &w.inc_ref, &w.cand_ref, 3);
    go("rollback", &w.inc_ref, &w.cand_ref, 3, &adm, ADMITTER).unwrap();
}

/// The incumbent-of-record is designated by an independent admitter: not by
/// Compute Fabric (a trusted verifier) nor by an EVO proposer, even listed as
/// admitters.
#[test]
fn a_baseline_issuer_holds_no_other_loop_role() {
    let dir = tempfile::tempdir().unwrap();
    let s = store_with_config(dir.path());
    let inc_ref = s.put_cas("policies", &incumbent()).unwrap();
    let mut cfg = s.config().unwrap();
    cfg.trusted_admitters
        .push(OpaqueRef::new(VERIFIER).unwrap());
    s.write_config(&cfg).unwrap();
    let mut b = baseline_doc(&inc_ref);
    b["issuer_ref"] = json!(VERIFIER);
    let before = snapshot(dir.path());
    match pointer::designate_baseline(&s, &pointer::parse_baseline(&b.to_string()).unwrap()) {
        Err(LoopError::Refused(m)) => assert!(m.contains("a trusted verifier"), "{m}"),
        o => panic!("Fabric designated the incumbent-of-record: {o:?}"),
    }
    assert_eq!(snapshot(dir.path()), before);
    pointer::designate_baseline(
        &s,
        &pointer::parse_baseline(&baseline_doc(&inc_ref).to_string()).unwrap(),
    )
    .unwrap();
}
