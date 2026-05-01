//! ASI Layer-2: `@[verify(expr)]` predicate checker.
//!
//! For each `FnDef` that has a `VerifySpec`, this pass:
//!   1. Computes the **minimum reachable confidence** at any return site of the
//!      function body via a simple lattice analysis over `Uncertain<T>` value
//!      sources (`uncertain_new`, `uncertain_deterministic`, calls to other
//!      verify-annotated functions, BinOps, branches).
//!   2. Evaluates the verify predicate against that minimum.
//!   3. Emits `E1101` if the predicate cannot hold for the computed minimum.
//!
//! v1 scope is intentionally narrow: only the form
//!
//! ```axon
//! @[verify(confidence OP literal_f64)]
//! fn foo(...) -> Uncertain<T> { ... }
//! ```
//!
//! where `OP` is one of `>=`, `>`, `<=`, `<`, `==`, `!=`, and `confidence` is
//! the magic identifier referring to the per-return-site minimum.
//!
//! Other predicate shapes are accepted by the parser but evaluated trivially
//! (treated as a no-op) and emit no diagnostic — they are *out of scope* for
//! v1 but reserved for future extension (refinement types, comptime).
//!
//! ## Error codes
//! E1101 — verify bound not satisfied
//!
//! ## Conservative analysis
//! The lattice is intentionally simple. Unknown sources resolve to confidence
//! `0.0` (the bottom element), making the analysis sound but conservative. If
//! Track A's `Uncertain<T>` arithmetic propagation has not yet landed, this
//! pass may compute a lower bound than the runtime would actually see — which
//! is acceptable for v1.

use std::collections::HashMap;

use crate::ast::{BinOp, Expr, FnDef, Item, Literal, Program, Stmt, VerifySpec};
use crate::span::Span;

// ── Error codes ───────────────────────────────────────────────────────────────

pub const E1101: &str = "E1101";

// ── Diagnostic ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct VerifyError {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

impl VerifyError {
    fn new(code: &'static str, message: impl Into<String>, span: Span) -> Self {
        Self { code, message: message.into(), span }
    }
}

// ── Confidence lattice ───────────────────────────────────────────────────────

/// A confidence-lattice value attached to an expression.
///
/// `Known(c)` means the analysis proved confidence is at least `c` (a lower
/// bound — we can be more confident than this, but never less). `Unknown`
/// means we couldn't track the value and treat it as `0.0` for soundness.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Confidence {
    /// Lower bound on the confidence in `[0.0, 1.0]`.
    Known(f64),
    /// Cannot prove any bound — soundly treated as 0.0.
    Unknown,
}

impl Confidence {
    /// Minimum of two confidences. The bottom element propagates.
    fn min(self, other: Confidence) -> Confidence {
        match (self, other) {
            (Confidence::Known(a), Confidence::Known(b)) => Confidence::Known(a.min(b)),
            // Treat Unknown as 0.0 for ordering but keep the Unknown tag.
            (Confidence::Unknown, _) | (_, Confidence::Unknown) => Confidence::Unknown,
        }
    }

    /// Concrete numeric value — Unknown maps to 0.0.
    fn as_f64(self) -> f64 {
        match self {
            Confidence::Known(c) => c,
            Confidence::Unknown => 0.0,
        }
    }
}

// ── Pre-computed table of verify bounds for inter-procedural lookup ──────────

/// For each user fn with `@[verify(confidence OP K)]` returning `Uncertain<_>`,
/// records the lower-bound confidence the verify clause guarantees.
///
/// Entries are keyed by function name. Only `>=` and `>` predicates contribute
/// (those are the ones that *establish* a lower bound). For `>=` the bound is
/// `K`; for `>` we use `K` as a conservative lower bound (the runtime value is
/// strictly greater, but using `K` is sound).
fn build_fn_bounds(program: &Program) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for item in &program.items {
        if let Item::FnDef(fndef) = item {
            if let Some(spec) = &fndef.verify {
                if let Some((op, lit)) = match_confidence_predicate(&spec.predicate) {
                    let bound = match op {
                        BinOp::GtEq | BinOp::Gt => Some(lit),
                        _ => None,
                    };
                    if let Some(b) = bound {
                        out.insert(fndef.name.clone(), b);
                    }
                }
            }
        }
    }
    out
}

// ── Predicate matching ───────────────────────────────────────────────────────

/// Public re-export for codegen: decode an `@[verify(...)]` predicate of the
/// form `confidence OP literal_f64` (or its symmetric `literal OP confidence`).
/// Returns `(op, literal)` with `op` normalised so that `confidence` is always
/// the left-hand side.  Any other predicate shape returns `None`.
///
/// The runtime verify-check codegen path uses this to extract the operator
/// and bound at every return site of a `@[verify]`-annotated fn.  The static
/// checker uses the same helper to decide whether to evaluate the predicate.
/// Single source of truth for predicate decoding — the operator-string
/// rendering for runtime panic messages comes from [`op_to_str`] (also pub).
pub fn decode_verify_predicate(expr: &Expr) -> Option<(BinOp, f64)> {
    match_confidence_predicate(expr)
}

/// Public re-export of the internal `op_to_str` so codegen can format the
/// operator into runtime-panic messages identically to compile-time messages.
pub fn binop_to_verify_str(op: &BinOp) -> &'static str {
    op_to_str(op)
}

/// Match an expression of the shape `confidence OP literal_f64` and return
/// `(op, literal)`. Returns `None` for any other shape.
fn match_confidence_predicate(expr: &Expr) -> Option<(BinOp, f64)> {
    if let Expr::BinOp { op, left, right } = expr {
        // Accept `confidence OP lit` and `lit OP confidence` (with op flipped).
        if is_confidence_ident(left) {
            if let Some(lit) = as_float_literal(right) {
                return Some((op.clone(), lit));
            }
        }
        if is_confidence_ident(right) {
            if let Some(lit) = as_float_literal(left) {
                return Some((flip_op(op.clone()), lit));
            }
        }
    }
    None
}

fn is_confidence_ident(expr: &Expr) -> bool {
    matches!(expr, Expr::Ident(name) if name == "confidence")
}

fn as_float_literal(expr: &Expr) -> Option<f64> {
    match expr {
        Expr::Literal(Literal::Float(f)) => Some(*f),
        Expr::Literal(Literal::Int(i)) => Some(*i as f64),
        _ => None,
    }
}

fn flip_op(op: BinOp) -> BinOp {
    match op {
        BinOp::Lt   => BinOp::Gt,
        BinOp::Gt   => BinOp::Lt,
        BinOp::LtEq => BinOp::GtEq,
        BinOp::GtEq => BinOp::LtEq,
        // Symmetric ops stay the same.
        other => other,
    }
}

/// Decide whether `op c lit` holds for a *lower-bound* confidence `c`.
///
/// Returns:
///   - `Some(true)`  — predicate provably holds for the computed min.
///   - `Some(false)` — predicate provably fails for the computed min.
///   - `None`        — cannot decide statically (e.g. predicate uses an
///                     equality that depends on the exact value).
fn evaluate_predicate(op: &BinOp, min_conf: f64, lit: f64) -> Option<bool> {
    match op {
        // `c >= lit` — sound iff min_conf >= lit.
        BinOp::GtEq => Some(min_conf >= lit),
        // `c > lit`  — sound iff min_conf > lit.
        BinOp::Gt   => Some(min_conf > lit),
        // `c <= lit` — sound iff *every* possible c <= lit. We only have a
        // lower bound, so this is undecidable unless lit >= 1.0.
        BinOp::LtEq => if lit >= 1.0 { Some(true) } else { None },
        // `c < lit`  — sound iff every possible c < lit; undecidable unless lit > 1.0.
        BinOp::Lt   => if lit > 1.0 { Some(true) } else { None },
        // Equality / inequality with a single literal can't be decided from a
        // lower bound alone unless lit < min_conf (then `==` provably fails).
        BinOp::Eq => {
            if lit < min_conf { Some(false) } else { None }
        }
        BinOp::NotEq => {
            if lit < min_conf { Some(true) } else { None }
        }
        _ => None,
    }
}

// ── Public entry point ───────────────────────────────────────────────────────

/// Run the verify check on all functions in `program`.
pub fn check_verify(program: &Program) -> Vec<VerifyError> {
    let mut errors = Vec::new();
    let fn_bounds = build_fn_bounds(program);
    for item in &program.items {
        if let Item::FnDef(fndef) = item {
            if let Some(spec) = &fndef.verify {
                check_fn(fndef, spec, &fn_bounds, &mut errors);
            }
        }
    }
    errors
}

fn check_fn(
    fndef: &FnDef,
    spec: &VerifySpec,
    fn_bounds: &HashMap<String, f64>,
    errors: &mut Vec<VerifyError>,
) {
    // Decode predicate. If we can't decode it as `confidence OP literal`,
    // treat the verify clause as unsupported and skip silently for v1.
    let (op, lit) = match match_confidence_predicate(&spec.predicate) {
        Some(p) => p,
        None => return,
    };

    // Compute the minimum reachable confidence at the function's "result"
    // expression (the body). The body lattice value represents the confidence
    // of the value the function produces.
    let analyzer = Analyzer { fn_bounds };
    let min_conf = analyzer.confidence_of(&fndef.body, &HashMap::new());

    let min = min_conf.as_f64();
    match evaluate_predicate(&op, min, lit) {
        Some(true) => { /* predicate holds — clean */ }
        Some(false) => {
            errors.push(VerifyError::new(
                E1101,
                format!(
                    "@[verify(confidence {} {})] not satisfied: minimum reachable confidence \
                     in `{}` is {} (must be {} {})",
                    op_to_str(&op), lit, fndef.name, min, op_to_str(&op), lit,
                ),
                spec.span,
            ));
        }
        None => { /* undecidable — be silent rather than noisy in v1 */ }
    }
}

fn op_to_str(op: &BinOp) -> &'static str {
    match op {
        BinOp::Lt   => "<",
        BinOp::Gt   => ">",
        BinOp::LtEq => "<=",
        BinOp::GtEq => ">=",
        BinOp::Eq   => "==",
        BinOp::NotEq=> "!=",
        _ => "?",
    }
}

// ── Lattice analysis ─────────────────────────────────────────────────────────

struct Analyzer<'a> {
    fn_bounds: &'a HashMap<String, f64>,
}

/// Local environment mapping variable name → confidence at the binding point.
type Env = HashMap<String, Confidence>;

impl<'a> Analyzer<'a> {
    /// Compute the lattice value for an expression. The returned confidence is
    /// a lower bound on the confidence of the value `expr` evaluates to,
    /// considering *only* `Uncertain<T>` provenance — non-Uncertain values
    /// resolve to `Unknown` (the spec says verify only applies to functions
    /// returning `Uncertain<T>`).
    fn confidence_of(&self, expr: &Expr, env: &Env) -> Confidence {
        match expr {
            Expr::Block(stmts) => self.confidence_of_block(stmts, env),

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    self.confidence_of_call(name, args, env)
                } else {
                    Confidence::Unknown
                }
            }

            Expr::If { then, else_, .. } => {
                let t = self.confidence_of(then, env);
                let e = match else_ {
                    Some(e) => self.confidence_of(e, env),
                    // No else — the value is `()` from the missing branch,
                    // so soundness requires Unknown there.
                    None => Confidence::Unknown,
                };
                t.min(e)
            }

            Expr::Match { arms, .. } => {
                if arms.is_empty() { return Confidence::Unknown; }
                let mut acc: Option<Confidence> = None;
                for arm in arms {
                    let c = self.confidence_of(&arm.body, env);
                    acc = Some(match acc {
                        Some(prev) => prev.min(c),
                        None => c,
                    });
                }
                acc.unwrap_or(Confidence::Unknown)
            }

            Expr::Return(inner) => match inner {
                Some(e) => self.confidence_of(e, env),
                None => Confidence::Unknown,
            },

            Expr::BinOp { left, right, .. } => {
                // For Uncertain arithmetic the runtime takes the min of the
                // operand confidences. We mirror that here.
                let l = self.confidence_of(left, env);
                let r = self.confidence_of(right, env);
                l.min(r)
            }

            Expr::UnaryOp { operand, .. } => self.confidence_of(operand, env),

            Expr::Ident(name) => env.get(name).copied().unwrap_or(Confidence::Unknown),

            // Pass-through for transparent wrappers.
            Expr::Comptime(inner) => self.confidence_of(inner, env),
            Expr::Question(inner) => self.confidence_of(inner, env),
            Expr::Ok(inner) | Expr::Err(inner) | Expr::Some(inner) => {
                self.confidence_of(inner, env)
            }

            // Anything else: unknown.
            _ => Confidence::Unknown,
        }
    }

    fn confidence_of_block(&self, stmts: &[Stmt], env: &Env) -> Confidence {
        // Walk statements in order, threading let-bindings through the env.
        // The block's lattice value is the confidence of its final expression
        // (or any explicit `return` within).
        let mut local = env.clone();
        let mut last = Confidence::Unknown;
        let mut return_min: Option<Confidence> = None;

        for (i, stmt) in stmts.iter().enumerate() {
            // Track explicit `return` sites — they all flow into the function
            // result and we take the min across them.
            if let Some(ret_c) = self.scan_returns(&stmt.expr, &local) {
                return_min = Some(match return_min {
                    Some(prev) => prev.min(ret_c),
                    None => ret_c,
                });
            }

            match &stmt.expr {
                Expr::Let { name, value }
                | Expr::Own { name, value }
                | Expr::RefBind { name, value } => {
                    let c = self.confidence_of(value, &local);
                    local.insert(name.clone(), c);
                    last = Confidence::Unknown; // not the tail expr
                }
                _ => {
                    let c = self.confidence_of(&stmt.expr, &local);
                    if i == stmts.len() - 1 {
                        last = c;
                    }
                }
            }
        }

        match return_min {
            Some(r) => r.min(last),
            None => last,
        }
    }

    /// Recursively scan an expression for `return e` sites and return the min
    /// confidence among them, or `None` if none found.
    fn scan_returns(&self, expr: &Expr, env: &Env) -> Option<Confidence> {
        match expr {
            Expr::Return(Some(e)) => Some(self.confidence_of(e, env)),
            Expr::Return(None) => Some(Confidence::Unknown),
            Expr::Block(stmts) => {
                let mut acc: Option<Confidence> = None;
                for s in stmts {
                    if let Some(c) = self.scan_returns(&s.expr, env) {
                        acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                    }
                }
                acc
            }
            Expr::If { cond, then, else_ } => {
                let mut acc = self.scan_returns(cond, env);
                if let Some(c) = self.scan_returns(then, env) {
                    acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                }
                if let Some(e) = else_ {
                    if let Some(c) = self.scan_returns(e, env) {
                        acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                    }
                }
                acc
            }
            Expr::Match { subject, arms } => {
                let mut acc = self.scan_returns(subject, env);
                for arm in arms {
                    if let Some(c) = self.scan_returns(&arm.body, env) {
                        acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                    }
                }
                acc
            }
            Expr::While { body, .. } | Expr::WhileLet { body, .. } => {
                let mut acc: Option<Confidence> = None;
                for s in body {
                    if let Some(c) = self.scan_returns(&s.expr, env) {
                        acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                    }
                }
                acc
            }
            Expr::For { body, .. } => {
                let mut acc: Option<Confidence> = None;
                for s in body {
                    if let Some(c) = self.scan_returns(&s.expr, env) {
                        acc = Some(match acc { Some(prev) => prev.min(c), None => c });
                    }
                }
                acc
            }
            // Other compound exprs don't typically host `return` in v1 — be
            // silent. The body-tail traversal handles the common cases.
            _ => None,
        }
    }

    fn confidence_of_call(&self, name: &str, args: &[Expr], env: &Env) -> Confidence {
        match name {
            // `uncertain_new(value, confidence)` — second arg is the literal.
            "uncertain_new" => {
                if args.len() == 2 {
                    if let Some(c) = as_float_literal(&args[1]) {
                        return Confidence::Known(c);
                    }
                }
                Confidence::Unknown
            }
            // `uncertain_deterministic(value)` — confidence is exactly 1.0.
            "uncertain_deterministic" => Confidence::Known(1.0),
            // Inter-procedural: another verify-annotated function.
            other => match self.fn_bounds.get(other) {
                Some(b) => Confidence::Known(*b),
                None => {
                    // Unknown user fn: take the conservative min of arg
                    // confidences if any of them are tracked Uncertains.
                    // This catches helpers that just forward an Uncertain
                    // arg through. Otherwise Unknown.
                    let mut acc: Option<Confidence> = None;
                    for a in args {
                        let c = self.confidence_of(a, env);
                        if matches!(c, Confidence::Known(_)) {
                            acc = Some(match acc {
                                Some(prev) => prev.min(c),
                                None => c,
                            });
                        }
                    }
                    acc.unwrap_or(Confidence::Unknown)
                }
            },
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, FnDef, Item, Literal, Program, Stmt, VerifySpec};
    use crate::span::Span;

    fn lit_f(f: f64) -> Expr { Expr::Literal(Literal::Float(f)) }
    fn lit_i(i: i64) -> Expr { Expr::Literal(Literal::Int(i)) }

    fn call(name: &str, args: Vec<Expr>) -> Expr {
        Expr::Call { callee: Box::new(Expr::Ident(name.into())), args }
    }

    fn make_fn(name: &str, body: Expr, predicate: Option<Expr>) -> FnDef {
        FnDef {
            public: false,
            name: name.into(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: None,
            body,
            attrs: vec![],
            contained: None,
            verify: predicate.map(|p| VerifySpec {
                predicate: Box::new(p),
                span: Span::dummy(),
            }),
            span: Span::dummy(),
        }
    }

    #[test]
    fn pass_when_uncertain_new_meets_threshold() {
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_new",
            vec![lit_i(42), lit_f(0.9)],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("safe", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn fail_when_uncertain_new_below_threshold() {
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_new",
            vec![lit_i(42), lit_f(0.5)],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("risky", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, E1101);
    }

    #[test]
    fn deterministic_meets_any_threshold_below_one() {
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_deterministic",
            vec![lit_i(0)],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.99)),
        };
        let fndef = make_fn("d", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }

    #[test]
    fn if_else_takes_min_of_branches() {
        // if true { uncertain_new(_, 0.9) } else { uncertain_new(_, 0.5) }
        // → min = 0.5  →  fails @[verify(confidence >= 0.8)]
        let body = Expr::Block(vec![Stmt::simple(Expr::If {
            cond: Box::new(Expr::Literal(Literal::Bool(true))),
            then: Box::new(call("uncertain_new", vec![lit_i(1), lit_f(0.9)])),
            else_: Some(Box::new(call("uncertain_new", vec![lit_i(2), lit_f(0.5)]))),
        })]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("cond", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert_eq!(errors.len(), 1);
        assert_eq!(errors[0].code, E1101);
    }

    #[test]
    fn unknown_predicate_form_is_silent() {
        // @[verify(some_other_thing())] — not the supported form, no diagnostic.
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_new",
            vec![lit_i(1), lit_f(0.5)],
        ))]);
        let pred = call("some_other_thing", vec![]);
        let fndef = make_fn("weird", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(errors.is_empty(), "unsupported predicate form should be silent");
    }

    #[test]
    fn inter_procedural_verify_bound_propagates() {
        // fn safe() ... @[verify(confidence >= 0.9)]
        // fn caller() @[verify(confidence >= 0.8)] { safe() }   // pass
        let safe_body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_new",
            vec![lit_i(1), lit_f(0.95)],
        ))]);
        let safe_pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.9)),
        };
        let safe_fn = make_fn("safe", safe_body, Some(safe_pred));

        let caller_body = Expr::Block(vec![Stmt::simple(call("safe", vec![]))]);
        let caller_pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let caller_fn = make_fn("caller", caller_body, Some(caller_pred));

        let prog = Program { items: vec![Item::FnDef(safe_fn), Item::FnDef(caller_fn)] };
        let errors = check_verify(&prog);
        assert!(errors.is_empty(), "expected no errors, got: {errors:?}");
    }
}
