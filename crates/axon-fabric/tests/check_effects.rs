//! B264 remainder — registered check suites (visible vs hidden) and
//! G03-check-effects: every substitution of argv / executable / check bytes,
//! and a stale authority epoch, is refused with ZERO journal launch records
//! and zero spawns. A hidden check's bytes live in their own read-only
//! WorkspaceVersion and never reach the subject's context. The cortex
//! executor accepts a report only when the receipt binds its candidate.

mod common;
use common::*;

use std::path::{Path, PathBuf};

use axon_cortex::runner::{
    CheckExecutor, CheckRegistry, CheckRequest, CheckVisibility, FabricDispatch,
    FabricSubmitExecutor, RegisteredCheck,
};
use axon_fabric::submit;
use axon_fabric::workspace::{Quota, WorkspaceStore, WorkspaceTree};
use axon_loop_contracts::{Acf1Ref, ReceiptVerification};
use serde_json::{json, Value};

const MARKER: &str = "HIDDEN-GRADER-7f3a91";

fn hidden_suite_src() -> String {
    format!(
        "// {MARKER}: the expected outputs the subject must never see\n\
         mod f\n\
         use f.{{double}}\n\n\
         @[test]\n\
         fn hidden_completion() {{ assert_eq(double(21), 42) }}\n"
    )
}

struct Suite {
    env: Env,
    candidate: Acf1Ref,
    suite_root: PathBuf,
    suite_ref: String,
}

/// Write a suite at `<dir>/suites/<id>/h.ax`, register it, and publish the
/// candidate workspace (f.ax) as a WorkspaceVersion.
fn with_suite(src: &str, visibility: &str) -> Suite {
    let env = Env::new();
    let suite_root = env.dir.path().join("suites/hidden-suite");
    std::fs::create_dir_all(&suite_root).unwrap();
    std::fs::write(suite_root.join("h.ax"), src).unwrap();
    let suite_ref = WorkspaceTree::import_dir(&suite_root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&env.registry).unwrap()).unwrap();
    reg["checks"] = json!([{
        "id": "hidden-suite", "visibility": visibility, "root": suite_root,
        "entry": "h.ax", "workspace_version_ref": suite_ref,
    }]);
    std::fs::write(&env.registry, reg.to_string()).unwrap();
    let candidate = WorkspaceStore::open(&env.cfg(0).state_dir, &tenant())
        .unwrap()
        .import_dir(&env.ws, &Quota::default())
        .unwrap();
    Suite {
        env,
        candidate,
        suite_root,
        suite_ref,
    }
}

fn suite_request(s: &Suite, op: &str) -> Value {
    let mut r = request(&s.env, op, "hidden_completion");
    r["argv"] = json!(["check:hidden-suite", "hidden_completion"]);
    r["workspace_version_ref"] = s.candidate.as_str().into();
    r
}

fn all_bytes_under(p: &Path, out: &mut Vec<u8>) {
    for e in std::fs::read_dir(p).unwrap().flatten() {
        let m = std::fs::symlink_metadata(e.path()).unwrap();
        if m.is_dir() {
            all_bytes_under(&e.path(), out);
        } else if m.is_file() {
            out.extend(std::fs::read(e.path()).unwrap());
        }
    }
}

fn assert_untouched(env: &Env, what: &str) {
    assert_eq!(spawn_count(&env.spawns), 0, "{what}: nothing spawned");
    assert_eq!(env.launch_records(), 0, "{what}: no journal launch record");
}

// ── hidden checks ───────────────────────────────────────────────────────────

#[test]
fn a_hidden_check_judges_the_candidate_from_outside_its_context() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let cfg = s.env.cfg(0);
    // Positive control: the grader exists and names the marker.
    assert!(std::fs::read_to_string(s.suite_root.join("h.ax"))
        .unwrap()
        .contains(MARKER));

    let sub = submit(&suite_request(&s, "op-hidden").to_string(), &cfg).unwrap();
    let r = &sub.receipt;
    assert_eq!(
        r.verification,
        ReceiptVerification::Passed,
        "{:?}",
        sub.reason
    );
    assert_eq!(r.matched_checks, Some(1));
    assert_eq!(r.input_workspace_ref, s.candidate);
    assert_eq!(
        r.output_workspace_ref.as_ref(),
        Some(&s.candidate),
        "the run left the candidate exactly as it found it"
    );

    // The subject's context — its workspace version (in and out), the
    // request it sent, the receipt and report it gets back, the journal —
    // carries the grader's identity at most, never its bytes.
    let store = WorkspaceStore::open(&cfg.state_dir, &tenant()).unwrap();
    let cand = store.tree(&s.candidate).unwrap();
    let out = store
        .tree(r.output_workspace_ref.as_ref().unwrap())
        .unwrap();
    assert_eq!(cand, out);
    assert_eq!(
        cand.entries()
            .iter()
            .map(|e| e.path.as_str())
            .collect::<Vec<_>>(),
        vec!["f.ax"],
        "no check file was placed in the candidate"
    );
    for e in cand.entries() {
        assert!(!String::from_utf8_lossy(&e.content).contains(MARKER));
    }
    let seen = format!(
        "{}{}{}{}",
        suite_request(&s, "op-hidden"),
        serde_json::to_string(r).unwrap(),
        sub.check_report.as_ref().unwrap(),
        s.env.journal_text()
    );
    assert!(!seen.contains(MARKER), "hidden bytes leaked into: {seen}");
    assert!(
        s.env.journal_text().contains(&s.suite_ref),
        "the journal records WHICH suite judged (its identity)"
    );

    // The grader is its own WorkspaceVersion in the store, distinct from the
    // candidate, and the per-op materializations are gone.
    let suite_ref = Acf1Ref::new(s.suite_ref.clone()).unwrap();
    assert_ne!(suite_ref, s.candidate);
    let mut suite_bytes = Vec::new();
    for e in store.tree(&suite_ref).unwrap().entries() {
        suite_bytes.extend(&e.content);
    }
    assert!(String::from_utf8_lossy(&suite_bytes).contains(MARKER));
    let runs = cfg.state_dir.join("runs");
    assert_eq!(std::fs::read_dir(&runs).unwrap().count(), 0);
    let mut state_ws = Vec::new();
    all_bytes_under(&s.env.ws, &mut state_ws);
    assert!(!String::from_utf8_lossy(&state_ws).contains(MARKER));

    // A context builder's listing omits it.
    let reg = CheckRegistry::load(&s.env.registry).unwrap();
    assert_eq!(
        reg.check("hidden-suite").unwrap().visibility,
        CheckVisibility::Hidden
    );
    assert_eq!(reg.subject_visible_checks().count(), 0);
}

#[test]
fn a_visible_suite_is_listed_for_the_subject_and_a_hidden_one_is_not() {
    let mut reg = CheckRegistry::new();
    for (id, v) in [
        ("seen", CheckVisibility::Visible),
        ("unseen", CheckVisibility::Hidden),
    ] {
        reg.register_check(RegisteredCheck {
            id: id.into(),
            visibility: v,
            root: "/nowhere".into(),
            entry: "h.ax".into(),
            workspace_version_ref: format!("acf1:{}", "1".repeat(64)),
        });
    }
    let ids: Vec<&str> = reg
        .subject_visible_checks()
        .map(|c| c.id.as_str())
        .collect();
    assert_eq!(ids, vec!["seen"]);
    // An unknown visibility is refused at load, never defaulted.
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("r.json");
    std::fs::write(
        &f,
        json!({"schema":"cortex-check-registry/1","executors":[],"checks":[{
            "id":"x","visibility":"secret","root":"/r","entry":"h.ax",
            "workspace_version_ref": format!("acf1:{}", "1".repeat(64))}]})
        .to_string(),
    )
    .unwrap();
    assert!(CheckRegistry::load(&f).unwrap_err().contains("visibility"));
}

const WRITES_INTO_CANDIDATE: &str = "\
mod f
use f.{double}

@[test]
fn hidden_completion() {
    match env_var(\"AXON_PATH\") {
        Ok(p) => {
            let dirs = str_split(p, \":\")
            let c = dirs[1]
            let _ = write_file(\"{c}/planted.txt\", \"moved\")
            assert_eq(double(21), 42)
        }
        Err(e) => assert(false)
    }
}
";

const WRITES_INTO_SUITE: &str = "\
mod f
use f.{double}

@[test]
fn hidden_completion() {
    match env_var(\"AXON_PATH\") {
        Ok(p) => {
            let dirs = str_split(p, \":\")
            let c = dirs[0]
            let _ = write_file(\"{c}/planted.txt\", \"moved\")
            assert_eq(double(21), 42)
        }
        Err(e) => assert(false)
    }
}
";

#[test]
fn a_run_that_moves_its_candidate_or_its_grader_is_no_verdict() {
    for (src, what) in [
        (WRITES_INTO_CANDIDATE, "candidate"),
        (WRITES_INTO_SUITE, "suite"),
    ] {
        let s = with_suite(src, "hidden");
        let sub = submit(&suite_request(&s, "op-moves").to_string(), &s.env.cfg(0)).unwrap();
        // The named test itself passed …
        assert_eq!(
            sub.check_report.as_ref().unwrap()["passed"][0],
            "hidden_completion",
            "{what}"
        );
        // … but the verdict cannot stand for the candidate.
        assert_eq!(
            sub.receipt.verification,
            ReceiptVerification::Unknown,
            "{what}: {:?}",
            sub.reason
        );
        assert!(
            sub.reason.as_deref().unwrap_or("").contains("changed"),
            "{what}"
        );
        let out = sub.receipt.output_workspace_ref.clone().unwrap();
        if what == "candidate" {
            assert_ne!(out, s.candidate, "the output ref names what the run left");
            let t = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
                .unwrap()
                .tree(&out)
                .expect("the output version is retrievable");
            assert!(t.entries().iter().any(|e| e.path == "planted.txt"));
        }
    }
}

// ── G03-check-effects: substitutions and stale authority, zero launches ─────

#[test]
fn substituted_argv_is_refused_with_zero_launch_records() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let cfg = s.env.cfg(0);
    let cases: Vec<(&str, Value, Option<&str>)> = vec![
        ("absolute program", json!(["/bin/sh"]), None),
        ("traversal", json!(["../f.ax", "t_ok"]), None),
        ("unregistered suite", json!(["check:nope", "t_ok"]), None),
        ("extra argv", json!(["f.ax", "t_ok", "--exec"]), None),
        // A published version, but the named file is not in it.
        (
            "file outside the version",
            json!(["other.ax", "t_ok"]),
            Some("candidate"),
        ),
    ];
    for (i, (what, argv, ws)) in cases.into_iter().enumerate() {
        let mut r = request(&s.env, &format!("op-argv-{i}"), "t_ok");
        r["argv"] = argv;
        if ws.is_some() {
            r["workspace_version_ref"] = s.candidate.as_str().into();
        }
        let e = submit(&r.to_string(), &cfg).unwrap_err();
        assert!(
            matches!(e.kind(), "malformed" | "unregistered"),
            "{what}: {e}"
        );
        assert_untouched(&s.env, what);
    }
    // A hidden suite against a candidate that is not a published version.
    let mut r = suite_request(&s, "op-unpub");
    r["workspace_version_ref"] = format!("acf1:{}", "5".repeat(64)).into();
    assert_eq!(submit(&r.to_string(), &cfg).unwrap_err().kind(), "conflict");
    assert_untouched(&s.env, "unpublished candidate");
}

#[test]
fn a_substituted_check_suite_is_refused_with_zero_launch_records() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    // The suite on disk no longer matches what the operator pinned.
    std::fs::write(
        s.suite_root.join("h.ax"),
        "mod f\nuse f.{double}\n@[test]\nfn hidden_completion() { assert(true) }\n",
    )
    .unwrap();
    let e = submit(&suite_request(&s, "op-sub").to_string(), &s.env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "unregistered", "{e}");
    assert!(e.to_string().contains("not the registered"), "{e}");
    assert_untouched(&s.env, "substituted suite");
}

#[test]
fn a_substituted_executable_is_refused_with_zero_launch_records() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let cfg = s.env.cfg(0);
    // Another registry id, and a digest for another binary.
    let mut r = suite_request(&s, "op-exe-1");
    r["registered_executable_ref"] = "sh".into();
    assert_eq!(
        submit(&r.to_string(), &cfg).unwrap_err().kind(),
        "unregistered"
    );
    let mut r = suite_request(&s, "op-exe-2");
    r["executable_digest"] =
        axon_cortex::runner::fabric_executable_digest("axon-test-local", &"0".repeat(64)).into();
    assert_eq!(
        submit(&r.to_string(), &cfg).unwrap_err().kind(),
        "unregistered"
    );
    assert_untouched(&s.env, "substituted executable ref/digest");

    // The registered binary is replaced after admission, before launch.
    fn swap_binary(cfg: &axon_fabric::SubmitConfig) {
        let p = cfg.registry.get("axon-test-local").unwrap().path.clone();
        std::fs::write(&p, "#!/bin/sh\necho substituted\n").unwrap();
    }
    let mut cfg = s.env.cfg(0);
    cfg.pre_launch_hook = Some(swap_binary);
    let e = submit(&suite_request(&s, "op-exe-3").to_string(), &cfg).unwrap_err();
    assert_eq!(e.kind(), "unregistered", "{e}");
    assert_untouched(&s.env, "binary swapped before launch");
    assert!(
        s.env.journal_text().contains("\"cancel"),
        "reservation released"
    );
}

#[test]
fn a_stale_epoch_is_refused_with_zero_launch_records() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    // At submit.
    s.env.bump_epoch();
    let e = submit(&suite_request(&s, "op-stale-1").to_string(), &s.env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "stale_epoch");
    assert_untouched(&s.env, "stale at submit");
    // Between admission and launch.
    fn bump(cfg: &axon_fabric::SubmitConfig) {
        let axon_fabric::EpochSource::LoopStore { store, .. } = &cfg.epoch;
        bump_store_epoch(store);
    }
    let mut cfg = s.env.cfg(1);
    cfg.pre_launch_hook = Some(bump);
    let e = submit(&suite_request(&s, "op-stale-2").to_string(), &cfg).unwrap_err();
    assert_eq!(e.kind(), "stale_epoch");
    assert_untouched(&s.env, "stale at dispatch");
}

fn bump_store_epoch(store: &Path) {
    let st = axon_loop::Store::open_dir(store).unwrap();
    let cur = axon_loop::epoch::current(&st, &scope()).unwrap().get();
    let p = axon_loop::pointer::load(&st, &scope()).unwrap();
    let t: axon_loop_contracts::PolicyTransition = axon_loop_contracts::parse(
        &json!({
            "schema": "axon.closed-loop.transition/1",
            "transition_id": format!("bump-{cur}"),
            "kind": "pause",
            "scope": scope(),
            "expected_policy_ref": p.expected_ref(),
            "target_policy_ref": null,
            "expected_epoch": cur,
            "next_epoch": cur + 1,
            "admission_ref": null,
            "reason_ref": format!("cl22:{}", "e".repeat(64)),
            "issuer_ref": ADMITTER,
            "mechanism_test": true
        })
        .to_string(),
    )
    .unwrap();
    axon_loop::pointer::transition(&st, &t).unwrap();
}

// ── the cortex executor binds its candidate ─────────────────────────────────

/// A stand-in `axon-fabric` that answers every submit with a receipt whose
/// input/output refs are `input` / `output` (`SENT` = echo the request's).
fn fake_fabric(dir: &Path, input: &str, output: &str) -> PathBuf {
    let p = dir.join(format!("fake-fabric-{}-{}.sh", &input[..4], &output[..4]));
    let body = format!(
        r#"#!/bin/sh
REQ=$(cat)
SENT=$(printf '%s' "$REQ" | sed -n 's/.*"workspace_version_ref":"\([^"]*\)".*/\1/p')
IN="{input}"; OUT="{output}"
[ "$IN" = SENT ] && IN="$SENT"
[ "$OUT" = SENT ] && OUT="$SENT"
printf '{{"schema":"axon-fabric-submit/1","receipt":{{"input_workspace_ref":"%s","output_workspace_ref":"%s","status":"completed"}},"check_report":{{"schema":"cortex-check-report/1","failed":[],"passed":["t_ok"],"total":1,"exit_code":0}},"replayed":false,"backend":"x","reason":null}}\n' "$IN" "$OUT"
"#
    );
    std::fs::write(&p, body).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o755)).unwrap();
    p
}

fn executor_with(env: &Env, fabric: &Path) -> FabricSubmitExecutor {
    let reg = env.dir.path().join(format!(
        "reg-{}.json",
        fabric.file_name().unwrap().to_str().unwrap()
    ));
    write_registry(&reg, &env.exe, Some(fabric));
    FabricSubmitExecutor::new(FabricDispatch {
        registry_file: reg,
        journal: env.journal.clone(),
        store: env.store.clone(),
        tenant: "tenant-t".into(),
        family: "family-f".into(),
        expected_epoch: 0,
        grant_registry: env.grant_registry.clone(),
        principal_ref: PRINCIPAL.into(),
        grant_ref: "grant:test".into(),
        policy_digest: format!("acf1:{}", "c".repeat(64)),
        task_id: "task-1".into(),
    })
    .unwrap()
}

#[test]
fn the_cortex_executor_accepts_only_a_receipt_that_binds_its_candidate() {
    let env = Env::new();
    let other = format!("acf1:{}", "9".repeat(64));
    let run = |fabric: PathBuf| {
        executor_with(&env, &fabric).run_checks(&CheckRequest {
            workspace: &env.ws,
            rel_path: "f.ax",
            filter: Some("t_ok"),
        })
    };
    // Positive control: echoing the candidate is accepted.
    let ok = run(fake_fabric(env.dir.path(), "SENT", "SENT")).unwrap();
    assert_eq!(ok.passed, vec!["t_ok".to_string()]);
    // Output moved under the verdict; output missing; input substituted.
    for (i, o) in [
        ("SENT", other.as_str()),
        ("SENT", "garbage"),
        (other.as_str(), "SENT"),
    ] {
        let e = run(fake_fabric(env.dir.path(), i, o)).unwrap_err();
        assert!(
            e.to_string().contains("does not bind the candidate"),
            "({i},{o}): {e}"
        );
    }
}

fn tenant() -> axon_loop_contracts::TenantId {
    axon_loop_contracts::TenantId::new("tenant-t").unwrap()
}

// ── the suite's own modules come first ──────────────────────────────────────

/// A suite with a helper module of its own, over a candidate whose files are
/// `cand` (replacing f.ax): `(env, config, request)`.
fn suite_with_helper(cand: &[(&str, &str)]) -> (Suite, Value) {
    let s = with_suite(
        "mod f\nmod helper\nuse f.{double}\nuse helper.{want}\n\n\
         @[test]\nfn hidden_completion() { assert_eq(double(21), want()) }\n",
        "hidden",
    );
    std::fs::write(s.suite_root.join("helper.ax"), "fn want() -> i64 { 42 }\n").unwrap();
    let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
    reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
    std::fs::write(&s.env.registry, reg.to_string()).unwrap();
    for (name, text) in cand {
        std::fs::write(s.env.ws.join(name), text).unwrap();
    }
    let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
        .unwrap()
        .import_dir(&s.env.ws, &Quota::default())
        .unwrap();
    let s = Suite {
        candidate,
        suite_ref,
        ..s
    };
    let r = suite_request(&s, "op-helper");
    (s, r)
}

/// G01 re-audit 2: the suite reached every module through `AXON_PATH`, which
/// held ONLY the candidate — so a candidate shipping a module named like one
/// of the suite's helpers replaced the rubric's code. Here the candidate's
/// `double` is broken and its `helper.ax` moves the expected value to match;
/// the suite's own helper must win and the check must FAIL.
#[test]
fn a_candidate_cannot_shadow_a_module_of_the_suite() {
    // Positive control: an honest candidate passes against the suite helper.
    let (s, r) = suite_with_helper(&[]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        sub.reason
    );

    let (s, r) = suite_with_helper(&[
        ("f.ax", "fn double(n: i64) -> i64 { n * 0 }\n"),
        ("helper.ax", "fn want() -> i64 { 0 }\n"),
    ]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Failed,
        "the candidate's helper.ax judged its own broken double: {:?} {:?}",
        sub.reason,
        sub.check_report
    );
}

/// A candidate declaring a test under the suite's check name, or one the
/// filter also matches, cannot turn a failing verdict into a pass. Probed, and
/// the two fail closed for DIFFERENT reasons, both pinned here:
///
/// * same name: the duplicate definition stops the run — no summary, Unknown;
/// * a name the filter matches as a SUBSTRING: the candidate's test RUNS in the
///   rubric's run (the `axon test` filter is a substring) — but Fabric judges
///   and counts only the EXACT named check (`matched_checks` 1), so the extra
///   test cannot stand in for it; a failing extra test can only demote a pass
///   to Unknown through the nonzero exit, which harms nobody but the candidate.
#[test]
fn a_candidate_test_named_like_the_suites_cannot_pass_for_it() {
    let broken = "fn double(n: i64) -> i64 { n * 0 }\n";
    let (s, r) = suite_with_helper(&[(
        "f.ax",
        &format!("{broken}@[test]\nfn hidden_completion() {{ assert(true) }}\n"),
    )]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        sub.reason
    );
    assert!(sub.reason.as_deref().unwrap_or("").contains("no summary"));

    let (s, r) = suite_with_helper(&[(
        "f.ax",
        &format!("{broken}@[test]\nfn hidden_completion_ok() {{ assert(true) }}\n"),
    )]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Failed,
        "{:?}",
        sub.reason
    );
    let rep = sub.check_report.unwrap();
    assert_eq!(rep["failed"], json!(["hidden_completion"]));
    assert_eq!(
        rep["passed"],
        json!(["hidden_completion_ok"]),
        "the candidate's test ran"
    );
    assert_eq!(
        sub.receipt.matched_checks,
        Some(1),
        "only the exact name counts"
    );
}

/// Re-audit 4 (R09 survived): "passed requires exit 0" had no test. The named
/// check passes, but another test the substring filter also runs fails, so
/// the run exits nonzero: that is not evidence of a pass, and the receipt
/// says Unknown. Mutation: disable the exit-0 rule in `local_receipt` → red.
#[test]
fn a_named_pass_in_a_run_that_exits_nonzero_is_not_a_pass() {
    let (s, r) = suite_with_helper(&[(
        "f.ax",
        "fn double(n: i64) -> i64 { n * 2 }\n@[test]\nfn hidden_completion_z() { assert(false) }\n",
    )]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    let rep = sub.check_report.clone().unwrap();
    assert_eq!(
        rep["passed"],
        json!(["hidden_completion"]),
        "the named check passed"
    );
    assert_ne!(rep["exit_code"], json!(0), "{rep}");
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Unknown,
        "{:?}",
        sub.reason
    );
}
