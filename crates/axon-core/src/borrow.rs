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
    #[error(
        "borrow[E0601]: use of moved value `{name}` — \
         the value was moved out earlier and is no longer accessible. \
         hint: use `ref` to borrow without moving, or clone the value before the move"
    )]
    UseAfterMove { name: String, span: Span },
    #[error(
        "borrow[E0602]: cannot move out of `{name}` — \
         the value is currently borrowed by an outstanding `ref` binding. \
         hint: drop the borrow (let it go out of scope) before moving the value"
    )]
    MoveBorrowed { name: String, span: Span },
    #[error(
        "borrow[E0603]: conflict on `{name}` — value is borrowed here but a move was \
         attempted at the use site. hint: re-borrow with `ref` or restructure to avoid \
         overlapping the borrow with the move"
    )]
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
    // R13: a native *value* handle (`resource = false`) is Copy; a *resource*
    // handle is affine (NOT Copy) so use-after-consume is E0601 at compile time.
    if crate::native::is_native_handle(ty) {
        return !crate::native::is_resource_handle(ty);
    }
    matches!(
        ty,
        Type::I8
            | Type::I16
            | Type::I32
            | Type::I64
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::F32
            | Type::F64
            | Type::Bool
    )
}

/// Whether an `if` branch never falls through: its block's last statement is
/// a `return`, or a call that reaches the builtin `exit` (CS-6). A call with a
/// `&mut` argument runs a user `fn exit` instead (CS-4), which returns. So
/// does a call to a LOCAL binding named `exit` (dispatch is local closure ->
/// builtin -> user fn): `exit_local` says one is in scope at `e`, and a
/// block's own `let exit = ..` before its last statement brings one in.
fn branch_diverges(e: &Expr, exit_local: bool) -> bool {
    match e {
        Expr::Return(_) => true,
        Expr::Call { callee, args, .. } => {
            !exit_local
                && matches!(callee.as_ref(), Expr::Ident(n) if n == "exit")
                && crate::builtins::call_prefers_builtin(
                    "exit",
                    crate::builtins::call_has_mut_arg(args),
                )
        }
        Expr::Block(stmts) => stmts_diverge(stmts, exit_local),
        _ => false,
    }
}

/// [`branch_diverges`] for a statement list: its last statement diverges,
/// with an `exit` bound by an earlier statement counted as local.
fn stmts_diverge(stmts: &[crate::ast::Stmt], exit_local: bool) -> bool {
    stmts.split_last().is_some_and(|(last, init)| {
        let binds_exit = init.iter().any(|s| {
            matches!(&s.expr, Expr::Let { name, .. } | Expr::Own { name, .. } | Expr::RefBind { name, .. } if name == "exit")
        });
        branch_diverges(&last.expr, exit_local || binds_exit)
    })
}

/// Whether `e` contains a `break` or `continue` that leaves `e`, i.e. one not
/// bound by a loop nested inside `e`. Lambda bodies are not entered (they are
/// checked separately); `spawn` and `select` bodies are (a jump there counts
/// as escaping, which keeps moves: the refusing direction).
fn has_escaping_jump(e: &Expr) -> bool {
    let mut work = vec![e];
    let mut kids = Vec::new();
    while let Some(e) = work.pop() {
        match e {
            Expr::Break | Expr::Continue => return true,
            Expr::Lambda { .. } => {}
            // A nested loop binds the jumps of its body; its header does not.
            Expr::While { cond, .. } => work.push(cond),
            Expr::WhileLet { expr, .. } => work.push(expr),
            Expr::For { start, end, .. } => work.extend([&**start, &**end]),
            _ => {
                crate::ast::children(e, &mut kids);
                work.append(&mut kids);
            }
        }
    }
    false
}

/// Whether an `if` branch's only escaping `break`/`continue` is its final
/// statement. That jump then leaves with the branch's end state (CS4-1).
fn jump_is_final(e: &Expr) -> bool {
    match e {
        Expr::Break | Expr::Continue => true,
        Expr::Block(stmts) => stmts.split_last().is_some_and(|(last, init)| {
            matches!(last.expr, Expr::Break | Expr::Continue)
                && !init.iter().any(|s| has_escaping_jump(&s.expr))
        }),
        _ => false,
    }
}

/// Whether `pat` binds the name `exit` (a call to it then runs the bound
/// value, not the builtin, so it does not diverge).
fn pattern_binds_exit(pat: &crate::ast::Pattern) -> bool {
    let mut names = Vec::new();
    crate::resolver::collect_pattern_binding_names(pat, &mut names);
    names.iter().any(|n| n == "exit")
}

/// Whether a loop body never falls out of the loop: its last statement
/// diverges and no `break`/`continue` leaves it. Every iteration that starts
/// then ends in `return`/`exit`, so none of the body's moves reach the code
/// after the loop, and the zero-iteration path keeps the state before it.
fn loop_body_diverges(body: &[crate::ast::Stmt], exit_local: bool) -> bool {
    stmts_diverge(body, exit_local) && !body.iter().any(|s| has_escaping_jump(&s.expr))
}

/// A name's state after a match, from two live arms' end states: `Moved` if
/// either arm moved it, `Borrowed` if either still holds a `ref` of it (the
/// refusing direction), else the later arm's (the state they share).
fn join_state(a: &BindingState, b: BindingState) -> BindingState {
    if *a == BindingState::Moved || b == BindingState::Moved {
        BindingState::Moved
    } else if *a == BindingState::Borrowed {
        BindingState::Borrowed
    } else {
        b
    }
}

/// Best-effort type inference for a binding's RHS, used to track Copy status
/// without access to the full inference context.
/// Returns `None` when the type cannot be determined (caller treats as Copy).
fn infer_expr_type(expr: &Expr, known: &HashMap<String, Type>) -> Option<Type> {
    // A `BinOp` has its left operand's type: skip down a long left-nested chain
    // with a loop, not one frame per term (AX-64).
    let mut expr = expr;
    while let Expr::BinOp { left, .. } = expr {
        expr = left;
    }
    match expr {
        Expr::Literal(crate::ast::Literal::Int(_)) => Some(Type::I64),
        Expr::Literal(crate::ast::Literal::Float(_)) => Some(Type::F64),
        Expr::Literal(crate::ast::Literal::Bool(_)) => Some(Type::Bool),
        Expr::Literal(crate::ast::Literal::Str(_)) => Some(Type::Str),
        Expr::Ident(name) => known.get(name).cloned(),
        Expr::StructLit { name, .. } => Some(Type::Struct(name.clone())),
        // R13: a native `M::fn(...)` call binds its FFI return type — a resource
        // handle is non-Copy here, so `let win = gfx::window_open(...)` makes
        // `win` affine and use-after-consume is E0601 at compile time.
        Expr::Call { callee, .. } => {
            if let Expr::StructLit { name, fields } = callee.as_ref() {
                if fields.is_empty() {
                    if let Some((_m, nf)) = crate::native::resolve_call(name) {
                        return Some(nf.ret.to_type());
                    }
                }
            }
            None
        }
        Expr::BinOp { .. } => unreachable!("the loop above stops at a non-BinOp"),
        Expr::Comptime(inner) => infer_expr_type(inner, known),
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
    /// How many enclosing match arms or loops have a pattern that binds
    /// `exit`. The checker keeps no pattern bindings in `bindings`, so this
    /// is how a call to such an `exit` is known to run the bound value.
    exit_bound: usize,
}

impl OwnershipGraph {
    fn new(param_types: HashMap<String, Type>) -> Self {
        let mut g = OwnershipGraph {
            bindings: HashMap::new(),
            scopes: vec![Vec::new()],
            errors: Vec::new(),
            types: param_types.clone(),
            current_span: Span::dummy(),
            exit_bound: 0,
        };
        // Parameters start as Owned.
        for name in param_types.keys() {
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
                self.errors.push(BorrowError::UseAfterMove {
                    name: name.to_string(),
                    span,
                });
            }
            Some(BindingState::Borrowed) => {
                self.errors.push(BorrowError::MoveBorrowed {
                    name: name.to_string(),
                    span,
                });
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

            // Phase 6: walk the handler arm bodies and the `with` body for moves.
            Expr::WithHandler { handler, body } => {
                if let crate::ast::HandlerExpr::Inline { arms, return_arm } = handler.as_ref() {
                    for arm in arms.iter().chain(return_arm.as_deref()) {
                        self.check_expr(&arm.body);
                    }
                }
                self.check_expr(body);
            }

            Expr::Let { name, value, .. } => {
                let ty = infer_expr_type(value, &self.types);
                // Arrays are values: `let b = a` copies an array or `&[T]`
                // binding (NH-3 (d), CS-7); `str` and structs still move.
                if matches!(ty, Some(Type::Slice(_))) {
                    self.check_read(value);
                } else {
                    self.check_move_expr(value);
                }
                self.introduce(name, BindingState::Owned, ty);
            }

            Expr::Own { name, value, .. } => {
                let ty = infer_expr_type(value, &self.types);
                self.check_move_expr(value);
                self.introduce(name, BindingState::Owned, ty);
            }

            Expr::RefBind { name, value, .. } => {
                if let Expr::Ident(src) = value.as_ref() {
                    match self.bindings.get(src).cloned() {
                        Some(BindingState::Owned) => {
                            self.bindings
                                .insert(src.to_string(), BindingState::Borrowed);
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
            Expr::AssignTo { place, value } => {
                // The value is moved into the place; the place's base is read.
                self.check_move_expr(value);
                self.check_read(place);
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
                let after_then = std::mem::replace(&mut self.bindings, snapshot.clone());
                if let Some(e) = else_ {
                    self.check_expr(e);
                }
                let after_else = std::mem::replace(&mut self.bindings, snapshot);
                let exit_local = self.exit_bound > 0 || self.bindings.contains_key("exit");
                let then_live = !branch_diverges(then, exit_local);
                let else_live = !else_
                    .as_deref()
                    .is_some_and(|e| branch_diverges(e, exit_local));
                let live = [
                    then_live.then_some(&after_then),
                    else_live.then_some(&after_else),
                ];
                // Re-bind join (CS4-1, like the match join): a name moved
                // before the `if` that every live branch re-binds is re-bound
                // after it. Not when a `break`/`continue` leaves a branch
                // before its end: that path carries the moved state past the
                // loop (O-AF-02). A final jump leaves with the branch's end
                // state, so it joins. A name a branch no longer holds (a
                // popped shadowing `let`) keeps its state from before the
                // `if`: `Moved`. With no live branch nothing is joined.
                let blocks_join = |b: &Expr| has_escaping_jump(b) && !jump_is_final(b);
                if !blocks_join(then) && !else_.as_deref().is_some_and(blocks_join) {
                    for (name, state) in self.bindings.iter_mut() {
                        if *state != BindingState::Moved {
                            continue;
                        }
                        let joined = live
                            .into_iter()
                            .flatten()
                            .map(|end| end.get(name).cloned().unwrap_or(BindingState::Moved))
                            .reduce(|a, b| join_state(&a, b));
                        if let Some(j) = joined.filter(|j| *j != BindingState::Moved) {
                            *state = j;
                        }
                    }
                }
                // Union: any binding moved in a branch that falls through is
                // moved afterwards. A branch ending in `return` or `exit(..)`
                // never reaches the code after the `if`, so its moves are
                // discarded (CS-6). `break`/`continue` branches keep theirs:
                // their state flows past the loop. (A name the join above
                // re-bound is `Moved` in no live branch, so the order of the
                // two steps does not matter.)
                for (name, state) in live.into_iter().flatten().flatten() {
                    if *state == BindingState::Moved {
                        self.bindings.insert(name.clone(), BindingState::Moved);
                    }
                }
            }

            Expr::Call { callee, args, .. } => {
                self.check_read(callee);
                // R13: a native `M::fn(...)` consumes resource-handle args passed
                // in `Move` mode (e.g. `window_close(win)`) — this is the affine
                // drop site. A `Borrow` (ref) param leaves the owner valid. A
                // later use of a consumed handle is the existing E0601 move check.
                let native = if let Expr::StructLit { name, fields } = callee.as_ref() {
                    if fields.is_empty() {
                        crate::native::resolve_call(name).map(|(_m, nf)| nf)
                    } else {
                        None
                    }
                } else {
                    None
                };
                for (i, arg) in args.iter().enumerate() {
                    // A `Move`-mode param consumes the arg ONLY when it is a
                    // resource handle (the affine drop site). A `Move` scalar /
                    // `str` / `[scalar]` is passed by value (the frozen ABI copies
                    // the descriptor) and must NOT consume the Axon binding —
                    // otherwise `let s = ...; native::f(s); native::g(s)` would
                    // wrongly E0601 on a plain str (R22).
                    let consumes = native
                        .and_then(|nf| nf.params.get(i))
                        .map(|(ty, mode)| {
                            *mode == crate::native::ParamMode::Move
                                && matches!(
                                    ty,
                                    crate::native::FfiType::Handle { resource: true, .. }
                                )
                        })
                        .unwrap_or(false);
                    if consumes {
                        // Move position: consume the arg (only matters for a
                        // non-Copy resource handle; Copy args are no-ops).
                        self.check_move_expr(arg);
                    } else {
                        self.check_read(arg);
                    }
                }
            }

            Expr::MethodCall { receiver, args, .. } => {
                self.check_read(receiver);
                for arg in args {
                    self.check_read(arg);
                }
            }

            Expr::Return(Some(v)) => {
                // A return ends the frame, so every `ref` of it is dead:
                // returning a Borrowed owner is no move error (O-HX3-03, the
                // collection `for` borrows its collection). A Moved one is.
                let borrowed = matches!(v.as_ref(), Expr::Ident(n)
                    if self.bindings.get(n) == Some(&BindingState::Borrowed));
                if !borrowed {
                    self.check_move_expr(v);
                }
            }

            // The left spine of a long chain is walked with a loop (AX-64).
            Expr::BinOp { left, right, .. } => {
                let mut rights = vec![&**right];
                let mut cur = &**left;
                while let Expr::BinOp { left, right, .. } = cur {
                    rights.push(&**right);
                    cur = left;
                }
                self.check_read(cur);
                for r in rights.into_iter().rev() {
                    self.check_read(r);
                }
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
            Expr::Array(elems) | Expr::Tuple(elems) => {
                for e in elems {
                    self.check_read(e);
                }
            }
            Expr::StructLit { fields, .. } => {
                for (_, v) in fields {
                    self.check_read(v);
                }
            }
            Expr::While { cond, body } => {
                self.check_read(cond);
                self.check_loop_body(body, false);
            }
            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => {
                self.check_read(expr);
                self.check_loop_body(body, pattern_binds_exit(pattern));
            }
            Expr::For {
                var,
                start,
                end,
                body,
                ..
            } => {
                self.check_read(start);
                self.check_read(end);
                self.check_loop_body(body, var == "exit");
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
                self.check_match_arms(arms);
            }
            Expr::Return(None) | Expr::Break | Expr::Continue | Expr::InlineAsm { .. } => {}
        }
    }

    /// Walk a loop body. Its moves flow past the loop, except when the body
    /// never falls out of the loop (see [`loop_body_diverges`]): then the
    /// state after the loop is the state before it.
    fn check_loop_body(&mut self, body: &[crate::ast::Stmt], binds_exit: bool) {
        let exit_local = binds_exit || self.exit_bound > 0 || self.bindings.contains_key("exit");
        let snapshot = loop_body_diverges(body, exit_local).then(|| self.bindings.clone());
        self.exit_bound += usize::from(binds_exit);
        self.push_scope();
        for stmt in body {
            if !stmt.span.is_dummy() {
                self.current_span = stmt.span;
            }
            self.check_expr(&stmt.expr);
        }
        self.pop_scope();
        self.exit_bound -= usize::from(binds_exit);
        if let Some(snapshot) = snapshot {
            self.bindings = snapshot;
        }
    }

    /// Match arms are alternatives (O-HX3-02): each arm starts from the state
    /// its run reaches, which is the state before the match plus the moves
    /// of every guard tried before it (a guard runs whenever its arm is
    /// tried, even when it fails). An arm that diverges syntactically and
    /// has no escaping `break`/`continue` never reaches the code after the
    /// match, so it is not live. After the match each name is `Moved` if a
    /// live arm ends with it moved or a guard moved it, else the state the
    /// live arms end with; with no live arm it is the state after the guards.
    fn check_match_arms(&mut self, arms: &[crate::ast::MatchArm]) {
        let snapshot = self.bindings.clone();
        let exit_local = self.exit_bound > 0 || snapshot.contains_key("exit");
        let mut tried = snapshot.clone();
        let mut joined: Option<HashMap<String, BindingState>> = None;
        for arm in arms {
            self.bindings = tried.clone();
            self.push_scope();
            if let Some(guard) = &arm.guard {
                self.check_read(guard);
                tried = self.bindings.clone();
            }
            let binds_exit = pattern_binds_exit(&arm.pattern);
            self.exit_bound += usize::from(binds_exit);
            self.check_expr(&arm.body);
            self.exit_bound -= usize::from(binds_exit);
            self.pop_scope();
            let live = !branch_diverges(&arm.body, exit_local || binds_exit)
                || has_escaping_jump(&arm.body);
            if !live {
                continue;
            }
            let end = std::mem::take(&mut self.bindings);
            let Some(j) = joined.as_mut() else {
                joined = Some(end);
                continue;
            };
            for (name, state) in end {
                match j.get_mut(&name) {
                    None => {
                        j.insert(name, state);
                    }
                    Some(cur) => *cur = join_state(cur, state),
                }
            }
        }
        self.bindings = match joined {
            None => tried,
            Some(mut j) => {
                for (name, state) in tried {
                    if state == BindingState::Moved
                        && snapshot.get(&name) != Some(&BindingState::Moved)
                    {
                        j.insert(name, BindingState::Moved);
                    }
                }
                j
            }
        };
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
            contained: None,
            verify: None,
            effect_row: None,
            span: Span::dummy(),
        }
    }

    #[test]
    fn test_move_detected() {
        // own b = s; use s  — should be E0601
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                ty: None,
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::Own {
                ty: None,
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
                ty: None,
                name: "a".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Int(5))),
            }),
            Stmt::simple(Expr::Own {
                ty: None,
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
                ty: None,
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
        assert!(
            errs.is_empty(),
            "Clean let-binding should produce no errors: {errs:?}"
        );
    }

    #[test]
    fn test_own_binding_then_consume_no_error() {
        // own s = "hello"; own b = s  — s moved into b exactly once, no use after
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                ty: None,
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::Own {
                ty: None,
                name: "b".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(
            errs.is_empty(),
            "Single move should not raise an error: {errs:?}"
        );
    }

    #[test]
    fn test_ref_binding_no_error() {
        // own s = "hello"; ref r = s  — borrow without move
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                ty: None,
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::RefBind {
                ty: None,
                name: "r".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(
            errs.is_empty(),
            "ref-binding alone should produce no errors: {errs:?}"
        );
    }

    #[test]
    fn test_move_while_borrowed() {
        // own s = "hello"; ref r = s; own b = s  — moving while borrowed → E0602
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Own {
                ty: None,
                name: "s".into(),
                value: Box::new(Expr::Literal(crate::ast::Literal::Str("hello".into()))),
            }),
            Stmt::simple(Expr::RefBind {
                ty: None,
                name: "r".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
            Stmt::simple(Expr::Own {
                ty: None,
                name: "b".into(),
                value: Box::new(Expr::Ident("s".into())),
            }),
        ]);
        let fndef = make_fn(body);
        let mut types = HashMap::new();
        types.insert("s".into(), Type::Str);
        let errs = check_fn(&fndef, types);
        assert!(
            errs.iter()
                .any(|e| matches!(e, BorrowError::MoveBorrowed { .. })),
            "expected E0602 MoveBorrowed, got {errs:?}",
        );
    }

    #[test]
    fn test_copy_used_repeatedly_no_error() {
        // let a = 1; a; a; a  — i64 is Copy, multiple uses fine
        let body = Expr::Block(vec![
            Stmt::simple(Expr::Let {
                ty: None,
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
        assert!(
            errs.is_empty(),
            "Empty function should produce no errors: {errs:?}"
        );
    }
}
