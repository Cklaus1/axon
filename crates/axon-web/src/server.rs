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
        _ => (
            404,
            r#"{"error":"not found"}"#.to_string(),
            "application/json",
        ),
    };

    let ct_hdr = Header::from_bytes(b"Content-Type", ct.as_bytes()).expect("valid header");
    let cors_hdr = Header::from_bytes(b"Access-Control-Allow-Origin", b"*").expect("valid header");

    let resp = Response::from_string(body)
        .with_status_code(status)
        .with_header(ct_hdr)
        .with_header(cors_hdr);

    req.respond(resp).ok();
}

fn read_body(req: &mut Request) -> String {
    let mut buf = String::new();
    req.as_reader().read_to_string(&mut buf).unwrap_or(0);
    buf
}
