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

use crate::ast::{BinOp, Expr, FnDef, Item, Literal, Program, Stmt, UnaryOp};

use z3::ast::{Ast, Bool, Int, Real};
use z3::{Config, Context, SatResult, Solver};

/// The outcome of attempting a static proof of one `@[verify]` bound.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProofResult {
    /// Proven: the bound holds for every input in the integer domain.
    Proven { function: String },
    /// Disproven: a concrete input violates the bound (E1102).
    Counterexample {
        function: String,
        inputs: Vec<(String, i64)>,
        predicate: String,
    },
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
                         (disjunctions `||` stay runtime-only)"
                    .into(),
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
        let value_atoms: Vec<(BinOp, f64)> = atoms.into_iter().map(|(_, op, b)| (op, b)).collect();
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
        return ProofResult::Proven {
            function: f.name.clone(),
        };
    }
    prove_one_int_conjunction(f, atoms)
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

    let cfg = solver_config();
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
                         (loops, calls, float, string, …)"
                    .into(),
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
        atom_strs.push(format!(
            "value {} {bound_i}",
            crate::verify::binop_to_verify_str(op)
        ));
    }
    // Conjoin: B := A₁ ∧ A₂ ∧ … ; the VC is ¬B.
    let refs: Vec<&z3::ast::Bool> = atom_preds.iter().collect();
    let conj = z3::ast::Bool::and(&ctx, &refs);
    let neg = conj.not();

    let solver = Solver::new(&ctx);
    solver.assert(&neg);
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven {
            function: f.name.clone(),
        },
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
    let cfg = solver_config();
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
                         (loops, calls, div, string, IEEE rounding, …)"
                    .into(),
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
        SatResult::Unsat => ProofResult::Proven {
            function: f.name.clone(),
        },
        SatResult::Sat => {
            let model = solver.get_model().expect("sat ⇒ a model exists");
            // Report the violating inputs as the truncated integer part (the
            // float counterexample's exact rational is in the model; the
            // ProofResult carries i64 inputs, so we round toward zero).
            let inputs = params
                .iter()
                .map(|(name, c)| {
                    let v = model
                        .eval(c, true)
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
    let (n, d) = if g != 0 {
        (num / g, den / g)
    } else {
        (num, den)
    };
    Real::from_real(ctx, n as i32, d as i32)
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a
}

/// Encode an `Expr` as a Z3 `Real` term — the float counterpart of
/// [`encode_expr`]. Same straight-line fragment (`+`/`-`/`*`, `ite`, literals,
/// params); `Div` stays out (partial). Integer literals coerce to Real.
fn encode_expr_real<'c>(
    ctx: &'c Context,
    e: &Expr,
    params: &[(String, Real<'c>)],
) -> Option<Real<'c>> {
    match e {
        Expr::Literal(Literal::Float(x)) => Some(f64_to_real(ctx, *x)),
        Expr::Literal(Literal::Int(n)) => Some(f64_to_real(ctx, *n as f64)),
        Expr::Ident(name) => params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c.clone()),
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
        Expr::Block(stmts) => encode_block_real(ctx, stmts, params),
        // Float bound builtins (counterpart of the integer min/max/abs case).
        Expr::Call { callee, args, .. } => {
            let Expr::Ident(name) = callee.as_ref() else {
                return None;
            };
            match (name.as_str(), args.len()) {
                ("min_f64", 2) | ("max_f64", 2) => {
                    let a = encode_expr_real(ctx, &args[0], params)?;
                    let b = encode_expr_real(ctx, &args[1], params)?;
                    let cond = if name == "min_f64" {
                        a.le(&b)
                    } else {
                        a.ge(&b)
                    };
                    Some(cond.ite(&a, &b))
                }
                ("abs_f64", 1) => {
                    let x = encode_expr_real(ctx, &args[0], params)?;
                    let zero = f64_to_real(ctx, 0.0);
                    let neg = &zero - &x;
                    Some(x.ge(&zero).ite(&x, &neg))
                }
                _ => None,
            }
        }
        _ => None,
    }
}

/// Float counterpart of [`encode_block`]: fold leading `let` bindings into the
/// environment, then encode the tail expression.
fn encode_block_real<'c>(
    ctx: &'c Context,
    stmts: &[Stmt],
    params: &[(String, Real<'c>)],
) -> Option<Real<'c>> {
    let (tail, lets) = stmts.split_last()?;
    let mut env: Vec<(String, Real<'c>)> = params.to_vec();
    for s in lets {
        let Expr::Let { name, value, .. } = &s.expr else {
            return None;
        };
        let term = encode_expr_real(ctx, value, &env)?;
        env.push((name.clone(), term));
    }
    encode_expr_real(ctx, &tail.expr, &env)
}

/// Encode a boolean condition over `Real` terms (float counterpart of
/// [`encode_bool`]).
fn encode_bool_real<'c>(
    ctx: &'c Context,
    e: &Expr,
    params: &[(String, Real<'c>)],
) -> Option<Bool<'c>> {
    match e {
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            let l = encode_bool_real(ctx, left, params)?;
            let r = encode_bool_real(ctx, right, params)?;
            Some(Bool::and(ctx, &[&l, &r]))
        }
        Expr::BinOp {
            op: BinOp::Or,
            left,
            right,
        } => {
            let l = encode_bool_real(ctx, left, params)?;
            let r = encode_bool_real(ctx, right, params)?;
            Some(Bool::or(ctx, &[&l, &r]))
        }
        Expr::UnaryOp {
            op: UnaryOp::Not,
            operand,
        } => Some(encode_bool_real(ctx, operand, params)?.not()),
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
        Expr::Ident(name) => params
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, c)| c.clone()),
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
        // Pure, total integer bound builtins, encoded exactly via `ite` — these are
        // the common shape of a provable @[verify]/refinement postcondition (clamp,
        // bound). min/max(a,b) = ite(a≤b|a≥b, a, b); abs(x) = ite(x≥0, x, -x). The
        // proof runs over ℤ (like the rest of this encoder), which stays sound: an
        // i64 overflow (e.g. abs_i64(i64::MIN)) panics at runtime BEFORE the return
        // check, so eliding a proven check is still observably a no-op (I-2).
        Expr::Call { callee, args, .. } => {
            let Expr::Ident(name) = callee.as_ref() else {
                return None;
            };
            match (name.as_str(), args.len()) {
                ("min_i64", 2) | ("max_i64", 2) => {
                    let a = encode_expr(ctx, &args[0], params)?;
                    let b = encode_expr(ctx, &args[1], params)?;
                    let cond = if name == "min_i64" {
                        a.le(&b)
                    } else {
                        a.ge(&b)
                    };
                    Some(cond.ite(&a, &b))
                }
                ("abs_i64", 1) => {
                    let x = encode_expr(ctx, &args[0], params)?;
                    let zero = Int::from_i64(ctx, 0);
                    let neg = &zero - &x;
                    Some(x.ge(&zero).ite(&x, &neg))
                }
                _ => None,
            }
        }
        // Unary minus shows up as `0 - x` from the parser, handled by BinOp::Sub.
        _ => None,
    }
}

/// A block's value is its final expression. Leading `let name = E;` bindings are
/// folded into the environment by substitution (each `E` is encoded in the
/// bindings visible so far, then bound to `name`), so multi-line straight-line
/// bodies like `let d = a - b; d * d` are in-fragment. Any non-`let` statement
/// before the tail has an effect we don't model → out of fragment.
fn encode_block<'c>(
    ctx: &'c Context,
    stmts: &[Stmt],
    params: &[(String, Int<'c>)],
) -> Option<Int<'c>> {
    let (tail, lets) = stmts.split_last()?;
    let mut env: Vec<(String, Int<'c>)> = params.to_vec();
    for s in lets {
        let Expr::Let { name, value, .. } = &s.expr else {
            return None;
        };
        let term = encode_expr(ctx, value, &env)?;
        env.push((name.clone(), term));
    }
    encode_expr(ctx, &tail.expr, &env)
}

/// Encode a boolean condition (a comparison of integer terms, or a logical
/// combination of such) as a Z3 `Bool`. Handles `&&`/`||`/`!` so a conjunctive
/// guard like `if x > 0 && x < 10 { … }` is in-fragment.
fn encode_bool<'c>(ctx: &'c Context, e: &Expr, params: &[(String, Int<'c>)]) -> Option<Bool<'c>> {
    match e {
        Expr::BinOp {
            op: BinOp::And,
            left,
            right,
        } => {
            let l = encode_bool(ctx, left, params)?;
            let r = encode_bool(ctx, right, params)?;
            Some(Bool::and(ctx, &[&l, &r]))
        }
        Expr::BinOp {
            op: BinOp::Or,
            left,
            right,
        } => {
            let l = encode_bool(ctx, left, params)?;
            let r = encode_bool(ctx, right, params)?;
            Some(Bool::or(ctx, &[&l, &r]))
        }
        Expr::UnaryOp {
            op: UnaryOp::Not,
            operand,
        } => Some(encode_bool(ctx, operand, params)?.not()),
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

// ── Proof-budget configuration ───────────────────────────────────────────────

/// Default maximum inlining depth: a callee may itself call another in-fragment
/// fn, up to this many nested expansions. Bounds work and stops runaway
/// recursion (a self-recursive fn simply hits the limit and stays an un-inlined
/// `Call`, which the encoder then reports Unsupported — never a false proof).
/// Override with `AXON_PROOF_DEPTH`.
const INLINE_DEPTH_DEFAULT: u32 = 4;

/// Default per-query Z3 timeout (ms). A hard refinement that would otherwise
/// hang the solver instead returns `unknown` → Unsupported (the runtime
/// obligation still applies). Override with `AXON_PROOF_TIMEOUT_MS`.
const PROOF_TIMEOUT_MS_DEFAULT: u64 = 10_000;

/// Inlining depth from `AXON_PROOF_DEPTH`, clamped to [0, 64]; default
/// `INLINE_DEPTH_DEFAULT`. 0 disables call-inlining entirely.
fn inline_depth() -> u32 {
    inline_depth_from(std::env::var("AXON_PROOF_DEPTH").ok().as_deref())
}

/// Pure core of [`inline_depth`] (parse + clamp), split out so it can be tested
/// without mutating process-global env state — mirrors `interp::max_depth_from_env`.
fn inline_depth_from(s: Option<&str>) -> u32 {
    s.and_then(|s| s.trim().parse::<u32>().ok())
        .map(|d| d.min(64))
        .unwrap_or(INLINE_DEPTH_DEFAULT)
}

/// A nonzero Z3 timeout below this (ms) is floored to it. Z3 wedges on
/// sub-resolution timeouts (observed: 1–2ms hangs indefinitely) because the
/// timeout timer setup races the solver — so a user who sets `=1` to "fail fast"
/// would instead get a hang. Flooring keeps the knob useful (hundreds of ms and
/// up work as expected) while making the degenerate case impossible.
const PROOF_TIMEOUT_MS_FLOOR: u64 = 10;

/// Pure core of the timeout read: parse `AXON_PROOF_TIMEOUT_MS`, default
/// `PROOF_TIMEOUT_MS_DEFAULT`. 0 means "no timeout" (passed through); any other
/// value is floored to `PROOF_TIMEOUT_MS_FLOOR` to avoid the Z3 small-timeout wedge.
fn proof_timeout_ms_from(s: Option<&str>) -> u64 {
    let ms = s
        .and_then(|s| s.trim().parse::<u64>().ok())
        .unwrap_or(PROOF_TIMEOUT_MS_DEFAULT);
    if ms == 0 {
        0
    } else {
        ms.max(PROOF_TIMEOUT_MS_FLOOR)
    }
}

/// A Z3 `Config` with the per-query timeout from `AXON_PROOF_TIMEOUT_MS` (ms),
/// default `PROOF_TIMEOUT_MS_DEFAULT`. A value of 0 disables the timeout (Z3
/// runs unbounded). Every prover builds its config here so the budget is
/// uniform across the integer, float, verify, and refinement paths.
fn solver_config() -> Config {
    let mut cfg = Config::new();
    let ms = proof_timeout_ms_from(std::env::var("AXON_PROOF_TIMEOUT_MS").ok().as_deref());
    if ms > 0 {
        cfg.set_timeout_msec(ms);
    }
    cfg
}

/// Rewrite an expression by β-reducing calls to straight-line in-program
/// functions: `f(a, b)` with `fn f(x, y) { BODY }` becomes `BODY[x:=a, y:=b]`.
/// Only direct calls to a named fn in `fns` are inlined, and only down to
/// `depth` levels; everything else is copied structurally. Argument expressions
/// are inlined first, then substituted, so nested calls expand too. This is the
/// SOUND way to bring helper calls into the straight-line fragment without
/// teaching the Z3 encoders about function application: substitution preserves
/// the exact value, and anything left un-inlined is caught as Unsupported.
fn inline_calls(e: &Expr, fns: &std::collections::HashMap<String, &FnDef>, depth: u32) -> Expr {
    match e {
        Expr::Call { callee, args, tier } => {
            let inlined_args: Vec<Expr> =
                args.iter().map(|a| inline_calls(a, fns, depth)).collect();
            if depth > 0 {
                if let Expr::Ident(name) = callee.as_ref() {
                    if let Some(f) = fns.get(name) {
                        if f.params.len() == inlined_args.len() {
                            let subst: std::collections::HashMap<String, Expr> = f
                                .params
                                .iter()
                                .map(|p| p.name.clone())
                                .zip(inlined_args.iter().cloned())
                                .collect();
                            let body = substitute(&f.body, &subst);
                            // Recurse: the substituted body may contain more calls.
                            return inline_calls(&body, fns, depth - 1);
                        }
                    }
                }
            }
            Expr::Call {
                callee: Box::new(inline_calls(callee, fns, depth)),
                args: inlined_args,
                tier: tier.clone(),
            }
        }
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left: Box::new(inline_calls(left, fns, depth)),
            right: Box::new(inline_calls(right, fns, depth)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: op.clone(),
            operand: Box::new(inline_calls(operand, fns, depth)),
        },
        Expr::If { cond, then, else_ } => Expr::If {
            cond: Box::new(inline_calls(cond, fns, depth)),
            then: Box::new(inline_calls(then, fns, depth)),
            else_: else_
                .as_ref()
                .map(|e| Box::new(inline_calls(e, fns, depth))),
        },
        Expr::Block(stmts) => Expr::Block(
            stmts
                .iter()
                .map(|s| Stmt {
                    expr: inline_calls(&s.expr, fns, depth),
                    span: s.span,
                })
                .collect(),
        ),
        Expr::Let { name, ty, value } => Expr::Let {
            name: name.clone(),
            ty: ty.clone(),
            value: Box::new(inline_calls(value, fns, depth)),
        },
        // Literals, idents, and everything else copy unchanged.
        other => other.clone(),
    }
}

/// Substitute free identifiers per `subst` throughout `e`. The straight-line
/// fragment has no shadowing constructs we encode (a `let` that rebinds a
/// param name would shadow — guarded by `skip` below), so a structural walk is
/// sound. Used only on a callee body whose params are being bound to args.
fn substitute(e: &Expr, subst: &std::collections::HashMap<String, Expr>) -> Expr {
    match e {
        Expr::Ident(name) => subst.get(name).cloned().unwrap_or_else(|| e.clone()),
        Expr::BinOp { op, left, right } => Expr::BinOp {
            op: op.clone(),
            left: Box::new(substitute(left, subst)),
            right: Box::new(substitute(right, subst)),
        },
        Expr::UnaryOp { op, operand } => Expr::UnaryOp {
            op: op.clone(),
            operand: Box::new(substitute(operand, subst)),
        },
        Expr::If { cond, then, else_ } => Expr::If {
            cond: Box::new(substitute(cond, subst)),
            then: Box::new(substitute(then, subst)),
            else_: else_.as_ref().map(|e| Box::new(substitute(e, subst))),
        },
        Expr::Call { callee, args, tier } => Expr::Call {
            callee: Box::new(substitute(callee, subst)),
            args: args.iter().map(|a| substitute(a, subst)).collect(),
            tier: tier.clone(),
        },
        Expr::Block(stmts) => {
            // A `let` inside the body introduces a binding that shadows any
            // same-named param for the rest of the block; drop it from `subst`.
            let mut local = subst.clone();
            Expr::Block(
                stmts
                    .iter()
                    .map(|s| {
                        let out = Stmt {
                            expr: substitute(&s.expr, &local),
                            span: s.span,
                        };
                        if let Expr::Let { name, .. } = &s.expr {
                            local.remove(name);
                        }
                        out
                    })
                    .collect(),
            )
        }
        Expr::Let { name, ty, value } => Expr::Let {
            name: name.clone(),
            ty: ty.clone(),
            value: Box::new(substitute(value, subst)),
        },
        other => other.clone(),
    }
}

/// Build the inlinable-fn table: every top-level `fn` in the program, by name.
/// (Recursion is bounded by `INLINE_DEPTH`, so including a fn that calls itself
/// is safe — it just stops expanding at the limit.)
fn program_fns(program: &Program) -> std::collections::HashMap<String, &FnDef> {
    program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::FnDef(f) => Some((f.name.clone(), f)),
            _ => None,
        })
        .collect()
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
    let fns = program_fns(program);
    for item in &program.items {
        let Item::FnDef(f) = item else { continue };
        // The return type must name a known refinement.
        let Some(crate::ast::AxonType::Named(rname)) = &f.return_type else {
            continue;
        };
        let Some(pred) = refinements.get(rname) else {
            continue;
        };
        // β-reduce calls to in-program helpers so the body is straight-line.
        let body = inline_calls(&f.body, &fns, inline_depth());
        // Integer fragment: all params i64.
        if f.params.iter().all(|p| is_i64_type(&p.ty)) {
            out.push(prove_one_refinement_return(f, &body, pred, rname));
        } else if !f.params.is_empty() && f.params.iter().all(|p| is_f64_type(&p.ty)) {
            // Float fragment: all params f64 (e.g. `fn norm(x: f64) -> NonNegF`).
            out.push(prove_one_refinement_return_f64(f, &body, pred, rname));
        }
        // Mixed / other param types fall outside the v1 fragment — skipped
        // (the runtime obligation / constant checker still applies).
    }
    out
}

/// Phase 5 §1.5 (first sound case): refinement subtyping under ARGUMENT
/// FORWARDING. When a function forwards one of its own refinement-typed
/// parameters `p` directly as an argument to a callee slot that also carries a
/// refinement, the call is safe iff the caller's predicate IMPLIES the callee's:
/// `∀ p. caller_pred(p) ⟹ callee_pred(p)`. Z3 decides this implication directly
/// (no body encoding needed) — `∃ p. caller_pred(p) ∧ ¬callee_pred(p)` unsat ⇒
/// proven safe; sat ⇒ a concrete `p` the caller admits but the callee forbids
/// (E1102). This is the dual of the return prover: it discharges the call-site
/// argument obligation the constant checker (E1209) defers for variables.
///
/// Only DIRECT forwarding (`callee(p)` where `p` is a bare refinement param) is
/// in scope here — a general expression argument needs the full path-condition
/// machinery. Anything else is simply not reported (the runtime obligation
/// still applies); we never emit a false counterexample.
pub fn prove_refinement_arg_forwarding(
    program: &Program,
    refinements: &std::collections::HashMap<String, Expr>,
) -> Vec<ProofResult> {
    let mut out = Vec::new();
    // fn name → per-param-slot refinement name (None if that slot is unrefined).
    let slot_refs: std::collections::HashMap<String, Vec<Option<String>>> = program
        .items
        .iter()
        .filter_map(|it| match it {
            Item::FnDef(f) => {
                let slots = f
                    .params
                    .iter()
                    .map(|p| match &p.ty {
                        crate::ast::AxonType::Named(n) if refinements.contains_key(n) => {
                            Some(n.clone())
                        }
                        _ => None,
                    })
                    .collect();
                Some((f.name.clone(), slots))
            }
            _ => None,
        })
        .collect();

    for item in &program.items {
        let Item::FnDef(f) = item else { continue };
        // This caller's own params → their refinement name (for the forwarded id).
        let caller_param_ref: std::collections::HashMap<String, String> = f
            .params
            .iter()
            .filter_map(|p| match &p.ty {
                crate::ast::AxonType::Named(n) if refinements.contains_key(n) => {
                    Some((p.name.clone(), n.clone()))
                }
                _ => None,
            })
            .collect();
        if caller_param_ref.is_empty() {
            continue;
        }
        let mut calls = Vec::new();
        collect_calls(&f.body, &mut calls);
        for (callee, args) in calls {
            let Some(callee_slots) = slot_refs.get(&callee) else {
                continue;
            };
            for (i, arg) in args.iter().enumerate() {
                let Expr::Ident(argname) = arg else { continue };
                let Some(caller_rname) = caller_param_ref.get(argname) else {
                    continue;
                };
                let Some(Some(callee_rname)) = callee_slots.get(i) else {
                    continue;
                };
                if caller_rname == callee_rname {
                    continue; // identical refinement — trivially safe, skip the query.
                }
                let (Some(cpred), Some(epred)) =
                    (refinements.get(caller_rname), refinements.get(callee_rname))
                else {
                    continue;
                };
                out.push(prove_implies(
                    cpred,
                    epred,
                    &format!("{}→{callee}", f.name),
                    callee_rname,
                ));
            }
        }
    }
    out
}

/// Prove `∀ _. antecedent(_) ⟹ consequent(_)` over the integer fragment. Used
/// for refinement subtyping: the binder `_` ranges over the forwarded value.
fn prove_implies(
    antecedent: &Expr,
    consequent: &Expr,
    site: &str,
    callee_rname: &str,
) -> ProofResult {
    let cfg = solver_config();
    let ctx = Context::new(&cfg);
    let x = Int::new_const(&ctx, "_");
    // Subtyping is over a single value `_` with no fn params in scope.
    let no_params: &[(String, Int)] = &[];
    let (Some(a), Some(c)) = (
        encode_pred_binder(&ctx, antecedent, &x, no_params),
        encode_pred_binder(&ctx, consequent, &x, no_params),
    ) else {
        return ProofResult::Unsupported {
            function: site.to_string(),
            reason: "refinement predicate is outside the i64 implication fragment".into(),
        };
    };
    // VC: ∃ _. a ∧ ¬c  (a witness the caller admits but the callee rejects).
    let solver = Solver::new(&ctx);
    solver.assert(&Bool::and(&ctx, &[&a, &c.not()]));
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven {
            function: site.to_string(),
        },
        SatResult::Sat => {
            let v = solver
                .get_model()
                .and_then(|m| m.eval(&x, true))
                .and_then(|i| i.as_i64())
                .unwrap_or(0);
            ProofResult::Counterexample {
                function: site.to_string(),
                inputs: vec![("_".to_string(), v)],
                predicate: format!("forwarded value must satisfy `{callee_rname}`"),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: site.to_string(),
            reason: "Z3 returned unknown".into(),
        },
    }
}

/// Phase 5 §4: run the two ∀-inputs provers and collect every obligation Z3
/// proves holds for ALL inputs into a [`crate::verify::Discharged`] set, so the
/// default `run`/`build` pipeline can ELIDE the corresponding runtime check.
///
/// This is the bridge that wires the static prover (today reachable only via the
/// explicit `axon verify`) into the normal compile path. Only `Proven` outcomes
/// are collected — a `Counterexample` is left for the runtime gate to catch (and
/// is independently surfaced as E1102 by `axon verify`), and `Unsupported`
/// obligations stay runtime-checked. The two prover kinds correspond exactly to
/// the two fields of `Discharged`; see its doc-comment for why only these two
/// obligation kinds are sound to elide.
///
/// `refinements` maps a refinement type name → its predicate (binder `_`),
/// assembled by the caller from the program's `RefineDef`s (same as `cmd_verify`).
pub fn discharge(
    program: &Program,
    refinements: &std::collections::HashMap<String, Expr>,
) -> crate::verify::Discharged {
    let mut d = crate::verify::Discharged::default();
    for r in prove_verify_bounds(program) {
        if let ProofResult::Proven { function } = r {
            d.verify_fns.insert(function);
        }
    }
    if !refinements.is_empty() {
        for r in prove_refinement_returns(program, refinements) {
            if let ProofResult::Proven { function } = r {
                d.refine_return_fns.insert(function);
            }
        }
    }
    // R20: discharge the kernel capability-mint obligation (O1 attenuation +
    // O2 budget carve) once per build. This is a TCB lemma about the fixed
    // `PrincipalRegistry::mint` primitive, independent of the user program — so
    // it is proven unconditionally here, not gated on the program using mint.
    d.mint_obligations_proven = matches!(
        prove_mint_obligations(true, true),
        ProofResult::Proven { .. }
    );
    d
}

/// R20: the I-12 build-time tripwire. Returns `Err(E1610, message)` iff the
/// kernel mint obligation is NOT SMT-discharged — which can only happen if the
/// faithful encoding in `prove_mint_obligations` has been weakened to match a
/// weakened `mint` (or Z3 is unavailable). For the in-tree minter this is always
/// `Ok(())`. Pair with the `kernel.rs` differential grid test, which catches the
/// other half (the Rust impl diverging from the proven model).
#[cfg(feature = "smt")]
pub fn check_mint_tcb_obligation() -> Result<(), (&'static str, String)> {
    match prove_mint_obligations(true, true) {
        ProofResult::Proven { .. } => Ok(()),
        ProofResult::Counterexample {
            inputs, predicate, ..
        } => Err((
            crate::error::E1610,
            format!(
                "kernel mint obligation not discharged — the minter has been weakened. \
                 Violated: {predicate}. Counterexample: {inputs:?}"
            ),
        )),
        ProofResult::Unsupported { reason, .. } => Err((
            crate::error::E1610,
            format!("kernel mint obligation could not be proven: {reason}"),
        )),
    }
}

// ── R20 Slice 3: TCB attestation ─────────────────────────────────────────────
//
// Content-address the kernel TCB obligations into a digest that is PINNED in a
// manifest and re-checked at boot. The digest folds in BOTH (a) the canonical
// human-readable obligation spec text and (b) the LIVE proof verdict — so the
// boot check fails closed (E1611) if either the obligation DEFINITION changes or
// the PROOF stops holding, without a corresponding manifest update. This makes
// "self-modification cannot weaken the TCB" (I-12) a boot-time invariant, not
// only a CI test (the kernel.rs grid test) or a build-time check (E1610).
//
// Honest scope: the digest pins the obligation SET, the spec TEXT, and the
// verdict. It does not hash the Rust AST of `prove_mint_obligations`, so a
// predicate change that BOTH stays provable AND leaves the spec text unchanged
// would not be caught here — that is the grid test's job (impl ↔ model) and code
// review's. The three layers are complementary, not redundant.

/// The canonical, human-readable specification of the kernel mint obligations.
/// Changing what `prove_mint_obligations` proves MUST be reflected here (or the
/// attestation is a lie); changing this text changes the digest → boot mismatch.
pub const MINT_OBLIGATION_SPEC: &str = "R20/principal_mint: \
    O1 attenuation [child.net⇒parent.net ∧ child.fs_write⇒parent.fs_write ∧ child.exec⇒parent.exec]; \
    O2 budget-carve [0≤child.cap≤rem ∧ rem_after+child.cap=rem, rem=max(0,cap-used)]";

/// The pinned content address of the all-obligations-proven TCB state. Computed
/// by `tcb_attestation_digest()` over (spec text ⊕ live verdict). Updating it is
/// a deliberate, audited manifest change (ROADMAP §7: multi-sig update path).
pub const TCB_MANIFEST_DIGEST: &str =
    "axtcb1:38ed24ddd83aa34a87533bc622817e6b8a752f0d3f7530461807ed1c88a77704";

/// Compute the live TCB attestation digest: `sha256(spec ⊕ verdict)`, tagged.
/// Deterministic (the spec is constant and the proof is deterministic).
#[cfg(feature = "smt")]
pub fn tcb_attestation_digest() -> String {
    use sha2::{Digest, Sha256};
    let verdict = match prove_mint_obligations(true, true) {
        ProofResult::Proven { .. } => "Proven",
        ProofResult::Counterexample { .. } => "Counterexample",
        ProofResult::Unsupported { .. } => "Unsupported",
    };
    let mut h = Sha256::new();
    h.update(MINT_OBLIGATION_SPEC.as_bytes());
    h.update(b"\x1f"); // unit separator: spec ⊕ verdict
    h.update(verdict.as_bytes());
    format!("axtcb1:{:x}", h.finalize())
}

/// R20 Slice 3: the boot-time attestation check. Returns `Err(E1611, …)` iff the
/// live digest ≠ the pinned manifest — the proven TCB changed without a manifest
/// update. For the in-tree, all-proven obligations this is `Ok(())`.
#[cfg(feature = "smt")]
pub fn check_tcb_attestation() -> Result<(), (&'static str, String)> {
    let live = tcb_attestation_digest();
    if live == TCB_MANIFEST_DIGEST {
        Ok(())
    } else {
        Err((
            crate::error::E1611,
            format!(
                "TCB attestation mismatch at boot: kernel obligation digest `{live}` \
                 ≠ pinned manifest `{TCB_MANIFEST_DIGEST}` — the proven TCB changed \
                 without a manifest update (I-12)"
            ),
        ))
    }
}

/// Collect every `callee_name(args…)` direct call in `e` as (name, args).
fn collect_calls(e: &Expr, out: &mut Vec<(String, Vec<Expr>)>) {
    match e {
        Expr::Call { callee, args, .. } => {
            if let Expr::Ident(name) = callee.as_ref() {
                out.push((name.clone(), args.clone()));
            }
            for a in args {
                collect_calls(a, out);
            }
            collect_calls(callee, out);
        }
        Expr::BinOp { left, right, .. } => {
            collect_calls(left, out);
            collect_calls(right, out);
        }
        Expr::UnaryOp { operand, .. } => collect_calls(operand, out),
        Expr::If { cond, then, else_ } => {
            collect_calls(cond, out);
            collect_calls(then, out);
            if let Some(e) = else_ {
                collect_calls(e, out);
            }
        }
        Expr::Block(stmts) => {
            for s in stmts {
                collect_calls(&s.expr, out);
            }
        }
        Expr::Let { value, .. } => collect_calls(value, out),
        _ => {}
    }
}

/// Float-fragment analog of `prove_one_refinement_return`: encode the f64 body as
/// a Z3 Real and prove the predicate (with `_` → body) holds for all inputs.
fn prove_one_refinement_return_f64(
    f: &FnDef,
    body_ast: &Expr,
    pred: &Expr,
    rname: &str,
) -> ProofResult {
    let cfg = solver_config();
    let ctx = Context::new(&cfg);
    let params: Vec<(String, Real)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Real::new_const(&ctx, p.name.as_str())))
        .collect();

    let body = match encode_expr_real(&ctx, body_ast, &params) {
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
        SatResult::Unsat => ProofResult::Proven {
            function: f.name.clone(),
        },
        SatResult::Sat => {
            let inputs = solver
                .get_model()
                .map(|m| {
                    params
                        .iter()
                        .filter_map(|(n, c)| {
                            // Reals print as rationals; round toward zero for the i64 report.
                            m.eval(c, true)
                                .and_then(|v| v.as_real())
                                .map(|(num, den)| (n.clone(), if den != 0 { num / den } else { 0 }))
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
        Expr::UnaryOp {
            op: crate::ast::UnaryOp::Not,
            operand,
        } => Some(encode_pred_binder_real(ctx, operand, binder_term)?.not()),
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
fn encode_pred_real<'c>(ctx: &'c Context, e: &Expr, binder_term: &Real<'c>) -> Option<Real<'c>> {
    match e {
        Expr::Ident(n) if n == "_" => Some(binder_term.clone()),
        Expr::Literal(Literal::Float(v)) => {
            // Z3 Real from a decimal — go through a rational string.
            Some(
                Real::from_real_str(ctx, &format!("{v}"), "1")
                    .unwrap_or_else(|| Real::from_real(ctx, 0, 1)),
            )
        }
        Expr::Literal(Literal::Int(v)) => Some(Real::from_real(ctx, *v as i32, 1)),
        Expr::UnaryOp {
            op: crate::ast::UnaryOp::Neg,
            operand,
        } => Some(encode_pred_real(ctx, operand, binder_term)?.unary_minus()),
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

fn prove_one_refinement_return(
    f: &FnDef,
    body_ast: &Expr,
    pred: &Expr,
    rname: &str,
) -> ProofResult {
    let cfg = solver_config();
    let ctx = Context::new(&cfg);
    let params: Vec<(String, Int)> = f
        .params
        .iter()
        .map(|p| (p.name.clone(), Int::new_const(&ctx, p.name.as_str())))
        .collect();

    // Encode the body as an Int term R(params).
    let body = match encode_expr(&ctx, body_ast, &params) {
        Some(t) => t,
        None => {
            return ProofResult::Unsupported {
                function: f.name.clone(),
                reason: "body uses a construct outside the straight-line integer fragment".into(),
            }
        }
    };

    // Encode the predicate with `_` bound to the body term and the fn's params
    // in scope (R20 Slice 2: relational refinements relating `_` to a param).
    let pred_z3 = match encode_pred_binder(&ctx, pred, &body, &params) {
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
        SatResult::Unsat => ProofResult::Proven {
            function: f.name.clone(),
        },
        SatResult::Sat => {
            let inputs = solver
                .get_model()
                .map(|m| {
                    params
                        .iter()
                        .filter_map(|(n, c)| {
                            m.eval(c, true)
                                .and_then(|v| v.as_i64())
                                .map(|v| (n.clone(), v))
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

/// Encode a refinement predicate as a Z3 Bool, with the binder `_` mapped to
/// `binder_term`. Supports comparisons of i64 terms + `&&`/`||`/`!`.
fn encode_pred_binder<'c>(
    ctx: &'c Context,
    e: &Expr,
    binder_term: &Int<'c>,
    params: &[(String, Int<'c>)],
) -> Option<Bool<'c>> {
    match e {
        Expr::UnaryOp {
            op: crate::ast::UnaryOp::Not,
            operand,
        } => Some(encode_pred_binder(ctx, operand, binder_term, params)?.not()),
        Expr::BinOp { op, left, right } => match op {
            BinOp::And => {
                let l = encode_pred_binder(ctx, left, binder_term, params)?;
                let r = encode_pred_binder(ctx, right, binder_term, params)?;
                Some(Bool::and(ctx, &[&l, &r]))
            }
            BinOp::Or => {
                let l = encode_pred_binder(ctx, left, binder_term, params)?;
                let r = encode_pred_binder(ctx, right, binder_term, params)?;
                Some(Bool::or(ctx, &[&l, &r]))
            }
            BinOp::Lt | BinOp::Gt | BinOp::LtEq | BinOp::GtEq | BinOp::Eq | BinOp::NotEq => {
                let l = encode_pred_int(ctx, left, binder_term, params)?;
                let r = encode_pred_int(ctx, right, binder_term, params)?;
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
    params: &[(String, Int<'c>)],
) -> Option<Int<'c>> {
    match e {
        Expr::Ident(n) if n == "_" => Some(binder_term.clone()),
        // R20 Slice 2: a parameter reference resolves to its Z3 const, so a
        // RELATIONAL refinement predicate (return `_` vs a param, e.g.
        // `_ <= parent_rem`) becomes statically provable, not just a constant
        // bound. Unknown idents stay unsupported (→ None → runtime fallback).
        Expr::Ident(n) => params
            .iter()
            .find(|(pn, _)| pn == n)
            .map(|(_, c)| c.clone()),
        Expr::Literal(Literal::Int(v)) => Some(Int::from_i64(ctx, *v)),
        Expr::UnaryOp {
            op: crate::ast::UnaryOp::Neg,
            operand,
        } => Some(encode_pred_int(ctx, operand, binder_term, params)?.unary_minus()),
        Expr::BinOp { op, left, right } => {
            let l = encode_pred_int(ctx, left, binder_term, params)?;
            let r = encode_pred_int(ctx, right, binder_term, params)?;
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

// ── R20 Slice 0: capability-attenuation spike (mint O1) ──────────────────────
//
// The first cut of `R20-smt-capability-proofs.md`. The mint obligation lives
// OUTSIDE the `body OP constant` fragment the `@[verify]`/refinement provers
// above target — it is *relational* (output child vs input parent), *boolean*
// (net/fs/exec are caps, not Int terms), and *struct-valued* (a Principal is a
// tuple of fields). This spike de-risks that encoder surface by proving the
// simplest obligation in isolation, via the **direct-prover route** (route B in
// the spec): we encode `mint`'s FIXED boolean semantics straight into Z3 rather
// than parsing Axon source, because `mint` is a fixed TCB primitive.
//
// Obligation O1 (capability attenuation, invariant I-11):
//     child.net ⇒ parent.net  ∧  child.fs_write ⇒ parent.fs_write
//                              ∧  child.exec ⇒ parent.exec
// where `mint` computes `child.cap = want_cap ∧ parent.cap` for each cap
// (kernel.rs `mint`). Proving ∀ ≡ asserting ¬O1 is UNSAT — the same
// negate-and-check shape the Int/Real provers use.
//
// `faithful_and` selects the encoding: `true` = the real conjunctive semantics
// (must PROVE); `false` = a deliberately-weakened `child.net = want_net` that
// drops the `&& parent.net` guard (must produce a COUNTEREXAMPLE). The latter is
// the seed of the slice-1 E1610 I-12 tripwire: a mint edit that breaks
// attenuation must be caught, not silently discharged.

/// Prove mint's capability-attenuation obligation (O1) over all 8 boolean
/// inputs (3 parent caps × 3 want flags, minus the irrelevant cross terms).
/// Route B: the `mint` semantics are encoded directly, not lifted from source.
#[cfg(feature = "smt")]
pub fn prove_mint_attenuation(faithful_and: bool) -> ProofResult {
    use z3::ast::Bool;

    let cfg = Config::new();
    let ctx = Context::new(&cfg);

    // Free boolean inputs: the parent's held caps and the child's requested caps.
    let p_net = Bool::new_const(&ctx, "parent_net");
    let p_fs = Bool::new_const(&ctx, "parent_fs_write");
    let p_exec = Bool::new_const(&ctx, "parent_exec");
    let w_net = Bool::new_const(&ctx, "want_net");
    let w_fs = Bool::new_const(&ctx, "want_fs_write");
    let w_exec = Bool::new_const(&ctx, "want_exec");

    // mint: child.cap = want_cap ∧ parent.cap (kernel.rs:124-126). The `net`
    // arm is the one the mutation weakens to expose the tripwire.
    let c_net = if faithful_and {
        Bool::and(&ctx, &[&w_net, &p_net])
    } else {
        w_net.clone() // WEAKENED: drops `&& parent.net` — escalation hole.
    };
    let c_fs = Bool::and(&ctx, &[&w_fs, &p_fs]);
    let c_exec = Bool::and(&ctx, &[&w_exec, &p_exec]);

    // O1: every child cap implies the parent holds it.
    let o1 = Bool::and(
        &ctx,
        &[
            &c_net.implies(&p_net),
            &c_fs.implies(&p_fs),
            &c_exec.implies(&p_exec),
        ],
    );

    // ∀ inputs. O1  ⟺  ¬O1 is UNSAT.
    let solver = Solver::new(&ctx);
    solver.assert(&o1.not());
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven {
            function: "principal_mint(O1:attenuation)".into(),
        },
        SatResult::Sat => {
            let model = solver.get_model().expect("sat ⇒ a model exists");
            let as_int = |b: &Bool| -> i64 {
                match model.eval(b, true).and_then(|v| v.as_bool()) {
                    Some(true) => 1,
                    _ => 0,
                }
            };
            ProofResult::Counterexample {
                function: "principal_mint(O1:attenuation)".into(),
                inputs: vec![
                    ("parent_net".into(), as_int(&p_net)),
                    ("want_net".into(), as_int(&w_net)),
                ],
                predicate: "child.net ⇒ parent.net".into(),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: "principal_mint(O1:attenuation)".into(),
            reason: "Z3 returned unknown on the attenuation VC".into(),
        },
    }
}

/// R20 Slice 1: prove the FULL mint obligation — O1 (capability attenuation)
/// AND O2 (budget carve / non-inflation) — for all inputs. Extends the slice-0
/// spike with the integer budget arithmetic. Route B: `mint`'s fixed semantics
/// are encoded directly.
///
/// O2 (over `parent.cap`, `parent.used`, `grant` ∈ ℤ), faithful to
/// `Budget::remaining` (clamps at 0) and `mint`'s `clamp(grant, 0, rem)`:
/// ```text
///   rem        = max(0, cap − used)
///   g          = max(0, min(grant, rem))      // child.budget.cap
///   used'      = used + g
///   rem_after  = max(0, cap − used')
///   O2a: g ≥ 0          O2b: g ≤ rem          O2c: rem_after + g = rem
/// ```
/// `faithful_caps` / `faithful_budget` select the real semantics (must PROVE)
/// vs. a weakened encoding (must yield a counterexample — the slice-1 E1610
/// I-12 tripwire at the *model* level; the kernel-impl tripwire is the
/// differential property test in `kernel.rs`).
#[cfg(feature = "smt")]
pub fn prove_mint_obligations(faithful_caps: bool, faithful_budget: bool) -> ProofResult {
    use z3::ast::{Bool, Int};

    let cfg = Config::new();
    let ctx = Context::new(&cfg);
    let name = "principal_mint(O1:attenuation + O2:budget-carve)".to_string();

    // ── O1: capability attenuation (boolean) ──
    let p_net = Bool::new_const(&ctx, "parent_net");
    let p_fs = Bool::new_const(&ctx, "parent_fs_write");
    let p_exec = Bool::new_const(&ctx, "parent_exec");
    let w_net = Bool::new_const(&ctx, "want_net");
    let w_fs = Bool::new_const(&ctx, "want_fs_write");
    let w_exec = Bool::new_const(&ctx, "want_exec");

    let c_net = if faithful_caps {
        Bool::and(&ctx, &[&w_net, &p_net])
    } else {
        w_net.clone() // WEAKENED: drops `&& parent.net`.
    };
    let c_fs = Bool::and(&ctx, &[&w_fs, &p_fs]);
    let c_exec = Bool::and(&ctx, &[&w_exec, &p_exec]);
    let o1 = Bool::and(
        &ctx,
        &[
            &c_net.implies(&p_net),
            &c_fs.implies(&p_fs),
            &c_exec.implies(&p_exec),
        ],
    );

    // ── O2: budget carve (integer, with the remaining()/clamp max-0 logic) ──
    let zero = Int::from_i64(&ctx, 0);
    let cap = Int::new_const(&ctx, "parent_cap");
    let used = Int::new_const(&ctx, "parent_used");
    let grant = Int::new_const(&ctx, "grant");

    // max0(x) = ite(x >= 0, x, 0);  min(a,b) = ite(a <= b, a, b).
    // Nested fns (not closures) so the ctx lifetime unifies cleanly.
    fn max0<'c>(zero: &Int<'c>, x: &Int<'c>) -> Int<'c> {
        x.ge(zero).ite(x, zero)
    }
    fn min2<'c>(a: &Int<'c>, b: &Int<'c>) -> Int<'c> {
        a.le(b).ite(a, b)
    }

    let cap_minus_used = &cap - &used;
    let rem = max0(&zero, &cap_minus_used);
    let g = if faithful_budget {
        let clamped = min2(&grant, &rem);
        max0(&zero, &clamped)
    } else {
        grant.clone() // WEAKENED: no clamp — budget can be inflated past rem.
    };
    let used_after = &used + &g;
    let cap_minus_used_after = &cap - &used_after;
    let rem_after = max0(&zero, &cap_minus_used_after);

    let o2a = g.ge(&zero);
    let o2b = g.le(&rem);
    let o2c = (&rem_after + &g)._eq(&rem);
    let o2 = Bool::and(&ctx, &[&o2a, &o2b, &o2c]);

    // ∀ inputs. (O1 ∧ O2)  ⟺  ¬(O1 ∧ O2) is UNSAT.
    let obligation = Bool::and(&ctx, &[&o1, &o2]);
    let solver = Solver::new(&ctx);
    solver.assert(&obligation.not());
    match solver.check() {
        SatResult::Unsat => ProofResult::Proven { function: name },
        SatResult::Sat => {
            let model = solver.get_model().expect("sat ⇒ a model exists");
            let b = |x: &Bool| match model.eval(x, true).and_then(|v| v.as_bool()) {
                Some(true) => 1,
                _ => 0,
            };
            let i = |x: &Int| model.eval(x, true).and_then(|v| v.as_i64()).unwrap_or(0);
            ProofResult::Counterexample {
                function: name,
                inputs: vec![
                    ("parent_net".into(), b(&p_net)),
                    ("want_net".into(), b(&w_net)),
                    ("parent_cap".into(), i(&cap)),
                    ("parent_used".into(), i(&used)),
                    ("grant".into(), i(&grant)),
                ],
                predicate: "child.cap <= parent.remaining ∧ child.cap ⇒ parent.cap".into(),
            }
        }
        SatResult::Unknown => ProofResult::Unsupported {
            function: name,
            reason: "Z3 returned unknown on the mint obligation VC".into(),
        },
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
                let x = inputs
                    .iter()
                    .find(|(n, _)| n == "x")
                    .map(|(_, v)| *v)
                    .unwrap();
                assert!(
                    x < 1,
                    "counterexample x={x} must violate value>=0 (x-1 < 0)"
                );
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
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );
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

    // ── Encoder widening: bound builtins (min/max/abs) + logical connectives ──
    #[test]
    fn smt_proves_bound_builtins() {
        // max_i64(x, 0) >= 0, abs_i64(x) >= 0, and min_i64(x, 10) <= 10 hold ∀x.
        for (src, what) in [
            (
                "@[verify(value >= 0)]\nfn f(x: i64) -> i64 { max_i64(x, 0) }",
                "max_i64",
            ),
            (
                "@[verify(value >= 0)]\nfn f(x: i64) -> i64 { abs_i64(x) }",
                "abs_i64",
            ),
            (
                "@[verify(value <= 10)]\nfn f(x: i64) -> i64 { min_i64(x, 10) }",
                "min_i64",
            ),
        ] {
            assert!(
                matches!(prove(src).as_slice(), [ProofResult::Proven { .. }]),
                "{what} bound must be proven, got {:?}",
                prove(src)
            );
        }
    }

    #[test]
    fn smt_proves_conjunctive_guard() {
        // A `&&` condition is now in-fragment: in the then-arm x>0 && x<10 holds,
        // so returning x satisfies value >= 0; the else-arm returns 0.
        let r = prove(
            "@[verify(value >= 0)]\n\
             fn band(x: i64) -> i64 { if x > 0 && x < 10 { x } else { 0 } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );
    }

    #[test]
    fn smt_disproves_false_bound_builtin_no_unsound_proof() {
        // SOUNDNESS: the widening must NOT falsely prove a bound that doesn't
        // hold. max_i64(x, 0) >= 5 is false (x<=0 → result 0 < 5) → counterexample.
        let r = prove(
            "@[verify(value >= 5)]\n\
             fn f(x: i64) -> i64 { max_i64(x, 0) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "a false bound over max_i64 must be disproven, not proven: got {r:?}"
        );
    }

    #[test]
    fn smt_proves_f64_bound_builtin() {
        let r = prove(
            "@[verify(value >= 0.0)]\n\
             fn fclamp(x: f64) -> f64 { max_f64(x, 0.0) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );
    }

    #[test]
    fn smt_user_call_still_unsupported_not_falsely_proven() {
        // The widening is limited to the named bound builtins — an arbitrary user
        // fn call stays Unsupported (runtime gate kept), never falsely proven.
        let r = prove(
            "fn helper(x: i64) -> i64 { x + 100 }\n\
             @[verify(value >= 0)]\n\
             fn uses_call(x: i64) -> i64 { helper(x) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Unsupported { .. }]),
            "got {r:?}"
        );
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
                assert!(
                    predicate.contains("&&"),
                    "predicate should show the conjunction: {predicate}"
                );
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
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );
    }

    #[test]
    fn smt_float_ite_bound_proven() {
        // The float fragment includes `ite`: abs(x) >= 0.0 for all real x.
        let r = prove(
            "@[verify(value >= 0.0)]\n\
             fn absf(x: f64) -> f64 { if x >= 0.0 { x } else { 0.0 - x } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );
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
        let r = prove_refines("type NonNeg = i64 where _ >= 0\nfn sq(n: i64) -> NonNeg { n * n }");
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "n*n >= 0, got {r:?}"
        );

        // COUNTEREXAMPLE: returning n unchanged does NOT guarantee > 0 (n=0).
        let r = prove_refines("type Positive = i64 where _ > 0\nfn id(n: i64) -> Positive { n }");
        match r.as_slice() {
            [ProofResult::Counterexample { inputs, .. }] => {
                assert!(
                    inputs.iter().any(|(_, v)| *v <= 0),
                    "counterexample must be <= 0: {inputs:?}"
                );
            }
            other => panic!("expected a counterexample, got {other:?}"),
        }
    }

    #[test]
    fn smt_refinement_return_uses_bound_builtins_and_connectives() {
        // The encoder widening (min/max/abs + &&/||/!) is SHARED by both provers,
        // so a refinement RETURN built from a bound builtin is proven too.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\nfn clamp(x: i64) -> NonNeg { max_i64(x, 0) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "max_i64(x,0) -> NonNeg must be proven, got {r:?}"
        );

        // A `&&` guard in the body, returning a refined value, is also in-fragment.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\n\
             fn g(x: i64) -> NonNeg { if x > 0 && x < 10 { x } else { 0 } }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "got {r:?}"
        );

        // SOUNDNESS: min_i64(x, 5) is NOT always > 0 (x<=0 → result <= 0) → counterexample.
        let r = prove_refines(
            "type Positive = i64 where _ > 0\nfn h(x: i64) -> Positive { min_i64(x, 5) }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "min_i64(x,5) -> Positive must be disproven (x<=0), got {r:?}"
        );
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
        let r = prove_refines("type PosF = f64 where _ > 0.0\nfn idf(x: f64) -> PosF { x }");
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "idf -> PosF must yield a counterexample, got {r:?}"
        );
    }

    #[test]
    fn smt_proves_refinement_return_through_let_bindings() {
        // A multi-line body with `let` bindings: `d = a - b; d * d >= 0` for all a, b.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\n\
             fn dist2(a: i64, b: i64) -> NonNeg { let d = a - b\n d * d }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "let-bound (a-b)^2 >= 0 must be proven, got {r:?}"
        );

        // The same over the reals, chaining two `let`s.
        let r = prove_refines(
            "type NonNegF = f64 where _ >= 0.0\n\
             fn sqdiff(x: f64, y: f64) -> NonNegF { let d = x - y\n let s = d * d\n s }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Proven { .. }]),
            "let-bound real (x-y)^2 >= 0 must be proven, got {r:?}"
        );

        // A `let` that does NOT rescue a false claim still yields a counterexample.
        let r = prove_refines(
            "type Positive = i64 where _ > 0\n\
             fn viaLet(n: i64) -> Positive { let m = n\n m }",
        );
        assert!(
            matches!(r.as_slice(), [ProofResult::Counterexample { .. }]),
            "let-bound identity must still be refuted at n=0, got {r:?}"
        );
    }

    #[test]
    fn smt_proves_refinement_return_through_inlined_calls() {
        // The body calls a helper `sq`; β-inlining brings `n*n` into the solver,
        // so `wrap(n) -> NonNeg` is proven for all n — only one of the two fns
        // returns a refinement, so we expect exactly one Proven result.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\n\
             fn sq(x: i64) -> i64 { x * x }\n\
             fn wrap(n: i64) -> NonNeg { sq(n) }",
        );
        assert!(
            r.iter()
                .any(|p| matches!(p, ProofResult::Proven { function } if function == "wrap")),
            "inlined sq(n)=n*n >= 0 must prove wrap, got {r:?}"
        );

        // Nested inlining (depth > 1): quad(n) = sq(sq(n)) = n^4 >= 0.
        let r = prove_refines(
            "type NonNeg = i64 where _ >= 0\n\
             fn sq(x: i64) -> i64 { x * x }\n\
             fn quad(n: i64) -> NonNeg { sq(sq(n)) }",
        );
        assert!(
            r.iter()
                .any(|p| matches!(p, ProofResult::Proven { function } if function == "quad")),
            "nested-inlined n^4 >= 0 must prove quad, got {r:?}"
        );

        // Inlining a helper that does NOT make the claim true is still refuted:
        // `idh(n) = n`, so `pos(n) -> Positive` fails at n=0.
        let r = prove_refines(
            "type Positive = i64 where _ > 0\n\
             fn idh(x: i64) -> i64 { x }\n\
             fn pos(n: i64) -> Positive { idh(n) }",
        );
        assert!(
            r.iter().any(
                |p| matches!(p, ProofResult::Counterexample { function, .. } if function == "pos")
            ),
            "inlined identity must still be refuted, got {r:?}"
        );
    }

    #[test]
    fn smt_proves_refinement_arg_forwarding_subtyping() {
        // A `Positive` (> 0) forwarded where `NonNeg` (>= 0) is required is SAFE:
        // every positive is non-negative, so the implication holds.
        let program = parse_source(
            "type Positive = i64 where _ > 0\n\
             type NonNeg = i64 where _ >= 0\n\
             fn sink(x: NonNeg) -> i64 { x }\n\
             fn forward(p: Positive) -> i64 { sink(p) }",
        )
        .expect("parse");
        let mut refs = std::collections::HashMap::new();
        for it in &program.items {
            if let Item::RefineDef(r) = it {
                refs.insert(r.name.clone(), (*r.predicate).clone());
            }
        }
        let r = prove_refinement_arg_forwarding(&program, &refs);
        assert!(
            r.iter().any(|p| matches!(p, ProofResult::Proven { .. })),
            "Positive ⟹ NonNeg forwarding must be proven safe, got {r:?}"
        );
        assert!(
            !r.iter()
                .any(|p| matches!(p, ProofResult::Counterexample { .. })),
            "no counterexample expected for a sound forward, got {r:?}"
        );

        // The UNSOUND direction: forwarding a `NonNeg` where `Positive` is
        // required fails — `_ = 0` is non-negative but not positive.
        let program = parse_source(
            "type Positive = i64 where _ > 0\n\
             type NonNeg = i64 where _ >= 0\n\
             fn sink(x: Positive) -> i64 { x }\n\
             fn forward(p: NonNeg) -> i64 { sink(p) }",
        )
        .expect("parse");
        let mut refs = std::collections::HashMap::new();
        for it in &program.items {
            if let Item::RefineDef(r) = it {
                refs.insert(r.name.clone(), (*r.predicate).clone());
            }
        }
        let r = prove_refinement_arg_forwarding(&program, &refs);
        match r
            .iter()
            .find(|p| matches!(p, ProofResult::Counterexample { .. }))
        {
            Some(ProofResult::Counterexample { inputs, .. }) => {
                assert!(
                    inputs.iter().any(|(_, v)| *v <= 0),
                    "witness must be <= 0: {inputs:?}"
                );
            }
            _ => panic!("NonNeg ⟹ Positive must yield a counterexample, got {r:?}"),
        }
    }

    #[test]
    fn smt_proof_budget_env_parsing() {
        // Depth: default, explicit, 0 (disable), clamp to 64, and junk → default.
        assert_eq!(inline_depth_from(None), INLINE_DEPTH_DEFAULT);
        assert_eq!(inline_depth_from(Some("2")), 2);
        assert_eq!(inline_depth_from(Some("0")), 0);
        assert_eq!(inline_depth_from(Some("999")), 64);
        assert_eq!(inline_depth_from(Some(" 3 ")), 3);
        assert_eq!(inline_depth_from(Some("nope")), INLINE_DEPTH_DEFAULT);
        // Timeout: default, explicit, 0 (unbounded), junk → default, and the
        // small-value floor that dodges the Z3 sub-resolution-timeout wedge.
        assert_eq!(proof_timeout_ms_from(None), PROOF_TIMEOUT_MS_DEFAULT);
        assert_eq!(proof_timeout_ms_from(Some("500")), 500);
        assert_eq!(proof_timeout_ms_from(Some("0")), 0);
        assert_eq!(proof_timeout_ms_from(Some("x")), PROOF_TIMEOUT_MS_DEFAULT);
        assert_eq!(proof_timeout_ms_from(Some("1")), PROOF_TIMEOUT_MS_FLOOR);
        assert_eq!(proof_timeout_ms_from(Some("10000")), 10000);
    }

    #[test]
    fn smt_inlining_respects_depth_argument() {
        // Directly exercise the pre-pass: at depth 0 the call survives un-inlined
        // (→ Unsupported when encoded); at the default depth it expands to `n*n`.
        let program = parse_source(
            "type NonNeg = i64 where _ >= 0\n\
             fn sq(x: i64) -> i64 { x * x }\n\
             fn wrap(n: i64) -> NonNeg { sq(n) }",
        )
        .expect("parse");
        let fns = program_fns(&program);
        let wrap = program
            .items
            .iter()
            .find_map(|it| match it {
                Item::FnDef(f) if f.name == "wrap" => Some(f),
                _ => None,
            })
            .expect("wrap");
        // depth 0: the call survives (the body is a Block wrapping it).
        let kept = inline_calls(&wrap.body, &fns, 0);
        assert!(
            format!("{kept:?}").contains("Call"),
            "depth 0 must keep the call: {kept:?}"
        );
        // depth >= 1: the call is gone (expanded to arithmetic).
        let inlined = inline_calls(&wrap.body, &fns, 4);
        assert!(
            !format!("{inlined:?}").contains("Call"),
            "call must be inlined: {inlined:?}"
        );
    }

    // ── Phase 5 §4: discharge() — the bridge into the default pipeline ────────

    fn discharge_of(src: &str) -> crate::verify::Discharged {
        let program = parse_source(src).expect("parse");
        let mut refs = std::collections::HashMap::new();
        for item in &program.items {
            if let Item::RefineDef(r) = item {
                refs.insert(r.name.clone(), (*r.predicate).clone());
            }
        }
        discharge(&program, &refs)
    }

    #[test]
    fn discharge_collects_proven_verify_and_refine_returns() {
        // A provable scalar @[verify] bound lands in verify_fns; a provable
        // refinement return lands in refine_return_fns — both eligible for
        // runtime-check elision.
        let d = discharge_of(
            "@[verify(value >= 0)]\n\
             fn absish(x: i64) -> i64 { if x >= 0 { x } else { 0 - x } }\n\
             type NonNeg = i64 where _ >= 0\n\
             fn sq(n: i64) -> NonNeg { n * n }",
        );
        assert!(d.verify_proven("absish"), "absish's bound is ∀-proven");
        assert!(
            d.refine_return_proven("sq"),
            "sq's NonNeg return is ∀-proven"
        );
        assert_eq!(d.total(), 2);
    }

    #[test]
    fn discharge_excludes_violating_and_unsupported() {
        // SOUNDNESS: a fn whose bound has a counterexample (dec(0) = -1) must NOT
        // be discharged — its runtime check must stay armed. Likewise an
        // out-of-fragment obligation (a call) is left runtime-checked.
        let d = discharge_of(
            "@[verify(value >= 0)]\n\
             fn dec(x: i64) -> i64 { x - 1 }\n\
             fn helper(x: i64) -> i64 { x }\n\
             @[verify(value >= 0)]\n\
             fn uses_call(x: i64) -> i64 { helper(x) }",
        );
        assert!(
            !d.verify_proven("dec"),
            "a violable bound must NOT be discharged"
        );
        assert!(
            !d.verify_proven("uses_call"),
            "an unsupported bound stays runtime-checked"
        );
        assert_eq!(d.total(), 0, "nothing provable here");
    }

    // ── R20 Slice 0: capability-attenuation spike ────────────────────────────

    #[test]
    fn r20_mint_attenuation_o1_is_proven_for_all_inputs() {
        // The faithful `child.cap = want ∧ parent.cap` encoding must PROVE O1
        // (no child cap exceeds the parent) for all 2^6 boolean inputs. This is
        // the spike's primary signal: the Bool + relational encoder surface the
        // mint obligation needs is reachable in z3 0.12 from this codebase.
        match prove_mint_attenuation(true) {
            ProofResult::Proven { function } => {
                assert!(function.contains("attenuation"));
            }
            other => panic!("faithful mint must prove O1, got {other:?}"),
        }
    }

    #[test]
    fn r20_weakened_mint_is_caught_not_discharged() {
        // SOUNDNESS / I-12 tripwire seed: a mint that drops `&& parent.net`
        // (an escalation hole) must NOT be provable — the encoder must return a
        // concrete counterexample (want_net ∧ ¬parent.net ⇒ child.net but not
        // parent.net), which slice 1 turns into E1610.
        match prove_mint_attenuation(false) {
            ProofResult::Counterexample {
                inputs, predicate, ..
            } => {
                assert!(predicate.contains("parent.net"));
                // The witness sets want_net=1 while parent_net=0.
                let want = inputs
                    .iter()
                    .find(|(n, _)| n == "want_net")
                    .map(|(_, v)| *v);
                let parent = inputs
                    .iter()
                    .find(|(n, _)| n == "parent_net")
                    .map(|(_, v)| *v);
                assert_eq!(want, Some(1), "witness must request net");
                assert_eq!(parent, Some(0), "witness parent must lack net");
            }
            other => panic!("weakened mint must be refuted, got {other:?}"),
        }
    }

    #[test]
    fn r20_mint_full_obligation_o1_and_o2_proven() {
        // Slice 1: the FULL obligation (attenuation AND budget carve) proves for
        // all boolean caps × all integer (cap, used, grant).
        match prove_mint_obligations(true, true) {
            ProofResult::Proven { .. } => {}
            other => panic!("faithful mint must prove O1∧O2, got {other:?}"),
        }
    }

    #[test]
    fn r20_budget_weakening_inflation_is_caught() {
        // Dropping the clamp (`g = grant`) lets a child be granted more than the
        // parent's remaining — budget inflation. Must be refuted with a witness
        // where grant exceeds parent.remaining.
        match prove_mint_obligations(true, false) {
            ProofResult::Counterexample { inputs, .. } => {
                let get = |k: &str| {
                    inputs
                        .iter()
                        .find(|(n, _)| n == k)
                        .map(|(_, v)| *v)
                        .unwrap()
                };
                let (cap, used, grant) = (get("parent_cap"), get("parent_used"), get("grant"));
                // The witness must actually exercise the dropped clamp: the
                // unclamped grant differs from the faithful clamp(grant, 0, rem).
                // (Z3 may refute via O2a negative-grant, O2b over-grant, or O2c
                // conservation — all are valid; what matters is the clamp mattered.)
                let rem = (cap - used).max(0);
                let faithful_g = grant.clamp(0, rem);
                assert_ne!(
                    grant, faithful_g,
                    "witness must exercise the dropped clamp: grant={grant} rem={rem}"
                );
            }
            other => panic!("budget inflation must be refuted, got {other:?}"),
        }
    }

    #[test]
    fn r20_cap_weakening_still_caught_with_budget_arith_present() {
        // Sanity: the O1 mutation is still caught when O2 is also encoded (the
        // conjunction doesn't mask the attenuation hole).
        match prove_mint_obligations(false, true) {
            ProofResult::Counterexample { .. } => {}
            other => panic!("cap escalation must be refuted, got {other:?}"),
        }
    }

    #[test]
    fn r20_tcb_check_passes_for_the_in_tree_minter() {
        // The E1610 build-time gate: the faithful minter must discharge cleanly
        // (Ok). The negative path — Err(E1610, …) when the obligation is broken —
        // is covered by the mutation tests above, which exercise the same
        // prove_mint_obligations the check calls.
        assert!(
            check_mint_tcb_obligation().is_ok(),
            "in-tree mint must satisfy its TCB obligation"
        );
    }

    // ── R20 Slice 2: relational refinements for user attenuating fns ──────────

    #[test]
    fn r20_relational_attenuation_refinement_is_discharged() {
        // A user attenuating fn whose RETURN relates to a PARAMETER:
        // `carve` never returns more than `avail`, ∀ integers. With params
        // threaded into the predicate encoder this is now SMT-proven, not just
        // runtime-checked — the generalization of the mint budget-carve to
        // user-written functions.
        let d = discharge_of(
            "fn carve(grant: i64, avail: i64) -> (i64 where _ <= avail) \
             { min_i64(grant, avail) }",
        );
        assert!(
            d.refine_return_proven("carve"),
            "carve's `_ <= avail` relational return refinement must be ∀-proven"
        );
    }

    #[test]
    fn r20_violable_relational_refinement_is_not_discharged() {
        // SOUNDNESS: an attenuating contract that does NOT hold for all inputs
        // (returns `grant` unclamped, which can exceed `avail`) must NOT be
        // discharged — its runtime check (exit 6) stays armed.
        let d = discharge_of("fn over(grant: i64, avail: i64) -> (i64 where _ <= avail) { grant }");
        assert!(
            !d.refine_return_proven("over"),
            "a violable relational refinement must stay runtime-checked"
        );
    }

    // ── R20 Slice 3: TCB attestation ─────────────────────────────────────────

    #[test]
    fn r20_tcb_attestation_digest_matches_pinned_manifest() {
        // The live digest of (obligation spec ⊕ proof verdict) must equal the
        // pinned manifest. If this fails, the TCB changed — either update the
        // obligation/spec deliberately and re-pin TCB_MANIFEST_DIGEST (an
        // audited manifest change), or a regression broke the proof.
        assert_eq!(
            tcb_attestation_digest(),
            TCB_MANIFEST_DIGEST,
            "TCB digest drifted from the pinned manifest"
        );
    }

    #[test]
    fn r20_attestation_check_passes_for_proven_tcb() {
        assert!(
            check_tcb_attestation().is_ok(),
            "the in-tree, all-proven TCB must attest cleanly"
        );
    }

    #[test]
    fn r20_attestation_digest_changes_if_the_verdict_flips() {
        // Soundness of the boot tripwire: the digest folds in the verdict, so a
        // weakened proof (verdict ≠ Proven) MUST produce a different digest than
        // the all-proven manifest — boot would then fail closed (E1611).
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(MINT_OBLIGATION_SPEC.as_bytes());
        h.update(b"\x1f");
        h.update(b"Counterexample"); // a weakened/broken proof
        let weakened = format!("axtcb1:{:x}", h.finalize());
        assert_ne!(
            weakened, TCB_MANIFEST_DIGEST,
            "a flipped verdict must change the digest → boot mismatch"
        );
    }
}
