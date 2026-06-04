//! Single-source registry for already-`extern "C"` builtins (R1d slice 1).
//!
//! Each [`ExternSig`] row replaces a hand-written `declare_builtins` block of
//! the form "declare the `__axon_*` symbol, insert into `self.functions`,
//! insert into `self.fn_return_types`". `declare_builtins` iterates this table
//! and calls [`Codegen::declare_one_extern`] for each row, which builds the
//! `fn_type` from the row's param/ret shapes and replicates those exact
//! inserts. See `governance/specs/R1d-single-source-builtins.md`.
//!
//! Scope (slice 1): ONLY the straight `declare → link` externs — the ones whose
//! original block was nothing but a get-or-`add_function` plus the two inserts.
//! Builtins with bespoke call-site lowering (`to_str`, the arr_*/dict_get/set/
//! remove/keys inline loops, `str_slice`'s out-param wrapper, …) stay as
//! emit_call special-cases and are NOT in this table.

use inkwell::types::{BasicMetadataTypeEnum, BasicType};

use crate::types::Type;

/// LLVM shape of one builtin parameter or return slot.
///
/// Resolved to a concrete `BasicMetadataTypeEnum` / return type by
/// [`Codegen::declare_one_extern`]. Covers the by-value-scalar + str-struct +
/// opaque-pointer shapes that the slice-1 externs already use.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum L {
    /// `i64`
    I64,
    /// `i32`
    I32,
    /// `f64`
    F64,
    /// `i1` (bool)
    I1,
    /// Axon `str` = `{ i64 len, i8* data }` struct, passed/returned by value.
    Str,
    /// Opaque `i8*` (e.g. a Dict handle).
    Ptr,
    /// `void` — return slot only. No slice-1 row uses it yet (the void-returning
    /// dict_set has bespoke lowering), but it's part of the shape vocabulary the
    /// later batches will need.
    #[allow(dead_code)]
    Void,
}

/// Semantic return `Type` to record in `fn_return_types`.
///
/// A const-friendly stand-in for [`Type`] (which can't be built in a `const`
/// because of its `String` payloads). Mapped back to a real `Type` by
/// [`SemRet::to_type`].
#[derive(Clone, Copy, Debug)]
pub(super) enum SemRet {
    I64,
    I32,
    F64,
    Bool,
    /// `Type::Deferred("Dict")` — the opaque Dict handle.
    DictHandle,
}

impl SemRet {
    pub(super) fn to_type(self) -> Type {
        match self {
            SemRet::I64 => Type::I64,
            SemRet::I32 => Type::I32,
            SemRet::F64 => Type::F64,
            SemRet::Bool => Type::Bool,
            SemRet::DictHandle => Type::Deferred("Dict".to_string()),
        }
    }
}

/// One already-extern builtin: its axon-source name, its `__axon_*` symbol, the
/// LLVM param/ret shape, and its semantic return `Type`.
pub(super) struct ExternSig {
    /// Source-level builtin name (e.g. `"abs_i64"`). Documents the row and is
    /// the join key for the slice-3 drift cross-check; not read by slice 1.
    #[allow(dead_code)]
    pub axon_name: &'static str,
    /// The `extern "C"` symbol declared in the module (e.g. `"__axon_abs_i64"`).
    pub symbol: &'static str,
    /// LLVM parameter shapes, in order.
    pub params: &'static [L],
    /// LLVM return shape.
    pub ret: L,
    /// If `Some(key)`, insert the declared `FunctionValue` into
    /// `self.functions[key]`. Most rows use `Some(axon_name)`; rows whose call
    /// site resolves via the bare `__axon_*` symbol (the dict scalars) use
    /// `None` to match the original blocks exactly (no `functions` insert).
    pub fn_key: Option<&'static str>,
    /// If `Some((key, sem))`, insert `self.fn_return_types[key] = sem.to_type()`.
    pub ret_type: Option<(&'static str, SemRet)>,
}

/// The single-source table of straight `declare → link` externs (R1d slice 1).
///
/// Adding an already-extern builtin = one row here (plus the axon-rt fn + a
/// parity case), instead of a hand-written `declare_builtins` block.
pub(super) const BUILTIN_EXTERNS: &[ExternSig] = &[
    // ── math scalars (migrated to axon-rt in R1 Batch 1/3) ──────────────────
    ExternSig { axon_name: "abs_i64",  symbol: "__axon_abs_i64",  params: &[L::I64],          ret: L::I64, fn_key: Some("abs_i64"),  ret_type: Some(("abs_i64",  SemRet::I64)) },
    ExternSig { axon_name: "abs_i32",  symbol: "__axon_abs_i32",  params: &[L::I32],          ret: L::I32, fn_key: Some("abs_i32"),  ret_type: Some(("abs_i32",  SemRet::I32)) },
    ExternSig { axon_name: "abs_f64",  symbol: "__axon_abs_f64",  params: &[L::F64],          ret: L::F64, fn_key: Some("abs_f64"),  ret_type: Some(("abs_f64",  SemRet::F64)) },
    ExternSig { axon_name: "sign_i64", symbol: "__axon_sign_i64", params: &[L::I64],          ret: L::I64, fn_key: Some("sign_i64"), ret_type: Some(("sign_i64", SemRet::I64)) },
    ExternSig { axon_name: "pow_i64",  symbol: "__axon_pow_i64",  params: &[L::I64, L::I64],  ret: L::I64, fn_key: Some("pow_i64"),  ret_type: Some(("pow_i64",  SemRet::I64)) },
    ExternSig { axon_name: "min_i64",  symbol: "__axon_min_i64",  params: &[L::I64, L::I64],  ret: L::I64, fn_key: Some("min_i64"),  ret_type: Some(("min_i64",  SemRet::I64)) },
    ExternSig { axon_name: "max_i64",  symbol: "__axon_max_i64",  params: &[L::I64, L::I64],  ret: L::I64, fn_key: Some("max_i64"),  ret_type: Some(("max_i64",  SemRet::I64)) },
    ExternSig { axon_name: "min_i32",  symbol: "__axon_min_i32",  params: &[L::I32, L::I32],  ret: L::I32, fn_key: Some("min_i32"),  ret_type: Some(("min_i32",  SemRet::I32)) },
    ExternSig { axon_name: "max_i32",  symbol: "__axon_max_i32",  params: &[L::I32, L::I32],  ret: L::I32, fn_key: Some("max_i32"),  ret_type: Some(("max_i32",  SemRet::I32)) },
    ExternSig { axon_name: "clamp_i64", symbol: "__axon_clamp_i64", params: &[L::I64, L::I64, L::I64], ret: L::I64, fn_key: Some("clamp_i64"), ret_type: Some(("clamp_i64", SemRet::I64)) },
    ExternSig { axon_name: "clamp_f64", symbol: "__axon_clamp_f64", params: &[L::F64, L::F64, L::F64], ret: L::F64, fn_key: Some("clamp_f64"), ret_type: Some(("clamp_f64", SemRet::F64)) },

    // ── str predicates / scalars (migrated in R1 Batch 2) ───────────────────
    ExternSig { axon_name: "str_contains",    symbol: "__axon_str_contains",    params: &[L::Str, L::Str], ret: L::I1,  fn_key: Some("str_contains"),    ret_type: Some(("str_contains",    SemRet::Bool)) },
    ExternSig { axon_name: "str_starts_with", symbol: "__axon_str_starts_with", params: &[L::Str, L::Str], ret: L::I1,  fn_key: Some("str_starts_with"), ret_type: Some(("str_starts_with", SemRet::Bool)) },
    ExternSig { axon_name: "str_ends_with",   symbol: "__axon_str_ends_with",   params: &[L::Str, L::Str], ret: L::I1,  fn_key: Some("str_ends_with"),   ret_type: Some(("str_ends_with",   SemRet::Bool)) },
    ExternSig { axon_name: "str_index_of",    symbol: "__axon_str_index_of",    params: &[L::Str, L::Str], ret: L::I64, fn_key: Some("str_index_of"),    ret_type: Some(("str_index_of",    SemRet::I64)) },
    ExternSig { axon_name: "char_at",         symbol: "__axon_char_at",         params: &[L::Str, L::I64], ret: L::I64, fn_key: Some("char_at"),         ret_type: Some(("char_at",         SemRet::I64)) },
    ExternSig { axon_name: "str_len",         symbol: "__axon_str_len",         params: &[L::Str],         ret: L::I64, fn_key: Some("str_len"),         ret_type: Some(("str_len",         SemRet::I64)) },

    // ── dict scalars (R1c) ──────────────────────────────────────────────────
    // These resolve at the call site via the bare `__axon_*` symbol
    // (`self.functions.get(...).or_else(module.get_function(...))`), so the
    // original blocks did NOT insert into `self.functions` — fn_key is None to
    // replicate that exactly. dict_get/set/remove/keys keep their bespoke
    // out-param lowering and are intentionally NOT in this table.
    ExternSig { axon_name: "dict_new", symbol: "__axon_dict_new", params: &[],                ret: L::Ptr,  fn_key: None, ret_type: Some(("dict_new", SemRet::DictHandle)) },
    ExternSig { axon_name: "dict_has", symbol: "__axon_dict_has", params: &[L::Ptr, L::Str],  ret: L::I1,   fn_key: None, ret_type: Some(("dict_has", SemRet::Bool)) },
    ExternSig { axon_name: "dict_len", symbol: "__axon_dict_len", params: &[L::Ptr],          ret: L::I64,  fn_key: None, ret_type: Some(("dict_len", SemRet::I64)) },
    ExternSig { axon_name: "dict_inc", symbol: "__axon_dict_inc", params: &[L::Ptr, L::Str],  ret: L::I64,  fn_key: None, ret_type: Some(("dict_inc", SemRet::I64)) },
];

impl<'ctx> super::Codegen<'ctx> {
    /// Resolve an [`L`] shape to a `BasicMetadataTypeEnum` (param / by-value ret).
    fn l_basic(&self, l: L) -> BasicMetadataTypeEnum<'ctx> {
        let ctx = self.ir.context;
        match l {
            L::I64 => ctx.i64_type().into(),
            L::I32 => ctx.i32_type().into(),
            L::F64 => ctx.f64_type().into(),
            L::I1 => ctx.bool_type().into(),
            L::Ptr => ctx.i8_type().ptr_type(inkwell::AddressSpace::default()).into(),
            L::Str => {
                let i8_ptr = ctx.i8_type().ptr_type(inkwell::AddressSpace::default());
                ctx.struct_type(&[ctx.i64_type().into(), i8_ptr.into()], false).into()
            }
            L::Void => unreachable!("L::Void is a return-only shape"),
        }
    }

    /// Declare ONE already-extern builtin from a registry row, exactly
    /// replicating the hand-written block it replaces: build the `fn_type` from
    /// the row's param/ret shapes, idempotently get-or-`add_function` the
    /// `__axon_*` symbol, then perform the optional `self.functions` and
    /// `self.fn_return_types` inserts.
    pub(super) fn declare_one_extern(&mut self, row: &ExternSig) {
        let param_tys: Vec<BasicMetadataTypeEnum<'ctx>> =
            row.params.iter().map(|&l| self.l_basic(l)).collect();

        let fn_ty = match row.ret {
            L::Void => self.ir.context.void_type().fn_type(&param_tys, false),
            other => {
                // `BasicMetadataTypeEnum` → `BasicTypeEnum` for the return slot.
                let ret_basic = self.l_basic(other);
                let ret: inkwell::types::BasicTypeEnum<'ctx> = match ret_basic {
                    BasicMetadataTypeEnum::IntType(t) => t.as_basic_type_enum(),
                    BasicMetadataTypeEnum::FloatType(t) => t.as_basic_type_enum(),
                    BasicMetadataTypeEnum::PointerType(t) => t.as_basic_type_enum(),
                    BasicMetadataTypeEnum::StructType(t) => t.as_basic_type_enum(),
                    _ => unreachable!("unsupported extern return shape: {:?}", row.ret),
                };
                ret.fn_type(&param_tys, false)
            }
        };

        let fn_val = self
            .ir
            .module
            .get_function(row.symbol)
            .unwrap_or_else(|| self.ir.module.add_function(row.symbol, fn_ty, None));

        if let Some(key) = row.fn_key {
            self.functions.insert(key.to_string(), fn_val);
        }
        if let Some((key, sem)) = row.ret_type {
            self.fn_return_types.insert(key.to_string(), sem.to_type());
        }
    }

    /// Declare every row in [`BUILTIN_EXTERNS`] (R1d slice 1).
    pub(super) fn declare_builtin_externs(&mut self) {
        for row in BUILTIN_EXTERNS {
            self.declare_one_extern(row);
        }
    }
}
