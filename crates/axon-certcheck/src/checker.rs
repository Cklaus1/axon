//! R23 §4.1–§4.3 — THE TRUSTED CHECKER. Pure, total, solver-free (std + sha2 +
//! serde only). Its whole value is that an auditor can read all of it.
//!
//! Given `(obligation, certificate)` it independently returns
//! `Valid | Invalid | Unsupported` by re-deriving the contradiction itself —
//! it NEVER trusts a claim stated in the certificate it didn't confirm:
//!   1. **Binding** (§4.1.4): `cert.obligation_id == obligation.id`,
//!      `cert.obligation_digest == sha256(obligation)`, and `cert.cert_digest`
//!      re-hashes. A swapped/forged cert fails here.
//!   2. **Bound** (§4.4 / I-4): the cert is within `MAX_CERT_NODES`.
//!   3. **Negate + NNF** (§4.1.1): `neg = NNF(Not(claim))`, with `Min/Max/Abs`
//!      expanded to `Ite` so the only nonlinear construct is `Ite`.
//!   4. **Walk the case tree** (§4.1.2): `BoolCase`/`IteCase` must be exhaustive
//!      (both branches refute); the assumption is substituted in and the walk
//!      continues. An incomplete split, or a split on something not in the
//!      formula, → `Invalid`.
//!   5. **Leaf contradiction** (§4.1.3): the checker re-derives the leaf's
//!      hypothesis set from the branch-resolved negated claim (a conjunction of
//!      linear literals), confirms every cert `LinFact` is among them, confirms
//!      every coefficient is ≥ 0, and recomputes `Σ coeff·fact` to a literal
//!      contradiction. Only then ⇒ `Valid`.
//!
//! Soundness over completeness (I-1): it returns `Valid` ONLY after itself
//! deriving the contradiction; otherwise `Unsupported`/`Invalid`, never a false
//! `Valid`.

use crate::certificate::{FactOp, LinFact, ProofCertificate, Refutation};
use crate::eval::{self, EvalErr, LinIneq};
use crate::obligation::{Formula, Obligation, Op, Sort, Term};
use std::collections::BTreeMap;

/// The checker's verdict (§3.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CheckResult {
    /// The certificate constructively proves the obligation for ALL inputs.
    Valid,
    /// The certificate does NOT prove the claim (forged, mutated, wrong
    /// obligation, non-exhaustive split, bad Farkas witness) — fail closed.
    Invalid { reason: String },
    /// The obligation is outside the R23 fragment (nonlinear, etc.) — fail
    /// closed for admission purposes (treated like Invalid by the gate).
    Unsupported { reason: String },
}

impl CheckResult {
    pub fn is_valid(&self) -> bool {
        matches!(self, CheckResult::Valid)
    }
}

/// The bound on a certificate's size (§4.4 / I-4): a maliciously huge cert is
/// rejected, never given unbounded work.
pub const MAX_CERT_NODES: usize = 100_000;

/// The bound on a certificate's DEPTH. The checker walks the refutation tree
/// recursively; bounding the depth keeps that recursion within the stack (no
/// overflow on a degenerate deep cert). Comfortably above any real proof.
pub const MAX_CERT_DEPTH: usize = 1_000;

/// Iterative depth check (an explicit work-stack, so it cannot itself overflow).
/// Returns true iff the tree is deeper than `limit`.
fn refutation_depth_exceeds(root: &Refutation, limit: usize) -> bool {
    // (node, depth) work items.
    let mut stack: Vec<(&Refutation, usize)> = vec![(root, 1)];
    while let Some((r, depth)) = stack.pop() {
        if depth > limit {
            return true;
        }
        match r {
            Refutation::LinearContradiction { .. } => {}
            Refutation::BoolCase {
                on_true, on_false, ..
            }
            | Refutation::IteCase {
                on_true, on_false, ..
            } => {
                stack.push((on_true, depth + 1));
                stack.push((on_false, depth + 1));
            }
        }
    }
    false
}

/// Independently validate `cert` against `obligation` (§4.1). The TRUSTED entry
/// point. Pure, total, solver-free.
pub fn check(obligation: &Obligation, cert: &ProofCertificate) -> CheckResult {
    // (1) Binding — a cert is valid only for the exact obligation it was issued
    // against (I-3). This is what makes a swapped/forged cert fail.
    if cert.obligation_id != obligation.id {
        return invalid(format!(
            "obligation_id mismatch: cert is for {:?}, obligation is {:?}",
            cert.obligation_id, obligation.id
        ));
    }
    let want_digest = obligation.digest();
    if cert.obligation_digest != want_digest {
        return invalid("obligation_digest does not match this obligation".into());
    }
    let want_cert_digest = ProofCertificate::compute_cert_digest(&cert.refutation);
    if cert.cert_digest != want_cert_digest {
        return invalid("cert_digest does not match the refutation bytes".into());
    }

    // (2) Bound — total, no DoS (I-4). The size check is ITERATIVE (no recursion
    // on a possibly-deep cert), and it also bounds the tree DEPTH so the
    // subsequent recursive `walk` cannot overflow the stack.
    if cert.refutation.bounded_node_count(MAX_CERT_NODES) > MAX_CERT_NODES {
        return invalid("certificate too large".into());
    }
    if refutation_depth_exceeds(&cert.refutation, MAX_CERT_DEPTH) {
        return invalid("certificate too deep".into());
    }

    // (3) Negate + NNF, with Min/Max/Abs expanded to Ite.
    let neg = eval::nnf(&Formula::Not {
        f: Box::new(expand_formula(&obligation.claim)),
    });

    // (4)+(5) Walk the refutation tree against the negated claim under no
    // assumptions, re-deriving each leaf's hypotheses.
    let env = Env::new(obligation);
    walk(&neg, &cert.refutation, &env)
}

fn invalid(reason: String) -> CheckResult {
    CheckResult::Invalid { reason }
}
fn unsupported(reason: String) -> CheckResult {
    CheckResult::Unsupported { reason }
}

/// The variable sorts, so the checker knows which `BoolCase` vars are legal.
struct Env<'a> {
    obligation: &'a Obligation,
}
impl<'a> Env<'a> {
    fn new(o: &'a Obligation) -> Env<'a> {
        Env { obligation: o }
    }
}

/// Walk the negated-claim formula `f` against the refutation `r`. Returns Valid
/// iff `r` constructively refutes `f` (proves `f` unsatisfiable).
fn walk(f: &Formula, r: &Refutation, env: &Env) -> CheckResult {
    match r {
        Refutation::BoolCase {
            var,
            on_true,
            on_false,
        } => {
            // The split var must be a declared boolean and must occur in f
            // (splitting on an absent var is not progress → non-exhaustive).
            if env.obligation.sort_of(var) != Some(Sort::Bool) {
                return invalid(format!("BoolCase on non-boolean / undeclared var {var:?}"));
            }
            if !formula_mentions_bvar(f, var) {
                return invalid(format!(
                    "non-exhaustive: BoolCase on {var:?} which does not occur in the formula"
                ));
            }
            // Both branches must refute. Substitute the assumption and recurse.
            let ft = subst_bvar(f, var, true);
            let r1 = walk(&ft, on_true, env);
            if !r1.is_valid() {
                return prefix(r1, &format!("BoolCase {var}=⊤"));
            }
            let ff = subst_bvar(f, var, false);
            let r2 = walk(&ff, on_false, env);
            if !r2.is_valid() {
                return prefix(r2, &format!("BoolCase {var}=⊥"));
            }
            CheckResult::Valid
        }
        Refutation::IteCase {
            cond,
            on_true,
            on_false,
        } => {
            // The cond must actually be the condition of some Ite present in f
            // (otherwise the split is vacuous / non-exhaustive). We expand the
            // cond too (it may mention Min/Max/Abs) for the occurrence check.
            let ecur = expand_formula(cond);
            if !formula_has_ite_cond(f, &ecur) {
                return invalid(
                    "non-exhaustive: IteCase cond does not occur as an Ite condition in the formula"
                        .into(),
                );
            }
            // on_true: resolve matching Ites to `then` and add `cond` as a
            // hypothesis (conjoined). on_false: resolve to `els` and add ¬cond.
            let ft = conj(resolve_ite(f, &ecur, true), ecur.clone());
            let r1 = walk(&ft, on_true, env);
            if !r1.is_valid() {
                return prefix(r1, "IteCase cond=⊤");
            }
            let ff = conj(
                resolve_ite(f, &ecur, false),
                Formula::Not {
                    f: Box::new(ecur.clone()),
                },
            );
            let r2 = walk(&ff, on_false, env);
            if !r2.is_valid() {
                return prefix(r2, "IteCase cond=⊥");
            }
            CheckResult::Valid
        }
        Refutation::LinearContradiction { facts, coeffs } => check_leaf(f, facts, coeffs),
    }
}

fn prefix(res: CheckResult, at: &str) -> CheckResult {
    match res {
        CheckResult::Invalid { reason } => CheckResult::Invalid {
            reason: format!("at [{at}]: {reason}"),
        },
        CheckResult::Unsupported { reason } => CheckResult::Unsupported {
            reason: format!("at [{at}]: {reason}"),
        },
        CheckResult::Valid => CheckResult::Valid,
    }
}

/// Re-derive the leaf's hypothesis set from the (fully branch-resolved) negated
/// claim, then confirm the cert's Farkas witness is a genuine contradiction over
/// those hypotheses. The checker DOES NOT trust the cert's stated facts: it
/// re-derives the legal hypotheses and requires each cert fact to be among them.
fn check_leaf(f: &Formula, facts: &[LinFact], coeffs: &[i64]) -> CheckResult {
    // The negated claim at a leaf must be a CONJUNCTION of linear literals
    // (after all Ites are resolved). Collect those hypotheses as canonical
    // `LinIneq`s. An unresolved Ite / disjunction here → not constructively
    // confirmable at this leaf.
    let mut hyps: Vec<LinIneq> = Vec::new();
    match collect_conjunctive_hypotheses(f, &mut hyps) {
        Ok(()) => {}
        Err(LeafErr::Unsupported(reason)) => return unsupported(reason),
        Err(LeafErr::Invalid(reason)) => return invalid(reason),
    }

    // Each cert fact must be ENTAILED by (i.e. present among) the re-derived
    // hypotheses. The checker normalizes the cert fact and looks for a match.
    for (i, fact) in facts.iter().enumerate() {
        let lf = match certfact_to_ineq(fact) {
            Ok(l) => l,
            Err(e) => return invalid(format!("fact {i}: {e}")),
        };
        if !hyps.iter().any(|h| ineq_eq(h, &lf)) {
            return invalid(format!(
                "fact {i} is not among the hypotheses re-derived from the negated claim \
                 (the checker does not trust the certificate's stated facts)"
            ));
        }
    }

    // Recompute the Farkas combination — must reduce to a literal contradiction.
    let ineqs: Vec<LinIneq> = match facts.iter().map(certfact_to_ineq).collect() {
        Ok(v) => v,
        Err(e) => return invalid(e),
    };
    match eval::farkas_combine(&ineqs, coeffs) {
        Ok(()) => CheckResult::Valid,
        Err(e) => invalid(format!("Farkas witness is not a contradiction: {e}")),
    }
}

enum LeafErr {
    Unsupported(String),
    Invalid(String),
}

/// Collect the conjunctive linear hypotheses of a branch-resolved negated claim.
/// Accepts only `And`/`Cmp(linear)`/`Bool(true)` (a true literal is vacuous).
/// A `Bool(false)` makes the conjunction trivially UNSAT — but the certificate
/// must witness that via a leaf; we surface it as an immediate contradiction by
/// pushing a `0 ≤ -1` fact so the Farkas step can use it. Any residual `Or`,
/// `BVar`, `Not`, or `Ite` term → the leaf cannot be confirmed here.
fn collect_conjunctive_hypotheses(f: &Formula, out: &mut Vec<LinIneq>) -> Result<(), LeafErr> {
    match f {
        Formula::Bool { v: true } => Ok(()), // vacuous
        Formula::Bool { v: false } => {
            // `false` ⇒ trivially unsatisfiable; encode as `0 ≤ -1`.
            out.push(LinIneq {
                lhs: BTreeMap::new(),
                strict: false,
                rhs: -1,
            });
            Ok(())
        }
        Formula::And { fs } => {
            for g in fs {
                collect_conjunctive_hypotheses(g, out)?;
            }
            Ok(())
        }
        Formula::Cmp { op, l, r } => {
            // Ne should have been NNF'd away (it becomes Eq under double-neg) or
            // split; a residual Ne at a leaf is a disjunction we can't take.
            if matches!(op, Op::Ne) {
                return Err(LeafErr::Invalid(
                    "residual `≠` at a leaf (must be case-split into < or >)".into(),
                ));
            }
            match eval::cmp_to_ineq(*op, l, r) {
                Ok(mut v) => {
                    out.append(&mut v);
                    Ok(())
                }
                Err(EvalErr::NotLinear { what }) => Err(LeafErr::Unsupported(format!(
                    "leaf still contains a nonlinear term ({what}) — not resolved by a case split"
                ))),
                Err(EvalErr::Overflow { what }) => {
                    Err(LeafErr::Invalid(format!("coefficient overflow: {what}")))
                }
            }
        }
        Formula::Or { .. } => Err(LeafErr::Invalid(
            "non-exhaustive: a disjunction reached a leaf un-split".into(),
        )),
        Formula::BVar { name } => Err(LeafErr::Invalid(format!(
            "boolean variable {name:?} reached a leaf un-split (needs a BoolCase)"
        ))),
        Formula::Not { f } => match f.as_ref() {
            Formula::BVar { name } => Err(LeafErr::Invalid(format!(
                "boolean literal ¬{name} reached a leaf un-split (needs a BoolCase)"
            ))),
            _ => Err(LeafErr::Invalid(
                "unexpected `Not` at a leaf (NNF should have eliminated it)".into(),
            )),
        },
        Formula::Implies { .. } => Err(LeafErr::Invalid(
            "unexpected `Implies` at a leaf (NNF should have eliminated it)".into(),
        )),
    }
}

/// Convert a certificate's `LinFact` (`Σ cᵢxᵢ op k`) to the checker's `LinIneq`.
/// Checked: duplicate-var coefficients are summed with overflow detection.
fn certfact_to_ineq(fact: &LinFact) -> Result<LinIneq, String> {
    let mut lhs: BTreeMap<String, i64> = BTreeMap::new();
    for (c, v) in &fact.terms {
        let e = lhs.entry(v.clone()).or_insert(0);
        *e = e
            .checked_add(*c)
            .ok_or_else(|| format!("coefficient overflow on {v}"))?;
    }
    lhs.retain(|_, c| *c != 0);
    Ok(LinIneq {
        lhs,
        strict: matches!(fact.op, FactOp::Lt),
        rhs: fact.constant,
    })
}

fn ineq_eq(a: &LinIneq, b: &LinIneq) -> bool {
    a.strict == b.strict && a.rhs == b.rhs && a.lhs == b.lhs
}

// ── Formula / term rewriting (pure, no search) ───────────────────────────────

/// Expand `Min/Max/Abs` into `Ite` so the only nonlinear construct is `Ite`.
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
        // min(l,r) = if l ≤ r then l else r
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
        // max(l,r) = if l ≥ r then l else r
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
        // abs(t) = if t ≥ 0 then t else -t
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

/// Substitute a boolean variable with a constant, simplifying along the way.
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
    if out.is_empty() {
        Formula::Bool { v: true }
    } else if out.len() == 1 {
        out.pop().unwrap()
    } else {
        Formula::And { fs: out }
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
    if out.is_empty() {
        Formula::Bool { v: false }
    } else if out.len() == 1 {
        out.pop().unwrap()
    } else {
        Formula::Or { fs: out }
    }
}

/// Conjoin an extra hypothesis onto a formula (then re-NNF to keep leaves clean).
fn conj(f: Formula, extra: Formula) -> Formula {
    eval::nnf(&Formula::And { fs: vec![f, extra] })
}

fn formula_mentions_bvar(f: &Formula, var: &str) -> bool {
    match f {
        Formula::Bool { .. } => false,
        Formula::BVar { name } => name == var,
        Formula::Not { f } => formula_mentions_bvar(f, var),
        Formula::And { fs } | Formula::Or { fs } => {
            fs.iter().any(|g| formula_mentions_bvar(g, var))
        }
        Formula::Implies { a, b } => formula_mentions_bvar(a, var) || formula_mentions_bvar(b, var),
        Formula::Cmp { .. } => false,
    }
}

/// True iff `cond` occurs as the condition of some `Ite` term in `f` (both args
/// already expanded). This makes an `IteCase` split non-vacuous.
fn formula_has_ite_cond(f: &Formula, cond: &Formula) -> bool {
    match f {
        Formula::Bool { .. } | Formula::BVar { .. } => false,
        Formula::Not { f } => formula_has_ite_cond(f, cond),
        Formula::And { fs } | Formula::Or { fs } => {
            fs.iter().any(|g| formula_has_ite_cond(g, cond))
        }
        Formula::Implies { a, b } => formula_has_ite_cond(a, cond) || formula_has_ite_cond(b, cond),
        Formula::Cmp { l, r, .. } => term_has_ite_cond(l, cond) || term_has_ite_cond(r, cond),
    }
}

fn term_has_ite_cond(t: &Term, cond: &Formula) -> bool {
    match t {
        Term::Int { .. } | Term::IVar { .. } => false,
        Term::Add { l, r } | Term::Sub { l, r } | Term::Min { l, r } | Term::Max { l, r } => {
            term_has_ite_cond(l, cond) || term_has_ite_cond(r, cond)
        }
        Term::MulConst { t, .. } | Term::Abs { t } => term_has_ite_cond(t, cond),
        Term::Ite { cond: c, then, els } => {
            c.as_ref() == cond || term_has_ite_cond(then, cond) || term_has_ite_cond(els, cond)
        }
    }
}

/// Resolve every `Ite` whose condition equals `cond` to its `then` (if `branch`)
/// or `els` (otherwise), throughout the formula. Other Ites are left intact.
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
    use crate::certificate::ProofCertificate;
    use crate::obligation::build::*;
    use crate::obligation::Var;

    /// ∀ g, avail. min(g, avail) ≤ avail
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

    /// A hand-built valid certificate for carve. Negated claim:
    ///   ¬(min(g,avail) ≤ avail)  =  min(g,avail) > avail
    /// min expands to Ite(g≤avail, g, avail). Split on (g ≤ avail):
    ///   • TRUE  : g > avail ∧ g ≤ avail  →  contradiction
    ///       hyps: (g - avail < 0)  [from g≤avail]   and  (avail - g < 0) [from g>avail]
    ///       wait — g>avail ⇒ avail - g < 0 ; g≤avail ⇒ g - avail ≤ 0
    ///       Farkas: 1·(g-avail ≤ 0) + 1·(avail-g < 0) = 0 < 0  ✓
    ///   • FALSE : avail > avail  →  (avail - avail < 0) = (0 < 0) immediate
    fn carve_cert() -> ProofCertificate {
        let o = carve();
        let cond = cmp(Op::Le, ivar("g"), ivar("avail")); // the Ite cond
        let on_true = Refutation::LinearContradiction {
            facts: vec![
                // g ≤ avail   ⇒  g - avail ≤ 0   ⇒ {g:1, avail:-1} ≤ 0
                LinFact {
                    terms: vec![(1, "g".into()), (-1, "avail".into())],
                    op: FactOp::Le,
                    constant: 0,
                },
                // g > avail   ⇒  avail - g < 0   ⇒ {avail:1, g:-1} < 0
                LinFact {
                    terms: vec![(1, "avail".into()), (-1, "g".into())],
                    op: FactOp::Lt,
                    constant: 0,
                },
            ],
            coeffs: vec![1, 1],
        };
        let on_false = Refutation::LinearContradiction {
            // avail > avail  ⇒  avail - avail < 0  ⇒  {} < 0   (literal 0 < 0)
            facts: vec![LinFact {
                terms: vec![],
                op: FactOp::Lt,
                constant: 0,
            }],
            coeffs: vec![1],
        };
        let refutation = Refutation::IteCase {
            cond,
            on_true: Box::new(on_true),
            on_false: Box::new(on_false),
        };
        ProofCertificate::new(&o.id, &o.digest(), refutation)
    }

    #[test]
    fn valid_carve_cert_is_accepted() {
        let o = carve();
        let c = carve_cert();
        assert_eq!(
            check(&o, &c),
            CheckResult::Valid,
            "carve cert must be valid"
        );
    }

    #[test]
    fn wrong_obligation_binding_is_invalid() {
        let o = carve();
        let mut c = carve_cert();
        c.obligation_digest = "axsha256:deadbeef".into();
        assert!(matches!(check(&o, &c), CheckResult::Invalid { .. }));
    }

    #[test]
    fn mutated_cert_digest_is_invalid() {
        let o = carve();
        let mut c = carve_cert();
        c.cert_digest = "axcert1:0".into();
        assert!(matches!(check(&o, &c), CheckResult::Invalid { .. }));
    }

    #[test]
    fn negative_coeff_is_invalid() {
        let o = carve();
        let mut c = carve_cert();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { coeffs, .. } = on_true.as_mut() {
                coeffs[1] = -1;
            }
        }
        // Re-hash so it passes binding but fails the Farkas check.
        c.cert_digest = ProofCertificate::compute_cert_digest(&c.refutation);
        let r = check(&o, &c);
        assert!(matches!(r, CheckResult::Invalid { .. }), "{r:?}");
    }

    #[test]
    fn nonexhaustive_split_substituting_a_bogus_leaf_is_invalid() {
        // Replace on_false with a leaf whose fact is NOT among the hypotheses.
        let o = carve();
        let mut c = carve_cert();
        if let Refutation::IteCase { on_false, .. } = &mut c.refutation {
            **on_false = Refutation::LinearContradiction {
                facts: vec![LinFact {
                    terms: vec![(1, "g".into())],
                    op: FactOp::Le,
                    constant: -1,
                }],
                coeffs: vec![1],
            };
        }
        c.cert_digest = ProofCertificate::compute_cert_digest(&c.refutation);
        assert!(matches!(check(&o, &c), CheckResult::Invalid { .. }));
    }

    // R23 §7 normative: a valid cert for one obligation presented for another.
    #[test]
    fn wrong_obligation_binding() {
        let carve_o = carve();
        let other = Obligation {
            id: "principal_mint/O2".into(),
            vars: carve_o.vars.clone(),
            claim: cmp(Op::Le, min(ivar("g"), ivar("avail")), ivar("avail")),
        };
        // A perfectly valid carve cert, checked against a DIFFERENT obligation id.
        let c = carve_cert();
        assert!(
            matches!(check(&other, &c), CheckResult::Invalid { .. }),
            "a cert cannot be transplanted to another obligation"
        );
    }

    // R23 §7 normative: a nonlinear obligation is Unsupported (fail-closed),
    // never a false Valid.
    #[test]
    fn unsupported_is_failclosed() {
        // g·g ≤ 0 is nonlinear — but `Mul(Term,Term)` is unrepresentable, so we
        // express nonlinearity via an Ite the cert never splits, forcing the
        // leaf to hit a residual nonlinear term → Unsupported.
        let o = Obligation {
            id: "nonlinear/x".into(),
            vars: vec![Var {
                name: "g".into(),
                sort: Sort::Int,
            }],
            // min(g, g) ≤ 0 ; a cert that goes straight to a leaf without
            // splitting the min's Ite leaves a residual nonlinearity.
            claim: cmp(Op::Le, min(ivar("g"), ivar("g")), int(0)),
        };
        // A leaf cert that does NOT split the Ite.
        let c = ProofCertificate::new(
            &o.id,
            &o.digest(),
            Refutation::LinearContradiction {
                facts: vec![],
                coeffs: vec![],
            },
        );
        assert!(
            matches!(
                check(&o, &c),
                CheckResult::Unsupported { .. } | CheckResult::Invalid { .. }
            ),
            "nonlinear-at-leaf must be Unsupported/Invalid, never Valid"
        );
        assert!(!check(&o, &c).is_valid());
    }

    // R23 §7 normative: adversarial huge coefficients → coefficient overflow is
    // CAUGHT (checked arithmetic), never wrapped into a spurious contradiction.
    #[test]
    fn eval_coeff_overflow_is_caught() {
        let o = carve();
        let mut c = carve_cert();
        if let Refutation::IteCase { on_true, .. } = &mut c.refutation {
            if let Refutation::LinearContradiction { coeffs, .. } = on_true.as_mut() {
                coeffs[0] = i64::MAX; // multiplying a fact by i64::MAX overflows
            }
        }
        c.cert_digest = ProofCertificate::compute_cert_digest(&c.refutation);
        let r = check(&o, &c);
        assert!(matches!(r, CheckResult::Invalid { .. }), "{r:?}");
    }

    // R23 §4.4 / I-4 normative: an over-DEEP cert is rejected, bounded — the
    // checker never recurses unboundedly (no stack overflow, fail closed).
    #[test]
    fn oversize_cert_is_invalid() {
        let o = carve();
        // A degenerate left-spine BoolCase tree deeper than MAX_CERT_DEPTH.
        let mut r = Refutation::LinearContradiction {
            facts: vec![],
            coeffs: vec![],
        };
        for _ in 0..(MAX_CERT_DEPTH + 5) {
            r = Refutation::BoolCase {
                var: "b".into(),
                on_true: Box::new(r),
                on_false: Box::new(Refutation::LinearContradiction {
                    facts: vec![],
                    coeffs: vec![],
                }),
            };
        }
        let c = ProofCertificate::new(&o.id, &o.digest(), r);
        match check(&o, &c) {
            CheckResult::Invalid { reason } => assert!(
                reason.contains("too deep") || reason.contains("too large"),
                "{reason}"
            ),
            other => panic!("oversize cert must be Invalid, got {other:?}"),
        }
    }

    // The bound rejects a WIDE cert too (many nodes, shallow): build a balanced
    // tree just over MAX_CERT_NODES and assert "too large".
    #[test]
    fn oversize_by_node_count_is_invalid() {
        // A balanced binary tree of depth d has ~2^d leaves; depth 20 ⇒ ~1M
        // nodes (> MAX_CERT_NODES) but depth (20) < MAX_CERT_DEPTH, so this hits
        // the NODE bound, not the depth bound. Build it iteratively-ish; cap the
        // construction depth small enough to be cheap yet exceed 100k nodes.
        let o = carve();
        let mut r = Refutation::LinearContradiction {
            facts: vec![],
            coeffs: vec![],
        };
        // depth 17 ⇒ 2^17 ≈ 131k leaf nodes (> MAX_CERT_NODES).
        for _ in 0..17 {
            r = Refutation::BoolCase {
                var: "b".into(),
                on_true: Box::new(r.clone()),
                on_false: Box::new(r),
            };
        }
        assert!(r.bounded_node_count(MAX_CERT_NODES) > MAX_CERT_NODES);
        let c = ProofCertificate::new(&o.id, &o.digest(), r);
        match check(&o, &c) {
            CheckResult::Invalid { reason } => assert!(reason.contains("too large"), "{reason}"),
            other => panic!("wide cert must be Invalid, got {other:?}"),
        }
    }
}
