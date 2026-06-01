//! Expression-emission methods on `Codegen<'ctx>`.
//!
//! Phase 2.7 of the §7.5 module split: the giant `emit_expr` method
//! (~1380 lines) plus closely-related literal / binop / if-else emitters.
//!
//! No decomposition done here — `emit_expr` is moved as one giant single
//! method.  Decomposing it into per-`Expr` variant helpers is a separate
//! refactor, deferred until the codegen-feature build is fast enough to
//! validate iteratively.
//!
//! Visibility: all methods `pub(super)` so the parent `codegen::mod` can
//! call them from `emit_fn` (the function-body emission entry point).
//!
//! Imports are deliberately broad — `emit_expr` uses essentially the
//! whole inkwell surface.

#[allow(unused_imports)]
use std::collections::HashMap;

use inkwell::types::{BasicType, BasicTypeEnum, BasicMetadataTypeEnum, StructType};
use inkwell::values::{
    BasicMetadataValueEnum, BasicValueEnum, FunctionValue, InstructionOpcode, PointerValue,
};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::FloatPredicate;

use crate::ast;
use crate::types::Type;

use super::build_wrappers;

impl<'ctx> super::Codegen<'ctx> {
    // ── Expression emission ───────────────────────────────────────────────────

    /// Core expression emitter. Returns the LLVM value (or None for Unit/void).
    pub(super) fn emit_expr(
        &mut self,
        expr: &ast::Expr,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        match expr {
            // ── Literal ──────────────────────────────────────────────────────
            ast::Expr::Literal(lit) => Some(self.emit_literal(lit)),

            // ── Identifier (load from local) ─────────────────────────────────
            ast::Expr::Ident(name) => {
                if let Some((ptr, llvm_ty)) = self.locals.get(name).cloned() {
                    let val = build_wrappers::w_load(&self.ir.builder, llvm_ty.into(), ptr, name);
                    return Some(val);
                }
                // Fall back to checking module-level comptime constants.
                if let Some(cv) = self.comptime_env.get(name).cloned() {
                    return Some(self.comptime_val_to_llvm(&cv));
                }
                // Fall back to checking for a function (first-class fn value).
                if let Some(fn_v) = self.functions.get(name).copied() {
                    let ptr: PointerValue = fn_v.as_global_value().as_pointer_value();
                    return Some(ptr.into());
                }
                // Closure-env fallback: if we're emitting a lambda body and the
                // resolver listed `name` as a capture, load it from the env
                // struct via GEP. The primary path (see Lambda handler) already
                // binds capture field-pointers into `self.locals`; this is a
                // safety net for resolver gaps and AST rewrites that introduce
                // new identifiers after `fill_captures` ran.
                let env_lookup: Option<(PointerValue<'ctx>, StructType<'ctx>, u32)> =
                    self.current_lambda_env.as_ref().and_then(|(env_ptr, env_ty, idx_map)| {
                        idx_map.get(name).map(|&idx| (*env_ptr, *env_ty, idx))
                    });
                if let Some((env_ptr, env_ty, idx)) = env_lookup {
                    let field_ptr = self.ir.builder
                        .build_struct_gep(env_ty, env_ptr, idx, name)
                        .unwrap();
                    let i64_ty = self.ir.context.i64_type();
                    let val = self.ir.builder
                        .build_load(i64_ty, field_ptr, name)
                        .unwrap();
                    return Some(val);
                }
                // Genuinely unknown identifier — emit a diagnostic and return
                // None so the caller can decide whether to recover or abort.
                eprintln!(
                    "codegen error [E0701]: identifier '{}' not found in current scope",
                    name
                );
                None
            }

            // ── Let / Own / RefBind ──────────────────────────────────────────
            ast::Expr::Let { name, value, .. }
            | ast::Expr::Own { name, value, .. }
            | ast::Expr::RefBind { name, value, .. } => {
                let sem_ty = self.infer_expr_sem_type(value);
                let val = self.emit_expr(value, fn_val)?;
                let alloca = build_wrappers::w_alloca(&self.ir.builder, val.get_type(), name);
                build_wrappers::w_store(&self.ir.builder, alloca, val);
                self.locals.insert(name.clone(), (alloca, val.get_type()));
                if let Some(ty) = sem_ty {
                    self.local_types.insert(name.clone(), ty);
                }
                None
            }

            // ── Block ────────────────────────────────────────────────────────
            ast::Expr::Block(stmts) => {
                let mut last = None;
                for stmt in stmts {
                    last = self.emit_expr(&stmt.expr, fn_val);
                }
                last
            }

            // ── Binary operation ─────────────────────────────────────────────
            ast::Expr::BinOp { op, left, right } => {
                // Layer-2 ASI: if either operand is `Uncertain<T>`, route through
                // the dedicated emitter that propagates `min(c1, c2)` confidence.
                let lt_sem = self.infer_expr_sem_type(left);
                let rt_sem = self.infer_expr_sem_type(right);
                let is_unc = |t: &Option<Type>| matches!(t, Some(Type::Uncertain(_)));
                if is_unc(&lt_sem) || is_unc(&rt_sem) {
                    return self.emit_binop_uncertain(op, left, right, &lt_sem, &rt_sem, fn_val);
                }
                let lhs = self.emit_expr(left, fn_val)?;
                let rhs = self.emit_expr(right, fn_val)?;
                // Prefer the semantic type from inference (distinguishes u32/u64
                // from i32/i64) then fall back to the LLVM-level value hint.
                let ty = lt_sem.unwrap_or_else(|| self.value_type_hint(&lhs));
                Some(self.emit_binop(op, lhs, rhs, &ty))
            }

            // ── Unary operation ──────────────────────────────────────────────
            ast::Expr::UnaryOp { op, operand } => {
                let val = self.emit_expr(operand, fn_val)?;
                match op {
                    ast::UnaryOp::Neg => match val {
                        BasicValueEnum::IntValue(i) => {
                            let neg = build_wrappers::w_int_neg(&self.ir.builder, i, "neg");
                            Some(neg.into())
                        }
                        BasicValueEnum::FloatValue(f) => {
                            let neg = build_wrappers::w_float_neg(&self.ir.builder, f, "fneg");
                            Some(neg.into())
                        }
                        _ => None,
                    },
                    ast::UnaryOp::Not => match val {
                        BasicValueEnum::IntValue(i) => {
                            let r = build_wrappers::w_not(&self.ir.builder, i, "not");
                            Some(r.into())
                        }
                        _ => None,
                    },
                    ast::UnaryOp::Ref => {
                        // Reference is currently a no-op at the LLVM level: all
                        // values are passed by value (i64-wide) and the borrow
                        // checker enforces aliasing rules at the AST level.
                        // True address-taking requires a re-design of the local
                        // ABI (alloca-everywhere or escape analysis) — tracked
                        // for a future phase rather than emitted as a stub here.
                        Some(val)
                    }
                    ast::UnaryOp::BitNot => match val {
                        BasicValueEnum::IntValue(i) => {
                            // LLVM `not` on an integer flips all bits — identical
                            // to C's `~` operator.
                            let r = build_wrappers::w_not(&self.ir.builder, i, "bitnot");
                            Some(r.into())
                        }
                        _ => None,
                    },
                }
            }

            // ── Function call ─────────────────────────────────────────────────
            ast::Expr::Call { callee, args } => self.emit_call(callee, args, fn_val),

            // ── Method call — dispatches to mangled `TypeName__method` fn ──────
            ast::Expr::MethodCall { receiver, method, args } => self.emit_method_call(receiver, method, args, fn_val),

            // ── If / else ─────────────────────────────────────────────────────
            ast::Expr::If { cond, then, else_ } => {
                let cond_val = self.emit_expr(cond, fn_val)?;
                let cond_int = match cond_val {
                    BasicValueEnum::IntValue(i) => i,
                    _ => return None,
                };
                self.emit_if(cond_int.into(), then, else_.as_deref(), fn_val)
            }

            // ── Match ─────────────────────────────────────────────────────────
            ast::Expr::Match { subject, arms } => {
                let subj_sem_ty = self.infer_expr_sem_type(subject);
                let subj_val = self.emit_expr(subject, fn_val)?;
                // Temporarily override current_result_types when matching a Result,
                // so pattern binding can extract typed payloads from the union.
                let saved_result_types = self.current_result_types.clone();
                if let Some(Type::Result(ok_ty, err_ty)) = &subj_sem_ty {
                    self.current_result_types = Some((*ok_ty.clone(), *err_ty.clone()));
                }
                let result = self.emit_match(subj_val, arms, fn_val);
                self.current_result_types = saved_result_types;
                result
            }

            // ── ? operator ────────────────────────────────────────────────────
            ast::Expr::Question(inner) => {
                let val = self.emit_expr(inner, fn_val)?;
                Some(self.emit_question(val, fn_val))
            }

            // ── Ok / Err wrappers ─────────────────────────────────────────────
            ast::Expr::Ok(inner) => {
                let val = self.emit_expr(inner, fn_val)?;
                Some(self.emit_result(true, val))
            }
            ast::Expr::Err(inner) => {
                let val = self.emit_expr(inner, fn_val)?;
                Some(self.emit_result(false, val))
            }

            // ── Some / None wrappers ──────────────────────────────────────────
            ast::Expr::Some(inner) => {
                let val = self.emit_expr(inner, fn_val)?;
                let ty = self.value_type_hint(&val);
                Some(self.emit_option(std::option::Option::Some(val), &ty))
            }
            ast::Expr::None => {
                // Emit Option<i64 placeholder> with no inner value.
                let placeholder = Type::I64;
                Some(self.emit_option(std::option::Option::None, &placeholder))
            }

            // ── Return ────────────────────────────────────────────────────────
            ast::Expr::Return(maybe_val) => self.emit_return(maybe_val, fn_val),

            // ── Array literal ─────────────────────────────────────────────────
            ast::Expr::Array(elems) => self.emit_array_lit(elems, fn_val),

            // ── Tuple literal ─────────────────────────────────────────────────
            // Native codegen for tuples is not yet implemented; the interpreter
            // path handles them. Keep the arm to satisfy exhaustiveness.
            ast::Expr::Tuple(_) => None,

            // ── Struct literal: Name { field: expr, ... } ─────────────────────
            ast::Expr::StructLit { name, fields } => self.emit_struct_lit(name, fields, fn_val),

            // ── Field access: receiver.field ──────────────────────────────────
            ast::Expr::FieldAccess { receiver, field } => self.emit_field_access(receiver, field, fn_val),

            // ── Index: receiver[index] ────────────────────────────────────────
            ast::Expr::Index { receiver, index } => self.emit_index(receiver, index, fn_val),

            // ── Spawn: compile lambda then call __axon_spawn(fn_ptr, env_ptr) ──
            ast::Expr::Spawn(inner) => self.emit_spawn(inner, fn_val),

            // ── Comptime: evaluate at compile time, emit LLVM constant ──────────
            ast::Expr::Comptime(inner) => self.emit_comptime_expr(inner),

            // ── Lambda: lower to a named module-level function with closure ABI ─
            //
            // Closure ABI (Phase 4):
            //   - Every lambda function's LLVM signature is:
            //       fn(__env: i8*, param0: i64, param1: i64, ...) -> i64
            //   - If the lambda has captures, we malloc an env struct at the
            //     creation site and populate it with the current values.
            //   - Inside the lambda body, loads of captured names go through
            //     the env struct (GEP + load).
            //   - The result value is a fat-pointer struct `{ i8*, i8* }`:
            //       (fn_ptr, env_ptr)  — env_ptr is null for capture-free lambdas.
            ast::Expr::Lambda { params, body, captures } => self.emit_lambda(params, body, captures, fn_val),

            // ── Select (phase 1: stub) ────────────────────────────────────────
            // ── Select: non-blocking channel dispatch ────────────────────────
            // Lowers to:
            //   chans[n] = { emit_expr(arm.recv) for each arm }
            //   let ready = __axon_select(chans, n)
            //   switch ready -> arm bodies
            ast::Expr::Select(arms) => self.emit_select_expr(arms, fn_val),

            // ── While loop ────────────────────────────────────────────────────
            ast::Expr::While { cond, body } => self.emit_while(cond, body, fn_val),

            // ── While-let loop ───────────────────────────────────────────────
            // `while let <pattern> = <expr> { body }` — compiled as:
            //   loop { val = expr; if !pattern_matches(val) { break }; bind; body }
            ast::Expr::WhileLet { pattern, expr, body } => self.emit_while_let(pattern, expr, body, fn_val),

            // ── For-in range loop ─────────────────────────────────────────────
            // `for i in start..end { body }` or `start..=end` (inclusive).
            ast::Expr::For { var, start, end, body, inclusive } => self.emit_for_in(var, start, end, body, *inclusive, fn_val),

            // ── Break / Continue ──────────────────────────────────────────────
            ast::Expr::Break => {
                if let Some(&(_cont, exit)) = self.loop_stack.last() {
                    build_wrappers::w_br(&self.ir.builder, exit);
                }
                None
            }
            ast::Expr::Continue => {
                if let Some(&(cont, _exit)) = self.loop_stack.last() {
                    build_wrappers::w_br(&self.ir.builder, cont);
                }
                None
            }

            // ── Assign (rebind existing local without let) ────────────────────
            ast::Expr::Assign { name, value } => {
                if let Some(val) = self.emit_expr(value, fn_val) {
                    if let Some((ptr, _llvm_ty)) = self.locals.get(name).copied() {
                        build_wrappers::w_store(&self.ir.builder, ptr, val);
                    }
                }
                None
            }

            // Place assignment (`xs[i] = v`, `s.field = v`) isn't lowered to
            // native code yet; the interpreter is the supported execution path.
            ast::Expr::AssignTo { .. } => None,

            // ── FmtStr: lower to a chain of axon_concat calls ────────────────
            ast::Expr::FmtStr { parts } => self.emit_fmt_str(parts, fn_val),
        }
    }

    // ── Literal emission ──────────────────────────────────────────────────────

    pub(super) fn comptime_val_to_llvm(&self, cv: &crate::comptime::ComptimeVal) -> BasicValueEnum<'ctx> {
        use crate::comptime::ComptimeVal;
        match cv {
            ComptimeVal::Int(n) => self.ir.context.i64_type().const_int(*n as u64, true).into(),
            ComptimeVal::Bool(b) => self.ir.context.bool_type().const_int(*b as u64, false).into(),
            ComptimeVal::Float(f) => self.ir.context.f64_type().const_float(*f).into(),
            ComptimeVal::Str(s) => {
                let global = build_wrappers::w_global_string_ptr(&self.ir.builder, s, "comptime_str");
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let mut sv = str_ty.get_undef();
                sv = build_wrappers::w_insert_value(&self.ir.builder, sv, i64_ty.const_int(s.len() as u64, false).into(), 0, "s_len").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder, sv, global.into(), 1, "s_ptr").into_struct_value();
                sv.into()
            }
        }
    }

    pub(super) fn emit_literal(&self, lit: &ast::Literal) -> BasicValueEnum<'ctx> {
        match lit {
            ast::Literal::Int(n) => {
                self.ir.context
                    .i64_type()
                    .const_int(*n as u64, /*sign_extend=*/ true)
                    .into()
            }
            ast::Literal::Float(f) => {
                self.ir.context.f64_type().const_float(*f).into()
            }
            ast::Literal::Bool(b) => {
                self.ir.context
                    .bool_type()
                    .const_int(if *b { 1 } else { 0 }, false)
                    .into()
            }
            ast::Literal::Str(s) => {
                // Build a global constant for the string bytes, then construct
                // the { i64, ptr } struct.
                let bytes = s.as_bytes();
                let len_val = self.ir.context.i64_type().const_int(bytes.len() as u64, false);

                // Create a global byte array for the string data.
                let i8_ty = self.ir.context.i8_type();
                let arr_ty = i8_ty.array_type(bytes.len() as u32 + 1); // null-terminated
                // Use add_global which auto-dedups by letting LLVM pick unique names.
                let global = self.ir.module.add_global(arr_ty, None, "str_data");
                let byte_vals: Vec<_> = bytes
                    .iter()
                    .chain(std::iter::once(&0u8)) // null terminator
                    .map(|&b| i8_ty.const_int(b as u64, false))
                    .collect();
                global.set_initializer(&i8_ty.const_array(&byte_vals));
                global.set_constant(true);

                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let ptr = global.as_pointer_value();
                let cast_ptr = self.ir
                    .builder
                    .build_pointer_cast(ptr, ptr_ty, "strptr")
                    .unwrap();

                let i64_ty = self.ir.context.i64_type();
                let str_ty = self.ir.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()],
                    false,
                );
                // Build the struct value via an alloca + stores.
                let alloca = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "strlit");
                let len_ptr = self.ir
                    .builder
                    .build_struct_gep(str_ty, alloca, 0, "lenptr")
                    .unwrap();
                build_wrappers::w_store(&self.ir.builder, len_ptr, len_val.into());
                let data_ptr = self.ir
                    .builder
                    .build_struct_gep(str_ty, alloca, 1, "dataptr")
                    .unwrap();
                build_wrappers::w_store(&self.ir.builder, data_ptr, cast_ptr.into());
                build_wrappers::w_load(&self.ir.builder, str_ty.into(), alloca, "strval")
            }
        }
    }

    // ── Binary operation emission ─────────────────────────────────────────────

    pub(super) fn emit_binop(
        &mut self,
        op: &ast::BinOp,
        lhs: BasicValueEnum<'ctx>,
        rhs: BasicValueEnum<'ctx>,
        ty: &Type,
    ) -> BasicValueEnum<'ctx> {
        // True when the semantic type is an unsigned integer.
        let is_unsigned = matches!(ty, Type::U8 | Type::U16 | Type::U32 | Type::U64);

        match (lhs, rhs) {
            // Integer arithmetic.
            (BasicValueEnum::IntValue(l), BasicValueEnum::IntValue(r)) => match op {
                ast::BinOp::Add => build_wrappers::w_int_add(&self.ir.builder, l, r, "add").into(),
                ast::BinOp::Sub => build_wrappers::w_int_sub(&self.ir.builder, l, r, "sub").into(),
                ast::BinOp::Mul => build_wrappers::w_int_mul(&self.ir.builder, l, r, "mul").into(),
                ast::BinOp::Div => if is_unsigned {
                    build_wrappers::w_int_unsigned_div(&self.ir.builder, l, r, "udiv").into()
                } else {
                    build_wrappers::w_int_signed_div(&self.ir.builder, l, r, "div").into()
                },
                ast::BinOp::Eq => self.ir
                    .builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .unwrap()
                    .into(),
                ast::BinOp::NotEq => self.ir
                    .builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .unwrap()
                    .into(),
                ast::BinOp::Lt => self.ir
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::ULT } else { IntPredicate::SLT },
                        l, r, "lt",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::Gt => self.ir
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::UGT } else { IntPredicate::SGT },
                        l, r, "gt",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::LtEq => self.ir
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::ULE } else { IntPredicate::SLE },
                        l, r, "le",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::GtEq => self.ir
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::UGE } else { IntPredicate::SGE },
                        l, r, "ge",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::Rem => if is_unsigned {
                    build_wrappers::w_int_unsigned_rem(&self.ir.builder, l, r, "urem").into()
                } else {
                    build_wrappers::w_int_signed_rem(&self.ir.builder, l, r, "rem").into()
                },
                ast::BinOp::And => build_wrappers::w_and(&self.ir.builder, l, r, "and").into(),
                ast::BinOp::Or => build_wrappers::w_or(&self.ir.builder, l, r, "or").into(),
                ast::BinOp::BitAnd => build_wrappers::w_and(&self.ir.builder, l, r, "band").into(),
                ast::BinOp::BitOr  => build_wrappers::w_or(&self.ir.builder, l, r, "bor").into(),
                ast::BinOp::BitXor => build_wrappers::w_xor(&self.ir.builder, l, r, "bxor").into(),
                ast::BinOp::Shl => build_wrappers::w_left_shift(&self.ir.builder, l, r, "shl").into(),
                ast::BinOp::Shr => if is_unsigned {
                    build_wrappers::w_right_shift(&self.ir.builder, l, r, false, "lshr").into()
                } else {
                    build_wrappers::w_right_shift(&self.ir.builder, l, r, true, "ashr").into()
                },
            },

            // Float arithmetic.
            (BasicValueEnum::FloatValue(l), BasicValueEnum::FloatValue(r)) => match op {
                ast::BinOp::Add => build_wrappers::w_float_add(&self.ir.builder, l, r, "fadd").into(),
                ast::BinOp::Sub => build_wrappers::w_float_sub(&self.ir.builder, l, r, "fsub").into(),
                ast::BinOp::Mul => build_wrappers::w_float_mul(&self.ir.builder, l, r, "fmul").into(),
                ast::BinOp::Div => build_wrappers::w_float_div(&self.ir.builder, l, r, "fdiv").into(),
                ast::BinOp::Eq => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, l, r, "feq")
                    .unwrap()
                    .into(),
                ast::BinOp::NotEq => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::ONE, l, r, "fne")
                    .unwrap()
                    .into(),
                ast::BinOp::Lt => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::OLT, l, r, "flt")
                    .unwrap()
                    .into(),
                ast::BinOp::Gt => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::OGT, l, r, "fgt")
                    .unwrap()
                    .into(),
                ast::BinOp::LtEq => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::OLE, l, r, "fle")
                    .unwrap()
                    .into(),
                ast::BinOp::GtEq => self.ir
                    .builder
                    .build_float_compare(FloatPredicate::OGE, l, r, "fge")
                    .unwrap()
                    .into(),
                ast::BinOp::Rem => build_wrappers::w_float_rem(&self.ir.builder, l, r, "frem").into(),
                // Bool ops on floats — truncate to i1 first.
                ast::BinOp::And | ast::BinOp::Or => {
                    let zero = l.get_type().const_zero();
                    let li = self.ir
                        .builder
                        .build_float_compare(FloatPredicate::ONE, l, zero, "ftoi_l")
                        .unwrap();
                    let ri = self.ir
                        .builder
                        .build_float_compare(FloatPredicate::ONE, r, zero, "ftoi_r")
                        .unwrap();
                    match op {
                        ast::BinOp::And => build_wrappers::w_and(&self.ir.builder, li, ri, "fand").into(),
                        _ => build_wrappers::w_or(&self.ir.builder, li, ri, "for").into(),
                    }
                }
                // Bitwise ops on floats are rejected by the type-checker; unreachable here.
                ast::BinOp::BitAnd | ast::BinOp::BitOr | ast::BinOp::BitXor
                | ast::BinOp::Shl  | ast::BinOp::Shr => {
                    unreachable!("bitwise op on float — should have been rejected by the type checker")
                }
            },

            // String struct equality: `a == b` / `a != b` where both are { i64, i8* }.
            // Delegates to the str_eq builtin function declared in declare_builtins.
            (BasicValueEnum::StructValue(l), BasicValueEnum::StructValue(r))
                if matches!(op, ast::BinOp::Eq | ast::BinOp::NotEq) =>
            {
                let str_eq_fn = self.ir.module.get_function("str_eq");
                if let Some(eq_fn) = str_eq_fn {
                    let result = self.ir.builder
                        .build_call(eq_fn, &[l.into(), r.into()], "seq")
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_int_value();
                    if matches!(op, ast::BinOp::NotEq) {
                        // Flip the result: NotEq = !Eq
                        build_wrappers::w_not(&self.ir.builder, result, "sne").into()
                    } else {
                        result.into()
                    }
                } else {
                    // str_eq not declared yet — return false (shouldn't happen)
                    self.ir.context.bool_type().const_int(0, false).into()
                }
            }

            // Mismatched or unsupported — return lhs unchanged.
            (l, _) => l,
        }
    }

    // ── If/else emission ──────────────────────────────────────────────────────

    pub(super) fn emit_if(
        &mut self,
        cond: BasicValueEnum<'ctx>,
        then_expr: &ast::Expr,
        else_expr: Option<&ast::Expr>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let cond_int = match cond {
            BasicValueEnum::IntValue(i) => i,
            _ => return None,
        };

        let then_bb = self.ir.context.append_basic_block(fn_val, "if_then");
        let else_bb = self.ir.context.append_basic_block(fn_val, "if_else");
        let merge_bb = self.ir.context.append_basic_block(fn_val, "if_merge");

        self.ir.builder
            .build_conditional_branch(cond_int, then_bb, else_bb)
            .unwrap();

        // Then branch.
        self.ir.builder.position_at_end(then_bb);
        let then_val = self.emit_expr(then_expr, fn_val);
        let then_end = self.ir.builder.get_insert_block().unwrap();
        if then_end.get_terminator().is_none() {
            build_wrappers::w_br(&self.ir.builder, merge_bb);
        }

        // Else branch.
        self.ir.builder.position_at_end(else_bb);
        let else_val = if let Some(e) = else_expr {
            self.emit_expr(e, fn_val)
        } else {
            None
        };
        let else_end = self.ir.builder.get_insert_block().unwrap();
        if else_end.get_terminator().is_none() {
            build_wrappers::w_br(&self.ir.builder, merge_bb);
        }

        self.ir.builder.position_at_end(merge_bb);

        // Build phi if both branches produce a value of the same type.
        match (then_val, else_val) {
            (Some(tv), Some(ev)) if tv.get_type() == ev.get_type() => {
                let phi = build_wrappers::w_phi(&self.ir.builder, tv.get_type(), "ifval");
                phi.add_incoming(&[(&tv, then_end), (&ev, else_end)]);
                Some(phi.as_basic_value())
            }
            (Some(tv), None) => {
                // No else branch. We need a phi only if then_end actually flows
                // to merge_bb (i.e., it did not end with `return`).
                // else_end always flows to merge_bb (unconditional branch above).
                let zero = tv.get_type().const_zero();
                // Check if then_end branches to merge_bb (not a return).
                let then_flows_to_merge = then_end.get_terminator()
                    .map(|t| {
                        // It's a branch, not unreachable/return
                        t.get_opcode() == InstructionOpcode::Br
                    })
                    .unwrap_or(false);
                if then_flows_to_merge {
                    let phi = build_wrappers::w_phi(&self.ir.builder, tv.get_type(), "ifval");
                    phi.add_incoming(&[(&tv, then_end), (&zero, else_end)]);
                    Some(phi.as_basic_value())
                } else {
                    // then_end returns — merge_bb only has else_end as predecessor.
                    // Return zero as the value (the if-without-else produces Unit).
                    Some(zero)
                }
            }
            _ => None,
        }
    }
    // ── Phase 3 decomposition: per-Expr-variant helper methods ────────

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_fmt_str(&mut self, parts: &[ast::FmtPart], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        // We build the result left-to-right:
        //   acc = ""
        //   for each part: acc = axon_concat(acc, part_value)
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i64_ty = self.ir.context.i64_type();
        let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

        // Start with an empty string literal (use a unique name per fmtstr).
        let fmtstr_id = self.fmtstr_counter;
        self.fmtstr_counter += 1;
        let empty_arr_ty = self.ir.context.i8_type().array_type(1);
        let empty_name = format!("fmtstr_empty_{fmtstr_id}");
        let empty_global = self.ir.module.add_global(empty_arr_ty, None, &empty_name);
        empty_global.set_initializer(
            &self.ir.context.i8_type().const_array(&[self.ir.context.i8_type().const_int(0, false)])
        );
        empty_global.set_constant(true);
        let empty_ptr = self.ir.builder
            .build_pointer_cast(empty_global.as_pointer_value(), i8_ptr, "emptyptr")
            .unwrap();

        // Build the empty str struct as the initial accumulator.
        let init_alloca = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "fmtinit");
        let init_len_ptr = build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), init_alloca, 0, "il");
        let init_dat_ptr = build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), init_alloca, 1, "id");
        build_wrappers::w_store(&self.ir.builder, init_len_ptr, i64_ty.const_int(0, false).into());
        build_wrappers::w_store(&self.ir.builder, init_dat_ptr, empty_ptr.into());
        let mut acc: BasicValueEnum<'ctx> = self.ir.builder
            .build_load(str_ty, init_alloca, "fmtacc0")
            .unwrap();

        let concat_fn = self.functions.get("axon_concat").copied()?;

        for part in parts {
            let part_val: BasicValueEnum<'ctx> = match part {
                ast::FmtPart::Lit(s) => {
                    // Emit the literal as a str value.
                    let bytes = s.as_bytes();
                    let lit_len = i64_ty.const_int(bytes.len() as u64, false);
                    let arr_ty = self.ir.context.i8_type().array_type(bytes.len() as u32 + 1);
                    let lit_name = format!("fmtlit_{fmtstr_id}_{}", self.fmtstr_counter);
                    self.fmtstr_counter += 1;
                    let g = self.ir.module.add_global(arr_ty, None, &lit_name);
                    let byte_vals: Vec<_> = bytes
                        .iter()
                        .chain(std::iter::once(&0u8))
                        .map(|&b| self.ir.context.i8_type().const_int(b as u64, false))
                        .collect();
                    g.set_initializer(&self.ir.context.i8_type().const_array(&byte_vals));
                    g.set_constant(true);
                    let lit_ptr = self.ir.builder
                        .build_pointer_cast(g.as_pointer_value(), i8_ptr, "litptr")
                        .unwrap();
                    let lit_alloca = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "litstr");
                    let lp = build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), lit_alloca, 0, "lp");
                    let dp = build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), lit_alloca, 1, "dp");
                    build_wrappers::w_store(&self.ir.builder, lp, lit_len.into());
                    build_wrappers::w_store(&self.ir.builder, dp, lit_ptr.into());
                    build_wrappers::w_load(&self.ir.builder, str_ty.into(), lit_alloca, "litval")
                }
                ast::FmtPart::Expr(e) => {
                    let v = self.emit_expr(e, fn_val)?;
                    // Auto-coerce non-str values to str.
                    match v {
                        BasicValueEnum::StructValue(_) => v, // already str
                        BasicValueEnum::IntValue(iv) => {
                            if iv.get_type().get_bit_width() == 1 {
                                // bool → to_str_bool
                                if let Some(f) = self.functions.get("to_str_bool").copied() {
                                    build_wrappers::w_call(&self.ir.builder, f, &[iv.into()], "fmtb")
                                        .try_as_basic_value().left()?
                                } else { v }
                            } else {
                                // i64 → to_str
                                if let Some(f) = self.functions.get("to_str").copied() {
                                    build_wrappers::w_call(&self.ir.builder, f, &[iv.into()], "fmti")
                                        .try_as_basic_value().left()?
                                } else { v }
                            }
                        }
                        BasicValueEnum::FloatValue(fv) => {
                            // f64 → to_str_f64
                            if let Some(f) = self.functions.get("to_str_f64").copied() {
                                build_wrappers::w_call(&self.ir.builder, f, &[fv.into()], "fmtf")
                                    .try_as_basic_value().left()?
                            } else { v }
                        }
                        _ => v,
                    }
                }
            };
            // acc = axon_concat(acc, part_val)
            let res = build_wrappers::w_call(
                &self.ir.builder, concat_fn,
                &[acc.into(), part_val.into()],
                "fmtcat",
            );
            acc = res.try_as_basic_value().left()?;
        }

        Some(acc)
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_for_in(&mut self, var: &str, start: &ast::Expr, end: &ast::Expr, body: &[ast::Stmt], inclusive: bool, fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();

        // Evaluate start and end once before the loop.
        let start_val = match self.emit_expr(start, fn_val) {
            Some(BasicValueEnum::IntValue(i)) => i,
            _ => i64_ty.const_zero(),
        };
        let end_val = match self.emit_expr(end, fn_val) {
            Some(BasicValueEnum::IntValue(i)) => i,
            _ => i64_ty.const_zero(),
        };

        // Allocate induction variable on the stack.
        let var_ptr = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), var);
        build_wrappers::w_store(&self.ir.builder, var_ptr, start_val.into());
        // Register the variable so body statements can read it.
        self.locals.insert(var.to_string(), (var_ptr, i64_ty.into()));

        let cond_bb = self.ir.context.append_basic_block(fn_val, "for.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "for.body");
        let incr_bb = self.ir.context.append_basic_block(fn_val, "for.incr");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "for.exit");

        self.loop_stack.push((incr_bb, exit_bb));

        // Jump to condition.
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // Condition: i < end  (exclusive)  or  i <= end  (inclusive)
        self.ir.builder.position_at_end(cond_bb);
        let cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), var_ptr, "for.i").into_int_value();
        let pred = if inclusive { inkwell::IntPredicate::SLE } else { inkwell::IntPredicate::SLT };
        let cmp = build_wrappers::w_int_compare(
            &self.ir.builder, pred, cur, end_val, "for.cmp");
        build_wrappers::w_cond_br(&self.ir.builder, cmp, body_bb, exit_bb);

        // Body.
        self.ir.builder.position_at_end(body_bb);
        for stmt in body {
            self.emit_expr(&stmt.expr, fn_val);
            if self.ir.builder.get_insert_block().unwrap().get_terminator().is_some() {
                break;
            }
        }
        if self.ir.builder.get_insert_block().unwrap().get_terminator().is_none() {
            build_wrappers::w_br(&self.ir.builder, incr_bb);
        }

        // Increment: i = i + 1
        self.ir.builder.position_at_end(incr_bb);
        let cur2 = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), var_ptr, "for.i2").into_int_value();
        let next = build_wrappers::w_int_add(&self.ir.builder, cur2, i64_ty.const_int(1, false), "for.next");
        build_wrappers::w_store(&self.ir.builder, var_ptr, next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.loop_stack.pop();
        self.locals.remove(var);

        self.ir.builder.position_at_end(exit_bb);
        Some(i64_ty.const_zero().into())
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_while_let(&mut self, pattern: &ast::Pattern, expr: &ast::Expr, body: &[ast::Stmt], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let cond_bb = self.ir.context.append_basic_block(fn_val, "wl.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "wl.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "wl.exit");

        self.loop_stack.push((cond_bb, exit_bb));
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // Evaluate the scrutinee and test the pattern.
        self.ir.builder.position_at_end(cond_bb);
        let subject = match self.emit_expr(expr, fn_val) {
            Some(v) => v,
            None => {
                // Expression produced no value; treat as infinite loop.
                build_wrappers::w_br(&self.ir.builder, body_bb);
                self.loop_stack.pop();
                self.ir.builder.position_at_end(exit_bb);
                return Some(self.ir.context.i64_type().const_zero().into());
            }
        };
        let matches = self.emit_pattern_test(pattern, subject);
        let cond_int = match matches {
            BasicValueEnum::IntValue(i) => i,
            _ => self.ir.context.bool_type().const_int(1, false),
        };
        build_wrappers::w_cond_br(&self.ir.builder, cond_int, body_bb, exit_bb);

        // Bind pattern variables and emit body.
        self.ir.builder.position_at_end(body_bb);
        self.emit_pattern_bindings(pattern, subject);
        for stmt in body {
            self.emit_expr(&stmt.expr, fn_val);
            if self.ir.builder.get_insert_block().unwrap().get_terminator().is_some() {
                break;
            }
        }
        if self.ir.builder.get_insert_block().unwrap().get_terminator().is_none() {
            build_wrappers::w_br(&self.ir.builder, cond_bb);
        }

        self.loop_stack.pop();
        self.ir.builder.position_at_end(exit_bb);
        Some(self.ir.context.i64_type().const_zero().into())
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_while(&mut self, cond: &ast::Expr, body: &[ast::Stmt], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let cond_bb = self.ir.context.append_basic_block(fn_val, "while.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "while.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "while.exit");

        // Push loop context so break/continue can find their targets.
        self.loop_stack.push((cond_bb, exit_bb));

        // Jump to condition check.
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // Emit condition.
        self.ir.builder.position_at_end(cond_bb);
        let cond_val = match self.emit_expr(cond, fn_val) {
            Some(BasicValueEnum::IntValue(i)) => i,
            _ => {
                // If condition didn't produce a value, treat as infinite loop.
                build_wrappers::w_br(&self.ir.builder, body_bb);
                self.loop_stack.pop();
                self.ir.builder.position_at_end(exit_bb);
                return Some(self.ir.context.i64_type().const_zero().into());
            }
        };
        build_wrappers::w_cond_br(&self.ir.builder, cond_val, body_bb, exit_bb);

        // Emit body.
        self.ir.builder.position_at_end(body_bb);
        for stmt in body {
            self.emit_expr(&stmt.expr, fn_val);
            // Stop emitting if a terminator was added (e.g., return, break, continue).
            if self.ir.builder.get_insert_block().unwrap().get_terminator().is_some() {
                break;
            }
        }
        // Jump back to condition if not already terminated.
        if self.ir.builder.get_insert_block().unwrap().get_terminator().is_none() {
            build_wrappers::w_br(&self.ir.builder, cond_bb);
        }

        // Pop loop context after body is fully emitted.
        self.loop_stack.pop();

        // Continue after loop.
        self.ir.builder.position_at_end(exit_bb);
        Some(self.ir.context.i64_type().const_zero().into())
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_select_expr(&mut self, arms: &[ast::SelectArm], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let i64_ty = self.ir.context.i64_type();
        let n = arms.len() as u64;

        // Allocate an array of i8* on the stack: [n x i8*]
        let arr_ty = i8_ptr.array_type(n as u32);
        let chans_alloca = build_wrappers::w_alloca(&self.ir.builder, arr_ty.into(), "select_chans");

        // Fill each slot with the channel pointer from each arm.
        // arm.recv is typically `ch.recv()` — extract the channel (receiver).
        for (i, arm) in arms.iter().enumerate() {
            let chan_expr = match &arm.recv {
                // `ch.recv()` — use the receiver `ch` as the channel
                ast::Expr::MethodCall { receiver, .. } => receiver.as_ref(),
                // `ch` — the channel expression itself
                other => other,
            };
            if let Some(chan_val) = self.emit_expr(chan_expr, fn_val) {
                // cast to i8* if needed
                let as_ptr = match chan_val {
                    BasicValueEnum::PointerValue(pv) => {
                        build_wrappers::w_pointer_cast(&self.ir.builder, pv, i8_ptr, "chan_ptr")
                    }
                    _ => continue,
                };
                let slot = unsafe {
                    build_wrappers::w_gep(
                        &self.ir.builder,
                        arr_ty.into(),
                        chans_alloca,
                        &[i64_ty.const_int(0, false).into(), i64_ty.const_int(i as u64, false).into()],
                        "chan_slot",
                    )
                };
                build_wrappers::w_store(&self.ir.builder, slot, as_ptr.into());
            }
        }

        // Cast array pointer to i8** for __axon_select.
        let chans_ptr = build_wrappers::w_pointer_cast(
            &self.ir.builder,
            chans_alloca,
            i8_ptr.ptr_type(AddressSpace::default()),
            "chans_ptr",
        );

        // Call __axon_select(chans, n) → i64 ready_idx.
        let ready_idx = if let Some(sel_fn) = self.functions.get("__axon_select").copied() {
            build_wrappers::w_call(
                &self.ir.builder,
                sel_fn,
                &[chans_ptr.into(), i64_ty.const_int(n, false).into()],
                "select_idx",
            ).try_as_basic_value().left()
        } else {
            None
        };

        let merge_bb = self.ir.context.append_basic_block(fn_val, "select.merge");
        let else_bb = self.ir.context.append_basic_block(fn_val, "select.else");

        // Build arm basic blocks.
        let arm_bbs: Vec<_> = arms.iter().enumerate()
            .map(|(i, _)| self.ir.context.append_basic_block(fn_val, &format!("select.arm{i}")))
            .collect();

        // Build switch: pass all (tag, bb) cases at once.
        if let Some(BasicValueEnum::IntValue(iv)) = ready_idx {
            let cases: Vec<_> = arm_bbs.iter().enumerate()
                .map(|(i, bb)| (i64_ty.const_int(i as u64, false), *bb))
                .collect();
            build_wrappers::w_switch(&self.ir.builder, iv, else_bb, &cases);
        } else {
            build_wrappers::w_br(&self.ir.builder, else_bb);
        }

        // Emit each arm body and jump to merge.
        for (arm, bb) in arms.iter().zip(arm_bbs.iter()) {
            self.ir.builder.position_at_end(*bb);
            self.emit_expr(&arm.body, fn_val);
            build_wrappers::w_br(&self.ir.builder, merge_bb);
        }

        // else: no arm ready — branch to merge (runtime will have blocked).
        self.ir.builder.position_at_end(else_bb);
        build_wrappers::w_br(&self.ir.builder, merge_bb);

        self.ir.builder.position_at_end(merge_bb);
        None
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_lambda(&mut self, params: &[ast::LambdaParam], body: &ast::Expr, captures: &[(String, Option<crate::types::Type>)], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let lambda_name = format!("__lambda_{}", self.lambda_counter);
        self.lambda_counter += 1;

        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let closure_ty = self.ir.context.struct_type(&[ptr_ty.into(), ptr_ty.into()], false);

        // ── Build the env struct type for captures ────────────────────
        // All captured variables are stored as i64 (Phase 4 limitation).
        let n_captures = captures.len();
        let env_field_tys: Vec<BasicTypeEnum<'ctx>> =
            (0..n_captures).map(|_| i64_ty.into()).collect();
        let env_struct_ty = self.ir.context.struct_type(&env_field_tys, false);

        // ── Declare the lambda function (env_ptr first, then params) ──
        let mut lambda_param_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
            vec![ptr_ty.into()]; // env_ptr
        for _ in params {
            lambda_param_tys.push(i64_ty.into());
        }
        let fn_ty = i64_ty.fn_type(&lambda_param_tys, false);
        let lambda_fn = self.ir.module.add_function(&lambda_name, fn_ty, None);

        // ── Emit the lambda body ──────────────────────────────────────
        let entry_bb = self.ir.context.append_basic_block(lambda_fn, "entry");
        let saved_ip = self.ir.builder.get_insert_block();
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_lambda_env = self.current_lambda_env.take();

        self.ir.builder.position_at_end(entry_bb);

        // env_ptr is param 0; explicit params start at 1.
        let env_ptr_arg = lambda_fn.get_nth_param(0).unwrap().into_pointer_value();

        // Bind captured variables directly to their env struct field pointers.
        // Using the field pointer as the "alloca" means stores inside the lambda
        // persist across calls (required for mutable closures like make_counter).
        let mut capture_idx_map: HashMap<String, u32> = HashMap::new();
        if n_captures > 0 {
            for (idx, (cap_name, _)) in captures.iter().enumerate() {
                let field_ptr = self.ir.builder
                    .build_struct_gep(env_struct_ty, env_ptr_arg, idx as u32, cap_name)
                    .unwrap();
                self.locals.insert(cap_name.clone(), (field_ptr, i64_ty.into()));
                capture_idx_map.insert(cap_name.clone(), idx as u32);
            }
        }

        // Publish the env context so nested `Ident` lookups can fall back
        // to loading captures via GEP if the resolver missed them.
        self.current_lambda_env = Some((env_ptr_arg, env_struct_ty, capture_idx_map));

        // Bind explicit parameters (offset by 1 for env_ptr).
        for (i, p) in params.iter().enumerate() {
            if let Some(arg) = lambda_fn.get_nth_param((i + 1) as u32) {
                let alloca = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), &p.name);
                build_wrappers::w_store(&self.ir.builder, alloca, arg);
                self.locals.insert(p.name.clone(), (alloca, i64_ty.into()));
            }
        }

        let body_val = self.emit_expr(body, lambda_fn);
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_none() {
            match body_val {
                Some(v) => { build_wrappers::w_ret(&self.ir.builder, v); }
                None => { build_wrappers::w_ret(&self.ir.builder, i64_ty.const_zero().into()); }
            }
        }

        // Restore caller's state.
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.current_lambda_env = saved_lambda_env;
        if let Some(b) = saved_ip { self.ir.builder.position_at_end(b); }
        self.functions.insert(lambda_name.clone(), lambda_fn);

        // ── At the creation site: build the fat pointer struct ─────────
        let fn_ptr = self.ir.builder
            .build_pointer_cast(
                lambda_fn.as_global_value().as_pointer_value(),
                ptr_ty,
                "lfp",
            )
            .unwrap();

        let env_ptr: BasicValueEnum<'ctx> = if n_captures > 0 {
            // Malloc an env struct and populate it.
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ty = ptr_ty.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ty, None)
            });
            let env_size = i64_ty.const_int(
                (n_captures * 8) as u64, // 8 bytes per i64
                false,
            );
            let raw = self.ir.builder
                .build_call(malloc_fn, &[env_size.into()], "env_alloc")
                .unwrap()
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_pointer_value();

            // Cast to env_struct_ty pointer for GEP.
            for (idx, (cap_name, _)) in captures.iter().enumerate() {
                // Load current value of the captured variable from caller scope
                // (self.locals has been restored to the caller's locals at this point).
                let cap_val = if let Some(&(alloca, ty)) = self.locals.get(cap_name.as_str()) {
                    build_wrappers::w_load(&self.ir.builder, ty.into(), alloca, cap_name)
                } else {
                    i64_ty.const_zero().into()
                };
                let field_ptr = self.ir.builder
                    .build_struct_gep(env_struct_ty, raw, idx as u32, &format!("env_f{idx}"))
                    .unwrap();
                build_wrappers::w_store(&self.ir.builder, field_ptr, cap_val);
            }
            // Cast back to i8* for the fat pointer.
            self.ir.builder
                .build_pointer_cast(raw, ptr_ty, "env_i8")
                .unwrap()
                .into()
        } else {
            ptr_ty.const_null().into()
        };

        // Build { fn_ptr, env_ptr } fat pointer struct.
        let mut fat = closure_ty.get_undef();
        fat = build_wrappers::w_insert_value(&self.ir.builder, fat, fn_ptr.into(), 0, "fat0").into_struct_value();
        fat = build_wrappers::w_insert_value(&self.ir.builder, fat, env_ptr, 1, "fat1").into_struct_value();
        Some(fat.into())
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_comptime_expr(&mut self, inner: &ast::Expr) -> Option<BasicValueEnum<'ctx>> {
        let evaluator = crate::comptime::Evaluator {
            env: self.comptime_env.clone(),
            fns: &self.fndefs,
        };
        match evaluator.eval(inner) {
            Ok(crate::comptime::ComptimeVal::Int(n)) => {
                Some(self.ir.context.i64_type().const_int(n as u64, true).into())
            }
            Ok(crate::comptime::ComptimeVal::Bool(b)) => {
                Some(self.ir.context.bool_type().const_int(b as u64, false).into())
            }
            Ok(crate::comptime::ComptimeVal::Float(f)) => {
                Some(self.ir.context.f64_type().const_float(f).into())
            }
            Ok(crate::comptime::ComptimeVal::Str(s)) => {
                // Emit as a { i64 len, i8* ptr } struct matching Axon's Str layout.
                let len = s.len() as u64;
                let global = build_wrappers::w_global_string_ptr(&self.ir.builder, &s, "comptime_str");
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let mut sv = str_ty.get_undef();
                sv = build_wrappers::w_insert_value(&self.ir.builder, sv, i64_ty.const_int(len, false).into(), 0, "str_len").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder, sv, global.into(), 1, "str_ptr").into_struct_value();
                Some(sv.into())
            }
            Err(e) => {
                eprintln!("comptime evaluation error: {e}");
                None
            }
        }
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_spawn(&mut self, inner: &ast::Expr, fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        // inner must be a lambda expression. Compile it to get the fat ptr.
        let fat = self.emit_expr(inner, fn_val)?;
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());

        // If we got a struct back, extract fn_ptr and env_ptr.
        let (fn_ptr_val, env_ptr_val) = match fat {
            BasicValueEnum::StructValue(sv) => {
                let fp = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "spawn_fp");
                let ep = build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "spawn_ep");
                (fp, ep)
            }
            other => {
                // Bare function pointer — wrap with null env.
                let null_env = ptr_ty.const_null();
                (other, null_env.into())
            }
        };

        if let Some(spawn_fn) = self.functions.get("__axon_spawn").copied() {
            build_wrappers::w_call(
                &self.ir.builder,
                spawn_fn,
                &[fn_ptr_val.into(), env_ptr_val.into()],
                "spawn",
            );
        }
        // spawn returns unit
        None
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_index(&mut self, receiver: &ast::Expr, index: &ast::Expr, fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let elem_llvm_ty = if let ast::Expr::Ident(n) = receiver {
            self.local_types.get(n.as_str()).and_then(|ty| {
                if let Type::Slice(inner) = ty { self.llvm_type(inner) } else { None }
            })
        } else {
            None
        };

        let slice_val = self.emit_expr(receiver, fn_val)?;
        let idx_val = self.emit_expr(index, fn_val)?;
        let elem_ty = elem_llvm_ty?;
        let idx_int = match idx_val {
            BasicValueEnum::IntValue(i) => i,
            _ => return None,
        };

        // Slice struct { i64, ptr }: extract the data pointer (field 1).
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(
            &[i64_ty.into(), ptr_ty.into()], false,
        );
        let slice_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "slicetmp");
        build_wrappers::w_store(&self.ir.builder, slice_alloca, slice_val);
        let data_field_ptr = self.ir.builder
            .build_struct_gep(slice_ty, slice_alloca, 1, "dataptr")
            .unwrap();
        let data_ptr = self.ir.builder
            .build_load(ptr_ty, data_field_ptr, "dataval")
            .unwrap()
            .into_pointer_value();
        let elem_ptr = unsafe {
            self.ir.builder
                .build_gep(elem_ty, data_ptr, &[idx_int], "elemptr")
                .unwrap()
        };
        let elem = build_wrappers::w_load(&self.ir.builder, elem_ty.into(), elem_ptr, "elemval");
        Some(elem)
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_field_access(&mut self, receiver: &ast::Expr, field: &str, fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        // Layer-1 ASI: handle Uncertain<T> / Temporal<T> field access via
        // a direct GEP on the inferred struct shape (no named LLVM struct).
        let recv_ty = self.sem_type_of_expr(receiver);
        if let Some(Type::Uncertain(_)) | Some(Type::Temporal(_)) = recv_ty.clone() {
            let ty = recv_ty.unwrap();
            let idx_opt: Option<u32> = match (&ty, field) {
                (Type::Uncertain(_), "value") => Some(0),
                (Type::Uncertain(_), "confidence") => Some(1),
                (Type::Uncertain(_), "source_tag") => Some(2),
                (Type::Temporal(_), "value") => Some(0),
                (Type::Temporal(_), "confidence") => Some(1),
                (Type::Temporal(_), "horizon_ms") => Some(2),
                (Type::Temporal(_), "decay") => Some(3),
                (Type::Temporal(_), "valid_until_ms") => Some(4),
                _ => None,
            };
            if let (Some(idx), Some(struct_be)) = (idx_opt, self.llvm_type(&ty)) {
                if let BasicTypeEnum::StructType(struct_ty) = struct_be {
                    let recv_val = self.emit_expr(receiver, fn_val)?;
                    let recv_alloca = self.ir.builder
                        .build_alloca(struct_ty, "asi_recv_tmp")
                        .unwrap();
                    build_wrappers::w_store(&self.ir.builder, recv_alloca, recv_val);
                    let fptr = self.ir.builder
                        .build_struct_gep(struct_ty, recv_alloca, idx, field)
                        .unwrap();
                    if let Some(fty) = struct_ty.get_field_type_at_index(idx) {
                        let fval = build_wrappers::w_load(&self.ir.builder, fty.into(), fptr, field);
                        return Some(fval);
                    }
                }
            }
        }

        // Determine the struct name from the receiver's semantic type.
        // Handle both bare Ident receivers and chained FieldAccess receivers.
        let struct_name = self.sem_type_of_expr(receiver).and_then(|ty| {
            if let Type::Struct(sn) = ty { Some(sn) } else { None }
        });

        if let Some(sname) = struct_name {
            if let (Some(struct_ty), Some(field_names)) = (
                self.ir.module.get_struct_type(&sname),
                self.struct_fields.get(&sname).cloned(),
            ) {
                if let Some(idx) = field_names.iter().position(|n| n == field) {
                    let recv_val = self.emit_expr(receiver, fn_val)?;
                    let recv_alloca = self.ir.builder
                        .build_alloca(struct_ty, "recv_tmp")
                        .unwrap();
                    build_wrappers::w_store(&self.ir.builder, recv_alloca, recv_val);
                    let fptr = self.ir.builder
                        .build_struct_gep(struct_ty, recv_alloca, idx as u32, field)
                        .unwrap();
                    if let Some(field_ty) = struct_ty.get_field_type_at_index(idx as u32) {
                        let fval = build_wrappers::w_load(&self.ir.builder, field_ty.into(), fptr, field);
                        return Some(fval);
                    }
                }
            }
        }
        // Fallback: emit receiver for side-effects only.
        let _ = self.emit_expr(receiver, fn_val);
        None
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_struct_lit(&mut self, name: &str, fields: &[(String, ast::Expr)], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        if name.contains("::") {
            // Enum variant construction: "EnumName::VariantName"
            let mut parts = name.splitn(2, "::");
            let enum_name = parts.next().unwrap().to_string();
            let variant_name = parts.next().unwrap().to_string();

            // Look up variant info.
            let variants = self.enum_variants.get(&enum_name).cloned()?;
            let (tag_int, field_types) = variants
                .iter()
                .find(|(vn, _, _)| vn == &variant_name)
                .map(|(_, tag, fts)| (*tag, fts.clone()))?;

            // Look up the LLVM struct type for the enum.
            let struct_name = format!("{enum_name}_enum");
            let enum_struct_ty = self.ir.module.get_struct_type(&struct_name)?;

            // Alloca for the enum struct { i32, [N x i8] }.
            let alloca = build_wrappers::w_alloca(&self.ir.builder, enum_struct_ty.into(), &struct_name);

            // Store tag (field 0).
            let i32_ty = self.ir.context.i32_type();
            let tag_ptr = self.ir.builder
                .build_struct_gep(enum_struct_ty, alloca, 0, "tagptr")
                .unwrap();
            self.ir.builder
                .build_store(tag_ptr, i32_ty.const_int(tag_int as u64, false))
                .unwrap();

            // Store each field into the payload (field 1) at byte offsets.
            if !fields.is_empty() {
                let i8_ty = self.ir.context.i8_type();
                let ptr_ty = i8_ty.ptr_type(AddressSpace::default());

                // Get pointer to payload field.
                let pay_ptr = self.ir.builder
                    .build_struct_gep(enum_struct_ty, alloca, 1, "payptr")
                    .unwrap();
                let pay_i8ptr = self.ir.builder
                    .build_pointer_cast(pay_ptr, ptr_ty, "payi8ptr")
                    .unwrap();

                let mut byte_offset: u64 = 0;
                for (fi, (fname, fexpr)) in fields.iter().enumerate() {
                    if let Some(fval) = self.emit_expr(fexpr, fn_val) {
                        let fty = field_types.get(fi).cloned().unwrap_or(Type::Unknown);
                        let fsize = self.llvm_sizeof(&fty).unwrap_or(8);
                        // GEP into the payload at the current byte offset.
                        let offset_val = i32_ty.const_int(byte_offset, false);
                        let field_ptr = unsafe {
                            self.ir.builder
                                .build_gep(i8_ty, pay_i8ptr, &[offset_val], fname)
                                .unwrap()
                        };
                        // Cast to the appropriate typed pointer and store.
                        let fval_ptr_ty = fval.get_type().ptr_type(AddressSpace::default());
                        let typed_ptr = self.ir.builder
                            .build_pointer_cast(field_ptr, fval_ptr_ty, "ftyptr")
                            .unwrap();
                        build_wrappers::w_store(&self.ir.builder, typed_ptr, fval);
                        byte_offset += fsize;
                    }
                }
            }

            let val = build_wrappers::w_load(&self.ir.builder, enum_struct_ty.into(), alloca, name);
            Some(val)
        } else {
            // Regular struct literal.
            let struct_ty = self.ir.module.get_struct_type(name)?;
            let field_names = self.struct_fields.get(name).cloned().unwrap_or_default();
            let alloca = build_wrappers::w_alloca(&self.ir.builder, struct_ty.into(), name);
            for (fname, fexpr) in fields {
                let idx = field_names.iter().position(|n| n == fname).unwrap_or(0) as u32;
                if let Some(fval) = self.emit_expr(fexpr, fn_val) {
                    let fptr = self.ir.builder
                        .build_struct_gep(struct_ty, alloca, idx, fname)
                        .unwrap();
                    build_wrappers::w_store(&self.ir.builder, fptr, fval);
                }
            }
            let val = build_wrappers::w_load(&self.ir.builder, struct_ty.into(), alloca, name);
            Some(val)
        }
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_array_lit(&mut self, elems: &[ast::Expr], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        if elems.is_empty() {
            // Return a zero-length slice struct.
            let i64_ty = self.ir.context.i64_type();
            let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
            let slice_ty = self.ir.context.struct_type(
                &[i64_ty.into(), ptr_ty.into()],
                false,
            );
            let zero = i64_ty.const_zero();
            let null = ptr_ty.const_null();
            let agg = slice_ty.const_named_struct(&[zero.into(), null.into()]);
            return Some(agg.into());
        }

        // Emit each element.
        let mut vals: Vec<BasicValueEnum<'ctx>> = Vec::with_capacity(elems.len());
        for e in elems {
            if let Some(v) = self.emit_expr(e, fn_val) {
                vals.push(v);
            }
        }
        if vals.is_empty() {
            return None;
        }

        let elem_ty = vals[0].get_type();
        let n = vals.len() as u32;

        // Use malloc for the array backing store so the slice remains
        // valid if returned from a function (no dangling stack pointer).
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        // Compute element size from the LLVM type bit-width.
        let elem_size_bytes: u64 = match elem_ty {
            BasicTypeEnum::IntType(it) => (it.get_bit_width() as u64 + 7) / 8,
            BasicTypeEnum::FloatType(ft) => {
                if ft == self.ir.context.f32_type() { 4 } else { 8 }
            }
            BasicTypeEnum::StructType(_) | BasicTypeEnum::ArrayType(_)
            | BasicTypeEnum::PointerType(_) | BasicTypeEnum::VectorType(_) => 8,
        };
        let total_bytes = i64_ty.const_int(elem_size_bytes * n as u64, false);
        let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
            let malloc_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
            self.ir.module.add_function("malloc", malloc_ty, None)
        });
        let malloc_call = self.ir.builder
            .build_call(malloc_fn, &[total_bytes.into()], "arrdata")
            .unwrap();
        let raw_ptr = malloc_call.try_as_basic_value().left().unwrap().into_pointer_value();
        // Cast to typed element pointer for GEP.
        let elem_ptr_ty = elem_ty.ptr_type(AddressSpace::default());
        let elem_data_ptr = self.ir.builder
            .build_pointer_cast(raw_ptr, elem_ptr_ty, "arrelemptr")
            .unwrap();
        for (idx, v) in vals.iter().enumerate() {
            let idx_val = i64_ty.const_int(idx as u64, false);
            let gep = unsafe {
                self.ir.builder
                    .build_gep(elem_ty, elem_data_ptr, &[idx_val], "arrelem")
                    .unwrap()
            };
            build_wrappers::w_store(&self.ir.builder, gep, *v);
        }

        // Build slice struct { len, ptr }.
        let slice_ty = self.ir.context.struct_type(
            &[i64_ty.into(), ptr_ty.into()],
            false,
        );
        let len_val = i64_ty.const_int(n as u64, false);
        // Cast malloc ptr to opaque i8* for the slice data field.
        let data_ptr = self.ir
            .builder
            .build_pointer_cast(raw_ptr, ptr_ty, "sliceptr")
            .unwrap();
        let slice_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "slice");
        // Store len.
        let len_ptr = self.ir
            .builder
            .build_struct_gep(slice_ty, slice_alloca, 0, "lenptr")
            .unwrap();
        build_wrappers::w_store(&self.ir.builder, len_ptr, len_val.into());
        // Store data ptr.
        let data_field_ptr = self.ir
            .builder
            .build_struct_gep(slice_ty, slice_alloca, 1, "dataptr")
            .unwrap();
        build_wrappers::w_store(&self.ir.builder, data_field_ptr, data_ptr.into());
        let slice_val = self.ir
            .builder
            .build_load(slice_ty, slice_alloca, "sliceval")
            .unwrap();
        Some(slice_val)
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_return(&mut self, maybe_val: &Option<Box<ast::Expr>>, fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        match maybe_val {
            std::option::Option::Some(e) => {
                if let Some(v) = self.emit_expr(e, fn_val) {
                    self.log_return_if_adaptive_val(v);
                    self.emit_verify_check_if_needed(v, fn_val);
                    build_wrappers::w_ret(&self.ir.builder, v);
                } else {
                    self.log_return_if_adaptive();
                    build_wrappers::w_ret_void(&self.ir.builder);
                }
            }
            std::option::Option::None => {
                self.log_return_if_adaptive();
                build_wrappers::w_ret_void(&self.ir.builder);
            }
        }
        None
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_method_call(&mut self, receiver: &ast::Expr, method: &str, args: &[ast::Expr], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        // --- DynTrait: vtable dispatch ---
        let recv_sem_ty = self.infer_expr_sem_type(receiver);
        if let Some(Type::DynTrait(trait_name)) = recv_sem_ty {
            let recv_val = self.emit_expr(receiver, fn_val)?;
            let fat = recv_val.into_struct_value();
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder, fat, 0, "data_ptr")
                .into_pointer_value();
            let vtbl_ptr = build_wrappers::w_extract_value(&self.ir.builder, fat, 1, "vtbl_ptr")
                .into_pointer_value();

            // Find method index in the trait definition.
            let trait_def = self.trait_defs.get(&trait_name).cloned()?;
            let method_idx = trait_def.methods.iter().position(|m| m.name == *method)?;

            // GEP into vtable array to get the function pointer slot.
            let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
            let arr_ty = i8_ptr.array_type(trait_def.methods.len() as u32);
            let idx_zero = self.ir.context.i64_type().const_zero();
            let idx_m = self.ir.context.i64_type().const_int(method_idx as u64, false);
            let fn_slot = unsafe {
                build_wrappers::w_gep(&self.ir.builder, arr_ty.into(), vtbl_ptr, &[idx_zero, idx_m], "fn_slot")
            };
            let fn_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), fn_slot, "fn_ptr")
                .into_pointer_value();

            // Build call args: data_ptr + any extra args.
            let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![data_ptr.into()];
            for a in args {
                if let Some(v) = self.emit_expr(a, fn_val) {
                    call_args.push(v.into());
                }
            }

            let thunk_ty = self.vtable_thunk_types.get(&(trait_name, method.to_string())).copied()?;
            let call = build_wrappers::w_indirect_call(&self.ir.builder, thunk_ty, fn_ptr, &call_args, "vtbl_call");
            return call.try_as_basic_value().left();
        }

        // --- Static dispatch (struct/enum method) ---
        // Determine the receiver's struct/enum type for name mangling.
        let type_name = self.infer_expr_sem_type(receiver).and_then(|t| match t {
            Type::Struct(n) | Type::Enum(n) => Some(n),
            _ => None,
        });

        let recv_val = self.emit_expr(receiver, fn_val);

        // Try mangled name first, fall back to bare method name.
        let mangled = type_name.as_deref().map(|tn| format!("{tn}__{method}"));
        let fn_v = mangled
            .as_deref()
            .and_then(|m| self.functions.get(m).copied())
            .or_else(|| self.functions.get(method).copied());

        if let Some(fn_v) = fn_v {
            let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
            // Prepend the receiver as the first argument.
            // For Chan methods, cast receiver to i8* (opaque pointer ABI).
            if let Some(rv) = recv_val {
                let is_chan_method = matches!(method, "send" | "recv" | "clone");
                let rv = if is_chan_method {
                    if let BasicValueEnum::PointerValue(pv) = rv {
                        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                        build_wrappers::w_pointer_cast(&self.ir.builder, pv, i8_ptr, "chan_cast").into()
                    } else {
                        rv
                    }
                } else {
                    rv
                };
                arg_vals.push(rv.into());
            }
            for a in args {
                if let Some(v) = self.emit_expr(a, fn_val) {
                    arg_vals.push(v.into());
                }
            }
            let call = self.ir
                .builder
                .build_call(fn_v, &arg_vals, "mcall")
                .unwrap();
            return call.try_as_basic_value().left();
        }
        None
    }

    /// Auto-extracted from `emit_expr` (Phase 3 decomposition).
    pub(super) fn emit_call(&mut self, callee: &ast::Expr, args: &[ast::Expr], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let i64_ty = self.ir.context.i64_type();

        // Try to resolve callee as a global function first.
        let maybe_fn_v = match callee {
            ast::Expr::Ident(name) => self.functions.get(name.as_str()).copied(),
            // Chan::new / chan<T>() — StructLit callees with known names.
            ast::Expr::StructLit { name, fields } if fields.is_empty() => {
                // chan::<T> → alias to Chan::new
                if name.starts_with("chan::<") {
                    self.functions.get("Chan::new").copied()
                } else if let Some(inner) =
                    name.strip_prefix("ai_extract::<").and_then(|s| s.strip_suffix(">"))
                {
                    // ai_extract::<T> → dispatch to the per-T helper.
                    // v1 T set: i64, f64, bool, Uncertain<i64>, Uncertain<f64>.
                    // The concrete LLVM functions are emitted in
                    // declare_builtins under the same names.
                    let helper = match inner.trim() {
                        "i64"            => "ai_extract_i64",
                        "f64"            => "ai_extract_f64",
                        "bool"           => "ai_extract_bool",
                        "Uncertain<i64>" => "ai_extract_uncertain_i64",
                        "Uncertain<f64>" => "ai_extract_uncertain_f64",
                        _ => "",
                    };
                    self.functions.get(helper).copied()
                } else {
                    self.functions.get(name.as_str()).copied()
                }
            }
            _ => None,
        };

        // Try closure call: callee is a local holding a {fn_ptr, env_ptr} struct.
        if maybe_fn_v.is_none() {
            if let ast::Expr::Ident(name) = callee {
                if let Some(&(alloca, ty)) = self.locals.get(name.as_str()) {
                    let fat = build_wrappers::w_load(&self.ir.builder, ty.into(), alloca, "closure");
                    if let BasicValueEnum::StructValue(sv) = fat {
                        let fp = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "cfp");
                        let ep = build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "cep");
                        // Build arg list: env_ptr first, then explicit args.
                        let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> =
                            vec![ep.into()];
                        for a in args {
                            if let Some(v) = self.emit_expr(a, fn_val) {
                                call_args.push(v.into());
                            }
                        }
                        // Build an indirect call via fn pointer.
                        let fn_ptr = self.ir.builder
                            .build_pointer_cast(
                                fp.into_pointer_value(),
                                ptr_ty,
                                "fp_cast",
                            )
                            .unwrap();
                        // Build the function type for the indirect call.
                        let mut ipt: Vec<BasicMetadataTypeEnum<'ctx>> =
                            vec![ptr_ty.into()];
                        for _ in args {
                            ipt.push(i64_ty.into());
                        }
                        let indirect_ty = i64_ty.fn_type(&ipt, false);
                        let call = self.ir.builder
                            .build_indirect_call(indirect_ty, fn_ptr, &call_args, "icall")
                            .unwrap();
                        return call.try_as_basic_value().left();
                    }
                }
            }
        }

        // Resolve the callee to an LLVM FunctionValue (direct call).
        let fn_v = maybe_fn_v?;

        // Get declared parameter types to coerce mismatched integer widths.
        let param_tys: Vec<BasicTypeEnum<'ctx>> = fn_v.get_type().get_param_types();

        // Get Axon-level param types for DynTrait coercion.
        let axon_params: Vec<ast::AxonType> = if let ast::Expr::Ident(name) = callee {
            self.fn_axon_params.get(name).cloned().unwrap_or_default()
        } else {
            Vec::new()
        };

        let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
        for (i, a) in args.iter().enumerate() {
            // Check for DynTrait coercion: concrete type → fat pointer.
            let axon_param_ty = axon_params.get(i);
            let is_dyn_param = matches!(axon_param_ty, Some(ast::AxonType::DynTrait(_)));

            if is_dyn_param {
                let trait_name = match axon_param_ty {
                    Some(ast::AxonType::DynTrait(t)) => t.clone(),
                    _ => unreachable!(),
                };
                // Get the concrete argument's type to find the right vtable.
                let arg_sem_ty = self.infer_expr_sem_type(a);
                let type_name = match &arg_sem_ty {
                    Some(Type::Struct(n)) | Some(Type::Enum(n)) => Some(n.clone()),
                    _ => None,
                };

                if let Some(type_name) = type_name {
                    let vtable_key = (trait_name.clone(), type_name.clone());
                    if let Some(vtable_global) = self.vtable_globals.get(&vtable_key).copied() {
                        let concrete_val = self.emit_expr(a, fn_val);
                        if let Some(val) = concrete_val {
                            // Alloca the concrete value; store it so we have a data ptr.
                            let concrete_llvm_ty = val.get_type();
                            let data_alloca = build_wrappers::w_alloca(&self.ir.builder, concrete_llvm_ty.into(), "dyn_data");
                            build_wrappers::w_store(&self.ir.builder, data_alloca, val);

                            // Build fat pointer { data_ptr, vtable_ptr }.
                            let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                            let fat_ty = self.ir.context.struct_type(&[i8_ptr.into(), i8_ptr.into()], false);
                            let fat_undef = fat_ty.get_undef();

                            let data_cast = build_wrappers::w_pointer_cast(&self.ir.builder, data_alloca, i8_ptr, "data_cast");
                            let vtbl_ptr = vtable_global.as_pointer_value();
                            let vtbl_cast = build_wrappers::w_pointer_cast(&self.ir.builder, vtbl_ptr, i8_ptr, "vtbl_cast");

                            let fat0 = build_wrappers::w_insert_value(&self.ir.builder, fat_undef, data_cast.into(), 0, "fat0");
                            let fat1 = build_wrappers::w_insert_value(&self.ir.builder, fat0.into_struct_value(), vtbl_cast.into(), 1, "fat1");
                            // AggregateValueEnum → StructValue → BasicMetadataValueEnum
                            arg_vals.push(BasicValueEnum::StructValue(fat1.into_struct_value()).into());
                            continue;
                        }
                    }
                }
                // Fallthrough: emit arg as-is if coercion wasn't possible.
                if let Some(v) = self.emit_expr(a, fn_val) {
                    arg_vals.push(v.into());
                }
                continue;
            }

            let val = match self.emit_expr(a, fn_val) {
                Some(v) => v,
                None => continue,
            };
            // Coerce argument types to match declared parameter types.
            let expected_ty = param_tys.get(i).copied();
            let coerced = match (expected_ty, val) {
                // int width mismatch: truncate or extend (zext for unsigned, sext for signed)
                (Some(BasicTypeEnum::IntType(exp_int)), BasicValueEnum::IntValue(iv)) => {
                    let actual = iv.get_type().get_bit_width();
                    let expect = exp_int.get_bit_width();
                    if actual > expect {
                        build_wrappers::w_int_truncate(&self.ir.builder, iv, exp_int, "trunc").into()
                    } else if actual < expect {
                        let sem_ty = self.infer_expr_sem_type(a);
                        let is_unsigned = matches!(
                            sem_ty,
                            Some(Type::U8) | Some(Type::U16) | Some(Type::U32) | Some(Type::U64)
                        );
                        if is_unsigned {
                            build_wrappers::w_int_z_extend(&self.ir.builder, iv, exp_int, "zext").into()
                        } else {
                            build_wrappers::w_int_s_extend(&self.ir.builder, iv, exp_int, "sext").into()
                        }
                    } else {
                        val
                    }
                }
                // float → int: e.g. to_str(f64_val) where to_str takes i64
                (Some(BasicTypeEnum::IntType(exp_int)), BasicValueEnum::FloatValue(fv)) => {
                    build_wrappers::w_float_to_signed_int(&self.ir.builder, fv, exp_int, "ftoi").into()
                }
                // int → float
                (Some(BasicTypeEnum::FloatType(exp_flt)), BasicValueEnum::IntValue(iv)) => {
                    build_wrappers::w_signed_int_to_float(&self.ir.builder, iv, exp_flt, "itof").into()
                }
                _ => val,
            };
            arg_vals.push(coerced.into());
        }

        let call = self.ir
            .builder
            .build_call(fn_v, &arg_vals, "call")
            .unwrap();
        call.try_as_basic_value().left()
    }


}
