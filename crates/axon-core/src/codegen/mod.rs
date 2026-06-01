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

// ── Module split (ROADMAP §7.5) ───────────────────────────────────────────────
// Phase 1:    free-function helpers (object emission + linking + staticlib
//             builds)                                  → `link.rs`
// Phase 2.1:  Axon→LLVM type lowering
//             (llvm_type, llvm_sizeof, llvm_align_of)  → `types.rs`
// Phase 2.2:  ASI helpers (provenance log emission, @[verify] runtime gate,
//             @[adaptive] registry init)               → `asi.rs`
//
// Remaining Codegen<'ctx> methods (Pass 1 declarations including
// `declare_builtins` ~3870 lines, expression emission `emit_expr` ~1380 lines,
// statement emission, match/pattern emission, link orchestration) stay in
// this file pending Phase 2.3+ which requires faster-machine validation
// because the bigger remaining splits will involve cross-cutting field-access
// pub(super) decisions.
pub mod link;
pub mod types;
pub mod asi;
pub mod option_result;
pub mod output;
pub mod match_pat;
pub mod builtins;
pub mod build_wrappers;
pub mod expr;
pub mod ir;
pub mod ir_inkwell;

// Re-export TestResult so backward-compatible path
// `axon_core::codegen::TestResult` keeps working.
pub use output::TestResult;

// Re-export the public path that lib.rs / main.rs expect: callers reach
// `compile_bitcode_to_binary` via `axon_core::codegen::compile_bitcode_to_binary`.
pub use link::compile_bitcode_to_binary;

/// Phase 4 `@[adaptive]`: returns true if the attribute list contains an
/// `adaptive` annotation (regardless of its argument list).  Used by
/// `emit_fn` to decide whether to inject `__axon_provenance_log` calls.
fn has_adaptive_attr(attrs: &[ast::Attr]) -> bool {
    attrs.iter().any(|a| a.name == "adaptive")
}

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
    /// Maps local variable names to their alloca pointers and LLVM types.
    locals: HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    /// Maps fn names to the LLVM function value.
    pub(super) functions: HashMap<String, FunctionValue<'ctx>>,
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
    /// Phase 4 `@[adaptive]`: when emitting a function carrying that attribute,
    /// this holds the function name so `log_return_if_adaptive` can log
    /// a "return" event before each early/tail return.
    pub(super) current_adaptive_fn: Option<String>,
    /// ASI Layer-3: names of `@[adaptive] fn(i64) -> i64` functions that
    /// should be registered with the runtime adaptive registry at module
    /// startup.  Populated lazily when each FnDef is declared; consumed in
    /// `emit_fn` for `main` to emit one `__axon_register_adaptive` call per
    /// entry.  v1 narrowing: only `(i64) -> i64` is eligible; other
    /// signatures (multi-arg, f64 input, str input, …) silently fall
    /// through and rely on the Layer-2 retrospective `goal_run` path.
    pub(super) adaptive_registry_targets: Vec<String>,
    /// ASI Layer-3 `@[verify]` runtime: when emitting a function that carries
    /// `@[verify(confidence OP K)]` *and* whose return type is `Uncertain<T>`,
    /// this holds `(fn_name, op_str, bound)` so each return site can inject
    /// a runtime confidence check via `__axon_verify_panic`.  `op_str` is the
    /// source-level operator (`">="`, `">"`, …) and `bound` is the literal K.
    /// `None` whenever the surrounding function has no decodable verify spec.
    pub(super) current_verify_fn: Option<(String, &'static str, f64)>,
    /// IR.3 prep: a parallel inkwell-backed IR backend that codegen
    /// modules can call via `self.ir.*` instead of `self.ir.builder.*` /
    /// `self.ir.context.*` / `self.ir.module.*`.  During the migration phase
    /// (IR.3) both this field AND the legacy `context/module/builder`
    /// fields are populated; modules migrate one at a time per
    /// `MIGRATION.md`.  IR.4 will remove the legacy fields once every
    /// caller has migrated.  Empty until first use; backend wraps its
    /// own `Module<'ctx>` independent of `self.ir.module`.
    pub(super) ir: ir_inkwell::InkwellBackend<'ctx>,
}

impl<'ctx> Codegen<'ctx> {
    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        // IR_REARCH.md option (c): InkwellBackend owns the only module +
        // builder.  Codegen accesses them through `self.ir.{module,
        // builder, context}`.  Single symbol table → IR.3 partial
        // migrations are end-to-end linkable.
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let ir = ir_inkwell::InkwellBackend::adopt(context, module, builder);
        Self {
            ir,
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
            current_adaptive_fn: None,
            adaptive_registry_targets: Vec::new(),
            current_verify_fn: None,
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
        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());

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
                        let fv = self.ir.module.add_function(&thunk_name, fn_ty, None);
                        (fv, fn_ty)
                    }
                    None => {
                        let fn_ty = self.ir.context.void_type().fn_type(&param_tys, false);
                        let fv = self.ir.module.add_function(&thunk_name, fn_ty, None);
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
                let named_struct = self.ir.context.opaque_struct_type(&td.name);
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
                let i32_ty = self.ir.context.i32_type();
                let i8_ty = self.ir.context.i8_type();

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
                let named_struct = self.ir.context.opaque_struct_type(&struct_name);
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
            let fn_ty = self.ir.context.i32_type().fn_type(&param_tys, false);
            self.ir.module.add_function("main", fn_ty, None)
        } else {
            match self.llvm_type(&ret_sem) {
                Some(ret_ty) => {
                    let fn_ty = ret_ty.fn_type(&param_tys, /*variadic=*/ false);
                    self.ir.module.add_function(name, fn_ty, None)
                }
                None => {
                    let fn_ty = self.ir.context.void_type().fn_type(&param_tys, false);
                    self.ir.module.add_function(name, fn_ty, None)
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

        // ── ASI Layer-3: collect eligible adaptive fns for registry init. ─────
        // v1 narrowing: only `@[adaptive] fn(i64) -> i64` is eligible for
        // live hill-climb.  Anything else (multi-arg, f64 input, str input,
        // non-i64 return) is silently skipped here; goal_run will fall back
        // to the Layer-2 retrospective best-observed path for those.
        self.adaptive_registry_targets.clear();
        for (mangled, f) in &fn_work {
            if !has_adaptive_attr(&f.attrs) { continue; }
            if f.params.len() != 1 { continue; }
            let p_sem = self.axon_type_to_semantic(&f.params[0].ty);
            if !matches!(p_sem, Type::I64) { continue; }
            let r_sem = f
                .return_type
                .as_ref()
                .map(|t| self.axon_type_to_semantic(t))
                .unwrap_or(Type::Unit);
            if !matches!(r_sem, Type::I64) { continue; }
            // Use the (mangled) name so we register the actual LLVM symbol.
            // For top-level fns this equals f.name; for impl methods it's
            // `Type__method`.  v1 doesn't expect impl methods to carry
            // @[adaptive] but if they do the registration is still valid.
            self.adaptive_registry_targets.push(mangled.clone());
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

                let saved = self.ir.builder.get_insert_block();
                let entry = self.ir.context.append_basic_block(thunk_fn, "entry");
                self.ir.builder.position_at_end(entry);

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
                        let self_val = build_wrappers::w_load(&self.ir.builder, ty, self_ptr, "self_val");
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

                let call = build_wrappers::w_call(&self.ir.builder, concrete_fn, &call_args, "thunk_ret");
                let ret_sem = tm.return_type.as_ref()
                    .map(|t| self.axon_type_to_semantic(t))
                    .unwrap_or(crate::types::Type::Unit);

                if matches!(ret_sem, crate::types::Type::Unit) {
                    build_wrappers::w_ret_void(&self.ir.builder);
                } else if let Some(ret_val) = call.try_as_basic_value().left() {
                    build_wrappers::w_ret(&self.ir.builder, ret_val);
                } else {
                    build_wrappers::w_ret_void(&self.ir.builder);
                }

                if let Some(b) = saved { self.ir.builder.position_at_end(b); }
            }
        }
    }

    /// Emit one `@vtable_Trait_Type = constant [N x ptr] [...]` global per impl block.
    fn emit_vtable_globals(&mut self, program: &ast::Program) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());

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
            let global = self.ir.module.add_global(arr_ty, None, &global_name);
            global.set_initializer(&arr_const);
            global.set_constant(true);

            self.vtable_globals.insert((trait_name, type_name), global);
        }
    }

    // ── Function bodies ───────────────────────────────────────────────────────

    fn emit_fn(&mut self, f: &ast::FnDef, llvm_fn: FunctionValue<'ctx>) {
        let entry = self.ir.context.append_basic_block(llvm_fn, "entry");
        self.ir.builder.position_at_end(entry);

        // Save outer locals/types; reset for this function scope.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_result_types = self.current_result_types.take();
        let saved_adaptive = self.current_adaptive_fn.take();
        let saved_verify = self.current_verify_fn.take();

        // ── @[adaptive]: emit a "call" event at the prologue. ─────────────────
        // Activates `current_adaptive_fn` so any subsequent build_return inside
        // this function gets a "return" event injected before it.
        if has_adaptive_attr(&f.attrs) {
            self.current_adaptive_fn = Some(f.name.clone());
            self.emit_provenance_log(&f.name, "call");
        }

        // ── ASI Layer-3 @[verify]: arm runtime predicate enforcement. ─────────
        // Activates `current_verify_fn` so every return site of this function
        // emits a guarded call to `__axon_verify_panic` when the runtime
        // confidence violates the predicate.  We only arm the check when:
        //   1. The function has a `VerifySpec`.
        //   2. The predicate decodes as `confidence OP literal_f64`.
        //   3. The declared return type is `Uncertain<T>` (defensive — the
        //      static checker should have rejected the verify clause otherwise).
        // If any of those fail, we silently emit nothing — same shape as the
        // static checker, which also no-ops on undecodable predicates.
        if let Some(spec) = &f.verify {
            if let Some((op, bound)) = crate::verify::decode_verify_predicate(&spec.predicate) {
                let ret_is_uncertain = f
                    .return_type
                    .as_ref()
                    .map(|t| matches!(self.axon_type_to_semantic(t), Type::Uncertain(_)))
                    .unwrap_or(false);
                if ret_is_uncertain {
                    let op_str = crate::verify::binop_to_verify_str(&op);
                    self.current_verify_fn = Some((f.name.clone(), op_str, bound));
                }
            }
        }

        // ── ASI Layer-3: in `main`'s prologue, register each eligible
        // `@[adaptive] fn(i64) -> i64` with the runtime adaptive registry.
        // The runtime then knows how to call them back during goal_run
        // hill-climb.  No-op when the target list is empty, so non-AI
        // programs pay nothing.
        if f.name == "main" {
            self.emit_adaptive_registry_init();
        }

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
                let alloca = build_wrappers::w_alloca(&self.ir.builder, llvm_ty, &param.name);
                if let Some(arg) = llvm_fn.get_nth_param(i as u32) {
                    build_wrappers::w_store(&self.ir.builder, alloca, arg);
                }
                self.locals.insert(param.name.clone(), (alloca, llvm_ty));
                self.local_types.insert(param.name.clone(), sem_ty);
            }
        }

        let body_val = self.emit_expr(&f.body, llvm_fn);

        // Emit return if the builder is still on a live block.
        if self.ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            if f.name == "main" && matches!(ret_sem, Type::Unit) {
                let zero = self.ir.context.i32_type().const_int(0, false);
                // main() returning 0 isn't an interesting score; use the legacy event log.
                self.log_return_if_adaptive();
                build_wrappers::w_ret(&self.ir.builder, zero.into());
            } else {
                match body_val {
                    Some(v) if !matches!(ret_sem, Type::Unit) => {
                        self.log_return_if_adaptive_val(v);
                        self.emit_verify_check_if_needed(v, llvm_fn);
                        build_wrappers::w_ret(&self.ir.builder, v);
                    }
                    None if !matches!(ret_sem, Type::Unit) => {
                        // No value from body but function has non-void return type:
                        // emit a zero value of the appropriate type to keep IR valid.
                        if let Some(ret_llvm_ty) = self.llvm_type(&ret_sem) {
                            let zero_val = ret_llvm_ty.const_zero();
                            self.log_return_if_adaptive_val(zero_val);
                            self.emit_verify_check_if_needed(zero_val, llvm_fn);
                            build_wrappers::w_ret(&self.ir.builder, zero_val.into());
                        } else {
                            self.log_return_if_adaptive();
                            build_wrappers::w_ret_void(&self.ir.builder);
                        }
                    }
                    _ => {
                        self.log_return_if_adaptive();
                        build_wrappers::w_ret_void(&self.ir.builder);
                    }
                }
            }
        }

        // Restore outer scope.
        self.locals = saved_locals;
        self.local_types = saved_local_types;
        self.current_result_types = saved_result_types;
        self.current_adaptive_fn = saved_adaptive;
        self.current_verify_fn = saved_verify;
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
                // Layer-1 ASI types are first-class generics in the type system.
                if base == "Uncertain" {
                    let inner = args
                        .first()
                        .map(|a| self.axon_type_to_semantic(a))
                        .unwrap_or(Type::I64);
                    return Type::Uncertain(Box::new(inner));
                }
                if base == "Temporal" {
                    let inner = args
                        .first()
                        .map(|a| self.axon_type_to_semantic(a))
                        .unwrap_or(Type::I64);
                    return Type::Temporal(Box::new(inner));
                }
                // Other generic types not yet resolved — use Deferred.
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
                if f.get_type() == self.ir.context.f32_type() {
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
            ast::Expr::Tuple(elems) => {
                let tys = elems
                    .iter()
                    .map(|e| self.infer_expr_sem_type(e).unwrap_or(Type::Unknown))
                    .collect();
                Some(Type::Tuple(tys))
            }
            ast::Expr::Block(stmts) => {
                stmts.last().and_then(|s| self.infer_expr_sem_type(&s.expr))
            }
            ast::Expr::If { then, .. } => self.infer_expr_sem_type(then),
            ast::Expr::FmtStr { .. } => Some(Type::Str),
            // Layer-2 ASI: BinOp with an Uncertain<T> operand produces Uncertain<T>;
            // comparisons over Uncertain produce Uncertain<bool>. This enables
            // chained `let x = a + b; let y = x * c` to track Uncertain typing in
            // local_types, which `emit_binop_uncertain` relies on for layout.
            ast::Expr::BinOp { op, left, right } => {
                let lt = self.infer_expr_sem_type(left);
                let rt = self.infer_expr_sem_type(right);
                let is_unc = |t: &Option<Type>| matches!(t, Some(Type::Uncertain(_)));
                let unc_inner = |t: &Option<Type>| -> Option<Type> {
                    match t {
                        Some(Type::Uncertain(inner)) => Some(*inner.clone()),
                        _ => None,
                    }
                };
                match op {
                    ast::BinOp::Add | ast::BinOp::Sub | ast::BinOp::Mul
                    | ast::BinOp::Div | ast::BinOp::Rem => {
                        if is_unc(&lt) || is_unc(&rt) {
                            let inner = unc_inner(&lt).or_else(|| unc_inner(&rt))
                                .unwrap_or(Type::I64);
                            Some(Type::Uncertain(Box::new(inner)))
                        } else {
                            lt.or(rt)
                        }
                    }
                    ast::BinOp::Eq | ast::BinOp::NotEq
                    | ast::BinOp::Lt | ast::BinOp::Gt
                    | ast::BinOp::LtEq | ast::BinOp::GtEq => {
                        if is_unc(&lt) || is_unc(&rt) {
                            Some(Type::Uncertain(Box::new(Type::Bool)))
                        } else {
                            Some(Type::Bool)
                        }
                    }
                    ast::BinOp::And | ast::BinOp::Or => {
                        if is_unc(&lt) || is_unc(&rt) {
                            Some(Type::Uncertain(Box::new(Type::Bool)))
                        } else {
                            Some(Type::Bool)
                        }
                    }
                    _ => Some(Type::I64),
                }
            }
            // Field access on Uncertain<T> / Temporal<T>: `.value` → T, `.confidence` → f64.
            ast::Expr::FieldAccess { receiver, field } => {
                let recv_ty = self.infer_expr_sem_type(receiver)
                    .or_else(|| self.sem_type_of_expr(receiver));
                match (recv_ty, field.as_str()) {
                    (Some(Type::Uncertain(inner)), "value") => Some(*inner),
                    (Some(Type::Temporal(inner)), "value") => Some(*inner),
                    (Some(Type::Uncertain(_)), "confidence")
                    | (Some(Type::Temporal(_)), "confidence")
                    | (Some(Type::Temporal(_)), "decay") => Some(Type::F64),
                    (Some(Type::Uncertain(_)), "source_tag")
                    | (Some(Type::Temporal(_)), "horizon_ms")
                    | (Some(Type::Temporal(_)), "valid_until_ms") => Some(Type::I64),
                    _ => None,
                }
            }
            ast::Expr::Index { receiver, .. } => {
                // `arr[i]` → element type of the slice's inner type
                self.infer_expr_sem_type(receiver).and_then(|ty| {
                    if let Type::Slice(inner) = ty { Some(*inner.clone()) } else { None }
                })
            }
            _ => None,
        }
    }

    /// Infer the semantic type of an expression, including chained FieldAccess.
    /// Used by FieldAccess codegen to find the struct name of a receiver.
    fn sem_type_of_expr(&self, expr: &ast::Expr) -> Option<Type> {
        match expr {
            ast::Expr::Ident(name) => self.local_types.get(name).cloned(),
            ast::Expr::FieldAccess { receiver, field } => {
                let recv_ty = self.sem_type_of_expr(receiver)?;
                // Handle tuple field access by numeric index.
                if let Some(elts) = match &recv_ty {
                    Type::Tuple(elts) => Some(elts),
                    _ => None,
                } {
                    if let Ok(idx) = field.parse::<u32>() {
                        return elts.get(idx as usize).cloned();
                    }
                }
                let sname = if let Type::Struct(sn) = &recv_ty { sn } else { return None; };
                let field_names = self.struct_fields.get(sname.as_str())?;
                let idx = field_names.iter().position(|n| n == field)?;
                let struct_ty = self.ir.module.get_struct_type(&sname)?;
                let field_llvm_ty = struct_ty.get_field_type_at_index(idx as u32)?;
                match field_llvm_ty {
                    BasicTypeEnum::IntType(it) => Some(match it.get_bit_width() {
                        1 => Type::Bool, 8 => Type::I8, 16 => Type::I16, 32 => Type::I32, _ => Type::I64,
                    }),
                    BasicTypeEnum::FloatType(ft) => Some(if ft == self.ir.context.f32_type() { Type::F32 } else { Type::F64 }),
                    BasicTypeEnum::StructType(st) => {
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

// (TestResult moved to codegen/output.rs in Phase 2.6 — re-exported above
// via `pub use output::TestResult` for backwards-compatible path access.)
