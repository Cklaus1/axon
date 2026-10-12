//! `--negotiate` (B256) through the REAL adapter binary: a library that passes
//! the vectors is not a peer that answers them. Every shared vector whose local
//! side is exactly this build's capabilities is replayed against the process,
//! and so are MiCode's real old-peer documents.

use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use axon_loop_contracts::profile::{LocalCapabilities, ProfileId};
use serde_json::Value;

fn run(args: &[&str], stdin: &str) -> (i32, String, String) {
    let mut c = Command::new(env!("CARGO_BIN_EXE_cortex-policy-adapter"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn adapter");
    // BrokenPipe only: a usage error exits before reading stdin.
    if let Err(e) = c.stdin.as_mut().unwrap().write_all(stdin.as_bytes()) {
        assert_eq!(e.kind(), std::io::ErrorKind::BrokenPipe, "{e}");
    }
    let o = c.wait_with_output().unwrap();
    (
        o.status.code().unwrap_or(-1),
        String::from_utf8_lossy(&o.stdout).trim().to_string(),
        String::from_utf8_lossy(&o.stderr).to_string(),
    )
}

fn negotiate(offer: &str) -> (i32, Value) {
    let (code, out, err) = run(&["--negotiate"], offer);
    assert!(code == 0 || code == 4, "exit {code}, stderr {err}");
    (
        code,
        serde_json::from_str(&out).unwrap_or_else(|e| panic!("{e}: {out:?}")),
    )
}

fn doc(name: &str) -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/closed-loop-profile")
        .join(name);
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn is_axon_local(local: &Value) -> bool {
    let axon = LocalCapabilities::axon_v022();
    let set = |k: &str| -> Vec<String> {
        let mut v: Vec<String> = local[k]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap().to_string())
            .collect();
        v.sort();
        v
    };
    fn mine(s: &std::collections::BTreeSet<ProfileId>) -> Vec<String> {
        s.iter().map(|x| x.as_str().to_string()).collect()
    }
    local["peer"] == "axon"
        && set("schemas") == mine(&axon.schemas)
        && set("features") == mine(&axon.features)
        && set("adapters") == mine(&axon.adapters)
        && set("required") == mine(&axon.required)
}

#[test]
fn the_real_adapter_answers_every_shared_vector_for_its_own_capabilities() {
    let v = doc("vectors.json");
    let mut ran = 0;
    for c in v["negotiate"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        // An absent offer is a client-side state (the client never sent one);
        // the process always receives SOME bytes.
        let (Some(offer), true) = (c["offer"].as_str(), is_axon_local(&c["local"])) else {
            continue;
        };
        let (code, got) = negotiate(offer);
        match c["expect"].get("accept") {
            Some(want) => {
                assert_eq!(code, 0, "{name}: {got}");
                assert_eq!(&got, want, "{name}");
            }
            None => {
                assert_eq!(code, 4, "{name}: {got}");
                assert_eq!(
                    got["unsupported"], c["expect"]["unsupported"],
                    "{name}: {got}"
                );
            }
        }
        ran += 1;
    }
    // Measured on the 38-case file: the floor keeps a filter change from
    // quietly replaying nothing.
    assert!(ran >= 20, "only {ran} vectors replayed against the binary");
}

#[test]
fn micodes_real_old_peer_documents_get_an_explicit_unsupported() {
    let v = doc("micode-old-peer-vectors.json");
    for c in v["negotiate"].as_array().unwrap() {
        let (code, got) = negotiate(c["offer"].as_str().unwrap());
        assert_eq!(code, 4, "{got}");
        assert_eq!(got["unsupported"], "not_a_profile", "{got}");
    }
}

/// G16-r22-closed-wire at the process boundary: ambiguous or open input is
/// refused before anything is negotiated.
#[test]
fn ambiguous_or_open_offers_are_refused_not_resolved() {
    let good = r#"{"schema":"closed-loop-profile/1","role":"offer","peer":"micode","schemas":["axon.closed-loop.episode/1"],"features":[],"adapters":[],"required":[]}"#;
    let (code, got) = negotiate(good);
    assert_eq!(code, 0, "{got}");
    assert_eq!(got["role"], "accept");
    assert!(got["offer_ref"].as_str().unwrap().starts_with("cl22:"));

    for (why, bad) in [
        (
            "duplicate key",
            good.replacen(r#""peer":"micode""#, r#""peer":"micode","peer":"x""#, 1),
        ),
        (
            "escaped alias key",
            good.replacen(
                r#""peer":"micode""#,
                r#""peer":"micode","pe\u0065r":"x""#,
                1,
            ),
        ),
        (
            "unknown field",
            good.replacen(r#""required":[]"#, r#""required":[],"extra":1"#, 1),
        ),
        (
            "float",
            good.replacen(r#""features":[]"#, r#""features":[],"n":1.5"#, 1),
        ),
    ] {
        let (code, got) = negotiate(&bad);
        assert_eq!(code, 4, "{why}: {got}");
        assert_eq!(got["unsupported"], "refused", "{why}: {got}");
    }

    let needs_usage2 = good.replacen(
        r#""features":[],"adapters":[],"required":[]"#,
        r#""features":["usage/2"],"adapters":[],"required":["usage/2"]"#,
        1,
    );
    let (code, got) = negotiate(&needs_usage2);
    assert_eq!(code, 4, "{got}");
    assert_eq!(got["unsupported"], "required_not_agreed", "{got}");
}

#[test]
fn negotiate_is_a_mode_and_cannot_be_mixed_with_a_grant() {
    let (code, out, _) = run(&["--negotiate", "--principal", "agent"], "{}");
    assert_eq!(code, 2);
    assert!(out.is_empty(), "a usage error decides nothing: {out}");
}
