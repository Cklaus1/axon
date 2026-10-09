//! R50: the compiler from a fn body to a [`Body`]. Slice S1 lowers the scalar
//! core (spec §4 "Lowered set per slice"); every other node is one `Tree` op.
//! The `match` in [`Compiler::expr`] names every `Expr` variant (no wildcard):
//! a new variant does not compile until someone decides how the engine runs
//! it.
//!
//! Every lowered construct emits its ops in `eval`'s evaluation order, and a
//! `ScopePush`/`ScopePop` exactly where `eval` calls `env.push()`/`env.pop()`
//! (block, loop iteration, `for`). The compiler tracks the operand-stack
//! height and the scope depth statically: [`Compiler::expr`] leaves one value
//! more on the stack, [`Compiler::stmt`] none.

use super::{Body, Cond, Loop, Op, Opnd, Var};
use crate::ast::{BinOp, Expr, FmtPart, Literal, Stmt, UnaryOp};
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
    };
    c.expr(body);
    debug_assert_eq!((c.height, c.scopes), (1, 0), "a body leaves one value");
    Body {
        ops: c.ops.into_boxed_slice(),
        loops: c.loops.into_boxed_slice(),
        max_stack: c.max_height as usize,
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
            | Expr::While { .. }
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
                if matches!(callee.as_ref(), Expr::Ident(_)) && tree_shape(e).is_none() {
                    for a in args {
                        self.expr(a);
                    }
                    let argc = args.len() as u32;
                    self.emit(
                        Op::Call {
                            callee,
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
            Expr::MethodCall { .. } => self.tree(e),
            Expr::Match { .. } => self.tree(e),
            Expr::Spawn(_) => self.tree(e),
            Expr::Select(_) => self.tree(e),
            Expr::Comptime(_) => self.tree(e),
            Expr::InlineAsm { .. } => self.tree(e),
            Expr::Lambda { .. } => self.tree(e),
            Expr::FieldAccess { .. } => self.tree(e),
            Expr::Index { .. } => self.tree(e),
            Expr::Tuple(_) => self.tree(e),
            Expr::Array(_) => self.tree(e),
            Expr::StructLit { .. } => self.tree(e),
            Expr::WhileLet { .. } => self.tree(e),
            Expr::WithHandler { .. } => self.tree(e),
            Expr::AssignTo { .. } => self.tree(e),
        }
    }

    /// Emit the ops that evaluate `e` for its effect only (its value, which
    /// `eval` would compute and the caller drop, is not kept).
    fn stmt(&mut self, e: &'p Expr) {
        match e {
            Expr::Let { name, value, ty }
            | Expr::Own { name, value, ty }
            | Expr::RefBind { name, value, ty } => {
                self.expr(value);
                let var = self.var(e, name);
                let op = match ty {
                    None => Op::Define(var),
                    Some(ty) => Op::Let { var, ty },
                };
                self.emit(op, 1, 0);
            }
            Expr::Assign { name, value } => self.assign(e, name, value),
            Expr::While { cond, body } => self.while_(cond, body),
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

    /// A block: a scope around its statements; the last one's value (unit
    /// for an empty block) when `value`.
    fn block(&mut self, stmts: &'p [Stmt], value: bool) {
        self.scope_push();
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
        self.scope_pop();
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
    /// iteration's scope when it does not exit; the back edge pops it.
    fn while_(&mut self, cond: &'p Expr, body: &'p [Stmt]) {
        let (scopes, height) = (self.scopes, self.height);
        let head = self.here();
        let branch = self.branch(cond, Cond::While, true);
        self.scopes += 1;
        let start = self.here();
        for s in body {
            self.stmt(&s.expr);
        }
        let end = self.here();
        self.emit(Op::PopJump(head), 0, 0);
        self.scopes -= 1;
        let exit = self.here();
        self.patch(branch);
        self.loops.push(Loop {
            start,
            end,
            scopes,
            height,
            brk: exit,
            cont: end,
            cont_scopes: scopes + 1,
        });
    }

    /// `for var in start..end` (`..=` when `inclusive`): the bounds through
    /// `strict_int`, `start` before `end` runs, kept on the stack as the
    /// counter and the bound; then per iteration the test, `env.push()`, the
    /// variable, `env.push()` (`run_loop_body`'s), the body, `env.pop()`
    /// twice, the increment (spec §4 `strict_int`).
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
        let var = self.var(e, var);
        let head = self.emit(
            Op::ForTest {
                var,
                inclusive,
                exit: 0,
            },
            0,
            0,
        );
        self.scopes += 2;
        let first = self.here();
        for s in body {
            self.stmt(&s.expr);
        }
        let next = self.emit(
            Op::ForNext {
                var,
                inclusive,
                first,
            },
            0,
            0,
        );
        self.scopes -= 2;
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
            cont_scopes: scopes + 2,
        });
    }

    /// `name = value` to a local, as the `Assign` arm: `assign_in_place`
    /// first when the statement has an AX-31 shape, then the value and
    /// `assign_var`. A binary-operation value stores in the same op
    /// ([`Op::StoreBin`]).
    fn assign(&mut self, e: &'p Expr, name: &'p String, value: &'p Expr) {
        let var = self.var(e, name);
        let shaped = Interp::assign_in_place_shaped(name, value);
        if let Expr::BinOp { op, left, right } = value {
            if !matches!(op, BinOp::And | BinOp::Or) {
                if let (Some(l), Some(r)) = (self.inline(left), self.inline(right)) {
                    let in_place = shaped.then_some(value);
                    self.emit(store_bin(var, in_place, op.clone(), l, r), 0, 0);
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
    /// returns the branch op, for [`Compiler::patch`]. A binary operation
    /// other than `&&`/`||` fuses into one compare-and-branch. With `push`
    /// (a `while`), the op pushes the iteration's scope when it falls through.
    fn branch(&mut self, cond: &'p Expr, kind: Cond, push: bool) -> u32 {
        if let Expr::BinOp { op, left, right } = cond {
            if !matches!(op, BinOp::And | BinOp::Or) {
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

    /// Point the branch at `at` to the next op.
    fn patch(&mut self, at: u32) {
        let here = self.here();
        match &mut self.ops[at as usize] {
            Op::Jump(t)
            | Op::BranchFalse { target: t, .. }
            | Op::BranchCmp { target: t, .. }
            | Op::BranchLocalInt { target: t, .. }
            | Op::BranchLocalLocal { target: t, .. } => *t = here,
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
