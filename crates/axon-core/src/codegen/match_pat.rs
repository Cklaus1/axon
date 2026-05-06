//! Match expression + pattern-test + pattern-binding emission.
//!
//! Phase 2.4 of the §7.5 module split.  These three methods cooperate
//! tightly to compile `match` expressions:
//!   * `emit_match`            walks arms, builds cond/body/merge blocks
//!   * `emit_pattern_test`     emits the `cmp` for each pattern
//!   * `emit_pattern_bindings` introduces locals for variables bound
//!                             inside a pattern (e.g. `Some(x) => …`).
//!
//! All `pub(super)` so the parent `codegen::mod` can call `emit_match`
//! from inside `emit_expr`'s `Expr::Match` arm.

use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::ast;
use crate::types::Type;

impl<'ctx> super::Codegen<'ctx> {
    // ── Match emission ────────────────────────────────────────────────────────

    /// Emit a match expression. Each arm is tested in order with a cond branch;
    /// matching arms jump to their body block. All arms converge via a phi node
    /// in the merge block (if the match produces a value).
    pub(super) fn emit_match(
        &mut self,
        subject: BasicValueEnum<'ctx>,
        arms: &[ast::MatchArm],
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        if arms.is_empty() {
            return None;
        }

        let merge_bb = self.ir.context.append_basic_block(fn_val, "match_merge");
        let mut arm_results: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
            Vec::new();
        // Track the last arm's test block so we can add the false-branch incoming to phi.
        let mut last_test_bb: Option<inkwell::basic_block::BasicBlock<'ctx>> = None;

        for (i, arm) in arms.iter().enumerate() {
            let test_bb = self.ir
                .context
                .append_basic_block(fn_val, &format!("arm{i}_test"));
            let body_bb = self.ir
                .context
                .append_basic_block(fn_val, &format!("arm{i}_body"));
            let next_bb = if i + 1 < arms.len() {
                self.ir.context
                    .append_basic_block(fn_val, &format!("arm{i}_next"))
            } else {
                // Last arm: false branch goes to merge_bb. Track this test_bb.
                last_test_bb = Some(test_bb);
                merge_bb
            };

            self.ir.builder.build_unconditional_branch(test_bb).unwrap();
            self.ir.builder.position_at_end(test_bb);

            // Emit pattern test.
            let matches = self.emit_pattern_test(&arm.pattern, subject);

            // Apply guard if present.
            let final_cond = if let Some(guard_expr) = &arm.guard {
                if let Some(guard_val) = self.emit_expr(guard_expr, fn_val) {
                    if let (
                        BasicValueEnum::IntValue(m),
                        BasicValueEnum::IntValue(g),
                    ) = (matches, guard_val)
                    {
                        self.ir.builder.build_and(m, g, "guarded").unwrap().into()
                    } else {
                        matches
                    }
                } else {
                    matches
                }
            } else {
                matches
            };

            let cond_int = match final_cond {
                BasicValueEnum::IntValue(i) => i,
                _ => self.ir.context.bool_type().const_int(1, false),
            };

            self.ir.builder
                .build_conditional_branch(cond_int, body_bb, next_bb)
                .unwrap();

            // Emit body.
            self.ir.builder.position_at_end(body_bb);
            // Bind pattern variables.
            self.emit_pattern_bindings(&arm.pattern, subject);
            let body_val = self.emit_expr(&arm.body, fn_val);

            let current_bb = self.ir.builder.get_insert_block().unwrap();
            if current_bb.get_terminator().is_none() {
                self.ir.builder.build_unconditional_branch(merge_bb).unwrap();
                // Only add to phi predecessors when this block flows to merge_bb.
                if let Some(v) = body_val {
                    arm_results.push((v, current_bb));
                }
            }
            // Arms with a terminator (e.g., `return`) are NOT phi predecessors.

            if i + 1 < arms.len() {
                self.ir.builder.position_at_end(next_bb);
            }
        }

        self.ir.builder.position_at_end(merge_bb);

        // Build phi if all arms produce a value of the same type.
        // Note: the last arm's test_bb false-branch also goes to merge_bb, so
        // we must add an `undef` incoming for that predecessor to keep the phi valid.
        if arm_results.len() == arms.len() && !arm_results.is_empty() {
            let val_ty = arm_results[0].0.get_type();
            let phi = self.ir.builder.build_phi(val_ty, "match_val").unwrap();
            for (v, bb) in &arm_results {
                phi.add_incoming(&[(v, *bb)]);
            }
            // The last arm's test block (false branch) also flows to merge_bb.
            // LLVM requires all predecessors to have an incoming in the phi.
            // Add an undef value for that predecessor.
            if let Some(last_test_bb) = last_test_bb {
                let undef = val_ty.const_zero(); // Zero is safer than undef for debugging
                phi.add_incoming(&[(&undef, last_test_bb)]);
            }
            Some(phi.as_basic_value())
        } else {
            None
        }
    }

    /// Emit a boolean test for whether `subject` matches `pattern`.
    pub(super) fn emit_pattern_test(
        &mut self,
        pattern: &ast::Pattern,
        subject: BasicValueEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let true_val = self.ir.context.bool_type().const_int(1, false);
        let false_val = self.ir.context.bool_type().const_int(0, false);

        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Ident(_) => true_val.into(),

            ast::Pattern::Literal(lit) => {
                let lit_val = self.emit_literal(lit);
                match (subject, lit_val) {
                    (BasicValueEnum::IntValue(s), BasicValueEnum::IntValue(l)) => self.ir
                        .builder
                        .build_int_compare(IntPredicate::EQ, s, l, "patlit")
                        .unwrap()
                        .into(),
                    (BasicValueEnum::FloatValue(s), BasicValueEnum::FloatValue(l)) => self.ir
                        .builder
                        .build_float_compare(FloatPredicate::OEQ, s, l, "patflit")
                        .unwrap()
                        .into(),
                    // String literal match: use strcmp.
                    (BasicValueEnum::StructValue(subj_sv), BasicValueEnum::StructValue(lit_sv)) => {
                        // Both are { i64, ptr } str structs. Extract data pointers and call strcmp.
                        let strcmp_fn = self.ir.module.get_function("strcmp").unwrap_or_else(|| {
                            let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
                            let strcmp_ty = self.ir.context.i32_type().fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                            self.ir.module.add_function("strcmp", strcmp_ty, None)
                        });
                        let subj_ptr = self.ir.builder.build_extract_value(subj_sv, 1, "subj_ptr")
                            .unwrap().into_pointer_value();
                        let lit_ptr = self.ir.builder.build_extract_value(lit_sv, 1, "lit_ptr")
                            .unwrap().into_pointer_value();
                        let cmp_result = self.ir.builder
                            .build_call(strcmp_fn, &[subj_ptr.into(), lit_ptr.into()], "strcmp_res")
                            .unwrap()
                            .try_as_basic_value().left().unwrap().into_int_value();
                        self.ir.builder
                            .build_int_compare(IntPredicate::EQ, cmp_result, self.ir.context.i32_type().const_zero(), "streq")
                            .unwrap()
                            .into()
                    }
                    _ => true_val.into(),
                }
            }

            ast::Pattern::None => {
                // Check tag == 0.
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let BasicValueEnum::IntValue(tag) =
                        self.ir.builder.build_extract_value(sv, 0, "opttag").unwrap()
                    {
                        return self.ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_zero(),
                                "isnone",
                            )
                            .unwrap()
                            .into();
                    }
                }
                false_val.into()
            }

            ast::Pattern::Some(inner_pat) => {
                // Check tag == 1.
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let BasicValueEnum::IntValue(tag) =
                        self.ir.builder.build_extract_value(sv, 0, "opttag").unwrap()
                    {
                        let is_some = self.ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_int(1, false),
                                "issome",
                            )
                            .unwrap();

                        // Also recurse on the inner value.
                        let inner_val = self.ir
                            .builder
                            .build_extract_value(sv, 1, "optval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner_val);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self.ir
                                .builder
                                .build_and(is_some, im, "somematch")
                                .unwrap()
                                .into();
                        }
                        return is_some.into();
                    }
                }
                false_val.into()
            }

            ast::Pattern::Ok(inner_pat) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let BasicValueEnum::IntValue(tag) =
                        self.ir.builder.build_extract_value(sv, 0, "restag").unwrap()
                    {
                        let is_ok = self.ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_int(1, false),
                                "isok",
                            )
                            .unwrap();
                        let inner = self.ir
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self.ir
                                .builder
                                .build_and(is_ok, im, "okmatch")
                                .unwrap()
                                .into();
                        }
                        return is_ok.into();
                    }
                }
                false_val.into()
            }

            ast::Pattern::Err(inner_pat) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let BasicValueEnum::IntValue(tag) =
                        self.ir.builder.build_extract_value(sv, 0, "restag").unwrap()
                    {
                        let is_err = self.ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_zero(),
                                "iserr",
                            )
                            .unwrap();
                        let inner = self.ir
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self.ir
                                .builder
                                .build_and(is_err, im, "errmatch")
                                .unwrap()
                                .into();
                        }
                        return is_err.into();
                    }
                }
                false_val.into()
            }

            // Enum variant struct pattern: "EnumName::Variant { ... }" — check tag.
            ast::Pattern::Struct { name, .. } if name.contains("::") => {
                let mut parts = name.splitn(2, "::");
                let enum_name = parts.next().unwrap();
                let variant_name = parts.next().unwrap();

                // Find the tag for this variant.
                let tag_int = self.enum_variants
                    .get(enum_name)
                    .and_then(|vs| vs.iter().find(|(vn, _, _)| vn == variant_name))
                    .map(|(_, tag, _)| *tag);

                if let Some(tag_int) = tag_int {
                    // Subject is the enum struct { i32, [N x i8] }.
                    // We need to alloca it to GEP field 0.
                    if let BasicValueEnum::StructValue(sv) = subject {
                        // Extract tag (field 0) — it's an i32.
                        if let Ok(BasicValueEnum::IntValue(tag_val)) =
                            self.ir.builder.build_extract_value(sv, 0, "enumtag")
                        {
                            let expected = tag_val.get_type().const_int(tag_int as u64, false);
                            return self.ir
                                .builder
                                .build_int_compare(IntPredicate::EQ, tag_val, expected, "tagcmp")
                                .unwrap()
                                .into();
                        }
                    }
                }
                false_val.into()
            }

            // Plain struct / tuple patterns: phase 1 — always match (wildcard semantics).
            ast::Pattern::Struct { .. } | ast::Pattern::Tuple(_) => true_val.into(),
        }
    }

    /// Bind pattern variables in the current locals map.
    pub(super) fn emit_pattern_bindings(
        &mut self,
        pattern: &ast::Pattern,
        subject: BasicValueEnum<'ctx>,
    ) {
        match pattern {
            ast::Pattern::Ident(name) => {
                let subject_ty = subject.get_type();
                let alloca = self.ir
                    .builder
                    .build_alloca(subject_ty, name)
                    .unwrap();
                self.ir.builder.build_store(alloca, subject).unwrap();
                self.locals.insert(name.clone(), (alloca, subject_ty));
            }
            ast::Pattern::Some(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let Ok(inner_val) = self.ir.builder.build_extract_value(sv, 1, "patinner") {
                        self.emit_pattern_bindings(inner, inner_val);
                    }
                }
            }
            ast::Pattern::Ok(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let Ok(payload) = self.ir.builder.build_extract_value(sv, 1, "okpayload") {
                        let typed = if let Some((ok_ty, _)) = self.current_result_types.clone() {
                            self.extract_result_payload(payload, &ok_ty)
                        } else {
                            Some(payload)
                        };
                        if let Some(v) = typed {
                            self.emit_pattern_bindings(inner, v);
                        }
                    }
                }
            }
            ast::Pattern::Err(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let Ok(payload) = self.ir.builder.build_extract_value(sv, 1, "errpayload") {
                        let typed = if let Some((_, err_ty)) = self.current_result_types.clone() {
                            self.extract_result_payload(payload, &err_ty)
                        } else {
                            Some(payload)
                        };
                        if let Some(v) = typed {
                            self.emit_pattern_bindings(inner, v);
                        }
                    }
                }
            }
            ast::Pattern::Struct { name, fields } if name.contains("::") => {
                // Enum variant pattern bindings.
                // Extract field values from the payload of the enum struct { i32, [N x i8] }.
                if fields.is_empty() {
                    return;
                }

                let mut parts = name.splitn(2, "::");
                let enum_name = parts.next().unwrap().to_string();
                let variant_name = parts.next().unwrap().to_string();

                let field_types = self.enum_variants
                    .get(&enum_name)
                    .and_then(|vs| vs.iter().find(|(vn, _, _)| vn == &variant_name))
                    .map(|(_, _, fts)| fts.clone());

                let field_types = match field_types {
                    Some(ft) => ft,
                    None => return,
                };

                if let BasicValueEnum::StructValue(sv) = subject {
                    // Alloca the enum struct so we can GEP into it.
                    let struct_name = format!("{enum_name}_enum");
                    let enum_struct_ty = match self.ir.module.get_struct_type(&struct_name) {
                        Some(ty) => ty,
                        None => return,
                    };
                    let alloca = self.ir.builder.build_alloca(enum_struct_ty, "enumtmp").unwrap();
                    self.ir.builder.build_store(alloca, sv).unwrap();

                    // GEP to payload field (index 1).
                    let pay_ptr = self.ir.builder
                        .build_struct_gep(enum_struct_ty, alloca, 1, "pay")
                        .unwrap();

                    let i8_ty = self.ir.context.i8_type();
                    let i32_ty = self.ir.context.i32_type();
                    let ptr_ty = i8_ty.ptr_type(AddressSpace::default());

                    let pay_i8ptr = self.ir.builder
                        .build_pointer_cast(pay_ptr, ptr_ty, "payi8ptr")
                        .unwrap();

                    // For each bound field, compute byte offset in payload.
                    let mut byte_offset: u64 = 0;
                    for (fi, (_fname, pat)) in fields.iter().enumerate() {
                        let fty = field_types.get(fi).cloned().unwrap_or(Type::Unknown);
                        let fsize = self.llvm_sizeof(&fty).unwrap_or(8);

                        if let Some(llvm_fty) = self.llvm_type(&fty) {
                            let offset_val = i32_ty.const_int(byte_offset, false);
                            let field_ptr = unsafe {
                                self.ir.builder
                                    .build_gep(i8_ty, pay_i8ptr, &[offset_val], "fieldptr")
                                    .unwrap()
                            };
                            let typed_ptr = self.ir.builder
                                .build_pointer_cast(field_ptr, ptr_ty, "tfptr")
                                .unwrap();
                            let field_val = self.ir.builder
                                .build_load(llvm_fty, typed_ptr, "fieldval")
                                .unwrap();
                            self.emit_pattern_bindings(pat, field_val);
                        }

                        byte_offset += fsize;
                    }
                }
            }
            ast::Pattern::Struct { fields, .. } => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    for (i, (_fname, pat)) in fields.iter().enumerate() {
                        if let Ok(field_val) =
                            self.ir.builder.build_extract_value(sv, i as u32, "sfield")
                        {
                            self.emit_pattern_bindings(pat, field_val);
                        }
                    }
                }
            }
            ast::Pattern::Tuple(pats) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    for (i, pat) in pats.iter().enumerate() {
                        if let Ok(elem_val) =
                            self.ir.builder.build_extract_value(sv, i as u32, "telem")
                        {
                            self.emit_pattern_bindings(pat, elem_val);
                        }
                    }
                }
            }
            _ => {} // Wildcard, Literal, None — no bindings
        }
    }

}
