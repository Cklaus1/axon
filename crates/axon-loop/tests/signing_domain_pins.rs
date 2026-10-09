//! Amendment 115 (C9 round 13, eqgate9): THIS crate's signing domains, pinned by this crate's own suite.
//!
//! `tests/signing_domains.rs` in axon-fabric kills a collapse of two signing domains, but only when axon-fabric's
//! suite is the one run: against axon-loop alone, `CLEARANCE_DOMAIN := CONTEXT_DOMAIN` (and the reverse)
//! survived, and so did `EXECUTION_DOMAIN` collapsing onto either. A signature minted under one domain would
//! then verify as the other protocol's. Each crate that owns a domain now asserts, in its own suite, that the
//! constant equals its documented literal and differs from every other domain that shares a signing protocol
//! with it.

use axon_loop::evl::CONTEXT_DOMAIN;
use axon_loop::safety::CLEARANCE_DOMAIN;
use axon_loop_contracts::attestation::{
    ATTESTATION_SCHEMA, DOCUMENT_SIGNATURE_SCHEMA, EXECUTION_DOMAIN,
};

#[test]
fn this_crates_signing_domains_are_their_documented_literals() {
    assert_eq!(
        CLEARANCE_DOMAIN, "axon.loop.trial-safety/1",
        "ATTACK: the trial-safety clearance domain is not its documented literal"
    );
    assert_eq!(
        CONTEXT_DOMAIN, "axon.closed-loop.context/1",
        "ATTACK: the closed-loop context domain is not its documented literal"
    );
}

#[test]
fn this_crates_signing_domains_are_distinct_from_every_domain_they_share_a_protocol_with() {
    // every document signature of this crate is `sign_document`/`verify_document` over ONE of these
    let all = [
        ("CLEARANCE_DOMAIN", CLEARANCE_DOMAIN),
        ("CONTEXT_DOMAIN", CONTEXT_DOMAIN),
        ("EXECUTION_DOMAIN", EXECUTION_DOMAIN),
        ("DOCUMENT_SIGNATURE_SCHEMA", DOCUMENT_SIGNATURE_SCHEMA),
        ("ATTESTATION_SCHEMA", ATTESTATION_SCHEMA),
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
}
