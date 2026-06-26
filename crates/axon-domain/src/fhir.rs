//! `native::fhir` — minimal FHIR R4 REST client (healthtech).
//!
//! A real HTTP client (reqwest blocking + serde_json) for the FHIR R4 read +
//! search interactions. The verification (`scripts/fhir_roundtrip.sh` / the
//! `#[test]`) stands up an IN-TEST `tiny_http` server returning a canned
//! `Patient` resource, reads it through the shim, and parses a field out of the
//! returned JSON — a true HTTP round-trip, not a stub.
//!
//! Surface (R13 representable set):
//!  * `fhir_connect(base_url: str) -> Handle`        (FhirConn, affine resource)
//!  * `fhir_read(ref h, resource_type: str, id: str) -> str`   (the JSON body)
//!  * `fhir_search(ref h, resource_type: str, query: str) -> str`  (Bundle JSON)
//!  * `fhir_json_get(json: str, path: str) -> str`   (pure dotted-path extract)
//!  * `fhir_close(h) -> Unit`                        (consumes the connection)
//!
//! PHI angle: a returned `Patient` is Protected Health Information. The demo
//! pairs `fhir_read` with the info-flow `Secret` lattice — the JSON is tagged
//! `Secret` and may not flow to an AI/Net sink without declassification.
//!
//! Codegen E0910-refuses these (live network I/O). Net-host pinning is enforced
//! at CHECK time against the `fhir_connect` base-URL host via the net-cap
//! allowlist.

use std::time::Duration;

use crate::{DomainArg, DomainResult, DomainValue, Slab};

/// A FHIR REST endpoint binding — just the base URL + a blocking HTTP client.
/// An affine resource handle, consumed by `fhir_close`.
pub struct FhirConn {
    base_url: String,
    client: reqwest::blocking::Client,
}

impl std::fmt::Debug for FhirConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "FhirConn({})", self.base_url)
    }
}

#[derive(Debug, Default)]
pub struct FhirBackend {
    conns: Slab<FhirConn>,
}

impl FhirBackend {
    pub fn dispatch(&mut self, fnname: &str, args: &[DomainArg]) -> DomainResult {
        match (fnname, args) {
            ("fhir_connect", [DomainArg::Str(base)]) => self.connect(base),
            (
                "fhir_read",
                [DomainArg::Handle { payload, .. }, DomainArg::Str(rtype), DomainArg::Str(id)],
            ) => self.read(*payload, rtype, id),
            (
                "fhir_search",
                [DomainArg::Handle { payload, .. }, DomainArg::Str(rtype), DomainArg::Str(query)],
            ) => self.search(*payload, rtype, query),
            // Pure JSON path extract — no network, no handle. Dotted path with
            // numeric segments for array indexing (e.g. "name.0.family").
            ("fhir_json_get", [DomainArg::Str(json), DomainArg::Str(path)]) => {
                Ok(DomainValue::Str(json_get(json, path)))
            }
            ("fhir_close", [DomainArg::Handle { payload, .. }]) => {
                self.conns.free(*payload)?;
                Ok(DomainValue::Unit)
            }
            _ => Err(format!(
                "native::fhir: bad call `{fnname}` (wrong argument shape)"
            )),
        }
    }

    fn connect(&mut self, base: &str) -> DomainResult {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| format!("fhir_connect: client build: {e}"))?;
        let base_url = base.trim_end_matches('/').to_string();
        let idx = self.conns.insert(FhirConn { base_url, client });
        Ok(DomainValue::Handle {
            name: "FhirConn",
            payload: idx,
        })
    }

    fn read(&mut self, h: i64, rtype: &str, id: &str) -> DomainResult {
        let conn = self.conns.get(h)?;
        let url = format!("{}/{}/{}", conn.base_url, rtype, id);
        let resp = conn
            .client
            .get(&url)
            .header("Accept", "application/fhir+json")
            .send()
            .map_err(|e| format!("fhir_read: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("fhir_read: body: {e}"))?;
        if !status.is_success() {
            return Err(format!("fhir_read: HTTP {status}: {body}"));
        }
        Ok(DomainValue::Str(body))
    }

    fn search(&mut self, h: i64, rtype: &str, query: &str) -> DomainResult {
        let conn = self.conns.get(h)?;
        let url = format!("{}/{}?{}", conn.base_url, rtype, query);
        let resp = conn
            .client
            .get(&url)
            .header("Accept", "application/fhir+json")
            .send()
            .map_err(|e| format!("fhir_search: {e}"))?;
        let status = resp.status();
        let body = resp.text().map_err(|e| format!("fhir_search: body: {e}"))?;
        if !status.is_success() {
            return Err(format!("fhir_search: HTTP {status}: {body}"));
        }
        Ok(DomainValue::Str(body))
    }
}

/// Extract a value from a JSON document by a dotted path (numeric segments index
/// arrays). Returns the scalar as a string, or "" if absent. Keeps the
/// boundary str-only (no JSON value crosses the FFI line).
fn json_get(json: &str, path: &str) -> String {
    let v: serde_json::Value = match serde_json::from_str(json) {
        Ok(v) => v,
        Err(_) => return String::new(),
    };
    let mut cur = &v;
    for seg in path.split('.') {
        if seg.is_empty() {
            continue;
        }
        cur = if let Ok(idx) = seg.parse::<usize>() {
            match cur.get(idx) {
                Some(x) => x,
                None => return String::new(),
            }
        } else {
            match cur.get(seg) {
                Some(x) => x,
                None => return String::new(),
            }
        };
    }
    match cur {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    const PATIENT_JSON: &str = r#"{
        "resourceType": "Patient",
        "id": "example",
        "name": [{"family": "Chalmers", "given": ["Peter", "James"]}],
        "gender": "male",
        "birthDate": "1974-12-25"
    }"#;

    // A canned in-test FHIR server: GET /Patient/example -> the Patient above;
    // GET /Patient?family=Chalmers -> a Bundle wrapping it.
    fn spawn_server() -> (String, std::thread::JoinHandle<()>) {
        let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
        let addr = server.server_addr().to_ip().unwrap();
        let base = format!("http://{}:{}", addr.ip(), addr.port());
        let handle = std::thread::spawn(move || {
            // Serve a bounded number of requests then exit.
            for _ in 0..8 {
                let req = match server.recv() {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let url = req.url().to_string();
                let body = if url.starts_with("/Patient/example") {
                    PATIENT_JSON.to_string()
                } else if url.starts_with("/Patient?") {
                    format!(
                        r#"{{"resourceType":"Bundle","type":"searchset","total":1,"entry":[{{"resource":{PATIENT_JSON}}}]}}"#
                    )
                } else {
                    let resp = tiny_http::Response::from_string("not found").with_status_code(404);
                    let _ = req.respond(resp);
                    continue;
                };
                let header = tiny_http::Header::from_bytes(
                    &b"Content-Type"[..],
                    &b"application/fhir+json"[..],
                )
                .unwrap();
                let resp = tiny_http::Response::from_string(body).with_header(header);
                let _ = req.respond(resp);
            }
        });
        (base, handle)
    }

    #[test]
    fn read_patient_roundtrip_and_parse_field() {
        let (base, _h) = spawn_server();
        let mut b = FhirBackend::default();
        let conn = match b.dispatch("fhir_connect", &[DomainArg::Str(base)]).unwrap() {
            DomainValue::Handle { payload, .. } => payload,
            _ => panic!("handle"),
        };
        let hh = DomainArg::Handle {
            tag: crate::tag_for("FhirConn"),
            payload: conn,
        };
        let json = match b
            .dispatch(
                "fhir_read",
                &[
                    hh.clone(),
                    DomainArg::Str("Patient".into()),
                    DomainArg::Str("example".into()),
                ],
            )
            .unwrap()
        {
            DomainValue::Str(s) => s,
            _ => panic!("str"),
        };
        // Parse a field out of the returned JSON.
        assert_eq!(json_get(&json, "resourceType"), "Patient");
        assert_eq!(json_get(&json, "name.0.family"), "Chalmers");
        assert_eq!(json_get(&json, "gender"), "male");

        // Search round-trip.
        let bundle = match b
            .dispatch(
                "fhir_search",
                &[
                    hh.clone(),
                    DomainArg::Str("Patient".into()),
                    DomainArg::Str("family=Chalmers".into()),
                ],
            )
            .unwrap()
        {
            DomainValue::Str(s) => s,
            _ => panic!("str"),
        };
        assert_eq!(json_get(&bundle, "resourceType"), "Bundle");
        assert_eq!(
            json_get(&bundle, "entry.0.resource.name.0.family"),
            "Chalmers"
        );

        b.dispatch("fhir_close", &[hh]).unwrap();
    }

    #[test]
    fn pure_json_get_via_dispatch() {
        let mut b = FhirBackend::default();
        let r = b
            .dispatch(
                "fhir_json_get",
                &[
                    DomainArg::Str(PATIENT_JSON.into()),
                    DomainArg::Str("birthDate".into()),
                ],
            )
            .unwrap();
        assert_eq!(r, DomainValue::Str("1974-12-25".into()));
    }

    #[test]
    fn bad_handle_is_graceful_err() {
        let mut b = FhirBackend::default();
        for bad in [9999i64, -1, i64::MIN, i64::MAX] {
            let h = DomainArg::Handle {
                tag: crate::tag_for("FhirConn"),
                payload: bad,
            };
            assert!(b
                .dispatch(
                    "fhir_read",
                    &[
                        h,
                        DomainArg::Str("Patient".into()),
                        DomainArg::Str("x".into())
                    ]
                )
                .is_err());
        }
    }

    // Quiet the unused-import lint when the server helper changes shape.
    #[allow(dead_code)]
    fn _touch(mut r: impl Read) {
        let mut s = String::new();
        let _ = r.read_to_string(&mut s);
    }
}
