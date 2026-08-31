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

use inkwell::context::Context;
use inkwell::types::{BasicMetadataTypeEnum, BasicType, BasicTypeEnum, StructType};
use inkwell::values::{BasicMetadataValueEnum, BasicValueEnum, FunctionValue, PointerValue};
use inkwell::AddressSpace;

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
pub mod asi;
pub mod bpf;
pub mod build_wrappers;
pub mod builtin_externs;
pub mod builtins;
pub mod expr;
pub mod ir_inkwell;
pub mod link;
pub mod match_pat;
pub mod option_result;
pub mod output;
pub mod types;

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

/// BUG_HUNT #19: does this top-level item contain any `goal_run(…)` call?
/// Used to gate the goal-name registry emission — non-goal programs pay nothing.
fn program_calls_goal_run(item: &ast::Item) -> bool {
    fn body_of(item: &ast::Item) -> Vec<&ast::Expr> {
        match item {
            ast::Item::FnDef(f) => vec![&f.body],
            ast::Item::ImplBlock(b) => b.methods.iter().map(|m| &m.body).collect(),
            _ => Vec::new(),
        }
    }
    body_of(item).into_iter().any(expr_calls_goal_run)
}

/// Recursively: does `e` (or any sub-expression) call the builtin named `target`?
fn expr_calls(e: &ast::Expr, target: &str) -> bool {
    use ast::Expr;
    let mut found = false;
    ast::walk_expr(e, &mut |x| {
        if let Expr::Call { callee, .. } = x {
            if let Expr::Ident(name) = callee.as_ref() {
                if name == target {
                    found = true;
                }
            }
        }
    });
    found
}

/// Recursively: does `e` (or any sub-expression) call `goal_run`?
fn expr_calls_goal_run(e: &ast::Expr) -> bool {
    expr_calls(e, "goal_run")
}

/// R21 — does `f` touch the `Decimal` fixed-point type anywhere (signature,
/// body literal, or `decimal_*` builtin)? Used by `emit_fn` to E0910-refuse
/// the function (Decimal is interp-only in this slice). This is a deliberately
/// CONSERVATIVE over-approximation: it scans the function's debug rendering for
/// the `Decimal` AST tags / `decimal_` builtin prefix, so it can only ever
/// OVER-refuse (refuse a fn that doesn't really need Decimal) — never UNDER-
/// refuse and let subtly-wrong money IR through (sound-by-refusal, I-2). A real
/// per-op walker would risk missing an Expr variant; the string scan cannot.
fn fn_uses_decimal(f: &ast::FnDef) -> bool {
    // Signature: a `Decimal` param or return type.
    let sig_decimal = f.return_type.as_ref().map(type_mentions_decimal).unwrap_or(false)
        || f.params.iter().any(|p| type_mentions_decimal(&p.ty));
    if sig_decimal {
        return true;
    }
    // Body: any `Literal::Decimal(...)` node or `decimal_*` builtin call. The
    // debug format embeds `Decimal(` for the literal and the call ident for the
    // builtin; either signals Decimal usage.
    let dbg = format!("{:?}", f.body);
    dbg.contains("Decimal(") || dbg.contains("decimal_")
}

/// True if an `AxonType` (including nested positions) names `Decimal`.
fn type_mentions_decimal(ty: &AxonType) -> bool {
    format!("{ty:?}").contains("Decimal")
}

// ── Public surface ────────────────────────────────────────────────────────────

pub struct Codegen<'ctx> {
    /// Maps local variable names to their alloca pointers and LLVM types.
    locals: HashMap<String, (PointerValue<'ctx>, BasicTypeEnum<'ctx>)>,
    /// Maps fn names to the LLVM function value.
    pub(super) functions: HashMap<String, FunctionValue<'ctx>>,
    /// Maps struct names to their ordered field names (for FieldAccess GEP).
    struct_fields: HashMap<String, Vec<String>>,
    /// Maps struct name → declared semantic field types, in declaration order.
    /// Used so a sum-type field initializer (`Box { r: Err("x") }`) builds the
    /// field's full canonical layout, not a value-only one.
    struct_field_sem_types: HashMap<String, Vec<Type>>,
    /// Phase 5: named refinement types → their (erased) base AxonType. A
    /// refinement is transparent at the value/layout level, so codegen lowers
    /// `Positive` (and a synthetic inline `__refine_N`) to its base `i64`. Without
    /// this a refinement-typed param resolves to an unknown Struct and the param
    /// isn't lowered (E0701). The codegen analog of the infer/checker
    /// refinement_base maps — the third engine that must learn the type form.
    refinement_base: HashMap<String, ast::AxonType>,
    /// Phase 5: named refinement → its predicate Expr (binder `_`). Drives the
    /// runtime precondition check emitted at function entry — when a parameter's
    /// type is a refinement, the predicate is lowered (with `_` aliased to the
    /// param) and a violation calls `__axon_refine_panic` (exit 6), the codegen
    /// analog of the interpreter's `Interp::refine_preds` entry check (I-2).
    refine_preds: HashMap<String, ast::Expr>,
    /// Phase 5 §4: obligations an SMT prover discharged for ALL inputs, so the
    /// matching runtime check is provably dead and is NOT armed at all. Empty by
    /// default (and unless `set_discharged` is called by an `smt`-feature
    /// pipeline), so the default native build emits every check as before.
    discharged: crate::verify::Discharged,
    /// Phase 5: per-struct, the list of `(field_name, refinement_name)` for fields
    /// whose declared type is a refinement. Drives the runtime field-precondition
    /// check emitted at struct construction (`emit_refine_struct_checks`).
    struct_field_refines: HashMap<String, Vec<(String, String)>>,
    /// Phase 5: per-struct, the WHOLE-STRUCT refinement predicate (`type Range =
    /// {…} where _.lo <= _.hi`), binder `_` = the instance. Evaluated against the
    /// just-constructed struct at construction time.
    struct_whole_refines: HashMap<String, ast::Expr>,
    /// Maps fn names to their Axon semantic return type (for call-site type inference).
    fn_return_types: HashMap<String, Type>,
    /// Tracks inferred Axon semantic types for named locals (for match/field-access dispatch).
    local_types: HashMap<String, Type>,
    /// AUDIT T37 (finding F061): the DECLARED LLVM parameter types and semantic
    /// return type of each `let f = |…| …` closure binding, keyed by binding name.
    ///
    /// A closure value is a bare `{fn_ptr, env_ptr}` pair carrying no type tag, so
    /// the direct-call site used to build its indirect-call signature from the
    /// ARGUMENT's LLVM type and read the result back as a raw i64. Both are wrong
    /// whenever the lambda's declared types are narrower or non-integer, and the
    /// resulting mismatch is UB — the observed value depended on the order the
    /// lambdas happened to be emitted in.
    closure_sigs: HashMap<String, (Vec<Option<Type>>, Option<Type>)>,
    /// Set when inside a function returning `Result<T,E>`; drives canonical union layout.
    current_result_types: Option<(Type, Type)>,
    /// Set when emitting a value whose target type is `Option<T>`; lets a bare
    /// `None` build the correct `{ i1, T }` layout (otherwise it defaults to
    /// `{i1,i64}` and a later `Some(str)` / match mis-sizes the payload).
    current_option_inner: Option<Type>,
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
    loop_stack: Vec<(
        inkwell::basic_block::BasicBlock<'ctx>,
        inkwell::basic_block::BasicBlock<'ctx>,
    )>,
    /// Current lambda's closure environment, set when emitting a lambda body.
    /// Tuple of (env_ptr, env_struct_ty, capture_index_map).
    /// When set, `Ident` lookups that miss `self.locals` fall back to loading
    /// the captured value from the env struct via GEP. This is a defensive
    /// safety net — the primary capture path binds field pointers directly
    /// into `self.locals` (see `Expr::Lambda` handler), so this fallback only
    /// fires for variables the resolver missed (e.g. names introduced by AST
    /// rewrites after `fill_captures` ran).
    current_lambda_env: Option<(PointerValue<'ctx>, StructType<'ctx>, HashMap<String, u32>)>,
    /// Expected LLVM parameter types for the NEXT lambda emitted, set by a
    /// builtin lowering (e.g. dict_filter's `fn(str, i64)` predicate) just before
    /// emitting an inline `|k, v|` whose params carry no annotation. `emit_lambda`
    /// consumes (takes) this so it types the params correctly; cleared after.
    /// `None` (the default) leaves the annotation-or-i64 behavior unchanged.
    pending_lambda_param_tys: Option<Vec<BasicTypeEnum<'ctx>>>,
    /// Phase 4 `@[adaptive]`: when emitting a function carrying that attribute,
    /// this holds the function name so `log_return_if_adaptive` can log
    /// a "return" event before each early/tail return.
    pub(super) current_adaptive_fn: Option<String>,
    /// F11: the leading `i64` parameter of the current `@[adaptive]` fn, if it
    /// has one. Captured at the prologue and passed to the runtime's
    /// `__axon_provenance_log_ret_i64_in` at each return site, so `goal_run` can
    /// warm-start its hill-climb from the best prior input. `None` when the fn
    /// has no leading i64 param (then the score-only log path is used).
    pub(super) current_adaptive_input: Option<inkwell::values::IntValue<'ctx>>,
    /// ASI Layer-3: names of `@[adaptive] fn(i64) -> i64` functions that
    /// should be registered with the runtime adaptive registry at module
    /// startup.  Populated lazily when each FnDef is declared; consumed in
    /// `emit_fn` for `main` to emit one `__axon_register_adaptive` call per
    /// entry.  v1 narrowing: only `(i64) -> i64` is eligible; other
    /// signatures (multi-arg, f64 input, str input, …) silently fall
    /// through and rely on the Layer-2 retrospective `goal_run` path.
    pub(super) adaptive_registry_targets: Vec<String>,
    /// BUG_HUNT #19: every top-level fn name in the program, registered with the
    /// runtime in `main`'s prologue (one `__axon_register_goal_name` call each)
    /// so native `goal_run` can reject a typo'd metric name with the same panic
    /// the interpreter raises (I-9 parity). Populated only when the program
    /// actually calls `goal_run` (empty ⇒ no calls emitted, zero cost for
    /// non-goal programs).
    pub(super) goal_name_targets: Vec<String>,
    /// R4: the program's source path, stamped into native `@[adaptive]`
    /// provenance as the `"src"` field (parity with the interpreter, which sets
    /// it via `set_provenance_source`). Emitted in `main`'s prologue as a
    /// one-time `__axon_set_provenance_source` call. Empty ⇒ no call emitted.
    pub(super) source_path: String,
    /// R4 §4.3: the name of the current fn if it carries `@[agent]`, else `None`.
    /// Set on entry to an agent fn; while set, every capability-bearing builtin
    /// call emits an `agent_action` audit record (the mandatory, un-opt-out-able
    /// agent action log, I-13) — matching the interpreter.
    pub(super) current_agent_fn: Option<String>,
    /// ASI Layer-3 `@[verify]` runtime: when emitting a function that carries a
    /// decodable `@[verify(<ident> OP K)]`, this holds `(fn_name, ident, op_str,
    /// bound)` so each return site injects a runtime check via
    /// `__axon_verify_panic`. `ident` is `"confidence"` (Uncertain field 1) or
    /// `"value"` (the Uncertain value field OR the whole scalar return). `op_str`
    /// is the source operator (`">="`, …) and `bound` is the literal K. `None`
    /// whenever the surrounding function has no decodable verify spec.
    pub(super) current_verify_fn: Option<(String, String, &'static str, f64)>,
    /// Phase 5: when emitting a fn whose declared return type is a refinement
    /// (`-> T where P`) with a lowerable predicate, this holds `(refine_name,
    /// predicate)` so every return site injects a runtime POSTCONDITION check via
    /// `__axon_refine_panic` (exit 6) — the dual of the entry-time precondition
    /// check (`emit_refine_preconditions`). `None` when the return is not a
    /// refinement; a refinement whose predicate is out of the lowerable subset is
    /// E0910-refused in `emit_fn` instead of being set here.
    pub(super) current_ret_refine: Option<(String, ast::Expr)>,
    /// R7 (AOT-wasm): true when emitting for a wasm32 target. wasm32 is an
    /// ILP32 target — `size_t`/pointers are 32-bit — so the C runtime's
    /// `malloc`/`free`/`realloc` take an **i32** size, not the i64 the native
    /// (LP64) path uses. When set, the malloc-family declarations use an i32
    /// size param and call sites truncate the i64 byte-count to i32. Set by
    /// `set_target_is_wasm` BEFORE `emit_program`; defaults to false (native).
    pub(super) target_is_wasm: bool,
    /// R17 §12 Q9: true for `--freestanding` builds. `axon-rt` (the runtime
    /// providing `__axon_arith_panic`/`__axon_bounds_panic`/`__axon_refine_panic`
    /// etc.) is never linked into a freestanding kernel — there is no host OS
    /// underneath it to provide one. When set, those three implicit safety
    /// checks (arithmetic overflow/div-zero, array bounds, refinement
    /// violations — the ones the compiler inserts automatically, not an
    /// explicit API call) get a minimal internal trap DEFINED in the same
    /// module instead of an external symbol DECLARED against it: write a
    /// distinguishing marker byte to the QEMU debugcon port (0xE9, the same
    /// diagnostic convention every other R17 example/test already uses), then
    /// halt forever. Set by `set_freestanding` BEFORE `emit_program`; defaults
    /// to false (hosted builds keep linking the real `axon-rt` implementation,
    /// unchanged).
    pub(super) freestanding: bool,
    /// Hard codegen errors collected during emission (e.g. a known builtin that
    /// has no native lowering). emit_program does not return a Result, so these
    /// accumulate here; the build pipeline checks `codegen_errors()` after
    /// emission and aborts rather than shipping a binary that silently computes
    /// a wrong value (the arr_*/dict_* "returns 0 natively" class).
    pub(super) codegen_errors: Vec<String>,
    /// Phase 6: per-fn set of builtin effects each function ACTUALLY performs
    /// (directly or transitively). Computed once at the start of `emit_program`
    /// from `effects::transitive_builtin_effects`. The `WithHandler` lowering
    /// uses it to detect whether a handler genuinely intercepts an effect —
    /// including through a user-fn call (the indirect case the interpreter
    /// discharges dynamically) — so codegen refuses (E0910) instead of silently
    /// erasing and miscompiling. Empty until `emit_program` populates it.
    pub(super) transitive_effects: HashMap<String, std::collections::HashSet<String>>,
    /// Phase 6: compile-time stack of active inline-handler arms, pushed around
    /// the body of a LOWERABLE `with handler { … } { body }` (see
    /// `effects::handler_is_tail_resumptive_lowerable`). Handlers are lexically
    /// scoped, so when `emit_call` emits a builtin carrying a handled effect it
    /// finds the matching arm here and emits the arm (a tail `resume(v)`) in
    /// place of the call — straight-line, no runtime continuation. Only the
    /// direct-builtin tail-resumptive subset is pushed; everything else is still
    /// E0910-refused, so this can never miscompile.
    pub(super) handler_ctx: Vec<Vec<ast::HandlerArm>>,
    /// The inkwell IR holder: the one `Context`/`Module`/`Builder` per codegen
    /// run. Codegen emits LLVM through `self.ir.{context, module, builder}`
    /// (paired with the `build_wrappers::w_*` helpers). This is the SINGLE
    /// IR-emission path — the earlier `IR`-trait/arena abstraction was
    /// abandoned and removed (R1e); there are no longer any "legacy" fields to
    /// migrate off of.
    pub(super) ir: ir_inkwell::InkwellBackend<'ctx>,
}

impl<'ctx> Codegen<'ctx> {
    /// Route a value `main` is about to return through the runtime's
    /// program-status rule (`__axon_main_status` in `axon-rt`), so a native
    /// binary reports the same status — and prints the same line — as
    /// `axon run` does for the same return (I-2).
    ///
    /// A no-op for every other function, and for a `main` not returning an
    /// `i64`: the rule is about the process exit status, and that is the only
    /// thing `main`'s return becomes.
    pub(super) fn map_main_exit_status(
        &mut self,
        fn_val: FunctionValue<'ctx>,
        v: BasicValueEnum<'ctx>,
    ) -> BasicValueEnum<'ctx> {
        if fn_val.get_name().to_str() != Ok("main") {
            return v;
        }
        let BasicValueEnum::IntValue(iv) = v else {
            return v;
        };
        if iv.get_type().get_bit_width() != 64 {
            return v;
        }
        // A CONSTANT return that the rule leaves alone needs no call at all.
        // Without this every program would reference `__axon_main_status`, and a
        // pure-integer program is supposed to link ZERO `__axon_*` symbols —
        // `wasm_object_prune.sh` checks exactly that, and freestanding targets
        // (R17) depend on it. Only a constant the rule would REWRITE keeps the
        // call, so the runtime message still appears where the interpreter
        // prints one (the two engines must agree on stderr, not just on status).
        if let Some(k) = iv.get_sign_extended_constant() {
            if crate::interp::returned_exit_status(k) == (k as i32, None) {
                return v;
            }
        }
        let i64_ty = self.ir.context.i64_type();
        let f = match self.ir.module.get_function("__axon_main_status") {
            Some(f) => f,
            None => {
                let ty = i64_ty.fn_type(&[i64_ty.into()], false);
                self.ir.module.add_function("__axon_main_status", ty, None)
            }
        };
        build_wrappers::w_call(&self.ir.builder, f, &[iv.into()], "main_status")
            .try_as_basic_value()
            .left()
            .unwrap_or(v)
    }

    pub fn new(context: &'ctx Context, module_name: &str) -> Self {
        // InkwellBackend owns the only module + builder (IR_REARCH.md option
        // (c)); Codegen accesses them through `self.ir.{module, builder,
        // context}`. One module per run → a single symbol table.
        let module = context.create_module(module_name);
        let builder = context.create_builder();
        let ir = ir_inkwell::InkwellBackend::adopt(context, module, builder);
        Self {
            ir,
            locals: HashMap::new(),
            functions: HashMap::new(),
            struct_fields: HashMap::new(),
            struct_field_sem_types: HashMap::new(),
            refinement_base: HashMap::new(),
            refine_preds: HashMap::new(),
            discharged: crate::verify::Discharged::default(),
            struct_field_refines: HashMap::new(),
            struct_whole_refines: HashMap::new(),
            fn_return_types: HashMap::new(),
            local_types: HashMap::new(),
            closure_sigs: HashMap::new(),
            current_result_types: None,
            current_option_inner: None,
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
            pending_lambda_param_tys: None,
            current_adaptive_fn: None,
            current_adaptive_input: None,
            adaptive_registry_targets: Vec::new(),
            goal_name_targets: Vec::new(),
            source_path: String::new(),
            current_agent_fn: None,
            current_verify_fn: None,
            current_ret_refine: None,
            target_is_wasm: false,
            freestanding: false,
            codegen_errors: Vec::new(),
            transitive_effects: HashMap::new(),
            handler_ctx: Vec::new(),
        }
    }

    /// Hard errors collected during emission (see `codegen_errors` field). The
    /// build pipeline calls this after `emit_program` and aborts if non-empty.
    pub fn codegen_errors(&self) -> &[String] {
        &self.codegen_errors
    }

    /// Phase 5 §4: install the SMT-discharged obligation set. Call BEFORE
    /// `emit_program`. For every fn whose scalar `@[verify]` or refinement-return
    /// postcondition was proven ∀-inputs, the corresponding runtime check is not
    /// armed (it is provably dead). A no-op for unproven obligations, so native
    /// output is unchanged unless an `smt`-feature pipeline supplies a set.
    pub fn set_discharged(&mut self, discharged: crate::verify::Discharged) {
        self.discharged = discharged;
    }

    /// R7: declare the target as wasm32 (ILP32) so the malloc-family runtime
    /// declarations and their call sites use a 32-bit size. Call BEFORE
    /// `emit_program`; native builds leave this false (LP64, i64 size).
    pub fn set_target_is_wasm(&mut self, is_wasm: bool) {
        self.target_is_wasm = is_wasm;
    }

    /// R17 §12 Q9: mark this build as `--freestanding`. Call BEFORE
    /// `emit_program`; defaults to false (hosted).
    pub fn set_freestanding(&mut self, freestanding: bool) {
        self.freestanding = freestanding;
    }

    /// R7: the LLVM integer type of a C `size_t` on the current target — i32 on
    /// wasm32 (ILP32), i64 on native (LP64). Used to declare `malloc`/`free`/
    /// `realloc` and to size their arguments so the emitted object's signatures
    /// match the wasm32 libc at link (otherwise: `type mismatch: expected i32,
    /// found i64`).
    pub(super) fn size_ty(&self) -> inkwell::types::IntType<'ctx> {
        if self.target_is_wasm {
            self.ir.context.i32_type()
        } else {
            self.ir.context.i64_type()
        }
    }

    /// R7: narrow an i64 byte-count to the target's `size_t` for a malloc/free/
    /// memcpy-family call. On native (LP64) returns the value unchanged; on
    /// wasm32 (ILP32) truncates i64→i32 so the arg matches the canonical i32
    /// malloc declaration. Use at every malloc call site that builds its own
    /// `build_call(malloc_fn, …)` rather than going through `emit_malloc`.
    pub(super) fn msize(
        &self,
        byte_count: inkwell::values::IntValue<'ctx>,
        name: &str,
    ) -> inkwell::values::IntValue<'ctx> {
        if self.target_is_wasm {
            self.ir
                .builder
                .build_int_truncate(byte_count, self.size_ty(), name)
                .unwrap()
        } else {
            byte_count
        }
    }

    /// R7: zero-extend a `size_t`-typed value (i32 on wasm32) back to i64 for
    /// storage in an i64 field (e.g. the AxonStr `len`). Identity on native,
    /// where `size_t` is already i64. The inverse of `msize` — use on the
    /// RESULT of a size_t-returning libc call (`strlen`) before it flows into
    /// the i64 ABI.
    pub(super) fn zext_size_to_i64(
        &self,
        v: inkwell::values::IntValue<'ctx>,
        name: &str,
    ) -> inkwell::values::IntValue<'ctx> {
        if self.target_is_wasm {
            self.ir
                .builder
                .build_int_z_extend(v, self.ir.context.i64_type(), name)
                .unwrap()
        } else {
            v
        }
    }

    /// R7: get-or-declare `malloc` with the target-correct `size_t` width, and
    /// build a call passing `byte_count` (an i64) truncated to `size_t`. Returns
    /// the raw `i8*` result. Centralizes the 8 ad-hoc malloc declarations so they
    /// can't disagree on width (first-declaration-wins per module).
    pub(super) fn emit_malloc(
        &self,
        byte_count: inkwell::values::IntValue<'ctx>,
        name: &str,
    ) -> inkwell::values::PointerValue<'ctx> {
        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());
        let size_ty = self.size_ty();
        let malloc_fn = self.ir.module.get_function("malloc").unwrap_or_else(|| {
            let malloc_ty = i8_ptr.fn_type(&[size_ty.into()], false);
            self.ir.module.add_function("malloc", malloc_ty, None)
        });
        // Narrow the i64 byte-count to size_t when targeting wasm32 (ILP32).
        let size_arg = if self.target_is_wasm {
            self.ir
                .builder
                .build_int_truncate(byte_count, size_ty, "msize")
                .unwrap()
        } else {
            byte_count
        };
        let call = self
            .ir
            .builder
            .build_call(malloc_fn, &[size_arg.into()], name)
            .unwrap();
        call.try_as_basic_value()
            .left()
            .unwrap()
            .into_pointer_value()
    }

    /// R4: set the program source path stamped into native `@[adaptive]`
    /// provenance (`"src"` field), for parity with the interpreter. Call after
    /// `new` and before `emit_program`.
    pub fn set_source_path(&mut self, path: impl Into<String>) {
        self.source_path = path.into();
    }

    /// Forward-declare every top-level function so mutual recursion resolves.
    pub fn declare_functions(&mut self, program: &ast::Program) {
        self.declare_builtins();
        // Phase 5: collect named refinements → base BEFORE declare_types, so
        // refinement-typed struct fields / params resolve transparently.
        for item in &program.items {
            if let ast::Item::RefineDef(r) = item {
                self.refinement_base.insert(r.name.clone(), r.base.clone());
                // Phase 5: also index the predicate for the entry-time runtime
                // precondition check (mirrors the interpreter's refine_preds).
                self.refine_preds
                    .insert(r.name.clone(), (*r.predicate).clone());
            }
        }
        // Phase 5: index struct-construction refinement obligations — per-field
        // refinements and the whole-struct `where` predicate — so the StructLit
        // emitter can check them at runtime (the codegen dual of the interp
        // StructLit checks). Done after refine_preds is filled above.
        for item in &program.items {
            if let ast::Item::TypeDef(td) = item {
                let refs: Vec<(String, String)> = td
                    .fields
                    .iter()
                    .filter_map(|f| match &f.ty {
                        ast::AxonType::Named(rn) if self.refine_preds.contains_key(rn) => {
                            Some((f.name.clone(), rn.clone()))
                        }
                        _ => None,
                    })
                    .collect();
                if !refs.is_empty() {
                    self.struct_field_refines.insert(td.name.clone(), refs);
                }
                if let Some(pred) = &td.refinement {
                    self.struct_whole_refines
                        .insert(td.name.clone(), (**pred).clone());
                }
            }
        }
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
        let impls: Vec<ast::ImplBlock> = program
            .items
            .iter()
            .filter_map(|item| {
                if let ast::Item::ImplBlock(b) = item {
                    Some(b.clone())
                } else {
                    None
                }
            })
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
                    if p.name == "self" {
                        continue;
                    }
                    if let Some(llvm_ty) = self.llvm_type_from_axon(&p.ty) {
                        param_tys.push(llvm_ty.into());
                    }
                }

                let ret_sem = tm
                    .return_type
                    .as_ref()
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
                self.vtable_thunk_types
                    .insert((trait_name.clone(), tm.name.clone()), fn_ty);
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
                // R17 Slice 3: `@[packed]` lays the struct out with NO inter-field
                // padding (alignment 1) — exact byte layout for a hardware
                // descriptor (GDT/IDT entry). `@[repr(C)]` keeps the natural C
                // layout (the default field order is already declaration order),
                // and `@[align(N)]` raises the whole-struct alignment at its
                // allocation sites (the LLVM struct type itself only carries the
                // packed bit). So packed drives the struct body's packed flag.
                let packed = td.attrs.iter().any(|a| a.name == "packed");
                let named_struct = self.ir.context.opaque_struct_type(&td.name);
                named_struct.set_body(&field_types, packed);
                let field_names: Vec<String> = td.fields.iter().map(|f| f.name.clone()).collect();
                self.struct_fields.insert(td.name.clone(), field_names);
                let field_sem_types: Vec<Type> = td
                    .fields
                    .iter()
                    .map(|f| self.axon_type_to_semantic(&f.ty))
                    .collect();
                self.struct_field_sem_types
                    .insert(td.name.clone(), field_sem_types);
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
            // A void `fn main()` is emitted as `int main()` (returns 0). On
            // native that matches the C `int main(void)` the OS startup expects.
            // R7: on wasm32 the wasi libc provides a C `int main(int, char**)`
            // startup wrapper (`__original_main`/`_start`) that BINDS any symbol
            // literally named `main` with the i32 return — so our 0-arg i32
            // `main` links as that 2-arg convention and `wasmtime --invoke main`
            // fails with "not enough arguments". Emitting an i64 return (as the
            // explicit `fn main() -> i64` case already does, which works) avoids
            // the C-main binding, so the reactor `--export=main` is a clean
            // 0-arg entry. Native keeps i32.
            let ret_int = if self.target_is_wasm {
                self.ir.context.i64_type()
            } else {
                self.ir.context.i32_type()
            };
            let fn_ty = ret_int.fn_type(&param_tys, false);
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

        // R17 Slice 1: @[naked] → LLVM "naked" attribute (no prologue/epilogue).
        if f.attrs.iter().any(|a| a.name == "naked") {
            let kind = inkwell::attributes::Attribute::get_named_enum_kind_id("naked");
            if kind != 0 {
                let attr = self.ir.context.create_enum_attribute(kind, 0);
                fn_val.add_attribute(inkwell::attributes::AttributeLoc::Function, attr);
            }
        }

        // R17 Slice 1: @[interrupt] → x86-interrupt calling convention (CC 83).
        if f.attrs.iter().any(|a| a.name == "interrupt") {
            fn_val.set_call_conventions(83);
        }

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

        // Phase 6: precompute per-fn ACTUAL builtin effects (transitive) so the
        // `with handler` lowering can tell whether a handler genuinely intercepts
        // an effect — including one reached through a user-fn call — and refuse
        // (E0910) the cases native codegen can't lower, instead of erasing them.
        self.transitive_effects = crate::effects::transitive_builtin_effects(program);

        // ── ASI Layer-3: collect eligible adaptive fns for registry init. ─────
        // v1 narrowing: only `@[adaptive] fn(i64) -> i64` is eligible for
        // live hill-climb.  Anything else (multi-arg, f64 input, str input,
        // non-i64 return) is silently skipped here; goal_run will fall back
        // to the Layer-2 retrospective best-observed path for those.
        self.adaptive_registry_targets.clear();
        for (mangled, f) in &fn_work {
            if !has_adaptive_attr(&f.attrs) {
                continue;
            }
            if f.params.len() != 1 {
                continue;
            }
            let p_sem = self.axon_type_to_semantic(&f.params[0].ty);
            if !matches!(p_sem, Type::I64) {
                continue;
            }
            let r_sem = f
                .return_type
                .as_ref()
                .map(|t| self.axon_type_to_semantic(t))
                .unwrap_or(Type::Unit);
            if !matches!(r_sem, Type::I64) {
                continue;
            }
            // Use the (mangled) name so we register the actual LLVM symbol.
            // For top-level fns this equals f.name; for impl methods it's
            // `Type__method`.  v1 doesn't expect impl methods to carry
            // @[adaptive] but if they do the registration is still valid.
            self.adaptive_registry_targets.push(mangled.clone());
        }

        // ── BUG_HUNT #19: collect goal-name targets for the I-9 typo guard. ───
        // Only when the program actually calls `goal_run` do we register the fn
        // names — so native `goal_run` can reject an unknown (typo'd) metric
        // name the same way the interpreter does, instead of silently returning
        // `target`. Use UNMANGLED top-level fn names, matching the string a
        // `goal_run("name", …)` literal passes (and the interpreter's
        // `Interp::fns` keyset).
        self.goal_name_targets.clear();
        if program.items.iter().any(program_calls_goal_run) {
            for item in &program.items {
                if let ast::Item::FnDef(f) = item {
                    self.goal_name_targets.push(f.name.clone());
                }
            }
        }

        // ── R3: native AI tier-routing refusal (I-2, sound-by-refusal). ───────
        // The native `__axon_ai_complete` ABI carries no model, so it always
        // routes to the DEFAULT model (sonnet) — which is exactly the `balanced`
        // tier (= DEFAULT_TIER). So native MATCHES the interpreter for balanced /
        // no-policy fns, but would SILENTLY misroute a `cheap`/`strong` policy
        // (the interp routes those to haiku/opus via `Tier::api_model`). Rather
        // than emit a binary that quietly calls the wrong model, refuse (E0910):
        // such a program runs faithfully under the interpreter. (An unknown tier
        // name also refuses — the interp stops with E1302, which native can't
        // replicate.) The tier is resolved by the SHARED `tier_from_attrs`, so
        // codegen and the interpreter agree on each fn's tier exactly.
        for (mangled, f) in &fn_work {
            if !expr_calls(&f.body, "ai_complete") {
                continue;
            }
            let refuse = !matches!(
                crate::ai_routing::tier_from_attrs(&f.attrs),
                Ok(crate::ai_routing::Tier::Balanced)
            );
            if refuse {
                let msg = format!(
                    "codegen error [E0910]: native codegen cannot honor a non-`balanced` AI \
                     tier for `ai_complete` in `{mangled}` — the native runtime routes every \
                     call to the default (balanced/sonnet) model, so a `cheap`/`strong` policy \
                     would silently call the wrong model. Run this program under the interpreter \
                     (`axon run`), which routes the tier correctly, or use the `balanced` tier."
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
            }

            // ── R3c/F141: native AI *budget* refusal (I-2, sound-by-refusal). ─
            // `@[ai(policy(budget: N))]` makes the (N+1)th `ai_complete` a fatal
            // E1301 (exit 5) under the interpreter. The native `ai_complete` ABI
            // carries no meter at all — `grep E1301` over codegen/ and axon-rt/
            // returns nothing — so the AOT binary happily ran every call and
            // exited 0. That is the I-2 violation in the UNSAFE direction: the
            // binary keeps spending past a policy stop, which defeats the entire
            // point of declaring a budget. Until the meter exists natively
            // (a per-activation counter + an `__axon_ai_policy_halt` extern,
            // mirroring `__axon_verify_panic`), refuse rather than emit a binary
            // that silently ignores the ceiling.
            //
            // The refusal condition mirrors the interpreter's ENFORCEMENT
            // condition exactly: the meter keys on the fn that is *current* when
            // the call happens (R3c §3 "per-fn-activation"), so only a DIRECT
            // `ai_complete` in this fn's own body is metered. A call made from an
            // un-budgeted helper is unmetered in the interpreter too, so refusing
            // it would reject a program the two backends already agree on.

            // ── R3b/F062: the PER-CALL `tier:` (AUDIT T46). ──────────────────
            // The scan above resolves the tier from fn ATTRIBUTES only. R3b also
            // allows `ai_complete("hi", tier: "cheap")`, carried on
            // `Expr::Call { tier }` — and the interpreter gives that form TOP
            // priority (`current_ai_tier` step 1). Codegen dropped it under a
            // comment asserting "native AI calls aren't in the codegen path",
            // which was false. So the attribute path was refused and the
            // per-call path, with the identical hazard, was wide open:
            //
            //   ai_complete("say hi", tier: "cheap")  ->  axon build exit 0
            //   interp routes it to haiku ($0.000000); the binary calls sonnet.
            //
            // ANY `Some(tier)` refuses, not just non-`balanced`: an unknown tier
            // name is E1302 (exit 5) in the interpreter, which native cannot
            // replicate either, so "balanced" is the only value that would need
            // a carve-out and it buys nothing.
            let mut refusals: Vec<String> = Vec::new();
            ast::walk_expr(&f.body, &mut |x| {
                if let ast::Expr::Call { tier: Some(t), .. } = x {
                    let msg = format!(
                        "codegen error [E0910]: native codegen cannot honor the per-call AI \
                         tier `tier: \"{t}\"` in `{mangled}` — the native runtime routes every \
                         call to the default (balanced/sonnet) model, so this call would \
                         silently reach the wrong model and be metered at the wrong rate. Run \
                         this program under the interpreter (`axon run`), which routes the \
                         per-call tier correctly, or drop the `tier:` argument."
                    );
                    if !refusals.contains(&msg) {
                        refusals.push(msg);
                    }
                }
            });
            for msg in refusals {
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
            }
            //
            // `Some(Err(_))` — a MALFORMED budget — deliberately does not match:
            // the interpreter warns (W1311) and runs the fn unmetered, so the two
            // backends already agree and there is nothing to refuse. Refusing it
            // would make a typo'd budget stricter than a correct one.
            if let Some(Ok(n)) = crate::ai_routing::budget_from_attrs(&f.attrs) {
                let msg = format!(
                    "codegen error [E0910]: native codegen cannot enforce the AI call \
                     budget `@[ai(policy(budget: {n}))]` on `{mangled}` — the native \
                     runtime has no call meter, so the binary would run past the budget \
                     and exit 0 where the interpreter stops with [E1301] and exit 5. Run \
                     this program under the interpreter (`axon run`), which meters the \
                     calls, or remove the `budget:` field."
                );
                if !self.codegen_errors.iter().any(|e| e == &msg) {
                    self.codegen_errors.push(msg);
                }
            }
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
                    Ok(cv) => {
                        self.comptime_env.insert(name.clone(), cv);
                    }
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
        let impls: Vec<ast::ImplBlock> = program
            .items
            .iter()
            .filter_map(|item| {
                if let ast::Item::ImplBlock(b) = item {
                    Some(b.clone())
                } else {
                    None
                }
            })
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
                let has_self_param = blk
                    .methods
                    .iter()
                    .find(|m| m.name == tm.name)
                    .map(|m| m.params.iter().any(|p| p.name == "self"))
                    .unwrap_or(false);

                if has_self_param {
                    if let Some(ty) = concrete_llvm_ty {
                        let self_val =
                            build_wrappers::w_load(&self.ir.builder, ty, self_ptr, "self_val");
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

                let call =
                    build_wrappers::w_call(&self.ir.builder, concrete_fn, &call_args, "thunk_ret");
                let ret_sem = tm
                    .return_type
                    .as_ref()
                    .map(|t| self.axon_type_to_semantic(t))
                    .unwrap_or(crate::types::Type::Unit);

                if matches!(ret_sem, crate::types::Type::Unit) {
                    build_wrappers::w_ret_void(&self.ir.builder);
                } else if let Some(ret_val) = call.try_as_basic_value().left() {
                    build_wrappers::w_ret(&self.ir.builder, ret_val);
                } else {
                    build_wrappers::w_ret_void(&self.ir.builder);
                }

                if let Some(b) = saved {
                    self.ir.builder.position_at_end(b);
                }
            }
        }
    }

    /// Emit one `@vtable_Trait_Type = constant [N x ptr] [...]` global per impl block.
    fn emit_vtable_globals(&mut self, program: &ast::Program) {
        let i8_ptr = self.ir.context.i8_type().ptr_type(AddressSpace::default());

        let impls: Vec<ast::ImplBlock> = program
            .items
            .iter()
            .filter_map(|item| {
                if let ast::Item::ImplBlock(b) = item {
                    Some(b.clone())
                } else {
                    None
                }
            })
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
            if n == 0 {
                continue;
            }

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

        // ── R21 — Decimal is interp-only in this slice (sound-by-refusal). ─────
        // Exact money arithmetic (i128 mantissa, banker's-rounding mul/div) is
        // fully wired in the tree-walking interpreter. Native i128 codegen for
        // the rounding-bearing ops (mul rescale, div) is NOT yet implemented, so
        // rather than emit subtly-wrong money IR we REFUSE any function that
        // touches a `Decimal` (literal, signature, or `decimal_*` builtin) with a
        // clear E0910 — exactly like host_await / kernel / sandbox (I-2:
        // refuse, never silently miscompile). `axon run` (interp) is unaffected.
        if fn_uses_decimal(f) {
            let msg = format!(
                "codegen error [E0910]: native codegen cannot yet lower the `Decimal` \
                 fixed-point type used by `{}` — run it on the interpreter (`axon run`); \
                 exact Decimal arithmetic is interp-only in this slice (R21)",
                f.name
            );
            if !self.codegen_errors.iter().any(|e| e == &msg) {
                self.codegen_errors.push(msg);
            }
            // Emit a trivial body so IR generation doesn't crash before the
            // pipeline checks codegen_errors() and aborts. Route through the w_*
            // wrapper (R1e: one IR path — no raw builder calls in mod.rs).
            build_wrappers::w_ret_void(&self.ir.builder);
            return;
        }

        // Save outer locals/types; reset for this function scope.
        let saved_locals = std::mem::take(&mut self.locals);
        let saved_local_types = std::mem::take(&mut self.local_types);
        let saved_result_types = self.current_result_types.take();
        let saved_adaptive = self.current_adaptive_fn.take();
        let saved_adaptive_input = self.current_adaptive_input.take();
        let saved_agent = self.current_agent_fn.take();
        let saved_verify = self.current_verify_fn.take();
        let saved_ret_refine = self.current_ret_refine.take();
        // R4 §4.3: arm the mandatory agent action log for an `@[agent]` fn.
        if f.attrs.iter().any(|a| a.name == "agent") {
            self.current_agent_fn = Some(f.name.clone());
        }

        // ── @[adaptive]: emit a "call" event at the prologue. ─────────────────
        // Activates `current_adaptive_fn` so any subsequent build_return inside
        // this function gets a "return" event injected before it.
        if has_adaptive_attr(&f.attrs) {
            self.current_adaptive_fn = Some(f.name.clone());
            self.emit_provenance_log(&f.name, "call");
            // F11: capture the leading i64 parameter (the optimizer's input) so
            // the return log can record (input, score). Only when param 0 is an
            // i64 — matches the runtime's `(i64) -> i64` warm-start narrowing.
            if let Some(inkwell::values::BasicValueEnum::IntValue(iv)) = llvm_fn.get_nth_param(0) {
                if iv.get_type() == self.ir.context.i64_type() {
                    self.current_adaptive_input = Some(iv);
                }
            }
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
            if let Some((ident, op, bound)) =
                crate::verify::decode_verify_predicate_with_ident(&spec.predicate)
            {
                let ret_sem = f
                    .return_type
                    .as_ref()
                    .map(|t| self.axon_type_to_semantic(t));
                // Uncertain AND Temporal both carry value(0)/confidence(1) fields,
                // so a `value`/`confidence` predicate applies to either.
                let ret_is_wrapper =
                    matches!(ret_sem, Some(Type::Uncertain(_)) | Some(Type::Temporal(_)));
                let ret_is_scalar = matches!(
                    ret_sem,
                    Some(Type::I64 | Type::I32 | Type::F64 | Type::Bool)
                );
                // Arm when: `confidence`/`value` on an Uncertain/Temporal return,
                // OR `value` on a SCALAR return (a `@[verify(value <= 500)]` bound
                // must be enforced natively too, matching the interpreter).
                let armable = (ret_is_wrapper && (ident == "confidence" || ident == "value"))
                    || (ret_is_scalar && ident == "value");
                // Phase 5 §4: don't arm the gate if the SMT prover discharged this
                // fn's `value OP K` bound for all inputs — the SCALAR case is
                // exactly what `prove_verify_bounds` proves, so the check is dead.
                if armable && !self.discharged.verify_proven(&f.name) {
                    let op_str = crate::verify::binop_to_verify_str(&op);
                    self.current_verify_fn = Some((f.name.clone(), ident, op_str, bound));
                }
            }
        }

        // ── Phase 5 refinement RETURN postcondition: arm the return-site check. ─
        // If the declared return type is a refinement (`-> T where P`) with a
        // lowerable predicate, every return site emits a guarded
        // `__axon_refine_panic` (exit 6) when the value fails P — the dual of the
        // entry-time `emit_refine_preconditions`. A refinement whose predicate is
        // OUTSIDE the lowerable subset is E0910-refused here (never silently
        // emitted without the check — that would let native return a value the
        // interpreter rejects).
        // Phase 5 §4: skip the whole arm — including the E0910 out-of-subset
        // refusal — when the SMT prover discharged this fn's refinement-return
        // postcondition for all inputs. A proven obligation needs no lowering at
        // all, so SMT can even discharge a predicate native codegen could not
        // itself lower (the runtime check is dead either way).
        if !self.refine_preds.is_empty() && !self.discharged.refine_return_proven(&f.name) {
            if let Some(ast::AxonType::Named(rname)) = &f.return_type {
                if let Some(pred) = self.refine_preds.get(rname.as_str()).cloned() {
                    if Self::refine_predicate_is_lowerable(&pred, &self.fndefs) {
                        self.current_ret_refine = Some((rname.clone(), pred));
                    } else {
                        let msg = format!(
                            "codegen error [E0910]: native codegen cannot lower the refinement \
                             predicate of return type `{rname}` of `{}` — it is outside the \
                             runtime-checkable subset. Run it under the interpreter (`axon run`).",
                            f.name
                        );
                        if !self.codegen_errors.iter().any(|e| e == &msg) {
                            self.codegen_errors.push(msg);
                        }
                    }
                }
            }
        }

        // ── ASI Layer-3: in `main`'s prologue, register each eligible
        // `@[adaptive] fn(i64) -> i64` with the runtime adaptive registry.
        // The runtime then knows how to call them back during goal_run
        // hill-climb.  No-op when the target list is empty, so non-AI
        // programs pay nothing.
        if f.name == "main" {
            // Convert a native stack overflow (deep recursion) into a graceful
            // exit-101 panic instead of a raw SIGSEGV (139) — interp parity on the
            // recursion fault. First in the prologue so it covers everything after.
            self.emit_recursion_guard_init();
            self.emit_adaptive_registry_init();
            // BUG_HUNT #19: register every fn name so native goal_run can reject
            // a typo'd metric the same way the interpreter does (I-9 parity).
            self.emit_goal_name_registry_init();
            // R4: stamp the source path into the runtime so native @[adaptive]
            // provenance carries the `"src"` field (interp parity).
            self.emit_provenance_source_init();
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

        // Phase 5: refinement-type PRECONDITIONS. A parameter `p: T where P`
        // (named or inline-desugared) carries a runtime contract the checker
        // discharges statically only for constant args (E1209). Emit the spec's
        // Z3-free fallback for non-constant args: evaluate P at entry with `_`
        // aliased to the param and call `__axon_refine_panic` (exit 6) on
        // violation — the codegen analog of the interpreter's `call_fn` check, so
        // native and `axon run` agree on the exit code (I-2). Out-of-subset
        // predicates are E0910-refused inside the helper (never silently skipped).
        if !self.refine_preds.is_empty() {
            self.emit_refine_preconditions(f, llvm_fn);
        }

        let body_val = self.emit_expr(&f.body, llvm_fn);

        // Emit return if the builder is still on a live block.
        if self
            .ir
            .builder
            .get_insert_block()
            .and_then(|b| b.get_terminator())
            .is_none()
        {
            if f.name == "main" && matches!(ret_sem, Type::Unit) {
                // Match the return width chosen at declaration: i64 on wasm32
                // (to dodge the wasi C-main binding), i32 on native.
                let ret_int = if self.target_is_wasm {
                    self.ir.context.i64_type()
                } else {
                    self.ir.context.i32_type()
                };
                let zero = ret_int.const_int(0, false);
                // main() returning 0 isn't an interesting score; use the legacy event log.
                self.log_return_if_adaptive();
                build_wrappers::w_ret(&self.ir.builder, zero.into());
            } else {
                match body_val {
                    Some(v) if !matches!(ret_sem, Type::Unit) => {
                        // Soft typing at the RETURN boundary: a fn declared `-> T`
                        // where T is a plain SCALAR (i64/i32/f64/bool) whose body
                        // produced an `Uncertain<T>` (`{T,f64,i64}` struct) unwraps
                        // to the inner value (field 0), matching the interpreter —
                        // else the struct is returned where a scalar is declared
                        // (IR mismatch). Scoped to SCALAR return types only, so a
                        // str/struct/tuple/Uncertain return is never touched.
                        let ret_is_scalar =
                            matches!(ret_sem, Type::I64 | Type::I32 | Type::F64 | Type::Bool);
                        let v = if ret_is_scalar {
                            if let BasicValueEnum::StructValue(sv) = v {
                                self.ir
                                    .builder
                                    .build_extract_value(sv, 0, "unc_ret")
                                    .unwrap_or(v)
                            } else {
                                v
                            }
                        } else {
                            v
                        };
                        self.log_return_if_adaptive_val(v);
                        self.emit_verify_check_if_needed(v, llvm_fn);
                        self.emit_refine_return_check_if_needed(v, llvm_fn);
                        // AFTER the @[verify]/refinement checks: those judge the
                        // value the program computed, not the status it turns
                        // into. Mapping first would have them inspect a 1.
                        let v = self.map_main_exit_status(llvm_fn, v);
                        build_wrappers::w_ret(&self.ir.builder, v);
                    }
                    None if !matches!(ret_sem, Type::Unit) => {
                        // No value from body but function has non-void return type:
                        // emit a zero value of the appropriate type to keep IR valid.
                        if let Some(ret_llvm_ty) = self.llvm_type(&ret_sem) {
                            let zero_val = ret_llvm_ty.const_zero();
                            self.log_return_if_adaptive_val(zero_val);
                            self.emit_verify_check_if_needed(zero_val, llvm_fn);
                            self.emit_refine_return_check_if_needed(zero_val, llvm_fn);
                            build_wrappers::w_ret(&self.ir.builder, zero_val);
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
        self.current_adaptive_input = saved_adaptive_input;
        self.current_agent_fn = saved_agent;
        self.current_verify_fn = saved_verify;
        self.current_ret_refine = saved_ret_refine;
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
                    // Phase 5: a named refinement is transparent — resolve to its
                    // erased base type (e.g. `Positive`/`__refine_N` → `i64`).
                    if let Some(base) = self.refinement_base.get(other) {
                        let base = base.clone();
                        return self.axon_type_to_semantic(&base);
                    }
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
            ast::AxonType::Slice(inner) => Type::Slice(Box::new(self.axon_type_to_semantic(inner))),
            ast::AxonType::Chan(inner) => Type::Chan(Box::new(self.axon_type_to_semantic(inner))),
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
                params
                    .iter()
                    .map(|p| self.axon_type_to_semantic(p))
                    .collect(),
                Box::new(self.axon_type_to_semantic(ret)),
            ),
            ast::AxonType::Ref(inner) => self.axon_type_to_semantic(inner),
            ast::AxonType::TypeParam(name) => Type::TypeParam(name.clone()),
            ast::AxonType::DynTrait(name) => Type::DynTrait(name.clone()),
            ast::AxonType::Tuple(elems) => Type::Tuple(
                elems
                    .iter()
                    .map(|e| self.axon_type_to_semantic(e))
                    .collect(),
            ),
            // Union types are not yet first-class — fall back to Unknown so
            // codegen does not assert a specific LLVM lowering.
            ast::AxonType::Union(_) => Type::Unknown,
            // R17 HAL: `*T` raw pointer → opaque pointer in codegen.
            ast::AxonType::RawPtr(inner) => {
                Type::RawPtr(Box::new(self.axon_type_to_semantic(inner)))
            }
        }
    }

    /// Resolve the concrete return type of a CALL to a (possibly generic)
    /// function. For a generic fn like `first<T>(a: [T]) -> Option<T>`, the
    /// declared return carries `TypeParam("T")`; without substitution a
    /// `match first(xs) { Some(v) => … }` can't lay out the `Some(v)` binding
    /// (the binding's type stays unresolved → E0701). This binds each type param
    /// by matching the declared param AxonTypes against the actual arguments'
    /// inferred types, then substitutes into the declared return type. Returns
    /// `None` when the fn is unknown or a param binding can't be inferred (the
    /// caller then keeps the unresolved declared type, unchanged behavior).
    fn resolve_call_return_type(&self, name: &str, args: &[ast::Expr]) -> Option<Type> {
        let ret = self.fn_return_types.get(name)?.clone();
        // The fn's declared generic param names (e.g. ["T"]). A bare `T` parses
        // as `AxonType::Named("T")` → `Type::Struct("T")`, so we can't tell it's
        // a param from the Type alone — use this set to recognise them.
        let gp: &[String] = self
            .generic_fn_params
            .get(name)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        if gp.is_empty() || !Self::type_has_param(&ret, gp) {
            return Some(ret);
        }
        let params = self.fn_axon_params.get(name)?;
        // Build a param -> concrete binding by matching declared params against
        // the actual args (peeling `&` and one level of `[T]`/Option/etc).
        let mut binding: HashMap<String, Type> = HashMap::new();
        for (decl, arg) in params.iter().zip(args.iter()) {
            let arg_inner = match arg {
                ast::Expr::UnaryOp {
                    op: ast::UnaryOp::Ref,
                    operand,
                } => operand.as_ref(),
                other => other,
            };
            if let Some(arg_ty) = self.infer_expr_sem_type(arg_inner) {
                Self::bind_type_params(
                    &self.axon_type_to_semantic(decl),
                    &arg_ty,
                    gp,
                    &mut binding,
                );
            }
        }
        if binding.is_empty() {
            return None;
        }
        Some(Self::subst_type_params(&ret, &binding))
    }

    /// True if `ty` mentions one of the generic param names `gp` (as a bare
    /// `Struct(name)` — how a type param survives `axon_type_to_semantic`).
    fn type_has_param(ty: &Type, gp: &[String]) -> bool {
        match ty {
            Type::Struct(n) | Type::TypeParam(n) => gp.iter().any(|p| p == n),
            Type::Option(i)
            | Type::Slice(i)
            | Type::Chan(i)
            | Type::Uncertain(i)
            | Type::Temporal(i) => Self::type_has_param(i, gp),
            Type::Result(a, b) => Self::type_has_param(a, gp) || Self::type_has_param(b, gp),
            Type::Tuple(es) => es.iter().any(|e| Self::type_has_param(e, gp)),
            Type::Fn(ps, r) => {
                ps.iter().any(|p| Self::type_has_param(p, gp)) || Self::type_has_param(r, gp)
            }
            _ => false,
        }
    }

    /// Unify a declared (possibly-generic) type against a concrete one, filling
    /// `out` with param-name -> concrete bindings. `gp` is the set of names that
    /// are type params (a bare `Struct(n)` whose n ∈ gp is a param, not a real
    /// struct).
    fn bind_type_params(
        decl: &Type,
        concrete: &Type,
        gp: &[String],
        out: &mut HashMap<String, Type>,
    ) {
        match (decl, concrete) {
            (Type::Struct(n) | Type::TypeParam(n), c) if gp.iter().any(|p| p == n) => {
                out.entry(n.clone()).or_insert_with(|| c.clone());
            }
            (Type::Option(d), Type::Option(c))
            | (Type::Slice(d), Type::Slice(c))
            | (Type::Chan(d), Type::Chan(c))
            | (Type::Uncertain(d), Type::Uncertain(c))
            | (Type::Temporal(d), Type::Temporal(c)) => Self::bind_type_params(d, c, gp, out),
            (Type::Result(da, db), Type::Result(ca, cb)) => {
                Self::bind_type_params(da, ca, gp, out);
                Self::bind_type_params(db, cb, gp, out);
            }
            (Type::Tuple(ds), Type::Tuple(cs)) => {
                for (d, c) in ds.iter().zip(cs.iter()) {
                    Self::bind_type_params(d, c, gp, out);
                }
            }
            _ => {}
        }
    }

    /// Substitute param-name occurrences in `ty` using `binding` (keys are the
    /// generic param names; they appear as `Struct(name)`/`TypeParam(name)`).
    fn subst_type_params(ty: &Type, binding: &HashMap<String, Type>) -> Type {
        match ty {
            Type::Struct(n) | Type::TypeParam(n) => {
                binding.get(n).cloned().unwrap_or_else(|| ty.clone())
            }
            Type::Option(i) => Type::Option(Box::new(Self::subst_type_params(i, binding))),
            Type::Slice(i) => Type::Slice(Box::new(Self::subst_type_params(i, binding))),
            Type::Chan(i) => Type::Chan(Box::new(Self::subst_type_params(i, binding))),
            Type::Uncertain(i) => Type::Uncertain(Box::new(Self::subst_type_params(i, binding))),
            Type::Temporal(i) => Type::Temporal(Box::new(Self::subst_type_params(i, binding))),
            Type::Result(a, b) => Type::Result(
                Box::new(Self::subst_type_params(a, binding)),
                Box::new(Self::subst_type_params(b, binding)),
            ),
            Type::Tuple(es) => Type::Tuple(
                es.iter()
                    .map(|e| Self::subst_type_params(e, binding))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    /// Convert an `ast::AxonType` directly to an LLVM type.
    fn llvm_type_from_axon(&self, ty: &ast::AxonType) -> Option<BasicTypeEnum<'ctx>> {
        let sem = self.axon_type_to_semantic(ty);
        self.llvm_type(&sem)
    }

    /// R19 Slice C: coerce an LLVM value to the target fixed-width integer type.
    ///
    /// `emit_literal` always emits integer literals as i64. When the binding
    /// annotation is a fixed-width type (I8/I16/I32/U8/U16/U32), the emitted
    /// i64 value must be truncated to the correct narrow LLVM integer type so
    /// that locals are stored at their declared width.  The range-check (E1900)
    /// already guarantees the literal fits, so truncation is safe.
    ///
    /// Also handles the reverse direction (narrow → wider via zext/sext) for
    /// completeness, though the primary need is i64 → narrow.
    ///
    /// Only acts on Int→Int mismatches for the fixed-width integer types; all
    /// other values are returned unchanged.
    pub(crate) fn coerce_to_fixed_width(
        &self,
        val: inkwell::values::BasicValueEnum<'ctx>,
        target: &crate::types::Type,
    ) -> inkwell::values::BasicValueEnum<'ctx> {
        let target_llvm = match self.llvm_type(target) {
            Some(inkwell::types::BasicTypeEnum::IntType(t)) => t,
            _ => return val, // not a fixed-width int target — leave as-is
        };
        let int_val = match val {
            inkwell::values::BasicValueEnum::IntValue(i) => i,
            _ => return val, // not an int — leave as-is (handles struct/ptr/etc.)
        };
        let actual_bits = int_val.get_type().get_bit_width();
        let target_bits = target_llvm.get_bit_width();
        if actual_bits == target_bits {
            return val; // already correct width
        }
        // Bool (i1) is special — don't coerce a bool to a narrower int or vice versa.
        if actual_bits == 1 || target_bits == 1 {
            return val;
        }
        let is_unsigned = matches!(
            target,
            crate::types::Type::U8
                | crate::types::Type::U16
                | crate::types::Type::U32
                | crate::types::Type::U64
        );
        if actual_bits > target_bits {
            // truncate (i64 → i32/i16/i8); range-check (E1900) ensures no data loss
            build_wrappers::w_int_truncate(&self.ir.builder, int_val, target_llvm, "trunc_fw")
                .into()
        } else {
            // extend (narrow → wider); use zext for unsigned, sext for signed
            if is_unsigned {
                build_wrappers::w_int_z_extend(&self.ir.builder, int_val, target_llvm, "zext_fw")
                    .into()
            } else {
                build_wrappers::w_int_s_extend(&self.ir.builder, int_val, target_llvm, "sext_fw")
                    .into()
            }
        }
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

    /// The FIXED (input-independent) return shape of a collection builtin, or
    /// `None` if the builtin's result shape depends on its argument (those —
    /// arr_reverse/map/filter/… — propagate the input slice type and are handled
    /// in `infer_expr_sem_type` itself).
    ///
    /// This is the single place to register a collection builtin whose codegen
    /// result layout is a constant: adding one is a match arm here, not a new
    /// `if name == "…"` branch threaded into the 160-line `infer_expr_sem_type`
    /// heuristic. (A step toward R2a — `governance/specs/R2a-type-map-threading.md`
    /// — which will ultimately thread the HM-inferred type map to codegen and
    /// retire this heuristic wholesale; until then, this keeps the per-builtin
    /// shape data declarative and in one spot, the way R1d did for extern decls.)
    fn fixed_collection_return_type(name: &str) -> Option<Type> {
        let i64_slice = || Type::Slice(Box::new(Type::I64));
        Some(match name {
            // → [i64] regardless of args
            "arr_range" | "arr_repeat" | "arr_flatten" => i64_slice(),
            // → [i64] (v1 int-valued dict)
            "dict_values" => i64_slice(),
            // → [str]
            "dict_keys" | "str_split" => Type::Slice(Box::new(Type::Str)),
            // → [(i64, i64)] — slice of pairs
            "arr_enumerate" | "arr_zip" => {
                Type::Slice(Box::new(Type::Tuple(vec![Type::I64, Type::I64])))
            }
            // → [(str, i64)] — dict entries as a slice of (key, value) tuples
            "dict_to_pairs" => Type::Slice(Box::new(Type::Tuple(vec![Type::Str, Type::I64]))),
            // → [[i64]] — slice of i64 slices
            "arr_chunk" => Type::Slice(Box::new(i64_slice())),
            // → ([i64], [i64]) — tuple of two slices
            "arr_partition" => Type::Tuple(vec![i64_slice(), i64_slice()]),
            _ => return None,
        })
    }

    /// Heuristic: infer the Axon semantic type of an expression without emitting IR.
    /// Used to populate `local_types` and select Result union layouts.
    fn infer_expr_sem_type(&self, expr: &ast::Expr) -> Option<Type> {
        match expr {
            ast::Expr::Literal(lit) => match lit {
                ast::Literal::Int(_) => Some(Type::I64),
                ast::Literal::Float(_) => Some(Type::F64),
                ast::Literal::Decimal(_) => Some(Type::Decimal),
                ast::Literal::Bool(_) => Some(Type::Bool),
                ast::Literal::Str(_) => Some(Type::Str),
            },
            ast::Expr::Ident(name) => self.local_types.get(name).cloned(),
            ast::Expr::Call { callee, args, .. } => {
                if let ast::Expr::Ident(name) = callee.as_ref() {
                    // arr_reverse/take/drop are lowered inline (not in
                    // fn_return_types) and return `[T]` — propagate the input
                    // arg's slice type so a `let b = arr_reverse(&a)` binding is
                    // indexable (b[i]). The arg may be `&a` (UnaryOp::Ref), peel it.
                    //
                    // Builtins whose result shape is a CONSTANT (arr_range→[i64],
                    // dict_keys→[str], arr_chunk→[[i64]], …) are single-sourced in
                    // `fixed_collection_return_type` — registering one is a match
                    // arm there, not a new branch in this 160-line heuristic.
                    if let Some(t) = Self::fixed_collection_return_type(name) {
                        return Some(t);
                    }
                    if name == "arr_reverse"
                        || name == "arr_take"
                        || name == "arr_drop"
                        || name == "arr_map"
                        || name == "arr_filter"
                        || name == "arr_zip_with"
                        || name == "arr_sort_by"
                        || name == "arr_concat"
                        || name == "arr_unique"
                        || name == "arr_push"
                        || name == "arr_take_while"
                        || name == "arr_drop_while"
                    {
                        if let Some(arg0) = args.first() {
                            let inner = match arg0 {
                                ast::Expr::UnaryOp {
                                    op: ast::UnaryOp::Ref,
                                    operand,
                                } => operand.as_ref(),
                                other => other,
                            };
                            return self.infer_expr_sem_type(inner);
                        }
                    }
                    // Resolve a generic return (`first<T>(..) -> Option<T>`) to
                    // the concrete type by binding T from the args; falls back to
                    // the declared (possibly-unresolved) type when not generic or
                    // not inferable.
                    self.resolve_call_return_type(name, args)
                        .or_else(|| self.fn_return_types.get(name).cloned())
                } else {
                    None
                }
            }
            ast::Expr::Ok(_) | ast::Expr::Err(_) => self
                .current_result_types
                .as_ref()
                .map(|(ok, err)| Type::Result(Box::new(ok.clone()), Box::new(err.clone()))),
            ast::Expr::StructLit { name, .. } => {
                if name.contains("::") {
                    // "EnumName::Variant" → Type::Enum("EnumName")
                    let enum_name = name.split("::").next().unwrap_or(name).to_string();
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
            ast::Expr::Block(stmts) => stmts.last().and_then(|s| self.infer_expr_sem_type(&s.expr)),
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
                    ast::BinOp::Add
                    | ast::BinOp::Sub
                    | ast::BinOp::Mul
                    | ast::BinOp::Div
                    | ast::BinOp::Rem => {
                        if is_unc(&lt) || is_unc(&rt) {
                            let inner = unc_inner(&lt)
                                .or_else(|| unc_inner(&rt))
                                .unwrap_or(Type::I64);
                            Some(Type::Uncertain(Box::new(inner)))
                        } else {
                            lt.or(rt)
                        }
                    }
                    ast::BinOp::Eq
                    | ast::BinOp::NotEq
                    | ast::BinOp::Lt
                    | ast::BinOp::Gt
                    | ast::BinOp::LtEq
                    | ast::BinOp::GtEq => {
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
                let recv_ty = self
                    .infer_expr_sem_type(receiver)
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
                    // Record-struct field: look up the declared field type so a
                    // `match b.r { … }` on a `Result`/`Option`-typed field knows
                    // its layout (else the match extracts the payload with the
                    // wrong type → IR-verify failure).
                    (Some(Type::Struct(sname)), fname) => {
                        let names = self.struct_fields.get(&sname)?;
                        let idx = names.iter().position(|n| n == fname)?;
                        self.struct_field_sem_types.get(&sname)?.get(idx).cloned()
                    }
                    _ => None,
                }
            }
            ast::Expr::Index { receiver, .. } => {
                // `arr[i]` → element type of the slice's inner type
                self.infer_expr_sem_type(receiver).and_then(|ty| {
                    if let Type::Slice(inner) = ty {
                        Some(*inner.clone())
                    } else {
                        None
                    }
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
                let sname = if let Type::Struct(sn) = &recv_ty {
                    sn
                } else {
                    return None;
                };
                let field_names = self.struct_fields.get(sname.as_str())?;
                let idx = field_names.iter().position(|n| n == field)?;
                let struct_ty = self.ir.module.get_struct_type(sname)?;
                let field_llvm_ty = struct_ty.get_field_type_at_index(idx as u32)?;
                match field_llvm_ty {
                    BasicTypeEnum::IntType(it) => Some(match it.get_bit_width() {
                        1 => Type::Bool,
                        8 => Type::I8,
                        16 => Type::I16,
                        32 => Type::I32,
                        _ => Type::I64,
                    }),
                    BasicTypeEnum::FloatType(ft) => Some(if ft == self.ir.context.f32_type() {
                        Type::F32
                    } else {
                        Type::F64
                    }),
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

#[cfg(test)]
mod walk_expr_tests {
    use super::*;

    /// Parse a program and return the body of its first fn.
    fn body_of(src: &str) -> ast::Expr {
        let prog = crate::parse_source(src).expect("fixture must parse");
        for item in &prog.items {
            if let ast::Item::FnDef(f) = item {
                return f.body.clone();
            }
        }
        panic!("no fn in fixture");
    }

    fn calls_in(src: &str) -> Vec<String> {
        let mut names = Vec::new();
        ast::walk_expr(&body_of(src), &mut |e| {
            if let ast::Expr::Call { callee, .. } = e {
                if let ast::Expr::Ident(n) = callee.as_ref() {
                    names.push(n.clone());
                }
            }
        });
        names
    }

    #[test]
    fn walk_expr_reaches_arms_the_old_catch_all_dropped_t46() {
        // AUDIT T46. `expr_calls` ended in `_ => false`, so `Select` and
        // `WithHandler` sub-expressions were never visited. Every E0910
        // sound-by-refusal scan in this file is built on that walk, so an
        // unwalked arm is a program COMPILED where it should have been REFUSED.
        // These two shapes are the ones the catch-all swallowed.
        let with_handler = "fn f() -> i64 {\n\
                              with handler { on Net(p) => resume(0) } {\n\
                                let a = marker_in_body(1)\n\
                                0\n\
                              }\n\
                            }\n";
        assert!(
            calls_in(with_handler).iter().any(|n| n == "marker_in_body"),
            "walk must descend into a `with handler` BODY"
        );

        let handler_arm = "fn f() -> i64 {\n\
                             with handler { on Net(p) => marker_in_arm(1) } {\n\
                               0\n\
                             }\n\
                           }\n";
        assert!(
            calls_in(handler_arm).iter().any(|n| n == "marker_in_arm"),
            "walk must descend into a handler ARM — an effect handler body is \
             ordinary code and can call anything"
        );
    }

    #[test]
    fn walk_expr_visits_the_root_and_nests_arbitrarily_deep_t46() {
        // The callback must see the root expression itself (a scan looking for a
        // top-level call would otherwise miss it), and nesting must not bottom
        // out early.
        let deep = "fn f() -> i64 {\n\
                      if outer(1) > 0 {\n\
                        match inner(2) { Ok(v) => arm_call(v)  Err(e) => 0 }\n\
                      } else {\n\
                        let xs = [elem_call(3)]\n\
                        0\n\
                      }\n\
                    }\n";
        let names = calls_in(deep);
        for want in ["outer", "inner", "arm_call", "elem_call"] {
            assert!(
                names.iter().any(|n| n == want),
                "missed `{want}` in {names:?}"
            );
        }

        let mut saw_root = false;
        let root = body_of("fn f() -> i64 { 1 }\n");
        ast::walk_expr(&root, &mut |e| {
            if std::ptr::eq(e, &root) {
                saw_root = true;
            }
        });
        assert!(
            saw_root,
            "walk_expr must call the visitor on the root itself"
        );
    }
}
