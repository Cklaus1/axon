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
    /// `void` — return slot only (e.g. `sleep_ms(i64) -> ()`). Paired with
    /// `SemRet::Unit`. The void-returning dict_set keeps its bespoke call-site
    /// lowering and is NOT a row.
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
    /// `Type::Unit` — the `()` return of a void extern (e.g. `sleep_ms`).
    Unit,
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
            SemRet::Unit => Type::Unit,
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
    ExternSig {
        axon_name: "abs_i64",
        symbol: "__axon_abs_i64",
        params: &[L::I64],
        ret: L::I64,
        fn_key: Some("abs_i64"),
        ret_type: Some(("abs_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "abs_i32",
        symbol: "__axon_abs_i32",
        params: &[L::I32],
        ret: L::I32,
        fn_key: Some("abs_i32"),
        ret_type: Some(("abs_i32", SemRet::I32)),
    },
    ExternSig {
        axon_name: "abs_f64",
        symbol: "__axon_abs_f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("abs_f64"),
        ret_type: Some(("abs_f64", SemRet::F64)),
    },
    ExternSig {
        axon_name: "sign_i64",
        symbol: "__axon_sign_i64",
        params: &[L::I64],
        ret: L::I64,
        fn_key: Some("sign_i64"),
        ret_type: Some(("sign_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "pow_i64",
        symbol: "__axon_pow_i64",
        params: &[L::I64, L::I64],
        ret: L::I64,
        fn_key: Some("pow_i64"),
        ret_type: Some(("pow_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "min_i64",
        symbol: "__axon_min_i64",
        params: &[L::I64, L::I64],
        ret: L::I64,
        fn_key: Some("min_i64"),
        ret_type: Some(("min_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "max_i64",
        symbol: "__axon_max_i64",
        params: &[L::I64, L::I64],
        ret: L::I64,
        fn_key: Some("max_i64"),
        ret_type: Some(("max_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "min_i32",
        symbol: "__axon_min_i32",
        params: &[L::I32, L::I32],
        ret: L::I32,
        fn_key: Some("min_i32"),
        ret_type: Some(("min_i32", SemRet::I32)),
    },
    ExternSig {
        axon_name: "max_i32",
        symbol: "__axon_max_i32",
        params: &[L::I32, L::I32],
        ret: L::I32,
        fn_key: Some("max_i32"),
        ret_type: Some(("max_i32", SemRet::I32)),
    },
    ExternSig {
        axon_name: "clamp_i64",
        symbol: "__axon_clamp_i64",
        params: &[L::I64, L::I64, L::I64],
        ret: L::I64,
        fn_key: Some("clamp_i64"),
        ret_type: Some(("clamp_i64", SemRet::I64)),
    },
    ExternSig {
        axon_name: "clamp_f64",
        symbol: "__axon_clamp_f64",
        params: &[L::F64, L::F64, L::F64],
        ret: L::F64,
        fn_key: Some("clamp_f64"),
        ret_type: Some(("clamp_f64", SemRet::F64)),
    },
    // ── str predicates / scalars (migrated in R1 Batch 2) ───────────────────
    ExternSig {
        axon_name: "str_contains",
        symbol: "__axon_str_contains",
        params: &[L::Str, L::Str],
        ret: L::I1,
        fn_key: Some("str_contains"),
        ret_type: Some(("str_contains", SemRet::Bool)),
    },
    ExternSig {
        axon_name: "str_starts_with",
        symbol: "__axon_str_starts_with",
        params: &[L::Str, L::Str],
        ret: L::I1,
        fn_key: Some("str_starts_with"),
        ret_type: Some(("str_starts_with", SemRet::Bool)),
    },
    ExternSig {
        axon_name: "str_ends_with",
        symbol: "__axon_str_ends_with",
        params: &[L::Str, L::Str],
        ret: L::I1,
        fn_key: Some("str_ends_with"),
        ret_type: Some(("str_ends_with", SemRet::Bool)),
    },
    ExternSig {
        axon_name: "str_index_of",
        symbol: "__axon_str_index_of",
        params: &[L::Str, L::Str],
        ret: L::I64,
        fn_key: Some("str_index_of"),
        ret_type: Some(("str_index_of", SemRet::I64)),
    },
    ExternSig {
        axon_name: "char_at",
        symbol: "__axon_char_at",
        params: &[L::Str, L::I64],
        ret: L::I64,
        fn_key: Some("char_at"),
        ret_type: Some(("char_at", SemRet::I64)),
    },
    ExternSig {
        axon_name: "str_len",
        symbol: "__axon_str_len",
        params: &[L::Str],
        ret: L::I64,
        fn_key: Some("str_len"),
        ret_type: Some(("str_len", SemRet::I64)),
    },
    // str_count was previously miscategorized as one of the str-returning out-param wrapper
    // builtins (R1d spec's own "exhaustive scan") purely by name resemblance to str_replace/
    // str_slice/etc — its actual codegen never used the out-param dance at all: a straight
    // `__axon_str_count(AxonStr, AxonStr) -> i64` call, same shape as str_index_of right above.
    // Found while sizing "extend ExternSig for out-param synthesis" (governance/specs/
    // R1d-single-source-builtins.md) and migrating it here instead — a real, if small,
    // simple-batch candidate the spec's own scan missed, not new registry capability.
    ExternSig {
        axon_name: "str_count",
        symbol: "__axon_str_count",
        params: &[L::Str, L::Str],
        ret: L::I64,
        fn_key: Some("str_count"),
        ret_type: Some(("str_count", SemRet::I64)),
    },
    // ── dict scalars (R1c) ──────────────────────────────────────────────────
    // These resolve at the call site via the bare `__axon_*` symbol
    // (`self.functions.get(...).or_else(module.get_function(...))`), so the
    // original blocks did NOT insert into `self.functions` — fn_key is None to
    // replicate that exactly. dict_get/set/remove/keys keep their bespoke
    // out-param lowering and are intentionally NOT in this table.
    ExternSig {
        axon_name: "dict_new",
        symbol: "__axon_dict_new",
        params: &[],
        ret: L::Ptr,
        fn_key: None,
        ret_type: Some(("dict_new", SemRet::DictHandle)),
    },
    ExternSig {
        axon_name: "dict_has",
        symbol: "__axon_dict_has",
        params: &[L::Ptr, L::Str],
        ret: L::I1,
        fn_key: None,
        ret_type: Some(("dict_has", SemRet::Bool)),
    },
    ExternSig {
        axon_name: "dict_len",
        symbol: "__axon_dict_len",
        params: &[L::Ptr],
        ret: L::I64,
        fn_key: None,
        ret_type: Some(("dict_len", SemRet::I64)),
    },
    ExternSig {
        axon_name: "dict_inc",
        symbol: "__axon_dict_inc",
        params: &[L::Ptr, L::Str],
        ret: L::I64,
        fn_key: None,
        ret_type: Some(("dict_inc", SemRet::I64)),
    },
    // dict_merge(d1, d2) → a fresh Dict handle (d2 wins conflicts). Both args +
    // result are opaque i8* handles, so it's a plain registry row like dict_new.
    ExternSig {
        axon_name: "dict_merge",
        symbol: "__axon_dict_merge",
        params: &[L::Ptr, L::Ptr],
        ret: L::Ptr,
        fn_key: Some("dict_merge"),
        ret_type: Some(("dict_merge", SemRet::DictHandle)),
    },
    // ── time builtins (Phase 4) ─────────────────────────────────────────────
    ExternSig {
        axon_name: "sleep_ms",
        symbol: "__axon_sleep_ms",
        params: &[L::I64],
        ret: L::Void,
        fn_key: Some("sleep_ms"),
        ret_type: Some(("sleep_ms", SemRet::Unit)),
    },
    ExternSig {
        axon_name: "now_ms",
        symbol: "__axon_now_ms",
        params: &[],
        ret: L::I64,
        fn_key: Some("now_ms"),
        ret_type: Some(("now_ms", SemRet::I64)),
    },
    // ── f64 math intrinsics (Phase 3; R1d slice 2 batch) ────────────────────
    // LLVM intrinsics lowered to C libm (`llvm.*.f64`), not axon-rt externs —
    // `symbol` is just the declared function name either way, so the same row
    // shape covers both. `sqrt`/`floor`/`ceil` are also reused directly (by
    // `self.functions.get_function`) from the sqrt_f64/floor_f64/ceil_f64
    // wrapper builtins later in `declare_builtins`; that reuse still works
    // because `declare_builtin_externs()` (this table) runs first (see the
    // top of `declare_builtins`), so those lookups always find an existing
    // declaration to reuse rather than redeclaring.
    ExternSig {
        axon_name: "sqrt",
        symbol: "llvm.sqrt.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("sqrt"),
        ret_type: Some(("sqrt", SemRet::F64)),
    },
    ExternSig {
        axon_name: "pow",
        symbol: "llvm.pow.f64",
        params: &[L::F64, L::F64],
        ret: L::F64,
        fn_key: Some("pow"),
        ret_type: Some(("pow", SemRet::F64)),
    },
    ExternSig {
        axon_name: "floor",
        symbol: "llvm.floor.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("floor"),
        ret_type: Some(("floor", SemRet::F64)),
    },
    ExternSig {
        axon_name: "ceil",
        symbol: "llvm.ceil.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("ceil"),
        ret_type: Some(("ceil", SemRet::F64)),
    },
    // exp / ln / log10 — the transcendental trio, matching the interpreter's
    // Rust f64::{exp,ln,log10} (which call the same libm), so native==interp.
    // Axon `ln` = natural log = `llvm.log.f64` (C `log`); `log10` is the
    // base-10 intrinsic.
    ExternSig {
        axon_name: "exp",
        symbol: "llvm.exp.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("exp"),
        ret_type: Some(("exp", SemRet::F64)),
    },
    ExternSig {
        axon_name: "ln",
        symbol: "llvm.log.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("ln"),
        ret_type: Some(("ln", SemRet::F64)),
    },
    ExternSig {
        axon_name: "log10",
        symbol: "llvm.log10.f64",
        params: &[L::F64],
        ret: L::F64,
        fn_key: Some("log10"),
        ret_type: Some(("log10", SemRet::F64)),
    },
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
            L::Ptr => ctx
                .i8_type()
                .ptr_type(inkwell::AddressSpace::default())
                .into(),
            L::Str => {
                let i8_ptr = ctx.i8_type().ptr_type(inkwell::AddressSpace::default());
                ctx.struct_type(&[ctx.i64_type().into(), i8_ptr.into()], false)
                    .into()
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

// ── R1d slice 3: drift cross-check ──────────────────────────────────────────
// `axon_name` is the join key against `crate::builtins::BUILTINS` (see its
// doc comment above). These tests are the "so the two tables can't drift"
// promise from governance/specs/R1d-single-source-builtins.md §4 slice 3.
#[cfg(test)]
mod drift_tests {
    use super::BUILTIN_EXTERNS;
    use crate::builtins::BUILTINS;

    #[test]
    fn every_extern_row_matches_a_known_builtin_with_the_same_arity() {
        for row in BUILTIN_EXTERNS {
            let b = BUILTINS.iter().find(|b| b.name == row.axon_name).unwrap_or_else(|| {
                panic!(
                    "BUILTIN_EXTERNS row '{}' has no matching BUILTINS entry \
                     (renamed or removed without updating the registry — R1d slice 3 drift)",
                    row.axon_name
                )
            });
            assert_eq!(
                b.params.len(),
                row.params.len(),
                "BUILTIN_EXTERNS row '{}' declares {} LLVM param(s) but BUILTINS says {} \
                 source-level param(s) — signature drift (R1d slice 3)",
                row.axon_name,
                row.params.len(),
                b.params.len()
            );
        }
    }

    #[test]
    fn no_duplicate_extern_registry_rows() {
        let mut seen = std::collections::HashSet::new();
        for row in BUILTIN_EXTERNS {
            assert!(
                seen.insert(row.axon_name),
                "BUILTIN_EXTERNS has more than one row for '{}' — duplicate registration",
                row.axon_name
            );
        }
    }
}
