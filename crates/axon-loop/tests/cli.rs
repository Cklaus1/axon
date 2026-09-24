//! Every verb, through the actual `axon-loop` binary.
mod common;
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
    let tmp = tempfile::NamedTempFile::new().unwrap();
    if let Some(v) = input {
        std::fs::write(tmp.path(), v.to_string()).unwrap();
        cmd.arg("--in").arg(tmp.path());
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

/// A store with config + a stored incumbent (nothing designated yet).
fn fresh() -> (tempfile::TempDir, axon_loop::Store, PolicyEnvelope, Ref) {
    let d = tempfile::tempdir().unwrap();
    let s = store_with_config(d.path());
    let p = incumbent();
    let pr = s.put_cas("policies", &p).unwrap();
    (d, s, p, pr)
}

#[test]
fn cli_pointer_baseline_resolve_transition_show_revoke() {
    let (d, _s, p, pr) = fresh();
    assert_eq!(
        run(d.path(), &args(&["pointer", "resolve"], &SC), None).0,
        6
    );

    let (c, v, e) = run(d.path(), &["pointer", "baseline"], Some(&baseline_doc(&pr)));
    assert_eq!(c, 0, "{e}");
    let b: Ref = serde_json::from_value(v["baseline_ref"].clone()).unwrap();
    let mut bad = baseline_doc(&pr);
    bad["issuer_ref"] = json!("agent:self");
    assert_eq!(run(d.path(), &["pointer", "baseline"], Some(&bad)).0, 4);

    let mut t = transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&b),
        false,
    );
    t["issuer_ref"] = json!("agent:self");
    let before = snapshot(d.path());
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&t)).0, 4);
    let mut bad = transition(
        "t1",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&b),
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
        Some(&b),
        false,
    );
    let (c, v, e) = run(d.path(), &["pointer", "transition"], Some(&t));
    assert_eq!(c, 0, "{e}");
    assert_eq!(v["pointer"]["epoch"], json!(1));
    let t2 = transition(
        "t2",
        "activate",
        &null_policy_ref(),
        Some(&pr),
        0,
        Some(&b),
        false,
    );
    assert_eq!(run(d.path(), &["pointer", "transition"], Some(&t2)).0, 5);

    let (c, v, _) = run(d.path(), &args(&["pointer", "resolve"], &SC), None);
    assert_eq!(c, 0);
    assert_eq!(v["pin"]["version"]["digest"], json!(pr));
    assert_eq!(v["pin"]["epoch"], json!(1));
    assert_eq!(v["mechanism_test"], json!(false));
    assert_eq!(v["policy"], serde_json::to_value(&p).unwrap());

    let (c, v, _) = run(d.path(), &args(&["pointer", "show"], &SC), None);
    assert_eq!((c, v["transitions"].clone()), (0, json!(1)));

    // policy put
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
    let mut badp = serde_json::to_value(&other).unwrap();
    badp["authority_expansion"] = json!(true);
    assert_ne!(run(d.path(), &["policy", "put"], Some(&badp)).0, 0);

    let pr_s = pr.to_string();
    let reason = r('e').to_string();
    let rv = args(
        &["pointer", "revoke"],
        &[
            SC[0], SC[1], SC[2], SC[3], "--policy", &pr_s, "--reason", &reason, "--issuer",
            ADMITTER,
        ],
    );
    assert_eq!(run(d.path(), &rv, None).0, 0);
    assert_eq!(
        run(d.path(), &args(&["pointer", "resolve"], &SC), None).0,
        6
    );
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

    // A7b through the binary: a hand-edited projection is exit 2
    let pp = d
        .path()
        .join("scopes/fixture-tenant/fixture-coding/pointer.json");
    let mut v: Value = serde_json::from_slice(&std::fs::read(&pp).unwrap()).unwrap();
    v["pointer"]["active_policy_ref"] = json!(digest(&other).unwrap());
    std::fs::write(&pp, v.to_string()).unwrap();
    assert_eq!(
        run(d.path(), &args(&["pointer", "resolve"], &SC), None).0,
        2
    );
}

#[test]
fn cli_two_processes_race_exactly_one_wins() {
    let dir = tempfile::tempdir().unwrap();
    for round in 0..5 {
        let (d, s, _p, pr) = fresh();
        let b = axon_loop::pointer::designate_baseline(
            &s,
            &axon_loop::pointer::parse_baseline(&baseline_doc(&pr).to_string()).unwrap(),
        )
        .unwrap();
        let t1 = dir.path().join(format!("a{round}.json"));
        let t2 = dir.path().join(format!("b{round}.json"));
        std::fs::write(
            &t1,
            transition(
                "ta",
                "activate",
                &null_policy_ref(),
                Some(&pr),
                0,
                Some(&b),
                false,
            )
            .to_string(),
        )
        .unwrap();
        let mut other = transition(
            "tb",
            "activate",
            &null_policy_ref(),
            Some(&pr),
            0,
            Some(&b),
            false,
        );
        other["reason_ref"] = json!(r('d'));
        std::fs::write(&t2, other.to_string()).unwrap();
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
        let mut codes = [
            c1.wait_with_output().unwrap().status.code().unwrap(),
            c2.wait_with_output().unwrap().status.code().unwrap(),
        ];
        codes.sort();
        assert_eq!(codes, [0, 5], "exactly one winner, one CAS conflict");
        assert_eq!(
            axon_loop::pointer::load(&s, &scope()).unwrap().epoch.get(),
            1
        );
        assert_eq!(axon_loop::pointer::log(&s, &scope()).unwrap().len(), 1);
    }
}

#[test]
fn cli_plan_register_freeze_show() {
    let w = world();
    let d = w.dir.path();
    let tmpl: Value = serde_json::from_str(include_str!("fixtures/pilot-template.json")).unwrap();
    let (c, v, _) = run(d, &["plan", "register"], Some(&tmpl));
    assert_eq!(c, 0);
    assert_eq!(v["unset_fields"].as_array().unwrap().len(), 24);
    let before = snapshot(d);
    assert_eq!(
        run(
            d,
            &["plan", "freeze", "--experiment", "operator-must-configure"],
            None
        )
        .0,
        7
    );
    assert_eq!(snapshot(d), before);
    let (c, v, _) = run(
        d,
        &["plan", "show", "--experiment", "operator-must-configure"],
        None,
    );
    assert_eq!((c, v["frozen_ref"].clone()), (0, Value::Null));

    let mut full = complete_plan("exp", &w.inc_ref, &w.cand_ref);
    full["task_manifest_ref"] = json!(r('1')); // a manifest nobody registered
    assert_eq!(run(d, &["plan", "register"], Some(&full)).0, 0);
    // AB9: freeze needs the registered task manifest behind task_manifest_ref
    let before = snapshot(d);
    let (c, _, e) = run(d, &["plan", "freeze", "--experiment", "exp"], None);
    assert!(
        c == 4 && e.contains("not a registered task manifest"),
        "{e}"
    );
    assert_eq!(snapshot(d), before);
    let manifest = json!({"schema":"axon.loop.task-manifest/1","scope":scope(),
                          "tasks":["task-0","task-1","task-2"],"issuer_ref":ADMITTER});
    let (c, v, e) = run(d, &["tasks", "put"], Some(&manifest));
    assert_eq!(c, 0, "{e}");
    full["task_manifest_ref"] = v["task_manifest_ref"].clone();
    assert_eq!(run(d, &["plan", "register"], Some(&full)).0, 0);
    let (c, v, e) = run(d, &["plan", "freeze", "--experiment", "exp"], None);
    assert_eq!(c, 0, "{e}");
    let pref = v["plan_ref"].clone();
    let (_, v, _) = run(d, &["plan", "show", "--experiment", "exp"], None);
    assert_eq!(v["frozen_ref"], pref);
    assert_eq!(v["start_blockers"], json!([]));
    let before = snapshot(d);
    let mut changed = full.clone();
    changed["repetitions"] = json!(9);
    assert_eq!(run(d, &["plan", "register"], Some(&changed)).0, 4);
    assert_eq!(snapshot(d), before);
    let mut bad = full.clone();
    bad["repetitions"] = json!(0);
    assert_eq!(run(d, &["plan", "register"], Some(&bad)).0, 3);
    // D1/D8 through the binary: nothing created outside the store
    let outside = d.parent().unwrap().join("outside-d1");
    assert_eq!(
        run(
            d,
            &["plan", "freeze", "--experiment", "../../outside-d1/evil"],
            None
        )
        .0,
        2
    );
    assert_eq!(
        run(
            d,
            &["plan", "freeze", "--experiment", outside.to_str().unwrap()],
            None
        )
        .0,
        2
    );
    assert!(!outside.exists());
}

#[test]
fn cli_evo_evl_admit_tel_end_to_end() {
    let (d, s, inc, inc_ref) = fresh();
    let b = axon_loop::pointer::designate_baseline(
        &s,
        &axon_loop::pointer::parse_baseline(&baseline_doc(&inc_ref).to_string()).unwrap(),
    )
    .unwrap();
    assert_eq!(
        run(
            d.path(),
            &["pointer", "transition"],
            Some(&transition(
                "boot",
                "activate",
                &null_policy_ref(),
                Some(&inc_ref),
                0,
                Some(&b),
                false
            ))
        )
        .0,
        0
    );

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

    let manifest = json!({"schema":"axon.loop.task-manifest/1","scope":scope(),
                          "tasks":["task-0","task-1"],"issuer_ref":ADMITTER});
    let (c, v, e) = run(d.path(), &["tasks", "put"], Some(&manifest));
    assert_eq!(c, 0, "{e}");
    let mut plan_doc = complete_plan("exp", &inc_ref, &cand_ref);
    plan_doc["task_manifest_ref"] = v["task_manifest_ref"].clone();
    assert_eq!(run(d.path(), &["plan", "register"], Some(&plan_doc)).0, 0);
    assert_eq!(
        run(d.path(), &["plan", "freeze", "--experiment", "exp"], None).0,
        0
    );

    let specs = pair(&inc, &cand, 2, 2, 2, Some(100), Some(50));
    let req = evl_request("exp", &inc, &cand, &specs, &EvlOpts::default());
    let (c, v, e) = run(d.path(), &["evl", "evaluate"], Some(&req));
    assert_eq!(c, 0, "{e}");
    let eval_ref = v["evaluation_ref"].clone();
    let mut dup = req.clone();
    dup["surprise"] = json!(true);
    assert_eq!(run(d.path(), &["evl", "evaluate"], Some(&dup)).0, 3);

    let adm_req = |who: &str| {
        json!({"schema":"axon.loop.admit-request/1","experiment_id":"exp",
        "evaluation_ref":eval_ref,"admitter_ref":who,"mechanism_test":false})
    };
    assert_eq!(
        run(d.path(), &["admit"], Some(&adm_req("op:stranger"))).0,
        4
    );
    let (c, v, e) = run(d.path(), &["admit"], Some(&adm_req(ADMITTER)));
    assert_eq!(c, 0, "{e}");
    assert_eq!(v["admission"]["decision"], json!("ACCEPT"));
    let adm: Ref = serde_json::from_value(v["admission_ref"].clone()).unwrap();

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
    let mut episodes: Vec<Value> = req["trials"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["episode"].clone())
        .collect();
    episodes.push(trial(&tf)["episode"].clone());
    let mut o = Command::new(BIN)
        .args(["tel", "summarize", "--in", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    use std::io::Write;
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
    assert_eq!(v["summary"]["records"], json!(5));
    let dupe = json!({"schema":"axon.loop.tel-request/1","episodes":[episodes[0], episodes[0]]});
    assert_eq!(run(d.path(), &["tel", "summarize"], Some(&dupe)).0, 4);
}
