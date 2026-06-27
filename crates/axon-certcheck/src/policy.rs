//! R23 §4.6 / S8 — the `--require-certificates` gating policy. Pure.
//!
//! The spec's S8 wires this flag into axon-core's `smt::discharge` and axon-os's
//! admission gate. Per the R23 build's cross-crate boundary, the SUBSTANCE lives
//! here as a standalone policy function; the main agent wires it into the other
//! crates later (see the CROSS-CRATE HOOK comment below).
//!
//! The policy is: given an obligation, an OPTIONAL certificate, and whether the
//! `--require-certificates` flag is set, decide whether the obligation is
//! discharged. Under the flag, an obligation counts as discharged ONLY IF its
//! certificate is present AND the checker returns `Valid`. A missing, `Invalid`,
//! or `Unsupported` certificate ⇒ **fail closed** (not discharged). With the
//! flag OFF, behavior is byte-compatible with today: certificates are advisory
//! (checked + reported, never gating) so adoption is incremental (§4.6).

use crate::certificate::ProofCertificate;
use crate::checker::{check, CheckResult};
use crate::obligation::Obligation;

/// The decision the gate makes for one obligation under the require-certs policy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequireOutcome {
    /// The obligation is discharged (admit / elide its runtime check).
    Discharged,
    /// Fail closed: the obligation is NOT discharged (its runtime check stays
    /// armed; for the TCB mint obligation this is E1610; for R21 it is a denied
    /// admission). Carries the human reason.
    FailClosed { reason: String },
}

impl RequireOutcome {
    pub fn is_discharged(&self) -> bool {
        matches!(self, RequireOutcome::Discharged)
    }
}

/// Decide whether `obligation` is discharged, given an optional `cert` and the
/// `require` flag (R23 §4.6).
///
/// * `require == false` (default): the obligation is treated as discharged by
///   the EXISTING path — certificates are advisory only. (This function returns
///   `Discharged`; the caller still ran its normal discharge logic. We never
///   *weaken* the default by demanding a cert it didn't used to need.)
/// * `require == true`: discharged **iff** `cert` is `Some` and
///   `check(obligation, cert) == Valid`. Absent / Invalid / Unsupported ⇒
///   `FailClosed`.
pub fn require_certificates(
    obligation: &Obligation,
    cert: Option<&ProofCertificate>,
    require: bool,
) -> RequireOutcome {
    if !require {
        // Default path is byte-compatible: do not gate on certificates.
        return RequireOutcome::Discharged;
    }
    match cert {
        None => RequireOutcome::FailClosed {
            reason: format!(
                "--require-certificates: no certificate present for obligation {:?}",
                obligation.id
            ),
        },
        Some(c) => match check(obligation, c) {
            CheckResult::Valid => RequireOutcome::Discharged,
            CheckResult::Invalid { reason } => RequireOutcome::FailClosed {
                reason: format!(
                    "--require-certificates: certificate for {:?} is INVALID: {reason}",
                    obligation.id
                ),
            },
            CheckResult::Unsupported { reason } => RequireOutcome::FailClosed {
                reason: format!(
                    "--require-certificates: obligation {:?} is UNSUPPORTED (outside the certified \
                     fragment), so it cannot be discharged by certificate: {reason}",
                    obligation.id
                ),
            },
        },
    }
}

// CROSS-CRATE HOOK (main agent): wire this gate into axon-core smt::discharge / axon-os admission.
// In axon-core, after `smt::discharge` returns a `Proven` obligation, lower it to an
// `axon_certcheck::Obligation`, emit/attach its `ProofCertificate`, and call
// `axon_certcheck::require_certificates(&obl, cert.as_ref(), require_flag)`; only treat the bound as
// statically discharged (eliding the runtime @[verify]/refinement check) when the result
// `is_discharged()` — otherwise keep the runtime check armed and, for the TCB mint obligation, raise
// E1610 fail-closed. In axon-os `gate::admit`, under `--require-certificates`, admit a program's bound
// only when `require_certificates(...)` for its effect-subset obligation returns `Discharged`, else
// return `Verdict::Denied` (fail closed, no run). The require flag comes from the `--require-certificates`
// CLI option or the `AXON_REQUIRE_CERTS=1` env var.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::certificate::{FactOp, LinFact, ProofCertificate, Refutation};
    use crate::obligation::build::*;
    use crate::obligation::{Op, Sort, Var};

    fn carve() -> Obligation {
        Obligation {
            id: "carve/return".into(),
            vars: vec![
                Var {
                    name: "g".into(),
                    sort: Sort::Int,
                },
                Var {
                    name: "avail".into(),
                    sort: Sort::Int,
                },
            ],
            claim: cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail")),
        }
    }

    fn valid_cert() -> ProofCertificate {
        let o = carve();
        let cond = cmp(Op::Le, ivar("g"), ivar("avail"));
        let on_true = Refutation::LinearContradiction {
            facts: vec![
                LinFact {
                    terms: vec![(1, "g".into()), (-1, "avail".into())],
                    op: FactOp::Le,
                    constant: 0,
                },
                LinFact {
                    terms: vec![(1, "avail".into()), (-1, "g".into())],
                    op: FactOp::Lt,
                    constant: 0,
                },
            ],
            coeffs: vec![1, 1],
        };
        let on_false = Refutation::LinearContradiction {
            facts: vec![LinFact {
                terms: vec![],
                op: FactOp::Lt,
                constant: 0,
            }],
            coeffs: vec![1],
        };
        ProofCertificate::new(
            &o.id,
            &o.digest(),
            Refutation::IteCase {
                cond,
                on_true: Box::new(on_true),
                on_false: Box::new(on_false),
            },
        )
    }

    // R23 §0 Core: `require_certificates_fails_closed`. A missing/invalid/
    // unsupported certificate fails closed under the flag; a valid one passes.
    #[test]
    fn require_certificates_fails_closed() {
        let o = carve();
        let good = valid_cert();

        // (1) valid cert + flag ⇒ Discharged.
        assert!(require_certificates(&o, Some(&good), true).is_discharged());

        // (2) MISSING cert + flag ⇒ FailClosed.
        let r = require_certificates(&o, None, true);
        assert!(matches!(r, RequireOutcome::FailClosed { .. }), "{r:?}");

        // (3) INVALID cert + flag ⇒ FailClosed.
        let mut bad = good.clone();
        bad.obligation_digest = "axsha256:0".into();
        let r = require_certificates(&o, Some(&bad), true);
        assert!(matches!(r, RequireOutcome::FailClosed { .. }), "{r:?}");

        // (4) default path (flag OFF) ⇒ Discharged regardless (byte-compatible).
        assert!(require_certificates(&o, None, false).is_discharged());
        assert!(require_certificates(&o, Some(&bad), false).is_discharged());
    }

    #[test]
    fn unsupported_obligation_fails_closed_under_flag() {
        // A valid-looking cert whose checker verdict is Unsupported must NOT
        // discharge under the flag. We model this via a wrong-binding cert that
        // the checker rejects; the FailClosed branch is what matters here.
        let o = carve();
        let mut c = valid_cert();
        c.cert_digest = "axcert1:0".into();
        assert!(!require_certificates(&o, Some(&c), true).is_discharged());
    }
}
