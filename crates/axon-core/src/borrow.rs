//! Phase 3 — Lite borrow checker.
//!
//! Enforces single-ownership and move semantics within function bodies using a
//! lexical (no lifetime parameters) forward pass.  This is a pure analysis
//! pass: it emits diagnostics but does not mutate the AST or LLVM IR.

use std::collections::HashMap;

use crate::ast::{Expr, FnDef};
use crate::span::Span;
use crate::types::Type;

// ── Error codes ───────────────────────────────────────────────────────────────

#[derive(Debug, Clone, thiserror::Error)]
pub enum BorrowError {
    #[error("borrow[E0601]: use of moved value '{name}'")]
    UseAfterMove { name: String, span: Span },
    #[error("borrow[E0602]: cannot move '{name}': value is currently borrowed")]
    MoveBorrowed { name: String, span: Span },
    #[error("borrow[E0603]: '{name}' borrowed here, move attempted at use site")]
    BorrowConflict { name: String, span: Span },
}

impl BorrowError {
    pub fn span(&self) -> Span {
        match self {
            BorrowError::UseAfterMove { span, .. }
            | BorrowError::MoveBorrowed { span, .. }
            | BorrowError::BorrowConflict { span, .. } => *span,
        }
    }
}

// ── Binding state ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
enum BindingState {
    Owned,
    Moved,
    Borrowed,
    Ref(String),
}

/// Types that are `Copy` — assignment copies the value instead of moving it.
fn is_copy(ty: &Type) -> bool {
    matches!(
        ty,
        Type::I8  | Type::I16 | Type::I32 | Type::I64
        | Type::U8  | Type::U16 | Type::U32 | Type::U64
        | Type::F32 | Type::F64
        | Type::Bool
    )
}

/// Best-effort type inference for a binding's RHS, used to track Copy status
/// without access to the full inference context.
/// Returns `None` when the type cannot be determined (caller treats as Copy).
fn infer_expr_type(expr: &Expr, known: &HashMap<String, Type>) -> Option<Type> {
    match expr {
        Expr::Literal(crate::ast::Literal::Int(_))   => Some(Type::I64),
        Expr::Literal(crate::ast::Literal::Float(_)) => Some(Type::F64),
        Expr::Literal(crate::ast::Literal::Bool(_))  => Some(Type::Bool),
        Expr::Literal(crate::ast::Literal::Str(_))   => Some(Type::Str),
        Expr::Ident(name)                            => known.get(name).cloned(),
        Expr::StructLit { name, .. }                 => Some(Type::Struct(name.clone())),
        Expr::BinOp { left, .. }                     => infer_expr_type(left, known),
        Expr::Comptime(inner)                        => infer_expr_type(inner, known),
        Expr::Block(stmts) if !stmts.is_empty() => {
            infer_expr_type(&stmts.last().unwrap().expr, known)
        }
        _ => None,
    }
}

// ── Ownership graph ───────────────────────────────────────────────────────────

pub struct OwnershipGraph {
    bindings: HashMap<String, BindingState>,
    /// Stack of lexical scopes; each entry is the list of names introduced.
    scopes: Vec<Vec<String>>,
    pub errors: Vec<BorrowError>,
    /// Semantic types from the type checker (used for Copy-type detection).
    types: HashMap<String, Type>,
    /// Span of the statement currently being checked (updated at each stmt boundary).
    current_span: Span,
}

impl OwnershipGraph {
    fn new(param_types: HashMap<String, Type>) -> Self {
        let mut g = OwnershipGraph {
            bindings: HashMap::new(),
            scopes: vec![Vec::new()],
            errors: Vec::new(),
            types: param_types.clone(),
            current_span: Span::dummy(),
        };
        // Parameters start as Owned.
        for (name, _) in &param_types {
            g.bindings.insert(name.clone(), BindingState::Owned);
            g.scopes[0].push(name.clone());
        }
        g
    }

    fn push_scope(&mut self) {
        self.scopes.push(Vec::new());
    }

    fn pop_scope(&mut self) {
        if let Some(scope) = self.scopes.pop() {
            for name in &scope {
                // When a ref goes out of scope, restore the owner to Owned.
                if let Some(BindingState::Ref(owner)) = self.bindings.get(name).cloned() {
                    self.bindings.insert(owner, BindingState::Owned);
                }
                self.bindings.remove(name);
            }
        }
    }

    fn introduce(&mut self, name: &str, state: BindingState, ty: Option<Type>) {
        self.bindings.insert(name.to_string(), state);
        if let Some(scope) = self.scopes.last_mut() {
            scope.push(name.to_string());
        }
        if let Some(t) = ty {
            self.types.insert(name.to_string(), t);
        }
    }

    /// Record a move of `name` — sets its state to Moved.
    fn move_binding(&mut self, name: &str, span: Span) {
        match self.bindings.get(name).cloned() {
            Some(BindingState::Moved) => {
                self.errors.push(BorrowError::UseAfterMove { name: name.to_string(), span });
            }
            Some(BindingState::Borrowed) => {
                self.errors.push(BorrowError::MoveBorrowed { name: name.to_string(), span });
            }
            Some(BindingState::Owned) => {
                let ty = self.types.get(name).cloned();
                // If type is unknown, assume Copy (avoids false positives for
                // local variables whose types aren't tracked in the borrow context).
                if ty.map(|t| is_copy(&t)).unwrap_or(true) {
                    // Copy type — no move
                } else {
                    self.bindings.insert(name.to_string(), BindingState::Moved);
                }
            }
            Some(BindingState::Ref(_)) | None => {}
        }
    }

    /// Walk `expr` in a "read" context (not a move position).
    fn check_read(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => {
                if matches!(self.bindings.get(name), Some(BindingState::Moved)) {
                    self.errors.push(BorrowError::UseAfterMove {
                        name: name.clone(),
                        span: self.current_span,
                    });
                }
            }
            _ => self.check_expr(expr),
        }
    }

    /// Walk `expr` handling ownership of sub-expressions.
    fn check_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Ident(name) => {
                // Reading an ident in expression position — not a move unless in a move context.
                if matches!(self.bindings.get(name), Some(BindingState::Moved)) {
                    self.errors.push(BorrowError::UseAfterMove {
                        name: name.clone(),
                        span: self.current_span,
                    });
                }
            }

            Expr::Let { name, value } => {
                let ty = infer_expr_type(value, &self.types);
                self.check_move_expr(value);
                self.introduce(name, BindingState::Owned, ty);
            }

            Expr::Own { name, value } => {
                let ty = infer_expr_type(value, &self.types);
                self.check_move_expr(value);
                self.introduce(name, BindingState::Owned, ty);
            }

            Expr::RefBind { name, value } => {
                if let Expr::Ident(src) = value.as_ref() {
                    match self.bindings.get(src).cloned() {
                        Some(BindingState::Owned) => {
                            self.bindings.insert(src.to_string(), BindingState::Borrowed);
                            self.introduce(name, BindingState::Ref(src.clone()), None);
                        }
                        Some(BindingState::Moved) => {
                            self.errors.push(BorrowError::UseAfterMove {
                                name: src.clone(),
                                span: self.current_span,
                            });
                        }
                        _ => {
                            self.introduce(name, BindingState::Owned, None);
                        }
                    }
                } else {
                    self.check_expr(value);
                    self.introduce(name, BindingState::Owned, None);
                }
            }

            Expr::Assign { name, value } => {
                self.check_move_expr(value);
                // Re-binding moves old value out if it was owned.
                self.bindings.insert(name.to_string(), BindingState::Owned);
            }

            Expr::Block(stmts) => {
                self.push_scope();
                for stmt in stmts {
                    if !stmt.span.is_dummy() {
                        self.current_span = stmt.span;
                    }
                    self.check_expr(&stmt.expr);
                }
                self.pop_scope();
            }

            Expr::If { cond, then, else_ } => {
                self.check_read(cond);
                // Check both branches; union moved bindings.
                let snapshot = self.bindings.clone();
                self.check_expr(then);
                let after_then = self.bindings.clone();
                self.bindings = snapshot.clone();
                if let Some(e) = else_ {
                    self.check_expr(e);
                }
                let after_else = self.bindings.clone();
                // Union: any binding moved in either branch is moved afterwards.
                self.bindings = snapshot;
                for (name, state) in &after_then {
                    if *state == BindingState::Moved {
                        self.bindings.insert(name.clone(), BindingState::Moved);
                    }
                }
                for (name, state) in &after_else {
                    if *state == BindingState::Moved {
                        self.bindings.insert(name.clone(), BindingState::Moved);
                    }
                }
            }

            Expr::Call { callee, args } => {
                self.check_read(callee);
                for arg in args {
                    self.check_read(arg);
                }
            }

            Expr::MethodCall { receiver, args, .. } => {
                self.check_read(receiver);
                for arg in args {
                    self.check_read(arg);
                }
            }

            Expr::Return(Some(v)) => {
                self.check_move_expr(v);
            }

            Expr::BinOp { left, right, .. } => {
                self.check_read(left);
                self.check_read(right);
            }

            Expr::UnaryOp { operand, .. } => self.check_read(operand),
            Expr::Question(inner) => self.check_read(inner),
            Expr::FieldAccess { receiver, .. } => self.check_read(receiver),
            Expr::Index { receiver, index } => {
                self.check_read(receiver);
                self.check_read(index);
            }
            Expr::FmtStr { .. } | Expr::Literal(_) | Expr::None => {}
            Expr::Ok(v) | Expr::Err(v) | Expr::Some(v) => self.check_read(v),
            Expr::Array(elems) => {
                for e in elems { self.check_read(e); }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields { self.check_read(v); }
            }
            Expr::While { cond, body } => {
                self.check_read(cond);
                self.push_scope();
                for stmt in body {
                    if !stmt.span.is_dummy() {
                        self.current_span = stmt.span;
                    }
                    self.check_expr(&stmt.expr);
                }
                self.pop_scope();
            }
            Expr::For { start, end, body, .. } => {
                self.check_read(start);
                self.check_read(end);
                self.push_scope();
                for stmt in body {
                    if !stmt.span.is_dummy() {
                        self.current_span = stmt.span;
                    }
                    self.check_expr(&stmt.expr);
                }
                self.pop_scope();
            }
            Expr::Lambda { body, .. } => {
                // Lambda bodies are checked separately.
                let _ = body;
            }
            Expr::Spawn(body) => self.check_expr(body),
            Expr::Select(arms) => {
                for arm in arms {
                    self.check_read(&arm.recv);
                    self.check_expr(&arm.body);
                }
            }
            Expr::Comptime(inner) => self.check_read(inner),
            Expr::Match { subject, arms } => {
                self.check_read(subject);
                for arm in arms {
                    self.push_scope();
                    self.check_expr(&arm.body);
                    self.pop_scope();
                }
            }
            Expr::Return(None) | Expr::Break | Expr::Continue => {}
        }
    }

    /// Check `expr` in a "move" context (the value will be moved out of its owner).
    fn check_move_expr(&mut self, expr: &Expr) {
        if let Expr::Ident(name) = expr {
            self.move_binding(name, self.current_span);
        } else {
            self.check_expr(expr);
        }
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Run the borrow checker on a single function definition.
/// Returns the list of borrow errors found.
pub fn check_fn(fndef: &FnDef, param_types: HashMap<String, Type>) -> Vec<BorrowError> {
    let mut graph = OwnershipGraph::new(param_types);
    graph.current_span = fndef.span;
    graph.check_expr(&fndef.body);
    graph.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::{Expr, FnDef, Stmt};
    use crate::span::Span;

    fn make_fn(body: Expr) -> FnDef {
        FnDef {
            public: false,
            name: "test".into(),
            generic_params: vec![],
            generic_bounds: vec![],
            params: vec![],
            return_type: None,
            body,
            attrs: vec![],
            span: Span::dummy(),
        }
    }

    #[test]
    fn test_move_detected() {
        // own b = s; use s  — should be E0601
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::Own {
                name: "b".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
            // Reading s after it was moved — this is a move of a Str (non-Copy)
            Stmt::simple(Expr::Ident("s".into())),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(!errs.is_empty(), "expected E0601");
        assert!(matches!(errs[0], BorrowError::UseAfterMove { .. }));
    }

    #[test]
    fn test_copy_types_not_moved() {
        // let a = 5; let b = a; use a  — i64 is Copy, no error
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let {
                name: "a".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Int(5))),
            }),
            Stmt::simple(Expr::Own {
                name: "b".into(),
                value: Box::new(Expr::Ident("a".into())),
            }),
            Stmt::simple(Expr::Ident("a".into())),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("a".into(), Type::I64);
        let errs = check_fn(&fndef, types);
        assert!(errs.is_empty(), "Copy type should not be moved: {errs:?}");
    }

    // ── Negative tests: clean programs should not raise borrow errors ────────

    #[test]
    fn test_clean_let_no_errors() {
        // let x = 42; x + 1  — i64 Copy, no moves
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let {
                name: "x".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Int(42))),
            }),
            Stmt::simple(Expr::BinOp {
                op: crate::ast::BinOp::Add,
                left: Box::new(Expr::Ident("x".into())),
                right: Box::new(Expr::Literal(crate::ast::Literal::Int(1))),
            }),
        ]);
        let fndef = make_fn(body);
        let errs = check_fn(&fndef, HashMap::new());
        assert!(errs.is_empty(), "Clean let-binding should produce no errors: {errs:?}");
    }

    #[test]
    fn test_own_binding_then_consume_no_error() {
        // own s = "hello"; own b = s  — s moved into b exactly once, no use after
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::Own {
                name: "b".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(errs.is_empty(), "Single move should not raise an error: {errs:?}");
    }

    #[test]
    fn test_ref_binding_no_error() {
        // own s = "hello"; ref r = s  — borrow without move
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::RefBind {
                name: "r".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(errs.is_empty(), "ref-binding alone should produce no errors: {errs:?}");
    }

    #[test]
    fn test_move_while_borrowed() {
        // own s = "hello"; ref r = s; own b = s  — moving while borrowed → E0602
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::RefBind {
                name: "r".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
            Stmt::simple(Expr::Own {
                name: "b".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(
            errs.iter().any(|e| matches!(e, BorrowError::MoveBorrowed { .. })),
            "expected E0602 MoveBorrowed, got {errs:?}",
        );
    }

    #[test]
    fn test_copy_used_repeatedly_no_error() {
        // let a = 1; a; a; a  — i64 is Copy, multiple uses fine
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let {
                name: "a".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Int(1))),
            }),
            Stmt::simple(Expr::Ident("a".into())),
            Stmt::simple(Expr::Ident("a".into())),
            Stmt::simple(Expr::Ident("a".into())),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("a".into(), Type::I64);
        let errs = check_fn(&fndef, types);
        assert!(errs.is_empty(), "Copy type can be reused: {errs:?}");
    }

    #[test]
    fn test_empty_function_no_error() {
        // fn test() {}  — empty body, no errors
        let fndef = make_fn(Expr::Block(vec![]));
        let errs = check_fn(&fndef, HashMap::new());
        assert!(errs.is_empty(), "Empty function should produce no errors: {errs:?}");
    }
}
