//! v0.22 G01-r22-independent-issuer / G32-r22-sidecar-bindings: a verifier's
//! receipt, AUTHENTICATED.
//!
//! A receipt's JSON and its digests can be reproduced by anyone who has seen
//! one, and an `issuer_ref` is only a name: before this, "trusted issuer" meant
//! "a string on the operator's list", so a subject that copied the string (or a
//! worker that claimed it) was indistinguishable from the verifier (FG-050).
//! The receipt's schema is vendored and closed, so the proof is DETACHED: an
//! `acf-receipt-attestation/1` in which the issuer signs, with an Ed25519 key,
//! the canonical JSON of one explicit, domain-separated binding:
//!
//! * `schema` — the domain: these bytes are an attestation of this version and
//!   can be nothing else;
//! * `issuer_ref` and `key_id` — who vouches, and with which key (the id is
//!   the key's fingerprint, so a later rotation adds keys without changing
//!   what an existing attestation signed);
//! * `request_ref` — cl22 of the request answered: registered executable and
//!   digest (verifier revision), job kind, limits and required profile
//!   (configuration);
//! * `receipt_ref` — cl22 of the receipt: status, verdict, matched checks,
//!   output workspace (artifact), backend profile, policy digest;
//! * `operation_id`, `task_id`, `trial_id`, `attempt_id`, `execution_id` — the
//!   receipt's identity, stated in the signed bytes even though the digests
//!   imply it, so no reading of the attestation depends on re-deriving it.
//!
//! Cryptographic authenticity does NOT replace the semantic joins: intake still
//! requires that this signed pair is the one the sidecar cites, for THIS trial,
//! over THIS output tree, run as a principal that is not the subject.

use crate::error::{shape, Refusal};
use crate::{ComputeRequest, ExecutionReceipt, OpaqueRef};
use serde_json::{json, Value};

pub const ATTESTATION_SCHEMA: &str = "acf-receipt-attestation/1";

/// Every field an attestation binds, in one place: the signed bytes are the
/// canonical JSON of exactly this object.
fn binding(
    issuer_ref: &OpaqueRef,
    key_id: &str,
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
) -> Result<Value, Refusal> {
    Ok(json!({
        "schema": ATTESTATION_SCHEMA,
        "issuer_ref": issuer_ref.as_str(),
        "key_id": key_id,
        "request_ref": crate::digest(req)?.to_string(),
        "receipt_ref": crate::digest(rc)?.to_string(),
        "operation_id": rc.operation_id.as_str(),
        "task_id": rc.task_id.as_str(),
        "trial_id": rc.trial_id.as_str(),
        "attempt_id": rc.attempt_id.as_str(),
        "execution_id": rc.execution_id.as_str(),
    }))
}

/// `ed25519:<first 16 hex of sha256(public key)>` — a key's id.
pub fn key_fingerprint(public_key: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let h = Sha256::digest(public_key);
    format!("ed25519:{}", &hex(&h)[..16])
}

/// A fresh Ed25519 key: `(PKCS#8 private key, 64-hex public key)`. For key
/// provisioning (`axon-fabric keygen`) and test fixtures.
pub fn generate() -> Result<(Vec<u8>, String), String> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let pkcs8 = Ed25519KeyPair::generate_pkcs8(&ring::rand::SystemRandom::new())
        .map_err(|_| "key generation failed".to_string())?;
    let pk = Ed25519KeyPair::from_pkcs8(pkcs8.as_ref())
        .map_err(|_| "a generated key does not load".to_string())?
        .public_key()
        .as_ref()
        .to_vec();
    Ok((pkcs8.as_ref().to_vec(), hex(&pk)))
}

/// The 64-hex public key of a PKCS#8 Ed25519 private key.
pub fn public_key_of(pkcs8: &[u8]) -> Result<String, String> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_pkcs8(pkcs8)
        .map_err(|_| "not a PKCS#8 Ed25519 private key".to_string())?;
    Ok(hex(kp.public_key().as_ref()))
}

/// Sign the attestation of `rc` answering `req`, as `issuer_ref`. For the
/// issuer (Fabric's operator-configured signer) and tests only.
pub fn sign(
    pkcs8: &[u8],
    issuer_ref: &OpaqueRef,
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
) -> Result<Value, String> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_pkcs8(pkcs8)
        .map_err(|_| "the issuer key is not a PKCS#8 Ed25519 private key".to_string())?;
    let pk = kp.public_key().as_ref().to_vec();
    let mut doc = binding(issuer_ref, &key_fingerprint(&pk), req, rc).map_err(|e| e.to_string())?;
    let bytes = crate::canonical_bytes(&doc).map_err(|e| e.to_string())?;
    doc["alg"] = json!("ed25519");
    doc["public_key"] = json!(hex(&pk));
    doc["signature"] = json!(hex(kp.sign(&bytes).as_ref()));
    Ok(doc)
}

/// Verify `doc` as `issuer_ref`'s attestation of `rc` answering `req`, under
/// the operator-registered `public_key_hex`. Returns the key id. Each refusal
/// names its rule.
pub fn verify(
    doc: &Value,
    issuer_ref: &OpaqueRef,
    req: &ComputeRequest,
    rc: &ExecutionReceipt,
    public_key_hex: &str,
) -> Result<String, Refusal> {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let registered = unhex(public_key_hex)
        .filter(|k| k.len() == 32)
        .ok_or_else(|| {
            shape(format!(
                "the key registered for verifier {issuer_ref} is not a 64-hex Ed25519 public key"
            ))
        })?;
    let key_id = key_fingerprint(&registered);
    let want = binding(issuer_ref, &key_id, req, rc)?;
    let obj = doc
        .as_object()
        .ok_or_else(|| shape("attestation: not a JSON object"))?;
    let bound = want.as_object().expect("an object");
    for k in obj.keys() {
        if !bound.contains_key(k) && !["alg", "public_key", "signature"].contains(&k.as_str()) {
            return Err(shape(format!("attestation: unknown field {k:?}")));
        }
    }
    if doc["schema"] != ATTESTATION_SCHEMA || doc["alg"] != "ed25519" {
        return Err(shape(format!(
            "attestation: not {ATTESTATION_SCHEMA} with alg ed25519"
        )));
    }
    let presented = doc["public_key"]
        .as_str()
        .and_then(unhex)
        .filter(|k| k.len() == 32)
        .ok_or_else(|| shape("attestation: no 32-byte public_key"))?;
    if presented != registered {
        return Err(shape(format!(
            "attestation is signed by {}, not by {key_id}, the key the operator registered for \
             verifier {issuer_ref}",
            key_fingerprint(&presented)
        )));
    }
    for (field, want) in bound {
        if &doc[field] != want {
            return Err(shape(format!(
                "attestation: {field} is {} but the evidence it must vouch for has {want}",
                doc[field]
            )));
        }
    }
    let sig = doc["signature"]
        .as_str()
        .and_then(unhex)
        .filter(|s| s.len() == 64)
        .ok_or_else(|| shape("attestation: no 64-byte signature"))?;
    let bytes = crate::canonical_bytes(&want)?;
    UnparsedPublicKey::new(&ED25519, &registered)
        .verify(&bytes, &sig)
        .map_err(|_| {
            shape(format!(
                "attestation signature does not verify under {key_id}: these are not the bytes \
                 verifier {issuer_ref} signed"
            ))
        })?;
    Ok(key_id)
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    if !s.len().is_multiple_of(2)
        || !s
            .bytes()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
