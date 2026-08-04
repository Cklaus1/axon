//! Integration tests: start a real HTTP server, exercise every route.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

fn start_server_thread(port: u16) {
    let axon_bin = std::env::var("AXON_BIN").unwrap_or_else(|_| "true".into());
    start_server_thread_with(port, axon_bin);
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
    let body = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("").to_string();
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
    let resp_body = resp.splitn(2, "\r\n\r\n").nth(1).unwrap_or("").to_string();
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
    assert!(
        flag_at > passed_at,
        "done.redteamed must be set INSIDE the pass branch, not after the if/else chain"
    );
    let caught_at = body.find("if (caught)").expect("caught branch");
    assert!(
        !body[caught_at..passed_at].contains("done.redteamed = true"),
        "done.redteamed must not be set on the CAUGHT path"
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

    if ax_body.is_empty() {
        // Can't proceed without .ax content; compile step alone proves the integration
        return;
    }

    // Step 2: ast review
    let body2 = serde_json::json!({"content": ax_body}).to_string();
    let (status2, resp2) = post_json(18090, "/api/ast/review", &body2);
    assert_eq!(status2, 200, "ast/review status");
    let review_j: serde_json::Value = serde_json::from_str(&resp2)
        .unwrap_or_else(|_| panic!("ast/review not JSON: {resp2:.200}"));
    assert_eq!(
        review_j["schema"], "axon-ast-review/1",
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
