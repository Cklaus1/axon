//! IR shim — the *target* surface for the inkwell decoupling refactor.
//!
//! # Why this file exists
//!
//! The codegen-feature build of `axon-core` is unbuildable in reasonable
//! time on this codebase (see `ROADMAP.md` §7.5).  Empirically:
//!   * Pre-decomposition: 9h+ never finished, OOM-trending.
//!   * Phase 2 file split: 5h+ stalled mid-codegen.
//!   * Phase 3 method decomposition: 5h+ stalled, same pattern.
//!   * Nightly `-Z parallel-frontend`: 4h 35m, 1 of 8 threads working.
//!
//! Diagnosis: inkwell's heavily-generic API (`Builder<'ctx>`,
//! `IntValue<'ctx>`, `BasicTypeEnum<'ctx>`, etc.) makes every codegen
//! call site a fresh trait-monomorphization site.  ~5,000 such sites
//! exist across the 9 codegen modules; the trait solver cannot bound
//! the work.
//!
//! The fix is to wrap inkwell behind a *monomorphic* IR trait.  Codegen
//! calls become `self.ir.add(a, b)` returning `IRValue` (a plain
//! `u32` arena handle) instead of `self.builder.build_int_add(iv1,
//! iv2, "name")` returning `IntValue<'ctx>` (heavily generic).  The
//! inkwell trait machinery is then bounded to a single `impl IR for
//! InkwellBackend<'ctx>` block.
//!
//! # What this file is and isn't
//!
//! **Is**: the trait declaration + handle types + a catalog of every
//! method codegen needs.  Compiles without inkwell.  Discoverable by
//! `rustfmt --check`.  Documents scope.
//!
//! **Isn't**: an implementation.  No `impl IR for InkwellBackend`
//! exists in this file.  Migrating call sites from `self.builder.*` to
//! `self.ir.*` is the *next* phase, batched per file (asi.rs first —
//! smallest cross-section — then types.rs, option_result.rs,
//! match_pat.rs, output.rs, expr.rs, builtins.rs).
//!
//! # Method count
//!
//! Cataloged from `grep -hoE "self\.(builder|context|module)\.[a-z_]+"
//! crates/axon-core/src/codegen/*.rs | sort -u`:
//!   * 46 unique `builder.build_*` methods (all listed below)
//!   * ~10 `context.*_type` methods (primitive + struct + array)
//!   * ~5  `module.*` methods (add/get function, add_global)
//!   * Total: ~60 methods.  Manageable.

#![allow(dead_code)] // Trait is unused until the impl phase.

// ── Opaque handle types ─────────────────────────────────────────────────────
//
// All four are `Copy + Clone + Eq + Hash` so they round-trip through
// HashMaps / Vecs without lifetime entanglement.  The arena lives inside
// the impl backend (`InkwellBackend<'ctx>`); handles are just indices.
//
// The lifetime erasure pattern is **arena-with-handles** (cranelift uses
// the same approach internally).  *Not* `Arc<Context>` — that requires
// unsafe `'static` transmutes whose safety is hard to maintain.

/// Handle to an LLVM SSA value (int / float / pointer / struct / aggregate).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct IRValue(pub u32);

/// Handle to an LLVM type.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct IRType(pub u32);

/// Handle to a basic block.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct IRBlock(pub u32);

/// Handle to an LLVM function (the symbolic definition + linkage).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct IRFunction(pub u32);

/// Handle to a global value (string constant, vtable, etc.).
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub struct IRGlobal(pub u32);

// ── Helper enums (mirror inkwell's, but Copy + monomorphic) ────────────────

/// Mirror of `inkwell::IntPredicate`.  Stored as a tag, not a generic
/// type, so the monomorphizer doesn't have to thread it through.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum IRIntPred {
    Eq, Ne, Slt, Sle, Sgt, Sge, Ult, Ule, Ugt, Uge,
}

/// Mirror of `inkwell::FloatPredicate`.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum IRFloatPred {
    Oeq, Ogt, Oge, Olt, Ole, One, Ord, Uno,
    Ueq, Ugt, Uge, Ult, Ule, Une,
}

/// Mirror of `inkwell::AddressSpace::default()` is the only space we use.
/// If we ever need others, extend this enum.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash, Default)]
pub enum IRAddrSpace {
    #[default]
    Default,
}

/// Mirror of `inkwell::OptimizationLevel`.
#[derive(Debug, Copy, Clone, Eq, PartialEq, Hash)]
pub enum IROptLevel { None, Less, Default, Aggressive }

// ── The IR trait ────────────────────────────────────────────────────────────
//
// Codegen calls go through this trait.  The single impl
// `InkwellBackend<'ctx> : IR` is where the inkwell trait machinery is
// bounded.  Any other backend (cranelift, MLIR, custom) implements the
// same trait without modifying codegen.
//
// Method names mirror inkwell's `builder.build_*` convention so the
// migration diff is mostly mechanical:
//   `self.builder.build_int_add(a, b, "name").unwrap()`  →
//   `self.ir.iadd(a, b)`
// (The `"name"` parameter is dropped; the impl can synthesize names if
// the LLVM backend wants them.  Names don't affect semantics.)

pub trait IR {
    // ── Type construction ────────────────────────────────────────────────
    fn t_void(&mut self) -> IRType;
    fn t_bool(&mut self) -> IRType;
    fn t_i8(&mut self) -> IRType;
    fn t_i16(&mut self) -> IRType;
    fn t_i32(&mut self) -> IRType;
    fn t_i64(&mut self) -> IRType;
    fn t_u8(&mut self) -> IRType;
    fn t_u16(&mut self) -> IRType;
    fn t_u32(&mut self) -> IRType;
    fn t_u64(&mut self) -> IRType;
    fn t_f32(&mut self) -> IRType;
    fn t_f64(&mut self) -> IRType;
    /// Opaque pointer (LLVM 17 has no typed pointers).
    fn t_ptr(&mut self) -> IRType;
    fn t_array(&mut self, elem: IRType, n: u32) -> IRType;
    fn t_struct(&mut self, fields: &[IRType], packed: bool) -> IRType;
    fn t_named_struct(&mut self, name: &str, fields: &[IRType], packed: bool) -> IRType;
    /// Function type for declarations and indirect calls.
    fn t_fn(&mut self, params: &[IRType], ret: Option<IRType>, variadic: bool) -> IRType;

    // ── Type queries (for the few places codegen does runtime type checks) ──
    fn ty_of(&self, val: IRValue) -> IRType;
    fn t_get_struct_field(&self, ty: IRType, idx: u32) -> Option<IRType>;
    fn t_get_named_struct(&self, name: &str) -> Option<IRType>;

    // ── Constants ────────────────────────────────────────────────────────
    fn const_i64(&mut self, v: i64) -> IRValue;
    fn const_i32(&mut self, v: i32) -> IRValue;
    fn const_i8(&mut self, v: u8) -> IRValue;
    fn const_bool(&mut self, v: bool) -> IRValue;
    fn const_f64(&mut self, v: f64) -> IRValue;
    fn const_zero(&mut self, ty: IRType) -> IRValue;
    fn const_undef(&mut self, ty: IRType) -> IRValue;
    fn const_struct(&mut self, fields: &[IRValue], packed: bool) -> IRValue;
    fn const_array(&mut self, elem_ty: IRType, elems: &[IRValue]) -> IRValue;
    fn const_string(&mut self, s: &str, null_terminate: bool) -> IRValue;
    /// Insert a global string constant; returns the global's address.
    fn global_string(&mut self, s: &str, name: &str) -> IRGlobal;

    // ── Function / module declarations ───────────────────────────────────
    fn add_function(&mut self, name: &str, ty: IRType) -> IRFunction;
    fn get_function(&self, name: &str) -> Option<IRFunction>;
    fn add_global(&mut self, name: &str, ty: IRType, init: Option<IRValue>) -> IRGlobal;
    fn fn_param(&self, f: IRFunction, idx: u32) -> IRValue;
    fn fn_as_value(&self, f: IRFunction) -> IRValue;
    fn global_as_value(&self, g: IRGlobal) -> IRValue;

    // ── Basic blocks ──────────────────────────────────────────────────────
    fn append_block(&mut self, f: IRFunction, name: &str) -> IRBlock;
    fn position_at_end(&mut self, b: IRBlock);
    fn current_block(&self) -> Option<IRBlock>;
    fn block_terminated(&self, b: IRBlock) -> bool;
    fn block_parent_fn(&self, b: IRBlock) -> IRFunction;

    // ── Memory operations ────────────────────────────────────────────────
    fn alloca(&mut self, ty: IRType) -> IRValue;
    fn load(&mut self, ty: IRType, ptr: IRValue) -> IRValue;
    fn store(&mut self, ptr: IRValue, val: IRValue);
    /// `getelementptr` for struct field access.  Returns a pointer to
    /// the field at `idx`.
    fn struct_gep(&mut self, ty: IRType, ptr: IRValue, idx: u32) -> IRValue;
    /// `getelementptr` for array indexing.
    fn array_gep(&mut self, ty: IRType, ptr: IRValue, idx: IRValue) -> IRValue;
    fn ptr_cast(&mut self, ptr: IRValue, target_ty: IRType) -> IRValue;
    fn ptr_to_int(&mut self, ptr: IRValue, int_ty: IRType) -> IRValue;

    // ── Aggregate value ops ──────────────────────────────────────────────
    fn extract_value(&mut self, agg: IRValue, idx: u32) -> IRValue;
    fn insert_value(&mut self, agg: IRValue, val: IRValue, idx: u32) -> IRValue;

    // ── Integer arithmetic ───────────────────────────────────────────────
    fn iadd(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn isub(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn imul(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn idiv_signed(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn idiv_unsigned(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn irem_signed(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn irem_unsigned(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn ineg(&mut self, a: IRValue) -> IRValue;
    fn icmp(&mut self, pred: IRIntPred, a: IRValue, b: IRValue) -> IRValue;
    fn shl(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn shr(&mut self, a: IRValue, b: IRValue, signed: bool) -> IRValue;
    fn iand(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn ior(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn ixor(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn inot(&mut self, a: IRValue) -> IRValue;

    // ── Integer width conversions ───────────────────────────────────────
    fn int_truncate(&mut self, a: IRValue, target_ty: IRType) -> IRValue;
    fn int_zext(&mut self, a: IRValue, target_ty: IRType) -> IRValue;
    fn int_sext(&mut self, a: IRValue, target_ty: IRType) -> IRValue;

    // ── Float arithmetic ─────────────────────────────────────────────────
    fn fadd(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn fsub(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn fmul(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn fdiv(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn frem(&mut self, a: IRValue, b: IRValue) -> IRValue;
    fn fneg(&mut self, a: IRValue) -> IRValue;
    fn fcmp(&mut self, pred: IRFloatPred, a: IRValue, b: IRValue) -> IRValue;

    // ── Float ↔ int conversions ──────────────────────────────────────────
    fn sitof(&mut self, a: IRValue, target_ty: IRType) -> IRValue;
    fn ftosi(&mut self, a: IRValue, target_ty: IRType) -> IRValue;

    // ── Control flow ─────────────────────────────────────────────────────
    fn br(&mut self, target: IRBlock);
    fn cond_br(&mut self, cond: IRValue, then_b: IRBlock, else_b: IRBlock);
    fn switch(&mut self, scrutinee: IRValue, default: IRBlock, cases: &[(IRValue, IRBlock)]);
    fn ret_void(&mut self);
    fn ret(&mut self, val: IRValue);
    fn unreachable(&mut self);
    fn select(&mut self, cond: IRValue, then_v: IRValue, else_v: IRValue) -> IRValue;
    /// Phi node: returns a handle that the impl populates as `add_incoming`
    /// is called.  Use `phi_add_incoming` for each predecessor block.
    fn phi(&mut self, ty: IRType) -> IRValue;
    fn phi_add_incoming(&mut self, phi: IRValue, val: IRValue, b: IRBlock);

    // ── Calls ────────────────────────────────────────────────────────────
    /// Direct call of a known function.  `args` may be empty.
    fn call(&mut self, callee: IRFunction, args: &[IRValue]) -> Option<IRValue>;
    /// Indirect call of a function pointer with explicit signature.
    fn call_indirect(&mut self, sig: IRType, fn_ptr: IRValue, args: &[IRValue]) -> Option<IRValue>;

    // ── Output / serialization (for the `output` module) ─────────────────
    fn verify(&self) -> Result<(), String>;
    fn write_ir_text(&self, path: &str) -> Result<(), String>;
    fn emit_bitcode(&self) -> Vec<u8>;
}

// ── Migration plan (informative) ────────────────────────────────────────────
//
// Phase IR.1 — define this trait (this file).  ✅ when committed.
// Phase IR.2 — implement `InkwellBackend<'ctx>: IR` in a new
//              `ir_inkwell.rs`.  All inkwell calls bounded to ONE impl block.
// Phase IR.3 — migrate codegen modules to use `self.ir.*` instead of
//              `self.builder.*` / `self.context.*` / `self.module.*`.
//              Order (smallest first, validate after each):
//                1. asi.rs           (~390 LoC, ~30 sites)
//                2. option_result.rs (~220 LoC, ~30 sites)
//                3. types.rs         (~210 LoC, ~20 sites)
//                4. output.rs        (~120 LoC, ~10 sites)
//                5. match_pat.rs     (~490 LoC, ~80 sites)
//                6. expr.rs          (~1760 LoC, ~700 sites)
//                7. builtins.rs      (~3960 LoC, ~3000 sites)
//                8. mod.rs           (~980 LoC, ~200 sites)
// Phase IR.4 — replace `Codegen<'ctx>::context/module/builder` fields
//              with a single `ir: InkwellBackend<'ctx>` field.
// Phase IR.5 — verify cargo build -p axon-core succeeds in <30 min on
//              canonical hardware.  If yes, the shim is the answer.  If
//              not, escalate to MLIR / cranelift backend.
//
// Each Phase IR.3 batch is bounded by file size and validated by a fresh
// cargo build.  Phase 2 + Phase 3 work (already shipped) is what makes
// these batches small enough to validate one at a time.
