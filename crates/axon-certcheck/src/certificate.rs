//! R23 §3.2 — the `ProofCertificate` format: the evidence the checker validates.
//! Pure (std + sha2 + serde only — NO solver). The certificate lets the checker
//! confirm **the negation of the claim is unsatisfiable** without search: it is a
//! case-split refutation tree whose leaves are linear-arithmetic contradictions,
//! each witnessed by a nonnegative Farkas combination summing to `0 < 0`.
//!
//! The certificate is content-addressed and self-checking (§1.3): it carries the
//! `obligation_digest` it was issued against (binding — a swapped cert fails) and
//! a `cert_digest` over its own canonical refutation bytes (tamper-evidence on
//! the refutation itself).

use crate::obligation::Formula;
use crate::obligation::ParseErr;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

pub const CERT_SCHEMA: &str = "axon-cert/1";

/// A single linear fact `Σ cᵢ·xᵢ  op  k` used as a Farkas hypothesis (§3.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LinFact {
    /// `(coeff, ivar)` pairs — the left-hand side `Σ cᵢ·xᵢ`.
    pub terms: Vec<(i64, String)>,
    /// `le` (≤) or `lt` (<).
    pub op: FactOp,
    /// The right-hand side `k`.
    pub constant: i64,
}

/// The operator of a `LinFact` — only `≤`/`<` (the canonical Farkas form).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FactOp {
    Le,
    Lt,
}

/// The refutation tree (§3.2): a proof that the NEGATED claim is unsatisfiable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "r", rename_all = "snake_case")]
pub enum Refutation {
    /// Split on a boolean variable: BOTH branches must refute.
    BoolCase {
        var: String,
        on_true: Box<Refutation>,
        on_false: Box<Refutation>,
    },
    /// Split on an `Ite`/`Min`/`Max`/`Abs` condition occurring in the formula:
    /// BOTH the cond-true and cond-false branches must refute.
    IteCase {
        cond: Formula,
        on_true: Box<Refutation>,
        on_false: Box<Refutation>,
    },
    /// A leaf: a set of linear ≤/< facts with nonnegative Farkas multipliers
    /// whose weighted sum is a literal contradiction (`0 < 0` / `0 ≤ -1`).
    LinearContradiction {
        facts: Vec<LinFact>,
        coeffs: Vec<i64>,
    },
}

impl Refutation {
    /// Count the nodes in the tree (for the `MAX_CERT_NODES` bound, §4.4 / I-4).
    /// ITERATIVE (an explicit work-stack), so a maliciously deep cert cannot
    /// overflow the stack during the size check itself. Saturates at `limit + 1`
    /// (early termination): callers only need "≤ limit?".
    pub fn bounded_node_count(&self, limit: usize) -> usize {
        let mut count = 0usize;
        let mut stack: Vec<&Refutation> = vec![self];
        while let Some(r) = stack.pop() {
            count += 1;
            if count > limit {
                return count; // saturate — definitely over the bound
            }
            match r {
                Refutation::LinearContradiction { facts, .. } => {
                    count += facts.len();
                    if count > limit {
                        return count;
                    }
                }
                Refutation::BoolCase {
                    on_true, on_false, ..
                }
                | Refutation::IteCase {
                    on_true, on_false, ..
                } => {
                    stack.push(on_true);
                    stack.push(on_false);
                }
            }
        }
        count
    }

    /// The exact node count (recursive; for small trees / tests only — callers on
    /// the trusted path use `bounded_node_count`).
    pub fn node_count(&self) -> usize {
        self.bounded_node_count(usize::MAX)
    }
}

/// The proof certificate (§3.2), serialized as `axon-cert/1`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProofCertificate {
    pub schema: String,
    pub obligation_id: String,
    /// `"axsha256:"+sha256(canonical obligation)` — binds cert to obligation.
    pub obligation_digest: String,
    pub refutation: Refutation,
    /// `"axcert1:"+sha256(canonical(refutation))`.
    pub cert_digest: String,
}

impl ProofCertificate {
    /// Build a certificate, computing both digests (used by the prover/emit and
    /// by example-artifact generation). Pure.
    pub fn new(
        obligation_id: &str,
        obligation_digest: &str,
        refutation: Refutation,
    ) -> ProofCertificate {
        let cert_digest = Self::compute_cert_digest(&refutation);
        ProofCertificate {
            schema: CERT_SCHEMA.to_string(),
            obligation_id: obligation_id.to_string(),
            obligation_digest: obligation_digest.to_string(),
            refutation,
            cert_digest,
        }
    }

    /// `"axcert1:"+sha256(canonical(refutation))`. The canonical bytes are the
    /// refutation's own JSON (deterministic field order, no whitespace).
    pub fn compute_cert_digest(refutation: &Refutation) -> String {
        let canon = serde_json::to_string(refutation).expect("Refutation serializes");
        let mut h = Sha256::new();
        h.update(canon.as_bytes());
        format!("axcert1:{:x}", h.finalize())
    }

    /// Parse a certificate from its JSON form. Schema-checked. Does NOT validate
    /// the proof (that is the checker's job) — only the envelope.
    pub fn parse(s: &str) -> Result<ProofCertificate, ParseErr> {
        let cert: ProofCertificate = serde_json::from_str(s).map_err(|e| ParseErr {
            detail: format!("malformed certificate JSON: {e}"),
        })?;
        if cert.schema != CERT_SCHEMA {
            return Err(ParseErr {
                detail: format!("unexpected schema {:?} (want {CERT_SCHEMA})", cert.schema),
            });
        }
        Ok(cert)
    }

    /// Serialize to canonical JSON (deterministic — §4.5).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).expect("ProofCertificate serializes")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn leaf() -> Refutation {
        Refutation::LinearContradiction {
            facts: vec![LinFact {
                terms: vec![(1, "x".into())],
                op: FactOp::Le,
                constant: -1,
            }],
            coeffs: vec![1],
        }
    }

    #[test]
    fn cert_roundtrips_and_digest_is_stable() {
        let c = ProofCertificate::new("carve/return", "axsha256:abc", leaf());
        let js = c.to_json();
        let back = ProofCertificate::parse(&js).unwrap();
        assert_eq!(back, c);
        assert!(c.cert_digest.starts_with("axcert1:"));
        // The digest is purely a function of the refutation bytes.
        assert_eq!(
            c.cert_digest,
            ProofCertificate::compute_cert_digest(&c.refutation)
        );
    }

    #[test]
    fn node_count_counts_the_tree() {
        let t = Refutation::BoolCase {
            var: "b".into(),
            on_true: Box::new(leaf()),
            on_false: Box::new(leaf()),
        };
        // 1 (BoolCase) + (1 leaf + 1 fact)*2 = 1 + 2 + 2 = 5
        assert_eq!(t.node_count(), 5);
    }

    #[test]
    fn wrong_schema_is_rejected() {
        let err = ProofCertificate::parse(
            r#"{"schema":"nope","obligation_id":"x","obligation_digest":"d","refutation":{"r":"linear_contradiction","facts":[],"coeffs":[]},"cert_digest":"c"}"#,
        )
        .unwrap_err();
        assert!(err.detail.contains("schema"));
    }
}
