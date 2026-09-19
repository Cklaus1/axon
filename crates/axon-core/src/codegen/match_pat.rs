//! Match expression + pattern-test + pattern-binding emission.
//!
//! Phase 2.4 of the §7.5 module split.  These three methods cooperate
//! tightly to compile `match` expressions:
//! - `emit_match`            walks arms, builds cond/body/merge blocks
//! - `emit_pattern_test`     emits the `cmp` for each pattern
//! - `emit_pattern_bindings` introduces locals for variables bound inside a
//!   pattern (e.g. `Some(x) => …`).
//!
//! All `pub(super)` so the parent `codegen::mod` can call `emit_match`
//! from inside `emit_expr`'s `Expr::Match` arm.

use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::ast;
use crate::types::Type;

use super::build_wrappers;

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
        subject_sem_ty: Option<&Type>,
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
            let test_bb = self
                .ir
                .context
                .append_basic_block(fn_val, &format!("arm{i}_test"));
            let body_bb = self
                .ir
                .context
                .append_basic_block(fn_val, &format!("arm{i}_body"));
            let next_bb = if i + 1 < arms.len() {
                self.ir
                    .context
                    .append_basic_block(fn_val, &format!("arm{i}_next"))
            } else {
                // Last arm: false branch goes to merge_bb. Track this test_bb.
                last_test_bb = Some(test_bb);
                merge_bb
            };

            build_wrappers::w_br(&self.ir.builder, test_bb);
            self.ir.builder.position_at_end(test_bb);

            // Emit pattern test.
            let matches = self.emit_pattern_test(&arm.pattern, subject);

            // Apply guard if present.
            let final_cond = if let Some(guard_expr) = &arm.guard {
                if let Some(guard_val) = self.emit_expr(guard_expr, fn_val) {
                    if let (BasicValueEnum::IntValue(m), BasicValueEnum::IntValue(g)) =
                        (matches, guard_val)
                    {
                        build_wrappers::w_and(&self.ir.builder, m, g, "guarded").into()
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

            build_wrappers::w_cond_br(&self.ir.builder, cond_int, body_bb, next_bb);

            // Emit body.
            self.ir.builder.position_at_end(body_bb);
            // Bind pattern variables.
            self.emit_pattern_bindings(&arm.pattern, subject, subject_sem_ty);
            let body_val = self.emit_expr(&arm.body, fn_val);

            let current_bb = self.ir.builder.get_insert_block().unwrap();
            if current_bb.get_terminator().is_none() {
                build_wrappers::w_br(&self.ir.builder, merge_bb);
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
            // A phi's operands must all have the RESULT type. This built the
            // phi from arm 0 and added the rest regardless, so a match whose
            // arms disagree emitted invalid IR and died in the verifier with
            // "PHI node operands are not the same type as the result!" --- a
            // message that names neither the match nor the file.
            //
            // The shape that gets here: native `dict_get` always yields
            // `Option<i64>` (the v1 dict is int-valued), so
            // `match dict_get(d, k) { Some(v) => v  None => 0.0 }` --- which
            // the interpreter runs fine over an f64-valued dict --- gives an
            // i64 arm and an f64 arm. Codegen cannot know a dict's value type
            // statically, so it cannot fix the Option; what it CAN do is say
            // so. Every program reaching here already failed to build, so
            // refusing is strictly an error-message improvement.
            if arm_results.iter().any(|(v, _)| v.get_type() != val_ty) {
                let msg = "codegen error [E0910]: native codegen cannot lower a `match` whose \
                           arms produce different types. A common cause is matching on \
                           `dict_get`/`dict_remove` over a dict holding f64 or str values: \
                           native's dict is int-valued (v1), so the `Some` arm is an i64 while \
                           the default arm is not. The interpreter supports it; run under \
                           `axon run`."
                    .to_string();
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    eprintln!("{msg}");
                    self.codegen_errors.push(msg);
                }
                return None;
            }
            let phi = build_wrappers::w_phi(&self.ir.builder, val_ty, "match_val");
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
                    (BasicValueEnum::IntValue(s), BasicValueEnum::IntValue(l)) => self
                        .ir
                        .builder
                        .build_int_compare(IntPredicate::EQ, s, l, "patlit")
                        .unwrap()
                        .into(),
                    (BasicValueEnum::FloatValue(s), BasicValueEnum::FloatValue(l)) => self
                        .ir
                        .builder
                        .build_float_compare(FloatPredicate::OEQ, s, l, "patflit")
                        .unwrap()
                        .into(),
                    // String literal match: use strcmp.
                    (BasicValueEnum::StructValue(subj_sv), BasicValueEnum::StructValue(lit_sv)) => {
                        // Both are { i64, ptr } str structs. Extract data pointers and call strcmp.
                        let strcmp_fn =
                            self.ir.module.get_function("strcmp").unwrap_or_else(|| {
                                let i8_ptr = self
                                    .ir
                                    .context
                                    .i8_type()
                                    .ptr_type(inkwell::AddressSpace::default());
                                let strcmp_ty = self
                                    .ir
                                    .context
                                    .i32_type()
                                    .fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                                self.ir.module.add_function("strcmp", strcmp_ty, None)
                            });
                        let subj_ptr = build_wrappers::w_extract_value(
                            &self.ir.builder,
                            subj_sv,
                            1,
                            "subj_ptr",
                        )
                        .into_pointer_value();
                        let lit_ptr =
                            build_wrappers::w_extract_value(&self.ir.builder, lit_sv, 1, "lit_ptr")
                                .into_pointer_value();
                        let cmp_result = build_wrappers::w_call(
                            &self.ir.builder,
                            strcmp_fn,
                            &[subj_ptr.into(), lit_ptr.into()],
                            "strcmp_res",
                        )
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_int_value();
                        self.ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                cmp_result,
                                self.ir.context.i32_type().const_zero(),
                                "streq",
                            )
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
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "opttag")
                    {
                        return self
                            .ir
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
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "opttag")
                    {
                        let is_some = self
                            .ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_int(1, false),
                                "issome",
                            )
                            .unwrap();

                        // Also recurse on the inner value.
                        let inner_val = self
                            .ir
                            .builder
                            .build_extract_value(sv, 1, "optval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner_val);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
                                .ir
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
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "restag")
                    {
                        let is_ok = self
                            .ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_int(1, false),
                                "isok",
                            )
                            .unwrap();
                        let inner = self
                            .ir
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
                                .ir
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
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "restag")
                    {
                        let is_err = self
                            .ir
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_zero(),
                                "iserr",
                            )
                            .unwrap();
                        let inner = self
                            .ir
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
                                .ir
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
                let (enum_name, variant_name) = name.split_once("::").unwrap();

                // Find the tag for this variant.
                let tag_int = self
                    .enum_variants
                    .get(enum_name)
                    .and_then(|vs| vs.iter().find(|(vn, _, _)| vn == variant_name))
                    .map(|(_, tag, _)| *tag);

                if let Some(tag_int) = tag_int {
                    // Subject is the enum struct { i32, [N x i8] }.
                    // We need to alloca it to GEP field 0.
                    if let BasicValueEnum::StructValue(sv) = subject {
                        // Extract tag (field 0) — it's an i32.
                        if let BasicValueEnum::IntValue(tag_val) =
                            build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "enumtag")
                        {
                            let expected = tag_val.get_type().const_int(tag_int as u64, false);
                            return self
                                .ir
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
    ///
    /// `subject_sem_ty` is the SEMANTIC type of `subject` (the scrutinee at this
    /// level of the pattern), when codegen knows it. It is narrowed as the walk
    /// descends — `Ok(p)` over a `Result<T, E>` binds `p` at `T`, `Some(p)`
    /// over an `Option<T>` binds at `T`, a tuple element at its element type,
    /// a struct field at its declared field type — and recorded in
    /// `local_types` for every `Ident` leaf.
    ///
    /// Without this, a match-arm binding reached codegen with NO semantic type:
    /// `sem_type_of_expr(Ident)` reads `local_types`, so `u.value` on an
    /// `Uncertain<i64>` bound by `Ok(u) =>` took neither the ASI GEP path nor
    /// the named-struct path, `emit_field_access` returned `None`, and the whole
    /// enclosing body lowered to nothing — which the non-Unit return path then
    /// replaced with a fabricated zero. Measured before this change: a fn whose
    /// arms are 7, 3 and 5 returned 0 natively while the interpreter returned 7.
    /// The same loss hit `Temporal<T>`, a plain struct in a `Result`, and a
    /// struct in an `Option`; it is a property of the BINDING, not of any one
    /// builtin.
    pub(super) fn emit_pattern_bindings(
        &mut self,
        pattern: &ast::Pattern,
        subject: BasicValueEnum<'ctx>,
        subject_sem_ty: Option<&Type>,
    ) {
        match pattern {
            ast::Pattern::Ident(name) => {
                let subject_ty = subject.get_type();
                let alloca = build_wrappers::w_alloca(&self.ir.builder, subject_ty, name);
                build_wrappers::w_store(&self.ir.builder, alloca, subject);
                self.locals.insert(name.clone(), (alloca, subject_ty));
                match subject_sem_ty {
                    Some(t) if !matches!(t, Type::Unknown) => {
                        self.local_types.insert(name.clone(), t.clone());
                    }
                    // Nothing known: do not leave a STALE type from an outer
                    // binding of the same name in place — a wrong type here is
                    // worse than none, since it selects a layout.
                    _ => {
                        self.local_types.remove(name.as_str());
                    }
                }
            }
            ast::Pattern::Some(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    // Field 1 is the payload (NOT necessarily a struct — for
                    // Result/Option it is an `[N x i8]` array). Bind whatever
                    // value is extracted, matching the original `if let Ok(_)`
                    // (the wrapper returns the value directly, no Result).
                    let inner_val =
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "patinner");
                    let inner_ty = match subject_sem_ty {
                        Some(Type::Option(t)) => Some((**t).clone()),
                        _ => None,
                    };
                    self.emit_pattern_bindings(inner, inner_val, inner_ty.as_ref());
                }
            }
            ast::Pattern::Ok(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    // Payload (field 1) is the `[N x i8]` array, not a struct —
                    // bind it unconditionally (matches the original `if let Ok`).
                    let payload =
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "okpayload");
                    let ok_sem = match subject_sem_ty {
                        Some(Type::Result(ok, _)) => Some((**ok).clone()),
                        _ => self.current_result_types.clone().map(|(ok, _)| ok),
                    };
                    let typed = if let Some(ok_ty) = &ok_sem {
                        self.extract_result_payload(payload, ok_ty)
                    } else {
                        Some(payload)
                    };
                    if let Some(v) = typed {
                        self.emit_pattern_bindings(inner, v, ok_sem.as_ref());
                    }
                }
            }
            ast::Pattern::Err(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    let payload =
                        build_wrappers::w_extract_value(&self.ir.builder, sv, 1, "errpayload");
                    let err_sem = match subject_sem_ty {
                        Some(Type::Result(_, e)) => Some((**e).clone()),
                        _ => self.current_result_types.clone().map(|(_, e)| e),
                    };
                    let typed = if let Some(err_ty) = &err_sem {
                        self.extract_result_payload(payload, err_ty)
                    } else {
                        Some(payload)
                    };
                    if let Some(v) = typed {
                        self.emit_pattern_bindings(inner, v, err_sem.as_ref());
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

                let field_types = self
                    .enum_variants
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
                    let alloca = build_wrappers::w_alloca(
                        &self.ir.builder,
                        enum_struct_ty.into(),
                        "enumtmp",
                    );
                    build_wrappers::w_store(&self.ir.builder, alloca, sv.into());

                    // GEP to payload field (index 1).
                    let pay_ptr = self
                        .ir
                        .builder
                        .build_struct_gep(enum_struct_ty, alloca, 1, "pay")
                        .unwrap();

                    let i8_ty = self.ir.context.i8_type();
                    let i32_ty = self.ir.context.i32_type();
                    let ptr_ty = i8_ty.ptr_type(AddressSpace::default());

                    let pay_i8ptr = self
                        .ir
                        .builder
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
                                self.ir
                                    .builder
                                    .build_gep(i8_ty, pay_i8ptr, &[offset_val], "fieldptr")
                                    .unwrap()
                            };
                            let typed_ptr = self
                                .ir
                                .builder
                                .build_pointer_cast(field_ptr, ptr_ty, "tfptr")
                                .unwrap();
                            let field_val = self
                                .ir
                                .builder
                                .build_load(llvm_fty, typed_ptr, "fieldval")
                                .unwrap();
                            self.emit_pattern_bindings(pat, field_val, Some(&fty));
                        }

                        byte_offset += fsize;
                    }
                }
            }
            ast::Pattern::Struct { fields, .. } => {
                // Declared field types of the scrutinee struct, when known, so a
                // `Point { x, y }` pattern binds `x` at its own type rather than
                // losing it.
                //
                // MEASURED DEAD as of this commit, and recorded rather than
                // dressed up: the parser builds `Pattern::Struct` ONLY for an
                // `Enum::Variant { … }` pattern (parser.rs, the `Ident` arm —
                // a bare `Name { … }` in match position is a parse error), so
                // every `Pattern::Struct` that reaches codegen has a `::` in
                // its name and is handled by the arm above. A mutation that
                // drops the line below therefore SURVIVES. Kept because it is
                // the correct narrowing the moment plain struct patterns parse;
                // noted so no later reader reads it as exercised.
                let field_sem: Option<Vec<Type>> = match subject_sem_ty {
                    Some(Type::Struct(sn)) => self.struct_field_sem_types.get(sn).cloned(),
                    _ => None,
                };
                if let BasicValueEnum::StructValue(sv) = subject {
                    for (i, (_fname, pat)) in fields.iter().enumerate() {
                        // A struct field can be any value type (int/float/ptr/
                        // struct), not only int — bind whatever is extracted,
                        // matching the original `if let Ok(field_val)`.
                        let field_val = build_wrappers::w_extract_value(
                            &self.ir.builder,
                            sv,
                            i as u32,
                            "sfield",
                        );
                        let fty = field_sem.as_ref().and_then(|v| v.get(i));
                        self.emit_pattern_bindings(pat, field_val, fty);
                    }
                }
            }
            ast::Pattern::Tuple(pats) => {
                let elt_sem: Option<&Vec<Type>> = match subject_sem_ty {
                    Some(Type::Tuple(elts)) => Some(elts),
                    _ => None,
                };
                if let BasicValueEnum::StructValue(sv) = subject {
                    for (i, pat) in pats.iter().enumerate() {
                        let elem_val = build_wrappers::w_extract_value(
                            &self.ir.builder,
                            sv,
                            i as u32,
                            "telem",
                        );
                        let ety = elt_sem.and_then(|v| v.get(i));
                        self.emit_pattern_bindings(pat, elem_val, ety);
                    }
                }
            }
            _ => {} // Wildcard, Literal, None — no bindings
        }
    }
}
