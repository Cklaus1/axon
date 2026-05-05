//! Axon semantic type → LLVM `BasicTypeEnum` lowering.
//!
//! Phase 2 of the §7.5 module split: extracts the three type-mapping helpers
//! from the monolithic `codegen.rs`.  All three are methods on
//! `super::Codegen<'ctx>`, so this file declares its own impl block on the
//! parent struct.  Visibility:
//!   * `llvm_type`        — `pub` (re-exported public API; many callers)
//!   * `llvm_sizeof`      — `pub(super)` (called from mod.rs internally)
//!   * `llvm_align_of`    — `pub(super)` (only called from `llvm_sizeof` today
//!                          but kept reachable from mod.rs in case of future use)
//!
//! No state is mutated by any of these methods — they are pure functions of
//! `&self` and the input `Type`.

use inkwell::types::{BasicType, BasicTypeEnum};
use inkwell::AddressSpace;

use crate::types::Type;

impl<'ctx> super::Codegen<'ctx> {
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
            // Uncertain<T> → struct { T value, f64 confidence, i64 source_tag }
            // V1 simplification: omits `alternatives` and `interval` slots from
            // the full PRD (AI_Language_Plan.md lines 1360-1410). Layer-1 only.
            Type::Uncertain(inner) => {
                let inner_llvm = self.llvm_type(inner)?;
                let f64_ty = self.context.f64_type();
                let i64_ty = self.context.i64_type();
                Some(
                    self.context
                        .struct_type(&[inner_llvm, f64_ty.into(), i64_ty.into()], false)
                        .into(),
                )
            }
            // Temporal<T> → struct { T value, f64 confidence, i64 horizon_ms,
            //                        f64 decay, i64 valid_until_ms }
            // V1 monomorphisation on T = i64 / f64 (PRD lines 1411-1467).
            Type::Temporal(inner) => {
                let inner_llvm = self.llvm_type(inner)?;
                let f64_ty = self.context.f64_type();
                let i64_ty = self.context.i64_type();
                Some(
                    self.context
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
                fields.iter().map(|f| self.llvm_sizeof(f).unwrap_or(0)).sum(),
            ),
            Type::Struct(_) | Type::Enum(_) => Some(8), // conservative
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
