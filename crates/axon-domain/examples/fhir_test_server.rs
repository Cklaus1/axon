//! A fixed-port mock FHIR R4 HTTP server for the `fhir_roundtrip.sh` gate's
//! demo leg: serves a canned Patient resource on 127.0.0.1:<PORT> (default
//! 18080) via tiny_http. Used ONLY for verification — the `.ax` demo connects
//! to it through the shim and reads/parses a field. NOT a product path.

const PATIENT_JSON: &str = r#"{
  "resourceType": "Patient",
  "id": "example",
  "name": [{"family": "Chalmers", "given": ["Peter", "James"]}],
  "gender": "male",
  "birthDate": "1974-12-25"
}"#;

fn main() {
    let port: u16 = std::env::var("FHIR_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(18080);
    let server = tiny_http::Server::http(("127.0.0.1", port)).unwrap();
    eprintln!("fhir_test_server: listening on 127.0.0.1:{port}");
    for req in server.incoming_requests() {
        let url = req.url().to_string();
        let body = if url.starts_with("/Patient/example") {
            PATIENT_JSON.to_string()
        } else if url.starts_with("/Patient?") {
            format!(
                r#"{{"resourceType":"Bundle","type":"searchset","total":1,"entry":[{{"resource":{PATIENT_JSON}}}]}}"#
            )
        } else {
            let _ =
                req.respond(tiny_http::Response::from_string("not found").with_status_code(404));
            continue;
        };
        let header =
            tiny_http::Header::from_bytes(&b"Content-Type"[..], &b"application/fhir+json"[..])
                .unwrap();
        let _ = req.respond(tiny_http::Response::from_string(body).with_header(header));
    }
}
