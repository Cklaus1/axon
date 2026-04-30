//! Phase 3 — comptime expression evaluator.
//!
//! Evaluates `comptime { expr }` blocks at compile time, producing a
//! `ComptimeVal` that can be substituted directly into LLVM IR as a constant.

use std::collections::HashMap;

use crate::ast::{BinOp, Expr, Literal, UnaryOp};
use crate::span::Span;

/// A compile-time constant value.
#[derive(Debug, Clone, PartialEq)]
pub enum ComptimeVal {
    Int(i64),
    Float(f64),
    Bool(bool),
    Str(String),
}

impl ComptimeVal {
    pub fn type_name(&self) -> &'static str {
        match self {
            ComptimeVal::Int(_)   => "i64",
            ComptimeVal::Float(_) => "f64",
            ComptimeVal::Bool(_)  => "bool",
            ComptimeVal::Str(_)   => "str",
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ComptimeError {
    #[error("comptime[E0701]: not comptime-evaluable: {reason}")]
    NotEvaluable { reason: String, span: Span },
    #[error("comptime[E0702]: integer division by zero")]
    DivByZero { span: Span },
    #[error("comptime[E0703]: integer overflow")]
    Overflow { span: Span },
    #[error("comptime: undefined identifier '{name}'")]
    UndefinedIdent { name: String, span: Span },
}

pub struct Evaluator<'a> {
    /// Module-level comptime bindings accumulated so far.
    pub env: HashMap<String, ComptimeVal>,
    /// Pure function definitions available for comptime calls.
    pub fns: &'a HashMap<String, crate::ast::FnDef>,
}

impl<'a> Evaluator<'a> {
    pub fn new(fns: &'a HashMap<String, crate::ast::FnDef>) -> Self {
        Evaluator { env: HashMap::new(), fns }
    }

    pub fn eval(&self, expr: &Expr) -> Result<ComptimeVal, ComptimeError> {
        match expr {
            Expr::Literal(lit) => self.eval_literal(lit),

            Expr::Ident(name) => {
                self.env.get(name).cloned().ok_or_else(|| ComptimeError::UndefinedIdent {
                    name: name.clone(),
                    span: Span::dummy(),
                })
            }

            Expr::BinOp { op, left, right } => {
                let l = self.eval(left)?;
                let r = self.eval(right)?;
                self.eval_binop(op, l, r)
            }

            Expr::UnaryOp { op, operand } => {
                let v = self.eval(operand)?;
                self.eval_unary(op, v)
            }

            Expr::Comptime(inner) => self.eval(inner),

            Expr::Block(stmts) => {
                // For comptime blocks: allow a sequence of let-bindings followed by
                // a final expression.
                let mut local_env = self.env.clone();
                let mut last = None;
                for stmt in stmts {
                    match &stmt.expr {
                        Expr::Let { name, value } => {
                            let val = {
                                let sub = Evaluator { env: local_env.clone(), fns: self.fns };
                                sub.eval(value)?
                            };
                            local_env.insert(name.clone(), val.clone());
                            last = Some(val);
                        }
                        other => {
                            let sub = Evaluator { env: local_env.clone(), fns: self.fns };
                            last = Some(sub.eval(other)?);
                        }
                    }
                }
                last.ok_or_else(|| ComptimeError::NotEvaluable {
                    reason: "empty comptime block".into(),
                    span: Span::dummy(),
                })
            }

            Expr::Call { callee, args } => {
                if let Expr::Ident(name) = callee.as_ref() {
                    // Try a built-in first.
                    if let Some(v) = self.eval_builtin(name, args)? {
                        return Ok(v);
                    }
                    if let Some(fndef) = self.fns.get(name) {
                        return self.eval_fn_call(fndef, args);
                    }
                }
                Err(ComptimeError::NotEvaluable {
                    reason: "non-pure function call in comptime".into(),
                    span: Span::dummy(),
                })
            }

            Expr::If { cond, then, else_ } => {
                let c = self.eval(cond)?;
                match c {
                    ComptimeVal::Bool(true) => self.eval(then),
                    ComptimeVal::Bool(false) => match else_ {
                        Some(e) => self.eval(e),
                        None => Err(ComptimeError::NotEvaluable {
                            reason: "if-without-else in comptime returning a value".into(),
                            span: Span::dummy(),
                        }),
                    },
                    other => Err(ComptimeError::NotEvaluable {
                        reason: format!("if-condition must be bool, got {}", other.type_name()),
                        span: Span::dummy(),
                    }),
                }
            }

            other => Err(ComptimeError::NotEvaluable {
                reason: format!("{}", expr_kind_name(other)),
                span: Span::dummy(),
            }),
        }
    }

    /// Evaluate a built-in function call.  Returns:
    ///   - `Ok(Some(val))` — built-in matched and produced a value.
    ///   - `Ok(None)`      — name is not a built-in; caller should try user fns.
    ///   - `Err(_)`        — built-in matched but evaluation failed.
    fn eval_builtin(
        &self,
        name: &str,
        args: &[Expr],
    ) -> Result<Option<ComptimeVal>, ComptimeError> {
        let arity_err = |expected: usize| ComptimeError::NotEvaluable {
            reason: format!("{name} expects {expected} args, got {}", args.len()),
            span: Span::dummy(),
        };
        match name {
            "str_len" => {
                if args.len() != 1 { return Err(arity_err(1)); }
                match self.eval(&args[0])? {
                    ComptimeVal::Str(s) => Ok(Some(ComptimeVal::Int(s.len() as i64))),
                    other => Err(ComptimeError::NotEvaluable {
                        reason: format!("str_len expects str, got {}", other.type_name()),
                        span: Span::dummy(),
                    }),
                }
            }
            "str_concat" => {
                if args.len() != 2 { return Err(arity_err(2)); }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                match (a, b) {
                    (ComptimeVal::Str(s1), ComptimeVal::Str(s2)) =>
                        Ok(Some(ComptimeVal::Str(s1 + &s2))),
                    (a, b) => Err(ComptimeError::NotEvaluable {
                        reason: format!(
                            "str_concat expects two strs, got {}/{}",
                            a.type_name(), b.type_name()
                        ),
                        span: Span::dummy(),
                    }),
                }
            }
            "str_eq" => {
                if args.len() != 2 { return Err(arity_err(2)); }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                match (a, b) {
                    (ComptimeVal::Str(s1), ComptimeVal::Str(s2)) =>
                        Ok(Some(ComptimeVal::Bool(s1 == s2))),
                    (a, b) => Err(ComptimeError::NotEvaluable {
                        reason: format!(
                            "str_eq expects two strs, got {}/{}",
                            a.type_name(), b.type_name()
                        ),
                        span: Span::dummy(),
                    }),
                }
            }
            "min_i64" => {
                if args.len() != 2 { return Err(arity_err(2)); }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                match (a, b) {
                    (ComptimeVal::Int(x), ComptimeVal::Int(y)) =>
                        Ok(Some(ComptimeVal::Int(x.min(y)))),
                    (a, b) => Err(ComptimeError::NotEvaluable {
                        reason: format!(
                            "min_i64 expects two i64s, got {}/{}",
                            a.type_name(), b.type_name()
                        ),
                        span: Span::dummy(),
                    }),
                }
            }
            "max_i64" => {
                if args.len() != 2 { return Err(arity_err(2)); }
                let a = self.eval(&args[0])?;
                let b = self.eval(&args[1])?;
                match (a, b) {
                    (ComptimeVal::Int(x), ComptimeVal::Int(y)) =>
                        Ok(Some(ComptimeVal::Int(x.max(y)))),
                    (a, b) => Err(ComptimeError::NotEvaluable {
                        reason: format!(
                            "max_i64 expects two i64s, got {}/{}",
                            a.type_name(), b.type_name()
                        ),
                        span: Span::dummy(),
                    }),
                }
            }
            "abs_i64" => {
                if args.len() != 1 { return Err(arity_err(1)); }
                match self.eval(&args[0])? {
                    ComptimeVal::Int(n) => n
                        .checked_abs()
                        .map(|v| Some(ComptimeVal::Int(v)))
                        .ok_or(ComptimeError::Overflow { span: Span::dummy() }),
                    other => Err(ComptimeError::NotEvaluable {
                        reason: format!("abs_i64 expects i64, got {}", other.type_name()),
                        span: Span::dummy(),
                    }),
                }
            }
            "i64_to_str" => {
                if args.len() != 1 { return Err(arity_err(1)); }
                match self.eval(&args[0])? {
                    ComptimeVal::Int(n) => Ok(Some(ComptimeVal::Str(n.to_string()))),
                    other => Err(ComptimeError::NotEvaluable {
                        reason: format!("i64_to_str expects i64, got {}", other.type_name()),
                        span: Span::dummy(),
                    }),
                }
            }
            _ => Ok(None),
        }
    }

    fn eval_literal(&self, lit: &Literal) -> Result<ComptimeVal, ComptimeError> {
        match lit {
            Literal::Int(n)   => Ok(ComptimeVal::Int(*n)),
            Literal::Float(f) => Ok(ComptimeVal::Float(*f)),
            Literal::Bool(b)  => Ok(ComptimeVal::Bool(*b)),
            Literal::Str(s)   => Ok(ComptimeVal::Str(s.clone())),
        }
    }

    fn eval_binop(&self, op: &BinOp, l: ComptimeVal, r: ComptimeVal) -> Result<ComptimeVal, ComptimeError> {
        use ComptimeVal::*;
        match (op, l, r) {
            (BinOp::Add, Int(a), Int(b)) =>
                a.checked_add(b).map(Int).ok_or(ComptimeError::Overflow { span: Span::dummy() }),
            (BinOp::Sub, Int(a), Int(b)) =>
                a.checked_sub(b).map(Int).ok_or(ComptimeError::Overflow { span: Span::dummy() }),
            (BinOp::Mul, Int(a), Int(b)) =>
                a.checked_mul(b).map(Int).ok_or(ComptimeError::Overflow { span: Span::dummy() }),
            (BinOp::Div, Int(a), Int(b)) => {
                if b == 0 { return Err(ComptimeError::DivByZero { span: Span::dummy() }); }
                a.checked_div(b).map(Int).ok_or(ComptimeError::Overflow { span: Span::dummy() })
            }
            (BinOp::Rem, Int(a), Int(b)) => {
                if b == 0 { return Err(ComptimeError::DivByZero { span: Span::dummy() }); }
                Ok(Int(a % b))
            }

            (BinOp::Add, Float(a), Float(b)) => Ok(Float(a + b)),
            (BinOp::Sub, Float(a), Float(b)) => Ok(Float(a - b)),
            (BinOp::Mul, Float(a), Float(b)) => Ok(Float(a * b)),
            (BinOp::Div, Float(a), Float(b)) => Ok(Float(a / b)),

            (BinOp::Add, Str(a), Str(b)) => Ok(Str(a + &b)),

            (BinOp::Eq,    Int(a), Int(b)) => Ok(Bool(a == b)),
            (BinOp::NotEq, Int(a), Int(b)) => Ok(Bool(a != b)),
            (BinOp::Lt,    Int(a), Int(b)) => Ok(Bool(a < b)),
            (BinOp::Gt,    Int(a), Int(b)) => Ok(Bool(a > b)),
            (BinOp::LtEq,  Int(a), Int(b)) => Ok(Bool(a <= b)),
            (BinOp::GtEq,  Int(a), Int(b)) => Ok(Bool(a >= b)),

            (BinOp::And, Bool(a), Bool(b)) => Ok(Bool(a && b)),
            (BinOp::Or,  Bool(a), Bool(b)) => Ok(Bool(a || b)),

            (BinOp::BitAnd, Int(a), Int(b)) => Ok(Int(a & b)),
            (BinOp::BitOr,  Int(a), Int(b)) => Ok(Int(a | b)),
            (BinOp::BitXor, Int(a), Int(b)) => Ok(Int(a ^ b)),
            (BinOp::Shl, Int(a), Int(b)) =>
                Ok(Int(a.checked_shl(b as u32).unwrap_or(0))),
            (BinOp::Shr, Int(a), Int(b)) =>
                Ok(Int(a.checked_shr(b as u32).unwrap_or(if a < 0 { -1 } else { 0 }))),

            (op, l, r) => Err(ComptimeError::NotEvaluable {
                reason: format!("cannot apply {op:?} to {}/{}", l.type_name(), r.type_name()),
                span: Span::dummy(),
            }),
        }
    }

    fn eval_unary(&self, op: &UnaryOp, v: ComptimeVal) -> Result<ComptimeVal, ComptimeError> {
        use ComptimeVal::*;
        match (op, v) {
            (UnaryOp::Neg, Int(n))    => Ok(Int(-n)),
            (UnaryOp::Neg, Float(f))  => Ok(Float(-f)),
            (UnaryOp::Not, Bool(b))   => Ok(Bool(!b)),
            (UnaryOp::BitNot, Int(n)) => Ok(Int(!n)),
            (op, v) => Err(ComptimeError::NotEvaluable {
                reason: format!("cannot apply {op:?} to {}", v.type_name()),
                span: Span::dummy(),
            }),
        }
    }

    fn eval_fn_call(
        &self,
        fndef: &crate::ast::FnDef,
        args: &[Expr],
    ) -> Result<ComptimeVal, ComptimeError> {
        if fndef.params.len() != args.len() {
            return Err(ComptimeError::NotEvaluable {
                reason: "argument count mismatch in comptime call".into(),
                span: Span::dummy(),
            });
        }
        let mut local_env = self.env.clone();
        for (param, arg) in fndef.params.iter().zip(args) {
            let val = self.eval(arg)?;
            local_env.insert(param.name.clone(), val);
        }
        let sub = Evaluator { env: local_env, fns: self.fns };
        sub.eval(&fndef.body)
    }
}

fn expr_kind_name(e: &Expr) -> &'static str {
    match e {
        Expr::Block(_)       => "block",
        Expr::Let { .. }     => "let",
        Expr::Own { .. }     => "own",
        Expr::RefBind { .. } => "ref",
        Expr::Call { .. }    => "call",
        Expr::MethodCall { .. } => "method_call",
        Expr::BinOp { .. }   => "binop",
        Expr::UnaryOp { .. } => "unaryop",
        Expr::Question(_)    => "question",
        Expr::Match { .. }   => "match",
        Expr::If { .. }      => "if",
        Expr::Spawn(_)       => "spawn",
        Expr::Select(_)      => "select",
        Expr::Comptime(_)    => "comptime",
        Expr::Lambda { .. }  => "lambda",
        Expr::Return(_)      => "return",
        Expr::FieldAccess { .. } => "field_access",
        Expr::Index { .. }   => "index",
        Expr::Ident(_)       => "ident",
        Expr::Literal(_)     => "literal",
        Expr::FmtStr { .. }  => "fmt_str",
        Expr::Ok(_)          => "Ok",
        Expr::Err(_)         => "Err",
        Expr::Some(_)        => "Some",
        Expr::None           => "None",
        Expr::Array(_)       => "array",
        Expr::StructLit { .. } => "struct_lit",
        Expr::While { .. }   => "while",
        Expr::WhileLet { .. } => "while_let",
        Expr::Assign { .. }  => "assign",
        Expr::Break          => "break",
        Expr::Continue       => "continue",
        Expr::For { .. }     => "for",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{BinOp, Expr, Literal};

    fn eval(expr: Expr) -> ComptimeVal {
        let fns = HashMap::new();
        Evaluator::new(&fns).eval(&expr).expect("eval failed")
    }

    #[test]
    fn test_int_arith() {
        let e = Expr::BinOp {
            op: BinOp::Add,
            left: Box::new(Expr::Literal(Literal::Int(3))),
            right: Box::new(Expr::Literal(Literal::Int(4))),
        };
        assert_eq!(eval(e), ComptimeVal::Int(7));
    }

    #[test]
    fn test_mul() {
        let e = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(Expr::Literal(Literal::Int(4))),
            right: Box::new(Expr::Literal(Literal::Int(1024))),
        };
        assert_eq!(eval(e), ComptimeVal::Int(4096));
    }

    #[test]
    fn test_nested_via_env() {
        let fns = HashMap::new();
        let mut ev = Evaluator::new(&fns);
        ev.env.insert("KB".to_string(), ComptimeVal::Int(1024));
        let e = Expr::BinOp {
            op: BinOp::Mul,
            left: Box::new(Expr::Ident("KB".to_string())),
            right: Box::new(Expr::Literal(Literal::Int(1024))),
        };
        assert_eq!(ev.eval(&e).unwrap(), ComptimeVal::Int(1_048_576));
    }

    #[test]
    fn test_div_by_zero() {
        let e = Expr::BinOp {
            op: BinOp::Div,
            left: Box::new(Expr::Literal(Literal::Int(1))),
            right: Box::new(Expr::Literal(Literal::Int(0))),
        };
        let fns = HashMap::new();
        assert!(matches!(
            Evaluator::new(&fns).eval(&e),
            Err(ComptimeError::DivByZero { .. })
        ));
    }

    #[test]
    fn test_bool_and_false() {
        let e = Expr::BinOp {
            op: BinOp::And,
            left: Box::new(Expr::Literal(Literal::Bool(true))),
            right: Box::new(Expr::Literal(Literal::Bool(false))),
        };
        assert_eq!(eval(e), ComptimeVal::Bool(false));
    }

    #[test]
    fn test_bool_or_true() {
        let e = Expr::BinOp {
            op: BinOp::Or,
            left: Box::new(Expr::Literal(Literal::Bool(false))),
            right: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert_eq!(eval(e), ComptimeVal::Bool(true));
    }

    #[test]
    fn test_not_bool() {
        let e = Expr::UnaryOp {
            op: crate::ast::UnaryOp::Not,
            operand: Box::new(Expr::Literal(Literal::Bool(true))),
        };
        assert_eq!(eval(e), ComptimeVal::Bool(false));
    }

    #[test]
    fn test_if_true_branch() {
        let e = Expr::If {
            cond: Box::new(Expr::Literal(Literal::Bool(true))),
            then: Box::new(Expr::Literal(Literal::Int(10))),
            else_: Some(Box::new(Expr::Literal(Literal::Int(20)))),
        };
        assert_eq!(eval(e), ComptimeVal::Int(10));
    }

    #[test]
    fn test_if_false_branch() {
        let e = Expr::If {
            cond: Box::new(Expr::Literal(Literal::Bool(false))),
            then: Box::new(Expr::Literal(Literal::Int(10))),
            else_: Some(Box::new(Expr::Literal(Literal::Int(20)))),
        };
        assert_eq!(eval(e), ComptimeVal::Int(20));
    }

    #[test]
    fn test_if_with_computed_cond() {
        // if (3 < 5) { 1 } else { 2 } -> 1
        let e = Expr::If {
            cond: Box::new(Expr::BinOp {
                op: BinOp::Lt,
                left: Box::new(Expr::Literal(Literal::Int(3))),
                right: Box::new(Expr::Literal(Literal::Int(5))),
            }),
            then: Box::new(Expr::Literal(Literal::Int(1))),
            else_: Some(Box::new(Expr::Literal(Literal::Int(2)))),
        };
        assert_eq!(eval(e), ComptimeVal::Int(1));
    }

    #[test]
    fn test_str_len_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("str_len".to_string())),
            args: vec![Expr::Literal(Literal::Str("hello".to_string()))],
        };
        assert_eq!(eval(e), ComptimeVal::Int(5));
    }

    #[test]
    fn test_str_concat_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("str_concat".to_string())),
            args: vec![
                Expr::Literal(Literal::Str("foo".to_string())),
                Expr::Literal(Literal::Str("bar".to_string())),
            ],
        };
        assert_eq!(eval(e), ComptimeVal::Str("foobar".to_string()));
    }

    #[test]
    fn test_str_eq_builtin() {
        let e_eq = Expr::Call {
            callee: Box::new(Expr::Ident("str_eq".to_string())),
            args: vec![
                Expr::Literal(Literal::Str("abc".to_string())),
                Expr::Literal(Literal::Str("abc".to_string())),
            ],
        };
        assert_eq!(eval(e_eq), ComptimeVal::Bool(true));
        let e_neq = Expr::Call {
            callee: Box::new(Expr::Ident("str_eq".to_string())),
            args: vec![
                Expr::Literal(Literal::Str("abc".to_string())),
                Expr::Literal(Literal::Str("xyz".to_string())),
            ],
        };
        assert_eq!(eval(e_neq), ComptimeVal::Bool(false));
    }

    #[test]
    fn test_min_i64_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("min_i64".to_string())),
            args: vec![
                Expr::Literal(Literal::Int(3)),
                Expr::Literal(Literal::Int(7)),
            ],
        };
        assert_eq!(eval(e), ComptimeVal::Int(3));
    }

    #[test]
    fn test_max_i64_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("max_i64".to_string())),
            args: vec![
                Expr::Literal(Literal::Int(3)),
                Expr::Literal(Literal::Int(7)),
            ],
        };
        assert_eq!(eval(e), ComptimeVal::Int(7));
    }

    #[test]
    fn test_abs_i64_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("abs_i64".to_string())),
            args: vec![Expr::Literal(Literal::Int(-42))],
        };
        assert_eq!(eval(e), ComptimeVal::Int(42));
    }

    #[test]
    fn test_i64_to_str_builtin() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("i64_to_str".to_string())),
            args: vec![Expr::Literal(Literal::Int(123))],
        };
        assert_eq!(eval(e), ComptimeVal::Str("123".to_string()));
    }

    #[test]
    fn test_builtin_arity_error() {
        let e = Expr::Call {
            callee: Box::new(Expr::Ident("str_len".to_string())),
            args: vec![],
        };
        let fns = HashMap::new();
        assert!(matches!(
            Evaluator::new(&fns).eval(&e),
            Err(ComptimeError::NotEvaluable { .. })
        ));
    }
}
