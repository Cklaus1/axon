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

use inkwell::types::BasicTypeEnum;
use inkwell::values::{BasicValueEnum, FunctionValue, IntValue, PointerValue, StructValue};
use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::ast;
use crate::types::Type;

use super::build_wrappers;
use super::enum_layout::EnumField;

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
        // Blocks whose "no arm matched" edge flows into merge_bb (the last
        // arm's test, and its guard if it has one): each needs a phi incoming.
        let mut miss_preds: Vec<inkwell::basic_block::BasicBlock<'ctx>> = Vec::new();

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
                merge_bb
            };
            let is_last = i + 1 == arms.len();

            build_wrappers::w_br(&self.ir.builder, test_bb);
            self.ir.builder.position_at_end(test_bb);

            // Emit pattern test.
            let matches = match self.emit_pattern_test(&arm.pattern, subject) {
                BasicValueEnum::IntValue(i) => i,
                _ => self.ir.context.bool_type().const_int(1, false),
            };
            if is_last {
                miss_preds.push(self.ir.builder.get_insert_block().unwrap());
            }

            // The arm's bindings are scoped to the arm (see `end_pattern_scope`).
            let scope = self.begin_pattern_scope(&arm.pattern);
            if let Some(guard_expr) = &arm.guard {
                // The guard sees the arm's bindings (`Some(v) if v > 2`), so it
                // runs in its own block once the pattern has matched. It used
                // to be emitted BEFORE the bindings: a guard naming one failed
                // to lower and was silently dropped, taking the arm whenever
                // the pattern matched.
                let guard_bb = self
                    .ir
                    .context
                    .append_basic_block(fn_val, &format!("arm{i}_guard"));
                build_wrappers::w_cond_br(&self.ir.builder, matches, guard_bb, next_bb);
                self.ir.builder.position_at_end(guard_bb);
                self.emit_pattern_bindings(&arm.pattern, subject, subject_sem_ty);
                let guard = match self.emit_expr(guard_expr, fn_val) {
                    Some(BasicValueEnum::IntValue(g)) => g,
                    _ => {
                        self.refuse_unlowered("a `match` arm guard");
                        self.ir.context.bool_type().const_int(1, false)
                    }
                };
                if is_last {
                    miss_preds.push(self.ir.builder.get_insert_block().unwrap());
                }
                build_wrappers::w_cond_br(&self.ir.builder, guard, body_bb, next_bb);
                self.ir.builder.position_at_end(body_bb);
            } else {
                build_wrappers::w_cond_br(&self.ir.builder, matches, body_bb, next_bb);
                self.ir.builder.position_at_end(body_bb);
                self.emit_pattern_bindings(&arm.pattern, subject, subject_sem_ty);
            }
            let body_val = self.emit_expr(&arm.body, fn_val);
            self.end_pattern_scope(scope);

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
                self.record_error(msg);
                return None;
            }
            let phi = build_wrappers::w_phi(&self.ir.builder, val_ty, "match_val");
            for (v, bb) in &arm_results {
                phi.add_incoming(&[(v, *bb)]);
            }
            // Every "no arm matched" edge also flows to merge_bb. LLVM
            // requires an incoming per predecessor; the value is never used.
            let zero = val_ty.const_zero();
            for bb in &miss_preds {
                phi.add_incoming(&[(&zero, *bb)]);
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

                        // Also recurse on the inner value, once the tag says
                        // there is one.
                        let inner_val = self
                            .ir
                            .builder
                            .build_extract_value(sv, 1, "optval")
                            .unwrap();
                        return self
                            .emit_test_when(is_some, inner_pat, |s| {
                                s.emit_pattern_test(inner_pat, inner_val)
                            })
                            .into();
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
                        return self
                            .emit_test_when(is_ok, inner_pat, |s| {
                                s.emit_pattern_test(inner_pat, inner)
                            })
                            .into();
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
                        return self
                            .emit_test_when(is_err, inner_pat, |s| {
                                s.emit_pattern_test(inner_pat, inner)
                            })
                            .into();
                    }
                }
                false_val.into()
            }

            // Enum variant struct pattern: "EnumName::Variant { ... }" — check
            // the tag, then each refutable field sub-pattern (`A::Lit { v: 0 }`).
            // The field tests used to be skipped, so `A::Lit { v: 0 }` matched
            // every `Lit`.
            ast::Pattern::Struct { name, fields } if name.contains("::") => {
                let (enum_name, variant_name) = name.split_once("::").unwrap();

                // Find the tag and layout for this variant.
                let variant = self
                    .enum_variants
                    .get(enum_name)
                    .and_then(|vs| vs.iter().find(|(vn, _, _)| vn == variant_name))
                    .map(|(_, tag, layout)| (*tag, layout.clone()));

                if let Some((tag_int, layout)) = variant {
                    // Subject is the enum struct { i32, [N x i8] }.
                    if let BasicValueEnum::StructValue(sv) = subject {
                        // Extract tag (field 0) — it's an i32.
                        if let BasicValueEnum::IntValue(tag_val) =
                            build_wrappers::w_extract_value(&self.ir.builder, sv, 0, "enumtag")
                        {
                            let expected = tag_val.get_type().const_int(tag_int as u64, false);
                            let tag_ok = self
                                .ir
                                .builder
                                .build_int_compare(IntPredicate::EQ, tag_val, expected, "tagcmp")
                                .unwrap();
                            let refutable: Vec<&(String, ast::Pattern)> = fields
                                .iter()
                                .filter(|(_, p)| !Self::pattern_is_irrefutable(p))
                                .collect();
                            if refutable.is_empty() {
                                return tag_ok.into();
                            }
                            // The payload is only meaningful for this variant
                            // (a boxed field of another is not a pointer), so
                            // read it only once the tag matched.
                            return self
                                .emit_test_when(tag_ok, pattern, |s| {
                                    s.emit_enum_field_tests(sv, enum_name, &layout, &refutable)
                                })
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

    /// A sub-pattern that matches every value of its type.
    fn pattern_is_irrefutable(pattern: &ast::Pattern) -> bool {
        matches!(pattern, ast::Pattern::Wildcard | ast::Pattern::Ident(_))
    }

    /// `cond && test(self)`, where the code `test` emits runs only once `cond`
    /// holds; `cond` alone when `inner` is irrefutable.
    ///
    /// A sub-pattern reads the payload the outer tag guards. Evaluating it
    /// unconditionally (`cond & test`) read a `None`'s or another variant's
    /// payload, which for a boxed enum field is not a valid pointer.
    fn emit_test_when(
        &mut self,
        cond: IntValue<'ctx>,
        inner: &ast::Pattern,
        test: impl FnOnce(&mut Self) -> BasicValueEnum<'ctx>,
    ) -> IntValue<'ctx> {
        if Self::pattern_is_irrefutable(inner) {
            return cond;
        }
        let entry_bb = self.ir.builder.get_insert_block().unwrap();
        let fn_val = entry_bb.get_parent().unwrap();
        let then_bb = self.ir.context.append_basic_block(fn_val, "pat_inner");
        let join_bb = self.ir.context.append_basic_block(fn_val, "pat_join");
        build_wrappers::w_cond_br(&self.ir.builder, cond, then_bb, join_bb);
        self.ir.builder.position_at_end(then_bb);
        let bool_ty = self.ir.context.bool_type();
        let inner_ok = match test(self) {
            BasicValueEnum::IntValue(i) => i,
            _ => bool_ty.const_int(1, false),
        };
        let then_end = self.ir.builder.get_insert_block().unwrap();
        build_wrappers::w_br(&self.ir.builder, join_bb);
        self.ir.builder.position_at_end(join_bb);
        let phi = build_wrappers::w_phi(&self.ir.builder, bool_ty.into(), "pat_ok");
        phi.add_incoming(&[(&bool_ty.const_zero(), entry_bb), (&inner_ok, then_end)]);
        phi.as_basic_value().into_int_value()
    }

    /// AND of the sub-pattern tests of an enum variant's payload `fields`
    /// (already known to be this variant's), each found in `layout` by name.
    fn emit_enum_field_tests(
        &mut self,
        sv: StructValue<'ctx>,
        enum_name: &str,
        layout: &[EnumField],
        fields: &[&(String, ast::Pattern)],
    ) -> BasicValueEnum<'ctx> {
        let bool_ty = self.ir.context.bool_type();
        let Some(pay_ptr) = self.enum_payload_ptr(sv, enum_name) else {
            return bool_ty.const_zero().into();
        };
        let mut all = bool_ty.const_int(1, false);
        for (fname, pat) in fields {
            let field_val = layout
                .iter()
                .find(|f| &f.name == fname)
                .and_then(|slot| self.load_enum_field(pay_ptr, slot));
            let Some(field_val) = field_val else {
                self.refuse_unlowered(&format!("the pattern for field `{fname}` of `{enum_name}`"));
                continue;
            };
            if let BasicValueEnum::IntValue(ok) = self.emit_pattern_test(pat, field_val) {
                all = build_wrappers::w_and(&self.ir.builder, all, ok, "fieldsok");
            }
        }
        all.into()
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
                } else {
                    self.poison_pattern_bindings(
                        inner,
                        "the `Some` payload has no native layout here",
                    );
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
                    match typed {
                        Some(v) => self.emit_pattern_bindings(inner, v, ok_sem.as_ref()),
                        None => self.poison_pattern_bindings(
                            inner,
                            "the `Ok` payload has no native layout here",
                        ),
                    }
                } else {
                    self.poison_pattern_bindings(
                        inner,
                        "the `Ok` payload has no native layout here",
                    );
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
                    match typed {
                        Some(v) => self.emit_pattern_bindings(inner, v, err_sem.as_ref()),
                        None => self.poison_pattern_bindings(
                            inner,
                            "the `Err` payload has no native layout here",
                        ),
                    }
                } else {
                    self.poison_pattern_bindings(
                        inner,
                        "the `Err` payload has no native layout here",
                    );
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

                let field_layout = self
                    .enum_variants
                    .get(&enum_name)
                    .and_then(|vs| vs.iter().find(|(vn, _, _)| vn == &variant_name))
                    .map(|(_, _, fs)| fs.clone());

                let Some(field_layout) = field_layout else {
                    let why = format!("the variant `{name}` has no native field layout");
                    self.poison_pattern_bindings(pattern, &why);
                    return;
                };

                if let BasicValueEnum::StructValue(sv) = subject {
                    let Some(pay_ptr) = self.enum_payload_ptr(sv, &enum_name) else {
                        let why = format!("the enum `{enum_name}` has no native layout");
                        self.poison_pattern_bindings(pattern, &why);
                        return;
                    };
                    // Each bound field is found by NAME. Indexing the declared
                    // layout by the pattern's position bound `P::Pt { s, x }`
                    // with `s` at `x`'s offset and type, and a pattern naming a
                    // subset (`P::Pt { s }`) at the first field's.
                    for (fname, pat) in fields {
                        let Some(slot) = field_layout.iter().find(|f| &f.name == fname) else {
                            let why = format!("the variant `{name}` has no field `{fname}`");
                            self.poison_pattern_bindings(pat, &why);
                            continue;
                        };
                        if let Some(field_val) = self.load_enum_field(pay_ptr, slot) {
                            self.emit_pattern_bindings(pat, field_val, Some(&slot.ty));
                        } else {
                            let why = format!(
                                "field `{fname}` of `{name}` has type {}, which native codegen \
                                 cannot lower",
                                slot.ty.display()
                            );
                            self.poison_pattern_bindings(pat, &why);
                        }
                    }
                } else {
                    let why = format!("the matched `{name}` value has no native layout here");
                    self.poison_pattern_bindings(pattern, &why);
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
                } else {
                    self.poison_pattern_bindings(
                        pattern,
                        "the matched value has no native layout here",
                    );
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
                } else {
                    self.poison_pattern_bindings(
                        pattern,
                        "the matched tuple has no native layout here",
                    );
                }
            }
            _ => {} // Wildcard, Literal, None — no bindings
        }
    }

    /// The bindings of `pattern` could not be created; `why` says what was
    /// missing. Each bound name is poisoned with a deferred E0910 naming that
    /// cause (AX-45): a read reports it, where it used to report the name as
    /// unknown (E0701) — or, when it shadowed an outer local, silently read the
    /// OUTER value, which is why that local is dropped here (and put back by
    /// `end_pattern_scope`). A binding nobody reads stays harmless, as before.
    fn poison_pattern_bindings(&mut self, pattern: &ast::Pattern, why: &str) {
        match pattern {
            ast::Pattern::Ident(name) => {
                self.locals.remove(name);
                self.local_types.remove(name);
                let msg = format!(
                    "codegen error [E0910]: native codegen could not bind `{name}`: {why}. The \
                     interpreter supports it; run under `axon run`."
                );
                self.poison_binding(name, Some(msg));
            }
            ast::Pattern::Some(p) | ast::Pattern::Ok(p) | ast::Pattern::Err(p) => {
                self.poison_pattern_bindings(p, why);
            }
            ast::Pattern::Struct { fields, .. } => {
                for (_, p) in fields {
                    self.poison_pattern_bindings(p, why);
                }
            }
            ast::Pattern::Tuple(pats) => {
                for p in pats {
                    self.poison_pattern_bindings(p, why);
                }
            }
            ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::None => {}
        }
    }

    /// What each name `pattern` binds meant before the bindings were emitted,
    /// so `end_pattern_scope` can put it back once the arm or loop body that
    /// could see the bindings is done.
    pub(super) fn begin_pattern_scope(&self, pattern: &ast::Pattern) -> PatternScope<'ctx> {
        let mut names = Vec::new();
        collect_pattern_names(pattern, &mut names);
        names
            .into_iter()
            .map(|name| PriorBinding {
                local: self.locals.get(&name).copied(),
                ty: self.local_types.get(&name).cloned(),
                poison: self.poisoned.get(&name).cloned(),
                name,
            })
            .collect()
    }

    /// A pattern's bindings are visible only inside the arm or loop body it
    /// scopes, as in the interpreter: past it, each name means what it meant
    /// before — an outer local again, or nothing (AX-42). Left in `locals`, a
    /// LATER arm naming the same identifier (`A::Neg { v } => v * 10 _ => v`,
    /// with `v` a parameter) read the earlier arm's never-written slot; and an
    /// unread failed binding that shadowed an outer local left the outer local
    /// refused after the `match` (AX-45).
    pub(super) fn end_pattern_scope(&mut self, scope: PatternScope<'ctx>) {
        for prior in scope {
            match prior.local {
                Some(l) => self.locals.insert(prior.name.clone(), l),
                None => self.locals.remove(&prior.name),
            };
            match prior.ty {
                Some(t) => self.local_types.insert(prior.name.clone(), t),
                None => self.local_types.remove(&prior.name),
            };
            match prior.poison {
                Some(p) => self.poisoned.insert(prior.name, p),
                None => self.poisoned.remove(&prior.name),
            };
        }
    }
}

/// One name a pattern binds, with what it meant before the pattern.
pub(super) struct PriorBinding<'ctx> {
    name: String,
    local: Option<(PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    ty: Option<Type>,
    poison: Option<Option<String>>,
}

pub(super) type PatternScope<'ctx> = Vec<PriorBinding<'ctx>>;

fn collect_pattern_names(pattern: &ast::Pattern, out: &mut Vec<String>) {
    match pattern {
        ast::Pattern::Ident(name) => out.push(name.clone()),
        ast::Pattern::Some(p) | ast::Pattern::Ok(p) | ast::Pattern::Err(p) => {
            collect_pattern_names(p, out)
        }
        ast::Pattern::Struct { fields, .. } => {
            for (_, p) in fields {
                collect_pattern_names(p, out);
            }
        }
        ast::Pattern::Tuple(pats) => {
            for p in pats {
                collect_pattern_names(p, out);
            }
        }
        ast::Pattern::Wildcard | ast::Pattern::Literal(_) | ast::Pattern::None => {}
    }
}
