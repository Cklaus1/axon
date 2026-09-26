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

/// The key id of a registered 64-hex public key, or `None` if it is not one.
pub fn key_id_of_hex(public_key_hex: &str) -> Option<String> {
    unhex(public_key_hex)
        .filter(|k| k.len() == 32)
        .map(|k| key_fingerprint(&k))
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

/// A detached signature over ONE document, domain-separated: the signed bytes
/// are the canonical JSON of `{schema, domain, issuer_ref, key_id, doc_ref}`,
/// so a signature made for one kind of document (`domain`) can never be
/// presented as another's. The same key discipline as a receipt attestation:
/// the public key must be the one the operator registered for the issuer.
pub const DOCUMENT_SIGNATURE_SCHEMA: &str = "axon-document-signature/1";

fn document_binding(
    domain: &str,
    issuer_ref: &OpaqueRef,
    key_id: &str,
    doc: &Value,
) -> Result<Value, Refusal> {
    Ok(json!({
        "schema": DOCUMENT_SIGNATURE_SCHEMA,
        "domain": domain,
        "issuer_ref": issuer_ref.as_str(),
        "key_id": key_id,
        "doc_ref": crate::digest_value(doc)?.to_string(),
    }))
}

/// Sign `doc` as `issuer_ref` for `domain`. For issuers (monitors) and tests.
pub fn sign_document(
    pkcs8: &[u8],
    domain: &str,
    issuer_ref: &OpaqueRef,
    doc: &Value,
) -> Result<Value, String> {
    use ring::signature::{Ed25519KeyPair, KeyPair};
    let kp = Ed25519KeyPair::from_pkcs8(pkcs8)
        .map_err(|_| "the issuer key is not a PKCS#8 Ed25519 private key".to_string())?;
    let pk = kp.public_key().as_ref().to_vec();
    let mut s = document_binding(domain, issuer_ref, &key_fingerprint(&pk), doc)
        .map_err(|e| e.to_string())?;
    let bytes = crate::canonical_bytes(&s).map_err(|e| e.to_string())?;
    s["alg"] = json!("ed25519");
    s["public_key"] = json!(hex(&pk));
    s["signature"] = json!(hex(kp.sign(&bytes).as_ref()));
    Ok(s)
}

/// Verify `sig` as `issuer_ref`'s `domain` signature of `doc` under the
/// operator-registered `public_key_hex`. Returns the key id.
pub fn verify_document(
    sig: &Value,
    domain: &str,
    issuer_ref: &OpaqueRef,
    doc: &Value,
    public_key_hex: &str,
) -> Result<String, Refusal> {
    use ring::signature::{UnparsedPublicKey, ED25519};
    let registered = unhex(public_key_hex)
        .filter(|k| k.len() == 32)
        .ok_or_else(|| {
            shape(format!(
                "the key registered for {issuer_ref} is not a 64-hex Ed25519 public key"
            ))
        })?;
    let key_id = key_fingerprint(&registered);
    let want = document_binding(domain, issuer_ref, &key_id, doc)?;
    let obj = sig
        .as_object()
        .ok_or_else(|| shape("signature: not a JSON object"))?;
    let bound = want.as_object().expect("an object");
    for k in obj.keys() {
        if !bound.contains_key(k) && !["alg", "public_key", "signature"].contains(&k.as_str()) {
            return Err(shape(format!("signature: unknown field {k:?}")));
        }
    }
    if sig["alg"] != "ed25519" {
        return Err(shape("signature: alg is not ed25519"));
    }
    let presented = sig["public_key"]
        .as_str()
        .and_then(unhex)
        .filter(|k| k.len() == 32)
        .ok_or_else(|| shape("signature: no 32-byte public_key"))?;
    if presented != registered {
        return Err(shape(format!(
            "signed by {}, not by {key_id}, the key the operator registered for {issuer_ref}",
            key_fingerprint(&presented)
        )));
    }
    for (field, want) in bound {
        if &sig[field] != want {
            return Err(shape(format!(
                "signature: {field} is {} but the document it must vouch for has {want}",
                sig[field]
            )));
        }
    }
    let s = sig["signature"]
        .as_str()
        .and_then(unhex)
        .filter(|s| s.len() == 64)
        .ok_or_else(|| shape("signature: no 64-byte signature"))?;
    UnparsedPublicKey::new(&ED25519, &registered)
        .verify(&crate::canonical_bytes(&want)?, &s)
        .map_err(|_| shape(format!("signature does not verify under {key_id}")))?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> Value {
        let p = format!("{}/tests/fixtures/acf/{name}", env!("CARGO_MANIFEST_DIR"));
        serde_json::from_str(&std::fs::read_to_string(p).expect("fixture")).expect("json")
    }

    fn req() -> ComputeRequest {
        serde_json::from_value(fixture("request_offline.json")).expect("request")
    }

    fn rc() -> ExecutionReceipt {
        serde_json::from_value(fixture("receipt_outcome_unknown.json")).expect("receipt")
    }

    fn issuer() -> OpaqueRef {
        OpaqueRef::new("fabric:verifier").expect("ref")
    }

    /// A key, its public half, and a genuine attestation of (req, rc).
    fn signed() -> (Vec<u8>, String, Value) {
        let (k, pk) = generate().expect("key");
        let doc = sign(&k, &issuer(), &req(), &rc()).expect("sign");
        (k, pk, doc)
    }

    fn refusal(r: Result<String, Refusal>) -> String {
        r.expect_err("must be refused").to_string()
    }

    #[test]
    fn a_genuine_attestation_verifies_and_names_its_key() {
        let (k, pk, doc) = signed();
        assert_eq!(public_key_of(&k).expect("pk"), pk);
        let id = verify(&doc, &issuer(), &req(), &rc(), &pk).expect("verifies");
        assert_eq!(id, key_fingerprint(&unhex(&pk).expect("hex")));
        assert_eq!(doc["key_id"], id.as_str());
        assert!(id.starts_with("ed25519:") && id.len() == "ed25519:".len() + 16);
    }

    /// Every bound field, altered in the document alone: refused by the
    /// binding comparison, before the signature is even consulted.
    #[test]
    fn every_bound_field_is_load_bearing() {
        let (_, pk, doc) = signed();
        for field in [
            "schema",
            "issuer_ref",
            "key_id",
            "request_ref",
            "receipt_ref",
            "operation_id",
            "task_id",
            "trial_id",
            "attempt_id",
            "execution_id",
        ] {
            let mut d = doc.clone();
            d[field] = json!("x");
            assert!(
                verify(&d, &issuer(), &req(), &rc(), &pk).is_err(),
                "{field} altered, still verified"
            );
        }
    }

    /// The document is untouched; what it is checked AGAINST differs.
    #[test]
    fn it_vouches_for_this_request_this_receipt_and_this_issuer_only() {
        let (_, pk, doc) = signed();
        let mut other_rc = serde_json::to_value(rc()).expect("v");
        other_rc["verification"] = json!("failed");
        let other_rc: ExecutionReceipt = serde_json::from_value(other_rc).expect("rc");
        assert!(refusal(verify(&doc, &issuer(), &req(), &other_rc, &pk)).contains("receipt_ref"));

        let mut other_req = serde_json::to_value(req()).expect("v");
        other_req["limits"]["max_cost_micro"] = json!(7);
        let other_req: ComputeRequest = serde_json::from_value(other_req).expect("req");
        assert!(refusal(verify(&doc, &issuer(), &other_req, &rc(), &pk)).contains("request_ref"));

        let other = OpaqueRef::new("fabric:someone-else").expect("ref");
        assert!(refusal(verify(&doc, &other, &req(), &rc(), &pk)).contains("issuer_ref"));
    }

    #[test]
    fn only_the_registered_key_is_accepted() {
        let (_, _, doc) = signed();
        let (_, other_pk) = generate().expect("key");
        assert!(refusal(verify(&doc, &issuer(), &req(), &rc(), &other_pk)).contains("not by"));

        // Self-signed under another key but CLAIMING the registered one: the
        // presented key and key id say "registered", the signature does not.
        let (_, pk, _) = signed();
        let (k2, _) = generate().expect("key");
        let mut forged = sign(&k2, &issuer(), &req(), &rc()).expect("sign");
        forged["public_key"] = json!(pk);
        forged["key_id"] = json!(key_fingerprint(&unhex(&pk).expect("hex")));
        assert!(refusal(verify(&forged, &issuer(), &req(), &rc(), &pk)).contains("does not verify"));
    }

    #[test]
    fn malformed_documents_and_keys_are_refused() {
        let (_, pk, doc) = signed();
        for (why, d) in [
            ("unknown field", {
                let mut d = doc.clone();
                d["note"] = json!(1);
                d
            }),
            ("wrong alg", {
                let mut d = doc.clone();
                d["alg"] = json!("rsa");
                d
            }),
            ("no signature", {
                let mut d = doc.clone();
                d.as_object_mut().expect("obj").remove("signature");
                d
            }),
            ("short signature", {
                let mut d = doc.clone();
                d["signature"] = json!("abcd");
                d
            }),
            ("uppercase hex", {
                let mut d = doc.clone();
                d["signature"] = json!(d["signature"].as_str().expect("s").to_uppercase());
                d
            }),
            ("not an object", json!("attestation")),
        ] {
            assert!(verify(&d, &issuer(), &req(), &rc(), &pk).is_err(), "{why}");
        }
        for bad in ["", "zz", &pk[..62], &pk.to_uppercase()] {
            assert!(
                refusal(verify(&doc, &issuer(), &req(), &rc(), bad)).contains("64-hex"),
                "registered key {bad:?}"
            );
        }
        assert!(public_key_of(b"not pkcs8").is_err());
        assert!(sign(b"not pkcs8", &issuer(), &req(), &rc()).is_err());
    }

    #[test]
    fn a_document_signature_vouches_for_that_document_domain_and_issuer_only() {
        let (k, pk) = generate().expect("key");
        let doc = json!({"schema": "x/1", "finding": "clear", "n": 1});
        let sig = sign_document(&k, "dom/1", &issuer(), &doc).expect("sign");
        let id = verify_document(&sig, "dom/1", &issuer(), &doc, &pk).expect("verifies");
        assert_eq!(sig["key_id"], id.as_str());
        let other_doc = json!({"schema": "x/1", "finding": "clear", "n": 2});
        assert!(
            refusal(verify_document(&sig, "dom/1", &issuer(), &other_doc, &pk)).contains("doc_ref")
        );
        assert!(refusal(verify_document(&sig, "dom/2", &issuer(), &doc, &pk)).contains("domain"));
        let other = OpaqueRef::new("fabric:someone-else").expect("ref");
        assert!(refusal(verify_document(&sig, "dom/1", &other, &doc, &pk)).contains("issuer_ref"));
        let (_, other_pk) = generate().expect("key");
        assert!(
            refusal(verify_document(&sig, "dom/1", &issuer(), &doc, &other_pk)).contains("not by")
        );
        // Self-signed but claiming the registered key: the signature fails.
        let (k2, _) = generate().expect("key");
        let mut forged = sign_document(&k2, "dom/1", &issuer(), &doc).expect("sign");
        forged["public_key"] = json!(pk);
        forged["key_id"] = sig["key_id"].clone();
        assert!(
            refusal(verify_document(&forged, "dom/1", &issuer(), &doc, &pk))
                .contains("does not verify")
        );
        let mut extra = sig.clone();
        extra["note"] = json!(1);
        assert!(verify_document(&extra, "dom/1", &issuer(), &doc, &pk).is_err());
    }
}
