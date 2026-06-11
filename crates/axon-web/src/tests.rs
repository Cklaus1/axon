//! Integration tests: start a real HTTP server, exercise every route.

use std::io::{Read, Write};
use std::net::TcpStream;
use std::thread;
use std::time::Duration;

fn start_server_thread(port: u16) {
    let axon_bin = std::env::var("AXON_BIN").unwrap_or_else(|_| "true".into());
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
    assert!(body.contains("<!DOCTYPE html>"), "expected HTML, got: {body:.200}");
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
    let v: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
    assert!(v["error"].is_string());
}

#[test]
fn post_intent_compile_returns_json() {
    start_server_thread(18083);
    let payload = "{\"content\":\"Goal: test something.\"}";
    let (status, body) = post_json(18083, "/api/intent/compile", payload);
    assert_eq!(status, 200, "status: {status}, body: {body:.200}");
    // Body must be valid JSON regardless of axon binary availability
    let _: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn post_deploy_returns_json() {
    start_server_thread(18084);
    let payload = "{\"content\":\"fn main() { println(\\\"ok\\\") }\",\"risk\":\"low\"}";
    let (status, body) = post_json(18084, "/api/deploy", payload);
    assert_eq!(status, 200, "status: {status}");
    let _: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn get_trace_returns_json() {
    start_server_thread(18085);
    let (status, body) = get(18085, "/api/trace");
    assert_eq!(status, 200);
    let _: serde_json::Value = serde_json::from_str(&body)
        .unwrap_or_else(|_| panic!("expected JSON, got: {body:.200}"));
}

#[test]
fn html_contains_all_panes() {
    let html = crate::html::INDEX_HTML;
    for step in ["Intent", "AST Review", "Approve", "Red Team", "Deploy", "Trace"] {
        assert!(html.contains(step), "HTML missing pane: {step}");
    }
    // Every API endpoint referenced in the JS
    for ep in [
        "/api/intent/compile",
        "/api/ast/review",
        "/api/ast/approve",
        "/api/redteam",
        "/api/deploy",
        "/api/trace",
    ] {
        assert!(html.contains(ep), "HTML missing endpoint ref: {ep}");
    }
}
