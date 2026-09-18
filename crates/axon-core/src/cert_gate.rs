//! R23 cert gate — get Z3 out of the TCB for the capability minter, in the
//! DEFAULT build (no `smt` feature, no Z3 in the dependency closure).
//!
//! BOTH pinned mint obligations and their proof CERTIFICATES are embedded here:
//!   * O1 (boolean attenuation) — for each capability axis X, `child_X = want_X ∧
//!     parent_X`, so `(want_X ∧ parent_X) ⇒ parent_X`: a child holds a cap only if
//!     the parent does (a child can never EXCEED its parent). A boolean tautology.
//!   * O2 (budget carve) — `rem ≥ 0 ⇒ max(0, min(g, rem)) ≤ rem`: a child's grant
//!     never exceeds the parent's remaining budget.
//!
//! Under `--require-certificates` (env `AXON_REQUIRE_CERTS=1`), each obligation is
//! discharged ONLY when the SOLVER-FREE checker validates its certificate — so
//! these TCB obligations rest on `axon_certcheck::check` (a few hundred auditable
//! lines), NOT on trusting Z3's verdict. Each certificate is bound to its
//! obligation by content digest, so a mismatched/swapped cert is rejected.
//!
//! This module is COMPILED IN EVERY BUILD (the `axon-certcheck` checker is
//! solver-free and a default dependency) — the gate is NOT behind `smt`, so the
//! shipped default `axon` binary's mint TCB rests on the checker, not on Z3 being
//! linked. The Z3-backed *proof model* (`smt::check_mint_tcb_obligation`) is the
//! separate, opt-in path that PRODUCES these certs; this gate only CHECKS them.
//! Default (flag off) behavior is byte-unchanged: `Ok(())` and no output.
//!
//! The certified obligations are the in-code mint lowering: the shipped
//! `examples/proofs/*.obl` are locked equal to `axon_certcheck::{mint_o1, mint_o2}`
//! by `axon_certcheck::obligations::shipped_obligations_match_the_code_lowering`,
//! so a hand-edited `.obl` (certifying a formula that is NOT the mint law) is
//! caught — the certified obligation IS the mint law, by test, not by trust.

pub const PINNED_MINT_O1_OBLIGATION: &str = include_str!("../../../examples/proofs/mint_o1.obl");
pub const PINNED_MINT_O1_CERTIFICATE: &str = include_str!("../../../examples/proofs/mint_o1.cert");
pub const PINNED_MINT_OBLIGATION: &str = include_str!("../../../examples/proofs/mint_o2.obl");
pub const PINNED_MINT_CERTIFICATE: &str = include_str!("../../../examples/proofs/mint_o2.cert");

/// Whether the `--require-certificates` policy is active (env `AXON_REQUIRE_CERTS=1`).
pub fn require_certificates_enabled() -> bool {
    std::env::var("AXON_REQUIRE_CERTS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}

/// R23: gate BOTH mint TCB obligations (O1 boolean attenuation + O2 budget carve)
/// on a solver-free certificate check when `--require-certificates` is on. `Ok(())`
/// when the policy is off OR both certificates validate; `Err((E1611, …))` (fail
/// closed) if EITHER obligation is not certificate-discharged. The check uses NO
/// solver — Z3 is not in the path. Available in EVERY build.
pub fn check_mint_certificate() -> Result<(), (&'static str, String)> {
    let require = require_certificates_enabled();
    // BOTH O1 and O2 must be certificate-discharged (fail closed on either).
    check_one_mint_obligation(
        "O1",
        PINNED_MINT_O1_OBLIGATION,
        PINNED_MINT_O1_CERTIFICATE,
        require,
    )?;
    check_one_mint_obligation(
        "O2",
        PINNED_MINT_OBLIGATION,
        PINNED_MINT_CERTIFICATE,
        require,
    )?;
    Ok(())
}

/// Run the cert gate in the run/build path: fail closed on a bad cert, and emit
/// the legibility line when the policy is active. Always compiled.
pub fn enforce_or_exit() {
    if let Err((code, msg, exit)) = enforcement_decision() {
        eprintln!("{code}: {msg}");
        std::process::exit(exit);
    }
    if require_certificates_enabled() {
        eprintln!(
            "axon: mint attenuation (O1) + budget-carve (O2) obligations certificate-checked (solver-free; Z3 out of the trust root)"
        );
    }
}

/// What `enforce_or_exit` will DO about the certificate check, as a value.
///
/// Split out because every test in this module called `check_one_mint_obligation`
/// or `check_mint_certificate` — the DETECTION — and none covered the
/// enforcement. Deleting the `process::exit` from the wrapper left the gate
/// printing a failed certificate and continuing, and the whole suite stayed
/// green: the decision was tested, the refusal was not.
///
/// The obligations are compile-time constants, so the failing branch cannot be
/// reached by any input at runtime — which is exactly why it needs a test that
/// does not depend on reaching it. `enforcement_is_fatal_for_a_failed_check`
/// asserts the mapping directly.
pub(crate) fn enforcement_decision() -> Result<(), (&'static str, String, i32)> {
    decide(check_mint_certificate())
}

/// The mapping itself, as a pure function of the check result, so a test can
/// drive the FAILING case that the compile-time constants can never produce.
///
/// Exit 2 is the usage/refusal code: a TCB obligation that does not discharge
/// must stop the run, not annotate it.
fn decide(checked: Result<(), (&'static str, String)>) -> Result<(), (&'static str, String, i32)> {
    match checked {
        Ok(()) => Ok(()),
        Err((code, msg)) => Err((code, msg, 2)),
    }
}

/// Discharge one pinned mint obligation by its embedded certificate (solver-free).
fn check_one_mint_obligation(
    tag: &str,
    obl_src: &str,
    cert_src: &str,
    require: bool,
) -> Result<(), (&'static str, String)> {
    let obl = axon_certcheck::parse_obligation(obl_src).map_err(|e| {
        (
            crate::error::E1611,
            format!("pinned mint {tag} obligation unparseable: {e:?}"),
        )
    })?;
    let cert = axon_certcheck::parse_certificate(cert_src).map_err(|e| {
        (
            crate::error::E1611,
            format!("pinned mint {tag} certificate unparseable: {e:?}"),
        )
    })?;
    match axon_certcheck::require_certificates(&obl, Some(&cert), require) {
        axon_certcheck::RequireOutcome::Discharged => Ok(()),
        axon_certcheck::RequireOutcome::FailClosed { reason } => Err((
            crate::error::E1611,
            format!("mint {tag} obligation not certificate-discharged: {reason}"),
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enforcement_is_fatal_for_a_failed_check() {
        // The mapping `Err(check) -> refuse with exit 2`, asserted without
        // needing to reach the failing branch. A mutation deleting the refusal
        // from `enforce_or_exit` previously survived the entire suite.
        assert!(
            enforcement_decision().is_ok(),
            "the shipped certificates must discharge"
        );
        // Drive the real mapping, not a copy of it: re-implementing the match
        // inline would pass whatever `decide` actually did.
        let (code, _, exit) = decide(Err((crate::error::E1611, "synthetic".to_string())))
            .expect_err("a failed check must not map to success");
        assert!(decide(Ok(())).is_ok(), "a clean check must not refuse");
        assert_eq!(exit, 2, "a non-discharged TCB obligation must refuse");
        assert_eq!(code, crate::error::E1611);
    }

    #[test]
    fn default_off_is_ok_and_silent() {
        // With the policy off (default), the gate is a no-op success.
        assert!(check_mint_certificate().is_ok());
    }

    #[test]
    fn pinned_artifacts_parse_and_check_solver_free() {
        // The shipped certs validate via the solver-free checker (Z3 not linked
        // in the default build), so `require` mode would discharge, not fail.
        for (tag, obl, cert) in [
            ("O1", PINNED_MINT_O1_OBLIGATION, PINNED_MINT_O1_CERTIFICATE),
            ("O2", PINNED_MINT_OBLIGATION, PINNED_MINT_CERTIFICATE),
        ] {
            assert!(
                check_one_mint_obligation(tag, obl, cert, true).is_ok(),
                "{tag} must discharge under require=true via the solver-free checker"
            );
        }
    }

    /// The module header claims each certificate is "bound to its obligation by
    /// content digest, so a mismatched/swapped cert is rejected". Both tests
    /// above only exercise the direction where the gate says YES -- off is
    /// silent, and the shipped pairs discharge. A gate that cannot be shown to
    /// say NO has not been shown to be a gate at all.
    ///
    /// The swap is the realization that matters here: O1 and O2 are both
    /// genuine, both validly certified, and both shipped in the same directory.
    /// If the binding were by schema or by id-shape rather than by digest, each
    /// cert would sail through against the other's obligation and the gate would
    /// still look green on every test above.
    #[test]
    fn a_swapped_certificate_is_rejected_even_though_both_are_valid() {
        for (tag, obl, wrong_cert) in [
            ("O1", PINNED_MINT_O1_OBLIGATION, PINNED_MINT_CERTIFICATE),
            ("O2", PINNED_MINT_OBLIGATION, PINNED_MINT_O1_CERTIFICATE),
        ] {
            let r = check_one_mint_obligation(tag, obl, wrong_cert, true);
            assert!(
                r.is_err(),
                "{tag} discharged against the OTHER obligation's certificate -- \
                 the digest binding is not holding"
            );
            // Fail closed with the R23 code, not some incidental parse error:
            // a swapped cert must be reported as an undischarged obligation.
            let (code, msg) = r.unwrap_err();
            assert_eq!(code, crate::error::E1611, "{tag}: {msg}");
        }
    }

    /// The same swap with the policy OFF must stay silent and successful.
    /// Default behavior is documented as byte-unchanged, and a gate that
    /// starts rejecting when nobody asked it to would break every default run.
    #[test]
    fn a_swapped_certificate_is_ignored_when_the_policy_is_off() {
        assert!(
            check_one_mint_obligation(
                "O1",
                PINNED_MINT_O1_OBLIGATION,
                PINNED_MINT_CERTIFICATE,
                false
            )
            .is_ok(),
            "policy off must not fail closed"
        );
    }
}
