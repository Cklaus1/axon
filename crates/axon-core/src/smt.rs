//! SMT-backed `@[verify]` static proof (R9, `smt` feature).
//!
//! `@[verify(value OP K)]` is a *runtime* gate today (interp.rs / verify.rs).
//! This module adds a *static* proof: encode a function's body as an SMT term
//! over its integer parameters, then ask Z3 whether the bound can be VIOLATED.
//! If `∃ params. ¬(R(params) OP K)` is **unsat**, the bound holds for all inputs
//! (proven); if **sat**, Z3's model is a concrete counterexample.
//!
//! Scope (R9-smt-verify.md §4.1): the *provable fragment* is straight-line
//! integer arithmetic — i64 params, body built from int literals, params,
//! `+ - *`, comparisons, and `if/else` (→ SMT `ite`). Anything else (loops,
//! calls, float, string) is honestly **Unsupported**, not a false proof — the
//! runtime gate still applies.
//!
//! The whole module (and the `z3` dependency) is behind `#[cfg(feature = "smt")]`,
//! so the default codegen/interp build never links Z3.

#![cfg(feature = "smt")]

use crate::ast::{BinOp, Expr, FnDef, Item, Literal, Program, Stmt};

use z3::ast::{Ast, Bool, Int, Real};
use z3::{Config, Context, SatResult, Solver};

/// The outcome of attempting a static proof of one `@[verify]` bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofResult {
    /// Proven: the bound holds for every input in the integer domain.
    Proven { function: String },
    /// Disproven: a concrete input violates the bound (E1102).
    Counterexample { function: String, inputs: Vec<(String, i64)>, predicate: String },
    /// Outside the provable fragment — the runtime gate still applies (W1103).
    Unsupported { function: String, reason: String },
}

/// Statically check every `@[verify]`-annotated function in `program` against
/// its bound, via Z3. Functions with no `@[verify]` are skipped. Deterministic:
/// the encoding is a pure function of the AST and Z3 is deterministic per query.
pub fn prove_verify_bounds(program: &Program) -> Vec<ProofResult> {
    let mut out = Vec::new();
    for item in &program.items {
        let Item::FnDef(f) = item else { continue };
        let Some(spec) = &f.verify else { continue };
        // Decode the predicate as a CONJUNCTION of `ident OP K` atoms (a single
        // atom is a 1-element list). `value > 0 && value < 100` proves both
        // bounds; a `confidence` atom (a runtime Uncertain field, not a static
        // integer property) makes the whole conjunction Unsupported — the
        // runtime gate still enforces it.
        let Some(atoms) = crate::verify::decode_verify_conjunction(&spec.predicate) else {
            out.push(ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "predicate is not a conjunction of `ident OP K` comparisons \
                         (disjunctions `||` stay runtime-only)".into(),
            });
            continue;
        };
        if let Some((bad, _, _)) = atoms.iter().find(|(id, _, _)| id != "value") {
            out.push(ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!(
                    "only `value OP K` atoms are statically provable in v1 (got `{bad}`); \
                     the runtime gate enforces the full predicate"
                ),
            });
            continue;
        }
        let value_atoms: Vec<(BinOp, f64)> =
            atoms.into_iter().map(|(_, op, b)| (op, b)).collect();
        out.push(prove_conjunction(f, &value_atoms));
    }
    out
}

/// Prove a CONJUNCTION of `value OP K` atoms: every atom must hold for all
/// inputs. Z3 sat-checks the negation of the AND — `∃ params. ¬(A ∧ B ∧ …)` —
/// so `unsat` ⇒ all bounds proven, `sat` ⇒ a single input violates at least one
/// (the counterexample). A 1-element list is exactly the old single-bound proof.
fn prove_conjunction(f: &FnDef, atoms: &[(BinOp, f64)]) -> ProofResult {
    // Route to the float encoder if any param/return is f64 or any bound is
    // non-integer — but only the single-atom case has a float encoder today;
    // a float CONJUNCTION falls back to per-atom float proof being all-proven.
    let any_f64 = f.params.iter().any(|p| is_f64_type(&p.ty));
    let ret_f64 = f.return_type.as_ref().map(is_f64_type).unwrap_or(false);
    let any_frac = atoms.iter().any(|(_, b)| b.fract() != 0.0);
    if any_f64 || ret_f64 || any_frac {
        // Float fragment: prove each atom independently; the conjunction holds
        // iff all do, and a counterexample on any atom disproves the whole.
        for (op, bound) in atoms {
            match prove_one_f64(f, op, *bound) {
                ProofResult::Proven { .. } => continue,
                other => return other, // Counterexample or Unsupported short-circuits
            }
        }
        return ProofResult::Proven { function: f.name.clone() };
    }
    prove_one_int_conjunction(f, atoms)
}

/// Prove a single fn's `value OP K` bound (the original entry point, retained
/// for callers/tests). Delegates to the conjunction path with one atom.
fn prove_one(f: &FnDef, op: &BinOp, bound: f64) -> ProofResult {
    let any_f64 = f.params.iter().any(|p| is_f64_type(&p.ty));
    let ret_f64 = f.return_type.as_ref().map(is_f64_type).unwrap_or(false);
    if any_f64 || ret_f64 || bound.fract() != 0.0 {
        return prove_one_f64(f, op, bound);
    }
    prove_one_int_conjunction(f, std::slice::from_ref(&(op.clone(), bound)))
}

/// Prove a conjunction of `value OP K` atoms over the INTEGER fragment. Every
/// atom must hold for all inputs; Z3 sat-checks `∃ params. ¬(A₁ ∧ A₂ ∧ …)`.
/// `unsat` ⇒ all proven; `sat` ⇒ a single counterexample input that breaks the
/// conjunction. One atom is the classic single-bound proof.
fn prove_one_int_conjunction(f: &FnDef, atoms: &[(BinOp, f64)]) -> ProofResult {
    // Integer fragment: every param must be i64.
    for p in &f.params {
        if !is_i64_type(&p.ty) {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("param `{}` is not i64 (v1 fragment is integer)", p.name),
            };
        }
    }

    let cfg = Config::new();
    let ctx = Context::new(&cfg);

    // One Z3 Int const per parameter.
    let params: Vec<(String, Int)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Int::new_const(&ctx, p.name.as_str())))
        .collect();

    // Encode the body as an Int term R(params).
    let body = match encode_expr(&ctx, &f.body, &params) {
        Some(t) => t,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "body uses a construct outside the straight-line integer fragment \
                         (loops, calls, float, string, …)".into(),
            }
        }
    };

    // Build each atom's bound predicate and conjoin them.
    let mut atom_preds = Vec::with_capacity(atoms.len());
    let mut atom_strs = Vec::with_capacity(atoms.len());
    for (op, bound) in atoms {
        let bound_i = *bound as i64;
        let k = Int::from_i64(&ctx, bound_i);
        let p = match op {
            BinOp::GtEq => body.ge(&k),
            BinOp::Gt => body.gt(&k),
            BinOp::LtEq => body.le(&k),
            BinOp::Lt => body.lt(&k),
            BinOp::Eq => body._eq(&k),
            BinOp::NotEq => body._eq(&k).not(),
            _ => {
                return ProofResult::Unsupported {
                    function: f.name.clone(),
                    reason: format!("verify operator {op:?} is not a supported comparison"),
                }
            }
        };
        atom_preds.push(p);
        atom_strs.push(format!("value {} {bound_i}", crate::verify::binop_to_verify_str(op)));
    }
    // Conjoin: B := A₁ ∧ A₂ ∧ … ; the VC is ¬B.
    let refs: Vec<&z3::ast::Bool> = atom_preds.iter().collect();
    let conj = z3::ast::Bool::and(&ctx, &refs);
    let neg = conj.not();

    let solver = Solver::new(&ctx);
    solver.assert(&neg);
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven { function: f.name.clone() },
        SatResult::Sat => {
            let model = solver.get_model().expect("sat ⇒ a model exists");
            let inputs = params
                .iter()
                .map(|(name, c)| {
                    let v = model.eval(c, true).and_then(|i| i.as_i64()).unwrap_or(0);
                    (name.clone(), v)
                })
                .collect();
            ProofResult::Counterexample {
                function: f.name.clone(),
                inputs,
                predicate: atom_strs.join(" && "),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "Z3 returned unknown (the VC was undecidable within limits)".into(),
        },
    }
}

/// The FLOAT fragment (R9): prove a `@[verify(value OP K)]` bound over a
/// straight-line `f64` function, encoding the body as a Z3 `Real` term. Z3's
/// `Real` is EXACT rational arithmetic — so this proves the *mathematical*
/// bound soundly (`x*x >= 0` for all real x), with the honest caveat that it
/// models real, not IEEE-754, arithmetic (rounding/NaN/Inf are out of fragment,
/// reported Unsupported). Mirrors the integer path's structure exactly.
fn prove_one_f64(f: &FnDef, op: &BinOp, bound: f64) -> ProofResult {
    for p in &f.params {
        if !is_f64_type(&p.ty) {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("param `{}` is not f64 (this is the float fragment)", p.name),
            };
        }
    }
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let params: Vec<(String, Real)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Real::new_const(&ctx, p.name.as_str())))
        .collect();
    let body = match encode_expr_real(&ctx, &f.body, &params) {
        Some(t) => t,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "body uses a construct outside the straight-line float fragment \
                         (loops, calls, div, string, IEEE rounding, …)".into(),
            }
        }
    };
    // Bound as a Real rational. f64 → (num, den) via a fixed denominator keeps
    // it exact for the common decimal bounds (e.g. 0.5 → 1/2, 0.0 → 0/1).
    let k = f64_to_real(&ctx, bound);
    let bound_pred = match op {
        BinOp::GtEq => body.ge(&k),
        BinOp::Gt => body.gt(&k),
        BinOp::LtEq => body.le(&k),
        BinOp::Lt => body.lt(&k),
        BinOp::Eq => body._eq(&k),
        BinOp::NotEq => body._eq(&k).not(),
        _ => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("verify operator {op:?} is not a supported comparison"),
            }
        }
    };
    let solver = Solver::new(&ctx);
    solver.assert(&bound_pred.not());
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven { function: f.name.clone() },
        SatResult::Sat => {
            let model = solver.get_model().expect("sat ⇒ a model exists");
            // Report the violating inputs as the truncated integer part (the
            // float counterexample's exact rational is in the model; the
            // ProofResult carries i64 inputs, so we round toward zero).
            let inputs = params
                .iter()
                .map(|(name, c)| {
                    let v = model.eval(c, true)
                        .and_then(|r| r.as_real())
                        .map(|(num, den)| if den != 0 { num / den } else { 0 })
                        .unwrap_or(0);
                    (name.clone(), v)
                })
                .collect();
            ProofResult::Counterexample {
                function: f.name.clone(),
                inputs,
                predicate: format!("value {} {bound}", crate::verify::binop_to_verify_str(op)),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "Z3 returned unknown (the float VC was undecidable within limits)".into(),
        },
    }
}

/// Convert an `f64` bound to a Z3 `Real`. Uses a fixed 1e6 denominator so the
/// common decimal bounds (`0.0`, `0.5`, `0.9`, `1.25`) are represented exactly.
fn f64_to_real(ctx: &Context, x: f64) -> Real<'_> {
    let den: i64 = 1_000_000;
    let num = (x * den as f64).round() as i64;
    // Real::from_real takes i32; reduce to keep within range for typical bounds.
    let g = gcd(num.unsigned_abs(), den as u64) as i64;
    let (n, d) = if g != 0 { (num / g, den / g) } else { (num, den) };
    Real::from_real(ctx, n as i32, d as i32)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 { let t = b; b = a % b; a = t; }
    a
}

/// Encode an `Expr` as a Z3 `Real` term — the float counterpart of
/// [`encode_expr`]. Same straight-line fragment (`+`/`-`/`*`, `ite`, literals,
/// params); `Div` stays out (partial). Integer literals coerce to Real.
fn encode_expr_real<'c>(ctx: &'c Context, e: &Expr, params: &[(String, Real<'c>)]) -> Option<Real<'c>> {
    match e {
        Expr::Literal(Literal::Float(x)) => Some(f64_to_real(ctx, *x)),
        Expr::Literal(Literal::Int(n)) => Some(f64_to_real(ctx, *n as f64)),
        Expr::Ident(name) => params.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone()),
        Expr::BinOp { op, left, right } => {
            let l = encode_expr_real(ctx, left, params)?;
            let r = encode_expr_real(ctx, right, params)?;
            match op {
                BinOp::Add => Some(&l + &r),
                BinOp::Sub => Some(&l - &r),
                BinOp::Mul => Some(&l * &r),
                _ => None,
            }
        }
        Expr::If { cond, then, else_ } => {
            let else_e = else_.as_ref()?;
            let c = encode_bool_real(ctx, cond, params)?;
            let a = encode_expr_real(ctx, then, params)?;
            let b = encode_expr_real(ctx, else_e, params)?;
            Some(c.ite(&a, &b))
        }
        Expr::Block(stmts) => match stmts.as_slice() {
            [only] => encode_expr_real(ctx, &only.expr, params),
            _ => None,
        },
        _ => None,
    }
}

/// Encode a boolean condition over `Real` terms (float counterpart of
/// [`encode_bool`]).
fn encode_bool_real<'c>(ctx: &'c Context, e: &Expr, params: &[(String, Real<'c>)]) -> Option<Bool<'c>> {
    match e {
        Expr::BinOp { op, left, right } => {
            let l = encode_expr_real(ctx, left, params)?;
            let r = encode_expr_real(ctx, right, params)?;
            match op {
                BinOp::Lt => Some(l.lt(&r)),
                BinOp::Gt => Some(l.gt(&r)),
                BinOp::LtEq => Some(l.le(&r)),
                BinOp::GtEq => Some(l.ge(&r)),
                BinOp::Eq => Some(l._eq(&r)),
                BinOp::NotEq => Some(l._eq(&r).not()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Encode an `Expr` as a Z3 `Int` term, or `None` if it falls outside the
/// straight-line integer fragment. A block evaluates to its tail expression.
fn encode_expr<'c>(ctx: &'c Context, e: &Expr, params: &[(String, Int<'c>)]) -> Option<Int<'c>> {
    match e {
        Expr::Literal(Literal::Int(n)) => Some(Int::from_i64(ctx, *n)),
        Expr::Ident(name) => params.iter().find(|(n, _)| n == name).map(|(_, c)| c.clone()),
        Expr::BinOp { op, left, right } => {
            let l = encode_expr(ctx, left, params)?;
            let r = encode_expr(ctx, right, params)?;
            match op {
                BinOp::Add => Some(&l + &r),
                BinOp::Sub => Some(&l - &r),
                BinOp::Mul => Some(&l * &r),
                // Div/Rem are partial (div-by-zero) — keep them out of v1.
                _ => None,
            }
        }
        Expr::If { cond, then, else_ } => {
            // `if c { a } else { b }` → ite(encode_bool(c), encode(a), encode(b)).
            let else_e = else_.as_ref()?; // a verify-return must have both arms
            let c = encode_bool(ctx, cond, params)?;
            let a = encode_expr(ctx, then, params)?;
            let b = encode_expr(ctx, else_e, params)?;
            Some(c.ite(&a, &b))
        }
        Expr::Block(stmts) => encode_block(ctx, stmts, params),
        // Unary minus shows up as `0 - x` from the parser, handled by BinOp::Sub.
        _ => None,
    }
}

/// A block's value is its final expression (no `let`-bindings in the v1
/// fragment — they'd need substitution; keep v1 to straight-line returns).
fn encode_block<'c>(ctx: &'c Context, stmts: &[Stmt], params: &[(String, Int<'c>)]) -> Option<Int<'c>> {
    // Only a single tail expression is in-fragment.
    match stmts {
        [only] => encode_expr(ctx, &only.expr, params),
        _ => None,
    }
}

/// Encode a boolean condition (a comparison of integer terms) as a Z3 `Bool`.
fn encode_bool<'c>(ctx: &'c Context, e: &Expr, params: &[(String, Int<'c>)]) -> Option<Bool<'c>> {
    match e {
        Expr::BinOp { op, left, right } => {
            let l = encode_expr(ctx, left, params)?;
            let r = encode_expr(ctx, right, params)?;
            match op {
                BinOp::Lt => Some(l.lt(&r)),
                BinOp::Gt => Some(l.gt(&r)),
                BinOp::LtEq => Some(l.le(&r)),
                BinOp::GtEq => Some(l.ge(&r)),
                BinOp::Eq => Some(l._eq(&r)),
                BinOp::NotEq => Some(l._eq(&r).not()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// Whether an AST type is `i64` (the integer-fragment parameter type).
fn is_i64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "i64")
}

/// Whether an AST type is `f64` (the float-fragment parameter type).
fn is_f64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "f64")
}

// ── Phase 5 §4: refinement-RETURN proofs ─────────────────────────────────────

/// Statically prove that every function with a named-refinement *return type*
/// satisfies its predicate for ALL inputs — the non-constant proof the checker's
/// constant evaluator can't do. e.g. `fn abs_pos(n: i64) -> Positive { if n < 0
/// { 0 - n } else { n } }` where `type Positive = i64 where _ > 0`. Encode the
/// body as `R(params)`, bind the predicate's `_` to `R`, and ask Z3 whether the
/// predicate can be VIOLATED (`∃ params. ¬pred[R/_]` unsat ⇒ proven). Same
/// integer fragment as the @[verify] prover.
///
/// `refinements` maps a refinement type name → its predicate Expr (the binder is
/// `_`). Returns one ProofResult per refinement-returning fn in the fragment.
pub fn prove_refinement_returns(
    program: &Program,
    refinements: &std::collections::HashMap<String, Expr>,
) -> Vec<ProofResult> {
    let mut out = Vec::new();
    for item in &program.items {
        let Item::FnDef(f) = item else { continue };
        // The return type must name a known refinement.
        let Some(crate::ast::AxonType::Named(rname)) = &f.return_type else { continue };
        let Some(pred) = refinements.get(rname) else { continue };
        // Integer fragment: all params i64.
        if f.params.iter().all(|p| is_i64_type(&p.ty)) {
            out.push(prove_one_refinement_return(f, pred, rname));
        } else if !f.params.is_empty() && f.params.iter().all(|p| is_f64_type(&p.ty)) {
            // Float fragment: all params f64 (e.g. `fn norm(x: f64) -> NonNegF`).
            out.push(prove_one_refinement_return_f64(f, pred, rname));
        }
        // Mixed / other param types fall outside the v1 fragment — skipped
        // (the runtime obligation / constant checker still applies).
    }
    out
}

/// Float-fragment analog of `prove_one_refinement_return`: encode the f64 body as
/// a Z3 Real and prove the predicate (with `_` → body) holds for all inputs.
fn prove_one_refinement_return_f64(f: &FnDef, pred: &Expr, rname: &str) -> ProofResult {
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let params: Vec<(String, Real)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Real::new_const(&ctx, p.name.as_str())))
        .collect();

    let body = match encode_expr_real(&ctx, &f.body, &params) {
        Some(t) => t,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "body uses a construct outside the straight-line float fragment".into(),
            }
        }
    };
    let pred_z3 = match encode_pred_binder_real(&ctx, pred, &body) {
        Some(b) => b,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("refinement `{rname}`'s predicate is outside the f64 fragment"),
            }
        }
    };
    let solver = Solver::new(&ctx);
    solver.assert(&pred_z3.not());
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven { function: f.name.clone() },
        SatResult::Sat => {
            let inputs = solver
                .get_model()
                .map(|m| {
                    params
                        .iter()
                        .filter_map(|(n, c)| {
                            // Reals print as rationals; round toward zero for the i64 report.
                            m.eval(c, true).and_then(|v| v.as_real()).map(|(num, den)| {
                                (n.clone(), if den != 0 { num / den } else { 0 })
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            ProofResult::Counterexample {
                function: f.name.clone(),
                inputs,
                predicate: format!("return type `{rname}`"),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "Z3 returned unknown".into(),
        },
    }
}

/// f64 analog of `encode_pred_binder`: encode a refinement predicate as Z3 Bool
/// with `_` → the Real body term.
fn encode_pred_binder_real<'c>(
    ctx: &'c Context,
    e: &Expr,
    binder_term: &Real<'c>,
) -> Option<Bool<'c>> {
    match e {
        Expr::UnaryOp { op: crate::ast::UnaryOp::Not, operand } => {
            Some(encode_pred_binder_real(ctx, operand, binder_term)?.not())
        }
        Expr::BinOp { op, left, right } => match op {
            BinOp::And => {
                let l = encode_pred_binder_real(ctx, left, binder_term)?;
                let r = encode_pred_binder_real(ctx, right, binder_term)?;
                Some(Bool::and(ctx, &[&l, &r]))
            }
            BinOp::Or => {
                let l = encode_pred_binder_real(ctx, left, binder_term)?;
                let r = encode_pred_binder_real(ctx, right, binder_term)?;
                Some(Bool::or(ctx, &[&l, &r]))
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::Eq | BinOp::NotEq => {
                let l = encode_pred_real(ctx, left, binder_term)?;
                let r = encode_pred_real(ctx, right, binder_term)?;
                Some(match op {
                    BinOp::Lt => l.lt(&r),
                    BinOp::Gt => l.gt(&r),
                    BinOp::LtEq => l.le(&r),
                    BinOp::GtEq => l.ge(&r),
                    BinOp::Eq => l._eq(&r),
                    BinOp::NotEq => l._eq(&r).not(),
                    _ => unreachable!(),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// f64 analog of `encode_pred_int`: a Real-valued predicate sub-expression with
/// `_` → the body. Float literals come through as `Literal::Float`.
fn encode_pred_real<'c>(
    ctx: &'c Context,
    e: &Expr,
    binder_term: &Real<'c>,
) -> Option<Real<'c>> {
    match e {
        Expr::Ident(n) if n == "_" => Some(binder_term.clone()),
        Expr::Literal(Literal::Float(v)) => {
            // Z3 Real from a decimal — go through a rational string.
            Some(Real::from_real_str(ctx, &format!("{v}"), "1").unwrap_or_else(|| Real::from_real(ctx, 0, 1)))
        }
        Expr::Literal(Literal::Int(v)) => Some(Real::from_real(ctx, *v as i32, 1)),
        Expr::UnaryOp { op: crate::ast::UnaryOp::Neg, operand } => {
            Some(encode_pred_real(ctx, operand, binder_term)?.unary_minus())
        }
        Expr::BinOp { op, left, right } => {
            let l = encode_pred_real(ctx, left, binder_term)?;
            let r = encode_pred_real(ctx, right, binder_term)?;
            match op {
                BinOp::Add => Some(&l + &r),
                BinOp::Sub => Some(&l - &r),
                BinOp::Mul => Some(&l * &r),
                _ => None,
            }
        }
        _ => None,
    }
}

fn prove_one_refinement_return(f: &FnDef, pred: &Expr, rname: &str) -> ProofResult {
    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let params: Vec<(String, Int)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Int::new_const(&ctx, p.name.as_str())))
        .collect();

    // Encode the body as an Int term R(params).
    let body = match encode_expr(&ctx, &f.body, &params) {
        Some(t) => t,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "body uses a construct outside the straight-line integer fragment".into(),
            }
        }
    };

    // Encode the predicate with `_` bound to the body term.
    let pred_z3 = match encode_pred_binder(&ctx, pred, &body) {
        Some(b) => b,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("refinement `{rname}`'s predicate is outside the i64 fragment"),
            }
        }
    };

    let solver = Solver::new(&ctx);
    solver.assert(&pred_z3.not());
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven { function: f.name.clone() },
        SatResult::Sat => {
            let inputs = solver
                .get_model()
                .map(|m| {
                    params
                        .iter()
                        .filter_map(|(n, c)| m.eval(c, true).and_then(|v| v.as_i64()).map(|v| (n.clone(), v)))
                        .collect()
                })
                .unwrap_or_default();
            ProofResult::Counterexample {
                function: f.name.clone(),
                inputs,
                predicate: format!("return type `{rname}`"),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "Z3 returned unknown".into(),
        },
    }
}

/// Encode a refinement predicate as a Z3 Bool, with the binder `_` mapped to
/// `binder_term`. Supports comparisons of i64 terms + `&&`/`||`/`!`.
fn encode_pred_binder<'c>(
    ctx: &'c Context,
    e: &Expr,
    binder_term: &Int<'c>,
) -> Option<Bool<'c>> {
    match e {
        Expr::UnaryOp { op: crate::ast::UnaryOp::Not, operand } => {
            Some(encode_pred_binder(ctx, operand, binder_term)?.not())
        }
        Expr::BinOp { op, left, right } => match op {
            BinOp::And => {
                let l = encode_pred_binder(ctx, left, binder_term)?;
                let r = encode_pred_binder(ctx, right, binder_term)?;
                Some(Bool::and(ctx, &[&l, &r]))
            }
            BinOp::Or => {
                let l = encode_pred_binder(ctx, left, binder_term)?;
                let r = encode_pred_binder(ctx, right, binder_term)?;
                Some(Bool::or(ctx, &[&l, &r]))
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::Eq | BinOp::NotEq => {
                let l = encode_pred_int(ctx, left, binder_term)?;
                let r = encode_pred_int(ctx, right, binder_term)?;
                Some(match op {
                    BinOp::Lt => l.lt(&r),
                    BinOp::Gt => l.gt(&r),
                    BinOp::LtEq => l.le(&r),
                    BinOp::GtEq => l.ge(&r),
                    BinOp::Eq => l._eq(&r),
                    BinOp::NotEq => l._eq(&r).not(),
                    _ => unreachable!(),
                })
            }
            _ => None,
        },
        _ => None,
    }
}

/// Encode an int sub-expression of a refinement predicate, with `_` → binder.
fn encode_pred_int<'c>(
    ctx: &'c Context,
    e: &Expr,
    binder_term: &Int<'c>,
) -> Option<Int<'c>> {
    match e {
        Expr::Ident(n) if n == "_" => Some(binder_term.clone()),
        Expr::Literal(Literal::Int(v)) => Some(Int::from_i64(ctx, *v)),
        Expr::UnaryOp { op: crate::ast::UnaryOp::Neg, operand } => {
            Some(encode_pred_int(ctx, operand, binder_term)?.unary_minus())
        }
        Expr::BinOp { op, left, right } => {
            let l = encode_pred_int(ctx, left, binder_term)?;
            let r = encode_pred_int(ctx, right, binder_term)?;
            match op {
                BinOp::Add => Some(&l + &r),
                BinOp::Sub => Some(&l - &r),
                BinOp::Mul => Some(&l * &r),
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_source;

    fn prove(src: &str) -> Vec<ProofResult> {
        prove_verify_bounds(&parse_source(src).expect("parse"))
    }

    #[test]
    fn smt_proves_nonneg_and_finds_counterexample() {
        // PROVEN: abs is always >= 0 for all i64 x.
        let r = prove(
            "@[verify(value >= 0)]\n\
             fn absish(x: i64) -> i64 { if x >= 0 { x } else { 0 - x } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "abs >= 0 must be proven, got {r:?}"
        );

        // COUNTEREXAMPLE: x - 1 is not always >= 0 (x=0 → -1).
        let r = prove(
            "@[verify(value >= 0)]\n\
             fn dec(x: i64) -> i64 { x - 1 }",
        );
        match r.as_slice() {
            [ProofResult::Counterexample { inputs, .. }] => {
                // The model's x must actually violate value >= 0 (i.e. x < 1).
                let x = inputs.iter().find(|(n, _)| n == "x").map(|(_, v)| *v).unwrap();
                assert!(x < 1, "counterexample x={x} must violate value>=0 (x-1 < 0)");
            }
            other => panic!("expected a counterexample, got {other:?}"),
        }
    }

    #[test]
    fn smt_proves_a_linear_bound() {
        // 2*x + 1 is always > 2*x → with bound `value > 0` for x>=0… use a
        // provable constant-ish: 3 is always >= 3.
        let r = prove(
            "@[verify(value >= 3)]\n\
             fn k() -> i64 { 3 }",
        );
        assert!(matches!(r.as_slice(), [ProofResult::Proven { .. }]), "got {r:?}");
    }

    #[test]
    fn smt_reports_unsupported_for_out_of_fragment() {
        // A call to another fn is outside the fragment → Unsupported, NOT a
        // false proof (the runtime gate still applies).
        let r = prove(
            "fn helper(x: i64) -> i64 { x }\n\
             @[verify(value >= 0)]\n\
             fn uses_call(x: i64) -> i64 { helper(x) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Unsupported { .. }]),
            "a call must be Unsupported, got {r:?}"
        );
    }

    #[test]
    fn smt_is_deterministic() {
        let src = "@[verify(value >= 0)]\nfn f(x: i64) -> i64 { if x >= 0 { x } else { 0 - x } }";
        assert_eq!(prove(src), prove(src), "same program ⇒ same proof result");
    }

    // ── R9: composite (conjunction) predicates ───────────────────────────────
    #[test]
    fn smt_proves_a_value_conjunction() {
        // value >= 0 && value <= 100 for a fn that always returns a clamped
        // constant 100 — both bounds hold for all inputs → PROVEN.
        let r = prove(
            "@[verify(value >= 0 && value <= 100)]\n\
             fn clamp(x: i64) -> i64 { if x >= 0 { 100 } else { 0 } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "a satisfied conjunction must be proven, got {r:?}"
        );
    }

    #[test]
    fn smt_conjunction_counterexample_on_one_atom() {
        // value >= 0 && value <= 5 for g(x)=x: the upper bound is violable
        // (x=6) → COUNTEREXAMPLE naming the full predicate.
        let r = prove(
            "@[verify(value >= 0 && value <= 5)]\n\
             fn g(x: i64) -> i64 { x }",
        );
        match r.as_slice() {
            [ProofResult::Counterexample { predicate, .. }] => {
                assert!(predicate.contains("&&"), "predicate should show the conjunction: {predicate}");
            }
            other => panic!("expected a counterexample, got {other:?}"),
        }
    }

    #[test]
    fn smt_confidence_conjunct_is_unsupported_not_false_proof() {
        // A conjunction mixing `value` with `confidence` (a runtime field, not a
        // static integer) is Unsupported — the runtime gate enforces it. Crucially
        // NOT a false proof.
        let r = prove(
            "@[verify(value >= 0 && confidence >= 0.8)]\n\
             fn h(x: i64) -> i64 { if x >= 0 { x } else { 0 - x } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Unsupported { .. }]),
            "a confidence conjunct must be Unsupported, got {r:?}"
        );
    }

    #[test]
    fn smt_disjunction_stays_unsupported() {
        // `||` is not decoded by the conjunction path → Unsupported (runtime
        // gate applies). We do not claim a false proof for disjunctions in v1.
        let r = prove(
            "@[verify(value >= 100 || value <= 0)]\n\
             fn d(x: i64) -> i64 { x }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Unsupported { .. }]),
            "a disjunction must be Unsupported in v1, got {r:?}"
        );
    }

    // ── R9 FLOAT fragment (Z3 Real) ──────────────────────────────────────────
    #[test]
    fn smt_proves_a_float_bound() {
        // PROVEN: x*x >= 0.0 for all real x (a square is non-negative).
        let r = prove(
            "@[verify(value >= 0.0)]\n\
             fn sq(x: f64) -> f64 { x * x }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "x*x >= 0.0 must be proven, got {r:?}"
        );
    }

    #[test]
    fn smt_finds_a_float_counterexample() {
        // COUNTEREXAMPLE: x - 0.5 is not always >= 0.0 (x=0 → -0.5).
        let r = prove(
            "@[verify(value >= 0.0)]\n\
             fn dec(x: f64) -> f64 { x - 0.5 }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "x-0.5 >= 0 must yield a counterexample, got {r:?}"
        );
    }

    #[test]
    fn smt_float_constant_bound_proven() {
        // A non-integer bound now routes to the float fragment instead of
        // Unsupported: 1.5 is always >= 1.0.
        let r = prove(
            "@[verify(value >= 1.0)]\n\
             fn k() -> f64 { 1.5 }",
        );
        assert!(matches!(r.as_slice(), [ProofResult::Proven { .. }]), "got {r:?}");
    }

    #[test]
    fn smt_float_ite_bound_proven() {
        // The float fragment includes `ite`: abs(x) >= 0.0 for all real x.
        let r = prove(
            "@[verify(value >= 0.0)]\n\
             fn absf(x: f64) -> f64 { if x >= 0.0 { x } else { 0.0 - x } }",
        );
        assert!(matches!(r.as_slice(), [ProofResult::Proven { .. }]), "got {r:?}");
    }

    // ── Phase 5 §4: refinement-RETURN proofs ─────────────────────────────────

    fn prove_refines(src: &str) -> Vec<ProofResult> {
        let program = parse_source(src).expect("parse");
        let mut refs = std::collections::HashMap::new();
        for item in &program.items {
            if let Item::RefineDef(r) = item {
                refs.insert(r.name.clone(), (*r.predicate).clone());
            }
        }
        prove_refinement_returns(&program, &refs)
    }

    #[test]
    fn smt_proves_refinement_return_and_finds_counterexample() {
        // PROVEN: abs returns a NonNeg for every input (the spec's abs_pos).
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\n\
             fn my_abs(n: i64) -> NonNeg { if n < 0 { 0 - n } else { n } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "abs -> NonNeg must be proven, got {r:?}"
        );

        // PROVEN: n*n is always >= 0 — a non-trivial property over all integers.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\nfn sq(n: i64) -> NonNeg { n * n }",
        );
        assert!(matches!(r.as_slice(), [ProofResult::Proven { .. }]), "n*n >= 0, got {r:?}");

        // COUNTEREXAMPLE: returning n unchanged does NOT guarantee > 0 (n=0).
        let r = prove_refines(
            "type Positive = i64 where _ > 0\nfn id(n: i64) -> Positive { n }",
        );
        match r.as_slice() {
            [ProofResult::Counterexample { inputs, .. }] => {
                assert!(inputs.iter().any(|(_, v)| *v <= 0), "counterexample must be <= 0: {inputs:?}");
            }
            other => panic!("expected a counterexample, got {other:?}"),
        }
    }

    #[test]
    fn smt_proves_f64_refinement_return_and_finds_counterexample() {
        // PROVEN: |x| is non-negative for every f64 input — over the reals, not just integers.
        let r = prove_refines(
            "type NonNegF = f64 where _ >= 0.0\n\
             fn absf(x: f64) -> NonNegF { if x < 0.0 { 0.0 - x } else { x } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "absf -> NonNegF must be proven over the reals, got {r:?}"
        );

        // COUNTEREXAMPLE: the identity does not guarantee strict positivity (x=0.0).
        let r = prove_refines(
            "type PosF = f64 where _ > 0.0\nfn idf(x: f64) -> PosF { x }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "idf -> PosF must yield a counterexample, got {r:?}"
        );
    }
}
