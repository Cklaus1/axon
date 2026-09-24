//! Every verb, through the actual `axon-loop` binary.
mod common;
use axon_loop::admission::Decision;
use axon_loop::null_policy_ref;
use axon_loop_contracts::*;
use common::*;
use serde_json::{json, Value};
use std::path::Path;
use std::process::{Command, Output, Stdio};

const BIN: &str = env!("CARGO_BIN_EXE_axon-loop");

fn run(store: &Path, args: &[&str], input: Option<&Value>) -> (i32, Value, String) {
    let mut cmd = Command::new(BIN);
    cmd.arg("--store").arg(store).args(args);
    let tmp;
    if let Some(v) = input {
        tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), v.to_string()).unwrap();
        cmd.arg("--in").arg(tmp.path());
        return finish(cmd.output().unwrap());
    }
    finish(cmd.output().unwrap())
}

fn finish(o: Output) -> (i32, Value, String) {
    let code = o.status.code().unwrap();
    let out = String::from_utf8(o.stdout).unwrap();
    let err = String::from_utf8(o.stderr).unwrap();
    let v = if out.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&out).unwrap()
    };
    if code != 0 {
        assert!(out.is_empty(), "stdout must be empty on failure");
        let e: Value = serde_json::from_str(err.trim()).unwrap();
        assert_eq!(e["exit_code"], json!(code));
    }
    (code, v, err)
}

const SC: [&str; 4] = ["--tenant", "fixture-tenant", "--family", "fixture-coding"];

fn args<'a>(verb: &[&'a str], extra: &[&'a str]) -> Vec<&'a str> {
    let mut v = verb.to_vec();
    v.extend_from_slice(extra);
    v
}

#[test]
fn cli_pointer_resolve_transition_show_revoke() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = incumbent();
    let pr = digest(&p).unwrap();
    let a = seed_admission(&s, &p, Decision::Accept, false, true);

    let (c, _, e) = run(d.path(), &args(&["pointer", "resolve"], &SC), None);
    assert_eq!(c, 6, "{e}");

    // refused: untrusted issuer, bytes unchanged
    let mut t = transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&a),
        false,
    );
    t["issuer_ref"] = json!("agent:self");
    let before = snapshot(d.path());
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&t)).0, 4);
    // malformed: next_epoch non-contiguous
    let mut bad = transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&a),
        false,
    );
    bad["next_epoch"] = json!(5);
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&bad)).0, 4);
    bad["unknown"] = json!(1);
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&bad)).0, 3);
    assert_eq!(snapshot(d.path()), before);

    let t = transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&a),
        false,
    );
    let (c, v, e) = run(d.path(), &["pointer", "transition"], Some(&t));
    assert_eq!(c, 0, "{e}");
    assert_eq!(v["pointer"]["epoch"], json!(1));
    // stale replay under a new id ⇒ conflict 5
    let t2 = transition(
        "t2",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&a),
        false,
    );
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&t2)).0, 5);

    let (c, v, _) = run(d.path(), &args(&["pointer", "resolve"], &SC), None);
    assert_eq!(c, 0);
    assert_eq!(v["pin"]["version"]["digest"], json!(pr));
    assert_eq!(v["pin"]["epoch"], json!(1));

    let (c, v, _) = run(d.path(), &args(&["pointer", "show"], &SC), None);
    assert_eq!((c, v["transitions"].clone()), (0, json!(1)));

    let pr_s = pr.to_string();
    let reason = r('e').to_string();
    let (c, _, _) = run(
        d.path(),
        &args(
            &["pointer", "revoke"],
            &[
                SC[0], SC[1], SC[2], SC[3], "--policy", &pr_s, "--reason", &reason, "--issuer",
                ADMITTER,
            ],
        ),
        None,
    );
    assert_eq!(c, 0);
    assert_eq!(
        run(d.path(), &args(&["pointer", "resolve"], &SC), None).0,
        6
    );
    // policy put stores by cl22 digest; an authority-expanding policy is refused
    let other = policy("other", &["read"]);
    let (c, v, _) = run(
        d.path(),
        &["policy", "put"],
        Some(&serde_json::to_value(&other).unwrap()),
    );
    assert_eq!(
        (c, v["policy_ref"].clone()),
        (0, json!(digest(&other).unwrap()))
    );
    let mut bad = serde_json::to_value(&other).unwrap();
    bad["authority_expansion"] = json!(true);
    assert_ne!(run(d.path(), &["policy", "put"], Some(&bad)).0, 0);
    // usage errors
    assert_eq!(run(d.path(), &["pointer", "frobnicate"], None).0, 2);
    assert_eq!(
        run(
            d.path(),
            &args(&["pointer", "resolve"], &["--tenant", "x"]),
            None
        )
        .0,
        2
    );
}

#[test]
fn cli_two_processes_race_exactly_one_wins() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = incumbent();
    let pr = digest(&p).unwrap();
    let a = seed_admission(&s, &p, Decision::Accept, false, true);
    let other = policy("other", &["read"]);
    let oa = seed_admission(&s, &other, Decision::Accept, false, true);
    let oref = digest(&other).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let t1 = dir.path().join("a.json");
    let t2 = dir.path().join("b.json");
    std::fs::write(
        &t1,
        transition(
            "ta",
            "activate",
            &null_policy_ref(),
            Some(&pr),
            0,
            Some(&a),
            false,
        )
        .to_string(),
    )
    .unwrap();
    std::fs::write(
        &t2,
        transition(
            "tb",
            "activate",
            &null_policy_ref(),
            Some(&oref),
            0,
            Some(&oa),
            false,
        )
        .to_string(),
    )
    .unwrap();
    for _round in 0..5 {
        let d = tempfile::tempdir().unwrap();
        let s2 = store_with_config(d.path());
        seed_admission(&s2, &p, Decision::Accept, false, true);
        seed_admission(&s2, &other, Decision::Accept, false, true);
        let spawn = |f: &Path| {
            Command::new(BIN)
                .arg("--store")
                .arg(d.path())
                .args(["pointer", "transition", "--in"])
                .arg(f)
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap()
        };
        let (c1, c2) = (spawn(&t1), spawn(&t2));
        let codes = [
            c1.wait_with_output().unwrap().status.code().unwrap(),
            c2.wait_with_output().unwrap().status.code().unwrap(),
        ];
        let mut sorted = codes;
        sorted.sort();
        assert_eq!(
            sorted,
            [0, 5],
            "exactly one winner, one CAS conflict: {codes:?}"
        );
        let v = axon_loop::pointer::load(&s2, &scope()).unwrap();
        assert_eq!(v.epoch.get(), 1);
        assert_eq!(axon_loop::pointer::log(&s2, &scope()).unwrap().len(), 1);
    }
    drop(s);
}

#[test]
fn cli_plan_register_freeze_show() {
    let d = tempfile::tempdir().unwrap();
    store_with_config(d.path());
    let tmpl: Value = serde_json::from_str(include_str!("fixtures/pilot-template.json")).unwrap();
    let (c, v, _) = run(d.path(), &["plan", "register"], Some(&tmpl));
    assert_eq!(c, 0);
    assert_eq!(v["unset_fields"].as_array().unwrap().len(), 24);
    let before = snapshot(d.path());
    assert_eq!(
        run(
            d.path(),
            &["plan", "freeze", "--experiment", "operator-must-configure"],
            None
        )
        .0,
        7
    );
    assert_eq!(snapshot(d.path()), before);
    let (c, v, _) = run(
        d.path(),
        &["plan", "show", "--experiment", "operator-must-configure"],
        None,
    );
    assert_eq!(c, 0);
    assert_eq!(v["frozen_ref"], Value::Null);

    let full = complete_plan("exp", &r('a'), &r('b'));
    assert_eq!(run(d.path(), &["plan", "register"], Some(&full)).0, 0);
    let (c, v, _) = run(d.path(), &["plan", "freeze", "--experiment", "exp"], None);
    assert_eq!(c, 0);
    let pref = v["plan_ref"].clone();
    let (_, v, _) = run(d.path(), &["plan", "show", "--experiment", "exp"], None);
    assert_eq!(v["frozen_ref"], pref);
    assert_eq!(v["start_blockers"], json!([]));
    let before = snapshot(d.path());
    let mut changed = full.clone();
    changed["repetitions"] = json!(9);
    assert_eq!(run(d.path(), &["plan", "register"], Some(&changed)).0, 4);
    assert_eq!(snapshot(d.path()), before);
    let mut bad = full.clone();
    bad["repetitions"] = json!(0);
    assert_eq!(run(d.path(), &["plan", "register"], Some(&bad)).0, 3);
}

#[test]
fn cli_evo_evl_admit_tel_end_to_end() {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let inc = incumbent();
    let inc_ref = digest(&inc).unwrap();

    // evo propose: confirmation evidence refused, nothing written
    let before = snapshot(d.path());
    let bad = evo_request(
        &inc,
        1,
        "cand",
        vec![episode_with_role(&inc, "x", CorpusRole::Confirmation)],
    );
    assert_eq!(run(d.path(), &["evo", "propose"], Some(&bad)).0, 4);
    assert_eq!(snapshot(d.path()), before);
    let (c, v, e) = run(
        d.path(),
        &["evo", "propose"],
        Some(&evo_request(
            &inc,
            1,
            "cand",
            vec![discovery_episode(&inc, "d1")],
        )),
    );
    assert_eq!(c, 0, "{e}");
    let cand: PolicyEnvelope = parse(&v["candidate"].to_string()).unwrap();
    let cand_ref: Ref = serde_json::from_value(v["candidate_policy_ref"].clone()).unwrap();
    assert_eq!(digest(&cand).unwrap(), cand_ref);

    // plan
    run(
        d.path(),
        &["plan", "register"],
        Some(&complete_plan("exp", &inc_ref, &cand_ref)),
    );
    assert_eq!(
        run(d.path(), &["plan", "freeze", "--experiment", "exp"], None).0,
        0
    );

    // evl evaluate
    let mut assigned = vec![];
    let mut trials = vec![];
    for (arm, p, task, t, cost) in [
        ("incumbent", &inc, "task-1", "i1", 100),
        ("incumbent", &inc, "task-2", "i2", 100),
        ("challenger-1", &cand, "task-1", "c1", 50),
        ("challenger-1", &cand, "task-2", "c2", 50),
    ] {
        assigned.push(json!({"task_id": task, "arm_id": arm, "trial_id": t, "policy_ref": digest(p).unwrap()}));
        let mut tr = Trial::new(p, task, arm, t);
        tr.cost = Some(cost);
        trials.push(trial(&tr));
    }
    let req = json!({"schema":"axon.loop.evl-request/1","scope":scope(),"evaluator_ref":EVALUATOR,
        "subject_issuers":[WORKER],"policies":[inc, cand],"assigned":assigned,"trials":trials.clone()});
    let (c, v, e) = run(d.path(), &["evl", "evaluate"], Some(&req));
    assert_eq!(c, 0, "{e}");
    let eval_ref = v["evaluation_ref"].clone();
    let mut dup = req.clone();
    dup["surprise"] = json!(true);
    assert_eq!(run(d.path(), &["evl", "evaluate"], Some(&dup)).0, 3);

    // admit
    let adm_req = |who: &str| {
        json!({"schema":"axon.loop.admit-request/1","experiment_id":"exp",
        "evaluation_ref":eval_ref,"admitter_ref":who,"mechanism_test":false})
    };
    assert_eq!(run(d.path(), &["admit"], Some(&adm_req(PROPOSER))).0, 4);
    let (c, v, e) = run(d.path(), &["admit"], Some(&adm_req(ADMITTER)));
    assert_eq!(c, 0, "{e}");
    assert_eq!(v["admission"]["decision"], json!("ACCEPT"));
    let adm: Ref = serde_json::from_value(v["admission_ref"].clone()).unwrap();

    // activate through the CLI: incumbent first, then the admitted candidate
    let inc_adm = seed_admission(&s, &inc, Decision::Accept, false, true);
    assert_eq!(
        run(
            d.path(),
            &["pointer", "transition"],
            Some(&transition(
                "a0",
                "activate",
                &null_policy_ref(),
                Some(&inc_ref),
                0,
                Some(&inc_adm),
                false
            ))
        )
        .0,
        0
    );
    assert_eq!(
        run(
            d.path(),
            &["pointer", "transition"],
            Some(&transition(
                "a1",
                "activate",
                &inc_ref,
                Some(&cand_ref),
                1,
                Some(&adm),
                false
            ))
        )
        .0,
        0
    );
    let (_, v, _) = run(d.path(), &args(&["pointer", "resolve"], &SC), None);
    assert_eq!(v["pin"]["version"]["digest"], json!(cand_ref));

    // tel summarize (no store needed): includes a failed + unknown-cost attempt
    let mut tf = Trial::new(&cand, "task-3", "challenger-1", "c3");
    tf.out = Out::Fail;
    tf.cost = None;
    let mut episodes: Vec<Value> = trials.iter().map(|t| t["episode"].clone()).collect();
    episodes.push(trial(&tf)["episode"].clone());
    let o = Command::new(BIN)
        .args(["tel", "summarize", "--in", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
    let mut o = o;
    o.stdin
        .take()
        .unwrap()
        .write_all(
            json!({"schema":"axon.loop.tel-request/1","episodes":episodes})
                .to_string()
                .as_bytes(),
        )
        .unwrap();
    let (c, v, e) = finish(o.wait_with_output().unwrap());
    assert_eq!(c, 0, "{e}");
    let cur = &v["summary"]["by_currency"][0];
    assert_eq!(
        cur["total"],
        json!({"state":"unresolved","known_sum_micro":300,"unknown_count":1,"unresolved_liability_micro":0})
    );
    assert_eq!(
        cur["non_completed_records"],
        json!(0),
        "failed-verification episodes are completed processes"
    );
    assert_eq!(v["summary"]["records"], json!(5));
    // duplicate attempt ⇒ refused
    let dupe = json!({"schema":"axon.loop.tel-request/1","episodes":[episodes[0], episodes[0]]});
    assert_eq!(run(d.path(), &["tel", "summarize"], Some(&dupe)).0, 4);
}
