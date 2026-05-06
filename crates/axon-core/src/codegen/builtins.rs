//! Pass-1 LLVM declarations for every Axon builtin (`declare_builtins`).
//!
//! Phase 2.8 of the §7.5 module split: this is the single largest method in
//! the original codegen — ~3,870 lines of `module.add_function(...)` calls
//! that wire every Axon stdlib name to either an extern-C runtime symbol or
//! a small inline LLVM-IR thunk.
//!
//! No decomposition done here — this is a *pure file move*.  Decomposing
//! `declare_builtins` into per-builtin helpers is a separate, larger
//! refactor (estimated 1-2 weeks) that should happen after the rest of the
//! module split lands and the codegen-feature build is fast enough to
//! validate every iteration.
//!
//! The method is `pub` so external callers (`emit_program`) can still call
//! it via `Codegen::declare_builtins`.

#[allow(unused_imports)]
use std::collections::HashMap;

use inkwell::types::{BasicTypeEnum, BasicMetadataTypeEnum};
use inkwell::values::{BasicMetadataValueEnum, FunctionValue};
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::ast;
use crate::types::Type;

impl<'ctx> super::Codegen<'ctx> {
    // ── Pass 1: forward-declare functions ────────────────────────────────────

    /// Declare all Axon built-in functions as either extern C declarations or
    /// thin wrappers, so calls resolve during emit.
    pub fn declare_builtins(&mut self) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.ir.context.i32_type();
        let i64_ty = self.ir.context.i64_type();
        let bool_ty = self.ir.context.bool_type();
        let void_ty = self.ir.context.void_type();

        // C stdlib: int puts(const char *s)  — prints string + newline
        let puts_ty = i32_ty.fn_type(&[i8_ptr.into()], false);
        let puts_fn = self.ir.module.add_function("puts", puts_ty, None);

        // C stdlib: int printf(const char *fmt, ...)
        let printf_ty = i32_ty.fn_type(&[i8_ptr.into()], /*variadic=*/true);
        let printf_fn = self.ir.module.add_function("printf", printf_ty, None);

        // C stdlib: void exit(int status)
        let exit_ty = void_ty.fn_type(&[i32_ty.into()], false);
        let exit_fn = self.ir.module.add_function("exit", exit_ty, None);

        // axon_println: takes { i64, i8* } Axon str struct, calls puts on the data ptr
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("println", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved_block = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            self.ir.builder.build_call(puts_fn, &[data_ptr.into()], "").unwrap();
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("println".to_string(), fn_val);
        }

        // axon_print: like println but uses printf without newline
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("print", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved_block = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            // printf("%s", data_ptr)
            let fmt = self.ir.context.const_string(b"%s", true);
            let fmt_global = self.ir.module.add_global(fmt.get_type(), None, "print_fmt");
            fmt_global.set_initializer(&fmt);
            fmt_global.set_constant(true);
            let fmt_ptr = fmt_global.as_pointer_value();
            self.ir.builder.build_call(printf_fn, &[fmt_ptr.into(), data_ptr.into()], "").unwrap();
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("print".to_string(), fn_val);
        }

        // axon_assert: takes bool, panics (calls exit(1)) if false
        {
            let fn_ty = void_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.ir.module.add_function("assert", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.ir.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "ok");
            let saved_block = self.ir.builder.get_insert_block();

            self.ir.builder.position_at_end(entry_bb);
            let cond = fn_val.get_nth_param(0).unwrap().into_int_value();
            self.ir.builder.build_conditional_branch(cond, ok_bb, fail_bb).unwrap();

            self.ir.builder.position_at_end(fail_bb);
            let msg = b"assertion failed\n\0";
            let msg_const = self.ir.context.const_string(msg, false);
            let msg_global = self.ir.module.add_global(msg_const.get_type(), None, "assert_msg");
            msg_global.set_initializer(&msg_const);
            msg_global.set_constant(true);
            let msg_ptr = msg_global.as_pointer_value();
            self.ir.builder.build_call(printf_fn, &[msg_ptr.into()], "").unwrap();
            let one = i32_ty.const_int(1, false);
            self.ir.builder.build_call(exit_fn, &[one.into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();

            self.ir.builder.position_at_end(ok_bb);
            self.ir.builder.build_return(None).unwrap();

            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert".to_string(), fn_val);
        }

        // Declare write(fd: i32, buf: ptr, count: i64) -> i64 for stderr output.
        let write_ty = i64_ty.fn_type(&[i32_ty.into(), i8_ptr.into(), i64_ty.into()], false);
        let write_fn = self.ir.module.add_function("write", write_ty, None);

        // eprintln: writes string + newline to stderr (fd=2) using write(2, ...).
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("eprintln", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved_block = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            let length = self.ir.builder
                .build_extract_value(str_arg, 0, "ep_len")
                .unwrap()
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            // Write the string content.
            self.ir.builder.build_call(write_fn, &[fd2.into(), data_ptr.into(), length.into()], "").unwrap();
            // Write the newline.
            let nl_arr = self.ir.context.i8_type().array_type(1);
            let nl_g = self.ir.module.add_global(nl_arr, None, "eprintln_nl");
            nl_g.set_initializer(&self.ir.context.i8_type().const_array(&[self.ir.context.i8_type().const_int(b'\n' as u64, false)]));
            nl_g.set_constant(true);
            let nl_ptr = self.ir.builder.build_pointer_cast(nl_g.as_pointer_value(), i8_ptr, "nlptr").unwrap();
            let one64 = i64_ty.const_int(1, false);
            self.ir.builder.build_call(write_fn, &[fd2.into(), nl_ptr.into(), one64.into()], "").unwrap();
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("eprintln".to_string(), fn_val);
        }

        // eprint: writes string to stderr (fd=2) using write(2, ...) without newline.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("eprint", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved_block = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            let length = self.ir.builder
                .build_extract_value(str_arg, 0, "ep_len")
                .unwrap()
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            self.ir.builder.build_call(write_fn, &[fd2.into(), data_ptr.into(), length.into()], "").unwrap();
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("eprint".to_string(), fn_val);
        }

        // C stdlib: int snprintf(char *buf, size_t n, const char *fmt, ...)
        let snprintf_ty = i32_ty.fn_type(&[i8_ptr.into(), i64_ty.into(), i8_ptr.into()], true);
        let snprintf_fn = self.ir.module.add_function("snprintf", snprintf_ty, None);

        // to_str: i64 → { i64, ptr }
        // Uses malloc-allocated buffer so the returned str is heap-owned and
        // remains valid when returned from a function (no dangling static buffer).
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("to_str", fn_ty, None);

            // Get (or re-use) malloc declaration.
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", malloc_ty, None)
            });

            // Format string "%lld\0".
            let fmt_bytes = self.ir.context.const_string(b"%lld", true);
            let fmt_global2 = self.ir.module.add_global(fmt_bytes.get_type(), None, "to_str_fmt");
            fmt_global2.set_initializer(&fmt_bytes);
            fmt_global2.set_constant(true);

            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);

            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let fmt_ptr2 = self.ir.builder
                .build_pointer_cast(fmt_global2.as_pointer_value(), i8_ptr, "fmtptr")
                .unwrap();

            // Pass 1: snprintf(NULL, 0, "%lld", n) → required length (not counting '\0').
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = self.ir.builder
                .build_call(
                    snprintf_fn,
                    &[null_ptr.into(), zero64.into(), fmt_ptr2.into(), n.into()],
                    "snplen",
                )
                .unwrap();
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = self.ir.builder.build_int_z_extend(len_i32, i64_ty, "len64").unwrap();

            // Allocate len + 1 bytes (room for null terminator).
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = self.ir.builder.build_int_add(len_i64, one64, "allocsz").unwrap();
            let buf_call = self.ir.builder
                .build_call(malloc_fn, &[alloc_size.into()], "buf")
                .unwrap();
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%lld", n) → writes the decimal string.
            self.ir.builder
                .build_call(
                    snprintf_fn,
                    &[buf_ptr.into(), alloc_size.into(), fmt_ptr2.into(), n.into()],
                    "snpwrite",
                )
                .unwrap();

            // Build { i64, ptr } return struct.
            let out_alloca = self.ir.builder.build_alloca(str_ty, "out").unwrap();
            let len_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.ir.builder.build_store(len_ptr, len_i64).unwrap();
            self.ir.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.ir.builder.build_load(str_ty, out_alloca, "outval").unwrap();
            self.ir.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("to_str".to_string(), fn_val);
        }

        // to_str_f64: f64 → { i64, ptr } via snprintf("%.6g")
        // Uses malloc-allocated buffer so the returned str is heap-owned.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = str_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("to_str_f64", fn_ty, None);

            // Get (or re-use) malloc declaration.
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", malloc_ty, None)
            });

            let fmt_bytes = self.ir.context.const_string(b"%.6g", true);
            let fmt_global = self.ir.module.add_global(fmt_bytes.get_type(), None, "to_str_f64_fmt");
            fmt_global.set_initializer(&fmt_bytes);
            fmt_global.set_constant(true);

            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);

            let n = fn_val.get_nth_param(0).unwrap().into_float_value();
            let fmt_ptr = self.ir.builder
                .build_pointer_cast(fmt_global.as_pointer_value(), i8_ptr, "fmtptr")
                .unwrap();

            // Pass 1: snprintf(NULL, 0, "%.6g", n) → required length.
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = self.ir.builder
                .build_call(
                    snprintf_fn,
                    &[null_ptr.into(), zero64.into(), fmt_ptr.into(), n.into()],
                    "snplen",
                )
                .unwrap();
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = self.ir.builder.build_int_z_extend(len_i32, i64_ty, "len64").unwrap();

            // Allocate len + 1 bytes.
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = self.ir.builder.build_int_add(len_i64, one64, "allocsz").unwrap();
            let buf_call = self.ir.builder
                .build_call(malloc_fn, &[alloc_size.into()], "buf")
                .unwrap();
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%.6g", n).
            self.ir.builder
                .build_call(
                    snprintf_fn,
                    &[buf_ptr.into(), alloc_size.into(), fmt_ptr.into(), n.into()],
                    "snpwrite",
                )
                .unwrap();

            let out_alloca = self.ir.builder.build_alloca(str_ty, "out").unwrap();
            let len_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.ir.builder.build_store(len_ptr, len_i64).unwrap();
            self.ir.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.ir.builder.build_load(str_ty, out_alloca, "outval").unwrap();
            self.ir.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("to_str_f64".to_string(), fn_val);
        }

        // assert_eq(a: i64, b: i64): panic if a != b
        {
            let fn_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("assert_eq", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.ir.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "ok");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b_param = fn_val.get_nth_param(1).unwrap().into_int_value();
            let eq = self.ir.builder
                .build_int_compare(IntPredicate::EQ, a, b_param, "eq")
                .unwrap();
            self.ir.builder.build_conditional_branch(eq, ok_bb, fail_bb).unwrap();
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: values not equal\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_eq_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.ir.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.ir.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(ok_bb);
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert_eq".to_string(), fn_val);
        }

        // assert_err(tag: i1): panic if tag == 1 (Ok) — expected Err
        {
            let fn_ty = void_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.ir.module.add_function("assert_err", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.ir.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "ok");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let tag = fn_val.get_nth_param(0).unwrap().into_int_value();
            let is_ok_val = self.ir.builder
                .build_int_compare(IntPredicate::EQ, tag, bool_ty.const_int(1, false), "isok")
                .unwrap();
            self.ir.builder.build_conditional_branch(is_ok_val, fail_bb, ok_bb).unwrap();
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: expected Err, got Ok\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_err_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.ir.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.ir.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(ok_bb);
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert_err".to_string(), fn_val);
        }

        // len(s: str) -> i64: extracts the length field (field 0) from the str struct
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("len", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let length = self.ir.builder
                .build_extract_value(s, 0, "len")
                .unwrap()
                .into_int_value();
            self.ir.builder.build_return(Some(&length)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("len".to_string(), fn_val);
        }

        // parse_int(s: str) -> Result<i64, str>
        //
        // Layout: Result<i64, str> = { i1 tag, [8 x i8] payload }
        //   Ok(n)    → tag=1, payload contains i64 n as 8 bytes
        //   Err(msg) → tag=0, payload contains i64(0)
        //
        // Implemented in pure LLVM IR (no external C dependency) using strtoll.
        // strtoll is available from libc, which the JIT resolves from the host process.
        //
        // C stdlib: long long strtoll(const char *nptr, char **endptr, int base)
        {
            // strtoll declaration (variadic=false; endptr is i8**)
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let strtoll_ty = i64_ty.fn_type(
                &[i8_ptr.into(), i8_ptr_ptr.into(), i32_ty.into()],
                false,
            );
            let strtoll_fn = self.ir.module.add_function("strtoll", strtoll_ty, None);

            // Result<i64, str> LLVM type: { i1, [16 x i8] }
            // The Err case holds a str struct { i64, ptr } which is 16 bytes.
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(
                &[bool_ty.into(), i8_arr16_ty.into()],
                false,
            );

            // parse_int takes a str struct { i64 len, ptr data }.
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_int", fn_ty, None);

            // Basic blocks: entry, ok_bb, err_bb
            let entry_bb = self.ir.context.append_basic_block(fn_val, "pi_entry");
            let ok_bb   = self.ir.context.append_basic_block(fn_val, "pi_ok");
            let err_bb  = self.ir.context.append_basic_block(fn_val, "pi_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            // Unpack the str struct.
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder
                .build_extract_value(s, 1, "pi_data")
                .unwrap()
                .into_pointer_value();

            // Allocate an endptr on the stack so strtoll can write to it.
            let endptr_slot = self.ir.builder.build_alloca(i8_ptr, "pi_endptr").unwrap();
            // Null-initialise so strtoll doesn't read garbage.
            let null_ptr = i8_ptr.const_null();
            self.ir.builder.build_store(endptr_slot, null_ptr).unwrap();

            // Cast endptr slot to i8** (same type on all targets).
            let endptr_slot_cast = self.ir.builder
                .build_pointer_cast(endptr_slot, i8_ptr_ptr, "pi_endptr_cast")
                .unwrap();

            // Call strtoll(data, &endptr, 10).
            let base10 = i32_ty.const_int(10, false);
            let strtoll_ret = self.ir.builder
                .build_call(
                    strtoll_fn,
                    &[data_ptr.into(), endptr_slot_cast.into(), base10.into()],
                    "pi_strtoll",
                )
                .unwrap();
            let parsed_i64 = strtoll_ret
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            // Read back endptr to detect parse errors.
            // If endptr == data_ptr, no digits were consumed → Err.
            let endptr_val = self.ir.builder
                .build_load(i8_ptr, endptr_slot, "pi_endptr_val")
                .unwrap()
                .into_pointer_value();
            let endptr_int = self.ir.builder
                .build_ptr_to_int(endptr_val, i64_ty, "pi_endptr_int")
                .unwrap();
            let data_int = self.ir.builder
                .build_ptr_to_int(data_ptr, i64_ty, "pi_data_int")
                .unwrap();
            let consumed = self.ir.builder
                .build_int_compare(
                    IntPredicate::NE,
                    endptr_int,
                    data_int,
                    "pi_consumed",
                )
                .unwrap();
            self.ir.builder
                .build_conditional_branch(consumed, ok_bb, err_bb)
                .unwrap();

            // ok_bb: return { tag=1, payload=parsed_i64 as [8 x i8] }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = self.ir.builder.build_alloca(result_ty, "pi_ok_slot").unwrap();
            // Store tag = 1 (i1 true)
            let tag1 = bool_ty.const_int(1, false);
            let tag_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "pi_tagptr_ok").unwrap();
            self.ir.builder.build_store(tag_ptr_ok, tag1).unwrap();
            // Store the i64 value into the [8 x i8] payload via a pointer cast.
            let payload_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "pi_payptr_ok").unwrap();
            let payload_i64_ptr = self.ir.builder
                .build_pointer_cast(payload_ptr_ok, i64_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_i64")
                .unwrap();
            self.ir.builder.build_store(payload_i64_ptr, parsed_i64).unwrap();
            let ok_val = self.ir.builder.build_load(result_ty, ok_alloca, "pi_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: return { tag=0, payload = str { len=0, ptr=null_byte } }
            self.ir.builder.position_at_end(err_bb);
            let err_alloca = self.ir.builder.build_alloca(result_ty, "pi_err_slot").unwrap();
            let tag0 = bool_ty.const_int(0, false);
            let tag_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "pi_tagptr_err").unwrap();
            self.ir.builder.build_store(tag_ptr_err, tag0).unwrap();
            // Store empty str struct { i64=0, ptr=null_byte } into the payload.
            let null_byte_arr = self.ir.context.i8_type().array_type(1);
            let null_byte_g = self.ir.module.add_global(null_byte_arr, None, "pi_null_byte");
            null_byte_g.set_initializer(&self.ir.context.i8_type().const_array(&[self.ir.context.i8_type().const_int(0, false)]));
            null_byte_g.set_constant(true);
            let null_byte_ptr = self.ir.builder
                .build_pointer_cast(null_byte_g.as_pointer_value(), i8_ptr, "pi_null_ptr")
                .unwrap();
            let err_str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let payload_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "pi_payptr_err").unwrap();
            let payload_str_ptr = self.ir.builder
                .build_pointer_cast(payload_ptr_err, err_str_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_str_err")
                .unwrap();
            let err_str_alloca = self.ir.builder.build_alloca(err_str_ty, "pi_err_str").unwrap();
            let err_str_len_ptr = self.ir.builder.build_struct_gep(err_str_ty, err_str_alloca, 0, "pi_esl").unwrap();
            let err_str_dat_ptr = self.ir.builder.build_struct_gep(err_str_ty, err_str_alloca, 1, "pi_esd").unwrap();
            self.ir.builder.build_store(err_str_len_ptr, i64_ty.const_int(0, false)).unwrap();
            self.ir.builder.build_store(err_str_dat_ptr, null_byte_ptr).unwrap();
            let err_str_val = self.ir.builder.build_load(err_str_ty, err_str_alloca, "pi_err_str_val").unwrap();
            self.ir.builder.build_store(payload_str_ptr, err_str_val).unwrap();
            let err_val = self.ir.builder.build_load(result_ty, err_alloca, "pi_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_int".to_string(), fn_val);
        }

        // axon_concat(a: str, b: str) -> str
        // Used by string interpolation lowering.
        // Allocates a new buffer via malloc, copies both strings, null-terminates.
        {
            // C stdlib: void *malloc(size_t n)
            let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
            // Use get_function to avoid duplicate declarations (malloc may have been
            // declared already by to_str or to_str_f64 above).
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                self.ir.module.add_function("malloc", malloc_ty, None)
            });

            // C stdlib: void *memcpy(void *dst, const void *src, size_t n)
            let memcpy_ty = i8_ptr.fn_type(
                &[i8_ptr.into(), i8_ptr.into(), i64_ty.into()],
                false,
            );
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                self.ir.module.add_function("memcpy", memcpy_ty, None)
            });

            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("axon_concat", fn_ty, None);

            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);

            let a_val = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b_val = fn_val.get_nth_param(1).unwrap().into_struct_value();

            // Extract lengths and data pointers.
            let a_len = self.ir.builder.build_extract_value(a_val, 0, "a_len").unwrap().into_int_value();
            let a_ptr = self.ir.builder.build_extract_value(a_val, 1, "a_ptr").unwrap().into_pointer_value();
            let b_len = self.ir.builder.build_extract_value(b_val, 0, "b_len").unwrap().into_int_value();
            let b_ptr = self.ir.builder.build_extract_value(b_val, 1, "b_ptr").unwrap().into_pointer_value();

            // total_len = a_len + b_len
            let total_len = self.ir.builder.build_int_add(a_len, b_len, "total_len").unwrap();
            // alloc_len = total_len + 1  (null terminator)
            let one64 = i64_ty.const_int(1, false);
            let alloc_len = self.ir.builder.build_int_add(total_len, one64, "alloc_len").unwrap();

            // buf = malloc(alloc_len)
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_len.into()], "buf").unwrap();
            let buf_ptr = buf.try_as_basic_value().left().unwrap().into_pointer_value();

            // memcpy(buf, a_ptr, a_len)
            self.ir.builder.build_call(
                memcpy_fn,
                &[buf_ptr.into(), a_ptr.into(), a_len.into()],
                "",
            ).unwrap();

            // buf_b = buf + a_len  (GEP to offset into buf)
            let buf_b_ptr = unsafe {
                self.ir.builder.build_gep(
                    self.ir.context.i8_type(),
                    buf_ptr,
                    &[a_len],
                    "buf_b",
                ).unwrap()
            };

            // memcpy(buf_b, b_ptr, b_len)
            self.ir.builder.build_call(
                memcpy_fn,
                &[buf_b_ptr.into(), b_ptr.into(), b_len.into()],
                "",
            ).unwrap();

            // null-terminate: *(buf + total_len) = 0
            let null_pos = unsafe {
                self.ir.builder.build_gep(
                    self.ir.context.i8_type(),
                    buf_ptr,
                    &[total_len],
                    "null_pos",
                ).unwrap()
            };
            self.ir.builder.build_store(null_pos, self.ir.context.i8_type().const_int(0, false)).unwrap();

            // Return { total_len, buf_ptr }
            let out_alloca = self.ir.builder.build_alloca(str_ty, "concat_out").unwrap();
            let len_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.ir.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.ir.builder.build_store(len_ptr, total_len).unwrap();
            self.ir.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.ir.builder.build_load(str_ty, out_alloca, "concat_val").unwrap();
            self.ir.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("axon_concat".to_string(), fn_val);
        }

        // abs_i32(n: i32) -> i32
        {
            let i32_ty = self.ir.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into()], false);
            let fn_val = self.ir.module.add_function("abs_i32", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i32_ty.const_zero();
            let is_neg = self.ir.builder.build_int_compare(IntPredicate::SLT, n, zero, "isneg").unwrap();
            let neg_n = self.ir.builder.build_int_neg(n, "negn").unwrap();
            let abs_val = self.ir.builder.build_select(is_neg, neg_n, n, "absval").unwrap();
            self.ir.builder.build_return(Some(&abs_val)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("abs_i32".to_string(), fn_val);
            self.fn_return_types.insert("abs_i32".to_string(), Type::I32);
        }

        // abs_f64(n: f64) -> f64
        {
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("abs_f64", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let n = fn_val.get_nth_param(0).unwrap().into_float_value();
            let zero = f64_ty.const_zero();
            let is_neg = self.ir.builder.build_float_compare(FloatPredicate::OLT, n, zero, "isneg").unwrap();
            let neg_n = self.ir.builder.build_float_neg(n, "negn").unwrap();
            let abs_val = self.ir.builder.build_select(is_neg, neg_n, n, "absval").unwrap();
            self.ir.builder.build_return(Some(&abs_val)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("abs_f64".to_string(), fn_val);
            self.fn_return_types.insert("abs_f64".to_string(), Type::F64);
        }

        // min_i32(a: i32, b: i32) -> i32
        {
            let i32_ty = self.ir.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into(), i32_ty.into()], false);
            let fn_val = self.ir.module.add_function("min_i32", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_lt_b = self.ir.builder.build_int_compare(IntPredicate::SLT, a, b, "altb").unwrap();
            let min_val = self.ir.builder.build_select(a_lt_b, a, b, "minval").unwrap();
            self.ir.builder.build_return(Some(&min_val)).unwrap();
            if let Some(b2) = saved { self.ir.builder.position_at_end(b2); }
            self.functions.insert("min_i32".to_string(), fn_val);
            self.fn_return_types.insert("min_i32".to_string(), Type::I32);
        }

        // max_i32(a: i32, b: i32) -> i32
        {
            let i32_ty = self.ir.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into(), i32_ty.into()], false);
            let fn_val = self.ir.module.add_function("max_i32", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_gt_b = self.ir.builder.build_int_compare(IntPredicate::SGT, a, b, "agtb").unwrap();
            let max_val = self.ir.builder.build_select(a_gt_b, a, b, "maxval").unwrap();
            self.ir.builder.build_return(Some(&max_val)).unwrap();
            if let Some(b2) = saved { self.ir.builder.position_at_end(b2); }
            self.functions.insert("max_i32".to_string(), fn_val);
            self.fn_return_types.insert("max_i32".to_string(), Type::I32);
        }

        // malloc: void* malloc(i64 size)
        let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
            let ty = i8_ptr.fn_type(&[i64_ty.into()], false);
            self.ir.module.add_function("malloc", ty, None)
        });
        self.functions.insert("malloc".to_string(), malloc_fn);

        // __axon_spawn: void __axon_spawn(fn_ptr: i8*, env_ptr: i8*)
        let spawn_ty = void_ty.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        let spawn_fn = self.ir.module.add_function("__axon_spawn", spawn_ty, None);
        self.functions.insert("__axon_spawn".to_string(), spawn_fn);

        // __axon_chan_new: i8* __axon_chan_new(capacity: i64)
        let chan_new_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
        let chan_new_fn = self.ir.module.add_function("__axon_chan_new", chan_new_ty, None);
        self.functions.insert("__axon_chan_new".to_string(), chan_new_fn);

        // __axon_chan_send: void __axon_chan_send(chan: i8*, val: i64)
        let chan_send_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
        let chan_send_fn = self.ir.module.add_function("__axon_chan_send", chan_send_ty, None);
        self.functions.insert("__axon_chan_send".to_string(), chan_send_fn);

        // __axon_chan_recv: i64 __axon_chan_recv(chan: i8*)
        let chan_recv_ty = i64_ty.fn_type(&[i8_ptr.into()], false);
        let chan_recv_fn = self.ir.module.add_function("__axon_chan_recv", chan_recv_ty, None);
        self.functions.insert("__axon_chan_recv".to_string(), chan_recv_fn);

        // __axon_select: i64 __axon_select(chans: i8**, n: i64)
        // Returns the index of the first ready channel arm.
        let select_ty = i64_ty.fn_type(&[i8_ptr.ptr_type(AddressSpace::default()).into(), i64_ty.into()], false);
        let select_fn = self.ir.module.add_function("__axon_select", select_ty, None);
        self.functions.insert("__axon_select".to_string(), select_fn);

        // __axon_chan_clone: i8* __axon_chan_clone(chan: i8*)
        let chan_clone_ty = i8_ptr.fn_type(&[i8_ptr.into()], false);
        let chan_clone_fn = self.ir.module.add_function("__axon_chan_clone", chan_clone_ty, None);
        self.functions.insert("__axon_chan_clone".to_string(), chan_clone_fn);

        // Chan::new — alias for __axon_chan_new (called as Chan::new(capacity))
        self.functions.insert("Chan::new".to_string(), chan_new_fn);
        // chan.send / chan.recv / chan.clone — registered under bare method names for MethodCall dispatch.
        self.functions.insert("send".to_string(), chan_send_fn);
        self.functions.insert("recv".to_string(), chan_recv_fn);
        self.functions.insert("clone".to_string(), chan_clone_fn);
        self.fn_return_types.insert("Chan::new".to_string(), Type::Chan(Box::new(Type::Unknown)));
        self.fn_return_types.insert("recv".to_string(), Type::I64);
        self.fn_return_types.insert("send".to_string(), Type::Unit);
        self.fn_return_types.insert("clone".to_string(), Type::Chan(Box::new(Type::Unknown)));

        // Populate fn_return_types for all other builtins (Fix 19).
        self.fn_return_types.insert("println".to_string(), Type::Unit);
        self.fn_return_types.insert("print".to_string(), Type::Unit);
        self.fn_return_types.insert("eprintln".to_string(), Type::Unit);
        self.fn_return_types.insert("eprint".to_string(), Type::Unit);
        self.fn_return_types.insert("assert".to_string(), Type::Unit);
        self.fn_return_types.insert("assert_eq".to_string(), Type::Unit);
        self.fn_return_types.insert("assert_err".to_string(), Type::Unit);
        self.fn_return_types.insert("len".to_string(), Type::I64);
        self.fn_return_types.insert("to_str".to_string(), Type::Str);
        self.fn_return_types.insert("to_str_f64".to_string(), Type::Str);
        self.fn_return_types.insert("axon_concat".to_string(), Type::Str);

        // ── format(template: str) -> str — identity wrapper ───────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("format", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let s = fn_val.get_nth_param(0).unwrap();
            self.ir.builder.build_return(Some(&s)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("format".to_string(), fn_val);
            self.fn_return_types.insert("format".to_string(), Type::Str);
        }

        self.fn_return_types.insert("parse_int".to_string(),
            Type::Result(Box::new(Type::I64), Box::new(Type::Str)));
        self.fn_return_types.insert("read_file".to_string(),
            Type::Result(Box::new(Type::Str), Box::new(Type::Str)));
        self.fn_return_types.insert("write_file".to_string(),
            Type::Result(Box::new(Type::Unit), Box::new(Type::Str)));
        self.fn_return_types.insert("ai_complete".to_string(),
            Type::Result(Box::new(Type::Str), Box::new(Type::Str)));

        // ── Phase 3 math builtins (backed by C libm via LLVM intrinsics) ───
        {
            let f64_ty = self.ir.context.f64_type();
            let f1 = f64_ty.fn_type(&[f64_ty.into()], false);
            let f2 = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);

            let sqrt_fn = self.ir.module.add_function("llvm.sqrt.f64", f1, None);
            self.functions.insert("sqrt".to_string(), sqrt_fn);
            self.fn_return_types.insert("sqrt".to_string(), Type::F64);

            let pow_fn = self.ir.module.add_function("llvm.pow.f64", f2, None);
            self.functions.insert("pow".to_string(), pow_fn);
            self.fn_return_types.insert("pow".to_string(), Type::F64);

            let floor_fn = self.ir.module.add_function("llvm.floor.f64", f1, None);
            self.functions.insert("floor".to_string(), floor_fn);
            self.fn_return_types.insert("floor".to_string(), Type::F64);

            let ceil_fn = self.ir.module.add_function("llvm.ceil.f64", f1, None);
            self.functions.insert("ceil".to_string(), ceil_fn);
            self.fn_return_types.insert("ceil".to_string(), Type::F64);
        }

        // assert_eq_f64 — panic if two f64 values differ.
        {
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = void_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("assert_eq_f64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.ir.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "ok");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_float_value();
            let b_param = fn_val.get_nth_param(1).unwrap().into_float_value();
            let eq = self.ir.builder.build_float_compare(FloatPredicate::OEQ, a, b_param, "eq").unwrap();
            self.ir.builder.build_conditional_branch(eq, ok_bb, fail_bb).unwrap();
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: f64 values not equal\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_eq_f64_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.ir.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.ir.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(ok_bb);
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert_eq_f64".to_string(), fn_val);
            self.fn_return_types.insert("assert_eq_f64".to_string(), Type::Unit);
        }

        // assert_eq_str — panic if two str values differ (compare len then bytes via memcmp).
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("assert_eq_str", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "entry");
            let len_fail_bb = self.ir.context.append_basic_block(fn_val, "len_fail");
            let cmp_bb = self.ir.context.append_basic_block(fn_val, "cmp");
            let bytes_fail_bb = self.ir.context.append_basic_block(fn_val, "bytes_fail");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "ok");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a_struct = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b_struct = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let a_len = self.ir.builder.build_extract_value(a_struct, 0, "a_len").unwrap().into_int_value();
            let b_len = self.ir.builder.build_extract_value(b_struct, 0, "b_len").unwrap().into_int_value();
            let len_eq = self.ir.builder.build_int_compare(IntPredicate::EQ, a_len, b_len, "len_eq").unwrap();
            self.ir.builder.build_conditional_branch(len_eq, cmp_bb, len_fail_bb).unwrap();
            // lengths differ → fail
            self.ir.builder.position_at_end(len_fail_bb);
            let fail_msg = self.ir.context.const_string(b"assert_eq_str failed: lengths differ\n\0", false);
            let fail_g = self.ir.module.add_global(fail_msg.get_type(), None, "aeqs_len_msg");
            fail_g.set_initializer(&fail_msg);
            fail_g.set_constant(true);
            self.ir.builder.build_call(printf_fn, &[fail_g.as_pointer_value().into()], "").unwrap();
            self.ir.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            // same length — compare bytes via memcmp
            self.ir.builder.position_at_end(cmp_bb);
            let a_ptr = self.ir.builder.build_extract_value(a_struct, 1, "a_ptr").unwrap().into_pointer_value();
            let b_ptr = self.ir.builder.build_extract_value(b_struct, 1, "b_ptr").unwrap().into_pointer_value();
            let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = self.ir.builder.build_call(memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "cmp").unwrap().try_as_basic_value().left().unwrap().into_int_value();
            let zero32 = i32_ty.const_zero();
            let bytes_eq = self.ir.builder.build_int_compare(IntPredicate::EQ, cmp_result, zero32, "bytes_eq").unwrap();
            self.ir.builder.build_conditional_branch(bytes_eq, ok_bb, bytes_fail_bb).unwrap();
            // bytes differ → fail
            self.ir.builder.position_at_end(bytes_fail_bb);
            let bytes_msg = self.ir.context.const_string(b"assert_eq_str failed: bytes differ\n\0", false);
            let bytes_g = self.ir.module.add_global(bytes_msg.get_type(), None, "aeqs_bytes_msg");
            bytes_g.set_initializer(&bytes_msg);
            bytes_g.set_constant(true);
            self.ir.builder.build_call(printf_fn, &[bytes_g.as_pointer_value().into()], "").unwrap();
            self.ir.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            self.ir.builder.position_at_end(ok_bb);
            self.ir.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert_eq_str".to_string(), fn_val);
            self.fn_return_types.insert("assert_eq_str".to_string(), Type::Unit);
        }

        // ── Phase 4: time builtins ─────────────────────────────────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            // sleep_ms(ms: i64) -> ()
            let sleep_ty = void_ty.fn_type(&[i64_ty.into()], false);
            let sleep_fn = self.ir.module.add_function("__axon_sleep_ms", sleep_ty, None);
            self.functions.insert("sleep_ms".to_string(), sleep_fn);
            self.fn_return_types.insert("sleep_ms".to_string(), Type::Unit);

            // now_ms() -> i64
            let now_ty = i64_ty.fn_type(&[], false);
            let now_fn = self.ir.module.add_function("__axon_now_ms", now_ty, None);
            self.functions.insert("now_ms".to_string(), now_fn);
            self.fn_return_types.insert("now_ms".to_string(), Type::I64);
        }

        // ── Layer-1 ASI: Uncertain<i64> / Temporal<i64> builtins ───────────────
        // V1 monomorphisation on i64 (PRD AI_Language_Plan.md lines 1360-1467).
        // Layouts:
        //   Uncertain<i64> = { i64 value, f64 confidence, i64 source_tag }
        //   Temporal<i64>  = { i64 value, f64 confidence, i64 horizon_ms,
        //                      f64 decay, i64 valid_until_ms }
        {
            let i64_ty = self.ir.context.i64_type();
            let f64_ty = self.ir.context.f64_type();
            let unc_ty = self.ir.context
                .struct_type(&[i64_ty.into(), f64_ty.into(), i64_ty.into()], false);
            let tmp_ty = self.ir.context.struct_type(
                &[
                    i64_ty.into(),
                    f64_ty.into(),
                    i64_ty.into(),
                    f64_ty.into(),
                    i64_ty.into(),
                ],
                false,
            );

            let saved = self.ir.builder.get_insert_block();

            // uncertain_new(value: i64, confidence: f64) -> Uncertain<i64>
            {
                let fn_ty = unc_ty.fn_type(&[i64_ty.into(), f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_new", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_int_value();
                let c = fn_val.get_nth_param(1).unwrap().into_float_value();
                let mut sv = unc_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "u_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, c, 1, "u_conf").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, i64_ty.const_zero(), 2, "u_src")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("uncertain_new".to_string(), fn_val);
                self.fn_return_types
                    .insert("uncertain_new".to_string(), Type::Uncertain(Box::new(Type::I64)));
            }

            // uncertain_deterministic(value: i64) -> Uncertain<i64> (confidence = 1.0)
            {
                let fn_ty = unc_ty.fn_type(&[i64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_deterministic", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_int_value();
                let one = f64_ty.const_float(1.0);
                let mut sv = unc_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "ud_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, one, 1, "ud_conf").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, i64_ty.const_zero(), 2, "ud_src")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("uncertain_deterministic".to_string(), fn_val);
                self.fn_return_types.insert(
                    "uncertain_deterministic".to_string(),
                    Type::Uncertain(Box::new(Type::I64)),
                );
            }

            // uncertain_new_f64(value: f64, confidence: f64) -> Uncertain<f64>
            // Layer-2 ASI: f64 variant of uncertain_new for floating-point
            // Uncertain<T> values. Layout is { f64 value, f64 confidence,
            // i64 source_tag } — same shape as Uncertain<i64> with an f64
            // value slot.
            {
                let unc_f64_ty = self.ir.context
                    .struct_type(&[f64_ty.into(), f64_ty.into(), i64_ty.into()], false);
                let fn_ty = unc_f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_new_f64", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_float_value();
                let c = fn_val.get_nth_param(1).unwrap().into_float_value();
                let mut sv = unc_f64_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "uf_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, c, 1, "uf_conf").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, i64_ty.const_zero(), 2, "uf_src")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("uncertain_new_f64".to_string(), fn_val);
                self.fn_return_types
                    .insert("uncertain_new_f64".to_string(), Type::Uncertain(Box::new(Type::F64)));
            }

            // uncertain_dyn_i64(value: i64, confidence: f64) -> Uncertain<i64>
            // Layer-3.6 ASI: identical lowering to `uncertain_new`, but stamps
            // source_tag = 2 to mark the value as Runtime-classified for the
            // static @[verify] lattice (verify::confidence_of_call).  The
            // static checker treats this source as `Confidence::Runtime` and
            // defers entirely to `__axon_verify_panic` (the runtime check
            // injected by `emit_verify_check_if_needed` at every return site).
            {
                let fn_ty = unc_ty.fn_type(&[i64_ty.into(), f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_dyn_i64", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_int_value();
                let c = fn_val.get_nth_param(1).unwrap().into_float_value();
                let mut sv = unc_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "udy_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, c, 1, "udy_conf").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, i64_ty.const_int(2, false), 2, "udy_src")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("uncertain_dyn_i64".to_string(), fn_val);
                self.fn_return_types
                    .insert("uncertain_dyn_i64".to_string(), Type::Uncertain(Box::new(Type::I64)));
            }

            // uncertain_dyn_f64(value: f64, confidence: f64) -> Uncertain<f64>
            // f64 variant of uncertain_dyn_i64.  source_tag = 2 (Runtime).
            {
                let unc_f64_ty = self.ir.context
                    .struct_type(&[f64_ty.into(), f64_ty.into(), i64_ty.into()], false);
                let fn_ty = unc_f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_dyn_f64", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_float_value();
                let c = fn_val.get_nth_param(1).unwrap().into_float_value();
                let mut sv = unc_f64_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "udyf_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, c, 1, "udyf_conf").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, i64_ty.const_int(2, false), 2, "udyf_src")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("uncertain_dyn_f64".to_string(), fn_val);
                self.fn_return_types
                    .insert("uncertain_dyn_f64".to_string(), Type::Uncertain(Box::new(Type::F64)));
            }

            // temporal_new(value: i64, horizon_ms: i64, decay: f64) -> Temporal<i64>
            // valid_until_ms = __axon_now_ms() + horizon_ms; confidence starts at 1.0.
            {
                let now_fn = self.ir
                    .module
                    .get_function("__axon_now_ms")
                    .unwrap_or_else(|| {
                        let now_ty = i64_ty.fn_type(&[], false);
                        self.ir.module.add_function("__axon_now_ms", now_ty, None)
                    });
                let fn_ty = tmp_ty.fn_type(&[i64_ty.into(), i64_ty.into(), f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("temporal_new", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let v = fn_val.get_nth_param(0).unwrap().into_int_value();
                let horizon = fn_val.get_nth_param(1).unwrap().into_int_value();
                let decay = fn_val.get_nth_param(2).unwrap().into_float_value();
                let now = self.ir
                    .builder
                    .build_call(now_fn, &[], "tn_now")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                let valid_until = self.ir
                    .builder
                    .build_int_add(now, horizon, "tn_valid_until")
                    .unwrap();
                let one = f64_ty.const_float(1.0);
                let mut sv = tmp_ty.get_undef();
                sv = self.ir.builder.build_insert_value(sv, v, 0, "tn_val").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, one, 1, "tn_conf").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, horizon, 2, "tn_hor").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, decay, 3, "tn_decay").unwrap().into_struct_value();
                sv = self.ir.builder
                    .build_insert_value(sv, valid_until, 4, "tn_vu")
                    .unwrap()
                    .into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("temporal_new".to_string(), fn_val);
                self.fn_return_types
                    .insert("temporal_new".to_string(), Type::Temporal(Box::new(Type::I64)));
            }

            // temporal_at(t: Temporal<i64>, offset_ms: i64) -> Temporal<i64>
            // Recompute confidence as c * (1 - decay)^(offset_ms / 86_400_000).
            // Implementation: linear approximation — c_new = c * max(0, 1 - decay * days)
            // where days = offset_ms / 86_400_000.0. This avoids pulling in pow().
            // valid_until_ms is shifted by offset_ms.
            {
                let fn_ty = tmp_ty.fn_type(&[tmp_ty.into(), i64_ty.into()], false);
                let fn_val = self.ir.module.add_function("temporal_at", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let t = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let offset_ms = fn_val.get_nth_param(1).unwrap().into_int_value();
                let conf = self.ir
                    .builder
                    .build_extract_value(t, 1, "ta_conf")
                    .unwrap()
                    .into_float_value();
                let decay = self.ir
                    .builder
                    .build_extract_value(t, 3, "ta_decay")
                    .unwrap()
                    .into_float_value();
                let valid_until = self.ir
                    .builder
                    .build_extract_value(t, 4, "ta_vu")
                    .unwrap()
                    .into_int_value();
                // days = (f64) offset_ms / 86_400_000.0
                let offset_f = self.ir
                    .builder
                    .build_signed_int_to_float(offset_ms, f64_ty, "ta_offf")
                    .unwrap();
                let day_ms = f64_ty.const_float(86_400_000.0);
                let days = self.ir.builder.build_float_div(offset_f, day_ms, "ta_days").unwrap();
                // factor = max(0, 1 - decay * days)
                let one = f64_ty.const_float(1.0);
                let zero = f64_ty.const_float(0.0);
                let dd = self.ir.builder.build_float_mul(decay, days, "ta_dd").unwrap();
                let one_minus = self.ir.builder.build_float_sub(one, dd, "ta_1md").unwrap();
                let is_neg = self.ir
                    .builder
                    .build_float_compare(inkwell::FloatPredicate::OLT, one_minus, zero, "ta_neg")
                    .unwrap();
                let factor = self.ir
                    .builder
                    .build_select(is_neg, zero, one_minus, "ta_factor")
                    .unwrap()
                    .into_float_value();
                let new_conf = self.ir.builder.build_float_mul(conf, factor, "ta_nc").unwrap();
                let new_valid = self.ir
                    .builder
                    .build_int_add(valid_until, offset_ms, "ta_nvu")
                    .unwrap();
                // Build new struct, preserving value/horizon/decay.
                let mut sv = t;
                sv = self.ir.builder.build_insert_value(sv, new_conf, 1, "ta_iconf").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, new_valid, 4, "ta_ivu").unwrap().into_struct_value();
                self.ir.builder.build_return(Some(&sv)).unwrap();
                self.functions.insert("temporal_at".to_string(), fn_val);
                self.fn_return_types
                    .insert("temporal_at".to_string(), Type::Temporal(Box::new(Type::I64)));
            }

            // temporal_is_valid(t: Temporal<i64>) -> bool
            // Returns __axon_now_ms() <= valid_until_ms.
            {
                let now_fn = self.ir
                    .module
                    .get_function("__axon_now_ms")
                    .unwrap_or_else(|| {
                        let now_ty = i64_ty.fn_type(&[], false);
                        self.ir.module.add_function("__axon_now_ms", now_ty, None)
                    });
                let bool_ty = self.ir.context.bool_type();
                let fn_ty = bool_ty.fn_type(&[tmp_ty.into()], false);
                let fn_val = self.ir.module.add_function("temporal_is_valid", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let t = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let valid_until = self.ir
                    .builder
                    .build_extract_value(t, 4, "tiv_vu")
                    .unwrap()
                    .into_int_value();
                let now = self.ir
                    .builder
                    .build_call(now_fn, &[], "tiv_now")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                let cmp = self.ir
                    .builder
                    .build_int_compare(IntPredicate::SLE, now, valid_until, "tiv_cmp")
                    .unwrap();
                self.ir.builder.build_return(Some(&cmp)).unwrap();
                self.functions.insert("temporal_is_valid".to_string(), fn_val);
                self.fn_return_types
                    .insert("temporal_is_valid".to_string(), Type::Bool);
            }

            // Stub bodies for the legacy `uncertain_confidence` / `temporal_now`
            // helpers, so callers compile even when they predate the new API.
            // uncertain_confidence(confidence: f64) -> () (no-op)
            if self.ir.module.get_function("uncertain_confidence").is_none() {
                let fn_ty = self.ir.context.void_type().fn_type(&[f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_confidence", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                self.ir.builder.build_return(None).unwrap();
                self.functions.insert("uncertain_confidence".to_string(), fn_val);
                self.fn_return_types.insert("uncertain_confidence".to_string(), Type::Unit);
            }
            // temporal_now() -> i64 (delegates to __axon_now_ms)
            if self.ir.module.get_function("temporal_now").is_none() {
                let now_fn = self.ir
                    .module
                    .get_function("__axon_now_ms")
                    .unwrap_or_else(|| {
                        let now_ty = i64_ty.fn_type(&[], false);
                        self.ir.module.add_function("__axon_now_ms", now_ty, None)
                    });
                let fn_ty = i64_ty.fn_type(&[], false);
                let fn_val = self.ir.module.add_function("temporal_now", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let n = self.ir
                    .builder
                    .build_call(now_fn, &[], "tnow")
                    .unwrap()
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                self.ir.builder.build_return(Some(&n)).unwrap();
                self.functions.insert("temporal_now".to_string(), fn_val);
                self.fn_return_types.insert("temporal_now".to_string(), Type::I64);
            }

            if let Some(b) = saved {
                self.ir.builder.position_at_end(b);
            }
        }

        // ── Phase 4: read_line() -> str ────────────────────────────────────────
        // The runtime function `__axon_read_line(out_len: *i64, out_ptr: **u8)` allocates
        // a heap buffer. The codegen wrapper allocates the out-params on the stack and
        // packages the result into the Axon `{ i64, i8* }` str struct.
        {
            let i64_ty = self.ir.context.i64_type();
            let i8_ptr  = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty  = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let rt_ty = void_ty.fn_type(&[i64_ptr.into(), i8_ptr_ptr.into()], false);
            let rt_fn = self.ir.module.add_function("__axon_read_line", rt_ty, None);

            let fn_ty = str_ty.fn_type(&[], false);
            let fn_val = self.ir.module.add_function("read_line", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);

            let len_slot = self.ir.builder.build_alloca(i64_ty, "read_len").unwrap();
            let ptr_slot = self.ir.builder.build_alloca(i8_ptr, "read_ptr").unwrap();
            let ptr_slot_cast = self.ir.builder.build_pointer_cast(ptr_slot, i8_ptr_ptr, "ptrptr").unwrap();
            self.ir.builder.build_call(rt_fn, &[len_slot.into(), ptr_slot_cast.into()], "").unwrap();

            let len_val = self.ir.builder.build_load(i64_ty, len_slot, "len").unwrap().into_int_value();
            let ptr_val = self.ir.builder.build_load(i8_ptr, ptr_slot, "ptr").unwrap().into_pointer_value();

            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, len_val, 0, "str0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, ptr_val, 1, "str1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("read_line".to_string(), fn_val);
            self.fn_return_types.insert("read_line".to_string(), Type::Str);
        }

        // ── Phase 4: read_file(path: str) -> Result<str, str> ─────────────────
        // Runtime: __axon_read_file(path_ptr, path_len, out_len: *i64, out_ptr: **u8)
        // Result<str,str> = { i1 tag, [16 x i8] payload }
        // tag=1 → Ok; payload = str{len, ptr}. tag=0 → Err; payload = str{|len|, ptr}.
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.add_function("__axon_read_file", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("read_file", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "rf_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "rf_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "rf_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let path_str = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let path_len = self.ir.builder.build_extract_value(path_str, 0, "rf_plen").unwrap().into_int_value();
            let path_ptr_v = self.ir.builder.build_extract_value(path_str, 1, "rf_pptr").unwrap().into_pointer_value();

            let out_len_slot = self.ir.builder.build_alloca(i64_ty, "rf_out_len").unwrap();
            let out_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "rf_out_ptr").unwrap();
            let out_ptr_cast = self.ir.builder.build_pointer_cast(out_ptr_slot, i8_ptr_ptr, "rf_ptrptr").unwrap();
            self.ir.builder.build_call(rt_fn, &[path_ptr_v.into(), path_len.into(), out_len_slot.into(), out_ptr_cast.into()], "").unwrap();

            let out_len = self.ir.builder.build_load(i64_ty, out_len_slot, "rf_len").unwrap().into_int_value();
            let out_ptr = self.ir.builder.build_load(i8_ptr, out_ptr_slot, "rf_ptr").unwrap().into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, out_len, zero_i64, "rf_is_ok").unwrap();
            self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = self.ir.builder.build_alloca(result_ty, "rf_ok_slot").unwrap();
            let tag_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "rf_tag_ok").unwrap();
            self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "rf_pay_ok").unwrap();
            let str_ok_ptr = self.ir.builder.build_pointer_cast(payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_ok_ptr").unwrap();
            let str_ok_slot = self.ir.builder.build_alloca(str_ty, "rf_str_ok").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_ok_slot, 0, "").unwrap(), out_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_ok_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_ok_val = self.ir.builder.build_load(str_ty, str_ok_slot, "rf_str_ok_val").unwrap();
            self.ir.builder.build_store(str_ok_ptr, str_ok_val).unwrap();
            let ok_val = self.ir.builder.build_load(result_ty, ok_alloca, "rf_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = self.ir.builder.build_int_neg(out_len, "rf_actual_len").unwrap();
            let err_alloca = self.ir.builder.build_alloca(result_ty, "rf_err_slot").unwrap();
            let tag_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "rf_tag_err").unwrap();
            self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "rf_pay_err").unwrap();
            let str_err_ptr = self.ir.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_err_ptr").unwrap();
            let str_err_slot = self.ir.builder.build_alloca(str_ty, "rf_str_err").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), actual_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "rf_str_err_val").unwrap();
            self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
            let err_val = self.ir.builder.build_load(result_ty, err_alloca, "rf_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("read_file".to_string(), fn_val);
        }

        // ── Phase 4: write_file(path: str, content: str) -> Result<(), str> ───
        // Runtime: __axon_write_file(path_ptr, path_len, content_ptr, content_len, out_err_len: *i64, out_err_ptr: **u8)
        // err_len==0 → Ok(()); err_len>0 → Err(str{err_len, err_ptr})
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.add_function("__axon_write_file", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("write_file", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "wf_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "wf_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "wf_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let path_str    = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let content_str = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let path_len    = self.ir.builder.build_extract_value(path_str, 0, "wf_plen").unwrap().into_int_value();
            let path_ptr_v  = self.ir.builder.build_extract_value(path_str, 1, "wf_pptr").unwrap().into_pointer_value();
            let cont_len    = self.ir.builder.build_extract_value(content_str, 0, "wf_clen").unwrap().into_int_value();
            let cont_ptr    = self.ir.builder.build_extract_value(content_str, 1, "wf_cptr").unwrap().into_pointer_value();

            let err_len_slot = self.ir.builder.build_alloca(i64_ty, "wf_err_len").unwrap();
            let err_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "wf_err_ptr").unwrap();
            let err_ptr_cast = self.ir.builder.build_pointer_cast(err_ptr_slot, i8_ptr_ptr, "wf_ptrptr").unwrap();
            self.ir.builder.build_store(err_len_slot, i64_ty.const_int(0, false)).unwrap();
            self.ir.builder.build_store(err_ptr_slot, i8_ptr.const_null()).unwrap();

            self.ir.builder.build_call(rt_fn, &[path_ptr_v.into(), path_len.into(), cont_ptr.into(), cont_len.into(), err_len_slot.into(), err_ptr_cast.into()], "").unwrap();

            let err_len = self.ir.builder.build_load(i64_ty, err_len_slot, "wf_err_len_val").unwrap().into_int_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, err_len, zero_i64, "wf_is_ok").unwrap();
            self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=zeroed }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = self.ir.builder.build_alloca(result_ty, "wf_ok_slot").unwrap();
            let tag_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "wf_tag_ok").unwrap();
            self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "wf_pay_ok").unwrap();
            let zero_arr = self.ir.context.i8_type().array_type(16).const_zero();
            self.ir.builder.build_store(payload_ok, zero_arr).unwrap();
            let ok_val = self.ir.builder.build_load(result_ty, ok_alloca, "wf_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: { tag=0, payload=str{err_len, err_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let err_ptr = self.ir.builder.build_load(i8_ptr, err_ptr_slot, "wf_err_ptr_val").unwrap().into_pointer_value();
            let err_alloca = self.ir.builder.build_alloca(result_ty, "wf_err_slot").unwrap();
            let tag_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "wf_tag_err").unwrap();
            self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "wf_pay_err").unwrap();
            let str_err_ptr = self.ir.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "wf_str_err_ptr").unwrap();
            let str_err_slot = self.ir.builder.build_alloca(str_ty, "wf_str_err").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), err_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), err_ptr).unwrap();
            let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "wf_str_err_val").unwrap();
            self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
            let err_val = self.ir.builder.build_load(result_ty, err_alloca, "wf_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("write_file".to_string(), fn_val);
        }

        self.declare_string_builtins();
        // ── Phase 7: min_f64 / max_f64 ───────────────────────────────────────
        for (fname, is_min) in &[("min_f64", true), ("max_f64", false)] {
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "mf_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_float_value();
            let b = fn_val.get_nth_param(1).unwrap().into_float_value();
            let pred = if *is_min { inkwell::FloatPredicate::OLT } else { inkwell::FloatPredicate::OGT };
            let cmp = self.ir.builder.build_float_compare(pred, a, b, "mf_cmp").unwrap();
            let result = self.ir.builder.build_select(cmp, a, b, "mf_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::F64);
        }

        // ── Phase 7: clamp_i64(n: i64, lo: i64, hi: i64) -> i64 ─────────────
        {
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("clamp_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ci_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let n  = fn_val.get_nth_param(0).unwrap().into_int_value();
            let lo = fn_val.get_nth_param(1).unwrap().into_int_value();
            let hi = fn_val.get_nth_param(2).unwrap().into_int_value();
            // max(lo, min(n, hi))
            let lt_hi = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, n, hi, "ci_lthi").unwrap();
            let n_or_hi = self.ir.builder.build_select(lt_hi, n, hi, "ci_nhi").unwrap().into_int_value();
            let gt_lo = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGT, n_or_hi, lo, "ci_gtlo").unwrap();
            let result = self.ir.builder.build_select(gt_lo, n_or_hi, lo, "ci_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("clamp_i64".to_string(), fn_val);
            self.fn_return_types.insert("clamp_i64".to_string(), Type::I64);
        }

        // ── Phase 7: clamp_f64(n: f64, lo: f64, hi: f64) -> f64 ─────────────
        {
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("clamp_f64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "cf_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let n  = fn_val.get_nth_param(0).unwrap().into_float_value();
            let lo = fn_val.get_nth_param(1).unwrap().into_float_value();
            let hi = fn_val.get_nth_param(2).unwrap().into_float_value();
            // max(lo, min(n, hi))
            let lt_hi = self.ir.builder.build_float_compare(inkwell::FloatPredicate::OLT, n, hi, "cf_lthi").unwrap();
            let n_or_hi = self.ir.builder.build_select(lt_hi, n, hi, "cf_nhi").unwrap().into_float_value();
            let gt_lo = self.ir.builder.build_float_compare(inkwell::FloatPredicate::OGT, n_or_hi, lo, "cf_gtlo").unwrap();
            let result = self.ir.builder.build_select(gt_lo, n_or_hi, lo, "cf_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("clamp_f64".to_string(), fn_val);
            self.fn_return_types.insert("clamp_f64".to_string(), Type::F64);
        }

        // ── Phase 7: parse_bool(s: str) -> Result<bool, str> ─────────────────
        // Accepts "true"/"false" (exact, lowercase). Returns Ok(bool) or Err("invalid bool").
        // Result<bool,str> layout: { i1 tag, [16 x i8] payload }
        // (bool=1 byte, str=16 bytes → max=16; same layout as Result<f64,str>)
        {
            let str_ty  = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i1_ty   = self.ir.context.bool_type();
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            // tag is stored as i1 (matches parse_float convention)
            let result_ty = self.ir.context.struct_type(&[i1_ty.into(), i8_arr16_ty.into()], false);

            let strncmp_fn = self.ir.module.get_function("strncmp").unwrap_or_else(|| {
                let ft = self.ir.context.i32_type().fn_type(
                    &[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("strncmp", ft, None)
            });

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_bool", fn_ty, None);

            let entry_bb    = self.ir.context.append_basic_block(fn_val, "pb_entry");
            let check_f_bb  = self.ir.context.append_basic_block(fn_val, "pb_chk_false");
            let ok_true_bb  = self.ir.context.append_basic_block(fn_val, "pb_ok_true");
            let ok_false_bb = self.ir.context.append_basic_block(fn_val, "pb_ok_false");
            let err_bb      = self.ir.context.append_basic_block(fn_val, "pb_err");

            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "pb_slen").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "pb_sptr").unwrap().into_pointer_value();

            // Check s == "true": len==4 && strncmp(s_ptr,"true",4)==0
            let len4 = i64_ty.const_int(4, false);
            let is_len4 = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ, s_len, len4, "pb_l4").unwrap();
            let true_lit_g = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(5), None, "pb_true_lit");
            true_lit_g.set_initializer(&self.ir.context.const_string(b"true", true));
            true_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let true_lit = true_lit_g.as_pointer_value();
            let cmp_t = self.ir.builder.build_call(strncmp_fn,
                &[s_ptr.into(), true_lit.into(), len4.into()], "pb_cmpt").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_t_eq = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ, cmp_t,
                self.ir.context.i32_type().const_int(0, false), "pb_teq").unwrap();
            let is_true_str = self.ir.builder.build_and(is_len4, cmp_t_eq, "pb_istrue").unwrap();
            self.ir.builder.build_conditional_branch(is_true_str, ok_true_bb, check_f_bb).unwrap();

            // check_f_bb: check s == "false": len==5 && strncmp(s_ptr,"false",5)==0
            self.ir.builder.position_at_end(check_f_bb);
            let len5 = i64_ty.const_int(5, false);
            let is_len5 = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ, s_len, len5, "pb_l5").unwrap();
            let false_lit_g = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(6), None, "pb_false_lit");
            false_lit_g.set_initializer(&self.ir.context.const_string(b"false", true));
            false_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let false_lit = false_lit_g.as_pointer_value();
            let cmp_f = self.ir.builder.build_call(strncmp_fn,
                &[s_ptr.into(), false_lit.into(), len5.into()], "pb_cmpf").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_f_eq = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ, cmp_f,
                self.ir.context.i32_type().const_int(0, false), "pb_feq").unwrap();
            let is_false_str = self.ir.builder.build_and(is_len5, cmp_f_eq, "pb_isfalse").unwrap();
            self.ir.builder.build_conditional_branch(is_false_str, ok_false_bb, err_bb).unwrap();

            // ok_true_bb: tag=1, payload = i1 true cast to [16 x i8]
            self.ir.builder.position_at_end(ok_true_bb);
            {
                let ok_alloca = self.ir.builder.build_alloca(result_ty, "pb_ot_slot").unwrap();
                self.ir.builder.build_store(
                    self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "pb_ot_tag").unwrap(),
                    i1_ty.const_int(1, false)).unwrap();
                let payload_ptr = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "pb_ot_pay").unwrap();
                let bool_ptr = self.ir.builder.build_pointer_cast(
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_ot_bptr").unwrap();
                self.ir.builder.build_store(bool_ptr, i1_ty.const_int(1, false)).unwrap();
                let val = self.ir.builder.build_load(result_ty, ok_alloca, "pb_ot_val").unwrap();
                self.ir.builder.build_return(Some(&val)).unwrap();
            }

            // ok_false_bb: tag=1, payload = i1 false cast to [16 x i8]
            self.ir.builder.position_at_end(ok_false_bb);
            {
                let ok_alloca = self.ir.builder.build_alloca(result_ty, "pb_of_slot").unwrap();
                self.ir.builder.build_store(
                    self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "pb_of_tag").unwrap(),
                    i1_ty.const_int(1, false)).unwrap();
                let payload_ptr = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "pb_of_pay").unwrap();
                let bool_ptr = self.ir.builder.build_pointer_cast(
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_of_bptr").unwrap();
                self.ir.builder.build_store(bool_ptr, i1_ty.const_int(0, false)).unwrap();
                let val = self.ir.builder.build_load(result_ty, ok_alloca, "pb_of_val").unwrap();
                self.ir.builder.build_return(Some(&val)).unwrap();
            }

            // err_bb: tag=0, payload = str{"invalid bool"} cast to [16 x i8]
            self.ir.builder.position_at_end(err_bb);
            {
                let err_alloca = self.ir.builder.build_alloca(result_ty, "pb_err_slot").unwrap();
                self.ir.builder.build_store(
                    self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "pb_err_tag").unwrap(),
                    i1_ty.const_int(0, false)).unwrap();
                let payload_ptr = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "pb_err_pay").unwrap();
                let str_ptr = self.ir.builder.build_pointer_cast(
                    payload_ptr, str_ty.ptr_type(inkwell::AddressSpace::default()), "pb_err_sptr").unwrap();
                let err_str_alloca = self.ir.builder.build_alloca(str_ty, "pb_err_s").unwrap();
                let err_msg = b"invalid bool";
                let err_lit_g = self.ir.module.add_global(
                    self.ir.context.i8_type().array_type(err_msg.len() as u32 + 1),
                    None, "pb_err_msg");
                err_lit_g.set_initializer(&self.ir.context.const_string(err_msg, true));
                err_lit_g.set_linkage(inkwell::module::Linkage::Private);
                let err_lit = err_lit_g.as_pointer_value();
                self.ir.builder.build_store(
                    self.ir.builder.build_struct_gep(str_ty, err_str_alloca, 0, "pb_esl").unwrap(),
                    i64_ty.const_int(err_msg.len() as u64, false)).unwrap();
                self.ir.builder.build_store(
                    self.ir.builder.build_struct_gep(str_ty, err_str_alloca, 1, "pb_esp").unwrap(),
                    err_lit).unwrap();
                let err_str_val = self.ir.builder.build_load(str_ty, err_str_alloca, "pb_esv").unwrap();
                self.ir.builder.build_store(str_ptr, err_str_val).unwrap();
                let val = self.ir.builder.build_load(result_ty, err_alloca, "pb_err_val").unwrap();
                self.ir.builder.build_return(Some(&val)).unwrap();
            }

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_bool".to_string(), fn_val);
            self.fn_return_types.insert("parse_bool".to_string(),
                Type::Result(Box::new(Type::Bool), Box::new(Type::Str)));
        }

        // ── Phase 7: random_i64(lo: i64, hi: i64) -> i64 ─────────────────────
        // Uses C rand() % (hi - lo) + lo. Behavior undefined if hi <= lo.
        {
            let rand_fn = self.ir.module.get_function("rand").unwrap_or_else(|| {
                let ft = self.ir.context.i32_type().fn_type(&[], false);
                self.ir.module.add_function("rand", ft, None)
            });
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("random_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ri_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let lo = fn_val.get_nth_param(0).unwrap().into_int_value();
            let hi = fn_val.get_nth_param(1).unwrap().into_int_value();
            let r_i32 = self.ir.builder.build_call(rand_fn, &[], "ri_rand").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let r = self.ir.builder.build_int_s_extend(r_i32, i64_ty, "ri_r64").unwrap();
            let range = self.ir.builder.build_int_sub(hi, lo, "ri_range").unwrap();
            let r_mod = self.ir.builder.build_int_signed_rem(r, range, "ri_mod").unwrap();
            // Ensure non-negative: (r_mod + range) % range
            let r_pos = self.ir.builder.build_int_add(r_mod, range, "ri_pos").unwrap();
            let r_final = self.ir.builder.build_int_signed_rem(r_pos, range, "ri_final").unwrap();
            let result = self.ir.builder.build_int_add(r_final, lo, "ri_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("random_i64".to_string(), fn_val);
            self.fn_return_types.insert("random_i64".to_string(), Type::I64);
        }

        // ── Phase 7: random_f64() -> f64 ─────────────────────────────────────
        // Returns rand() / (RAND_MAX + 1.0) in [0.0, 1.0).
        {
            let f64_ty = self.ir.context.f64_type();
            let rand_fn = self.ir.module.get_function("rand").unwrap_or_else(|| {
                let ft = self.ir.context.i32_type().fn_type(&[], false);
                self.ir.module.add_function("rand", ft, None)
            });
            let fn_ty = f64_ty.fn_type(&[], false);
            let fn_val = self.ir.module.add_function("random_f64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "rf_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let r_i32 = self.ir.builder.build_call(rand_fn, &[], "rf_rand").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            // Convert to f64
            let r_f = self.ir.builder.build_signed_int_to_float(r_i32, f64_ty, "rf_f").unwrap();
            // RAND_MAX = 2147483647 → divisor = 2147483648.0
            let divisor = f64_ty.const_float(2147483648.0);
            let result = self.ir.builder.build_float_div(r_f, divisor, "rf_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("random_f64".to_string(), fn_val);
            self.fn_return_types.insert("random_f64".to_string(), Type::F64);
        }

        self.declare_phase9_math_builtins();
        // ── Phase 10: str_count(s: str, needle: str) -> i64 ─────────────────
        // Count non-overlapping occurrences of needle in s.
        // Algorithm: walk s with strstr, advance past each match by needle_len.
        // Returns 0 when needle is empty or not found.
        //
        // CFG:
        //   entry     → early_ret (needle_len == 0)
        //             → loop     (needle_len > 0)
        //   loop      → found    (strstr != null)
        //             → done     (strstr == null)
        //   found     → loop
        //   early_ret : return 0
        //   done      : return count
        //
        // Allocas are placed in entry_bb (before the branch) so they dominate
        // all successors, keeping the IR valid even without mem2reg.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_count", fn_ty, None);

            let entry_bb     = self.ir.context.append_basic_block(fn_val, "sc_entry");
            let early_ret_bb = self.ir.context.append_basic_block(fn_val, "sc_early_ret");
            let loop_bb      = self.ir.context.append_basic_block(fn_val, "sc_loop");
            let found_bb     = self.ir.context.append_basic_block(fn_val, "sc_found");
            let done_bb      = self.ir.context.append_basic_block(fn_val, "sc_done");
            let saved = self.ir.builder.get_insert_block();

            // ── entry: extract fields, place allocas, then branch ───────────
            self.ir.builder.position_at_end(entry_bb);
            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr      = self.ir.builder.build_extract_value(s, 1, "sc_sptr").unwrap().into_pointer_value();
            let needle_len = self.ir.builder.build_extract_value(needle, 0, "sc_nlen").unwrap().into_int_value();
            let needle_ptr = self.ir.builder.build_extract_value(needle, 1, "sc_nptr").unwrap().into_pointer_value();
            let zero = i64_ty.const_zero();

            // Allocas here so they dominate all successors (including done_bb).
            let cur_slot   = self.ir.builder.build_alloca(i8_ptr, "sc_cur").unwrap();
            let count_slot = self.ir.builder.build_alloca(i64_ty, "sc_cnt").unwrap();
            self.ir.builder.build_store(cur_slot, s_ptr).unwrap();
            self.ir.builder.build_store(count_slot, zero).unwrap();

            let strstr_fn = self.ir.module.get_function("strstr").unwrap_or_else(|| {
                let t = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.ir.module.add_function("strstr", t, None)
            });

            let needle_empty = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ, needle_len, zero, "sc_nempty",
            ).unwrap();
            self.ir.builder.build_conditional_branch(needle_empty, early_ret_bb, loop_bb).unwrap();

            // ── early_ret: return 0 for empty needle ─────────────────────────
            self.ir.builder.position_at_end(early_ret_bb);
            self.ir.builder.build_return(Some(&zero)).unwrap();

            // ── loop: cur = strstr(cur, needle); branch on null ──────────────
            self.ir.builder.position_at_end(loop_bb);
            let cur = self.ir.builder.build_load(i8_ptr, cur_slot, "sc_cur_v").unwrap().into_pointer_value();
            let found_ptr = self.ir.builder.build_call(
                strstr_fn, &[cur.into(), needle_ptr.into()], "sc_fp",
            ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
            let found_int = self.ir.builder.build_ptr_to_int(found_ptr, i64_ty, "sc_fpi").unwrap();
            let null_int  = self.ir.builder.build_ptr_to_int(i8_ptr.const_null(), i64_ty, "sc_ni").unwrap();
            let is_found = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::NE, found_int, null_int, "sc_isf",
            ).unwrap();
            self.ir.builder.build_conditional_branch(is_found, found_bb, done_bb).unwrap();

            // ── found: count++, advance cursor past the match ────────────────
            self.ir.builder.position_at_end(found_bb);
            let cnt = self.ir.builder.build_load(i64_ty, count_slot, "sc_cnt_v").unwrap().into_int_value();
            let cnt1 = self.ir.builder.build_int_add(cnt, i64_ty.const_int(1, false), "sc_cnt1").unwrap();
            self.ir.builder.build_store(count_slot, cnt1).unwrap();
            let next = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), found_ptr, &[needle_len], "sc_next").unwrap()
            };
            self.ir.builder.build_store(cur_slot, next).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: return accumulated count ───────────────────────────────
            self.ir.builder.position_at_end(done_bb);
            let final_count = self.ir.builder.build_load(i64_ty, count_slot, "sc_final").unwrap().into_int_value();
            self.ir.builder.build_return(Some(&final_count)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_count".to_string(), fn_val);
            self.fn_return_types.insert("str_count".to_string(), Type::I64);
        }


        // ── Phase 10: str_reverse(s: str) -> str ─────────────────────────────
        // Returns a malloc'd copy of s with bytes in reverse order.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_reverse", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "srev_entry");
            let loop_bb  = self.ir.context.append_basic_block(fn_val, "srev_loop");
            let body_bb  = self.ir.context.append_basic_block(fn_val, "srev_body");
            let done_bb  = self.ir.context.append_basic_block(fn_val, "srev_done");
            let saved = self.ir.builder.get_insert_block();

            self.ir.builder.position_at_end(entry_bb);
            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "srev_len").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "srev_ptr").unwrap().into_pointer_value();

            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            // alloc s_len + 1 bytes
            let alloc_sz = self.ir.builder.build_int_add(s_len, i64_ty.const_int(1, false), "srev_az").unwrap();
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_sz.into()], "srev_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            // i = 0; loop while i < s_len
            let zero = i64_ty.const_zero();
            let i_slot = self.ir.builder.build_alloca(i64_ty, "srev_i").unwrap();
            self.ir.builder.build_store(i_slot, zero).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            self.ir.builder.position_at_end(loop_bb);
            let i_val = self.ir.builder.build_load(i64_ty, i_slot, "srev_iv").unwrap().into_int_value();
            let in_range = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, s_len, "srev_ir").unwrap();
            self.ir.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // body: buf[i] = s_ptr[s_len - 1 - i]; i++
            self.ir.builder.position_at_end(body_bb);
            let src_idx = self.ir.builder.build_int_sub(
                self.ir.builder.build_int_sub(s_len, i64_ty.const_int(1, false), "srev_sm1").unwrap(),
                i_val,
                "srev_si",
            ).unwrap();
            let src_byte_ptr = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), s_ptr, &[src_idx], "srev_sbp").unwrap()
            };
            let byte = self.ir.builder.build_load(self.ir.context.i8_type(), src_byte_ptr, "srev_b").unwrap();
            let dst_byte_ptr = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[i_val], "srev_dbp").unwrap()
            };
            self.ir.builder.build_store(dst_byte_ptr, byte).unwrap();
            let next_i = self.ir.builder.build_int_add(i_val, i64_ty.const_int(1, false), "srev_ni").unwrap();
            self.ir.builder.build_store(i_slot, next_i).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // done: null-terminate and return
            self.ir.builder.position_at_end(done_bb);
            let null_pos = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[s_len], "srev_np").unwrap() };
            self.ir.builder.build_store(null_pos, self.ir.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, s_len, 0, "srev_r0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, buf, 1, "srev_r1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_reverse".to_string(), fn_val);
            self.fn_return_types.insert("str_reverse".to_string(), Type::Str);
        }


        // ── Phase 10: i64_to_str_radix(n: i64, base: i64) -> str ─────────────
        // Convert n to string in given base (2-36). Negative n gets '-' prefix.
        // Delegates to __axon_i64_to_str_radix in the runtime via out-params.
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            // Runtime: void __axon_i64_to_str_radix(i64 n, i64 base, i64* out_len, i8** out_ptr)
            let void_ty = self.ir.context.void_type();
            let rt_fn_ty = void_ty.fn_type(
                &[i64_ty.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.get_function("__axon_i64_to_str_radix").unwrap_or_else(|| {
                self.ir.module.add_function("__axon_i64_to_str_radix", rt_fn_ty, None)
            });

            let fn_ty = str_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("i64_to_str_radix", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let n    = fn_val.get_nth_param(0).unwrap().into_int_value();
            let base = fn_val.get_nth_param(1).unwrap().into_int_value();

            // Stack slots for out-params.
            let out_len_slot = self.ir.builder.build_alloca(i64_ty, "radix_olen").unwrap();
            let out_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "radix_optr").unwrap();
            // Cast *i8* → i8** for the runtime call.
            let out_ptr_slot_cast = self.ir.builder.build_pointer_cast(
                out_ptr_slot, i8_ptr_ptr, "radix_ptrptr",
            ).unwrap();

            self.ir.builder.build_call(rt_fn, &[
                n.into(),
                base.into(),
                out_len_slot.into(),
                out_ptr_slot_cast.into(),
            ], "radix_call").unwrap();

            let out_len = self.ir.builder.build_load(i64_ty, out_len_slot, "radix_len").unwrap().into_int_value();
            let out_ptr = self.ir.builder.build_load(i8_ptr, out_ptr_slot, "radix_ptr").unwrap().into_pointer_value();

            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, out_len, 0, "radix_r0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, out_ptr, 1, "radix_r1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("i64_to_str_radix".to_string(), fn_val);
            self.fn_return_types.insert("i64_to_str_radix".to_string(), Type::Str);
        }

        self.declare_ai_builtins();
        self.declare_asi_runtime_builtins();
    }


    // ── Phase 3.2 decomposition: per-section helper methods ────────────

    /// Auto-extracted from `declare_builtins` (Phase 3.2 decomposition).
    /// Declares ASI runtime ABI: provenance log, @[verify] panic, adaptive registry, goal_run.
    pub(super) fn declare_asi_runtime_builtins(&mut self) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.ir.context.i32_type();
        let i64_ty = self.ir.context.i64_type();
        let bool_ty = self.ir.context.bool_type();
        let void_ty = self.ir.context.void_type();
        let _ = (i32_ty, bool_ty, void_ty, i8_ptr, i64_ty);

        // ── Provenance log: __axon_provenance_log(name_ptr, name_len, payload_ptr, payload_len) ──
        // Used by `@[adaptive]` injection at fn prologue / return sites for the
        // string-event flavour ("call" / "return").  Not exposed as a user-
        // visible builtin — only the codegen invokes it.
        {
            let prov_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into()],
                false,
            );
            self.ir.module.add_function("__axon_provenance_log", prov_ty, None);
        }

        // ── Layer-2 typed-return provenance: ──────────────────────────────────
        //   __axon_provenance_log_ret_i64(name_ptr, name_len, ret: i64)
        //   __axon_provenance_log_ret_f64(name_ptr, name_len, ret: f64)
        //
        // Codegen calls one of these immediately before `build_return` inside
        // an `@[adaptive]` function whose return value is i64/f64.  Records the
        // return value into the runtime's in-memory provenance store so
        // `__axon_goal_run` can compute a best-observed score.
        {
            let f64_ty = self.ir.context.f64_type();
            let prov_i64_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i64_ty.into()],
                false,
            );
            self.ir.module.add_function("__axon_provenance_log_ret_i64", prov_i64_ty, None);

            let prov_f64_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), f64_ty.into()],
                false,
            );
            self.ir.module.add_function("__axon_provenance_log_ret_f64", prov_f64_ty, None);
        }

        // ── ASI Layer-3: @[verify] runtime panic ──────────────────────────────
        //   __axon_verify_panic(fn_name_ptr, fn_name_len,
        //                       op_ptr, op_len,
        //                       bound: f64, actual: f64) -> noreturn
        //
        // Codegen injects a guarded call to this at every return site of a
        // function carrying `@[verify(confidence OP K)]`.  The static
        // `verify::check_verify` pass remains the primary gate; this runtime
        // hook catches violations whose source is unknown to the static
        // lattice (e.g. confidence flowing in from `ai_extract_uncertain_*`).
        {
            let f64_ty = self.ir.context.f64_type();
            let vp_ty = void_ty.fn_type(
                &[
                    i8_ptr.into(),  // fn_name_ptr
                    i64_ty.into(),  // fn_name_len
                    i8_ptr.into(),  // op_ptr
                    i64_ty.into(),  // op_len
                    f64_ty.into(),  // bound
                    f64_ty.into(),  // actual
                ],
                false,
            );
            // Mark as noreturn at the LLVM level so optimisers know the
            // failure path doesn't fall through.
            let vp_fn = self.ir.module.add_function("__axon_verify_panic", vp_ty, None);
            // (No attribute setting on inkwell 0.4 stable API — codegen still
            // emits an `unreachable` on the failure path so semantic
            // correctness doesn't depend on the noreturn attribute.)
            let _ = vp_fn;
        }

        // ── ASI Layer-3: adaptive registry registration ───────────────────────
        //   __axon_register_adaptive(name_ptr, name_len, fn_ptr)
        //
        // Called from `main`'s prologue once per `@[adaptive] fn(i64) -> i64`
        // so the runtime can call those functions back during goal_run
        // hill-climb.  v1 narrowing: only single-i64-arg, i64-return adaptive
        // fns get registered; all other adaptive fns silently fall through
        // and use the Layer-2 retrospective path.
        {
            let reg_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into()],
                false,
            );
            self.ir.module.add_function("__axon_register_adaptive", reg_ty, None);
        }

        // ── goal_run(name: str, target: f64, max_evals: i64) -> f64 ───────────
        // Runtime: __axon_goal_run(fn_ptr, name_ptr, name_len, target, max_evals, *out_score)
        // Layer-2: reads the in-memory provenance store populated by
        // __axon_provenance_log_ret_{i64,f64} from any @[adaptive] function
        // returns and writes the *best observed score* (closest to `target`)
        // into `*out_score`.  Falls back to `target` when no records exist
        // (preserves Layer-1 stub behaviour for adaptive_basic.ax and similar).
        {
            let f64_ty = self.ir.context.f64_type();
            let f64_ptr = f64_ty.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            let rt_ty = void_ty.fn_type(
                &[
                    i8_ptr.into(),    // fn_ptr (unused in v1)
                    i8_ptr.into(),    // name_ptr
                    i64_ty.into(),    // name_len
                    f64_ty.into(),    // target
                    i64_ty.into(),    // max_evals
                    f64_ptr.into(),   // out_score
                ],
                false,
            );
            let rt_fn = self.ir.module.add_function("__axon_goal_run", rt_ty, None);

            let fn_ty = f64_ty.fn_type(
                &[str_ty.into(), f64_ty.into(), i64_ty.into()],
                false,
            );
            let fn_val = self.ir.module.add_function("goal_run", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "gr_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let name_str  = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let name_len  = self.ir.builder.build_extract_value(name_str, 0, "gr_nlen").unwrap().into_int_value();
            let name_ptr  = self.ir.builder.build_extract_value(name_str, 1, "gr_nptr").unwrap().into_pointer_value();
            let target    = fn_val.get_nth_param(1).unwrap().into_float_value();
            let max_evals = fn_val.get_nth_param(2).unwrap().into_int_value();

            let null_ptr  = i8_ptr.const_null();
            let out_slot  = self.ir.builder.build_alloca(f64_ty, "gr_out_score").unwrap();
            self.ir.builder.build_call(
                rt_fn,
                &[
                    null_ptr.into(),
                    name_ptr.into(),
                    name_len.into(),
                    target.into(),
                    max_evals.into(),
                    out_slot.into(),
                ],
                "",
            ).unwrap();

            let score = self.ir.builder.build_load(f64_ty, out_slot, "gr_score").unwrap();
            self.ir.builder.build_return(Some(&score)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("goal_run".to_string(), fn_val);
            self.fn_return_types.insert("goal_run".to_string(), Type::F64);
        }
    }

    /// Auto-extracted from `declare_builtins` (Phase 3.2 decomposition).
    /// Declares ai_complete, ai_extract_uncertain_{i64,f64}, ai_extract::<T> flat helpers.
    pub(super) fn declare_ai_builtins(&mut self) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.ir.context.i32_type();
        let i64_ty = self.ir.context.i64_type();
        let bool_ty = self.ir.context.bool_type();
        let void_ty = self.ir.context.void_type();
        let _ = (i32_ty, bool_ty, void_ty, i8_ptr, i64_ty);

        // ── AI: ai_complete(prompt: str) -> Result<str, str> ─────────────────────
        // Runtime: __axon_ai_complete(prompt_ptr, prompt_len, out_len: *i64, out_ptr: **u8)
        // Same out-param ABI as __axon_read_file: out_len<0 on error.
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.add_function("__axon_ai_complete", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("ai_complete", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "aic_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "aic_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "aic_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let prompt_str   = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let prompt_len   = self.ir.builder.build_extract_value(prompt_str, 0, "aic_plen").unwrap().into_int_value();
            let prompt_ptr_v = self.ir.builder.build_extract_value(prompt_str, 1, "aic_pptr").unwrap().into_pointer_value();

            let out_len_slot = self.ir.builder.build_alloca(i64_ty, "aic_out_len").unwrap();
            let out_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "aic_out_ptr").unwrap();
            let out_ptr_cast = self.ir.builder.build_pointer_cast(out_ptr_slot, i8_ptr_ptr, "aic_ptrptr").unwrap();
            self.ir.builder.build_call(rt_fn, &[prompt_ptr_v.into(), prompt_len.into(), out_len_slot.into(), out_ptr_cast.into()], "").unwrap();

            let out_len = self.ir.builder.build_load(i64_ty, out_len_slot, "aic_len").unwrap().into_int_value();
            let out_ptr = self.ir.builder.build_load(i8_ptr, out_ptr_slot, "aic_ptr").unwrap().into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, out_len, zero_i64, "aic_is_ok").unwrap();
            self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = self.ir.builder.build_alloca(result_ty, "aic_ok_slot").unwrap();
            let tag_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "aic_tag_ok").unwrap();
            self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "aic_pay_ok").unwrap();
            let str_ok_ptr = self.ir.builder.build_pointer_cast(payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "aic_str_ok_ptr").unwrap();
            let str_ok_slot = self.ir.builder.build_alloca(str_ty, "aic_str_ok").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_ok_slot, 0, "").unwrap(), out_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_ok_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_ok_val = self.ir.builder.build_load(str_ty, str_ok_slot, "aic_str_ok_val").unwrap();
            self.ir.builder.build_store(str_ok_ptr, str_ok_val).unwrap();
            let ok_val = self.ir.builder.build_load(result_ty, ok_alloca, "aic_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = self.ir.builder.build_int_neg(out_len, "aic_actual_len").unwrap();
            let err_alloca = self.ir.builder.build_alloca(result_ty, "aic_err_slot").unwrap();
            let tag_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "aic_tag_err").unwrap();
            self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "aic_pay_err").unwrap();
            let str_err_ptr = self.ir.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aic_str_err_ptr").unwrap();
            let str_err_slot = self.ir.builder.build_alloca(str_ty, "aic_str_err").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), actual_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "aic_str_err_val").unwrap();
            self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
            let err_val = self.ir.builder.build_load(result_ty, err_alloca, "aic_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("ai_complete".to_string(), fn_val);
        }

        // ── ASI Layer-3: ai_extract_uncertain_i64 / ai_extract_uncertain_f64 ───
        // Structured-output extraction via Anthropic tool-use.
        //
        //   ai_extract_uncertain_i64(prompt: str) -> Result<Uncertain<i64>, str>
        //   ai_extract_uncertain_f64(prompt: str) -> Result<Uncertain<f64>, str>
        //
        // Runtime ABI (defined in axon-ai/src/lib.rs):
        //   i32 __axon_ai_extract_uncertain_i64(
        //       prompt_ptr, prompt_len,
        //       out_value: *i64, out_confidence: *f64,
        //       out_err_len: *i64, out_err_ptr: **u8) -> 0 ok | 1 err
        //   i32 __axon_ai_extract_uncertain_f64(
        //       prompt_ptr, prompt_len,
        //       out_value: *f64, out_confidence: *f64,
        //       out_err_len: *i64, out_err_ptr: **u8) -> 0 ok | 1 err
        //
        // Result layout: Uncertain<T> is 24 bytes (T+f64+i64), str is 16 bytes,
        // so the payload union is sized to 24 bytes here (vs 16 for ai_complete).
        // source_tag = 1 (`from AI`); 0 is reserved for user-constructed.
        {
            let f64_ty = self.ir.context.f64_type();
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let f64_ptr = f64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            // Payload is sized to fit Uncertain<i64/f64> (24 bytes); err = str fits in 16.
            let i8_arr24_ty = self.ir.context.i8_type().array_type(24);
            let result_unc_i64_ty = self.ir.context.struct_type(
                &[bool_ty.into(), i8_arr24_ty.into()],
                false,
            );
            let result_unc_f64_ty = result_unc_i64_ty;
            let unc_i64_ty = self.ir.context.struct_type(
                &[i64_ty.into(), f64_ty.into(), i64_ty.into()],
                false,
            );
            let unc_f64_ty = self.ir.context.struct_type(
                &[f64_ty.into(), f64_ty.into(), i64_ty.into()],
                false,
            );

            // Common runtime extern signature factory.
            let make_rt_ty = |val_ptr: inkwell::types::PointerType<'ctx>| {
                i32_ty.fn_type(
                    &[
                        i8_ptr.into(),       // prompt_ptr
                        i64_ty.into(),       // prompt_len
                        val_ptr.into(),      // out_value (*i64 or *f64)
                        f64_ptr.into(),      // out_confidence
                        i64_ptr.into(),      // out_err_len
                        i8_ptr_ptr.into(),   // out_err_ptr
                    ],
                    false,
                )
            };

            // ── ai_extract_uncertain_i64 ──
            {
                let rt_fn = self.ir.module.add_function(
                    "__axon_ai_extract_uncertain_i64",
                    make_rt_ty(i64_ptr),
                    None,
                );

                let fn_ty = result_unc_i64_ty.fn_type(&[str_ty.into()], false);
                let fn_val = self.ir.module.add_function("ai_extract_uncertain_i64", fn_ty, None);

                let entry_bb = self.ir.context.append_basic_block(fn_val, "aei_entry");
                let ok_bb    = self.ir.context.append_basic_block(fn_val, "aei_ok");
                let err_bb   = self.ir.context.append_basic_block(fn_val, "aei_err");
                let saved = self.ir.builder.get_insert_block();
                self.ir.builder.position_at_end(entry_bb);

                let prompt_str   = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let prompt_len   = self.ir.builder.build_extract_value(prompt_str, 0, "aei_plen").unwrap().into_int_value();
                let prompt_ptr_v = self.ir.builder.build_extract_value(prompt_str, 1, "aei_pptr").unwrap().into_pointer_value();

                let out_val_slot   = self.ir.builder.build_alloca(i64_ty, "aei_out_val").unwrap();
                let out_conf_slot  = self.ir.builder.build_alloca(f64_ty, "aei_out_conf").unwrap();
                let out_err_len_slot = self.ir.builder.build_alloca(i64_ty, "aei_out_err_len").unwrap();
                let out_err_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "aei_out_err_ptr").unwrap();
                let out_err_ptr_cast = self.ir.builder
                    .build_pointer_cast(out_err_ptr_slot, i8_ptr_ptr, "aei_eptrptr")
                    .unwrap();

                let rc_call = self.ir.builder.build_call(
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_conf_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aei_rc",
                ).unwrap();
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = self.ir.builder
                    .build_int_compare(IntPredicate::EQ, rc, zero_i32, "aei_is_ok")
                    .unwrap();
                self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

                // ok_bb: build Uncertain<i64> { value, confidence, source_tag=1 }
                // and wrap in Result::Ok.
                self.ir.builder.position_at_end(ok_bb);
                let val = self.ir.builder.build_load(i64_ty, out_val_slot, "aei_val").unwrap().into_int_value();
                let conf = self.ir.builder.build_load(f64_ty, out_conf_slot, "aei_conf").unwrap().into_float_value();
                let mut unc_sv = unc_i64_ty.get_undef();
                unc_sv = self.ir.builder.build_insert_value(unc_sv, val,  0, "aei_unc_v").unwrap().into_struct_value();
                unc_sv = self.ir.builder.build_insert_value(unc_sv, conf, 1, "aei_unc_c").unwrap().into_struct_value();
                unc_sv = self.ir.builder
                    .build_insert_value(unc_sv, i64_ty.const_int(1, false), 2, "aei_unc_s")
                    .unwrap()
                    .into_struct_value();
                let ok_alloca = self.ir.builder.build_alloca(result_unc_i64_ty, "aei_ok_slot").unwrap();
                let tag_ptr_ok = self.ir.builder.build_struct_gep(result_unc_i64_ty, ok_alloca, 0, "aei_tag_ok").unwrap();
                self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
                let payload_ok = self.ir.builder.build_struct_gep(result_unc_i64_ty, ok_alloca, 1, "aei_pay_ok").unwrap();
                let unc_payload_ptr = self.ir.builder
                    .build_pointer_cast(payload_ok, unc_i64_ty.ptr_type(inkwell::AddressSpace::default()), "aei_unc_pp")
                    .unwrap();
                self.ir.builder.build_store(unc_payload_ptr, unc_sv).unwrap();
                let ok_val = self.ir.builder.build_load(result_unc_i64_ty, ok_alloca, "aei_ok_val").unwrap();
                self.ir.builder.build_return(Some(&ok_val)).unwrap();

                // err_bb: read err_len/err_ptr, build str payload, wrap Result::Err.
                self.ir.builder.position_at_end(err_bb);
                let err_len = self.ir.builder.build_load(i64_ty, out_err_len_slot, "aei_elen").unwrap().into_int_value();
                let err_ptr = self.ir.builder.build_load(i8_ptr, out_err_ptr_slot, "aei_eptr").unwrap().into_pointer_value();
                let err_alloca = self.ir.builder.build_alloca(result_unc_i64_ty, "aei_err_slot").unwrap();
                let tag_ptr_err = self.ir.builder.build_struct_gep(result_unc_i64_ty, err_alloca, 0, "aei_tag_err").unwrap();
                self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
                let payload_err = self.ir.builder.build_struct_gep(result_unc_i64_ty, err_alloca, 1, "aei_pay_err").unwrap();
                let str_err_ptr = self.ir.builder
                    .build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aei_str_err_pp")
                    .unwrap();
                let str_err_slot = self.ir.builder.build_alloca(str_ty, "aei_str_err").unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), err_len).unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), err_ptr).unwrap();
                let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "aei_str_err_val").unwrap();
                self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
                let err_val = self.ir.builder.build_load(result_unc_i64_ty, err_alloca, "aei_err_val").unwrap();
                self.ir.builder.build_return(Some(&err_val)).unwrap();

                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert("ai_extract_uncertain_i64".to_string(), fn_val);
                self.fn_return_types.insert(
                    "ai_extract_uncertain_i64".to_string(),
                    Type::Result(
                        Box::new(Type::Uncertain(Box::new(Type::I64))),
                        Box::new(Type::Str),
                    ),
                );
            }

            // ── ai_extract_uncertain_f64 ──
            {
                let rt_fn = self.ir.module.add_function(
                    "__axon_ai_extract_uncertain_f64",
                    make_rt_ty(f64_ptr),
                    None,
                );

                let fn_ty = result_unc_f64_ty.fn_type(&[str_ty.into()], false);
                let fn_val = self.ir.module.add_function("ai_extract_uncertain_f64", fn_ty, None);

                let entry_bb = self.ir.context.append_basic_block(fn_val, "aef_entry");
                let ok_bb    = self.ir.context.append_basic_block(fn_val, "aef_ok");
                let err_bb   = self.ir.context.append_basic_block(fn_val, "aef_err");
                let saved = self.ir.builder.get_insert_block();
                self.ir.builder.position_at_end(entry_bb);

                let prompt_str   = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let prompt_len   = self.ir.builder.build_extract_value(prompt_str, 0, "aef_plen").unwrap().into_int_value();
                let prompt_ptr_v = self.ir.builder.build_extract_value(prompt_str, 1, "aef_pptr").unwrap().into_pointer_value();

                let out_val_slot   = self.ir.builder.build_alloca(f64_ty, "aef_out_val").unwrap();
                let out_conf_slot  = self.ir.builder.build_alloca(f64_ty, "aef_out_conf").unwrap();
                let out_err_len_slot = self.ir.builder.build_alloca(i64_ty, "aef_out_err_len").unwrap();
                let out_err_ptr_slot = self.ir.builder.build_alloca(i8_ptr, "aef_out_err_ptr").unwrap();
                let out_err_ptr_cast = self.ir.builder
                    .build_pointer_cast(out_err_ptr_slot, i8_ptr_ptr, "aef_eptrptr")
                    .unwrap();

                let rc_call = self.ir.builder.build_call(
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_conf_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aef_rc",
                ).unwrap();
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = self.ir.builder
                    .build_int_compare(IntPredicate::EQ, rc, zero_i32, "aef_is_ok")
                    .unwrap();
                self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

                // ok_bb
                self.ir.builder.position_at_end(ok_bb);
                let val = self.ir.builder.build_load(f64_ty, out_val_slot, "aef_val").unwrap().into_float_value();
                let conf = self.ir.builder.build_load(f64_ty, out_conf_slot, "aef_conf").unwrap().into_float_value();
                let mut unc_sv = unc_f64_ty.get_undef();
                unc_sv = self.ir.builder.build_insert_value(unc_sv, val,  0, "aef_unc_v").unwrap().into_struct_value();
                unc_sv = self.ir.builder.build_insert_value(unc_sv, conf, 1, "aef_unc_c").unwrap().into_struct_value();
                unc_sv = self.ir.builder
                    .build_insert_value(unc_sv, i64_ty.const_int(1, false), 2, "aef_unc_s")
                    .unwrap()
                    .into_struct_value();
                let ok_alloca = self.ir.builder.build_alloca(result_unc_f64_ty, "aef_ok_slot").unwrap();
                let tag_ptr_ok = self.ir.builder.build_struct_gep(result_unc_f64_ty, ok_alloca, 0, "aef_tag_ok").unwrap();
                self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
                let payload_ok = self.ir.builder.build_struct_gep(result_unc_f64_ty, ok_alloca, 1, "aef_pay_ok").unwrap();
                let unc_payload_ptr = self.ir.builder
                    .build_pointer_cast(payload_ok, unc_f64_ty.ptr_type(inkwell::AddressSpace::default()), "aef_unc_pp")
                    .unwrap();
                self.ir.builder.build_store(unc_payload_ptr, unc_sv).unwrap();
                let ok_val = self.ir.builder.build_load(result_unc_f64_ty, ok_alloca, "aef_ok_val").unwrap();
                self.ir.builder.build_return(Some(&ok_val)).unwrap();

                // err_bb
                self.ir.builder.position_at_end(err_bb);
                let err_len = self.ir.builder.build_load(i64_ty, out_err_len_slot, "aef_elen").unwrap().into_int_value();
                let err_ptr = self.ir.builder.build_load(i8_ptr, out_err_ptr_slot, "aef_eptr").unwrap().into_pointer_value();
                let err_alloca = self.ir.builder.build_alloca(result_unc_f64_ty, "aef_err_slot").unwrap();
                let tag_ptr_err = self.ir.builder.build_struct_gep(result_unc_f64_ty, err_alloca, 0, "aef_tag_err").unwrap();
                self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
                let payload_err = self.ir.builder.build_struct_gep(result_unc_f64_ty, err_alloca, 1, "aef_pay_err").unwrap();
                let str_err_ptr = self.ir.builder
                    .build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aef_str_err_pp")
                    .unwrap();
                let str_err_slot = self.ir.builder.build_alloca(str_ty, "aef_str_err").unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), err_len).unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), err_ptr).unwrap();
                let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "aef_str_err_val").unwrap();
                self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
                let err_val = self.ir.builder.build_load(result_unc_f64_ty, err_alloca, "aef_err_val").unwrap();
                self.ir.builder.build_return(Some(&err_val)).unwrap();

                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert("ai_extract_uncertain_f64".to_string(), fn_val);
                self.fn_return_types.insert(
                    "ai_extract_uncertain_f64".to_string(),
                    Type::Result(
                        Box::new(Type::Uncertain(Box::new(Type::F64))),
                        Box::new(Type::Str),
                    ),
                );
            }
        }

        // ── ASI Layer-3 generic ai_extract::<T> — flat-T helpers ────────────────
        // The user-facing surface is `ai_extract::<T>(prompt: str) -> Result<T, str>`.
        // The parser lowers that to a `Call { callee: StructLit { name: "ai_extract::<T>" }, … }`
        // and `emit_call`'s StructLit dispatch routes to one of these per-T helpers.
        //
        // For T ∈ { Uncertain<i64>, Uncertain<f64> } we reuse the existing
        // `ai_extract_uncertain_i64`/`_f64` helpers above (no new bridges).
        //
        // For T ∈ { i64, f64, bool } we emit fresh helpers here that call the
        // new flat-T runtime bridges in axon-ai:
        //   i32 __axon_ai_extract_i64 (prompt_ptr, prompt_len, *out_value: *i64,
        //                              *out_err_len: *i64, *out_err_ptr: **u8) -> 0|1
        //   i32 __axon_ai_extract_f64 (… *out_value: *f64, …)                  -> 0|1
        //   i32 __axon_ai_extract_bool(… *out_value: *bool, …)                 -> 0|1
        //
        // Each Axon-level helper returns `Result<T, str>` with the canonical
        // 16-byte payload union (T fits — i64/f64 are 8 bytes, bool is 1, str
        // is 16).  source_tag is *not* present here because T is a flat scalar.
        {
            let f64_ty = self.ir.context.f64_type();
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let f64_ptr = f64_ty.ptr_type(inkwell::AddressSpace::default());
            let bool_ptr = bool_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_flat_ty = self.ir.context.struct_type(
                &[bool_ty.into(), i8_arr16_ty.into()],
                false,
            );

            // Common runtime extern signature factory (5 args, no confidence).
            let make_rt_ty = |val_ptr: inkwell::types::PointerType<'ctx>| {
                i32_ty.fn_type(
                    &[
                        i8_ptr.into(),       // prompt_ptr
                        i64_ty.into(),       // prompt_len
                        val_ptr.into(),      // out_value
                        i64_ptr.into(),      // out_err_len
                        i8_ptr_ptr.into(),   // out_err_ptr
                    ],
                    false,
                )
            };

            // Emit one flat-T extraction helper.
            //
            //   helper_name : Axon-level fn name registered in self.functions
            //   rt_name     : runtime symbol exported by axon-ai
            //   axon_t      : Axon Type for fn_return_types
            //   val_kind    : enum-ish marker for the value LLVM type
            #[derive(Clone, Copy)]
            enum FlatVal { I64, F64, Bool }
            let mut emit_flat_helper = |val_kind: FlatVal, helper_name: &str, rt_name: &str, axon_t: Type| {
                let (val_llvm_ty, val_ptr_ty): (BasicTypeEnum<'ctx>, inkwell::types::PointerType<'ctx>) =
                    match val_kind {
                        FlatVal::I64  => (i64_ty.into(),  i64_ptr),
                        FlatVal::F64  => (f64_ty.into(),  f64_ptr),
                        FlatVal::Bool => (bool_ty.into(), bool_ptr),
                    };
                let rt_fn = self.ir.module.add_function(rt_name, make_rt_ty(val_ptr_ty), None);

                let fn_ty = result_flat_ty.fn_type(&[str_ty.into()], false);
                let fn_val = self.ir.module.add_function(helper_name, fn_ty, None);

                let entry_bb = self.ir.context.append_basic_block(fn_val, "aex_entry");
                let ok_bb    = self.ir.context.append_basic_block(fn_val, "aex_ok");
                let err_bb   = self.ir.context.append_basic_block(fn_val, "aex_err");
                let saved = self.ir.builder.get_insert_block();
                self.ir.builder.position_at_end(entry_bb);

                let prompt_str   = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let prompt_len   = self.ir.builder.build_extract_value(prompt_str, 0, "aex_plen").unwrap().into_int_value();
                let prompt_ptr_v = self.ir.builder.build_extract_value(prompt_str, 1, "aex_pptr").unwrap().into_pointer_value();

                let out_val_slot     = self.ir.builder.build_alloca(val_llvm_ty, "aex_out_val").unwrap();
                let out_err_len_slot = self.ir.builder.build_alloca(i64_ty,     "aex_out_err_len").unwrap();
                let out_err_ptr_slot = self.ir.builder.build_alloca(i8_ptr,     "aex_out_err_ptr").unwrap();
                let out_err_ptr_cast = self.ir.builder
                    .build_pointer_cast(out_err_ptr_slot, i8_ptr_ptr, "aex_eptrptr")
                    .unwrap();

                let rc_call = self.ir.builder.build_call(
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aex_rc",
                ).unwrap();
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = self.ir.builder
                    .build_int_compare(IntPredicate::EQ, rc, zero_i32, "aex_is_ok")
                    .unwrap();
                self.ir.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

                // ok_bb: load typed value, store into payload via a typed pointer
                // cast, set tag=1, return.
                self.ir.builder.position_at_end(ok_bb);
                let ok_alloca = self.ir.builder.build_alloca(result_flat_ty, "aex_ok_slot").unwrap();
                let tag_ptr_ok = self.ir.builder.build_struct_gep(result_flat_ty, ok_alloca, 0, "aex_tag_ok").unwrap();
                self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
                let payload_ok = self.ir.builder.build_struct_gep(result_flat_ty, ok_alloca, 1, "aex_pay_ok").unwrap();
                let typed_payload_ptr = self.ir.builder
                    .build_pointer_cast(payload_ok, val_ptr_ty, "aex_typed_pp")
                    .unwrap();
                let val_loaded = self.ir.builder.build_load(val_llvm_ty, out_val_slot, "aex_val").unwrap();
                self.ir.builder.build_store(typed_payload_ptr, val_loaded).unwrap();
                let ok_val = self.ir.builder.build_load(result_flat_ty, ok_alloca, "aex_ok_val").unwrap();
                self.ir.builder.build_return(Some(&ok_val)).unwrap();

                // err_bb: read err_len/err_ptr, build str payload, set tag=0, return.
                self.ir.builder.position_at_end(err_bb);
                let err_len = self.ir.builder.build_load(i64_ty, out_err_len_slot, "aex_elen").unwrap().into_int_value();
                let err_ptr = self.ir.builder.build_load(i8_ptr, out_err_ptr_slot, "aex_eptr").unwrap().into_pointer_value();
                let err_alloca = self.ir.builder.build_alloca(result_flat_ty, "aex_err_slot").unwrap();
                let tag_ptr_err = self.ir.builder.build_struct_gep(result_flat_ty, err_alloca, 0, "aex_tag_err").unwrap();
                self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
                let payload_err = self.ir.builder.build_struct_gep(result_flat_ty, err_alloca, 1, "aex_pay_err").unwrap();
                let str_err_ptr = self.ir.builder
                    .build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aex_str_err_pp")
                    .unwrap();
                let str_err_slot = self.ir.builder.build_alloca(str_ty, "aex_str_err").unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), err_len).unwrap();
                self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), err_ptr).unwrap();
                let str_err_val = self.ir.builder.build_load(str_ty, str_err_slot, "aex_str_err_val").unwrap();
                self.ir.builder.build_store(str_err_ptr, str_err_val).unwrap();
                let err_val = self.ir.builder.build_load(result_flat_ty, err_alloca, "aex_err_val").unwrap();
                self.ir.builder.build_return(Some(&err_val)).unwrap();

                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert(helper_name.to_string(), fn_val);
                self.fn_return_types.insert(
                    helper_name.to_string(),
                    Type::Result(Box::new(axon_t), Box::new(Type::Str)),
                );
            };

            emit_flat_helper(FlatVal::I64,  "ai_extract_i64",  "__axon_ai_extract_i64",  Type::I64);
            emit_flat_helper(FlatVal::F64,  "ai_extract_f64",  "__axon_ai_extract_f64",  Type::F64);
            emit_flat_helper(FlatVal::Bool, "ai_extract_bool", "__axon_ai_extract_bool", Type::Bool);
        }

    }

    /// Auto-extracted from `declare_builtins` (Phase 3.2 decomposition).
    /// Declares Phase 9 numeric/math: i64<->f64 casts, abs_*, sign_i64, pow_i64, sqrt/floor/ceil/round_f64.
    pub(super) fn declare_phase9_math_builtins(&mut self) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.ir.context.i32_type();
        let i64_ty = self.ir.context.i64_type();
        let bool_ty = self.ir.context.bool_type();
        let void_ty = self.ir.context.void_type();
        let _ = (i32_ty, bool_ty, void_ty, i8_ptr, i64_ty);

        // ── Phase 9: i64_to_f64(n: i64) -> f64 ──────────────────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("i64_to_f64", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let r = self.ir.builder.build_signed_int_to_float(n, f64_ty, "itf").unwrap();
            self.ir.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("i64_to_f64".to_string(), fn_val);
            self.fn_return_types.insert("i64_to_f64".to_string(), Type::F64);
        }

        // ── Phase 9: f64_to_i64(x: f64) -> i64 ──────────────────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = i64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("f64_to_i64", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let x = fn_val.get_nth_param(0).unwrap().into_float_value();
            let r = self.ir.builder.build_float_to_signed_int(x, i64_ty, "fti").unwrap();
            self.ir.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("f64_to_i64".to_string(), fn_val);
            self.fn_return_types.insert("f64_to_i64".to_string(), Type::I64);
        }

        // ── Phase 9: abs_i64(n: i64) -> i64 ─────────────────────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("abs_i64", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let neg = self.ir.builder.build_int_neg(n, "abs_neg").unwrap();
            let is_neg = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::SLT, n, i64_ty.const_zero(), "abs_cmp").unwrap();
            let r = self.ir.builder.build_select(is_neg, neg, n, "abs_r").unwrap().into_int_value();
            self.ir.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("abs_i64".to_string(), fn_val);
            self.fn_return_types.insert("abs_i64".to_string(), Type::I64);
        }

        // ── Phase 9: abs_f64(x: f64) -> f64 ─────────────────────────────────
        {
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("abs_f64", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let x = fn_val.get_nth_param(0).unwrap().into_float_value();
            let zero = f64_ty.const_float(0.0);
            let neg = self.ir.builder.build_float_neg(x, "abf_neg").unwrap();
            let is_neg = self.ir.builder.build_float_compare(
                inkwell::FloatPredicate::OLT, x, zero, "abf_cmp").unwrap();
            let r = self.ir.builder.build_select(is_neg, neg, x, "abf_r").unwrap().into_float_value();
            self.ir.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("abs_f64".to_string(), fn_val);
            self.fn_return_types.insert("abs_f64".to_string(), Type::F64);
        }

        // ── Phase 9: sign_i64(n: i64) -> i64  (-1 | 0 | 1) ─────────────────
        {
            let i64_ty = self.ir.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("sign_i64", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i64_ty.const_zero();
            let is_pos = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::SGT, n, zero, "sg_pos").unwrap();
            let is_neg = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::SLT, n, zero, "sg_neg").unwrap();
            let one = i64_ty.const_int(1, false);
            let neg_one = i64_ty.const_int(u64::MAX, true);
            // if positive → 1, else if negative → -1, else → 0
            let step1 = self.ir.builder.build_select(is_neg, neg_one, zero, "sg_s1").unwrap().into_int_value();
            let r     = self.ir.builder.build_select(is_pos, one, step1, "sg_r").unwrap().into_int_value();
            self.ir.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("sign_i64".to_string(), fn_val);
            self.fn_return_types.insert("sign_i64".to_string(), Type::I64);
        }

        // ── Phase 9: pow_i64(base: i64, exp: i64) -> i64 ────────────────────
        // Iterative: result=1; while exp>0 { result*=base; exp-=1 }
        {
            let i64_ty = self.ir.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("pow_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "pi_entry");
            let cond_bb  = self.ir.context.append_basic_block(fn_val, "pi_cond");
            let body_bb  = self.ir.context.append_basic_block(fn_val, "pi_body");
            let exit_bb  = self.ir.context.append_basic_block(fn_val, "pi_exit");
            let saved = self.ir.builder.get_insert_block();

            self.ir.builder.position_at_end(entry_bb);
            let base_slot   = self.ir.builder.build_alloca(i64_ty, "pi_base").unwrap();
            let exp_slot    = self.ir.builder.build_alloca(i64_ty, "pi_exp").unwrap();
            let result_slot = self.ir.builder.build_alloca(i64_ty, "pi_result").unwrap();
            let base = fn_val.get_nth_param(0).unwrap().into_int_value();
            let exp  = fn_val.get_nth_param(1).unwrap().into_int_value();
            self.ir.builder.build_store(base_slot, base).unwrap();
            self.ir.builder.build_store(exp_slot, exp).unwrap();
            self.ir.builder.build_store(result_slot, i64_ty.const_int(1, false)).unwrap();
            self.ir.builder.build_unconditional_branch(cond_bb).unwrap();

            self.ir.builder.position_at_end(cond_bb);
            let e = self.ir.builder.build_load(i64_ty, exp_slot, "pi_e").unwrap().into_int_value();
            let cmp = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::SGT, e, i64_ty.const_zero(), "pi_cmp").unwrap();
            self.ir.builder.build_conditional_branch(cmp, body_bb, exit_bb).unwrap();

            self.ir.builder.position_at_end(body_bb);
            let r = self.ir.builder.build_load(i64_ty, result_slot, "pi_r").unwrap().into_int_value();
            let b = self.ir.builder.build_load(i64_ty, base_slot, "pi_b").unwrap().into_int_value();
            let r2 = self.ir.builder.build_int_mul(r, b, "pi_r2").unwrap();
            self.ir.builder.build_store(result_slot, r2).unwrap();
            let e2 = self.ir.builder.build_load(i64_ty, exp_slot, "pi_e2").unwrap().into_int_value();
            let e3 = self.ir.builder.build_int_sub(e2, i64_ty.const_int(1, false), "pi_e3").unwrap();
            self.ir.builder.build_store(exp_slot, e3).unwrap();
            self.ir.builder.build_unconditional_branch(cond_bb).unwrap();

            self.ir.builder.position_at_end(exit_bb);
            let res = self.ir.builder.build_load(i64_ty, result_slot, "pi_res").unwrap();
            self.ir.builder.build_return(Some(&res)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("pow_i64".to_string(), fn_val);
            self.fn_return_types.insert("pow_i64".to_string(), Type::I64);
        }

        // ── Phase 9: sqrt_f64 / floor_f64 / ceil_f64 / round_f64 ────────────
        // Use LLVM intrinsics via C libm linkage.
        {
            let f64_ty = self.ir.context.f64_type();
            let fn1_ty = f64_ty.fn_type(&[f64_ty.into()], false);

            for (axon_name, c_name) in &[
                ("sqrt_f64",  "sqrt"),
                ("floor_f64", "floor"),
                ("ceil_f64",  "ceil"),
                ("round_f64", "round"),
            ] {
                // Declare the C libm function (or reuse if already declared).
                let libm_fn = self.ir.module.get_function(c_name)
                    .unwrap_or_else(|| self.ir.module.add_function(c_name, fn1_ty, None));

                let fn_val = self.ir.module.add_function(axon_name, fn1_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                let saved = self.ir.builder.get_insert_block();
                self.ir.builder.position_at_end(bb);
                let x = fn_val.get_nth_param(0).unwrap().into_float_value();
                let r = self.ir.builder.build_call(libm_fn, &[x.into()], "r").unwrap()
                    .try_as_basic_value().left().unwrap().into_float_value();
                self.ir.builder.build_return(Some(&r)).unwrap();
                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert(axon_name.to_string(), fn_val);
                self.fn_return_types.insert(axon_name.to_string(), Type::F64);
            }
        }


    }

    /// Auto-extracted from `declare_builtins` (Phase 3.2 decomposition).
    /// Declares Phase 5/6/7 string + adjacent builtins: str_*, parse_float, abs/min/max_i64, env_var, exit.
    pub(super) fn declare_string_builtins(&mut self) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.ir.context.i32_type();
        let i64_ty = self.ir.context.i64_type();
        let bool_ty = self.ir.context.bool_type();
        let void_ty = self.ir.context.void_type();
        let _ = (i32_ty, bool_ty, void_ty, i8_ptr, i64_ty);

        // ── Phase 5: String builtins ──────────────────────────────────────────

        // str_eq(a: str, b: str) -> bool
        // Compare two strings for byte-equal content. Uses memcmp after length check.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_eq", fn_ty, None);

            let entry_bb  = self.ir.context.append_basic_block(fn_val, "se_entry");
            let cmp_bb    = self.ir.context.append_basic_block(fn_val, "se_cmp");
            let true_bb   = self.ir.context.append_basic_block(fn_val, "se_true");
            let false_bb  = self.ir.context.append_basic_block(fn_val, "se_false");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let a = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let a_len = self.ir.builder.build_extract_value(a, 0, "se_alen").unwrap().into_int_value();
            let b_len = self.ir.builder.build_extract_value(b, 0, "se_blen").unwrap().into_int_value();
            let a_ptr = self.ir.builder.build_extract_value(a, 1, "se_aptr").unwrap().into_pointer_value();
            let b_ptr = self.ir.builder.build_extract_value(b, 1, "se_bptr").unwrap().into_pointer_value();

            // If lengths differ → false immediately.
            let lens_eq = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, a_len, b_len, "se_leneq").unwrap();
            self.ir.builder.build_conditional_branch(lens_eq, cmp_bb, false_bb).unwrap();

            // Same length → call memcmp.
            self.ir.builder.position_at_end(cmp_bb);
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = self.ir.builder.build_call(memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "se_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp_result, i32_ty.const_int(0, false), "se_iszero").unwrap();
            self.ir.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.ir.builder.position_at_end(true_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();

            self.ir.builder.position_at_end(false_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_eq".to_string(), fn_val);
            self.fn_return_types.insert("str_eq".to_string(), Type::Bool);
        }

        // str_contains(s: str, needle: str) -> bool
        // Uses memmem-like loop: slide needle over s, compare with memcmp.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_contains", fn_ty, None);

            // We implement via strstr(3) since strings are null-terminated.
            // strstr returns a non-null pointer if needle is found.
            let strstr_fn = self.ir.module.get_function("strstr").unwrap_or_else(|| {
                let strstr_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.ir.module.add_function("strstr", strstr_ty, None)
            });

            let entry_bb = self.ir.context.append_basic_block(fn_val, "sc_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "sc_sptr").unwrap().into_pointer_value();
            let n_ptr = self.ir.builder.build_extract_value(needle, 1, "sc_nptr").unwrap().into_pointer_value();

            let found = self.ir.builder.build_call(strstr_fn, &[s_ptr.into(), n_ptr.into()], "sc_found").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let null = i8_ptr.const_null();
            let found_int = self.ir.builder.build_ptr_to_int(found, i64_ty, "sc_found_int").unwrap();
            let null_int  = self.ir.builder.build_ptr_to_int(null,  i64_ty, "sc_null_int").unwrap();
            let is_found  = self.ir.builder.build_int_compare(inkwell::IntPredicate::NE, found_int, null_int, "sc_is_found").unwrap();
            let result = self.ir.builder.build_int_z_extend(is_found, bool_ty, "sc_result").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_contains".to_string(), fn_val);
            self.fn_return_types.insert("str_contains".to_string(), Type::Bool);
        }

        // str_starts_with(s: str, prefix: str) -> bool
        // len(s) >= len(prefix) && memcmp(s.ptr, prefix.ptr, len(prefix)) == 0
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_starts_with", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "ssw_entry");
            let cmp_bb   = self.ir.context.append_basic_block(fn_val, "ssw_cmp");
            let true_bb  = self.ir.context.append_basic_block(fn_val, "ssw_true");
            let false_bb = self.ir.context.append_basic_block(fn_val, "ssw_false");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let p = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "ssw_slen").unwrap().into_int_value();
            let p_len = self.ir.builder.build_extract_value(p, 0, "ssw_plen").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "ssw_sptr").unwrap().into_pointer_value();
            let p_ptr = self.ir.builder.build_extract_value(p, 1, "ssw_pptr").unwrap().into_pointer_value();

            // s_len >= p_len?
            let long_enough = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, s_len, p_len, "ssw_longenough").unwrap();
            self.ir.builder.build_conditional_branch(long_enough, cmp_bb, false_bb).unwrap();

            self.ir.builder.position_at_end(cmp_bb);
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp = self.ir.builder.build_call(memcmp_fn, &[s_ptr.into(), p_ptr.into(), p_len.into()], "ssw_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp, i32_ty.const_int(0, false), "ssw_iszero").unwrap();
            self.ir.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.ir.builder.position_at_end(true_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();
            self.ir.builder.position_at_end(false_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_starts_with".to_string(), fn_val);
            self.fn_return_types.insert("str_starts_with".to_string(), Type::Bool);
        }

        // str_ends_with(s: str, suffix: str) -> bool
        // len(s) >= len(suffix) && memcmp(s.ptr + len(s) - len(suffix), suffix.ptr, len(suffix)) == 0
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_ends_with", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "sew_entry");
            let cmp_bb   = self.ir.context.append_basic_block(fn_val, "sew_cmp");
            let true_bb  = self.ir.context.append_basic_block(fn_val, "sew_true");
            let false_bb = self.ir.context.append_basic_block(fn_val, "sew_false");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let sf = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_len  = self.ir.builder.build_extract_value(s, 0, "sew_slen").unwrap().into_int_value();
            let sf_len = self.ir.builder.build_extract_value(sf, 0, "sew_sflen").unwrap().into_int_value();
            let s_ptr  = self.ir.builder.build_extract_value(s, 1, "sew_sptr").unwrap().into_pointer_value();
            let sf_ptr = self.ir.builder.build_extract_value(sf, 1, "sew_sfptr").unwrap().into_pointer_value();

            let long_enough = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, s_len, sf_len, "sew_longenough").unwrap();
            self.ir.builder.build_conditional_branch(long_enough, cmp_bb, false_bb).unwrap();

            self.ir.builder.position_at_end(cmp_bb);
            // offset = s_len - sf_len; start = s.ptr + offset
            let offset = self.ir.builder.build_int_sub(s_len, sf_len, "sew_offset").unwrap();
            let start = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), s_ptr, &[offset], "sew_start").unwrap()
            };
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp = self.ir.builder.build_call(memcmp_fn, &[start.into(), sf_ptr.into(), sf_len.into()], "sew_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp, i32_ty.const_int(0, false), "sew_iszero").unwrap();
            self.ir.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.ir.builder.position_at_end(true_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();
            self.ir.builder.position_at_end(false_bb);
            self.ir.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_ends_with".to_string(), fn_val);
            self.fn_return_types.insert("str_ends_with".to_string(), Type::Bool);
        }

        // str_slice(s: str, start: i64, end: i64) -> str
        // Returns heap-allocated substring. Clamps start/end to [0, len].
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_slice", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "ss_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let start = fn_val.get_nth_param(1).unwrap().into_int_value();
            let end   = fn_val.get_nth_param(2).unwrap().into_int_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "ss_slen").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "ss_sptr").unwrap().into_pointer_value();

            // Clamp start to [0, s_len]
            let zero = i64_ty.const_int(0, false);
            let start_pos = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, start, zero, "ss_s_neg").unwrap();
            let start_clamped_lo = self.ir.builder.build_select(start_pos, zero, start, "ss_start_lo").unwrap().into_int_value();
            let start_gt = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGT, start_clamped_lo, s_len, "ss_s_gt").unwrap();
            let start_clamped = self.ir.builder.build_select(start_gt, s_len, start_clamped_lo, "ss_start").unwrap().into_int_value();

            // Clamp end to [start_clamped, s_len]
            let end_lt = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, end, start_clamped, "ss_e_lt").unwrap();
            let end_clamped_lo = self.ir.builder.build_select(end_lt, start_clamped, end, "ss_end_lo").unwrap().into_int_value();
            let end_gt = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGT, end_clamped_lo, s_len, "ss_e_gt").unwrap();
            let end_clamped = self.ir.builder.build_select(end_gt, s_len, end_clamped_lo, "ss_end").unwrap().into_int_value();

            // slice_len = end_clamped - start_clamped
            let slice_len = self.ir.builder.build_int_sub(end_clamped, start_clamped, "ss_slicelen").unwrap();

            // Allocate slice_len + 1 bytes via malloc.
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", malloc_ty, None)
            });
            let alloc_size = self.ir.builder.build_int_add(slice_len, i64_ty.const_int(1, false), "ss_alloc").unwrap();
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_size.into()], "ss_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            // src = s_ptr + start_clamped
            let src_ptr = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), s_ptr, &[start_clamped], "ss_src").unwrap()
            };

            // memcpy(buf, src, slice_len)
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", memcpy_ty, None)
            });
            self.ir.builder.build_call(memcpy_fn, &[buf.into(), src_ptr.into(), slice_len.into()], "").unwrap();

            // Null-terminate: buf[slice_len] = 0
            let null_byte = self.ir.context.i8_type().const_int(0, false);
            let null_pos = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[slice_len], "ss_null_pos").unwrap()
            };
            self.ir.builder.build_store(null_pos, null_byte).unwrap();

            // Build result str struct { slice_len, buf }
            let result_alloca = self.ir.builder.build_alloca(str_ty, "ss_result").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, result_alloca, 0, "").unwrap(), slice_len).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, result_alloca, 1, "").unwrap(), buf).unwrap();
            let result = self.ir.builder.build_load(str_ty, result_alloca, "ss_result_val").unwrap();
            self.ir.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_slice".to_string(), fn_val);
            self.fn_return_types.insert("str_slice".to_string(), Type::Str);
        }

        // str_index_of(s: str, needle: str) -> i64
        // Returns byte index of first occurrence of needle in s, or -1 if not found.
        // Uses strstr and pointer arithmetic.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_index_of", fn_ty, None);

            let entry_bb    = self.ir.context.append_basic_block(fn_val, "sio_entry");
            let found_bb    = self.ir.context.append_basic_block(fn_val, "sio_found");
            let notfound_bb = self.ir.context.append_basic_block(fn_val, "sio_notfound");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr  = self.ir.builder.build_extract_value(s, 1, "sio_sptr").unwrap().into_pointer_value();
            let n_ptr  = self.ir.builder.build_extract_value(needle, 1, "sio_nptr").unwrap().into_pointer_value();

            let strstr_fn = self.ir.module.get_function("strstr").unwrap_or_else(|| {
                let strstr_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.ir.module.add_function("strstr", strstr_ty, None)
            });
            let found = self.ir.builder.build_call(strstr_fn, &[s_ptr.into(), n_ptr.into()], "sio_found_ptr").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let null = i8_ptr.const_null();
            let found_int = self.ir.builder.build_ptr_to_int(found, i64_ty, "sio_fi").unwrap();
            let null_int  = self.ir.builder.build_ptr_to_int(null, i64_ty, "sio_ni").unwrap();
            let s_int     = self.ir.builder.build_ptr_to_int(s_ptr, i64_ty, "sio_si").unwrap();
            let is_found  = self.ir.builder.build_int_compare(inkwell::IntPredicate::NE, found_int, null_int, "sio_is_found").unwrap();
            self.ir.builder.build_conditional_branch(is_found, found_bb, notfound_bb).unwrap();

            self.ir.builder.position_at_end(found_bb);
            let offset = self.ir.builder.build_int_sub(found_int, s_int, "sio_offset").unwrap();
            self.ir.builder.build_return(Some(&offset)).unwrap();

            self.ir.builder.position_at_end(notfound_bb);
            self.ir.builder.build_return(Some(&i64_ty.const_int(u64::MAX, true))).unwrap(); // -1 as i64

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_index_of".to_string(), fn_val);
            self.fn_return_types.insert("str_index_of".to_string(), Type::I64);
        }

        // char_at(s: str, i: i64) -> i64
        // Returns byte value at index i, or -1 if out of bounds.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("char_at", fn_ty, None);

            let entry_bb   = self.ir.context.append_basic_block(fn_val, "ca_entry");
            let inbounds_bb = self.ir.context.append_basic_block(fn_val, "ca_inbounds");
            let oob_bb     = self.ir.context.append_basic_block(fn_val, "ca_oob");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let idx   = fn_val.get_nth_param(1).unwrap().into_int_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "ca_len").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "ca_ptr").unwrap().into_pointer_value();

            // Check 0 <= idx < s_len
            let zero = i64_ty.const_int(0, false);
            let ge_zero = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, idx, zero, "ca_gez").unwrap();
            let lt_len  = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, idx, s_len, "ca_ltl").unwrap();
            let in_bounds = self.ir.builder.build_and(ge_zero, lt_len, "ca_inb").unwrap();
            self.ir.builder.build_conditional_branch(in_bounds, inbounds_bb, oob_bb).unwrap();

            self.ir.builder.position_at_end(inbounds_bb);
            let byte_ptr = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), s_ptr, &[idx], "ca_byteptr").unwrap()
            };
            let byte_val = self.ir.builder.build_load(self.ir.context.i8_type(), byte_ptr, "ca_byte").unwrap().into_int_value();
            // zero-extend i8 to i64
            let byte_i64 = self.ir.builder.build_int_z_extend(byte_val, i64_ty, "ca_byte_i64").unwrap();
            self.ir.builder.build_return(Some(&byte_i64)).unwrap();

            self.ir.builder.position_at_end(oob_bb);
            self.ir.builder.build_return(Some(&i64_ty.const_int(u64::MAX, true))).unwrap(); // -1

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("char_at".to_string(), fn_val);
            self.fn_return_types.insert("char_at".to_string(), Type::I64);
        }

        // to_str_bool(b: bool) -> str
        // Returns str "true" or "false" (global string constants).
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.ir.module.add_function("to_str_bool", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "tsb_entry");
            let true_bb  = self.ir.context.append_basic_block(fn_val, "tsb_true");
            let false_bb = self.ir.context.append_basic_block(fn_val, "tsb_false");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let b = fn_val.get_nth_param(0).unwrap().into_int_value();
            let is_true = self.ir.builder.build_int_compare(inkwell::IntPredicate::NE, b, bool_ty.const_int(0, false), "tsb_cond").unwrap();
            self.ir.builder.build_conditional_branch(is_true, true_bb, false_bb).unwrap();

            // Declare "true\0" and "false\0" as global string constants.
            let true_bytes: Vec<_> = b"true\0".iter().map(|&c| self.ir.context.i8_type().const_int(c as u64, false)).collect();
            let false_bytes: Vec<_> = b"false\0".iter().map(|&c| self.ir.context.i8_type().const_int(c as u64, false)).collect();
            let true_g = self.ir.module.add_global(self.ir.context.i8_type().array_type(5), None, "tsb_true_str");
            true_g.set_initializer(&self.ir.context.i8_type().const_array(&true_bytes));
            true_g.set_constant(true);
            let false_g = self.ir.module.add_global(self.ir.context.i8_type().array_type(6), None, "tsb_false_str");
            false_g.set_initializer(&self.ir.context.i8_type().const_array(&false_bytes));
            false_g.set_constant(true);

            self.ir.builder.position_at_end(true_bb);
            let true_ptr = self.ir.builder.build_pointer_cast(true_g.as_pointer_value(), i8_ptr, "tsb_tptr").unwrap();
            let mut true_str = str_ty.get_undef();
            true_str = self.ir.builder.build_insert_value(true_str, i64_ty.const_int(4, false), 0, "tsb_t0").unwrap().into_struct_value();
            true_str = self.ir.builder.build_insert_value(true_str, true_ptr, 1, "tsb_t1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&true_str)).unwrap();

            self.ir.builder.position_at_end(false_bb);
            let false_ptr = self.ir.builder.build_pointer_cast(false_g.as_pointer_value(), i8_ptr, "tsb_fptr").unwrap();
            let mut false_str = str_ty.get_undef();
            false_str = self.ir.builder.build_insert_value(false_str, i64_ty.const_int(5, false), 0, "tsb_f0").unwrap().into_struct_value();
            false_str = self.ir.builder.build_insert_value(false_str, false_ptr, 1, "tsb_f1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&false_str)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("to_str_bool".to_string(), fn_val);
            self.fn_return_types.insert("to_str_bool".to_string(), Type::Str);
        }

        // ── Phase 5: parse_float(s: str) -> Result<f64, str> ─────────────────
        // Uses strtod; endptr check detects parse failure.
        // Result<f64, str> = { i1, [16 x i8] } (f64=8 bytes, str=16 bytes → max=16)
        {
            let f64_ty = self.ir.context.f64_type();
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let strtod_ty = f64_ty.fn_type(&[i8_ptr.into(), i8_ptr_ptr.into()], false);
            let strtod_fn = self.ir.module.get_function("strtod").unwrap_or_else(|| {
                self.ir.module.add_function("strtod", strtod_ty, None)
            });

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_float", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "pf_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "pf_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "pf_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.ir.builder.build_extract_value(s, 1, "pf_data").unwrap().into_pointer_value();

            let endptr_slot = self.ir.builder.build_alloca(i8_ptr, "pf_endptr").unwrap();
            self.ir.builder.build_store(endptr_slot, i8_ptr.const_null()).unwrap();
            let endptr_cast = self.ir.builder.build_pointer_cast(endptr_slot, i8_ptr_ptr, "pf_endptr_cast").unwrap();

            let parsed_f64 = self.ir.builder.build_call(strtod_fn, &[data_ptr.into(), endptr_cast.into()], "pf_strtod").unwrap()
                .try_as_basic_value().left().unwrap().into_float_value();

            let endptr_val = self.ir.builder.build_load(i8_ptr, endptr_slot, "pf_endptr_val").unwrap().into_pointer_value();
            let endptr_int = self.ir.builder.build_ptr_to_int(endptr_val, i64_ty, "pf_ep_int").unwrap();
            let data_int   = self.ir.builder.build_ptr_to_int(data_ptr, i64_ty, "pf_data_int").unwrap();
            let consumed   = self.ir.builder.build_int_compare(inkwell::IntPredicate::NE, endptr_int, data_int, "pf_consumed").unwrap();
            self.ir.builder.build_conditional_branch(consumed, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=f64 as [16 x i8] }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = self.ir.builder.build_alloca(result_ty, "pf_ok_slot").unwrap();
            let tag_ptr_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 0, "pf_tag_ok").unwrap();
            self.ir.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.ir.builder.build_struct_gep(result_ty, ok_alloca, 1, "pf_pay_ok").unwrap();
            let f64_ptr = self.ir.builder.build_pointer_cast(payload_ok, f64_ty.ptr_type(inkwell::AddressSpace::default()), "pf_f64_ptr").unwrap();
            self.ir.builder.build_store(f64_ptr, parsed_f64).unwrap();
            let ok_val = self.ir.builder.build_load(result_ty, ok_alloca, "pf_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: { tag=0, payload=str{len=0, ptr=null} }
            self.ir.builder.position_at_end(err_bb);
            let err_alloca = self.ir.builder.build_alloca(result_ty, "pf_err_slot").unwrap();
            let tag_ptr_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 0, "pf_tag_err").unwrap();
            self.ir.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.ir.builder.build_struct_gep(result_ty, err_alloca, 1, "pf_pay_err").unwrap();
            let err_str_ptr = self.ir.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "pf_str_err_ptr").unwrap();
            let err_str_slot = self.ir.builder.build_alloca(str_ty, "pf_str_err").unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, err_str_slot, 0, "").unwrap(), i64_ty.const_int(0, false)).unwrap();
            self.ir.builder.build_store(self.ir.builder.build_struct_gep(str_ty, err_str_slot, 1, "").unwrap(), i8_ptr.const_null()).unwrap();
            let err_str_val = self.ir.builder.build_load(str_ty, err_str_slot, "pf_err_str_val").unwrap();
            self.ir.builder.build_store(err_str_ptr, err_str_val).unwrap();
            let err_val = self.ir.builder.build_load(result_ty, err_alloca, "pf_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_float".to_string(), fn_val);
            self.fn_return_types.insert("parse_float".to_string(),
                Type::Result(Box::new(Type::F64), Box::new(Type::Str)));
        }

        // ── Phase 5: abs_i64, min_i64, max_i64 ───────────────────────────────
        {
            // abs_i64(n: i64) -> i64: if n < 0 then -n else n
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("abs_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ai_entry");
            let neg_bb   = self.ir.context.append_basic_block(fn_val, "ai_neg");
            let pos_bb   = self.ir.context.append_basic_block(fn_val, "ai_pos");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_neg = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, n, zero, "ai_isneg").unwrap();
            self.ir.builder.build_conditional_branch(is_neg, neg_bb, pos_bb).unwrap();
            self.ir.builder.position_at_end(neg_bb);
            let negn = self.ir.builder.build_int_neg(n, "ai_neg").unwrap();
            self.ir.builder.build_return(Some(&negn)).unwrap();
            self.ir.builder.position_at_end(pos_bb);
            self.ir.builder.build_return(Some(&n)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("abs_i64".to_string(), fn_val);
            self.fn_return_types.insert("abs_i64".to_string(), Type::I64);
        }

        {
            // min_i64(a: i64, b: i64) -> i64
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("min_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "mn_entry");
            let a_bb = self.ir.context.append_basic_block(fn_val, "mn_a");
            let b_bb = self.ir.context.append_basic_block(fn_val, "mn_b");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_le_b = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLE, a, b, "mn_ale").unwrap();
            self.ir.builder.build_conditional_branch(a_le_b, a_bb, b_bb).unwrap();
            self.ir.builder.position_at_end(a_bb);
            self.ir.builder.build_return(Some(&a)).unwrap();
            self.ir.builder.position_at_end(b_bb);
            self.ir.builder.build_return(Some(&b)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("min_i64".to_string(), fn_val);
            self.fn_return_types.insert("min_i64".to_string(), Type::I64);
        }

        {
            // max_i64(a: i64, b: i64) -> i64
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("max_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "mx_entry");
            let a_bb = self.ir.context.append_basic_block(fn_val, "mx_a");
            let b_bb = self.ir.context.append_basic_block(fn_val, "mx_b");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_ge_b = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGE, a, b, "mx_age").unwrap();
            self.ir.builder.build_conditional_branch(a_ge_b, a_bb, b_bb).unwrap();
            self.ir.builder.position_at_end(a_bb);
            self.ir.builder.build_return(Some(&a)).unwrap();
            self.ir.builder.position_at_end(b_bb);
            self.ir.builder.build_return(Some(&b)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("max_i64".to_string(), fn_val);
            self.fn_return_types.insert("max_i64".to_string(), Type::I64);
        }

        // ── Phase 6: str_to_upper / str_to_lower ─────────────────────────────
        // Both functions: malloc len+1 bytes, copy with ASCII conversion, null-terminate.
        for (fname, is_upper) in &[("str_to_upper", true), ("str_to_lower", false)] {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);
            // Create all blocks upfront so we can pass them as branch targets.
            let entry_bb = self.ir.context.append_basic_block(fn_val, "stl_entry");
            let loop_bb  = self.ir.context.append_basic_block(fn_val, "stl_loop");
            let body_bb  = self.ir.context.append_basic_block(fn_val, "stl_body");
            let done_bb  = self.ir.context.append_basic_block(fn_val, "stl_done");
            let saved = self.ir.builder.get_insert_block();

            // ── entry: malloc, init i=0, jump to loop ──────────────────────
            self.ir.builder.position_at_end(entry_bb);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "stl_len").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "stl_ptr").unwrap().into_pointer_value();
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            let alloc_size = self.ir.builder.build_int_add(s_len, i64_ty.const_int(1, false), "stl_sz").unwrap();
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_size.into()], "stl_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let i_slot = self.ir.builder.build_alloca(i64_ty, "stl_i").unwrap();
            self.ir.builder.build_store(i_slot, i64_ty.const_zero()).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── loop: if i < s_len goto body else done ─────────────────────
            self.ir.builder.position_at_end(loop_bb);
            let i_val = self.ir.builder.build_load(i64_ty, i_slot, "stl_iv").unwrap().into_int_value();
            let in_range = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, s_len, "stl_cmp").unwrap();
            self.ir.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // ── body: convert byte, store, increment i ─────────────────────
            self.ir.builder.position_at_end(body_bb);
            let src_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), s_ptr, &[i_val], "stl_src").unwrap() };
            let byte = self.ir.builder.build_load(self.ir.context.i8_type(), src_gep, "stl_byte").unwrap().into_int_value();
            let converted = if *is_upper {
                // toupper: if byte in 'a'..'z' => byte - 32
                let lo = self.ir.context.i8_type().const_int(b'a' as u64, false);
                let hi = self.ir.context.i8_type().const_int(b'z' as u64, false);
                let is_lo = self.ir.builder.build_int_compare(inkwell::IntPredicate::UGE, byte, lo, "stl_uge").unwrap();
                let is_hi = self.ir.builder.build_int_compare(inkwell::IntPredicate::ULE, byte, hi, "stl_ule").unwrap();
                let in_range_c = self.ir.builder.build_and(is_lo, is_hi, "stl_islc").unwrap();
                let sub32 = self.ir.builder.build_int_sub(byte, self.ir.context.i8_type().const_int(32, false), "stl_sub").unwrap();
                self.ir.builder.build_select(in_range_c, sub32, byte, "stl_sel").unwrap().into_int_value()
            } else {
                // tolower: if byte in 'A'..'Z' => byte + 32
                let lo = self.ir.context.i8_type().const_int(b'A' as u64, false);
                let hi = self.ir.context.i8_type().const_int(b'Z' as u64, false);
                let is_lo = self.ir.builder.build_int_compare(inkwell::IntPredicate::UGE, byte, lo, "stl_uge").unwrap();
                let is_hi = self.ir.builder.build_int_compare(inkwell::IntPredicate::ULE, byte, hi, "stl_ule").unwrap();
                let in_range_c = self.ir.builder.build_and(is_lo, is_hi, "stl_isuc").unwrap();
                let add32 = self.ir.builder.build_int_add(byte, self.ir.context.i8_type().const_int(32, false), "stl_add").unwrap();
                self.ir.builder.build_select(in_range_c, add32, byte, "stl_sel").unwrap().into_int_value()
            };
            let dst_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[i_val], "stl_dst").unwrap() };
            self.ir.builder.build_store(dst_gep, converted).unwrap();
            let next_i = self.ir.builder.build_int_add(i_val, i64_ty.const_int(1, false), "stl_ni").unwrap();
            self.ir.builder.build_store(i_slot, next_i).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: null-terminate and return ───────────────────────────
            self.ir.builder.position_at_end(done_bb);
            let null_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[s_len], "stl_null").unwrap() };
            self.ir.builder.build_store(null_gep, self.ir.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, s_len, 0, "stl_r0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, buf, 1, "stl_r1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

        // ── Phase 6: str_trim / str_trim_start / str_trim_end ────────────────
        // Each trims ASCII whitespace (bytes <= 32).
        // Strategy: compute new ptr/len without allocating (returns a slice into the original).
        // For simplicity, we malloc+memcpy to preserve the "always owns" invariant.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", ft, None)
            });

            // Helper for all three trim variants.
            // trim_start: advance ptr while isspace; trim_end: retreat len while isspace.
            for (fname, do_start, do_end) in &[
                ("str_trim", true, true),
                ("str_trim_start", true, false),
                ("str_trim_end", false, true),
            ] {
                let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
                let fn_val = self.ir.module.add_function(fname, fn_ty, None);
                let entry_bb = self.ir.context.append_basic_block(fn_val, "stt_entry");
                let saved = self.ir.builder.get_insert_block();
                self.ir.builder.position_at_end(entry_bb);

                let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let orig_len = self.ir.builder.build_extract_value(s, 0, "stt_olen").unwrap().into_int_value();
                let orig_ptr = self.ir.builder.build_extract_value(s, 1, "stt_optr").unwrap().into_pointer_value();

                // start = 0, end = orig_len
                let start_slot = self.ir.builder.build_alloca(i64_ty, "stt_start").unwrap();
                let end_slot   = self.ir.builder.build_alloca(i64_ty, "stt_end").unwrap();
                self.ir.builder.build_store(start_slot, i64_ty.const_zero()).unwrap();
                self.ir.builder.build_store(end_slot, orig_len).unwrap();

                let space_threshold = self.ir.context.i8_type().const_int(32, false);

                if *do_start {
                    // while start < end && orig_ptr[start] <= 32: start++
                    let ts_cond = self.ir.context.append_basic_block(fn_val, "stt_sc");
                    let ts_body = self.ir.context.append_basic_block(fn_val, "stt_sb");
                    let ts_done = self.ir.context.append_basic_block(fn_val, "stt_sd");
                    self.ir.builder.build_unconditional_branch(ts_cond).unwrap();
                    self.ir.builder.position_at_end(ts_cond);
                    let cur_start = self.ir.builder.build_load(i64_ty, start_slot, "stt_cs").unwrap().into_int_value();
                    let cur_end   = self.ir.builder.build_load(i64_ty, end_slot, "stt_ce").unwrap().into_int_value();
                    let in_range  = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, cur_start, cur_end, "stt_ir").unwrap();
                    // check byte
                    let byte_ptr = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), orig_ptr, &[cur_start], "stt_bp").unwrap() };
                    let byte_val = self.ir.builder.build_load(self.ir.context.i8_type(), byte_ptr, "stt_bv").unwrap().into_int_value();
                    let is_space = self.ir.builder.build_int_compare(inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_isp").unwrap();
                    let should_skip = self.ir.builder.build_and(in_range, is_space, "stt_skip").unwrap();
                    self.ir.builder.build_conditional_branch(should_skip, ts_body, ts_done).unwrap();
                    self.ir.builder.position_at_end(ts_body);
                    let next_start = self.ir.builder.build_int_add(cur_start, i64_ty.const_int(1, false), "stt_ns").unwrap();
                    self.ir.builder.build_store(start_slot, next_start).unwrap();
                    self.ir.builder.build_unconditional_branch(ts_cond).unwrap();
                    self.ir.builder.position_at_end(ts_done);
                }

                if *do_end {
                    // while end > start && orig_ptr[end-1] <= 32: end--
                    let te_cond = self.ir.context.append_basic_block(fn_val, "stt_ec");
                    let te_body = self.ir.context.append_basic_block(fn_val, "stt_eb");
                    let te_done = self.ir.context.append_basic_block(fn_val, "stt_ed");
                    self.ir.builder.build_unconditional_branch(te_cond).unwrap();
                    self.ir.builder.position_at_end(te_cond);
                    let cur_start = self.ir.builder.build_load(i64_ty, start_slot, "stt_ecs").unwrap().into_int_value();
                    let cur_end   = self.ir.builder.build_load(i64_ty, end_slot, "stt_ece").unwrap().into_int_value();
                    let in_range  = self.ir.builder.build_int_compare(inkwell::IntPredicate::SGT, cur_end, cur_start, "stt_eir").unwrap();
                    let prev_idx  = self.ir.builder.build_int_sub(cur_end, i64_ty.const_int(1, false), "stt_pi").unwrap();
                    let byte_ptr  = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), orig_ptr, &[prev_idx], "stt_ebp").unwrap() };
                    let byte_val  = self.ir.builder.build_load(self.ir.context.i8_type(), byte_ptr, "stt_ebv").unwrap().into_int_value();
                    let is_space  = self.ir.builder.build_int_compare(inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_eisp").unwrap();
                    let should_trim = self.ir.builder.build_and(in_range, is_space, "stt_etrim").unwrap();
                    self.ir.builder.build_conditional_branch(should_trim, te_body, te_done).unwrap();
                    self.ir.builder.position_at_end(te_body);
                    let next_end = self.ir.builder.build_int_sub(cur_end, i64_ty.const_int(1, false), "stt_ne").unwrap();
                    self.ir.builder.build_store(end_slot, next_end).unwrap();
                    self.ir.builder.build_unconditional_branch(te_cond).unwrap();
                    self.ir.builder.position_at_end(te_done);
                }

                // new_start, new_end computed; new_len = end - start
                let final_start = self.ir.builder.build_load(i64_ty, start_slot, "stt_fs").unwrap().into_int_value();
                let final_end   = self.ir.builder.build_load(i64_ty, end_slot, "stt_fe").unwrap().into_int_value();
                let new_len = self.ir.builder.build_int_sub(final_end, final_start, "stt_nl").unwrap();

                // malloc(new_len + 1)
                let alloc_sz = self.ir.builder.build_int_add(new_len, i64_ty.const_int(1, false), "stt_az").unwrap();
                let buf = self.ir.builder.build_call(malloc_fn, &[alloc_sz.into()], "stt_buf").unwrap()
                    .try_as_basic_value().left().unwrap().into_pointer_value();
                // memcpy(buf, orig_ptr+start, new_len)
                let src_ptr = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), orig_ptr, &[final_start], "stt_src").unwrap() };
                self.ir.builder.build_call(memcpy_fn, &[buf.into(), src_ptr.into(), new_len.into()], "stt_cpy").unwrap();
                // null-terminate
                let null_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[new_len], "stt_nul").unwrap() };
                self.ir.builder.build_store(null_gep, self.ir.context.i8_type().const_zero()).unwrap();

                // return str { new_len, buf }
                let mut result = str_ty.const_zero();
                result = self.ir.builder.build_insert_value(result, new_len, 0, "stt_r0").unwrap().into_struct_value();
                result = self.ir.builder.build_insert_value(result, buf, 1, "stt_r1").unwrap().into_struct_value();
                self.ir.builder.build_return(Some(&result)).unwrap();
                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert(fname.to_string(), fn_val);
                self.fn_return_types.insert(fname.to_string(), Type::Str);
            }
        }

        // ── Phase 6: str_repeat(s: str, n: i64) -> str ───────────────────────
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_repeat", fn_ty, None);
            // Create all blocks upfront.
            let entry_bb = self.ir.context.append_basic_block(fn_val, "srep_entry");
            let loop_bb  = self.ir.context.append_basic_block(fn_val, "srep_loop");
            let body_bb  = self.ir.context.append_basic_block(fn_val, "srep_body");
            let done_bb  = self.ir.context.append_basic_block(fn_val, "srep_done");
            let saved = self.ir.builder.get_insert_block();

            // ── entry ──────────────────────────────────────────────────────
            self.ir.builder.position_at_end(entry_bb);
            let s   = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let n   = fn_val.get_nth_param(1).unwrap().into_int_value();
            let s_len = self.ir.builder.build_extract_value(s, 0, "srep_slen").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "srep_sptr").unwrap().into_pointer_value();

            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", ft, None)
            });

            // n_clamped = max(n, 0); total_len = s_len * n_clamped
            let zero = i64_ty.const_zero();
            let n_neg = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, n, zero, "srep_neg").unwrap();
            let n_clamped = self.ir.builder.build_select(n_neg, zero, n, "srep_nc").unwrap().into_int_value();
            let total_len = self.ir.builder.build_int_mul(s_len, n_clamped, "srep_tlen").unwrap();
            // malloc(total_len + 1)
            let alloc_sz = self.ir.builder.build_int_add(total_len, i64_ty.const_int(1, false), "srep_az").unwrap();
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_sz.into()], "srep_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let i_slot = self.ir.builder.build_alloca(i64_ty, "srep_i").unwrap();
            self.ir.builder.build_store(i_slot, zero).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── loop: if i < n_clamped goto body else done ─────────────────
            self.ir.builder.position_at_end(loop_bb);
            let i_val = self.ir.builder.build_load(i64_ty, i_slot, "srep_iv").unwrap().into_int_value();
            let in_range = self.ir.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, n_clamped, "srep_ir").unwrap();
            self.ir.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // ── body: memcpy one copy, i++ ─────────────────────────────────
            self.ir.builder.position_at_end(body_bb);
            let offset = self.ir.builder.build_int_mul(i_val, s_len, "srep_off").unwrap();
            let dst = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[offset], "srep_dst").unwrap() };
            self.ir.builder.build_call(memcpy_fn, &[dst.into(), s_ptr.into(), s_len.into()], "srep_mc").unwrap();
            let next_i = self.ir.builder.build_int_add(i_val, i64_ty.const_int(1, false), "srep_ni").unwrap();
            self.ir.builder.build_store(i_slot, next_i).unwrap();
            self.ir.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: null-terminate and return ───────────────────────────
            self.ir.builder.position_at_end(done_bb);
            let null_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[total_len], "srep_nul").unwrap() };
            self.ir.builder.build_store(null_gep, self.ir.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, total_len, 0, "srep_r0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, buf, 1, "srep_r1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_repeat".to_string(), fn_val);
            self.fn_return_types.insert("str_repeat".to_string(), Type::Str);
        }

        // ── Phase 6: str_replace(s: str, from: str, to: str) -> str ──────────
        // Replaces all non-overlapping occurrences of `from` in `s` with `to`.
        // Uses strstr for finding, then malloc+memcpy for building the result.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_replace", fn_ty, None);

            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", ft, None)
            });
            let strstr_fn = self.ir.module.get_function("strstr").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.ir.module.add_function("strstr", ft, None)
            });

            let entry_bb = self.ir.context.append_basic_block(fn_val, "srpl_entry");
            let count_cond = self.ir.context.append_basic_block(fn_val, "srpl_cc");
            let count_body = self.ir.context.append_basic_block(fn_val, "srpl_cb");
            let build_init = self.ir.context.append_basic_block(fn_val, "srpl_bi");
            let build_cond = self.ir.context.append_basic_block(fn_val, "srpl_bc");
            let build_body = self.ir.context.append_basic_block(fn_val, "srpl_bb");
            let build_done = self.ir.context.append_basic_block(fn_val, "srpl_bd");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s    = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let from = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let to   = fn_val.get_nth_param(2).unwrap().into_struct_value();

            let s_len    = self.ir.builder.build_extract_value(s, 0, "srpl_slen").unwrap().into_int_value();
            let s_ptr    = self.ir.builder.build_extract_value(s, 1, "srpl_sptr").unwrap().into_pointer_value();
            let from_len = self.ir.builder.build_extract_value(from, 0, "srpl_flen").unwrap().into_int_value();
            let from_ptr = self.ir.builder.build_extract_value(from, 1, "srpl_fptr").unwrap().into_pointer_value();
            let to_len   = self.ir.builder.build_extract_value(to, 0, "srpl_tlen").unwrap().into_int_value();
            let to_ptr   = self.ir.builder.build_extract_value(to, 1, "srpl_tptr").unwrap().into_pointer_value();

            // --- Pass 1: count occurrences and compute output length ---
            let count_slot  = self.ir.builder.build_alloca(i64_ty, "srpl_cnt").unwrap();
            let out_len_slot = self.ir.builder.build_alloca(i64_ty, "srpl_ol").unwrap();
            let scan_slot   = self.ir.builder.build_alloca(i8_ptr, "srpl_scan").unwrap();
            self.ir.builder.build_store(count_slot, i64_ty.const_zero()).unwrap();
            self.ir.builder.build_store(out_len_slot, s_len).unwrap();
            self.ir.builder.build_store(scan_slot, s_ptr).unwrap();
            // If from_len == 0, skip replacement (avoid infinite loop).
            let from_empty = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, from_len, i64_ty.const_zero(), "srpl_fe").unwrap();
            self.ir.builder.build_conditional_branch(from_empty, build_init, count_cond).unwrap();

            self.ir.builder.position_at_end(count_cond);
            let scan = self.ir.builder.build_load(i8_ptr, scan_slot, "srpl_sv").unwrap().into_pointer_value();
            let found = self.ir.builder.build_call(strstr_fn, &[scan.into(), from_ptr.into()], "srpl_found").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            // found == null → done
            let null_ptr = i8_ptr.const_null();
            let is_null = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.ir.builder.build_ptr_to_int(found, i64_ty, "srpl_fi").unwrap(),
                self.ir.builder.build_ptr_to_int(null_ptr, i64_ty, "srpl_ni").unwrap(),
                "srpl_isnull"
            ).unwrap();
            self.ir.builder.build_conditional_branch(is_null, build_init, count_body).unwrap();

            self.ir.builder.position_at_end(count_body);
            // count++; out_len += (to_len - from_len); scan = found + from_len
            let cnt = self.ir.builder.build_load(i64_ty, count_slot, "srpl_cv").unwrap().into_int_value();
            let new_cnt = self.ir.builder.build_int_add(cnt, i64_ty.const_int(1, false), "srpl_nc").unwrap();
            self.ir.builder.build_store(count_slot, new_cnt).unwrap();

            let ol = self.ir.builder.build_load(i64_ty, out_len_slot, "srpl_olv").unwrap().into_int_value();
            let ol_adj = self.ir.builder.build_int_add(
                self.ir.builder.build_int_sub(ol, from_len, "srpl_sub").unwrap(),
                to_len, "srpl_ol2"
            ).unwrap();
            self.ir.builder.build_store(out_len_slot, ol_adj).unwrap();

            // scan = found + from_len
            let new_scan = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), found, &[from_len], "srpl_ns").unwrap() };
            self.ir.builder.build_store(scan_slot, new_scan).unwrap();
            self.ir.builder.build_unconditional_branch(count_cond).unwrap();

            // --- Pass 2: build output ---
            self.ir.builder.position_at_end(build_init);
            let out_len = self.ir.builder.build_load(i64_ty, out_len_slot, "srpl_fin_ol").unwrap().into_int_value();
            let alloc_sz = self.ir.builder.build_int_add(out_len, i64_ty.const_int(1, false), "srpl_az").unwrap();
            let out_buf = self.ir.builder.build_call(malloc_fn, &[alloc_sz.into()], "srpl_obuf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            let write_slot = self.ir.builder.build_alloca(i8_ptr, "srpl_wr").unwrap();
            self.ir.builder.build_store(write_slot, out_buf).unwrap();
            self.ir.builder.build_store(scan_slot, s_ptr).unwrap();
            self.ir.builder.build_unconditional_branch(build_cond).unwrap();

            self.ir.builder.position_at_end(build_cond);
            let scan2 = self.ir.builder.build_load(i8_ptr, scan_slot, "srpl_s2").unwrap().into_pointer_value();
            let found2 = self.ir.builder.build_call(strstr_fn, &[scan2.into(), from_ptr.into()], "srpl_f2").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let is_null2 = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.ir.builder.build_ptr_to_int(found2, i64_ty, "srpl_f2i").unwrap(),
                self.ir.builder.build_ptr_to_int(null_ptr, i64_ty, "srpl_n2i").unwrap(),
                "srpl_isnull2"
            ).unwrap();
            let from_empty2 = self.ir.builder.build_int_compare(inkwell::IntPredicate::EQ, from_len, i64_ty.const_zero(), "srpl_fe2").unwrap();
            let skip = self.ir.builder.build_or(is_null2, from_empty2, "srpl_skip").unwrap();
            self.ir.builder.build_conditional_branch(skip, build_done, build_body).unwrap();

            self.ir.builder.position_at_end(build_body);
            // copy [scan2, found2) into write, then copy to
            let wr = self.ir.builder.build_load(i8_ptr, write_slot, "srpl_wrv").unwrap().into_pointer_value();
            let prefix_len = self.ir.builder.build_int_sub(
                self.ir.builder.build_ptr_to_int(found2, i64_ty, "srpl_pfound").unwrap(),
                self.ir.builder.build_ptr_to_int(scan2, i64_ty, "srpl_pscan").unwrap(),
                "srpl_plen"
            ).unwrap();
            self.ir.builder.build_call(memcpy_fn, &[wr.into(), scan2.into(), prefix_len.into()], "srpl_cpy1").unwrap();
            let wr2 = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), wr, &[prefix_len], "srpl_wr2").unwrap() };
            self.ir.builder.build_call(memcpy_fn, &[wr2.into(), to_ptr.into(), to_len.into()], "srpl_cpy2").unwrap();
            let wr3 = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), wr2, &[to_len], "srpl_wr3").unwrap() };
            self.ir.builder.build_store(write_slot, wr3).unwrap();
            let new_scan2 = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), found2, &[from_len], "srpl_ns2").unwrap() };
            self.ir.builder.build_store(scan_slot, new_scan2).unwrap();
            self.ir.builder.build_unconditional_branch(build_cond).unwrap();

            self.ir.builder.position_at_end(build_done);
            // copy remaining tail
            let final_scan = self.ir.builder.build_load(i8_ptr, scan_slot, "srpl_fscan").unwrap().into_pointer_value();
            let final_wr   = self.ir.builder.build_load(i8_ptr, write_slot, "srpl_fwr").unwrap().into_pointer_value();
            let tail_len = self.ir.builder.build_int_sub(
                self.ir.builder.build_int_add(
                    self.ir.builder.build_ptr_to_int(s_ptr, i64_ty, "srpl_sp_int").unwrap(),
                    s_len, "srpl_sp_end"
                ).unwrap(),
                self.ir.builder.build_ptr_to_int(final_scan, i64_ty, "srpl_scan_int").unwrap(),
                "srpl_tlen"
            ).unwrap();
            self.ir.builder.build_call(memcpy_fn, &[final_wr.into(), final_scan.into(), tail_len.into()], "srpl_tail").unwrap();
            // null-terminate out_buf[out_len] = 0
            let null_gep = unsafe { self.ir.builder.build_gep(self.ir.context.i8_type(), out_buf, &[out_len], "srpl_nul").unwrap() };
            self.ir.builder.build_store(null_gep, self.ir.context.i8_type().const_zero()).unwrap();

            // return str { out_len, out_buf }
            let mut result = str_ty.const_zero();
            result = self.ir.builder.build_insert_value(result, out_len, 0, "srpl_r0").unwrap().into_struct_value();
            result = self.ir.builder.build_insert_value(result, out_buf, 1, "srpl_r1").unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_replace".to_string(), fn_val);
            self.fn_return_types.insert("str_replace".to_string(), Type::Str);
        }

        // ── Phase 6: env_var(name: str) -> Result<str, str> ──────────────────
        // Calls C getenv(). Returns Ok(str) if set, Err("not set") otherwise.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("env_var", fn_ty, None);

            let getenv_fn = self.ir.module.get_function("getenv").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into()], false);
                self.ir.module.add_function("getenv", ft, None)
            });
            let strlen_fn = self.ir.module.get_function("strlen").unwrap_or_else(|| {
                let ft = i64_ty.fn_type(&[i8_ptr.into()], false);
                self.ir.module.add_function("strlen", ft, None)
            });

            let entry_bb = self.ir.context.append_basic_block(fn_val, "ev_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "ev_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "ev_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let name_s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let name_ptr = self.ir.builder.build_extract_value(name_s, 1, "ev_np").unwrap().into_pointer_value();

            let val_ptr = self.ir.builder.build_call(getenv_fn, &[name_ptr.into()], "ev_val").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            let null_ptr = i8_ptr.const_null();
            let is_null = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.ir.builder.build_ptr_to_int(val_ptr, i64_ty, "ev_vi").unwrap(),
                self.ir.builder.build_ptr_to_int(null_ptr, i64_ty, "ev_ni").unwrap(),
                "ev_isnull"
            ).unwrap();
            self.ir.builder.build_conditional_branch(is_null, err_bb, ok_bb).unwrap();

            // Ok branch: return { tag=1, payload=str{strlen(val_ptr), val_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let val_len = self.ir.builder.build_call(strlen_fn, &[val_ptr.into()], "ev_vlen").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let ok_str_ptr = self.ir.builder.build_alloca(result_ty, "ev_ok_r").unwrap();
            let tag_gep = self.ir.builder.build_struct_gep(result_ty, ok_str_ptr, 0, "ev_tag").unwrap();
            self.ir.builder.build_store(tag_gep, bool_ty.const_int(1, false)).unwrap();
            let payload_gep = self.ir.builder.build_struct_gep(result_ty, ok_str_ptr, 1, "ev_pay").unwrap();
            let payload_as_str = self.ir.builder.build_pointer_cast(payload_gep, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr").unwrap();
            let ok_str = {
                let mut sv = str_ty.const_zero();
                sv = self.ir.builder.build_insert_value(sv, val_len, 0, "ev_sv0").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, val_ptr, 1, "ev_sv1").unwrap().into_struct_value();
                sv
            };
            self.ir.builder.build_store(payload_as_str, ok_str).unwrap();
            let ok_result = self.ir.builder.build_load(result_ty, ok_str_ptr, "ev_ok_val").unwrap();
            self.ir.builder.build_return(Some(&ok_result)).unwrap();

            // Err branch: return { tag=0, payload=str{"not set"} }
            self.ir.builder.position_at_end(err_bb);
            let err_msg = "not set\0";
            let err_global = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(err_msg.len() as u32), None, "ev_err_str"
            );
            let err_bytes: Vec<_> = err_msg.bytes().map(|c| self.ir.context.i8_type().const_int(c as u64, false)).collect();
            err_global.set_initializer(&self.ir.context.i8_type().const_array(&err_bytes));
            err_global.set_constant(true);
            let err_ptr = self.ir.builder.build_pointer_cast(
                err_global.as_pointer_value(), i8_ptr, "ev_eptr"
            ).unwrap();
            let err_str_ptr = self.ir.builder.build_alloca(result_ty, "ev_err_r").unwrap();
            let tag_gep2 = self.ir.builder.build_struct_gep(result_ty, err_str_ptr, 0, "ev_etag").unwrap();
            self.ir.builder.build_store(tag_gep2, bool_ty.const_int(0, false)).unwrap();
            let payload_gep2 = self.ir.builder.build_struct_gep(result_ty, err_str_ptr, 1, "ev_epay").unwrap();
            let payload_as_str2 = self.ir.builder.build_pointer_cast(payload_gep2, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr2").unwrap();
            let err_str = {
                let err_len = i64_ty.const_int((err_msg.len() - 1) as u64, false); // exclude null
                let mut sv = str_ty.const_zero();
                sv = self.ir.builder.build_insert_value(sv, err_len, 0, "ev_es0").unwrap().into_struct_value();
                sv = self.ir.builder.build_insert_value(sv, err_ptr, 1, "ev_es1").unwrap().into_struct_value();
                sv
            };
            self.ir.builder.build_store(payload_as_str2, err_str).unwrap();
            let err_result = self.ir.builder.build_load(result_ty, err_str_ptr, "ev_err_val").unwrap();
            self.ir.builder.build_return(Some(&err_result)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("env_var".to_string(), fn_val);
            let result_type = Type::Result(Box::new(Type::Str), Box::new(Type::Str));
            self.fn_return_types.insert("env_var".to_string(), result_type);
        }

        // ── Phase 6: exit(code: i64) -> () ───────────────────────────────────
        {
            let c_exit_fn = self.ir.module.get_function("exit").unwrap_or_else(|| {
                let ft = self.ir.context.void_type().fn_type(&[self.ir.context.i32_type().into()], false);
                self.ir.module.add_function("exit", ft, None)
            });
            let fn_ty = self.ir.context.void_type().fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("exit_axon", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ex_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let code = fn_val.get_nth_param(0).unwrap().into_int_value();
            let code_i32 = self.ir.builder.build_int_truncate(code, self.ir.context.i32_type(), "ex_code").unwrap();
            self.ir.builder.build_call(c_exit_fn, &[code_i32.into()], "ex_call").unwrap();
            self.ir.builder.build_unreachable().unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            // Register as "exit" — this is what the Axon source calls
            self.functions.insert("exit".to_string(), fn_val);
            self.fn_return_types.insert("exit".to_string(), Type::Unit);
        }

        // ── Phase 7: str_len(s: str) -> i64 ──────────────────────────────────
        // Extracts the length field (index 0) from the str struct.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_len", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "sl_entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let len = self.ir.builder.build_extract_value(s, 0, "sl_len").unwrap().into_int_value();
            self.ir.builder.build_return(Some(&len)).unwrap();
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_len".to_string(), fn_val);
            self.fn_return_types.insert("str_len".to_string(), Type::I64);
        }

        // ── Phase 7: str_pad_start / str_pad_end ─────────────────────────────
        // str_pad_start(s: str, width: i64, fill: str) -> str
        //   Left-pad s with fill[0] until byte-length == width (no-op if already >= width).
        // str_pad_end(s: str, width: i64, fill: str) -> str
        //   Right-pad s with fill[0] until byte-length == width.
        for pad_start in &[true, false] {
            let fname = if *pad_start { "str_pad_start" } else { "str_pad_end" };
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);

            // Blocks: entry → short-circuit (width <= len) or pad path → done
            let entry_bb = self.ir.context.append_basic_block(fn_val, "sp_entry");
            let pad_bb   = self.ir.context.append_basic_block(fn_val, "sp_pad");
            let done_bb  = self.ir.context.append_basic_block(fn_val, "sp_done");

            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let width = fn_val.get_nth_param(1).unwrap().into_int_value();
            let fill  = fn_val.get_nth_param(2).unwrap().into_struct_value();

            let s_len = self.ir.builder.build_extract_value(s, 0, "sp_slen").unwrap().into_int_value();
            let s_ptr = self.ir.builder.build_extract_value(s, 1, "sp_sptr").unwrap().into_pointer_value();
            let fill_ptr = self.ir.builder.build_extract_value(fill, 1, "sp_fptr").unwrap().into_pointer_value();

            // if s_len >= width: return s as-is
            let need_pad = self.ir.builder.build_int_compare(
                inkwell::IntPredicate::SLT, s_len, width, "sp_need").unwrap();
            self.ir.builder.build_conditional_branch(need_pad, pad_bb, done_bb).unwrap();

            // pad_bb: allocate width+1 bytes, fill pad chars, copy s, null-terminate
            self.ir.builder.position_at_end(pad_bb);
            let pad_len = self.ir.builder.build_int_sub(width, s_len, "sp_padlen").unwrap();
            let alloc_size = self.ir.builder.build_int_add(width, i64_ty.const_int(1, false), "sp_alloc").unwrap();
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", malloc_ty, None)
            });
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", memcpy_ty, None)
            });
            let buf = self.ir.builder.build_call(malloc_fn, &[alloc_size.into()], "sp_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            // fill_char = fill_ptr[0]
            let fill_char = self.ir.builder.build_load(self.ir.context.i8_type(), fill_ptr, "sp_fchar").unwrap().into_int_value();
            // Use memset (declare if needed)
            let memset_fn = self.ir.module.get_function("memset").unwrap_or_else(|| {
                let memset_ty = i8_ptr.fn_type(
                    &[i8_ptr.into(), self.ir.context.i32_type().into(), i64_ty.into()], false);
                self.ir.module.add_function("memset", memset_ty, None)
            });
            let fill_char_i32 = self.ir.builder.build_int_z_extend(fill_char, self.ir.context.i32_type(), "sp_fc32").unwrap();
            if *pad_start {
                // Pad bytes at start, then s
                self.ir.builder.build_call(memset_fn, &[buf.into(), fill_char_i32.into(), pad_len.into()], "").unwrap();
                let s_dest = unsafe {
                    self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[pad_len], "sp_sdest").unwrap()
                };
                self.ir.builder.build_call(memcpy_fn, &[s_dest.into(), s_ptr.into(), s_len.into()], "").unwrap();
            } else {
                // s then pad bytes
                self.ir.builder.build_call(memcpy_fn, &[buf.into(), s_ptr.into(), s_len.into()], "").unwrap();
                let pad_dest = unsafe {
                    self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[s_len], "sp_pdest").unwrap()
                };
                self.ir.builder.build_call(memset_fn, &[pad_dest.into(), fill_char_i32.into(), pad_len.into()], "").unwrap();
            }
            // null-terminate
            let null_pos = unsafe {
                self.ir.builder.build_gep(self.ir.context.i8_type(), buf, &[width], "sp_null").unwrap()
            };
            self.ir.builder.build_store(null_pos, self.ir.context.i8_type().const_int(0, false)).unwrap();
            self.ir.builder.build_unconditional_branch(done_bb).unwrap();

            // done_bb: phi nodes must come FIRST (before any non-phi instructions).
            self.ir.builder.position_at_end(done_bb);
            let len_phi = self.ir.builder.build_phi(i64_ty, "sp_rlen").unwrap();
            len_phi.add_incoming(&[(&s_len, entry_bb), (&width, pad_bb)]);
            let ptr_phi = self.ir.builder.build_phi(i8_ptr, "sp_rptr").unwrap();
            ptr_phi.add_incoming(&[(&s_ptr, entry_bb), (&buf, pad_bb)]);
            // Build the result str struct using insert_value (no alloca needed).
            let mut sp_res = str_ty.get_undef();
            sp_res = self.ir.builder
                .build_insert_value(sp_res, len_phi.as_basic_value().into_int_value(), 0, "sp_wl")
                .unwrap().into_struct_value();
            sp_res = self.ir.builder
                .build_insert_value(sp_res, ptr_phi.as_basic_value().into_pointer_value(), 1, "sp_rv")
                .unwrap().into_struct_value();
            self.ir.builder.build_return(Some(&sp_res)).unwrap();

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

    }

}
