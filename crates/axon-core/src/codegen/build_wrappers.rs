//! Non-generic `#[inline(never)]` wrappers around inkwell's heavily-generic
//! `Builder::build_*` API.
//!
//! # Why this exists
//!
//! `codegen::builtins::declare_builtins` issues hundreds of `self.ir.builder
//! .build_*` calls.  Every one of inkwell's `build_*` methods is generic
//! (`build_int_add<T: IntMathValue>`, `build_alloca<T: BasicType>`, …), so
//! rustc monomorphizes the full generic body at *every* call site inside the
//! giant `declare_builtins` function.  That superlinear pile-up of inlined
//! monomorphizations is the dominant input to the pathologically-slow native
//! codegen build (`BUILD_DIAGNOSIS.md`, `CODEGEN_WRAPPER_PROTOTYPE.md`).
//!
//! The fix (proven in an isolated repro: −43% LLVM-IR, −36% RSS, ~1.7–3×
//! faster): route each generic call through a **non-generic** free function
//! that takes/returns *concrete* inkwell types and is marked
//! `#[inline(never)]`.  Then each generic inkwell instantiation is
//! monomorphized **exactly once** (inside the wrapper) instead of once per
//! call site, and `#[inline(never)]` forecloses the optimizer folding the
//! generic body back into the caller.
//!
//! A *trait* surface (the earlier `ir_inkwell.rs` shim) did NOT help because
//! its methods stayed generic / got inlined back, so the instantiation count
//! was unchanged.  These wrappers are structurally different: one concrete IR
//! shape each, `#[inline(never)]`, so `Copies = 1` per wrapper.
//!
//! Every function here MUST stay `#[inline(never)]` and MUST NOT be generic.
//! `.unwrap()` matches the existing `declare_builtins` call style (these are
//! infallible given a positioned builder).

use inkwell::basic_block::BasicBlock;
use inkwell::builder::Builder;
use inkwell::types::BasicTypeEnum;
use inkwell::values::{
    AggregateValueEnum, BasicMetadataValueEnum, BasicValueEnum, CallSiteValue, FloatValue,
    FunctionValue, IntValue, PhiValue, PointerValue, StructValue,
};
use inkwell::types::PointerType;
use inkwell::{FloatPredicate, IntPredicate};

// ── memory: alloca / load / store / gep ──────────────────────────────────────

#[inline(never)]
pub(crate) fn w_alloca<'ctx>(
    b: &Builder<'ctx>,
    ty: BasicTypeEnum<'ctx>,
    name: &str,
) -> PointerValue<'ctx> {
    b.build_alloca(ty, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_load<'ctx>(
    b: &Builder<'ctx>,
    pointee_ty: BasicTypeEnum<'ctx>,
    ptr: PointerValue<'ctx>,
    name: &str,
) -> BasicValueEnum<'ctx> {
    b.build_load(pointee_ty, ptr, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_store<'ctx>(
    b: &Builder<'ctx>,
    ptr: PointerValue<'ctx>,
    value: BasicValueEnum<'ctx>,
) {
    b.build_store(ptr, value).unwrap();
}

#[inline(never)]
pub(crate) fn w_struct_gep<'ctx>(
    b: &Builder<'ctx>,
    struct_ty: BasicTypeEnum<'ctx>,
    ptr: PointerValue<'ctx>,
    index: u32,
    name: &str,
) -> PointerValue<'ctx> {
    b.build_struct_gep(struct_ty, ptr, index, name).unwrap()
}

/// In-bounds-unaware GEP.  `unsafe` mirrors inkwell's `build_gep`.
///
/// # Safety
/// `indices` must be valid for `pointee_ty` at `ptr` (same contract as
/// `Builder::build_gep`).
#[inline(never)]
pub(crate) unsafe fn w_gep<'ctx>(
    b: &Builder<'ctx>,
    pointee_ty: BasicTypeEnum<'ctx>,
    ptr: PointerValue<'ctx>,
    indices: &[IntValue<'ctx>],
    name: &str,
) -> PointerValue<'ctx> {
    b.build_gep(pointee_ty, ptr, indices, name).unwrap()
}

// ── aggregates: extract / insert ──────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_extract_value<'ctx>(
    b: &Builder<'ctx>,
    agg: StructValue<'ctx>,
    index: u32,
    name: &str,
) -> BasicValueEnum<'ctx> {
    b.build_extract_value(agg, index, name).unwrap()
}

/// Returns `AggregateValueEnum` (matching inkwell's `build_insert_value().unwrap()`)
/// so existing call-site `.into_struct_value()` chains keep working.
#[inline(never)]
pub(crate) fn w_insert_value<'ctx>(
    b: &Builder<'ctx>,
    agg: StructValue<'ctx>,
    value: BasicValueEnum<'ctx>,
    index: u32,
    name: &str,
) -> AggregateValueEnum<'ctx> {
    b.build_insert_value(agg, value, index, name).unwrap()
}

// ── calls ─────────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_call<'ctx>(
    b: &Builder<'ctx>,
    function: FunctionValue<'ctx>,
    args: &[BasicMetadataValueEnum<'ctx>],
    name: &str,
) -> CallSiteValue<'ctx> {
    b.build_call(function, args, name).unwrap()
}

// ── casts ─────────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_pointer_cast<'ctx>(
    b: &Builder<'ctx>,
    from: PointerValue<'ctx>,
    to: PointerType<'ctx>,
    name: &str,
) -> PointerValue<'ctx> {
    b.build_pointer_cast(from, to, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_ptr_to_int<'ctx>(
    b: &Builder<'ctx>,
    ptr: PointerValue<'ctx>,
    int_type: inkwell::types::IntType<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_ptr_to_int(ptr, int_type, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_truncate<'ctx>(
    b: &Builder<'ctx>,
    value: IntValue<'ctx>,
    int_type: inkwell::types::IntType<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_truncate(value, int_type, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_z_extend<'ctx>(
    b: &Builder<'ctx>,
    value: IntValue<'ctx>,
    int_type: inkwell::types::IntType<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_z_extend(value, int_type, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_s_extend<'ctx>(
    b: &Builder<'ctx>,
    value: IntValue<'ctx>,
    int_type: inkwell::types::IntType<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_s_extend(value, int_type, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_signed_int_to_float<'ctx>(
    b: &Builder<'ctx>,
    int: IntValue<'ctx>,
    float_type: inkwell::types::FloatType<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_signed_int_to_float(int, float_type, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_float_to_signed_int<'ctx>(
    b: &Builder<'ctx>,
    float: FloatValue<'ctx>,
    int_type: inkwell::types::IntType<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_float_to_signed_int(float, int_type, name).unwrap()
}

// ── int arithmetic ────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_int_add<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_add(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_sub<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_sub(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_mul<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_mul(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_signed_rem<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_signed_rem(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_neg<'ctx>(
    b: &Builder<'ctx>,
    value: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_neg(value, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_and<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_and(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_or<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_or(l, r, name).unwrap()
}

// ── float arithmetic ──────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_float_mul<'ctx>(
    b: &Builder<'ctx>,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_mul(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_float_div<'ctx>(
    b: &Builder<'ctx>,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_div(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_float_sub<'ctx>(
    b: &Builder<'ctx>,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_sub(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_float_neg<'ctx>(
    b: &Builder<'ctx>,
    value: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_neg(value, name).unwrap()
}

// ── comparisons ───────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_int_compare<'ctx>(
    b: &Builder<'ctx>,
    op: IntPredicate,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_compare(op, l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_float_compare<'ctx>(
    b: &Builder<'ctx>,
    op: FloatPredicate,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_float_compare(op, l, r, name).unwrap()
}

// ── select ────────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_select<'ctx>(
    b: &Builder<'ctx>,
    condition: IntValue<'ctx>,
    then: BasicValueEnum<'ctx>,
    else_: BasicValueEnum<'ctx>,
    name: &str,
) -> BasicValueEnum<'ctx> {
    b.build_select(condition, then, else_, name).unwrap()
}

// ── control flow ──────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_br<'ctx>(b: &Builder<'ctx>, dest: BasicBlock<'ctx>) {
    b.build_unconditional_branch(dest).unwrap();
}

#[inline(never)]
pub(crate) fn w_cond_br<'ctx>(
    b: &Builder<'ctx>,
    cond: IntValue<'ctx>,
    then_block: BasicBlock<'ctx>,
    else_block: BasicBlock<'ctx>,
) {
    b.build_conditional_branch(cond, then_block, else_block)
        .unwrap();
}

#[inline(never)]
pub(crate) fn w_ret_void(b: &Builder<'_>) {
    b.build_return(None).unwrap();
}

#[inline(never)]
pub(crate) fn w_ret<'ctx>(b: &Builder<'ctx>, value: BasicValueEnum<'ctx>) {
    b.build_return(Some(&value)).unwrap();
}

#[inline(never)]
pub(crate) fn w_unreachable(b: &Builder<'_>) {
    b.build_unreachable().unwrap();
}

// ── phi ───────────────────────────────────────────────────────────────────────

/// Returns the `PhiValue` so call sites can still call `.add_incoming(..)` /
/// `.as_basic_value()` exactly as before.
#[inline(never)]
pub(crate) fn w_phi<'ctx>(
    b: &Builder<'ctx>,
    ty: BasicTypeEnum<'ctx>,
    name: &str,
) -> PhiValue<'ctx> {
    b.build_phi(ty, name).unwrap()
}

// ── float add ─────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_float_add<'ctx>(
    b: &Builder<'ctx>,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_add(l, r, name).unwrap()
}

// ── float rem ─────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_float_rem<'ctx>(
    b: &Builder<'ctx>,
    l: FloatValue<'ctx>,
    r: FloatValue<'ctx>,
    name: &str,
) -> FloatValue<'ctx> {
    b.build_float_rem(l, r, name).unwrap()
}

// ── integer division ──────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_int_signed_div<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_signed_div(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_unsigned_div<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_unsigned_div(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_int_unsigned_rem<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_int_unsigned_rem(l, r, name).unwrap()
}

// ── shift / bitwise ───────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_left_shift<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_left_shift(l, r, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_right_shift<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    asr: bool,
    name: &str,
) -> IntValue<'ctx> {
    b.build_right_shift(l, r, asr, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_not<'ctx>(
    b: &Builder<'ctx>,
    value: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_not(value, name).unwrap()
}

#[inline(never)]
pub(crate) fn w_xor<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    b.build_xor(l, r, name).unwrap()
}

// ── string ────────────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_global_string_ptr<'ctx>(
    b: &Builder<'ctx>,
    s: &str,
    name: &str,
) -> PointerValue<'ctx> {
    b.build_global_string_ptr(s, name).unwrap().as_pointer_value()
}

// ── switch ────────────────────────────────────────────────────────────────────

/// Non-generic wrapper for `build_switch`.  `cases` is a slice of
/// `(IntValue, BasicBlock)` pairs — same shape as the caller passes.
/// Returns `()` because `build_switch` does not return a value.
#[inline(never)]
pub(crate) fn w_switch<'ctx>(
    b: &Builder<'ctx>,
    int_val: IntValue<'ctx>,
    default_bb: BasicBlock<'ctx>,
    cases: &[(IntValue<'ctx>, BasicBlock<'ctx>)],
) {
    b.build_switch(int_val, default_bb, cases).unwrap();
}

// ── indirect call ─────────────────────────────────────────────────────────────

#[inline(never)]
pub(crate) fn w_indirect_call<'ctx>(
    b: &Builder<'ctx>,
    fn_type: inkwell::types::FunctionType<'ctx>,
    fn_ptr: PointerValue<'ctx>,
    args: &[BasicMetadataValueEnum<'ctx>],
    name: &str,
) -> CallSiteValue<'ctx> {
    b.build_indirect_call(fn_type, fn_ptr, args, name).unwrap()
}
