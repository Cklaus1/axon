//! SIGNING-DOMAIN SEPARATION (C9 round 12, eqgate8, amendment 110).
//!
//! Every constant that is INPUT TO a signing, verification or MAC primitive is pinned here, in the one
//! place that can see all of them (axon-fabric depends on axon-loop, axon-loop-contracts and axon-psv).
//! Before this file, setting `CLEARANCE_DOMAIN` or `EXECUTION_DOMAIN` equal to another domain string kept
//! the whole axon-fabric, axon-loop and axon-loop-contracts suites green: the constants were filed as
//! "compared tags" when they are SIGNED OVER, and every test signed and verified with the same constant
//! on both sides, so a collapse of two domains was invisible. Three kinds of test close it:
//!
//! * DISTINCT AND GOLDEN: the domain strings are pairwise distinct and equal their documented literals.
//! * CROSS-PROTOCOL REPLAY: a signature minted under domain A is refused under every other domain B,
//!   using the real constants on both sides (so `A := B` makes it verify and the test fails).
//! * KNOWN ANSWER: Ed25519 and HMAC are deterministic, so a fixed key over a fixed document has ONE
//!   correct signature. A change to ANY signed byte (a schema tag inside the binding, a format string, a
//!   pass/fail context) changes it, which a round trip through the same code can never show.
//!
//! RESIDUAL, stated as the other validators state theirs: the golden values were computed from the
//! implementation at the head that introduced them, so they pin it against drift; they do not prove the
//! choice of the strings was right, and a peer implementation outside this repository that signs under a
//! different literal is not seen at all.

use axon_loop::evl::CONTEXT_DOMAIN;
use axon_loop::safety::CLEARANCE_DOMAIN;
use axon_loop_contracts::attestation::{
    execution_document, sign, sign_document, verify_document, ATTESTATION_SCHEMA,
    DOCUMENT_SIGNATURE_SCHEMA, EXECUTION_DOMAIN,
};
use axon_loop_contracts::operator_trust::{
    evidence_signing_message, TrustAuthority, EVIDENCE_SIGNATURE_SCHEMA,
};
use axon_loop_contracts::{ComputeRequest, ExecutionReceipt, OpaqueRef};
use axon_psv::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{json, Value};

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// A PKCS#8 v2 Ed25519 key from a fixed seed: the signatures below are deterministic.
fn fixed_key() -> Vec<u8> {
    let seed = [7u8; 32];
    let kp = Ed25519KeyPair::from_seed_unchecked(&seed).expect("seed");
    let mut v = hex_decode("3053020101300506032b657004220420");
    v.extend_from_slice(&seed);
    v.extend_from_slice(&[0xa1, 0x23, 0x03, 0x21, 0x00]);
    v.extend_from_slice(kp.public_key().as_ref());
    v
}

fn hex_decode(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn public_hex(pkcs8: &[u8]) -> String {
    axon_loop_contracts::attestation::public_key_of(pkcs8).expect("public key")
}

fn issuer() -> OpaqueRef {
    OpaqueRef::new("fabric:verifier").expect("ref")
}

fn fixture(name: &str) -> Value {
    let p = format!(
        "{}/../axon-loop-contracts/tests/fixtures/acf/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json")
}

fn req() -> ComputeRequest {
    serde_json::from_value(fixture("request_offline.json")).expect("request")
}

fn rc() -> ExecutionReceipt {
    serde_json::from_value(fixture("receipt_outcome_unknown.json")).expect("receipt")
}

/// A known-answer check whose panic names the context: the marker cannot match a setup failure.
fn kat(what: &str, got: &str, want: &str) {
    assert_eq!(
        got, want,
        "signing KAT {what}: the signed bytes changed (a domain, a schema tag or a context string \
         inside the message differs from the pinned one)"
    );
}

// ── the protocol domains ─────────────────────────────────────────────────────

/// Every string that scopes a signature or a MAC to one protocol: (name, value, documented literal).
fn domains() -> Vec<(&'static str, &'static str, &'static str)> {
    vec![
        (
            "CLEARANCE_DOMAIN",
            CLEARANCE_DOMAIN,
            "axon.loop.trial-safety/1",
        ),
        (
            "EXECUTION_DOMAIN",
            EXECUTION_DOMAIN,
            "axon.fabric-execution/1",
        ),
        (
            "CONTEXT_DOMAIN",
            CONTEXT_DOMAIN,
            "axon.closed-loop.context/1",
        ),
        (
            "DOCUMENT_SIGNATURE_SCHEMA",
            DOCUMENT_SIGNATURE_SCHEMA,
            "axon-document-signature/1",
        ),
        (
            "ATTESTATION_SCHEMA",
            ATTESTATION_SCHEMA,
            "acf-receipt-attestation/2",
        ),
        (
            "EVIDENCE_SIGNATURE_SCHEMA",
            EVIDENCE_SIGNATURE_SCHEMA,
            "axon-evidence-signature/2",
        ),
        (
            "COMPLETION_SCHEME",
            COMPLETION_SCHEME,
            "axon-guest-completion/1",
        ),
    ]
}

#[test]
fn signing_domains_are_pairwise_distinct_and_golden() {
    let d = domains();
    for (i, (an, av, _)) in d.iter().enumerate() {
        for (bn, bv, _) in &d[i + 1..] {
            assert_ne!(av, bv, "signing domain collapse: {an} == {bn} ({av:?})");
        }
    }
    for (name, value, literal) in &d {
        assert_eq!(
            value, literal,
            "signing domain {name} is not its documented literal {literal:?}"
        );
    }
    // the evidence signature's own domain axis: one per trust authority
    let names: Vec<_> = TrustAuthority::ALL.iter().map(|a| a.dir_name()).collect();
    assert_eq!(
        names,
        [
            "qualification",
            "observer",
            "verifier",
            "admission",
            "monitor"
        ],
        "the trust authorities (the evidence signature's domain) are not the documented five"
    );
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            assert_ne!(a, b, "signing domain collapse: authority {a} == {b}");
        }
    }
}

#[test]
fn a_signature_for_one_domain_never_verifies_under_another() {
    let k = fixed_key();
    let pk = public_hex(&k);
    let doc = json!({"subject": "cross-protocol replay", "n": 1});
    let all: Vec<(&str, &str)> = domains().iter().map(|(n, v, _)| (*n, *v)).collect();
    for (an, av) in &all {
        let sig = sign_document(&k, av, &issuer(), &doc).expect("sign");
        verify_document(&sig, av, &issuer(), &doc, &pk)
            .unwrap_or_else(|e| panic!("domain {an} does not verify under itself: {e}"));
        for (bn, bv) in &all {
            if an == bn {
                continue;
            }
            assert!(
                verify_document(&sig, bv, &issuer(), &doc, &pk).is_err(),
                "cross-protocol replay: a {an} signature verified as {bn}"
            );
        }
    }
    // a receipt attestation is not a document signature of the execution domain, and the other way round
    let att = sign(&k, &issuer(), &req(), &rc(), 1_000).expect("attest");
    assert!(
        verify_document(&att, EXECUTION_DOMAIN, &issuer(), &json!({}), &pk).is_err(),
        "cross-protocol replay: a receipt attestation verified as a document signature"
    );
    let exec_doc = execution_document(&req(), &rc()).expect("execution document");
    let sig = sign_document(&k, EXECUTION_DOMAIN, &issuer(), &exec_doc).expect("sign");
    assert!(
        axon_loop_contracts::attestation::verify(&sig, &issuer(), &req(), &rc(), &pk).is_err(),
        "cross-protocol replay: an execution document signature verified as a receipt attestation"
    );
    // the evidence signature: one message per authority, none a prefix-collision of another
    let msgs: Vec<Vec<u8>> = TrustAuthority::ALL
        .iter()
        .map(|a| evidence_signing_message(*a, b"record"))
        .collect();
    for (i, a) in msgs.iter().enumerate() {
        for b in &msgs[i + 1..] {
            assert_ne!(
                a, b,
                "signing domain collapse: two authorities sign the same evidence bytes"
            );
        }
    }
}

#[test]
fn fabric_signs_an_execution_attestation_in_the_execution_domain_and_no_other() {
    let k = fixed_key();
    let pk = public_hex(&k);
    let att = axon_fabric::signing::sign_execution_attestation(&k, &issuer(), &req(), &rc())
        .expect("sign");
    let doc = execution_document(&req(), &rc()).expect("execution document");
    verify_document(&att, EXECUTION_DOMAIN, &issuer(), &doc, &pk).unwrap_or_else(|e| {
        panic!(
            "ATTACK: Fabric's execution attestation does not verify in the execution domain: {e}"
        )
    });
    for (name, domain, _) in domains() {
        if domain == EXECUTION_DOMAIN {
            continue;
        }
        assert!(
            verify_document(&att, domain, &issuer(), &doc, &pk).is_err(),
            "cross-protocol replay: Fabric's execution attestation verified as {name}"
        );
    }
    assert_eq!(
        att,
        sign_document(&k, EXECUTION_DOMAIN, &issuer(), &doc).expect("sign"),
        "the execution attestation is the execution document signed in the execution domain, nothing else"
    );
}

// ── known answers ────────────────────────────────────────────────────────────

fn signature_of(v: &Value) -> String {
    v["signature"].as_str().expect("signature").to_string()
}

#[test]
fn known_answer_document_signature_per_domain() {
    let k = fixed_key();
    let doc = json!({"subject": "known answer", "n": 1});
    for (name, domain, want) in [
        ("CLEARANCE_DOMAIN", CLEARANCE_DOMAIN, KAT_CLEARANCE),
        ("EXECUTION_DOMAIN", EXECUTION_DOMAIN, KAT_EXECUTION),
        ("CONTEXT_DOMAIN", CONTEXT_DOMAIN, KAT_CONTEXT),
        (
            "generic domain (pins DOCUMENT_SIGNATURE_SCHEMA)",
            "dom/1",
            KAT_GENERIC,
        ),
    ] {
        let s = sign_document(&k, domain, &issuer(), &doc).expect("sign");
        kat(
            &format!("document signature {name}"),
            &signature_of(&s),
            want,
        );
    }
}

#[test]
fn known_answer_receipt_attestation() {
    let k = fixed_key();
    let a = sign(&k, &issuer(), &req(), &rc(), 1_000).expect("attest");
    kat(
        "receipt attestation (pins ATTESTATION_SCHEMA)",
        &signature_of(&a),
        KAT_ATTESTATION,
    );
}

#[test]
fn known_answer_execution_document() {
    let d = execution_document(&req(), &rc()).expect("execution document");
    let bytes = axon_loop_contracts::canonical_bytes(&d).expect("canonical");
    kat(
        "execution document (pins its schema tag and both refs)",
        &String::from_utf8(bytes).unwrap(),
        KAT_EXEC_DOC,
    );
}

#[test]
fn known_answer_evidence_signing_message_per_authority() {
    for (a, want) in TrustAuthority::ALL.iter().zip([
        "axon-evidence-signature/2\nqualification\nrecord",
        "axon-evidence-signature/2\nobserver\nrecord",
        "axon-evidence-signature/2\nverifier\nrecord",
        "axon-evidence-signature/2\nadmission\nrecord",
        "axon-evidence-signature/2\nmonitor\nrecord",
    ]) {
        let got = String::from_utf8(evidence_signing_message(*a, b"record")).unwrap();
        kat(&format!("evidence message {}", a.dir_name()), &got, want);
    }
}

fn manifest() -> LaunchManifest {
    LaunchManifest {
        schema: LAUNCH_MANIFEST_SCHEMA.into(),
        operation_id: "op-1".into(),
        task_id: "task-1".into(),
        trial_id: "trial-1".into(),
        attempt_id: "attempt-1".into(),
        backend_profile: PROTECTED_PROFILE.into(),
        fabric_revision: "f".repeat(40),
        verifier_sha256: "d".repeat(64),
        qualification_sha256: "1".repeat(64),
        host_config_sha256: "2".repeat(64),
        launcher_sha256: "3".repeat(64),
        firecracker_sha256: "4".repeat(64),
        profile_manifest_sha256: "5".repeat(64),
        guest: GuestDigests {
            kernel_sha256: "6".repeat(64),
            rootfs_sha256: "7".repeat(64),
            axon_sha256: "8".repeat(64),
            init_sha256: "9".repeat(64),
        },
        policy_sha256: "a".repeat(64),
        suite: SuiteRef {
            id: "acceptance".into(),
            version: format!("acf1:{}", "b".repeat(64)),
            entry: "accept.ax".into(),
            test: "t_ok".into(),
            tree_digest: format!("acf1:{}", "b".repeat(64)),
            registry_sha256: "c".repeat(64),
        },
        candidate: CandidateRef {
            workspace_version: format!("acf1:{}", "d".repeat(64)),
            tree_digest: format!("acf1:{}", "d".repeat(64)),
        },
        completion: Completion {
            scheme: COMPLETION_SCHEME.into(),
        },
        observation_nonce: "e".repeat(32),
        authority: AuthorityRef {
            epoch: 0,
            tenant_id: "tenant-t".into(),
            task_family: "family-f".into(),
        },
        limits: Limits {
            wall_time_ms: 60_000,
            output_bytes: 1 << 20,
        },
    }
}

#[test]
fn known_answer_completion_key_and_outcome_tokens() {
    let m = manifest();
    let key = completion_key(&[3u8; 32], &m);
    kat(
        "completion key (pins COMPLETION_SCHEME in the key message and the binding)",
        &hex(&key),
        KAT_COMPLETION_KEY,
    );
    let pass = outcome_token(&key, "t_ok", true);
    let fail = outcome_token(&key, "t_ok", false);
    assert_ne!(
        pass, fail,
        "signing domain collapse: the PASS and FAILED outcome contexts produce the same token"
    );
    kat(
        "outcome token, passed (pins axon-test-completion/1)",
        &pass,
        KAT_PASS_TOKEN,
    );
    kat(
        "outcome token, failed (pins axon-test-failed/1)",
        &fail,
        KAT_FAIL_TOKEN,
    );
}

#[test]
fn known_answer_ledger_key_derivation() {
    let k = axon_loop::store::LedgerKey::derive(b"0123456789abcdef0123").expect("key");
    kat(
        "ledger key derivation label",
        &hex(&k.mac(b"x")),
        KAT_LEDGER_MAC,
    );
}

// golden values, computed from the implementation at the head that introduced this file
const KAT_CLEARANCE: &str = "a7ccdc34fc02151a36b351b59c9e358edf0f4484e0de8f958ff4e6795f38ca0b1cd897a7a4534c4e5db164af255977330be57c4b1ecc9ea5daa646f65afcfc05";
const KAT_EXECUTION: &str = "36dd450b54ef212ebf9a81b3b458221f6c5c2f5f3365a44563e1eddce8928623a5dda151341362a40a0df4d08220897c2ea65cdacd1564ce934b6f242541030e";
const KAT_CONTEXT: &str = "6805a1b986fe8460aa37fcddac13577570900a3358cbe5f1560c3487eec8bf0de042d508ca34620358673ee9b3add4a158229a626e7a56643fb9810dd9a4870d";
const KAT_GENERIC: &str = "0f08f1fc5ed0310812f99e5593f43d4552ce7e83232c2ac3921647d95877d59f59d041d77eb602436b73f84868d461503d045006d11c2736473e71c1f3ea4702";
const KAT_ATTESTATION: &str = "6af6ea42280388d2624162c154d01fafb74eff6e87748a0e7134ca008e26506ace534651775b92ccc577a3463a9c3853a1ebdd4869e79b85500ff6f277c23405";
const KAT_EXEC_DOC: &str = r#"{"receipt_ref":"cl22:4b349c1d5f1c513e2bfe679949820d7971c6854f5b80b4a52fd3122007617f9a","request_ref":"cl22:1e76837a7cd36abf01d9c2aac10a9c609b9af1b03435b83e590c270e045af749","schema":"axon.fabric-execution-attestation/1"}"#;
const KAT_COMPLETION_KEY: &str = "9560211e3d5b588d490e073f28fbba5a3dfe0d9f40b0e0a7bc95a5dc8ce5f656";
const KAT_PASS_TOKEN: &str = "a875b3413a17359d72e634a6f4fe07f2de3e20de9d4d2359b370ac51c0cb9f54";
const KAT_FAIL_TOKEN: &str = "c9ed37e39e339072d5c5984526379387efc65131001b8da55dd17dd69a67f5bd";
const KAT_LEDGER_MAC: &str = "bdda876a50e9d7514e92b4e6cf0f5108a69092bb68f855e0aa3185206f9660c5";
