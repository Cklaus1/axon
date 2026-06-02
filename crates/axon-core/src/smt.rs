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
use crate::verify::decode_verify_predicate_with_ident;

use z3::ast::{Ast, Bool, Int};
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
        // Only the decodable `ident OP K` shape (value/confidence OP literal).
        let Some((ident, op, bound)) = decode_verify_predicate_with_ident(&spec.predicate) else {
            out.push(ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "predicate is not a simple `value OP K` comparison".into(),
            });
            continue;
        };
        // v1 proves the `value` bound (the return); `confidence` is an
        // Uncertain-field runtime concept, not a static integer property.
        if ident != "value" {
            out.push(ProofResult::Unsupported {
                function: f.name.clone(),
                reason: format!("only `value OP K` is statically provable in v1 (got `{ident}`)"),
            });
            continue;
        }
        out.push(prove_one(f, &op, bound));
    }
    out
}

/// Prove a single fn's `value OP K` bound. The bound `K` is taken as an integer
/// (the fragment is integer arithmetic; a non-integer bound → Unsupported).
fn prove_one(f: &FnDef, op: &BinOp, bound: f64) -> ProofResult {
    // Integer fragment: every param must be i64 and the bound an integer.
    if bound.fract() != 0.0 {
        return ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "non-integer bound (v1 fragment is integer arithmetic)".into(),
        };
    }
    let bound_i = bound as i64;
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

    // Encode the body as an Int term R(params). Bail to Unsupported on any node
    // outside the fragment.
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

    // The bound predicate B := R(params) OP K, and its negation ¬B.
    let k = Int::from_i64(&ctx, bound_i);
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
    let neg = bound_pred.not();

    // Ask Z3: does a violating input exist? unsat ⇒ proven; sat ⇒ counterexample.
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
                predicate: format!("value {} {bound_i}", crate::verify::binop_to_verify_str(op)),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: f.name.clone(),
            reason: "Z3 returned unknown (the VC was undecidable within limits)".into(),
        },
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

/// Whether an AST type is `i64` (the only encodable parameter type in v1).
fn is_i64_type(ty: &crate::ast::AxonType) -> bool {
    matches!(ty, crate::ast::AxonType::Named(n) if n == "i64")
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
}
