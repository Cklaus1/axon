//! Option / Result construction + payload extraction + `?` operator.
//!
//! Phase 2.5 of the §7.5 module split.  These four methods are tightly
//! cohesive (Option is structurally simpler, Result uses the canonical
//! `{ i1 tag, [N x i8] payload }` union layout, `?` operator inspects
//! the Result tag).
//!
//! All methods declared `pub(super)` so the parent `codegen::mod` can
//! call them from within `emit_expr` and elsewhere.

use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::values::{BasicValueEnum, FunctionValue};
use inkwell::AddressSpace;

use crate::types::Type;

impl<'ctx> super::Codegen<'ctx> {
    // ── Option emission ────────────────────────────────────────────────────────

    pub(super) fn emit_option(
        &mut self,
        inner: Option<BasicValueEnum<'ctx>>,
        inner_ty: &Type,
    ) -> BasicValueEnum<'ctx> {
        let tag_ty = self.ir.context.bool_type();

        match inner {
            Some(val) => {
                let llvm_inner = val.get_type();
                let opt_ty = self.ir.context.struct_type(
                    &[tag_ty.into(), llvm_inner],
                    false,
                );
                let alloca = self.ir.builder.build_alloca(opt_ty, "some").unwrap();
                let tag_ptr = self.ir
                    .builder
                    .build_struct_gep(opt_ty, alloca, 0, "tagptr")
                    .unwrap();
                let val_ptr = self.ir
                    .builder
                    .build_struct_gep(opt_ty, alloca, 1, "valptr")
                    .unwrap();
                self.ir.builder
                    .build_store(tag_ptr, tag_ty.const_int(1, false))
                    .unwrap();
                self.ir.builder.build_store(val_ptr, val).unwrap();
                self.ir.builder.build_load(opt_ty, alloca, "someval").unwrap()
            }
            None => {
                // None: if we have a known inner type, build { false, undef }
                let llvm_inner = self.llvm_type(inner_ty);
                if let Some(inner_llvm_ty) = llvm_inner {
                    let opt_ty = self.ir.context.struct_type(
                        &[tag_ty.into(), inner_llvm_ty],
                        false,
                    );
                    let alloca = self.ir.builder.build_alloca(opt_ty, "none").unwrap();
                    let tag_ptr = self.ir
                        .builder
                        .build_struct_gep(opt_ty, alloca, 0, "tagptr")
                        .unwrap();
                    self.ir.builder
                        .build_store(tag_ptr, tag_ty.const_zero())
                        .unwrap();
                    self.ir.builder.build_load(opt_ty, alloca, "noneval").unwrap()
                } else {
                    // Unit inner: None is just `false`.
                    tag_ty.const_zero().into()
                }
            }
        }
    }

    // ── Result emission ───────────────────────────────────────────────────────

    pub(super) fn emit_result(
        &mut self,
        is_ok: bool,
        val: BasicValueEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let tag_ty = self.ir.context.bool_type();
        let tag_val = tag_ty.const_int(if is_ok { 1 } else { 0 }, false);

        if let Some((ok_ty, err_ty)) = self.current_result_types.clone() {
            // Canonical union layout: { i1, [max(sizeof T, sizeof E) x i8] }
            // Both Ok and Err arms produce the same LLVM struct type so phi nodes work.
            let ok_size = self.llvm_sizeof(&ok_ty).unwrap_or(0);
            let err_size = self.llvm_sizeof(&err_ty).unwrap_or(0);
            let payload_size = ok_size.max(err_size).max(1) as u32;
            let i8_ty = self.ir.context.i8_type();
            let payload_arr_ty = i8_ty.array_type(payload_size);
            let result_ty = self.ir.context.struct_type(
                &[tag_ty.into(), payload_arr_ty.into()], false,
            );

            let alloca = self.ir.builder.build_alloca(result_ty, "result").unwrap();
            let tag_ptr = self.ir.builder
                .build_struct_gep(result_ty, alloca, 0, "tagptr")
                .unwrap();
            let pay_ptr = self.ir.builder
                .build_struct_gep(result_ty, alloca, 1, "payptr")
                .unwrap();

            self.ir.builder.build_store(tag_ptr, tag_val).unwrap();
            // Cast the payload pointer to the value's typed pointer before storing,
            // so the store type matches the pointer's element type.
            let val_ptr_ty = val.get_type().ptr_type(inkwell::AddressSpace::default());
            let typed_pay_ptr = self.ir.builder
                .build_pointer_cast(pay_ptr, val_ptr_ty, "pay_typed")
                .unwrap();
            self.ir.builder.build_store(typed_pay_ptr, val).unwrap();

            self.ir.builder.build_load(result_ty, alloca, "resultval").unwrap()
        } else {
            // Fallback: simple { i1, T } when no type info is available.
            let payload_ty = val.get_type();
            let result_ty = self.ir.context.struct_type(
                &[tag_ty.into(), payload_ty], false,
            );
            let alloca = self.ir.builder.build_alloca(result_ty, "result").unwrap();
            let tag_ptr = self.ir.builder
                .build_struct_gep(result_ty, alloca, 0, "tagptr")
                .unwrap();
            let val_ptr = self.ir.builder
                .build_struct_gep(result_ty, alloca, 1, "valptr")
                .unwrap();
            self.ir.builder.build_store(tag_ptr, tag_val).unwrap();
            self.ir.builder.build_store(val_ptr, val).unwrap();
            self.ir.builder.build_load(result_ty, alloca, "resultval").unwrap()
        }
    }

    /// Cast the `[N x i8]` payload of a canonical Result union to a typed value.
    pub(super) fn extract_result_payload(
        &mut self,
        payload: BasicValueEnum<'ctx>,
        typed_ty: &Type,
    ) -> Option<BasicValueEnum<'ctx>> {
        let llvm_ty = self.llvm_type(typed_ty)?;
        let arr_ty = payload.get_type();
        let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let arr_alloca = self.ir.builder.build_alloca(arr_ty, "payloadalc").unwrap();
        self.ir.builder.build_store(arr_alloca, payload).unwrap();
        let typed_ptr = self.ir.builder
            .build_pointer_cast(arr_alloca, ptr_ty, "payloadptr")
            .unwrap();
        let val = self.ir.builder.build_load(llvm_ty, typed_ptr, "payloadval").unwrap();
        Some(val)
    }

    // ── ? operator ────────────────────────────────────────────────────────────

    /// Emit the `?` (early-return-on-Err) operator.
    ///
    /// The result struct is `{ i1 tag, payload }`. If tag == 0 (Err), emit an
    /// early return of the whole result value. Otherwise extract and return the
    /// Ok payload.
    pub(super) fn emit_question(
        &mut self,
        result_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let _result_ty = match result_val.get_type() {
            BasicTypeEnum::StructType(s) => s,
            _ => return result_val, // not a struct — just pass through
        };

        // Extract the tag (field 0).
        let tag = self.ir
            .builder
            .build_extract_value(result_val.into_struct_value(), 0, "qtag")
            .unwrap();

        let tag_int = match tag {
            BasicValueEnum::IntValue(i) => i,
            _ => return result_val,
        };

        // Build the two branches: ok_bb and err_bb.
        let ok_bb = self.ir.context.append_basic_block(fn_val, "q_ok");
        let err_bb = self.ir.context.append_basic_block(fn_val, "q_err");

        self.ir.builder
            .build_conditional_branch(tag_int, ok_bb, err_bb)
            .unwrap();

        // --- Err branch: early return an Err using the *outer* function's Result type.
        // This ensures the return type matches the enclosing function's signature.
        self.ir.builder.position_at_end(err_bb);
        let err_payload = self.ir
            .builder
            .build_extract_value(result_val.into_struct_value(), 1, "qerr_payload")
            .unwrap();
        // Re-wrap as Err with the outer function's Result type so the return type matches.
        let err_return_val = if let Some((_, err_ty)) = self.current_result_types.clone() {
            // Extract the typed Err value from the inner result's payload.
            let typed_err = self.extract_result_payload(err_payload, &err_ty)
                .unwrap_or(err_payload);
            // Emit an Err with the outer result type.
            self.emit_result(false, typed_err)
        } else {
            // No type info: return the inner result as-is.
            result_val
        };
        self.log_return_if_adaptive();
        self.ir.builder.build_return(Some(&err_return_val)).unwrap();

        // --- Ok branch: extract the typed Ok payload using extract_result_payload.
        self.ir.builder.position_at_end(ok_bb);
        let raw_payload = self.ir
            .builder
            .build_extract_value(result_val.into_struct_value(), 1, "qpayload")
            .unwrap();
        // Use extract_result_payload to get the properly typed Ok value.
        let payload = if let Some((ok_ty, _)) = self.current_result_types.clone() {
            self.extract_result_payload(raw_payload, &ok_ty)
                .unwrap_or(raw_payload)
        } else {
            raw_payload
        };

        payload
    }
}
