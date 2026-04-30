//! LLVM IR emission for Axon via inkwell (LLVM 17).
//!
//! # Two-pass design
//! 1. `declare_functions` — forward-declare every top-level fn so mutual recursion works.
//! 2. `emit_program` — emit bodies, struct layouts, etc.
//!
//! # Struct layout conventions
//! - `Option<T>`  → `{ i1, T }` (discriminant + value)
//! - `Result<T,E>` → `{ i1, [N x i8] }` (discriminant + union-sized payload)
//! - `Str`        → `{ i64, ptr }` (length + heap data pointer)
//! - `Slice<T>`   → `{ i64, ptr }` (length + heap data pointer)
//! - `Unit`       → treated as void; functions returning Unit use `build_return_void`.

use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

use inkwell::builder::Builder;
use inkwell::context::Context;
use inkwell::module::Module;
use inkwell::targets::{
    CodeModel, FileType, InitializationConfig, RelocMode, Target, TargetMachine, TargetTriple,
};
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue, InstructionOpcode};
use inkwell::AddressSpace;
use inkwell::IntPredicate;
use inkwell::FloatPredicate;
use inkwell::OptimizationLevel;

use crate::ast;
use crate::ast::AxonType;
use crate::types::Type;

/// Extract a simple string name from an `AxonType` for impl-method name mangling.
fn ast_type_simple_name(ty: &AxonType) -> String {
    match ty {
        AxonType::Named(n) => n.clone(),
        AxonType::Generic { base, .. } => base.clone(),
        _ => "Unknown".into(),
    }
}

// ── Public surface ────────────────────────────────────────────────────────────

pub struct Codegen<'ctx> {
    pub context: &'ctx Context,
    pub module: Module<'ctx>,
    pub builder: Builder<'ctx>,
    /// Maps local variable names to their alloca pointers and LLVM types.
    locals: HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    /// Maps fn names to the LLVM function value.
    functions: HashMap<String, FunctionValue<'ctx>>,
    /// Maps struct names to their ordered field names (for FieldAccess GEP).
    struct_fields: HashMap<String, Vec<String>>,
    /// Maps fn names to their Axon semantic return type (for call-site type inference).
    fn_return_types: HashMap<String, Type>,
    /// Tracks inferred Axon semantic types for named locals (for match/field-access dispatch).
    local_types: HashMap<String, Type>,
    /// Set when inside a function returning `Result<T,E>`; drives canonical union layout.
    current_result_types: Option<(Type, Type)>,
    /// Counter for generating unique anonymous function names (lambdas).
    lambda_counter: u32,
    /// Counter for generating unique global names in format strings.
    fmtstr_counter: u32,
    /// Maps enum name → list of (variant_name, tag_int, field_types).
    /// Used by StructLit and Pattern::Struct for enum variant codegen.
    enum_variants: HashMap<String, Vec<(String, usize, Vec<Type>)>>,
    /// All top-level FnDefs by name, populated during emit_program for comptime evaluation.
    fndefs: HashMap<String, ast::FnDef>,
    /// Generic function type-parameter names (fn_name → [type param names]).
    /// Used to mangle call sites to their concrete monomorphized versions.
    pub generic_fn_params: HashMap<String, Vec<String>>,
    /// Phase 3: trait definitions (for method order during vtable construction).
    trait_defs: HashMap<String, ast::TraitDef>,
    /// Phase 3: (trait_name, type_name) → vtable global (array of fn ptrs).
    vtable_globals: HashMap<(String, String), inkwell::values::GlobalValue<'ctx>>,
    /// Phase 3: Axon param types per function (for coercion at call sites).
    fn_axon_params: HashMap<String, Vec<ast::AxonType>>,
    /// Phase 3: vtable thunk function types (trait_name, method_name) → FunctionType.
    vtable_thunk_types: HashMap<(String, String), inkwell::types::FunctionType<'ctx>>,
    /// Module-level comptime binding table: name → evaluated constant.
    comptime_env: HashMap<String, crate::comptime::ComptimeVal>,
    /// Stack of (continue_target, break_target) for the enclosing while loops.
    loop_stack: Vec<(inkwell::basic_block::BasicBlock<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)>,
    /// Current lambda's closure environment, set when emitting a lambda body.
    /// Tuple of (env_ptr, env_struct_ty, capture_index_map).
    /// When set, `Ident` lookups that miss `self.locals` fall back to loading
    /// the captured value from the env struct via GEP. This is a defensive
    /// safety net — the primary capture path binds field pointers directly
    /// into `self.locals` (see `Expr::Lambda` handler), so this fallback only
    /// fires for variables the resolver missed (e.g. names introduced by AST
    /// rewrites after `fill_captures` ran).
    current_lambda_env: Option<(PointerValue<'ctx>, StructType<'ctx>, HashMap<String, u32>)>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        Self {
            context,
            module,
            builder,
            locals: HashMap::new(),
            functions: HashMap::new(),
            struct_fields: HashMap::new(),
            fn_return_types: HashMap::new(),
            local_types: HashMap::new(),
            current_result_types: None,
            lambda_counter: 0,
            fmtstr_counter: 0,
            enum_variants: HashMap::new(),
            fndefs: HashMap::new(),
            generic_fn_params: HashMap::new(),
            trait_defs: HashMap::new(),
            vtable_globals: HashMap::new(),
            fn_axon_params: HashMap::new(),
            vtable_thunk_types: HashMap::new(),
            comptime_env: HashMap::new(),
            loop_stack: Vec::new(),
            current_lambda_env: None,
        }
    }

    // ── Type mapping ──────────────────────────────────────────────────────────

    /// Convert an Axon semantic `Type` into an LLVM `BasicTypeEnum`.
    /// Returns `None` for `Unit` (void) and unresolved/unknown types.
    pub fn llvm_type(&self, ty: &Type) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            Type::I8 | Type::U8 => Some(self.context.i8_type().into()),
            Type::I16 | Type::U16 => Some(self.context.i16_type().into()),
            Type::I32 | Type::U32 => Some(self.context.i32_type().into()),
            Type::I64 | Type::U64 => Some(self.context.i64_type().into()),
            Type::F32 => Some(self.context.f32_type().into()),
            Type::F64 => Some(self.context.f64_type().into()),
            Type::Bool => Some(self.context.bool_type().into()),

            // Str → struct { i64, ptr }
            Type::Str => {
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()],
                    /*packed=*/ false,
                );
                Some(str_ty.into())
            }

            // Unit → no LLVM basic type (void)
            Type::Unit => None,

            // Option<T> → struct { i1, T }
            Type::Option(inner) => {
                let tag = self.context.bool_type();
                if let Some(inner_llvm) = self.llvm_type(inner) {
                    let opt_ty = self.context.struct_type(
                        &[tag.into(), inner_llvm],
                        false,
                    );
                    Some(opt_ty.into())
                } else {
                    // Option<Unit> is just a bool
                    Some(tag.into())
                }
            }

            // Result<T,E> → struct { i1, [max(sizeof T, sizeof E) x i8] }
            Type::Result(ok_ty, err_ty) => {
                let tag = self.context.bool_type();
                let ok_size = self.llvm_sizeof(ok_ty).unwrap_or(0);
                let err_size = self.llvm_sizeof(err_ty).unwrap_or(0);
                let payload_size = ok_size.max(err_size).max(1);
                let i8_ty = self.context.i8_type();
                let payload = i8_ty.array_type(payload_size as u32);
                let result_ty = self.context.struct_type(
                    &[tag.into(), payload.into()],
                    false,
                );
                Some(result_ty.into())
            }

            // Slice<T> → struct { i64, ptr }
            Type::Slice(_inner) => {
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()],
                    false,
                );
                Some(slice_ty.into())
            }

            // Tuple → struct { T0, T1, ... }
            Type::Tuple(fields) => {
                let field_tys: Vec<BasicTypeEnum<'ctx>> = fields
                    .iter()
                    .filter_map(|f| self.llvm_type(f))
                    .collect();
                let tuple_ty = self.context.struct_type(&field_tys, false);
                Some(tuple_ty.into())
            }

            // Fn<params, ret> → opaque pointer (function pointers in LLVM 17
            // use the opaque pointer representation; typed fn pointers are gone).
            Type::Fn(_, _) => {
                Some(self.context.i8_type().ptr_type(AddressSpace::default()).into())
            }

            // Named struct — look up the named struct in the LLVM module.
            Type::Struct(name) => {
                self.module.get_struct_type(name).map(|s| s.into())
            }

            // Enum — look up by name with "_enum" suffix convention.
            Type::Enum(name) => {
                let mangled = format!("{name}_enum");
                self.module.get_struct_type(&mangled).map(|s| s.into())
            }

            // Unresolved — skip
            Type::Unknown | Type::Var(_) | Type::Deferred(_) => None,
            // TypeParam should be eliminated by monomorphization.
            Type::TypeParam(_) => None,
            // DynTrait → fat pointer { ptr data, ptr vtable }
            Type::DynTrait(_) => {
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                Some(self.context.struct_type(&[ptr_ty.into(), ptr_ty.into()], false).into())
            }
            // Chan<T> → opaque pointer to axon-rt channel object
            Type::Chan(_) => {
                Some(self.context.i8_type().ptr_type(AddressSpace::default()).into())
            }
        }
    }

    /// Heuristic byte-size of a type (used for Result payload sizing).
    fn llvm_sizeof(&self, ty: &Type) -> Option<u64> {
        match ty {
            Type::I8 | Type::U8 | Type::Bool => Some(1),
            Type::I16 | Type::U16 => Some(2),
            Type::I32 | Type::U32 | Type::F32 => Some(4),
            Type::I64 | Type::U64 | Type::F64 => Some(8),
            Type::Str | Type::Slice(_) => Some(16), // { i64, ptr }
            // Option<T> → { i1, T }: the i1 tag is padded up to align(T),
            // so the total size is align(T) + sizeof(T).
            Type::Option(inner) => {
                let inner_size = self.llvm_sizeof(inner).unwrap_or(0);
                let align = self.llvm_align_of(inner);
                Some(align + inner_size)
            }
            Type::Tuple(fields) => Some(
                fields.iter().map(|f| self.llvm_sizeof(f).unwrap_or(0)).sum(),
            ),
            Type::Struct(_) | Type::Enum(_) => Some(8), // conservative
            Type::Unit => Some(0),
            _ => None,
        }
    }

    /// Alignment in bytes of a type (matches LLVM's ABI alignment on x86-64).
    fn llvm_align_of(&self, ty: &Type) -> u64 {
        match ty {
            Type::Bool | Type::I8 | Type::U8 => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            // i64, f64, ptr, str, slice all align to 8
            _ => 8,
        }
    }

    // ── Pass 1: forward-declare functions ────────────────────────────────────

    /// Declare all Axon built-in functions as either extern C declarations or
    /// thin wrappers, so calls resolve during emit.
    pub fn declare_builtins(&mut self) {
        let i8_ptr = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
        let i32_ty = self.context.i32_type();
        let i64_ty = self.context.i64_type();
        let bool_ty = self.context.bool_type();
        let void_ty = self.context.void_type();

        // C stdlib: int puts(const char *s)  — prints string + newline
        let puts_ty = i32_ty.fn_type(&[i8_ptr.into()], false);
        let puts_fn = self.module.add_function("puts", puts_ty, None);

        // C stdlib: int printf(const char *fmt, ...)
        let printf_ty = i32_ty.fn_type(&[i8_ptr.into()], /*variadic=*/true);
        let printf_fn = self.module.add_function("printf", printf_ty, None);

        // C stdlib: void exit(int status)
        let exit_ty = void_ty.fn_type(&[i32_ty.into()], false);
        let exit_fn = self.module.add_function("exit", exit_ty, None);

        // axon_println: takes { i64, i8* } Axon str struct, calls puts on the data ptr
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("println", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved_block = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            self.builder.build_call(puts_fn, &[data_ptr.into()], "").unwrap();
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.builder.position_at_end(b); }
            self.functions.insert("println".to_string(), fn_val);
        }

        // axon_print: like println but uses printf without newline
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("print", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved_block = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            // printf("%s", data_ptr)
            let fmt = self.context.const_string(b"%s", true);
            let fmt_global = self.module.add_global(fmt.get_type(), None, "print_fmt");
            fmt_global.set_initializer(&fmt);
            fmt_global.set_constant(true);
            let fmt_ptr = fmt_global.as_pointer_value();
            self.builder.build_call(printf_fn, &[fmt_ptr.into(), data_ptr.into()], "").unwrap();
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.builder.position_at_end(b); }
            self.functions.insert("print".to_string(), fn_val);
        }

        // axon_assert: takes bool, panics (calls exit(1)) if false
        {
            let fn_ty = void_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.module.add_function("assert", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.context.append_basic_block(fn_val, "ok");
            let saved_block = self.builder.get_insert_block();

            self.builder.position_at_end(entry_bb);
            let cond = fn_val.get_nth_param(0).unwrap().into_int_value();
            self.builder.build_conditional_branch(cond, ok_bb, fail_bb).unwrap();

            self.builder.position_at_end(fail_bb);
            let msg = b"assertion failed\n\0";
            let msg_const = self.context.const_string(msg, false);
            let msg_global = self.module.add_global(msg_const.get_type(), None, "assert_msg");
            msg_global.set_initializer(&msg_const);
            msg_global.set_constant(true);
            let msg_ptr = msg_global.as_pointer_value();
            self.builder.build_call(printf_fn, &[msg_ptr.into()], "").unwrap();
            let one = i32_ty.const_int(1, false);
            self.builder.build_call(exit_fn, &[one.into()], "").unwrap();
            self.builder.build_unreachable().unwrap();

            self.builder.position_at_end(ok_bb);
            self.builder.build_return(None).unwrap();

            if let Some(b) = saved_block { self.builder.position_at_end(b); }
            self.functions.insert("assert".to_string(), fn_val);
        }

        // Declare write(fd: i32, buf: ptr, count: i64) -> i64 for stderr output.
        let write_ty = i64_ty.fn_type(&[i32_ty.into(), i8_ptr.into(), i64_ty.into()], false);
        let write_fn = self.module.add_function("write", write_ty, None);

        // eprintln: writes string + newline to stderr (fd=2) using write(2, ...).
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("eprintln", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved_block = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            let length = self.builder
                .build_extract_value(str_arg, 0, "ep_len")
                .unwrap()
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            // Write the string content.
            self.builder.build_call(write_fn, &[fd2.into(), data_ptr.into(), length.into()], "").unwrap();
            // Write the newline.
            let nl_arr = self.context.i8_type().array_type(1);
            let nl_g = self.module.add_global(nl_arr, None, "eprintln_nl");
            nl_g.set_initializer(&self.context.i8_type().const_array(&[self.context.i8_type().const_int(b'\n' as u64, false)]));
            nl_g.set_constant(true);
            let nl_ptr = self.builder.build_pointer_cast(nl_g.as_pointer_value(), i8_ptr, "nlptr").unwrap();
            let one64 = i64_ty.const_int(1, false);
            self.builder.build_call(write_fn, &[fd2.into(), nl_ptr.into(), one64.into()], "").unwrap();
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.builder.position_at_end(b); }
            self.functions.insert("eprintln".to_string(), fn_val);
        }

        // eprint: writes string to stderr (fd=2) using write(2, ...) without newline.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("eprint", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved_block = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let str_arg = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder
                .build_extract_value(str_arg, 1, "data_ptr")
                .unwrap()
                .into_pointer_value();
            let length = self.builder
                .build_extract_value(str_arg, 0, "ep_len")
                .unwrap()
                .into_int_value();
            let fd2 = i32_ty.const_int(2, false);
            self.builder.build_call(write_fn, &[fd2.into(), data_ptr.into(), length.into()], "").unwrap();
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved_block { self.builder.position_at_end(b); }
            self.functions.insert("eprint".to_string(), fn_val);
        }

        // C stdlib: int snprintf(char *buf, size_t n, const char *fmt, ...)
        let snprintf_ty = i32_ty.fn_type(&[i8_ptr.into(), i64_ty.into(), i8_ptr.into()], true);
        let snprintf_fn = self.module.add_function("snprintf", snprintf_ty, None);

        // to_str: i64 → { i64, ptr }
        // Uses malloc-allocated buffer so the returned str is heap-owned and
        // remains valid when returned from a function (no dangling static buffer).
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("to_str", fn_ty, None);

            // Get (or re-use) malloc declaration.
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", malloc_ty, None)
            });

            // Format string "%lld\0".
            let fmt_bytes = self.context.const_string(b"%lld", true);
            let fmt_global2 = self.module.add_global(fmt_bytes.get_type(), None, "to_str_fmt");
            fmt_global2.set_initializer(&fmt_bytes);
            fmt_global2.set_constant(true);

            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);

            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let fmt_ptr2 = self.builder
                .build_pointer_cast(fmt_global2.as_pointer_value(), i8_ptr, "fmtptr")
                .unwrap();

            // Pass 1: snprintf(NULL, 0, "%lld", n) → required length (not counting '\0').
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = self.builder
                .build_call(
                    snprintf_fn,
                    &[null_ptr.into(), zero64.into(), fmt_ptr2.into(), n.into()],
                    "snplen",
                )
                .unwrap();
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = self.builder.build_int_z_extend(len_i32, i64_ty, "len64").unwrap();

            // Allocate len + 1 bytes (room for null terminator).
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = self.builder.build_int_add(len_i64, one64, "allocsz").unwrap();
            let buf_call = self.builder
                .build_call(malloc_fn, &[alloc_size.into()], "buf")
                .unwrap();
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%lld", n) → writes the decimal string.
            self.builder
                .build_call(
                    snprintf_fn,
                    &[buf_ptr.into(), alloc_size.into(), fmt_ptr2.into(), n.into()],
                    "snpwrite",
                )
                .unwrap();

            // Build { i64, ptr } return struct.
            let out_alloca = self.builder.build_alloca(str_ty, "out").unwrap();
            let len_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.builder.build_store(len_ptr, len_i64).unwrap();
            self.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.builder.build_load(str_ty, out_alloca, "outval").unwrap();
            self.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("to_str".to_string(), fn_val);
        }

        // to_str_f64: f64 → { i64, ptr } via snprintf("%.6g")
        // Uses malloc-allocated buffer so the returned str is heap-owned.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let f64_ty = self.context.f64_type();
            let fn_ty = str_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.module.add_function("to_str_f64", fn_ty, None);

            // Get (or re-use) malloc declaration.
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", malloc_ty, None)
            });

            let fmt_bytes = self.context.const_string(b"%.6g", true);
            let fmt_global = self.module.add_global(fmt_bytes.get_type(), None, "to_str_f64_fmt");
            fmt_global.set_initializer(&fmt_bytes);
            fmt_global.set_constant(true);

            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);

            let n = fn_val.get_nth_param(0).unwrap().into_float_value();
            let fmt_ptr = self.builder
                .build_pointer_cast(fmt_global.as_pointer_value(), i8_ptr, "fmtptr")
                .unwrap();

            // Pass 1: snprintf(NULL, 0, "%.6g", n) → required length.
            let null_ptr = i8_ptr.const_null();
            let zero64 = i64_ty.const_int(0, false);
            let snp_len = self.builder
                .build_call(
                    snprintf_fn,
                    &[null_ptr.into(), zero64.into(), fmt_ptr.into(), n.into()],
                    "snplen",
                )
                .unwrap();
            let len_i32 = snp_len.try_as_basic_value().left().unwrap().into_int_value();
            let len_i64 = self.builder.build_int_z_extend(len_i32, i64_ty, "len64").unwrap();

            // Allocate len + 1 bytes.
            let one64 = i64_ty.const_int(1, false);
            let alloc_size = self.builder.build_int_add(len_i64, one64, "allocsz").unwrap();
            let buf_call = self.builder
                .build_call(malloc_fn, &[alloc_size.into()], "buf")
                .unwrap();
            let buf_ptr = buf_call.try_as_basic_value().left().unwrap().into_pointer_value();

            // Pass 2: snprintf(buf, len+1, "%.6g", n).
            self.builder
                .build_call(
                    snprintf_fn,
                    &[buf_ptr.into(), alloc_size.into(), fmt_ptr.into(), n.into()],
                    "snpwrite",
                )
                .unwrap();

            let out_alloca = self.builder.build_alloca(str_ty, "out").unwrap();
            let len_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.builder.build_store(len_ptr, len_i64).unwrap();
            self.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.builder.build_load(str_ty, out_alloca, "outval").unwrap();
            self.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("to_str_f64".to_string(), fn_val);
        }

        // assert_eq(a: i64, b: i64): panic if a != b
        {
            let fn_ty = void_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("assert_eq", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.context.append_basic_block(fn_val, "ok");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b_param = fn_val.get_nth_param(1).unwrap().into_int_value();
            let eq = self.builder
                .build_int_compare(IntPredicate::EQ, a, b_param, "eq")
                .unwrap();
            self.builder.build_conditional_branch(eq, ok_bb, fail_bb).unwrap();
            self.builder.position_at_end(fail_bb);
            let msg = self.context.const_string(b"assertion failed: values not equal\n\0", false);
            let msg_g = self.module.add_global(msg.get_type(), None, "assert_eq_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.builder.build_unreachable().unwrap();
            self.builder.position_at_end(ok_bb);
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("assert_eq".to_string(), fn_val);
        }

        // assert_err(tag: i1): panic if tag == 1 (Ok) — expected Err
        {
            let fn_ty = void_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.module.add_function("assert_err", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.context.append_basic_block(fn_val, "ok");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let tag = fn_val.get_nth_param(0).unwrap().into_int_value();
            let is_ok_val = self.builder
                .build_int_compare(IntPredicate::EQ, tag, bool_ty.const_int(1, false), "isok")
                .unwrap();
            self.builder.build_conditional_branch(is_ok_val, fail_bb, ok_bb).unwrap();
            self.builder.position_at_end(fail_bb);
            let msg = self.context.const_string(b"assertion failed: expected Err, got Ok\n\0", false);
            let msg_g = self.module.add_global(msg.get_type(), None, "assert_err_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.builder.build_unreachable().unwrap();
            self.builder.position_at_end(ok_bb);
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("assert_err".to_string(), fn_val);
        }

        // len(s: str) -> i64: extracts the length field (field 0) from the str struct
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("len", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let length = self.builder
                .build_extract_value(s, 0, "len")
                .unwrap()
                .into_int_value();
            self.builder.build_return(Some(&length)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
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
            let strtoll_fn = self.module.add_function("strtoll", strtoll_ty, None);

            // Result<i64, str> LLVM type: { i1, [16 x i8] }
            // The Err case holds a str struct { i64, ptr } which is 16 bytes.
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            let result_ty = self.context.struct_type(
                &[bool_ty.into(), i8_arr16_ty.into()],
                false,
            );

            // parse_int takes a str struct { i64 len, ptr data }.
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("parse_int", fn_ty, None);

            // Basic blocks: entry, ok_bb, err_bb
            let entry_bb = self.context.append_basic_block(fn_val, "pi_entry");
            let ok_bb   = self.context.append_basic_block(fn_val, "pi_ok");
            let err_bb  = self.context.append_basic_block(fn_val, "pi_err");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            // Unpack the str struct.
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder
                .build_extract_value(s, 1, "pi_data")
                .unwrap()
                .into_pointer_value();

            // Allocate an endptr on the stack so strtoll can write to it.
            let endptr_slot = self.builder.build_alloca(i8_ptr, "pi_endptr").unwrap();
            // Null-initialise so strtoll doesn't read garbage.
            let null_ptr = i8_ptr.const_null();
            self.builder.build_store(endptr_slot, null_ptr).unwrap();

            // Cast endptr slot to i8** (same type on all targets).
            let endptr_slot_cast = self.builder
                .build_pointer_cast(endptr_slot, i8_ptr_ptr, "pi_endptr_cast")
                .unwrap();

            // Call strtoll(data, &endptr, 10).
            let base10 = i32_ty.const_int(10, false);
            let strtoll_ret = self.builder
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
            let endptr_val = self.builder
                .build_load(i8_ptr, endptr_slot, "pi_endptr_val")
                .unwrap()
                .into_pointer_value();
            let endptr_int = self.builder
                .build_ptr_to_int(endptr_val, i64_ty, "pi_endptr_int")
                .unwrap();
            let data_int = self.builder
                .build_ptr_to_int(data_ptr, i64_ty, "pi_data_int")
                .unwrap();
            let consumed = self.builder
                .build_int_compare(
                    IntPredicate::NE,
                    endptr_int,
                    data_int,
                    "pi_consumed",
                )
                .unwrap();
            self.builder
                .build_conditional_branch(consumed, ok_bb, err_bb)
                .unwrap();

            // ok_bb: return { tag=1, payload=parsed_i64 as [8 x i8] }
            self.builder.position_at_end(ok_bb);
            let ok_alloca = self.builder.build_alloca(result_ty, "pi_ok_slot").unwrap();
            // Store tag = 1 (i1 true)
            let tag1 = bool_ty.const_int(1, false);
            let tag_ptr_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 0, "pi_tagptr_ok").unwrap();
            self.builder.build_store(tag_ptr_ok, tag1).unwrap();
            // Store the i64 value into the [8 x i8] payload via a pointer cast.
            let payload_ptr_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "pi_payptr_ok").unwrap();
            let payload_i64_ptr = self.builder
                .build_pointer_cast(payload_ptr_ok, i64_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_i64")
                .unwrap();
            self.builder.build_store(payload_i64_ptr, parsed_i64).unwrap();
            let ok_val = self.builder.build_load(result_ty, ok_alloca, "pi_ok_val").unwrap();
            self.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: return { tag=0, payload = str { len=0, ptr=null_byte } }
            self.builder.position_at_end(err_bb);
            let err_alloca = self.builder.build_alloca(result_ty, "pi_err_slot").unwrap();
            let tag0 = bool_ty.const_int(0, false);
            let tag_ptr_err = self.builder.build_struct_gep(result_ty, err_alloca, 0, "pi_tagptr_err").unwrap();
            self.builder.build_store(tag_ptr_err, tag0).unwrap();
            // Store empty str struct { i64=0, ptr=null_byte } into the payload.
            let null_byte_arr = self.context.i8_type().array_type(1);
            let null_byte_g = self.module.add_global(null_byte_arr, None, "pi_null_byte");
            null_byte_g.set_initializer(&self.context.i8_type().const_array(&[self.context.i8_type().const_int(0, false)]));
            null_byte_g.set_constant(true);
            let null_byte_ptr = self.builder
                .build_pointer_cast(null_byte_g.as_pointer_value(), i8_ptr, "pi_null_ptr")
                .unwrap();
            let err_str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let payload_ptr_err = self.builder.build_struct_gep(result_ty, err_alloca, 1, "pi_payptr_err").unwrap();
            let payload_str_ptr = self.builder
                .build_pointer_cast(payload_ptr_err, err_str_ty.ptr_type(inkwell::AddressSpace::default()), "pi_payload_str_err")
                .unwrap();
            let err_str_alloca = self.builder.build_alloca(err_str_ty, "pi_err_str").unwrap();
            let err_str_len_ptr = self.builder.build_struct_gep(err_str_ty, err_str_alloca, 0, "pi_esl").unwrap();
            let err_str_dat_ptr = self.builder.build_struct_gep(err_str_ty, err_str_alloca, 1, "pi_esd").unwrap();
            self.builder.build_store(err_str_len_ptr, i64_ty.const_int(0, false)).unwrap();
            self.builder.build_store(err_str_dat_ptr, null_byte_ptr).unwrap();
            let err_str_val = self.builder.build_load(err_str_ty, err_str_alloca, "pi_err_str_val").unwrap();
            self.builder.build_store(payload_str_ptr, err_str_val).unwrap();
            let err_val = self.builder.build_load(result_ty, err_alloca, "pi_err_val").unwrap();
            self.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
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
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                self.module.add_function("malloc", malloc_ty, None)
            });

            // C stdlib: void *memcpy(void *dst, const void *src, size_t n)
            let memcpy_ty = i8_ptr.fn_type(
                &[i8_ptr.into(), i8_ptr.into(), i64_ty.into()],
                false,
            );
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                self.module.add_function("memcpy", memcpy_ty, None)
            });

            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("axon_concat", fn_ty, None);

            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);

            let a_val = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b_val = fn_val.get_nth_param(1).unwrap().into_struct_value();

            // Extract lengths and data pointers.
            let a_len = self.builder.build_extract_value(a_val, 0, "a_len").unwrap().into_int_value();
            let a_ptr = self.builder.build_extract_value(a_val, 1, "a_ptr").unwrap().into_pointer_value();
            let b_len = self.builder.build_extract_value(b_val, 0, "b_len").unwrap().into_int_value();
            let b_ptr = self.builder.build_extract_value(b_val, 1, "b_ptr").unwrap().into_pointer_value();

            // total_len = a_len + b_len
            let total_len = self.builder.build_int_add(a_len, b_len, "total_len").unwrap();
            // alloc_len = total_len + 1  (null terminator)
            let one64 = i64_ty.const_int(1, false);
            let alloc_len = self.builder.build_int_add(total_len, one64, "alloc_len").unwrap();

            // buf = malloc(alloc_len)
            let buf = self.builder.build_call(malloc_fn, &[alloc_len.into()], "buf").unwrap();
            let buf_ptr = buf.try_as_basic_value().left().unwrap().into_pointer_value();

            // memcpy(buf, a_ptr, a_len)
            self.builder.build_call(
                memcpy_fn,
                &[buf_ptr.into(), a_ptr.into(), a_len.into()],
                "",
            ).unwrap();

            // buf_b = buf + a_len  (GEP to offset into buf)
            let buf_b_ptr = unsafe {
                self.builder.build_gep(
                    self.context.i8_type(),
                    buf_ptr,
                    &[a_len],
                    "buf_b",
                ).unwrap()
            };

            // memcpy(buf_b, b_ptr, b_len)
            self.builder.build_call(
                memcpy_fn,
                &[buf_b_ptr.into(), b_ptr.into(), b_len.into()],
                "",
            ).unwrap();

            // null-terminate: *(buf + total_len) = 0
            let null_pos = unsafe {
                self.builder.build_gep(
                    self.context.i8_type(),
                    buf_ptr,
                    &[total_len],
                    "null_pos",
                ).unwrap()
            };
            self.builder.build_store(null_pos, self.context.i8_type().const_int(0, false)).unwrap();

            // Return { total_len, buf_ptr }
            let out_alloca = self.builder.build_alloca(str_ty, "concat_out").unwrap();
            let len_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 0, "lenptr").unwrap();
            let dat_ptr = self.builder.build_struct_gep(str_ty, out_alloca, 1, "datptr").unwrap();
            self.builder.build_store(len_ptr, total_len).unwrap();
            self.builder.build_store(dat_ptr, buf_ptr).unwrap();
            let out = self.builder.build_load(str_ty, out_alloca, "concat_val").unwrap();
            self.builder.build_return(Some(&out)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("axon_concat".to_string(), fn_val);
        }

        // abs_i32(n: i32) -> i32
        {
            let i32_ty = self.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into()], false);
            let fn_val = self.module.add_function("abs_i32", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i32_ty.const_zero();
            let is_neg = self.builder.build_int_compare(IntPredicate::SLT, n, zero, "isneg").unwrap();
            let neg_n = self.builder.build_int_neg(n, "negn").unwrap();
            let abs_val = self.builder.build_select(is_neg, neg_n, n, "absval").unwrap();
            self.builder.build_return(Some(&abs_val)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("abs_i32".to_string(), fn_val);
            self.fn_return_types.insert("abs_i32".to_string(), Type::I32);
        }

        // abs_f64(n: f64) -> f64
        {
            let f64_ty = self.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.module.add_function("abs_f64", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);
            let n = fn_val.get_nth_param(0).unwrap().into_float_value();
            let zero = f64_ty.const_zero();
            let is_neg = self.builder.build_float_compare(FloatPredicate::OLT, n, zero, "isneg").unwrap();
            let neg_n = self.builder.build_float_neg(n, "negn").unwrap();
            let abs_val = self.builder.build_select(is_neg, neg_n, n, "absval").unwrap();
            self.builder.build_return(Some(&abs_val)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("abs_f64".to_string(), fn_val);
            self.fn_return_types.insert("abs_f64".to_string(), Type::F64);
        }

        // min_i32(a: i32, b: i32) -> i32
        {
            let i32_ty = self.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into(), i32_ty.into()], false);
            let fn_val = self.module.add_function("min_i32", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_lt_b = self.builder.build_int_compare(IntPredicate::SLT, a, b, "altb").unwrap();
            let min_val = self.builder.build_select(a_lt_b, a, b, "minval").unwrap();
            self.builder.build_return(Some(&min_val)).unwrap();
            if let Some(b2) = saved { self.builder.position_at_end(b2); }
            self.functions.insert("min_i32".to_string(), fn_val);
            self.fn_return_types.insert("min_i32".to_string(), Type::I32);
        }

        // max_i32(a: i32, b: i32) -> i32
        {
            let i32_ty = self.context.i32_type();
            let fn_ty = i32_ty.fn_type(&[i32_ty.into(), i32_ty.into()], false);
            let fn_val = self.module.add_function("max_i32", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_gt_b = self.builder.build_int_compare(IntPredicate::SGT, a, b, "agtb").unwrap();
            let max_val = self.builder.build_select(a_gt_b, a, b, "maxval").unwrap();
            self.builder.build_return(Some(&max_val)).unwrap();
            if let Some(b2) = saved { self.builder.position_at_end(b2); }
            self.functions.insert("max_i32".to_string(), fn_val);
            self.fn_return_types.insert("max_i32".to_string(), Type::I32);
        }

        // malloc: void* malloc(i64 size)
        let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
            let ty = i8_ptr.fn_type(&[i64_ty.into()], false);
            self.module.add_function("malloc", ty, None)
        });
        self.functions.insert("malloc".to_string(), malloc_fn);

        // __axon_spawn: void __axon_spawn(fn_ptr: i8*, env_ptr: i8*)
        let spawn_ty = void_ty.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
        let spawn_fn = self.module.add_function("__axon_spawn", spawn_ty, None);
        self.functions.insert("__axon_spawn".to_string(), spawn_fn);

        // __axon_chan_new: i8* __axon_chan_new(capacity: i64)
        let chan_new_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
        let chan_new_fn = self.module.add_function("__axon_chan_new", chan_new_ty, None);
        self.functions.insert("__axon_chan_new".to_string(), chan_new_fn);

        // __axon_chan_send: void __axon_chan_send(chan: i8*, val: i64)
        let chan_send_ty = void_ty.fn_type(&[i8_ptr.into(), i64_ty.into()], false);
        let chan_send_fn = self.module.add_function("__axon_chan_send", chan_send_ty, None);
        self.functions.insert("__axon_chan_send".to_string(), chan_send_fn);

        // __axon_chan_recv: i64 __axon_chan_recv(chan: i8*)
        let chan_recv_ty = i64_ty.fn_type(&[i8_ptr.into()], false);
        let chan_recv_fn = self.module.add_function("__axon_chan_recv", chan_recv_ty, None);
        self.functions.insert("__axon_chan_recv".to_string(), chan_recv_fn);

        // __axon_select: i64 __axon_select(chans: i8**, n: i64)
        // Returns the index of the first ready channel arm.
        let select_ty = i64_ty.fn_type(&[i8_ptr.ptr_type(AddressSpace::default()).into(), i64_ty.into()], false);
        let select_fn = self.module.add_function("__axon_select", select_ty, None);
        self.functions.insert("__axon_select".to_string(), select_fn);

        // __axon_chan_clone: i8* __axon_chan_clone(chan: i8*)
        let chan_clone_ty = i8_ptr.fn_type(&[i8_ptr.into()], false);
        let chan_clone_fn = self.module.add_function("__axon_chan_clone", chan_clone_ty, None);
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
            let i64_ty = self.context.i64_type();
            let i8_ptr = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("format", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let s = fn_val.get_nth_param(0).unwrap();
            self.builder.build_return(Some(&s)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("format".to_string(), fn_val);
            self.fn_return_types.insert("format".to_string(), Type::Str);
        }

        self.fn_return_types.insert("parse_int".to_string(),
            Type::Result(Box::new(Type::I64), Box::new(Type::Str)));
        self.fn_return_types.insert("read_file".to_string(),
            Type::Result(Box::new(Type::Str), Box::new(Type::Str)));
        self.fn_return_types.insert("write_file".to_string(),
            Type::Result(Box::new(Type::Unit), Box::new(Type::Str)));

        // ── Phase 3 math builtins (backed by C libm via LLVM intrinsics) ───
        {
            let f64_ty = self.context.f64_type();
            let f1 = f64_ty.fn_type(&[f64_ty.into()], false);
            let f2 = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);

            let sqrt_fn = self.module.add_function("llvm.sqrt.f64", f1, None);
            self.functions.insert("sqrt".to_string(), sqrt_fn);
            self.fn_return_types.insert("sqrt".to_string(), Type::F64);

            let pow_fn = self.module.add_function("llvm.pow.f64", f2, None);
            self.functions.insert("pow".to_string(), pow_fn);
            self.fn_return_types.insert("pow".to_string(), Type::F64);

            let floor_fn = self.module.add_function("llvm.floor.f64", f1, None);
            self.functions.insert("floor".to_string(), floor_fn);
            self.fn_return_types.insert("floor".to_string(), Type::F64);

            let ceil_fn = self.module.add_function("llvm.ceil.f64", f1, None);
            self.functions.insert("ceil".to_string(), ceil_fn);
            self.fn_return_types.insert("ceil".to_string(), Type::F64);
        }

        // assert_eq_f64 — panic if two f64 values differ.
        {
            let f64_ty = self.context.f64_type();
            let fn_ty = void_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.module.add_function("assert_eq_f64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "entry");
            let fail_bb = self.context.append_basic_block(fn_val, "fail");
            let ok_bb = self.context.append_basic_block(fn_val, "ok");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_float_value();
            let b_param = fn_val.get_nth_param(1).unwrap().into_float_value();
            let eq = self.builder.build_float_compare(FloatPredicate::OEQ, a, b_param, "eq").unwrap();
            self.builder.build_conditional_branch(eq, ok_bb, fail_bb).unwrap();
            self.builder.position_at_end(fail_bb);
            let msg = self.context.const_string(b"assertion failed: f64 values not equal\n\0", false);
            let msg_g = self.module.add_global(msg.get_type(), None, "assert_eq_f64_msg");
            msg_g.set_initializer(&msg);
            msg_g.set_constant(true);
            self.builder.build_call(printf_fn, &[msg_g.as_pointer_value().into()], "").unwrap();
            self.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.builder.build_unreachable().unwrap();
            self.builder.position_at_end(ok_bb);
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("assert_eq_f64".to_string(), fn_val);
            self.fn_return_types.insert("assert_eq_f64".to_string(), Type::Unit);
        }

        // assert_eq_str — panic if two str values differ (compare len then bytes via memcmp).
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = void_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("assert_eq_str", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "entry");
            let len_fail_bb = self.context.append_basic_block(fn_val, "len_fail");
            let cmp_bb = self.context.append_basic_block(fn_val, "cmp");
            let bytes_fail_bb = self.context.append_basic_block(fn_val, "bytes_fail");
            let ok_bb = self.context.append_basic_block(fn_val, "ok");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a_struct = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b_struct = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let a_len = self.builder.build_extract_value(a_struct, 0, "a_len").unwrap().into_int_value();
            let b_len = self.builder.build_extract_value(b_struct, 0, "b_len").unwrap().into_int_value();
            let len_eq = self.builder.build_int_compare(IntPredicate::EQ, a_len, b_len, "len_eq").unwrap();
            self.builder.build_conditional_branch(len_eq, cmp_bb, len_fail_bb).unwrap();
            // lengths differ → fail
            self.builder.position_at_end(len_fail_bb);
            let fail_msg = self.context.const_string(b"assert_eq_str failed: lengths differ\n\0", false);
            let fail_g = self.module.add_global(fail_msg.get_type(), None, "aeqs_len_msg");
            fail_g.set_initializer(&fail_msg);
            fail_g.set_constant(true);
            self.builder.build_call(printf_fn, &[fail_g.as_pointer_value().into()], "").unwrap();
            self.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.builder.build_unreachable().unwrap();
            // same length — compare bytes via memcmp
            self.builder.position_at_end(cmp_bb);
            let a_ptr = self.builder.build_extract_value(a_struct, 1, "a_ptr").unwrap().into_pointer_value();
            let b_ptr = self.builder.build_extract_value(b_struct, 1, "b_ptr").unwrap().into_pointer_value();
            let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
            let memcmp_fn = self.module.get_function("memcmp").unwrap_or_else(|| {
                self.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = self.builder.build_call(memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "cmp").unwrap().try_as_basic_value().left().unwrap().into_int_value();
            let zero32 = i32_ty.const_zero();
            let bytes_eq = self.builder.build_int_compare(IntPredicate::EQ, cmp_result, zero32, "bytes_eq").unwrap();
            self.builder.build_conditional_branch(bytes_eq, ok_bb, bytes_fail_bb).unwrap();
            // bytes differ → fail
            self.builder.position_at_end(bytes_fail_bb);
            let bytes_msg = self.context.const_string(b"assert_eq_str failed: bytes differ\n\0", false);
            let bytes_g = self.module.add_global(bytes_msg.get_type(), None, "aeqs_bytes_msg");
            bytes_g.set_initializer(&bytes_msg);
            bytes_g.set_constant(true);
            self.builder.build_call(printf_fn, &[bytes_g.as_pointer_value().into()], "").unwrap();
            self.builder.build_call(exit_fn, &[i32_ty.const_int(1, false).into()], "").unwrap();
            self.builder.build_unreachable().unwrap();
            self.builder.position_at_end(ok_bb);
            self.builder.build_return(None).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("assert_eq_str".to_string(), fn_val);
            self.fn_return_types.insert("assert_eq_str".to_string(), Type::Unit);
        }

        // ── Phase 4: time builtins ─────────────────────────────────────────────
        {
            let i64_ty = self.context.i64_type();
            // sleep_ms(ms: i64) -> ()
            let sleep_ty = void_ty.fn_type(&[i64_ty.into()], false);
            let sleep_fn = self.module.add_function("__axon_sleep_ms", sleep_ty, None);
            self.functions.insert("sleep_ms".to_string(), sleep_fn);
            self.fn_return_types.insert("sleep_ms".to_string(), Type::Unit);

            // now_ms() -> i64
            let now_ty = i64_ty.fn_type(&[], false);
            let now_fn = self.module.add_function("__axon_now_ms", now_ty, None);
            self.functions.insert("now_ms".to_string(), now_fn);
            self.fn_return_types.insert("now_ms".to_string(), Type::I64);
        }

        // ── Phase 4: read_line() -> str ────────────────────────────────────────
        // The runtime function `__axon_read_line(out_len: *i64, out_ptr: **u8)` allocates
        // a heap buffer. The codegen wrapper allocates the out-params on the stack and
        // packages the result into the Axon `{ i64, i8* }` str struct.
        {
            let i64_ty = self.context.i64_type();
            let i8_ptr  = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty  = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let rt_ty = void_ty.fn_type(&[i64_ptr.into(), i8_ptr_ptr.into()], false);
            let rt_fn = self.module.add_function("__axon_read_line", rt_ty, None);

            let fn_ty = str_ty.fn_type(&[], false);
            let fn_val = self.module.add_function("read_line", fn_ty, None);
            let entry = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry);

            let len_slot = self.builder.build_alloca(i64_ty, "read_len").unwrap();
            let ptr_slot = self.builder.build_alloca(i8_ptr, "read_ptr").unwrap();
            let ptr_slot_cast = self.builder.build_pointer_cast(ptr_slot, i8_ptr_ptr, "ptrptr").unwrap();
            self.builder.build_call(rt_fn, &[len_slot.into(), ptr_slot_cast.into()], "").unwrap();

            let len_val = self.builder.build_load(i64_ty, len_slot, "len").unwrap().into_int_value();
            let ptr_val = self.builder.build_load(i8_ptr, ptr_slot, "ptr").unwrap().into_pointer_value();

            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, len_val, 0, "str0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, ptr_val, 1, "str1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
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
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            let result_ty = self.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.module.add_function("__axon_read_file", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("read_file", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "rf_entry");
            let ok_bb    = self.context.append_basic_block(fn_val, "rf_ok");
            let err_bb   = self.context.append_basic_block(fn_val, "rf_err");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let path_str = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let path_len = self.builder.build_extract_value(path_str, 0, "rf_plen").unwrap().into_int_value();
            let path_ptr_v = self.builder.build_extract_value(path_str, 1, "rf_pptr").unwrap().into_pointer_value();

            let out_len_slot = self.builder.build_alloca(i64_ty, "rf_out_len").unwrap();
            let out_ptr_slot = self.builder.build_alloca(i8_ptr, "rf_out_ptr").unwrap();
            let out_ptr_cast = self.builder.build_pointer_cast(out_ptr_slot, i8_ptr_ptr, "rf_ptrptr").unwrap();
            self.builder.build_call(rt_fn, &[path_ptr_v.into(), path_len.into(), out_len_slot.into(), out_ptr_cast.into()], "").unwrap();

            let out_len = self.builder.build_load(i64_ty, out_len_slot, "rf_len").unwrap().into_int_value();
            let out_ptr = self.builder.build_load(i8_ptr, out_ptr_slot, "rf_ptr").unwrap().into_pointer_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = self.builder.build_int_compare(inkwell::IntPredicate::SGE, out_len, zero_i64, "rf_is_ok").unwrap();
            self.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=str{out_len, out_ptr} }
            self.builder.position_at_end(ok_bb);
            let ok_alloca = self.builder.build_alloca(result_ty, "rf_ok_slot").unwrap();
            let tag_ptr_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 0, "rf_tag_ok").unwrap();
            self.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "rf_pay_ok").unwrap();
            let str_ok_ptr = self.builder.build_pointer_cast(payload_ok, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_ok_ptr").unwrap();
            let str_ok_slot = self.builder.build_alloca(str_ty, "rf_str_ok").unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_ok_slot, 0, "").unwrap(), out_len).unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_ok_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_ok_val = self.builder.build_load(str_ty, str_ok_slot, "rf_str_ok_val").unwrap();
            self.builder.build_store(str_ok_ptr, str_ok_val).unwrap();
            let ok_val = self.builder.build_load(result_ty, ok_alloca, "rf_ok_val").unwrap();
            self.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: negate len, { tag=0, payload=str{|len|, out_ptr} }
            self.builder.position_at_end(err_bb);
            let actual_len = self.builder.build_int_neg(out_len, "rf_actual_len").unwrap();
            let err_alloca = self.builder.build_alloca(result_ty, "rf_err_slot").unwrap();
            let tag_ptr_err = self.builder.build_struct_gep(result_ty, err_alloca, 0, "rf_tag_err").unwrap();
            self.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.builder.build_struct_gep(result_ty, err_alloca, 1, "rf_pay_err").unwrap();
            let str_err_ptr = self.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "rf_str_err_ptr").unwrap();
            let str_err_slot = self.builder.build_alloca(str_ty, "rf_str_err").unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), actual_len).unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), out_ptr).unwrap();
            let str_err_val = self.builder.build_load(str_ty, str_err_slot, "rf_str_err_val").unwrap();
            self.builder.build_store(str_err_ptr, str_err_val).unwrap();
            let err_val = self.builder.build_load(result_ty, err_alloca, "rf_err_val").unwrap();
            self.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("read_file".to_string(), fn_val);
        }

        // ── Phase 4: write_file(path: str, content: str) -> Result<(), str> ───
        // Runtime: __axon_write_file(path_ptr, path_len, content_ptr, content_len, out_err_len: *i64, out_err_ptr: **u8)
        // err_len==0 → Ok(()); err_len>0 → Err(str{err_len, err_ptr})
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            let result_ty = self.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let rt_ty = void_ty.fn_type(
                &[i8_ptr.into(), i64_ty.into(), i8_ptr.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.module.add_function("__axon_write_file", rt_ty, None);

            let fn_ty = result_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("write_file", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "wf_entry");
            let ok_bb    = self.context.append_basic_block(fn_val, "wf_ok");
            let err_bb   = self.context.append_basic_block(fn_val, "wf_err");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let path_str    = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let content_str = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let path_len    = self.builder.build_extract_value(path_str, 0, "wf_plen").unwrap().into_int_value();
            let path_ptr_v  = self.builder.build_extract_value(path_str, 1, "wf_pptr").unwrap().into_pointer_value();
            let cont_len    = self.builder.build_extract_value(content_str, 0, "wf_clen").unwrap().into_int_value();
            let cont_ptr    = self.builder.build_extract_value(content_str, 1, "wf_cptr").unwrap().into_pointer_value();

            let err_len_slot = self.builder.build_alloca(i64_ty, "wf_err_len").unwrap();
            let err_ptr_slot = self.builder.build_alloca(i8_ptr, "wf_err_ptr").unwrap();
            let err_ptr_cast = self.builder.build_pointer_cast(err_ptr_slot, i8_ptr_ptr, "wf_ptrptr").unwrap();
            self.builder.build_store(err_len_slot, i64_ty.const_int(0, false)).unwrap();
            self.builder.build_store(err_ptr_slot, i8_ptr.const_null()).unwrap();

            self.builder.build_call(rt_fn, &[path_ptr_v.into(), path_len.into(), cont_ptr.into(), cont_len.into(), err_len_slot.into(), err_ptr_cast.into()], "").unwrap();

            let err_len = self.builder.build_load(i64_ty, err_len_slot, "wf_err_len_val").unwrap().into_int_value();
            let zero_i64 = i64_ty.const_int(0, false);
            let is_ok = self.builder.build_int_compare(inkwell::IntPredicate::EQ, err_len, zero_i64, "wf_is_ok").unwrap();
            self.builder.build_conditional_branch(is_ok, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=zeroed }
            self.builder.position_at_end(ok_bb);
            let ok_alloca = self.builder.build_alloca(result_ty, "wf_ok_slot").unwrap();
            let tag_ptr_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 0, "wf_tag_ok").unwrap();
            self.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "wf_pay_ok").unwrap();
            let zero_arr = self.context.i8_type().array_type(16).const_zero();
            self.builder.build_store(payload_ok, zero_arr).unwrap();
            let ok_val = self.builder.build_load(result_ty, ok_alloca, "wf_ok_val").unwrap();
            self.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: { tag=0, payload=str{err_len, err_ptr} }
            self.builder.position_at_end(err_bb);
            let err_ptr = self.builder.build_load(i8_ptr, err_ptr_slot, "wf_err_ptr_val").unwrap().into_pointer_value();
            let err_alloca = self.builder.build_alloca(result_ty, "wf_err_slot").unwrap();
            let tag_ptr_err = self.builder.build_struct_gep(result_ty, err_alloca, 0, "wf_tag_err").unwrap();
            self.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.builder.build_struct_gep(result_ty, err_alloca, 1, "wf_pay_err").unwrap();
            let str_err_ptr = self.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "wf_str_err_ptr").unwrap();
            let str_err_slot = self.builder.build_alloca(str_ty, "wf_str_err").unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_err_slot, 0, "").unwrap(), err_len).unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, str_err_slot, 1, "").unwrap(), err_ptr).unwrap();
            let str_err_val = self.builder.build_load(str_ty, str_err_slot, "wf_str_err_val").unwrap();
            self.builder.build_store(str_err_ptr, str_err_val).unwrap();
            let err_val = self.builder.build_load(result_ty, err_alloca, "wf_err_val").unwrap();
            self.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("write_file".to_string(), fn_val);
        }

        // ── Phase 5: String builtins ──────────────────────────────────────────

        // str_eq(a: str, b: str) -> bool
        // Compare two strings for byte-equal content. Uses memcmp after length check.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_eq", fn_ty, None);

            let entry_bb  = self.context.append_basic_block(fn_val, "se_entry");
            let cmp_bb    = self.context.append_basic_block(fn_val, "se_cmp");
            let true_bb   = self.context.append_basic_block(fn_val, "se_true");
            let false_bb  = self.context.append_basic_block(fn_val, "se_false");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let a = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let b = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let a_len = self.builder.build_extract_value(a, 0, "se_alen").unwrap().into_int_value();
            let b_len = self.builder.build_extract_value(b, 0, "se_blen").unwrap().into_int_value();
            let a_ptr = self.builder.build_extract_value(a, 1, "se_aptr").unwrap().into_pointer_value();
            let b_ptr = self.builder.build_extract_value(b, 1, "se_bptr").unwrap().into_pointer_value();

            // If lengths differ → false immediately.
            let lens_eq = self.builder.build_int_compare(inkwell::IntPredicate::EQ, a_len, b_len, "se_leneq").unwrap();
            self.builder.build_conditional_branch(lens_eq, cmp_bb, false_bb).unwrap();

            // Same length → call memcmp.
            self.builder.position_at_end(cmp_bb);
            let memcmp_fn = self.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp_result = self.builder.build_call(memcmp_fn, &[a_ptr.into(), b_ptr.into(), a_len.into()], "se_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp_result, i32_ty.const_int(0, false), "se_iszero").unwrap();
            self.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.builder.position_at_end(true_bb);
            self.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();

            self.builder.position_at_end(false_bb);
            self.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_eq".to_string(), fn_val);
            self.fn_return_types.insert("str_eq".to_string(), Type::Bool);
        }

        // str_contains(s: str, needle: str) -> bool
        // Uses memmem-like loop: slide needle over s, compare with memcmp.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_contains", fn_ty, None);

            // We implement via strstr(3) since strings are null-terminated.
            // strstr returns a non-null pointer if needle is found.
            let strstr_fn = self.module.get_function("strstr").unwrap_or_else(|| {
                let strstr_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.module.add_function("strstr", strstr_ty, None)
            });

            let entry_bb = self.context.append_basic_block(fn_val, "sc_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "sc_sptr").unwrap().into_pointer_value();
            let n_ptr = self.builder.build_extract_value(needle, 1, "sc_nptr").unwrap().into_pointer_value();

            let found = self.builder.build_call(strstr_fn, &[s_ptr.into(), n_ptr.into()], "sc_found").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let null = i8_ptr.const_null();
            let found_int = self.builder.build_ptr_to_int(found, i64_ty, "sc_found_int").unwrap();
            let null_int  = self.builder.build_ptr_to_int(null,  i64_ty, "sc_null_int").unwrap();
            let is_found  = self.builder.build_int_compare(inkwell::IntPredicate::NE, found_int, null_int, "sc_is_found").unwrap();
            let result = self.builder.build_int_z_extend(is_found, bool_ty, "sc_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_contains".to_string(), fn_val);
            self.fn_return_types.insert("str_contains".to_string(), Type::Bool);
        }

        // str_starts_with(s: str, prefix: str) -> bool
        // len(s) >= len(prefix) && memcmp(s.ptr, prefix.ptr, len(prefix)) == 0
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_starts_with", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "ssw_entry");
            let cmp_bb   = self.context.append_basic_block(fn_val, "ssw_cmp");
            let true_bb  = self.context.append_basic_block(fn_val, "ssw_true");
            let false_bb = self.context.append_basic_block(fn_val, "ssw_false");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let p = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_len = self.builder.build_extract_value(s, 0, "ssw_slen").unwrap().into_int_value();
            let p_len = self.builder.build_extract_value(p, 0, "ssw_plen").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "ssw_sptr").unwrap().into_pointer_value();
            let p_ptr = self.builder.build_extract_value(p, 1, "ssw_pptr").unwrap().into_pointer_value();

            // s_len >= p_len?
            let long_enough = self.builder.build_int_compare(inkwell::IntPredicate::SGE, s_len, p_len, "ssw_longenough").unwrap();
            self.builder.build_conditional_branch(long_enough, cmp_bb, false_bb).unwrap();

            self.builder.position_at_end(cmp_bb);
            let memcmp_fn = self.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp = self.builder.build_call(memcmp_fn, &[s_ptr.into(), p_ptr.into(), p_len.into()], "ssw_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp, i32_ty.const_int(0, false), "ssw_iszero").unwrap();
            self.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.builder.position_at_end(true_bb);
            self.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();
            self.builder.position_at_end(false_bb);
            self.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_starts_with".to_string(), fn_val);
            self.fn_return_types.insert("str_starts_with".to_string(), Type::Bool);
        }

        // str_ends_with(s: str, suffix: str) -> bool
        // len(s) >= len(suffix) && memcmp(s.ptr + len(s) - len(suffix), suffix.ptr, len(suffix)) == 0
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = bool_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_ends_with", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "sew_entry");
            let cmp_bb   = self.context.append_basic_block(fn_val, "sew_cmp");
            let true_bb  = self.context.append_basic_block(fn_val, "sew_true");
            let false_bb = self.context.append_basic_block(fn_val, "sew_false");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let sf = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_len  = self.builder.build_extract_value(s, 0, "sew_slen").unwrap().into_int_value();
            let sf_len = self.builder.build_extract_value(sf, 0, "sew_sflen").unwrap().into_int_value();
            let s_ptr  = self.builder.build_extract_value(s, 1, "sew_sptr").unwrap().into_pointer_value();
            let sf_ptr = self.builder.build_extract_value(sf, 1, "sew_sfptr").unwrap().into_pointer_value();

            let long_enough = self.builder.build_int_compare(inkwell::IntPredicate::SGE, s_len, sf_len, "sew_longenough").unwrap();
            self.builder.build_conditional_branch(long_enough, cmp_bb, false_bb).unwrap();

            self.builder.position_at_end(cmp_bb);
            // offset = s_len - sf_len; start = s.ptr + offset
            let offset = self.builder.build_int_sub(s_len, sf_len, "sew_offset").unwrap();
            let start = unsafe {
                self.builder.build_gep(self.context.i8_type(), s_ptr, &[offset], "sew_start").unwrap()
            };
            let memcmp_fn = self.module.get_function("memcmp").unwrap_or_else(|| {
                let memcmp_ty = i32_ty.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcmp", memcmp_ty, None)
            });
            let cmp = self.builder.build_call(memcmp_fn, &[start.into(), sf_ptr.into(), sf_len.into()], "sew_cmp").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let is_zero = self.builder.build_int_compare(inkwell::IntPredicate::EQ, cmp, i32_ty.const_int(0, false), "sew_iszero").unwrap();
            self.builder.build_conditional_branch(is_zero, true_bb, false_bb).unwrap();

            self.builder.position_at_end(true_bb);
            self.builder.build_return(Some(&bool_ty.const_int(1, false))).unwrap();
            self.builder.position_at_end(false_bb);
            self.builder.build_return(Some(&bool_ty.const_int(0, false))).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_ends_with".to_string(), fn_val);
            self.fn_return_types.insert("str_ends_with".to_string(), Type::Bool);
        }

        // str_slice(s: str, start: i64, end: i64) -> str
        // Returns heap-allocated substring. Clamps start/end to [0, len].
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("str_slice", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "ss_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let start = fn_val.get_nth_param(1).unwrap().into_int_value();
            let end   = fn_val.get_nth_param(2).unwrap().into_int_value();
            let s_len = self.builder.build_extract_value(s, 0, "ss_slen").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "ss_sptr").unwrap().into_pointer_value();

            // Clamp start to [0, s_len]
            let zero = i64_ty.const_int(0, false);
            let start_pos = self.builder.build_int_compare(inkwell::IntPredicate::SLT, start, zero, "ss_s_neg").unwrap();
            let start_clamped_lo = self.builder.build_select(start_pos, zero, start, "ss_start_lo").unwrap().into_int_value();
            let start_gt = self.builder.build_int_compare(inkwell::IntPredicate::SGT, start_clamped_lo, s_len, "ss_s_gt").unwrap();
            let start_clamped = self.builder.build_select(start_gt, s_len, start_clamped_lo, "ss_start").unwrap().into_int_value();

            // Clamp end to [start_clamped, s_len]
            let end_lt = self.builder.build_int_compare(inkwell::IntPredicate::SLT, end, start_clamped, "ss_e_lt").unwrap();
            let end_clamped_lo = self.builder.build_select(end_lt, start_clamped, end, "ss_end_lo").unwrap().into_int_value();
            let end_gt = self.builder.build_int_compare(inkwell::IntPredicate::SGT, end_clamped_lo, s_len, "ss_e_gt").unwrap();
            let end_clamped = self.builder.build_select(end_gt, s_len, end_clamped_lo, "ss_end").unwrap().into_int_value();

            // slice_len = end_clamped - start_clamped
            let slice_len = self.builder.build_int_sub(end_clamped, start_clamped, "ss_slicelen").unwrap();

            // Allocate slice_len + 1 bytes via malloc.
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", malloc_ty, None)
            });
            let alloc_size = self.builder.build_int_add(slice_len, i64_ty.const_int(1, false), "ss_alloc").unwrap();
            let buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "ss_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            // src = s_ptr + start_clamped
            let src_ptr = unsafe {
                self.builder.build_gep(self.context.i8_type(), s_ptr, &[start_clamped], "ss_src").unwrap()
            };

            // memcpy(buf, src, slice_len)
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcpy", memcpy_ty, None)
            });
            self.builder.build_call(memcpy_fn, &[buf.into(), src_ptr.into(), slice_len.into()], "").unwrap();

            // Null-terminate: buf[slice_len] = 0
            let null_byte = self.context.i8_type().const_int(0, false);
            let null_pos = unsafe {
                self.builder.build_gep(self.context.i8_type(), buf, &[slice_len], "ss_null_pos").unwrap()
            };
            self.builder.build_store(null_pos, null_byte).unwrap();

            // Build result str struct { slice_len, buf }
            let result_alloca = self.builder.build_alloca(str_ty, "ss_result").unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, result_alloca, 0, "").unwrap(), slice_len).unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, result_alloca, 1, "").unwrap(), buf).unwrap();
            let result = self.builder.build_load(str_ty, result_alloca, "ss_result_val").unwrap();
            self.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_slice".to_string(), fn_val);
            self.fn_return_types.insert("str_slice".to_string(), Type::Str);
        }

        // str_index_of(s: str, needle: str) -> i64
        // Returns byte index of first occurrence of needle in s, or -1 if not found.
        // Uses strstr and pointer arithmetic.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_index_of", fn_ty, None);

            let entry_bb    = self.context.append_basic_block(fn_val, "sio_entry");
            let found_bb    = self.context.append_basic_block(fn_val, "sio_found");
            let notfound_bb = self.context.append_basic_block(fn_val, "sio_notfound");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr  = self.builder.build_extract_value(s, 1, "sio_sptr").unwrap().into_pointer_value();
            let n_ptr  = self.builder.build_extract_value(needle, 1, "sio_nptr").unwrap().into_pointer_value();

            let strstr_fn = self.module.get_function("strstr").unwrap_or_else(|| {
                let strstr_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.module.add_function("strstr", strstr_ty, None)
            });
            let found = self.builder.build_call(strstr_fn, &[s_ptr.into(), n_ptr.into()], "sio_found_ptr").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let null = i8_ptr.const_null();
            let found_int = self.builder.build_ptr_to_int(found, i64_ty, "sio_fi").unwrap();
            let null_int  = self.builder.build_ptr_to_int(null, i64_ty, "sio_ni").unwrap();
            let s_int     = self.builder.build_ptr_to_int(s_ptr, i64_ty, "sio_si").unwrap();
            let is_found  = self.builder.build_int_compare(inkwell::IntPredicate::NE, found_int, null_int, "sio_is_found").unwrap();
            self.builder.build_conditional_branch(is_found, found_bb, notfound_bb).unwrap();

            self.builder.position_at_end(found_bb);
            let offset = self.builder.build_int_sub(found_int, s_int, "sio_offset").unwrap();
            self.builder.build_return(Some(&offset)).unwrap();

            self.builder.position_at_end(notfound_bb);
            self.builder.build_return(Some(&i64_ty.const_int(u64::MAX, true))).unwrap(); // -1 as i64

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_index_of".to_string(), fn_val);
            self.fn_return_types.insert("str_index_of".to_string(), Type::I64);
        }

        // char_at(s: str, i: i64) -> i64
        // Returns byte value at index i, or -1 if out of bounds.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("char_at", fn_ty, None);

            let entry_bb   = self.context.append_basic_block(fn_val, "ca_entry");
            let inbounds_bb = self.context.append_basic_block(fn_val, "ca_inbounds");
            let oob_bb     = self.context.append_basic_block(fn_val, "ca_oob");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let idx   = fn_val.get_nth_param(1).unwrap().into_int_value();
            let s_len = self.builder.build_extract_value(s, 0, "ca_len").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "ca_ptr").unwrap().into_pointer_value();

            // Check 0 <= idx < s_len
            let zero = i64_ty.const_int(0, false);
            let ge_zero = self.builder.build_int_compare(inkwell::IntPredicate::SGE, idx, zero, "ca_gez").unwrap();
            let lt_len  = self.builder.build_int_compare(inkwell::IntPredicate::SLT, idx, s_len, "ca_ltl").unwrap();
            let in_bounds = self.builder.build_and(ge_zero, lt_len, "ca_inb").unwrap();
            self.builder.build_conditional_branch(in_bounds, inbounds_bb, oob_bb).unwrap();

            self.builder.position_at_end(inbounds_bb);
            let byte_ptr = unsafe {
                self.builder.build_gep(self.context.i8_type(), s_ptr, &[idx], "ca_byteptr").unwrap()
            };
            let byte_val = self.builder.build_load(self.context.i8_type(), byte_ptr, "ca_byte").unwrap().into_int_value();
            // zero-extend i8 to i64
            let byte_i64 = self.builder.build_int_z_extend(byte_val, i64_ty, "ca_byte_i64").unwrap();
            self.builder.build_return(Some(&byte_i64)).unwrap();

            self.builder.position_at_end(oob_bb);
            self.builder.build_return(Some(&i64_ty.const_int(u64::MAX, true))).unwrap(); // -1

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("char_at".to_string(), fn_val);
            self.fn_return_types.insert("char_at".to_string(), Type::I64);
        }

        // to_str_bool(b: bool) -> str
        // Returns str "true" or "false" (global string constants).
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[bool_ty.into()], false);
            let fn_val = self.module.add_function("to_str_bool", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "tsb_entry");
            let true_bb  = self.context.append_basic_block(fn_val, "tsb_true");
            let false_bb = self.context.append_basic_block(fn_val, "tsb_false");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let b = fn_val.get_nth_param(0).unwrap().into_int_value();
            let is_true = self.builder.build_int_compare(inkwell::IntPredicate::NE, b, bool_ty.const_int(0, false), "tsb_cond").unwrap();
            self.builder.build_conditional_branch(is_true, true_bb, false_bb).unwrap();

            // Declare "true\0" and "false\0" as global string constants.
            let true_bytes: Vec<_> = b"true\0".iter().map(|&c| self.context.i8_type().const_int(c as u64, false)).collect();
            let false_bytes: Vec<_> = b"false\0".iter().map(|&c| self.context.i8_type().const_int(c as u64, false)).collect();
            let true_g = self.module.add_global(self.context.i8_type().array_type(5), None, "tsb_true_str");
            true_g.set_initializer(&self.context.i8_type().const_array(&true_bytes));
            true_g.set_constant(true);
            let false_g = self.module.add_global(self.context.i8_type().array_type(6), None, "tsb_false_str");
            false_g.set_initializer(&self.context.i8_type().const_array(&false_bytes));
            false_g.set_constant(true);

            self.builder.position_at_end(true_bb);
            let true_ptr = self.builder.build_pointer_cast(true_g.as_pointer_value(), i8_ptr, "tsb_tptr").unwrap();
            let mut true_str = str_ty.get_undef();
            true_str = self.builder.build_insert_value(true_str, i64_ty.const_int(4, false), 0, "tsb_t0").unwrap().into_struct_value();
            true_str = self.builder.build_insert_value(true_str, true_ptr, 1, "tsb_t1").unwrap().into_struct_value();
            self.builder.build_return(Some(&true_str)).unwrap();

            self.builder.position_at_end(false_bb);
            let false_ptr = self.builder.build_pointer_cast(false_g.as_pointer_value(), i8_ptr, "tsb_fptr").unwrap();
            let mut false_str = str_ty.get_undef();
            false_str = self.builder.build_insert_value(false_str, i64_ty.const_int(5, false), 0, "tsb_f0").unwrap().into_struct_value();
            false_str = self.builder.build_insert_value(false_str, false_ptr, 1, "tsb_f1").unwrap().into_struct_value();
            self.builder.build_return(Some(&false_str)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("to_str_bool".to_string(), fn_val);
            self.fn_return_types.insert("to_str_bool".to_string(), Type::Str);
        }

        // ── Phase 5: parse_float(s: str) -> Result<f64, str> ─────────────────
        // Uses strtod; endptr check detects parse failure.
        // Result<f64, str> = { i1, [16 x i8] } (f64=8 bytes, str=16 bytes → max=16)
        {
            let f64_ty = self.context.f64_type();
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            let result_ty = self.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);

            let strtod_ty = f64_ty.fn_type(&[i8_ptr.into(), i8_ptr_ptr.into()], false);
            let strtod_fn = self.module.get_function("strtod").unwrap_or_else(|| {
                self.module.add_function("strtod", strtod_ty, None)
            });

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("parse_float", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "pf_entry");
            let ok_bb    = self.context.append_basic_block(fn_val, "pf_ok");
            let err_bb   = self.context.append_basic_block(fn_val, "pf_err");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let data_ptr = self.builder.build_extract_value(s, 1, "pf_data").unwrap().into_pointer_value();

            let endptr_slot = self.builder.build_alloca(i8_ptr, "pf_endptr").unwrap();
            self.builder.build_store(endptr_slot, i8_ptr.const_null()).unwrap();
            let endptr_cast = self.builder.build_pointer_cast(endptr_slot, i8_ptr_ptr, "pf_endptr_cast").unwrap();

            let parsed_f64 = self.builder.build_call(strtod_fn, &[data_ptr.into(), endptr_cast.into()], "pf_strtod").unwrap()
                .try_as_basic_value().left().unwrap().into_float_value();

            let endptr_val = self.builder.build_load(i8_ptr, endptr_slot, "pf_endptr_val").unwrap().into_pointer_value();
            let endptr_int = self.builder.build_ptr_to_int(endptr_val, i64_ty, "pf_ep_int").unwrap();
            let data_int   = self.builder.build_ptr_to_int(data_ptr, i64_ty, "pf_data_int").unwrap();
            let consumed   = self.builder.build_int_compare(inkwell::IntPredicate::NE, endptr_int, data_int, "pf_consumed").unwrap();
            self.builder.build_conditional_branch(consumed, ok_bb, err_bb).unwrap();

            // ok_bb: { tag=1, payload=f64 as [16 x i8] }
            self.builder.position_at_end(ok_bb);
            let ok_alloca = self.builder.build_alloca(result_ty, "pf_ok_slot").unwrap();
            let tag_ptr_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 0, "pf_tag_ok").unwrap();
            self.builder.build_store(tag_ptr_ok, bool_ty.const_int(1, false)).unwrap();
            let payload_ok = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "pf_pay_ok").unwrap();
            let f64_ptr = self.builder.build_pointer_cast(payload_ok, f64_ty.ptr_type(inkwell::AddressSpace::default()), "pf_f64_ptr").unwrap();
            self.builder.build_store(f64_ptr, parsed_f64).unwrap();
            let ok_val = self.builder.build_load(result_ty, ok_alloca, "pf_ok_val").unwrap();
            self.builder.build_return(Some(&ok_val)).unwrap();

            // err_bb: { tag=0, payload=str{len=0, ptr=null} }
            self.builder.position_at_end(err_bb);
            let err_alloca = self.builder.build_alloca(result_ty, "pf_err_slot").unwrap();
            let tag_ptr_err = self.builder.build_struct_gep(result_ty, err_alloca, 0, "pf_tag_err").unwrap();
            self.builder.build_store(tag_ptr_err, bool_ty.const_int(0, false)).unwrap();
            let payload_err = self.builder.build_struct_gep(result_ty, err_alloca, 1, "pf_pay_err").unwrap();
            let err_str_ptr = self.builder.build_pointer_cast(payload_err, str_ty.ptr_type(inkwell::AddressSpace::default()), "pf_str_err_ptr").unwrap();
            let err_str_slot = self.builder.build_alloca(str_ty, "pf_str_err").unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, err_str_slot, 0, "").unwrap(), i64_ty.const_int(0, false)).unwrap();
            self.builder.build_store(self.builder.build_struct_gep(str_ty, err_str_slot, 1, "").unwrap(), i8_ptr.const_null()).unwrap();
            let err_str_val = self.builder.build_load(str_ty, err_str_slot, "pf_err_str_val").unwrap();
            self.builder.build_store(err_str_ptr, err_str_val).unwrap();
            let err_val = self.builder.build_load(result_ty, err_alloca, "pf_err_val").unwrap();
            self.builder.build_return(Some(&err_val)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("parse_float".to_string(), fn_val);
            self.fn_return_types.insert("parse_float".to_string(),
                Type::Result(Box::new(Type::F64), Box::new(Type::Str)));
        }

        // ── Phase 5: abs_i64, min_i64, max_i64 ───────────────────────────────
        {
            // abs_i64(n: i64) -> i64: if n < 0 then -n else n
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("abs_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "ai_entry");
            let neg_bb   = self.context.append_basic_block(fn_val, "ai_neg");
            let pos_bb   = self.context.append_basic_block(fn_val, "ai_pos");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i64_ty.const_int(0, false);
            let is_neg = self.builder.build_int_compare(inkwell::IntPredicate::SLT, n, zero, "ai_isneg").unwrap();
            self.builder.build_conditional_branch(is_neg, neg_bb, pos_bb).unwrap();
            self.builder.position_at_end(neg_bb);
            let negn = self.builder.build_int_neg(n, "ai_neg").unwrap();
            self.builder.build_return(Some(&negn)).unwrap();
            self.builder.position_at_end(pos_bb);
            self.builder.build_return(Some(&n)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("abs_i64".to_string(), fn_val);
            self.fn_return_types.insert("abs_i64".to_string(), Type::I64);
        }

        {
            // min_i64(a: i64, b: i64) -> i64
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("min_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "mn_entry");
            let a_bb = self.context.append_basic_block(fn_val, "mn_a");
            let b_bb = self.context.append_basic_block(fn_val, "mn_b");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_le_b = self.builder.build_int_compare(inkwell::IntPredicate::SLE, a, b, "mn_ale").unwrap();
            self.builder.build_conditional_branch(a_le_b, a_bb, b_bb).unwrap();
            self.builder.position_at_end(a_bb);
            self.builder.build_return(Some(&a)).unwrap();
            self.builder.position_at_end(b_bb);
            self.builder.build_return(Some(&b)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("min_i64".to_string(), fn_val);
            self.fn_return_types.insert("min_i64".to_string(), Type::I64);
        }

        {
            // max_i64(a: i64, b: i64) -> i64
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("max_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "mx_entry");
            let a_bb = self.context.append_basic_block(fn_val, "mx_a");
            let b_bb = self.context.append_basic_block(fn_val, "mx_b");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_int_value();
            let b = fn_val.get_nth_param(1).unwrap().into_int_value();
            let a_ge_b = self.builder.build_int_compare(inkwell::IntPredicate::SGE, a, b, "mx_age").unwrap();
            self.builder.build_conditional_branch(a_ge_b, a_bb, b_bb).unwrap();
            self.builder.position_at_end(a_bb);
            self.builder.build_return(Some(&a)).unwrap();
            self.builder.position_at_end(b_bb);
            self.builder.build_return(Some(&b)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("max_i64".to_string(), fn_val);
            self.fn_return_types.insert("max_i64".to_string(), Type::I64);
        }

        // ── Phase 6: str_to_upper / str_to_lower ─────────────────────────────
        // Both functions: malloc len+1 bytes, copy with ASCII conversion, null-terminate.
        for (fname, is_upper) in &[("str_to_upper", true), ("str_to_lower", false)] {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function(fname, fn_ty, None);
            // Create all blocks upfront so we can pass them as branch targets.
            let entry_bb = self.context.append_basic_block(fn_val, "stl_entry");
            let loop_bb  = self.context.append_basic_block(fn_val, "stl_loop");
            let body_bb  = self.context.append_basic_block(fn_val, "stl_body");
            let done_bb  = self.context.append_basic_block(fn_val, "stl_done");
            let saved = self.builder.get_insert_block();

            // ── entry: malloc, init i=0, jump to loop ──────────────────────
            self.builder.position_at_end(entry_bb);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.builder.build_extract_value(s, 0, "stl_len").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "stl_ptr").unwrap().into_pointer_value();
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", ft, None)
            });
            let alloc_size = self.builder.build_int_add(s_len, i64_ty.const_int(1, false), "stl_sz").unwrap();
            let buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "stl_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let i_slot = self.builder.build_alloca(i64_ty, "stl_i").unwrap();
            self.builder.build_store(i_slot, i64_ty.const_zero()).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── loop: if i < s_len goto body else done ─────────────────────
            self.builder.position_at_end(loop_bb);
            let i_val = self.builder.build_load(i64_ty, i_slot, "stl_iv").unwrap().into_int_value();
            let in_range = self.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, s_len, "stl_cmp").unwrap();
            self.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // ── body: convert byte, store, increment i ─────────────────────
            self.builder.position_at_end(body_bb);
            let src_gep = unsafe { self.builder.build_gep(self.context.i8_type(), s_ptr, &[i_val], "stl_src").unwrap() };
            let byte = self.builder.build_load(self.context.i8_type(), src_gep, "stl_byte").unwrap().into_int_value();
            let converted = if *is_upper {
                // toupper: if byte in 'a'..'z' => byte - 32
                let lo = self.context.i8_type().const_int(b'a' as u64, false);
                let hi = self.context.i8_type().const_int(b'z' as u64, false);
                let is_lo = self.builder.build_int_compare(inkwell::IntPredicate::UGE, byte, lo, "stl_uge").unwrap();
                let is_hi = self.builder.build_int_compare(inkwell::IntPredicate::ULE, byte, hi, "stl_ule").unwrap();
                let in_range_c = self.builder.build_and(is_lo, is_hi, "stl_islc").unwrap();
                let sub32 = self.builder.build_int_sub(byte, self.context.i8_type().const_int(32, false), "stl_sub").unwrap();
                self.builder.build_select(in_range_c, sub32, byte, "stl_sel").unwrap().into_int_value()
            } else {
                // tolower: if byte in 'A'..'Z' => byte + 32
                let lo = self.context.i8_type().const_int(b'A' as u64, false);
                let hi = self.context.i8_type().const_int(b'Z' as u64, false);
                let is_lo = self.builder.build_int_compare(inkwell::IntPredicate::UGE, byte, lo, "stl_uge").unwrap();
                let is_hi = self.builder.build_int_compare(inkwell::IntPredicate::ULE, byte, hi, "stl_ule").unwrap();
                let in_range_c = self.builder.build_and(is_lo, is_hi, "stl_isuc").unwrap();
                let add32 = self.builder.build_int_add(byte, self.context.i8_type().const_int(32, false), "stl_add").unwrap();
                self.builder.build_select(in_range_c, add32, byte, "stl_sel").unwrap().into_int_value()
            };
            let dst_gep = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[i_val], "stl_dst").unwrap() };
            self.builder.build_store(dst_gep, converted).unwrap();
            let next_i = self.builder.build_int_add(i_val, i64_ty.const_int(1, false), "stl_ni").unwrap();
            self.builder.build_store(i_slot, next_i).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: null-terminate and return ───────────────────────────
            self.builder.position_at_end(done_bb);
            let null_gep = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[s_len], "stl_null").unwrap() };
            self.builder.build_store(null_gep, self.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, s_len, 0, "stl_r0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, buf, 1, "stl_r1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

        // ── Phase 6: str_trim / str_trim_start / str_trim_end ────────────────
        // Each trims ASCII whitespace (bytes <= 32).
        // Strategy: compute new ptr/len without allocating (returns a slice into the original).
        // For simplicity, we malloc+memcpy to preserve the "always owns" invariant.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcpy", ft, None)
            });

            // Helper for all three trim variants.
            // trim_start: advance ptr while isspace; trim_end: retreat len while isspace.
            for (fname, do_start, do_end) in &[
                ("str_trim", true, true),
                ("str_trim_start", true, false),
                ("str_trim_end", false, true),
            ] {
                let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
                let fn_val = self.module.add_function(fname, fn_ty, None);
                let entry_bb = self.context.append_basic_block(fn_val, "stt_entry");
                let saved = self.builder.get_insert_block();
                self.builder.position_at_end(entry_bb);

                let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
                let orig_len = self.builder.build_extract_value(s, 0, "stt_olen").unwrap().into_int_value();
                let orig_ptr = self.builder.build_extract_value(s, 1, "stt_optr").unwrap().into_pointer_value();

                // start = 0, end = orig_len
                let start_slot = self.builder.build_alloca(i64_ty, "stt_start").unwrap();
                let end_slot   = self.builder.build_alloca(i64_ty, "stt_end").unwrap();
                self.builder.build_store(start_slot, i64_ty.const_zero()).unwrap();
                self.builder.build_store(end_slot, orig_len).unwrap();

                let space_threshold = self.context.i8_type().const_int(32, false);

                if *do_start {
                    // while start < end && orig_ptr[start] <= 32: start++
                    let ts_cond = self.context.append_basic_block(fn_val, "stt_sc");
                    let ts_body = self.context.append_basic_block(fn_val, "stt_sb");
                    let ts_done = self.context.append_basic_block(fn_val, "stt_sd");
                    self.builder.build_unconditional_branch(ts_cond).unwrap();
                    self.builder.position_at_end(ts_cond);
                    let cur_start = self.builder.build_load(i64_ty, start_slot, "stt_cs").unwrap().into_int_value();
                    let cur_end   = self.builder.build_load(i64_ty, end_slot, "stt_ce").unwrap().into_int_value();
                    let in_range  = self.builder.build_int_compare(inkwell::IntPredicate::SLT, cur_start, cur_end, "stt_ir").unwrap();
                    // check byte
                    let byte_ptr = unsafe { self.builder.build_gep(self.context.i8_type(), orig_ptr, &[cur_start], "stt_bp").unwrap() };
                    let byte_val = self.builder.build_load(self.context.i8_type(), byte_ptr, "stt_bv").unwrap().into_int_value();
                    let is_space = self.builder.build_int_compare(inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_isp").unwrap();
                    let should_skip = self.builder.build_and(in_range, is_space, "stt_skip").unwrap();
                    self.builder.build_conditional_branch(should_skip, ts_body, ts_done).unwrap();
                    self.builder.position_at_end(ts_body);
                    let next_start = self.builder.build_int_add(cur_start, i64_ty.const_int(1, false), "stt_ns").unwrap();
                    self.builder.build_store(start_slot, next_start).unwrap();
                    self.builder.build_unconditional_branch(ts_cond).unwrap();
                    self.builder.position_at_end(ts_done);
                }

                if *do_end {
                    // while end > start && orig_ptr[end-1] <= 32: end--
                    let te_cond = self.context.append_basic_block(fn_val, "stt_ec");
                    let te_body = self.context.append_basic_block(fn_val, "stt_eb");
                    let te_done = self.context.append_basic_block(fn_val, "stt_ed");
                    self.builder.build_unconditional_branch(te_cond).unwrap();
                    self.builder.position_at_end(te_cond);
                    let cur_start = self.builder.build_load(i64_ty, start_slot, "stt_ecs").unwrap().into_int_value();
                    let cur_end   = self.builder.build_load(i64_ty, end_slot, "stt_ece").unwrap().into_int_value();
                    let in_range  = self.builder.build_int_compare(inkwell::IntPredicate::SGT, cur_end, cur_start, "stt_eir").unwrap();
                    let prev_idx  = self.builder.build_int_sub(cur_end, i64_ty.const_int(1, false), "stt_pi").unwrap();
                    let byte_ptr  = unsafe { self.builder.build_gep(self.context.i8_type(), orig_ptr, &[prev_idx], "stt_ebp").unwrap() };
                    let byte_val  = self.builder.build_load(self.context.i8_type(), byte_ptr, "stt_ebv").unwrap().into_int_value();
                    let is_space  = self.builder.build_int_compare(inkwell::IntPredicate::ULE, byte_val, space_threshold, "stt_eisp").unwrap();
                    let should_trim = self.builder.build_and(in_range, is_space, "stt_etrim").unwrap();
                    self.builder.build_conditional_branch(should_trim, te_body, te_done).unwrap();
                    self.builder.position_at_end(te_body);
                    let next_end = self.builder.build_int_sub(cur_end, i64_ty.const_int(1, false), "stt_ne").unwrap();
                    self.builder.build_store(end_slot, next_end).unwrap();
                    self.builder.build_unconditional_branch(te_cond).unwrap();
                    self.builder.position_at_end(te_done);
                }

                // new_start, new_end computed; new_len = end - start
                let final_start = self.builder.build_load(i64_ty, start_slot, "stt_fs").unwrap().into_int_value();
                let final_end   = self.builder.build_load(i64_ty, end_slot, "stt_fe").unwrap().into_int_value();
                let new_len = self.builder.build_int_sub(final_end, final_start, "stt_nl").unwrap();

                // malloc(new_len + 1)
                let alloc_sz = self.builder.build_int_add(new_len, i64_ty.const_int(1, false), "stt_az").unwrap();
                let buf = self.builder.build_call(malloc_fn, &[alloc_sz.into()], "stt_buf").unwrap()
                    .try_as_basic_value().left().unwrap().into_pointer_value();
                // memcpy(buf, orig_ptr+start, new_len)
                let src_ptr = unsafe { self.builder.build_gep(self.context.i8_type(), orig_ptr, &[final_start], "stt_src").unwrap() };
                self.builder.build_call(memcpy_fn, &[buf.into(), src_ptr.into(), new_len.into()], "stt_cpy").unwrap();
                // null-terminate
                let null_gep = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[new_len], "stt_nul").unwrap() };
                self.builder.build_store(null_gep, self.context.i8_type().const_zero()).unwrap();

                // return str { new_len, buf }
                let mut result = str_ty.const_zero();
                result = self.builder.build_insert_value(result, new_len, 0, "stt_r0").unwrap().into_struct_value();
                result = self.builder.build_insert_value(result, buf, 1, "stt_r1").unwrap().into_struct_value();
                self.builder.build_return(Some(&result)).unwrap();
                if let Some(b) = saved { self.builder.position_at_end(b); }
                self.functions.insert(fname.to_string(), fn_val);
                self.fn_return_types.insert(fname.to_string(), Type::Str);
            }
        }

        // ── Phase 6: str_repeat(s: str, n: i64) -> str ───────────────────────
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("str_repeat", fn_ty, None);
            // Create all blocks upfront.
            let entry_bb = self.context.append_basic_block(fn_val, "srep_entry");
            let loop_bb  = self.context.append_basic_block(fn_val, "srep_loop");
            let body_bb  = self.context.append_basic_block(fn_val, "srep_body");
            let done_bb  = self.context.append_basic_block(fn_val, "srep_done");
            let saved = self.builder.get_insert_block();

            // ── entry ──────────────────────────────────────────────────────
            self.builder.position_at_end(entry_bb);
            let s   = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let n   = fn_val.get_nth_param(1).unwrap().into_int_value();
            let s_len = self.builder.build_extract_value(s, 0, "srep_slen").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "srep_sptr").unwrap().into_pointer_value();

            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcpy", ft, None)
            });

            // n_clamped = max(n, 0); total_len = s_len * n_clamped
            let zero = i64_ty.const_zero();
            let n_neg = self.builder.build_int_compare(inkwell::IntPredicate::SLT, n, zero, "srep_neg").unwrap();
            let n_clamped = self.builder.build_select(n_neg, zero, n, "srep_nc").unwrap().into_int_value();
            let total_len = self.builder.build_int_mul(s_len, n_clamped, "srep_tlen").unwrap();
            // malloc(total_len + 1)
            let alloc_sz = self.builder.build_int_add(total_len, i64_ty.const_int(1, false), "srep_az").unwrap();
            let buf = self.builder.build_call(malloc_fn, &[alloc_sz.into()], "srep_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let i_slot = self.builder.build_alloca(i64_ty, "srep_i").unwrap();
            self.builder.build_store(i_slot, zero).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── loop: if i < n_clamped goto body else done ─────────────────
            self.builder.position_at_end(loop_bb);
            let i_val = self.builder.build_load(i64_ty, i_slot, "srep_iv").unwrap().into_int_value();
            let in_range = self.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, n_clamped, "srep_ir").unwrap();
            self.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // ── body: memcpy one copy, i++ ─────────────────────────────────
            self.builder.position_at_end(body_bb);
            let offset = self.builder.build_int_mul(i_val, s_len, "srep_off").unwrap();
            let dst = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[offset], "srep_dst").unwrap() };
            self.builder.build_call(memcpy_fn, &[dst.into(), s_ptr.into(), s_len.into()], "srep_mc").unwrap();
            let next_i = self.builder.build_int_add(i_val, i64_ty.const_int(1, false), "srep_ni").unwrap();
            self.builder.build_store(i_slot, next_i).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: null-terminate and return ───────────────────────────
            self.builder.position_at_end(done_bb);
            let null_gep = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[total_len], "srep_nul").unwrap() };
            self.builder.build_store(null_gep, self.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, total_len, 0, "srep_r0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, buf, 1, "srep_r1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_repeat".to_string(), fn_val);
            self.fn_return_types.insert("str_repeat".to_string(), Type::Str);
        }

        // ── Phase 6: str_replace(s: str, from: str, to: str) -> str ──────────
        // Replaces all non-overlapping occurrences of `from` in `s` with `to`.
        // Uses strstr for finding, then malloc+memcpy for building the result.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_replace", fn_ty, None);

            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", ft, None)
            });
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcpy", ft, None)
            });
            let strstr_fn = self.module.get_function("strstr").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.module.add_function("strstr", ft, None)
            });

            let entry_bb = self.context.append_basic_block(fn_val, "srpl_entry");
            let count_cond = self.context.append_basic_block(fn_val, "srpl_cc");
            let count_body = self.context.append_basic_block(fn_val, "srpl_cb");
            let build_init = self.context.append_basic_block(fn_val, "srpl_bi");
            let build_cond = self.context.append_basic_block(fn_val, "srpl_bc");
            let build_body = self.context.append_basic_block(fn_val, "srpl_bb");
            let build_done = self.context.append_basic_block(fn_val, "srpl_bd");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s    = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let from = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let to   = fn_val.get_nth_param(2).unwrap().into_struct_value();

            let s_len    = self.builder.build_extract_value(s, 0, "srpl_slen").unwrap().into_int_value();
            let s_ptr    = self.builder.build_extract_value(s, 1, "srpl_sptr").unwrap().into_pointer_value();
            let from_len = self.builder.build_extract_value(from, 0, "srpl_flen").unwrap().into_int_value();
            let from_ptr = self.builder.build_extract_value(from, 1, "srpl_fptr").unwrap().into_pointer_value();
            let to_len   = self.builder.build_extract_value(to, 0, "srpl_tlen").unwrap().into_int_value();
            let to_ptr   = self.builder.build_extract_value(to, 1, "srpl_tptr").unwrap().into_pointer_value();

            // --- Pass 1: count occurrences and compute output length ---
            let count_slot  = self.builder.build_alloca(i64_ty, "srpl_cnt").unwrap();
            let out_len_slot = self.builder.build_alloca(i64_ty, "srpl_ol").unwrap();
            let scan_slot   = self.builder.build_alloca(i8_ptr, "srpl_scan").unwrap();
            self.builder.build_store(count_slot, i64_ty.const_zero()).unwrap();
            self.builder.build_store(out_len_slot, s_len).unwrap();
            self.builder.build_store(scan_slot, s_ptr).unwrap();
            // If from_len == 0, skip replacement (avoid infinite loop).
            let from_empty = self.builder.build_int_compare(inkwell::IntPredicate::EQ, from_len, i64_ty.const_zero(), "srpl_fe").unwrap();
            self.builder.build_conditional_branch(from_empty, build_init, count_cond).unwrap();

            self.builder.position_at_end(count_cond);
            let scan = self.builder.build_load(i8_ptr, scan_slot, "srpl_sv").unwrap().into_pointer_value();
            let found = self.builder.build_call(strstr_fn, &[scan.into(), from_ptr.into()], "srpl_found").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            // found == null → done
            let null_ptr = i8_ptr.const_null();
            let is_null = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.builder.build_ptr_to_int(found, i64_ty, "srpl_fi").unwrap(),
                self.builder.build_ptr_to_int(null_ptr, i64_ty, "srpl_ni").unwrap(),
                "srpl_isnull"
            ).unwrap();
            self.builder.build_conditional_branch(is_null, build_init, count_body).unwrap();

            self.builder.position_at_end(count_body);
            // count++; out_len += (to_len - from_len); scan = found + from_len
            let cnt = self.builder.build_load(i64_ty, count_slot, "srpl_cv").unwrap().into_int_value();
            let new_cnt = self.builder.build_int_add(cnt, i64_ty.const_int(1, false), "srpl_nc").unwrap();
            self.builder.build_store(count_slot, new_cnt).unwrap();

            let ol = self.builder.build_load(i64_ty, out_len_slot, "srpl_olv").unwrap().into_int_value();
            let ol_adj = self.builder.build_int_add(
                self.builder.build_int_sub(ol, from_len, "srpl_sub").unwrap(),
                to_len, "srpl_ol2"
            ).unwrap();
            self.builder.build_store(out_len_slot, ol_adj).unwrap();

            // scan = found + from_len
            let new_scan = unsafe { self.builder.build_gep(self.context.i8_type(), found, &[from_len], "srpl_ns").unwrap() };
            self.builder.build_store(scan_slot, new_scan).unwrap();
            self.builder.build_unconditional_branch(count_cond).unwrap();

            // --- Pass 2: build output ---
            self.builder.position_at_end(build_init);
            let out_len = self.builder.build_load(i64_ty, out_len_slot, "srpl_fin_ol").unwrap().into_int_value();
            let alloc_sz = self.builder.build_int_add(out_len, i64_ty.const_int(1, false), "srpl_az").unwrap();
            let out_buf = self.builder.build_call(malloc_fn, &[alloc_sz.into()], "srpl_obuf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            let write_slot = self.builder.build_alloca(i8_ptr, "srpl_wr").unwrap();
            self.builder.build_store(write_slot, out_buf).unwrap();
            self.builder.build_store(scan_slot, s_ptr).unwrap();
            self.builder.build_unconditional_branch(build_cond).unwrap();

            self.builder.position_at_end(build_cond);
            let scan2 = self.builder.build_load(i8_ptr, scan_slot, "srpl_s2").unwrap().into_pointer_value();
            let found2 = self.builder.build_call(strstr_fn, &[scan2.into(), from_ptr.into()], "srpl_f2").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            let is_null2 = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.builder.build_ptr_to_int(found2, i64_ty, "srpl_f2i").unwrap(),
                self.builder.build_ptr_to_int(null_ptr, i64_ty, "srpl_n2i").unwrap(),
                "srpl_isnull2"
            ).unwrap();
            let from_empty2 = self.builder.build_int_compare(inkwell::IntPredicate::EQ, from_len, i64_ty.const_zero(), "srpl_fe2").unwrap();
            let skip = self.builder.build_or(is_null2, from_empty2, "srpl_skip").unwrap();
            self.builder.build_conditional_branch(skip, build_done, build_body).unwrap();

            self.builder.position_at_end(build_body);
            // copy [scan2, found2) into write, then copy to
            let wr = self.builder.build_load(i8_ptr, write_slot, "srpl_wrv").unwrap().into_pointer_value();
            let prefix_len = self.builder.build_int_sub(
                self.builder.build_ptr_to_int(found2, i64_ty, "srpl_pfound").unwrap(),
                self.builder.build_ptr_to_int(scan2, i64_ty, "srpl_pscan").unwrap(),
                "srpl_plen"
            ).unwrap();
            self.builder.build_call(memcpy_fn, &[wr.into(), scan2.into(), prefix_len.into()], "srpl_cpy1").unwrap();
            let wr2 = unsafe { self.builder.build_gep(self.context.i8_type(), wr, &[prefix_len], "srpl_wr2").unwrap() };
            self.builder.build_call(memcpy_fn, &[wr2.into(), to_ptr.into(), to_len.into()], "srpl_cpy2").unwrap();
            let wr3 = unsafe { self.builder.build_gep(self.context.i8_type(), wr2, &[to_len], "srpl_wr3").unwrap() };
            self.builder.build_store(write_slot, wr3).unwrap();
            let new_scan2 = unsafe { self.builder.build_gep(self.context.i8_type(), found2, &[from_len], "srpl_ns2").unwrap() };
            self.builder.build_store(scan_slot, new_scan2).unwrap();
            self.builder.build_unconditional_branch(build_cond).unwrap();

            self.builder.position_at_end(build_done);
            // copy remaining tail
            let final_scan = self.builder.build_load(i8_ptr, scan_slot, "srpl_fscan").unwrap().into_pointer_value();
            let final_wr   = self.builder.build_load(i8_ptr, write_slot, "srpl_fwr").unwrap().into_pointer_value();
            let tail_len = self.builder.build_int_sub(
                self.builder.build_int_add(
                    self.builder.build_ptr_to_int(s_ptr, i64_ty, "srpl_sp_int").unwrap(),
                    s_len, "srpl_sp_end"
                ).unwrap(),
                self.builder.build_ptr_to_int(final_scan, i64_ty, "srpl_scan_int").unwrap(),
                "srpl_tlen"
            ).unwrap();
            self.builder.build_call(memcpy_fn, &[final_wr.into(), final_scan.into(), tail_len.into()], "srpl_tail").unwrap();
            // null-terminate out_buf[out_len] = 0
            let null_gep = unsafe { self.builder.build_gep(self.context.i8_type(), out_buf, &[out_len], "srpl_nul").unwrap() };
            self.builder.build_store(null_gep, self.context.i8_type().const_zero()).unwrap();

            // return str { out_len, out_buf }
            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, out_len, 0, "srpl_r0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, out_buf, 1, "srpl_r1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_replace".to_string(), fn_val);
            self.fn_return_types.insert("str_replace".to_string(), Type::Str);
        }

        // ── Phase 6: env_var(name: str) -> Result<str, str> ──────────────────
        // Calls C getenv(). Returns Ok(str) if set, Err("not set") otherwise.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            let result_ty = self.context.struct_type(&[bool_ty.into(), i8_arr16_ty.into()], false);
            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("env_var", fn_ty, None);

            let getenv_fn = self.module.get_function("getenv").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i8_ptr.into()], false);
                self.module.add_function("getenv", ft, None)
            });
            let strlen_fn = self.module.get_function("strlen").unwrap_or_else(|| {
                let ft = i64_ty.fn_type(&[i8_ptr.into()], false);
                self.module.add_function("strlen", ft, None)
            });

            let entry_bb = self.context.append_basic_block(fn_val, "ev_entry");
            let ok_bb    = self.context.append_basic_block(fn_val, "ev_ok");
            let err_bb   = self.context.append_basic_block(fn_val, "ev_err");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let name_s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let name_ptr = self.builder.build_extract_value(name_s, 1, "ev_np").unwrap().into_pointer_value();

            let val_ptr = self.builder.build_call(getenv_fn, &[name_ptr.into()], "ev_val").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            let null_ptr = i8_ptr.const_null();
            let is_null = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ,
                self.builder.build_ptr_to_int(val_ptr, i64_ty, "ev_vi").unwrap(),
                self.builder.build_ptr_to_int(null_ptr, i64_ty, "ev_ni").unwrap(),
                "ev_isnull"
            ).unwrap();
            self.builder.build_conditional_branch(is_null, err_bb, ok_bb).unwrap();

            // Ok branch: return { tag=1, payload=str{strlen(val_ptr), val_ptr} }
            self.builder.position_at_end(ok_bb);
            let val_len = self.builder.build_call(strlen_fn, &[val_ptr.into()], "ev_vlen").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let ok_str_ptr = self.builder.build_alloca(result_ty, "ev_ok_r").unwrap();
            let tag_gep = self.builder.build_struct_gep(result_ty, ok_str_ptr, 0, "ev_tag").unwrap();
            self.builder.build_store(tag_gep, bool_ty.const_int(1, false)).unwrap();
            let payload_gep = self.builder.build_struct_gep(result_ty, ok_str_ptr, 1, "ev_pay").unwrap();
            let payload_as_str = self.builder.build_pointer_cast(payload_gep, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr").unwrap();
            let ok_str = {
                let mut sv = str_ty.const_zero();
                sv = self.builder.build_insert_value(sv, val_len, 0, "ev_sv0").unwrap().into_struct_value();
                sv = self.builder.build_insert_value(sv, val_ptr, 1, "ev_sv1").unwrap().into_struct_value();
                sv
            };
            self.builder.build_store(payload_as_str, ok_str).unwrap();
            let ok_result = self.builder.build_load(result_ty, ok_str_ptr, "ev_ok_val").unwrap();
            self.builder.build_return(Some(&ok_result)).unwrap();

            // Err branch: return { tag=0, payload=str{"not set"} }
            self.builder.position_at_end(err_bb);
            let err_msg = "not set\0";
            let err_global = self.module.add_global(
                self.context.i8_type().array_type(err_msg.len() as u32), None, "ev_err_str"
            );
            let err_bytes: Vec<_> = err_msg.bytes().map(|c| self.context.i8_type().const_int(c as u64, false)).collect();
            err_global.set_initializer(&self.context.i8_type().const_array(&err_bytes));
            err_global.set_constant(true);
            let err_ptr = self.builder.build_pointer_cast(
                err_global.as_pointer_value(), i8_ptr, "ev_eptr"
            ).unwrap();
            let err_str_ptr = self.builder.build_alloca(result_ty, "ev_err_r").unwrap();
            let tag_gep2 = self.builder.build_struct_gep(result_ty, err_str_ptr, 0, "ev_etag").unwrap();
            self.builder.build_store(tag_gep2, bool_ty.const_int(0, false)).unwrap();
            let payload_gep2 = self.builder.build_struct_gep(result_ty, err_str_ptr, 1, "ev_epay").unwrap();
            let payload_as_str2 = self.builder.build_pointer_cast(payload_gep2, str_ty.ptr_type(inkwell::AddressSpace::default()), "ev_str_ptr2").unwrap();
            let err_str = {
                let err_len = i64_ty.const_int((err_msg.len() - 1) as u64, false); // exclude null
                let mut sv = str_ty.const_zero();
                sv = self.builder.build_insert_value(sv, err_len, 0, "ev_es0").unwrap().into_struct_value();
                sv = self.builder.build_insert_value(sv, err_ptr, 1, "ev_es1").unwrap().into_struct_value();
                sv
            };
            self.builder.build_store(payload_as_str2, err_str).unwrap();
            let err_result = self.builder.build_load(result_ty, err_str_ptr, "ev_err_val").unwrap();
            self.builder.build_return(Some(&err_result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("env_var".to_string(), fn_val);
            let result_type = Type::Result(Box::new(Type::Str), Box::new(Type::Str));
            self.fn_return_types.insert("env_var".to_string(), result_type);
        }

        // ── Phase 6: exit(code: i64) -> () ───────────────────────────────────
        {
            let c_exit_fn = self.module.get_function("exit").unwrap_or_else(|| {
                let ft = self.context.void_type().fn_type(&[self.context.i32_type().into()], false);
                self.module.add_function("exit", ft, None)
            });
            let fn_ty = self.context.void_type().fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("exit_axon", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "ex_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let code = fn_val.get_nth_param(0).unwrap().into_int_value();
            let code_i32 = self.builder.build_int_truncate(code, self.context.i32_type(), "ex_code").unwrap();
            self.builder.build_call(c_exit_fn, &[code_i32.into()], "ex_call").unwrap();
            self.builder.build_unreachable().unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            // Register as "exit" — this is what the Axon source calls
            self.functions.insert("exit".to_string(), fn_val);
            self.fn_return_types.insert("exit".to_string(), Type::Unit);
        }

        // ── Phase 7: str_len(s: str) -> i64 ──────────────────────────────────
        // Extracts the length field (index 0) from the str struct.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("str_len", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "sl_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let s = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let len = self.builder.build_extract_value(s, 0, "sl_len").unwrap().into_int_value();
            self.builder.build_return(Some(&len)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
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
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into(), i64_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function(fname, fn_ty, None);

            // Blocks: entry → short-circuit (width <= len) or pad path → done
            let entry_bb = self.context.append_basic_block(fn_val, "sp_entry");
            let pad_bb   = self.context.append_basic_block(fn_val, "sp_pad");
            let done_bb  = self.context.append_basic_block(fn_val, "sp_done");

            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let width = fn_val.get_nth_param(1).unwrap().into_int_value();
            let fill  = fn_val.get_nth_param(2).unwrap().into_struct_value();

            let s_len = self.builder.build_extract_value(s, 0, "sp_slen").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "sp_sptr").unwrap().into_pointer_value();
            let fill_ptr = self.builder.build_extract_value(fill, 1, "sp_fptr").unwrap().into_pointer_value();

            // if s_len >= width: return s as-is
            let need_pad = self.builder.build_int_compare(
                inkwell::IntPredicate::SLT, s_len, width, "sp_need").unwrap();
            self.builder.build_conditional_branch(need_pad, pad_bb, done_bb).unwrap();

            // pad_bb: allocate width+1 bytes, fill pad chars, copy s, null-terminate
            self.builder.position_at_end(pad_bb);
            let pad_len = self.builder.build_int_sub(width, s_len, "sp_padlen").unwrap();
            let alloc_size = self.builder.build_int_add(width, i64_ty.const_int(1, false), "sp_alloc").unwrap();
            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let malloc_ty = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", malloc_ty, None)
            });
            let memcpy_fn = self.module.get_function("memcpy").unwrap_or_else(|| {
                let memcpy_ty = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("memcpy", memcpy_ty, None)
            });
            let buf = self.builder.build_call(malloc_fn, &[alloc_size.into()], "sp_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();
            // fill_char = fill_ptr[0]
            let fill_char = self.builder.build_load(self.context.i8_type(), fill_ptr, "sp_fchar").unwrap().into_int_value();
            // Use memset (declare if needed)
            let memset_fn = self.module.get_function("memset").unwrap_or_else(|| {
                let memset_ty = i8_ptr.fn_type(
                    &[i8_ptr.into(), self.context.i32_type().into(), i64_ty.into()], false);
                self.module.add_function("memset", memset_ty, None)
            });
            let fill_char_i32 = self.builder.build_int_z_extend(fill_char, self.context.i32_type(), "sp_fc32").unwrap();
            if *pad_start {
                // Pad bytes at start, then s
                self.builder.build_call(memset_fn, &[buf.into(), fill_char_i32.into(), pad_len.into()], "").unwrap();
                let s_dest = unsafe {
                    self.builder.build_gep(self.context.i8_type(), buf, &[pad_len], "sp_sdest").unwrap()
                };
                self.builder.build_call(memcpy_fn, &[s_dest.into(), s_ptr.into(), s_len.into()], "").unwrap();
            } else {
                // s then pad bytes
                self.builder.build_call(memcpy_fn, &[buf.into(), s_ptr.into(), s_len.into()], "").unwrap();
                let pad_dest = unsafe {
                    self.builder.build_gep(self.context.i8_type(), buf, &[s_len], "sp_pdest").unwrap()
                };
                self.builder.build_call(memset_fn, &[pad_dest.into(), fill_char_i32.into(), pad_len.into()], "").unwrap();
            }
            // null-terminate
            let null_pos = unsafe {
                self.builder.build_gep(self.context.i8_type(), buf, &[width], "sp_null").unwrap()
            };
            self.builder.build_store(null_pos, self.context.i8_type().const_int(0, false)).unwrap();
            self.builder.build_unconditional_branch(done_bb).unwrap();

            // done_bb: phi nodes must come FIRST (before any non-phi instructions).
            self.builder.position_at_end(done_bb);
            let len_phi = self.builder.build_phi(i64_ty, "sp_rlen").unwrap();
            len_phi.add_incoming(&[(&s_len, entry_bb), (&width, pad_bb)]);
            let ptr_phi = self.builder.build_phi(i8_ptr, "sp_rptr").unwrap();
            ptr_phi.add_incoming(&[(&s_ptr, entry_bb), (&buf, pad_bb)]);
            // Build the result str struct using insert_value (no alloca needed).
            let mut sp_res = str_ty.get_undef();
            sp_res = self.builder
                .build_insert_value(sp_res, len_phi.as_basic_value().into_int_value(), 0, "sp_wl")
                .unwrap().into_struct_value();
            sp_res = self.builder
                .build_insert_value(sp_res, ptr_phi.as_basic_value().into_pointer_value(), 1, "sp_rv")
                .unwrap().into_struct_value();
            self.builder.build_return(Some(&sp_res)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::Str);
        }

        // ── Phase 7: min_f64 / max_f64 ───────────────────────────────────────
        for (fname, is_min) in &[("min_f64", true), ("max_f64", false)] {
            let f64_ty = self.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.module.add_function(fname, fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "mf_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let a = fn_val.get_nth_param(0).unwrap().into_float_value();
            let b = fn_val.get_nth_param(1).unwrap().into_float_value();
            let pred = if *is_min { inkwell::FloatPredicate::OLT } else { inkwell::FloatPredicate::OGT };
            let cmp = self.builder.build_float_compare(pred, a, b, "mf_cmp").unwrap();
            let result = self.builder.build_select(cmp, a, b, "mf_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert(fname.to_string(), fn_val);
            self.fn_return_types.insert(fname.to_string(), Type::F64);
        }

        // ── Phase 7: clamp_i64(n: i64, lo: i64, hi: i64) -> i64 ─────────────
        {
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("clamp_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "ci_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let n  = fn_val.get_nth_param(0).unwrap().into_int_value();
            let lo = fn_val.get_nth_param(1).unwrap().into_int_value();
            let hi = fn_val.get_nth_param(2).unwrap().into_int_value();
            // max(lo, min(n, hi))
            let lt_hi = self.builder.build_int_compare(inkwell::IntPredicate::SLT, n, hi, "ci_lthi").unwrap();
            let n_or_hi = self.builder.build_select(lt_hi, n, hi, "ci_nhi").unwrap().into_int_value();
            let gt_lo = self.builder.build_int_compare(inkwell::IntPredicate::SGT, n_or_hi, lo, "ci_gtlo").unwrap();
            let result = self.builder.build_select(gt_lo, n_or_hi, lo, "ci_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("clamp_i64".to_string(), fn_val);
            self.fn_return_types.insert("clamp_i64".to_string(), Type::I64);
        }

        // ── Phase 7: clamp_f64(n: f64, lo: f64, hi: f64) -> f64 ─────────────
        {
            let f64_ty = self.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into(), f64_ty.into(), f64_ty.into()], false);
            let fn_val = self.module.add_function("clamp_f64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "cf_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let n  = fn_val.get_nth_param(0).unwrap().into_float_value();
            let lo = fn_val.get_nth_param(1).unwrap().into_float_value();
            let hi = fn_val.get_nth_param(2).unwrap().into_float_value();
            // max(lo, min(n, hi))
            let lt_hi = self.builder.build_float_compare(inkwell::FloatPredicate::OLT, n, hi, "cf_lthi").unwrap();
            let n_or_hi = self.builder.build_select(lt_hi, n, hi, "cf_nhi").unwrap().into_float_value();
            let gt_lo = self.builder.build_float_compare(inkwell::FloatPredicate::OGT, n_or_hi, lo, "cf_gtlo").unwrap();
            let result = self.builder.build_select(gt_lo, n_or_hi, lo, "cf_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("clamp_f64".to_string(), fn_val);
            self.fn_return_types.insert("clamp_f64".to_string(), Type::F64);
        }

        // ── Phase 7: parse_bool(s: str) -> Result<bool, str> ─────────────────
        // Accepts "true"/"false" (exact, lowercase). Returns Ok(bool) or Err("invalid bool").
        // Result<bool,str> layout: { i1 tag, [16 x i8] payload }
        // (bool=1 byte, str=16 bytes → max=16; same layout as Result<f64,str>)
        {
            let str_ty  = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let i1_ty   = self.context.bool_type();
            let i8_arr16_ty = self.context.i8_type().array_type(16);
            // tag is stored as i1 (matches parse_float convention)
            let result_ty = self.context.struct_type(&[i1_ty.into(), i8_arr16_ty.into()], false);

            let strncmp_fn = self.module.get_function("strncmp").unwrap_or_else(|| {
                let ft = self.context.i32_type().fn_type(
                    &[i8_ptr.into(), i8_ptr.into(), i64_ty.into()], false);
                self.module.add_function("strncmp", ft, None)
            });

            let fn_ty = result_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("parse_bool", fn_ty, None);

            let entry_bb    = self.context.append_basic_block(fn_val, "pb_entry");
            let check_f_bb  = self.context.append_basic_block(fn_val, "pb_chk_false");
            let ok_true_bb  = self.context.append_basic_block(fn_val, "pb_ok_true");
            let ok_false_bb = self.context.append_basic_block(fn_val, "pb_ok_false");
            let err_bb      = self.context.append_basic_block(fn_val, "pb_err");

            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);

            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.builder.build_extract_value(s, 0, "pb_slen").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "pb_sptr").unwrap().into_pointer_value();

            // Check s == "true": len==4 && strncmp(s_ptr,"true",4)==0
            let len4 = i64_ty.const_int(4, false);
            let is_len4 = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ, s_len, len4, "pb_l4").unwrap();
            let true_lit_g = self.module.add_global(
                self.context.i8_type().array_type(5), None, "pb_true_lit");
            true_lit_g.set_initializer(&self.context.const_string(b"true", true));
            true_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let true_lit = true_lit_g.as_pointer_value();
            let cmp_t = self.builder.build_call(strncmp_fn,
                &[s_ptr.into(), true_lit.into(), len4.into()], "pb_cmpt").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_t_eq = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ, cmp_t,
                self.context.i32_type().const_int(0, false), "pb_teq").unwrap();
            let is_true_str = self.builder.build_and(is_len4, cmp_t_eq, "pb_istrue").unwrap();
            self.builder.build_conditional_branch(is_true_str, ok_true_bb, check_f_bb).unwrap();

            // check_f_bb: check s == "false": len==5 && strncmp(s_ptr,"false",5)==0
            self.builder.position_at_end(check_f_bb);
            let len5 = i64_ty.const_int(5, false);
            let is_len5 = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ, s_len, len5, "pb_l5").unwrap();
            let false_lit_g = self.module.add_global(
                self.context.i8_type().array_type(6), None, "pb_false_lit");
            false_lit_g.set_initializer(&self.context.const_string(b"false", true));
            false_lit_g.set_linkage(inkwell::module::Linkage::Private);
            let false_lit = false_lit_g.as_pointer_value();
            let cmp_f = self.builder.build_call(strncmp_fn,
                &[s_ptr.into(), false_lit.into(), len5.into()], "pb_cmpf").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let cmp_f_eq = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ, cmp_f,
                self.context.i32_type().const_int(0, false), "pb_feq").unwrap();
            let is_false_str = self.builder.build_and(is_len5, cmp_f_eq, "pb_isfalse").unwrap();
            self.builder.build_conditional_branch(is_false_str, ok_false_bb, err_bb).unwrap();

            // ok_true_bb: tag=1, payload = i1 true cast to [16 x i8]
            self.builder.position_at_end(ok_true_bb);
            {
                let ok_alloca = self.builder.build_alloca(result_ty, "pb_ot_slot").unwrap();
                self.builder.build_store(
                    self.builder.build_struct_gep(result_ty, ok_alloca, 0, "pb_ot_tag").unwrap(),
                    i1_ty.const_int(1, false)).unwrap();
                let payload_ptr = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "pb_ot_pay").unwrap();
                let bool_ptr = self.builder.build_pointer_cast(
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_ot_bptr").unwrap();
                self.builder.build_store(bool_ptr, i1_ty.const_int(1, false)).unwrap();
                let val = self.builder.build_load(result_ty, ok_alloca, "pb_ot_val").unwrap();
                self.builder.build_return(Some(&val)).unwrap();
            }

            // ok_false_bb: tag=1, payload = i1 false cast to [16 x i8]
            self.builder.position_at_end(ok_false_bb);
            {
                let ok_alloca = self.builder.build_alloca(result_ty, "pb_of_slot").unwrap();
                self.builder.build_store(
                    self.builder.build_struct_gep(result_ty, ok_alloca, 0, "pb_of_tag").unwrap(),
                    i1_ty.const_int(1, false)).unwrap();
                let payload_ptr = self.builder.build_struct_gep(result_ty, ok_alloca, 1, "pb_of_pay").unwrap();
                let bool_ptr = self.builder.build_pointer_cast(
                    payload_ptr, i1_ty.ptr_type(inkwell::AddressSpace::default()), "pb_of_bptr").unwrap();
                self.builder.build_store(bool_ptr, i1_ty.const_int(0, false)).unwrap();
                let val = self.builder.build_load(result_ty, ok_alloca, "pb_of_val").unwrap();
                self.builder.build_return(Some(&val)).unwrap();
            }

            // err_bb: tag=0, payload = str{"invalid bool"} cast to [16 x i8]
            self.builder.position_at_end(err_bb);
            {
                let err_alloca = self.builder.build_alloca(result_ty, "pb_err_slot").unwrap();
                self.builder.build_store(
                    self.builder.build_struct_gep(result_ty, err_alloca, 0, "pb_err_tag").unwrap(),
                    i1_ty.const_int(0, false)).unwrap();
                let payload_ptr = self.builder.build_struct_gep(result_ty, err_alloca, 1, "pb_err_pay").unwrap();
                let str_ptr = self.builder.build_pointer_cast(
                    payload_ptr, str_ty.ptr_type(inkwell::AddressSpace::default()), "pb_err_sptr").unwrap();
                let err_str_alloca = self.builder.build_alloca(str_ty, "pb_err_s").unwrap();
                let err_msg = b"invalid bool";
                let err_lit_g = self.module.add_global(
                    self.context.i8_type().array_type(err_msg.len() as u32 + 1),
                    None, "pb_err_msg");
                err_lit_g.set_initializer(&self.context.const_string(err_msg, true));
                err_lit_g.set_linkage(inkwell::module::Linkage::Private);
                let err_lit = err_lit_g.as_pointer_value();
                self.builder.build_store(
                    self.builder.build_struct_gep(str_ty, err_str_alloca, 0, "pb_esl").unwrap(),
                    i64_ty.const_int(err_msg.len() as u64, false)).unwrap();
                self.builder.build_store(
                    self.builder.build_struct_gep(str_ty, err_str_alloca, 1, "pb_esp").unwrap(),
                    err_lit).unwrap();
                let err_str_val = self.builder.build_load(str_ty, err_str_alloca, "pb_esv").unwrap();
                self.builder.build_store(str_ptr, err_str_val).unwrap();
                let val = self.builder.build_load(result_ty, err_alloca, "pb_err_val").unwrap();
                self.builder.build_return(Some(&val)).unwrap();
            }

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("parse_bool".to_string(), fn_val);
            self.fn_return_types.insert("parse_bool".to_string(),
                Type::Result(Box::new(Type::Bool), Box::new(Type::Str)));
        }

        // ── Phase 7: random_i64(lo: i64, hi: i64) -> i64 ─────────────────────
        // Uses C rand() % (hi - lo) + lo. Behavior undefined if hi <= lo.
        {
            let rand_fn = self.module.get_function("rand").unwrap_or_else(|| {
                let ft = self.context.i32_type().fn_type(&[], false);
                self.module.add_function("rand", ft, None)
            });
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("random_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "ri_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let lo = fn_val.get_nth_param(0).unwrap().into_int_value();
            let hi = fn_val.get_nth_param(1).unwrap().into_int_value();
            let r_i32 = self.builder.build_call(rand_fn, &[], "ri_rand").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            let r = self.builder.build_int_s_extend(r_i32, i64_ty, "ri_r64").unwrap();
            let range = self.builder.build_int_sub(hi, lo, "ri_range").unwrap();
            let r_mod = self.builder.build_int_signed_rem(r, range, "ri_mod").unwrap();
            // Ensure non-negative: (r_mod + range) % range
            let r_pos = self.builder.build_int_add(r_mod, range, "ri_pos").unwrap();
            let r_final = self.builder.build_int_signed_rem(r_pos, range, "ri_final").unwrap();
            let result = self.builder.build_int_add(r_final, lo, "ri_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("random_i64".to_string(), fn_val);
            self.fn_return_types.insert("random_i64".to_string(), Type::I64);
        }

        // ── Phase 7: random_f64() -> f64 ─────────────────────────────────────
        // Returns rand() / (RAND_MAX + 1.0) in [0.0, 1.0).
        {
            let f64_ty = self.context.f64_type();
            let rand_fn = self.module.get_function("rand").unwrap_or_else(|| {
                let ft = self.context.i32_type().fn_type(&[], false);
                self.module.add_function("rand", ft, None)
            });
            let fn_ty = f64_ty.fn_type(&[], false);
            let fn_val = self.module.add_function("random_f64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "rf_entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(entry_bb);
            let r_i32 = self.builder.build_call(rand_fn, &[], "rf_rand").unwrap()
                .try_as_basic_value().left().unwrap().into_int_value();
            // Convert to f64
            let r_f = self.builder.build_signed_int_to_float(r_i32, f64_ty, "rf_f").unwrap();
            // RAND_MAX = 2147483647 → divisor = 2147483648.0
            let divisor = f64_ty.const_float(2147483648.0);
            let result = self.builder.build_float_div(r_f, divisor, "rf_result").unwrap();
            self.builder.build_return(Some(&result)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("random_f64".to_string(), fn_val);
            self.fn_return_types.insert("random_f64".to_string(), Type::F64);
        }

        // ── Phase 9: i64_to_f64(n: i64) -> f64 ──────────────────────────────
        {
            let i64_ty = self.context.i64_type();
            let f64_ty = self.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("i64_to_f64", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let r = self.builder.build_signed_int_to_float(n, f64_ty, "itf").unwrap();
            self.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("i64_to_f64".to_string(), fn_val);
            self.fn_return_types.insert("i64_to_f64".to_string(), Type::F64);
        }

        // ── Phase 9: f64_to_i64(x: f64) -> i64 ──────────────────────────────
        {
            let i64_ty = self.context.i64_type();
            let f64_ty = self.context.f64_type();
            let fn_ty = i64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.module.add_function("f64_to_i64", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let x = fn_val.get_nth_param(0).unwrap().into_float_value();
            let r = self.builder.build_float_to_signed_int(x, i64_ty, "fti").unwrap();
            self.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("f64_to_i64".to_string(), fn_val);
            self.fn_return_types.insert("f64_to_i64".to_string(), Type::I64);
        }

        // ── Phase 9: abs_i64(n: i64) -> i64 ─────────────────────────────────
        {
            let i64_ty = self.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("abs_i64", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let neg = self.builder.build_int_neg(n, "abs_neg").unwrap();
            let is_neg = self.builder.build_int_compare(
                inkwell::IntPredicate::SLT, n, i64_ty.const_zero(), "abs_cmp").unwrap();
            let r = self.builder.build_select(is_neg, neg, n, "abs_r").unwrap().into_int_value();
            self.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("abs_i64".to_string(), fn_val);
            self.fn_return_types.insert("abs_i64".to_string(), Type::I64);
        }

        // ── Phase 9: abs_f64(x: f64) -> f64 ─────────────────────────────────
        {
            let f64_ty = self.context.f64_type();
            let fn_ty = f64_ty.fn_type(&[f64_ty.into()], false);
            let fn_val = self.module.add_function("abs_f64", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let x = fn_val.get_nth_param(0).unwrap().into_float_value();
            let zero = f64_ty.const_float(0.0);
            let neg = self.builder.build_float_neg(x, "abf_neg").unwrap();
            let is_neg = self.builder.build_float_compare(
                inkwell::FloatPredicate::OLT, x, zero, "abf_cmp").unwrap();
            let r = self.builder.build_select(is_neg, neg, x, "abf_r").unwrap().into_float_value();
            self.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("abs_f64".to_string(), fn_val);
            self.fn_return_types.insert("abs_f64".to_string(), Type::F64);
        }

        // ── Phase 9: sign_i64(n: i64) -> i64  (-1 | 0 | 1) ─────────────────
        {
            let i64_ty = self.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into()], false);
            let fn_val = self.module.add_function("sign_i64", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);
            let n = fn_val.get_nth_param(0).unwrap().into_int_value();
            let zero = i64_ty.const_zero();
            let is_pos = self.builder.build_int_compare(
                inkwell::IntPredicate::SGT, n, zero, "sg_pos").unwrap();
            let is_neg = self.builder.build_int_compare(
                inkwell::IntPredicate::SLT, n, zero, "sg_neg").unwrap();
            let one = i64_ty.const_int(1, false);
            let neg_one = i64_ty.const_int(u64::MAX, true);
            // if positive → 1, else if negative → -1, else → 0
            let step1 = self.builder.build_select(is_neg, neg_one, zero, "sg_s1").unwrap().into_int_value();
            let r     = self.builder.build_select(is_pos, one, step1, "sg_r").unwrap().into_int_value();
            self.builder.build_return(Some(&r)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("sign_i64".to_string(), fn_val);
            self.fn_return_types.insert("sign_i64".to_string(), Type::I64);
        }

        // ── Phase 9: pow_i64(base: i64, exp: i64) -> i64 ────────────────────
        // Iterative: result=1; while exp>0 { result*=base; exp-=1 }
        {
            let i64_ty = self.context.i64_type();
            let fn_ty = i64_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("pow_i64", fn_ty, None);
            let entry_bb = self.context.append_basic_block(fn_val, "pi_entry");
            let cond_bb  = self.context.append_basic_block(fn_val, "pi_cond");
            let body_bb  = self.context.append_basic_block(fn_val, "pi_body");
            let exit_bb  = self.context.append_basic_block(fn_val, "pi_exit");
            let saved = self.builder.get_insert_block();

            self.builder.position_at_end(entry_bb);
            let base_slot   = self.builder.build_alloca(i64_ty, "pi_base").unwrap();
            let exp_slot    = self.builder.build_alloca(i64_ty, "pi_exp").unwrap();
            let result_slot = self.builder.build_alloca(i64_ty, "pi_result").unwrap();
            let base = fn_val.get_nth_param(0).unwrap().into_int_value();
            let exp  = fn_val.get_nth_param(1).unwrap().into_int_value();
            self.builder.build_store(base_slot, base).unwrap();
            self.builder.build_store(exp_slot, exp).unwrap();
            self.builder.build_store(result_slot, i64_ty.const_int(1, false)).unwrap();
            self.builder.build_unconditional_branch(cond_bb).unwrap();

            self.builder.position_at_end(cond_bb);
            let e = self.builder.build_load(i64_ty, exp_slot, "pi_e").unwrap().into_int_value();
            let cmp = self.builder.build_int_compare(
                inkwell::IntPredicate::SGT, e, i64_ty.const_zero(), "pi_cmp").unwrap();
            self.builder.build_conditional_branch(cmp, body_bb, exit_bb).unwrap();

            self.builder.position_at_end(body_bb);
            let r = self.builder.build_load(i64_ty, result_slot, "pi_r").unwrap().into_int_value();
            let b = self.builder.build_load(i64_ty, base_slot, "pi_b").unwrap().into_int_value();
            let r2 = self.builder.build_int_mul(r, b, "pi_r2").unwrap();
            self.builder.build_store(result_slot, r2).unwrap();
            let e2 = self.builder.build_load(i64_ty, exp_slot, "pi_e2").unwrap().into_int_value();
            let e3 = self.builder.build_int_sub(e2, i64_ty.const_int(1, false), "pi_e3").unwrap();
            self.builder.build_store(exp_slot, e3).unwrap();
            self.builder.build_unconditional_branch(cond_bb).unwrap();

            self.builder.position_at_end(exit_bb);
            let res = self.builder.build_load(i64_ty, result_slot, "pi_res").unwrap();
            self.builder.build_return(Some(&res)).unwrap();
            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("pow_i64".to_string(), fn_val);
            self.fn_return_types.insert("pow_i64".to_string(), Type::I64);
        }

        // ── Phase 9: sqrt_f64 / floor_f64 / ceil_f64 / round_f64 ────────────
        // Use LLVM intrinsics via C libm linkage.
        {
            let f64_ty = self.context.f64_type();
            let fn1_ty = f64_ty.fn_type(&[f64_ty.into()], false);

            for (axon_name, c_name) in &[
                ("sqrt_f64",  "sqrt"),
                ("floor_f64", "floor"),
                ("ceil_f64",  "ceil"),
                ("round_f64", "round"),
            ] {
                // Declare the C libm function (or reuse if already declared).
                let libm_fn = self.module.get_function(c_name)
                    .unwrap_or_else(|| self.module.add_function(c_name, fn1_ty, None));

                let fn_val = self.module.add_function(axon_name, fn1_ty, None);
                let bb = self.context.append_basic_block(fn_val, "entry");
                let saved = self.builder.get_insert_block();
                self.builder.position_at_end(bb);
                let x = fn_val.get_nth_param(0).unwrap().into_float_value();
                let r = self.builder.build_call(libm_fn, &[x.into()], "r").unwrap()
                    .try_as_basic_value().left().unwrap().into_float_value();
                self.builder.build_return(Some(&r)).unwrap();
                if let Some(b) = saved { self.builder.position_at_end(b); }
                self.functions.insert(axon_name.to_string(), fn_val);
                self.fn_return_types.insert(axon_name.to_string(), Type::F64);
            }
        }


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
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = i64_ty.fn_type(&[str_ty.into(), str_ty.into()], false);
            let fn_val = self.module.add_function("str_count", fn_ty, None);

            let entry_bb     = self.context.append_basic_block(fn_val, "sc_entry");
            let early_ret_bb = self.context.append_basic_block(fn_val, "sc_early_ret");
            let loop_bb      = self.context.append_basic_block(fn_val, "sc_loop");
            let found_bb     = self.context.append_basic_block(fn_val, "sc_found");
            let done_bb      = self.context.append_basic_block(fn_val, "sc_done");
            let saved = self.builder.get_insert_block();

            // ── entry: extract fields, place allocas, then branch ───────────
            self.builder.position_at_end(entry_bb);
            let s      = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let needle = fn_val.get_nth_param(1).unwrap().into_struct_value();
            let s_ptr      = self.builder.build_extract_value(s, 1, "sc_sptr").unwrap().into_pointer_value();
            let needle_len = self.builder.build_extract_value(needle, 0, "sc_nlen").unwrap().into_int_value();
            let needle_ptr = self.builder.build_extract_value(needle, 1, "sc_nptr").unwrap().into_pointer_value();
            let zero = i64_ty.const_zero();

            // Allocas here so they dominate all successors (including done_bb).
            let cur_slot   = self.builder.build_alloca(i8_ptr, "sc_cur").unwrap();
            let count_slot = self.builder.build_alloca(i64_ty, "sc_cnt").unwrap();
            self.builder.build_store(cur_slot, s_ptr).unwrap();
            self.builder.build_store(count_slot, zero).unwrap();

            let strstr_fn = self.module.get_function("strstr").unwrap_or_else(|| {
                let t = i8_ptr.fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                self.module.add_function("strstr", t, None)
            });

            let needle_empty = self.builder.build_int_compare(
                inkwell::IntPredicate::EQ, needle_len, zero, "sc_nempty",
            ).unwrap();
            self.builder.build_conditional_branch(needle_empty, early_ret_bb, loop_bb).unwrap();

            // ── early_ret: return 0 for empty needle ─────────────────────────
            self.builder.position_at_end(early_ret_bb);
            self.builder.build_return(Some(&zero)).unwrap();

            // ── loop: cur = strstr(cur, needle); branch on null ──────────────
            self.builder.position_at_end(loop_bb);
            let cur = self.builder.build_load(i8_ptr, cur_slot, "sc_cur_v").unwrap().into_pointer_value();
            let found_ptr = self.builder.build_call(
                strstr_fn, &[cur.into(), needle_ptr.into()], "sc_fp",
            ).unwrap().try_as_basic_value().left().unwrap().into_pointer_value();
            let found_int = self.builder.build_ptr_to_int(found_ptr, i64_ty, "sc_fpi").unwrap();
            let null_int  = self.builder.build_ptr_to_int(i8_ptr.const_null(), i64_ty, "sc_ni").unwrap();
            let is_found = self.builder.build_int_compare(
                inkwell::IntPredicate::NE, found_int, null_int, "sc_isf",
            ).unwrap();
            self.builder.build_conditional_branch(is_found, found_bb, done_bb).unwrap();

            // ── found: count++, advance cursor past the match ────────────────
            self.builder.position_at_end(found_bb);
            let cnt = self.builder.build_load(i64_ty, count_slot, "sc_cnt_v").unwrap().into_int_value();
            let cnt1 = self.builder.build_int_add(cnt, i64_ty.const_int(1, false), "sc_cnt1").unwrap();
            self.builder.build_store(count_slot, cnt1).unwrap();
            let next = unsafe {
                self.builder.build_gep(self.context.i8_type(), found_ptr, &[needle_len], "sc_next").unwrap()
            };
            self.builder.build_store(cur_slot, next).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // ── done: return accumulated count ───────────────────────────────
            self.builder.position_at_end(done_bb);
            let final_count = self.builder.build_load(i64_ty, count_slot, "sc_final").unwrap().into_int_value();
            self.builder.build_return(Some(&final_count)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_count".to_string(), fn_val);
            self.fn_return_types.insert("str_count".to_string(), Type::I64);
        }


        // ── Phase 10: str_reverse(s: str) -> str ─────────────────────────────
        // Returns a malloc'd copy of s with bytes in reverse order.
        {
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);
            let fn_ty = str_ty.fn_type(&[str_ty.into()], false);
            let fn_val = self.module.add_function("str_reverse", fn_ty, None);

            let entry_bb = self.context.append_basic_block(fn_val, "srev_entry");
            let loop_bb  = self.context.append_basic_block(fn_val, "srev_loop");
            let body_bb  = self.context.append_basic_block(fn_val, "srev_body");
            let done_bb  = self.context.append_basic_block(fn_val, "srev_done");
            let saved = self.builder.get_insert_block();

            self.builder.position_at_end(entry_bb);
            let s     = fn_val.get_nth_param(0).unwrap().into_struct_value();
            let s_len = self.builder.build_extract_value(s, 0, "srev_len").unwrap().into_int_value();
            let s_ptr = self.builder.build_extract_value(s, 1, "srev_ptr").unwrap().into_pointer_value();

            let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                let ft = i8_ptr.fn_type(&[i64_ty.into()], false);
                self.module.add_function("malloc", ft, None)
            });
            // alloc s_len + 1 bytes
            let alloc_sz = self.builder.build_int_add(s_len, i64_ty.const_int(1, false), "srev_az").unwrap();
            let buf = self.builder.build_call(malloc_fn, &[alloc_sz.into()], "srev_buf").unwrap()
                .try_as_basic_value().left().unwrap().into_pointer_value();

            // i = 0; loop while i < s_len
            let zero = i64_ty.const_zero();
            let i_slot = self.builder.build_alloca(i64_ty, "srev_i").unwrap();
            self.builder.build_store(i_slot, zero).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            self.builder.position_at_end(loop_bb);
            let i_val = self.builder.build_load(i64_ty, i_slot, "srev_iv").unwrap().into_int_value();
            let in_range = self.builder.build_int_compare(inkwell::IntPredicate::SLT, i_val, s_len, "srev_ir").unwrap();
            self.builder.build_conditional_branch(in_range, body_bb, done_bb).unwrap();

            // body: buf[i] = s_ptr[s_len - 1 - i]; i++
            self.builder.position_at_end(body_bb);
            let src_idx = self.builder.build_int_sub(
                self.builder.build_int_sub(s_len, i64_ty.const_int(1, false), "srev_sm1").unwrap(),
                i_val,
                "srev_si",
            ).unwrap();
            let src_byte_ptr = unsafe {
                self.builder.build_gep(self.context.i8_type(), s_ptr, &[src_idx], "srev_sbp").unwrap()
            };
            let byte = self.builder.build_load(self.context.i8_type(), src_byte_ptr, "srev_b").unwrap();
            let dst_byte_ptr = unsafe {
                self.builder.build_gep(self.context.i8_type(), buf, &[i_val], "srev_dbp").unwrap()
            };
            self.builder.build_store(dst_byte_ptr, byte).unwrap();
            let next_i = self.builder.build_int_add(i_val, i64_ty.const_int(1, false), "srev_ni").unwrap();
            self.builder.build_store(i_slot, next_i).unwrap();
            self.builder.build_unconditional_branch(loop_bb).unwrap();

            // done: null-terminate and return
            self.builder.position_at_end(done_bb);
            let null_pos = unsafe { self.builder.build_gep(self.context.i8_type(), buf, &[s_len], "srev_np").unwrap() };
            self.builder.build_store(null_pos, self.context.i8_type().const_zero()).unwrap();
            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, s_len, 0, "srev_r0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, buf, 1, "srev_r1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("str_reverse".to_string(), fn_val);
            self.fn_return_types.insert("str_reverse".to_string(), Type::Str);
        }


        // ── Phase 10: i64_to_str_radix(n: i64, base: i64) -> str ─────────────
        // Convert n to string in given base (2-36). Negative n gets '-' prefix.
        // Delegates to __axon_i64_to_str_radix in the runtime via out-params.
        {
            let i64_ptr = i64_ty.ptr_type(inkwell::AddressSpace::default());
            let i8_ptr_ptr = i8_ptr.ptr_type(inkwell::AddressSpace::default());
            let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

            // Runtime: void __axon_i64_to_str_radix(i64 n, i64 base, i64* out_len, i8** out_ptr)
            let void_ty = self.context.void_type();
            let rt_fn_ty = void_ty.fn_type(
                &[i64_ty.into(), i64_ty.into(), i64_ptr.into(), i8_ptr_ptr.into()],
                false,
            );
            let rt_fn = self.module.get_function("__axon_i64_to_str_radix").unwrap_or_else(|| {
                self.module.add_function("__axon_i64_to_str_radix", rt_fn_ty, None)
            });

            let fn_ty = str_ty.fn_type(&[i64_ty.into(), i64_ty.into()], false);
            let fn_val = self.module.add_function("i64_to_str_radix", fn_ty, None);
            let bb = self.context.append_basic_block(fn_val, "entry");
            let saved = self.builder.get_insert_block();
            self.builder.position_at_end(bb);

            let n    = fn_val.get_nth_param(0).unwrap().into_int_value();
            let base = fn_val.get_nth_param(1).unwrap().into_int_value();

            // Stack slots for out-params.
            let out_len_slot = self.builder.build_alloca(i64_ty, "radix_olen").unwrap();
            let out_ptr_slot = self.builder.build_alloca(i8_ptr, "radix_optr").unwrap();
            // Cast *i8* → i8** for the runtime call.
            let out_ptr_slot_cast = self.builder.build_pointer_cast(
                out_ptr_slot, i8_ptr_ptr, "radix_ptrptr",
            ).unwrap();

            self.builder.build_call(rt_fn, &[
                n.into(),
                base.into(),
                out_len_slot.into(),
                out_ptr_slot_cast.into(),
            ], "radix_call").unwrap();

            let out_len = self.builder.build_load(i64_ty, out_len_slot, "radix_len").unwrap().into_int_value();
            let out_ptr = self.builder.build_load(i8_ptr, out_ptr_slot, "radix_ptr").unwrap().into_pointer_value();

            let mut result = str_ty.const_zero();
            result = self.builder.build_insert_value(result, out_len, 0, "radix_r0").unwrap().into_struct_value();
            result = self.builder.build_insert_value(result, out_ptr, 1, "radix_r1").unwrap().into_struct_value();
            self.builder.build_return(Some(&result)).unwrap();

            if let Some(b) = saved { self.builder.position_at_end(b); }
            self.functions.insert("i64_to_str_radix".to_string(), fn_val);
            self.fn_return_types.insert("i64_to_str_radix".to_string(), Type::Str);
        }


    }

    /// Forward-declare every top-level function so mutual recursion resolves.
    pub fn declare_functions(&mut self, program: &ast::Program) {
        self.declare_builtins();
        self.declare_types(program);
        self.declare_enum_types(program);

        // Collect trait definitions first (needed for vtable thunk declaration).
        for item in &program.items {
            if let ast::Item::TraitDef(td) = item {
                self.trait_defs.insert(td.name.clone(), td.clone());
            }
        }

        for item in &program.items {
            match item {
                ast::Item::FnDef(f) => {
                    self.declare_one_fn(f);
                    self.fn_axon_params.insert(
                        f.name.clone(),
                        f.params.iter().map(|p| p.ty.clone()).collect(),
                    );
                }
                ast::Item::ImplBlock(blk) => {
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        self.declare_one_fn_named(m, &mangled);
                    }
                }
                _ => {}
            }
        }

        // Declare vtable thunks for every impl block.
        self.declare_vtable_thunks(program);
    }

    /// For each `impl Trait for Type`, declare a thunk function per trait method.
    /// The thunk takes `ptr` as self (for uniform vtable ABI) and calls the concrete impl.
    fn declare_vtable_thunks(&mut self, program: &ast::Program) {
        let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());

        // Collect all impl blocks first (avoid borrow issues with trait_defs).
        let impls: Vec<ast::ImplBlock> = program.items.iter()
            .filter_map(|item| if let ast::Item::ImplBlock(b) = item { Some(b.clone()) } else { None })
            .collect();

        for blk in &impls {
            let type_name = ast_type_simple_name(&blk.for_type);
            let trait_name = &blk.trait_name;

            let trait_def = match self.trait_defs.get(trait_name).cloned() {
                Some(td) => td,
                None => continue,
            };

            for tm in &trait_def.methods {
                let thunk_name = format!("__vtbl_{trait_name}_{type_name}_{}", tm.name);

                // Thunk params: (ptr self, non-self args...)
                let mut param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = vec![i8_ptr.into()];
                for p in &tm.params {
                    if p.name == "self" { continue; }
                    if let Some(llvm_ty) = self.llvm_type_from_axon(&p.ty) {
                        param_tys.push(llvm_ty.into());
                    }
                }

                let ret_sem = tm.return_type.as_ref()
                    .map(|t| self.axon_type_to_semantic(t))
                    .unwrap_or(crate::types::Type::Unit);

                let (fn_val, fn_ty) = match self.llvm_type(&ret_sem) {
                    Some(ret_ty) => {
                        let fn_ty = ret_ty.fn_type(&param_tys, false);
                        let fv = self.module.add_function(&thunk_name, fn_ty, None);
                        (fv, fn_ty)
                    }
                    None => {
                        let fn_ty = self.context.void_type().fn_type(&param_tys, false);
                        let fv = self.module.add_function(&thunk_name, fn_ty, None);
                        (fv, fn_ty)
                    }
                };

                self.functions.insert(thunk_name, fn_val);
                // Store one thunk type per (trait, method) pair for indirect dispatch.
                self.vtable_thunk_types.insert(
                    (trait_name.clone(), tm.name.clone()),
                    fn_ty,
                );
            }
        }
    }

    fn declare_types(&mut self, program: &ast::Program) {
        for item in &program.items {
            if let ast::Item::TypeDef(td) = item {
                let field_types: Vec<BasicTypeEnum<'ctx>> = td
                    .fields
                    .iter()
                    .filter_map(|f| self.llvm_type_from_axon(&f.ty))
                    .collect();
                let named_struct = self.context.opaque_struct_type(&td.name);
                named_struct.set_body(&field_types, false);
                let field_names: Vec<String> =
                    td.fields.iter().map(|f| f.name.clone()).collect();
                self.struct_fields.insert(td.name.clone(), field_names);
            }
        }
    }

    /// Declare LLVM struct types for enums.
    ///
    /// Layout: `{ i32 tag, [max_payload_size x i8] payload }`
    /// where `max_payload_size` is the maximum byte size of any variant's fields.
    fn declare_enum_types(&mut self, program: &ast::Program) {
        for item in &program.items {
            if let ast::Item::EnumDef(ed) = item {
                let i32_ty = self.context.i32_type();
                let i8_ty = self.context.i8_type();

                // Compute field semantic types and payload size for each variant.
                let mut variants_info: Vec<(String, usize, Vec<Type>)> = Vec::new();
                let mut max_size: u64 = 0;

                for (tag_int, variant) in ed.variants.iter().enumerate() {
                    let field_types: Vec<Type> = variant
                        .fields
                        .iter()
                        .map(|f| self.axon_type_to_semantic(&f.ty))
                        .collect();
                    let payload_size: u64 = field_types
                        .iter()
                        .map(|t| self.llvm_sizeof(t).unwrap_or(8))
                        .sum();
                    if payload_size > max_size {
                        max_size = payload_size;
                    }
                    variants_info.push((variant.name.clone(), tag_int, field_types));
                }

                // Ensure at least 1 byte payload so LLVM doesn't complain.
                let payload_size = max_size.max(1) as u32;

                let struct_name = format!("{}_enum", ed.name);
                let named_struct = self.context.opaque_struct_type(&struct_name);
                named_struct.set_body(
                    &[i32_ty.into(), i8_ty.array_type(payload_size).into()],
                    false,
                );

                self.enum_variants.insert(ed.name.clone(), variants_info);
            }
        }
    }

    fn declare_one_fn(&mut self, f: &ast::FnDef) -> FunctionValue<'ctx> {
        self.declare_one_fn_named(f, &f.name.clone())
    }

    fn declare_one_fn_named(&mut self, f: &ast::FnDef, name: &str) -> FunctionValue<'ctx> {
        // Build parameter type list.
        let param_tys: Vec<BasicMetadataTypeEnum<'ctx>> = f
            .params
            .iter()
            .filter_map(|p| self.llvm_type_from_axon(&p.ty))
            .map(|t| t.into())
            .collect();

        // Build return type.
        // Special case: the entry-point `main` with no return annotation is
        // lowered to `i32 main()` so the C runtime gets a well-defined exit
        // code. All other Unit-returning functions stay `void`.
        let ret_sem = f
            .return_type
            .as_ref()
            .map(|t| self.axon_type_to_semantic(t))
            .unwrap_or(Type::Unit);

        let fn_val = if name == "main" && matches!(ret_sem, Type::Unit) {
            let fn_ty = self.context.i32_type().fn_type(&param_tys, false);
            self.module.add_function("main", fn_ty, None)
        } else {
            match self.llvm_type(&ret_sem) {
                Some(ret_ty) => {
                    let fn_ty = ret_ty.fn_type(&param_tys, /*variadic=*/ false);
                    self.module.add_function(name, fn_ty, None)
                }
                None => {
                    let fn_ty = self.context.void_type().fn_type(&param_tys, false);
                    self.module.add_function(name, fn_ty, None)
                }
            }
        };

        self.fn_return_types.insert(name.to_string(), ret_sem);
        self.functions.insert(name.to_string(), fn_val);
        fn_val
    }

    // ── Pass 2: emit program ─────────────────────────────────────────────────

    /// Emit LLVM IR for the entire program (call after `declare_functions`).
    pub fn emit_program(&mut self, program: &ast::Program) {
        // Collect (mangled_name, FnDef) pairs for all functions including impl methods.
        let mut fn_work: Vec<(String, ast::FnDef)> = Vec::new();
        for item in &program.items {
            match item {
                ast::Item::FnDef(f) => fn_work.push((f.name.clone(), f.clone())),
                ast::Item::ImplBlock(blk) => {
                    let type_name = ast_type_simple_name(&blk.for_type);
                    for m in &blk.methods {
                        let mangled = format!("{type_name}__{}", m.name);
                        fn_work.push((mangled, m.clone()));
                    }
                }
                _ => {}
            }
        }

        // Populate fndefs for comptime evaluation.
        for (name, f) in &fn_work {
            self.fndefs.insert(name.clone(), f.clone());
        }

        // Evaluate module-level comptime let bindings (in source order so that
        // later bindings can reference earlier ones).
        for item in &program.items {
            if let ast::Item::LetDef { name, value, .. } = item {
                let evaluator = crate::comptime::Evaluator {
                    env: self.comptime_env.clone(),
                    fns: &self.fndefs,
                };
                match evaluator.eval(value) {
                    Ok(cv) => { self.comptime_env.insert(name.clone(), cv); }
                    Err(e) => eprintln!("comptime[E0701]: {e}"),
                }
            }
        }

        // Emit vtable thunk bodies (before user functions, so vtable globals can reference them).
        self.emit_vtable_thunks(program);
        // Emit vtable global constants.
        self.emit_vtable_globals(program);

        for (name, f) in &fn_work {
            let llvm_fn = match self.functions.get(name.as_str()).copied() {
                Some(v) => v,
                None => self.declare_one_fn_named(f, name),
            };
            self.emit_fn(f, llvm_fn);
        }
    }

    /// Emit the body of each vtable thunk function.
    fn emit_vtable_thunks(&mut self, program: &ast::Program) {
        let impls: Vec<ast::ImplBlock> = program.items.iter()
            .filter_map(|item| if let ast::Item::ImplBlock(b) = item { Some(b.clone()) } else { None })
            .collect();

        for blk in &impls {
            let type_name = ast_type_simple_name(&blk.for_type);
            let trait_name = blk.trait_name.clone();

            let trait_def = match self.trait_defs.get(&trait_name).cloned() {
                Some(td) => td,
                None => continue,
            };

            for tm in &trait_def.methods {
                let thunk_name = format!("__vtbl_{trait_name}_{type_name}_{}", tm.name);
                let concrete_name = format!("{type_name}__{}", tm.name);

                let thunk_fn = match self.functions.get(&thunk_name).copied() {
                    Some(v) => v,
                    None => continue,
                };
                let concrete_fn = match self.functions.get(&concrete_name).copied() {
                    Some(v) => v,
                    None => continue,
                };

                let saved = self.builder.get_insert_block();
                let entry = self.context.append_basic_block(thunk_fn, "entry");
                self.builder.position_at_end(entry);

                // Parameter 0 is `ptr self_ptr`; load the concrete type from it.
                let self_ptr = thunk_fn.get_nth_param(0).unwrap().into_pointer_value();

                // Determine concrete LLVM type for the `self` parameter.
                let concrete_llvm_ty = self.llvm_type_from_axon(&blk.for_type);

                let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();

                // If the concrete method has a self param, load it from the pointer.
                let has_self_param = blk.methods.iter()
                    .find(|m| m.name == tm.name)
                    .map(|m| m.params.iter().any(|p| p.name == "self"))
                    .unwrap_or(false);

                if has_self_param {
                    if let Some(ty) = concrete_llvm_ty {
                        let self_val = self.builder.build_load(ty, self_ptr, "self_val").unwrap();
                        call_args.push(self_val.into());
                    } else {
                        // Opaque self — pass the ptr directly.
                        call_args.push(self_ptr.into());
                    }
                }

                // Forward non-self arguments (params 1..N from the thunk).
                let non_self_count = tm.params.iter().filter(|p| p.name != "self").count();
                for i in 0..non_self_count {
                    if let Some(arg) = thunk_fn.get_nth_param((i + 1) as u32) {
                        call_args.push(arg.into());
                    }
                }

                let call = self.builder.build_call(concrete_fn, &call_args, "thunk_ret").unwrap();
                let ret_sem = tm.return_type.as_ref()
                    .map(|t| self.axon_type_to_semantic(t))
                    .unwrap_or(crate::types::Type::Unit);

                if matches!(ret_sem, crate::types::Type::Unit) {
                    self.builder.build_return(None).unwrap();
                } else if let Some(ret_val) = call.try_as_basic_value().left() {
                    self.builder.build_return(Some(&ret_val)).unwrap();
                } else {
                    self.builder.build_return(None).unwrap();
                }

                if let Some(b) = saved { self.builder.position_at_end(b); }
            }
        }
    }

    /// Emit one `@vtable_Trait_Type = constant [N x ptr] [...]` global per impl block.
    fn emit_vtable_globals(&mut self, program: &ast::Program) {
        let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());

        let impls: Vec<ast::ImplBlock> = program.items.iter()
            .filter_map(|item| if let ast::Item::ImplBlock(b) = item { Some(b.clone()) } else { None })
            .collect();

        for blk in &impls {
            let type_name = ast_type_simple_name(&blk.for_type);
            let trait_name = blk.trait_name.clone();

            let trait_def = match self.trait_defs.get(&trait_name).cloned() {
                Some(td) => td,
                None => continue,
            };

            // Build array of thunk function pointers in trait method declaration order.
            let mut thunk_ptrs: Vec<inkwell::values::PointerValue<'ctx>> = Vec::new();
            for tm in &trait_def.methods {
                let thunk_name = format!("__vtbl_{trait_name}_{type_name}_{}", tm.name);
                if let Some(fv) = self.functions.get(&thunk_name).copied() {
                    thunk_ptrs.push(fv.as_global_value().as_pointer_value());
                }
            }

            let n = thunk_ptrs.len();
            if n == 0 { continue; }

            let arr_ty = i8_ptr.array_type(n as u32);
            // inkwell's const_array for pointer arrays uses PointerType::const_array.
            let arr_const = i8_ptr.const_array(&thunk_ptrs);

            let global_name = format!("vtable_{trait_name}_{type_name}");
            let global = self.module.add_global(arr_ty, None, &global_name);
            global.set_initializer(&arr_const);
            global.set_constant(true);

            self.vtable_globals.insert((trait_name, type_name), global);
        }
    }

    // ── Function bodies ───────────────────────────────────────────────────────

    fn emit_fn(&mut self, f: &ast::FnDef, llvm_fn: FunctionValue<'ctx>) {
        let entry = self.context.append_basic_block(llvm_fn, "entry");
        self.builder.position_at_end(entry);

        // Save outer locals/types; reset for this function scope.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_result_types = self.current_result_types.take();

        // Determine return semantic type early (needed for current_result_types).
        let ret_sem = f
            .return_type
            .as_ref()
            .map(|t| self.axon_type_to_semantic(t))
            .unwrap_or(Type::Unit);

        // Set current_result_types when this function returns Result<T,E>.
        if let Type::Result(ok_ty, err_ty) = &ret_sem {
            self.current_result_types = Some((*ok_ty.clone(), *err_ty.clone()));
        }

        // Bind parameters to named allocas.
        for (i, param) in f.params.iter().enumerate() {
            let sem_ty = self.axon_type_to_semantic(&param.ty);
            if let Some(llvm_ty) = self.llvm_type(&sem_ty) {
                let alloca = self.builder.build_alloca(llvm_ty, &param.name).unwrap();
                if let Some(arg) = llvm_fn.get_nth_param(i as u32) {
                    self.builder.build_store(alloca, arg).unwrap();
                }
                self.locals.insert(param.name.clone(), (alloca, llvm_ty));
                self.local_types.insert(param.name.clone(), sem_ty);
            }
        }

        let body_val = self.emit_expr(&f.body, llvm_fn);

        // Emit return if the builder is still on a live block.
        if self
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            if f.name == "main" && matches!(ret_sem, Type::Unit) {
                let zero = self.context.i32_type().const_int(0, false);
                self.builder.build_return(Some(&zero)).unwrap();
            } else {
                match body_val {
                    Some(v) if !matches!(ret_sem, Type::Unit) => {
                        self.builder.build_return(Some(&v)).unwrap();
                    }
                    None if !matches!(ret_sem, Type::Unit) => {
                        // No value from body but function has non-void return type:
                        // emit a zero value of the appropriate type to keep IR valid.
                        if let Some(ret_llvm_ty) = self.llvm_type(&ret_sem) {
                            let zero_val = ret_llvm_ty.const_zero();
                            self.builder.build_return(Some(&zero_val)).unwrap();
                        } else {
                            self.builder.build_return(None).unwrap();
                        }
                    }
                    _ => {
                        self.builder.build_return(None).unwrap();
                    }
                }
            }
        }

        // Restore outer scope.
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.current_result_types = saved_result_types;
    }

    // ── Expression emission ───────────────────────────────────────────────────

    /// Core expression emitter. Returns the LLVM value (or None for Unit/void).
    fn emit_expr(
        &mut self,
        expr: &ast::Expr,
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        match expr {
            // ── Literal ──────────────────────────────────────────────────────
            ast::Expr::Literal(lit) => Some(self.emit_literal(lit)),

            // ── Identifier (load from local) ─────────────────────────────────
            ast::Expr::Ident(name) => {
                if let Some((ptr, llvm_ty)) = self.locals.get(name).cloned() {
                    let val = self.builder.build_load(llvm_ty, ptr, name).unwrap();
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
                    let field_ptr = self.builder
                        .build_struct_gep(env_ty, env_ptr, idx, name)
                        .unwrap();
                    let i64_ty = self.context.i64_type();
                    let val = self.builder
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
            ast::Expr::Let { name, value }
            | ast::Expr::Own { name, value }
            | ast::Expr::RefBind { name, value } => {
                let sem_ty = self.infer_expr_sem_type(value);
                let val = self.emit_expr(value, fn_val)?;
                let alloca = self.builder.build_alloca(val.get_type(), name).unwrap();
                self.builder.build_store(alloca, val).unwrap();
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
                let lhs = self.emit_expr(left, fn_val)?;
                let rhs = self.emit_expr(right, fn_val)?;
                // Prefer the semantic type from inference (distinguishes u32/u64
                // from i32/i64) then fall back to the LLVM-level value hint.
                let ty = self.infer_expr_sem_type(left)
                    .unwrap_or_else(|| self.value_type_hint(&lhs));
                Some(self.emit_binop(op, lhs, rhs, &ty))
            }

            // ── Unary operation ──────────────────────────────────────────────
            ast::Expr::UnaryOp { op, operand } => {
                let val = self.emit_expr(operand, fn_val)?;
                match op {
                    ast::UnaryOp::Neg => match val {
                        BasicValueEnum::IntValue(i) => {
                            let neg = self.builder.build_int_neg(i, "neg").unwrap();
                            Some(neg.into())
                        }
                        BasicValueEnum::FloatValue(f) => {
                            let neg = self.builder.build_float_neg(f, "fneg").unwrap();
                            Some(neg.into())
                        }
                        _ => None,
                    },
                    ast::UnaryOp::Not => match val {
                        BasicValueEnum::IntValue(i) => {
                            let r = self.builder.build_not(i, "not").unwrap();
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
                }
            }

            // ── Function call ─────────────────────────────────────────────────
            ast::Expr::Call { callee, args } => {
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let i64_ty = self.context.i64_type();

                // Try to resolve callee as a global function first.
                let maybe_fn_v = match callee.as_ref() {
                    ast::Expr::Ident(name) => self.functions.get(name.as_str()).copied(),
                    // Chan::new / chan<T>() — StructLit callees with known names.
                    ast::Expr::StructLit { name, fields } if fields.is_empty() => {
                        // chan::<T> → alias to Chan::new
                        if name.starts_with("chan::<") {
                            self.functions.get("Chan::new").copied()
                        } else {
                            self.functions.get(name.as_str()).copied()
                        }
                    }
                    _ => None,
                };

                // Try closure call: callee is a local holding a {fn_ptr, env_ptr} struct.
                if maybe_fn_v.is_none() {
                    if let ast::Expr::Ident(name) = callee.as_ref() {
                        if let Some(&(alloca, ty)) = self.locals.get(name.as_str()) {
                            let fat = self.builder.build_load(ty, alloca, "closure").unwrap();
                            if let BasicValueEnum::StructValue(sv) = fat {
                                let fp = self.builder.build_extract_value(sv, 0, "cfp").unwrap();
                                let ep = self.builder.build_extract_value(sv, 1, "cep").unwrap();
                                // Build arg list: env_ptr first, then explicit args.
                                let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> =
                                    vec![ep.into()];
                                for a in args {
                                    if let Some(v) = self.emit_expr(a, fn_val) {
                                        call_args.push(v.into());
                                    }
                                }
                                // Build an indirect call via fn pointer.
                                let fn_ptr = self.builder
                                    .build_pointer_cast(
                                        fp.into_pointer_value(),
                                        ptr_ty,
                                        "fp_cast",
                                    )
                                    .unwrap();
                                // Build the function type for the indirect call.
                                let mut ipt: Vec<BasicMetadataTypeEnum<'ctx>> =
                                    vec![ptr_ty.into()];
                                for _ in args {
                                    ipt.push(i64_ty.into());
                                }
                                let indirect_ty = i64_ty.fn_type(&ipt, false);
                                let call = self.builder
                                    .build_indirect_call(indirect_ty, fn_ptr, &call_args, "icall")
                                    .unwrap();
                                return call.try_as_basic_value().left();
                            }
                        }
                    }
                }

                // Resolve the callee to an LLVM FunctionValue (direct call).
                let fn_v = maybe_fn_v?;

                // Get declared parameter types to coerce mismatched integer widths.
                let param_tys: Vec<BasicTypeEnum<'ctx>> = fn_v.get_type().get_param_types();

                // Get Axon-level param types for DynTrait coercion.
                let axon_params: Vec<ast::AxonType> = if let ast::Expr::Ident(name) = callee.as_ref() {
                    self.fn_axon_params.get(name).cloned().unwrap_or_default()
                } else {
                    Vec::new()
                };

                let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                for (i, a) in args.iter().enumerate() {
                    // Check for DynTrait coercion: concrete type → fat pointer.
                    let axon_param_ty = axon_params.get(i);
                    let is_dyn_param = matches!(axon_param_ty, Some(ast::AxonType::DynTrait(_)));

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
                                    let data_alloca = self.builder.build_alloca(concrete_llvm_ty, "dyn_data").unwrap();
                                    self.builder.build_store(data_alloca, val).unwrap();

                                    // Build fat pointer { data_ptr, vtable_ptr }.
                                    let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
                                    let fat_ty = self.context.struct_type(&[i8_ptr.into(), i8_ptr.into()], false);
                                    let fat_undef = fat_ty.get_undef();

                                    let data_cast = self.builder.build_pointer_cast(data_alloca, i8_ptr, "data_cast").unwrap();
                                    let vtbl_ptr = vtable_global.as_pointer_value();
                                    let vtbl_cast = self.builder.build_pointer_cast(vtbl_ptr, i8_ptr, "vtbl_cast").unwrap();

                                    let fat0 = self.builder.build_insert_value(fat_undef, data_cast, 0, "fat0").unwrap();
                                    let fat1 = self.builder.build_insert_value(fat0.into_struct_value(), vtbl_cast, 1, "fat1").unwrap();
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
                                self.builder.build_int_truncate(iv, exp_int, "trunc").unwrap().into()
                            } else if actual < expect {
                                let sem_ty = self.infer_expr_sem_type(a);
                                let is_unsigned = matches!(
                                    sem_ty,
                                    Some(Type::U8) | Some(Type::U16) | Some(Type::U32) | Some(Type::U64)
                                );
                                if is_unsigned {
                                    self.builder.build_int_z_extend(iv, exp_int, "zext").unwrap().into()
                                } else {
                                    self.builder.build_int_s_extend(iv, exp_int, "sext").unwrap().into()
                                }
                            } else {
                                val
                            }
                        }
                        // float → int: e.g. to_str(f64_val) where to_str takes i64
                        (Some(BasicTypeEnum::IntType(exp_int)), BasicValueEnum::FloatValue(fv)) => {
                            self.builder.build_float_to_signed_int(fv, exp_int, "ftoi").unwrap().into()
                        }
                        // int → float
                        (Some(BasicTypeEnum::FloatType(exp_flt)), BasicValueEnum::IntValue(iv)) => {
                            self.builder.build_signed_int_to_float(iv, exp_flt, "itof").unwrap().into()
                        }
                        _ => val,
                    };
                    arg_vals.push(coerced.into());
                }

                let call = self
                    .builder
                    .build_call(fn_v, &arg_vals, "call")
                    .unwrap();
                call.try_as_basic_value().left()
            }

            // ── Method call — dispatches to mangled `TypeName__method` fn ──────
            ast::Expr::MethodCall { receiver, method, args } => {
                // --- DynTrait: vtable dispatch ---
                let recv_sem_ty = self.infer_expr_sem_type(receiver);
                if let Some(Type::DynTrait(trait_name)) = recv_sem_ty {
                    let recv_val = self.emit_expr(receiver, fn_val)?;
                    let fat = recv_val.into_struct_value();
                    let data_ptr = self.builder.build_extract_value(fat, 0, "data_ptr")
                        .unwrap().into_pointer_value();
                    let vtbl_ptr = self.builder.build_extract_value(fat, 1, "vtbl_ptr")
                        .unwrap().into_pointer_value();

                    // Find method index in the trait definition.
                    let trait_def = self.trait_defs.get(&trait_name).cloned()?;
                    let method_idx = trait_def.methods.iter().position(|m| m.name == *method)?;

                    // GEP into vtable array to get the function pointer slot.
                    let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
                    let arr_ty = i8_ptr.array_type(trait_def.methods.len() as u32);
                    let idx_zero = self.context.i64_type().const_zero();
                    let idx_m = self.context.i64_type().const_int(method_idx as u64, false);
                    let fn_slot = unsafe {
                        self.builder.build_gep(arr_ty, vtbl_ptr, &[idx_zero, idx_m], "fn_slot").unwrap()
                    };
                    let fn_ptr = self.builder.build_load(i8_ptr, fn_slot, "fn_ptr")
                        .unwrap().into_pointer_value();

                    // Build call args: data_ptr + any extra args.
                    let mut call_args: Vec<BasicMetadataValueEnum<'ctx>> = vec![data_ptr.into()];
                    for a in args {
                        if let Some(v) = self.emit_expr(a, fn_val) {
                            call_args.push(v.into());
                        }
                    }

                    let thunk_ty = self.vtable_thunk_types.get(&(trait_name, method.clone())).copied()?;
                    let call = self.builder.build_indirect_call(thunk_ty, fn_ptr, &call_args, "vtbl_call").unwrap();
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
                    .or_else(|| self.functions.get(method.as_str()).copied());

                if let Some(fn_v) = fn_v {
                    let mut arg_vals: Vec<BasicMetadataValueEnum<'ctx>> = Vec::new();
                    // Prepend the receiver as the first argument.
                    // For Chan methods, cast receiver to i8* (opaque pointer ABI).
                    if let Some(rv) = recv_val {
                        let is_chan_method = matches!(method.as_str(), "send" | "recv" | "clone");
                        let rv = if is_chan_method {
                            if let BasicValueEnum::PointerValue(pv) = rv {
                                let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
                                self.builder.build_pointer_cast(pv, i8_ptr, "chan_cast").unwrap().into()
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
                    let call = self
                        .builder
                        .build_call(fn_v, &arg_vals, "mcall")
                        .unwrap();
                    return call.try_as_basic_value().left();
                }
                None
            }

            // ── If / else ─────────────────────────────────────────────────────
            ast::Expr::If { cond, then, else_ } => {
                let cond_val = self.emit_expr(cond, fn_val)?;
                let cond_int = match cond_val {
                    BasicValueEnum::IntValue(i) => i,
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
                // Emit Option<i64 placeholder> with no inner value.
                let placeholder = Type::I64;
                Some(self.emit_option(std::option::Option::None, &placeholder))
            }

            // ── Return ────────────────────────────────────────────────────────
            ast::Expr::Return(maybe_val) => {
                match maybe_val {
                    std::option::Option::Some(e) => {
                        if let Some(v) = self.emit_expr(e, fn_val) {
                            self.builder.build_return(Some(&v)).unwrap();
                        } else {
                            self.builder.build_return(None).unwrap();
                        }
                    }
                    std::option::Option::None => {
                        self.builder.build_return(None).unwrap();
                    }
                }
                None
            }

            // ── Array literal ─────────────────────────────────────────────────
            ast::Expr::Array(elems) => {
                if elems.is_empty() {
                    // Return a zero-length slice struct.
                    let i64_ty = self.context.i64_type();
                    let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                    let slice_ty = self.context.struct_type(
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
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                // Compute element size from the LLVM type bit-width.
                let elem_size_bytes: u64 = match elem_ty {
                    BasicTypeEnum::IntType(it) => (it.get_bit_width() as u64 + 7) / 8,
                    BasicTypeEnum::FloatType(ft) => {
                        if ft == self.context.f32_type() { 4 } else { 8 }
                    }
                    BasicTypeEnum::StructType(_) | BasicTypeEnum::ArrayType(_)
                    | BasicTypeEnum::PointerType(_) | BasicTypeEnum::VectorType(_) => 8,
                };
                let total_bytes = i64_ty.const_int(elem_size_bytes * n as u64, false);
                let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                    let malloc_ty = ptr_ty.fn_type(&[i64_ty.into()], false);
                    self.module.add_function("malloc", malloc_ty, None)
                });
                let malloc_call = self.builder
                    .build_call(malloc_fn, &[total_bytes.into()], "arrdata")
                    .unwrap();
                let raw_ptr = malloc_call.try_as_basic_value().left().unwrap().into_pointer_value();
                // Cast to typed element pointer for GEP.
                let elem_ptr_ty = elem_ty.ptr_type(AddressSpace::default());
                let elem_data_ptr = self.builder
                    .build_pointer_cast(raw_ptr, elem_ptr_ty, "arrelemptr")
                    .unwrap();
                for (idx, v) in vals.iter().enumerate() {
                    let idx_val = i64_ty.const_int(idx as u64, false);
                    let gep = unsafe {
                        self.builder
                            .build_gep(elem_ty, elem_data_ptr, &[idx_val], "arrelem")
                            .unwrap()
                    };
                    self.builder.build_store(gep, *v).unwrap();
                }

                // Build slice struct { len, ptr }.
                let slice_ty = self.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()],
                    false,
                );
                let len_val = i64_ty.const_int(n as u64, false);
                // Cast malloc ptr to opaque i8* for the slice data field.
                let data_ptr = self
                    .builder
                    .build_pointer_cast(raw_ptr, ptr_ty, "sliceptr")
                    .unwrap();
                let slice_alloca = self.builder.build_alloca(slice_ty, "slice").unwrap();
                // Store len.
                let len_ptr = self
                    .builder
                    .build_struct_gep(slice_ty, slice_alloca, 0, "lenptr")
                    .unwrap();
                self.builder.build_store(len_ptr, len_val).unwrap();
                // Store data ptr.
                let data_field_ptr = self
                    .builder
                    .build_struct_gep(slice_ty, slice_alloca, 1, "dataptr")
                    .unwrap();
                self.builder.build_store(data_field_ptr, data_ptr).unwrap();
                let slice_val = self
                    .builder
                    .build_load(slice_ty, slice_alloca, "sliceval")
                    .unwrap();
                Some(slice_val)
            }

            // ── Struct literal: Name { field: expr, ... } ─────────────────────
            ast::Expr::StructLit { name, fields } => {
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
                    let enum_struct_ty = self.module.get_struct_type(&struct_name)?;

                    // Alloca for the enum struct { i32, [N x i8] }.
                    let alloca = self.builder.build_alloca(enum_struct_ty, &struct_name).unwrap();

                    // Store tag (field 0).
                    let i32_ty = self.context.i32_type();
                    let tag_ptr = self.builder
                        .build_struct_gep(enum_struct_ty, alloca, 0, "tagptr")
                        .unwrap();
                    self.builder
                        .build_store(tag_ptr, i32_ty.const_int(tag_int as u64, false))
                        .unwrap();

                    // Store each field into the payload (field 1) at byte offsets.
                    if !fields.is_empty() {
                        let i8_ty = self.context.i8_type();
                        let ptr_ty = i8_ty.ptr_type(AddressSpace::default());

                        // Get pointer to payload field.
                        let pay_ptr = self.builder
                            .build_struct_gep(enum_struct_ty, alloca, 1, "payptr")
                            .unwrap();
                        let pay_i8ptr = self.builder
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
                                    self.builder
                                        .build_gep(i8_ty, pay_i8ptr, &[offset_val], fname)
                                        .unwrap()
                                };
                                // Cast to the appropriate typed pointer and store.
                                let fval_ptr_ty = fval.get_type().ptr_type(AddressSpace::default());
                                let typed_ptr = self.builder
                                    .build_pointer_cast(field_ptr, fval_ptr_ty, "ftyptr")
                                    .unwrap();
                                self.builder.build_store(typed_ptr, fval).unwrap();
                                byte_offset += fsize;
                            }
                        }
                    }

                    let val = self.builder.build_load(enum_struct_ty, alloca, name).unwrap();
                    Some(val)
                } else {
                    // Regular struct literal.
                    let struct_ty = self.module.get_struct_type(name)?;
                    let field_names = self.struct_fields.get(name).cloned().unwrap_or_default();
                    let alloca = self.builder.build_alloca(struct_ty, name).unwrap();
                    for (fname, fexpr) in fields {
                        let idx = field_names.iter().position(|n| n == fname).unwrap_or(0) as u32;
                        if let Some(fval) = self.emit_expr(fexpr, fn_val) {
                            let fptr = self.builder
                                .build_struct_gep(struct_ty, alloca, idx, fname)
                                .unwrap();
                            self.builder.build_store(fptr, fval).unwrap();
                        }
                    }
                    let val = self.builder.build_load(struct_ty, alloca, name).unwrap();
                    Some(val)
                }
            }

            // ── Field access: receiver.field ──────────────────────────────────
            ast::Expr::FieldAccess { receiver, field } => {
                // Determine the struct name from the receiver's semantic type.
                // Handle both bare Ident receivers and chained FieldAccess receivers.
                let struct_name = self.sem_type_of_expr(receiver).and_then(|ty| {
                    if let Type::Struct(sn) = ty { Some(sn) } else { None }
                });

                if let Some(sname) = struct_name {
                    if let (Some(struct_ty), Some(field_names)) = (
                        self.module.get_struct_type(&sname),
                        self.struct_fields.get(&sname).cloned(),
                    ) {
                        if let Some(idx) = field_names.iter().position(|n| n == field) {
                            let recv_val = self.emit_expr(receiver, fn_val)?;
                            let recv_alloca = self.builder
                                .build_alloca(struct_ty, "recv_tmp")
                                .unwrap();
                            self.builder.build_store(recv_alloca, recv_val).unwrap();
                            let fptr = self.builder
                                .build_struct_gep(struct_ty, recv_alloca, idx as u32, field)
                                .unwrap();
                            if let Some(field_ty) = struct_ty.get_field_type_at_index(idx as u32) {
                                let fval = self.builder.build_load(field_ty, fptr, field).unwrap();
                                return Some(fval);
                            }
                        }
                    }
                }
                // Fallback: emit receiver for side-effects only.
                let _ = self.emit_expr(receiver, fn_val);
                None
            }

            // ── Index: receiver[index] ────────────────────────────────────────
            ast::Expr::Index { receiver, index } => {
                let elem_llvm_ty = if let ast::Expr::Ident(n) = receiver.as_ref() {
                    self.local_types.get(n).and_then(|ty| {
                        if let Type::Slice(inner) = ty { self.llvm_type(inner) } else { None }
                    })
                } else {
                    None
                };

                let slice_val = self.emit_expr(receiver, fn_val)?;
                let idx_val = self.emit_expr(index, fn_val)?;
                let elem_ty = elem_llvm_ty?;
                let idx_int = match idx_val {
                    BasicValueEnum::IntValue(i) => i,
                    _ => return None,
                };

                // Slice struct { i64, ptr }: extract the data pointer (field 1).
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()], false,
                );
                let slice_alloca = self.builder.build_alloca(slice_ty, "slicetmp").unwrap();
                self.builder.build_store(slice_alloca, slice_val).unwrap();
                let data_field_ptr = self.builder
                    .build_struct_gep(slice_ty, slice_alloca, 1, "dataptr")
                    .unwrap();
                let data_ptr = self.builder
                    .build_load(ptr_ty, data_field_ptr, "dataval")
                    .unwrap()
                    .into_pointer_value();
                let elem_ptr = unsafe {
                    self.builder
                        .build_gep(elem_ty, data_ptr, &[idx_int], "elemptr")
                        .unwrap()
                };
                let elem = self.builder.build_load(elem_ty, elem_ptr, "elemval").unwrap();
                Some(elem)
            }

            // ── Spawn: compile lambda then call __axon_spawn(fn_ptr, env_ptr) ──
            ast::Expr::Spawn(inner) => {
                // inner must be a lambda expression. Compile it to get the fat ptr.
                let fat = self.emit_expr(inner, fn_val)?;
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());

                // If we got a struct back, extract fn_ptr and env_ptr.
                let (fn_ptr_val, env_ptr_val) = match fat {
                    BasicValueEnum::StructValue(sv) => {
                        let fp = self.builder.build_extract_value(sv, 0, "spawn_fp").unwrap();
                        let ep = self.builder.build_extract_value(sv, 1, "spawn_ep").unwrap();
                        (fp, ep)
                    }
                    other => {
                        // Bare function pointer — wrap with null env.
                        let null_env = ptr_ty.const_null();
                        (other, null_env.into())
                    }
                };

                if let Some(spawn_fn) = self.functions.get("__axon_spawn").copied() {
                    self.builder.build_call(
                        spawn_fn,
                        &[fn_ptr_val.into(), env_ptr_val.into()],
                        "spawn",
                    ).unwrap();
                }
                // spawn returns unit
                None
            }

            // ── Comptime: evaluate at compile time, emit LLVM constant ──────────
            ast::Expr::Comptime(inner) => {
                let evaluator = crate::comptime::Evaluator {
                    env: self.comptime_env.clone(),
                    fns: &self.fndefs,
                };
                match evaluator.eval(inner) {
                    Ok(crate::comptime::ComptimeVal::Int(n)) => {
                        Some(self.context.i64_type().const_int(n as u64, true).into())
                    }
                    Ok(crate::comptime::ComptimeVal::Bool(b)) => {
                        Some(self.context.bool_type().const_int(b as u64, false).into())
                    }
                    Ok(crate::comptime::ComptimeVal::Float(f)) => {
                        Some(self.context.f64_type().const_float(f).into())
                    }
                    Ok(crate::comptime::ComptimeVal::Str(s)) => {
                        // Emit as a { i64 len, i8* ptr } struct matching Axon's Str layout.
                        let len = s.len() as u64;
                        let global = self.builder.build_global_string_ptr(&s, "comptime_str").unwrap();
                        let i64_ty = self.context.i64_type();
                        let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                        let str_ty = self.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                        let mut sv = str_ty.get_undef();
                        sv = self.builder.build_insert_value(sv, i64_ty.const_int(len, false), 0, "str_len").unwrap().into_struct_value();
                        sv = self.builder.build_insert_value(sv, global.as_pointer_value(), 1, "str_ptr").unwrap().into_struct_value();
                        Some(sv.into())
                    }
                    Err(e) => {
                        eprintln!("comptime evaluation error: {e}");
                        None
                    }
                }
            }

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
            ast::Expr::Lambda { params, body, captures } => {
                let lambda_name = format!("__lambda_{}", self.lambda_counter);
                self.lambda_counter += 1;

                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let closure_ty = self.context.struct_type(&[ptr_ty.into(), ptr_ty.into()], false);

                // ── Build the env struct type for captures ────────────────────
                // All captured variables are stored as i64 (Phase 4 limitation).
                let n_captures = captures.len();
                let env_field_tys: Vec<BasicTypeEnum<'ctx>> =
                    (0..n_captures).map(|_| i64_ty.into()).collect();
                let env_struct_ty = self.context.struct_type(&env_field_tys, false);

                // ── Declare the lambda function (env_ptr first, then params) ──
                let mut lambda_param_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
                    vec![ptr_ty.into()]; // env_ptr
                for _ in params {
                    lambda_param_tys.push(i64_ty.into());
                }
                let fn_ty = i64_ty.fn_type(&lambda_param_tys, false);
                let lambda_fn = self.module.add_function(&lambda_name, fn_ty, None);

                // ── Emit the lambda body ──────────────────────────────────────
                let entry_bb = self.context.append_basic_block(lambda_fn, "entry");
                let saved_ip = self.builder.get_insert_block();
                let saved_locals = std::mem::take(&mut self.locals);
                let saved_local_types = std::mem::take(&mut self.local_types);
                let saved_lambda_env = self.current_lambda_env.take();

                self.builder.position_at_end(entry_bb);

                // env_ptr is param 0; explicit params start at 1.
                let env_ptr_arg = lambda_fn.get_nth_param(0).unwrap().into_pointer_value();

                // Bind captured variables directly to their env struct field pointers.
                // Using the field pointer as the "alloca" means stores inside the lambda
                // persist across calls (required for mutable closures like make_counter).
                let mut capture_idx_map: HashMap<String, u32> = HashMap::new();
                if n_captures > 0 {
                    for (idx, (cap_name, _)) in captures.iter().enumerate() {
                        let field_ptr = self.builder
                            .build_struct_gep(env_struct_ty, env_ptr_arg, idx as u32, cap_name)
                            .unwrap();
                        self.locals.insert(cap_name.clone(), (field_ptr, i64_ty.into()));
                        capture_idx_map.insert(cap_name.clone(), idx as u32);
                    }
                }

                // Publish the env context so nested `Ident` lookups can fall back
                // to loading captures via GEP if the resolver missed them.
                self.current_lambda_env = Some((env_ptr_arg, env_struct_ty, capture_idx_map));

                // Bind explicit parameters (offset by 1 for env_ptr).
                for (i, p) in params.iter().enumerate() {
                    if let Some(arg) = lambda_fn.get_nth_param((i + 1) as u32) {
                        let alloca = self.builder.build_alloca(i64_ty, &p.name).unwrap();
                        self.builder.build_store(alloca, arg).unwrap();
                        self.locals.insert(p.name.clone(), (alloca, i64_ty.into()));
                    }
                }

                let body_val = self.emit_expr(body, lambda_fn);
                if self.builder.get_insert_block().and_then(|b| b.get_terminator()).is_none() {
                    match body_val {
                        Some(v) => { self.builder.build_return(Some(&v)).unwrap(); }
                        None => { self.builder.build_return(Some(&i64_ty.const_zero())).unwrap(); }
                    }
                }

                // Restore caller's state.
                self.locals = saved_locals;
                self.local_types = saved_local_types;
                self.current_lambda_env = saved_lambda_env;
                if let Some(b) = saved_ip { self.builder.position_at_end(b); }
                self.functions.insert(lambda_name.clone(), lambda_fn);

                // ── At the creation site: build the fat pointer struct ─────────
                let fn_ptr = self.builder
                    .build_pointer_cast(
                        lambda_fn.as_global_value().as_pointer_value(),
                        ptr_ty,
                        "lfp",
                    )
                    .unwrap();

                let env_ptr: BasicValueEnum<'ctx> = if n_captures > 0 {
                    // Malloc an env struct and populate it.
                    let malloc_fn = self.module.get_function("malloc").unwrap_or_else(|| {
                        let ty = ptr_ty.fn_type(&[i64_ty.into()], false);
                        self.module.add_function("malloc", ty, None)
                    });
                    let env_size = i64_ty.const_int(
                        (n_captures * 8) as u64, // 8 bytes per i64
                        false,
                    );
                    let raw = self.builder
                        .build_call(malloc_fn, &[env_size.into()], "env_alloc")
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
                            self.builder.build_load(ty, alloca, cap_name).unwrap()
                        } else {
                            i64_ty.const_zero().into()
                        };
                        let field_ptr = self.builder
                            .build_struct_gep(env_struct_ty, raw, idx as u32, &format!("env_f{idx}"))
                            .unwrap();
                        self.builder.build_store(field_ptr, cap_val).unwrap();
                    }
                    // Cast back to i8* for the fat pointer.
                    self.builder
                        .build_pointer_cast(raw, ptr_ty, "env_i8")
                        .unwrap()
                        .into()
                } else {
                    ptr_ty.const_null().into()
                };

                // Build { fn_ptr, env_ptr } fat pointer struct.
                let mut fat = closure_ty.get_undef();
                fat = self.builder.build_insert_value(fat, fn_ptr, 0, "fat0").unwrap().into_struct_value();
                fat = self.builder.build_insert_value(fat, env_ptr, 1, "fat1").unwrap().into_struct_value();
                Some(fat.into())
            }

            // ── Select (phase 1: stub) ────────────────────────────────────────
            // ── Select: non-blocking channel dispatch ────────────────────────
            // Lowers to:
            //   chans[n] = { emit_expr(arm.recv) for each arm }
            //   let ready = __axon_select(chans, n)
            //   switch ready -> arm bodies
            ast::Expr::Select(arms) => {
                let i8_ptr = self.context.i8_type().ptr_type(AddressSpace::default());
                let i64_ty = self.context.i64_type();
                let n = arms.len() as u64;

                // Allocate an array of i8* on the stack: [n x i8*]
                let arr_ty = i8_ptr.array_type(n as u32);
                let chans_alloca = self.builder.build_alloca(arr_ty, "select_chans").unwrap();

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
                                self.builder.build_pointer_cast(pv, i8_ptr, "chan_ptr").unwrap()
                            }
                            _ => continue,
                        };
                        let slot = unsafe {
                            self.builder.build_gep(
                                arr_ty,
                                chans_alloca,
                                &[i64_ty.const_int(0, false).into(), i64_ty.const_int(i as u64, false).into()],
                                "chan_slot",
                            ).unwrap()
                        };
                        self.builder.build_store(slot, as_ptr).unwrap();
                    }
                }

                // Cast array pointer to i8** for __axon_select.
                let chans_ptr = self.builder.build_pointer_cast(
                    chans_alloca,
                    i8_ptr.ptr_type(AddressSpace::default()),
                    "chans_ptr",
                ).unwrap();

                // Call __axon_select(chans, n) → i64 ready_idx.
                let ready_idx = if let Some(sel_fn) = self.functions.get("__axon_select").copied() {
                    self.builder.build_call(
                        sel_fn,
                        &[chans_ptr.into(), i64_ty.const_int(n, false).into()],
                        "select_idx",
                    ).unwrap().try_as_basic_value().left()
                } else {
                    None
                };

                let merge_bb = self.context.append_basic_block(fn_val, "select.merge");
                let else_bb = self.context.append_basic_block(fn_val, "select.else");

                // Build arm basic blocks.
                let arm_bbs: Vec<_> = arms.iter().enumerate()
                    .map(|(i, _)| self.context.append_basic_block(fn_val, &format!("select.arm{i}")))
                    .collect();

                // Build switch: pass all (tag, bb) cases at once.
                if let Some(BasicValueEnum::IntValue(iv)) = ready_idx {
                    let cases: Vec<_> = arm_bbs.iter().enumerate()
                        .map(|(i, bb)| (i64_ty.const_int(i as u64, false), *bb))
                        .collect();
                    self.builder.build_switch(iv, else_bb, &cases).unwrap();
                } else {
                    self.builder.build_unconditional_branch(else_bb).unwrap();
                }

                // Emit each arm body and jump to merge.
                for (arm, bb) in arms.iter().zip(arm_bbs.iter()) {
                    self.builder.position_at_end(*bb);
                    self.emit_expr(&arm.body, fn_val);
                    self.builder.build_unconditional_branch(merge_bb).unwrap();
                }

                // else: no arm ready — branch to merge (runtime will have blocked).
                self.builder.position_at_end(else_bb);
                self.builder.build_unconditional_branch(merge_bb).unwrap();

                self.builder.position_at_end(merge_bb);
                None
            }

            // ── While loop ────────────────────────────────────────────────────
            ast::Expr::While { cond, body } => {
                let cond_bb = self.context.append_basic_block(fn_val, "while.cond");
                let body_bb = self.context.append_basic_block(fn_val, "while.body");
                let exit_bb = self.context.append_basic_block(fn_val, "while.exit");

                // Push loop context so break/continue can find their targets.
                self.loop_stack.push((cond_bb, exit_bb));

                // Jump to condition check.
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                // Emit condition.
                self.builder.position_at_end(cond_bb);
                let cond_val = match self.emit_expr(cond, fn_val) {
                    Some(BasicValueEnum::IntValue(i)) => i,
                    _ => {
                        // If condition didn't produce a value, treat as infinite loop.
                        self.builder.build_unconditional_branch(body_bb).unwrap();
                        self.loop_stack.pop();
                        self.builder.position_at_end(exit_bb);
                        return Some(self.context.i64_type().const_zero().into());
                    }
                };
                self.builder.build_conditional_branch(cond_val, body_bb, exit_bb).unwrap();

                // Emit body.
                self.builder.position_at_end(body_bb);
                for stmt in body {
                    self.emit_expr(&stmt.expr, fn_val);
                    // Stop emitting if a terminator was added (e.g., return, break, continue).
                    if self.builder.get_insert_block().unwrap().get_terminator().is_some() {
                        break;
                    }
                }
                // Jump back to condition if not already terminated.
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }

                // Pop loop context after body is fully emitted.
                self.loop_stack.pop();

                // Continue after loop.
                self.builder.position_at_end(exit_bb);
                Some(self.context.i64_type().const_zero().into())
            }

            // ── While-let loop ───────────────────────────────────────────────
            // `while let <pattern> = <expr> { body }` — compiled as:
            //   loop { val = expr; if !pattern_matches(val) { break }; bind; body }
            ast::Expr::WhileLet { pattern, expr, body } => {
                let cond_bb = self.context.append_basic_block(fn_val, "wl.cond");
                let body_bb = self.context.append_basic_block(fn_val, "wl.body");
                let exit_bb = self.context.append_basic_block(fn_val, "wl.exit");

                self.loop_stack.push((cond_bb, exit_bb));
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                // Evaluate the scrutinee and test the pattern.
                self.builder.position_at_end(cond_bb);
                let subject = match self.emit_expr(expr, fn_val) {
                    Some(v) => v,
                    None => {
                        // Expression produced no value; treat as infinite loop.
                        self.builder.build_unconditional_branch(body_bb).unwrap();
                        self.loop_stack.pop();
                        self.builder.position_at_end(exit_bb);
                        return Some(self.context.i64_type().const_zero().into());
                    }
                };
                let matches = self.emit_pattern_test(pattern, subject);
                let cond_int = match matches {
                    BasicValueEnum::IntValue(i) => i,
                    _ => self.context.bool_type().const_int(1, false),
                };
                self.builder.build_conditional_branch(cond_int, body_bb, exit_bb).unwrap();

                // Bind pattern variables and emit body.
                self.builder.position_at_end(body_bb);
                self.emit_pattern_bindings(pattern, subject);
                for stmt in body {
                    self.emit_expr(&stmt.expr, fn_val);
                    if self.builder.get_insert_block().unwrap().get_terminator().is_some() {
                        break;
                    }
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(cond_bb).unwrap();
                }

                self.loop_stack.pop();
                self.builder.position_at_end(exit_bb);
                Some(self.context.i64_type().const_zero().into())
            }

            // ── For-in range loop ─────────────────────────────────────────────
            // `for i in start..end { body }` or `start..=end` (inclusive).
            ast::Expr::For { var, start, end, body, inclusive } => {
                let i64_ty = self.context.i64_type();

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
                let var_ptr = self.builder.build_alloca(i64_ty, var).unwrap();
                self.builder.build_store(var_ptr, start_val).unwrap();
                // Register the variable so body statements can read it.
                self.locals.insert(var.clone(), (var_ptr, i64_ty.into()));

                let cond_bb = self.context.append_basic_block(fn_val, "for.cond");
                let body_bb = self.context.append_basic_block(fn_val, "for.body");
                let incr_bb = self.context.append_basic_block(fn_val, "for.incr");
                let exit_bb = self.context.append_basic_block(fn_val, "for.exit");

                self.loop_stack.push((incr_bb, exit_bb));

                // Jump to condition.
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                // Condition: i < end  (exclusive)  or  i <= end  (inclusive)
                self.builder.position_at_end(cond_bb);
                let cur = self.builder.build_load(i64_ty, var_ptr, "for.i").unwrap().into_int_value();
                let pred = if *inclusive { inkwell::IntPredicate::SLE } else { inkwell::IntPredicate::SLT };
                let cmp = self.builder.build_int_compare(
                    pred, cur, end_val, "for.cmp").unwrap();
                self.builder.build_conditional_branch(cmp, body_bb, exit_bb).unwrap();

                // Body.
                self.builder.position_at_end(body_bb);
                for stmt in body {
                    self.emit_expr(&stmt.expr, fn_val);
                    if self.builder.get_insert_block().unwrap().get_terminator().is_some() {
                        break;
                    }
                }
                if self.builder.get_insert_block().unwrap().get_terminator().is_none() {
                    self.builder.build_unconditional_branch(incr_bb).unwrap();
                }

                // Increment: i = i + 1
                self.builder.position_at_end(incr_bb);
                let cur2 = self.builder.build_load(i64_ty, var_ptr, "for.i2").unwrap().into_int_value();
                let next = self.builder.build_int_add(cur2, i64_ty.const_int(1, false), "for.next").unwrap();
                self.builder.build_store(var_ptr, next).unwrap();
                self.builder.build_unconditional_branch(cond_bb).unwrap();

                self.loop_stack.pop();
                self.locals.remove(var);

                self.builder.position_at_end(exit_bb);
                Some(i64_ty.const_zero().into())
            }

            // ── Break / Continue ──────────────────────────────────────────────
            ast::Expr::Break => {
                if let Some(&(_cont, exit)) = self.loop_stack.last() {
                    self.builder.build_unconditional_branch(exit).unwrap();
                }
                None
            }
            ast::Expr::Continue => {
                if let Some(&(cont, _exit)) = self.loop_stack.last() {
                    self.builder.build_unconditional_branch(cont).unwrap();
                }
                None
            }

            // ── Assign (rebind existing local without let) ────────────────────
            ast::Expr::Assign { name, value } => {
                if let Some(val) = self.emit_expr(value, fn_val) {
                    if let Some((ptr, _llvm_ty)) = self.locals.get(name).copied() {
                        self.builder.build_store(ptr, val).unwrap();
                    }
                }
                None
            }

            // ── FmtStr: lower to a chain of axon_concat calls ────────────────
            ast::Expr::FmtStr { parts } => {
                // We build the result left-to-right:
                //   acc = ""
                //   for each part: acc = axon_concat(acc, part_value)
                let i8_ptr = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
                let i64_ty = self.context.i64_type();
                let str_ty = self.context.struct_type(&[i64_ty.into(), i8_ptr.into()], false);

                // Start with an empty string literal (use a unique name per fmtstr).
                let fmtstr_id = self.fmtstr_counter;
                self.fmtstr_counter += 1;
                let empty_arr_ty = self.context.i8_type().array_type(1);
                let empty_name = format!("fmtstr_empty_{fmtstr_id}");
                let empty_global = self.module.add_global(empty_arr_ty, None, &empty_name);
                empty_global.set_initializer(
                    &self.context.i8_type().const_array(&[self.context.i8_type().const_int(0, false)])
                );
                empty_global.set_constant(true);
                let empty_ptr = self.builder
                    .build_pointer_cast(empty_global.as_pointer_value(), i8_ptr, "emptyptr")
                    .unwrap();

                // Build the empty str struct as the initial accumulator.
                let init_alloca = self.builder.build_alloca(str_ty, "fmtinit").unwrap();
                let init_len_ptr = self.builder.build_struct_gep(str_ty, init_alloca, 0, "il").unwrap();
                let init_dat_ptr = self.builder.build_struct_gep(str_ty, init_alloca, 1, "id").unwrap();
                self.builder.build_store(init_len_ptr, i64_ty.const_int(0, false)).unwrap();
                self.builder.build_store(init_dat_ptr, empty_ptr).unwrap();
                let mut acc: BasicValueEnum<'ctx> = self.builder
                    .build_load(str_ty, init_alloca, "fmtacc0")
                    .unwrap();

                let concat_fn = self.functions.get("axon_concat").copied()?;

                for part in parts {
                    let part_val: BasicValueEnum<'ctx> = match part {
                        ast::FmtPart::Lit(s) => {
                            // Emit the literal as a str value.
                            let bytes = s.as_bytes();
                            let lit_len = i64_ty.const_int(bytes.len() as u64, false);
                            let arr_ty = self.context.i8_type().array_type(bytes.len() as u32 + 1);
                            let lit_name = format!("fmtlit_{fmtstr_id}_{}", self.fmtstr_counter);
                            self.fmtstr_counter += 1;
                            let g = self.module.add_global(arr_ty, None, &lit_name);
                            let byte_vals: Vec<_> = bytes
                                .iter()
                                .chain(std::iter::once(&0u8))
                                .map(|&b| self.context.i8_type().const_int(b as u64, false))
                                .collect();
                            g.set_initializer(&self.context.i8_type().const_array(&byte_vals));
                            g.set_constant(true);
                            let lit_ptr = self.builder
                                .build_pointer_cast(g.as_pointer_value(), i8_ptr, "litptr")
                                .unwrap();
                            let lit_alloca = self.builder.build_alloca(str_ty, "litstr").unwrap();
                            let lp = self.builder.build_struct_gep(str_ty, lit_alloca, 0, "lp").unwrap();
                            let dp = self.builder.build_struct_gep(str_ty, lit_alloca, 1, "dp").unwrap();
                            self.builder.build_store(lp, lit_len).unwrap();
                            self.builder.build_store(dp, lit_ptr).unwrap();
                            self.builder.build_load(str_ty, lit_alloca, "litval").unwrap()
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
                                            self.builder.build_call(f, &[iv.into()], "fmtb")
                                                .unwrap().try_as_basic_value().left()?
                                        } else { v }
                                    } else {
                                        // i64 → to_str
                                        if let Some(f) = self.functions.get("to_str").copied() {
                                            self.builder.build_call(f, &[iv.into()], "fmti")
                                                .unwrap().try_as_basic_value().left()?
                                        } else { v }
                                    }
                                }
                                BasicValueEnum::FloatValue(fv) => {
                                    // f64 → to_str_f64
                                    if let Some(f) = self.functions.get("to_str_f64").copied() {
                                        self.builder.build_call(f, &[fv.into()], "fmtf")
                                            .unwrap().try_as_basic_value().left()?
                                    } else { v }
                                }
                                _ => v,
                            }
                        }
                    };
                    // acc = axon_concat(acc, part_val)
                    let res = self.builder.build_call(
                        concat_fn,
                        &[acc.into(), part_val.into()],
                        "fmtcat",
                    ).unwrap();
                    acc = res.try_as_basic_value().left()?;
                }

                Some(acc)
            }
        }
    }

    // ── Literal emission ──────────────────────────────────────────────────────

    fn comptime_val_to_llvm(&self, cv: &crate::comptime::ComptimeVal) -> BasicValueEnum<'ctx> {
        use crate::comptime::ComptimeVal;
        match cv {
            ComptimeVal::Int(n) => self.context.i64_type().const_int(*n as u64, true).into(),
            ComptimeVal::Bool(b) => self.context.bool_type().const_int(*b as u64, false).into(),
            ComptimeVal::Float(f) => self.context.f64_type().const_float(*f).into(),
            ComptimeVal::Str(s) => {
                let global = self.builder.build_global_string_ptr(s, "comptime_str").unwrap();
                let i64_ty = self.context.i64_type();
                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self.context.struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                let mut sv = str_ty.get_undef();
                sv = self.builder.build_insert_value(sv, i64_ty.const_int(s.len() as u64, false), 0, "s_len").unwrap().into_struct_value();
                sv = self.builder.build_insert_value(sv, global.as_pointer_value(), 1, "s_ptr").unwrap().into_struct_value();
                sv.into()
            }
        }
    }

    fn emit_literal(&self, lit: &ast::Literal) -> BasicValueEnum<'ctx> {
        match lit {
            ast::Literal::Int(n) => {
                self.context
                    .i64_type()
                    .const_int(*n as u64, /*sign_extend=*/ true)
                    .into()
            }
            ast::Literal::Float(f) => {
                self.context.f64_type().const_float(*f).into()
            }
            ast::Literal::Bool(b) => {
                self.context
                    .bool_type()
                    .const_int(if *b { 1 } else { 0 }, false)
                    .into()
            }
            ast::Literal::Str(s) => {
                // Build a global constant for the string bytes, then construct
                // the { i64, ptr } struct.
                let bytes = s.as_bytes();
                let len_val = self.context.i64_type().const_int(bytes.len() as u64, false);

                // Create a global byte array for the string data.
                let i8_ty = self.context.i8_type();
                let arr_ty = i8_ty.array_type(bytes.len() as u32 + 1); // null-terminated
                // Use add_global which auto-dedups by letting LLVM pick unique names.
                let global = self.module.add_global(arr_ty, None, "str_data");
                let byte_vals: Vec<_> = bytes
                    .iter()
                    .chain(std::iter::once(&0u8)) // null terminator
                    .map(|&b| i8_ty.const_int(b as u64, false))
                    .collect();
                global.set_initializer(&i8_ty.const_array(&byte_vals));
                global.set_constant(true);

                let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
                let ptr = global.as_pointer_value();
                let cast_ptr = self
                    .builder
                    .build_pointer_cast(ptr, ptr_ty, "strptr")
                    .unwrap();

                let i64_ty = self.context.i64_type();
                let str_ty = self.context.struct_type(
                    &[i64_ty.into(), ptr_ty.into()],
                    false,
                );
                // Build the struct value via an alloca + stores.
                let alloca = self.builder.build_alloca(str_ty, "strlit").unwrap();
                let len_ptr = self
                    .builder
                    .build_struct_gep(str_ty, alloca, 0, "lenptr")
                    .unwrap();
                self.builder.build_store(len_ptr, len_val).unwrap();
                let data_ptr = self
                    .builder
                    .build_struct_gep(str_ty, alloca, 1, "dataptr")
                    .unwrap();
                self.builder.build_store(data_ptr, cast_ptr).unwrap();
                self.builder.build_load(str_ty, alloca, "strval").unwrap()
            }
        }
    }

    // ── Binary operation emission ─────────────────────────────────────────────

    fn emit_binop(
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
                ast::BinOp::Add => self.builder.build_int_add(l, r, "add").unwrap().into(),
                ast::BinOp::Sub => self.builder.build_int_sub(l, r, "sub").unwrap().into(),
                ast::BinOp::Mul => self.builder.build_int_mul(l, r, "mul").unwrap().into(),
                ast::BinOp::Div => if is_unsigned {
                    self.builder.build_int_unsigned_div(l, r, "udiv").unwrap().into()
                } else {
                    self.builder.build_int_signed_div(l, r, "div").unwrap().into()
                },
                ast::BinOp::Eq => self
                    .builder
                    .build_int_compare(IntPredicate::EQ, l, r, "eq")
                    .unwrap()
                    .into(),
                ast::BinOp::NotEq => self
                    .builder
                    .build_int_compare(IntPredicate::NE, l, r, "ne")
                    .unwrap()
                    .into(),
                ast::BinOp::Lt => self
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::ULT } else { IntPredicate::SLT },
                        l, r, "lt",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::Gt => self
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::UGT } else { IntPredicate::SGT },
                        l, r, "gt",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::LtEq => self
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::ULE } else { IntPredicate::SLE },
                        l, r, "le",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::GtEq => self
                    .builder
                    .build_int_compare(
                        if is_unsigned { IntPredicate::UGE } else { IntPredicate::SGE },
                        l, r, "ge",
                    )
                    .unwrap()
                    .into(),
                ast::BinOp::Rem => if is_unsigned {
                    self.builder.build_int_unsigned_rem(l, r, "urem").unwrap().into()
                } else {
                    self.builder.build_int_signed_rem(l, r, "rem").unwrap().into()
                },
                ast::BinOp::And => self.builder.build_and(l, r, "and").unwrap().into(),
                ast::BinOp::Or => self.builder.build_or(l, r, "or").unwrap().into(),
            },

            // Float arithmetic.
            (BasicValueEnum::FloatValue(l), BasicValueEnum::FloatValue(r)) => match op {
                ast::BinOp::Add => self.builder.build_float_add(l, r, "fadd").unwrap().into(),
                ast::BinOp::Sub => self.builder.build_float_sub(l, r, "fsub").unwrap().into(),
                ast::BinOp::Mul => self.builder.build_float_mul(l, r, "fmul").unwrap().into(),
                ast::BinOp::Div => self.builder.build_float_div(l, r, "fdiv").unwrap().into(),
                ast::BinOp::Eq => self
                    .builder
                    .build_float_compare(FloatPredicate::OEQ, l, r, "feq")
                    .unwrap()
                    .into(),
                ast::BinOp::NotEq => self
                    .builder
                    .build_float_compare(FloatPredicate::ONE, l, r, "fne")
                    .unwrap()
                    .into(),
                ast::BinOp::Lt => self
                    .builder
                    .build_float_compare(FloatPredicate::OLT, l, r, "flt")
                    .unwrap()
                    .into(),
                ast::BinOp::Gt => self
                    .builder
                    .build_float_compare(FloatPredicate::OGT, l, r, "fgt")
                    .unwrap()
                    .into(),
                ast::BinOp::LtEq => self
                    .builder
                    .build_float_compare(FloatPredicate::OLE, l, r, "fle")
                    .unwrap()
                    .into(),
                ast::BinOp::GtEq => self
                    .builder
                    .build_float_compare(FloatPredicate::OGE, l, r, "fge")
                    .unwrap()
                    .into(),
                ast::BinOp::Rem => self.builder.build_float_rem(l, r, "frem").unwrap().into(),
                // Bool ops on floats — truncate to i1 first.
                ast::BinOp::And | ast::BinOp::Or => {
                    let zero = l.get_type().const_zero();
                    let li = self
                        .builder
                        .build_float_compare(FloatPredicate::ONE, l, zero, "ftoi_l")
                        .unwrap();
                    let ri = self
                        .builder
                        .build_float_compare(FloatPredicate::ONE, r, zero, "ftoi_r")
                        .unwrap();
                    match op {
                        ast::BinOp::And => self.builder.build_and(li, ri, "fand").unwrap().into(),
                        _ => self.builder.build_or(li, ri, "for").unwrap().into(),
                    }
                }
            },

            // String struct equality: `a == b` / `a != b` where both are { i64, i8* }.
            // Delegates to the str_eq builtin function declared in declare_builtins.
            (BasicValueEnum::StructValue(l), BasicValueEnum::StructValue(r))
                if matches!(op, ast::BinOp::Eq | ast::BinOp::NotEq) =>
            {
                let str_eq_fn = self.module.get_function("str_eq");
                if let Some(eq_fn) = str_eq_fn {
                    let result = self.builder
                        .build_call(eq_fn, &[l.into(), r.into()], "seq")
                        .unwrap()
                        .try_as_basic_value()
                        .left()
                        .unwrap()
                        .into_int_value();
                    if matches!(op, ast::BinOp::NotEq) {
                        // Flip the result: NotEq = !Eq
                        self.builder.build_not(result, "sne").unwrap().into()
                    } else {
                        result.into()
                    }
                } else {
                    // str_eq not declared yet — return false (shouldn't happen)
                    self.context.bool_type().const_int(0, false).into()
                }
            }

            // Mismatched or unsupported — return lhs unchanged.
            (l, _) => l,
        }
    }

    // ── Option emission ────────────────────────────────────────────────────────

    fn emit_option(
        &mut self,
        inner: Option<BasicValueEnum<'ctx>>,
        inner_ty: &Type,
    ) -> BasicValueEnum<'ctx> {
        let tag_ty = self.context.bool_type();

        match inner {
            Some(val) => {
                let llvm_inner = val.get_type();
                let opt_ty = self.context.struct_type(
                    &[tag_ty.into(), llvm_inner],
                    false,
                );
                let alloca = self.builder.build_alloca(opt_ty, "some").unwrap();
                let tag_ptr = self
                    .builder
                    .build_struct_gep(opt_ty, alloca, 0, "tagptr")
                    .unwrap();
                let val_ptr = self
                    .builder
                    .build_struct_gep(opt_ty, alloca, 1, "valptr")
                    .unwrap();
                self.builder
                    .build_store(tag_ptr, tag_ty.const_int(1, false))
                    .unwrap();
                self.builder.build_store(val_ptr, val).unwrap();
                self.builder.build_load(opt_ty, alloca, "someval").unwrap()
            }
            None => {
                // None: if we have a known inner type, build { false, undef }
                let llvm_inner = self.llvm_type(inner_ty);
                if let Some(inner_llvm_ty) = llvm_inner {
                    let opt_ty = self.context.struct_type(
                        &[tag_ty.into(), inner_llvm_ty],
                        false,
                    );
                    let alloca = self.builder.build_alloca(opt_ty, "none").unwrap();
                    let tag_ptr = self
                        .builder
                        .build_struct_gep(opt_ty, alloca, 0, "tagptr")
                        .unwrap();
                    self.builder
                        .build_store(tag_ptr, tag_ty.const_zero())
                        .unwrap();
                    self.builder.build_load(opt_ty, alloca, "noneval").unwrap()
                } else {
                    // Unit inner: None is just `false`.
                    tag_ty.const_zero().into()
                }
            }
        }
    }

    // ── Result emission ───────────────────────────────────────────────────────

    fn emit_result(
        &mut self,
        is_ok: bool,
        val: BasicValueEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let tag_ty = self.context.bool_type();
        let tag_val = tag_ty.const_int(if is_ok { 1 } else { 0 }, false);

        if let Some((ok_ty, err_ty)) = self.current_result_types.clone() {
            // Canonical union layout: { i1, [max(sizeof T, sizeof E) x i8] }
            // Both Ok and Err arms produce the same LLVM struct type so phi nodes work.
            let ok_size = self.llvm_sizeof(&ok_ty).unwrap_or(0);
            let err_size = self.llvm_sizeof(&err_ty).unwrap_or(0);
            let payload_size = ok_size.max(err_size).max(1) as u32;
            let i8_ty = self.context.i8_type();
            let payload_arr_ty = i8_ty.array_type(payload_size);
            let result_ty = self.context.struct_type(
                &[tag_ty.into(), payload_arr_ty.into()], false,
            );

            let alloca = self.builder.build_alloca(result_ty, "result").unwrap();
            let tag_ptr = self.builder
                .build_struct_gep(result_ty, alloca, 0, "tagptr")
                .unwrap();
            let pay_ptr = self.builder
                .build_struct_gep(result_ty, alloca, 1, "payptr")
                .unwrap();

            self.builder.build_store(tag_ptr, tag_val).unwrap();
            // Cast the payload pointer to the value's typed pointer before storing,
            // so the store type matches the pointer's element type.
            let val_ptr_ty = val.get_type().ptr_type(inkwell::AddressSpace::default());
            let typed_pay_ptr = self.builder
                .build_pointer_cast(pay_ptr, val_ptr_ty, "pay_typed")
                .unwrap();
            self.builder.build_store(typed_pay_ptr, val).unwrap();

            self.builder.build_load(result_ty, alloca, "resultval").unwrap()
        } else {
            // Fallback: simple { i1, T } when no type info is available.
            let payload_ty = val.get_type();
            let result_ty = self.context.struct_type(
                &[tag_ty.into(), payload_ty], false,
            );
            let alloca = self.builder.build_alloca(result_ty, "result").unwrap();
            let tag_ptr = self.builder
                .build_struct_gep(result_ty, alloca, 0, "tagptr")
                .unwrap();
            let val_ptr = self.builder
                .build_struct_gep(result_ty, alloca, 1, "valptr")
                .unwrap();
            self.builder.build_store(tag_ptr, tag_val).unwrap();
            self.builder.build_store(val_ptr, val).unwrap();
            self.builder.build_load(result_ty, alloca, "resultval").unwrap()
        }
    }

    /// Cast the `[N x i8]` payload of a canonical Result union to a typed value.
    fn extract_result_payload(
        &mut self,
        payload: BasicValueEnum<'ctx>,
        typed_ty: &Type,
    ) -> Option<BasicValueEnum<'ctx>> {
        let llvm_ty = self.llvm_type(typed_ty)?;
        let arr_ty = payload.get_type();
        let ptr_ty = self.context.i8_type().ptr_type(AddressSpace::default());
        let arr_alloca = self.builder.build_alloca(arr_ty, "payloadalc").unwrap();
        self.builder.build_store(arr_alloca, payload).unwrap();
        let typed_ptr = self.builder
            .build_pointer_cast(arr_alloca, ptr_ty, "payloadptr")
            .unwrap();
        let val = self.builder.build_load(llvm_ty, typed_ptr, "payloadval").unwrap();
        Some(val)
    }

    // ── ? operator ────────────────────────────────────────────────────────────

    /// Emit the `?` (early-return-on-Err) operator.
    ///
    /// The result struct is `{ i1 tag, payload }`. If tag == 0 (Err), emit an
    /// early return of the whole result value. Otherwise extract and return the
    /// Ok payload.
    fn emit_question(
        &mut self,
        result_val: BasicValueEnum<'ctx>,
        fn_val: FunctionValue<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let _result_ty = match result_val.get_type() {
            BasicTypeEnum::StructType(s) => s,
            _ => return result_val, // not a struct — just pass through
        };

        // Extract the tag (field 0).
        let tag = self
            .builder
            .build_extract_value(result_val.into_struct_value(), 0, "qtag")
            .unwrap();

        let tag_int = match tag {
            BasicValueEnum::IntValue(i) => i,
            _ => return result_val,
        };

        // Build the two branches: ok_bb and err_bb.
        let ok_bb = self.context.append_basic_block(fn_val, "q_ok");
        let err_bb = self.context.append_basic_block(fn_val, "q_err");

        self.builder
            .build_conditional_branch(tag_int, ok_bb, err_bb)
            .unwrap();

        // --- Err branch: early return an Err using the *outer* function's Result type.
        // This ensures the return type matches the enclosing function's signature.
        self.builder.position_at_end(err_bb);
        let err_payload = self
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
        self.builder.build_return(Some(&err_return_val)).unwrap();

        // --- Ok branch: extract the typed Ok payload using extract_result_payload.
        self.builder.position_at_end(ok_bb);
        let raw_payload = self
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

    // ── Match emission ────────────────────────────────────────────────────────

    /// Emit a match expression. Each arm is tested in order with a cond branch;
    /// matching arms jump to their body block. All arms converge via a phi node
    /// in the merge block (if the match produces a value).
    fn emit_match(
        &mut self,
        subject: BasicValueEnum<'ctx>,
        arms: &[ast::MatchArm],
        fn_val: FunctionValue<'ctx>,
    ) -> Option<BasicValueEnum<'ctx>> {
        if arms.is_empty() {
            return None;
        }

        let merge_bb = self.context.append_basic_block(fn_val, "match_merge");
        let mut arm_results: Vec<(BasicValueEnum<'ctx>, inkwell::basic_block::BasicBlock<'ctx>)> =
            Vec::new();
        // Track the last arm's test block so we can add the false-branch incoming to phi.
        let mut last_test_bb: Option<inkwell::basic_block::BasicBlock<'ctx>> = None;

        for (i, arm) in arms.iter().enumerate() {
            let test_bb = self
                .context
                .append_basic_block(fn_val, &format!("arm{i}_test"));
            let body_bb = self
                .context
                .append_basic_block(fn_val, &format!("arm{i}_body"));
            let next_bb = if i + 1 < arms.len() {
                self.context
                    .append_basic_block(fn_val, &format!("arm{i}_next"))
            } else {
                // Last arm: false branch goes to merge_bb. Track this test_bb.
                last_test_bb = Some(test_bb);
                merge_bb
            };

            self.builder.build_unconditional_branch(test_bb).unwrap();
            self.builder.position_at_end(test_bb);

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
                        self.builder.build_and(m, g, "guarded").unwrap().into()
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
                _ => self.context.bool_type().const_int(1, false),
            };

            self.builder
                .build_conditional_branch(cond_int, body_bb, next_bb)
                .unwrap();

            // Emit body.
            self.builder.position_at_end(body_bb);
            // Bind pattern variables.
            self.emit_pattern_bindings(&arm.pattern, subject);
            let body_val = self.emit_expr(&arm.body, fn_val);

            let current_bb = self.builder.get_insert_block().unwrap();
            if current_bb.get_terminator().is_none() {
                self.builder.build_unconditional_branch(merge_bb).unwrap();
                // Only add to phi predecessors when this block flows to merge_bb.
                if let Some(v) = body_val {
                    arm_results.push((v, current_bb));
                }
            }
            // Arms with a terminator (e.g., `return`) are NOT phi predecessors.

            if i + 1 < arms.len() {
                self.builder.position_at_end(next_bb);
            }
        }

        self.builder.position_at_end(merge_bb);

        // Build phi if all arms produce a value of the same type.
        // Note: the last arm's test_bb false-branch also goes to merge_bb, so
        // we must add an `undef` incoming for that predecessor to keep the phi valid.
        if arm_results.len() == arms.len() && !arm_results.is_empty() {
            let val_ty = arm_results[0].0.get_type();
            let phi = self.builder.build_phi(val_ty, "match_val").unwrap();
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
    fn emit_pattern_test(
        &mut self,
        pattern: &ast::Pattern,
        subject: BasicValueEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        let true_val = self.context.bool_type().const_int(1, false);
        let false_val = self.context.bool_type().const_int(0, false);

        match pattern {
            ast::Pattern::Wildcard | ast::Pattern::Ident(_) => true_val.into(),

            ast::Pattern::Literal(lit) => {
                let lit_val = self.emit_literal(lit);
                match (subject, lit_val) {
                    (BasicValueEnum::IntValue(s), BasicValueEnum::IntValue(l)) => self
                        .builder
                        .build_int_compare(IntPredicate::EQ, s, l, "patlit")
                        .unwrap()
                        .into(),
                    (BasicValueEnum::FloatValue(s), BasicValueEnum::FloatValue(l)) => self
                        .builder
                        .build_float_compare(FloatPredicate::OEQ, s, l, "patflit")
                        .unwrap()
                        .into(),
                    // String literal match: use strcmp.
                    (BasicValueEnum::StructValue(subj_sv), BasicValueEnum::StructValue(lit_sv)) => {
                        // Both are { i64, ptr } str structs. Extract data pointers and call strcmp.
                        let strcmp_fn = self.module.get_function("strcmp").unwrap_or_else(|| {
                            let i8_ptr = self.context.i8_type().ptr_type(inkwell::AddressSpace::default());
                            let strcmp_ty = self.context.i32_type().fn_type(&[i8_ptr.into(), i8_ptr.into()], false);
                            self.module.add_function("strcmp", strcmp_ty, None)
                        });
                        let subj_ptr = self.builder.build_extract_value(subj_sv, 1, "subj_ptr")
                            .unwrap().into_pointer_value();
                        let lit_ptr = self.builder.build_extract_value(lit_sv, 1, "lit_ptr")
                            .unwrap().into_pointer_value();
                        let cmp_result = self.builder
                            .build_call(strcmp_fn, &[subj_ptr.into(), lit_ptr.into()], "strcmp_res")
                            .unwrap()
                            .try_as_basic_value().left().unwrap().into_int_value();
                        self.builder
                            .build_int_compare(IntPredicate::EQ, cmp_result, self.context.i32_type().const_zero(), "streq")
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
                        self.builder.build_extract_value(sv, 0, "opttag").unwrap()
                    {
                        return self
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
                        self.builder.build_extract_value(sv, 0, "opttag").unwrap()
                    {
                        let is_some = self
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
                            .builder
                            .build_extract_value(sv, 1, "optval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner_val);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
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
                        self.builder.build_extract_value(sv, 0, "restag").unwrap()
                    {
                        let is_ok = self
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_int(1, false),
                                "isok",
                            )
                            .unwrap();
                        let inner = self
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
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
                        self.builder.build_extract_value(sv, 0, "restag").unwrap()
                    {
                        let is_err = self
                            .builder
                            .build_int_compare(
                                IntPredicate::EQ,
                                tag,
                                tag.get_type().const_zero(),
                                "iserr",
                            )
                            .unwrap();
                        let inner = self
                            .builder
                            .build_extract_value(sv, 1, "resval")
                            .unwrap();
                        let inner_match = self.emit_pattern_test(inner_pat, inner);
                        if let BasicValueEnum::IntValue(im) = inner_match {
                            return self
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
                            self.builder.build_extract_value(sv, 0, "enumtag")
                        {
                            let expected = tag_val.get_type().const_int(tag_int as u64, false);
                            return self
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
    fn emit_pattern_bindings(
        &mut self,
        pattern: &ast::Pattern,
        subject: BasicValueEnum<'ctx>,
    ) {
        match pattern {
            ast::Pattern::Ident(name) => {
                let subject_ty = subject.get_type();
                let alloca = self
                    .builder
                    .build_alloca(subject_ty, name)
                    .unwrap();
                self.builder.build_store(alloca, subject).unwrap();
                self.locals.insert(name.clone(), (alloca, subject_ty));
            }
            ast::Pattern::Some(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let Ok(inner_val) = self.builder.build_extract_value(sv, 1, "patinner") {
                        self.emit_pattern_bindings(inner, inner_val);
                    }
                }
            }
            ast::Pattern::Ok(inner) => {
                if let BasicValueEnum::StructValue(sv) = subject {
                    if let Ok(payload) = self.builder.build_extract_value(sv, 1, "okpayload") {
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
                    if let Ok(payload) = self.builder.build_extract_value(sv, 1, "errpayload") {
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
                    let enum_struct_ty = match self.module.get_struct_type(&struct_name) {
                        Some(ty) => ty,
                        None => return,
                    };
                    let alloca = self.builder.build_alloca(enum_struct_ty, "enumtmp").unwrap();
                    self.builder.build_store(alloca, sv).unwrap();

                    // GEP to payload field (index 1).
                    let pay_ptr = self.builder
                        .build_struct_gep(enum_struct_ty, alloca, 1, "pay")
                        .unwrap();

                    let i8_ty = self.context.i8_type();
                    let i32_ty = self.context.i32_type();
                    let ptr_ty = i8_ty.ptr_type(AddressSpace::default());

                    let pay_i8ptr = self.builder
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
                                self.builder
                                    .build_gep(i8_ty, pay_i8ptr, &[offset_val], "fieldptr")
                                    .unwrap()
                            };
                            let typed_ptr = self.builder
                                .build_pointer_cast(field_ptr, ptr_ty, "tfptr")
                                .unwrap();
                            let field_val = self.builder
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
                            self.builder.build_extract_value(sv, i as u32, "sfield")
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
                            self.builder.build_extract_value(sv, i as u32, "telem")
                        {
                            self.emit_pattern_bindings(pat, elem_val);
                        }
                    }
                }
            }
            _ => {} // Wildcard, Literal, None — no bindings
        }
    }

    // ── If/else emission ──────────────────────────────────────────────────────

    fn emit_if(
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

        let then_bb = self.context.append_basic_block(fn_val, "if_then");
        let else_bb = self.context.append_basic_block(fn_val, "if_else");
        let merge_bb = self.context.append_basic_block(fn_val, "if_merge");

        self.builder
            .build_conditional_branch(cond_int, then_bb, else_bb)
            .unwrap();

        // Then branch.
        self.builder.position_at_end(then_bb);
        let then_val = self.emit_expr(then_expr, fn_val);
        let then_end = self.builder.get_insert_block().unwrap();
        if then_end.get_terminator().is_none() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        // Else branch.
        self.builder.position_at_end(else_bb);
        let else_val = if let Some(e) = else_expr {
            self.emit_expr(e, fn_val)
        } else {
            None
        };
        let else_end = self.builder.get_insert_block().unwrap();
        if else_end.get_terminator().is_none() {
            self.builder.build_unconditional_branch(merge_bb).unwrap();
        }

        self.builder.position_at_end(merge_bb);

        // Build phi if both branches produce a value of the same type.
        match (then_val, else_val) {
            (Some(tv), Some(ev)) if tv.get_type() == ev.get_type() => {
                let phi = self.builder.build_phi(tv.get_type(), "ifval").unwrap();
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
                    let phi = self.builder.build_phi(tv.get_type(), "ifval").unwrap();
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

    // ── Output ────────────────────────────────────────────────────────────────

    /// Write the LLVM IR text representation to `path` (usually `*.ll`).
    pub fn write_ir(&self, path: &str) -> Result<(), String> {
        self.module
            .print_to_file(Path::new(path))
            .map_err(|e| e.to_string())
    }

    /// Compile the module to a native binary at `output_path`.
    ///
    /// Steps:
    /// Compile the module to a native binary (default target, no cross-compilation).
    ///
    /// Convenience wrapper around `compile_to_binary_target` with `target_triple = None`.
    pub fn compile_to_binary(&self, output_path: &str, release: bool) -> Result<(), String> {
        self.compile_to_binary_target(output_path, release, None)
    }

    /// Compile the module to a binary for an optional target triple.
    ///
    /// Steps:
    /// 1. Verify the LLVM IR.
    /// 2. Initialize the appropriate LLVM backend (native or all targets for cross).
    /// 3. Create the `TargetMachine`.
    /// 4. Emit an object file to a temp path.
    /// 5. Link with the system linker (or cross-linker from `~/.config/axon/cross.toml`).
    pub fn compile_to_binary_target(
        &self,
        output_path: &str,
        release: bool,
        target_triple: Option<&str>,
    ) -> Result<(), String> {
        self.module.verify().map_err(|e| format!("IR verification failed: {}", e.to_string()))?;
        emit_object_and_link(&self.module, output_path, release, target_triple)
    }

    /// Serialize the compiled LLVM IR as bitcode bytes (for the incremental cache).
    pub fn emit_bitcode(&self) -> Vec<u8> {
        self.module.write_bitcode_to_memory().as_slice().to_vec()
    }

    // ── Test runner ───────────────────────────────────────────────────────
}

// ── Standalone build helpers ──────────────────────────────────────────────────

/// Load LLVM bitcode bytes into a fresh context and compile to a binary.
///
/// Used by the incremental cache on a cache hit: the IR emission stages are
/// skipped; we go directly from cached bitcode to object file → binary.
pub fn compile_bitcode_to_binary(
    bitcode: &[u8],
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    use inkwell::memory_buffer::MemoryBuffer;
    let ctx = inkwell::context::Context::create();
    let buf = MemoryBuffer::create_from_memory_range(bitcode, "cached_bitcode");
    let module = ctx
        .create_module_from_ir(buf)
        .map_err(|e| format!("[E0906] cached bitcode could not be loaded: {}", e.to_string()))?;
    emit_object_and_link(&module, output_path, release, target_triple)
}

/// Initialize LLVM targets, create a `TargetMachine`, emit an object file, and
/// link it into a binary at `output_path`.
///
/// When `target_triple` is `None` the native host triple is used.  When it is
/// `Some(triple)` all LLVM backends are initialized and the specified triple is
/// used (cross-compilation).
fn emit_object_and_link(
    module: &inkwell::module::Module<'_>,
    output_path: &str,
    release: bool,
    target_triple: Option<&str>,
) -> Result<(), String> {
    let opt = if release {
        OptimizationLevel::Default
    } else {
        OptimizationLevel::None
    };

    let (triple, machine) = if let Some(triple_str) = target_triple {
        // Cross-compilation: initialise every backend so any target is reachable.
        Target::initialize_all(&InitializationConfig::default());
        let triple = TargetTriple::create(triple_str);
        let target = Target::from_triple(&triple).map_err(|e| {
            format!(
                "[E0904] target '{}' not supported by this LLVM build: {}",
                triple_str, e
            )
        })?;
        let machine = target
            .create_target_machine(&triple, "generic", "", opt, RelocMode::PIC, CodeModel::Default)
            .ok_or_else(|| {
                format!(
                    "[E0904] could not create target machine for '{}'",
                    triple_str
                )
            })?;
        (triple, machine)
    } else {
        // Native compilation.
        Target::initialize_native(&InitializationConfig::default())
            .map_err(|e| format!("LLVM native target init: {e}"))?;
        let triple = TargetMachine::get_default_triple();
        let target =
            Target::from_triple(&triple).map_err(|e| format!("get native target: {e}"))?;
        let machine = target
            .create_target_machine(
                &triple,
                "generic",
                "",
                opt,
                RelocMode::Default,
                CodeModel::Default,
            )
            .ok_or_else(|| "failed to create native target machine".to_string())?;
        (triple, machine)
    };

    // Update the module's target triple so the emitted object is correct.
    module.set_triple(&triple);

    // Emit object file to a temporary path.
    let obj_path = format!("{output_path}.o");
    machine
        .write_to_file(module, FileType::Object, Path::new(&obj_path))
        .map_err(|e| format!("object emit: {e}"))?;

    // Build axon-rt static library so channel/spawn builtins are available.
    let rt_lib = build_axon_rt(release);

    // Determine linker: prefer the cross.toml override, else probe the host.
    let linker_override = target_triple.and_then(read_cross_linker);
    let linker = if let Some(ref l) = linker_override {
        std::path::PathBuf::from(l)
    } else {
        which::which("cc")
            .or_else(|_| which::which("clang"))
            .or_else(|_| which::which("gcc"))
            .map_err(|_| "no C compiler found (tried cc, clang, gcc)".to_string())?
    };

    let mut link_args: Vec<&str> = vec![&obj_path, "-o", output_path, "-lpthread"];
    if let Some(ref lib) = rt_lib {
        link_args.push(lib.as_str());
    }

    let status = Command::new(&linker)
        .args(&link_args)
        .status()
        .map_err(|e| format!("linker spawn: {e}"))?;

    let _ = std::fs::remove_file(&obj_path);

    if status.success() {
        Ok(())
    } else {
        Err(format!("linker ({}) exited with {}", linker.display(), status))
    }
}

/// Look up the configured linker for `target` in `~/.config/axon/cross.toml`.
///
/// Returns `None` if the file is absent or the target section has no `linker`
/// key — in which case the caller falls through to the host linker (which may
/// fail for truly cross-compiled targets, emitting E0905 guidance).
fn read_cross_linker(target: &str) -> Option<String> {
    let home = std::env::var_os("HOME")?;
    let config_path = std::path::PathBuf::from(home)
        .join(".config")
        .join("axon")
        .join("cross.toml");
    let content = std::fs::read_to_string(config_path).ok()?;

    // Minimal TOML section parser: find [target.<triple>] then scan key = "value" lines.
    let section_header = format!("[target.{target}]");
    let pos = content.find(&section_header)?;
    let after = &content[pos + section_header.len()..];

    for line in after.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            break; // reached the next section
        }
        if let Some(rest) = trimmed.strip_prefix("linker") {
            // Accept: linker = "value"  or  linker="value"
            let val = rest
                .trim_start_matches([' ', '\t', '='])
                .trim_matches('"');
            if !val.is_empty() {
                return Some(val.to_string());
            }
        }
    }
    None
}

impl<'ctx> Codegen<'ctx> {
    // ── Test runner ───────────────────────────────────────────────────────

    /// Run functions tagged `@[test]` via the LLVM JIT and report results.
    ///
    /// One JIT execution engine is created for the entire module and reused
    /// across all tests. Test functions have Axon signature `fn()` (void);
    /// they pass if they return normally. If `assert(false)` fires it calls
    /// `exit(1)`, terminating the process — Phase 1 limitation.
    pub fn run_tests(&self, fns: &[String]) -> Vec<TestResult> {

        // Verify the module before running tests.
        if let Err(e) = self.module.verify() {
            return fns.iter().map(|name| TestResult {
                name: name.clone(),
                passed: false,
                duration_ms: 0,
                error: Some(format!("IR verification failed: {}", e.to_string())),
            }).collect();
        }

        // Create a single JIT engine for the whole module.
        let ee = match self.module.create_jit_execution_engine(OptimizationLevel::None) {
            Ok(e) => e,
            Err(e) => {
                return fns.iter().map(|name| TestResult {
                    name: name.clone(),
                    passed: false,
                    duration_ms: 0,
                    error: Some(format!("JIT init: {e}")),
                }).collect();
            }
        };

        fns.iter()
            .map(|name| {
                let start = std::time::Instant::now();

                type VoidFn = unsafe extern "C" fn();
                let result: Result<(), String> = unsafe {
                    ee.get_function::<VoidFn>(name)
                        .map_err(|e| format!("JIT lookup '{name}': {e}"))
                        .map(|f| f.call())
                };

                let duration_ms = start.elapsed().as_millis() as u64;
                match result {
                    Ok(()) => TestResult { name: name.clone(), passed: true, duration_ms, error: None },
                    Err(e) => TestResult { name: name.clone(), passed: false, duration_ms, error: Some(e) },
                }
            })
            .collect()
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    /// Convert an `ast::AxonType` to the semantic `Type` enum.
    fn axon_type_to_semantic(&self, ty: &ast::AxonType) -> Type {
        match ty {
            ast::AxonType::Named(name) => match name.as_str() {
                "i8" => Type::I8,
                "i16" => Type::I16,
                "i32" => Type::I32,
                "i64" => Type::I64,
                "u8" => Type::U8,
                "u16" => Type::U16,
                "u32" => Type::U32,
                "u64" => Type::U64,
                "f32" => Type::F32,
                "f64" => Type::F64,
                "bool" => Type::Bool,
                "str" | "String" => Type::Str,
                "()" | "unit" | "Unit" => Type::Unit,
                other => {
                    // If this name is a known enum, use Type::Enum so llvm_type
                    // can look up the "{name}_enum" struct in the module.
                    if self.enum_variants.contains_key(other) {
                        Type::Enum(other.to_string())
                    } else {
                        Type::Struct(other.to_string())
                    }
                }
            },
            ast::AxonType::Result { ok, err } => Type::Result(
                Box::new(self.axon_type_to_semantic(ok)),
                Box::new(self.axon_type_to_semantic(err)),
            ),
            ast::AxonType::Option(inner) => {
                Type::Option(Box::new(self.axon_type_to_semantic(inner)))
            }
            ast::AxonType::Slice(inner) => {
                Type::Slice(Box::new(self.axon_type_to_semantic(inner)))
            }
            ast::AxonType::Chan(inner) => {
                Type::Chan(Box::new(self.axon_type_to_semantic(inner)))
            }
            ast::AxonType::Generic { base, args } => {
                // Generic types not yet resolved — use Deferred.
                let _ = args;
                Type::Deferred(base.clone())
            }
            ast::AxonType::Fn { params, ret } => Type::Fn(
                params.iter().map(|p| self.axon_type_to_semantic(p)).collect(),
                Box::new(self.axon_type_to_semantic(ret)),
            ),
            ast::AxonType::Ref(inner) => self.axon_type_to_semantic(inner),
            ast::AxonType::TypeParam(name) => Type::TypeParam(name.clone()),
            ast::AxonType::DynTrait(name) => Type::DynTrait(name.clone()),
            ast::AxonType::Tuple(elems) => Type::Tuple(
                elems.iter().map(|e| self.axon_type_to_semantic(e)).collect(),
            ),
            // Union types are not yet first-class — fall back to Unknown so
            // codegen does not assert a specific LLVM lowering.
            ast::AxonType::Union(_) => Type::Unknown,
        }
    }

    /// Convert an `ast::AxonType` directly to an LLVM type.
    fn llvm_type_from_axon(&self, ty: &ast::AxonType) -> Option<BasicTypeEnum<'ctx>> {
        let sem = self.axon_type_to_semantic(ty);
        self.llvm_type(&sem)
    }

    /// Heuristic: infer the semantic `Type` from an LLVM `BasicValueEnum`.
    fn value_type_hint(&self, val: &BasicValueEnum<'ctx>) -> Type {
        match val {
            BasicValueEnum::IntValue(i) => match i.get_type().get_bit_width() {
                1 => Type::Bool,
                8 => Type::I8,
                16 => Type::I16,
                32 => Type::I32,
                _ => Type::I64,
            },
            BasicValueEnum::FloatValue(f) => {
                if f.get_type() == self.context.f32_type() {
                    Type::F32
                } else {
                    Type::F64
                }
            }
            BasicValueEnum::StructValue(_) => Type::Unknown,
            BasicValueEnum::PointerValue(_) => Type::Unknown,
            BasicValueEnum::ArrayValue(_) => Type::Unknown,
            BasicValueEnum::VectorValue(_) => Type::Unknown,
        }
    }

    /// Heuristic: infer the Axon semantic type of an expression without emitting IR.
    /// Used to populate `local_types` and select Result union layouts.
    fn infer_expr_sem_type(&self, expr: &ast::Expr) -> Option<Type> {
        match expr {
            ast::Expr::Literal(lit) => match lit {
                ast::Literal::Int(_) => Some(Type::I64),
                ast::Literal::Float(_) => Some(Type::F64),
                ast::Literal::Bool(_) => Some(Type::Bool),
                ast::Literal::Str(_) => Some(Type::Str),
            },
            ast::Expr::Ident(name) => self.local_types.get(name).cloned(),
            ast::Expr::Call { callee, .. } => {
                if let ast::Expr::Ident(name) = callee.as_ref() {
                    self.fn_return_types.get(name).cloned()
                } else {
                    None
                }
            }
            ast::Expr::Ok(_) | ast::Expr::Err(_) => {
                self.current_result_types.as_ref().map(|(ok, err)| {
                    Type::Result(Box::new(ok.clone()), Box::new(err.clone()))
                })
            }
            ast::Expr::StructLit { name, .. } => {
                if name.contains("::") {
                    // "EnumName::Variant" → Type::Enum("EnumName")
                    let enum_name = name.splitn(2, "::").next().unwrap_or(name).to_string();
                    Some(Type::Enum(enum_name))
                } else {
                    Some(Type::Struct(name.clone()))
                }
            }
            ast::Expr::Array(elems) => {
                let inner = elems
                    .first()
                    .and_then(|e| self.infer_expr_sem_type(e))
                    .unwrap_or(Type::Unknown);
                Some(Type::Slice(Box::new(inner)))
            }
            ast::Expr::Block(stmts) => {
                stmts.last().and_then(|s| self.infer_expr_sem_type(&s.expr))
            }
            ast::Expr::If { then, .. } => self.infer_expr_sem_type(then),
            ast::Expr::FmtStr { .. } => Some(Type::Str),
            _ => None,
        }
    }

    /// Infer the semantic type of an expression, including chained FieldAccess.
    /// Used by FieldAccess codegen to find the struct name of a receiver.
    fn sem_type_of_expr(&self, expr: &ast::Expr) -> Option<Type> {
        match expr {
            ast::Expr::Ident(name) => self.local_types.get(name).cloned(),
            ast::Expr::FieldAccess { receiver, field } => {
                // Recursively get the type of the receiver struct, then look up
                // the field's type within that struct.
                let recv_ty = self.sem_type_of_expr(receiver)?;
                let sname = if let Type::Struct(sn) = recv_ty { sn } else { return None; };
                let field_names = self.struct_fields.get(&sname)?;
                let idx = field_names.iter().position(|n| n == field)?;
                let struct_ty = self.module.get_struct_type(&sname)?;
                let field_llvm_ty = struct_ty.get_field_type_at_index(idx as u32)?;
                // Convert the LLVM field type back to a semantic type via local_types heuristics.
                // For struct fields, we need to look up the LLVM type name.
                match field_llvm_ty {
                    BasicTypeEnum::IntType(it) => Some(match it.get_bit_width() {
                        1 => Type::Bool, 8 => Type::I8, 16 => Type::I16, 32 => Type::I32, _ => Type::I64,
                    }),
                    BasicTypeEnum::FloatType(ft) => Some(if ft == self.context.f32_type() { Type::F32 } else { Type::F64 }),
                    BasicTypeEnum::StructType(st) => {
                        // Try to find the struct name in the module.
                        st.get_name().and_then(|n| n.to_str().ok()).map(|n| {
                            if n.ends_with("_enum") {
                                Type::Enum(n.trim_end_matches("_enum").to_string())
                            } else {
                                Type::Struct(n.to_string())
                            }
                        })
                    }
                    _ => None,
                }
            }
            _ => self.infer_expr_sem_type(expr),
        }
    }
}

// ── axon-rt build helper ──────────────────────────────────────────────────────

/// Build `axon-rt` as a static library and return the path to `libaxon_rt.a`.
///
/// Silently returns `None` if cargo is not found or the build fails, so that
/// the linker step still attempts to proceed (channel/spawn functions will
/// simply be missing symbols if the rt wasn't linked).
fn build_axon_rt(release: bool) -> Option<String> {
    let cargo = std::env::var("CARGO").ok().unwrap_or_else(|| "cargo".into());
    let profile = if release { "release" } else { "debug" };

    // Locate the workspace root relative to this binary.
    let manifest = std::env::var("CARGO_MANIFEST_DIR")
        .map(|d| format!("{d}/../../../Cargo.toml"))
        .unwrap_or_else(|_| "Cargo.toml".into());

    let status = Command::new(&cargo)
        .args(["build", "-p", "axon-rt", "--manifest-path", &manifest])
        .args(if release { &["--release"][..] } else { &[][..] })
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    if !status.success() {
        return None;
    }

    // Resolve the target directory from CARGO_TARGET_DIR or adjacent to manifest.
    let target_dir = std::env::var("CARGO_TARGET_DIR")
        .unwrap_or_else(|_| {
            // Walk up from CARGO_MANIFEST_DIR to find <workspace>/target.
            std::env::var("CARGO_MANIFEST_DIR")
                .map(|d| format!("{d}/../../../target"))
                .unwrap_or_else(|_| "target".into())
        });

    let lib_path = format!("{target_dir}/{profile}/libaxon_rt.a");
    if std::path::Path::new(&lib_path).exists() {
        Some(lib_path)
    } else {
        None
    }
}

// ── Test result ───────────────────────────────────────────────────────────────

#[derive(Debug)]
pub struct TestResult {
    pub name: String,
    pub passed: bool,
    pub duration_ms: u64,
    pub error: Option<String>,
}
