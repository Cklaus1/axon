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
                    if let Some(fndef) = self.fns.get(name) {
                        return self.eval_fn_call(fndef, args);
                    }
                }
                Err(ComptimeError::NotEvaluable {
                    reason: "non-pure function call in comptime".into(),
                    span: Span::dummy(),
                })
            }

            other => Err(ComptimeError::NotEvaluable {
                reason: format!("{}", expr_kind_name(other)),
                span: Span::dummy(),
            }),
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

            (op, l, r) => Err(ComptimeError::NotEvaluable {
                reason: format!("cannot apply {op:?} to {}/{}", l.type_name(), r.type_name()),
                span: Span::dummy(),
            }),
        }
    }

    fn eval_unary(&self, op: &UnaryOp, v: ComptimeVal) -> Result<ComptimeVal, ComptimeError> {
        use ComptimeVal::*;
        match (op, v) {
            (UnaryOp::Neg, Int(n))   => Ok(Int(-n)),
            (UnaryOp::Neg, Float(f)) => Ok(Float(-f)),
            (UnaryOp::Not, Bool(b))  => Ok(Bool(!b)),
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
}
