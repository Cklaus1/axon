use tiny_http::{Header, Request, Response};

pub fn handle(mut req: Request, axon_bin: &str) {
    let method = req.method().as_str().to_ascii_uppercase();
    let url = req.url().split('?').next().unwrap_or("").to_owned();

    let (status, body, ct) = match (method.as_str(), url.as_str()) {
        ("GET", "/") | ("GET", "/index.html") => (
            200,
            crate::html::INDEX_HTML.to_string(),
            "text/html; charset=utf-8",
        ),
        ("POST", "/api/intent/compile") => {
            let b = read_body(&mut req);
            (
                200,
                crate::api::intent_compile(&b, axon_bin),
                "application/json",
            )
        }
        ("POST", "/api/ast/review") => {
            let b = read_body(&mut req);
            (
                200,
                crate::api::ast_review(&b, axon_bin),
                "application/json",
            )
        }
        ("POST", "/api/ast/approve") => {
            let b = read_body(&mut req);
            (
                200,
                crate::api::ast_approve(&b, axon_bin),
                "application/json",
            )
        }
        ("POST", "/api/redteam") => {
            let b = read_body(&mut req);
            (200, crate::api::redteam(&b, axon_bin), "application/json")
        }
        ("POST", "/api/deploy") => {
            let b = read_body(&mut req);
            (200, crate::api::deploy(&b, axon_bin), "application/json")
        }
        ("GET", "/api/trace") => (200, crate::api::trace(axon_bin), "application/json"),
        ("POST", "/api/goal/improve") => {
            let b = read_body(&mut req);
            (
                200,
                crate::api::goal_improve(&b, axon_bin),
                "application/json",
            )
        }
        // Safety layer endpoints (R26 / R27 / R28)
        ("POST", "/api/safety/attest") => (200, crate::api::safety_attest(), "application/json"),
        ("POST", "/api/safety/kill") => {
            let b = read_body(&mut req);
            (200, crate::api::safety_kill(&b), "application/json")
        }
        ("GET", "/api/safety/ledger") => (200, crate::api::safety_ledger(), "application/json"),
        ("GET", "/api/safety/status") => (200, crate::api::safety_status(), "application/json"),
        _ => (
            404,
            r#"{"error":"not found"}"#.to_string(),
            "application/json",
        ),
    };

    let ct_hdr = Header::from_bytes(b"Content-Type", ct.as_bytes()).expect("valid header");

    // NO `Access-Control-Allow-Origin: *`. This server binds loopback, which
    // stops the NETWORK reaching it — and a wildcard CORS header handed it to
    // every page the operator's own browser visits.
    //
    // That is far worse here than on a read-only dashboard. `POST /api/deploy`
    // writes the CALLER-SUPPLIED `content` to a temp .ax and executes it
    // (`api.rs`), and `/api/ast/approve`, `/api/redteam` and the safety
    // endpoints are equally reachable. A cross-origin POST with
    // `Content-Type: text/plain` is a CORS *simple request* — no preflight —
    // so any site could fire it, and the wildcard let that site read the
    // result.
    //
    // REPRODUCED against a live server: a cross-origin POST to /api/deploy
    // carrying a program that calls write_file returned 200 with
    // `Access-Control-Allow-Origin: *` and the file was created.
    //
    // The UI is served from THIS origin (`GET /` returns the HTML), so it is a
    // same-origin caller and needs no CORS header at all. A genuine
    // cross-origin consumer would need a deliberate, narrow allowlist — never
    // `*`, and never on endpoints that execute code.
    let resp = Response::from_string(body)
        .with_status_code(status)
        .with_header(ct_hdr);

    req.respond(resp).ok();
}

fn read_body(req: &mut Request) -> String {
    let mut buf = String::new();
    req.as_reader().read_to_string(&mut buf).unwrap_or(0);
    buf
}
