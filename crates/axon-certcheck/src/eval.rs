//! R23 §4.1 (step 1–3 arithmetic) — the checker's OWN linear-integer + boolean
//! evaluator. Pure, total, solver-free, and uses CHECKED integer arithmetic
//! everywhere (overflow → an error, NEVER a wrapped spurious contradiction; the
//! adversary controls the certificate's coefficients — §12).
//!
//! This module is part of the TRUSTED checker. Keep it obvious: prefer plain
//! arithmetic to cleverness. It provides:
//!   * `LinComb` — a normalized linear combination `Σ cᵢ·xᵢ + c` over int vars;
//!   * `term_to_lincomb` — normalize a (branch-resolved) `Term` to a `LinComb`,
//!     FAILING on any residual nonlinearity (`Min/Max/Abs/Ite` must already be
//!     resolved by the certificate's case-splits);
//!   * `LinIneq` — a normalized linear inequality `Σ cᵢ·xᵢ  {Le|Lt}  k`;
//!   * `farkas_combine` — the nonnegative-combination arithmetic that the
//!     checker uses to confirm a leaf reduces to a literal `0 < 0` (Farkas).

use crate::obligation::{Formula, Op, Term};
use std::collections::BTreeMap;

/// Why an evaluation step could not complete soundly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvalErr {
    /// A residual nonlinearity (`Min/Max/Abs/Ite`) reached the linear layer:
    /// the certificate did not split it away. Fail closed.
    NotLinear { what: String },
    /// Checked integer arithmetic overflowed. NEVER wrap into a false result.
    Overflow { what: String },
}

impl std::fmt::Display for EvalErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EvalErr::NotLinear { what } => {
                write!(f, "nonlinear term not resolved by a case split: {what}")
            }
            EvalErr::Overflow { what } => write!(f, "coefficient overflow in {what}"),
        }
    }
}

/// A normalized linear combination `Σ cᵢ·xᵢ + const`. Coefficients are kept in a
/// `BTreeMap` keyed by variable name so the representation is canonical
/// (deterministic iteration order) and equality is structural.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LinComb {
    pub coeffs: BTreeMap<String, i64>,
    pub constant: i64,
}

impl LinComb {
    pub fn constant(c: i64) -> LinComb {
        LinComb {
            coeffs: BTreeMap::new(),
            constant: c,
        }
    }

    pub fn var(name: &str) -> LinComb {
        let mut coeffs = BTreeMap::new();
        coeffs.insert(name.to_string(), 1);
        LinComb {
            coeffs,
            constant: 0,
        }
    }

    /// `self + other`, checked.
    pub fn add(&self, other: &LinComb) -> Result<LinComb, EvalErr> {
        let mut out = self.clone();
        for (k, v) in &other.coeffs {
            let e = out.coeffs.entry(k.clone()).or_insert(0);
            *e = e.checked_add(*v).ok_or_else(|| EvalErr::Overflow {
                what: format!("adding coefficient of {k}"),
            })?;
        }
        out.constant =
            out.constant
                .checked_add(other.constant)
                .ok_or_else(|| EvalErr::Overflow {
                    what: "adding constants".into(),
                })?;
        out.prune();
        Ok(out)
    }

    /// `self - other`, checked.
    pub fn sub(&self, other: &LinComb) -> Result<LinComb, EvalErr> {
        self.add(&other.scale(-1)?)
    }

    /// `k · self`, checked.
    pub fn scale(&self, k: i64) -> Result<LinComb, EvalErr> {
        let mut out = LinComb::default();
        for (name, c) in &self.coeffs {
            let nc = c.checked_mul(k).ok_or_else(|| EvalErr::Overflow {
                what: format!("scaling coefficient of {name}"),
            })?;
            out.coeffs.insert(name.clone(), nc);
        }
        out.constant = self
            .constant
            .checked_mul(k)
            .ok_or_else(|| EvalErr::Overflow {
                what: "scaling constant".into(),
            })?;
        out.prune();
        Ok(out)
    }

    /// Drop zero coefficients so equality + the contradiction check are exact.
    fn prune(&mut self) {
        self.coeffs.retain(|_, c| *c != 0);
    }
}

/// Normalize a (branch-resolved) `Term` into a `LinComb`. Any residual
/// `Min/Max/Abs/Ite` is a soundness fail (the certificate was supposed to split
/// it away) → `EvalErr::NotLinear`. All arithmetic is checked.
pub fn term_to_lincomb(t: &Term) -> Result<LinComb, EvalErr> {
    match t {
        Term::Int { v } => Ok(LinComb::constant(*v)),
        Term::IVar { name } => Ok(LinComb::var(name)),
        Term::Add { l, r } => {
            let a = term_to_lincomb(l)?;
            let b = term_to_lincomb(r)?;
            a.add(&b)
        }
        Term::Sub { l, r } => {
            let a = term_to_lincomb(l)?;
            let b = term_to_lincomb(r)?;
            a.sub(&b)
        }
        Term::MulConst { k, t } => {
            let a = term_to_lincomb(t)?;
            a.scale(*k)
        }
        Term::Min { .. } => Err(EvalErr::NotLinear {
            what: "min(...)".into(),
        }),
        Term::Max { .. } => Err(EvalErr::NotLinear {
            what: "max(...)".into(),
        }),
        Term::Abs { .. } => Err(EvalErr::NotLinear {
            what: "abs(...)".into(),
        }),
        Term::Ite { .. } => Err(EvalErr::NotLinear {
            what: "ite(...)".into(),
        }),
    }
}

/// A normalized linear inequality `Σ cᵢ·xᵢ  op  k`, where `op ∈ {Le, Lt}` and
/// the left side has NO constant (it was moved into `k`). This is the canonical
/// hypothesis form the Farkas combination operates on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinIneq {
    pub lhs: BTreeMap<String, i64>, // Σ cᵢ·xᵢ   (constant moved to rhs)
    pub strict: bool,               // true ⇒ `< k`, false ⇒ `≤ k`
    pub rhs: i64,                   // k
}

impl LinIneq {
    /// Build the canonical `Σ cᵢxᵢ op k` from `lc op 0`-style data: given a
    /// `LinComb` representing the lhs minus rhs of an `OP` comparison, and the
    /// comparison's direction, produce a `≤`/`<` form. The constant of the
    /// `LinComb` is moved to the right-hand side. Checked.
    fn from_lincomb_le(lc: &LinComb, strict: bool) -> Result<LinIneq, EvalErr> {
        // lc ≤ 0  (or  < 0)   ⇔   Σ cᵢxᵢ  ≤  -constant
        let rhs = 0i64
            .checked_sub(lc.constant)
            .ok_or_else(|| EvalErr::Overflow {
                what: "moving constant".into(),
            })?;
        Ok(LinIneq {
            lhs: lc.coeffs.clone(),
            strict,
            rhs,
        })
    }
}

/// Lower a comparison `Cmp(op, l, r)` into ONE canonical `≤`/`<` `LinIneq`
/// (always of the form `Σ cᵢxᵢ op k`). `Eq`/`Ne` are NOT single inequalities —
/// callers split those into the pair `{≤, ≥}` (handled in the checker). This is
/// the leaf-fact normalization the checker re-derives from the negated claim.
pub fn cmp_to_ineq(op: Op, l: &Term, r: &Term) -> Result<Vec<LinIneq>, EvalErr> {
    let lc = term_to_lincomb(l)?;
    let rc = term_to_lincomb(r)?;
    let diff = lc.sub(&rc)?; // l - r
    let ndiff = rc.sub(&lc)?; // r - l
    match op {
        // l < r   ⇔   l - r < 0
        Op::Lt => Ok(vec![LinIneq::from_lincomb_le(&diff, true)?]),
        // l ≤ r   ⇔   l - r ≤ 0
        Op::Le => Ok(vec![LinIneq::from_lincomb_le(&diff, false)?]),
        // l > r   ⇔   r - l < 0
        Op::Gt => Ok(vec![LinIneq::from_lincomb_le(&ndiff, true)?]),
        // l ≥ r   ⇔   r - l ≤ 0
        Op::Ge => Ok(vec![LinIneq::from_lincomb_le(&ndiff, false)?]),
        // l = r   ⇔   (l - r ≤ 0) ∧ (r - l ≤ 0)
        Op::Eq => Ok(vec![
            LinIneq::from_lincomb_le(&diff, false)?,
            LinIneq::from_lincomb_le(&ndiff, false)?,
        ]),
        // l ≠ r is a DISJUNCTION (l < r) ∨ (r < l) — not a conjunction of facts;
        // it cannot be a single hypothesis. The checker handles `Ne` by case
        // split, never as a leaf fact, so reaching here is a fragment violation.
        Op::Ne => Err(EvalErr::NotLinear {
            what: "≠ as a leaf hypothesis (must be case-split)".into(),
        }),
    }
}

/// The Farkas combination (R23 §4.1 step 3): given a set of `≤`/`<` facts and
/// nonnegative multipliers, compute `Σ coeffᵢ · factᵢ` as a single `LinIneq` and
/// confirm it is a LITERAL contradiction (`0 ≤ -1` / `0 < 0`).
///
/// Soundness rules (all CHECKED arithmetic):
///   * every multiplier must be `≥ 0` (a negative multiplier flips the
///     inequality and is unsound) — else `Err`;
///   * the weighted sum's variable coefficients must ALL cancel to 0;
///   * the result must contradict: either `0 ≤ k` with `k < 0`, or `0 < k`
///     with `k ≤ 0` (strict: at least one combined strict fact with positive
///     weight makes the sum strict; `0 < 0` is a contradiction).
///
/// Returns `Ok(())` iff the combination is a genuine contradiction.
pub fn farkas_combine(facts: &[LinIneq], coeffs: &[i64]) -> Result<(), FarkasErr> {
    if facts.len() != coeffs.len() {
        return Err(FarkasErr::Shape {
            detail: format!("{} facts but {} coefficients", facts.len(), coeffs.len()),
        });
    }
    if facts.is_empty() {
        return Err(FarkasErr::Shape {
            detail: "no facts to combine".into(),
        });
    }
    let mut acc: BTreeMap<String, i64> = BTreeMap::new();
    let mut rhs: i64 = 0;
    let mut any_strict = false;
    for (fact, &c) in facts.iter().zip(coeffs) {
        if c < 0 {
            return Err(FarkasErr::NegativeCoeff { coeff: c });
        }
        if c == 0 {
            continue; // a zero multiplier contributes nothing (harmless)
        }
        // Accumulate c · fact.lhs.
        for (name, fc) in &fact.lhs {
            let prod = fc.checked_mul(c).ok_or(FarkasErr::Overflow)?;
            let e = acc.entry(name.clone()).or_insert(0);
            *e = e.checked_add(prod).ok_or(FarkasErr::Overflow)?;
        }
        // Accumulate c · fact.rhs.
        let prod = fact.rhs.checked_mul(c).ok_or(FarkasErr::Overflow)?;
        rhs = rhs.checked_add(prod).ok_or(FarkasErr::Overflow)?;
        if fact.strict {
            any_strict = true;
        }
    }
    // Every variable must cancel: Σ coeff·fact has lhs ≡ 0.
    acc.retain(|_, v| *v != 0);
    if !acc.is_empty() {
        return Err(FarkasErr::NotCancelled {
            residual: acc.into_iter().collect(),
        });
    }
    // The combined inequality is `0 (op) rhs`, where op is `<` iff any combined
    // fact (with positive weight) was strict. A contradiction is:
    //   strict:     0 < rhs   contradicts iff rhs ≤ 0
    //   non-strict: 0 ≤ rhs   contradicts iff rhs < 0
    let contradiction = if any_strict { rhs <= 0 } else { rhs < 0 };
    if contradiction {
        Ok(())
    } else {
        Err(FarkasErr::NotContradiction {
            strict: any_strict,
            rhs,
        })
    }
}

/// Why a Farkas combination is not a valid refutation witness.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FarkasErr {
    Shape { detail: String },
    NegativeCoeff { coeff: i64 },
    Overflow,
    NotCancelled { residual: Vec<(String, i64)> },
    NotContradiction { strict: bool, rhs: i64 },
}

impl std::fmt::Display for FarkasErr {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FarkasErr::Shape { detail } => write!(f, "malformed Farkas witness: {detail}"),
            FarkasErr::NegativeCoeff { coeff } => {
                write!(f, "negative Farkas coefficient {coeff} (must be ≥ 0)")
            }
            FarkasErr::Overflow => write!(f, "coefficient overflow"),
            FarkasErr::NotCancelled { residual } => {
                write!(
                    f,
                    "weighted sum does not cancel variables: residual {residual:?}"
                )
            }
            FarkasErr::NotContradiction { strict, rhs } => {
                let op = if *strict { "<" } else { "≤" };
                write!(f, "weighted sum is `0 {op} {rhs}`, not a contradiction")
            }
        }
    }
}

/// Negation-normal-form push-down of `Not` over a formula (R23 §4.1 step 1).
/// Pure rewriting, NO search. `Implies` is desugared (`a→b ≡ ¬a ∨ b`), `Not`
/// pushed to the leaves, and comparisons negated by flipping the operator. The
/// result has `Not` only (never) on leaves — every comparison is positive.
pub fn nnf(f: &Formula) -> Formula {
    nnf_inner(f, false)
}

fn nnf_inner(f: &Formula, neg: bool) -> Formula {
    match f {
        Formula::Bool { v } => Formula::Bool { v: v ^ neg },
        Formula::BVar { name } => {
            if neg {
                Formula::Not {
                    f: Box::new(Formula::BVar { name: name.clone() }),
                }
            } else {
                Formula::BVar { name: name.clone() }
            }
        }
        Formula::Not { f } => nnf_inner(f, !neg),
        Formula::And { fs } => {
            let mapped: Vec<Formula> = fs.iter().map(|g| nnf_inner(g, neg)).collect();
            if neg {
                Formula::Or { fs: mapped }
            } else {
                Formula::And { fs: mapped }
            }
        }
        Formula::Or { fs } => {
            let mapped: Vec<Formula> = fs.iter().map(|g| nnf_inner(g, neg)).collect();
            if neg {
                Formula::And { fs: mapped }
            } else {
                Formula::Or { fs: mapped }
            }
        }
        Formula::Implies { a, b } => {
            // a → b ≡ ¬a ∨ b. Under an outer negation that becomes a ∧ ¬b.
            let na = nnf_inner(a, !neg);
            let nb = nnf_inner(b, neg);
            if neg {
                Formula::And {
                    fs: vec![nnf_inner(a, false), nnf_inner(b, true)],
                }
            } else {
                let _ = (na, nb);
                Formula::Or {
                    fs: vec![nnf_inner(a, true), nnf_inner(b, false)],
                }
            }
        }
        Formula::Cmp { op, l, r } => {
            let op = if neg { negate_op(*op) } else { *op };
            Formula::Cmp {
                op,
                l: l.clone(),
                r: r.clone(),
            }
        }
    }
}

/// The boolean negation of a comparison operator.
pub fn negate_op(op: Op) -> Op {
    match op {
        Op::Lt => Op::Ge,
        Op::Le => Op::Gt,
        Op::Gt => Op::Le,
        Op::Ge => Op::Lt,
        Op::Eq => Op::Ne,
        Op::Ne => Op::Eq,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::obligation::build::*;

    #[test]
    fn lincomb_arith_is_checked() {
        let big = LinComb::constant(i64::MAX);
        assert!(matches!(
            big.add(&LinComb::constant(1)),
            Err(EvalErr::Overflow { .. })
        ));
        let v = LinComb::var("x");
        assert!(v.scale(i64::MAX).is_ok());
        let big_coeff = LinComb {
            coeffs: [("x".to_string(), i64::MAX)].into_iter().collect(),
            constant: 0,
        };
        assert!(matches!(big_coeff.scale(2), Err(EvalErr::Overflow { .. })));
    }

    #[test]
    fn nonlinear_term_is_rejected() {
        let t = min(ivar("g"), ivar("a"));
        assert!(matches!(
            term_to_lincomb(&t),
            Err(EvalErr::NotLinear { .. })
        ));
    }

    #[test]
    fn cmp_lowers_to_le_form() {
        // g ≤ avail   ⇔   g - avail ≤ 0   ⇔   {g:1, avail:-1} ≤ 0
        let ineqs = cmp_to_ineq(Op::Le, &ivar("g"), &ivar("avail")).unwrap();
        assert_eq!(ineqs.len(), 1);
        let i = &ineqs[0];
        assert!(!i.strict);
        assert_eq!(i.rhs, 0);
        assert_eq!(i.lhs.get("g"), Some(&1));
        assert_eq!(i.lhs.get("avail"), Some(&-1));
    }

    #[test]
    fn farkas_detects_real_contradiction() {
        // x ≤ -1  and  -x ≤ -1   ⇒  (1)·(x≤-1)+(1)·(-x≤-1) = 0 ≤ -2  (contradiction)
        let f1 = LinIneq {
            lhs: [("x".to_string(), 1)].into_iter().collect(),
            strict: false,
            rhs: -1,
        };
        let f2 = LinIneq {
            lhs: [("x".to_string(), -1)].into_iter().collect(),
            strict: false,
            rhs: -1,
        };
        assert!(farkas_combine(&[f1, f2], &[1, 1]).is_ok());
    }

    #[test]
    fn farkas_rejects_negative_coeff() {
        let f1 = LinIneq {
            lhs: [("x".to_string(), 1)].into_iter().collect(),
            strict: false,
            rhs: -1,
        };
        let f2 = LinIneq {
            lhs: [("x".to_string(), -1)].into_iter().collect(),
            strict: false,
            rhs: -1,
        };
        assert!(matches!(
            farkas_combine(&[f1, f2], &[1, -1]),
            Err(FarkasErr::NegativeCoeff { .. })
        ));
    }

    #[test]
    fn farkas_rejects_non_contradiction() {
        // x ≤ 5  alone, coeff 1 ⇒ 1·x ≤ 5, variable doesn't cancel.
        let f1 = LinIneq {
            lhs: [("x".to_string(), 1)].into_iter().collect(),
            strict: false,
            rhs: 5,
        };
        assert!(matches!(
            farkas_combine(&[f1], &[1]),
            Err(FarkasErr::NotCancelled { .. })
        ));
    }

    #[test]
    fn farkas_sum_to_zero_is_not_a_contradiction() {
        // 0 ≤ 0 is NOT a contradiction (only 0 < 0 or 0 ≤ -1 are).
        let f1 = LinIneq {
            lhs: [("x".to_string(), 1)].into_iter().collect(),
            strict: false,
            rhs: 0,
        };
        let f2 = LinIneq {
            lhs: [("x".to_string(), -1)].into_iter().collect(),
            strict: false,
            rhs: 0,
        };
        assert!(matches!(
            farkas_combine(&[f1, f2], &[1, 1]),
            Err(FarkasErr::NotContradiction { .. })
        ));
    }

    #[test]
    fn farkas_strict_zero_is_a_contradiction() {
        // x < 0 and -x ≤ 0  ⇒  0 < 0 (strict) ⇒ contradiction.
        let f1 = LinIneq {
            lhs: [("x".to_string(), 1)].into_iter().collect(),
            strict: true,
            rhs: 0,
        };
        let f2 = LinIneq {
            lhs: [("x".to_string(), -1)].into_iter().collect(),
            strict: false,
            rhs: 0,
        };
        assert!(farkas_combine(&[f1, f2], &[1, 1]).is_ok());
    }

    #[test]
    fn farkas_overflow_is_caught() {
        let f1 = LinIneq {
            lhs: [("x".to_string(), i64::MAX)].into_iter().collect(),
            strict: false,
            rhs: -1,
        };
        assert!(matches!(
            farkas_combine(&[f1], &[2]),
            Err(FarkasErr::Overflow)
        ));
    }

    #[test]
    fn nnf_negates_comparisons() {
        // ¬(g ≤ avail)  ⇒  g > avail
        let f = cmp(Op::Le, ivar("g"), ivar("avail"));
        let n = nnf(&Formula::Not { f: Box::new(f) });
        assert_eq!(n, cmp(Op::Gt, ivar("g"), ivar("avail")));
    }

    #[test]
    fn nnf_pushes_through_and_or() {
        // ¬(a ∧ b)  ⇒  ¬a ∨ ¬b
        let f = and(vec![bvar("a"), bvar("b")]);
        let n = nnf(&Formula::Not { f: Box::new(f) });
        match n {
            Formula::Or { fs } => assert_eq!(fs.len(), 2),
            _ => panic!("expected Or"),
        }
    }
}
