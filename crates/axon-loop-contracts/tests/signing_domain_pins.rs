//! Amendment 115 (C9 round 13, eqgate9): THIS crate's signing domains, pinned by this crate's own suite.
//!
//! The execution-attestation domain collapsing onto the closed-loop context domain, or onto the trial-safety
//! clearance domain, survived this crate's whole suite: only axon-fabric's `signing_domains` test saw it. The
//! two sibling domains live in axon-loop (which depends on this crate, so it cannot be imported here); their
//! documented literals are therefore written out, and the axon-loop crate pins the same literals from its
//! side (`crates/axon-loop/tests/signing_domain_pins.rs`).

use axon_loop_contracts::attestation::{
    ATTESTATION_SCHEMA, DOCUMENT_SIGNATURE_SCHEMA, EXECUTION_DOMAIN,
};
use axon_loop_contracts::operator_trust::{TrustAuthority, EVIDENCE_SIGNATURE_SCHEMA};

const SIBLING_CLEARANCE_DOMAIN: &str = "axon.loop.trial-safety/1";
const SIBLING_CONTEXT_DOMAIN: &str = "axon.closed-loop.context/1";

#[test]
fn this_crates_signing_domains_are_their_documented_literals() {
    for (name, got, want) in [
        (
            "EXECUTION_DOMAIN",
            EXECUTION_DOMAIN,
            "axon.fabric-execution/1",
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
    ] {
        assert_eq!(
            got, want,
            "ATTACK: signing domain {name} is not its documented literal"
        );
    }
}

#[test]
fn this_crates_signing_domains_are_distinct_from_each_other_and_from_the_sibling_crates_domains() {
    let all = [
        ("EXECUTION_DOMAIN", EXECUTION_DOMAIN),
        ("DOCUMENT_SIGNATURE_SCHEMA", DOCUMENT_SIGNATURE_SCHEMA),
        ("ATTESTATION_SCHEMA", ATTESTATION_SCHEMA),
        ("EVIDENCE_SIGNATURE_SCHEMA", EVIDENCE_SIGNATURE_SCHEMA),
        ("axon-loop CLEARANCE_DOMAIN", SIBLING_CLEARANCE_DOMAIN),
        ("axon-loop CONTEXT_DOMAIN", SIBLING_CONTEXT_DOMAIN),
    ];
    for (i, (an, av)) in all.iter().enumerate() {
        for (bn, bv) in &all[i + 1..] {
            assert_ne!(
                av, bv,
                "ATTACK: signing domain collapse: {an} == {bn} ({av:?}); a signature minted for one \
                 protocol verifies as the other"
            );
        }
    }
    // the evidence signature's own domain axis: one directory name per trust authority
    let names: Vec<_> = TrustAuthority::ALL.iter().map(|a| a.dir_name()).collect();
    for (i, a) in names.iter().enumerate() {
        for b in &names[i + 1..] {
            assert_ne!(
                a, b,
                "ATTACK: signing domain collapse: trust authority {a} == {b}"
            );
        }
    }
}
