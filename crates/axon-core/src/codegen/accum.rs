//! AX-78: a recursion through a checked `+` runs as a loop with an
//! accumulator.
//!
//! The shape is
//!
//! ```text
//! fn f(p1: i64, .., pm: i64) -> i64 { if C { B } else { L + f(A1, .., Am) } }
//! ```
//!
//! (either arm may be the recursive one), where `C`, `B`, `L` and the `Ai`
//! are built only from integer and bool literals, the parameters, operators,
//! `if`s with an `else` and calls to `f` itself. Such a function is pure:
//! evaluating it has no effect but its value or a panic, and evaluating it
//! again from the same arguments retraces the same evaluation. The source
//! evaluates `C, L, A, C, L, A, .., C, B` and then the adds from the
//! innermost out, `t0 + (t1 + (.. + (tk-1 + b)))` with `ti` the left
//! operands and `b` the base. LLVM turns such a recursion into a loop
//! (TailCallElim's accumulator recursion elimination) only when the add
//! reassociates; a checked add with a panic edge does not, so every trip
//! was a call frame.
//!
//! Here the trips run as a loop that evaluates `C`, `L`, `A` and `B` exactly
//! where the source does, so a panic in any of them is the same panic at the
//! same point, and sums the left operands in source order. Only the adds
//! move, and only while every left operand so far was non-negative and
//! their sum fits: the loop adds with wrapping and tests
//! `(acc + t | t) < 0`, which, with `acc >= 0` and `t >= 0`, is true iff
//! `t < 0` or the exact sum exceeds `i64::MAX`. The pending source adds are
//! then `t0 + (t1 + (.. + s))` with every `ti >= 0`, for `s` the exact value
//! of the innermost term:
//!
//! * `s >= 0`: the partial sums grow from `s` to `acc + s`, so a source add
//!   overflows iff `acc + s > i64::MAX`;
//! * `s < 0`: every partial sum lies in `[s, acc + s]`, inside i64: no source
//!   add overflows, and `acc + s` does not either.
//!
//! So the source overflows iff the checked `acc + s` does. The operands of
//! the source's first overflowing add are then unknown, so `f.orig`, the
//! source lowering of `f` (calling itself), evaluates the call again from
//! its arguments and panics with the source's message.
//!
//! When the test fires (`t < 0`, or the sum would overflow), the loop stops
//! reordering and `f.rest` finishes in source order: `s = t + f(A)` with the
//! source's checked add and message, then `acc + s` under the rule above.
//! With `acc == 0` every pending add is `0 + ..` (non-negative terms summing
//! to 0), so that case is the source add alone and stays in the loop.
//!
//! A loop frame runs at most `TRIPS_PER_FRAME` trips and then recurses for
//! real (`f.loop` from the next trip's arguments, its value the innermost
//! term `s` above), so a recursion that never reaches its base still
//! exhausts the stack, exit 101 "stack overflow", instead of spinning: the
//! reason every call keeps its frame (`link::optimize_module`).
//!
//! `f` itself evaluates `C` and either returns `B` or calls the loop
//! `f.loop`, so a call that ends in the base, most calls of a binary
//! recursion, never enters the loop's frame. The form costs three more
//! bodies (`f.loop`, `f.rest`, `f.orig`), so codegen applies it at the speed
//! levels `O1`..`O3` only (`set_accumulate`).

use inkwell::attributes::{Attribute, AttributeLoc};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, IntValue};
use inkwell::IntPredicate;

use super::build_wrappers;
use crate::ast;

/// Trips one loop frame runs before it recurses for real.
const TRIPS_PER_FRAME: u64 = 64;

/// The parts of a function of the accumulator shape.
struct Shape<'a> {
    cond: &'a ast::Expr,
    /// The value of `cond` that takes the recursive arm.
    rec_if: bool,
    base: &'a ast::Expr,
    left: &'a ast::Expr,
    args: &'a [ast::Expr],
}

/// Which of the two `emit_fn` bodies of an accumulator-shaped function to
/// emit in place of its source body.
#[derive(Clone, Copy)]
pub(super) enum Role<'ctx> {
    /// `f`: `C` then `B` or a call of `loop_fn`.
    Wrapper { loop_fn: FunctionValue<'ctx> },
    /// `f.loop`: the trips, entered on the recursive arm.
    Loop {
        wrapper: FunctionValue<'ctx>,
        rest_fn: FunctionValue<'ctx>,
        orig_fn: FunctionValue<'ctx>,
    },
}

/// A single-expression block is its expression.
fn peel(e: &ast::Expr) -> &ast::Expr {
    match e {
        ast::Expr::Block(stmts) if stmts.len() == 1 => peel(&stmts[0].expr),
        other => other,
    }
}

fn is_i64(t: &ast::AxonType) -> bool {
    matches!(t, ast::AxonType::Named(n) if n == "i64")
}

fn is_self_call(f: &ast::FnDef, callee: &ast::Expr, args: &[ast::Expr]) -> bool {
    matches!(callee, ast::Expr::Ident(n) if *n == f.name) && args.len() == f.params.len()
}

/// `e` has no effect but its value or a panic, and evaluating it again
/// gives the same result.
fn is_pure(f: &ast::FnDef, e: &ast::Expr) -> bool {
    use ast::Expr as E;
    match e {
        E::Literal(ast::Literal::Int(_) | ast::Literal::Bool(_)) => true,
        E::Ident(n) => f.params.iter().any(|p| p.name == *n),
        E::BinOp { left, right, .. } => is_pure(f, left) && is_pure(f, right),
        E::UnaryOp {
            op: ast::UnaryOp::Neg | ast::UnaryOp::Not | ast::UnaryOp::BitNot,
            operand,
        } => is_pure(f, operand),
        E::If {
            cond,
            then,
            else_: Some(els),
        } => is_pure(f, cond) && is_pure(f, then) && is_pure(f, els),
        E::Block(stmts) if stmts.len() == 1 => is_pure(f, &stmts[0].expr),
        E::Call {
            callee,
            args,
            tier: None,
        } => is_self_call(f, callee, args) && args.iter().all(|a| is_pure(f, a)),
        _ => false,
    }
}

/// `L + f(A)`: the left operand and the arguments.
fn rec_arm<'a>(f: &ast::FnDef, arm: &'a ast::Expr) -> Option<(&'a ast::Expr, &'a [ast::Expr])> {
    let ast::Expr::BinOp {
        op: ast::BinOp::Add,
        left,
        right,
    } = peel(arm)
    else {
        return None;
    };
    match peel(right) {
        ast::Expr::Call {
            callee,
            args,
            tier: None,
        } if is_self_call(f, callee, args) => Some((left, args)),
        _ => None,
    }
}

fn shape(f: &ast::FnDef) -> Option<Shape<'_>> {
    if f.name == "main"
        || f.params.is_empty()
        || !f.generic_params.is_empty()
        || f.verify.is_some()
        || f.contained.is_some()
        || f.attrs.iter().any(|a| a.name != "pure")
        || !f.params.iter().all(|p| is_i64(&p.ty) && p.name != f.name)
        || !f.return_type.as_ref().is_some_and(is_i64)
    {
        return None;
    }
    let ast::Expr::If {
        cond,
        then,
        else_: Some(els),
    } = peel(&f.body)
    else {
        return None;
    };
    let (rec_if, (left, args), base) = match (rec_arm(f, then), rec_arm(f, els)) {
        (Some(rec), None) => (true, rec, &**els),
        (None, Some(rec)) => (false, rec, &**then),
        _ => return None,
    };
    let pure = is_pure(f, cond)
        && is_pure(f, base)
        && is_pure(f, left)
        && args.iter().all(|a| is_pure(f, a));
    pure.then_some(Shape {
        cond,
        rec_if,
        base,
        left,
        args,
    })
}

impl<'ctx> super::Codegen<'ctx> {
    /// Emit `f` (declared as `llvm_fn` under `name`) in the accumulator form
    /// of the module doc when it has that shape and the form is on. `false`:
    /// nothing was emitted, emit the source lowering.
    pub(super) fn emit_accum_fn(
        &mut self,
        name: &str,
        f: &ast::FnDef,
        llvm_fn: FunctionValue<'ctx>,
    ) -> bool {
        if !self.accumulate
            || name != f.name
            || self.functions.get(name) != Some(&llvm_fn)
            || shape(f).is_none()
        {
            return false;
        }
        let ctx = self.ir.context;
        let i64_ty = ctx.i64_type();
        let fn_ty = llvm_fn.get_type();
        let stem = llvm_fn.get_name().to_string_lossy().into_owned();
        let loop_fn = self.ir.module.add_function(&format!("{stem}.loop"), fn_ty, None);
        let orig_fn = self.ir.module.add_function(&format!("{stem}.orig"), fn_ty, None);
        let rest_ty = i64_ty.fn_type(&vec![i64_ty.into(); 2 * f.params.len() + 2], false);
        let rest_fn = self.ir.module.add_function(&format!("{stem}.rest"), rest_ty, None);
        // The loop stays out of line so a call that ends in the base never
        // sets up its frame; the rest and the re-run are cold.
        let attrs: [(FunctionValue<'ctx>, &[&str]); 3] = [
            (loop_fn, &["noinline"]),
            (rest_fn, &["cold", "noinline"]),
            (orig_fn, &["cold", "noinline"]),
        ];
        for (func, names) in attrs {
            for attr in names {
                let kind = Attribute::get_named_enum_kind_id(attr);
                func.add_attribute(AttributeLoc::Function, ctx.create_enum_attribute(kind, 0));
            }
        }
        // The source lowering, its self-calls bound to itself.
        self.functions.insert(name.to_string(), orig_fn);
        self.emit_fn(f, orig_fn);
        self.functions.insert(name.to_string(), llvm_fn);
        self.accum_role = Some(Role::Wrapper { loop_fn });
        self.emit_fn(f, llvm_fn);
        self.accum_role = Some(Role::Loop {
            wrapper: llvm_fn,
            rest_fn,
            orig_fn,
        });
        self.emit_fn(f, loop_fn);
        self.emit_accum_rest(rest_fn, llvm_fn, orig_fn);
        true
    }

    /// The body `emit_fn` emits for `role` instead of `f`'s source body,
    /// with the parameters bound. Every block it leaves ends in a `ret`.
    pub(super) fn emit_accum_body(
        &mut self,
        role: Role<'ctx>,
        f: &ast::FnDef,
        llvm_fn: FunctionValue<'ctx>,
    ) {
        let Some(s) = shape(f) else {
            self.record_error(format!(
                "codegen error: `{}` lost its accumulator shape (AX-78)",
                f.name
            ));
            return;
        };
        let ctx = self.ir.context;
        let i64_ty = ctx.i64_type();
        let a0: Vec<BasicValueEnum<'ctx>> = llvm_fn.get_params();
        let rec_bb = ctx.append_basic_block(llvm_fn, "rec");
        let base_bb = ctx.append_basic_block(llvm_fn, "base");
        match role {
            Role::Wrapper { loop_fn } => {
                let Some(c) = self.accum_int(s.cond, f, llvm_fn) else {
                    return;
                };
                self.accum_branch(c, s.rec_if, rec_bb, base_bb);
                self.ir.builder.position_at_end(base_bb);
                let Some(b) = self.accum_int(s.base, f, llvm_fn) else {
                    return;
                };
                build_wrappers::w_ret(&self.ir.builder, b.into());
                self.ir.builder.position_at_end(rec_bb);
                let r = self.accum_call(loop_fn, &a0);
                build_wrappers::w_ret(&self.ir.builder, r.into());
            }
            Role::Loop {
                wrapper,
                rest_fn,
                orig_fn,
            } => {
                let b = &self.ir.builder;
                let slots: Vec<_> = f.params.iter().map(|p| self.locals[&p.name].0).collect();
                let acc_slot = build_wrappers::w_alloca(b, i64_ty.into(), "acc");
                build_wrappers::w_store(b, acc_slot, i64_ty.const_zero().into());
                let trips_slot = build_wrappers::w_alloca(b, i64_ty.into(), "trips");
                let trips = i64_ty.const_int(TRIPS_PER_FRAME, false);
                build_wrappers::w_store(b, trips_slot, trips.into());
                // The wrapper took the recursive arm: the first trip skips `C`.
                build_wrappers::w_br(b, rec_bb);

                // Next trip: `C`, then the trip budget of this frame.
                let head_bb = ctx.append_basic_block(llvm_fn, "head");
                let budget_bb = ctx.append_basic_block(llvm_fn, "budget");
                self.ir.builder.position_at_end(head_bb);
                let Some(c) = self.accum_int(s.cond, f, llvm_fn) else {
                    return;
                };
                self.accum_branch(c, s.rec_if, budget_bb, base_bb);
                self.ir.builder.position_at_end(budget_bb);
                let left = self.accum_load(trips_slot);
                let b = &self.ir.builder;
                let one = i64_ty.const_int(1, false);
                let left = build_wrappers::w_int_sub(b, left, one, "trips_left");
                build_wrappers::w_store(b, trips_slot, left.into());
                let zero = i64_ty.const_zero();
                let spent = build_wrappers::w_int_compare(b, IntPredicate::EQ, left, zero, "spent");
                let spent = self.accum_unlikely(spent);
                let deep_bb = ctx.append_basic_block(llvm_fn, "deep");
                build_wrappers::w_cond_br(&self.ir.builder, spent, deep_bb, rec_bb);
                // The frame ran its trips: recurse for real, `C` already true.
                self.ir.builder.position_at_end(deep_bb);
                let a: Vec<BasicValueEnum<'ctx>> =
                    slots.iter().map(|&slot| self.accum_load(slot).into()).collect();
                let r = self.accum_call(llvm_fn, &a);
                let acc = self.accum_load(acc_slot);
                self.accum_finish(acc, r, orig_fn, &a0);

                self.ir.builder.position_at_end(base_bb);
                let Some(bv) = self.accum_int(s.base, f, llvm_fn) else {
                    return;
                };
                let acc = self.accum_load(acc_slot);
                self.accum_finish(acc, bv, orig_fn, &a0);

                self.ir.builder.position_at_end(rec_bb);
                let Some(t) = self.accum_int(s.left, f, llvm_fn) else {
                    return;
                };
                let mut a1 = Vec::with_capacity(s.args.len());
                for arg in s.args {
                    let Some(v) = self.accum_int(arg, f, llvm_fn) else {
                        return;
                    };
                    a1.push(BasicValueEnum::from(v));
                }
                let acc = self.accum_load(acc_slot);
                let b = &self.ir.builder;
                let acc2 = build_wrappers::w_int_add(b, acc, t, "acc_next");
                let signs = build_wrappers::w_or(b, acc2, t, "acc_signs");
                let zero = i64_ty.const_zero();
                let off = build_wrappers::w_int_compare(b, IntPredicate::SLT, signs, zero, "acc_off");
                let off = self.accum_unlikely(off);
                let off_bb = ctx.append_basic_block(llvm_fn, "acc_off");
                let next_bb = ctx.append_basic_block(llvm_fn, "next");
                build_wrappers::w_cond_br(&self.ir.builder, off, off_bb, next_bb);

                // Off the fast path: finish in source order.
                self.ir.builder.position_at_end(off_bb);
                let b = &self.ir.builder;
                let empty = build_wrappers::w_int_compare(b, IntPredicate::EQ, acc, zero, "acc_empty");
                let alone_bb = ctx.append_basic_block(llvm_fn, "alone");
                let defer_bb = ctx.append_basic_block(llvm_fn, "defer");
                build_wrappers::w_cond_br(b, empty, alone_bb, defer_bb);
                self.ir.builder.position_at_end(alone_bb);
                let s0 = self.accum_call(wrapper, &a1);
                let Some(sum) =
                    self.emit_checked_overflow_op("llvm.sadd.with.overflow", "+", t, s0)
                else {
                    return;
                };
                build_wrappers::w_ret(&self.ir.builder, sum);
                self.ir.builder.position_at_end(defer_bb);
                let pair: [BasicValueEnum<'ctx>; 2] = [acc.into(), t.into()];
                let rest_args = [&a0[..], &pair[..], &a1[..]].concat();
                let r = self.accum_call(rest_fn, &rest_args);
                build_wrappers::w_ret(&self.ir.builder, r.into());

                // On the fast path: defer the add, take the next trip.
                self.ir.builder.position_at_end(next_bb);
                let b = &self.ir.builder;
                for (slot, v) in slots.iter().zip(&a1) {
                    build_wrappers::w_store(b, *slot, *v);
                }
                build_wrappers::w_store(b, acc_slot, acc2.into());
                build_wrappers::w_br(b, head_bb);
            }
        }
    }

    /// `f.rest(a0.., acc, t, a1..)`: the rest of the evaluation of
    /// `f(a0..)` in source order from the trip whose left operand is `t` and
    /// whose arguments are `a1`, with `acc` the sum of the left operands
    /// before it: `acc + (t + f(a1..))`, the inner add the source's own.
    fn emit_accum_rest(
        &mut self,
        rest_fn: FunctionValue<'ctx>,
        wrapper: FunctionValue<'ctx>,
        orig_fn: FunctionValue<'ctx>,
    ) {
        let entry = self.ir.context.append_basic_block(rest_fn, "entry");
        self.ir.builder.position_at_end(entry);
        let params = rest_fn.get_params();
        let m = (params.len() - 2) / 2;
        let (a0, rest) = params.split_at(m);
        let (acc, t, a1) = (rest[0].into_int_value(), rest[1].into_int_value(), &rest[2..]);
        let s0 = self.accum_call(wrapper, a1);
        let Some(s) = self.emit_checked_overflow_op("llvm.sadd.with.overflow", "+", t, s0) else {
            return;
        };
        self.accum_finish(acc, s.into_int_value(), orig_fn, a0);
    }

    /// Return `acc + s`; when that overflows, the source overflows too and
    /// `orig_fn(a0..)` re-evaluates the call to raise the source's panic.
    fn accum_finish(
        &mut self,
        acc: IntValue<'ctx>,
        s: IntValue<'ctx>,
        orig_fn: FunctionValue<'ctx>,
        a0: &[BasicValueEnum<'ctx>],
    ) {
        let Some((value, ovf)) = self.accum_sadd(acc, s) else {
            return;
        };
        let ovf = self.accum_unlikely(ovf);
        let Some(cur) = self.ir.builder.get_insert_block().and_then(|bb| bb.get_parent()) else {
            return;
        };
        let done_bb = self.ir.context.append_basic_block(cur, "acc_done");
        let rerun_bb = self.ir.context.append_basic_block(cur, "acc_rerun");
        build_wrappers::w_cond_br(&self.ir.builder, ovf, rerun_bb, done_bb);
        self.ir.builder.position_at_end(done_bb);
        build_wrappers::w_ret(&self.ir.builder, value.into());
        self.ir.builder.position_at_end(rerun_bb);
        let r = self.accum_call(orig_fn, a0);
        build_wrappers::w_ret(&self.ir.builder, r.into());
    }

    /// `llvm.sadd.with.overflow(l, r)`: the wrapped sum and the overflow bit.
    fn accum_sadd(
        &mut self,
        l: IntValue<'ctx>,
        r: IntValue<'ctx>,
    ) -> Option<(IntValue<'ctx>, IntValue<'ctx>)> {
        let i64_ty = self.ir.context.i64_type();
        let intr = inkwell::intrinsics::Intrinsic::find("llvm.sadd.with.overflow")?;
        let decl = intr.get_declaration(&self.ir.module, &[i64_ty.into()])?;
        let b = &self.ir.builder;
        let call = build_wrappers::w_call(b, decl, &[l.into(), r.into()], "acc_sum");
        let agg = call.try_as_basic_value().basic()?.into_struct_value();
        let value = build_wrappers::w_extract_value(b, agg, 0, "acc_total").into_int_value();
        let ovf = build_wrappers::w_extract_value(b, agg, 1, "acc_ovf").into_int_value();
        Some((value, ovf))
    }

    /// `c` marked as almost never true (`llvm.expect`), so the paths it
    /// guards lay out out of the loop.
    fn accum_unlikely(&self, c: IntValue<'ctx>) -> IntValue<'ctx> {
        let i1 = self.ir.context.bool_type();
        let decl = inkwell::intrinsics::Intrinsic::find("llvm.expect")
            .and_then(|intr| intr.get_declaration(&self.ir.module, &[i1.into()]));
        let Some(decl) = decl else {
            return c;
        };
        let args = [c.into(), i1.const_zero().into()];
        build_wrappers::w_call(&self.ir.builder, decl, &args, "unlikely")
            .try_as_basic_value()
            .basic()
            .map_or(c, BasicValueEnum::into_int_value)
    }

    fn accum_branch(
        &self,
        c: IntValue<'ctx>,
        rec_if: bool,
        rec_bb: inkwell::basic_block::BasicBlock<'ctx>,
        base_bb: inkwell::basic_block::BasicBlock<'ctx>,
    ) {
        let (then_bb, else_bb) = if rec_if {
            (rec_bb, base_bb)
        } else {
            (base_bb, rec_bb)
        };
        build_wrappers::w_cond_br(&self.ir.builder, c, then_bb, else_bb);
    }

    fn accum_load(&self, slot: inkwell::values::PointerValue<'ctx>) -> IntValue<'ctx> {
        let i64_ty = self.ir.context.i64_type();
        build_wrappers::w_load(&self.ir.builder, i64_ty.into(), slot, "acc_slot").into_int_value()
    }

    fn accum_call(
        &self,
        callee: FunctionValue<'ctx>,
        args: &[BasicValueEnum<'ctx>],
    ) -> IntValue<'ctx> {
        let args: Vec<BasicMetadataValueEnum<'ctx>> = args.iter().map(|&a| a.into()).collect();
        build_wrappers::w_call(&self.ir.builder, callee, &args, "acc_call")
            .try_as_basic_value()
            .basic()
            .expect("an i64 fn returns a value")
            .into_int_value()
    }

    /// `e` lowered as the source lowers it; `None` (an error recorded) when
    /// it does not yield an integer.
    fn accum_int(
        &mut self,
        e: &ast::Expr,
        f: &ast::FnDef,
        llvm_fn: FunctionValue<'ctx>,
    ) -> Option<IntValue<'ctx>> {
        match self.emit_expr(e, llvm_fn) {
            Some(BasicValueEnum::IntValue(v)) => Some(v),
            _ => {
                self.record_error(format!(
                    "codegen error: a part of `{}` did not lower to an integer (AX-78)",
                    f.name
                ));
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::shape;
    use crate::ast;

    /// Whether the last fn of `src` has the accumulator shape.
    fn shaped(src: &str) -> bool {
        let prog = crate::parse_source(src).expect("fixture must parse");
        let f = prog
            .items
            .iter()
            .rev()
            .find_map(|i| match i {
                ast::Item::FnDef(f) => Some(f),
                _ => None,
            })
            .expect("a fn");
        shape(f).is_some()
    }

    /// The shape is a pure `if` with one arm `L + f(A)`: any operators,
    /// literals, parameters, `if`s and self-calls in `C`, `B`, `L` and `A`,
    /// either arm recursive, any number of i64 parameters.
    #[test]
    fn pure_self_recursive_adds_have_the_shape() {
        for src in [
            "fn fib(n: i64) -> i64 { if n < 2 { n } else { fib(n - 1) + fib(n - 2) } }",
            "fn s(n: i64, k: i64) -> i64 { if n > 0 { k * n + s(n - 1, k / 2) } else { 0 } }",
            "fn d(n: i64) -> i64 { if n == 0 { 1 } else { (if n % 2 == 0 { 0 - n } else { n }) + d(n - 1) } }",
        ] {
            assert!(shaped(src), "{src}");
        }
    }

    /// Anything that could observe the moved adds, or that the loop could not
    /// re-run as the source does, keeps the source lowering: an effect, a
    /// call of another fn, a `let`, a non-i64 parameter or result, the call
    /// on the left (it is evaluated first), another operator, no `else`.
    #[test]
    fn impure_or_other_recursions_do_not() {
        for src in [
            "fn p(n: i64) -> i64 { if n < 1 { 0 } else { println(to_str(n))\n n + p(n - 1) } }",
            "fn e(n: i64) -> i64 { if n < 1 { 0 } else { str_len(to_str(n)) + e(n - 1) } }",
            "fn g(n: i64) -> i64 { n }\nfn q(n: i64) -> i64 { if n < 1 { 0 } else { g(n) + q(n - 1) } }",
            "fn l(n: i64) -> i64 { if n < 1 { 0 } else { let m = n - 1\n n + l(m) } }",
            "fn u(n: i32) -> i32 { if n < 1 { 0 } else { n + u(n - 1) } }",
            "fn r(n: i64) -> i64 { if n < 1 { 0 } else { r(n - 1) + n } }",
            "fn m(n: i64) -> i64 { if n < 1 { 1 } else { n * m(n - 1) } }",
            "fn b(n: i64) -> i64 { if n < 1 { 0 } else { n + b(n - 1) + 1 } }",
            "fn t(n: i64) -> i64 { if n < 1 { 0 } else { t(n - 1) + t(n - 2) + 1 } }",
        ] {
            assert!(!shaped(src), "{src}");
        }
    }
}
