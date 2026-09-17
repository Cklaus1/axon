//! Integration tests: start a real HTTP server, exercise every route.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

/// Resolve the `axon` binary these tests proxy to.
///
/// This used to be `env::var("AXON_BIN").unwrap_or("true")` — literally
/// `/bin/true`, which outputs nothing and exits 0. So a module whose own doc
/// comment says "exercise every route" exercised them against a binary that does
/// NOTHING: 17 tests passing in 0.1s, proving the server's routing and its JSON
/// merging and not one thing about the CLI it wraps.
///
/// That mattered. `intent compile --json` reported a path and wrote no file, and
/// this suite was green throughout — the defect only surfaced when the e2e test
/// was run with a real binary by hand.
///
/// Now: explicit `AXON_BIN` wins (except the "true" sentinel), else the
/// workspace build is auto-discovered, else `/bin/true` with a LOUD note. The
/// fallback is announced rather than silent, because "these tests passed" and
/// "these tests ran against nothing" must not look the same.
fn resolve_axon_bin() -> String {
    if let Ok(b) = std::env::var("AXON_BIN") {
        if b != "true" && !b.is_empty() {
            return b;
        }
    }
    let workspace =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/axon");
    if let Ok(p) = workspace.canonicalize() {
        if p.exists() {
            return p.to_string_lossy().into_owned();
        }
    }
    eprintln!(
        "axon-web tests: NO axon binary found (built target/debug/axon, or set AXON_BIN) — \
         falling back to /bin/true. Routing and HTML assertions still mean something; \
         anything that proxies to the CLI does NOT."
    );
    "true".into()
}

fn start_server_thread(port: u16) {
    start_server_thread_with(port, resolve_axon_bin());
}

fn start_server_thread_with(port: u16, axon_bin: String) {
    let addr = format!("127.0.0.1:{port}");
    thread::spawn(move || {
        let srv = tiny_http::Server::http(&addr).expect("bind");
        for req in srv.incoming_requests() {
            crate::server::handle(req, &axon_bin);
        }
    });
    // Give the thread a moment to bind
    thread::sleep(Duration::from_millis(80));
}

fn get(port: u16, path: &str) -> (u16, String) {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    let req = format!("GET {path} HTTP/1.0\r\nHost: localhost\r\n\r\n");
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    let status = resp
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let body = resp
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (status, body)
}

fn post_json(port: u16, path: &str, body: &str) -> (u16, String) {
    let mut s = TcpStream::connect(format!("127.0.0.1:{port}")).expect("connect");
    let req = format!(
        "POST {path} HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
        body.len(),
        body
    );
    s.write_all(req.as_bytes()).unwrap();
    let mut resp = String::new();
    s.read_to_string(&mut resp).unwrap();
    let status = resp
        .split_whitespace()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    let resp_body = resp
        .split_once("\r\n\r\n")
        .map(|x| x.1)
        .unwrap_or("")
        .to_string();
    (status, resp_body)
}

#[test]
fn get_root_serves_html() {
    start_server_thread(18080);
    let (status, body) = get(18080, "/");
    assert_eq!(status, 200, "expected 200 got {status}");
    assert!(
        body.contains("<!DOCTYPE html>"),
        "expected HTML, got: {body:.200}"
    );
    assert!(body.contains("Axon Goal Approval Flow"), "title missing");
}

#[test]
fn get_index_html_serves_html() {
    start_server_thread(18081);
    let (status, body) = get(18081, "/index.html");
    assert_eq!(status, 200);
    assert!(body.contains("<!DOCTYPE html>"));
}

#[test]
fn unknown_route_returns_404_json() {
    start_server_thread(18082);
    let (status, body) = get(18082, "/not/a/thing");
    assert_eq!(status, 404);
    let v: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
    assert!(v["error"].is_string());
}

#[test]
fn post_intent_compile_returns_json() {
    start_server_thread(18083);
    let payload = "{\"content\":\"Goal: test something.\"}";
    let (status, body) = post_json(18083, "/api/intent/compile", payload);
    assert_eq!(status, 200, "status: {status}, body: {body:.200}");
    // Body must be valid JSON regardless of axon binary availability
    let _: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn post_deploy_returns_json() {
    start_server_thread(18084);
    let payload = "{\"content\":\"fn main() { println(\\\"ok\\\") }\",\"risk\":\"low\"}";
    let (status, body) = post_json(18084, "/api/deploy", payload);
    assert_eq!(status, 200, "status: {status}");
    let _: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn get_trace_returns_json() {
    start_server_thread(18085);
    let (status, body) = get(18085, "/api/trace");
    assert_eq!(status, 200);
    let _: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn post_goal_improve_returns_json() {
    start_server_thread(18086);
    let payload = "{\"content\":\"fn main() { println(\\\"ok\\\") }\"}";
    let (status, body) = post_json(18086, "/api/goal/improve", payload);
    assert_eq!(status, 200, "status: {status}, body: {body:.200}");
    let v: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
    assert_eq!(
        v["schema"], "axon-goal-improve/1",
        "wrong schema in goal/improve response"
    );
}

#[test]
fn html_contains_all_panes() {
    let html = crate::html::INDEX_HTML;
    for step in [
        "Intent",
        "AST Review",
        "Approve",
        "Improve",
        "Red Team",
        "Deploy",
        "Trace",
    ] {
        assert!(html.contains(step), "HTML missing pane: {step}");
    }
    // Every API endpoint referenced in the JS
    for ep in [
        "/api/intent/compile",
        "/api/ast/review",
        "/api/ast/approve",
        "/api/goal/improve",
        "/api/redteam",
        "/api/deploy",
        "/api/trace",
    ] {
        assert!(html.contains(ep), "HTML missing endpoint ref: {ep}");
    }
}

#[test]
fn html_state_machine_lockall_and_unlock() {
    let html = crate::html::INDEX_HTML;
    // State machine: lockAll disables all buttons, unlockByState re-enables by state
    assert!(html.contains("lockAll"), "missing lockAll");
    assert!(html.contains("unlockByState"), "missing unlockByState");
    assert!(html.contains("running = true"), "missing running lock");
    assert!(html.contains("running = false"), "missing running unlock");
    // done state flags
    for flag in [
        "done.compiled",
        "done.reviewed",
        "done.approved",
        "done.redteamed",
    ] {
        assert!(html.contains(flag), "missing state flag: {flag}");
    }
}

/// The review pane's capability summary reads `j.fns` -- and a wrong field name
/// there fails SILENTLY: the summary renders empty and the pane looks exactly
/// as it did before the summary existed. So assert the name against the RUNNING
/// server rather than against the HTML, which cannot tell a correct key from a
/// plausible one. (The first draft of the summary read `j.functions`.)
#[test]
fn ast_review_response_uses_the_field_names_the_review_pane_reads() {
    start_server_thread(18091);
    let payload = "{\"content\":\"@[contained(fs: [read(\\\"./data/\\\")], exec: none)]\\nfn narrow(n: i64) -> i64 { n }\\nfn wide(n: i64) -> i64 { n }\\nfn main() { println(to_str(narrow(1) + wide(2))) }\"}";
    let (status, body) = post_json(18091, "/api/ast/review", payload);
    assert_eq!(status, 200, "status: {status}, body: {body:.300}");
    let j: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.300}"));

    // Skip when the axon binary is unavailable in this environment -- an empty
    // fns array then means "nothing ran", not "the key is wrong", and asserting
    // through it would turn a missing toolchain into a false failure.
    let fns = match j.get("fns").and_then(|v| v.as_array()) {
        Some(f) if !f.is_empty() => f.clone(),
        _ => {
            eprintln!("skip: no `fns` in review output (axon binary unavailable?): {body:.300}");
            return;
        }
    };

    // The three keys the pane indexes. `contained` must be PRESENT on every fn
    // -- absent and null are different claims, and the pane distinguishes them.
    for f in &fns {
        assert!(f.get("name").is_some(), "fn entry lacks `name`: {f}");
        assert!(
            f.get("effect_set").is_some(),
            "fn entry lacks `effect_set`: {f}"
        );
        assert!(
            f.get("contained").is_some(),
            "`contained` must be present (null when undeclared), not omitted: {f}"
        );
    }

    // PRIMARY: the contained fn and the unconstrained one must be
    // distinguishable through exactly the keys the pane reads.
    let narrow = fns.iter().find(|f| f["name"] == "narrow").expect("narrow");
    let wide = fns.iter().find(|f| f["name"] == "wide").expect("wide");
    assert!(
        narrow["contained"].is_object(),
        "a fn with @[contained] must carry the grant object: {narrow}"
    );
    assert!(
        wide["contained"].is_null(),
        "a fn with no boundary must be null, not an empty grant object: {wide}"
    );
    assert_eq!(
        narrow["contained"]["read"][0], "./data/",
        "the read grant must survive to the reviewer: {narrow}"
    );
    assert_eq!(narrow["contained"]["exec"], serde_json::Value::Bool(false));
}

/// `axon redteam` distinguishes "the red team ran and found nothing"
/// (`status: "safe"`) from "there was no red team" (`status: "no_redteam_fn"`).
/// BOTH carry `caught: false` and neither sets `error`, so a pane keying its
/// green branch on `caught` alone collapses them -- and a program with no
/// `redteam_check` function at all rendered as "no adversarial issues found"
/// and unlocked Deploy.
///
/// Same defect as the deploy pane's, one step earlier in the same flow: an
/// absent check reported as a passed check. The reviewer must be told which of
/// the two happened, because only one of them is evidence.
#[test]
fn redteam_pane_separates_no_redteam_fn_from_a_clean_pass() {
    let html = crate::html::INDEX_HTML;
    let handler = html
        .split("async function runRedteam")
        .nth(1)
        .expect("runRedteam handler must exist");
    let end = handler.find("async function").unwrap_or(handler.len());
    let body: String = handler[..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    // The branch must exist and must read the field that carries the
    // distinction. `caught` cannot express it -- both statuses set it false.
    assert!(
        body.contains("j.status === 'no_redteam_fn'"),
        "the pane must branch on `status`, the only field separating an absent \
         red team from a clean one; got:\n{body}"
    );

    // PRIMARY: the two outcomes must not render the same words. This is the
    // whole test -- a message that says "no adversarial issues found" when
    // nothing was examined is the defect, however the branch is spelled.
    let absent_at = body.find("no_redteam_fn").expect("absent branch");
    let absent_msg_end = body[absent_at..]
        .find("} else")
        .map(|i| absent_at + i)
        .unwrap_or(body.len());
    let absent_msg = &body[absent_at..absent_msg_end];
    assert!(
        !absent_msg.contains("no adversarial issues found"),
        "the absent-red-team branch must not claim a clean result: {absent_msg}"
    );
    assert!(
        absent_msg.contains("NOTHING WAS RED-TEAMED"),
        "the absent-red-team branch must say so unmissably: {absent_msg}"
    );
}

/// `axon-goal-improve/1`'s `ok` field means the program RAN, not that anything
/// was optimized. A file with no `@[adaptive]` function runs cleanly and returns
/// `ok: true, best_score: null, trajectory: []` -- and the pane reported
/// "optimization complete".
///
/// Milder than the redteam case (Improve is a demonstration step, not a safety
/// gate) but the same collapse: a message asserting a result that was never
/// produced. `trajectory` is the field that carries the distinction -- the API
/// filters it to adaptive fns with more than one eval, so a non-empty
/// trajectory is exactly "something was actually optimized".
#[test]
fn improve_pane_separates_ran_from_actually_optimized() {
    let html = crate::html::INDEX_HTML;
    let handler = html
        .split("async function runImprove")
        .nth(1)
        .expect("runImprove handler must exist");
    let end = handler.find("async function").unwrap_or(handler.len());
    let body: String = handler[..end]
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");

    // Scope to the DECISION branch. The handler also builds a summary string
    // that reads `j.trajectory` for display, so a whole-body search for
    // "trajectory" -- or any assertion of the form "the guard appears before
    // the claim" -- is satisfied by the pre-fix code purely by source order.
    // RED-proving caught exactly that: two earlier drafts of this test passed
    // against the broken build. The question is not whether the trajectory is
    // mentioned; it is whether the CLAIM depends on it.
    let decision_at = body
        .find("if (j.ok !== false")
        .expect("the ok/error decision branch must exist");
    let decision = &body[decision_at..];

    // PRIMARY: within the success arm, "optimization complete" must be reached
    // only through a trajectory test. Anything else prints a result for a run
    // that produced none.
    let claim_at = decision
        .find("optimization complete")
        .expect("the success message must exist");
    let before_claim = &decision[..claim_at];
    assert!(
        before_claim.contains("trajectory") && before_claim.contains("length > 0"),
        "'optimization complete' must sit behind a non-empty-trajectory test \
         inside the success arm; the arm reads:\n{before_claim}"
    );
    assert!(
        decision.contains("NOTHING WAS OPTIMIZED"),
        "the no-adaptive-fn case must say so rather than claiming a result"
    );
}

/// AUDIT T50 (P4-PROD-09/P4-PROD-10): the approval-flow gates must key on the
/// fields the CLI schemas actually emit, and staging must be content-addressed.
#[test]
fn approval_flow_gates_key_on_real_schema_fields_t50() {
    let html = crate::html::INDEX_HTML;

    // (1) `axon-ast-review/1` reports `errors` (an ARRAY), never `error`. The
    // review pane gated on `j.error`, so a program that failed to type-check
    // set done.reviewed = true and unlocked Approve. Verified against the
    // running server: reviewing `let x: i64 = "not an int"` returns
    // `{"errors":["[E0102] ..."]}` with no `error` field at all.
    assert!(
        html.contains("Array.isArray(j.errors)"),
        "review must gate on the `errors` array the schema emits, not `j.error`"
    );

    // (2) `done.redteamed = true` used to sit OUTSIDE the caught/passed branches,
    // so a redteam that CAUGHT something still unlocked Deploy — falsifying the
    // documented Acid-Test-4 gating claim. It must now be set only where the
    // redteam passed.
    let redteam_fn = html
        .split("async function runRedteam")
        .nth(1)
        .expect("runRedteam handler must exist");
    let end = redteam_fn
        .find("async function")
        .unwrap_or(redteam_fn.len());
    let whole = &redteam_fn[..end];
    // Strip `//` comment lines first: the explanatory comment above the branch
    // quotes `done.redteamed = true`, and a naive `find` reads the COMMENT
    // rather than the code — which would make this test pass or fail on prose.
    let body: String = whole
        .lines()
        .filter(|l| !l.trim_start().starts_with("//"))
        .collect::<Vec<_>>()
        .join("\n");
    let passed_at = body
        .find("redteam passed")
        .expect("the pass branch must exist");
    let flag_at = body
        .find("done.redteamed = true")
        .expect("the flag must be set somewhere");
    let caught_at = body.find("if (caught)").expect("caught branch");
    // Scope to the CAUGHT branch itself -- from `if (caught)` to the `} else`
    // that closes it -- rather than to everything before the pass branch. The
    // chain legitimately grew a third arm (`no_redteam_fn`) that sits between
    // the two and sets the flag, so "before the pass branch" stopped meaning
    // "on the caught path". The invariant under test was never about ordering:
    // it is that a redteam which CAUGHT something must not unlock Deploy.
    let caught_body = &body[caught_at..];
    let caught_end = caught_body
        .find("} else")
        .expect("the caught branch must be closed by an else");
    assert!(
        !caught_body[..caught_end].contains("done.redteamed = true"),
        "done.redteamed must not be set on the CAUGHT path"
    );
    // And it must still be set somewhere at or after the caught branch closes,
    // i.e. on one of the non-caught arms -- never unconditionally before the
    // if/else chain, which is the original T50 defect.
    assert!(
        flag_at > caught_at,
        "done.redteamed must be set inside a non-caught branch, not before the chain"
    );
    assert!(
        body[passed_at..].contains("done.redteamed = true"),
        "the pass branch itself must still set the flag"
    );

    // (3) `axon-deploy/1` reports `status`, never `error`. The deploy pane
    // treated any response without an `error` field as success, so a
    // gate-blocked deploy rendered as "deployed".
    assert!(
        html.contains("j.status === 'deployed'"),
        "deploy must gate on the `status` field the schema emits"
    );
}

/// AUDIT T50 (P4-PROD-10): staging is content-addressed, so `ast approve` and
/// `deploy` of the same program resolve to the same path — and an edited
/// program resolves elsewhere, so its approval does NOT carry over.
#[test]
fn staging_is_content_addressed_so_approval_binds_t50() {
    // Reproduced before the fix against the running server: the path came from
    // `subsec_nanos()`, so approve wrote `/tmp/axon_web_400484501.ax.approved`
    // while deploy ran `/tmp/axon_web_416046457.ax` and reported
    // `"approved": false`. Every approval a user clicked was discarded.
    let a = crate::api::stage_for_test("fn main() -> i64 { 0 }\n", "ax");
    let b = crate::api::stage_for_test("fn main() -> i64 { 0 }\n", "ax");
    assert_eq!(
        a, b,
        "the same program text must stage to the same path, or its approval is lost"
    );

    let edited = crate::api::stage_for_test("fn main() -> i64 { 1 }\n", "ax");
    assert_ne!(
        a, edited,
        "edited text must stage elsewhere, so an approval cannot silently carry over"
    );
}

/// POST /api/safety/attest returns ok=true in mock mode (AXON_CI_NO_KVM=1 or
/// no kernel image on disk, which is always the case in CI/unit tests).
#[test]
fn test_safety_attest_mock() {
    // Ensure mock mode: unset or set AXON_CI_NO_KVM=1 (kernel won't exist in tests).
    std::env::set_var("AXON_CI_NO_KVM", "1");
    start_server_thread(18087);
    let (status, body) = post_json(18087, "/api/safety/attest", "{}");
    assert_eq!(status, 200, "expected 200, got {status}");
    let v: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
    assert_eq!(v["ok"], true, "attest mock must return ok=true, got: {v}");
    assert_eq!(
        v["attested"], true,
        "attest mock must return attested=true, got: {v}"
    );
    assert_eq!(
        v["mode"], "mock",
        "attest in CI should be mode=mock, got: {v}"
    );
}

/// GET /api/safety/status returns all four required fields.
#[test]
fn test_safety_status_aggregate() {
    start_server_thread(18088);
    let (status, body) = get(18088, "/api/safety/status");
    assert_eq!(status, 200, "expected 200, got {status}");
    let v: serde_json::Value =
        serde_json::from_str(&body).unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
    assert_eq!(v["ok"], true, "safety/status must return ok=true, got: {v}");
    assert!(
        v["attested"].is_boolean(),
        "safety/status must include 'attested' bool, got: {v}"
    );
    assert!(
        v["killable"].is_boolean(),
        "safety/status must include 'killable' bool, got: {v}"
    );
    assert!(
        v["ledger_ok"].is_boolean(),
        "safety/status must include 'ledger_ok' bool, got: {v}"
    );
    assert!(
        v["coalition_ok"].is_boolean(),
        "safety/status must include 'coalition_ok' bool, got: {v}"
    );
}

/// End-to-end test using the real `axon` binary.
/// Auto-discovers the workspace debug build; skips if binary not found.
/// Override via AXON_BIN env var: AXON_BIN=/abs/path/axon cargo test -p axon-web e2e
#[test]
fn e2e_full_flow_with_real_axon_binary() {
    // Prefer explicit AXON_BIN, fall back to workspace target/debug/axon
    let workspace_axon = {
        // CARGO_MANIFEST_DIR is crates/axon-web; ../../target/debug/axon is workspace binary
        let manifest = env!("CARGO_MANIFEST_DIR");
        std::path::Path::new(manifest)
            .join("../../target/debug/axon")
            .canonicalize()
            .ok()
            .map(|p| p.to_string_lossy().into_owned())
    };
    let axon_bin = match std::env::var("AXON_BIN") {
        Ok(b) if b != "true" && !b.is_empty() => b,
        _ => match workspace_axon {
            Some(p) if std::path::Path::new(&p).exists() => p,
            _ => return, // skip when binary not available
        },
    };

    // Use hello-goal.md content (the Phase-10 forcing function)
    let hello_goal_md = include_str!("../../../examples/goals/hello-goal.md");

    start_server_thread_with(18090, axon_bin.clone());

    // Step 1: intent compile
    let body = serde_json::json!({"content": hello_goal_md}).to_string();
    let (status, resp) = post_json(18090, "/api/intent/compile", &body);
    assert_eq!(status, 200, "intent/compile status");
    let compile_j: serde_json::Value = serde_json::from_str(&resp)
        .unwrap_or_else(|_| panic!("intent/compile not JSON: {resp:.200}"));
    assert_eq!(compile_j["schema"], "axon-intent-compile/1", "wrong schema");
    let ax_content = compile_j["ax_content"]
        .as_str()
        .unwrap_or_else(|| compile_j["stdout"].as_str().unwrap_or(""))
        .to_string();
    // ax_content may be empty if the server reads it from file; just verify JSON shape
    assert!(
        compile_j["title"].is_string(),
        "missing title in compile response"
    );

    // Read the .ax file the compile step produced (path is in the response)
    let ax_path = compile_j["path"].as_str().unwrap_or("");
    let ax_body = if !ax_content.is_empty() {
        ax_content
    } else if !ax_path.is_empty() {
        std::fs::read_to_string(ax_path).unwrap_or_default()
    } else {
        String::new()
    };

    // ASSERT, do not return. This used to bail green with the comment "the
    // compile step alone proves the integration" — so a test named
    // `e2e_full_flow` covered ONE of its four steps and reported success.
    //
    // That silence hid a real product defect for as long as it existed:
    // `axon intent compile --json` emitted `"path"` and `"ax_bytes": 3415` and
    // WROTE NO FILE (the json branch returned before the write), so the server
    // handed back a path to nothing and review/approve/deploy never ran. A test
    // that skips the rest of the flow when step 1 half-fails cannot report the
    // one thing it exists to report.
    assert!(
        !ax_body.is_empty(),
        "intent/compile reported path {ax_path:?} but it is empty or unreadable — \
         the remaining steps (review, approve, deploy) cannot run, and this test \
         must not pass while they are skipped"
    );

    // Step 2: ast review
    let body2 = serde_json::json!({"content": ax_body}).to_string();
    let (status2, resp2) = post_json(18090, "/api/ast/review", &body2);
    assert_eq!(status2, 200, "ast/review status");
    let review_j: serde_json::Value = serde_json::from_str(&resp2)
        .unwrap_or_else(|_| panic!("ast/review not JSON: {resp2:.200}"));
    assert_eq!(
        review_j["schema"], "axon-ast-review/2",
        "wrong review schema"
    );
    assert!(review_j["fns"].is_array(), "missing fns in review response");

    // Step 3: ast approve
    let body3 = serde_json::json!({"content": ax_body}).to_string();
    let (status3, resp3) = post_json(18090, "/api/ast/approve", &body3);
    assert_eq!(status3, 200, "ast/approve status");
    let approve_j: serde_json::Value = serde_json::from_str(&resp3)
        .unwrap_or_else(|_| panic!("ast/approve not JSON: {resp3:.200}"));
    assert!(
        approve_j["ok"].as_bool().unwrap_or(false),
        "approve not ok: {approve_j}"
    );

    // Step 6: trace (always available)
    let (status6, resp6) = get(18090, "/api/trace");
    assert_eq!(status6, 200, "trace status");
    let _: serde_json::Value =
        serde_json::from_str(&resp6).unwrap_or_else(|_| panic!("trace not JSON: {resp6:.200}"));

    // axon binary is the actual binary
    let _ = axon_bin; // used via env var in start_server_thread
}
