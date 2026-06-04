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

use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;
use inkwell::FloatPredicate;
use inkwell::IntPredicate;

use crate::types::Type;

// Non-generic `#[inline(never)]` inkwell builder wrappers (see build_wrappers.rs):
// routing `declare_builtins`' `.build_*` calls through these collapses each generic
// inkwell instantiation to one copy instead of one-per-call-site.
use super::build_wrappers;

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

        // R1d slice 1: single-source registry for the straight `declare → link`
        // externs (math scalars, str predicates, dict scalars). Replaces the
        // hand-written get-or-`add_function` + `functions`/`fn_return_types`
        // blocks that used to live inline below and in declare_string_builtins /
        // declare_phase9_math_builtins. See codegen/builtin_externs.rs.
        self.declare_builtin_externs();

        // R7 (AOT-wasm): declare `malloc` ONCE, up front, with the target-correct
        // `size_t` width — i32 on wasm32 (ILP32), i64 on native (LP64). Every
        // later `get_function("malloc").unwrap_or_else(…)` reuses THIS decl (the
        // fallbacks are now dead), so the module can't end up with the i64 malloc
        // that traps the wasm32 verifier (`expected i32, found i64`). Call sites
        // narrow their i64 byte-count to `size_ty()` via `emit_malloc`/`msize`.
        let malloc_size_ty = self.size_ty();
        let malloc_ty0 = i8_ptr.fn_type(&[malloc_size_ty.into()], false);
        self.ir.module.add_function("malloc", malloc_ty0, None);
        // Same size_t story for memcpy/memset (`void* memcpy(dst, src, size_t)`,
        // `void* memset(dst, int, size_t)`): declare once at target width so
        // every later get_function reuses it; call sites narrow the count via
        // msize(). Used by axon_concat (string interpolation), str_slice/pad.
        let memcpy_ty0 = i8_ptr.fn_type(
            &[i8_ptr.into(), i8_ptr.into(), malloc_size_ty.into()],
            false,
        );
        self.ir.module.add_function("memcpy", memcpy_ty0, None);
        let memset_ty0 = i8_ptr.fn_type(
            &[i8_ptr.into(), i32_ty.into(), malloc_size_ty.into()],
            false,
        );
        self.ir.module.add_function("memset", memset_ty0, None);
        // `size_t strlen(const char*)`: the RESULT is size_t (i32 on wasm32,
        // i64 native). Callers that feed it into the i64 AxonStr len field
        // zero-extend on wasm. Declared once at target width so it agrees with
        // the wasi libc strlen (else: `strlen … (i32)->i64 vs (i32)->i32`).
        let strlen_ty0 = malloc_size_ty.fn_type(&[i8_ptr.into()], false);
        self.ir.module.add_function("strlen", strlen_ty0, None);
        // `int strncmp(const char*, const char*, size_t n)`: the count is
        // size_t (i32 on wasm32). Declared once at target width; the parse_bool
        // call sites narrow the count via msize().
        let strncmp_ty0 = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), malloc_size_ty.into()], false);
        self.ir.module.add_function("strncmp", strncmp_ty0, None);

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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 1, "data_ptr")
                .into_pointer_value();
            build_wrappers::w_call(&self.ir.builder,puts_fn, &[data_ptr.into()], "");
            build_wrappers::w_ret_void(&self.ir.builder);
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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 1, "data_ptr")
                .into_pointer_value();
            // printf("%s", data_ptr)
            let fmt = self.ir.context.const_string(b"%s", true);
            let fmt_global = self.ir.module.add_global(fmt.get_type(), None, "print_fmt");
            fmt_global.set_initializer(&fmt);
            fmt_global.set_constant(true);
            let fmt_ptr = fmt_global.as_pointer_value();
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[fmt_ptr.into(), data_ptr.into()], "");
            build_wrappers::w_ret_void(&self.ir.builder);
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
            build_wrappers::w_cond_br(&self.ir.builder,cond, ok_bb, fail_bb);

            self.ir.builder.position_at_end(fail_bb);
            let msg = b"assertion failed\n\0";
            let msg_const = self.ir.context.const_string(msg, false);
            let msg_global = self.ir.module.add_global(msg_const.get_type(), None, "assert_msg");
            msg_global.set_initializer(&msg_const);
            msg_global.set_constant(true);
            let msg_ptr = msg_global.as_pointer_value();
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[msg_ptr.into()], "");
            let one = i32_ty.const_int(1, false);
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[one.into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);

            self.ir.builder.position_at_end(ok_bb);
            build_wrappers::w_ret_void(&self.ir.builder);

            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert".to_string(), fn_val);
        }

        // Declare `ssize_t write(int fd, const void *buf, size_t count)` for
        // stderr output. R7: `count` is `size_t` and the result `ssize_t` — both
        // i32 on wasm32 (ILP32), i64 on native (LP64). Call sites narrow the
        // count via msize(); the result is discarded by eprintln/eprint.
        let write_ret_ty = self.size_ty();
        let write_ty = write_ret_ty.fn_type(&[i32_ty.into(), i8_ptr.into(), malloc_size_ty.into()], false);
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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 1, "data_ptr")
                .into_pointer_value();
            let length = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 0, "ep_len")
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            // Write the string content.
            build_wrappers::w_call(&self.ir.builder,write_fn, &[fd2.into(), data_ptr.into(), self.msize(length, "msz").into()], "");
            // Write the newline.
            let nl_arr = self.ir.context.i8_type().array_type(1);
            let nl_g = self.ir.module.add_global(nl_arr, None, "eprintln_nl");
            nl_g.set_initializer(&self.ir.context.i8_type().const_array(&[self.ir.context.i8_type().const_int(b'\n' as u64, false)]));
            nl_g.set_constant(true);
            let nl_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,nl_g.as_pointer_value(), i8_ptr, "nlptr");
            let one64 = i64_ty.const_int(1, false);
            build_wrappers::w_call(&self.ir.builder,write_fn, &[fd2.into(), nl_ptr.into(), self.msize(one64, "msz").into()], "");
            build_wrappers::w_ret_void(&self.ir.builder);
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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 1, "data_ptr")
                .into_pointer_value();
            let length = build_wrappers::w_extract_value(&self.ir.builder,str_arg, 0, "ep_len")
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            build_wrappers::w_call(&self.ir.builder,write_fn, &[fd2.into(), data_ptr.into(), self.msize(length, "msz").into()], "");
            build_wrappers::w_ret_void(&self.ir.builder);
            if let Some(b) = saved_block { self.ir.builder.position_at_end(b); }
            self.functions.insert("eprint".to_string(), fn_val);
        }

        // C stdlib: int snprintf(char *buf, size_t n, const char *fmt, ...)
        // R7: `n` is `size_t` — i32 on wasm32 (ILP32), i64 on native. Call sites
        // pass the buffer length through `msize` to match this width.
        let snprintf_ty = i32_ty.fn_type(&[i8_ptr.into(), malloc_size_ty.into(), i8_ptr.into()], true);
        let snprintf_fn = self.ir.module.add_function("snprintf", snprintf_ty, None);

        // to_str: i64 → { i64, ptr }
        // Uses malloc-allocated buffer so the returned str is heap-owned and
        // remains valid when returned from a function (no dangling static buffer).
        //
        // PARITY GAP (BUG_HUNT #29 / #33): the interpreter made `to_str`
        // polymorphic over scalars (i64/f64/bool), dispatching on the runtime
        // value. Codegen still declares only the i64 form, and the generic
        // float→int arg coercion in expr.rs would truncate `to_str(3.14)` to
        // "3" instead of "3.14". Closing this needs per-arg-type dispatch here
        // (select to_str / to_str_f64 / to_str_bool by the inferred arg type)
        // and is tracked as finding #33 — deferred because the codegen build is
        // pathologically slow (see BUILD_DIAGNOSIS.md), so it can't be verified
        // in this loop. The interpreter is the reference semantics.
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
            let fmt_ptr2 = build_wrappers::w_pointer_cast(&self.ir.builder,fmt_global2.as_pointer_value(), i8_ptr, "fmtptr");

            // Pass 1: snprintf(NULL, 0, "%lld", n) → required length (not counting '\0').
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = build_wrappers::w_call(&self.ir.builder,
                    snprintf_fn,
                    &[null_ptr.into(), self.msize(zero64, "msz").into(), fmt_ptr2.into(), n.into()],
                    "snplen");
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = build_wrappers::w_int_z_extend(&self.ir.builder,len_i32, i64_ty, "len64");

            // Allocate len + 1 bytes (room for null terminator).
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = build_wrappers::w_int_add(&self.ir.builder,len_i64, one64, "allocsz");
            let buf_call = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_size, "msz").into()], "buf");
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%lld", n) → writes the decimal string.
            build_wrappers::w_call(&self.ir.builder,
                    snprintf_fn,
                    &[buf_ptr.into(), self.msize(alloc_size, "msz").into(), fmt_ptr2.into(), n.into()],
                    "snpwrite");

            // Build { i64, ptr } return struct.
            let out_alloca = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "out");
            let len_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 0, "lenptr");
            let dat_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 1, "datptr");
            build_wrappers::w_store(&self.ir.builder,len_ptr, len_i64.into());
            build_wrappers::w_store(&self.ir.builder,dat_ptr, buf_ptr.into());
            let out = build_wrappers::w_load(&self.ir.builder,str_ty.into(), out_alloca, "outval");
            build_wrappers::w_ret(&self.ir.builder, out);

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

            let raw_n = fn_val.get_nth_param(0).unwrap().into_float_value();
            // I-2 parity: the interpreter's fmt_g returns "0" for x == 0.0, which
            // (since -0.0 == 0.0) normalizes negative zero. C's snprintf("%.6g",
            // -0.0) instead prints "-0". Collapse -0.0 → +0.0 here so native
            // matches the oracle: n = (raw_n == 0.0) ? 0.0 : raw_n. The OEQ
            // compare is true for both +0.0 and -0.0, so the select replaces
            // negative zero with a literal +0.0 and leaves every other value.
            let zero_f = f64_ty.const_float(0.0);
            let is_zero = self.ir.builder
                .build_float_compare(inkwell::FloatPredicate::OEQ, raw_n, zero_f, "iszero")
                .unwrap();
            let n = self.ir.builder
                .build_select(is_zero, zero_f, raw_n, "n_norm")
                .unwrap()
                .into_float_value();
            let fmt_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,fmt_global.as_pointer_value(), i8_ptr, "fmtptr");

            // Pass 1: snprintf(NULL, 0, "%.6g", n) → required length.
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = build_wrappers::w_call(&self.ir.builder,
                    snprintf_fn,
                    &[null_ptr.into(), self.msize(zero64, "msz").into(), fmt_ptr.into(), n.into()],
                    "snplen");
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = build_wrappers::w_int_z_extend(&self.ir.builder,len_i32, i64_ty, "len64");

            // Allocate len + 1 bytes.
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = build_wrappers::w_int_add(&self.ir.builder,len_i64, one64, "allocsz");
            let buf_call = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_size, "msz").into()], "buf");
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%.6g", n).
            build_wrappers::w_call(&self.ir.builder,
                    snprintf_fn,
                    &[buf_ptr.into(), self.msize(alloc_size, "msz").into(), fmt_ptr.into(), n.into()],
                    "snpwrite");

            let out_alloca = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "out");
            let len_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 0, "lenptr");
            let dat_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 1, "datptr");
            build_wrappers::w_store(&self.ir.builder,len_ptr, len_i64.into());
            build_wrappers::w_store(&self.ir.builder,dat_ptr, buf_ptr.into());
            let out = build_wrappers::w_load(&self.ir.builder,str_ty.into(), out_alloca, "outval");
            build_wrappers::w_ret(&self.ir.builder, out);

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
            let eq = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, a, b_param, "eq");
            build_wrappers::w_cond_br(&self.ir.builder,eq, ok_bb, fail_bb);
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: values not equal\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_eq_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[msg_g.as_pointer_value().into()], "");
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[i32_ty.const_int(1, false).into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);
            self.ir.builder.position_at_end(ok_bb);
            build_wrappers::w_ret_void(&self.ir.builder);
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
            let is_ok_val = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, tag, bool_ty.const_int(1, false), "isok");
            build_wrappers::w_cond_br(&self.ir.builder,is_ok_val, fail_bb, ok_bb);
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: expected Err, got Ok\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_err_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[msg_g.as_pointer_value().into()], "");
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[i32_ty.const_int(1, false).into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);
            self.ir.builder.position_at_end(ok_bb);
            build_wrappers::w_ret_void(&self.ir.builder);
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
            let length = build_wrappers::w_extract_value(&self.ir.builder,s, 0, "len")
                .into_int_value();
            build_wrappers::w_ret(&self.ir.builder, length.into());
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
            //
            // PARITY GAP (BUG_HUNT #22 / #37): two divergences from the
            // interpreter, both needing the (pathologically slow, see
            // BUILD_DIAGNOSIS.md) codegen build to fix + verify:
            //   1. The Err payload here is an EMPTY string (see err_bb below),
            //      while the interpreter returns a specific
            //      "could not parse `<input>` as a base-10 integer" message.
            //   2. strtoll(base 10) below stops at the first non-digit and
            //      reports success if it consumed ANY leading digit. So
            //      "0x1F" consumes the leading "0" → endptr advances → this
            //      returns Ok(0); the interpreter (Rust `str::parse`) requires
            //      the WHOLE trimmed string to be a valid integer and Errs.
            //      Codegen should reject trailing garbage (endptr must reach
            //      end-of-string, modulo trailing whitespace) to match.
            // The interpreter is the reference semantics.
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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "pi_data")
                .into_pointer_value();
            // #37: also read the length so we can require strtoll to consume the
            // WHOLE string (no trailing garbage) — matching the interpreter's
            // `str::parse`, which rejects "0x1F"/"12abc" rather than returning
            // Ok of the leading digits.
            let len_val = build_wrappers::w_extract_value(&self.ir.builder, s, 0, "pi_len")
                .into_int_value();

            // Allocate an endptr on the stack so strtoll can write to it.
            let endptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "pi_endptr");
            // Null-initialise so strtoll doesn't read garbage.
            let null_ptr = i8_ptr.const_null();
            build_wrappers::w_store(&self.ir.builder,endptr_slot, null_ptr.into());

            // Cast endptr slot to i8** (same type on all targets).
            let endptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,endptr_slot, i8_ptr_ptr, "pi_endptr_cast");

            // Call strtoll(data, &endptr, 10).
            let base10 = i32_ty.const_int(10, false);
            let strtoll_ret = build_wrappers::w_call(&self.ir.builder,
                    strtoll_fn,
                    &[data_ptr.into(), endptr_slot_cast.into(), base10.into()],
                    "pi_strtoll");
            let parsed_i64 = strtoll_ret
                .try_as_basic_value()
                .left()
                .unwrap()
                .into_int_value();

            // Read back endptr to detect parse errors.
            // If endptr == data_ptr, no digits were consumed → Err.
            let endptr_val = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), endptr_slot, "pi_endptr_val")
                .into_pointer_value();
            let endptr_int = build_wrappers::w_ptr_to_int(&self.ir.builder,endptr_val, i64_ty, "pi_endptr_int");
            let data_int = build_wrappers::w_ptr_to_int(&self.ir.builder,data_ptr, i64_ty, "pi_data_int");
            // (a) at least one digit consumed: endptr != data.
            let consumed = build_wrappers::w_int_compare(&self.ir.builder,
                    IntPredicate::NE,
                    endptr_int,
                    data_int,
                    "pi_consumed");
            // (b) #37: the WHOLE string was consumed: endptr == data + len.
            // strtoll skips leading whitespace and stops at the first non-digit;
            // requiring it to reach data+len rejects trailing garbage ("12abc",
            // "0x1F") the way the interpreter's str::parse does. (Trailing
            // whitespace is a documented minor divergence: the interp trims, this
            // requires an exact end — inputs with trailing spaces are rare and
            // the interp remains the reference.)
            let end_int = build_wrappers::w_int_add(&self.ir.builder, data_int, len_val, "pi_end_int");
            let reached_end = build_wrappers::w_int_compare(&self.ir.builder,
                    IntPredicate::EQ,
                    endptr_int,
                    end_int,
                    "pi_reached_end");
            // Success iff (a) AND (b).
            let ok_cond = build_wrappers::w_and(&self.ir.builder, consumed, reached_end, "pi_ok_cond");
            build_wrappers::w_cond_br(&self.ir.builder, ok_cond, ok_bb, err_bb);

            // ok_bb: return { tag=1, payload=parsed_i64 as [8 x i8] }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pi_ok_slot");
            // Store tag = 1 (i1 true)
            let tag1 = bool_ty.const_int(1, false);
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "pi_tagptr_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, tag1.into());
            // Store the i64 value into the [8 x i8] payload via a pointer cast.
            let payload_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "pi_payptr_ok");
            let payload_i64_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ptr_ok, i64_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_i64");
            build_wrappers::w_store(&self.ir.builder,payload_i64_ptr, parsed_i64.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "pi_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: return { tag=0, payload = str { len, ptr } } with the same
            // message the interpreter produces (#37 parity): delegate to axon-rt's
            // __axon_parse_int_err, which formats `could not parse `<input>` as a
            // base-10 integer` (+ radix hint) from the INPUT str — so native==interp
            // on the message, not just both-are-Err.
            self.ir.builder.position_at_end(err_bb);
            let err_str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            // Runtime: void __axon_parse_int_err(AxonStr input, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let pie_ty = void_ty.fn_type(&[err_str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()], false);
            let pie_fn = self.ir.module.get_function("__axon_parse_int_err")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_int_err", pie_ty, None));
            // The input str `s` is the param; reassemble it as the AxonStr arg.
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pi_eolen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "pi_eoptr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "pi_eoptrptr");
            build_wrappers::w_call(&self.ir.builder, pie_fn, &[s.into(), out_len_slot.into(), out_ptr_slot_cast.into()], "pi_err_call");
            let msg_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "pi_emlen").into_int_value();
            let msg_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "pi_emptr").into_pointer_value();

            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pi_err_slot");
            let tag0 = bool_ty.const_int(0, false);
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "pi_tagptr_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, tag0.into());
            let payload_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "pi_payptr_err");
            let payload_str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ptr_err, err_str_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_str_err");
            let err_str_alloca = build_wrappers::w_alloca(&self.ir.builder,err_str_ty.into(), "pi_err_str");
            let err_str_len_ptr = build_wrappers::w_struct_gep(&self.ir.builder,err_str_ty.into(), err_str_alloca, 0, "pi_esl");
            let err_str_dat_ptr = build_wrappers::w_struct_gep(&self.ir.builder,err_str_ty.into(), err_str_alloca, 1, "pi_esd");
            build_wrappers::w_store(&self.ir.builder,err_str_len_ptr, msg_len.into());
            build_wrappers::w_store(&self.ir.builder,err_str_dat_ptr, msg_ptr.into());
            let err_str_val = build_wrappers::w_load(&self.ir.builder,err_str_ty.into(), err_str_alloca, "pi_err_str_val");
            build_wrappers::w_store(&self.ir.builder,payload_str_ptr, err_str_val);
            let err_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "pi_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_int".to_string(), fn_val);
        }

        // parse_int_radix(s: str, base: i64) -> Result<i64, str>  (BUG_HUNT #22)
        //
        // Delegate-to-rt pattern (like #37/#38/#39): the whole radix parse lives
        // in axon-rt's __axon_parse_int_radix; codegen just calls it and
        // assembles the Result<i64,str> = { i1 tag, [16 x i8] payload } struct
        // from the out-params. Byte-identical to the interpreter by construction.
        {
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(
                &[bool_ty.into(), i8_arr16_ty.into()],
                false,
            );
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_int_radix", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "pir_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "pir_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "pir_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let base = fn_val.get_nth_param(1).unwrap().into_int_value();

            // Runtime: void __axon_parse_int_radix(AxonStr s, i64 base,
            //   i64* out_ok, i64* out_val, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_ty = void_ty.fn_type(
                &[str_ty.into(), i64_ty.into(), i64_ptr.into(), i64_ptr.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.get_function("__axon_parse_int_radix")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_int_radix", rt_ty, None));

            let out_ok_slot  = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pir_ok_slot");
            let out_val_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pir_val_slot");
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pir_len_slot");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "pir_ptr_slot");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "pir_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[s.into(), base.into(), out_ok_slot.into(), out_val_slot.into(), out_len_slot.into(), out_ptr_slot_cast.into()],
                "");

            let ok_flag = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_ok_slot, "pir_okflag").into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, IntPredicate::NE, ok_flag, zero, "pir_isok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload = out_val as i64 }
            self.ir.builder.position_at_end(ok_bb);
            let parsed = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_val_slot, "pir_parsed").into_int_value();
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pir_ok_alloca");
            let tag1 = bool_ty.const_int(1, false);
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 0, "pir_tagok");
            build_wrappers::w_store(&self.ir.builder, tag_ptr_ok, tag1.into());
            let payload_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 1, "pir_payok");
            let payload_i64_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ptr_ok, i64_ty.ptr_type(inkwell::AddressSpace::default()), "pir_payi64");
            build_wrappers::w_store(&self.ir.builder, payload_i64_ptr, parsed.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), ok_alloca, "pir_okval");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload = str { out_len, out_ptr } }
            self.ir.builder.position_at_end(err_bb);
            let err_str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let msg_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "pir_emlen").into_int_value();
            let msg_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "pir_emptr").into_pointer_value();
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pir_err_alloca");
            let tag0 = bool_ty.const_int(0, false);
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 0, "pir_tagerr");
            build_wrappers::w_store(&self.ir.builder, tag_ptr_err, tag0.into());
            let payload_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 1, "pir_payerr");
            let payload_str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ptr_err, err_str_ty.ptr_type(inkwell::AddressSpace::default()), "pir_paystr");
            let err_str_alloca = build_wrappers::w_alloca(&self.ir.builder, err_str_ty.into(), "pir_errstr");
            let esl = build_wrappers::w_struct_gep(&self.ir.builder, err_str_ty.into(), err_str_alloca, 0, "pir_esl");
            let esd = build_wrappers::w_struct_gep(&self.ir.builder, err_str_ty.into(), err_str_alloca, 1, "pir_esd");
            build_wrappers::w_store(&self.ir.builder, esl, msg_len.into());
            build_wrappers::w_store(&self.ir.builder, esd, msg_ptr.into());
            let err_str_val = build_wrappers::w_load(&self.ir.builder, err_str_ty.into(), err_str_alloca, "pir_errstrval");
            build_wrappers::w_store(&self.ir.builder, payload_str_ptr, err_str_val);
            let err_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), err_alloca, "pir_errval");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_int_radix".to_string(), fn_val);
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
            let a_len = build_wrappers::w_extract_value(&self.ir.builder,a_val, 0, "a_len").into_int_value();
            let a_ptr = build_wrappers::w_extract_value(&self.ir.builder,a_val, 1, "a_ptr").into_pointer_value();
            let b_len = build_wrappers::w_extract_value(&self.ir.builder,b_val, 0, "b_len").into_int_value();
            let b_ptr = build_wrappers::w_extract_value(&self.ir.builder,b_val, 1, "b_ptr").into_pointer_value();

            // total_len = a_len + b_len
            let total_len = build_wrappers::w_int_add(&self.ir.builder,a_len, b_len, "total_len");
            // alloc_len = total_len + 1  (null terminator)
            let one64 = i64_ty.const_int(1, false);
            let alloc_len = build_wrappers::w_int_add(&self.ir.builder,total_len, one64, "alloc_len");

            // buf = malloc(alloc_len)
            let buf = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_len, "msz").into()], "buf");
            let buf_ptr = buf.try_as_basic_value().left().unwrap().into_pointer_value();

            // memcpy(buf, a_ptr, a_len)
            build_wrappers::w_call(&self.ir.builder,
                memcpy_fn,
                &[buf_ptr.into(), a_ptr.into(), self.msize(a_len, "msz").into()],
                "");

            // buf_b = buf + a_len  (GEP to offset into buf)
            let buf_b_ptr = unsafe {
                build_wrappers::w_gep(&self.ir.builder,
                    self.ir.context.i8_type().into(),
                    buf_ptr,
                    &[a_len],
                    "buf_b")
            };

            // memcpy(buf_b, b_ptr, b_len)
            build_wrappers::w_call(&self.ir.builder,
                memcpy_fn,
                &[buf_b_ptr.into(), b_ptr.into(), self.msize(b_len, "msz").into()],
                "");

            // null-terminate: *(buf + total_len) = 0
            let null_pos = unsafe {
                build_wrappers::w_gep(&self.ir.builder,
                    self.ir.context.i8_type().into(),
                    buf_ptr,
                    &[total_len],
                    "null_pos")
            };
            build_wrappers::w_store(&self.ir.builder,null_pos, self.ir.context.i8_type().const_int(0, false).into());

            // Return { total_len, buf_ptr }
            let out_alloca = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "concat_out");
            let len_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 0, "lenptr");
            let dat_ptr = build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), out_alloca, 1, "datptr");
            build_wrappers::w_store(&self.ir.builder,len_ptr, total_len.into());
            build_wrappers::w_store(&self.ir.builder,dat_ptr, buf_ptr.into());
            let out = build_wrappers::w_load(&self.ir.builder,str_ty.into(), out_alloca, "concat_val");
            build_wrappers::w_ret(&self.ir.builder, out);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("axon_concat".to_string(), fn_val);
        }

        // abs_i32 / abs_f64 / min_i32 / max_i32 — now registry rows (R1d slice 1).

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

        // ── Dict runtime externs (R1c) ──────────────────────────────────────
        // A Dict is an opaque i8* handle to an Arc<Mutex<HashMap>> in axon-rt.
        // Values are tagged (0=Int,1=Float,2=Str); str keys/values pass the
        // AxonStr by value. Codegen for dict_set/get does the call-site tag
        // dispatch (like to_str) and assembles Option<T> from get's out-params.
        let str_ty_d = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
        let i64_ptr_d = i64_ty.ptr_type(inkwell::AddressSpace::default());
        // __axon_dict_new / _has / _len / _inc — now registry rows (R1d slice 1).
        // The bespoke out-param entries (_set / _get / _remove / _keys) stay here.
        // void __axon_dict_set(d:i8*, key:str, tag:i64, payload:i64, pstr:i8*, plen:i64)
        let ds_ty = void_ty.fn_type(
            &[i8_ptr.into(), str_ty_d.into(), i64_ty.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into()], false);
        self.ir.module.add_function("__axon_dict_set", ds_ty, None);
        // i1 __axon_dict_get(d:i8*, key:str, out_tag:i64*, out_payload:i64*, out_strlen:i64*)
        let dg_ty = bool_ty.fn_type(
            &[i8_ptr.into(), str_ty_d.into(), i64_ptr_d.into(), i64_ptr_d.into(), i64_ptr_d.into()], false);
        self.ir.module.add_function("__axon_dict_get", dg_ty, None);
        // i1 __axon_dict_remove(d:i8*, key:str, out_tag, out_payload, out_strlen)
        let dr_ty = bool_ty.fn_type(
            &[i8_ptr.into(), str_ty_d.into(), i64_ptr_d.into(), i64_ptr_d.into(), i64_ptr_d.into()], false);
        self.ir.module.add_function("__axon_dict_remove", dr_ty, None);
        // void __axon_dict_keys(d:i8*, out_len:i64*, out_data:i8**)
        let i8_ptr_ptr_d = i8_ptr.ptr_type(inkwell::AddressSpace::default());
        let dk_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ptr_d.into(), i8_ptr_ptr_d.into()], false);
        self.ir.module.add_function("__axon_dict_keys", dk_ty, None);
        // void __axon_dict_values(d:i8*, out_len:i64*, out_data:i8**) — same
        // out-param shape as dict_keys; the data array is i64 (v1 int-valued).
        let dvl_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ptr_d.into(), i8_ptr_ptr_d.into()], false);
        self.ir.module.add_function("__axon_dict_values", dvl_ty, None);
        // dict_new/has/len/inc return types are now registry rows (R1d slice 1).
        // dict_set keeps its Unit entry here (bespoke out-param lowering).
        self.fn_return_types.insert("dict_set".to_string(), Type::Unit);

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
            build_wrappers::w_ret(&self.ir.builder, s);
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("format".to_string(), fn_val);
            self.fn_return_types.insert("format".to_string(), Type::Str);
        }

        self.fn_return_types.insert("parse_int".to_string(),
            Type::Result(Box::new(Type::I64), Box::new(Type::Str)));
        self.fn_return_types.insert("parse_int_radix".to_string(),
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
            let eq = build_wrappers::w_float_compare(&self.ir.builder,FloatPredicate::OEQ, a, b_param, "eq");
            build_wrappers::w_cond_br(&self.ir.builder,eq, ok_bb, fail_bb);
            self.ir.builder.position_at_end(fail_bb);
            let msg = self.ir.context.const_string(b"assertion failed: f64 values not equal\n\0", false);
            let msg_g = self.ir.module.add_global(msg.get_type(), None, "assert_eq_f64_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[msg_g.as_pointer_value().into()], "");
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[i32_ty.const_int(1, false).into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);
            self.ir.builder.position_at_end(ok_bb);
            build_wrappers::w_ret_void(&self.ir.builder);
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
            let a_len = build_wrappers::w_extract_value(&self.ir.builder,a_struct, 0, "a_len").into_int_value();
            let b_len = build_wrappers::w_extract_value(&self.ir.builder,b_struct, 0, "b_len").into_int_value();
            let len_eq = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, a_len, b_len, "len_eq");
            build_wrappers::w_cond_br(&self.ir.builder,len_eq, cmp_bb, len_fail_bb);
            // lengths differ → fail
            self.ir.builder.position_at_end(len_fail_bb);
            let fail_msg = self.ir.context.const_string(b"assert_eq_str failed: lengths differ\n\0", false);
            let fail_g = self.ir.module.add_global(fail_msg.get_type(), None, "aeqs_len_msg");
            fail_g.set_initializer(&fail_msg);
            fail_g.set_constant(true);
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[fail_g.as_pointer_value().into()], "");
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[i32_ty.const_int(1, false).into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);
            // same length — compare bytes via memcmp
            self.ir.builder.position_at_end(cmp_bb);
            let a_ptr = build_wrappers::w_extract_value(&self.ir.builder,a_struct, 1, "a_ptr").into_pointer_value();
            let b_ptr = build_wrappers::w_extract_value(&self.ir.builder,b_struct, 1, "b_ptr").into_pointer_value();
            let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = build_wrappers::w_call(&self.ir.builder,memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "cmp").try_as_basic_value().left().unwrap().into_int_value();
            let zero32 = i32_ty.const_zero();
            let bytes_eq = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, cmp_result, zero32, "bytes_eq");
            build_wrappers::w_cond_br(&self.ir.builder,bytes_eq, ok_bb, bytes_fail_bb);
            // bytes differ → fail
            self.ir.builder.position_at_end(bytes_fail_bb);
            let bytes_msg = self.ir.context.const_string(b"assert_eq_str failed: bytes differ\n\0", false);
            let bytes_g = self.ir.module.add_global(bytes_msg.get_type(), None, "aeqs_bytes_msg");
            bytes_g.set_initializer(&bytes_msg);
            bytes_g.set_constant(true);
            build_wrappers::w_call(&self.ir.builder,printf_fn, &[bytes_g.as_pointer_value().into()], "");
            build_wrappers::w_call(&self.ir.builder,exit_fn, &[i32_ty.const_int(1, false).into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);
            self.ir.builder.position_at_end(ok_bb);
            build_wrappers::w_ret_void(&self.ir.builder);
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("assert_eq_str".to_string(), fn_val);
            self.fn_return_types.insert("assert_eq_str".to_string(), Type::Unit);
        }

        // ── Phase 4: time builtins (sleep_ms / now_ms) ─────────────────────────
        // Migrated to the BUILTIN_EXTERNS registry (R1d slice 2) — see
        // builtin_externs.rs; declared by `declare_builtin_externs` above.

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
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "u_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, c.into(), 1, "u_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, i64_ty.const_zero().into(), 2, "u_src")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "ud_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, one.into(), 1, "ud_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, i64_ty.const_zero().into(), 2, "ud_src")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "uf_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, c.into(), 1, "uf_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, i64_ty.const_zero().into(), 2, "uf_src")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "udy_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, c.into(), 1, "udy_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, i64_ty.const_int(2, false).into(), 2, "udy_src")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "udyf_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, c.into(), 1, "udyf_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, i64_ty.const_int(2, false).into(), 2, "udyf_src")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                let now = build_wrappers::w_call(&self.ir.builder,now_fn, &[], "tn_now")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                let valid_until = build_wrappers::w_int_add(&self.ir.builder,now, horizon, "tn_valid_until");
                let one = f64_ty.const_float(1.0);
                let mut sv = tmp_ty.get_undef();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, v.into(), 0, "tn_val").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, one.into(), 1, "tn_conf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, horizon.into(), 2, "tn_hor").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, decay.into(), 3, "tn_decay").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, valid_until.into(), 4, "tn_vu")
                    .into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
                self.functions.insert("temporal_new".to_string(), fn_val);
                self.fn_return_types
                    .insert("temporal_new".to_string(), Type::Temporal(Box::new(Type::I64)));
            }

            // temporal_at(t: Temporal<i64>, offset_ms: i64) -> Temporal<i64>
            // Recompute confidence as c * (1 - decay)^(offset_ms / 86_400_000) —
            // the EXACT power form (matches the interpreter + the builtin doc).
            // Uses llvm.pow.f64 (already declared); a non-positive offset leaves
            // confidence unchanged. valid_until_ms is shifted by offset_ms.
            {
                // Reuse the already-declared llvm.pow.f64 (registered under the
                // Axon name "pow"); only declare it if missing (defensive).
                let pow_fn = self.functions.get("pow").copied()
                    .or_else(|| self.ir.module.get_function("llvm.pow.f64"))
                    .unwrap_or_else(|| {
                        let f2 = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
                        self.ir.module.add_function("llvm.pow.f64", f2, None)
                    });
                let fn_ty = tmp_ty.fn_type(&[tmp_ty.into(), i64_ty.into()], false);
                let fn_val = self.ir.module.add_function("temporal_at", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let t = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let offset_ms = fn_val.get_nth_param(1).unwrap().into_int_value();
                let conf = build_wrappers::w_extract_value(&self.ir.builder,t, 1, "ta_conf")
                    .into_float_value();
                let decay = build_wrappers::w_extract_value(&self.ir.builder,t, 3, "ta_decay")
                    .into_float_value();
                let valid_until = build_wrappers::w_extract_value(&self.ir.builder,t, 4, "ta_vu")
                    .into_int_value();
                // days = (f64) offset_ms / 86_400_000.0
                let offset_f = build_wrappers::w_signed_int_to_float(&self.ir.builder,offset_ms, f64_ty, "ta_offf");
                let day_ms = f64_ty.const_float(86_400_000.0);
                let days = build_wrappers::w_float_div(&self.ir.builder,offset_f, day_ms, "ta_days");
                let one = f64_ty.const_float(1.0);
                let zero = f64_ty.const_float(0.0);
                // base = max(0, 1 - decay); decayed = conf * base^days.
                let one_minus_decay = build_wrappers::w_float_sub(&self.ir.builder, one, decay, "ta_1md");
                let base_neg = build_wrappers::w_float_compare(&self.ir.builder, inkwell::FloatPredicate::OLT, one_minus_decay, zero, "ta_bneg");
                let base = build_wrappers::w_select(&self.ir.builder, base_neg, zero.into(), one_minus_decay.into(), "ta_base").into_float_value();
                let powed = build_wrappers::w_call(&self.ir.builder, pow_fn, &[base.into(), days.into()], "ta_pow")
                    .try_as_basic_value().left().unwrap().into_float_value();
                let decayed = build_wrappers::w_float_mul(&self.ir.builder, conf, powed, "ta_decayed");
                // Only decay for a positive offset; otherwise keep `conf`.
                let pos = build_wrappers::w_float_compare(&self.ir.builder, inkwell::FloatPredicate::OGT, days, zero, "ta_pos");
                let new_conf = build_wrappers::w_select(&self.ir.builder, pos, decayed.into(), conf.into(), "ta_nc").into_float_value();
                let new_valid = build_wrappers::w_int_add(&self.ir.builder,valid_until, offset_ms, "ta_nvu");
                // Build new struct, preserving value/horizon/decay.
                let mut sv = t;
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, new_conf.into(), 1, "ta_iconf").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, new_valid.into(), 4, "ta_ivu").into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, sv.into());
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
                let valid_until = build_wrappers::w_extract_value(&self.ir.builder,t, 4, "tiv_vu")
                    .into_int_value();
                let now = build_wrappers::w_call(&self.ir.builder,now_fn, &[], "tiv_now")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                let cmp = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::SLE, now, valid_until, "tiv_cmp");
                build_wrappers::w_ret(&self.ir.builder, cmp.into());
                self.functions.insert("temporal_is_valid".to_string(), fn_val);
                self.fn_return_types
                    .insert("temporal_is_valid".to_string(), Type::Bool);
            }

            // temporal_confidence(t: Temporal<i64>) -> f64 — extract field 1.
            {
                let fn_ty = f64_ty.fn_type(&[tmp_ty.into()], false);
                let fn_val = self.ir.module.add_function("temporal_confidence", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                let t = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let conf = build_wrappers::w_extract_value(&self.ir.builder, t, 1, "tc_conf");
                build_wrappers::w_ret(&self.ir.builder, conf);
                self.functions.insert("temporal_confidence".to_string(), fn_val);
                self.fn_return_types.insert("temporal_confidence".to_string(), Type::F64);
            }

            // Stub bodies for the legacy `uncertain_confidence` / `temporal_now`
            // helpers, so callers compile even when they predate the new API.
            // uncertain_confidence(confidence: f64) -> () (no-op)
            if self.ir.module.get_function("uncertain_confidence").is_none() {
                let fn_ty = self.ir.context.void_type().fn_type(&[f64_ty.into()], false);
                let fn_val = self.ir.module.add_function("uncertain_confidence", fn_ty, None);
                let bb = self.ir.context.append_basic_block(fn_val, "entry");
                self.ir.builder.position_at_end(bb);
                build_wrappers::w_ret_void(&self.ir.builder);
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
                let n = build_wrappers::w_call(&self.ir.builder,now_fn, &[], "tnow")
                    .try_as_basic_value()
                    .left()
                    .unwrap()
                    .into_int_value();
                build_wrappers::w_ret(&self.ir.builder, n.into());
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

            let len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "read_len");
            let ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "read_ptr");
            let ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,ptr_slot, i8_ptr_ptr, "ptrptr");
            build_wrappers::w_call(&self.ir.builder,rt_fn, &[len_slot.into(), ptr_slot_cast.into()], "");

            let len_val = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), len_slot, "len").into_int_value();
            let ptr_val = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), ptr_slot, "ptr").into_pointer_value();

            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, len_val.into(), 0, "str0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, ptr_val.into(), 1, "str1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

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
            let path_len = build_wrappers::w_extract_value(&self.ir.builder,path_str, 0, "rf_plen").into_int_value();
            let path_ptr_v = build_wrappers::w_extract_value(&self.ir.builder,path_str, 1, "rf_pptr").into_pointer_value();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "rf_out_len");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "rf_out_ptr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,out_ptr_slot, i8_ptr_ptr, "rf_ptrptr");
            build_wrappers::w_call(&self.ir.builder,rt_fn, &[path_ptr_v.into(), path_len.into(), out_len_slot.into(), out_ptr_cast.into()], "");

            let out_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_len_slot, "rf_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_ptr_slot, "rf_ptr").into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::SGE, out_len, zero_i64, "rf_is_ok");
            build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "rf_ok_slot");
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "rf_tag_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "rf_pay_ok");
            let str_ok_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_ok_ptr");
            let str_ok_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "rf_str_ok");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_ok_slot, 0, ""), out_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_ok_slot, 1, ""), out_ptr.into());
            let str_ok_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_ok_slot, "rf_str_ok_val");
            build_wrappers::w_store(&self.ir.builder,str_ok_ptr, str_ok_val);
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "rf_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = build_wrappers::w_int_neg(&self.ir.builder,out_len, "rf_actual_len");
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "rf_err_slot");
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "rf_tag_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "rf_pay_err");
            let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_err_ptr");
            let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "rf_str_err");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), actual_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), out_ptr.into());
            let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "rf_str_err_val");
            build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
            let err_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "rf_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("read_file".to_string(), fn_val);
        }

        // ── R6: exec(cmd: str, args: [str]) -> Result<str, str> ───────────────
        // Runtime: __axon_exec(cmd: AxonStr, args_ptr: *const AxonStr,
        //                      args_count: i64, out_len: *i64, out_ptr: **u8)
        // Same ±len Ok/Err convention as read_file. The `[str]` array is
        // {i64 len, ptr→[str struct]}; we pass its data ptr + len through.
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);
            // The `[str]` array is also {i64 len, ptr}.
            let arr_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            let rt_ty = void_ty.fn_type(
                &[str_ty.into(), i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.ir.module.add_function("__axon_exec", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into(), arr_ty.into()], false);
            let fn_val = self.ir.module.add_function("exec", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ex_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "ex_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "ex_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let cmd_str = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let args_arr = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let args_len = build_wrappers::w_extract_value(&self.ir.builder, args_arr, 0, "ex_alen").into_int_value();
            let args_data = build_wrappers::w_extract_value(&self.ir.builder, args_arr, 1, "ex_adata").into_pointer_value();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ex_out_len");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "ex_out_ptr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "ex_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[cmd_str.into(), args_data.into(), args_len.into(), out_len_slot.into(), out_ptr_cast.into()], "");

            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "ex_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "ex_ptr").into_pointer_value();
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGE, out_len, i64_ty.const_int(0, false), "ex_is_ok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "ex_ok_slot");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 0, "ex_tag_ok"), bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 1, "ex_pay_ok");
            let str_ok_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "ex_str_ok_ptr");
            let str_ok_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "ex_str_ok");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_ok_slot, 0, ""), out_len.into());
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_ok_slot, 1, ""), out_ptr.into());
            let str_ok_val = build_wrappers::w_load(&self.ir.builder, str_ty.into(), str_ok_slot, "ex_str_ok_val");
            build_wrappers::w_store(&self.ir.builder, str_ok_ptr, str_ok_val);
            let ok_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), ok_alloca, "ex_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = build_wrappers::w_int_neg(&self.ir.builder, out_len, "ex_actual_len");
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "ex_err_slot");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 0, "ex_tag_err"), bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 1, "ex_pay_err");
            let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "ex_str_err_ptr");
            let str_err_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "ex_str_err");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_err_slot, 0, ""), actual_len.into());
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_err_slot, 1, ""), out_ptr.into());
            let str_err_val = build_wrappers::w_load(&self.ir.builder, str_ty.into(), str_err_slot, "ex_str_err_val");
            build_wrappers::w_store(&self.ir.builder, str_err_ptr, str_err_val);
            let err_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), err_alloca, "ex_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("exec".to_string(), fn_val);
            self.fn_return_types.insert("exec".to_string(), Type::Result(Box::new(Type::Str), Box::new(Type::Str)));
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
            let path_len    = build_wrappers::w_extract_value(&self.ir.builder,path_str, 0, "wf_plen").into_int_value();
            let path_ptr_v  = build_wrappers::w_extract_value(&self.ir.builder,path_str, 1, "wf_pptr").into_pointer_value();
            let cont_len    = build_wrappers::w_extract_value(&self.ir.builder,content_str, 0, "wf_clen").into_int_value();
            let cont_ptr    = build_wrappers::w_extract_value(&self.ir.builder,content_str, 1, "wf_cptr").into_pointer_value();

            let err_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "wf_err_len");
            let err_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "wf_err_ptr");
            let err_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,err_ptr_slot, i8_ptr_ptr, "wf_ptrptr");
            build_wrappers::w_store(&self.ir.builder,err_len_slot, i64_ty.const_int(0, false).into());
            build_wrappers::w_store(&self.ir.builder,err_ptr_slot, i8_ptr.const_null().into());

            build_wrappers::w_call(&self.ir.builder,rt_fn, &[path_ptr_v.into(), path_len.into(), cont_ptr.into(), cont_len.into(), err_len_slot.into(), err_ptr_cast.into()], "");

            let err_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), err_len_slot, "wf_err_len_val").into_int_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::EQ, err_len, zero_i64, "wf_is_ok");
            build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=zeroed }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "wf_ok_slot");
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "wf_tag_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "wf_pay_ok");
            let zero_arr = self.ir.context.i8_type().array_type(16).const_zero();
            build_wrappers::w_store(&self.ir.builder,payload_ok, zero_arr.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "wf_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload=str{err_len, err_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let err_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), err_ptr_slot, "wf_err_ptr_val").into_pointer_value();
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "wf_err_slot");
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "wf_tag_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "wf_pay_err");
            let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "wf_str_err_ptr");
            let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "wf_str_err");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), err_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), err_ptr.into());
            let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "wf_str_err_val");
            build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
            let err_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "wf_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

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
            let cmp = build_wrappers::w_float_compare(&self.ir.builder,pred, a, b, "mf_cmp");
            let result = build_wrappers::w_select(&self.ir.builder,cmp, a.into(), b.into(), "mf_result");
            build_wrappers::w_ret(&self.ir.builder, result);
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::F64);
        }

        // clamp_i64 / clamp_f64 — now registry rows (R1d slice 1).

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
            let s_len = build_wrappers::w_extract_value(&self.ir.builder,s, 0, "pb_slen").into_int_value();
            let s_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "pb_sptr").into_pointer_value();

            // Check s == "true": len==4 && strncmp(s_ptr,"true",4)==0
            let len4 = i64_ty.const_int(4, false);
            let is_len4 = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ, s_len, len4, "pb_l4");
            let true_lit_g = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(5), None, "pb_true_lit");
            true_lit_g.set_initializer(&self.ir.context.const_string(b"true", true));
            true_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let true_lit = true_lit_g.as_pointer_value();
            let cmp_t = build_wrappers::w_call(&self.ir.builder,strncmp_fn,
                &[s_ptr.into(), true_lit.into(), self.msize(len4, "msz").into()], "pb_cmpt")
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_t_eq = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ, cmp_t,
                self.ir.context.i32_type().const_int(0, false), "pb_teq");
            let is_true_str = build_wrappers::w_and(&self.ir.builder,is_len4, cmp_t_eq, "pb_istrue");
            build_wrappers::w_cond_br(&self.ir.builder,is_true_str, ok_true_bb, check_f_bb);

            // check_f_bb: check s == "false": len==5 && strncmp(s_ptr,"false",5)==0
            self.ir.builder.position_at_end(check_f_bb);
            let len5 = i64_ty.const_int(5, false);
            let is_len5 = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ, s_len, len5, "pb_l5");
            let false_lit_g = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(6), None, "pb_false_lit");
            false_lit_g.set_initializer(&self.ir.context.const_string(b"false", true));
            false_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let false_lit = false_lit_g.as_pointer_value();
            let cmp_f = build_wrappers::w_call(&self.ir.builder,strncmp_fn,
                &[s_ptr.into(), false_lit.into(), self.msize(len5, "msz").into()], "pb_cmpf")
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_f_eq = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ, cmp_f,
                self.ir.context.i32_type().const_int(0, false), "pb_feq");
            let is_false_str = build_wrappers::w_and(&self.ir.builder,is_len5, cmp_f_eq, "pb_isfalse");
            build_wrappers::w_cond_br(&self.ir.builder,is_false_str, ok_false_bb, err_bb);

            // ok_true_bb: tag=1, payload = i1 true cast to [16 x i8]
            self.ir.builder.position_at_end(ok_true_bb);
            {
                let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pb_ot_slot");
                build_wrappers::w_store(&self.ir.builder,
                    build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "pb_ot_tag"),
                    i1_ty.const_int(1, false).into());
                let payload_ptr = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "pb_ot_pay");
                let bool_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_ot_bptr");
                build_wrappers::w_store(&self.ir.builder,bool_ptr, i1_ty.const_int(1, false).into());
                let val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "pb_ot_val");
                build_wrappers::w_ret(&self.ir.builder, val);
            }

            // ok_false_bb: tag=1, payload = i1 false cast to [16 x i8]
            self.ir.builder.position_at_end(ok_false_bb);
            {
                let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pb_of_slot");
                build_wrappers::w_store(&self.ir.builder,
                    build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "pb_of_tag"),
                    i1_ty.const_int(1, false).into());
                let payload_ptr = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "pb_of_pay");
                let bool_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_of_bptr");
                build_wrappers::w_store(&self.ir.builder,bool_ptr, i1_ty.const_int(0, false).into());
                let val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "pb_of_val");
                build_wrappers::w_ret(&self.ir.builder, val);
            }

            // err_bb: tag=0, payload = str{"invalid bool"} cast to [16 x i8]
            self.ir.builder.position_at_end(err_bb);
            {
                let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pb_err_slot");
                build_wrappers::w_store(&self.ir.builder,
                    build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "pb_err_tag"),
                    i1_ty.const_int(0, false).into());
                let payload_ptr = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "pb_err_pay");
                let str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,
                    payload_ptr, str_ty.ptr_type(inkwell::AddressSpace::default()), "pb_err_sptr");
                let err_str_alloca = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "pb_err_s");
                let err_msg = b"invalid bool";
                let err_lit_g = self.ir.module.add_global(
                    self.ir.context.i8_type().array_type(err_msg.len() as u32 + 1),
                    None, "pb_err_msg");
                err_lit_g.set_initializer(&self.ir.context.const_string(err_msg, true));
                err_lit_g.set_linkage(inkwell::module::Linkage::Private);
                let err_lit = err_lit_g.as_pointer_value();
                build_wrappers::w_store(&self.ir.builder,
                    build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_alloca, 0, "pb_esl"),
                    i64_ty.const_int(err_msg.len() as u64, false).into());
                build_wrappers::w_store(&self.ir.builder,
                    build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_alloca, 1, "pb_esp"),
                    err_lit.into());
                let err_str_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), err_str_alloca, "pb_esv");
                build_wrappers::w_store(&self.ir.builder,str_ptr, err_str_val);
                let val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "pb_err_val");
                build_wrappers::w_ret(&self.ir.builder, val);
            }

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_bool".to_string(), fn_val);
            self.fn_return_types.insert("parse_bool".to_string(),
                Type::Result(Box::new(Type::Bool), Box::new(Type::Str)));
        }

        // ── Phase 7: random_i64(lo: i64, hi: i64) -> i64 ─────────────────────
        // Uses C rand() % (hi - lo) + lo, with the SAME degenerate-bounds guard
        // the interpreter has (BUG_HUNT #27/#36, I-2 parity, I-9 no-silent-wrong):
        //   • hi <  lo → inverted bounds: print an error + exit(1) (matches the
        //     interpreter's graceful panic; previously yielded garbage).
        //   • hi == lo → empty range [lo, lo): return lo (previously a signed-rem
        //     by 0 → SIGFPE hard crash).
        //   • else      → lo + (rand() mod range), normalised non-negative.
        {
            let rand_fn = self.ir.module.get_function("rand").unwrap_or_else(|| {
                let ft = self.ir.context.i32_type().fn_type(&[], false);
                self.ir.module.add_function("rand", ft, None)
            });
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("random_i64", fn_ty, None);
            let entry_bb = self.ir.context.append_basic_block(fn_val, "ri_entry");
            let inverted_bb = self.ir.context.append_basic_block(fn_val, "ri_inverted");
            let chk_eq_bb = self.ir.context.append_basic_block(fn_val, "ri_chk_eq");
            let empty_bb = self.ir.context.append_basic_block(fn_val, "ri_empty");
            let gen_bb = self.ir.context.append_basic_block(fn_val, "ri_gen");
            let saved = self.ir.builder.get_insert_block();

            self.ir.builder.position_at_end(entry_bb);
            let lo = fn_val.get_nth_param(0).unwrap().into_int_value();
            let hi = fn_val.get_nth_param(1).unwrap().into_int_value();
            // hi < lo → inverted bounds branch.
            let is_inverted = build_wrappers::w_int_compare(
                &self.ir.builder, IntPredicate::SLT, hi, lo, "ri_inv",
            );
            build_wrappers::w_cond_br(&self.ir.builder, is_inverted, inverted_bb, chk_eq_bb);

            // Inverted bounds: print an error and exit(1) — loud failure, not a
            // silent garbage value (I-9). Mirrors the interpreter's panic.
            self.ir.builder.position_at_end(inverted_bb);
            let msg = b"random_i64: inverted bounds (lo must be <= hi); the range is [lo, hi)\n\0";
            let msg_const = self.ir.context.const_string(msg, false);
            let msg_global = self.ir.module.add_global(msg_const.get_type(), None, "random_i64_inv_msg");
            msg_global.set_initializer(&msg_const);
            msg_global.set_constant(true);
            build_wrappers::w_call(&self.ir.builder, printf_fn, &[msg_global.as_pointer_value().into()], "");
            let one = i32_ty.const_int(1, false);
            build_wrappers::w_call(&self.ir.builder, exit_fn, &[one.into()], "");
            build_wrappers::w_unreachable(&self.ir.builder);

            // hi == lo → empty range branch.
            self.ir.builder.position_at_end(chk_eq_bb);
            let is_empty = build_wrappers::w_int_compare(
                &self.ir.builder, IntPredicate::EQ, hi, lo, "ri_eq",
            );
            build_wrappers::w_cond_br(&self.ir.builder, is_empty, empty_bb, gen_bb);

            // Empty range [lo, lo): return lo (no rem-by-zero).
            self.ir.builder.position_at_end(empty_bb);
            build_wrappers::w_ret(&self.ir.builder, lo.into());

            // General case: lo + (rand() mod range), range = hi - lo > 0.
            self.ir.builder.position_at_end(gen_bb);
            let r_i32 = build_wrappers::w_call(&self.ir.builder,rand_fn, &[], "ri_rand")
                .try_as_basic_value().left().unwrap().into_int_value();
            let r = build_wrappers::w_int_s_extend(&self.ir.builder,r_i32, i64_ty, "ri_r64");
            let range = build_wrappers::w_int_sub(&self.ir.builder,hi, lo, "ri_range");
            let r_mod = build_wrappers::w_int_signed_rem(&self.ir.builder,r, range, "ri_mod");
            // Ensure non-negative: (r_mod + range) % range
            let r_pos = build_wrappers::w_int_add(&self.ir.builder,r_mod, range, "ri_pos");
            let r_final = build_wrappers::w_int_signed_rem(&self.ir.builder,r_pos, range, "ri_final");
            let result = build_wrappers::w_int_add(&self.ir.builder,r_final, lo, "ri_result");
            build_wrappers::w_ret(&self.ir.builder, result.into());

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
            let r_i32 = build_wrappers::w_call(&self.ir.builder,rand_fn, &[], "rf_rand")
                .try_as_basic_value().left().unwrap().into_int_value();
            // Convert to f64
            let r_f = build_wrappers::w_signed_int_to_float(&self.ir.builder,r_i32, f64_ty, "rf_f");
            // RAND_MAX = 2147483647 → divisor = 2147483648.0
            let divisor = f64_ty.const_float(2147483648.0);
            let result = build_wrappers::w_float_div(&self.ir.builder,r_f, divisor, "rf_result");
            build_wrappers::w_ret(&self.ir.builder, result.into());
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
            let s_ptr      = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "sc_sptr").into_pointer_value();
            let needle_len = build_wrappers::w_extract_value(&self.ir.builder,needle, 0, "sc_nlen").into_int_value();
            let needle_ptr = build_wrappers::w_extract_value(&self.ir.builder,needle, 1, "sc_nptr").into_pointer_value();
            let zero = i64_ty.const_zero();

            // Allocas here so they dominate all successors (including done_bb).
            let cur_slot   = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "sc_cur");
            let count_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "sc_cnt");
            build_wrappers::w_store(&self.ir.builder,cur_slot, s_ptr.into());
            build_wrappers::w_store(&self.ir.builder,count_slot, zero.into());

            let strstr_fn = self.ir.module.get_function("strstr").unwrap_or_else(|| {
                let t = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.ir.module.add_function("strstr", t, None)
            });

            let needle_empty = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ, needle_len, zero, "sc_nempty");
            build_wrappers::w_cond_br(&self.ir.builder,needle_empty, early_ret_bb, loop_bb);

            // ── early_ret: return 0 for empty needle ─────────────────────────
            self.ir.builder.position_at_end(early_ret_bb);
            build_wrappers::w_ret(&self.ir.builder, zero.into());

            // ── loop: cur = strstr(cur, needle); branch on null ──────────────
            self.ir.builder.position_at_end(loop_bb);
            let cur = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), cur_slot, "sc_cur_v").into_pointer_value();
            let found_ptr = build_wrappers::w_call(&self.ir.builder,
                strstr_fn, &[cur.into(), needle_ptr.into()], "sc_fp").try_as_basic_value().left().unwrap().into_pointer_value();
            let found_int = build_wrappers::w_ptr_to_int(&self.ir.builder,found_ptr, i64_ty, "sc_fpi");
            let null_int  = build_wrappers::w_ptr_to_int(&self.ir.builder,i8_ptr.const_null(), i64_ty, "sc_ni");
            let is_found = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::NE, found_int, null_int, "sc_isf");
            build_wrappers::w_cond_br(&self.ir.builder,is_found, found_bb, done_bb);

            // ── found: count++, advance cursor past the match ────────────────
            self.ir.builder.position_at_end(found_bb);
            let cnt = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), count_slot, "sc_cnt_v").into_int_value();
            let cnt1 = build_wrappers::w_int_add(&self.ir.builder,cnt, i64_ty.const_int(1, false), "sc_cnt1");
            build_wrappers::w_store(&self.ir.builder,count_slot, cnt1.into());
            let next = unsafe {
                build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), found_ptr, &[needle_len], "sc_next")
            };
            build_wrappers::w_store(&self.ir.builder,cur_slot, next.into());
            build_wrappers::w_br(&self.ir.builder,loop_bb);

            // ── done: return accumulated count ───────────────────────────────
            self.ir.builder.position_at_end(done_bb);
            let final_count = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), count_slot, "sc_final").into_int_value();
            build_wrappers::w_ret(&self.ir.builder, final_count.into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_count".to_string(), fn_val);
            self.fn_return_types.insert("str_count".to_string(), Type::I64);
        }


        // ── Phase 10: str_reverse(s: str) -> str ─────────────────────────────
        // BUG_HUNT #38: delegate to axon-rt's char-correct __axon_str_reverse
        // (reverses by Unicode scalar). The old inline body reversed BYTES,
        // mangling multibyte UTF-8 (str_reverse("héllo") → invalid bytes). The
        // interpreter is the oracle (I-2); the runtime now matches it.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            // Runtime: void __axon_str_reverse(AxonStr s, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function("__axon_str_reverse")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_str_reverse", rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_reverse", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "srev_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "srev_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "srev_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "srev_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "srev_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "srev_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "srev_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "srev_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

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
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "radix_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "radix_optr");
            // Cast *i8* → i8** for the runtime call.
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "radix_ptrptr");

            build_wrappers::w_call(&self.ir.builder,rt_fn, &[
                n.into(),
                base.into(),
                out_len_slot.into(),
                out_ptr_slot_cast.into(),
            ], "radix_call");

            let out_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_len_slot, "radix_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_ptr_slot, "radix_ptr").into_pointer_value();

            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, out_len.into(), 0, "radix_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, out_ptr.into(), 1, "radix_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

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

            // F11: __axon_provenance_log_ret_i64_in(name_ptr, name_len,
            //                                       input: i64, ret: i64)
            // logs (input, score) so goal_run can warm-start from the best input.
            let prov_i64_in_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i64_ty.into(), i64_ty.into()],
                false,
            );
            self.ir.module.add_function("__axon_provenance_log_ret_i64_in", prov_i64_in_ty, None);

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

            // BUG_HUNT #19: __axon_register_goal_name(name_ptr, name_len) — one
            // call per top-level fn in main's prologue (only when the program
            // calls goal_run), so native goal_run can reject a typo'd metric
            // name with the same panic the interpreter raises (I-9 parity)
            // instead of silently returning `target`.
            let reg_goal_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
            self.ir.module.add_function("__axon_register_goal_name", reg_goal_ty, None);

            // R4: __axon_set_provenance_source(path_ptr, path_len) — called once
            // in main's prologue to stamp the program's source path into native
            // @[adaptive] provenance (`"src"` field, interp parity).
            let set_src_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
            self.ir.module.add_function("__axon_set_provenance_source", set_src_ty, None);

            // R4 §4.3: __axon_log_agent_action(fn_ptr, fn_len, action_ptr,
            // action_len, caps_ptr, caps_len) — emitted at a capability builtin
            // call inside an @[agent] fn (the mandatory agent action log).
            let log_aa_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into()],
                false,
            );
            self.ir.module.add_function("__axon_log_agent_action", log_aa_ty, None);
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
            let name_len  = build_wrappers::w_extract_value(&self.ir.builder,name_str, 0, "gr_nlen").into_int_value();
            let name_ptr  = build_wrappers::w_extract_value(&self.ir.builder,name_str, 1, "gr_nptr").into_pointer_value();
            let target    = fn_val.get_nth_param(1).unwrap().into_float_value();
            let max_evals = fn_val.get_nth_param(2).unwrap().into_int_value();

            let null_ptr  = i8_ptr.const_null();
            let out_slot  = build_wrappers::w_alloca(&self.ir.builder,f64_ty.into(), "gr_out_score");
            build_wrappers::w_call(&self.ir.builder,
                rt_fn,
                &[
                    null_ptr.into(),
                    name_ptr.into(),
                    name_len.into(),
                    target.into(),
                    max_evals.into(),
                    out_slot.into(),
                ],
                "");

            let score = build_wrappers::w_load(&self.ir.builder,f64_ty.into(), out_slot, "gr_score");
            build_wrappers::w_ret(&self.ir.builder, score);

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
            let prompt_len   = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 0, "aic_plen").into_int_value();
            let prompt_ptr_v = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 1, "aic_pptr").into_pointer_value();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "aic_out_len");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "aic_out_ptr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,out_ptr_slot, i8_ptr_ptr, "aic_ptrptr");
            build_wrappers::w_call(&self.ir.builder,rt_fn, &[prompt_ptr_v.into(), prompt_len.into(), out_len_slot.into(), out_ptr_cast.into()], "");

            let out_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_len_slot, "aic_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_ptr_slot, "aic_ptr").into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::SGE, out_len, zero_i64, "aic_is_ok");
            build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "aic_ok_slot");
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "aic_tag_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "aic_pay_ok");
            let str_ok_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "aic_str_ok_ptr");
            let str_ok_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "aic_str_ok");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_ok_slot, 0, ""), out_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_ok_slot, 1, ""), out_ptr.into());
            let str_ok_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_ok_slot, "aic_str_ok_val");
            build_wrappers::w_store(&self.ir.builder,str_ok_ptr, str_ok_val);
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "aic_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = build_wrappers::w_int_neg(&self.ir.builder,out_len, "aic_actual_len");
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "aic_err_slot");
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "aic_tag_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "aic_pay_err");
            let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aic_str_err_ptr");
            let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "aic_str_err");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), actual_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), out_ptr.into());
            let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "aic_str_err_val");
            build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
            let err_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "aic_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

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
                let prompt_len   = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 0, "aei_plen").into_int_value();
                let prompt_ptr_v = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 1, "aei_pptr").into_pointer_value();

                let out_val_slot   = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "aei_out_val");
                let out_conf_slot  = build_wrappers::w_alloca(&self.ir.builder,f64_ty.into(), "aei_out_conf");
                let out_err_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "aei_out_err_len");
                let out_err_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "aei_out_err_ptr");
                let out_err_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,out_err_ptr_slot, i8_ptr_ptr, "aei_eptrptr");

                let rc_call = build_wrappers::w_call(&self.ir.builder,
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_conf_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aei_rc");
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, rc, zero_i32, "aei_is_ok");
                build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

                // ok_bb: build Uncertain<i64> { value, confidence, source_tag=1 }
                // and wrap in Result::Ok.
                self.ir.builder.position_at_end(ok_bb);
                let val = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_val_slot, "aei_val").into_int_value();
                let conf = build_wrappers::w_load(&self.ir.builder,f64_ty.into(), out_conf_slot, "aei_conf").into_float_value();
                let mut unc_sv = unc_i64_ty.get_undef();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, val.into(),  0, "aei_unc_v").into_struct_value();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, conf.into(), 1, "aei_unc_c").into_struct_value();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, i64_ty.const_int(1, false).into(), 2, "aei_unc_s")
                    .into_struct_value();
                let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_unc_i64_ty.into(), "aei_ok_slot");
                let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_i64_ty.into(), ok_alloca, 0, "aei_tag_ok");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
                let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_i64_ty.into(), ok_alloca, 1, "aei_pay_ok");
                let unc_payload_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, unc_i64_ty.ptr_type(inkwell::AddressSpace::default()), "aei_unc_pp");
                build_wrappers::w_store(&self.ir.builder,unc_payload_ptr, unc_sv.into());
                let ok_val = build_wrappers::w_load(&self.ir.builder,result_unc_i64_ty.into(), ok_alloca, "aei_ok_val");
                build_wrappers::w_ret(&self.ir.builder, ok_val);

                // err_bb: read err_len/err_ptr, build str payload, wrap Result::Err.
                self.ir.builder.position_at_end(err_bb);
                let err_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_err_len_slot, "aei_elen").into_int_value();
                let err_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_err_ptr_slot, "aei_eptr").into_pointer_value();
                let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_unc_i64_ty.into(), "aei_err_slot");
                let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_i64_ty.into(), err_alloca, 0, "aei_tag_err");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
                let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_i64_ty.into(), err_alloca, 1, "aei_pay_err");
                let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aei_str_err_pp");
                let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "aei_str_err");
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), err_len.into());
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), err_ptr.into());
                let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "aei_str_err_val");
                build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
                let err_val = build_wrappers::w_load(&self.ir.builder,result_unc_i64_ty.into(), err_alloca, "aei_err_val");
                build_wrappers::w_ret(&self.ir.builder, err_val);

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
                let prompt_len   = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 0, "aef_plen").into_int_value();
                let prompt_ptr_v = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 1, "aef_pptr").into_pointer_value();

                let out_val_slot   = build_wrappers::w_alloca(&self.ir.builder,f64_ty.into(), "aef_out_val");
                let out_conf_slot  = build_wrappers::w_alloca(&self.ir.builder,f64_ty.into(), "aef_out_conf");
                let out_err_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "aef_out_err_len");
                let out_err_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "aef_out_err_ptr");
                let out_err_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,out_err_ptr_slot, i8_ptr_ptr, "aef_eptrptr");

                let rc_call = build_wrappers::w_call(&self.ir.builder,
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_conf_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aef_rc");
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, rc, zero_i32, "aef_is_ok");
                build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

                // ok_bb
                self.ir.builder.position_at_end(ok_bb);
                let val = build_wrappers::w_load(&self.ir.builder,f64_ty.into(), out_val_slot, "aef_val").into_float_value();
                let conf = build_wrappers::w_load(&self.ir.builder,f64_ty.into(), out_conf_slot, "aef_conf").into_float_value();
                let mut unc_sv = unc_f64_ty.get_undef();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, val.into(),  0, "aef_unc_v").into_struct_value();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, conf.into(), 1, "aef_unc_c").into_struct_value();
                unc_sv = build_wrappers::w_insert_value(&self.ir.builder,unc_sv, i64_ty.const_int(1, false).into(), 2, "aef_unc_s")
                    .into_struct_value();
                let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_unc_f64_ty.into(), "aef_ok_slot");
                let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_f64_ty.into(), ok_alloca, 0, "aef_tag_ok");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
                let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_f64_ty.into(), ok_alloca, 1, "aef_pay_ok");
                let unc_payload_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, unc_f64_ty.ptr_type(inkwell::AddressSpace::default()), "aef_unc_pp");
                build_wrappers::w_store(&self.ir.builder,unc_payload_ptr, unc_sv.into());
                let ok_val = build_wrappers::w_load(&self.ir.builder,result_unc_f64_ty.into(), ok_alloca, "aef_ok_val");
                build_wrappers::w_ret(&self.ir.builder, ok_val);

                // err_bb
                self.ir.builder.position_at_end(err_bb);
                let err_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_err_len_slot, "aef_elen").into_int_value();
                let err_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_err_ptr_slot, "aef_eptr").into_pointer_value();
                let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_unc_f64_ty.into(), "aef_err_slot");
                let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_f64_ty.into(), err_alloca, 0, "aef_tag_err");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
                let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_unc_f64_ty.into(), err_alloca, 1, "aef_pay_err");
                let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aef_str_err_pp");
                let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "aef_str_err");
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), err_len.into());
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), err_ptr.into());
                let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "aef_str_err_val");
                build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
                let err_val = build_wrappers::w_load(&self.ir.builder,result_unc_f64_ty.into(), err_alloca, "aef_err_val");
                build_wrappers::w_ret(&self.ir.builder, err_val);

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
                let prompt_len   = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 0, "aex_plen").into_int_value();
                let prompt_ptr_v = build_wrappers::w_extract_value(&self.ir.builder,prompt_str, 1, "aex_pptr").into_pointer_value();

                let out_val_slot     = build_wrappers::w_alloca(&self.ir.builder,val_llvm_ty, "aex_out_val");
                let out_err_len_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(),     "aex_out_err_len");
                let out_err_ptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(),     "aex_out_err_ptr");
                let out_err_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,out_err_ptr_slot, i8_ptr_ptr, "aex_eptrptr");

                let rc_call = build_wrappers::w_call(&self.ir.builder,
                    rt_fn,
                    &[
                        prompt_ptr_v.into(),
                        prompt_len.into(),
                        out_val_slot.into(),
                        out_err_len_slot.into(),
                        out_err_ptr_cast.into(),
                    ],
                    "aex_rc");
                let rc = rc_call.try_as_basic_value().left().unwrap().into_int_value();
                let zero_i32 = i32_ty.const_int(0, false);
                let is_ok = build_wrappers::w_int_compare(&self.ir.builder,IntPredicate::EQ, rc, zero_i32, "aex_is_ok");
                build_wrappers::w_cond_br(&self.ir.builder,is_ok, ok_bb, err_bb);

                // ok_bb: load typed value, store into payload via a typed pointer
                // cast, set tag=1, return.
                self.ir.builder.position_at_end(ok_bb);
                let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_flat_ty.into(), "aex_ok_slot");
                let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_flat_ty.into(), ok_alloca, 0, "aex_tag_ok");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
                let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_flat_ty.into(), ok_alloca, 1, "aex_pay_ok");
                let typed_payload_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, val_ptr_ty, "aex_typed_pp");
                let val_loaded = build_wrappers::w_load(&self.ir.builder,val_llvm_ty, out_val_slot, "aex_val");
                build_wrappers::w_store(&self.ir.builder,typed_payload_ptr, val_loaded);
                let ok_val = build_wrappers::w_load(&self.ir.builder,result_flat_ty.into(), ok_alloca, "aex_ok_val");
                build_wrappers::w_ret(&self.ir.builder, ok_val);

                // err_bb: read err_len/err_ptr, build str payload, set tag=0, return.
                self.ir.builder.position_at_end(err_bb);
                let err_len = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), out_err_len_slot, "aex_elen").into_int_value();
                let err_ptr = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), out_err_ptr_slot, "aex_eptr").into_pointer_value();
                let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_flat_ty.into(), "aex_err_slot");
                let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_flat_ty.into(), err_alloca, 0, "aex_tag_err");
                build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
                let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_flat_ty.into(), err_alloca, 1, "aex_pay_err");
                let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "aex_str_err_pp");
                let str_err_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "aex_str_err");
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 0, ""), err_len.into());
                build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), str_err_slot, 1, ""), err_ptr.into());
                let str_err_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), str_err_slot, "aex_str_err_val");
                build_wrappers::w_store(&self.ir.builder,str_err_ptr, str_err_val);
                let err_val = build_wrappers::w_load(&self.ir.builder,result_flat_ty.into(), err_alloca, "aex_err_val");
                build_wrappers::w_ret(&self.ir.builder, err_val);

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
            let r = build_wrappers::w_signed_int_to_float(&self.ir.builder,n, f64_ty, "itf");
            build_wrappers::w_ret(&self.ir.builder, r.into());
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
            let r = build_wrappers::w_float_to_signed_int(&self.ir.builder,x, i64_ty, "fti");
            build_wrappers::w_ret(&self.ir.builder, r.into());
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("f64_to_i64".to_string(), fn_val);
            self.fn_return_types.insert("f64_to_i64".to_string(), Type::I64);
        }

        // abs_i64 / abs_f64 / sign_i64 / pow_i64 — now registry rows (R1d slice 1).

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
                let r = build_wrappers::w_call(&self.ir.builder,libm_fn, &[x.into()], "r")
                    .try_as_basic_value().left().unwrap().into_float_value();
                build_wrappers::w_ret(&self.ir.builder, r.into());
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
            let a_len = build_wrappers::w_extract_value(&self.ir.builder,a, 0, "se_alen").into_int_value();
            let b_len = build_wrappers::w_extract_value(&self.ir.builder,b, 0, "se_blen").into_int_value();
            let a_ptr = build_wrappers::w_extract_value(&self.ir.builder,a, 1, "se_aptr").into_pointer_value();
            let b_ptr = build_wrappers::w_extract_value(&self.ir.builder,b, 1, "se_bptr").into_pointer_value();

            // If lengths differ → false immediately.
            let lens_eq = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::EQ, a_len, b_len, "se_leneq");
            build_wrappers::w_cond_br(&self.ir.builder,lens_eq, cmp_bb, false_bb);

            // Same length → call memcmp.
            self.ir.builder.position_at_end(cmp_bb);
            let memcmp_fn = self.ir.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = build_wrappers::w_call(&self.ir.builder,memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "se_cmp")
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::EQ, cmp_result, i32_ty.const_int(0, false), "se_iszero");
            build_wrappers::w_cond_br(&self.ir.builder,is_zero, true_bb, false_bb);

            self.ir.builder.position_at_end(true_bb);
            build_wrappers::w_ret(&self.ir.builder, bool_ty.const_int(1, false).into());

            self.ir.builder.position_at_end(false_bb);
            build_wrappers::w_ret(&self.ir.builder, bool_ty.const_int(0, false).into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_eq".to_string(), fn_val);
            self.fn_return_types.insert("str_eq".to_string(), Type::Bool);
        }

        // str_contains / str_starts_with / str_ends_with — now registry rows (R1d slice 1).

        // ── str_slice(s: str, start: i64, end: i64) -> str ──
        // Migrated to axon-rt (R1 Batch 2b): out-param ABI via __axon_str_slice.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            // Runtime: void __axon_str_slice(AxonStr s, i64 start, i64 end, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ty.into(), i64_ty.into(),
                i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function("__axon_str_slice")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_str_slice", rt_fn_ty, None));

            // Axon-side wrapper: str_slice(str, i64, i64) -> str
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_slice", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let start_arg = fn_val.get_nth_param(1).unwrap().into_int_value();
            let end_arg = fn_val.get_nth_param(2).unwrap().into_int_value();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sslice_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "sslice_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "sslice_ptrptr");

            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), start_arg.into(), end_arg.into(),
                out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "sslice_call");

            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "sslice_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "sslice_ptr").into_pointer_value();

            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "sslice_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "sslice_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_slice".to_string(), fn_val);
            self.fn_return_types.insert("str_slice".to_string(), Type::Str);
        }

        // str_index_of / char_at — now registry rows (R1d slice 1).

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
            let is_true = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::NE, b, bool_ty.const_int(0, false), "tsb_cond");
            build_wrappers::w_cond_br(&self.ir.builder,is_true, true_bb, false_bb);

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
            let true_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,true_g.as_pointer_value(), i8_ptr, "tsb_tptr");
            let mut true_str = str_ty.get_undef();
            true_str = build_wrappers::w_insert_value(&self.ir.builder,true_str, i64_ty.const_int(4, false).into(), 0, "tsb_t0").into_struct_value();
            true_str = build_wrappers::w_insert_value(&self.ir.builder,true_str, true_ptr.into(), 1, "tsb_t1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, true_str.into());

            self.ir.builder.position_at_end(false_bb);
            let false_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,false_g.as_pointer_value(), i8_ptr, "tsb_fptr");
            let mut false_str = str_ty.get_undef();
            false_str = build_wrappers::w_insert_value(&self.ir.builder,false_str, i64_ty.const_int(5, false).into(), 0, "tsb_f0").into_struct_value();
            false_str = build_wrappers::w_insert_value(&self.ir.builder,false_str, false_ptr.into(), 1, "tsb_f1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, false_str.into());

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
            let data_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "pf_data").into_pointer_value();

            let endptr_slot = build_wrappers::w_alloca(&self.ir.builder,i8_ptr.into(), "pf_endptr");
            build_wrappers::w_store(&self.ir.builder,endptr_slot, i8_ptr.const_null().into());
            let endptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder,endptr_slot, i8_ptr_ptr, "pf_endptr_cast");

            let parsed_f64 = build_wrappers::w_call(&self.ir.builder,strtod_fn, &[data_ptr.into(), endptr_cast.into()], "pf_strtod")
                .try_as_basic_value().left().unwrap().into_float_value();

            let endptr_val = build_wrappers::w_load(&self.ir.builder,i8_ptr.into(), endptr_slot, "pf_endptr_val").into_pointer_value();
            let endptr_int = build_wrappers::w_ptr_to_int(&self.ir.builder,endptr_val, i64_ty, "pf_ep_int");
            let data_int   = build_wrappers::w_ptr_to_int(&self.ir.builder,data_ptr, i64_ty, "pf_data_int");
            let consumed   = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::NE, endptr_int, data_int, "pf_consumed");
            build_wrappers::w_cond_br(&self.ir.builder,consumed, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=f64 as [16 x i8] }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pf_ok_slot");
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "pf_tag_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "pf_pay_ok");
            let f64_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, f64_ty.ptr_type(inkwell::AddressSpace::default()), "pf_f64_ptr");
            build_wrappers::w_store(&self.ir.builder,f64_ptr, parsed_f64.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "pf_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload=str{len=0, ptr=null} }
            self.ir.builder.position_at_end(err_bb);
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pf_err_slot");
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "pf_tag_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "pf_pay_err");
            let err_str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "pf_str_err_ptr");
            let err_str_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "pf_str_err");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_slot, 0, ""), i64_ty.const_int(0, false).into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_slot, 1, ""), i8_ptr.const_null().into());
            let err_str_val = build_wrappers::w_load(&self.ir.builder,str_ty.into(), err_str_slot, "pf_err_str_val");
            build_wrappers::w_store(&self.ir.builder,err_str_ptr, err_str_val);
            let err_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_alloca, "pf_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_float".to_string(), fn_val);
            self.fn_return_types.insert("parse_float".to_string(),
                Type::Result(Box::new(Type::F64), Box::new(Type::Str)));
        }

        // ── parse-with-default wrappers: parse_int_or / parse_float_or /
        //    parse_bool_or ────────────────────────────────────────────────────
        // These fold the Result-match ceremony away: `parse_int_or(s, d)` is
        // `match parse_int(s) { Ok(n) => n, Err(_) => d }`. They existed in the
        // interpreter but had NO codegen lowering, so native silently returned a
        // zero value (a real native↔interp divergence). Each is built here as a
        // thin LLVM fn that calls the already-registered Result-returning parser,
        // reads the i1 tag, and `select`s between the Ok payload and the default.
        // The payload is read by storing the Result to an alloca and loading the
        // value slot at the right scalar type (tag=1 ⇒ Ok). str_ty is the {i64,
        // i8*} AxonStr used as the first param of each parser.
        {
            use inkwell::types::BasicType;
            let f64_ty = self.ir.context.f64_type();
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            // Build one parse-or wrapper. `parser_name` is the Result-returning
            // builtin; `val_ty` is the Ok payload's scalar LLVM type and the
            // default-arg / return type.
            let build_or = |slf: &mut Self,
                                or_name: &str,
                                parser_name: &str,
                                val_ty: inkwell::types::BasicTypeEnum<'ctx>,
                                ret_sem: Type| {
                let parser = match slf.functions.get(parser_name).copied() {
                    Some(p) => p,
                    None => return,
                };
                let result_ty = parser.get_type().get_return_type().unwrap();
                let fn_ty = val_ty.fn_type(&[str_ty.into(), val_ty.into()], false);
                let fn_val = slf.ir.module.add_function(or_name, fn_ty, None);
                let bb = slf.ir.context.append_basic_block(fn_val, "po_entry");
                let saved = slf.ir.builder.get_insert_block();
                slf.ir.builder.position_at_end(bb);

                let s_arg = fn_val.get_nth_param(0).unwrap();
                let default_arg = fn_val.get_nth_param(1).unwrap();
                let ok_bb = slf.ir.context.append_basic_block(fn_val, "po_ok");
                let def_bb = slf.ir.context.append_basic_block(fn_val, "po_def");
                // r = parser(s)
                let r = build_wrappers::w_call(&slf.ir.builder, parser, &[s_arg.into()], "po_r")
                    .try_as_basic_value().left().unwrap();
                // Store to an alloca so we can read the tag and the payload slot.
                let r_slot = build_wrappers::w_alloca(&slf.ir.builder, result_ty, "po_rslot");
                build_wrappers::w_store(&slf.ir.builder, r_slot, r);
                // tag = *(i1*)&r.0  (field 0); branch Ok vs default.
                let tag_ptr = build_wrappers::w_struct_gep(&slf.ir.builder, result_ty, r_slot, 0, "po_tagp");
                let tag = build_wrappers::w_load(&slf.ir.builder, slf.ir.context.bool_type().into(), tag_ptr, "po_tag")
                    .into_int_value();
                build_wrappers::w_cond_br(&slf.ir.builder, tag, ok_bb, def_bb);

                // ok_bb: return the parsed Ok payload. The parsers store the Ok
                // value at the start of the payload array at its natural type
                // (i64 / f64 / i1), so a same-typed load reads it back.
                slf.ir.builder.position_at_end(ok_bb);
                let pay_ptr = build_wrappers::w_struct_gep(&slf.ir.builder, result_ty, r_slot, 1, "po_payp");
                let pay_val_ptr = build_wrappers::w_pointer_cast(
                    &slf.ir.builder, pay_ptr,
                    val_ty.ptr_type(inkwell::AddressSpace::default()), "po_payvp");
                let ok_val = build_wrappers::w_load(&slf.ir.builder, val_ty, pay_val_ptr, "po_okv");
                build_wrappers::w_ret(&slf.ir.builder, ok_val);

                // def_bb: return the caller's default.
                slf.ir.builder.position_at_end(def_bb);
                build_wrappers::w_ret(&slf.ir.builder, default_arg);

                if let Some(b) = saved { slf.ir.builder.position_at_end(b); }
                slf.functions.insert(or_name.to_string(), fn_val);
                slf.fn_return_types.insert(or_name.to_string(), ret_sem);
            };

            build_or(self, "parse_int_or", "parse_int", i64_ty.into(), Type::I64);
            build_or(self, "parse_float_or", "parse_float", f64_ty.into(), Type::F64);
            // parse_bool_or is NOT built here as a hand-emitted fn — its i1 (bool)
            // default *parameter* read back as 0 across the call boundary (an ABI
            // corner the i64/f64 wrappers don't hit). It is instead lowered INLINE
            // at the call site (`emit_call`, search "parse_bool_or"), where the i1
            // default stays an SSA value in the caller's frame — no cross-function
            // i1 param. That form is native==interp==wasm.
            let _ = bool_ty;
        }

        // abs_i64 / min_i64 / max_i64 — now registry rows (R1d slice 1).

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
            let s_len = build_wrappers::w_extract_value(&self.ir.builder,s, 0, "stl_len").into_int_value();
            let s_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "stl_ptr").into_pointer_value();
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", ft, None)
            });
            let alloc_size = build_wrappers::w_int_add(&self.ir.builder,s_len, i64_ty.const_int(1, false), "stl_sz");
            let buf = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_size, "msz").into()], "stl_buf")
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let i_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "stl_i");
            build_wrappers::w_store(&self.ir.builder,i_slot, i64_ty.const_zero().into());
            build_wrappers::w_br(&self.ir.builder,loop_bb);

            // ── loop: if i < s_len goto body else done ─────────────────────
            self.ir.builder.position_at_end(loop_bb);
            let i_val = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), i_slot, "stl_iv").into_int_value();
            let in_range = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::SLT, i_val, s_len, "stl_cmp");
            build_wrappers::w_cond_br(&self.ir.builder,in_range, body_bb, done_bb);

            // ── body: convert byte, store, increment i ─────────────────────
            self.ir.builder.position_at_end(body_bb);
            let src_gep = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), s_ptr, &[i_val], "stl_src") };
            let byte = build_wrappers::w_load(&self.ir.builder,self.ir.context.i8_type().into(), src_gep, "stl_byte").into_int_value();
            let converted = if *is_upper {
                // toupper: if byte in 'a'..'z' => byte - 32
                let lo = self.ir.context.i8_type().const_int(b'a' as u64, false);
                let hi = self.ir.context.i8_type().const_int(b'z' as u64, false);
                let is_lo = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::UGE, byte, lo, "stl_uge");
                let is_hi = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::ULE, byte, hi, "stl_ule");
                let in_range_c = build_wrappers::w_and(&self.ir.builder,is_lo, is_hi, "stl_islc");
                let sub32 = build_wrappers::w_int_sub(&self.ir.builder,byte, self.ir.context.i8_type().const_int(32, false), "stl_sub");
                build_wrappers::w_select(&self.ir.builder,in_range_c, sub32.into(), byte.into(), "stl_sel").into_int_value()
            } else {
                // tolower: if byte in 'A'..'Z' => byte + 32
                let lo = self.ir.context.i8_type().const_int(b'A' as u64, false);
                let hi = self.ir.context.i8_type().const_int(b'Z' as u64, false);
                let is_lo = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::UGE, byte, lo, "stl_uge");
                let is_hi = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::ULE, byte, hi, "stl_ule");
                let in_range_c = build_wrappers::w_and(&self.ir.builder,is_lo, is_hi, "stl_isuc");
                let add32 = build_wrappers::w_int_add(&self.ir.builder,byte, self.ir.context.i8_type().const_int(32, false), "stl_add");
                build_wrappers::w_select(&self.ir.builder,in_range_c, add32.into(), byte.into(), "stl_sel").into_int_value()
            };
            let dst_gep = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[i_val], "stl_dst") };
            build_wrappers::w_store(&self.ir.builder,dst_gep, converted.into());
            let next_i = build_wrappers::w_int_add(&self.ir.builder,i_val, i64_ty.const_int(1, false), "stl_ni");
            build_wrappers::w_store(&self.ir.builder,i_slot, next_i.into());
            build_wrappers::w_br(&self.ir.builder,loop_bb);

            // ── done: null-terminate and return ───────────────────────────
            self.ir.builder.position_at_end(done_bb);
            let null_gep = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[s_len], "stl_null") };
            build_wrappers::w_store(&self.ir.builder,null_gep, self.ir.context.i8_type().const_zero().into());
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, s_len.into(), 0, "stl_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder,result, buf.into(), 1, "stl_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
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
                let orig_len = build_wrappers::w_extract_value(&self.ir.builder,s, 0, "stt_olen").into_int_value();
                let orig_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "stt_optr").into_pointer_value();

                // start = 0, end = orig_len
                let start_slot = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "stt_start");
                let end_slot   = build_wrappers::w_alloca(&self.ir.builder,i64_ty.into(), "stt_end");
                build_wrappers::w_store(&self.ir.builder,start_slot, i64_ty.const_zero().into());
                build_wrappers::w_store(&self.ir.builder,end_slot, orig_len.into());

                let space_threshold = self.ir.context.i8_type().const_int(32, false);

                if *do_start {
                    // while start < end && orig_ptr[start] <= 32: start++
                    let ts_cond = self.ir.context.append_basic_block(fn_val, "stt_sc");
                    let ts_body = self.ir.context.append_basic_block(fn_val, "stt_sb");
                    let ts_done = self.ir.context.append_basic_block(fn_val, "stt_sd");
                    build_wrappers::w_br(&self.ir.builder,ts_cond);
                    self.ir.builder.position_at_end(ts_cond);
                    let cur_start = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), start_slot, "stt_cs").into_int_value();
                    let cur_end   = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), end_slot, "stt_ce").into_int_value();
                    let in_range  = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::SLT, cur_start, cur_end, "stt_ir");
                    // check byte
                    let byte_ptr = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), orig_ptr, &[cur_start], "stt_bp") };
                    let byte_val = build_wrappers::w_load(&self.ir.builder,self.ir.context.i8_type().into(), byte_ptr, "stt_bv").into_int_value();
                    let is_space = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_isp");
                    let should_skip = build_wrappers::w_and(&self.ir.builder,in_range, is_space, "stt_skip");
                    build_wrappers::w_cond_br(&self.ir.builder,should_skip, ts_body, ts_done);
                    self.ir.builder.position_at_end(ts_body);
                    let next_start = build_wrappers::w_int_add(&self.ir.builder,cur_start, i64_ty.const_int(1, false), "stt_ns");
                    build_wrappers::w_store(&self.ir.builder,start_slot, next_start.into());
                    build_wrappers::w_br(&self.ir.builder,ts_cond);
                    self.ir.builder.position_at_end(ts_done);
                }

                if *do_end {
                    // while end > start && orig_ptr[end-1] <= 32: end--
                    let te_cond = self.ir.context.append_basic_block(fn_val, "stt_ec");
                    let te_body = self.ir.context.append_basic_block(fn_val, "stt_eb");
                    let te_done = self.ir.context.append_basic_block(fn_val, "stt_ed");
                    build_wrappers::w_br(&self.ir.builder,te_cond);
                    self.ir.builder.position_at_end(te_cond);
                    let cur_start = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), start_slot, "stt_ecs").into_int_value();
                    let cur_end   = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), end_slot, "stt_ece").into_int_value();
                    let in_range  = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::SGT, cur_end, cur_start, "stt_eir");
                    let prev_idx  = build_wrappers::w_int_sub(&self.ir.builder,cur_end, i64_ty.const_int(1, false), "stt_pi");
                    let byte_ptr  = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), orig_ptr, &[prev_idx], "stt_ebp") };
                    let byte_val  = build_wrappers::w_load(&self.ir.builder,self.ir.context.i8_type().into(), byte_ptr, "stt_ebv").into_int_value();
                    let is_space  = build_wrappers::w_int_compare(&self.ir.builder,inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_eisp");
                    let should_trim = build_wrappers::w_and(&self.ir.builder,in_range, is_space, "stt_etrim");
                    build_wrappers::w_cond_br(&self.ir.builder,should_trim, te_body, te_done);
                    self.ir.builder.position_at_end(te_body);
                    let next_end = build_wrappers::w_int_sub(&self.ir.builder,cur_end, i64_ty.const_int(1, false), "stt_ne");
                    build_wrappers::w_store(&self.ir.builder,end_slot, next_end.into());
                    build_wrappers::w_br(&self.ir.builder,te_cond);
                    self.ir.builder.position_at_end(te_done);
                }

                // new_start, new_end computed; new_len = end - start
                let final_start = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), start_slot, "stt_fs").into_int_value();
                let final_end   = build_wrappers::w_load(&self.ir.builder,i64_ty.into(), end_slot, "stt_fe").into_int_value();
                let new_len = build_wrappers::w_int_sub(&self.ir.builder,final_end, final_start, "stt_nl");

                // malloc(new_len + 1)
                let alloc_sz = build_wrappers::w_int_add(&self.ir.builder,new_len, i64_ty.const_int(1, false), "stt_az");
                let buf = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_sz, "msz").into()], "stt_buf")
                    .try_as_basic_value().left().unwrap().into_pointer_value();
                // memcpy(buf, orig_ptr+start, new_len)
                let src_ptr = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), orig_ptr, &[final_start], "stt_src") };
                build_wrappers::w_call(&self.ir.builder,memcpy_fn, &[buf.into(), src_ptr.into(), self.msize(new_len, "msz").into()], "stt_cpy");
                // null-terminate
                let null_gep = unsafe { build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[new_len], "stt_nul") };
                build_wrappers::w_store(&self.ir.builder,null_gep, self.ir.context.i8_type().const_zero().into());

                // return str { new_len, buf }
                let mut result = str_ty.const_zero();
                result = build_wrappers::w_insert_value(&self.ir.builder,result, new_len.into(), 0, "stt_r0").into_struct_value();
                result = build_wrappers::w_insert_value(&self.ir.builder,result, buf.into(), 1, "stt_r1").into_struct_value();
                build_wrappers::w_ret(&self.ir.builder, result.into());
                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
                self.functions.insert(fname.to_string(), fn_val);
                self.fn_return_types.insert(fname.to_string(), Type::Str);
            }
        }

        // ── Phase 6: str_repeat(s: str, n: i64) -> str ───────────
        // Migrated to axon-rt (R1 Batch 2b): out-param ABI via __axon_str_repeat.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            // Runtime: void __axon_str_repeat(AxonStr s, i64 n, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function("__axon_str_repeat")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_str_repeat", rt_fn_ty, None));

            // Axon-side wrapper: str_repeat(str, i64) -> str
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_repeat", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let n_arg = fn_val.get_nth_param(1).unwrap().into_int_value();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "srep_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "srep_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "srep_ptrptr");

            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), n_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "srep_call");

            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "srep_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "srep_ptr").into_pointer_value();

            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "srep_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "srep_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_repeat".to_string(), fn_val);
            self.fn_return_types.insert("str_repeat".to_string(), Type::Str);
        }

        // ── Phase 6: str_replace(s: str, from: str, to: str) -> str ──────────
        // BUG_HUNT #39: delegate to axon-rt's __axon_str_replace (Rust
        // str::replace semantics). The old inline body SKIPPED the empty-`from`
        // case (returned `s` unchanged), but the interpreter (oracle, I-2)
        // interleaves `to` between every char: str_replace("abc","","X") →
        // "XaXbXcX". The runtime now matches the interpreter.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            // Runtime: void __axon_str_replace(AxonStr s, AxonStr from, AxonStr to, i64* out_len, i8** out_ptr)
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), str_ty.into(), str_ty.into(),
                i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function("__axon_str_replace")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_str_replace", rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into(), str_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_replace", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg    = fn_val.get_nth_param(0).unwrap();
            let from_arg = fn_val.get_nth_param(1).unwrap();
            let to_arg   = fn_val.get_nth_param(2).unwrap();

            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "srpl_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "srpl_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "srpl_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), from_arg.into(), to_arg.into(),
                out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "srpl_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "srpl_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "srpl_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "srpl_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "srpl_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

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
            let name_ptr = build_wrappers::w_extract_value(&self.ir.builder,name_s, 1, "ev_np").into_pointer_value();

            let val_ptr = build_wrappers::w_call(&self.ir.builder,getenv_fn, &[name_ptr.into()], "ev_val")
                .try_as_basic_value().left().unwrap().into_pointer_value();

            let null_ptr = i8_ptr.const_null();
            let is_null = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::EQ,
                build_wrappers::w_ptr_to_int(&self.ir.builder,val_ptr, i64_ty, "ev_vi"),
                build_wrappers::w_ptr_to_int(&self.ir.builder,null_ptr, i64_ty, "ev_ni"),
                "ev_isnull"
            );
            build_wrappers::w_cond_br(&self.ir.builder,is_null, err_bb, ok_bb);

            // Ok branch: return { tag=1, payload=str{strlen(val_ptr), val_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let val_len_raw = build_wrappers::w_call(&self.ir.builder,strlen_fn, &[val_ptr.into()], "ev_vlen")
                .try_as_basic_value().left().unwrap().into_int_value();
            // strlen returns size_t (i32 on wasm32); the AxonStr len field is
            // i64 — widen before storing.
            let val_len = self.zext_size_to_i64(val_len_raw, "ev_vlen64");
            let ok_str_ptr = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "ev_ok_r");
            let tag_gep = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_str_ptr, 0, "ev_tag");
            build_wrappers::w_store(&self.ir.builder,tag_gep, bool_ty.const_int(1, false).into());
            let payload_gep = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_str_ptr, 1, "ev_pay");
            let payload_as_str = build_wrappers::w_pointer_cast(&self.ir.builder,payload_gep, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr");
            let ok_str = {
                let mut sv = str_ty.const_zero();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, val_len.into(), 0, "ev_sv0").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, val_ptr.into(), 1, "ev_sv1").into_struct_value();
                sv
            };
            build_wrappers::w_store(&self.ir.builder,payload_as_str, ok_str.into());
            let ok_result = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_str_ptr, "ev_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_result);

            // Err branch: return { tag=0, payload=str{"not set"} }
            self.ir.builder.position_at_end(err_bb);
            let err_msg = "not set\0";
            let err_global = self.ir.module.add_global(
                self.ir.context.i8_type().array_type(err_msg.len() as u32), None, "ev_err_str"
            );
            let err_bytes: Vec<_> = err_msg.bytes().map(|c| self.ir.context.i8_type().const_int(c as u64, false)).collect();
            err_global.set_initializer(&self.ir.context.i8_type().const_array(&err_bytes));
            err_global.set_constant(true);
            let err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,
                err_global.as_pointer_value(), i8_ptr, "ev_eptr"
            );
            let err_str_ptr = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "ev_err_r");
            let tag_gep2 = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_str_ptr, 0, "ev_etag");
            build_wrappers::w_store(&self.ir.builder,tag_gep2, bool_ty.const_int(0, false).into());
            let payload_gep2 = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_str_ptr, 1, "ev_epay");
            let payload_as_str2 = build_wrappers::w_pointer_cast(&self.ir.builder,payload_gep2, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr2");
            let err_str = {
                let err_len = i64_ty.const_int((err_msg.len() - 1) as u64, false); // exclude null
                let mut sv = str_ty.const_zero();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, err_len.into(), 0, "ev_es0").into_struct_value();
                sv = build_wrappers::w_insert_value(&self.ir.builder,sv, err_ptr.into(), 1, "ev_es1").into_struct_value();
                sv
            };
            build_wrappers::w_store(&self.ir.builder,payload_as_str2, err_str.into());
            let err_result = build_wrappers::w_load(&self.ir.builder,result_ty.into(), err_str_ptr, "ev_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_result);

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
            let code_i32 = build_wrappers::w_int_truncate(&self.ir.builder,code, self.ir.context.i32_type(), "ex_code");
            build_wrappers::w_call(&self.ir.builder,c_exit_fn, &[code_i32.into()], "ex_call");
            build_wrappers::w_unreachable(&self.ir.builder);
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            // Register as "exit" — this is what the Axon source calls
            self.functions.insert("exit".to_string(), fn_val);
            self.fn_return_types.insert("exit".to_string(), Type::Unit);
        }

        // str_len — now a registry row (R1d slice 1).

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

            let s_len = build_wrappers::w_extract_value(&self.ir.builder,s, 0, "sp_slen").into_int_value();
            let s_ptr = build_wrappers::w_extract_value(&self.ir.builder,s, 1, "sp_sptr").into_pointer_value();
            let fill_ptr = build_wrappers::w_extract_value(&self.ir.builder,fill, 1, "sp_fptr").into_pointer_value();

            // if s_len >= width: return s as-is
            let need_pad = build_wrappers::w_int_compare(&self.ir.builder,
                inkwell::IntPredicate::SLT, s_len, width, "sp_need");
            build_wrappers::w_cond_br(&self.ir.builder,need_pad, pad_bb, done_bb);

            // pad_bb: allocate width+1 bytes, fill pad chars, copy s, null-terminate
            self.ir.builder.position_at_end(pad_bb);
            let pad_len = build_wrappers::w_int_sub(&self.ir.builder,width, s_len, "sp_padlen");
            let alloc_size = build_wrappers::w_int_add(&self.ir.builder,width, i64_ty.const_int(1, false), "sp_alloc");
            let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("malloc", malloc_ty, None)
            });
            let memcpy_fn = self.ir.module.get_function("memcpy").unwrap_or_else(|| {
                let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.ir.module.add_function("memcpy", memcpy_ty, None)
            });
            let buf = build_wrappers::w_call(&self.ir.builder,malloc_fn, &[self.msize(alloc_size, "msz").into()], "sp_buf")
                .try_as_basic_value().left().unwrap().into_pointer_value();
            // fill_char = fill_ptr[0]
            let fill_char = build_wrappers::w_load(&self.ir.builder,self.ir.context.i8_type().into(), fill_ptr, "sp_fchar").into_int_value();
            // Use memset (declare if needed)
            let memset_fn = self.ir.module.get_function("memset").unwrap_or_else(|| {
                let memset_ty = i8_ptr.fn_type(
                    &[i8_ptr.into(), self.ir.context.i32_type().into(), i64_ty.into()], false);
                self.ir.module.add_function("memset", memset_ty, None)
            });
            let fill_char_i32 = build_wrappers::w_int_z_extend(&self.ir.builder,fill_char, self.ir.context.i32_type(), "sp_fc32");
            if *pad_start {
                // Pad bytes at start, then s
                build_wrappers::w_call(&self.ir.builder,memset_fn, &[buf.into(), fill_char_i32.into(), self.msize(pad_len, "msz").into()], "");
                let s_dest = unsafe {
                    build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[pad_len], "sp_sdest")
                };
                build_wrappers::w_call(&self.ir.builder,memcpy_fn, &[s_dest.into(), s_ptr.into(), self.msize(s_len, "msz").into()], "");
            } else {
                // s then pad bytes
                build_wrappers::w_call(&self.ir.builder,memcpy_fn, &[buf.into(), s_ptr.into(), self.msize(s_len, "msz").into()], "");
                let pad_dest = unsafe {
                    build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[s_len], "sp_pdest")
                };
                build_wrappers::w_call(&self.ir.builder,memset_fn, &[pad_dest.into(), fill_char_i32.into(), self.msize(pad_len, "msz").into()], "");
            }
            // null-terminate
            let null_pos = unsafe {
                build_wrappers::w_gep(&self.ir.builder,self.ir.context.i8_type().into(), buf, &[width], "sp_null")
            };
            build_wrappers::w_store(&self.ir.builder,null_pos, self.ir.context.i8_type().const_int(0, false).into());
            build_wrappers::w_br(&self.ir.builder,done_bb);

            // done_bb: phi nodes must come FIRST (before any non-phi instructions).
            self.ir.builder.position_at_end(done_bb);
            let len_phi = build_wrappers::w_phi(&self.ir.builder,i64_ty.into(), "sp_rlen");
            len_phi.add_incoming(&[(&s_len, entry_bb), (&width, pad_bb)]);
            let ptr_phi = build_wrappers::w_phi(&self.ir.builder,i8_ptr.into(), "sp_rptr");
            ptr_phi.add_incoming(&[(&s_ptr, entry_bb), (&buf, pad_bb)]);
            // Build the result str struct using insert_value (no alloca needed).
            let mut sp_res = str_ty.get_undef();
            sp_res = build_wrappers::w_insert_value(&self.ir.builder,sp_res, len_phi.as_basic_value().into_int_value().into(), 0, "sp_wl").into_struct_value();
            sp_res = build_wrappers::w_insert_value(&self.ir.builder,sp_res, ptr_phi.as_basic_value().into_pointer_value().into(), 1, "sp_rv").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, sp_res.into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

    }

}
