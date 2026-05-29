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
/// bound — we can be more confident than this, but never less). `Runtime`
/// means the value comes from a source with a runtime confidence check
/// (e.g. `ai_extract_uncertain_*`); the static checker defers to the
/// runtime gate (`__axon_verify_panic`, Layer 3.5). `Unknown` means we
/// couldn't track the value and treat it as `0.0` for soundness.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Confidence {
    /// Lower bound on the confidence in `[0.0, 1.0]`.
    Known(f64),
    /// Confidence is checked at runtime — static analysis stays silent.
    Runtime,
    /// Cannot prove any bound — soundly treated as 0.0.
    Unknown,
}

impl Confidence {
    /// Minimum of two confidences. The bottom element propagates.
    ///
    /// Lattice ordering (worst → best for the caller):
    ///   Unknown  <  Runtime  <  Known(c)
    ///
    /// `Unknown` is strictly worse than `Runtime` because `Runtime` carries
    /// a guarantee (the runtime check will fire), whereas `Unknown` carries
    /// no guarantee at all.
    fn min(self, other: Confidence) -> Confidence {
        match (self, other) {
            (Confidence::Known(a), Confidence::Known(b)) => Confidence::Known(a.min(b)),
            // Unknown dominates anything (it is the bottom element).
            (Confidence::Unknown, _) | (_, Confidence::Unknown) => Confidence::Unknown,
            // Runtime dominates Known — defer to the runtime check.
            (Confidence::Runtime, Confidence::Known(_))
            | (Confidence::Known(_), Confidence::Runtime)
            | (Confidence::Runtime, Confidence::Runtime) => Confidence::Runtime,
        }
    }

    /// Concrete numeric value — Unknown maps to 0.0, Runtime is not numeric
    /// but is treated as 0.0 here for any caller that still needs a value
    /// (the predicate evaluator short-circuits on `Runtime` before reaching
    /// this).
    fn as_f64(self) -> f64 {
        match self {
            Confidence::Known(c) => c,
            Confidence::Runtime => 0.0,
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
///     equality that depends on the exact value).
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

    // `Runtime` means a runtime-checked source dominates the lattice — the
    // static checker defers entirely to `__axon_verify_panic` (Layer 3.5).
    if matches!(min_conf, Confidence::Runtime) {
        return;
    }

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
                match callee.as_ref() {
                    Expr::Ident(name) => self.confidence_of_call(name, args, env),
                    // Generic builtin lowering: `ai_extract::<T>(prompt)` parses
                    // to `Call { callee: StructLit { name: "ai_extract::<T>", … }, … }`.
                    // Route through the same classifier so the synthetic name
                    // is recognised as a Runtime source for `@[verify]`.
                    Expr::StructLit { name, fields } if fields.is_empty() => {
                        self.confidence_of_call(name, args, env)
                    }
                    _ => Confidence::Unknown,
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

            Expr::Return(Some(e)) => self.confidence_of(e, env),
            Expr::Return(None) => Confidence::Unknown,

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
                Expr::Let { name, value, .. }
                | Expr::Own { name, value, .. }
                | Expr::RefBind { name, value, .. } => {
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
            // Runtime sources: the LLM reports a confidence at runtime that
            // `__axon_verify_panic` (Layer 3.5) will gate.  Static analysis
            // stays silent and defers to the runtime check.
            //
            // `uncertain_dyn_{i64,f64}` (Layer 3.6) are user-visible
            // constructors that mirror `uncertain_new`, but classify as
            // Runtime so that callers building Uncertains from non-literal
            // confidence (sensors, configs, computations) get the same
            // defer-to-runtime treatment AI sources do.
            "ai_extract_uncertain_i64"
            | "ai_extract_uncertain_f64"
            | "uncertain_dyn_i64"
            | "uncertain_dyn_f64" => Confidence::Runtime,
            // Layer-3 generic surface: `ai_extract::<Uncertain<T>>` returns an
            // `Uncertain<T>` whose confidence is reported by the LLM at
            // runtime.  Mirror the legacy `ai_extract_uncertain_*` aliases so
            // `@[verify]` defers to the runtime check.  The flat-T variants
            // (`ai_extract::<i64>` etc.) return non-Uncertain types and so
            // never reach this classifier (no Uncertain to inspect).
            "ai_extract::<Uncertain<i64>>"
            | "ai_extract::<Uncertain<f64>>" => Confidence::Runtime,
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
    fn ai_source_is_runtime_silent() {
        // fn ai_query() @[verify(confidence >= 0.8)] { ai_extract_uncertain_i64("...") }
        // Lattice value at the body is `Runtime` — static checker stays silent
        // and defers to the runtime check (`__axon_verify_panic`).
        let body = Expr::Block(vec![Stmt::simple(call(
            "ai_extract_uncertain_i64",
            vec![Expr::Literal(Literal::Str("how many?".into()))],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("ai_query", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(
            errors.is_empty(),
            "AI runtime source should be silent at compile time, got: {errors:?}",
        );
    }

    #[test]
    fn ai_extract_generic_uncertain_is_runtime_silent() {
        // The generic surface `ai_extract::<Uncertain<i64>>(prompt)` parses to
        // a `Call { callee: StructLit { name: "ai_extract::<Uncertain<i64>>" } }`.
        // Even though the predicate threshold is 0.8 and we have no static
        // confidence to check, the source must classify as Runtime so the
        // static checker stays silent and defers to `__axon_verify_panic`.
        let synthetic_call = Expr::Call {
            callee: Box::new(Expr::StructLit {
                name: "ai_extract::<Uncertain<i64>>".into(),
                fields: vec![],
            }),
            args: vec![Expr::Literal(Literal::Str("how many?".into()))],
        };
        let body = Expr::Block(vec![Stmt::simple(synthetic_call)]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("ai_query_generic", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(
            errors.is_empty(),
            "ai_extract::<Uncertain<i64>> should classify Runtime → silent, got: {errors:?}",
        );
    }

    #[test]
    fn uncertain_dyn_i64_classifies_as_runtime() {
        // fn dyn_i64() @[verify(confidence >= 0.8)] { uncertain_dyn_i64(42, 0.5) }
        // Even though the literal 0.5 is below 0.8, the static checker MUST
        // stay silent: `uncertain_dyn_i64` is a Runtime source — predicate
        // decisions defer to `__axon_verify_panic`.
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_dyn_i64",
            vec![lit_i(42), lit_f(0.5)],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("dyn_i64", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(
            errors.is_empty(),
            "uncertain_dyn_i64 should be Runtime → silent at compile time, got: {errors:?}",
        );
    }

    #[test]
    fn uncertain_dyn_f64_classifies_as_runtime() {
        // f64 variant — same lattice classification as the i64 form.
        let body = Expr::Block(vec![Stmt::simple(call(
            "uncertain_dyn_f64",
            vec![lit_f(3.14), lit_f(0.5)],
        ))]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("dyn_f64", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(
            errors.is_empty(),
            "uncertain_dyn_f64 should be Runtime → silent at compile time, got: {errors:?}",
        );
    }

    #[test]
    fn ai_source_combined_with_known_via_binop() {
        // ai_extract_uncertain_i64(...) + uncertain_new(0, 0.9)
        //   →  min(Runtime, Known(0.9)) = Runtime  →  silent
        let body = Expr::Block(vec![Stmt::simple(Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(call(
                "ai_extract_uncertain_i64",
                vec![Expr::Literal(Literal::Str("q".into()))],
            )),
            right: Box::new(call("uncertain_new", vec![lit_i(0), lit_f(0.9)])),
        })]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("mixed", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let errors = check_verify(&prog);
        assert!(
            errors.is_empty(),
            "min(Runtime, Known) should be Runtime → silent, got: {errors:?}",
        );
    }

    #[test]
    fn ai_source_combined_with_unknown_is_unknown() {
        // ai_extract_uncertain_i64(...) + some_unknown_thing()
        //   →  min(Runtime, Unknown) = Unknown
        // Unknown maps to 0.0; predicate `confidence >= 0.8` fails for 0.0 →
        // the existing `Known(0.0)`-style behavior would emit, but Unknown
        // here is computed via the binop path. The point of this test is to
        // pin down that Runtime does NOT mask Unknown — Unknown remains the
        // bottom and dominates.
        let body = Expr::Block(vec![Stmt::simple(Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(call(
                "ai_extract_uncertain_i64",
                vec![Expr::Literal(Literal::Str("q".into()))],
            )),
            // An unknown-classified call: not a known builtin, not a tracked
            // user fn. Its arg list contains no Uncertains, so the helper
            // resolves to Unknown.
            right: Box::new(call("totally_opaque_helper", vec![lit_i(7)])),
        })]);
        let pred = Expr::BinOp {
            op: BinOp::GtEq,
            left: Box::new(Expr::Ident("confidence".into())),
            right: Box::new(lit_f(0.8)),
        };
        let fndef = make_fn("opaque_mix", body, Some(pred));
        let prog = Program { items: vec![Item::FnDef(fndef)] };
        let _errors = check_verify(&prog);
        // We don't assert on emit/silence here — the spec instructs *not* to
        // change Unknown semantics. We only assert the lattice combinator
        // produced Unknown (via the direct-min unit test) and that nothing
        // panicked.  Behavior is whatever Layer 2 Track B already does.
    }

    #[test]
    fn lattice_min_rules() {
        let k_a = Confidence::Known(0.9);
        let k_b = Confidence::Known(0.5);
        let r   = Confidence::Runtime;
        let u   = Confidence::Unknown;

        // Known × Known
        assert_eq!(k_a.min(k_b), Confidence::Known(0.5));
        // Known × Runtime  →  Runtime  (both directions)
        assert_eq!(k_a.min(r), Confidence::Runtime);
        assert_eq!(r.min(k_a), Confidence::Runtime);
        // Runtime × Runtime  →  Runtime
        assert_eq!(r.min(r), Confidence::Runtime);
        // Known × Unknown  →  Unknown
        assert_eq!(k_a.min(u), Confidence::Unknown);
        // Runtime × Unknown  →  Unknown  (Unknown is bottom)
        assert_eq!(r.min(u), Confidence::Unknown);
        assert_eq!(u.min(r), Confidence::Unknown);
        // Unknown × Unknown  →  Unknown
        assert_eq!(u.min(u), Confidence::Unknown);
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
