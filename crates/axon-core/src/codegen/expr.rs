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

/// Reduction kind for `emit_arr_f64_loop` — a counted loop over an f64 slice.
enum ArrReduceF64 {
    /// Σ of all elements (f64 result).
    Sum,
    /// Arithmetic mean (f64): Σ / len, or 0.0 for empty (no panic).
    Mean,
    /// Max/min element (f64). `true` = max. Panics (exit 101) on empty.
    Extreme { is_max: bool },
    /// Index of the max/min element (i64 result). `true` = argmax. Panics
    /// (exit 101) on empty. First index wins ties (strict compare).
    ArgExtreme { is_max: bool },
}

/// Predicate-reduction kind for `emit_arr_i64_pred`.
enum PredReduce {
    /// Count elements where the predicate is true (i64 result).
    Count,
    /// True iff ALL elements satisfy the predicate (i1; short-circuits false).
    All,
    /// True iff ANY element satisfies the predicate (i1; short-circuits true).
    Any,
}

/// Reduction kind for `emit_arr_i64_loop` — a counted loop over an i64 slice.
enum ArrReduce<'ctx> {
    /// Σ of all elements (i64 result).
    Sum,
    /// Whether any element equals the needle (i1 result).
    Contains(inkwell::values::IntValue<'ctx>),
    /// Max / min of the elements (i64 result). `true` = max. Panics (exit 101,
    /// matching the interpreter) on an empty array.
    Extreme { is_max: bool },
    /// Arithmetic mean of the elements (f64 result): Σ / len, or 0.0 for empty
    /// (matching the interpreter, which does NOT panic on an empty mean).
    Mean,
    /// Index of the max/min element (i64 result). `true` = argmax. Panics
    /// (exit 101) on an empty array, matching the interpreter.
    ArgExtreme { is_max: bool },
}

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
                    let val = build_wrappers::w_load(&self.ir.builder, llvm_ty, ptr, name);
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
            // R3b: codegen ignores the per-call `tier:` (it's an interp-side AI
            // routing concern; native AI calls aren't in the codegen path).
            ast::Expr::Call { callee, args, .. } => self.emit_call(callee, args, fn_val),

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
            ast::Expr::Tuple(elems) => self.emit_tuple_lit(elems, fn_val),

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

            // Enum equality: `a == b` where both are `{name}_enum` = { i32 tag,
            // [N x i8] payload }. Compare the TAG (discriminant, field 0) as
            // ints — an enum StructValue would otherwise fall into the str_eq
            // arm below and fail IR verification (BUG_HUNT #41). The interpreter
            // oracle compares (enum, variant, fields); tag-compare is exact for
            // fieldless variants and for any two DIFFERENT-tag variants (the
            // example usages: `Op::Zero == Op::Zero`, `Op::Add{..} != Op::Zero`).
            // Payload-field equality for same-tag-different-fields is a follow-up.
            (BasicValueEnum::StructValue(l), BasicValueEnum::StructValue(r))
                if matches!(op, ast::BinOp::Eq | ast::BinOp::NotEq)
                    && l.get_type().get_name()
                        .and_then(|n| n.to_str().ok())
                        .map(|n| n.ends_with("_enum"))
                        .unwrap_or(false) =>
            {
                let lt = build_wrappers::w_extract_value(&self.ir.builder, l, 0, "ltag")
                    .into_int_value();
                let rt = build_wrappers::w_extract_value(&self.ir.builder, r, 0, "rtag")
                    .into_int_value();
                let pred = if matches!(op, ast::BinOp::NotEq) {
                    inkwell::IntPredicate::NE
                } else {
                    inkwell::IntPredicate::EQ
                };
                build_wrappers::w_int_compare(&self.ir.builder, pred, lt, rt, "enumeq").into()
            }

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
                        &[i64_ty.const_int(0, false), i64_ty.const_int(i as u64, false)],
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
    pub(super) fn emit_lambda(&mut self, params: &[ast::LambdaParam], body: &ast::Expr, captures: &[(String, Option<crate::types::Type>)], _fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
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
                Some(v) => {
                    // The lambda ABI declares an i64 return, but a bool-bodied
                    // lambda (`|x| x > 2`, the filter/any/all predicate shape)
                    // produces i1 — zero-extend it to i64 so the `ret` matches
                    // the function type (callers read it back as i64 and test
                    // != 0). Other narrow ints widen the same way.
                    let coerced = match v {
                        BasicValueEnum::IntValue(iv) if iv.get_type().get_bit_width() < 64 => {
                            build_wrappers::w_int_z_extend(&self.ir.builder, iv, i64_ty, "lam_ret_zext").into()
                        }
                        other => other,
                    };
                    build_wrappers::w_ret(&self.ir.builder, coerced);
                }
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
                .build_call(malloc_fn, &[self.msize(env_size, "msz").into()], "env_alloc")
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
                    build_wrappers::w_load(&self.ir.builder, ty, alloca, cap_name)
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
        let elem = build_wrappers::w_load(&self.ir.builder, elem_ty, elem_ptr, "elemval");
        Some(elem)
    }

    /// Emit a counted loop over an i64 slice `{i64 len, i8* data}` performing a
    /// reduction. Pure IR (no runtime extern), so it works on native AND wasm.
    /// Returns the reduced scalar (i64 for Sum, i1 for Contains).
    fn emit_arr_i64_loop(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        op: ArrReduce<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let i1_ty = self.ir.context.bool_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        // Unpack len (field 0) and data ptr (field 1).
        let slice_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "arr_s");
        build_wrappers::w_store(&self.ir.builder, slice_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, slice_alloca, 0, "arr_lenp").unwrap(),
            "arr_len").into_int_value();
        let data_ptr = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, slice_alloca, 1, "arr_datp").unwrap(),
            "arr_dat").into_pointer_value();
        let i64_data = build_wrappers::w_pointer_cast(
            &self.ir.builder, data_ptr,
            i64_ty.ptr_type(AddressSpace::default()), "arr_i64p");

        // Max/min and argmax/argmin panic on an empty array (interp parity,
        // exit 101). Emit the check before the loop: if len == 0, call exit(101).
        if matches!(op, ArrReduce::Extreme { .. } | ArrReduce::ArgExtreme { .. }) {
            let empty_bb = self.ir.context.append_basic_block(fn_val, "arr.empty");
            let nonempty_bb = self.ir.context.append_basic_block(fn_val, "arr.nonempty");
            let is_empty = build_wrappers::w_int_compare(
                &self.ir.builder, inkwell::IntPredicate::EQ, len, i64_ty.const_zero(), "arr_isempty");
            build_wrappers::w_cond_br(&self.ir.builder, is_empty, empty_bb, nonempty_bb);
            self.ir.builder.position_at_end(empty_bb);
            // Use the C `exit(i32)` (declared in declare_builtins), NOT the
            // Axon-source `exit` wrapper in self.functions (which is `exit_axon`
            // and takes an i64). Matches the interpreter's panic exit code 101.
            if let Some(exit_fn) = self.ir.module.get_function("exit") {
                let code = self.ir.context.i32_type().const_int(101, false);
                build_wrappers::w_call(&self.ir.builder, exit_fn, &[code.into()], "");
            }
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(nonempty_bb);
        }

        // Accumulator + index slots.
        let acc_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "arr_acc");
        // Sentinel init: 0 for Sum/Contains; i64::MIN for max / i64::MAX for min
        // so the first element always wins the running compare.
        let init: u64 = match &op {
            ArrReduce::Sum | ArrReduce::Mean | ArrReduce::Contains(_) => 0,
            ArrReduce::Extreme { is_max: true } | ArrReduce::ArgExtreme { is_max: true } => i64::MIN as u64,
            ArrReduce::Extreme { is_max: false } | ArrReduce::ArgExtreme { is_max: false } => i64::MAX as u64,
        };
        build_wrappers::w_store(&self.ir.builder, acc_slot, i64_ty.const_int(init, false).into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "arr_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        // For argmax/argmin: track the best element's INDEX (acc holds its value).
        let best_idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "arr_bidx");
        build_wrappers::w_store(&self.ir.builder, best_idx_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "arr.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "arr.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "arr.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // cond: i < len  (and, for Contains, not-yet-found short-circuits below)
        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "arr_icur").into_int_value();
        let in_range = build_wrappers::w_int_compare(
            &self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "arr_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        // body: load elem, update accumulator, i++.
        self.ir.builder.position_at_end(body_bb);
        let elem_ptr = unsafe {
            self.ir.builder.build_gep(i64_ty, i64_data, &[i_cur], "arr_ep").unwrap()
        };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), elem_ptr, "arr_e").into_int_value();
        match &op {
            ArrReduce::Sum | ArrReduce::Mean => {
                let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "arr_a").into_int_value();
                let nacc = build_wrappers::w_int_add(&self.ir.builder, acc, elem, "arr_na");
                build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
            }
            ArrReduce::Contains(needle) => {
                let eq = build_wrappers::w_int_compare(
                    &self.ir.builder, inkwell::IntPredicate::EQ, elem, *needle, "arr_eq");
                // acc = acc | (elem == needle)  (kept as i64 0/1 for a uniform slot)
                let eq64 = build_wrappers::w_int_z_extend(&self.ir.builder, eq, i64_ty, "arr_eq64");
                let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "arr_a").into_int_value();
                let nacc = build_wrappers::w_or(&self.ir.builder, acc, eq64, "arr_or");
                build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
            }
            ArrReduce::Extreme { is_max } => {
                let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "arr_a").into_int_value();
                // pick = (is_max ? elem > acc : elem < acc) ? elem : acc
                let pred = if *is_max { inkwell::IntPredicate::SGT } else { inkwell::IntPredicate::SLT };
                let better = build_wrappers::w_int_compare(&self.ir.builder, pred, elem, acc, "arr_better");
                let nacc = self.ir.builder.build_select(better, elem, acc, "arr_pick").unwrap().into_int_value();
                build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
            }
            ArrReduce::ArgExtreme { is_max } => {
                let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "arr_a").into_int_value();
                // STRICT compare (interp updates only on strictly-better), so the
                // FIRST max/min index wins ties — matching the interpreter.
                let pred = if *is_max { inkwell::IntPredicate::SGT } else { inkwell::IntPredicate::SLT };
                let better = build_wrappers::w_int_compare(&self.ir.builder, pred, elem, acc, "arr_argbetter");
                let nacc = self.ir.builder.build_select(better, elem, acc, "arr_argval").unwrap().into_int_value();
                build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
                let cur_best = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_idx_slot, "arr_cb").into_int_value();
                let nbest = self.ir.builder.build_select(better, i_cur, cur_best, "arr_argidx").unwrap().into_int_value();
                build_wrappers::w_store(&self.ir.builder, best_idx_slot, nbest.into());
            }
        }
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "arr_inext");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // exit: produce the result.
        self.ir.builder.position_at_end(exit_bb);
        let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "arr_res").into_int_value();
        match op {
            ArrReduce::Sum | ArrReduce::Extreme { .. } => Some(acc.into()),
            ArrReduce::ArgExtreme { .. } => {
                // Return the tracked best index, not the value.
                let bidx = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_idx_slot, "arr_bres").into_int_value();
                Some(bidx.into())
            }
            ArrReduce::Contains(_) => {
                // acc is 0/1 → truncate to i1 (bool).
                let b = self.ir.builder.build_int_truncate(acc, i1_ty, "arr_found").unwrap();
                Some(b.into())
            }
            ArrReduce::Mean => {
                // mean = len == 0 ? 0.0 : (f64)sum / (f64)len. Guard div-by-zero
                // (interp returns 0.0 for an empty array, never panics).
                let f64_ty = self.ir.context.f64_type();
                let is_empty = build_wrappers::w_int_compare(
                    &self.ir.builder, inkwell::IntPredicate::EQ, len, i64_ty.const_zero(), "arr_meane");
                let sum_f = build_wrappers::w_signed_int_to_float(&self.ir.builder, acc, f64_ty, "arr_sumf");
                let len_f = build_wrappers::w_signed_int_to_float(&self.ir.builder, len, f64_ty, "arr_lenf");
                let div = self.ir.builder.build_float_div(sum_f, len_f, "arr_mean").unwrap();
                let zero_f = f64_ty.const_float(0.0);
                let mean = self.ir.builder.build_select(is_empty, zero_f, div, "arr_meansel").unwrap();
                Some(mean)
            }
        }
    }

    /// Reverse an i64 slice into a freshly malloc'd buffer, returning a new
    /// `{i64 len, i8* data}` slice. Pure IR + malloc (target-size-aware via
    /// `emit_malloc`), so it works native AND wasm. dst[i] = src[len-1-i].
    fn emit_arr_i64_reverse(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        // Unpack src len + data ptr.
        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "rev_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "rev_lenp").unwrap(),
            "rev_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "rev_datp").unwrap(),
            "rev_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "rev_srci");

        // Malloc len*8 bytes for the destination (target-size-aware).
        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, len, eight, "rev_bytes");
        let dst_raw = self.emit_malloc(total, "rev_dst");
        let dst_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "rev_dsti");

        // for i in 0..len: dst[i] = src[len-1-i]
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "rev_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "rev.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "rev.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "rev.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "rev_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(
            &self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "rev_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        // src index = len - 1 - i
        let len_m1 = build_wrappers::w_int_sub(&self.ir.builder, len, i64_ty.const_int(1, false), "rev_lm1");
        let src_idx = build_wrappers::w_int_sub(&self.ir.builder, len_m1, i_cur, "rev_si");
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[src_idx], "rev_sp").unwrap() };
        let v = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "rev_v");
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[i_cur], "rev_dp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, dp, v);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "rev_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // Build the result slice { len, dst_raw(as i8*) }.
        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "rev_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "rev_olen").unwrap(),
            len.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "rev_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "rev_optr").unwrap(),
            dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "rev_res"))
    }

    /// arr_take / arr_drop on an i64 slice → a fresh slice holding a contiguous
    /// range. `n` is clamped to [0, len]; take copies src[0..n], drop copies
    /// src[n..len]. Pure IR + malloc (native AND wasm).
    fn emit_arr_i64_take_drop(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        n: inkwell::values::IntValue<'ctx>,
        is_take: bool,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "td_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "td_lenp").unwrap(),
            "td_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "td_datp").unwrap(),
            "td_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "td_srci");

        // clamp n to [0, len]: nc = max(0, min(n, len)).
        let zero = i64_ty.const_zero();
        let n_le_len = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, n, len, "td_nlt");
        let n_min = self.ir.builder.build_select(n_le_len, n, len, "td_nmin").unwrap().into_int_value();
        let n_ge_0 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGT, n_min, zero, "td_ngt");
        let nc = self.ir.builder.build_select(n_ge_0, n_min, zero, "td_nc").unwrap().into_int_value();

        // take: start=0, count=nc ; drop: start=nc, count=len-nc.
        let (start, count) = if is_take {
            (zero, nc)
        } else {
            (nc, build_wrappers::w_int_sub(&self.ir.builder, len, nc, "td_cnt"))
        };

        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, count, eight, "td_bytes");
        let dst_raw = self.emit_malloc(total, "td_dst");
        let dst_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "td_dsti");

        // for i in 0..count: dst[i] = src[start + i]
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "td_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, zero.into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "td.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "td.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "td.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "td_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, count, "td_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let s_idx = build_wrappers::w_int_add(&self.ir.builder, start, i_cur, "td_si");
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[s_idx], "td_sp").unwrap() };
        let v = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "td_v");
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[i_cur], "td_dp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, dp, v);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "td_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "td_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "td_olen").unwrap(),
            count.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "td_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "td_optr").unwrap(),
            dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "td_res"))
    }

    /// arr_map / arr_filter on an i64 slice with a lambda fat-pointer `{i8* fn,
    /// i8* env}`. Per element, indirect-call `fn(env, elem) -> i64`:
    ///   map    → dst[i] = result          (result len == src len)
    ///   filter → keep elem where result≠0 (result len ≤ src len, write-index)
    /// Returns a fresh `{len, ptr}` slice. Pure IR + malloc (native AND wasm).
    fn emit_arr_i64_closure(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        is_map: bool,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        // Extract the lambda fn ptr + env ptr from the fat pointer.
        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "cl_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "cl_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "cl_fp");
        // Indirect-call signature: i64 fn(i8* env, i64 arg). The lambda ABI is
        // uniformly i64-return — a bool predicate (filter) is zero-extended to
        // i64 at the lambda's return site, so we read it back as i64 and test
        // `!= 0` below.
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);

        // Unpack src len + data ptr.
        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "cl_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "cl_lenp").unwrap(),
            "cl_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "cl_datp").unwrap(),
            "cl_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "cl_srci");

        // Malloc len*8 for the destination (filter may use fewer — over-alloc ok).
        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, len, eight, "cl_bytes");
        let dst_raw = self.emit_malloc(total, "cl_dst");
        let dst_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "cl_dsti");

        // Loop index `i` and (for filter) write index `w`.
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "cl_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let w_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "cl_w");
        build_wrappers::w_store(&self.ir.builder, w_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "cl.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "cl.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "cl.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "cl_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "cl_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "cl_sp").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "cl_e").into_int_value();
        // r = lambda(env, elem)
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), elem.into()], "cl_call")
            .unwrap()
            .try_as_basic_value().left()?.into_int_value();
        let w_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), w_slot, "cl_wc").into_int_value();
        if is_map {
            // dst[i] = r ; (write index tracks i)
            let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[i_cur], "cl_dp").unwrap() };
            build_wrappers::w_store(&self.ir.builder, dp, r.into());
        } else {
            // filter: keep where the predicate is truthy (r != 0). r is the
            // i64-widened predicate result.
            let keep = build_wrappers::w_int_compare(
                &self.ir.builder, inkwell::IntPredicate::NE, r, i64_ty.const_zero(), "cl_keep");
            let keep_bb = self.ir.context.append_basic_block(fn_val, "cl.keep");
            let skip_bb = self.ir.context.append_basic_block(fn_val, "cl.skip");
            build_wrappers::w_cond_br(&self.ir.builder, keep, keep_bb, skip_bb);
            self.ir.builder.position_at_end(keep_bb);
            let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[w_cur], "cl_dpf").unwrap() };
            build_wrappers::w_store(&self.ir.builder, dp, elem.into());
            let w_next = build_wrappers::w_int_add(&self.ir.builder, w_cur, i64_ty.const_int(1, false), "cl_wn");
            build_wrappers::w_store(&self.ir.builder, w_slot, w_next.into());
            build_wrappers::w_br(&self.ir.builder, skip_bb);
            self.ir.builder.position_at_end(skip_bb);
        }
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "cl_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // Result length: map → src len; filter → write index.
        self.ir.builder.position_at_end(exit_bb);
        let out_len = if is_map {
            len
        } else {
            build_wrappers::w_load(&self.ir.builder, i64_ty.into(), w_slot, "cl_wf").into_int_value()
        };
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "cl_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "cl_olen").unwrap(),
            out_len.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "cl_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "cl_optr").unwrap(),
            dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "cl_res"))
    }

    /// arr_fold on an i64 slice with a 2-arg lambda fat-pointer: acc starts at
    /// `init`, then per element `acc = fn(env, acc, elem)`. Returns the final
    /// i64 acc. Pure IR (no allocation), native AND wasm.
    fn emit_arr_i64_fold(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        init: inkwell::values::IntValue<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "fd_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "fd_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "fd_fp");
        // i64 fn(i8* env, i64 acc, i64 elem).
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "fd_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "fd_lenp").unwrap(),
            "fd_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "fd_datp").unwrap(),
            "fd_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "fd_srci");

        let acc_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "fd_acc");
        build_wrappers::w_store(&self.ir.builder, acc_slot, init.into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "fd_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "fd.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "fd.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "fd.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "fd_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "fd_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "fd_sp").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "fd_e").into_int_value();
        let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "fd_a").into_int_value();
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), acc.into(), elem.into()], "fd_call")
            .unwrap()
            .try_as_basic_value().left()?.into_int_value();
        build_wrappers::w_store(&self.ir.builder, acc_slot, r.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "fd_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        Some(build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "fd_res"))
    }

    /// arr_zip_with(a, b, f) on two i64 slices: result[i] = f(a[i], b[i]) for
    /// i in 0..min(len_a, len_b). Mallocs an i64 result of length n; returns a
    /// fresh `{len, ptr}` slice. Pure IR + malloc (native AND wasm).
    fn emit_arr_i64_zip_with(
        &mut self,
        a_slice: BasicValueEnum<'ctx>,
        b_slice: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "zw_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "zw_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "zw_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);

        // Unpack both slices.
        let unpack = |slf: &mut Self, sv: BasicValueEnum<'ctx>, tag: &str| {
            let al = build_wrappers::w_alloca(&slf.ir.builder, slice_ty.into(), tag);
            build_wrappers::w_store(&slf.ir.builder, al, sv);
            let l = build_wrappers::w_load(
                &slf.ir.builder, i64_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 0, "zw_lp").unwrap(), "zw_l").into_int_value();
            let d = build_wrappers::w_load(
                &slf.ir.builder, ptr_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 1, "zw_dp").unwrap(), "zw_d").into_pointer_value();
            let di = build_wrappers::w_pointer_cast(&slf.ir.builder, d, i64_ty.ptr_type(AddressSpace::default()), "zw_di");
            (l, di)
        };
        let (a_len, a_data) = unpack(self, a_slice, "zw_a");
        let (b_len, b_data) = unpack(self, b_slice, "zw_b");

        // n = min(a_len, b_len).
        let a_le = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, a_len, b_len, "zw_alt");
        let n = self.ir.builder.build_select(a_le, a_len, b_len, "zw_n").unwrap().into_int_value();

        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, n, eight, "zw_bytes");
        let dst_raw = self.emit_malloc(total, "zw_dst");
        let dst_i64 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "zw_dsti");

        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "zw_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "zw.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "zw.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "zw.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "zw_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, n, "zw_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ap = unsafe { self.ir.builder.build_gep(i64_ty, a_data, &[i_cur], "zw_ap").unwrap() };
        let av = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ap, "zw_av").into_int_value();
        let bp = unsafe { self.ir.builder.build_gep(i64_ty, b_data, &[i_cur], "zw_bp").unwrap() };
        let bv = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), bp, "zw_bv").into_int_value();
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), av.into(), bv.into()], "zw_call")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[i_cur], "zw_dpst").unwrap() };
        build_wrappers::w_store(&self.ir.builder, dp, r.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "zw_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "zw_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "zw_olen").unwrap(), n.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "zw_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "zw_optr").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "zw_res"))
    }

    /// Build a fresh i64 slice `{i64 len, i8* data}` of length `count`, filling
    /// each slot via `fill(self, dst_i64_ptr, index)`. Shared backend for the
    /// constructor builtins. `count` must be ≥ 0 (callers clamp).
    fn emit_arr_i64_build(
        &mut self,
        count: inkwell::values::IntValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
        mut fill: impl FnMut(&mut Self, inkwell::values::PointerValue<'ctx>, inkwell::values::IntValue<'ctx>),
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, count, eight, "ab_bytes");
        let dst_raw = self.emit_malloc(total, "ab_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "ab_di");

        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ab_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "ab.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "ab.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "ab.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "ab_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, count, "ab_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[i_cur], "ab_dp").unwrap() };
        fill(self, dp, i_cur);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "ab_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ab_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "ab_olen").unwrap(), count.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "ab_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "ab_optr").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "ab_res"))
    }

    /// arr_range(start, end) → [start, start+1, …, end-1]. count = max(0,
    /// end-start); dst[i] = start + i.
    fn emit_arr_i64_range(
        &mut self,
        start: inkwell::values::IntValue<'ctx>,
        end: inkwell::values::IntValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let raw = build_wrappers::w_int_sub(&self.ir.builder, end, start, "rg_raw");
        let pos = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGT, raw, i64_ty.const_zero(), "rg_pos");
        let count = self.ir.builder.build_select(pos, raw, i64_ty.const_zero(), "rg_cnt").unwrap().into_int_value();
        self.emit_arr_i64_build(count, fn_val, move |slf, dp, i| {
            let v = build_wrappers::w_int_add(&slf.ir.builder, start, i, "rg_v");
            build_wrappers::w_store(&slf.ir.builder, dp, v.into());
        })
    }

    /// arr_repeat(v, n) → [v; max(0,n)]. dst[i] = v.
    fn emit_arr_i64_repeat(
        &mut self,
        v: inkwell::values::IntValue<'ctx>,
        n: inkwell::values::IntValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let pos = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGT, n, i64_ty.const_zero(), "rp_pos");
        let count = self.ir.builder.build_select(pos, n, i64_ty.const_zero(), "rp_cnt").unwrap().into_int_value();
        self.emit_arr_i64_build(count, fn_val, move |slf, dp, _i| {
            build_wrappers::w_store(&slf.ir.builder, dp, v.into());
        })
    }

    /// arr_concat(a, b) → a ++ b. count = a_len + b_len; dst[i] = i < a_len ?
    /// a[i] : b[i - a_len].
    fn emit_arr_i64_concat(
        &mut self,
        a_slice: BasicValueEnum<'ctx>,
        b_slice: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let unpack = |slf: &mut Self, sv: BasicValueEnum<'ctx>, tag: &str| {
            let al = build_wrappers::w_alloca(&slf.ir.builder, slice_ty.into(), tag);
            build_wrappers::w_store(&slf.ir.builder, al, sv);
            let l = build_wrappers::w_load(&slf.ir.builder, i64_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 0, "ct_lp").unwrap(), "ct_l").into_int_value();
            let d = build_wrappers::w_load(&slf.ir.builder, ptr_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 1, "ct_dp").unwrap(), "ct_d").into_pointer_value();
            let di = build_wrappers::w_pointer_cast(&slf.ir.builder, d, i64_ty.ptr_type(AddressSpace::default()), "ct_di");
            (l, di)
        };
        let (a_len, a_data) = unpack(self, a_slice, "ct_a");
        let (b_len, b_data) = unpack(self, b_slice, "ct_b");
        let count = build_wrappers::w_int_add(&self.ir.builder, a_len, b_len, "ct_cnt");
        self.emit_arr_i64_build(count, fn_val, move |slf, dp, i| {
            // v = i < a_len ? a[i] : b[i - a_len]. Both GEPs are emitted, so the
            // indices must stay in-bounds for the UNTAKEN branch too: clamp a's
            // index to a_len-1 and b's to 0 on the wrong side (the select then
            // discards that load's value).
            let in_a = build_wrappers::w_int_compare(&slf.ir.builder, inkwell::IntPredicate::SLT, i, a_len, "ct_ina");
            // a index: i if in_a else 0
            let a_idx = slf.ir.builder.build_select(in_a, i, i64_ty.const_zero(), "ct_aidx").unwrap().into_int_value();
            let ai = unsafe { slf.ir.builder.build_gep(i64_ty, a_data, &[a_idx], "ct_ai").unwrap() };
            let av = build_wrappers::w_load(&slf.ir.builder, i64_ty.into(), ai, "ct_av").into_int_value();
            // b index: 0 if in_a else (i - a_len)
            let bsub = build_wrappers::w_int_sub(&slf.ir.builder, i, a_len, "ct_bsub");
            let b_idx = slf.ir.builder.build_select(in_a, i64_ty.const_zero(), bsub, "ct_bidx").unwrap().into_int_value();
            let bi = unsafe { slf.ir.builder.build_gep(i64_ty, b_data, &[b_idx], "ct_bi").unwrap() };
            let bv = build_wrappers::w_load(&slf.ir.builder, i64_ty.into(), bi, "ct_bv").into_int_value();
            let v = slf.ir.builder.build_select(in_a, av, bv, "ct_v").unwrap().into_int_value();
            build_wrappers::w_store(&slf.ir.builder, dp, v.into());
        })
    }

    /// f64-element reductions over a slice `{i64 len, f64* data}` (Sum/Mean/
    /// Extreme). len/index stay i64; the element + accumulator are f64. Mean
    /// guards empty→0.0; Extreme panics (exit 101) on empty. Pure IR (native+wasm).
    fn emit_arr_f64_loop(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        op: ArrReduceF64,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let f64_ty = self.ir.context.f64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let s_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "af_s");
        build_wrappers::w_store(&self.ir.builder, s_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, s_alloca, 0, "af_lp").unwrap(), "af_len").into_int_value();
        let data_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, s_alloca, 1, "af_dp").unwrap(), "af_dat").into_pointer_value();
        let f64_data = build_wrappers::w_pointer_cast(&self.ir.builder, data_raw, f64_ty.ptr_type(AddressSpace::default()), "af_fd");

        // Extreme/ArgExtreme panic on empty (interp parity). Sum/Mean do not.
        if matches!(op, ArrReduceF64::Extreme { .. } | ArrReduceF64::ArgExtreme { .. }) {
            let empty_bb = self.ir.context.append_basic_block(fn_val, "af.empty");
            let ne_bb = self.ir.context.append_basic_block(fn_val, "af.ne");
            let is_empty = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::EQ, len, i64_ty.const_zero(), "af_e");
            build_wrappers::w_cond_br(&self.ir.builder, is_empty, empty_bb, ne_bb);
            self.ir.builder.position_at_end(empty_bb);
            if let Some(exit_fn) = self.ir.module.get_function("exit") {
                let code = self.ir.context.i32_type().const_int(101, false);
                build_wrappers::w_call(&self.ir.builder, exit_fn, &[code.into()], "");
            }
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(ne_bb);
        }

        // acc: Sum/Mean start 0.0; Extreme starts the first element (loaded once
        // len>0 is guaranteed). To keep it simple, init Extreme to ±inf so the
        // first compare always wins.
        let acc_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "af_acc");
        let init = match &op {
            ArrReduceF64::Sum | ArrReduceF64::Mean => f64_ty.const_float(0.0),
            ArrReduceF64::Extreme { is_max: true } | ArrReduceF64::ArgExtreme { is_max: true } => f64_ty.const_float(f64::NEG_INFINITY),
            ArrReduceF64::Extreme { is_max: false } | ArrReduceF64::ArgExtreme { is_max: false } => f64_ty.const_float(f64::INFINITY),
        };
        build_wrappers::w_store(&self.ir.builder, acc_slot, init.into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "af_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        // For argmax/argmin_f64: track the best element's INDEX.
        let best_idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "af_bidx");
        build_wrappers::w_store(&self.ir.builder, best_idx_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "af.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "af.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "af.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "af_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "af_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ep = unsafe { self.ir.builder.build_gep(f64_ty, f64_data, &[i_cur], "af_ep").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), ep, "af_e").into_float_value();
        let acc = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), acc_slot, "af_a").into_float_value();
        let nacc = match &op {
            ArrReduceF64::Sum | ArrReduceF64::Mean => build_wrappers::w_float_add(&self.ir.builder, acc, elem, "af_na"),
            ArrReduceF64::Extreme { is_max } => {
                let pred = if *is_max { inkwell::FloatPredicate::OGT } else { inkwell::FloatPredicate::OLT };
                let better = build_wrappers::w_float_compare(&self.ir.builder, pred, elem, acc, "af_b");
                self.ir.builder.build_select(better, elem, acc, "af_pick").unwrap().into_float_value()
            }
            ArrReduceF64::ArgExtreme { is_max } => {
                let pred = if *is_max { inkwell::FloatPredicate::OGT } else { inkwell::FloatPredicate::OLT };
                let better = build_wrappers::w_float_compare(&self.ir.builder, pred, elem, acc, "af_argb");
                let cur_best = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_idx_slot, "af_cb").into_int_value();
                let nbest = self.ir.builder.build_select(better, i_cur, cur_best, "af_argidx").unwrap().into_int_value();
                build_wrappers::w_store(&self.ir.builder, best_idx_slot, nbest.into());
                self.ir.builder.build_select(better, elem, acc, "af_argval").unwrap().into_float_value()
            }
        };
        build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "af_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let acc = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), acc_slot, "af_res").into_float_value();
        match op {
            ArrReduceF64::Sum | ArrReduceF64::Extreme { .. } => Some(acc.into()),
            ArrReduceF64::ArgExtreme { .. } => {
                let bidx = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_idx_slot, "af_bres").into_int_value();
                Some(bidx.into())
            }
            ArrReduceF64::Mean => {
                // mean = len==0 ? 0.0 : acc / (f64)len.
                let is_empty = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::EQ, len, i64_ty.const_zero(), "af_me");
                let len_f = build_wrappers::w_signed_int_to_float(&self.ir.builder, len, f64_ty, "af_lf");
                let div = self.ir.builder.build_float_div(acc, len_f, "af_mean").unwrap();
                let mean = self.ir.builder.build_select(is_empty, f64_ty.const_float(0.0), div, "af_msel").unwrap();
                Some(mean)
            }
        }
    }

    /// arr_count_if / arr_all / arr_any — a predicate reduction over an i64
    /// slice. Per element calls the lambda `i64 fn(env, elem)` (predicate, i64-
    /// widened) and folds: Count sums the truthy count; All accumulates AND;
    /// Any accumulates OR. (A full loop — no short-circuit — is observably
    /// identical for pure predicates and keeps the IR simple.) Returns i64 for
    /// Count, i1 for All/Any.
    fn emit_arr_i64_pred(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        kind: PredReduce,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let i1_ty = self.ir.context.bool_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "pr_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "pr_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "pr_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "pr_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "pr_lp").unwrap(), "pr_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "pr_dp").unwrap(), "pr_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "pr_si");

        // acc: Count starts 0; All starts 1 (true); Any starts 0 (false).
        let acc_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pr_acc");
        let init: u64 = match kind { PredReduce::Count | PredReduce::Any => 0, PredReduce::All => 1 };
        build_wrappers::w_store(&self.ir.builder, acc_slot, i64_ty.const_int(init, false).into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pr_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "pr.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "pr.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "pr.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "pr_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "pr_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ep = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "pr_ep").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ep, "pr_e").into_int_value();
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), elem.into()], "pr_call")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        // truthy = (r != 0) as i64 0/1.
        let truthy = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, r, i64_ty.const_zero(), "pr_t");
        let truthy64 = build_wrappers::w_int_z_extend(&self.ir.builder, truthy, i64_ty, "pr_t64");
        let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "pr_a").into_int_value();
        let nacc = match kind {
            PredReduce::Count => build_wrappers::w_int_add(&self.ir.builder, acc, truthy64, "pr_na"),
            PredReduce::All => build_wrappers::w_and(&self.ir.builder, acc, truthy64, "pr_na"),
            PredReduce::Any => build_wrappers::w_or(&self.ir.builder, acc, truthy64, "pr_na"),
        };
        build_wrappers::w_store(&self.ir.builder, acc_slot, nacc.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "pr_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let acc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), acc_slot, "pr_res").into_int_value();
        match kind {
            PredReduce::Count => Some(acc.into()),
            PredReduce::All | PredReduce::Any => {
                // acc is 0/1 → truncate to i1.
                let b = self.ir.builder.build_int_truncate(acc, i1_ty, "pr_b").unwrap();
                Some(b.into())
            }
        }
    }

    /// arr_sort_by(&a, cmp) — stable insertion sort of an i64 slice using an
    /// i64-comparator lambda (cmp(x, y) < 0 ⇒ x before y). Builds a fresh sorted
    /// buffer: for each element x, find lo = first index where cmp(x, dst[lo])<0,
    /// shift dst[lo..cnt] right, write dst[lo]=x. Matches the interpreter's
    /// insertion sort (stable). Pure IR + malloc (native AND wasm).
    fn emit_arr_i64_sort_by(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let one = i64_ty.const_int(1, false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "so_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "so_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "so_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into(), i64_ty.into()], false);

        // Unpack src.
        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "so_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "so_lp").unwrap(), "so_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "so_dp").unwrap(), "so_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "so_si");

        // dst buffer (len*8) and a running count.
        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, len, eight, "so_bytes");
        let dst_raw = self.emit_malloc(total, "so_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "so_di");
        let cnt_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "so_cnt");
        build_wrappers::w_store(&self.ir.builder, cnt_slot, i64_ty.const_zero().into());

        // Outer loop: i in 0..len.
        let i_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "so_i");
        build_wrappers::w_store(&self.ir.builder, i_slot, i64_ty.const_zero().into());
        let o_cond = self.ir.context.append_basic_block(fn_val, "so.ocond");
        let o_body = self.ir.context.append_basic_block(fn_val, "so.obody");
        let o_exit = self.ir.context.append_basic_block(fn_val, "so.oexit");
        build_wrappers::w_br(&self.ir.builder, o_cond);

        self.ir.builder.position_at_end(o_cond);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), i_slot, "so_ic").into_int_value();
        let o_go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "so_og");
        build_wrappers::w_cond_br(&self.ir.builder, o_go, o_body, o_exit);

        self.ir.builder.position_at_end(o_body);
        let xp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "so_xp").unwrap() };
        let x = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), xp, "so_x").into_int_value();
        let cnt = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), cnt_slot, "so_c").into_int_value();

        // Find lo: first index in 0..cnt where cmp(x, dst[lo]) < 0. Probe loop.
        let lo_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "so_lo");
        build_wrappers::w_store(&self.ir.builder, lo_slot, i64_ty.const_zero().into());
        let p_cond = self.ir.context.append_basic_block(fn_val, "so.pcond");
        let p_body = self.ir.context.append_basic_block(fn_val, "so.pbody");
        let p_exit = self.ir.context.append_basic_block(fn_val, "so.pexit");
        build_wrappers::w_br(&self.ir.builder, p_cond);

        self.ir.builder.position_at_end(p_cond);
        let lo_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), lo_slot, "so_loc").into_int_value();
        let p_go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, lo_cur, cnt, "so_pg");
        build_wrappers::w_cond_br(&self.ir.builder, p_go, p_body, p_exit);

        self.ir.builder.position_at_end(p_body);
        let dlo_p = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[lo_cur], "so_dlop").unwrap() };
        let dlo = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), dlo_p, "so_dlo").into_int_value();
        let cmp_r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), x.into(), dlo.into()], "so_cmp")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        // if cmp_r < 0 → break (found position); else lo++.
        let neg = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, cmp_r, i64_ty.const_zero(), "so_neg");
        let p_inc = self.ir.context.append_basic_block(fn_val, "so.pinc");
        build_wrappers::w_cond_br(&self.ir.builder, neg, p_exit, p_inc);
        self.ir.builder.position_at_end(p_inc);
        let lo_next = build_wrappers::w_int_add(&self.ir.builder, lo_cur, one, "so_lon");
        build_wrappers::w_store(&self.ir.builder, lo_slot, lo_next.into());
        build_wrappers::w_br(&self.ir.builder, p_cond);

        // Shift dst[lo..cnt] right by one: for j = cnt; j > lo; j-- : dst[j]=dst[j-1].
        self.ir.builder.position_at_end(p_exit);
        let lo_final = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), lo_slot, "so_lof").into_int_value();
        let j_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "so_j");
        build_wrappers::w_store(&self.ir.builder, j_slot, cnt.into());
        let s_cond = self.ir.context.append_basic_block(fn_val, "so.scond");
        let s_body = self.ir.context.append_basic_block(fn_val, "so.sbody");
        let s_exit = self.ir.context.append_basic_block(fn_val, "so.sexit");
        build_wrappers::w_br(&self.ir.builder, s_cond);

        self.ir.builder.position_at_end(s_cond);
        let j_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), j_slot, "so_jc").into_int_value();
        let s_go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGT, j_cur, lo_final, "so_sg");
        build_wrappers::w_cond_br(&self.ir.builder, s_go, s_body, s_exit);

        self.ir.builder.position_at_end(s_body);
        let j_prev = build_wrappers::w_int_sub(&self.ir.builder, j_cur, one, "so_jp");
        let from_p = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[j_prev], "so_fp2").unwrap() };
        let from_v = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), from_p, "so_fv");
        let to_p = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[j_cur], "so_tp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, to_p, from_v);
        build_wrappers::w_store(&self.ir.builder, j_slot, j_prev.into());
        build_wrappers::w_br(&self.ir.builder, s_cond);

        // Insert x at lo, cnt++, i++.
        self.ir.builder.position_at_end(s_exit);
        let ins_p = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[lo_final], "so_insp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, ins_p, x.into());
        let cnt2 = build_wrappers::w_int_add(&self.ir.builder, cnt, one, "so_c2");
        build_wrappers::w_store(&self.ir.builder, cnt_slot, cnt2.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, one, "so_in");
        build_wrappers::w_store(&self.ir.builder, i_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, o_cond);

        // Result slice { len, dst }.
        self.ir.builder.position_at_end(o_exit);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "so_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "so_olen").unwrap(), len.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "so_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "so_optr").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "so_res"))
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
            if let (Some(idx), Some(BasicTypeEnum::StructType(struct_ty))) =
                (idx_opt, self.llvm_type(&ty))
            {
                let recv_val = self.emit_expr(receiver, fn_val)?;
                let recv_alloca = self.ir.builder
                    .build_alloca(struct_ty, "asi_recv_tmp")
                    .unwrap();
                build_wrappers::w_store(&self.ir.builder, recv_alloca, recv_val);
                let fptr = self.ir.builder
                    .build_struct_gep(struct_ty, recv_alloca, idx, field)
                    .unwrap();
                if let Some(fty) = struct_ty.get_field_type_at_index(idx) {
                    let fval = build_wrappers::w_load(&self.ir.builder, fty, fptr, field);
                    return Some(fval);
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
                        let fval = build_wrappers::w_load(&self.ir.builder, field_ty, fptr, field);
                        return Some(fval);
                    }
                }
            }
        }
        // ── Tuple field access ───────────────────────────────────────────
        // For FieldAccess on a tuple, sem_type_of_expr returns None (it falls
        // through to infer_expr_sem_type). We need to get the type from the
        // base identifier in local_types.
        let tuple_ty = self.sem_type_of_expr(receiver).or_else(|| {
            // For chained field access like `t.0.1`, the receiver is a FieldAccess;
            // walk down to the base Ident and look it up.
            let mut base = receiver;
            loop {
                base = match base {
                    ast::Expr::FieldAccess { receiver, .. } => receiver.as_ref(),
                    _ => break,
                };
            }
            match base {
                ast::Expr::Ident(name) => self.local_types.get(name).cloned(),
                _ => None,
            }
        });
        if let Some(Type::Tuple(_elts)) = tuple_ty {
            if let Ok(field_idx) = field.parse::<u32>() {
                let recv_val = self.emit_expr(receiver, fn_val)?;
                // Tuple values are anonymous struct types stored on the stack
                // via alloca.  GEP + load to extract field N.
                let struct_ty = match recv_val {
                    BasicValueEnum::StructValue(s) => s.get_type(),
                    _ => return None,
                };
                let recv_alloca = self.ir.builder
                    .build_alloca(struct_ty, "tup_recv_tmp")
                    .unwrap();
                build_wrappers::w_store(&self.ir.builder, recv_alloca, recv_val);
                let fptr = self.ir.builder
                    .build_struct_gep(struct_ty, recv_alloca, field_idx, field)
                    .unwrap();
                if let Some(field_ty) = struct_ty.get_field_type_at_index(field_idx) {
                    let fval = build_wrappers::w_load(&self.ir.builder, field_ty, fptr, field);
                    return Some(fval);
                }
            }
        }
        // Fallback: emit receiver for side-effects only.
        let _ = self.emit_expr(receiver, fn_val);
        None
    }

    /// Emit a tuple literal as an anonymous LLVM struct value.
    fn emit_tuple_lit(&mut self, elems: &[ast::Expr], fn_val: FunctionValue<'ctx>) -> Option<BasicValueEnum<'ctx>> {
        let elem_vals: Vec<BasicValueEnum<'ctx>> = elems
            .iter()
            .map(|e| self.emit_expr(e, fn_val))
            .collect::<Option<_>>()?;

        // Build the struct type from the element LLVM types.
        let elem_types: Vec<BasicTypeEnum<'ctx>> = elem_vals.iter().map(|v| v.get_type()).collect();
        let struct_ty = self.ir.context.struct_type(&elem_types, false);

        // Allocate stack slot, store each field, load back for callers to use.
        let alloca = self.ir.builder.build_alloca(struct_ty, "tup_lit").unwrap();
        for (i, elem_val) in elem_vals.iter().enumerate() {
            let fptr = self.ir.builder
                .build_struct_gep(struct_ty, alloca, i as u32, "tup_elem")
                .unwrap();
            self.ir.builder.build_store(fptr, *elem_val).unwrap();
        }
        let loaded = self.ir.builder.build_load(struct_ty, alloca, "tup_copy").unwrap();
        Some(loaded)
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
        // Element size: use the type's REAL ABI size via LLVM `size_of()`, not a
        // hardcoded guess. The old code used 8 bytes for ANY struct, which
        // under-allocated an array of enums (`{i32 tag, [N x i8]}`, often 16-24
        // bytes): the GEP stores then wrote past the buffer → heap corruption
        // (`malloc(): corrupted top size`, BUG_HUNT #42). `size_of()` returns an
        // i64 LLVM constant that is exact for every element type (int/float/
        // struct/array/ptr), so `n * size_of` is the correct malloc size and
        // matches the GEP stride below.
        let elem_size = elem_ty.size_of().unwrap_or_else(|| i64_ty.const_int(8, false));
        let n_val = i64_ty.const_int(n as u64, false);
        let total_bytes = build_wrappers::w_int_mul(&self.ir.builder, elem_size, n_val, "arrbytes");
        // R7: target-aware malloc (i32 size on wasm32, i64 on native).
        let raw_ptr = self.emit_malloc(total_bytes, "arrdata");
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
                    let fat = build_wrappers::w_load(&self.ir.builder, ty, alloca, "closure");
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

        // `to_str` is polymorphic over scalars (BUG_HUNT #29/#40): the
        // interpreter dispatches on the runtime value, so codegen must dispatch
        // on the arg's LLVM type at the CALL SITE — `to_str` is declared
        // `i64→str`, so passing an f64/bool would otherwise be silently coerced
        // (float→int truncation; bool reinterpret) producing wrong output.
        // Mirror the string-interpolation path (`emit_fmt`): bool→to_str_bool,
        // f64→to_str_f64, i64→to_str. The interpreter is the oracle (I-2).
        if let ast::Expr::Ident(name) = callee {
            if name == "to_str" && args.len() == 1 {
                let v = self.emit_expr(&args[0], fn_val)?;
                let dispatched = match v {
                    BasicValueEnum::IntValue(iv) if iv.get_type().get_bit_width() == 1 => {
                        self.functions.get("to_str_bool").copied()
                            .map(|f| (f, BasicMetadataValueEnum::from(iv)))
                    }
                    BasicValueEnum::FloatValue(fv) => {
                        self.functions.get("to_str_f64").copied()
                            .map(|f| (f, BasicMetadataValueEnum::from(fv)))
                    }
                    BasicValueEnum::IntValue(iv) => {
                        // `to_str` takes i64; a narrower int (i8/i16/i32, e.g.
                        // from `abs_i32`) must be sign-extended first or the
                        // call fails IR verification (BUG_HUNT #40 i32 case).
                        let i64_ty = self.ir.context.i64_type();
                        let widened = if iv.get_type().get_bit_width() < 64 {
                            build_wrappers::w_int_s_extend(&self.ir.builder, iv, i64_ty, "to_str_sext")
                        } else {
                            iv
                        };
                        self.functions.get("to_str").copied()
                            .map(|f| (f, BasicMetadataValueEnum::from(widened)))
                    }
                    // Non-scalar (shouldn't reach here — checker rejects it):
                    // fall through to the default i64 path for a clean error.
                    _ => None,
                };
                if let Some((f, arg)) = dispatched {
                    return build_wrappers::w_call(&self.ir.builder, f, &[arg], "to_str_call")
                        .try_as_basic_value().left();
                }
            }
        }

        // `parse_bool_or(s, default)` — desugared INLINE here rather than as a
        // hand-built LLVM function. The hand-built form mishandled the i1 (bool)
        // default *parameter* across the call boundary (it read back as 0 — an
        // ABI corner that the i64/f64 `*_or` wrappers don't hit). Lowering it
        // inline keeps the i1 default as an SSA value in THIS frame, so there is
        // no cross-function i1 param: call `parse_bool(s)` (Result<bool,str>),
        // read the i1 Ok/Err tag, and `select` the Ok payload (i1) or `default`.
        // Matches the interpreter `parse_bool_or` (I-2).
        if let ast::Expr::Ident(name) = callee {
            if name == "parse_bool_or" && args.len() == 2 {
                if let Some(parse_bool_fn) = self.functions.get("parse_bool").copied() {
                    let s_val = self.emit_expr(&args[0], fn_val)?;
                    let default_val = self.emit_expr(&args[1], fn_val)?.into_int_value();
                    let result_ty = parse_bool_fn.get_type().get_return_type().unwrap();
                    let r = build_wrappers::w_call(&self.ir.builder, parse_bool_fn, &[s_val.into()], "pbo_r")
                        .try_as_basic_value().left()?;
                    let r_slot = build_wrappers::w_alloca(&self.ir.builder, result_ty, "pbo_rslot");
                    build_wrappers::w_store(&self.ir.builder, r_slot, r);
                    // tag (field 0, i1): Ok=1.
                    let tag_ptr = build_wrappers::w_struct_gep(&self.ir.builder, result_ty, r_slot, 0, "pbo_tagp");
                    let tag = build_wrappers::w_load(&self.ir.builder, self.ir.context.bool_type().into(), tag_ptr, "pbo_tag")
                        .into_int_value();
                    // Ok payload (field 1, reinterpreted as i1).
                    let pay_ptr = build_wrappers::w_struct_gep(&self.ir.builder, result_ty, r_slot, 1, "pbo_payp");
                    let pay_i1_ptr = build_wrappers::w_pointer_cast(
                        &self.ir.builder, pay_ptr,
                        self.ir.context.bool_type().ptr_type(AddressSpace::default()), "pbo_payvp");
                    let ok_val = build_wrappers::w_load(&self.ir.builder, self.ir.context.bool_type().into(), pay_i1_ptr, "pbo_okv")
                        .into_int_value();
                    let chosen = self.ir.builder
                        .build_select(tag, ok_val, default_val, "pbo_sel")
                        .unwrap();
                    return Some(chosen);
                }
            }
        }

        // Bitwise / shift builtins — had NO codegen lowering, so native silently
        // returned 0 (a real native↔interp divergence). They are trivial integer
        // ops, lowered INLINE here. Semantics match the interpreter (interp.rs):
        // bit_and/or/xor → a&b / a|b / a^b ; bit_not → ~n ; shl/shr → wrapping
        // left / ARITHMETIC right shift (i64 is signed). The interpreter is I-2.
        if let ast::Expr::Ident(name) = callee {
            let bin = |s: &str| -> bool { name == s && args.len() == 2 };
            if bin("bit_and") || bin("bit_or") || bin("bit_xor") || bin("shl") || bin("shr") {
                let a = self.emit_expr(&args[0], fn_val)?.into_int_value();
                let b = self.emit_expr(&args[1], fn_val)?.into_int_value();
                let r = match name.as_str() {
                    "bit_and" => build_wrappers::w_and(&self.ir.builder, a, b, "band"),
                    "bit_or"  => build_wrappers::w_or(&self.ir.builder, a, b, "bor"),
                    "bit_xor" => build_wrappers::w_xor(&self.ir.builder, a, b, "bxor"),
                    "shl"     => build_wrappers::w_left_shift(&self.ir.builder, a, b, "shl"),
                    // i64 is signed → arithmetic shift right (asr=true), matching
                    // the interpreter's `wrapping_shr` on i64.
                    "shr"     => build_wrappers::w_right_shift(&self.ir.builder, a, b, true, "shr"),
                    _ => unreachable!(),
                };
                return Some(r.into());
            }
            if name == "bit_not" && args.len() == 1 {
                let a = self.emit_expr(&args[0], fn_val)?.into_int_value();
                return Some(build_wrappers::w_not(&self.ir.builder, a, "bnot").into());
            }
        }

        // Polymorphic numeric casts as_i64 / as_f64 — like to_str, the
        // interpreter dispatches on the runtime value, so codegen dispatches on
        // the arg's LLVM type at the CALL SITE. Without a lowering native
        // returned 0 (silent divergence). Semantics (interp.rs): as_i64 accepts
        // i64 (identity), f64 (truncating), bool (0/1); as_f64 accepts i64/f64/
        // bool → f64. i1(bool) is sign-irrelevant (0/1) so zero-extend to i64.
        if let ast::Expr::Ident(name) = callee {
            if (name == "as_i64" || name == "as_f64") && args.len() == 1 {
                let v = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let f64_ty = self.ir.context.f64_type();
                if name == "as_i64" {
                    let out = match v {
                        BasicValueEnum::IntValue(iv) if iv.get_type().get_bit_width() == 64 => iv,
                        // bool / narrow int → zero-extend (bool is 0/1).
                        BasicValueEnum::IntValue(iv) => {
                            build_wrappers::w_int_z_extend(&self.ir.builder, iv, i64_ty, "as_i64_zext")
                        }
                        BasicValueEnum::FloatValue(fv) => {
                            build_wrappers::w_float_to_signed_int(&self.ir.builder, fv, i64_ty, "as_i64_ftoi")
                        }
                        _ => return None,
                    };
                    return Some(out.into());
                } else {
                    let out = match v {
                        BasicValueEnum::FloatValue(fv) => fv,
                        // i64 / bool → signed-int-to-float (bool 0/1 widens fine).
                        BasicValueEnum::IntValue(iv) => {
                            let widened = if iv.get_type().get_bit_width() < 64 {
                                build_wrappers::w_int_z_extend(&self.ir.builder, iv, i64_ty, "as_f64_zext")
                            } else {
                                iv
                            };
                            build_wrappers::w_signed_int_to_float(&self.ir.builder, widened, f64_ty, "as_f64_itof")
                        }
                        _ => return None,
                    };
                    return Some(out.into());
                }
            }
        }

        // Array reductions over an i64 slice (`{i64 len, i8* data}`), lowered
        // INLINE as a counted loop (pure IR → works native AND wasm). These had
        // no codegen (E0910). Semantics match the interpreter (interp.rs):
        //   arr_sum_i64(&a)      → Σ elements (0 for empty)
        //   arr_contains(&a, x)  → bool, any element == x
        // The slice arg is `&a`; `&` is a no-op at the LLVM level so it yields
        // the slice value directly. (arr_sum uses plain i64 add, not the
        // interpreter's saturating_add — they differ only on i64 overflow, which
        // realistic arrays don't hit; documented in the parity harness.)
        if let ast::Expr::Ident(name) = callee {
            if name == "arr_sum_i64" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_i64_loop(slice_val, ArrReduce::Sum, fn_val);
                }
            }
            if name == "arr_contains" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(n))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_loop(slice_val, ArrReduce::Contains(n), fn_val);
                }
            }
            if (name == "arr_max_i64" || name == "arr_min_i64") && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    let is_max = name == "arr_max_i64";
                    return self.emit_arr_i64_loop(slice_val, ArrReduce::Extreme { is_max }, fn_val);
                }
            }
            if name == "arr_mean_i64" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_i64_loop(slice_val, ArrReduce::Mean, fn_val);
                }
            }
            if (name == "arr_argmax_i64" || name == "arr_argmin_i64") && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    let is_max = name == "arr_argmax_i64";
                    return self.emit_arr_i64_loop(slice_val, ArrReduce::ArgExtreme { is_max }, fn_val);
                }
            }
            // f64-element reductions: arr_sum_f64 / arr_mean_f64 / arr_max_f64 /
            // arr_min_f64 — same loop shape as the i64 ones but the slice element
            // is f64 (8-byte load/store, float compares).
            if (name == "arr_sum_f64" || name == "arr_mean_f64"
                || name == "arr_max_f64" || name == "arr_min_f64"
                || name == "arr_argmax_f64" || name == "arr_argmin_f64") && args.len() == 1
            {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    let kind = match name.as_str() {
                        "arr_sum_f64" => ArrReduceF64::Sum,
                        "arr_mean_f64" => ArrReduceF64::Mean,
                        "arr_max_f64" => ArrReduceF64::Extreme { is_max: true },
                        "arr_min_f64" => ArrReduceF64::Extreme { is_max: false },
                        "arr_argmax_f64" => ArrReduceF64::ArgExtreme { is_max: true },
                        _ => ArrReduceF64::ArgExtreme { is_max: false },
                    };
                    return self.emit_arr_f64_loop(slice_val, kind, fn_val);
                }
            }
            // arr_reverse(&a) — the first ALLOCATING arr_* lowering: malloc a new
            // i64 buffer of the same length and copy src[len-1-i] → dst[i].
            // Returns a fresh `{len, ptr}` slice (i64-element arrays; the common
            // case — other element types stay E0910-gated below).
            if name == "arr_reverse" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    if let Some(r) = self.emit_arr_i64_reverse(slice_val, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_take(&a, n) / arr_drop(&a, n) — copy a contiguous i64 range into
            // a fresh slice. take = first min(n,len); drop = from min(n,len) on.
            if (name == "arr_take" || name == "arr_drop") && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(n))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let is_take = name == "arr_take";
                    if let Some(r) = self.emit_arr_i64_take_drop(slice_val, n, is_take, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_map(&a, |x| ...) / arr_filter(&a, |x| ...) — the first
            // CLOSURE-taking arr_* lowerings. The 2nd arg is a lambda fat-pointer
            // `{i8* fn, i8* env}`; per element we indirect-call `fn(env, elem)`.
            // map → mapped value into the result; filter → keep where pred true.
            if (name == "arr_map" || name == "arr_filter") && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let is_map = name == "arr_map";
                    if let Some(r) = self.emit_arr_i64_closure(slice_val, lam, is_map, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_fold(&a, init, |acc, x| ...) — reduce with a 2-arg i64 closure.
            // acc starts at `init`; per element acc = f(acc, elem). Returns the
            // final i64 acc (no allocation).
            if name == "arr_fold" && args.len() == 3 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(init)), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val), self.emit_expr(&args[2], fn_val))
                {
                    if let Some(r) = self.emit_arr_i64_fold(slice_val, init, lam, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_zip_with(a, b, |x, y| ...) — the first TWO-SLICE closure op.
            // result[i] = f(a[i], b[i]) for i in 0..min(len_a, len_b); i64 result.
            if name == "arr_zip_with" && args.len() == 3 {
                if let (Some(a_slice), Some(b_slice), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val), self.emit_expr(&args[2], fn_val))
                {
                    if let Some(r) = self.emit_arr_i64_zip_with(a_slice, b_slice, lam, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_sort_by(&a, |x, y| cmp) — stable insertion sort with an i64
            // comparator (negative ⇒ x sorts before y). Builds a fresh sorted
            // slice by inserting each element at its position.
            if name == "arr_sort_by" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    if let Some(r) = self.emit_arr_i64_sort_by(slice_val, lam, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_range(start, end) → [start, start+1, …, end-1] (empty if
            // end<=start). arr_repeat(v, n) → [v; max(0,n)]. Both allocate an
            // i64 slice; no closure.
            if name == "arr_range" && args.len() == 2 {
                if let (Some(BasicValueEnum::IntValue(start)), Some(BasicValueEnum::IntValue(end))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_range(start, end, fn_val);
                }
            }
            if name == "arr_repeat" && args.len() == 2 {
                if let (Some(BasicValueEnum::IntValue(v)), Some(BasicValueEnum::IntValue(n))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_repeat(v, n, fn_val);
                }
            }
            // arr_concat(a, b) → a ++ b (two i64 slices into one fresh slice).
            if name == "arr_concat" && args.len() == 2 {
                if let (Some(a_slice), Some(b_slice)) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_concat(a_slice, b_slice, fn_val);
                }
            }
            // arr_count_if / arr_all / arr_any — predicate reductions over an i64
            // slice. count_if → i64 count of true; all → i1 (false on first
            // false, short-circuits); any → i1 (true on first true).
            if (name == "arr_count_if" || name == "arr_all" || name == "arr_any") && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let kind = match name.as_str() {
                        "arr_count_if" => PredReduce::Count,
                        "arr_all" => PredReduce::All,
                        _ => PredReduce::Any,
                    };
                    if let Some(r) = self.emit_arr_i64_pred(slice_val, lam, kind, fn_val) {
                        return Some(r);
                    }
                }
            }
        }

        // Honest-error guard: if the callee is a KNOWN builtin (in the BUILTINS
        // table) that reached here unresolved — no registered LLVM fn and no
        // inline lowering above — it has no native codegen. Emitting None here
        // would silently drop the call and yield a 0/garbage value (the
        // arr_*/dict_* "returns 0 natively" divergence class). Record a hard
        // error so the build pipeline aborts instead of shipping a wrong binary.
        if maybe_fn_v.is_none() {
            if let ast::Expr::Ident(name) = callee {
                if crate::builtins::BUILTINS.iter().any(|b| b.name == name.as_str()) {
                    let msg = format!(
                        "codegen error [E0910]: builtin `{name}` is not yet supported by the \
                         native codegen backend (it runs under the interpreter — use `axon run`). \
                         Building it would silently compute a wrong value."
                    );
                    if !self.codegen_errors.iter().any(|e| e == &msg) {
                        eprintln!("{msg}");
                        self.codegen_errors.push(msg);
                    }
                    // Return a zero of a best-effort type so emission continues
                    // (the build aborts afterward on codegen_errors); this avoids
                    // a cascade of confusing secondary diagnostics.
                    return Some(self.ir.context.i64_type().const_zero().into());
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
                            let data_alloca = build_wrappers::w_alloca(&self.ir.builder, concrete_llvm_ty, "dyn_data");
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

        // R4 §4.3: if this is a capability-bearing builtin called inside an
        // `@[agent]` fn, emit the mandatory agent_action audit log before the
        // call (no-op otherwise). Keyed on the callee name + the agent context.
        if let ast::Expr::Ident(name) = callee {
            self.emit_agent_action_log(name);
        }

        let call = self.ir
            .builder
            .build_call(fn_v, &arg_vals, "call")
            .unwrap();
        call.try_as_basic_value().left()
    }


}
