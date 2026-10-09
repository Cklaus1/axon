//! R50: the compiler from a fn body to a [`Body`]. Slice S1 lowers the scalar
//! core (spec §4 "Lowered set per slice"); every other node is one `Tree` op.
//! The `match` in [`Compiler::expr`] names every `Expr` variant (no wildcard):
//! a new variant does not compile until someone decides how the engine runs
//! it.
//!
//! Every lowered construct emits its ops in `eval`'s evaluation order, and a
//! `ScopePush`/`ScopePop` exactly where `eval` calls `env.push()`/`env.pop()`
//! (block, loop iteration, `for`), except around statements that bind
//! nothing ([`binds`]): there the scope would stay empty, and an empty scope
//! is unobservable (no slot moves, no lookup sees it, a closure's write-back
//! and `snapshot` read the same bindings), so the push and the pop are elided.
//! The compiler tracks the operand-stack height and the scope depth
//! statically: [`Compiler::expr`] leaves one value more on the stack,
//! [`Compiler::stmt`] none.

use super::pure::{Pure, PureExpr, PureLoop, PureOp, PureStmt, Sink};
use super::{Body, Cond, Loop, Op, Opnd, Var};
use crate::ast::{BinOp, Expr, FmtPart, Literal, MatchArm, Pattern, Stmt, UnaryOp};
use crate::interp::sym::NOT_LOCAL;
use crate::interp::PlaceStep;
use crate::interp::{lit_to_val, Interp, Resolution, Value};

/// Compile the fn body `body`; `res` is the program's resolution table (the
/// `(sym, slot)` of every name node, the string and decimal literals).
pub(in crate::interp) fn compile<'p>(res: &Resolution, body: &'p Expr) -> Body<'p> {
    let mut c = Compiler {
        res,
        ops: Vec::new(),
        loops: Vec::new(),
        height: 0,
        max_height: 0,
        scopes: 0,
        twins: Vec::new(),
    };
    c.expr(body);
    debug_assert_eq!((c.height, c.scopes), (1, 0), "a body leaves one value");
    let leaf = c
        .pure_tree(body, &mut 0)
        .map(|t| Box::new(PureExpr::new(t)));
    Body {
        ops: c.ops.into_boxed_slice(),
        loops: c.loops.into_boxed_slice(),
        max_stack: c.max_height as usize,
        leaf,
    }
}

struct Compiler<'r, 'p> {
    res: &'r Resolution,
    ops: Vec<Op<'p>>,
    loops: Vec<Loop>,
    /// Operand-stack height after the ops emitted so far.
    height: u32,
    max_height: u32,
    /// Scopes pushed by the ops emitted so far.
    scopes: u32,
    /// `(branch, pure)`: the generic branch op of an `if`/`while` condition
    /// and the [`Op::Pure`] before it, whose `Sink::Branch` target
    /// [`Compiler::patch`] sets with the branch's.
    twins: Vec<(u32, u32)>,
}

impl<'p> Compiler<'_, 'p> {
    /// Emit the ops that evaluate `e` and push its value.
    fn expr(&mut self, e: &'p Expr) {
        match e {
            Expr::Literal(lit) => {
                let v = self.literal(e, lit);
                self.emit(Op::Const(v), 0, 1);
            }
            Expr::Ident(name) => {
                let var = self.var(e, name);
                self.emit(Op::Load(var), 0, 1);
            }
            Expr::Block(stmts) => self.block(stmts, true),
            Expr::Let { .. }
            | Expr::Own { .. }
            | Expr::RefBind { .. }
            | Expr::Assign { .. }
            | Expr::AssignTo { .. }
            | Expr::While { .. }
            | Expr::WhileLet { .. }
            | Expr::For { .. } => {
                self.stmt(e);
                self.emit(Op::Const(Value::Unit), 0, 1);
            }
            Expr::If { cond, then, else_ } => self.if_(cond, then, else_.as_deref(), true),
            Expr::BinOp { op, left, right } => self.binop(op, left, right),
            Expr::UnaryOp {
                op: UnaryOp::RefMut,
                ..
            } => self.tree(e),
            Expr::UnaryOp { op, operand } => {
                self.expr(operand);
                self.emit(Op::Unary(op), 1, 1);
            }
            Expr::Return(_) | Expr::Break | Expr::Continue => {
                self.stmt(e);
                // Control never reaches the next op; the value it would
                // have pushed keeps the static height consistent at joins.
                self.adjust(0, 1);
            }
            Expr::Question(inner) => {
                self.expr(inner);
                self.emit(Op::Question, 1, 1);
            }
            Expr::Some(inner) => {
                self.expr(inner);
                self.emit(Op::WrapSome, 1, 1);
            }
            Expr::Ok(inner) => {
                self.expr(inner);
                self.emit(Op::WrapOk, 1, 1);
            }
            Expr::Err(inner) => {
                self.expr(inner);
                self.emit(Op::WrapErr, 1, 1);
            }
            Expr::None => {
                self.emit(Op::Const(Value::None), 0, 1);
            }
            Expr::FmtStr { parts } => {
                self.emit(Op::FmtNew, 0, 1);
                for part in parts {
                    match part {
                        FmtPart::Lit(t) => {
                            self.emit(Op::FmtLit(t), 0, 0);
                        }
                        FmtPart::Expr(x) => {
                            self.expr(x);
                            self.emit(Op::FmtPush, 1, 0);
                        }
                    }
                }
            }
            Expr::Call { callee, args, tier } => {
                // An `Ident` callee without `P(x)` or a `&mut` argument
                // (spec §4: every other callee stays on the tree for good).
                if let (Expr::Ident(name), None) = (callee.as_ref(), tree_shape(e)) {
                    for a in args {
                        self.expr(a);
                    }
                    let argc = args.len() as u32;
                    self.emit(
                        Op::Call {
                            callee: self.var(callee, name),
                            resume: name == "resume",
                            argc,
                            tier: tier.as_deref(),
                        },
                        argc,
                        1,
                    );
                } else {
                    self.tree(e);
                }
            }
            Expr::MethodCall {
                receiver,
                method,
                args,
            } => self.method_call(receiver, method, args),
            Expr::Match { subject, arms } => self.match_(subject, arms),
            Expr::Spawn(_) => self.tree(e),
            Expr::Select(_) => self.tree(e),
            Expr::Comptime(_) => self.tree(e),
            Expr::InlineAsm { .. } => self.tree(e),
            Expr::Lambda { .. } => {
                self.emit(Op::Lambda(e), 0, 1);
            }
            Expr::FieldAccess { receiver, field } => self.field(e, receiver, field),
            Expr::Index { .. } if tree_shape(e).is_some() => self.tree(e),
            Expr::Index { receiver, index } => self.index(receiver, index),
            Expr::Tuple(elems) => {
                let n = self.exprs(elems.iter());
                self.emit(Op::MakeTuple(n), n, 1);
            }
            Expr::Array(elems) => {
                let n = self.exprs(elems.iter());
                self.emit(Op::MakeArray(n), n, 1);
            }
            Expr::StructLit { fields, .. } => {
                match self.res.record_lit(e).and_then(|lit| lit.empty.as_ref()) {
                    Some(v) => {
                        self.emit(Op::Const(v.clone()), 0, 1);
                    }
                    None => {
                        let n = self.exprs(fields.iter().map(|(_, x)| x));
                        self.emit(Op::Record(e), n, 1);
                    }
                }
            }
            Expr::WithHandler { .. } => self.tree(e),
        }
    }

    /// Emit the ops that evaluate `e` for its effect only (its value, which
    /// `eval` would compute and the caller drop, is not kept).
    fn stmt(&mut self, e: &'p Expr) {
        match e {
            Expr::Let { name, value, ty }
            | Expr::Own { name, value, ty }
            | Expr::RefBind { name, value, ty } => {
                let var = self.var(e, name);
                let pure = match (e, ty) {
                    (Expr::Let { .. }, None) => self.try_pure(value, Sink::Define(var)),
                    _ => None,
                };
                self.expr(value);
                let op = match ty {
                    None => Op::Define(var),
                    Some(ty) => Op::Let { var, ty },
                };
                self.emit(op, 1, 0);
                self.end_pure(pure);
            }
            Expr::Assign { name, value } => self.assign(e, name, value),
            Expr::AssignTo { place, value } => self.assign_to(place, value),
            Expr::While { cond, body } => self.while_(cond, body),
            Expr::WhileLet {
                pattern,
                expr,
                body,
            } => self.while_let(pattern, expr, body),
            Expr::For {
                var,
                start,
                end,
                inclusive,
                body,
            } => self.for_(e, var, start, end, *inclusive, body),
            Expr::If { cond, then, else_ } => self.if_(cond, then, else_.as_deref(), false),
            Expr::Block(stmts) => self.block(stmts, false),
            Expr::Return(value) => {
                match value {
                    Some(v) => self.expr(v),
                    None => {
                        self.emit(Op::Const(Value::Unit), 0, 1);
                    }
                }
                self.emit(Op::Return, 1, 0);
            }
            Expr::Break => {
                self.emit(Op::Break, 0, 0);
            }
            Expr::Continue => {
                self.emit(Op::Continue, 0, 0);
            }
            _ => {
                self.expr(e);
                self.emit(Op::Drop, 1, 0);
            }
        }
    }

    /// A block: a scope around its statements (elided when they bind
    /// nothing); the last one's value (unit for an empty block) when `value`.
    fn block(&mut self, stmts: &'p [Stmt], value: bool) {
        let scoped = binds(stmts);
        if scoped {
            self.scope_push();
        }
        match stmts.split_last() {
            Some((last, init)) if value => {
                for s in init {
                    self.stmt(&s.expr);
                }
                self.expr(&last.expr);
            }
            _ => {
                for s in stmts {
                    self.stmt(&s.expr);
                }
                if value {
                    self.emit(Op::Const(Value::Unit), 0, 1);
                }
            }
        }
        if scoped {
            self.scope_pop();
        }
    }

    /// `if cond { then } else { else_ }`, pushing its value when `value`.
    fn if_(&mut self, cond: &'p Expr, then: &'p Expr, else_: Option<&'p Expr>, value: bool) {
        let branch = self.branch(cond, Cond::If, false);
        if value {
            self.expr(then);
        } else {
            self.stmt(then);
        }
        if else_.is_none() && !value {
            self.patch(branch);
            return;
        }
        let jump = self.emit(Op::Jump(0), 0, 0);
        self.patch(branch);
        if value {
            // The `else` arm starts at the height the `then` arm started at.
            self.adjust(1, 0);
        }
        match (else_, value) {
            (Some(e), true) => self.expr(e),
            (Some(e), false) => self.stmt(e),
            (None, _) => {
                self.emit(Op::Const(Value::Unit), 0, 1);
            }
        }
        self.patch(jump);
    }

    /// `while cond { body }`: the condition, then one iteration in its own
    /// scope, as `run_loop_body` runs it. The condition op pushes the
    /// iteration's scope when it does not exit; the back edge pops it. A
    /// body that binds nothing runs without the scope.
    fn while_(&mut self, cond: &'p Expr, body: &'p [Stmt]) {
        let scoped = binds(body);
        let pure = self.pure_loop(cond, body);
        let (scopes, height) = (self.scopes, self.height);
        let head = self.here();
        let branch = self.branch(cond, Cond::While, scoped);
        let inner = scopes + scoped as u32;
        self.scopes = inner;
        let start = self.here();
        for s in body {
            self.stmt(&s.expr);
        }
        let end = self.here();
        if scoped {
            self.emit(Op::PopJump(head), 0, 0);
        } else {
            self.emit(Op::Jump(head), 0, 0);
        }
        self.scopes = scopes;
        let exit = self.here();
        self.patch(branch);
        if let Some(at) = pure {
            if let Op::PureLoop { exit: x, .. } = &mut self.ops[at as usize] {
                *x = exit;
            }
        }
        self.loops.push(Loop {
            start,
            end,
            scopes,
            height,
            brk: exit,
            cont: end,
            cont_scopes: inner,
        });
    }

    /// `for var in start..end` (`..=` when `inclusive`): the bounds through
    /// `strict_int`, `start` before `end` runs, kept on the stack as the
    /// counter and the bound; then per iteration the test, `env.push()`, the
    /// variable, `env.push()` (`run_loop_body`'s, elided when the body binds
    /// nothing), the body, the pops, the increment (spec §4 `strict_int`).
    fn for_(
        &mut self,
        e: &'p Expr,
        var: &'p String,
        start: &'p Expr,
        end: &'p Expr,
        inclusive: bool,
        body: &'p [Stmt],
    ) {
        self.expr(start);
        self.emit(Op::StrictInt, 1, 1);
        self.expr(end);
        self.emit(Op::StrictInt, 1, 1);
        let (scopes, height) = (self.scopes, self.height);
        let scoped = binds(body);
        let inner = scopes + 1 + scoped as u32;
        let var = self.var(e, var);
        let head = self.emit(
            Op::ForTest {
                var,
                inclusive,
                scoped,
                exit: 0,
            },
            0,
            0,
        );
        self.scopes = inner;
        let first = self.here();
        for s in body {
            self.stmt(&s.expr);
        }
        let next = self.emit(
            Op::ForNext {
                var,
                inclusive,
                scoped,
                first,
            },
            0,
            0,
        );
        self.scopes = scopes;
        let exit = self.here();
        if let Op::ForTest { exit: x, .. } = &mut self.ops[head as usize] {
            *x = exit;
        }
        self.emit(Op::Drop, 1, 0);
        self.emit(Op::Drop, 1, 0);
        self.loops.push(Loop {
            start: first,
            end: next,
            scopes,
            height,
            brk: exit,
            cont: next,
            cont_scopes: inner,
        });
    }

    /// `name = value` to a local, as the `Assign` arm: `assign_in_place`
    /// first when the statement has an AX-31 shape, then the value and
    /// `assign_var`. A binary-operation value stores in the same op
    /// ([`Op::StoreBin`]). A pure tree of two or more operators that is not
    /// AX-31-shaped gets an [`Op::Pure`] first (spec §4 S7).
    fn assign(&mut self, e: &'p Expr, name: &'p String, value: &'p Expr) {
        let var = self.var(e, name);
        let shaped = Interp::assign_in_place_shaped(name, value);
        let pure = match shaped {
            false => self.try_pure(value, Sink::Store(var)),
            true => None,
        };
        if let Expr::BinOp { op, left, right } = value {
            if !matches!(op, BinOp::And | BinOp::Or) {
                if let (Some(l), Some(r)) = (self.inline(left), self.inline(right)) {
                    let in_place = shaped.then_some(value);
                    self.emit(store_bin(var, in_place, op.clone(), l, r), 0, 0);
                    self.end_pure(pure);
                    return;
                }
            }
        }
        let at = shaped.then(|| {
            self.emit(
                Op::AssignInPlace {
                    var,
                    value,
                    done: 0,
                },
                0,
                0,
            )
        });
        match value {
            Expr::BinOp { op, left, right } if !matches!(op, BinOp::And | BinOp::Or) => {
                let (l, r, popped) = self.operands(left, right);
                self.emit(store_bin(var, None, op.clone(), l, r), popped, 0);
            }
            _ => {
                self.expr(value);
                self.emit(Op::Store(var), 1, 0);
            }
        }
        if let Some(at) = at {
            let done = self.here();
            if let Op::AssignInPlace { done: d, .. } = &mut self.ops[at as usize] {
                *d = done;
            }
        }
        self.end_pure(pure);
    }

    /// Push the values of `elems` left to right; returns how many.
    fn exprs(&mut self, elems: impl Iterator<Item = &'p Expr>) -> u32 {
        let mut n = 0;
        for x in elems {
            self.expr(x);
            n += 1;
        }
        n
    }

    /// `receiver.field` (`.N` included): an identifier receiver is read in
    /// place (`field_in_place`), any other is evaluated, then `field_of`.
    fn field(&mut self, e: &'p Expr, receiver: &'p Expr, field: &'p String) {
        let f = self.res.sym(e, field);
        if let Expr::Ident(name) = receiver {
            let var = self.var(receiver, name);
            self.emit(Op::FieldLocal { var, f, field }, 0, 1);
        } else {
            self.expr(receiver);
            self.emit(Op::Field { f, field }, 1, 1);
        }
    }

    /// `receiver[index]` (not `E[..]`/`Var[..]`, which stay on the tree), in
    /// the `Index` arm's two paths: an identifier receiver is tested for a
    /// binding before the index runs ([`Op::IndexLocal`] in one op when the
    /// index is inline, else [`Op::IndexTest`] .. [`Op::IndexIdent`]); any
    /// other receiver is evaluated, then the index ([`Op::IndexValue`]).
    fn index(&mut self, receiver: &'p Expr, index: &'p Expr) {
        if let Expr::Ident(name) = receiver {
            let arr = self.var(receiver, name);
            if let Some(idx) = self.inline(index) {
                self.emit(Op::IndexLocal { arr, idx }, 0, 1);
            } else {
                self.emit(Op::IndexTest(arr), 0, 1);
                self.expr(index);
                self.emit(Op::IndexIdent(arr), 2, 1);
            }
            return;
        }
        self.expr(receiver);
        self.expr(index);
        self.emit(Op::IndexValue, 2, 1);
    }

    /// `place = value`, as the `AssignTo` arm: the value, then the index
    /// expressions in `flatten_place`'s order (the outermost node first,
    /// walking toward the root), each through `place_index`, then
    /// `write_place`. A root that is not an identifier ends in the
    /// `invalid assignment target` panic and is never evaluated.
    fn assign_to(&mut self, place: &'p Expr, value: &'p Expr) {
        self.expr(value);
        if let Expr::Index { receiver, index } = place {
            if let (Expr::Ident(name), Some(idx)) = (receiver.as_ref(), self.inline(index)) {
                let base = self.var(receiver, name);
                self.emit(Op::WriteIndexLocal { base, idx }, 1, 0);
                return;
            }
        }
        let mut steps = Vec::new();
        let mut nidx = 0;
        let mut cur = place;
        let base = loop {
            match cur {
                Expr::Ident(name) => break Some(self.var(cur, name)),
                Expr::FieldAccess { receiver, field } => {
                    steps.push(PlaceStep::Field(self.res.sym(cur, field)));
                    cur = receiver;
                }
                Expr::Index { receiver, index } => {
                    self.expr(index);
                    self.emit(Op::PlaceIndex, 1, 1);
                    steps.push(PlaceStep::Index(0));
                    nidx += 1;
                    cur = receiver;
                }
                _ => break None,
            }
        };
        match base {
            Some(base) => {
                steps.reverse();
                let steps = steps.into_boxed_slice();
                self.emit(Op::WritePlace { base, steps, nidx }, 1 + nidx, 0);
            }
            None => {
                self.emit(Op::PlaceInvalid, 1 + nidx, 0);
            }
        }
    }

    /// `match subject { arms }`, as the `Match` arm runs it: the subject
    /// first (kept on the stack under each arm), then per arm the scope,
    /// `match_pattern`, the guard (true only for a plain `true`) and the
    /// body; the arm's scope is popped on every way out of it. No arm taken:
    /// the `no match arm matched` panic. An arm whose pattern, guard and
    /// body bind nothing runs without the scope ([`binds`]).
    fn match_(&mut self, subject: &'p Expr, arms: &'p [MatchArm]) {
        self.expr(subject);
        let mut ends = Vec::with_capacity(arms.len());
        for arm in arms {
            let scoped = pattern_binds(&arm.pattern)
                || arm.guard.as_ref().is_some_and(expr_binds)
                || expr_binds(&arm.body);
            let head = self.emit(
                Op::MatchArm {
                    pat: &arm.pattern,
                    scoped,
                    next: 0,
                },
                0,
                0,
            );
            self.scopes += scoped as u32;
            let guard = arm.guard.as_ref().map(|g| {
                self.expr(g);
                self.emit(Op::Guard { scoped, next: 0 }, 1, 0)
            });
            self.expr(&arm.body);
            self.scopes -= scoped as u32;
            ends.push(self.emit(Op::MatchEnd { scoped, end: 0 }, 2, 1));
            // The next arm starts with the subject alone on top again.
            let next = self.here();
            for at in std::iter::once(head).chain(guard) {
                self.patch_to(at, next);
            }
        }
        // `panic` never yields; the static height is the match's value's.
        self.emit(Op::NoMatch, 1, 1);
        let end = self.here();
        for at in ends {
            self.patch_to(at, end);
        }
    }

    /// `while let pat = expr { body }`, as the `WhileLet` arm runs it: per
    /// iteration the value, the pattern's scope and `match_pattern`, then
    /// the body in a scope of its own (`run_loop_body`'s), both popped at
    /// the end of the iteration. A pattern that binds nothing, or a body
    /// that binds nothing, runs without that scope.
    fn while_let(&mut self, pat: &'p Pattern, expr: &'p Expr, body: &'p [Stmt]) {
        let scoped = pattern_binds(pat);
        let body_scoped = binds(body);
        let (scopes, height) = (self.scopes, self.height);
        let head = self.here();
        self.expr(expr);
        let test = self.emit(
            Op::WhileLet {
                pat,
                scoped,
                body: body_scoped,
                exit: 0,
            },
            1,
            0,
        );
        let inner = scopes + scoped as u32 + body_scoped as u32;
        self.scopes = inner;
        let start = self.here();
        for s in body {
            self.stmt(&s.expr);
        }
        let end = self.emit(
            Op::WhileLetNext {
                scoped,
                body: body_scoped,
                head,
            },
            0,
            0,
        );
        self.scopes = scopes;
        let exit = self.here();
        self.patch_to(test, exit);
        self.loops.push(Loop {
            start,
            end,
            scopes,
            height,
            brk: exit,
            cont: end,
            cont_scopes: inner,
        });
    }

    /// `receiver.method(args)`, as the `MethodCall` arm: the receiver first;
    /// a channel takes `chan_method` with the argument nodes
    /// ([`Op::MethodRecv`]), any other receiver the compiled arguments, left
    /// to right, and `impl_method`.
    fn method_call(&mut self, receiver: &'p Expr, method: &'p str, args: &'p [Expr]) {
        self.expr(receiver);
        let recv = self.emit(
            Op::MethodRecv {
                method,
                args,
                done: 0,
            },
            0,
            0,
        );
        for a in args {
            self.expr(a);
        }
        let argc = args.len() as u32;
        self.emit(Op::MethodCall { method, argc }, argc + 1, 1);
        let done = self.here();
        self.patch_to(recv, done);
    }

    /// Point the S3 op at `at` (an arm's or `while let`'s exit, a channel
    /// method's skip) to `to`.
    fn patch_to(&mut self, at: u32, to: u32) {
        match &mut self.ops[at as usize] {
            Op::MatchArm { next: t, .. }
            | Op::Guard { next: t, .. }
            | Op::MatchEnd { end: t, .. }
            | Op::WhileLet { exit: t, .. }
            | Op::MethodRecv { done: t, .. } => *t = to,
            _ => unreachable!("vm: patching a non-S3 op"),
        }
    }

    /// `left op right`, pushing the result.
    fn binop(&mut self, op: &'p BinOp, left: &'p Expr, right: &'p Expr) {
        if matches!(op, BinOp::And | BinOp::Or) {
            self.expr(left);
            let sc = self.emit(Op::ShortCircuit { op, end: 0 }, 0, 0);
            self.expr(right);
            self.emit(Op::Logic(op), 2, 1);
            let end = self.here();
            if let Op::ShortCircuit { end: x, .. } = &mut self.ops[sc as usize] {
                *x = end;
            }
            return;
        }
        let (l, r, popped) = self.operands(left, right);
        let op = op.clone();
        self.emit(Op::Bin { op, l, r }, popped, 1);
    }

    /// Emit an `if`/`while` condition and the branch taken when it is false;
    /// returns the branch op, for [`Compiler::patch`]. A pure tree of two
    /// or more operators gets an [`Op::Pure`] first, its twin the generic
    /// condition below (spec §4 S7).
    fn branch(&mut self, cond: &'p Expr, kind: Cond, push: bool) -> u32 {
        let pure = self.try_pure(cond, Sink::Branch { target: 0, push });
        let branch = self.branch_generic(cond, kind, push);
        if let Some(at) = pure {
            self.twins.push((branch, at));
        }
        self.end_pure(pure);
        branch
    }

    /// `e` as a [`Pure`] tree, or `None` when it is not one (spec §4 S7);
    /// `n` counts its operator nodes. An identifier is a leaf only when
    /// resolution binds it to a local slot.
    fn pure_tree(&self, e: &'p Expr, n: &mut u32) -> Option<Pure<'p>> {
        Some(match e {
            Expr::Ident(name) => {
                let var = self.var(e, name);
                if var.slot >= NOT_LOCAL {
                    return None;
                }
                Pure::Local(var)
            }
            Expr::Literal(Literal::Int(v)) => Pure::Int(*v),
            Expr::Literal(Literal::Float(v)) => Pure::Float(*v),
            Expr::Literal(Literal::Bool(v)) => Pure::Bool(*v),
            Expr::BinOp { op, left, right } => {
                let k = Box::new([self.pure_tree(left, n)?, self.pure_tree(right, n)?]);
                *n += 1;
                match op {
                    BinOp::And => Pure::And(k),
                    BinOp::Or => Pure::Or(k),
                    op => Pure::Bin(op.clone(), k),
                }
            }
            _ => return None,
        })
    }

    /// An [`Op::Pure`] for `e` into `sink` when `e` is a pure tree of two or
    /// more operators; the caller emits the twin next, then
    /// [`Compiler::end_pure`].
    fn try_pure(&mut self, e: &'p Expr, sink: Sink<'p>) -> Option<u32> {
        let mut n = 0;
        let tree = self.pure_tree(e, &mut n)?;
        if n < 2 {
            return None;
        }
        let op = PureOp {
            e: PureExpr::new(tree),
            sink,
            skip: 0,
        };
        Some(self.emit(Op::Pure(Box::new(op)), 0, 0))
    }

    /// Point the [`Op::Pure`] at `at` past its twin, which ends here.
    fn end_pure(&mut self, at: Option<u32>) {
        let Some(at) = at else { return };
        let here = self.here();
        if let Op::Pure(p) = &mut self.ops[at as usize] {
            p.skip = here;
        }
    }

    /// An [`Op::PureLoop`] ahead of the generic loop of `while cond { body
    /// }` when `cond` is a pure tree and every statement is an untyped
    /// `let x = <pure>` or an `x = <pure>` to a local (spec §4 S7); its exit
    /// is patched by [`Compiler::while_`].
    fn pure_loop(&mut self, cond: &'p Expr, body: &'p [Stmt]) -> Option<u32> {
        let n = &mut 0;
        let cond = self.pure_tree(cond, n)?;
        let mut stmts = Vec::with_capacity(body.len());
        for s in body {
            let e = &s.expr;
            let (name, value, is_let) = match e {
                Expr::Let {
                    name,
                    value,
                    ty: None,
                } => (name, value, true),
                Expr::Assign { name, value } => (name, value, false),
                _ => return None,
            };
            let var = self.var(e, name);
            if var.slot >= NOT_LOCAL {
                return None;
            }
            let value = self.pure_tree(value, n)?;
            stmts.push(PureStmt { var, is_let, value });
        }
        let lp = Box::new(PureLoop::new(cond, stmts));
        Some(self.emit(Op::PureLoop { lp, exit: 0 }, 0, 0))
    }

    /// The generic condition and branch of [`Compiler::branch`]. A binary
    /// operation other than `&&`/`||` fuses into one compare-and-branch.
    /// With `push` (a `while`), the op pushes the iteration's scope when it
    /// falls through.
    fn branch_generic(&mut self, cond: &'p Expr, kind: Cond, push: bool) -> u32 {
        if let Expr::BinOp { op, left, right } = cond {
            if !matches!(op, BinOp::And | BinOp::Or) {
                if let Some(op) = self.branch_index(op, left, right, kind, push) {
                    return op;
                }
                let (l, r, popped) = self.operands(left, right);
                let op = op.clone();
                return self.emit(branch_cmp(op, l, r, kind, push), popped, 0);
            }
        }
        self.expr(cond);
        self.emit(
            Op::BranchFalse {
                target: 0,
                cond: kind,
                push,
            },
            1,
            0,
        )
    }

    /// [`Op::BranchIndexLocal`] or [`Op::BranchIndexInt`] for a condition
    /// `name[i] op r` with `i` a local and `r` a local or an int literal (not
    /// an `E[..]`/`Var[..]` read, which stays on the tree).
    fn branch_index(
        &mut self,
        op: &BinOp,
        left: &'p Expr,
        right: &'p Expr,
        cond: Cond,
        push: bool,
    ) -> Option<u32> {
        let Expr::Index { receiver, index } = left else {
            return None;
        };
        let (Expr::Ident(name), Some(Opnd::Local(idx))) = (receiver.as_ref(), self.inline(index))
        else {
            return None;
        };
        if tree_shape(left).is_some() {
            return None;
        }
        let arr = self.var(receiver, name);
        let op = op.clone();
        let op = match self.inline(right)? {
            Opnd::Local(r) => Op::BranchIndexLocal {
                op,
                arr,
                idx,
                r,
                target: 0,
                cond,
                push,
            },
            Opnd::Int(r) => Op::BranchIndexInt {
                op,
                arr,
                idx,
                r,
                target: 0,
                cond,
                push,
            },
            _ => return None,
        };
        // The slow path pushes the element, then pops it.
        self.max_height = self.max_height.max(self.height + 1);
        Some(self.emit(op, 0, 0))
    }

    /// The operands of a binary operation, left to right, and how many of
    /// them are on the stack. The left one is inline only when the right one
    /// is too, so nothing runs between the tree's read of the left operand
    /// and the op's.
    fn operands(&mut self, left: &'p Expr, right: &'p Expr) -> (Opnd<'p>, Opnd<'p>, u32) {
        if let Some(r) = self.inline(right) {
            if let Some(l) = self.inline(left) {
                return (l, r, 0);
            }
            self.expr(left);
            return (Opnd::Stack, r, 1);
        }
        self.expr(left);
        self.expr(right);
        (Opnd::Stack, Opnd::Stack, 2)
    }

    /// `e` as an inline operand: an identifier or a literal.
    fn inline(&self, e: &'p Expr) -> Option<Opnd<'p>> {
        match e {
            Expr::Ident(name) => Some(Opnd::Local(self.var(e, name))),
            Expr::Literal(Literal::Int(n)) => Some(Opnd::Int(*n)),
            Expr::Literal(Literal::Float(f)) => Some(Opnd::Float(*f)),
            Expr::Literal(lit) => Some(Opnd::Const(self.literal(e, lit))),
            _ => None,
        }
    }

    /// The value literal node `e` evaluates to, as the `Literal` arm builds
    /// it.
    fn literal(&self, e: &Expr, lit: &Literal) -> Value {
        match lit {
            Literal::Int(n) => Value::Int(*n),
            Literal::Float(f) => Value::Float(*f),
            Literal::Bool(b) => Value::Bool(*b),
            _ => match self.res.lit(e) {
                Some(v) => v.clone(),
                None => lit_to_val(lit),
            },
        }
    }

    fn var(&self, e: &Expr, name: &'p String) -> Var<'p> {
        let (s, slot) = self.res.var(e, name);
        Var { s, slot, name }
    }

    /// `e` runs on the tree-walker as one op.
    fn tree(&mut self, e: &'p Expr) {
        self.emit(Op::Tree(e), 0, 1);
    }

    fn scope_push(&mut self) {
        self.emit(Op::ScopePush, 0, 0);
        self.scopes += 1;
    }

    fn scope_pop(&mut self) {
        self.emit(Op::ScopePop, 0, 0);
        self.scopes -= 1;
    }

    fn here(&self) -> u32 {
        self.ops.len() as u32
    }

    /// Append `op`, which pops `pops` values and then pushes `pushes`;
    /// returns its index.
    fn emit(&mut self, op: Op<'p>, pops: u32, pushes: u32) -> u32 {
        let at = self.here();
        self.ops.push(op);
        self.adjust(pops, pushes);
        at
    }

    fn adjust(&mut self, pops: u32, pushes: u32) {
        self.height = self.height - pops + pushes;
        self.max_height = self.max_height.max(self.height);
    }

    /// Point the branch at `at` to the next op, and the [`Op::Pure`] twin of
    /// a condition branch with it.
    fn patch(&mut self, at: u32) {
        let here = self.here();
        if let Some(i) = self.twins.iter().position(|(b, _)| *b == at) {
            let (_, pure) = self.twins.swap_remove(i);
            if let Op::Pure(p) = &mut self.ops[pure as usize] {
                if let Sink::Branch { target, .. } = &mut p.sink {
                    *target = here;
                }
            }
        }
        match &mut self.ops[at as usize] {
            Op::Jump(t)
            | Op::BranchFalse { target: t, .. }
            | Op::BranchCmp { target: t, .. }
            | Op::BranchLocalInt { target: t, .. }
            | Op::BranchLocalLocal { target: t, .. }
            | Op::BranchIndexLocal { target: t, .. }
            | Op::BranchIndexInt { target: t, .. } => *t = here,
            _ => unreachable!("vm: patching a non-branch op"),
        }
    }
}

/// `x = l op r` as one op: the local/int and local/local shapes get their
/// own op ([`Op::StoreLocalInt`], [`Op::StoreLocalLocal`]).
fn store_bin<'p>(
    var: Var<'p>,
    in_place: Option<&'p Expr>,
    op: BinOp,
    l: Opnd<'p>,
    r: Opnd<'p>,
) -> Op<'p> {
    match (l, r) {
        (Opnd::Local(l), Opnd::Int(r)) => Op::StoreLocalInt {
            var,
            in_place,
            op,
            l,
            r,
        },
        (Opnd::Local(l), Opnd::Local(r)) => Op::StoreLocalLocal {
            var,
            in_place,
            op,
            l,
            r,
        },
        (l, r) => Op::StoreBin {
            var,
            in_place,
            op,
            l,
            r,
        },
    }
}

/// Whether running `stmts` in a scope of their own can bind anything in that
/// scope. `eval` binds into the current scope only in the `let`/`own`/`ref`
/// arm (`bind_let`); `for`, match arms, `while let` and lambdas push their
/// own scope (or frame) first. A `let` anywhere below, nested blocks
/// included, counts, so an unsure case keeps the scope.
fn binds(stmts: &[Stmt]) -> bool {
    let mut hit = false;
    for s in stmts {
        crate::ast::walk_expr(&s.expr, &mut |e| {
            hit |= matches!(
                e,
                Expr::Let { .. } | Expr::Own { .. } | Expr::RefBind { .. }
            );
        });
    }
    hit
}

/// [`binds`] for one expression run in a scope of its own (a match arm's
/// guard or body).
fn expr_binds(e: &Expr) -> bool {
    let mut hit = false;
    crate::ast::walk_expr(e, &mut |e| {
        hit |= matches!(
            e,
            Expr::Let { .. } | Expr::Own { .. } | Expr::RefBind { .. }
        );
    });
    hit
}

/// Whether `match_pattern` can bind a name for `pat`: it binds only through
/// an identifier pattern, nested ones included.
fn pattern_binds(pat: &Pattern) -> bool {
    match pat {
        Pattern::Ident(_) => true,
        Pattern::Wildcard | Pattern::Literal(_) | Pattern::None => false,
        Pattern::Some(p) | Pattern::Ok(p) | Pattern::Err(p) => pattern_binds(p),
        Pattern::Struct { fields, .. } => fields.iter().any(|(_, p)| pattern_binds(p)),
        Pattern::Tuple(ps) => ps.iter().any(pattern_binds),
    }
}

/// A compare-and-branch (target patched later), specialized as
/// [`store_bin`] is.
fn branch_cmp<'p>(op: BinOp, l: Opnd<'p>, r: Opnd<'p>, cond: Cond, push: bool) -> Op<'p> {
    match (l, r) {
        (Opnd::Local(l), Opnd::Int(r)) => Op::BranchLocalInt {
            op,
            l,
            r,
            target: 0,
            cond,
            push,
        },
        (Opnd::Local(l), Opnd::Local(r)) => Op::BranchLocalLocal {
            op,
            l,
            r,
            target: 0,
            cond,
            push,
        },
        (l, r) => Op::BranchCmp {
            op,
            l,
            r,
            target: 0,
            cond,
            push,
        },
    }
}

/// The `Expr` variant's name, as the `vm: tree-op` trace line prints it.
pub(super) fn variant_name(e: &Expr) -> &'static str {
    match e {
        Expr::Block(_) => "Block",
        Expr::Let { .. } => "Let",
        Expr::Own { .. } => "Own",
        Expr::RefBind { .. } => "RefBind",
        Expr::Call { .. } => "Call",
        Expr::MethodCall { .. } => "MethodCall",
        Expr::BinOp { .. } => "BinOp",
        Expr::UnaryOp { .. } => "UnaryOp",
        Expr::Question(_) => "Question",
        Expr::Match { .. } => "Match",
        Expr::If { .. } => "If",
        Expr::Spawn(_) => "Spawn",
        Expr::Select(_) => "Select",
        Expr::Comptime(_) => "Comptime",
        Expr::InlineAsm { .. } => "InlineAsm",
        Expr::Lambda { .. } => "Lambda",
        Expr::Return(_) => "Return",
        Expr::FieldAccess { .. } => "FieldAccess",
        Expr::Index { .. } => "Index",
        Expr::Tuple(_) => "Tuple",
        Expr::Ident(_) => "Ident",
        Expr::Literal(_) => "Literal",
        Expr::FmtStr { .. } => "FmtStr",
        Expr::Ok(_) => "Ok",
        Expr::Err(_) => "Err",
        Expr::Some(_) => "Some",
        Expr::None => "None",
        Expr::Array(_) => "Array",
        Expr::StructLit { .. } => "StructLit",
        Expr::While { .. } => "While",
        Expr::WhileLet { .. } => "WhileLet",
        Expr::Assign { .. } => "Assign",
        Expr::WithHandler { .. } => "WithHandler",
        Expr::AssignTo { .. } => "AssignTo",
        Expr::Break => "Break",
        Expr::Continue => "Continue",
        Expr::For { .. } => "For",
    }
}

/// The `<shape>` of a `vm: tree-op` line (spec §3, §8): the exceptions inside
/// a variant that is (or will be) lowered. `None` for every other node.
pub(super) fn tree_shape(e: &Expr) -> Option<&'static str> {
    match e {
        // In `eval`'s order: the `StructLit` callee forms, then `P(..)`, then
        // the `&mut` protocol, then a callee that is not an identifier.
        Expr::Call { callee, args, .. } => match callee.as_ref() {
            Expr::StructLit { .. } => Some("struct-lit"),
            Expr::Ident(n) if n == "P" && args.len() == 1 => Some("P"),
            _ if args.iter().any(|a| {
                matches!(
                    a,
                    Expr::UnaryOp {
                        op: UnaryOp::RefMut,
                        ..
                    }
                )
            }) =>
            {
                Some("&mut")
            }
            Expr::Ident(_) => None,
            _ => Some("computed"),
        },
        Expr::Index { receiver, .. } if matches!(receiver.as_ref(), Expr::Ident(n) if n == "E" || n == "Var") => {
            Some("E|Var")
        }
        _ => None,
    }
}
