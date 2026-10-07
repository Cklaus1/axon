//! B256: `closed-loop-profile/1` negotiation.
//!
//! The named tests pin the rules the task states; `cross_language_vectors`
//! runs every case in `docs/closed-loop-profile/vectors.json`, which is the
//! file the MiCode half implements against.

use axon_loop_contracts::profile::*;
use axon_loop_contracts::*;
use serde_json::{json, Value};
use std::path::PathBuf;

fn local(
    schemas: &[&str],
    features: &[&str],
    adapters: &[&str],
    required: &[&str],
) -> LocalCapabilities {
    LocalCapabilities::new("axon", schemas, features, adapters, required).unwrap()
}

fn offer_json(schemas: &[&str], features: &[&str], adapters: &[&str], required: &[&str]) -> String {
    json!({"schema": "closed-loop-profile/1", "role": "offer", "peer": "micode",
           "schemas": schemas, "features": features, "adapters": adapters, "required": required})
    .to_string()
}

fn offer(
    schemas: &[&str],
    features: &[&str],
    adapters: &[&str],
    required: &[&str],
) -> ProfileOffer {
    parse(&offer_json(schemas, features, adapters, required)).unwrap()
}

fn strs<'a>(it: impl Iterator<Item = &'a str>) -> Vec<&'a str> {
    it.collect()
}

const P: &str = "axon.closed-loop.policy/1";
const E: &str = "axon.closed-loop.episode/1";
const C: &str = "axon.closed-loop.context/1";

#[test]
fn exact_match_accepts_everything_offered() {
    let l = LocalCapabilities::axon_v022();
    let o = offer(&CLOSED_LOOP_SCHEMAS, &[], &[ADAPTER_CORTEX_POLICY_V1], &[]);
    let a = negotiate(&o, &l).unwrap();
    assert_eq!(strs(a.agreement.schemas()), CLOSED_LOOP_SCHEMAS.to_vec());
    assert_eq!(strs(a.agreement.adapters()), vec![ADAPTER_CORTEX_POLICY_V1]);
    assert_eq!(a.document.offer_ref, digest(&o).unwrap());
    // The accept survives its own wire round trip, and the offerer confirms it.
    let wire = String::from_utf8(canonical_json(&a.document).unwrap()).unwrap();
    let back: ProfileAccept = parse(&wire).unwrap();
    assert_eq!(back, a.document);
    let micode = LocalCapabilities::new(
        "micode",
        &CLOSED_LOOP_SCHEMAS,
        &[],
        &[ADAPTER_CORTEX_POLICY_V1],
        &[],
    )
    .unwrap();
    assert_eq!(confirm(&o, &back, &micode).unwrap(), a.agreement);
}

#[test]
fn subset_accept_is_the_sorted_intersection() {
    let l = local(&[P, E, C], &[], &[], &[]);
    let o = offer(&[P, "axon.closed-loop.future/2", E], &[], &[], &[]);
    let a = negotiate(&o, &l).unwrap();
    // Sorted by byte order, never offer order, and nothing only one side lists.
    assert_eq!(
        a.document
            .schemas
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>(),
        vec![E, P]
    );
}

#[test]
fn no_common_schema_is_unsupported_not_a_fallback() {
    let l = LocalCapabilities::axon_v022();
    // Shares a pinned adapter, but no schema: the adapter is NOT a fallback.
    let o = offer(
        &["axon.closed-loop.other/1"],
        &[],
        &[ADAPTER_CORTEX_POLICY_V1],
        &[],
    );
    assert_eq!(negotiate(&o, &l).unwrap_err(), Unsupported::NoCommonSchema);
    // No version range: policy/2 does not match policy/1.
    let o = offer(&["axon.closed-loop.policy/2"], &[], &[], &[]);
    assert_eq!(negotiate(&o, &l).unwrap_err(), Unsupported::NoCommonSchema);
}

#[test]
fn a_pinned_adapter_is_never_inferred() {
    // An adapter id offered as a SCHEMA does not match a local ADAPTER.
    let l = local(&[P], &[], &[ADAPTER_AXON_BRIDGE_V0], &[]);
    let o = offer(&[ADAPTER_AXON_BRIDGE_V0], &[], &[], &[]);
    assert_eq!(negotiate(&o, &l).unwrap_err(), Unsupported::NoCommonSchema);
    // Listed on one side only: not agreed.
    let o = offer(&[P], &[], &[], &[]);
    let a = negotiate(&o, &l).unwrap();
    assert!(!a.agreement.has_adapter(ADAPTER_AXON_BRIDGE_V0));
    // Axon's own capabilities do not claim MiCode's wire.
    assert!(!LocalCapabilities::axon_v022()
        .adapters
        .iter()
        .any(|a| a.as_str() == ADAPTER_AXON_BRIDGE_V0));
}

#[test]
fn usage2_is_agreed_only_when_both_sides_offer_it() {
    // Both.
    let a = negotiate(
        &offer(&[P], &[FEATURE_USAGE2], &[], &[]),
        &local(&[P], &[FEATURE_USAGE2], &[], &[]),
    )
    .unwrap();
    assert!(a.agreement.usage2().is_ok());
    // Offer only.
    let a = negotiate(
        &offer(&[P], &[FEATURE_USAGE2], &[], &[]),
        &local(&[P], &[], &[], &[]),
    )
    .unwrap();
    assert!(a.document.features.is_empty());
    assert_eq!(
        a.agreement.usage2().unwrap_err(),
        Unsupported::FeatureNotAgreed(FEATURE_USAGE2.into())
    );
    // Local only.
    let a = negotiate(
        &offer(&[P], &[], &[], &[]),
        &local(&[P], &[FEATURE_USAGE2], &[], &[]),
    )
    .unwrap();
    assert!(a.document.features.is_empty());
    assert!(a.agreement.usage2().is_err());
    // Axon claims no Usage/2 today.
    assert!(LocalCapabilities::axon_v022().features.is_empty());
}

#[test]
fn a_required_usage2_is_unsupported_not_downgraded() {
    let e = negotiate(
        &offer(&[P], &[FEATURE_USAGE2], &[], &[FEATURE_USAGE2]),
        &local(&[P], &[], &[], &[]),
    )
    .unwrap_err();
    assert_eq!(
        e,
        Unsupported::RequiredNotAgreed {
            id: FEATURE_USAGE2.into(),
            side: Side::Offerer
        }
    );
    let e = negotiate(
        &offer(&[P], &[], &[], &[]),
        &local(&[P], &[FEATURE_USAGE2], &[], &[FEATURE_USAGE2]),
    )
    .unwrap_err();
    assert_eq!(
        e,
        Unsupported::RequiredNotAgreed {
            id: FEATURE_USAGE2.into(),
            side: Side::Local
        }
    );
}

#[test]
fn confirm_refuses_a_one_sided_usage2_accept() {
    // We offered no usage/2; a peer's accept that selects it anyway is refused.
    let o = offer(&[P], &[], &[], &[]);
    let me = LocalCapabilities::new("micode", &[P], &[FEATURE_USAGE2], &[], &[]).unwrap();
    let mut acc = negotiate(&o, &local(&[P], &[], &[], &[])).unwrap().document;
    acc.features.push(ProfileId::new(FEATURE_USAGE2).unwrap());
    assert_eq!(
        confirm(&o, &acc, &me).unwrap_err(),
        Unsupported::NotOffered {
            id: FEATURE_USAGE2.into()
        }
    );
}

#[test]
fn confirm_refuses_an_accept_bound_to_another_offer() {
    // A genuine accept for offer A, replayed against offer B (which offered
    // usage/2): same ids, but it did not accept B.
    let a = offer(&[P], &[], &[], &[]);
    let b = offer(&[P], &[FEATURE_USAGE2], &[], &[]);
    let me = LocalCapabilities::new("micode", &[P], &[FEATURE_USAGE2], &[], &[]).unwrap();
    let acc = negotiate(&a, &local(&[P], &[], &[], &[])).unwrap().document;
    assert!(confirm(&a, &acc, &me).is_ok());
    assert_eq!(
        confirm(&b, &acc, &me).unwrap_err(),
        Unsupported::OfferMismatch
    );
}

#[test]
fn unknown_field_and_duplicate_key_are_refused() {
    let l = LocalCapabilities::axon_v022();
    let good = offer_json(&[P], &[], &[], &[]);
    let extra = good.replacen('{', r#"{"extra":1,"#, 1);
    let e = negotiate_wire(Some(&extra), &l).unwrap_err();
    assert_eq!(e.refusal_class(), Some("unknown_field"), "{e}");
    let dup = good.replacen('{', r#"{"schemas":["axon.closed-loop.other/1"],"#, 1);
    let e = negotiate_wire(Some(&dup), &l).unwrap_err();
    assert_eq!(e.refusal_class(), Some("duplicate_key"), "{e}");
    // The accept schema is closed too.
    let acc = negotiate_wire(Some(&good), &l).unwrap().document;
    let mut v = serde_json::to_value(&acc).unwrap();
    v["note"] = json!("x");
    assert!(matches!(
        parse::<ProfileAccept>(&v.to_string()),
        Err(Refusal::Strict(_))
    ));
}

#[test]
fn an_absent_or_old_peer_is_unsupported() {
    let l = LocalCapabilities::axon_v022();
    assert_eq!(negotiate_wire(None, &l).unwrap_err(), Unsupported::NoOffer);
    let cortex_v1 = r#"{"protocol_version":1,"action":"inspect","principal":"agent","target_path":"x","snapshot_id":"s1"}"#;
    assert_eq!(
        negotiate_wire(Some(cortex_v1), &l).unwrap_err(),
        Unsupported::NotAProfile { schema: None }
    );
    let bridge_v0 = r#"{"schema":"axon-bridge/v0","episode_id":"ep-1"}"#;
    assert_eq!(
        negotiate_wire(Some(bridge_v0), &l).unwrap_err(),
        Unsupported::NotAProfile {
            schema: Some("axon-bridge/v0".into())
        }
    );
    let newer =
        offer_json(&[P], &[], &[], &[]).replace("closed-loop-profile/1", "closed-loop-profile/2");
    assert_eq!(
        negotiate_wire(Some(&newer), &l).unwrap_err(),
        Unsupported::UnknownProfileVersion("closed-loop-profile/2".into())
    );
}

#[test]
fn the_offer_schema_matches_the_rust_serialization() {
    let schema = parse_value(include_str!("../schemas/closed-loop-profile.schema.json")).unwrap();
    let keys = |branch: usize| -> Vec<String> {
        let mut k: Vec<String> = schema["allOf"][branch]["then"]["properties"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        k.sort();
        k
    };
    let o = serde_json::to_value(LocalCapabilities::axon_v022().offer()).unwrap();
    let mut ok: Vec<String> = o.as_object().unwrap().keys().cloned().collect();
    ok.sort();
    assert_eq!(ok, keys(0));
    let a = serde_json::to_value(
        negotiate(
            &LocalCapabilities::axon_v022().offer(),
            &LocalCapabilities::axon_v022(),
        )
        .unwrap()
        .document,
    )
    .unwrap();
    let mut ak: Vec<String> = a.as_object().unwrap().keys().cloned().collect();
    ak.sort();
    assert_eq!(ak, keys(1));
}

// ── cross-language vectors ─────────────────────────────────────────────────

fn vectors() -> Value {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/closed-loop-profile/vectors.json");
    serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap()
}

fn caps(v: &Value) -> LocalCapabilities {
    let l = |k: &str| -> Vec<&str> {
        v[k].as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_str().unwrap())
            .collect()
    };
    LocalCapabilities::new(
        v["peer"].as_str().unwrap(),
        &l("schemas"),
        &l("features"),
        &l("adapters"),
        &l("required"),
    )
    .unwrap()
}

fn check_unsupported(name: &str, got: &Unsupported, want: &Value) {
    assert_eq!(
        got.code(),
        want["unsupported"].as_str().unwrap(),
        "{name}: {got}"
    );
    if let Some(class) = want.get("refusal") {
        assert_eq!(got.refusal_class(), class.as_str(), "{name}: {got}");
    }
    if let (Some(side), Unsupported::RequiredNotAgreed { id, side: s }) = (want.get("side"), got) {
        let s = match s {
            Side::Offerer => "offerer",
            Side::Local => "local",
        };
        assert_eq!(s, side.as_str().unwrap(), "{name}");
        assert_eq!(id, want["id"].as_str().unwrap(), "{name}");
    }
}

#[test]
fn cross_language_vectors() {
    let v = vectors();
    let cases = v["negotiate"].as_array().unwrap();
    assert!(cases.len() >= 25, "vector file shrank");
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got = negotiate_wire(c["offer"].as_str(), &caps(&c["local"]));
        match (got, c["expect"].get("accept")) {
            (Ok(a), Some(want)) => {
                assert_eq!(&serde_json::to_value(&a.document).unwrap(), want, "{name}");
                // Byte-level: the accept's canonical bytes are the expected
                // value's canonical bytes.
                assert_eq!(
                    canonical_json(&a.document).unwrap(),
                    canonical_bytes(want).unwrap(),
                    "{name}"
                );
            }
            (Ok(a), None) => panic!(
                "{name}: ACCEPTED {:?}, expected {}",
                a.document, c["expect"]
            ),
            (Err(e), Some(_)) => panic!("{name}: expected accept, got {e}"),
            (Err(e), None) => check_unsupported(name, &e, &c["expect"]),
        }
    }
    for c in v["confirm"].as_array().unwrap() {
        let name = c["name"].as_str().unwrap();
        let o: ProfileOffer = parse(c["offer"].as_str().unwrap()).unwrap();
        let got = parse::<ProfileAccept>(c["accept"].as_str().unwrap())
            .map_err(Unsupported::Refused)
            .and_then(|a| confirm(&o, &a, &caps(&c["local"])));
        match (got, c["expect"].get("agreement")) {
            (Ok(ag), Some(want)) => {
                let got = json!({"schemas": strs(ag.schemas()), "features": strs(ag.features()),
                                 "adapters": strs(ag.adapters())});
                assert_eq!(&got, want, "{name}");
            }
            (Ok(ag), None) => panic!("{name}: AGREED {ag:?}, expected {}", c["expect"]),
            (Err(e), Some(_)) => panic!("{name}: expected agreement, got {e}"),
            (Err(e), None) => check_unsupported(name, &e, &c["expect"]),
        }
    }
}

/// MiCode's REAL old-peer documents, vendored verbatim from micode
/// `docs/axon-support/closed-loop-profile/micode-old-peer-vectors.json`
/// (`v022/micode-stage5-ld`, commit dd0b18e3). The `old_peer_axon_bridge_v0`
/// case in `vectors.json` GUESSED the bridge document as `{"schema":
/// "axon-bridge/v0", …}`; the real `AxonBundle` has no `schema` key at all, so
/// it reaches `not_a_profile` through the schema-ABSENT branch rather than the
/// schema-not-a-profile one. Both are kept: the guessed case still pins the
/// tagged branch, this one pins what a real old MiCode sends. `vectors.json` is
/// left unchanged because MiCode pins it by SHA-256.
#[test]
fn micodes_real_old_peer_documents_are_not_a_profile() {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../docs/closed-loop-profile/micode-old-peer-vectors.json");
    let v: Value = serde_json::from_str(&std::fs::read_to_string(p).unwrap()).unwrap();
    let cases = v["negotiate"].as_array().unwrap();
    assert_eq!(cases.len(), 2, "MiCode's old-peer vector set changed");
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let offer = c["offer"].as_str().unwrap();
        let doc: Value = serde_json::from_str(offer).unwrap();
        assert!(
            doc.get("schema").is_none(),
            "{name}: a real old-peer document carries no `schema`"
        );
        match negotiate_wire(Some(offer), &caps(&c["local"])) {
            Ok(a) => panic!("{name}: ACCEPTED {:?}", a.document),
            Err(e) => check_unsupported(name, &e, &c["expect"]),
        }
    }
}

/// The loop's protected-profile list and the launch manifest's pinned profile
/// are one string (amendment 82): `check_bundle` does not join the manifest's
/// `backend_profile` to the receipt's `backend_profile_ref`, which is sound only
/// while the list has this single element.
#[test]
fn protected_profiles_is_the_one_profile_the_launch_manifest_pins() {
    assert_eq!(PROTECTED_PROFILES, &[axon_psv::PROTECTED_PROFILE]);
}
