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
        })
        .unwrap();
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

// Each program finds its target directory by NAME, not by its position in
// AXON_PATH: the search order is M04's own guard (four-cell vs M436), and
// this test pins the moved-tree property, not the order.
const WRITES_INTO_CANDIDATE: &str = "\
mod f
use f.{double}

@[test]
fn hidden_completion() {
    match env_var(\"AXON_PATH\") {
        Ok(p) => {
            let dirs = str_split(p, \":\")
            let c = if str_ends_with(dirs[0], \"/candidate\") { dirs[0] } else { dirs[1] }
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
            let c = if str_ends_with(dirs[0], \"/check\") { dirs[0] } else { dirs[1] }
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
        write_executable(&p, "#!/bin/sh\necho substituted\n", 0o755);
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
    fake_fabric_verdict(dir, input, output, "passed")
}

fn fake_fabric_verdict(dir: &Path, input: &str, output: &str, verdict: &str) -> PathBuf {
    let p = dir.join(format!(
        "fake-fabric-{}-{}-{verdict}.sh",
        &input[..4],
        &output[..4]
    ));
    let body = format!(
        r#"#!/bin/sh
REQ=$(cat)
SENT=$(printf '%s' "$REQ" | sed -n 's/.*"workspace_version_ref":"\([^"]*\)".*/\1/p')
IN="{input}"; OUT="{output}"
[ "$IN" = SENT ] && IN="$SENT"
[ "$OUT" = SENT ] && OUT="$SENT"
printf '{{"schema":"axon-fabric-submit/1","receipt":{{"input_workspace_ref":"%s","output_workspace_ref":"%s","status":"completed","verification":"{verdict}"}},"check_report":{{"schema":"cortex-check-report/1","failed":[],"passed":["t_ok"],"total":1,"exit_code":0}},"replayed":false,"backend":"x","reason":null}}\n' "$IN" "$OUT"
"#
    );
    write_executable(&p, body, 0o755);
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
    // Fabric's VERDICT decides, not the raw report: the report lists `t_ok`
    // as passed, but Fabric recorded Unknown (no completion evidence, say).
    // Cortex used to accept it (PCI candidate-2 review).
    for verdict in ["unknown", "not_run", "not_requested"] {
        let e = run(fake_fabric_verdict(env.dir.path(), "SENT", "SENT", verdict)).unwrap_err();
        assert!(e.to_string().contains("no verdict"), "{verdict}: {e}");
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
        let p = s.env.ws.join(name);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
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
    // Suite-first order (M04) and the sealed-import rule (M436) each refuse
    // this alone: Failed with the order, Unknown when only M436 stands. The
    // attack is the check PASSING, and needs both removed. M04's own row is
    // the honest candidate below, where the order is the only guard.
    assert_ne!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "ATTACK: the candidate's helper.ax judged its own broken double: {:?} {:?}",
        sub.reason,
        sub.check_report
    );
}

/// The ORDER itself (M04), which the sealed-import rule does not restore: an
/// HONEST candidate that happens to hold a module named like one of the
/// suite's (`helper.ax`) is judged against the suite's helper and passes.
/// With the order reversed, the candidate's copy is the first match for the
/// suite's `use helper`: the sealed-import rule (M436) refuses the run (E0901,
/// Unknown), and without it the candidate's `want` defines the suite's helper
/// (Failed). The order is the only guard of the right verdict; the Fabric
/// twin of the guest runner's `a_candidate_cannot_shadow_the_suites_own_modules`
/// (M177). Fabric reports no interpreter stderr, so the discriminator is a
/// CONTROL: the same honest candidate WITHOUT `helper.ax` passes first, and
/// the two submissions differ only by that file, which the candidate itself
/// never imports. With the suite first it is never reached, so its presence
/// alone changing the verdict is the candidate's file being resolved.
#[test]
fn an_honest_candidate_holding_a_suite_module_name_is_judged_by_the_suite() {
    let (s, r) = suite_with_helper(&[]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "control: {:?} {:?}",
        sub.reason,
        sub.check_report
    );
    let (s, r) = suite_with_helper(&[("helper.ax", "fn want() -> i64 { 0 }\n")]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "ATTACK: the candidate's helper.ax was resolved before the suite's own helper (its \
         presence alone changed the verdict): {:?} {:?}",
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
    // Since PSV review wf_d725935a-7ed (B1) a sealed candidate's own @[test]
    // is never collected: it neither stands in for the named check nor runs
    // beside it. (It used to run; the exact-name rule already kept it from
    // counting.)
    assert_eq!(rep["passed"], json!([]), "the candidate's test did NOT run");
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
///
/// The failing sibling is the SUITE's own (`hidden_completion_z`): since PSV
/// review wf_d725935a-7ed a sealed candidate's @[test] is never collected, so
/// it can no longer serve as the vehicle this test used before.
#[test]
fn a_named_pass_in_a_run_that_exits_nonzero_is_not_a_pass() {
    let s = with_suite(
        "mod f\nuse f.{double}\n\n@[test]\nfn hidden_completion() { assert_eq(double(21), 42) }\n\n\
         @[test]\nfn hidden_completion_z() { assert(false) }\n",
        "hidden",
    );
    let r = suite_request(&s, "op-exit0");
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

/// Re-audit 5 blocker (executed): AXON_PATH is ':'-separated and built from
/// the caller-named state dir, so a ':' in it put caller directories ahead of
/// the suite and Fabric signed a pass the suite would fail. Such a state dir
/// is refused before anything is written or launched. Positive control: the
/// same request under an ordinary state dir runs.
///
/// Mutation: drop the ':' refusal in `submit` → red.
#[test]
fn a_state_dir_that_would_split_the_module_path_is_refused() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let mut cfg = s.env.cfg(0);
    let evil = s.env.dir.path().join("x:y");
    cfg.state_dir = evil.clone();
    // The candidate is published under the colon state dir too, so every
    // other check on this route passes: the ':' refusal is the only guard
    // (C9 round 1b; without this, the unpublished candidate was refused
    // elsewhere and the row was never killed by its own attack).
    WorkspaceStore::open(&evil, &tenant())
        .unwrap()
        .import_dir(&s.env.ws, &Quota::default())
        .unwrap();
    let got = submit(&suite_request(&s, "op-colon").to_string(), &cfg);
    let e = match got {
        Ok(sub) => panic!(
            "ATTACK: a check ran under a state dir containing ':' (its module path splits \
             into caller-chosen directories): {:?} {:?}",
            sub.receipt.verification, sub.reason
        ),
        Err(e) => e,
    };
    assert_eq!(e.kind(), "malformed", "{e}");
    assert!(e.to_string().contains("contains ':'"), "{e}");
    assert!(!evil.join("runs").exists(), "nothing was written under it");
    assert_untouched(&s.env, "colon state dir");
    // Positive control.
    let sub = submit(&suite_request(&s, "op-plain").to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        sub.reason
    );
}

/// Re-audit 5 blocker (executed): modules fell through AXON_PATH to ambient
/// dirs — the trial cache's `~/.axon/lib`, under the caller-named state dir —
/// so a candidate whose tree does NOT compile on its own passed with a module
/// planted there, and Fabric signed it. A check now resolves modules only from
/// its module path (AXON_PATH_EXCLUSIVE). Positive control: the same candidate
/// with the module shipped in its own tree passes.
///
/// Mutation: drop AXON_PATH_EXCLUSIVE from the check executor → red.
#[test]
fn a_check_loads_no_module_from_outside_the_suite_and_the_candidate() {
    // `use f::h` loads `f/h.ax` with no `mod` keyword, so the admission scan
    // sees nothing to refuse — resolution alone decides where it comes from.
    let uses_h = "use f::h\n\nfn double(n: i64) -> i64 { d2(n) }\n";
    let h = "fn d2(n: i64) -> i64 { n * 2 }\n";

    // Planted in the trial cache's HOME, absent from the candidate's tree.
    let (s, r) = suite_with_helper(&[("f.ax", uses_h)]);
    let cfg = s.env.cfg(0);
    let cache = axon_fabric::workspace::TrialCache::for_trial(
        &cfg.state_dir,
        &tenant(),
        &axon_loop_contracts::TrialId::new(r["trial_id"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let lib = cache.root.join("home/.axon/lib/f");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("h.ax"), h).unwrap();
    let sub = submit(&r.to_string(), &cfg).unwrap();
    assert_ne!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "a module from the trial cache judged the candidate: {:?} {:?}",
        sub.reason,
        sub.check_report
    );

    // Positive control: shipped in the tree, the same candidate passes.
    let (s, r) = suite_with_helper(&[("f.ax", uses_h), ("f/h.ax", h)]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        sub.reason
    );
}

/// v0.22 G01 final re-audit blocker (executed): a `break`/`continue` escaping
/// a candidate function unwound the operator's acceptance test before its
/// assertion ran, and the interpreter counted that as a PASS — so Fabric
/// signed a pass for a candidate the suite fails. The test did not complete,
/// so it did not pass. Positive control: the honest candidate passes.
///
/// Mutation: treat an escaped break/continue as clean again → red.
#[test]
fn an_escaped_break_does_not_pass_the_operators_test() {
    let (s, r) = suite_with_helper(&[(
        "f.ax",
        "fn double(n: i64) -> i64 {\n    if n > 0 { break }\n    n * 0\n}\n",
    )]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_ne!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?} {:?}",
        sub.reason,
        sub.check_report
    );
    let (s, r) = suite_with_helper(&[]);
    let sub = submit(&r.to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?}",
        sub.reason
    );
}

/// v0.22 G01 final re-audit blocker (executed): import checks a symlink's
/// target LEXICALLY, so a chain — `z/a/b -> ../..`, then `f/h.ax` through it —
/// stays "inside" the tree on paper and lands outside it once materialized.
/// The candidate's `use f::h` then loaded a module planted in the state dir,
/// and Fabric signed the pass. A check now refuses any tree holding a link,
/// before anything is launched. Positive control: the same candidate with a
/// real f/h.ax passes (a_check_loads_no_module_from_outside_the_suite_and_the_candidate).
///
/// Mutation: drop the candidate's link refusal in `check_suite_target` → red.
#[cfg(unix)]
#[test]
fn a_candidate_holding_a_symlink_is_refused() {
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
    let ws = &s.env.ws;
    std::fs::write(
        ws.join("f.ax"),
        "use f::h\n\nfn double(n: i64) -> i64 { d2(n) }\n",
    )
    .unwrap();
    std::fs::create_dir_all(ws.join("z/a")).unwrap();
    std::fs::create_dir_all(ws.join("f")).unwrap();
    std::os::unix::fs::symlink("../..", ws.join("z/a/b")).unwrap();
    std::os::unix::fs::symlink("../z/a/b/../../../plant/h.ax", ws.join("f/h.ax")).unwrap();
    let cfg = s.env.cfg(0);
    // The planted module: three levels above the materialized candidate.
    std::fs::create_dir_all(cfg.state_dir.join("plant")).unwrap();
    std::fs::write(
        cfg.state_dir.join("plant/h.ax"),
        "fn d2(n: i64) -> i64 { n * 2 }\n",
    )
    .unwrap();
    // Import accepts the chain (its check is lexical) — that is the hole.
    let candidate = WorkspaceStore::open(&cfg.state_dir, &tenant())
        .unwrap()
        .import_dir(ws, &Quota::default())
        .expect("import accepts the link chain");
    let s = Suite {
        candidate,
        suite_ref,
        ..s
    };
    let r = suite_request(&s, "op-link");
    let e = submit(&r.to_string(), &s.env.cfg(0)).unwrap_err();
    assert_eq!(e.kind(), "malformed", "{e}");
    assert!(e.to_string().contains("symbolic link"), "{e}");
    assert_untouched(&s.env, "candidate with a symlink");
}

/// Candidate 2's final-review blocker (executed): the operator's test LOOPS
/// over inputs, and a `break`/`continue` in the candidate's function landed
/// in that loop — ending it before any assertion ran — so the broken
/// candidate passed and Fabric signed it. Loop control no longer crosses a
/// function boundary. Positive control: the honest candidate passes the same
/// loop-shaped suite.
///
/// Mutation: let break/continue cross `call_fn` again → red.
#[test]
fn an_escaped_break_cannot_end_the_operators_test_loop() {
    let suite = "mod f\nuse f.{double}\n\n@[test]\nfn hidden_completion() {\n    for i in 1..4 {\n        assert_eq(double(i), i * 2)\n    }\n    let mut j = 1\n    while j < 4 {\n        assert_eq(double(j), j * 2)\n        j = j + 1\n    }\n}\n";
    for (why, body, pass) in [
        (
            "break",
            "fn double(n: i64) -> i64 {\n    if n > 0 { break }\n    n * 0\n}\n",
            false,
        ),
        (
            "continue",
            "fn double(n: i64) -> i64 {\n    if n > 0 { continue }\n    n * 0\n}\n",
            false,
        ),
        ("honest", "fn double(n: i64) -> i64 { n * 2 }\n", true),
    ] {
        let s = with_suite(suite, "hidden");
        std::fs::write(s.env.ws.join("f.ax"), body).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite { candidate, ..s };
        let sub = submit(&suite_request(&s, "op-loop").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// Candidate 4's second final-review blocker (executed): a candidate module
/// `use`d a suite helper and redefined its trait impl (or its `let` constant),
/// and the merged program kept the candidate's definition — so a broken
/// candidate passed the operator's test and Fabric signed it. A duplicate
/// definition in the merged program is now E0002, so the check does not pass.
/// Positive control: the honest candidate passes against the same suite.
///
/// Mutation: drop the impl (or let) uniqueness check in the resolver → red.
#[test]
fn a_candidate_cannot_redefine_a_suite_helpers_impl_or_constant() {
    let suite_with = |accept: &str, rubric: &str| {
        let s = with_suite(accept, "hidden");
        std::fs::write(s.suite_root.join("rubric.ax"), rubric).unwrap();
        let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
            .unwrap()
            .reference()
            .to_string();
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
        reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
        std::fs::write(&s.env.registry, reg.to_string()).unwrap();
        Suite { suite_ref, ..s }
    };
    let impl_accept = "mod f\nmod rubric\nuse f.{double}\nuse rubric.{Expect, Judge}\n\n@[test]\nfn hidden_completion() {\n    let e = Expect { want: 42 }\n    e.check(double(21))\n}\n";
    let impl_rubric = "type Expect = { want: i64 }\ntrait Judge { fn check(self: Expect, got: i64) }\nimpl Judge for Expect { fn check(self: Expect, got: i64) { assert_eq(got, self.want) } }\n";
    let helper_first = "mod rubric\nmod f\nuse rubric.{Expect, Judge}\nuse f.{double}\n\n@[test]\nfn hidden_completion() {\n    let e = Expect { want: 42 }\n    e.check(double(21))\n}\n";
    let let_accept = "mod f\nmod rubric\nuse f.{double}\nuse rubric.{WANT}\n\n@[test]\nfn hidden_completion() { assert_eq(double(21), WANT) }\n";
    let let_rubric = "let WANT = 42\n";
    for (why, accept, rubric, cand, pass) in [
        ("impl override", impl_accept, impl_rubric,
         "use rubric\n\nfn double(n: i64) -> i64 { n * 0 }\nimpl Judge for Expect { fn check(self: Expect, got: i64) { } }\n", false),
        ("impl honest", impl_accept, impl_rubric, "fn double(n: i64) -> i64 { n * 2 }\n", true),
        // PCI 2b: a DIFFERENT trait naming the same method on the suite's type.
        // Method calls dispatch on (type, method), so the later impl replaced
        // the suite's `check` — and needs no import of the rubric at all.
        // Live when the candidate's module is merged AFTER the helper (modules
        // merge in `use` order and the later definition won), so this suite
        // imports them that way. Executed: a signed pass for `n * 0`.
        ("second-trait method", helper_first, impl_rubric,
         "trait Other { fn check(self: Expect, got: i64) }\nimpl Other for Expect { fn check(self: Expect, got: i64) { } }\nfn double(n: i64) -> i64 { n * 0 }\n", false),
        ("second-trait honest", helper_first, impl_rubric, "fn double(n: i64) -> i64 { n * 2 }\n", true),
        ("let override", let_accept, let_rubric,
         "use rubric\n\nlet WANT = 0\nfn double(n: i64) -> i64 { n * 0 }\n", false),
        ("let honest", let_accept, let_rubric, "fn double(n: i64) -> i64 { n * 2 }\n", true),
    ] {
        let s = suite_with(accept, rubric);
        std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite { candidate, ..s };
        let sub = submit(&suite_request(&s, "op-override").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// Protected Check Isolation — AFFIRMATIVE completion evidence. A pass counts
/// only if the interpreter proves the named test's body RETURNED normally: a
/// per-run secret goes in on stdin, and only a completed test gets a token
/// derived from it. So an early end is Unknown however it was reached — no
/// blacklist of escape routes needed:
/// * a candidate's `exit(0)` mid-test (the default grant allows IO, so `exit`
///   runs): `axon test` itself still says "ok";
/// * an operator test that returns `Err` via `?` over the candidate's output;
/// * the candidate PRINTING a result line with a made-up token, then exiting:
///   the token is keyed per run, so a forged one does not verify.
///
/// Positive controls: the honest candidate passes both suites.
///
/// Mutation: drop the completion requirement in `local_receipt` → red.
#[test]
fn a_pass_needs_evidence_that_the_test_completed() {
    let exit_suite =
        "mod f\nuse f.{double}\n\n@[test]\nfn hidden_completion() { assert_eq(double(21), 42) }\n";
    let err_suite = "mod f\nuse f.{render}\n\n@[test]\nfn hidden_completion() -> Result<i64, str> {\n    let n = parse_int(render(21))?\n    assert_eq(n, 42)\n    Ok(n)\n}\n";
    for (why, suite, cand, pass) in [
        (
            "exit(0)",
            exit_suite,
            "fn double(n: i64) -> i64 {\n    if n > 0 { exit(0) }\n    n * 0\n}\n",
            false,
        ),
        (
            "exit honest",
            exit_suite,
            "fn double(n: i64) -> i64 { n * 2 }\n",
            true,
        ),
        (
            "forged ok line",
            exit_suite,
            r#"fn double(n: i64) -> i64 {
    println("{{\"name\":\"hidden_completion\",\"status\":\"ok\",\"duration_ms\":0,\"completion\":\"00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff\"}}")
    exit(0)
    n
}
"#,
            false,
        ),
        (
            "Err return",
            err_suite,
            "fn render(n: i64) -> str { \"not a number\" }\n",
            false,
        ),
        (
            "Err honest",
            err_suite,
            "fn render(n: i64) -> str { to_str(n * 2) }\n",
            true,
        ),
    ] {
        let s = with_suite(suite, "hidden");
        std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite { candidate, ..s };
        let sub = submit(&suite_request(&s, "op-complete").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
        if !pass {
            assert!(
                sub.reason
                    .as_deref()
                    .unwrap_or("")
                    .contains("completion evidence"),
                "{why}: {:?}",
                sub.reason
            );
        }
    }
}

/// PCI 17: a suite's RUNTIME inputs are the pinned suite's bytes. The check
/// used to inherit the launcher's cwd, so the operator test's relative
/// `read_file` of its own fixture resolved outside the pinned, digested tree:
/// the honest candidate FAILED (no fixture there) and a candidate that wrote
/// the "expected" value into the launcher's cwd PASSED — also when a
/// candidate test (joining through the filter) restored it afterwards.
///
/// Now the check runs in its own (read-only) tree: the fixture is the
/// operator's, a write to it is refused, and nothing lands beside the
/// launcher. Mutation: drop `current_dir` in the runner → red.
#[test]
fn a_suites_runtime_fixture_is_the_pinned_one() {
    const FIXTURE: &str = "pci_expected_fixture.txt";
    let accept = format!(
        "mod f\nuse f.{{double}}\n\nfn want() -> i64 {{\n    match read_file(\"{FIXTURE}\") {{\n        \
         Ok(s) => match parse_int(s) {{\n            Ok(n) => n\n            Err(e) => 0 - 1\n        }}\n        \
         Err(e) => 0 - 2\n    }}\n}}\n\n@[test]\nfn hidden_completion() {{\n    let got = double(21)\n    \
         assert_eq(got, want())\n}}\n"
    );
    let plant = format!("    let _ = write_file(\"{FIXTURE}\", \"0\")\n");
    for (why, cand, pass) in [
        ("plant", format!("fn double(n: i64) -> i64 {{\n{plant}    0\n}}\n"), false),
        (
            "plant+restore",
            format!(
                "fn double(n: i64) -> i64 {{\n{plant}    0\n}}\n\n@[test]\n\
                 fn hidden_completion_zrestore() {{\n    let _ = write_file(\"{FIXTURE}\", \"42\")\n}}\n"
            ),
            false,
        ),
        ("honest", "fn double(n: i64) -> i64 { n * 2 }\n".to_string(), true),
    ] {
        let s = with_suite(&accept, "hidden");
        std::fs::write(s.suite_root.join(FIXTURE), "42").unwrap();
        let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
            .unwrap()
            .reference()
            .to_string();
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
        reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
        std::fs::write(&s.env.registry, reg.to_string()).unwrap();
        std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite {
            candidate,
            suite_ref,
            ..s
        };
        let sub = submit(&suite_request(&s, "op-fixture").to_string(), &s.env.cfg(0)).unwrap();
        let planted = std::env::current_dir().unwrap().join(FIXTURE);
        let leaked = planted.exists();
        let _ = std::fs::remove_file(&planted);
        assert!(!leaked, "{why}: the check wrote into the launcher's cwd");
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// PCI 7/8/13 in the SIGNABLE profile (empty ceiling): candidate code cannot
/// end the operator's test as a normal completion by a transfer out of its own
/// frame. A `return` inside a candidate's parameter refinement, return
/// refinement or `@[verify]` predicate used to unwind the operator's test to a
/// clean finish with a valid completion token, and Fabric signed-off a pass for
/// `n * 0` (PCI candidate-1 review, executed). The frame edge is now an
/// allowlist. Honest control: Passed.
///
/// Mutation: let `Flow::Return` through `contain_frame` → red.
#[test]
fn a_candidate_predicate_cannot_end_the_operators_test() {
    const GRANT_PURE: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";
    let suite =
        "mod f\nuse f.{double}\n\n@[test]\nfn hidden_completion() { assert_eq(double(21), 42) }\n";
    for (why, cand, pass) in [
        (
            "param refinement",
            "fn double(n: i64 where if n > 0 { return 0 } else { true }) -> i64 { n * 0 }\n",
            false,
        ),
        (
            "return refinement",
            "fn double(n: i64) -> (i64 where if _ == 0 { return 0 } else { true }) { n * 0 }\n",
            false,
        ),
        (
            "verify",
            "@[verify(if value == 0 { return 0 } else { true })]\nfn double(n: i64) -> i64 { n * 0 }\n",
            false,
        ),
        ("honest", "fn double(n: i64) -> i64 { n * 2 }\n", true),
    ] {
        let s = with_suite(suite, "hidden");
        write_grant_registry(&s.env.grant_registry, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
        std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite { candidate, ..s };
        let sub = submit(&suite_request(&s, "op-frame").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.ran_under.as_ref().map(|r| r.effect_ceiling.as_str()),
            Some(""),
            "{why}: not the signable (empty-ceiling) profile"
        );
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// PCI 21 in the SIGNABLE profile (empty ceiling): the candidate under test is
/// SEALED — it cannot name anything the operator's suite defines. The merged
/// program used to be one global namespace: a candidate mutated the suite's
/// reference-shared `Dict` constant (`dict_set` is effect-free, so the empty
/// ceiling did not refuse it) or called its hidden helper for the answer, and
/// Fabric signed-off a pass for `n * 0` (PCI candidate-2 review, executed).
/// A refinement named after the suite's generic parameter (`type T = …`) is
/// refused too. Honest control: Passed.
///
/// Mutation: drop `with_sealed_dir` in submit → red.
#[test]
fn a_sealed_candidate_cannot_reach_the_operators_names() {
    const GRANT_PURE: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";
    let accept = "mod f\nmod rubric\nuse f.{double}\nuse rubric.{expected, same}\n\n@[test]\nfn hidden_completion() {\n    assert_eq(double(21), dict_get_or(expected(), \"d21\", 0 - 1))\n    assert(same(double(1), 2))\n}\n";
    let rubric = "fn build() -> Dict {\n    let d = dict_new()\n    dict_set(d, \"d21\", 42)\n    d\n}\nlet TABLE = build()\nfn expected() -> Dict { TABLE }\nfn same<T>(a: T, b: T) -> bool { a == b }\nfn answer_of(n: i64) -> i64 { n * 2 }\ntrait Answers { fn answer(self) -> i64 }\nimpl Answers for i64 { fn answer(self: i64) -> i64 { self * 2 } }\n";
    for (why, cand, pass) in [
        (
            "mutates the suite's constant",
            "fn double(n: i64) -> i64 {\n    dict_set(TABLE, \"d21\", 0)\n    n * 0\n}\n",
            false,
        ),
        (
            "reads the answer key",
            "fn double(n: i64) -> i64 {\n    if n == 21 { dict_get_or(expected(), \"d21\", 0) } else { 2 }\n}\n",
            false,
        ),
        // PCI candidate-3 review: routes the static walk missed, each a
        // signed-off pass — held now by the RUNTIME edges.
        (
            "calls an operator method on its own value",
            "fn double(n: i64) -> i64 { n.answer() }\n",
            false,
        ),
        (
            "names an operator function in a string",
            "fn double(n: i64) -> i64 {\n    let id = scheduler_spawn(\"answer_of\", n)\n    scheduler_run()\n    scheduler_result(id)\n}\n",
            false,
        ),
        (
            "reads the answer key in a match guard",
            "fn double(n: i64) -> i64 {\n    match n {\n        x if x == 21 && dict_get_or(TABLE, \"d21\", 0) > 0 => dict_get_or(TABLE, \"d21\", 0)\n        _ => 2\n    }\n}\n",
            false,
        ),
        (
            "refinement named after a generic parameter",
            "type T = i64 where _ >= 0\nfn double(n: i64) -> i64 { n * 2 }\n",
            false,
        ),
        ("honest", "fn double(n: i64) -> i64 { n * 2 }\n", true),
    ] {
        let s = with_suite(accept, "hidden");
        std::fs::write(s.suite_root.join("rubric.ax"), rubric).unwrap();
        let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
            .unwrap()
            .reference()
            .to_string();
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
        reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
        std::fs::write(&s.env.registry, reg.to_string()).unwrap();
        write_grant_registry(&s.env.grant_registry, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
        std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite {
            candidate,
            suite_ref,
            ..s
        };
        let sub = submit(&suite_request(&s, "op-sealed").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.ran_under.as_ref().map(|r| r.effect_ceiling.as_str()),
            Some(""),
            "{why}: not the signable profile"
        );
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// PCI 4, isolated from sealing: the operator's OWN module resolution is
/// confined too. A suite naming a module it does not ship must not pick one up
/// from the trial cache's HOME (`~/.axon/lib`) — a previous trial could have
/// written it, and that code would judge this candidate. With the path
/// exclusive the module is not found (not a pass); honest control: shipped in
/// the suite, the same suite passes.
///
/// Mutation: drop Fabric's `AXON_PATH_EXCLUSIVE` → the planted module judges
/// the candidate → Passed → red.
#[test]
fn a_suite_module_never_resolves_from_the_trial_cache() {
    // The suite ships `lib.ax` (declared by `mod lib`, so the admission scan
    // accepts it), and `use lib::extra` loads `lib/extra.ax` with no `mod`
    // line of its own: the scan sees nothing, resolution alone decides.
    let accept = "mod f\nmod lib\nuse f.{double}\nuse lib::extra\n\n@[test]\nfn hidden_completion() { assert_eq(double(21), want()) }\n";
    let lib_ax = "fn lib_version() -> i64 { 1 }\n";
    let planted = "fn want() -> i64 { 0 }\n";
    // The candidate is wrong for the real answer (42) but right for the planted one.
    let cand = "fn double(n: i64) -> i64 { n * 0 }\n";
    let s = with_suite(accept, "hidden");
    std::fs::write(s.suite_root.join("lib.ax"), lib_ax).unwrap();
    let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
    reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
    std::fs::write(&s.env.registry, reg.to_string()).unwrap();
    std::fs::write(s.env.ws.join("f.ax"), cand).unwrap();
    let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
        .unwrap()
        .import_dir(&s.env.ws, &Quota::default())
        .unwrap();
    let s = Suite {
        candidate,
        suite_ref,
        ..s
    };
    let r = suite_request(&s, "op-ambient");
    let cfg = s.env.cfg(0);
    let cache = axon_fabric::workspace::TrialCache::for_trial(
        &cfg.state_dir,
        &tenant(),
        &axon_loop_contracts::TrialId::new(r["trial_id"].as_str().unwrap()).unwrap(),
    )
    .unwrap();
    let lib = cache.root.join("home/.axon/lib/lib");
    std::fs::create_dir_all(&lib).unwrap();
    std::fs::write(lib.join("extra.ax"), planted).unwrap();
    let sub = submit(&r.to_string(), &cfg).unwrap();
    assert_ne!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "a module from the trial cache judged the candidate: {:?} {:?}",
        sub.reason,
        sub.check_report
    );

    // Control: the suite ships `extra` (the real answer); the honest candidate passes.
    let accept2 = accept;
    let s = with_suite(accept2, "hidden");
    std::fs::write(s.suite_root.join("lib.ax"), lib_ax).unwrap();
    std::fs::create_dir_all(s.suite_root.join("lib")).unwrap();
    std::fs::write(
        s.suite_root.join("lib/extra.ax"),
        "fn want() -> i64 { 42 }\n",
    )
    .unwrap();
    let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
        .unwrap()
        .reference()
        .to_string();
    let mut reg: Value =
        serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
    reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
    std::fs::write(&s.env.registry, reg.to_string()).unwrap();
    std::fs::write(
        s.env.ws.join("f.ax"),
        "fn double(n: i64) -> i64 { n * 2 }\n",
    )
    .unwrap();
    let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
        .unwrap()
        .import_dir(&s.env.ws, &Quota::default())
        .unwrap();
    let s = Suite {
        candidate,
        suite_ref,
        ..s
    };
    let sub = submit(
        &suite_request(&s, "op-ambient-ok").to_string(),
        &s.env.cfg(0),
    )
    .unwrap();
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "{:?} {:?}",
        sub.reason,
        sub.check_report
    );
}

/// PCI 21, candidate-4 review, SIGNABLE profile (empty ceiling):
/// * handle-addressed kernel state — a sealed candidate read the result of the
///   OPERATOR's scheduler fiber by guessing its id (`scheduler_result(0)`), a
///   pass for a candidate that computes nothing. A sealed frame now has its OWN
///   kernel, so an operator id does not exist from it;
/// * definition provenance — a candidate struct's whole-struct `where` ran
///   unsealed when the OPERATOR built one, and called operator code from there.
///   It now runs under its type's (sealed) provenance.
///
/// Honest control: Passed.
///
/// Mutation: route every frame to the operator's kernel → red.
#[test]
fn a_sealed_candidate_cannot_reach_operator_state_by_handle_or_definition() {
    const GRANT_PURE: &str = "\
profile = \"restricted\"
[grant]
max_label = \"internal\"
[grant.budget]
cost_micro = 1000
";
    let accept = "mod f\nmod rubric\nuse f.{solve, Range}\nuse rubric.{reference}\n\n@[test]\nfn hidden_completion() {\n    let r = Range { lo: 1, hi: 2 }\n    let id = scheduler_spawn(\"reference\", 21)\n    scheduler_run()\n    assert_eq(solve(21), scheduler_result(id))\n}\n";
    let rubric =
        "fn reference(n: i64) -> i64 { n * 7 + 5 }\nfn secret(n: i64) -> i64 { n * 7 + 5 }\n";
    let honest_range = "type Range = { lo: i64, hi: i64 }\n";
    for (why, cand, pass) in [
        (
            "reads the operator's fiber result by id",
            format!("{honest_range}fn solve(n: i64) -> i64 {{ scheduler_result(0) }}\n"),
            false,
        ),
        (
            "struct where built by the operator calls operator code",
            "let STASH = dict_new()\n\
             type Range = { lo: i64, hi: i64 } where stash(_.lo)\n\
             fn stash(x: i64) -> bool {\n    let id = scheduler_spawn(\"secret\", 21)\n    scheduler_run()\n    dict_set(STASH, \"k\", scheduler_result(id))\n    true\n}\n\
             fn solve(n: i64) -> i64 { dict_get_or(STASH, \"k\", 0 - 1) }\n"
                .to_string(),
            false,
        ),
        (
            "honest",
            format!("{honest_range}fn solve(n: i64) -> i64 {{ n * 7 + 5 }}\n"),
            true,
        ),
    ] {
        let s = with_suite(accept, "hidden");
        std::fs::write(s.suite_root.join("rubric.ax"), rubric).unwrap();
        let suite_ref = WorkspaceTree::import_dir(&s.suite_root, &Quota::default())
            .unwrap()
            .reference()
            .to_string();
        let mut reg: Value =
            serde_json::from_str(&std::fs::read_to_string(&s.env.registry).unwrap()).unwrap();
        reg["checks"][0]["workspace_version_ref"] = json!(suite_ref);
        std::fs::write(&s.env.registry, reg.to_string()).unwrap();
        write_grant_registry(&s.env.grant_registry, &[("grant:test", PRINCIPAL, GRANT_PURE)]);
        std::fs::write(s.env.ws.join("f.ax"), &cand).unwrap();
        let candidate = WorkspaceStore::open(&s.env.cfg(0).state_dir, &tenant())
            .unwrap()
            .import_dir(&s.env.ws, &Quota::default())
            .unwrap();
        let s = Suite {
            candidate,
            suite_ref,
            ..s
        };
        let sub = submit(&suite_request(&s, "op-kernel").to_string(), &s.env.cfg(0)).unwrap();
        assert_eq!(
            sub.ran_under.as_ref().map(|r| r.effect_ceiling.as_str()),
            Some(""),
            "{why}: not the signable profile"
        );
        assert_eq!(
            sub.receipt.verification == ReceiptVerification::Passed,
            pass,
            "{why}: {:?} {:?}",
            sub.reason,
            sub.check_report
        );
    }
}

/// C9 round 4c, amendment 71 (M1655): a run dir left by a crashed Fabric
/// whose restart has the SAME pid (PID 1 in a container) does not refuse the
/// retry. The crashed process's names were `<op16>-<pid>-<seq>`; every one
/// of them for this pid, sequence 0..4096, is planted here holding a stale
/// file, as a crash would leave them. The retry runs, and none of the
/// leftovers is reused (each still holds its stale file and nothing else).
#[test]
fn a_leftover_run_dir_of_a_crashed_process_with_the_same_pid_does_not_refuse_the_retry() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let cfg = s.env.cfg(0);
    let op = "op-crashed";
    let runs = cfg.state_dir.join("runs");
    let op16 = &axon_psv::sha256_hex(op.as_bytes())[..16];
    let pid = std::process::id();
    for seq in 0..4096u32 {
        let d = runs.join(format!("{op16}-{pid}-{seq}"));
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("stale"), "left by the crashed process").unwrap();
    }
    assert_eq!(
        std::fs::read_dir(&runs).unwrap().count(),
        4096,
        "setup: the crashed process's run dirs are in place"
    );
    let sub = submit(&suite_request(&s, op).to_string(), &cfg);
    let sub = sub.unwrap_or_else(|e| {
        panic!(
            "ATTACK: a leftover run dir of a crashed process with the same pid refused the \
             retry: {e}"
        )
    });
    assert_eq!(
        sub.receipt.verification,
        ReceiptVerification::Passed,
        "control: the retry ran: {:?}",
        sub.reason
    );
    let left: Vec<_> = std::fs::read_dir(&runs).unwrap().flatten().collect();
    assert_eq!(
        left.len(),
        4096,
        "the run's own dir is gone, the leftovers untouched"
    );
    for e in left {
        let names: Vec<_> = std::fs::read_dir(e.path())
            .unwrap()
            .flatten()
            .map(|x| x.file_name())
            .collect();
        assert_eq!(
            names,
            vec![std::ffi::OsString::from("stale")],
            "a leftover was reused"
        );
    }
}

// ── C9 round 4c, EQGATE (amendment 81; M1934-M1935): the post-run suite check ─
//
// `post_run` re-imports the operator suite's materialized copy after the run
// and compares it with the version the registry pinned. A suite that CHANGED
// during the run, or can no longer be read, demotes the verdict to "no verdict":
// both arms build `problem = Some(..)` (not an `Err`), so the refusal-coverage
// gate did not see them, and no test touched them. The `pre_launch_hook` runs
// after the suite is materialized and before the launch, which is the same
// state a run that rewrote its own check dir leaves behind.

fn run_check_dir(cfg: &axon_fabric::SubmitConfig) -> PathBuf {
    let runs = cfg.state_dir.join("runs");
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(&runs)
        .unwrap()
        .flatten()
        .map(|e| e.path().join("check"))
        .filter(|p| p.is_dir())
        .collect();
    assert_eq!(dirs.len(), 1, "setup: exactly one run dir has a check suite: {dirs:?}");
    dirs.remove(0)
}

fn change_the_suite_during_the_run(cfg: &axon_fabric::SubmitConfig) {
    let d = run_check_dir(cfg);
    let p = d.join("h.ax");
    let mut b = std::fs::read(&p).unwrap();
    b.extend_from_slice(b"// changed while the run held it\n");
    // The materialized copy is read-only; the run's own uid owns it.
    let _ = std::fs::set_permissions(&d, std::os::unix::fs::PermissionsExt::from_mode(0o755));
    let _ = std::fs::set_permissions(&p, std::os::unix::fs::PermissionsExt::from_mode(0o644));
    std::fs::write(&p, b).unwrap();
}

fn make_the_suite_unreadable_after_the_run(cfg: &axon_fabric::SubmitConfig) {
    let d = run_check_dir(cfg);
    let _ = std::fs::set_permissions(&d, std::os::unix::fs::PermissionsExt::from_mode(0o755));
    let c = std::ffi::CString::new(d.join("pipe").to_str().unwrap()).unwrap();
    // SAFETY: mkfifo with a valid path.
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0, "setup: mkfifo");
}

#[test]
fn a_suite_that_changed_during_the_run_yields_no_verdict() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let control = submit(&suite_request(&s, "op-suite-control").to_string(), &s.env.cfg(0)).unwrap();
    assert_eq!(
        control.receipt.verification,
        ReceiptVerification::Passed,
        "control: {:?}",
        control.reason
    );
    let mut cfg = s.env.cfg(0);
    cfg.pre_launch_hook = Some(change_the_suite_during_the_run);
    let sub = submit(&suite_request(&s, "op-suite-changed").to_string(), &cfg).unwrap();
    if sub.receipt.verification == ReceiptVerification::Passed {
        panic!(
            "ATTACK: a verdict was receipted Passed over a check suite that changed during the \
             run: {:?}",
            sub.reason
        );
    }
    assert!(
        sub.reason.as_deref().is_some_and(|r| r.contains("changed during the run")),
        "{:?}",
        sub.reason
    );
}

#[test]
fn a_suite_that_cannot_be_read_after_the_run_yields_no_verdict() {
    let s = with_suite(&hidden_suite_src(), "hidden");
    let mut cfg = s.env.cfg(0);
    cfg.pre_launch_hook = Some(make_the_suite_unreadable_after_the_run);
    let sub = submit(&suite_request(&s, "op-suite-unreadable").to_string(), &cfg).unwrap();
    if sub.receipt.verification == ReceiptVerification::Passed {
        panic!(
            "ATTACK: a verdict was receipted Passed over a check suite that could not be read \
             after the run: {:?}",
            sub.reason
        );
    }
    assert!(
        sub.reason.as_deref().is_some_and(|r| r.contains("unreadable after the run")),
        "{:?}",
        sub.reason
    );
}
