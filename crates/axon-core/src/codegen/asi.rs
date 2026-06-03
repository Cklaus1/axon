//! ASI Layer-1/2/3 codegen helpers.
//!
//! Phase 2 of the §7.5 module split: extracts the cohesive block of methods
//! that emit ASI-specific runtime hooks:
//!   * `__axon_provenance_log` calls (event-style, Layer 1)
//!   * `__axon_provenance_log_ret_{i64,f64}` typed-return logs (Layer 2)
//!   * `__axon_register_adaptive` registry init in main (Layer 3 hill-climb)
//!   * `__axon_verify_panic` runtime gate emission (Layer 3 @[verify])
//!
//! All methods are `pub(super)` so the parent `codegen::mod` impl block can
//! call them.  No fields are mutated except `self.ir.builder`'s position;
//! callers must restore the insert block when needed.
//!
//! Field visibility requirements: this file accesses `self.ir.builder`,
//! `self.ir.context`, `self.ir.module` (all `pub`), plus the ASI-specific state
//! `self.adaptive_registry_targets`, `self.current_adaptive_fn`,
//! `self.current_verify_fn`, and `self.functions` (made `pub(super)` in
//! `mod.rs` to support this extraction).

use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::FloatPredicate;

use crate::ast;
use crate::types::Type;

use super::build_wrappers;

impl<'ctx> super::Codegen<'ctx> {
    /// R4 §4.3: emit the mandatory `agent_action` audit log when a capability-
    /// bearing builtin `action` (exercising capability `caps`) is called inside
    /// the current `@[agent]` fn. No-op when not in an agent fn, when the builtin
    /// is pure (no capability), or when the block is already terminated. This is
    /// the codegen counterpart to the interpreter's `append_agent_action_jsonl`
    /// — the highest-trust zone's un-opt-out-able audit trail (I-13).
    pub(super) fn emit_agent_action_log(&mut self, action: &str) {
        let Some(agent_fn) = self.current_agent_fn.clone() else { return };
        let Some(caps) = crate::capabilities::capability_of_builtin(action) else { return };
        let log_fn = match self.ir.module.get_function("__axon_log_agent_action") {
            Some(f) => f,
            None => return,
        };
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let fn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &agent_fn, "aa_fn");
        let act_g = build_wrappers::w_global_string_ptr(&self.ir.builder, action, "aa_action");
        let cap_g = build_wrappers::w_global_string_ptr(&self.ir.builder, caps, "aa_caps");
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            log_fn,
            &[
                fn_g.into(), i64_ty.const_int(agent_fn.len() as u64, false).into(),
                act_g.into(), i64_ty.const_int(action.len() as u64, false).into(),
                cap_g.into(), i64_ty.const_int(caps.len() as u64, false).into(),
            ],
            "",
        );
    }

    // ── Provenance logging helpers (for @[adaptive] functions) ──────────────
    /// Emit a call to `__axon_provenance_log(fn_name, event)` at the current
    /// builder position.  Used at function prologues and immediately before
    /// every `build_return` in adaptive functions.
    pub(super) fn emit_provenance_log(&mut self, fn_name: &str, event: &str) {
        let prov_fn = match self.ir.module.get_function("__axon_provenance_log") {
            Some(f) => f,
            None => return, // safety: declare_builtins should have added this
        };
        // Skip if the current basic block is already terminated.
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, fn_name, "prov_fn_name");
        let evt_g  = build_wrappers::w_global_string_ptr(&self.ir.builder, event,   "prov_event");
        let name_len = i64_ty.const_int(fn_name.len() as u64, false);
        let evt_len  = i64_ty.const_int(event.len()    as u64, false);
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            prov_fn,
            &[
                name_g.into(),
                name_len.into(),
                evt_g.into(),
                evt_len.into(),
            ],
            "",
        );
    }

    /// Layer-2: emit a typed-return provenance event for an `@[adaptive]`
    /// function.  If the return value's LLVM type is `i64`, calls
    /// `__axon_provenance_log_ret_i64`; if it's `f64`, calls
    /// `__axon_provenance_log_ret_f64`.  For any other return type (str,
    /// struct, enum, etc.) we still emit the legacy `"return"` string event so
    /// the on-disk JSONL stays complete.
    pub(super) fn emit_provenance_log_ret(&mut self, fn_name: &str, ret_val: BasicValueEnum<'ctx>) {
        // Skip if the current basic block is already terminated.
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let f64_ty = self.ir.context.f64_type();
        let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, fn_name, "prov_fn_name");
        let name_len = i64_ty.const_int(fn_name.len() as u64, false);

        match ret_val {
            BasicValueEnum::IntValue(iv) if iv.get_type() == i64_ty => {
                // F11: when the adaptive fn has a leading i64 input, log
                // (input, score) via the `_in` entry point so goal_run can
                // warm-start. Otherwise fall back to the score-only log.
                if let Some(input_iv) = self.current_adaptive_input {
                    if let Some(rt) = self.ir.module.get_function("__axon_provenance_log_ret_i64_in") {
                        let _ = build_wrappers::w_call(
                            &self.ir.builder,
                            rt,
                            &[name_g.into(), name_len.into(), input_iv.into(), iv.into()],
                            "",
                        );
                        return;
                    }
                }
                if let Some(rt) = self.ir.module.get_function("__axon_provenance_log_ret_i64") {
                    let _ = build_wrappers::w_call(
                        &self.ir.builder,
                        rt,
                        &[name_g.into(), name_len.into(), iv.into()],
                        "",
                    );
                    return;
                }
            }
            BasicValueEnum::FloatValue(fv) if fv.get_type() == f64_ty => {
                if let Some(rt) = self.ir.module.get_function("__axon_provenance_log_ret_f64") {
                    let _ = build_wrappers::w_call(
                        &self.ir.builder,
                        rt,
                        &[name_g.into(), name_len.into(), fv.into()],
                        "",
                    );
                    return;
                }
            }
            _ => {}
        }
        // Fallback: legacy event-style log.
        self.emit_provenance_log(fn_name, "return");
    }

    /// Emit a "return" provenance event if the current function carries
    /// `@[adaptive]`.  Call this immediately before each `build_return` whose
    /// return value is *not* known to be i64/f64 (or which has no value).
    pub(super) fn log_return_if_adaptive(&mut self) {
        if let Some(fn_name) = self.current_adaptive_fn.clone() {
            self.emit_provenance_log(&fn_name, "return");
        }
    }

    /// Layer-2 variant: emit a typed-return event if the current function
    /// carries `@[adaptive]`.  Pass the value being returned so the runtime
    /// can score it.  Call this *instead* of `log_return_if_adaptive()` at
    /// every `build_return(Some(&v))` site.
    pub(super) fn log_return_if_adaptive_val(&mut self, ret_val: BasicValueEnum<'ctx>) {
        if let Some(fn_name) = self.current_adaptive_fn.clone() {
            self.emit_provenance_log_ret(&fn_name, ret_val);
        }
    }

    /// ASI Layer-3 `@[verify]` runtime helper.
    ///
    /// Called at every return site of a function whose `current_verify_fn`
    /// is set (i.e. the surrounding fn is `@[verify(confidence OP K)]` and
    /// has return type `Uncertain<T>`).  At codegen time we:
    ///
    ///   1. `extractvalue` field index 1 (`confidence: f64`) from the
    ///      Uncertain struct value being returned.
    ///   2. `fcmp <pred>` it against the literal bound K, where `<pred>`
    ///      mirrors the source operator (`>=` → OGE, `>` → OGT, `<=` → OLE,
    ///      `<` → OLT, `==` → OEQ, `!=` → ONE).
    ///   3. Branch: on success, fall through to the original `build_return`;
    ///      on failure, call `__axon_verify_panic(fn_name, op, K, actual)`
    ///      and emit `unreachable`.
    ///
    /// No-op when `current_verify_fn` is `None`, when the value is not an
    /// Uncertain struct (defensive), or when the verify-panic extern is
    /// missing.  Codegen never aborts compilation here — the static checker
    /// is the primary gate; this is *additional* enforcement.
    ///
    /// Call this at every `build_return(Some(&v))` site immediately *before*
    /// the actual `build_return`, alongside `log_return_if_adaptive_val(v)`.
    pub(super) fn emit_verify_check_if_needed(
        &mut self,
        ret_val: BasicValueEnum<'ctx>,
        llvm_fn: FunctionValue<'ctx>,
    ) {
        // Quick exit: not in a verify-armed function.
        let (fn_name, op_str, bound) = match self.current_verify_fn.clone() {
            Some(t) => t,
            None => return,
        };

        // Defensive: skip if the runtime extern isn't declared.
        let panic_fn = match self.ir.module.get_function("__axon_verify_panic") {
            Some(f) => f,
            None => return,
        };

        // Skip if the current basic block is already terminated — we can't
        // legally insert further IR there.
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }

        // The return value must be an Uncertain<T> struct (field 1 = f64
        // confidence).  Anything else means the verify clause was attached to
        // a function whose return type isn't actually Uncertain — the static
        // checker should have rejected it, but be defensive.
        let struct_val = match ret_val {
            BasicValueEnum::StructValue(sv) => sv,
            _ => return,
        };
        let f64_ty = self.ir.context.f64_type();
        // Confidence is at index 1 in `{ value, confidence: f64, source_tag: i64 }`.
        let conf_ev = build_wrappers::w_extract_value(&self.ir.builder, struct_val, 1, "verify_conf");
        // Sanity: must be an f64.  If the struct shape is unexpected, bail.
        let actual = match conf_ev {
            BasicValueEnum::FloatValue(fv) if fv.get_type() == f64_ty => fv,
            _ => return,
        };

        // Translate operator string → LLVM float predicate.  Matches the set
        // accepted by `verify::decode_verify_predicate`.
        let pred = match op_str {
            ">="          => FloatPredicate::OGE,
            ">"           => FloatPredicate::OGT,
            "<="          => FloatPredicate::OLE,
            "<"           => FloatPredicate::OLT,
            "=="          => FloatPredicate::OEQ,
            "!="          => FloatPredicate::ONE,
            // Unknown op string: silently no-op (mirrors static checker).
            _ => return,
        };

        let bound_const = f64_ty.const_float(bound);
        let cmp = build_wrappers::w_float_compare(&self.ir.builder, pred, actual, bound_const, "verify_cmp");

        // Build branch: cmp ? continue : panic.  We append two blocks to the
        // current function and route the *current* block into them.
        let panic_bb = self.ir.context.append_basic_block(llvm_fn, "verify_panic");
        let cont_bb  = self.ir.context.append_basic_block(llvm_fn, "verify_ok");

        let _ = build_wrappers::w_cond_br(&self.ir.builder, cmp, cont_bb, panic_bb);

        // ── Panic path ────────────────────────────────────────────────────
        self.ir.builder.position_at_end(panic_bb);
        let i64_ty = self.ir.context.i64_type();
        let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &fn_name, "verify_fn_name");
        let op_g   = build_wrappers::w_global_string_ptr(&self.ir.builder, op_str,   "verify_op");
        let name_len = i64_ty.const_int(fn_name.len() as u64, false);
        let op_len   = i64_ty.const_int(op_str.len()   as u64, false);
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            panic_fn,
            &[
                name_g.into(),
                name_len.into(),
                op_g.into(),
                op_len.into(),
                bound_const.into(),
                actual.into(),
            ],
            "",
        );
        let _ = build_wrappers::w_unreachable(&self.ir.builder);

        // ── Continue path: original return falls through here. ────────────
        self.ir.builder.position_at_end(cont_bb);
    }

    /// ASI Layer-3: emit one `__axon_register_adaptive(name, len, fn_ptr)`
    /// call per eligible adaptive function (`@[adaptive] fn(i64) -> i64`).
    /// Called from main's prologue.  No-op when no eligible functions exist
    /// (no runtime cost for non-AI programs).  Eligibility was decided in
    /// `emit_program` and recorded in `self.adaptive_registry_targets`.
    pub(super) fn emit_adaptive_registry_init(&mut self) {
        if self.adaptive_registry_targets.is_empty() {
            return;
        }
        let reg_fn = match self.ir.module.get_function("__axon_register_adaptive") {
            Some(f) => f,
            None => return,
        };
        // Skip if the current basic block is already terminated (defensive).
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let targets = self.adaptive_registry_targets.clone();
        for name in &targets {
            let target_fn = match self.functions.get(name).copied() {
                Some(f) => f,
                None => continue,
            };
            let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, name, "adapt_reg_name");
            let name_len = i64_ty.const_int(name.len() as u64, false);
            // Cast the user fn's pointer to i8* for the C ABI.
            let fn_ptr_val = target_fn.as_global_value().as_pointer_value();
            let cast_ptr = self.ir
                .builder
                .build_pointer_cast(fn_ptr_val, i8_ptr, "adapt_reg_fn")
                .unwrap();
            let _ = build_wrappers::w_call(
                &self.ir.builder,
                reg_fn,
                &[
                    name_g.into(),
                    name_len.into(),
                    cast_ptr.into(),
                ],
                "",
            );
        }
    }

    /// BUG_HUNT #19: emit one `__axon_register_goal_name(name, len)` per
    /// top-level fn in `main`'s prologue, so native `goal_run` knows the full
    /// set of legitimate target names and can reject a typo'd metric with the
    /// same panic the interpreter raises (I-9 parity) instead of silently
    /// returning `target`. No-op unless the program calls `goal_run`
    /// (`goal_name_targets` is empty), so non-goal programs pay nothing.
    pub(super) fn emit_goal_name_registry_init(&mut self) {
        if self.goal_name_targets.is_empty() {
            return;
        }
        let reg_fn = match self.ir.module.get_function("__axon_register_goal_name") {
            Some(f) => f,
            None => return,
        };
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let names = self.goal_name_targets.clone();
        for name in &names {
            let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, name, "goal_name");
            let name_len = i64_ty.const_int(name.len() as u64, false);
            let _ = build_wrappers::w_call(
                &self.ir.builder,
                reg_fn,
                &[name_g.into(), name_len.into()],
                "",
            );
        }
    }

    /// R4: emit a one-time `__axon_set_provenance_source(path_ptr, path_len)` in
    /// main's prologue so native `@[adaptive]` provenance carries the program's
    /// `"src"` path — parity with the interpreter (`set_provenance_source`).
    /// No-op when the source path is unknown or the runtime extern is absent.
    pub(super) fn emit_provenance_source_init(&mut self) {
        if self.source_path.is_empty() {
            return;
        }
        let set_fn = match self.ir.module.get_function("__axon_set_provenance_source") {
            Some(f) => f,
            None => return,
        };
        if self.ir.builder.get_insert_block().and_then(|b| b.get_terminator()).is_some() {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let path = self.source_path.clone();
        let path_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &path, "prov_src_path");
        let path_len = i64_ty.const_int(path.len() as u64, false);
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            set_fn,
            &[path_g.into(), path_len.into()],
            "",
        );
    }

    // ── Uncertain<T> binary operation emission (ASI Layer 2) ─────────────────
    //
    // V1 design choice: even multiplication uses `min` rather than `c1 * c2`.
    // Multiplicative confidence (joint probability) is more accurate when
    // operands are independent, but it conflates correctness with independence
    // assumptions. Layer-3 will revisit with a proper combinator API once we
    // have provenance tracking. The simple `min` rule keeps confidence
    // monotonically non-increasing through any chain of operations.
    pub(super) fn emit_binop_uncertain(
        &mut self,
        op: &ast::BinOp,
        left: &ast::Expr,
        right: &ast::Expr,
        lt_sem: &Option<Type>,
        rt_sem: &Option<Type>,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        let lhs = self.emit_expr(left, fn_val)?;
        let rhs = self.emit_expr(right, fn_val)?;
        let f64_ty = self.ir.context.f64_type();
        let i64_ty = self.ir.context.i64_type();
        let one_conf = f64_ty.const_float(1.0);

        // Extract (value, confidence) from a side that may or may not be Uncertain.
        // For non-Uncertain sides, confidence defaults to 1.0.
        let extract = |this: &Self,
                       val: BasicValueEnum<'ctx>,
                       sem: &Option<Type>|
         -> Option<(BasicValueEnum<'ctx>, inkwell::values::FloatValue<'ctx>)> {
            if let Some(Type::Uncertain(_)) = sem {
                if let BasicValueEnum::StructValue(sv) = val {
                    let v = this
                        .ir
                        .builder
                        .build_extract_value(sv, 0, "unc_v")
                        .ok()?;
                    let c = this
                        .ir
                        .builder
                        .build_extract_value(sv, 1, "unc_c")
                        .ok()?
                        .into_float_value();
                    return Some((v, c));
                }
                None
            } else {
                Some((val, one_conf))
            }
        };

        let (l_val, l_conf) = extract(self, lhs, lt_sem)?;
        let (r_val, r_conf) = extract(self, rhs, rt_sem)?;

        // min(l_conf, r_conf): select the smaller of the two via OLT compare.
        let cmp = self.ir
            .builder
            .build_float_compare(FloatPredicate::OLT, l_conf, r_conf, "uconf_lt")
            .ok()?;
        let new_conf = self.ir
            .builder
            .build_select(cmp, l_conf, r_conf, "uconf_min")
            .ok()?
            .into_float_value();

        // Determine the inner T from whichever side is Uncertain.
        let inner_ty: Type = match (lt_sem, rt_sem) {
            (Some(Type::Uncertain(t)), _) => *t.clone(),
            (_, Some(Type::Uncertain(t))) => *t.clone(),
            _ => Type::I64,
        };

        // Compute the operation on the underlying values. Reuses `emit_binop`
        // so we get the standard integer/float lowering paths.
        let op_result = self.emit_binop(op, l_val, r_val, &inner_ty);

        // Determine the result struct type. For arithmetic ops the result
        // matches the inner T; for comparisons/logical it is bool.
        let is_cmp = matches!(
            op,
            ast::BinOp::Eq | ast::BinOp::NotEq
                | ast::BinOp::Lt | ast::BinOp::Gt
                | ast::BinOp::LtEq | ast::BinOp::GtEq
                | ast::BinOp::And | ast::BinOp::Or
        );
        let result_inner_ty = if is_cmp { Type::Bool } else { inner_ty.clone() };
        let result_inner_llvm = self.llvm_type(&result_inner_ty)?;
        let result_struct_ty = self.ir.context.struct_type(
            &[result_inner_llvm, f64_ty.into(), i64_ty.into()],
            false,
        );

        // Build { value, confidence, source_tag = 0 }.
        let mut sv = result_struct_ty.get_undef();
        sv = self.ir
            .builder
            .build_insert_value(sv, op_result, 0, "unc_iv")
            .ok()?
            .into_struct_value();
        sv = self.ir
            .builder
            .build_insert_value(sv, new_conf, 1, "unc_ic")
            .ok()?
            .into_struct_value();
        sv = self.ir
            .builder
            .build_insert_value(sv, i64_ty.const_zero(), 2, "unc_is")
            .ok()?
            .into_struct_value();
        Some(sv.into())
    }
}
