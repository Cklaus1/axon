//! R23 §3.2 / §4.5 — the deterministic refutation SYNTHESIZER. Pure (no solver,
//! no clock, no random). Builds a case-split refutation tree for an obligation
//! in the certified fragment, byte-deterministically (A5). UNTRUSTED: the checker
//! re-derives everything, so this only needs to produce certs the checker
//! accepts; if it cannot, it returns an error and `prove` reports `Unsupported`.
//!
//! Strategy (mirrors the checker's own reduction, in reverse):
//!   1. neg = NNF(Not(claim)), with Min/Max/Abs expanded to Ite.
//!   2. If the formula reduces (under accumulated assumptions) to a single
//!      conjunction of linear facts, find nonnegative Farkas multipliers by a
//!      tiny bounded search and emit a `LinearContradiction`.
//!   3. Otherwise pick the first unresolved boolean var (→ `BoolCase`) or Ite
//!      condition (→ `IteCase`), in a FIXED deterministic order, and recurse on
//!      both branches.

use crate::certificate::{FactOp, LinFact, ProofCertificate, Refutation};
use crate::eval::{self, LinIneq};
use crate::obligation::{Formula, Obligation, Op, Sort, Term};
use std::collections::BTreeMap;

/// Build a full certificate for `obligation` (digests included). Pure.
pub fn synthesize(obligation: &Obligation) -> Result<ProofCertificate, String> {
    let neg = eval::nnf(&Formula::Not {
        f: Box::new(expand_formula(&obligation.claim)),
    });
    let refutation = build(&neg, obligation, 0)?;
    Ok(ProofCertificate::new(
        &obligation.id,
        &obligation.digest(),
        refutation,
    ))
}

const MAX_DEPTH: usize = 64;
const MAX_COEFF: i64 = 8;

fn build(f: &Formula, obl: &Obligation, depth: usize) -> Result<Refutation, String> {
    if depth > MAX_DEPTH {
        return Err("synthesis depth exceeded (cert would be too deep)".into());
    }
    // Try to close this branch as a single conjunctive leaf.
    if let Some(hyps) = conjunctive_hypotheses(f) {
        if let Some((facts, coeffs)) = find_farkas(&hyps) {
            return Ok(Refutation::LinearContradiction { facts, coeffs });
        }
        // A conjunction with no contradiction means the negated claim is SAT on
        // this branch — the obligation is not valid here.
        return Err("branch has no linear contradiction (obligation may be false)".into());
    }

    // Not a pure conjunction: split. Prefer a boolean var, else an Ite cond.
    if let Some(var) = first_unresolved_bvar(f) {
        if obl.sort_of(&var) != Some(Sort::Bool) {
            return Err(format!("split var {var} is not declared boolean"));
        }
        let ft = subst_bvar(f, &var, true);
        let ff = subst_bvar(f, &var, false);
        return Ok(Refutation::BoolCase {
            var,
            on_true: Box::new(build(&ft, obl, depth + 1)?),
            on_false: Box::new(build(&ff, obl, depth + 1)?),
        });
    }
    if let Some(cond) = first_ite_cond(f) {
        let ft = conj(resolve_ite(f, &cond, true), cond.clone());
        let ff = conj(
            resolve_ite(f, &cond, false),
            Formula::Not {
                f: Box::new(cond.clone()),
            },
        );
        // The cond stored in the cert is the (already-expanded) Ite condition.
        return Ok(Refutation::IteCase {
            cond,
            on_true: Box::new(build(&ft, obl, depth + 1)?),
            on_false: Box::new(build(&ff, obl, depth + 1)?),
        });
    }
    Err("formula is neither a conjunction nor splittable (outside the fragment)".into())
}

/// If `f` is a conjunction of linear comparison literals (no bool vars, no Ites,
/// no Or), return the hypotheses as `LinIneq`s; else `None`.
fn conjunctive_hypotheses(f: &Formula) -> Option<Vec<LinIneq>> {
    let mut out = Vec::new();
    if gather(f, &mut out) {
        Some(out)
    } else {
        None
    }
}

fn gather(f: &Formula, out: &mut Vec<LinIneq>) -> bool {
    match f {
        Formula::Bool { v: true } => true,
        Formula::Bool { v: false } => {
            out.push(LinIneq {
                lhs: BTreeMap::new(),
                strict: false,
                rhs: -1,
            });
            true
        }
        Formula::And { fs } => fs.iter().all(|g| gather(g, out)),
        Formula::Cmp { op, l, r } => {
            if matches!(op, Op::Ne) {
                return false; // a disjunction; must be split, not a leaf
            }
            match eval::cmp_to_ineq(*op, l, r) {
                Ok(mut v) => {
                    out.append(&mut v);
                    true
                }
                Err(_) => false, // residual nonlinearity or overflow
            }
        }
        _ => false, // BVar / Not / Or / Implies / Ite-bearing → not a pure leaf
    }
}

/// A tiny, deterministic bounded search for nonnegative Farkas multipliers that
/// make the hypotheses contradict. Tries the empty/duplicate-free subsets with
/// coefficients in `1..=MAX_COEFF`. Deterministic order ⇒ byte-stable certs.
fn find_farkas(hyps: &[LinIneq]) -> Option<(Vec<LinFact>, Vec<i64>)> {
    let n = hyps.len();
    if n == 0 {
        return None;
    }
    // First: a single fact that is itself `0 (≤|<) k` contradiction.
    for (i, h) in hyps.iter().enumerate() {
        if h.lhs.is_empty() && eval::farkas_combine(std::slice::from_ref(h), &[1]).is_ok() {
            return Some((vec![ineq_to_fact(h)], vec![1]));
        }
        let _ = i;
    }
    // Pairs (the common shape: complementary `x ≤ k` and `-x ≤ -k-1`).
    for i in 0..n {
        for j in 0..n {
            if i == j {
                continue;
            }
            for ci in 1..=MAX_COEFF {
                for cj in 1..=MAX_COEFF {
                    let pair = [hyps[i].clone(), hyps[j].clone()];
                    if eval::farkas_combine(&pair, &[ci, cj]).is_ok() {
                        // Emit in ascending index order for determinism.
                        let (a, b, ca, cb) = if i < j {
                            (i, j, ci, cj)
                        } else {
                            (j, i, cj, ci)
                        };
                        return Some((
                            vec![ineq_to_fact(&hyps[a]), ineq_to_fact(&hyps[b])],
                            vec![ca, cb],
                        ));
                    }
                }
            }
        }
    }
    // Whole-set unit combination (covers >2-fact contradictions).
    let all: Vec<LinIneq> = hyps.to_vec();
    let ones = vec![1i64; n];
    if eval::farkas_combine(&all, &ones).is_ok() {
        return Some((all.iter().map(ineq_to_fact).collect(), ones));
    }
    None
}

fn ineq_to_fact(i: &LinIneq) -> LinFact {
    LinFact {
        terms: i.lhs.iter().map(|(k, v)| (*v, k.clone())).collect(),
        op: if i.strict { FactOp::Lt } else { FactOp::Le },
        constant: i.rhs,
    }
}

// ── shared rewriting (kept local to the synthesizer; mirrors checker.rs) ──────

fn first_unresolved_bvar(f: &Formula) -> Option<String> {
    match f {
        Formula::BVar { name } => Some(name.clone()),
        Formula::Not { f } => first_unresolved_bvar(f),
        Formula::And { fs } | Formula::Or { fs } => fs.iter().find_map(first_unresolved_bvar),
        Formula::Implies { a, b } => first_unresolved_bvar(a).or_else(|| first_unresolved_bvar(b)),
        _ => None,
    }
}

fn first_ite_cond(f: &Formula) -> Option<Formula> {
    match f {
        Formula::Bool { .. } | Formula::BVar { .. } => None,
        Formula::Not { f } => first_ite_cond(f),
        Formula::And { fs } | Formula::Or { fs } => fs.iter().find_map(first_ite_cond),
        Formula::Implies { a, b } => first_ite_cond(a).or_else(|| first_ite_cond(b)),
        Formula::Cmp { l, r, .. } => term_first_ite_cond(l).or_else(|| term_first_ite_cond(r)),
    }
}

fn term_first_ite_cond(t: &Term) -> Option<Formula> {
    match t {
        Term::Int { .. } | Term::IVar { .. } => None,
        Term::Add { l, r } | Term::Sub { l, r } | Term::Min { l, r } | Term::Max { l, r } => {
            term_first_ite_cond(l).or_else(|| term_first_ite_cond(r))
        }
        Term::MulConst { t, .. } | Term::Abs { t } => term_first_ite_cond(t),
        Term::Ite { cond, then, els } => Some((**cond).clone())
            .or_else(|| term_first_ite_cond(then))
            .or_else(|| term_first_ite_cond(els)),
    }
}

// The following rewriting helpers are intentionally identical in behavior to the
// checker's, so a synthesized cert reduces the same way the checker re-derives.

fn expand_term(t: &Term) -> Term {
    match t {
        Term::Int { .. } | Term::IVar { .. } => t.clone(),
        Term::Add { l, r } => Term::Add {
            l: Box::new(expand_term(l)),
            r: Box::new(expand_term(r)),
        },
        Term::Sub { l, r } => Term::Sub {
            l: Box::new(expand_term(l)),
            r: Box::new(expand_term(r)),
        },
        Term::MulConst { k, t } => Term::MulConst {
            k: *k,
            t: Box::new(expand_term(t)),
        },
        Term::Min { l, r } => {
            let el = expand_term(l);
            let er = expand_term(r);
            Term::Ite {
                cond: Box::new(Formula::Cmp {
                    op: Op::Le,
                    l: el.clone(),
                    r: er.clone(),
                }),
                then: Box::new(el),
                els: Box::new(er),
            }
        }
        Term::Max { l, r } => {
            let el = expand_term(l);
            let er = expand_term(r);
            Term::Ite {
                cond: Box::new(Formula::Cmp {
                    op: Op::Ge,
                    l: el.clone(),
                    r: er.clone(),
                }),
                then: Box::new(el),
                els: Box::new(er),
            }
        }
        Term::Abs { t } => {
            let et = expand_term(t);
            Term::Ite {
                cond: Box::new(Formula::Cmp {
                    op: Op::Ge,
                    l: et.clone(),
                    r: Term::Int { v: 0 },
                }),
                then: Box::new(et.clone()),
                els: Box::new(Term::MulConst {
                    k: -1,
                    t: Box::new(et),
                }),
            }
        }
        Term::Ite { cond, then, els } => Term::Ite {
            cond: Box::new(expand_formula(cond)),
            then: Box::new(expand_term(then)),
            els: Box::new(expand_term(els)),
        },
    }
}

fn expand_formula(f: &Formula) -> Formula {
    match f {
        Formula::Bool { .. } | Formula::BVar { .. } => f.clone(),
        Formula::Not { f } => Formula::Not {
            f: Box::new(expand_formula(f)),
        },
        Formula::And { fs } => Formula::And {
            fs: fs.iter().map(expand_formula).collect(),
        },
        Formula::Or { fs } => Formula::Or {
            fs: fs.iter().map(expand_formula).collect(),
        },
        Formula::Implies { a, b } => Formula::Implies {
            a: Box::new(expand_formula(a)),
            b: Box::new(expand_formula(b)),
        },
        Formula::Cmp { op, l, r } => Formula::Cmp {
            op: *op,
            l: expand_term(l),
            r: expand_term(r),
        },
    }
}

fn subst_bvar(f: &Formula, var: &str, val: bool) -> Formula {
    match f {
        Formula::Bool { .. } => f.clone(),
        Formula::BVar { name } if name == var => Formula::Bool { v: val },
        Formula::BVar { .. } => f.clone(),
        Formula::Not { f } => match subst_bvar(f, var, val) {
            Formula::Bool { v } => Formula::Bool { v: !v },
            other => Formula::Not { f: Box::new(other) },
        },
        Formula::And { fs } => simplify_and(fs.iter().map(|g| subst_bvar(g, var, val)).collect()),
        Formula::Or { fs } => simplify_or(fs.iter().map(|g| subst_bvar(g, var, val)).collect()),
        Formula::Implies { a, b } => Formula::Implies {
            a: Box::new(subst_bvar(a, var, val)),
            b: Box::new(subst_bvar(b, var, val)),
        },
        Formula::Cmp { .. } => f.clone(),
    }
}

fn simplify_and(fs: Vec<Formula>) -> Formula {
    let mut out = Vec::new();
    for g in fs {
        match g {
            Formula::Bool { v: true } => {}
            Formula::Bool { v: false } => return Formula::Bool { v: false },
            other => out.push(other),
        }
    }
    match out.len() {
        0 => Formula::Bool { v: true },
        1 => out.pop().unwrap(),
        _ => Formula::And { fs: out },
    }
}

fn simplify_or(fs: Vec<Formula>) -> Formula {
    let mut out = Vec::new();
    for g in fs {
        match g {
            Formula::Bool { v: false } => {}
            Formula::Bool { v: true } => return Formula::Bool { v: true },
            other => out.push(other),
        }
    }
    match out.len() {
        0 => Formula::Bool { v: false },
        1 => out.pop().unwrap(),
        _ => Formula::Or { fs: out },
    }
}

fn conj(f: Formula, extra: Formula) -> Formula {
    eval::nnf(&Formula::And { fs: vec![f, extra] })
}

fn resolve_ite(f: &Formula, cond: &Formula, branch: bool) -> Formula {
    match f {
        Formula::Bool { .. } | Formula::BVar { .. } => f.clone(),
        Formula::Not { f } => Formula::Not {
            f: Box::new(resolve_ite(f, cond, branch)),
        },
        Formula::And { fs } => Formula::And {
            fs: fs.iter().map(|g| resolve_ite(g, cond, branch)).collect(),
        },
        Formula::Or { fs } => Formula::Or {
            fs: fs.iter().map(|g| resolve_ite(g, cond, branch)).collect(),
        },
        Formula::Implies { a, b } => Formula::Implies {
            a: Box::new(resolve_ite(a, cond, branch)),
            b: Box::new(resolve_ite(b, cond, branch)),
        },
        Formula::Cmp { op, l, r } => Formula::Cmp {
            op: *op,
            l: resolve_ite_term(l, cond, branch),
            r: resolve_ite_term(r, cond, branch),
        },
    }
}

fn resolve_ite_term(t: &Term, cond: &Formula, branch: bool) -> Term {
    match t {
        Term::Int { .. } | Term::IVar { .. } => t.clone(),
        Term::Add { l, r } => Term::Add {
            l: Box::new(resolve_ite_term(l, cond, branch)),
            r: Box::new(resolve_ite_term(r, cond, branch)),
        },
        Term::Sub { l, r } => Term::Sub {
            l: Box::new(resolve_ite_term(l, cond, branch)),
            r: Box::new(resolve_ite_term(r, cond, branch)),
        },
        Term::Min { l, r } => Term::Min {
            l: Box::new(resolve_ite_term(l, cond, branch)),
            r: Box::new(resolve_ite_term(r, cond, branch)),
        },
        Term::Max { l, r } => Term::Max {
            l: Box::new(resolve_ite_term(l, cond, branch)),
            r: Box::new(resolve_ite_term(r, cond, branch)),
        },
        Term::MulConst { k, t } => Term::MulConst {
            k: *k,
            t: Box::new(resolve_ite_term(t, cond, branch)),
        },
        Term::Abs { t } => Term::Abs {
            t: Box::new(resolve_ite_term(t, cond, branch)),
        },
        Term::Ite { cond: c, then, els } => {
            if c.as_ref() == cond {
                let chosen = if branch { then } else { els };
                resolve_ite_term(chosen, cond, branch)
            } else {
                Term::Ite {
                    cond: c.clone(),
                    then: Box::new(resolve_ite_term(then, cond, branch)),
                    els: Box::new(resolve_ite_term(els, cond, branch)),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
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
    fn synthesize_carve_is_checker_valid() {
        let o = carve();
        let c = synthesize(&o).expect("carve synthesizes");
        assert_eq!(check(&o, &c), CheckResult::Valid);
    }

    #[test]
    fn synthesis_is_byte_deterministic() {
        let o = carve();
        let a = synthesize(&o).unwrap().to_json();
        let b = synthesize(&o).unwrap().to_json();
        assert_eq!(a, b);
    }

    #[test]
    fn synthesize_mint_o2_is_valid() {
        // (rem ≥ 0) ⇒ max(0, min(g, rem)) ≤ rem
        let o = Obligation {
            id: "principal_mint/O2".into(),
            vars: vec![
                Var {
                    name: "g".into(),
                    sort: Sort::Int,
                },
                Var {
                    name: "rem".into(),
                    sort: Sort::Int,
                },
            ],
            claim: implies(
                cmp(Op::Ge, ivar("rem"), int(0)),
                cmp(
                    Op::Le,
                    max(int(0), min(ivar("g"), ivar("rem"))),
                    ivar("rem"),
                ),
            ),
        };
        let c = synthesize(&o).expect("mint_o2 synthesizes");
        assert_eq!(check(&o, &c), CheckResult::Valid);
    }

    #[test]
    fn false_obligation_cannot_synthesize() {
        let mut o = carve();
        o.claim = cmp(Op::Ge, min(ivar("g"), ivar("avail")), ivar("avail"));
        assert!(synthesize(&o).is_err());
    }
}
