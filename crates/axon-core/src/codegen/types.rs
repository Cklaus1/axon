//! Axon semantic type → LLVM `BasicTypeEnum` lowering.
//!
//! Phase 2 of the §7.5 module split: extracts the three type-mapping helpers
//! from the monolithic `codegen.rs`.  All three are methods on
//! `super::Codegen<'ctx>`, so this file declares its own impl block on the
//! parent struct.  Visibility:
//! - `llvm_type`     — `pub` (re-exported public API; many callers)
//! - `llvm_sizeof`   — `pub(super)` (called from mod.rs internally)
//! - `llvm_align_of` — `pub(super)` (only called from `llvm_sizeof` today but
//!   kept reachable from mod.rs in case of future use)
//!
//! No state is mutated by any of these methods — they are pure functions of
//! `&self` and the input `Type`.

use inkwell::types::BasicTypeEnum;
use inkwell::AddressSpace;

use crate::types::Type;

/// What to assume when a type's size cannot be computed, for a UNION payload.
///
/// The rule this encodes: over-sizing a payload wastes stack, under-sizing
/// corrupts memory. Those are not comparable costs, so every unknown rounds UP.
///
/// The bug that motivated it sized every struct at 8 bytes, which silently
/// truncated `Result<BigStruct, str>` and read adjacent memory. The first fix
/// replaced that with `None` — and both payload call sites did `.unwrap_or(0)`,
/// which would have sized the buffer at ZERO, trading a truncation for a worse
/// one. 64 bytes is past any struct in the tree today, and a struct whose size
/// genuinely cannot be computed is E0910-refused before reaching codegen
/// (generic instantiations are, measured).
pub(super) const UNKNOWN_TYPE_PAYLOAD_SIZE: u64 = 64;

impl<'ctx> super::Codegen<'ctx> {
    /// Convert an Axon semantic `Type` into an LLVM `BasicTypeEnum`.
    /// Returns `None` for `Unit` (void) and unresolved/unknown types.
    pub fn llvm_type(&self, ty: &Type) -> Option<BasicTypeEnum<'ctx>> {
        match ty {
            Type::I8 | Type::U8 => Some(self.ir.context.i8_type().into()),
            Type::I16 | Type::U16 => Some(self.ir.context.i16_type().into()),
            Type::I32 | Type::U32 => Some(self.ir.context.i32_type().into()),
            Type::I64 | Type::U64 => Some(self.ir.context.i64_type().into()),
            Type::F32 => Some(self.ir.context.f32_type().into()),
            Type::F64 => Some(self.ir.context.f64_type().into()),
            // R21 — Decimal is an i128 mantissa (LLVM has native i128).
            Type::Decimal => Some(self.ir.context.i128_type().into()),
            Type::Bool => Some(self.ir.context.bool_type().into()),

            // Str → struct { i64, ptr }
            Type::Str => {
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let str_ty = self
                    .ir
                    .context
                    .struct_type(&[i64_ty.into(), ptr_ty.into()], /*packed=*/ false);
                Some(str_ty.into())
            }

            // Unit → no LLVM basic type (void)
            Type::Unit => None,

            // Option<T> → struct { i1, T }
            Type::Option(inner) => {
                let tag = self.ir.context.bool_type();
                if let Some(inner_llvm) = self.llvm_type(inner) {
                    let opt_ty = self
                        .ir
                        .context
                        .struct_type(&[tag.into(), inner_llvm], false);
                    Some(opt_ty.into())
                } else {
                    // Option<Unit> is just a bool
                    Some(tag.into())
                }
            }

            // Result<T,E> → struct { i1, [max(sizeof T, sizeof E) x i8] }
            Type::Result(ok_ty, err_ty) => {
                let tag = self.ir.context.bool_type();
                // An unsizable side rounds UP — see UNKNOWN_TYPE_PAYLOAD_SIZE.
                // This was `unwrap_or(0)`, which for `Result<Dict, Dict>` gives
                // `max(0, 0).max(1)` = a ONE-BYTE payload.
                let ok_size = self.llvm_sizeof(ok_ty).unwrap_or(UNKNOWN_TYPE_PAYLOAD_SIZE);
                let err_size = self
                    .llvm_sizeof(err_ty)
                    .unwrap_or(UNKNOWN_TYPE_PAYLOAD_SIZE);
                let payload_size = ok_size.max(err_size).max(1);
                let i8_ty = self.ir.context.i8_type();
                let payload = i8_ty.array_type(payload_size as u32);
                let result_ty = self
                    .ir
                    .context
                    .struct_type(&[tag.into(), payload.into()], false);
                Some(result_ty.into())
            }

            // Slice<T> → struct { i64, ptr }
            Type::Slice(_inner) => {
                let i64_ty = self.ir.context.i64_type();
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                let slice_ty = self
                    .ir
                    .context
                    .struct_type(&[i64_ty.into(), ptr_ty.into()], false);
                Some(slice_ty.into())
            }

            // Tuple → struct { T0, T1, ... }
            Type::Tuple(fields) => {
                // `filter_map` here silently DROPPED any element with no LLVM
                // lowering (`Dict`), shrinking the tuple while every consumer
                // still indexes it by the SOURCE position. `(11, dict_new(), 22)`
                // built a 2-field struct, so `t.2` GEP'd out of range and
                // panicked the codegen worker on a bare `unwrap()`; had the
                // dropped element sat last, the shifted indices would have
                // stayed in range and read the WRONG element instead. Same bug
                // as `declare_types` (structs) — a tuple with an un-lowerable
                // element has no layout, so refuse it and let the caller's
                // `None` path report E0910.
                let mut field_tys: Vec<BasicTypeEnum<'ctx>> = Vec::with_capacity(fields.len());
                for f in fields {
                    field_tys.push(self.llvm_type(f)?);
                }
                let tuple_ty = self.ir.context.struct_type(&field_tys, false);
                Some(tuple_ty.into())
            }

            // Fn<params, ret> → opaque pointer (function pointers in LLVM 17
            // use the opaque pointer representation; typed fn pointers are gone).
            // A function value (closure) is a `{ fn_ptr, env_ptr }` fat pointer
            // — see `emit_lambda` (expr.rs), which always builds a 2-field
            // struct (env_ptr is null for capture-free lambdas). A `(T)->U`
            // PARAMETER must lower to the SAME struct, or passing a closure to a
            // higher-order fn fails IR verification: the call supplies
            // `{ptr,ptr}` while a single-`ptr` param expects one pointer
            // (BUG_HUNT #41). The interpreter treats a closure as a fat value
            // (fn + captured env), so this is the faithful lowering.
            Type::Fn(_, _) => {
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                Some(
                    self.ir
                        .context
                        .struct_type(&[ptr_ty.into(), ptr_ty.into()], false)
                        .into(),
                )
            }

            // Named struct — look up the named struct in the LLVM module.
            Type::Struct(name) => self.ir.module.get_struct_type(name).map(|s| s.into()),

            // Enum — look up by name with "_enum" suffix convention.
            Type::Enum(name) => {
                let mangled = format!("{name}_enum");
                self.ir.module.get_struct_type(&mangled).map(|s| s.into())
            }

            // R1c: a `Dict` is an opaque runtime handle (i8*), like a channel —
            // the dynamically-typed map lives behind `__axon_dict_*` externs.
            // The builtin-sig parser lands "Dict" as `Deferred("Dict")`.
            Type::Deferred(name) if name == "Dict" => Some(
                self.ir
                    .context
                    .i8_type()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // R13: a native FFI `Handle` is the frozen `{i64 tag, i64 payload}`
            // layout (I-6). `tag` = nominal id, `payload` = slab index — NEVER a
            // raw pointer. This matches `axon-rt::gfx_mock_ffi::AxonHandle`
            // (repr(C) `{i64,i64}`), so a handle passes/returns identically
            // across the C ABI on x86-64/aarch64 SysV (two integer registers).
            Type::Deferred(name) if crate::native::parse_handle_key(name).is_some() => {
                let i64_ty = self.ir.context.i64_type();
                Some(
                    self.ir
                        .context
                        .struct_type(&[i64_ty.into(), i64_ty.into()], false)
                        .into(),
                )
            }
            // Unresolved — skip
            Type::Unknown | Type::Var(_) | Type::Deferred(_) => None,
            // TypeParam should be eliminated by monomorphization.
            Type::TypeParam(_) => None,
            // DynTrait → fat pointer { ptr data, ptr vtable }
            Type::DynTrait(_) => {
                let ptr_ty = self.ir.context.i8_type().ptr_type(AddressSpace::default());
                Some(
                    self.ir
                        .context
                        .struct_type(&[ptr_ty.into(), ptr_ty.into()], false)
                        .into(),
                )
            }
            // Chan<T> → opaque pointer to axon-rt channel object
            Type::Chan(_) => Some(
                self.ir
                    .context
                    .i8_type()
                    .ptr_type(AddressSpace::default())
                    .into(),
            ),
            // Uncertain<T> → struct { T value, f64 confidence, i64 source_tag }
            // V1 simplification: omits `alternatives` and `interval` slots from
            // the full PRD (AI_Language_Plan.md lines 1360-1410). Layer-1 only.
            Type::Uncertain(inner) => {
                let inner_llvm = self.llvm_type(inner)?;
                let f64_ty = self.ir.context.f64_type();
                let i64_ty = self.ir.context.i64_type();
                Some(
                    self.ir
                        .context
                        .struct_type(&[inner_llvm, f64_ty.into(), i64_ty.into()], false)
                        .into(),
                )
            }
            // R17 HAL: `*T` → opaque LLVM `ptr` (LLVM 17 typed-pointer-free model).
            // The element type is tracked at load/store call sites, not in the pointer.
            Type::RawPtr(_) => Some(
                self.ir
                    .context
                    .i8_type()
                    .ptr_type(inkwell::AddressSpace::default())
                    .into(),
            ),
            // R17 HAL: `never` → no BasicTypeEnum (void; functions returning never
            // are emitted as LLVM void functions and marked noreturn by the caller).
            Type::Never => None,
            // Temporal<T> → struct { T value, f64 confidence, i64 horizon_ms,
            //                        f64 decay, i64 valid_until_ms }
            // V1 monomorphisation on T = i64 / f64 (PRD lines 1411-1467).
            Type::Temporal(inner) => {
                let inner_llvm = self.llvm_type(inner)?;
                let f64_ty = self.ir.context.f64_type();
                let i64_ty = self.ir.context.i64_type();
                Some(
                    self.ir
                        .context
                        .struct_type(
                            &[
                                inner_llvm,
                                f64_ty.into(),
                                i64_ty.into(),
                                f64_ty.into(),
                                i64_ty.into(),
                            ],
                            false,
                        )
                        .into(),
                )
            }
        }
    }

    /// Heuristic byte-size of a type (used for Result payload sizing).
    pub(super) fn llvm_sizeof(&self, ty: &Type) -> Option<u64> {
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
                fields
                    .iter()
                    .map(|f| self.llvm_sizeof(f).unwrap_or(0))
                    .sum(),
            ),
            // A struct's REAL size, laid out with C/LLVM padding rules.
            //
            // This was `Some(8)`, commented "conservative". For a union payload
            // it is the opposite of conservative: `Result<T, E>` sizes its
            // buffer as `max(sizeof T, sizeof E)`, so under-reporting T does not
            // waste space, it TRUNCATES. `Result<Quorum, str>` — an 8-field
            // struct — was sized by `str` at 16 bytes, and everything past byte
            // 16 was lost:
            //
            //     interp: 11 22 33 44
            //     native: 11 22 0 4419688      <- field 3 zero, field 4 garbage
            //
            // Silent wrong answers, not a crash, in a completely ordinary
            // pattern; `examples/stdlib/replicated.ax` hit it and its quorum
            // stopped committing and started accepting stale writes.
            //
            // Recursion terminates because Axon structs hold no by-value cycles
            // (a recursive type needs indirection, which is a pointer here).
            Type::Struct(name) => {
                let Some(fields) = self.struct_field_sem_types.get(name) else {
                    return Some(UNKNOWN_TYPE_PAYLOAD_SIZE);
                };
                let mut off: u64 = 0;
                let mut max_align: u64 = 1;
                for f in fields {
                    let a = self.llvm_align_of(f);
                    // An unknown field errs LARGE (8), never small: over-sizing a
                    // payload wastes stack, under-sizing corrupts memory.
                    let sz = self.llvm_sizeof(f).unwrap_or(8);
                    max_align = max_align.max(a);
                    off = off.div_ceil(a) * a;
                    off += sz;
                }
                Some(off.div_ceil(max_align) * max_align)
            }
            // An enum is a tag plus the largest variant, padded to the widest
            // field's alignment — the same reasoning, and the same direction on
            // anything unknown.
            Type::Enum(name) => {
                let Some(variants) = self.enum_variants.get(name) else {
                    return Some(UNKNOWN_TYPE_PAYLOAD_SIZE);
                };
                let mut widest: u64 = 0;
                let mut max_align: u64 = 8;
                for (_, _, field_tys) in variants {
                    let mut off: u64 = 0;
                    for f in field_tys {
                        let a = self.llvm_align_of(f);
                        let sz = self.llvm_sizeof(f).unwrap_or(8);
                        max_align = max_align.max(a);
                        off = off.div_ceil(a) * a;
                        off += sz;
                    }
                    widest = widest.max(off);
                }
                Some((max_align + widest).div_ceil(max_align) * max_align)
            }
            Type::Unit => Some(0),
            // Uncertain<T> = T value + f64 confidence + i64 source_tag → 24 bytes for T = i64
            Type::Uncertain(inner) => {
                let inner_size = self.llvm_sizeof(inner).unwrap_or(8);
                Some(inner_size + 8 + 8)
            }
            // Temporal<T> = T value + f64 confidence + i64 horizon + f64 decay + i64 valid_until
            Type::Temporal(inner) => {
                let inner_size = self.llvm_sizeof(inner).unwrap_or(8);
                Some(inner_size + 8 + 8 + 8 + 8)
            }
            Type::RawPtr(_) => Some(8), // 64-bit pointer
            Type::Never => Some(0),
            _ => None,
        }
    }

    /// Alignment in bytes of a type (matches LLVM's ABI alignment on x86-64).
    pub(super) fn llvm_align_of(&self, ty: &Type) -> u64 {
        match ty {
            Type::Bool | Type::I8 | Type::U8 => 1,
            Type::I16 | Type::U16 => 2,
            Type::I32 | Type::U32 | Type::F32 => 4,
            // i64, f64, ptr, str, slice all align to 8
            _ => 8,
        }
    }
}
