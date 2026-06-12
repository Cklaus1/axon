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
        let Some(agent_fn) = self.current_agent_fn.clone() else {
            return;
        };
        let Some(caps) = crate::capabilities::capability_of_builtin(action) else {
            return;
        };
        let log_fn = match self.ir.module.get_function("__axon_log_agent_action") {
            Some(f) => f,
            None => return,
        };
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
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
                fn_g.into(),
                i64_ty.const_int(agent_fn.len() as u64, false).into(),
                act_g.into(),
                i64_ty.const_int(action.len() as u64, false).into(),
                cap_g.into(),
                i64_ty.const_int(caps.len() as u64, false).into(),
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
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let name_g = build_wrappers::w_global_string_ptr(&self.ir.builder, fn_name, "prov_fn_name");
        let evt_g = build_wrappers::w_global_string_ptr(&self.ir.builder, event, "prov_event");
        let name_len = i64_ty.const_int(fn_name.len() as u64, false);
        let evt_len = i64_ty.const_int(event.len() as u64, false);
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            prov_fn,
            &[name_g.into(), name_len.into(), evt_g.into(), evt_len.into()],
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
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
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
                    if let Some(rt) = self
                        .ir
                        .module
                        .get_function("__axon_provenance_log_ret_i64_in")
                    {
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
        let (fn_name, ident, op_str, bound) = match self.current_verify_fn.clone() {
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
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
            return;
        }

        let f64_ty = self.ir.context.f64_type();
        // The compared `actual` depends on the predicate ident and return shape:
        //  - `confidence` (Uncertain field 1, f64) — the original path.
        //  - `value` on an Uncertain return — field 0, widened to f64.
        //  - `value` on a SCALAR return — the return value itself (int→f64).
        let actual: inkwell::values::FloatValue<'ctx> = match ret_val {
            BasicValueEnum::StructValue(sv) => {
                let idx = if ident == "confidence" { 1 } else { 0 };
                let ev = build_wrappers::w_extract_value(&self.ir.builder, sv, idx, "verify_field");
                match ev {
                    BasicValueEnum::FloatValue(fv) if fv.get_type() == f64_ty => fv,
                    BasicValueEnum::IntValue(iv) => self
                        .ir
                        .builder
                        .build_signed_int_to_float(iv, f64_ty, "verify_val_f")
                        .unwrap(),
                    _ => return,
                }
            }
            // Scalar return (`value` predicate on i64/i32/bool/f64).
            BasicValueEnum::IntValue(iv) => self
                .ir
                .builder
                .build_signed_int_to_float(iv, f64_ty, "verify_scalar_f")
                .unwrap(),
            BasicValueEnum::FloatValue(fv) if fv.get_type() == f64_ty => fv,
            _ => return,
        };

        // Translate operator string → LLVM float predicate.  Matches the set
        // accepted by `verify::decode_verify_predicate`.
        let pred = match op_str {
            ">=" => FloatPredicate::OGE,
            ">" => FloatPredicate::OGT,
            "<=" => FloatPredicate::OLE,
            "<" => FloatPredicate::OLT,
            "==" => FloatPredicate::OEQ,
            "!=" => FloatPredicate::ONE,
            // Unknown op string: silently no-op (mirrors static checker).
            _ => return,
        };

        let bound_const = f64_ty.const_float(bound);
        let cmp = build_wrappers::w_float_compare(
            &self.ir.builder,
            pred,
            actual,
            bound_const,
            "verify_cmp",
        );

        // Build branch: cmp ? continue : panic.  We append two blocks to the
        // current function and route the *current* block into them.
        let panic_bb = self.ir.context.append_basic_block(llvm_fn, "verify_panic");
        let cont_bb = self.ir.context.append_basic_block(llvm_fn, "verify_ok");

        build_wrappers::w_cond_br(&self.ir.builder, cmp, cont_bb, panic_bb);

        // ── Panic path ────────────────────────────────────────────────────
        self.ir.builder.position_at_end(panic_bb);
        let i64_ty = self.ir.context.i64_type();
        let name_g =
            build_wrappers::w_global_string_ptr(&self.ir.builder, &fn_name, "verify_fn_name");
        let op_g = build_wrappers::w_global_string_ptr(&self.ir.builder, op_str, "verify_op");
        let name_len = i64_ty.const_int(fn_name.len() as u64, false);
        let op_len = i64_ty.const_int(op_str.len() as u64, false);
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
        build_wrappers::w_unreachable(&self.ir.builder);

        // ── Continue path: original return falls through here. ────────────
        self.ir.builder.position_at_end(cont_bb);
    }

    /// Phase 5: emit the runtime refinement-PRECONDITION check for each refined
    /// parameter at function entry. For `p: T where P`, evaluate `P` with `_`
    /// aliased to the parameter's local; on `false`, branch to a panic block that
    /// calls `__axon_refine_panic(fn, param, refine)` (exit 6). This is the
    /// codegen analog of the interpreter's `call_fn` entry check — both engines
    /// enforce the same predicate subset, so the observable exit code agrees
    /// (I-2). A predicate outside the lowerable subset is E0910-refused (never
    /// silently skipped — that would let native accept a value the interpreter
    /// rejects). Call only when `self.refine_preds` is non-empty.
    pub(super) fn emit_refine_preconditions(
        &mut self,
        f: &ast::FnDef,
        llvm_fn: FunctionValue<'ctx>,
    ) {
        let panic_fn = match self.ir.module.get_function("__axon_refine_panic") {
            Some(f) => f,
            None => return,
        };
        for param in &f.params {
            let ast::AxonType::Named(rname) = &param.ty else {
                continue;
            };
            let Some(pred) = self.refine_preds.get(rname.as_str()).cloned() else {
                continue;
            };

            // Bail out honestly if the predicate is outside the subset BOTH
            // engines can evaluate — refusing (E0910) rather than skipping the
            // check keeps native from silently accepting an out-of-contract value.
            if !Self::refine_predicate_is_lowerable(&pred, &self.fndefs) {
                let msg = format!(
                    "codegen error [E0910]: native codegen cannot lower the refinement \
                     predicate of `{rname}` on parameter `{}` of `{}` — it is outside the \
                     runtime-checkable subset (literals, `_`, arithmetic, comparisons, \
                     `&&`/`||`/`!`, `str_len`/`str_eq`, `_.field`, and calls to @[pure] \
                     functions). Run it under the interpreter (`axon run`).",
                    param.name, f.name
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
                continue;
            }

            // Don't insert into a terminated block (defensive).
            if self
                .ir
                .builder
                .get_insert_block()
                .and_then(|b| b.get_terminator())
                .is_some()
            {
                return;
            }

            // Alias `_` to this parameter's local for the predicate emission, then
            // restore so the binder never leaks into the function body. The param
            // is already in `self.locals`/`self.local_types` from the binding loop.
            let saved_local = self.locals.get(&param.name).copied();
            let saved_type = self.local_types.get(&param.name).cloned();
            let prev_underscore_local = self.locals.remove("_");
            let prev_underscore_type = self.local_types.remove("_");
            if let Some(l) = saved_local {
                self.locals.insert("_".to_string(), l);
            }
            if let Some(t) = saved_type {
                self.local_types.insert("_".to_string(), t);
            }

            let pred_val = self.emit_expr(&pred, llvm_fn);

            // Restore `_`.
            self.locals.remove("_");
            self.local_types.remove("_");
            if let Some(l) = prev_underscore_local {
                self.locals.insert("_".to_string(), l);
            }
            if let Some(t) = prev_underscore_type {
                self.local_types.insert("_".to_string(), t);
            }

            let cond = match pred_val {
                Some(BasicValueEnum::IntValue(iv)) if iv.get_type().get_bit_width() == 1 => iv,
                // A predicate that didn't lower to an i1 — defensive; refuse.
                _ => {
                    let msg = format!(
                        "codegen error [E0910]: refinement predicate of `{rname}` on parameter \
                         `{}` of `{}` did not lower to a boolean. Run it under the interpreter.",
                        param.name, f.name
                    );
                    if !self.codegen_errors.iter().any(|e| e == &msg) {
                        self.codegen_errors.push(msg);
                    }
                    continue;
                }
            };

            // cond ? continue : panic.
            let panic_bb = self.ir.context.append_basic_block(llvm_fn, "refine_panic");
            let cont_bb = self.ir.context.append_basic_block(llvm_fn, "refine_ok");
            build_wrappers::w_cond_br(&self.ir.builder, cond, cont_bb, panic_bb);

            // ── Panic path: name the violation, exit 6. ───────────────────────
            self.ir.builder.position_at_end(panic_bb);
            let i64_ty = self.ir.context.i64_type();
            let fn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &f.name, "refine_fn");
            let pn_g =
                build_wrappers::w_global_string_ptr(&self.ir.builder, &param.name, "refine_param");
            let rn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, rname, "refine_name");
            let _ = build_wrappers::w_call(
                &self.ir.builder,
                panic_fn,
                &[
                    fn_g.into(),
                    i64_ty.const_int(f.name.len() as u64, false).into(),
                    pn_g.into(),
                    i64_ty.const_int(param.name.len() as u64, false).into(),
                    rn_g.into(),
                    i64_ty.const_int(rname.len() as u64, false).into(),
                ],
                "",
            );
            build_wrappers::w_unreachable(&self.ir.builder);

            // ── Continue path: subsequent params / the body fall through here. ─
            self.ir.builder.position_at_end(cont_bb);
        }
    }

    /// Phase 5: emit the runtime refinement-POSTCONDITION check at a return site.
    /// The dual of `emit_refine_preconditions`: when the current fn's declared
    /// return type is a refinement (`current_ret_refine` set in `emit_fn`),
    /// evaluate the predicate with `_` bound to the about-to-be-returned value;
    /// on `false`, call `__axon_refine_panic` (exit 6). Call immediately BEFORE
    /// each `w_ret(value)` (alongside `emit_verify_check_if_needed`). No-op when
    /// the return is not a refinement. The value is an SSA result, so it is
    /// spilled to a short-lived alloca that `_` aliases for the predicate emit.
    pub(super) fn emit_refine_return_check_if_needed(
        &mut self,
        ret_val: BasicValueEnum<'ctx>,
        llvm_fn: FunctionValue<'ctx>,
    ) {
        let Some((rname, pred)) = self.current_ret_refine.clone() else {
            return;
        };
        let panic_fn = match self.ir.module.get_function("__axon_refine_panic") {
            Some(f) => f,
            None => return,
        };
        // Don't insert into an already-terminated block.
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
            return;
        }

        // Spill the SSA return value to a temp alloca and alias `_` to it for the
        // predicate emission; restore `_` afterwards so nothing leaks.
        let val_ty = ret_val.get_type();
        let slot = build_wrappers::w_alloca(&self.ir.builder, val_ty, "refine_ret_tmp");
        build_wrappers::w_store(&self.ir.builder, slot, ret_val);
        // Map the SSA value's LLVM type to the semantic type the binder needs.
        // The return value is a scalar/str in the lowerable subset; reuse the
        // declared return semantic type if available, else fall back to the LLVM
        // type's natural sem mapping via local_types of `_` being absent.
        let sem = self
            .fn_return_types
            .get(llvm_fn.get_name().to_str().unwrap_or(""))
            .cloned();

        let prev_underscore_local = self.locals.remove("_");
        let prev_underscore_type = self.local_types.remove("_");
        self.locals.insert("_".to_string(), (slot, val_ty));
        if let Some(s) = sem {
            self.local_types.insert("_".to_string(), s);
        }

        let pred_val = self.emit_expr(&pred, llvm_fn);

        // Restore `_`.
        self.locals.remove("_");
        self.local_types.remove("_");
        if let Some(l) = prev_underscore_local {
            self.locals.insert("_".to_string(), l);
        }
        if let Some(t) = prev_underscore_type {
            self.local_types.insert("_".to_string(), t);
        }

        let cond = match pred_val {
            Some(BasicValueEnum::IntValue(iv)) if iv.get_type().get_bit_width() == 1 => iv,
            _ => {
                // Defensive: predicate didn't lower to i1. Arming already proved
                // lowerability, so this is unexpected; refuse rather than miscompile.
                let msg = format!(
                    "codegen error [E0910]: refinement return predicate of `{rname}` did not \
                     lower to a boolean. Run it under the interpreter."
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
                return;
            }
        };

        let fn_name = llvm_fn.get_name().to_str().unwrap_or("?").to_string();
        let panic_bb = self
            .ir
            .context
            .append_basic_block(llvm_fn, "refine_ret_panic");
        let cont_bb = self.ir.context.append_basic_block(llvm_fn, "refine_ret_ok");
        build_wrappers::w_cond_br(&self.ir.builder, cond, cont_bb, panic_bb);

        // ── Panic path: name the violation, exit 6. ───────────────────────────
        self.ir.builder.position_at_end(panic_bb);
        let i64_ty = self.ir.context.i64_type();
        let param_label = "<return>";
        let fn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &fn_name, "refine_ret_fn");
        let pn_g =
            build_wrappers::w_global_string_ptr(&self.ir.builder, param_label, "refine_ret_slot");
        let rn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, &rname, "refine_ret_name");
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            panic_fn,
            &[
                fn_g.into(),
                i64_ty.const_int(fn_name.len() as u64, false).into(),
                pn_g.into(),
                i64_ty.const_int(param_label.len() as u64, false).into(),
                rn_g.into(),
                i64_ty.const_int(rname.len() as u64, false).into(),
            ],
            "",
        );
        build_wrappers::w_unreachable(&self.ir.builder);

        // ── Continue path: the original w_ret falls through here. ─────────────
        self.ir.builder.position_at_end(cont_bb);
    }

    /// Phase 5: emit the runtime refinement checks at STRUCT construction — the
    /// codegen dual of the interp StructLit checks. For each refined field, load
    /// it and check its predicate with `_` aliased to the field value; then, if
    /// the struct has a whole-struct `where` predicate, check it with `_` aliased
    /// to the assembled struct alloca (so `_.field` projects via GEP). A violation
    /// calls `__axon_refine_panic` (exit 6). Predicates outside the lowerable
    /// subset are E0910-refused (never silently skipped — that breaks I-2). `name`
    /// is the struct type; `alloca`/`struct_ty` hold the just-built instance.
    pub(super) fn emit_refine_struct_checks(
        &mut self,
        name: &str,
        alloca: inkwell::values::PointerValue<'ctx>,
        struct_ty: inkwell::types::StructType<'ctx>,
        field_names: &[String],
        field_sem_types: &[Type],
        llvm_fn: FunctionValue<'ctx>,
    ) {
        let has_fields = self.struct_field_refines.contains_key(name);
        let has_whole = self.struct_whole_refines.contains_key(name);
        if !has_fields && !has_whole {
            return;
        }
        if self.ir.module.get_function("__axon_refine_panic").is_none() {
            return;
        }

        // ── Per-field refinements ─────────────────────────────────────────────
        if let Some(refs) = self.struct_field_refines.get(name).cloned() {
            for (fname, rn) in refs {
                let Some(pred) = self.refine_preds.get(&rn).cloned() else {
                    continue;
                };
                if !Self::refine_predicate_is_lowerable(&pred, &self.fndefs) {
                    self.refuse_struct_refine(name, &fname, &rn);
                    continue;
                }
                let Some(idx) = field_names.iter().position(|n| n == &fname) else {
                    continue;
                };
                if self.block_terminated() {
                    return;
                }
                // Load the field value into a temp slot, alias `_` to it.
                let fptr = self
                    .ir
                    .builder
                    .build_struct_gep(struct_ty, alloca, idx as u32, "refine_fld")
                    .unwrap();
                let fty = struct_ty.get_field_type_at_index(idx as u32).unwrap();
                let fval = build_wrappers::w_load(&self.ir.builder, fty, fptr, "refine_fldv");
                let slot = build_wrappers::w_alloca(&self.ir.builder, fty, "refine_fld_tmp");
                build_wrappers::w_store(&self.ir.builder, slot, fval);
                let sem = field_sem_types.get(idx).cloned();

                let label = format!("field `{fname}` of `{name}` (refinement `{rn}`)");
                let cond =
                    self.emit_refine_pred_with_underscore(&pred, (slot, fty), sem, &label, llvm_fn);
                if let Some(cond) = cond {
                    self.emit_refine_branch(cond, name, &fname, &rn, llvm_fn, "refine_fld");
                }
            }
        }

        // ── Whole-struct refinement (`_` = the instance, `_.field` projects) ──
        if let Some(pred) = self.struct_whole_refines.get(name).cloned() {
            if !Self::refine_predicate_is_lowerable(&pred, &self.fndefs) {
                self.refuse_struct_refine(name, "<struct>", name);
                return;
            }
            if self.block_terminated() {
                return;
            }
            // Alias `_` to the struct alloca with semantic type Struct(name) so
            // `emit_field_access` lowers `_.lo`/`_.hi` via its GEP path.
            let cond = self.emit_refine_pred_with_underscore(
                &pred,
                (alloca, struct_ty.into()),
                Some(Type::Struct(name.to_string())),
                &format!("whole-struct `{name}`"),
                llvm_fn,
            );
            if let Some(cond) = cond {
                self.emit_refine_branch(cond, name, "<struct>", name, llvm_fn, "refine_struct");
            }
        }
    }

    /// Phase 5: emit the runtime check for a `let p: T where P = …` binding — the
    /// codegen dual of the interp Let check. `slot`/`val_ty` hold the bound value
    /// (its alloca). A violation calls `__axon_refine_panic` (exit 6). Out-of-
    /// subset predicates are E0910-refused.
    pub(super) fn emit_refine_let_check(
        &mut self,
        binding: &str,
        rname: &str,
        slot: inkwell::values::PointerValue<'ctx>,
        val_ty: inkwell::types::BasicTypeEnum<'ctx>,
        sem: Option<Type>,
        llvm_fn: FunctionValue<'ctx>,
    ) {
        let Some(pred) = self.refine_preds.get(rname).cloned() else {
            return;
        };
        if self.ir.module.get_function("__axon_refine_panic").is_none() {
            return;
        }
        if !Self::refine_predicate_is_lowerable(&pred, &self.fndefs) {
            let msg = format!(
                "codegen error [E0910]: native codegen cannot lower the refinement predicate of \
                 `{rname}` on the let-binding `{binding}` — it is outside the runtime-checkable \
                 subset. Run it under the interpreter (`axon run`)."
            );
            if !self.codegen_errors.iter().any(|e| e == &msg) {
                self.codegen_errors.push(msg);
            }
            return;
        }
        if self.block_terminated() {
            return;
        }
        let fn_name = llvm_fn.get_name().to_str().unwrap_or("?").to_string();
        let cond = self.emit_refine_pred_with_underscore(
            &pred,
            (slot, val_ty),
            sem,
            &format!("let `{binding}`"),
            llvm_fn,
        );
        if let Some(cond) = cond {
            self.emit_refine_branch(cond, &fn_name, binding, rname, llvm_fn, "refine_let");
        }
    }

    /// Shared: alias `_` to `(slot, ty)` with semantic type `sem`, emit `pred`,
    /// restore `_`, and return the i1 condition (or push E0910 and return None if
    /// it didn't lower to a boolean). `label` is only for the E0910 message.
    fn emit_refine_pred_with_underscore(
        &mut self,
        pred: &ast::Expr,
        binder: (
            inkwell::values::PointerValue<'ctx>,
            inkwell::types::BasicTypeEnum<'ctx>,
        ),
        sem: Option<Type>,
        label: &str,
        llvm_fn: FunctionValue<'ctx>,
    ) -> Option<inkwell::values::IntValue<'ctx>> {
        let prev_local = self.locals.remove("_");
        let prev_type = self.local_types.remove("_");
        self.locals.insert("_".to_string(), binder);
        if let Some(s) = sem {
            self.local_types.insert("_".to_string(), s);
        }

        let pred_val = self.emit_expr(pred, llvm_fn);

        self.locals.remove("_");
        self.local_types.remove("_");
        if let Some(l) = prev_local {
            self.locals.insert("_".to_string(), l);
        }
        if let Some(t) = prev_type {
            self.local_types.insert("_".to_string(), t);
        }

        match pred_val {
            Some(BasicValueEnum::IntValue(iv)) if iv.get_type().get_bit_width() == 1 => Some(iv),
            _ => {
                let msg = format!(
                    "codegen error [E0910]: refinement predicate for {label} did not lower to a \
                     boolean. Run it under the interpreter."
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
                None
            }
        }
    }

    /// Shared: branch on `cond` — continue if true, else call `__axon_refine_panic`
    /// (exit 6) naming `fn_name`/`slot`/`rname` and `unreachable`.
    fn emit_refine_branch(
        &mut self,
        cond: inkwell::values::IntValue<'ctx>,
        fn_name: &str,
        slot: &str,
        rname: &str,
        llvm_fn: FunctionValue<'ctx>,
        tag: &str,
    ) {
        let panic_fn = self.ir.module.get_function("__axon_refine_panic").unwrap();
        let panic_bb = self
            .ir
            .context
            .append_basic_block(llvm_fn, &format!("{tag}_panic"));
        let cont_bb = self
            .ir
            .context
            .append_basic_block(llvm_fn, &format!("{tag}_ok"));
        build_wrappers::w_cond_br(&self.ir.builder, cond, cont_bb, panic_bb);

        self.ir.builder.position_at_end(panic_bb);
        let i64_ty = self.ir.context.i64_type();
        let fn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, fn_name, "refine_fn");
        let pn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, slot, "refine_slot");
        let rn_g = build_wrappers::w_global_string_ptr(&self.ir.builder, rname, "refine_name");
        let _ = build_wrappers::w_call(
            &self.ir.builder,
            panic_fn,
            &[
                fn_g.into(),
                i64_ty.const_int(fn_name.len() as u64, false).into(),
                pn_g.into(),
                i64_ty.const_int(slot.len() as u64, false).into(),
                rn_g.into(),
                i64_ty.const_int(rname.len() as u64, false).into(),
            ],
            "",
        );
        build_wrappers::w_unreachable(&self.ir.builder);
        self.ir.builder.position_at_end(cont_bb);
    }

    /// Push an E0910 for a struct refinement codegen can't lower.
    fn refuse_struct_refine(&mut self, sname: &str, slot: &str, rname: &str) {
        let msg = format!(
            "codegen error [E0910]: native codegen cannot lower the refinement predicate of \
             `{rname}` on `{slot}` of struct `{sname}` — it is outside the runtime-checkable \
             subset. Run it under the interpreter (`axon run`)."
        );
        if !self.codegen_errors.iter().any(|e| e == &msg) {
            self.codegen_errors.push(msg);
        }
    }

    /// True if the current insert block already has a terminator.
    fn block_terminated(&self) -> bool {
        self.ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
    }

    /// Is `pred` within the predicate subset native codegen can faithfully lower
    /// at function entry — the SAME subset the interpreter evaluates, so the two
    /// engines agree (I-2)? Accepts: int/bool/str literals; the binder `_`;
    /// checked arithmetic `+ - * / %`; comparisons; `&& || !`; `_.field`;
    /// `str_len(_)` / `str_eq(...)`; calls to `@[pure]` user fns; and `if`/block
    /// forms reducing to those. Anything else (impure builtins are already
    /// E1209-rejected statically) is out of subset → the caller E0910-refuses.
    pub(super) fn refine_predicate_is_lowerable(
        pred: &ast::Expr,
        fndefs: &std::collections::HashMap<String, ast::FnDef>,
    ) -> bool {
        match pred {
            ast::Expr::Literal(_) => true,
            ast::Expr::Ident(_) => true, // `_` (and, inside a @[pure] body, its params)
            ast::Expr::UnaryOp { operand, .. } => {
                Self::refine_predicate_is_lowerable(operand, fndefs)
            }
            ast::Expr::BinOp { left, right, .. } => {
                Self::refine_predicate_is_lowerable(left, fndefs)
                    && Self::refine_predicate_is_lowerable(right, fndefs)
            }
            ast::Expr::FieldAccess { receiver, .. } => {
                Self::refine_predicate_is_lowerable(receiver, fndefs)
            }
            ast::Expr::If { cond, then, else_ } => {
                Self::refine_predicate_is_lowerable(cond, fndefs)
                    && Self::refine_predicate_is_lowerable(then, fndefs)
                    && else_
                        .as_ref()
                        .map(|e| Self::refine_predicate_is_lowerable(e, fndefs))
                        .unwrap_or(true)
            }
            ast::Expr::Block(stmts) => stmts.iter().all(|s| match &s.expr {
                ast::Expr::Let { value, .. }
                | ast::Expr::Own { value, .. }
                | ast::Expr::RefBind { value, .. } => {
                    Self::refine_predicate_is_lowerable(value, fndefs)
                }
                other => Self::refine_predicate_is_lowerable(other, fndefs),
            }),
            ast::Expr::Call { callee, args, .. } => {
                // Only direct calls to a known builtin in the pure-predicate set,
                // or to a user `@[pure]` fn, with all args lowerable.
                let ast::Expr::Ident(name) = callee.as_ref() else {
                    return false;
                };
                let args_ok = args
                    .iter()
                    .all(|a| Self::refine_predicate_is_lowerable(a, fndefs));
                if !args_ok {
                    return false;
                }
                if matches!(name.as_str(), "str_len" | "str_eq") {
                    return true;
                }
                // A user @[pure] fn (the same forms the E1209 const-folder allows).
                fndefs
                    .get(name)
                    .map(|d| {
                        d.attrs.iter().any(|a| a.name == "pure")
                            && Self::refine_predicate_is_lowerable(&d.body, fndefs)
                    })
                    .unwrap_or(false)
            }
            _ => false,
        }
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
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
            return;
        }
        let i64_ty = self.ir.context.i64_type();
        let i8_ptr = self
            .ir
            .context
            .i8_type()
            .ptr_type(inkwell::AddressSpace::default());
        let targets = self.adaptive_registry_targets.clone();
        for name in &targets {
            let target_fn = match self.functions.get(name).copied() {
                Some(f) => f,
                None => continue,
            };
            let name_g =
                build_wrappers::w_global_string_ptr(&self.ir.builder, name, "adapt_reg_name");
            let name_len = i64_ty.const_int(name.len() as u64, false);
            // Cast the user fn's pointer to i8* for the C ABI.
            let fn_ptr_val = target_fn.as_global_value().as_pointer_value();
            let cast_ptr = self
                .ir
                .builder
                .build_pointer_cast(fn_ptr_val, i8_ptr, "adapt_reg_fn")
                .unwrap();
            let _ = build_wrappers::w_call(
                &self.ir.builder,
                reg_fn,
                &[name_g.into(), name_len.into(), cast_ptr.into()],
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
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
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
    /// Emit a call (in `main`'s prologue) to install the native stack-overflow
    /// guard, so deep recursion exits 101 gracefully instead of SIGSEGV-139.
    /// No-op on wasm (no signals); the runtime fn itself is a no-op on non-unix.
    pub(super) fn emit_recursion_guard_init(&mut self) {
        if self.target_is_wasm {
            return;
        }
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
            return;
        }
        let void_ty = self.ir.context.void_type();
        let fn_ty = void_ty.fn_type(&[], false);
        let f = self
            .ir
            .module
            .get_function("__axon_install_recursion_guard")
            .unwrap_or_else(|| {
                self.ir
                    .module
                    .add_function("__axon_install_recursion_guard", fn_ty, None)
            });
        let _ = build_wrappers::w_call(&self.ir.builder, f, &[], "");
    }

    pub(super) fn emit_provenance_source_init(&mut self) {
        if self.source_path.is_empty() {
            return;
        }
        let set_fn = match self.ir.module.get_function("__axon_set_provenance_source") {
            Some(f) => f,
            None => return,
        };
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_some()
        {
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
        let extract =
            |this: &Self,
             val: BasicValueEnum<'ctx>,
             sem: &Option<Type>|
             -> Option<(BasicValueEnum<'ctx>, inkwell::values::FloatValue<'ctx>)> {
                if let Some(Type::Uncertain(_)) = sem {
                    if let BasicValueEnum::StructValue(sv) = val {
                        let v = build_wrappers::w_extract_value(&this.ir.builder, sv, 0, "unc_v");
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
        let cmp = self
            .ir
            .builder
            .build_float_compare(FloatPredicate::OLT, l_conf, r_conf, "uconf_lt")
            .ok()?;
        let new_conf = self
            .ir
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
            ast::BinOp::Eq
                | ast::BinOp::NotEq
                | ast::BinOp::Lt
                | ast::BinOp::Gt
                | ast::BinOp::LtEq
                | ast::BinOp::GtEq
                | ast::BinOp::And
                | ast::BinOp::Or
        );
        let result_inner_ty = if is_cmp { Type::Bool } else { inner_ty.clone() };
        let result_inner_llvm = self.llvm_type(&result_inner_ty)?;
        let result_struct_ty = self
            .ir
            .context
            .struct_type(&[result_inner_llvm, f64_ty.into(), i64_ty.into()], false);

        // Build { value, confidence, source_tag = 0 }.
        let mut sv = result_struct_ty.get_undef();
        sv = self
            .ir
            .builder
            .build_insert_value(sv, op_result, 0, "unc_iv")
            .ok()?
            .into_struct_value();
        sv = self
            .ir
            .builder
            .build_insert_value(sv, new_conf, 1, "unc_ic")
            .ok()?
            .into_struct_value();
        sv = self
            .ir
            .builder
            .build_insert_value(sv, i64_ty.const_zero(), 2, "unc_is")
            .ok()?
            .into_struct_value();
        Some(sv.into())
    }
}
