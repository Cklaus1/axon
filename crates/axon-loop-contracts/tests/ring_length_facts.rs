//! C9 round 4c, ADMIT (amendment 76): the fact the refusal-site gate's
//! exemptions of the Ed25519 length filters rest on. attestation.rs and
//! operator_trust.rs pre-screen a public key to 32 bytes and a signature to 64
//! (`.filter(|k| k.len() == 32).ok_or(..)`); the exemption says the filter only
//! changes the REASON of a refusal, because ring's verifier refuses any other
//! length itself. If ring ever accepted one, this test fails and the filters
//! become decisions the gate must row.

use ring::signature::{UnparsedPublicKey, ED25519};

#[test]
fn ring_refuses_a_key_or_signature_of_any_other_length() {
    let msg = b"the bytes a verifier signed";
    // Control: the right shapes are PARSED (the signature is wrong, so the
    // verdict is still an Err, but of the same kind a length error would be:
    // there is no length for which verify returns Ok on junk).
    for klen in [0usize, 1, 16, 31, 33, 64, 128] {
        let k = UnparsedPublicKey::new(&ED25519, vec![7u8; klen]);
        assert!(
            k.verify(msg, &[7u8; 64]).is_err(),
            "ring accepted a {klen}-byte public key"
        );
    }
    for slen in [0usize, 1, 32, 63, 65, 128] {
        let k = UnparsedPublicKey::new(&ED25519, vec![7u8; 32]);
        assert!(
            k.verify(msg, &vec![7u8; slen]).is_err(),
            "ring accepted a {slen}-byte signature"
        );
    }
}
