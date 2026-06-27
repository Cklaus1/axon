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
    if let Err((code, msg)) = check_mint_certificate() {
        eprintln!("{code}: {msg}");
        std::process::exit(2);
    }
    if require_certificates_enabled() {
        eprintln!(
            "axon: mint attenuation (O1) + budget-carve (O2) obligations certificate-checked (solver-free; Z3 out of the trust root)"
        );
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
}
