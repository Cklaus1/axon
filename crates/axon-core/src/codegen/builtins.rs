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

        // C stdlib: void exit(int status). Declared here so later code can
        // `get_function("exit")`; the assert/panic builtins now route through the
        // __axon_*_panic rt helpers (stderr + interp-matching message) rather than
        // calling exit directly, so the binding itself is unused here.
        let exit_ty = void_ty.fn_type(&[i32_ty.into()], false);
        let _exit_fn = self.ir.module.add_function("exit", exit_ty, None);

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

        // axon_assert: takes bool, panics (exit 101 — the interp's panic code) if false
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
            // I-2: the interpreter prints "axon: panic: assertion failed" to STDERR
            // (exit 101). Route through __axon_msg_panic so native matches the
            // stream + prefix + text (was printf "assertion failed" to STDOUT).
            let amsg = "assertion failed";
            let mp_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
            let p = self.ir.module.get_function("__axon_msg_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_msg_panic", mp_ty, None));
            let g = build_wrappers::w_global_string_ptr(&self.ir.builder, amsg, "assert_msg");
            let mlen = i64_ty.const_int(amsg.len() as u64, false);
            build_wrappers::w_call(&self.ir.builder, p, &[g.into(), mlen.into()], "");
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

        // (snprintf is no longer declared — to_str(i64)/to_str_f64 now delegate to
        // axon-rt's __axon_i64_to_str_radix / __axon_f64_to_str (Rust formatting),
        // so codegen emits NO libc snprintf at all. That removes the last variadic
        // libc dep and lets number printing link on the browser target.)

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
            // Delegate to axon-rt's __axon_i64_to_str_radix(n, 10, out_len, out_ptr)
            // — Rust decimal formatting, byte-identical to the old snprintf("%lld")
            // but with NO libc dependency (snprintf/malloc). That removes the last
            // libc tie from to_str(i64), so INTEGER printing links on the browser
            // target (wasm32-unknown-unknown) too — not just wasi/native. (f64
            // to_str_f64 still uses snprintf("%.6g"); a __axon_f64_to_str extern is
            // the follow-on for browser float printing.) Native byte-parity is the
            // 34/34 all_examples gate.
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.ir.module.add_function("to_str", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();

            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_ty = self.ir.context.void_type().fn_type(
                &[i64_ty.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()], false);
            let rt_fn = self.ir.module.get_function("__axon_i64_to_str_radix")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_i64_to_str_radix", rt_ty, None));
            let out_len = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "ts_olen");
            let out_ptr = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "ts_optr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr, i8_ptr_ptr, "ts_optrcast");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[n.into(), i64_ty.const_int(10, false).into(), out_len.into(), out_ptr_cast.into()], "ts_call");
            let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len, "ts_len").into_int_value();
            let ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr, "ts_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, len.into(), 0, "ts_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, ptr.into(), 1, "ts_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("to_str".to_string(), fn_val);
        }

        // to_str_f64: f64 → { i64, ptr }. Delegate to axon-rt's __axon_f64_to_str,
        // which formats via the shared `axon_fmt_g` (a verbatim copy of the
        // interpreter's `fmt_g`) — so native uses the SAME %.6g code as the
        // interpreter oracle (no separate snprintf to drift from), matching by
        // construction. fmt_g handles -0.0→"0" and any-NaN→"nan" internally, so
        // the old codegen-side normalization is gone. NO libc snprintf → f64
        // printing links on the browser target (wasm32-unknown-unknown) too.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let f64_ty = self.ir.context.f64_type();
            let fn_ty = str_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.ir.module.add_function("to_str_f64", fn_ty, None);
            let entry = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry);
            let x = fn_val.get_nth_param(0).unwrap().into_float_value();

            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_ty = self.ir.context.void_type().fn_type(
                &[f64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()], false);
            let rt_fn = self.ir.module.get_function("__axon_f64_to_str")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_f64_to_str", rt_ty, None));
            let out_len = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "tf_olen");
            let out_ptr = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "tf_optr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr, i8_ptr_ptr, "tf_optrcast");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[x.into(), out_len.into(), out_ptr_cast.into()], "tf_call");
            let len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len, "tf_len").into_int_value();
            let ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr, "tf_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, len.into(), 0, "tf_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, ptr.into(), 1, "tf_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
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
            // I-2: print the interpreter's exact stderr line "axon: panic:
            // assertion failed: <a> != <b>" (with values) + exit 101, instead of a
            // generic message to STDOUT.
            let aei_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let p = self.ir.module.get_function("__axon_assert_eq_i64_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_assert_eq_i64_panic", aei_ty, None));
            build_wrappers::w_call(&self.ir.builder, p, &[a.into(), b_param.into()], "");
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
            // I-2: interp prints "axon: panic: assert_err: expected Err, got Ok" to
            // STDERR (note the `assert_err:` prefix, NOT `assertion failed:`).
            let aemsg = "assert_err: expected Err, got Ok";
            let mp_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
            let p = self.ir.module.get_function("__axon_msg_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_msg_panic", mp_ty, None));
            let g = build_wrappers::w_global_string_ptr(&self.ir.builder, aemsg, "assert_err_msg");
            let mlen = i64_ty.const_int(aemsg.len() as u64, false);
            build_wrappers::w_call(&self.ir.builder, p, &[g.into(), mlen.into()], "");
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

        // parse_int(s: str) -> Result<i64, str>.
        //
        // Delegate the VALUE parse to axon-rt's __axon_parse_int_radix(s, 10)
        // (Rust `from_str_radix`, whole-string), exactly like the parse_int_radix
        // block below — base is just the constant 10. This drops the old libc
        // `strtoll` (+ the endptr dance), so parse_int emits NO libc dep and links
        // on the browser target (wasm32-unknown-unknown) too. CAVEAT (I-2): the
        // Ok/Err *value* is identical to parse_int_radix(s, 10), but the Err
        // *message* is NOT — the interpreter's `parse_int` adds a radix-prefix hint
        // (`0x1F` → "...base-10 only; strip the radix prefix") that `parse_int_radix`
        // does not. So the Err branch below rebuilds the message via
        // `__axon_parse_int_err` (the hinted base-10 builder) to match interp's
        // parse_int byte-for-byte. (`parse_int_err_parity.sh` guards this.)
        {
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_int", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "pi_entry");
            let ok_bb   = self.ir.context.append_basic_block(fn_val, "pi_ok");
            let err_bb  = self.ir.context.append_basic_block(fn_val, "pi_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let base = i64_ty.const_int(10, false);
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_ty = void_ty.fn_type(
                &[str_ty.into(), i64_ty.into(), i64_ptr.into(), i64_ptr.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false);
            let rt_fn = self.ir.module.get_function("__axon_parse_int_radix")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_int_radix", rt_ty, None));
            let out_ok_slot  = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pi_ok_slot");
            let out_val_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pi_val_slot");
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pi_len_slot");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "pi_ptr_slot");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "pi_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[s.into(), base.into(), out_ok_slot.into(), out_val_slot.into(), out_len_slot.into(), out_ptr_slot_cast.into()],
                "");

            let ok_flag = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_ok_slot, "pi_okflag").into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, IntPredicate::NE, ok_flag, zero, "pi_isok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload = out_val as i64 }
            self.ir.builder.position_at_end(ok_bb);
            let parsed = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_val_slot, "pi_parsed").into_int_value();
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pi_ok_alloca");
            let tag1 = bool_ty.const_int(1, false);
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 0, "pi_tagok");
            build_wrappers::w_store(&self.ir.builder, tag_ptr_ok, tag1.into());
            let payload_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 1, "pi_payok");
            let payload_i64_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ptr_ok, i64_ptr, "pi_payi64");
            build_wrappers::w_store(&self.ir.builder, payload_i64_ptr, parsed.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), ok_alloca, "pi_okval");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload = str { out_len, out_ptr } }
            self.ir.builder.position_at_end(err_bb);
            // I-2: `parse_int`'s Err message must match the INTERPRETER's parse_int,
            // which adds a radix-prefix hint (`0x1F` → "...base-10 only; strip the
            // radix prefix") that `parse_int_radix` (base 10) does NOT. The radix
            // delegate above produced the no-hint `parse_int_radix` message, so
            // rebuild the message here via `__axon_parse_int_err` (the dedicated
            // hinted base-10 builder), overwriting the out slots. The parsed VALUE
            // is unaffected; `parse_int_radix` keeps the no-hint message.
            let perr_ty = void_ty.fn_type(&[str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()], false);
            let perr_fn = self.ir.module.get_function("__axon_parse_int_err")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_int_err", perr_ty, None));
            build_wrappers::w_call(&self.ir.builder, perr_fn,
                &[s.into(), out_len_slot.into(), out_ptr_slot_cast.into()], "");
            let err_str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let msg_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "pi_emlen").into_int_value();
            let msg_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "pi_emptr").into_pointer_value();
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pi_err_alloca");
            let tag0 = bool_ty.const_int(0, false);
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 0, "pi_tagerr");
            build_wrappers::w_store(&self.ir.builder, tag_ptr_err, tag0.into());
            let payload_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 1, "pi_payerr");
            let payload_str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ptr_err, err_str_ty.ptr_type(inkwell::AddressSpace::default()), "pi_paystr");
            let err_str_alloca = build_wrappers::w_alloca(&self.ir.builder, err_str_ty.into(), "pi_errstr");
            let esl = build_wrappers::w_struct_gep(&self.ir.builder, err_str_ty.into(), err_str_alloca, 0, "pi_esl");
            let esd = build_wrappers::w_struct_gep(&self.ir.builder, err_str_ty.into(), err_str_alloca, 1, "pi_esd");
            build_wrappers::w_store(&self.ir.builder, esl, msg_len.into());
            build_wrappers::w_store(&self.ir.builder, esd, msg_ptr.into());
            let err_str_val = build_wrappers::w_load(&self.ir.builder, err_str_ty.into(), err_str_alloca, "pi_errstrval");
            build_wrappers::w_store(&self.ir.builder, payload_str_ptr, err_str_val);
            let err_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), err_alloca, "pi_errval");
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
        // void __axon_str_split(s:str, sep:str, out_len:i64*, out_data:i8**) →
        // an array of AxonStr (same {len,data} out-param shape as dict_keys).
        let ssplit_ty = void_ty.fn_type(
            &[str_ty_d.into(), str_ty_d.into(), i64_ptr_d.into(), i8_ptr_ptr_d.into()], false);
        self.ir.module.add_function("__axon_str_split", ssplit_ty, None);
        // i8* __axon_dict_from_pairs(len:i64, data:i8*) → a Dict handle. `data`
        // points at an array of (str,i64) tuples = LLVM `{{i64,i8*}, i64}`;
        // passed as scalars (the str_join slice-arg ABI). Returns the handle.
        let dfp_ty = i8_ptr.fn_type(&[i64_ty.into(), i8_ptr.into()], false);
        self.ir.module.add_function("__axon_dict_from_pairs", dfp_ty, None);
        self.fn_return_types.insert("dict_from_pairs".to_string(), Type::Deferred("Dict".to_string()));
        // void __axon_dict_to_pairs(d:i8*, out_len:i64*, out_data:i8**) → an array
        // of (str,i64) tuples (StrI64Pair). Same out-param shape as dict_keys.
        let dtp_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ptr_d.into(), i8_ptr_ptr_d.into()], false);
        self.ir.module.add_function("__axon_dict_to_pairs", dtp_ty, None);
        // i8* __axon_dict_map_values(d:i8*, fn_ptr:i8*, env:i8*) → a Dict handle.
        // The runtime indirect-calls the lambda `i64 fn(i8* env, i64)` per value.
        let dmv_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i8_ptr.into()], false);
        self.ir.module.add_function("__axon_dict_map_values", dmv_ty, None);
        self.fn_return_types.insert("dict_map_values".to_string(), Type::Deferred("Dict".to_string()));
        // i8* __axon_dict_filter(d:i8*, fn_ptr:i8*, env:i8*) → a Dict handle. The
        // runtime indirect-calls `i64 fn(i8* env, AxonStr key, i64 val)` per entry
        // (keeps it iff non-zero). Same lowering as dict_map_values.
        let dfl_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i8_ptr.into()], false);
        self.ir.module.add_function("__axon_dict_filter", dfl_ty, None);
        self.fn_return_types.insert("dict_filter".to_string(), Type::Deferred("Dict".to_string()));
        // void __axon_dict_each(d:i8*, fn_ptr:i8*, env:i8*). Runtime calls
        // `i64 fn(i8* env, AxonStr key, i64 val)` per entry for side effects;
        // returns nothing. Same callback ABI as dict_filter.
        let dea_ty = void_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i8_ptr.into()], false);
        self.ir.module.add_function("__axon_dict_each", dea_ty, None);
        self.fn_return_types.insert("dict_each".to_string(), Type::Unit);
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

            // exp / ln / log10 — the transcendental trio. LLVM intrinsics
            // (lowered to C libm exp/log/log10), matching the interpreter's
            // Rust f64::{exp,ln,log10} (which call the same libm), so
            // native==interp. Note: Axon `ln` = natural log = `llvm.log.f64`
            // (C `log`); `log10` is the base-10 intrinsic. Closes the E0910
            // gap for these (they were interp-only).
            let exp_fn = self.ir.module.add_function("llvm.exp.f64", f1, None);
            self.functions.insert("exp".to_string(), exp_fn);
            self.fn_return_types.insert("exp".to_string(), Type::F64);

            let ln_fn = self.ir.module.add_function("llvm.log.f64", f1, None);
            self.functions.insert("ln".to_string(), ln_fn);
            self.fn_return_types.insert("ln".to_string(), Type::F64);

            let log10_fn = self.ir.module.add_function("llvm.log10.f64", f1, None);
            self.functions.insert("log10".to_string(), log10_fn);
            self.fn_return_types.insert("log10".to_string(), Type::F64);
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
            // I-2: interp prints "axon: panic: assertion failed: <a> != <b>" (f64
            // Display, with values) to STDERR. Delegate to the rt helper.
            let aef_ty = void_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let p = self.ir.module.get_function("__axon_assert_eq_f64_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_assert_eq_f64_panic", aef_ty, None));
            build_wrappers::w_call(&self.ir.builder, p, &[a.into(), b_param.into()], "");
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
            // lengths differ → fail. I-2: interp prints one message regardless of
            // why ("assertion failed: <a:?> != <b:?>", debug-quoted, to STDERR).
            self.ir.builder.position_at_end(len_fail_bb);
            let aes_ty = void_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let p = self.ir.module.get_function("__axon_assert_eq_str_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_assert_eq_str_panic", aes_ty, None));
            build_wrappers::w_call(&self.ir.builder, p, &[a_struct.into(), b_struct.into()], "");
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
            // bytes differ → fail (same interp message as the length case).
            self.ir.builder.position_at_end(bytes_fail_bb);
            let aes_ty = void_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let p = self.ir.module.get_function("__axon_assert_eq_str_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_assert_eq_str_panic", aes_ty, None));
            build_wrappers::w_call(&self.ir.builder, p, &[a_struct.into(), b_struct.into()], "");
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

        // ── dict_to_str(d: Dict) -> Result<str, str> ─────────────────────────
        // Serialize to `key=value\n` lines; an unrepresentable key/value is a
        // recoverable Err. IDENTICAL Result<str,str> assembly to read_file —
        // the runtime signals Err via a negative out_len. Only the arg differs
        // (an i8* dict handle instead of a str path).
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.ir.context.i8_type().array_type(16);
            let result_ty = self.ir.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ptr.into(), i8_ptr_ptr.into()], false);
            let rt_fn = self.ir.module.add_function("__axon_dict_to_str", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[i8_ptr.into()], false);
            let fn_val = self.ir.module.add_function("dict_to_str", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "dts_entry");
            let ok_bb = self.ir.context.append_basic_block(fn_val, "dts_ok");
            let err_bb = self.ir.context.append_basic_block(fn_val, "dts_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let d_arg = fn_val.get_nth_param(0).unwrap().into_pointer_value();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "dts_out_len");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "dts_out_ptr");
            let out_ptr_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "dts_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[d_arg.into(), out_len_slot.into(), out_ptr_cast.into()], "");

            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "dts_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "dts_ptr").into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::SGE, out_len, zero_i64, "dts_is_ok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(ok_bb);
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "dts_ok_slot");
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 0, "dts_tag_ok"),
                bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 1, "dts_pay_ok");
            let str_ok_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "dts_str_ok_ptr");
            let str_ok_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "dts_str_ok");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_ok_slot, 0, ""), out_len.into());
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_ok_slot, 1, ""), out_ptr.into());
            let str_ok_val = build_wrappers::w_load(&self.ir.builder, str_ty.into(), str_ok_slot, "dts_str_ok_val");
            build_wrappers::w_store(&self.ir.builder, str_ok_ptr, str_ok_val);
            let ok_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), ok_alloca, "dts_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: actual_len = -out_len, { tag=0, payload=str{actual_len, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let actual_len = build_wrappers::w_int_neg(&self.ir.builder, out_len, "dts_actual_len");
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "dts_err_slot");
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 0, "dts_tag_err"),
                bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 1, "dts_pay_err");
            let str_err_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "dts_str_err_ptr");
            let str_err_slot = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "dts_str_err");
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_err_slot, 0, ""), actual_len.into());
            build_wrappers::w_store(&self.ir.builder, build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), str_err_slot, 1, ""), out_ptr.into());
            let str_err_val = build_wrappers::w_load(&self.ir.builder, str_ty.into(), str_err_slot, "dts_str_err_val");
            build_wrappers::w_store(&self.ir.builder, str_err_ptr, str_err_val);
            let err_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), err_alloca, "dts_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("dict_to_str".to_string(), fn_val);
            self.fn_return_types.insert("dict_to_str".to_string(),
                Type::Result(Box::new(Type::Str), Box::new(Type::Str)));
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

            // Delegate the whole parse to axon-rt's __axon_parse_bool — accepts
            // "true"/"false" AFTER trim() (so "  true  " is Ok, which the old
            // strncmp-on-raw-bytes path rejected) and produces the interpreter's
            // Err message ("could not parse `<s>` as a bool …"), not "invalid
            // bool". out_val is i64 (0/1); the i1 payload is `out_val != 0`.
            let void_ty = self.ir.context.void_type();
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_bool", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "pb_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "pb_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "pb_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let rt_ty = void_ty.fn_type(
                &[str_ty.into(), i64_ptr.into(), i64_ptr.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false);
            let rt_fn = self.ir.module.get_function("__axon_parse_bool")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_bool", rt_ty, None));
            let out_ok_slot  = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pb_ok_slot");
            let out_val_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pb_val_slot");
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pb_len_slot");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "pb_ptr_slot");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "pb_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[s.into(), out_ok_slot.into(), out_val_slot.into(), out_len_slot.into(), out_ptr_slot_cast.into()],
                "");

            let ok_flag = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_ok_slot, "pb_okflag").into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, ok_flag, zero, "pb_isok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload = (out_val != 0) as i1 }
            self.ir.builder.position_at_end(ok_bb);
            let val_i64 = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_val_slot, "pb_valint").into_int_value();
            let val_i1 = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, val_i64, zero, "pb_valbool");
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pb_ok_alloca");
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 0, "pb_ot_tag"),
                i1_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), ok_alloca, 1, "pb_ot_pay");
            let bool_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_ok, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_ot_bptr");
            build_wrappers::w_store(&self.ir.builder, bool_ptr, val_i1.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), ok_alloca, "pb_ot_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload = str{out_len, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let msg_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "pb_emlen").into_int_value();
            let msg_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "pb_emptr").into_pointer_value();
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder, result_ty.into(), "pb_err_alloca");
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 0, "pb_err_tag"),
                i1_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder, result_ty.into(), err_alloca, 1, "pb_err_pay");
            let str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder, payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "pb_err_sptr");
            let err_str_alloca = build_wrappers::w_alloca(&self.ir.builder, str_ty.into(), "pb_err_s");
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), err_str_alloca, 0, "pb_esl"), msg_len.into());
            build_wrappers::w_store(&self.ir.builder,
                build_wrappers::w_struct_gep(&self.ir.builder, str_ty.into(), err_str_alloca, 1, "pb_esp"), msg_ptr.into());
            let err_str_val = build_wrappers::w_load(&self.ir.builder, str_ty.into(), err_str_alloca, "pb_esv");
            build_wrappers::w_store(&self.ir.builder, str_ptr, err_str_val);
            let err_val = build_wrappers::w_load(&self.ir.builder, result_ty.into(), err_alloca, "pb_err_val");
            build_wrappers::w_ret(&self.ir.builder, err_val);

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("parse_bool".to_string(), fn_val);
            self.fn_return_types.insert("parse_bool".to_string(),
                Type::Result(Box::new(Type::Bool), Box::new(Type::Str)));
        }

        // ── Phase 7: random_i64(lo: i64, hi: i64) -> i64 ─────────────────────
        // Uses C rand() % (hi - lo) + lo, with the SAME degenerate-bounds guard
        // the interpreter has (BUG_HUNT #27/#36, I-2 parity, I-9 no-silent-wrong):
        //   • hi <  lo → inverted bounds: print an error + exit(101) (matches the
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

            // Inverted bounds: panic(101) with the interpreter's EXACT stderr
            // line (incl. the lo/hi values) via __axon_random_inverted_panic —
            // loud failure, not a silent garbage value (I-9). Was a printf to
            // STDOUT with generic, value-less text and no "axon: panic:" prefix.
            self.ir.builder.position_at_end(inverted_bb);
            let rip_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let rip = self.ir.module.get_function("__axon_random_inverted_panic")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_random_inverted_panic", rip_ty, None));
            build_wrappers::w_call(&self.ir.builder, rip, &[lo.into(), hi.into()], "");
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

        // str_digits_only(s: str) -> str — keep only ASCII digits. Identical
        // str→str out-param shape as str_reverse; delegates to axon-rt's
        // __axon_str_digits_only (matches the interpreter's char filter).
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function("__axon_str_digits_only")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_str_digits_only", rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("str_digits_only", fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sdo_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "sdo_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "sdo_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "sdo_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "sdo_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "sdo_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "sdo_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "sdo_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());

            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert("str_digits_only".to_string(), fn_val);
            self.fn_return_types.insert("str_digits_only".to_string(), Type::Str);
        }

        // Declare __axon_str_join for the emit_call lowering (str_join takes a
        // [str] slice + sep → str). The slice is passed as two scalars
        // (i64 len, AxonStr* data) + the sep str struct + str out-params.
        {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let str_ptr = str_ty.ptr_type(inkwell::AddressSpace::default());
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let sj_ty = self.ir.context.void_type().fn_type(&[
                i64_ty.into(), str_ptr.into(), str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            self.ir.module.add_function("__axon_str_join", sj_ty, None);
            self.fn_return_types.insert("str_join".to_string(), Type::Str);
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

        // ── Checked-arithmetic runtime trap (I-9 parity with the interpreter) ─
        //   __axon_arith_panic(kind: i64, op_ptr, op_len, a: i64, b: i64) -> noreturn
        //
        // Codegen injects a guarded call to this on the failure branch of every
        // signed-i64 `+`/`-`/`*` (overflow) and `/`/`%` (divisor == 0). The
        // interpreter checks these in `interp/value.rs` and exits 101 on the
        // fault; native used to silently two's-complement-wrap (a wrong answer,
        // I-9) or raw-SIGFPE (exit 136, no message). This brings native into
        // line: same `axon: panic: …` text, same exit 101. The message is
        // formatted in the runtime from the operands (the overflow text embeds
        // the runtime values, unknown at IR-build time).
        {
            let ap_ty = void_ty.fn_type(
                &[
                    i64_ty.into(),  // kind: 0 overflow, 1 div0, 2 rem0
                    i8_ptr.into(),  // op_ptr (operator glyph, overflow only)
                    i64_ty.into(),  // op_len
                    i64_ty.into(),  // a (left operand)
                    i64_ty.into(),  // b (right operand)
                ],
                false,
            );
            let _ = self.ir.module.add_function("__axon_arith_panic", ap_ty, None);
        }

        // ── Array bounds-check runtime trap (I-9 + memory-safety parity) ──────
        //   __axon_bounds_panic(idx: i64, len: i64) -> noreturn
        //
        // Codegen guards every `a[i]` load with `i < 0 || i >= len` and calls
        // this on the failing branch. The interpreter bounds-checks
        // (eval.rs, "index {i} out of bounds (len {n})", exit 101); native used
        // to do an UNCHECKED raw GEP — a[5] on a len-3 slice returned garbage and
        // a[-1] read arbitrary memory, both at exit 0 (a silent wrong result AND
        // a memory-safety hole). This brings native into line.
        {
            let bp_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_bounds_panic", bp_ty, None);
        }

        // ── Generic message panic (fixed-string runtime faults) ──────────────
        //   __axon_msg_panic(msg_ptr, msg_len) -> noreturn
        // For faults with a build-time-known message (e.g. "arr_max_i64: array
        // is empty"). Native used to call the bare C exit(101) with NO message,
        // diverging from the interpreter's text; this prints the same line.
        {
            let mp_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_msg_panic", mp_ty, None);
            // …and the i64-interpolating variant: __axon_msg_panic_i64(msg, len, n)
            // for messages that append a runtime value (e.g. "…got <n>").
            let mpi_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into(), i64_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_msg_panic_i64", mpi_ty, None);
            // assert_eq panic helpers — print the interpreter's exact stderr line
            // (`assertion failed: <a> != <b>`) with the actual values, exit 101.
            let f64_ty = self.ir.context.f64_type();
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let aei_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_assert_eq_i64_panic", aei_ty, None);
            let aef_ty = void_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_assert_eq_f64_panic", aef_ty, None);
            let aes_ty = void_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let _ = self.ir.module.add_function("__axon_assert_eq_str_panic", aes_ty, None);
        }

        // ── Phase 5: refinement-precondition violation panic ─────────────────
        //   __axon_refine_panic(fn_ptr, fn_len, param_ptr, param_len,
        //                        refine_ptr, refine_len) -> noreturn (exit 6)
        // Emitted at function entry on the branch where a refined parameter's
        // `where` predicate evaluated false. Names the fn/param/refinement (all
        // build-time strings) and exits 6 — the runtime fallback for non-constant
        // refinement args; matches the interpreter's Flow::RefineViolation (I-2).
        {
            let rp_ty = void_ty.fn_type(
                &[
                    i8_ptr.into(),
                    i64_ty.into(),
                    i8_ptr.into(),
                    i64_ty.into(),
                    i8_ptr.into(),
                    i64_ty.into(),
                ],
                false,
            );
            let _ = self.ir.module.add_function("__axon_refine_panic", rp_ty, None);
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

            // Delegate the whole parse to axon-rt's __axon_parse_float (Rust
            // `trim().parse::<f64>()`, whole-string) — byte-identical to interp
            // (value AND Err message), unlike the old libc `strtod` which
            // prefix-parsed (`"12abc"` → 12) and emitted an EMPTY Err message.
            // Drops the libc `strtod` dep too. Mirrors the parse_int delegation.
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function("parse_float", fn_ty, None);

            let entry_bb = self.ir.context.append_basic_block(fn_val, "pf_entry");
            let ok_bb    = self.ir.context.append_basic_block(fn_val, "pf_ok");
            let err_bb   = self.ir.context.append_basic_block(fn_val, "pf_err");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let f64_ptr_ty = f64_ty.ptr_type(inkwell::AddressSpace::default());
            let rt_ty = void_ty.fn_type(
                &[str_ty.into(), i64_ptr.into(), f64_ptr_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false);
            let rt_fn = self.ir.module.get_function("__axon_parse_float")
                .unwrap_or_else(|| self.ir.module.add_function("__axon_parse_float", rt_ty, None));
            let out_ok_slot  = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pf_ok_slot");
            let out_val_slot = build_wrappers::w_alloca(&self.ir.builder, f64_ty.into(), "pf_val_slot");
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "pf_len_slot");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "pf_ptr_slot");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder, out_ptr_slot, i8_ptr_ptr, "pf_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn,
                &[s.into(), out_ok_slot.into(), out_val_slot.into(), out_len_slot.into(), out_ptr_slot_cast.into()],
                "");

            let ok_flag = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_ok_slot, "pf_okflag").into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_ok = build_wrappers::w_int_compare(&self.ir.builder, inkwell::IntPredicate::NE, ok_flag, zero, "pf_isok");
            build_wrappers::w_cond_br(&self.ir.builder, is_ok, ok_bb, err_bb);

            // ok_bb: { tag=1, payload=f64 as [16 x i8] }
            self.ir.builder.position_at_end(ok_bb);
            let parsed_f64 = build_wrappers::w_load(&self.ir.builder, f64_ty.into(), out_val_slot, "pf_parsed").into_float_value();
            let ok_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pf_ok_alloca");
            let tag_ptr_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 0, "pf_tag_ok");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_ok, bool_ty.const_int(1, false).into());
            let payload_ok = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), ok_alloca, 1, "pf_pay_ok");
            let f64_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_ok, f64_ptr_ty, "pf_f64_ptr");
            build_wrappers::w_store(&self.ir.builder,f64_ptr, parsed_f64.into());
            let ok_val = build_wrappers::w_load(&self.ir.builder,result_ty.into(), ok_alloca, "pf_ok_val");
            build_wrappers::w_ret(&self.ir.builder, ok_val);

            // err_bb: { tag=0, payload=str{out_len, out_ptr} }
            self.ir.builder.position_at_end(err_bb);
            let msg_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "pf_emlen").into_int_value();
            let msg_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "pf_emptr").into_pointer_value();
            let err_alloca = build_wrappers::w_alloca(&self.ir.builder,result_ty.into(), "pf_err_alloca");
            let tag_ptr_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 0, "pf_tag_err");
            build_wrappers::w_store(&self.ir.builder,tag_ptr_err, bool_ty.const_int(0, false).into());
            let payload_err = build_wrappers::w_struct_gep(&self.ir.builder,result_ty.into(), err_alloca, 1, "pf_pay_err");
            let err_str_ptr = build_wrappers::w_pointer_cast(&self.ir.builder,payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "pf_str_err_ptr");
            let err_str_slot = build_wrappers::w_alloca(&self.ir.builder,str_ty.into(), "pf_str_err");
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_slot, 0, ""), msg_len.into());
            build_wrappers::w_store(&self.ir.builder,build_wrappers::w_struct_gep(&self.ir.builder,str_ty.into(), err_str_slot, 1, ""), msg_ptr.into());
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

        // ── Phase 6 / R1b: str_to_upper / str_to_lower ───────────────────────
        // Delegate to axon-rt's __axon_str_to_{upper,lower}, which use full
        // Unicode case mapping (str::to_uppercase / to_lowercase) — matching the
        // interpreter oracle (I-2). The old inline body converted only ASCII
        // a-z/A-Z byte-wise, so it DIVERGED on any non-ASCII letter and could not
        // represent case maps that GROW the string (ß→SS). Same str→str
        // out-param shape as str_reverse: void f(AxonStr, i64* out_len, i8** out_ptr).
        for (fname, rt_name) in &[("str_to_upper", "__axon_str_to_upper"), ("str_to_lower", "__axon_str_to_lower")] {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function(rt_name)
                .unwrap_or_else(|| self.ir.module.add_function(rt_name, rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "stl_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "stl_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "stl_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "stl_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "stl_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "stl_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "stl_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "stl_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

        // ── Phase 6 / R1b: str_trim / str_trim_start / str_trim_end ──────────
        // Delegate to axon-rt's __axon_str_trim{,_start,_end}, which use Rust's
        // str::trim (Unicode White_Space) — matching the interpreter oracle (I-2).
        // The old inline body trimmed every byte <= 32, which both UNDER-trims
        // (kept U+00A0 the interp removes) and OVER-trims (stripped ASCII control
        // chars like \x01 the interp keeps). Same str→str out-param shape as
        // str_reverse: void f(AxonStr, i64* out_len, i8** out_ptr).
        for (fname, rt_name) in &[
            ("str_trim", "__axon_str_trim"),
            ("str_trim_start", "__axon_str_trim_start"),
            ("str_trim_end", "__axon_str_trim_end"),
        ] {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function(rt_name)
                .unwrap_or_else(|| self.ir.module.add_function(rt_name, rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "stt_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "stt_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "stt_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "stt_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "stt_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "stt_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "stt_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "stt_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
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

        // ── Phase 7 / R1b: str_pad_start / str_pad_end ───────────────────────
        // Delegate to axon-rt's __axon_str_pad_{start,end}, which pad with the
        // first CHAR of fill repeated (width - s.len()) times — matching the
        // interpreter oracle (I-2). The old inline body memset the first BYTE of
        // fill, so a multibyte fill char (★, é) produced INVALID UTF-8 and padded
        // to exactly `width` bytes rather than `width - s.len()` whole chars.
        // ABI: void f(AxonStr s, i64 width, AxonStr fill, i64* out_len, i8** out_ptr).
        for (fname, rt_name) in &[
            ("str_pad_start", "__axon_str_pad_start"),
            ("str_pad_end", "__axon_str_pad_end"),
        ] {
            let str_ty = self.ir.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let rt_fn_ty = self.ir.context.void_type().fn_type(&[
                str_ty.into(), i64_ty.into(), str_ty.into(), i64_ptr.into(), i8_ptr_ptr.into(),
            ], false);
            let rt_fn = self.ir.module.get_function(rt_name)
                .unwrap_or_else(|| self.ir.module.add_function(rt_name, rt_fn_ty, None));

            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), str_ty.into()], false);
            let fn_val = self.ir.module.add_function(fname, fn_ty, None);
            let bb = self.ir.context.append_basic_block(fn_val, "entry");
            let saved = self.ir.builder.get_insert_block();
            self.ir.builder.position_at_end(bb);

            let s_arg = fn_val.get_nth_param(0).unwrap();
            let width_arg = fn_val.get_nth_param(1).unwrap();
            let fill_arg = fn_val.get_nth_param(2).unwrap();
            let out_len_slot = build_wrappers::w_alloca(&self.ir.builder, i64_ty.into(), "sp_olen");
            let out_ptr_slot = build_wrappers::w_alloca(&self.ir.builder, i8_ptr.into(), "sp_optr");
            let out_ptr_slot_cast = build_wrappers::w_pointer_cast(&self.ir.builder,
                out_ptr_slot, i8_ptr_ptr, "sp_ptrptr");
            build_wrappers::w_call(&self.ir.builder, rt_fn, &[
                s_arg.into(), width_arg.into(), fill_arg.into(), out_len_slot.into(), out_ptr_slot_cast.into(),
            ], "sp_call");
            let out_len = build_wrappers::w_load(&self.ir.builder, i64_ty.into(), out_len_slot, "sp_len").into_int_value();
            let out_ptr = build_wrappers::w_load(&self.ir.builder, i8_ptr.into(), out_ptr_slot, "sp_ptr").into_pointer_value();
            let mut result = str_ty.const_zero();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_len.into(), 0, "sp_r0").into_struct_value();
            result = build_wrappers::w_insert_value(&self.ir.builder, result, out_ptr.into(), 1, "sp_r1").into_struct_value();
            build_wrappers::w_ret(&self.ir.builder, result.into());
            if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

    }

}
