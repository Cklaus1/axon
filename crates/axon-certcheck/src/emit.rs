//! R23 §5.1 / S5 — prover-side certificate emission. UNTRUSTED (behind `smt`):
//! Z3 is used only as an ORACLE to confirm the obligation is actually valid
//! (its negation is UNSAT); the certificate itself is built by a small,
//! deterministic constructive synthesizer (`synth`). Because the TRUSTED checker
//! re-derives everything from the certificate, the synthesizer's correctness is
//! not trust-critical — but it must produce certs the checker accepts, and it
//! does so byte-deterministically (A5).
//!
//! The synthesis half is pure and always compiled (so example artifacts can be
//! generated without a solver); the Z3 oracle gate is `#[cfg(feature = "smt")]`.

use crate::certificate::ProofCertificate;
use crate::obligation::Obligation;
use crate::synth;

/// Why `prove` could not emit a certificate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProveErr {
    /// The obligation is outside the certified fragment (nonlinear, etc.).
    Unsupported { reason: String },
    /// The obligation is FALSE: Z3 found a counterexample (a SAT model of the
    /// negation). No valid certificate exists (soundness, §4.2).
    Counterexample { reason: String },
    /// An internal synthesis/oracle error.
    Internal { reason: String },
}

/// Find a proof via the Z3 oracle and EMIT a certificate (§5.1). The oracle only
/// CONFIRMS validity; the certificate is built by the deterministic synthesizer.
/// Returns `Counterexample` for a false obligation (so a lying prover cannot make
/// `prove` emit a cert for something false — Z3 returns SAT).
pub fn prove(obligation: &Obligation) -> Result<ProofCertificate, ProveErr> {
    // (1) Oracle gate: is the negation UNSAT (i.e. the claim ∀-true)?
    oracle_confirm_valid(obligation)?;
    // (2) Construct the certificate deterministically.
    synth::synthesize(obligation).map_err(|e| ProveErr::Unsupported { reason: e })
}

/// Confirm via Z3 that `Not(claim)` is unsatisfiable. SAT ⇒ the obligation is
/// false ⇒ `Counterexample`. Unknown / encoding gap ⇒ `Unsupported`.
#[cfg(feature = "smt")]
fn oracle_confirm_valid(obligation: &Obligation) -> Result<(), ProveErr> {
    use z3::ast::{Bool, Int};
    use z3::{Config, Context, SatResult, Solver};

    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let solver = Solver::new(&ctx);

    // Map each declared variable to a Z3 constant.
    use crate::obligation::Sort;
    use std::collections::HashMap;
    let mut ints: HashMap<String, Int> = HashMap::new();
    let mut bools: HashMap<String, Bool> = HashMap::new();
    for v in &obligation.vars {
        match v.sort {
            Sort::Int => {
                ints.insert(v.name.clone(), Int::new_const(&ctx, v.name.clone()));
            }
            Sort::Bool => {
                bools.insert(v.name.clone(), Bool::new_const(&ctx, v.name.clone()));
            }
        }
    }

    let claim = encode_formula(&ctx, &obligation.claim, &ints, &bools)
        .map_err(|reason| ProveErr::Unsupported { reason })?;
    // Assert the NEGATION; UNSAT ⇒ valid.
    solver.assert(&claim.not());
    match solver.check() {
        SatResult::Unsat => Ok(()),
        SatResult::Sat => Err(ProveErr::Counterexample {
            reason: "Z3 found an input violating the claim (the obligation is not ∀-true)".into(),
        }),
        SatResult::Unknown => Err(ProveErr::Unsupported {
            reason: "Z3 returned `unknown` (outside the tractable fragment)".into(),
        }),
    }
}

#[cfg(not(feature = "smt"))]
fn oracle_confirm_valid(_obligation: &Obligation) -> Result<(), ProveErr> {
    Err(ProveErr::Internal {
        reason: "prove requires the `smt` build".into(),
    })
}

#[cfg(feature = "smt")]
fn encode_formula<'c>(
    ctx: &'c z3::Context,
    f: &crate::obligation::Formula,
    ints: &std::collections::HashMap<String, z3::ast::Int<'c>>,
    bools: &std::collections::HashMap<String, z3::ast::Bool<'c>>,
) -> Result<z3::ast::Bool<'c>, String> {
    use crate::obligation::{Formula, Op};
    use z3::ast::{Ast, Bool};
    match f {
        Formula::Bool { v } => Ok(Bool::from_bool(ctx, *v)),
        Formula::BVar { name } => bools
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown bool var {name}")),
        Formula::Not { f } => Ok(encode_formula(ctx, f, ints, bools)?.not()),
        Formula::And { fs } => {
            let parts: Result<Vec<_>, _> = fs
                .iter()
                .map(|g| encode_formula(ctx, g, ints, bools))
                .collect();
            let parts = parts?;
            let refs: Vec<&Bool> = parts.iter().collect();
            Ok(Bool::and(ctx, &refs))
        }
        Formula::Or { fs } => {
            let parts: Result<Vec<_>, _> = fs
                .iter()
                .map(|g| encode_formula(ctx, g, ints, bools))
                .collect();
            let parts = parts?;
            let refs: Vec<&Bool> = parts.iter().collect();
            Ok(Bool::or(ctx, &refs))
        }
        Formula::Implies { a, b } => {
            Ok(encode_formula(ctx, a, ints, bools)?.implies(&encode_formula(ctx, b, ints, bools)?))
        }
        Formula::Cmp { op, l, r } => {
            let lt = encode_term(ctx, l, ints)?;
            let rt = encode_term(ctx, r, ints)?;
            Ok(match op {
                Op::Lt => lt.lt(&rt),
                Op::Le => lt.le(&rt),
                Op::Gt => lt.gt(&rt),
                Op::Ge => lt.ge(&rt),
                Op::Eq => lt._eq(&rt),
                Op::Ne => lt._eq(&rt).not(),
            })
        }
    }
}

#[cfg(feature = "smt")]
fn encode_term<'c>(
    ctx: &'c z3::Context,
    t: &crate::obligation::Term,
    ints: &std::collections::HashMap<String, z3::ast::Int<'c>>,
) -> Result<z3::ast::Int<'c>, String> {
    use crate::obligation::Term;
    use z3::ast::Int;
    match t {
        Term::Int { v } => Ok(Int::from_i64(ctx, *v)),
        Term::IVar { name } => ints
            .get(name)
            .cloned()
            .ok_or_else(|| format!("unknown int var {name}")),
        Term::Add { l, r } => Ok(encode_term(ctx, l, ints)? + encode_term(ctx, r, ints)?),
        Term::Sub { l, r } => Ok(encode_term(ctx, l, ints)? - encode_term(ctx, r, ints)?),
        Term::MulConst { k, t } => Ok(Int::from_i64(ctx, *k) * encode_term(ctx, t, ints)?),
        Term::Min { l, r } => {
            let a = encode_term(ctx, l, ints)?;
            let b = encode_term(ctx, r, ints)?;
            Ok(a.le(&b).ite(&a, &b))
        }
        Term::Max { l, r } => {
            let a = encode_term(ctx, l, ints)?;
            let b = encode_term(ctx, r, ints)?;
            Ok(a.ge(&b).ite(&a, &b))
        }
        Term::Abs { t } => {
            let a = encode_term(ctx, t, ints)?;
            let zero = Int::from_i64(ctx, 0);
            let neg = Int::from_i64(ctx, -1) * a.clone();
            Ok(a.ge(&zero).ite(&a, &neg))
        }
        Term::Ite { cond, then, els } => {
            let c = encode_formula_ite(ctx, cond, ints)?;
            Ok(c.ite(
                &encode_term(ctx, then, ints)?,
                &encode_term(ctx, els, ints)?,
            ))
        }
    }
}

#[cfg(feature = "smt")]
fn encode_formula_ite<'c>(
    ctx: &'c z3::Context,
    f: &crate::obligation::Formula,
    ints: &std::collections::HashMap<String, z3::ast::Int<'c>>,
) -> Result<z3::ast::Bool<'c>, String> {
    // Ite conditions in the fragment are linear comparisons; encode them without
    // needing the bool map.
    let empty = std::collections::HashMap::new();
    encode_formula(ctx, f, ints, &empty)
}

#[cfg(all(test, feature = "smt"))]
mod smt_tests {
    use super::*;
    use crate::checker::{check, CheckResult};
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

    #[test]
    fn prove_then_check_is_valid() {
        let o = carve();
        let cert = prove(&o).expect("carve is provable");
        assert_eq!(check(&o, &cert), CheckResult::Valid);
    }

    #[test]
    fn false_obligation_yields_counterexample() {
        // min(g, avail) ≥ avail is false for g < avail.
        let mut o = carve();
        o.claim = cmp(Op::Ge, min(ivar("g"), ivar("avail")), ivar("avail"));
        o.id = "carve/false".into();
        assert!(matches!(prove(&o), Err(ProveErr::Counterexample { .. })));
    }

    #[test]
    fn emission_is_byte_identical() {
        let o = carve();
        let a = prove(&o).unwrap().to_json();
        let b = prove(&o).unwrap().to_json();
        assert_eq!(a, b, "emission must be deterministic (A5)");
    }
}
