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

/// How `emit_arr_i64_closure` turns the per-element lambda result into output.
#[derive(Clone, Copy, PartialEq)]
enum ClosureMode {
    /// `arr_map`: dst[i] = lambda(elem); result len = src len.
    Map,
    /// `arr_filter`: keep elem where lambda(elem) != 0; result len = #kept.
    Filter,
    /// `arr_take_while`: keep the leading prefix while lambda(elem) != 0; stop
    /// (keep nothing further) at the first element that fails.
    TakeWhile,
    /// `arr_drop_while`: skip the leading prefix while lambda(elem) != 0; once an
    /// element fails, keep it and every element after.
    DropWhile,
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
    /// Index of the FIRST element equal to the needle, or -1 if none (i64
    /// result). Empty array → -1 (no panic), matching the interpreter.
    IndexOf(inkwell::values::IntValue<'ctx>),
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

            // Phase 6 (surface slice): a `with <handler> { body }` lowers to its
            // body — handlers are inert (no effect discharge / resume yet), and
            // effect rows are erased before codegen, so the IR is exactly the
            // body's. Handler arm bodies are not emitted (never dispatched yet).
            ast::Expr::WithHandler { body, .. } => self.emit_expr(body, fn_val),

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
            ast::Expr::Let { name, value, ty }
            | ast::Expr::Own { name, value, ty }
            | ast::Expr::RefBind { name, value, ty } => {
                // Prefer the EXPLICIT type annotation when present — the value
                // alone can't always be fully inferred. Critically for
                // `let r: Result<i64, str> = Ok(7)`: `infer_expr_sem_type(Ok(7))`
                // yields `Result<i64, Unknown>` (the value can't reveal the Err
                // type), so a later `match r { Err(e) => … }` would extract the
                // Err payload with the wrong type and fail IR verification. The
                // annotation carries the full `Result<i64, str>`.
                let sem_ty = ty
                    .as_ref()
                    .map(|t| self.axon_type_to_semantic(t))
                    .or_else(|| self.infer_expr_sem_type(value));
                // When the annotation is a Result<T,E>, set current_result_types
                // around the VALUE emission so `emit_result` allocates the full
                // canonical union layout `{ i1, [max(sizeof T, sizeof E)] }`
                // instead of the value-only fallback `{ i1, sizeof(Ok-value) }`.
                // Without this, `let r: Result<i64,str> = Ok(1)` lays r out as
                // `{i1,i64}` (8-byte payload), too small for a later `Err(str)`
                // (16 bytes) — a reassignment then stored a wrong-sized payload
                // and the match read GARBAGE at exit 0 (I-9), and passing r to a
                // fn expecting the full layout failed IR verification.
                let saved_rt = self.current_result_types.clone();
                let saved_oi = self.current_option_inner.clone();
                // The annotation may be the sum type directly, OR a slice of it
                // (`let a: [Result<i64,str>] = [Ok(1), Err("x")]`) — in the slice
                // case each ELEMENT is the sum type, so unwrap one level so the
                // array-literal elements get the right layout (a mismatched-size
                // element array otherwise SIGSEGVs at exit 139, not just IR-fail).
                let target = match &sem_ty {
                    Some(Type::Slice(inner)) => Some((**inner).clone()),
                    other => other.clone(),
                };
                if let Some(Type::Result(ok_ty, err_ty)) = &target {
                    self.current_result_types = Some((*ok_ty.clone(), *err_ty.clone()));
                }
                if let Some(Type::Option(inner)) = &target {
                    self.current_option_inner = Some(*inner.clone());
                }
                let val = self.emit_expr(value, fn_val)?;
                self.current_result_types = saved_rt;
                self.current_option_inner = saved_oi;
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
                // Layer-2 ASI: a `Temporal<T>` operand unwraps to its present
                // value (field 0) and operates as a plain `T` — a plain result,
                // no propagation (mirrors the interp's Temporal binop path). Lets
                // `t > 5` / `t + n` compile instead of an IR type mismatch.
                let is_temp = |t: &Option<Type>| matches!(t, Some(Type::Temporal(_)));
                if is_temp(&lt_sem) || is_temp(&rt_sem) {
                    let mut lhs = self.emit_expr(left, fn_val)?;
                    let mut rhs = self.emit_expr(right, fn_val)?;
                    if is_temp(&lt_sem) {
                        if let BasicValueEnum::StructValue(sv) = lhs {
                            lhs = self.ir.builder.build_extract_value(sv, 0, "temp_l").ok()?;
                        }
                    }
                    if is_temp(&rt_sem) {
                        if let BasicValueEnum::StructValue(sv) = rhs {
                            rhs = self.ir.builder.build_extract_value(sv, 0, "temp_r").ok()?;
                        }
                    }
                    // The inner T from whichever side is Temporal.
                    let inner_ty = match (&lt_sem, &rt_sem) {
                        (Some(Type::Temporal(t)), _) => (**t).clone(),
                        (_, Some(Type::Temporal(t))) => (**t).clone(),
                        _ => Type::I64,
                    };
                    return Some(self.emit_binop(op, lhs, rhs, &inner_ty));
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
                    // An `Uncertain<bool>` condition (`if a > 5` where `a` is
                    // Uncertain — the comparison stays Uncertain `{bool,f64,i64}`)
                    // branches on its inner bool (field 0); confidence is
                    // irrelevant to control flow. Matches the interpreter.
                    BasicValueEnum::StructValue(sv) => {
                        match self.ir.builder.build_extract_value(sv, 0, "unc_cond") {
                            Ok(BasicValueEnum::IntValue(i)) => i,
                            _ => return None,
                        }
                    }
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
                // Use the target Option<T> inner type when known (from a `let
                // o: Option<str> = None` annotation or an Option-typed assign
                // target), so the layout is `{ i1, T }` — not the `{i1,i64}`
                // default that mis-sizes a later `Some(str)` / match.
                let inner = self.current_option_inner.clone().unwrap_or(Type::I64);
                Some(self.emit_option(std::option::Option::None, &inner))
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
                // If the target local is a Result<T,E>, emit the new value with
                // current_result_types set so a reassigned `Err(..)`/`Ok(..)`
                // builds the SAME canonical union layout the slot was allocated
                // for. Without this, `r = Err("no")` (str payload) built a
                // `{i1,ptr}` and stored it into the `{i1,[16 x i8]}` slot —
                // mismatched, and the later match read garbage at exit 0 (I-9).
                let saved_rt = self.current_result_types.clone();
                let saved_oi = self.current_option_inner.clone();
                match self.local_types.get(name).cloned() {
                    Some(Type::Result(ok_ty, err_ty)) => {
                        self.current_result_types = Some((*ok_ty, *err_ty));
                    }
                    Some(Type::Option(inner)) => {
                        self.current_option_inner = Some(*inner);
                    }
                    _ => {}
                }
                let emitted = self.emit_expr(value, fn_val);
                self.current_result_types = saved_rt;
                self.current_option_inner = saved_oi;
                if let Some(val) = emitted {
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

    /// Emit a guarded call to `__axon_arith_panic` on the *current* block's
    /// failure branch, then leave the builder positioned at a fresh "ok" block
    /// so the caller can continue emitting the success value. `cond_fail` is the
    /// i1 "the fault happened" predicate. `kind` selects the runtime message (0
    /// overflow / 1 div0 / 2 rem0); `a`/`b` are the operands passed through for
    /// the overflow message (ignored by the runtime for kinds 1/2). This mirrors
    /// the `__axon_verify_panic` emission shape in `asi.rs`.
    fn emit_arith_guard(
        &mut self,
        cond_fail: inkwell::values::IntValue<'ctx>,
        kind: i64,
        op_glyph: &str,
        a: inkwell::values::IntValue<'ctx>,
        b: inkwell::values::IntValue<'ctx>,
    ) {
        let panic_fn = match self.ir.module.get_function("__axon_arith_panic") {
            Some(f) => f,
            None => return, // runtime extern absent — leave the raw op (defensive)
        };
        let cur_block = match self.ir.builder.get_insert_block() {
            Some(b) if b.get_terminator().is_none() => b,
            _ => return,
        };
        let llvm_fn = match cur_block.get_parent() {
            Some(f) => f,
            None => return,
        };
        let panic_bb = self.ir.context.append_basic_block(llvm_fn, "arith_panic");
        let ok_bb = self.ir.context.append_basic_block(llvm_fn, "arith_ok");
        // cond_fail ? panic : ok
        build_wrappers::w_cond_br(&self.ir.builder, cond_fail, panic_bb, ok_bb);

        // Panic path: call the runtime trap, then `unreachable`.
        self.ir.builder.position_at_end(panic_bb);
        let i64_ty = self.ir.context.i64_type();
        let op_g = build_wrappers::w_global_string_ptr(&self.ir.builder, op_glyph, "arith_op");
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            panic_fn,
            &[
                i64_ty.const_int(kind as u64, true).into(),
                op_g.into(),
                i64_ty.const_int(op_glyph.len() as u64, false).into(),
                a.into(),
                b.into(),
            ],
            "",
        );
        build_wrappers::w_unreachable(&self.ir.builder);

        // Continue at the ok block.
        self.ir.builder.position_at_end(ok_bb);
    }

    /// Constant-fold a checked signed-i64 op when BOTH operands are compile-time
    /// constants and the checked operation succeeds (no overflow / no div0).
    /// Returns the folded constant so the caller can skip the runtime guard
    /// entirely — both a perf win and the reason a pure-constant program (e.g.
    /// `21 + 21`) emits NO `__axon_arith_panic` extern. Returns `None` when an
    /// operand isn't constant or the fold would fault (leave it to the runtime
    /// guard, which produces the same panic the interpreter does). Mirrors the
    /// interpreter's checked_add/sub/mul + zero-divisor / wrapping_div.
    fn try_const_fold_int(
        &self,
        op: &ast::BinOp,
        l: inkwell::values::IntValue<'ctx>,
        r: inkwell::values::IntValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let a = l.get_sign_extended_constant()?;
        let b = r.get_sign_extended_constant()?;
        let folded: i64 = match op {
            ast::BinOp::Add => a.checked_add(b)?,
            ast::BinOp::Sub => a.checked_sub(b)?,
            ast::BinOp::Mul => a.checked_mul(b)?,
            ast::BinOp::Div => {
                if b == 0 { return None; }
                a.wrapping_div(b)
            }
            ast::BinOp::Rem => {
                if b == 0 { return None; }
                a.wrapping_rem(b)
            }
            _ => return None,
        };
        Some(self.ir.context.i64_type().const_int(folded as u64, true).into())
    }

    /// Emit a bounds guard for an `a[i]` access: if `idx < 0 || idx >= len`,
    /// divert to `__axon_bounds_panic(idx, len)` (exit 101, same message the
    /// interpreter prints), else fall through to the load. Leaves the builder
    /// positioned at the "in-bounds" block. No-op (raw access kept) if the
    /// runtime extern is absent or the current block is already terminated.
    fn emit_bounds_guard(
        &mut self,
        idx: inkwell::values::IntValue<'ctx>,
        len: inkwell::values::IntValue<'ctx>,
    ) {
        let panic_fn = match self.ir.module.get_function("__axon_bounds_panic") {
            Some(f) => f,
            None => return,
        };
        let cur_block = match self.ir.builder.get_insert_block() {
            Some(b) if b.get_terminator().is_none() => b,
            _ => return,
        };
        let llvm_fn = match cur_block.get_parent() {
            Some(f) => f,
            None => return,
        };
        // idx < 0  (signed)  OR  idx >= len  (signed) → out of bounds.
        let zero = self.ir.context.i64_type().const_zero();
        let neg = build_wrappers::w_int_compare(
            &self.ir.builder, IntPredicate::SLT, idx, zero, "idx_neg",
        );
        let over = build_wrappers::w_int_compare(
            &self.ir.builder, IntPredicate::SGE, idx, len, "idx_over",
        );
        let oob = self.ir.builder.build_or(neg, over, "oob").unwrap();

        let panic_bb = self.ir.context.append_basic_block(llvm_fn, "bounds_panic");
        let ok_bb = self.ir.context.append_basic_block(llvm_fn, "bounds_ok");
        build_wrappers::w_cond_br(&self.ir.builder, oob, panic_bb, ok_bb);

        self.ir.builder.position_at_end(panic_bb);
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            panic_fn,
            &[idx.into(), len.into()],
            "",
        );
        build_wrappers::w_unreachable(&self.ir.builder);

        self.ir.builder.position_at_end(ok_bb);
    }

    /// Checked signed-i64 `+`/`-`/`*` using the LLVM `llvm.s{add,sub,mul}.with
    /// .overflow` intrinsics. Returns the result; on overflow control never
    /// reaches the return — `emit_arith_guard` diverts to the runtime trap
    /// (exit 101), matching the interpreter's checked arithmetic (I-9). Returns
    /// `None` if the intrinsic can't be resolved, so the caller falls back to
    /// the raw (unchecked) op rather than miscompiling.
    fn emit_checked_overflow_op(
        &mut self,
        intrinsic_name: &str,
        op_glyph: &str,
        l: inkwell::values::IntValue<'ctx>,
        r: inkwell::values::IntValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let intr = inkwell::intrinsics::Intrinsic::find(intrinsic_name)?;
        let decl = intr.get_declaration(&self.ir.module, &[i64_ty.into()])?;
        let call = build_wrappers::w_call(
            &self.ir.builder,
            decl,
            &[l.into(), r.into()],
            "ovf",
        );
        let agg = call.try_as_basic_value().left()?.into_struct_value();
        // Field 0 = result, field 1 = i1 overflow flag.
        let result = build_wrappers::w_extract_value(&self.ir.builder, agg, 0, "ovf_res");
        let flag = build_wrappers::w_extract_value(&self.ir.builder, agg, 1, "ovf_flag")
            .into_int_value();
        self.emit_arith_guard(flag, 0, op_glyph, l, r);
        Some(result)
    }

    /// Checked signed-i64 `/` or `%`. Guards divisor == 0 (→ runtime trap, exit
    /// 101) and neutralises the `INT_MIN / -1` hardware trap with a select that
    /// reproduces the interpreter's `wrapping_div`/`wrapping_rem` (INT_MIN for
    /// div, 0 for rem) — so native never SIGFPEs (exit 136) where the
    /// interpreter returns a defined value or a clean panic.
    fn emit_checked_div_rem(
        &mut self,
        is_rem: bool,
        l: inkwell::values::IntValue<'ctx>,
        r: inkwell::values::IntValue<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let i64_ty = self.ir.context.i64_type();
        let zero = i64_ty.const_zero();
        // divisor == 0 → trap (kind 1 div0 / 2 rem0).
        let is_zero = build_wrappers::w_int_compare(
            &self.ir.builder, IntPredicate::EQ, r, zero, "divzero",
        );
        self.emit_arith_guard(is_zero, if is_rem { 2 } else { 1 }, if is_rem { "%" } else { "/" }, l, r);

        // Guard INT_MIN / -1 (the one signed-division case that traps in
        // hardware). Replace the divisor with 1 on that path and patch the
        // result via select, matching wrapping_div/wrapping_rem.
        let int_min = i64_ty.const_int(i64::MIN as u64, false);
        let neg_one = i64_ty.const_int((-1i64) as u64, true);
        let l_is_min = build_wrappers::w_int_compare(
            &self.ir.builder, IntPredicate::EQ, l, int_min, "l_is_min",
        );
        let r_is_neg1 = build_wrappers::w_int_compare(
            &self.ir.builder, IntPredicate::EQ, r, neg_one, "r_is_neg1",
        );
        let is_trap = self.ir.builder.build_and(l_is_min, r_is_neg1, "minneg1").unwrap();
        // Safe divisor: 1 when the trap case, else r.
        let one = i64_ty.const_int(1, false);
        let safe_r = self.ir.builder
            .build_select(is_trap, one, r, "safe_div")
            .unwrap()
            .into_int_value();
        let raw = if is_rem {
            build_wrappers::w_int_signed_rem(&self.ir.builder, l, safe_r, "rem")
        } else {
            build_wrappers::w_int_signed_div(&self.ir.builder, l, safe_r, "div")
        };
        // wrapping_div(INT_MIN,-1)=INT_MIN ; wrapping_rem(INT_MIN,-1)=0.
        let patched = if is_rem { zero } else { int_min };
        self.ir.builder
            .build_select(is_trap, patched, raw, "divrem_fix")
            .unwrap()
    }

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
                // Checked signed-i64 +/-/* and /,% — match the interpreter's
                // checked arithmetic (overflow / div-by-zero → graceful panic
                // exit 101, never a silent wrap or a raw SIGFPE; I-9). Only the
                // i64 signed path is checked (the interpreter's `Int` is i64);
                // narrower or unsigned ints keep the raw op.
                ast::BinOp::Add if !is_unsigned && l.get_type().get_bit_width() == 64 => self
                    .try_const_fold_int(op, l, r)
                    .or_else(|| self.emit_checked_overflow_op("llvm.sadd.with.overflow", "+", l, r))
                    .unwrap_or_else(|| build_wrappers::w_int_add(&self.ir.builder, l, r, "add").into()),
                ast::BinOp::Sub if !is_unsigned && l.get_type().get_bit_width() == 64 => self
                    .try_const_fold_int(op, l, r)
                    .or_else(|| self.emit_checked_overflow_op("llvm.ssub.with.overflow", "-", l, r))
                    .unwrap_or_else(|| build_wrappers::w_int_sub(&self.ir.builder, l, r, "sub").into()),
                ast::BinOp::Mul if !is_unsigned && l.get_type().get_bit_width() == 64 => self
                    .try_const_fold_int(op, l, r)
                    .or_else(|| self.emit_checked_overflow_op("llvm.smul.with.overflow", "*", l, r))
                    .unwrap_or_else(|| build_wrappers::w_int_mul(&self.ir.builder, l, r, "mul").into()),
                ast::BinOp::Div if !is_unsigned && l.get_type().get_bit_width() == 64 =>
                    self.try_const_fold_int(op, l, r)
                        .unwrap_or_else(|| self.emit_checked_div_rem(false, l, r)),
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
                ast::BinOp::Rem if !is_unsigned && l.get_type().get_bit_width() == 64 =>
                    self.try_const_fold_int(op, l, r)
                        .unwrap_or_else(|| self.emit_checked_div_rem(true, l, r)),
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
            // An `Uncertain<bool>` condition branches on its inner bool (field 0),
            // same as `if` — confidence is irrelevant to control flow.
            Some(BasicValueEnum::StructValue(sv)) => {
                match self.ir.builder.build_extract_value(sv, 0, "unc_wcond") {
                    Ok(BasicValueEnum::IntValue(i)) => i,
                    _ => {
                        build_wrappers::w_br(&self.ir.builder, body_bb);
                        self.loop_stack.pop();
                        self.ir.builder.position_at_end(exit_bb);
                        return Some(self.ir.context.i64_type().const_zero().into());
                    }
                }
            }
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
        // Each captured variable's env slot uses its ACTUAL LLVM type, read from
        // the caller's locals (still in scope here — `self.locals` is saved
        // below). This lets a pointer (e.g. a Dict handle), f64, or str capture
        // round-trip instead of being forced through an i64 slot (which crashed
        // IR-verification when the body used it as its real type). A capture the
        // caller doesn't have a local for falls back to i64 (the old default).
        let n_captures = captures.len();
        let capture_llvm_tys: Vec<BasicTypeEnum<'ctx>> = captures
            .iter()
            .map(|(name, _)| {
                self.locals
                    .get(name.as_str())
                    .map(|&(_, ty)| ty)
                    .unwrap_or_else(|| i64_ty.into())
            })
            .collect();
        let env_struct_ty = self.ir.context.struct_type(&capture_llvm_tys, false);

        // Each parameter's LLVM type: a caller-supplied hint
        // (`pending_lambda_param_tys`, set by a builtin lowering like dict_filter
        // whose `fn(str,V)` sig types an inline `|k,v|`) takes priority, then the
        // param's explicit annotation, then i64 for an un-annotated `|x|` — which
        // keeps every existing i64/bool closure byte-identical. The generic
        // closure-call site types the indirect call from the actual arg values,
        // so the declaration and the call agree.
        let hint = self.pending_lambda_param_tys.take();
        let param_llvm_tys: Vec<BasicTypeEnum<'ctx>> = params
            .iter()
            .enumerate()
            .map(|(i, p)| {
                hint.as_ref()
                    .and_then(|h| h.get(i).copied())
                    .or_else(|| p.ty.as_ref().and_then(|t| self.llvm_type_from_axon(t)))
                    .unwrap_or_else(|| i64_ty.into())
            })
            .collect();

        // Honest gate (E0910): the closure ABI is i64-RETURN — a closure value is
        // a bare `{fn_ptr, env_ptr}` fat pointer carrying no return-type tag, so a
        // polymorphic call site can only assume i64 back. A lambda whose body
        // yields a str (a {i64,ptr} struct), a slice, or a tuple therefore can't
        // round-trip its result; emitting it crashes IR-verification. Predict the
        // body type (the same inference used elsewhere) and, when it's clearly
        // non-i64, record a clean E0910 so the build aborts with an actionable
        // message instead of a raw LLVM error. (i64/bool/f64 bodies are fine —
        // f64 is bitcast-transported through the i64 slot; see the return site.)
        if let Some(bt) = self.infer_expr_sem_type(body) {
            let unsupported_ret = match &bt {
                crate::types::Type::Str => Some("str"),
                crate::types::Type::Slice(_) => Some("slice"),
                crate::types::Type::Tuple(_) => Some("tuple"),
                _ => None,
            };
            if let Some(kind) = unsupported_ret {
                let msg = format!(
                    "codegen error [E0910]: native codegen does not yet support a lambda whose \
                     body returns a {kind} — the closure ABI is i64-return (a closure value \
                     carries no return-type tag, so a str/slice/tuple result can't round-trip). \
                     This program runs under the interpreter (`axon run`)."
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    eprintln!("{msg}");
                    self.codegen_errors.push(msg);
                }
            }
        }

        // ── Declare the lambda function (env_ptr first, then params) ──
        let mut lambda_param_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
            vec![ptr_ty.into()]; // env_ptr
        for t in &param_llvm_tys {
            lambda_param_tys.push((*t).into());
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
                self.locals.insert(cap_name.clone(), (field_ptr, capture_llvm_tys[idx]));
                capture_idx_map.insert(cap_name.clone(), idx as u32);
            }
        }

        // Publish the env context so nested `Ident` lookups can fall back
        // to loading captures via GEP if the resolver missed them.
        self.current_lambda_env = Some((env_ptr_arg, env_struct_ty, capture_idx_map));

        // Bind explicit parameters (offset by 1 for env_ptr), each with its real
        // LLVM type so the body sees a correctly-typed local (a str param is a
        // {i64,ptr} struct local that str_len/str_contains/etc. read directly).
        for (i, p) in params.iter().enumerate() {
            if let Some(arg) = lambda_fn.get_nth_param((i + 1) as u32) {
                let pty = param_llvm_tys[i];
                let alloca = build_wrappers::w_alloca(&self.ir.builder, pty, &p.name);
                build_wrappers::w_store(&self.ir.builder, alloca, arg);
                self.locals.insert(p.name.clone(), (alloca, pty));
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
                        // An f64-bodied lambda (e.g. a numeric `key_fn` for
                        // arr_max_by/min_by) is transported through the uniform
                        // i64-return ABI by BITCASTING its bits to i64 (a pure
                        // reinterpret — no value change). The caller bitcasts the
                        // i64 back to f64 to recover the key. Keeps every lambda's
                        // ABI i64-return without an f64-specific function type.
                        BasicValueEnum::FloatValue(fv) => {
                            self.ir.builder.build_bitcast(fv, i64_ty, "lam_ret_f2i").unwrap()
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
            // Size the env buffer by the ACTUAL struct (its fields now have their
            // real types, so a str/ptr capture is wider than 8 bytes). `size_of`
            // returns an i64 including alignment padding — exactly what the
            // `build_struct_gep` field offsets below assume.
            let env_size = env_struct_ty
                .size_of()
                .unwrap_or_else(|| i64_ty.const_int((n_captures * 8) as u64, false));
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
        // Element LLVM type: from a named local's tracked Slice type, OR — for a
        // non-Ident receiver like a chained index `b[1][0]` — from the inferred
        // sem-type of the receiver expression (e.g. arr_chunk → Slice(Slice)).
        let elem_llvm_ty = if let ast::Expr::Ident(n) = receiver {
            self.local_types.get(n.as_str()).and_then(|ty| {
                if let Type::Slice(inner) = ty { self.llvm_type(inner) } else { None }
            })
        } else {
            self.infer_expr_sem_type(receiver).and_then(|ty| {
                if let Type::Slice(inner) = ty { self.llvm_type(&inner) } else { None }
            })
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
        // Bounds check (I-9 + memory safety): trap on idx<0 || idx>=len before
        // the load, matching the interpreter. Extract len (field 0) first.
        let len_field_ptr = self.ir.builder
            .build_struct_gep(slice_ty, slice_alloca, 0, "lenptr")
            .unwrap();
        let len_val = self.ir.builder
            .build_load(i64_ty, len_field_ptr, "lenval")
            .unwrap()
            .into_int_value();
        self.emit_bounds_guard(idx_int, len_val);
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
            // Panic with the SAME message the interpreter prints (e.g.
            // "arr_max_i64: array is empty") via __axon_msg_panic (exit 101).
            // Previously this called the bare C exit(101) with NO message, so
            // native diverged from the interpreter's stderr text.
            let msg = match &op {
                ArrReduce::Extreme { is_max: true } | ArrReduce::ArgExtreme { is_max: true } => {
                    if matches!(op, ArrReduce::ArgExtreme { .. }) { "arr_argmax_i64: array is empty" }
                    else { "arr_max_i64: array is empty" }
                }
                _ => {
                    if matches!(op, ArrReduce::ArgExtreme { .. }) { "arr_argmin_i64: array is empty" }
                    else { "arr_min_i64: array is empty" }
                }
            };
            if let Some(panic_fn) = self.ir.module.get_function("__axon_msg_panic") {
                let g = build_wrappers::w_global_string_ptr(&self.ir.builder, msg, "arr_empty_msg");
                let len = i64_ty.const_int(msg.len() as u64, false);
                build_wrappers::w_call(&self.ir.builder, panic_fn, &[g.into(), len.into()], "");
            } else if let Some(exit_fn) = self.ir.module.get_function("exit") {
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
            ArrReduce::Sum | ArrReduce::Mean | ArrReduce::Contains(_) | ArrReduce::IndexOf(_) => 0,
            ArrReduce::Extreme { is_max: true } | ArrReduce::ArgExtreme { is_max: true } => i64::MIN as u64,
            ArrReduce::Extreme { is_max: false } | ArrReduce::ArgExtreme { is_max: false } => i64::MAX as u64,
        };
        build_wrappers::w_store(&self.ir.builder, acc_slot, i64_ty.const_int(init, false).into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "arr_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        // For argmax/argmin: track the best element's INDEX (acc holds its value).
        // For IndexOf: holds the first-match index, init -1 (not found).
        let best_idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "arr_bidx");
        let best_idx_init = match &op {
            ArrReduce::IndexOf(_) => i64_ty.const_int(-1i64 as u64, true),
            _ => i64_ty.const_zero(),
        };
        build_wrappers::w_store(&self.ir.builder, best_idx_slot, best_idx_init.into());

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
                // SATURATING add — matches the interpreter's saturating_add
                // accumulator. Raw wrapping add silently produced a wrong total
                // on i64 overflow (I-9).
                let nacc = build_wrappers::w_int_add_sat(&self.ir.builder, acc, elem, "arr_na");
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
            ArrReduce::IndexOf(needle) => {
                // Record i_cur the FIRST time elem == needle; leave it otherwise.
                // best_idx starts at -1, so `best == -1` means "not yet found":
                // update = (elem == needle) && (best == -1).
                let eq = build_wrappers::w_int_compare(
                    &self.ir.builder, inkwell::IntPredicate::EQ, elem, *needle, "arr_ioeq");
                let cur_best = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_idx_slot, "arr_iocb").into_int_value();
                let not_found = build_wrappers::w_int_compare(
                    &self.ir.builder, inkwell::IntPredicate::EQ, cur_best, i64_ty.const_int(-1i64 as u64, true), "arr_ionf");
                let do_set = self.ir.builder.build_and(eq, not_found, "arr_ioset").unwrap();
                let nbest = self.ir.builder.build_select(do_set, i_cur, cur_best, "arr_ioidx").unwrap().into_int_value();
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
            ArrReduce::ArgExtreme { .. } | ArrReduce::IndexOf(_) => {
                // Return the tracked best index (ArgExtreme: best max/min;
                // IndexOf: first-match index, or the -1 sentinel if none).
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

    /// arr_push(&a, x) → a fresh `[i64]` = a ++ [x] (copy semantics, input
    /// untouched). Malloc (len+1)*8, copy the `len` source elements, write `x`
    /// at index `len`, return the {len+1, dst} slice. Pure IR (native + wasm).
    fn emit_arr_i64_push(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        x: inkwell::values::IntValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ps_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "ps_lenp").unwrap(),
            "ps_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "ps_datp").unwrap(),
            "ps_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "ps_srci");

        // new_len = len + 1; malloc new_len * 8.
        let one = i64_ty.const_int(1, false);
        let new_len = build_wrappers::w_int_add(&self.ir.builder, len, one, "ps_nlen");
        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, new_len, eight, "ps_bytes");
        let dst_raw = self.emit_malloc(total, "ps_dst");
        let dst_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "ps_dsti");

        // for i in 0..len: dst[i] = src[i]
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ps_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "ps.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "ps.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "ps.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "ps_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "ps_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "ps_sp").unwrap() };
        let v = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "ps_v");
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[i_cur], "ps_dp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, dp, v);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, one, "ps_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // dst[len] = x; assemble + return the {new_len, dst} slice.
        self.ir.builder.position_at_end(exit_bb);
        let tailp = unsafe { self.ir.builder.build_gep(i64_ty, dst_i64, &[len], "ps_tp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, tailp, x.into());
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ps_out");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "ps_olen").unwrap(),
            new_len.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "ps_di8");
        build_wrappers::w_store(
            &self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "ps_optr").unwrap(),
            dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "ps_res"))
    }

    /// arr_max_by / arr_min_by(&a, key_fn) → the i64 ELEMENT that maximizes
    /// (resp. minimizes) the numeric key. `key_fn` returns f64, transported
    /// through the i64 lambda ABI as bitcast bits; we bitcast back to f64 to
    /// compare. STRICT compare so the FIRST best element wins ties (interp
    /// parity). Empty array → exit(101), matching the interpreter's panic.
    fn emit_arr_i64_max_by(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        is_max: bool,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let f64_ty = self.ir.context.f64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "mb_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "mb_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "mb_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "mb_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(
            &self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "mb_lenp").unwrap(),
            "mb_len").into_int_value();
        let src_raw = build_wrappers::w_load(
            &self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "mb_datp").unwrap(),
            "mb_dat").into_pointer_value();
        let src_i64 = build_wrappers::w_pointer_cast(
            &self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "mb_srci");

        // Empty → exit(101), matching the interpreter panic.
        let empty_bb = self.ir.context.append_basic_block(fn_val, "mb.empty");
        let nonempty_bb = self.ir.context.append_basic_block(fn_val, "mb.nonempty");
        let is_empty = build_wrappers::w_int_compare(
            &self.ir.builder, inkwell::IntPredicate::EQ, len, i64_ty.const_zero(), "mb_isempty");
        build_wrappers::w_cond_br(&self.ir.builder, is_empty, empty_bb, nonempty_bb);
        self.ir.builder.position_at_end(empty_bb);
        if let Some(exit_fn) = self.ir.module.get_function("exit") {
            let code = self.ir.context.i32_type().const_int(101, false);
            build_wrappers::w_call(&self.ir.builder, exit_fn, &[code.into()], "");
        }
        self.ir.builder.build_unreachable().unwrap();
        self.ir.builder.position_at_end(nonempty_bb);

        // Helper to compute the f64 key of element at index `i`.
        // best_val (the element) + best_key (its f64 key); init from element 0.
        let best_val_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "mb_bv");
        let best_key_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "mb_bk");
        let e0p = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i64_ty.const_zero()], "mb_e0p").unwrap() };
        let e0 = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), e0p, "mb_e0").into_int_value();
        let k0_i = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), e0.into()], "mb_k0c")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        let k0 = self.ir.builder.build_bitcast(k0_i, f64_ty, "mb_k0").unwrap().into_float_value();
        build_wrappers::w_store(&self.ir.builder, best_val_slot, e0.into());
        build_wrappers::w_store(&self.ir.builder, best_key_slot, k0.into());

        // for i in 1..len: k = bitcast(key_fn(elem)); if strictly better, update.
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "mb_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_int(1, false).into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "mb.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "mb.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "mb.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "mb_ic").into_int_value();
        let in_range = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "mb_inr");
        build_wrappers::w_cond_br(&self.ir.builder, in_range, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ep = unsafe { self.ir.builder.build_gep(i64_ty, src_i64, &[i_cur], "mb_ep").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ep, "mb_e").into_int_value();
        let k_i = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), elem.into()], "mb_kc")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        let k = self.ir.builder.build_bitcast(k_i, f64_ty, "mb_k").unwrap().into_float_value();
        let best_key = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), best_key_slot, "mb_bkl").into_float_value();
        // STRICT: max uses OGT, min uses OLT → first best wins ties (interp parity).
        let pred = if is_max { inkwell::FloatPredicate::OGT } else { inkwell::FloatPredicate::OLT };
        let better = self.ir.builder.build_float_compare(pred, k, best_key, "mb_better").unwrap();
        let cur_val = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_val_slot, "mb_cv").into_int_value();
        let new_val = self.ir.builder.build_select(better, elem, cur_val, "mb_nv").unwrap().into_int_value();
        let new_key = self.ir.builder.build_select(better, k, best_key, "mb_nk").unwrap().into_float_value();
        build_wrappers::w_store(&self.ir.builder, best_val_slot, new_val.into());
        build_wrappers::w_store(&self.ir.builder, best_key_slot, new_key.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "mb_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        Some(build_wrappers::w_load(&self.ir.builder, i64_ty.into(), best_val_slot, "mb_res"))
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
        mode: ClosureMode,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let is_map = mode == ClosureMode::Map;

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
        // take_while/drop_while: a one-way "transition done" flag. For TakeWhile
        // it latches to 1 at the first failing element (stop keeping). For
        // DropWhile it latches to 1 at the first failing element (start keeping).
        let done_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "cl_done");
        build_wrappers::w_store(&self.ir.builder, done_slot, i64_ty.const_zero().into());

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
            // r is the i64-widened predicate result; pred = (r != 0).
            let pred = build_wrappers::w_int_compare(
                &self.ir.builder, inkwell::IntPredicate::NE, r, i64_ty.const_zero(), "cl_pred");
            // Decide `keep` (whether to append elem) per mode:
            //   Filter:    keep = pred
            //   TakeWhile: keep = !done && pred; latch done when !pred
            //   DropWhile: keep = done || !pred; latch done when !pred
            let keep = match mode {
                ClosureMode::Filter => pred,
                ClosureMode::TakeWhile | ClosureMode::DropWhile => {
                    let done = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), done_slot, "cl_dn").into_int_value();
                    let done_b = build_wrappers::w_int_compare(
                        &self.ir.builder, inkwell::IntPredicate::NE, done, i64_ty.const_zero(), "cl_dnb");
                    // Latch done on the first failing element (!pred): done |= !pred.
                    let not_pred = self.ir.builder.build_not(pred, "cl_npred").unwrap();
                    let new_done = self.ir.builder.build_or(done_b, not_pred, "cl_ndn").unwrap();
                    let new_done_i = build_wrappers::w_int_z_extend(&self.ir.builder, new_done, i64_ty, "cl_ndni");
                    build_wrappers::w_store(&self.ir.builder, done_slot, new_done_i.into());
                    if mode == ClosureMode::TakeWhile {
                        // keep = pred && !was-already-done (use the PRE-update done)
                        let not_done = self.ir.builder.build_not(done_b, "cl_ntd").unwrap();
                        self.ir.builder.build_and(pred, not_done, "cl_tw").unwrap()
                    } else {
                        // DropWhile: keep = new_done (true once we've stopped
                        // dropping, i.e. at and after the first failing element).
                        new_done
                    }
                }
                ClosureMode::Map => unreachable!(),
            };
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

    /// arr_partition(&a, pred) → ([yes], [no]): a `{slice, slice}` tuple of two
    /// i64 slices. Per element, the predicate lambda routes it to the yes-buffer
    /// or no-buffer (both over-allocated to len). Returns a 2-slice tuple.
    fn emit_arr_i64_partition(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let pair_ty = self.ir.context.struct_type(&[slice_ty.into(), slice_ty.into()], false);
        let one = i64_ty.const_int(1, false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "pt_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "pt_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "pt_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "pt_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "pt_lp").unwrap(), "pt_len").into_int_value();
        let src_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "pt_dp").unwrap(), "pt_dat").into_pointer_value();
        let src = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "pt_si");

        let eight = i64_ty.const_int(8, false);
        let bytes = build_wrappers::w_int_mul(&self.ir.builder, len, eight, "pt_bytes");
        let yes_raw = self.emit_malloc(bytes, "pt_yes");
        let yes = build_wrappers::w_pointer_cast(&self.ir.builder, yes_raw, i64_ty.ptr_type(AddressSpace::default()), "pt_yi");
        let no_raw = self.emit_malloc(bytes, "pt_no");
        let no = build_wrappers::w_pointer_cast(&self.ir.builder, no_raw, i64_ty.ptr_type(AddressSpace::default()), "pt_ni");
        let yc = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pt_yc");
        build_wrappers::w_store(&self.ir.builder, yc, i64_ty.const_zero().into());
        let nc = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pt_nc");
        build_wrappers::w_store(&self.ir.builder, nc, i64_ty.const_zero().into());

        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pt_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "pt.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "pt.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "pt.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);
        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "pt_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "pt_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);
        self.ir.builder.position_at_end(body_bb);
        let ep = unsafe { self.ir.builder.build_gep(i64_ty, src, &[i_cur], "pt_ep").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ep, "pt_e").into_int_value();
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), elem.into()], "pt_call")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        let yes_bb = self.ir.context.append_basic_block(fn_val, "pt.yes");
        let no_bb = self.ir.context.append_basic_block(fn_val, "pt.no");
        let cont_bb = self.ir.context.append_basic_block(fn_val, "pt.cont");
        let truthy = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, r, i64_ty.const_zero(), "pt_t");
        build_wrappers::w_cond_br(&self.ir.builder, truthy, yes_bb, no_bb);
        self.ir.builder.position_at_end(yes_bb);
        let yw = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), yc, "pt_yw").into_int_value();
        let yp = unsafe { self.ir.builder.build_gep(i64_ty, yes, &[yw], "pt_yp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, yp, elem.into());
        build_wrappers::w_store(&self.ir.builder, yc, build_wrappers::w_int_add(&self.ir.builder, yw, one, "pt_yw1").into());
        build_wrappers::w_br(&self.ir.builder, cont_bb);
        self.ir.builder.position_at_end(no_bb);
        let nw = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), nc, "pt_nw").into_int_value();
        let np = unsafe { self.ir.builder.build_gep(i64_ty, no, &[nw], "pt_np").unwrap() };
        build_wrappers::w_store(&self.ir.builder, np, elem.into());
        build_wrappers::w_store(&self.ir.builder, nc, build_wrappers::w_int_add(&self.ir.builder, nw, one, "pt_nw1").into());
        build_wrappers::w_br(&self.ir.builder, cont_bb);
        self.ir.builder.position_at_end(cont_bb);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, one, "pt_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // exit: build the ({yc, yes}, {nc, no}) pair tuple.
        self.ir.builder.position_at_end(exit_bb);
        let yfin = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), yc, "pt_yf").into_int_value();
        let nfin = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), nc, "pt_nf").into_int_value();
        let pair = build_wrappers::w_alloca(&self.ir.builder, pair_ty.into(), "pt_pair");
        // pair.0 = {yfin, yes_i8}
        let p0 = self.ir.builder.build_struct_gep(pair_ty, pair, 0, "pt_p0").unwrap();
        build_wrappers::w_store(&self.ir.builder, self.ir.builder.build_struct_gep(slice_ty, p0, 0, "pt_p0l").unwrap(), yfin.into());
        let yes_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, yes_raw, ptr_ty, "pt_yi8");
        build_wrappers::w_store(&self.ir.builder, self.ir.builder.build_struct_gep(slice_ty, p0, 1, "pt_p0p").unwrap(), yes_i8.into());
        // pair.1 = {nfin, no_i8}
        let p1 = self.ir.builder.build_struct_gep(pair_ty, pair, 1, "pt_p1").unwrap();
        build_wrappers::w_store(&self.ir.builder, self.ir.builder.build_struct_gep(slice_ty, p1, 0, "pt_p1l").unwrap(), nfin.into());
        let no_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, no_raw, ptr_ty, "pt_ni8");
        build_wrappers::w_store(&self.ir.builder, self.ir.builder.build_struct_gep(slice_ty, p1, 1, "pt_p1p").unwrap(), no_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, pair_ty.into(), pair, "pt_res"))
    }

    /// arr_enumerate(&a) → [(i64 idx, i64 val)]. Result is a slice whose
    /// elements are `{i64, i64}` tuples (16-byte stride). dst[i] = (i, src[i]).
    /// Pure IR + malloc. The element type is the tuple, so indexing the result
    /// (`b[k].0`) flows through the standard tuple field-access codegen.
    fn emit_arr_i64_enumerate(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let tup_ty = self.ir.context.struct_type(&[i64_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "en_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "en_lp").unwrap(), "en_len").into_int_value();
        let src_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "en_dp").unwrap(), "en_dat").into_pointer_value();
        let src = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "en_si");

        // malloc len * sizeof({i64,i64}) = len * 16.
        let sixteen = i64_ty.const_int(16, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, len, sixteen, "en_bytes");
        let dst_raw = self.emit_malloc(total, "en_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, tup_ty.ptr_type(AddressSpace::default()), "en_dt");

        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "en_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "en.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "en.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "en.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "en_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "en_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src, &[i_cur], "en_sp").unwrap() };
        let v = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "en_v").into_int_value();
        // dst[i] = { i_cur, v } — GEP the tuple slot, store both fields.
        let tp = unsafe { self.ir.builder.build_gep(tup_ty, dst, &[i_cur], "en_tp").unwrap() };
        let f0 = self.ir.builder.build_struct_gep(tup_ty, tp, 0, "en_f0").unwrap();
        build_wrappers::w_store(&self.ir.builder, f0, i_cur.into());
        let f1 = self.ir.builder.build_struct_gep(tup_ty, tp, 1, "en_f1").unwrap();
        build_wrappers::w_store(&self.ir.builder, f1, v.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "en_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "en_out");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "en_ol").unwrap(), len.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "en_di8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "en_op").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "en_res"))
    }

    /// arr_chunk(&a, n) — [i64] → [[i64]] in chunks of n (last shorter). n<=0 →
    /// exit(101). Outer = ceil(len/n) slice structs; each chunk mallocs its own
    /// i64 buffer + copies its range. Nested allocation. Pure IR + malloc.
    fn emit_arr_i64_chunk(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        n: inkwell::values::IntValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let one = i64_ty.const_int(1, false);
        let eight = i64_ty.const_int(8, false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ck_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "ck_lp").unwrap(), "ck_len").into_int_value();
        let src_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "ck_dp").unwrap(), "ck_dat").into_pointer_value();
        let src = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "ck_si");

        // n <= 0 → exit(101) panic.
        let bad_bb = self.ir.context.append_basic_block(fn_val, "ck.bad");
        let ok_bb = self.ir.context.append_basic_block(fn_val, "ck.ok");
        let npos = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGT, n, i64_ty.const_zero(), "ck_np");
        build_wrappers::w_cond_br(&self.ir.builder, npos, ok_bb, bad_bb);
        self.ir.builder.position_at_end(bad_bb);
        if let Some(exit_fn) = self.ir.module.get_function("exit") {
            let code = self.ir.context.i32_type().const_int(101, false);
            build_wrappers::w_call(&self.ir.builder, exit_fn, &[code.into()], "");
        }
        self.ir.builder.build_unreachable().unwrap();
        self.ir.builder.position_at_end(ok_bb);

        // outer count = ceil(len / n) = (len + n - 1) / n.
        let lpn = build_wrappers::w_int_add(&self.ir.builder, len, n, "ck_lpn");
        let lpnm1 = build_wrappers::w_int_sub(&self.ir.builder, lpn, one, "ck_lpnm1");
        let ocount = self.ir.builder.build_int_signed_div(lpnm1, n, "ck_oc").unwrap();
        // malloc outer = ocount * 16 (slice structs).
        let sixteen = i64_ty.const_int(16, false);
        let obytes = build_wrappers::w_int_mul(&self.ir.builder, ocount, sixteen, "ck_obytes");
        let outer_raw = self.emit_malloc(obytes, "ck_outer");
        let outer = build_wrappers::w_pointer_cast(&self.ir.builder, outer_raw, slice_ty.ptr_type(AddressSpace::default()), "ck_od");

        // for c in 0..ocount: start = c*n; clen = min(n, len-start); malloc
        // clen*8; copy src[start..start+clen]; outer[c] = {clen, buf}.
        let cs = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ck_c");
        build_wrappers::w_store(&self.ir.builder, cs, i64_ty.const_zero().into());
        let oc = self.ir.context.append_basic_block(fn_val, "ck.oc");
        let ob = self.ir.context.append_basic_block(fn_val, "ck.ob");
        let oe = self.ir.context.append_basic_block(fn_val, "ck.oe");
        build_wrappers::w_br(&self.ir.builder, oc);
        self.ir.builder.position_at_end(oc);
        let cc = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), cs, "ck_cc").into_int_value();
        let og = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, cc, ocount, "ck_og");
        build_wrappers::w_cond_br(&self.ir.builder, og, ob, oe);
        self.ir.builder.position_at_end(ob);
        let start = build_wrappers::w_int_mul(&self.ir.builder, cc, n, "ck_start");
        let rem = build_wrappers::w_int_sub(&self.ir.builder, len, start, "ck_rem");
        let n_le_rem = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, n, rem, "ck_nlr");
        let clen = self.ir.builder.build_select(n_le_rem, n, rem, "ck_clen").unwrap().into_int_value();
        let cbytes = build_wrappers::w_int_mul(&self.ir.builder, clen, eight, "ck_cbytes");
        let cbuf_raw = self.emit_malloc(cbytes, "ck_cbuf");
        let cbuf = build_wrappers::w_pointer_cast(&self.ir.builder, cbuf_raw, i64_ty.ptr_type(AddressSpace::default()), "ck_cb");
        // inner copy j in 0..clen: cbuf[j] = src[start+j].
        let js = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ck_j");
        build_wrappers::w_store(&self.ir.builder, js, i64_ty.const_zero().into());
        let jc = self.ir.context.append_basic_block(fn_val, "ck.jc");
        let jb = self.ir.context.append_basic_block(fn_val, "ck.jb");
        let je = self.ir.context.append_basic_block(fn_val, "ck.je");
        build_wrappers::w_br(&self.ir.builder, jc);
        self.ir.builder.position_at_end(jc);
        let jcur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), js, "ck_jcur").into_int_value();
        let jg = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, jcur, clen, "ck_jg");
        build_wrappers::w_cond_br(&self.ir.builder, jg, jb, je);
        self.ir.builder.position_at_end(jb);
        let sidx = build_wrappers::w_int_add(&self.ir.builder, start, jcur, "ck_sidx");
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, src, &[sidx], "ck_sp").unwrap() };
        let sv = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "ck_sv");
        let cp = unsafe { self.ir.builder.build_gep(i64_ty, cbuf, &[jcur], "ck_cp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, cp, sv);
        let jn = build_wrappers::w_int_add(&self.ir.builder, jcur, one, "ck_jn");
        build_wrappers::w_store(&self.ir.builder, js, jn.into());
        build_wrappers::w_br(&self.ir.builder, jc);
        self.ir.builder.position_at_end(je);
        // outer[c] = { clen, cbuf as i8* }.
        let op = unsafe { self.ir.builder.build_gep(slice_ty, outer, &[cc], "ck_op").unwrap() };
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, op, 0, "ck_olp").unwrap(), clen.into());
        let cbuf_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, cbuf_raw, ptr_ty, "ck_cbi8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, op, 1, "ck_odp").unwrap(), cbuf_i8.into());
        let cn = build_wrappers::w_int_add(&self.ir.builder, cc, one, "ck_cn");
        build_wrappers::w_store(&self.ir.builder, cs, cn.into());
        build_wrappers::w_br(&self.ir.builder, oc);
        self.ir.builder.position_at_end(oe);

        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ck_out");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "ck_ol2").unwrap(), ocount.into());
        let outer_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, outer_raw, ptr_ty, "ck_oi8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "ck_op2").unwrap(), outer_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "ck_res"))
    }

    /// arr_flatten(&a) — [[i64]] → [i64]. The outer slice's elements are
    /// `{i64 len, i8* ptr}` slice structs (16-byte stride). Two passes: sum all
    /// inner lengths → total; malloc total*8; copy each inner slice's i64s into
    /// the destination, advancing a write cursor. Pure IR + malloc.
    fn emit_arr_i64_flatten(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        // Unpack outer slice: len = #inner slices, data → array of slice structs.
        let o_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ft_o");
        build_wrappers::w_store(&self.ir.builder, o_alloca, slice_val);
        let olen = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, o_alloca, 0, "ft_olp").unwrap(), "ft_olen").into_int_value();
        let odata_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, o_alloca, 1, "ft_odp").unwrap(), "ft_odat").into_pointer_value();
        // Outer data as a pointer to slice structs (16-byte stride).
        let odata = build_wrappers::w_pointer_cast(&self.ir.builder, odata_raw, slice_ty.ptr_type(AddressSpace::default()), "ft_od");

        // Pass 1: total = Σ inner_len.
        let total_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ft_total");
        build_wrappers::w_store(&self.ir.builder, total_slot, i64_ty.const_zero().into());
        let p1i = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ft_p1i");
        build_wrappers::w_store(&self.ir.builder, p1i, i64_ty.const_zero().into());
        let c1 = self.ir.context.append_basic_block(fn_val, "ft.c1");
        let b1 = self.ir.context.append_basic_block(fn_val, "ft.b1");
        let e1 = self.ir.context.append_basic_block(fn_val, "ft.e1");
        build_wrappers::w_br(&self.ir.builder, c1);
        self.ir.builder.position_at_end(c1);
        let i1 = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), p1i, "ft_i1").into_int_value();
        let g1 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i1, olen, "ft_g1");
        build_wrappers::w_cond_br(&self.ir.builder, g1, b1, e1);
        self.ir.builder.position_at_end(b1);
        let inner_p = unsafe { self.ir.builder.build_gep(slice_ty, odata, &[i1], "ft_ip").unwrap() };
        let il = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, inner_p, 0, "ft_ilp").unwrap(), "ft_il").into_int_value();
        let cur_t = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), total_slot, "ft_ct").into_int_value();
        let nt = build_wrappers::w_int_add(&self.ir.builder, cur_t, il, "ft_nt");
        build_wrappers::w_store(&self.ir.builder, total_slot, nt.into());
        let n1 = build_wrappers::w_int_add(&self.ir.builder, i1, i64_ty.const_int(1, false), "ft_n1");
        build_wrappers::w_store(&self.ir.builder, p1i, n1.into());
        build_wrappers::w_br(&self.ir.builder, c1);
        self.ir.builder.position_at_end(e1);
        let total = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), total_slot, "ft_totf").into_int_value();

        // malloc total*8.
        let eight = i64_ty.const_int(8, false);
        let bytes = build_wrappers::w_int_mul(&self.ir.builder, total, eight, "ft_bytes");
        let dst_raw = self.emit_malloc(bytes, "ft_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "ft_di");

        // Pass 2: copy, with a write cursor `w`.
        let w_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ft_w");
        build_wrappers::w_store(&self.ir.builder, w_slot, i64_ty.const_zero().into());
        let p2i = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ft_p2i");
        build_wrappers::w_store(&self.ir.builder, p2i, i64_ty.const_zero().into());
        let c2 = self.ir.context.append_basic_block(fn_val, "ft.c2");
        let b2 = self.ir.context.append_basic_block(fn_val, "ft.b2");
        let e2 = self.ir.context.append_basic_block(fn_val, "ft.e2");
        build_wrappers::w_br(&self.ir.builder, c2);
        self.ir.builder.position_at_end(c2);
        let i2 = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), p2i, "ft_i2").into_int_value();
        let g2 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i2, olen, "ft_g2");
        build_wrappers::w_cond_br(&self.ir.builder, g2, b2, e2);
        self.ir.builder.position_at_end(b2);
        // Inner slice i2: len + data ptr.
        let inp = unsafe { self.ir.builder.build_gep(slice_ty, odata, &[i2], "ft_inp").unwrap() };
        let inlen = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, inp, 0, "ft_inlp").unwrap(), "ft_inlen").into_int_value();
        let indat_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, inp, 1, "ft_indp").unwrap(), "ft_indat").into_pointer_value();
        let indat = build_wrappers::w_pointer_cast(&self.ir.builder, indat_raw, i64_ty.ptr_type(AddressSpace::default()), "ft_ind");
        // Inner copy loop j in 0..inlen: dst[w]=inner[j]; w++.
        let js = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ft_j");
        build_wrappers::w_store(&self.ir.builder, js, i64_ty.const_zero().into());
        let jc = self.ir.context.append_basic_block(fn_val, "ft.jc");
        let jb = self.ir.context.append_basic_block(fn_val, "ft.jb");
        let je = self.ir.context.append_basic_block(fn_val, "ft.je");
        build_wrappers::w_br(&self.ir.builder, jc);
        self.ir.builder.position_at_end(jc);
        let jcur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), js, "ft_jc").into_int_value();
        let jg = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, jcur, inlen, "ft_jg");
        build_wrappers::w_cond_br(&self.ir.builder, jg, jb, je);
        self.ir.builder.position_at_end(jb);
        let sp = unsafe { self.ir.builder.build_gep(i64_ty, indat, &[jcur], "ft_sp").unwrap() };
        let sv = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), sp, "ft_sv");
        let wcur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), w_slot, "ft_wc").into_int_value();
        let dp = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[wcur], "ft_dp2").unwrap() };
        build_wrappers::w_store(&self.ir.builder, dp, sv);
        let wn = build_wrappers::w_int_add(&self.ir.builder, wcur, i64_ty.const_int(1, false), "ft_wn");
        build_wrappers::w_store(&self.ir.builder, w_slot, wn.into());
        let jn = build_wrappers::w_int_add(&self.ir.builder, jcur, i64_ty.const_int(1, false), "ft_jn");
        build_wrappers::w_store(&self.ir.builder, js, jn.into());
        build_wrappers::w_br(&self.ir.builder, jc);
        self.ir.builder.position_at_end(je);
        let n2 = build_wrappers::w_int_add(&self.ir.builder, i2, i64_ty.const_int(1, false), "ft_n2");
        build_wrappers::w_store(&self.ir.builder, p2i, n2.into());
        build_wrappers::w_br(&self.ir.builder, c2);
        self.ir.builder.position_at_end(e2);

        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ft_out");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "ft_ol").unwrap(), total.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "ft_di8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "ft_op").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "ft_res"))
    }

    /// arr_zip(a, b) → [(a[i], b[i])] for i in 0..min(len_a, len_b). Result is
    /// a slice of {i64, i64} tuples (16-byte stride). Pure IR + malloc.
    fn emit_arr_i64_zip(
        &mut self,
        a_slice: BasicValueEnum<'ctx>,
        b_slice: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let tup_ty = self.ir.context.struct_type(&[i64_ty.into(), i64_ty.into()], false);

        let unpack = |slf: &mut Self, sv: BasicValueEnum<'ctx>, tag: &str| {
            let al = build_wrappers::w_alloca(&slf.ir.builder, slice_ty.into(), tag);
            build_wrappers::w_store(&slf.ir.builder, al, sv);
            let l = build_wrappers::w_load(&slf.ir.builder, i64_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 0, "zp_lp").unwrap(), "zp_l").into_int_value();
            let d = build_wrappers::w_load(&slf.ir.builder, ptr_ty.into(),
                slf.ir.builder.build_struct_gep(slice_ty, al, 1, "zp_dp").unwrap(), "zp_d").into_pointer_value();
            let di = build_wrappers::w_pointer_cast(&slf.ir.builder, d, i64_ty.ptr_type(AddressSpace::default()), "zp_di");
            (l, di)
        };
        let (a_len, a_data) = unpack(self, a_slice, "zp_a");
        let (b_len, b_data) = unpack(self, b_slice, "zp_b");
        let a_le = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, a_len, b_len, "zp_alt");
        let n = self.ir.builder.build_select(a_le, a_len, b_len, "zp_n").unwrap().into_int_value();

        let sixteen = i64_ty.const_int(16, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, n, sixteen, "zp_bytes");
        let dst_raw = self.emit_malloc(total, "zp_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, tup_ty.ptr_type(AddressSpace::default()), "zp_dt");

        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "zp_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());
        let cond_bb = self.ir.context.append_basic_block(fn_val, "zp.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "zp.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "zp.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "zp_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, n, "zp_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ap = unsafe { self.ir.builder.build_gep(i64_ty, a_data, &[i_cur], "zp_ap").unwrap() };
        let av = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ap, "zp_av").into_int_value();
        let bp = unsafe { self.ir.builder.build_gep(i64_ty, b_data, &[i_cur], "zp_bp").unwrap() };
        let bv = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), bp, "zp_bv").into_int_value();
        let tp = unsafe { self.ir.builder.build_gep(tup_ty, dst, &[i_cur], "zp_tp").unwrap() };
        let f0 = self.ir.builder.build_struct_gep(tup_ty, tp, 0, "zp_f0").unwrap();
        build_wrappers::w_store(&self.ir.builder, f0, av.into());
        let f1 = self.ir.builder.build_struct_gep(tup_ty, tp, 1, "zp_f1").unwrap();
        build_wrappers::w_store(&self.ir.builder, f1, bv.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "zp_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(exit_bb);
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "zp_out");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "zp_ol").unwrap(), n.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "zp_di8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "zp_op").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "zp_res"))
    }

    /// arr_find(&a, pred) → Option<i64>: first element where the predicate
    /// lambda returns truthy (Some), else None. Loops the whole slice tracking
    /// a found-flag + found-value (no short-circuit — observably identical for
    /// pure predicates), then builds the Option via emit_option.
    fn emit_arr_i64_find(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        lam: inkwell::values::StructValue<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "fd2_fn").into_pointer_value();
        let env_ptr = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "fd2_env").into_pointer_value();
        let fn_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "fd2_fp");
        let indirect_ty = i64_ty.fn_type(&[ptr_ty.into(), i64_ty.into()], false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "fd2_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "fd2_lp").unwrap(), "fd2_len").into_int_value();
        let src_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "fd2_dp").unwrap(), "fd2_dat").into_pointer_value();
        let src = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "fd2_si");

        // found flag (0/1) + found value. Keep the FIRST match: only update when
        // not-yet-found AND predicate true.
        let found_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "fd2_f");
        build_wrappers::w_store(&self.ir.builder, found_slot, i64_ty.const_zero().into());
        let val_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "fd2_v");
        build_wrappers::w_store(&self.ir.builder, val_slot, i64_ty.const_zero().into());
        let idx_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "fd2_i");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i64_ty.const_zero().into());

        let cond_bb = self.ir.context.append_basic_block(fn_val, "fd2.cond");
        let body_bb = self.ir.context.append_basic_block(fn_val, "fd2.body");
        let exit_bb = self.ir.context.append_basic_block(fn_val, "fd2.exit");
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        self.ir.builder.position_at_end(cond_bb);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), idx_slot, "fd2_ic").into_int_value();
        let go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "fd2_go");
        build_wrappers::w_cond_br(&self.ir.builder, go, body_bb, exit_bb);

        self.ir.builder.position_at_end(body_bb);
        let ep = unsafe { self.ir.builder.build_gep(i64_ty, src, &[i_cur], "fd2_ep").unwrap() };
        let elem = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), ep, "fd2_e").into_int_value();
        let r = self.ir.builder
            .build_indirect_call(indirect_ty, fn_ptr, &[env_ptr.into(), elem.into()], "fd2_call")
            .unwrap().try_as_basic_value().left()?.into_int_value();
        let truthy = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, r, i64_ty.const_zero(), "fd2_t");
        let cur_found = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), found_slot, "fd2_cf").into_int_value();
        let not_found = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::EQ, cur_found, i64_ty.const_zero(), "fd2_nf");
        // take = truthy && not-yet-found
        let take = build_wrappers::w_and(&self.ir.builder, truthy, not_found, "fd2_take");
        // val = take ? elem : val ; found = take ? 1 : found
        let cur_val = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), val_slot, "fd2_cv").into_int_value();
        let nval = self.ir.builder.build_select(take, elem, cur_val, "fd2_nv").unwrap().into_int_value();
        build_wrappers::w_store(&self.ir.builder, val_slot, nval.into());
        let take64 = build_wrappers::w_int_z_extend(&self.ir.builder, take, i64_ty, "fd2_t64");
        let nfound = build_wrappers::w_or(&self.ir.builder, cur_found, take64, "fd2_nfound");
        build_wrappers::w_store(&self.ir.builder, found_slot, nfound.into());
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, i64_ty.const_int(1, false), "fd2_in");
        build_wrappers::w_store(&self.ir.builder, idx_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, cond_bb);

        // exit: build Some(val) if found, else None — via a select on the
        // Option struct (both branches built, select picks).
        self.ir.builder.position_at_end(exit_bb);
        let found = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), found_slot, "fd2_ff").into_int_value();
        let val = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), val_slot, "fd2_fv");
        let found_i1 = self.ir.builder.build_int_truncate(found, self.ir.context.bool_type(), "fd2_fi1").unwrap();
        let some_v = self.emit_option(Some(val), &Type::I64);
        let none_v = self.emit_option(None, &Type::I64);
        let chosen = self.ir.builder.build_select(found_i1, some_v, none_v, "fd2_opt").unwrap();
        Some(chosen)
    }

    /// arr_unique(&a) — keep the FIRST occurrence of each i64 value. Mallocs a
    /// len-sized buffer; for each src element, linearly scan the already-written
    /// `cnt` entries; if absent, append. O(n²). Result length = cnt.
    fn emit_arr_i64_unique(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
        let one = i64_ty.const_int(1, false);

        let src_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "uq_s");
        build_wrappers::w_store(&self.ir.builder, src_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 0, "uq_lp").unwrap(), "uq_len").into_int_value();
        let src_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, src_alloca, 1, "uq_dp").unwrap(), "uq_dat").into_pointer_value();
        let src = build_wrappers::w_pointer_cast(&self.ir.builder, src_raw, i64_ty.ptr_type(AddressSpace::default()), "uq_si");

        let eight = i64_ty.const_int(8, false);
        let total = build_wrappers::w_int_mul(&self.ir.builder, len, eight, "uq_bytes");
        let dst_raw = self.emit_malloc(total, "uq_dst");
        let dst = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, i64_ty.ptr_type(AddressSpace::default()), "uq_di");
        let cnt_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "uq_cnt");
        build_wrappers::w_store(&self.ir.builder, cnt_slot, i64_ty.const_zero().into());

        // Outer i in 0..len.
        let i_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "uq_i");
        build_wrappers::w_store(&self.ir.builder, i_slot, i64_ty.const_zero().into());
        let o_cond = self.ir.context.append_basic_block(fn_val, "uq.ocond");
        let o_body = self.ir.context.append_basic_block(fn_val, "uq.obody");
        let o_exit = self.ir.context.append_basic_block(fn_val, "uq.oexit");
        build_wrappers::w_br(&self.ir.builder, o_cond);
        self.ir.builder.position_at_end(o_cond);
        let i_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), i_slot, "uq_ic").into_int_value();
        let o_go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i_cur, len, "uq_og");
        build_wrappers::w_cond_br(&self.ir.builder, o_go, o_body, o_exit);

        self.ir.builder.position_at_end(o_body);
        let xp = unsafe { self.ir.builder.build_gep(i64_ty, src, &[i_cur], "uq_xp").unwrap() };
        let x = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), xp, "uq_x").into_int_value();
        let cnt = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), cnt_slot, "uq_c").into_int_value();
        // Inner scan j in 0..cnt: found = any(dst[j] == x). Use a found-slot.
        let found_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "uq_f");
        build_wrappers::w_store(&self.ir.builder, found_slot, i64_ty.const_zero().into());
        let j_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "uq_j");
        build_wrappers::w_store(&self.ir.builder, j_slot, i64_ty.const_zero().into());
        let s_cond = self.ir.context.append_basic_block(fn_val, "uq.scond");
        let s_body = self.ir.context.append_basic_block(fn_val, "uq.sbody");
        let s_exit = self.ir.context.append_basic_block(fn_val, "uq.sexit");
        build_wrappers::w_br(&self.ir.builder, s_cond);
        self.ir.builder.position_at_end(s_cond);
        let j_cur = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), j_slot, "uq_jc").into_int_value();
        let s_go = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, j_cur, cnt, "uq_sg");
        build_wrappers::w_cond_br(&self.ir.builder, s_go, s_body, s_exit);
        self.ir.builder.position_at_end(s_body);
        let djp = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[j_cur], "uq_djp").unwrap() };
        let dj = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), djp, "uq_dj").into_int_value();
        let eq = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::EQ, dj, x, "uq_eq");
        let eq64 = build_wrappers::w_int_z_extend(&self.ir.builder, eq, i64_ty, "uq_eq64");
        let cur_f = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), found_slot, "uq_cf").into_int_value();
        let nf = build_wrappers::w_or(&self.ir.builder, cur_f, eq64, "uq_nf");
        build_wrappers::w_store(&self.ir.builder, found_slot, nf.into());
        let j_next = build_wrappers::w_int_add(&self.ir.builder, j_cur, one, "uq_jn");
        build_wrappers::w_store(&self.ir.builder, j_slot, j_next.into());
        build_wrappers::w_br(&self.ir.builder, s_cond);
        // After scan: if !found → dst[cnt] = x; cnt++.
        self.ir.builder.position_at_end(s_exit);
        let found = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), found_slot, "uq_ff").into_int_value();
        let is_new = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::EQ, found, i64_ty.const_zero(), "uq_new");
        let app_bb = self.ir.context.append_basic_block(fn_val, "uq.app");
        let skip_bb = self.ir.context.append_basic_block(fn_val, "uq.skip");
        build_wrappers::w_cond_br(&self.ir.builder, is_new, app_bb, skip_bb);
        self.ir.builder.position_at_end(app_bb);
        let wp = unsafe { self.ir.builder.build_gep(i64_ty, dst, &[cnt], "uq_wp").unwrap() };
        build_wrappers::w_store(&self.ir.builder, wp, x.into());
        let cnt2 = build_wrappers::w_int_add(&self.ir.builder, cnt, one, "uq_c2");
        build_wrappers::w_store(&self.ir.builder, cnt_slot, cnt2.into());
        build_wrappers::w_br(&self.ir.builder, skip_bb);
        self.ir.builder.position_at_end(skip_bb);
        let i_next = build_wrappers::w_int_add(&self.ir.builder, i_cur, one, "uq_in");
        build_wrappers::w_store(&self.ir.builder, i_slot, i_next.into());
        build_wrappers::w_br(&self.ir.builder, o_cond);

        self.ir.builder.position_at_end(o_exit);
        let final_cnt = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), cnt_slot, "uq_fc").into_int_value();
        let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "uq_out");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 0, "uq_ol").unwrap(), final_cnt.into());
        let dst_i8 = build_wrappers::w_pointer_cast(&self.ir.builder, dst_raw, ptr_ty, "uq_di8");
        build_wrappers::w_store(&self.ir.builder,
            self.ir.builder.build_struct_gep(slice_ty, out, 1, "uq_op").unwrap(), dst_i8.into());
        Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "uq_res"))
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

    /// arr_std_f64(&a) → sample standard deviation. <2 elements → 0.0 (no
    /// spread). Two passes over the f64 slice: Σ → mean; Σ(x-mean)² → var/(n-1);
    /// sqrt. Pure IR + llvm.sqrt.f64 (native + wasm).
    fn emit_arr_f64_std(
        &mut self,
        slice_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let i64_ty = self.ir.context.i64_type();
        let f64_ty = self.ir.context.f64_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);

        let s_alloca = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "sd_s");
        build_wrappers::w_store(&self.ir.builder, s_alloca, slice_val);
        let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, s_alloca, 0, "sd_lp").unwrap(), "sd_len").into_int_value();
        let data_raw = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
            self.ir.builder.build_struct_gep(slice_ty, s_alloca, 1, "sd_dp").unwrap(), "sd_dat").into_pointer_value();
        let data = build_wrappers::w_pointer_cast(&self.ir.builder, data_raw, f64_ty.ptr_type(AddressSpace::default()), "sd_fd");

        // <2 elements → return 0.0 immediately.
        let small_bb = self.ir.context.append_basic_block(fn_val, "sd.small");
        let ok_bb = self.ir.context.append_basic_block(fn_val, "sd.ok");
        let done_bb = self.ir.context.append_basic_block(fn_val, "sd.done");
        let result_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "sd_res");
        let two = i64_ty.const_int(2, false);
        let lt2 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, len, two, "sd_lt2");
        build_wrappers::w_cond_br(&self.ir.builder, lt2, small_bb, ok_bb);
        self.ir.builder.position_at_end(small_bb);
        build_wrappers::w_store(&self.ir.builder, result_slot, f64_ty.const_float(0.0).into());
        build_wrappers::w_br(&self.ir.builder, done_bb);

        self.ir.builder.position_at_end(ok_bb);
        // helper to run a counted loop accumulating an f64 via a callback.
        let len_f = build_wrappers::w_signed_int_to_float(&self.ir.builder, len, f64_ty, "sd_lf");

        // Pass 1: sum.
        let sum_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "sd_sum");
        build_wrappers::w_store(&self.ir.builder, sum_slot, f64_ty.const_float(0.0).into());
        let i1s = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sd_i1");
        build_wrappers::w_store(&self.ir.builder, i1s, i64_ty.const_zero().into());
        let c1 = self.ir.context.append_basic_block(fn_val, "sd.c1");
        let b1 = self.ir.context.append_basic_block(fn_val, "sd.b1");
        let e1 = self.ir.context.append_basic_block(fn_val, "sd.e1");
        build_wrappers::w_br(&self.ir.builder, c1);
        self.ir.builder.position_at_end(c1);
        let i1c = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), i1s, "sd_i1c").into_int_value();
        let g1 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i1c, len, "sd_g1");
        build_wrappers::w_cond_br(&self.ir.builder, g1, b1, e1);
        self.ir.builder.position_at_end(b1);
        let p1 = unsafe { self.ir.builder.build_gep(f64_ty, data, &[i1c], "sd_p1").unwrap() };
        let v1 = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), p1, "sd_v1").into_float_value();
        let s1 = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), sum_slot, "sd_s1").into_float_value();
        let ns1 = build_wrappers::w_float_add(&self.ir.builder, s1, v1, "sd_ns1");
        build_wrappers::w_store(&self.ir.builder, sum_slot, ns1.into());
        let n1 = build_wrappers::w_int_add(&self.ir.builder, i1c, i64_ty.const_int(1, false), "sd_n1");
        build_wrappers::w_store(&self.ir.builder, i1s, n1.into());
        build_wrappers::w_br(&self.ir.builder, c1);
        self.ir.builder.position_at_end(e1);
        let sum = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), sum_slot, "sd_sumf").into_float_value();
        let mean = self.ir.builder.build_float_div(sum, len_f, "sd_mean").unwrap();

        // Pass 2: Σ(x-mean)².
        let var_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "sd_var");
        build_wrappers::w_store(&self.ir.builder, var_slot, f64_ty.const_float(0.0).into());
        let i2s = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sd_i2");
        build_wrappers::w_store(&self.ir.builder, i2s, i64_ty.const_zero().into());
        let c2 = self.ir.context.append_basic_block(fn_val, "sd.c2");
        let b2 = self.ir.context.append_basic_block(fn_val, "sd.b2");
        let e2 = self.ir.context.append_basic_block(fn_val, "sd.e2");
        build_wrappers::w_br(&self.ir.builder, c2);
        self.ir.builder.position_at_end(c2);
        let i2c = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), i2s, "sd_i2c").into_int_value();
        let g2 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SLT, i2c, len, "sd_g2");
        build_wrappers::w_cond_br(&self.ir.builder, g2, b2, e2);
        self.ir.builder.position_at_end(b2);
        let p2 = unsafe { self.ir.builder.build_gep(f64_ty, data, &[i2c], "sd_p2").unwrap() };
        let v2 = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), p2, "sd_v2").into_float_value();
        let d = self.ir.builder.build_float_sub(v2, mean, "sd_d").unwrap();
        let dd = self.ir.builder.build_float_mul(d, d, "sd_dd").unwrap();
        let va = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), var_slot, "sd_va").into_float_value();
        let nva = build_wrappers::w_float_add(&self.ir.builder, va, dd, "sd_nva");
        build_wrappers::w_store(&self.ir.builder, var_slot, nva.into());
        let n2 = build_wrappers::w_int_add(&self.ir.builder, i2c, i64_ty.const_int(1, false), "sd_n2");
        build_wrappers::w_store(&self.ir.builder, i2s, n2.into());
        build_wrappers::w_br(&self.ir.builder, c2);
        self.ir.builder.position_at_end(e2);
        let var_acc = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), var_slot, "sd_vaf").into_float_value();
        // variance = var_acc / (len - 1); std = sqrt(variance).
        let lm1 = build_wrappers::w_int_sub(&self.ir.builder, len, i64_ty.const_int(1, false), "sd_lm1");
        let lm1_f = build_wrappers::w_signed_int_to_float(&self.ir.builder, lm1, f64_ty, "sd_lm1f");
        let variance = self.ir.builder.build_float_div(var_acc, lm1_f, "sd_variance").unwrap();
        let sqrt_fn = self.ir.module.get_function("sqrt")?;
        let std = self.ir.builder.build_call(sqrt_fn, &[variance.into()], "sd_sqrt").unwrap()
            .try_as_basic_value().left()?.into_float_value();
        build_wrappers::w_store(&self.ir.builder, result_slot, std.into());
        build_wrappers::w_br(&self.ir.builder, done_bb);

        self.ir.builder.position_at_end(done_bb);
        Some(build_wrappers::w_load(&self.ir.builder, f64_ty.into(), result_slot, "sd_final"))
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
            // Same message the interpreter prints (e.g. "arr_max_f64: array is
            // empty") via __axon_msg_panic — was a bare exit(101) with no text.
            let msg = match &op {
                ArrReduceF64::Extreme { is_max: true } => "arr_max_f64: array is empty",
                ArrReduceF64::Extreme { is_max: false } => "arr_min_f64: array is empty",
                ArrReduceF64::ArgExtreme { is_max: true } => "arr_argmax_f64: array is empty",
                ArrReduceF64::ArgExtreme { is_max: false } => "arr_argmin_f64: array is empty",
                _ => "array is empty",
            };
            if let Some(panic_fn) = self.ir.module.get_function("__axon_msg_panic") {
                let g = build_wrappers::w_global_string_ptr(&self.ir.builder, msg, "af_empty_msg");
                let mlen = i64_ty.const_int(msg.len() as u64, false);
                build_wrappers::w_call(&self.ir.builder, panic_fn, &[g.into(), mlen.into()], "");
            } else if let Some(exit_fn) = self.ir.module.get_function("exit") {
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
            let field_sem_types = self.struct_field_sem_types.get(name).cloned().unwrap_or_default();
            let alloca = build_wrappers::w_alloca(&self.ir.builder, struct_ty.into(), name);
            for (fname, fexpr) in fields {
                let idx = field_names.iter().position(|n| n == fname).unwrap_or(0) as u32;
                // Set the Option/Result context from the field's DECLARED type so
                // a sum-type field initializer (`Box { r: Err("x") }`) builds the
                // field's full canonical layout, not a value-only `{i1,ptr}` that
                // mismatches the struct's `{i1,[16 x i8]}` field slot.
                let saved_oi = self.current_option_inner.clone();
                let saved_rt = self.current_result_types.clone();
                match field_sem_types.get(idx as usize) {
                    Some(Type::Option(inner)) => self.current_option_inner = Some((**inner).clone()),
                    Some(Type::Result(ok, err)) => {
                        self.current_result_types = Some(((**ok).clone(), (**err).clone()))
                    }
                    _ => {}
                }
                let emitted = self.emit_expr(fexpr, fn_val);
                self.current_option_inner = saved_oi;
                self.current_result_types = saved_rt;
                if let Some(fval) = emitted {
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
                        // Track each arg's ACTUAL LLVM type so the indirect-call
                        // signature matches the value passed (a str arg is a
                        // {i64,ptr} struct, not an i64) — and emit_lambda declares
                        // its params from the same annotation, so the two agree.
                        let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> =
                            vec![ep.into()];
                        let mut arg_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
                            vec![ptr_ty.into()];
                        for a in args {
                            if let Some(v) = self.emit_expr(a, fn_val) {
                                call_args.push(v.into());
                                arg_tys.push(v.get_type().into());
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
                        let indirect_ty = i64_ty.fn_type(&arg_tys, false);
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

        // ── Dict builtins (R1c) — opaque i8* handle + tagged-value runtime ──
        if let ast::Expr::Ident(name) = callee {
            if name == "dict_new" && args.is_empty() {
                let f = self.functions.get("__axon_dict_new").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_new"))?;
                return self.ir.builder.build_call(f, &[], "dict_new").unwrap().try_as_basic_value().left();
            }
            if name == "dict_set" && args.len() == 3 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let v = self.emit_expr(&args[2], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
                // Tag dispatch on the value's LLVM type (call-site, like to_str).
                let (tag, payload, pstr, plen) = match v {
                    BasicValueEnum::FloatValue(fv) => {
                        // tag=1; payload = bitcast f64→i64.
                        let bits = self.ir.builder.build_bitcast(fv, i64_ty, "ds_fbits").unwrap().into_int_value();
                        (1i64, bits, i8_ptr.const_null(), i64_ty.const_zero())
                    }
                    BasicValueEnum::StructValue(sv) if sv.get_type() == str_ty => {
                        // tag=2; pass the str's (ptr,len) as the str payload.
                        let slen = build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "ds_sl").into_int_value();
                        let sptr = build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "ds_sp").into_pointer_value();
                        (2i64, i64_ty.const_zero(), sptr, slen)
                    }
                    BasicValueEnum::IntValue(iv) => {
                        // tag=0; widen narrow ints (bool/i32) to i64 payload.
                        let p = if iv.get_type().get_bit_width() < 64 {
                            build_wrappers::w_int_s_extend(&self.ir.builder, iv, i64_ty, "ds_iw")
                        } else { iv };
                        (0i64, p, i8_ptr.const_null(), i64_ty.const_zero())
                    }
                    _ => return None,
                };
                let f = self.functions.get("__axon_dict_set").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_set"))?;
                self.ir.builder.build_call(f, &[
                    d.into(), key.into(), i64_ty.const_int(tag as u64, false).into(),
                    payload.into(), pstr.into(), plen.into()], "").unwrap();
                return None;
            }
            if name == "dict_has" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let f = self.functions.get("__axon_dict_has").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_has"))?;
                return self.ir.builder.build_call(f, &[d.into(), key.into()], "dict_has").unwrap().try_as_basic_value().left();
            }
            // dict_get(d, k) → Option<i64>. v1: the value is reinterpreted as i64
            // (the int-valued case — the common state-counter shape). Str-valued
            // dicts are a follow-on (would need Option<str>, dispatched by the
            // surrounding match's expected type). Calls the extern with out-param
            // slots, then builds Some(payload)/None from the found flag.
            if name == "dict_get" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let tag_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dg_tag");
                let pay_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dg_pay");
                let sl_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dg_sl");
                let f = self.functions.get("__axon_dict_get").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_get"))?;
                let found = self.ir.builder
                    .build_call(f, &[d.into(), key.into(), tag_slot.into(), pay_slot.into(), sl_slot.into()], "dg_call")
                    .unwrap().try_as_basic_value().left()?.into_int_value();
                let payload = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), pay_slot, "dg_p").into_int_value();
                // Build Option<i64> via emit_option + select on the found flag.
                let some_v = self.emit_option(Some(payload.into()), &Type::I64);
                let none_v = self.emit_option(None, &Type::I64);
                let chosen = self.ir.builder.build_select(found, some_v, none_v, "dg_opt").unwrap();
                return Some(chosen);
            }
            if name == "dict_len" && args.len() == 1 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let f = self.functions.get("__axon_dict_len").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_len"))?;
                return self.ir.builder.build_call(f, &[d.into()], "dict_len").unwrap().try_as_basic_value().left();
            }
            // dict_keys(d) → [str]: the runtime mallocs an array of AxonStr +
            // each key's bytes; codegen wraps it in a {len, ptr} slice struct.
            if name == "dict_keys" && args.len() == 1 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dk_len");
                let data_slot = build_wrappers::w_alloca(&self.ir.builder, ptr_ty.into(), "dk_data");
                let f = self.functions.get("__axon_dict_keys").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_keys"))?;
                self.ir.builder.build_call(f, &[d.into(), len_slot.into(), data_slot.into()], "dk_call").unwrap();
                let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), len_slot, "dk_l").into_int_value();
                let data = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(), data_slot, "dk_d").into_pointer_value();
                let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "dk_out");
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 0, "dk_ol").unwrap(), len.into());
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 1, "dk_op").unwrap(), data.into());
                return Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "dk_res"));
            }
            // str_split(s, sep) → [str]: the runtime mallocs an array of AxonStr
            // (each part's bytes too); codegen wraps it in a {len, ptr} slice.
            // Same out-param shape as dict_keys, but two str args by value.
            if name == "str_split" && args.len() == 2 {
                let s = self.emit_expr(&args[0], fn_val)?;
                let sep = self.emit_expr(&args[1], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ss_len");
                let data_slot = build_wrappers::w_alloca(&self.ir.builder, ptr_ty.into(), "ss_data");
                let f = self.functions.get("__axon_str_split").copied()
                    .or_else(|| self.ir.module.get_function("__axon_str_split"))?;
                self.ir.builder.build_call(f, &[s.into(), sep.into(), len_slot.into(), data_slot.into()], "ss_call").unwrap();
                let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), len_slot, "ss_l").into_int_value();
                let data = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(), data_slot, "ss_d").into_pointer_value();
                let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "ss_out");
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 0, "ss_ol").unwrap(), len.into());
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 1, "ss_op").unwrap(), data.into());
                return Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "ss_res"));
            }
            // str_join(parts, sep) → str: the inverse of str_split. Unpack the
            // [str] slice arg into its (len, data) scalars (data → AxonStr*),
            // pass them + the sep str to the runtime, assemble the str result.
            if name == "str_join" && args.len() == 2 {
                let parts = self.emit_expr(&args[0], fn_val)?;
                let sep = self.emit_expr(&args[1], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let str_ptr_ty = str_ty.ptr_type(AddressSpace::default());
                // Spill the slice to extract its {len, data} fields.
                let parts_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "sj_parts");
                build_wrappers::w_store(&self.ir.builder, parts_slot, parts);
                let slen = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
                    self.ir.builder.build_struct_gep(str_ty, parts_slot, 0, "sj_lp").unwrap(), "sj_len").into_int_value();
                let sdata_i8 = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
                    self.ir.builder.build_struct_gep(str_ty, parts_slot, 1, "sj_dp").unwrap(), "sj_data").into_pointer_value();
                // data points at an array of AxonStr (== str_ty); cast i8* → str_ty*.
                let sdata = build_wrappers::w_pointer_cast(&self.ir.builder, sdata_i8, str_ptr_ty, "sj_dcast");
                let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sj_olen");
                let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, ptr_ty.into(), "sj_optr");
                let f = self.functions.get("__axon_str_join").copied()
                    .or_else(|| self.ir.module.get_function("__axon_str_join"))?;
                self.ir.builder.build_call(f,
                    &[slen.into(), sdata.into(), sep.into(), out_len_slot.into(), out_ptr_slot.into()], "sj_call").unwrap();
                let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "sj_ol").into_int_value();
                let out_ptr = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(), out_ptr_slot, "sj_op").into_pointer_value();
                let res = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "sj_res");
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(str_ty, res, 0, "sj_rl").unwrap(), out_len.into());
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(str_ty, res, 1, "sj_rp").unwrap(), out_ptr.into());
                return Some(build_wrappers::w_load(&self.ir.builder, str_ty.into(), res, "sj_resv"));
            }
            // dict_from_pairs([(str,i64)]) → Dict: unpack the slice's {len,data}
            // and hand them to the runtime, which reads `data` as an array of
            // (str,i64) tuples and inserts each into a fresh dict. Returns the
            // i8* handle directly (no out-params).
            if name == "dict_from_pairs" && args.len() == 1 {
                let pairs = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let slot = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "dfp_s");
                build_wrappers::w_store(&self.ir.builder, slot, pairs);
                let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(),
                    self.ir.builder.build_struct_gep(slice_ty, slot, 0, "dfp_lp").unwrap(), "dfp_len").into_int_value();
                let data = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(),
                    self.ir.builder.build_struct_gep(slice_ty, slot, 1, "dfp_dp").unwrap(), "dfp_data").into_pointer_value();
                let f = self.functions.get("__axon_dict_from_pairs").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_from_pairs"))?;
                return self.ir.builder.build_call(f, &[len.into(), data.into()], "dfp_call")
                    .unwrap().try_as_basic_value().left();
            }
            // dict_values(d) → [i64] (v1 int-valued): the runtime mallocs an
            // i64 array in key-sorted order; codegen wraps it in a {len, ptr}
            // slice. Same out-param shape as dict_keys, but the element is a
            // fixed-width i64 (no per-element malloc), reinterpreted with the
            // dict_get convention.
            if name == "dict_values" && args.len() == 1 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dv_len");
                let data_slot = build_wrappers::w_alloca(&self.ir.builder, ptr_ty.into(), "dv_data");
                let f = self.functions.get("__axon_dict_values").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_values"))?;
                self.ir.builder.build_call(f, &[d.into(), len_slot.into(), data_slot.into()], "dv_call").unwrap();
                let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), len_slot, "dv_l").into_int_value();
                let data = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(), data_slot, "dv_d").into_pointer_value();
                let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "dv_out");
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 0, "dv_ol").unwrap(), len.into());
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 1, "dv_op").unwrap(), data.into());
                return Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "dv_res"));
            }
            // dict_to_pairs(d) → [(str,i64)] (inverse of dict_from_pairs): the
            // runtime mallocs an array of StrI64Pair tuples; codegen wraps it in
            // a {len, ptr} slice. Identical out-param shape to dict_keys/values;
            // the element-tuple type is set by infer_expr_sem_type.
            if name == "dict_to_pairs" && args.len() == 1 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.ir.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dtp_len");
                let data_slot = build_wrappers::w_alloca(&self.ir.builder, ptr_ty.into(), "dtp_data");
                let f = self.functions.get("__axon_dict_to_pairs").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_to_pairs"))?;
                self.ir.builder.build_call(f, &[d.into(), len_slot.into(), data_slot.into()], "dtp_call").unwrap();
                let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), len_slot, "dtp_l").into_int_value();
                let data = build_wrappers::w_load(&self.ir.builder, ptr_ty.into(), data_slot, "dtp_d").into_pointer_value();
                let out = build_wrappers::w_alloca(&self.ir.builder, slice_ty.into(), "dtp_out");
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 0, "dtp_ol").unwrap(), len.into());
                build_wrappers::w_store(&self.ir.builder,
                    self.ir.builder.build_struct_gep(slice_ty, out, 1, "dtp_op").unwrap(), data.into());
                return Some(build_wrappers::w_load(&self.ir.builder, slice_ty.into(), out, "dtp_res"));
            }
            // dict_map_values(d, f) → Dict: hand the dict handle + the lambda's
            // (fn_ptr, env) to the runtime, which iterates and indirect-calls
            // `i64 f(i8* env, i64 val)` per value (the __axon_spawn callback
            // pattern). Returns the new handle. v1: int-valued.
            if name == "dict_map_values" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let lam = match self.emit_expr(&args[1], fn_val)? {
                    BasicValueEnum::StructValue(s) => s,
                    _ => return None,
                };
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "dmv_fn").into_pointer_value();
                let env_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "dmv_env").into_pointer_value();
                let fn_p = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "dmv_fp");
                let env_p = build_wrappers::w_pointer_cast(&self.ir.builder, env_raw, ptr_ty, "dmv_ep");
                let f = self.functions.get("__axon_dict_map_values").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_map_values"))?;
                return self.ir.builder.build_call(f, &[d.into(), fn_p.into(), env_p.into()], "dmv_call")
                    .unwrap().try_as_basic_value().left();
            }
            // dict_filter(d, pred) → Dict: same runtime-callback lowering as
            // dict_map_values, but the predicate is `fn(str key, i64 val) -> bool`
            // (works now that lambda params are typed by annotation) and the
            // runtime keeps each entry iff the call returns non-zero.
            if name == "dict_filter" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                // The predicate is `fn(str key, i64 val) -> bool`; hint those
                // param types so an inline `|k, v|` (no annotations) types `k` as
                // the str struct and `v` as i64.
                let i64_ty = self.ir.context.i64_type();
                let i8p = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8p.into()], false);
                self.pending_lambda_param_tys = Some(vec![str_ty.into(), i64_ty.into()]);
                let lam = match self.emit_expr(&args[1], fn_val)? {
                    BasicValueEnum::StructValue(s) => s,
                    _ => { self.pending_lambda_param_tys = None; return None; }
                };
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "dfl_fn").into_pointer_value();
                let env_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "dfl_env").into_pointer_value();
                let fn_p = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "dfl_fp");
                let env_p = build_wrappers::w_pointer_cast(&self.ir.builder, env_raw, ptr_ty, "dfl_ep");
                let f = self.functions.get("__axon_dict_filter").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_filter"))?;
                return self.ir.builder.build_call(f, &[d.into(), fn_p.into(), env_p.into()], "dfl_call")
                    .unwrap().try_as_basic_value().left();
            }
            // dict_each(d, f) → (): same runtime-callback as dict_filter but the
            // result is discarded and nothing is returned (side effects only).
            // The predicate `fn(str key, i64 val)` works via the [str,i64] hint.
            if name == "dict_each" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let i8p = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8p.into()], false);
                self.pending_lambda_param_tys = Some(vec![str_ty.into(), i64_ty.into()]);
                let lam = match self.emit_expr(&args[1], fn_val)? {
                    BasicValueEnum::StructValue(s) => s,
                    _ => { self.pending_lambda_param_tys = None; return None; }
                };
                let ptr_ty = i8p;
                let fn_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 0, "dea_fn").into_pointer_value();
                let env_raw = build_wrappers::w_extract_value(&self.ir.builder, lam, 1, "dea_env").into_pointer_value();
                let fn_p = build_wrappers::w_pointer_cast(&self.ir.builder, fn_raw, ptr_ty, "dea_fp");
                let env_p = build_wrappers::w_pointer_cast(&self.ir.builder, env_raw, ptr_ty, "dea_ep");
                let f = self.functions.get("__axon_dict_each").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_each"))?;
                self.ir.builder.build_call(f, &[d.into(), fn_p.into(), env_p.into()], "dea_call").unwrap();
                // Unit-returning: produce a placeholder i64 0 (Unit values are
                // never read; matches how other Unit builtins lower).
                return Some(i64_ty.const_zero().into());
            }
            if name == "dict_inc" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let f = self.functions.get("__axon_dict_inc").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_inc"))?;
                return self.ir.builder.build_call(f, &[d.into(), key.into()], "dict_inc").unwrap().try_as_basic_value().left();
            }
            // dict_remove(d, k) → Option<i64> (v1 int-valued): remove + return
            // the prior value. Same out-param + emit_option shape as dict_get.
            if name == "dict_remove" && args.len() == 2 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let tag_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dr_tag");
                let pay_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dr_pay");
                let sl_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dr_sl");
                let f = self.functions.get("__axon_dict_remove").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_remove"))?;
                let found = self.ir.builder
                    .build_call(f, &[d.into(), key.into(), tag_slot.into(), pay_slot.into(), sl_slot.into()], "dr_call")
                    .unwrap().try_as_basic_value().left()?.into_int_value();
                let payload = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), pay_slot, "dr_p").into_int_value();
                let some_v = self.emit_option(Some(payload.into()), &Type::I64);
                let none_v = self.emit_option(None, &Type::I64);
                let chosen = self.ir.builder.build_select(found, some_v, none_v, "dr_opt").unwrap();
                return Some(chosen);
            }
            // dict_get_or(d, k, default) → value-or-default. v1: int-valued —
            // calls dict_get's extern and selects payload (found) vs default.
            if name == "dict_get_or" && args.len() == 3 {
                let d = self.emit_expr(&args[0], fn_val)?;
                let key = self.emit_expr(&args[1], fn_val)?;
                let default = self.emit_expr(&args[2], fn_val)?;
                let i64_ty = self.ir.context.i64_type();
                let tag_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "go_tag");
                let pay_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "go_pay");
                let sl_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "go_sl");
                let f = self.functions.get("__axon_dict_get").copied()
                    .or_else(|| self.ir.module.get_function("__axon_dict_get"))?;
                let found = self.ir.builder
                    .build_call(f, &[d.into(), key.into(), tag_slot.into(), pay_slot.into(), sl_slot.into()], "go_call")
                    .unwrap().try_as_basic_value().left()?.into_int_value();
                let payload = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), pay_slot, "go_p");
                // select(found, payload, default).
                let chosen = self.ir.builder.build_select(found, payload, default, "go_sel").unwrap();
                return Some(chosen);
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
            // arr_index_of(&a, x) → Option<i64>: Some(first index where elem == x),
            // or None. The loop yields the index (or the -1 sentinel for "none");
            // wrap it in Option<i64> here (Some(idx) iff idx != -1), matching the
            // interpreter's `Some(i)`/`None` return — same shape as dict_get.
            if name == "arr_index_of" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(n))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let idx = self.emit_arr_i64_loop(slice_val, ArrReduce::IndexOf(n), fn_val)?
                        .into_int_value();
                    let i64_ty = self.ir.context.i64_type();
                    let found = build_wrappers::w_int_compare(
                        &self.ir.builder, inkwell::IntPredicate::NE,
                        idx, i64_ty.const_int(-1i64 as u64, true), "aio_found");
                    let some_v = self.emit_option(Some(idx.into()), &Type::I64);
                    let none_v = self.emit_option(None, &Type::I64);
                    let chosen = self.ir.builder.build_select(found, some_v, none_v, "aio_opt").unwrap();
                    return Some(chosen);
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
            // arr_std_f64(&a) → sample standard deviation (n-1 denominator),
            // 0.0 for <2 elements. Two-pass: Σ→mean, then Σ(x-mean)²/(n-1), sqrt.
            if name == "arr_std_f64" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_f64_std(slice_val, fn_val);
                }
            }
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
            // arr_push(&a, x) → a ++ [x] (fresh array; input untouched).
            if name == "arr_push" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(x))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    if let Some(r) = self.emit_arr_i64_push(slice_val, x, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_map(&a, |x| ...) / arr_filter(&a, |x| ...) — the first
            // CLOSURE-taking arr_* lowerings. The 2nd arg is a lambda fat-pointer
            // `{i8* fn, i8* env}`; per element we indirect-call `fn(env, elem)`.
            // map → mapped value into the result; filter → keep where pred true.
            // map → mapped value; filter → keep where pred true; take_while →
            // keep the leading pred-true prefix; drop_while → keep from the first
            // pred-false element on. All four share emit_arr_i64_closure (a bool
            // predicate is the i64-widened lambda result, read back as != 0).
            if matches!(name.as_str(), "arr_map" | "arr_filter" | "arr_take_while" | "arr_drop_while")
                && args.len() == 2
            {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let cmode = match name.as_str() {
                        "arr_map" => ClosureMode::Map,
                        "arr_filter" => ClosureMode::Filter,
                        "arr_take_while" => ClosureMode::TakeWhile,
                        _ => ClosureMode::DropWhile,
                    };
                    if let Some(r) = self.emit_arr_i64_closure(slice_val, lam, cmode, fn_val) {
                        return Some(r);
                    }
                }
            }
            // arr_max_by / arr_min_by(&a, key_fn) → the i64 element maximizing /
            // minimizing the f64 key. key_fn returns f64, transported through the
            // i64 lambda ABI as bitcast bits (recovered with a bitcast back).
            if (name == "arr_max_by" || name == "arr_min_by") && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    let is_max = name == "arr_max_by";
                    if let Some(r) = self.emit_arr_i64_max_by(slice_val, lam, is_max, fn_val) {
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
            // arr_unique(&a) → first occurrence of each value (O(n²) seen-scan).
            if name == "arr_unique" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_i64_unique(slice_val, fn_val);
                }
            }
            // arr_enumerate(&a) → [(i, a[i])] : a slice of {i64 idx, i64 val}
            // tuples (16-byte stride). The first nested/tuple-element arr_*.
            if name == "arr_enumerate" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_i64_enumerate(slice_val, fn_val);
                }
            }
            // arr_zip(a, b) → [(a[i], b[i])] for i in 0..min(len) : a slice of
            // {i64, i64} tuples (16-byte stride).
            if name == "arr_zip" && args.len() == 2 {
                if let (Some(a_slice), Some(b_slice)) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_zip(a_slice, b_slice, fn_val);
                }
            }
            // arr_flatten(&a) — [[i64]] → [i64]. Outer slice has 16-byte
            // {i64 len, ptr} slice-struct elements; concatenate all inner i64s.
            if name == "arr_flatten" && args.len() == 1 {
                if let Some(slice_val) = self.emit_expr(&args[0], fn_val) {
                    return self.emit_arr_i64_flatten(slice_val, fn_val);
                }
            }
            // arr_chunk(&a, n) — [i64] → [[i64]] in chunks of n (last may be
            // shorter). n<=0 → exit(101) panic. Each chunk is a fresh slice.
            if name == "arr_chunk" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::IntValue(n))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_chunk(slice_val, n, fn_val);
                }
            }
            // arr_partition(&a, |x| pred) → ([yes], [no]): a tuple of two i64
            // slices — elements where the predicate is true / false.
            if name == "arr_partition" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_partition(slice_val, lam, fn_val);
                }
            }
            // arr_find(&a, |x| pred) → Option<i64>: the first element satisfying
            // the predicate (Some), else None.
            if name == "arr_find" && args.len() == 2 {
                if let (Some(slice_val), Some(BasicValueEnum::StructValue(lam))) =
                    (self.emit_expr(&args[0], fn_val), self.emit_expr(&args[1], fn_val))
                {
                    return self.emit_arr_i64_find(slice_val, lam, fn_val);
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

            // Set the Option/Result type context from the DECLARED param type so a
            // bare `None` / `Ok(..)`/`Err(..)` argument builds the param's full
            // canonical layout — otherwise `g(None)` where `g(o: Option<str>)`
            // emits `{i1,i64}` and fails IR verification against the `{i1,ptr}`
            // param. Restored at the end of this iteration.
            let saved_oi_arg = self.current_option_inner.clone();
            let saved_rt_arg = self.current_result_types.clone();
            if let Some(pt) = axon_param_ty {
                match self.axon_type_to_semantic(pt) {
                    Type::Option(inner) => self.current_option_inner = Some(*inner),
                    Type::Result(ok, err) => self.current_result_types = Some((*ok, *err)),
                    _ => {}
                }
            }

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
                // Soft typing: an `Uncertain<T>` argument (`{ T, f64, i64 }`
                // struct) passed to a plain-`T` parameter — unwrap to the inner
                // value (field 0), matching the interpreter. Confidence is dropped
                // at the T-typed boundary. Only when the param is a scalar (the
                // struct passed to an Uncertain param stays a struct).
                (Some(BasicTypeEnum::IntType(_) | BasicTypeEnum::FloatType(_)), BasicValueEnum::StructValue(sv)) => {
                    match self.ir.builder.build_extract_value(sv, 0, "unc_arg") {
                        Ok(inner) => inner,
                        Err(_) => val,
                    }
                }
                _ => val,
            };
            arg_vals.push(coerced.into());
            // Restore the Option/Result context after this arg.
            self.current_option_inner = saved_oi_arg;
            self.current_result_types = saved_rt_arg;
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
