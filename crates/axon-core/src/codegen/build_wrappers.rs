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
    // SATURATING float→signed-int, matching the interpreter (Rust `as i64`,
    // saturating since Rust 1.45) and avoiding LLVM `fptosi`'s UNDEFINED result
    // for out-of-range / NaN inputs (which produced garbage like i64::MIN for
    // 1e30, NaN, +Inf — a silent-wrong-result, ARCHITECTURE INVARIANTS I-9).
    //
    // Rust's semantics: NaN → 0; value ≥ int::MAX → int::MAX; value ≤ int::MIN →
    // int::MIN; otherwise the truncated `fptosi`. We reproduce it with float
    // compares + selects (no new basic blocks needed, so this stays a drop-in
    // wrapper usable from `declare_builtins`). The raw fptosi is only ever
    // evaluated for in-range inputs; the selects override the out-of-range and
    // NaN cases, so the poison value can't be observed.
    let raw = b.build_float_to_signed_int(float, int_type, name).unwrap();
    let bits = int_type.get_bit_width();
    // Only i64 (and narrower signed ints) are produced here; guard generically
    // via the type's min/max as f64 thresholds.
    let f_ty = float.get_type();
    let (min_i, max_i) = match bits {
        64 => (i64::MIN, i64::MAX),
        32 => (i32::MIN as i64, i32::MAX as i64),
        16 => (i16::MIN as i64, i16::MAX as i64),
        8 => (i8::MIN as i64, i8::MAX as i64),
        _ => return raw, // unusual width — leave as-is
    };
    // Thresholds as f64. (int::MAX isn't exactly representable in f64 for i64,
    // but `>=` against the rounded value is the conservative bound Rust uses.)
    let max_f = f_ty.const_float(max_i as f64);
    let min_f = f_ty.const_float(min_i as f64);
    let max_c = int_type.const_int(max_i as u64, true);
    let min_c = int_type.const_int(min_i as u64, true);
    let zero_c = int_type.const_zero();

    // ord: float == float is false iff NaN — use UNO (unordered) to detect NaN.
    let is_nan = b
        .build_float_compare(inkwell::FloatPredicate::UNO, float, float, "fti_isnan")
        .unwrap();
    let ge_max = b
        .build_float_compare(inkwell::FloatPredicate::OGE, float, max_f, "fti_gemax")
        .unwrap();
    let le_min = b
        .build_float_compare(inkwell::FloatPredicate::OLE, float, min_f, "fti_lemin")
        .unwrap();

    // raw, then clamp high, then clamp low, then NaN→0 (innermost wins last).
    let clamped_hi = b.build_select(ge_max, max_c, raw, "fti_hi").unwrap().into_int_value();
    let clamped_lo = b.build_select(le_min, min_c, clamped_hi, "fti_lo").unwrap().into_int_value();
    b.build_select(is_nan, zero_c, clamped_lo, "fti_sat").unwrap().into_int_value()
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

/// SATURATING signed add — `a + b` clamped to [int::MIN, int::MAX] on overflow,
/// matching the interpreter's `saturating_add` (used by `arr_sum_i64`'s
/// accumulator). Avoids the raw wrapping add (which silently produced a wrong
/// total on overflow — I-9). Overflow detection by sign: signed addition
/// overflows iff both operands share a sign AND the result's sign differs from
/// them. On overflow the saturated value is int::MAX when the operands were
/// positive, int::MIN when negative. Pure compares + selects (no new blocks).
#[inline(never)]
pub(crate) fn w_int_add_sat<'ctx>(
    b: &Builder<'ctx>,
    l: IntValue<'ctx>,
    r: IntValue<'ctx>,
    name: &str,
) -> IntValue<'ctx> {
    let ity = l.get_type();
    let sum = b.build_int_add(l, r, name).unwrap();
    let zero = ity.const_zero();
    // sign bits
    let l_neg = b.build_int_compare(inkwell::IntPredicate::SLT, l, zero, "sat_lneg").unwrap();
    let r_neg = b.build_int_compare(inkwell::IntPredicate::SLT, r, zero, "sat_rneg").unwrap();
    let s_neg = b.build_int_compare(inkwell::IntPredicate::SLT, sum, zero, "sat_sneg").unwrap();
    // operands same sign?  (l_neg == r_neg)
    let same_sign = b.build_int_compare(inkwell::IntPredicate::EQ, l_neg, r_neg, "sat_same").unwrap();
    // result sign differs from operands? (s_neg != l_neg)
    let sign_flip = b.build_int_compare(inkwell::IntPredicate::NE, s_neg, l_neg, "sat_flip").unwrap();
    let overflow = b.build_and(same_sign, sign_flip, "sat_ovf").unwrap();
    // saturate target: operands positive → MAX, negative → MIN.
    let bits = ity.get_bit_width();
    let (min_v, max_v): (u64, u64) = match bits {
        64 => (i64::MIN as u64, i64::MAX as u64),
        32 => (i32::MIN as i64 as u64, i32::MAX as u64),
        _ => return sum, // only the i64/i32 accumulators use this
    };
    let max_c = ity.const_int(max_v, true);
    let min_c = ity.const_int(min_v, true);
    let sat_target = b.build_select(l_neg, min_c, max_c, "sat_tgt").unwrap().into_int_value();
    b.build_select(overflow, sat_target, sum, "sat_res").unwrap().into_int_value()
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
